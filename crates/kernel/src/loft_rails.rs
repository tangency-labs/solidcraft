//! Guide rails for a loft (#201): curves the loft follows between its sections. A rail runs from
//! the first section to the last and passes through every section's curve. Between each two
//! sections, stations are added: each is the plain loft there, moved so that the point of it a
//! rail guides lands on the rail. With one rail the station turns about the loft's direction
//! there and grows or shrinks about its middle (stretches across to the rail, with
//! [`RailFit::Stretch`]), shifted along the loft as far as the rail leads or lags; with two it
//! moves with the first rail, turns with the line between the rails and sizes with their distance
//! (stretches along that line), leaning to follow the second. At the sections the move is none,
//! so the sections and the loft's caps stay exactly as drawn; the loft's smooth surfaces then run
//! through the sections and the stations together.

use mt::{BoundedCurve, ParametricCurve};
use solidcraft_geom::{Plane, Vec3};
use truck_modeling as mt;

use crate::body::{from_p3, p3};
use crate::sweep_path::RailFit;
use crate::{KernelError, Result};

/// Most wires a railed loft is skinned through, sections and stations together.
const MAX_WIRES: usize = 33;
/// Most stations between two sections.
const MAX_PER_SPAN: usize = 15;

type Curve = mt::BSplineCurve<mt::Point3>;

/// Where a rail meets a section: the point of the section's wire it guides (as a wire parameter:
/// the edge's index plus how far along the edge), that point, how far it is from where the rail
/// crosses the section's plane (the nudge onto it), and how far along the rail that crossing is
/// (segment and fraction).
struct Anchor {
    g: f64,
    on: Vec3,
    nudge: Vec3,
    at: (usize, f64),
}

/// A rail as a polyline from the first section, and where it meets each section.
struct Rail {
    pts: Vec<Vec3>,
    anchors: Vec<Anchor>,
}

/// An affine move: `x` goes to `to + m·(x − from)` (`m` by columns).
struct Affine {
    from: Vec3,
    to: Vec3,
    m: [Vec3; 3],
}

impl Affine {
    fn apply(&self, x: Vec3) -> Vec3 {
        let d = x - self.from;
        self.to + self.m[0] * d.x + self.m[1] * d.y + self.m[2] * d.z
    }
}

/// The loft's wires with stations added between the sections so that the loft follows `rails`
/// (polylines; see the module notes). `planes` are the sections' planes, `wires` their matched
/// wires (as [`crate::build::loft_wires`] or a sheet loft makes them: the k-th edges of every
/// wire are B-splines on one knot vector), both in loft order; `closed` whether the wires close.
/// Without rails the wires come back as they are.
pub(crate) fn railed_wires(planes: &[Plane], wires: &[mt::Wire], closed: bool, rails: &[Vec<Vec3>], fit: RailFit) -> Result<Vec<mt::Wire>> {
    if rails.is_empty() {
        return Ok(wires.to_vec());
    }
    if rails.len() > 2 {
        return Err(KernelError::Invalid("a loft takes one or two guide rails".into()));
    }
    let fail = |m: &str| KernelError::Failed(format!("loft rails: {m}"));
    let count = wires.len();
    if count < 2 || planes.len() != count {
        return Err(fail("sections"));
    }
    // Each section's edge curves, and the knot vector the k-th edges share.
    let curves: Vec<Vec<Curve>> = wires
        .iter()
        .map(|w| {
            w.edge_iter()
                .map(|e| match e.curve() {
                    mt::Curve::BSplineCurve(c) if e.orientation() => Some(c),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<_>>()
        .ok_or_else(|| fail("section curves"))?;
    let first = curves.first().ok_or_else(|| fail("sections"))?;
    let n = first.len();
    let knots: Vec<mt::KnotVec> = first.iter().map(|c| c.knot_vec().clone()).collect();
    let sizes: Vec<usize> = first.iter().map(|c| c.control_points().len()).collect();
    if n == 0
        || curves
            .iter()
            .any(|cs| cs.len() != n || cs.iter().zip(&knots).zip(&sizes).any(|((c, k), m)| c.knot_vec() != k || c.control_points().len() != *m))
    {
        return Err(fail("sections parameterised differently"));
    }
    let ctrl: Vec<Vec<Vec<Vec3>>> =
        curves.iter().map(|cs| cs.iter().map(|c| c.control_points().iter().map(|p| from_p3(*p)).collect()).collect()).collect();
    let edges: Vec<Vec<mt::Edge>> = wires.iter().map(|w| w.edge_iter().cloned().collect()).collect();
    let along = crate::build::section_parameters(&edges);
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for p in ctrl.iter().flatten().flatten() {
        if !p.is_finite() {
            return Err(fail("sections"));
        }
        bounds.add(*p);
    }
    let size = bounds.diagonal();
    let tol = (size * 1e-4).max(1e-6);
    let planes = section_planes(planes, &ctrl, tol);
    let start = centroid(ctrl.first().ok_or_else(|| fail("sections"))?);
    let rails: Vec<Rail> = rails.iter().map(|r| rail(r, start, &planes, &curves, tol)).collect::<Result<_>>()?;
    if let [a, b] = &rails[..]
        && a.anchors.iter().zip(&b.anchors).any(|(p, q)| p.on.dist(q.on) <= tol)
    {
        return Err(KernelError::Invalid("the two guide rails meet a section at the same point".into()));
    }
    // The plain loft between the sections: each control point across them, straight between two
    // sections, the smooth loft's curve through three or more.
    let across: Vec<Vec<Option<Curve>>> = if count == 2 {
        Vec::new()
    } else {
        (0..n)
            .map(|k| {
                (0..sizes.get(k).copied().unwrap_or(0))
                    .map(|j| {
                        let pts: Vec<Vec3> = ctrl.iter().filter_map(|s| s.get(k).and_then(|e| e.get(j)).copied()).collect();
                        crate::build::across(&pts, &along)
                    })
                    .collect()
            })
            .collect()
    };
    let plain = |v: f64| -> Result<Vec<Vec<Vec3>>> {
        let (Some(a), Some(b)) = (ctrl.first(), ctrl.last()) else { return Err(fail("sections")) };
        if count == 2 {
            return Ok(a.iter().zip(b).map(|(ea, eb)| ea.iter().zip(eb).map(|(p, q)| p.lerp(*q, v)).collect()).collect());
        }
        across.iter().map(|cs| cs.iter().map(|c| c.as_ref().map(|c| from_p3(c.subs(v))).ok_or_else(|| fail("the plain loft"))).collect()).collect()
    };
    // The middle of the plain loft (the middles of the sections' control points, across them as
    // the control points are: the middle is linear in them) and its direction.
    let middles: Vec<Vec3> = ctrl.iter().map(|s| centroid(s)).collect();
    let middle_curve = if count == 2 { None } else { Some(crate::build::across(&middles, &along).ok_or_else(|| fail("the plain loft"))?) };
    let middle = |v: f64| -> Result<(Vec3, Vec3)> {
        let (p, d) = match (&middle_curve, middles.first(), middles.last()) {
            (Some(c), ..) => {
                let d = c.der(v);
                (from_p3(c.subs(v)), Vec3::new(d.x, d.y, d.z))
            }
            (None, Some(a), Some(b)) => (a.lerp(*b, v), *b - *a),
            _ => return Err(fail("sections")),
        };
        let d = d.normalized().ok_or_else(|| KernelError::Invalid("two loft sections sit on top of each other".into()))?;
        Ok((p, d))
    };
    // The sections' normals, turned to agree with the first's.
    let mut normals: Vec<Vec3> = Vec::with_capacity(count);
    for p in &planes {
        let n = p.normal();
        let prev = normals.last().copied().unwrap_or(n);
        normals.push(if n.dot(prev) < 0.0 { n * -1.0 } else { n });
    }
    let spans = count - 1;
    let per = (MAX_WIRES.saturating_sub(count) / spans).clamp(1, MAX_PER_SPAN);
    let mut out = Vec::with_capacity(count + spans * per);
    for i in 0..spans {
        out.push(wires.get(i).cloned().ok_or_else(|| fail("sections"))?);
        let (Some(&v0), Some(&v1), Some(&n0), Some(&n1)) = (along.get(i), along.get(i + 1), normals.get(i), normals.get(i + 1)) else {
            return Err(fail("sections"));
        };
        // Each rail's anchors at the span's two sections, and where its walk has got to.
        let mut spans_of: Vec<(&Rail, &Anchor, &Anchor, (usize, f64))> = Vec::with_capacity(rails.len());
        for r in &rails {
            let (Some(a0), Some(a1)) = (r.anchors.get(i), r.anchors.get(i + 1)) else { return Err(fail("rail")) };
            spans_of.push((r, a0, a1, a0.at));
        }
        for q in 1..=per {
            let f = q as f64 / (per + 1) as f64;
            let v = v0 + f * (v1 - v0);
            let station = plain(v)?;
            let (mid, dir) = middle(v)?;
            // The station's plane turns from one section's to the next's.
            let normal = n0.lerp(n1, f).normalized().ok_or_else(|| fail("section planes"))?;
            let mut pairs = Vec::with_capacity(rails.len());
            for (r, a0, a1, cursor) in &mut spans_of {
                let anchor = wire_point(&station, &knots, interpolate_g(a0.g, a1.g, f, n, closed))?;
                // Where the rail crosses the station's plane through the point it guides, nudged
                // so that at each section it is the section's own point (a rail drawn a hair off
                // a section moves nothing there).
                let Some((hit, at)) = crate::sweep_path::rail_crossing(&r.pts, anchor, normal, *cursor, tol) else {
                    return Err(KernelError::Invalid(format!(
                        "a guide rail turns back between sections {} and {}: it has to run on from one section to the next",
                        i + 1,
                        i + 2
                    )));
                };
                *cursor = at;
                let target = hit + a0.nudge * (1.0 - f) + a1.nudge * f;
                pairs.push((anchor, target));
            }
            let map = station_map(&pairs, mid, dir, fit, size)?;
            let moved: Vec<Vec<Vec3>> = station.iter().map(|e| e.iter().map(|p| map.apply(*p)).collect()).collect();
            out.push(wire_of(&moved, &knots, closed)?);
        }
    }
    out.push(wires.last().cloned().ok_or_else(|| fail("sections"))?);
    Ok(out)
}

/// The rail `pts` from its end nearer `start` (the first section's middle), where it meets each
/// section: across the section's plane, on the section's curve within `tol`.
fn rail(pts: &[Vec3], start: Vec3, planes: &[Plane], curves: &[Vec<Curve>], tol: f64) -> Result<Rail> {
    let mut pts =
        crate::sweep_path::clean(pts).map_err(|_| KernelError::Invalid("a guide rail needs 2…100000 finite points along its length".into()))?;
    if let (Some(a), Some(b)) = (pts.first(), pts.last())
        && b.dist(start) < a.dist(start)
    {
        pts.reverse();
    }
    let mut cursor = (0usize, 0.0f64);
    let mut anchors = Vec::with_capacity(planes.len());
    for (i, (plane, section)) in planes.iter().zip(curves).enumerate() {
        let Some((hit, at)) = crate::sweep_path::rail_crossing(&pts, plane.origin, plane.normal(), cursor, tol) else {
            return Err(KernelError::Invalid(format!(
                "a guide rail does not reach section {}: a rail has to run from the first section to the last, through every one",
                i + 1
            )));
        };
        cursor = at;
        let (g, on, d) = nearest_on(section, hit).ok_or_else(|| KernelError::Failed("loft rails: section".into()))?;
        if d > tol {
            return Err(KernelError::Invalid(format!(
                "a guide rail crosses section {}'s plane {d:.3} mm from its curves: a rail has to pass through every section",
                i + 1
            )));
        }
        anchors.push(Anchor { g, on, nudge: on - hit, at });
    }
    Ok(Rail { pts, anchors })
}

/// The point of a section's wire (its edge curves) nearest `p`: as a wire parameter, the point,
/// and how far it is.
fn nearest_on(section: &[Curve], p: Vec3) -> Option<(f64, Vec3, f64)> {
    let mut best: Option<(f64, Vec3, f64)> = None;
    for (k, c) in section.iter().enumerate() {
        let (t0, t1) = c.range_tuple();
        let t = crate::curveblend::param_on(&mt::Curve::BSplineCurve(c.clone()), p)?;
        let q = from_p3(c.subs(t));
        let d = q.dist(p);
        if best.as_ref().is_none_or(|b| d < b.2) {
            best = Some((k as f64 + (t - t0) / (t1 - t0).max(1e-300), q, d));
        }
    }
    best
}

/// A wire parameter `f` of the way from `g0` to `g1`; round a closed wire of `n` edges the
/// shorter way.
fn interpolate_g(g0: f64, g1: f64, f: f64, n: usize, closed: bool) -> f64 {
    let n = n as f64;
    if !closed {
        return (g0 + f * (g1 - g0)).clamp(0.0, n);
    }
    let mut d = g1 - g0;
    if d > n / 2.0 {
        d -= n;
    } else if d < -n / 2.0 {
        d += n;
    }
    (g0 + f * d).rem_euclid(n)
}

/// The point at wire parameter `g` of a wire given by its edges' control points on `knots`.
fn wire_point(ctrl: &[Vec<Vec3>], knots: &[mt::KnotVec], g: f64) -> Result<Vec3> {
    let n = ctrl.len();
    let k = (g.floor().max(0.0) as usize).min(n.saturating_sub(1));
    let (Some(pts), Some(kv)) = (ctrl.get(k), knots.get(k)) else { return Err(KernelError::Failed("loft rails: wire".into())) };
    let c = edge_curve(kv, pts)?;
    let (t0, t1) = c.range_tuple();
    let f = (g - k as f64).clamp(0.0, 1.0);
    Ok(from_p3(c.subs(t0 + (t1 - t0) * f)))
}

/// The middle of a wire's control points.
/// The sections' planes as the rails meet them. A section whose curves run along one line lies in
/// every plane through that line, so the plane of the sketch it was drawn in says nothing about the
/// loft: it takes the plane through the line that faces most nearly as the nearest curved section
/// does, or along the loft when every section is straight. A curved section's curves fix its plane (#4056).
fn section_planes(planes: &[Plane], ctrl: &[Vec<Vec<Vec3>>], tol: f64) -> Vec<Plane> {
    let lines: Vec<Option<Vec3>> = ctrl.iter().map(|s| line_of(&s.iter().flatten().copied().collect::<Vec<_>>(), tol)).collect();
    let middles: Vec<Vec3> = ctrl.iter().map(|s| centroid(s)).collect();
    let curved = |j: usize| lines.get(j).is_some_and(Option::is_none);
    planes
        .iter()
        .enumerate()
        .map(|(i, plane)| {
            let (Some(Some(d)), Some(&mid)) = (lines.get(i), middles.get(i)) else { return *plane };
            let nearest = (1..planes.len()).flat_map(|k| [i.checked_sub(k), Some(i + k)]).flatten().find(|j| curved(*j));
            let facing = nearest.and_then(|j| planes.get(j)).map(Plane::normal);
            let along = match (middles.get(i + 1), i.checked_sub(1).and_then(|j| middles.get(j))) {
                (Some(next), _) => Some(*next - mid),
                (None, Some(prev)) => Some(mid - *prev),
                (None, None) => None,
            };
            // The first way to face that is not along the line itself, turned square to it.
            [facing, along]
                .into_iter()
                .flatten()
                .filter_map(|r| r.normalized())
                .map(|r| r - *d * r.dot(*d))
                .find(|n| n.len() > 1e-6)
                .and_then(|n| Plane::new(mid, *d, n.cross(*d)))
                .unwrap_or(*plane)
        })
        .collect()
}

/// The direction of the line every point of `pts` lies on (within `tol`), if there is one.
fn line_of(pts: &[Vec3], tol: f64) -> Option<Vec3> {
    let a = *pts.first()?;
    let far = pts.iter().copied().max_by(|p, q| p.dist(a).total_cmp(&q.dist(a)))?;
    let d = (far - a).normalized()?;
    pts.iter().all(|p| (*p - a - d * (*p - a).dot(d)).len() <= tol).then_some(d)
}

fn centroid(ctrl: &[Vec<Vec3>]) -> Vec3 {
    let (sum, n) = ctrl.iter().flatten().fold((Vec3::ZERO, 0usize), |(s, n), p| (s + *p, n + 1));
    sum * (1.0 / n.max(1) as f64)
}

/// The move that puts a station's guided points onto the rails (`pairs`: each point with its
/// rail's point), the station's middle `middle`, the loft's direction there `dir` (unit); `size`
/// the loft's size. One rail: turned about `dir` and sized about the middle, shifted along `dir`;
/// two: carried with the first, turned and sized with the line between them, leaning along `dir`.
fn station_map(pairs: &[(Vec3, Vec3)], middle: Vec3, dir: Vec3, fit: RailFit, size: f64) -> Result<Affine> {
    let (from, to, a, r, two) = match pairs {
        [(p, q)] => (middle, middle, *p - middle, *q - middle, false),
        [(p1, q1), (p2, q2)] => (*p1, *q1, *p2 - *p1, *q2 - *q1, true),
        _ => return Err(KernelError::Invalid("a loft takes one or two guide rails".into())),
    };
    let flat = |v: Vec3| v - dir * v.dot(dir);
    let (ap, rp) = (flat(a), flat(r));
    let tiny = 1e-6 * size.max(1e-9);
    let (Some(u0), Some(u1)) = (ap.normalized().filter(|_| ap.len() > tiny), rp.normalized().filter(|_| rp.len() > tiny)) else {
        return Err(KernelError::Invalid(if two {
            "the two guide rails come together between the sections: the loft would shrink to nothing".into()
        } else {
            "a guide rail runs through the middle of the loft: it has to guide a point of the sections away from their middle".into()
        }));
    };
    // Turned about the loft's direction and sized as the sweep's rails size its profile.
    let mut m = crate::sweep_path::rail_frame(dir, u0, dir, u1, rp.len() / ap.len(), fit);
    // Whatever the rails ask along the loft: one rail shifts the station that far, two lean it,
    // in step with the way across.
    let lead = (r - a).dot(dir);
    if two {
        let lean = lead / ap.len();
        for (col, e) in m.iter_mut().zip([Vec3::X, Vec3::Y, Vec3::Z]) {
            *col += dir * (lean * e.dot(u0));
        }
    }
    let shift = if two { Vec3::ZERO } else { dir * lead };
    Ok(Affine { from, to: to + shift, m })
}

/// A wire of B-spline edges with these control points on these knots, joined end to end
/// (closing back on its first edge's start when `closed`).
fn wire_of(ctrl: &[Vec<Vec3>], knots: &[mt::KnotVec], closed: bool) -> Result<mt::Wire> {
    let fail = || KernelError::Failed("loft rails: station".into());
    let n = ctrl.len();
    let mut verts: Vec<mt::Vertex> = ctrl.iter().map(|e| e.first().map(|p| mt::Vertex::new(p3(*p)))).collect::<Option<_>>().ok_or_else(fail)?;
    if !closed {
        verts.push(mt::Vertex::new(p3(*ctrl.last().and_then(|e| e.last()).ok_or_else(fail)?)));
    }
    let mut edges = Vec::with_capacity(n);
    for (i, (pts, kv)) in ctrl.iter().zip(knots).enumerate() {
        let (Some(a), Some(b)) = (verts.get(i), verts.get(if closed { (i + 1) % n } else { i + 1 })) else { return Err(fail()) };
        let c = edge_curve(kv, pts)?;
        edges.push(mt::Edge::new(a, b, mt::Curve::BSplineCurve(c)));
    }
    Ok(edges.into())
}

/// An edge's B-spline from its control points on its knots.
fn edge_curve(kv: &mt::KnotVec, pts: &[Vec3]) -> Result<Curve> {
    Curve::try_new(kv.clone(), pts.iter().map(|p| p3(*p)).collect()).map_err(|e| KernelError::Failed(format!("loft rails: {e}")))
}
