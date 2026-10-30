//! Differential coverage of topology p2's local tree traversal.
//!
//! The shared p1 fixture adapter builds the same objects and trees on both sides;
//! the p2 C++ entry reaches the pinned `SmTopologySolver::LocalSolve`.
//!
//! The static face entries (5 to 8: `SmTLSVertexFace`, `SmTLSCurveFace`,
//! `SmTLSEdgeFace`, `SmTLSFaceFace`) run on two planes, or, with `faces`
//! (`static-analytic-faces.json`), on faces of real `SmCylinder` / `SmCone` /
//! `SmSphere` / `SmTorus` surfaces over their natural domains, built as `--ssi-corpus`
//! builds them (the Rust side with [`crate::ssi::kernel::rust_surface`]); each such C++
//! case runs in a fresh process. Their projected operations reach
//! `sm_GetFaceSilhouettes` (unit `intersect.topology_solver_p1`), whose
//! `SM_CAST_PTR(SmBSplineSurface)` accepts every analytic kind. An analytic face's
//! surface is handle 2005 / 2006 (a plane's 2002 / 2003, whose natural domain the
//! handle answers itself); its natural domain comes from the surface.

use super::intersect_topology_solver_p1 as shared;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::Deserialize;
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::error::{EntityRef, Result as SolidResult, SolidError};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::object::{Shape, ShapeMember};
use solidcore::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use solidcore::geom::tree::{NodeGeomType, Tree};
use solidcore::intersect::topology_solver::p1::{CurveRef, TopologyObjects, TopologySolver};
use solidcore::intersect::topology_solver::p2::{TopologySolveOptions, TopologyTrees};
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::surface::surface::p1::SurfaceSolveContext;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::face_context::{
    prepare_face_surface_cache, FaceQueries, PreparedFaceQueries,
};
use solidcore::surface::surface_cache::SurfaceCache;
use solidcore::topology::{Brep, FaceId};
use solidcore_ffi::units::intersect_topology_solver_p2 as ffi;
use std::cell::RefCell;

use crate::ssi::case::SurfaceSpec;

/// Topology solver p2 differential runner.
pub struct Runner;

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.topology_solver_p2"
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        crate::runner::default_cases(
            self,
            &CaseContext {
                seeds: ctx.seeds.max(600),
                ..ctx.clone()
            },
        )
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.inputs.get("op").and_then(Value::as_str) == Some("static") {
            let input = static_input(case)?;
            let i: StaticInputs = case.parse()?;
            let flat = match &i.faces {
                Some([a, b]) => ffi::eval_static_faces(&input, &[a.ffi(), b.ffi()]),
                None => ffi::eval_static(&input),
            };
            return flat
                .map(|v| static_output(&v))
                .map_err(|e| HarnessError::new(e.to_string()));
        }
        let output = if case.inputs.get("op").and_then(Value::as_str) == Some("local") {
            ffi::eval_local(&shared::p2_local_input(case)?)
        } else {
            ffi::eval_trees(&shared::p2_tree_input(case)?)
        };
        output
            .map(|v| shared::p2_tree_output(&v))
            .map_err(|e| HarnessError::new(e.to_string()))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.inputs.get("op").and_then(Value::as_str) == Some("static") {
            return static_rust(case);
        }
        shared::Runner.run_rust(case)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        shared::p2_tree_generate(rng)
    }
}

// Independent fixtures for the static entrypoints. The adapter supplies cache
// trees for real C++ shell-vertex B-reps; this does not port SmBrepCache itself.

#[derive(Deserialize)]
struct StaticInputs {
    #[serde(default)]
    swap: bool,
    #[serde(default)]
    included: bool,
    entry: i32,
    operation: i32,
    request: i32,
    tolerance: f64,
    point: [f64; 3],
    first: Vec<[f64; 3]>,
    second: Vec<[f64; 3]>,
    /// The surfaces of the two faces of entries 5 to 8 (`None`: the default planes).
    #[serde(default)]
    faces: Option<[FaceIn; 2]>,
}

/// A face surface of the static face entries (`ffi::FaceSurface`; angles in degrees).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum FaceIn {
    /// The entry's default plane.
    Plane,
    Cylinder {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        radius: f64,
        angle: [f64; 2],
        height: [f64; 2],
    },
    Cone {
        apex: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        half_angle_deg: f64,
        height: [f64; 2],
    },
    Sphere {
        center: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        radius: f64,
        lon: [f64; 2],
        lat: [f64; 2],
    },
    Torus {
        center: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
        major: [f64; 2],
        minor: [f64; 2],
    },
}

impl FaceIn {
    fn ffi(&self) -> ffi::FaceSurface {
        match self.clone() {
            FaceIn::Plane => ffi::FaceSurface::Plane,
            FaceIn::Cylinder {
                origin,
                x,
                y,
                radius,
                angle,
                height,
            } => ffi::FaceSurface::Cylinder {
                origin,
                x,
                y,
                radius,
                angle,
                height,
            },
            FaceIn::Cone {
                apex,
                x,
                y,
                half_angle_deg,
                height,
            } => ffi::FaceSurface::Cone {
                apex,
                x,
                y,
                half_angle_deg,
                height,
            },
            FaceIn::Sphere {
                center,
                x,
                y,
                radius,
                lon,
                lat,
            } => ffi::FaceSurface::Sphere {
                center,
                x,
                y,
                radius,
                lon,
                lat,
            },
            FaceIn::Torus {
                center,
                x,
                y,
                major_radius,
                minor_radius,
                major,
                minor,
            } => ffi::FaceSurface::Torus {
                center,
                x,
                y,
                major_radius,
                minor_radius,
                major,
                minor,
            },
        }
    }

    /// The `--ssi-corpus` description (`None` for the default plane).
    fn ssi_spec(&self) -> Option<SurfaceSpec> {
        Some(match self.clone() {
            FaceIn::Plane => return None,
            FaceIn::Cylinder {
                origin,
                x,
                y,
                radius,
                angle,
                height,
            } => SurfaceSpec::Cylinder {
                origin,
                xaxis: x,
                yaxis: y,
                radius,
                angle,
                height,
            },
            FaceIn::Cone {
                apex,
                x,
                y,
                half_angle_deg,
                height,
            } => SurfaceSpec::Cone {
                apex,
                xaxis: x,
                yaxis: y,
                half_angle_deg,
                height,
            },
            FaceIn::Sphere {
                center,
                x,
                y,
                radius,
                lon,
                lat,
            } => SurfaceSpec::Sphere {
                center,
                xaxis: x,
                yaxis: y,
                radius,
                lon,
                lat,
            },
            FaceIn::Torus {
                center,
                x,
                y,
                major_radius,
                minor_radius,
                major,
                minor,
            } => SurfaceSpec::Torus {
                center,
                xaxis: x,
                yaxis: y,
                major_radius,
                minor_radius,
                major,
                minor,
            },
        })
    }
}

fn static_best(operation: i32) -> f64 {
    if matches!(operation, 4 | 13 | 15) {
        0.0
    } else {
        1e30
    }
}
fn static_input(case: &Case) -> Result<Vec<f64>, HarnessError> {
    let i: StaticInputs = case.parse()?;
    let mut out = vec![
        f64::from(i.entry),
        f64::from(i.operation),
        f64::from(i.request),
        i.tolerance,
        static_best(i.operation),
        i.first.len() as f64,
        i.second.len() as f64,
    ];
    out.extend(i.point);
    for p in i.first.iter().chain(&i.second) {
        out.extend(p);
    }
    out.push(f64::from(u8::from(i.swap)));
    out.push(f64::from(u8::from(i.included)));
    Ok(out)
}
fn static_output(flat: &[f64]) -> Output {
    Output::new()
        .scalar("status", flat.first().copied().unwrap_or(-1.0))
        .values("static", flat.get(1..).unwrap_or_default().to_vec())
}
/// The shim's `id(object)` of a solution object: the vertex index, `999` for the
/// curve, `-2` for anything else (the faces and surfaces of face/face solves).
fn static_object_id(h: StaticHandle) -> f64 {
    if (0..=999).contains(&h.0) {
        f64::from(h.0)
    } else {
        -2.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
struct StaticHandle(i32);
impl SolutionObject for StaticHandle {
    fn kind(&self) -> ObjectKind {
        match self.0 {
            999 | -2 => ObjectKind::Curve,
            1000 | 1001 | 10_000.. => ObjectKind::Other,
            2000 | 2001 => ObjectKind::Face,
            2002 | 2003 | 2005 | 2006 => ObjectKind::Surface,
            2004 => ObjectKind::Edge,
            _ => ObjectKind::Vertex,
        }
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        (self.0 == 999 || self.0 == -2).then(|| Extent1d::from_min_max(0.0, 1.0))
    }
    fn natural_uv_domain(&self) -> Option<Extent2d> {
        (self.0 == 2002 || self.0 == 2003).then(|| Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0))
    }
}
#[derive(Debug)]
struct StaticScene {
    points: Vec<Point3>,
    first: Tree<StaticHandle>,
    second: Tree<StaticHandle>,
    empty_first: Tree<StaticHandle>,
    empty_second: Tree<StaticHandle>,
    curve: Curve,
    edge_owner: bool,
    surfaces: [Surface; 2],
    /// The handles of the faces' surfaces (2002 / 2003 for a plane, 2005 / 2006 for an
    /// analytic surface) and the faces' UV domains.
    surface_handles: [StaticHandle; 2],
    face_domains: [Extent2d; 2],
    /// The shim's `CreateFaceFromSurface` faces (handles 2000, 2001), each in its
    /// own B-rep as `first` / `second` (entries 5 to 8 only), with their prepared
    /// trimmed surface caches and entity handle tables.
    faces: Vec<(Brep, FaceId)>,
    face_caches: Vec<SurfaceCache<StaticHandle>>,
    face_entities: Vec<RefCell<Vec<EntityRef>>>,
}
struct StaticSurfaceContext<'a>(&'a StaticScene);
impl SurfaceSolveContext<StaticHandle> for StaticSurfaceContext<'_> {
    fn build_surface_cache(
        &mut self,
        _k: &Kernel,
        _surface: &Surface,
        handle: Option<StaticHandle>,
    ) -> SolidResult<SurfaceCache<StaticHandle>> {
        let at = match handle.map(|h| h.0) {
            Some(2002 | 2005) => 0,
            Some(2003 | 2006) => 1,
            _ => return Err(static_error()),
        };
        self.0.face_caches.get(at).cloned().ok_or_else(static_error)
    }
    fn face_queries(&self) -> Option<&dyn FaceQueries<StaticHandle>> {
        Some(self.0)
    }
}
impl StaticScene {
    fn classify_prepared(
        &self,
        k: &Kernel,
        face: StaticHandle,
        uv: Point2,
        src_tol: f64,
        fast_drop: bool,
    ) -> SolidResult<(i32, Option<StaticHandle>)> {
        let at = match face.0 {
            2000 => 0,
            2001 => 1,
            _ => return Err(static_error()),
        };
        let (b, f) = self.faces.get(at).ok_or_else(static_error)?;
        let entities = self.face_entities.get(at).ok_or_else(static_error)?;
        let base = 10_000 * (at as i32 + 1);
        let handle_of = |entity: EntityRef| -> SolidResult<StaticHandle> {
            if entity == EntityRef::Face(*f) {
                return Ok(face);
            }
            let mut table = entities.borrow_mut();
            if let Some(idx) = table.iter().position(|&e| e == entity) {
                return Ok(StaticHandle(base + idx as i32));
            }
            let idx = table.len() as i32;
            table.push(entity);
            Ok(StaticHandle(base + idx))
        };
        let prepared = PreparedFaceQueries::new(b, *f, &handle_of)?;
        if fast_drop {
            prepared.face_point_classify_fast_drop(k, face, uv, src_tol)
        } else {
            prepared.face_point_classify(k, face, uv, src_tol)
        }
    }
}
impl FaceQueries<StaticHandle> for StaticScene {
    fn face_zone_tol_3d(&self, face: StaticHandle) -> SolidResult<f64> {
        self.face_tolerance(face)
    }
    fn face_point_classify(
        &self,
        k: &Kernel,
        face: StaticHandle,
        uv: Point2,
        src_tol: f64,
    ) -> SolidResult<(i32, Option<StaticHandle>)> {
        self.classify_prepared(k, face, uv, src_tol, false)
    }
    fn face_point_classify_fast_drop(
        &self,
        k: &Kernel,
        face: StaticHandle,
        uv: Point2,
        src_tol: f64,
    ) -> SolidResult<(i32, Option<StaticHandle>)> {
        self.classify_prepared(k, face, uv, src_tol, true)
    }
}
fn static_error() -> SolidError {
    SolidError::invalid_input(
        "difftest topology p2 static adapter",
        "object absent from shell-vertex fixture",
    )
}
fn static_tree(
    points: &[Point3],
    offset: usize,
    tolerance: f64,
) -> SolidResult<Tree<StaticHandle>> {
    let mut bbox = Extent3d::new();
    for point in points {
        let mut b = Extent3d::from_point(*point);
        b.expand_absolute(tolerance);
        bbox = bbox.union(&b);
    }
    bbox.expand_absolute(tolerance * 100.0);
    let size = bbox.size();
    bbox.add_point(bbox.max() + size * 0.15);
    bbox.add_point(bbox.min() - size * 0.15);
    let mut tree = Tree::new_spatial(&bbox);
    for (index, point) in points.iter().enumerate() {
        let mut b = Extent3d::from_point(*point);
        b.expand_absolute(tolerance);
        tree.add_to_spatial_tree(
            &b,
            Some(StaticHandle((offset + index) as i32)),
            NodeGeomType::Default,
        )?;
    }
    Ok(tree)
}
impl TopologyObjects<StaticHandle> for StaticScene {
    fn surface_solve_context(&self) -> Option<Box<dyn SurfaceSolveContext<StaticHandle> + '_>> {
        (!self.face_caches.is_empty())
            .then(|| Box::new(StaticSurfaceContext(self)) as Box<dyn SurfaceSolveContext<StaticHandle> + '_>)
    }
    fn brep_trees(&self, brep: StaticHandle) -> SolidResult<TopologyTrees<'_, StaticHandle>> {
        let (vertices, empty) = match brep.0 {
            1000 => (&self.first, &self.empty_first),
            1001 => (&self.second, &self.empty_second),
            _ => return Err(static_error()),
        };
        Ok(TopologyTrees {
            vertices,
            curves: empty,
            surfaces: empty,
        })
    }
    fn curve(&self, object: StaticHandle) -> SolidResult<&Curve> {
        if object.0 == 999 {
            Ok(&self.curve)
        } else {
            Err(static_error())
        }
    }
    fn analytic_curve_handle(&self, _: StaticHandle) -> Option<StaticHandle> {
        Some(StaticHandle(-2))
    }
    fn vertex_point(&self, vertex: StaticHandle) -> SolidResult<Point3> {
        usize::try_from(vertex.0)
            .ok()
            .and_then(|i| self.points.get(i))
            .copied()
            .ok_or_else(static_error)
    }
    fn vertex_tolerance(&self, vertex: StaticHandle) -> SolidResult<f64> {
        self.vertex_point(vertex)?;
        Ok(1e-5)
    }
    fn curve_edge(&self, _: StaticHandle) -> SolidResult<Option<StaticHandle>> {
        Ok(self.edge_owner.then_some(StaticHandle(2004)))
    }
    fn edge_curve(&self, edge: StaticHandle) -> SolidResult<Option<StaticHandle>> {
        if self.edge_owner && edge.0 == 2004 {
            Ok(Some(StaticHandle(999)))
        } else {
            Err(static_error())
        }
    }
    fn edge_interval(&self, edge: StaticHandle) -> SolidResult<Extent1d> {
        self.edge_curve(edge)?;
        Ok(Extent1d::from_min_max(0.0, 1.0))
    }
    fn edge_tolerance(&self, edge: StaticHandle) -> SolidResult<f64> {
        self.edge_curve(edge)?;
        Ok(1e-5)
    }
    fn surface_face(&self, surface: StaticHandle) -> SolidResult<Option<StaticHandle>> {
        match surface.0 {
            2002 | 2005 => Ok(Some(StaticHandle(2000))),
            2003 | 2006 => Ok(Some(StaticHandle(2001))),
            _ => Err(static_error()),
        }
    }
    fn face_surface(&self, face: StaticHandle) -> SolidResult<StaticHandle> {
        match face.0 {
            2000 => Ok(self.surface_handles[0]),
            2001 => Ok(self.surface_handles[1]),
            _ => Err(static_error()),
        }
    }
    fn surface(&self, surface: StaticHandle) -> SolidResult<&Surface> {
        let at = match surface.0 {
            2002 | 2005 => 0,
            2003 | 2006 => 1,
            _ => return Err(static_error()),
        };
        if let Some((b, f)) = self.faces.get(at) {
            return b.face(*f)?.surface.as_ref().ok_or_else(static_error);
        }
        self.surfaces.get(at).ok_or_else(static_error)
    }
    fn face_uv_domain(&self, face: StaticHandle) -> SolidResult<Extent2d> {
        self.face_surface(face)?;
        Ok(if face.0 == 2000 {
            self.face_domains[0]
        } else {
            self.face_domains[1]
        })
    }
    fn face_tolerance(&self, face: StaticHandle) -> SolidResult<f64> {
        self.face_surface(face)?;
        Ok(1e-5)
    }
    fn face_entity(&self, face: StaticHandle) -> Option<(&Brep, FaceId)> {
        let at = match face.0 {
            2000 => 0,
            2001 => 1,
            _ => return None,
        };
        self.faces.get(at).map(|(b, f)| (b, *f))
    }
    fn surface_owner(&self, surface: StaticHandle) -> SolidResult<Option<StaticHandle>> {
        self.surface_face(surface)
    }
    fn shape_member(&self, _: StaticHandle) -> ShapeMember {
        ShapeMember::Other
    }
    // SmTol::GetZoneTol3d(const SmObject*) of the shim's objects: topology's own
    // GetTolerance, a curve's edge's or a surface's face's tolerance (the system's
    // without one, as for the analytic curve), a B-rep's tolerance
    fn object_zone_tol_3d(&self, k: &Kernel, object: StaticHandle) -> SolidResult<f64> {
        let system = || {
            solidcore::geom::tol::zone_tol_3d(k, solidcore::geom::tol::SmallTopoType::NormalSize)
        };
        match object.0 {
            1000 | 1001 => Ok(1e-5),
            2000 | 2001 => self.face_tolerance(object),
            2002 | 2003 | 2005 | 2006 => match self.surface_face(object)? {
                Some(face) => self.face_tolerance(face),
                None => Ok(system()),
            },
            2004 => self.edge_tolerance(object),
            999 => match self.curve_edge(object)? {
                Some(edge) => self.edge_tolerance(edge),
                None => Ok(system()),
            },
            -2 => Ok(system()),
            _ => self.vertex_tolerance(object),
        }
    }
}
fn static_rust(case: &Case) -> Result<Output, HarnessError> {
    let input = static_input(case)?;
    // Validate the Rust fixture independently against the native protocol guard.
    // Do not invoke C++ here: the two sides must construct their own objects.
    let i: StaticInputs = case.parse()?;
    if input.len() > 396
        || i.first.is_empty()
        || i.second.is_empty()
        || i.first.len() > 64
        || i.second.len() > 64
    {
        return Err(HarnessError::new("unbounded static fixture"));
    }
    // ffi::eval_static_faces: face surfaces only for the face entries
    if i.faces.is_some() && !(5..=8).contains(&i.entry) {
        return Err(HarnessError::new("face surfaces for a non-face entry"));
    }
    let mut kernel = Kernel::default();
    let first: Vec<Point3> = i
        .first
        .iter()
        .map(|p| Point3::new(p[0], p[1], p[2]))
        .collect();
    let second: Vec<Point3> = i
        .second
        .iter()
        .map(|p| Point3::new(p[0], p[1], p[2]))
        .collect();
    let mut all = first.clone();
    all.extend(&second);
    let tree1 = static_tree(&first, 0, 1e-5).map_err(|e| HarnessError::new(e.to_string()))?;
    let tree2 =
        static_tree(&second, first.len(), 1e-5).map_err(|e| HarnessError::new(e.to_string()))?;
    let empty_first = Tree::new_spatial(
        &tree1
            .node(
                tree1
                    .top_node()
                    .ok_or_else(|| HarnessError::new("missing fixture root"))?,
            )
            .map_err(|e| HarnessError::new(e.to_string()))?
            .bbox,
    );
    let empty_second = Tree::new_spatial(
        &tree2
            .node(
                tree2
                    .top_node()
                    .ok_or_else(|| HarnessError::new("missing fixture root"))?,
            )
            .map_err(|e| HarnessError::new(e.to_string()))?
            .bbox,
    );
    let nurb = NlCurve::new(
        vec![
            CPoint::new(-1.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 0.0, 0.0, NOW),
        ],
        1,
        vec![0.0, 0.0, 1.0, 1.0],
    )
    .map_err(|e| HarnessError::new(e.to_string()))?;
    let planes = [0.0, 1.0].map(|z| {
        Surface::from(solidcore::surface::plane::Plane::new(
            Point3::new(0.0, 0.0, z),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            solidcore::geom::vector2d::Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ))
    });
    let mut faces = Vec::new();
    let mut face_caches = Vec::new();
    let mut face_entities = Vec::new();
    let unit = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
    let mut surface_handles = [StaticHandle(2002), StaticHandle(2003)];
    let mut face_domains = [unit, unit];
    if i.entry >= 5 {
        // the shim's face_surface: the default plane, or the analytic surface (its NURB
        // made, as the shim's GetNaturalUVDomain() makes it) on its natural domain
        let mut face_surfaces = planes.clone();
        if let Some(specs) = &i.faces {
            for (which, spec) in specs.iter().enumerate() {
                let Some(spec) = spec.ssi_spec() else {
                    continue;
                };
                let srf = match crate::ssi::kernel::rust_surface(&kernel, &spec) {
                    Ok(Ok(srf)) => srf,
                    Ok(Err(d)) => return Ok(Output::deferred(d.unit, d.symbol)),
                    Err(e) => return Err(HarnessError::new(format!("analytic face: {e}"))),
                };
                let dom = match srf.natural_uv_domain_with_nurb(&kernel) {
                    Ok(d) => d,
                    Err(e) => {
                        return match Output::unported(&e) {
                            Some(d) => Ok(d),
                            None => Err(HarnessError::new(format!("natural domain: {e}"))),
                        }
                    }
                };
                if let (Some(slot), Some(d), Some(h)) = (
                    face_surfaces.get_mut(which),
                    face_domains.get_mut(which),
                    surface_handles.get_mut(which),
                ) {
                    *slot = srf;
                    *d = dom;
                    *h = StaticHandle(2005 + which as i32);
                }
            }
        }
        for (which, plane) in face_surfaces.iter().enumerate() {
            // new SmBrep(SM_USE_DEFAULT, SM_USE_DEFAULT, &context)
            let mut b = super::blend_fillet_geom_p1::new_brep(&mut kernel)?;
            let f = super::sweep_trans_sweep_geometry::create_face(
                &mut kernel,
                &mut b,
                plane.clone(),
                face_domains.get(which).unwrap_or(&unit),
            )
            .map_err(|e| HarnessError::new(format!("CreateFaceFromSurface: {e}")))?;
            let face_handle = StaticHandle(2000 + which as i32);
            let surface_handle = surface_handles
                .get(which)
                .copied()
                .unwrap_or(StaticHandle(2002 + which as i32));
            let base = 10_000 * (which as i32 + 1);
            let entities = RefCell::new(Vec::new());
            let handle_of = |entity: EntityRef| -> SolidResult<StaticHandle> {
                if entity == EntityRef::Face(f) {
                    return Ok(face_handle);
                }
                let mut table = entities.borrow_mut();
                if let Some(idx) = table.iter().position(|&e| e == entity) {
                    return Ok(StaticHandle(base + idx as i32));
                }
                let idx = table.len() as i32;
                table.push(entity);
                Ok(StaticHandle(base + idx))
            };
            let entity_of = |handle: StaticHandle| -> SolidResult<EntityRef> {
                if handle == face_handle {
                    return Ok(EntityRef::Face(f));
                }
                let idx = usize::try_from(handle.0 - base).map_err(|_| static_error())?;
                entities.borrow().get(idx).copied().ok_or_else(static_error)
            };
            let cache = prepare_face_surface_cache(
                &kernel,
                &mut b,
                f,
                surface_handle,
                &handle_of,
                &entity_of,
            )
            .map_err(|e| HarnessError::new(format!("prepare_face_surface_cache: {e}")))?;
            faces.push((b, f));
            face_caches.push(cache);
            face_entities.push(entities);
        }
    }
    let scene = StaticScene {
        points: all,
        first: tree1,
        second: tree2,
        empty_first,
        empty_second,
        curve: Curve::from(BSplineCurve::from_nurb(nurb, 3, false)),
        edge_owner: i.entry == 7,
        surfaces: planes,
        surface_handles,
        face_domains,
        faces,
        face_caches,
        face_entities,
    };
    let vectors = [Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)];
    let options = TopologySolveOptions {
        operation: SolverOperation::from_code(i.operation)
            .ok_or_else(|| HarnessError::new("operation"))?,
        requested: SolutionRequested::from_code(i.request)
            .ok_or_else(|| HarnessError::new("request"))?,
        tolerance_3d: i.tolerance,
        best_answer_so_far_sq: static_best(i.operation),
        opt_vectors: &vectors,
    };
    let point = Point3::new(i.point[0], i.point[1], i.point[2]);
    let curve = CurveRef {
        curve: &scene.curve,
        handle: StaticHandle(999),
    };
    let interval = Extent1d::from_min_max(0.0, 1.0);
    let shape = Shape::new(&mut kernel, &solidcore::geom::object::NoBreps, &[], &[], &[], &[])
        .map_err(|e| HarnessError::new(e.to_string()))?;
    let result = match i.entry {
        0 => TopologySolver::brep_point_solve(
            &kernel,
            &scene,
            Some(StaticHandle(1000)),
            point,
            options,
        ),
        1 => TopologySolver::brep_curve_solve(
            &kernel,
            &scene,
            Some(StaticHandle(1000)),
            curve,
            &interval,
            options,
        ),
        2 => TopologySolver::brep_brep_solve(
            &kernel,
            &scene,
            Some(StaticHandle(1000)),
            Some(StaticHandle(1001)),
            options,
        ),
        3 => TopologySolver::shape_point_solve(&kernel, &scene, &shape, point, options),
        4 => TopologySolver::shape_curve_solve(&kernel, &scene, &shape, curve, &interval, options),
        5..=8 => {
            use solidcore::intersect::topology_solver::p1::{LocalSolver, LocalSolverKind};
            let mut ts = TopologySolver::new(
                &kernel,
                &scene,
                options.operation,
                options.requested,
                options.tolerance_3d,
                options.best_answer_so_far_sq,
                options.opt_vectors,
            );
            let kind = match i.entry {
                5 => LocalSolverKind::VertexFace,
                6 => LocalSolverKind::CurveFace,
                7 => LocalSolverKind::EdgeFace,
                _ => LocalSolverKind::FaceFace,
            };
            let mut local =
                LocalSolver::new(kind, i.swap, if i.included { None } else { Some(&shape) });
            if i.entry == 6 {
                local.set_curve(Some(curve), interval);
            }
            let [s0, s1] = scene.surface_handles;
            let (first, second) = match i.entry {
                5 => (StaticHandle(0), s0),
                6 | 7 => (StaticHandle(999), s0),
                _ => (s0, s1),
            };
            let (first, second) = if i.swap {
                (second, first)
            } else {
                (first, second)
            };
            local
                .solve_it(&kernel, &mut ts, Some(first), Some(second))
                .map(|()| ts.base.solutions)
        }
        _ => return Err(HarnessError::new("static entry")),
    };
    let mut flat = vec![0.0, 1e-5, 1e-5];
    flat.extend(scene.points.iter().map(|_| 1e-5));
    match result {
        Ok(solutions) => {
            flat.push(solutions.len() as f64);
            for s in solutions.as_slice() {
                flat.extend([
                    f64::from(s.solution_type.code()),
                    s.num_variables as f64,
                    s.num_objects as f64,
                    s.objects[0].map_or(-1.0, static_object_id),
                    s.objects[1].map_or(-1.0, static_object_id),
                    s.start.solution_value,
                ]);
                flat.extend(s.start.parameters.iter().take(s.num_variables).copied());
                if s.solution_type == SolutionType::RangeOfValues {
                    flat.push(s.end.solution_value);
                    flat.extend(s.end.parameters.iter().take(s.num_variables).copied());
                }
                flat.extend(
                    s.nodes
                        .iter()
                        .map(|node| f64::from(u8::from(node.is_some()))),
                );
            }
        }
        Err(e) => {
            if let Some(deferred) = Output::unported(&e) {
                return Ok(deferred);
            }
            let status = match e {
                SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
                SolidError::Upstream { code, .. } => f64::from(code),
                _ => return Err(HarnessError::new(e.to_string())),
            };
            if let Some(slot) = flat.first_mut() {
                *slot = status;
            }
            flat.push(0.0);
        }
    }
    Ok(static_output(&flat))
}
