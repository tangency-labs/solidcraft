//! A design can hold a `tab_slot` feature (#17422): the tab plates and slot plates (each a body,
//! an optional picking point and a key SolidCraft mints), the next key, the tab count or spacing,
//! the tab width, end distance, clearance and protrusion. Tabs are joined to the tab plates and
//! slots cut through the slot plates. Older designs have no such feature, so nothing in them is
//! rewritten. The last release opens a design that has one with a warning about the newer format,
//! keeps the feature as raw data it cannot read, and saves it back as it was
//! (`crates/doc/src/newer_file.rs`).
