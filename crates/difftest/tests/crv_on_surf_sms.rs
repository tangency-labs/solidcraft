//! Non-C++ regression coverage for the curve-on-surface runner's actual SMS parser.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/crv_on_surf_sms.rs"]
mod sms;

fn assert_rejected(text: &str, message: &str) {
    // A panic is a test failure, not a successful rejection.
    let result = sms::parse_sms(text, "malformed.sms");
    assert!(result.is_err(), "accepted {text}");
    if let Err(error) = result {
        let error = error.to_string();
        assert!(error.contains("malformed.sms"), "{error}");
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn saturated_counts_are_rejected_without_panicking() {
    assert_rejected("18446744073709551616 0 0 0 0", "bad n count");
    assert_rejected("1e100 0 0 0 0", "bad n count");
}

#[test]
fn exclusive_usize_bound_is_rejected_in_every_position() {
    let upper = 2.0_f64.powi(usize::BITS as i32);
    for header in [
        format!("{upper:.0} 0 0 0 0"),
        format!("0 {upper:.0} 0 0 0"),
        format!("0 0 {upper:.0} 0 0"),
        format!("0 0 0 {upper:.0} 0"),
    ] {
        assert_rejected(&header, "count");
    }
}

#[test]
fn malformed_and_nonfinite_counts_are_rejected() {
    for value in ["-1", "0.5", "NaN", "inf", "-inf", "1e100"] {
        for header in [
            format!("{value} 0 0 0 0"),
            format!("0 {value} 0 0 0"),
            format!("0 0 {value} 0 0"),
            format!("0 0 0 {value} 0"),
        ] {
            assert_rejected(&header, "count");
        }
    }
}

#[test]
fn huge_truncated_dimensions_are_rejected_before_net_growth() {
    for header in [
        "1000000000 0 0 0 0",
        "0 1000000000 0 0 1",
        "0 0 1000000000 0 0",
        "0 0 0 1000000000 0",
    ] {
        assert_rejected(header, "truncated");
    }
}

#[test]
fn individual_knot_count_overflow_is_rejected() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    assert_rejected(
        &format!("{large:.0} 0 {large:.0} 0 0"),
        "u knot count overflow",
    );
    assert_rejected(
        &format!("0 {large:.0} 0 {large:.0} 0"),
        "v knot count overflow",
    );
}

#[test]
fn combined_knot_count_overflow_with_one_control_point_is_rejected() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    // Each knot count fits, but their combined payload cannot fit in usize.
    assert_rejected(
        &format!("0 0 {large:.0} {large:.0} 0 1 2 3"),
        "required token count overflow",
    );
}

#[test]
fn control_token_arithmetic_overflow_is_rejected() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    assert_rejected(
        &format!("{large:.0} {large:.0} 0 0 0"),
        "required token count overflow",
    );
    assert_rejected(
        &format!("{large:.0} 0 0 0 1"),
        "required token count overflow",
    );
}

#[test]
fn truncated_headers_and_both_payload_layouts_are_rejected() {
    for text in ["", "0 0 0 0", "0 0 0 0 0 1 2", "0 0 0 0 1 1 2 3"] {
        assert_rejected(text, "truncated");
    }
    for points in ["0 1 2 3", "1 1 2 3 4"] {
        assert_rejected(&format!("0 0 0 0 {points} 0"), "truncated");
        assert_rejected(&format!("0 0 0 0 {points} 0 1 0"), "truncated");
    }
}

#[test]
fn non_numeric_payload_tokens_are_rejected() {
    for text in [
        "bad 0 0 0 0",
        "0 0 0 0 0 bad 2 3 0 1 0 1",
        "0 0 0 0 0 1 2 3 0 1 bad 1",
    ] {
        assert_rejected(text, "invalid float literal");
    }
}

#[test]
fn nonrational_surface_preserves_now_and_signed_zero() -> Result<(), HarnessError> {
    let surface = sms::parse_sms("-0.0 0e0 0.0 0 0 -0.0 2.5 -3e-2 -1 1 2 4", "valid.sms")?;
    assert_eq!(
        surface.net,
        vec![vec![[-0.0, 2.5, -0.03, solidcore::nurbs::math::NOW]]]
    );
    assert_eq!(surface.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!((surface.p, surface.q), (0, 0));
    assert_eq!(surface.u, vec![-1.0, 1.0]);
    assert_eq!(surface.v, vec![2.0, 4.0]);
    Ok(())
}

#[test]
fn rational_surface_preserves_weighted_coordinates_exactly() -> Result<(), HarnessError> {
    // Nonzero flags select rational storage; coordinates are not divided by w.
    let surface = sms::parse_sms("0 0 0 0 -2 2 -4 6 2 0 1 -2 3", "valid.sms")?;
    assert_eq!(surface.net, vec![vec![[2.0, -4.0, 6.0, 2.0]]]);
    assert_eq!((surface.p, surface.q), (0, 0));
    assert_eq!(surface.u, vec![0.0, 1.0]);
    assert_eq!(surface.v, vec![-2.0, 3.0]);
    Ok(())
}

#[test]
fn nonsquare_surface_preserves_order_degrees_knots_and_numeric_tail() -> Result<(), HarnessError> {
    let surface = sms::parse_sms(
        "1 2 1 2 0 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 \
         0 0 1 1 -1 -1 -1 2 2 2 99",
        "valid.sms",
    )?;
    let w = solidcore::nurbs::math::NOW;
    assert_eq!(
        surface.net,
        vec![
            vec![[0.0, 1.0, 2.0, w], [3.0, 4.0, 5.0, w], [6.0, 7.0, 8.0, w]],
            vec![
                [9.0, 10.0, 11.0, w],
                [12.0, 13.0, 14.0, w],
                [15.0, 16.0, 17.0, w]
            ],
        ]
    );
    assert_eq!((surface.p, surface.q), (1, 2));
    assert_eq!(surface.u, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v, vec![-1.0, -1.0, -1.0, 2.0, 2.0, 2.0]);
    Ok(())
}

#[test]
fn trailing_non_numeric_tokens_stay_ignored() -> Result<(), HarnessError> {
    // Tokens are consumed lazily, so junk after the last knot was never parsed.
    let surface = sms::parse_sms("0 0 0 0 0 0 1 2 0 1 0 1 junk", "valid.sms")?;
    assert_eq!(
        surface.net,
        vec![vec![[0.0, 1.0, 2.0, solidcore::nurbs::math::NOW]]]
    );
    assert_eq!(surface.u, vec![0.0, 1.0]);
    assert_eq!(surface.v, vec![0.0, 1.0]);
    Ok(())
}
