//! Bitbucket Data Center's REST API 1.0.
use crate::endpoints::HostEndpoints;
use crate::http::{ChangeCounter, ClientConfig, HttpClient, Method, encode_component};
use gitnexia_core::avatar::AvatarPayload;
use gitnexia_core::error::{GbError, GbErrorKind};
use gitnexia_core::forge::*;
use gitnexia_core::redact::Secret;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

type CommentVersions = HashMap<(String, u64, u64), (u64, bool)>;

pub struct BitbucketProvider {
    host: String,
    installation: String,
    http: HttpClient,
    build_http: HttpClient,
    checks: crate::pipelines::PipelineCache,
    versions: Mutex<HashMap<(String, u64), Value>>,
    comments: Mutex<CommentVersions>,
    drafts: Mutex<HashMap<(String, u64), Vec<LocalDraft>>>,
    next_draft: AtomicU64,
    submit_lock: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
struct LocalDraft {
    id: u64,
    comment: NewReviewComment,
}

fn draft_view(d: &LocalDraft) -> ReviewDraft {
    let a = &d.comment.anchor;
    let (old, new) = a.end.position_lines();
    ReviewDraft {
        id: format!("local-{}", d.id),
        body: d.comment.body.clone(),
        position: Some(DiffPosition {
            path: a.path.clone(),
            old_path: Some(a.old_path.clone()),
            line: new,
            old_line: old,
            snippet: None,
            start_line: None,
            start_old_line: None,
            head_sha: Some(d.comment.refs.head_sha.clone()),
            outdated: false,
        }),
        reply_to: None,
    }
}

fn unreadable(host: &str, what: &str) -> GbError {
    GbError::other(format!("{host} sent a {what} GitNexia couldn't read"))
}

fn truncated(v: &Value) -> bool {
    v.as_bool() == Some(true) || v.as_str() == Some("true")
}

fn user(v: &Value) -> Option<ForgeUser> {
    let username = v["name"].as_str()?.to_owned();
    let email = v["emailAddress"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    Some(ForgeUser {
        id: v["id"].as_u64()?.into(),
        name: v["displayName"].as_str().unwrap_or(&username).to_owned(),
        username,
        avatar_url: None,
        web_url: v["links"]["self"][0]["href"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        emails: email.iter().cloned().collect(),
        email,
    })
}

fn project(host: &str, installation: &str, v: &Value) -> Option<ForgeProject> {
    let owner = v["project"]["key"].as_str()?.to_owned();
    let slug = v["slug"].as_str()?;
    let clone = v["links"]["clone"].as_array();
    let clone_url = |name| {
        clone
            .and_then(|items| items.iter().find(|item| item["name"] == name))
            .and_then(|item| item["href"].as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let fork_of = v["origin"]["project"]["key"]
        .as_str()
        .zip(v["origin"]["slug"].as_str())
        .map(|(key, slug)| format!("{key}/{slug}"));
    let location = if let Some(user) = owner.strip_prefix('~') {
        format!("users/{}", encode_component(user))
    } else {
        format!("projects/{}", encode_component(&owner))
    };
    Some(ForgeProject {
        kind: ForgeKind::Bitbucket,
        id: v["id"].as_u64()?.into(),
        host: host.to_owned(),
        path: format!("{owner}/{slug}"),
        name: v["name"].as_str().unwrap_or(slug).to_owned(),
        owner,
        web_url: format!(
            "{}/{location}/repos/{}",
            installation.trim_end_matches('/'),
            encode_component(slug)
        ),
        default_branch: None,
        clone_https: clone_url("http"),
        clone_ssh: clone_url("ssh"),
        fork_of,
        updated_at: None,
        archived: v["archived"].as_bool().unwrap_or(false),
        owner_avatar_url: None,
    })
}

fn mr(v: &Value) -> Option<ForgeMr> {
    let ref_path = |reference: &Value| {
        Some(format!(
            "{}/{}",
            reference["repository"]["project"]["key"].as_str()?,
            reference["repository"]["slug"].as_str()?
        ))
    };
    let branch = |reference: &Value| {
        reference["displayId"]
            .as_str()
            .or_else(|| {
                reference["id"]
                    .as_str()
                    .and_then(|s| s.strip_prefix("refs/heads/"))
            })
            .map(str::to_owned)
    };
    let reviews: Vec<ForgeReview> = v["reviewers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let state = match r["status"].as_str() {
                Some("APPROVED") => ReviewState::Approved,
                Some("NEEDS_WORK") => ReviewState::ChangesRequested,
                _ => ReviewState::Pending,
            };
            Some(ForgeReview {
                user: user(&r["user"])?,
                state,
                submitted_at: None,
                requested: true,
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
    Some(ForgeMr {
        number: v["id"].as_u64()?,
        title: v["title"].as_str()?.to_owned(),
        state: match v["state"].as_str()? {
            "OPEN" => MrState::Open,
            "MERGED" => MrState::Merged,
            "DECLINED" => MrState::Closed,
            _ => return None,
        },
        author: user(&v["author"]["user"])?,
        source_project: ref_path(&v["fromRef"])?,
        source_branch: branch(&v["fromRef"])?,
        target_project: ref_path(&v["toRef"])?,
        target_branch: branch(&v["toRef"])?,
        head_sha: v["fromRef"]["latestCommit"].as_str().map(str::to_owned),
        web_url: v["links"]["self"][0]["href"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        pipeline: None,
        review: ReviewSummary {
            decision,
            approvals,
            approvals_required: None,
            reviews,
        },
        conflicts: None,
        labels: Vec::new(),
        label_colors: Default::default(),
        updated_at: v["updatedDate"].as_i64().unwrap_or(0) / 1000,
        stacked: v["description"]
            .as_str()
            .is_some_and(gitnexia_core::forge::stack::carries_stack_table),
        auto_merge: None,
    })
}

fn comment_position(anchor: &Value) -> Option<DiffPosition> {
    let path = anchor["path"].as_str()?.to_owned();
    let line = u32::try_from(anchor["line"].as_u64()?).ok()?;
    let old = anchor["fileType"].as_str() == Some("FROM");
    Some(DiffPosition {
        path,
        old_path: anchor["srcPath"].as_str().map(str::to_owned),
        line: (!old).then_some(line),
        old_line: old.then_some(line),
        snippet: None,
        start_line: None,
        start_old_line: None,
        head_sha: anchor["toHash"].as_str().map(str::to_owned),
        outdated: anchor["orphaned"].as_bool().unwrap_or(false),
    })
}

fn note(v: &Value, position: Option<DiffPosition>) -> Option<ForgeNote> {
    Some(ForgeNote {
        id: v["id"].as_u64()?.to_string(),
        author: user(&v["author"])?,
        body: v["text"].as_str()?.to_owned(),
        created_at: v["createdDate"].as_i64().unwrap_or(0) / 1000,
        system: false,
        position,
        body_html: None,
        reactions: Vec::new(),
        web_url: None,
    })
}

fn repo_path(path: &str, web: &str) -> Result<String, GbError> {
    let context = web
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/').map(|(_, p)| p))
        .unwrap_or("");
    let path = path.trim_matches('/');
    let path = if !context.is_empty() {
        path.strip_prefix(&format!("{context}/scm/"))
            .or_else(|| path.strip_prefix("scm/"))
            .unwrap_or(path)
    } else {
        path.strip_prefix("scm/").unwrap_or(path)
    };
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (key, slug) = path.split_once('/').ok_or_else(|| {
        GbError::new(
            GbErrorKind::InvalidInput,
            "Expected a Bitbucket project key and repository slug",
        )
    })?;
    if key.is_empty()
        || slug.is_empty()
        || slug.contains('/')
        || key == "."
        || key == ".."
        || slug == "."
        || slug == ".."
        || key
            .chars()
            .chain(slug.chars())
            .any(|c| c == '%' || c == '\\' || c.is_control())
    {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "Expected a Bitbucket project key and repository slug",
        ));
    }
    Ok(format!(
        "/projects/{}/repos/{}",
        encode_component(key),
        encode_component(slug)
    ))
}

impl BitbucketProvider {
    pub fn new(
        host: &str,
        endpoints: &HostEndpoints,
        token: Secret,
        changes: Option<ChangeCounter>,
    ) -> Self {
        let http = HttpClient::new(ClientConfig {
            host: host.to_owned(),
            api_base: endpoints.api.clone(),
            token: Some(token.clone()),
            headers: vec![("Accept", "application/json")],
            timeout: Duration::from_secs(45),
        });
        let build_http = HttpClient::new(ClientConfig {
            host: host.to_owned(),
            api_base: format!("{}/rest/build-status/1.0", endpoints.web),
            token: Some(token),
            headers: vec![("Accept", "application/json")],
            timeout: Duration::from_secs(45),
        });
        let (http, build_http) = if let Some(changes) = changes {
            (
                http.with_change_counter(changes.clone()),
                build_http.with_change_counter(changes),
            )
        } else {
            (http, build_http)
        };
        Self {
            host: host.to_owned(),
            installation: endpoints.web.clone(),
            http,
            build_http,
            checks: Default::default(),
            versions: Mutex::default(),
            comments: Mutex::default(),
            drafts: Mutex::default(),
            next_draft: AtomicU64::new(1),
            submit_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn with_installation_base(mut self, base: &str) -> Self {
        self.installation = base.to_owned();
        self
    }

    fn pull_path(&self, project: &ForgeProject, number: u64) -> Result<String, GbError> {
        Ok(format!(
            "{}/pull-requests/{number}",
            repo_path(&project.path, &self.installation)?
        ))
    }

    fn remember_mr(&self, project: &ForgeProject, value: &Value) -> Result<ForgeMr, GbError> {
        let parsed = mr(value).ok_or_else(|| unreadable(&self.host, "pull request"))?;
        if value["version"].as_u64().is_none() {
            return Err(unreadable(&self.host, "pull request version"));
        }
        self.versions
            .lock()
            .expect("versions poisoned")
            .entry((project.path.clone(), parsed.number))
            .or_insert_with(|| value.clone());
        Ok(parsed)
    }

    fn written_mr(&self, project: &ForgeProject, value: &Value) -> Result<ForgeMr, GbError> {
        let parsed = mr(value).ok_or_else(|| unreadable(&self.host, "pull request"))?;
        if value["version"].as_u64().is_none() {
            return Err(unreadable(&self.host, "pull request version"));
        }
        self.versions
            .lock()
            .expect("versions poisoned")
            .insert((project.path.clone(), parsed.number), value.clone());
        Ok(parsed)
    }

    fn remembered(&self, project: &ForgeProject, number: u64) -> Result<Value, GbError> {
        self.versions
            .lock()
            .expect("versions poisoned")
            .get(&(project.path.clone(), number))
            .cloned()
            .ok_or_else(|| {
                GbError::new(
                    GbErrorKind::InvalidInput,
                    "Refresh the pull request before changing it",
                )
            })
    }

    async fn get_mr(&self, project: &ForgeProject, number: u64) -> Result<Value, GbError> {
        let value: Value = self
            .http
            .get(&self.pull_path(project, number)?)
            .await?
            .json(&self.host)?;
        self.remember_mr(project, &value)?;
        Ok(value)
    }

    async fn put_mr(
        &self,
        project: &ForgeProject,
        number: u64,
        body: &Value,
    ) -> Result<ForgeMr, GbError> {
        let value = self
            .http
            .send_json(Method::Put, &self.pull_path(project, number)?, body)
            .await?
            .json(&self.host)?;
        self.written_mr(project, &value)
    }

    fn collect_notes(
        &self,
        p: &ForgeProject,
        number: u64,
        value: &Value,
        position: Option<DiffPosition>,
        notes: &mut Vec<ForgeNote>,
    ) -> Result<(), GbError> {
        let parsed =
            note(value, position.clone()).ok_or_else(|| unreadable(&self.host, "comment"))?;
        let id = value["id"]
            .as_u64()
            .ok_or_else(|| unreadable(&self.host, "comment"))?;
        let version = value["version"]
            .as_u64()
            .ok_or_else(|| unreadable(&self.host, "comment version"))?;
        self.comments
            .lock()
            .expect("comments poisoned")
            .entry((p.path.clone(), number, id))
            .or_insert((version, value["severity"].as_str() == Some("BLOCKER")));
        notes.push(parsed);
        if let Some(children) = value["comments"].as_array() {
            for child in children {
                self.collect_notes(p, number, child, position.clone(), notes)?;
            }
        }
        Ok(())
    }

    fn written_comment(&self, p: &ForgeProject, number: u64, value: &Value) -> Result<(), GbError> {
        let id = value["id"]
            .as_u64()
            .ok_or_else(|| unreadable(&self.host, "comment"))?;
        let version = value["version"]
            .as_u64()
            .ok_or_else(|| unreadable(&self.host, "comment version"))?;
        self.comments.lock().expect("comments poisoned").insert(
            (p.path.clone(), number, id),
            (version, value["severity"].as_str() == Some("BLOCKER")),
        );
        Ok(())
    }

    fn comment_version(
        &self,
        p: &ForgeProject,
        number: u64,
        note: &NoteRef,
    ) -> Result<(u64, u64), GbError> {
        let id = note
            .note
            .parse::<u64>()
            .map_err(|_| GbError::new(GbErrorKind::InvalidInput, "Invalid comment id"))?;
        let version = self
            .comments
            .lock()
            .expect("comments poisoned")
            .get(&(p.path.clone(), number, id))
            .map(|(version, _)| *version)
            .ok_or_else(|| {
                GbError::new(
                    GbErrorKind::InvalidInput,
                    "Refresh the discussion before changing this comment",
                )
            })?;
        Ok((id, version))
    }

    async fn identity(&self) -> Result<ForgeUser, GbError> {
        let response = self.http.get_uncached("/application-properties").await?;
        let username = response
            .authenticated_username
            .as_deref()
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("anonymous"))
            .ok_or_else(|| {
                GbError::new(
                    GbErrorKind::AuthFailed,
                    "Bitbucket did not identify this token as a user",
                )
            })?;
        let id = response
            .authenticated_user_id
            .as_deref()
            .map(|s| s.parse::<u64>())
            .transpose()
            .map_err(|_| {
                GbError::new(
                    GbErrorKind::AuthFailed,
                    "Bitbucket did not identify this token as a user",
                )
            })?;
        // The user detail endpoint takes a slug, which the REST reference says may differ from
        // the username in X-AUSERNAME. Search by name and verify the optional ID when present.
        let values = self
            .pages(&format!("/users?filter={}", encode_component(username)))
            .await?;
        values
            .iter()
            .find_map(|value| {
                let found = user(value)?;
                (id.is_none_or(|id| found.id == id)
                    && found.username == username
                    && value["type"].as_str() == Some("NORMAL"))
                .then_some(found)
            })
            .ok_or_else(|| {
                GbError::new(
                    GbErrorKind::AuthFailed,
                    "Bitbucket's authenticated identity did not match its user record",
                )
            })
    }

    async fn pages_on(&self, client: &HttpClient, path: &str) -> Result<Vec<Value>, GbError> {
        let mut start = 0u64;
        let mut all = Vec::new();
        for _ in 0..100 {
            let sep = if path.contains('?') { '&' } else { '?' };
            let value: Value = client
                .get(&format!("{path}{sep}limit=100&start={start}"))
                .await?
                .json(&self.host)?;
            let values = value["values"]
                .as_array()
                .ok_or_else(|| unreadable(&self.host, "page"))?;
            all.extend(values.iter().cloned());
            match value["isLastPage"].as_bool() {
                Some(true) => return Ok(all),
                Some(false) => {
                    let next = value["nextPageStart"]
                        .as_u64()
                        .filter(|n| *n > start)
                        .ok_or_else(|| unreadable(&self.host, "page cursor"))?;
                    start = next;
                }
                None => return Err(unreadable(&self.host, "page")),
            }
        }
        Err(GbError::other(format!(
            "{0} returned too many pages; narrow the request",
            self.host
        )))
    }

    async fn pages(&self, path: &str) -> Result<Vec<Value>, GbError> {
        self.pages_on(&self.http, path).await
    }

    async fn pipeline(&self, sha: &str) -> Result<Option<ForgePipeline>, GbError> {
        let now = crate::time::unix_now();
        if self.checks.needs([sha], now) {
            let values = self
                .pages_on(
                    &self.build_http,
                    &format!("/commits/{}", encode_component(sha)),
                )
                .await?;
            let mut status = None;
            let mut web_url = None;
            for value in values {
                let next = match value["state"].as_str() {
                    Some("FAILED") => PipelineStatus::Failed,
                    Some("INPROGRESS") => PipelineStatus::Running,
                    Some("SUCCESSFUL") => PipelineStatus::Success,
                    _ => PipelineStatus::Pending,
                };
                let rank = |s| match s {
                    PipelineStatus::Failed => 4,
                    PipelineStatus::Running => 3,
                    PipelineStatus::Pending => 2,
                    PipelineStatus::Success => 1,
                    _ => 2,
                };
                if status.is_none_or(|old| rank(next) > rank(old)) {
                    status = Some(next);
                    web_url = value["url"]
                        .as_str()
                        .filter(|s| s.starts_with("https://"))
                        .map(str::to_owned);
                }
            }
            self.checks.put(
                sha,
                status.map(|status| ForgePipeline { status, web_url }),
                now,
            );
        }
        Ok(self.checks.get(sha))
    }

    async fn publish_comment(
        &self,
        p: &ForgeProject,
        number: u64,
        comment: &NewReviewComment,
    ) -> Result<ForgeDiscussion, GbError> {
        let diff = self.review_diff(p, number).await?;
        if diff.refs != comment.refs {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "The pull request diff changed; refresh before publishing this comment",
            ));
        }
        let anchor = &comment.anchor;
        if anchor.start.is_some() {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "Bitbucket does not support multi-line review comments",
            ));
        }
        let valid = diff.files.iter().any(|f| {
            f.path == anchor.path
                && f.old_path == anchor.old_path
                && !f.too_large
                && f.lines.contains(&anchor.end)
        });
        if !valid {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "This line is no longer available in the pull request diff",
            ));
        }
        let (file_type, line_type) = match anchor.end.kind {
            LineKind::Added => ("TO", "ADDED"),
            LineKind::Removed => ("FROM", "REMOVED"),
            LineKind::Context => ("TO", "CONTEXT"),
        };
        let body = json!({"text":comment.body,"anchor":{"diffType":"RANGE","fromHash":comment.refs.base_sha,"toHash":comment.refs.head_sha,"line":anchor.end.number(),"lineType":line_type,"fileType":file_type,"path":anchor.path,"srcPath":anchor.old_path}});
        let value: Value = self
            .http
            .send_json(
                Method::Post,
                &format!("{}/comments", self.pull_path(p, number)?),
                &body,
            )
            .await?
            .json(&self.host)?;
        let mut notes = Vec::new();
        let position = Some(DiffPosition {
            path: anchor.path.clone(),
            old_path: Some(anchor.old_path.clone()),
            line: anchor.end.position_lines().1,
            old_line: anchor.end.position_lines().0,
            snippet: None,
            start_line: None,
            start_old_line: None,
            head_sha: Some(comment.refs.head_sha.clone()),
            outdated: false,
        });
        self.collect_notes(p, number, &value, position, &mut notes)?;
        let id = notes
            .first()
            .ok_or_else(|| unreadable(&self.host, "comment"))?
            .id
            .clone();
        Ok(ForgeDiscussion {
            id,
            notes,
            resolvable: false,
            resolved: false,
            resolved_by: None,
            resolved_at: None,
        })
    }
}

impl ForgeProvider for BitbucketProvider {
    fn kind(&self) -> ForgeKind {
        ForgeKind::Bitbucket
    }
    fn host(&self) -> &str {
        &self.host
    }
    fn rate_limit(&self) -> RateLimitState {
        let mut api = self.http.rate_limit();
        let builds = self.build_http.rate_limit();
        api.limited_until = api.limited_until.max(builds.limited_until);
        api
    }
    fn expire_fresh(&self) {
        self.http.expire_fresh();
        self.build_http.expire_fresh();
    }
    fn take_request_stats(&self) -> RequestStats {
        let mut api = self.http.take_stats();
        let builds = self.build_http.take_stats();
        api.sent += builds.sent;
        api.not_modified += builds.not_modified;
        api.fresh += builds.fresh;
        api
    }
    fn current_user(&self) -> ForgeFuture<'_, ForgeUser> {
        Box::pin(self.identity())
    }
    fn check_token(&self) -> ForgeFuture<'_, TokenCheck> {
        Box::pin(async move {
            Ok(TokenCheck {
                user: self.identity().await?,
                write: WriteAccess::Unknown,
            })
        })
    }

    fn version(&self) -> ForgeFuture<'_, Option<String>> {
        Box::pin(async move {
            Ok(self
                .http
                .get("/application-properties")
                .await?
                .json::<Value>(&self.host)?["version"]
                .as_str()
                .map(str::to_owned))
        })
    }
    fn project<'a>(&'a self, path: &'a str) -> ForgeFuture<'a, Fresh<ForgeProject>> {
        Box::pin(async move {
            let url = repo_path(path, &self.installation)?;
            let r = self.http.get(&url).await?;
            let mut p = project(
                &self.host,
                &self.installation,
                &r.json::<Value>(&self.host)?,
            )
            .ok_or_else(|| unreadable(&self.host, "repository"))?;
            match self.http.get(&format!("{url}/branches/default")).await {
                Ok(branch) => {
                    p.default_branch = branch.json::<Value>(&self.host)?["displayId"]
                        .as_str()
                        .map(str::to_owned)
                }
                Err(e) if e.kind == GbErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            Ok(Fresh {
                value: p,
                not_modified: r.not_modified,
                poll_interval_secs: r.poll_interval_secs,
                fetched_at: crate::time::unix_now(),
            })
        })
    }
    fn search_repos<'a>(&'a self, query: &'a str) -> ForgeFuture<'a, Vec<ForgeProject>> {
        Box::pin(async move {
            let values = self
                .pages(&format!("/repos?name={}", encode_component(query.trim())))
                .await?;
            values
                .iter()
                .map(|v| {
                    project(&self.host, &self.installation, v)
                        .ok_or_else(|| unreadable(&self.host, "repository"))
                })
                .collect()
        })
    }
    fn project_settings<'a>(
        &'a self,
        _project: &'a ForgeProject,
    ) -> ForgeFuture<'a, ForgeProjectSettings> {
        unsupported("Bitbucket merge settings")
    }
    fn forks<'a>(&'a self, p: &'a ForgeProject) -> ForgeFuture<'a, Vec<ForgeProject>> {
        Box::pin(async move {
            let values = self
                .pages(&format!(
                    "{}/forks",
                    repo_path(&p.path, &self.installation)?
                ))
                .await?;
            values
                .iter()
                .map(|v| {
                    project(&self.host, &self.installation, v)
                        .ok_or_else(|| unreadable(&self.host, "repository"))
                })
                .collect()
        })
    }
    fn open_mrs_light<'a>(
        &'a self,
        p: &'a ForgeProject,
        filter: MrFilter,
    ) -> ForgeFuture<'a, Fresh<Vec<ForgeMr>>> {
        Box::pin(async move {
            let values = self
                .pages(&format!(
                    "{}/pull-requests?state=OPEN",
                    repo_path(&p.path, &self.installation)?
                ))
                .await?;
            let mut list: Vec<ForgeMr> = values
                .iter()
                .map(|v| self.remember_mr(p, v))
                .collect::<Result<_, _>>()?;
            if filter != MrFilter::All {
                let me = self.identity().await?;
                list.retain(|pr| match filter {
                    MrFilter::Mine => pr.author.id == me.id,
                    MrFilter::ReviewRequested => pr
                        .review
                        .reviews
                        .iter()
                        .any(|r| r.user.id == me.id && r.state == ReviewState::Pending),
                    MrFilter::All => true,
                });
            }
            Ok(Fresh::new(list, crate::time::unix_now()))
        })
    }
    fn open_mrs<'a>(
        &'a self,
        p: &'a ForgeProject,
        filter: MrFilter,
    ) -> ForgeFuture<'a, Fresh<Vec<ForgeMr>>> {
        Box::pin(async move {
            let mut list = self.open_mrs_light(p, filter).await?;
            for item in list.value.iter_mut().take(20) {
                if let Some(sha) = &item.head_sha {
                    item.pipeline = self.pipeline(sha).await?;
                }
            }
            Ok(list)
        })
    }
    fn mr_for_branch<'a>(
        &'a self,
        p: &'a ForgeProject,
        source: &'a SourceRef,
    ) -> ForgeFuture<'a, Fresh<Option<ForgeMr>>> {
        Box::pin(async move {
            let list = self.open_mrs_light(p, MrFilter::All).await?;
            Ok(list.map(|list| {
                list.into_iter().find(|m| {
                    m.source_project.eq_ignore_ascii_case(&source.project)
                        && m.source_branch == source.branch
                })
            }))
        })
    }
    fn mr_detail<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
    ) -> ForgeFuture<'a, Fresh<ForgeMrDetail>> {
        Box::pin(async move {
            let value = self.get_mr(p, number).await?;
            let mut mr = self.remember_mr(p, &value)?;
            if let Some(sha) = &mr.head_sha {
                mr.pipeline = self.pipeline(sha).await?;
            }
            let detail = ForgeMrDetail {
                reviewers: mr.review.reviews.iter().map(|r| r.user.clone()).collect(),
                assignees: Vec::new(),
                merge_status: MergeStatus::Blocked {
                    reason: "Open this pull request in your browser to merge".into(),
                },
                squash: None,
                delete_source_branch: None,
                base_sha: value["toRef"]["latestCommit"].as_str().map(str::to_owned),
                subscribed: None,
                body_html: None,
                update: None,
                merged_sha: None,
                description: value["description"].as_str().unwrap_or_default().to_owned(),
                mr,
            };
            Ok(Fresh::new(detail, crate::time::unix_now()))
        })
    }
    fn search_users<'a>(
        &'a self,
        _p: &'a ForgeProject,
        query: &'a str,
    ) -> ForgeFuture<'a, Vec<ForgeUser>> {
        Box::pin(async move {
            let values = self
                .pages(&format!("/users?filter={}", encode_component(query)))
                .await?;
            values
                .iter()
                .map(|v| user(v).ok_or_else(|| unreadable(&self.host, "user")))
                .collect()
        })
    }
    fn create_mr<'a>(
        &'a self,
        p: &'a ForgeProject,
        req: &'a CreateMr,
    ) -> ForgeFuture<'a, CreateOutcome> {
        Box::pin(async move {
            if req.draft
                || !req.labels.is_empty()
                || !req.assignees.is_empty()
                || req.squash.is_some()
                || req.delete_source_branch.is_some()
            {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "Bitbucket does not support these pull request creation options",
                ));
            }
            let (source_key, source_slug) =
                req.source.project.split_once('/').ok_or_else(|| {
                    GbError::new(GbErrorKind::InvalidInput, "Invalid source repository")
                })?;
            let (target_key, target_slug) = p.path.split_once('/').ok_or_else(|| {
                GbError::new(GbErrorKind::InvalidInput, "Invalid target repository")
            })?;
            let mut reviewers = Vec::new();
            if !req.reviewers.is_empty() {
                let all = self.search_users(p, "").await?;
                for id in &req.reviewers {
                    let u = all.iter().find(|u| u.id == *id).ok_or_else(|| {
                        GbError::new(
                            GbErrorKind::InvalidInput,
                            "A reviewer is no longer available; refresh and try again",
                        )
                    })?;
                    reviewers.push(json!({"user":{"name":u.username}}));
                }
            }
            let body = json!({"title":req.title,"description":req.description,
                "fromRef":{"id":format!("refs/heads/{}",req.source.branch),"repository":{"slug":source_slug,"project":{"key":source_key}}},
                "toRef":{"id":format!("refs/heads/{}",req.target_branch),"repository":{"slug":target_slug,"project":{"key":target_key}}}, "reviewers":reviewers});
            let path = format!("{}/pull-requests", repo_path(&p.path, &self.installation)?);
            let value: Value = self
                .http
                .send_json(Method::Post, &path, &body)
                .await?
                .json(&self.host)?;
            Ok(CreateOutcome {
                mr: self.remember_mr(p, &value)?,
                failed: vec![],
            })
        })
    }
    fn edit<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        edit: &'a MrEdit,
    ) -> ForgeFuture<'a, ForgeMr> {
        Box::pin(async move {
            if edit.labels.is_some() || edit.assignees.is_some() {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "Bitbucket pull requests do not support labels or assignees",
                ));
            }
            let current = self.remembered(p, number)?;
            let mut body = json!({"id":number,"version":current["version"],"title":edit.title.as_deref().unwrap_or(current["title"].as_str().unwrap_or_default()),"description":edit.description.as_deref().unwrap_or(current["description"].as_str().unwrap_or_default())});
            if let Some(changes) = &edit.reviewers {
                let all = self.search_users(p, "").await?;
                let existing: Vec<u64> = current["reviewers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r["user"]["id"].as_u64())
                    .collect();
                let mut reviewers = Vec::new();
                for id in changes.apply(&existing) {
                    let username = all
                        .iter()
                        .find(|u| u.id == id)
                        .map(|u| u.username.as_str())
                        .or_else(|| {
                            current["reviewers"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .find(|r| r["user"]["id"].as_u64() == Some(id))
                                .and_then(|r| r["user"]["name"].as_str())
                        })
                        .ok_or_else(|| {
                            GbError::new(
                                GbErrorKind::InvalidInput,
                                "A reviewer is no longer available; refresh and try again",
                            )
                        })?;
                    reviewers.push(json!({"user":{"name":username}}));
                }
                body["reviewers"] = json!(reviewers);
            }
            self.put_mr(p, number, &body).await
        })
    }
    fn retarget<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        branch: &'a str,
    ) -> ForgeFuture<'a, ForgeMr> {
        Box::pin(async move {
            let current = self.remembered(p, number)?;
            let mut to_ref = current["toRef"].clone();
            to_ref["id"] = json!(format!("refs/heads/{branch}"));
            self.put_mr(p, number, &json!({"id":number,"version":current["version"],"title":current["title"],"description":current["description"],"toRef":to_ref})).await
        })
    }
    fn set_closed<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        closed: bool,
    ) -> ForgeFuture<'a, ForgeMr> {
        Box::pin(async move {
            let current = self.remembered(p, number)?;
            let version = current["version"]
                .as_u64()
                .ok_or_else(|| unreadable(&self.host, "pull request version"))?;
            let action = if closed { "decline" } else { "reopen" };
            let response = self
                .http
                .send_json(
                    Method::Post,
                    &format!("{}/{action}?version={version}", self.pull_path(p, number)?),
                    &json!({}),
                )
                .await?;
            if response.body.is_empty() {
                return Err(GbError::other(
                    "Bitbucket accepted the pull request change but returned no version; reconnect before changing it again",
                ));
            }
            let value: Value = response.json(&self.host)?;
            self.written_mr(p, &value)
        })
    }
    fn approve<'a>(&'a self, p: &'a ForgeProject, number: u64) -> ForgeFuture<'a, ()> {
        Box::pin(async move {
            self.http
                .send_json(
                    Method::Post,
                    &format!("{}/approve", self.pull_path(p, number)?),
                    &json!({}),
                )
                .await?;
            Ok(())
        })
    }
    fn request_changes<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        body: &'a str,
    ) -> ForgeFuture<'a, ()> {
        Box::pin(async move {
            let me = self.identity().await?;
            self.http
                .send_json(
                    Method::Put,
                    &format!(
                        "{}/participants/{}",
                        self.pull_path(p, number)?,
                        encode_component(&me.username)
                    ),
                    &json!({"status":"NEEDS_WORK"}),
                )
                .await?;
            if !body.trim().is_empty() {
                self.reply(
                    p,
                    number,
                    &NewNote {
                        discussion: None,
                        body: body.to_owned(),
                    },
                )
                .await?;
            }
            Ok(())
        })
    }
    fn set_subscribed<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        on: bool,
    ) -> ForgeFuture<'a, bool> {
        Box::pin(async move {
            let path = format!("{}/watch", self.pull_path(p, number)?);
            if on {
                self.http.send_json(Method::Post, &path, &json!({})).await?;
            } else {
                self.http.delete(&path).await?;
            }
            Ok(on)
        })
    }
    fn discussions<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
    ) -> ForgeFuture<'a, Fresh<Vec<ForgeDiscussion>>> {
        Box::pin(async move {
            let values = self
                .pages(&format!("{}/activities", self.pull_path(p, number)?))
                .await?;
            let mut threads = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for activity in values {
                if activity["action"].as_str() != Some("COMMENTED") {
                    continue;
                }
                let value = &activity["comment"];
                if value["parent"]["id"].as_u64().is_some() {
                    continue;
                }
                let id = value["id"]
                    .as_u64()
                    .ok_or_else(|| unreadable(&self.host, "comment"))?;
                if !seen.insert(id) {
                    continue;
                }
                if activity["commentAction"].as_str() == Some("DELETED") {
                    continue;
                }
                let mut notes = Vec::new();
                self.collect_notes(
                    p,
                    number,
                    value,
                    comment_position(&activity["commentAnchor"]),
                    &mut notes,
                )?;
                for note in &notes {
                    if let Ok(id) = note.id.parse::<u64>() {
                        seen.insert(id);
                    }
                }
                threads.push(ForgeDiscussion {
                    id: id.to_string(),
                    notes,
                    resolvable: value["severity"].as_str() == Some("BLOCKER"),
                    resolved: value["state"].as_str() == Some("RESOLVED"),
                    resolved_by: None,
                    resolved_at: None,
                });
            }
            Ok(Fresh::new(threads, crate::time::unix_now()))
        })
    }
    fn reply<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        new: &'a NewNote,
    ) -> ForgeFuture<'a, ForgeNote> {
        Box::pin(async move {
            let mut body = json!({"text":new.body});
            if let Some(parent) = &new.discussion {
                let id = parent.parse::<u64>().map_err(|_| {
                    GbError::new(GbErrorKind::InvalidInput, "Invalid discussion id")
                })?;
                body["parent"] = json!({"id":id});
            }
            let value: Value = self
                .http
                .send_json(
                    Method::Post,
                    &format!("{}/comments", self.pull_path(p, number)?),
                    &body,
                )
                .await?
                .json(&self.host)?;
            let mut notes = Vec::new();
            self.collect_notes(p, number, &value, None, &mut notes)?;
            notes
                .into_iter()
                .next()
                .ok_or_else(|| unreadable(&self.host, "comment"))
        })
    }
    fn edit_note<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        n: &'a NoteRef,
        body: &'a str,
    ) -> ForgeFuture<'a, ForgeNote> {
        Box::pin(async move {
            let (id, version) = self.comment_version(p, number, n)?;
            let value: Value = self
                .http
                .send_json(
                    Method::Put,
                    &format!("{}/comments/{id}", self.pull_path(p, number)?),
                    &json!({"text":body,"version":version}),
                )
                .await?
                .json(&self.host)?;
            self.written_comment(p, number, &value)?;
            let mut notes = Vec::new();
            self.collect_notes(p, number, &value, None, &mut notes)?;
            notes
                .into_iter()
                .next()
                .ok_or_else(|| unreadable(&self.host, "comment"))
        })
    }
    fn delete_note<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        n: &'a NoteRef,
    ) -> ForgeFuture<'a, ()> {
        Box::pin(async move {
            let (id, version) = self.comment_version(p, number, n)?;
            self.http
                .delete(&format!(
                    "{}/comments/{id}?version={version}",
                    self.pull_path(p, number)?
                ))
                .await?;
            self.comments
                .lock()
                .expect("comments poisoned")
                .remove(&(p.path.clone(), number, id));
            Ok(())
        })
    }
    fn resolve<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        discussion: &'a str,
        resolved: bool,
    ) -> ForgeFuture<'a, ThreadState> {
        Box::pin(async move {
            let reference = NoteRef {
                discussion: discussion.to_owned(),
                note: discussion.to_owned(),
            };
            let (id, version) = self.comment_version(p, number, &reference)?;
            if !self
                .comments
                .lock()
                .expect("comments poisoned")
                .get(&(p.path.clone(), number, id))
                .is_some_and(|(_, task)| *task)
            {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "Only Bitbucket tasks can be resolved",
                ));
            }
            let value: Value = self
                .http
                .send_json(
                    Method::Put,
                    &format!("{}/comments/{id}", self.pull_path(p, number)?),
                    &json!({"version":version,"state":if resolved { "RESOLVED" } else { "OPEN" }}),
                )
                .await?
                .json(&self.host)?;
            self.written_comment(p, number, &value)?;
            let mut notes = Vec::new();
            self.collect_notes(p, number, &value, None, &mut notes)?;
            Ok(ThreadState {
                resolved: value["state"].as_str() == Some("RESOLVED"),
                resolved_by: None,
            })
        })
    }
    fn review_diff<'a>(&'a self, p: &'a ForgeProject, number: u64) -> ForgeFuture<'a, ReviewDiff> {
        Box::pin(async move {
            let pull = self.get_mr(p, number).await?;
            let value: Value = self
                .http
                .get(&format!(
                    "{}/diff?contextLines=10",
                    self.pull_path(p, number)?
                ))
                .await?
                .json(&self.host)?;
            let base = value["fromHash"]
                .as_str()
                .ok_or_else(|| unreadable(&self.host, "diff base"))?
                .to_owned();
            let head = value["toHash"]
                .as_str()
                .ok_or_else(|| unreadable(&self.host, "diff head"))?
                .to_owned();
            if pull["fromRef"]["latestCommit"].as_str() != Some(&head) {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "The pull request changed while reading its diff; refresh and try again",
                ));
            }
            let mut files = Vec::new();
            let diffs = value["diffs"]
                .as_array()
                .ok_or_else(|| unreadable(&self.host, "diff"))?;
            for f in diffs {
                let old_path = f["source"]["toString"]
                    .as_str()
                    .or_else(|| f["destination"]["toString"].as_str())
                    .ok_or_else(|| unreadable(&self.host, "diff path"))?
                    .to_owned();
                let path = f["destination"]["toString"]
                    .as_str()
                    .unwrap_or(&old_path)
                    .to_owned();
                let mut lines = Vec::new();
                let mut incomplete = truncated(&value["truncated"]) || truncated(&f["truncated"]);
                for h in f["hunks"].as_array().into_iter().flatten() {
                    incomplete |= truncated(&h["truncated"]);
                    for segment in h["segments"].as_array().into_iter().flatten() {
                        incomplete |= truncated(&segment["truncated"]);
                        let kind = match segment["type"].as_str() {
                            Some("ADDED") => LineKind::Added,
                            Some("REMOVED") => LineKind::Removed,
                            Some("CONTEXT") => LineKind::Context,
                            _ => return Err(unreadable(&self.host, "diff segment")),
                        };
                        for line in segment["lines"].as_array().into_iter().flatten() {
                            incomplete |= truncated(&line["truncated"]);
                            lines.push(ReviewLine {
                                kind,
                                old_line: u32::try_from(
                                    line["source"]
                                        .as_u64()
                                        .ok_or_else(|| unreadable(&self.host, "diff line"))?,
                                )
                                .map_err(|_| unreadable(&self.host, "diff line"))?,
                                new_line: u32::try_from(
                                    line["destination"]
                                        .as_u64()
                                        .ok_or_else(|| unreadable(&self.host, "diff line"))?,
                                )
                                .map_err(|_| unreadable(&self.host, "diff line"))?,
                            });
                        }
                    }
                }
                files.push(ReviewFile {
                    path,
                    old_path,
                    lines,
                    too_large: incomplete,
                });
            }
            Ok(ReviewDiff {
                refs: DiffRefs {
                    base_sha: base.clone(),
                    start_sha: base,
                    head_sha: head,
                },
                files,
            })
        })
    }
    fn review_drafts<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
    ) -> ForgeFuture<'a, ReviewDrafts> {
        Box::pin(async move {
            let refs = self.review_diff(p, number).await?.refs;
            let drafts = self
                .drafts
                .lock()
                .expect("drafts poisoned")
                .get(&(p.path.clone(), number))
                .map(|ds| ds.iter().map(draft_view).collect::<Vec<_>>())
                .unwrap_or_default();
            Ok(ReviewDrafts {
                refs: Some(refs),
                pending_review: (!drafts.is_empty()).then(|| "local".to_owned()),
                drafts,
                can_draft: true,
            })
        })
    }
    fn add_draft<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        comment: &'a NewReviewComment,
    ) -> ForgeFuture<'a, ReviewDraft> {
        Box::pin(async move {
            let _guard = self.submit_lock.lock().await;
            if comment.anchor.start.is_some() {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "Bitbucket does not support multi-line review comments",
                ));
            }
            let diff = self.review_diff(p, number).await?;
            if diff.refs != comment.refs
                || !diff.files.iter().any(|f| {
                    f.path == comment.anchor.path
                        && f.old_path == comment.anchor.old_path
                        && !f.too_large
                        && f.lines.contains(&comment.anchor.end)
                })
            {
                return Err(GbError::new(
                    GbErrorKind::InvalidInput,
                    "The pull request diff changed; refresh before adding this comment",
                ));
            }
            let draft = LocalDraft {
                id: self.next_draft.fetch_add(1, Ordering::Relaxed),
                comment: comment.clone(),
            };
            let view = draft_view(&draft);
            self.drafts
                .lock()
                .expect("drafts poisoned")
                .entry((p.path.clone(), number))
                .or_default()
                .push(draft);
            Ok(view)
        })
    }
    fn edit_draft<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        id: &'a str,
        body: &'a str,
    ) -> ForgeFuture<'a, ReviewDraft> {
        Box::pin(async move {
            let _guard = self.submit_lock.lock().await;
            let mut drafts = self.drafts.lock().expect("drafts poisoned");
            let d = drafts
                .get_mut(&(p.path.clone(), number))
                .and_then(|v| v.iter_mut().find(|d| format!("local-{}", d.id) == id))
                .ok_or_else(|| GbError::new(GbErrorKind::NotFound, "That draft is gone"))?;
            d.comment.body = body.to_owned();
            Ok(draft_view(d))
        })
    }
    fn delete_draft<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        id: &'a str,
    ) -> ForgeFuture<'a, ()> {
        Box::pin(async move {
            let _guard = self.submit_lock.lock().await;
            let mut drafts = self.drafts.lock().expect("drafts poisoned");
            let list = drafts
                .get_mut(&(p.path.clone(), number))
                .ok_or_else(|| GbError::new(GbErrorKind::NotFound, "That draft is gone"))?;
            let index = list
                .iter()
                .position(|d| format!("local-{}", d.id) == id)
                .ok_or_else(|| GbError::new(GbErrorKind::NotFound, "That draft is gone"))?;
            list.remove(index);
            Ok(())
        })
    }
    fn discard_review<'a>(&'a self, p: &'a ForgeProject, number: u64) -> ForgeFuture<'a, u32> {
        Box::pin(async move {
            let _guard = self.submit_lock.lock().await;
            Ok(self
                .drafts
                .lock()
                .expect("drafts poisoned")
                .remove(&(p.path.clone(), number))
                .map_or(0, |v| v.len() as u32))
        })
    }
    fn comment_now<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        comment: &'a NewReviewComment,
    ) -> ForgeFuture<'a, ForgeDiscussion> {
        Box::pin(self.publish_comment(p, number, comment))
    }
    fn submit_review<'a>(
        &'a self,
        p: &'a ForgeProject,
        number: u64,
        review: &'a ReviewSubmit,
    ) -> ForgeFuture<'a, SubmitOutcome> {
        Box::pin(async move {
            let _guard = self.submit_lock.lock().await;
            let key = (p.path.clone(), number);
            let drafts = self
                .drafts
                .lock()
                .expect("drafts poisoned")
                .get(&key)
                .cloned()
                .unwrap_or_default();
            let mut published = 0;
            for draft in drafts {
                if let Err(e) = self.publish_comment(p, number, &draft.comment).await {
                    return Ok(SubmitOutcome {
                        published,
                        event_error: Some(e.message),
                        ..Default::default()
                    });
                }
                if let Some(list) = self.drafts.lock().expect("drafts poisoned").get_mut(&key) {
                    list.retain(|d| d.id != draft.id);
                }
                published += 1;
            }
            let mut outcome = SubmitOutcome {
                published,
                ..Default::default()
            };
            if review.event != ReviewEvent::Comment {
                let result = if review.event == ReviewEvent::Approve {
                    self.approve(p, number).await
                } else {
                    self.request_changes(p, number, "").await
                };
                if let Err(e) = result {
                    outcome.event_error = Some(e.message);
                    return Ok(outcome);
                }
                outcome.event_sent = true;
            }
            if !review.body.trim().is_empty() {
                match self
                    .reply(
                        p,
                        number,
                        &NewNote {
                            discussion: None,
                            body: review.body.clone(),
                        },
                    )
                    .await
                {
                    Ok(_) => outcome.body_posted = true,
                    Err(e) => outcome.event_error = Some(e.message),
                }
            }
            Ok(outcome)
        })
    }
    fn avatar_for_email<'a>(&'a self, _email: &'a str) -> ForgeFuture<'a, Option<AvatarPayload>> {
        Box::pin(async { Ok(None) })
    }
    fn merge<'a>(
        &'a self,
        _project: &'a ForgeProject,
        _number: u64,
        _opts: &'a MergeOptions,
    ) -> ForgeFuture<'a, ForgeMr> {
        Box::pin(async {
            Err(GbError::new(
                GbErrorKind::InvalidInput,
                "Bitbucket HTTP access token merges are unavailable; open the pull request in your browser",
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::{Canned, TestServer};

    fn provider(server: &TestServer) -> BitbucketProvider {
        BitbucketProvider::new(
            "bitbucket.example.com",
            &HostEndpoints {
                api: format!("{}/context/rest/api/1.0", server.base),
                web: format!("{}/context", server.base),
                avatars: None,
            },
            Secret::new("fake-token"),
            None,
        )
    }

    #[test]
    fn paths_preserve_project_and_context() {
        let web = "https://bitbucket.example.com/context";
        assert_eq!(
            repo_path("context/scm/TEAM/widget.git", web).unwrap(),
            "/projects/TEAM/repos/widget"
        );
        assert_eq!(
            repo_path("~dev/widget", web).unwrap(),
            "/projects/~dev/repos/widget"
        );
        assert!(repo_path("TEAM/a/b", web).is_err());
        assert!(repo_path("../widget", web).is_err());
        let personal = json!({"id": 3, "slug": "widget", "project": {"key": "~nova"}, "links": {"self": [{"href": "https://wrong.example/browse"}]}});
        assert_eq!(
            project("bitbucket.example.com", web, &personal)
                .unwrap()
                .web_url,
            "https://bitbucket.example.com/context/users/nova/repos/widget"
        );
    }

    #[tokio::test]
    async fn identity_requires_authenticated_headers_and_matching_user() {
        let server = TestServer::start(|_, head| {
            if head.starts_with("get /context/rest/api/1.0/application-properties ") {
                Canned::json(200, r#"{"version":"8.19.14"}"#)
                    .header("X-AUSERNAME", "dev")
                    .header("X-AUSERID", "7")
            } else {
                Canned::json(
                    200,
                    r#"{"values":[{"name":"dev","slug":"different-slug","id":7,"displayName":"Developer","type":"NORMAL"}],"isLastPage":true}"#,
                )
            }
        });
        assert_eq!(
            provider(&server).check_token().await.unwrap().user.username,
            "dev"
        );
        let no_id = TestServer::start(|_, head| {
            if head.contains("application-properties") {
                Canned::json(200, "{}").header("X-AUSERNAME", "dev")
            } else {
                Canned::json(
                    200,
                    r#"{"values":[{"name":"developer","id":9,"type":"NORMAL"},{"name":"dev","id":7,"type":"NORMAL"}],"isLastPage":true}"#,
                )
            }
        });
        assert_eq!(provider(&no_id).check_token().await.unwrap().user.id, 7);
        let anonymous = TestServer::start(|_, _| Canned::json(200, r#"{"version":"8.19.14"}"#));
        assert_eq!(
            provider(&anonymous).check_token().await.unwrap_err().kind,
            GbErrorKind::AuthFailed
        );
        let wrong = TestServer::start(|_, head| {
            if head.contains("application-properties") {
                Canned::json(200, "{}")
                    .header("X-AUSERNAME", "dev")
                    .header("X-AUSERID", "7")
            } else {
                Canned::json(
                    200,
                    r#"{"values":[{"name":"dev","id":8,"type":"NORMAL"}],"isLastPage":true}"#,
                )
            }
        });
        assert_eq!(
            provider(&wrong).check_token().await.unwrap_err().kind,
            GbErrorKind::AuthFailed
        );
        let wrong_name = TestServer::start(|_, head| {
            if head.contains("application-properties") {
                Canned::json(200, "{}").header("X-AUSERNAME", "dev")
            } else {
                Canned::json(
                    200,
                    r#"{"values":[{"name":"developer","id":7,"type":"NORMAL"}],"isLastPage":true}"#,
                )
            }
        });
        assert_eq!(
            provider(&wrong_name).check_token().await.unwrap_err().kind,
            GbErrorKind::AuthFailed
        );
        let malformed = TestServer::start(|_, _| {
            Canned::json(200, "{}")
                .header("X-AUSERNAME", "dev")
                .header("X-AUSERID", "not-a-number")
        });
        assert_eq!(
            provider(&malformed).check_token().await.unwrap_err().kind,
            GbErrorKind::AuthFailed
        );
        let explicit_anonymous =
            TestServer::start(|_, _| Canned::json(200, "{}").header("X-AUSERNAME", "Anonymous"));
        assert_eq!(
            provider(&explicit_anonymous)
                .check_token()
                .await
                .unwrap_err()
                .kind,
            GbErrorKind::AuthFailed
        );
        let redirected = TestServer::start(|_, _| {
            Canned::json(302, "{}").header("Location", "/context/rest/api/1.0/elsewhere")
        });
        assert!(provider(&redirected).check_token().await.is_err());
    }

    #[tokio::test]
    async fn pagination_follows_server_cursor_and_rejects_nonadvancing_cursor() {
        let server = TestServer::start(|_, head| {
            if head.contains("start=0") {
                Canned::json(
                    200,
                    r#"{"values":[{"id":1}],"isLastPage":false,"nextPageStart":123}"#,
                )
            } else {
                Canned::json(200, r#"{"values":[{"id":2}],"isLastPage":true}"#)
            }
        });
        assert_eq!(provider(&server).pages("/repos").await.unwrap().len(), 2);
        let invalid = TestServer::start(|_, _| {
            Canned::json(200, r#"{"values":[],"isLastPage":false,"nextPageStart":0}"#)
        });
        assert!(provider(&invalid).pages("/repos").await.is_err());
    }
}
