import type { DynamicImportThemeRegistration, ThemeRegistration } from 'shiki';
import { editorColors, withEditorColors } from '../diff/monaco/theme';
import { beardedArcBlueberry } from './bearded';
import { darcula } from './darcula';
import { useTheme } from './store';
import { type SHIKI_BUNDLED_THEMES, THEME_IDS, THEMES, type ThemeDef, type ThemeId } from './themes';

/** The Shiki themes the app themes use, each imported by name: Shiki's registry (shiki/themes)
 * would bundle every theme it ships, aurora-x (GPL-3.0) among them (docs/licensing.md). */
export const SHIKI_THEMES: Record<(typeof SHIKI_BUNDLED_THEMES)[number], DynamicImportThemeRegistration> = {
  'dark-plus': () => import('shiki/themes/dark-plus.mjs'),
  'light-plus': () => import('shiki/themes/light-plus.mjs'),
  monokai: () => import('shiki/themes/monokai.mjs'),
  dracula: () => import('shiki/themes/dracula.mjs'),
  'one-dark-pro': () => import('shiki/themes/one-dark-pro.mjs'),
  'solarized-dark': () => import('shiki/themes/solarized-dark.mjs'),
  'solarized-light': () => import('shiki/themes/solarized-light.mjs'),
  'github-dark-default': () => import('shiki/themes/github-dark-default.mjs'),
  nord: () => import('shiki/themes/nord.mjs'),
};

/**
 * The first theme for each editor theme: its overlay is the one registered under the base name.
 * Two built-ins can share a Shiki base (GitNexia Dark and GitHub Dark both use
 * `github-dark-default`); the others get a name per overlay.
 */
const canonicalByEditorTheme = new Map<string, ThemeId>();
for (const id of THEME_IDS) {
  const editorTheme = THEMES[id].editorTheme;
  if (!canonicalByEditorTheme.has(editorTheme)) canonicalByEditorTheme.set(editorTheme, id);
}

/**
 * Every theme's Shiki registration with its per-theme overlay (monaco/theme.ts) layered on, for
 * the highlighter's `themes` option. One registration per editor theme: two app themes can share
 * a Shiki base (GitNexia Dark and GitHub Dark do), and the first one's overlay is the base name's;
 * the other gets a name per overlay (`editorThemeName`). They all load up front because
 * `shikiToMonaco` defines Monaco themes only for themes already loaded when it runs, and wraps
 * `monaco.editor.setTheme` once: a theme loaded later would never reach Monaco.
 */
export function editorThemeRegistrations(): Promise<ThemeRegistration>[] {
  return [...canonicalByEditorTheme].map(async ([editorTheme, id]) => {
    const def = THEMES[id];
    const base: ThemeRegistration =
      def.editorTheme === darcula.name
        ? darcula
        : def.editorTheme === beardedArcBlueberry.name
          ? beardedArcBlueberry
          : (await SHIKI_THEMES[def.editorTheme as keyof typeof SHIKI_THEMES]()).default;
    return { ...withEditorColors(base, editorColors(def)), name: editorTheme };
  });
}

/** The Shiki theme name the tokens are coloured with: `def.editorTheme` (a custom theme uses its
 * base's: the highlight worker only knows the built-in names). */
export const currentTokenTheme = (): string => useTheme.getState().def.editorTheme;

/** 32-bit FNV-1a, in base 36. */
function fnv(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(36);
}

/** Between a custom theme's base editor theme and its hash: Monaco takes only `[a-z0-9-]` in a
 * theme name (#14), and no built-in editor theme name has a `--`. */
const SEP = '--';

/** `def`'s Monaco theme name: the base's own for the first built-in that uses it; a name per
 * overlay (`base--hash`) for a custom theme and for another built-in that tints the editor
 * differently, so a colour edit is a new name (shiki.ts `defineEditorTheme` defines it). */
export const editorThemeName = (def: ThemeDef): string =>
  !def.base && canonicalByEditorTheme.get(def.editorTheme) === def.id
    ? def.editorTheme
    : `${def.editorTheme}${SEP}${fnv(JSON.stringify(editorColors(def)))}`;

export const currentEditorTheme = (): string => editorThemeName(useTheme.getState().def);

/** The longest Monaco lags a custom theme's colour edits (a picker being dragged). */
export const EDITOR_THEME_THROTTLE_MS = 150;

const family = (name: string) => name.split(SEP)[0];

/** Calls `setTheme` now and whenever the app theme's editor theme changes: at once for another
 * theme, and at most every `EDITOR_THEME_THROTTLE_MS` while one theme's colours change (the last
 * change always lands; one timeout, none while idle). */
export function bindEditorTheme(setTheme: (name: string) => void): () => void {
  let last = '';
  let timer: ReturnType<typeof setTimeout> | null = null;
  const push = () => {
    const name = currentEditorTheme();
    if (name === last) return false;
    last = name;
    setTheme(name);
    return true;
  };
  const coolDown = () => {
    timer = setTimeout(() => {
      timer = null;
      if (push()) coolDown();
    }, EDITOR_THEME_THROTTLE_MS);
  };
  const changed = () => {
    const name = currentEditorTheme();
    if (name === last) return;
    if (family(name) !== family(last)) {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      push();
    } else if (timer === null) {
      push();
      coolDown();
    }
  };
  push();
  const off = useTheme.subscribe(changed);
  return () => {
    off();
    if (timer !== null) clearTimeout(timer);
  };
}

/** Calls `setTheme` now and whenever the token theme (`currentTokenTheme`) changes. */
export function bindTokenTheme(setTheme: (name: string) => void): () => void {
  let last = '';
  const apply = () => {
    const name = currentTokenTheme();
    if (name === last) return;
    last = name;
    setTheme(name);
  };
  apply();
  return useTheme.subscribe(apply);
}
