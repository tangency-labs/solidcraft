//! Independent current-geometry intervals for uniform prisms.
//! These are not mesh measurements or certificates of construction history.
use crate::body::from_p3;
use solidcraft_geom::{Plane, Ring, Seg2, Vec2, Vec3};
use truck_modeling as mt;

#[derive(Clone, Copy, Debug)]
struct Interval {
    lo: f64,
    hi: f64,
}

impl Interval {
    fn enclosing(value: f64, error: f64) -> Option<Self> {
        if !value.is_finite() || !error.is_finite() || error < 0. {
            return None;
        }
        // The explicit extra arithmetic allowance also covers subtraction and
        // accumulation; do not round an uncertainty inward at its endpoints.
        let padding = 16. * f64::EPSILON * (value.abs() + error + 1.);
        Some(Self { lo: value - error - padding, hi: value + error + padding })
    }

    fn add(self, other: Self) -> Option<Self> {
        Self::enclosing((self.lo + self.hi + other.lo + other.hi) * 0.5, (self.hi - self.lo + other.hi - other.lo) * 0.5)
    }
}

/// Signed Green integral of a fitted segment with the *whole-curve* spatial
/// uncertainty. Angular/longitudinal monotonicity is mandatory: a curve in a
/// thin corridor could otherwise retrace it arbitrarily often.
fn green_interval(segment: &Seg2, tolerance: f64) -> Option<Interval> {
    let (length, a, b) = (segment.length(), segment.start(), segment.end());
    let extra = match *segment {
        Seg2::Line { .. } => 0.,
        Seg2::Arc { center, radius, .. } => center.len() + radius,
        _ => return None,
    };
    let scale = length + a.len() + b.len() + extra;
    // A monotone line differs from its chord by a strip; a radial-monotone
    // arc contributes 1/2 integral r(theta)^2 dtheta. The endpoint wedges,
    // coordinate uncertainty, and angular endpoint uncertainty are included
    // by this conservative bound (rather than treating the fitted arc exact).
    let error = 8. * tolerance * scale + 64. * tolerance * tolerance + 1024. * f64::EPSILON * (1. + scale * scale);
    Interval::enclosing(segment.area_term(), error)
}

struct CapLoop {
    segments: Ring<Seg2>,
    area: Interval,
}

/// Cross-product cancellation belongs to the surface representation, not
/// just the later incidence dot product. Reject ill-conditioned plane bases.
fn plane_normal(p: &mt::Plane) -> Option<(Vec3, Vec3)> {
    let u = p.u_axis();
    let v = p.v_axis();
    let u = Vec3::new(u.x, u.y, u.z);
    let v = Vec3::new(v.x, v.y, v.z);
    let cross = u.cross(v);
    let length = cross.len();
    if !length.is_finite() || length <= 1e-6 * u.len() * v.len() {
        return None;
    }
    let n = cross.normalized()?;
    let ce = Vec3::new((u.y * v.z).abs() + (u.z * v.y).abs(), (u.z * v.x).abs() + (u.x * v.z).abs(), (u.x * v.y).abs() + (u.y * v.x).abs())
        * (256. * f64::EPSILON);
    let ne = ce * (1. / length) + Vec3::new(n.x.abs(), n.y.abs(), n.z.abs()) * (ce.len() / length);
    (ne.x.is_finite() && ne.y.is_finite() && ne.z.is_finite()).then_some((n, ne))
}

/// A whole positive control-net bound for the wall's projected variation.
/// A sampled normal or a vertex-only test is deliberately insufficient.
fn wall_error(surface: &mt::Surface, d: Vec3, height: f64, work: &mut usize) -> Option<f64> {
    if let mt::Surface::Plane(p) = surface {
        let (n, ne) = plane_normal(p)?;
        let terms = (n.x * d.x).abs() + (n.y * d.y).abs() + (n.z * d.z).abs();
        let error = ne.x * d.x.abs() + ne.y * d.y.abs() + ne.z * d.z.abs() + 1024. * f64::EPSILON * terms;
        return Some((n.dot(d).abs() + error) * height * 2.);
    }
    if let mt::Surface::RevolutedCurve(p) = surface {
        let r = p.entity();
        let mt::Curve::Line(line) = r.entity_curve() else { return None };
        let axis = r.axis();
        let meridian = line.1 - line.0;
        let a = Vec3::new(axis.x, axis.y, axis.z).normalized()?;
        let m = Vec3::new(meridian.x, meridian.y, meridian.z).normalized()?;
        let placed = *p.transform() * mt::Vector4::new(axis.x, axis.y, axis.z, 0.);
        if placed.w != 0. {
            return None;
        }
        let pa = Vec3::new(placed.x, placed.y, placed.z).normalized()?;
        return Some((a.cross(m).len() + pa.cross(d).len()) * height * 4.);
    }
    let (rows, cols) = match surface {
        mt::Surface::BSplineSurface(s) => (s.control_points().len(), s.control_points().first()?.len()),
        mt::Surface::NurbsSurface(s) => (s.control_points().len(), s.control_points().first()?.len()),
        _ => return None,
    };
    let count = rows.checked_mul(cols)?;
    let rectangular = match surface {
        mt::Surface::BSplineSurface(s) => s.control_points().iter().all(|r| r.len() == cols),
        mt::Surface::NurbsSurface(s) => s.control_points().iter().all(|r| r.len() == cols),
        _ => false,
    };
    if rows < 2 || cols < 2 || count > 16_384 || !rectangular {
        return None;
    }
    *work = work.checked_sub(count.checked_mul(4)?)?;
    let points: Vec<Vec<(Vec3, f64)>> = match surface {
        mt::Surface::BSplineSurface(s) => s.control_points().iter().map(|r| r.iter().map(|p| (from_p3(*p), 1.)).collect()).collect(),
        mt::Surface::NurbsSurface(s) => {
            s.control_points().iter().map(|r| r.iter().map(|p| (Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w), p.w)).collect()).collect()
        }
        _ => return None,
    };
    if points.iter().any(|r| r.len() != cols) || points.iter().flatten().any(|(p, w)| !p.is_finite() || !w.is_finite() || *w <= 0.) {
        return None;
    }
    let max_w = points.iter().flatten().map(|p| p.1).fold(0., f64::max);
    let min_w = points.iter().flatten().map(|p| p.1 / max_w).fold(1., f64::min);
    if min_w < 1e-6 {
        return None;
    }
    let magnitude = points.iter().flatten().map(|p| p.0.len()).fold(1., f64::max);
    let mut best = f64::INFINITY;
    for transpose in [false, true] {
        let at = |i: usize, j: usize| if transpose { points.get(j)?.get(i).copied() } else { points.get(i)?.get(j).copied() };
        let (groups, length) = if transpose { (cols, rows) } else { (rows, cols) };
        let mut variation: f64 = 0.;
        let mut rank_error: f64 = 0.;
        let mut monotone = true;
        let mut axial_sign = 0.;
        for i in 0..groups {
            let (base, bw) = at(i, 0)?;
            let mut previous = base.dot(d);
            for j in 0..length {
                let (point, w) = at(i, j)?;
                let step = point.dot(d) - previous;
                previous = point.dot(d);
                if step.abs() > 2048. * f64::EPSILON * magnitude {
                    if axial_sign * step < 0. {
                        monotone = false;
                    }
                    axial_sign = step.signum();
                }
                variation = variation.max((point - base).cross(d).len());
                let lhs = w / max_w * (at(0, 0)?.1 / max_w);
                let rhs = bw / max_w * (at(0, j)?.1 / max_w);
                rank_error = rank_error.max((lhs - rhs).abs());
            }
            if (at(i, length - 1)?.0 - base).dot(d).abs() < height * 0.5 {
                monotone = false;
            }
        }
        if !monotone || axial_sign == 0. {
            continue;
        }
        // Positive weights turn the control hull into a positional bound.
        // Rank-one weight error perturbs the normalized convex coefficients;
        // the positive minimum weight prevents an unbounded denominator.
        let error = 2. * variation + 8. * magnitude * rank_error / min_w.powi(2) + 2048. * f64::EPSILON * magnitude / min_w.powi(2);
        best = best.min(error);
    }
    best.is_finite().then_some(best)
}

fn segment_same(a: &Seg2, b: &Seg2, tolerance: f64) -> bool {
    match (*a, *b) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => {
            (a.dist(c) <= tolerance && b.dist(d) <= tolerance) || (a.dist(d) <= tolerance && b.dist(c) <= tolerance)
        }
        (Seg2::Arc { center, radius, start, sweep }, Seg2::Arc { center: c, radius: r, start: t, sweep: s }) => {
            center.dist(c) <= tolerance
                && (radius - r).abs() <= tolerance
                && (sweep.abs() - s.abs()).abs() * radius <= tolerance
                && ((Vec2::new(start.cos(), start.sin()) * radius).dist(Vec2::new(t.cos(), t.sin()) * r) <= tolerance
                    || (Vec2::new(start.cos(), start.sin()) * radius).dist(Vec2::new((t + s).cos(), (t + s).sin()) * r) <= tolerance)
        }
        _ => false,
    }
}

fn wall_supports_cap(surface: &mt::Surface, edge: &mt::Edge, plane: &Plane, levels: (f64, f64), tol: f64, work: &mut usize) -> Option<bool> {
    use mt::{BoundedCurve, ParametricCurve};
    let cap = crate::prism_curve::segment(edge, plane, tol, work)?;
    if let mt::Surface::Plane(p) = surface {
        let (n, ne) = plane_normal(p)?;
        let origin = from_p3(p.origin());
        for s in cap {
            let Seg2::Line { a, b } = s else { return Some(false) };
            for q in [a, b] {
                let point = plane.to_world(q) + plane.normal() * plane.height(from_p3(edge.front().point()));
                let delta = point - origin;
                let error =
                    1024. * f64::EPSILON * (1. + point.len() + origin.len()) + delta.x.abs() * ne.x + delta.y.abs() * ne.y + delta.z.abs() * ne.z;
                if delta.dot(n).abs() + error > tol * 8. {
                    return Some(false);
                }
            }
        }
        return Some(true);
    }
    if let mt::Surface::RevolutedCurve(processor) = surface {
        let r = processor.entity();
        let mt::Curve::Line(line) = r.entity_curve() else { return None };
        let o = from_p3(r.origin());
        let axis = r.axis();
        let axis = Vec3::new(axis.x, axis.y, axis.z).unit()?;
        let delta = from_p3(line.0) - o;
        let radial = (delta - axis * delta.dot(axis)).len();
        let (u, v) = axis.perp_pair();
        let matrix = *processor.transform();
        let place = |p: Vec3, w: f64| {
            let q = matrix * mt::Vector4::new(p.x, p.y, p.z, w);
            (q.w == w).then_some(Vec3::new(q.x, q.y, q.z))
        };
        let center = plane.to_local(place(o, 1.)?);
        let ha = plane.height(place(from_p3(line.0), 1.)?);
        let hb = plane.height(place(from_p3(line.1), 1.)?);
        if ha.min(hb) > levels.0 + tol * 8. || ha.max(hb) < levels.1 - tol * 8. {
            return Some(false);
        }
        let (pu, pv) = (place(*u, 0.)?, place(*v, 0.)?);
        let radius = pu.len() * radial;
        if (pu.len() - pv.len()).abs() * radial > tol
            || pu.dot(pv).abs() * radial > tol
            || pu.dot(plane.normal()).abs() * radial > tol
            || pv.dot(plane.normal()).abs() * radial > tol
        {
            return None;
        }
        return Some(cap.iter().all(|s| matches!(*s,Seg2::Arc {center:c,radius:r,..} if c.dist(center)+(r-radius).abs()<=tol*8.)));
    }
    let (rows, cols) = match surface {
        mt::Surface::BSplineSurface(s) => (s.control_points().len(), s.control_points().first()?.len()),
        mt::Surface::NurbsSurface(s) => (s.control_points().len(), s.control_points().first()?.len()),
        _ => return None,
    };
    *work = work.checked_sub(rows.checked_mul(cols)?.checked_mul(4)?)?;
    let at = |i: usize, j: usize| -> Option<(Vec3, f64)> {
        match surface {
            mt::Surface::BSplineSurface(s) => s.control_points().get(i)?.get(j).map(|p| (from_p3(*p), 1.)),
            mt::Surface::NurbsSurface(s) => s.control_points().get(i)?.get(j).map(|p| (Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w), p.w)),
            _ => None,
        }
    };
    for transpose in [false, true] {
        let point = |i, j| if transpose { at(j, i) } else { at(i, j) };
        let (groups, length) = if transpose { (cols, rows) } else { (rows, cols) };
        // The first measurement subset uses a degree-one axial ruling. Its
        // positive rational interpolation is monotone and reaches both caps;
        // projected incidence alone would miss an axially displaced support.
        if !(2..=4096).contains(&groups) || length != 2 {
            continue;
        }
        let axial_knots = match surface {
            mt::Surface::BSplineSurface(s) => {
                if transpose {
                    s.uknot_vec()
                } else {
                    s.vknot_vec()
                }
            }
            mt::Surface::NurbsSurface(s) => {
                if transpose {
                    s.uknot_vec()
                } else {
                    s.vknot_vec()
                }
            }
            _ => return None,
        };
        if axial_knots.len() != 4
            || axial_knots.first() != axial_knots.get(1)
            || axial_knots.get(2) != axial_knots.last()
            || axial_knots.get(1)? >= axial_knots.get(2)?
        {
            continue;
        }
        let mut projected = Vec::new();
        let mut valid = true;
        let mut increasing = None;
        for i in 0..groups {
            let (base, w) = point(i, 0)?;
            let h0 = plane.height(base);
            let h1 = plane.height(point(i, 1)?.0);
            if h0.min(h1) > levels.0 + tol * 8. || h0.max(h1) < levels.1 - tol * 8. || increasing.is_some_and(|s| s != (h1 > h0)) {
                valid = false;
                break;
            }
            increasing = Some(h1 > h0);
            let p = plane.to_local(base);
            for j in 0..length {
                if plane.to_local(point(i, j)?.0).dist(p) > tol * 8. {
                    valid = false;
                    break;
                }
            }
            let q = plane.to_world(p);
            projected.push(mt::Vector4::new(q.x * w, q.y * w, q.z * w, w));
        }
        if !valid {
            continue;
        }
        let knots = match surface {
            mt::Surface::BSplineSurface(s) => {
                if transpose {
                    s.vknot_vec()
                } else {
                    s.uknot_vec()
                }
            }
            mt::Surface::NurbsSurface(s) => {
                if transpose {
                    s.vknot_vec()
                } else {
                    s.uknot_vec()
                }
            }
            _ => return None,
        };
        if knots.len() > 8192 {
            return None;
        }
        *work = work.checked_sub(knots.len())?;
        let c = mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::try_new(knots.clone(), projected).ok()?));
        let (a, b) = c.range_tuple();
        let support = mt::Edge::new(&mt::Vertex::new(c.subs(a)), &mt::Vertex::new(c.subs(b)), c);
        let Some(fit) = crate::prism_curve::segment(&support, plane, tol, work) else {
            continue;
        };
        let fit_loop = CapLoop { segments: Ring::from(fit.clone()), area: Interval::enclosing(0., 0.)? };
        if let Some((center, radius)) = circle(&fit_loop, tol * 8.)
            && cap.iter().all(|s| matches!(*s,Seg2::Arc {center:c,radius:r,..} if c.dist(center)+(r-radius).abs()<=tol*8.))
        {
            return Some(true);
        }
        let same = loops_same(
            &CapLoop { segments: Ring::from(cap.clone()), area: Interval::enclosing(0., 0.)? },
            &CapLoop { segments: Ring::from(fit), area: Interval::enclosing(0., 0.)? },
            tol * 8.,
            work,
        )?;
        if same {
            return Some(true);
        }
    }
    Some(false)
}

fn circle(lp: &CapLoop, tolerance: f64) -> Option<(Vec2, f64)> {
    let Seg2::Arc { center, radius, sweep: first, .. } = *lp.segments.first()? else { return None };
    let mut sweep = 0.;
    for s in &lp.segments {
        let Seg2::Arc { center: c, radius: r, sweep: a, .. } = *s else { return None };
        if c.dist(center) > tolerance || (r - radius).abs() > tolerance || a * first <= 0. {
            return None;
        }
        sweep += a;
    }
    ((sweep.abs() - std::f64::consts::TAU).abs() * radius <= tolerance).then_some((center, radius))
}

fn loops_same(a: &CapLoop, b: &CapLoop, tolerance: f64, work: &mut usize) -> Option<bool> {
    *work = work.checked_sub(a.segments.len().checked_add(b.segments.len())?)?;
    if (a.area.lo > 0.) != (b.area.lo > 0.) {
        return Some(false);
    }
    if let (Some((ca, ra)), Some((cb, rb))) = (circle(a, tolerance), circle(b, tolerance)) {
        return Some(ca.dist(cb) + (ra - rb).abs() <= tolerance);
    }
    if a.segments.len() != b.segments.len() {
        return Some(false);
    }
    *work = work.checked_sub(a.segments.len().checked_mul(b.segments.len())?)?;
    let mut used = vec![false; b.segments.len()];
    for s in &a.segments {
        let mut match_index = None;
        for (i, t) in b.segments.iter().enumerate() {
            if !used.get(i).copied()? && segment_same(s, t, tolerance) {
                if match_index.is_some() {
                    return None;
                }
                match_index = Some(i);
            }
        }
        let Some(i) = match_index else { return Some(false) };
        *used.get_mut(i)? = true;
    }
    Some(true)
}

struct Certificate {
    loops: Vec<CapLoop>,
    volume: Interval,
    lo: f64,
    hi: f64,
}

fn preflight_body(body: &crate::Body) -> Option<()> {
    if body.solid.boundaries().len() != 1 || body.mesh.is_some() {
        return None;
    }
    let mut faces = 16_384usize;
    let mut edges = 65_536usize;
    for face in body.solid.face_iter() {
        faces = faces.checked_sub(1)?;
        // Borrow absolute wires: the oriented iterator would clone a hostile
        // oversized wire before its edge count has been checked.
        for wire in face.absolute_boundaries() {
            for _ in wire.edge_iter() {
                edges = edges.checked_sub(1)?;
            }
        }
    }
    Some(())
}

/// Initial material scope: a convex straight outer or a circle, with separated
/// circular holes. Wider profiles must acquire their own separation proof.
fn separated_material(loops: &[CapLoop], clearance: f64, work: &mut usize) -> Option<()> {
    let outer = loops.iter().find(|l| l.area.lo > 0.)?;
    if loops.iter().filter(|l| l.area.lo > 0.).count() != 1 {
        return None;
    }
    let outer_circle = circle(outer, clearance);
    if outer_circle.is_none() {
        *work = work.checked_sub(outer.segments.len().checked_mul(outer.segments.len())?)?;
        for s in &outer.segments {
            let Seg2::Line { a, b } = *s else { return None };
            let delta = b - a;
            if delta.len() <= clearance {
                return None;
            }
            for t in &outer.segments {
                if delta.cross(t.start() - a) < -clearance * delta.len() {
                    return None;
                }
            }
        }
        let mut turn = 0.;
        let mut angular_error = 0.;
        for (i, (s, next)) in outer.segments.edges().enumerate() {
            let Seg2::Line { a, b } = *s else {
                return None;
            };
            let x = b - a;
            let y = next.end() - next.start();
            turn += x.cross(y).atan2(x.dot(y));
            angular_error += clearance * 8. / x.len() + 256. * f64::EPSILON;
            for (j, t) in outer.segments.iter().enumerate() {
                if i != j && a.dist(t.start()) <= clearance {
                    return None;
                }
            }
        }
        if angular_error > 1e-3 || (turn - std::f64::consts::TAU).abs() > angular_error {
            return None;
        }
    }
    let mut holes = Vec::new();
    for lp in loops {
        if std::ptr::eq(lp, outer) {
            continue;
        }
        if lp.area.hi >= 0. {
            return None;
        }
        let (center, radius) = circle(lp, clearance)?;
        if let Some((c, r)) = outer_circle {
            if center.dist(c) + radius + clearance >= r {
                return None;
            }
        } else {
            *work = work.checked_sub(outer.segments.len())?;
            for s in &outer.segments {
                let Seg2::Line { a, b } = *s else { return None };
                if (b - a).cross(center - a) / (b - a).len() <= radius + clearance {
                    return None;
                }
            }
        }
        *work = work.checked_sub(holes.len())?;
        for (c, r) in &holes {
            if center.dist(*c) <= radius + r + clearance {
                return None;
            }
        }
        holes.push((center, radius));
    }
    Some(())
}

fn certificate(body: &crate::Body, plane: &Plane, tol: f64) -> Option<Certificate> {
    preflight_body(body)?;
    let d = plane.normal();
    let body_size = body.size();
    let mut remaining = 16_384usize;
    let mut caps = Vec::new();
    let mut walls = Vec::new();
    let mut edge_uses = 65_536usize;
    for face in body.solid.face_iter() {
        remaining = remaining.checked_sub(1)?;
        for _ in face.edge_iter() {
            edge_uses = edge_uses.checked_sub(1)?;
        }
        let surface = face.oriented_surface();
        if let mt::Surface::Plane(p) = &surface {
            let n = p.normal();
            let n = Vec3::new(n.x, n.y, n.z).normalized()?;
            if n.dot(d).abs() > 1. - 1e-12 {
                if n.cross(d).len() * body_size > tol {
                    return None;
                }
                // A planar support's parameter origin may be arbitrarily far
                // from its trimmed face. Use actual boundary geometry only.
                let level = plane.height(from_p3(face.vertex_iter().next()?.point()));
                caps.push((face, level, n.dot(d) > 0.));
                if caps.len() > 2 {
                    return None;
                }
                continue;
            }
        }
        walls.push((face, surface));
    }
    if caps.len() != 2 {
        return None;
    }
    let top = caps.iter().find(|c| c.2)?;
    let bottom = caps.iter().find(|c| !c.2)?;
    let height = top.1 - bottom.1;
    if height <= tol || !height.is_finite() {
        return None;
    }
    for cap in [top, bottom] {
        let mt::Surface::Plane(support) = cap.0.oriented_surface() else {
            return None;
        };
        let origin = from_p3(support.origin());
        let (n, ne) = plane_normal(&support)?;
        for vertex in cap.0.vertex_iter() {
            let point = from_p3(vertex.point());
            let delta = point - origin;
            let terms = (delta.x * n.x).abs() + (delta.y * n.y).abs() + (delta.z * n.z).abs();
            let error = 1024. * f64::EPSILON * terms + delta.x.abs() * ne.x + delta.y.abs() * ne.y + delta.z.abs() * ne.z;
            if delta.dot(n).abs() + error > tol * 2. || (plane.height(point) - cap.1).abs() > tol * 2. {
                return None;
            }
        }
    }
    let mut work = 2_000_000;
    let loops = top.0.boundaries().iter().map(|w| cap_loop(w, plane, tol, &mut work)).collect::<Option<Vec<_>>>()?;
    let lower = bottom.0.boundaries().iter().map(|w| cap_loop(w, plane, tol, &mut work)).collect::<Option<Vec<_>>>()?;
    if loops.len() != lower.len() || loops.is_empty() || loops.len() > 8192 {
        return None;
    }
    let mut pair_work = 16_000_000;
    for a in &loops {
        // Bottom cap has the opposite material orientation.
        let reverse = CapLoop { segments: a.segments.clone(), area: Interval { lo: -a.area.hi, hi: -a.area.lo } };
        let mut matches = 0usize;
        for b in &lower {
            if loops_same(&reverse, b, tol * 8., &mut pair_work)? {
                matches += 1;
            }
        }
        if matches != 1 {
            return None;
        }
    }
    let mut controls = 262_144;
    // Include cap fitting, support fitting and the <=8tol support-to-cap
    // comparison, not just the control-net's projected ruling variation.
    let mut corridor = tol * 16.;
    let cap_edges = top.0.edge_iter().count().checked_add(bottom.0.edge_iter().count())?;
    pair_work = pair_work.checked_sub((65_536 - edge_uses).checked_mul(cap_edges)?)?;
    for (face, surface) in &walls {
        let variation = wall_error(surface, d, height, &mut controls)?;
        if variation > tol * 8. {
            return None;
        }
        corridor = corridor.max(tol * 16. + variation);
        if corridor > tol * 24. {
            return None;
        }
        // Every wall must end on these actual cap wires. This is topology
        // correspondence, not nearest geometry or stale face provenance.
        let mut upper = 0usize;
        let mut lower = 0usize;
        for edge in face.edge_iter() {
            if top.0.edge_iter().any(|e| e.id() == edge.id()) {
                if !wall_supports_cap(surface, &edge, plane, (bottom.1, top.1), tol, &mut work)? {
                    return None;
                }
                upper += 1;
                continue;
            }
            if bottom.0.edge_iter().any(|e| e.id() == edge.id()) {
                if !wall_supports_cap(surface, &edge, plane, (bottom.1, top.1), tol, &mut work)? {
                    return None;
                }
                lower += 1;
                continue;
            }
            let mt::Curve::Line(line) = edge.curve() else { return None };
            if from_p3(line.0).dist(from_p3(edge.absolute_front().point())) > tol || from_p3(line.1).dist(from_p3(edge.absolute_back().point())) > tol
            {
                return None;
            }
            let delta = from_p3(line.1) - from_p3(line.0);
            if delta.cross(d).len() > corridor || delta.dot(d).abs() < height - tol * 8. {
                return None;
            }
        }
        if upper == 0 || upper != lower {
            return None;
        }
    }
    let mut area = Interval::enclosing(0., 0.)?;
    let mut length = 0.;
    let mut count = 0usize;
    for lp in &loops {
        area = area.add(lp.area)?;
        for edge in &lp.segments {
            length += edge.length();
            count = count.checked_add(1)?;
        }
    }
    if count > 8192 || area.lo <= 0. {
        return None;
    }
    separated_material(&loops, corridor * 16., &mut pair_work)?;
    // Each boundary's monotone radial/linear corridor contributes at most a
    // two-sided strip plus four endpoint disks. Unknown intersecting or
    // touching corridors will be refused by the separation check below.
    let strip = 2. * length * corridor + 4. * std::f64::consts::PI * count as f64 * corridor * corridor;
    // Each cap's vertices are within 2tol and its whole rational edge
    // control heights within another tol; include both cap corridors.
    let h_error = tol * 8.;
    if area.lo <= strip || height <= h_error {
        return None;
    }
    let lo = (area.lo - strip) * (height - h_error);
    let hi = (area.hi + strip) * (height + h_error);
    let volume = Interval::enclosing((lo + hi) * 0.5, (hi - lo) * 0.5)?;
    if volume.lo <= 0. || volume.hi - volume.lo > 0.001 {
        return None;
    }
    Some(Certificate { loops, volume, lo: bottom.1, hi: top.1 })
}

/// Independently prove equality of the supported current uniform prisms.
/// `None` means unproved, including budget exhaustion; it never grants a copy.
pub fn certified_prism_equivalent(a: &crate::Body, b: &crate::Body) -> crate::Result<Option<bool>> {
    crate::guard("certified prism comparison", || {
        let result = (|| {
            preflight_body(a)?;
            preflight_body(b)?;
            if !a.size().is_finite() || !b.size().is_finite() {
                return None;
            }
            let size = a.size().max(b.size()).max(1.);
            let tol = size * 1e-10;
            let identity_tol = size * 1e-7;
            let origin = a.solid.vertex_iter().next().map(|v| from_p3(v.point()))?;
            let mut candidates = Vec::new();
            let mut remaining = 16_384usize;
            for f in a.solid.face_iter() {
                remaining = remaining.checked_sub(1)?;
                if let mt::Surface::Plane(p) = f.oriented_surface() {
                    let n = p.normal();
                    let n = Vec3::new(n.x, n.y, n.z).normalized()?;
                    if !candidates.iter().any(|d: &Vec3| d.dot(n).abs() > 1. - 1e-12) {
                        candidates.push(n);
                        if candidates.len() == 8 {
                            break;
                        }
                    }
                }
            }
            for d in candidates {
                let plane = Plane::from_normal(origin, d)?;
                let Some(ca) = certificate(a, &plane, tol) else { continue };
                let Some(cb) = certificate(b, &plane, tol) else { continue };
                if (ca.lo - cb.lo).abs() > identity_tol || (ca.hi - cb.hi).abs() > identity_tol || ca.loops.len() != cb.loops.len() {
                    return Some(false);
                }
                let mut work = 16_000_000;
                let mut used = vec![false; cb.loops.len()];
                for lp in &ca.loops {
                    let mut matched = None;
                    for (i, other) in cb.loops.iter().enumerate() {
                        if !used.get(i).copied()? && loops_same(lp, other, identity_tol, &mut work)? {
                            if matched.is_some() {
                                return None;
                            }
                            matched = Some(i);
                        }
                    }
                    let Some(i) = matched else { return Some(false) };
                    *used.get_mut(i)? = true;
                }
                let delta = (ca.volume.hi - cb.volume.lo).abs().max((cb.volume.hi - ca.volume.lo).abs());
                return Some(delta <= 1e-5 * (1. + ca.volume.lo.abs()));
            }
            None
        })();
        Ok(result)
    })
}

fn cap_loop(wire: &mt::Wire, plane: &Plane, tolerance: f64, work: &mut usize) -> Option<CapLoop> {
    let mut segments = Vec::new();
    let mut area = Interval::enclosing(0., 0.)?;
    let mut ids = std::collections::HashSet::new();
    for edge in wire.edge_iter() {
        if !ids.insert(edge.id()) {
            return None;
        }
        let certified = crate::prism_curve::segment(edge, plane, tolerance, work)?;
        if segments.len().checked_add(certified.len())? > 8192 {
            return None;
        }
        for segment in &certified {
            area = area.add(green_interval(segment, tolerance)?)?;
        }
        segments.extend(certified);
    }
    if segments.is_empty() || !wire.is_closed() {
        return None;
    }
    // Curve endpoints already agree with actual shared vertices in the
    // whole-span proof. Check the fitted representation's continuity too.
    let segments = Ring::new(segments);
    for (a, b) in segments.edges() {
        if a.end().dist(b.start()) > 2. * tolerance {
            return None;
        }
    }
    Some(CapLoop { segments, area })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use mt::builder;

    fn drilled(radius: f64, shift: f64) -> crate::Body {
        use solidcraft_geom::{Loop2, Region2};
        let region = Region2 {
            outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(20., 0.), Vec2::new(20., 20.), Vec2::new(0., 20.)]),
            holes: vec![Loop2::circle(Vec2::new(10. + shift, 10.), radius)],
        };
        crate::extrude(&Plane::XY, &[region], 0., 10.).unwrap().remove(0)
    }

    #[test]
    fn actual_drilled_prism_and_step_reopen_are_certified_without_history() {
        let source = drilled(6., 0.);
        let before = crate::step_export(&[&source], "source").unwrap();
        let imported = crate::step_import(&before).unwrap();
        let copy = &imported.bodies[0].body;
        let exact = 4000. - 360. * std::f64::consts::PI;
        for body in [&source, copy] {
            let cap = certificate(body, &Plane::XY, 3e-9).expect("actual complete current cap and walls");
            assert!(cap.volume.lo <= exact && cap.volume.hi >= exact, "{:?}, expected {exact}", cap.volume);
            assert!(cap.volume.hi - cap.volume.lo <= 0.001);
        }
        assert_eq!(certified_prism_equivalent(&source, copy).unwrap(), Some(true));
        let wider = drilled(6.0001, 0.);
        let a = certificate(&source, &Plane::XY, 3e-9).unwrap();
        let b = certificate(&wider, &Plane::XY, 3e-9).unwrap();
        assert!(a.volume.lo - b.volume.hi > 1e-5 * (1. + a.volume.hi));
        assert_eq!(certified_prism_equivalent(&source, &wider).unwrap(), Some(false));
        let shifted = drilled(6., 0.0001);
        let same_volume = certificate(&shifted, &Plane::XY, 3e-9).unwrap();
        assert!(same_volume.volume.lo <= exact && same_volume.volume.hi >= exact);
        assert_eq!(certified_prism_equivalent(&source, &shifted).unwrap(), Some(false));
        assert_eq!(crate::step_export(&[&source], "source").unwrap(), before);
    }

    #[test]
    fn poorly_conditioned_plane_walls_do_not_get_certificates() {
        let origin = mt::Point3::new(0., 0., 0.);
        let u = mt::Point3::new(1., 0., 0.);
        let v = mt::Point3::new(1., 0., 1e-9);
        let surface = mt::Surface::Plane(mt::Plane::new(origin, u, v));
        let edge = builder::line(&mt::Vertex::new(origin), &mt::Vertex::new(u));
        let mut work = 2_000_000;
        assert!(wall_error(&surface, Vec3::Z, 10., &mut work).is_none());
        assert!(wall_supports_cap(&surface, &edge, &Plane::XY, (0., 10.), 1e-9, &mut work).is_none());
        let normal = plane_normal(&mt::Plane::new(origin, u, mt::Point3::new(0., 0., 1.))).unwrap();
        assert!(normal.0.dist(-Vec3::Y) < 1e-12);
        assert!(normal.1.len() > 0.);
    }

    #[test]
    fn rotated_current_boolean_prism_step_and_wrong_surface_incidence() {
        let stock = crate::box_solid(Vec3::ZERO, Vec3::new(20., 20., 10.)).unwrap();
        let cutter = crate::cylinder(Vec3::new(10., 10., 0.), Vec3::Z, 6., 10.).unwrap();
        let source = crate::boolean(&stock, &cutter, crate::BoolOp::Cut).unwrap().unwrap();
        let moved = crate::transform(&source, Vec3::new(30., 20., 10.), Vec3::ZERO, Vec3::new(1., 2., 3.), 0.7).unwrap();
        let saved = crate::step_export(&[&moved], "actual boolean").unwrap();
        let imported = crate::step_import(&saved).unwrap();
        assert_eq!(certified_prism_equivalent(&moved, &imported.bodies[0].body).unwrap(), Some(true));
        // Preserve every current cap/edge ID while replacing only the curved
        // wall support with radius five. Topology incidence alone must fail.
        let wrong = drilled(5., 0.);
        let wrong_surface = wrong.solid.face_iter().find(|f| !matches!(f.surface(), mt::Surface::Plane(_))).unwrap().surface();
        let faces = source
            .solid
            .face_iter()
            .map(|f| {
                if matches!(f.surface(), mt::Surface::Plane(_)) {
                    return f.clone();
                }
                let mut face = mt::Face::new(f.absolute_boundaries().clone(), wrong_surface.clone());
                if !f.orientation() {
                    face.invert();
                }
                face
            })
            .collect::<Vec<_>>();
        let invalid = crate::Body::new(crate::body::Solid::try_new(vec![faces.into()]).unwrap()).unwrap();
        assert!(certificate(&invalid, &Plane::XY, 3e-9).is_none());
        let actual = source.solid.face_iter().find(|f| !matches!(f.surface(), mt::Surface::Plane(_))).unwrap().surface();
        let shifted = mt::Transformed::transformed(&actual, mt::Matrix4::from_translation(mt::Vector3::new(0., 0., 1000.)));
        let top = source.solid.face_iter().find(|f| matches!(f.oriented_surface(),mt::Surface::Plane(p) if p.normal().z>0.9)).unwrap();
        let arc = top.edge_iter().find(|e| !matches!(e.curve(), mt::Curve::Line(_))).unwrap();
        assert_ne!(wall_supports_cap(&shifted, &arc, &Plane::XY, (0., 10.), 3e-9, &mut 2_000_000), Some(true));
    }

    #[test]
    fn cap_support_origin_is_not_a_trimmed_face_height() {
        let source = drilled(6., 0.);
        let faces = source
            .solid
            .face_iter()
            .map(|f| {
                let mt::Surface::Plane(p) = f.surface() else {
                    return f.clone();
                };
                if p.normal().z.abs() < 0.9 {
                    return f.clone();
                }
                let origin = p.origin() + p.u_axis() * 1e8;
                let plane = mt::Surface::Plane(mt::Plane::new(origin, origin + p.u_axis(), origin + p.v_axis()));
                let mut face = mt::Face::new(f.absolute_boundaries().clone(), plane);
                if !f.orientation() {
                    face.invert();
                }
                face
            })
            .collect::<Vec<_>>();
        let same = crate::Body::new(crate::body::Solid::try_new(vec![faces.into()]).unwrap()).unwrap();
        assert_eq!(certified_prism_equivalent(&source, &same).unwrap(), Some(true));
        assert!(
            loops_same(
                &cap_loop(&source.solid.face_iter().last().unwrap().boundaries()[0], &Plane::XY, 3e-9, &mut 2_000_000).unwrap(),
                &cap_loop(&source.solid.face_iter().last().unwrap().boundaries()[0], &Plane::XY, 3e-9, &mut 2_000_000).unwrap(),
                1e-6,
                &mut 0
            )
            .is_none()
        );
    }

    #[test]
    fn winding_retracing_and_uncertain_material_do_not_get_certificates() {
        let arc = Seg2::Arc { center: Vec2::ZERO, radius: 6., start: 0., sweep: std::f64::consts::TAU };
        let retraced =
            CapLoop { segments: Ring::from([arc, arc.reversed(), arc]), area: Interval::enclosing(36. * std::f64::consts::PI, 1e-8).unwrap() };
        assert!(circle(&retraced, 1e-6).is_none());
        let region = solidcraft_geom::Loop2::polygon(&[Vec2::ZERO, Vec2::new(20., 0.), Vec2::new(20., 20.), Vec2::new(0., 20.)]);
        let doubled = CapLoop { segments: region.segs.iter().chain(&region.segs).copied().collect(), area: Interval::enclosing(800., 1e-8).unwrap() };
        assert!(separated_material(&[doubled], 1e-6, &mut 16_000_000).is_none());
        let thin = crate::box_solid(Vec3::ZERO, Vec3::new(20., 20., 1e-5)).unwrap();
        assert!(certificate(&thin, &Plane::XY, 3e-6).is_none());
    }

    #[test]
    fn current_rational_circle_green_interval_encloses_independent_area() {
        let r = 6.;
        let vertices =
            [mt::Point3::new(r, 0., 0.), mt::Point3::new(0., r, 0.), mt::Point3::new(-r, 0., 0.), mt::Point3::new(0., -r, 0.)].map(builder::vertex);
        let mut edges = Vec::new();
        for i in 0..4 {
            let angle = (i as f64 + 0.5) * std::f64::consts::FRAC_PI_2;
            edges.push(builder::circle_arc(&vertices[i], &vertices[(i + 1) % 4], mt::Point3::new(r * angle.cos(), r * angle.sin(), 0.)));
        }
        let wire: mt::Wire = edges.into();
        let cap = cap_loop(&wire, &Plane::XY, 3e-9, &mut 2_000_000).unwrap();
        let exact = std::f64::consts::PI * r * r;
        assert!(cap.area.lo <= exact && cap.area.hi >= exact, "{:?}", cap.area);
        assert!((cap.area.hi - cap.area.lo) * 10. < 0.001);
        assert_eq!(cap.segments.len(), 4);
        let reversed = cap_loop(&wire.inverse(), &Plane::XY, 3e-9, &mut 2_000_000).unwrap();
        assert!(reversed.area.lo <= -exact && reversed.area.hi >= -exact);
    }
}
