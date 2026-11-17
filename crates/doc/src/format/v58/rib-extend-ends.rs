//! Designs save a rib's `extend`: the rib's curves run their open ends on in their own direction
//! until they meet the body, so the wall reaches the face beyond them (#13019). Older designs have
//! no such field: their ribs end where their curves end, as they did, and need no migration. The
//! last release opens such a design with a warning and keeps what it can't read
//! (`crates/doc/src/newer_file.rs`).
