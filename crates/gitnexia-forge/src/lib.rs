//! Forge integrations. #1: Gravatar avatars with a disk cache. #4: the forge providers (GitLab,
//! GitHub, Forgejo) over personal access tokens, implementing `gitnexia_core::forge`'s contract.

pub mod gravatar;

// --- 4A T3 ---
pub mod tokens;
// --- 4A T2 ---
pub mod avatar_cache;
pub mod endpoints;
pub mod http;
// --- 5A T2 ---
pub mod images;
// --- end 5A T2 ---
pub mod known_names;
pub mod pipelines;
#[cfg(test)]
pub(crate) mod test_server;
pub mod time;
/// GitNexia's own releases (the update check).
pub mod updates;
/// The contract these implement (core's, re-exported for callers of this crate).
pub use gitnexia_core::forge;
// --- end 4A T2 ---
// --- 4A T8 ---
pub mod gitlab;
// --- end 4A T8 ---
// --- 4A T9 ---
pub mod github;
// --- end 4A T9 ---
pub mod forgejo;
// --- 4A T10 ---
pub mod connector;
pub mod detect;
// --- end 4A T10 ---
pub mod azure_devops;
pub mod bitbucket;

#[cfg(test)]
mod ssh_tests {
    use super::*;
    use crate::endpoints::HostEndpoints;
    use crate::test_server::{Canned, TestServer};
    use forge::ForgeProvider;
    use gitnexia_core::redact::Secret;

    #[tokio::test]
    async fn ssh_uploads_use_user_key_endpoints_and_surface_token_refusals() {
        let server = TestServer::start(|n, _| {
            if n < 4 {
                Canned::json(201, r#"{"id":7}"#)
            } else {
                Canned::json(
                    403,
                    r#"{"message":"SSH key management is not allowed","errors":[{"message":"SSH key management is not allowed"}]}"#,
                )
            }
        });
        let endpoints = |prefix| HostEndpoints {
            api: format!("{}/{prefix}", server.base),
            web: format!("{}/{prefix}", server.base),
            avatars: None,
        };
        let providers: Vec<Box<dyn ForgeProvider>> = vec![
            Box::new(github::GitHubProvider::new(
                "github.example.com",
                &endpoints("github"),
                Secret::new("fake-token"),
                None,
            )),
            Box::new(gitlab::GitLabProvider::new(
                "gitlab.example.com",
                &endpoints("gitlab"),
                Secret::new("fake-token"),
                None,
            )),
            Box::new(forgejo::ForgejoProvider::new(
                "forgejo.example.com",
                &endpoints("forgejo"),
                Secret::new("fake-token"),
                None,
            )),
            Box::new(bitbucket::BitbucketProvider::new(
                "bitbucket.example.com",
                &endpoints("bitbucket"),
                Secret::new("fake-token"),
                None,
            )),
        ];
        for provider in &providers {
            provider
                .add_ssh_key("GitNexia - Work", "ssh-ed25519 fictional-public-key")
                .await
                .unwrap();
            assert_eq!(provider.take_request_stats().sent, 1);
        }
        let heads = server.heads.lock().unwrap().clone();
        for (head, path) in heads.iter().zip([
            "/github/user/keys",
            "/gitlab/user/keys",
            "/forgejo/user/keys",
            "/bitbucket/rest/ssh/1.0/keys",
        ]) {
            assert!(head.starts_with(&format!("post {path} ")));
            assert!(head.contains("authorization: bearer "));
        }
        let bodies: Vec<serde_json::Value> = server
            .bodies
            .lock()
            .unwrap()
            .iter()
            .map(|b| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(bodies[0]["key"], "ssh-ed25519 fictional-public-key");
        assert_eq!(bodies[1]["usage_type"], "auth");
        assert_eq!(bodies[2]["read_only"], false);
        assert_eq!(bodies[3]["text"], "ssh-ed25519 fictional-public-key");
        for provider in &providers {
            assert!(
                provider
                    .add_ssh_key("GitNexia - Work", "ssh-ed25519 fictional-public-key")
                    .await
                    .is_err()
            );
        }
    }
}
