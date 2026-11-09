/**
 * GitVolt and GitNex kept their UI preferences (theme mirror, densities, WIP drafts, list prefs, …)
 * in `localStorage` under `gitvolt.*` and `gitnex.*`. The app is GitNexia now and reads
 * `gitnexia.*`; copy the old keys once, before any store reads them. Old keys stay, so a
 * downgrade still finds them.
 */
const LEGACY_PREFIXES = ['gitnex.', 'gitvolt.'] as const;

export function migrateLegacyStoreKeys(): void {
  try {
    for (const prefix of LEGACY_PREFIXES) {
      const old: string[] = [];
      for (let i = 0; i < localStorage.length; i++) {
        const key = localStorage.key(i);
        if (key?.startsWith(prefix)) old.push(key);
      }
      for (const key of old) {
        const next = `gitnexia.${key.slice(prefix.length)}`;
        if (localStorage.getItem(next) === null) {
          const value = localStorage.getItem(key);
          if (value !== null) localStorage.setItem(next, value);
        }
      }
    }
  } catch {
    /* storage unavailable: the defaults apply */
  }
}

migrateLegacyStoreKeys();
