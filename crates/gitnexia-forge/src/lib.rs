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
