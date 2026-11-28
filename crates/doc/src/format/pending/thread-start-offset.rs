//! A Thread feature can save `"offset"`, a length expression: how far in from the end it starts at
//! (`from`) the thread starts, leaving that much of the face plain (an unthreaded lead-in on a
//! bolt, a plain stretch below a hole's mouth, a thread band in the middle of a rod). `length` and
//! `up_to` are measured from there; the runout stays at the inner end. An older design has no
//! offset and needs no step: its threads start at their end, as before, and save the same bytes.
//! The last release (0.50.0) warns about the newer format number and names this change among those
//! it doesn't know (`crates/doc/src/newer_file.rs`). Its typed reading leaves the field out, so it
//! builds such a thread from the end (a cosmetic one is marked and drawn there, a modelled one cuts
//! its groove from there), and saving puts `offset` and the format number back as they came.
