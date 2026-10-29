//! Exact end-plane cuts of round, linearly ruled stock with straight through-bores.
//! A cylindrical cutter enclosing the outer wall, with its excluded cylinders wholly
//! in existing bores, cuts only at its caps. Certify current geometry, then trim there.

use std::collections::HashMap;

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling::{self as mt, BoundedCurve, ParametricCurve};

use crate::body::from_p3;
use crate::{Body, KernelError, Result};

#[derive(Clone, Copy, Debug)]
struct Circle {
    center: Vec2,
    radius: f64,
}

struct Cap {
    index: usize,
    height: f64,
    outer: Circle,
    holes: Vec<Circle>,
    normal: f64,
    edges: HashMap<mt::EdgeID, Circle>,
}

struct Stock {
    bottom: Cap,
    top: Cap,
    walls: Vec<(usize, usize, Vec<Seg2>)>,
}

/// A convex circular frustum certified from its current caps and complete ruled
/// walls. No construction metadata or fitted surface normals are trusted.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RoundStock {
    pub base: Vec3,
    pub axis: Vec3,
    pub height: f64,
    pub radii: [f64; 2],
    pub caps: [usize; 2],
}

impl RoundStock {
    /// Project a rational circular wall row along its generators onto a plane.
    /// Central (cone) and parallel (cylinder) projection preserve exact conics.
    pub(crate) fn section_curve(&self, surface: &mt::Surface, plane: &Plane, a: mt::Point3, b: mt::Point3) -> Option<mt::Curve> {
        use mt::{Cut, InnerSpace, Invertible, MetricSpace, SearchNearestParameter};
        let mt::Surface::NurbsSurface(s) = surface else { return None };
        let net = s.control_points();
        let (rows, cols) = (net.len(), net.first()?.len());
        let slope = (self.radii[1] - self.radii[0]) / self.height;
        for transpose in [false, true] {
            let (count, length, knots, along) =
                if transpose { (cols, rows, s.vknot_vec(), s.uknot_vec()) } else { (rows, cols, s.uknot_vec(), s.vknot_vec()) };
            if length != 2 || along.len() != 4 || count > 4096 {
                continue;
            }
            let at = |i: usize, j: usize| if transpose { net.get(j)?.get(i).copied() } else { net.get(i)?.get(j).copied() };
            let p = at(0, 0)?;
            let q = at(0, 1)?;
            let h0 = (Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w) - self.base).dot(self.axis);
            let h1 = (Vec3::new(q.x / q.w, q.y / q.w, q.z / q.w) - self.base).dot(self.axis);
            let r0 = self.radii[0] + slope * h0;
            let r1 = self.radii[0] + slope * h1;
            if r0 <= 0. || (h0 - h1).abs() < self.height * 1e-9 {
                continue;
            }
            let ratio = r1 / r0;
            let c0 = self.base + self.axis * h0;
            let c1 = self.base + self.axis * h1;
            // Corresponding end rows obey Q = ratio·P + v·P.w. Thus v
            // is the common generator apex in homogeneous coordinates; for a
            // cylinder v.w is zero (parallel projection along its axis).
            let v = c1 - c0 * ratio;
            let v = mt::Vector4::new(v.x, v.y, v.z, 1. - ratio);
            let n = plane.normal();
            let field = |p: mt::Vector4| n.dot(Vec3::new(p.x, p.y, p.z) - plane.origin * p.w);
            let fv = field(v);
            let base_controls = (0..count).map(|i| at(i, 0)).collect::<Option<Vec<_>>>()?;
            let mut base = mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::try_new(knots.clone(), base_controls).ok()?));
            let (lo, hi) = base.range_tuple();
            let parameter = |p: mt::Point3| {
                let p = Vec3::new(p.x, p.y, p.z);
                let h = (p - self.base).dot(self.axis);
                let radius = self.radii[0] + slope * h;
                if radius <= 0. {
                    return None;
                }
                let center = self.base + self.axis * h;
                let q = c0 + (p - center) * (r0 / radius);
                let q = mt::Point3::new(q.x, q.y, q.z);
                let hint = (0..=32)
                    .map(|i| lo + (hi - lo) * i as f64 / 32.)
                    .min_by(|a, b| base.subs(*a).distance2(q).total_cmp(&base.subs(*b).distance2(q)));
                let mut t = base.search_nearest_parameter(q, hint, 100)?.clamp(lo, hi);
                // Truck's nearest-point stop is coarse enough to amplify into
                // micrometres at the end of a steep projected conic. Refine on
                // the original rational circle before its projective mapping.
                for _ in 0..16 {
                    let offset = base.subs(t) - q;
                    let derivative = base.der(t);
                    let denominator = derivative.dot(derivative) + offset.dot(base.der2(t));
                    if !denominator.is_finite() || denominator.abs() < 1e-24 {
                        return None;
                    }
                    let next = (t - offset.dot(derivative) / denominator).clamp(lo, hi);
                    if !next.is_finite() {
                        return None;
                    }
                    if next == t {
                        break;
                    }
                    t = next;
                }
                (base.subs(t).distance(q) <= self.height * 1e-10 + 1e-10).then_some(t)
            };
            let (ta, tb) = (parameter(a)?, parameter(b)?);
            // Trim the circular parameter interval first. A hyperbolic section
            // can cross infinity outside the retained arc; its retained weights
            // must stay finite and have one sign.
            if a.distance(b) > self.height * 1e-9 {
                let (start, end) = (ta.min(tb), ta.max(tb));
                if end < hi - 1e-12 {
                    let _ = base.cut(end);
                }
                if start > lo + 1e-12 {
                    base = base.cut(start);
                }
            }
            let mt::Curve::NurbsCurve(base) = base else { return None };
            // L(v)·P - L(P)·v lies both on the generator through P and on
            // L=0. Applying this linear homogeneous map to rational controls
            // preserves the conic exactly.
            let controls = base.control_points().iter().map(|p| *p * fv - v * field(*p)).collect::<Vec<_>>();
            let sign = controls.first()?.w.signum();
            if sign == 0. || controls.iter().any(|p| ![p.x, p.y, p.z, p.w].iter().all(|v| v.is_finite()) || p.w * sign <= 1e-12 * fv.abs()) {
                continue;
            }
            let controls = controls.into_iter().map(|p| p * sign).collect();
            let curve = mt::NurbsCurve::new(mt::BSplineCurve::try_new(base.knot_vec().clone(), controls).ok()?);
            let mut curve = mt::Curve::NurbsCurve(curve);
            if ta > tb {
                curve.invert();
            }
            let (lo, hi) = curve.range_tuple();
            if curve.subs(lo).distance(a) > self.height * 1e-9 + 1e-9 || curve.subs(hi).distance(b) > self.height * 1e-9 + 1e-9 {
                return None;
            }
            return Some(curve);
        }
        None
    }
}

pub(crate) fn convex_round_stock(body: &Body) -> Option<RoundStock> {
    if body.is_mesh() || body.shell_count() != 1 || body.face_count() > 256 {
        return None;
    }
    crate::guard("convex round stock", || {
        let recognize = || {
            let axis = body.solid.face_iter().find_map(|face| {
                let mt::Surface::Plane(plane) = face.surface() else { return None };
                let n = plane.normal();
                let n = Vec3::new(n.x, n.y, n.z).normalized()?;
                let sign = [n.x, n.y, n.z].into_iter().find(|v| v.abs() > 1e-9)?;
                Some(if sign < 0. { -n } else { n })
            })?;
            let frame = Plane::from_normal(Vec3::ZERO, axis)?;
            let tol = (body.size() * 1e-9).max(1e-9);
            let mut work = 2_000_000;
            let current = stock(body, &frame, tol, &mut work)?;
            if !current.bottom.holes.is_empty() || !body.validity().is_empty() {
                return None;
            }
            let mut round = RoundStock {
                base: frame.to_world(current.bottom.outer.center) + axis * current.bottom.height,
                axis,
                height: current.top.height - current.bottom.height,
                radii: [current.bottom.outer.radius, current.top.outer.radius],
                caps: [current.bottom.index, current.top.index],
            };
            // A cone's axis follows increasing radius, independent of how a
            // rigid placement changes the signs of its world coordinates.
            if round.radii[1] < round.radii[0] {
                round.base += round.axis * round.height;
                round.axis = -round.axis;
                round.radii.swap(0, 1);
                round.caps.swap(0, 1);
            }
            Some(round)
        };
        Ok(recognize())
    })
    .ok()
    .flatten()
}

/// Whole boundary curves, not a circle fitted through seam vertices.
fn circle(wire: &mt::Wire, frame: &Plane, tol: f64, work: &mut usize) -> Option<(Circle, f64)> {
    if wire.is_empty() || wire.len() > 32 {
        return None;
    }
    let mut found: Option<Circle> = None;
    let mut turn = 0.;
    for edge in wire.edge_iter() {
        let read = crate::prism_curve::segment(edge, frame, tol, work);
        for segment in read? {
            let Seg2::Arc { center, radius, sweep, .. } = segment else { return None };
            if let Some(c) = found {
                if center.dist(c.center) + (radius - c.radius).abs() > tol || sweep * turn < 0. {
                    return None;
                }
            } else {
                found = Some(Circle { center, radius });
            }
            turn += sweep;
        }
    }
    let circle = found?;
    // Independently certified arcs can have tiny angular endpoint uncertainty.
    // Judge closure in millimetres, as the curve certificates do.
    ((turn.abs() - std::f64::consts::TAU).abs() * circle.radius <= 4. * tol).then_some((circle, turn))
}

fn cap(index: usize, face: &mt::Face, frame: &Plane, tol: f64, work: &mut usize) -> Option<Cap> {
    let mt::Surface::Plane(p) = face.oriented_surface() else { return None };
    let n = p.normal();
    let n = Vec3::new(n.x, n.y, n.z).normalized()?;
    if n.cross(frame.normal()).len() > 1e-10 || face.boundaries().len() > 65 {
        return None;
    }
    let height = frame.height(from_p3(p.origin()));
    if face.vertex_iter().any(|v| (frame.height(from_p3(v.point())) - height).abs() > tol) {
        return None;
    }
    let local = frame.offset(height);
    let mut circles = Vec::new();
    let mut edges = HashMap::new();
    for wire in face.boundaries() {
        let (circle, turn) = circle(&wire, &local, tol, work)?;
        circles.push((circle, turn));
        edges.extend(wire.edge_iter().map(|edge| (edge.id(), circle)));
    }
    circles.sort_by(|a, b| b.0.radius.total_cmp(&a.0.radius));
    let (outer, turn) = *circles.first()?;
    let normal = n.dot(frame.normal());
    if turn * normal <= 0. || circles.iter().skip(1).any(|(_, turn)| turn * normal >= 0.) {
        return None;
    }
    let holes: Vec<_> = circles.into_iter().skip(1).map(|(c, _)| c).collect();
    if holes.iter().any(|c| c.center.dist(outer.center) + c.radius >= outer.radius - tol) {
        return None;
    }
    for (i, c) in holes.iter().enumerate() {
        if holes.iter().skip(i + 1).any(|b| c.center.dist(b.center) <= c.radius + b.radius + tol) {
            return None;
        }
    }
    Some(Cap { index, height, outer, holes, normal, edges })
}

/// A surface is exactly a ruled interpolation between concentric circular arcs.
/// The complete rational controls must share angular positions and weights; arbitrary
/// positive weights or circular end rows alone would permit an intervening bulge.
fn round_wall(surface: &mt::Surface, frame: &Plane, tol: f64, work: &mut usize) -> Option<(Circle, Circle, f64, f64)> {
    let mt::Surface::NurbsSurface(s) = surface else { return None };
    let net = s.control_points();
    let (rows, cols) = (net.len(), net.first()?.len());
    if s.uknot_vec().len() > 8192
        || s.vknot_vec().len() > 8192
        || rows < 2
        || cols < 2
        || rows.saturating_mul(cols) > 4096
        || net.iter().any(|r| r.len() != cols)
    {
        return None;
    }
    let max_w = net.iter().flatten().map(|p| p.w).fold(0., f64::max);
    if net.iter().flatten().any(|p| ![p.x, p.y, p.z, p.w].iter().all(|v| v.is_finite()) || p.w <= 0. || p.w / max_w < 1e-12) {
        return None;
    }
    for transpose in [false, true] {
        let (count, length, knots, along) =
            if transpose { (cols, rows, s.vknot_vec(), s.uknot_vec()) } else { (rows, cols, s.uknot_vec(), s.vknot_vec()) };
        if length != 2 || along.len() != 4 || along.first()? != along.get(1)? || along.get(2)? != along.get(3)? || along.get(1)? >= along.get(2)? {
            continue;
        }
        let at = |i: usize, j: usize| if transpose { net.get(j)?.get(i).copied() } else { net.get(i)?.get(j).copied() };
        let ends = (0..2).map(|j| (0..count).map(|i| at(i, j)).collect::<Option<Vec<_>>>()).collect::<Option<Vec<_>>>()?;
        let read = |controls: &Vec<mt::Vector4>, work: &mut usize| -> Option<(Circle, f64)> {
            let first = *controls.first()?;
            let point = Vec3::new(first.x / first.w, first.y / first.w, first.z / first.w);
            let height = frame.height(point);
            let plane = frame.offset(height);
            let curve = mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::try_new(knots.clone(), controls.clone()).ok()?));
            let (a, b) = curve.range_tuple();
            let edge = mt::Edge::new(&mt::Vertex::new(curve.subs(a)), &mt::Vertex::new(curve.subs(b)), curve);
            let segments = crate::prism_curve::segment(&edge, &plane, tol, work)?;
            let mut c: Option<Circle> = None;
            for segment in segments {
                let Seg2::Arc { center, radius, .. } = segment else { return None };
                if c.is_some_and(|old| old.center.dist(center) + (old.radius - radius).abs() > tol) {
                    return None;
                }
                c = Some(Circle { center, radius });
            }
            Some((c?, height))
        };
        let (a, ha) = read(ends.first()?, work)?;
        let (b, hb) = read(ends.get(1)?, work)?;
        if a.center.dist(b.center) > tol || (ha - hb).abs() <= tol {
            continue;
        }
        let (wa, wb) = (at(0, 0)?.w, at(0, 1)?.w);
        let mut matches = true;
        for i in 0..count {
            let (p, q) = (at(i, 0)?, at(i, 1)?);
            let p3 = Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w);
            let q3 = Vec3::new(q.x / q.w, q.y / q.w, q.z / q.w);
            let (u, v) = ((frame.to_local(p3) - a.center) / a.radius, (frame.to_local(q3) - b.center) / b.radius);
            if u.dist(v) * a.radius.max(b.radius) > tol
                || (frame.height(p3) - ha).abs() > tol
                || (frame.height(q3) - hb).abs() > tol
                || (p.w / wa - q.w / wb).abs() > 1e-12 * (p.w / wa).abs()
            {
                matches = false;
                break;
            }
        }
        if matches {
            return Some(if ha < hb { (a, b, ha, hb) } else { (b, a, hb, ha) });
        }
    }
    None
}

/// Exactly two circular caps and ruled round walls spanning them. Every wall must
/// bound the outer taper or a complete straight bore; no unexamined remote faces.
fn stock(body: &Body, frame: &Plane, tol: f64, work: &mut usize) -> Option<Stock> {
    if body.is_mesh() || body.shell_count() != 1 || body.face_count() > 256 {
        return None;
    }
    let mut caps = Vec::new();
    let mut walls = Vec::new();
    for (index, face) in body.solid.face_iter().enumerate() {
        if matches!(face.surface(), mt::Surface::Plane(_)) {
            caps.push(cap(index, face, frame, tol, work)?);
        } else {
            walls.push((index, face));
        }
    }
    if caps.len() != 2 {
        return None;
    }
    caps.sort_by(|a, b| a.height.total_cmp(&b.height));
    let top = caps.pop()?;
    let bottom = caps.pop()?;
    if bottom.normal > -1. + 1e-10
        || top.normal < 1. - 1e-10
        || top.height <= bottom.height + tol
        || top.outer.center.dist(bottom.outer.center) > tol
        || top.holes.len() != bottom.holes.len()
    {
        return None;
    }
    let same = |a: Circle, b: Circle| a.center.dist(b.center) + (a.radius - b.radius).abs() <= tol;
    if bottom.holes.iter().any(|c| !top.holes.iter().any(|b| same(*c, *b))) {
        return None;
    }
    let min_radius = bottom.outer.radius.min(top.outer.radius);
    if bottom.holes.iter().any(|c| c.center.dist(bottom.outer.center) + c.radius >= min_radius - tol) {
        return None;
    }
    let mut used = vec![false; bottom.holes.len() + 1];
    let mut lineage = Vec::new();
    for (index, face) in walls {
        if face.boundaries().len() != 1 || face.edge_iter().count() > 32 {
            return None;
        }
        let low = face.edge_iter().filter_map(|edge| bottom.edges.get(&edge.id()).copied()).collect::<Vec<_>>();
        let high = face.edge_iter().filter_map(|edge| top.edges.get(&edge.id()).copied()).collect::<Vec<_>>();
        let (a, b) = (*low.first()?, *high.first()?);
        if low.iter().any(|c| !same(*c, a)) || high.iter().any(|c| !same(*c, b)) {
            return None;
        }
        let surface = face.surface();
        if crate::prism::wall_along(&surface, frame.normal(), tol, work) {
            if !same(a, b) {
                return None;
            }
        } else {
            let wall = round_wall(&surface, frame, tol, work);
            let (u, v, lo, hi) = wall?;
            if !same(u, a) || !same(v, b) || (lo - bottom.height).abs() + (hi - top.height).abs() > tol {
                return None;
            }
        }
        let which = if same(a, bottom.outer) && same(b, top.outer) { 0 } else { bottom.holes.iter().position(|c| same(a, *c) && same(b, *c))? + 1 };
        *used.get_mut(which)? = true;
        let mut arcs = Vec::new();
        for edge in face.edge_iter().filter(|edge| bottom.edges.contains_key(&edge.id())) {
            arcs.extend(crate::prism_curve::segment(&edge, frame, tol, work)?);
        }
        lineage.push((index, which, arcs));
    }
    used.iter().all(|seen| *seen).then_some(Stock { bottom, top, walls: lineage })
}

/// Positive angular overlap, independent of seam position and edge orientation.
fn arc_overlap(a: &Seg2, b: &Seg2, tol: f64) -> bool {
    let (Seg2::Arc { start: a, sweep: da, radius: ra, .. }, Seg2::Arc { start: b, sweep: db, radius: rb, .. }) = (*a, *b) else { return false };
    let tau = std::f64::consts::TAU;
    let x = (a + da.min(0.)).rem_euclid(tau);
    let y = (b + db.min(0.)).rem_euclid(tau);
    [-tau, 0., tau].into_iter().any(|shift| (x + da.abs()).min(y + shift + db.abs()) - x.max(y + shift) > 8. * tol / ra.min(rb))
}

fn section(stock: &Stock, z: f64) -> Region2 {
    let t = (z - stock.bottom.height) / (stock.top.height - stock.bottom.height);
    let radius = stock.bottom.outer.radius + (stock.top.outer.radius - stock.bottom.outer.radius) * t;
    Region2 {
        outer: Loop2::circle(stock.bottom.outer.center, radius),
        holes: stock.bottom.holes.iter().map(|c| Loop2::circle(c.center, c.radius).reversed()).collect(),
    }
}

/// Rebuild the exact ruled section, avoiding a numerically fitted section curve.
/// Origins follow actual angular overlap with each original wall, and each new
/// cap follows the cutter cap that made it (opposite orientation for a Cut).
fn piece(target: &Stock, tool: &Stock, frame: &Plane, lo: f64, hi: f64, tol: f64, counts: &[(usize, usize)]) -> Result<Body> {
    use crate::history::{Change, FaceOrigin, FaceProv};
    let bottom = section(target, lo);
    let top = section(target, hi);
    let mut low = Vec::new();
    let mut high = Vec::new();
    let mut sides = Vec::new();
    for (i, (a, b)) in std::iter::once(&bottom.outer).chain(&bottom.holes).zip(std::iter::once(&top.outer).chain(&top.holes)).enumerate() {
        // Use exact rational arcs. The general loft builder interpolates its
        // section curves as cubics, which cannot represent a circle exactly.
        let a = if i == 0 { a.clone() } else { a.reversed() };
        let b = if i == 0 { b.clone() } else { b.reversed() };
        let a = crate::build::chain_wire(&frame.offset(lo), &a.segs)?;
        let b = crate::build::chain_wire(&frame.offset(hi), &b.segs)?;
        let walls = mt::builder::try_wire_homotopy(&a, &b).map_err(|e| KernelError::Failed(format!("round cap cut: {e}")))?;
        sides.extend(walls.face_iter().map(|face| (if i == 0 { face.clone() } else { face.inverse() }, None)));
        low.push(if i == 0 { a } else { a.inverse() });
        high.push(if i == 0 { b } else { b.inverse() });
    }
    let body = crate::build::cap_and_close_in_frames(sides, &low, &high, false, &frame.offset(lo), &frame.offset(hi))?;
    let mut work = 2_000_000;
    let fresh = stock(&body, frame, tol, &mut work).ok_or_else(|| KernelError::Failed("round cap cut: rebuilt section certificate".into()))?;
    let mut faces = vec![None; body.face_count()];
    let mut put = |index: usize, prov: FaceProv| -> Option<()> {
        *faces.get_mut(index)? = Some(prov);
        Some(())
    };
    let cap_source = |cap: &Cap| {
        let old = if (cap.height - target.bottom.height).abs() <= tol {
            (0, target.bottom.index)
        } else if (cap.height - target.top.height).abs() <= tol {
            (0, target.top.index)
        } else if (cap.height - tool.bottom.height).abs() <= tol {
            (1, tool.bottom.index)
        } else if (cap.height - tool.top.height).abs() <= tol {
            (1, tool.top.index)
        } else {
            return None;
        };
        Some(crate::record::from_face(old.0, old.1, Change::Modified))
    };
    let mut follow = || -> Option<()> {
        put(fresh.bottom.index, cap_source(&fresh.bottom)?)?;
        put(fresh.top.index, cap_source(&fresh.top)?)?;
        for (index, ring, arcs) in &fresh.walls {
            let original_ring = if *ring == 0 {
                0
            } else {
                let c = fresh.bottom.holes.get(ring.checked_sub(1)?)?;
                target.bottom.holes.iter().position(|old| old.center.dist(c.center) + (old.radius - c.radius).abs() <= tol)? + 1
            };
            let origins: Vec<_> = target
                .walls
                .iter()
                .filter(|(_, ring, old)| *ring == original_ring && arcs.iter().any(|a| old.iter().any(|b| arc_overlap(a, b, tol))))
                .map(|(index, _, _)| FaceOrigin::Face { operand: 0, face: *index })
                .collect();
            if origins.is_empty() {
                return None;
            }
            let change = if origins.len() > 1 { Change::Merged } else { Change::Modified };
            put(*index, FaceProv { origins, change })?;
        }
        Some(())
    };
    follow().ok_or_else(|| KernelError::Failed("round cap cut: section lineage".into()))?;
    let faces = faces.into_iter().collect::<Option<Vec<_>>>().ok_or_else(|| KernelError::Failed("round cap cut: incomplete lineage".into()))?;
    let history = crate::record::from_faces(&body.solid, 2, faces);
    body.with_history_counts(history, counts)
}

pub(crate) fn cut(target: &Body, tool: &Body) -> Option<Result<Option<Body>>> {
    if target.is_mesh() || tool.is_mesh() || target.face_count().max(tool.face_count()) > 256 {
        return None;
    }
    let tol = (target.size().max(tool.size()) * 1e-9).max(1e-9);
    let certified = crate::guard("round cap cut certificate", || {
        let recognize = || {
            let axis = crate::prism::prism_axis(tool)?;
            let frame = Plane::from_normal(Vec3::ZERO, axis)?;
            let mut work = 2_000_000;
            let a = stock(target, &frame, tol, &mut work);
            let b = stock(tool, &frame, tol, &mut work);
            Some((axis, a?, b?))
        };
        Ok(recognize())
    })
    .ok()??;
    let (axis, a, b) = certified;
    let frame = Plane::from_normal(Vec3::ZERO, axis)?;
    // This recovery targets stock whose shared bore walls defeat the general
    // intersection. Leave other tapered-stock cases to the existing strategies.
    if a.bottom.holes.is_empty() {
        return None;
    }
    // Tool walls enclose the complete target, including between its seams.
    if b.bottom.outer.center.dist(b.top.outer.center) + (b.bottom.outer.radius - b.top.outer.radius).abs() > tol
        || a.bottom.outer.center.dist(b.bottom.outer.center) + a.bottom.outer.radius.max(a.top.outer.radius) > b.bottom.outer.radius + tol
        || b.bottom.holes.iter().any(|hole| !a.bottom.holes.iter().any(|void| hole.center.dist(void.center) + hole.radius <= void.radius + tol))
        || b.top.height <= a.bottom.height + tol
        || b.bottom.height >= a.top.height - tol
    {
        return None;
    }
    let run = || -> Result<Option<Body>> {
        let counts = crate::record::counts(&[target, tool]);
        let mut pieces = Vec::new();
        if b.bottom.height > a.bottom.height + tol {
            pieces.push(piece(&a, &b, &frame, a.bottom.height, b.bottom.height, tol, &counts)?);
        }
        if b.top.height < a.top.height - tol {
            pieces.push(piece(&a, &b, &frame, b.top.height, a.top.height, tol, &counts)?);
        }
        let origins: HashMap<_, _> = pieces
            .iter()
            .flat_map(|body| body.solid.face_iter().zip(body.history().into_iter().flat_map(|h| &h.faces)).map(|(f, h)| (f.id(), h.clone())))
            .collect();
        let result = crate::ops::join_pieces(pieces)?;
        if let Some(body) = &result {
            let problems = body.validity();
            if !problems.is_empty() {
                return Err(KernelError::Failed(format!("round cap cut: {}", problems.join("; "))));
            }
        }
        crate::boolean_guard::check_retry(target, tool, crate::BoolOp::Cut, result.as_ref())?;
        result
            .map(|body| {
                let faces = body
                    .solid
                    .face_iter()
                    .map(|face| origins.get(&face.id()).cloned())
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| KernelError::Failed("round cap cut: incomplete combined lineage".into()))?;
                let history = crate::record::from_faces(&body.solid, 2, faces);
                body.with_history_counts(history, &counts)
            })
            .transpose()
    };
    Some(crate::guard("round cap cut", run))
}
