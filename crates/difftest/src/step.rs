//! `--step-corpus`: imports real-world STEP files with the clean-room `step-import`
//! crate and builds every B-rep body on both kernels from the same intermediate.
//!
//! The corpus is STEP Tools' AP203/AP214 sample set, fetched into `corpus/steptools/`
//! by `scripts/fetch-steptools-corpus.sh` (All Rights Reserved; never committed). Each
//! body is encoded once ([`step_import::flat::encode`]); the Rust side builds it from
//! the decoded [`Body`] ([`build_rust`]) and the C++ side from the same numbers
//! (`harness/cpp/shim/step_import.cpp`), with the same recipe: per face its surface
//! and the bounded 3D curves of its edge loops into `CreateFacesWithCurves`, then
//! `SmApiStitchIntoSolid`, counts, `SmApiBrepIsManifoldSolid`, area and volume,
//! `SmApiHealBrep`, and counts, manifold test and volume again. Any difference is a
//! kernel difference.
//!
//! Every (body, kernel) runs in a child process (`--step-case`), so a crash or hang is a
//! `CRASH` verdict, not the end of the run. Stages are compared in order with the usual
//! rule (statuses and counts equal, values within [`crate::REL_TOL`]); the first
//! differing stage is reported. A Rust stage that reaches unported code is `DEFERRED`
//! with its unit. This is a report, not a gate: verdicts never fail the run, and it is
//! not part of CI (the files are not in the repository).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::heal::heal_data::p1::api_heal_brep;
use solidcore::heal::stitch::p1::api_stitch_into_solid;
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3;
use solidcore::topology::brep::p8::{create_faces_with_curves, FaceBuildEnv};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, SolidError};

use step_import::model::{self as sm, Body, Bound};

use crate::compare::values_agree;
use crate::ssi::{exit_detail, glob_match};
use crate::HarnessError;

/// Relative accuracy of the area and volume integrations.
pub const ACCURACY: f64 = 1e-6;
/// Per (body, kernel) time limit.
pub const TIMEOUT: Duration = Duration::from_secs(600);
/// Where the corpus is fetched.
pub const CORPUS_DIR: &str = "corpus/steptools";
/// Verdicts, in summary order.
/// `HEAL-DIFF`: every build stage agrees and the first difference is in `SmApiHealBrep`
/// or after it; `DIFF`: a build stage (faces, stitch, counts, manifold, area, volume)
/// differs; `RUST-ONLY`: a build without `cpp` built the body on the Rust side alone and
/// compared nothing.
pub const VERDICTS: [&str; 7] = [
    "MATCH",
    "HEAL-DIFF",
    "DIFF",
    "DEFERRED",
    "CRASH",
    "SKIPPED",
    "RUST-ONLY",
];

/// What one kernel reported for one body: stages in order, and where the Rust side
/// stopped on unported code.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Record {
    /// `(stage, values)` in order (names as `solidcore_ffi`'s `STAGES`).
    pub stages: Vec<(String, Vec<f64>)>,
    /// `(unit, upstream symbol)` of the unported callee that stopped the Rust side.
    pub deferred: Option<(String, String)>,
    /// Rust errors with no upstream status (shown, compared as status `-1`).
    pub notes: Vec<String>,
}

impl Record {
    fn to_json(&self) -> Value {
        let stages: Vec<Value> = self
            .stages
            .iter()
            .map(|(n, v)| json!({"stage": n, "values": v}))
            .collect();
        json!({
            "stages": stages,
            "deferred": self.deferred.as_ref().map(|(u, s)| json!({"unit": u, "symbol": s})),
            "notes": self.notes,
        })
    }

    fn from_json(v: &Value) -> Result<Self, String> {
        let mut r = Record::default();
        for s in v["stages"].as_array().ok_or("no stages")? {
            let name = s["stage"].as_str().ok_or("stage name")?.to_string();
            let vals = s["values"]
                .as_array()
                .ok_or("stage values")?
                .iter()
                .map(|x| x.as_f64().unwrap_or(f64::NAN))
                .collect();
            r.stages.push((name, vals));
        }
        if let Some(d) = v.get("deferred").filter(|d| !d.is_null()) {
            r.deferred = Some((
                d["unit"].as_str().unwrap_or("?").to_string(),
                d["symbol"].as_str().unwrap_or("?").to_string(),
            ));
        }
        if let Some(n) = v["notes"].as_array() {
            r.notes = n
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect();
        }
        Ok(r)
    }

    fn stage(&self, name: &str) -> Option<&[f64]> {
        self.stages
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
    }
}

/// Stops the Rust build on unported code.
struct Deferred(String, String);

/// The status of a Rust result as upstream reports it: `0` success, the code of an
/// upstream status, `SM_ERR_INVALID_INPUT` for `InvalidInput`, `-1` (with a note) for
/// an error with no upstream status; unported code stops the build.
fn status<T>(r: &Result<T, SolidError>, notes: &mut Vec<String>) -> Result<f64, Deferred> {
    match r {
        Ok(_) => Ok(0.0),
        Err(SolidError::Unported { upstream, unit }) => {
            Err(Deferred((*unit).to_string(), (*upstream).to_string()))
        }
        Err(SolidError::Upstream { code, .. }) => Ok(f64::from(*code)),
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => {
            Ok(f64::from(solidcore::error::SM_ERR_INVALID_INPUT))
        }
        Err(e) => {
            if notes.len() < 20 {
                notes.push(e.to_string());
            }
            Ok(-1.0)
        }
    }
}

fn v3(p: sm::P3) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn a2p(f: &sm::Frame) -> Axis2Placement {
    Axis2Placement::new(v3(f.origin), v3(f.x), v3(f.y))
}

fn usizes(m: &[u32]) -> Vec<usize> {
    m.iter().map(|&x| x as usize).collect()
}

/// The curve bounded to `[lo, hi]` (an edge) or unbounded (a swept curve), as the shim's
/// `make_curve`.
fn make_curve(k: &Kernel, c: &sm::Curve, bounds: Option<(f64, f64)>) -> Result<Curve, SolidError> {
    match c {
        sm::Curve::Line {
            origin,
            dir,
            magnitude,
        } => Ok(match bounds {
            Some((lo, hi)) => Curve::from(Line::bounded(
                v3(*origin),
                v3(*dir),
                &Extent1d::from_min_max(lo, hi),
                &BoundedLineOptions {
                    scale: *magnitude,
                    dim: 3,
                    ..BoundedLineOptions::default()
                },
            )),
            None => Curve::from(Line::create_canonical(v3(*origin), v3(*dir))),
        }),
        sm::Curve::Circle { frame, radius } => {
            let ivl = bounds.map(|(lo, hi)| Extent1d::from_min_max(lo, hi));
            Ok(Curve::from(Ellipse::create_canonical_circle(
                k,
                &a2p(frame),
                *radius,
                ivl.as_ref(),
                None,
            )?))
        }
        sm::Curve::Ellipse {
            frame,
            semi1,
            semi2,
        } => {
            let ivl = bounds.map(|(lo, hi)| Extent1d::from_min_max(lo, hi));
            Ok(Curve::from(Ellipse::create_canonical(
                k,
                &a2p(frame),
                *semi1,
                *semi2,
                ivl.as_ref(),
                None,
            )?))
        }
        sm::Curve::BSpline(b) => {
            let (k0, k1) = (
                b.knots.first().copied().unwrap_or(0.0),
                b.knots.last().copied().unwrap_or(0.0),
            );
            // Trim only to a strict sub-interval of the knot range.
            let trim = bounds
                .filter(|&(lo, hi)| !(lo == k0 && hi == k1))
                .map(|(lo, hi)| Extent1d::from_min_max(lo, hi));
            let pts: Vec<Point3> = b.points.iter().map(|p| v3(*p)).collect();
            let mults = usizes(&b.mults);
            Ok(Curve::from(BSplineCurve::create_canonical(
                &CanonicalData {
                    dim: 3,
                    degree: b.degree as usize,
                    control_points: &pts,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &mults,
                    knots: &b.knots,
                    knot_type: KnotType::Unspecified,
                    weights: b.weights.as_deref(),
                    trim,
                },
            )?))
        }
    }
}

fn make_surface(k: &Kernel, s: &sm::Surface) -> Result<Surface, SolidError> {
    Ok(match s {
        sm::Surface::Plane { frame } => Surface::from(Plane::create_canonical(&a2p(frame))?),
        sm::Surface::Cylinder { frame, radius } => {
            Surface::from(Cone::create_canonical_cylinder(k, &a2p(frame), *radius)?)
        }
        sm::Surface::Cone {
            frame,
            radius,
            semi_angle_deg,
        } => Surface::from(Cone::create_canonical_with_semi_angle(
            k,
            &a2p(frame),
            *radius,
            *semi_angle_deg,
            None,
            false,
            false,
        )?),
        sm::Surface::Sphere { frame, radius } => {
            Surface::from(Sphere::create_canonical(k, &a2p(frame), *radius)?)
        }
        sm::Surface::Torus {
            frame,
            major,
            minor,
        } => Surface::from(Torus::create_canonical(
            k,
            &a2p(frame),
            *major,
            *minor,
            TorusOptions::default(),
            None,
        )?),
        sm::Surface::Revolution {
            curve,
            axis_origin,
            axis_dir,
        } => {
            let swept = make_curve(k, curve, None)?;
            Surface::from(SurfOfRevolution::create_canonical(
                k,
                swept,
                v3(*axis_origin),
                v3(*axis_dir),
                false,
            )?)
        }
        sm::Surface::Extrusion {
            curve,
            dir,
            magnitude,
        } => {
            let swept = make_curve(k, curve, None)?;
            let m = *magnitude;
            Surface::from(SurfOfExtrusion::create_canonical(
                k,
                swept,
                Vec3::new(dir[0] * m, dir[1] * m, dir[2] * m),
            )?)
        }
        sm::Surface::BSpline(b) => {
            let pts: Vec<Point3> = b.points.iter().flatten().map(|p| v3(*p)).collect();
            let w: Option<Vec<f64>> = b
                .weights
                .as_ref()
                .map(|w| w.iter().flatten().copied().collect());
            let um = usizes(&b.u_mults);
            let vm = usizes(&b.v_mults);
            Surface::from(BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: b.u_degree as usize,
                v_degree: b.v_degree as usize,
                control_points: &pts,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &um,
                v_multiplicities: &vm,
                u_knots: &b.u_knots,
                v_knots: &b.v_knots,
                knot_type: KnotType::Unspecified,
                weights: w.as_deref(),
                uv_domain: None,
            })?)
        }
    })
}

/// One face's loops as the shim gathers them: curve counts per edge loop, the bounded
/// curves (the first failure stops making them) and their orientations.
struct FaceInput {
    loops: Vec<usize>,
    curves: Vec<Option<Curve>>,
    orients: Vec<OrientType>,
}

fn face_input(
    k: &Kernel,
    body: &Body,
    face: &sm::Face,
    mut ok: bool,
    first_err: &mut Option<SolidError>,
) -> FaceInput {
    let mut out = FaceInput {
        loops: Vec::new(),
        curves: Vec::new(),
        orients: Vec::new(),
    };
    for l in &face.loops {
        let Bound::Edges(es) = &l.bound else {
            continue; // vertex loops are skipped
        };
        let oes: Vec<(usize, bool)> = if l.orientation {
            es.iter().map(|e| (e.edge, e.forward)).collect()
        } else {
            es.iter().rev().map(|e| (e.edge, !e.forward)).collect()
        };
        out.loops.push(oes.len());
        for (ei, fwd) in oes {
            let Some(e) = body.edges.get(ei) else {
                continue;
            };
            let same = e.same_sense;
            let (lo, hi) = if same {
                (e.t_start, e.t_end)
            } else {
                (e.t_end, e.t_start)
            };
            let c = if ok {
                match make_curve(k, &e.curve, Some((lo, hi))) {
                    Ok(c) => Some(c),
                    Err(err) => {
                        ok = false;
                        *first_err = Some(err);
                        None
                    }
                }
            } else {
                None
            };
            out.curves.push(c);
            out.orients.push(if fwd == same {
                OrientType::Same
            } else {
                OrientType::Opposite
            });
        }
    }
    out
}

fn counts(k: &mut Kernel, b: &Brep) -> Result<Vec<f64>, SolidError> {
    Ok(vec![
        bp1::regions(b, None)?.len() as f64,
        bp1::shells(k, b, None)?.len() as f64,
        bp1::faces(b, None)?.len() as f64,
        bp1::edges(b, None)?.len() as f64,
        bp1::vertices(b, None)?.len() as f64,
    ])
}

/// Builds `body` with the Rust port (the shim's recipe, stage by stage).
pub fn build_rust(body: &Body) -> Record {
    let mut rec = Record::default();
    if let Err(Deferred(u, s)) = build_rust_stages(body, &mut rec) {
        rec.deferred = Some((u, s));
    }
    rec
}

fn build_rust_stages(body: &Body, rec: &mut Record) -> Result<(), Deferred> {
    let mut k = Kernel::default();
    let mut notes = Vec::new();
    let mut b = match p3::new_brep(
        &mut k,
        solidcore::geom::tol::USE_DEFAULT,
        solidcore::geom::tol::USE_DEFAULT,
    ) {
        Ok(b) => b,
        Err(e) => {
            status(&Err::<(), _>(e), &mut rec.notes)?;
            return Ok(());
        }
    };

    // faces
    let mut per_face = Vec::new();
    let (mut made_total, mut failed) = (0.0, 0.0);
    {
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let key_of: &dyn Fn(EntityRef) -> solidcore::geom::object::ObjectRef = env.key_of;
        let this_face = |f: FaceId| key_of(EntityRef::Face(f));
        let mut marks = Marks::default();
        let mut fenv = FaceBuildEnv {
            env: &mut env,
            marks: &mut marks,
            this_face: &this_face,
        };
        for face in &body.faces {
            let mut first_err = None;
            let srf = match make_surface(&k, &face.surface) {
                Ok(s) => Some(s),
                Err(e) => {
                    first_err = Some(e);
                    None
                }
            };
            let input = face_input(&k, body, face, srf.is_some(), &mut first_err);
            let (st, made) = if let Some(e) = first_err {
                (status(&Err::<(), _>(e), &mut notes)?, 0.0)
            } else {
                let r = create_faces_with_curves(
                    &mut k,
                    &mut b,
                    None,
                    &input.loops,
                    Some(input.curves),
                    &input.orients,
                    srf,
                    &mut fenv,
                );
                let made = r.as_ref().map_or(0.0, |f| f.faces.len() as f64);
                (status(&r, &mut notes)?, made)
            };
            if st != 0.0 {
                failed += 1.0;
            }
            made_total += made;
            per_face.push(st);
            per_face.push(made);
        }
    }
    let mut fv = vec![body.faces.len() as f64, made_total, failed];
    fv.extend(per_face);
    rec.stages.push(("faces".into(), fv));

    let result = (|| -> Result<(), Deferred> {
        let r = api_stitch_into_solid(&mut k, Some(&mut b));
        let s = status(&r, &mut notes)?;
        rec.stages.push((
            "stitch".into(),
            match &r {
                Ok(a) => vec![
                    s,
                    f64::from(u8::from(a.produces_a_solid)),
                    a.stitched_edges as f64,
                    a.max_vertex_gap,
                    a.max_edge_gap,
                ],
                Err(_) => vec![s, 0.0, 0.0, 0.0, 0.0],
            },
        ));
        let c = counts(&mut k, &b);
        status(&c, &mut notes)?;
        rec.stages.push(("counts".into(), c.unwrap_or_default()));
        let m = brep_is_manifold_solid(&b);
        let s = status(&m, &mut notes)?;
        rec.stages.push((
            "manifold".into(),
            vec![s, f64::from(u8::from(m.unwrap_or(false)))],
        ));
        let a = brep_area(&k, &mut b, ACCURACY);
        let s = status(&a, &mut notes)?;
        rec.stages.push(("area".into(), vec![s, a.unwrap_or(0.0)]));
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let v = brep_volume(&k, &mut b, ACCURACY, &attrs);
        let s = status(&v, &mut notes)?;
        rec.stages
            .push(("volume".into(), vec![s, v.unwrap_or(0.0)]));
        let h = api_heal_brep(&mut k, Some(&mut b));
        let s = status(&h, &mut notes)?;
        rec.stages.push(("heal".into(), vec![s]));
        let c = counts(&mut k, &b);
        status(&c, &mut notes)?;
        rec.stages
            .push(("healed_counts".into(), c.unwrap_or_default()));
        let m = brep_is_manifold_solid(&b);
        let s = status(&m, &mut notes)?;
        rec.stages.push((
            "healed_manifold".into(),
            vec![s, f64::from(u8::from(m.unwrap_or(false)))],
        ));
        let v = brep_volume(&k, &mut b, ACCURACY, &attrs);
        let s = status(&v, &mut notes)?;
        rec.stages
            .push(("healed_volume".into(), vec![s, v.unwrap_or(0.0)]));
        Ok(())
    })();
    rec.notes.extend(notes);
    result
}

/// Builds `body` with the C++ reference.
#[cfg(feature = "cpp")]
pub fn build_cpp(body: &Body) -> Result<Record, String> {
    use solidcore_ffi::units::step_import as ffi;
    let flat = step_import::flat::encode(body);
    let out = ffi::build(&flat, ACCURACY).map_err(|e| e.to_string())?;
    let stages = ffi::stages(&out).map_err(|e| e.to_string())?;
    Ok(Record {
        stages: stages
            .into_iter()
            .map(|(n, v)| (n.to_string(), v))
            .collect(),
        ..Record::default()
    })
}

fn load_body(path: &Path, index: usize) -> Result<Body, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let model = step_import::import(&bytes, &step_import::Limits::default())
        .map_err(|e| format!("parse: {e}"))?;
    let body = model
        .bodies
        .into_iter()
        .nth(index)
        .ok_or_else(|| format!("no body {index}"))?;
    // Both kernels build from the decoded encoding: the very numbers the shim reads.
    step_import::flat::decode(&step_import::flat::encode(&body))
}

/// `--step-case <file> <body> <cpp|rust> <out.json>`: one kernel on one body.
pub fn run_child(args: &[String]) -> ExitCode {
    let [file, index, side, out] = args else {
        eprintln!("usage: difftest --step-case <file.stp> <body> <cpp|rust> <out.json>");
        return ExitCode::from(2);
    };
    let Ok(index) = index.parse::<usize>() else {
        eprintln!("difftest: bad body index {index}");
        return ExitCode::from(2);
    };
    let body = match load_body(Path::new(file), index) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("difftest: {e}");
            return ExitCode::from(2);
        }
    };
    let rec = match side.as_str() {
        "rust" => Ok(build_rust(&body)),
        #[cfg(feature = "cpp")]
        "cpp" => build_cpp(&body),
        other => Err(format!("side `{other}` not available in this build")),
    };
    match rec {
        Ok(r) => {
            let text = serde_json::to_string(&r.to_json()).unwrap_or_default();
            match std::fs::write(out, text + "\n") {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("difftest: {out}: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Err(e) => {
            eprintln!("difftest: {e}");
            ExitCode::from(2)
        }
    }
}

/// Options of [`run_corpus`].
#[derive(Debug, Clone, Default)]
pub struct CorpusOptions {
    /// Only files whose path under the corpus matches this glob.
    pub glob: Option<String>,
    /// Child processes at a time.
    pub jobs: usize,
    /// Where the report and raw outputs go (default `bin/step-corpus`).
    pub report_dir: Option<PathBuf>,
    /// Write no report.
    pub no_report: bool,
}

/// The sides this build runs.
fn sides() -> &'static [&'static str] {
    if cfg!(feature = "cpp") {
        &["cpp", "rust"]
    } else {
        &["rust"]
    }
}

fn step_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            step_files(&p, out);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("stp") || e.eq_ignore_ascii_case("step"))
        {
            out.push(p);
        }
    }
}

type RunResult = Result<Record, String>;

struct Job {
    slot: (usize, usize),
    out: PathBuf,
    child: Child,
    start: Instant,
}

fn read_record(path: &Path) -> RunResult {
    let text = std::fs::read_to_string(path).map_err(|e| format!("unreadable output: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("unreadable output: {e}"))?;
    Record::from_json(&v)
}

/// Runs every (job, side) in a child process, `jobs` at a time.
fn run_children(
    work: &[(PathBuf, usize, String)],
    raw: &Path,
    jobs: usize,
) -> Result<Vec<Vec<RunResult>>, HarnessError> {
    let exe =
        std::env::current_exe().map_err(|e| HarnessError::new(format!("current_exe: {e}")))?;
    let mut results: Vec<Vec<RunResult>> = work
        .iter()
        .map(|_| sides().iter().map(|_| Err("not run".to_string())).collect())
        .collect();
    let mut queue: Vec<(usize, usize)> = (0..work.len())
        .flat_map(|w| (0..sides().len()).map(move |s| (w, s)))
        .rev()
        .collect();
    let mut running: Vec<Job> = Vec::new();
    while !queue.is_empty() || !running.is_empty() {
        while running.len() < jobs.max(1) {
            let Some((w, s)) = queue.pop() else { break };
            let (Some((file, body, id)), Some(side)) = (work.get(w), sides().get(s)) else {
                continue;
            };
            let out = raw.join(format!("{id}.{side}.json"));
            let log = raw.join(format!("{id}.{side}.log"));
            let _ = std::fs::remove_file(&out);
            let log_file = std::fs::File::create(&log)
                .map_err(|err| HarnessError::new(format!("{}: {err}", log.display())))?;
            let log_err = log_file
                .try_clone()
                .map_err(|err| HarnessError::new(format!("{}: {err}", log.display())))?;
            let child = Command::new(&exe)
                .arg("--step-case")
                .arg(file)
                .arg(body.to_string())
                .arg(side)
                .arg(&out)
                .stdin(Stdio::null())
                .stdout(log_file)
                .stderr(log_err)
                .spawn()
                .map_err(|err| HarnessError::new(format!("spawn {}: {err}", exe.display())))?;
            running.push(Job {
                slot: (w, s),
                out,
                child,
                start: Instant::now(),
            });
        }
        let mut i = 0;
        let mut progressed = false;
        while i < running.len() {
            let done = match running.get_mut(i) {
                Some(job) => match job.child.try_wait() {
                    Ok(Some(st)) => Some(if st.success() {
                        read_record(&job.out)
                    } else {
                        Err(exit_detail(st))
                    }),
                    Ok(None) if job.start.elapsed() > TIMEOUT => {
                        let _ = job.child.kill();
                        let _ = job.child.wait();
                        Some(Err(format!("timeout after {} s", TIMEOUT.as_secs())))
                    }
                    Ok(None) => None,
                    Err(err) => Some(Err(format!("wait failed: {err}"))),
                },
                None => None,
            };
            if let Some(r) = done {
                let job = running.swap_remove(i);
                if let Some(slot) = results
                    .get_mut(job.slot.0)
                    .and_then(|v| v.get_mut(job.slot.1))
                {
                    *slot = r;
                }
                progressed = true;
            } else {
                i += 1;
            }
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(results)
}

/// The first stage where the two records differ: `(stage, index, cpp, rust)`.
pub fn first_difference(cpp: &Record, rust: &Record) -> Option<(String, usize, f64, f64)> {
    for (name, rv) in &rust.stages {
        let Some(cv) = cpp.stage(name) else {
            continue;
        };
        if cv.len() != rv.len() {
            return Some((name.clone(), usize::MAX, cv.len() as f64, rv.len() as f64));
        }
        for (i, (&a, &b)) in cv.iter().zip(rv).enumerate() {
            if !values_agree(a, b, None) {
                return Some((name.clone(), i, a, b));
            }
        }
    }
    None
}

/// A short summary of one side: faces built, solid, volume.
fn side_summary(r: &RunResult) -> Value {
    match r {
        Err(e) => json!({"error": e}),
        Ok(rec) => {
            let faces = rec
                .stage("faces")
                .map(|v| json!({"total": v.first(), "made": v.get(1), "failed": v.get(2)}));
            json!({
                "faces": faces,
                "stitch": rec.stage("stitch"),
                "counts": rec.stage("counts"),
                "manifold": rec.stage("manifold"),
                "area": rec.stage("area"),
                "volume": rec.stage("volume"),
                "heal": rec.stage("heal"),
                "healed_counts": rec.stage("healed_counts"),
                "healed_manifold": rec.stage("healed_manifold"),
                "healed_volume": rec.stage("healed_volume"),
                "deferred": rec.deferred.as_ref().map(|(u, s)| json!({"unit": u, "symbol": s})),
                "notes": rec.notes,
            })
        }
    }
}

/// The verdict of one body from its results in [`sides`] order (`two_sided`: C++ then
/// Rust; otherwise Rust alone, which compares nothing and is `RUST-ONLY`, not `MATCH`).
fn verdict(results: &[RunResult], two_sided: bool) -> (String, Value) {
    let (cpp, rust) = match (results.first(), results.get(1)) {
        (Some(c), Some(r)) if two_sided => (Some(c), r),
        (Some(r), _) => (None, r),
        _ => return ("CRASH".into(), json!("no result")),
    };
    if let Some(Err(e)) = cpp {
        return ("CRASH".into(), json!({"side": "cpp", "detail": e}));
    }
    let rust = match rust {
        Err(e) => return ("CRASH".into(), json!({"side": "rust", "detail": e})),
        Ok(r) => r,
    };
    let diff = cpp
        .and_then(|c| c.as_ref().ok())
        .and_then(|c| first_difference(c, rust));
    if let Some((stage, i, a, b)) = diff {
        let v = if stage.starts_with("heal") {
            "HEAL-DIFF"
        } else {
            "DIFF"
        };
        return (
            v.into(),
            json!({"stage": stage, "index": i, "cpp": a, "rust": b}),
        );
    }
    if let Some((u, s)) = &rust.deferred {
        return (
            "DEFERRED".into(),
            json!({"unit": u, "symbol": s, "after": rust.stages.last().map(|(n, _)| n)}),
        );
    }
    if cpp.is_none() {
        return ("RUST-ONLY".into(), Value::Null);
    }
    ("MATCH".into(), Value::Null)
}

/// Runs the corpus (see the module docs).
pub fn run_corpus(root: &Path, opts: &CorpusOptions) -> ExitCode {
    match run_corpus_inner(root, opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("difftest: {e}");
            ExitCode::from(2)
        }
    }
}

fn run_corpus_inner(root: &Path, opts: &CorpusOptions) -> Result<(), HarnessError> {
    let dir = root.join(CORPUS_DIR);
    let mut files = Vec::new();
    step_files(&dir, &mut files);
    if files.is_empty() {
        return Err(HarnessError::new(format!(
            "no STEP files in {}; run scripts/fetch-steptools-corpus.sh",
            dir.display()
        )));
    }
    files.retain(|f| {
        let rel = f
            .strip_prefix(&dir)
            .unwrap_or(f)
            .to_string_lossy()
            .into_owned();
        opts.glob.as_deref().is_none_or(|g| glob_match(g, &rel))
    });
    // smallest first
    files.sort_by_key(|f| (std::fs::metadata(f).map_or(0, |m| m.len()), f.clone()));
    let report_dir = opts
        .report_dir
        .clone()
        .unwrap_or_else(|| root.join("bin/step-corpus"));
    let raw = report_dir.join("raw");
    std::fs::create_dir_all(&raw)
        .map_err(|e| HarnessError::new(format!("{}: {e}", raw.display())))?;

    // import in the parent: parse status, skipped bodies, the work list
    let mut file_entries: Vec<Value> = Vec::new();
    let mut work: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut work_file: Vec<usize> = Vec::new();
    let mut unsupported: BTreeMap<String, usize> = BTreeMap::new();
    for (fi, f) in files.iter().enumerate() {
        let rel = f
            .strip_prefix(&dir)
            .unwrap_or(f)
            .to_string_lossy()
            .into_owned();
        let bytes = std::fs::read(f).map_err(|e| HarnessError::new(format!("{rel}: {e}")))?;
        match step_import::import(&bytes, &step_import::Limits::default()) {
            Err(e) => file_entries.push(json!({"file": rel, "parse": e.to_string(), "bodies": []})),
            Ok(m) => {
                for s in &m.skipped {
                    *unsupported.entry(s.reason.clone()).or_default() += 1;
                }
                let bodies: Vec<Value> = m
                    .bodies
                    .iter()
                    .enumerate()
                    .map(|(bi, b)| {
                        let id = format!("{}-{bi}", rel.replace(['/', '.'], "_"));
                        work.push((f.clone(), bi, id));
                        work_file.push(fi);
                        json!({
                            "step_id": b.step_id,
                            "kind": b.kind.step_name(),
                            "unit_mm": b.unit_mm,
                            "uncertainty_mm": b.uncertainty_mm,
                            "source": {"shells": b.shells.len(), "faces": b.faces.len(),
                                       "edges": b.edges.len(), "vertices": b.vertices.len()},
                        })
                    })
                    .collect();
                let skipped: Vec<Value> = m
                    .skipped
                    .iter()
                    .map(|s| json!({"step_id": s.step_id, "kind": s.kind, "reason": s.reason}))
                    .collect();
                file_entries.push(json!({
                    "file": rel,
                    "schemas": m.schemas,
                    "bodies": bodies,
                    "skipped": skipped,
                    "assembly_instances": m.assembly_instances,
                }));
            }
        }
    }

    let results = run_children(&work, &raw, opts.jobs)?;
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut deferred_units: BTreeMap<String, usize> = BTreeMap::new();
    let mut diff_stages: BTreeMap<String, usize> = BTreeMap::new();
    let mut body_iter = 0usize;
    println!(
        "{:<36} {:>5}  {:<9} {:<22} {:<22} detail",
        "file", "body", "verdict", "cpp faces/solid", "rust faces/solid"
    );
    for (fi, entry) in file_entries.iter_mut().enumerate() {
        let rel = entry["file"].as_str().unwrap_or("?").to_string();
        if let Some(p) = entry.get("parse").and_then(Value::as_str) {
            println!("{rel:<36} {:>5}  {:<9} parse error: {p}", "-", "PARSE");
            *tally.entry("PARSE".into()).or_default() += 1;
            continue;
        }
        let n = entry["bodies"].as_array().map_or(0, Vec::len);
        if n == 0 {
            let why = entry["skipped"]
                .as_array()
                .and_then(|s| s.first())
                .and_then(|s| s["reason"].as_str())
                .unwrap_or("no B-rep body");
            println!("{rel:<36} {:>5}  {:<9} {why}", "-", "SKIPPED");
            *tally.entry("SKIPPED".into()).or_default() += 1;
        }
        for bi in 0..n {
            let wi = body_iter;
            body_iter += 1;
            if work_file.get(wi) != Some(&fi) {
                continue;
            }
            let res = results.get(wi).map_or(&[][..], Vec::as_slice);
            let (v, detail) = verdict(res, sides().len() == 2);
            *tally.entry(v.clone()).or_default() += 1;
            if v == "DEFERRED" {
                if let Some(u) = detail.get("unit").and_then(Value::as_str) {
                    *deferred_units.entry(u.to_string()).or_default() += 1;
                }
            }
            if v == "DIFF" || v == "HEAL-DIFF" {
                if let Some(s) = detail.get("stage").and_then(Value::as_str) {
                    *diff_stages.entry(s.to_string()).or_default() += 1;
                }
            }
            let short = |r: Option<&RunResult>| -> String {
                match r {
                    None => "-".into(),
                    Some(Err(_)) => "crash".into(),
                    Some(Ok(rec)) => {
                        let f = rec.stage("faces").unwrap_or(&[]);
                        let made = f.get(1).copied().unwrap_or(0.0);
                        let tot = f.first().copied().unwrap_or(0.0);
                        let solid = rec
                            .stage("stitch")
                            .and_then(|s| s.get(1))
                            .is_some_and(|&x| x == 1.0);
                        format!("{made}/{tot} {}", if solid { "solid" } else { "-" })
                    }
                }
            };
            let (c, r) = if sides().len() == 2 {
                (res.first(), res.get(1))
            } else {
                (None, res.first())
            };
            println!(
                "{rel:<36} {bi:>5}  {v:<9} {:<22} {:<22} {}",
                short(c),
                short(r),
                if detail.is_null() {
                    String::new()
                } else {
                    detail.to_string()
                }
            );
            if let Some(b) = entry["bodies"].get_mut(bi) {
                b["verdict"] = json!(v);
                b["detail"] = detail;
                for (si, side) in sides().iter().enumerate() {
                    b[*side] = side_summary(res.get(si).unwrap_or(&Err("not run".into())));
                }
            }
        }
    }
    let summary: Vec<String> = tally.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!(
        "STEP-CORPUS files={} bodies={} {}",
        files.len(),
        work.len(),
        summary.join(" ")
    );
    if !deferred_units.is_empty() {
        println!("deferred on: {deferred_units:?}");
    }
    if !diff_stages.is_empty() {
        println!("first differing stage: {diff_stages:?}");
    }
    if !unsupported.is_empty() {
        println!("unsupported (bodies skipped by the importer): {unsupported:?}");
    }
    if !opts.no_report {
        let report = json!({
            "corpus": CORPUS_DIR,
            "accuracy": ACCURACY,
            "sides": sides(),
            "tally": tally,
            "deferred_units": deferred_units,
            "diff_stages": diff_stages,
            "unsupported": unsupported,
            "files": file_entries,
        });
        let path = report_dir.join("report.json");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&report).unwrap_or_default(),
        )
        .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
        println!("report: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(volume: f64) -> Record {
        Record {
            stages: vec![
                ("faces".into(), vec![1.0, 1.0, 0.0, 0.0, 1.0]),
                ("volume".into(), vec![0.0, volume]),
                ("heal".into(), vec![0.0]),
                ("healed_volume".into(), vec![0.0, volume]),
            ],
            ..Record::default()
        }
    }

    fn deferred() -> Record {
        Record {
            deferred: Some(("unit.x".into(), "SmX::Y".into())),
            ..built(1.0)
        }
    }

    #[test]
    fn a_rust_only_build_is_not_a_match() {
        // Without `cpp` there is no C++ side: nothing was compared.
        assert_eq!(verdict(&[Ok(built(1.0))], false).0, "RUST-ONLY");
        assert_eq!(verdict(&[Ok(deferred())], false).0, "DEFERRED");
        assert_eq!(verdict(&[Err("killed".into())], false).0, "CRASH");
        assert_eq!(verdict(&[], false).0, "CRASH");
        assert!(VERDICTS.contains(&"RUST-ONLY"));
    }

    #[test]
    fn two_sided_verdicts_are_unchanged() {
        let v = |c: RunResult, r: RunResult| verdict(&[c, r], true).0;
        assert_eq!(v(Ok(built(1.0)), Ok(built(1.0))), "MATCH");
        assert_eq!(v(Ok(built(1.0)), Ok(built(2.0))), "DIFF");
        let mut healed = built(1.0);
        healed.stages[3].1[1] = 2.0;
        assert_eq!(v(Ok(built(1.0)), Ok(healed)), "HEAL-DIFF");
        assert_eq!(v(Ok(built(1.0)), Ok(deferred())), "DEFERRED");
        assert_eq!(v(Err("timeout".into()), Ok(built(1.0))), "CRASH");
        assert_eq!(v(Ok(built(1.0)), Err("timeout".into())), "CRASH");
    }
}
