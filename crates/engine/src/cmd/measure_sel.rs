//! Measuring picked geometry (`inspect.measure` with `items`): one item gives its length, area,
//! volume or position, and a round one (circle, arc, cylinder) its diameter and radius, a slot
//! its sizes; two give the minimum and maximum distance between them (with their points), the
//! distance from a round item's centre or axis (see [`super::measure_shape`]) and, where both
//! have a direction (planar faces, straight edges), the angle.

use serde_json::{Value, json};
use solidcraft_geom::{Mesh, UnitVec3, Vec3};

use super::measure_shape::{Shape, Slot, add_round, centre};
use crate::params::bad;
use crate::{Result, Sel, Session};

/// Geometry of a picked item, as points or triangles.
enum Geo {
    Points(Vec<Vec3>),
    Tris(Vec<[Vec3; 3]>),
}

/// Caps that keep a measurement interactive on big meshes.
const MAX_POINTS: usize = 4000;
const MAX_TRIS: usize = 20000;

fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let n = (b - a).cross(c - a);
    if let Some(nn) = n.normalized() {
        let q = p - nn * (p - a).dot(nn);
        if [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-12) {
            return q;
        }
    }
    let on_seg = |u: Vec3, v: Vec3| {
        let d = v - u;
        let l2 = d.dot(d);
        let t = if l2 > 0.0 { ((p - u).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        u + d * t
    };
    [on_seg(a, b), on_seg(b, c), on_seg(c, a)].into_iter().min_by(|x, y| x.dist(p).total_cmp(&y.dist(p))).unwrap_or(a)
}

fn polyline_len(pts: &[Vec3]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// A polyline with extra points so no gap is longer than 1/64 of it.
fn densify(pts: &[Vec3]) -> Vec<Vec3> {
    let step = (polyline_len(pts) / 64.0).max(1e-9);
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let n = ((w[0].dist(w[1]) / step).ceil() as usize).clamp(1, 64);
        for k in 0..n {
            out.push(w[0].lerp(w[1], k as f64 / n as f64));
        }
    }
    out.extend(pts.last().copied());
    out
}

/// How far `point` lies from the polyline `e`.
fn polyline_dist(e: &[Vec3], point: Vec3) -> f64 {
    e.windows(2).map(|w| point.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min)
}

/// The mesh edge a selection means: by index if it still passes through the point, otherwise
/// (or with no index) the nearest.
fn edge_index(m: &Mesh, index: Option<usize>, point: Vec3) -> Option<usize> {
    let tol = m.bounds().diagonal().max(1.0) * 1e-6;
    if let Some(index) = index.filter(|&i| m.edges.get(i).is_some_and(|e| polyline_dist(e, point) < tol)) {
        return Some(index);
    }
    m.edges.iter().enumerate().min_by(|(_, a), (_, b)| polyline_dist(a, point).total_cmp(&polyline_dist(b, point))).map(|(i, _)| i)
}

fn face_tris(m: &Mesh, f: usize) -> Vec<[Vec3; 3]> {
    m.triangles.iter().zip(&m.tri_face).filter(|(_, tf)| **tf as usize == f).filter_map(|(t, _)| m.tri(t)).collect()
}

/// A described item: its geometry, direction (planar face normal, straight edge direction),
/// description and exact shape.
struct Item {
    geo: Geo,
    dir: Option<Vec3>,
    json: Value,
    shape: Shape,
    /// A slot this item is one end of (or the outline of).
    slot: Option<Slot>,
}

/// The circle through a polyline's points (all on one within `tol`): a fallback for edges the
/// kernel can't name (mesh bodies).
fn fit_circle(pts: &[Vec3], tol: f64) -> Option<(Vec3, UnitVec3, f64)> {
    let n = pts.len().checked_sub(1)?;
    let (a, b, c) = (*pts.first()?, *pts.get(n / 3)?, *pts.get(2 * n / 3)?);
    let (u, v) = (b - a, c - a);
    let w = u.cross(v);
    if !(w.len2() > u.len2() * v.len2() * 1e-16) {
        return None;
    }
    let center = a + (v * u.len2() - u * v.len2()).cross(w) * (1.0 / (2.0 * w.len2()));
    let axis = w.unit()?;
    let radius = center.dist(a);
    let on = pts.iter().all(|p| (p.dist(center) - radius).abs() <= tol && (*p - center).dot(axis).abs() <= tol);
    (on && radius.is_finite()).then_some((center, axis, radius))
}

/// The round shape of mesh edge `index` of body `b`: from the kernel's exact curve when it
/// is the same edge (its ends on the polyline), else fitted to the polyline's points; and
/// whether it is the exact one.
fn edge_round(b: &solidcraft_doc::ModelBody, m: &Mesh, index: usize, whole: bool) -> Option<(Shape, bool)> {
    let pts = m.edges.get(index)?;
    let tol = (m.bounds().diagonal().max(1.0) * 1e-6).max(1e-9);
    let exact = solidcraft_kernel::edge_circle(&b.body, index).filter(|c| {
        let ends = [pts.first(), pts.last()].into_iter().flatten().all(|p| p.dist(c.start).min(p.dist(c.end)) <= tol * 10.0);
        let on = pts.iter().all(|p| (p.dist(c.center) - c.radius).abs() <= tol * 10.0);
        ends && on
    });
    let is_exact = exact.is_some();
    let (center, axis, radius, angle) = match exact {
        Some(c) => (c.center, c.axis, c.radius, if whole { std::f64::consts::TAU } else { c.angle }),
        None => {
            let all: Vec<Vec3> =
                if whole { m.circular_rim(index).iter().filter_map(|&i| m.edges.get(i)).flatten().copied().collect() } else { pts.clone() };
            let (center, axis, radius) = fit_circle(&all, tol * 10.0)?;
            let angle = if whole { std::f64::consts::TAU } else { polyline_len(pts) / radius };
            (center, axis, radius, angle)
        }
    };
    Some((Shape::Round { center, axis, radius, angle, planar: true, lo: 0.0, hi: 0.0 }, is_exact))
}

/// The length of a measured curve: an exact circle's or arc's own (radius times angle, as its
/// circumference and arc length are), else that of the polyline drawn for it, which runs inside
/// a curve (#9086).
fn curve_len(shape: &Shape, exact: bool, polyline: f64) -> f64 {
    match *shape {
        Shape::Round { radius, angle, .. } if exact => radius * angle,
        _ => polyline,
    }
}

/// Points along a circle or arc (`angle` from `start` about `axis`).
fn arc_points(center: Vec3, axis: UnitVec3, start: Vec3, angle: f64) -> Vec<Vec3> {
    let x = start - center;
    let y = axis.cross(x);
    let n = 128;
    (0..=n)
        .map(|i| {
            let t = angle * f64::from(i) / f64::from(n);
            center + x * t.cos() + y * t.sin()
        })
        .collect()
}

/// The solved sketch holding a picked curve or point: `sketch` when given, else the active
/// sketch, else the last sketch with that id.
fn sketch_of<'a>(
    s: &Session,
    st: &'a solidcraft_doc::ModelState,
    sketch: Option<u64>,
    has: impl Fn(&solidcraft_sketch::Sketch) -> bool,
) -> Option<&'a solidcraft_doc::SolvedSketch> {
    if let Some(id) = sketch {
        return st.sketch(id);
    }
    if let Some(ss) = super::sketch_store::active(s).and_then(|id| st.sketch(id)).filter(|ss| has(&ss.sketch)) {
        return Some(ss);
    }
    st.sketches.iter().rev().map(|ss| &**ss).find(|ss| has(&ss.sketch))
}

fn sketch_curve(ss: &solidcraft_doc::SolvedSketch, id: &str) -> Option<Item> {
    use solidcraft_sketch::CurveKind;
    let sk = &ss.sketch;
    let c = sk.curves.get(sk.curve_index(id)?)?;
    let at = |i: usize| sk.points.get(i).map(|p| ss.plane.to_world(p.pos));
    let normal = ss.plane.normal().unit()?;
    let (shape, pts) = match c.kind {
        CurveKind::Circle { c, r } => {
            let center = at(c)?;
            let shape = Shape::Round { center, axis: normal, radius: r, angle: std::f64::consts::TAU, planar: true, lo: 0.0, hi: 0.0 };
            (shape, arc_points(center, normal, center + ss.plane.x * r, std::f64::consts::TAU))
        }
        CurveKind::Arc { c, a, b } => {
            let (center, a, b) = (at(c)?, at(a)?, at(b)?);
            let radius = center.dist(a);
            let (x, y) = ((a - center).normalized()?, normal.cross((a - center).normalized()?));
            let mut angle = (b - center).dot(y).atan2((b - center).dot(x));
            if angle <= 0.0 {
                angle += std::f64::consts::TAU;
            }
            let shape = Shape::Round { center, axis: normal, radius, angle, planar: true, lo: 0.0, hi: 0.0 };
            (shape, arc_points(center, normal, a, angle))
        }
        CurveKind::Line { a, b } => {
            let (a, b) = (at(a)?, at(b)?);
            (Shape::Line { a, b }, densify(&[a, b]))
        }
        _ => return None,
    };
    let mut json = json!({"type": "sketch_curve", "id": id});
    let dir = match shape {
        Shape::Line { a, b } => {
            json["length_mm"] = json!(a.dist(b));
            (b - a).normalized()
        }
        _ => {
            // A sketch's circle or arc is exact.
            json["length_mm"] = json!(curve_len(&shape, true, polyline_len(&pts)));
            add_round(&mut json, &shape);
            None
        }
    };
    Some(Item { geo: Geo::Points(pts), dir, json, shape, slot: None })
}

/// A profile (closed region of a sketch): its area, and when it is a circle or a slot, its
/// sizes.
fn profile(ss: &solidcraft_doc::SolvedSketch, index: usize) -> Option<Item> {
    use solidcraft_geom::{Seg2, Vec2};
    let pr = ss.profiles.get(index)?;
    let outline: Vec<Vec3> = pr.region.outer.polyline(1e-3).iter().map(|p| ss.plane.to_world(*p)).collect();
    let mut json = json!({"type": "profile", "area_mm2": pr.region.area()});
    let normal = ss.plane.normal().unit()?;
    // Arcs about one centre merge (a circle is two halves); straight pieces stay as they are.
    let mut arcs: Vec<(Vec2, f64, f64)> = Vec::new();
    let mut lines: Vec<(Vec2, Vec2)> = Vec::new();
    let mut other = !pr.region.holes.is_empty();
    for seg in &pr.region.outer.segs {
        match *seg {
            Seg2::Arc { center, radius, sweep, .. } => {
                match arcs.iter_mut().find(|(c, r, _)| c.dist(center) <= 1e-6 * (1.0 + radius) && (r - radius).abs() <= 1e-6 * (1.0 + radius)) {
                    Some(a) => a.2 += sweep,
                    None => arcs.push((center, radius, sweep)),
                }
            }
            Seg2::Line { a, b } => lines.push((a, b)),
            _ => other = true,
        }
    }
    let mut shape = Shape::Other;
    let mut slot = None;
    if !other {
        if let ([(c, r, sweep)], []) = (arcs.as_slice(), lines.as_slice())
            && (sweep.abs() - std::f64::consts::TAU).abs() < 1e-6
        {
            shape = Shape::Round {
                center: ss.plane.to_world(*c),
                axis: normal,
                radius: *r,
                angle: std::f64::consts::TAU,
                planar: true,
                lo: 0.0,
                hi: 0.0,
            };
            add_round(&mut json, &shape);
        } else if let Some((a, b, r)) = super::measure_shape::slot_outline(&arcs, &lines) {
            let s = Slot { width: 2.0 * r, centres: (ss.plane.to_world(a), ss.plane.to_world(b)) };
            json["slot"] = s.json();
            slot = Some(s);
        }
    }
    let planar = Some(*normal);
    let geo = Geo::Points(densify(&outline));
    let shape = if matches!(shape, Shape::Other) { Shape::Plane { point: ss.plane.origin, normal } } else { shape };
    Some(Item { geo, dir: planar, json, shape, slot })
}

/// A slot this cylindrical face is one end of, from the body's other faces.
fn face_slot(b: &solidcraft_doc::ModelBody, m: &Mesh, shape: &Shape, pts: &[Vec3]) -> Option<Slot> {
    let faces = m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap_or(0);
    if faces > 4000 {
        return None;
    }
    let mut verts: Vec<Vec<Vec3>> = vec![Vec::new(); faces];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if let (Some(v), Some(tri)) = (verts.get_mut(*f as usize), m.tri(t)) {
            v.extend(tri);
        }
    }
    let surfaces: Vec<(solidcraft_kernel::FaceSurface, Vec<Vec3>)> =
        verts.into_iter().enumerate().filter_map(|(i, v)| Some((solidcraft_kernel::face_surface(&b.body, i)?, v))).collect();
    super::measure_shape::slot_from_faces(shape, pts, &surfaces)
}

/// `sel` in the placed model `st`.
fn describe(s: &Session, st: &solidcraft_doc::ModelState, sel: &Sel) -> Option<Item> {
    let plain = |geo, dir, json| Some(Item { geo, dir, json, shape: Shape::Other, slot: None });
    match sel {
        Sel::Vertex { point, .. } => Some(Item {
            geo: Geo::Points(vec![*point]),
            dir: None,
            json: json!({"type": "vertex", "position": point}),
            shape: Shape::Point(*point),
            slot: None,
        }),
        Sel::Edge { body, index, point } => {
            let b = st.body(body)?;
            let m = b.mesh();
            let index = edge_index(&m, Some(*index), *point)?;
            let pts = m.edges.get(index)?;
            let rim = m.circular_rim(index);
            if rim.len() > 1 {
                let pieces: Vec<&Vec<Vec3>> = rim.iter().filter_map(|&i| m.edges.get(i)).collect();
                let len: f64 = pieces.iter().map(|p| polyline_len(p)).sum();
                let points = pieces.iter().flat_map(|p| densify(p)).collect();
                let (shape, exact) = edge_round(b, &m, index, true).unwrap_or((Shape::Other, false));
                let mut json = json!({"type": "edge", "length_mm": curve_len(&shape, exact, len)});
                add_round(&mut json, &shape);
                return Some(Item { geo: Geo::Points(points), dir: None, json, shape, slot: None });
            }
            let (a, z) = (*pts.first()?, *pts.last()?);
            let straight = (z - a).normalized().filter(|d| pts.iter().all(|p| (*p - a).cross(*d).len() < 1e-6 * (1.0 + a.dist(z))));
            let (shape, exact) = match straight {
                Some(_) => (Shape::Line { a, b: z }, false),
                None => edge_round(b, &m, index, false).unwrap_or((Shape::Other, false)),
            };
            let mut json = json!({"type": "edge", "length_mm": curve_len(&shape, exact, polyline_len(pts))});
            add_round(&mut json, &shape);
            Some(Item { geo: Geo::Points(densify(pts)), dir: straight, json, shape, slot: None })
        }
        Sel::Face { body, index, .. } => {
            let b = st.body(body)?;
            let m = b.mesh();
            let patch = m.surface_patch(*index);
            let tris = patch.iter().flat_map(|f| face_tris(&m, *f)).collect::<Vec<_>>();
            let area: f64 = tris.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).len() * 0.5).sum();
            let normals: Vec<UnitVec3> = tris.iter().filter_map(|[a, b, c]| (*b - *a).cross(*c - *a).unit()).collect();
            let planar = normals.first().copied().filter(|n0| normals.iter().all(|n| n.dot(*n0) > 1.0 - 1e-6));
            let mut json = json!({"type": "face", "area_mm2": area, "planar": planar.is_some()});
            let verts: Vec<Vec3> = tris.iter().flat_map(|t| t.iter().copied()).collect();
            let shape = match solidcraft_kernel::face_surface(&b.body, *index) {
                Some(solidcraft_kernel::FaceSurface::Cylinder { origin, axis, radius }) => {
                    super::measure_shape::cylinder(origin, axis, radius, &verts).unwrap_or(Shape::Other)
                }
                Some(solidcraft_kernel::FaceSurface::Plane { normal, point }) if planar.is_some() => Shape::Plane { point, normal },
                None => match (planar, verts.first()) {
                    (Some(normal), Some(point)) => Shape::Plane { point: *point, normal },
                    _ => Shape::Other,
                },
                _ => Shape::Other,
            };
            add_round(&mut json, &shape);
            let slot = if matches!(shape, Shape::Round { .. }) && !shape.full() { face_slot(b, &m, &shape, &verts) } else { None };
            if let Some(sl) = &slot {
                json["slot"] = sl.json();
            }
            Some(Item { geo: Geo::Tris(tris), dir: planar.map(Vec3::from), json, shape, slot })
        }
        Sel::Body { name } => {
            let body = st.body(name)?;
            let closed = crate::cmd::inspect::encloses_volume(&body.body);
            let m = body.mesh();
            let tris: Vec<[Vec3; 3]> = m.triangles.iter().filter_map(|t| m.tri(t)).collect();
            let v = m.measure().volume;
            plain(Geo::Tris(tris), None, json!({"type": "body", "name": name, "closed": closed, "volume_mm3": closed.then_some(v)}))
        }
        Sel::SketchCurve { id, sketch } => sketch_curve(sketch_of(s, st, *sketch, |sk| sk.curve_index(id).is_some())?, id),
        Sel::ExternalSketchCurve { sketch, id } => sketch_curve(sketch_of(s, st, Some(*sketch), |_| true)?, id),
        Sel::SketchPoint { id } | Sel::ExternalSketchPoint { id, .. } => {
            let given = match sel {
                Sel::ExternalSketchPoint { sketch, .. } => Some(*sketch),
                _ => None,
            };
            // "<sketch id>:<point id>" names a point of any sketch.
            let (given, id) = match id.split_once(':').and_then(|(k, p)| Some((k.parse::<u64>().ok()?, p))) {
                Some((k, p)) if given.is_none() => (Some(k), p),
                _ => (given, id.as_str()),
            };
            let ss = sketch_of(s, st, given, |sk| sk.point_index(id).is_some())?;
            let p = ss.plane.to_world(ss.sketch.points.get(ss.sketch.point_index(id)?)?.pos);
            Some(Item {
                geo: Geo::Points(vec![p]),
                dir: None,
                json: json!({"type": "sketch_point", "id": id, "position": p}),
                shape: Shape::Point(p),
                slot: None,
            })
        }
        Sel::Profile { sketch, index } => profile(st.sketch(*sketch)?, *index),
        _ => None,
    }
}

/// A picked item's exact shape with its points (an edge's polyline) and triangles (a face's),
/// as Measure sees it: what [`crate::snap`] takes an item's centres, middle and ends from.
pub(crate) fn shape_of(s: &Session, sel: &Sel) -> Option<(Shape, Vec<Vec3>, Vec<[Vec3; 3]>)> {
    let world = s.world_state().ok()?;
    let item = describe(s, &world, sel)?;
    Some(match item.geo {
        Geo::Points(p) => (item.shape, p, Vec::new()),
        Geo::Tris(t) => (item.shape, Vec::new(), t),
    })
}

fn sample<T: Copy>(v: &[T], max: usize) -> Vec<T> {
    let step = v.len().div_ceil(max).max(1);
    v.iter().step_by(step).copied().collect()
}

/// Closest pair of points between two pieces of geometry.
fn closest(a: &Geo, b: &Geo) -> Option<(Vec3, Vec3)> {
    let pts_to_tris = |pts: &[Vec3], tris: &[[Vec3; 3]]| -> Option<(Vec3, Vec3)> {
        let pts = sample(pts, MAX_POINTS);
        let tris = sample(tris, MAX_TRIS);
        let mut best: Option<(f64, Vec3, Vec3)> = None;
        for p in &pts {
            for [x, y, z] in &tris {
                let q = closest_on_triangle(*p, *x, *y, *z);
                let d = p.dist(q);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, *p, q));
                }
            }
        }
        best.map(|(_, p, q)| (p, q))
    };
    let verts = |tris: &[[Vec3; 3]]| -> Vec<Vec3> { tris.iter().flat_map(|t| t.iter().copied()).collect() };
    match (a, b) {
        (Geo::Points(p), Geo::Points(q)) => {
            let (p, q) = (sample(p, MAX_POINTS), sample(q, MAX_POINTS));
            let mut best: Option<(f64, Vec3, Vec3)> = None;
            for x in &p {
                for y in &q {
                    let d = x.dist(*y);
                    if best.is_none_or(|(bd, _, _)| d < bd) {
                        best = Some((d, *x, *y));
                    }
                }
            }
            best.map(|(_, x, y)| (x, y))
        }
        (Geo::Points(p), Geo::Tris(t)) => pts_to_tris(p, t),
        (Geo::Tris(t), Geo::Points(p)) => pts_to_tris(p, t).map(|(x, y)| (y, x)),
        (Geo::Tris(t1), Geo::Tris(t2)) => {
            let ab = pts_to_tris(&verts(t1), t2);
            let ba = pts_to_tris(&verts(t2), t1).map(|(x, y)| (y, x));
            [ab, ba].into_iter().flatten().min_by(|x, y| x.0.dist(x.1).total_cmp(&y.0.dist(y.1)))
        }
    }
}

/// Two straight edges sharing exactly one end define rays away from that corner. Their
/// stored directions are arbitrary, so use the rays to preserve obtuse (and straight) angles.
/// Separate edges and coincident copies have no unique corner and keep the undirected angle.
fn corner_cos(a: Shape, b: Shape) -> Option<f64> {
    let (Shape::Line { a, b }, Shape::Line { a: c, b: d }) = (a, b) else { return None };
    let tol = a.dist(b).min(c.dist(d)).max(1.0) * 1e-6;
    let mut corners = [(a, b, c, d), (a, b, d, c), (b, a, c, d), (b, a, d, c)].into_iter().filter(|(p, _, q, _)| p.dist(*q) <= tol);
    let (p, end_a, q, end_b) = corners.next()?;
    if corners.next().is_some() {
        return None;
    }
    Some((end_a - p).normalized()?.dot((end_b - q).normalized()?))
}

/// A face or edge as `inspect.measure` takes it: the body and a point on the item; `index` is
/// optional (the point finds the item).
#[derive(serde::Deserialize)]
struct Pick {
    body: String,
    point: Vec3,
    #[serde(default)]
    index: Option<usize>,
}

/// Resolves a face or edge pick to the [`Sel`] it means, merging the two kernel halves of one
/// visible item (a split cylinder's patches, a circle's two arcs) into one measurement target.
fn resolve_pick(st: &solidcraft_doc::ModelState, face: bool, value: &Value) -> std::result::Result<Sel, String> {
    let Pick { body, point, index } = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let b = st.body(&body).ok_or_else(|| format!("no body named '{body}'"))?;
    let m = b.mesh();
    if face {
        let index = match index {
            Some(i) => i,
            None => solidcraft_doc::appearance::face_index_at(b, point).ok_or_else(|| format!("no face of '{body}' at the picked point"))?,
        };
        let index = m.surface_patch(index).first().copied().unwrap_or(index);
        return Ok(Sel::Face { body, index, point });
    }
    let resolved = edge_index(&m, index, point)
        .filter(|&i| index.is_some() || m.edges.get(i).is_some_and(|e| polyline_dist(e, point) <= solidcraft_doc::appearance::pick_tolerance(b)))
        .or(index)
        .ok_or_else(|| format!("no edge of '{body}' at the picked point"))?;
    let index = m.circular_rim(resolved).first().copied().unwrap_or(resolved);
    Ok(Sel::Edge { body, index, point })
}

/// `inspect.measure` with `items` (one or two selections).
pub(crate) fn measure_items(s: &Session, items: &Value) -> Result<Value> {
    let cmd = "inspect.measure";
    let list = items.as_array().ok_or_else(|| bad(cmd, "`items` must be a list of selections"))?;
    if list.is_empty() || list.len() > 1000 {
        return Err(bad(cmd, "measure one or two items"));
    }
    let st = s.world_state()?;
    let mut sels = Vec::new();
    for value in list {
        let sel = match value.get("type").and_then(Value::as_str) {
            Some(kind @ ("face" | "edge")) => resolve_pick(&st, kind == "face", value).map_err(|e| bad(cmd, e))?,
            _ => serde_json::from_value::<Sel>(value.clone()).map_err(|e| bad(cmd, e.to_string()))?,
        };
        let duplicate = sels.iter().any(|x| match (x, &sel) {
            (Sel::Edge { body: a, index: i, .. }, Sel::Edge { body: b, index: j, .. })
            | (Sel::Face { body: a, index: i, .. }, Sel::Face { body: b, index: j, .. }) => a == b && i == j,
            _ => false,
        });
        if !duplicate {
            sels.push(sel);
        }
        if sels.len() > 2 {
            return Err(bad(cmd, "measure one or two items"));
        }
    }
    let world = s.world_state()?;
    let mut described = Vec::new();
    for x in &sels {
        described.push(
            describe(s, &world, x)
                .ok_or_else(|| bad(cmd, "an item can't be measured (pick vertices, edges, faces, bodies, sketch curves or profiles)"))?,
        );
    }
    let mut out = json!({"items": described.iter().map(|d| d.json.clone()).collect::<Vec<_>>()});
    if let [one] = described.as_slice() {
        for k in ["diameter", "radius", "circumference", "angle_deg", "arc_length", "center", "slot"] {
            if let Some(v) = one.json.get(k) {
                out[k] = v.clone();
            }
        }
        if let Some(sl) = &one.slot {
            out["center_from"] = json!(sl.centres.0);
            out["center_to"] = json!(sl.centres.1);
        }
    }
    if let [a, b] = described.as_slice() {
        let near = closest(&a.geo, &b.geo);
        let far = farthest(&a.geo, &b.geo);
        let c = centre(&a.shape, &b.shape);
        // An exact closest or farthest distance stands only where the mesh agrees with it to
        // within its chord error (a bounded face can end before the exact value's point).
        let radii = [a.shape, b.shape].iter().map(|x| if let Shape::Round { radius, .. } = x { *radius } else { 0.0 }).sum::<f64>();
        let slack = radii * 0.05 + 1e-6;
        let mut min = near.map(|(p, q)| p.dist(q));
        if let (Some(exact), Some(m)) = (c.and_then(|c| c.min), min)
            && (m - exact).abs() <= slack
        {
            min = Some(exact);
        }
        let mut max = far.map(|(p, q)| p.dist(q));
        if let (Some(exact), Some(m)) = (c.and_then(|c| c.max), max)
            && (m - exact).abs() <= slack
        {
            max = Some(exact);
        }
        if let (Some((p, q)), Some(d)) = (near, min) {
            out["distance_mm"] = json!(d);
            out["from"] = json!(p);
            out["to"] = json!(q);
            out["delta"] = json!(q - p);
            out["min"] = json!(d);
        }
        if let (Some((p, q)), Some(d)) = (far, max) {
            out["max"] = json!(d);
            out["max_from"] = json!(p);
            out["max_to"] = json!(q);
            out["max_delta"] = json!(q - p);
        }
        if let Some(c) = c {
            out["center_distance"] = json!(c.distance);
            out["center_from"] = json!(c.from);
            out["center_to"] = json!(c.to);
            out["center_delta"] = json!(c.to - c.from);
        }
        out["measure"] = json!(if c.is_some() { "center" } else { "min" });
        if let (Some(da), Some(db)) = (a.dir, b.dir) {
            let both_faces = matches!(sels.as_slice(), [Sel::Face { .. }, Sel::Face { .. }]);
            let cos = if both_faces { da.dot(db) } else { corner_cos(a.shape, b.shape).unwrap_or_else(|| da.dot(db).abs()) };
            out["angle_deg"] = json!(cos.clamp(-1.0, 1.0).acos().to_degrees());
        }
    }
    Ok(out)
}

/// Farthest pair of points between two pieces of geometry (on their samples and vertices).
fn farthest(a: &Geo, b: &Geo) -> Option<(Vec3, Vec3)> {
    let pts = |g: &Geo| -> Vec<Vec3> {
        match g {
            Geo::Points(p) => sample(p, 2000),
            Geo::Tris(t) => sample(&t.iter().flat_map(|t| t.iter().copied()).collect::<Vec<_>>(), 2000),
        }
    };
    let (p, q) = (pts(a), pts(b));
    let mut best: Option<(f64, Vec3, Vec3)> = None;
    for x in &p {
        for y in &q {
            let d = x.dist(*y);
            if best.is_none_or(|(bd, _, _)| d > bd) {
                best = Some((d, *x, *y));
            }
        }
    }
    best.map(|(_, x, y)| (x, y))
}
