//! Azure DevOps Services repository and pull request access. Requests stay within one organization.

use crate::endpoints::HostEndpoints;
use crate::http::{ChangeCounter, ClientConfig, HttpClient, REQUEST_TIMEOUT};
use crate::time::{parse_rfc3339, unix_now};
use futures_util::future::try_join_all;
use gitnexia_core::avatar::AvatarPayload;
use gitnexia_core::error::{GbError, GbErrorKind};
use gitnexia_core::forge::*;
use gitnexia_core::redact::Secret;
use serde::Deserialize;
use serde_json::{Value, json};

const REVIEW_FILE_LIMIT: usize = 1024 * 1024;
const CHANGE_PAGE_SIZE: usize = 2000;

pub struct AzureDevOpsProvider {
    host: String,
    http: HttpClient,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    id: String,
    provider_display_name: String,
    #[serde(default)]
    is_active: bool,
    properties: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    authenticated_user: Identity,
}

#[derive(Deserialize)]
struct List<T> {
    value: Vec<T>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IterationList {
    Bare(Vec<Value>),
    Wrapped(List<Value>),
}

#[derive(Deserialize)]
struct Project {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    id: String,
    name: String,
    project: Project,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    is_disabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IterationChanges {
    change_entries: Vec<Value>,
    #[serde(default)]
    next_skip: usize,
    #[serde(default)]
    next_top: usize,
}

struct ReviewSnapshot {
    iteration: u64,
    refs: DiffRefs,
    changes: Vec<Value>,
}

fn guid(id: &str) -> Result<String, GbError> {
    let bytes = id.as_bytes();
    if bytes.len() != 36
        || !bytes.iter().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
        || id.bytes().all(|b| matches!(b, b'0' | b'-'))
    {
        return Err(GbError::other("Azure DevOps returned an invalid identity"));
    }
    Ok(id.to_ascii_lowercase())
}

fn segment(s: &str) -> Result<String, GbError> {
    if s.is_empty()
        || s == "."
        || s == ".."
        || s.contains(['/', '\\'])
        || s.chars().any(char::is_control)
    {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "Expected an Azure DevOps project and repository name",
        ));
    }
    Ok(crate::http::encode_component(s))
}

fn identity(v: &Value, host: &str) -> Option<ForgeUser> {
    let id = guid(v["id"].as_str()?).ok()?;
    let username = v["uniqueName"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(&id)
        .to_owned();
    let name = v["displayName"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(&username)
        .to_owned();
    Some(ForgeUser {
        id: ForgeId::Text(id),
        username,
        name,
        avatar_url: None,
        web_url: format!("https://{host}/_usersSettings/profile"),
        email: None,
        emails: Vec::new(),
    })
}

fn branch_name(v: &Value) -> Option<String> {
    v.as_str()?.strip_prefix("refs/heads/").map(str::to_owned)
}

fn timestamp(v: &Value) -> Option<i64> {
    parse_rfc3339(v.as_str()?)
}

fn review_path(raw: &str) -> Result<String, GbError> {
    let path = raw.strip_prefix('/').unwrap_or(raw);
    if path.is_empty()
        || path.contains(['\\', '\0'])
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(GbError::other(
            "Azure DevOps returned an invalid pull request file path",
        ));
    }
    Ok(path.to_owned())
}

fn diff_lines(old: &str, new: &str) -> Vec<ReviewLine> {
    let patch = similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .to_string();
    gitnexia_core::forge::review::commentable_lines(&patch)
}

fn position_point(line: Option<u32>) -> Value {
    line.map_or(Value::Null, |line| json!({"line": line, "offset": 1}))
}

fn position_bounds(start: Option<u32>, end: Option<u32>) -> (Value, Value) {
    match (start, end) {
        (Some(start), Some(end)) => (position_point(Some(start)), position_point(Some(end))),
        (Some(line), None) | (None, Some(line)) => {
            (position_point(Some(line)), position_point(Some(line)))
        }
        (None, None) => (Value::Null, Value::Null),
    }
}

fn thread_position(v: &Value) -> Option<DiffPosition> {
    let context = &v["threadContext"];
    let path = review_path(context["filePath"].as_str()?).ok()?;
    let left_start = context["leftFileStart"]["line"].as_u64().map(|n| n as u32);
    let left_end = context["leftFileEnd"]["line"].as_u64().map(|n| n as u32);
    let right_start = context["rightFileStart"]["line"].as_u64().map(|n| n as u32);
    let right_end = context["rightFileEnd"]["line"].as_u64().map(|n| n as u32);
    if left_start.is_none() && left_end.is_none() && right_start.is_none() && right_end.is_none() {
        return None;
    }
    Some(DiffPosition {
        path: path.clone(),
        old_path: Some(path),
        line: right_end,
        old_line: left_end,
        snippet: None,
        start_line: right_start,
        start_old_line: left_start,
        head_sha: None,
        outdated: v["pullRequestThreadContext"]["isOutdated"].as_bool() == Some(true),
    })
}

fn pull_request(v: &Value, project: &ForgeProject, host: &str) -> Result<ForgeMr, GbError> {
    let number = v["pullRequestId"]
        .as_u64()
        .ok_or_else(|| GbError::other(format!("{host} sent an unreadable pull request")))?;
    let raw_status = v["status"].as_str().unwrap_or_default();
    let raw_merge_status = v["mergeStatus"].as_str().unwrap_or_default();
    let state = match raw_status {
        "active" if v["isDraft"].as_bool() == Some(true) => MrState::Draft,
        "active" => MrState::Open,
        "completed" if raw_merge_status == "queued" => MrState::Merging,
        "completed" => MrState::Merged,
        "abandoned" => MrState::Closed,
        _ => {
            return Err(GbError::other(format!(
                "{host} sent an unreadable pull request"
            )));
        }
    };
    let author = identity(&v["createdBy"], host)
        .ok_or_else(|| GbError::other(format!("{host} sent an unreadable pull request author")))?;
    let title = v["title"]
        .as_str()
        .ok_or_else(|| GbError::other(format!("{host} sent an unreadable pull request title")))?;
    let reviews: Vec<ForgeReview> = v["reviewers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let vote = r["vote"].as_i64().unwrap_or_default();
            Some(ForgeReview {
                user: identity(r, host)?,
                state: if vote >= 5 {
                    ReviewState::Approved
                } else if vote <= -5 {
                    ReviewState::ChangesRequested
                } else {
                    ReviewState::Pending
                },
                submitted_at: None,
                requested: vote == 0,
            })
        })
        .collect();
    let approvals = reviews
        .iter()
        .filter(|r| r.state == ReviewState::Approved)
        .count() as u32;
    let decision = if reviews
        .iter()
        .any(|r| r.state == ReviewState::ChangesRequested)
    {
        ReviewDecision::ChangesRequested
    } else if approvals > 0 {
        ReviewDecision::Approved
    } else if reviews.is_empty() {
        ReviewDecision::None
    } else {
        ReviewDecision::ReviewRequired
    };
    let source_project = v["forkSource"]["repository"]["project"]["name"]
        .as_str()
        .zip(v["forkSource"]["repository"]["name"].as_str())
        .map(|(project, repo)| format!("{project}/{repo}"))
        .unwrap_or_else(|| project.path.clone());
    let target_branch = branch_name(&v["targetRefName"])
        .ok_or_else(|| GbError::other(format!("{host} sent an unreadable pull request target")))?;
    let source_branch = branch_name(&v["sourceRefName"])
        .ok_or_else(|| GbError::other(format!("{host} sent an unreadable pull request source")))?;
    let labels = v["labels"].as_array().cloned().unwrap_or_default();
    let (labels, label_colors) = labels_of(&Value::Array(labels));
    let updated_at = [
        timestamp(&v["creationDate"]),
        timestamp(&v["closedDate"]),
        timestamp(&v["completionQueueTime"]),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);

    Ok(ForgeMr {
        number,
        title: title.to_owned(),
        state,
        author,
        source_project,
        source_branch,
        target_project: project.path.clone(),
        target_branch,
        head_sha: v["lastMergeSourceCommit"]["commitId"]
            .as_str()
            .map(str::to_owned),
        web_url: format!(
            "{}/pullrequest/{number}",
            project.web_url.trim_end_matches('/')
        ),
        pipeline: None,
        review: ReviewSummary {
            decision,
            approvals,
            approvals_required: None,
            reviews,
        },
        conflicts: match raw_merge_status {
            "conflicts" => Some(true),
            "succeeded" => Some(false),
            _ => None,
        },
        labels,
        label_colors,
        updated_at,
        stacked: v["description"]
            .as_str()
            .is_some_and(gitnexia_core::forge::stack::carries_stack_table),
        auto_merge: None,
    })
}

fn pull_request_detail(
    v: &Value,
    project: &ForgeProject,
    host: &str,
) -> Result<ForgeMrDetail, GbError> {
    let mr = pull_request(v, project, host)?;
    let merge_status = if mr.state == MrState::Merged || mr.state == MrState::Closed {
        MergeStatus::Blocked {
            reason: "This pull request is no longer open".into(),
        }
    } else {
        match v["mergeStatus"].as_str() {
            Some("succeeded") => MergeStatus::Mergeable,
            Some("conflicts") => MergeStatus::Blocked {
                reason: "This pull request has merge conflicts".into(),
            },
            Some("rejected") => MergeStatus::Blocked {
                reason: v["mergeFailureMessage"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Azure DevOps rejected the merge")
                    .to_owned(),
            },
            Some("queued") => MergeStatus::Blocked {
                reason: "A merge is already in progress".into(),
            },
            _ => MergeStatus::Checking,
        }
    };
    let reviewers = mr.review.reviews.iter().map(|r| r.user.clone()).collect();
    Ok(ForgeMrDetail {
        mr,
        description: v["description"].as_str().unwrap_or_default().to_owned(),
        reviewers,
        assignees: Vec::new(),
        merge_status,
        squash: v["completionOptions"]["mergeStrategy"]
            .as_str()
            .map(|s| s.eq_ignore_ascii_case("squash"))
            .or_else(|| v["completionOptions"]["squashMerge"].as_bool()),
        delete_source_branch: v["completionOptions"]["deleteSourceBranch"].as_bool(),
        base_sha: v["lastMergeTargetCommit"]["commitId"]
            .as_str()
            .map(str::to_owned),
        subscribed: None,
        body_html: None,
        update: None,
        merged_sha: v["lastMergeCommit"]["commitId"].as_str().map(str::to_owned),
    })
}

fn azure_note(v: &Value, host: &str, position: Option<DiffPosition>) -> Option<ForgeNote> {
    Some(ForgeNote {
        id: v["id"].as_i64()?.to_string(),
        author: identity(&v["author"], host)?,
        body: v["content"].as_str()?.to_owned(),
        created_at: timestamp(&v["publishedDate"]).unwrap_or(0),
        system: v["commentType"].as_str() == Some("system"),
        position,
        body_html: None,
        reactions: Vec::new(),
        web_url: None,
    })
}

fn discussion(v: &Value, host: &str) -> Option<ForgeDiscussion> {
    let id = v["id"].as_i64()?.to_string();
    let position = thread_position(v);
    let notes = v["comments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["isDeleted"].as_bool() != Some(true))
        .filter_map(|c| azure_note(c, host, position.clone()))
        .collect();
    let resolved = matches!(
        v["status"].as_str(),
        Some("fixed" | "wontFix" | "closed" | "byDesign")
    ) || matches!(v["status"].as_i64(), Some(2..=5));
    Some(ForgeDiscussion {
        id,
        notes,
        resolvable: false,
        resolved,
        resolved_by: None,
        resolved_at: None,
    })
}

impl AzureDevOpsProvider {
    pub fn new(
        host: &str,
        endpoints: &HostEndpoints,
        token: Secret,
        changes: Option<ChangeCounter>,
    ) -> Self {
        let http = HttpClient::new(ClientConfig {
            host: host.into(),
            api_base: endpoints.api.clone(),
            token: Some(token),
            headers: vec![("Accept", "application/json")],
            timeout: REQUEST_TIMEOUT,
        })
        .with_basic_auth();
        let http = match changes {
            Some(c) => http.with_change_counter(c),
            None => http,
        };
        Self {
            host: host.into(),
            http,
        }
    }

    async fn user(&self) -> Result<ForgeUser, GbError> {
        let response = self
            .http
            .get("/_apis/connectionData?api-version=7.1-preview.1")
            .await?;
        if response.status != 200 {
            return Err(GbError::new(
                GbErrorKind::AuthFailed,
                "Azure DevOps did not authenticate this token",
            ));
        }
        let c: Connection = response.json(&self.host)?;
        if !c.authenticated_user.is_active {
            return Err(GbError::new(
                GbErrorKind::AuthFailed,
                "Azure DevOps did not identify an active token user",
            ));
        }
        let id = guid(&c.authenticated_user.id)?;
        let username = c.authenticated_user.properties["Account"]["$value"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&id)
            .to_owned();
        Ok(ForgeUser {
            id: ForgeId::Text(id),
            username,
            name: c.authenticated_user.provider_display_name,
            avatar_url: None,
            web_url: format!("https://{}/_usersSettings/profile", self.host),
            email: None,
            emails: Vec::new(),
        })
    }

    fn repository(&self, r: Repository) -> Result<ForgeProject, GbError> {
        let project = segment(&r.project.name)?;
        let repo = segment(&r.name)?;
        let org = self
            .host
            .strip_prefix("dev.azure.com/")
            .ok_or_else(|| GbError::other("Azure DevOps account has no organization"))?;
        let https = format!("https://{}/{project}/_git/{repo}", self.host);
        Ok(ForgeProject {
            kind: ForgeKind::AzureDevOps,
            id: ForgeId::Text(guid(&r.id)?),
            host: self.host.clone(),
            path: format!("{}/{}", r.project.name, r.name),
            name: r.name,
            owner: r.project.name,
            web_url: https.clone(),
            default_branch: r
                .default_branch
                .map(|b| b.strip_prefix("refs/heads/").unwrap_or(&b).to_owned()),
            clone_https: https,
            clone_ssh: format!("git@ssh.dev.azure.com:v3/{org}/{project}/{repo}"),
            fork_of: None,
            updated_at: None,
            archived: r.is_disabled,
            owner_avatar_url: None,
        })
    }

    async fn repositories(&self) -> Result<Vec<ForgeProject>, GbError> {
        // This API returns the accessible repositories across all projects, without paging.
        let list: List<Repository> = self
            .http
            .get("/_apis/git/repositories?api-version=7.1")
            .await?
            .json(&self.host)?;
        list.value
            .into_iter()
            .filter(|r| !r.is_disabled)
            .map(|r| self.repository(r))
            .collect()
    }

    fn repository_url(&self, project: &ForgeProject) -> Result<String, GbError> {
        if project.kind != ForgeKind::AzureDevOps || project.host != self.host {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "The pull request repository must be in this Azure DevOps organization",
            ));
        }
        let ForgeId::Text(id) = &project.id else {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "Azure DevOps repository IDs must be GUIDs",
            ));
        };
        Ok(format!(
            "/{}/_apis/git/repositories/{}",
            segment(&project.owner)?,
            guid(id)?
        ))
    }

    fn pull_request_url(&self, project: &ForgeProject, number: u64) -> Result<String, GbError> {
        Ok(format!(
            "{}/pullrequests/{number}",
            self.repository_url(project)?
        ))
    }

    async fn get_pull_request(
        &self,
        project: &ForgeProject,
        number: u64,
    ) -> Result<crate::http::HttpResponse, GbError> {
        self.http
            .get(&format!(
                "{}?api-version=7.1",
                self.pull_request_url(project, number)?
            ))
            .await
    }

    async fn review_iteration(
        &self,
        project: &ForgeProject,
        number: u64,
    ) -> Result<(u64, DiffRefs), GbError> {
        let response = self
            .http
            .get(&format!(
                "{}/iterations?api-version=7.1",
                self.pull_request_url(project, number)?
            ))
            .await?;
        let iterations = match response.json(&self.host)? {
            IterationList::Bare(iterations) => iterations,
            IterationList::Wrapped(list) => list.value,
        };
        let (id, latest) = iterations
            .iter()
            .filter_map(|v| v["id"].as_u64().map(|id| (id, v)))
            .max_by_key(|(id, _)| *id)
            .ok_or_else(|| {
                GbError::other(format!(
                    "Azure DevOps hasn't worked out pull request #{number}'s diff yet"
                ))
            })?;
        let commit = |key: &str| {
            latest[key]["commitId"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let refs = DiffRefs {
            base_sha: commit("commonRefCommit").ok_or_else(|| {
                GbError::other("Azure DevOps returned an iteration without its common commit")
            })?,
            start_sha: commit("targetRefCommit").ok_or_else(|| {
                GbError::other("Azure DevOps returned an iteration without its target commit")
            })?,
            head_sha: commit("sourceRefCommit").ok_or_else(|| {
                GbError::other("Azure DevOps returned an iteration without its source commit")
            })?,
        };
        Ok((id, refs))
    }

    async fn review_snapshot(
        &self,
        project: &ForgeProject,
        number: u64,
    ) -> Result<ReviewSnapshot, GbError> {
        let (iteration, refs) = self.review_iteration(project, number).await?;
        let base = format!(
            "{}/iterations/{iteration}/changes",
            self.pull_request_url(project, number)?
        );
        let mut changes = Vec::new();
        let mut skip = 0;
        loop {
            let response = self
                .http
                .get(&format!(
                    "{base}?$compareTo=0&$top={CHANGE_PAGE_SIZE}&$skip={skip}&api-version=7.1"
                ))
                .await?;
            let page: IterationChanges = response.json(&self.host)?;
            changes.extend(page.change_entries);
            if page.next_skip <= skip || page.next_top == 0 {
                break;
            }
            skip = page.next_skip;
        }
        Ok(ReviewSnapshot {
            iteration,
            refs,
            changes,
        })
    }

    async fn item_text(
        &self,
        project: &ForgeProject,
        path: &str,
        commit: &str,
    ) -> Result<Option<String>, GbError> {
        let response = self
            .http
            .get_as(
                &format!(
                    "{}/items?path={}&versionDescriptor.version={}&versionDescriptor.versionType=commit&api-version=7.1",
                    self.repository_url(project)?,
                    crate::http::encode_component(&format!("/{path}")),
                    crate::http::encode_component(commit)
                ),
                "text/plain",
            )
            .await?;
        if response.body.len() > REVIEW_FILE_LIMIT || response.body.contains(&0) {
            return Ok(None);
        }
        Ok(String::from_utf8(response.body.as_ref().clone()).ok())
    }

    async fn review_file(
        &self,
        project: &ForgeProject,
        snapshot: &ReviewSnapshot,
        change: &Value,
    ) -> Result<Option<ReviewFile>, GbError> {
        if change["item"]["isFolder"].as_bool() == Some(true) {
            return Ok(None);
        }
        let Some(raw_path) = change["item"]["path"].as_str() else {
            return Err(GbError::other(
                "Azure DevOps returned a pull request change without a file path",
            ));
        };
        let path = review_path(raw_path)?;
        let old_path = review_path(
            change["originalPath"]
                .as_str()
                .or(change["item"]["originalPath"].as_str())
                .unwrap_or(raw_path),
        )?;
        let change_types = change["changeType"].as_str().unwrap_or_default();
        let has_change_type = |expected: &str| {
            change_types
                .split(',')
                .any(|kind| kind.trim().eq_ignore_ascii_case(expected))
        };
        let added = has_change_type("add") || has_change_type("undelete");
        let deleted = has_change_type("delete");
        let old = if added {
            Some(String::new())
        } else {
            self.item_text(project, &old_path, &snapshot.refs.base_sha)
                .await?
        };
        let new = if deleted {
            Some(String::new())
        } else {
            self.item_text(project, &path, &snapshot.refs.head_sha)
                .await?
        };
        let too_large = old.is_none() || new.is_none();
        let lines = match (&old, &new) {
            (Some(old), Some(new)) => diff_lines(old, new),
            _ => Vec::new(),
        };
        Ok(Some(ReviewFile {
            path: if deleted { old_path.clone() } else { path },
            old_path,
            lines,
            too_large,
        }))
    }

    async fn review_files(
        &self,
        project: &ForgeProject,
        snapshot: &ReviewSnapshot,
    ) -> Result<Vec<ReviewFile>, GbError> {
        let mut files = Vec::new();
        for changes in snapshot.changes.chunks(crate::http::PARALLEL) {
            let batch = changes
                .iter()
                .map(|change| self.review_file(project, snapshot, change));
            files.extend(try_join_all(batch).await?.into_iter().flatten());
        }
        Ok(files)
    }
}

impl ForgeProvider for AzureDevOpsProvider {
    fn kind(&self) -> ForgeKind {
        ForgeKind::AzureDevOps
    }
    fn host(&self) -> &str {
        &self.host
    }
    fn rate_limit(&self) -> RateLimitState {
        self.http.rate_limit()
    }
    fn expire_fresh(&self) {
        self.http.expire_fresh();
    }
    fn take_request_stats(&self) -> RequestStats {
        self.http.take_stats()
    }
    fn current_user(&self) -> ForgeFuture<'_, ForgeUser> {
        Box::pin(self.user())
    }
    fn check_token(&self) -> ForgeFuture<'_, TokenCheck> {
        Box::pin(async move {
            let user = self.user().await?;
            self.repositories().await?;
            Ok(TokenCheck {
                user,
                write: WriteAccess::Unknown,
            })
        })
    }
    fn version(&self) -> ForgeFuture<'_, Option<String>> {
        Box::pin(async { Ok(None) })
    }
    fn project<'a>(&'a self, path: &'a str) -> ForgeFuture<'a, Fresh<ForgeProject>> {
        Box::pin(async move {
            let (project, repo) = path.split_once('/').ok_or_else(|| {
                GbError::new(GbErrorKind::InvalidInput, "Expected project/repository")
            })?;
            let r = self
                .http
                .get(&format!(
                    "/{}/_apis/git/repositories/{}?api-version=7.1",
                    segment(project)?,
                    segment(repo)?
                ))
                .await?;
            let value = self.repository(r.json(&self.host)?)?;
            Ok(Fresh {
                value,
                not_modified: r.not_modified,
                poll_interval_secs: r.poll_interval_secs,
                fetched_at: unix_now(),
            })
        })
    }
    fn search_repos<'a>(&'a self, query: &'a str) -> ForgeFuture<'a, Vec<ForgeProject>> {
        Box::pin(async move {
            let q = query.trim().to_lowercase();
            let mut repos = self.repositories().await?;
            repos.retain(|r| r.path.to_lowercase().contains(&q));
            repos.sort_by_key(|r| r.path.to_lowercase());
            Ok(repos)
        })
    }
    fn project_settings<'a>(
        &'a self,
        _p: &'a ForgeProject,
    ) -> ForgeFuture<'a, ForgeProjectSettings> {
        Box::pin(async {
            Ok(ForgeProjectSettings {
                merge_methods: vec![
                    MergeMethod::Merge,
                    MergeMethod::Squash,
                    MergeMethod::Rebase,
                    MergeMethod::SemiLinear,
                ],
                squash: SquashOption::DefaultOff,
                delete_source_branch: false,
            })
        })
    }
    fn forks<'a>(&'a self, _p: &'a ForgeProject) -> ForgeFuture<'a, Vec<ForgeProject>> {
        unsupported("Azure DevOps forks")
    }
    fn avatar_for_email<'a>(&'a self, _email: &'a str) -> ForgeFuture<'a, Option<AvatarPayload>> {
        Box::pin(async { Ok(None) })
    }

    fn open_mrs_light<'a>(
        &'a self,
        project: &'a ForgeProject,
        filter: MrFilter,
    ) -> ForgeFuture<'a, Fresh<Vec<ForgeMr>>> {
        Box::pin(async move {
            let path = format!(
                "{}/pullrequests?searchCriteria.status=active&api-version=7.1",
                self.repository_url(project)?
            );
            let response = self.http.get(&path).await?;
            let list: List<Value> = response.json(&self.host)?;
            let mut mrs = list
                .value
                .iter()
                .map(|v| pull_request(v, project, &self.host))
                .collect::<Result<Vec<_>, _>>()?;
            if filter != MrFilter::All {
                let me = self.user().await?.id;
                mrs.retain(|mr| match filter {
                    MrFilter::Mine => mr.author.id == me,
                    MrFilter::ReviewRequested => {
                        mr.review.reviews.iter().any(|review| {
                            review.user.id == me && review.state == ReviewState::Pending
                        })
                    }
                    MrFilter::All => true,
                });
            }
            Ok(Fresh {
                value: mrs,
                not_modified: response.not_modified,
                poll_interval_secs: response.poll_interval_secs,
                fetched_at: unix_now(),
            })
        })
    }

    fn open_mrs<'a>(
        &'a self,
        project: &'a ForgeProject,
        filter: MrFilter,
    ) -> ForgeFuture<'a, Fresh<Vec<ForgeMr>>> {
        self.open_mrs_light(project, filter)
    }

    fn mr_for_branch<'a>(
        &'a self,
        project: &'a ForgeProject,
        source: &'a SourceRef,
    ) -> ForgeFuture<'a, Fresh<Option<ForgeMr>>> {
        Box::pin(async move {
            let ref_name = crate::http::encode_component(&format!("refs/heads/{}", source.branch));
            let path = format!(
                "{}/pullrequests?searchCriteria.status=all&searchCriteria.sourceRefName={ref_name}&api-version=7.1",
                self.repository_url(project)?
            );
            let response = self.http.get(&path).await?;
            let list: List<Value> = response.json(&self.host)?;
            let mrs = list
                .value
                .iter()
                .map(|v| pull_request(v, project, &self.host))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Fresh {
                value: mrs.into_iter().find(|mr| {
                    mr.source_project.eq_ignore_ascii_case(&source.project)
                        && mr.source_branch == source.branch
                }),
                not_modified: response.not_modified,
                poll_interval_secs: response.poll_interval_secs,
                fetched_at: unix_now(),
            })
        })
    }

    fn mr_detail<'a>(
        &'a self,
        project: &'a ForgeProject,
        number: u64,
    ) -> ForgeFuture<'a, Fresh<ForgeMrDetail>> {
        Box::pin(async move {
            let response = self.get_pull_request(project, number).await?;
            let detail = pull_request_detail(&response.json(&self.host)?, project, &self.host)?;
            Ok(Fresh {
                value: detail,
                not_modified: response.not_modified,
                poll_interval_secs: response.poll_interval_secs,
                fetched_at: unix_now(),
            })
        })
    }

    fn discussions<'a>(
        &'a self,
        project: &'a ForgeProject,
        number: u64,
    ) -> ForgeFuture<'a, Fresh<Vec<ForgeDiscussion>>> {
        Box::pin(async move {
            let path = format!(
                "{}/threads?api-version=7.1",
                self.pull_request_url(project, number)?
            );
            let response = self.http.get(&path).await?;
            let list: List<Value> = response.json(&self.host)?;
            let discussions = list
                .value
                .iter()
                .filter_map(|v| discussion(v, &self.host))
                .filter(|d| !d.notes.is_empty())
                .collect();
            Ok(Fresh {
                value: discussions,
                not_modified: response.not_modified,
                poll_interval_secs: response.poll_interval_secs,
                fetched_at: unix_now(),
            })
        })
    }

    fn review_diff<'a>(
        &'a self,
        project: &'a ForgeProject,
        number: u64,
    ) -> ForgeFuture<'a, ReviewDiff> {
        Box::pin(async move {
            let snapshot = self.review_snapshot(project, number).await?;
            let files = self.review_files(project, &snapshot).await?;
            Ok(ReviewDiff {
                refs: snapshot.refs,
                files,
            })
        })
    }

    fn review_drafts<'a>(
        &'a self,
        project: &'a ForgeProject,
        number: u64,
    ) -> ForgeFuture<'a, ReviewDrafts> {
        Box::pin(async move {
            let (_, refs) = self.review_iteration(project, number).await?;
            Ok(ReviewDrafts {
                refs: Some(refs),
                drafts: Vec::new(),
                pending_review: None,
                can_draft: false,
            })
        })
    }

    fn comment_now<'a>(
        &'a self,
        project: &'a ForgeProject,
        number: u64,
        comment: &'a NewReviewComment,
    ) -> ForgeFuture<'a, ForgeDiscussion> {
        Box::pin(async move {
            let snapshot = self.review_snapshot(project, number).await?;
            if comment.refs != snapshot.refs {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "This Azure DevOps pull request changed; compare again before commenting",
                ));
            }
            let path = review_path(&comment.anchor.path)?;
            let change = snapshot.changes.iter().find(|c| {
                c["item"]["path"]
                    .as_str()
                    .and_then(|p| review_path(p).ok())
                    .is_some_and(|p| p == path)
            });
            let change_tracking_id = change
                .and_then(|c| c["changeTrackingId"].as_u64())
                .ok_or_else(|| {
                    GbError::new(
                        GbErrorKind::InvalidInput,
                        "That file is no longer in the Azure DevOps pull request diff",
                    )
                })?;

            let end = comment.anchor.end.position_lines();
            let start = comment
                .anchor
                .start
                .as_ref()
                .map(ReviewLine::position_lines)
                .unwrap_or(end);
            let (left_file_start, left_file_end) = position_bounds(start.0, end.0);
            let (right_file_start, right_file_end) = position_bounds(start.1, end.1);
            let base = self.pull_request_url(project, number)?;
            let response = self
                .http
                .send_json(
                    crate::http::Method::Post,
                    &format!("{base}/threads?api-version=7.1"),
                    &json!({
                        "comments": [{
                            "parentCommentId": 0,
                            "content": comment.body,
                            "commentType": 1
                        }],
                        "status": 1,
                        "threadContext": {
                            "filePath": format!("/{path}"),
                            "leftFileStart": left_file_start,
                            "leftFileEnd": left_file_end,
                            "rightFileStart": right_file_start,
                            "rightFileEnd": right_file_end
                        },
                        "pullRequestThreadContext": {
                            "changeTrackingId": change_tracking_id,
                            "iterationContext": {
                                "firstComparingIteration": 0,
                                "secondComparingIteration": snapshot.iteration
                            }
                        }
                    }),
                )
                .await?;
            let value: Value = response.json(&self.host)?;
            discussion(&value, &self.host)
                .ok_or_else(|| GbError::other("Azure DevOps returned an unreadable review thread"))
        })
    }

    fn reply<'a>(
        &'a self,
        project: &'a ForgeProject,
        number: u64,
        note: &'a NewNote,
    ) -> ForgeFuture<'a, ForgeNote> {
        Box::pin(async move {
            let base = self.pull_request_url(project, number)?;
            let response = if let Some(thread) = &note.discussion {
                let thread_id = thread.parse::<u64>().map_err(|_| {
                    GbError::new(GbErrorKind::InvalidInput, "Invalid Azure DevOps thread ID")
                })?;
                let thread_response = self
                    .http
                    .get(&format!("{base}/threads/{thread_id}?api-version=7.1"))
                    .await?;
                let thread: Value = thread_response.json(&self.host)?;
                let parent = thread["comments"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|c| c["parentCommentId"].as_i64() == Some(0))
                    .and_then(|c| c["id"].as_i64())
                    .ok_or_else(|| {
                        GbError::other("Azure DevOps returned a thread without a root comment")
                    })?;
                self.http
                    .send_json(
                        crate::http::Method::Post,
                        &format!("{base}/threads/{thread_id}/comments?api-version=7.1"),
                        &json!({"content": note.body, "parentCommentId": parent, "commentType": 1}),
                    )
                    .await?
            } else {
                self.http
                    .send_json(
                        crate::http::Method::Post,
                        &format!("{base}/threads?api-version=7.1"),
                        &json!({
                            "comments": [{"parentCommentId": 0, "content": note.body, "commentType": 1}],
                            "status": 1
                        }),
                    )
                    .await?
            };
            let value: Value = response.json(&self.host)?;
            let comment = if note.discussion.is_some() {
                &value
            } else {
                value["comments"]
                    .as_array()
                    .and_then(|comments| comments.first())
                    .ok_or_else(|| {
                        GbError::other("Azure DevOps returned a thread without a comment")
                    })?
            };
            azure_note(comment, &self.host, None)
                .ok_or_else(|| GbError::other("Azure DevOps returned an unreadable comment"))
        })
    }

    fn create_mr<'a>(
        &'a self,
        project: &'a ForgeProject,
        req: &'a CreateMr,
    ) -> ForgeFuture<'a, CreateOutcome> {
        Box::pin(async move {
            if !req.source.project.eq_ignore_ascii_case(&project.path) {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "Azure DevOps pull requests must come from a branch in the target repository",
                ));
            }
            if !req.reviewers.is_empty()
                || !req.assignees.is_empty()
                || !req.labels.is_empty()
                || req.squash.is_some()
                || req.delete_source_branch.is_some()
            {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "Azure DevOps reviewer, assignee, label and merge-default selection is not supported here",
                ));
            }
            let body = json!({
                "sourceRefName": format!("refs/heads/{}", req.source.branch),
                "targetRefName": format!("refs/heads/{}", req.target_branch),
                "title": req.title,
                "description": req.description,
                "isDraft": req.draft,
            });
            let response = self
                .http
                .send_json(
                    crate::http::Method::Post,
                    &format!(
                        "{}/pullrequests?api-version=7.1",
                        self.repository_url(project)?
                    ),
                    &body,
                )
                .await?;
            let value: Value = response.json(&self.host)?;
            Ok(CreateOutcome {
                mr: pull_request(&value, project, &self.host)?,
                failed: Vec::new(),
            })
        })
    }

    fn merge<'a>(
        &'a self,
        project: &'a ForgeProject,
        number: u64,
        opts: &'a MergeOptions,
    ) -> ForgeFuture<'a, ForgeMr> {
        Box::pin(async move {
            let strategy = match opts.method.unwrap_or(MergeMethod::Merge) {
                MergeMethod::Merge => "noFastForward",
                MergeMethod::Squash => "squash",
                MergeMethod::Rebase => "rebase",
                MergeMethod::SemiLinear => "rebaseMerge",
                MergeMethod::FastForward => {
                    return Err(GbError::new(
                        GbErrorKind::InvalidInput,
                        "Azure DevOps does not support fast-forward pull request completion",
                    ));
                }
            };
            let expected_sha = opts.expected_sha.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| {
                GbError::new(
                    GbErrorKind::InvalidInput,
                    "Refresh this pull request before merging so its latest commit can be checked",
                )
            })?;
            let path = format!(
                "{}?api-version=7.1",
                self.pull_request_url(project, number)?
            );
            let response = self
                .http
                .send_json(
                    crate::http::Method::Patch,
                    &path,
                    &json!({
                        "status": "completed",
                        "lastMergeSourceCommit": {"commitId": expected_sha},
                        "completionOptions": {
                            "mergeStrategy": strategy,
                            "deleteSourceBranch": opts.delete_source_branch.unwrap_or(false)
                        }
                    }),
                )
                .await?;
            let value: Value = response.json(&self.host)?;
            pull_request(&value, project, &self.host)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::{Canned, TestServer};

    const USER: &str = r#"{"authenticatedUser":{"id":"11111111-2222-3333-4444-555555555555","isActive":true,"providerDisplayName":"Robin","properties":{"Account":{"$value":"robin@example.com"}}}}"#;
    const REPO: &str = r#"{"id":"66666666-7777-8888-9999-aaaaaaaaaaaa","name":"Orbit App","project":{"name":"Space Tools"},"defaultBranch":"refs/heads/main"}"#;

    fn provider(s: &TestServer) -> AzureDevOpsProvider {
        AzureDevOpsProvider::new(
            "dev.azure.com/orbital",
            &HostEndpoints {
                api: s.base.clone(),
                web: s.base.clone(),
                avatars: None,
            },
            Secret::new("FAKE-azure-pat"),
            None,
        )
    }

    #[tokio::test]
    async fn connects_with_basic_auth_and_discovers_project_repositories() {
        let s = TestServer::start(|_, head| {
            if head.contains("/_apis/connectiondata?") {
                Canned::json(200, USER)
            } else if head.contains("/_apis/git/repositories?") {
                Canned::json(200, &format!(r#"{{"value":[{REPO}]}}"#))
            } else {
                Canned::json(404, "{}")
            }
        });
        let p = provider(&s);
        let check = p.check_token().await.unwrap();
        assert_eq!(
            check.user.id,
            ForgeId::Text("11111111-2222-3333-4444-555555555555".into())
        );
        let repos = p.search_repos("space").await.unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].path, "Space Tools/Orbit App");
        assert_eq!(repos[0].default_branch.as_deref(), Some("main"));
        assert_eq!(
            repos[0].clone_https,
            "https://dev.azure.com/orbital/Space%20Tools/_git/Orbit%20App"
        );
        assert_eq!(
            repos[0].clone_ssh,
            "git@ssh.dev.azure.com:v3/orbital/Space%20Tools/Orbit%20App"
        );
        use base64::Engine;
        let expected = base64::engine::general_purpose::STANDARD
            .encode(":FAKE-azure-pat")
            .to_ascii_lowercase();
        for head in s.heads.lock().unwrap().iter() {
            assert!(head.contains(&format!("authorization: basic {expected}")));
            assert!(!head.lines().next().unwrap().contains("FAKE"));
        }
    }

    #[tokio::test]
    async fn lookup_encodes_names_and_rejects_traversal_and_unsupported_writes() {
        let s = TestServer::start(|_, head| {
            assert!(head.starts_with(
                "get /space%20tools/_apis/git/repositories/orbit%20app?api-version=7.1 "
            ));
            Canned::json(200, REPO)
        });
        let p = provider(&s);
        let project = p.project("Space Tools/Orbit App").await.unwrap().value;
        assert_eq!(
            project.id,
            ForgeId::Text("66666666-7777-8888-9999-aaaaaaaaaaaa".into())
        );
        assert!(p.project("../Orbit App").await.is_err());
        assert!(p.project("Space Tools/../Orbit").await.is_err());
        assert_eq!(
            p.project_settings(&project).await.unwrap().merge_methods,
            [
                MergeMethod::Merge,
                MergeMethod::Squash,
                MergeMethod::Rebase,
                MergeMethod::SemiLinear,
            ]
        );
    }

    #[tokio::test]
    async fn auth_errors_and_malformed_responses_are_not_successful_connections() {
        for (status, body) in [
            (401, "{}"),
            (203, "<html>sign in</html>"),
            (
                200,
                r#"{"authenticatedUser":{"id":"00000000-0000-0000-0000-000000000000","isActive":false,"providerDisplayName":"Anonymous","properties":{}}}"#,
            ),
        ] {
            let s = TestServer::start(move |_, _| Canned::json(status, body));
            assert!(provider(&s).check_token().await.is_err());
        }
    }

    #[tokio::test]
    async fn errors_redact_both_the_pat_and_its_basic_auth_encoding() {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(":FAKE-azure-pat");
        let echo = encoded.clone();
        let s = TestServer::start(move |_, _| {
            Canned::json(403, &format!(r#"{{"message":"FAKE-azure-pat {echo}"}}"#))
        });
        let e = provider(&s).current_user().await.unwrap_err();
        assert!(!e.message.contains("FAKE-azure-pat"));
        assert!(!e.message.contains(&encoded));
    }

    #[tokio::test]
    async fn discovery_returns_all_accessible_repositories() {
        let s = TestServer::start(|_, _| {
            let mut values: Vec<_> = (0..60).map(|i| serde_json::json!({
                    "id": format!("66666666-7777-8888-9999-{i:012x}"), "name": format!("Orbit {i}"),
                    "project": { "name": "Space" }
                })).collect();
            values.push(serde_json::json!({
                "id": "66666666-7777-8888-9999-aaaaaaaaaaaa", "name": "Disabled",
                "project": { "name": "Space" }, "isDisabled": true
            }));
            Canned::json(200, &serde_json::json!({"value": values}).to_string())
        });
        let p = provider(&s);
        assert_eq!(p.search_repos("space").await.unwrap().len(), 60);
        assert!(p.search_repos("missing").await.unwrap().is_empty());
        assert!(p.search_repos("disabled").await.unwrap().is_empty());
    }

    fn sample_project() -> ForgeProject {
        ForgeProject {
            kind: ForgeKind::AzureDevOps,
            id: ForgeId::Text("66666666-7777-8888-9999-aaaaaaaaaaaa".into()),
            host: "dev.azure.com/orbital".into(),
            path: "Space Tools/Orbit App".into(),
            name: "Orbit App".into(),
            owner: "Space Tools".into(),
            web_url: "https://dev.azure.com/orbital/Space%20Tools/_git/Orbit%20App".into(),
            default_branch: Some("main".into()),
            clone_https: "https://dev.azure.com/orbital/Space%20Tools/_git/Orbit%20App".into(),
            clone_ssh: "git@ssh.dev.azure.com:v3/orbital/Space%20Tools/Orbit%20App".into(),
            fork_of: None,
            updated_at: None,
            archived: false,
            owner_avatar_url: None,
        }
    }

    fn sample_pr(status: &str) -> Value {
        json!({
            "pullRequestId": 42,
            "status": status,
            "isDraft": false,
            "createdBy": {
                "id": "11111111-2222-3333-4444-555555555555",
                "displayName": "Robin",
                "uniqueName": "robin@example.com"
            },
            "reviewers": [{
                "id": "22222222-3333-4444-5555-666666666666",
                "displayName": "Alex",
                "uniqueName": "alex@example.com",
                "vote": 10,
                "isRequired": true
            }],
            "sourceRefName": "refs/heads/feature",
            "targetRefName": "refs/heads/main",
            "title": "Add feature",
            "description": "Details",
            "creationDate": "2026-10-01T10:00:00Z",
            "mergeStatus": "succeeded",
            "lastMergeSourceCommit": {"commitId": "head-sha"},
            "lastMergeTargetCommit": {"commitId": "base-sha"},
            "lastMergeCommit": {"commitId": "merge-sha"},
            "completionOptions": {"mergeStrategy": "squash", "deleteSourceBranch": true}
        })
    }

    #[tokio::test]
    async fn lists_and_reads_pull_requests_with_reviews_and_merge_status() {
        let body = serde_json::json!({"value": [sample_pr("active")]}).to_string();
        let list_body = body.clone();
        let detail_body = sample_pr("active").to_string();
        let s = TestServer::start(move |_, head| {
            if head.contains("/pullrequests?") {
                Canned::json(200, &list_body)
            } else {
                Canned::json(200, &detail_body)
            }
        });
        let p = provider(&s);
        let project = sample_project();
        let list = p.open_mrs(&project, MrFilter::All).await.unwrap();
        assert_eq!(list.value.len(), 1);
        assert_eq!(list.value[0].number, 42);
        assert_eq!(list.value[0].state, MrState::Open);
        assert_eq!(list.value[0].source_branch, "feature");
        assert_eq!(list.value[0].head_sha.as_deref(), Some("head-sha"));
        assert_eq!(list.value[0].review.approvals, 1);

        let detail = p.mr_detail(&project, 42).await.unwrap().value;
        assert_eq!(detail.merge_status, MergeStatus::Mergeable);
        assert_eq!(detail.base_sha.as_deref(), Some("base-sha"));
        assert_eq!(detail.merged_sha.as_deref(), Some("merge-sha"));
        assert_eq!(detail.reviewers.len(), 1);
        assert!(detail.squash.unwrap());
        assert!(detail.delete_source_branch.unwrap());
        assert_eq!(
            list.value[0].web_url,
            "https://dev.azure.com/orbital/Space%20Tools/_git/Orbit%20App/pullrequest/42"
        );
    }

    #[tokio::test]
    async fn branch_lookup_includes_completed_pull_requests() {
        let mut pr = sample_pr("completed");
        pr["sourceRefName"] = json!("refs/heads/feature/login");
        let body = serde_json::json!({"value": [pr]}).to_string();
        let s = TestServer::start(move |_, _| Canned::json(200, &body));
        let found = provider(&s)
            .mr_for_branch(
                &sample_project(),
                &SourceRef {
                    project: "space tools/orbit app".into(),
                    branch: "feature/login".into(),
                },
            )
            .await
            .unwrap()
            .value
            .unwrap();
        assert_eq!(found.state, MrState::Merged);
        let heads = s.heads.lock().unwrap();
        let request_line = heads[0].lines().next().unwrap();
        assert!(
            request_line.contains(
                "searchcriteria.status=all&searchcriteria.sourcerefname=refs%2fheads%2ffeature%2flogin"
            ),
            "{}",
            request_line
        );
    }

    #[tokio::test]
    async fn review_diff_uses_the_latest_iteration_and_marks_its_changed_lines() {
        let iterations = json!({
            "count": 1,
            "value": [{
                "id": 3,
                "commonRefCommit": {"commitId": "base"},
                "targetRefCommit": {"commitId": "target"},
                "sourceRefCommit": {"commitId": "head"}
            }]
        })
        .to_string();
        let changes = json!({
            "changeEntries": [
                {"changeType": "add", "item": {"path": "/src", "isFolder": true}},
                {
                    "changeTrackingId": 19,
                    "changeType": "edit",
                    "item": {"path": "/src/lib.rs"}
                }
            ]
        })
        .to_string();
        let s = TestServer::start(move |_, head| {
            if head.contains("/iterations?") {
                Canned::json(200, &iterations)
            } else if head.contains("/iterations/3/changes?") {
                Canned::json(200, &changes)
            } else if head.contains("versiondescriptor.version=base") {
                Canned {
                    status: 200,
                    headers: vec![("Content-Type".into(), "text/plain".into())],
                    body: b"one\nold\nend\n".to_vec(),
                }
            } else if head.contains("versiondescriptor.version=head") {
                Canned {
                    status: 200,
                    headers: vec![("Content-Type".into(), "text/plain".into())],
                    body: b"one\nnew\nend\n".to_vec(),
                }
            } else {
                Canned::json(404, "{}")
            }
        });
        let result = provider(&s)
            .review_diff(&sample_project(), 42)
            .await
            .unwrap();
        assert_eq!(
            result.refs,
            DiffRefs {
                base_sha: "base".into(),
                start_sha: "target".into(),
                head_sha: "head".into()
            }
        );
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "src/lib.rs");
        assert!(!result.files[0].too_large);
        assert!(result.files[0].lines.contains(&ReviewLine {
            kind: LineKind::Removed,
            old_line: 2,
            new_line: 2
        }));
        assert!(
            result.files[0].lines.contains(&ReviewLine {
                kind: LineKind::Added,
                old_line: 3,
                new_line: 2
            }),
            "{:?}",
            result.files[0].lines
        );
        let heads = s.heads.lock().unwrap();
        let item_reads: Vec<_> = heads
            .iter()
            .filter(|head| head.contains("/items?"))
            .collect();
        assert_eq!(item_reads.len(), 2);
        assert!(
            item_reads
                .iter()
                .all(|head| head.contains("accept: text/plain"))
        );
    }

    #[tokio::test]
    async fn inline_review_comments_use_the_current_iteration_and_are_read_back() {
        let iterations = json!([{
            "id": 3,
            "commonRefCommit": {"commitId": "base"},
            "targetRefCommit": {"commitId": "target"},
            "sourceRefCommit": {"commitId": "head"}
        }])
        .to_string();
        let changes = json!({
            "changeEntries": [{
                "changeTrackingId": 19,
                "changeType": "edit",
                "item": {"path": "/src/lib.rs"}
            }]
        })
        .to_string();
        let thread = json!({
            "id": 141,
            "status": "active",
            "threadContext": {
                "filePath": "/src/lib.rs",
                "rightFileStart": {"line": 2, "offset": 1},
                "rightFileEnd": {"line": 2, "offset": 1}
            },
            "pullRequestThreadContext": {
                "changeTrackingId": 19,
                "iterationContext": {
                    "firstComparingIteration": 0,
                    "secondComparingIteration": 3
                }
            },
            "comments": [{
                "id": 7,
                "parentCommentId": 0,
                "content": "Inline",
                "author": {
                    "id": "11111111-2222-3333-4444-555555555555",
                    "displayName": "Robin",
                    "uniqueName": "robin@example.com"
                },
                "publishedDate": "2026-10-02T09:00:00Z",
                "commentType": "text"
            }]
        })
        .to_string();
        let iterations_reply = iterations.clone();
        let changes_reply = changes.clone();
        let thread_reply = thread.clone();
        let threads_reply = json!({
            "value": [serde_json::from_str::<Value>(&thread).unwrap()]
        })
        .to_string();
        let s = TestServer::start(move |_, head| {
            if head.contains("/iterations?") {
                Canned::json(200, &iterations_reply)
            } else if head.contains("/iterations/3/changes?") {
                Canned::json(200, &changes_reply)
            } else if head.starts_with("post ") {
                Canned::json(200, &thread_reply)
            } else if head.contains("/threads?") {
                Canned::json(200, &threads_reply)
            } else {
                Canned::json(404, "{}")
            }
        });
        let p = provider(&s);
        let project = sample_project();
        let comment = NewReviewComment {
            anchor: ReviewAnchor {
                path: "src/lib.rs".into(),
                old_path: "src/lib.rs".into(),
                start: None,
                end: ReviewLine {
                    kind: LineKind::Added,
                    old_line: 2,
                    new_line: 2,
                },
            },
            body: "Inline".into(),
            refs: DiffRefs {
                base_sha: "base".into(),
                start_sha: "target".into(),
                head_sha: "head".into(),
            },
        };
        let posted = p.comment_now(&project, 42, &comment).await.unwrap();
        assert_eq!(posted.notes[0].position.as_ref().unwrap().line, Some(2));
        let request = s
            .heads
            .lock()
            .unwrap()
            .iter()
            .find(|h| h.starts_with("post ") && h.contains("/threads?"))
            .cloned()
            .unwrap();
        assert!(request.contains("/pullrequests/42/threads?api-version=7.1"));
        let body: Value = s
            .bodies
            .lock()
            .unwrap()
            .iter()
            .find(|b| b.contains("Inline"))
            .map(|b| serde_json::from_str(b).unwrap())
            .unwrap();
        assert_eq!(body["threadContext"]["filePath"], "/src/lib.rs");
        assert_eq!(body["threadContext"]["rightFileStart"]["line"], 2);
        assert_eq!(body["pullRequestThreadContext"]["changeTrackingId"], 19);
        assert_eq!(
            body["pullRequestThreadContext"]["iterationContext"]["secondComparingIteration"],
            3
        );

        let discussions = p.discussions(&project, 42).await.unwrap().value;
        assert_eq!(discussions.len(), 1, "{:?}", s.heads.lock().unwrap());
        assert_eq!(
            discussions[0].notes[0].position.as_ref().unwrap().line,
            Some(2)
        );
    }

    #[tokio::test]
    async fn reads_discussions_and_replies_to_a_thread_root() {
        let s = TestServer::start(|_, head| {
            if head.contains("/threads/141?") {
                Canned::json(
                    200,
                    r#"{"id":141,"comments":[{"id":7,"parentCommentId":0}]}"#,
                )
            } else if head.contains("/threads/141/comments?") {
                Canned::json(
                    200,
                    r#"{"id":8,"parentCommentId":7,"content":"Reply","author":{"id":"11111111-2222-3333-4444-555555555555","displayName":"Robin","uniqueName":"robin"},"publishedDate":"2026-10-02T10:00:00Z","commentType":"text"}"#,
                )
            } else {
                Canned::json(
                    200,
                    r#"{"value":[{"id":141,"status":"active","comments":[{"id":7,"parentCommentId":0,"content":"Question","author":{"id":"22222222-3333-4444-5555-666666666666","displayName":"Alex","uniqueName":"alex"},"publishedDate":"2026-10-02T09:00:00Z","commentType":"text"}]},{"id":142,"threadContext":{"filePath":"/src/lib.rs","rightFileStart":{"line":8,"offset":1},"rightFileEnd":{"line":8,"offset":1}},"comments":[{"id":9,"parentCommentId":0,"content":"Inline","author":{"id":"11111111-2222-3333-4444-555555555555","displayName":"Robin","uniqueName":"robin"},"publishedDate":"2026-10-02T10:00:00Z","commentType":"text"}]}]}"#,
                )
            }
        });
        let p = provider(&s);
        let project = sample_project();
        let discussions = p.discussions(&project, 42).await.unwrap().value;
        assert_eq!(discussions.len(), 2);
        assert_eq!(discussions[0].notes[0].body, "Question");
        assert_eq!(
            discussions[1].notes[0].position.as_ref().unwrap().line,
            Some(8)
        );
        let reply = p
            .reply(
                &project,
                42,
                &NewNote {
                    discussion: Some("141".into()),
                    body: "Reply".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(reply.body, "Reply");
        assert_eq!(reply.id, "8");
        let heads = s.heads.lock().unwrap();
        assert!(heads[2].starts_with("post /space%20tools/_apis/git/repositories/"));
        assert!(heads[2].contains("/pullrequests/42/threads/141/comments?api-version=7.1"));
        let body = &s.bodies.lock().unwrap()[2];
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["parentCommentId"], 7);
        assert_eq!(body["content"], "Reply");
    }

    #[tokio::test]
    async fn creates_and_merges_pull_requests_with_supported_options() {
        let response = sample_pr("active").to_string();
        let merged = sample_pr("completed").to_string();
        let create_response = response.clone();
        let merge_response = merged.clone();
        let s = TestServer::start(move |_, head| {
            if head.starts_with("post ") {
                Canned::json(201, &create_response)
            } else {
                Canned::json(200, &merge_response)
            }
        });
        let p = provider(&s);
        let project = sample_project();
        let req = CreateMr {
            source: SourceRef {
                project: project.path.clone(),
                branch: "feature".into(),
            },
            target_branch: "main".into(),
            title: "Add feature".into(),
            description: "Details".into(),
            draft: true,
            reviewers: Vec::new(),
            assignees: Vec::new(),
            labels: Vec::new(),
            squash: None,
            delete_source_branch: None,
        };
        let created = p.create_mr(&project, &req).await.unwrap();
        assert_eq!(created.mr.number, 42);
        let body: Value = serde_json::from_str(&s.bodies.lock().unwrap()[0]).unwrap();
        assert_eq!(body["sourceRefName"], "refs/heads/feature");
        assert_eq!(body["isDraft"], true);

        let merged = p
            .merge(
                &project,
                42,
                &MergeOptions {
                    method: Some(MergeMethod::Squash),
                    squash: None,
                    delete_source_branch: Some(true),
                    expected_sha: Some("head-sha".into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(merged.state, MrState::Merged);
        let heads = s.heads.lock().unwrap();
        assert!(heads[1].starts_with("patch /space%20tools/_apis/git/repositories/"));
        let body: Value = serde_json::from_str(&s.bodies.lock().unwrap()[1]).unwrap();
        assert_eq!(body["status"], "completed");
        assert_eq!(body["lastMergeSourceCommit"]["commitId"], "head-sha");
        assert_eq!(body["completionOptions"]["mergeStrategy"], "squash");
        assert_eq!(body["completionOptions"]["deleteSourceBranch"], true);
    }

    #[tokio::test]
    async fn rejects_unsupported_create_options_and_stale_unchecked_merges() {
        let s = TestServer::start(|_, _| Canned::json(200, "{}"));
        let p = provider(&s);
        let project = sample_project();
        let req = CreateMr {
            source: SourceRef {
                project: project.path.clone(),
                branch: "feature".into(),
            },
            target_branch: "main".into(),
            title: "Add feature".into(),
            description: String::new(),
            draft: false,
            reviewers: vec![1],
            assignees: Vec::new(),
            labels: Vec::new(),
            squash: None,
            delete_source_branch: None,
        };
        assert!(p.create_mr(&project, &req).await.is_err());
        assert!(
            p.merge(&project, 42, &MergeOptions::default())
                .await
                .is_err()
        );
        assert_eq!(s.hits(), 0);
    }
}
