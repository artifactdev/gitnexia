//! AI commit messages (Settings › AI): the settings a profile keeps, what a provider is detected
//! as, and the trait `gitnexia-ai` implements. Core knows no HTTP and starts no process: the app
//! injects an [`AiProvider`], as it does the forge and avatar providers.

use crate::error::GbError;
use crate::redact::Secret;
use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

/// How the message is made: an OpenAI-compatible HTTP endpoint, or a local command-line tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AiProviderKind {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "cli")]
    Cli,
}

/// One profile's AI commit-message settings. The token is never here: it lives in the token store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiCommitSettings {
    pub enabled: bool,
    pub provider: AiProviderKind,
    /// OpenAI-compatible base, e.g. `https://api.openai.com/v1` or `http://localhost:11434/v1`.
    #[serde(default)]
    pub endpoint: String,
    /// The selected model id.
    #[serde(default)]
    pub model: String,
    /// A command template for a local CLI: `{prompt}` is replaced by the whole prompt, `{model}`
    /// by the model (an argument of its own; no shell is involved).
    #[serde(default)]
    pub command: String,
}

impl Default for AiCommitSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: AiProviderKind::OpenAi,
            endpoint: "https://api.openai.com/v1".into(),
            model: String::new(),
            command: String::new(),
        }
    }
}

/// A local provider or CLI found on this machine, offered by the settings page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiDetected {
    /// Stable within a detection: `openai:<endpoint>` or `cli:<command name>`.
    pub id: String,
    pub name: String,
    pub provider: AiProviderKind,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub models: Vec<String>,
}

/// The generated message, split into the commit box's two fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiCommitMessage {
    pub summary: String,
    pub description: String,
}

/// What a provider needs to make one message.
pub struct AiRequest<'a> {
    pub settings: &'a AiCommitSettings,
    pub token: Option<&'a Secret>,
    pub prompt: &'a str,
    pub worktree: &'a Path,
}

/// The HTTP/CLI side of AI commit messages, implemented by `gitnexia-ai` and injected by the app.
/// Every method is blocking (ureq and `Command`); core runs them on its blocking pool.
pub trait AiProvider: Send + Sync {
    /// The local OpenAI-compatible servers and command-line tools found now.
    fn detect(&self) -> Vec<AiDetected>;
    /// The model ids an OpenAI-compatible endpoint lists.
    fn models(&self, endpoint: &str, token: Option<&Secret>) -> Result<Vec<String>, GbError>;
    /// The raw text the provider returned for `request.prompt`.
    fn generate(&self, request: &AiRequest<'_>) -> Result<String, GbError>;
}
