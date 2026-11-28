//! A `frame_member` feature with `groups` can also hold a `corner` type, `corners` overrides, `flip`,
//! `fit_ends`, `coped` and `gap` (#4003): its corners are then finished across its groups, one frame,
//! and an override may name a line of each of two groups. Its groups agree on `mirror`. Older designs
//! need no transformation: a grouped feature without a corner type or overrides builds each group on
//! its own, as before. The last release warns about the newer format and refuses to build such a
//! feature ("not supported yet: frame member groups with joined corners"), keeping its settings.
