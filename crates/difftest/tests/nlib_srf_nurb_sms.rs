//! Non-C++ regression coverage for the runner's actual lazy SMS decoder.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/nlib_srf_nurb_sms.rs"]
mod sms;

fn assert_rejected(text: &str, message: &str) {
    // A panic is a test failure, never a successful rejection.
    let result = sms::parse_sms(text, "malformed.sms");
    assert!(result.is_err(), "accepted {text}");
    if let Err(error) = result {
        let error = error.to_string();
        assert!(error.starts_with("malformed.sms:"), "{error}");
        assert!(error.contains(message), "{error}");
    }
}

fn header_at(position: usize, value: &str) -> String {
    (0..5)
        .map(|index| if index == position { value } else { "0" })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn invalid_counts_in_every_position_are_rejected() {
    let upper = 2.0_f64.powi(usize::BITS as i32).to_string();
    for value in ["-1", "0.5", "NaN", "inf", "-inf", "1e100", &upper] {
        for position in 0..4 {
            assert_rejected(&header_at(position, value), "bad count");
        }
    }
}

#[test]
fn huge_representable_counts_with_short_bodies_do_not_allocate() {
    let huge = 2.0_f64.powi(usize::BITS as i32 - 4).to_string();
    for position in 0..4 {
        assert_rejected(&header_at(position, &huge), "truncated");
        assert_rejected(
            &format!("{} 1 bad", header_at(position, &huge)),
            "invalid float literal",
        );
    }
}

#[test]
fn largest_f64_count_below_exclusive_limit_is_not_saturated_or_capped() {
    let upper = 2.0_f64.powi(usize::BITS as i32);
    let largest = f64::from_bits(upper.to_bits() - 1).floor().to_string();
    // In a degree field this value fits usize and its knot count fits on 64-bit
    // hosts. On 32-bit hosts +2 overflows, but it is still not a bad count.
    for position in [2, 3] {
        assert_rejected(
            &header_at(position, &largest),
            if usize::BITS > 53 {
                "truncated"
            } else {
                "overflow"
            },
        );
    }
}

#[test]
fn checked_knot_net_coordinate_and_total_arithmetic_rejects_overflow() {
    let half = 2.0_f64.powi(usize::BITS as i32 - 1);
    let quarter = 2.0_f64.powi(usize::BITS as i32 - 2);
    assert_rejected(
        &format!("{half:.0} 0 {half:.0} 0 0"),
        "u knot count overflow",
    );
    assert_rejected(
        &format!("0 {half:.0} 0 {half:.0} 0"),
        "v knot count overflow",
    );
    // Net product, coordinate product (both layouts), then payload sum.
    for header in [
        format!("{half:.0} {half:.0} 0 0 0"),
        format!("{half:.0} 0 0 0 0"),
        format!("{quarter:.0} 0 0 0 1"),
        format!("{quarter:.0} 0 {quarter:.0} 0 0"),
    ] {
        assert_rejected(&header, "required token count overflow");
    }
}

#[test]
fn every_polynomial_and_rational_truncation_point_is_rejected() {
    for text in [
        "0 0 0 0 0 1 2 3 0 1 0 1",
        "0 0 0 0 1 2 4 6 2 0 1 0 1",
        "1 2 1 2 0 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 \
         0 0 1 1 -1 -1 -1 2 2 2",
    ] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_rejected(&tokens[..end].join(" "), "truncated");
        }
        assert!(sms::parse_sms(text, "valid.sms").is_ok());
    }
}

#[test]
fn first_bad_consumed_token_precedes_truncation() {
    for text in [
        "0 0 bad",
        "0 0 0 0 bad",
        "0 0 0 0 0 bad",
        "0 0 0 0 0 1 bad",
        "0 0 0 0 0 1 2 3 0 bad",
        "0 0 0 0 1 1 2 3 bad",
        "0 0 0 0 1 1 2 3 4 0 1 0 bad",
    ] {
        assert_rejected(text, "invalid float literal");
    }
    // Header validation stays lazy too: later junk must not hide this error.
    assert_rejected("-1 bad", "bad count -1");
}

#[test]
fn every_consumed_numeric_position_is_validated() {
    for text in ["0 0 0 0 0 1 2 3 0 1 0 1", "0 0 0 0 1 2 4 6 2 0 1 0 1"] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for position in 0..tokens.len() {
            let mut bad = tokens.clone();
            bad[position] = "bad";
            assert_rejected(&bad.join(" "), "invalid float literal");
        }
    }
}

#[test]
fn polynomial_scientific_and_signed_zero_values_decode_exactly() -> Result<(), HarnessError> {
    let surface = sms::parse_sms("-0 0e0 0.0 -0.0 -0 -0.0 2.5 -3e-2 -1 1 2 4", "valid.sms")?;
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
fn every_nonzero_rational_flag_preserves_homogeneous_coordinates() -> Result<(), HarnessError> {
    for flag in ["1", "-2", "0.5", "NaN", "inf", "-inf"] {
        let surface = sms::parse_sms(&format!("0 0 0 0 {flag} 2 -4 6 2 0 1 -2 3"), "valid.sms")?;
        assert_eq!(surface.net, vec![vec![[2.0, -4.0, 6.0, 2.0]]]);
        assert_eq!((surface.p, surface.q), (0, 0));
        assert_eq!(surface.u, vec![0.0, 1.0]);
        assert_eq!(surface.v, vec![-2.0, 3.0]);
    }
    Ok(())
}

#[test]
fn nonsquare_net_degrees_and_knots_keep_v_fastest_order() -> Result<(), HarnessError> {
    let surface = sms::parse_sms(
        "1e0 2.0 1 2 0 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 \
         0 0 1 1 -1 -1 -1 2 2 2",
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
fn numeric_and_nonnumeric_trailing_tokens_are_ignored() -> Result<(), HarnessError> {
    for tail in ["99", "bad", "99 NaN inf junk"] {
        let surface = sms::parse_sms(&format!("0 0 0 0 0 1 2 3 0 1 0 1 {tail}"), "valid.sms")?;
        assert_eq!(
            surface.net,
            vec![vec![[1.0, 2.0, 3.0, solidcore::nurbs::math::NOW]]]
        );
        assert_eq!(surface.u, vec![0.0, 1.0]);
        assert_eq!(surface.v, vec![0.0, 1.0]);
    }
    Ok(())
}

#[test]
fn payload_nonfinite_values_and_unusual_degrees_are_not_restricted() -> Result<(), HarnessError> {
    let surface = sms::parse_sms("0 0 2 1 0 NaN inf -inf -0 0 1 1 -2 0 2", "valid.sms")?;
    assert!(surface.net[0][0][0].is_nan());
    assert_eq!(surface.net[0][0][1], f64::INFINITY);
    assert_eq!(surface.net[0][0][2], f64::NEG_INFINITY);
    assert_eq!((surface.p, surface.q), (2, 1));
    assert_eq!(surface.u[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(surface.u, vec![-0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v, vec![-2.0, 0.0, 2.0]);
    Ok(())
}
