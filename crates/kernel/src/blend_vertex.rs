//! Fillets of the three edges at a convex vertex of any smooth faces, of one radius: the rounds
//! end in a piece of sphere, as they do at the corner of a box.
//!
//! The ball of the fillets' radius that touches the three faces there (its centre `c`, its
//! contacts `q`) ends each round: round `k` stops at the circle where the ball meets the plane
//! through `c` and the contacts of its two faces, and the sphere's patch joins the three circles.
//! The construction uses only what a round with a plane end does already:
//! 1. cut off the corner beyond the three planes (a skewed box whose three faces are those
//!    planes), so each of the three edges ends square on a plane face;
//! 2. round the three edges one after another, each ending in its plane face, which a round's
//!    end arc and the sides it uses up reduce to a quarter disc of the ball's section;
//! 3. replace the three quarter discs, which meet along radii at `c`, by the sphere's patch,
//!    which has the three end arcs for its boundary.

use std::collections::{HashMap, HashSet};

use solidcraft_geom::{Length, Vec3};
use truck_modeling::{self as mt};

use crate::blend::{edge_mid, too_large, unsupported, vtx};
use crate::body::{Body, Solid, from_p3, p3, v3};
use crate::curveblend::project;
use crate::record;
use crate::{BoolOp, KernelError, Result};

/// Round the edges nearest the points, `radius` each. The three edges meeting at a vertex that
/// all lie among them end in a sphere; the others are rounded as they would be alone.
pub(crate) fn fillet_ball_vertices(body: &Body, edges: &[Vec3], radius: Length) -> Result<Body> {
    body.require_brep("fillet")?;
    if edges.len() < 3 {
        return Err(unsupported("a vertex with three rounded edges"));
    }
    let counts = record::counts(&[body]);
    let mut cur = crate::boolean_record::healed(body)?;
    let mut todo: Vec<Vec3> = edges.to_vec();
    let mut done = false;
    while let Some((at, picks)) = find_vertex(&cur, &todo)? {
        let next = round_vertex(&cur, at, radius)?;
        cur = record::after(next, &cur, &counts);
        todo.retain(|p| !picks.iter().any(|q| q.dist(*p) < 1e-12));
        done = true;
    }
    if !done {
        return Err(unsupported("a vertex with three rounded edges"));
    }
    if todo.is_empty() { Ok(cur) } else { crate::blend::fillet(&cur, &todo, radius) }
}

/// A vertex of three edges all among the picked ones (its position) and their picks.
fn find_vertex(cur: &Body, picks: &[Vec3]) -> Result<Option<(Vec3, Vec<Vec3>)>> {
    let size = cur.size();
    let tol = (size * 2e-3).max(1e-3);
    let solid = cur.deep_copy();
    let all = Body::unique_edges(&solid);
    // The picked edges, by identity, with the pick each came from.
    let mut picked: Vec<(mt::EdgeID, Vec3)> = Vec::new();
    for p in picks {
        let Some((idx, dist)) = cur.nearest_edge(*p, tol)? else { return Ok(None) };
        if dist > size * 0.05 + 1e-3 {
            return Err(KernelError::Invalid(format!("no edge near {:?}", [p.x, p.y, p.z])));
        }
        if let Some(e) = all.get(idx)
            && !picked.iter().any(|(id, _)| *id == e.id())
        {
            picked.push((e.id(), *p));
        }
    }
    let mut at_vertex: HashMap<mt::VertexID, Vec<(mt::EdgeID, Vec3)>> = HashMap::new();
    for e in &all {
        let Some((_, p)) = picked.iter().find(|(id, _)| *id == e.id()) else { continue };
        for v in [e.absolute_front(), e.absolute_back()] {
            at_vertex.entry(v.id()).or_default().push((e.id(), *p));
        }
    }
    for (vid, list) in at_vertex {
        let degree = all.iter().filter(|e| e.absolute_front().id() == vid || e.absolute_back().id() == vid).count();
        if list.len() == 3 && degree == 3 {
            let v = all.iter().find_map(|e| [e.absolute_front(), e.absolute_back()].into_iter().find(|v| v.id() == vid));
            if let Some(v) = v {
                return Ok(Some((vtx(v), list.iter().map(|(_, p)| *p).collect())));
            }
        }
    }
    Ok(None)
}

/// The ball touching three faces from inside, near the corner `v`: its centre and contacts.
struct Ball {
    c: Vec3,
    /// Contact with each face, and the face's outward normal there.
    q: [Vec3; 3],
}

fn ball_at(faces: &[&mt::Face; 3], v: Vec3, r: f64, tol: f64) -> Result<Ball> {
    let rows = |n: [Vec3; 3]| [[n[0].x, n[0].y, n[0].z], [n[1].x, n[1].y, n[1].z], [n[2].x, n[2].y, n[2].z]];
    let near = |p: Vec3, hint: &mut [Option<(f64, f64)>; 3]| -> Option<([Vec3; 3], [Vec3; 3])> {
        let mut q = [Vec3::ZERO; 3];
        let mut n = [Vec3::ZERO; 3];
        for (i, f) in faces.iter().enumerate() {
            let (qi, ni, uv) = project(f, p, hint[i])?;
            (q[i], n[i], hint[i]) = (qi, ni, Some(uv));
        }
        Some((q, n))
    };
    let mut hint = [None; 3];
    let (_, n0) = near(v, &mut hint).ok_or_else(|| unsupported("the faces at the vertex"))?;
    // The planes' ball first: each face r inside its tangent plane at the vertex.
    let x = crate::polyhedron::solve3_pub(rows(n0), [-r, -r, -r]).ok_or_else(|| unsupported("a vertex whose faces do not meet in a corner"))?;
    let mut c = v + Vec3::new(x[0], x[1], x[2]);
    for _ in 0..60 {
        let (q, n) = near(c, &mut hint).ok_or_else(|| unsupported("the ball at the vertex"))?;
        let g = [(c - q[0]).dot(n[0]) + r, (c - q[1]).dot(n[1]) + r, (c - q[2]).dot(n[2]) + r];
        let d = crate::polyhedron::solve3_pub(rows(n), [-g[0], -g[1], -g[2]])
            .ok_or_else(|| unsupported("a vertex whose faces do not meet in a corner"))?;
        let dc = Vec3::new(d[0], d[1], d[2]);
        c += dc;
        if !c.is_finite() {
            break;
        }
        if dc.len() < tol * 1e-3 && g.iter().all(|x| x.abs() < tol) {
            let (q, n) = near(c, &mut hint).ok_or_else(|| unsupported("the ball at the vertex"))?;
            // A convex corner: the vertex lies out from every face the ball touches.
            if n.iter().any(|ni| (v - c).dot(*ni) < 0.25 * r) {
                return Err(unsupported("rounds at a corner that is not convex"));
            }
            return Ok(Ball { c, q });
        }
    }
    Err(unsupported("the ball does not fit at the vertex"))
}

/// Round the three edges at the vertex at `at`, ending in a sphere.
fn round_vertex(cur: &Body, at: Vec3, radius: Length) -> Result<Body> {
    let r = *radius;
    let size = cur.size();
    let tol = (size * 1e-7).max(1e-9);
    let solid = cur.deep_copy();
    let all = Body::unique_edges(&solid);
    let face_list: Vec<&mt::Face> = solid.face_iter().collect();
    // The vertex, its three edges and the faces between them.
    let v = all
        .iter()
        .flat_map(|e| [e.absolute_front().clone(), e.absolute_back().clone()])
        .find(|v| vtx(v).dist(at) <= size * 1e-9 + 1e-12)
        .ok_or_else(|| KernelError::Failed("fillet vertex".into()))?;
    let edges: Vec<mt::Edge> = all.iter().filter(|e| *e.absolute_front() == v || *e.absolute_back() == v).cloned().collect();
    let faces_of = |e: &mt::Edge| -> Vec<usize> {
        (0..face_list.len()).filter(|i| face_list.get(*i).is_some_and(|f| f.edge_iter().any(|x| x.id() == e.id()))).collect()
    };
    let mut around: Vec<usize> = Vec::new();
    for e in &edges {
        for f in faces_of(e) {
            if !around.contains(&f) {
                around.push(f);
            }
        }
    }
    let [fa, fb, fc] = around[..] else { return Err(unsupported("a vertex of three faces")) };
    let (Some(a), Some(b), Some(c)) = (face_list.get(fa), face_list.get(fb), face_list.get(fc)) else {
        return Err(KernelError::Failed("faces".into()));
    };
    let faces = [*a, *b, *c];
    let ball = ball_at(&faces, vtx(&v), r, tol * 100.0)?;
    // The plane of each edge's end: through the centre and the contacts of the edge's two faces,
    // its normal out toward the vertex.
    let mut normals: Vec<Vec3> = Vec::with_capacity(3);
    for e in &edges {
        let sides: Vec<usize> = faces_of(e).iter().filter_map(|f| around.iter().position(|x| x == f)).collect();
        let [i, j] = sides[..] else { return Err(unsupported("an edge not between two faces")) };
        let (Some(qi), Some(qj)) = (ball.q.get(i), ball.q.get(j)) else { return Err(KernelError::Failed("contacts".into())) };
        let m = (*qi - ball.c).cross(*qj - ball.c).normalized().ok_or_else(|| unsupported("a ball that touches an edge's faces at one point"))?;
        normals.push(if m.dot(vtx(&v) - ball.c) < 0.0 { -m } else { m });
    }
    let [m0, m1, m2] = normals[..] else { return Err(KernelError::Failed("planes".into())) };
    // The corner beyond the three planes, a skewed box from the centre along the three lines
    // where two of the planes meet, long enough to hold the corner of the body.
    let gen_ = |x: Vec3, y: Vec3, toward: Vec3| -> Result<Vec3> {
        let g = x.cross(y).normalized().ok_or_else(|| unsupported("planes that do not meet in a corner"))?;
        Ok(if g.dot(toward) < 0.0 { -g } else { g })
    };
    let reach = 3.0 * (vtx(&v).dist(ball.c) + r);
    let (g0, g1, g2) = (gen_(m1, m2, m0)? * reach, gen_(m2, m0, m1)? * reach, gen_(m0, m1, m2)? * reach);
    let unit = crate::build::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0))?;
    let m = [[g0.x, g0.y, g0.z, 0.0], [g1.x, g1.y, g1.z, 0.0], [g2.x, g2.y, g2.z, 0.0], [ball.c.x, ball.c.y, ball.c.z, 1.0]];
    let cutter = crate::ops::transform_matrix(&unit, m)?;
    let notched = crate::ops::boolean(cur, &cutter, BoolOp::Cut)?.ok_or_else(|| unsupported("a corner this small"))?;
    // The cut splits a curved face along the trace of its plane (the whole way across, not just
    // where the plane cuts): the pieces are one face again.
    let notched = crate::guard("merge split faces", || merge_split_faces(&notched, cur, size))?;
    // A point on what is left of each edge, half way from where its plane crosses it to its far end.
    let mut picks: Vec<Vec3> = Vec::with_capacity(3);
    for (e, m) in edges.iter().zip(&normals) {
        picks.push(remaining_pick(e, &v, ball.c, *m)?);
    }
    // Round them one after another, in an order that works.
    let mut last: Option<KernelError> = None;
    for order in [[0usize, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let mut step = notched.clone();
        let mut ok = true;
        for k in order {
            let Some(p) = picks.get(k) else { return Err(KernelError::Failed("pick".into())) };
            match crate::blend::fillet(&step, &[*p], radius) {
                Ok(b) => step = b,
                Err(e) => {
                    last = Some(e);
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            return close_with_sphere(&step, &ball, [m0, m1, m2], r, vtx(&v));
        }
    }
    Err(last.unwrap_or_else(|| unsupported("the three rounds at a vertex")))
}

/// A point on edge `e` between where the plane (through `c`, normal `m`) crosses it and its far
/// end (the end that is not `v`).
fn remaining_pick(e: &mt::Edge, v: &mt::Vertex, c: Vec3, m: Vec3) -> Result<Vec3> {
    use mt::{BoundedCurve, ParametricCurve};
    let curve = e.curve();
    let (lo, hi) = curve.range_tuple();
    // From the end at the vertex (outside the plane's far side) to the far end.
    let (near, far) = if e.absolute_front() == v { (lo, hi) } else { (hi, lo) };
    let side = |t: f64| (from_p3(curve.subs(t)) - c).dot(m);
    let (mut a, mut b) = (near, far);
    if !(side(a) > 0.0 && side(b) < 0.0) {
        return Err(too_large("the blend is larger than the neighbouring faces"));
    }
    for _ in 0..80 {
        let mid = 0.5 * (a + b);
        if side(mid) > 0.0 { a = mid } else { b = mid }
    }
    Ok(from_p3(curve.subs(0.5 * (0.5 * (a + b) + far))))
}

/// `step` has the three rounds ending in three quarter discs; the sphere's patch takes their place.
fn close_with_sphere(step: &Body, ball: &Ball, planes: [Vec3; 3], r: f64, v: Vec3) -> Result<Body> {
    let size = step.size();
    let on_tol = (size * 1e-6).max(1e-9);
    let solid = step.deep_copy();
    let faces: Vec<mt::Face> = solid.face_iter().cloned().collect();
    // The face in each plane: a plane face through the centre.
    let mut discs: Vec<usize> = Vec::new();
    for m in planes {
        let found = faces.iter().position(|f| {
            crate::blend::plane_normal(f).is_some_and(|n| n.dot(m).abs() > 1.0 - 1e-9)
                && f.vertex_iter().next().is_some_and(|p| (vtx(&p) - ball.c).dot(m).abs() <= on_tol * 10.0)
                && f.vertex_iter().any(|p| vtx(&p).dist(ball.c) <= on_tol * 10.0)
        });
        let Some(i) = found else { return Err(unsupported("the ends of the three rounds")) };
        discs.push(i);
    }
    // The arc of each: its one edge that is not a straight line.
    let mut arcs: Vec<mt::Edge> = Vec::with_capacity(3);
    for i in &discs {
        let f = faces.get(*i).ok_or_else(|| KernelError::Failed("face".into()))?;
        let curved: Vec<mt::Edge> = f.edge_iter().filter(|e| !matches!(e.curve(), mt::Curve::Line(_))).collect();
        let [arc] = &curved[..] else { return Err(unsupported("the ends of the three rounds")) };
        arcs.push(arc.absolute_clone());
    }
    // The three arcs in a loop.
    let first_arc = arcs.first().cloned().ok_or_else(|| KernelError::Failed("arc".into()))?;
    let mut used: HashSet<mt::EdgeID> = HashSet::from([first_arc.id()]);
    let mut wire: Vec<mt::Edge> = vec![first_arc];
    for _ in 0..2 {
        let end = wire.last().map(|e| e.back().clone()).ok_or_else(|| KernelError::Failed("arc".into()))?;
        let next = arcs
            .iter()
            .find(|a| !used.contains(&a.id()) && (*a.front() == end || *a.back() == end))
            .ok_or_else(|| unsupported("the ends of the three rounds"))?;
        used.insert(next.id());
        wire.push(if *next.front() == end { next.clone() } else { next.inverse() });
    }
    if wire.last().map(|e| e.back().clone()) != wire.first().map(|e| e.front().clone()) {
        return Err(unsupported("the ends of the three rounds"));
    }
    // A sphere about the centre, its poles and seam away from the patch.
    let out = (v - ball.c).normalized().ok_or_else(|| KernelError::Failed("sphere".into()))?;
    let side = out.any_perp();
    let (np, sp, back) = (ball.c + side * r, ball.c - side * r, ball.c - out * r);
    let meridian = mt::builder::circle_arc(&mt::builder::vertex(p3(np)), &mt::builder::vertex(p3(sp)), p3(back));
    let mut surf =
        mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(meridian.oriented_curve(), p3(ball.c), v3(side))));
    {
        use mt::{ParametricSurface3D, SearchNearestParameter};
        if let Some((u, w)) = surf.search_nearest_parameter(p3(ball.c + out * r), None, 100) {
            let n = surf.normal(u, w);
            if Vec3::new(n.x, n.y, n.z).dot(out) < 0.0 {
                surf = mt::Invertible::inverse(&surf);
            }
        }
    }
    // The patch runs each arc opposite to the round that ends on it.
    let rest: Vec<&mt::Face> = faces.iter().enumerate().filter(|(i, _)| !discs.contains(i)).map(|(_, f)| f).collect();
    let uses = |id: mt::EdgeID| {
        rest.iter().find_map(|f| {
            f.boundaries().iter().flat_map(|w| w.edge_iter().cloned().collect::<Vec<_>>()).find(|x| x.id() == id).map(|x| x.front().id())
        })
    };
    if let (Some(first), Some(front)) = (wire.first(), wire.first().and_then(|e| uses(e.id())))
        && front == first.front().id()
    {
        wire = wire.iter().rev().map(|e| e.inverse()).collect();
    }
    let patch = mt::Face::try_new(vec![mt::Wire::from(wire)], surf).map_err(|e| KernelError::Failed(format!("sphere patch: {e}")))?;
    let mut new_faces: Vec<mt::Face> = rest.into_iter().cloned().collect();
    new_faces.push(patch);
    let mut shells = solid.boundaries().clone();
    let si = shells.iter().position(|sh| sh.face_iter().any(|f| discs.iter().any(|i| faces.get(*i).is_some_and(|d| d.id() == f.id())))).unwrap_or(0);
    if let Some(sh) = shells.get_mut(si) {
        *sh = new_faces.into();
    }
    let out: Solid = Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("rounds ending in a sphere: {e}")))?;
    record::rebuilt(out, &[&step.solid], &record::Rebuild::default())
}

/// `body` with the curved faces that are pieces of one input face (as the cut that made `body`
/// recorded them) joined again: the edges two pieces share go, the rest make the one boundary.
fn merge_split_faces(body: &Body, source: &Body, size: f64) -> Result<Body> {
    let tol = (size * 1e-6).max(1e-9);
    let Some(history) = body.history() else { return Ok(body.clone()) };
    let solid = body.deep_copy();
    let faces: Vec<mt::Face> = solid.face_iter().cloned().collect();
    if history.faces.len() != faces.len() {
        return Ok(body.clone());
    }
    let on = |a: &mt::Face, p: Vec3| project(a, p, None).is_some_and(|(q, _, uv)| crate::curveblend::in_range(a, uv) && q.dist(p) <= tol * 10.0);
    // Pieces by the input face they come from.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, f) in history.faces.iter().enumerate() {
        let curved = faces.get(i).is_some_and(|x| crate::blend::plane_normal(x).is_none());
        if !(curved && f.change == crate::history::Change::Split && f.origins.len() == 1) {
            continue;
        }
        match groups.iter_mut().find(|g| g.first().and_then(|k| history.faces.get(*k)).is_some_and(|o| o.origins == f.origins)) {
            Some(g) => g.push(i),
            None => groups.push(vec![i]),
        }
    }
    let mut out: Vec<Option<mt::Face>> = faces.iter().cloned().map(Some).collect();
    let mut merged_any = false;
    for group in groups.iter().filter(|g| g.len() >= 2) {
        let pieces: Vec<&mt::Face> = group.iter().filter_map(|i| faces.get(*i)).collect();
        if pieces.iter().any(|f| f.boundaries().len() != 1) {
            continue;
        }
        // Edges used once, as the pieces walk them.
        let all: Vec<mt::Edge> =
            pieces.iter().flat_map(|f| f.boundaries().iter().flat_map(|w| w.edge_iter().cloned().collect::<Vec<_>>()).collect::<Vec<_>>()).collect();
        let mut uses: HashMap<mt::EdgeID, usize> = HashMap::new();
        for e in &all {
            *uses.entry(e.id()).or_default() += 1;
        }
        let mut left: Vec<mt::Edge> = all.into_iter().filter(|e| uses.get(&e.id()) == Some(&1)).collect();
        let Some(start) = left.pop() else { continue };
        let mut wire = vec![start];
        while let Some(end) = wire.last().map(|e| e.back().clone()) {
            if wire.first().is_some_and(|e| *e.front() == end) {
                break;
            }
            let Some(at) = left.iter().position(|e| *e.front() == end) else { break };
            wire.push(left.swap_remove(at));
        }
        // One closed boundary, all of it used.
        if !left.is_empty() || wire.first().map(|e| e.front().clone()) != wire.last().map(|e| e.back().clone()) {
            continue;
        }
        // A boolean may restrict a piece's surface to its own parameter box. The
        // joined boundary needs the original face's full patch, not an extrapolation
        // of one piece (which the bounded display planner correctly refuses).
        let all_on = |f: &mt::Face| {
            pieces.iter().all(|p| p.vertex_iter().all(|v| on(f, vtx(&v))) && p.edge_iter().all(|e| edge_mid(&e).is_none_or(|m| on(f, m))))
        };
        let original = group.first().and_then(|i| history.faces.get(*i)).and_then(|h| match h.origins.as_slice() {
            [crate::history::FaceOrigin::Face { operand: 0, face }] => source.solid.face_iter().nth(*face),
            _ => None,
        });
        let Some(like) = original.filter(|f| all_on(f)).or_else(|| pieces.iter().copied().find(|f| all_on(f))) else { continue };
        let Ok(joined) = mt::Face::try_new(vec![mt::Wire::from(wire)], like.oriented_surface()) else { continue };
        let Some(first) = group.first() else { continue };
        for i in group {
            if let Some(slot) = out.get_mut(*i) {
                *slot = None;
            }
        }
        if let Some(slot) = out.get_mut(*first) {
            *slot = Some(joined);
        }
        merged_any = true;
    }
    if !merged_any {
        return Ok(body.clone());
    }
    let mut shells = solid.boundaries().clone();
    if let Some(sh) = shells.first_mut() {
        *sh = out.into_iter().flatten().collect::<Vec<_>>().into();
    }
    let rebuilt = Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("a corner cut: {e}")))?;
    record::rebuilt(rebuilt, &[&body.solid], &record::Rebuild::default())
}
