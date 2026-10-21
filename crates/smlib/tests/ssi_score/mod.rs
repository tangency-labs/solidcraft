//! The SSI corpus scorer (`tests/ssi-corpus`), ported from the corpus's `scoring/score.py`
//! (jyviko/ssi-corpus b04c8a6, MIT OR Apache-2.0) as test-support code.
//!
//! [`score_case`] grades one engine result against a case's exact truth and returns the
//! verdict, most severe first: CRASH, ERROR, INCOMPLETE, WRONG TOPOLOGY, INACCURATE, PASS, with
//! every flag and number score.py reports. The flag texts are score.py's, formatted the same.
//!
//! Fill an [`EngineResult`] from sampled curves (per sample: `xyz`, `uv1`, `uv2`, both surfaces
//! evaluated at the UV samples, and the curve type), isolated points and the status, or read
//! the corpus runner's JSON with [`EngineResult::from_runner_json`].
#![allow(dead_code)]

pub mod geom;
pub mod polyset;

use std::path::{Path, PathBuf};

use serde_json::Value;

use geom::{Truth, V3, np_max, py_max, v3, v3_list};
use polyset::{PolySet, diameter, signature, stride};

/// SMLib's `SM_SUCCESS`: the only status score.py treats as success.
pub const SM_SUCCESS: i64 = 1000;

/// The corpus directory: `tests/ssi-corpus/cases`.
pub fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/ssi-corpus/cases")
}

/// Every case id, sorted.
pub fn case_ids() -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(cases_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json.gz")).map(str::to_string)).collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// Reads `<id>.json.gz` from the corpus.
pub fn load_case_json(id: &str) -> Result<Value, String> {
    use std::io::Read;
    let path = cases_dir().join(format!("{id}.json.gz"));
    let f = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut s = String::new();
    flate2::read::GzDecoder::new(f).read_to_string(&mut s).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load_case(id: &str) -> Result<Case, String> {
    Case::from_json(&load_case_json(id)?)
}

/// What a case expects: the true intersection, sampled.
#[derive(Clone, Debug, Default)]
pub struct Expected {
    pub branches: Vec<Vec<V3>>,
    pub isolated_points: Vec<V3>,
    pub singular_points: Vec<V3>,
    /// Boundary polylines of coincident regions.
    pub coincident_regions: Vec<Vec<V3>>,
}

/// One corpus case (the fields the scorer reads, plus the raw JSON).
#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    pub class: String,
    /// The two surface specs as the corpus stores them (`type`: plane, cylinder, cone, sphere,
    /// torus, nurbs); [`geom::Nurbs::from_spec`] evaluates the NURBS ones.
    pub surfaces: Vec<Value>,
    pub truth: Vec<Truth>,
    pub expected: Expected,
    pub cov_tol: f64,
    pub ill_cond: Option<f64>,
    pub feature_size: Option<f64>,
    pub json: Value,
}

fn polylines(v: Option<&Value>, key: &str) -> Result<Vec<Vec<V3>>, String> {
    match v {
        None => Ok(Vec::new()),
        Some(Value::Array(a)) => a.iter().map(|b| v3_list(b.get(key))).collect(),
        Some(other) => Err(format!("not a list: {other}")),
    }
}

impl Case {
    pub fn from_json(c: &Value) -> Result<Case, String> {
        let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let truth = c.get("truth").and_then(Value::as_array).ok_or("truth: not a list")?;
        let truth: Vec<Truth> = truth.iter().map(Truth::from_json).collect::<Result<_, _>>()?;
        if truth.len() != 2 {
            return Err("need two truths".into());
        }
        let exp = c.get("expected").ok_or("missing expected")?;
        let expected = Expected {
            branches: polylines(exp.get("branches"), "points")?,
            isolated_points: v3_list(exp.get("isolated_points"))?,
            singular_points: v3_list(exp.get("singular_points"))?,
            coincident_regions: polylines(exp.get("coincident_regions"), "boundary")?,
        };
        let opt = |k: &str| c.get(k).and_then(Value::as_f64);
        Ok(Case {
            id: s("id"),
            class: s("class"),
            surfaces: c.get("surfaces").and_then(Value::as_array).cloned().unwrap_or_default(),
            truth,
            expected,
            cov_tol: opt("cov_tol").unwrap_or(1e-3),
            ill_cond: opt("ill_cond"),
            feature_size: opt("feature_size"),
            json: c.clone(),
        })
    }
}

/// One output curve, sampled. Missing or non-finite coordinates are NaN.
#[derive(Clone, Debug, Default)]
pub struct EngineCurve {
    /// The curve type as the corpus runner names it: `TOUCHING`, `CROSSING`, `TANGENT`,
    /// `COINCIDENT`, `NEAR_TANGENT`, `REGION_BOUNDARY`, `UNKNOWN` (`n/a` without types). Only
    /// `TOUCHING` changes the scoring: such a curve counts as a point.
    pub curve_type: String,
    /// 3D samples.
    pub xyz: Vec<V3>,
    /// UV samples on surface 1 and 2 (not scored themselves).
    pub uv1: Vec<[f64; 2]>,
    pub uv2: Vec<[f64; 2]>,
    /// Surface 1 evaluated at `uv1` (must lie on surface 2), surface 2 at `uv2` (on surface 1).
    pub s1_at_uv: Vec<V3>,
    pub s2_at_uv: Vec<V3>,
}

/// An engine's answer for one case.
#[derive(Clone, Debug, Default)]
pub struct EngineResult {
    /// The engine status; [`SM_SUCCESS`] is success.
    pub status: i64,
    /// The status' name for the flags (score.py: the runner's `status_name`, else the SMLib
    /// name, else the number).
    pub status_name: Option<String>,
    pub time_ms: f64,
    pub curves: Vec<EngineCurve>,
    /// Isolated points.
    pub points: Vec<V3>,
}

fn uv_list(v: Option<&Value>) -> Vec<[f64; 2]> {
    v.and_then(Value::as_array).map(|a| a.iter().map(|p| [geom::num(&p[0]), geom::num(&p[1])]).collect()).unwrap_or_default()
}

impl EngineResult {
    /// Reads the corpus runner's output JSON (`runner/ssi_runner.cpp`).
    pub fn from_runner_json(o: &Value) -> Result<EngineResult, String> {
        let curves = match o.get("curves") {
            Some(Value::Array(a)) => a
                .iter()
                .map(|c| {
                    Ok(EngineCurve {
                        curve_type: c.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
                        xyz: v3_list(c.get("xyz"))?,
                        uv1: uv_list(c.get("uv1")),
                        uv2: uv_list(c.get("uv2")),
                        s1_at_uv: v3_list(c.get("s1_at_uv"))?,
                        s2_at_uv: v3_list(c.get("s2_at_uv"))?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
            _ => return Err("curves: not a list".into()),
        };
        Ok(EngineResult {
            status: o.get("status").and_then(Value::as_i64).ok_or("status: not an integer")?,
            status_name: o.get("status_name").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
            time_ms: o.get("time_ms").map(geom::num).unwrap_or(f64::NAN),
            curves,
            points: v3_list(o.get("points"))?,
        })
    }
}

/// The engine ran and answered, or it did not (died, timed out, unreadable output).
#[derive(Clone, Debug)]
pub enum Outcome {
    Ran(EngineResult),
    Crashed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Crash,
    Error,
    Incomplete,
    WrongTopology,
    Inaccurate,
    Pass,
    /// Default-tolerance runs only: the case's feature size is under the judging floor.
    NotApplicable,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Crash => "CRASH",
            Verdict::Error => "ERROR",
            Verdict::Incomplete => "INCOMPLETE",
            Verdict::WrongTopology => "WRONG TOPOLOGY",
            Verdict::Inaccurate => "INACCURATE",
            Verdict::Pass => "PASS",
            Verdict::NotApplicable => "N/A",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How well one expected branch / region boundary, or one isolated point, is covered.
#[derive(Clone, Debug, PartialEq)]
pub enum Coverage {
    Polyline { max: f64, covered_frac: f64 },
    Point { point: usize, dist: f64 },
}

/// The tolerances of a run.
#[derive(Clone, Copy, Debug)]
pub struct Tolerances {
    /// `10 * approx_tol`; `None` for a run at the engine's default tolerance.
    pub acc_tol: Option<f64>,
    /// Coverage and join floor: 5e-3 for default-tolerance runs, else 0.
    pub cov_floor: f64,
}

impl Tolerances {
    /// A run at approximation tolerance `tol` (score.py: `acc_tol = 10 * tol`, no floor).
    pub fn at(tol: f64) -> Tolerances {
        Tolerances { acc_tol: Some(10.0 * tol), cov_floor: 0.0 }
    }

    /// A run at the engine's default tolerance.
    pub fn default_tol() -> Tolerances {
        Tolerances { acc_tol: None, cov_floor: 5e-3 }
    }
}

/// The score of one case (score.py's result record).
#[derive(Clone, Debug)]
pub struct Score {
    pub id: String,
    pub class: String,
    pub verdict: Verdict,
    pub flags: Vec<String>,
    /// The effective coverage tolerance.
    pub cov_tol: f64,
    /// The crash description (CRASH only).
    pub detail: Option<String>,
    pub status: Option<i64>,
    pub status_name: Option<String>,
    pub time_ms: Option<f64>,
    pub n_curves: usize,
    pub n_out_points: usize,
    pub types: Vec<String>,
    pub max_dist_3d: f64,
    pub max_dist_uv: f64,
    pub coverage: Vec<Coverage>,
    pub hausdorff_truth_to_out: f64,
    pub hausdorff_out_to_truth: f64,
    pub expected_topology: Vec<String>,
    pub output_topology: Vec<String>,
}

/// Python's `"%.<prec>e" % x`.
pub fn fmt_e(x: f64, prec: usize) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let s = format!("{x:.prec$e}");
    let Some((m, e)) = s.split_once('e') else { return s };
    let (sign, digits) = match e.strip_prefix('-') {
        Some(d) => ('-', d),
        None => ('+', e),
    };
    format!("{m}e{sign}{digits:0>2}")
}

/// Python's `"%.0f" % x`.
fn fmt_f0(x: f64) -> String {
    format!("{x:.0}")
}

/// Python's `str(list_of_str)`.
fn py_list(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", "))
}

fn finite(p: &[V3]) -> Vec<V3> {
    p.iter().copied().filter(|q| q.iter().all(|c| c.is_finite())).collect()
}

/// Scores one engine outcome against a case (score.py's `score_case`).
pub fn score_case(case: &Case, outcome: &Outcome, tol: Tolerances) -> Score {
    let exp = &case.expected;
    let cov_tol = case.cov_tol;
    let mut cov_eff = cov_tol;
    if let (Some(ill), Some(acc)) = (case.ill_cond, tol.acc_tol)
        && ill != 0.0
        && acc != 0.0
    {
        cov_eff = py_max(cov_tol, geom::py_min(0.5, 2.0 * acc * ill));
    }
    cov_eff = py_max(cov_eff, tol.cov_floor);
    let join_tol = py_max(cov_tol, tol.cov_floor);
    let mut r = Score {
        id: case.id.clone(),
        class: case.class.clone(),
        verdict: Verdict::Crash,
        flags: Vec::new(),
        cov_tol: cov_eff,
        detail: None,
        status: None,
        status_name: None,
        time_ms: None,
        n_curves: 0,
        n_out_points: 0,
        types: Vec::new(),
        max_dist_3d: 0.0,
        max_dist_uv: 0.0,
        coverage: Vec::new(),
        hausdorff_truth_to_out: 0.0,
        hausdorff_out_to_truth: 0.0,
        expected_topology: Vec::new(),
        output_topology: Vec::new(),
    };
    let out = match outcome {
        Outcome::Crashed(why) => {
            r.detail = Some(why.clone());
            return r;
        }
        Outcome::Ran(out) => out,
    };
    let status_name = out.status_name.clone().unwrap_or_else(|| out.status.to_string());
    r.status = Some(out.status);
    r.status_name = Some(status_name.clone());
    r.time_ms = Some(out.time_ms);

    let mut out_curves: Vec<Vec<V3>> = Vec::new();
    let mut out_points: Vec<V3> = Vec::new();
    let mut nonfinite = 0;
    for c in &out.curves {
        let p = finite(&c.xyz);
        nonfinite += c.xyz.len() - p.len();
        if p.is_empty() {
            continue;
        }
        if c.curve_type == "TOUCHING" || diameter(&p) <= cov_tol {
            let n = p.len() as f64;
            let s = p.iter().fold([0.0; 3], |a, q| [a[0] + q[0], a[1] + q[1], a[2] + q[2]]);
            out_points.push([s[0] / n, s[1] / n, s[2] / n]);
        } else {
            out_curves.push(p);
        }
        r.types.push(c.curve_type.clone());
    }
    out_points.extend(out.points.iter().copied());
    r.n_curves = out.curves.len();
    r.n_out_points = out_points.len();

    // accuracy against the exact surfaces
    let worst = |pts: &[V3]| -> f64 {
        let mut m = np_max(&case.truth[0].distance(pts));
        for t in &case.truth[1..] {
            m = py_max(m, np_max(&t.distance(pts)));
        }
        m
    };
    let (mut acc3d, mut accuv) = (0.0_f64, 0.0_f64);
    for c in &out.curves {
        let p = finite(&c.xyz);
        if !p.is_empty() {
            acc3d = py_max(acc3d, worst(&p));
        }
        // S1(uv1) must lie on surface 2, S2(uv2) on surface 1.
        for (k, q) in [(1, &c.s1_at_uv), (0, &c.s2_at_uv)] {
            let q = finite(q);
            if !q.is_empty() {
                accuv = py_max(accuv, np_max(&case.truth[k].distance(&q)));
            }
        }
    }
    if !out_points.is_empty() {
        acc3d = py_max(acc3d, worst(&out_points));
    }
    r.max_dist_3d = acc3d;
    r.max_dist_uv = accuv;

    let truth_polys: Vec<&[V3]> = exp.branches.iter().map(Vec::as_slice).chain(exp.coincident_regions.iter().map(Vec::as_slice)).collect();
    let truth_pts = &exp.isolated_points;
    let expected_any = !truth_polys.is_empty() || !truth_pts.is_empty();

    let ok_status = out.status == SM_SUCCESS;
    if !ok_status {
        if !expected_any && out.curves.is_empty() && out_points.is_empty() {
            r.flags.push(format!("status {status_name} with empty output (expected empty)"));
        } else {
            r.flags.push(format!("ERROR status {status_name}"));
        }
    }

    // coverage truth -> output
    let curve_refs: Vec<&[V3]> = out_curves.iter().map(Vec::as_slice).collect();
    let outset = PolySet::new(&curve_refs, &out_points);
    let mut missed = Vec::new();
    for (k, p) in truth_polys.iter().enumerate() {
        let d = outset.query(&stride(p, p.len() / 1000));
        let frac = d.iter().filter(|&&x| x <= cov_eff).count() as f64 / d.len() as f64;
        let dmax = np_max(&d);
        r.coverage.push(Coverage::Polyline { max: dmax, covered_frac: frac });
        if dmax > cov_eff {
            let kind = if k >= exp.branches.len() { "region boundary".to_string() } else { format!("branch {k}") };
            missed.push(format!("{kind} covered {}% (max gap {})", fmt_f0(100.0 * frac), fmt_e(dmax, 2)));
        }
    }
    for (k, &p) in truth_pts.iter().enumerate() {
        let d = outset.query1(p);
        r.coverage.push(Coverage::Point { point: k, dist: d });
        if d > cov_eff {
            missed.push(format!("isolated point {k} not found (nearest output {})", fmt_e(d, 2)));
        }
    }
    let mut h = 0.0;
    for (i, c) in r.coverage.iter().enumerate() {
        let x = match c {
            Coverage::Polyline { max, .. } => *max,
            Coverage::Point { dist, .. } => *dist,
        };
        h = if i == 0 { x } else { py_max(h, x) };
    }
    r.hausdorff_truth_to_out = h;

    // output -> truth (spurious output)
    let tset = PolySet::new(&truth_polys, truth_pts);
    let mut spurious = Vec::new();
    let mut h_out = 0.0;
    for (k, p) in out_curves.iter().enumerate() {
        let d = tset.query(&stride(p, p.len() / 1000));
        let dmax = np_max(&d);
        h_out = py_max(h_out, dmax);
        if dmax > cov_eff {
            let off = d.iter().filter(|&&x| x > cov_eff).count() as f64 / d.len() as f64;
            spurious.push(format!("output curve {k} off the true intersection by up to {} ({}% of it)", fmt_e(dmax, 2), fmt_f0(100.0 * off)));
        }
    }
    for (k, &p) in out_points.iter().enumerate() {
        let d = tset.query1(p);
        h_out = py_max(h_out, d);
        if d > cov_eff {
            spurious.push(format!("output point {k} off the true intersection by {}", fmt_e(d, 2)));
        }
    }
    r.hausdorff_out_to_truth = h_out;

    // topology signature
    let truth_owned: Vec<Vec<V3>> = truth_polys.iter().map(|p| p.to_vec()).collect();
    let sig_exp = signature(&truth_owned, truth_pts, join_tol);
    let sig_out = signature(&out_curves, &out_points, join_tol);
    let mut topo = Vec::new();
    if nonfinite > 0 {
        topo.push(format!("{nonfinite} non-finite output sample(s)"));
    }
    // duplicated / overlapping output curves
    let osets: Vec<PolySet> = out_curves.iter().map(|c| PolySet::new(&[c], &[])).collect();
    for i in 0..out_curves.len() {
        for j in i + 1..out_curves.len() {
            let (ci, cj) = (&out_curves[i], &out_curves[j]);
            let di = osets[j].query(&stride(ci, ci.len() / 500));
            let dj = osets[i].query(&stride(cj, cj.len() / 500));
            let fi = di.iter().filter(|&&x| x <= join_tol).count() as f64 / di.len() as f64;
            let fj = dj.iter().filter(|&&x| x <= join_tol).count() as f64 / dj.len() as f64;
            let frac = py_max(fi, fj);
            if frac > 0.2 {
                topo.push(format!("output curves {i} and {j} overlap ({}% of one lies on the other)", fmt_f0(100.0 * frac)));
            }
        }
    }
    if sig_exp != sig_out {
        topo.push(format!("topology {}, expected {}", py_list(&sig_out), py_list(&sig_exp)));
    }
    for (k, &s) in exp.singular_points.iter().enumerate() {
        let ends = out_curves.iter().filter_map(|c| c.first()).chain(out_curves.iter().filter_map(|c| c.last())).chain(out_points.iter());
        let mut d = f64::INFINITY;
        for (n, &e) in ends.enumerate() {
            let x = geom::norm(geom::sub(e, s));
            d = if n == 0 { x } else { geom::py_min(d, x) };
        }
        if d > cov_eff {
            topo.push(format!("singular point {k} not identified (no curve end/point within {}; nearest {})", fmt_e(cov_eff, 1), fmt_e(d, 2)));
        }
    }
    let mut inacc = Vec::new();
    if let Some(acc) = tol.acc_tol {
        if acc3d > acc {
            inacc.push(format!("3D output up to {} from a surface (acc_tol {})", fmt_e(acc3d, 2), fmt_e(acc, 0)));
        }
        if accuv > acc {
            inacc.push(format!("surface at output UV up to {} from the other surface", fmt_e(accuv, 2)));
        }
    }

    let has_output = !out.curves.is_empty() || !out_points.is_empty();
    let verdict = if !ok_status && (expected_any || has_output) {
        Verdict::Error
    } else if !missed.is_empty() {
        Verdict::Incomplete
    } else if !spurious.is_empty() || !topo.is_empty() {
        Verdict::WrongTopology
    } else if !inacc.is_empty() {
        Verdict::Inaccurate
    } else {
        Verdict::Pass
    };
    r.flags.extend(missed);
    r.flags.extend(spurious);
    r.flags.extend(topo);
    r.flags.extend(inacc);
    r.expected_topology = sig_exp;
    r.output_topology = sig_out;
    r.verdict = verdict;
    if tol.cov_floor != 0.0
        && let Some(fs) = case.feature_size
        && fs < tol.cov_floor
    {
        r.flags.insert(0, format!("feature size {} below the {} judging floor of default-tolerance runs", fmt_e(fs, 1), fmt_e(tol.cov_floor, 0)));
        r.verdict = Verdict::NotApplicable;
    }
    r
}

/// Parses `#define SM_NAME 1234L` lines of SMLib's `SmMessages.h` (first name per number), for
/// status names in flags as score.py prints them.
pub fn status_names(sm_messages_h: &str) -> std::collections::HashMap<i64, String> {
    let mut names = std::collections::HashMap::new();
    for line in sm_messages_h.lines() {
        let Some(rest) = line.strip_prefix("#define") else { continue };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let mut it = rest.split_whitespace();
        let (Some(name), Some(val)) = (it.next(), it.next()) else { continue };
        let Some(digits) = val.strip_suffix('L') else { continue };
        if !name.starts_with("SM_") || !name[3..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.len() == 3 {
            continue;
        }
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if let Ok(n) = digits.parse::<i64>() {
            names.entry(n).or_insert_with(|| name.to_string());
        }
    }
    names
}

/// `v3` re-exported for callers building points from JSON.
pub fn point(v: &Value) -> Result<V3, String> {
    v3(v)
}
