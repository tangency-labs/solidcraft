//! A trimmed offset of a chain that includes a free-form curve keeps the sources it was
//! made from, the curves it made and the distance it runs at (`freeform_chain_offsets` in
//! a sketch), so the distance that drives it rebuilds the offset when it changes (#12294).
//!
//! An older design has no such record: its trimmed offsets are ordinary curves with no
//! association, and they open as they were saved. A design with the record is written with
//! the next format number, which the last release warns about: it opens the design, keeps
//! what it can read, and says the format is newer.
