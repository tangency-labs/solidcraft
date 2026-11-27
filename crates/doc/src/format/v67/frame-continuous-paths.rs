//! Frame members can retain explicitly selected circular arcs and a `continuous` line/arc
//! chain as one physical member. Older designs default to separate members, with the existing
//! all-straight-lines selection unchanged. A release that lacks this change opens the design
//! with a warning and preserves fields it cannot read when saving it.
