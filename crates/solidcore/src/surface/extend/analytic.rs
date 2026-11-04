//! A plane, cone, sphere or torus extended past the sides of its box by its own NURBS net
//! continued Natural ([`Surface::extended_sides`] on an analytic kind), still on its analytic
//! surface.
//!
//! Not upstream: upstream extends an analytic's NURB as any B-spline's. Here (owner,
//! 2026-11-04, the ball patch's rule) the analytic's OWN net is continued Natural past each
//! side (the homogeneous polynomial of its end span carried on), so the source's net, knots
//! and parameters stay bit for bit (a straight direction's end points to rounding, below): a face keeps its UV trims on the result (the shell's
//! extended offset replaces the face's surface with UV trim creation off). Widening the
//! analytic itself ([`Surface::continued`]) rebuilds its canonical form over the wider STEP
//! box, which moves every kept UV trim. The implicit equation composed with the net is a
//! polynomial identity, so it holds on its continuation: the result is certified on the
//! analytic on its net ([`lies_on`]), within its rounding. It is a B-spline, carrying the
//! source's kept exact round where it has one (#2487); an offset surface of an analytic is
//! the offset of its base so extended.
//!
//! A plane follows the same rule for a caller keeping its trims (owner, 2026-11-04;
//! [`Surface::extended_sides_keeping_parameters`]): its bilinear net continued, a B-spline
//! whose parameters over the source's box are the plane's own bit for bit, where widening the
//! plane ([`Surface::continued`]) handed back a plane kind the shell's offset takes other
//! branches for. Its form is straight both ways, so every continuity is its continuation.
//!
//! **Straight directions.** Across a side where the form is of degree 1 (a plane either way,
//! a cone along its rulings) the continuation is the end span's own line, so its joint knot
//! is removed exactly and the span is widened (`joint_removed`): the result is again one
//! straight span there, as the kind's canonical form is and its recognisers read it (a cone's
//! form must have its two end circles only), on the same parameters. The source's end control
//! points are then the widened span's points at the old ends, to their rounding (the plane's
//! corners, a ruling's ends); its other points, its knots and its parameters stay bit for
//! bit.
//!
//! A side is left where the analytic closes in its direction (a full turn), where the side
//! is not proved longer than a point (a pole or an apex), or a plane's side at
//! `+-INFINITE_PARAMETER`, as [`Surface::extended_sides`] leaves them; where no side is
//! extended the result is the source itself. Every other side asked is extended `distance` across it, capped (said so,
//! [`SideExtension::UpToLimit`]):
//! - **round the axis**, no farther than a quarter of the turn the source leaves open (the
//!   shell's rule for a round, WR-532), so it never laps itself;
//! - **along the generator**, never past the cone's apex or the sphere's pole, and never past
//!   the torus's full turn of its tube.
//!
//! **The distance** across a side is measured at the side's middle on the iso across it, in
//! closed form on the analytic: the iso is certified (on its homogeneous control points, within
//! their rounding) in a plane normal to the axis (round it: a parallel, a circle about the
//! axis), in a plane through the axis (a sphere's meridian, a great circle; a torus's tube, the
//! circle of the minor radius about its centre), or straight (a cone's ruling or a plane's
//! iso, of degree 1).
//! Its length is the circle's radius times the angle swept from the side through the
//! extension's middle point to its end, or the ruling's chord. The extension parameter is
//! solved for the distance (or the cap) by bisection on that closed form, met to `1e-12` of it.
//!
//! **Weights.** An extension whose weights are not all positive is refused
//! ([`ExtendRefusal::WeightVanishes`]) naming the longest distance found whose weights are.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::ball::{knot_box, reach};
use super::sides::{round_the_axis, ExtendedSides, SideExtension};
use super::{keeps, Continuity, ExtendRefusal, Local};
use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::math::INFINITE_PARAMETER;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::math::{gamma, NOW};
use crate::nurbs::srf_basic::{Direction, Surface as Net};
use crate::nurbs::srf_tool::p1::extract_iso_crv;
use crate::surface::implicit::{lies_on, AnalyticSupport, Implicit};
use crate::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use crate::surface::side::{Across, BoxSide, Bracket};
use crate::surface::surface::{Surface, SurfaceKind};
use std::f64::consts::PI;

const OP: &str = "surface::extend::analytic::analytic_extended_sides";

/// A full turn, radians.
const TURN: f64 = 2.0 * PI;

fn refused(refusal: ExtendRefusal, distance: Option<f64>) -> SolidError {
    SolidError::ExtendRefused {
        operation: OP,
        refusal,
        distance,
    }
}

/// The iso across a side, by the analytic's closed form.
#[derive(Debug, Clone, Copy)]
enum Iso {
    /// A circle about the axis, in a plane normal to it.
    Parallel,
    /// A sphere's great circle through its poles, of the sphere's radius.
    Meridian { radius: f64 },
    /// A torus's tube circle, of its minor radius about the point at its major radius.
    Tube { major: f64, minor: f64 },
    /// A cone's straight ruling.
    Ruling,
}

/// The analytic's axis and the data its caps need.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// A point of the axis (the sphere's or torus's centre).
    origin: Vec3,
    /// The unit axis.
    axis: Vec3,
    /// A cone's apex, if it has one (not a cylinder).
    apex: Option<Vec3>,
    /// The points' absolute size, for the rounding.
    size: f64,
    /// A quarter of the turn the source leaves open round the axis, radians.
    quarter: f64,
    /// The turn a torus's tube leaves open, radians (0 for the other kinds).
    tube_left: f64,
}

/// `s`, an analytic kind (cone, sphere, torus; a plane with `planes`) or an offset surface of one, extended past
/// the sides `asked` of its natural box by `distance` (module docs); `None` for any other
/// kind.
pub(super) fn analytic_extended_sides(
    k: &Kernel,
    s: &Surface,
    asked: &[BoxSide],
    distance: f64,
    planes: bool,
) -> Result<Option<ExtendedSides>> {
    let mut offsets = Vec::new();
    let mut carrier = s;
    while let SurfaceKind::OffsetSurface(o) = carrier.kind() {
        offsets.push(o.offset_distance());
        carrier = o.base_surface();
    }
    let plane = matches!(carrier.kind(), SurfaceKind::Plane(_));
    if !(plane && planes
        || matches!(
            carrier.kind(),
            SurfaceKind::Cone(_) | SurfaceKind::Sphere(_) | SurfaceKind::Torus(_)
        ))
    {
        return Ok(None);
    }
    if !(distance.is_finite() && distance > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the distance is not finite and positive",
        ));
    }
    // the form proved on its label once; its continuation is certified at the end
    let support = AnalyticSupport::check(k, carrier)?;
    let implicit = support.implicit();
    let form = crate::blend::fillet_section::station::as_b_spline(k, carrier)?
        .ok_or_else(|| SolidError::sm_err(OP))?
        .into_owned();
    let step = carrier.step_uv_domain();
    let open = TURN - step.u_interval().length().to_radians();
    let (frame, along) = match implicit {
        Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } => (
            Frame {
                origin,
                axis,
                apex: (tan_angle != 0.0).then(|| origin + axis * (-radius / tan_angle)),
                size: 0.0,
                quarter: 0.25 * open,
                tube_left: 0.0,
            },
            Iso::Ruling,
        ),
        Implicit::Sphere { centre, radius } => {
            let AnalyticSupport::Sphere { placement, .. } = support else {
                return Err(SolidError::sm_err(OP));
            };
            (
                Frame {
                    origin: centre,
                    axis: placement.x_axis().cross(placement.y_axis()),
                    apex: None,
                    size: radius,
                    quarter: 0.25 * open,
                    tube_left: 0.0,
                },
                Iso::Meridian { radius },
            )
        }
        Implicit::Torus {
            centre,
            axis,
            major,
            minor,
        } => (
            Frame {
                origin: centre,
                axis,
                apex: None,
                size: major + minor,
                quarter: 0.25 * open,
                tube_left: TURN - step.v_interval().length().to_radians(),
            },
            Iso::Tube { major, minor },
        ),
        // straight both ways (its form is bilinear), never capped
        Implicit::Plane { origin, normal } => (
            Frame {
                origin,
                axis: normal,
                apex: None,
                size: 0.0,
                quarter: 0.0,
                tube_left: 0.0,
            },
            Iso::Ruling,
        ),
    };
    // the size of the net's points from the origin, for the rounding of the closed forms
    let net0 = form.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let mut size = frame.size;
    for c in net0.cpts().iter().flatten() {
        let w = if c.w == NOW { 1.0 } else { c.w };
        size = size.max(c.x.abs().max(c.y.abs()).max(c.z.abs()) / w.abs());
    }
    let o = frame.origin;
    let frame = Frame {
        size: size + o.x.abs().max(o.y.abs()).max(o.z.abs()),
        ..frame
    };
    let dom_src = carrier.natural_uv_domain_with_nurb(k)?;
    let mut cur = form;
    let mut sides = [SideExtension::NotAsked; 4];
    let mut extension = 0.0_f64;
    for (&side, done) in BoxSide::ALL.iter().zip(sides.iter_mut()) {
        if !asked.contains(&side) {
            continue;
        }
        let param = if side.is_u() {
            SurfParamType::U
        } else {
            SurfParamType::V
        };
        if carrier.is_closed(k, &dom_src, param, None, false)?.0 {
            *done = SideExtension::Closed;
            continue;
        }
        if carrier.side_diameter(k, &dom_src, side)?.lower <= 0.0 {
            *done = SideExtension::Collapsed;
            continue;
        }
        if matches!(implicit, Implicit::Plane { .. })
            && side.at(&dom_src).abs() >= INFINITE_PARAMETER
        {
            *done = SideExtension::Unbounded;
            continue;
        }
        let iso = if round_the_axis(carrier, side) == Some(true) {
            if open <= 0.0 {
                *done = SideExtension::Closed;
                continue;
            }
            Iso::Parallel
        } else {
            along
        };
        let (grown, across, capped, bound) = extend_side(&cur, side, &frame, iso, distance)?;
        cur = grown;
        extension = extension.max(bound);
        *done = if capped {
            SideExtension::UpToLimit(across)
        } else {
            SideExtension::Extended(across)
        };
    }
    // nothing extended: the source itself, as [`Surface::extended_sides`] leaves it
    if !sides
        .iter()
        .any(|x| matches!(x, SideExtension::Extended(_) | SideExtension::UpToLimit(_)))
    {
        return Ok(Some(ExtendedSides {
            surface: s.copy(k)?,
            sides,
            extension,
        }));
    }
    if !lies_on(&cur, &implicit)? {
        return Err(refused(ExtendRefusal::NotExact, None));
    }
    let mut surface = carrier.carry_round_on_continuation(Surface::from(cur));
    for &d in offsets.iter().rev() {
        surface = Surface::from(OffsetSurface::new(k, d, surface, true, SS_UNKNOWN)?);
    }
    Ok(Some(ExtendedSides {
        surface,
        sides,
        extension,
    }))
}

/// The unit vector along `v`, refused where it has no direction.
fn unit(v: Vec3) -> Result<Vec3> {
    let l = v.length();
    if l.is_nan() || l <= 0.0 {
        return Err(SolidError::sm_err(OP));
    }
    Ok(v * (1.0 / l))
}

/// `x`'s part normal to the axis.
fn radial(f: &Frame, x: Vec3) -> Vec3 {
    let d = x - f.origin;
    d - f.axis * f.axis.dot(d)
}

/// The angle between `a` and `b`, in radians.
fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b))
}

/// The circle an iso runs (its centre, its plane's normal and its radius) through `x0`;
/// `None` for a ruling.
fn circle(f: &Frame, iso: Iso, x0: Vec3) -> Result<Option<(Vec3, Vec3, f64)>> {
    let d = x0 - f.origin;
    Ok(match iso {
        Iso::Parallel => {
            let centre = f.origin + f.axis * f.axis.dot(d);
            Some((centre, f.axis, (x0 - centre).length()))
        }
        Iso::Meridian { radius } => Some((f.origin, unit(f.axis.cross(d))?, radius)),
        Iso::Tube { major, minor } => {
            let centre = f.origin + unit(radial(f, x0))? * major;
            Some((centre, unit(f.axis.cross(d))?, minor))
        }
        Iso::Ruling => None,
    })
}

/// The length along the iso from `x0` through `xm` to `x1`, in closed form.
fn length(f: &Frame, iso: Iso, x0: Vec3, xm: Vec3, x1: Vec3) -> Result<f64> {
    let Some((centre, n, radius)) = circle(f, iso, x0)? else {
        return Ok((x1 - x0).length());
    };
    // the swept angle, the way round through the middle point
    let n = if (xm - x0).cross(x1 - xm).dot(n) < 0.0 {
        n * -1.0
    } else {
        n
    };
    let (u0, u1) = (x0 - centre, x1 - centre);
    let mut phi = u0.cross(u1).dot(n).atan2(u0.dot(u1));
    if phi < 0.0 {
        phi += TURN;
    }
    Ok(radius * phi)
}

/// `cur` continued Natural past `side` by `distance` across it, capped (module docs): the
/// result, its across length, whether the cap stopped it, and its construction bound.
fn extend_side(
    cur: &BSplineSurface,
    side: BoxSide,
    f: &Frame,
    iso: Iso,
    distance: f64,
) -> Result<(BSplineSurface, Across, bool, f64)> {
    let net = cur.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    if matches!(iso, Iso::Ruling) && (if side.is_u() { net.p() } else { net.q() }) != 1 {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a cone's form is not straight along its rulings",
        });
    }
    let source = Local::of(net, side)?;
    let dom = knot_box(cur)?;
    let straight = (if side.is_u() { net.p() } else { net.q() }) == 1;
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
        let out = if straight {
            joint_removed(&out, side, side.at(&dom))?
        } else {
            out
        };
        Ok(Some((BSplineSurface::from_gw_nurb(&out), bound)))
    };
    // the iso across the side at its middle, from the side (0) to the extension's end (1)
    let from = side.at(&dom);
    let at = |s: &BSplineSurface, t: f64| -> Point2 {
        let end = knot_box(s).map_or(from, |d| side.at(&d));
        let x = from + t * (end - from);
        if side.is_u() {
            Point2::new(x, middle.y)
        } else {
            Point2::new(middle.x, x)
        }
    };
    let measure = |s: &BSplineSurface| -> Result<f64> {
        let x0 = s.evaluate_point(at(s, 0.0))?;
        let xm = s.evaluate_point(at(s, 0.5))?;
        let x1 = s.evaluate_point(at(s, 1.0))?;
        length(f, iso, x0, xm, x1)
    };
    let value = |e: f64| -> Result<Option<f64>> { made(e)?.map(|(s, _)| measure(&s)).transpose() };
    // the cap, from the side's middle point and the iso's other end inside the source
    let x_side = cur.evaluate_point(middle)?;
    let inside = {
        let (lo, hi) = (dom.min(), dom.max());
        match side {
            BoxSide::UMin => Point2::new(hi.x, middle.y),
            BoxSide::UMax => Point2::new(lo.x, middle.y),
            BoxSide::VMin => Point2::new(middle.x, hi.y),
            BoxSide::VMax => Point2::new(middle.x, lo.y),
        }
    };
    let x_in = cur.evaluate_point(inside)?;
    let cap = match iso {
        Iso::Parallel => radial(f, x_side).length() * f.quarter,
        Iso::Meridian { radius } => {
            // toward the pole the latitude climbs to
            let (h0, h_in) = (f.axis.dot(x_side - f.origin), f.axis.dot(x_in - f.origin));
            let pole = if h0 > h_in { f.axis } else { f.axis * -1.0 };
            radius * angle(x_side - f.origin, pole)
        }
        Iso::Tube { minor, .. } => minor * f.tube_left,
        Iso::Ruling => match f.apex {
            Some(apex) if (x_side - apex).length() < (x_in - apex).length() => {
                (x_side - apex).length()
            }
            _ => f64::INFINITY,
        },
    };
    let target = distance.min(cap);
    let capped = target < distance;
    let e = reach(&value, target, source.range()?)?;
    let (grown, bound) = made(e)?.ok_or_else(|| SolidError::sm_err(OP))?;
    let length = measure(&grown)?;
    certify_iso(&grown, side, middle, f, iso)?;
    // the closed form's rounding (the evaluations at the points' size, the products, the
    // atan2), and the points' construction bound through the angle
    let rounding = gamma(64) * (length + f.size) + 4.0 * bound;
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

/// `out`, a net of degree 1 across `side` continued Natural past it, with the joint knot
/// `joint` (the source's old end, now of multiplicity 1) removed: the span widened. Exact:
/// the continuation of a straight row is its own line, so the row at the joint lies on the
/// segment between its neighbours at the knots' ratio (on the homogeneous rows, so a rational
/// row too), and dropping it leaves the same surface on the same parameters. The analytic's
/// canonical form is one straight span (a plane's 2 x 2 net, a cone's two end circles), which
/// is what the kind's recognisers read; a joint left would make it two.
///
/// # Errors
///
/// `SM_ERR` where the joint is not a single interior knot of `out`; those of the net.
fn joint_removed(out: &Net, side: BoxSide, joint: f64) -> Result<Net> {
    let (p, q) = out.degrees();
    let knots = if side.is_u() {
        out.u_knots()
    } else {
        out.v_knots()
    };
    let last = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| SolidError::sm_err(OP))?;
    let at: Vec<usize> = (0..knots.len())
        .filter(|&i| knots.get(i) == Some(&joint))
        .collect();
    let &[i] = at.as_slice() else {
        return Err(SolidError::sm_err(OP));
    };
    // degree 1, clamped: an interior knot has two knots on each side, and the control
    // point at knot index i is point i - 1
    if i < 2 || i.checked_add(2).is_none_or(|j| j > last) {
        return Err(SolidError::sm_err(OP));
    }
    let row = i.checked_sub(1).ok_or_else(|| SolidError::sm_err(OP))?;
    let kept: Vec<f64> = knots
        .iter()
        .enumerate()
        .filter(|&(j, _)| j != i)
        .map(|(_, &x)| x)
        .collect();
    let mut net = out.cpts();
    if side.is_u() {
        if row >= net.len() {
            return Err(SolidError::sm_err(OP));
        }
        net = net
            .into_iter()
            .enumerate()
            .filter(|&(j, _)| j != row)
            .map(|(_, r)| r)
            .collect();
        Net::new(net, p, q, kept, out.v_knots().to_vec())
    } else {
        for r in &mut net {
            if row >= r.len() {
                return Err(SolidError::sm_err(OP));
            }
            *r = std::mem::take(r)
                .into_iter()
                .enumerate()
                .filter(|&(j, _)| j != row)
                .map(|(_, x)| x)
                .collect();
        }
        Net::new(net, p, q, out.u_knots().to_vec(), kept)
    }
}

/// Whether the iso across `side` of `s` at the side's middle `middle` is the closed form's
/// curve: every homogeneous control point in a plane normal to the axis (a parallel) or
/// through it (a meridian, a tube), within its rounding; a ruling is straight by its
/// degree. Refused as not exact otherwise.
fn certify_iso(
    s: &BSplineSurface,
    side: BoxSide,
    middle: Point2,
    f: &Frame,
    iso: Iso,
) -> Result<()> {
    if matches!(iso, Iso::Ruling) {
        return Ok(());
    }
    let net = s.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
    let (t, dir) = if side.is_u() {
        (middle.y, Direction::U)
    } else {
        (middle.x, Direction::V)
    };
    let curve = extract_iso_crv(net, t, dir)?;
    let points: Vec<(Vec3, f64, f64)> = curve
        .cpts()
        .iter()
        .map(|p| {
            let w = if p.w == NOW { 1.0 } else { p.w };
            let h = Vec3::new(p.x, p.y, p.z);
            // X W from the origin, and the size of its terms
            let mag = p.x.abs().max(p.y.abs()).max(p.z.abs()) + f.size * w.abs();
            (h - f.origin * w, w, mag)
        })
        .collect();
    let Some(&(x0, w0, _)) = points.first() else {
        return Err(SolidError::sm_err(OP));
    };
    // the plane's normal and its offset from the origin, per unit weight
    let (n, height) = if matches!(iso, Iso::Parallel) {
        (f.axis, f.axis.dot(x0) / w0)
    } else {
        // through the axis and the iso: its farthest point from the axis sets it
        let far = points
            .iter()
            .map(|&(x, w, _)| {
                let d = x * (1.0 / w);
                d - f.axis * f.axis.dot(d)
            })
            .fold(
                Vec3::ZERO,
                |m, d| if d.length() > m.length() { d } else { m },
            );
        (unit(f.axis.cross(far))?, 0.0)
    };
    for &(x, w, mag) in &points {
        // the dot product's rounding and the plane's own (its normal and height from
        // stored points), a few ulps of the terms' size each
        let off = n.dot(x) - height * w;
        if off.abs() > 8.0 * gamma(32) * (mag + height.abs() * w.abs()) {
            return Err(refused(ExtendRefusal::NotExact, None));
        }
    }
    Ok(())
}
