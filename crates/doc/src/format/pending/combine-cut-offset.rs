//! Combine: a cut may grow its tools before it cuts, so it leaves a clearance round them (#3774).
//! The Combine feature saves `offset` (a length expression) and, when the grown tools' convex
//! edges are rounded, `offset_edges: "round"`. Older designs never hold either, so they need no
//! step: their cuts keep their result. The last release warns about the newer format number,
//! opens the design and keeps the fields it cannot read.
