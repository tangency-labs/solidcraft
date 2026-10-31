//! Public `SmApi` queries against real native geometry. The C++ side builds each
//! topology input natively; the Rust side builds the same B-rep with the Rust kernel
//! ([`own_brep`], in the shim's `sc_edgeuse_graph.h` traversal order), so no C++ state
//! reaches it. All query results come from `SmApi*`.
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::ShapeMember;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::intersect::topology_solver::p1::TopologyObjects;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::query::api_queries as api;
use solidcore::query::brep::{BrepIdentity, LiveBrepObjects};
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::p1::SurfaceSolveContext;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::face_context::FaceQueries;
use solidcore::surface::surface_cache::SurfaceCache;
use solidcore::topology::brep_cache::BrepCache;
use solidcore::topology::brep_cache::{arena_identity, CacheObject, CacheSource};
use solidcore::topology::{Brep, BrepId, FaceId};
use solidcore::{Result, SolidError};
use solidcore_ffi::units::query_api_queries as ffi;

/// Differential coverage of the public geometry and topology queries.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: i32,
    data: [f64; 22],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    other_box: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_relationship: Option<i32>,
    /// Rust side only (op 19): the solve over a bare [`LiveBrepObjects`], as a library
    /// caller makes it (no B-rep cache, no `face_entity`), not this runner's [`Objects`]
    /// adapter. The C++ side is the same `SmApiFaceClosestPoint`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    live: bool,
}

fn v(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}
fn vec(o: &mut Vec<f64>, p: Point3) {
    o.extend([p.x, p.y, p.z]);
}
fn closest(o: &mut Vec<f64>, p: api::ClosestPoint) {
    vec(o, p.point);
    o.push(p.distance);
}
fn cclosest(o: &mut Vec<f64>, p: api::CurveClosestPoint) {
    closest(o, p.closest);
    o.push(p.parameter);
}
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.,
    }
}
fn output(op: i32, r: Result<Vec<f64>>) -> Output {
    match r {
        Ok(v) => decode(op, v),
        Err(e) => Output::unported(&e).unwrap_or_else(|| decode(op, vec![status(&e)])),
    }
}

/// The case ops are `1..=25` and `27` (`SmApiBrepMaterialCensus`); the FFI's `0` and `26` are the graph exports
/// [`imported`] reads the native B-rep through, which have no Rust counterpart to
/// compare.
fn compared_op(op: i32) -> std::result::Result<(), HarnessError> {
    if (1..=25).contains(&op) || op == 27 {
        Ok(())
    } else {
        Err(HarnessError::new(format!("op {op} is not a compared query (1..=25)")))
    }
}

// Native null contracts are covered by the FFI tests; typed Rust APIs cannot
// represent them. Refuse these fixtures on both sides of the comparison.
fn compared_input(i: &Inputs) -> std::result::Result<(), HarnessError> {
    compared_op(i.op)?;
    if i.data[16] != 0. {
        return Err(HarnessError::new(
            "null pointers have no typed Rust equivalent; use the FFI contract tests",
        ));
    }
    Ok(())
}

// Integer topology and status fields are compared exactly; only geometric
// measurements use the harness's fixed relative tolerance.
fn decode(op: i32, values: Vec<f64>) -> Output {
    let mut ints = Vec::new();
    let mut floats = Vec::new();
    for (i, value) in values.into_iter().enumerate() {
        let integer = i == 0
            || matches!(op, 9 | 10 | 11 | 13 | 14 | 18 | 23 | 27)
            || matches!(op, 2 | 6 | 12) && i == 1
            || op == 3 && i == 5
            || op == 7 && i == 2;
        if integer {
            ints.push(value as i64);
        } else {
            floats.push(value);
        }
    }
    Output::new()
        .ints("discrete", ints)
        .values("geometry", floats)
}

fn curve(k: &Kernel, a: &[f64; 22]) -> Result<Curve> {
    let p = v([a[0], a[1], a[2]]);
    let d = v([a[3], a[4], a[5]]);
    let s = a[6];
    let domain = Extent1d::from_min_max(a[7], a[8]);
    match a[21] as i32 {
        10 => Ok(Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                vec![
                    CPoint::new(p.x, p.y, p.z, 1.),
                    CPoint::new(p.x + s, p.y, p.z, 1.),
                    CPoint::new(p.x + s, p.y + s, p.z, 1.),
                ],
                1,
                vec![0., 0., 0.5, 1., 1.],
            )?,
            3,
            false,
        ))),
        0 => Ok(Curve::from(Line::bounded(
            p,
            d,
            &domain,
            &BoundedLineOptions {
                scale: s,
                inside_out: a[18] != 0.,
                ..BoundedLineOptions::default()
            },
        ))),
        1 | 2 => {
            let opts = EllipseOptions {
                inside_out: a[18] != 0.,
                ..EllipseOptions::default()
            };
            let domain = Extent1d::from_min_max(0., 360.);
            Ok(Curve::from(if a[21] == 1. {
                Ellipse::new_circle(
                    k,
                    p,
                    Vec3::new(1., 0., 0.),
                    Vec3::new(0., 1., 0.),
                    domain,
                    s,
                    &opts,
                )?
            } else {
                Ellipse::new(
                    k,
                    p,
                    Vec3::new(1., 0., 0.),
                    Vec3::new(0., 1., 0.),
                    domain,
                    s,
                    s * 0.5,
                    &opts,
                )?
            }))
        }
        _ => {
            let pts = [
                p,
                p + s * d,
                p + Vec3::new(s, s, 0.),
                p + Vec3::new(2. * s, 0., s),
            ];
            let pw = pts
                .into_iter()
                .map(|p| CPoint::new(p.x, p.y, p.z, 1.))
                .collect();
            Ok(Curve::from(BSplineCurve::from_nurb(
                NlCurve::new(pw, 3, vec![0., 0., 0., 0., 1., 1., 1., 1.])?,
                3,
                false,
            )))
        }
    }
}

// Same control nets, operation order and concrete types as api_surface in the
// C++ shim. Surface queries have no topology owner on either side.
fn surface(k: &Kernel, a: &[f64; 22]) -> Result<Surface> {
    if a[21] == 13. {
        let origin = v([a[0], a[1], a[2]]);
        let generator = Curve::from(Line::bounded(
            origin + Vec3::new(a[6], 0., 0.),
            Vec3::new(0., 0., 1.),
            &Extent1d::from_min_max(0., 1.),
            &BoundedLineOptions {
                scale: a[6],
                ..BoundedLineOptions::default()
            },
        ));
        return Ok(Surface::from(SurfOfRevolution::create_canonical(
            k,
            generator,
            origin,
            Vec3::new(0., 0., 1.),
            false,
        )?));
    }
    // `api_surface`: an `SmPlane` for kinds 4 and 11
    if a[21] == 4. || a[21] == 11. {
        return Ok(Surface::from(Plane::from_normal(
            v([a[0], a[1], a[2]]),
            v([a[3], a[4], a[5]]),
        )));
    }
    let (net, p, q, u, w) = if a[21] == 8. || a[21] == 9. {
        let sphere = a[21] == 9.;
        let n = if sphere { 4 } else { 1 };
        let xy = [
            (1., 0.),
            (1., 1.),
            (0., 1.),
            (-1., 1.),
            (-1., 0.),
            (-1., -1.),
            (0., -1.),
            (1., -1.),
            (1., 0.),
        ];
        let radial = [0., 1., 1., 1., 0.];
        let height = [1., 1., 0., -1., -1.];
        let weight = 0.5_f64.sqrt();
        let mut net = Vec::new();
        for i in 0..=n {
            let mut row = Vec::new();
            for (j, &(x, y)) in xy.iter().enumerate() {
                let wi = if sphere && i % 2 != 0 { weight } else { 1. };
                let wj = if j % 2 != 0 { weight } else { 1. };
                let w = wi * wj;
                let rad = if sphere {
                    radial.get(i).copied().unwrap_or(0.)
                } else {
                    1.
                };
                let z = if sphere {
                    height.get(i).copied().unwrap_or(0.)
                } else {
                    i as f64
                };
                row.push(CPoint::new(
                    (a[0] + a[6] * rad * x) * w,
                    (a[1] + a[6] * rad * y) * w,
                    (a[2] + a[6] * z) * w,
                    w,
                ));
            }
            net.push(row);
        }
        (
            net,
            if sphere { 2 } else { 1 },
            2,
            if sphere {
                vec![0., 0., 0., 0.5, 0.5, 1., 1., 1.]
            } else {
                vec![0., 0., 1., 1.]
            },
            vec![0., 0., 0., 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1., 1., 1.],
        )
    } else {
        let net = (0..3)
            .map(|i| {
                (0..3)
                    .map(|j| {
                        CPoint::new(
                            a[0] + a[6] * f64::from(i),
                            a[1] + a[6] * f64::from(j),
                            a[2] + if i == 1 && j == 1 { a[6] } else { 0. },
                            1.,
                        )
                    })
                    .collect()
            })
            .collect();
        (
            net,
            2,
            2,
            vec![0., 0., 0., 1., 1., 1.],
            vec![0., 0., 0., 1., 1., 1.],
        )
    };
    Ok(Surface::from(BSplineSurface::from_nurb(
        NlSurface::new(net, p, q, u, w)?,
        false,
    )))
}

/// The shim's B-rep of a topology op, built by the Rust kernel (no C++ state reaches
/// it): a wire edge on the case curve (kinds 0-3, 10), a face on the case surface (4, 5,
/// 8, 9, 11; kind 11 then two vertex loops at the surface points at the domain fractions
/// 0.25 and 0.75), `SmApiCreateBox` (6), a planar face with a square hole from 3D line
/// segments (12), or an empty B-rep; renumbered into the shim's traversal order
/// (`sc_edgeuse_graph.h`), then the shim's edits on the selected edge and face:
/// `SetTolerance(a[19], FALSE, FALSE)` when `a[19] >= 0`, `SetCurve(NULL, TRUE, FALSE,
/// FALSE)` when `a[17]`, and with `orient_edit` (the shim's ops 11 and 26) and `a[18]`
/// the last edge use of the last upward loop use that has any set to unknown
/// orientation. The geometry is owned under the arena identities, recorded as the
/// B-rep's naming. A failed build is a harness error, as the shim's `SC_ERR_ARG`; an
/// unported callee defers.
fn own_brep(
    k: &mut Kernel,
    a: &[f64; 22],
    orient_edit: bool,
) -> std::result::Result<std::result::Result<Brep, Output>, HarnessError> {
    use crate::own_builder::{create_face, finish, in_new_brep, Built, Conventions};
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::brep::p2::ArenaNotifyState;
    let kind = a[21] as i32;
    let raw = if kind <= 3 || kind == 10 {
        match curve(k, a) {
            Ok(c) => in_new_brep(k, |k, b| {
                solidcore::topology::brep::p5::create_wire_edge_from_curve(
                    k,
                    b,
                    c,
                    Extent1d::from_min_max(a[7], a[8]),
                )
                .map(|_| ())
            })?,
            Err(e) => Err(e),
        }
    } else if matches!(kind, 4 | 5 | 8 | 9 | 11) {
        match surface(k, a) {
            Ok(s) => in_new_brep(k, |k, b| {
                let f = create_face(k, b, s, [a[7], a[8], a[9], a[10]])?;
                if kind == 11 {
                    for fraction in [0.25, 0.75] {
                        let uv = solidcore::geom::vector2d::Point2::new(
                            a[7] + fraction * (a[9] - a[7]),
                            a[8] + fraction * (a[10] - a[8]),
                        );
                        let point = solidcore::topology::face::p2::surface(b, f)?
                            .ok_or_else(|| SolidError::sm_err("query::api_queries fixture"))?
                            .evaluate_point(k, uv)?;
                        let mut state = ArenaNotifyState::new(b);
                        let mut env = state.env(None);
                        solidcore::topology::brep::p9::make_vertex_loop(
                            b,
                            f,
                            point,
                            OrientType::Opposite,
                            &mut env,
                        )?;
                    }
                }
                Ok(())
            })?,
            Err(e) => Err(e),
        }
    } else if kind == 6 {
        solidcore::primitive::api_primitives::create_box(
            k,
            v([a[0], a[1], a[2]]),
            a[6],
            a[6],
            a[6],
        )
    } else if (14..=16).contains(&kind) {
        use solidcore::boolean::api_brep as bo;
        let origin = v([a[0], a[1], a[2]]);
        let first = solidcore::primitive::api_primitives::create_box(k, origin, a[6], a[6], a[6]);
        let (at, size) = if kind == 15 {
            (v([a[0] + 2. * a[6], a[1], a[2]]), a[6])
        } else {
            (
                v([a[0] + 0.25 * a[6], a[1] + 0.25 * a[6], a[2] + 0.25 * a[6]]),
                0.5 * a[6],
            )
        };
        match first {
            Ok(outer) => solidcore::primitive::api_primitives::create_box(k, at, size, size, size)
                .and_then(|inner| {
                    if kind == 14 {
                        bo::boolean_difference(k, outer, inner)
                    } else {
                        bo::boolean_merge(k, outer, inner)
                    }
                }),
            Err(e) => Err(e),
        }
    } else if kind == 12 {
        // two real closed curve loops: an outer square and a square hole
        const XY: [[f64; 2]; 8] = [
            [0., 0.],
            [4., 0.],
            [4., 4.],
            [0., 4.],
            [1., 1.],
            [1., 3.],
            [3., 3.],
            [3., 1.],
        ];
        let mut curves = Vec::new();
        let mut failed = None;
        for (i, [x, y]) in XY.iter().enumerate() {
            let next = (i / 4) * 4 + (i + 1) % 4;
            let [nx, ny] = XY.get(next).copied().unwrap_or([0., 0.]);
            let start = Point3::new(a[0] + a[6] * x, a[1] + a[6] * y, a[2]);
            let end = Point3::new(a[0] + a[6] * nx, a[1] + a[6] * ny, a[2]);
            match BSplineCurve::create_line_segment(3, start, end) {
                Ok(c) => curves.push(Curve::from(c)),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        match failed {
            Some(e) => Err(e),
            None => in_new_brep(k, |k, b| {
                let mut state = ArenaNotifyState::new(b);
                let mut env = state.env(None);
                let mut marks = solidcore::topology::topology_traverser::Marks::default();
                let this_face = |face| arena_identity(CacheObject::Face(face));
                let mut fenv = solidcore::topology::brep::p8::FaceBuildEnv {
                    env: &mut env,
                    marks: &mut marks,
                    this_face: &this_face,
                };
                solidcore::topology::brep::p8::create_planar_face_with_3d_curves(
                    k, b, None, &curves, 1e-5, &mut fenv,
                )
                .map(|_| ())
            })?,
        }
    } else {
        in_new_brep(k, |_, _| Ok(()))?
    };
    let mut b = match finish(k, raw, false, false, &Conventions::EDGEUSE_GRAPH)? {
        Built::Brep(b) => *b,
        Built::Failed(e) => {
            return Output::unported(&e)
                .map(Err)
                .ok_or_else(|| HarnessError::new(format!("rust build: {e}")))
        }
    };
    if let Err(e) = own_edits(&mut b, a, orient_edit) {
        return Output::unported(&e)
            .map(Err)
            .ok_or_else(|| HarnessError::new(format!("rust edit: {e}")));
    }
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        // the face's tolerance, as the face upstream answers GetTolerance with
        b.sync_face_surface_tolerance(id)
            .map_err(|e| HarnessError::new(e.to_string()))?;
    }
    // record the arena naming the build gave the owners
    b.use_arena_naming()
        .map_err(|e| HarnessError::new(e.to_string()))?;
    Ok(Ok(b))
}

/// The shim's edits after its walk (see [`own_brep`]), on the traversal-ordered B-rep.
fn own_edits(b: &mut Brep, a: &[f64; 22], orient_edit: bool) -> Result<()> {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::brep::p2::ArenaNotifyState;
    use solidcore::topology::edge as ed;
    let sel = a[20] as usize;
    let es: Vec<_> = b.edges().ids().collect();
    let fs: Vec<_> = b.faces().ids().collect();
    let e = es.get(sel % es.len().max(1)).copied();
    let f = fs.get(sel % fs.len().max(1)).copied();
    if let Some(e) = e {
        if a[19] >= 0. {
            ed::set_tolerance(b, e, a[19], false, false)?;
        }
        if a[17] != 0. {
            let mut state = ArenaNotifyState::new(b);
            let mut env = state.env(None);
            ed::set_curve(
                b,
                e,
                None,
                ed::SetCurveOptions {
                    delete_old_curve: true,
                    dbg_warn_leaks: false,
                    call_notify: false,
                },
                &mut env,
                [None, None],
            )?;
        }
    }
    if let (true, Some(f), true) = (orient_edit, f, a[18] != 0.) {
        let loops = solidcore::topology::face::p4::upward_loopuses(b, f, false, None)?;
        for lu in loops.iter().rev() {
            let uses = solidcore::topology::loopuse::edgeuses(b, *lu, None, None)?;
            if let Some(&last) = uses.last() {
                solidcore::topology::edgeuse::p1::set_orientation(b, last, OrientType::Unknown)?;
                break;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
struct Object(usize, CacheObject);
impl SolutionObject for Object {
    fn kind(&self) -> ObjectKind {
        self.1.kind()
    }
}

/// The topology solver's view of the two B-reps: each is read live, and the face
/// caches and classifications a solve requests are made on the B-rep itself (as
/// upstream's `smsurf_GetSurfaceCache` and `SmFace::PointClassify` do), through
/// [`LiveBrepObjects`]. `Object(i, ..)` names an object of B-rep `i`.
struct Objects<'a> {
    lives: [LiveBrepObjects<'a, 'a>; 2],
    caches: [BrepCache<Object>; 2],
    /// The two B-reps as they were when the scene was built, for the face queries that
    /// take a `&Brep` (`SmFace::CurveOnClassify` of a curve/face solve,
    /// [`TopologyObjects::face_entity`]): the live B-reps are held mutably. The solves
    /// only add caches, so their topology and geometry stay the snapshot's.
    snapshots: [Brep; 2],
}
impl<'a> Objects<'a> {
    fn new(k: &Kernel, breps: [&'a mut Brep; 2], identities: &'a [BrepIdentity<'a>; 2]) -> Result<Self> {
        let mut caches = [
            BrepCache::new(solidcore::geom::a_object::BREP_TYPE)?,
            BrepCache::new(solidcore::geom::a_object::BREP_TYPE)?,
        ];
        for (i, (cache, brep)) in caches.iter_mut().zip(&breps).enumerate() {
            cache.build_trees(k, CacheSource::Brep(brep), |o| Object(i, o), &arena_identity)?;
        }
        let snapshots = [breps[0].clone(), breps[1].clone()];
        let [b0, b1] = breps;
        let [i0, i1] = identities;
        Ok(Self {
            lives: [
                LiveBrepObjects::new(b0, i0, None)?,
                LiveBrepObjects::new(b1, i1, None)?,
            ],
            caches,
            snapshots,
        })
    }
    fn live(&self, o: Object) -> Result<&LiveBrepObjects<'a, 'a>> {
        self.lives
            .get(o.0)
            .ok_or(SolidError::sm_err("query api scene handle"))
    }
    fn map(o: Object, c: Option<CacheObject>) -> Option<Object> {
        c.map(|c| Object(o.0, c))
    }
}

/// The two B-reps' identities (`BrepId` 0 and 1, the arena identity of their records).
fn identities() -> [BrepIdentity<'static>; 2] {
    let owner_of: &'static dyn Fn(solidcore::geom::object::ObjectRef) -> Option<solidcore::geom::object::ObjectRef> = &|_| None;
    [0, 1].map(|i| BrepIdentity {
        brep_id: BrepId::from_index(i),
        keys: &arena_identity,
        owner_of,
    })
}

impl TopologyObjects<Object> for Objects<'_> {
    fn surface_solve_context(&self) -> Option<Box<dyn SurfaceSolveContext<Object> + '_>> {
        let [l0, l1] = &self.lives;
        Some(Box::new(Contexts([
            l0.surface_solve_context()?,
            l1.surface_solve_context()?,
        ])))
    }
    fn classify_face_point(
        &self,
        k: &Kernel,
        face: Object,
        uv: solidcore::math::Point2,
        tolerance: f64,
        boundaries: bool,
        use_uv: bool,
    ) -> Option<Result<i32>> {
        match self.live(face) {
            Ok(live) => live.classify_face_point(k, face.1, uv, tolerance, boundaries, use_uv),
            Err(e) => Some(Err(e)),
        }
    }
    fn classifies_on_face(
        &self,
        k: &Kernel,
        face: Object,
        point: Point3,
        tolerance: f64,
    ) -> Option<Result<bool>> {
        match self.live(face) {
            Ok(live) => live.classifies_on_face(k, face.1, point, tolerance),
            Err(e) => Some(Err(e)),
        }
    }
    fn face_entity(&self, face: Object) -> Option<(&Brep, FaceId)> {
        let CacheObject::Face(f) = face.1 else {
            return None;
        };
        self.snapshots.get(face.0).map(|b| (b, f))
    }
    fn brep_cache(&self, o: Object) -> Result<&BrepCache<Object>> {
        self.caches
            .get(o.0)
            .ok_or(SolidError::sm_err("query api scene cache"))
    }
    fn curve(&self, o: Object) -> Result<&Curve> {
        self.live(o)?.curve(o.1)
    }
    fn analytic_curve_handle(&self, o: Object) -> Option<Object> {
        Some(o)
    }
    fn vertex_point(&self, o: Object) -> Result<Point3> {
        self.live(o)?.vertex_point(o.1)
    }
    fn vertex_tolerance(&self, o: Object) -> Result<f64> {
        self.live(o)?.vertex_tolerance(o.1)
    }
    fn curve_edge(&self, o: Object) -> Result<Option<Object>> {
        Ok(Self::map(o, self.live(o)?.curve_edge(o.1)?))
    }
    fn edge_curve(&self, o: Object) -> Result<Option<Object>> {
        Ok(Self::map(o, self.live(o)?.edge_curve(o.1)?))
    }
    fn edge_interval(&self, o: Object) -> Result<Extent1d> {
        self.live(o)?.edge_interval(o.1)
    }
    fn edge_tolerance(&self, o: Object) -> Result<f64> {
        self.live(o)?.edge_tolerance(o.1)
    }
    fn surface_face(&self, o: Object) -> Result<Option<Object>> {
        Ok(Self::map(o, self.live(o)?.surface_face(o.1)?))
    }
    fn face_surface(&self, o: Object) -> Result<Object> {
        Ok(Object(o.0, self.live(o)?.face_surface(o.1)?))
    }
    fn surface(&self, o: Object) -> Result<&Surface> {
        self.live(o)?.surface(o.1)
    }
    fn face_uv_domain(&self, o: Object) -> Result<solidcore::geom::extent2d::Extent2d> {
        self.live(o)?.face_uv_domain(o.1)
    }
    fn face_tolerance(&self, o: Object) -> Result<f64> {
        self.live(o)?.face_tolerance(o.1)
    }
    fn surface_owner(&self, o: Object) -> Result<Option<Object>> {
        Ok(Self::map(o, self.live(o)?.surface_owner(o.1)?))
    }
    fn object_zone_tol_3d(&self, k: &Kernel, o: Object) -> Result<f64> {
        self.live(o)?.object_zone_tol_3d(k, o.1)
    }
    fn shape_member(&self, o: Object) -> ShapeMember {
        let b = BrepId::from_index(o.0 as u32);
        match o.1 {
            CacheObject::Vertex(v) => ShapeMember::Vertex((b, v)),
            CacheObject::Edge(e) => ShapeMember::Edge((b, e)),
            CacheObject::Face(f) => ShapeMember::Face((b, f)),
            _ => ShapeMember::Other,
        }
    }
    fn shape_member_object(&self, member: ShapeMember) -> Result<Object> {
        Ok(match member {
            ShapeMember::Vertex((b, v)) => Object(b.index() as usize, CacheObject::Vertex(v)),
            ShapeMember::Edge((b, e)) => Object(b.index() as usize, CacheObject::Edge(e)),
            ShapeMember::Face((b, f)) => Object(b.index() as usize, CacheObject::Face(f)),
            ShapeMember::Other => return Err(SolidError::sm_err("shape handle")),
        })
    }
}

/// The owning-face services of the two B-reps' solves, on `Object` handles.
struct Contexts<'a>([Box<dyn SurfaceSolveContext<CacheObject> + 'a>; 2]);
impl std::fmt::Debug for Contexts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Contexts").finish_non_exhaustive()
    }
}
impl Contexts<'_> {
    fn queries(&self, o: Object) -> Result<&dyn FaceQueries<CacheObject>> {
        self.0
            .get(o.0)
            .and_then(|c| c.face_queries())
            .ok_or(SolidError::sm_err("query api face queries"))
    }
}
impl SurfaceSolveContext<Object> for Contexts<'_> {
    fn build_surface_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<Object>,
    ) -> Result<SurfaceCache<Object>> {
        let Some(Object(i, c)) = handle else {
            return Err(SolidError::sm_err("query api surface handle"));
        };
        let context = self
            .0
            .get_mut(i)
            .ok_or(SolidError::sm_err("query api scene handle"))?;
        context
            .build_surface_cache(k, surface, Some(c))?
            .map_handles(|c| Ok(Object(i, c)))
    }
    fn face_queries(&self) -> Option<&dyn FaceQueries<Object>> {
        Some(self)
    }
}
impl FaceQueries<Object> for Contexts<'_> {
    fn face_zone_tol_3d(&self, face: Object) -> Result<f64> {
        self.queries(face)?.face_zone_tol_3d(face.1)
    }
    fn face_point_classify(
        &self,
        k: &Kernel,
        face: Object,
        uv: solidcore::math::Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<Object>)> {
        let (class, object) = self.queries(face)?.face_point_classify(k, face.1, uv, src_tol)?;
        Ok((class, Objects::map(face, object)))
    }
    fn face_point_classify_fast_drop(
        &self,
        k: &Kernel,
        face: Object,
        uv: solidcore::math::Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<Object>)> {
        let (class, object) =
            self.queries(face)?
                .face_point_classify_fast_drop(k, face.1, uv, src_tol)?;
        Ok((class, Objects::map(face, object)))
    }
}

fn topology(k: &mut Kernel, i: &Inputs, b: &mut Brep, second: &mut Brep) -> Result<Vec<f64>> {
    let a = &i.data;
    let point = v([a[11], a[12], a[13]]);
    let index = a[20] as usize;
    let es: Vec<_> = b.edges().ids().collect();
    let fs: Vec<_> = b.faces().ids().collect();
    let vs: Vec<_> = b.vertices().ids().collect();
    let e = es.get(index % es.len().max(1)).copied();
    let f = fs.get(index % fs.len().max(1)).copied();
    let vert = vs.get(index % vs.len().max(1)).copied();
    let missing = || SolidError::invalid_input("query::api_queries", "null input");
    let mut o = vec![0.];
    match i.op {
        4..=6 => {
            let identities = identities();
            let mut empty = Brep::new();
            let objects = Objects::new(k, [&mut *b, &mut empty], &identities)?;
            let handle = Object(0, CacheObject::Brep);
            match i.op {
                4 => closest(&mut o, api::get_closest_point(k, &objects, handle, point)?),
                5 => closest(&mut o, api::brep_closest_point(k, &objects, handle, point)?),
                _ => {
                    let ps = api::brep_closest_point_all(k, &objects, handle, point)?;
                    o.push(ps.len() as f64);
                    for p in ps {
                        closest(&mut o, p);
                    }
                }
            }
        }
        7 | 8 => {
            // the solve makes the face caches it reaches on the B-reps themselves
            // (upstream's `smsurf_GetSurfaceCache`); the classification then reads them
            let identities = identities();
            let objects = Objects::new(k, [&mut *b, &mut *second], &identities)?;
            let solution = api::brep_relationship_solve(
                k,
                &objects,
                Object(0, CacheObject::Brep),
                Object(1, CacheObject::Brep),
            )?;
            let [l0, l1] = &objects.lives;
            let result = l0.with_brep(|a| {
                l1.with_brep(|b| {
                    api::brep_relationship_classify(
                        k,
                        &objects,
                        &solution,
                        [Object(0, CacheObject::Brep), Object(1, CacheObject::Brep)],
                        a,
                        b,
                    )
                })
            })?;
            if i.op == 7 {
                o.extend([result.distance, f64::from(result.relationship)]);
            } else {
                // `SmApiBrepDistance` is the relationship query's distance
                o.push(result.distance);
            }
        }
        9 => {
            let es = api::get_edges(b)?;
            o.push(es.len() as f64);
            o.extend(es.iter().map(|id| f64::from(id.index())));
        }
        10 => {
            let fs = api::get_faces(b)?;
            o.push(fs.len() as f64);
            o.extend(fs.iter().map(|id| f64::from(id.index())));
        }
        11 => {
            let loops = api::face_get_boundary_loops(b, f.ok_or_else(missing)?)?;
            o.push(loops.edge_counts.len() as f64);
            o.extend(loops.edge_counts.iter().map(|&n| n as f64));
            o.push(loops.edges.len() as f64);
            for (id, orient) in loops.edges.iter().zip(&loops.orientations) {
                o.push(f64::from(id.index()));
                o.push(f64::from(*orient as i32));
            }
        }
        12 => {
            let hit = api::find_edge(k, b, point)?;
            o.push(hit.index as f64);
            o.push(hit.parameter);
        }
        13 => o.push(f64::from(
            api::find_faces(k, b, BrepId::from_index(0), point, &arena_identity, &|_| {
                None
            })?
            .index(),
        )),
        14 => o.push(if api::brep_is_manifold_solid(b)? {
            1.
        } else {
            0.
        }),
        15 | 16 | 20 => {
            let bx = if i.op == 15 {
                api::brep_bounding_box(k, b, a[15] != 0.)?
            } else if i.op == 16 {
                api::face_bounding_box(k, b, f.ok_or_else(missing)?, a[15] != 0.)?
            } else {
                api::edge_bounding_box(k, b, e.ok_or_else(missing)?, a[15] != 0.)?
            };
            vec(&mut o, bx.min());
            vec(&mut o, bx.max());
        }
        17 => vec(
            &mut o,
            api::face_internal_point(k, b, f.ok_or_else(missing)?)?,
        ),
        18 => o.push(f64::from(
            api::face_classify_uv(
                k,
                // (the classification may attach missing UV trim curves: a copy)
                &mut b.clone(),
                f.ok_or_else(missing)?,
                solidcore::math::Point2::new(a[11], a[12]),
            )?
            .code(),
        )),
        19 => {
            let face = f.ok_or_else(missing)?;
            // the shape reads the face from the B-rep before the solve
            let shape = solidcore::geom::object::Shape::new(
                k,
                &(BrepId::from_index(0), &*b),
                &[],
                &[(BrepId::from_index(0), face)],
                &[],
                &[],
            )?;
            let identities = identities();
            if i.live {
                // the library path: a bare live view of the B-rep, its geometry named by
                // its owners' arena identities (`own_brep`)
                let [identity, _] = &identities;
                let objects = LiveBrepObjects::new(b, identity, None)?;
                closest(
                    &mut o,
                    api::face_closest_point_in_shape(
                        k,
                        &shape,
                        &objects,
                        (BrepId::from_index(0), face),
                        point,
                    )?,
                );
            } else {
                let mut empty = Brep::new();
                let objects = Objects::new(k, [&mut *b, &mut empty], &identities)?;
                closest(
                    &mut o,
                    api::face_closest_point_in_shape(
                        k,
                        &shape,
                        &objects,
                        (BrepId::from_index(0), face),
                        point,
                    )?,
                );
            }
        }
        21 => cclosest(
            &mut o,
            api::edge_closest_point(k, b, e.ok_or_else(missing)?, point)?,
        ),
        22 => {
            let d = api::edge_parameter_range(b, e.ok_or_else(missing)?)?;
            o.extend([d.min(), d.max()]);
        }
        23 => o.push(f64::from(
            api::edge_classify_parameter(k, b, e.ok_or_else(missing)?, a[14])?.code(),
        )),
        24 => vec(&mut o, api::edge_tangent(k, b, e.ok_or_else(missing)?, a[14])?),
        27 => {
            if a[18] != 0. {
                // the shim's rewrite of the void flags along the region list
                let ids = solidcore::topology::brep::p1::regions(b, None)?;
                for (n, id) in ids.into_iter().enumerate() {
                    let flag = match (n + index) % 3 {
                        0 => 0,
                        1 => 1,
                        _ => solidcore::topology::brep_data::UNSURE,
                    };
                    b.region_mut(id)?.set_is_void(flag);
                }
            }
            let (solids, voids) = api::brep_material_census(b)?;
            o.extend([solids as f64, voids as f64]);
        }
        25 => vec(&mut o, api::vertex_get_point(b, vert.ok_or_else(missing)?)?),
        _ => return Err(missing()),
    }
    Ok(o)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.api_queries"
    }
    fn run_cpp(&self, c: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Inputs = c.parse()?;
        compared_input(&i)?;
        let mut input = i.data.to_vec();
        if let Some(other) = i.other_box {
            input.extend(other);
        }
        let data = ffi::eval(i.op, &input).map_err(|e| HarnessError::new(e.to_string()))?;
        if let Some(expected) = i.expected_relationship {
            if i.op != 7 || data.first() != Some(&0.) || data.get(2) != Some(&f64::from(expected)) {
                return Err(HarnessError::new(format!(
                    "native relationship fixture expected class {expected}, got {data:?}"
                )));
            }
        }
        Ok(decode(i.op, data))
    }
    fn run_rust(&self, c: &Case) -> std::result::Result<Output, HarnessError> {
        let i: Inputs = c.parse()?;
        compared_input(&i)?;
        let a = &i.data;
        let mut k = Kernel::default();
        let point = v([a[11], a[12], a[13]]);
        let result = match i.op {
            1 => curve(&k, a)
                .and_then(|c| api::curve_closest_point(&k, &c, point))
                .map(|p| {
                    let mut o = vec![0.];
                    cclosest(&mut o, p);
                    o
                }),
            2 => curve(&k, a)
                .and_then(|c| api::curve_closest_point_all(&k, &c, point))
                .map(|ps| {
                    let mut o = vec![0., ps.len() as f64];
                    for p in ps {
                        cclosest(&mut o, p);
                    }
                    o
                }),
            3 => surface(&k, a)
                .and_then(|s| api::surface_closest_point(&k, &s, point))
                .map(|p| {
                    let mut o = vec![0.];
                    closest(&mut o, p.closest);
                    o.push(p.uvs.len() as f64);
                    for uv in p.uvs {
                        o.extend([uv.x, uv.y]);
                    }
                    o
                }),
            _ => {
                let mut b = match own_brep(&mut k, a, i.op == 11)? {
                    Ok(b) => b,
                    Err(o) => return Ok(o),
                };
                let mut other = *a;
                other[0] += 3. * a[6];
                if let Some([x, y, z, size]) = i.other_box {
                    other[..3].copy_from_slice(&[x, y, z]);
                    other[6] = size;
                }
                other[21] = 6.;
                other[17] = 0.;
                let mut second = if i.op == 7 || i.op == 8 {
                    match own_brep(&mut k, &other, false)? {
                        Ok(b) => b,
                        Err(o) => return Ok(o),
                    }
                } else {
                    Brep::new()
                };
                topology(&mut k, &i, &mut b, &mut second)
            }
        };
        Ok(output(i.op, result))
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = 1 + rng.below(25) as i32;
        let kind = if op <= 2 {
            rng.below(4)
        } else if op == 3 {
            *rng.choose(&[4, 5, 8, 9])?
        } else if [11, 16, 17, 18, 19].contains(&op) {
            if rng.below(2) == 0 {
                5
            } else {
                6
            }
        } else {
            rng.below(8)
        };
        let scale = *rng.choose(&[1e-5, 0.25, 1., 100., 1e5])?;
        let mut a = [0.; 22];
        a[0..3].copy_from_slice(&rng.point3(-2. * scale, 2. * scale));
        a[3] = 1.;
        a[4] = 0.3;
        a[5] = 0.7;
        a[6] = scale;
        if kind <= 3 {
            a[7] = 0.;
            a[8] = 1.;
        } else {
            a[7] = 0.;
            a[8] = 0.;
            a[9] = 1.;
            a[10] = 1.;
        }
        a[11..14].copy_from_slice(&rng.point3(-3. * scale, 3. * scale));
        a[14] = rng.uniform(-0.5, 1.5);
        a[15] = f64::from(rng.below(2) as u32);
        a[18] = f64::from(rng.below(2) as u32);
        a[19] = 1e-5;
        a[20] = f64::from(rng.below(15) as u32);
        a[21] = kind as f64;
        serde_json::to_value(Inputs {
            op,
            data: a,
            other_box: None,
            expected_relationship: None,
            live: false,
        })
        .ok()
    }
    fn cases(&self, ctx: &CaseContext) -> std::result::Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(800);
        default_cases(self, &ctx)
    }
}
