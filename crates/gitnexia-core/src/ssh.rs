use crate::api::Api;
use crate::error::{GbError, GbErrorKind};
use crate::forge::accounts::ForgeAccount;
use crate::git::GitInvocation;
use crate::redact::Secret;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SshKey {
    pub private_key: String,
    pub public_key: String,
    pub fingerprint: String,
}

#[derive(Debug, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum SshAction {
    Get,
    Select {
        path: String,
        #[ts(type = "string")]
        passphrase: Secret,
    },
    Generate {
        #[ts(type = "string")]
        passphrase: Secret,
    },
    UseSystem,
    Upload,
    SaveSystemConfig {
        #[ts(type = "string")]
        contents: Secret,
        #[ts(type = "string")]
        expected: Secret,
    },
    ClearGlobalCommand {
        #[ts(type = "string")]
        expected: Secret,
    },
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SshState {
    pub key: Option<SshKey>,
    pub key_page: String,
    pub config_path: String,
    pub config: String,
    pub global_command: Option<String>,
    pub environment_command: Option<String>,
}

fn invalid(message: &str) -> GbError {
    GbError::new(GbErrorKind::InvalidInput, message)
}

fn config_quote(value: &str) -> Result<String, GbError> {
    if value.contains(['\n', '\r', '\0']) {
        return Err(invalid(
            "SSH paths must not contain line breaks or NUL characters",
        ));
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    ))
}

fn shell_quote(path: &Path) -> String {
    format!(
        "'{}'",
        crate::platform::fs::to_git_path(path).replace('\'', "'\\''")
    )
}

fn read_config(path: &Path) -> Result<String, GbError> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.len() <= 256 * 1024 => Ok(text),
        Ok(_) => Err(invalid(
            "The SSH configuration is too large to edit in GitNexia",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

fn save_config(path: &Path, expected: &str, contents: &str) -> Result<(), GbError> {
    if contents.len() > 256 * 1024 || contents.contains('\0') {
        return Err(invalid(
            "The SSH configuration is too large or contains NUL characters",
        ));
    }
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(invalid(
            "The SSH configuration is a symbolic link. Edit its target outside GitNexia.",
        ));
    }
    let old = read_config(path)?;
    if old != expected {
        return Err(invalid(
            "The SSH configuration changed outside GitNexia. Reload before saving.",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("The SSH configuration has no parent directory"))?;
    crate::paths::private_dir(parent)?;
    if path.exists() {
        let mut backup = tempfile::Builder::new()
            .prefix("config.gitnexia-backup-")
            .tempfile_in(parent)?;
        backup.write_all(old.as_bytes())?;
        backup.as_file().sync_all()?;
        backup.keep().map_err(|e| {
            GbError::other(format!(
                "Couldn't preserve the SSH configuration backup: {e}"
            ))
        })?;
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(contents.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| GbError::other(format!("Couldn't save the SSH configuration: {e}")))?;
    Ok(())
}

impl Api {
    async fn ssh_global_command(&self) -> Result<Option<String>, GbError> {
        let out = self
            .cli
            .run(
                GitInvocation::new(
                    self.tmp_dir()?,
                    ["config", "--global", "--get", "core.sshCommand"],
                )
                .ok_exit(1),
            )
            .await?;
        Ok(
            (!out.stdout.is_empty())
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()),
        )
    }

    async fn inspect_ssh_key(&self, path: &Path, passphrase: &Secret) -> Result<SshKey, GbError> {
        if !path.is_absolute() || !path.is_file() {
            return Err(invalid(
                "Choose an existing private key using an absolute path",
            ));
        }
        if passphrase.expose().contains(['\r', '\n', '\0']) {
            return Err(invalid(
                "The passphrase must not contain line breaks or NUL characters",
            ));
        }
        let out = self
            .cli
            .run_program(
                Path::new("ssh-keygen"),
                GitInvocation::new(
                    self.tmp_dir()?,
                    [
                        OsString::from("-y"),
                        "-f".into(),
                        path.as_os_str().to_owned(),
                    ],
                )
                .stdin(format!("{}\n", passphrase.expose()).into_bytes())
                .env("SSH_ASKPASS_REQUIRE", "never")
                .detach_terminal(),
            )
            .await?;
        let public_key = String::from_utf8(out.stdout)
            .map_err(|_| invalid("ssh-keygen returned an unreadable public key"))?
            .trim()
            .to_string();
        let mut public = tempfile::NamedTempFile::new_in(self.tmp_dir()?)?;
        public.write_all(public_key.as_bytes())?;
        let out = self
            .cli
            .run_program(
                Path::new("ssh-keygen"),
                GitInvocation::new(
                    self.tmp_dir()?,
                    [
                        OsString::from("-l"),
                        "-E".into(),
                        "sha256".into(),
                        "-f".into(),
                        public.path().as_os_str().to_owned(),
                    ],
                ),
            )
            .await?;
        Ok(SshKey {
            private_key: path.to_string_lossy().into_owned(),
            public_key,
            fingerprint: String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        })
    }

    pub(crate) async fn manage_ssh(
        &self,
        profile_id: &str,
        account_id: &str,
        action: SshAction,
    ) -> Result<SshState, GbError> {
        let profile = self.store.active_profile();
        if profile.id != profile_id {
            return Err(invalid(
                "The profile changed. Reopen SSH settings for the intended profile.",
            ));
        }
        let account = profile
            .forge_accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
            .ok_or_else(|| invalid("The account was removed"))?;
        let home = self
            .home
            .as_ref()
            .ok_or_else(|| invalid("The home directory is unavailable"))?;
        let config_path = home.join(".ssh/config");
        let mut config = read_config(&config_path)?;
        let mut global_command = self.ssh_global_command().await?;
        if self.store.active_profile().id != profile_id {
            return Err(invalid(
                "The profile changed. Reopen SSH settings for the intended profile.",
            ));
        }
        match action {
            SshAction::Get => {}
            SshAction::Select { path, passphrase } => {
                let key = self.inspect_ssh_key(Path::new(&path), &passphrase).await?;
                if account.kind == crate::forge::ForgeKind::AzureDevOps
                    && !key.public_key.starts_with("ssh-rsa ")
                {
                    return Err(invalid("Azure DevOps requires an RSA SSH key"));
                }
                self.store.set_ssh_key(profile_id, account_id, Some(key))?;
            }
            SshAction::Generate { passphrase } => {
                if passphrase.expose().contains(['\n', '\r', '\0']) {
                    return Err(invalid(
                        "The passphrase must not contain line breaks or NUL characters",
                    ));
                }
                let dir = crate::paths::private_dir(&self.data_dir.join("ssh-keys"))?;
                let generated = tempfile::Builder::new().prefix("key-").tempdir_in(dir)?;
                crate::platform::fs::set_mode(generated.path(), 0o700)?;
                let azure = account.kind == crate::forge::ForgeKind::AzureDevOps;
                let path = generated
                    .path()
                    .join(if azure { "id_rsa" } else { "id_ed25519" });
                let mut args = vec![
                    OsString::from("-q"),
                    "-t".into(),
                    (if azure { "rsa" } else { "ed25519" }).into(),
                    "-C".into(),
                    "GitNexia".into(),
                    "-f".into(),
                    path.as_os_str().to_owned(),
                ];
                if azure {
                    args.extend(["-b".into(), "4096".into()]);
                }
                self.cli
                    .run_program(
                        Path::new("ssh-keygen"),
                        GitInvocation::new(generated.path(), args)
                            .stdin(
                                format!("{}\n{}\n", passphrase.expose(), passphrase.expose())
                                    .into_bytes(),
                            )
                            .env("SSH_ASKPASS_REQUIRE", "never")
                            .detach_terminal(),
                    )
                    .await?;
                crate::platform::fs::set_mode(&path, 0o600)?;
                let key = self.inspect_ssh_key(&path, &passphrase).await?;
                self.store.set_ssh_key(profile_id, account_id, Some(key))?;
                let _ = generated.keep();
            }
            SshAction::UseSystem => self.store.set_ssh_key(profile_id, account_id, None)?,
            SshAction::Upload => {
                let key = profile
                    .ssh_keys
                    .get(account_id)
                    .ok_or_else(|| invalid("Select or generate an SSH key first"))?;
                let hub = self
                    .forge
                    .as_ref()
                    .ok_or_else(|| invalid("Forge accounts are unavailable"))?;
                let (_, provider) = hub
                    .provider_for_profile_account(&self.store, profile_id, account.clone())
                    .await?;
                provider
                    .add_ssh_key(&format!("GitNexia - {}", profile.name), &key.public_key)
                    .await?;
            }
            SshAction::SaveSystemConfig { contents, expected } => {
                save_config(&config_path, expected.expose(), contents.expose())?;
                config = contents.expose().to_owned();
            }
            SshAction::ClearGlobalCommand { expected } => {
                if global_command.as_deref() != Some(expected.expose()) {
                    return Err(invalid(
                        "The global SSH command changed. Reload before removing it.",
                    ));
                }
                self.cli
                    .run(GitInvocation::new(
                        self.tmp_dir()?,
                        ["config", "--global", "--unset-all", "core.sshCommand"],
                    ))
                    .await?;
                global_command = None;
            }
        }
        let environment = self.cli.child_env().await;
        let environment_command = match environment {
            Some(vars) => vars
                .iter()
                .find(|(k, _)| k == "GIT_SSH_COMMAND")
                .or_else(|| vars.iter().find(|(k, _)| k == "GIT_SSH"))
                .map(|(_, v)| v.to_string_lossy().into_owned()),
            None => std::env::var("GIT_SSH_COMMAND")
                .ok()
                .or_else(|| std::env::var("GIT_SSH").ok()),
        };
        Ok(SshState {
            key: self
                .store
                .profile(profile_id)
                .and_then(|p| p.ssh_keys.get(account_id).cloned()),
            key_page: key_page(&account),
            config,
            config_path: config_path.to_string_lossy().into_owned(),
            global_command,
            environment_command,
        })
    }

    pub(crate) fn ssh_env(
        &self,
        entry: &crate::ops::OpEntry,
        profile: &crate::settings::Profile,
    ) -> Result<Vec<(OsString, OsString)>, GbError> {
        let mut hosts = BTreeMap::new();
        for remote in entry.remotes() {
            let Some(host) = ssh_host(&remote.url) else {
                continue;
            };
            let account_host =
                crate::remotes::forge_host(&remote.url).unwrap_or_else(|| host.clone());
            let Some(account) = crate::forge::hub::account_for_selected(
                &profile.forge_accounts,
                &account_host,
                remote.account.as_deref(),
            ) else {
                continue;
            };
            let key = profile.ssh_keys.get(&account.id);
            if key.is_some_and(|key| !Path::new(&key.private_key).is_file()) {
                return Err(invalid(
                    "The selected SSH private key is missing. Update this account's SSH settings.",
                ));
            }
            let path = key.map(|key| key.private_key.clone());
            if hosts
                .insert(host, path.clone())
                .is_some_and(|old| old != path)
            {
                return Err(invalid(
                    "This operation selects different SSH keys for the same host. Run its remotes separately.",
                ));
            }
        }
        let hosts: BTreeMap<_, _> = hosts
            .into_iter()
            .filter_map(|(host, key)| key.map(|key| (host, key)))
            .collect();
        if hosts.is_empty() {
            return Ok(Vec::new());
        }
        let mut config = String::new();
        for (host, key) in &hosts {
            config.push_str(&format!("Host {host}\n IdentityFile {}\n IdentitiesOnly yes\n IdentityAgent none\n CertificateFile none\n PreferredAuthentications publickey\n", config_quote(key)?));
        }
        config.push_str(&format!(
            "Host *{}\n",
            hosts.keys().map(|h| format!(" !{h}")).collect::<String>()
        ));
        if let Some(home) = &self.home {
            config.push_str(&format!(
                " Include {}\n",
                config_quote(&home.join(".ssh/config").to_string_lossy())?
            ));
        }
        #[cfg(unix)]
        config.push_str(" Include /etc/ssh/ssh_config\n");
        let mut file = tempfile::Builder::new()
            .prefix("ssh-")
            .tempfile_in(self.tmp_dir()?)?;
        file.write_all(config.as_bytes())?;
        let path = entry.keep_ssh_config(file);
        Ok(vec![
            (
                "GIT_SSH_COMMAND".into(),
                format!("ssh -F {}", shell_quote(&path)).into(),
            ),
            ("GIT_SSH_VARIANT".into(), "ssh".into()),
        ])
    }
}

fn ssh_host(url: &str) -> Option<String> {
    let host = if let Some(rest) = url.strip_prefix("ssh://") {
        rest.split('/')
            .next()?
            .rsplit('@')
            .next()?
            .split(':')
            .next()?
    } else if !url.contains("://") && !url.starts_with(['/', '.']) {
        url.split_once(':')?.0.rsplit('@').next()?
    } else {
        return None;
    };
    (!host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')))
    .then(|| host.to_ascii_lowercase())
}

fn key_page(account: &ForgeAccount) -> String {
    use crate::forge::ForgeKind;
    match account.kind {
        ForgeKind::GitHub => "https://github.com/settings/keys".into(),
        ForgeKind::GitLab => format!("https://{}/-/user_settings/ssh_keys", account.host),
        ForgeKind::Forgejo => format!("https://{}/user/settings/keys", account.host),
        ForgeKind::Bitbucket => format!(
            "{}/plugins/servlet/ssh/account/keys",
            account
                .base_url
                .as_deref()
                .unwrap_or(&format!("https://{}", account.host))
        ),
        ForgeKind::AzureDevOps => format!("https://{}/_usersSettings/keys", account.host),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::OpKind;
    use crate::forge::{ForgeKind, TokenStorage};
    use crate::git::GitCli;
    use crate::log::CommandLog;
    use crate::testing::isolated_git_env;
    use std::sync::Arc;

    fn setup(home: &Path) -> (Api, Arc<CommandLog>, ForgeAccount) {
        let log = Arc::new(CommandLog::new(100));
        let api = Api::new(GitCli::new(log.clone()).with_env(isolated_git_env()), None)
            .with_home(Some(home.to_owned()));
        let account = ForgeAccount {
            id: "gitlab.example.com".into(),
            host: "gitlab.example.com".into(),
            kind: ForgeKind::GitLab,
            base_url: None,
            user: crate::forge::fake::user("Nova"),
            storage: TokenStorage::File,
            version: None,
            version_checked_at: 0,
            added_at: 0,
        };
        let profile = api.store.active_profile();
        api.store
            .update_forge_accounts(&profile.id, |a| a.push(account.clone()))
            .unwrap();
        (api, log, account)
    }

    #[tokio::test]
    async fn ssh_generation_supports_encryption_without_exposing_or_persisting_passphrases() {
        let home = tempfile::tempdir().unwrap();
        let (api, log, account) = setup(home.path());
        let profile = api.store.active_profile();
        for password in ["", "fictional key passphrase"] {
            let state = api
                .manage_ssh(
                    &profile.id,
                    &account.id,
                    SshAction::Generate {
                        passphrase: Secret::new(password),
                    },
                )
                .await
                .unwrap();
            let key = state.key.unwrap();
            assert!(key.public_key.starts_with("ssh-ed25519 "));
            assert!(key.fingerprint.contains("SHA256:"));
            let path = Path::new(&key.private_key);
            assert!(path.is_file());
            #[cfg(unix)]
            {
                assert_eq!(
                    crate::platform::fs::mode(&std::fs::metadata(path).unwrap()) & 0o777,
                    0o600
                );
                assert_eq!(
                    crate::platform::fs::mode(&std::fs::metadata(path.parent().unwrap()).unwrap())
                        & 0o777,
                    0o700
                );
            }
            if !password.is_empty() {
                assert!(
                    api.inspect_ssh_key(path, &Secret::new("incorrect"))
                        .await
                        .is_err()
                );
                assert!(!format!("{:?}", log.entries()).contains(password));
                assert!(
                    !serde_json::to_string(&api.store.active_profile())
                        .unwrap()
                        .contains(password)
                );
            }
            let selected = api
                .manage_ssh(
                    &profile.id,
                    &account.id,
                    SshAction::Select {
                        path: key.private_key.clone(),
                        passphrase: Secret::new(password),
                    },
                )
                .await
                .unwrap()
                .key
                .unwrap();
            assert_eq!(selected, key);
            // A delayed UI save must not overwrite the store-owned identity.
            api.store.save_profile(profile.clone()).unwrap();
            assert_eq!(api.store.active_profile().ssh_keys[&account.id], key);
            api.manage_ssh(&profile.id, &account.id, SshAction::UseSystem)
                .await
                .unwrap();
            assert!(
                path.is_file(),
                "Switching back to system SSH must not delete the key"
            );
        }
    }

    #[tokio::test]
    async fn ssh_identities_exclude_system_keys_and_follow_account_and_profile_selection() {
        let home = tempfile::tempdir().unwrap();
        let (api, log, account) = setup(home.path());
        let first = api.store.active_profile();
        let mut other = account.clone();
        other.id.push_str("#2");
        other.user = crate::forge::fake::user("Orbit");
        api.store
            .update_forge_accounts(&first.id, |a| a.push(other.clone()))
            .unwrap();
        let key = |name: &str| {
            let path = home.path().join(name);
            std::fs::write(&path, "test-only placeholder").unwrap();
            SshKey {
                private_key: path.to_string_lossy().into_owned(),
                public_key: "test public key".into(),
                fingerprint: "test fingerprint".into(),
            }
        };
        let a = key("profile-one");
        let b = key("account two");
        api.store
            .set_ssh_key(&first.id, &account.id, Some(a.clone()))
            .unwrap();
        api.store
            .set_ssh_key(&first.id, &other.id, Some(b.clone()))
            .unwrap();
        std::fs::create_dir(home.path().join(".ssh")).unwrap();
        std::fs::write(home.path().join(".ssh/config"), "Host *\n IdentityFile /nonexistent/competing-system-key\n IdentityAgent /nonexistent/agent\n").unwrap();
        let begin = |selected: Option<String>| {
            let op = api.ops.begin(OpKind::Fetch, None, true);
            op.set_remotes(vec![crate::ops::OpRemote {
                name: "origin".into(),
                url: "ssh://git@gitlab.example.com:7999/team/orbit.git".into(),
                settable: false,
                account: selected,
            }]);
            op
        };
        for (selected, expected) in [(None, &a), (Some(other.id.clone()), &b)] {
            let op = begin(selected);
            let env = api.net_env(op.id).unwrap();
            let config = api.ops.get(op.id).unwrap().ssh_configs_for_test();
            let output = api
                .cli
                .run_program(
                    Path::new("ssh"),
                    GitInvocation::new(
                        home.path(),
                        [
                            OsString::from("-G"),
                            "-F".into(),
                            config.as_os_str().to_owned(),
                            "-p".into(),
                            "7999".into(),
                            "git@gitlab.example.com".into(),
                        ],
                    ),
                )
                .await
                .unwrap();
            let effective = String::from_utf8_lossy(&output.stdout);
            let identities: Vec<_> = effective
                .lines()
                .filter(|s| s.starts_with("identityfile "))
                .collect();
            assert_eq!(
                identities,
                [format!("identityfile {}", expected.private_key)]
            );
            assert!(effective.contains("identityagent none\n"));
            assert!(effective.contains("identitiesonly yes\n"));
            assert!(effective.contains("port 7999\n"));
            assert!(!effective.contains("competing-system-key"));
            assert!(env.iter().any(|(k, _)| k == "GIT_SSH_COMMAND"));
        }
        let conflict = begin(None);
        let mut remotes = conflict.remotes();
        let mut extra = remotes[0].clone();
        extra.account = Some(other.id.clone());
        remotes.push(extra);
        conflict.set_remotes(remotes);
        assert!(api.net_env(conflict.id).is_err());
        api.store.set_ssh_key(&first.id, &other.id, None).unwrap();
        assert!(
            api.net_env(conflict.id).is_err(),
            "Mixed system and dedicated identities on one host must also fail"
        );
        let op = begin(None);
        api.net_env(op.id).unwrap();
        let second = api.store.create_profile("Personal", "#123456").unwrap();
        api.store.switch_profile(&second.id).unwrap();
        api.store
            .update_forge_accounts(&second.id, |a| a.push(account.clone()))
            .unwrap();
        let c = key("profile-two");
        api.store
            .set_ssh_key(&second.id, &account.id, Some(c.clone()))
            .unwrap();
        assert!(
            api.net_env(op.id).is_err(),
            "An operation must not cross a profile switch"
        );
        let second_op = begin(None);
        let env = api.net_env(second_op.id).unwrap();
        assert!(env.iter().any(|(k, _)| k == "GIT_SSH_COMMAND"));
        let config = second_op.ssh_configs_for_test();
        assert!(
            std::fs::read_to_string(config)
                .unwrap()
                .contains(&c.private_key)
        );
        std::fs::remove_file(&c.private_key).unwrap();
        assert!(
            api.net_env(second_op.id).is_err(),
            "A missing explicit key must not fall back"
        );
        let before = log.entries().len();
        let error = api
            .cli
            .run(
                GitInvocation::new(home.path(), ["--version"])
                    .checked_env(api.net_env(second_op.id)),
            )
            .await
            .unwrap_err();
        assert!(error.message.contains("private key is missing"));
        assert_eq!(
            log.entries().len(),
            before,
            "A failed SSH preflight must not start Git"
        );
        api.store
            .set_ssh_key(&second.id, &account.id, None)
            .unwrap();
        assert!(api.net_env(second_op.id).unwrap().is_empty());
        api.store.switch_profile(&first.id).unwrap();
        api.store
            .update_forge_accounts(&first.id, |accounts| {
                accounts.retain(|a| a.id != account.id)
            })
            .unwrap();
        assert!(
            !api.store
                .active_profile()
                .ssh_keys
                .contains_key(&account.id)
        );
        assert!(Path::new(&a.private_key).exists());
    }

    #[test]
    fn ssh_system_edits_keep_backups_and_reject_stale_writes_and_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        save_config(&path, "", "Host original\n").unwrap();
        save_config(&path, "Host original\n", "Host changed\n").unwrap();
        assert!(save_config(&path, "Host original\n", "").is_err());
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("config.gitnexia-backup-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read_to_string(&backups[0]).unwrap(),
            "Host original\n"
        );
        save_config(&path, "Host changed\n", "").unwrap();
        assert_eq!(read_config(&path).unwrap(), "");
        #[cfg(unix)]
        {
            let link = dir.path().join("linked-config");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(save_config(&link, "", "Host injected\n").is_err());
            assert_eq!(read_config(&path).unwrap(), "");
        }
    }

    #[tokio::test]
    async fn ssh_uploads_use_only_the_requested_profiles_account_and_keep_keys_on_failure() {
        use crate::forge::fake::{FakeConnector, FakeProvider, MemTokens};
        let home = tempfile::tempdir().unwrap();
        let (api, _, _) = setup(home.path());
        let connector = Arc::new(FakeConnector::default());
        let provider_a = connector.add(
            "fake-pat-a",
            FakeProvider::new(ForgeKind::GitHub, "github.com"),
        );
        let provider_b = connector.add(
            "fake-pat-b",
            FakeProvider::new(ForgeKind::GitHub, "github.com"),
        );
        let api = api.with_forge(connector, MemTokens::new(TokenStorage::File));
        let hub = api.forge.as_ref().unwrap();
        let first = api.store.active_profile();
        let account_a = hub
            .add_account(
                &api.store,
                "github.com",
                ForgeKind::GitHub,
                Secret::new("fake-pat-a"),
                None,
            )
            .await
            .unwrap()
            .account;
        let a = SshKey {
            private_key: "/fictional/key-a".into(),
            public_key: "ssh-ed25519 fictional-a".into(),
            fingerprint: "fictional-a".into(),
        };
        api.store
            .set_ssh_key(&first.id, &account_a.id, Some(a))
            .unwrap();
        let second = api.store.create_profile("Personal", "#123456").unwrap();
        api.store.switch_profile(&second.id).unwrap();
        let account_b = hub
            .add_account(
                &api.store,
                "github.com",
                ForgeKind::GitHub,
                Secret::new("fake-pat-b"),
                None,
            )
            .await
            .unwrap()
            .account;
        let b = SshKey {
            private_key: "/fictional/key-b".into(),
            public_key: "ssh-ed25519 fictional-b".into(),
            fingerprint: "fictional-b".into(),
        };
        api.store
            .set_ssh_key(&second.id, &account_b.id, Some(b.clone()))
            .unwrap();
        let switched = hub
            .provider_for_profile_account(&api.store, &first.id, account_a.clone())
            .await
            .err()
            .unwrap();
        assert_eq!(switched.kind, GbErrorKind::AuthFailed);
        assert!(
            api.manage_ssh(&first.id, &account_a.id, SshAction::Upload)
                .await
                .is_err()
        );
        api.manage_ssh(&second.id, &account_b.id, SshAction::Upload)
            .await
            .unwrap();
        assert!(provider_a.ssh_keys.lock().unwrap().is_empty());
        assert_eq!(provider_b.ssh_keys.lock().unwrap()[0].1, b.public_key);
        *provider_b.ssh_error.lock().unwrap() = Some(GbError::new(
            GbErrorKind::AuthFailed,
            "Token cannot manage SSH keys",
        ));
        assert!(
            api.manage_ssh(&second.id, &account_b.id, SshAction::Upload)
                .await
                .is_err()
        );
        assert_eq!(api.store.active_profile().ssh_keys[&account_b.id], b);
    }

    #[tokio::test]
    async fn ssh_global_command_removal_is_scoped_and_rejects_stale_values() {
        let home = tempfile::tempdir().unwrap();
        let (mut api, _, account) = setup(home.path());
        let global = home.path().join("gitconfig");
        std::fs::write(
            &global,
            "[core]\n sshCommand = ssh -i fictional-key\n[alias]\n keep = status\n",
        )
        .unwrap();
        let mut env = isolated_git_env();
        env.push(("GIT_CONFIG_GLOBAL".into(), global.as_os_str().to_owned()));
        api.cli = api.cli.clone().with_env(env);
        let profile = api.store.active_profile();
        let stale = api
            .manage_ssh(
                &profile.id,
                &account.id,
                SshAction::ClearGlobalCommand {
                    expected: Secret::new("ssh -i stale"),
                },
            )
            .await;
        assert!(stale.is_err());
        let state = api
            .manage_ssh(
                &profile.id,
                &account.id,
                SshAction::ClearGlobalCommand {
                    expected: Secret::new("ssh -i fictional-key"),
                },
            )
            .await
            .unwrap();
        assert!(state.global_command.is_none());
        let remaining = std::fs::read_to_string(global).unwrap();
        assert!(remaining.contains("keep = status"));
        assert!(!remaining.contains("sshCommand"));
    }

    #[tokio::test]
    async fn ssh_azure_generation_uses_rsa_instead_of_unsupported_ed25519() {
        let home = tempfile::tempdir().unwrap();
        let (api, _, account) = setup(home.path());
        let profile = api.store.active_profile();
        let azure = ForgeAccount {
            id: "dev.azure.com/orbital".into(),
            host: "dev.azure.com/orbital".into(),
            kind: ForgeKind::AzureDevOps,
            ..account
        };
        api.store
            .update_forge_accounts(&profile.id, |a| a.push(azure.clone()))
            .unwrap();
        let key = api
            .manage_ssh(
                &profile.id,
                &azure.id,
                SshAction::Generate {
                    passphrase: Secret::new(""),
                },
            )
            .await
            .unwrap()
            .key
            .unwrap();
        assert!(key.public_key.starts_with("ssh-rsa "));
        assert!(key.fingerprint.starts_with("4096 SHA256:"));
    }

    #[tokio::test]
    async fn ssh_private_key_picker_returns_paths_or_cancellation_without_reading_keys() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".ssh")).unwrap();
        let (api, _, _) = setup(home.path());
        let start = home.path().join(".ssh");
        let path = start.join("fictional-key");
        let chosen = path.clone();
        let api = api.with_file_picker(Arc::new(move |folder| {
            assert_eq!(folder, Some(start.as_path()));
            Some(chosen.clone())
        }));
        let result = api.dispatch(crate::api::Request::PickSshKey).await.unwrap();
        assert_eq!(result, path.to_string_lossy().as_ref());
        let api = api.with_file_picker(Arc::new(|_| None));
        assert!(
            api.dispatch(crate::api::Request::PickSshKey)
                .await
                .unwrap()
                .is_null()
        );
    }
}
