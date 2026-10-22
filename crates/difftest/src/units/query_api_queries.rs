//! Public `SmApi` queries against real native geometry. Topology inputs are imported
//! from the established raw graph exporter; all query results come from `SmApi*`.
//! Analytic wire curves are reconstructed as their concrete Rust kinds after import.
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
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep_cache::BrepCache;
use solidcore::topology::brep_cache::{arena_identity, CacheObject, CacheSource};
use solidcore::topology::{Brep, BrepId, EdgeId, FaceId, FaceuseId, VertexId};
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
        SolidError::InvalidInput { .. } => 1007.,
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

/// The case ops are `1..=25`; the FFI's `0` and `26` are the graph exports
/// [`imported`] reads the native B-rep through, which have no Rust counterpart to
/// compare.
fn compared_op(op: i32) -> std::result::Result<(), HarnessError> {
    if (1..=25).contains(&op) {
        Ok(())
    } else {
        Err(HarnessError::new(format!("op {op} is not a compared query (1..=25)")))
    }
}

// Integer topology and status fields are compared exactly; only geometric
// measurements use the harness's fixed relative tolerance.
fn decode(op: i32, values: Vec<f64>) -> Output {
    let mut ints = Vec::new();
    let mut floats = Vec::new();
    for (i, value) in values.into_iter().enumerate() {
        let integer = i == 0
            || matches!(op, 9 | 10 | 11 | 13 | 14 | 18 | 23)
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
    if a[21] == 4. {
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

fn imported(k: &Kernel, a: &[f64; 22], export_op: i32) -> std::result::Result<Brep, HarnessError> {
    let raw = ffi::eval(export_op, a).map_err(|e| HarnessError::new(e.to_string()))?;
    let counts = solidcore_ffi::units::topology_edgeuse_p1::counts(&raw)
        .ok_or_else(|| HarnessError::new("invalid topology export"))?;
    let count = counts
        .get(3)
        .copied()
        .ok_or_else(|| HarnessError::new("missing face count"))?;
    // the B-rep's face, edge and vertex lists in their list order follow the metadata
    let (n_e, n_v) = (
        counts
            .get(7)
            .copied()
            .ok_or_else(|| HarnessError::new("missing edge count"))?,
        counts
            .get(9)
            .copied()
            .ok_or_else(|| HarnessError::new("missing vertex count"))?,
    );
    let lists_len = 3 + count + n_e + n_v;
    let lists_at = raw
        .len()
        .checked_sub(lists_len)
        .ok_or_else(|| HarnessError::new("missing list orders"))?;
    let (raw, lists) = raw.split_at(lists_at);
    let tail = count
        .checked_mul(2)
        .ok_or_else(|| HarnessError::new("face metadata overflow"))?;
    let split = raw
        .len()
        .checked_sub(tail)
        .ok_or_else(|| HarnessError::new("missing face metadata"))?;
    let (graph, metadata) = raw.split_at(split);
    let (mut b, types) = crate::units::topology_edgeuse_p1::import_with_types(graph)?;
    {
        let mut at = 0usize;
        let mut read_list = |expect: usize| -> std::result::Result<Vec<u32>, HarnessError> {
            let n = lists.get(at).copied().unwrap_or(-1.0);
            if n < 0.0 || n as usize != expect {
                return Err(HarnessError::new("list order length mismatch"));
            }
            let v: Vec<u32> = lists
                .get(at + 1..at + 1 + expect)
                .ok_or_else(|| HarnessError::new("short list order"))?
                .iter()
                .map(|&i| i as u32)
                .collect();
            at += 1 + expect;
            Ok(v)
        };
        let fl = read_list(count)?;
        let el = read_list(n_e)?;
        let vl = read_list(n_v)?;
        b.set_face_list(fl.into_iter().map(FaceId::from_index).collect())
            .map_err(|e| HarnessError::new(format!("face list: {e}")))?;
        b.set_edge_list(el.into_iter().map(EdgeId::from_index).collect())
            .map_err(|e| HarnessError::new(format!("edge list: {e}")))?;
        b.set_vertex_list(vl.into_iter().map(VertexId::from_index).collect())
            .map_err(|e| HarnessError::new(format!("vertex list: {e}")))?;
    }
    let faces: Vec<_> = b.faces().ids().collect();
    let (metadata, remainder) = metadata.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(HarnessError::new("incomplete face metadata"));
    }
    for (id, &[fu, rect]) in faces.iter().zip(metadata) {
        let face = b
            .face_mut(*id)
            .map_err(|e| HarnessError::new(e.to_string()))?;
        face.faceuse = (fu >= 0.).then(|| FaceuseId::from_index(fu as u32));
        face.rectangular_trim = rect as i32;
    }
    if (a[21] <= 3. || a[21] == 10.) && a[17] == 0. {
        let ids: Vec<_> = b.edges().ids().collect();
        for id in ids {
            b.edge_mut(id)
                .map_err(|e| HarnessError::new(e.to_string()))?
                .curve = Some(curve(k, a).map_err(|e| HarnessError::new(e.to_string()))?);
        }
    }
    // Reject a lossy import instead of comparing a virtual call on a stand-in.
    for ((_, edge), &native) in b.edges().iter().zip(&types.edge_curve_types) {
        let rust = edge.curve.as_ref().map_or(-1, Curve::type_code);
        if rust != native {
            return Err(HarnessError::new(format!(
                "native edge curve type {native} imported as {rust}"
            )));
        }
    }
    for ((_, face), &native) in b.faces().iter().zip(&types.face_surface_types) {
        let rust = face.surface.as_ref().map_or(-1, Surface::type_code);
        if rust != native {
            return Err(HarnessError::new(format!(
                "native face surface type {native} imported as {rust}"
            )));
        }
    }
    let edges: Vec<_> = b.edges().ids().collect();
    for id in edges {
        if let Some(curve) = b
            .edge_mut(id)
            .map_err(|e| HarnessError::new(e.to_string()))?
            .curve
            .as_mut()
        {
            let _ = curve.set_owner(
                Some(arena_identity(CacheObject::Edge(id))),
                Some(arena_identity(CacheObject::Brep)),
            );
        }
    }
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        if let Some(surface) = b
            .face_mut(id)
            .map_err(|e| HarnessError::new(e.to_string()))?
            .surface
            .as_mut()
        {
            let _ = surface.set_owner(Some(arena_identity(CacheObject::Face(id))));
        }
        // the face's tolerance, as the face upstream answers GetTolerance with
        b.sync_face_surface_tolerance(id)
            .map_err(|e| HarnessError::new(e.to_string()))?;
    }
    Ok(b)
}

#[derive(Clone, Copy, PartialEq)]
struct Object(usize, CacheObject);
impl SolutionObject for Object {
    fn kind(&self) -> ObjectKind {
        self.1.kind()
    }
}

struct Objects<'a> {
    breps: [&'a Brep; 2],
    caches: [BrepCache<Object>; 2],
}
impl<'a> Objects<'a> {
    fn new(breps: [&'a Brep; 2]) -> Result<Self> {
        Ok(Self {
            breps,
            caches: [
                BrepCache::new(solidcore::geom::a_object::BREP_TYPE)?,
                BrepCache::new(solidcore::geom::a_object::BREP_TYPE)?,
            ],
        })
    }
    fn build(&mut self, k: &Kernel) -> Result<()> {
        for (i, (cache, brep)) in self.caches.iter_mut().zip(&self.breps).enumerate() {
            cache.build_trees(
                k,
                CacheSource::Brep(brep),
                |o| Object(i, o),
                &arena_identity,
            )?;
        }
        Ok(())
    }
    fn b(&self, o: Object) -> Result<&Brep> {
        self.breps
            .get(o.0)
            .copied()
            .ok_or(SolidError::sm_err("query api scene handle"))
    }
}
impl TopologyObjects<Object> for Objects<'_> {
    fn brep_cache(&self, o: Object) -> Result<&BrepCache<Object>> {
        self.caches
            .get(o.0)
            .ok_or(SolidError::sm_err("query api scene cache"))
    }
    fn curve(&self, o: Object) -> Result<&Curve> {
        let CacheObject::Curve(e) = o.1 else {
            return Err(SolidError::sm_err("curve handle"));
        };
        self.b(o)?
            .edge(e)?
            .curve
            .as_ref()
            .ok_or(SolidError::null_pointer("curve"))
    }
    fn analytic_curve_handle(&self, o: Object) -> Option<Object> {
        Some(o)
    }
    fn vertex_point(&self, o: Object) -> Result<Point3> {
        let CacheObject::Vertex(v) = o.1 else {
            return Err(SolidError::sm_err("vertex handle"));
        };
        Ok(self.b(o)?.vertex(v)?.point)
    }
    fn vertex_tolerance(&self, o: Object) -> Result<f64> {
        let CacheObject::Vertex(v) = o.1 else {
            return Err(SolidError::sm_err("vertex handle"));
        };
        Ok(self.b(o)?.vertex(v)?.tolerance)
    }
    fn curve_edge(&self, o: Object) -> Result<Option<Object>> {
        let Some(owner) = self.curve(o)?.edge(&|_| None)? else {
            return Ok(None);
        };
        Ok(self
            .b(o)?
            .edges()
            .ids()
            .find(|e| arena_identity(CacheObject::Edge(*e)) == owner)
            .map(|e| Object(o.0, CacheObject::Edge(e))))
    }
    fn edge_curve(&self, o: Object) -> Result<Option<Object>> {
        let CacheObject::Edge(e) = o.1 else {
            return Err(SolidError::sm_err("edge handle"));
        };
        Ok(self
            .b(o)?
            .edge(e)?
            .curve
            .as_ref()
            .map(|_| Object(o.0, CacheObject::Curve(e))))
    }
    fn edge_interval(&self, o: Object) -> Result<Extent1d> {
        let CacheObject::Edge(e) = o.1 else {
            return Err(SolidError::sm_err("edge handle"));
        };
        Ok(self.b(o)?.edge(e)?.interval)
    }
    fn surface_face(&self, o: Object) -> Result<Option<Object>> {
        let Some(owner) = self.surface(o)?.face()? else {
            return Ok(None);
        };
        Ok(self
            .b(o)?
            .faces()
            .ids()
            .find(|f| arena_identity(CacheObject::Face(*f)) == owner)
            .map(|f| Object(o.0, CacheObject::Face(f))))
    }
    fn face_surface(&self, o: Object) -> Result<Object> {
        let CacheObject::Face(f) = o.1 else {
            return Err(SolidError::sm_err("face handle"));
        };
        self.b(o)?
            .face(f)?
            .surface
            .as_ref()
            .ok_or(SolidError::null_pointer("surface"))?;
        Ok(Object(o.0, CacheObject::Surface(f)))
    }
    fn surface(&self, o: Object) -> Result<&Surface> {
        let CacheObject::Surface(f) = o.1 else {
            return Err(SolidError::sm_err("surface handle"));
        };
        self.b(o)?
            .face(f)?
            .surface
            .as_ref()
            .ok_or(SolidError::null_pointer("surface"))
    }
    fn face_uv_domain(&self, o: Object) -> Result<solidcore::geom::extent2d::Extent2d> {
        let CacheObject::Face(f) = o.1 else {
            return Err(SolidError::sm_err("face handle"));
        };
        Ok(self.b(o)?.face(f)?.uv_domain)
    }
    fn face_tolerance(&self, o: Object) -> Result<f64> {
        let CacheObject::Face(f) = o.1 else {
            return Err(SolidError::sm_err("face handle"));
        };
        Ok(self.b(o)?.face(f)?.tolerance)
    }
    fn surface_owner(&self, o: Object) -> Result<Option<Object>> {
        self.surface_face(o)
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

fn topology(k: &mut Kernel, i: &Inputs, b: &Brep, second: &Brep) -> Result<Vec<f64>> {
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
        4..=8 => {
            let mut objects = Objects::new([b, second])?;
            objects.build(k)?;
            let handle = Object(0, CacheObject::Brep);
            match i.op {
                4 => closest(&mut o, api::get_closest_point(k, &objects, handle, point)?),
                5 => closest(&mut o, api::brep_closest_point(k, &objects, handle, point)?),
                6 => {
                    let ps = api::brep_closest_point_all(k, &objects, handle, point)?;
                    o.push(ps.len() as f64);
                    for p in ps {
                        closest(&mut o, p);
                    }
                }
                _ => {
                    let a = api::BrepRef { brep: b, handle };
                    let b = api::BrepRef {
                        brep: second,
                        handle: Object(1, CacheObject::Brep),
                    };
                    if i.op == 7 {
                        let result = api::brep_relationship(k, &objects, a, b)?;
                        o.extend([result.distance, f64::from(result.relationship)]);
                    } else {
                        o.push(api::brep_distance(k, &objects, a, b)?);
                    }
                }
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
            let mut objects = Objects::new([b, second])?;
            objects.build(k)?;
            closest(
                &mut o,
                api::face_closest_point(
                    k,
                    &(BrepId::from_index(0), b),
                    &objects,
                    (BrepId::from_index(0), face),
                    point,
                )?,
            );
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
        compared_op(i.op)?;
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
        compared_op(i.op)?;
        let a = &i.data;
        let mut k = Kernel::default();
        if a[16] != 0. {
            return Err(HarnessError::new(
                "null pointers have no typed Rust equivalent; use the FFI contract tests",
            ));
        }
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
                let b = imported(&k, a, if i.op == 11 { 26 } else { 0 })?;
                let mut other = *a;
                other[0] += 3. * a[6];
                if let Some([x, y, z, size]) = i.other_box {
                    other[..3].copy_from_slice(&[x, y, z]);
                    other[6] = size;
                }
                other[21] = 6.;
                other[17] = 0.;
                let second = if i.op == 7 || i.op == 8 {
                    imported(&k, &other, 0)?
                } else {
                    Brep::new()
                };
                topology(&mut k, &i, &b, &second)
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
        })
        .ok()
    }
    fn cases(&self, ctx: &CaseContext) -> std::result::Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(800);
        default_cases(self, &ctx)
    }
}
