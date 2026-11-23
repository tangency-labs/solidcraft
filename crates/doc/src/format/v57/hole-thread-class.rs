//! A tapped Hole can save its thread's tolerance class as `thread_class` (`"6H"`, `"5H6H"` or
//! `"2B"`, as written), beside `thread` and `hand` (#16336). It is an annotation for drawings: the
//! drill and a modelled thread don't change with it. An older design has no class and needs no
//! step; a hole without one saves no key. The last release warns about the newer format number,
//! opens what it can read and keeps the rest (`crates/doc/src/newer_file.rs`): it drills and taps
//! such a hole as before, names `thread_class` among what it didn't understand, and saves it back
//! as it came.
