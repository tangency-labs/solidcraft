//! Remove artificial subdivisions of one cylindrical support before blending.
//! A tiny rim chord left by a Boolean must not limit a fillet that can follow the
//! same cylinder's full rim. Authored Split Face boundaries remain separate.
use std::collections::{BTreeMap, HashMap, HashSet};

use solidcraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3, v3};
use crate::offset::{Surf, surf_of};
use crate::record::{self, Rebuild, Was};
use crate::{Result, guard};

pub(crate) fn coalesce(body: &Body, points: &[Vec3]) -> Result<Body> {
    let made = guard("coalesce blend supports", || Ok(build(body, points)))?;
    Ok(made.map_or_else(|| body.clone(), |made| record::after(made, body, &record::counts(&[body]))))
}

fn same(a: Surf, b: Surf, tol: f64) -> bool {
    match (a, b) {
        (Surf::Cylinder { o: a, a: da, r: ra, convex: ca }, Surf::Cylinder { o: b, a: db, r: rb, convex: cb }) => {
            ca == cb && da.dot(db).abs() > 1.0 - 1e-12 && (ra - rb).abs() < tol && ((b - a) - da * (b - a).dot(da)).len() < tol
        }
        _ => false,
    }
}

// Exact support equality lets a microscopic trim borrow the classification of
// a wider patch, without estimating a radius from almost coincident normals.
fn same_support(a: &mt::Surface, b: &mt::Surface) -> bool {
    match (a, b) {
        (mt::Surface::BSplineSurface(a), mt::Surface::BSplineSurface(b)) => a == b,
        (mt::Surface::NurbsSurface(a), mt::Surface::NurbsSurface(b)) => a == b,
        (mt::Surface::RevolutedCurve(a), mt::Surface::RevolutedCurve(b)) => {
            a.transform() == b.transform()
                && a.orientation() == b.orientation()
                && a.entity().origin() == b.entity().origin()
                && a.entity().axis() == b.entity().axis()
                && match (a.entity().entity_curve(), b.entity().entity_curve()) {
                    (mt::Curve::Line(a), mt::Curve::Line(b)) => a == b,
                    (mt::Curve::BSplineCurve(a), mt::Curve::BSplineCurve(b)) => a == b,
                    (mt::Curve::NurbsCurve(a), mt::Curve::NurbsCurve(b)) => a == b,
                    _ => false,
                }
        }
        _ => false,
    }
}

fn build(body: &Body, points: &[Vec3]) -> Option<Body> {
    if body.solid.face_iter().take(1001).count() > 1000 || body.solid.edge_iter().take(20_001).count() > 20_000 {
        return None;
    }
    let solid = body.deep_copy();
    // Only supports of selected circular edges need their rim subdivisions
    // removed. A straight edge between distinct cylinders keeps its end seams.
    let pick_tol = (body.size() * 2e-3).max(1e-3);
    let picked: HashSet<_> = Body::unique_edges(&solid)
        .into_iter()
        .filter(|edge| {
            crate::curveblend::circle_of(edge, pick_tol * 1e-3).is_some_and(|(o, r, axis)| {
                points.iter().any(|p| {
                    (p.dist(o) - r).abs() <= pick_tol && (*p - o).dot(axis).abs() <= pick_tol && crate::blend::on_edge(edge, *p, pick_tol).is_some()
                })
            })
        })
        .map(|e| e.id())
        .collect();
    if picked.is_empty() {
        return None;
    }
    let tol = (body.size() * 1e-9).max(1e-10);
    let mut rec = Rebuild::default();
    let mut shells = Vec::new();
    let mut changed = false;
    for shell in solid.boundaries() {
        let faces: Vec<_> = shell.face_iter().cloned().collect();
        let mut cylinders: Vec<_> = faces.iter().map(|f| surf_of(f, tol).filter(|s| matches!(s, Surf::Cylinder { .. }))).collect();
        // A microscopic trim has too little angular span to identify a cylinder
        // from its boundary. An identical support already identified on a wider
        // patch supplies that classification without fitting the tiny patch.
        for i in 0..faces.len() {
            if cylinders.get(i).is_some_and(Option::is_some) {
                continue;
            }
            let known = faces.iter().enumerate().find_map(|(j, other)| {
                (faces.get(i)?.orientation() == other.orientation() && same_support(&faces.get(i)?.surface(), &other.surface()))
                    .then(|| cylinders.get(j).copied().flatten())
                    .flatten()
            });
            if let Some(slot) = cylinders.get_mut(i) {
                *slot = known;
            }
        }
        let mut parent: Vec<_> = (0..faces.len()).collect();
        let mut beside: HashMap<_, Vec<usize>> = HashMap::new();
        for (i, f) in faces.iter().enumerate() {
            for e in f.edge_iter() {
                beside.entry(e.id()).or_default().push(i);
            }
        }
        for e in Body::unique_edges(&Solid::try_new(vec![shell.clone()]).ok()?) {
            let Some(fs) = beside.get(&e.id()) else { continue };
            let [a, b] = fs.as_slice() else { continue };
            if crate::heal::on_split_line(&crate::heal::edge_points(&e), body.split_keep()) {
                continue;
            }
            if let (Some(Some(a_surface)), Some(Some(b_surface))) = (cylinders.get(*a), cylinders.get(*b))
                && same(*a_surface, *b_surface, tol)
            {
                let (a, b) = (crate::heal::find(&mut parent, *a), crate::heal::find(&mut parent, *b));
                if let Some(p) = parent.get_mut(a.max(b)) {
                    *p = a.min(b);
                }
            }
        }
        let mut groups: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        for i in 0..faces.len() {
            let root = crate::heal::find(&mut parent, i);
            groups.entry(root).or_default().push(i);
        }
        let mut merged_here = false;
        let mut out = Vec::new();
        let mut origins = Vec::new();
        for members in groups.values() {
            let first = *members.first()?;
            let selected = members.iter().filter_map(|i| faces.get(*i)).any(|f| f.edge_iter().any(|e| picked.contains(&e.id())));
            // Consolidate a support only when a microscopic trim would limit
            // the round. Ordinary cylinder-half seams keep their established
            // trimming domains (notably the cap of a hinge knuckle).
            let sliver = members.iter().filter_map(|i| faces.get(*i)).flat_map(|f| f.edge_iter()).any(|e| {
                let points = crate::heal::edge_points(&e);
                let length: f64 = points.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).sum();
                length > tol && length < body.size() * 1e-5
            });
            let merged =
                (members.len() > 1 && selected && sliver).then(|| merge_faces(&faces, members, *cylinders.get(first)?.as_ref()?, tol)).flatten();
            match merged {
                Some(face) => {
                    out.push(face);
                    origins.push(members.clone());
                    changed = true;
                    merged_here = true;
                }
                None => {
                    for i in members {
                        out.push(faces.get(*i)?.clone());
                        origins.push(vec![*i]);
                    }
                }
            }
        }
        // The seam removal leaves degree-two rim vertices; join their circular
        // pieces, including a chord whose whole error is below the healing tolerance.
        let out = if merged_here { join_rims(out, body.size() * 1e-7, body.split_keep(), &mut rec) } else { out };
        for (face, under) in out.iter().zip(&origins) {
            if under.len() > 1 {
                rec.made(face, under.iter().filter_map(|i| faces.get(*i)).map(|f| Was::Face(f.id())).collect());
            } else if let Some(old) = under.first().and_then(|i| faces.get(*i)) {
                rec.whole(face, old);
            }
        }
        shells.push(mt::Shell::from(out));
    }
    if !changed {
        return None;
    }
    let result = Solid::try_new(shells).ok()?;
    record::rebuilt(result, &[&solid], &rec).ok()
}

fn merge_faces(faces: &[mt::Face], members: &[usize], cylinder: Surf, tol: f64) -> Option<mt::Face> {
    let Surf::Cylinder { o, a, r, convex } = cylinder else { return None };
    let mut uses = HashMap::new();
    for i in members {
        for e in faces.get(*i)?.edge_iter() {
            *uses.entry(e.id()).or_insert(0usize) += 1;
        }
    }
    let mut rest: Vec<_> = members
        .iter()
        .filter_map(|i| faces.get(*i))
        .flat_map(|f| f.boundary_iters().into_iter().flatten())
        .filter(|e| uses.get(&e.id()) == Some(&1))
        .collect();
    let first = rest.pop()?;
    let (start, mut end) = (first.front().clone(), first.back().clone());
    let mut wire = vec![first];
    while end != start {
        let index = rest.iter().position(|e| e.front() == &end)?;
        let e = rest.remove(index);
        end = e.back().clone();
        wire.push(e);
    }
    // Several loops need a cylindrical containment classification. Keep them as
    // they were rather than joining unrelated regions or assigning a hole wrongly.
    if !rest.is_empty() {
        return None;
    }
    let heights: Vec<_> = wire.iter().flat_map(|e| [e.front(), e.back()]).map(|v| (from_p3(v.point()) - o).dot(a)).collect();
    let lo = heights.iter().copied().reduce(f64::min)?;
    let hi = heights.iter().copied().reduce(f64::max)?;
    if !lo.is_finite() || !hi.is_finite() || hi - lo <= tol {
        return None;
    }
    let radial = a.any_perp() * r;
    let (from, to) = if convex { (hi, lo) } else { (lo, hi) };
    let line = mt::Curve::Line(mt::Line(p3(o + radial + a * from), p3(o + radial + a * to)));
    let surface = mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(line, p3(o), v3(a))));
    mt::Face::try_new(vec![wire.into()], surface).ok()
}

fn join_rims(mut faces: Vec<mt::Face>, tol: f64, keep: &[Vec<Vec3>], rec: &mut Rebuild) -> Vec<mt::Face> {
    use mt::{BoundedCurve, ParametricCurve};
    for _ in 0..1000 {
        let mut vertex_at = HashMap::new();
        let mut vertices: Vec<Vec<mt::Edge>> = Vec::new();
        let mut seen = HashSet::new();
        for f in &faces {
            for e in f.edge_iter() {
                if seen.insert(e.id()) {
                    for v in [e.front(), e.back()] {
                        let index = *vertex_at.entry(v.id()).or_insert_with(|| {
                            vertices.push(Vec::new());
                            vertices.len() - 1
                        });
                        if let Some(edges) = vertices.get_mut(index) {
                            edges.push(e.clone());
                        }
                    }
                }
            }
        }
        let mut replacement = None;
        for es in &vertices {
            let [a, b] = es.as_slice() else { continue };
            if [a, b].iter().any(|e| crate::heal::on_split_line(&crate::heal::edge_points(e), keep)) {
                continue;
            }
            let shared = if a.front() == b.front() || a.front() == b.back() { a.front() } else { a.back() };
            let from = if a.front() == shared { a.back() } else { a.front() };
            let to = if b.front() == shared { b.back() } else { b.front() };
            if from == to {
                continue;
            }
            let leaving = |e: &mt::Edge| {
                let c = e.curve();
                let (lo, hi) = c.range_tuple();
                let d = if e.absolute_front() == shared { c.der(lo) } else { -c.der(hi) };
                Vec3::new(d.x, d.y, d.z).normalized()
            };
            if !leaving(a).zip(leaving(b)).is_some_and(|(a, b)| a.dot(b) < -1.0 + 1e-6) {
                continue;
            }
            let circle = crate::curveblend::circle_of(a, tol).or_else(|| crate::curveblend::circle_of(b, tol));
            let Some((center, radius, axis)) = circle else { continue };
            let on = |p: Vec3| (p.dist(center) - radius).abs() <= tol && (p - center).dot(axis).abs() <= tol;
            if ![a, b].iter().all(|e| crate::heal::edge_points(e).into_iter().all(on)) {
                continue;
            }
            // A well-separated transit point avoids constructing a circle from
            // three almost coincident points when one piece is a microscopic chord.
            let source =
                if crate::blend::vtx(a.front()).dist(crate::blend::vtx(a.back())) > crate::blend::vtx(b.front()).dist(crate::blend::vtx(b.back())) {
                    a
                } else {
                    b
                };
            let c = source.curve();
            let (lo, hi) = c.range_tuple();
            let transit = c.subs((lo + hi) * 0.5);
            let joined = builder::circle_arc(from, to, transit);
            rec.piece(&joined, a);
            rec.piece(&joined, b);
            replacement = Some((a.clone(), b.clone(), joined));
            break;
        }
        let Some((a, b, joined)) = replacement else { break };
        let rebuilt: Option<Vec<_>> = faces
            .iter()
            .map(|f| {
                let wires: Option<Vec<mt::Wire>> = f
                    .absolute_boundaries()
                    .iter()
                    .map(|w| {
                        let mut output = Vec::new();
                        let mut used = false;
                        for e in w.edge_iter() {
                            if e.id() == a.id() || e.id() == b.id() {
                                if !used {
                                    let forwards = e.front() == joined.front() || e.back() == joined.back();
                                    output.push(if forwards { joined.clone() } else { joined.inverse() });
                                    used = true;
                                }
                            } else {
                                output.push(e.clone());
                            }
                        }
                        Some(output.into())
                    })
                    .collect();
                crate::heal::absolute_face(f, wires?)
            })
            .collect();
        let Some(rebuilt) = rebuilt else { break };
        faces = rebuilt;
    }
    faces
}
