//! Thread designations and tapped-hole thread strings may hold text expressions, evaluated
//! with the same evaluator as sketch text on each rebuild. Older designs keep literal sizes.
//! Holes can also save auto_thread_diameter to recompute an omitted drill diameter from the
//! evaluated size; older holes default to their stored diameter. The last release opens these
//! designs with a warning and preserves unread data (crates/doc/src/newer_file.rs).
