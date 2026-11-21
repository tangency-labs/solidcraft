//! Optional drawing-owned sheet sketches (#7082), with drawing-global ids, explicit sheet
//! ownership and the original Sketch payload in paper millimetres. Empty/absent sketches keep
//! the old record shape. No migration: old drawings have no sketches; serde supplies an empty
//! list. A build without this pending change refuses its newer format instead of dropping it.
