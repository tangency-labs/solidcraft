//! Booleans of a body with a closed triangle mesh: the mesh becomes a polyhedral solid (each
//! run of coplanar triangles joined across their shared edges one planar face, sewn), which
//! SMLib's boolean takes with the body as it is. The result's history is over the body and the
//! mesh, the mesh's faces being its triangles.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_geom::{Mesh, Vec3};

use crate::body::Body;
use crate::heal::find;
use crate::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History, Side};
use crate::sew::{EdgeSpec, FaceSpec, SurfSpec};
use crate::{BoolOp, KernelError, Result};

/// The most triangles a mesh operand may have.
const MAX_TRIANGLES: usize = 200_000;

fn failed(what: &str) -> KernelError {
    KernelError::Failed(format!("faceted boolean: {what}"))
}

/// Boolean of a body with a closed triangle mesh (wound outward). `tol` (mm): how far a
/// triangle's corners may lie off the plane of the triangles beside it and still make one
/// face with them. For tools that come as meshes; the body keeps its exact faces.
///
/// The result's history is over `[a, b]`, the mesh's faces being its triangles (and it having
/// no edges): a face of the result lies on faces of `a` or on triangles of `b`; an edge where
/// `a` meets the mesh meets the triangles of the face it lies along.
pub fn faceted_boolean(a: &Body, b: &Mesh, op: BoolOp, tol: f64) -> Result<Option<Body>> {
    crate::profile::timed(crate::profile::Kind::Boolean, "faceted_boolean", || faceted_boolean_untimed(a, b, op, tol))
}

fn faceted_boolean_untimed(a: &Body, b: &Mesh, op: BoolOp, tol: f64) -> Result<Option<Body>> {
    a.require_brep("a boolean")?;
    if !(tol.is_finite() && tol > 0.0) {
        return Err(KernelError::Invalid("facet tolerance".into()));
    }
    if b.triangles.len() > MAX_TRIANGLES {
        return Err(failed(&format!("the mesh is too detailed ({} triangles, at most {MAX_TRIANGLES})", b.triangles.len())));
    }
    let welded = welded(b);
    let b = &welded;
    let groups = coplanar_groups(b, tol)?;
    let mut faces = Vec::new();
    let mut face_group = Vec::new();
    for (g, triangles) in groups.iter().enumerate() {
        for spec in group_faces(b, triangles)? {
            faces.push(spec);
            face_group.push(g);
        }
    }
    let (tool, from) = crate::sew::sew_touching_with_sources(&faces, &vec![None; faces.len()])?;
    let Some(result) = crate::boolean(a, &tool, op)? else { return Ok(None) };
    // The result's history over [a, tool], with the tool's own over the mesh put in its place.
    let inner = from.and_then(|from| tool_history(&tool, &from, &face_group, &groups));
    let history = match (result.history(), inner) {
        (Some(h), Some(inner)) => h.clone().substituted(1, &inner).ok(),
        _ => None,
    };
    let counts = [(a.face_count(), a.edge_count()), (b.triangles.len(), 0)];
    Ok(Some(result.with_checked_history(history, &counts)))
}

/// `m` with corners at the very same point made one vertex, triangles in their order: a body's
/// own mesh (Body::mesh) gives each face its own vertices, the faces along an edge its very
/// points, and is closed only once they are one.
fn welded(m: &Mesh) -> Mesh {
    let mut at: BTreeMap<[u64; 3], u32> = BTreeMap::new();
    let mut positions = Vec::with_capacity(m.positions.len());
    let index: Vec<u32> = m
        .positions
        .iter()
        .map(|p| {
            *at.entry([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).or_insert_with(|| {
                positions.push(*p);
                (positions.len() - 1) as u32
            })
        })
        .collect();
    let triangles = m.triangles.iter().map(|t| t.map(|i| index.get(i as usize).copied().unwrap_or(i))).collect();
    Mesh { positions, triangles, ..Mesh::default() }
}

/// The mesh's triangles in runs that make one planar face each: triangles sharing an edge, on
/// one plane within `tol`, are one run.
fn coplanar_groups(m: &Mesh, tol: f64) -> Result<Vec<Vec<usize>>> {
    let mut planes = Vec::with_capacity(m.triangles.len());
    for t in &m.triangles {
        let [p, q, r] = m.tri(t).ok_or_else(|| failed("a triangle refers to a missing vertex"))?;
        let n = (q - p).cross(r - p).normalized().ok_or_else(|| failed("a triangle has no area"))?;
        planes.push((n, n.dot(p)));
    }
    let mut by_edge: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (i, t) in m.triangles.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let on_plane = |(n, w): (Vec3, f64), t: usize| {
        m.triangles.get(t).and_then(|t| m.tri(t)).is_some_and(|corners| corners.iter().all(|p| (n.dot(*p) - w).abs() <= tol))
    };
    let mut parent: Vec<usize> = (0..m.triangles.len()).collect();
    for sharing in by_edge.values() {
        let &[t, u] = sharing.as_slice() else { return Err(failed("the mesh is not closed: an edge is not shared by exactly two triangles")) };
        let (Some(&pt), Some(&pu)) = (planes.get(t), planes.get(u)) else { continue };
        if pt.0.dot(pu.0) > 1.0 - 1e-12 && on_plane(pt, u) && on_plane(pu, t) {
            let (rt, ru) = (find(&mut parent, t), find(&mut parent, u));
            if let Some(slot) = parent.get_mut(rt) {
                *slot = ru;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for t in 0..m.triangles.len() {
        groups.entry(find(&mut parent, t)).or_default().push(t);
    }
    Ok(groups.into_values().collect())
}

/// The planar faces of one run of coplanar triangles: its boundary (the edges no other triangle
/// of the run runs back along) chained into loops, split where a loop passes a vertex twice,
/// each outer loop (counter-clockwise about the run's normal) with the holes it holds.
fn group_faces(m: &Mesh, triangles: &[usize]) -> Result<Vec<FaceSpec>> {
    let bad = || failed("a triangle refers to a missing vertex");
    let first = *triangles.first().ok_or_else(|| failed("an empty face"))?;
    let [p, q, r] = m.triangles.get(first).and_then(|t| m.tri(t)).ok_or_else(bad)?;
    let normal = (q - p).cross(r - p).normalized().ok_or_else(|| failed("a triangle has no area"))?;
    let directed: BTreeSet<(u32, u32)> =
        triangles.iter().filter_map(|t| m.triangles.get(*t)).flat_map(|t| (0..3).map(move |k| (t[k], t[(k + 1) % 3]))).collect();
    let mut out: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(a, b) in &directed {
        if !directed.contains(&(b, a)) {
            out.entry(a).or_default().push(b);
        }
    }
    let point = |v: u32| m.positions.get(v as usize).copied().ok_or_else(bad);
    let mut loops: Vec<Vec<u32>> = Vec::new();
    while let Some((&start, _)) = out.iter().find(|(_, next)| !next.is_empty()) {
        let mut walk = vec![start];
        let mut at = start;
        loop {
            let next = out.get_mut(&at).and_then(Vec::pop).ok_or_else(|| failed("a face's boundary does not close"))?;
            if next == start {
                break;
            }
            walk.push(next);
            at = next;
        }
        // A walk through a vertex twice (two loops touching there) is split into simple loops.
        let mut stack: Vec<u32> = Vec::new();
        for v in walk {
            if let Some(at) = stack.iter().position(|x| *x == v) {
                loops.push(stack.split_off(at));
            }
            stack.push(v);
        }
        loops.push(stack);
    }
    let polygon = |lp: &[u32]| lp.iter().map(|v| point(*v)).collect::<Result<Vec<Vec3>>>();
    let area = |pts: &[Vec3]| pts.iter().zip(pts.iter().cycle().skip(1)).fold(Vec3::ZERO, |s, (a, b)| s + a.cross(*b)).dot(normal) * 0.5;
    let (u, w) = (normal.any_perp(), normal.cross(normal.any_perp()));
    let inside = |pt: Vec3, poly: &[Vec3]| {
        let (px, py) = (pt.dot(u), pt.dot(w));
        let mut odd = false;
        for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
            let ((ax, ay), (bx, by)) = ((a.dot(u), a.dot(w)), (b.dot(u), b.dot(w)));
            if (ay > py) != (by > py) && px < ax + (py - ay) / (by - ay) * (bx - ax) {
                odd = !odd;
            }
        }
        odd
    };
    let polys: Vec<Vec<Vec3>> = loops.iter().filter(|l| l.len() >= 3).map(|l| polygon(l)).collect::<Result<_>>()?;
    let (outers, holes): (Vec<&Vec<Vec3>>, Vec<&Vec<Vec3>>) = polys.iter().partition(|p| area(p) > 0.0);
    let edges = |poly: &[Vec3]| poly.iter().zip(poly.iter().cycle().skip(1)).map(|(a, b)| EdgeSpec::Line { a: *a, b: *b }).collect::<Vec<_>>();
    let mut faces: Vec<FaceSpec> = outers.iter().map(|o| FaceSpec { loops: vec![edges(o)], surface: SurfSpec::Plane, probe: None }).collect();
    for hole in holes {
        let probe = *hole.first().ok_or_else(|| failed("an empty loop"))?;
        // The innermost outer loop holding the hole.
        let owner = (0..outers.len())
            .filter(|i| outers.get(*i).is_some_and(|o| inside(probe, o)))
            .min_by(|x, y| outers.get(*x).map_or(0.0, |o| area(o)).total_cmp(&outers.get(*y).map_or(0.0, |o| area(o))))
            .ok_or_else(|| failed("a hole in no face"))?;
        if let Some(face) = faces.get_mut(owner) {
            face.loops.push(edges(hole));
        }
    }
    Ok(faces)
}

/// The tool's history over the mesh: each face from the triangles of its run (`from` gives
/// the face spec each tool face was sewn from, `face_group` the run of each spec); each edge
/// where the runs of the faces either side of it meet. `None` where a face can't be followed.
fn tool_history(tool: &Body, from: &[usize], face_group: &[usize], groups: &[Vec<usize>]) -> Option<History> {
    let triangles = |face: usize| -> Option<&Vec<usize>> { groups.get(*face_group.get(*from.get(face)?)?) };
    let origins =
        |face: usize| -> Option<Vec<FaceOrigin>> { Some(triangles(face)?.iter().map(|t| FaceOrigin::Face { operand: 0, face: *t }).collect()) };
    let faces = (0..tool.face_count())
        .map(|f| {
            let origins = origins(f)?;
            let change = if origins.len() == 1 { Change::Kept } else { Change::Merged };
            Some(FaceProv { origins, change })
        })
        .collect::<Option<Vec<_>>>()?;
    let record = crate::native::brep_record(tool).ok()?;
    let edges = record
        .edges
        .iter()
        .map(|e| {
            let mut sides: Vec<usize> = e.uses.iter().map(|u| u.face as usize).collect();
            sides.sort_unstable();
            sides.dedup();
            let meet = match sides.as_slice() {
                [f, g] => vec![EdgeOrigin::meet(Side::new(origins(*f)?)?, Side::new(origins(*g)?)?)],
                _ => Vec::new(),
            };
            Some(EdgeProv { faces: sides, origins: meet })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(History { operands: 1, faces, edges })
}
