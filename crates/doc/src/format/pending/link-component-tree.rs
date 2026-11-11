//! A linked component's link can save `tree`: the components and occurrences it keeps for the
//! linked design's own components and occurrences, by the ids they have in that design
//! (`{"components": {"3": 12}, "occurrences": {"4": 13}}`). A linked design that has
//! components is then a linked subassembly here, and Update Links follows its components
//! through these ids (#5526). An
//! older design's links have no `tree` and read unchanged: each keeps every body of its linked
//! design in the one linked component. The last release opens a design that has one with a
//! warning about the newer format, shows those components and occurrences as ordinary ones,
//! and saves `tree` back as it came (`crates/doc/src/newer_file.rs`).
