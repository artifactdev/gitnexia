//! Azure DevOps Services repository access. All API requests stay within one organization.

use crate::endpoints::HostEndpoints;
use crate::http::{ChangeCounter, ClientConfig, HttpClient, REQUEST_TIMEOUT};
use crate::time::unix_now;
use gitnexia_core::avatar::AvatarPayload;
use gitnexia_core::error::{GbError, GbErrorKind};
use gitnexia_core::forge::*;
use gitnexia_core::redact::Secret;
use serde::Deserialize;

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
        unsupported("Azure DevOps pull request workflows")
    }
    fn forks<'a>(&'a self, _p: &'a ForgeProject) -> ForgeFuture<'a, Vec<ForgeProject>> {
        unsupported("Azure DevOps forks")
    }
    fn avatar_for_email<'a>(&'a self, _email: &'a str) -> ForgeFuture<'a, Option<AvatarPayload>> {
        Box::pin(async { Ok(None) })
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
        assert!(p.project_settings(&project).await.is_err());
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
}
