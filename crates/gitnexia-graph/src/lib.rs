//! The commit graph's pure pipeline (website demo spec §4.2): from plain data (the walked commits,
//! the refs, the worktrees and their WIP) to `GraphPayload`. No git access and no I/O: gitnexia-core
//! feeds it what gix reads, and the website demo (through gitnexia-graph-wasm) what isomorphic-git
//! reads, so both lay a repository out identically.

mod ascii;
mod assemble;
#[cfg(any(test, feature = "testing"))]
mod check;
mod input;
mod labels;
pub mod layout;
mod message;
mod message_refs;
mod oid;
mod payload;
mod pins;
mod presets;
#[cfg(test)]
mod testkit;
mod view;
mod walk;

pub use ascii::{AsciiRow, render_ascii};
pub use assemble::{
    assemble, build, build_hidden_owner, build_preview, build_reveal, graph_tips, walk_order,
};
#[cfg(any(test, feature = "testing"))]
pub use check::check_continuity;
pub use input::{
    CommitIn, ForkFacts, GraphInput, GraphRequest, HeadIn, RefIn, RefKind, RefsIn, StashIn,
    WorktreeIn,
};
pub use layout::{
    GraphRow, Half, LANE_COLORS, Layout, LayoutNode, LayoutState, NodeKind, Parent, Segment, layout,
};
pub use message::split_message;
pub use message_refs::{MAX_MESSAGE_REFS, parse_message_refs};
pub use oid::Oid;
pub use payload::{
    BranchState, BranchView, CollapsedRow, DecidedBy, FoldMark, ForkDetection, ForkPayload,
    GhostBranch, GraphPayload, GraphWorktree, HeadPayload, HostKind, InProgress, PresetPreview,
    RefLabel, RemoteRefLabel, RevealPayload, RevealTag, RowPayload, TagAnnotation, TagRemoteLabel,
    TagRemoteState, ViewPayload, WipCounts, WipPayload,
};
pub use presets::{
    Author, BranchPicks, BranchPreset, CollapsePlacement, Matcher, PresetAction, PresetScope,
    PresetSlot, Rule,
};
pub use view::{ViewSpec, hidden_owner, preview, reveal};
pub use walk::{CommitMeta, WalkOptions, WalkResult, walk, walk_by_date};
