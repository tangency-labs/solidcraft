//! An Import kept as B-rep records may mark its records `native: true` when they come directly from
//! SolidCraft's own evaluated bodies (such as a linked design), so opening restores them directly
//! without an SMLib round-trip while keeping exact face numbering. Older designs have no `native`
//! field and need no rewriting; older releases warn about the newer format and keep the records as
//! the file has them.
