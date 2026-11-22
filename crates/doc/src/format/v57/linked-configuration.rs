//! Linked components can save a source configuration row's numeric id and creation identity.
//! New configuration rows save random creation identities in their table, so undo followed by
//! another row cannot silently retarget a link when the numeric id is reused. Legacy rows keep
//! no identity; old links without a selection continue to follow the source's saved state.
//! The last release warns about the newer format, opens what it can read and keeps the fields
//! it cannot read.
