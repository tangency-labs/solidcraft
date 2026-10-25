//! Non-C++ tests of the `surface.surface_p6` runner's actual pure SMS parser.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/surface_surface_p6_sms.rs"]
mod sms;

use solidcore::nurbs::math::NOW;

fn rejected(text: &str, message: &str) {
    let result = sms::parse_sms(text, "input.sms");
    assert!(result.is_err(), "accepted input");
    if let Err(error) = result {
        let error = error.to_string();
        assert!(error.starts_with("input.sms: "), "{error}");
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn polynomial_nonsquare_net_and_knot_order() {
    let s = sms::parse_sms(
        "1.0 2e0 0.0 1e0 -0.0 \
         1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 \
         21 22 23 31 32 33 34 35",
        "poly.sms",
    )
    .unwrap();
    assert_eq!((s.p, s.q), (0, 1));
    assert_eq!(
        s.net,
        vec![
            vec![[1., 2., 3., NOW], [4., 5., 6., NOW], [7., 8., 9., NOW]],
            vec![
                [10., 11., 12., NOW],
                [13., 14., 15., NOW],
                [16., 17., 18., NOW]
            ],
        ]
    );
    assert_eq!(s.u, [21., 22., 23.]);
    assert_eq!(s.v, [31., 32., 33., 34., 35.]);
}

#[test]
fn raw_nonzero_rational_flags_preserve_homogeneous_coordinates() {
    for flag in ["1", "-2", "0.5", "NaN", "inf", "-inf"] {
        let s = sms::parse_sms(
            &format!("0 1 1 0 {flag} 2 4 6 2 -3 -6 -9 -3 0 0 1 4 5 6"),
            "rat.sms",
        )
        .unwrap();
        assert_eq!((s.p, s.q), (1, 0));
        assert_eq!(s.net, vec![vec![[2., 4., 6., 2.], [-3., -6., -9., -3.]]]);
        assert_eq!(s.u, [0., 0., 1.]);
        assert_eq!(s.v, [4., 5., 6.]);
    }
}

#[test]
fn signed_zero_counts_flags_and_coordinates() {
    for flag in ["0", "-0", "0.0", "-0e10"] {
        let s = sms::parse_sms(
            &format!("-0 -0.0 0e0 -0e1 {flag} -0 0 -0 -0 1 0 -0"),
            "zero.sms",
        )
        .unwrap();
        assert_eq!((s.p, s.q), (0, 0));
        assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(s.net[0][0][2].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(s.net[0][0][3].to_bits(), NOW.to_bits());
        assert_eq!(s.u[0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(s.v[1].to_bits(), (-0.0_f64).to_bits());
    }
}

#[test]
fn invalid_counts_in_every_header_position() {
    for value in ["-1", "0.5", "NaN", "inf", "-inf", "1e100"] {
        for position in 0..4 {
            let mut header = ["0"; 5];
            header[position] = value;
            rejected(&header.join(" "), "bad count");
        }
    }
}

#[test]
fn exclusive_usize_bound_and_larger_counts() {
    let upper = 2.0_f64.powi(usize::BITS as i32);
    for value in [upper, upper * 2.] {
        for position in 0..4 {
            let mut header = vec!["0".to_owned(); 5];
            header[position] = format!("{value:.0}");
            rejected(&header.join(" "), "bad count");
        }
    }
}

#[test]
fn independently_checked_knot_additions() {
    // On 64-bit, the largest f64 below 2^64 is 2048 below it. A parser-level
    // sum overflow therefore needs an enormous net; exercise the exact shared
    // arithmetic directly instead, without allocating such a net.
    let half = 1usize << (usize::BITS - 1);
    for axis in ["u", "v"] {
        for (last, degree) in [(half, half), (usize::MAX - 1, 0), (0, usize::MAX)] {
            let error = sms::knot_count(last, degree, axis, "input.sms").unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("input.sms: {axis} knot count overflow")
            );
        }
        assert_eq!(sms::knot_count(1, 2, axis, "input.sms").unwrap(), 5);
    }
}

#[test]
fn net_then_u_then_v_processing_order_with_huge_counts() {
    let half = 2.0_f64.powi(usize::BITS as i32 - 1);
    // Both knot sums overflow, but the net is read first.
    rejected(
        &format!("{half:.0} {half:.0} {half:.0} {half:.0} 0"),
        "truncated",
    );
    // No early U/V arithmetic or degree policy may mask net truncation.
    rejected(&format!("0 0 {half:.0} {half:.0} 0 1 2"), "truncated");
    // Huge U and V declarations remain token-bounded, as does a huge V alone.
    rejected(&format!("0 0 {half:.0} {half:.0} 0 1 2 3"), "truncated");
    rejected(&format!("0 0 0 {half:.0} 0 1 2 3 0 1"), "truncated");
    // Even an eventual overflowing V sum cannot mask an earlier net truncation.
    // Reaching U with this m would require an enormous net; the independent
    // helper test above covers that arithmetic without materializing one.
    rejected(&format!("0 {half:.0} 0 {half:.0} 0 1 2 3"), "truncated");
}

#[test]
fn huge_representable_headers_preflight_without_allocation() {
    let below_upper = 2.0_f64.powi(usize::BITS as i32) - 4096.;
    for position in 0..4 {
        let mut header = vec!["0".to_owned(); 5];
        header[position] = "1000000000".to_owned();
        rejected(&header.join(" "), "truncated");
    }
    // A valid cast just below the exclusive limit, not a saturated conversion.
    rejected(&format!("0 0 {below_upper:.0} 0 0"), "truncated");
}

#[test]
fn truncation_at_every_header_net_and_knot_token() {
    for full in ["0 0 0 0 0 1 2 3 0 1 0 1", "0 0 0 0 1 1 2 3 4 0 1 0 1"] {
        let tokens: Vec<_> = full.split_whitespace().collect();
        for end in 0..tokens.len() {
            rejected(&tokens[..end].join(" "), "truncated");
        }
        assert!(sms::parse_sms(full, "complete.sms").is_ok());
    }
}

#[test]
fn numeric_tails_are_parsed_but_ignored() {
    let s = sms::parse_sms("0 0 0 0 0 1 2 3 0 1 0 1 NaN inf -inf 42", "tail.sms").unwrap();
    assert_eq!(s.net, vec![vec![[1., 2., 3., NOW]]]);
    assert_eq!(s.u, [0., 1.]);
    assert_eq!(s.v, [0., 1.]);
}

#[test]
fn bad_tail_wins_before_bad_header_and_eof() {
    for text in [
        "0 0 0 0 0 1 2 3 0 1 0 1 BAD",
        "-1 0 0 0 0 BAD",
        "NaN 0 0 0 0 BAD",
        "1e100 0 0 0 0 BAD",
        "0 BAD",
        "0 0 0 0 0 1 BAD",
    ] {
        rejected(text, "invalid float literal");
    }
}

#[test]
fn first_float_error_wins_across_entire_text() {
    let first = "1e9999999999999999999999999999999999999999999999999999x";
    let expected = first.parse::<f64>().unwrap_err().to_string();
    rejected(&format!("-1 {first} BAD"), &expected);
}

#[test]
fn semantic_header_errors_still_precede_missing_later_fields() {
    for text in ["-1", "0 -1", "0 0 -1", "0 0 0 -1"] {
        rejected(text, "bad count -1");
    }
}

#[test]
fn no_new_geometry_validity_caps_or_coordinate_checks() {
    let s = sms::parse_sms(
        "0 0 3 2 1 NaN inf -inf -0 0 1 2 3 4 5 6 7 8",
        "geometry.sms",
    )
    .unwrap();
    assert_eq!((s.p, s.q), (3, 2));
    assert!(s.net[0][0][0].is_nan());
    assert_eq!(s.net[0][0][1], f64::INFINITY);
    assert_eq!(s.net[0][0][2], f64::NEG_INFINITY);
    assert_eq!(s.net[0][0][3].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u, [0., 1., 2., 3., 4.]);
    assert_eq!(s.v, [5., 6., 7., 8.]);
}

#[test]
fn malformed_float_in_every_consumed_position() {
    for full in ["0 0 0 0 0 1 2 3 0 1 0 1", "0 0 0 0 1 1 2 3 4 0 1 0 1"] {
        let tokens: Vec<_> = full.split_whitespace().collect();
        for position in 0..tokens.len() {
            let mut bad = tokens.clone();
            bad[position] = "BAD";
            rejected(&bad.join(" "), "invalid float literal");
        }
    }
}

#[test]
fn available_upstream_fixture_census_matches_original_storage_bits() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles");
    for (name, n, m, p, q, total) in [
        ("Surface.sms", 13, 1, 3, 1, 111),
        ("DropCurveSurface.sms", 1, 1, 1, 1, 25),
        ("offset_nurb.sms", 5, 1, 3, 1, 55),
    ] {
        let path = root.join(name);
        if !path.exists() {
            // The licensed upstream checkout is optional for non-C++ CI.
            eprintln!("fixture census: {name} unavailable (not checked)");
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let values: Vec<f64> = text
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        // Fixed, independently inventoried fixture sizes, not untrusted casts.
        assert_eq!(values.len(), total);
        assert_eq!(&values[..5], &[n as f64, m as f64, p as f64, q as f64, 0.]);
        let rational = false;
        let parsed = sms::parse_sms(&text, &path.display().to_string()).unwrap();
        assert_eq!((parsed.p, parsed.q), (p, q));
        assert_eq!(parsed.net.len(), n + 1);
        let mut original = values[5..].iter().copied();
        for row in &parsed.net {
            assert_eq!(row.len(), m + 1);
            for point in row {
                for value in point.iter().take(3) {
                    assert_eq!(value.to_bits(), original.next().unwrap().to_bits());
                }
                let weight = if rational {
                    original.next().unwrap()
                } else {
                    NOW
                };
                assert_eq!(point[3].to_bits(), weight.to_bits());
            }
        }
        assert_eq!(parsed.u.len(), n + p + 2);
        assert_eq!(parsed.v.len(), m + q + 2);
        for value in parsed.u.iter().chain(&parsed.v) {
            assert_eq!(value.to_bits(), original.next().unwrap().to_bits());
        }
        assert_eq!(original.clone().count(), 0);
        eprintln!("fixture census: {name}: {}x{}, p={p}, q={q}, rational={rational}, u={}, v={}, numeric_tail={}",
            n + 1, m + 1, parsed.u.len(), parsed.v.len(), original.count());
    }
}
