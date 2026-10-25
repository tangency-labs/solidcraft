//! Non-C++ regressions exercising the surface-basic runner's actual SMS decoder.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/nlib_srf_basic_sms.rs"]
mod sms;

use solidcore::nurbs::math::NOW;

fn rejected(text: &str, message: &str) {
    let result = sms::parse_sms(text, "regression.sms");
    assert!(result.is_err(), "accepted malformed SMS: {text}");
    if let Err(error) = result {
        let error = error.to_string();
        assert!(error.contains("regression.sms"), "{error}");
        assert!(error.contains(message), "{error}");
    }
}

fn header_with(position: usize, value: &str) -> String {
    (0..5)
        .map(|index| if index == position { value } else { "0" })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn every_count_rejects_malformed_and_unrepresentable_values() {
    let upper = 2.0_f64.powi(usize::BITS as i32);
    for position in 0..4 {
        for value in ["-1", "0.5", "NaN", "inf", "-inf", "1e100"] {
            rejected(&header_with(position, value), "count");
        }
        for value in [upper, upper * 2.0] {
            rejected(&header_with(position, &format!("{value:.0}")), "count");
        }
        rejected(&header_with(position, "invalid"), "invalid float");
    }
}

#[test]
fn huge_representable_dimensions_do_not_allocate_from_headers() {
    let huge = 2.0_f64.powi(usize::BITS as i32 - 5);
    for position in 0..4 {
        rejected(&header_with(position, &format!("{huge:.0}")), "truncated");
    }
    // The greatest integral float below the exclusive bound must not saturate.
    let upper = 2.0_f64.powi(usize::BITS as i32);
    let below = if usize::BITS <= 53 {
        upper - 1.0
    } else {
        f64::from_bits(upper.to_bits() - 1)
    };
    for position in 0..4 {
        let message = if position < 2 || usize::BITS <= 53 {
            "overflow"
        } else {
            "truncated"
        };
        rejected(&header_with(position, &format!("{below:.0}")), message);
    }
}

#[test]
fn checked_size_sums_and_products_reject_overflow() {
    let half = 2.0_f64.powi(usize::BITS as i32 - 1);
    let quarter = half / 2.0;
    for header in [
        format!("{half:.0} 0 {half:.0} 0 0"),    // u knot sum
        format!("0 {half:.0} 0 {half:.0} 0"),    // v knot sum
        format!("{half:.0} 1 0 0 0"),            // net product
        format!("{half:.0} 0 0 0 0"),            // coordinate product
        format!("{quarter:.0} 0 {half:.0} 0 0"), // net plus u knots
        format!("0 0 {half:.0} {half:.0} 0"),    // plus v knots
        format!("{quarter:.0} 0 0 0 1"),         // rational coordinate product
    ] {
        rejected(&header, "overflow");
    }
}

#[test]
fn every_header_and_payload_truncation_is_reported() {
    for text in [
        "0 0 0 0 0 1 2 3 4 5 6 7",
        "0 0 0 0 1 1 2 3 4 5 6 7 8",
        "1 2 1 0 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 0 0 1 1 0 0 1 1",
    ] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            rejected(&tokens[..end].join(" "), "truncated");
        }
        assert!(sms::parse_sms(text, "complete.sms").is_ok());
    }
}

#[test]
fn polynomial_preserves_signed_zero_scientific_counts_and_nonsquare_order() {
    let decoded = sms::parse_sms(
        "1e0 2e0 1e0 -0 -0 -0 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 -0 0 1 1 0 0 1 1",
        "polynomial.sms",
    )
    .expect("polynomial SMS");
    assert_eq!((decoded.p, decoded.q), (1, 0));
    assert_eq!(
        decoded.net,
        vec![
            vec![
                [-0.0, 2.0, 3.0, NOW],
                [4.0, 5.0, 6.0, NOW],
                [7.0, 8.0, 9.0, NOW]
            ],
            vec![
                [10.0, 11.0, 12.0, NOW],
                [13.0, 14.0, 15.0, NOW],
                [16.0, 17.0, 18.0, NOW]
            ],
        ]
    );
    assert_eq!(decoded.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(decoded.u, [-0.0, 0.0, 1.0, 1.0]);
    assert_eq!(decoded.u[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(decoded.v, [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn rational_flags_preserve_homogeneous_coordinates_verbatim() {
    for flag in ["1", "-1", "2.5", "NaN", "inf", "-inf"] {
        let text = format!("0 1 0 0 {flag} -0 4 6 2 9 12 15 -3 2 1 3 2 1");
        let decoded = sms::parse_sms(&text, "rational.sms").expect("rational SMS");
        assert_eq!((decoded.p, decoded.q), (0, 0));
        assert_eq!(
            decoded.net,
            vec![vec![[-0.0, 4.0, 6.0, 2.0], [9.0, 12.0, 15.0, -3.0]]]
        );
        assert_eq!(decoded.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(decoded.u, [2.0, 1.0]);
        assert_eq!(decoded.v, [3.0, 2.0, 1.0]);
    }
}

#[test]
fn consumed_invalid_tokens_win_over_later_truncation() {
    for text in ["0 0 0 0 0 1 2 3 4 5 6 7", "0 0 0 0 1 1 2 3 4 5 6 7 8"] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        // Every consumed token, including the rational flag, is parsed lazily.
        for position in 4..tokens.len() {
            let mut invalid = tokens.clone();
            invalid[position] = "invalid";
            rejected(&invalid.join(" "), "invalid float");
            rejected(&invalid[..=position].join(" "), "invalid float");
        }
    }
    rejected("1e6 0 0 0 0 invalid", "invalid float");
    rejected("-1 invalid", "count");
}

#[test]
fn trailing_junk_is_not_parsed_and_payload_nonfinite_values_are_allowed() {
    for suffix in ["", " 9", " invalid", " NaN inf -inf invalid"] {
        let decoded = sms::parse_sms(
            &format!("-0 -0 -0 -0 0 NaN inf -inf 3 2 1 0{suffix}"),
            "trailing.sms",
        )
        .expect("ignored trailing data");
        assert!(decoded.net[0][0][0].is_nan());
        assert_eq!(decoded.net[0][0][1], f64::INFINITY);
        assert_eq!(decoded.net[0][0][2], f64::NEG_INFINITY);
        assert_eq!(decoded.net[0][0][3], NOW);
        assert_eq!((decoded.p, decoded.q), (0, 0));
        assert_eq!(decoded.u, [3.0, 2.0]);
        assert_eq!(decoded.v, [1.0, 0.0]);
    }
}
