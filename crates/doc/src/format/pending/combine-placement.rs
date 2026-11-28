//! A Combine feature can save `"placement"`: a list of assembly constraints, each `{"type", "target",
//! "tool", "offset"?, "angle"?, "flip"?}`, between a reference on the target body and one on the one
//! tool body (`target` and `tool` are joint origins, as a joint saves its sides). The boolean uses a
//! copy of the tool moved to where the constraints put it, worked out again on every rebuild; the
//! tool body stays where it is (#23542). An older design has no placement and needs no step: its
//! Combines use their tools where they are, as before, and save the same bytes. The last release
//! (0.50.0) warns about the newer format number and names this change among those it doesn't know
//! (`crates/doc/src/newer_file.rs`). Its typed reading leaves the field out, so it combines the tool
//! where the tool body is (a cut there misses the target, or cuts it in the wrong place), and saving
//! puts `placement` and the format number back as they came.
