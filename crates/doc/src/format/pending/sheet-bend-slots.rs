//! Saves the native sheet_bend_slots feature, its length expressions, cut-end profile,
//! relief settings and typed references to upstream producing features and bend indices.
//! Older designs have no such feature and need no migration. The last release warns about
//! the newer design format, opens supported features and preserves the unknown feature data.
//! Saved bend selections additionally retain bounded structural source certificates. Earlier
//! experimental definitions without certificates fail closed and require explicit reselection.
