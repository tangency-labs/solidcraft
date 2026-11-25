//! Resolving sketch links against the model: projecting edges, faces, body silhouettes, other
//! sketches and the origin onto a sketch plane, and intersecting bodies with it.
//!
//! Model geometry is found again on every recompute: a face or an edge by the persistent name
//! its link keeps, anything else (and a link saved without a name) from the reference point
//! stored in the link. An edge of a solid lying in a plane parallel to the sketch's comes in as
//! its exact curve (a line, an arc, the Bézier pieces of a free-form curve such as a letter
//! cut from a font), so a profile drawn round it meets the solid's faces exactly.
//! Spline edges also project exactly between nonparallel planes, keeping their knots, weights
//! and selected interval. Straight edges stay lines, and tilted circles become ellipses or
//! conics. Unsupported native curves fail explicitly; mesh bodies retain polyline fitting.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::Mesh;
use solidcraft_sketch::{CurveKind, Link, LinkGeom, LinkKind, LinkPlacement, LinkSource, Sketch, WireCurve};

use crate::eval::ModelState;
use crate::expr::Value;
use crate::{DocError, Document, Mat, PlaneRef, Result};

/// Half length of the line an axis or plane projects to.
const AXIS_HALF: f64 = 100.0;
/// Points in one fitted chain at most (longer chains are thinned).
const MAX_CHAIN: usize = 4000;
pub(crate) const MAX_SKETCH_PLACEMENTS: usize = 1_000;

#[path = "project_order.rs"]
mod order;

/// Sketch definitions read by one link, including the two sides of an intersection.
pub(crate) fn source_sketches(source: &LinkSource) -> Result<std::collections::BTreeSet<u64>> {
    let mut sketches = std::collections::BTreeSet::new();
    let mut pending = vec![(source, 0)];
    let mut work = 0;
    while let Some((source, depth)) = pending.pop() {
        work += 1;
        if depth > 64 || work > MAX_SKETCH_PLACEMENTS {
            return Err(DocError::Invalid("too many nested sketch projection references".into()));
        }
        match source {
            LinkSource::SketchCurve { sketch, .. } | LinkSource::SketchPoint { sketch, .. } | LinkSource::OnSurface { sketch, .. } => {
                sketches.insert(*sketch);
            }
            LinkSource::Intersection { a, b } => pending.extend([(a.as_ref(), depth + 1), (b.as_ref(), depth + 1)]),
            _ => {}
        }
    }
    Ok(sketches)
}

/// Keep foreign sketch references in the assembly context in which an independently placed
/// part was drawn. Root and same-component references continue to use their live placements.
pub fn capture_link_sketch_placements(
    doc: &Document,
    author: u64,
    frame: &crate::Mat,
    source: &LinkSource,
) -> Result<Vec<solidcraft_sketch::SketchPlacement>> {
    if author == 0 {
        return Ok(Vec::new());
    }
    let inverse = crate::mat_inverse(frame).ok_or_else(|| DocError::Invalid("projection author frame is invalid".into()))?;
    let mut placements = Vec::new();
    for sketch in source_sketches(source)? {
        let Some(feature) = doc.feature(sketch).filter(|f| f.component != author) else { continue };
        let placement = crate::mat_mul(&inverse, &doc.component_transform(feature.component));
        if !crate::is_placement(&placement) {
            return Err(DocError::Invalid("captured sketch projection placement is invalid".into()));
        }
        placements.push(solidcraft_sketch::SketchPlacement { sketch, placement });
    }
    Ok(placements)
}

/// A copy or feature move can make a formerly local reference foreign. Capture only the new
/// relationships after reframing existing captures; a saved context is never replaced here.
pub fn capture_missing_sketch_references(doc: &mut Document, id: u64, frame: Option<crate::Mat>) -> Result<()> {
    let Some(feature) = doc.feature(id).filter(|feature| feature.component != 0) else { return Ok(()) };
    let crate::FeatureKind::Sketch { plane, sketch } = &feature.kind else { return Ok(()) };
    let frame = frame.unwrap_or_else(|| doc.component_transform(feature.component));
    let mut additions = Vec::new();
    for link in &sketch.links {
        let mut pins = capture_link_sketch_placements(doc, feature.component, &frame, &link.source)?;
        pins.retain(|pin| !link.sketch_placements.iter().any(|saved| saved.sketch == pin.sketch));
        if !pins.is_empty() {
            additions.push((link.id.clone(), pins));
        }
    }
    let mut base = plane;
    let mut next_plane = None;
    for _ in 0..=64 {
        match base {
            PlaneRef::Construction { feature: source, placement: None }
                if doc.feature(source.0).is_some_and(|source| source.component != feature.component) =>
            {
                let mut captured = plane.clone();
                crate::capture_sketch_plane_placement(doc, &mut captured, feature.component, &frame)?;
                next_plane = Some(captured);
                break;
            }
            PlaneRef::Offset { base: inner, .. }
            | PlaneRef::Flipped { base: inner, .. }
            | PlaneRef::AtAngle { base: inner, .. }
            | PlaneRef::ThroughPoint { base: inner, .. } => base = inner,
            _ => break,
        }
    }
    if let Some(crate::Feature { kind: crate::FeatureKind::Sketch { plane, sketch }, .. }) = doc.feature_mut(id) {
        if let Some(captured) = next_plane {
            *plane = captured;
        }
        for (id, pins) in additions {
            if let Some(link) = sketch.links.iter_mut().find(|link| link.id == id) {
                link.sketch_placements.extend(pins);
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_link_sketch_placements(link: &Link) -> Result<()> {
    if link.sketch_placements.is_empty() {
        return Ok(());
    }
    let invalid = || DocError::Invalid("invalid captured sketch projection placement".into());
    if link.sketch_placements.len() > MAX_SKETCH_PLACEMENTS {
        return Err(invalid());
    }
    let sources = source_sketches(&link.source)?;
    let mut seen = std::collections::BTreeSet::new();
    for pin in &link.sketch_placements {
        if pin.sketch == 0 || !sources.contains(&pin.sketch) || !seen.insert(pin.sketch) || !crate::is_placement(&pin.placement) {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Re-express saved relationships when definitions are copied or moved into another frame.
pub fn reframe_link_sketch_placements(sketch: &mut Sketch, author: &crate::Mat, source: impl Fn(u64) -> crate::Mat) -> Result<()> {
    for link in &mut sketch.links {
        validate_link_sketch_placements(link)?;
        for pin in &mut link.sketch_placements {
            let inverse = crate::mat_inverse(&source(pin.sketch)).ok_or_else(|| DocError::Invalid("projection source frame is invalid".into()))?;
            let placement = crate::mat_mul(&crate::mat_mul(author, &pin.placement), &inverse);
            if !crate::is_placement(&placement) {
                return Err(DocError::Invalid("reframed sketch projection placement is invalid".into()));
            }
            pin.placement = placement;
        }
    }
    Ok(())
}

/// `st` is already in the author's current frame. Correct only the source planes read by this
/// link, from their current relative placement to the one captured with the link. Their solved
/// curves and points remain live. Each link starts from `st`, so separately picked contexts do
/// not change one another.
fn state_for_link<'a>(
    doc: &Document,
    st: &'a ModelState,
    link: &Link,
    frame: Option<&(u64, crate::Mat)>,
) -> Result<std::borrow::Cow<'a, ModelState>> {
    use std::borrow::Cow;
    validate_link_sketch_placements(link)?;
    let mut result = Cow::Borrowed(st);
    if link.sketch_placements.is_empty() {
        return Ok(result);
    }
    let (author, frame) = frame.ok_or_else(|| DocError::Invalid("captured sketch projection requires its author frame".into()))?;
    let inverse = crate::mat_inverse(frame).ok_or_else(|| DocError::Invalid("projection author frame is invalid".into()))?;
    for pin in &link.sketch_placements {
        let Some(feature) = doc.feature(pin.sketch) else { continue };
        let current =
            if feature.component == *author { crate::IDENTITY } else { crate::mat_mul(&inverse, &doc.component_transform(feature.component)) };
        let undo = crate::mat_inverse(&current).ok_or_else(|| DocError::Invalid("projection source frame is invalid".into()))?;
        let correction = crate::mat_mul(&pin.placement, &undo);
        if crate::is_identity(&correction) || st.sketch(pin.sketch).is_none() {
            continue;
        }
        if let Some(source) = result.to_mut().sketches.iter_mut().find(|sketch| sketch.feature == pin.sketch) {
            let source = std::sync::Arc::make_mut(source);
            source.plane = crate::eval::placed_plane(&correction, &source.plane)?;
        }
    }
    Ok(result)
}

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
    /// For a face's projection, the persistent name of the edge each piece of `geom` comes from
    /// (empty for an edge no name follows exactly); empty when the pieces do not stand one for
    /// one for edges. The link records them ([`Link::curve_edges`]) and is matched by them when
    /// it is read again (#18711).
    pub edge_names: Vec<String>,
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
    resolve_omitting(doc, vals, st, plane, kind, src, frame, &std::collections::BTreeSet::new())
}

/// [`resolve_at`] leaving out the face edges `skip` (indices in the source body's mesh): the
/// members deleted from a face link ([`omitted_edges`]).
#[allow(clippy::too_many_arguments)]
pub fn resolve_omitting(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    plane: &Plane,
    kind: LinkKind,
    src: &LinkSource,
    frame: Option<&(u64, crate::Mat)>,
    skip: &std::collections::BTreeSet<usize>,
) -> Result<Resolved> {
    let key = cache_key(st, plane, kind, src).map(|k| if skip.is_empty() { k } else { format!("{k}|omit{skip:?}") });
    if let Some(k) = &key
        && let Ok(c) = CACHE.lock()
        && let Some((_, r)) = c.iter().find(|(x, _)| x == k)
    {
        return Ok(r.clone());
    }
    let r = resolve_uncached(doc, vals, st, plane, kind, src, frame, skip)?;
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
    skip: &std::collections::BTreeSet<usize>,
) -> Result<Resolved> {
    let lost = |what: &str| DocError::Invalid(format!("{what} no longer exists"));
    let mut moved = false;
    let mut face_index = None;
    let mut edge_index = None;
    let mut vertex_index = None;
    let mut edge_names = Vec::new();
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
            let planar = || match st.bodies.get(bi) {
                Some(b) => Ok(exact_edges(b, plane, &[e])?.pop().flatten()),
                None => Ok(None),
            };
            let section = || match st.bodies.get(bi) {
                Some(b) => edge_section(b, e, plane),
                None => Err(lost("the edge")),
            };
            let pts = mesh.edges.get(e).cloned().unwrap_or_default();
            edge_geom(plane, kind, planar, section, (pts, || edge_curves(st.bodies.get(bi), e)), &mut wires, &mut exact)?
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
                LinkKind::Intersect => match st.bodies.get(bi).map(|b| axial_section(b, plane, f)).transpose()?.flatten() {
                    Some(exact) => exact,
                    None => section(plane, &mesh, Some(f), st.bodies.get(bi).is_some_and(shaded_flat)),
                },
                _ => {
                    let target = st.bodies.get(bi).ok_or_else(|| lost("the projection body"))?;
                    // A mesh body is one face with no edges: the flat patch of triangles picked,
                    // traced (#13464).
                    if target.body.is_mesh() {
                        crate::project3d::flat_outline(&mesh, q).iter().flat_map(|outline| fit_mesh_outline(plane, outline)).collect()
                    } else {
                        // Members deleted from the projection stay out by their source edge.
                        let edges: Vec<usize> = projection_face_edges(target, &mesh, f, plane)?.into_iter().filter(|e| !skip.contains(e)).collect();
                        let (g, names) = projected_edges_named(target, &mesh, plane, &edges)?;
                        let before = g.len();
                        let g = closed_by_silhouettes(target, plane, f as usize, g);
                        let g = if certified_planar_construction(target, f as usize) { merge_family_lines(g)? } else { g };
                        edge_names = names_kept(names, before, &g);
                        g
                    }
                }
            }
        }
        LinkSource::Body { body } => {
            let b = st.body(body).ok_or_else(|| lost(&format!("body {body}")))?;
            let mesh = b.mesh();
            match kind {
                LinkKind::Intersect => section(plane, &mesh, None, shaded_flat(b)),
                _ => match exact_silhouette(b, plane) {
                    Some(g) if !g.is_empty() => g,
                    _ => silhouette(plane, &mesh, Some(&b.body)),
                },
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
        // A drawing view's sketch resolves its view edges itself, on the state its view draws.
        LinkSource::ViewEdge { .. } => return Err(DocError::Invalid("a view edge is projected only in its drawing view's sketch".into())),
        LinkSource::SketchCurve { sketch, curve } => {
            let ss = st.sketch(*sketch).ok_or_else(|| lost(&format!("sketch {sketch}")))?;
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| lost(&format!("sketch curve {curve}")))?;
            match kind {
                LinkKind::Intersect => {
                    let curves = sketch_curves(ss, ci);
                    let pieces: Vec<_> = curves.iter().map(|piece| (&piece.curve, piece.interval)).collect();
                    let hits = solidcraft_kernel::curves::piece_plane_crossings(&pieces, plane).map_err(|e| DocError::Invalid(e.to_string()))?;
                    hits.into_iter().map(LinkGeom::Point).collect()
                }
                LinkKind::Include => {
                    let pts: Vec<Vec3> = ss.sketch.polyline(ci).iter().map(|p| ss.plane.to_world(*p)).collect();
                    if off_plane(plane, &pts) {
                        wires.push(pts);
                        exact.push(sketch_curves(ss, ci));
                        Vec::new()
                    } else {
                        project_sketch_curve(&ss.sketch, &ss.plane, ci, plane)?
                    }
                }
                _ => project_sketch_curve(&ss.sketch, &ss.plane, ci, plane)?,
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
                _ => {
                    let (author, frame_mat) = frame.copied().unwrap_or((0, crate::IDENTITY));
                    match crate::construct::DatumRef::parse_key(name) {
                        // The origin axis of a component (#15086), where that component now stands,
                        // in the sketch's frame.
                        Some(datum) => {
                            let resolved = crate::construct::resolve_datum(doc, &datum)?;
                            let geom = if frame.is_some() {
                                let inverse = crate::mat_inverse(&frame_mat)
                                    .ok_or_else(|| DocError::Invalid("the sketch's frame cannot be inverted".into()))?;
                                resolved.placed(&inverse)
                            } else {
                                resolved
                            };
                            match geom {
                                crate::construct::ConstructGeom::Axis { origin, dir } => (origin, dir),
                                _ => return Err(lost(&format!("axis {name}"))),
                            }
                        }
                        // A construction axis, by name (#9324), where the timeline so far puts it,
                        // in the sketch's frame.
                        None => crate::eval::construction_axis_in_frame(doc, st, name, author, &frame_mat)?,
                    }
                }
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
                PlaneRef::Construction { feature, placement: None }
            };
            let other = doc.resolve_plane_in(vals, &pr, 0, 0, Some(st)).map_err(|_| lost(&format!("plane {name}")))?;
            // A plane of another component is in that component's frame: brought into the
            // sketch's, the trace of the two planes is where they meet (#17534).
            let other = plane_in_sketch_frame(doc, &pr, other, st, frame)?;
            plane_trace(plane, &other).into_iter().collect()
        }
    };
    if geom.is_empty() && wires.is_empty() {
        return Err(DocError::Invalid(format!("the {} projects to nothing on this sketch plane", src.describe())));
    }
    let edge_names = names_kept(edge_names, geom.len(), &geom);
    Ok(Resolved { geom: merge_split_circles(geom), wires, exact, source, moved, face_index, edge_index, vertex_index, edge_names })
}

/// Whether a curve (world points) leaves the sketch plane: included, it then stays 3D.
fn off_plane(plane: &Plane, pts: &[Vec3]) -> bool {
    let size = pts.iter().fold(1.0_f64, |m, p| m.max(p.len()));
    pts.iter().any(|p| plane.height(*p).abs() > size * 1e-7)
}

/// What a body edge (its `exact` curve when read, and its polyline `pts`) gives a sketch: its
/// section with the sketch plane (`section`: where its exact curve crosses it, the whole edge
/// when it lies in the plane, a mesh body's polyline crossings), itself as a 3D curve (its
/// points into `wires`, its exact curve into `exact`) when it is included and leaves the plane,
/// else its projection (the exact planar curve `planar`, or the polyline fitted).
fn edge_geom(
    plane: &Plane,
    kind: LinkKind,
    planar: impl FnOnce() -> Result<Option<Vec<LinkGeom>>>,
    section: impl FnOnce() -> Result<EdgeSection>,
    (pts, curves): (Vec<Vec3>, impl FnOnce() -> Vec<WireCurve>),
    wires: &mut Vec<Vec<Vec3>>,
    exact: &mut Vec<Vec<WireCurve>>,
) -> Result<Vec<LinkGeom>> {
    Ok(match kind {
        LinkKind::Intersect => match section()? {
            EdgeSection::Meets(geom) => geom,
            EdgeSection::InPlane => planar()?.unwrap_or_else(|| fit_world(plane, &pts)),
            EdgeSection::Mesh => section_of_polyline(plane, &pts),
        },
        LinkKind::Include if off_plane(plane, &pts) => {
            wires.push(pts);
            exact.push(curves());
            Vec::new()
        }
        _ => planar()?.unwrap_or_else(|| fit_world(plane, &pts)),
    })
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

/// Where a body edge meets a sketch plane.
enum EdgeSection {
    /// The points where its exact curve crosses or touches the plane, and the parts of it lying
    /// in the plane, in the plane's coordinates.
    Meets(Vec<LinkGeom>),
    /// It lies in the plane: it meets the plane along its whole length.
    InPlane,
    /// A mesh body's edge: its display polyline is the edge.
    Mesh,
}

/// Where edge `e` of `body` meets `plane`: the kernel's section of the curve it evaluates the
/// edge on (an intersection curve's NURBS form), which tells an edge lying in the plane from
/// one crossing it and counts a contact at either end of the edge. A part of the edge lying in
/// the plane is projected exactly, as a whole edge lying in it is.
fn edge_section(body: &crate::ModelBody, e: usize, plane: &Plane) -> Result<EdgeSection> {
    use solidcraft_kernel::curves::PlaneSection;
    if body.body.is_mesh() {
        return Ok(EdgeSection::Mesh);
    }
    let edge = body.body.record().edges.get(e).ok_or_else(|| lost("the edge"))?;
    let curve = edge.parameters();
    let (points, parts) = match solidcraft_kernel::curves::plane_section(curve, edge.interval, plane).map_err(|e| DocError::Invalid(e.to_string()))? {
        PlaneSection::InPlane => return Ok(EdgeSection::InPlane),
        PlaneSection::Meets { points, in_plane } => (points, in_plane),
    };
    let mut geom: Vec<LinkGeom> = points.into_iter().map(LinkGeom::Point).collect();
    for part in parts {
        let projected = solidcraft_kernel::services::spline_projection::projected_part(
            curve,
            part.interval,
            part.ends,
            plane,
            body.body.planar_edges_tolerance(),
            solidcraft_sketch::MAX_SPLINE_POINTS,
            solidcraft_sketch::MAX_COORD,
        )
        .map_err(|e| DocError::Invalid(e.to_string()))?
        .ok_or_else(|| DocError::Invalid(format!("exact projection is not supported for the part of native edge {e} in the sketch plane")))?;
        geom.extend(projection_links(projected)?);
    }
    Ok(EdgeSection::Meets(geom))
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

/// `c` run the other way: negate its spline/angle parameter (with reversed controls
/// or a turned y axis), or use `1 - t` for a line. Knot negation preserves narrow spans.
fn reversed(c: &WireCurve) -> Option<WireCurve> {
    use solidcore_record::brep::{Curve, Frame};
    let [t0, t1] = c.interval;
    let flip = |frame: &Frame| Frame { origin: frame.origin, x_axis: frame.x_axis, y_axis: frame.y_axis.map(|v| -v) };
    let (curve, interval) = match &c.curve {
        Curve::Line { start, end } => (Curve::Line { start: *end, end: *start }, [1.0 - t1, 1.0 - t0]),
        Curve::Circle { frame, radius } => (Curve::Circle { frame: flip(frame), radius: *radius }, [-t1, -t0]),
        Curve::Ellipse { frame, radii } => (Curve::Ellipse { frame: flip(frame), radii: *radii }, [-t1, -t0]),
        Curve::Spline { knots, points } => {
            (Curve::Spline { knots: knots.iter().rev().map(|k| -k).collect(), points: points.iter().rev().copied().collect() }, [-t1, -t0])
        }
        Curve::RationalSpline { knots, homogeneous } => (
            Curve::RationalSpline { knots: knots.iter().rev().map(|k| -k).collect(), homogeneous: homogeneous.iter().rev().copied().collect() },
            [-t1, -t0],
        ),
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

/// Exact native edge projections. Only mesh bodies may use the polyline fallback;
/// unsupported native curves report an error rather than silently changing shape.
fn exact_edges(body: &crate::ModelBody, plane: &Plane, edges: &[usize]) -> Result<Vec<Option<Vec<LinkGeom>>>> {
    if body.body.is_mesh() {
        return Ok(vec![None; edges.len()]);
    }
    edges
        .iter()
        .map(|e| {
            let projected = if let Some(pieces) = body.body.cylinder_edge_projection(plane, *e) {
                solidcraft_kernel::services::conic_projection::classify_pieces(
                    plane,
                    &pieces,
                    solidcraft_sketch::MAX_SPLINE_POINTS,
                    solidcraft_sketch::MAX_COORD,
                )
                .map(Some)
            } else {
                solidcraft_kernel::services::spline_projection::projected_edge(
                    body.body.record(),
                    plane,
                    *e,
                    body.body.planar_edges_tolerance(),
                    solidcraft_sketch::MAX_SPLINE_POINTS,
                    solidcraft_sketch::MAX_COORD,
                )
            }
            .map_err(|e| DocError::Invalid(e.to_string()))?;
            match projected {
                Some(geometry) => projection_links(geometry).map(Some),
                None => Err(DocError::Invalid(format!("exact projection is not supported for native edge {e}"))),
            }
        })
        .collect()
}

/// Store the modelling operation's geometric images as document link state.
fn projection_links(geometry: Vec<solidcraft_kernel::services::conic_projection::Projection>) -> Result<Vec<LinkGeom>> {
    use solidcraft_kernel::services::conic_projection::Projection;
    geometry
        .into_iter()
        .map(|g| {
            Ok(match g {
                Projection::Segment(s) => {
                    LinkGeom::from_seg(&s).ok_or_else(|| DocError::Invalid("exact edge degree is not supported by a sketch".into()))?
                }
                Projection::Point(p) => LinkGeom::Point(p),
                Projection::Line(a, b) => LinkGeom::Line(a, b),
                Projection::Spline { spline, degree, implicit } => LinkGeom::Spline {
                    pts: spline.points,
                    control: true,
                    degree: u8::try_from(degree).map_err(|_| DocError::Invalid("exact edge degree is not supported by a sketch".into()))?,
                    periodic: false,
                    nurbs: (!implicit).then_some(solidcraft_sketch::Nurbs { knots: spline.knots, weights: spline.weights }),
                },
                Projection::Conic { a, apex, b, rho } => LinkGeom::Conic { a, apex, b, rho },
                Projection::Ellipse { center, major, minor } => LinkGeom::Ellipse { c: center, major, minor },
            })
        })
        .collect()
}

/// `edges` of `body` projected on `plane`: each exactly ([`exact_edges`]) or fitted to its
/// polyline in `mesh`.
///
/// With each piece, the persistent name of the edge it comes from, when
/// that name follows exact history (the faces it separates do, and it is not one of several edges
/// between them numbered by where they lie); empty otherwise, and for a mesh body.
fn projected_edges_named(body: &crate::ModelBody, mesh: &Mesh, plane: &Plane, edges: &[usize]) -> Result<(Vec<LinkGeom>, Vec<String>)> {
    let (mut out, owners) = projected_edges_owned(body, mesh, plane, edges)?;
    if body.body.is_mesh() {
        meet_at_shared_ends(&mut out, order::join_tolerance(mesh));
    }
    let named = (!body.body.is_mesh()).then(|| crate::name_lookup::BodyEdges::new(body));
    let name = |e: usize| named.as_ref().filter(|n| n.verified(e)).and_then(|n| n.name(e)).unwrap_or_default().to_string();
    let names = owners.into_iter().map(name).collect();
    Ok((out, names))
}

/// The names of `geom`'s pieces (see [`Resolved::edge_names`]) after a pass that keeps each piece
/// where it was unless it joins pieces together: kept while the pass gave as many pieces back.
fn names_kept(names: Vec<String>, before: usize, geom: &[LinkGeom]) -> Vec<String> {
    if names.len() == before && geom.len() == before { names } else { Vec::new() }
}

/// Where curve ends lie within `gap` of each other, moves them onto the first of them. A fitted
/// round's edges end a few 1e-5 mm short of their neighbours (#8284); the sketch only shares a
/// point between ends 1e-7 apart, and without it the outline has no closed profile.
fn meet_at_shared_ends(geom: &mut [LinkGeom], gap: f64) {
    let met = solidcraft_kernel::services::outline::meet_at_shared_ends(geom.iter().cloned().map(outline_piece).collect(), gap);
    for (g, m) in geom.iter_mut().zip(met) {
        *g = link_of_piece(m);
    }
}

/// Link geometry as an outline piece of the kernel's ([`solidcraft_kernel::services::outline::Piece`]), field
/// by field.
fn outline_piece(g: LinkGeom) -> solidcraft_kernel::services::outline::Piece {
    use solidcraft_kernel::services::outline::Piece;
    match g {
        LinkGeom::Point(p) => Piece::Point(p),
        LinkGeom::Line(a, b) => Piece::Line(a, b),
        LinkGeom::Circle(c, r) => Piece::Circle(c, r),
        LinkGeom::Arc { c, a, b } => Piece::Arc { c, a, b },
        LinkGeom::Conic { a, apex, b, rho } => Piece::Conic { a, apex, b, rho },
        LinkGeom::Ellipse { c, major, minor } => Piece::Ellipse { c, major, minor },
        LinkGeom::Spline { pts, control, degree, periodic, nurbs } => {
            Piece::Spline { pts, control, degree, periodic, nurbs: nurbs.map(|n| (n.knots, n.weights)) }
        }
    }
}

/// An outline piece of the kernel's as link geometry, field by field.
fn link_of_piece(g: solidcraft_kernel::services::outline::Piece) -> LinkGeom {
    use solidcraft_kernel::services::outline::Piece;
    match g {
        Piece::Point(p) => LinkGeom::Point(p),
        Piece::Line(a, b) => LinkGeom::Line(a, b),
        Piece::Circle(c, r) => LinkGeom::Circle(c, r),
        Piece::Arc { c, a, b } => LinkGeom::Arc { c, a, b },
        Piece::Conic { a, apex, b, rho } => LinkGeom::Conic { a, apex, b, rho },
        Piece::Ellipse { c, major, minor } => LinkGeom::Ellipse { c, major, minor },
        Piece::Spline { pts, control, degree, periodic, nurbs } => {
            LinkGeom::Spline { pts, control, degree, periodic, nurbs: nurbs.map(|(knots, weights)| solidcraft_sketch::Nurbs { knots, weights }) }
        }
    }
}

/// Each of `edges` projected on its own ([`projected_edges_named`] without the ends brought together).
/// With each piece, the edge (of `edges`) it comes from.
fn projected_edges_owned(body: &crate::ModelBody, mesh: &Mesh, plane: &Plane, edges: &[usize]) -> Result<(Vec<LinkGeom>, Vec<usize>)> {
    let mut out = Vec::new();
    let mut owners = Vec::new();
    for (e, exact) in edges.iter().zip(exact_edges(body, plane, edges)?) {
        let before = out.len();
        match exact {
            Some(mut g) => {
                at_vertices(body, plane, *e, &mut g);
                out.extend(g);
            }
            None => {
                if let Some(pts) = mesh.edges.get(*e) {
                    out.extend(fit_world(plane, pts));
                }
            }
        }
        owners.resize(out.len().max(before), *e);
    }
    Ok((out, owners))
}

/// Edge `e`'s exact pieces `g` with their outer ends on the edge's vertices, where they lie
/// within the edge's and the reading's tolerance of them. An edge's curve need only pass within
/// its tolerance of its vertices, and an arc is read off a NURBS within the planar-edge
/// tolerance: one SMLib made (a shell's offset corner) ended 2e-6 mm from the line it meets, and
/// the two ends, not taken for one point, left the loop open, so an offset of one curve found
/// no chain to follow. The vertex is the point both edges share.
fn at_vertices(body: &crate::ModelBody, plane: &Plane, e: usize, g: &mut [LinkGeom]) {
    let record = body.body.record();
    let Some(edge) = record.edges.get(e) else { return };
    let vertices: Option<Vec<_>> =
        edge.vertices.iter().map(|v| record.vertices.get(*v as usize).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]))).collect();
    let Some(vertices) = vertices else { return };
    // The pieces are read within the planar-edge tolerance of the edge's curve, which passes
    // within its own tolerance of the vertices.
    let source_tolerance = edge.vertices.iter().filter_map(|v| record.vertices.get(*v as usize)).map(|v| v.tolerance).fold(edge.tolerance, f64::max);
    let roundoff = vertices.iter().fold(plane.origin.len().max(1.), |s, v| s.max(v.len())) * f64::EPSILON * 256.;
    // Each vertex moves the piece end nearest it (the pieces' order and direction need not be
    // the edge's).
    for v in vertices.into_iter().map(|v| plane.to_local(v)) {
        let mut best: Option<(f64, &mut Vec2)> = None;
        for piece in g.iter_mut() {
            let tol = source_tolerance + if matches!(piece, LinkGeom::Spline { .. }) { roundoff } else { body.body.planar_edges_tolerance() };
            let ends = match piece {
                // A collapsed free-form curve may attain a bound in its interior. Moving
                // it to a nearby source vertex could cut that extremum off again.
                LinkGeom::Line(..)
                    if !matches!(edge.parameters(), solidcore_record::brep::Curve::Line { .. } | solidcore_record::brep::Curve::Polyline { .. }) =>
                {
                    None
                }
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
        LinkKind::Project => crate::name_lookup::locate_projected_edge_from(b, body, name, at),
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
    let planar = || Ok(exact_edges(target, plane, &[index])?.pop().flatten());
    let section = || edge_section(target, index, plane);
    let geom = edge_geom(plane, kind, planar, section, (pts, || edge_curves(Some(target), index)), &mut wires, &mut exact)?;
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
        edge_names: Vec::new(),
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
        edge_names: Vec::new(),
    })
}

fn resolve_named_face(
    st: &ModelState,
    plane: &Plane,
    kind: LinkKind,
    body: &str,
    index: usize,
    at: Vec3,
    skip: &std::collections::BTreeSet<usize>,
) -> Result<Resolved> {
    let target = st.body(body).ok_or_else(|| lost("named projection body"))?;
    let mesh = target.mesh();
    if mesh.triangles.len() > 2_000_000 || mesh.edges.len() > 2_000_000 {
        return Err(DocError::Invalid("named face projection work limit exceeded".into()));
    }
    let face_index = Some(index);
    let index = u32::try_from(index).map_err(|_| DocError::Invalid("named projection face index exceeds limit".into()))?;
    let mut edge_names = Vec::new();
    let geom = if kind == LinkKind::Intersect {
        match axial_section(target, plane, index)? {
            Some(exact) => exact,
            None => section(plane, &mesh, Some(index), shaded_flat(target)),
        }
    } else {
        let edges: Vec<usize> = projection_face_edges(target, &mesh, index, plane)?.into_iter().filter(|e| !skip.contains(e)).collect();
        let (geom, names) = projected_edges_named(target, &mesh, plane, &edges)?;
        let before = geom.len();
        let geom = closed_by_silhouettes(target, plane, index as usize, geom);
        let geom = merge_split_circles(geom);
        let geom = if certified_planar_construction(target, index as usize) { merge_family_lines(geom)? } else { geom };
        edge_names = names_kept(names, before, &geom);
        geom
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
        edge_names,
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
    resolve_anchored_face_omitting(doc, st, plane, kind, anchor, &std::collections::BTreeSet::new())
}

fn resolve_anchored_face_omitting(
    doc: &Document,
    st: &ModelState,
    plane: &Plane,
    kind: LinkKind,
    anchor: &crate::FaceAnchor,
    skip: &std::collections::BTreeSet<usize>,
) -> Result<Resolved> {
    let body = st.body(&anchor.target.body).ok_or_else(|| lost("anchored projection body"))?;
    // The sketch's plane says when the face was found by fallback; its projection follows it.
    let (indices, _) = doc.anchored_face_indices(st, body, &anchor.target)?;
    resolve_face_indices(st, plane, kind, &body.name, &indices, skip, None)
}

/// Project explicitly selected current face indices, without claiming persistent provenance.
/// Callers retaining a link must supply their own replay identity policy. The edges in `skip`
/// (a face link's deleted members) are left out. `pieces`, when given, receives each projected
/// source edge with the geometry it gives the sketch ([`face_link_edges`]).
pub fn resolve_face_indices(
    st: &ModelState,
    plane: &Plane,
    kind: LinkKind,
    body_name: &str,
    indices: &[usize],
    skip: &std::collections::BTreeSet<usize>,
    pieces: Option<&mut Vec<EdgePieces>>,
) -> Result<Resolved> {
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
    let mut edge_names = Vec::new();
    if kind == LinkKind::Intersect {
        for index in indices {
            let index = u32::try_from(*index).map_err(|_| DocError::Invalid("anchored face index exceeds format bounds".into()))?;
            match axial_section(body, plane, index)? {
                Some(exact) => geometry.extend(exact),
                None => geometry.extend(section(plane, &mesh, Some(index), shaded_flat(body))),
            }
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
        let edges: Vec<usize> = edges.into_iter().filter(|e| !skip.contains(e)).collect();
        if let Some(out) = pieces {
            // Each edge's pieces as the link has them: a circle whose two edges fit to the same
            // half is made whole again across all of them (`merge_split_circles`), keeping which
            // edge each is of. Each edge is read as `projected_edges_named` reads it.
            let names = crate::naming::edge_names(body);
            let mut owner = Vec::new();
            let mut flat = Vec::new();
            for (k, (e, exact)) in edges.iter().zip(exact_edges(body, plane, &edges)?).enumerate() {
                let fitted = || mesh.edges.get(*e).map(|pts| fit_world(plane, pts)).unwrap_or_default();
                for g in exact.unwrap_or_else(fitted) {
                    owner.push(k);
                    flat.push(g);
                }
            }
            *out = edges
                .iter()
                .map(|e| {
                    let name = names.get(*e).cloned().unwrap_or_default();
                    let faces_exact = mesh.edge_faces.get(*e).is_some_and(|fs| fs.iter().all(|f| crate::naming::face_name_exact(body, *f as usize)));
                    EdgePieces { exact: !name.is_empty() && faces_exact, name, geom: Vec::new() }
                })
                .collect();
            for (k, g) in owner.into_iter().zip(merge_split_circles(flat)) {
                if let Some(edge) = out.get_mut(k) {
                    edge.geom.push(g);
                }
            }
        }
        let (pieces, names) = projected_edges_named(body, &mesh, plane, &edges)?;
        // Only a single face's pieces are named: a family's edges are its outline, not one face's.
        let pieces = if let [index] = indices {
            let before = pieces.len();
            let pieces = closed_by_silhouettes(body, plane, *index, pieces);
            edge_names = names_kept(names, before, &pieces);
            pieces
        } else {
            pieces
        };
        geometry.extend(pieces);
    }
    if kind != LinkKind::Intersect && (indices.len() > 1 || indices.first().is_some_and(|index| certified_planar_construction(body, *index))) {
        let before = geometry.len();
        geometry = merge_family_lines(geometry)?;
        edge_names = names_kept(edge_names, before, &geometry);
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
        edge_names,
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
    /// The sketch fails, naming every such link ([`DocError::LostProjections`]).
    Fail,
    /// The link is marked lost and keeps its last geometry, with a warning naming it.
    /// No other face or edge is taken for it. Only for a failed sketch built to be shown, or to
    /// check what it fails for (#12085), never for the evaluation the features after it build from.
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

/// [`refresh_sketch_links_with`] for a caller that has only the model before the sketch: a link
/// to an exact placement reads that same model, in the component of `frame` (the root without one).
#[allow(clippy::too_many_arguments)]
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
    let placed = Placed { raw: st, component: frame.map_or(0, |(component, _)| *component) };
    refresh_sketch_links_with(doc, vals, placed, st, plane, sk, anchor, lost_names, frame)
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

/// Re-resolve the links of a sketch in a design without placed components: `st` is both the
/// model before the sketch and its component's view of it. Returns warnings for lost, moved or
/// rebuilt links; lost links keep their last geometry. A link kept by the name of a face or edge
/// that no longer finds exactly that face or edge is not lost: the sketch fails, naming every such
/// link ([`DocError::LostProjections`]).
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
        match sk.saved_text_source_geometry(&source) {
            Ok((geom, _)) if geom.is_empty() => warn.push(format!("the {} makes no outline", source.describe())),
            // (A missing font was told when the links were set, before the solve.)
            Ok((geom, _)) => {
                if let Err(e) = sk.update_link(&id, &geom) {
                    warn.push(format!("{}: {e}", source.describe()));
                }
            }
            Err(e) => warn.push(format!("{}: {e}", source.describe())),
        }
    }
    warn
}

/// What links to an exact placement of another component are resolved with: the model before
/// the sketch in each component's own frame (`raw`, as evaluation builds it) and the sketch's
/// component. Other links read the model as that component sees it.
#[derive(Clone, Copy)]
pub struct Placed<'a> {
    pub raw: &'a ModelState,
    pub component: u64,
}

/// "A:2/B:1": the names of a placement path's occurrences (an unknown id as `#id`).
pub fn path_label(doc: &Document, path: &[u64]) -> String {
    if path.is_empty() {
        return "the design".into();
    }
    path.iter()
        .take(solidcraft_sketch::MAX_PLACEMENT_PATH + 1)
        .map(|id| doc.occurrences.iter().find(|o| o.id == *id).map(|o| o.name.clone()).unwrap_or_else(|| format!("#{id}")))
        .collect::<Vec<_>>()
        .join("/")
}

/// World transform of placement `path` of `component` (empty: the root). An error names the
/// placement when it no longer exists or no longer places that component.
fn placement_frame(doc: &Document, component: u64, path: &[u64], what: &str) -> Result<Mat> {
    if path.len() > solidcraft_sketch::MAX_PLACEMENT_PATH || doc.occurrences.len() > crate::MAX_PLACEMENTS {
        return Err(DocError::Invalid(format!("{what} placement path exceeds work limits")));
    }
    let mut frame = crate::IDENTITY;
    let mut at = 0;
    for id in path {
        let o = doc
            .occurrences
            .iter()
            .find(|o| o.id == *id && o.parent == at)
            .ok_or_else(|| DocError::Unknown(format!("{what} placement {} no longer exists", path_label(doc, path))))?;
        frame = crate::mat_mul(&frame, &o.transform);
        at = o.component;
    }
    if at != component {
        return Err(DocError::Invalid(format!("{what} placement {} no longer holds its geometry", path_label(doc, path))));
    }
    if frame.iter().flatten().any(|x| !x.is_finite()) {
        return Err(DocError::Invalid(format!("{what} placement {} is not finite", path_label(doc, path))));
    }
    Ok(frame)
}

/// Is `m` a rotation (or a reflection) and a translation, as a placement is?
fn is_rigid(m: &Mat) -> bool {
    solidcraft_kernel::services::affine::is_rigid(m, 1e-6, 1e-12)
}

/// The transform from the source's own frame (its body's frame, or its sketch's component
/// frame) into the frame of the sketch's component `author`: `inverse(author placement) ·
/// source placement · body offset`.
pub fn placement_relative(doc: &Document, author: u64, placement: &LinkPlacement, source_component: u64, offset: Option<&Mat>) -> Result<Mat> {
    placement.validate()?;
    if author != 0 && placement.author.is_empty() {
        return Err(DocError::Invalid("the link does not record the placement of its sketch".into()));
    }
    let author_frame = placement_frame(doc, author, &placement.author, "sketch")?;
    let mut source = placement_frame(doc, source_component, &placement.source, "source")?;
    if let Some(offset) = offset {
        source = crate::mat_mul(&source, offset);
    }
    let inverse = crate::mat_inverse(&author_frame).ok_or_else(|| DocError::Invalid("the sketch's placement is singular".into()))?;
    let relative = crate::mat_mul(&inverse, &source);
    if !is_rigid(&relative) {
        return Err(DocError::Invalid("the placements between the source and the sketch are not rigid".into()));
    }
    Ok(relative)
}

/// The source of a placed link as it is now (a named edge followed into another body of its
/// component), and the transform from its frame into the sketch's ([`placement_relative`]).
/// Never another placement or another component's body: a source that is not found is lost.
pub fn placed_source(
    doc: &Document,
    placed: Placed,
    source: &LinkSource,
    edge_name: Option<&str>,
    placement: &LinkPlacement,
) -> Result<(LinkSource, Mat)> {
    placement.validate()?;
    let mut source = source.clone();
    let (component, offset) = match &mut source {
        LinkSource::Edge { body, at } | LinkSource::Face { body, at } | LinkSource::Vertex { body, at } => {
            let found = match placed.raw.body(body) {
                Some(b) => Some(b),
                // A named edge whose body was joined into another body of the same component is
                // that edge of the joined body; nothing outside the placement's component is searched.
                None => {
                    // An empty path is the root component (0).
                    let leaf = match placement.source.last() {
                        None => Some(0),
                        Some(id) => doc.occurrences.iter().find(|o| o.id == *id).map(|o| o.component),
                    };
                    edge_name.and_then(|name| {
                        placed
                            .raw
                            .bodies
                            .iter()
                            .filter(|b| !b.body.is_mesh() && Some(doc.model_body_component(b)) == leaf)
                            .filter_map(|b| crate::naming::locate_edge(b, name, *at).ok().map(|hit| (b, hit.point.dist(*at))))
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                            .map(|(b, _)| b)
                    })
                }
            };
            let b = found.ok_or_else(|| lost(&format!("body {body}")))?;
            *body = b.name.clone();
            (doc.model_body_component(b), doc.model_body_offset(b).copied())
        }
        LinkSource::Body { body } => {
            let b = placed.raw.body(body).ok_or_else(|| lost(&format!("body {body}")))?;
            (doc.model_body_component(b), doc.model_body_offset(b).copied())
        }
        LinkSource::SketchCurve { sketch, .. } | LinkSource::SketchPoint { sketch, .. } => {
            placed.raw.sketch(*sketch).ok_or_else(|| lost(&format!("sketch {sketch}")))?;
            (doc.feature(*sketch).map(|f| f.component).ok_or_else(|| lost(&format!("sketch {sketch}")))?, None)
        }
        other => return Err(DocError::Invalid(format!("not supported yet: the {} from another component's placement", other.describe()))),
    };
    let relative = placement_relative(doc, placed.component, placement, component, offset.as_ref())?;
    Ok((source, relative))
}

/// `plane` (in the sketch's component frame) seen from the source's frame, given the transform
/// from the source's frame into the sketch's. Projecting the source onto it gives the same
/// sketch coordinates as projecting the placed source onto `plane`, without moving the source.
fn plane_in_source(relative: &Mat, plane: &Plane) -> Result<Plane> {
    let inverse = crate::mat_inverse(relative).ok_or_else(|| DocError::Invalid("singular placement".into()))?;
    Plane::new(crate::apply_point(&inverse, plane.origin), crate::apply_vector(&inverse, plane.x), crate::apply_vector(&inverse, plane.y))
        .ok_or_else(|| DocError::Invalid("the sketch plane is degenerate in the source's frame".into()))
}

/// Resolve a new link to an exact placement of another component's geometry: its geometry in
/// the sketch, its 3D curves in the sketch's component frame and its source (points in the
/// source's own frame).
pub fn resolve_placed(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    placed: Placed,
    plane: &Plane,
    kind: LinkKind,
    source: &LinkSource,
    placement: &LinkPlacement,
) -> Result<(Resolved, Mat)> {
    let (source, relative) = placed_source(doc, placed, source, None, placement)?;
    let mut r = resolve(doc, vals, placed.raw, &plane_in_source(&relative, plane)?, kind, &source)?;
    map_wires(&mut r, &relative);
    Ok((r, relative))
}

/// A rigid motion of sketch coordinates: `p -> a·p + b` (`a` a rotation).
#[derive(Clone, Copy, Debug)]
struct Motion2 {
    a: [[f64; 2]; 2],
    b: Vec2,
}

impl Motion2 {
    fn point(&self, p: Vec2) -> Vec2 {
        Vec2::new(self.a[0][0] * p.x + self.a[0][1] * p.y, self.a[1][0] * p.x + self.a[1][1] * p.y) + self.b
    }

    /// The motion back (`a` is orthonormal, so its inverse is its transpose).
    fn inverse(&self) -> Motion2 {
        let a = [[self.a[0][0], self.a[1][0]], [self.a[0][1], self.a[1][1]]];
        let turned = Motion2 { a, b: Vec2::ZERO }.point(self.b);
        Motion2 { a, b: Vec2::ZERO - turned }
    }

    fn geom(&self, g: &LinkGeom) -> LinkGeom {
        let f = |p: &Vec2| self.point(*p);
        match g {
            LinkGeom::Point(p) => LinkGeom::Point(f(p)),
            LinkGeom::Line(a, b) => LinkGeom::Line(f(a), f(b)),
            LinkGeom::Circle(c, r) => LinkGeom::Circle(f(c), *r),
            LinkGeom::Arc { c, a, b } => LinkGeom::Arc { c: f(c), a: f(a), b: f(b) },
            LinkGeom::Conic { a, apex, b, rho } => LinkGeom::Conic { a: f(a), apex: f(apex), b: f(b), rho: *rho },
            LinkGeom::Ellipse { c, major, minor } => LinkGeom::Ellipse { c: f(c), major: f(major), minor: *minor },
            // Knots and weights don't change under a rigid motion.
            LinkGeom::Spline { pts, control, degree, periodic, nurbs } => {
                LinkGeom::Spline { pts: pts.iter().map(f).collect(), control: *control, degree: *degree, periodic: *periodic, nurbs: nurbs.clone() }
            }
        }
    }
}

/// How a source read with transform `seen` and now with `now` (each from its frame into the
/// sketch's component frame) has moved in sketch coordinates, when that is a rigid motion of
/// the sketch plane: a turn about the plane's normal and any shift. `None` when it has not
/// moved, or when it tilts against the plane (its projection then changes shape).
fn in_plane_motion(seen: &Mat, now: &Mat, plane: &Plane) -> Option<Motion2> {
    let coordinates = |v: Vec3| [v.x, v.y, v.z];
    let (a, [x, y]) =
        solidcraft_kernel::services::affine::projection_motion(seen, now, coordinates(plane.origin), coordinates(plane.x), coordinates(plane.y))?;
    Some(Motion2 { a, b: Vec2::new(x, y) })
}

/// One source edge of a face link: its persistent name, whether the names of the faces it
/// separates follow exact history (else the name is found again by geometry), and the
/// geometry it gives the sketch. Edges whose names differ only by numbers (a position number
/// among several edges between the same two faces, or the piece number of a split face) are
/// one family: their [`omit_name`] is the same, and the numbers may pass between them.
#[derive(Clone, Debug)]
pub struct EdgePieces {
    pub name: String,
    pub exact: bool,
    pub geom: Vec<LinkGeom>,
}

/// The name a deleted source edge is kept by in [`solidcraft_sketch::Link::omit_edges`]: its
/// family's (its name without position and piece numbers). Only a whole family is ever left
/// out, so numbers passing between its edges, or a later split of one of them, cannot leave out
/// another edge instead. A name saved in the older nested form (`F5:blend:A|B`) is read as it is
/// written now (`F5:blend(A|B)`, #5857), so a member deleted before that change stays deleted.
pub fn omit_name(name: &str) -> String {
    let name = crate::persistent_name::written_now(name);
    crate::naming::strip_edge_piece(crate::piece_names::strip_edge_number(&name))
}

/// The edges of `body` a face link leaves out: those of the families the `omit` names name. A
/// family no edge belongs to any more is an error (the link is lost; nothing else is left out
/// instead).
pub fn omitted_edges(body: &crate::ModelBody, omit: &[String]) -> Result<std::collections::BTreeSet<usize>> {
    if omit.len() > solidcraft_sketch::MAX_LINK_OMIT {
        return Err(DocError::Invalid("too many deleted projection members".into()));
    }
    let keys: Vec<String> = crate::naming::edge_names(body).iter().map(|n| omit_name(n)).collect();
    let mut skip = std::collections::BTreeSet::new();
    for name in omit {
        let key = omit_name(name);
        let hits: Vec<usize> = keys.iter().enumerate().filter(|(_, k)| **k == key).map(|(i, _)| i).collect();
        if hits.is_empty() {
            return Err(DocError::Unknown(format!("deleted projected edge `{name}` no longer exists")));
        }
        skip.extend(hits);
    }
    Ok(skip)
}

/// The source edges a face link (Project or Include of a face) is made of now, each with its
/// geometry in the sketch: what deleting some of the link's curves leaves out by identity.
/// `st` and `plane` are as [`refresh_sketch_links`] takes them; `anchor` is the sketch's face
/// attachment (an automatically projected face's edges are those of the attachment).
pub fn face_link_edges(
    doc: &Document,
    placed: Placed,
    st: &ModelState,
    plane: &Plane,
    link: &solidcraft_sketch::Link,
    anchor: Option<&crate::FaceAnchor>,
) -> Result<Vec<EdgePieces>> {
    if link.kind == LinkKind::Intersect || !matches!(link.source, LinkSource::Face { .. }) {
        return Err(DocError::Invalid("only a projected face is made of source edges".into()));
    }
    let (st, plane, source) = match &link.placement {
        None => (st, *plane, link.source.clone()),
        Some(placement) => {
            let (source, relative) = placed_source(doc, placed, &link.source, None, placement)?;
            (placed.raw, plane_in_source(&relative, plane)?, source)
        }
    };
    let LinkSource::Face { body, at } = &source else { return Err(DocError::Invalid("not a face".into())) };
    let target = st.body(body).ok_or_else(|| lost(&format!("body {body}")))?;
    if target.body.is_mesh() {
        return Err(DocError::Invalid("a mesh body's edges have no names to keep deleted members by".into()));
    }
    let indices = match anchor.filter(|_| link.auto_face) {
        Some(anchor) => doc.anchored_face_indices(st, target, &anchor.target)?.0,
        None => vec![
            match &link.face_name {
                Some(name) => crate::naming::face_on_body(target, name, *at).map(|(i, ..)| i),
                None => crate::appearance::nearest_face(target, *at).map(|(_, f)| f),
            }
            .ok_or_else(|| lost("the projected face"))?,
        ],
    };
    // The face's edges as the link resolves them, so a member is the same edge both ways.
    let mut out = Vec::new();
    resolve_face_indices(st, &plane, link.kind, body, &indices, &std::collections::BTreeSet::new(), Some(&mut out))?;
    Ok(out)
}

/// Carry a resolution's 3D wires, and their exact curves, by the rigid motion `m`. A curve the
/// motion can't carry (an intersection curve) leaves its wire known by its points alone.
fn map_wires(r: &mut Resolved, m: &Mat) {
    if crate::is_identity(m) {
        return;
    }
    for p in r.wires.iter_mut().flatten() {
        *p = crate::apply_point(m, *p);
    }
    let point = |p: Vec3| crate::apply_point(m, p);
    let vector = |v: Vec3| crate::apply_vector(m, v);
    for curves in &mut r.exact {
        *curves = curves.iter().map(|c| c.mapped(&point, &vector)).collect::<Option<Vec<_>>>().unwrap_or_default();
    }
}

/// Re-resolve every link of a sketch (in place): `st` is the model before it as its component
/// sees it, `placed` what links to an exact placement read, `anchor` the face the sketch is
/// attached to, `frame` as [`resolve_at`] takes it. Returns warnings for lost, moved or rebuilt
/// links; lost links keep their last geometry, and frozen ones are left as they are. A link kept
/// by a face or edge name that finds nothing fails the sketch ([`LostNames::Fail`]).
#[allow(clippy::too_many_arguments)]
pub fn refresh_sketch_links(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    placed: Placed,
    st: &ModelState,
    plane: &Plane,
    sk: &mut Sketch,
    anchor: Option<&crate::FaceAnchor>,
    frame: Option<&(u64, crate::Mat)>,
) -> Result<Vec<String>> {
    refresh_sketch_links_with(doc, vals, placed, st, plane, sk, anchor, LostNames::Fail, frame)
}

/// [`refresh_sketch_links`] with what a lost face or edge name does (`lost_names`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn refresh_sketch_links_with(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    placed: Placed,
    st: &ModelState,
    plane: &Plane,
    sk: &mut Sketch,
    anchor: Option<&crate::FaceAnchor>,
    lost_names: LostNames,
    frame: Option<&(u64, crate::Mat)>,
) -> Result<Vec<String>> {
    let mut warn = Vec::new();
    let mut lost_by_name = Vec::new();
    let links = sk.links.clone();
    let sketch_plane = plane;
    for l in &links {
        if l.frozen {
            continue;
        }
        // A link carrying the source-sketch frames captured when it was made (#17980) is read as
        // those frames say: the sketch stays where it was relative to the author through
        // assembly motion. Otherwise a link with a placement reads its source at that placement.
        let (framed, plane, relative, mut source) = match l.placement.as_ref().filter(|_| l.sketch_placements.is_empty()) {
            None => (state_for_link(doc, st, l, frame)?, *sketch_plane, None, l.source.clone()),
            Some(placement) => match placed_source(doc, placed, &l.source, l.edge_name.as_deref(), placement)
                .and_then(|(source, relative)| Ok((plane_in_source(&relative, sketch_plane)?, relative, source)))
            {
                Ok((plane, relative, source)) => (std::borrow::Cow::Borrowed(placed.raw), plane, Some(relative), source),
                Err(e) => {
                    sk.set_link_lost(&l.id, true);
                    warn.push(format!("projected reference lost: {e}; last geometry retained"));
                    continue;
                }
            },
        };
        let st = framed.as_ref();
        let plane = &plane;
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
                lost_by_name.push(lose_named(doc, sk, l, name, lost_names, &mut warn));
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
                lost_by_name.push(lose_named(doc, sk, l, name, lost_names, &mut warn));
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
                lost_by_name.push(lose_named(doc, sk, l, name, lost_names, &mut warn));
                continue;
            };
            warn.extend(hit.note(name).map(|note| format!("projected {note}")));
            named_vertex = Some((body, hit.vertex, hit.point));
        }
        // Members deleted from a face link are left out by the persistent names of their source
        // edges. A name no longer found makes the link lost: nothing else is left out instead.
        let skip = if l.omit_edges.is_empty() {
            std::collections::BTreeSet::new()
        } else {
            let body = match (&source, anchor.filter(|_| l.auto_face)) {
                (_, Some(anchor)) => anchor.target.body.clone(),
                (LinkSource::Face { body, .. }, None) => body.clone(),
                _ => String::new(),
            };
            match st.body(&body).ok_or_else(|| lost("the projected face's body")).and_then(|b| omitted_edges(b, &l.omit_edges)) {
                Ok(skip) => skip,
                Err(e) => {
                    sk.set_link_lost(&l.id, true);
                    warn.push(format!("projected reference lost: {e}; last geometry retained (repair or remove the projection)"));
                    continue;
                }
            }
        };
        let resolved = if matches!(source, LinkSource::Text { .. }) {
            // A text needs nothing outside its sketch; one on a path follows a curve of it. One
            // whose font this computer lacks is set in another, and the sketch says so (#19230).
            sk.saved_text_source_geometry(&source).map_err(DocError::from).and_then(|(geom, note)| {
                if geom.is_empty() {
                    return Err(DocError::Invalid(format!("the {} makes no outline", source.describe())));
                }
                warn.extend(note.map(|note| format!("{}: {note}", source.describe())));
                Ok(Resolved {
                    geom,
                    wires: Vec::new(),
                    exact: Vec::new(),
                    source: source.clone(),
                    moved: false,
                    face_index: None,
                    edge_index: None,
                    vertex_index: None,
                    edge_names: Vec::new(),
                })
            })
        } else if l.auto_face {
            anchor
                .filter(|anchor| {
                    l.face_name.as_ref() == Some(&anchor.target.name) && matches!(&l.source,LinkSource::Face {body,..} if *body==anchor.target.body)
                })
                .ok_or_else(|| lost("exact auto-projected attachment"))
                .and_then(|anchor| resolve_anchored_face_omitting(doc, st, plane, l.kind, anchor, &skip))
        } else if let Some((body, index, at)) = named_face {
            resolve_named_face(st, plane, l.kind, &body, index, at, &skip)
        } else if let Some((body, index, at)) = named_edge {
            resolve_named_edge(st, plane, l.kind, &body, index, at)
        } else if let Some((body, index, at)) = named_vertex {
            resolve_named_vertex(plane, &body, index, at)
        } else {
            resolve_omitting(doc, vals, st, plane, l.kind, &source, frame, &skip)
        };
        match resolved {
            Ok(mut r) => {
                if let Some(relative) = &relative {
                    map_wires(&mut r, relative);
                }
                // An edge or vertex link saved without a name takes the name of the geometry it was
                // found on. A sketch that is edited stores it; evaluation leaves the design as it is.
                let new_edge_name = l.edge_name.is_none().then(|| resolved_edge_name(st, &r)).flatten();
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
                let mut geom = r.geom.clone();
                // A source that turned or moved within the sketch plane since the link was read
                // is matched where it was, then moved: its curves keep their ids.
                let motion = relative.as_ref().and_then(|now| {
                    let seen = l.placement.as_ref()?.seen.filter(is_rigid)?;
                    in_plane_motion(&seen, now, sketch_plane)
                });
                if let Some(m) = &motion {
                    let back = m.inverse();
                    geom = geom.iter().map(|g| back.geom(g)).collect();
                }
                // Match a face by its saved model-edge names while retaining the occurrence
                // motion and the exact list of constraints a rebuild removed.
                let updated = if l.kind == LinkKind::Project {
                    sk.update_link_named_detail(&l.id, &geom, &r.edge_names)
                } else {
                    sk.update_link_detail(&l.id, &geom)
                };
                // A name captured from a point on this rebuild did not identify
                // the saved curve. Publish it only after matching the old link.
                if let Some(name) = new_edge_name
                    && let Some(link) = sk.links.iter_mut().find(|link| link.id == l.id)
                {
                    link.edge_name = Some(name);
                }
                match updated {
                    Ok(update) => {
                        if let Some(m) = &motion {
                            for i in sk.link_points(&l.id) {
                                if let Some(p) = sk.points.get_mut(i) {
                                    p.pos = m.point(p.pos);
                                }
                            }
                        }
                        if let (Some(now), Some(link)) = (relative, sk.links.iter_mut().find(|link| link.id == l.id))
                            && let Some(placement) = link.placement.as_mut()
                        {
                            placement.seen = Some(now);
                        }
                        if update.rebuilt && !update.dropped.is_empty() {
                            warn.push(format!(
                                "projected {} changed shape; removed {} on geometry that is gone ({})",
                                l.source.describe(),
                                if update.dropped.len() == 1 { "1 constraint".to_string() } else { format!("{} constraints", update.dropped.len()) },
                                update.dropped.join(", ")
                            ));
                        } else if r.moved {
                            warn.push(format!("projected {} was re-found on the nearest geometry", l.source.describe()));
                        }
                    }
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
    if lost_names == LostNames::Fail && !lost_by_name.is_empty() {
        return Err(DocError::LostProjections { feature: String::new(), links: lost_by_name });
    }
    warn.extend(refresh_wire_targets(doc, vals, st, plane, sk, frame));
    Ok(warn)
}

/// A link whose face or edge `name` finds nothing, keeping its geometry. The sketch fails naming
/// every such link; or, built to be shown, the link is marked lost and gets a warning.
fn lose_named(doc: &Document, sk: &mut Sketch, l: &Link, name: &str, lost_names: LostNames, warn: &mut Vec<String>) -> crate::LostProjection {
    let what = match l.kind {
        LinkKind::Intersect => format!("intersected {}", l.source.describe()),
        LinkKind::Include => format!("included {}", l.source.describe()),
        _ => format!("projected {}", l.source.describe()),
    };
    let lost = crate::LostProjection {
        link: l.id.clone(),
        what,
        curves: sk.link_curves(&l.id).into_iter().filter_map(|i| sk.curves.get(i)).map(|c| c.id.clone()).collect(),
        points: sk.link_points(&l.id).into_iter().filter_map(|i| sk.points.get(i)).map(|p| p.id.clone()).collect(),
        name: name.to_string(),
        from: features_named_in(doc, name),
    };
    if lost_names == LostNames::Mark {
        sk.set_link_lost(&l.id, true);
        warn.push(lost.warning());
    }
    lost
}

/// The names of the design's features that a face, edge or vertex name `name` refers to, each
/// once, in timeline order. Nothing for a name that does not read, or that names no feature
/// (`Body1:face:3`); a feature no longer in the design is left out.
fn features_named_in(doc: &Document, name: &str) -> Vec<String> {
    use crate::persistent_name::EdgeName;
    let Ok(read) = EdgeName::parse(name) else { return Vec::new() };
    let ids: std::collections::BTreeSet<u64> = read.faces.iter().flat_map(|f| f.features()).map(|f| f.0).collect();
    doc.features.iter().filter(|f| ids.contains(&f.id)).map(|f| f.name.clone()).collect()
}

/// Where the fixed geometry the sketch's 3D constraints are held to is now (#10616): the sketch's
/// own plane, through which a 3D point is held to a 2D point or curve, and each model plane a 3D
/// constraint is held to. A named plane is resolved like any plane reference; a face by its
/// persistent face name, then its exact plane (`Body::planar_face`). A model plane that cannot be
/// resolved stays unresolved, its constraint does not solve, and a warning says why.
pub fn refresh_wire_targets(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    plane: &Plane,
    sk: &mut Sketch,
    frame: Option<&(u64, crate::Mat)>,
) -> Vec<String> {
    sk.frame = solidcraft_sketch::SketchFrame(Some(*plane));
    let mut warn = Vec::new();
    for c in &mut sk.wire_constraints {
        let id = c.id.clone();
        let Some(solidcraft_sketch::PlaneOf::Model(m)) = c.kind.plane_mut() else { continue };
        if let Err(why) = resolve_model_plane(doc, vals, st, m, frame) {
            warn.push(format!("3D constraint {id}: {why}"));
        }
    }
    warn
}

/// Resolve a model plane a 3D constraint is held to (see [`refresh_wire_targets`]); an error
/// says why it could not be, and leaves it unresolved.
pub fn resolve_model_plane(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    m: &mut solidcraft_sketch::ModelPlane,
    frame: Option<&(u64, crate::Mat)>,
) -> std::result::Result<(), String> {
    {
        let (source, face_name) = (m.source.clone(), m.face_name.clone());
        let mut resolved_face = None;
        let resolved = match &source {
            LinkSource::Plane { name } => {
                let pr = if Plane::named(name.as_str()).is_some() {
                    PlaneRef::Origin { name: name.clone() }
                } else {
                    let feature = doc.find_feature(name.as_str()).map(|f| crate::FeatureId(f.id)).unwrap_or(crate::FeatureId::DANGLING);
                    PlaneRef::Construction { feature, placement: None }
                };
                let owner = match &pr {
                    PlaneRef::Construction { feature, .. } => doc.feature(feature.0).map(|f| f.component).unwrap_or(0),
                    _ => 0,
                };
                doc.resolve_plane_in(vals, &pr, 0, owner, Some(st))
                    .and_then(|plane| plane_in_sketch_frame(doc, &pr, plane, st, frame))
                    .map_err(|e| format!("the plane {name} cannot be resolved: {e}"))
            }
            LinkSource::Face { body, at } => match &face_name {
                Some(name) => st
                    .body(body.as_str())
                    .and_then(|target| {
                        let (index, _) = crate::naming::face_on_body(target, name, *at)?;
                        let plane = target.body.planar_face(index)?;
                        resolved_face = Some(index);
                        Some(plane)
                    })
                    .ok_or_else(|| format!("the planar face {name} of {body} no longer exists")),
                None => Err(format!("the face of {body} has no name to find it by")),
            },
            other => Err(format!("{} is not a plane", other.describe())),
        };
        m.resolved = resolved.as_ref().ok().copied();
        m.resolved_face = resolved_face;
        resolved.map(|_| ())
    }
}

/// The planar face of the model a 3D constraint is held to, picked through `at` (#10616): of
/// the bodies (`body`, or all), the planar faces whose exact trimmed surface contains `at`.
/// The kernel checks the B-rep's surface and trim curves, then compares the support planes:
/// every face found must lie on one plane, else the pick is refused (a point on an edge between
/// two planar faces). Returns the body, the face's persistent name and its plane.
pub fn pick_planar_face(st: &ModelState, body: Option<&str>, at: Vec3) -> std::result::Result<(String, String, Plane), String> {
    let mut found: Vec<(String, usize, Plane, &crate::ModelBody)> = Vec::new();
    for b in st.bodies.iter().filter(|b| body.is_none_or(|n| b.name == n) && !b.body.is_mesh()) {
        let tol = (b.body.size() * 1e-9).max(1e-6);
        let query = solidcraft_kernel::services::query::Query::new(b.body.record());
        for (_, i) in query.faces_at(at, tol) {
            if let Some(p) = b.body.planar_face(i) {
                found.push((b.name.clone(), i, p, b));
            }
        }
    }
    let Some((name, index, plane, b)) = found.first() else {
        return Err("no planar face of the model passes through that point".into());
    };
    if !found.iter().all(|(_, _, p, _)| solidcraft_kernel::services::constraint::same_plane(p, plane)) {
        return Err("that point lies where faces on different planes meet: pick a point inside one planar face".into());
    }
    let face = crate::naming::face_names(b).get(*index).cloned().ok_or_else(|| "that face has no name to keep it by".to_string())?;
    Ok((name.clone(), face, *plane))
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
            let Some((qa, qb)) = solidcraft_kernel::curves::pcurve_eval(&used.pcurve, a).zip(solidcraft_kernel::curves::pcurve_eval(&used.pcurve, b))
            else {
                return false;
            };
            let above_a = qa[1] > v;
            if above_a == (qb[1] > v) {
                continue;
            }
            let (mut lo, mut hi) = (a, b);
            for _ in 0..80 {
                let mid = lo.midpoint(hi);
                let Some(q) = solidcraft_kernel::curves::pcurve_eval(&used.pcurve, mid) else { return false };
                if (q[1] > v) == above_a {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let Some(q) = solidcraft_kernel::curves::pcurve_eval(&used.pcurve, lo.midpoint(hi)).filter(|q| q[0].is_finite()) else { return false };
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

fn project_sketch_curve(sk: &Sketch, from: &Plane, ci: usize, to: &Plane) -> Result<Vec<LinkGeom>> {
    let map = |p: Vec2| to.to_local(from.to_world(p));
    let sampled = || {
        let pts: Vec<Vec3> = sk.polyline(ci).iter().map(|p| from.to_world(*p)).collect();
        fit_world(to, &pts)
    };
    let relation = solidcraft_kernel::services::conic_projection::plane_relation(from, to);
    let parallel = relation.parallel;
    let edge_on = relation.edge_on;
    let flip = relation.flip;
    Ok(match sk.curves.get(ci).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => match (sk.point(*a), sk.point(*b)) {
            (Some(a), Some(b)) => {
                let (a, b) = (map(a), map(b));
                if a.dist(b) > 1e-7 { vec![LinkGeom::Line(a, b)] } else { vec![LinkGeom::Point(a)] }
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
        Some(CurveKind::Spline { pts, control: false, periodic, nurbs: None, handles, .. }) if !handles.is_empty() => {
            let source: Vec<Vec2> =
                pts.iter().map(|p| sk.point(*p).ok_or_else(|| DocError::Invalid("missing source spline point".into()))).collect::<Result<_>>()?;
            let tangents: Vec<Vec2> = handles
                .iter()
                .map(|p| sk.point(*p).ok_or_else(|| DocError::Invalid("missing source spline handle".into())))
                .collect::<Result<_>>()?;
            let pieces = solidcraft_sketch::fit_beziers_with_handles(&source, &tangents, *periodic).map_err(|e| DocError::Invalid(e.to_string()))?;
            projection_links(
                solidcraft_kernel::services::conic_projection::project_beziers(from, to, &pieces, edge_on, solidcraft_sketch::MAX_COORD)
                    .map_err(|e| DocError::Invalid(e.to_string()))?,
            )?
        }
        Some(CurveKind::Spline { pts, control, degree, periodic, nurbs, .. }) => {
            let source: Vec<Vec2> =
                pts.iter().map(|p| sk.point(*p).ok_or_else(|| DocError::Invalid("missing source spline point".into()))).collect::<Result<_>>()?;
            if *control {
                let metadata = nurbs.as_ref().map(|n| (n.knots.as_slice(), n.weights.as_slice()));
                projection_links(
                    solidcraft_kernel::services::conic_projection::project_control(
                        from,
                        to,
                        &source,
                        usize::from(*degree),
                        metadata,
                        edge_on,
                        solidcraft_sketch::MAX_COORD,
                    )
                    .map_err(|e| DocError::Invalid(e.to_string()))?,
                )?
            } else if parallel {
                vec![LinkGeom::Spline {
                    pts: solidcraft_kernel::services::conic_projection::project_points(from, to, &source),
                    control: false,
                    degree: *degree,
                    periodic: *periodic,
                    nurbs: None,
                }]
            } else {
                projection_links(
                    solidcraft_kernel::services::conic_projection::project_fit(
                        from,
                        to,
                        &source,
                        *periodic,
                        solidcraft_sketch::MAX_SPLINE_POINTS,
                        edge_on,
                        solidcraft_sketch::MAX_COORD,
                    )
                    .map_err(|e| DocError::Invalid(e.to_string()))?,
                )?
            }
        }
        // A circle or arc seen edge on is the line it covers, out to its own extremes: samples of
        // it fall short of them, by up to its chord tolerance (#17482).
        Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }) if edge_on => {
            match solidcraft_kernel::services::conic_projection::project_arcs(from, to, &sk.segs(ci), solidcraft_sketch::MAX_COORD)
                .map_err(|e| DocError::Invalid(e.to_string()))?
            {
                Some(geometry) => projection_links(geometry)?,
                None => sampled(),
            }
        }
        Some(CurveKind::Conic { a, b, apex, rho }) if parallel => match (sk.point(*a), sk.point(*apex), sk.point(*b)) {
            (Some(a), Some(x), Some(b)) => vec![LinkGeom::Conic { a: map(a), apex: map(x), b: map(b), rho: *rho }],
            _ => Vec::new(),
        },
        Some(_) => sampled(),
        None => Vec::new(),
    })
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

/// The plane `pr` names, in the frame the sketch is in. A construction plane of another
/// component is in that component's own frame: brought into the sketch's, the trace of the
/// two planes is where they meet, and not where the plane's own frame has it (#17534).
fn plane_in_sketch_frame(doc: &Document, pr: &PlaneRef, plane: Plane, st: &ModelState, frame: Option<&(u64, crate::Mat)>) -> Result<Plane> {
    let PlaneRef::Construction { feature, .. } = pr else { return Ok(plane) };
    // The sketch's own frame: the component it is authored in and where that component stands.
    let Some((author, author_frame)) = frame else { return Ok(plane) };
    if doc.occurrences.is_empty() {
        return Ok(plane);
    }
    // The component the construction plane belongs to: the frame its geometry is in.
    let owner = doc.features.iter().find(|f| f.id == feature.0).map(|f| f.component).unwrap_or(*author);
    if owner == *author {
        return Ok(plane);
    }
    // The plane as it was built, in its own component's frame.
    let built = match st.construct.iter().find(|c| c.feature == feature.0).map(|c| &c.geom) {
        Some(crate::construct::ConstructGeom::Plane(built)) => built,
        // The uncached resolver also returns the construction plane in its owner's frame.
        _ => &plane,
    };
    let inverse = crate::mat_inverse(author_frame).ok_or_else(|| DocError::Invalid("the sketch's frame cannot be inverted".into()))?;
    let m = crate::mat_mul(&inverse, &doc.component_transform(owner));
    crate::eval::placed_plane(&m, built)
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

/// Whole-body outlines already traced, newest first. Each is kept with what it was traced from,
/// which is all it depends on ([`exact_silhouette_traced`]): the body (kept, so its shared data
/// stays alive and no other body is taken for it), its record as written (JSON, which tells
/// `-0.0` from `0.0`), its size, whether it is a mesh, and the sketch plane as written. A re-evaluation that makes the
/// very same body ([`solidcraft_kernel::Body::same_shape`]), or one with the same record and
/// size (recomputing the timeline, opening the design again), reuses the outline.
struct TracedOutline {
    body: solidcraft_kernel::Body,
    record: String,
    size: u64,
    plane: String,
    outline: Option<Vec<LinkGeom>>,
}
static OUTLINES: std::sync::Mutex<Vec<TracedOutline>> = std::sync::Mutex::new(Vec::new());
const OUTLINES_KEPT: usize = 16;
/// Records written longer than this (bytes) are not kept: comparing them would cost more than
/// it saves, and they would hold too much memory.
const OUTLINE_RECORD_MAX: usize = 4 << 20;

/// [`exact_silhouette_traced`], kept for each body and plane ([`OUTLINES`]).
fn exact_silhouette(body: &crate::ModelBody, plane: &Plane) -> Option<Vec<LinkGeom>> {
    // A mesh body has no exact outline, whatever its record.
    let plane_text = format!("{plane:?}|{}", body.body.is_mesh());
    let size = body.body.size().to_bits();
    let mut record: Option<Option<String>> = None;
    let mut written =
        || record.get_or_insert_with(|| serde_json::to_string(body.body.record()).ok().filter(|r| r.len() <= OUTLINE_RECORD_MAX)).clone();
    if let Ok(kept) = OUTLINES.lock() {
        let mut hit = kept.iter().filter(|k| k.plane == plane_text && k.size == size).find(|k| k.body.same_shape(&body.body));
        if hit.is_none() && kept.iter().any(|k| k.plane == plane_text && k.size == size) {
            let now = written();
            hit = kept.iter().find(|k| k.plane == plane_text && k.size == size && now.as_ref() == Some(&k.record));
        }
        if let Some(k) = hit {
            return k.outline.clone();
        }
    }
    let outline = exact_silhouette_traced(body, plane);
    if let Some(written) = written()
        && let Ok(mut kept) = OUTLINES.lock()
    {
        kept.insert(0, TracedOutline { body: body.body.clone(), record: written, size, plane: plane_text, outline: outline.clone() });
        kept.truncate(OUTLINES_KEPT);
    }
    outline
}

/// Whether face `f` of `rec` is a straight extrusion along `n`: a B-spline or NURBS surface of
/// degree 1 one way with two rows of controls, each control of the second row its partner of the
/// first moved by one vector parallel to `n` (with the same weight). Seen along `n`, such a face
/// is edge-on everywhere and shows only its edges.
fn extruded_along(rec: &solidcore_record::brep::Brep, f: usize, n: Vec3) -> bool {
    use solidcore_record::brep::Surface;
    let Some(face) = rec.faces.get(f) else { return false };
    // Each control as a point and its weight, by rows along u.
    let (u_knots, v_knots, rows): (&[f64], &[f64], Vec<Vec<(Vec3, f64)>>) = match &face.surface {
        Surface::Spline { u_knots, v_knots, points } => {
            (u_knots, v_knots, points.iter().map(|row| row.iter().map(|p| (Vec3::new(p[0], p[1], p[2]), 1.0)).collect()).collect())
        }
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => (
            u_knots,
            v_knots,
            homogeneous
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|h| if h[3] > 0.0 { (Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]), h[3]) } else { (Vec3::ZERO, 0.0) })
                        .collect()
                })
                .collect(),
        ),
        _ => return false,
    };
    if rows.iter().flatten().any(|(p, w)| !(*w > 0.0 && p.is_finite())) {
        return false;
    }
    // The pairs a straight extrusion moves by one vector: the two rows along u (degree 1 in u:
    // four knots), or the two controls of each row (degree 1 in v).
    let pairs: Vec<((Vec3, f64), (Vec3, f64))> = match rows.as_slice() {
        [first, second] if u_knots.len() == 4 && first.len() == second.len() => first.iter().copied().zip(second.iter().copied()).collect(),
        _ if v_knots.len() == 4 && rows.iter().all(|row| row.len() == 2) => {
            rows.iter().filter_map(|row| Some((*row.first()?, *row.get(1)?))).collect()
        }
        _ => return false,
    };
    let Some(&((p0, _), (q0, _))) = pairs.first() else { return false };
    let step = q0 - p0;
    let scale = pairs.iter().fold(step.len(), |s, ((p, _), (q, _))| s.max(p.len()).max(q.len())).max(1.0);
    let tol = 1e-9 * scale;
    let Some(dir) = step.normalized() else { return false };
    dir.cross(n).len() <= 1e-9 && pairs.iter().all(|((p, wp), (q, wq))| ((*q - *p) - step).len() <= tol && (wp - wq).abs() <= 1e-12 * wp.max(*wq))
}

/// The outline of a solid seen along the sketch normal, from its exact faces and edges: the
/// stretches of its edges between a face that faces the sketch and one that does not (and of an
/// open sheet's border where its face faces the sketch; the kernel's
/// [`Query::outline_stretches`](solidcraft_kernel::services::query::Query::outline_stretches)), each projected exactly as an edge link
/// projects it ([`exact_edges`], or the kernel's projection of the stretch), and the silhouettes of
/// its curved faces as the drawing's hidden lines trace them ([`hlr::silhouettes`]). A face faces
/// the sketch where its normal out of the material leans toward it; edge-on is away, as in
/// [`silhouette`]. A circle seen at a slant is an ellipse: its stretches come out as exact
/// elliptical arcs, not lines and circular arcs fitted to the mesh (which bulged the wrong way and
/// ran to hundreds of curves on a sketch at 45 degrees).
///
/// `None` for a mesh body, a body hidden lines leave out (a freeform face that is not a straight
/// extrusion along the normal, [`extruded_along`]; a trim they cannot read), or an edge or face
/// the kernel cannot read exactly here: the mesh's outline is traced
/// then ([`silhouette`]).
///
/// [`hlr::silhouettes`]: solidcraft_kernel::hlr::silhouettes
fn exact_silhouette_traced(body: &crate::ModelBody, plane: &Plane) -> Option<Vec<LinkGeom>> {
    use solidcraft_kernel::hlr;
    if body.body.is_mesh() {
        return None;
    }
    let rec = body.body.record();
    let n = plane.normal();
    let size = body.body.size().max(1e-9);
    let q = solidcraft_kernel::services::query::Query::new(rec);
    let mut edges = Vec::new();
    // The stretches of the edges on the outline, each projected as an edge link projects it.
    for stretch in q.outline_stretches(n, size).ok()? {
        let e = stretch.edge;
        let edge = rec.edges.get(e)?;
        let geom = if stretch.whole {
            let mut g = exact_edges(body, plane, &[e]).ok()?.pop().flatten()?;
            at_vertices(body, plane, e, &mut g);
            g
        } else {
            let pieces = solidcraft_kernel::services::spline_projection::project_curve(
                edge.parameters(),
                stretch.range,
                true,
                stretch.ends,
                plane,
                body.body.planar_edges_tolerance(),
                &mut 262_144,
            )?;
            let classified = solidcraft_kernel::services::conic_projection::classify_pieces(
                plane,
                &pieces,
                solidcraft_sketch::MAX_SPLINE_POINTS,
                solidcraft_sketch::MAX_COORD,
            )
            .ok()?;
            projection_links(classified).ok()?
        };
        // Tidied by the kernel: an edge seen end on is a point of the outline, not a curve of it.
        edges.extend(solidcraft_kernel::services::outline::tidy_stretch(geom.into_iter().map(outline_piece).collect()));
    }
    // The silhouettes of the curved faces, traced where they lie inside them.
    // Prepared once per body: every sketch that projects it, on any plane, shares it.
    let def = body.body.outline_prepared()?;
    let opts = hlr::Options { trim_samples: solidcraft_kernel::OUTLINE_TRIM_SAMPLES, threads: 1, ..hlr::Options::default() };
    // A free-form face hidden lines leave out is no matter when it is a straight extrusion along
    // the sketch normal (the walls of an extruded letter or spline profile): seen edge-on all
    // over, its outline is its edges, which the stretches above already hold. Falling back to the
    // mesh for it fitted arcs that missed each other by thousandths of a millimetre, so a
    // letter's outline made no profile and could not be offset (#14987).
    let freeform_matters = def.report.freeform_faces.iter().any(|f| !extruded_along(rec, *f, n));
    if freeform_matters || !def.report.unsupported_trims.is_empty() {
        return None;
    }
    let view = hlr::View { x: plane.x, y: plane.y, d: n };
    let placed = hlr::Placement { body: 0, rotation: [Vec3::X, Vec3::Y, Vec3::Z], translation: Vec3::ZERO - plane.origin };
    let mut sils = Vec::new();
    for s in hlr::silhouettes(def, &placed, &view, &opts).ok()? {
        match s.accuracy {
            solidcraft_geom::section::CurveAccuracy::Analytic => {
                let pieces: Vec<LinkGeom> = s.geometry.iter().map(LinkGeom::from_seg).collect::<Option<_>>()?;
                sils.extend(solidcraft_kernel::services::outline::tidy_silhouette(pieces.into_iter().map(outline_piece).collect()));
            }
            _ => {
                let pts: Vec<Vec2> = s.points.iter().map(|p| Vec2::new(p[0], p[1])).collect();
                // A traced silhouette along an edge already in the outline (a round's end, where
                // the view grazes the round) is that edge.
                if pts.iter().all(|p| edges.iter().any(|g| solidcraft_kernel::services::outline::near(g, *p, size * 1e-6))) {
                    continue;
                }
                let tol = size * 1e-5;
                let mut fitted = Vec::new();
                match as_spline(&pts, tol) {
                    Some(sp) => fitted.push(sp),
                    None => fit_chain(&pts, Some(tol), &mut fitted),
                }
                sils.extend(fitted.into_iter().map(outline_piece));
            }
        }
    }
    // A silhouette ends where it meets an edge, on the face's trim: within its chords of the
    // edge's own end there. Edges come first, so their exact ends are the ones kept.
    let mut out = edges;
    out.extend(sils);
    let out = solidcraft_kernel::services::outline::meet_at_shared_ends(out, 8.0 * def.report.max_trim_chord + size * 1e-7);
    Some(solidcraft_kernel::services::outline::drop_coincident(out, (size * 1e-9).max(1e-7)).into_iter().map(link_of_piece).collect())
}

/// The ends of `g` (none for a closed curve or a point).
fn piece_ends(g: &LinkGeom) -> Option<[Vec2; 2]> {
    match g {
        LinkGeom::Line(a, b) | LinkGeom::Arc { a, b, .. } | LinkGeom::Conic { a, b, .. } => Some([*a, *b]),
        LinkGeom::Spline { pts, periodic: false, .. } => Some([*pts.first()?, *pts.last()?]),
        _ => None,
    }
}

/// `geom`, the projected edges of curved face `face` of `body`, closed with the face's exact
/// silhouettes where its edges leave the outline open: a cylinder's side seen across its axis
/// projects its rims as two lines, and the silhouettes are the sides between them (#21295). Only
/// a silhouette both of whose ends close open ends of `geom` is taken: the kernel traces one
/// silhouette for all the faces on a surface, and one that lies on a neighbouring face meets
/// this face's edges nowhere. `geom` as it is when it has no open end, the face is planar or
/// meshed, or the kernel cannot trace its silhouette exactly.
fn closed_by_silhouettes(body: &crate::ModelBody, plane: &Plane, face: usize, geom: Vec<LinkGeom>) -> Vec<LinkGeom> {
    use solidcraft_kernel::hlr;
    let size = body.body.size().max(1e-9);
    let tol = size * 1e-6;
    let ends: Vec<Vec2> = geom.iter().filter_map(piece_ends).flatten().collect();
    let open: Vec<Vec2> = ends.iter().copied().filter(|p| ends.iter().filter(|q| q.dist(*p) <= tol).count() == 1).collect();
    if open.is_empty() || body.body.is_mesh() || body.body.planar_face(face).is_some() {
        return geom;
    }
    let Some(def) = body.body.outline_prepared() else { return geom };
    let opts = hlr::Options { trim_samples: solidcraft_kernel::OUTLINE_TRIM_SAMPLES, threads: 1, ..hlr::Options::default() };
    let view = hlr::View { x: plane.x, y: plane.y, d: plane.normal() };
    let placed = hlr::Placement { body: 0, rotation: [Vec3::X, Vec3::Y, Vec3::Z], translation: Vec3::ZERO - plane.origin };
    let Ok(silhouettes) = hlr::silhouettes(def, &placed, &view, &opts) else { return geom };
    let closes = |p: Vec2| open.iter().any(|q| q.dist(p) <= tol);
    let mut sides = Vec::new();
    for s in silhouettes {
        if s.face != face || !matches!(s.accuracy, solidcraft_geom::section::CurveAccuracy::Analytic) {
            continue;
        }
        let Some(pieces) = s.geometry.iter().map(LinkGeom::from_seg).collect::<Option<Vec<_>>>() else { continue };
        let pieces = solidcraft_kernel::services::outline::tidy_silhouette(pieces.into_iter().map(outline_piece).collect());
        let pieces: Vec<LinkGeom> = pieces.into_iter().map(link_of_piece).collect();
        let (Some(first), Some(last)) = (pieces.first().and_then(piece_ends), pieces.last().and_then(piece_ends)) else { continue };
        if closes(first[0]) && closes(last[1]) {
            sides.extend(pieces);
        }
    }
    if sides.is_empty() {
        return geom;
    }
    let mut out: Vec<_> = geom.into_iter().map(outline_piece).collect();
    out.extend(sides.into_iter().map(outline_piece));
    let out = solidcraft_kernel::services::outline::meet_at_shared_ends(out, tol);
    solidcraft_kernel::services::outline::drop_coincident(out, (size * 1e-9).max(1e-7)).into_iter().map(link_of_piece).collect()
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

/// The section of face `face` of `body` by the sketch plane, exactly, when the face's surface is
/// one of revolution (a cylinder, cone, sphere or torus) and the plane holds its axis: lines
/// and circles from the kernel, trimmed at the face's edges ([`solidcraft_kernel::axial_face_section`],
/// #17496, #17498, #17499, #17500). `None` for any other face or plane, which [`section`] cuts.
fn axial_section(body: &crate::ModelBody, plane: &Plane, face: u32) -> Result<Option<Vec<LinkGeom>>> {
    use solidcraft_kernel::AxialPiece;
    let Some(pieces) = solidcraft_kernel::axial_face_section(&body.body, face as usize, plane).map_err(|e| DocError::Invalid(e.to_string()))? else {
        return Ok(None);
    };
    let local = |[x, y, z]: [f64; 3]| plane.to_local(Vec3::new(x, y, z));
    Ok(Some(
        pieces
            .into_iter()
            .map(|piece| match piece {
                AxialPiece::Line { start, end } => LinkGeom::Line(local(start), local(end)),
                AxialPiece::Arc { centre, start, end } => LinkGeom::Arc { c: local(centre), a: local(start), b: local(end) },
                AxialPiece::Circle { centre, radius } => LinkGeom::Circle(local(centre), radius),
            })
            .collect(),
    ))
}

/// Section of a mesh (or one of its faces) with the sketch plane.
/// Whether `b` is a mesh body without shading normals of its own (as an STL has none): its
/// display mesh is shaded flat, each triangle with its own normal.
fn shaded_flat(b: &crate::ModelBody) -> bool {
    b.body.triangle_corner_normals().is_some_and(<[_]>::is_empty)
}

/// How far two neighbouring facets of a mesh body shaded flat may turn and still be one smooth
/// surface to its sections: a coarsely tessellated round turns up to about 35 degrees from one
/// facet to the next, a chamfer 45.
const SMOOTH_TURN_DEGREES: f64 = 40.0;

/// How far an STL exported for CAD strays from its part, as a share of its size: SolidCraft's
/// own standard export's chord (`solidcraft_io::MeshQuality::Standard`).
const STL_CHORD: f64 = 2e-4;

/// `shaded_flat`: `m` is the display mesh of a mesh body without shading normals of its own
/// ([`shaded_flat`]), whose section takes the surface's normals from its facets.
pub(crate) fn section(plane: &Plane, m: &Mesh, face: Option<u32>, shaded_flat: bool) -> Vec<LinkGeom> {
    let size = m.bounds().diagonal().max(1e-9);
    let (segs, exact, sag) = section_segments(plane, m, face, shaded_flat);
    let segs: Vec<(Vec2, Vec2)> = segs.into_iter().map(|(a, b, _)| (a, b)).collect();
    let refine = |v: Vec<LinkGeom>| -> Vec<LinkGeom> { refine_rounds(v, &exact, size) };
    let lossy = refine(fit_segments_within(&segs, size, false, size * 2e-3, shaded_flat));
    // Lines and arcs that follow the section closely (the corners of a rounded profile stay
    // where they are) are kept when they take no more curves than the loose fit. Closely: to a
    // hundred-thousandth of the size, where a display mesh's normals are its surface's. A mesh
    // body shaded flat only gives the surface its facets approximate, which the crossings taken
    // from them can miss by up to the facets' sag: as far as an STL exported for CAD strays from
    // its part, at most STL_CHORD of its size. Past that (a coarser or a noisy mesh, whose
    // normals make its sag look larger) the close fit would bend straight walls into wide arcs,
    // and the loose fit is the one to stand.
    let near = if shaded_flat { sag.min(size * STL_CHORD).max(size * 1e-5) } else { size * 1e-5 };
    let close = refine(fit_segments_within(&segs, size, true, near, shaded_flat));
    let plain = |v: &[LinkGeom]| v.iter().all(|g| matches!(g, LinkGeom::Line(..) | LinkGeom::Arc { .. } | LinkGeom::Circle(..)));
    // A loose fit with fewer curves can still be wrong: short faces beside a small round (2 mm
    // walls each side of an R2 corner) fit within its tolerance as one larger arc. The exact
    // crossings (on planes and rulings) show it: one of them off every curve of the fit.
    // Any other curve counts as near, and past a work limit nothing is checked: the loose fit
    // stands, as before.
    let tight = size * 1e-4;
    let checkable = exact.len().saturating_mul(close.len()) <= 4_000_000;
    let strays = |v: &[LinkGeom]| exact.iter().any(|p| !v.iter().any(|g| plain_distance(g, *p).is_none_or(|d| d <= tight)));
    let (fitted, within) = if plain(&close) && (close.len() <= lossy.len() || (checkable && strays(&lossy) && !strays(&close))) {
        (close, near)
    } else {
        (lossy, size * 2e-3)
    };
    // A line ends on a point of the section, an arc on its circle nearest that point: they part
    // by up to the fit's tolerance. On a mesh body's facets (a scan's, noisy), by that much: so
    // there an arc's end joins only the curve end nearest it, when that is a line's, and not a
    // line of the outline beside it (a thin wall's other side). Two arcs that meet part by up to
    // twice that, and are stitched where they are each other's nearest.
    if shaded_flat { stitch_ends(join_rounds_nearest(fitted, within), 3.0 * within) } else { join_rounds(fitted, size * 1e-5) }
}

/// A fit puts each arc's ends on its own circle, so two pieces that met at one point of the
/// section can end up to twice the fit's tolerance apart, and the outline does not close (a
/// sketch joins ends only 1e-4 mm apart). Ends that are each other's nearest, within `tol`, move
/// to their midpoint. A moved arc takes the centre nearest its own that lies as far from both its
/// ends (keeping the radius instead moves the centre of a half circle far off).
fn stitch_ends(mut v: Vec<LinkGeom>, tol: f64) -> Vec<LinkGeom> {
    let ends: Vec<(usize, Vec2)> = v
        .iter()
        .enumerate()
        .flat_map(|(i, g)| match *g {
            LinkGeom::Line(a, b) | LinkGeom::Arc { a, b, .. } => vec![(i, a), (i, b)],
            _ => Vec::new(),
        })
        .collect();
    if !(tol > 0.0) {
        return v;
    }
    let cell = |p: Vec2| ((p.x / tol).floor() as i64, (p.y / tol).floor() as i64);
    let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (k, (_, p)) in ends.iter().enumerate() {
        grid.entry(cell(*p)).or_default().push(k);
    }
    let nearest = |k: usize| -> Option<usize> {
        let &(i, p) = ends.get(k)?;
        let (cx, cy) = cell(p);
        let mut best: Option<(f64, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &m in grid.get(&(cx + dx, cy + dy)).map(Vec::as_slice).unwrap_or_default() {
                    let Some(&(j, q)) = ends.get(m) else { continue };
                    let d = p.dist(q);
                    if j != i && d <= tol && best.is_none_or(|(e, _)| d < e) {
                        best = Some((d, m));
                    }
                }
            }
        }
        best.map(|(_, m)| m)
    };
    let mut moved: Vec<Vec2> = ends.iter().map(|(_, p)| *p).collect();
    for k in 0..ends.len() {
        let Some(m) = nearest(k).filter(|m| nearest(*m) == Some(k)) else { continue };
        if let (Some(&(_, p)), Some(&(_, q)), Some(slot)) = (ends.get(k), ends.get(m), moved.get_mut(k)) {
            *slot = p.lerp(q, 0.5);
        }
    }
    let mut at = moved.into_iter();
    for g in &mut v {
        match *g {
            LinkGeom::Line(..) => {
                if let (Some(a), Some(b)) = (at.next(), at.next()) {
                    *g = LinkGeom::Line(a, b);
                }
            }
            LinkGeom::Arc { c, a: a0, b: b0 } => {
                let (Some(a), Some(b)) = (at.next(), at.next()) else { continue };
                if a == a0 && b == b0 {
                    continue;
                }
                let mid = a.lerp(b, 0.5);
                let Some(across) = (b - a).normalized().map(|u| u.perp()) else { continue };
                *g = LinkGeom::Arc { c: mid + across * (c - mid).dot(across), a, b };
            }
            _ => {}
        }
    }
    v
}

/// [`join_rounds`], each arc end joining at most one line: the line whose end is the nearest
/// curve end to it (another arc's end nearer, it joins none), when that is within `tol`.
fn join_rounds_nearest(mut fitted: Vec<LinkGeom>, tol: f64) -> Vec<LinkGeom> {
    for i in 0..fitted.len() {
        let Some(&LinkGeom::Arc { c, mut a, mut b }) = fitted.get(i) else { continue };
        let r = c.dist(a);
        for end in [&mut a, &mut b] {
            let nearest = fitted
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .filter_map(|(j, g)| match *g {
                    LinkGeom::Line(p, q) => Some((j, p.dist(*end).min(q.dist(*end)), true)),
                    LinkGeom::Arc { a: p, b: q, .. } => Some((j, p.dist(*end).min(q.dist(*end)), false)),
                    _ => None,
                })
                .min_by(|x, y| x.1.total_cmp(&y.1));
            let Some((j, gap, true)) = nearest else { continue };
            if gap > tol {
                continue;
            }
            if let Some(LinkGeom::Line(p, q)) = fitted.get_mut(j) {
                let (near, far) = if p.dist(*end) <= q.dist(*end) { (p, q) } else { (q, p) };
                let joint = line_meets_round(*far, *near, c, r, tol);
                *near = joint;
                *end = joint;
            }
        }
        if let Some(arc) = fitted.get_mut(i) {
            *arc = LinkGeom::Arc { c, a, b };
        }
    }
    fitted
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
/// its B-rep face), the crossings that lie exactly on the surface (on a ruling), and the
/// facets' sag there: the furthest the surface strays from a crossed edge, at its middle.
/// `shaded_flat`: the mesh is a mesh body's shaded flat ([`shaded_flat`]).
fn section_segments(plane: &Plane, m: &Mesh, face: Option<u32>, shaded_flat: bool) -> (Vec<(Vec2, Vec2, u32)>, Vec<Vec2>, f64) {
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
    // A mesh shaded flat gives each triangle its own normal, so every vertex of a curved surface
    // would be a crease and its crossings would fall on the chords, inside the surface by up to
    // the mesh's sag: too far for the close fit, which then breaks each round into lines. Its
    // surface normals are its facets' normals averaged by their angles at each vertex, over the
    // facets that turn less than SMOOTH_TURN_DEGREES from their neighbours (never across a crease).
    // Only the corners at the ends of the edges the plane crosses are asked for: those of the
    // triangles it crosses, smoothed over every facet around them (their whole fans).
    let smoothed: Option<std::collections::HashMap<u32, Vec3>> = shaded_flat
        .then(|| {
            let at = |k: &u32| m.positions.get(*k as usize).copied();
            let crossed = |t: &&[u32; 3]| {
                let sides: Vec<bool> = t.iter().filter_map(at).map(|p| above(plane.height(p))).collect();
                sides.len() == 3 && sides.contains(&true) && sides.contains(&false)
            };
            let ends: std::collections::HashSet<Key> = m.triangles.iter().filter(crossed).flat_map(|t| t.iter().filter_map(at).map(key)).collect();
            let around: Vec<&[u32; 3]> = m.triangles.iter().filter(|t| t.iter().filter_map(at).any(|p| ends.contains(&key(p)))).collect();
            let mut fans = Mesh::default();
            for t in &around {
                let base = u32::try_from(fans.positions.len()).ok()?;
                fans.positions.extend(t.iter().map(|k| at(k).unwrap_or(Vec3::ZERO)));
                fans.triangles.push([base, base.checked_add(1)?, base.checked_add(2)?]);
            }
            let normals = fans.crease_corner_normals(SMOOTH_TURN_DEGREES.to_radians(), None).ok()?;
            let mut out = std::collections::HashMap::new();
            for (t, fan) in around.iter().zip(normals) {
                for (k, n) in t.iter().zip(fan) {
                    if at(k).is_some_and(|p| ends.contains(&key(p))) {
                        out.insert(*k, n);
                    }
                }
            }
            Some(out)
        })
        .flatten();
    let corner_normal = |k: u32| smoothed.as_ref().and_then(|o| o.get(&k)).or_else(|| m.normals.get(k as usize)).copied();
    let mut normals: BTreeMap<Key, Option<Vec3>> = BTreeMap::new();
    let mut face_normals: BTreeMap<(u32, Key), Option<Vec3>> = BTreeMap::new();
    // Faces on either side of each mesh edge.
    let mut edge_faces: BTreeMap<(Key, Key), Vec<u32>> = BTreeMap::new();
    // A mesh body is one face, so a vertex on a round's rim (where it meets a flat) has no normal
    // of its own: its smoothed edges take the normals at their ends of the facets either side of
    // them (None where those differ, across a crease). Lower key's end first.
    let mut edge_ends: BTreeMap<(Key, Key), (Option<Vec3>, Option<Vec3>)> = BTreeMap::new();
    let merge = |e: &mut Option<Vec3>, n: Vec3| {
        if e.is_some_and(|o| o.dot(n) < 1.0 - 1e-6) {
            *e = None;
        }
    };
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        for k in t {
            let (Some(p), Some(n)) = (m.positions.get(*k as usize), corner_normal(*k)) else { continue };
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
            if let Some(o) = &smoothed {
                let end = |k: usize| t.get(k).and_then(|v| o.get(v)).and_then(|n| n.normalized());
                let (Some(na), Some(nb)) = (end(i), end(j)) else { continue };
                let (edge, (n0, n1)) = if ka <= kb { ((ka, kb), (na, nb)) } else { ((kb, ka), (nb, na)) };
                edge_ends
                    .entry(edge)
                    .and_modify(|(e0, e1)| {
                        merge(e0, n0);
                        merge(e1, n1);
                    })
                    .or_insert((Some(n0), Some(n1)));
            }
        }
    }
    // Where the plane crosses a mesh edge: on a cubic through the end points whose end
    // tangents lie in the surface's tangent planes (so sections of curved faces land on the
    // surface, not on the chord), computed once per edge so neighbours agree.
    let exact: std::cell::RefCell<Vec<Vec2>> = std::cell::RefCell::new(Vec::new());
    let sag = std::cell::Cell::new(0.0_f64);
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
        let (na, nb) = edge_ends.get(&(key(a), key(b))).copied().unwrap_or_else(|| (normal(a), normal(b)));
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
        // A mesh body's surface, taken from its facets, strays from an edge (at its middle) as
        // far as an STL exported for CAD strays from its part, at most STL_CHORD of its size.
        // Further is a guess, not a round (a noisy scan's normals, or a long flat edge whose
        // corner normals lean into the round beside it): the edge's straight crossing stands.
        let bow = h(0.5).dist(a.lerp(b, 0.5));
        if shaded_flat && bow > size * STL_CHORD {
            return Some(a.lerp(b, ha / (ha - hb)));
        }
        sag.set(sag.get().max(bow));
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
    (segs, exact.into_inner(), sag.get())
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
    fit_segments_within(segs, size, preserve_arcs, if preserve_arcs { size * 1e-5 } else { size * 2e-3 }, false)
}

/// [`fit_segments`], the curves within `tol` of the segments' ends. `rejoin`: a closed chain
/// that fits as several pieces is fitted again from where its first piece ends. A loop without
/// corners starts wherever the walk met it first, and a piece is cut in two there: on a mesh
/// body's section, the crossing furthest down and left, which can lie on a straight wall.
fn fit_segments_within(segs: &[(Vec2, Vec2)], size: f64, preserve_arcs: bool, tol: f64, rejoin: bool) -> Vec<LinkGeom> {
    let loose = Some(tol);
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
            let mut pieces = Vec::new();
            fit_chain_with_arcs(&c, loose, &mut pieces, preserve_arcs);
            if rejoin
                && pieces.len() > 1
                && let Some(c) = from_first_joint(&c, pieces.first())
            {
                pieces.clear();
                fit_chain_with_arcs(&c, loose, &mut pieces, preserve_arcs);
            }
            out.append(&mut pieces);
        }
    }
    if preserve_arcs {
        snap_tangent_arcs(&mut out, size * 1e-3);
    }
    out
}

/// A closed chain (its last point its first) started again at the point nearest where `first`,
/// the first piece fitted to it, ends (never its first point); None for a chain of fewer than
/// three points or a first piece that is not a line or an arc.
fn from_first_joint(c: &[Vec2], first: Option<&LinkGeom>) -> Option<Vec<Vec2>> {
    let start = *c.first()?;
    let ends = match *first? {
        LinkGeom::Line(a, b) | LinkGeom::Arc { a, b, .. } => [a, b],
        _ => return None,
    };
    let joint = if ends[0].dist(start) >= ends[1].dist(start) { ends[0] } else { ends[1] };
    let n = c.len();
    let k = (1..n.saturating_sub(1)).min_by(|i, j| {
        let d = |k: &usize| c.get(*k).map_or(f64::MAX, |p| p.dist(joint));
        d(i).total_cmp(&d(j))
    })?;
    let mut q = c.get(k..n - 1)?.to_vec();
    q.extend(c.get(..=k)?);
    Some(q)
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
    if p.len() < 2 || ext <= 1e-9 {
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
    let fitted_arc = || {
        fits_circle(p, tol, exact, preserve_arcs)
            .filter(|(_, _, sw)| curved(*sw) || (!fits_line(p, tol) && sw.abs() >= 2f64.to_radians()))
            .and_then(|(c, r, sw)| arc_geom(p, c, r, sw, tol))
    };
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

    /// A shallow circular arc (e.g. 10 degrees on an R10 torus, #17498) is preserved as an arc
    /// rather than being flattened into a chord or discarded.
    #[test]
    fn shallow_ten_degree_arc_fits_as_an_arc_not_a_chord() {
        let pts = [
            Vec2::new(10.0, 20.0),
            Vec2::new(9.9996192306417, 20.087265354984),
            Vec2::new(9.9984769515639, 20.174524064373),
            Vec2::new(9.9965732497556, 20.261769483079),
            Vec2::new(9.993908270191, 20.348994967025),
            Vec2::new(9.9904822158186, 20.436193873653),
            Vec2::new(9.9862953475457, 20.523359562429),
            Vec2::new(9.9813479842187, 20.610485395349),
            Vec2::new(9.9756405025982, 20.697564737441),
            Vec2::new(9.9691733373313, 20.784590957278),
            Vec2::new(9.9619469809175, 20.871557427477),
            Vec2::new(9.9539619836718, 20.958457525202),
            Vec2::new(9.9452189536827, 21.045284632677),
            Vec2::new(9.9357185567659, 21.132032137679),
            Vec2::new(9.9254615164132, 21.218693434051),
            Vec2::new(9.9144486137381, 21.305261922201),
            Vec2::new(9.9026806874157, 21.391731009601),
            Vec2::new(9.8901586336192, 21.478094111296),
            Vec2::new(9.8768834059514, 21.564344650402),
            Vec2::new(9.8628560153723, 21.650476058607),
            Vec2::new(9.8480775301221, 21.736481776669),
        ];
        let mut out = Vec::new();
        fit_chain_with_arcs(&pts, Some(1e-4), &mut out, true);
        assert!(matches!(out.as_slice(), [LinkGeom::Arc { .. }]), "expected 10-degree arc preserved, got {out:?}");
    }
}
