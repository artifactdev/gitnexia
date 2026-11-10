import { api, errorMessage } from '../api/client';
import type { OpenerPayload } from '../api/gen/OpenerPayload';
import { useRuntime, worktreeOf } from '../app/runtime';
import { ICONS } from '../menu/icons';
import { openMenuAt, useMenu } from '../menu/menuStore';
import type { MenuRow } from '../menu/types';
import { useToast } from '../ui/toastStore';
import { openerRows } from './openerRows';

// Custom editor settings belong to the target repo, which need not be the active tab.
const detected = new Map<number, { list: OpenerPayload[] | null; error: string | null }>();
const inflight = new Set<number>();

export function refreshRepoEditors(tabId: string): void {
  const repo = useRuntime.getState().tabs[tabId]?.repo?.id;
  if (repo === undefined || inflight.has(repo)) return;
  inflight.add(repo);
  void api.listOpeners(repo).then(
    (list) => { detected.set(repo, { list, error: null }); },
    (e: unknown) => { detected.set(repo, { list: null, error: errorMessage(e) }); },
  ).finally(() => {
    inflight.delete(repo);
    useMenu.getState().refresh();
  });
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
    void api.openIn(repo, { worktree, path: '.', line: null, opener: opener.id, source: null, fallback: null })
      .catch((e: unknown) => useToast.getState().show(errorMessage(e)));
  }).map((row, i) => ({ ...row, tooltip: `Open the whole project folder in ${editors[i].name}` }));
}

export function showRepoEditors(tabId: string): void {
  refreshRepoEditors(tabId);
  const anchor = document.querySelector<HTMLElement>('[data-tab-id="' + tabId + '"] .tb-open-editor')
    ?? document.querySelector<HTMLElement>('.tb-open-editor')
    ?? document.activeElement;
  if (anchor) openMenuAt(anchor, repoEditorRows(tabId), undefined, () => repoEditorRows(tabId), 'Open repository in editor');
}
