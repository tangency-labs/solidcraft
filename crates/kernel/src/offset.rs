//! What analytic surface a face of the schema 4 record lies on, with its outward side, and the
//! faces that continue a chosen face on the same surface (a hole's wall stored in halves).
//! Moving faces along their normals (offset faces, shell, draft) is SMLib's
//! (`crate::offset_backend`).

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{Brep, Frame, Loop, Surface};

use crate::body::Body;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Surf {
    /// Outward normal and offset: n·x = d.
    Plane { n: Vec3, d: f64 },
    /// Axis point and direction, radius; `convex` when the outward normal points away from
    /// the axis.
    Cylinder { o: Vec3, a: Vec3, r: f64, convex: bool },
    /// Centre and radius; `convex` when the outward normal points away from the centre.
    Sphere { c: Vec3, r: f64, convex: bool },
    /// Apex, unit axis (into the cone, away from the apex) and half-angle; `convex` when the
    /// outward normal points away from the axis.
    Cone { v: Vec3, a: Vec3, half: f64, convex: bool },
    /// Axis point and unit axis, the tube's centre circle (radius `big` at height `h` along the
    /// axis) and the tube radius; `convex` when the outward normal points away from the tube's
    /// centre.
    Torus { o: Vec3, a: Vec3, big: f64, h: f64, r: f64, convex: bool },
}

fn axes(f: &Frame) -> Option<(Vec3, Vec3)> {
    let (x, y) = (Vec3::from(f.x_axis), Vec3::from(f.y_axis));
    Some((Vec3::from(f.origin), x.cross(y).normalized()?))
}

/// What surface face `face` of the record `b` lies on, as its record says (planes, cylinders,
/// cones, spheres and tori; `None` for a free-form face). The record's analytic surfaces have
/// their natural normals away from their axis or centre, so a face is convex when its shell
/// uses its natural side.
pub(crate) fn surf_of(b: &Brep, face: usize) -> Option<Surf> {
    let f = b.faces.get(face)?;
    // `forward` is about the face's NURBS form, which may run against its surface.
    let out = crate::assemble::natural_out(b, face)? != f.form_turns()?;
    let sign = if out { 1.0 } else { -1.0 };
    Some(match &f.surface {
        Surface::Plane { origin, u_axis, v_axis } => {
            let n = Vec3::from(*u_axis).cross(Vec3::from(*v_axis)).normalized()? * sign;
            Surf::Plane { n, d: n.dot(Vec3::from(*origin)) }
        }
        Surface::PlaneThrough { origin, u_point, v_point } => {
            let o = Vec3::from(*origin);
            let n = (Vec3::from(*u_point) - o).cross(Vec3::from(*v_point) - o).normalized()? * sign;
            Surf::Plane { n, d: n.dot(o) }
        }
        Surface::Cylinder { frame, radius } => {
            let (o, a) = axes(frame)?;
            Surf::Cylinder { o, a, r: *radius, convex: out }
        }
        Surface::Sphere { frame, radius } => Surf::Sphere { c: Vec3::from(frame.origin), r: *radius, convex: out },
        Surface::Cone { frame, radius_at_origin, half_angle } => {
            let (o, z) = axes(frame)?;
            let apex = o + z * (-radius_at_origin / half_angle.tan());
            let a = if *half_angle > 0.0 { z } else { -z };
            Surf::Cone { v: apex, a, half: half_angle.abs(), convex: out }
        }
        Surface::Torus { frame, major_radius, minor_radius, .. } => {
            let (o, a) = axes(frame)?;
            Surf::Torus { o, a, big: *major_radius, h: 0.0, r: *minor_radius, convex: out }
        }
        Surface::Spline { .. } | Surface::RationalSpline { .. } | Surface::PlacedRevolution { .. } => return None,
    })
}

impl Surf {
    /// Whether `other` is this surface, with the same outward side, within `tol`.
    pub(crate) fn same(&self, other: &Surf, tol: f64) -> bool {
        match (*self, *other) {
            (Surf::Plane { n, d }, Surf::Plane { n: m, d: e }) => n.dot(m) > 1.0 - 1e-9 && (d - e).abs() < tol,
            (Surf::Cylinder { o, a, r, convex }, Surf::Cylinder { o: o2, a: a2, r: r2, convex: c2 }) => {
                a.dot(a2).abs() > 1.0 - 1e-9 && (r - r2).abs() < tol && convex == c2 && (o2 - o - a * (o2 - o).dot(a)).len() < tol
            }
            (Surf::Sphere { c, r, convex }, Surf::Sphere { c: c2, r: r2, convex: k2 }) => c.dist(c2) < tol && (r - r2).abs() < tol && convex == k2,
            (Surf::Cone { v, a, half, convex }, Surf::Cone { v: v2, a: a2, half: h2, convex: k2 }) => {
                v.dist(v2) < tol && a.dot(a2) > 1.0 - 1e-9 && (half - h2).abs() < 1e-9 && convex == k2
            }
            (Surf::Torus { o, a, big, h, r, convex }, Surf::Torus { o: o2, a: a2, big: b2, h: h2, r: r2, convex: k2 }) => {
                (o + a * h).dist(o2 + a2 * h2) < tol && a.dot(a2).abs() > 1.0 - 1e-9 && (big - b2).abs() < tol && (r - r2).abs() < tol && convex == k2
            }
            _ => false,
        }
    }
}

/// The record edges each face of `b` uses.
pub(crate) fn face_edges(b: &Brep, face: usize) -> Vec<u32> {
    let mut out: Vec<u32> = b
        .faces
        .get(face)
        .into_iter()
        .flat_map(|f| &f.loops)
        .flat_map(|l| match l {
            Loop::Edges { edges } => edges.iter().map(|u| u.edge).collect(),
            Loop::Vertex { .. } => Vec::new(),
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The chosen faces and every face on the same surface joined to them (the halves of a hole's
/// wall), as face indices. Pieces of a face split on purpose (Split Face) stay apart.
pub fn with_same_surface(b: &Body, chosen: &[usize]) -> Vec<usize> {
    let Ok(record) = crate::native::brep_record(b) else { return chosen.to_vec() };
    let tol = (b.size() * 1e-7).max(1e-9) * 100.0;
    let surfs: Vec<Option<Surf>> = (0..record.faces.len()).map(|i| surf_of(&record, i)).collect();
    let edges: Vec<Vec<u32>> = (0..record.faces.len()).map(|i| face_edges(&record, i)).collect();
    let keep = b.split_keep();
    let mut out: Vec<usize> = chosen.to_vec();
    let mut grew = true;
    while grew {
        grew = false;
        for i in 0..record.faces.len() {
            if out.contains(&i) {
                continue;
            }
            let Some(Some(si)) = surfs.get(i) else { continue };
            let mine = edges.get(i).map(Vec::as_slice).unwrap_or(&[]);
            let joins = out.iter().any(|j| {
                surfs.get(*j).and_then(|x| x.as_ref()).is_some_and(|sj| si.same(sj, tol))
                    && edges.get(*j).is_some_and(|theirs| {
                        theirs.iter().any(|e| {
                            mine.binary_search(e).is_ok()
                                && (keep.is_empty() || !crate::heal::on_split_line(&crate::heal::edge_points(&record, *e), keep))
                        })
                    })
            });
            if joins {
                out.push(i);
                grew = true;
            }
        }
    }
    out
}
