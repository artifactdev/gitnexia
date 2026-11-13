import { UserRound } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../../api/client';
import type { GitIdentityInfo } from '../../api/gen/GitIdentityInfo';
import { examplePath } from '../../app/osPath';
import { useRuntime } from '../../app/runtime';
import { flushSaves, useAppState } from '../../app/state';
import { profileField } from '../bindings';
import { TextField } from '../frame/fields';
import { registerSettings, registerSettingsPage, type SettingsCtx } from '../registry';

function ExtraGitconfigControl() {
  const path = useAppState((s) => s.profile.extraGitconfig);
  return <TextField id="extraGitconfig" value={path ?? ''} placeholder={examplePath('.gitconfig-work')} onCommit={(v) => useAppState.getState().updateProfile((p) => ({ ...p, extraGitconfig: v || null }))} />;
}

function CommitNameControl() {
  const value = useAppState((s) => s.profile.gitName ?? '');
  return <TextField id="profileGitName" label="Commit name" value={value} placeholder="Your name" onCommit={(v) => useAppState.getState().updateProfile((p) => ({ ...p, gitName: v.trim() || undefined }))} />;
}

function CommitEmailControl() {
  const value = useAppState((s) => s.profile.gitEmail ?? '');
  return <TextField id="profileGitEmail" label="Email" value={value} placeholder="you@example.com" onCommit={(v) => useAppState.getState().updateProfile((p) => ({ ...p, gitEmail: v.trim() || undefined }))} />;
}

function signedText(info: GitIdentityInfo): string | null {
  const format = info.signing?.format;
  if (!info.signing) return null;
  return format === 'ssh' ? 'signed (SSH key)' : format === 'x509' ? 'signed (X.509)' : 'signed (GPG key)';
}

/** Who commits are made as: read-only, fetched on mount and when the tab, the extra config or the window focus changes. */
function GitIdentityControl({ ctx }: { ctx: SettingsCtx }) {
  const repoId = useRuntime((r) => (ctx.tabId ? r.tabs[ctx.tabId]?.repo?.id ?? null : null));
  const worktree = useRuntime((r) => (ctx.tabId ? r.tabs[ctx.tabId]?.worktree ?? null : null));
  const extra = useAppState((s) => s.profile.extraGitconfig);
  const profileId = useAppState((s) => s.profile.id);
  const ownName = useAppState((s) => s.profile.gitName ?? '');
  const ownEmail = useAppState((s) => s.profile.gitEmail ?? '');
  const [info, setInfo] = useState<GitIdentityInfo | null>(null);
  const [focusTick, setFocusTick] = useState(0);
  useEffect(() => {
    const onFocus = () => setFocusTick((n) => n + 1);
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, []);
  useEffect(() => {
    let live = true;
    flushSaves().then(() => api.gitIdentity(repoId, repoId === null ? null : worktree ?? ctx.repoPath)).then((i) => { if (live) setInfo(i); }, () => { if (live) setInfo(null); });
    return () => { live = false; };
  }, [repoId, worktree, ctx.repoPath, profileId, extra, ownName, ownEmail, focusTick]);
  const from = ctx.repoName ? `From ${ctx.repoName}'s git config` : 'From your global git config';
  const signed = info && signedText(info);
  return (
    <div className="git-identity" data-testid="git-identity">
      {info && (info.identity ? (
        <>
          <strong>{info.identity.name}</strong>
          <span className="setting-desc">{[info.identity.email, signed].filter(Boolean).join(' · ')}</span>
        </>
      ) : <span className="setting-desc">No name and email set: commits will fail until you set user.name and user.email.</span>)}
      <span className="setting-desc">{ownName || ownEmail ? 'This profile sets it below.' : `${from}, with this profile's extra config applied.`}</span>
    </div>
  );
}

const offs = [
  registerSettingsPage({ id: 'git-identity', label: 'Git identity', scope: 'profile', order: 10, icon: UserRound, summary: "Who your commits say they're from in this profile." }),
  registerSettings([
    {
      id: 'gitIdentity', label: 'Commits are made as', page: 'git-identity', group: 'Commits', labelFor: 'group',
      keywords: 'user name email author committer signing gpg ssh key identity',
      description: 'Shown from git config, with this profile’s commit name, email and extra config applied.',
      Control: GitIdentityControl,
    },
    {
      id: 'profileGitName', label: 'Commit name for this profile', page: 'git-identity', group: 'This profile',
      keywords: 'user.name author name commits profile',
      description: 'The name this profile’s commits are made as. Empty uses your git config.',
      Control: CommitNameControl,
    },
    {
      id: 'profileGitEmail', label: 'Email for this profile', page: 'git-identity', group: 'This profile',
      keywords: 'user.email author email gravatar picture avatar commits profile',
      description: 'The email this profile’s commits are made as, and the address used for its Gravatar. Empty uses your git config.',
      Control: CommitEmailControl,
    },
    {
      id: 'extraGitconfig', label: 'Extra git config for this profile', page: 'git-identity', group: 'Config',
      keywords: 'include.path identity account signing',
      description: 'A gitconfig file added to every git command of this profile (as include.path): your identity, signing key.',
      Control: ExtraGitconfigControl, binding: profileField('extraGitconfig', null),
    },
  ]),
];
import.meta.hot?.dispose(() => offs.forEach((off) => off()));
