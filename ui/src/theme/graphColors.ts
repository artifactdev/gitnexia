import { THEMES } from './themes';

/** GitNexia Dark's lane colors, lanes 0–9 (spec §8.2). Themed code reads
 * `useTheme().colors.graph` instead. */
export const GRAPH_COLORS: readonly string[] = THEMES['default-dark'].graph;
