//! Bounded decoding of the `.smc` curve fixture files the unit runners read.
//!
//! Upstream text form (`N_CrvReadFromFilePtr`): `n`, `p`, rational flag, dimension,
//! `n + 1` control points (`x y [z]`, or `wx wy [wz] w` when rational) and `n + p + 2`
//! knots.
//!
//! The runners validated the whole file eagerly (a malformed tail precedes header
//! errors) but then trusted the header: `Vec::with_capacity(n + 1)`, `n + p + 2` and the
//! point loop all followed the declared count, so a short file with a huge `n`
//! reserved (or overflowed) gigabytes before the truncation was ever seen.
//!
//! Deviations: a count must be finite, non-negative, integral and below
//! `2^usize::BITS` (the exclusive boundary of the `f64`-to-`usize` cast), and the
//! declared payload is checked (`checked_add`/`checked_mul`) and compared with the
//! numbers actually present before any output grows. Parsing stays eager: every token
//! must be a number before the headers are inspected, and numeric tails are ignored.

use std::vec::IntoIter;

use solidcore::nurbs::math::{NOW, NOZ};

use crate::HarnessError;

/// A decoded `.smc` curve: control points (weight `NOW` when not rational), degree and
/// knots.
pub(crate) struct SmcCurve {
    /// The `n + 1` control points, `[x, y, z, w]`.
    pub(crate) cpts: Vec<[f64; 4]>,
    /// The curve degree `p`.
    pub(crate) p: usize,
    /// The `n + p + 2` knots.
    pub(crate) u: Vec<f64>,
}

/// Reads and decodes an `.smc` curve of any dimension (`z` is `NOZ` unless the
/// dimension is 3).
pub(crate) fn read(path: &str) -> Result<SmcCurve, HarnessError> {
    decode(&text(path)?, path)
}

/// Reads and decodes a 3D `.smc` curve, rejecting any other dimension as
/// `not a 3D curve`.
pub(crate) fn read_3d(path: &str) -> Result<SmcCurve, HarnessError> {
    decode_3d(&text(path)?, path)
}

/// Reads the file's text (`{path}: {io error}` on failure, as the runners did).
fn text(path: &str) -> Result<String, HarnessError> {
    std::fs::read_to_string(path).map_err(|e| HarnessError::new(format!("{path}: {e}")))
}

/// Decodes an `.smc` curve of any dimension (`z` is `NOZ` unless the dimension is 3).
pub(crate) fn decode(text: &str, path: &str) -> Result<SmcCurve, HarnessError> {
    decode_impl(text, path, false)
}

/// Decodes a 3D `.smc` curve, rejecting any other dimension as `not a 3D curve`.
pub(crate) fn decode_3d(text: &str, path: &str) -> Result<SmcCurve, HarnessError> {
    decode_impl(text, path, true)
}

/// One header or payload number; a file that ends early is `truncated`.
fn number(numbers: &mut IntoIter<f64>, path: &str) -> Result<f64, HarnessError> {
    numbers
        .next()
        .ok_or_else(|| HarnessError::new(format!("{path}: truncated")))
}

fn decode_impl(text: &str, path: &str, only_3d: bool) -> Result<SmcCurve, HarnessError> {
    let error = |message: String| HarnessError::new(format!("{path}: {message}"));
    // Eager lexical validation of the whole file, including ignored tails, before any
    // header is inspected (the readers' long-standing malformed-tail precedence).
    let mut numbers = text
        .split_whitespace()
        .map(|token| token.parse::<f64>().map_err(|e| error(e.to_string())))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let count = |value: f64| {
        // `usize::MAX as f64` rounds up, so the `2^BITS` boundary is exclusive.
        if !value.is_finite()
            || value < 0.0
            || value.fract() != 0.0
            || value >= 2.0_f64.powi(usize::BITS as i32)
        {
            return Err(error(format!("bad count {value}")));
        }
        Ok(value as usize)
    };
    let n = count(number(&mut numbers, path)?)?;
    let p = count(number(&mut numbers, path)?)?;
    let rational = number(&mut numbers, path)? != 0.0;
    let dim = count(number(&mut numbers, path)?)?;
    if only_3d && dim != 3 {
        return Err(error("not a 3D curve".to_owned()));
    }
    let overflow = || error("count overflow".to_owned());
    let points = n.checked_add(1).ok_or_else(overflow)?;
    let knots = n
        .checked_add(p)
        .and_then(|v| v.checked_add(2))
        .ok_or_else(overflow)?;
    let stride = 2usize
        .checked_add(usize::from(dim == 3))
        .and_then(|v| v.checked_add(usize::from(rational)))
        .ok_or_else(overflow)?;
    let payload = points
        .checked_mul(stride)
        .and_then(|v| v.checked_add(knots))
        .ok_or_else(overflow)?;
    // Preflight the numbers the file actually holds before growing any output.
    if numbers.len() < payload {
        return Err(error("truncated".to_owned()));
    }
    let mut cpts = Vec::new();
    for _ in 0..points {
        let (x, y) = (number(&mut numbers, path)?, number(&mut numbers, path)?);
        let z = if dim == 3 {
            number(&mut numbers, path)?
        } else {
            NOZ
        };
        let w = if rational {
            number(&mut numbers, path)?
        } else {
            NOW
        };
        cpts.push([x, y, z, w]);
    }
    let mut u = Vec::new();
    for _ in 0..knots {
        u.push(number(&mut numbers, path)?);
    }
    Ok(SmcCurve { cpts, p, u })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "fixtures/example.smc";

    fn failure(text: &str) -> String {
        let result = decode(text, PATH);
        assert!(result.is_err(), "unexpected success: {text}");
        result.err().map_or_else(String::new, |e| e.to_string())
    }

    fn assert_error(text: &str, message: &str) {
        assert_eq!(failure(text), format!("{PATH}: {message}"));
    }

    #[test]
    fn dimensions_rational_flags_and_undivided_order() {
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
                let c = decode(&text, PATH).unwrap();
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
    fn three_dimensional_variant_rejects_other_dimensions() {
        let c = decode_3d("0 0 0 3 1 2 3 10 11", PATH).unwrap();
        assert_eq!(c.cpts, [[1.0, 2.0, 3.0, NOW]]);
        for dim in ["0", "1", "2", "4", "100"] {
            let text = format!("0 0 0 {dim} 1 2 10 11");
            assert_eq!(
                decode_3d(&text, PATH).err().map(|e| e.to_string()),
                Some(format!("{PATH}: not a 3D curve"))
            );
            assert!(decode(&text, PATH).is_ok());
        }
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
                let text = header.join(" ");
                assert_eq!(
                    decode(&text, PATH).err().map(|e| e.to_string()),
                    Some(format!("{PATH}: bad count {parsed}"))
                );
                assert_eq!(
                    decode_3d(&text, PATH).err().map(|e| e.to_string()),
                    Some(format!("{PATH}: bad count {parsed}"))
                );
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
        // An absurd dimension does not grow a point; only `== 3` adds a coordinate.
        let c = decode(&format!("0 0 0 {huge} 1 2 10 11"), PATH).unwrap();
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
            format!("{largest} {remainder} 0 2"), // the final +2
            format!("{half} 0 0 2"),              // points * 2
            format!("{quarter} 0 1 3"),           // points * 4
            format!("{quarter} 0 0 3"),           // coordinates and knots
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
            assert!(decode(text, PATH).is_ok());
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
        assert_error("0 0 NaN -3", "bad count -3");
    }

    #[test]
    fn numeric_tails_and_nonfinite_payload_are_preserved() {
        let c = decode("0 0 0 2 1 2 10 11 999 NaN inf -inf", PATH).unwrap();
        assert_eq!(c.cpts, [[1.0, 2.0, NOZ, NOW]]);
        assert_eq!(c.u, [10.0, 11.0]);
        let c = decode("0 0 1 3 NaN inf -inf -0 NaN inf", PATH).unwrap();
        assert!(c.cpts[0][0].is_nan());
        assert_eq!(c.cpts[0][1], f64::INFINITY);
        assert_eq!(c.cpts[0][2], f64::NEG_INFINITY);
        assert_eq!(c.cpts[0][3].to_bits(), (-0.0_f64).to_bits());
        assert!(c.u[0].is_nan());
        assert_eq!(c.u[1], f64::INFINITY);
    }

    #[test]
    fn zero_length_curve_and_large_degree_shape() {
        let c = decode("0 0 0 2 1 2 10 11", PATH).unwrap();
        assert_eq!(c.p, 0);
        assert_eq!(c.u, [10.0, 11.0]);
        let knots = (10..19).map(|k| k.to_string()).collect::<Vec<_>>();
        let c = decode(&format!("0 7 0 2 1 2 {}", knots.join(" ")), PATH).unwrap();
        assert_eq!(c.p, 7);
        assert_eq!(c.u.len(), 9);
    }
}
