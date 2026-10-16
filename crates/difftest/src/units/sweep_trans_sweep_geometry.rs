//! Native differential coverage of translational sweep geometry and boundary roles.
// Fixed wire layout / test geometry indices are validated before use.
#![allow(clippy::indexing_slicing)]

use crate::case::CaseContext;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::kernel::Kernel;
use solidcore::math::{Vec2, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::CPoint;
use solidcore::primitive::api_curves;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::sweep::topology_sweep::SweepGeometryCreation;
use solidcore::sweep::trans_sweep_geometry::TranslationalSweepGeometry;
use solidcore::topology::{Brep, Edgeuse, EdgeuseOwner, Vertexuse, VertexuseOwner};
use solidcore::{Result, SolidError};
use solidcore_ffi::units::sweep_trans_sweep_geometry as ffi;

/// Translational sweep differential runner.
pub struct Runner;
#[derive(Clone, Serialize, Deserialize)]
struct Input {
    op: u8,
    sweep: [f64; 3],
    #[serde(default)]
    point: [f64; 3],
    #[serde(default = "tols")]
    tolerances: [f64; 4],
    #[serde(default = "eps")]
    epsilon: f64,
    #[serde(default)]
    analytics: bool,
    #[serde(default = "one")]
    kind: u8,
    #[serde(default = "points")]
    points: Vec<[f64; 4]>,
    #[serde(default = "ivl")]
    interval: [f64; 2],
    #[serde(default)]
    swap: bool,
    #[serde(default)]
    face_kind: u8,
}
fn one() -> u8 {
    1
}
fn eps() -> f64 {
    1e-5
}
fn tols() -> [f64; 4] {
    [1e-5; 4]
}
fn points() -> Vec<[f64; 4]> {
    vec![[0., 0., 0., 1.], [1., 0., 0., 1.]]
}
fn ivl() -> [f64; 2] {
    [0., 1.]
}
fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => 1007.,
        SolidError::Nlib { .. } => 1001.,
        _ => -1.,
    }
}
fn flat(i: &Input) -> Vec<f64> {
    let mut o = vec![f64::from(i.op)];
    o.extend(i.sweep);
    o.extend(i.point);
    o.extend(i.tolerances);
    o.extend([
        i.epsilon,
        f64::from(i.analytics),
        f64::from(i.kind),
        i.points.len() as f64,
        i.interval[0],
        i.interval[1],
        f64::from(i.swap),
        f64::from(i.face_kind),
    ]);
    o.extend(i.points.iter().flatten());
    o
}
fn build(k: &Kernel, i: &Input) -> Result<Curve> {
    let first = i
        .points
        .first()
        .ok_or(SolidError::invalid_input("runner", "points"))?;
    let last = i
        .points
        .last()
        .ok_or(SolidError::invalid_input("runner", "points"))?;
    if i.kind == 0 {
        return api_curves::create_line_segment(
            Vec3::new(first[0], first[1], first[2]),
            Vec3::new(last[0], last[1], last[2]),
        )
        .map(Curve::from);
    }
    if i.kind == 2 {
        return api_curves::create_analytic_circle(
            k,
            Vec3::new(first[0], first[1], first[2]),
            first[3],
        )
        .map(Curve::from);
    }
    if i.kind == 5 {
        return Ellipse::create_canonical(
            k,
            &Axis2Placement::new(Vec3::new(first[0], first[1], first[2]), Vec3::X, Vec3::Y),
            first[3],
            last[3],
            None,
            None,
        )
        .map(Curve::from);
    }
    if i.kind == 3 {
        return Ok(Curve::from(HermiteCurve::new(
            Vec3::new(first[0], first[1], first[2]),
            Vec3::new(1., 1., 0.),
            Vec3::new(last[0], last[1], last[2]),
            Vec3::new(1., -1., 0.),
            3,
        )));
    }
    let n = i.points.len();
    let mut knots = vec![0.; n];
    knots.extend(vec![1.; n]);
    let cp: Vec<_> = i
        .points
        .iter()
        .map(|p| CPoint::new(p[0], p[1], if i.kind == 4 { f64::MAX } else { p[2] }, p[3]))
        .collect();
    BSplineCurve::from_data(
        if i.kind == 4 { 2 } else { 3 },
        n - 1,
        2 * n - 1,
        n - 1,
        &cp,
        &knots,
    )
    .map(Curve::from)
}
fn put(o: &mut Vec<f64>, p: Vec3) {
    o.extend([p.x, p.y, p.z]);
}
fn curve(o: &mut Vec<f64>, c: &Curve) -> Result<()> {
    o.push(c.type_code() as f64);
    let iv = c.natural_interval();
    o.extend([iv.min(), iv.max()]);
    for j in 0..=8 {
        match c.evaluate(iv.evaluate(f64::from(j) / 8.), 1, true, true) {
            Ok(p) => {
                o.push(0.);
                for x in p {
                    put(o, x);
                }
            }
            Err(e) if e.is_unported() => return Err(e),
            Err(e) => o.push(status(&e)),
        }
    }
    Ok(())
}
fn surface(o: &mut Vec<f64>, s: &Surface) -> Result<()> {
    o.push(s.type_code() as f64);
    let d = s.natural_uv_domain();
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    for u in 0..=4 {
        for z in 0..=4 {
            let uv = d.evaluate(f64::from(u) / 4., f64::from(z) / 4.);
            for r in [
                s.evaluate_point(uv),
                s.evaluate_normal(uv, Side::Left, Side::Left),
            ] {
                match r {
                    Ok(p) => {
                        o.push(0.);
                        put(o, p);
                    }
                    Err(e) if e.is_unported() => return Err(e),
                    Err(e) => o.push(status(&e)),
                }
            }
        }
    }
    Ok(())
}
fn rust(i: &Input) -> Result<Vec<f64>> {
    let k = Kernel::default();
    let sweep = TranslationalSweepGeometry {
        sweep_vector: v(i.sweep),
        base: SweepGeometryCreation {
            eps: i.epsilon,
            make_analytics: i.analytics,
        },
    };
    let mut o = vec![0.];
    let mut b = Brep::new();
    match i.op {
        0..=2 => {
            let f = match i.op {
                0 => sweep.start_coord_system(v(i.point))?,
                1 => sweep.end_coord_system(v(i.point))?,
                _ => sweep.sweep_transform(),
            };
            for p in [f.origin(), f.x_axis(), f.y_axis(), f.z_axis()] {
                put(&mut o, p);
            }
        }
        3 | 4 => {
            let id = b.add_vertex(v(i.point))?;
            b.vertex_mut(id)?.tolerance = zone_tol3d(&k, i.tolerances[0]);
            if i.op == 3 {
                let (p, t) = sweep.vertex_sweep_same(&b, id)?;
                o.push(t);
                put(&mut o, p);
            } else {
                let (c, t) = sweep.vertex_sweep_higher(&b, id)?;
                o.push(t);
                curve(&mut o, &Curve::from(c))?;
            }
        }
        5 => {
            let c = build(&k, i)?;
            let result = sweep.check_curve_sweep(
                &k,
                &c,
                &Extent1d::from_min_max(i.interval[0], i.interval[1]),
                i.epsilon,
                v(i.sweep),
            )?;
            o.push(f64::from(result.code()));
        }
        6 | 7 => {
            let c = build(&k, i)?;
            let iv = Extent1d::from_min_max(i.interval[0], i.interval[1]);
            let mut a = c.evaluate_point(iv.min())?;
            let mut z = c.evaluate_point(iv.max())?;
            if i.swap {
                core::mem::swap(&mut a, &mut z);
            }
            let aid = b.add_vertex(a)?;
            let closed = (a - z).length() < b.tolerance();
            let zid = if closed { aid } else { b.add_vertex(z)? };
            b.vertex_mut(aid)?.tolerance = zone_tol3d(&k, i.tolerances[0]);
            if zid != aid {
                b.vertex_mut(zid)?.tolerance = zone_tol3d(&k, i.tolerances[1]);
            }
            let eid = b.add_edge(aid, zid)?;
            {
                let e = b.edge_mut(eid)?;
                e.curve = Some(c);
                e.interval = iv;
                e.tolerance = zone_tol3d(&k, i.tolerances[2]);
            }
            let shell = b.add_shell()?;
            let mut eus = Vec::new();
            for (id, orientation) in [(aid, OrientType::Same), (zid, OrientType::Opposite)] {
                let eu = b.add_edgeuse_record(Edgeuse {
                    edge: Some(eid),
                    owner: EdgeuseOwner::Shell(Some(shell)),
                    orientation,
                    ..Default::default()
                })?;
                let vu = b.add_vertexuse_record(Vertexuse {
                    vertex: Some(id),
                    owner: VertexuseOwner::Edgeuse(Some(eu)),
                    ..Default::default()
                })?;
                b.edgeuse_mut(eu)?.vertexuse = Some(vu);
                b.vertex_mut(id)?.vertexuses.push(vu);
                eus.push(eu);
            }
            b.edge_mut(eid)?.edgeuses = eus;
            if i.op == 6 {
                let (c, t) = sweep.edge_sweep_same(&k, &b, eid)?;
                o.push(t);
                curve(&mut o, &c)?;
            } else {
                let r = sweep.edge_sweep_higher(&k, &b, eid)?;
                o.extend([
                    r.face_tolerance,
                    r.curve_tolerances[0],
                    r.curve_tolerances[2],
                    r.curve_tolerances[r.start_vertex_curve],
                    r.end_vertex_curve.map_or(0., |j| r.curve_tolerances[j]),
                ]);
                put(&mut o, r.start_normal);
                surface(&mut o, &r.surface)?;
                o.push(4.);
                for j in 0..4 {
                    o.extend([
                        match r.orientations[j] {
                            OrientType::Same => 1.,
                            OrientType::Opposite => 2.,
                            OrientType::Unknown => 0.,
                        },
                        f64::from(j == r.start_vertex_curve),
                        f64::from(Some(j) == r.end_vertex_curve),
                    ]);
                    curve(&mut o, &r.curves[j])?;
                    curve(&mut o, &Curve::from(r.uv_curves[j].clone()))?;
                }
            }
        }
        _ => {
            let s = if i.face_kind == 0 {
                Surface::from(Plane::new(
                    v(i.point),
                    Vec3::X,
                    Vec3::Y,
                    Vec2::new(1., 1.),
                    &Extent2d::from_bounds(0., 0., 1., 1.),
                ))
            } else {
                let c = build(&k, i)?;
                let mut bs = match c.kind() {
                    CurveKind::BSpline(c) => c.copy(),
                    CurveKind::Line(c) => c.base_with_nurb()?.into_owned(),
                    CurveKind::Ellipse(c) => c.base().copy(),
                    _ => return Err(SolidError::sm_err("runner")),
                };
                Surface::from(BSplineSurface::create_linear_sweep(
                    &mut bs,
                    Vec3::new(0., 0., 2.),
                )?)
            };
            let sh = b.add_shell()?;
            let f = b.add_face(sh)?;
            {
                let f = b.face_mut(f)?;
                f.uv_domain = s.natural_uv_domain();
                f.surface = Some(s);
                f.tolerance = zone_tol3d(&k, i.tolerances[3]);
            }
            let (s, t) = sweep.face_sweep_same(&k, &b, f)?;
            o.push(t);
            surface(&mut o, &s)?;
        }
    }
    Ok(o)
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "sweep.trans_sweep_geometry"
    }
    fn run_cpp(&self, c: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Input = c.parse()?;
        match ffi::eval(&flat(&i)) {
            Ok(v) => Ok(Output::new().values("geometry", v)),
            Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => {
                Ok(Output::rejected(
                    "native fixture constructor rejected the geometry before the sweep",
                ))
            }
            Err(e) => Err(HarnessError::new(e.to_string())),
        }
    }
    fn run_rust(&self, c: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Input = c.parse()?;
        Ok(match rust(&i) {
            Ok(v) => Output::new().values("geometry", v),
            Err(e) => Output::unported(&e)
                .unwrap_or_else(|| Output::new().values("geometry", vec![status(&e)])),
        })
    }
    fn cases(&self, ctx: &CaseContext) -> std::result::Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(512);
        crate::runner::default_cases(self, &ctx)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = rng.below(9) as u8;
        let scale = [1e-9, 1e-6, 1., 1e6][rng.below(4) as usize];
        let n = 2 + rng.below(4) as usize;
        let mut p: Vec<[f64; 4]> = (0..n)
            .map(|j| {
                [
                    j as f64 * scale,
                    rng.uniform(-1., 1.) * scale,
                    rng.uniform(-1., 1.) * scale,
                    1.,
                ]
            })
            .collect();
        if rng.below(3) == 0 {
            for q in &mut p {
                let w = rng.uniform(0.5, 2.);
                for x in &mut q[..3] {
                    *x *= w;
                }
                q[3] = w;
            }
        }
        let kind = rng.below(6) as u8;
        if kind == 2 || kind == 5 {
            // Circle entries encode a center and a radius, not homogeneous points.
            p[0][3] = rng.uniform(0.5, 2.) * scale;
            if kind == 5 {
                p[n - 1][3] = p[0][3] * 0.5;
            }
        }
        let i = Input {
            op,
            sweep: [
                rng.uniform(-2., 2.) * scale,
                rng.uniform(-2., 2.) * scale,
                rng.uniform(0.5, 3.) * scale,
            ],
            point: rng.point3(-scale, scale),
            tolerances: [scale * 1e-6; 4],
            epsilon: scale * 1e-5,
            analytics: rng.below(2) == 0,
            kind,
            points: p,
            interval: if rng.below(2) == 0 {
                [0., 1.]
            } else {
                [0.2, 0.8]
            },
            swap: rng.below(2) == 0,
            // SmHermiteCurve does not derive from SmBSplineCurve, which the native
            // face-fixture extrusion requires. Its sweep dispatch is still tested.
            face_kind: if kind == 3 { 0 } else { rng.below(2) as u8 },
        };
        serde_json::to_value(i).ok()
    }
}
