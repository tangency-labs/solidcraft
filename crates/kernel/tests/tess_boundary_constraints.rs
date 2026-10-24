//! How the vendored tessellator adds a face's boundary loops to its constrained triangulation
//! (`PolyBoundary::add_constraints`): a segment crossing one already added is split where they
//! meet, and that ends whatever the loops look like.
//!
//! The vendored crates are outside the workspace, so their own tests never run in CI; the
//! function is reached through the crate's `test-hooks` feature (on only for kernel tests).

use std::sync::mpsc;
use std::time::Duration;
use truck_meshalgo::tessellation::constraint_test_hooks::add_loops;

/// `add_loops` on another thread, or `None` when it hasn't finished within ten seconds.
fn within_time(points: Vec<[f64; 2]>, loops: Vec<Vec<usize>>) -> Option<(usize, usize)> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(add_loops(&points, &loops));
    });
    rx.recv_timeout(Duration::from_secs(10)).ok()
}

/// A segment running back over another at a shallow angle (points on y = 3x + 0.1, to f64
/// rounding): spade's `add_constraint_and_split` never returned on it and grew memory at about
/// 80 MB/s, which froze the app in an interference check (#6294).
#[test]
fn a_segment_running_back_over_another_ends() {
    let points = vec![[0.6, -0.6], [0.1, 0.4], [0.13, 0.49], [0.139, 0.517], [0.092, 0.376]];
    // Two-point loops: each adds just its one segment (there and back).
    let loops = vec![vec![0, 1], vec![1, 2], vec![3, 4]];
    let (vertices, constraints) = within_time(points, loops).expect("adding the boundary finishes");
    assert!(vertices <= 5 + 2 * 256, "{vertices} vertices");
    assert!(constraints >= 3, "{constraints} constraint edges");
}

/// Two segments crossing in the middle are still split where they meet, into four constraint
/// edges through one new vertex (#1648).
#[test]
fn crossing_segments_are_split_where_they_meet() {
    let points = vec![[0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]];
    let loops = vec![vec![0, 1], vec![2, 3]];
    let (vertices, constraints) = within_time(points, loops).expect("adding the boundary finishes");
    assert_eq!(vertices, 5);
    assert_eq!(constraints, 4);
}
