import { act, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const gitIdentity = vi.fn();
const saveProfile = vi.fn(async () => null);
vi.mock('../../api/client', () => ({
  api: { saveSettings: vi.fn(async () => null), saveProfile: (...a: Parameters<typeof saveProfile>) => saveProfile(...a), gitIdentity: (...a: unknown[]) => gitIdentity(...a) },
  errorMessage: String,
}));

await import('./gitIdentity');
const { EMPTY_PROFILE, useAppState } = await import('../../app/state');
const { useRuntime } = await import('../../app/runtime');
const { settingDef } = await import('../registry');

function renderCard(ctx: { repoPath: string | null; repoName: string | null; tabId: string | null }) {
  const Control = settingDef('gitIdentity')!.Control!;
  render(<Control ctx={ctx} />);
}

beforeEach(() => {
  gitIdentity.mockReset();
  saveProfile.mockClear();
  useAppState.setState({ profile: { ...EMPTY_PROFILE, name: 'Work' } });
  useRuntime.setState({ tabs: { t1: { status: 'ready', error: null, repo: { id: 7, name: 'orbital', path: '/r/orbital' } as never, graph: null, info: null, lastFetchAt: 0, fetchSkipped: null, limit: null, worktree: '/r/orbital', sidebar: null } as never } });
});

describe('Git identity card', () => {
  it('shows the name, the email with signing, and the repository sentence', async () => {
    gitIdentity.mockResolvedValue({ identity: { name: 'Robin Example', email: 'robin@example.com' }, signing: { format: 'ssh', key: '/keys/robin.pub' } });
    renderCard({ repoPath: '/r/orbital', repoName: 'orbital', tabId: 't1' });
    expect(await screen.findByText('Robin Example')).toBeInTheDocument();
    expect(screen.getByText('robin@example.com · signed (SSH key)')).toBeInTheDocument();
    expect(screen.getByText(/From orbital's git config, with this profile's extra config applied/)).toBeInTheDocument();
    expect(gitIdentity).toHaveBeenCalledWith(7, '/r/orbital');
  });

  it("says when this profile's own name and email win", async () => {
    gitIdentity.mockResolvedValue({ identity: { name: 'Ada Lovelace', email: 'ada@example.com' }, signing: null });
    useAppState.setState({ profile: { ...useAppState.getState().profile, gitName: 'Ada Lovelace', gitEmail: 'ada@example.com' } });
    renderCard({ repoPath: '/r/orbital', repoName: 'orbital', tabId: 't1' });
    expect(await screen.findByText('This profile sets it below.')).toBeInTheDocument();
  });

  it('warns when no identity is set', async () => {
    gitIdentity.mockResolvedValue({ identity: null, signing: null });
    renderCard({ repoPath: '/r/orbital', repoName: 'orbital', tabId: 't1' });
    expect(await screen.findByText(/No name and email set: commits will fail/)).toBeInTheDocument();
  });

  it('without a repository reads the global config, and again on window focus', async () => {
    gitIdentity.mockResolvedValue({ identity: { name: 'Robin Example', email: 'robin@example.com' }, signing: null });
    renderCard({ repoPath: null, repoName: null, tabId: null });
    expect(await screen.findByText(/From your global git config/)).toBeInTheDocument();
    expect(gitIdentity).toHaveBeenCalledWith(null, null);
    await act(async () => { window.dispatchEvent(new Event('focus')); });
    await waitFor(() => expect(gitIdentity).toHaveBeenCalledTimes(2));
  });

  it('refreshes the identity after profile identity changes and profile switches', async () => {
    gitIdentity.mockResolvedValueOnce({ identity: { name: 'First', email: 'first@example.com' }, signing: null })
      .mockResolvedValueOnce({ identity: { name: 'Second', email: 'second@example.com' }, signing: null })
      .mockResolvedValueOnce({ identity: { name: 'Third', email: 'third@example.com' }, signing: null });
    renderCard({ repoPath: null, repoName: null, tabId: null });
    expect(await screen.findByText('First')).toBeInTheDocument();
    act(() => useAppState.getState().updateProfile((p) => ({ ...p, gitName: 'Second', gitEmail: 'second@example.com' })));
    expect(await screen.findByText('Second')).toBeInTheDocument();
    expect(saveProfile).toHaveBeenCalledOnce();
    expect(saveProfile.mock.invocationCallOrder[0]).toBeLessThan(gitIdentity.mock.invocationCallOrder[1]);
    act(() => useAppState.setState({ profile: { ...useAppState.getState().profile, id: 'other' } }));
    expect(await screen.findByText('Third')).toBeInTheDocument();
  });
});
