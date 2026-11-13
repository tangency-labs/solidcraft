//! Resolving sketch links against the model: projecting edges, faces, body silhouettes, other
//! sketches and the origin onto a sketch plane, and intersecting bodies with it.
//!
//! Model geometry is found again on every recompute: a face or an edge by the persistent name
//! its link keeps, anything else (and a link saved without a name) from the reference point
//! stored in the link. An edge of a solid lying in a plane parallel to the sketch's comes in as
//! its exact curve (a line, an arc, the Bézier pieces of a free-form curve such as a letter
//! cut from a font), so a profile drawn round it meets the solid's faces exactly. Anything
//! else is fitted to its polyline from the tessellation, which lies exactly on the curve:
//! lines and circular arcs are recovered exactly, the rest (an ellipse from a tilted circle,
//! a spline edge running out of the plane) becomes a chain of lines.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::Mesh;
use solidcraft_sketch::linalg::{Mat, solve};
use solidcraft_sketch::{CurveKind, LinkGeom, LinkKind, LinkSource, Sketch, WireCurve};

use crate::eval::ModelState;
use crate::expr::Value;
use crate::{DocError, Document, PlaneRef, Result};

/// Half length of the line an axis or plane projects to.
const AXIS_HALF: f64 = 100.0;
/// Points in one fitted chain at most (longer chains are thinned).
const MAX_CHAIN: usize = 4000;

#[path = "project_order.rs"]
mod order;

/// A resolved link: its geometry in sketch coordinates and the (possibly moved) reference.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub geom: Vec<LinkGeom>,
    /// 3D curves (world coordinates).
    pub wires: Vec<Vec<Vec3>>,
    /// Each wire's exact curve where its source has one (an included edge or sketch curve),
    /// one list per wire, empty for a curve known only as points ([`solidcraft_sketch::Wire::exact`]).
    pub exact: Vec<Vec<WireCurve>>,
    pub source: LinkSource,
    /// The reference was not exactly where it was and was re-found on the nearest geometry.
    pub moved: bool,
    /// Exact face selected during this resolution, before its closest point is
    /// projected onto an edge shared with another face. Computed metadata only.
    pub face_index: Option<usize>,
    /// The edge an `Edge` reference resolved to (its index in the body's mesh), whose
    /// persistent name the link keeps ([`resolved_edge_name`]). Computed metadata only.
    pub edge_index: Option<usize>,
    /// The vertex a `Vertex` reference resolved to (its index in the body's B-rep record),
    /// whose persistent name the link keeps ([`resolved_vertex_name`]). Computed metadata only.
    pub vertex_index: Option<usize>,
}

/// Resolutions of body-based references, keyed by the reference, the sketch plane and the
/// identity of every body's display mesh: re-resolving an unchanged model (every sketch edit
/// does) is free.
static CACHE: std::sync::Mutex<Vec<(String, Resolved)>> = std::sync::Mutex::new(Vec::new());
const CACHE_SIZE: usize = 64;

fn is_sketch_curve(src: &LinkSource) -> bool {
    matches!(src, LinkSource::SketchCurve { .. })
}

fn cache_key(st: &ModelState, plane: &Plane, kind: LinkKind, src: &LinkSource) -> Option<String> {
    // An intersection with a sketch curve (#11048) changes with that sketch, not with the meshes.
    let body_based = matches!(
        src,
        LinkSource::Edge { .. }
            | LinkSource::Face { .. }
            | LinkSource::Body { .. }
            | LinkSource::Vertex { .. }
            | LinkSource::Iso { .. }
            | LinkSource::Spun { .. }
    ) || matches!(src, LinkSource::Intersection { a, b } if !is_sketch_curve(a) && !is_sketch_curve(b));
    if !body_based {
        return None;
    }
    let bodies: Vec<String> = st
        .bodies
        .iter()
        .map(|b| {
            let m = b.mesh();
            format!("{}@{:p}/{}/{:?}", b.name, std::sync::Arc::as_ptr(&m), m.triangles.len(), m.bounds())
        })
        .collect();
    Some(format!("{kind:?}|{src:?}|{plane:?}|{}", bodies.join(";")))
}

/// Resolve a link source against the model state before the sketch.
pub fn resolve(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, plane: &Plane, kind: LinkKind, src: &LinkSource) -> Result<Resolved> {
    resolve_at(doc, vals, st, plane, kind, src, None)
}

/// [`resolve`] for a sketch evaluated in `frame`: its author component and that component's
/// world placement, including the author path of a face-attached sketch. A construction point
/// is kept in its own component's frame and is brought into this one (#12075); without a frame
/// it is taken in the root's.
pub fn resolve_at(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    plane: &Plane,
    kind: LinkKind,
    src: &LinkSource,
    frame: Option<&(u64, crate::Mat)>,
) -> Result<Resolved> {
    let key = cache_key(st, plane, kind, src);
    if let Some(k) = &key
        && let Ok(c) = CACHE.lock()
        && let Some((_, r)) = c.iter().find(|(x, _)| x == k)
    {
        return Ok(r.clone());
    }
    let r = resolve_uncached(doc, vals, st, plane, kind, src, frame)?;
    if let Some(k) = key
        && let Ok(mut c) = CACHE.lock()
    {
        c.insert(0, (k, r.clone()));
        c.truncate(CACHE_SIZE);
    }
    Ok(r)
}

fn resolve_uncached(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    plane: &Plane,
    kind: LinkKind,
    src: &LinkSource,
    frame: Option<&(u64, crate::Mat)>,
) -> Result<Resolved> {
    let lost = |what: &str| DocError::Invalid(format!("{what} no longer exists"));
    let mut moved = false;
    let mut face_index = None;
    let mut edge_index = None;
    let mut vertex_index = None;
    let mut source = src.clone();
    let mut wires: Vec<Vec<Vec3>> = Vec::new();
    let mut exact: Vec<Vec<WireCurve>> = Vec::new();
    let geom = match src {
        LinkSource::Edge { body, at } => {
            let (bi, mesh) = body_mesh(st, body, *at)?;
            let (e, q, d) = nearest_edge(&mesh, *at).ok_or_else(|| lost("the edge"))?;
            let size = mesh.bounds().diagonal().max(1e-9);
            if d > size * 0.25 {
                return Err(lost("the edge"));
            }
            moved = d > (size * 2e-3).max(1e-3);
            let name = st.bodies.get(bi).map(|b| b.name.clone()).unwrap_or_else(|| body.clone());
            source = LinkSource::Edge { body: name, at: q };
            edge_index = Some(e);
            let planar = st.bodies.get(bi).and_then(|b| exact_edges(b, plane, &[e]).pop().flatten());
            let pts = mesh.edges.get(e).cloned().unwrap_or_default();
            edge_geom(plane, kind, planar, (pts, || edge_curves(st.bodies.get(bi), e)), &mut wires, &mut exact)
        }
        LinkSource::Face { body, at } => {
            let (bi, mesh) = body_mesh(st, body, *at)?;
            let (f, q, d) = nearest_face(&mesh, *at).ok_or_else(|| lost("the face"))?;
            face_index = Some(usize::try_from(f).map_err(|_| DocError::Invalid("face index exceeds platform limit".into()))?);
            let size = mesh.bounds().diagonal().max(1e-9);
            if d > size * 0.25 {
                return Err(lost("the face"));
            }
            moved = d > (size * 2e-3).max(1e-3);
            let name = st.bodies.get(bi).map(|b| b.name.clone()).unwrap_or_else(|| body.clone());
            source = LinkSource::Face { body: name, at: q };
            match kind {
                LinkKind::Intersect => section(plane, &mesh, Some(f)),
                _ => {
                    let target = st.bodies.get(bi).ok_or_else(|| lost("the projection body"))?;
                    // A mesh body is one face with no edges: the flat patch of triangles picked,
                    // traced (#13464).
                    if target.body.is_mesh() {
                        crate::project3d::flat_outline(&mesh, q).iter().flat_map(|outline| fit_mesh_outline(plane, outline)).collect()
                    } else {
                        let g = projected_edges(target, &mesh, plane, &projection_face_edges(target, &mesh, f, plane)?);
                        if certified_planar_construction(target, f as usize) { merge_family_lines(g)? } else { g }
                    }
                }
            }
        }
        LinkSource::Body { body } => {
            let b = st.body(body).ok_or_else(|| lost(&format!("body {body}")))?;
            let mesh = b.mesh();
            match kind {
                LinkKind::Intersect => section(plane, &mesh, None),
                _ => silhouette(plane, &mesh, Some(&b.body)),
            }
        }
        LinkSource::Vertex { body, at } => {
            let (bi, mesh) = body_mesh(st, body, *at)?;
            let size = mesh.bounds().diagonal().max(1e-9);
            let v = mesh
                .edges
                .iter()
                .flat_map(|e| [e.first().copied(), e.last().copied()])
                .flatten()
                .min_by(|a, b| a.dist(*at).total_cmp(&b.dist(*at)))
                .ok_or_else(|| lost("the vertex"))?;
            let d = v.dist(*at);
            if d > size * 0.25 {
                return Err(lost("the vertex"));
            }
            moved = d > (size * 2e-3).max(1e-3);
            let body_ref = st.bodies.get(bi);
            vertex_index = body_ref.and_then(|b| crate::naming::vertex_index_at(b, v));
            let name = body_ref.map(|b| b.name.clone()).unwrap_or_else(|| body.clone());
            source = LinkSource::Vertex { body: name, at: v };
            vec![LinkGeom::Point(plane.to_local(v))]
        }
        LinkSource::SketchCurve { sketch, curve } => {
            let ss = st.sketch(*sketch).ok_or_else(|| lost(&format!("sketch {sketch}")))?;
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| lost(&format!("sketch curve {curve}")))?;
            match kind {
                LinkKind::Intersect => {
                    let pts: Vec<Vec3> = ss.sketch.segs(ci).iter().flat_map(|s| s.polyline(1e-3)).map(|p| ss.plane.to_world(p)).collect();
                    section_of_polyline(plane, &pts)
                }
                LinkKind::Include => {
                    let pts: Vec<Vec3> = ss.sketch.polyline(ci).iter().map(|p| ss.plane.to_world(*p)).collect();
                    if off_plane(plane, &pts) {
                        wires.push(pts);
                        exact.push(sketch_curves(ss, ci));
                        Vec::new()
                    } else {
                        project_sketch_curve(&ss.sketch, &ss.plane, ci, plane)
                    }
                }
                _ => project_sketch_curve(&ss.sketch, &ss.plane, ci, plane),
            }
        }
        LinkSource::OnSurface { sketch, curve, body, at } => {
            let ss = st.sketch(*sketch).ok_or_else(|| lost(&format!("sketch {sketch}")))?;
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| lost(&format!("sketch curve {curve}")))?;
            let pts: Vec<Vec3> = ss.sketch.polyline(ci).iter().map(|p| ss.plane.to_world(*p)).collect();
            let (bi, mesh) = body_mesh(st, body, *at)?;
            let (f, q, d) = nearest_face(&mesh, *at).ok_or_else(|| lost("the face"))?;
            let size = mesh.bounds().diagonal().max(1e-9);
            if d > size * 0.25 {
                return Err(lost("the face"));
            }
            let name = st.bodies.get(bi).map(|b| b.name.clone()).unwrap_or_else(|| body.clone());
            source = LinkSource::OnSurface { sketch: *sketch, curve: curve.clone(), body: name, at: q };
            match st.bodies.get(bi).filter(|b| !b.body.is_mesh()) {
                // A solid's face: SMLib's projection of the sketch curve itself.
                Some(target) => {
                    let segs = ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci));
                    (wires, exact) = projected_wires(target, q, ss, &segs, plane.normal(), size)?;
                }
                // A mesh body has only its triangles: the curve projected onto them.
                None => {
                    wires = crate::project3d::onto_face(&mesh, f, &pts, plane.normal());
                    exact = vec![Vec::new(); wires.len()];
                }
            }
            Vec::new()
        }
        LinkSource::Intersection { a, b } => {
            // Where the bodies, faces or other sketches' curves meet, as the edges SMLib's imprint
            // of one by the other makes: each its exact record curve, its points at the display
            // tolerance.
            for (pts, curves) in crate::project3d::meeting(st, a, b)? {
                wires.push(pts);
                exact.push(curves);
            }
            Vec::new()
        }
        LinkSource::Iso { body, at, dir } => {
            let (bi, mesh) = body_mesh(st, body, *at)?;
            let (f, q, d) = nearest_face(&mesh, *at).ok_or_else(|| lost("the face"))?;
            let size = mesh.bounds().diagonal().max(1e-9);
            if d > size * 0.25 {
                return Err(lost("the face"));
            }
            let name = st.bodies.get(bi).map(|b| b.name.clone()).unwrap_or_else(|| body.clone());
            source = LinkSource::Iso { body: name, at: q, dir: dir.clone() };
            match st.bodies.get(bi).filter(|b| !b.body.is_mesh()) {
                Some(target) => {
                    let face = usize::try_from(f).map_err(|_| DocError::Invalid("face index exceeds platform limit".into()))?;
                    let curves = match dir.trim().to_ascii_lowercase().as_str() {
                        // Along the face's own parameter: the surface's isoparametric curve.
                        "u" => crate::kernel::iso_curves(&target.body, face, q, true)?,
                        "v" => crate::kernel::iso_curves(&target.body, face, q, false)?,
                        // Along a direction: the face cut by the plane through the point that
                        // holds the face's normal there and the direction.
                        other => {
                            let v: Vec<f64> = other.split([',', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
                            let [x, y, z] = v[..] else { return Err(DocError::Invalid(format!("iso direction `{dir}`"))) };
                            crate::kernel::face_section_along(&target.body, face, q, Vec3::new(x, y, z))?
                        }
                    };
                    (wires, exact) = joined_wires(curves, size * 1e-6)?;
                }
                // A mesh body has only its triangles: their section.
                None => {
                    wires = crate::project3d::iso_curve(&mesh, f, q, dir);
                    exact = vec![Vec::new(); wires.len()];
                }
            }
            Vec::new()
        }
        LinkSource::Spun { body, origin, dir } => {
            let b = st.body(body).ok_or_else(|| lost(&format!("body {body}")))?;
            let pts = crate::project3d::spun_outline(&b.mesh(), *origin, *dir, plane);
            let mut g = Vec::new();
            fit_chain(&pts, Some(b.mesh().bounds().diagonal().max(1e-9) * 2e-3), &mut g);
            g
        }
        LinkSource::SketchPoint { sketch, point } => {
            let ss = st.sketch(*sketch).ok_or_else(|| lost(&format!("sketch {sketch}")))?;
            let pi = ss.sketch.resolve_point(point).ok_or_else(|| lost(&format!("sketch point {point}")))?;
            let p = ss.sketch.point(pi).unwrap_or_default();
            vec![LinkGeom::Point(plane.to_local(ss.plane.to_world(p)))]
        }
        LinkSource::Origin => vec![LinkGeom::Point(plane.to_local(Vec3::ZERO))],
        // A construction point, by name (#12075), where the timeline so far puts it, in the
        // sketch's frame.
        LinkSource::ConstructionPoint { name } => {
            let (author, frame) = frame.copied().unwrap_or((0, crate::IDENTITY));
            let p = crate::eval::construction_point_in(doc, st, name, author, &frame).map_err(|_| lost(&format!("point {name}")))?;
            vec![LinkGeom::Point(plane.to_local(p))]
        }
        LinkSource::Axis { name } => {
            let (o, d) = match name.to_ascii_uppercase().as_str() {
                "X" => (Vec3::ZERO, Vec3::X),
                "Y" => (Vec3::ZERO, Vec3::Y),
                "Z" => (Vec3::ZERO, Vec3::Z),
                _ => match crate::construct::DatumRef::parse_key(name) {
                    // The origin axis of a component (#15086), where that component now stands.
                    Some(datum) => match crate::construct::resolve_datum_in(doc, &datum, 0)? {
                        crate::construct::ConstructGeom::Axis { origin, dir } => (origin, dir),
                        _ => return Err(lost(&format!("axis {name}"))),
                    },
                    // A construction axis, by name (#9324), where the timeline so far puts it.
                    None => crate::eval::construction_axis_in(Some(doc), st, name, 0)?,
                },
            };
            let o = plane.to_local(o);
            let d2 = Vec2::new(d.dot(plane.x), d.dot(plane.y));
            match d2.normalized() {
                Some(u) if d2.len() > 1e-9 => vec![LinkGeom::Line(o - u * AXIS_HALF, o + u * AXIS_HALF)],
                _ => vec![LinkGeom::Point(o)],
            }
        }
        LinkSource::SketchAxis { name } => {
            vec![solidcraft_sketch::sketch_axis_geom(name).ok_or_else(|| DocError::Unknown(format!("sketch axis `{name}`")))?]
        }
        LinkSource::Text { text, at, height, angle, font, style, .. } if style.path.is_none() => {
            solidcraft_sketch::text_geometry_styled(font.as_deref(), text, *at, *height, *angle, style, None)?
        }
        // Its path is a curve of its own sketch: set with the sketch (`Sketch::text_source_geometry`).
        LinkSource::Text { .. } => return Err(DocError::Invalid("a text on a path is set with its sketch".into())),
        LinkSource::Plane { name } => {
            let pr = if Plane::named(name).is_some() {
                PlaneRef::Origin { name: name.clone() }
            } else {
                // Resolved to the feature's id here, once (#1134); a name that finds no feature
                // dangles below, as it did when the reference carried the name.
                let feature = doc.find_feature(name).map(|f| crate::FeatureId(f.id)).unwrap_or(crate::FeatureId::DANGLING);
                PlaneRef::Construction { feature }
            };
            let other = doc.resolve_plane_in(vals, &pr, 0, 0, Some(st)).map_err(|_| lost(&format!("plane {name}")))?;
            plane_trace(plane, &other).into_iter().collect()
        }
    };
    if geom.is_empty() && wires.is_empty() {
        return Err(DocError::Invalid(format!("the {} projects to nothing on this sketch plane", src.describe())));
    }
    Ok(Resolved { geom: merge_split_circles(geom), wires, exact, source, moved, face_index, edge_index, vertex_index })
}

/// Whether a curve (world points) leaves the sketch plane: included, it then stays 3D.
fn off_plane(plane: &Plane, pts: &[Vec3]) -> bool {
    let size = pts.iter().fold(1.0_f64, |m, p| m.max(p.len()));
    pts.iter().any(|p| plane.height(*p).abs() > size * 1e-7)
}

/// What a body edge (its `exact` curve when read, and its polyline `pts`) gives a sketch: its
/// section with the sketch plane, itself as a 3D curve (its points into `wires`, its exact
/// curve into `exact`) when it is included and leaves the plane, else its projection (the exact
/// planar curve `planar`, or the polyline fitted).
fn edge_geom(
    plane: &Plane,
    kind: LinkKind,
    planar: Option<Vec<LinkGeom>>,
    (pts, curves): (Vec<Vec3>, impl FnOnce() -> Vec<WireCurve>),
    wires: &mut Vec<Vec<Vec3>>,
    exact: &mut Vec<Vec<WireCurve>>,
) -> Vec<LinkGeom> {
    match kind {
        LinkKind::Intersect => section_of_polyline(plane, &pts),
        LinkKind::Include if off_plane(plane, &pts) => {
            wires.push(pts);
            exact.push(curves());
            Vec::new()
        }
        _ => planar.unwrap_or_else(|| fit_world(plane, &pts)),
    }
}

/// Edge `e` of `body` (its index in the body's edge order) as its exact curve from the body's
/// record; none for a mesh body or an intersection curve the record holds no curve for.
fn edge_curves(body: Option<&crate::ModelBody>, e: usize) -> Vec<WireCurve> {
    let Some(body) = body.filter(|b| !b.body.is_mesh()) else { return Vec::new() };
    let Ok(record) = crate::kernel::brep_record(&body.body) else { return Vec::new() };
    record
        .edges
        .get(e)
        .filter(|edge| !matches!(edge.curve, solidcore_record::brep::Curve::SurfaceIntersection { .. }))
        .map(|edge| vec![WireCurve { curve: edge.parameters().clone(), interval: edge.interval }])
        .unwrap_or_default()
}

/// The sketch segments `segs` of `ss` projected along the sketch normal `normal` (turned toward
/// the face) onto the face of `target` at `at`: SMLib's projection, the side of the face the
/// projection meets first ([`first_hits`]), the pieces that meet end to end joined into one wire
/// each, every wire its points (for display) and its exact curves.
fn projected_wires(
    target: &crate::ModelBody,
    at: Vec3,
    ss: &crate::eval::SolvedSketch,
    segs: &[Seg2],
    normal: Vec3,
    size: f64,
) -> Result<(Vec<Vec<Vec3>>, Vec<Vec<WireCurve>>)> {
    let from = segs.first().map(|s| ss.plane.to_world(s.start())).unwrap_or(at);
    let toward = if (at - from).dot(normal) < 0.0 { normal * -1.0 } else { normal };
    let curves = crate::kernel::project_onto_face(&target.body, at, &ss.plane, segs, toward)?;
    joined_wires(first_hits(target, curves, toward)?, size * 1e-6)
}

/// Of the curves a parallel projection along `toward` made on a face of `target`, the ones the
/// projection meets first: SMLib gives every curve where the projecting lines cross the face
/// (both sides of a sphere), and a projection onto a surface is where they first hit it. A curve
/// is kept when the first hit along the line through its middle, on the body's mesh, is the
/// curve itself (within the mesh's chord tolerance, a few times over), not a point of the body
/// in front of it.
fn first_hits(
    target: &crate::ModelBody,
    curves: Vec<(solidcore_record::brep::Curve, [f64; 2])>,
    toward: Vec3,
) -> Result<Vec<(solidcore_record::brep::Curve, [f64; 2])>> {
    let Some(dir) = toward.normalized() else { return Ok(curves) };
    let mesh = target.mesh();
    let reach = 2.0 * mesh.bounds().diagonal().max(1e-9);
    let slack = 4.0 * target.display_tol();
    let mut kept = Vec::with_capacity(curves.len());
    for (curve, interval) in curves {
        let eval = solidcraft_geom::brep::eval::CurveEval::new(&curve).map_err(|e| DocError::Invalid(format!("projected curve: {e}")))?;
        let mid = eval.point(0.5 * (interval[0] + interval[1])).ok_or_else(|| DocError::Invalid("projected curve: no middle point".into()))?;
        let origin = mid - dir * reach;
        let front = mesh.raycast(origin, dir).is_some_and(|(t, _)| t < reach - slack);
        if !front {
            kept.push((curve, interval));
        }
    }
    Ok(kept)
}

/// Record curves as wires: curves that meet end to end (within `tol`) one wire, in any order and
/// direction they come (a projection split at a sphere's pole into two pieces that both end there
/// joins again there, the second run the other way), each wire its points (for display) and its
/// exact curves.
fn joined_wires(curves: Vec<(solidcore_record::brep::Curve, [f64; 2])>, tol: f64) -> Result<(Vec<Vec<Vec3>>, Vec<Vec<WireCurve>>)> {
    let (mut wires, mut exact): (Vec<Vec<Vec3>>, Vec<Vec<WireCurve>>) = (Vec::new(), Vec::new());
    for (curve, interval) in curves {
        let pts = crate::kernel::PathPiece::Segs(crate::kernel::curve_path(&curve, interval)?).points();
        if pts.is_empty() {
            continue;
        }
        wires.push(pts);
        exact.push(vec![WireCurve { curve, interval }]);
    }
    // Join a wire onto the one that ends where it starts (or ends, turned round), until no two
    // meet: each join removes a wire.
    'join: loop {
        for i in 0..wires.len() {
            for j in 0..wires.len() {
                let (Some(a), Some(b)) = (wires.get(i), wires.get(j)) else { continue };
                let (Some(end), Some(start), Some(last)) = (a.last(), b.first(), b.last()) else { continue };
                let turned = if i == j {
                    continue;
                } else if end.dist(*start) <= tol {
                    false
                } else if end.dist(*last) <= tol && exact.get(j).is_some_and(|e| e.iter().all(|c| reversed(c).is_some())) {
                    true
                } else {
                    continue;
                };
                let (mut w, mut e) = (wires.remove(j), exact.remove(j));
                if turned {
                    w.reverse();
                    e = e.iter().rev().filter_map(reversed).collect();
                }
                let i = if j < i { i - 1 } else { i };
                if let (Some(into), Some(curves)) = (wires.get_mut(i), exact.get_mut(i)) {
                    into.extend(w.into_iter().skip(1));
                    curves.extend(e);
                }
                continue 'join;
            }
        }
        break;
    }
    Ok((wires, exact))
}

/// `c` run the other way: the same points, its parameter `t` now `c0 + c1 - t` over the curve's
/// own knots (a line's `1 - t`, a circle's or ellipse's angle negated with its y axis turned).
/// Exact; `None` for a curve kind it doesn't turn.
fn reversed(c: &WireCurve) -> Option<WireCurve> {
    use solidcore_record::brep::{Curve, Frame};
    let [t0, t1] = c.interval;
    let flip = |frame: &Frame| Frame { origin: frame.origin, x_axis: frame.x_axis, y_axis: frame.y_axis.map(|v| -v) };
    let (curve, interval) = match &c.curve {
        Curve::Line { start, end } => (Curve::Line { start: *end, end: *start }, [1.0 - t1, 1.0 - t0]),
        Curve::Circle { frame, radius } => (Curve::Circle { frame: flip(frame), radius: *radius }, [-t1, -t0]),
        Curve::Ellipse { frame, radii } => (Curve::Ellipse { frame: flip(frame), radii: *radii }, [-t1, -t0]),
        Curve::Spline { knots, points } => {
            let sum = knots.first()? + knots.last()?;
            (
                Curve::Spline { knots: knots.iter().rev().map(|k| sum - k).collect(), points: points.iter().rev().copied().collect() },
                [sum - t1, sum - t0],
            )
        }
        Curve::RationalSpline { knots, homogeneous } => {
            let sum = knots.first()? + knots.last()?;
            (
                Curve::RationalSpline {
                    knots: knots.iter().rev().map(|k| sum - k).collect(),
                    homogeneous: homogeneous.iter().rev().copied().collect(),
                },
                [sum - t1, sum - t0],
            )
        }
        _ => return None,
    };
    Some(WireCurve { curve, interval })
}

/// Curve `ci` of the solved sketch `ss` as its exact pieces in space, in the order its polyline
/// runs; none where a piece has no record curve.
fn sketch_curves(ss: &crate::eval::SolvedSketch, ci: usize) -> Vec<WireCurve> {
    let segs = ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci));
    segs.iter()
        .map(|s| crate::kernel::sketch_record_curve(&ss.plane, s).map(|(curve, interval)| WireCurve { curve, interval }))
        .collect::<Option<_>>()
        .unwrap_or_default()
}

/// Edges `edges` of `body` (indices into its mesh's `edges`) on `plane` as their exact curves:
/// lines, circles and arcs, and a free-form curve as the one spline it is, its knots and weights
/// its own. `None` for an edge that doesn't lie in a plane parallel to `plane`, isn't read
/// exactly (a mesh body's) or is a spline no sketch holds (past degree 9 or 500 points): it is
/// fitted to its polyline instead. An edge read as free-form pieces, or not read, that runs
/// along a circle in a plane parallel to `plane` comes in as that circle ([`circular`]): a
/// round's edge is a spline that only follows its circle.
fn exact_edges(body: &crate::ModelBody, plane: &Plane, edges: &[usize]) -> Vec<Option<Vec<LinkGeom>>> {
    use crate::kernel::PlanarPiece;
    let free_form = |p: &PlanarPiece| matches!(p, PlanarPiece::Spline(_) | PlanarPiece::Seg(Seg2::Cubic { .. } | Seg2::Bezier { .. }));
    let link = |p: &PlanarPiece| match p {
        PlanarPiece::Seg(s) => link_geom(s),
        PlanarPiece::Spline(s) => link_spline(s),
    };
    let mut mesh = None;
    body.body
        .planar_edges(plane, edges)
        .into_iter()
        .zip(edges)
        .map(|(pieces, e)| match pieces {
            Some(pieces) if !pieces.iter().any(free_form) => pieces.iter().map(link).collect(),
            Some(pieces) => {
                let samples: Option<Vec<Vec2>> = pieces.iter().map(piece_samples).collect::<Option<Vec<_>>>().map(|s| s.concat());
                match samples.as_deref().and_then(circular) {
                    Some((g, _)) => Some(vec![g]),
                    None => match samples.as_deref().and_then(elliptical) {
                        Some(gs) => Some(gs),
                        None => pieces.iter().map(link).collect(),
                    },
                }
            }
            None if body.body.is_mesh() => None,
            None => {
                // Not read: its polyline lies on the curve, but only many points of it show a circle.
                let pts = mesh.get_or_insert_with(|| body.mesh()).edges.get(*e).filter(|pts| pts.len() > 8)?;
                let level = plane.height(*pts.first()?);
                let (g, radius) = circular(&pts.iter().map(|p| plane.to_local(*p)).collect::<Vec<_>>())?;
                pts.iter().all(|p| (plane.height(*p) - level).abs() <= radius * CIRCLE_TOL).then(|| vec![g])
            }
        })
        .collect()
}

/// A free-form edge's B-spline as the control spline that is the same curve, with its own knots
/// and weights. `None` for one a sketch can't hold.
fn link_spline(s: &crate::kernel::PlanarSpline) -> Option<LinkGeom> {
    let degree = u8::try_from(s.degree).ok()?;
    let nurbs = solidcraft_sketch::Nurbs { knots: s.knots.clone(), weights: s.weights.clone() };
    nurbs.check(s.points.len(), degree).ok()?;
    Some(LinkGeom::Spline { pts: s.points.clone(), control: true, degree, periodic: false, nurbs: Some(nurbs) })
}

/// Points sampled along each free-form piece of an edge to see whether it runs along a circle.
const CIRCLE_SAMPLES: u16 = 16;
/// How far round (radians) a curve turns at least to be taken for a circle: a letter's nearly
/// straight stroke lies near a large circle too.
const CIRCLE_MIN_TURN: f64 = 0.1;
/// How far (a fraction of its radius) a curve may stray from a circle and still be that circle:
/// a round's edge keeps to it within 3e-7, a curve that merely looks round (a letter's bowl of
/// four cubic spans) only within 3e-4.
const CIRCLE_TOL: f64 = 1e-5;
/// How far (a fraction of its major semi-axis) a curve may stray from an ellipse and still be
/// that ellipse. A drafted face's edge is a circle off-axis by the draft angle, so it keeps to
/// the ellipse within 6e-7 of the major semi-axis, well inside this tolerance.
const ELLIPSE_TOL: f64 = 1e-5;
/// How far round (radians) an ellipse must turn before it is believable as one: a letter's
/// nearly straight stroke lies near a large ellipse too.
const ELLIPSE_MIN_TURN: f64 = 0.5;
/// The widest an ellipse may be (its major over its minor axis).
const ELLIPSE_MAX_RATIO: f64 = 1e3;

/// The circle (closing points) or arc through `samples`, taken in order along a curve, with
/// its radius: when they all lie on one within [`CIRCLE_TOL`]. The arc ends at the first and
/// last sample, so it meets what the curve met.
fn circular(samples: &[Vec2]) -> Option<(LinkGeom, f64)> {
    let (first, last) = (*samples.first()?, *samples.last()?);
    let (c, radius) = ls_circle(samples)?;
    let tol = radius * CIRCLE_TOL;
    if !(radius.is_finite() && tol > 0.0) || samples.iter().any(|p| (p.dist(c) - radius).abs() > tol) {
        return None;
    }
    let mut sweep = 0.0;
    for w in samples.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let mut d = (*b - c).angle() - (*a - c).angle();
        if d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        } else if d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        sweep += d;
    }
    let turn = sweep.abs();
    // A NaN turn is not refused here, as before.
    if !turn.is_nan() && !(CIRCLE_MIN_TURN..=std::f64::consts::TAU + 1e-6).contains(&turn) {
        return None;
    }
    if first.dist(last) <= tol {
        return (turn > std::f64::consts::TAU - 1e-6).then_some((LinkGeom::Circle(c, radius), radius));
    }
    // Counter-clockwise from `a` to `b`.
    let (a, b) = if sweep > 0.0 { (first, last) } else { (last, first) };
    Some((LinkGeom::Arc { c, a, b }, radius))
}

/// A least-squares ellipse through `samples`: its centre, its semi-major and semi-minor radii
/// and the direction of its major axis. The samples are centred on their mean and scaled by
/// their RMS radius, so the fit does not depend on the size of the model, and the conic is
/// fitted with the trace normalised (`a + c = 1`), which leaves the ellipse's shape to the
/// three remaining coefficients. `None` when the samples are not an ellipse: too few of them,
/// a singular fit, a quadratic form that is not positive definite, a centre or axes that are
/// not real, or an axis ratio no ellipse has.
fn ls_ellipse(samples: &[Vec2]) -> Option<(Vec2, f64, f64, Vec2)> {
    if samples.len() < 6 {
        return None;
    }
    let n = samples.len() as f64;
    let mean = samples.iter().fold(Vec2::ZERO, |a, p| a + *p) / n;
    let rms = (samples.iter().map(|p| (*p - mean).len2()).sum::<f64>() / n).sqrt();
    if !(rms > 0.0) || !rms.is_finite() {
        return None;
    }
    // `a x^2 + b x y + c y^2 + d x + e y + f = 0`, with `a + c = 1` and the samples centred
    // and scaled: `a (x^2 - y^2) + b x y + d x + e y + f = -y^2`.
    let mut m = Mat::zeros(5, 5);
    let mut rhs = [0.0f64; 5];
    for p in samples {
        let q = (*p - mean) / rms;
        let row = [q.x * q.x - q.y * q.y, q.x * q.y, q.x, q.y, 1.0];
        for i in 0..5 {
            for j in 0..5 {
                m.add(i, j, row[i] * row[j]);
            }
            rhs[i] += row[i] * -q.y * q.y;
        }
    }
    let sol = solve(m, rhs.to_vec())?;
    let (a, b, d, e, f) = (sol[0], sol[1], sol[2], sol[3], sol[4]);
    let c = 1.0 - a;
    let disc = a * c - (b / 2.0) * (b / 2.0);
    if disc <= 0.0 {
        return None;
    }
    let dm = 4.0 * disc;
    if dm.abs() < 1e-300 {
        return None;
    }
    let cx = (-d * 2.0 * c + b * e) / dm;
    let cy = (b * d - 2.0 * a * e) / dm;
    let fc = a * cx * cx + b * cx * cy + c * cy * cy + d * cx + e * cy + f;
    if fc >= 0.0 {
        return None;
    }
    let tr = a + c;
    let disc2 = ((a - c) * (a - c) + b * b).max(0.0).sqrt();
    let (l1, l2) = ((tr + disc2) / 2.0, (tr - disc2) / 2.0);
    if l2 <= 0.0 {
        return None;
    }
    let (vx, vy) = if b.abs() > 1e-18 {
        (l1 - c, b / 2.0)
    } else if a >= c {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };
    let axis = Vec2::new(vx, vy).normalized()?;
    let (mut r1, mut r2) = ((-fc / l1).sqrt(), (-fc / l2).sqrt());
    let mut major = axis;
    if r1 < r2 {
        std::mem::swap(&mut r1, &mut r2);
        major = axis.perp();
    }
    if !(r1.is_finite() && r2.is_finite() && r1 > 0.0 && r2 > 0.0) {
        return None;
    }
    if r1 / r2 > ELLIPSE_MAX_RATIO {
        return None;
    }
    Some((mean + Vec2::new(cx, cy) * rms, r1 * rms, r2 * rms, major))
}

/// How far `p` lies from the ellipse `(centre, a, b)` along its major axis `u`, in model units:
/// the normalised algebraic distance `|F(p)| / |grad F(p)|`.
fn ellipse_residual(p: Vec2, centre: Vec2, a: f64, b: f64, u: Vec2) -> f64 {
    let w = u.perp();
    let d = p - centre;
    let up = d.dot(u);
    let wp = d.dot(w);
    let f = (up / a) * (up / a) + (wp / b) * (wp / b) - 1.0;
    let gx = 2.0 * up * u.x / (a * a) + 2.0 * wp * w.x / (b * b);
    let gy = 2.0 * up * u.y / (a * a) + 2.0 * wp * w.y / (b * b);
    let g = (gx * gx + gy * gy).sqrt();
    if g > 0.0 { f.abs() / g } else { f64::INFINITY }
}

/// The ellipse `(centre, a, b)` with its major axis `u`, the point at its parameter `t`.
fn ellipse_point(centre: Vec2, a: f64, b: f64, u: Vec2, t: f64) -> Vec2 {
    let w = u.perp();
    centre + u * (a * t.cos()) + w * (b * t.sin())
}

/// The ellipse's own parameter of `p`: where it sits on the ellipse.
fn ellipse_param(p: Vec2, centre: Vec2, a: f64, b: f64, u: Vec2) -> f64 {
    let w = u.perp();
    let d = p - centre;
    ((d.dot(w)) / b).atan2((d.dot(u)) / a)
}

/// The ellipse through `samples` when they all lie on one: the whole of it when the samples
/// close on themselves, or rational quadratic pieces of at most 90 degrees when they do not.
/// A drafted face's edge is a circle seen at a slant, so it comes to the sketch as an ellipse
/// and not as the point set a spline of its own would be; `None` leaves the caller with what it
/// had.
fn elliptical(samples: &[Vec2]) -> Option<Vec<LinkGeom>> {
    let (centre, a, b, u) = ls_ellipse(samples)?;
    let tol = a * ELLIPSE_TOL;
    if samples.iter().any(|p| ellipse_residual(*p, centre, a, b, u) > tol) {
        return None;
    }
    let mut ts: Vec<f64> = samples.iter().map(|p| ellipse_param(*p, centre, a, b, u)).collect();
    for i in 1..ts.len() {
        while ts[i] - ts[i - 1] > std::f64::consts::PI {
            ts[i] -= std::f64::consts::TAU;
        }
        while ts[i] - ts[i - 1] < -std::f64::consts::PI {
            ts[i] += std::f64::consts::TAU;
        }
    }
    let sweep = ts[ts.len() - 1] - ts[0];
    if sweep.abs() < ELLIPSE_MIN_TURN {
        return None;
    }
    if samples[0].dist(samples[samples.len() - 1]) <= tol {
        // The whole ellipse: its centre, the end of its major axis and its minor radius.
        return Some(vec![LinkGeom::Ellipse { c: centre, major: centre + u * a, minor: b }]);
    }
    // An arc of it: rational quadratic pieces of at most 90 degrees, the weights the circle's
    // own arcs use (`w = cos(sweep / 2)` of the half piece).
    let k = ((sweep.abs() / (std::f64::consts::PI / 2.0)).ceil() as usize).clamp(1, 8);
    let h = sweep / k as f64;
    let mut out = Vec::new();
    for i in 0..k {
        let t0 = ts[0] + h * i as f64;
        let t1 = ts[0] + h * (i + 1) as f64;
        let mid = (t0 + t1) / 2.0;
        let half = (t1 - t0) / 2.0;
        let pa = ellipse_point(centre, a, b, u, t0);
        let pb = ellipse_point(centre, a, b, u, t1);
        let cd = half.cos();
        if cd.abs() < 1e-12 {
            return None;
        }
        let apex = ellipse_point(centre, a / cd, b / cd, u, mid);
        let w = cd;
        out.push(LinkGeom::Conic { a: pa, apex, b: pb, rho: w / (1.0 + w) });
    }
    Some(out)
}

/// [`CIRCLE_SAMPLES`] + 1 points along a piece of an edge, ends included; `None` for a spline
/// whose knots don't fit its points.
fn piece_samples(piece: &crate::kernel::PlanarPiece) -> Option<Vec<Vec2>> {
    let at = |i: u16| f64::from(i) / f64::from(CIRCLE_SAMPLES);
    match piece {
        crate::kernel::PlanarPiece::Seg(s) => Some((0..=CIRCLE_SAMPLES).map(|i| s.point_at(at(i))).collect()),
        crate::kernel::PlanarPiece::Spline(s) => {
            let degree = u8::try_from(s.degree).ok()?;
            let nurbs = solidcraft_sketch::Nurbs { knots: s.knots.clone(), weights: s.weights.clone() };
            (0..=CIRCLE_SAMPLES).map(|i| nurbs.point(&s.points, degree, at(i))).collect()
        }
    }
}

/// `edges` of `body` projected on `plane`: each exactly ([`exact_edges`]) or fitted to its
/// polyline in `mesh`.
fn projected_edges(body: &crate::ModelBody, mesh: &Mesh, plane: &Plane, edges: &[usize]) -> Vec<LinkGeom> {
    let mut out = projected_edges_apart(body, mesh, plane, edges);
    meet_at_shared_ends(&mut out, order::join_tolerance(mesh));
    out
}

/// Where curve ends lie within `gap` of each other, moves them onto the first of them. A fitted
/// round's edges end a few 1e-5 mm short of their neighbours (#8284); the sketch only shares a
/// point between ends 1e-7 apart, and without it the outline has no closed profile.
fn meet_at_shared_ends(geom: &mut [LinkGeom], gap: f64) {
    let mut seen: Vec<Vec2> = Vec::new();
    let mut snap = |p: &mut Vec2| match seen.iter().find(|q| q.dist(*p) <= gap) {
        Some(q) => *p = *q,
        None => seen.push(*p),
    };
    for g in geom {
        match g {
            LinkGeom::Line(a, b) | LinkGeom::Arc { a, b, .. } | LinkGeom::Conic { a, b, .. } => {
                snap(a);
                snap(b);
            }
            LinkGeom::Spline { pts, periodic: false, .. } => {
                if let Some(first) = pts.first_mut() {
                    snap(first);
                }
                if let Some(last) = pts.last_mut() {
                    snap(last);
                }
            }
            _ => {}
        }
    }
}

/// Each of `edges` projected on its own ([`projected_edges`] without the ends brought together).
fn projected_edges_apart(body: &crate::ModelBody, mesh: &Mesh, plane: &Plane, edges: &[usize]) -> Vec<LinkGeom> {
    let mut out = Vec::new();
    for (e, exact) in edges.iter().zip(exact_edges(body, plane, edges)) {
        match exact {
            Some(mut g) => {
                at_vertices(body, mesh, plane, *e, &mut g);
                out.extend(g);
            }
            None => {
                if let Some(pts) = mesh.edges.get(*e) {
                    out.extend(fit_world(plane, pts));
                }
            }
        }
    }
    out
}

/// Edge `e`'s exact pieces `g` with their outer ends on the edge's vertices, where they lie
/// within the edge's and the reading's tolerance of them. An edge's curve need only pass within
/// its tolerance of its vertices, and an arc is read off a NURBS within the planar-edge
/// tolerance: one SMLib made (a shell's offset corner) ended 2e-6 mm from the line it meets, and
/// the two ends, not taken for one point, left the loop open, so an offset of one curve found
/// no chain to follow. The vertex is the point both edges share.
fn at_vertices(body: &crate::ModelBody, mesh: &Mesh, plane: &Plane, e: usize, g: &mut [LinkGeom]) {
    let record = body.body.record();
    let Some(edge) = record.edges.get(e) else { return };
    let Some((first, last)) = mesh.edges.get(e).and_then(|pts| pts.first().zip(pts.last())) else { return };
    // The pieces are read within the planar-edge tolerance of the edge's curve, which passes
    // within its own tolerance of the vertices.
    let tol = edge.vertices.iter().filter_map(|v| record.vertices.get(*v as usize)).map(|v| v.tolerance).fold(edge.tolerance, f64::max)
        + body.body.planar_edges_tolerance();
    // Each vertex moves the piece end nearest it (the pieces' order and direction need not be
    // the edge's).
    for v in [plane.to_local(*first), plane.to_local(*last)] {
        let mut best: Option<(f64, &mut Vec2)> = None;
        for piece in g.iter_mut() {
            let ends = match piece {
                LinkGeom::Line(a, b) | LinkGeom::Arc { a, b, .. } | LinkGeom::Conic { a, b, .. } => Some((a, b)),
                // A control spline's end points are its ends.
                LinkGeom::Spline { pts, control: true, periodic: false, .. } => match pts.as_mut_slice() {
                    [a, .., b] => Some((a, b)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((a, b)) = ends {
                for p in [a, b] {
                    let d = p.dist(v);
                    if d <= tol && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, p));
                    }
                }
            }
        }
        if let Some((_, p)) = best {
            *p = v;
        }
    }
    // An arc is its centre and two ends on one circle: with its ends moved, its centre moves to
    // the nearest point equally far from both (on their bisector), or the sketch would move an
    // end back off the vertex onto the circle through the other.
    for piece in g.iter_mut() {
        if let LinkGeom::Arc { c, a, b } = piece
            && let Some(across) = Vec2::new(a.y - b.y, b.x - a.x).normalized()
        {
            let mid = (*a + *b) * 0.5;
            *c = mid + across * (*c - mid).dot(across);
        }
    }
}

/// A boundary segment as the link geometry that is the same curve.
fn link_geom(s: &Seg2) -> Option<LinkGeom> {
    Some(match *s {
        Seg2::Line { a, b } => LinkGeom::Line(a, b),
        Seg2::Arc { center, radius, sweep, .. } if sweep.abs() >= std::f64::consts::TAU - 1e-9 => LinkGeom::Circle(center, radius),
        Seg2::Arc { center, radius, start, sweep } => {
            let at = |t: f64| center + Vec2::new(t.cos(), t.sin()) * radius;
            // Counter-clockwise from `a` to `b`.
            let (a, b) = if sweep > 0. { (at(start), at(start + sweep)) } else { (at(start + sweep), at(start)) };
            LinkGeom::Arc { c: center, a, b }
        }
        Seg2::Conic { a, apex, b, w } => LinkGeom::Conic { a, apex, b, rho: w / (1. + w) },
        Seg2::Cubic { p0, p1, p2, p3 } => LinkGeom::Spline { pts: vec![p0, p1, p2, p3], control: true, degree: 3, periodic: false, nurbs: None },
        Seg2::Bezier { n, p } => LinkGeom::Spline { pts: p.get(..=usize::from(n))?.to_vec(), control: true, degree: n, periodic: false, nurbs: None },
    })
}

/// The persistent name of the edge an `Edge` reference resolved to, for its link to keep and
/// be found by from then on. None on a mesh body, whose edges have no names.
pub fn resolved_edge_name(st: &ModelState, r: &Resolved) -> Option<String> {
    let (LinkSource::Edge { body, .. }, Some(index)) = (&r.source, r.edge_index) else { return None };
    let body = st.body(body).filter(|body| !body.body.is_mesh())?;
    crate::naming::edge_names(body).get(index).filter(|name| !name.is_empty()).cloned()
}

/// The persistent name of the vertex a `Vertex` reference resolved to, for its link to keep and
/// be found by from then on. None on a mesh body, whose vertices have no names.
pub fn resolved_vertex_name(st: &ModelState, r: &Resolved) -> Option<String> {
    let (LinkSource::Vertex { body, .. }, Some(index)) = (&r.source, r.vertex_index) else { return None };
    let body = st.body(body).filter(|body| !body.body.is_mesh())?;
    crate::naming::vertex_names(body).get(index).filter(|name| !name.is_empty()).cloned()
}

fn lost(reference: &str) -> DocError {
    DocError::Unknown(format!("{reference} no longer exists"))
}

/// The edge a link's stored name finds, and the body it is on: the link's body, or, when no
/// body has that name any more (it was joined into another), the one body on which the name
/// finds an edge.
fn locate_named_edge(st: &ModelState, kind: LinkKind, body: &str, name: &str, at: Vec3) -> Option<(String, crate::naming::EdgeHit)> {
    let locate = |b: &crate::ModelBody| match kind {
        LinkKind::Project => crate::name_lookup::locate_projected_edge(b, name, at),
        _ => crate::naming::locate_edge(b, name, at),
    };
    let hit = |b: &crate::ModelBody| locate(b).ok().map(|hit| (b.name.clone(), hit));
    match st.body(body) {
        Some(b) => hit(b),
        None => {
            let mut found = st.bodies.iter().filter(|b| !b.body.is_mesh()).filter_map(hit);
            match (found.next(), found.next()) {
                (Some(one), None) => Some(one),
                _ => None,
            }
        }
    }
}

/// The vertex a link's stored name finds, and the body it is on: the link's body, or, when no
/// body has that name any more (it was joined into another), the one body on which the name
/// finds a vertex.
fn locate_named_vertex(st: &ModelState, kind: LinkKind, body: &str, name: &str, at: Vec3) -> Option<(String, crate::naming::VertexHit)> {
    let locate = |b: &crate::ModelBody| match kind {
        LinkKind::Project => crate::name_lookup::locate_projected_vertex(b, name, at),
        _ => crate::naming::locate_vertex(b, name, at),
    };
    let hit = |b: &crate::ModelBody| locate(b).ok().map(|hit| (b.name.clone(), hit));
    match st.body(body) {
        Some(b) => hit(b),
        None => {
            let mut found = st.bodies.iter().filter(|b| !b.body.is_mesh()).filter_map(hit);
            match (found.next(), found.next()) {
                (Some(one), None) => Some(one),
                _ => None,
            }
        }
    }
}

/// An edge found by its persistent name: edge `index` of `body`, with the link's point `near`
/// moved onto it.
fn resolve_named_edge(st: &ModelState, plane: &Plane, kind: LinkKind, body: &str, index: usize, near: Vec3) -> Result<Resolved> {
    let target = st.body(body).ok_or_else(|| lost("named projection body"))?;
    let pts = target.mesh().edges.get(index).cloned().unwrap_or_default();
    let at = pts
        .windows(2)
        .filter_map(|w| Some(closest_on_segment(near, *w.first()?, *w.get(1)?)))
        .min_by(|a, b| a.dist(near).total_cmp(&b.dist(near)))
        .ok_or_else(|| lost("named projection edge"))?;
    let (mut wires, mut exact) = (Vec::new(), Vec::new());
    let planar = exact_edges(target, plane, &[index]).pop().flatten();
    let geom = edge_geom(plane, kind, planar, (pts, || edge_curves(Some(target), index)), &mut wires, &mut exact);
    if geom.is_empty() && wires.is_empty() {
        return Err(DocError::Invalid("named edge projects to nothing on this sketch plane".into()));
    }
    let source = LinkSource::Edge { body: body.into(), at };
    Ok(Resolved {
        geom: merge_split_circles(geom),
        wires,
        exact,
        source,
        moved: false,
        face_index: None,
        edge_index: Some(index),
        vertex_index: None,
    })
}

/// A vertex found by its persistent name: vertex `index` of `body` at its evaluated point `at`.
fn resolve_named_vertex(plane: &Plane, body: &str, index: usize, at: Vec3) -> Result<Resolved> {
    if !at.is_finite() {
        return Err(lost("named projection vertex"));
    }
    Ok(Resolved {
        geom: vec![LinkGeom::Point(plane.to_local(at))],
        wires: Vec::new(),
        exact: Vec::new(),
        source: LinkSource::Vertex { body: body.into(), at },
        moved: false,
        face_index: None,
        edge_index: None,
        vertex_index: Some(index),
    })
}

fn resolve_named_face(st: &ModelState, plane: &Plane, kind: LinkKind, body: &str, index: usize, at: Vec3) -> Result<Resolved> {
    let target = st.body(body).ok_or_else(|| lost("named projection body"))?;
    let mesh = target.mesh();
    if mesh.triangles.len() > 2_000_000 || mesh.edges.len() > 2_000_000 {
        return Err(DocError::Invalid("named face projection work limit exceeded".into()));
    }
    let face_index = Some(index);
    let index = u32::try_from(index).map_err(|_| DocError::Invalid("named projection face index exceeds limit".into()))?;
    let geom = if kind == LinkKind::Intersect {
        section(plane, &mesh, Some(index))
    } else {
        let geom = projected_edges(target, &mesh, plane, &projection_face_edges(target, &mesh, index, plane)?);
        let geom = merge_split_circles(geom);
        if certified_planar_construction(target, index as usize) { merge_family_lines(geom)? } else { geom }
    };
    if geom.is_empty() {
        return Err(DocError::Invalid("named face projects to nothing on this sketch plane".into()));
    }
    Ok(Resolved {
        geom,
        wires: Vec::new(),
        exact: Vec::new(),
        source: LinkSource::Face { body: body.into(), at },
        moved: false,
        face_index,
        edge_index: None,
        vertex_index: None,
    })
}

fn projection_face_edges(body: &crate::ModelBody, mesh: &Mesh, index: u32, plane: &Plane) -> Result<Vec<usize>> {
    let shell_plane = body
        .body
        .face_sources()
        .and_then(|sources| sources.get(index as usize))
        .is_some_and(|source| matches!(source, Some(solidcraft_kernel::FaceSource::ShellPlanar { .. })));
    if shell_plane { order::raw_face_edges(mesh, index, plane) } else { order::face_edges(mesh, index, plane) }
}

fn certified_planar_construction(body: &crate::ModelBody, index: usize) -> bool {
    body.body.face_sources().and_then(|sources| sources.get(index)).is_some_and(|source| {
        matches!(
            source,
            Some(
                solidcraft_kernel::FaceSource::ShellPlanar { producer: Some(_), .. }
                    | solidcraft_kernel::FaceSource::Box { producer: Some(_), .. }
                    | solidcraft_kernel::FaceSource::CylinderCap { .. }
                    | solidcraft_kernel::FaceSource::CertifiedExtrudeSide { plane: Some(_), .. }
                    | solidcraft_kernel::FaceSource::CertifiedExtrudeCap { .. }
            )
        )
    })
}

pub fn resolve_anchored_face(doc: &Document, st: &ModelState, plane: &Plane, kind: LinkKind, anchor: &crate::FaceAnchor) -> Result<Resolved> {
    let body = st.body(&anchor.target.body).ok_or_else(|| lost("anchored projection body"))?;
    // The sketch's plane says when the face was found by fallback; its projection follows it.
    let (indices, _) = doc.anchored_face_indices(st, body, &anchor.target)?;
    resolve_face_indices(st, plane, kind, &body.name, &indices)
}

/// Project explicitly selected current face indices, without claiming persistent provenance.
/// Callers retaining a link must supply their own replay identity policy.
pub fn resolve_face_indices(st: &ModelState, plane: &Plane, kind: LinkKind, body_name: &str, indices: &[usize]) -> Result<Resolved> {
    let body = st.body(body_name).ok_or_else(|| lost("projection body"))?;
    let mesh = body.mesh();
    if mesh.triangles.len() > 2_000_000 || indices.len() > 10_000 {
        return Err(DocError::Invalid("anchored projection work limit exceeded".into()));
    }
    let work = mesh.triangles.len().saturating_add(mesh.edges.len()).saturating_mul(indices.len());
    if work > 10_000_000 {
        return Err(DocError::Invalid("anchored projection aggregate work limit exceeded".into()));
    }
    let selected: std::collections::BTreeSet<_> = indices.iter().copied().collect();
    let mut geometry = Vec::new();
    if kind == LinkKind::Intersect {
        for index in indices {
            let index = u32::try_from(*index).map_err(|_| DocError::Invalid("anchored face index exceeds format bounds".into()))?;
            geometry.extend(section(plane, &mesh, Some(index)));
        }
    } else {
        let edges = if let [index] = indices {
            projection_face_edges(
                body,
                &mesh,
                u32::try_from(*index).map_err(|_| DocError::Invalid("face index exceeds format bounds".into()))?,
                plane,
            )?
        } else {
            order::family_edges(&mesh, &selected, plane)?
        };
        geometry.extend(projected_edges(body, &mesh, plane, &edges));
    }
    if kind != LinkKind::Intersect && (indices.len() > 1 || indices.first().is_some_and(|index| certified_planar_construction(body, *index))) {
        geometry = merge_family_lines(geometry)?;
    }
    let point = mesh
        .triangles
        .iter()
        .zip(&mesh.tri_face)
        .find_map(|(triangle, face)| selected.contains(&(*face as usize)).then(|| mesh.tri(triangle)).flatten().map(|[a, b, c]| (a + b + c) / 3.0))
        .ok_or_else(|| lost("anchored projection face"))?;
    if geometry.is_empty() {
        return Err(lost("anchored projection boundaries"));
    }
    Ok(Resolved {
        geom: merge_split_circles(geometry),
        wires: Vec::new(),
        exact: Vec::new(),
        source: LinkSource::Face { body: body.name.clone(), at: point },
        moved: false,
        face_index: if indices.len() == 1 { indices.first().copied() } else { None },
        edge_index: None,
        vertex_index: None,
    })
}

/// [`refresh_links`] for a sketch attached to a face by `anchor`.
pub fn refresh_face_anchor_links(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    plane: &Plane,
    sk: &mut Sketch,
    anchor: &crate::FaceAnchor,
) -> Result<Vec<String>> {
    refresh_links_in(doc, vals, st, plane, sk, Some(anchor), LostNames::Fail, None)
}

/// What a link kept by the name of a face or edge does when the name finds nothing any more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LostNames {
    /// The sketch fails, asking for the reference to be picked again ([`DocError::Reselect`]).
    Fail,
    /// The link is marked lost and keeps its last geometry, with a warning naming the reference.
    /// No other face or edge is taken for it. Only for showing a failed sketch while it is edited
    /// (#12085), never for the evaluation the features after it build from.
    Mark,
}

/// [`refresh_links`] (or [`refresh_face_anchor_links`] with `anchor`) for a sketch evaluated in
/// `frame`, as [`resolve_at`] takes it.
pub fn refresh_links_at(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    plane: &Plane,
    sk: &mut Sketch,
    anchor: Option<&crate::FaceAnchor>,
    frame: Option<&(u64, crate::Mat)>,
) -> Result<Vec<String>> {
    refresh_links_in(doc, vals, st, plane, sk, anchor, LostNames::Fail, frame)
}

/// [`refresh_links_at`] with `lost_names` [`LostNames::Mark`] for a link whose named face or edge
/// is gone to be marked lost instead of failing the sketch.
pub(crate) fn refresh_links_with(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    plane: &Plane,
    sk: &mut Sketch,
    anchor: Option<&crate::FaceAnchor>,
    lost_names: LostNames,
    frame: Option<&(u64, crate::Mat)>,
) -> Result<Vec<String>> {
    refresh_links_in(doc, vals, st, plane, sk, anchor, lost_names, frame)
}

/// Remove degree-two straight stations introduced by coplanar face subdivision.
/// The walk uses endpoint incidence; geometric sorting does not imply adjacency.
fn merge_family_lines(geom: Vec<LinkGeom>) -> Result<Vec<LinkGeom>> {
    if geom.len().saturating_mul(geom.len()).saturating_mul(3) > 10_000_000 {
        return Err(DocError::Invalid("face family straight-boundary work limit exceeded".into()));
    }
    let ends = |curve: &LinkGeom| match curve {
        LinkGeom::Line(a, b) | LinkGeom::Arc { a, b, .. } | LinkGeom::Conic { a, b, .. } => Some((*a, *b)),
        LinkGeom::Spline { pts, periodic: false, .. } => pts.first().zip(pts.last()).map(|(a, b)| (*a, *b)),
        _ => None,
    };
    let scale = geom.iter().filter_map(&ends).flat_map(|(a, b)| [a.len(), b.len()]).fold(1., f64::max);
    let tol = scale * f64::EPSILON * 512.;
    let mut used = vec![false; geom.len()];
    let mut result = Vec::new();
    for (index, curve) in geom.iter().enumerate() {
        if used.get(index).copied().unwrap_or(true) {
            continue;
        }
        if let Some(slot) = used.get_mut(index) {
            *slot = true;
        }
        let LinkGeom::Line(a, b) = curve else {
            result.push(curve.clone());
            continue;
        };
        let mut endpoints = [*a, *b];
        for side in 0..2 {
            let mut current = index;
            for _ in 0..geom.len() {
                let Some(point) = endpoints.get(side).copied() else { break };
                let mut incidence = geom
                    .iter()
                    .enumerate()
                    .filter_map(|(other, curve)| ends(curve).filter(|(a, b)| a.dist(point) <= tol || b.dist(point) <= tol).map(|_| other));
                let (Some(one), Some(two), None) = (incidence.next(), incidence.next(), incidence.next()) else { break };
                let other = if one == current {
                    two
                } else if two == current {
                    one
                } else {
                    break;
                };
                if used.get(other).copied().unwrap_or(true) {
                    break;
                }
                let Some(LinkGeom::Line(a, b)) = geom.get(other) else { break };
                let next = if a.dist(point) <= tol { *b } else { *a };
                let Some(opposite) = endpoints.get(1 - side).copied() else { break };
                let (Some(incoming), Some(outgoing)) = ((point - opposite).normalized(), (next - point).normalized()) else { break };
                if incoming.dot(outgoing) <= 0. || incoming.cross(outgoing).abs() > f64::EPSILON * 512. {
                    break;
                }
                if let Some(slot) = used.get_mut(other) {
                    *slot = true;
                }
                if let Some(slot) = endpoints.get_mut(side) {
                    *slot = next;
                }
                current = other;
            }
        }
        if let (Some(a), Some(b)) = (endpoints.first(), endpoints.get(1)) {
            result.push(LinkGeom::Line(*a, *b));
        }
    }
    Ok(result)
}

/// Re-resolve every link of a sketch (in place). Returns warnings for lost, moved or rebuilt
/// links; lost links keep their last geometry. A link kept by the name of a face or edge that no
/// longer finds exactly that face or edge is not lost: the sketch fails, asking for the
/// reference to be picked again ([`DocError::Reselect`]).
pub fn refresh_links(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, plane: &Plane, sk: &mut Sketch) -> Result<Vec<String>> {
    refresh_links_in(doc, vals, st, plane, sk, None, LostNames::Fail, None)
}

/// Lay every text that runs along a curve of its sketch along that curve as the solve left it.
/// The texts are laid out with the other links, before the solve, which can move their path (a
/// path dimensioned from a parameter): without this a text followed its path one evaluation
/// late. The warnings for texts that no longer lay out.
pub(crate) fn relay_path_texts(sk: &mut Sketch) -> Vec<String> {
    let texts: Vec<(String, LinkSource)> = sk
        .links
        .iter()
        .filter(|l| !l.lost && matches!(&l.source, LinkSource::Text { style, .. } if style.path.is_some()))
        .map(|l| (l.id.clone(), l.source.clone()))
        .collect();
    let mut warn = Vec::new();
    for (id, source) in texts {
        match sk.text_source_geometry(&source) {
            Ok(geom) if geom.is_empty() => warn.push(format!("the {} makes no outline", source.describe())),
            Ok(geom) => {
                if let Err(e) = sk.update_link(&id, &geom) {
                    warn.push(format!("{}: {e}", source.describe()));
                }
            }
            Err(e) => warn.push(format!("{}: {e}", source.describe())),
        }
    }
    warn
}

fn refresh_links_in(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    plane: &Plane,
    sk: &mut Sketch,
    anchor: Option<&crate::FaceAnchor>,
    lost_names: LostNames,
    frame: Option<&(u64, crate::Mat)>,
) -> Result<Vec<String>> {
    let mut warn = Vec::new();
    let links = sk.links.clone();
    for l in &links {
        let mut source = l.source.clone();
        let mut named_face = None;
        if !l.auto_face
            && let (Some(name), LinkSource::Face { body, at }) = (&l.face_name, &mut source)
        {
            let found = st.body(body).and_then(|target| {
                if l.kind == LinkKind::Project {
                    crate::name_lookup::locate_projected_face(target, name, *at)
                } else {
                    crate::naming::face_on_body(target, name, *at)
                }
            });
            let Some((index, p)) = found else {
                lose_named(sk, &l.id, name, lost_names, &mut warn)?;
                continue;
            };
            *at = p;
            named_face = Some((body.clone(), index, p));
        }
        // An edge the link keeps by name is that edge of its body, wherever it went. When no
        // edge has the name any more no other edge is taken for it: the sketch fails.
        let mut named_edge = None;
        if let (Some(name), LinkSource::Edge { body, at }) = (&l.edge_name, &source) {
            let Some((body, hit)) = locate_named_edge(st, l.kind, body, name, *at) else {
                lose_named(sk, &l.id, name, lost_names, &mut warn)?;
                continue;
            };
            warn.extend(hit.note(name).map(|note| format!("projected {note}")));
            named_edge = Some((body, hit.edge, *at));
        }
        // A vertex the link keeps by name is that vertex of its body, wherever it went. When no
        // vertex has the name any more no other vertex is taken for it: the sketch fails.
        let mut named_vertex = None;
        if let (Some(name), LinkSource::Vertex { body, at }) = (&l.vertex_name, &source) {
            let Some((body, hit)) = locate_named_vertex(st, l.kind, body, name, *at) else {
                lose_named(sk, &l.id, name, lost_names, &mut warn)?;
                continue;
            };
            warn.extend(hit.note(name).map(|note| format!("projected {note}")));
            named_vertex = Some((body, hit.vertex, hit.point));
        }
        let resolved = if matches!(source, LinkSource::Text { .. }) {
            // A text needs nothing outside its sketch; one on a path follows a curve of it.
            sk.text_source_geometry(&source).map_err(DocError::from).and_then(|geom| {
                if geom.is_empty() {
                    return Err(DocError::Invalid(format!("the {} makes no outline", source.describe())));
                }
                Ok(Resolved {
                    geom,
                    wires: Vec::new(),
                    exact: Vec::new(),
                    source: source.clone(),
                    moved: false,
                    face_index: None,
                    edge_index: None,
                    vertex_index: None,
                })
            })
        } else if l.auto_face {
            anchor
                .filter(|anchor| {
                    l.face_name.as_ref() == Some(&anchor.target.name) && matches!(&l.source,LinkSource::Face {body,..} if *body==anchor.target.body)
                })
                .ok_or_else(|| lost("exact auto-projected attachment"))
                .and_then(|anchor| resolve_anchored_face(doc, st, plane, l.kind, anchor))
        } else if let Some((body, index, at)) = named_face {
            resolve_named_face(st, plane, l.kind, &body, index, at)
        } else if let Some((body, index, at)) = named_edge {
            resolve_named_edge(st, plane, l.kind, &body, index, at)
        } else if let Some((body, index, at)) = named_vertex {
            resolve_named_vertex(plane, &body, index, at)
        } else {
            resolve_at(doc, vals, st, plane, l.kind, &source, frame)
        };
        match resolved {
            Ok(r) => {
                // An edge or vertex link saved without a name takes the name of the geometry it was
                // found on. A sketch that is edited stores it; evaluation leaves the design as it is.
                if l.edge_name.is_none()
                    && let Some(name) = resolved_edge_name(st, &r)
                    && let Some(link) = sk.links.iter_mut().find(|link| link.id == l.id)
                {
                    link.edge_name = Some(name);
                }
                if l.vertex_name.is_none()
                    && let Some(name) = resolved_vertex_name(st, &r)
                    && let Some(link) = sk.links.iter_mut().find(|link| link.id == l.id)
                {
                    link.vertex_name = Some(name);
                }
                sk.set_link_source(&l.id, r.source);
                if let Err(e) = sk.set_link_wires(&l.id, &r.wires).and_then(|()| sk.set_link_wire_curves(&l.id, &r.exact)) {
                    warn.push(format!("projected {}: {e}", l.source.describe()));
                }
                // Words built from text parameters change shape when the parameters do: that
                // rebuild is the text following them, not a reference that changed under it.
                let words_follow = matches!(&l.source, LinkSource::Text { exprs, .. } if exprs.text.is_some());
                let others = |sk: &Sketch| (sk.constraints.len(), sk.curves.len().saturating_sub(sk.link_curves(&l.id).len()));
                let before = others(sk);
                match sk.update_link(&l.id, &r.geom) {
                    Ok(true) if words_follow => {}
                    // A rebuilt projection is only worth a warning when it took something of
                    // the user's with it: a constraint, a dimension or a curve drawn on it.
                    Ok(true) if others(sk) == before => {}
                    Ok(true) => warn.push(format!("projected {} changed shape and was rebuilt; constraints on it were removed", l.source.describe())),
                    Ok(false) if r.moved => warn.push(format!("projected {} was re-found on the nearest geometry", l.source.describe())),
                    Ok(false) => {}
                    Err(e) => {
                        sk.set_link_lost(&l.id, true);
                        warn.push(format!("projected {}: {e}", l.source.describe()));
                    }
                }
            }
            Err(e) => {
                sk.set_link_lost(&l.id, true);
                warn.push(format!("projected reference lost: {e}"));
            }
        }
    }
    Ok(warn)
}

/// A link whose face or edge `name` finds nothing: the sketch fails asking for it again, or (only
/// while a failed sketch is shown for editing) the link is marked lost, with a warning naming it.
fn lose_named(sk: &mut Sketch, id: &str, name: &str, lost_names: LostNames, warn: &mut Vec<String>) -> Result<()> {
    match lost_names {
        LostNames::Fail => Err(DocError::reselect(name)),
        LostNames::Mark => {
            sk.set_link_lost(id, true);
            warn.push(format!("projected reference lost: `{name}` no longer exists as it was picked; reselect it"));
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Model lookups

/// The body by name (or, when it is gone or unnamed, the one with geometry nearest `at`) and
/// its display mesh.
fn body_mesh(st: &ModelState, name: &str, at: Vec3) -> Result<(usize, std::sync::Arc<Mesh>)> {
    if let Some(i) = st.bodies.iter().position(|b| b.name == name)
        && let Some(b) = st.bodies.get(i)
    {
        return Ok((i, b.mesh()));
    }
    let mut best: Option<(usize, f64)> = None;
    for (i, b) in st.bodies.iter().enumerate() {
        let m = b.mesh();
        if let Some((_, _, d)) = nearest_face(&m, at)
            && best.is_none_or(|(_, bd)| d < bd)
        {
            best = Some((i, d));
        }
    }
    let (i, _) = best.ok_or_else(|| DocError::Invalid(format!("body {name} no longer exists")))?;
    let b = st.bodies.get(i).ok_or_else(|| DocError::Invalid(format!("body {name} no longer exists")))?;
    Ok((i, b.mesh()))
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let l2 = ab.len2();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    a + ab * t
}

/// Nearest non-seam edge: (index, closest point, distance).
pub(crate) fn nearest_edge(m: &Mesh, p: Vec3) -> Option<(usize, Vec3, f64)> {
    let mut best: Option<(usize, Vec3, f64)> = None;
    for (i, e) in m.edges.iter().enumerate() {
        if m.seams.get(i).copied().unwrap_or(false) {
            continue;
        }
        for w in e.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            let q = closest_on_segment(p, *a, *b);
            let d = q.dist(p);
            if best.is_none_or(|(_, _, bd)| d < bd) {
                best = Some((i, q, d));
            }
        }
    }
    best
}

pub(crate) fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else { return closest_on_segment(p, a, b) };
    let q = p - nn * (p - a).dot(nn);
    let inside = (b - a).cross(q - a).dot(n) >= 0.0 && (c - b).cross(q - b).dot(n) >= 0.0 && (a - c).cross(q - c).dot(n) >= 0.0;
    if inside {
        return q;
    }
    [closest_on_segment(p, a, b), closest_on_segment(p, b, c), closest_on_segment(p, c, a)]
        .into_iter()
        .min_by(|x, y| x.dist(p).total_cmp(&y.dist(p)))
        .unwrap_or(a)
}

/// Faces considered when a click asks which hole opening it passed through.
const HOLE_MAX_FACES: usize = 8_000;
/// Edges of one body considered for that same question.
const HOLE_MAX_EDGES: usize = 20_000;
/// Edges in one opening. A circular rim is one circle or two semicircles; more is not one hole.
const HOLE_MAX_LOOP_EDGES: usize = 128;

/// The rim of a hole whose opening a ray passes through: each edge named by a point on it,
/// in world space.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleRim {
    /// Name of the body whose face the hole is cut through.
    pub body: String,
    /// Where the ray crosses the opening.
    pub at: Vec3,
    /// One point on each boundary edge of that opening.
    pub edges: Vec<Vec3>,
}

/// The nearest hole opening `origin + t·dir` (t > 0) passes through.
///
/// A click in the dark centre of a hole hits no face: the opening is outside the trimmed B-rep.
/// The rim is the inner loop of the exact planar face the hole is cut through. `visible` skips
/// hidden bodies. `None` when the ray misses every opening, the inputs are not finite, or the
/// body is past the work limits above.
pub fn hole_rim_along_ray(st: &ModelState, origin: Vec3, dir: Vec3, visible: impl Fn(&str) -> bool) -> Option<HoleRim> {
    if !origin.is_finite() || !dir.is_finite() || dir.len2() < 1e-24 {
        return None;
    }
    let mut best: Option<(f64, HoleRim)> = None;
    for body in &st.bodies {
        // `body.body` is already placed exactly. A display mesh's reuse of a definition's pose
        // is not a reason to un-transform the B-rep.
        if !visible(&body.name) || body.body.is_mesh() || body.body.face_count() > HOLE_MAX_FACES || body.body.edge_count() > HOLE_MAX_EDGES {
            continue;
        }
        let Some((t, at, edges)) = opening_along_ray(&body.body, origin, dir) else { continue };
        let along = t * dir.len();
        if !(along > 1e-9) || edges.is_empty() || edges.len() > HOLE_MAX_LOOP_EDGES || edges.iter().any(|p| !p.is_finite()) {
            continue;
        }
        let rim = HoleRim { body: body.name.clone(), at, edges };
        if best.as_ref().is_none_or(|(d, _)| along < *d) {
            best = Some((along, rim));
        }
    }
    best.map(|(_, rim)| rim)
}

/// Distance along `dir` (not necessarily unit) to the nearest opening, the point there, and one
/// exact point on each of its full edges. All in the body's own coordinates.
fn opening_along_ray(body: &solidcraft_kernel::Body, origin: Vec3, dir: Vec3) -> Option<(f64, Vec3, Vec<Vec3>)> {
    let planes = planar_faces(body)?;
    let span = ray_bounds(origin, dir, &body.exact_bounds().ok()?)?;
    let mut best: Option<(f64, Vec3, Vec<Vec3>)> = None;
    for (face, plane) in planes.iter().enumerate() {
        let Some(plane) = plane else { continue };
        let den = dir.dot(plane.normal());
        if den.abs() < 1e-12 * dir.len().max(1.0) {
            continue;
        }
        let t = (plane.origin - origin).dot(plane.normal()) / den;
        if !(t.is_finite() && t >= span.0 && t <= span.1) || best.as_ref().is_some_and(|(d, _, _)| t >= *d) {
            continue;
        }
        let at = origin + dir * t;
        if !at.is_finite() || !matches!(body.classify_point(at), Ok(solidcraft_kernel::PointClass::Outside)) {
            continue;
        }
        let Some(edges) = hole_edges(body, face, at) else { continue };
        best = Some((t, at, edges));
    }
    best
}

/// The forward interval where a ray overlaps a body's exact box.
fn ray_bounds(origin: Vec3, dir: Vec3, bounds: &solidcraft_geom::Aabb3) -> Option<(f64, f64)> {
    let mut range: (f64, f64) = (0.0, f64::INFINITY);
    for (o, d, lo, hi) in
        [(origin.x, dir.x, bounds.min.x, bounds.max.x), (origin.y, dir.y, bounds.min.y, bounds.max.y), (origin.z, dir.z, bounds.min.z, bounds.max.z)]
    {
        if d.abs() <= 1e-14 * dir.len().max(1.0) {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let a = (lo - o) / d;
        let b = (hi - o) / d;
        range = (range.0.max(a.min(b)), range.1.min(a.max(b)));
    }
    let t = (range.0 > 1e-6).then_some(range.0).or_else(|| (range.1 > 1e-6).then_some(1e-6))?;
    (t.is_finite() && range.1.is_finite() && t <= range.1).then_some((t, range.1))
}

/// The exact plane of each B-rep face that has one, `None` for every other face.
fn planar_faces(body: &solidcraft_kernel::Body) -> Option<Vec<Option<Plane>>> {
    (!body.is_mesh() && body.face_count() <= HOLE_MAX_FACES).then(|| (0..body.face_count()).map(|face| body.planar_face(face)).collect())
}

/// One exact point on each edge of the innermost hole boundary containing `at`. Canonical loop
/// winding distinguishes holes from material: outer loops are CCW, inner loops CW in the face's
/// natural B-rep frame. Neither a display triangle nor a display polyline decides this.
fn hole_edges(body: &solidcraft_kernel::Body, face: usize, at: Vec3) -> Option<Vec<Vec3>> {
    let record = body.record().faces.get(face)?;
    let frame = match &record.surface {
        solidcore_record::brep::Surface::Plane { origin, u_axis, v_axis } => {
            let o = Vec3::from(*origin);
            (o, o + Vec3::from(*u_axis), o + Vec3::from(*v_axis))
        }
        solidcore_record::brep::Surface::PlaneThrough { origin, u_point, v_point } => {
            (Vec3::from(*origin), Vec3::from(*u_point), Vec3::from(*v_point))
        }
        _ => return None,
    };
    let native = Plane { origin: frame.0, x: frame.1 - frame.0, y: frame.2 - frame.0 };
    let (u, v) = plane_point(native, at)?;
    let mut loops = Vec::new();
    let mut work = 0usize;
    for lp in &record.loops {
        let solidcore_record::brep::Loop::Edges { edges } = lp else { continue };
        work = work.checked_add(edges.len())?;
        if work > HOLE_MAX_EDGES || edges.is_empty() || edges.len() > HOLE_MAX_LOOP_EDGES {
            return None;
        }
        // Body::exact_bounds gave the caller its ray interval. Loop orientation is independent
        // of exact body placement, including a reflected B-rep whose face frame changed.
        let area = loop_area(edges)?;
        loops.push((area, edges));
    }
    // The innermost containing negative loop is the hole, and its immediately larger
    // containing loop must be a positive material boundary. This also allows nested holes
    // and islands without relying on producer loop ordering.
    loops.sort_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));
    let containing: Vec<(f64, &[solidcore_record::brep::OrientedEdge])> =
        loops.iter().filter(|(_, edges)| loop_contains(edges, u, v)).map(|(area, edges)| (*area, edges.as_slice())).collect();
    let (&(area, edges), enclosing) = containing.split_first()?;
    if area >= 0.0 || enclosing.first().is_none_or(|(outer_area, _)| *outer_area <= 0.0) {
        return None;
    }

    edges
        .iter()
        .map(|used| {
            let edge = body.record().edges.get(usize::try_from(used.edge).ok()?)?;
            let [t0, t1] = edge.interval;
            solidcraft_geom::brep::eval::CurveEval::new(edge.parameters()).ok()?.point(t0.midpoint(t1)).filter(|p| p.is_finite())
        })
        .collect::<Option<Vec<_>>>()
        .filter(|points| !points.is_empty())
}

/// Point on a plane in its native B-rep surface coordinates. `PlaneThrough` axes are arbitrary
/// spanning vectors, whereas `Plane::new` orthonormalises them for sketch/body geometry.
fn plane_point(plane: Plane, at: Vec3) -> Option<(f64, f64)> {
    let d = at - plane.origin;
    let a = plane.x.len2();
    let b = plane.x.dot(plane.y);
    let c = plane.y.len2();
    let den = a * c - b * b;
    let x = d.dot(plane.x);
    let y = d.dot(plane.y);
    let (u, v) = ((x * c - y * b) / den, (y * a - x * b) / den);
    (den.abs() > 1e-24 && u.is_finite() && v.is_finite()).then_some((u, v))
}

/// Signed area of an ordered B-rep loop, evaluated on its exact pcurve geometry.
/// Homogeneous rational weights are retained: this does not flatten a rational circle to its
/// control polygon or consult the display tessellation.
fn loop_area(edges: &[solidcore_record::brep::OrientedEdge]) -> Option<f64> {
    let mut work = HOLE_MAX_AREA_WORK;
    let mut area = 0.0;
    for used in edges {
        let oriented = if used.forward { 1.0 } else { -1.0 };
        area += oriented * pcurve_spans(used, &mut work)?.iter().sum::<f64>();
    }
    (area != 0.0 && area.is_finite()).then_some(area)
}

/// Total spline decomposition and integration across one loop.
const HOLE_MAX_AREA_WORK: usize = 2_000_000;

/// Area contributions of an edge's exact pcurve over its active parameter interval. Each
/// polynomial piece of that curve becomes one Bézier, with every rational weight retained.
fn pcurve_spans(used: &solidcore_record::brep::OrientedEdge, work: &mut usize) -> Option<Vec<f64>> {
    use solidcraft_geom::bspline::{HPoint, insert_knot, segment};
    let [t0, t1] = used.pcurve.interval()?;
    let (knots, control) = match &used.pcurve {
        solidcore_record::brep::Curve2::Spline { knots, points } => {
            (knots.as_slice(), points.iter().map(|p| HPoint::weighted(Vec3::new(p[0], p[1], 0.0), 1.0)).collect::<Vec<_>>())
        }
        solidcore_record::brep::Curve2::RationalSpline { knots, homogeneous } => {
            (knots.as_slice(), homogeneous.iter().map(|p| HPoint { x: p[0], y: p[1], z: 0.0, w: p[2] }).collect::<Vec<_>>())
        }
    };
    let degree = knots.len().checked_sub(control.len() + 1)?;
    *work = work.checked_sub(control.len().checked_mul(knots.len())?.checked_mul(degree + 1)?)?;
    let (mut k, mut c) = segment(degree, knots, &control, t0, t1).ok()?;
    let mut interior: Vec<f64> = k.iter().copied().filter(|t| *t > t0 && *t < t1).collect();
    interior.dedup();
    for t in interior {
        while k.iter().filter(|q| **q == t).count() < degree {
            (k, c) = insert_knot(degree, &k, &c, t).ok()?;
        }
    }
    let mut spans = Vec::new();
    for i in degree..c.len() {
        let (a, b) = (*k.get(i)?, *k.get(i + 1)?);
        if a < b {
            *work = work.checked_sub(1)?;
            spans.push(bezier_area(c.get(i - degree..=i)?)?);
        }
    }
    (!spans.is_empty()).then_some(spans)
}

fn bezier_area(control: &[solidcraft_geom::bspline::HPoint]) -> Option<f64> {
    let point = |p: solidcraft_geom::bspline::HPoint| {
        let q = p.project()?;
        Some(Vec2::new(q.x, q.y))
    };
    if control.len() == 2 {
        return Some(solidcraft_geom::bezier(&[point(*control.first()?)?, point(*control.last()?)?])?.area_term());
    }
    if let [a, apex, b] = control {
        let (wa, wb, wc) = (a.w, apex.w, b.w);
        let w = wb / (wa * wc).sqrt();
        if !(w.is_finite() && w > 0.0) {
            return None;
        }
        return Some(Seg2::Conic { a: point(*a)?, apex: point(*apex)?, b: point(*b)?, w }.area_term());
    }
    if control.iter().all(|p| p.w == 1.0) {
        let points = control.iter().copied().map(point).collect::<Option<Vec<_>>>()?;
        return Some(solidcraft_geom::bezier(&points)?.area_term());
    }
    rational_area(control)
}

/// An integral over a general rational Bézier. Homogeneous de Casteljau evaluates
/// (X,Y,W), while the quotient derivative gives ½(x y' - y x') = (X Y' - X' Y)/(2W²).
/// Bounded Gauss–Legendre quadrature evaluates that exact curve, not a fitted polygon.
fn rational_area(control: &[solidcraft_geom::bspline::HPoint]) -> Option<f64> {
    use solidcraft_geom::bspline::HPoint;
    if control.len() < 3 || control.iter().any(|p| !p.is_finite() || p.w <= 0.0) {
        return None;
    }
    let difference: Vec<HPoint> = control.windows(2).map(|w| (w[1] - w[0]) * (control.len() - 1) as f64).collect();
    let at = |control: &[HPoint], t: f64| -> Option<HPoint> {
        if !(0.0..=1.0).contains(&t) {
            return None;
        }
        let mut work = control.to_vec();
        while work.len() > 1 {
            work = work.windows(2).map(|w| w[0] * (1.0 - t) + w[1] * t).collect();
        }
        let p = *work.first()?;
        // W'(t) on the derivative net may be zero or negative on a valid curve; only
        // the curve's own W(t) must stay positive.
        p.is_finite().then_some(p)
    };
    const GL5: [(f64, f64); 5] = [
        (0.046_910_077_030_668, 0.118_463_442_528_095),
        (0.230_765_344_947_158, 0.239_314_335_249_683),
        (0.5, 0.284_444_444_444_444),
        (0.769_234_655_052_842, 0.239_314_335_249_683),
        (0.953_089_922_969_332, 0.118_463_442_528_095),
    ];
    let mut area = 0.0;
    for i in 0..16 {
        for (x, weight) in GL5 {
            let t = (i as f64 + x) / 16.0;
            let q = at(control, t).filter(|q| q.w > 0.0)?;
            let d = at(&difference, t)?;
            let part = weight * (q.x * d.y - q.y * d.x) / (2.0 * q.w * q.w * 16.0);
            if !part.is_finite() {
                return None;
            }
            area += part;
        }
    }
    area.is_finite().then_some(area)
}

/// Exact B-rep pcurve crossing containment. This follows the kernel's isoparametric face clipping:
/// samples only bracket crossings; bisection then solves each one on the pcurve itself.
fn loop_contains(edges: &[solidcore_record::brep::OrientedEdge], u: f64, v: f64) -> bool {
    let mut cuts = Vec::new();
    for used in edges {
        let Some([t0, t1]) = used.pcurve.interval() else { return false };
        let samples = pcurve_levels(&used.pcurve, t0, t1);
        for pair in samples.windows(2) {
            let (Some(a), Some(b)) = (pair.first().copied(), pair.last().copied()) else { return false };
            let Some((qa, qb)) = solidcore_ops::pcurve::eval(&used.pcurve, a).zip(solidcore_ops::pcurve::eval(&used.pcurve, b)) else {
                return false;
            };
            let above_a = qa[1] > v;
            if above_a == (qb[1] > v) {
                continue;
            }
            let (mut lo, mut hi) = (a, b);
            for _ in 0..80 {
                let mid = lo.midpoint(hi);
                let Some(q) = solidcore_ops::pcurve::eval(&used.pcurve, mid) else { return false };
                if (q[1] > v) == above_a {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let Some(q) = solidcore_ops::pcurve::eval(&used.pcurve, lo.midpoint(hi)).filter(|q| q[0].is_finite()) else { return false };
            cuts.push(q[0]);
        }
    }
    let mut inside = false;
    cuts.sort_by(f64::total_cmp);
    let mut previous = None;
    for cut in cuts {
        if previous.is_none_or(|last| cut > last) && cut > u {
            inside = !inside;
        }
        previous = Some(cut);
    }
    inside
}

/// At least 64 levels of each exact record interval, with internal spline knots included. Limited
/// by solidcore-record's bounded knot and control-point counts.
fn pcurve_levels(curve: &solidcore_record::brep::Curve2, lo: f64, hi: f64) -> Vec<f64> {
    const LEVELS: usize = 64;
    let knots = match curve {
        solidcore_record::brep::Curve2::Spline { knots, .. } | solidcore_record::brep::Curve2::RationalSpline { knots, .. } => knots.as_slice(),
    };
    let mut out: Vec<_> = (0..=LEVELS).map(|i| lo + (hi - lo) * i as f64 / LEVELS as f64).collect();
    out.extend(knots.iter().copied().filter(|t| *t > lo && *t < hi));
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() <= (lo.abs() + hi.abs() + 1.0) * f64::EPSILON * 8.0);
    out
}

/// Nearest face: (B-rep face index, closest point, distance).
pub(crate) fn nearest_face(m: &Mesh, p: Vec3) -> Option<(u32, Vec3, f64)> {
    let mut best: Option<(u32, Vec3, f64)> = None;
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let Some([a, b, c]) = m.tri(t) else { continue };
        let q = closest_on_triangle(p, a, b, c);
        let d = q.dist(p);
        if best.is_none_or(|(_, _, bd)| d < bd) {
            best = Some((*f, q, d));
        }
    }
    best
}

/// A full circle split by a seam comes out as two arcs between the same two points, but some
/// meshes give the same half twice. That is a circle too: turn the duplicate into the other half
/// so re-reading an unchanged face always gives the same two complementary arcs.
fn merge_split_circles(geom: Vec<LinkGeom>) -> Vec<LinkGeom> {
    const TOL: f64 = 1e-6;
    let near = |p: Vec2, q: Vec2| p.dist(q) <= TOL * (1.0 + p.len().max(q.len()));
    let mut out = geom.clone();
    for (i, first) in geom.iter().enumerate() {
        let LinkGeom::Arc { c, a, b } = *first else { continue };
        for (second, slot) in geom.iter().zip(out.iter_mut()).skip(i + 1) {
            if let LinkGeom::Arc { c: c2, a: a2, b: b2 } = *second
                && near(c, c2)
                && near(a, a2)
                && near(b, b2)
            {
                *slot = LinkGeom::Arc { c, a: b, b: a };
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Projection

fn project_sketch_curve(sk: &Sketch, from: &Plane, ci: usize, to: &Plane) -> Vec<LinkGeom> {
    let map = |p: Vec2| to.to_local(from.to_world(p));
    let parallel = from.normal().cross(to.normal()).len() < 1e-9;
    let flip = from.normal().dot(to.normal()) < 0.0;
    match sk.curves.get(ci).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => match (sk.point(*a), sk.point(*b)) {
            (Some(a), Some(b)) => {
                let (a, b) = (map(a), map(b));
                if a.dist(b) > 1e-9 { vec![LinkGeom::Line(a, b)] } else { vec![LinkGeom::Point(a)] }
            }
            _ => Vec::new(),
        },
        Some(CurveKind::Circle { c, r }) if parallel => sk.point(*c).map(|c| vec![LinkGeom::Circle(map(c), *r)]).unwrap_or_default(),
        Some(CurveKind::Arc { c, a, b }) if parallel => match (sk.point(*c), sk.point(*a), sk.point(*b)) {
            (Some(c), Some(a), Some(b)) => {
                let (c, a, b) = (map(c), map(a), map(b));
                vec![if flip { LinkGeom::Arc { c, a: b, b: a } } else { LinkGeom::Arc { c, a, b } }]
            }
            _ => Vec::new(),
        },
        // Free-form curves in a parallel plane map exactly through their defining points.
        Some(CurveKind::Ellipse { c, m, r }) if parallel => match (sk.point(*c), sk.point(*m)) {
            (Some(c), Some(m)) => vec![LinkGeom::Ellipse { c: map(c), major: map(m), minor: *r }],
            _ => Vec::new(),
        },
        Some(CurveKind::Spline { pts, control: false, periodic, nurbs: None, handles, .. }) if parallel && !handles.is_empty() => {
            let p: Option<Vec<Vec2>> = pts.iter().map(|q| sk.point(*q).map(map)).collect();
            let tangent_handles: Option<Vec<Vec2>> = handles.iter().map(|q| sk.point(*q).map(map)).collect();
            let Some((p, tangent_handles)) = p.zip(tangent_handles) else { return Vec::new() };
            // One curve per span whether or not a handle has been moved, so turning a handle keeps
            // the projected layout and what is constrained to it (#6502).
            let Ok(pieces) = solidcraft_sketch::fit_beziers_with_handles(&p, &tangent_handles, *periodic) else { return Vec::new() };
            pieces.into_iter().map(|piece| LinkGeom::Spline { pts: piece.to_vec(), control: true, degree: 3, periodic: false, nurbs: None }).collect()
        }
        Some(CurveKind::Spline { pts, control, degree, periodic, nurbs, handles, .. }) if parallel && handles.is_empty() => {
            let p: Option<Vec<Vec2>> = pts.iter().map(|q| sk.point(*q).map(map)).collect();
            p.map(|pts| vec![LinkGeom::Spline { pts, control: *control, degree: *degree, periodic: *periodic, nurbs: nurbs.clone() }])
                .unwrap_or_default()
        }
        Some(CurveKind::Conic { a, b, apex, rho }) if parallel => match (sk.point(*a), sk.point(*apex), sk.point(*b)) {
            (Some(a), Some(x), Some(b)) => vec![LinkGeom::Conic { a: map(a), apex: map(x), b: map(b), rho: *rho }],
            _ => Vec::new(),
        },
        Some(_) => {
            let pts: Vec<Vec3> = sk.polyline(ci).iter().map(|p| from.to_world(*p)).collect();
            fit_world(to, &pts)
        }
        None => Vec::new(),
    }
}

/// A circle in space (centre, unit normal, radius) through the polyline's points, and whether
/// the polyline closes. Its points lie on it within `within`, by default 1e-7 of the radius.
fn circle3(pts: &[Vec3], within: Option<f64>) -> Option<(Vec3, Vec3, f64, bool)> {
    let n = pts.len();
    if n < 4 {
        return None;
    }
    let closed = pts.first()?.dist(*pts.last()?) <= 1e-9 * pts.first()?.len().max(1.0);
    let (a, b, c) = if closed { (*pts.first()?, *pts.get(n / 3)?, *pts.get(2 * n / 3)?) } else { (*pts.first()?, *pts.get(n / 2)?, *pts.last()?) };
    let (ab, ac) = (b - a, c - a);
    let nrm = ab.cross(ac);
    let l2 = nrm.len2();
    if l2 < 1e-24 {
        return None;
    }
    // Circumcentre of the triangle a, b, c.
    let centre = a + (nrm.cross(ab) * ac.len2() + ac.cross(nrm) * ab.len2()) / (2.0 * l2);
    let r = centre.dist(a);
    let un = nrm.normalized()?;
    let tol = within.unwrap_or(1e-7 * r.max(1.0));
    pts.iter().all(|p| (p.dist(centre) - r).abs() < tol && (*p - centre).dot(un).abs() < tol).then_some((centre, un, r, closed))
}

/// A circle seen at a slant is an ellipse, exactly: a full circle becomes an ellipse; an arc
/// becomes conics (rational quadratic Béziers of at most 90 degrees each — parallel projection
/// keeps their weights).
fn tilted_circle(plane: &Plane, pts: &[Vec3], within: Option<f64>) -> Option<Vec<LinkGeom>> {
    let (c, n, r, closed) = circle3(pts, within)?;
    let sn = plane.normal();
    let cosang = n.dot(sn).abs();
    if !(1e-9..=1.0 - 1e-12).contains(&cosang) {
        return None;
    }
    if closed {
        let u = n.cross(sn).normalized()?;
        return Some(vec![LinkGeom::Ellipse { c: plane.to_local(c), major: plane.to_local(c + u * r), minor: r * cosang }]);
    }
    // Sweep from the first point along the polyline's direction.
    let p0 = *pts.first()?;
    let e1 = (p0 - c).normalized()?;
    let e2 = n.cross(e1);
    let ang = |p: Vec3| (p - c).dot(e2).atan2((p - c).dot(e1));
    let mut sweep = 0.0;
    for w in pts.windows(2) {
        let mut d = ang(w[1]) - ang(w[0]);
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        sweep += d;
    }
    let k = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).clamp(1, 8);
    let h = sweep / k as f64;
    let at = |t: f64| c + (e1 * t.cos() + e2 * t.sin()) * r;
    let w = (h.abs() / 2.0).cos();
    let rho = w / (1.0 + w);
    let mut out = Vec::new();
    for i in 0..k {
        let (t0, t1) = (h * i as f64, h * (i + 1) as f64);
        let apex = c + (e1 * ((t0 + t1) / 2.0).cos() + e2 * ((t0 + t1) / 2.0).sin()) * (r / w);
        out.push(LinkGeom::Conic { a: plane.to_local(at(t0)), apex: plane.to_local(apex), b: plane.to_local(at(t1)), rho });
    }
    Some(out)
}

/// A smooth chain that fitting broke into many short lines is better as one fit spline (a
/// projected spline edge or elliptical arc): when a spline through a few of its points stays
/// within `tol` of all of them.
fn as_spline(pts: &[Vec2], tol: f64) -> Option<LinkGeom> {
    let n = pts.len();
    if n < 4 {
        return None;
    }
    // No sharp corners.
    for w in pts.windows(3) {
        let (Some(u), Some(v)) = ((w[1] - w[0]).normalized(), (w[2] - w[1]).normalized()) else { continue };
        if u.dot(v) < 30f64.to_radians().cos() {
            return None;
        }
    }
    for k in [8usize, 16, 32, 64] {
        let step = (n - 1) as f64 / k as f64;
        let mut fit: Vec<Vec2> = (0..=k).map(|i| pts[((i as f64 * step).round() as usize).min(n - 1)]).collect();
        fit.dedup_by(|a, b| a.dist(*b) < 1e-12);
        if fit.len() < 2 {
            return None;
        }
        let poly = solidcraft_sketch::spline_polyline(&fit, false, 3);
        let ok = pts.iter().all(|q| {
            poly.windows(2)
                .map(|w| {
                    let (a, b) = (w[0], w[1]);
                    let d = b - a;
                    let t = if d.len2() > 0.0 { ((*q - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
                    q.dist(a + d * t)
                })
                .fold(f64::INFINITY, f64::min)
                <= tol
        });
        if ok {
            return Some(LinkGeom::Spline { pts: fit, control: false, degree: 3, periodic: false, nurbs: None });
        }
    }
    None
}

/// The line where another plane crosses the sketch plane (None when parallel).
fn plane_trace(sk: &Plane, other: &Plane) -> Option<LinkGeom> {
    let m = other.normal();
    let (a, b) = (sk.x.dot(m), sk.y.dot(m));
    let n2 = a * a + b * b;
    if n2 < 1e-12 {
        return None;
    }
    let c = (other.origin - sk.origin).dot(m);
    let p0 = Vec2::new(a, b) * (c / n2);
    let u = Vec2::new(-b, a).normalized()?;
    Some(LinkGeom::Line(p0 - u * AXIS_HALF, p0 + u * AXIS_HALF))
}

/// Most corners a mesh face's outline has and is still taken for a polygon.
const MESH_POLYGON_CORNERS: usize = 12;

/// The outline of a mesh body's flat patch (a closed loop of its corners) as lines and arcs. Any
/// triangle's corners and any rectangle's lie on one circle, so only a loop of more corners than
/// [`MESH_POLYGON_CORNERS`], all on one circle, is a tessellated circle, an ellipse on a slanted
/// plane; and the sides of a polygon stay lines, never one fitted spline (#13464). A mesh file
/// keeps its corners in single precision, so they lie on the circle within about 1e-7 of their
/// distance from the origin, not of the circle's size; and never more loosely than 1e-4 of the
/// outline's own size, so a small outline far from the origin is not taken for a circle it misses.
fn fit_mesh_outline(plane: &Plane, pts: &[Vec3]) -> Vec<LinkGeom> {
    let (lo, hi) = pts.iter().fold(
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let size = lo.dist(hi);
    let within = (pts.iter().map(|p| p.len()).fold(1.0, f64::max) * 1e-6).min(size * 1e-4);
    if pts.len() > MESH_POLYGON_CORNERS + 1
        && let Some(e) = tilted_circle(plane, pts, Some(within))
    {
        return e;
    }
    let p2: Vec<Vec2> = pts.iter().map(|p| plane.to_local(*p)).collect();
    let mut out = Vec::new();
    fit_chain(&p2, None, &mut out);
    out
}

/// Project a world polyline and fit lines and arcs to it.
fn fit_world(plane: &Plane, pts: &[Vec3]) -> Vec<LinkGeom> {
    if let Some(e) = tilted_circle(plane, pts, None) {
        return e;
    }
    let p2: Vec<Vec2> = pts.iter().map(|p| plane.to_local(*p)).collect();
    let mut out = Vec::new();
    fit_chain(&p2, None, &mut out);
    // Many little lines from one smooth edge: a spline instead.
    if out.len() > 4 && out.iter().all(|g| matches!(g, LinkGeom::Line(..))) {
        let ext = p2.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x).min(p.y), hi.max(p.x).max(p.y)));
        if let Some(sp) = as_spline(&p2, (ext.1 - ext.0).abs().max(1e-9) * 1e-4) {
            return vec![sp];
        }
    }
    out
}

/// Where a world polyline crosses the plane.
fn section_of_polyline(plane: &Plane, pts: &[Vec3]) -> Vec<LinkGeom> {
    let mut out: Vec<LinkGeom> = Vec::new();
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let (ha, hb) = (plane.height(*a), plane.height(*b));
        if (ha > 0.0) != (hb > 0.0) || ha == 0.0 {
            let t = if (ha - hb).abs() < 1e-15 { 0.0 } else { ha / (ha - hb) };
            let p = plane.to_local(a.lerp(*b, t));
            if !out.iter().any(|g| matches!(g, LinkGeom::Point(q) if q.dist(p) < 1e-7)) {
                out.push(LinkGeom::Point(p));
            }
        }
    }
    out
}

/// Which way the surface faces at triangle `t`: its normals at the corners, added. A flat triangle
/// close to the outline can tip the other way from the surface it approximates (one whose far
/// corner lies inside the chord of its near edge), which would add spurs to the outline. The
/// triangle's own normal `flat` when a corner has none, or when they aren't that surface's: unit,
/// and close to `flat` (normals averaged across a crease, as an imported mesh may have, are not).
fn surface_facing(m: &Mesh, t: &[u32; 3], flat: Vec3) -> Vec3 {
    /// Least cosine between a corner's normal and the triangle's: about 26 degrees.
    const AGREES: f64 = 0.9;
    let mut sum = Vec3::ZERO;
    for i in t {
        match m.normals.get(*i as usize).and_then(|v| v.normalized()) {
            Some(v) if v.dot(flat) >= AGREES => sum += v,
            _ => return flat,
        }
    }
    sum.normalized().unwrap_or(flat)
}

/// The outline of a body seen along the sketch normal: mesh edges between triangles facing the
/// sketch and triangles facing away (or edge-on), projected and fitted. A face of `body` that
/// faces one way all over ([`face_facing`]) does so in every triangle, though they lean a hair
/// either way: a round wall seen edge-on joins its two rims at different angles, and thin
/// slivers of a narrow chamfer can point the other way. Any other triangle faces as the surface
/// does at its corners ([`surface_facing`]).
fn silhouette(plane: &Plane, m: &Mesh, body: Option<&solidcraft_kernel::Body>) -> Vec<LinkGeom> {
    let n = plane.normal();
    let size = m.bounds().diagonal().max(1e-9);
    // Vertices welded within this: the copies of a vertex on the seam between two faces can lie
    // a few 1e-7 apart, and an edge used by one triangle only is outline.
    let weld = size * 1e-6;
    let cell = |p: Vec3| ((p.x / weld).floor() as i64, (p.y / weld).floor() as i64, (p.z / weld).floor() as i64);
    let mut grid: BTreeMap<(i64, i64, i64), Vec<usize>> = BTreeMap::new();
    let mut verts: Vec<Vec3> = Vec::new();
    let mut vertex = |p: Vec3| -> usize {
        let (x, y, z) = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let found = grid
                        .get(&(x + dx, y + dy, z + dz))
                        .and_then(|ids| ids.iter().copied().find(|i| verts.get(*i).is_some_and(|v| v.dist(p) <= weld)));
                    if let Some(i) = found {
                        return i;
                    }
                }
            }
        }
        verts.push(p);
        grid.entry((x, y, z)).or_default().push(verts.len() - 1);
        verts.len() - 1
    };
    let tris: Vec<([Vec3; 3], Vec3, Option<u32>, &[u32; 3])> = m
        .triangles
        .iter()
        .enumerate()
        .filter_map(|(ti, t)| {
            let [a, b, c] = m.tri(t)?;
            Some(([a, b, c], (b - a).cross(c - a), m.tri_face.get(ti).copied(), t))
        })
        .collect();
    // Each face's area-weighted facing, and whether any triangle of it is in doubt: near
    // edge-on, or facing against the rest of its face. Only those faces ask what they are.
    let mut faces: BTreeMap<u32, (f64, bool)> = BTreeMap::new();
    for (_, w, f, _) in &tris {
        if let Some(f) = f {
            faces.entry(*f).or_default().0 += w.dot(n);
        }
    }
    for (_, w, f, _) in &tris {
        let (Some(f), Some(un)) = (f, w.normalized()) else { continue };
        if let Some(face) = faces.get_mut(f) {
            face.1 |= un.dot(n).abs() < 0.5 || (un.dot(n) > 0.0) != (face.0 > 0.0);
        }
    }
    let facing: BTreeMap<u32, Facing> = match body {
        Some(body) => faces.iter().filter(|(_, (_, doubt))| *doubt).filter_map(|(f, (sum, _))| Some((*f, face_facing(body, *f, n, *sum)?))).collect(),
        None => BTreeMap::new(),
    };
    // Undirected mesh edge → facing flags of the triangles using it.
    let ids: Vec<[usize; 3]> = tris.iter().map(|([a, b, c], _, _, _)| [vertex(*a), vertex(*b), vertex(*c)]).collect();
    let mut edges: BTreeMap<(usize, usize), (Vec3, Vec3, Vec<(bool, Option<u32>)>)> = BTreeMap::new();
    for ((_, w, f, t), [ia, ib, ic]) in tris.iter().zip(ids) {
        let Some(un) = w.normalized() else { continue };
        let front = match f.and_then(|f| facing.get(&f)) {
            Some(Facing::Front) => true,
            Some(Facing::Away) => false,
            None => surface_facing(m, t, un).dot(n) > 1e-6,
        };
        for (p, r) in [(ia, ib), (ib, ic), (ic, ia)] {
            if p == r {
                continue;
            }
            let (Some(vp), Some(vr)) = (verts.get(p).copied(), verts.get(r).copied()) else { continue };
            edges.entry((p.min(r), p.max(r))).or_insert((vp, vr, Vec::new())).2.push((front, *f));
        }
    }
    let mut segs: Vec<(Vec2, Vec2)> = edges
        .values()
        .filter(|(_, _, f)| {
            let fronts = f.iter().filter(|x| x.0).count();
            fronts > 0 && fronts < f.len()
        })
        .map(|(a, b, _)| (plane.to_local(*a), plane.to_local(*b)))
        .collect();
    let singles: Vec<(Vec3, Vec3, bool, Option<u32>)> = edges
        .values()
        .filter_map(|(a, b, f)| match f.as_slice() {
            [(front, face)] => Some((*a, *b, *front, *face)),
            _ => None,
        })
        .collect();
    segs.extend(open_edges(&singles, size).into_iter().map(|(a, b)| (plane.to_local(a), plane.to_local(b))));
    fit_segments(&segs, size, true)
}

/// Of the mesh edges used by one triangle each (`a`, `b`, whether the triangle faces the sketch,
/// its face), those facing the sketch that are outline: an open sheet's border. Where two faces
/// meet, one face's triangles can split the seam at a vertex the other's run straight past, and
/// both sides are then used once; a seam whose other side faces the sketch too is no outline.
fn open_edges(singles: &[(Vec3, Vec3, bool, Option<u32>)], size: f64) -> Vec<(Vec3, Vec3)> {
    // The two sides of a seam stray from each other by at most a chord's sag.
    let seam = size * 2e-3;
    let cell = seam * 8.0;
    let at = |p: Vec3| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64);
    let mut grid: BTreeMap<(i64, i64, i64), Vec<usize>> = BTreeMap::new();
    let mut long: Vec<usize> = Vec::new();
    for (i, (a, b, front, _)) in singles.iter().enumerate() {
        if !front {
            continue;
        }
        let (lo, hi) = (at(a.min(*b) - Vec3::new(seam, seam, seam)), at(a.max(*b) + Vec3::new(seam, seam, seam)));
        let cells = (hi.0 - lo.0 + 1).saturating_mul(hi.1 - lo.1 + 1).saturating_mul(hi.2 - lo.2 + 1);
        if cells > 512 {
            long.push(i);
            continue;
        }
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                for z in lo.2..=hi.2 {
                    grid.entry((x, y, z)).or_default().push(i);
                }
            }
        }
    }
    singles
        .iter()
        .enumerate()
        .filter(|(i, (a, b, front, face))| {
            let m = (*a + *b) * 0.5;
            *front
                && !grid
                    .get(&at(m))
                    .into_iter()
                    .flatten()
                    .chain(&long)
                    .any(|j| singles.get(*j).is_some_and(|(p, q, _, other)| j != i && other != face && m.dist_to_segment(*p, *q) <= seam))
        })
        .map(|(_, (a, b, _, _))| (*a, *b))
        .collect()
}

/// Which way a whole face of a body faces, seen along a sketch normal.
#[derive(Clone, Copy)]
enum Facing {
    Front,
    /// Away from the sketch or edge-on.
    Away,
}

/// Which way face `face` of `body` faces seen along `n`, when it faces one way all over: a
/// plane (edge-on when it contains `n`), a cylinder about `n` (edge-on), a cone about `n`.
/// `sum` is the face's triangles' normals weighted by their areas, dotted with `n`: the side a
/// plane or such a cone faces. `None` for any other face: each triangle says.
fn face_facing(body: &solidcraft_kernel::Body, face: u32, n: Vec3, sum: f64) -> Option<Facing> {
    use solidcraft_kernel::FaceSurface;
    let side = if sum > 0.0 { Facing::Front } else { Facing::Away };
    match solidcraft_kernel::face_surface(body, face as usize)? {
        FaceSurface::Plane { normal, .. } if normal.vec().dot(n).abs() < 1e-9 => Some(Facing::Away),
        FaceSurface::Plane { .. } => Some(side),
        FaceSurface::Cylinder { axis, .. } if axis.vec().dot(n).abs() > 1.0 - 1e-12 => Some(Facing::Away),
        FaceSurface::Cone { axis, .. } if axis.vec().dot(n).abs() > 1.0 - 1e-12 => Some(side),
        _ => None,
    }
}

/// Section of a mesh (or one of its faces) with the sketch plane.
pub(crate) fn section(plane: &Plane, m: &Mesh, face: Option<u32>) -> Vec<LinkGeom> {
    let size = m.bounds().diagonal().max(1e-9);
    let (segs, exact) = section_segments(plane, m, face);
    let segs: Vec<(Vec2, Vec2)> = segs.into_iter().map(|(a, b, _)| (a, b)).collect();
    let refine = |v: Vec<LinkGeom>| -> Vec<LinkGeom> { refine_rounds(v, &exact, size) };
    let lossy = refine(fit_segments(&segs, size, false));
    // Lines and arcs that follow the section closely (the corners of a rounded profile stay
    // where they are) are kept when they take no more curves than the loose fit.
    let close = refine(fit_segments(&segs, size, true));
    let plain = |v: &[LinkGeom]| v.iter().all(|g| matches!(g, LinkGeom::Line(..) | LinkGeom::Arc { .. } | LinkGeom::Circle(..)));
    // A loose fit with fewer curves can still be wrong: short faces beside a small round (2 mm
    // walls each side of an R2 corner) fit within its tolerance as one larger arc. The exact
    // crossings (on planes and rulings) show it: one of them off every curve of the fit.
    // Any other curve counts as near, and past a work limit nothing is checked: the loose fit
    // stands, as before.
    let tight = size * 1e-4;
    let checkable = exact.len().saturating_mul(close.len()) <= 4_000_000;
    let strays = |v: &[LinkGeom]| exact.iter().any(|p| !v.iter().any(|g| plain_distance(g, *p).is_none_or(|d| d <= tight)));
    let fitted = if plain(&close) && (close.len() <= lossy.len() || (checkable && strays(&lossy) && !strays(&close))) { close } else { lossy };
    join_rounds(fitted, size * 1e-5)
}

/// Where the fit left a line and an arc ends apart by at most `tol`, both end at one point. A
/// line can swallow an arc's first chord (a mesh crossing just past the tangent line), and
/// [`refine_round`] moves an arc's ends but not the lines': the outline would not close.
fn join_rounds(mut fitted: Vec<LinkGeom>, tol: f64) -> Vec<LinkGeom> {
    for i in 0..fitted.len() {
        let Some(&LinkGeom::Arc { c, mut a, mut b }) = fitted.get(i) else { continue };
        let r = c.dist(a);
        for end in [&mut a, &mut b] {
            for curve in &mut fitted {
                let LinkGeom::Line(p, q) = curve else { continue };
                let (near, far) = if p.dist(*end) <= q.dist(*end) { (p, q) } else { (q, p) };
                if near.dist(*end) <= tol {
                    let joint = line_meets_round(*far, *near, c, r, tol);
                    *near = joint;
                    *end = joint;
                }
            }
        }
        if let Some(arc) = fitted.get_mut(i) {
            *arc = LinkGeom::Arc { c, a, b };
        }
    }
    fitted
}

/// Where a line from `far` ending near `near` meets the circle (`c`, `r`): the tangent point when
/// the line runs into the circle there, within `tol` of it over the arc's first chord, else its
/// crossing nearest `near`, else the circle's point nearest `near`.
fn line_meets_round(far: Vec2, near: Vec2, c: Vec2, r: f64, tol: f64) -> Vec2 {
    let nearest = |[x, y]: [Vec2; 2]| if x.dist(near) <= y.dist(near) { x } else { y };
    let to_centre = c - far;
    let span = to_centre.len();
    if let Some(unit) = to_centre.normalized().filter(|_| span > r) {
        let reach = (span * span - r * r).sqrt();
        let (along, side) = (unit * (reach * reach / span), unit.perp() * (reach * r / span));
        let tangent = nearest([far + along + side, far + along - side]);
        if tangent.dist(near) <= tol + (2.0 * r * tol).sqrt() {
            return tangent;
        }
    }
    let Some(dir) = (near - far).normalized() else { return near };
    let foot = far + dir * (c - far).dot(dir);
    let half = r * r - (foot - c).len2();
    if half >= 0.0 {
        return nearest([foot + dir * half.sqrt(), foot - dir * half.sqrt()]);
    }
    (near - c).normalized().map_or(near, |unit| c + unit * r)
}

/// How far `p` lies from a line, arc or circle; None for any other curve.
fn plain_distance(g: &LinkGeom, p: Vec2) -> Option<f64> {
    match *g {
        LinkGeom::Line(a, b) => {
            let d = b - a;
            let t = if d.len2() > 0.0 { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
            Some(p.dist(a.lerp(b, t)))
        }
        LinkGeom::Circle(c, r) => Some((p.dist(c) - r).abs()),
        LinkGeom::Arc { c, a, b } => {
            let (s0, sw) = ((a - c).angle(), ((b - c).angle() - (a - c).angle()).rem_euclid(std::f64::consts::TAU));
            if ((p - c).angle() - s0).rem_euclid(std::f64::consts::TAU) <= sw + 1e-9 {
                Some((p.dist(c) - c.dist(a)).abs())
            } else {
                Some(p.dist(a).min(p.dist(b)))
            }
        }
        _ => None,
    }
}

/// The segments where the plane crosses the mesh's triangles (in plane coordinates, each with
/// its B-rep face), and the crossings that lie exactly on the surface (on a ruling).
fn section_segments(plane: &Plane, m: &Mesh, face: Option<u32>) -> (Vec<(Vec2, Vec2, u32)>, Vec<Vec2>) {
    let size = m.bounds().diagonal().max(1e-9);
    let q = size * 1e-7;
    let key = |p: Vec3| ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
    // Surface normal at each mesh vertex where the surface is smooth (None at creases), over
    // all faces and per face.
    type Key = (i64, i64, i64);
    // A vertex this close to the plane is on it, and lies on the same side for every edge it
    // ends (so a plane through a vertex, or on a face, cuts every edge there alike): below when
    // the mesh lies on or above the plane (the cut at its bottom face), else above (at its top
    // face, through an apex). An edge with an end on the plane crosses it at that end.
    let snap = size * 1e-9;
    let zero_above = m.positions.iter().any(|p| plane.height(*p) < -snap);
    let above = |h: f64| if h.abs() <= snap { zero_above } else { h > 0.0 };
    let mut normals: BTreeMap<Key, Option<Vec3>> = BTreeMap::new();
    let mut face_normals: BTreeMap<(u32, Key), Option<Vec3>> = BTreeMap::new();
    // Faces on either side of each mesh edge.
    let mut edge_faces: BTreeMap<(Key, Key), Vec<u32>> = BTreeMap::new();
    let merge = |e: &mut Option<Vec3>, n: Vec3| {
        if e.is_some_and(|o| o.dot(n) < 1.0 - 1e-6) {
            *e = None;
        }
    };
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        for k in t {
            let (Some(p), Some(n)) = (m.positions.get(*k as usize), m.normals.get(*k as usize)) else { continue };
            let Some(n) = n.normalized() else { continue };
            normals.entry(key(*p)).and_modify(|e| merge(e, n)).or_insert(Some(n));
            face_normals.entry((*f, key(*p))).and_modify(|e| merge(e, n)).or_insert(Some(n));
        }
        for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let (Some(a), Some(b)) = (t.get(i).and_then(|k| m.positions.get(*k as usize)), t.get(j).and_then(|k| m.positions.get(*k as usize)))
            else {
                continue;
            };
            let (ka, kb) = (key(*a), key(*b));
            edge_faces.entry(if ka <= kb { (ka, kb) } else { (kb, ka) }).or_default().push(*f);
        }
    }
    // Where the plane crosses a mesh edge: on a cubic through the end points whose end
    // tangents lie in the surface's tangent planes (so sections of curved faces land on the
    // surface, not on the chord), computed once per edge so neighbours agree.
    let exact: std::cell::RefCell<Vec<Vec2>> = std::cell::RefCell::new(Vec::new());
    let crossing = |a: Vec3, b: Vec3| -> Option<Vec3> {
        let (a, b) = if key(a) <= key(b) { (a, b) } else { (b, a) };
        let (ha, hb) = (plane.height(a), plane.height(b));
        if above(ha) == above(hb) {
            return None;
        }
        if let Some(p) = [(a, ha), (b, hb)].into_iter().find(|(_, h)| h.abs() <= snap).map(|(p, _)| p) {
            exact.borrow_mut().push(plane.to_local(p));
            return Some(p);
        }
        let d = b - a;
        // An edge inside one face takes that face's normals (its ends may lie on the face's
        // boundary, a crease for the whole mesh); an edge between faces, the merged ones, so
        // both faces agree on the crossing.
        let faces = edge_faces.get(&(key(a), key(b)));
        let inside = faces.and_then(|fs| fs.first().filter(|f| fs.len() == 2 && fs.iter().all(|x| x == *f)).copied());
        // An edge where a curved face meets another (a cylinder's rim on its cap) lies on the
        // curved face: its normals there put the crossing on the curve, not on the chord.
        let face_normal = |f: u32, p: Vec3| face_normals.get(&(f, key(p))).copied().flatten();
        let curved =
            || faces?.iter().copied().find(|f| matches!((face_normal(*f, a), face_normal(*f, b)), (Some(x), Some(y)) if x.dot(y) < 1.0 - 1e-12));
        let inside = inside.or_else(curved);
        let normal = |p: Vec3| match inside {
            Some(f) => face_normals.get(&(f, key(p))).copied().flatten(),
            None => normals.get(&key(p)).copied().flatten(),
        };
        // Tangent length: the arc length of a circle through both ends with these normals.
        let (na, nb) = (normal(a), normal(b));
        let l = match (na, nb) {
            (Some(x), Some(y)) => {
                let half = 0.5 * x.dot(y).clamp(-1.0, 1.0).acos();
                if half > 1e-9 { d.len() * half / half.sin() } else { d.len() }
            }
            _ => d.len(),
        };
        let tangent = |n: Option<Vec3>| -> Vec3 {
            match n {
                Some(n) => (d - n * d.dot(n)).normalized().map(|t| t * l).unwrap_or(d),
                None => d,
            }
        };
        // Along a ruling (same normal at both ends: planes, cylinder and cone generators) the
        // edge lies on the surface and the straight crossing is exact.
        if let (Some(x), Some(y)) = (na, nb)
            && x.dot(y) > 1.0 - 1e-12
        {
            let p = a.lerp(b, ha / (ha - hb));
            exact.borrow_mut().push(plane.to_local(p));
            return Some(p);
        }
        let (ta, tb) = (tangent(na), tangent(nb));
        let h = |t: f64| {
            let (t2, t3) = (t * t, t * t * t);
            a * (2.0 * t3 - 3.0 * t2 + 1.0) + ta * (t3 - 2.0 * t2 + t) + b * (-2.0 * t3 + 3.0 * t2) + tb * (t3 - t2)
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if (plane.height(h(mid)) > 0.0) == (ha > 0.0) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(h(0.5 * (lo + hi)))
    };
    let mut segs: Vec<(Vec2, Vec2, u32)> = Vec::new();
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if face.is_some_and(|x| x != *f) {
            continue;
        }
        let Some(tri) = m.tri(t) else { continue };
        let mut pts: Vec<Vec3> = Vec::new();
        for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let (Some(&pi), Some(&pj)) = (tri.get(i), tri.get(j)) else { continue };
            if let Some(x) = crossing(pi, pj) {
                pts.push(x);
            }
        }
        if let [a, b] = pts[..] {
            let (a, b) = (plane.to_local(a), plane.to_local(b));
            if a.dist(b) > size * 1e-9 {
                segs.push((a, b, *f));
            }
        }
    }
    (segs, exact.into_inner())
}

/// Re-fit each circle and arc of a fitted section through the exact section points near it.
/// The outline stays closed: a line meeting an arc's end moves with it. Where two arcs meet,
/// their refits stand only while their ends stay within the distance a profile joins ends
/// across; else both keep their fits, since moving an arc's end would take it off its circle.
fn refine_rounds(mut v: Vec<LinkGeom>, exact: &[Vec2], size: f64) -> Vec<LinkGeom> {
    // Ends this close meet: the fit joins crossings this close into one point.
    let meet = size * 1e-7;
    let ends = |h: &LinkGeom| match *h {
        LinkGeom::Arc { a, b, .. } => Some([a, b]),
        _ => None,
    };
    let fitted = v.clone();
    // Each arc's refit, and whether it stands.
    let refits: Vec<Option<LinkGeom>> =
        fitted.iter().map(|g| matches!(g, LinkGeom::Arc { .. }).then(|| refine_round(g.clone(), exact, size * 2e-3))).collect();
    let mut stands: Vec<bool> = refits.iter().map(Option::is_some).collect();
    // Where two arcs met as fitted: (arc, its end, other arc, its end).
    let mut joints = Vec::new();
    for (i, gi) in fitted.iter().enumerate() {
        for (j, gj) in fitted.iter().enumerate().skip(i + 1) {
            let (Some(ei), Some(ej)) = (ends(gi), ends(gj)) else { continue };
            for (ki, pi) in ei.iter().enumerate() {
                for (kj, pj) in ej.iter().enumerate() {
                    if pi.dist(*pj) <= meet {
                        joints.push((i, ki, j, kj));
                    }
                }
            }
        }
    }
    // Take back both refits at a joint they would tear, until no joint tears.
    let end_now = |stands: &[bool], i: usize, k: usize| {
        let g = if stands.get(i).copied().unwrap_or(false) { refits.get(i).and_then(Option::as_ref) } else { fitted.get(i) };
        g.and_then(ends).and_then(|e| e.get(k).copied())
    };
    let mut changed = true;
    while changed {
        changed = false;
        for &(i, ki, j, kj) in &joints {
            let (Some(p), Some(q)) = (end_now(&stands, i, ki), end_now(&stands, j, kj)) else { continue };
            if p.dist(q) > solidcraft_sketch::ENDPOINT_JOIN_TOL {
                for k in [i, j] {
                    if let Some(s) = stands.get_mut(k).filter(|s| **s) {
                        *s = false;
                        changed = true;
                    }
                }
            }
        }
    }
    for (i, g) in fitted.iter().enumerate() {
        match g {
            LinkGeom::Arc { a: a0, b: b0, .. } => {
                let (true, Some(refined @ LinkGeom::Arc { a: a1, b: b1, .. })) =
                    (stands.get(i).copied().unwrap_or(false), refits.get(i).and_then(Option::as_ref))
                else {
                    continue;
                };
                for h in v.iter_mut() {
                    if let LinkGeom::Line(s, t) = h {
                        for end in [s, t] {
                            if end.dist(*a0) <= meet {
                                *end = *a1;
                            } else if end.dist(*b0) <= meet {
                                *end = *b1;
                            }
                        }
                    }
                }
                if let Some(slot) = v.get_mut(i) {
                    *slot = refined.clone();
                }
            }
            LinkGeom::Circle(..) => {
                if let Some(slot) = v.get_mut(i) {
                    *slot = refine_round(g.clone(), exact, size * 2e-3);
                }
            }
            // Lines have moved with the arcs they meet; nothing else is refitted.
            _ => {}
        }
    }
    v
}

/// Re-fit a circle or arc through the exact section points near it, when there are enough.
fn refine_round(g: LinkGeom, exact: &[Vec2], tol: f64) -> LinkGeom {
    let (c, r) = match g {
        LinkGeom::Circle(c, r) => (c, r),
        LinkGeom::Arc { c, a, .. } => (c, c.dist(a)),
        _ => return g,
    };
    let near: Vec<Vec2> = exact.iter().filter(|p| (p.dist(c) - r).abs() <= tol).copied().collect();
    // Arcs: only points within the arc's sweep.
    let near: Vec<Vec2> = match g {
        LinkGeom::Arc { c, a, b } => {
            let (s0, sw) = ((a - c).angle(), ((b - c).angle() - (a - c).angle()).rem_euclid(std::f64::consts::TAU));
            near.into_iter().filter(|p| ((*p - c).angle() - s0).rem_euclid(std::f64::consts::TAU) <= sw + 1e-9).collect()
        }
        _ => near,
    };
    let Some((c2, r2)) = (near.len() >= 4).then(|| ls_circle(&near)).flatten() else { return g };
    if c2.dist(c) > tol || (r2 - r).abs() > tol || near.iter().any(|p| (p.dist(c2) - r2).abs() > tol * 1e-3) {
        return g;
    }
    let on = |q: Vec2| c2 + (q - c).normalized().unwrap_or(Vec2::X) * r2;
    match g {
        LinkGeom::Arc { a, b, .. } => LinkGeom::Arc { c: c2, a: on(a), b: on(b) },
        _ => LinkGeom::Circle(c2, r2),
    }
}

/// Chain loose 2D segments into polylines (merging coincident ends) and fit each chain.
fn fit_segments(segs: &[(Vec2, Vec2)], size: f64, preserve_arcs: bool) -> Vec<LinkGeom> {
    // Mesh chords stray from the surface by up to the display tolerance (1/1000 of the size).
    // A silhouette's vertices lie on the surface, so it is fitted tightly: a loose fit lets a line
    // swallow the start of a tangent arc, or an arc run on into the straight edge after it.
    let loose = Some(if preserve_arcs { size * 1e-5 } else { size * 2e-3 });
    let q = size * 1e-7;
    let key = |p: Vec2| ((p.x / q).round() as i64, (p.y / q).round() as i64);
    let mut nodes: BTreeMap<(i64, i64), (Vec2, Vec<usize>)> = BTreeMap::new();
    let mut uniq: Vec<((i64, i64), (i64, i64))> = Vec::new();
    for (a, b) in segs {
        let (ka, kb) = (key(*a), key(*b));
        if ka == kb {
            continue;
        }
        let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
        if uniq.contains(&k) {
            continue;
        }
        let si = uniq.len();
        uniq.push(k);
        nodes.entry(ka).or_insert_with(|| (*a, Vec::new())).1.push(si);
        nodes.entry(kb).or_insert_with(|| (*b, Vec::new())).1.push(si);
        if uniq.len() > 200_000 {
            break;
        }
    }
    let mut used = vec![false; uniq.len()];
    let mut out = Vec::new();
    let walk = |start: (i64, i64), used: &mut Vec<bool>| -> Vec<Vec2> {
        let mut chain = vec![nodes.get(&start).map(|n| n.0).unwrap_or_default()];
        let mut cur = start;
        while let Some((_, adj)) = nodes.get(&cur) {
            let next = adj.iter().copied().find(|s| !used.get(*s).copied().unwrap_or(true));
            let Some(s) = next else { break };
            if let Some(u) = used.get_mut(s) {
                *u = true;
            }
            let Some(&(ka, kb)) = uniq.get(s) else { break };
            cur = if ka == cur { kb } else { ka };
            chain.push(nodes.get(&cur).map(|n| n.0).unwrap_or_default());
            // Stop at junctions so each chain is a simple path.
            if nodes.get(&cur).is_some_and(|n| n.1.len() != 2) {
                break;
            }
        }
        chain
    };
    // Open chains start at ends and junctions; what is left are closed loops.
    let starts: Vec<(i64, i64)> = nodes.iter().filter(|(_, n)| n.1.len() != 2).map(|(k, _)| *k).collect();
    for s in starts {
        while nodes.get(&s).is_some_and(|n| n.1.iter().any(|e| !used.get(*e).copied().unwrap_or(true))) {
            let c = walk(s, &mut used);
            fit_chain_with_arcs(&c, loose, &mut out, preserve_arcs);
        }
    }
    let keys: Vec<(i64, i64)> = nodes.keys().copied().collect();
    for s in keys {
        while nodes.get(&s).is_some_and(|n| n.1.iter().any(|e| !used.get(*e).copied().unwrap_or(true))) {
            let c = walk(s, &mut used);
            fit_chain_with_arcs(&c, loose, &mut out, preserve_arcs);
        }
    }
    if preserve_arcs {
        snap_tangent_arcs(&mut out, size * 1e-3);
    }
    out
}

/// A fitted arc is only a least-squares circle: make it exactly tangent to the straight edges
/// it runs into, ending on them where they touch it, with one radius.
fn snap_tangent_arcs(out: &mut [LinkGeom], tol: f64) {
    for i in 0..out.len() {
        let Some(LinkGeom::Arc { c, a, b }) = out.get(i).cloned() else { continue };
        let r = (c.dist(a) + c.dist(b)) / 2.0;
        // Per arc end: (gap to the line's end, line index, line end index, tangent point, unit
        // normal towards the centre, centre distance)
        let mut hits: [Vec<(f64, usize, usize, Vec2, Vec2, f64)>; 2] = [Vec::new(), Vec::new()];
        for (end, e) in [a, b].into_iter().enumerate() {
            for (j, g) in out.iter().enumerate() {
                let LinkGeom::Line(p, q) = g else { continue };
                let Some(u) = (*q - *p).normalized() else { continue };
                let foot = *p + u * (c - *p).dot(u);
                let d = c.dist(foot);
                if (d - r).abs() > tol || foot.dist(e) > 0.5 * r {
                    continue;
                }
                let Some(n) = (c - foot).normalized() else { continue };
                // The end of the line nearest the arc, and the tangent point must lie before it.
                let (near, far) = if p.dist(e) <= q.dist(e) { (0, *q) } else { (1, *p) };
                let near_pt = if near == 0 { *p } else { *q };
                // The line must itself end next to the arc: not just lie on the same infinite line.
                if (foot - far).dot(near_pt - far) <= 0.0 || near_pt.dist(foot) > 0.25 * r {
                    continue;
                }
                // When this end of the arc already meets another piece of the outline and the line
                // doesn't end there, the line lies further along (past that piece, as along a
                // conic fitted with several arcs): moving the end onto it would leave a gap.
                let gap = near_pt.dist(e);
                let meets = |g: &LinkGeom| match *g {
                    LinkGeom::Line(s, t) | LinkGeom::Arc { a: s, b: t, .. } => s.dist(e) <= tol || t.dist(e) <= tol,
                    _ => false,
                };
                if gap > tol && out.iter().enumerate().any(|(k, g)| k != i && k != j && meets(g)) {
                    continue;
                }
                if let Some(h) = hits.get_mut(end) {
                    h.push((gap, j, near, foot, n, d));
                }
            }
        }
        for h in &mut hits {
            h.sort_by(|x, y| x.0.total_cmp(&y.0));
        }
        // Each end takes the line ending nearest it. Both ends on one line would pull the arc down
        // to a point: the end nearer that line's end keeps it, the other takes its next line.
        let [ha, hb] = &hits;
        let (ta, tb) = match (ha.first(), hb.first()) {
            (Some(x), Some(y)) if x.1 == y.1 => {
                if x.0 <= y.0 {
                    (Some(x), hb.iter().find(|h| h.1 != x.1))
                } else {
                    (ha.iter().find(|h| h.1 != y.1), Some(y))
                }
            }
            pair => pair,
        };
        let hits_by_end: [Option<(usize, usize, Vec2, Vec2, f64)>; 2] = [ta, tb].map(|h| h.map(|&(_, j, near, foot, n, d)| (j, near, foot, n, d)));
        let (mut c, mut r) = (c, r);
        if let [Some((_, _, _, n1, d1)), Some((_, _, _, n2, d2))] = hits_by_end {
            // One circle tangent to both lines: equal distance to each.
            let det = n1.cross(n2);
            if det.abs() > 1e-6 {
                let m = (d1 + d2) / 2.0;
                let (r1, r2) = (m - d1, m - d2);
                let delta = Vec2::new((r1 * n2.y - r2 * n1.y) / det, (n1.x * r2 - n2.x * r1) / det);
                c += delta;
                r = m;
            }
        } else if let [Some((_, _, _, _, d)), None] | [None, Some((_, _, _, _, d))] = hits_by_end {
            r = d;
        }
        let on = |q: Vec2| c + (q - c).normalized().unwrap_or(Vec2::X) * r;
        let (mut na, mut nb) = (on(a), on(b));
        for (end, hit) in hits_by_end.into_iter().enumerate() {
            let Some((j, near, _, n, _)) = hit else { continue };
            let t = c - n * r;
            if let Some(LinkGeom::Line(p, q)) = out.get_mut(j) {
                if near == 0 {
                    *p = t;
                } else {
                    *q = t;
                }
            }
            if end == 0 {
                na = t;
            } else {
                nb = t;
            }
        }
        if let Some(LinkGeom::Arc { c: cc, a: aa, b: bb }) = out.get_mut(i) {
            (*cc, *aa, *bb) = (c, na, nb);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Fitting

fn circumcircle(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-18 {
        return None;
    }
    let (a2, b2, c2) = (a.len2(), b.len2(), c.len2());
    let o = Vec2::new((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d);
    let r = o.dist(a);
    (o.is_finite() && r.is_finite() && r < 1e7).then_some((o, r))
}

/// Least-squares circle (algebraic fit, in coordinates centred on the points' mean).
fn ls_circle(p: &[Vec2]) -> Option<(Vec2, f64)> {
    let n = p.len() as f64;
    if p.len() < 3 {
        return None;
    }
    let m = p.iter().fold(Vec2::ZERO, |a, q| a + *q) / n;
    // x² + y² + D x + E y + F = 0 → normal equations.
    let (mut a, mut r) = ([[0.0f64; 3]; 3], [0.0f64; 3]);
    for q in p {
        let (x, y) = (q.x - m.x, q.y - m.y);
        let row = [x, y, 1.0];
        let rhs = -(x * x + y * y);
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += row[i] * row[j];
            }
            r[i] += row[i] * rhs;
        }
    }
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d0 = det(a);
    if d0.abs() < 1e-30 {
        return None;
    }
    let mut sol = [0.0; 3];
    for (k, s) in sol.iter_mut().enumerate() {
        let mut mk = a;
        for i in 0..3 {
            mk[i][k] = r[i];
        }
        *s = det(mk) / d0;
    }
    let c = Vec2::new(-sol[0] / 2.0, -sol[1] / 2.0);
    let rr = c.len2() - sol[2];
    let c = c + m;
    (rr > 0.0 && c.is_finite() && rr.sqrt() < 1e7).then(|| (c, rr.sqrt()))
}

fn fits_line(p: &[Vec2], tol: f64) -> bool {
    let (Some(a), Some(b)) = (p.first(), p.last()) else { return false };
    let d = *b - *a;
    let l = d.len();
    if l <= tol {
        return false;
    }
    let u = d / l;
    let mut last = -tol;
    p.iter().all(|q| {
        let t = (*q - *a).dot(u);
        let ok = u.cross(*q - *a).abs() <= tol && t >= last - tol && t <= l + tol;
        last = t;
        ok
    })
}

/// Circle through the chain (centre, radius, signed sweep from the first to the last point).
/// `exact`: the points lie on the curve (edge tessellation), else they are approximate
/// (mesh sections and silhouettes) and the circle is a least-squares fit.
fn fits_circle(p: &[Vec2], tol: f64, exact: bool, preserve_arcs: bool) -> Option<(Vec2, f64, f64)> {
    let n = p.len();
    if n < 3 || (!exact && !preserve_arcs && n < 5) {
        return None;
    }
    let closed = p.first()?.dist(*p.last()?) <= tol;
    let (c, r) = if exact {
        let (i1, i2) = if closed { (n / 3, 2 * n / 3) } else { (n / 2, n - 1) };
        circumcircle(*p.first()?, *p.get(i1)?, *p.get(i2)?)?
    } else {
        ls_circle(p)?
    };
    if r > 1e6 || p.iter().any(|q| (q.dist(c) - r).abs() > tol) {
        return None;
    }
    let mut sweep = 0.0;
    for w in p.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let mut d = (*b - c).angle() - (*a - c).angle();
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        // Chords of a tessellated circle are short; points jumping around the circle (a
        // polygon inscribed in it) are not an arc.
        if d.abs() > 1.0 || (sweep != 0.0 && d * sweep < 0.0) {
            return None;
        }
        sweep += d;
    }
    Some((c, r, sweep))
}

fn arc_geom(p: &[Vec2], c: Vec2, r: f64, sweep: f64, tol: f64) -> Option<LinkGeom> {
    let (a, b) = (*p.first()?, *p.last()?);
    if sweep.abs() >= std::f64::consts::TAU - 1e-3 || a.dist(b) <= tol {
        return Some(LinkGeom::Circle(c, r));
    }
    // Ends exactly on the circle.
    let on = |q: Vec2| c + (q - c).normalized().unwrap_or(Vec2::X) * r;
    let (a, b) = (on(a), on(b));
    Some(if sweep > 0.0 { LinkGeom::Arc { c, a, b } } else { LinkGeom::Arc { c, a: b, b: a } })
}

/// Fit lines and circular arcs to a 2D polyline: the whole chain when it is one line or arc,
/// else greedy maximal pieces. `loose`: the points are approximate (within `loose` mm), and
/// sharp corners always split the chain.
pub(crate) fn fit_chain(raw: &[Vec2], loose: Option<f64>, out: &mut Vec<LinkGeom>) {
    fit_chain_with_arcs(raw, loose, out, false);
}

fn fit_chain_with_arcs(raw: &[Vec2], loose: Option<f64>, out: &mut Vec<LinkGeom>, preserve_arcs: bool) {
    let mut p: Vec<Vec2> = Vec::with_capacity(raw.len());
    let scale = raw.iter().fold(0.0_f64, |m, q| m.max(q.x.abs()).max(q.y.abs())).max(1.0);
    let ext = match (
        raw.iter().map(|q| q.x).reduce(f64::min),
        raw.iter().map(|q| q.x).reduce(f64::max),
        raw.iter().map(|q| q.y).reduce(f64::min),
        raw.iter().map(|q| q.y).reduce(f64::max),
    ) {
        (Some(x0), Some(x1), Some(y0), Some(y1)) => (x1 - x0).max(y1 - y0),
        _ => return,
    };
    let exact = loose.is_none();
    let tol = loose.unwrap_or(1e-6 * scale.max(ext)).max(1e-9);
    for q in raw {
        if !q.is_finite() {
            return;
        }
        if p.last().is_none_or(|l| l.dist(*q) > tol * 1e-3) {
            p.push(*q);
        }
    }
    if p.len() > MAX_CHAIN {
        let step = p.len().div_ceil(MAX_CHAIN);
        let last = p.last().copied();
        p = p.iter().step_by(step).copied().collect();
        if let Some(l) = last
            && p.last() != Some(&l)
        {
            p.push(l);
        }
    }
    if p.len() < 2 || ext <= tol {
        return;
    }
    if !exact {
        // Split at corners (turns sharper than 30 degrees); a closed chain starts at one.
        let n = p.len();
        let closed = n > 3 && p.first().zip(p.last()).is_some_and(|(a, b)| a.dist(*b) <= tol);
        let turn = |i: usize| -> f64 {
            let prev = if i == 0 { if closed { n.saturating_sub(2) } else { return 0.0 } } else { i - 1 };
            let next = if i + 1 >= n { if closed { 1 } else { return 0.0 } } else { i + 1 };
            match (p.get(prev), p.get(i), p.get(next)) {
                (Some(a), Some(b), Some(c)) => match ((*b - *a).normalized(), (*c - *b).normalized()) {
                    (Some(u), Some(v)) => u.cross(v).atan2(u.dot(v)).abs(),
                    _ => 0.0,
                },
                _ => 0.0,
            }
        };
        // A coarsely tessellated round turns up to ~35 degrees per chord; keep those together.
        let corner_turn = if preserve_arcs { 50f64 } else { 30f64 }.to_radians();
        let corners: Vec<usize> = (0..n).filter(|i| turn(*i) > corner_turn).collect();
        let cut = |a: usize, b: usize, out: &mut Vec<LinkGeom>| {
            if let Some(s) = p.get(a..=b) {
                fit_piece(s, tol, exact, preserve_arcs, out);
            }
        };
        if closed && let Some(&c0) = corners.first() {
            // Rotate so the loop starts and ends at a corner.
            let mut q: Vec<Vec2> = p.get(c0..n.saturating_sub(1)).unwrap_or_default().to_vec();
            q.extend(p.get(..=c0).unwrap_or_default());
            let m = q.len();
            let shift = |i: usize| (i + n - 1 - c0) % (n - 1);
            let mut cs: Vec<usize> = corners.iter().map(|c| shift(*c)).filter(|c| *c > 0 && *c < m - 1).collect();
            cs.sort_unstable();
            let mut start = 0;
            for c in cs.into_iter().chain([m - 1]) {
                if let Some(s) = q.get(start..=c) {
                    fit_piece(s, tol, exact, preserve_arcs, out);
                }
                start = c;
            }
            return;
        }
        let mut start = 0;
        for c in corners.into_iter().filter(|c| *c > 0 && *c < n - 1).chain([n - 1]) {
            cut(start, c, out);
            start = c;
        }
        return;
    }
    fit_piece(&p, tol, exact, preserve_arcs, out);
}

fn fit_piece(p: &[Vec2], tol: f64, exact: bool, preserve_arcs: bool, out: &mut Vec<LinkGeom>) {
    if p.len() < 2 {
        return;
    }
    // A near-straight run is a line, not an arc of huge radius.
    let curved = |sw: f64| !preserve_arcs || exact || sw.abs() >= 20f64.to_radians();
    let fitted_arc = || fits_circle(p, tol, exact, preserve_arcs).filter(|(_, _, sw)| curved(*sw)).and_then(|(c, r, sw)| arc_geom(p, c, r, sw, tol));
    if preserve_arcs
        && (exact || p.len() >= 4)
        && let Some(g) = fitted_arc()
    {
        out.push(g);
        return;
    }
    if fits_line(p, tol) {
        if let (Some(a), Some(b)) = (p.first(), p.last()) {
            out.push(LinkGeom::Line(*a, *b));
        }
        return;
    }
    if !preserve_arcs && let Some(g) = fitted_arc() {
        out.push(g);
        return;
    }
    // Greedy pieces.
    let mut i = 0;
    let n = p.len();
    let mut guard = 0;
    while i + 1 < n && guard < MAX_CHAIN * 2 {
        guard += 1;
        let mut jl = i + 1;
        while jl + 1 < n && p.get(i..=jl + 1).is_some_and(|s| fits_line(s, tol)) {
            jl += 1;
        }
        let mut jc = i + 1;
        let mut arc = None;
        while jc + 1 < n {
            match p.get(i..=jc + 1).and_then(|s| fits_circle(s, tol, exact, preserve_arcs)) {
                Some(f) => {
                    arc = Some(f);
                    jc += 1;
                }
                None => break,
            }
        }
        if jc >= jl + 2
            && jc - i >= 3
            && let Some((c, r, sw)) = arc.filter(|(_, _, sw)| curved(*sw))
            && let Some(g) = p.get(i..=jc).and_then(|s| arc_geom(s, c, r, sw, tol))
        {
            out.push(g);
            i = jc;
        } else {
            if let (Some(a), Some(b)) = (p.get(i), p.get(jl)) {
                out.push(LinkGeom::Line(*a, *b));
            }
            i = jl;
        }
    }
}

#[cfg(test)]
mod exact_loop_tests {
    use super::*;
    use solidcore_record::brep::Curve2;

    #[test]
    fn a_full_rational_circle_has_an_exact_signed_area() {
        let c = std::f64::consts::FRAC_1_SQRT_2;
        let edge = solidcore_record::brep::OrientedEdge {
            edge: 0,
            forward: true,
            pcurve: Curve2::RationalSpline {
                knots: vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0],
                homogeneous: vec![
                    [5., 0., 1.],
                    [5. * c, 5. * c, c],
                    [0., 5., 1.],
                    [-5. * c, 5. * c, c],
                    [-5., 0., 1.],
                    [-5. * c, -5. * c, c],
                    [0., -5., 1.],
                    [5. * c, -5. * c, c],
                    [5., 0., 1.],
                ],
            },
        };
        let spans = pcurve_spans(&edge, &mut 2_000_000).unwrap();
        assert_eq!(spans.len(), 4, "{spans:?}");
        let area = loop_area(core::slice::from_ref(&edge)).unwrap();
        assert!((area - std::f64::consts::PI * 25.).abs() < 1e-10, "{area}");
    }

    /// A standard quarter-circle, degree-elevated to a cubic rational Bézier. W'(t) changes
    /// sign on this curve, so `rational_area` must not require a positive derivative weight.
    #[test]
    fn a_degree_elevated_quarter_circle_has_the_expected_area() {
        let c = std::f64::consts::FRAC_1_SQRT_2;
        let p0 = solidcraft_geom::bspline::HPoint { x: 1.0, y: 0.0, z: 0.0, w: 1.0 };
        let p1 = solidcraft_geom::bspline::HPoint { x: c, y: c, z: 0.0, w: c };
        let p2 = solidcraft_geom::bspline::HPoint { x: 0.0, y: 1.0, z: 0.0, w: 1.0 };
        let cubic = [p0, p0 * (1.0 / 3.0) + p1 * (2.0 / 3.0), p1 * (2.0 / 3.0) + p2 * (1.0 / 3.0), p2];
        let area = rational_area(&cubic).expect("degree-elevated quarter circle");
        assert!((area - std::f64::consts::FRAC_PI_4).abs() < 1e-8, "{area}");
    }
}
