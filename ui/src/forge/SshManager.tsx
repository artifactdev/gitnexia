import { useEffect, useRef, useState } from 'react';
import { api, errorMessage } from '../api/client';
import { copyText } from '../api/transport';
import type { ForgeAccount } from '../api/gen/ForgeAccount';
import type { SshAction } from '../api/gen/SshAction';
import type { SshState } from '../api/gen/SshState';
import { confirmAction } from '../ui/ConfirmDialog';

export function SshManager({ profile, account }: { profile: string; account: ForgeAccount }) {
  const [state, setState] = useState<SshState | null>(null);
  const [path, setPath] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [upload, setUpload] = useState(true);
  const [config, setConfig] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const alive = useRef(true);
  useEffect(() => {
    let active = true;
    alive.current = true;
    api.ssh(profile, account.id, { kind: 'get' }).then((s) => {
      if (active) { setState(s); setPath(s.key?.privateKey ?? ''); setConfig(s.config); }
    }, (e: unknown) => { if (active) setError(errorMessage(e)); });
    return () => { active = false; alive.current = false; };
  }, [profile, account.id]);

  const run = async (action: SshAction, register = false) => {
    setBusy(true); setError(''); setNotice('');
    let registering = action.kind === 'upload';
    try {
      const s = await api.ssh(profile, account.id, action);
      if (!alive.current) return;
      setState(s);
      if (['get', 'select', 'generate', 'useSystem'].includes(action.kind)) setPath(s.key?.privateKey ?? '');
      if (action.kind === 'get' || action.kind === 'saveSystemConfig') setConfig(s.config);
      setPassphrase('');
      if (register) { registering = true; await api.ssh(profile, account.id, { kind: 'upload' }); }
      if (alive.current) setNotice(registering ? 'Public key registered with this forge account.' : action.kind === 'get' ? 'SSH settings loaded.' : 'SSH settings saved.');
    } catch (e) {
      if (alive.current) {
        setPassphrase('');
        setError(`${errorMessage(e)}${registering ? ' The local key is kept. Copy the public key and register it in the forge settings if the token or server does not allow uploads.' : ''}`);
      }
    } finally { if (alive.current) setBusy(false); }
  };
  const confirmed = async (action: SshAction, label: string) => {
    if (await confirmAction({
      title: label, confirmLabel: label, arm: `Click again to ${label.toLowerCase()}`,
      body: 'This changes SSH settings for all applications, not just GitNexia. Existing private keys are never deleted.',
      caption: 'Affects all applications. SSH configuration edits keep a backup beside the file.', danger: true,
    })) await run(action);
  };
  const copy = async () => {
    if (!state?.key) return;
    try { await copyText(state.key.publicKey); setError(''); setNotice('Public key copied.'); }
    catch (e) { setError(errorMessage(e)); }
  };
  const browse = async () => {
    setBusy(true); setError('');
    try { const selected = await api.pickSshKey(); if (alive.current && selected) setPath(selected); }
    catch (e) { if (alive.current) setError(errorMessage(e)); }
    finally { if (alive.current) setBusy(false); }
  };
  return (
    <section className="account-ssh" aria-label={`SSH for @${account.user.username}`}>
      <h4>SSH identity for this account and profile</h4>
      <p className="dim">HTTPS remotes use the account token. These settings apply to matching SSH remotes only; commit name and email are unchanged.</p>
      <p>{state ? state.key ? 'Dedicated key (system keys and agent bypassed)' : 'System SSH (existing configuration and agent)' : 'Loading SSH settings...'}</p>
      <div className="account-ssh-status" aria-live="polite">
        {error ? <p role="alert" className="setting-error">{error}</p> : <p role="status">{notice || '\u00a0'}</p>}
      </div>
      {state && <>
        <p className="dim">A dedicated key uses isolated OpenSSH configuration: no system Host aliases, proxy settings, custom host-key policies or other identities. Use an SSH URL with the real forge hostname and port. Normal known-host verification stays enabled. Choose System SSH if you need your existing routing.</p>
        <div className="account-ssh-actions">
          <label className="account-ssh-path">Private key path
            <input value={path} onChange={(e) => setPath(e.target.value)} disabled={busy} placeholder="/absolute/path/to/private_key" autoComplete="off" />
          </label>
          <button type="button" disabled={busy} onClick={() => void browse()}>Browse private key</button>
        </div>
        <label>Passphrase (optional; not saved)
          <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} disabled={busy} autoComplete="new-password" />
        </label>
        <label className="account-ssh-check"><input type="checkbox" checked={upload} onChange={(e) => setUpload(e.target.checked)} disabled={busy} /> Register public key using this account's token</label>
        <div className="account-ssh-actions">
          <button type="button" disabled={busy || !path.trim()} onClick={() => void run({ kind: 'select', path: path.trim(), passphrase }, upload)}>Use existing key</button>
          <button type="button" disabled={busy} onClick={() => void run({ kind: 'generate', passphrase }, upload)}>{account.kind === 'azuredevops' ? 'Generate RSA key' : 'Generate Ed25519 key'}</button>
          <button type="button" disabled={busy || !state.key} onClick={() => void run({ kind: 'useSystem' })}>Use System SSH</button>
        </div>
        <p className="dim">Generated keys stay local, including after removing this account or profile. Switching keys never deletes or revokes an old key.</p>
        {state.key && <div>
          <p className="account-ssh-fingerprint">{state.key.fingerprint}</p>
          <label>Public key<textarea readOnly rows={3} value={state.key.publicKey} /></label>
          <div className="account-ssh-actions">
            <button type="button" disabled={busy} onClick={() => void copy()}>Copy public key</button>
            <button type="button" disabled={busy} onClick={() => void run({ kind: 'upload' })}>Register public key</button>
            <button type="button" disabled={busy} onClick={() => void api.openUrl(state.keyPage).catch((e: unknown) => setError(errorMessage(e)))}>Open forge SSH settings</button>
          </div>
          <p className="dim">The token needs permission to manage this user's SSH keys. Server policy or token type may require manual registration. Only the public key is uploaded.</p>
          {account.kind === 'github' && <p className="dim">GitHub classic PATs need write:public_key. Repository write permission alone is not enough.</p>}
        </div>}
        <details>
          <summary>System SSH configuration (all applications)</summary>
          <p className="dim">Git global core.sshCommand: <code>{state.globalCommand ?? 'Not set'}</code></p>
          <button type="button" className="danger" disabled={busy || state.globalCommand === null} onClick={() => void confirmed({ kind: 'clearGlobalCommand', expected: state.globalCommand ?? '' }, 'Remove global SSH command')}>Remove global SSH command</button>
          {state.environmentCommand && <p className="dim">GIT_SSH_COMMAND / GIT_SSH: <code>{state.environmentCommand}</code>. Change or unset this in the launching shell or OS environment; dedicated keys override it inside GitNexia.</p>}
          <label>{state.configPath}<textarea rows={10} value={config} onChange={(e) => setConfig(e.target.value)} disabled={busy} spellCheck={false} /></label>
          <div className="account-ssh-actions">
            <button type="button" className="danger" disabled={busy || config === state.config} onClick={() => void confirmed({ kind: 'saveSystemConfig', contents: config, expected: state.config }, 'Save system SSH configuration')}>Save system SSH configuration</button>
            <button type="button" className="danger" disabled={busy || !state.config} onClick={() => void confirmed({ kind: 'saveSystemConfig', contents: '', expected: state.config }, 'Clear system SSH configuration')}>Clear system SSH configuration</button>
            <button type="button" disabled={busy} onClick={() => void run({ kind: 'get' })}>Reload</button>
          </div>
          <p className="dim">Edits preserve a backup named config.gitnexia-backup-* beside the file. Clearing empties only this file, not included files, the agent or any private keys.</p>
        </details>
      </>}
    </section>
  );
}
