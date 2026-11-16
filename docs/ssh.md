# SSH management

Open **Settings > Accounts**, then **SSH** on the intended account. Keys are selected separately
for each account within each profile. The repository's selected forge account wins; otherwise
the host's first account is used. HTTPS URLs continue to use PAT authentication. Selecting a key
does not convert a remote URL or change commit authorship.

## System or dedicated key

**System SSH**, the default, keeps the configuration, credential agent and SSH command that Git
normally uses. **Browse private key** opens the native file chooser without importing anything.
**Use existing key** accepts an absolute private-key path and derives its public
key, asking for its passphrase in the form if necessary. Private-key contents are not sent to
the UI. **Generate Ed25519 key** creates a fresh key under GitNexia's app data directory.
Azure DevOps requires RSA; its generation button creates an RSA-4096 key instead.

The passphrase is optional and is never saved. An encrypted key prompts through GitNexia when
a user-started network operation needs it; background operations cannot prompt. Generation
requires `ssh-keygen`, and dedicated transport uses OpenSSH `ssh`.

A dedicated key uses isolated SSH configuration: other identities, SSH certificates and the
agent cannot authenticate instead. The app overrides global/repository `core.sshCommand` and
`GIT_SSH_COMMAND` / `GIT_SSH` for the operation. For covered hosts this also bypasses system Host
aliases, proxies, custom known-host paths and other SSH options. Use the actual forge hostname,
username and port in the remote URL. Normal known-host verification remains enabled; host keys
are not accepted automatically. Choose System SSH if you need your existing aliases or routing.
An operation selecting different identities for the same hostname is refused; fetch/push its
remotes separately. Non-covered hosts still load user/system SSH config, but the operation's
custom SSH executable or wrapper is bypassed.

Switching back to System SSH only removes the app's selection. Replacing a key or removing an
account/profile never deletes local key files or revokes public keys already on a forge.

## Registering the public key

The registration checkbox is initially enabled. With it checked, generation/import first saves
the local selection, then registers the public key using **this account's token**. Disable the
checkbox to work locally only. **Register public key** retries registration separately.

- GitHub uses `POST /user/keys`. Classic PATs need `write:public_key`; repository permissions
  alone do not grant SSH-key management. Other token types must allow this endpoint.
- GitLab uses `POST /user/keys` with authentication-only usage.
- Forgejo/Gitea uses `POST /user/keys`; the token needs user/key write permissions.
- Bitbucket Data Center / Server uses `<installation>/rest/ssh/1.0/keys`, preserving the
  installation's port/context. Token type, version or server policy may prohibit account-key
  changes even when repository operations work.
- Azure DevOps uses manual registration.

If registration is unsupported, denied or unreachable, the error is shown and the local key
is kept. Use **Copy public key** and **Open forge SSH settings** to register it manually.
Only the public key and title leave the computer. A successful upload means the API accepted
the registration, not that repository authorization or SSH connectivity has been tested.
Repeated uploads of an already registered key may be rejected by the forge.

## Editing global configuration

Expand **System SSH configuration (all applications)** to inspect Git's global SSH command,
the launching environment's SSH override, and the user's SSH config file.

- **Remove global SSH command** removes only global `core.sshCommand`, using Git's config CLI.
  Repository config, profile includes and shell environment settings are not removed.
- Edit and **Save system SSH configuration**, or **Clear system SSH configuration** to empty
  that file. Changes keep a private `config.gitnexia-backup-*` file beside it. Included files,
  system-wide config, agents and private keys are never deleted. Symlinked config files must
  be edited outside GitNexia. Reload after external edits; stale writes are refused.
- Environment variables must be changed in the launching shell or OS environment. A dedicated
  account key overrides them inside GitNexia without editing shell startup files.

Every global mutation requires a second, in-place confirmation. These settings affect other
Git/SSH applications as well; no global configuration is changed by key generation or import.
