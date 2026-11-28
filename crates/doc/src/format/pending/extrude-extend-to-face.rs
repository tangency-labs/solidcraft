//! An extrude or cut with To Object can save `extend_to_face: true` in its `extent`: the
//! To face's surface is carried on past its edges (a plane without end; a cylinder, cone, sphere
//! or torus along its surface) and the whole profile ends on it, where otherwise a profile that
//! reaches past the face is refused. An older design saves no `extend_to_face` and reads as off,
//! so it keeps the refusal and needs no migration step. The last release warns about the newer
//! format number, opens what it can read and keeps the rest (`crates/doc/src/newer_file.rs`): it
//! refuses such a profile as before, names `extend_to_face` among what it didn't understand, and
//! saves it back as it came.
