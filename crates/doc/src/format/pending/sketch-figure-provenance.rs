//! A sketch may save optional `figures`: the points and curves one creation command made as a
//! unit (a rectangle, polygon or slot), with the command that made them and any existing
//! entities the shape was built from. The browser uses them to show the shape instead of its
//! loose points and lines; the solver and the evaluated geometry never read them. An older
//! design has no figures and reads and evaluates exactly as before, and a sketch with no
//! figures saves without the field. The last release warns about the newer format number, opens
//! what it can read, and keeps the figure records it cannot read on save.
