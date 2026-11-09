import { useEffect, useState } from 'react';
import { create } from 'zustand';
import { api, errorMessage } from '../api/client';
import type { ForgeAccountView } from '../api/gen/ForgeAccountView';
import { useModalKeys } from '../app/modalKeys';
import { flushSaves, useAppState } from '../app/state';
import { Avatar } from '../avatars/Avatar';
import { ForgeMark } from '../icons/brands';
import { forgeName } from '../forge/labels';
import { useSettingsUi } from '../settings/schema';

export const PROFILE_COLORS = ['#4d88ff', '#2ece9d', '#f2ca33', '#f25d2e', '#d90171', '#8e00c2', '#15a0bf', '#7bd938'];

export const useProfileDialog = create<{ mode: 'new' | 'rename' | null; open(mode: 'new' | 'rename'): void; close(): void }>((set) => ({
  mode: null,
  open: (mode) => set({ mode }),
  close: () => set({ mode: null }),
}));

/** New profile… / Edit profile… (the profile switcher's menu), spec §14.1. */
export function ProfileDialog() {
  const mode = useProfileDialog((s) => s.mode);
  if (!mode) return null;
  return <ProfileForm mode={mode} key={mode} />;
}

function ProfileForm({ mode }: { mode: 'new' | 'rename' }) {
  const current = useAppState((s) => s.profile);
  const [name, setName] = useState(mode === 'rename' ? current.name : '');
  const [color, setColor] = useState(mode === 'rename' ? current.color : PROFILE_COLORS[1]);
  const [gitName, setGitName] = useState(mode === 'rename' ? current.gitName ?? '' : '');
  const [gitEmail, setGitEmail] = useState(mode === 'rename' ? current.gitEmail ?? '' : '');
  const [previewEmail, setPreviewEmail] = useState(gitEmail.trim());
  const [accounts, setAccounts] = useState<ForgeAccountView[] | null>(null);
  const [accountError, setAccountError] = useState<string | null>(null);
  useEffect(() => {
    // Ask for an avatar once the address settles, not for every character typed.
    const t = setTimeout(() => setPreviewEmail(gitEmail.trim()), 400);
    return () => clearTimeout(t);
  }, [gitEmail]);
  useEffect(() => {
    if (mode !== 'rename') return;
    let live = true;
    api.forgeAccounts().then(
      (found) => { if (live) setAccounts(found); },
      (e) => { if (live) setAccountError(errorMessage(e)); },
    );
    return () => { live = false; };
  }, [mode]);
  const close = useProfileDialog((s) => s.close);
  // `ProfileForm` only mounts while the dialog is open (`ProfileDialog` above), so the trap is
  // simply always active for as long as this component exists; its cleanup (on Escape, Cancel,
  // Create/Save, or unmount) returns focus to the profile switcher button that opened it.
  const ref = useModalKeys<HTMLFormElement>(true, close);
  const editAccounts = () => {
    close();
    useSettingsUi.getState().show();
    useSettingsUi.getState().setPage('accounts');
  };
  const submit = async () => {
    // An empty name is allowed: the switcher then shows just the colour.
    const trimmed = name.trim();
    const commitName = gitName.trim();
    const email = gitEmail.trim();
    if (mode === 'new') await useAppState.getState().createProfile(trimmed, color, commitName, email);
    else {
      useAppState.getState().renameProfile(trimmed, color, commitName, email);
      await flushSaves();
    }
    close();
  };
  return (
    <div className="modal-backdrop" onPointerDown={close}>
      <form
        ref={ref}
        className="modal"
        role="dialog"
        aria-label={mode === 'new' ? 'New profile' : 'Edit profile'}
        onPointerDown={(e) => e.stopPropagation()}
        onSubmit={(e) => { e.preventDefault(); void submit(); }}
      >
        <h2>{mode === 'new' ? 'New profile' : 'Edit profile'}</h2>
        <div className="profile-identity">
          <Avatar name={gitName.trim() || name.trim()} email={previewEmail} size={48} />
          <div className="profile-identity-fields">
            <label>
              Profile name
              <input autoFocus value={name} onChange={(e) => setName(e.target.value)} aria-label="Profile name" aria-describedby="profile-name-hint" />
            </label>
            <span className="profile-name-hint" id="profile-name-hint">Leave it empty to show just the color in the tab bar.</span>
            <label>
              Commit name
              <input value={gitName} onChange={(e) => setGitName(e.target.value)} aria-label="Commit name" placeholder="Your name" aria-describedby="profile-identity-hint" />
            </label>
            <label>
              Email
              <input type="email" value={gitEmail} onChange={(e) => setGitEmail(e.target.value)} aria-label="Email" placeholder="you@example.com" aria-describedby="profile-identity-hint" />
            </label>
            <span className="profile-name-hint" id="profile-identity-hint">Used for commits in this profile. The email's Gravatar shows here when there is one.</span>
          </div>
        </div>
        {mode === 'rename' && (
          <section className="profile-accounts" aria-label="Profile integrations and accounts">
            <h3>Integrations and accounts</h3>
            {accountError && <p className="profile-accounts-error" role="alert">{accountError}</p>}
            {!accounts && !accountError && <p className="dim">Loading accounts…</p>}
            {accounts?.length === 0 && <p className="dim">No integrations or accounts connected.</p>}
            {accounts && accounts.length > 0 && (
              <ul>
                {accounts.map(({ account }) => (
                  <li key={account.id}>
                    <ForgeMark kind={account.kind} size={15} />
                    <span>{forgeName(account.kind)} · @{account.user.username}</span>
                    <span className="dim">{account.baseUrl ?? account.host}</span>
                  </li>
                ))}
              </ul>
            )}
            <button type="button" className="profile-edit-accounts" onClick={editAccounts}>Edit accounts</button>
          </section>
        )}
        <div className="swatches" role="radiogroup" aria-label="Color">
          {PROFILE_COLORS.map((c) => (
            <button key={c} type="button" role="radio" aria-checked={c === color} aria-label={c} className="swatch" style={{ background: c }} onClick={() => setColor(c)} />
          ))}
        </div>
        <div className="modal-actions">
          <button type="button" onClick={close}>Cancel</button>
          <button type="submit">{mode === 'new' ? 'Create' : 'Save'}</button>
        </div>
      </form>
    </div>
  );
}
