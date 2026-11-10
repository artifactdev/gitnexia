// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { OpenerPayload } from '../api/gen/OpenerPayload';
import type { MenuRow } from '../menu/types';

const { listOpeners, openIn } = vi.hoisted(() => ({
  listOpeners: vi.fn<() => Promise<OpenerPayload[]>>(),
  openIn: vi.fn(async () => null),
}));
vi.mock('../api/client', () => ({ api: { listOpeners, openIn }, errorMessage: String }));
const { useRuntime } = await import('../app/runtime');
const { useMenu } = await import('../menu/menuStore');
const { useToast } = await import('../ui/toastStore');
const { refreshRepoEditors, repoEditorRows, openRepoInEditor } = await import('./repoEditors');
const { EMPTY_PROFILE, EMPTY_REPO_SETTINGS, useAppState } = await import('../app/state');
const editors: OpenerPayload[] = [
  { id: 'vscode', name: 'VS Code', kind: 'editor' },
  { id: 'zed', name: 'Zed', kind: 'editor' },
  { id: 'file-manager', name: 'Files', kind: 'fileManager' },
  { id: 'other', name: 'Other…', kind: 'chooser' },
];
let nextRepo = 100;
const actions = (rows: MenuRow[]) => rows.filter((r) => r.kind === 'action');

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  useAppState.setState({ profile: { ...EMPTY_PROFILE } });
  useRuntime.setState({ tabs: {} });
  useRuntime.getState().patch('target', {
    repo: { id: nextRepo++, name: 'orbital', path: '/projects/orbital', worktree: '/projects/orbital' },
    worktree: '/projects/orbital topic',
  });
  listOpeners.mockResolvedValue(editors);
  openIn.mockResolvedValue(null);
  useMenu.getState().close();
  useToast.getState().dismiss();
});

async function load() {
  refreshRepoEditors('target');
  await vi.waitFor(() => expect(actions(repoEditorRows('target'))[0].disabledReason).toBeUndefined());
}

describe('repository editors', () => {
  it('detects editors for the target repo, lists only editors and opens the entire linked worktree', async () => {
    const repo = useRuntime.getState().tabs.target.repo!.id;
    await load();
    expect(listOpeners).toHaveBeenCalledExactlyOnceWith(repo);
    const rows = actions(repoEditorRows('target'));
    expect(rows.map((r) => r.label)).toEqual(['Open in VS Code', 'Open in Zed']);
    rows[1].run();
    expect(openIn).toHaveBeenCalledExactlyOnceWith(repo, {
      worktree: '/projects/orbital topic', path: '.', line: null, opener: 'zed', source: null, fallback: null,
    });
  });

  it('uses the main folder when no linked worktree is selected and surfaces launch failures', async () => {
    useRuntime.getState().patch('target', { worktree: null });
    await load();
    openIn.mockRejectedValueOnce(new Error('Editor could not start'));
    actions(repoEditorRows('target'))[0].run();
    expect(openIn).toHaveBeenCalledWith(expect.any(Number), expect.objectContaining({ worktree: '/projects/orbital' }));
    await vi.waitFor(() => expect(useToast.getState().message).toContain('Editor could not start'));
  });

  it('shows loading, detection errors and no-editor states, and retries detection', async () => {
    expect(actions(repoEditorRows('target'))[0].label).toBe('Looking for editors…');
    listOpeners.mockRejectedValueOnce(new Error('Detection failed'));
    refreshRepoEditors('target');
    await vi.waitFor(() => expect(actions(repoEditorRows('target'))[0].disabledReason).toContain('Detection failed'));
    listOpeners.mockResolvedValueOnce(editors.filter((o) => o.kind !== 'editor'));
    refreshRepoEditors('target');
    await vi.waitFor(() => expect(actions(repoEditorRows('target'))[0].label).toBe('No editor found'));
    await load();
    expect(actions(repoEditorRows('missing'))[0].label).toBe('Still loading');
  });

  it('opens the default editor directly without showing a picker', async () => {
    await openRepoInEditor('target');
    expect(openIn).toHaveBeenCalledWith(expect.any(Number), expect.objectContaining({ opener: 'vscode', path: '.' }));
    expect(useMenu.getState().rows).toBeNull();
  });

  it('remembers a picked editor for the next main-button click', async () => {
    await load();
    actions(repoEditorRows('target'))[1].run();
    openIn.mockClear();
    await openRepoInEditor('target');
    expect(openIn).toHaveBeenCalledExactlyOnceWith(expect.any(Number), expect.objectContaining({ opener: 'zed' }));
  });

  it('prefers the target repository setting over the last-used editor and the profile', async () => {
    await load();
    actions(repoEditorRows('target'))[1].run();
    useAppState.setState({ profile: {
      ...EMPTY_PROFILE,
      editor: { kind: 'opener', id: 'zed' },
      tabs: [{ id: 'target', kind: 'repo', path: '/projects/orbital', alias: null, worktree: null }],
      repos: { '/projects/orbital': { ...EMPTY_REPO_SETTINGS, editor: { kind: 'opener', id: 'vscode' } } },
    } });
    openIn.mockClear();
    await openRepoInEditor('target');
    expect(openIn).toHaveBeenCalledWith(expect.any(Number), expect.objectContaining({ opener: 'vscode' }));
  });

  it('reports detection failure and missing editors without opening a picker', async () => {
    listOpeners.mockRejectedValueOnce(new Error('Detection failed'));
    await openRepoInEditor('target');
    expect(useToast.getState().message).toContain('Detection failed');
    listOpeners.mockResolvedValueOnce([]);
    await openRepoInEditor('target');
    expect(useToast.getState().message).toContain('No supported editor');
    expect(openIn).not.toHaveBeenCalled();
    expect(useMenu.getState().rows).toBeNull();
  });
});
