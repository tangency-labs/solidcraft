//! Grades one engine's output for one SSI corpus case against the exact ground truth.
//!
//! Ported from jyviko's ssi-corpus `scoring/score.py` (`score_case`, `status_names`),
//! <https://github.com/jyviko/ssi-corpus> at commit `b04c8a6`, licensed MIT OR Apache-2.0.
//! Same checks, flags and verdicts, most severe first: `CRASH`, `ERROR`, `INCOMPLETE`,
//! `WRONG TOPOLOGY`, `INACCURATE`, `PASS` (and `N/A` for default-tolerance runs below
//! the feature size floor). The runner, the truth generators and the multiprocessing
//! driver are not ported.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use super::case::SsiCase;
use super::geom::{dist, truth_distance, P3};
use super::polyset::{diameter, np_max, py_max, py_min, PolySet};
use super::raw::{RawOutput, SM_SUCCESS};
use crate::json::num;

/// The verdicts, most severe first.
pub const VERDICTS: [&str; 7] = [
    "CRASH",
    "ERROR",
    "INCOMPLETE",
    "WRONG TOPOLOGY",
    "INACCURATE",
    "PASS",
    "N/A",
];

/// `SmStatus` names from `SmMessages.h` (`#define SM_X 1234L`, first name per value).
pub fn status_names(root: &Path) -> BTreeMap<i64, String> {
    let mut names = BTreeMap::new();
    let path = root
        .join("upstream")
        .join("source")
        .join("SMLib")
        .join("inc")
        .join("SmMessages.h");
    let Ok(bytes) = std::fs::read(path) else {
        return names;
    };
    for line in String::from_utf8_lossy(&bytes).lines() {
        let mut it = line.split_whitespace();
        if it.next() != Some("#define") {
            continue;
        }
        let (Some(name), Some(val)) = (it.next(), it.next()) else {
            continue;
        };
        if !name.starts_with("SM_") || !name[3..].chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let Some(digits) = val.strip_suffix('L') else {
            continue;
        };
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if let Ok(v) = digits.parse::<i64>() {
            names.entry(v).or_insert_with(|| name.to_string());
        }
    }
    names
}

/// C `printf("%.<prec>e")`.
pub fn fmt_e(x: f64, prec: usize) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let s = format!("{x:.prec$e}");
    match s.split_once('e') {
        Some((m, e)) => {
            let ev: i32 = e.parse().unwrap_or(0);
            let sign = if ev < 0 { '-' } else { '+' };
            format!("{m}e{sign}{:02}", ev.abs())
        }
        None => s,
    }
}

/// Python `"%.0f" % x`.
fn fmt_0f(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    format!("{x:.0}")
}

/// The grade of one engine's output for one case (`score_case`'s record).
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    /// Verdict.
    pub verdict: &'static str,
    /// Every flag, in `score.py`'s order.
    pub flags: Vec<String>,
    /// Effective coverage tolerance.
    pub cov_tol: f64,
    /// CRASH detail.
    pub detail: Option<String>,
    /// Engine status.
    pub status: Option<i64>,
    /// Status name.
    pub status_name: Option<String>,
    /// Number of output curves (raw).
    pub n_curves: usize,
    /// Number of output points (degenerate curves and points).
    pub n_out_points: usize,
    /// Curve types.
    pub types: Vec<String>,
    /// Max distance of 3D output to a true surface.
    pub max_dist_3d: f64,
    /// Max distance of a surface at output UV from the other true surface.
    pub max_dist_uv: f64,
    /// Hausdorff truth -> output.
    pub hausdorff_truth_to_out: f64,
    /// Hausdorff output -> truth.
    pub hausdorff_out_to_truth: f64,
    /// Expected topology signature.
    pub expected_topology: Vec<String>,
    /// Output topology signature.
    pub output_topology: Vec<String>,
}

impl Score {
    fn crash(cov_tol: f64, detail: String) -> Self {
        Self {
            verdict: "CRASH",
            flags: Vec::new(),
            cov_tol,
            detail: Some(detail),
            status: None,
            status_name: None,
            n_curves: 0,
            n_out_points: 0,
            types: Vec::new(),
            max_dist_3d: 0.0,
            max_dist_uv: 0.0,
            hausdorff_truth_to_out: 0.0,
            hausdorff_out_to_truth: 0.0,
            expected_topology: Vec::new(),
            output_topology: Vec::new(),
        }
    }

    /// The report record (deterministic: no timings).
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "verdict": self.verdict,
            "flags": self.flags,
            "cov_tol": num(self.cov_tol),
        });
        if let Value::Object(m) = &mut v {
            if let Some(d) = &self.detail {
                m.insert("detail".into(), json!(d));
            }
            if let Some(s) = self.status {
                m.insert("status".into(), json!(s));
                m.insert("status_name".into(), json!(self.status_name));
                m.insert("n_curves".into(), json!(self.n_curves));
                m.insert("n_out_points".into(), json!(self.n_out_points));
                m.insert("types".into(), json!(self.types));
                m.insert("max_dist_3d".into(), num(self.max_dist_3d));
                m.insert("max_dist_uv".into(), num(self.max_dist_uv));
                m.insert(
                    "hausdorff_truth_to_out".into(),
                    num(self.hausdorff_truth_to_out),
                );
                m.insert(
                    "hausdorff_out_to_truth".into(),
                    num(self.hausdorff_out_to_truth),
                );
                m.insert("expected_topology".into(), json!(self.expected_topology));
                m.insert("output_topology".into(), json!(self.output_topology));
            }
        }
        v
    }
}

/// Every `stride`-th point (`P[::stride]`).
fn decimate(p: &[P3], stride: usize) -> Vec<P3> {
    p.iter().step_by(stride.max(1)).copied().collect()
}

fn finite(p: &P3) -> bool {
    p.iter().all(|x| x.is_finite())
}

/// `max(float(truth_distance(t, P).max()) for t in truths)` (Python `max`).
fn max_over_truths(case: &SsiCase, p: &[P3]) -> f64 {
    let mut it = case.truth.iter().map(|t| np_max(&truth_distance(t, p)));
    let first = it.next().unwrap_or(f64::NAN);
    it.fold(first, py_max)
}

/// `score_case(case, out, acc_tol, crash, cov_floor)`.
pub fn score_case(
    case: &SsiCase,
    out: Result<&RawOutput, &str>,
    acc_tol: Option<f64>,
    cov_floor: f64,
    names: &BTreeMap<i64, String>,
) -> Score {
    let cov_tol = case.cov_tol;
    let mut cov_eff = cov_tol;
    if let (Some(ic), Some(at)) = (case.ill_cond, acc_tol) {
        if ic != 0.0 && at != 0.0 {
            cov_eff = py_max(cov_tol, py_min(0.5, 2.0 * at * ic));
        }
    }
    cov_eff = py_max(cov_eff, cov_floor);
    let join_tol = py_max(cov_tol, cov_floor);
    let out = match out {
        Ok(o) => o,
        Err(detail) => return Score::crash(cov_eff, detail.to_string()),
    };
    let status_name = out.status_name.clone().unwrap_or_else(|| {
        names
            .get(&out.status)
            .cloned()
            .unwrap_or_else(|| out.status.to_string())
    });

    let mut out_curves: Vec<Vec<P3>> = Vec::new();
    let mut out_points: Vec<P3> = Vec::new();
    let mut types: Vec<String> = Vec::new();
    let mut nonfinite = 0usize;
    for c in &out.curves {
        let p: Vec<P3> = c.xyz.iter().filter(|p| finite(p)).copied().collect();
        nonfinite += c.xyz.len() - p.len();
        if p.is_empty() {
            continue;
        }
        if c.ty == "TOUCHING" || diameter(&p) <= cov_tol {
            let n = p.len() as f64;
            let mut m = [0.0; 3];
            for (k, mk) in m.iter_mut().enumerate() {
                *mk = p.iter().filter_map(|q| q.get(k)).sum::<f64>() / n;
            }
            out_points.push(m);
        } else {
            out_curves.push(p);
        }
        types.push(c.ty.clone());
    }
    out_points.extend(out.points.iter().copied());

    // accuracy against the exact surfaces
    let mut acc3d = 0.0f64;
    let mut accuv = 0.0f64;
    for c in &out.curves {
        let p: Vec<P3> = c.xyz.iter().filter(|p| finite(p)).copied().collect();
        if !p.is_empty() {
            acc3d = py_max(acc3d, max_over_truths(case, &p));
        }
        // S1(uv1) must lie on surface 2, etc.
        for (k, samples) in [(1usize, &c.s1_at_uv), (0, &c.s2_at_uv)] {
            let q: Vec<P3> = samples.iter().filter(|p| finite(p)).copied().collect();
            if !q.is_empty() {
                if let Some(t) = case.truth.get(k) {
                    accuv = py_max(accuv, np_max(&truth_distance(t, &q)));
                }
            }
        }
    }
    if !out_points.is_empty() {
        acc3d = py_max(acc3d, max_over_truths(case, &out_points));
    }

    let truth_polys: Vec<&[P3]> = case
        .branches
        .iter()
        .chain(&case.coincident_regions)
        .map(Vec::as_slice)
        .collect();
    let truth_pts = &case.isolated_points;
    let expected_any = !truth_polys.is_empty() || !truth_pts.is_empty();

    let mut flags: Vec<String> = Vec::new();
    let ok_status = out.status == SM_SUCCESS;
    if !ok_status {
        if !expected_any && out.curves.is_empty() && out_points.is_empty() {
            flags.push(format!(
                "status {status_name} with empty output (expected empty)"
            ));
        } else {
            flags.push(format!("ERROR status {status_name}"));
        }
    }

    // coverage truth -> output
    let curve_refs: Vec<&[P3]> = out_curves.iter().map(Vec::as_slice).collect();
    let outset = PolySet::new(&curve_refs, &out_points);
    let mut missed: Vec<String> = Vec::new();
    let mut h_truth = 0.0f64;
    let mut first_cover = true;
    let mut cover_max = |x: f64, first: &mut bool| {
        // Python max([...], default=0.0)
        if *first {
            h_truth = x;
            *first = false;
        } else {
            h_truth = py_max(h_truth, x);
        }
    };
    for (k, p) in truth_polys.iter().enumerate() {
        let d = outset.query(&decimate(p, p.len() / 1000));
        let frac = d.iter().filter(|&&x| x <= cov_eff).count() as f64 / d.len().max(1) as f64;
        let dmax = np_max(&d);
        cover_max(dmax, &mut first_cover);
        if dmax > cov_eff {
            let kind = if k >= case.branches.len() {
                "region boundary".to_string()
            } else {
                format!("branch {k}")
            };
            missed.push(format!(
                "{kind} covered {}% (max gap {})",
                fmt_0f(100.0 * frac),
                fmt_e(dmax, 2)
            ));
        }
    }
    for (k, p) in truth_pts.iter().enumerate() {
        let d = outset.query1(*p);
        cover_max(d, &mut first_cover);
        if d > cov_eff {
            missed.push(format!(
                "isolated point {k} not found (nearest output {})",
                fmt_e(d, 2)
            ));
        }
    }

    // output -> truth (spurious output)
    let tset = PolySet::new(&truth_polys, truth_pts);
    let mut spurious: Vec<String> = Vec::new();
    let mut h_out = 0.0f64;
    for (k, p) in out_curves.iter().enumerate() {
        let d = tset.query(&decimate(p, p.len() / 1000));
        let dmax = np_max(&d);
        h_out = py_max(h_out, dmax);
        if dmax > cov_eff {
            let frac = d.iter().filter(|&&x| x > cov_eff).count() as f64 / d.len().max(1) as f64;
            spurious.push(format!(
                "output curve {k} off the true intersection by up to {} ({}% of it)",
                fmt_e(dmax, 2),
                fmt_0f(100.0 * frac)
            ));
        }
    }
    for (k, p) in out_points.iter().enumerate() {
        let d = tset.query1(*p);
        h_out = py_max(h_out, d);
        if d > cov_eff {
            spurious.push(format!(
                "output point {k} off the true intersection by {}",
                fmt_e(d, 2)
            ));
        }
    }

    // topology signature
    let truth_owned: Vec<Vec<P3>> = truth_polys.iter().map(|p| p.to_vec()).collect();
    let sig_exp = super::polyset::signature(&truth_owned, truth_pts, join_tol);
    let sig_out = super::polyset::signature(&out_curves, &out_points, join_tol);
    let mut topo: Vec<String> = Vec::new();
    if nonfinite > 0 {
        topo.push(format!("{nonfinite} non-finite output sample(s)"));
    }
    // duplicated / overlapping output curves
    let osets: Vec<PolySet> = out_curves
        .iter()
        .map(|c| PolySet::new(&[c.as_slice()], &[]))
        .collect();
    for (i, ci) in out_curves.iter().enumerate() {
        for (j, cj) in out_curves.iter().enumerate().skip(i + 1) {
            let (Some(si), Some(sj)) = (osets.get(i), osets.get(j)) else {
                continue;
            };
            let di = sj.query(&decimate(ci, ci.len() / 500));
            let dj = si.query(&decimate(cj, cj.len() / 500));
            let fi = di.iter().filter(|&&x| x <= join_tol).count() as f64 / di.len().max(1) as f64;
            let fj = dj.iter().filter(|&&x| x <= join_tol).count() as f64 / dj.len().max(1) as f64;
            let frac = py_max(fi, fj);
            if frac > 0.2 {
                topo.push(format!(
                    "output curves {i} and {j} overlap ({}% of one lies on the other)",
                    fmt_0f(100.0 * frac)
                ));
            }
        }
    }
    if sig_exp != sig_out {
        topo.push(format!(
            "topology {}, expected {}",
            py_list(&sig_out),
            py_list(&sig_exp)
        ));
    }
    for (k, s) in case.singular_points.iter().enumerate() {
        let ends = out_curves
            .iter()
            .filter_map(|c| c.first())
            .chain(out_curves.iter().filter_map(|c| c.last()))
            .chain(out_points.iter());
        let mut d = f64::INFINITY;
        let mut first = true;
        for e in ends {
            let x = dist(*e, *s);
            d = if first { x } else { py_min(d, x) };
            first = false;
        }
        if d > cov_eff {
            topo.push(format!(
                "singular point {k} not identified (no curve end/point within {}; nearest {})",
                fmt_e(cov_eff, 1),
                fmt_e(d, 2)
            ));
        }
    }
    let mut inacc: Vec<String> = Vec::new();
    if let Some(at) = acc_tol {
        if acc3d > at {
            inacc.push(format!(
                "3D output up to {} from a surface (acc_tol {})",
                fmt_e(acc3d, 2),
                fmt_e(at, 0)
            ));
        }
        if accuv > at {
            inacc.push(format!(
                "surface at output UV up to {} from the other surface",
                fmt_e(accuv, 2)
            ));
        }
    }

    let any_out = !out.curves.is_empty() || !out_points.is_empty();
    let mut verdict = if !ok_status && (expected_any || any_out) {
        "ERROR"
    } else if !missed.is_empty() {
        "INCOMPLETE"
    } else if !spurious.is_empty() || !topo.is_empty() {
        "WRONG TOPOLOGY"
    } else if !inacc.is_empty() {
        "INACCURATE"
    } else {
        "PASS"
    };
    flags.extend(missed);
    flags.extend(spurious);
    flags.extend(topo);
    flags.extend(inacc);
    if cov_floor != 0.0 {
        if let Some(fs) = case.feature_size {
            if fs < cov_floor {
                flags.insert(
                    0,
                    format!(
                        "feature size {} below the {} judging floor of default-tolerance runs",
                        fmt_e(fs, 1),
                        fmt_e(cov_floor, 0)
                    ),
                );
                verdict = "N/A";
            }
        }
    }
    Score {
        verdict,
        flags,
        cov_tol: cov_eff,
        detail: None,
        status: Some(out.status),
        status_name: Some(status_name),
        n_curves: out.curves.len(),
        n_out_points: out_points.len(),
        types,
        max_dist_3d: acc3d,
        max_dist_uv: accuv,
        hausdorff_truth_to_out: if first_cover { 0.0 } else { h_truth },
        hausdorff_out_to_truth: h_out,
        expected_topology: sig_exp,
        output_topology: sig_out,
    }
}

/// Python `str(list_of_str)`.
fn py_list(v: &[String]) -> String {
    let items: Vec<String> = v.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", items.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssi::raw::RawCurve;

    #[test]
    fn printf_e() {
        assert_eq!(fmt_e(1.234e-5, 2), "1.23e-05");
        assert_eq!(fmt_e(1e-5, 0), "1e-05");
        assert_eq!(fmt_e(123.0, 1), "1.2e+02");
        assert_eq!(fmt_e(f64::INFINITY, 2), "inf");
    }

    fn circle_case() -> SsiCase {
        let pts: Vec<P3> = (0..4001)
            .map(|i| {
                let t = std::f64::consts::TAU * f64::from(i) / 4000.0;
                [t.cos(), t.sin(), 0.0]
            })
            .collect();
        let v = json!({
            "id": "t", "class": "1 transversal", "cov_tol": 1e-3,
            "surfaces": [
                {"type": "plane", "origin": [0,0,0], "xaxis": [1,0,0], "yaxis": [0,1,0], "u": [-2,2], "v": [-2,2]},
                {"type": "sphere", "center": [0,0,0], "xaxis": [1,0,0], "yaxis": [0,1,0], "radius": 1, "lon": [0,360], "lat": [-90,90]}
            ],
            "truth": [
                {"kind": "plane", "point": [0,0,0], "normal": [0,0,1]},
                {"kind": "sphere", "center": [0,0,0], "radius": 1}
            ],
            "expected": {"branches": [{"closed": true, "kind": "crossing", "points": pts}],
                         "isolated_points": [], "singular_points": [], "coincident_regions": []}
        });
        SsiCase::parse(&v).unwrap()
    }

    #[test]
    fn verdicts() {
        let case = circle_case();
        let names = BTreeMap::new();
        let full: Vec<P3> = case.branches[0].iter().step_by(4).copied().collect();
        let curve = |xyz: Vec<P3>| RawCurve {
            ty: "CROSSING".into(),
            xyz,
            ..RawCurve::default()
        };
        let mut out = RawOutput {
            status: SM_SUCCESS,
            curves: vec![curve(full.clone())],
            ..RawOutput::default()
        };
        let s = score_case(&case, Ok(&out), Some(1e-5), 0.0, &names);
        assert_eq!(s.verdict, "PASS", "{:?}", s.flags);
        // half the circle: incomplete
        out.curves = vec![curve(full[..500].to_vec())];
        assert_eq!(
            score_case(&case, Ok(&out), Some(1e-5), 0.0, &names).verdict,
            "INCOMPLETE"
        );
        // the circle twice: duplicated curves
        out.curves = vec![curve(full.clone()), curve(full.clone())];
        assert_eq!(
            score_case(&case, Ok(&out), Some(1e-5), 0.0, &names).verdict,
            "WRONG TOPOLOGY"
        );
        // inaccurate: scaled by 1 + 1e-4
        let off: Vec<P3> = full
            .iter()
            .map(|p| [p[0] * 1.0001, p[1] * 1.0001, 0.0])
            .collect();
        out.curves = vec![curve(off)];
        assert_eq!(
            score_case(&case, Ok(&out), Some(1e-5), 0.0, &names).verdict,
            "INACCURATE"
        );
        out.status = 1001;
        assert_eq!(
            score_case(&case, Ok(&out), Some(1e-5), 0.0, &names).verdict,
            "ERROR"
        );
        assert_eq!(
            score_case(&case, Err("killed by signal 11"), Some(1e-5), 0.0, &names).verdict,
            "CRASH"
        );
    }
}
