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

/// A blend certificate that ran out of its work budget (the kernel's
/// `BlendFailure::NotCertified`) is the operation's own refusal, naming no size: neither a bare
/// status nor a fit.
#[test]
fn a_certificate_past_its_budget_is_refused_as_not_certified() {
    use solidcore::error::{Failure, Reason};
    let e = Failure::new(
        "blend::fillet_section::GnSectionFsg::certify",
        Reason::Blend(BlendFailure::NotCertified {
            halvings: 499,
            stations: 523,
        }),
    )
    .into_error();
    assert_eq!(e.sm_status(), Some(solidcore::error::SM_ERR));
    // the same refusal for a caller that runs the kernel's blend itself (SolidCraft's)
    assert_eq!(
        failure(&e),
        Some(Error::BlendNotCertified {
            halvings: 499,
            stations: 523
        })
    );
    assert_eq!(
        status(e),
        Error::BlendNotCertified {
            halvings: 499,
            stations: 523
        }
    );
}
