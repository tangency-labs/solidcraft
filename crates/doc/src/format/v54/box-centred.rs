//! Designs save `centred: true` on a box placed by its centre (`solid.box` with `center`); its
//! `corner` is then that centre, so a later length, width or height keeps the box centred. Older
//! designs have no such flag: their boxes keep their stored corner and need no migration. The last
//! release opens such a design with a warning about the newer format, reads the centre as the
//! corner and keeps what it can't read (`crates/doc/src/newer_file.rs`).
