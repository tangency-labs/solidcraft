//! An engine's raw answer to one SSI corpus case, in the corpus runner's JSON format
//! (`runner/ssi_runner.cpp` of <https://github.com/jyviko/ssi-corpus>, commit `b04c8a6`,
//! MIT OR Apache-2.0): status, curves sampled as `{xyz, uv1, uv2, s1_at_uv, s2_at_uv,
//! type}`, and points. Non-finite numbers are written as `null`, as the runner does, so
//! the corpus' own `scoring/score.py` reads these files unchanged.

use serde_json::{json, Map, Value};

use super::geom::P3;
use crate::{Deferred, HarnessError, Output};

/// `SM_SUCCESS`.
pub const SM_SUCCESS: i64 = 1000;

/// Samples per curve (the runner's `N`).
pub const N_SAMPLES: i32 = 4001;

/// One output curve.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawCurve {
    /// `SmTsectCurveType` name (`CROSSING`, `TOUCHING`, ...; `n/a` when none).
    pub ty: String,
    /// `deviation` (NaN when none).
    pub deviation: f64,
    /// The 3D curve's natural interval (when there is a 3D curve).
    pub interval: Option<[f64; 2]>,
    /// The 3D curve's `GetType()` (when there is one).
    pub class_type: Option<i64>,
    /// 3D samples (failed evaluations skipped).
    pub xyz: Vec<P3>,
    /// UV samples on surface 1.
    pub uv1: Vec<[f64; 2]>,
    /// Surface 1 at those samples (NaN where its evaluation failed).
    pub s1_at_uv: Vec<P3>,
    /// UV samples on surface 2.
    pub uv2: Vec<[f64; 2]>,
    /// Surface 2 at those samples.
    pub s2_at_uv: Vec<P3>,
}

/// One engine's result for one case.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawOutput {
    /// Case id.
    pub case: String,
    /// `cpp` or `rust`.
    pub engine: String,
    /// `SmStatus` value (`1000` = success).
    pub status: i64,
    /// Status name when the engine knows it better than the code (Rust errors).
    pub status_name: Option<String>,
    /// Domain of surface 1, `[u0, u1, v0, v1]`.
    pub domain1: [f64; 4],
    /// Domain of surface 2.
    pub domain2: [f64; 4],
    /// The approximation tolerance (`None`: upstream default).
    pub approx_tol: Option<f64>,
    /// Wall time of the intersection (not part of any report).
    pub time_ms: f64,
    /// Curves.
    pub curves: Vec<RawCurve>,
    /// Isolated points.
    pub points: Vec<P3>,
    /// Set when the Rust side reached unported code: no result.
    pub deferred: Option<Deferred>,
}

/// `SmTsectCurveType` value -> runner name.
pub fn type_name(t: i64) -> &'static str {
    match t {
        0 => "UNKNOWN",
        1 => "TOUCHING",
        2 => "CROSSING",
        3 => "TANGENT",
        4 => "COINCIDENT",
        5 => "NEAR_TANGENT",
        6 => "REGION_BOUNDARY",
        -1 => "n/a",
        _ => "?",
    }
}

/// Runner name -> `SmTsectCurveType` value (`-1` for `n/a`, `-2` for anything else).
pub fn type_code(name: &str) -> i64 {
    (-1..=6).find(|&t| type_name(t) == name).unwrap_or(-2)
}

fn num(x: f64) -> Value {
    if x.is_finite() {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    } else {
        Value::Null
    }
}

fn pts3(v: &[P3]) -> Value {
    Value::Array(
        v.iter()
            .map(|p| Value::Array(p.iter().map(|&x| num(x)).collect()))
            .collect(),
    )
}

fn pts2(v: &[[f64; 2]]) -> Value {
    Value::Array(
        v.iter()
            .map(|p| Value::Array(p.iter().map(|&x| num(x)).collect()))
            .collect(),
    )
}

fn err(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn read_f(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

fn read_pts<const N: usize>(v: Option<&Value>) -> Result<Vec<[f64; N]>, HarnessError> {
    let Some(a) = v.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    a.iter()
        .map(|p| {
            let q = p.as_array().ok_or_else(|| err("bad sample"))?;
            let mut out = [f64::NAN; N];
            if q.len() != N {
                return Err(err("bad sample size"));
            }
            for (o, x) in out.iter_mut().zip(q) {
                *o = x.as_f64().unwrap_or(f64::NAN);
            }
            Ok(out)
        })
        .collect()
}

impl RawOutput {
    /// The runner's JSON document (plus `engine` and, for a deferred Rust run, `deferred`).
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("case".into(), json!(self.case));
        o.insert("engine".into(), json!(self.engine));
        o.insert("mode".into(), json!("gsi"));
        o.insert(
            "domain1".into(),
            Value::Array(self.domain1.iter().map(|&x| num(x)).collect()),
        );
        o.insert(
            "domain2".into(),
            Value::Array(self.domain2.iter().map(|&x| num(x)).collect()),
        );
        o.insert(
            "approx_tol".into(),
            self.approx_tol.map_or_else(|| json!("default"), num),
        );
        o.insert("status".into(), json!(self.status));
        if let Some(n) = &self.status_name {
            o.insert("status_name".into(), json!(n));
        }
        o.insert("time_ms".into(), num(self.time_ms));
        let curves: Vec<Value> = self
            .curves
            .iter()
            .map(|c| {
                let mut m = Map::new();
                m.insert("type".into(), json!(c.ty));
                m.insert("deviation".into(), num(c.deviation));
                if let Some([a, b]) = c.interval {
                    m.insert("interval".into(), json!([num(a), num(b)]));
                }
                if let Some(t) = c.class_type {
                    m.insert("class_type".into(), json!(t));
                }
                m.insert("xyz".into(), pts3(&c.xyz));
                m.insert("uv1".into(), pts2(&c.uv1));
                m.insert("s1_at_uv".into(), pts3(&c.s1_at_uv));
                m.insert("uv2".into(), pts2(&c.uv2));
                m.insert("s2_at_uv".into(), pts3(&c.s2_at_uv));
                Value::Object(m)
            })
            .collect();
        o.insert("curves".into(), Value::Array(curves));
        o.insert("points".into(), pts3(&self.points));
        if let Some(d) = &self.deferred {
            o.insert(
                "deferred".into(),
                json!({"unit": d.unit, "symbol": d.symbol}),
            );
        }
        Value::Object(o)
    }

    /// Reads a document written by [`RawOutput::to_json`] (or by the corpus runner).
    pub fn from_json(v: &Value) -> Result<Self, HarnessError> {
        let dom = |k: &str| -> [f64; 4] {
            let mut d = [f64::NAN; 4];
            if let Some(a) = v.get(k).and_then(Value::as_array) {
                for (o, x) in d.iter_mut().zip(a) {
                    *o = x.as_f64().unwrap_or(f64::NAN);
                }
            }
            d
        };
        let curves = v
            .get("curves")
            .and_then(Value::as_array)
            .ok_or_else(|| err("raw output without curves"))?
            .iter()
            .map(|c| {
                Ok(RawCurve {
                    ty: c
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string(),
                    deviation: read_f(c.get("deviation")),
                    interval: c.get("interval").and_then(Value::as_array).and_then(|a| {
                        match a.as_slice() {
                            [x, y] => Some([
                                x.as_f64().unwrap_or(f64::NAN),
                                y.as_f64().unwrap_or(f64::NAN),
                            ]),
                            _ => None,
                        }
                    }),
                    class_type: c.get("class_type").and_then(Value::as_i64),
                    xyz: read_pts(c.get("xyz"))?,
                    uv1: read_pts(c.get("uv1"))?,
                    s1_at_uv: read_pts(c.get("s1_at_uv"))?,
                    uv2: read_pts(c.get("uv2"))?,
                    s2_at_uv: read_pts(c.get("s2_at_uv"))?,
                })
            })
            .collect::<Result<Vec<_>, HarnessError>>()?;
        let deferred = v.get("deferred").map(|d| Deferred {
            unit: d
                .get("unit")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            symbol: d
                .get("symbol")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
        Ok(Self {
            case: v
                .get("case")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            engine: v
                .get("engine")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            status: v
                .get("status")
                .and_then(Value::as_i64)
                .ok_or_else(|| err("raw output without status"))?,
            status_name: v
                .get("status_name")
                .and_then(Value::as_str)
                .map(str::to_string),
            domain1: dom("domain1"),
            domain2: dom("domain2"),
            approx_tol: v.get("approx_tol").and_then(Value::as_f64),
            time_ms: read_f(v.get("time_ms")),
            curves,
            points: read_pts(v.get("points"))?,
            deferred,
        })
    }

    /// The comparable record for the Rust-vs-C++ parity check: status, domains, curve
    /// count, per-curve type, class, deviation, interval and every sample.
    pub fn parity_output(&self) -> Output {
        let mut o = Output::new()
            .int("status", self.status)
            .values("domains", [self.domain1, self.domain2].concat())
            .int("n_curves", self.curves.len() as i64)
            .points("points", &self.points);
        for (i, c) in self.curves.iter().enumerate() {
            let k = |s: &str| format!("curve{i:03}.{s}");
            o = o
                .ints(
                    &k("kind"),
                    vec![type_code(&c.ty), c.class_type.unwrap_or(-1)],
                )
                .scalar(&k("deviation"), c.deviation)
                .values(&k("interval"), c.interval.map_or_else(Vec::new, Vec::from))
                .points(&k("xyz"), &c.xyz)
                .points(&k("uv1"), &c.uv1)
                .points(&k("s1_at_uv"), &c.s1_at_uv)
                .points(&k("uv2"), &c.uv2)
                .points(&k("s2_at_uv"), &c.s2_at_uv);
        }
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_with_nan() {
        let r = RawOutput {
            case: "c".into(),
            engine: "cpp".into(),
            status: SM_SUCCESS,
            approx_tol: Some(1e-6),
            curves: vec![RawCurve {
                ty: "CROSSING".into(),
                deviation: 1e-7,
                interval: Some([0.0, 1.0]),
                class_type: Some(3),
                xyz: vec![[0.0, 1.0, 2.0]],
                uv1: vec![[0.5, 0.5]],
                s1_at_uv: vec![[f64::NAN, 0.0, 0.0]],
                ..RawCurve::default()
            }],
            ..RawOutput::default()
        };
        let v = r.to_json();
        assert_eq!(v["curves"][0]["s1_at_uv"][0][0], Value::Null);
        let back = RawOutput::from_json(&v).unwrap();
        assert!(back.curves[0].s1_at_uv[0][0].is_nan());
        assert_eq!(back.curves[0].xyz, r.curves[0].xyz);
        assert_eq!(back.status, SM_SUCCESS);
    }
}
