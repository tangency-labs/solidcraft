//! Non-C++ regression coverage for the runner's actual SMS parser.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/nlib_srf_offset_sms.rs"]
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
fn saturated_header_count_is_rejected_without_panicking() {
    assert_rejected("1e100 0 0 0 0", "bad n count");
}

#[test]
fn huge_representable_header_is_rejected_without_panicking() {
    // Representable on the affected 64-bit hosts; rejected before any net growth.
    assert_rejected(
        "1e18 0 0 0 0",
        if usize::BITS >= 64 {
            "truncated"
        } else {
            "count"
        },
    );
}

#[test]
fn exclusive_usize_limit_is_rejected() {
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
fn invalid_counts_are_rejected_in_every_header_position() {
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
fn oversized_knot_dimensions_are_rejected() {
    assert_rejected("0 0 1e100 0 0 0 0 0", "bad p count");
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
fn point_token_arithmetic_overflow_is_rejected() {
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
fn truncated_polynomial_and_rational_data_are_rejected() {
    for text in ["", "0 0 0 0", "0 0 0 0 0 1 2", "0 0 0 0 1 1 2 3"] {
        assert_rejected(text, "truncated");
    }
    for points in ["0 1 2 3", "1 1 2 3 4"] {
        // Missing u knots, then missing v knots, in both layouts.
        assert_rejected(&format!("0 0 0 0 {points} 0"), "truncated");
        assert_rejected(&format!("0 0 0 0 {points} 0 1 0"), "truncated");
    }
}

#[test]
fn polynomial_surface_decodes_exactly() -> Result<(), HarnessError> {
    let surface = sms::parse_sms("0e0 0.0 0 0 0 -0.0 2.5 -3e-2 -1 1 2 4", "valid.sms")?;
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
fn rational_surface_decodes_exactly() -> Result<(), HarnessError> {
    // Store weighted coordinates verbatim; any nonzero rational flag is true.
    let surface = sms::parse_sms("0 0 0 0 -2 2 -4 6 2 0 1 -2 3", "valid.sms")?;
    assert_eq!(surface.net, vec![vec![[2.0, -4.0, 6.0, 2.0]]]);
    assert_eq!((surface.p, surface.q), (0, 0));
    assert_eq!(surface.u, vec![0.0, 1.0]);
    assert_eq!(surface.v, vec![-2.0, 3.0]);
    Ok(())
}

#[test]
fn nonsquare_surface_preserves_row_order_degrees_knots_and_numeric_tail() -> Result<(), HarnessError>
{
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
fn invalid_numeric_tokens_including_trailing_tokens_still_error() {
    for text in ["0 0 0 0 0 bad 2 3 0 1 0 1", "0 0 0 0 0 1 2 3 0 1 0 1 bad"] {
        assert_rejected(text, "invalid float literal");
    }
}
