//! A Lighten feature saves the sketch profiles it cuts, the wall thickness, the tool radius,
//! and how far the pocket goes: a depth, through all, or up to a point on a face or body.
//! Trim defaults to on, so an older file that omits it keeps the pockets inside the part
//! outline. The tool radius defaults to zero, which leaves corners sharp. The last release
//! refuses the newer design format instead of dropping the feature.
