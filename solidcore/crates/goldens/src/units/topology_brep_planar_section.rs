//! Runner for unit `topology.brep_planar_section` (`SmBrep::CreatePlanarSectionCurves`,
//! `SmApiIntersectBrepWithPlane`). Ported from upstream `SmBrep::CreatePlanarSectionCurves`;
//! checked against its golden.
//!
//! The case builds the body: an upstream `.smb` solid (the Rust reader
//! `own_reader::read_file`) or a box (`create_box`), optionally filleted on every edge
//! (`circular_fillet`). It may move the body first (`transform`) and may take the plane
//! point from the B-rep's bounding box (`CalculateBoundingBox`, as `prog_test`'s
//! `my_test_tsurf_section` walks it). It then sections the body by a plane, with
//! `CreatePlanarSectionCurves` (its tolerances, outputs and `bForceUVClassify` from the
//! case) or with `SmApiIntersectBrepWithPlane`, and records the status (`status`) and the
//! record (`record`): the count and samples of every 3D and UV section curve (type code,
//! natural interval, five evaluated points) and the `GetFaces` index of each face.

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

fn path(i: &Inputs) -> Result<String, HarnessError> {
    match &i.body {
        Body::File { file } => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        Body::Box { .. } | Body::BoxStrut { .. } => Ok(String::new()),
    }
}

/// The status code of a section call (`SM_SUCCESS` is `0`).
fn status(e: &SolidError) -> Option<f64> {
    match e {
        up_err if solidcore::error::upstream_status(up_err).is_some() => Some(f64::from(solidcore::error::upstream_status(up_err).unwrap_or(-1))),
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

#[cfg(test)]
mod section_certificate {
    //! WR-723: the section curves of the rows that moved when a face's section became traced
    //! to the face's tolerance are proved within that tolerance of the plane and of a face's
    //! (untrimmed) surface, every 3D curve of every row. It does not prove a curve lies inside
    //! its face's trim, nor certify UV curves or op 4's drops and lifts. With upstream's size
    //! default the test fails.
    //!
    //! - **Plane:** a NURBS curve with positive weights lies in the hull of its Euclidean
    //!   control points, so the largest `|e . (P_i - p0)|` bounds its distance from the plane
    //!   (`plane_bound`, rounding counted with `gamma_k`).
    //! - **Surface:** `nurbs::certificate::span_bound` bounds the distance between the curve
    //!   and the surface's NURBS form over a correspondence between them (by the convex hull of
    //!   Bernstein coefficients, rounding counted), here between nodes at stations along the
    //!   curve. The nodes are projections and are not trusted: the bound holds for any
    //!   correspondence. Each piece of the curve then lies within the bound of the surface.
    use super::*;
    use solidcore::curve::curve::CurveKind;
    use solidcore::geom::certified::{gamma, unit, unit_error};
    use solidcore::geom::vector2d::Point2;
    use solidcore::math::Point3;
    use solidcore::nurbs::certificate::{node_on, span_bound, Node, RailSpans};
    use solidcore::nurbs::math::NOW;
    use solidcore::topology::brep::planar_section::face_planar_section_curves;
    use solidcore::topology::face::p2 as face2;
    use solidcore::topology::FaceId;

    /// The rows whose section moved (`planar_section_face_tolerance` retired), each with the
    /// number of 3D section curves its call traces (89 in all), so no row passes by certifying
    /// fewer curves than it has.
    const MOVED: [(&str, usize); 15] = [
        ("cone_slant", 1),
        ("faces_fillet_box_y98", 12),
        ("fillet_box_y98_uv_unforced", 4),
        ("fillet_box_y_forced", 8),
        ("gen0015", 6),
        ("gen0025", 5),
        ("gen0038", 4),
        ("gen0056", 4),
        ("gen0070", 4),
        ("gen0071", 5),
        ("gen0073", 8),
        ("gen0074", 4),
        ("gen0089", 9),
        ("gen0093", 10),
        ("nurb_fillet_section", 5),
    ];

    /// Stations per curve for the surface certificate, more where fewer do not prove it (each
    /// piece's correspondence starts as the line between two projected nodes; `span_bound`
    /// halves it where needed).
    const STATIONS: [usize; 4] = [1024, 8192, 65536, 262_144];

    /// A certified bound on the distance of the curve's NURB (positive weights) from the
    /// plane through `p0` with normal `n`: the curve lies in the hull of its Euclidean control
    /// points, so the largest `|e . (P_i - p0)|` bounds it; each term's rounding is counted.
    fn plane_bound(nurb: &solidcore::nurbs::crv_basic::Curve, p0: Point3, n: Vec3) -> Option<f64> {
        let e = unit(n)?;
        let ue = unit_error();
        let mut worst = 0.0_f64;
        for c in nurb.cpts() {
            let w = if c.w == NOW { 1.0 } else { c.w };
            if w.is_nan() || w <= 0.0 {
                return None;
            }
            let p = if c.w == NOW { Vec3::new(c.x, c.y, c.z) } else { Vec3::new(c.x / w, c.y / w, c.z / w) };
            let q = p - p0;
            let size = p.x.abs().max(p.y.abs()).max(p.z.abs()) + p0.x.abs().max(p0.y.abs()).max(p0.z.abs());
            // each component of q within eq of exact: the division and the subtraction
            let eq = gamma(2) * size;
            let ql = q.length() * (1.0 + gamma(5)) + 2.0 * eq;
            let err = 2.0 * ue * ql + (1.0 + 2.0 * ue) * 2.0 * eq + gamma(3) * (1.0 + 2.0 * ue) * ql;
            worst = worst.max((e.dot(q).abs() + err) * (1.0 + gamma(2)));
        }
        Some(worst)
    }

    /// The 3D curves of the row and the face each belongs to where the call says, from the
    /// same call `run_rust` makes (op 3 and op 4 both section each face with
    /// `face_planar_section_curves`; op 4's drops and lifts, and every row's UV curves, are not
    /// certified here).
    fn section(k: &Kernel, b: &mut Brep, i: &Inputs) -> SolidResult<Vec<(Curve, Option<FaceId>)>> {
        let (point, normal) = (v(i.point), v(i.normal));
        let faces: Vec<_> = b.faces().ids().collect();
        for f in faces {
            b.sync_face_surface_tolerance(f)?;
        }
        if i.faces || i.forced {
            let mut out = Vec::new();
            for f in brep1::faces(b, None)? {
                let dom = face2::uv_domain(b, f)?;
                let Some(s) = face2::surface(b, f)? else { continue };
                let s = s.clone_for_evaluation()?;
                if let Ok(sec) = face_planar_section_curves(k, b, f, &s, &dom, point, normal, i.tol, i.ang_tol) {
                    out.extend(sec.curves_3d.into_iter().map(|c| (c, Some(f))));
                }
            }
            return Ok(out);
        }
        if i.api {
            return Ok(intersect_brep_with_plane(k, b, point, normal)?.into_iter().map(|c| (c, None)).collect());
        }
        let opts = PlanarSectionOptions {
            approx_tol_3d: i.tol,
            ang_tol_rad: i.ang_tol,
            // the 3D curves are traced whether or not the row records them
            outputs: SectionOutputs { curves_3d: true, uv_curves: i.want_uv, faces: i.want_faces },
            force_uv_classify: i.force_uv_classify,
        };
        let s = create_planar_section_curves(k, b, point, normal, &opts)?;
        Ok(s.curves_3d.unwrap_or_default().into_iter().map(|c| (c, None)).collect())
    }

    /// The certified bound of a curve against the surface of the face it lies on (its own
    /// face where the call says, else the face whose surface is nearest the curve's middle),
    /// that face's tolerance, and a note of the face's index, the surface's kind and its
    /// definition bound. The bound is to the face's whole (untrimmed) surface.
    fn surface_bound(
        k: &Kernel,
        b: &Brep,
        c: &Curve,
        own: Option<FaceId>,
    ) -> Result<(f64, f64, String), String> {
        let base = match c.kind() {
            CurveKind::Line(l) => l.base_with_nurb().map_err(|e| e.to_string())?.into_owned(),
            _ => c.b_spline_base().ok_or("no NURBS form")?.clone(),
        };
        let nurb = base.gw_nurb().ok_or("no NURB")?.clone();
        let ivl = c.natural_interval();
        let mid = c.evaluate_point(k, ivl.evaluate(0.5)).map_err(|e| e.to_string())?;
        let face = if let Some(f) = own {
            f
        } else {
            let mut best: Option<(FaceId, f64)> = None;
            for f in brep1::faces(b, None).map_err(|e| e.to_string())? {
                let Some(s) = face2::surface(b, f).ok().flatten() else { continue };
                let Ok((_, d)) = node_on(k, s, mid) else { continue };
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((f, d));
                }
            }
            best.ok_or("no face's surface")?.0
        };
        let s = face2::surface(b, face).ok().flatten().ok_or("a face without surface")?.clone();
        let tol = face2::tolerance(b, face).map_err(|e| e.to_string())?;
        let slack = match s.kind() {
            solidcore::surface::surface::SurfaceKind::ProceduralBlend(pb) => pb.bound(),
            _ => 0.0,
        };
        let index = brep1::faces(b, None).map_err(|e| e.to_string())?.iter().position(|&f| f == face);
        let kind = format!("face {index:?} surface type {}", s.type_code());
        // The nodes are not trusted (the bound holds for any correspondence); they are chosen
        // so the correspondence runs in step with the curve: where the projection's foot is
        // ambiguous (a pole, where every `u` is the same point) the previous node's parameter
        // is kept if it is as near, and where the foot jumps across a closed surface's seam the
        // piece is cut at the seam, each side ending on its own edge of the parameter range.
        let dom = s.natural_uv_domain_with_nurb(k).map_err(|e| e.to_string())?;
        let (u0, u1) = (dom.evaluate(0.0, 0.0).x, dom.evaluate(1.0, 0.0).x);
        let (v0, v1) = (dom.evaluate(0.0, 0.0).y, dom.evaluate(0.0, 1.0).y);
        let gap = |uv: Point2, q: Point3| s.evaluate_point(k, uv).map_or(f64::INFINITY, |p| (p - q).length());
        let foot = |q: Point3, prev: Option<Point2>| -> Result<Point2, String> {
            let (uv, _) = node_on(k, &s, q).map_err(|e| e.to_string())?;
            let Some(pr) = prev else { return Ok(uv) };
            let d = gap(uv, q);
            let mut best = uv;
            for cand in [Point2::new(pr.x, uv.y), Point2::new(uv.x, pr.y), pr] {
                if gap(cand, q) <= d + 1e-12 * (1.0 + q.length())
                    && (cand - pr).length() < (best - pr).length()
                {
                    best = cand;
                }
            }
            Ok(best)
        };
        let jumps = |a: Point2, b: Point2| {
            (a.x - b.x).abs() > 0.5 * (u1 - u0).abs() || (a.y - b.y).abs() > 0.5 * (v1 - v0).abs()
        };
        let edge = |x: f64, lo: f64, hi: f64| if (x - lo).abs() < (x - hi).abs() { lo } else { hi };
        let mut worst = f64::INFINITY;
        for stations in STATIONS {
            let mut segments: Vec<(Node, Node)> = Vec::new();
            let ts: Vec<f64> = (0..=stations).map(|j| ivl.evaluate(j as f64 / stations as f64)).collect();
            let q0 = c.evaluate_point(k, ts[0]).map_err(|e| e.to_string())?;
            let mut prev = Node { t: ts[0], uv: foot(q0, None)? };
            for &t in &ts[1..] {
                let q = c.evaluate_point(k, t).map_err(|e| e.to_string())?;
                let uv = foot(q, Some(prev.uv))?;
                if jumps(prev.uv, uv) {
                    // cut at the seam: bisect for the curve parameter where the foot crosses it
                    let (mut lo, mut hi) = (prev.t, t);
                    let mut hi_uv = uv;
                    for _ in 0..60 {
                        let m = 0.5 * (lo + hi);
                        let qm = c.evaluate_point(k, m).map_err(|e| e.to_string())?;
                        let um = foot(qm, Some(prev.uv))?;
                        if jumps(prev.uv, um) {
                            hi = m;
                            hi_uv = um;
                        } else {
                            lo = m;
                        }
                    }
                    let qs = c.evaluate_point(k, hi).map_err(|e| e.to_string())?;
                    let at = foot(qs, Some(prev.uv))?;
                    let left = Point2::new(
                        if (prev.uv.x - hi_uv.x).abs() > 0.5 * (u1 - u0).abs() { edge(prev.uv.x, u0, u1) } else { at.x },
                        if (prev.uv.y - hi_uv.y).abs() > 0.5 * (v1 - v0).abs() { edge(prev.uv.y, v0, v1) } else { at.y },
                    );
                    let right = Point2::new(
                        if (prev.uv.x - hi_uv.x).abs() > 0.5 * (u1 - u0).abs() { edge(hi_uv.x, u0, u1) } else { at.x },
                        if (prev.uv.y - hi_uv.y).abs() > 0.5 * (v1 - v0).abs() { edge(hi_uv.y, v0, v1) } else { at.y },
                    );
                    segments.push((prev, Node { t: hi, uv: left }));
                    prev = Node { t: hi, uv: right };
                }
                segments.push((prev, Node { t, uv }));
                prev = Node { t, uv };
            }
            worst = 0.0_f64;
            for (a, b) in segments {
                let qa = c.evaluate_point(k, a.t).map_err(|e| e.to_string())?;
                let spans = RailSpans::about(&nurb, qa).map_err(|e| e.to_string())?;
                let bound = span_bound(k, &s, c, &spans, qa, a, b, tol).map_err(|e| e.to_string())?;
                worst = worst.max(bound);
            }
            if worst < tol {
                break;
            }
        }
        Ok((worst, tol, format!("{kind} definition bound {slack:.3e}")))
    }

    #[test]
    fn the_moved_rows_section_curves_are_proved_within_their_face_tolerance() {
        let ctx = CaseContext { root: crate::case::repo_root(), seeds: 16 };
        let cases = Runner.cases(&ctx).unwrap();
        let mut report = Vec::new();
        let mut failed = Vec::new();
        let mut total = 0;
        for (name, count) in MOVED {
            let case = cases.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("case {name}"));
            let i: Inputs = case.parse().unwrap();
            let mut k = Kernel::default();
            let mut b = build(&mut k, &i).unwrap().unwrap();
            assert!(i.translate.is_none() && i.plane_walk.is_none(), "{name}: not covered");
            println!(
                "row {name}: api {} faces {} forced {} want_3d {} want_uv {} want_faces {} force_uv {}",
                i.api, i.faces, i.forced, i.want_3d, i.want_uv, i.want_faces, i.force_uv_classify
            );
            let curves = section(&k, &mut b, &i).unwrap();
            if curves.len() != count {
                failed.push(format!("{name}: {} section curves, the row has {count}", curves.len()));
            }
            total += curves.len();
            for (ci, (c, own)) in curves.iter().enumerate() {
                let nurb = match c.kind() {
                    CurveKind::Line(l) => l.base_with_nurb().ok().and_then(|x| x.gw_nurb().cloned()),
                    _ => c.b_spline_base().and_then(|x| x.gw_nurb().cloned()),
                };
                let plane = nurb.as_ref().and_then(|n| plane_bound(n, v(i.point), v(i.normal)));
                let surf = surface_bound(&k, &b, c, *own);
                // both bounds against the chosen face's tolerance
                let ok = match (&plane, &surf) {
                    (Some(p), Ok((s, tol, _))) => p < tol && s < tol,
                    _ => false,
                };
                let line = format!("{name} c{ci} type {} plane {plane:?} surface {surf:?}", c.type_code());
                if !ok {
                    failed.push(line.clone());
                }
                report.push(line);
            }
            // The sampled measurement's off-face samples (gen0089 c7 after; gen0038 c0, c3
            // before): each face's surface foot and the face's UV box, to see which face the
            // sampler (nearest face, UV grid inside the face's box) measured.
            for (row, ci, t) in [("gen0089", 7, 1.0 / 400.0), ("gen0038", 0, 0.5), ("gen0038", 3, 0.125)] {
                let Some((c, _)) = curves.get(ci).filter(|_| row == name) else { continue };
                let q = c.evaluate_point(&k, c.natural_interval().evaluate(t)).unwrap();
                for (fi, f) in brep1::faces(&b, None).unwrap().into_iter().enumerate() {
                    let Some(s) = face2::surface(&b, f).unwrap() else { continue };
                    let foot = node_on(&k, s, q);
                    let dom = face2::uv_domain(&b, f).unwrap();
                    println!(
                        "probe {row} c{ci} t={t}: face {fi} type {} node_on {foot:?} face uv box {:?}..{:?}",
                        s.type_code(),
                        dom.evaluate(0.0, 0.0),
                        dom.evaluate(1.0, 1.0)
                    );
                }
            }
        }
        for l in &report {
            println!("{l}");
        }
        println!("{total} section curves in {} rows", MOVED.len());
        assert!(failed.is_empty(), "not proved within tolerance:\n{}", failed.join("\n"));
    }
}
