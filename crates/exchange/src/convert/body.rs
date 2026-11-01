//! A body: its welded mesh, and its faces, edges and corners as the kernel and the naming know
//! them. Nothing is reconstructed: the weld gives each edge's mesh vertices, the kernel each
//! edge's corners, the naming every name.

use solidcraft_doc::{Document, ModelBody};
use solidcraft_geom::Vec3;
use solidcraft_kernel::FaceSurface;

use super::Budget;
use crate::error::{Error, Result};
use crate::options::Tolerance;
use crate::schema::{
    Body, BodyName, Bounds, ControlPoint, Curve, Direction, Edge, EdgeIndex, EdgeName, Face, FaceIndex, FaceName, Knots, Mass, MaterialName,
    MeshVertex, Mm, Point, Radians, Surface, Vertex, VertexIndex, VertexName,
};
use crate::{MAX_CONTROL_POINTS, MAX_FACES, MAX_TRIANGLES, MIN_TOLERANCE_MM};

fn point(v: Vec3) -> Point {
    Point([v.x + 0.0, v.y + 0.0, v.z + 0.0])
}

fn direction(v: Vec3) -> Direction {
    Direction::new([v.x, v.y, v.z]).unwrap_or_default()
}

fn index32(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

pub(crate) fn body(doc: &Document, b: &ModelBody, tolerance: Tolerance, mass: bool, budget: &mut Budget) -> Result<Body> {
    let tol = match tolerance {
        Tolerance::Relative(r) => (b.body.size() * r).max(MIN_TOLERANCE_MM),
        Tolerance::Absolute(a) => a,
    };
    let (mesh, paths, edge_faces) =
        solidcraft_io::tagged_mesh_with_edges(&b.body, tol).map_err(|e| Error::Invalid(format!("body {}: {e}", b.name)))?;
    let face_count = mesh.faces as usize;
    if face_count > budget.faces || mesh.triangles.len() > budget.triangles {
        return Err(Error::Limit(format!(
            "body {} brings the snapshot over {MAX_FACES} faces or {MAX_TRIANGLES} triangles (a coarser tolerance or a body filter helps)",
            b.name
        )));
    }
    budget.faces -= face_count;
    budget.triangles -= mesh.triangles.len();
    if mesh.positions.iter().any(|p| !p.is_finite()) {
        return Err(Error::Invalid(format!("body {} has a vertex that is not a finite point", b.name)));
    }
    let positions: Vec<Point> = mesh.positions.iter().map(|p| point(*p)).collect();

    let names = solidcraft_doc::naming::face_names(b);
    let mut by_face: Vec<Vec<[MeshVertex; 3]>> = vec![Vec::new(); face_count];
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_faces) {
        if let Some(list) = by_face.get_mut(*f as usize) {
            list.push(t.map(MeshVertex));
        }
    }
    let mut faces = Vec::with_capacity(face_count);
    for (i, triangles) in by_face.into_iter().enumerate() {
        let name = names.get(i).cloned().unwrap_or_default();
        faces.push(Face {
            index: FaceIndex(index32(i)),
            features: super::features::feature_ids(&name),
            inferred: !solidcraft_doc::naming::face_name_exact(b, i),
            name: FaceName(name),
            surface: surface(&b.body, i, budget, &b.name)?,
            triangles,
            attributes: Default::default(),
        });
    }

    let corners = solidcraft_kernel::corners(&b.body);
    let edge_names = solidcraft_doc::naming::edge_names(b);
    let at = |v: MeshVertex| mesh.positions.get(v.index()).copied();
    let edges: Vec<Edge> = paths
        .into_iter()
        .enumerate()
        .map(|(i, path)| -> Result<Edge> {
            let path: Vec<MeshVertex> = path.into_iter().map(MeshVertex).collect();
            // The kernel's ends, in the direction the path runs.
            let ends = corners.ends.get(i).map(|&[a, z]| {
                let near = |c: usize, p: Option<&MeshVertex>| match (corners.points.get(c), p.and_then(|v| at(*v))) {
                    (Some(c), Some(p)) => c.dist(p),
                    _ => f64::INFINITY,
                };
                let flipped = near(z, path.first()) < near(a, path.first());
                let [a, z] = if flipped { [z, a] } else { [a, z] };
                [VertexIndex(index32(a)), VertexIndex(index32(z))]
            });
            let bounds = match edge_faces.get(i).map(Vec::as_slice) {
                Some(&[a, c]) if a != c => Bounds::Between(FaceIndex(a), FaceIndex(c)),
                Some(&[a, ..]) => Bounds::Seam(FaceIndex(a)),
                _ => return Err(Error::Invalid(format!("body {}: edge {i} bounds no face", b.name))),
            };
            Ok(Edge {
                index: EdgeIndex(index32(i)),
                name: EdgeName(edge_names.get(i).cloned().unwrap_or_default()),
                bounds,
                curve: curve(&b.body, i),
                ends,
                path,
            })
        })
        .collect::<Result<_>>()?;

    let vertex_names = solidcraft_doc::naming::vertex_names(b);
    let vertices = corners
        .points
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let me = VertexIndex(index32(k));
            let ending: Vec<&Edge> = edges.iter().filter(|e| e.ends.is_some_and(|ends| ends.contains(&me))).collect();
            // The mesh vertex at the corner: an end of an edge that ends there.
            let position = ending.iter().find_map(|e| {
                let [start, end] = e.ends?;
                if start == me {
                    e.path.first().copied()
                } else if end == me {
                    e.path.last().copied()
                } else {
                    None
                }
            });
            Vertex {
                index: me,
                name: VertexName(vertex_names.get(k).cloned().unwrap_or_default()),
                point: point(*p),
                position,
                edges: ending.iter().map(|e| e.index).collect(),
            }
        })
        .collect();

    let mass = if mass {
        let m = solidcraft_kernel::measure_mass(&b.body).map_err(|e| Error::Invalid(format!("body {}: mass properties: {e}", b.name)))?;
        Some(Mass { volume: m.volume, area: m.area, centroid: point(m.centroid), min: point(m.bbox.min), max: point(m.bbox.max) })
    } else {
        None
    };
    Ok(Body {
        size: Mm(b.body.size()),
        shells: index32(b.body.shell_count()),
        mass,
        name: BodyName(b.name.clone()),
        material: assigned_material(doc, b).map(MaterialName),
        closed: is_closed(&mesh.triangles),
        positions,
        faces,
        edges,
        vertices,
        attributes: Default::default(),
    })
}

/// The material assigned to a body (by its definition's name, so every placement of a
/// component's body has it), as the library or the design spells it.
fn assigned_material(doc: &Document, b: &ModelBody) -> Option<String> {
    let assigned = doc.materials.get(b.definition_name()).or_else(|| doc.materials.get(&b.name))?;
    doc.material_name(assigned)
}

/// Every side used once in each direction: a closed, consistently oriented mesh.
fn is_closed(triangles: &[[u32; 3]]) -> bool {
    if triangles.is_empty() {
        return false;
    }
    let mut sides: Vec<(u32, u32)> = triangles.iter().flat_map(|[a, b, c]| [(*a, *b), (*b, *c), (*c, *a)]).collect();
    sides.sort_unstable();
    if sides.windows(2).any(|w| w[0] == w[1]) {
        return false;
    }
    sides.iter().all(|(a, b)| sides.binary_search(&(*b, *a)).is_ok())
}

/// Face `i`'s surface: analytic, else its NURBS patch, else other.
fn surface(body: &solidcraft_kernel::Body, i: usize, budget: &mut Budget, name: &str) -> Result<Surface> {
    if let Some(s) = solidcraft_kernel::face_surface(body, i) {
        return Ok(match s {
            FaceSurface::Plane { normal, point: p } => Surface::Plane { point: point(p), normal: direction(normal.vec()) },
            FaceSurface::Cylinder { origin, axis, radius } => {
                Surface::Cylinder { origin: point(origin), axis: direction(axis.vec()), radius: Mm(radius) }
            }
            FaceSurface::Cone { apex, axis, half_angle } => {
                Surface::Cone { apex: point(apex), axis: direction(axis.vec()), half_angle: Radians(half_angle) }
            }
            FaceSurface::Sphere { center, radius } => Surface::Sphere { center: point(center), radius: Mm(radius) },
            FaceSurface::Torus { center, axis, major, minor } => {
                Surface::Torus { center: point(center), axis: direction(axis.vec()), major: Mm(major), minor: Mm(minor) }
            }
        });
    }
    let Some(n) = solidcraft_kernel::face_nurbs(body, i) else { return Ok(Surface::Other) };
    let count = n.points.iter().map(Vec::len).sum::<usize>();
    if count > budget.control_points {
        return Err(Error::Limit(format!("body {name} brings the snapshot over {MAX_CONTROL_POINTS} NURBS control points")));
    }
    budget.control_points -= count;
    let net = n.points.iter().zip(&n.weights).map(|(ps, ws)| ps.iter().zip(ws).map(|(p, w)| ControlPoint::new(point(*p), *w)).collect()).collect();
    Ok(Surface::Nurbs { u: Knots { degree: index32(n.degree_u), knots: n.knots_u }, v: Knots { degree: index32(n.degree_v), knots: n.knots_v }, net })
}

/// Edge `i`'s curve: a line, a circle, else other.
fn curve(body: &solidcraft_kernel::Body, i: usize) -> Curve {
    if let Some(l) = solidcraft_kernel::edge_line(body, i) {
        return Curve::Line { start: point(l.start), end: point(l.end) };
    }
    match solidcraft_kernel::edge_circle(body, i) {
        Some(c) => Curve::Circle { center: point(c.center), axis: direction(c.axis.vec()), radius: Mm(c.radius), angle: Radians(c.angle) },
        None => Curve::Other,
    }
}
