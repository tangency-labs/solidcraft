//! Equivalence checks for SolidCraft's changes to the vendored truck crates.
//!
//! The vendored crates are outside the workspace, so their own tests never run in CI. Each
//! change that replaces an algorithm keeps the old one behind the crate's `test-hooks` feature
//! (on only for these tests) and these tests check the new one gives the same answers.

use truck_meshalgo::analyzers::collision_test_hooks as collision;

/// Mesh collision candidate search (sweep with per-triangle boxes): same collisions, same order.
#[test]
fn mesh_collision_same_as_before() {
    match collision::same_collisions_in_the_same_order(0..200) {
        Ok(n) => assert!(n > 0, "the random meshes produced no collisions to compare"),
        Err(e) => panic!("{e}"),
    }
}

/// Meshes whose triangles all miss the other mesh's box are skipped without a sweep: the sweep
/// would have found no pairs in them.
#[test]
fn mesh_collision_skips_only_meshes_without_pairs() {
    match collision::apart_meshes_have_no_pairs(0..400) {
        Ok(n) => assert!(n > 0, "no random meshes were apart"),
        Err(e) => panic!("{e}"),
    }
}

/// NaN coordinates in a mesh: the collision search goes on without a panic.
#[test]
fn mesh_collision_nan_triangles_do_not_panic() {
    if let Err(e) = collision::nan_triangles_do_not_panic() {
        panic!("{e}");
    }
}
