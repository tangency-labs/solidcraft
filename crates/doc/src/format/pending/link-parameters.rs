//! A linked design (a component's `links` entry) can carry `parameters`: a map of the source
//! design's parameter names to expressions, which the link applies on top of its selected
//! configuration row when it builds the part (`component.insert_design {parameters}`,
//! `component.set_link_parameters`). A generator's part keeps the numbers of its ordering code
//! there, so changing one rebuilds the part within the limits the source declares. An older
//! design has none and needs no step: its links follow the row or the source's saved state as
//! before. The last release warns about the newer format number and names this change among
//! those it doesn't know (`crates/doc/src/newer_file.rs`); it does not read `parameters`, so it
//! builds the part from its row alone, and a save by it drops them.
