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
use crate::error::Result;
use crate::geom::certified::{gamma, next_up};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::topology::{edge, face, vertex, Brep, FaceId};

const OP: &str = "blend::fillet_exact::past_end";

/// The most boxes the subdivision judges before it gives up (nothing proven). The refusals of
/// fuzz body b27's edge 28 close in at most 221 (MEASURED).
const BUDGET: usize = 1 << 16;
/// The boxes judged between polls of the kernel's cancellation.
const POLL: usize = 256;
/// The cells the parameter range across F is first cut into, each judged on the bound of
/// F's other parameter that the boundary pieces over it give.
const CELLS: u32 = 64;

/// The pieces each knot span of F's boundary edges (or each line or conic) is enclosed in
/// ([`crate::heal::stitch::bounds::curve_cover`]).
const PIECES: u32 = 8;

type Box3 = [Ivl; 3];

/// A boundary piece's chart coordinates: its `s` range (`None`: every angle) and `h` range.
type Piece = (Option<Ivl>, Ivl);

/// A vector enclosed coordinate by coordinate.
type IVec = [Ivl; 3];

fn ivec(v: Vec3) -> IVec {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}

fn idot(a: IVec, b: IVec) -> Ivl {
    let ([a0, a1, a2], [b0, b1, b2]) = (a, b);
    a0 * b0 + a1 * b1 + a2 * b2
}

fn icross(a: IVec, b: IVec) -> IVec {
    let ([a0, a1, a2], [b0, b1, b2]) = (a, b);
    [a1 * b2 - a2 * b1, a2 * b0 - a0 * b2, a0 * b1 - a1 * b0]
}

/// An enclosure of `a / |a|` for every vector `a` in the enclosure; `None` where it may hold
/// the zero vector.
fn iunit(a: IVec) -> Option<IVec> {
    let length = idot(a, a).sqrt();
    if !(length.is_finite() && length.lo > 0.0) {
        return None;
    }
    Some(a.map(|x| x / length))
}

/// The exact unit frame `(x, y, n / |n|)` of the float vector `n`: `x` the unit vector along
/// `n x e`, `e` the coordinate axis `n` is least along (its coordinates are `n`'s, exactly),
/// and `y = (n / |n|) x x`, each enclosed.
fn unit_frame(n: Vec3) -> Option<[IVec; 3]> {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    // n x e_x = (0, z, -y), n x e_y = (-z, 0, x), n x e_z = (y, -x, 0): no rounding
    let across = if ax <= ay && ax <= az {
        Vec3::new(0.0, n.z, -n.y)
    } else if ay <= az {
        Vec3::new(-n.z, 0.0, n.x)
    } else {
        Vec3::new(n.y, -n.x, 0.0)
    };
    let z = iunit(ivec(n))?;
    let x = iunit(ivec(across))?;
    Some([x, icross(z, x), z])
}

/// `2pi`, enclosed (`TAU` is the double below it).
fn two_pi() -> Ivl {
    Ivl::new(TAU, next_up(TAU))
}

/// `pi`, enclosed (`PI` is the double below it).
fn pi() -> Ivl {
    Ivl::new(PI, next_up(PI))
}

/// The value `v` of `cos`, `sin` or `atan2`, enclosed: the platform's libm returns them
/// within one ulp of the true value (the convention of [`crate::geom::certified`]: two
/// roundings), and one ulp of a normal `v` is at most `2u |v|` = `gamma(2) |v|` to first
/// order; `f64::MIN_POSITIVE` covers a subnormal's ulp.
fn libm(v: f64) -> Ivl {
    let e = gamma(2) * v.abs() + f64::MIN_POSITIVE;
    Ivl::checked(v - e, v + e)
}

/// F's surface as a chart `(s, h)` of its feet, with the centre of the ball whose foot is
/// there: on a plane `s`, `h` are lengths along two directions in it; on a cylinder `s` is
/// the angle about the axis and `h` the length along it. The chart's frame is the exact unit
/// frame of the quadric's direction, enclosed ([`unit_frame`]), so the chart is the quadric
/// itself, with no allowance for a frame that is unit or square only to rounding.
#[derive(Debug, Clone, Copy)]
pub(super) enum FaceChart {
    /// The foot `origin + s x + h y`; the centre that moved by `shift` along the unit
    /// normal `n` (the face's own, the ball's signed offset along it).
    Plane {
        /// A point of the plane.
        origin: Point3,
        /// A unit direction in the plane.
        x: IVec,
        /// The unit direction in the plane square to `x`.
        y: IVec,
        /// The face's unit normal.
        n: IVec,
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
        u: IVec,
        /// `w x u`.
        v: IVec,
        /// The unit axis.
        w: IVec,
        /// The cylinder's radius.
        radius: f64,
        /// +1 where the face's normal points away from the axis, -1 toward it.
        side: f64,
        /// The ball centre's signed offset along the face's normal.
        shift: f64,
    },
}

impl FaceChart {
    /// The chart of a plane quadric (its point and normal), the normal turned to the face's
    /// own normal `n` (the side the ball's offset is signed by).
    pub(super) fn plane(q: &BlendQuadric, n: Vec3, shift: f64) -> Option<Self> {
        let dir = if q.dir.dot(n) < 0.0 { -q.dir } else { q.dir };
        let [x, y, n] = unit_frame(dir)?;
        Some(Self::Plane {
            origin: q.point,
            x,
            y,
            n,
            shift,
        })
    }

    /// The chart of a cylinder quadric, the face's unit normal at its point `at` being `n`.
    pub(super) fn cylinder(q: &BlendQuadric, at: Point3, n: Vec3, shift: f64) -> Option<Self> {
        let [u, v, w] = unit_frame(q.dir)?;
        let rel = at - q.point;
        let radial = (rel - q.dir * rel.dot(q.dir)).unitize().ok()?;
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
    /// for a cylinder, `None` for every angle: where the box holds the axis or comes near a
    /// half turn round it).
    fn coordinates(&self, bx: Box3) -> Option<Piece> {
        match *self {
            Self::Plane { origin, x, y, .. } => {
                let rel = sub_point(bx, origin);
                let (s, h) = (idot(rel, x), idot(rel, y));
                (s.is_finite() && h.is_finite()).then_some((Some(s), h))
            }
            Self::Cylinder { axis, u, v, w, .. } => {
                let rel = sub_point(bx, axis);
                let (px, py, h) = (idot(rel, u), idot(rel, v), idot(rel, w));
                if !(px.is_finite() && py.is_finite() && h.is_finite()) {
                    return None;
                }
                if px.lo <= 0.0 && px.hi >= 0.0 && py.lo <= 0.0 && py.hi >= 0.0 {
                    return Some((None, h));
                }
                // a box clear of the axis is seen from it within less than a half turn,
                // spanned by its corners; each corner's angle is taken within half a turn of
                // the box's middle, which is decided only well inside that half turn
                let mid = py.mid().atan2(px.mid());
                let mut arc: Option<Ivl> = None;
                for (cx, cy) in [
                    (px.lo, py.lo),
                    (px.lo, py.hi),
                    (px.hi, py.lo),
                    (px.hi, py.hi),
                ] {
                    let mut d = libm(cy.atan2(cx)) - Ivl::from(mid);
                    if d.mid() > PI {
                        d = d - two_pi();
                    } else if d.mid() < -PI {
                        d = d + two_pi();
                    }
                    if !(d.is_finite() && d.lo > 0.1 - PI && d.hi < PI - 0.1) {
                        return Some((None, h));
                    }
                    arc = Some(arc.map_or(d, |a| a.hull(d)));
                }
                Some((arc.map(|a| Ivl::from(mid) + a), h))
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

    /// An enclosure of the centres of the balls whose feet are at `(s, h)` in the box.
    fn centres(&self, s: Ivl, h: Ivl) -> Option<Box3> {
        let point = |o: Point3| ivec(o);
        Some(match *self {
            Self::Plane {
                origin,
                x,
                y,
                n,
                shift,
            } => {
                let shift = Ivl::from(shift);
                let o = point(origin);
                core::array::from_fn(|i| {
                    let at = |a: IVec| a.get(i).copied().unwrap_or(Ivl::whole());
                    at(o) + s * at(x) + h * at(y) + at(n) * shift
                })
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
                let o = point(axis);
                core::array::from_fn(|i| {
                    let at = |a: IVec| a.get(i).copied().unwrap_or(Ivl::whole());
                    at(o) + rho * (cs * at(u) + sn * at(v)) + h * at(w)
                })
            }
        })
    }
}

fn sub_point(bx: Box3, o: Point3) -> Box3 {
    let [x, y, z] = bx;
    [x - Ivl::from(o.x), y - Ivl::from(o.y), z - Ivl::from(o.z)]
}

fn dot_box(bx: Box3, d: Vec3) -> Ivl {
    idot(bx, ivec(d))
}

/// `x^2` over `x`, never below 0.
fn sqr(x: Ivl) -> Ivl {
    let (lo, hi) = (Ivl::from(x.mig()), Ivl::from(x.mag()));
    Ivl::new((lo * lo).lo.max(0.0), (hi * hi).hi)
}

/// Enclosures of `cos` and `sin` over `s` (a span below a quarter turn within three turns of
/// 0): the hull of their values at its ends ([`libm`]) and of the extremes it holds, `+-1` at
/// each `k pi` (cos) and `pi/2 + k pi` (sin) whose enclosure meets the span. `None`
/// elsewhere.
fn cos_sin(s: Ivl) -> Option<(Ivl, Ivl)> {
    if !(s.is_finite() && s.hi - s.lo <= FRAC_PI_2 && s.lo >= -9.0 && s.hi <= 9.0) {
        return None;
    }
    let mut c = libm(s.lo.cos()).hull(libm(s.hi.cos()));
    let mut n = libm(s.lo.sin()).hull(libm(s.hi.sin()));
    let half = pi() * Ivl::from(0.5);
    for k in -4_i32..=4 {
        let t = Ivl::from(f64::from(k)) * pi();
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        if t.hi >= s.lo && t.lo <= s.hi {
            c = c.hull(Ivl::from(sign));
        }
        let t = t + half;
        if t.hi >= s.lo && t.lo <= s.hi {
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
            let (sa, ca) = (libm(q.angle.sin()), libm(q.angle.cos()));
            let back = Ivl::from(q.side * dist) / (sa * dd.sqrt());
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
    turns.iter().any(|&k| {
        let shifted = a + Ivl::from(k) * two_pi();
        shifted.lo <= s.hi && s.lo <= shifted.hi
    })
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
/// outward-rounded boxes and grown by its tolerance
/// ([`crate::heal::stitch::bounds::curve_cover`]); their chart coordinates enclose the
/// face's boundary points they stand for. The range of `s` is cut into [`CELLS`] cells, each
/// given the hull of the `h` ranges of the pieces that meet it, and each box `(s, h)` is
/// judged by interval arithmetic on the centres of every foot in it: no ball is there where
/// the moved quadric's value excludes 0, and every ball there lies past where the plane's
/// signed distance is below `-margin`. A box neither clears is halved along its longer side,
/// up to [`BUDGET`] boxes.
///
/// The quadrics are the faces as the kernel reads them ([`BlendQuadric::from_surface`]); the
/// moved quadric holds every ball centre touching that surface on the given side.
///
/// # Errors
///
/// Those of the topology lookups; [`crate::SolidError::Cancelled`] when the kernel's
/// operation is cancelled (polled every [`POLL`] boxes and every cell).
pub(super) fn every_ball_on_face_past_plane(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    chart: &FaceChart,
    other: &BlendQuadric,
    other_dist: f64,
    plane: (Point3, Vec3),
    margin: f64,
    tol: f64,
) -> Result<bool> {
    let Some(pieces) = boundary_pieces(b, f, chart, tol)? else {
        return Ok(false);
    };
    every_ball_over_pieces(k, &pieces, chart, other, other_dist, plane, margin)
}

/// [`every_ball_on_face_past_plane`] over F's boundary pieces `pieces`.
fn every_ball_over_pieces(
    k: &Kernel,
    pieces: &[Piece],
    chart: &FaceChart,
    other: &BlendQuadric,
    other_dist: f64,
    plane: (Point3, Vec3),
    margin: f64,
) -> Result<bool> {
    let (at, dir) = plane;
    if margin.is_nan() || margin < 0.0 || pieces.is_empty() {
        return Ok(false);
    }
    let periodic = chart.periodic();
    // the range of s the cells cut: on a cylinder a little over a whole turn
    let range = if periodic {
        match pieces.iter().find_map(|(s, _)| *s) {
            Some(s) if pieces.iter().all(|(s, _)| s.is_some()) => {
                Ivl::new(s.mid() - 3.15, s.mid() + 3.15)
            }
            _ => Ivl::new(-3.15, 3.15),
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
        k.check_cancelled(OP)?;
        let mut h: Option<Ivl> = None;
        for (ps, ph) in pieces {
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
        if judged % POLL == 0 {
            k.check_cancelled(OP)?;
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
    /// extremes and the turns (sampled: a check of the construction, not its proof).
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

    fn axis_frame() -> FaceChart {
        let q = BlendQuadric {
            kind: QuadricKind::Cylinder,
            point: Point3::new(0.0, 0.0, 0.0),
            dir: Vec3::new(0.0, 0.0, 1.0),
            radius: 1.0,
            side: 1.0,
            ..BlendQuadric::default()
        };
        FaceChart::cylinder(
            &q,
            Point3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            0.5,
        )
        .unwrap()
    }

    /// The angle about the axis of the world point `(x, y)` in the chart's own frame.
    fn chart_angle(chart: &FaceChart, x: f64, y: f64) -> f64 {
        let FaceChart::Cylinder { u, v, .. } = chart else {
            panic!("not a cylinder")
        };
        let p = ivec(Vec3::new(x, y, 0.0));
        idot(p, *v).mid().atan2(idot(p, *u).mid())
    }

    /// A box clear of the axis has the angle range its corners span, across the turn too.
    #[test]
    fn a_box_off_the_axis_is_seen_within_its_corners_angles() {
        let chart = axis_frame();
        // a box whose angles about the axis run either side of the chart's own half turn
        let back = chart_angle(&chart, -1.0, 0.0);
        let (bc, bs) = (back.cos(), back.sin());
        let bx = [
            Ivl::new(1.5 * bc - 0.5, 1.5 * bc + 0.5),
            Ivl::new(1.5 * bs - 0.5, 1.5 * bs + 0.5),
            Ivl::new(3.0, 4.0),
        ];
        let (s, h) = chart.coordinates(bx).unwrap();
        let s = s.unwrap();
        for (x, y) in [
            (bx[0].lo, bx[1].lo),
            (bx[0].lo, bx[1].hi),
            (bx[0].hi, bx[1].lo),
            (bx[0].hi, bx[1].hi),
        ] {
            let t = chart_angle(&chart, x, y);
            assert!(meets(true, Some(s), Ivl::new(t, t)), "{t} in {s:?}");
        }
        let front = chart_angle(&chart, 1.0, 0.0);
        assert!(!meets(true, Some(s), Ivl::new(front, front)));
        assert!(h.lo <= 3.0 && h.hi >= 4.0);
        let around = [Ivl::new(-1.0, 1.0), Ivl::new(-1.0, 1.0), Ivl::new(0.0, 1.0)];
        assert_eq!(chart.coordinates(around).unwrap().0, None);
    }

    /// The certificate on a unit square of the plane z = 0, the balls of radius 0.5 above it
    /// touching the plane x = 0.3: their centres are (0.3, y, 0.5), y in [0, 1]. Against the
    /// plane through (0, -d, 0) facing -y, the nearest lies -d from it: proven past a margin
    /// of 1e-3 for d = 0.01; not for d = 0.0009 (the ball at y = 0 lies within the margin, a
    /// near miss) nor for d = -0.5 (the plane cuts the square).
    #[test]
    fn the_certificate_proves_only_what_holds_past_the_margin() {
        let k = Kernel::default();
        let plane = BlendQuadric {
            kind: QuadricKind::Plane,
            point: Point3::new(0.0, 0.0, 0.0),
            dir: Vec3::new(0.0, 0.0, 1.0),
            ..BlendQuadric::default()
        };
        let chart = FaceChart::plane(&plane, Vec3::new(0.0, 0.0, 1.0), 0.5).unwrap();
        let edge = |x: [f64; 2], y: [f64; 2]| {
            chart
                .coordinates([Ivl::new(x[0], x[1]), Ivl::new(y[0], y[1]), Ivl::from(0.0)])
                .unwrap()
        };
        let pieces = [
            edge([0.0, 1.0], [0.0, 0.0]),
            edge([1.0, 1.0], [0.0, 1.0]),
            edge([0.0, 1.0], [1.0, 1.0]),
            edge([0.0, 0.0], [0.0, 1.0]),
        ];
        let other = BlendQuadric {
            kind: QuadricKind::Plane,
            point: Point3::new(0.3, 0.0, 0.0),
            dir: Vec3::new(1.0, 0.0, 0.0),
            ..BlendQuadric::default()
        };
        let past = |d: f64| {
            let at = (Point3::new(0.0, -d, 0.0), Vec3::new(0.0, -1.0, 0.0));
            every_ball_over_pieces(&k, &pieces, &chart, &other, 0.0, at, 1e-3).unwrap()
        };
        assert!(past(0.01));
        assert!(!past(0.0009));
        assert!(!past(-0.5));
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
