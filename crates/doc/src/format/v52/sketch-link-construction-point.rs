//! A sketch link can save a named construction point as its source
//! (`{"type": "construction_point", "name": "Point1"}`): Project makes a linked point where the
//! construction point projects onto the sketch plane, and the point follows it (#12075). An older
//! design has no such link and opens as before. The last release opens a design that has one with
//! a warning about the newer format, keeps the sketch it can't read in its place on the timeline,
//! and saves it back as it came (`crates/doc/src/newer_file.rs`).
