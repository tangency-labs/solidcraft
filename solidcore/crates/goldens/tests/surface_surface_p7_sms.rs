//! Non-C++ tests of the exact SMS decoder used by the P7 adapter.

#[path = "../src/units/fixtures/surface_surface_p7_sms.rs"]
mod sms;

use solidcore::nurbs::math::NOW;

const PATH: &str = "fixture/input.sms";
const MINIMAL: &str = "0 0 0 0 0 1 2 3 0 1 2 3";

fn parse(text: &str) -> Result<sms::Parsed, String> {
    sms::parse(PATH, text, NOW)
}

fn exclusive() -> f64 {
    2.0_f64.powi(usize::BITS as i32)
}

fn largest_float_count() -> f64 {
    // On 32-bit targets MAX itself is exactly representable; on 64-bit targets
    // the immediately preceding f64 is the largest representable valid count.
    if usize::BITS <= 53 {
        usize::MAX as f64
    } else {
        f64::from_bits(exclusive().to_bits() - 1)
    }
}

#[test]
fn minimum_polynomial_and_signed_zero() {
    let result = parse(MINIMAL).unwrap();
    assert_eq!(result.net, vec![vec![[1.0, 2.0, 3.0, NOW]]]);
    assert_eq!((result.p, result.q), (0, 0));
    assert_eq!(result.u, vec![0.0, 1.0]);
    assert_eq!(result.v, vec![2.0, 3.0]);
    let signed = parse("-0 -0 -0 -0 -0 -0 2 3 -0 1 2 3").unwrap();
    assert_eq!(signed.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(signed.net[0][0][3], NOW);
    assert_eq!(signed.u[0].to_bits(), (-0.0_f64).to_bits());
}

#[test]
fn nonsquare_net_v_fastest_homogeneous_and_knot_order() {
    // Two u rows, three v columns; xyz are already homogeneous, not weighted here.
    let result = parse(
        "1 2 2 1 1
         1 2 3 2  4 5 6 3  7 8 9 4
         10 11 12 5  13 14 15 6  16 17 18 7
         20 21 22 23 24  30 31 32 33 34",
    )
    .unwrap();
    assert_eq!((result.p, result.q), (2, 1));
    assert_eq!(
        result.net,
        vec![
            vec![[1., 2., 3., 2.], [4., 5., 6., 3.], [7., 8., 9., 4.]],
            vec![
                [10., 11., 12., 5.],
                [13., 14., 15., 6.],
                [16., 17., 18., 7.]
            ],
        ]
    );
    assert_eq!(result.u, vec![20., 21., 22., 23., 24.]);
    assert_eq!(result.v, vec![30., 31., 32., 33., 34.]);
}

#[test]
fn raw_rational_flag_matrix() {
    for flag in ["1", "-1", "0.5", "NaN", "inf", "-inf"] {
        let result = parse(&format!("0 0 0 0 {flag} 1 2 3 -4 0 1 2 3")).unwrap();
        assert_eq!(result.net, vec![vec![[1., 2., 3., -4.]]], "{flag}");
    }
    for flag in ["0", "-0", "0.0", "-0.0"] {
        let result = parse(&format!("0 0 0 0 {flag} 1 2 3 0 1 2 3")).unwrap();
        assert_eq!(result.net[0][0][3], NOW, "{flag}");
    }
}

#[test]
fn every_count_rejects_invalid_nonfinite_and_exclusive_values() {
    let invalid = [
        "-1".to_owned(),
        "0.5".to_owned(),
        "NaN".to_owned(),
        "inf".to_owned(),
        "-inf".to_owned(),
        exclusive().to_string(),
        (exclusive() * 2.0).to_string(),
        "1e300".to_owned(),
    ];
    for position in 0..4 {
        for value in &invalid {
            let mut tokens = vec!["0".to_owned(); position];
            tokens.push(value.clone());
            let rounded = value.parse::<f64>().unwrap();
            assert_eq!(
                parse(&tokens.join(" ")).unwrap_err(),
                format!("{PATH}: bad count {rounded}"),
                "position={position}, value={value}"
            );
        }
    }
}

#[test]
fn huge_but_castable_counts_grow_only_after_successful_reads() {
    let huge = largest_float_count().to_string();
    for position in 0..4 {
        let mut tokens = MINIMAL
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        tokens[position] = huge.clone();
        let error = parse(&tokens.join(" ")).unwrap_err();
        // Tiny text cannot supply the declared net/knots. On 32-bit targets,
        // p/q == MAX reaches checked overflow before trying to consume knots.
        let expected = if usize::BITS <= 53 && position >= 2 {
            format!("{PATH}: knot count overflow")
        } else {
            format!("{PATH}: truncated")
        };
        assert_eq!(error, expected, "position={position}");
    }
}

#[test]
fn knot_additions_overflow_independently_without_large_payloads() {
    assert_eq!(sms::knot_count(PATH, 1, 2).unwrap(), 5);
    for (n, p) in [(usize::MAX, 1), (usize::MAX - 1, 0), (0, usize::MAX)] {
        assert_eq!(
            sms::knot_count(PATH, n, p).unwrap_err(),
            format!("{PATH}: knot count overflow")
        );
    }
    assert_eq!(
        sms::knot_count(PATH, usize::MAX - 2, 0).unwrap(),
        usize::MAX
    );
}

#[test]
fn every_header_point_and_knot_truncation() {
    for full in [
        MINIMAL,
        "0 0 0 0 1 1 2 3 4 0 1 2 3",
        "1 0 1 0 0 1 2 3 4 5 6 0 0 1 1 0 1",
    ] {
        let tokens = full.split_whitespace().collect::<Vec<_>>();
        assert!(parse(full).is_ok());
        for end in 0..tokens.len() {
            assert_eq!(
                parse(&tokens[..end].join(" ")).unwrap_err(),
                format!("{PATH}: truncated"),
                "prefix={end}, full={full}"
            );
        }
    }
}

#[test]
fn eager_bad_tail_beats_bad_counts_and_all_truncations() {
    let parse_error = format!("{PATH}: {}", "bad-token".parse::<f64>().unwrap_err());
    for prefix in ["-1", "NaN", "", "0 0 0 0 0", "0 0 0 0 0 1 2 3 0", MINIMAL] {
        assert_eq!(
            parse(&format!("{prefix} bad-token")).unwrap_err(),
            parse_error
        );
    }
    // First malformed token wins, not a later malformed token or header error.
    assert_eq!(parse("-1 bad-token 1e999x").unwrap_err(), parse_error);
}

#[test]
fn numeric_tails_and_unvalidated_geometry_are_accepted() {
    let expected = parse(MINIMAL).unwrap();
    assert_eq!(
        parse(&format!("{MINIMAL} 99 NaN inf -inf -0")).unwrap(),
        expected
    );
    // Degree exceeds net size; knots are descending/nonfinite and w is zero.
    let result = parse("0 0 2 1 1 NaN inf -inf 0 3 2 NaN 0 inf -inf 1").unwrap();
    assert_eq!((result.p, result.q), (2, 1));
    assert!(result.net[0][0][0].is_nan());
    assert_eq!(result.net[0][0][3], 0.0);
    assert!(result.u[2].is_nan());
    assert_eq!(result.v, vec![f64::INFINITY, f64::NEG_INFINITY, 1.0]);
}

#[test]
fn combined_errors_keep_header_net_u_v_processing_order() {
    assert_eq!(parse("-1 -2").unwrap_err(), format!("{PATH}: bad count -1"));
    let huge = largest_float_count();
    // Net truncation wins before either knot sizing; a huge q cannot preflight
    // ahead of u consumption. On 64-bit, exercise arithmetic overflow separately
    // above rather than constructing thousands of points to reach that branch.
    for text in [
        format!("0 0 {huge} {huge} 0 1 2"),
        format!("0 0 0 {huge} 0 1 2 3 0"),
    ] {
        assert_eq!(parse(&text).unwrap_err(), format!("{PATH}: truncated"));
    }
    if usize::BITS <= 53 {
        assert_eq!(
            parse(&format!("0 0 0 {huge} 0 1 2 3 0 1")).unwrap_err(),
            format!("{PATH}: knot count overflow")
        );
    }
}
