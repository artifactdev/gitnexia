// The graph pipeline as the demo uses it: crates/gitnexia-graph through gitnexia-graph-wasm
// (engine/graph.ts loads it). One per repository: a store of commits, and the build.
import type { GraphPayload } from '../../api/gen/GraphPayload';
import type { PresetPreview } from '../../api/gen/PresetPreview';
import type { RevealPayload } from '../../api/gen/RevealPayload';
import type { CommitIn } from '../gen/CommitIn';
import type { GraphRequest } from '../gen/GraphRequest';

export interface GraphLayout {
  /** Adds the commits it doesn't hold yet; how many. */
  addCommits(commits: CommitIn[]): number;
  build(request: GraphRequest): GraphPayload;
  /** The commit ids the build's walk collects, in collection order (`gitnexia_graph::walk_order`). */
  walkOrder(request: GraphRequest): string[];
  /** The reveal panel for `key` under the request's view (`gitnexia_graph::build_reveal`). */
  reveal(request: GraphRequest, key: string, below: boolean): RevealPayload | null;
  /** The view's live summary (`gitnexia_graph::build_preview`). */
  preview(request: GraphRequest): PresetPreview;
  /** The `expanded` key that would make `sha` a row under the view (`gitnexia_graph::build_hidden_owner`). */
  hiddenOwner(request: GraphRequest, sha: string): string | null;
}

export type GraphFactory = () => GraphLayout;
