//! Regression tests for the runner's actual SMC decoder, without C++ linkage.

use difftest::HarnessError;
use solidcore::nurbs::math::{NOW, NOZ};

#[path = "../src/units/fixtures/nlib_crv_tool_p1_smc.rs"]
mod smc;

const PATH: &str = "fixtures/example.smc";

fn failure(text: &str) -> String {
    let result = smc::decode(text, PATH);
    assert!(result.is_err(), "unexpected success: {text}");
    result.err().map_or_else(String::new, |e| e.to_string())
}

fn assert_error(text: &str, message: &str) {
    assert_eq!(failure(text), format!("{PATH}: {message}"));
}

#[test]
fn dimensions_rational_flags_shape_and_undivided_order() {
    for dim in [0, 1, 2, 3, 4, 100] {
        for flag in ["0", "-0", "1", "-1", "0.5", "-0.5", "NaN", "inf", "-inf"] {
            let rational = flag.parse::<f64>().unwrap() != 0.0;
            let mut text = format!("1 1 {flag} {dim} -0 2 ");
            if dim == 3 {
                text.push_str("3 ");
            }
            if rational {
                text.push_str("-4 ");
            }
            text.push_str("5 6 ");
            if dim == 3 {
                text.push_str("7 ");
            }
            if rational {
                text.push_str("8 ");
            }
            text.push_str("10 11 12 13");
            let c = smc::decode(&text, PATH).unwrap();
            assert_eq!(c.p, 1);
            assert_eq!(c.u, [10.0, 11.0, 12.0, 13.0]);
            assert_eq!(
                c.cpts,
                [
                    [
                        -0.0,
                        2.0,
                        if dim == 3 { 3.0 } else { NOZ },
                        if rational { -4.0 } else { NOW }
                    ],
                    [
                        5.0,
                        6.0,
                        if dim == 3 { 7.0 } else { NOZ },
                        if rational { 8.0 } else { NOW }
                    ],
                ]
            );
            assert_eq!(c.cpts[0][0].to_bits(), (-0.0_f64).to_bits());
        }
    }
}

#[test]
fn floating_counts_signed_zero_and_unrestricted_degree() {
    let c = smc::decode("-0.0 2e0 -0 3.0 -0 2 3 10 11 12 13", PATH).unwrap();
    assert_eq!(c.p, 2);
    assert_eq!(c.cpts, [[-0.0, 2.0, 3.0, NOW]]);
    assert_eq!(c.cpts[0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(c.u, [10.0, 11.0, 12.0, 13.0]);
    let c = smc::decode("0e0 -0 0 -0.0 1 2 10 11", PATH).unwrap();
    assert_eq!(c.cpts, [[1.0, 2.0, NOZ, NOW]]);
}

#[test]
fn invalid_nonfinite_and_exclusive_counts() {
    let exclusive = 2.0_f64.powi(usize::BITS as i32).to_string();
    for value in [
        "-1", "0.5", "NaN", "inf", "-inf", "1e300", "1e999", &exclusive,
    ] {
        for index in [0, 1, 3] {
            let mut header = ["0", "0", "0", "2"];
            header[index] = value;
            let parsed = value.parse::<f64>().unwrap();
            assert_error(&header.join(" "), &format!("bad count {parsed}"));
        }
    }
}

#[test]
fn huge_headers_with_tiny_bodies_do_not_allocate() {
    let huge = 2.0_f64.powi((usize::BITS - 4) as i32).to_string();
    for index in [0, 1] {
        let mut header = ["0", "0", "0", "2"];
        header[index] = &huge;
        assert_error(&header.join(" "), "truncated");
        assert_error(&format!("{} 1 2 3", header.join(" ")), "truncated");
    }
    let c = smc::decode(&format!("0 0 0 {huge} 1 2 10 11"), PATH).unwrap();
    assert_eq!(c.cpts, [[1.0, 2.0, NOZ, NOW]]);
}

#[test]
fn checked_knot_coordinate_and_payload_arithmetic() {
    let power = |bits| 2.0_f64.powi(bits).to_string();
    let half = power(usize::BITS as i32 - 1);
    let quarter = power(usize::BITS as i32 - 2);
    let exclusive = 2.0_f64.powi(usize::BITS as i32);
    let largest = f64::from_bits(exclusive.to_bits() - 1).floor();
    let remainder = (exclusive - largest - 1.0).to_string();
    for header in [
        format!("{half} {half} 0 2"),         // n + p
        format!("{largest} {remainder} 0 2"), // final knot +2
        format!("{half} 0 0 2"),              // points * 2
        format!("{quarter} 0 1 3"),           // points * 4
        format!("{quarter} 0 0 3"),           // coordinate and knot sum
    ] {
        assert_error(&header, "count overflow");
    }
    // usize::MAX is exactly representable on 32-bit targets, unlike on 64-bit.
    if usize::BITS <= f64::MANTISSA_DIGITS {
        assert_error(&format!("{} 0 0 2", usize::MAX), "count overflow");
    }
}

#[test]
fn all_header_and_payload_truncations() {
    for text in [
        "1 1 0 2 -0 2 5 6 10 11 12 13",
        "1 1 0 3 -0 2 3 5 6 7 10 11 12 13",
        "1 1 1 2 -0 2 -4 5 6 8 10 11 12 13",
        "1 1 1 3 -0 2 3 -4 5 6 7 8 10 11 12 13",
    ] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_error(&tokens[..end].join(" "), "truncated");
        }
        assert!(smc::decode(text, PATH).is_ok());
    }
}

#[test]
fn eager_bad_tail_precedes_bad_headers_overflow_and_truncation() {
    let lexical = "bad".parse::<f64>().unwrap_err().to_string();
    for prefix in ["", "-1", "0", "0 0 0 2", "0 0 0 2 1 2 10 11"] {
        assert_error(&format!("{prefix} bad"), &lexical);
    }
    let half = 2.0_f64.powi(usize::BITS as i32 - 1);
    assert_error(&format!("{half} {half} 0 2 bad"), &lexical);
    assert_error("-1", "bad count -1");
    assert_error("0 -2", "bad count -2");
    assert_error("0 0 0 -3", "bad count -3");
    assert_error("-1 -2 0 -3", "bad count -1");
    assert_error("0 -2 0 -3", "bad count -2");
    assert_error("0 0 NaN -3", "bad count -3");
}

#[test]
fn numeric_tails_and_nonfinite_payload_are_preserved() {
    let c = smc::decode("0 0 0 2 1 2 10 11 999 NaN inf -inf", PATH).unwrap();
    assert_eq!(c.cpts, [[1.0, 2.0, NOZ, NOW]]);
    assert_eq!(c.u, [10.0, 11.0]);
    let c = smc::decode("0 0 1 3 NaN inf -inf -0 NaN inf", PATH).unwrap();
    assert!(c.cpts[0][0].is_nan());
    assert_eq!(c.cpts[0][1], f64::INFINITY);
    assert_eq!(c.cpts[0][2], f64::NEG_INFINITY);
    assert_eq!(c.cpts[0][3].to_bits(), (-0.0_f64).to_bits());
    assert!(c.u[0].is_nan());
    assert_eq!(c.u[1], f64::INFINITY);
}
