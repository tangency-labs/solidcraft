//! Differential coverage for `topology.api_projection`: `SmApiProjectBrepOntoPlane`,
//! `SmApiCreateSilhouetteCurves` and both `SmApiProjectCurve`s (and through them the
//! file-static `SmApiAddCurveOutputSlots`, unit `topology.api_files`).
//!
//! Each side builds its own inputs: an upstream `.smb` B-rep (the native
//! `SmApiReadBrepFromFile`, the Rust reader `own_reader::read_file`) or a box
//! (`SmApiCreateBox` / `create_box`), optionally filleted on every edge
//! (`SmApiCircularFillet` / `circular_fillet`) and moved (`Transform` / `transform`); a
//! curve from control points (`SmApiCreateCurve` / `create_curve`); a bilinear surface
//! from four corners (`CreateBilinearSurface`). No C++-built geometry reaches the Rust
//! side. The output array may start with null entries (`prefill`), as upstream's tests
//! append to arrays holding curves. Compared: the status (`status`) and the record
//! (`record`, layout in `solidcore_ffi::units::topology_api_projection`). The native
//! side runs in a fresh child process; inputs its build or the FFI guard refuse are
//! `rejected`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::surface::surface::Surface;
use solidcore::topology::api_projection::{
    create_silhouette_curves, project_brep_onto_plane, project_curve_onto_brep,
    project_curve_onto_surface, SurfaceProjection,
};
use solidcore::topology::Brep;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::topology_api_projection::{self as ffi, NFIXED};
use solidcore_ffi::FfiError;

use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The runner of the `SM_API` projection wrappers.
pub struct Runner;

/// Upstream B-rep files (relative to `TestFiles/pt_TestFiles`) the generated cases use.
const FILES: [&str; 8] = [
    "Solids/box.smb",
    "Solids/cylinder.smb",
    "Solids/sphere.smb",
    "Solids/nurb_fillet.smb",
    "cone.smb",
    "torus.smb",
    "box_with_large_fillet.smb",
    "disk.smb",
];

/// The B-rep of ops 1-3.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Body {
    /// A null B-rep.
    Null,
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    File { file: String },
    /// `SmApiCreateBox(0, size)`, then `SmApiCircularFillet(fillet)` when `fillet > 0`.
    Box { size: [f64; 3], fillet: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Op {
    /// `SmApiProjectBrepOntoPlane(body, point, normal, out)`.
    ProjectBrep,
    /// `SmApiCreateSilhouetteCurves(body, point, normal, out)`.
    Silhouette,
    /// `SmApiProjectCurve(body, curve, normal, out)`.
    ProjectCurveBrep,
    /// `SmApiProjectCurve(curve, surface, want_2d, want_3d)`.
    ProjectCurveSurface,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Inputs {
    op: Op,
    body: Body,
    /// Move the body first: `Transform` of an `SmAxis2Placement` translated by this.
    translate: Option<[f64; 3]>,
    point: [f64; 3],
    /// The plane normal, or the projection vector of op 3.
    normal: [f64; 3],
    /// Null entries already in the output array.
    prefill: u8,
    /// The control points of the curve (ops 3 and 4).
    points: Vec<[f64; 3]>,
    /// The surface corners `u0v0 u1v0 u0v1 u1v1` (op 4).
    corners: [[f64; 3]; 4],
    want_2d: bool,
    want_3d: bool,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            op: Op::ProjectBrep,
            body: Body::Box {
                size: [10.0, 10.0, 10.0],
                fillet: 0.0,
            },
            translate: None,
            point: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            prefill: 0,
            points: Vec::new(),
            corners: [
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [0.0, 10.0, 0.0],
                [10.0, 10.0, 0.0],
            ],
            want_2d: true,
            want_3d: true,
        }
    }
}

fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn path(i: &Inputs) -> Result<String, HarnessError> {
    match &i.body {
        Body::File { file } if i.op != Op::ProjectCurveSurface => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

fn args(i: &Inputs) -> Vec<f64> {
    let (kind, size, fillet) = match &i.body {
        Body::Null => (0.0, [0.0; 3], 0.0),
        Body::File { .. } => (1.0, [0.0; 3], 0.0),
        Body::Box { size, fillet } => (2.0, *size, *fillet),
    };
    let t = i.translate.unwrap_or_default();
    let mut a = vec![
        kind,
        size[0],
        size[1],
        size[2],
        fillet,
        flag(i.translate.is_some()),
        t[0],
        t[1],
        t[2],
        i.point[0],
        i.point[1],
        i.point[2],
        i.normal[0],
        i.normal[1],
        i.normal[2],
        f64::from(i.prefill),
        flag(i.want_2d),
        flag(i.want_3d),
    ];
    for c in &i.corners {
        a.extend(c);
    }
    a.push(i.points.len() as f64);
    for p in &i.points {
        a.extend(p);
    }
    debug_assert_eq!(a.len(), NFIXED + 3 * i.points.len());
    a
}

fn op_code(op: Op) -> u8 {
    match op {
        Op::ProjectBrep => 1,
        Op::Silhouette => 2,
        Op::ProjectCurveBrep => 3,
        Op::ProjectCurveSurface => 4,
    }
}

fn output(raw: &[f64]) -> Result<Output, HarnessError> {
    let (&st, record) = raw.split_first().ok_or_else(|| herr("empty record"))?;
    Ok(Output::new()
        .scalar("status", st)
        .values("record", record.to_vec()))
}

/// The status code of a wrapper error (`SM_SUCCESS` is `0`).
fn status(e: &SolidError) -> Option<f64> {
    match e {
        SolidError::Upstream { code, .. } => Some(f64::from(*code)),
        SolidError::InvalidInput { .. } => Some(1007.),
        _ => None,
    }
}

/// The shim's `put_curve`.
fn put_curve(k: &Kernel, out: &mut Vec<f64>, c: Option<&Curve>) {
    let Some(c) = c else {
        out.push(-1.);
        return;
    };
    out.push(c.type_code() as f64);
    let d = c.natural_interval();
    out.extend([d.min(), d.max()]);
    for i in 0..5 {
        match c.evaluate_point(k, d.evaluate(f64::from(i) / 4.0)) {
            Ok(p) => out.extend([0., p.x, p.y, p.z]),
            Err(_) => out.push(1.),
        }
    }
}

/// `create_box(0, size)`, then `circular_fillet(fillet)` when `fillet > 0`.
fn build_box(k: &mut Kernel, size: [f64; 3], fillet: f64) -> SolidResult<Brep> {
    let mut b =
        solidcore::primitive::api_primitives::create_box(k, Vec3::ZERO, size[0], size[1], size[2])?;
    if fillet > 0.0 {
        solidcore::blend::fillet_executive::api::circular_fillet(k, &mut b, fillet)?;
    }
    Ok(b)
}

/// The Rust body of `i` (`None` for a null B-rep).
fn build(k: &mut Kernel, i: &Inputs) -> Result<SolidResult<Option<Brep>>, HarnessError> {
    let built = match &i.body {
        Body::Null => return Ok(Ok(None)),
        Body::File { file } => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            crate::own_reader::read_file(k, &p.to_string_lossy())?
        }
        Body::Box { size, fillet } => build_box(k, *size, *fillet),
    };
    let mut b = match built {
        Ok(b) => b,
        Err(e) => return Ok(Err(e)),
    };
    if let Some(t) = i.translate {
        let mut placement = solidcore::geom::axis2_placement::Axis2Placement::default();
        placement.translate(v(t));
        if let Err(e) = solidcore::topology::brep::p5::transform(k, &mut b, &placement, None, true)
        {
            return Ok(Err(e));
        }
    }
    Ok(Ok(Some(b)))
}

/// The Rust record of `i` after the status, and the status.
fn rust_record(k: &mut Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let curve = if matches!(i.op, Op::ProjectCurveBrep | Op::ProjectCurveSurface) {
        let pts: Vec<Vec3> = i.points.iter().map(|&p| v(p)).collect();
        match solidcore::primitive::api_curves::create_curve(&pts) {
            Ok(c) => Some(c),
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust curve: {e}")));
            }
        }
    } else {
        None
    };

    if i.op == Op::ProjectCurveSurface {
        let corners = i.corners.map(v);
        let surface = match BSplineSurface::create_bilinear_surface(
            corners[0], corners[1], corners[2], corners[3],
        ) {
            Ok(s) => Surface::from(s),
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| herr(format!("rust surface: {e}")));
            }
        };
        let mut c2 = Vec::new();
        let mut c3 = Vec::new();
        let r = project_curve_onto_surface(
            k,
            curve.as_ref(),
            Some(&surface),
            SurfaceProjection {
                curves_2d: i.want_2d.then_some(&mut c2),
                curves_3d: i.want_3d.then_some(&mut c3),
            },
        );
        let st = match &r {
            Ok(()) => 0.0,
            Err(e) => match (Output::unported(e), status(e)) {
                (Some(o), _) => return Ok(o),
                (None, Some(code)) => code,
                (None, None) => return Err(herr(format!("rust: {e}"))),
            },
        };
        let mut o = vec![st];
        for (want, curves) in [(i.want_2d, &c2), (i.want_3d, &c3)] {
            if want {
                o.push(curves.len() as f64);
                for c in curves {
                    put_curve(k, &mut o, Some(c));
                }
            } else {
                o.push(-1.0);
            }
        }
        return output(&o);
    }

    // `SmApiCreateSilhouetteCurves` dereferences a null B-rep once its normal unitizes:
    // the FFI guard refuses that input, and so does this side (the port's InvalidInput).
    if i.op == Op::Silhouette && matches!(i.body, Body::Null) && i.normal != [0.0; 3] {
        return Ok(Output::rejected(
            "null B-rep dereferenced upstream (SmApiCreateSilhouetteCurves)",
        ));
    }
    // nor a normal that unitizes to a zero vector, which ops 1 and 2 go on with
    // through uninitialised vectors upstream (UB-169)
    if matches!(i.op, Op::ProjectBrep | Op::Silhouette) && ffi::normal_overflows(i.normal) {
        return Ok(Output::rejected(format!(
            "ffi guard: invalid argument: {}",
            ffi::NORMAL_OVERFLOWS
        )));
    }
    let mut body = match build(k, i)? {
        Ok(b) => b,
        Err(e) => {
            return Output::unported(&e).ok_or_else(|| herr(format!("rust body build: {e}")));
        }
    };
    let prefill = usize::from(i.prefill);
    let mut out: Vec<Option<Curve>> = (0..prefill).map(|_| None).collect();
    let (point, normal) = (v(i.point), v(i.normal));
    let r = match i.op {
        Op::ProjectBrep => project_brep_onto_plane(k, body.as_ref(), point, normal, &mut out),
        Op::Silhouette => create_silhouette_curves(k, body.as_ref(), point, normal, &mut out),
        _ => project_curve_onto_brep(k, body.as_mut(), curve.as_ref(), normal, &mut out),
    };
    let st = match &r {
        Ok(()) => 0.0,
        Err(e) => match (Output::unported(e), status(e)) {
            (Some(o), _) => return Ok(o),
            (None, Some(code)) => code,
            (None, None) => return Err(herr(format!("rust: {e}"))),
        },
    };
    let mut o = vec![st, out.len() as f64];
    for c in out.iter().skip(prefill) {
        put_curve(k, &mut o, c.as_ref());
    }
    output(&o)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.api_projection"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match ffi::eval(op_code(i.op), &path(&i)?, &args(&i)) {
            Ok(raw) if raw.first() == Some(&-999.) => Ok(Output::rejected(format!(
                "native build status {}",
                raw.get(1).copied().unwrap_or(-1.)
            ))),
            Ok(raw) => output(&raw),
            Err(e @ FfiError::InvalidArgument(_)) => {
                Ok(Output::rejected(format!("ffi guard: {e}")))
            }
            Err(e) => Err(herr(e)),
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        rust_record(&mut k, &i)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = 10f64.powi(rng.range_i64(-1, 1) as i32);
        let op = match rng.below(4) {
            0 => Op::ProjectBrep,
            1 => Op::Silhouette,
            2 => Op::ProjectCurveBrep,
            _ => Op::ProjectCurveSurface,
        };
        let size = [
            rng.uniform(2.0, 20.0) * scale,
            rng.uniform(2.0, 20.0) * scale,
            rng.uniform(2.0, 20.0) * scale,
        ];
        let smallest = size[0].min(size[1]).min(size[2]);
        let body = if rng.chance(0.3) {
            Body::File {
                file: (*rng.choose(&FILES).unwrap_or(&"disk.smb")).to_string(),
            }
        } else {
            Body::Box {
                size,
                fillet: if rng.chance(0.3) {
                    rng.uniform(0.05, 0.2) * smallest
                } else {
                    0.0
                },
            }
        };
        let mut normal = match rng.below(4) {
            0 => [0.0, 0.0, 1.0],
            1 => [1.0, 0.0, 0.0],
            2 => [0.0, 0.0, rng.uniform(0.5, 5.0)],
            _ => [
                rng.uniform(-1.0, 1.0),
                rng.uniform(-1.0, 1.0),
                rng.uniform(-1.0, 1.0),
            ],
        };
        let point = rng.point3(-20.0 * scale, 20.0 * scale);
        let prefill = rng.below(3) as u8;
        let mut points = Vec::new();
        let (mut want_2d, mut want_3d) = (true, true);
        if matches!(op, Op::ProjectCurveBrep | Op::ProjectCurveSurface) {
            let n = 4 + rng.below(4) as usize;
            let reach = 20.0 * scale;
            for j in 0..n {
                let x = -0.5 * reach + reach * j as f64 / (n - 1) as f64;
                let y = rng.uniform(-0.3, 0.6) * reach;
                let z = if op == Op::ProjectCurveBrep {
                    rng.uniform(1.2, 2.0) * reach
                } else {
                    rng.uniform(-1.0, 3.0)
                };
                points.push([x, y, z]);
            }
            if op == Op::ProjectCurveBrep {
                normal = [0.0, 0.0, -1.0];
            } else {
                want_2d = rng.chance(0.7);
                want_3d = rng.chance(0.7);
            }
        }
        let i = Inputs {
            op,
            body,
            point,
            normal,
            prefill,
            points,
            want_2d,
            want_3d,
            ..Inputs::default()
        };
        serde_json::to_value(i).ok()
    }
}
