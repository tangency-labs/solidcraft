//! A Thread's `form` may be `custom`, with a `designation` such as `C10x1.5x90`, which gives
//! the major diameter, the pitch and the included flank angle (mm, mm, degrees, 20° to 90°).
//! The profile is the ISO 68-1 basic profile with its fundamental triangle at that angle:
//! H = P / (2 tan(α/2)), depth 5H/8, flats P/8 and P/4 (the ISO metric profile at 60°). It has
//! no size table and takes no chamfered tip.
//!
//! An older design has no custom thread and needs no step. A Thread saved without a form whose
//! designation starts with `C` keeps its old reading, a refused metric size, as an older
//! Unified one does; the command stores `custom` explicitly.
//!
//! The groove's section reaches past the crest by at most as much as keeps it 0.95 P wide
//! there. Every standard form already reaches less than that, so their grooves are unchanged.
//!
//! The last release warns about the newer format number and names this change among those it
//! doesn't know (`crates/doc/src/newer_file.rs`). Its `ThreadForm` has no `custom`, so it can't
//! read such a Thread. The Thread keeps its place on the timeline as a feature saved by a newer
//! SolidCraft, which cuts nothing and reports that it needs an update. The other features build,
//! though those that pick the thread's faces may fail. Saving writes the Thread and the format
//! number back as they came.
