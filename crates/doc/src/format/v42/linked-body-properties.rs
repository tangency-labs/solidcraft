//! Linked components retain the last inherited physical material, explicit body look and
//! authored face overrides by imported body slot. Face baselines are optional: an older
//! link is read again on Update Links while preserving local assignments. Authored face
//! overrides stay separate from imported STEP paint, including under a body appearance.
//! Source-only property edits can then update the link while
//! differing local assignments survive. Older links have no baseline: existing assignments
//! are preserved and missing ones can be recovered on Update Links. The last release
//! refuses the newer design format instead of silently discarding these baselines.
