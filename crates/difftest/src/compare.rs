//! Comparison of a C++ [`Output`] against a Rust [`Output`].
//!
//! Rule for each float pair `(a, b)` (a = C++, b = Rust):
//! * `a == b` (bitwise-equal value, including `+0 == -0` and equal infinities) passes;
//! * both NaN passes; exactly one NaN fails;
//! * otherwise passes iff `|a-b| <= REL_TOL * max(|a|,|b|)`;
//! * an absolute floor applies only where the unit documents one
//!   ([`AbsFloor`]) for that value key.
//!
//! Integers, topology and error strings must match exactly; key sets and
//! array lengths must match.

use crate::output::{Output, Topology};

/// Relative tolerance. Fixed by project policy: never weaken it.
pub const REL_TOL: f64 = 1e-9;

/// Max mismatches recorded per case (all are counted).
pub const MAX_RECORDED: usize = 20;

/// A documented absolute floor for one value key of one unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbsFloor {
    /// Key in [`Output::values`].
    pub key: &'static str,
    /// Absolute tolerance used in addition to the relative rule.
    pub abs: f64,
    /// Why this floor is legitimate (goes into the report).
    pub why: &'static str,
}

/// One recorded difference.
#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    /// Path of the differing item, e.g. `values.basis[2]`, `topology.entities.faces`.
    pub field: String,
    /// C++ side, rendered.
    pub cpp: String,
    /// Rust side, rendered.
    pub rust: String,
    /// Relative error for float mismatches.
    pub rel_err: Option<f64>,
}

/// Comparison result for one case.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CaseDiff {
    /// Total number of mismatches (may exceed `mismatches.len()`).
    pub mismatch_count: usize,
    /// First [`MAX_RECORDED`] mismatches.
    pub mismatches: Vec<Mismatch>,
    /// Max relative error over all compared float pairs (0 if none; may be inf).
    pub max_rel_err: f64,
    /// Number of float pairs compared.
    pub values_compared: usize,
    /// Number of integers, topology ids and adjacency entries compared.
    pub exact_compared: usize,
    /// Compared items where at least one side is not zero: floats or integers other
    /// than `0`, and every topology id or adjacency entry. A passing case with
    /// `nonzero_compared == 0` agreed only on zeros (e.g. `count = 0`), which a Rust
    /// error swallowed into a default would also produce; reported, never gated.
    pub nonzero_compared: usize,
}

impl CaseDiff {
    /// True if no mismatch was found.
    pub fn pass(&self) -> bool {
        self.mismatch_count == 0
    }

    fn push(&mut self, m: Mismatch) {
        self.mismatch_count += 1;
        if self.mismatches.len() < MAX_RECORDED {
            self.mismatches.push(m);
        }
    }
}

/// Relative error `|a-b| / max(|a|,|b|)`; 0 when equal (or both NaN), inf when
/// exactly one is NaN or they are unequal and one is infinite.
pub fn rel_err(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return if a.is_nan() && b.is_nan() {
            0.0
        } else {
            f64::INFINITY
        };
    }
    if a == b {
        return 0.0;
    }
    if a.is_infinite() || b.is_infinite() {
        return f64::INFINITY;
    }
    let diff = (a - b).abs();
    let scale = a.abs().max(b.abs());
    if diff.is_infinite() {
        // Both inputs are finite here. Scale before subtracting only when their
        // difference overflows; retain the usual operation order otherwise.
        (a / scale - b / scale).abs()
    } else {
        diff / scale
    }
}

/// Whether `a` and `b` agree under the rule above (`floor` = documented abs floor).
pub fn values_agree(a: f64, b: f64, floor: Option<f64>) -> bool {
    if a.is_nan() || b.is_nan() {
        return a.is_nan() && b.is_nan();
    }
    if a == b {
        return true;
    }
    if a.is_infinite() || b.is_infinite() {
        return false;
    }
    let diff = (a - b).abs();
    diff <= REL_TOL * a.abs().max(b.abs()) || floor.is_some_and(|f| diff <= f)
}

fn render_f64(x: f64) -> String {
    format!("{x:e}")
}

/// Compares two outputs. `floors` are the unit's documented absolute floors.
pub fn compare(cpp: &Output, rust: &Output, floors: &[AbsFloor]) -> CaseDiff {
    let mut d = CaseDiff::default();

    if cpp.error.is_some() || rust.error.is_some() {
        if cpp.error != rust.error {
            d.push(Mismatch {
                field: "error".into(),
                cpp: format!("{:?}", cpp.error),
                rust: format!("{:?}", rust.error),
                rel_err: None,
            });
        }
        return d;
    }

    // Floats.
    for key in union_keys(cpp.values.keys(), rust.values.keys()) {
        let (Some(a), Some(b)) = (cpp.values.get(&key), rust.values.get(&key)) else {
            d.push(missing(
                &format!("values.{key}"),
                cpp.values.contains_key(&key),
            ));
            continue;
        };
        if a.len() != b.len() {
            d.push(Mismatch {
                field: format!("values.{key}.len"),
                cpp: a.len().to_string(),
                rust: b.len().to_string(),
                rel_err: None,
            });
            continue;
        }
        let floor = floors.iter().find(|f| f.key == key).map(|f| f.abs);
        for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
            d.values_compared += 1;
            if x != 0.0 || y != 0.0 {
                d.nonzero_compared += 1;
            }
            let r = rel_err(x, y);
            if r > d.max_rel_err || r.is_nan() {
                d.max_rel_err = r;
            }
            if !values_agree(x, y, floor) {
                d.push(Mismatch {
                    field: format!("values.{key}[{i}]"),
                    cpp: render_f64(x),
                    rust: render_f64(y),
                    rel_err: Some(r),
                });
            }
        }
    }

    // Integers.
    for key in union_keys(cpp.ints.keys(), rust.ints.keys()) {
        let (Some(a), Some(b)) = (cpp.ints.get(&key), rust.ints.get(&key)) else {
            d.push(missing(&format!("ints.{key}"), cpp.ints.contains_key(&key)));
            continue;
        };
        exact_list(&mut d, &format!("ints.{key}"), a, b);
    }

    // Topology.
    match (&cpp.topology, &rust.topology) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            let (exact, nonzero) = (d.exact_compared, d.nonzero_compared);
            compare_topology(&mut d, a, b);
            // topology ids are indices: every compared entry is a real result
            d.nonzero_compared = nonzero + (d.exact_compared - exact);
        }
        (a, _) => d.push(missing("topology", a.is_some())),
    }
    d
}

fn compare_topology(d: &mut CaseDiff, a: &Topology, b: &Topology) {
    for key in union_keys(a.entities.keys(), b.entities.keys()) {
        let field = format!("topology.entities.{key}");
        match (a.entities.get(&key), b.entities.get(&key)) {
            (Some(x), Some(y)) => exact_list(d, &field, x, y),
            (x, _) => d.push(missing(&field, x.is_some())),
        }
    }
    for key in union_keys(a.adjacency.keys(), b.adjacency.keys()) {
        let field = format!("topology.adjacency.{key}");
        match (a.adjacency.get(&key), b.adjacency.get(&key)) {
            (Some(x), Some(y)) => {
                if x.len() == y.len() {
                    for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
                        d.exact_compared += 1;
                        if p != q {
                            d.push(Mismatch {
                                field: format!("{field}[{i}]"),
                                cpp: format!("{p:?}"),
                                rust: format!("{q:?}"),
                                rel_err: None,
                            });
                        }
                    }
                } else {
                    d.push(Mismatch {
                        field: format!("{field}.len"),
                        cpp: x.len().to_string(),
                        rust: y.len().to_string(),
                        rel_err: None,
                    });
                }
            }
            (x, _) => d.push(missing(&field, x.is_some())),
        }
    }
}

fn exact_list(d: &mut CaseDiff, field: &str, a: &[i64], b: &[i64]) {
    if a.len() != b.len() {
        d.push(Mismatch {
            field: format!("{field}.len"),
            cpp: a.len().to_string(),
            rust: b.len().to_string(),
            rel_err: None,
        });
        return;
    }
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        d.exact_compared += 1;
        if *x != 0 || *y != 0 {
            d.nonzero_compared += 1;
        }
        if x != y {
            d.push(Mismatch {
                field: format!("{field}[{i}]"),
                cpp: x.to_string(),
                rust: y.to_string(),
                rel_err: None,
            });
        }
    }
}

fn missing(field: &str, in_cpp: bool) -> Mismatch {
    let (cpp, rust) = if in_cpp {
        ("present", "missing")
    } else {
        ("missing", "present")
    };
    Mismatch {
        field: field.to_string(),
        cpp: cpp.into(),
        rust: rust.into(),
        rel_err: None,
    }
}

fn union_keys<'a>(
    a: impl Iterator<Item = &'a String>,
    b: impl Iterator<Item = &'a String>,
) -> Vec<String> {
    let mut v: Vec<String> = a.chain(b).cloned().collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_relative_error_does_not_overflow() {
        for (a, b, expected) in [
            (f64::MAX, -f64::MAX, 2.0),
            (-f64::MAX, f64::MAX, 2.0),
            (f64::MAX, -f64::MAX / 2.0, 1.5),
            (-f64::MAX / 2.0, f64::MAX, 1.5),
            (-f64::MAX, f64::MAX / 2.0, 1.5),
            (f64::MAX / 2.0, -f64::MAX, 1.5),
        ] {
            assert_eq!(rel_err(a, b), expected, "a={a}, b={b}");
            let d = compare(
                &Output::new().scalar("v", a),
                &Output::new().scalar("v", b),
                &[],
            );
            assert!(!d.pass());
            assert_eq!(d.mismatch_count, 1);
            assert_eq!(d.values_compared, 1);
            assert_eq!(d.max_rel_err, expected);
            assert_eq!(d.mismatches[0].rel_err, Some(expected));
            assert_eq!(crate::json::num(d.max_rel_err).as_f64(), Some(expected));
        }
    }

    #[test]
    fn relative_error_preserves_ordinary_and_nonfinite_cases() {
        for (a, b) in [
            (1.0, 1.0 + 1e-8),
            (f64::MAX, f64::MAX / 2.0),
            (f64::MAX / 4.0, -f64::MAX / 4.0),
            (0.0, f64::from_bits(1)),
            (f64::from_bits(1), -f64::from_bits(1)),
        ] {
            assert_eq!(
                rel_err(a, b).to_bits(),
                ((a - b).abs() / a.abs().max(b.abs())).to_bits()
            );
        }
        for (a, b) in [
            (0.0, -0.0),
            (f64::MAX, f64::MAX),
            (f64::NAN, f64::NAN),
            (f64::INFINITY, f64::INFINITY),
            (f64::NEG_INFINITY, f64::NEG_INFINITY),
        ] {
            assert_eq!(rel_err(a, b), 0.0);
        }
        for (a, b) in [
            (f64::NAN, 1.0),
            (1.0, f64::NAN),
            (f64::INFINITY, f64::MAX),
            (f64::MAX, f64::NEG_INFINITY),
            (f64::INFINITY, f64::NEG_INFINITY),
        ] {
            assert_eq!(rel_err(a, b), f64::INFINITY);
        }
    }

    #[test]
    fn rule() {
        assert!(values_agree(0.0, -0.0, None));
        assert!(values_agree(1.0, 1.0 + 0.9e-9, None));
        assert!(!values_agree(1.0, 1.0 + 1.1e-9, None));
        assert!(!values_agree(0.0, 1e-300, None));
        assert!(values_agree(0.0, 1e-300, Some(1e-12)));
        assert!(values_agree(f64::NAN, f64::NAN, None));
        assert!(!values_agree(f64::NAN, 1.0, None));
        assert!(!values_agree(f64::INFINITY, 1e308, None));
        assert!(values_agree(f64::INFINITY, f64::INFINITY, None));
    }

    #[test]
    fn perturbation_caught() {
        let a = Output::new().values("v", vec![1.0, 2.0, 3.0]).int("n", 3);
        let mut b = a.clone();
        assert!(compare(&a, &b, &[]).pass());
        b.values.get_mut("v").unwrap()[1] *= 1.0 + 1e-8;
        let d = compare(&a, &b, &[]);
        assert!(!d.pass());
        assert_eq!(d.mismatches[0].field, "values.v[1]");
        assert!(d.max_rel_err > 9e-9);
    }

    #[test]
    fn structure_mismatches() {
        let a = Output::new()
            .values("v", vec![1.0])
            .topology(Topology::new().count("faces", 6));
        let b = Output::new()
            .values("v", vec![1.0, 2.0])
            .topology(Topology::new().count("faces", 5));
        let d = compare(&a, &b, &[]);
        assert_eq!(d.mismatch_count, 2);
        let same = compare(&a, &a, &[]);
        assert_eq!((same.values_compared, same.exact_compared), (1, 6));
        let e = compare(&Output::err("x"), &Output::new(), &[]);
        assert_eq!((e.values_compared, e.exact_compared), (0, 0));
        assert!(!e.pass());
        assert!(compare(&Output::err("x"), &Output::err("x"), &[]).pass());
    }

    #[test]
    fn status_is_compared_even_with_empty_outputs() {
        // C++ genuinely found nothing; Rust failed and produced nothing.
        let cpp = Output::new().values("points", Vec::<f64>::new());
        let rust = Output::from_result(
            Err::<Vec<f64>, _>(solidcore::SolidError::sm_err("test")),
            |_| "status_3".into(),
            |v| Output::new().values("points", v),
        );
        let d = compare(&cpp, &rust, &[]);
        assert!(!d.pass());
        assert_eq!(d.mismatches[0].field, "error");
        let ok = Output::from_result(
            Ok::<Vec<f64>, solidcore::SolidError>(Vec::new()),
            |_| "status_3".into(),
            |v| Output::new().values("points", v),
        );
        let d = compare(&cpp, &ok, &[]);
        assert!(d.pass());
        assert_eq!((d.values_compared, d.nonzero_compared), (0, 0));
        let zero = Output::new().int("n", 0).scalar("t", 0.0);
        assert_eq!(compare(&zero, &zero, &[]).nonzero_compared, 0);
        let topo = Output::new().topology(Topology::new().count("faces", 1));
        assert_eq!(compare(&topo, &topo, &[]).nonzero_compared, 1);
    }
}
