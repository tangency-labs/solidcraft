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
use super::{below_resolution, keeps, Continuity, ExtendRefusal, Local};
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
        let Some((grown, across, capped, bound)) = extend_side(&cur, side, c, r, z, cap, distance)?
        else {
            *done = SideExtension::Unchanged;
            continue;
        };
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
        source: 0.0,
        extension,
    }))
}

/// The box of `s`'s knots (its B-spline form's ends).
pub(super) fn knot_box(s: &BSplineSurface) -> Result<Extent2d> {
    let net = s
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
    let (u, v) = (net.u_knots(), net.v_knots());
    match (u.first(), u.last(), v.first(), v.last()) {
        (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) => Ok(Extent2d::from_bounds(u0, v0, u1, v1)),
        _ => Err(SolidError::surface_failed(
            OP,
            "the B-spline surface has an empty knot vector",
        )),
    }
}

/// The source's iso across `side` at `middle` (a point of the side): its point at the
/// opposite side and its point halfway, which with the side's own point orient the iso's
/// circle. The extension's own points cannot: at the least step they lie within about
/// `EFF_ZERO_PARAM` of each other, so their chords are nearly parallel and the sense of
/// their turn is rounding (a sense flipped by it measures the arc the long way round).
pub(super) fn source_iso(
    cur: &BSplineSurface,
    dom: &Extent2d,
    side: BoxSide,
    middle: Point2,
) -> Result<(Vec3, Vec3)> {
    let (lo, hi) = (dom.min(), dom.max());
    let (far, half) = match side {
        BoxSide::UMin | BoxSide::UMax => {
            let far = if side == BoxSide::UMin { hi.x } else { lo.x };
            (
                Point2::new(far, middle.y),
                Point2::new(0.5 * (lo.x + hi.x), middle.y),
            )
        }
        BoxSide::VMin | BoxSide::VMax => {
            let far = if side == BoxSide::VMin { hi.y } else { lo.y };
            (
                Point2::new(middle.x, far),
                Point2::new(middle.x, 0.5 * (lo.y + hi.y)),
            )
        }
    };
    Ok((cur.evaluate_point(far)?, cur.evaluate_point(half)?))
}

/// What one ask's extension measures: its length across the side and its turn round the
/// pole's axis.
#[derive(Debug, Clone, Copy)]
struct Measure {
    length: f64,
    rounding: f64,
    turn: f64,
}

/// `cur` continued Natural past `side` by `distance` across it, capped by `cap`: the
/// result, its across length, whether the cap stopped it, and its construction bound;
/// `None` where the length asked (the distance, or the length cap below it) is below the
/// side's resolution ([`below_resolution`]).
///
/// The world-frame continuation is tried first and kept where it certifies, so a result
/// that worked before does not move. Only where its position-sized bound or rounding
/// cannot certify the ask is the local-frame continuation used.
fn extend_side(
    cur: &BSplineSurface,
    side: BoxSide,
    c: Vec3,
    r: f64,
    z: Vec3,
    cap: Cap,
    distance: f64,
) -> Result<Option<(BSplineSurface, Across, bool, f64)>> {
    match extend_side_world(cur, side, c, r, z, cap, distance) {
        Err(SolidError::ExtendRefused {
            refusal: ExtendRefusal::NotExact,
            ..
        }) => extend_side_local(cur, side, c, r, z, cap, distance),
        other => other,
    }
}

/// The world-frame continuation: the source's own net in world coordinates. Its rounding is
/// sized on the part's position, so a far ask that cannot be certified falls to
/// [`extend_side_local`].
fn extend_side_world(
    cur: &BSplineSurface,
    side: BoxSide,
    c: Vec3,
    r: f64,
    z: Vec3,
    cap: Cap,
    distance: f64,
) -> Result<Option<(BSplineSurface, Across, bool, f64)>> {
    let net = cur
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
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
        let out = g.surface(side)?;
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
    // the iso's circle, its plane through the source's own points (`source_iso`), its
    // normal turning from the source's far end through the side, outward
    let (x_far, x_half) = source_iso(cur, &dom, side, middle)?;
    let x_side = cur.evaluate_point(middle)?;
    let n = {
        let n = (x_half - x_far).cross(x_side - x_half);
        let len = n.length();
        if len.is_nan() || len <= 0.0 {
            return Err(SolidError::surface_failed(
                OP,
                "the side's normal has no direction",
            ));
        }
        n * (1.0 / len)
    };
    let measure = |s: &BSplineSurface| -> Result<Measure> {
        let x0 = s.evaluate_point(at(s, 0.0))? - c;
        let x1 = s.evaluate_point(at(s, 1.0))? - c;
        let cc = n * n.dot(x0);
        let (u0, u1) = (x0 - cc, x1 - cc);
        let mut phi = u0.cross(u1).dot(n).atan2(u0.dot(u1));
        if phi < 0.0 {
            phi += 2.0 * PI;
        }
        let (t0, t1) = (x0 - z * x0.dot(z), x1 - z * x1.dot(z));
        Ok(Measure {
            length: u0.length() * phi,
            rounding: 0.0,
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
    // the length across an extension whose construction bound is `bound`, with the closed
    // form's rounding (the evaluations at the points' size, the cross products, the atan2)
    // and the points' construction bound through the angle
    let size = c.length() + r;
    let across_of = |s: &BSplineSurface, bound: f64| -> Result<Across> {
        let length = measure(s)?.length;
        let rounding = gamma(64) * (length + size) + 4.0 * bound;
        Ok(Across {
            enclosure: Bracket {
                lower: (length - rounding).max(0.0),
                upper: length + rounding,
            },
            truncation: 0.0,
            rounding,
            converged: true,
            // a closed form at the points' size: its rounding part is sized on the part
            resolution: 0.0,
        })
    };
    let range = source.range()?;
    let least = source.least_step()?;
    let asked = match cap {
        Cap::Arc(arc) => distance.min(r * arc),
        Cap::Round { length, .. } => distance.min(length),
    };
    if let Some((g, bound)) = made(least)? {
        if below_resolution(Some(&across_of(&g, bound)?), bound, asked) {
            return Ok(None);
        }
    }
    // the extension parameter, the length it was solved to (the distance or its cap; none
    // where the turn's cap sets it) and whether the cap holds it short of the distance
    let (e, goal, capped) = match cap {
        Cap::Arc(arc) => {
            let target = distance.min(r * arc);
            (
                reach(&|e| value(e, false), target, range, least)?,
                Some(target),
                target < distance,
            )
        }
        Cap::Round { length, turn } => {
            // within the length cap and the turn's (none where the turn never gets there)
            let at_turn = match solve(&|e| value(e, true), turn, range, least)? {
                Reach::At(e_turn) => Some((
                    e_turn,
                    value(e_turn, false)?.ok_or_else(|| {
                        SolidError::surface_failed(
                            OP,
                            "the extension at the turn could not be made",
                        )
                    })?,
                )),
                Reach::Short(_) => None,
            };
            let target = distance.min(length);
            match at_turn {
                Some((e_turn, l_turn)) if l_turn <= target => (e_turn, None, true),
                _ => (
                    reach(&|e| value(e, false), target, range, least)?,
                    Some(target),
                    target < distance,
                ),
            }
        }
    };
    let (grown, bound) = made(e)?.ok_or_else(|| {
        SolidError::surface_failed(
            OP,
            "the extension at the reached distance could not be made",
        )
    })?;
    let across = across_of(&grown, bound)?;
    // a length solved for is met, or the extension is refused naming a length that works
    if goal.is_some_and(|goal| !solved_length_met(&across, bound, goal, e <= least)) {
        return Err(refused(
            ExtendRefusal::NotExact,
            super::workable(Some(&across), bound),
        ));
    }
    certify_circle(&grown, side, middle, c, size)?;
    Ok(Some((grown, across, capped, bound)))
}

fn extend_side_local(
    cur: &BSplineSurface,
    side: BoxSide,
    c: Vec3,
    r: f64,
    z: Vec3,
    cap: Cap,
    distance: f64,
) -> Result<Option<(BSplineSurface, Across, bool, f64)>> {
    let net = cur
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
    let source = Local::of(net, side)?;
    let dom = knot_box(cur)?;
    let middle = side.point(&dom, 0.5);
    let made = |e: f64| -> Result<Option<super::local_iso::Made>> {
        let Some(g) = super::local_iso::natural(&source, side, c, e)? else {
            return Ok(None);
        };
        if !keeps(net, g.world.gw_nurb().ok_or_else(super::op_err)?, side)? {
            return Err(SolidError::port_invariant(
                OP,
                "the extension moved the source's net",
            ));
        }
        Ok(Some(g))
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
    // Work about the sphere centre when finding the source iso's plane, too: its
    // normal must not inherit absolute-coordinate evaluation errors.
    let (lo, hi) = (dom.min(), dom.max());
    let far_uv = if side.is_u() {
        Point2::new(if side.is_high() { lo.x } else { hi.x }, middle.y)
    } else {
        Point2::new(middle.x, if side.is_high() { lo.y } else { hi.y })
    };
    let half_uv = Point2::new(0.5 * (lo.x + hi.x), 0.5 * (lo.y + hi.y));
    let (x_far, ef) = super::measure::point(cur, far_uv, c)?;
    let (x_half, eh) = super::measure::point(cur, half_uv, c)?;
    let (x_side, es) = super::measure::point(cur, middle, c)?;
    let (a, b) = (x_half - x_far, x_side - x_half);
    let raw = a.cross(b);
    let norm = raw.length();
    // Higham §§3.1–3.4: `gamma_16` covers the two differences, six cross-product
    // products and three differences, squares/sums/sqrt, and bound arithmetic.
    let ea = ef + eh + gamma(2) * (x_half.length() + x_far.length());
    let eb = es + eh + gamma(2) * (x_side.length() + x_half.length());
    let en = (a.length() * eb + b.length() * ea + ea * eb + gamma(16) * a.length() * b.length())
        * (1.0 + gamma(8));
    if norm.partial_cmp(&en) != Some(std::cmp::Ordering::Greater) {
        return Err(refused(ExtendRefusal::NotExact, None));
    }
    let n = raw * (1.0 / norm);
    let nerr = (2.0 * en / (norm - en) + gamma(8)) * (1.0 + gamma(8));
    let measure = |g: &super::local_iso::Made| -> Result<Measure> {
        let s = &g.local;
        let (x0, e0) = super::measure::point(s, at(s, 0.0), Vec3::ZERO)?;
        let (x1, e1) = super::measure::point(s, at(s, 1.0), Vec3::ZERO)?;
        let cc = n * n.dot(x0);
        let (u0, u1) = (x0 - cc, x1 - cc);
        let radius = u0.length();
        // The projection's sensitivity to its normal and point, plus its dot,
        // product, difference and norm's `gamma_16` (Higham §§3.1–3.4).
        let er = (2.0 * e0
            + (2.0 * nerr + nerr * nerr) * x0.length()
            + gamma(16) * (x0.length() + radius))
            * (1.0 + gamma(8));
        let (chord, chord_err) = super::measure::chord(s, at(s, 0.0), at(s, 1.0))?;
        let bound = g.bound;
        let eu = (er + e0 + e1 + 4.0 * bound) * (1.0 + gamma(8));
        let sense = super::measure::turn_sign(u0, u1, n, eu, nerr);
        // Endpoint perturbations move the chord and projected radius. Retain
        // 4*bound conservatively, propagating it through asin rather than adding
        // a fixed length error near a diameter.
        let (length, rounding) = super::measure::oriented_arc(
            (chord, chord_err + 4.0 * bound),
            (radius, er + 4.0 * bound),
            sense,
        )?;
        let (t0, t1) = (x0 - z * x0.dot(z), x1 - z * x1.dot(z));
        Ok(Measure {
            length,
            rounding,
            turn: angle(t0, t1),
        })
    };
    // The probes that steer the solve for an extension parameter are plain evaluations in
    // the local frame: they may be asked far past what is certified, and nothing is accepted
    // from them. The extension finally built is measured by `measure`.
    let probe = |s: &BSplineSurface| -> Result<Measure> {
        let x0 = s.evaluate_point(at(s, 0.0))?;
        let x1 = s.evaluate_point(at(s, 1.0))?;
        let cc = n * n.dot(x0);
        let (u0, u1) = (x0 - cc, x1 - cc);
        let mut phi = u0.cross(u1).dot(n).atan2(u0.dot(u1));
        if phi < 0.0 {
            phi += 2.0 * PI;
        }
        let (t0, t1) = (x0 - z * x0.dot(z), x1 - z * x1.dot(z));
        Ok(Measure {
            length: u0.length() * phi,
            rounding: 0.0,
            turn: angle(t0, t1),
        })
    };
    let value = |e: f64, turn: bool| -> Result<Option<f64>> {
        Ok(match made(e)? {
            Some(g) => {
                let m = probe(&g.local)?;
                Some(if turn { m.turn } else { m.length })
            }
            None => None,
        })
    };
    let across_of = |g: &super::local_iso::Made| -> Result<Across> {
        let m = measure(g)?;
        Ok(super::measure::across((m.length, m.rounding)))
    };
    let range = source.range()?;
    let least = source.least_step()?;
    let asked = match cap {
        Cap::Arc(arc) => distance.min(r * arc),
        Cap::Round { length, .. } => distance.min(length),
    };
    if let Some(g) = made(least)? {
        if below_resolution(Some(&across_of(&g)?), g.bound, asked) {
            return Ok(None);
        }
    }
    // the extension parameter, the length it was solved to (the distance or its cap; none
    // where the turn's cap sets it) and whether the cap holds it short of the distance
    let (e, goal, capped) = match cap {
        Cap::Arc(arc) => {
            let target = distance.min(r * arc);
            (
                reach(&|e| value(e, false), target, range, least)?,
                Some(target),
                target < distance,
            )
        }
        Cap::Round { length, turn } => {
            // within the length cap and the turn's (none where the turn never gets there)
            let at_turn = match solve(&|e| value(e, true), turn, range, least)? {
                Reach::At(e_turn) => Some((
                    e_turn,
                    value(e_turn, false)?.ok_or_else(|| {
                        SolidError::surface_failed(
                            OP,
                            "the extension at the turn could not be made",
                        )
                    })?,
                )),
                Reach::Short(_) => None,
            };
            let target = distance.min(length);
            match at_turn {
                Some((e_turn, l_turn)) if l_turn <= target => (e_turn, None, true),
                _ => (
                    reach(&|e| value(e, false), target, range, least)?,
                    Some(target),
                    target < distance,
                ),
            }
        }
    };
    let g = made(e)?.ok_or_else(|| {
        SolidError::surface_failed(
            OP,
            "the extension at the reached distance could not be made",
        )
    })?;
    let across = across_of(&g)?;
    let bound = g.bound;
    let grown = g.world;
    // a length solved for is met, or the extension is refused naming a length that works
    if goal.is_some_and(|goal| !solved_length_met(&across, bound, goal, e <= least)) {
        return Err(refused(
            ExtendRefusal::NotExact,
            super::workable(Some(&across), bound),
        ));
    }
    // The certificate is sized on the sphere's own radius, not the centre's position.
    certify_circle_local(&grown, side, middle, c)?;
    Ok(Some((grown, across, capped, bound)))
}

/// Whether the across length `across` of an extension whose construction bound is `bound`
/// meets the length `goal` it was solved to: the measure exact at it (converged, its rounding
/// within the owner's `1e-6` of it) and its middle within the bisection's stop of it, at or
/// past it at the least step (`least_step`), which can be no shorter. A bisection that ran
/// out before meeting it does not.
pub(super) fn solved_length_met(across: &Across, bound: f64, goal: f64, least_step: bool) -> bool {
    let off = across.enclosure.mid() - goal;
    super::exact(across, bound, goal)
        && if least_step {
            off >= -MEET * goal
        } else {
            off.abs() <= MEET * goal
        }
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
/// every positive ask is refused naming the longest value found. No ask is below `least`,
/// the shortest extension the side's knot can carry ([`Local::least_step`]): a
/// target its value is proved past is below the side's resolution and asked of no solve
/// ([`below_resolution`], the callers' check); a shorter target is met by it.
///
/// [`Local::least_step`]: super::Local::least_step
pub(super) fn solve(
    f: &dyn Fn(f64) -> Result<Option<f64>>,
    target: f64,
    e0: f64,
    least: f64,
) -> Result<Reach> {
    let (mut lo, mut lo_v) = (0.0_f64, 0.0_f64);
    let mut hi = None;
    let mut bad = None;
    let mut e = e0.max(least);
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
                let m = (0.5 * (lo + b)).max(least);
                if !(m > lo && m < b) {
                    break;
                }
                match f(m)? {
                    Some(v) if v >= target => {
                        return Ok(Reach::At(bisect(f, target, lo, m, least)?))
                    }
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
    Ok(Reach::At(bisect(f, target, lo, hi, least)?))
}

/// The parameter where `f` reaches `target` (no ask below `least`, [`solve`]), or refused
/// as not exact naming the longest value reached.
pub(super) fn reach(
    f: &dyn Fn(f64) -> Result<Option<f64>>,
    target: f64,
    e0: f64,
    least: f64,
) -> Result<f64> {
    match solve(f, target, e0, least)? {
        Reach::At(e) => Ok(e),
        Reach::Short(v) => Err(refused(ExtendRefusal::NotExact, (v > 0.0).then_some(v))),
    }
}

/// Bisection of `[lo, hi]` (`f(lo) < target <= f(hi)`) until `f` is within `MEET` of the
/// target, asking nothing below `least` (`least` itself when the target is below its value).
fn bisect(
    f: &dyn Fn(f64) -> Result<Option<f64>>,
    target: f64,
    lo: f64,
    hi: f64,
    least: f64,
) -> Result<f64> {
    let (mut lo, mut hi) = (lo, hi);
    for _ in 0..BISECTIONS {
        let m = (0.5 * (lo + hi)).max(least);
        if !(m > lo && m < hi) {
            break;
        }
        let v = f(m)?.ok_or_else(|| {
            SolidError::surface_failed(OP, "the extension could not be measured inside the bracket")
        })?;
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
    let net = s
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
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
        return Err(SolidError::surface_failed(OP, "the iso has no rows"));
    };
    let (Some(&m0), Some(&mm), Some(&ml)) = (mags.first(), mags.get(n / 2), mags.last()) else {
        return Err(SolidError::surface_failed(OP, "the iso has no rows"));
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

/// The local-frame certificate: the extraction's world-sized input budget is kept
/// separate from the local determinant's arithmetic, so a far-away, perfectly spread
/// plane is not read as rank-deficient.
fn certify_circle_local(s: &BSplineSurface, side: BoxSide, middle: Point2, c: Vec3) -> Result<()> {
    let net = s
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
    // the iso along the side's across direction: a u-curve across a u side
    let (t, dir) = if side.is_u() {
        (middle.y, Direction::U)
    } else {
        (middle.x, Direction::V)
    };
    let iso = extract_iso_crv(net, t, dir)?;
    // Extraction happens BEFORE translation and therefore still has a
    // world-sized input budget. Keep it separate from the local determinant's
    // arithmetic; using world-sized row magnitudes also for its rank test would
    // incorrectly declare a far-away, perfectly spread plane rank-deficient.
    let degree = if side.is_u() { net.q() } else { net.p() };
    let mut input = 0.0_f64;
    for p in net.cpts().iter().flatten() {
        let w = if p.w == NOW { 1.0 } else { p.w };
        input = input.max(
            p.x.abs().max(p.y.abs()).max(p.z.abs())
                + (1.0 + c.x.abs().max(c.y.abs()).max(c.z.abs())) * w.abs(),
        );
    }
    // Convex de Boor extraction: <=12q+4 operations on a path; the extra
    // four cover input/translation and evaluating this bound (Higham §3).
    let error = gamma(12 * degree + 8) * input * (1.0 + gamma(8));
    let mut rows: Vec<[f64; 4]> = Vec::new();
    let mut mags: Vec<f64> = Vec::new();
    for p in iso.cpts() {
        let w = if p.w == NOW { 1.0 } else { p.w };
        // One fused rounding per local coordinate, already covered by the
        // determinant's `gamma_32` below; never round C w separately at |C|.
        let local = [
            (-c.x).mul_add(w, p.x),
            (-c.y).mul_add(w, p.y),
            (-c.z).mul_add(w, p.z),
            w,
        ];
        mags.push(local.iter().fold(0.0_f64, |m, x| m.max(x.abs())));
        rows.push(local);
    }
    let n = rows.len();
    let (Some(&r0), Some(&rm), Some(&rl)) = (rows.first(), rows.get(n / 2), rows.last()) else {
        return Err(SolidError::surface_failed(OP, "the iso has no rows"));
    };
    let (Some(&m0), Some(&mm), Some(&ml)) = (mags.first(), mags.get(n / 2), mags.last()) else {
        return Err(SolidError::surface_failed(OP, "the iso has no rows"));
    };
    let g = |row: [f64; 4], x: usize| row.get(x).copied().unwrap_or(0.0);
    // the 3 x 3 minor of the three spread rows on columns i, j, l
    let minor = |i: usize, j: usize, l: usize| -> f64 {
        g(r0, i) * (g(rm, j) * g(rl, l) - g(rm, l) * g(rl, j))
            - g(r0, j) * (g(rm, i) * g(rl, l) - g(rm, l) * g(rl, i))
            + g(r0, l) * (g(rm, i) * g(rl, j) - g(rm, j) * g(rl, i))
    };
    let three = m0 * mm * ml;
    // Product perturbation without subtracting nearly equal products. These
    // three terms contain all seven nonempty subsets of perturbed factors.
    let perturb =
        ((m0 + error) * (mm + error) * error + (m0 + error) * error * ml + error * mm * ml)
            * (1.0 + gamma(12));
    // the three span a plane: a minor well past arithmetic AND input rounding
    let spans = [(0, 1, 2), (0, 1, 3), (0, 2, 3), (1, 2, 3)]
        .iter()
        .any(|&(i, j, l)| minor(i, j, l).abs() > 1e3 * 6.0 * (gamma(16) * three + perturb));
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
        let tolerance = 24.0
            * (gamma(32) * three * mag + perturb * (mag + error) + three * error)
            * (1.0 + gamma(12));
        if det.abs() > tolerance {
            return Err(refused(ExtendRefusal::NotExact, None));
        }
    }
    Ok(())
}
