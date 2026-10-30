//! Differential coverage for `topology.brep_planar_section`
//! (`SmBrep::CreatePlanarSectionCurves`, `SmApiIntersectBrepWithPlane`).
//!
//! Each side builds the body itself: an upstream `.smb` solid (the native
//! `SmBrep::ReadFromFile`, the Rust reader `own_reader::read_file`) or a box
//! (`SmApiCreateBox` / `create_box`), optionally filleted on every edge
//! (`SmApiCircularFillet` / `circular_fillet`). No C++-built geometry reaches the Rust
//! side. A case may move the body first (`Transform` / `transform`) and may take the
//! plane point from the B-rep's bounding box (`CalculateBoundingBox`, as `prog_test`'s
//! `my_test_tsurf_section` walks it). The case then sections the body by a plane, with
//! `CreatePlanarSectionCurves` (its tolerances, outputs and `bForceUVClassify` from the
//! case) or with `SmApiIntersectBrepWithPlane`, and compares the status (`status`) and
//! the record (`record`): the count and samples of every 3D and UV section curve (type
//! code, natural interval, five evaluated points) and the `GetFaces` index of each
//! face. The native side runs in a fresh child process. Inputs the native build or the
//! FFI guard refuse are `rejected`, not failed.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::intersect::api_intersectors::intersect_brep_with_plane;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::topology::brep::p1 as brep1;
use solidcore::topology::brep::planar_section::{
    create_planar_section_curves, PlanarSection, PlanarSectionOptions, SectionOutputs,
};
use solidcore::topology::Brep;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::topology_brep_planar_section::{self as ffi, NARGS};
use solidcore_ffi::FfiError;

use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The runner of `CreatePlanarSectionCurves` and `SmApiIntersectBrepWithPlane`.
pub struct Runner;

/// Upstream solids (relative to `TestFiles/pt_TestFiles`) the generated cases section.
const FILES: [&str; 10] = [
    "Solids/box.smb",
    "Solids/cylinder.smb",
    "Solids/sphere.smb",
    "Solids/pyramid.smb",
    "Solids/revolution.smb",
    "Solids/extrusion.smb",
    "Solids/l_shape_with_boss.smb",
    "Solids/nurb_fillet.smb",
    "cone.smb",
    "box_with_large_fillet.smb",
];

/// The sectioned body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Body {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    File { file: String },
    /// `SmApiCreateBox(0, size)`, then `SmApiCircularFillet(fillet)` when `fillet > 0`.
    Box { size: [f64; 3], fillet: f64 },
    /// `SmApiCreateBox(0, size)` with a strut in its top face: `MakeVertexLoop` at `strut`
    /// (on the top face), then `MakeEdgeInFace` from the face's first vertex to it along a
    /// line, so the face has an embedded edge.
    BoxStrut { size: [f64; 3], strut: [f64; 3] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // independent case switches, as the JSON fixtures spell them
struct Inputs {
    body: Body,
    /// Use `SmApiIntersectBrepWithPlane` instead of `CreatePlanarSectionCurves`.
    api: bool,
    /// Section every face's surface (`SmSurface::CreatePlanarSectionCurves` over the face's
    /// UV domain) instead.
    faces: bool,
    /// Run, on every face, the forced-UV continuation's callees (`DropAndTrimCurve` and
    /// `LiftCurve`) for each section curve without UV curve instead.
    forced: bool,
    point: [f64; 3],
    normal: [f64; 3],
    /// `cpdOpt3DApproximationTol`.
    tol: Option<f64>,
    /// `cpdOptAngleTolRadians`.
    ang_tol: Option<f64>,
    want_3d: bool,
    want_uv: bool,
    want_faces: bool,
    force_uv_classify: bool,
    /// Move the body first: `Transform` of an `SmAxis2Placement` translated by this.
    translate: Option<[f64; 3]>,
    /// `[n, i]`: the plane point is the B-rep box's `Evaluate(t, t, t)`,
    /// `t = i / (n - 1)` (`prog_test` `my_test_tsurf_section`'s walk), not `point`.
    plane_walk: Option<[u32; 2]>,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            body: Body::Box {
                size: [10.0, 10.0, 10.0],
                fillet: 0.0,
            },
            api: false,
            faces: false,
            forced: false,
            point: [5.0, 5.0, 5.0],
            normal: [1.0, 0.0, 0.0],
            tol: None,
            ang_tol: None,
            want_3d: true,
            want_uv: false,
            want_faces: false,
            force_uv_classify: false,
            translate: None,
            plane_walk: None,
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
        Body::File { file } => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        Body::Box { .. } | Body::BoxStrut { .. } => Ok(String::new()),
    }
}

fn args(i: &Inputs) -> [f64; NARGS] {
    let (kind, size, fillet, strut) = match &i.body {
        Body::File { .. } => (0.0, [0.0; 3], 0.0, [0.0; 3]),
        Body::Box { size, fillet } => (1.0, *size, *fillet, [0.0; 3]),
        Body::BoxStrut { size, strut } => (2.0, *size, 0.0, *strut),
    };
    [
        kind,
        size[0],
        size[1],
        size[2],
        fillet,
        i.point[0],
        i.point[1],
        i.point[2],
        i.normal[0],
        i.normal[1],
        i.normal[2],
        flag(i.tol.is_some()),
        i.tol.unwrap_or(0.0),
        flag(i.ang_tol.is_some()),
        i.ang_tol.unwrap_or(0.0),
        flag(i.want_3d),
        flag(i.want_uv),
        flag(i.want_faces),
        flag(i.force_uv_classify),
        flag(i.translate.is_some()),
        i.translate.unwrap_or_default()[0],
        i.translate.unwrap_or_default()[1],
        i.translate.unwrap_or_default()[2],
        f64::from(i.plane_walk.map_or(0, |w| w[0])),
        f64::from(i.plane_walk.map_or(0, |w| w[1])),
        strut[0],
        strut[1],
        strut[2],
    ]
}

/// The native record, or `Err(output)` when the FFI guard or the body build refuses.
fn native(i: &Inputs) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    let op = if i.forced {
        4
    } else if i.faces {
        3
    } else if i.api {
        2
    } else {
        1
    };
    match ffi::eval(op, &path(i)?, &args(i)) {
        Ok(raw) if raw.first() == Some(&-999.) => Ok(Err(Output::rejected(format!(
            "body build status {}",
            raw.get(1).copied().unwrap_or(-1.)
        )))),
        Ok(raw) => Ok(Ok(raw)),
        Err(e @ FfiError::InvalidArgument(_)) => {
            Ok(Err(Output::rejected(format!("ffi guard: {e}"))))
        }
        Err(e) => Err(herr(e)),
    }
}

/// The status code of a section call (`SM_SUCCESS` is `0`).
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

/// The op 1 record after the status (the shim's layout).
fn section_record(k: &Kernel, b: &Brep, s: &PlanarSection) -> SolidResult<Vec<f64>> {
    let mut out = Vec::new();
    match &s.curves_3d {
        Some(curves) => {
            out.push(curves.len() as f64);
            for c in curves {
                put_curve(k, &mut out, Some(c));
            }
        }
        None => out.push(-1.),
    }
    match &s.uv_curves {
        Some(curves) => {
            out.push(curves.len() as f64);
            for c in curves {
                put_curve(k, &mut out, c.as_ref());
            }
        }
        None => out.push(-1.),
    }
    match &s.faces {
        Some(faces) => {
            let all = brep1::faces(b, None)?;
            out.push(faces.len() as f64);
            for f in faces {
                out.push(all.iter().position(|g| g == f).map_or(-1., |at| at as f64));
            }
        }
        None => out.push(-1.),
    }
    Ok(out)
}

/// The Rust body of `i`.
fn build(k: &mut Kernel, i: &Inputs) -> Result<SolidResult<Brep>, HarnessError> {
    match &i.body {
        Body::File { .. } => crate::own_reader::read_file(k, &path(i)?),
        Body::BoxStrut { size, strut } => Ok(box_strut(k, *size, *strut)),
        Body::Box { size, fillet } => Ok((|| {
            let mut b = solidcore::primitive::api_primitives::create_box(
                k,
                Vec3::ZERO,
                size[0],
                size[1],
                size[2],
            )?;
            if *fillet > 0.0 {
                solidcore::blend::fillet_executive::api::circular_fillet(k, &mut b, *fillet)?;
            }
            Ok(b)
        })()),
    }
}

/// The shim's strut box: `create_box`, then on the top face (all vertices at `z =
/// size[2]`) `make_vertex_loop` at `strut` and `make_edge_in_face` from the face's first
/// vertex to it along `BSplineCurve::create_line_segment`, editing enabled for the edit.
fn box_strut(k: &mut Kernel, size: [f64; 3], strut: [f64; 3]) -> SolidResult<Brep> {
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::face::p4 as face4;
    let mut b =
        solidcore::primitive::api_primitives::create_box(k, Vec3::ZERO, size[0], size[1], size[2])?;
    let null = || SolidError::null_pointer("difftest::topology_brep_planar_section::box_strut");
    let mut top = None;
    for f in brep1::faces(&b, None)? {
        let vs = face4::vertices(&b, f, None, false, None)?;
        let mut all = !vs.is_empty();
        for &v in &vs {
            if b.vertex(v)?.point.z != size[2] {
                all = false;
            }
        }
        if all {
            top = Some(f);
            break;
        }
    }
    let top = top.ok_or_else(null)?;
    let start = *face4::vertices(&b, top, None, false, None)?
        .first()
        .ok_or_else(null)?;
    let saved = b.editing_enabled();
    b.set_editing_enabled(true);
    let point = v(strut);
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attributes = solidcore::geom::attribute::AttributeStore::default();
    let mut env = solidcore::topology::edge::NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &crate::own_reader::read_key_of,
    };
    let (_, vertex) = solidcore::topology::brep::p9::make_vertex_loop(
        &mut b,
        top,
        point,
        OrientType::Opposite,
        &mut env,
    )?;
    let curve = Curve::from(BSplineCurve::create_line_segment(3, b.vertex(start)?.point, point)?);
    let interval = curve.natural_interval();
    let mut out = solidcore::topology::brep::p5::EdgeInFace::default();
    solidcore::topology::brep::p5::make_edge_in_face(
        k,
        &mut b,
        true,
        Some(top),
        Some(start),
        Some(vertex),
        curve,
        None,
        interval,
        OrientType::Same,
        0.0,
        &mut out,
        None,
    )?;
    b.set_editing_enabled(saved);
    out.edge.ok_or_else(null)?;
    Ok(b)
}

/// The shim's `put_nurb`: the NURB of a B-spline kind (B-spline, line or conic) curve, `n (x y z w)* nknots
/// knot* mult*` (unique knots), `-1` for another kind or no NURB.
fn put_nurb(out: &mut Vec<f64>, c: &Curve) {
    use solidcore::curve::curve::CurveKind;
    let base = match c.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base()),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        _ => None,
    };
    let Some((points, (knots, mults))) =
        base.and_then(|b| Some((b.control_points().ok()?, b.knots(None).ok()?)))
    else {
        out.push(-1.);
        return;
    };
    out.push(points.len() as f64);
    for p in points {
        out.extend([p.x, p.y, p.z, p.w]);
    }
    out.push(knots.len() as f64);
    out.extend(&knots);
    out.extend(mults.iter().map(|&m| m as f64));
}

/// The op 4 record: on every face, for each section curve without UV curve (not
/// degenerate), `DropAndTrimCurve` onto the face's surface and `LiftCurve` of each drop,
/// as `CreatePlanarSectionCurves`' forced-UV continuation calls them.
fn forced_record(
    k: &Kernel,
    b: &mut Brep,
    point: Vec3,
    normal: Vec3,
    tol: Option<f64>,
    ang_tol: Option<f64>,
) -> SolidResult<Vec<f64>> {
    use solidcore::intersect::surface_drop_lib::p3::DropSolveContext;
    use solidcore::math::EFF_ZERO;
    use solidcore::query::brep::BrepFaceServices;
    use solidcore::surface::surface::p1::SurfaceSolveContext;
    use solidcore::surface::surface::p4::NoObject;
    use solidcore::topology::brep::planar_section::face_planar_section_curves;
    use solidcore::topology::brep_cache::CacheObject;
    use solidcore::topology::face::p2 as face2;
    let code = |e: SolidError| -> SolidResult<f64> {
        if e.is_unported() {
            return Err(e);
        }
        status(&e).ok_or(e)
    };
    let faces = brep1::faces(b, None)?;
    let mut out = vec![faces.len() as f64];
    for f in faces {
        let uv_domain = face2::uv_domain(b, f)?;
        let surface = face2::surface(b, f)?
            .ok_or_else(|| SolidError::null_pointer("difftest::topology_brep_planar_section"))?
            .clone_for_evaluation()?;
        let face_tol = face2::tolerance(b, f)?;
        let section = match face_planar_section_curves(
            k, b, f, &surface, &uv_domain, point, normal, tol, ang_tol,
        ) {
            Ok(section) => {
                out.push(0.);
                section
            }
            Err(e) => {
                out.extend([code(e)?, 0.]);
                continue;
            }
        };
        let mut forced = Vec::new();
        let mut count = 0usize;
        for (j, c) in section.curves_3d.iter().enumerate() {
            if section.uv_curves.get(j).is_some_and(Option::is_some)
                || c.is_degenerate(k, EFF_ZERO, None)?
            {
                continue;
            }
            count += 1;
            put_nurb(&mut forced, c);
            let handle = Some(CacheObject::Surface(f));
            let mut services = BrepFaceServices::new(&mut *b);
            let mut cache = services.build_surface_cache(k, &surface, handle)?;
            let drops = match surface.drop_and_trim_curve_in_context::<NoObject, CacheObject>(
                k,
                Some(&mut cache),
                &uv_domain,
                c,
                &c.natural_interval(),
                face_tol / 3.0,
                true,
                true,
                None,
                Some(DropSolveContext {
                    solver: &mut services,
                    handle,
                }),
            ) {
                Ok(d) => {
                    forced.push(0.);
                    d.uv_curves
                }
                Err(e) => {
                    forced.extend([code(e)?, 0.]);
                    continue;
                }
            };
            forced.push(drops.len() as f64);
            for drop in &drops {
                put_curve(k, &mut forced, Some(drop));
                put_nurb(&mut forced, drop);
                match surface.lift_curve_with_context(
                    k,
                    &uv_domain,
                    drop,
                    &drop.natural_interval(),
                    face_tol / 3.0,
                    true,
                    &mut services,
                    handle,
                ) {
                    Ok(lifted) => {
                        forced.extend([0., lifted.max_approx_dev]);
                        put_curve(k, &mut forced, Some(&lifted.curve));
                    }
                    Err(e) => {
                        forced.extend([code(e)?, 0.]);
                        put_curve(k, &mut forced, None);
                    }
                }
            }
        }
        out.push(count as f64);
        out.extend(forced);
    }
    Ok(out)
}

/// The op 3 record: every face surface's section (`status n3d curve* nuv curve*`).
fn faces_record(
    k: &Kernel,
    b: &mut Brep,
    point: Vec3,
    normal: Vec3,
    tol: Option<f64>,
    ang_tol: Option<f64>,
) -> SolidResult<Vec<f64>> {
    use solidcore::topology::brep::planar_section::face_planar_section_curves;
    use solidcore::topology::face::p2 as face2;
    let faces = brep1::faces(b, None)?;
    let mut out = vec![faces.len() as f64];
    for f in faces {
        let uv_domain = face2::uv_domain(b, f)?;
        let surface = face2::surface(b, f)?
            .ok_or_else(|| SolidError::null_pointer("difftest::topology_brep_planar_section"))?
            .clone_for_evaluation()?;
        match face_planar_section_curves(
            k, b, f, &surface, &uv_domain, point, normal, tol, ang_tol,
        ) {
            Ok(section) => {
                out.push(0.);
                out.push(section.curves_3d.len() as f64);
                for c in &section.curves_3d {
                    put_curve(k, &mut out, Some(c));
                }
                out.push(section.uv_curves.len() as f64);
                for c in &section.uv_curves {
                    put_curve(k, &mut out, c.as_ref());
                }
            }
            Err(e) if e.is_unported() => return Err(e),
            Err(e) => match status(&e) {
                Some(code) => out.extend([code, 0., 0.]),
                None => return Err(e),
            },
        }
    }
    Ok(out)
}

fn output(raw: &[f64]) -> Result<Output, HarnessError> {
    let (&st, record) = raw.split_first().ok_or_else(|| herr("empty record"))?;
    Ok(Output::new().scalar("status", st).values("record", record.to_vec()))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_planar_section"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match native(&i)? {
            Ok(raw) => output(&raw),
            Err(refused) => Ok(refused),
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        let mut b = match build(&mut k, &i)? {
            Ok(b) => b,
            Err(e) => {
                return Output::unported(&e)
                    .ok_or_else(|| herr(format!("rust body build: {e}")));
            }
        };
        if let Some(t) = i.translate {
            let mut placement = solidcore::geom::axis2_placement::Axis2Placement::default();
            placement.translate(v(t));
            if let Err(e) =
                solidcore::topology::brep::p5::transform(&mut k, &mut b, &placement, None, true)
            {
                return Output::unported(&e)
                    .ok_or_else(|| herr(format!("rust body transform: {e}")));
            }
        }
        // every face surface carries its face's tolerance (`GetFace()->GetTolerance()`)
        let faces: Vec<_> = b.faces().ids().collect();
        for f in faces {
            if let Err(e) = b.sync_face_surface_tolerance(f) {
                return Err(herr(format!("rust face tolerance sync: {e}")));
            }
        }
        let mut point = v(i.point);
        if let Some([n, at]) = i.plane_walk.filter(|w| w[0] > 1) {
            let bbox = match solidcore::query::brep::calculate_bounding_box(
                &k,
                &b,
                solidcore::query::brep::BoundingBoxOptions::default(),
            ) {
                Ok(bbox) => bbox,
                Err(e) => {
                    return Output::unported(&e)
                        .ok_or_else(|| herr(format!("rust body box: {e}")));
                }
            };
            let t = f64::from(at) / (f64::from(n) - 1.0);
            point = bbox.evaluate(t, t, t);
        }
        let normal = v(i.normal);
        let result = if i.forced {
            forced_record(&k, &mut b, point, normal, i.tol, i.ang_tol)
        } else if i.faces {
            faces_record(&k, &mut b, point, normal, i.tol, i.ang_tol)
        } else if i.api {
            intersect_brep_with_plane(&k, &mut b, point, normal).map(|curves| {
                let mut out = vec![curves.len() as f64];
                for c in &curves {
                    put_curve(&k, &mut out, Some(c));
                }
                out
            })
        } else {
            let opts = PlanarSectionOptions {
                approx_tol_3d: i.tol,
                ang_tol_rad: i.ang_tol,
                outputs: SectionOutputs {
                    curves_3d: i.want_3d,
                    uv_curves: i.want_uv,
                    faces: i.want_faces,
                },
                force_uv_classify: i.force_uv_classify,
            };
            create_planar_section_curves(&k, &mut b, point, normal, &opts)
                .and_then(|s| section_record(&k, &b, &s))
        };
        let raw = match result {
            Ok(record) => {
                let mut raw = vec![0.];
                raw.extend(record);
                raw
            }
            Err(e) if e.is_unported() => {
                return Output::unported(&e).ok_or_else(|| herr(e));
            }
            Err(e) => match status(&e) {
                // a failed call: its status, and the empty outputs of the shim's layout
                Some(_) if i.faces || i.forced => return Err(herr(e)),
                Some(code) if i.api => vec![code, 0.],
                Some(code) => vec![
                    code,
                    if i.want_3d { 0. } else { -1. },
                    if i.want_uv { 0. } else { -1. },
                    if i.want_faces { 0. } else { -1. },
                ],
                None => return Err(herr(e)),
            },
        };
        output(&raw)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let mut i = Inputs::default();
        let scale = 10f64.powi(rng.range_i64(-2, 2) as i32);
        if rng.below(3) == 0 {
            let file = FILES.get(rng.below(FILES.len() as u64) as usize)?;
            i.body = Body::File {
                file: (*file).into(),
            };
        } else {
            let size = [
                rng.uniform(1., 20.) * scale,
                rng.uniform(1., 20.) * scale,
                rng.uniform(1., 20.) * scale,
            ];
            let smallest = size[0].min(size[1]).min(size[2]);
            let fillet = if rng.chance(0.3) {
                rng.uniform(0.02, 0.2) * smallest
            } else {
                0.0
            };
            i.body = if fillet == 0.0 && rng.chance(0.3) {
                // a strut in the top face (an embedded edge)
                let strut = [
                    rng.uniform(0.1, 0.9) * size[0],
                    rng.uniform(0.1, 0.9) * size[1],
                    size[2],
                ];
                Body::BoxStrut { size, strut }
            } else {
                Body::Box { size, fillet }
            };
        }
        // a plane through the body's box (or past it), axis-aligned or arbitrary
        let reach = match &i.body {
            Body::Box { size, .. } | Body::BoxStrut { size, .. } => *size,
            Body::File { .. } => [10.; 3],
        };
        i.point = [
            rng.uniform(-0.2, 1.2) * reach[0],
            rng.uniform(-0.2, 1.2) * reach[1],
            rng.uniform(-0.2, 1.2) * reach[2],
        ];
        i.normal = match rng.below(4) {
            0 => [1., 0., 0.],
            1 => [0., 1., 0.],
            2 => [0., 0., rng.uniform(0.5, 5.)],
            _ => [
                rng.uniform(-1., 1.),
                rng.uniform(-1., 1.),
                rng.uniform(-1., 1.),
            ],
        };
        i.api = rng.chance(0.25);
        i.faces = !i.api && rng.chance(0.15);
        if rng.chance(0.3) {
            i.tol = Some(if rng.chance(0.1) {
                0.0
            } else {
                10f64.powi(rng.range_i64(-8, -3) as i32)
            });
        }
        if rng.chance(0.2) {
            i.ang_tol = Some(rng.uniform(0.05, 1.0));
        }
        i.want_3d = rng.chance(0.8);
        i.want_uv = rng.chance(0.5);
        i.want_faces = rng.chance(0.5);
        i.force_uv_classify = i.want_uv && rng.chance(0.4);
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(96);
        crate::runner::default_cases(self, &ctx)
    }
}
