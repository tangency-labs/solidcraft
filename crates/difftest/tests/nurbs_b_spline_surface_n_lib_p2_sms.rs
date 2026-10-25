//! Non-C++ regressions exercising the production SMS decoder.
pub use difftest::HarnessError;
#[path = "../src/units/fixtures/nurbs_b_spline_surface_n_lib_p2_sms.rs"]
mod sms;

use solidcore::nurbs::math::NOW;

fn error(text: &str) -> String {
    let result = sms::decode(text, "sample.sms");
    assert!(result.is_err(), "accepted malformed SMS: {text}");
    match result {
        Err(error) => error.to_string(),
        Ok(_) => String::new(),
    }
}

fn small(flag: &str) -> String {
    let rat = !matches!(flag, "0" | "-0");
    format!(
        "0 0 0 0 {flag} -0 2 -3 {} 0 1 2 3",
        if rat { "-2" } else { "" }
    )
}

#[test]
fn nonsquare_v_fast_and_unrestricted_degrees() {
    let mut text = "1e0 2.0 4e0 0 -0 ".to_owned();
    for i in 0..18 {
        text.push_str(&i.to_string());
        text.push(' ');
    }
    for i in 20..31 {
        text.push_str(&i.to_string());
        text.push(' ');
    }
    let s = sms::decode(&text, "net.sms").unwrap();
    assert_eq!((s.p, s.q), (4, 0));
    assert_eq!(s.net.len(), 2);
    for (i, row) in s.net.iter().enumerate() {
        assert_eq!(row.len(), 3);
        for (j, point) in row.iter().enumerate() {
            let k = (i * 3 + j) * 3;
            assert_eq!(*point, [k as f64, (k + 1) as f64, (k + 2) as f64, NOW]);
        }
    }
    assert_eq!(s.u, [20., 21., 22., 23., 24., 25., 26.]);
    assert_eq!(s.v, [27., 28., 29., 30.]);
}

#[test]
fn rational_nonsquare_net_is_not_dehomogenized() {
    let text = "1 0 1 0 0.5 2 -4 6 2 -3 9 -0 -3 0 0 1 1 0 1";
    let s = sms::decode(text, "rational.sms").unwrap();
    assert_eq!((s.p, s.q), (1, 0));
    assert_eq!(
        s.net,
        vec![vec![[2., -4., 6., 2.]], vec![[-3., 9., -0., -3.]]]
    );
    assert_eq!(s.net[1][0][2].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u, [0., 0., 1., 1.]);
    assert_eq!(s.v, [0., 1.]);
}

#[test]
fn raw_flags_homogeneous_weights_and_signed_zero() {
    for flag in ["0", "-0", "1", "-1", "0.5", "-0.5", "NaN", "inf", "-inf"] {
        let s = sms::decode(&small(flag), "flag.sms").unwrap();
        let point = s.net[0][0];
        assert_eq!(point[0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(&point[1..3], &[2., -3.]);
        assert_eq!(
            point[3],
            if flag.parse::<f64>().unwrap() == 0.0 {
                NOW
            } else {
                -2.
            }
        );
        assert_eq!((s.p, s.q), (0, 0));
    }
    for weight in ["0", "-0", "0.25", "inf", "-inf", "NaN"] {
        let s = sms::decode(&format!("0 0 0 0 1 2 3 4 {weight} 0 1 0 1"), "w").unwrap();
        assert_eq!(&s.net[0][0][..3], &[2., 3., 4.]);
        assert_eq!(
            s.net[0][0][3].to_bits(),
            weight.parse::<f64>().unwrap().to_bits()
        );
    }
}

#[test]
fn nonfinite_payload_and_numeric_tails_are_not_validated() {
    let s = sms::decode(
        "-0 0e0 0.0 -0 1 NaN inf -inf -0 -0 NaN inf -inf 42 NaN inf",
        "p",
    )
    .unwrap();
    assert!(s.net[0][0][0].is_nan());
    assert_eq!(s.net[0][0][1], f64::INFINITY);
    assert_eq!(s.net[0][0][2], f64::NEG_INFINITY);
    assert_eq!(s.net[0][0][3].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u[0].to_bits(), (-0.0_f64).to_bits());
    assert!(s.u[1].is_nan());
    assert_eq!(s.v, [f64::INFINITY, f64::NEG_INFINITY]);
}

#[test]
fn all_count_positions_reject_invalid_values_and_exclusive_boundary() {
    let boundary = 2.0_f64.powi(usize::BITS as i32).to_string();
    for bad in ["-1", "0.5", "NaN", "inf", "-inf", "1e300", &boundary] {
        for position in 0..4 {
            let mut header = vec!["0"; position];
            header.push(bad);
            assert!(error(&header.join(" ")).starts_with("sample.sms: bad count "));
        }
    }
}

#[test]
fn eager_lexical_errors_precede_headers_overflows_and_missing_body() {
    let parse_error = "oops".parse::<f64>().unwrap_err().to_string();
    for prefix in ["", "-1", "0 0", "1e300 0 0 0 0", &small("1")] {
        assert_eq!(
            error(&format!("{prefix} oops")),
            format!("sample.sms: {parse_error}")
        );
    }
    let h = 2.0_f64.powi((usize::BITS - 1) as i32);
    assert_eq!(
        error(&format!("{h} {h} 0 0 0 oops")),
        format!("sample.sms: {parse_error}")
    );
    assert_eq!(error("-1"), "sample.sms: bad count -1");
    assert_eq!(error("0 -2"), "sample.sms: bad count -2");
    assert_eq!(error("0 0 -3"), "sample.sms: bad count -3");
    assert_eq!(error("0 0 0 -4"), "sample.sms: bad count -4");
}

#[test]
fn every_prefix_of_rational_and_polynomial_payload_is_truncated() {
    for flag in ["0", "1"] {
        let text = small(flag);
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_eq!(
                error(&tokens[..end].join(" ")),
                "sample.sms: truncated",
                "prefix {end}, flag {flag}"
            );
        }
        sms::decode(&text, "complete").unwrap();
    }
}

#[test]
fn huge_in_range_counts_with_tiny_bodies_do_not_allocate_output() {
    let huge = 2.0_f64.powi((usize::BITS - 4) as i32).to_string();
    for position in 0..4 {
        let mut header = ["0", "0", "0", "0", "0"];
        header[position] = &huge;
        assert_eq!(error(&header.join(" ")), "sample.sms: truncated");
    }
}

#[test]
fn checked_arithmetic_failures() {
    let h = 1_usize << (usize::BITS - 1);
    let quarter = h / 2;
    // The last exactly representable float below the exclusive boundary.
    let gap = if usize::BITS > 53 {
        1_usize << (usize::BITS - 53)
    } else {
        1
    };
    let near = usize::MAX - (gap - 1).max(1);
    let headers = [
        format!("{h} 2 0 0 0"),                             // points multiplication
        format!("{h} 0 {h} 0 0"),                           // first u knot addition
        format!("{near} 0 {} 0 0", gap - 2_usize.min(gap)), // final u +2
        format!("0 {h} 0 {h} 0"),                           // first v knot addition
        format!("0 {near} 0 {} 0", gap - 2_usize.min(gap)), // final v +2
        format!("{quarter} 0 0 0 1"),                       // coordinate multiplication
        format!("{quarter} 0 0 0 0"),                       // coordinates + u
        format!("0 0 {h} {h} 0"),                           // payload + v
    ];
    for header in headers {
        assert_eq!(error(&header), "sample.sms: count overflow", "{header}");
    }
    // usize::MAX itself is representable only on narrower targets.
    if usize::BITS <= 53 {
        for header in [
            format!("{} 0 0 0 0", usize::MAX),
            format!("0 {} 0 0 0", usize::MAX),
        ] {
            assert_eq!(error(&header), "sample.sms: count overflow");
        }
    }
}

#[test]
fn available_fixture_bit_census() {
    fn refs(value: &serde_json::Value, sms: &mut Vec<String>, smc: &mut usize) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    if key == "sms" {
                        sms.push(child["$upstream_file"].as_str().unwrap().to_owned());
                    }
                    if key == "smc" {
                        *smc += 1;
                    }
                    refs(child, sms, smc);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    refs(child, sms, smc);
                }
            }
            _ => {}
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("fixtures/nurbs.b_spline_surface_n_lib_p2/cases.json"))
            .unwrap(),
    )
    .unwrap();
    let cases = json["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 54);
    let mut files = Vec::new();
    let mut smc = 0;
    refs(&json, &mut files, &mut smc);
    assert_eq!((files.len(), smc), (14, 0));
    files.sort();
    files.dedup();
    assert_eq!(files.len(), 3);
    let mut checked = 0;
    for (name, header, token_count) in [
        ("Surface.sms", [13, 1, 3, 1], 111),
        ("offset_nurb.sms", [5, 1, 3, 1], 55),
        ("DropCurveSurface.sms", [1, 1, 1, 1], 25),
    ] {
        assert!(files.iter().any(|file| file == name));
        let path = root.join("upstream/TestFiles/pt_TestFiles").join(name);
        if !path.exists() {
            eprintln!("unavailable fixture: {}", path.display());
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let values: Vec<f64> = text
            .split_whitespace()
            .map(|t| t.parse().unwrap())
            .collect();
        assert_eq!(values.len(), token_count);
        assert_eq!(
            &values[..5],
            &[
                header[0] as f64,
                header[1] as f64,
                header[2] as f64,
                header[3] as f64,
                0.
            ]
        );
        let s = sms::decode(&text, path.to_str().unwrap()).unwrap();
        assert_eq!((s.p, s.q), (header[2], header[3]));
        assert_eq!(s.net.len(), header[0] + 1);
        let mut expected = values[5..].iter();
        for row in &s.net {
            assert_eq!(row.len(), header[1] + 1);
            for point in row {
                for coordinate in &point[..3] {
                    assert_eq!(coordinate.to_bits(), expected.next().unwrap().to_bits());
                }
                assert_eq!(point[3].to_bits(), NOW.to_bits());
            }
        }
        assert_eq!(s.u.len(), header[0] + header[2] + 2);
        assert_eq!(s.v.len(), header[1] + header[3] + 2);
        for knot in s.u.iter().chain(&s.v) {
            assert_eq!(knot.to_bits(), expected.next().unwrap().to_bits());
        }
        assert!(expected.next().is_none());
        checked += 1;
    }
    eprintln!("fixture census: 54 cases, 14 SMS references, 0 SMC references; {checked}/3 available files bit-checked");
}
