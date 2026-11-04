//! A ball patch extended as itself ([`Surface::extended_sides`] on a [`BallPatch`]): its
//! own net continued Natural, still on its sphere.
//!
//! Not upstream: upstream extends a ball patch's net as any B-spline's (the base class),
//! which leaves the sphere. Here (owner, 2026-11-04) the patch's OWN net is continued
//! Natural past each side (the homogeneous polynomial of its end span carried on), so the
//! source's net and knots stay bit for bit (a face keeps its UV trims on it: the shell's
//! extended offset replaces the face's surface with UV trim creation off) and every point
//! stays on the sphere: the identity `|H - C w|^2 = R^2 w^2` that puts the net on the
//! sphere is a polynomial identity, so it holds on its continuation. The result is
//! certified on the sphere on its net ([`lies_on`]), within its rounding, and is the ball
//! patch on the same sphere.
//!
//! The patch is the sphere's triangle with one side collapsed to its pole `P` (proved: the
//! side's certified diameter's lower bound is 0), the two sides through the pole great
//! circles (meridians about the axis `C -> P`) and the third side a great circle's arc
//! across from the pole. The pole's side is collapsed (left). Each other side asked is
//! extended `distance` across it, capped (said so, [`SideExtension::UpToLimit`]):
//! - **round the pole** (a side through the pole), no farther than the sphere's radius
//!   times a quarter of the turn the patch leaves open round its axis (the shell's rule,
//!   WR-532 / WR-728: its gaps close on the rounds' ends there), and never past half that
//!   turn in longitude, so the two sides never meet;
//! - **away from the pole** (the third side), no farther than half the arc left from the
//!   third side's farthest point to the antipodal pole, where the meridians meet again
//!   (WR-728).
//!
//! **The distance** across a side is measured at the side's middle on the iso across it,
//! in closed form: that iso lies on the sphere and (certified on its homogeneous control
//! points: every 4 x 4 determinant of three of them and a fourth zero within its rounding)
//! in a plane, so it is a circle, and its length from the side to the end is the circle's
//! radius times the angle swept (away from the pole the isos are meridians: the sphere's
//! radius times the angle along the great circle). The angle is the one from the side's
//! point through the extension's middle point to its end: the arc the iso runs. The Ball's
//! patch is the inverse stereographic image of a rational quadratic arc's sector (blend
//! corner `ball_triangle_patch`), so its isos run their circles monotonically, and Natural
//! carries on the same rational map. The extension parameter is solved for the distance
//! (or the cap) by bisection on that closed form, met to `1e-12` of it.
//!
//! **Weights.** An extension whose weights are not all positive is refused
//! ([`ExtendRefusal::WeightVanishes`]) naming the longest distance found whose weights are.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::sides::{ExtendedSides, SideExtension};
use super::{keeps, Continuity, ExtendRefusal, Local};
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::{gamma, NOW};
use crate::nurbs::srf_basic::Direction;
use crate::nurbs::srf_tool::p1::extract_iso_crv;
use crate::surface::ball_patch::BallPatch;
use crate::surface::implicit::{lies_on, Implicit};
use crate::surface::side::{Across, BoxSide, Bracket};
use crate::surface::surface::Surface;
use std::f64::consts::PI;

const OP: &str = "surface::extend::ball::ball_extended_sides";

/// Doublings of the extension parameter that bracket the distance.
const BRACKET_TRIES: usize = 60;
/// Halvings of the bracket: the length met to `MEET` of it, or the bracket a few ulps wide.
const BISECTIONS: usize = 200;
/// The relative accuracy the closed-form length is met to.
const MEET: f64 = 1e-12;

/// The angle between `a` and `b`, in radians.
fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b))
}

fn refused(refusal: ExtendRefusal, distance: Option<f64>) -> SolidError {
    SolidError::ExtendRefused {
        operation: OP,
        refusal,
        distance,
    }
}

/// How far a side may go: round the pole by a length across it (the sphere's radius times
/// a quarter of the open turn) and no more than an angle of longitude (half the open
/// turn: the two sides never meet), or away from it by an arc along the meridians
/// (radians).
#[derive(Debug, Clone, Copy)]
enum Cap {
    Round { length: f64, turn: f64 },
    Arc(f64),
}

/// `patch` (the surface `s`) extended past the sides `asked` of its natural box by
/// `distance` as itself (module docs); `None` for a patch with no collapsed side (not the
/// sphere's triangle: extended as its net is).
pub(super) fn ball_extended_sides(
    k: &Kernel,
    s: &Surface,
    patch: &BallPatch,
    asked: &[BoxSide],
    distance: f64,
) -> Result<Option<ExtendedSides>> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the distance is not finite and positive",
        ));
    }
    // the net's knot box: the extension is past the net's ends, whatever box the patch keeps
    let dom = knot_box(patch.net())?;
    let mut pole_side = None;
    for side in BoxSide::ALL {
        if s.side_diameter(k, &dom, side)?.lower <= 0.0 {
            pole_side = Some(side);
            break;
        }
    }
    let Some(pole_side) = pole_side else {
        return Ok(None);
    };
    let away = match pole_side {
        BoxSide::UMin => BoxSide::UMax,
        BoxSide::UMax => BoxSide::UMin,
        BoxSide::VMin => BoxSide::VMax,
        BoxSide::VMax => BoxSide::VMin,
    };
    let net = patch.net();
    let (c, r) = (patch.centre(), patch.radius());
    let p = net.evaluate_point(pole_side.point(&dom, 0.5))? - c;
    let a = net.evaluate_point(away.point(&dom, 0.0))? - c;
    let b = net.evaluate_point(away.point(&dom, 1.0))? - c;
    let z = p * (1.0 / p.length());
    let (ax, bx) = (a - z * a.dot(z), b - z * b.dot(z));
    if !(ax.length() > 0.0 && bx.length() > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the third side's end is at the pole or its antipode",
        ));
    }
    // the turn round the pole the patch spans, and the third side's farthest point from it
    let lambda = angle(ax, bx);
    let m = a.cross(b);
    let foot = m * m.dot(p) - p * m.dot(m);
    let mut theta = angle(p, a).max(angle(p, b));
    if foot.length() > 0.0 && a.cross(foot).dot(m) >= 0.0 && foot.cross(b).dot(m) >= 0.0 {
        theta = theta.max(angle(p, foot));
    }
    if !(lambda > 0.0 && lambda < PI && theta > 0.0 && theta < PI) {
        return Err(SolidError::invalid_input(
            OP,
            "the patch is not a triangle under a half turn of its sphere",
        ));
    }
    let mut cur = net.copy();
    let mut sides = [SideExtension::NotAsked; 4];
    let mut extension = 0.0_f64;
    for (&side, done) in BoxSide::ALL.iter().zip(sides.iter_mut()) {
        if !asked.contains(&side) {
            continue;
        }
        if side == pole_side {
            *done = SideExtension::Collapsed;
            continue;
        }
        let cap = if side == away {
            Cap::Arc(0.5 * (PI - theta))
        } else {
            Cap::Round {
                length: r * 0.25 * (2.0 * PI - lambda),
                turn: 0.5 * (2.0 * PI - lambda),
            }
        };
        let (grown, across, capped, bound) = extend_side(&cur, side, c, r, z, cap, distance)?;
        cur = grown;
        extension = extension.max(bound);
        *done = if capped {
            SideExtension::UpToLimit(across)
        } else {
            SideExtension::Extended(across)
        };
    }
    // every point on the sphere, certified on the net
    let on = Implicit::Sphere {
        centre: c,
        radius: r,
    };
    if !lies_on(&cur, &on)? {
        return Err(refused(ExtendRefusal::NotExact, None));
    }
    Ok(Some(ExtendedSides {
        surface: Surface::from(BallPatch::create(&cur, c, r)?),
        sides,
        extension,
    }))
}

/// The box of `s`'s knots (its B-spline form's ends).
pub(super) fn knot_box(s: &BSplineSurface) -> Result<Extent2d> {
    let net = s.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let (u, v) = (net.u_knots(), net.v_knots());
    match (u.first(), u.last(), v.first(), v.last()) {
        (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) => Ok(Extent2d::from_bounds(u0, v0, u1, v1)),
        _ => Err(SolidError::sm_err(OP)),
    }
}

/// What one ask's extension measures: its length across the side and its turn round the
/// pole's axis.
#[derive(Debug, Clone, Copy)]
struct Measure {
    length: f64,
    turn: f64,
}

/// `cur` continued Natural past `side` by `distance` across it, capped by `cap`: the
/// result, its across length, whether the cap stopped it, and its construction bound.
fn extend_side(
    cur: &BSplineSurface,
    side: BoxSide,
    c: Vec3,
    r: f64,
    z: Vec3,
    cap: Cap,
    distance: f64,
) -> Result<(BSplineSurface, Across, bool, f64)> {
    let net = cur.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let source = Local::of(net, side)?;
    let dom = knot_box(cur)?;
    let middle = side.point(&dom, 0.5);
    let made = |e: f64| -> Result<Option<(BSplineSurface, f64)>> {
        let Some(g) = source.extended(Continuity::Natural, e)? else {
            return Ok(None);
        };
        if !g.is_finite() {
            return Ok(None);
        }
        let bound = g.bound();
        let out = g.surface(net, side)?;
        if !keeps(net, &out, side)? {
            return Err(SolidError::port_invariant(
                OP,
                "the extension moved the source's net",
            ));
        }
        Ok(Some((BSplineSurface::from_gw_nurb(&out), bound)))
    };
    // the iso across the side at its middle, from the side (0) to the extension's end (1)
    let from_end = side.at(&dom);
    let at = |s: &BSplineSurface, f: f64| -> Point2 {
        let end = knot_box(s).map_or(from_end, |d| side.at(&d));
        let from = side.at(&dom);
        let x = from + f * (end - from);
        if side.is_u() {
            Point2::new(x, middle.y)
        } else {
            Point2::new(middle.x, x)
        }
    };
    let measure = |s: &BSplineSurface| -> Result<Measure> {
        let x0 = s.evaluate_point(at(s, 0.0))? - c;
        let xm = s.evaluate_point(at(s, 0.5))? - c;
        let x1 = s.evaluate_point(at(s, 1.0))? - c;
        let n = (xm - x0).cross(x1 - xm);
        let len = n.length();
        if len.is_nan() || len <= 0.0 {
            return Err(SolidError::sm_err(OP));
        }
        let n = n * (1.0 / len);
        let cc = n * n.dot(x0);
        let (u0, u1) = (x0 - cc, x1 - cc);
        let mut phi = u0.cross(u1).dot(n).atan2(u0.dot(u1));
        if phi < 0.0 {
            phi += 2.0 * PI;
        }
        let (t0, t1) = (x0 - z * x0.dot(z), x1 - z * x1.dot(z));
        Ok(Measure {
            length: u0.length() * phi,
            turn: angle(t0, t1),
        })
    };
    let value = |e: f64, turn: bool| -> Result<Option<f64>> {
        Ok(match made(e)? {
            Some((s, _)) => {
                let m = measure(&s)?;
                Some(if turn { m.turn } else { m.length })
            }
            None => None,
        })
    };
    let range = source.range()?;
    let (e, capped) = match cap {
        Cap::Arc(arc) => {
            let target = distance.min(r * arc);
            (
                reach(&|e| value(e, false), target, range)?,
                target < distance,
            )
        }
        Cap::Round { length, turn } => {
            // within the length cap and the turn's (none where the turn never gets there)
            let at_turn = match solve(&|e| value(e, true), turn, range)? {
                Reach::At(e_turn) => Some((
                    e_turn,
                    value(e_turn, false)?.ok_or_else(|| SolidError::sm_err(OP))?,
                )),
                Reach::Short(_) => None,
            };
            let target = distance.min(length);
            match at_turn {
                Some((e_turn, l_turn)) if l_turn <= target => (e_turn, true),
                _ => (
                    reach(&|e| value(e, false), target, range)?,
                    target < distance,
                ),
            }
        }
    };
    let (grown, bound) = made(e)?.ok_or_else(|| SolidError::sm_err(OP))?;
    let length = measure(&grown)?.length;
    // the closed form's rounding (the evaluations at the points' size, the cross products,
    // the atan2), and the points' construction bound through the angle
    let size = c.length() + r;
    let rounding = gamma(64) * (length + size) + 4.0 * bound;
    certify_circle(&grown, side, middle, c, size)?;
    let across = Across {
        enclosure: Bracket {
            lower: (length - rounding).max(0.0),
            upper: length + rounding,
        },
        truncation: 0.0,
        rounding,
        converged: true,
    };
    Ok((grown, across, capped, bound))
}

/// Where `f` reaches its target ([`solve`]).
pub(super) enum Reach {
    /// At this extension parameter.
    At(f64),
    /// Never: `f` stops growing (its curve's continuation tends to a point) short of the
    /// target, at this value.
    Short(f64),
}

/// The extension parameter where `f` (increasing from 0, `None` where a weight is not
/// positive) reaches `target`: bracketed by doubling from `e0`, then bisected. A target past
/// every positive ask is refused naming the longest value found.
pub(super) fn solve(f: &dyn Fn(f64) -> Result<Option<f64>>, target: f64, e0: f64) -> Result<Reach> {
    let (mut lo, mut lo_v) = (0.0_f64, 0.0_f64);
    let mut hi = None;
    let mut bad = None;
    let mut e = e0;
    for _ in 0..BRACKET_TRIES {
        match f(e)? {
            Some(v) if v >= target => {
                hi = Some(e);
                break;
            }
            // no longer growing: the continuation tends to its limit point
            Some(v) if lo > 0.0 && v - lo_v <= MEET * v => return Ok(Reach::Short(v)),
            Some(v) => {
                lo = e;
                lo_v = v;
                e *= 2.0;
            }
            None => {
                bad = Some(e);
                break;
            }
        }
    }
    let hi = match (hi, bad) {
        (Some(h), _) => h,
        (None, Some(mut b)) => {
            // the longest positive ask below the first whose weights are not
            for _ in 0..BISECTIONS {
                let m = 0.5 * (lo + b);
                if !(m > lo && m < b) {
                    break;
                }
                match f(m)? {
                    Some(v) if v >= target => return Ok(Reach::At(bisect(f, target, lo, m)?)),
                    Some(v) => {
                        lo = m;
                        lo_v = v;
                    }
                    None => b = m,
                }
            }
            return Err(refused(
                ExtendRefusal::WeightVanishes,
                (lo_v > 0.0).then_some(lo_v),
            ));
        }
        (None, None) => return Ok(Reach::Short(lo_v)),
    };
    Ok(Reach::At(bisect(f, target, lo, hi)?))
}

/// The parameter where `f` reaches `target`, or refused as not exact naming the longest
/// value reached.
pub(super) fn reach(f: &dyn Fn(f64) -> Result<Option<f64>>, target: f64, e0: f64) -> Result<f64> {
    match solve(f, target, e0)? {
        Reach::At(e) => Ok(e),
        Reach::Short(v) => Err(refused(ExtendRefusal::NotExact, (v > 0.0).then_some(v))),
    }
}

/// Bisection of `[lo, hi]` (`f(lo) < target <= f(hi)`) until `f` is within `MEET` of the
/// target.
fn bisect(f: &dyn Fn(f64) -> Result<Option<f64>>, target: f64, lo: f64, hi: f64) -> Result<f64> {
    let (mut lo, mut hi) = (lo, hi);
    for _ in 0..BISECTIONS {
        let m = 0.5 * (lo + hi);
        if !(m > lo && m < hi) {
            break;
        }
        let v = f(m)?.ok_or_else(|| SolidError::sm_err(OP))?;
        if (v - target).abs() <= MEET * target {
            return Ok(m);
        }
        if v < target {
            lo = m;
        } else {
            hi = m;
        }
    }
    Ok(hi)
}

/// Whether the iso across `side` of `s` at the side's middle `middle` lies in a plane:
/// every determinant of three spread homogeneous control points (from the centre `c`) and
/// a fourth is zero within its rounding (`size` the points' absolute size), and the three
/// span a plane. Refused as not exact otherwise.
fn certify_circle(
    s: &BSplineSurface,
    side: BoxSide,
    middle: Point2,
    c: Vec3,
    size: f64,
) -> Result<()> {
    let net = s.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    // the iso along the side's across direction: a u-curve across a u side
    let (t, dir) = if side.is_u() {
        (middle.y, Direction::U)
    } else {
        (middle.x, Direction::V)
    };
    let iso = extract_iso_crv(net, t, dir)?;
    let mut rows: Vec<[f64; 4]> = Vec::new();
    let mut mags: Vec<f64> = Vec::new();
    for p in iso.cpts() {
        let w = if p.w == NOW { 1.0 } else { p.w };
        rows.push([p.x - c.x * w, p.y - c.y * w, p.z - c.z * w, w]);
        mags.push(p.x.abs().max(p.y.abs()).max(p.z.abs()) + (size + 1.0) * w.abs());
    }
    let n = rows.len();
    let (Some(&r0), Some(&rm), Some(&rl)) = (rows.first(), rows.get(n / 2), rows.last()) else {
        return Err(SolidError::sm_err(OP));
    };
    let (Some(&m0), Some(&mm), Some(&ml)) = (mags.first(), mags.get(n / 2), mags.last()) else {
        return Err(SolidError::sm_err(OP));
    };
    let g = |row: [f64; 4], x: usize| row.get(x).copied().unwrap_or(0.0);
    // the 3 x 3 minor of the three spread rows on columns i, j, l
    let minor = |i: usize, j: usize, l: usize| -> f64 {
        g(r0, i) * (g(rm, j) * g(rl, l) - g(rm, l) * g(rl, j))
            - g(r0, j) * (g(rm, i) * g(rl, l) - g(rm, l) * g(rl, i))
            + g(r0, l) * (g(rm, i) * g(rl, j) - g(rm, j) * g(rl, i))
    };
    let three = m0 * mm * ml;
    // the three span a plane: a minor well past its rounding
    let spans = [(0, 1, 2), (0, 1, 3), (0, 2, 3), (1, 2, 3)]
        .iter()
        .any(|&(i, j, l)| minor(i, j, l).abs() > 1e3 * 6.0 * gamma(16) * three);
    if !spans {
        return Err(refused(ExtendRefusal::NotExact, None));
    }
    let (m123, m023, m013, m012) = (
        minor(1, 2, 3),
        minor(0, 2, 3),
        minor(0, 1, 3),
        minor(0, 1, 2),
    );
    for (&row, &mag) in rows.iter().zip(&mags) {
        // expansion along the fourth row; |fl(det) - det| <= gamma_K per(|A|) <= gamma_K 4!
        // times the product of the rows' sizes
        let det = -g(row, 0) * m123 + g(row, 1) * m023 - g(row, 2) * m013 + g(row, 3) * m012;
        if det.abs() > gamma(32) * 24.0 * three * mag {
            return Err(refused(ExtendRefusal::NotExact, None));
        }
    }
    Ok(())
}
