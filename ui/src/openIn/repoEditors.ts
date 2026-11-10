import { api, errorMessage } from '../api/client';
import type { OpenerPayload } from '../api/gen/OpenerPayload';
import { useRuntime, worktreeOf } from '../app/runtime';
import { useAppState } from '../app/state';
import { ICONS } from '../menu/icons';
import { useMenu } from '../menu/menuStore';
import type { MenuRow } from '../menu/types';
import { useToast } from '../ui/toastStore';
import { defaultOpener, openerRows } from './openerRows';
import { configuredOpenerId } from './configuredOpener';
import { loadLastOpener } from './openInPrefs';
import { openWith } from './openers';

// Custom editor settings belong to the target repo, which need not be the active tab.
const detected = new Map<number, { list: OpenerPayload[] | null; error: string | null }>();
const inflight = new Map<number, Promise<void>>();

export function refreshRepoEditors(tabId: string): Promise<void> {
  const repo = useRuntime.getState().tabs[tabId]?.repo?.id;
  if (repo === undefined) return Promise.resolve();
  const pending = inflight.get(repo);
  if (pending) return pending;
  const request = api.listOpeners(repo).then(
    (list) => { detected.set(repo, { list, error: null }); },
    (e: unknown) => { detected.set(repo, { list: null, error: errorMessage(e) }); },
  ).finally(() => {
    inflight.delete(repo);
    useMenu.getState().refresh();
  });
  inflight.set(repo, request);
  return request;
}

function openProject(repo: number, worktree: string, opener: OpenerPayload): void {
  openWith(repo, opener, { worktree, path: '.', line: null, source: null, fallback: null });
}

export async function openRepoInEditor(tabId: string): Promise<void> {
  const rt = useRuntime.getState().tabs[tabId];
  const worktree = worktreeOf(rt);
  if (!rt?.repo || !worktree) {
    useToast.getState().show('The repository is still loading');
    return;
  }
  await refreshRepoEditors(tabId);
  const { list, error } = detected.get(rt.repo.id) ?? { list: null, error: null };
  if (error || !list) {
    useToast.getState().show(error ?? "Couldn't list editors");
    return;
  }
  const profile = useAppState.getState().profile;
  const preferred = configuredOpenerId({ ...profile, activeTab: tabId }) ?? loadLastOpener();
  const opener = defaultOpener(list.filter((o) => o.kind === 'editor'), preferred);
  if (!opener) {
    useToast.getState().show('No supported editor was found on this machine');
    return;
  }
  openProject(rt.repo.id, worktree, opener);
}

export function repoEditorRows(tabId: string): MenuRow[] {
  const status = (label: string, reason: string): MenuRow[] => [{
    kind: 'action', id: 'repo.editors.status', label, icon: ICONS.editorApp,
    tooltip: reason, disabledReason: reason, run: () => {},
  }];
  const rt = useRuntime.getState().tabs[tabId];
  const worktree = worktreeOf(rt);
  if (!rt?.repo || !worktree) return status('Still loading', 'The repository is still loading');
  const { list, error } = detected.get(rt.repo.id) ?? { list: null, error: null };
  if (error) return status("Couldn't list editors", error);
  if (!list) return status('Looking for editors…', 'Still looking for installed editors');
  const editors = list.filter((o) => o.kind === 'editor');
  if (!editors.length) return status('No editor found', 'No supported editor was found on this machine');
  const repo = rt.repo.id;
  return openerRows(editors, (opener) => {
    openProject(repo, worktree, opener);
  }).map((row, i) => ({ ...row, tooltip: `Open the whole project folder in ${editors[i].name}` }));
}
