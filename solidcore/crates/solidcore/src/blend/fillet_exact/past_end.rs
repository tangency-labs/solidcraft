//! The certificate behind [`super::ball_past_an_end`]: every ball touching a plane or
//! cylinder face F within F and a natural quadric lies past a plane by more than a margin,
//! by an outward-rounded enclosure of all of those balls' centres.
//!
//! Not upstream and not the fork's (solidcore, a follow-up to #14846).
//!
//! | Rust                              | Upstream |
//! |-----------------------------------|----------|
//! | [`FaceChart`]                     | none     |
//! | [`every_ball_on_face_past_plane`] | none     |
//!
//! Deviations: none (no upstream counterpart). No recursion: the enclosure's subdivision is a
//! worklist bounded by [`BUDGET`] boxes; past it nothing is proven (no refusal).

use core::f64::consts::{FRAC_PI_2, PI, TAU};

use super::{BlendQuadric, QuadricKind};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::tay::wide;
use crate::error::Result;
use crate::math::{Point3, Vec3};
use crate::topology::{edge, face, vertex, Brep, FaceId};

/// The most boxes the subdivision judges before it gives up (nothing proven). The refusals of
/// fuzz body b27's edge 28 close in at most 221 (MEASURED).
const BUDGET: usize = 1 << 16;
/// The cells the parameter range across F is first cut into, each judged on the bound of
/// F's other parameter that the boundary pieces over it give.
const CELLS: u32 = 64;

/// The pieces each knot span of F's boundary edges (or each line or conic) is enclosed in
/// ([`crate::heal::stitch::bounds::curve_cover`]).
const PIECES: u32 = 8;

type Box3 = [Ivl; 3];

/// A boundary piece's chart coordinates: its `s` range (`None`: every angle) and `h` range.
type Piece = (Option<Ivl>, Ivl);

/// F's surface as a chart `(s, h)` of its feet, with the centre of the ball whose foot is
/// there: on a plane `s`, `h` are lengths along two directions in it; on a cylinder `s` is
/// the angle about the axis and `h` the length along it.
#[derive(Debug, Clone, Copy)]
pub(super) enum FaceChart {
    /// The foot `origin + s x + h y`; the centre that moved by `shift` along the unit
    /// normal `n` (the face's own, the ball's signed offset along it).
    Plane {
        /// A point of the plane.
        origin: Point3,
        /// A unit direction in the plane.
        x: Vec3,
        /// The unit direction in the plane square to `x`.
        y: Vec3,
        /// The face's unit normal.
        n: Vec3,
        /// The ball centre's signed offset along `n`.
        shift: f64,
    },
    /// The foot `axis + radius (cos s u + sin s v) + h w`; the centre on the same radial
    /// line at `radius + side shift` from the axis (`side` +1 where the face's normal points
    /// away from the axis).
    Cylinder {
        /// A point of the axis.
        axis: Point3,
        /// A unit direction square to the axis.
        u: Vec3,
        /// `w x u`.
        v: Vec3,
        /// The unit axis.
        w: Vec3,
        /// The cylinder's radius.
        radius: f64,
        /// +1 where the face's normal points away from the axis, -1 toward it.
        side: f64,
        /// The ball centre's signed offset along the face's normal.
        shift: f64,
    },
}

/// A unit vector square to the unit `n`, from the coordinate axis it is least along.
fn square_to(n: Vec3) -> Option<Vec3> {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    let e = if ax <= ay && ax <= az {
        Vec3::new(1.0, 0.0, 0.0)
    } else if ay <= az {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    n.cross(e).unitize().ok()
}

impl FaceChart {
    /// The chart of the plane through `origin` with the face's unit normal `n`.
    pub(super) fn plane(origin: Point3, n: Vec3, shift: f64) -> Option<Self> {
        let x = square_to(n)?;
        let y = n.cross(x);
        Some(Self::Plane {
            origin,
            x,
            y,
            n,
            shift,
        })
    }

    /// The chart of a cylinder quadric, the face's unit normal at its point `at` being `n`.
    pub(super) fn cylinder(q: &BlendQuadric, at: Point3, n: Vec3, shift: f64) -> Option<Self> {
        let w = q.dir;
        let u = square_to(w)?;
        let v = w.cross(u);
        let rel = at - q.point;
        let radial = (rel - w * rel.dot(w)).unitize().ok()?;
        let along = n.dot(radial);
        // the face's normal is radial on a cylinder: anything else is not this cylinder
        if along.abs() < 0.5 || !crate::geom::certified::above(q.radius, 0.0) {
            return None;
        }
        Some(Self::Cylinder {
            axis: q.point,
            u,
            v,
            w,
            radius: q.radius,
            side: if along < 0.0 { -1.0 } else { 1.0 },
            shift,
        })
    }

    /// Whether `s` is an angle (wrapping every `2 pi`).
    fn periodic(&self) -> bool {
        matches!(self, Self::Cylinder { .. })
    }

    /// An enclosure of the chart coordinates `(s, h)` of every point of `bx` (an angle range
    /// for a cylinder, `None` where the box holds the axis and so every angle).
    fn coordinates(&self, bx: Box3) -> Option<(Option<Ivl>, Ivl)> {
        let rel = |o: Point3, d: Vec3| -> Ivl { dot_box(sub_point(bx, o), d) };
        match *self {
            Self::Plane { origin, x, y, .. } => {
                let (s, h) = (rel(origin, x), rel(origin, y));
                (s.is_finite() && h.is_finite()).then_some((Some(pad_rel(s)), pad_rel(h)))
            }
            Self::Cylinder { axis, u, v, w, .. } => {
                let (px, py, h) = (rel(axis, u), rel(axis, v), rel(axis, w));
                if !(px.is_finite() && py.is_finite() && h.is_finite()) {
                    return None;
                }
                if px.lo <= 0.0 && px.hi >= 0.0 && py.lo <= 0.0 && py.hi >= 0.0 {
                    return Some((None, pad_rel(h)));
                }
                // a box clear of the axis is seen from it within less than a half turn,
                // spanned by its corners
                let mid = py.mid().atan2(px.mid());
                let (mut lo, mut hi) = (0.0_f64, 0.0_f64);
                for (cx, cy) in [
                    (px.lo, py.lo),
                    (px.lo, py.hi),
                    (px.hi, py.lo),
                    (px.hi, py.hi),
                ] {
                    let mut d = cy.atan2(cx) - mid;
                    if d > PI {
                        d -= TAU;
                    } else if d < -PI {
                        d += TAU;
                    }
                    lo = lo.min(d);
                    hi = hi.max(d);
                }
                // atan2 is within an ulp or two: a wide margin
                Some((Some(Ivl::new(mid + lo - 1e-9, mid + hi + 1e-9)), pad_rel(h)))
            }
        }
    }

    /// The length the foot or the centre moves at most over `s` (to compare the cell's two
    /// sides when subdividing).
    fn s_length(&self, s: Ivl) -> f64 {
        let w = s.hi - s.lo;
        match *self {
            Self::Plane { .. } => w,
            Self::Cylinder {
                radius,
                side,
                shift,
                ..
            } => w * radius.max((radius + side * shift).abs()),
        }
    }

    /// An enclosure of the centres of the balls whose feet are at `(s, h)` in the box, and
    /// a bound on how far the chart's frame (unit and square to rounding only) can move a
    /// centre from the true one, added to every coordinate.
    fn centres(&self, s: Ivl, h: Ivl) -> Option<Box3> {
        let c = match *self {
            Self::Plane {
                origin,
                x,
                y,
                n,
                shift,
            } => {
                let shift = Ivl::from(shift);
                let at = |o: f64, xi: f64, yi: f64, ni: f64| {
                    Ivl::from(o) + s * Ivl::from(xi) + h * Ivl::from(yi) + Ivl::from(ni) * shift
                };
                [
                    at(origin.x, x.x, y.x, n.x),
                    at(origin.y, x.y, y.y, n.y),
                    at(origin.z, x.z, y.z, n.z),
                ]
            }
            Self::Cylinder {
                axis,
                u,
                v,
                w,
                radius,
                side,
                shift,
            } => {
                let (cs, sn) = cos_sin(s)?;
                let rho = Ivl::from(radius) + Ivl::from(side * shift);
                let at = |o: f64, ui: f64, vi: f64, wi: f64| {
                    Ivl::from(o)
                        + rho * (cs * Ivl::from(ui) + sn * Ivl::from(vi))
                        + h * Ivl::from(wi)
                };
                [
                    at(axis.x, u.x, v.x, w.x),
                    at(axis.y, u.y, v.y, w.y),
                    at(axis.z, u.z, v.z, w.z),
                ]
            }
        };
        // the frame's directions are unit and square to each other to a few roundings: the
        // point it reaches for a foot's coordinates is within 1e-14 of its size of the true one
        let size = s.mag() + h.mag() + 1.0;
        let size = match *self {
            Self::Plane { shift, .. } => size + shift.abs(),
            Self::Cylinder { radius, shift, .. } => size * (radius + shift.abs() + 1.0),
        };
        let frame = Ivl::new(-1e-14 * size, 1e-14 * size);
        Some(c.map(|ci| ci + frame))
    }
}

/// `[lo, hi]` widened by `1e-12` of its magnitude plus `1e-12`: the projections of a box onto
/// a frame unit and square to rounding only.
fn pad_rel(x: Ivl) -> Ivl {
    let e = 1e-12 * (x.mag() + 1.0);
    Ivl::new(x.lo - e, x.hi + e)
}

fn sub_point(bx: Box3, o: Point3) -> Box3 {
    let [x, y, z] = bx;
    [x - Ivl::from(o.x), y - Ivl::from(o.y), z - Ivl::from(o.z)]
}

fn dot_box(bx: Box3, d: Vec3) -> Ivl {
    let [x, y, z] = bx;
    x * Ivl::from(d.x) + y * Ivl::from(d.y) + z * Ivl::from(d.z)
}

/// `x^2` over `x`, never below 0.
fn sqr(x: Ivl) -> Ivl {
    let (lo, hi) = (Ivl::from(x.mig()), Ivl::from(x.mag()));
    Ivl::new((lo * lo).lo.max(0.0), (hi * hi).hi)
}

/// Enclosures of `cos` and `sin` over `s` (a span below a quarter turn within three turns of
/// 0): the hull of their values at its ends and of the extremes it holds, widened past the
/// library's error. `None` elsewhere.
fn cos_sin(s: Ivl) -> Option<(Ivl, Ivl)> {
    if !(s.is_finite() && s.hi - s.lo <= FRAC_PI_2 && s.lo >= -3.0 * PI && s.hi <= 3.0 * PI) {
        return None;
    }
    let (ca, cb, sa, sb) = (s.lo.cos(), s.hi.cos(), s.lo.sin(), s.hi.sin());
    let mut c = wide(ca.min(cb), ca.max(cb));
    let mut n = wide(sa.min(sb), sa.max(sb));
    // cos is +-1 at k pi and sin at pi/2 + k pi: taken where within 1e-9 of the span
    for k in -4_i32..=4 {
        let t = f64::from(k) * PI;
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        if t >= s.lo - 1e-9 && t <= s.hi + 1e-9 {
            c = c.hull(Ivl::from(sign));
        }
        let t = t + FRAC_PI_2;
        if t >= s.lo - 1e-9 && t <= s.hi + 1e-9 {
            n = n.hull(Ivl::from(sign));
        }
    }
    let clamp = |x: Ivl| Ivl::new(x.lo.max(-1.0), x.hi.min(1.0));
    Some((clamp(c), clamp(n)))
}

/// An enclosure of the quadric `q` moved by `dist` along its normal, evaluated at the box
/// `c`: 0 on that surface (both nappes of a cone). `None` for kinds not judged.
fn offset_value(q: &BlendQuadric, dist: f64, c: Box3) -> Option<Ivl> {
    let d = q.dir;
    let delta = |p: Point3| sub_point(c, p);
    let norm2 = |x: Box3| {
        let [a, b, e] = x;
        sqr(a) + sqr(b) + sqr(e)
    };
    let dd = Ivl::from(d.x) * Ivl::from(d.x)
        + Ivl::from(d.y) * Ivl::from(d.y)
        + Ivl::from(d.z) * Ivl::from(d.z);
    Some(match q.kind {
        QuadricKind::Plane => dot_box(delta(q.point), d) - Ivl::from(dist) * dd.sqrt(),
        QuadricKind::Sphere => {
            let rho = Ivl::from(q.radius) + Ivl::from(q.side * dist);
            norm2(delta(q.point)) - sqr(rho)
        }
        QuadricKind::Cylinder => {
            let rho = Ivl::from(q.radius) + Ivl::from(q.side * dist);
            let x = delta(q.point);
            let along = dot_box(x, d) / dd;
            let [a, b, e] = x;
            let perp = [
                a - Ivl::from(d.x) * along,
                b - Ivl::from(d.y) * along,
                e - Ivl::from(d.z) * along,
            ];
            norm2(perp) - sqr(rho)
        }
        QuadricKind::Cone => {
            // the normal away from the axis is cos(a) radial - sin(a) d: the apex moves back
            // along d by side dist / sin(a) (`BlendQuadric::offset`)
            let (sa, ca) = (q.angle.sin(), q.angle.cos());
            let (sa, ca) = (wide(sa, sa), wide(ca, ca));
            let back = Ivl::from(q.side * dist) / sa;
            let [a, b, e] = delta(q.point);
            let x = [
                a + Ivl::from(d.x) * back,
                b + Ivl::from(d.y) * back,
                e + Ivl::from(d.z) * back,
            ];
            sqr(ca) * norm2(x) * dd - sqr(dot_box(x, d))
        }
        QuadricKind::None
        | QuadricKind::Line
        | QuadricKind::Point
        | QuadricKind::Torus
        | QuadricKind::Circle => return None,
    })
}

/// F's boundary pieces: for each edge of `f`'s loops, the chart coordinates enclosing every
/// point within the edge's tolerance (and its vertices') of each box covering its curve. `None` where an edge
/// has no curve or one with no enclosure.
fn boundary_pieces(b: &Brep, f: FaceId, chart: &FaceChart, tol: f64) -> Result<Option<Vec<Piece>>> {
    let mut pieces = Vec::new();
    for e in face::edges(b, f, None, false, None)? {
        let Some(curve) = edge::curve(b, e)? else {
            return Ok(None);
        };
        let mut pad = tol.max(edge::tolerance(b, e)?);
        let (v0, v1) = edge::start_end_vertices(b, e)?;
        for v in [v0, v1].into_iter().flatten() {
            pad = pad.max(vertex::tolerance(b, v)?);
        }
        if !pad.is_finite() {
            return Ok(None);
        }
        let grow = Ivl::new(-pad, pad);
        let ivl = edge::interval(b, e)?;
        let Ok(cover) =
            crate::heal::stitch::bounds::curve_cover(curve, ivl.min(), ivl.max(), PIECES)
        else {
            return Ok(None);
        };
        for bx in cover {
            let Some(piece) = chart.coordinates(bx.map(|x| x + grow)) else {
                return Ok(None);
            };
            pieces.push(piece);
        }
    }
    Ok((!pieces.is_empty()).then_some(pieces))
}

/// Whether the angle range `a` (or every angle, `None`) meets `s` for some turn.
fn meets(periodic: bool, a: Option<Ivl>, s: Ivl) -> bool {
    let Some(a) = a else {
        return true;
    };
    let turns: &[f64] = if periodic {
        &[-2.0, -1.0, 0.0, 1.0, 2.0]
    } else {
        &[0.0]
    };
    turns
        .iter()
        .any(|&k| a.lo + k * TAU <= s.hi && s.lo <= a.hi + k * TAU)
}

/// Whether every ball touching F (the face `f`, charted by `chart`) at a foot within F, and
/// touching the quadric `other` moved by `other_dist` along its normal, has its centre `c`
/// with `(c - at) . dir < -margin`: PROVEN when `true` (`false`: not proven, whatever the
/// cause).
///
/// The feet within F are bounded by F's boundary: on a plane or a cylinder every line `s`
/// constant (a line of the plane, a ruling of the cylinder) that meets F meets it in a set
/// whose ends lie on F's boundary, so the `s` that F covers, and at each the range of `h`, lie
/// within those of the boundary's points there. Each boundary edge's curve is enclosed by
/// outward-rounded interval arithmetic and grown by its tolerance
/// ([`crate::heal::stitch::bounds::curve_enclosure`]); its chart coordinates enclose the
/// face's boundary points it stands for. The range of `s` is cut into [`CELLS`] cells, each
/// given the hull of the `h` ranges of the pieces that meet it, and each box `(s, h)` is
/// judged by interval arithmetic on the centres of every foot in it: no ball is there where
/// the moved quadric's value excludes 0, and every ball there lies past where the plane's
/// signed distance is below `-margin`. A box neither clears is halved along its longer side,
/// up to [`BUDGET`] boxes.
///
/// The quadrics are the faces as the kernel reads them ([`BlendQuadric::from_surface`]); the
/// moved quadric holds every ball centre touching that surface on the given side.
pub(super) fn every_ball_on_face_past_plane(
    b: &Brep,
    f: FaceId,
    chart: &FaceChart,
    other: &BlendQuadric,
    other_dist: f64,
    plane: (Point3, Vec3),
    margin: f64,
    tol: f64,
) -> Result<bool> {
    let (at, dir) = plane;
    if margin.is_nan() || margin < 0.0 {
        return Ok(false);
    }
    let Some(pieces) = boundary_pieces(b, f, chart, tol)? else {
        return Ok(false);
    };
    let periodic = chart.periodic();
    // the range of s the cells cut
    let range = if periodic {
        match pieces.iter().find_map(|(s, _)| *s) {
            Some(s) if pieces.iter().all(|(s, _)| s.is_some()) => {
                Ivl::new(s.mid() - PI, s.mid() + PI)
            }
            _ => Ivl::new(-PI, PI),
        }
    } else {
        let mut range: Option<Ivl> = None;
        for s in pieces.iter().filter_map(|(s, _)| *s) {
            range = Some(range.map_or(s, |r| r.hull(s)));
        }
        let Some(range) = range else {
            return Ok(false);
        };
        range
    };
    let mut work: Vec<(Ivl, Ivl)> = Vec::new();
    let width = range.hi - range.lo;
    for i in 0..CELLS {
        let lo = range.lo + width * f64::from(i) / f64::from(CELLS);
        let hi = if i + 1 == CELLS {
            range.hi
        } else {
            range.lo + width * f64::from(i + 1) / f64::from(CELLS)
        };
        let s = Ivl::new(lo, hi);
        let mut h: Option<Ivl> = None;
        for (ps, ph) in &pieces {
            if meets(periodic, *ps, s) {
                h = Some(h.map_or(*ph, |x| x.hull(*ph)));
            }
        }
        if let Some(h) = h {
            work.push((s, h));
        }
    }
    let mut judged = 0_usize;
    while let Some((s, h)) = work.pop() {
        judged += 1;
        if judged > BUDGET {
            return Ok(false);
        }
        let Some(c) = chart.centres(s, h) else {
            return Ok(false);
        };
        let Some(q) = offset_value(other, other_dist, c) else {
            return Ok(false);
        };
        if q.lo > 0.0 || q.hi < 0.0 {
            continue;
        }
        if dot_box(sub_point(c, at), dir).hi < -margin {
            continue;
        }
        let (ls, lh) = (chart.s_length(s), h.hi - h.lo);
        let split = |x: Ivl| {
            let m = x.mid();
            (m > x.lo && m < x.hi).then_some((Ivl::new(x.lo, m), Ivl::new(m, x.hi)))
        };
        if ls >= lh {
            let Some((a, z)) = split(s) else {
                return Ok(false);
            };
            work.push((a, h));
            work.push((z, h));
        } else {
            let Some((a, z)) = split(h) else {
                return Ok(false);
            };
            work.push((s, a));
            work.push((s, z));
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The enclosures of cos and sin hold the values at 10,001 points of spans across the
    /// extremes and the turns.
    #[test]
    fn cos_sin_holds_every_value_in_the_span() {
        for (lo, hi) in [
            (-0.3, 0.4),
            (1.2, 2.0),
            (3.0, 3.3),
            (-1.7, -1.4),
            (4.6, 4.8),
            (-6.4, -6.2),
        ] {
            let (c, s) = cos_sin(Ivl::new(lo, hi)).unwrap();
            for i in 0..=10_000 {
                let t = lo + (hi - lo) * f64::from(i) / 10_000.0;
                assert!(c.lo <= t.cos() && t.cos() <= c.hi, "cos {t}");
                assert!(s.lo <= t.sin() && t.sin() <= s.hi, "sin {t}");
            }
        }
        assert!(cos_sin(Ivl::new(0.0, 2.0)).is_none());
    }

    /// A box clear of the axis has the angle range its corners span, across the turn too.
    #[test]
    fn a_box_off_the_axis_is_seen_within_its_corners_angles() {
        let chart = FaceChart::Cylinder {
            axis: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
            w: Vec3::new(0.0, 0.0, 1.0),
            radius: 1.0,
            side: 1.0,
            shift: 0.5,
        };
        // straddling the negative x axis, where atan2 jumps from pi to -pi
        let bx = [
            Ivl::new(-2.0, -1.0),
            Ivl::new(-0.5, 0.5),
            Ivl::new(3.0, 4.0),
        ];
        let (s, h) = chart.coordinates(bx).unwrap();
        let s = s.unwrap();
        for t in [PI - 0.46, PI, PI + 0.46] {
            assert!(meets(true, Some(s), Ivl::new(t, t)), "{t} in {s:?}");
        }
        assert!(!meets(true, Some(s), Ivl::new(0.0, 0.0)));
        assert!(h.lo <= 3.0 && h.hi >= 4.0);
        let around = [Ivl::new(-1.0, 1.0), Ivl::new(-1.0, 1.0), Ivl::new(0.0, 1.0)];
        assert_eq!(chart.coordinates(around).unwrap().0, None);
    }

    /// The moved quadrics vanish on the centres they stand for: a sphere moved out, a
    /// cylinder moved in, a plane and a cone moved by their normal.
    #[test]
    fn the_moved_quadrics_vanish_on_their_points() {
        let point = |p: Point3| [Ivl::from(p.x), Ivl::from(p.y), Ivl::from(p.z)];
        let holds = |q: &BlendQuadric, dist: f64, p: Point3| {
            let v = offset_value(q, dist, point(p)).unwrap();
            v.lo <= 1e-12 && v.hi >= -1e-12
        };
        let sphere = BlendQuadric {
            kind: QuadricKind::Sphere,
            point: Point3::new(2.5, 50.0, 0.0),
            radius: 10.0,
            side: -1.0,
            ..BlendQuadric::default()
        };
        // a concave pocket (normal toward its centre) moved out by 2: radius 12
        assert!(holds(&sphere, -2.0, Point3::new(2.5, 38.0, 0.0)));
        assert!(!holds(&sphere, -2.0, Point3::new(2.5, 40.0, 0.0)));
        let cylinder = BlendQuadric {
            kind: QuadricKind::Cylinder,
            point: Point3::new(3.0, 0.0, 4.5),
            dir: Vec3::new(0.0, 1.0, 0.0),
            radius: 3.0,
            side: 1.0,
            ..BlendQuadric::default()
        };
        assert!(holds(&cylinder, -1.0, Point3::new(3.0, 7.0, 6.5)));
        let plane = BlendQuadric {
            kind: QuadricKind::Plane,
            point: Point3::new(0.0, 0.0, 7.5),
            dir: Vec3::new(0.0, 0.0, 1.0),
            ..BlendQuadric::default()
        };
        assert!(holds(&plane, -2.0, Point3::new(4.0, 1.0, 5.5)));
        let angle = 0.5_f64;
        let cone = BlendQuadric {
            kind: QuadricKind::Cone,
            point: Point3::new(0.0, 0.0, 0.0),
            dir: Vec3::new(0.0, 0.0, 1.0),
            angle,
            side: 1.0,
            ..BlendQuadric::default()
        };
        // the point at height 2 on the cone, moved out by 0.25 along its normal
        let (r, z) = (2.0 * angle.tan(), 2.0);
        let moved = Point3::new(r + 0.25 * angle.cos(), 0.0, z - 0.25 * angle.sin());
        assert!(holds(&cone, 0.25, moved));
    }
}
