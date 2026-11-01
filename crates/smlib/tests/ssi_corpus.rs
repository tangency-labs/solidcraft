//! The SSI corpus (`tests/ssi-corpus`) and its scorer (`ssi_score`, a port of the corpus's
//! `scoring/score.py`).
//!
//! The parity check against the Python scorer is ignored by default; it needs the corpus
//! runner's raw outputs and score.py's scores for the same run:
//!
//! ```text
//! SSI_PARITY_RAW=<corpus>/results/raw/gsi_1e-6 \
//! SSI_PARITY_SCORES=<corpus>/results/score_gsi_1e-6.json \
//! SSI_PARITY_TOL=1e-6 SSI_SM_MESSAGES=<smlib>/source/SMLib/inc/SmMessages.h \
//!   cargo test -p solidcraft-smlib --test ssi_corpus -- --ignored --nocapture
//! ```
//!
//! With the `native` feature, [`smlib_global_intersect_at_1e_6`] runs every case through SMLib's
//! GlobalSurfaceIntersect (via `solidcraft_smlib::ssi`, as the corpus runner calls it) and checks
//! exactly which cases pass.

mod ssi_score;

use serde_json::Value;
use ssi_score::geom::{Nurbs, V3, linspace, np_max};
use ssi_score::{Case, Coverage, EngineCurve, EngineResult, Outcome, Score, Tolerances, Verdict, case_ids, fmt_e, load_case, score_case};

fn all_cases() -> Vec<Case> {
    let ids = case_ids();
    assert_eq!(ids.len(), 119, "corpus case count");
    ids.iter().map(|id| load_case(id).unwrap_or_else(|e| panic!("{id}: {e}"))).collect()
}

/// `f` over `items` on a few threads, results in order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<(usize, R)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..n)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        mine.push((i, f(item)));
                    }
                    mine
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap()).collect()
    });
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

/// The expected intersection itself, as an engine answer.
fn truth_as_output(case: &Case) -> EngineResult {
    let curve = |p: &Vec<V3>| EngineCurve { curve_type: "CROSSING".into(), xyz: p.clone(), ..EngineCurve::default() };
    EngineResult {
        status: ssi_score::SM_SUCCESS,
        status_name: None,
        time_ms: 0.0,
        curves: case.expected.branches.iter().chain(&case.expected.coincident_regions).map(curve).collect(),
        points: case.expected.isolated_points.clone(),
    }
}

#[test]
fn every_case_parses_and_cites_its_source() {
    let cases = all_cases();
    let y23: Vec<&Case> = cases.iter().filter(|c| c.id.starts_with("y23_")).collect();
    assert_eq!(y23.len(), 25);
    for c in &y23 {
        let src = c.json.get("source").and_then(Value::as_str).unwrap_or_default();
        assert!(src.contains("Yang, Jia & Yan 2023"), "{}: source {src:?}", c.id);
    }
    for c in &cases {
        assert_eq!(c.surfaces.len(), 2, "{}", c.id);
        assert!(c.cov_tol > 0.0, "{}", c.id);
    }
}

/// Every NURBS surface of the corpus lies on its exact truth (the corpus's own self-check): this
/// checks the NURBS evaluation and the distance functions against each other.
#[test]
fn nurbs_surfaces_lie_on_their_truths() {
    let mut checked = 0;
    for case in all_cases() {
        for (k, spec) in case.surfaces.iter().enumerate() {
            if spec.get("type").and_then(Value::as_str) != Some("nurbs") {
                continue;
            }
            let s = Nurbs::from_spec(spec).unwrap_or_else(|e| panic!("{}: {e}", case.id));
            let d = s.domain();
            let pts: Vec<V3> = linspace(d[0], d[1], 13)
                .into_iter()
                .flat_map(|u| linspace(d[2], d[3], 13).into_iter().map(move |v| (u, v)))
                .map(|(u, v)| s.eval(u, v))
                .collect();
            let err = np_max(&case.truth[k].distance(&pts));
            assert!(err < 1e-9, "{} surface {k}: {err:e} off its truth", case.id);
            checked += 1;
        }
    }
    assert!(checked >= 99, "{checked} NURBS surfaces checked");
}

/// Handing the scorer the expected intersection itself passes every case, except that the
/// expected branches run through singular points where an answer must split its curves.
#[test]
fn the_truth_itself_passes() {
    let mut fails = Vec::new();
    let mut unsplit = 0;
    let cases = all_cases();
    let scores = par_map(&cases, |case| score_case(case, &Outcome::Ran(truth_as_output(case)), Tolerances::at(1e-6)));
    for (case, s) in cases.iter().zip(scores) {
        let only_unsplit = !case.expected.singular_points.is_empty()
            && !s.flags.is_empty()
            && s.flags.iter().all(|f| f.starts_with("singular point ") && f.contains(" not identified "));
        if s.verdict == Verdict::WrongTopology && only_unsplit {
            unsplit += 1;
        } else if s.verdict != Verdict::Pass {
            fails.push(format!("{} {}: {}", case.id, s.verdict, s.flags.join("; ")));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    assert_eq!(unsplit, 10, "cases whose expected branches run through a singular point");
}

#[test]
fn verdicts_follow_severity() {
    let case = load_case("c1_cone_plane_ellipse").unwrap();
    let tol = Tolerances::at(1e-6);
    let truth = truth_as_output(&case);

    let crash = score_case(&case, &Outcome::Crashed("killed by signal 11".into()), tol);
    assert_eq!(crash.verdict, Verdict::Crash);
    assert_eq!(crash.detail.as_deref(), Some("killed by signal 11"));

    let mut failed = truth.clone();
    failed.status = 1001;
    let s = score_case(&case, &Outcome::Ran(failed), tol);
    assert_eq!(s.verdict, Verdict::Error);
    assert_eq!(s.flags[0], "ERROR status 1001");

    let empty = EngineResult { curves: Vec::new(), ..truth.clone() };
    let s = score_case(&case, &Outcome::Ran(empty), tol);
    assert_eq!(s.verdict, Verdict::Incomplete);
    assert!(s.flags[0].starts_with("branch 0 covered 0% (max gap inf"), "{:?}", s.flags);

    // half of the ellipse: covered in part, two free ends
    let mut half = truth.clone();
    let n = half.curves[0].xyz.len();
    half.curves[0].xyz.truncate(n / 2);
    assert_eq!(score_case(&case, &Outcome::Ran(half), tol).verdict, Verdict::Incomplete);

    // the ellipse twice: duplicated output
    let mut twice = truth.clone();
    twice.curves.push(twice.curves[0].clone());
    let s = score_case(&case, &Outcome::Ran(twice), tol);
    assert_eq!(s.verdict, Verdict::WrongTopology);
    assert!(s.flags.iter().any(|f| f == "output curves 0 and 1 overlap (100% of one lies on the other)"), "{:?}", s.flags);

    // a stray point far away
    let mut stray = truth.clone();
    stray.points.push([50.0, 50.0, 50.0]);
    assert_eq!(score_case(&case, &Outcome::Ran(stray), tol).verdict, Verdict::WrongTopology);

    // the ellipse lifted by 1e-4: still covered (cov_tol 1e-3) but past acc_tol = 1e-5
    let mut lifted = truth.clone();
    for p in &mut lifted.curves[0].xyz {
        p[2] += 1e-4;
    }
    let s = score_case(&case, &Outcome::Ran(lifted), tol);
    assert_eq!(s.verdict, Verdict::Inaccurate, "{:?}", s.flags);
    assert!(s.max_dist_3d > 5e-5 && s.max_dist_3d < 1e-4 + 1e-12, "{}", s.max_dist_3d);

    // a TOUCHING curve counts as a point
    let mut touching = truth.clone();
    touching.curves[0].curve_type = "TOUCHING".into();
    let s = score_case(&case, &Outcome::Ran(touching), tol);
    assert_eq!(s.n_out_points, 1);
    assert_eq!(s.verdict, Verdict::Incomplete);

    assert_eq!(score_case(&case, &Outcome::Ran(truth), tol).verdict, Verdict::Pass);
}

#[test]
fn python_number_formatting() {
    assert_eq!(fmt_e(1.2345e-5, 2), "1.23e-05");
    assert_eq!(fmt_e(1e-5, 0), "1e-05");
    assert_eq!(fmt_e(123.0, 1), "1.2e+02");
    assert_eq!(fmt_e(f64::INFINITY, 2), "inf");
    assert_eq!(fmt_e(3.0e-300, 2), "3.00e-300");
}

// ------------------------------------------------------------------------------------------
// parity with the Python scorer

/// Python's `json.dump` writes `Infinity`, `-Infinity` and `NaN` bare; quote them (outside
/// strings) so serde_json reads them, and read numbers through [`pnum`].
fn python_json(s: &str) -> Value {
    let mut out = String::with_capacity(s.len());
    let (mut in_str, mut esc) = (false, false);
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if !in_str && let Some(tok) = ["-Infinity", "Infinity", "NaN"].into_iter().find(|t| rest.starts_with(t)) {
            out.push_str(&format!("\"{tok}\""));
            rest = &rest[tok.len()..];
            continue;
        }
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    serde_json::from_str(&out).expect("score JSON")
}

fn pnum(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::String(s)) => s.parse().unwrap_or(f64::NAN),
        Some(v) => v.as_f64().unwrap_or(f64::NAN),
        None => f64::NAN,
    }
}

fn close(a: f64, b: f64) -> bool {
    // Closest-point distances that converge to round-off (~1e-12) differ in their last digits.
    (a.is_nan() && b.is_nan()) || a == b || (a - b).abs() <= 1e-9 * a.abs().max(b.abs()) + 1e-11
}

/// Compares the numbers of one score with score.py's record; returns the mismatches.
fn compare(rs: &Score, py: &Value) -> Vec<String> {
    let mut bad = Vec::new();
    let f = |k: &str| pnum(py.get(k));
    let mut num = |k: &str, v: f64| {
        let p = f(k);
        if !close(v, p) {
            bad.push(format!("{k}: rust {v:e} python {p:e}"));
        }
    };
    num("cov_tol", rs.cov_tol);
    if rs.verdict != Verdict::Crash {
        num("max_dist_3d", rs.max_dist_3d);
        num("max_dist_uv", rs.max_dist_uv);
        num("hausdorff_truth_to_out", rs.hausdorff_truth_to_out);
        num("hausdorff_out_to_truth", rs.hausdorff_out_to_truth);
        let cov = py.get("coverage").and_then(Value::as_array).cloned().unwrap_or_default();
        if cov.len() != rs.coverage.len() {
            bad.push(format!("coverage: {} entries, python {}", rs.coverage.len(), cov.len()));
        }
        for (c, p) in rs.coverage.iter().zip(&cov) {
            let g = |k: &str| pnum(p.get(k));
            let (a, b) = match c {
                Coverage::Polyline { max, covered_frac } => ((*max, g("max")), (*covered_frac, g("covered_frac"))),
                Coverage::Point { dist, .. } => ((*dist, g("dist")), (0.0, 0.0)),
            };
            if !close(a.0, a.1) || !close(b.0, b.1) {
                bad.push(format!("coverage {c:?} python {p}"));
            }
        }
        let strs = |k: &str| -> Vec<String> {
            py.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
        };
        if strs("expected_topology") != rs.expected_topology || strs("output_topology") != rs.output_topology {
            bad.push(format!(
                "topology {:?}/{:?} python {:?}/{:?}",
                rs.output_topology,
                rs.expected_topology,
                strs("output_topology"),
                strs("expected_topology")
            ));
        }
        if strs("types") != rs.types {
            bad.push("types".into());
        }
        let n = |k: &str| py.get(k).and_then(Value::as_u64).unwrap_or(u64::MAX) as usize;
        if n("n_curves") != rs.n_curves || n("n_out_points") != rs.n_out_points {
            bad.push("curve/point counts".into());
        }
    }
    let flags: Vec<String> =
        py.get("flags").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if flags != rs.flags {
        bad.push(format!("flags: rust {:?} python {:?}", rs.flags, flags));
    }
    bad
}

#[test]
#[ignore = "needs the corpus runner's raw outputs and score.py's scores (SSI_PARITY_RAW, SSI_PARITY_SCORES)"]
fn parity_with_python_scorer() {
    let raw = std::env::var("SSI_PARITY_RAW").expect("SSI_PARITY_RAW: the runner's raw output directory");
    let scores = std::env::var("SSI_PARITY_SCORES").expect("SSI_PARITY_SCORES: score.py's score JSON");
    let tol: f64 = std::env::var("SSI_PARITY_TOL").map(|s| s.parse().expect("SSI_PARITY_TOL")).unwrap_or(1e-6);
    let names = std::env::var("SSI_SM_MESSAGES")
        .ok()
        .map(|p| ssi_score::status_names(&std::fs::read_to_string(p).expect("SSI_SM_MESSAGES")))
        .unwrap_or_default();
    let py = python_json(&std::fs::read_to_string(&scores).expect("scores"));
    let py = py.as_array().expect("score list");
    assert_eq!(py.len(), 119, "score.py scored {} cases", py.len());

    let scored = par_map(py, |p| {
        let id = p["id"].as_str().expect("id");
        let case = load_case(id).expect("case");
        let outcome = if p["verdict"] == "CRASH" {
            Outcome::Crashed(p["detail"].as_str().unwrap_or_default().to_string())
        } else {
            let path = std::path::Path::new(&raw).join(format!("{id}.json"));
            let o: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("raw output")).expect("raw JSON");
            let mut r = EngineResult::from_runner_json(&o).expect("runner JSON");
            if r.status_name.is_none() {
                r.status_name = names.get(&r.status).cloned();
            }
            Outcome::Ran(r)
        };
        let t0 = std::time::Instant::now();
        let rs = score_case(&case, &outcome, Tolerances::at(tol));
        (rs, t0.elapsed().as_millis())
    });

    let mut same = 0;
    let mut problems = Vec::new();
    let delta = |a: f64, b: f64| if a == b || (a.is_nan() && b.is_nan()) { 0.0 } else { (a - b).abs() };
    println!("{:<40} {:<15} {:<15} |rust - python|: max_dist_3d, max_dist_uv, hausdorff both ways", "case", "python", "rust");
    for (p, (rs, ms)) in py.iter().zip(&scored) {
        let id = rs.id.as_str();
        let pv = p["verdict"].as_str().unwrap_or_default();
        let g = |k: &str| pnum(p.get(k));
        let deltas = [
            delta(rs.max_dist_3d, g("max_dist_3d")),
            delta(rs.max_dist_uv, g("max_dist_uv")),
            delta(rs.hausdorff_truth_to_out, g("hausdorff_truth_to_out")),
            delta(rs.hausdorff_out_to_truth, g("hausdorff_out_to_truth")),
        ];
        println!(
            "{id:<40} {pv:<15} {:<15} {} {} {} {}  ({ms} ms)",
            rs.verdict.as_str(),
            fmt_e(deltas[0], 1),
            fmt_e(deltas[1], 1),
            fmt_e(deltas[2], 1),
            fmt_e(deltas[3], 1)
        );
        if pv == rs.verdict.as_str() {
            same += 1;
        } else {
            problems.push(format!("{id}: verdict rust {} python {pv}", rs.verdict));
        }
        for b in compare(rs, p) {
            problems.push(format!("{id}: {b}"));
        }
    }
    println!("{same}/119 identical verdicts");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// ---- SMLib itself (feature `native`) ---------------------------------------------------------

/// The cases SMLib's GlobalSurfaceIntersect passes at approximation tolerance 1e-6, as score.py
/// grades the corpus runner's output. A fix that makes another case pass adds it here.
const GSI_1E6_PASS: &[&str] = &[
    "c1_cone_plane_ellipse",
    "c1_cone_plane_hyperbola",
    "c1_cyl_cyl_two_loops",
    "c1_plane_cyl_ellipse",
    "c1_plane_halfcyl_arc",
    "c1_plane_sphere_circle",
    "c1_sphere_cyl_two_loops",
    "c1_torus_plane_nested",
    "c1n_cylN_plane_ellipse",
    "c1n_parab_parab_circle",
    "c1n_parab_plane_circle",
    "c1n_saddle_plane_hyperbola",
    "c1n_sphereN_cylN_two_loops",
    "c1n_torusN_planeN_nested",
    "c2_cones_tangent_ruling",
    "c2_cyl_cyl_external",
    "c2_cyl_on_plane",
    "c2_sphere_in_cyl",
    "c2_sphere_in_torus",
    "c2n_cylN_on_planeN",
    "c2n_parabcyl_plane",
    "c2n_quartic_circle",
    "c2n_quartic_contact_parabola",
    "c2n_sphereN_in_cyl",
    "c3_sphere_on_plane",
    "c3_sphere_torus_outer",
    "c3_spheres_external",
    "c3_spheres_internal",
    "c3n_four_points",
    "c3n_parab_apex",
    "c3n_parab_parab_apex",
    "c4_cone_apex_plane",
    "c4_steinmetz",
    "c4_viviani_figure8",
    "c4n_cusp",
    "c4n_saddle_tangent_plane",
    "c4n_viviani_nurbs",
    "c5_corner_to_corner",
    "c5_curve_on_edge",
    "c5_cyl_end_circle",
    "c5_cyl_quilt_seam",
    "c5_hemisphere_equator",
    "c5_shared_border_L",
    "c5n_curve_on_edge",
    "c5n_cylN_quilt_seam",
    "c6_cyl_overlap",
    "c6_planes_identical",
    "c6_planes_overlap",
    "c6_spheres_overlap",
    "c6n_cyl_vs_nurbscyl",
    "c6n_plane_vs_nurbsplane",
    "c6n_sphere_vs_nurbssphere",
    "c7_angle_planes_1e-3",
    "c7_angle_planes_1e-4",
    "c7_angle_planes_1e-5",
    "c7_offset_planes_1e-3",
    "c7_offset_planes_1e-5",
    "c7_offset_spheres_1e-3",
    "c7_offset_spheres_1e-5",
    "c7_sunk_cyl_1e-3",
    "c7_sunk_cyl_1e-4",
    "c7_sunk_cyl_1e-5",
    "c7_sunk_cyl_1e-6",
    "c7_tiny_loop_parab_1e-4",
    "c7_tiny_loop_parab_1e-5",
    "c7_tiny_loop_sphere_1e-4",
    "c7_tiny_loop_sphere_1e-5",
    "c7_tiny_loop_sphere_1e-6",
    "c8_cone_plane_seam",
    "c8_cyl_plane_seam_line",
    "c8_sphere_cyl_through_poles",
    "c8_sphere_plane_poles",
    "c8_sphere_plane_poles_seam",
    "c8_sphere_polar_cap",
    "c8_torus_axis_plane_seam",
    "c8_torus_equator_seam",
    "c8n_cylN_seam_line",
    "c8n_sphereN_polar_cap",
    "c8n_sphereN_poles",
    "c8n_torusN_equator_seam",
    "c9n_deg6_circle",
    "c9n_elevated_paraboloids",
    "c9n_many_spans_plane_sphere",
    "c9n_rational_deg5_cyl_arc",
    "c9n_rational_deg5_sphere_arc",
    "c9n_refined_torus_sphere",
    "y23_appendix_ex2",
    "y23_fig1a_two_loops_cross",
    "y23_fig1b_two_arcs_and_point",
    "y23_fig1c_boundary_triangle",
    "y23_fig1d_two_quads",
    "y23_fig1e_loops_touching",
    "y23_fig1f_high_order_contact",
    "y23_fig1g_three_points",
    "y23_t2_ex1_tangent_line",
    "y23_t2_ex2_tangential_curve",
    "y23_t2_ex3_loop_two_curves",
    "y23_t2_ex4_tangential_curve_saddle",
    "y23_t2_ex5_two_loops",
    "y23_t2_ex6_loop_and_point",
    "y23_t3_ex1_torus_ellipsoid",
    "y23_t3_ex2_torus_ellipsoid",
    "y23_t3_ex3_torus_ellipsoid",
    "y23_t3_ex4_torus_ellipsoid",
    "y23_t3_ex5_torus_ellipsoid",
    "y23_t6_ex1_tangent_line",
    "y23_t6_ex2_tangential_arc",
    "y23_t6_ex3_two_crossing_curves",
    "y23_t6_ex4_tangential_point",
    "y23_t6_ex5_four_boundary_curves",
    "y23_t6_ex6_four_boundary_curves",
];

/// The corpus runner's samples per curve.
const RUNNER_SAMPLES: usize = 4001;

fn surface(s: &Value) -> Result<solidcraft_smlib::ssi::Surface, String> {
    use solidcraft_smlib::ssi::Surface;
    let f = |k: &str| s.get(k).map(ssi_score::geom::num).ok_or(format!("{k}: missing"));
    let v3 = |k: &str| -> Result<[f64; 3], String> {
        let a = s.get(k).and_then(Value::as_array).filter(|a| a.len() == 3).ok_or(format!("{k}: not 3 numbers"))?;
        Ok([0, 1, 2].map(|i| ssi_score::geom::num(&a[i])))
    };
    let v2 = |k: &str| -> Result<[f64; 2], String> {
        let a = s.get(k).and_then(Value::as_array).filter(|a| a.len() == 2).ok_or(format!("{k}: not 2 numbers"))?;
        Ok([0, 1].map(|i| ssi_score::geom::num(&a[i])))
    };
    let list = |k: &str| -> Vec<f64> { s.get(k).and_then(Value::as_array).map(|a| a.iter().map(ssi_score::geom::num).collect()).unwrap_or_default() };
    let r = match s.get("type").and_then(Value::as_str).unwrap_or_default() {
        "plane" => Surface::plane(v3("origin")?, v3("xaxis")?, v3("yaxis")?, v2("u")?, v2("v")?),
        "cylinder" => Surface::cylinder(v3("origin")?, v3("xaxis")?, v3("yaxis")?, f("radius")?, v2("angle")?, v2("height")?),
        "cone" => Surface::cone(v3("apex")?, v3("xaxis")?, v3("yaxis")?, f("half_angle_deg")?, v2("height")?),
        "sphere" => Surface::sphere(v3("center")?, v3("xaxis")?, v3("yaxis")?, f("radius")?, v2("lon")?, v2("lat")?),
        "torus" => Surface::torus(v3("center")?, v3("xaxis")?, v3("yaxis")?, f("R")?, f("r")?, v2("major")?, v2("minor")?),
        "nurbs" => {
            let d = v2("degree")?;
            let points: Vec<[f64; 3]> = s
                .get("points")
                .and_then(Value::as_array)
                .ok_or("points: missing")?
                .iter()
                .map(|p| [0, 1, 2].map(|i| ssi_score::geom::num(&p[i])))
                .collect();
            let weights = list("weights");
            Surface::nurbs(
                [d[0] as usize, d[1] as usize],
                f("nu")? as usize,
                f("nv")? as usize,
                &list("knots_u"),
                &list("knots_v"),
                &points,
                (!weights.is_empty()).then_some(&weights[..]),
            )
        }
        t => return Err(format!("unknown surface type {t}")),
    };
    r.map_err(|e| e.to_string())
}

/// SMLib's answer as the corpus runner writes it for `gsi`: each exact curve sampled where the
/// runner samples it ([`RUNNER_SAMPLES`] parameters spread evenly over the 3D curve's natural
/// interval, each UV curve over its own, the surfaces at the UV samples), samples that cannot be
/// evaluated dropped, empty UV arrays for a UV curve the method did not return, the runner's type
/// names, and no isolated points (the runner only reports those in face mode).
fn runner_output(
    r: solidcraft_smlib::Result<solidcraft_smlib::ssi::SsiResult>,
    a: &solidcraft_smlib::ssi::Surface,
    b: &solidcraft_smlib::ssi::Surface,
    ms: f64,
) -> Outcome {
    use solidcraft_geom::brep::{Curve2, eval::CurveEval};
    use solidcraft_smlib::ssi::{CurveType, Surface};
    let r = match r {
        Ok(r) => r,
        Err(solidcraft_smlib::Error::Status(s)) => {
            return Outcome::Ran(EngineResult { status: i64::from(s), status_name: None, time_ms: ms, curves: Vec::new(), points: Vec::new() });
        }
        Err(e) => return Outcome::Crashed(e.to_string()),
    };
    let params = |[t0, t1]: [f64; 2]| (0..RUNNER_SAMPLES).map(move |i| t0 + (t1 - t0) * (i as f64 / (RUNNER_SAMPLES - 1) as f64));
    let on = |c: &Option<Curve2>, s: &Surface| -> (Vec<[f64; 2]>, Vec<V3>) {
        let Some(c) = c else { return (Vec::new(), Vec::new()) };
        let Some(ivl) = c.interval() else { return (Vec::new(), Vec::new()) };
        params(ivl).filter_map(|t| c.eval(t)).filter_map(|q| s.evaluate(q[0], q[1]).ok().map(|p| (q, p))).unzip()
    };
    let mut curves = Vec::new();
    for c in r.curves {
        let curve_type = match c.curve_type {
            CurveType::None => "n/a",
            CurveType::Unknown => "UNKNOWN",
            CurveType::Touching => "TOUCHING",
            CurveType::Crossing => "CROSSING",
            CurveType::Tangent => "TANGENT",
            CurveType::Coincident => "COINCIDENT",
            CurveType::NearTangent => "NEAR_TANGENT",
            CurveType::RegionBoundary => "REGION_BOUNDARY",
            CurveType::Other(_) => "?",
        };
        let xyz = match CurveEval::new(&c.curve) {
            Ok(e) => params(c.interval).filter_map(|t| e.point(t)).map(|p| [p.x, p.y, p.z]).collect(),
            Err(e) => return Outcome::Crashed(format!("an intersection curve: {e}")),
        };
        let (uv1, s1_at_uv) = on(&c.uv_a, a);
        let (uv2, s2_at_uv) = on(&c.uv_b, b);
        curves.push(EngineCurve { curve_type: curve_type.into(), xyz, uv1, uv2, s1_at_uv, s2_at_uv });
    }
    Outcome::Ran(EngineResult { status: ssi_score::SM_SUCCESS, status_name: None, time_ms: ms, curves, points: Vec::new() })
}

/// One kernel at a time: the choice of kernel is process-wide.
static KERNEL_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Every case through `backend`'s GlobalSurfaceIntersect at approximation tolerance 1e-6, scored as
/// score.py scores the corpus runner's `gsi 1e-6` run. Prints each case that does not pass.
/// `None` when `SOLIDCRAFT_KERNEL` chose the other kernel: it wins over `set_choice`, and scoring
/// the wrong kernel against this one's list says nothing.
fn global_intersect_scores(backend: solidcraft_smlib::route::Backend) -> Option<Vec<Score>> {
    use solidcraft_smlib::route::{Backend, choice, set_choice};
    use solidcraft_smlib::ssi::{Method, intersect};
    let t0 = std::time::Instant::now();
    let cases = all_cases();
    let outcomes: Vec<Outcome> = {
        let _kernel = KERNEL_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_choice(backend);
        if choice() != backend {
            set_choice(Backend::Solidcore);
            eprintln!("skipped: SOLIDCRAFT_KERNEL chose the other kernel");
            return None;
        }
        // The kernels serialise their calls, so the cases run one after another; only the scoring is parallel.
        let outcomes = cases
            .iter()
            .map(|case| {
                let a = surface(&case.surfaces[0]).unwrap_or_else(|e| panic!("{} surface 1: {e}", case.id));
                let b = surface(&case.surfaces[1]).unwrap_or_else(|e| panic!("{} surface 2: {e}", case.id));
                let t = std::time::Instant::now();
                let r = intersect(&a, &b, Method::Global, 1e-6);
                runner_output(r, &a, &b, t.elapsed().as_secs_f64() * 1e3)
            })
            .collect();
        set_choice(Backend::Solidcore);
        outcomes
    };
    let ran = t0.elapsed();
    // Status names for the flags, from the SMLib sources the build used, as score.py has them.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::env::var_os("SOLIDCRAFT_SMLIB_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| {
        let rev = std::fs::read_to_string(manifest.join("SMLIB_REV")).unwrap_or_default();
        manifest.join("../../target").join(format!("smlib-{}", rev.trim().get(..12).unwrap_or_default()))
    });
    let names = std::fs::read_to_string(src.join("source/SMLib/inc/SmMessages.h")).map(|h| ssi_score::status_names(&h)).unwrap_or_default();
    let outcomes: Vec<Outcome> = outcomes
        .into_iter()
        .map(|o| match o {
            Outcome::Ran(mut r) if r.status_name.is_none() => {
                r.status_name = names.get(&r.status).cloned();
                Outcome::Ran(r)
            }
            o => o,
        })
        .collect();
    let pairs: Vec<(&Case, &Outcome)> = cases.iter().zip(&outcomes).collect();
    let scores = par_map(&pairs, |(case, out)| score_case(case, out, Tolerances::at(1e-6)));

    let mut by_verdict: std::collections::BTreeMap<Verdict, Vec<&str>> = Default::default();
    for s in &scores {
        by_verdict.entry(s.verdict).or_default().push(&s.id);
    }
    println!(
        "{backend:?} GlobalSurfaceIntersect, approx tol 1e-6 ({} cases, kernel {:.1} s, total {:.1} s)",
        scores.len(),
        ran.as_secs_f64(),
        t0.elapsed().as_secs_f64()
    );
    for (v, ids) in &by_verdict {
        println!("  {:<15} {:>3}", v.as_str(), ids.len());
    }
    for s in scores.iter().filter(|s| s.verdict != Verdict::Pass) {
        println!("  {:<40} {:<15} {}", s.id, s.verdict.as_str(), s.flags.join("; "));
    }
    Some(scores)
}

/// Exactly [`GSI_1E6_PASS`] pass on the C++ kernel.
#[cfg(feature = "native")]
#[test]
fn smlib_global_intersect_at_1e_6() {
    let Some(scores) = global_intersect_scores(solidcraft_smlib::route::Backend::Smlib) else { return };
    let passed: std::collections::BTreeSet<&str> = scores.iter().filter(|s| s.verdict == Verdict::Pass).map(|s| s.id.as_str()).collect();
    let listed: std::collections::BTreeSet<&str> = GSI_1E6_PASS.iter().copied().collect();
    assert_eq!(listed.len(), GSI_1E6_PASS.len(), "GSI_1E6_PASS lists a case twice");
    let newly: Vec<_> = passed.difference(&listed).collect();
    let lost: Vec<_> = listed.difference(&passed).collect();
    assert!(newly.is_empty() && lost.is_empty(), "now pass (add to GSI_1E6_PASS): {newly:?}; no longer pass: {lost:?}");
    assert_eq!(passed.len(), 111);
}

/// The cases solidcore does not pass that the C++ kernel does (default-solidcore/bugs/
/// ssi-self-crossing-curve-not-split.md). Listed so the difference is seen and only shrinks.
const SOLIDCORE_GSI_1E6_MISSES: [&str; 6] = [
    "c4_viviani_figure8",
    "c4n_viviani_nurbs",
    "y23_fig1a_two_loops_cross",
    "y23_t2_ex5_two_loops",
    "y23_t3_ex4_torus_ellipsoid",
    "y23_t6_ex3_two_crossing_curves",
];

/// solidcore passes what the C++ kernel passes, but for [`SOLIDCORE_GSI_1E6_MISSES`].
#[test]
fn solidcore_global_intersect_at_1e_6() {
    let Some(scores) = global_intersect_scores(solidcraft_smlib::route::Backend::Solidcore) else { return };
    let passed: std::collections::BTreeSet<&str> = scores.iter().filter(|s| s.verdict == Verdict::Pass).map(|s| s.id.as_str()).collect();
    let listed: std::collections::BTreeSet<&str> = GSI_1E6_PASS.iter().copied().collect();
    let lost: Vec<&str> = listed.difference(&passed).copied().collect();
    let extra: Vec<_> = passed.difference(&listed).collect();
    assert_eq!(lost, SOLIDCORE_GSI_1E6_MISSES, "solidcore's misses against the C++ kernel; extra passes: {extra:?}");
}
