//! Non-C++ regression coverage for both curve-bounded-surface fixture parsers.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/surface_curve_bounded_surface_fixtures.rs"]
mod fixtures;

fn assert_rejected(text: &str, sms: bool, message: &str) {
    // Panics, including capacity overflow, fail the test rather than count as errors.
    let path = if sms {
        "malformed.sms"
    } else {
        "malformed.smc"
    };
    let result = if sms {
        fixtures::parse_sms(text, path).map(|_| ())
    } else {
        fixtures::parse_smc(text, path).map(|_| ())
    };
    assert!(result.is_err(), "accepted {text}");
    if let Err(error) = result {
        let error = error.to_string();
        assert!(error.contains(path), "{error}");
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn concrete_surface_capacity_overflow_input_returns_error() {
    assert_rejected(
        "1152921504606846976 0 0 0 0",
        true,
        if usize::BITS >= 64 {
            "truncated"
        } else {
            "bad n count"
        },
    );
}

#[test]
fn concrete_curve_capacity_overflow_input_returns_error() {
    assert_rejected(
        "1152921504606846976 0 0 3",
        false,
        if usize::BITS >= 64 {
            "truncated"
        } else {
            "bad n count"
        },
    );
}

#[test]
fn invalid_counts_are_rejected_in_every_header_position() {
    let upper = 2.0_f64.powi(usize::BITS as i32);
    for value in [
        "-1",
        "0.5",
        "NaN",
        "inf",
        "-inf",
        "1e100",
        &format!("{upper:.0}"),
    ] {
        for text in [
            format!("{value} 0 0 0 0"),
            format!("0 {value} 0 0 0"),
            format!("0 0 {value} 0 0"),
            format!("0 0 0 {value} 0"),
        ] {
            assert_rejected(&text, true, "count");
        }
        for text in [
            format!("{value} 0 0 3"),
            format!("0 {value} 0 3"),
            format!("0 0 0 {value}"),
        ] {
            assert_rejected(&text, false, "count");
        }
    }
}

#[test]
fn surface_knot_arithmetic_overflow_is_rejected() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    assert_rejected(
        &format!("{large:.0} 0 {large:.0} 0 0"),
        true,
        "u knot count overflow",
    );
    assert_rejected(
        &format!("0 {large:.0} 0 {large:.0} 0"),
        true,
        "v knot count overflow",
    );
}

#[test]
fn curve_knot_arithmetic_overflow_is_rejected() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    assert_rejected(
        &format!("{large:.0} {large:.0} 0 3"),
        false,
        "u knot count overflow",
    );
}

#[test]
fn surface_point_product_overflow_is_rejected() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    assert_rejected(
        &format!("{large:.0} {large:.0} 0 0 0"),
        true,
        "point count overflow",
    );
}

#[test]
fn coordinate_product_overflow_is_rejected_for_both_formats() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    for rat in [0, 1] {
        assert_rejected(
            &format!("{large:.0} 0 0 0 {rat}"),
            true,
            "required token count overflow",
        );
        for dim in [2, 3] {
            assert_rejected(
                &format!("{large:.0} 0 {rat} {dim}"),
                false,
                "required token count overflow",
            );
        }
    }
}

#[test]
fn total_token_sum_overflow_is_rejected() {
    let quarter = 2.0_f64.powi(usize::BITS as i32 - 2);
    // Coordinate products fit; adding the knot vectors does not.
    assert_rejected(
        &format!("{quarter:.0} 0 0 0 0"),
        true,
        "required token count overflow",
    );
    assert_rejected(
        &format!("{quarter:.0} 0 0 3"),
        false,
        "required token count overflow",
    );
}

#[test]
fn large_degrees_without_payload_are_rejected() {
    assert_rejected("0 0 1000000000 0 0", true, "truncated");
    assert_rejected("0 0 0 1000000000 1", true, "truncated");
    assert_rejected("0 1000000000 0 2", false, "truncated");
}

#[test]
fn truncated_surface_headers_points_and_knots_are_rejected() {
    for text in [
        "",
        "0",
        "0 0",
        "0 0 0",
        "0 0 0 0",
        "0 0 0 0 0 1 2",
        "0 0 0 0 1 1 2 3",
        "0 0 0 0 0 1 2 3 0",
        "0 0 0 0 0 1 2 3 0 1 0",
        "0 0 0 0 1 1 2 3 4 0 1 0",
    ] {
        assert_rejected(text, true, "truncated");
    }
}

#[test]
fn truncated_curve_headers_points_and_knots_are_rejected() {
    for text in ["", "0", "0 0", "0 0 0"] {
        assert_rejected(text, false, "truncated");
    }
    for (rat, dim, points) in [
        (0, 2, "1 2"),
        (1, 2, "1 2 4"),
        (0, 3, "1 2 3"),
        (1, 3, "1 2 3 4"),
    ] {
        for tail in ["", "1", points, &format!("{points} 0")] {
            assert_rejected(&format!("0 0 {rat} {dim} {tail}"), false, "truncated");
        }
    }
}

#[test]
fn malformed_numeric_headers_payloads_and_tails_are_rejected() {
    for text in [
        "bad 0 0 0 0",
        "0 0 0 0 0 bad 2 3 0 1 0 1",
        "0 0 0 0 0 1 2 3 0 1 0 1 bad",
    ] {
        assert_rejected(text, true, "invalid float literal");
    }
    for text in ["bad 0 0 3", "0 0 0 3 bad 2 3 0 1", "0 0 0 3 1 2 3 0 1 bad"] {
        assert_rejected(text, false, "invalid float literal");
    }
}

#[test]
fn polynomial_surface_decodes_exactly() -> Result<(), HarnessError> {
    let s = fixtures::parse_sms("0e0 0.0 0 0 0 -0.0 2.5 -3e-2 -1 1 2 4 99", "valid.sms")?;
    assert_eq!(
        s.net,
        vec![vec![[-0.0, 2.5, -0.03, solidcore::nurbs::math::NOW]]]
    );
    assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!((s.p, s.q), (0, 0));
    assert_eq!(s.u, vec![-1.0, 1.0]);
    assert_eq!(s.v, vec![2.0, 4.0]);
    Ok(())
}

#[test]
fn rational_surface_preserves_weighted_coordinates_and_nonsquare_order() -> Result<(), HarnessError>
{
    let s = fixtures::parse_sms(
        "1 2 1 2 -2 0 1 2 2 3 4 5 3 6 7 8 4 9 10 11 5 12 13 14 6 15 16 17 7 \
         0 0 1 1 -1 -1 -1 2 2 2 99",
        "valid.sms",
    )?;
    assert_eq!(
        s.net,
        vec![
            vec![
                [0.0, 1.0, 2.0, 2.0],
                [3.0, 4.0, 5.0, 3.0],
                [6.0, 7.0, 8.0, 4.0]
            ],
            vec![
                [9.0, 10.0, 11.0, 5.0],
                [12.0, 13.0, 14.0, 6.0],
                [15.0, 16.0, 17.0, 7.0]
            ],
        ]
    );
    assert_eq!((s.p, s.q), (1, 2));
    assert_eq!(s.u, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(s.v, vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0]);
    Ok(())
}

#[test]
fn polynomial_2d_curve_decodes_exactly() -> Result<(), HarnessError> {
    let c = fixtures::parse_smc("1e0 1.0 0 2 -0.0 2.5 3 -4 0 0 1 1 99", "valid.smc")?;
    let w = solidcore::nurbs::math::NOW;
    assert_eq!(c.cpts, vec![[-0.0, 2.5, 0.0, w], [3.0, -4.0, 0.0, w]]);
    assert_eq!(c.cpts[0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!((c.dim, c.p), (2, 1));
    assert_eq!(c.u, vec![0.0, 0.0, 1.0, 1.0]);
    Ok(())
}

#[test]
fn rational_2d_curve_decodes_exactly() -> Result<(), HarnessError> {
    let c = fixtures::parse_smc("1 1 -2 2 2 -4 2 6 8 4 -1 -1 3 3", "valid.smc")?;
    assert_eq!(c.cpts, vec![[2.0, -4.0, 0.0, 2.0], [6.0, 8.0, 0.0, 4.0]]);
    assert_eq!((c.dim, c.p), (2, 1));
    assert_eq!(c.u, vec![-1.0, -1.0, 3.0, 3.0]);
    Ok(())
}

#[test]
fn polynomial_3d_curve_decodes_exactly() -> Result<(), HarnessError> {
    let c = fixtures::parse_smc("1 1 0 3 1 2 -3 4 5 6 -1 -1 3 3", "valid.smc")?;
    let w = solidcore::nurbs::math::NOW;
    assert_eq!(c.cpts, vec![[1.0, 2.0, -3.0, w], [4.0, 5.0, 6.0, w]]);
    assert_eq!((c.dim, c.p), (3, 1));
    assert_eq!(c.u, vec![-1.0, -1.0, 3.0, 3.0]);
    Ok(())
}

#[test]
fn rational_3d_curve_decodes_exactly() -> Result<(), HarnessError> {
    let c = fixtures::parse_smc("1 1 1 3 2 -4 6 2 8 10 12 4 -1 -1 3 3 99", "valid.smc")?;
    assert_eq!(c.cpts, vec![[2.0, -4.0, 6.0, 2.0], [8.0, 10.0, 12.0, 4.0]]);
    assert_eq!((c.dim, c.p), (3, 1));
    assert_eq!(c.u, vec![-1.0, -1.0, 3.0, 3.0]);
    Ok(())
}
