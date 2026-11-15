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

/// A blend failure with status `SM_ERR` whose statement is the session's generic one.
fn not_built() -> SolidError {
    use solidcore::error::blend::BlendFailure;
    solidcore::error::Failure::new(
        "blend::fillet_executive::session::finish",
        BlendFailure::Failed("the fillet body is not built"),
    )
    .into_error()
}

/// An error info that noted `comment` with `code`.
fn noting(code: FilletErrorType, comment: &str) -> ErrorInfo<FilletObject> {
    ErrorInfo {
        code: code as i32,
        comment: comment.into(),
        ..ErrorInfo::default()
    }
}

/// A session's failure says what the session noted of the step that failed: two failures the
/// kernel states alike ("the fillet body is not built") are told apart by it.
#[test]
fn a_session_failure_says_what_the_session_noted() {
    let said = |info: &ErrorInfo<FilletObject>| {
        noted_failure(info, &not_built())
            .expect("a blend failure")
            .to_string()
    };
    // a step's own note, as the kernel words it
    assert_eq!(
        said(&noting(
            FilletErrorType::VertexProblem,
            "Fillet Corner Error: Corner Edge Computation Failure"
        )),
        "the blend failed (status 1001): the fillet body is not built: Fillet Corner Error: \
         Corner Edge Computation Failure"
    );
    assert_eq!(
        said(&noting(
            FilletErrorType::BadInt,
            "SmFilletEdge::CalcFilletIntFillet(): failure to calculate intersection of two \
             fillet surfaces"
        )),
        "the blend failed (status 1001): the fillet body is not built: \
         SmFilletEdge::CalcFilletIntFillet(): failure to calculate intersection of two fillet \
         surfaces"
    );
    // without the prefix that says only that the note is the fillet's
    assert_eq!(
        said(&noting(
            FilletErrorType::NotExact,
            "Fillet Error: a kept boss's wall continued down meets no round"
        )),
        "the blend failed (status 1001): the fillet body is not built: a kept boss's wall \
         continued down meets no round"
    );
    // a session that noted nothing, or said nothing of what it noted, adds nothing
    let plain = "the blend failed (status 1001): the fillet body is not built";
    assert_eq!(said(&ErrorInfo::default()), plain);
    assert_eq!(
        said(&noting(
            FilletErrorType::Ok,
            "Fillet Error: an earlier call's"
        )),
        plain
    );
    assert_eq!(
        said(&noting(FilletErrorType::EdgeProblem, "<no comment>\n")),
        plain
    );
    assert_eq!(said(&noting(FilletErrorType::EdgeProblem, "  ")), plain);
    // the class and the status are the failure's own, whatever was noted
    let info = noting(FilletErrorType::BadInt, "why");
    assert_eq!(
        noted_failure(&info, &not_built()),
        Some(Error::BlendFailed {
            class: solidcore::error::ErrorKind::Failed,
            status: 1001,
            phrase: "the fillet body is not built: why".to_owned(),
        })
    );
    // an error that is no blend failure is not one for the note
    let other = SolidError::invalid_input("test", "refused");
    assert_eq!(noted_failure(&info, &other), None);
}
