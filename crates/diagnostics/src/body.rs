//! The body as the kernel reads it: its faces, its edges with their polylines, and its
//! display mesh.

use std::path::Path;

use solidcore::boolean::api_brep::tessellate_boundaries;
use solidcore::geom::attribute::AttributeStore;
use solidcore::query::edge::edge_faces;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::{p1, p10::TopologyEnv, p7};
use solidcore::topology::database_io::FileType;
use solidcore::topology::loopuse;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// A body read from a file, with the kernel that owns its geometry.
pub struct Body {
    pub kernel: Kernel,
    pub brep: Brep,
}

/// One face: its index in the kernel's face arena, its surface's kind and its tolerance.
pub struct FaceInfo {
    pub index: usize,
    pub surface: &'static str,
    pub tolerance: f64,
}

/// One edge: its index, its tolerance, the faces it bounds and its curve as a polyline.
pub struct EdgeInfo {
    pub index: usize,
    pub tolerance: f64,
    pub faces: Vec<usize>,
    pub points: Vec<[f64; 3]>,
}

/// The display mesh, each triangle tagged with its kernel face.
pub struct Mesh {
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub triangle_face: Vec<usize>,
    /// Kernel faces the mesher refused, with its reason.
    pub refused: Vec<(usize, String)>,
    /// The size the mesh tolerance was chosen for (the edges' bounding-box diagonal).
    pub size: f64,
}

/// Reads `path` as `SmBrep::ReadFromFile` does, into a B-rep that names its geometry by
/// arena identity, as a kernel-built one does.
pub fn read(path: &Path, binary: bool, rebuild: bool) -> Result<Body, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut kernel = Kernel::default();
    let fail = |e: solidcore::SolidError| format!("reading {}: {e}", path.display());
    let mut brep = Brep::new_with_infinite_region().map_err(fail)?;
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    let kind = if binary { FileType::Binary } else { FileType::Ascii };
    p7::read_from_file(&mut kernel, &mut brep, bytes, kind, rebuild, &mut env).map_err(fail)?;
    brep.use_arena_naming().map_err(fail)?;
    brep.refresh_cached_topology();
    Ok(Body { kernel, brep })
}

/// Every face, in the kernel's face order.
pub fn faces(body: &Body) -> Result<Vec<FaceInfo>, String> {
    let b = &body.brep;
    p1::faces(b, None)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|f| {
            let face = b.face(f).map_err(|e| e.to_string())?;
            let surface = face.surface.as_ref().map_or("none", |s| surface_name(s.kind()));
            Ok(FaceInfo { index: f.index() as usize, surface, tolerance: face.tolerance })
        })
        .collect()
}

/// Every edge, in the kernel's edge order, with its curve sampled within 2°.
pub fn edges(body: &Body) -> Result<Vec<EdgeInfo>, String> {
    let b = &body.brep;
    let t = tessellate_boundaries(&body.kernel, b, 2.0).map_err(|e| e.to_string())?;
    let mut points = t.points.iter();
    let mut out = Vec::with_capacity(t.edges.len());
    for (&e, &n) in t.edges.iter().zip(&t.vertex_counts) {
        let edge = b.edge(e).map_err(|err| err.to_string())?;
        let faces = edge_faces(b, e).map_err(|err| err.to_string())?;
        out.push(EdgeInfo {
            index: e.index() as usize,
            tolerance: edge.tolerance,
            faces: faces.iter().map(|f| f.index() as usize).collect(),
            points: points.by_ref().take(n).map(|p| [p.x, p.y, p.z]).collect(),
        });
    }
    Ok(out)
}

/// The body meshed by `solidcore-tess` at its display tolerance, through the record as an
/// app meshes it.
pub fn mesh(body: &mut Body, edges: &[EdgeInfo]) -> Result<Mesh, String> {
    let size = diagonal(edges.iter().flat_map(|e| e.points.iter().copied()));
    let (record, order) = solidcore_convert::brep::to_record(&mut body.kernel, &mut body.brep)
        .map_err(|e| format!("converting to the record: {e}"))?;
    let tolerance = solidcore_tess::MeshTolerance::display_of(size);
    let (plan, _) = solidcore_tess::plan(&record, &tolerance, size).map_err(|e| format!("meshing: {e}"))?;
    let (mesh, refused) = plan.evaluate();
    let kernel_face = |record_face: usize| order.faces.get(record_face).map_or(usize::MAX, |f| f.index() as usize);
    Ok(Mesh {
        positions: mesh.positions.iter().map(|p| [p.x, p.y, p.z]).collect(),
        triangles: mesh.triangles,
        triangle_face: mesh.tri_face.iter().map(|&f| kernel_face(f as usize)).collect(),
        refused: refused.iter().map(|r| (kernel_face(r.face), r.reason.clone())).collect(),
        size,
    })
}

/// The diagonal of the points' bounding box, 1 when there are none.
fn diagonal(points: impl Iterator<Item = [f64; 3]>) -> f64 {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in points {
        for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(v);
            *h = h.max(v);
        }
    }
    let d = lo.iter().zip(&hi).map(|(l, h)| (h - l) * (h - l)).sum::<f64>().sqrt();
    if d.is_finite() && d > 0.0 { d } else { 1.0 }
}

/// The name of a surface's kind, as the report and the summary print it.
fn surface_name(kind: &SurfaceKind) -> &'static str {
    match kind {
        SurfaceKind::Cone(_) => "cone",
        SurfaceKind::BSpline(_) => "B-spline",
        SurfaceKind::CurveBounded(_) => "curve-bounded",
        SurfaceKind::Plane(_) => "plane",
        SurfaceKind::SurfOfRevolution(_) => "surface of revolution",
        SurfaceKind::Step(_) => "STEP surface",
        SurfaceKind::OffsetSurface(_) => "offset",
        SurfaceKind::SrfInVolume(_) => "surface in volume",
        SurfaceKind::Sphere(_) => "sphere",
        SurfaceKind::Torus(_) => "torus",
        SurfaceKind::SurfOfExtrusion(_) => "surface of extrusion",
        SurfaceKind::ProceduralBlend(_) => "procedural blend",
        SurfaceKind::BallPatch(_) => "ball patch",
    }
}
