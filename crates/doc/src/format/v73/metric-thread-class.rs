//! A Thread feature's ISO metric designation can end in its ISO 965-1 tolerance class, as the
//! standard writes it: `"designation": "M8-6g"`, `"M8x1-6H"` or `"M10x1.25-5g6g"`. The class is
//! kept in the thread's callout and refused on the wrong side (e, f, g or h on a shaft, G or H in
//! a hole); the thread's size, groove and fit are those of the size before the hyphen. An older
//! design has no class and needs no step: no command could create one, since the command refused
//! the text (a hand-edited file could carry one, and now builds the size it names). The last
//! release warns about the newer format number and names this change among those it doesn't
//! know (`crates/doc/src/newer_file.rs`). It fails such a Thread ("unknown metric thread
//! `M8-6g`"), so a modelled one cuts no groove, builds every other feature, and saves the text
//! and the format number back as they came.
