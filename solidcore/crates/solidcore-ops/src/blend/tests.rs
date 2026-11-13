//! The radius law's data checks (the kernel-calling cases are tests/blend.rs, their own test
//! binary: they hold the process kernel for seconds, which the session's own timing tests must
//! not wait behind).

use super::*;

#[test]
fn laws_are_checked_as_their_data() {
    assert!(law_ok(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]));
    assert!(!law_ok(1, &[0.0, 0.0, 1.0], &[1.0, 2.0]));
    assert!(!law_ok(1, &[0.0, 0.5, 1.0, 1.0], &[1.0, 2.0]));
    assert!(!law_ok(1, &[0.0, 0.0, 1.0, 1.0], &[-1.0, 2.0]));
    assert!(!law_ok(4, &[0.0; 7], &[1.0, 2.0]));
    assert!(law_pinches(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 0.0, 1.0]));
    assert!(!law_pinches(1, &[0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]));
    assert!(!law_pinches(
        2,
        &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
        &[1.0, 0.0, 1.0, 1.0]
    ));
    assert!(law_pinches(
        2,
        &[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        &[1.0, 1.0, 0.0, 1.0, 1.0]
    ));
}

/// The crease trim's refusal (a blend past its faces would crease the face that runs on
/// tangentially from a face it rolls on) names why: its rail leaves the face it rolls on.
#[test]
fn a_crease_across_a_tangent_edge_is_a_rail_leaving_its_face() {
    assert_eq!(
        room_reason(NoRoomReason::CreaseAcrossTangentEdge),
        Some(RoomReason::RailLeavesFace)
    );
}
