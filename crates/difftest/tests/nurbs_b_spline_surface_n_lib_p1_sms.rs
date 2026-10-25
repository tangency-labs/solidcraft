//! Non-C++ tests of the exact SMS decoder used by the surface runner.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/nurbs_b_spline_surface_n_lib_p1_sms.rs"]
mod sms;

use solidcore::nurbs::math::NOW;
use std::fmt::Write;

const PATH: &str = "test/surface.sms";

fn rejected(text: &str, message: &str) {
    let result = sms::decode(text, PATH);
    assert!(result.is_err(), "must reject, not panic");
    if let Err(error) = result {
        assert_eq!(error.to_string(), format!("{PATH}: {message}"));
    }
}

fn bits(actual: &[f64], expected: &[f64]) {
    assert_eq!(
        actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
}

fn record(rational: bool) -> String {
    // Two u rows, three v columns; unequal degrees and non-unit knot domains.
    let mut text = format!("1 2 1 2 {}", usize::from(rational));
    for i in 0..6 {
        let _ = write!(text, " {} {} {}", i * 3 + 1, i * 3 + 2, i * 3 + 3);
        if rational {
            let _ = write!(text, " {}", i + 2);
        }
    }
    text.push_str(" -2 -2 5 5 10 10 10 20 20 20");
    text
}

#[test]
fn nonsquare_polynomial_and_homogeneous_weighted_order() {
    for rational in [false, true] {
        let parsed = sms::decode(&record(rational), PATH).unwrap();
        assert_eq!((parsed.p, parsed.q), (1, 2));
        assert_eq!(parsed.net.len(), 2);
        for (i, point) in parsed.net.iter().flatten().enumerate() {
            bits(
                point,
                &[
                    (i * 3 + 1) as f64,
                    (i * 3 + 2) as f64,
                    (i * 3 + 3) as f64,
                    if rational { (i + 2) as f64 } else { NOW },
                ],
            );
        }
        assert!(parsed.net.iter().all(|row| row.len() == 3));
        bits(&parsed.u, &[-2.0, -2.0, 5.0, 5.0]);
        bits(&parsed.v, &[10.0, 10.0, 10.0, 20.0, 20.0, 20.0]);
    }
}

#[test]
fn raw_rational_flags_and_zero_indices() {
    for flag in ["0", "-0", "+0.0", "1", "-1", "0.5", "NaN", "inf", "-inf"] {
        let rational = flag.parse::<f64>().unwrap() != 0.0;
        let body = if rational { "2 4 6 2" } else { "2 4 6" };
        let parsed = sms::decode(&format!("0 0 0 0 {flag} {body} -1 3 7 9"), PATH).unwrap();
        assert_eq!((parsed.p, parsed.q), (0, 0));
        bits(
            &parsed.net[0][0],
            &[2.0, 4.0, 6.0, if rational { 2.0 } else { NOW }],
        );
    }
}

#[test]
fn decimal_scientific_counts_signed_zero_and_unrestricted_degrees() {
    let parsed = sms::decode("-0 0e3 2.0 3e0 -0 1 2 3 0 1 2 3 4 5 6 7 8", PATH).unwrap();
    assert_eq!((parsed.p, parsed.q), (2, 3));
    bits(&parsed.u, &[0.0, 1.0, 2.0, 3.0]);
    bits(&parsed.v, &[4.0, 5.0, 6.0, 7.0, 8.0]);
}

#[test]
fn signed_zero_and_nonfinite_payload_are_not_normalized() {
    let parsed = sms::decode("0 0 0 0 1 -0 NaN inf -inf -0 inf NaN -inf", PATH).unwrap();
    bits(&[parsed.net[0][0][0]], &[-0.0]);
    assert!(parsed.net[0][0][1].is_nan());
    bits(&parsed.net[0][0][2..], &[f64::INFINITY, f64::NEG_INFINITY]);
    bits(&parsed.u, &[-0.0, f64::INFINITY]);
    assert!(parsed.v[0].is_nan());
    assert_eq!(parsed.v[1], f64::NEG_INFINITY);
    let poly = sms::decode("0 0 0 0 0 -0 inf NaN 0 0 1 1", PATH).unwrap();
    bits(&[poly.net[0][0][0], poly.net[0][0][3]], &[-0.0, NOW]);
    assert!(poly.net[0][0][2].is_nan());
}

#[test]
fn invalid_and_exclusive_counts_in_every_position() {
    let upper = 2.0_f64.powi(usize::BITS as i32);
    for invalid in [
        "-1".into(),
        "0.5".into(),
        "NaN".into(),
        "inf".into(),
        "-inf".into(),
        "1e100".into(),
        "1e999".into(),
        format!("{upper:.0}"),
    ] {
        for position in 0..4 {
            let mut header = ["0", "0", "0", "0", "0"];
            header[position] = &invalid;
            let value = invalid.parse::<f64>().unwrap();
            rejected(&header.join(" "), &format!("bad count {value}"));
        }
    }
}

#[test]
fn ordered_header_validation_precedes_missing_later_headers() {
    rejected("-1", "bad count -1");
    rejected("0 -2", "bad count -2");
    rejected("0 0 -3", "bad count -3");
    rejected("0 0 0 -4", "bad count -4");
    rejected("-1 -2 -3 -4 0", "bad count -1");
    rejected("0 -2 -3 -4 0", "bad count -2");
    rejected("0 0 -3 -4 0", "bad count -3");
}

#[test]
fn huge_valid_counts_with_tiny_bodies_preflight_without_allocating() {
    let huge = 2.0_f64.powi(usize::BITS as i32 - 5);
    for position in 0..4 {
        let mut header = [
            "0".to_owned(),
            "0".into(),
            "0".into(),
            "0".into(),
            "0".into(),
        ];
        header[position] = format!("{huge:.0}");
        for tail in ["", " 1 2 3 4"] {
            rejected(&format!("{}{tail}", header.join(" ")), "truncated");
        }
    }
}

#[test]
fn checked_knot_point_stride_and_total_arithmetic() {
    let half = 2.0_f64.powi(usize::BITS as i32 - 1);
    let eighth = half / 4.0;
    for header in [
        format!("{half:.0} 0 {half:.0} 0 0"),   // n+p
        format!("0 {half:.0} 0 {half:.0} 0"),   // m+q
        format!("{half:.0} {half:.0} 0 0 0"),   // rows*cols
        format!("{half:.0} 0 0 0 0"),           // polynomial stride
        format!("{} 0 0 0 1", half / 2.0),      // rational stride
        format!("{eighth:.0} 0 {half:.0} 0 0"), // coordinates+u
        format!("{eighth:.0} 0 0 {half:.0} 0"), // +v
    ] {
        rejected(&header, "count overflow");
    }
    // On 64-bit, f64 cannot represent usize::MAX or MAX-1; no accepted count
    // can trigger just +1 or the final +2. Exercise those branches on 32-bit.
    if usize::BITS <= 53 {
        for header in [
            format!("{} 0 0 0 0", usize::MAX),
            format!("0 {} 0 0 0", usize::MAX),
            format!("0 0 {} 0 0", usize::MAX - 1),
            format!("0 0 0 {} 0", usize::MAX - 1),
        ] {
            rejected(&header, "count overflow");
        }
    }
}

#[test]
fn every_polynomial_and_rational_prefix_is_truncated() {
    for rational in [false, true] {
        let text = record(rational);
        let tokens = text.split_whitespace().collect::<Vec<_>>();
        for end in 0..tokens.len() {
            rejected(&tokens[..end].join(" "), "truncated");
        }
        assert!(sms::decode(&text, PATH).is_ok());
    }
}

#[test]
fn eager_first_lexical_error_beats_headers_overflow_and_truncation() {
    let half = 2.0_f64.powi(usize::BITS as i32 - 1);
    let lexical = "bad".parse::<f64>().unwrap_err().to_string();
    for text in [
        "bad".into(),
        "-1 bad".into(),
        "0 -2 0 0 0 bad".into(),
        "0 0 0 0 0 1 2 bad".into(),
        format!("{half:.0} {half:.0} 0 0 0 bad"),
        format!("{} bad", record(true)),
    ] {
        rejected(&text, &lexical);
    }
    rejected("bad 1e-not-a-number", &lexical);
}

#[test]
fn numeric_tails_are_eagerly_validated_but_ignored() {
    for rational in [false, true] {
        let text = record(rational);
        let a = sms::decode(&text, PATH).unwrap();
        let b = sms::decode(&format!("{text} -0 NaN inf -inf 1e999 123"), PATH).unwrap();
        assert_eq!((a.p, a.q), (b.p, b.q));
        bits(&a.u, &b.u);
        bits(&a.v, &b.v);
        for (a, b) in a.net.iter().flatten().zip(b.net.iter().flatten()) {
            bits(a, b);
        }
    }
}

#[test]
fn available_fixture_bit_census_and_unchanged_reference_inventory() {
    use std::collections::BTreeMap;
    use std::path::Path;
    fn refs(value: &serde_json::Value, key: &str, out: &mut BTreeMap<String, usize>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(path) = map
                    .get(key)
                    .and_then(|v| v.get("$upstream_file"))
                    .and_then(|v| v.as_str())
                {
                    *out.entry(path.into()).or_default() += 1;
                }
                for value in map.values() {
                    refs(value, key, out);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    refs(value, key, out);
                }
            }
            _ => {}
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sms_refs = BTreeMap::new();
    let mut smc_refs = BTreeMap::new();
    let mut cases = 0;
    for name in ["cases.json", "coverage.json"] {
        let text = std::fs::read_to_string(
            root.join("fixtures/nurbs.b_spline_surface_n_lib_p1")
                .join(name),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        cases += value["cases"].as_array().unwrap().len();
        refs(&value, "sms", &mut sms_refs);
        refs(&value, "smc", &mut smc_refs);
    }
    assert_eq!(cases, 99);
    assert_eq!((sms_refs.len(), sms_refs.values().sum::<usize>()), (3, 28));
    assert_eq!((smc_refs.len(), smc_refs.values().sum::<usize>()), (12, 13));
    let mut available = 0;
    let mut missing = Vec::new();
    for name in sms_refs.keys() {
        let path = root.join("upstream/TestFiles/pt_TestFiles").join(name);
        if !path.is_file() {
            missing.push(name.clone());
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let parsed = sms::decode(&text, name).unwrap();
        let values = text
            .split_whitespace()
            .map(|v| v.parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        let (n, m, p, q) = (
            values[0] as usize,
            values[1] as usize,
            values[2] as usize,
            values[3] as usize,
        );
        assert_eq!((parsed.net.len(), parsed.p, parsed.q), (n + 1, p, q));
        let rational = values[4] != 0.0;
        let mut cursor = 5;
        for row in &parsed.net {
            assert_eq!(row.len(), m + 1);
            for point in row {
                bits(&point[..3], &values[cursor..cursor + 3]);
                cursor += 3;
                bits(
                    &point[3..],
                    &[if rational {
                        let w = values[cursor];
                        cursor += 1;
                        w
                    } else {
                        NOW
                    }],
                );
            }
        }
        assert_eq!(parsed.u.len(), n + p + 2);
        assert_eq!(parsed.v.len(), m + q + 2);
        bits(&parsed.u, &values[cursor..cursor + parsed.u.len()]);
        cursor += parsed.u.len();
        bits(&parsed.v, &values[cursor..cursor + parsed.v.len()]);
        available += 1;
    }
    println!("SMS bit census: {available}/3 available; missing: {missing:?}; inventory 99 cases, 28 SMS refs, 13 SMC refs (SMC not decoded here)");
}
