//! Combine: a cut may grow its tools before it cuts, so it leaves a clearance round them (#3774).
//! The Combine feature saves `offset` (a length expression); when the grown tools' edges are
//! rounded, `offset_edges: "round"` and, for a radius other than the offset's, `offset_radius`;
//! when only some tool faces move, `offset_faces` (points on them). Older designs never hold any
//! of these, so they need no step: their cuts keep their result. The last release warns about the
//! newer format number, opens the design and keeps the fields it cannot read.
