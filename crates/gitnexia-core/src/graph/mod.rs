//! The commit graph's lane layout: `gitnexia_graph`'s (crates/gitnexia-graph), re-exported, and the
//! conversions between gix's object ids and its own.

#[cfg(any(test, feature = "testing"))]
pub use gitnexia_graph::check_continuity;
pub use gitnexia_graph::layout;
pub use gitnexia_graph::{
    AsciiRow, GraphRow, Half, LANE_COLORS, Layout, LayoutNode, NodeKind, Oid, Parent, Segment,
    render_ascii,
};

/// gix's id as the graph crate's (SHA-1 only, as this gix build).
pub fn to_oid(id: gix::ObjectId) -> Oid {
    Oid::from_bytes_or_panic(id.as_bytes())
}

/// The graph crate's id as gix's.
pub fn to_gix(id: Oid) -> gix::ObjectId {
    gix::ObjectId::from_bytes_or_panic(&id.0)
}

#[cfg(test)]
mod tests;
