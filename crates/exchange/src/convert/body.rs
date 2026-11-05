//! A body: its welded mesh, and its faces, edges and corners as the kernel's record and the
//! naming know them. Nothing is reconstructed: the weld gives each edge's mesh vertices, the
//! record (`solidcraft_kernel::brep_record`, in the body's own face, edge and vertex order) each
//! edge's corners and the NURBS nets, the naming every name.

use solidcore_record::Vec3;
use solidcore_record::brep::{Brep, Curve as RecordCurve, Surface as RecordSurface};
use solidcraft_doc::{Document, ModelBody};
use solidcraft_geom::prelude::*;
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

pub(crate) fn body(doc: &Document, b: &ModelBody, tolerance: Tolerance, mass: bool, with_record: bool, budget: &mut Budget) -> Result<Body> {
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

    // A mesh body has no record: no surfaces, curves or corners then.
    let record = solidcraft_kernel::brep_record(&b.body).ok();
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
            surface: surface(&b.body, record.as_ref(), i, budget, &b.name)?,
            triangles,
            attributes: Default::default(),
        });
    }

    let corner = |v: u32| record.as_ref()?.vertices.get(v as usize).map(|v| Vec3::from(v.point));
    let edge_names = solidcraft_doc::naming::edge_names(b);
    let at = |v: MeshVertex| mesh.positions.get(v.index()).copied();
    let edges: Vec<Edge> = paths
        .into_iter()
        .enumerate()
        .map(|(i, path)| -> Result<Edge> {
            let path: Vec<MeshVertex> = path.into_iter().map(MeshVertex).collect();
            // The kernel's ends, in the direction the path runs.
            let ends = record.as_ref().and_then(|r| r.edges.get(i)).map(|e| e.vertices).map(|[a, z]| {
                let near = |c: u32, p: Option<&MeshVertex>| match (corner(c), p.and_then(|v| at(*v))) {
                    (Some(c), Some(p)) => c.dist(p),
                    _ => f64::INFINITY,
                };
                let flipped = near(z, path.first()) < near(a, path.first());
                let [a, z] = if flipped { [z, a] } else { [a, z] };
                [VertexIndex(a), VertexIndex(z)]
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
                curve: curve(&b.body, record.as_ref(), i),
                ends,
                path,
            })
        })
        .collect::<Result<_>>()?;

    let vertex_names = solidcraft_doc::naming::vertex_names(b);
    let vertices = record
        .as_ref()
        .map(|r| r.vertices.as_slice())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(k, v)| {
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
                point: point(Vec3::from(v.point)),
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
        record: record.filter(|_| with_record),
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
fn surface(body: &solidcraft_kernel::Body, record: Option<&Brep>, i: usize, budget: &mut Budget, name: &str) -> Result<Surface> {
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
    // The face's own B-spline, else the NURBS form it is parameterised on.
    let Some(face) = record.and_then(|r| r.faces.get(i)) else { return Ok(Surface::Other) };
    let spline = match (&face.surface, &face.nurbs) {
        (s @ (RecordSurface::Spline { .. } | RecordSurface::RationalSpline { .. }), _) | (_, Some(s)) => s,
        _ => return Ok(Surface::Other),
    };
    let (u_knots, v_knots, net): (&[f64], &[f64], Vec<Vec<(Vec3, f64)>>) = match spline {
        RecordSurface::Spline { u_knots, v_knots, points } => {
            (u_knots, v_knots, points.iter().map(|row| row.iter().map(|p| (Vec3::from(*p), 1.0)).collect()).collect())
        }
        RecordSurface::RationalSpline { u_knots, v_knots, homogeneous } => (
            u_knots,
            v_knots,
            homogeneous.iter().map(|row| row.iter().map(|&[x, y, z, w]| (Vec3::new(x / w, y / w, z / w), w)).collect()).collect(),
        ),
        _ => return Ok(Surface::Other),
    };
    let rows = net.len();
    let columns = net.first().map_or(0, Vec::len);
    let count = rows * columns;
    if count == 0 || net.iter().any(|row| row.len() != columns) || u_knots.len() <= rows || v_knots.len() <= columns {
        return Ok(Surface::Other);
    }
    if count > budget.control_points {
        return Err(Error::Limit(format!("body {name} brings the snapshot over {MAX_CONTROL_POINTS} NURBS control points")));
    }
    budget.control_points -= count;
    // A knot vector has degree + 1 more knots than its control points.
    let degree = |knots: &[f64], points: usize| index32(knots.len() - points - 1);
    let net = net.into_iter().map(|row| row.into_iter().map(|(p, w)| ControlPoint::new(point(p), w)).collect()).collect();
    Ok(Surface::Nurbs {
        u: Knots { degree: degree(u_knots, rows), knots: u_knots.to_vec() },
        v: Knots { degree: degree(v_knots, columns), knots: v_knots.to_vec() },
        net,
    })
}

/// Edge `i`'s curve: a line (between its corners), a circle, else other.
fn curve(body: &solidcraft_kernel::Body, record: Option<&Brep>, i: usize) -> Curve {
    if let Some(record) = record
        && let Some(e) = record.edges.get(i)
        && matches!(e.curve, RecordCurve::Line { .. })
    {
        let [a, z] = e.vertices.map(|v| record.vertices.get(v as usize).map(|v| Vec3::from(v.point)));
        if let (Some(start), Some(end)) = (a, z) {
            return Curve::Line { start: point(start), end: point(end) };
        }
    }
    match solidcraft_kernel::edge_circle(body, i) {
        Some(c) => Curve::Circle { center: point(c.center), axis: direction(c.axis.vec()), radius: Mm(c.radius), angle: Radians(c.angle) },
        None => Curve::Other,
    }
}
