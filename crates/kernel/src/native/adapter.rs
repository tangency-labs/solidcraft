//! The truck adapter. Persisted data belongs to solidcraft_geom::brep, never truck.
//!
//! Truck keeps a face's wires relative to its surface (`absolute_boundaries`) and flips the
//! face with an orientation flag, which is schema 4's convention: loops are written as truck
//! holds them and the flag becomes the oriented face's `forward`. Faces, edges and vertices are
//! numbered in canonical order, which is truck's own (`face_iter`, and first appearance through
//! `edge_iter`, which walks a reversed face's wires backwards), so every index the kernel hands
//! out (history, face sources, paint) means the same face or edge in the record.
use super::{Result, invalid};
use crate::Body;
use mt::{Invertible, ParametricCurve};
use solidcraft_geom::brep as b;
use std::collections::HashMap;
use truck_modeling as mt;

#[path = "analytic.rs"]
mod analytic;
#[cfg(test)]
use analytic::revolved;
use analytic::{arc, load_analytic};
fn point(p: mt::Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn vector(p: mt::Vector3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn vector4(p: mt::Vector4) -> [f64; 4] {
    [p.x, p.y, p.z, p.w]
}
fn p([x, y, z]: [f64; 3]) -> mt::Point3 {
    mt::Point3::new(x, y, z)
}
fn v([x, y, z]: [f64; 3]) -> mt::Vector3 {
    mt::Vector3::new(x, y, z)
}
fn v4([x, y, z, w]: [f64; 4]) -> mt::Vector4 {
    mt::Vector4::new(x, y, z, w)
}
fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(invalid)
}
fn depth(d: usize) -> Result<()> {
    if d > 16 { Err(invalid("geometry nesting")) } else { Ok(()) }
}
fn unsupported(what: &str) -> crate::KernelError {
    crate::KernelError::Failed(format!("native B-rep: not supported yet: {what}"))
}

// Truck evaluates the full knot interval. The portable domain is the standard
// active interval, so only coincident (clamped) domains can cross this adapter.
fn clamped(knots: &[f64], count: usize) -> Result<()> {
    let degree = knots.len().checked_sub(count + 1).ok_or_else(|| invalid("spline degree"))?;
    if knots.first() != knots.get(degree) || knots.last() != knots.get(count) {
        return Err(invalid("unclamped spline domain unsupported by this adapter"));
    }
    Ok(())
}
fn compatible_curve(c: &b::Curve) -> Result<()> {
    match c {
        b::Curve::Spline { knots, points } => clamped(knots, points.len()),
        b::Curve::RationalSpline { knots, homogeneous } => clamped(knots, homogeneous.len()),
        // The producer's own trace: its surfaces must load; its leader is kept as it was.
        b::Curve::SurfaceIntersection { surfaces, .. } => surfaces.iter().try_for_each(|s| compatible_surface(s)),
        _ => Ok(()),
    }
}
fn compatible_surface(s: &b::Surface) -> Result<()> {
    match s {
        b::Surface::Plane { origin, u_axis, v_axis } => {
            let plane = mt::Plane::new(p(*origin), p(*origin) + v(*u_axis), p(*origin) + v(*v_axis));
            if vector(plane.u_axis()) != *u_axis || vector(plane.v_axis()) != *v_axis {
                return Err(invalid("plane axes cannot be reconstructed exactly"));
            }
            Ok(())
        }
        b::Surface::Spline { u_knots, v_knots, points } => {
            clamped(u_knots, points.len())?;
            clamped(v_knots, points.first().map_or(0, Vec::len))
        }
        b::Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
            clamped(u_knots, homogeneous.len())?;
            clamped(v_knots, homogeneous.first().map_or(0, Vec::len))
        }
        b::Surface::PlacedRevolution { profile, .. } => compatible_curve(profile),
        _ => Ok(()),
    }
}

/// The body's working tolerance, mm: how far later operations take its faces to be from its
/// edges (`fill::face_tolerance`, 10⁻⁶ of its size, at least 10⁻⁵ mm). Truck's own edges sit
/// that far off their surfaces (a traced intersection, an imported edge), so a pcurve lifts
/// onto the edge within it and SMLib can check the edge against the face with it.
fn working_tolerance(body: &Body) -> f64 {
    crate::fill::face_tolerance(body)
}

/// The record of `body`'s solid. Each face's pcurves come from the records written for it
/// (`brep::pcurve`, the routine every backend shares), within the body's working tolerance.
pub(super) fn encode(body: &Body) -> Result<b::Brep> {
    let solid = &body.solid;
    let tolerance = working_tolerance(body);
    let shells = solid.boundaries();
    let regions = regions(body)?;

    // Faces in truck's order; edges and vertices in first appearance, each edge's curve record
    // and interval written when it first appears.
    let mut face_index = HashMap::new();
    let mut edge_index = HashMap::new();
    let mut edges: Vec<(mt::Edge, b::Curve, [f64; 2])> = Vec::new();
    let mut uses: Vec<Vec<b::EdgeUseRef>> = Vec::new();
    let mut faces = Vec::new();
    let mut shell_records = Vec::new();
    for shell in shells {
        let mut oriented = Vec::new();
        for face in shell.face_iter() {
            let fi = faces.len();
            if face_index.insert(face.id(), fi).is_some() {
                return Err(unsupported("a face in two shells"));
            }
            oriented.push(b::OrientedFace { face: index(fi)?, forward: face.orientation() });
            let wires = face.absolute_boundaries();
            // The canonical walk: wires in order, each backwards on a reversed face.
            for (li, wire) in wires.iter().enumerate() {
                let n = wire.len();
                for k in 0..n {
                    let position = if face.orientation() { k } else { n - 1 - k };
                    let e = wire.get(position).ok_or_else(|| invalid("wire position"))?;
                    let i = match edge_index.get(&e.id()) {
                        Some(i) => *i,
                        None => {
                            let curve = e.curve();
                            let (t0, t1) = curve.try_range_tuple().ok_or_else(|| invalid("unbounded edge curve"))?;
                            edges.push((e.absolute_clone(), save_curve(&curve, 0)?, [t0, t1]));
                            uses.push(Vec::new());
                            edge_index.insert(e.id(), edges.len() - 1);
                            edges.len() - 1
                        }
                    };
                    let at = b::EdgeUseRef { face: index(fi)?, r#loop: index(li)?, position: index(position)?, upper: !e.orientation() };
                    uses.get_mut(i).ok_or_else(|| invalid("edge use"))?.push(at);
                }
            }
            faces.push(face_record(face, wires, &edge_index, &edges, tolerance)?);
        }
        shell_records.push(b::Shell::Faces { faces: oriented, wires: Vec::new() });
    }
    let mut vertex_index = HashMap::new();
    let mut vertices = Vec::new();
    let mut edge_records = Vec::with_capacity(edges.len());
    for ((e, curve, interval), uses) in edges.into_iter().zip(uses) {
        if uses.len() > 2 {
            return Err(unsupported("the radial order of an edge with more than two uses"));
        }
        let mut vertex = |x: &mt::Vertex| {
            *vertex_index.entry(x.id()).or_insert_with(|| {
                vertices.push(b::Vertex { point: point(x.point()), tolerance });
                vertices.len() - 1
            })
        };
        let ends = [vertex(e.absolute_front()), vertex(e.absolute_back())];
        edge_records.push(b::Edge { curve, interval, vertices: [index(ends[0])?, index(ends[1])?], tolerance, uses });
    }
    let result = b::Brep { version: b::VERSION, tolerance, regions, shells: shell_records, faces, edges: edge_records, vertices };
    result.validate().map_err(invalid)?;
    compatible(&result, Producer::Own)?;
    Ok(result)
}

/// A face's record: its surface, its loops as truck holds them (relative to the surface) and
/// their pcurves from the surface and edge records, and the domain around the pcurves.
fn face_record(
    face: &mt::Face,
    wires: &[mt::Wire],
    edge_index: &HashMap<mt::EdgeID, usize>,
    edges: &[(mt::Edge, b::Curve, [f64; 2])],
    tolerance: f64,
) -> Result<b::Face> {
    let surface = save_surface(&face.surface(), 0)?;
    let mut uses = Vec::with_capacity(wires.len());
    for wire in wires {
        let l = wire
            .edge_iter()
            .map(|e| {
                let i = *edge_index.get(&e.id()).ok_or_else(|| invalid("edge index"))?;
                let (_, curve, interval) = edges.get(i).ok_or_else(|| invalid("edge index"))?;
                Ok(b::pcurve::EdgeUse { edge: index(i)?, curve, interval: *interval, forward: e.orientation() })
            })
            .collect::<Result<Vec<_>>>()?;
        uses.push(l);
    }
    let curves = b::pcurve::pcurves(&surface, &uses, tolerance).map_err(pcurve_error)?;
    let loops = uses
        .iter()
        .zip(curves)
        .map(|(l, pcs)| b::Loop::Edges {
            edges: l.iter().zip(pcs).map(|(u, pcurve)| b::OrientedEdge { edge: u.edge, forward: u.forward, pcurve }).collect(),
        })
        .collect::<Vec<_>>();
    let domain = domain(&surface, &loops)?;
    Ok(b::Face { surface, domain, loops })
}

/// `face`'s record on its own, its edges numbered in first appearance: what [`encode`] writes
/// for it, pcurves included.
#[cfg(test)]
pub(super) fn face_alone(face: &mt::Face, tolerance: f64) -> Result<b::Face> {
    let wires = face.absolute_boundaries();
    let (mut edge_index, mut edges) = (HashMap::new(), Vec::new());
    for e in wires.iter().flat_map(|w| w.edge_iter()) {
        if let std::collections::hash_map::Entry::Vacant(slot) = edge_index.entry(e.id()) {
            let curve = e.curve();
            let (t0, t1) = curve.try_range_tuple().ok_or_else(|| invalid("unbounded edge curve"))?;
            edges.push((e.absolute_clone(), save_curve(&curve, 0)?, [t0, t1]));
            slot.insert(edges.len() - 1);
        }
    }
    face_record(face, wires, &edge_index, &edges, tolerance)
}

/// The kernel's error for the pcurve routine's.
fn pcurve_error(e: b::pcurve::Error) -> crate::KernelError {
    match e {
        b::pcurve::Error::Invalid(m) => crate::KernelError::Invalid(m),
        b::pcurve::Error::Failed(m) => crate::KernelError::Failed(m),
        b::pcurve::Error::TooLarge(m) => crate::KernelError::TooLarge(m),
    }
}

/// The rectangle around a face's pcurves (their control polygons, which hold them), clamped to
/// the surface's natural domain; at most one turn in a periodic direction, about its middle.
fn domain(surface: &b::Surface, loops: &[b::Loop]) -> Result<[[f64; 2]; 2]> {
    let mut d = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    let mut add = |uv: [f64; 2]| {
        for (r, x) in d.iter_mut().zip(uv) {
            r[0] = r[0].min(x);
            r[1] = r[1].max(x);
        }
    };
    for l in loops {
        let b::Loop::Edges { edges } = l else { continue };
        for e in edges {
            match &e.pcurve {
                b::Curve2::Spline { points, .. } => points.iter().for_each(|q| add(*q)),
                b::Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().for_each(|[x, y, w]| add([x / w, y / w])),
            }
        }
    }
    for (r, natural) in d.iter_mut().zip(surface.natural()) {
        match natural {
            b::Natural::Range(lo, hi) => *r = [r[0].max(lo), r[1].min(hi)],
            b::Natural::Periodic if r[1] - r[0] > std::f64::consts::TAU => {
                let middle = 0.5 * (r[0] + r[1]);
                *r = [middle - std::f64::consts::PI, middle + std::f64::consts::PI];
            }
            b::Natural::Periodic => {}
        }
        if !(r[0].is_finite() && r[1].is_finite() && r[0] < r[1]) {
            return Err(invalid("face domain"));
        }
    }
    Ok(d)
}

/// The infinite region and the bounded ones, from the shells as truck holds them: an open shell
/// lies in the infinite region; a closed one bounds material when it faces out (each truck
/// solid's shells are oriented) and a void when it faces in, and lies in the region around it.
/// Bounded regions come in the order of their outer shells.
fn regions(body: &Body) -> Result<Vec<b::Region>> {
    let shells = body.solid.boundaries();
    let closed: Vec<bool> = shells.iter().map(|s| s.shell_condition() == mt::ShellCondition::Closed).collect();
    let all: Vec<u32> = (0..shells.len()).map(index).collect::<Result<_>>()?;
    let mut regions = Vec::new();
    match closed.iter().filter(|c| **c).count() {
        0 => regions.push(b::Region { shells: all, void: false }),
        1 => {
            regions.push(b::Region { shells: all, void: false });
            let s = closed.iter().position(|c| *c).ok_or_else(|| invalid("closed shell"))?;
            regions.push(b::Region { shells: vec![index(s)?], void: false });
        }
        n if n < shells.len() => return Err(unsupported("open shells beside several closed shells")),
        _ => {
            let (material, parents) = body.shell_nesting()?;
            let inside =
                |p: Option<usize>| -> Result<Vec<u32>> { parents.iter().enumerate().filter(|(_, q)| **q == p).map(|(i, _)| index(i)).collect() };
            regions.push(b::Region { shells: inside(None)?, void: false });
            for (s, m) in material.iter().enumerate() {
                let mut shells = vec![index(s)?];
                shells.extend(inside(Some(s))?);
                regions.push(b::Region { shells, void: !m });
            }
            // The canonical shell order walks the regions; truck's must be the same.
            let mut seen = Vec::new();
            for s in regions.iter().flat_map(|r| &r.shells) {
                if !seen.contains(s) {
                    seen.push(*s);
                }
            }
            if seen != all {
                return Err(unsupported("shells in an order other than canonical (a void before another body)"));
            }
        }
    }
    Ok(regions)
}

fn compatible(body: &b::Brep, producer: Producer) -> Result<()> {
    body.edges.iter().try_for_each(|e| compatible_curve(&e.curve))?;
    body.faces.iter().try_for_each(|f| match (&f.surface, producer) {
        // Truck holds a plane by three points: another kernel's axes load rounded once.
        (b::Surface::Plane { .. }, Producer::Foreign) => Ok(()),
        (s, _) => compatible_surface(s),
    })
}

/// Who wrote a record, which sets how exactly it must load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Producer {
    /// This adapter: the record loads back number for number, or is refused.
    Own,
    /// Another kernel (SMLib): its records load as truck holds them, analytic surfaces as
    /// revolutions (as for our own) and planes through three points rounded once.
    #[cfg_attr(not(feature = "smlib"), allow(dead_code))]
    Foreign,
}

/// The solid of a record: validated, its geometry checked against its topology (truck builds
/// edges and faces from it unchecked), then built as the kernel built it.
pub(super) fn decode(body: b::Brep) -> Result<mt::Solid> {
    decode_from(body, Producer::Own)
}

pub(super) fn decode_from(body: b::Brep, producer: Producer) -> Result<mt::Solid> {
    body.validate().map_err(invalid)?;
    body.check_geometry().map_err(invalid)?;
    compatible(&body, producer)?;
    let b::Brep { shells, faces, edges, vertices, .. } = body;
    let vertices: Vec<mt::Vertex> = vertices.into_iter().map(|v| mt::Vertex::new(p(v.point))).collect();
    let vertex = |i: u32| vertices.get(i as usize).ok_or_else(|| invalid("edge vertices"));
    let edges = edges
        .into_iter()
        .map(|e| Ok(mt::Edge::new_unchecked(vertex(e.vertices[0])?, vertex(e.vertices[1])?, load_edge_curve(e.curve, e.interval)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut faces: Vec<Option<b::Face>> = faces.into_iter().map(Some).collect();
    let shells = shells
        .into_iter()
        .map(|s| match s {
            b::Shell::Faces { faces: uses, wires } if wires.is_empty() => uses
                .into_iter()
                .map(|u| {
                    // A face in two shells (both its sides used) is refused here: truck has one.
                    let f = faces.get_mut(u.face as usize).and_then(Option::take).ok_or_else(|| unsupported("a face in two shells"))?;
                    face(f, u.forward, &edges)
                })
                .collect::<Result<mt::Shell>>(),
            b::Shell::Faces { .. } => Err(unsupported("wire edges embedded in a shell")),
            b::Shell::Wire { .. } | b::Shell::Vertex { .. } => Err(unsupported("wire and vertex shells")),
        })
        .collect::<Result<Vec<_>>>()?;
    if shells.iter().any(|s| s.shell_condition() == mt::ShellCondition::Irregular) {
        return Err(invalid("non-manifold shell"));
    }
    Ok(mt::Solid::new_unchecked(shells))
}
/// A face's topology as recorded, references already checked: built as the kernel built it,
/// without truck's checks for wires that touch themselves (a seam edge used twice, a closed edge
/// on one vertex), which imported and modelled faces have. Pcurves are not needed (truck has none).
fn face(f: b::Face, forward: bool, edges: &[mt::Edge]) -> Result<mt::Face> {
    let wires = f
        .loops
        .into_iter()
        .map(|l| match l {
            b::Loop::Edges { edges: uses } => uses
                .into_iter()
                .map(|u| {
                    let e = edges.get(u.edge as usize).ok_or_else(|| invalid("edge reference"))?;
                    Ok(if u.forward { e.clone() } else { e.inverse() })
                })
                .collect::<Result<mt::Wire>>(),
            b::Loop::Vertex { .. } => Err(unsupported("vertex loops")),
        })
        .collect::<Result<Vec<_>>>()?;
    let mut face = mt::Face::new_unchecked(wires, load_surface(f.surface, f.domain)?);
    if !forward {
        face.invert();
    }
    Ok(face)
}

/// Exact: every record holds the producer's own numbers, so loading it gives back the same
/// curve, never a refit.
fn save_curve(c: &mt::Curve, d: usize) -> Result<b::Curve> {
    depth(d)?;
    Ok(match c {
        mt::Curve::Line(c) => b::Curve::Line { start: point(c.0), end: point(c.1) },
        mt::Curve::BSplineCurve(c) => {
            b::Curve::Spline { knots: c.knot_vec().iter().copied().collect(), points: c.control_points().iter().copied().map(point).collect() }
        }
        mt::Curve::NurbsCurve(c) => b::Curve::RationalSpline {
            knots: c.knot_vec().iter().copied().collect(),
            homogeneous: c.control_points().iter().copied().map(vector4).collect(),
        },
        mt::Curve::IntersectionCurve(c) => {
            let leader = match c.leader() {
                mt::Leader::Polyline(p) => b::Curve::Polyline { points: p.0.iter().copied().map(point).collect() },
                mt::Leader::BSpline(c) => b::Curve::Spline {
                    knots: c.knot_vec().iter().copied().collect(),
                    points: c.control_points().iter().copied().map(point).collect(),
                },
            };
            b::Curve::SurfaceIntersection {
                surfaces: [Box::new(save_surface(c.surface0(), d + 1)?), Box::new(save_surface(c.surface1(), d + 1)?)],
                leader: Box::new(leader),
                tolerance: c.tolerance(),
            }
        }
    })
}
/// A plane as truck holds it: three points.
#[derive(serde::Serialize, serde::Deserialize)]
struct PlanePoints {
    o: mt::Point3,
    p: mt::Point3,
    q: mt::Point3,
}

/// A revolution as truck holds it, built from its parts as they are (its constructors normalize
/// the axis and compose the placement with the identity, either of which can round).
#[derive(serde::Serialize)]
struct PlacedRevolution {
    entity: Revolved,
    transform: mt::Matrix4,
    orientation: bool,
}
#[derive(serde::Serialize)]
struct Revolved {
    curve: mt::Curve,
    revolution: Axis,
}
#[derive(serde::Serialize)]
struct Axis {
    origin: mt::Point3,
    axis: mt::Vector3,
}

fn save_surface(s: &mt::Surface, d: usize) -> Result<b::Surface> {
    depth(d)?;
    Ok(match s {
        mt::Surface::Plane(s) => {
            let PlanePoints { o, p, q } = serde_json::from_value(serde_json::to_value(s).map_err(invalid)?).map_err(invalid)?;
            b::Surface::PlaneThrough { origin: point(o), u_point: point(p), v_point: point(q) }
        }
        mt::Surface::BSplineSurface(s) => b::Surface::Spline {
            u_knots: s.uknot_vec().iter().copied().collect(),
            v_knots: s.vknot_vec().iter().copied().collect(),
            points: s.control_points().iter().map(|r| r.iter().copied().map(point).collect()).collect(),
        },
        mt::Surface::NurbsSurface(s) => b::Surface::RationalSpline {
            u_knots: s.uknot_vec().iter().copied().collect(),
            v_knots: s.vknot_vec().iter().copied().collect(),
            homogeneous: s.control_points().iter().map(|r| r.iter().copied().map(vector4).collect()).collect(),
        },
        mt::Surface::RevolutedCurve(s) => {
            let m = *s.transform();
            b::Surface::PlacedRevolution {
                profile: Box::new(save_curve(s.entity().entity_curve(), d + 1)?),
                origin: point(s.entity().origin()),
                axis: vector(s.entity().axis()),
                matrix: [m.x, m.y, m.z, m.w].map(vector4),
                forward: s.orientation(),
            }
        }
    })
}

/// An edge's curve for truck, which bounds an edge by its curve's range: the record itself
/// where the interval is that range (all this adapter writes), else the piece on the interval
/// for the kinds that have one in closed form.
fn load_edge_curve(c: b::Curve, interval: [f64; 2]) -> Result<mt::Curve> {
    match c {
        b::Curve::Line { start, end } if interval != [0., 1.] => {
            let at = |t: f64| p(start) + (p(end) - p(start)) * t;
            Ok(mt::Curve::Line(mt::Line(at(interval[0]), at(interval[1]))))
        }
        b::Curve::Circle { frame, radius } => arc(&frame, [radius, radius], interval),
        b::Curve::Ellipse { frame, radii } => arc(&frame, radii, interval),
        b::Curve::Line { .. } => load_curve(c),
        c => match c.natural() {
            b::Natural::Range(a, z) if [a, z] == interval => load_curve(c),
            _ => Err(unsupported("a spline or intersection edge on part of its curve")),
        },
    }
}
fn load_curve(c: b::Curve) -> Result<mt::Curve> {
    Ok(match c {
        b::Curve::Line { start, end } => mt::Curve::Line(mt::Line(p(start), p(end))),
        b::Curve::Circle { frame, radius } => arc(&frame, [radius, radius], [0., std::f64::consts::TAU])?,
        b::Curve::Ellipse { frame, radii } => arc(&frame, radii, [0., std::f64::consts::TAU])?,
        b::Curve::Spline { knots, points } => {
            mt::Curve::BSplineCurve(mt::BSplineCurve::try_new(knots.into(), points.into_iter().map(p).collect()).map_err(invalid)?)
        }
        b::Curve::RationalSpline { knots, homogeneous } => mt::Curve::NurbsCurve(mt::NurbsCurve::new(
            mt::BSplineCurve::try_new(knots.into(), homogeneous.into_iter().map(v4).collect()).map_err(invalid)?,
        )),
        b::Curve::Polyline { .. } => return Err(invalid("standalone polyline unsupported by this adapter")),
        b::Curve::SurfaceIntersection { surfaces: [s0, s1], leader, tolerance } => {
            let leader = match *leader {
                b::Curve::Polyline { points } => mt::Leader::Polyline(mt::PolylineCurve(points.into_iter().map(p).collect())),
                b::Curve::Spline { knots, points } => {
                    mt::Leader::BSpline(mt::BSplineCurve::try_new(knots.into(), points.into_iter().map(p).collect()).map_err(invalid)?)
                }
                _ => return Err(invalid("intersection leader")),
            };
            let (s0, s1) = (load_surface(*s0, UNIT)?, load_surface(*s1, UNIT)?);
            mt::Curve::IntersectionCurve(mt::IntersectionCurve::new_unchecked(Box::new(s0), Box::new(s1), leader, tolerance))
        }
    })
}
/// The domain an intersection's surfaces load with: they have no face of their own.
const UNIT: [[f64; 2]; 2] = [[0., 1.]; 2];
/// A surface for truck; `domain` (the face's) bounds the profile of an analytic surface,
/// which truck holds as a revolution.
fn load_surface(s: b::Surface, domain: [[f64; 2]; 2]) -> Result<mt::Surface> {
    Ok(match s {
        b::Surface::Plane { origin, u_axis, v_axis } => mt::Surface::Plane(mt::Plane::new(p(origin), p(origin) + v(u_axis), p(origin) + v(v_axis))),
        b::Surface::Spline { u_knots, v_knots, points } => mt::Surface::BSplineSurface(
            mt::BSplineSurface::try_new((u_knots.into(), v_knots.into()), points.into_iter().map(|r| r.into_iter().map(p).collect()).collect())
                .map_err(invalid)?,
        ),
        b::Surface::RationalSpline { u_knots, v_knots, homogeneous } => mt::Surface::NurbsSurface(mt::NurbsSurface::new(
            mt::BSplineSurface::try_new((u_knots.into(), v_knots.into()), homogeneous.into_iter().map(|r| r.into_iter().map(v4).collect()).collect())
                .map_err(invalid)?,
        )),
        b::Surface::PlaneThrough { origin, u_point, v_point } => mt::Surface::Plane(mt::Plane::new(p(origin), p(u_point), p(v_point))),
        b::Surface::PlacedRevolution { profile, origin, axis, matrix: [x, y, z, w], forward } => {
            let placed = PlacedRevolution {
                entity: Revolved { curve: load_curve(*profile)?, revolution: Axis { origin: p(origin), axis: v(axis) } },
                transform: mt::Matrix4::from_cols(v4(x), v4(y), v4(z), v4(w)),
                orientation: forward,
            };
            mt::Surface::RevolutedCurve(serde_json::from_value(serde_json::to_value(placed).map_err(invalid)?).map_err(invalid)?)
        }
        analytic => load_analytic(analytic, domain[1])?,
    })
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
