//! Regression test for #2012: every `SmStatus` number a [`SolidError::Upstream`] can
//! carry is public in `solidcore::error`, defined once, and equal to upstream's.
//!
//! The expected numbers are copied from the pinned upstream `SmMessages.h` (`SM_SUCCESS`
//! to `SM_ERR_TESS_FAIL_FACE`, lines 45-70) and `SmTypes.h` (`TRUE`, `FALSE`, `UNSURE`),
//! and for the two fork statuses from tangency-labs/smlib's `SmMessages.h` (a759622 and
//! 34318f4). SolidCraft maps these numbers to its own errors, so a moved number must fail
//! here rather than silently change a refusal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::error::{
    SolidError, SM_ERR, SM_ERR_ASSERTVALID_FAILURE, SM_ERR_ASSERT_FAILURE, SM_ERR_AXIS_INSIDE_FACE,
    SM_ERR_BAD_COINCIDENT_VERTICES, SM_ERR_BAD_FIND_DEGEN_PARAM, SM_ERR_BAD_INTERSECTIONS,
    SM_ERR_BAD_SURFACE_POINT, SM_ERR_BAD_TANGENT_DROP, SM_ERR_DEGENERATE_SURFACE, SM_ERR_FATAL,
    SM_ERR_INVALID_INPUT, SM_ERR_LICENSE_EXPIRED, SM_ERR_MESSAGE, SM_ERR_METHOD_FAILURE_QUITING,
    SM_ERR_NON_NULL_OUTPUT_POINTER, SM_ERR_NOTYET_HEAL_FACE, SM_ERR_NOT_CONVERGING,
    SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER, SM_ERR_OFFSET_EDGE_VANISHES,
    SM_ERR_OFFSET_WALLS_TOUCH, SM_ERR_OUTSIDE_OF_DOMAIN, SM_ERR_OUT_OF_MEMORY,
    SM_ERR_TESS_FAIL_FACE, SM_ERR_UNKNOWN, SM_ERR_WARNING, SM_FALSE, SM_SUCCESS, SM_TRUE,
    SM_UNSURE,
};

#[test]
fn issue_2012_status_codes_match_upstream() {
    let codes = [
        ("SM_SUCCESS", SM_SUCCESS, 1000),
        ("SM_ERR", SM_ERR, 1001),
        ("SM_ERR_OUT_OF_MEMORY", SM_ERR_OUT_OF_MEMORY, 1002),
        ("SM_ERR_UNKNOWN", SM_ERR_UNKNOWN, 1003),
        ("SM_ERR_FATAL", SM_ERR_FATAL, 1004),
        ("SM_ERR_ASSERT_FAILURE", SM_ERR_ASSERT_FAILURE, 1005),
        ("SM_ERR_NULL_POINTER", SM_ERR_NULL_POINTER, 1006),
        ("SM_ERR_INVALID_INPUT", SM_ERR_INVALID_INPUT, 1007),
        (
            "SM_ERR_NON_NULL_OUTPUT_POINTER",
            SM_ERR_NON_NULL_OUTPUT_POINTER,
            1008,
        ),
        (
            "SM_ERR_ASSERTVALID_FAILURE",
            SM_ERR_ASSERTVALID_FAILURE,
            1010,
        ),
        ("SM_ERR_OUTSIDE_OF_DOMAIN", SM_ERR_OUTSIDE_OF_DOMAIN, 1020),
        (
            "SM_ERR_NOT_WITHIN_TOLERANCE",
            SM_ERR_NOT_WITHIN_TOLERANCE,
            1021,
        ),
        ("SM_ERR_NOT_CONVERGING", SM_ERR_NOT_CONVERGING, 1022),
        ("SM_ERR_WARNING", SM_ERR_WARNING, 1023),
        ("SM_ERR_MESSAGE", SM_ERR_MESSAGE, 1024),
        ("SM_ERR_AXIS_INSIDE_FACE", SM_ERR_AXIS_INSIDE_FACE, 1025),
        ("SM_ERR_LICENSE_EXPIRED", SM_ERR_LICENSE_EXPIRED, 1026),
        ("SM_ERR_BAD_INTERSECTIONS", SM_ERR_BAD_INTERSECTIONS, 1027),
        (
            "SM_ERR_BAD_COINCIDENT_VERTICES",
            SM_ERR_BAD_COINCIDENT_VERTICES,
            1028,
        ),
        ("SM_ERR_BAD_SURFACE_POINT", SM_ERR_BAD_SURFACE_POINT, 1030),
        ("SM_ERR_BAD_TANGENT_DROP", SM_ERR_BAD_TANGENT_DROP, 1032),
        ("SM_ERR_DEGENERATE_SURFACE", SM_ERR_DEGENERATE_SURFACE, 1034),
        (
            "SM_ERR_BAD_FIND_DEGEN_PARAM",
            SM_ERR_BAD_FIND_DEGEN_PARAM,
            1036,
        ),
        (
            "SM_ERR_METHOD_FAILURE_QUITING",
            SM_ERR_METHOD_FAILURE_QUITING,
            1038,
        ),
        ("SM_ERR_NOTYET_HEAL_FACE", SM_ERR_NOTYET_HEAL_FACE, 1040),
        ("SM_ERR_TESS_FAIL_FACE", SM_ERR_TESS_FAIL_FACE, 1041),
        // the fork's statuses
        ("SM_ERR_OFFSET_WALLS_TOUCH", SM_ERR_OFFSET_WALLS_TOUCH, 1042),
        (
            "SM_ERR_OFFSET_EDGE_VANISHES",
            SM_ERR_OFFSET_EDGE_VANISHES,
            1043,
        ),
        // SmBoolean (SmTypes.h)
        ("SM_FALSE", SM_FALSE, 0),
        ("SM_TRUE", SM_TRUE, 1),
        ("SM_UNSURE", SM_UNSURE, 2),
    ];
    for (name, value, upstream) in codes {
        assert_eq!(value, upstream, "{name}");
    }
}

/// The modules that used to define their own copies re-export the one in `error`
/// (the names are the same items, so this module does not import them unqualified).
mod re_exports {
    use solidcore::intersect::surface_drop_lib::{p1, p2, p3};
    use solidcore::topology::shell;

    #[test]
    fn issue_2012_old_public_paths_keep_their_values() {
        assert_eq!(p1::SM_SUCCESS, 1000);
        assert_eq!(p1::SM_ERR_OUTSIDE_OF_DOMAIN, 1020);
        assert_eq!(p2::SM_ERR_INVALID_INPUT, 1007);
        assert_eq!(p2::SM_ERR_NOT_WITHIN_TOLERANCE, 1021);
        assert_eq!(p2::SM_ERR_BAD_SURFACE_POINT, 1030);
        assert_eq!(p2::SM_ERR_BAD_TANGENT_DROP, 1032);
        assert_eq!(p3::SM_ERR_NOT_CONVERGING, 1022);
        assert_eq!(p3::SM_ERR_DEGENERATE_SURFACE, 1034);
        assert_eq!(p3::SM_ERR_BAD_FIND_DEGEN_PARAM, 1036);
        assert_eq!(shell::SM_TRUE, 1);
        assert_eq!(shell::SM_FALSE, 0);
        assert_eq!(shell::SM_UNSURE, 2);
    }
}

#[test]
fn issue_2012_is_offset_edge_vanishes_only_for_1043() {
    let upstream = |code| SolidError::Upstream {
        operation: "op",
        code,
        entity: None,
    };
    assert!(upstream(SM_ERR_OFFSET_EDGE_VANISHES).is_offset_edge_vanishes());
    assert!(upstream(1043).is_offset_edge_vanishes());
    for code in [
        SM_ERR,
        SM_ERR_OFFSET_WALLS_TOUCH,
        SM_ERR_INVALID_INPUT,
        1044,
    ] {
        assert!(!upstream(code).is_offset_edge_vanishes(), "{code}");
    }
    assert!(!SolidError::sm_err("op").is_offset_edge_vanishes());
    assert!(!SolidError::unported("Sym", "unit").is_offset_edge_vanishes());
    assert_eq!(
        upstream(SM_ERR_OFFSET_EDGE_VANISHES).sm_status(),
        Some(SM_ERR_OFFSET_EDGE_VANISHES)
    );
}
