//! Drawing-owned sketch fills retain exact complete outer-plus-holes curve
//! identities, explicit solid/hatch paint and their last admitted region for
//! visible lost-boundary retention after save/reopen. Pattern snapshots use the
//! drawing-global ID namespace. Older drawings have no fills or patterns; serde
//! supplies empty lists. One pending drawing-format change, no migration.
