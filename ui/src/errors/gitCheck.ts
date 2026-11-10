import { create } from 'zustand';
import { api } from '../api/client';
import type { GbError } from '../api/gen/GbError';
import { isGbError } from './describe';

export type GitProblem =
  | { kind: 'missing' }
  | { kind: 'tooOld'; error: GbError }
  | { kind: 'unavailable'; error: GbError };

interface GitCheck { problem: GitProblem | null; check(): Promise<void>; retry(): Promise<void> }

const spawnFailed = (e: GbError) => e.kind === 'Io' && e.message.startsWith('failed to run git:');
const gitNotFound = (e: GbError) => spawnFailed(e) && /\bos error 2\)/i.test(e.message);

export const useGitCheck = create<GitCheck>((set, get) => ({
  problem: null,
  async check() {
    try {
      await api.appInfo();
      set({ problem: null });
    } catch (e) {
      if (isGbError(e) && e.kind === 'GitTooOld') set({ problem: { kind: 'tooOld', error: e } });
      else if (isGbError(e) && spawnFailed(e)) {
        set({ problem: gitNotFound(e) ? { kind: 'missing' } : { kind: 'unavailable', error: e } });
      } else console.warn('[gitnexia] git check failed; continuing', e);
    }
  },
  /** Re-checks; once git is fine, reloads so boot opens the repos it skipped. */
  async retry() {
    await get().check();
    if (get().problem === null) location.reload();
  },
}));
