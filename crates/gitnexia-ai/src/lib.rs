//! GitNexia's AI commit-message providers: an OpenAI-compatible HTTP endpoint, or a local
//! command-line tool. Tokens only ever leave in the `Authorization` header; a CLI gets the prompt
//! as an argument, never through a shell.

use gitnexia_core::ai::{AiDetected, AiProvider, AiProviderKind, AiRequest};
use gitnexia_core::error::{GbError, GbErrorKind};
use gitnexia_core::redact::{Secret, redact};
use gitnexia_core::shellenv::{EnvVars, ShellEnv};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const USER_AGENT: &str = concat!("GitNexia/", env!("CARGO_PKG_VERSION"));
const MAX_JSON: u64 = 4 * 1024 * 1024;
const MAX_ERROR: usize = 600;
const DETECT_TIMEOUT: Duration = Duration::from_millis(700);

/// The local OpenAI-compatible servers GitNexia knows to look for.
const LOCAL_SERVERS: &[(&str, &str)] = &[
    ("Ollama", "http://127.0.0.1:11434/v1"),
    ("LM Studio", "http://127.0.0.1:1234/v1"),
    ("llama.cpp", "http://127.0.0.1:8080/v1"),
    ("vLLM", "http://127.0.0.1:8000/v1"),
];

/// The command-line tools offered when found on `PATH`, with the argv template each gets.
const CLIS: &[(&str, &str, &str)] = &[
    ("copilot", "GitHub Copilot CLI", "copilot -s -p {prompt}"),
    ("codex", "OpenAI Codex CLI", "codex exec {prompt}"),
    ("claude", "Claude Code", "claude -p {prompt}"),
    ("gemini", "Gemini CLI", "gemini -p {prompt}"),
    ("llm", "LLM CLI", "llm {prompt}"),
    ("ollama", "Ollama CLI", "ollama run {model} {prompt}"),
];

pub struct Ai {
    agent: ureq::Agent,
    detect_agent: ureq::Agent,
    shell_env: Option<Arc<ShellEnv>>,
}

impl Default for Ai {
    fn default() -> Self {
        Self::new()
    }
}

impl Ai {
    pub fn new() -> Self {
        Self::with_shell_env(None)
    }

    /// The app's login-shell environment (spec §5.3). An app started from the desktop menu has
    /// launchd's bare `PATH`, which misses Homebrew and version managers, so the AI CLIs are
    /// looked up where the user's shell puts them. Until the capture finishes (or if it fails),
    /// the process environment stands in.
    pub fn with_shell_env(shell_env: Option<Arc<ShellEnv>>) -> Self {
        Self {
            agent: agent(Duration::from_secs(45)),
            detect_agent: agent(DETECT_TIMEOUT),
            shell_env,
        }
    }

    /// The captured login-shell environment, when it's ready.
    fn env(&self) -> Option<EnvVars> {
        self.shell_env.as_ref().and_then(|s| s.captured())
    }

    fn models_at(
        &self,
        agent: &ureq::Agent,
        endpoint: &str,
        token: Option<&Secret>,
    ) -> Result<Vec<String>, GbError> {
        let url = format!("{}/models", base(endpoint)?);
        let mut request = agent.get(&url);
        if let Some(token) = token.filter(|t| !t.is_empty()) {
            request = request.header("Authorization", format!("Bearer {}", token.expose()));
        }
        let mut response = request.call().map_err(network)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_JSON)
            .read_to_string()
            .map_err(network)?;
        if !(200..300).contains(&status) {
            return Err(http_error(status, &body));
        }
        let value: Value = serde_json::from_str(&body).map_err(|e| {
            GbError::new(
                GbErrorKind::InvalidInput,
                format!("The AI endpoint sent no readable model list: {e}"),
            )
        })?;
        let mut models: Vec<String> = ["data", "models"]
            .iter()
            .filter_map(|key| value[*key].as_array())
            .flatten()
            .filter_map(|m| {
                m["id"]
                    .as_str()
                    .or_else(|| m["name"].as_str())
                    .or_else(|| m["model"].as_str())
            })
            .map(str::to_string)
            .collect();
        models.sort();
        models.dedup();
        Ok(models)
    }
}

impl AiProvider for Ai {
    fn detect(&self) -> Vec<AiDetected> {
        let mut found = Vec::new();
        let env = self.env();
        let vars = env.as_deref().map(|vars| vars.as_slice());
        for (name, endpoint) in LOCAL_SERVERS {
            let Ok(models) = self.models_at(&self.detect_agent, endpoint, None) else {
                continue;
            };
            found.push(AiDetected {
                id: format!("openai:{endpoint}"),
                name: format!("{name} (local)"),
                provider: AiProviderKind::OpenAi,
                endpoint: (*endpoint).to_string(),
                command: String::new(),
                models,
            });
        }
        for (command, name, template) in CLIS {
            if !on_path(command, path_from(vars).as_deref())
                || (*command == "copilot" && is_aws_copilot(command, vars))
            {
                continue;
            }
            found.push(AiDetected {
                id: format!("cli:{command}"),
                name: format!("{name} ({command})"),
                provider: AiProviderKind::Cli,
                endpoint: String::new(),
                command: (*template).to_string(),
                models: Vec::new(),
            });
        }
        found
    }

    fn models(&self, endpoint: &str, token: Option<&Secret>) -> Result<Vec<String>, GbError> {
        self.models_at(&self.agent, endpoint, token)
    }

    fn generate(&self, request: &AiRequest<'_>) -> Result<String, GbError> {
        match request.settings.provider {
            AiProviderKind::OpenAi => self.generate_openai(request),
            AiProviderKind::Cli => {
                generate_cli(request, self.env().as_deref().map(|vars| vars.as_slice()))
            }
        }
    }
}

impl Ai {
    fn generate_openai(&self, request: &AiRequest<'_>) -> Result<String, GbError> {
        let endpoint = request.settings.endpoint.trim();
        if endpoint.is_empty() {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "Enter an API endpoint first",
            ));
        }
        if request.settings.model.trim().is_empty() {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "Choose a model first",
            ));
        }
        let url = chat_url(endpoint)?;
        let body = json!({
            "model": request.settings.model,
            "messages": [{ "role": "user", "content": request.prompt }],
            "temperature": 0.2,
            "stream": false,
        });
        let mut call = self
            .agent
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(token) = request.token.filter(|t| !t.is_empty()) {
            call = call.header("Authorization", format!("Bearer {}", token.expose()));
        }
        let mut response = call.send(body.to_string()).map_err(network)?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_JSON)
            .read_to_string()
            .map_err(network)?;
        if !(200..300).contains(&status) {
            return Err(http_error(status, &text));
        }
        let value: Value = serde_json::from_str(&text).map_err(|e| {
            GbError::new(
                GbErrorKind::InvalidInput,
                format!("The AI endpoint sent no readable answer: {e}"),
            )
        })?;
        let content = &value["choices"][0]["message"]["content"];
        let text = if let Some(s) = content.as_str() {
            s.to_string()
        } else if let Some(parts) = content.as_array() {
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        } else {
            value["choices"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        if text.trim().is_empty() {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "The AI endpoint returned an empty message",
            ));
        }
        Ok(text)
    }
}

/// The command template as argv: `{prompt}` and `{model}` become one argument each; no shell.
/// With `env` (the login shell's), the tool runs from that environment, so it finds the same
/// programs and credentials as in a terminal.
fn generate_cli(
    request: &AiRequest<'_>,
    env: Option<&[(std::ffi::OsString, std::ffi::OsString)]>,
) -> Result<String, GbError> {
    let template = request.settings.command.trim();
    if template.is_empty() {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "Enter a command first",
        ));
    }
    if !template.contains("{prompt}") {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "The command must contain {prompt}",
        ));
    }
    let model = request.settings.model.trim();
    if template.contains("{model}") && model.is_empty() {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "Choose a model for this command first",
        ));
    }
    let args: Vec<String> = template
        .split_whitespace()
        .map(|part| {
            let part = part.replace("{prompt}", request.prompt);
            if model.is_empty() {
                part
            } else {
                part.replace("{model}", model)
            }
        })
        .collect();
    let Some((program, rest)) = args.split_first() else {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "Enter a command first",
        ));
    };
    let program_name = std::path::Path::new(program)
        .file_stem()
        .and_then(OsStr::to_str);
    if program_name == Some("copilot") && is_aws_copilot(program, env) {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "This copilot command is AWS Copilot, not GitHub Copilot CLI. Install GitHub Copilot CLI and set its executable path in Settings > AI.",
        ));
    }
    let mut command = Command::new(program);
    command
        .args(rest)
        .current_dir(request.worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(env) = env {
        command.env_clear().envs(env.iter().map(|(k, v)| (k, v)));
    }
    if let Some(path) = path_with_program_dir(program, env)? {
        command.env("PATH", path);
    }
    let output = command
        .output()
        .map_err(|e| GbError::new(GbErrorKind::Network, format!("Couldn't run {program}: {e}")))?;
    if !output.status.success() {
        let stderr = redact(String::from_utf8_lossy(&output.stderr).trim());
        // Some CLIs print arguments as escaped strings, including the staged diff in the prompt.
        let prompt_start: String = request.prompt.chars().take(80).collect();
        if !prompt_start.is_empty()
            && (stderr.contains(&prompt_start)
                || stderr.contains(&prompt_start.escape_debug().to_string()))
        {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                format!(
                    "{program} exited with {} (error output contained the prompt)",
                    output.status
                ),
            ));
        }
        let stderr = truncate(&stderr, MAX_ERROR);
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            if stderr.is_empty() {
                format!("{program} exited with {}", output.status)
            } else {
                format!("{program}: {stderr}")
            },
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            format!("{program} returned no message"),
        ));
    }
    Ok(text)
}

/// AWS Copilot uses the same executable name but is unrelated to GitHub Copilot.
fn is_aws_copilot(program: &str, env: Option<&[(std::ffi::OsString, std::ffi::OsString)]>) -> bool {
    let mut command = Command::new(program);
    command
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(env) = env {
        command.env_clear().envs(env.iter().map(|(k, v)| (k, v)));
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout.take(64 * 1024).read_to_end(&mut output);
        let _ = send.send(result.map(|_| output));
    });
    let output = receive.recv_timeout(DETECT_TIMEOUT);
    let _ = child.kill();
    let _ = child.wait();
    output.ok().and_then(Result::ok).is_some_and(|output| {
        String::from_utf8_lossy(&output)
            .contains("Launch and manage containerized applications on AWS")
    })
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .user_agent(USER_AGENT)
        .build()
        .into()
}

/// The OpenAI base: the endpoint as entered, with `/v1` appended when it doesn't name a version.
fn base(endpoint: &str) -> Result<String, GbError> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "Enter an API endpoint first",
        ));
    }
    if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "The API endpoint must start with http:// or https://",
        ));
    }
    Ok(if endpoint.ends_with("/v1") {
        endpoint.to_string()
    } else {
        format!("{endpoint}/v1")
    })
}

fn chat_url(endpoint: &str) -> Result<String, GbError> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.ends_with("/chat/completions") {
        return Ok(endpoint.to_string());
    }
    Ok(format!("{}/chat/completions", base(endpoint)?))
}

fn network(error: ureq::Error) -> GbError {
    GbError::new(
        GbErrorKind::Network,
        format!(
            "Couldn't reach the AI endpoint: {}",
            redact(&error.to_string())
        ),
    )
}

fn http_error(status: u16, body: &str) -> GbError {
    let body = redact(body.trim());
    let body = truncate(&body, MAX_ERROR);
    GbError::new(
        GbErrorKind::InvalidInput,
        if body.is_empty() {
            format!("The AI endpoint answered {status}")
        } else {
            format!("The AI endpoint answered {status}: {body}")
        },
    )
}

fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The `PATH` in `vars` (the captured login shell's), else the process's own.
fn path_from(
    vars: Option<&[(std::ffi::OsString, std::ffi::OsString)]>,
) -> Option<std::ffi::OsString> {
    vars.and_then(|vars| {
        vars.iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| value.clone())
    })
    .or_else(|| std::env::var_os("PATH"))
}

/// A CLI invoked by its full path may still need sibling executables (for example, `env node`).
fn path_with_program_dir(
    program: &str,
    vars: Option<&[(std::ffi::OsString, std::ffi::OsString)]>,
) -> Result<Option<std::ffi::OsString>, GbError> {
    let Some(dir) = std::path::Path::new(program)
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
    else {
        return Ok(None);
    };
    let mut paths: Vec<std::path::PathBuf> = path_from(vars)
        .as_deref()
        .map(std::env::split_paths)
        .map(Iterator::collect)
        .unwrap_or_default();
    if !paths.iter().any(|path| path == dir) {
        paths.insert(0, dir.to_path_buf());
    }
    std::env::join_paths(paths).map(Some).map_err(|e| {
        GbError::new(
            GbErrorKind::InvalidInput,
            format!("Couldn't prepare PATH for {program}: {e}"),
        )
    })
}

fn on_path(command: &str, path: Option<&OsStr>) -> bool {
    let Some(path) = path else { return false };
    for dir in std::env::split_paths(path) {
        let candidates = if cfg!(windows) {
            vec![
                dir.join(command),
                dir.join(format!("{command}.exe")),
                dir.join(format!("{command}.cmd")),
                dir.join(format!("{command}.bat")),
            ]
        } else {
            vec![dir.join(command)]
        };
        for candidate in candidates {
            if !candidate.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if candidate
                    .metadata()
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
                {
                    return true;
                }
            }
            #[cfg(not(unix))]
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitnexia_core::ai::AiCommitSettings;

    #[test]
    fn endpoints_get_their_openai_paths() {
        assert_eq!(
            base("https://api.openai.com").unwrap(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            base("http://localhost:11434/v1/").unwrap(),
            "http://localhost:11434/v1"
        );
        assert_eq!(
            chat_url("https://api.openai.com/v1").unwrap(),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_url("https://api.example/v1/chat/completions").unwrap(),
            "https://api.example/v1/chat/completions"
        );
        assert_eq!(
            base("ftp://example.com").unwrap_err().kind,
            GbErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_cli_command_gets_the_prompt_as_one_argument() {
        let dir = tempfile::tempdir().unwrap();
        let settings = AiCommitSettings {
            enabled: true,
            provider: AiProviderKind::Cli,
            endpoint: String::new(),
            model: String::new(),
            command: "printf {prompt}".into(),
        };
        let request = AiRequest {
            settings: &settings,
            token: None,
            prompt: "hello world",
            worktree: dir.path(),
        };
        assert_eq!(generate_cli(&request, None).unwrap(), "hello world");
        let settings = AiCommitSettings {
            command: "printf {model} {prompt}".into(),
            ..settings
        };
        let request = AiRequest {
            settings: &settings,
            token: None,
            prompt: "hello world",
            worktree: dir.path(),
        };
        assert!(
            generate_cli(&request, None)
                .unwrap_err()
                .message
                .contains("Choose a model")
        );
        let settings = AiCommitSettings {
            command: "printf no-placeholder".into(),
            ..settings
        };
        let request = AiRequest {
            settings: &settings,
            token: None,
            prompt: "hello world",
            worktree: dir.path(),
        };
        assert!(
            generate_cli(&request, None)
                .unwrap_err()
                .message
                .contains("{prompt}")
        );
    }

    #[test]
    fn a_cli_runs_from_the_login_shells_environment() {
        // The app's own PATH (launchd's, from Finder) lacks the tool; the capture has it.
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("copilot");
        std::fs::write(&tool, "#!/bin/sh\nprintf '%s' \"$GREETING\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = std::ffi::OsString::from(format!("{}:/usr/bin:/bin", dir.path().display()));
        let vars = vec![
            (std::ffi::OsString::from("PATH"), path),
            (
                std::ffi::OsString::from("GREETING"),
                std::ffi::OsString::from("hi"),
            ),
        ];
        let settings = AiCommitSettings {
            enabled: true,
            provider: AiProviderKind::Cli,
            endpoint: String::new(),
            model: String::new(),
            command: "copilot {prompt}".into(),
        };
        let request = AiRequest {
            settings: &settings,
            token: None,
            prompt: "ignored",
            worktree: dir.path(),
        };
        let ai = Ai::with_shell_env(Some(ShellEnv::fixed(vars.clone())));
        assert_eq!(
            path_from(ai.env().as_deref().map(|vars| vars.as_slice())),
            Some(vars[0].1.clone())
        );
        assert!(on_path("copilot", path_from(Some(&vars)).as_deref()));
        assert_eq!(generate_cli(&request, Some(&vars)).unwrap(), "hi");
    }

    #[cfg(unix)]
    #[test]
    fn a_full_path_cli_can_find_its_shebang_interpreter() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("copilot");
        let node = dir.path().join("node");
        std::fs::write(&cli, "#!/usr/bin/env node\n").unwrap();
        std::fs::write(&node, "#!/bin/sh\nprintf '%s' 'A commit message'\n").unwrap();
        for path in [&cli, &node] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let vars = vec![(
            std::ffi::OsString::from("PATH"),
            std::ffi::OsString::from("/usr/bin:/bin"),
        )];
        let settings = AiCommitSettings {
            enabled: true,
            provider: AiProviderKind::Cli,
            endpoint: String::new(),
            model: String::new(),
            command: format!("{} -s -p {{prompt}}", cli.display()),
        };
        let request = AiRequest {
            settings: &settings,
            token: None,
            prompt: "ignored",
            worktree: dir.path(),
        };

        assert_eq!(
            generate_cli(&request, Some(&vars)).unwrap(),
            "A commit message"
        );
    }

    #[cfg(unix)]
    #[test]
    fn aws_copilot_is_not_detected_or_given_the_prompt() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("copilot");
        let invoked = dir.path().join("invoked");
        std::fs::write(&tool, format!("#!/bin/sh\nif [ \"$1\" = --help ]; then echo 'Launch and manage containerized applications on AWS.'; else /usr/bin/touch '{}'; fi\n", invoked.display())).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let vars = vec![(
            std::ffi::OsString::from("PATH"),
            dir.path().as_os_str().to_owned(),
        )];
        let ai = Ai::with_shell_env(Some(ShellEnv::fixed(vars.clone())));
        assert!(!ai.detect().iter().any(|d| d.id == "cli:copilot"));
        let settings = AiCommitSettings {
            enabled: true,
            provider: AiProviderKind::Cli,
            endpoint: String::new(),
            model: String::new(),
            command: "copilot -s -p {prompt}".into(),
        };
        let request = AiRequest {
            settings: &settings,
            token: None,
            prompt: "private diff",
            worktree: dir.path(),
        };
        let error = generate_cli(&request, Some(&vars)).unwrap_err();
        assert!(error.message.contains("AWS Copilot"));
        assert!(!error.message.contains("private diff"));
        assert!(!invoked.exists());
        let settings = AiCommitSettings {
            command: format!("{} -s -p {{prompt}}", tool.display()),
            ..settings
        };
        let request = AiRequest {
            settings: &settings,
            token: None,
            prompt: "private diff",
            worktree: dir.path(),
        };
        assert!(
            generate_cli(&request, Some(&vars))
                .unwrap_err()
                .message
                .contains("AWS Copilot")
        );
        assert!(!invoked.exists());
        std::fs::write(&tool, "#!/bin/sh\nif [ \"$1\" = --help ]; then echo 'GitHub Copilot CLI'; else printf '%s' 'A commit message'; fi\n").unwrap();
        assert!(ai.detect().iter().any(|d| d.id == "cli:copilot"));
        assert_eq!(
            generate_cli(&request, Some(&vars)).unwrap(),
            "A commit message"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cli_errors_do_not_echo_the_prompt() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("fake-cli");
        std::fs::write(&tool, "#!/bin/sh\nprintf '%s\\n' \"$1\" >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let settings = AiCommitSettings {
            enabled: true,
            provider: AiProviderKind::Cli,
            endpoint: String::new(),
            model: String::new(),
            command: format!("{} {{prompt}}", tool.display()),
        };
        let request = AiRequest {
            settings: &settings,
            token: None,
            prompt: "private diff\nsecond line",
            worktree: dir.path(),
        };
        let error = generate_cli(&request, None).unwrap_err();
        assert!(error.message.contains("error output contained the prompt"));
        assert!(!error.message.contains("private diff"));
        std::fs::write(&tool, "#!/bin/sh\nprintf '%s\\n' 'unknown command \"private diff\\nsecond line\"' >&2\nexit 1\n").unwrap();
        let error = generate_cli(&request, None).unwrap_err();
        assert!(error.message.contains("error output contained the prompt"));
        assert!(!error.message.contains("private diff"));
        std::fs::write(&tool, "#!/bin/sh\necho 'not authenticated' >&2\nexit 1\n").unwrap();
        assert!(
            generate_cli(&request, None)
                .unwrap_err()
                .message
                .contains("not authenticated")
        );
    }
}
