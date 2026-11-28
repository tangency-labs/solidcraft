//! A coil may save `"section": "triangle"`, an equilateral triangle of side `section_size` with its
//! point toward the axis, and may leave out `diameter` when its `axis_ref` is a circular edge, a
//! round face or a flat end face: the diameter is then the edge's, the face's or the end face's
//! round outline's, read as the model is now. A flat end face as `axis_ref` puts the base at the
//! face's centre and the axis along its outward normal. Older designs have none of these, so they
//! build as before and nothing is rewritten. The last release opens such a design with a warning
//! about the newer format number and keeps what it cannot read (`crates/doc/src/newer_file.rs`).
