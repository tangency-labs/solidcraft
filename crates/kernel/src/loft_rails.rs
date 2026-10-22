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
//!
//! Sections are exact NURBS curves ([`Section`], from the sketch segments' record curves); a
//! station is the plain loft's control points moved by an affine map, so each is exact too.

use solidcraft_geom::bspline::CurveN;
use solidcraft_geom::{Plane, Seg2, Vec3};

use crate::sweep_path::RailFit;
use crate::{KernelError, Result};

/// Most curves a railed loft is skinned through per column, sections and stations together.
pub(crate) const MAX_WIRES: usize = 33;
/// Most stations between two sections.
pub(crate) const MAX_PER_SPAN: usize = 15;

/// A loft section: its plane and its curves in order, each a NURBS curve with homogeneous control
/// points `[x w, y w, z w, w]`, and the profile segments each curve stands for.
#[derive(Clone, Debug)]
pub(crate) struct Section {
    pub plane: Plane,
    pub curves: Vec<CurveN<4>>,
    pub numbers: Vec<Vec<usize>>,
}

impl Section {
    /// The sketch segments `segs` on `plane` as a section (their exact record curves), segment i
    /// standing for the profile segments `numbers[i]`.
    pub(crate) fn of(plane: &Plane, segs: &[Seg2], numbers: Vec<Vec<usize>>) -> Result<Section> {
        let curves = segs.iter().map(|s| Ok(nurbs(&crate::freeform::segment(plane, s)?.0)?)).collect::<Result<Vec<_>>>()?;
        Ok(Section { plane: *plane, curves, numbers })
    }
}

/// A record line or (rational) B-spline as a homogeneous NURBS curve.
fn nurbs(c: &solidcraft_geom::brep::Curve) -> Result<CurveN<4>> {
    use solidcraft_geom::brep::Curve;
    let degree = |knots: &[f64], n: usize| knots.len().checked_sub(n + 1).ok_or_else(|| KernelError::Invalid("spline degree".into()));
    Ok(match c {
        Curve::Line { start, end } => {
            CurveN { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], ctrl: vec![[start[0], start[1], start[2], 1.0], [end[0], end[1], end[2], 1.0]] }
        }
        Curve::Spline { knots, points } => {
            CurveN { degree: degree(knots, points.len())?, knots: knots.clone(), ctrl: points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect() }
        }
        Curve::RationalSpline { knots, homogeneous } => {
            CurveN { degree: degree(knots, homogeneous.len())?, knots: knots.clone(), ctrl: homogeneous.clone() }
        }
        _ => return Err(KernelError::Invalid("a loft section curve that is not a line or a spline".into())),
    })
}

/// A curve's control points (x, y, z) and weights.
fn points(c: &CurveN<4>) -> (Vec<Vec3>, Vec<f64>) {
    c.ctrl.iter().map(|h| (Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]), h[3])).unzip()
}

/// `c` with the control points `pts` (its own weights and knots).
fn with(c: &CurveN<4>, pts: &[Vec3]) -> CurveN<4> {
    let ctrl = pts.iter().zip(&c.ctrl).map(|(p, h)| [p.x * h[3], p.y * h[3], p.z * h[3], h[3]]).collect();
    CurveN { degree: c.degree, knots: c.knots.clone(), ctrl }
}

/// The point `f` (0…1) of the way along a curve's parameter range.
fn point_at(c: &CurveN<4>, f: f64) -> Option<Vec3> {
    let (t0, t1) = (*c.knots.get(c.degree)?, *c.knots.get(c.ctrl.len())?);
    let ([x, y, z, w], _) = c.eval(t0 + (t1 - t0) * f.clamp(0.0, 1.0))?;
    (w.abs() > 1e-300).then(|| Vec3::new(x / w, y / w, z / w))
}

/// Whether two curves pair up: the same degree, knots and weights (their control points
/// correspond).
fn pairs(a: &CurveN<4>, b: &CurveN<4>) -> bool {
    let near = |x: f64, y: f64| (x - y).abs() <= 1e-9 * (1.0 + x.abs());
    a.degree == b.degree
        && a.ctrl.len() == b.ctrl.len()
        && a.knots.len() == b.knots.len()
        && a.knots.iter().zip(&b.knots).all(|(x, y)| near(*x, *y))
        && a.ctrl.iter().zip(&b.ctrl).all(|(x, y)| near(x[3], y[3]))
}

/// Where a rail meets a section: the point of the section it guides (as a wire parameter: the
/// curve's index plus how far along its parameter range), that point, the nudge onto it from where
/// the rail meets the section's plane, and how far along the rail that is (segment and fraction).
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
pub(crate) struct Affine {
    from: Vec3,
    to: Vec3,
    m: [Vec3; 3],
}

impl Affine {
    pub(crate) fn apply(&self, x: Vec3) -> Vec3 {
        let d = x - self.from;
        self.to + self.m[0] * d.x + self.m[1] * d.y + self.m[2] * d.z
    }
}

/// The loft's sections with stations added between them so that the loft follows `rails`
/// (polylines; see the module notes), in loft order: each entry one section's (or station's)
/// curves. `closed` says whether the sections close. Without rails the sections' own curves come
/// back. With rails, every section's k-th curve has to pair with the others' (the same kind,
/// so their control points correspond).
pub(crate) fn railed(sections: &[Section], closed: bool, rails: &[Vec<Vec3>], fit: RailFit) -> Result<Vec<Vec<CurveN<4>>>> {
    if rails.is_empty() {
        return Ok(sections.iter().map(|s| s.curves.clone()).collect());
    }
    if rails.len() > 2 {
        return Err(KernelError::Invalid("a loft takes one or two guide rails".into()));
    }
    let fail = |m: &str| KernelError::Failed(format!("loft rails: {m}"));
    let count = sections.len();
    let lead = sections.first().map(|s| &s.curves).ok_or_else(|| fail("sections"))?;
    let n = lead.len();
    if count < 2 || n == 0 || sections.iter().any(|s| s.curves.len() != n || s.curves.iter().zip(lead).any(|(a, b)| !pairs(a, b))) {
        return Err(KernelError::Invalid(
            "not supported yet: a loft along guide rails between sections whose curves differ in kind (a line against an arc)".into(),
        ));
    }
    let ctrl: Vec<Vec<Vec<Vec3>>> = sections.iter().map(|s| s.curves.iter().map(|c| points(c).0).collect()).collect();
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for p in ctrl.iter().flatten().flatten() {
        if !p.is_finite() {
            return Err(fail("sections"));
        }
        bounds.add(*p);
    }
    let size = bounds.diagonal();
    let tol = (size * 1e-4).max(1e-6);
    // The sections' parameters along the loft: in proportion to the mean distance between their
    // curves' starts.
    let mut along = vec![0.0];
    for w in ctrl.windows(2) {
        let step = w[0].iter().zip(&w[1]).filter_map(|(a, b)| Some(a.first()?.dist(*b.first()?))).sum::<f64>() / n as f64;
        along.push(along.last().copied().unwrap_or(0.0) + step);
    }
    let total = along.last().copied().unwrap_or(0.0);
    if !(total > 1e-9) || along.windows(2).any(|w| !(w[1] - w[0] > 1e-9 * total)) {
        return Err(KernelError::Invalid("two loft sections sit on top of each other".into()));
    }
    along.iter_mut().for_each(|u| *u /= total);
    // The plain loft between the sections: each control point straight between two sections, the
    // curve (quadratic, or cubic from four) through them across more.
    let degree = (count - 1).min(3);
    let across: Vec<Vec<CurveN<3>>> = if count == 2 {
        Vec::new()
    } else {
        (0..n)
            .map(|k| {
                (0..lead.get(k).map_or(0, |c| c.ctrl.len()))
                    .map(|j| {
                        let pts: Vec<[f64; 3]> = ctrl.iter().filter_map(|s| s.get(k)?.get(j).map(|q| [q.x, q.y, q.z])).collect();
                        CurveN::interpolate(&along, &pts, degree).map_err(|e| fail(&e.to_string()))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<_>>()?
    };
    let plain = |v: f64| -> Result<Vec<Vec<Vec3>>> {
        let (Some(a), Some(b)) = (ctrl.first(), ctrl.last()) else { return Err(fail("sections")) };
        if count == 2 {
            return Ok(a.iter().zip(b).map(|(ea, eb)| ea.iter().zip(eb).map(|(p, q)| p.lerp(*q, v)).collect()).collect());
        }
        across
            .iter()
            .map(|cs| cs.iter().map(|c| c.eval(v).map(|([x, y, z], _)| Vec3::new(x, y, z)).ok_or_else(|| fail("the plain loft"))).collect())
            .collect()
    };
    let curves_of = |pts: &[Vec<Vec3>]| -> Vec<CurveN<4>> { lead.iter().zip(pts).map(|(c, p)| with(c, p)).collect() };
    // The plain loft's middle and its direction there.
    let middle = |v: f64| -> Result<(Vec3, Vec3)> {
        let h = 1e-4;
        let d = centroid(&plain((v + h).min(1.0))?) - centroid(&plain((v - h).max(0.0))?);
        let d = d.normalized().ok_or_else(|| KernelError::Invalid("two loft sections sit on top of each other".into()))?;
        Ok((centroid(&plain(v)?), d))
    };
    let start = centroid(ctrl.first().ok_or_else(|| fail("sections"))?);
    let rails: Vec<Rail> = rails.iter().map(|r| rail(r, start, sections, tol)).collect::<Result<_>>()?;
    if let [a, b] = &rails[..]
        && a.anchors.iter().zip(&b.anchors).any(|(p, q)| p.on.dist(q.on) <= tol)
    {
        return Err(KernelError::Invalid("the two guide rails meet a section at the same point".into()));
    }
    // The sections' normals, turned to agree with the first's.
    let mut normals: Vec<Vec3> = Vec::with_capacity(count);
    for s in sections {
        let nn = s.plane.normal();
        let prev = normals.last().copied().unwrap_or(nn);
        normals.push(if nn.dot(prev) < 0.0 { nn * -1.0 } else { nn });
    }
    let spans = count - 1;
    let per = (MAX_WIRES.saturating_sub(count) / spans).clamp(1, MAX_PER_SPAN);
    let mut out = Vec::with_capacity(count + spans * per);
    for i in 0..spans {
        out.push(sections.get(i).map(|s| s.curves.clone()).ok_or_else(|| fail("sections"))?);
        let (Some(&v0), Some(&v1), Some(&n0), Some(&n1)) = (along.get(i), along.get(i + 1), normals.get(i), normals.get(i + 1)) else {
            return Err(fail("sections"));
        };
        let mut cursors: Vec<(usize, f64)> = rails.iter().map(|r| r.anchors.get(i).map_or((0, 0.0), |a| a.at)).collect();
        for q in 1..=per {
            let f = q as f64 / (per + 1) as f64;
            let v = v0 + f * (v1 - v0);
            let station = curves_of(&plain(v)?);
            let (mid, dir) = middle(v)?;
            // The station's plane turns from one section's to the next's.
            let normal = n0.lerp(n1, f).normalized().ok_or_else(|| fail("section planes"))?;
            let mut pairs = Vec::with_capacity(rails.len());
            for (r, cursor) in rails.iter().zip(cursors.iter_mut()) {
                let (Some(a0), Some(a1)) = (r.anchors.get(i), r.anchors.get(i + 1)) else { return Err(fail("rail")) };
                let anchor = wire_point(&station, interpolate_g(a0.g, a1.g, f, n, closed))?;
                // Where the rail crosses the station's plane through the point it guides, nudged so
                // that at each section it is the section's own point.
                let Some((hit, at)) = crate::sweep_path::rail_crossing(&r.pts, anchor, normal, *cursor, tol) else {
                    return Err(KernelError::Invalid(format!(
                        "a guide rail turns back between sections {} and {}: it has to run on from one section to the next",
                        i + 1,
                        i + 2
                    )));
                };
                *cursor = at;
                pairs.push((anchor, hit + a0.nudge * (1.0 - f) + a1.nudge * f));
            }
            let map = station_map(&pairs, mid, dir, fit, size)?;
            out.push(station.iter().map(|c| with(c, &points(c).0.iter().map(|p| map.apply(*p)).collect::<Vec<_>>())).collect());
        }
    }
    out.push(sections.last().map(|s| s.curves.clone()).ok_or_else(|| fail("sections"))?);
    Ok(out)
}

/// The rail `pts` from its end nearer `start` (the first section's middle), and where it meets
/// each section, on the section's curve within `tol`.
///
/// A rail runs from the first section to the last: it meets the last section at its end. In
/// between it meets each section where it first crosses the section's plane after the section
/// before. The end is not a crossing: a rail that lies in the last section's plane (a keel drawn
/// in the bow's sketch plane) crosses that plane all along, first at the far section, and meets the
/// section only where it ends.
fn rail(pts: &[Vec3], start: Vec3, sections: &[Section], tol: f64) -> Result<Rail> {
    let mut pts =
        crate::sweep_path::clean(pts).map_err(|_| KernelError::Invalid("a guide rail needs 2…100000 finite points along its length".into()))?;
    if let (Some(a), Some(b)) = (pts.first(), pts.last())
        && b.dist(start) < a.dist(start)
    {
        pts.reverse();
    }
    let not_reached = |i: usize| {
        KernelError::Invalid(format!(
            "a guide rail does not reach section {}: a rail has to run from the first section to the last, through every one",
            i + 1
        ))
    };
    let mut cursor = (0usize, 0.0f64);
    let mut anchors = Vec::with_capacity(sections.len());
    for (i, s) in sections.iter().enumerate() {
        let (plane, curves) = (&s.plane, &s.curves);
        let (hit, at) = if i + 1 == sections.len() {
            let end = *pts.last().ok_or_else(|| not_reached(i))?;
            if plane.height(end).abs() > tol {
                return Err(not_reached(i));
            }
            (end, (pts.len().saturating_sub(2), 1.0))
        } else {
            crate::sweep_path::rail_crossing(&pts, plane.origin, plane.normal(), cursor, tol).ok_or_else(|| not_reached(i))?
        };
        cursor = at;
        let (g, on, d) = nearest_on(curves, hit).ok_or_else(|| KernelError::Failed("loft rails: section".into()))?;
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

/// The point of a section (its curves) nearest `p`: as a wire parameter, the point, and how far
/// it is. Each curve is searched from its nearest of 256 even samples by golden-section search.
fn nearest_on(section: &[CurveN<4>], p: Vec3) -> Option<(f64, Vec3, f64)> {
    const SAMPLES: usize = 256;
    let mut best: Option<(f64, Vec3, f64)> = None;
    for (k, c) in section.iter().enumerate() {
        let d = |f: f64| point_at(c, f).map_or(f64::INFINITY, |q| q.dist(p));
        let Some(i) = (0..=SAMPLES).min_by(|a, b| d(*a as f64 / SAMPLES as f64).total_cmp(&d(*b as f64 / SAMPLES as f64))) else { continue };
        let (mut lo, mut hi) = ((i as f64 - 1.0).max(0.0) / SAMPLES as f64, (i as f64 + 1.0).min(SAMPLES as f64) / SAMPLES as f64);
        let g = 0.5 * (5f64.sqrt() - 1.0);
        for _ in 0..80 {
            let (a, b) = (hi - g * (hi - lo), lo + g * (hi - lo));
            if d(a) < d(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let f = 0.5 * (lo + hi);
        let q = point_at(c, f)?;
        if best.as_ref().is_none_or(|b| q.dist(p) < b.2) {
            best = Some((k as f64 + f, q, q.dist(p)));
        }
    }
    best
}

/// A wire parameter `f` of the way from `g0` to `g1`; round a closed wire of `n` edges the
/// shorter way.
pub(crate) fn interpolate_g(g0: f64, g1: f64, f: f64, n: usize, closed: bool) -> f64 {
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

/// The point at wire parameter `g` of a section's curves.
fn wire_point(section: &[CurveN<4>], g: f64) -> Result<Vec3> {
    let k = (g.floor().max(0.0) as usize).min(section.len().saturating_sub(1));
    section.get(k).and_then(|c| point_at(c, g - k as f64)).ok_or_else(|| KernelError::Failed("loft rails: section".into()))
}

/// The middle of a section's control points.
fn centroid(ctrl: &[Vec<Vec3>]) -> Vec3 {
    let (sum, n) = ctrl.iter().flatten().fold((Vec3::ZERO, 0usize), |(s, n), p| (s + *p, n + 1));
    sum * (1.0 / n.max(1) as f64)
}

/// The move that puts a station's guided points onto the rails (`pairs`: each point with its
/// rail's point), the station's middle `middle`, the loft's direction there `dir` (unit); `size`
/// the loft's size. One rail: turned about `dir` and sized about the middle, shifted along `dir`;
/// two: carried with the first, turned and sized with the line between them, leaning along `dir`.
pub(crate) fn station_map(pairs: &[(Vec3, Vec3)], middle: Vec3, dir: Vec3, fit: RailFit, size: f64) -> Result<Affine> {
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
