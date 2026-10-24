//! Clean-up after operations: coplanar neighbouring faces become one face, and two straight
//! edges meeting in line at a vertex nothing else uses become one edge (they are kept as one).

use std::collections::{BTreeSet, HashMap};

use solidcraft_geom::{Ring, Vec3};
use truck_modeling::{self as mt, builder};

use crate::body::{Solid, from_p3};
use crate::history::{Change, EdgeOrigin, FaceProv};

fn plane_of(f: &mt::Face) -> Option<(Vec3, f64)> {
    match f.oriented_surface() {
        mt::Surface::Plane(p) => {
            let n = p.normal();
            let n = Vec3::new(n.x, n.y, n.z).normalized()?;
            let o = p.origin();
            Some((n, n.dot(Vec3::new(o.x, o.y, o.z))))
        }
        _ => None,
    }
}

/// A face like `f` with new boundary wires given in `f`'s absolute (stored) orientation:
/// built on the stored surface and turned like `f` (rebuilding on the turned surface can make
/// the mesher pick the wrong side of a trim on periodic surfaces).
pub(crate) fn absolute_face(f: &mt::Face, wires: Vec<mt::Wire>) -> Option<mt::Face> {
    let mut nf = mt::Face::try_new(wires, f.surface()).ok()?;
    if !f.orientation() {
        nf.invert();
    }
    Some(nf)
}

pub(crate) fn find(p: &mut [usize], mut i: usize) -> usize {
    while let Some(&q) = p.get(i) {
        if q == i {
            break;
        }
        i = q;
    }
    i
}

/// Points along an oriented edge (for loop areas and containment).
fn samples(e: &mt::Edge) -> Vec<Vec3> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.oriented_curve();
    let (t0, t1) = c.range_tuple();
    (0..8).map(|k| t0 + (t1 - t0) * k as f64 / 8.0).map(|t| from_p3(c.subs(t))).collect()
}

fn loop_area(edges: &[mt::Edge], n: Vec3) -> f64 {
    edges.iter().flat_map(samples).collect::<Ring<Vec3>>().area_vector().dot(n)
}

fn inside(p: Vec3, edges: &[mt::Edge], n: Vec3) -> bool {
    let u = n.any_perp();
    let w = n.cross(u);
    let pts: Vec<(f64, f64)> = edges.iter().flat_map(samples).map(|q| (q.dot(u), q.dot(w))).collect();
    let (px, py) = (p.dot(u), p.dot(w));
    let k = pts.len();
    let mut c = false;
    for i in 0..k {
        let (Some(&(ax, ay)), Some(&(bx, by))) = (pts.get(i), pts.get((i + 1) % k)) else { continue };
        if (ay > py) != (by > py) && px < ax + (py - ay) / (by - ay) * (bx - ax) {
            c = !c;
        }
    }
    c
}

/// Do the points lie along one of the lines faces were split along on purpose (polylines;
/// chords of a curved line sag by under a tenth of their length)?
pub(crate) fn on_split_line(pts: &[Vec3], keep: &[Vec<Vec3>]) -> bool {
    !pts.is_empty()
        && keep.iter().any(|line| {
            let seg = line.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).fold(0.0, f64::max);
            let tol = 1e-6 * (1.0 + seg) + if line.len() > 2 { seg * 0.1 } else { 0.0 };
            pts.iter().all(|q| line.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| q.dist_to_segment(*a, *b) <= tol)))
        })
}

/// Points along an edge (ends and three inside).
pub(crate) fn edge_points(e: &mt::Edge) -> Vec<Vec3> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.curve();
    let (t0, t1) = c.range_tuple();
    (0..=4).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 4.0))).collect()
}

/// Merge each group of edge-connected coplanar faces into one face (keeping faces apart along
/// the lines in `keep`). Returns the new shell and, for each of its faces, the faces of `shell`
/// it was made from (by position); None when nothing changed or a merged face can't be built.
fn merge_coplanar(shell: &mt::Shell, tol: f64, keep: &[Vec<Vec3>]) -> Option<(mt::Shell, Vec<Vec<usize>>)> {
    let faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
    let planes: Vec<Option<(Vec3, f64)>> = faces.iter().map(plane_of).collect();
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    let mut by_edge: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for e in f.edge_iter() {
            by_edge.entry(e.id()).or_default().push(i);
        }
    }
    let mut merged_any = false;
    let edges: HashMap<mt::EdgeID, mt::Edge> = shell.edge_iter().map(|e| (e.id(), e)).collect();
    for (id, fs) in &by_edge {
        if let [a, b] = fs[..]
            && a != b
            && (keep.is_empty() || !edges.get(id).is_some_and(|e| on_split_line(&edge_points(e), keep)))
            && let (Some(Some((na, da))), Some(Some((nb, db)))) = (planes.get(a), planes.get(b))
            && na.dot(*nb) > 1.0 - 1e-9
            && (da - db).abs() < tol
        {
            // The lesser root wins, so each group's root is its first face whatever order the
            // edges (in hash order of their addresses) are met in (#707).
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                if let Some(x) = parent.get_mut(ra.max(rb)) {
                    *x = ra.min(rb);
                }
                merged_any = true;
            }
        }
    }
    if !merged_any {
        return None;
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..faces.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<mt::Face> = Vec::new();
    let mut from: Vec<Vec<usize>> = Vec::new();
    let mut order: Vec<usize> = groups.keys().copied().collect();
    order.sort_unstable();
    for g in order {
        let members = groups.get(&g)?;
        if members.len() == 1 {
            out.push(faces.get(*members.first()?)?.clone());
            from.push(members.clone());
            continue;
        }
        let (n, _) = (*planes.get(*members.first()?)?)?;
        // Oriented boundary edges of the group; edges used twice inside it are dropped.
        let mut count: HashMap<mt::EdgeID, usize> = HashMap::new();
        for m in members {
            for e in faces.get(*m)?.boundary_iters().into_iter().flatten() {
                *count.entry(e.id()).or_insert(0) += 1;
            }
        }
        let mut rest: Vec<mt::Edge> = Vec::new();
        for m in members {
            for e in faces.get(*m)?.boundary_iters().into_iter().flatten() {
                if count.get(&e.id()).copied().unwrap_or(0) == 1 {
                    rest.push(e);
                }
            }
        }
        // Chain into loops.
        let mut loops: Vec<Vec<mt::Edge>> = Vec::new();
        while let Some(first) = rest.pop() {
            let start = first.front().clone();
            let mut cur = first.back().clone();
            let mut lp = vec![first];
            let mut guard = 0;
            while cur != start && guard < 100_000 {
                guard += 1;
                let k = rest.iter().position(|e| e.front() == &cur)?;
                let e = rest.remove(k);
                cur = e.back().clone();
                lp.push(e);
            }
            loops.push(lp);
        }
        let (outer, holes): (Vec<Vec<mt::Edge>>, Vec<Vec<mt::Edge>>) = loops.into_iter().partition(|l| loop_area(l, n) > 0.0);
        let surface = faces.get(*members.first()?)?.oriented_surface();
        for o in &outer {
            let mut wires: Vec<mt::Wire> = vec![o.clone().into()];
            for h in &holes {
                let probe = h.first().map(|e| from_p3(e.front().point()))?;
                // The innermost outer loop around the hole owns it.
                let owner = outer.iter().filter(|x| inside(probe, x, n)).min_by(|a, b| loop_area(a, n).total_cmp(&loop_area(b, n)));
                if owner.is_some_and(|x| std::ptr::eq(x, o)) {
                    wires.push(h.clone().into());
                }
            }
            out.push(mt::Face::try_new(wires, surface.clone()).ok()?);
            from.push(members.clone());
        }
    }
    Some((out.into(), from))
}

/// Join runs of collinear straight edges at vertices only they use: each run becomes one edge,
/// every face it borders is rebuilt once (linear in the shell, however long the runs; a faceted
/// body such as a modeled thread has thousands of such edges).
fn merge_collinear(shell: &mt::Shell, trace: &mut Trace) -> Option<mt::Shell> {
    let mut faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
    let mut edges: Vec<mt::Edge> = Vec::new();
    let mut index: HashMap<mt::EdgeID, usize> = HashMap::new();
    let mut at: HashMap<mt::VertexID, Vec<usize>> = HashMap::new();
    for f in &faces {
        for e in f.edge_iter() {
            if index.contains_key(&e.id()) {
                continue;
            }
            index.insert(e.id(), edges.len());
            at.entry(e.front().id()).or_default().push(edges.len());
            at.entry(e.back().id()).or_default().push(edges.len());
            edges.push(e.absolute_clone());
        }
    }
    // Edges that continue each other in line through a vertex nothing else uses.
    let mut parent: Vec<usize> = (0..edges.len()).collect();
    let mut any = false;
    for (v, es) in &at {
        let [ia, ib] = es[..] else { continue };
        let (Some(a), Some(b)) = (edges.get(ia), edges.get(ib)) else { continue };
        if !matches!(a.curve(), mt::Curve::Line(_)) || !matches!(b.curve(), mt::Curve::Line(_)) {
            continue;
        }
        let other = |e: &mt::Edge| if e.front().id() == *v { e.back().clone() } else { e.front().clone() };
        let mid = if a.front().id() == *v { a.front().clone() } else { a.back().clone() };
        let (pa, pm, pb) = (from_p3(other(a).point()), from_p3(mid.point()), from_p3(other(b).point()));
        let (Some(u), Some(w)) = ((pm - pa).normalized(), (pb - pm).normalized()) else { continue };
        if u.dot(w) > 1.0 - 1e-10 {
            // The lesser root wins: each run's root is its first edge whatever order the
            // vertices are met in (#707).
            let (ra, rb) = (find(&mut parent, ia), find(&mut parent, ib));
            if ra != rb
                && let Some(x) = parent.get_mut(ra.max(rb))
            {
                *x = ra.min(rb);
                any = true;
            }
        }
    }
    if !any {
        return None;
    }
    // Ordered by run (and each run's ends by first use below): which end a joined edge starts
    // from must not follow hash order, or the same heal gives a different body from run to run.
    let mut runs: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..edges.len() {
        let r = find(&mut parent, i);
        runs.entry(r).or_default().push(i);
    }
    // Each run's joined edge, and the position of its vertices along it.
    let mut joined: Vec<(mt::Edge, HashMap<mt::VertexID, usize>)> = Vec::new();
    let mut run_of: HashMap<mt::EdgeID, usize> = HashMap::new();
    for members in runs.values().filter(|m| m.len() > 1) {
        let mut ends: Vec<(mt::VertexID, usize, mt::Vertex)> = Vec::new();
        for i in members {
            let Some(e) = edges.get(*i) else { continue };
            for v in [e.front(), e.back()] {
                match ends.iter_mut().find(|(id, ..)| *id == v.id()) {
                    Some(end) => end.1 += 1,
                    None => ends.push((v.id(), 1, v.clone())),
                }
            }
        }
        let mut ends = ends.into_iter().filter(|(_, n, _)| *n == 1).map(|(.., v)| v);
        let (Some(first), Some(last), None) = (ends.next(), ends.next(), ends.next()) else { continue };
        // Walk the run from one end, collecting its vertices in order.
        let mut order = vec![first.id()];
        let (mut cur, mut prev) = (first.id(), usize::MAX);
        for _ in 0..members.len() {
            let next = at.get(&cur).and_then(|es| es.iter().copied().find(|i| *i != prev && members.contains(i)));
            let Some(e) = next.and_then(|i| edges.get(i).map(|e| (i, e))) else { break };
            cur = if e.1.front().id() == cur { e.1.back().id() } else { e.1.front().id() };
            prev = e.0;
            order.push(cur);
        }
        if order.last() != Some(&last.id()) || order.len() != members.len() + 1 {
            continue;
        }
        // Every piece must lie along the whole run, not just along its neighbour.
        let Some(dir) = (from_p3(last.point()) - from_p3(first.point())).normalized() else { continue };
        let straight = members
            .iter()
            .filter_map(|i| edges.get(*i))
            .all(|e| (from_p3(e.back().point()) - from_p3(e.front().point())).normalized().is_some_and(|d| d.dot(dir).abs() > 1.0 - 1e-10));
        if !straight {
            continue;
        }
        let line = builder::line(&first, &last);
        trace.made_of(&line, members.iter().filter_map(|i| edges.get(*i)).map(|e| e.id()));
        for i in members {
            if let Some(e) = edges.get(*i) {
                run_of.insert(e.id(), joined.len());
            }
        }
        joined.push((line, order.into_iter().enumerate().map(|(k, v)| (v, k)).collect()));
    }
    if joined.is_empty() {
        return None;
    }
    for f in &mut faces {
        if !f.edge_iter().any(|e| run_of.contains_key(&e.id())) {
            continue;
        }
        let mut wires = Vec::new();
        for w in f.absolute_boundaries() {
            let mut done = std::collections::HashSet::new();
            let mut es: Vec<mt::Edge> = Vec::new();
            for e in w.edge_iter() {
                let Some(r) = run_of.get(&e.id()).copied() else {
                    es.push(e.clone());
                    continue;
                };
                // The run's pieces become its joined edge, in this wire's direction: once each
                // way, as a seam runs along its wire both ways.
                if let Some((line, pos)) = joined.get(r) {
                    let forward = pos.get(&e.front().id()) < pos.get(&e.back().id());
                    if done.insert((r, forward)) {
                        es.push(if forward { line.clone() } else { line.inverse() });
                    }
                }
            }
            wires.push(mt::Wire::from(es));
        }
        *f = absolute_face(f, wires)?;
    }
    Some(faces.into())
}

/// The exact curve an approximate edge (a boolean's intersection curve) really is: a line or a
/// circular arc, when every sample lies on it.
fn exact_curve(e: &mt::Edge, tol: f64) -> Option<mt::Edge> {
    // Only the approximate curves booleans make; exact ones stay as they are.
    if !matches!(e.curve(), mt::Curve::IntersectionCurve(_)) {
        return None;
    }
    fit_exact(e, tol)
}

/// A line or circular arc through the edge's samples, if they all lie on one.
fn fit_exact(e: &mt::Edge, tol: f64) -> Option<mt::Edge> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.curve();
    let (t0, t1) = c.range_tuple();
    let pts: Vec<Vec3> = (0..=16).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0))).collect();
    let (a, b) = (from_p3(e.absolute_front().point()), from_p3(e.absolute_back().point()));
    if e.absolute_front() == e.absolute_back() || a.dist(b) < tol {
        return None;
    }
    if pts.iter().all(|p| p.dist_to_segment(a, b) < tol) {
        return Some(builder::line(e.absolute_front(), e.absolute_back()));
    }
    // A circle through the ends and the middle sample.
    let m = *pts.get(8)?;
    let n = (m - a).cross(b - a).normalized()?;
    let (ab, am) = (b - a, m - a);
    // Centre in the plane: solve |c-a| = |c-b| = |c-m|.
    let d = 2.0 * ab.cross(am).len2();
    if d < 1e-300 {
        return None;
    }
    // Circumcentre: a + (|u|² v×w + |v|² w×u) / (2|w|²), u = b−a, v = m−a, w = u×v.
    let w = ab.cross(am);
    let centre = a + (am.cross(w) * ab.len2() + w.cross(ab) * am.len2()) * (1.0 / d);
    let r = centre.dist(a);
    if pts.iter().all(|p| (p.dist(centre) - r).abs() < tol && (*p - a).dot(n).abs() < tol) {
        return Some(builder::circle_arc(e.absolute_front(), e.absolute_back(), mt::Point3::new(m.x, m.y, m.z)));
    }
    None
}

/// Replace approximate edges that are really lines or arcs with exact ones. Faces with none of
/// them stay as they are (an imported face around its seam, whose boundary runs through a vertex
/// twice, cannot be rebuilt, #3085); a face that cannot be rebuilt with its new edges keeps its
/// old ones, and so do its neighbours along them.
fn exact_edges(shell: &mt::Shell, tol: f64, trace: &mut Trace) -> Option<mt::Shell> {
    let mut subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
    for e in shell.edge_iter() {
        if !subst.contains_key(&e.id())
            && let Some(x) = exact_curve(&e, tol)
        {
            subst.insert(e.id(), x);
        }
    }
    // Each refusal takes edges out, so this ends; the cap only bounds the work.
    for _ in 0..8 {
        if subst.is_empty() {
            return None;
        }
        match rebuilt_with(shell, &subst) {
            Ok(faces) => {
                for (id, x) in &subst {
                    trace.made(x, [*id]);
                }
                return Some(faces.into());
            }
            Err(kept) => subst.retain(|id, _| !kept.contains(id)),
        }
    }
    None
}

/// The faces of `shell` with the edges in `subst` replaced, or the edges of the faces that
/// cannot be rebuilt with them.
fn rebuilt_with(shell: &mt::Shell, subst: &HashMap<mt::EdgeID, mt::Edge>) -> Result<Vec<mt::Face>, Vec<mt::EdgeID>> {
    let mut faces = Vec::new();
    let mut kept = Vec::new();
    for f in shell.face_iter() {
        if !f.edge_iter().any(|e| subst.contains_key(&e.id())) {
            faces.push(f.clone());
            continue;
        }
        let mut wires = Vec::new();
        for w in f.absolute_boundaries() {
            let es: Vec<mt::Edge> = w
                .edge_iter()
                .map(|e| match subst.get(&e.id()) {
                    // The new edge runs absolute front → back; match this use's direction.
                    Some(n) => {
                        if e.front() == e.absolute_front() {
                            n.clone()
                        } else {
                            n.inverse()
                        }
                    }
                    None => e.clone(),
                })
                .collect();
            wires.push(mt::Wire::from(es));
        }
        match absolute_face(f, wires) {
            Some(nf) => faces.push(nf),
            None => kept.extend(f.edge_iter().map(|e| e.id()).filter(|id| subst.contains_key(id))),
        }
    }
    if kept.is_empty() { Ok(faces) } else { Err(kept) }
}

/// Faces whose surface is flat but not a plane (a revolved line square to the axis, a swept
/// line) get a real plane, so they merge with their coplanar neighbours and blend like planes.
pub(crate) fn planar_surfaces(shell: &mt::Shell, tol: f64) -> Option<mt::Shell> {
    use mt::{BoundedCurve, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};
    let mut changed = false;
    let mut faces = Vec::new();
    for f in shell.face_iter() {
        let surf = f.surface();
        if matches!(surf, mt::Surface::Plane(_)) {
            faces.push(f.clone());
            continue;
        }
        // Parameter box of the boundary, then a grid of surface points in it.
        let mut uv: Vec<(f64, f64)> = Vec::new();
        let mut pts: Vec<Vec3> = Vec::new();
        for w in f.absolute_boundaries() {
            for e in w.edge_iter() {
                let c = e.oriented_curve();
                let (t0, t1) = c.range_tuple();
                for k in 0..6 {
                    let p = c.subs(t0 + (t1 - t0) * k as f64 / 6.0);
                    pts.push(from_p3(p));
                    if let Some(x) = surf.search_nearest_parameter(p, uv.last().copied(), 50) {
                        uv.push(x);
                    }
                }
            }
        }
        if uv.len() < 3 || pts.len() < 3 {
            faces.push(f.clone());
            continue;
        }
        let (u0, u1) = uv.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, x| (a.0.min(x.0), a.1.max(x.0)));
        let (v0, v1) = uv.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, x| (a.0.min(x.1), a.1.max(x.1)));
        for i in 0..=4 {
            for j in 0..=4 {
                pts.push(from_p3(surf.subs(u0 + (u1 - u0) * i as f64 / 4.0, v0 + (v1 - v0) * j as f64 / 4.0)));
            }
        }
        // The plane through the points (normal from the surface).
        let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        let nn = surf.normal(um, vm);
        let Some(n) = Vec3::new(nn.x, nn.y, nn.z).normalized() else {
            faces.push(f.clone());
            continue;
        };
        let o = from_p3(surf.subs(um, vm));
        if pts.iter().any(|p| (*p - o).dot(n).abs() > tol) {
            faces.push(f.clone());
            continue;
        }
        let u = n.any_perp();
        let w = n.cross(u);
        let plane = mt::Plane::new(crate::body::p3(o), crate::body::p3(o + u), crate::body::p3(o + w));
        match mt::Face::try_new(f.absolute_boundaries().clone(), mt::Surface::Plane(plane)) {
            Ok(mut nf) => {
                if !f.orientation() {
                    nf.invert();
                }
                faces.push(nf);
                changed = true;
            }
            Err(_) => faces.push(f.clone()),
        }
    }
    changed.then(|| faces.into())
}

/// Heal a solid; returns the input unchanged when nothing applies or healing fails.
#[cfg(test)]
pub fn heal(solid: Solid, size: f64) -> Solid {
    healed(solid, size, &[], false, false).0
}

/// What healing made each face and edge of its result from.
#[derive(Debug)]
pub(crate) struct HealMap {
    /// For each face of the result (in its face order), the input faces it was made from (by
    /// position in the input's face order): one, or several merged into it.
    pub faces: Vec<Vec<usize>>,
    /// For each edge of the result (by identity), the input edges it was made from (by
    /// position in the input's edge order): one, or several joined into it.
    pub edges: HashMap<mt::EdgeID, Vec<usize>>,
}

impl HealMap {
    /// The history of the healed solid, from that of its input (`faces` in the input's face
    /// order, each a region of operand faces; `edges` the origins of each edge, in its edge
    /// order): a face healing merged from several is Merged when they came from several, the
    /// rest keep their origins (see [`crate::record::regions`]). Faces in the healed solid's
    /// face order, edges by identity.
    pub(crate) fn carry(&self, faces: &[FaceProv], edges: &[Vec<EdgeOrigin>]) -> Option<(Vec<FaceProv>, HashMap<mt::EdgeID, Vec<EdgeOrigin>>)> {
        let regions = self
            .faces
            .iter()
            .map(|under| {
                let whole = matches!(under.as_slice(), [i] if faces.get(*i).is_some_and(|f| f.change == Change::Kept));
                let mut origins = BTreeSet::new();
                for i in under {
                    origins.extend(faces.get(*i)?.origins.iter().copied());
                }
                Some((origins, whole))
            })
            .collect::<Option<Vec<_>>>()?;
        let edges = self
            .edges
            .iter()
            .map(|(id, under)| {
                let mut origins = BTreeSet::new();
                for i in under {
                    origins.extend(edges.get(*i)?.iter().cloned());
                }
                Some((*id, origins.into_iter().collect()))
            })
            .collect::<Option<HashMap<_, _>>>()?;
        Some((crate::record::regions(regions), edges))
    }

    /// Every face and edge of `solid` as itself (nothing healed).
    fn identity(solid: &Solid) -> HealMap {
        let faces = (0..solid.face_iter().count()).map(|i| vec![i]).collect();
        let edges = crate::record::edge_index(solid).into_iter().map(|(id, i)| (id, vec![i])).collect();
        HealMap { faces, edges }
    }
}

/// Heal a solid (the input unchanged when nothing applies or healing fails), keeping faces apart
/// along lines they were split along on purpose (`keep`), with what each face and edge of the
/// result was made from (`None` when that could not be followed).
pub(crate) fn heal_traced(solid: Solid, size: f64, keep: &[Vec<Vec3>]) -> (Solid, Option<HealMap>) {
    healed(solid, size, keep, true, false)
}

/// [`heal_traced`] that only merges coplanar neighbouring faces and joins the straight edges
/// they leave split in line (#1049), leaving every curve and surface as it is: for results
/// whose geometry is already exact (a prism boolean's).
pub(crate) fn merge_faces_traced(solid: Solid, size: f64, keep: &[Vec<Vec3>]) -> (Solid, Option<HealMap>) {
    healed(solid, size, keep, true, true)
}

/// The healed solid, and what it was made from when `traced`.
fn healed(solid: Solid, size: f64, keep: &[Vec<Vec3>], traced: bool, faces_only: bool) -> (Solid, Option<HealMap>) {
    // Edges a boolean made are intersection curves, which panic where they no longer
    // evaluate; the solid then stays as it is.
    match crate::guard("heal", || Ok(heal_shells(&solid, size, keep, traced, faces_only))) {
        Ok(Some((healed, map))) => (healed, map),
        _ => {
            let map = if traced { crate::guard("heal map", || Ok(HealMap::identity(&solid))).ok() } else { None };
            (solid, map)
        }
    }
}

/// The input edges each edge made while healing comes from. Input edges live as long as the
/// healing (the input solid is borrowed throughout), so their identities are never reused; an
/// edge healing makes is recorded when it is made, from edges then alive, which overwrites any
/// entry left under its identity by an edge made earlier and dropped since. Records nothing
/// when not `on`.
struct Trace {
    on: bool,
    input: HashMap<mt::EdgeID, usize>,
    made: HashMap<mt::EdgeID, Vec<usize>>,
}

impl Trace {
    /// The input edges under the live edge `id`.
    fn of(&self, id: &mt::EdgeID) -> Vec<usize> {
        match self.made.get(id) {
            Some(from) => from.clone(),
            None => self.input.get(id).map(|i| vec![*i]).unwrap_or_default(),
        }
    }

    /// `edge` was made from the live edges `from`.
    fn made<const N: usize>(&mut self, edge: &mt::Edge, from: [mt::EdgeID; N]) {
        self.made_of(edge, from);
    }

    /// `edge` was made from the live edges `from`, however many.
    fn made_of(&mut self, edge: &mt::Edge, from: impl IntoIterator<Item = mt::EdgeID>) {
        if !self.on {
            return;
        }
        let mut under: Vec<usize> = from.into_iter().flat_map(|id| self.of(&id)).collect();
        under.sort_unstable();
        under.dedup();
        self.made.insert(edge.id(), under);
    }
}

fn heal_shells(solid: &Solid, size: f64, keep: &[Vec<Vec3>], traced: bool, faces_only: bool) -> Option<(Solid, Option<HealMap>)> {
    let tol = (size * 1e-7).max(1e-9);
    let mut shells = Vec::new();
    let mut changed = false;
    let input = if traced { crate::record::edge_index(solid) } else { HashMap::new() };
    let mut trace = Trace { on: traced, input, made: HashMap::new() };
    // Each face of the shells so far, as the input faces it was made from.
    let mut faces: Vec<Vec<usize>> = Vec::new();
    let mut first = 0;
    for sh in solid.boundaries() {
        let mut cur = sh.clone();
        let count = cur.face_iter().count();
        let mut from: Vec<Vec<usize>> = (first..first + count).map(|i| vec![i]).collect();
        first += count;
        // Faces come out in place, one for one, except where coplanar ones merge.
        if !faces_only && let Some(m) = planar_surfaces(&cur, (size * 1e-7).max(1e-9)) {
            cur = m;
            changed = true;
        }
        if !faces_only && let Some(m) = exact_edges(&cur, (size * 1e-5).max(1e-7), &mut trace) {
            cur = m;
            changed = true;
        }
        if let Some((m, groups)) = merge_coplanar(&cur, tol, keep) {
            cur = m;
            from = groups.iter().map(|g| g.iter().filter_map(|i| from.get(*i)).flatten().copied().collect()).collect();
            changed = true;
        }
        // Straight edges the merged pieces leave split at vertices nothing else uses join
        // into one, after any boolean (#1049): later features meet one edge, not the pieces.
        if let Some(m) = merge_collinear(&cur, &mut trace) {
            cur = m;
            changed = true;
        }
        faces.extend(from);
        shells.push(cur);
    }
    if !changed {
        return None;
    }
    let healed = Solid::try_new(shells).ok()?;
    if !traced {
        return Some((healed, None));
    }
    let edges: Option<HashMap<mt::EdgeID, Vec<usize>>> = crate::body::edge_order(healed.face_iter())
        .0
        .iter()
        .map(|e| {
            let under = trace.of(&e.id());
            (!under.is_empty()).then(|| (e.id(), under))
        })
        .collect();
    let map = edges.map(|edges| HealMap { faces, edges });
    Some((healed, map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Body;

    /// Distinct faces, edges and vertices of a shell (its iterators repeat shared ones).
    fn counts(shell: &mt::Shell) -> (usize, usize, usize) {
        let edges: std::collections::HashSet<_> = shell.edge_iter().map(|e| e.id()).collect();
        let vertices: std::collections::HashSet<_> = shell.vertex_iter().map(|v| v.id()).collect();
        (shell.face_iter().count(), edges.len(), vertices.len())
    }

    /// A prism whose front face was cut into `n` strips along a single line: the strips merge
    /// into one face, which leaves `n` collinear pieces on each of its long edges.
    fn strip_prism(n: usize) -> Solid {
        let pts: Vec<mt::Vertex> = (0..=n)
            .map(|k| builder::vertex(mt::Point3::new(k as f64, 0.0, 0.0)))
            .chain([builder::vertex(mt::Point3::new(n as f64, 3.0, 0.0)), builder::vertex(mt::Point3::new(0.0, 3.0, 0.0))])
            .collect();
        let pts: solidcraft_geom::Ring<mt::Vertex> = pts.into();
        let edges: Vec<mt::Edge> = pts.edges().map(|(a, b)| builder::line(a, b)).collect();
        let face = builder::try_attach_plane(&[mt::Wire::from(edges)]).expect("a plane");
        builder::tsweep(&face, mt::Vector3::new(0.0, 0.0, 2.0))
    }

    /// Runs of collinear edges join in one pass, however long: past the 10 000 pieces the old
    /// pairwise loop stopped at, and in time (a modeled thread has thousands of such pieces,
    /// and filleting beside one froze the app).
    #[test]
    fn long_collinear_runs_join_whole() {
        let n = 12_000;
        let started = std::time::Instant::now();
        let healed = heal(strip_prism(n), n as f64);
        let shell = healed.boundaries().first().expect("a shell").clone();
        assert_eq!(counts(&shell), (6, 12, 8));
        let body = Body::new(healed).expect("a valid body");
        let volume = crate::measure::measure(&body).expect("measures").volume;
        assert!((volume - n as f64 * 3.0 * 2.0).abs() < 1e-6 * volume, "{volume}");
        assert!(started.elapsed().as_secs() < 30, "took {:?}", started.elapsed());
    }

    #[test]
    fn short_collinear_runs_join_and_keep_the_corners() {
        let healed = heal(strip_prism(3), 3.0);
        let shell = healed.boundaries().first().expect("a shell").clone();
        assert_eq!(counts(&shell), (6, 12, 8));
    }

    #[test]
    fn approximate_arcs_become_exact() {
        // A cubic (non-rational) spline through a quarter circle.
        let (a, b) = (builder::vertex(mt::Point3::new(10.0, 0.0, 0.0)), builder::vertex(mt::Point3::new(0.0, 10.0, 0.0)));
        let pts: Vec<Vec3> = (0..=40)
            .map(|k| {
                let t = std::f64::consts::FRAC_PI_2 * k as f64 / 40.0;
                Vec3::new(10.0 * t.cos(), 10.0 * t.sin(), 0.0)
            })
            .collect();
        let curve = mt::Curve::BSplineCurve(crate::build::interpolate_cubic(&pts).expect("spline"));
        let e = mt::Edge::new(&a, &b, curve);
        let x = fit_exact(&e, 1e-2).expect("an arc");
        use mt::{BoundedCurve, ParametricCurve};
        let c = x.curve();
        let (t0, t1) = c.range_tuple();
        let mid = c.subs((t0 + t1) / 2.0);
        assert!(((mid.x * mid.x + mid.y * mid.y).sqrt() - 10.0).abs() < 1e-9, "{mid:?}");
    }
}
