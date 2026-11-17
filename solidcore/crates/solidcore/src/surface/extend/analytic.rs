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
//! box, which moves every kept UV trim. The continuation gives the extension's parameters;
//! its points are then built from the analytic's definition (below), and the result is
//! certified on the analytic on its net ([`lies_on`]), within its rounding. It is a
//! B-spline, carrying the source's kept exact round where it has one (#2487); an offset
//! surface of an analytic is the offset of its base so extended.
//!
//! **The extension from the definition.** The continuation is the same polynomial, but the
//! source's net is on the analytic only to its rounding, and carrying a short span far
//! carries that residual out with it, `(1 + 2 e / L)` times per degree for a span of length
//! `L` continued by `e`: a 5.8 mm plane continued 547 mm each way is 6e-11 off its plane.
//! So the points past the joint are not taken from the continuation but built on the
//! analytic from its own data, at the place the continuation puts them (`on_definition`):
//! - a plane's: the plane's point at the continued point's own coordinates in the plane
//!   (its origin plus its two axes, the part along the normal dropped);
//! - a cone's along its rulings: at the continued point's height on the axis, at the cone's
//!   radius there, in the row's own direction from the axis and at its own scale;
//! - round the axis (a cone, a sphere, a torus): each row a circle about the axis, its arc
//!   past the joint the joint's point turned about the axis by the angle the continuation
//!   reached, its middle on the bisector at the radius over `cos` of half that angle, its
//!   weights the continuation's at its ends and `cos` of the half angle times their
//!   geometric mean between (the rational quadratic arc's own condition);
//! - a sphere's meridian or a torus's tube: the profile circle in the plane through the
//!   axis, turned the same way about its own centre, carried to every row as the form
//!   revolves it.
//!
//! Each new point is then as near the analytic as one evaluation of its data rounds,
//! whatever the reach; only where along the analytic it lies comes from the continuation,
//! and that only moves it on the analytic. The rows round the axis and along a meridian or
//! tube must be rational quadratic arcs (every analytic form is), refused as unsupported
//! otherwise.
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
//! extendable the result is the source itself. A side whose reach is below its resolution
//! ([`SideExtension::Unchanged`]) is extendable: with no side extended, the result is the
//! form itself (the same B-spline family as any extension). Every other side asked is extended `distance` across it, capped (said so,
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

use super::ball::{knot_box, reach, source_iso};
use super::sides::{round_the_axis, ExtendedSides, SideExtension};
use super::{below_resolution, keeps, Continuity, ExtendRefusal, Local};
use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::math::INFINITE_PARAMETER;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::math::CPoint;
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
        .ok_or_else(|| SolidError::surface_failed(OP, "the carrier has no B-spline form"))?
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
                return Err(SolidError::surface_failed(
                    OP,
                    "the support is not the sphere its implicit form says",
                ));
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
    let net0 = form
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
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
        let Some((grown, across, capped, bound)) =
            extend_side(&cur, side, &frame, iso, &implicit, distance)?
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
    // no side extendable (each closed, collapsed or unbounded): the source itself, as
    // [`Surface::extended_sides`] leaves it. A side left `Unchanged` is extendable, its reach
    // below its resolution: the result is the form continued by nothing, the same B-spline
    // family as a reach one least step longer (a plane kept as itself here takes the shell
    // offset's plane branches, which the form does not).
    if !sides.iter().any(|x| {
        matches!(
            x,
            SideExtension::Extended(_) | SideExtension::UpToLimit(_) | SideExtension::Unchanged
        )
    }) {
        return Ok(Some(ExtendedSides {
            surface: s.copy(k)?,
            sides,
            source: 0.0,
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
        source: 0.0,
        extension,
    }))
}

/// The unit vector along `v`, refused where it has no direction.
fn unit(v: Vec3) -> Result<Vec3> {
    let l = v.length();
    if l.is_nan() || l <= 0.0 {
        return Err(SolidError::surface_failed(
            OP,
            "the vector has no direction",
        ));
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

/// The length along the iso from `x0` to `x1`, in closed form, the way round that continues
/// the source's iso outward: from its far end `x_far` through its halfway point `x_half` to
/// `x0` (`ball::source_iso`; the extension's own points are too close at the least step to
/// say the sense).
fn length(f: &Frame, iso: Iso, (x_far, x_half): (Vec3, Vec3), x0: Vec3, x1: Vec3) -> Result<f64> {
    let Some((centre, n, radius)) = circle(f, iso, x0)? else {
        return Ok((x1 - x0).length());
    };
    // the swept angle, the way round the source's iso turns
    let n = if (x_half - x_far).cross(x0 - x_half).dot(n) < 0.0 {
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

/// The widened analytic path measures the distance in the side's own frame, from the
/// STEP parameters the construction widened: the closed-form radius and arc keep the
/// result the same at any position. The parameters come through the polar converters, so
/// their own rounding is in the angle and radius bounds, and a conversion that could only
/// be solved numerically is refused as not exact.
pub(super) fn widened_across(
    k: &Kernel,
    (source, source_dom, source_side): (&Surface, &crate::geom::extent2d::Extent2d, BoxSide),
    (s, dom, side): (&Surface, &crate::geom::extent2d::Extent2d, BoxSide),
) -> Result<Across> {
    let support = AnalyticSupport::check(k, s)?;
    let form = crate::blend::fillet_section::station::as_b_spline(k, s)?
        .ok_or_else(|| SolidError::surface_failed(OP, "the surface has no B-spline form"))?;
    let middle = side.point(dom, 0.5);
    let end = side.at(&knot_box(&form)?);
    let to = if side.is_u() {
        Point2::new(end, middle.y)
    } else {
        Point2::new(middle.x, end)
    };
    let from = source_side.point(source_dom, 0.5);
    let parallel = round_the_axis(s, side) == Some(true);
    // a straight iso (a cone's ruling, a plane) is its chord, on the form
    let circular = match support.implicit() {
        Implicit::Cone { .. } => parallel,
        Implicit::Sphere { .. } | Implicit::Torus { .. } => true,
        Implicit::Plane { .. } => false,
    };
    if !circular {
        return Ok(super::measure::across(super::measure::chord(
            &form, middle, to,
        )?));
    }
    let (a, a_exact) = source.convert_uv_from_nurbs_to_step_exact(k, from)?;
    let (b, b_exact) = s.convert_uv_from_nurbs_to_step_exact(k, to)?;
    // A numerical inverse is no closed form: its angle is not bounded by the conversion's
    // rounding, so the distance across cannot be certified here.
    if !(a_exact && b_exact) {
        return Err(super::not_exact(None));
    }
    // The polar converters' own rounding, in degrees: it scales with the angles converted
    // (up to a turn), so it is taken from both ends rather than from the difference alone.
    let conv_x = gamma(8) * (a.x.abs() + b.x.abs());
    let conv_y = gamma(8) * (a.y.abs() + b.y.abs());
    let (radius, radius_err, delta, conv) = match support.implicit() {
        Implicit::Cone {
            radius, tan_angle, ..
        } => {
            let term = tan_angle * a.y;
            let r = radius + term;
            (
                r,
                gamma(4) * (radius.abs() + term.abs()) + tan_angle.abs() * conv_y.to_radians(),
                b.x - a.x,
                conv_x,
            )
        }
        Implicit::Sphere { radius, .. } => {
            if parallel {
                let latitude = a.y.to_radians();
                let r = radius * latitude.cos();
                (
                    r,
                    gamma(8) * radius * (1.0 + latitude.abs())
                        + radius * latitude.sin().abs() * conv_y.to_radians(),
                    b.x - a.x,
                    conv_x,
                )
            } else {
                (radius, 0.0, b.y - a.y, conv_y)
            }
        }
        Implicit::Torus { major, minor, .. } => {
            if parallel {
                let latitude = a.y.to_radians();
                let term = minor * latitude.cos();
                (
                    major + term,
                    gamma(12) * (major.abs() + minor * (1.0 + latitude.abs()))
                        + minor * latitude.sin().abs() * conv_y.to_radians(),
                    b.x - a.x,
                    conv_x,
                )
            } else {
                (minor, 0.0, b.y - a.y, conv_y)
            }
        }
        Implicit::Plane { .. } => {
            return Ok(super::measure::across(super::measure::chord(
                &form, middle, to,
            )?));
        }
    };
    let angle = delta.to_radians().abs();
    let angle_err = gamma(4) * angle + conv.to_radians();
    Ok(super::measure::across(super::measure::circle_from_angle(
        (radius, radius_err),
        (angle, angle_err),
    )?))
}

/// `cur` continued Natural past `side` by `distance` across it, capped (module docs): the
/// result, its across length, whether the cap stopped it, and its construction bound;
/// `None` where the (capped) distance is below the side's resolution ([`below_resolution`]).
fn extend_side(
    cur: &BSplineSurface,
    side: BoxSide,
    f: &Frame,
    iso: Iso,
    implicit: &Implicit,
    distance: f64,
) -> Result<Option<(BSplineSurface, Across, bool, f64)>> {
    let net = cur
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
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
        let out = g.surface(side)?;
        if !keeps(net, &out, side)? {
            return Err(SolidError::port_invariant(
                OP,
                "the extension moved the source's net",
            ));
        }
        let Some(out) = on_definition(&out, side, f, iso, implicit)? else {
            return Ok(None);
        };
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
    let sense = source_iso(cur, &dom, side, middle)?;
    let measure = |s: &BSplineSurface| -> Result<f64> {
        let x0 = s.evaluate_point(at(s, 0.0))?;
        let x1 = s.evaluate_point(at(s, 1.0))?;
        length(f, iso, sense, x0, x1)
    };
    let value = |e: f64| -> Result<Option<f64>> { made(e)?.map(|(s, _)| measure(&s)).transpose() };
    // the length across an extension whose construction bound is `bound`, with the closed
    // form's rounding (the evaluations at the points' size, the products, the atan2) and the
    // points' construction bound through the angle
    let across_of = |s: &BSplineSurface, bound: f64| -> Result<Across> {
        let length = measure(s)?;
        let rounding = gamma(64) * (length + f.size) + 4.0 * bound;
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
    let least = source.least_step()?;
    if let Some((g, bound)) = made(least)? {
        if below_resolution(Some(&across_of(&g, bound)?), bound, target) {
            return Ok(None);
        }
    }
    let e = reach(&value, target, source.range()?, least)?;
    let (grown, bound) = made(e)?.ok_or_else(|| {
        SolidError::surface_failed(
            OP,
            "the extension at the reached distance could not be made",
        )
    })?;
    let across = across_of(&grown, bound)?;
    certify_iso(&grown, side, middle, f, iso)?;
    Ok(Some((grown, across, capped, bound)))
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
        .ok_or_else(|| SolidError::surface_failed(OP, "the side's knot vector is empty"))?;
    let at: Vec<usize> = (0..knots.len())
        .filter(|&i| knots.get(i) == Some(&joint))
        .collect();
    let &[i] = at.as_slice() else {
        return Err(SolidError::surface_failed(
            OP,
            "the joint is not a single knot of the extension",
        ));
    };
    // degree 1, clamped: an interior knot has two knots on each side, and the control
    // point at knot index i is point i - 1
    if i < 2 || i.checked_add(2).is_none_or(|j| j > last) {
        return Err(SolidError::surface_failed(
            OP,
            "the joint is not an interior knot",
        ));
    }
    let row = i
        .checked_sub(1)
        .ok_or_else(|| SolidError::surface_failed(OP, "the joint is not an interior knot"))?;
    let kept: Vec<f64> = knots
        .iter()
        .enumerate()
        .filter(|&(j, _)| j != i)
        .map(|(_, &x)| x)
        .collect();
    let mut net = out.cpts();
    if side.is_u() {
        if row >= net.len() {
            return Err(SolidError::surface_failed(
                OP,
                "the joint's control row is outside the net",
            ));
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
                return Err(SolidError::surface_failed(
                    OP,
                    "the joint's control row is outside the net",
                ));
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
///
/// The limit is the rounding of the test, counted (definitions-and-certificates.md §4.4,
/// §7.5). The iso's control point `c_j = (h_j, w_j)` is `L = q - m` de Boor levels of the
/// net's points across the side (`extract_iso_crv`: `q` the degree across, `m` the knot's
/// multiplicity at the middle; a boundary row is copied, `L = 0`), so it is within `γ_(12L)`
/// of its stencil's size (12 operations a level, as `eval::collapse` counts them), per
/// coordinate. With `H_j` and `W_j` the largest coordinate and weight of point `j`'s stencil
/// (the `q + 1` net points it is made from), `|o|` the largest coordinate of the axis's
/// origin and `1.75 >= √3` the norm of a unit normal over its largest coordinate, write
/// `A_j = 1.75 (H_j + |o| W_j) + |height| W_j`. Then:
///
/// | term | count | what it bounds |
/// |---|---|---|
/// | the point and its test | `γ_(12L+6) A_j` | the de Boor levels, `h - o w` (2), the dot product with the normal (3), the height times `w` and the difference (1 together, chained) |
/// | a parallel's height | `γ_(12L+7) A_0 \|w_j\| / (\|w_0\| - γ_(12L) W_0)` | the height read off the first point: its levels, the same five operations and the division |
/// | a meridian's or tube's normal | `2 (γ_(12L+8) A_k / (\|w_k\| - γ_(12L) W_k) / \|far\| + γ_12) · 1.75 (H_j + \|o\| W_j)` | the normal `unit(axis × far)` from the farthest point `k`: its levels, `x / w` and the radial part (8), the cross product and the unit (with the stored axis's own rounding, 12); a unit vector moves at most `2 \|δc\| / \|c\|` |
/// | the limit itself | `× (1 + γ_16)` | the limit's own operations, rounded outward |
///
/// Taken at the stencil's size, not the point's: a de Boor level rounds its inputs, the
/// stencil, so an iso point far smaller than its rows (a parallel at a long cylinder's
/// middle) has the rows' rounding. Measured on the offset and extension goldens before this
/// count (#15039), every point was off its plane by at most 0.0123 of the tuned limit
/// `8 γ_32 (|c| + size |w| + |height| |w|)`, which this replaces.
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
    let net = s
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP, "the B-spline surface has no NURB"))?;
    let (t, dir) = if side.is_u() {
        (middle.y, Direction::U)
    } else {
        (middle.x, Direction::V)
    };
    let curve = extract_iso_crv(net, t, dir)?;
    let stencils = iso_stencils(net, side.is_u(), t)?;
    let weight = |w: f64| if w == NOW { 1.0 } else { w };
    let o = f.origin.x.abs().max(f.origin.y.abs()).max(f.origin.z.abs());
    // (x_j = h_j - o w_j, w_j, the stencil's (H_j, W_j))
    let points: Vec<(Vec3, f64, (f64, f64))> = curve
        .cpts()
        .iter()
        .zip(&stencils.sizes)
        .map(|(p, &size)| {
            let w = weight(p.w);
            (Vec3::new(p.x, p.y, p.z) - f.origin * w, w, size)
        })
        .collect();
    if points.len() != curve.cpts().len() {
        return Err(SolidError::surface_failed(
            OP,
            "the iso and the net's stencils differ in count",
        ));
    }
    let Some(&(x0, w0, (h0, ww0))) = points.first() else {
        return Err(SolidError::surface_failed(
            OP,
            "the iso has no control points",
        ));
    };
    let levels = stencils.levels;
    let g = |ops: usize| gamma(12 * levels + ops);
    let lift = |h: f64, ww: f64| NORM_OVER_MAX * (h + o * ww);
    // the exact weight's least magnitude, from a computed one
    let least = |w: f64, ww: f64| w.abs() - gamma(12 * levels) * ww;
    // the plane's normal, its offset from the origin per unit weight, and the plane's own
    // error as a part per unit weight (a parallel's height) and per lifted size (a normal)
    let (n, height, per_w, turn) = if matches!(iso, Iso::Parallel) {
        let height = f.axis.dot(x0) / w0;
        let d0 = least(w0, ww0);
        if d0.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
            return Err(refused(ExtendRefusal::NotExact, None));
        }
        let a0 = lift(h0, ww0) + height.abs() * ww0;
        (f.axis, height, g(7) * a0 / d0, 0.0)
    } else {
        // through the axis and the iso: its farthest point from the axis sets it
        let (far, (wk, (hk, wwk))) = points
            .iter()
            .map(|&(x, w, size)| {
                let d = x * (1.0 / w);
                (d - f.axis * f.axis.dot(d), (w, size))
            })
            .fold((Vec3::ZERO, (1.0, (0.0, 0.0))), |m, d| {
                if d.0.length() > m.0.length() {
                    d
                } else {
                    m
                }
            });
        let dk = least(wk, wwk);
        let reach = far.length();
        if dk.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater)
            || reach.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater)
        {
            return Err(refused(ExtendRefusal::NotExact, None));
        }
        let turn = 2.0 * (g(8) * lift(hk, wwk) / dk / reach + gamma(12));
        (unit(f.axis.cross(far))?, 0.0, 0.0, turn)
    };
    for &(x, w, (h, ww)) in &points {
        let off = n.dot(x) - height * w;
        let lifted = lift(h, ww);
        let a = lifted + height.abs() * ww;
        let limit = (g(6) * a + per_w * w.abs() + turn * lifted) * (1.0 + gamma(16));
        // written to accept, so a NaN refuses
        if off.abs() <= limit {
            continue;
        }
        return Err(refused(ExtendRefusal::NotExact, None));
    }
    Ok(())
}

/// `1.75`, at least `√3`: the Euclidean norm of a vector over its largest coordinate, so a
/// unit normal's dot product with `x` is at most this times `x`'s largest coordinate.
const NORM_OVER_MAX: f64 = 1.75;

/// The net's points each control point of an iso is made from (module `certify_iso`).
struct IsoStencils {
    /// The de Boor levels the iso is made with: the degree across less the knot's
    /// multiplicity at the parameter (0 where the iso is a boundary row, copied).
    levels: usize,
    /// For each iso control point, the largest coordinate and the largest weight of its
    /// stencil.
    sizes: Vec<(f64, f64)>,
}

/// The stencils of the iso of `net` along u at `v = t` (`along_u`) or along v at `u = t`:
/// for each iso point, the `q + 1` net points across whose de Boor levels make it (the
/// span holding `t`, as `extract_iso_crv` finds it), and the levels.
///
/// # Errors
///
/// `SM_ERR` where the knots hold no span at `t`.
fn iso_stencils(net: &Net, along_u: bool, t: f64) -> Result<IsoStencils> {
    let (p, q) = net.degrees();
    let (knots, deg) = if along_u {
        (net.v_knots(), q)
    } else {
        (net.u_knots(), p)
    };
    let no_span = || SolidError::surface_failed(OP, "the knots hold no span at the iso");
    // the last span whose start is at most t, within the clamped spans
    let last = knots.len().checked_sub(deg + 2).ok_or_else(no_span)?;
    let span = (deg..=last)
        .rev()
        .find(|&i| knots.get(i).is_some_and(|&k| k <= t))
        .unwrap_or(deg);
    let mult = knots.iter().filter(|&&k| k == t).count();
    let levels = deg.saturating_sub(mult);
    let first = span.checked_sub(deg).ok_or_else(no_span)?;
    let cpts = net.cpts();
    let size = |c: &CPoint| {
        let w = if c.w == NOW { 1.0 } else { c.w };
        (c.x.abs().max(c.y.abs()).max(c.z.abs()), w.abs())
    };
    let fold = |a: (f64, f64), b: (f64, f64)| (a.0.max(b.0), a.1.max(b.1));
    let sizes: Vec<(f64, f64)> = if along_u {
        cpts.iter()
            .map(|row| {
                row.iter()
                    .skip(first)
                    .take(deg + 1)
                    .map(size)
                    .fold((0.0, 0.0), fold)
            })
            .collect()
    } else {
        let width = cpts.first().map_or(0, Vec::len);
        (0..width)
            .map(|j| {
                cpts.iter()
                    .skip(first)
                    .take(deg + 1)
                    .filter_map(|row| row.get(j))
                    .map(size)
                    .fold((0.0, 0.0), fold)
            })
            .collect()
    };
    Ok(IsoStencils { levels, sizes })
}

/// `out`, a net continued Natural past `side` (its `deg` points past the joint in each row
/// across, `deg` its degree across the side), with those points rebuilt from the analytic's
/// definition (module docs, **The extension from the definition**): the joint's points and
/// the continuation's parameters (where along the analytic each new point is, and the
/// weights that place it) are kept, and the points are put on the analytic by its own data.
///
/// `None` where the continuation turned a half turn or more (no quadratic arc's weights
/// are all positive there), as the continuation gives where a weight vanishes.
///
/// # Errors
///
/// `Unsupported` where the rows across a side round the axis, a sphere's meridian or a
/// torus's tube are not rational quadratic arcs; `SM_ERR` where a joint point is on the
/// axis or the net is short.
fn on_definition(
    out: &Net,
    side: BoxSide,
    f: &Frame,
    iso: Iso,
    implicit: &Implicit,
) -> Result<Option<Net>> {
    let (p, q) = out.degrees();
    let deg = if side.is_u() { p } else { q };
    let mut cpts = out.cpts();
    let rational = out.is_rational();
    let n_along = if side.is_u() {
        cpts.len()
    } else {
        cpts.first().map_or(0, Vec::len)
    };
    let rows = if side.is_u() {
        cpts.first().map_or(0, Vec::len)
    } else {
        cpts.len()
    };
    // the index along the side's direction of the joint (k = 0) and of the k-th point past it
    let index = |k: usize| -> Result<usize> {
        let i = if side.is_high() {
            n_along.checked_sub(deg + 1).map(|j| j + k)
        } else {
            deg.checked_sub(k)
        };
        i.filter(|&i| i < n_along).ok_or_else(|| {
            SolidError::surface_failed(OP, "the control point index is outside the net")
        })
    };
    let is_u = side.is_u();
    let slot = |cp: &mut [Vec<CPoint>], a: usize, r: usize| -> Result<CPoint> {
        Ok(*slot_mut(cp, is_u, a, r)?)
    };
    // a control point as its 3D point and its weight
    let point = |c: &CPoint| -> (Vec3, f64) {
        let w = if c.w == NOW { 1.0 } else { c.w };
        (Vec3::new(c.x / w, c.y / w, c.z / w), w)
    };
    let put = |c: &mut CPoint, x: Vec3, w: f64| {
        *c = if rational {
            CPoint::new(x.x * w, x.y * w, x.z * w, w)
        } else {
            CPoint::new(x.x, x.y, x.z, NOW)
        };
    };
    // each row's joint point and the points past it, as continued
    let mut joint = Vec::new();
    let mut past = Vec::new();
    for r in 0..rows {
        joint.push(point(&slot(&mut cpts, index(0)?, r)?));
        let mut row = Vec::new();
        for k in 1..=deg {
            row.push(point(&slot(&mut cpts, index(k)?, r)?));
        }
        past.push(row);
    }
    let mut rebuilt: Vec<Vec<(Vec3, f64)>> = Vec::new();
    match (*implicit, iso) {
        // a plane: each point the plane's at its own coordinates in the plane
        (Implicit::Plane { origin, normal }, _) => {
            for row in &past {
                rebuilt.push(
                    row.iter()
                        .map(|&(x, w)| (x - normal * normal.dot(x - origin), w))
                        .collect(),
                );
            }
        }
        // a cone's ruling: each point at its height along the axis, at the cone's radius
        // there, in its row's own direction from the axis (and its row's scale: a point off
        // the cone in the form, the middle of an arc, is that much off it again)
        (
            Implicit::Cone {
                origin,
                axis,
                radius,
                tan_angle,
            },
            Iso::Ruling,
        ) => {
            let at = |h: f64| radius + tan_angle * h;
            for (&(j, _), row) in joint.iter().zip(&past) {
                let d = j - origin;
                let h = axis.dot(d);
                let rho = at(h);
                if !(rho.is_finite() && rho.abs() > 0.0) {
                    return Err(SolidError::surface_failed(
                        OP,
                        "the iso's radius at a joint row is zero or undefined",
                    ));
                }
                let template = (d - axis * h) * (1.0 / rho);
                rebuilt.push(
                    row.iter()
                        .map(|&(x, w)| {
                            let h = axis.dot(x - origin);
                            (origin + axis * h + template * at(h), w)
                        })
                        .collect(),
                );
            }
        }
        // round the axis: each row a circle about the axis, its arc past the joint turned
        // about the axis from the joint's point by the angle the continuation reached
        (_, Iso::Parallel) => {
            if deg != 2 {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a form round its axis not of rational quadratic arcs",
                });
            }
            let (origin, axis) = (f.origin, f.axis);
            // every row turns by the same angle and ends at the same weight ratio (the form is
            // a tensor product): take them from the row farthest from the axis, since a row on
            // the axis (a pole) has no angle. A ratio per row would differ by the rounding of
            // the continuation, and rows far apart along the axis turn that into a height:
            // the iso between them leaves its plane
            let mut far = (0.0_f64, 0.0_f64, 1.0_f64);
            for (&(j, wj), row) in joint.iter().zip(&past) {
                let &[_, (end, wn)] = row.as_slice() else {
                    return Err(SolidError::surface_failed(
                        OP,
                        "a row past the joint does not have two points",
                    ));
                };
                let centre = origin + axis * axis.dot(j - origin);
                let (r0, r1) = (j - centre, end - centre);
                if r0.length() > far.0 {
                    far = (
                        r0.length(),
                        axis.dot(r0.cross(r1)).atan2(r0.dot(r1)),
                        wn / wj,
                    );
                }
            }
            if !(far.0.is_finite() && far.0 > 0.0) {
                return Err(SolidError::surface_failed(
                    OP,
                    "the rows' spread along the axis is zero or undefined",
                ));
            }
            let delta = far.1;
            let half = 0.5 * delta;
            if half.cos().is_nan() || half.cos() <= 0.0 {
                return Ok(None);
            }
            for &(j, wj) in &joint {
                let wn = wj * far.2;
                let centre = origin + axis * axis.dot(j - origin);
                let r0 = j - centre;
                let turn = |phi: f64| r0 * phi.cos() + axis.cross(r0) * phi.sin();
                rebuilt.push(vec![
                    (
                        centre + turn(half) * (1.0 / half.cos()),
                        half.cos() * (wj * wn).sqrt(),
                    ),
                    (centre + turn(delta), wn),
                ]);
            }
        }
        // a sphere's meridian or a torus's tube: the profile, a circle in the plane through
        // the axis, turned about its centre in that plane from the joint's point by the angle
        // the continuation reached, then carried to every row as the form revolves it
        (_, Iso::Meridian { .. } | Iso::Tube { .. }) => {
            if deg != 2 {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a form along its profile not of rational quadratic arcs",
                });
            }
            let (origin, axis) = (f.origin, f.axis);
            let major = match iso {
                Iso::Tube { major, .. } => major,
                _ => 0.0,
            };
            // the profile's row: the joint nearest the axis (the others are its scaled
            // copies, the middles of the arcs round the axis)
            let flat = |x: Vec3| {
                let d = x - origin;
                d - axis * axis.dot(d)
            };
            let mut r_ref = 0;
            for (r, &(x, _)) in joint.iter().enumerate() {
                let best = joint
                    .get(r_ref)
                    .map_or(f64::INFINITY, |&(y, _)| flat(y).length());
                if flat(x).length() < best {
                    r_ref = r;
                }
            }
            let (Some(&(j, wj)), Some(&[_, (end, wn)])) =
                (joint.get(r_ref), past.get(r_ref).map(Vec::as_slice))
            else {
                return Err(SolidError::surface_failed(
                    OP,
                    "the reference row is outside the net",
                ));
            };
            let radial = flat(j);
            let rho = radial.length();
            if !(rho.is_finite() && rho > 0.0) {
                return Err(SolidError::surface_failed(
                    OP,
                    "the reference row's radius is zero or undefined",
                ));
            }
            let dir = radial * (1.0 / rho);
            // the profile plane's coordinates (along the radial direction, along the axis)
            // about the profile circle's centre
            let plane = |x: Vec3| {
                let d = x - origin;
                (dir.dot(d) - major, axis.dot(d))
            };
            let (a0, b0) = plane(j);
            let (a1, b1) = plane(end);
            let delta = (a0 * b1 - b0 * a1).atan2(a0 * a1 + b0 * b1);
            let turn = |phi: f64, s: f64| {
                let (c, sn) = (phi.cos(), phi.sin());
                ((a0 * c - b0 * sn) * s + major, (a0 * sn + b0 * c) * s)
            };
            let half = 0.5 * delta;
            if half.cos().is_nan() || half.cos() <= 0.0 {
                return Ok(None);
            }
            let profile = [
                (
                    turn(half, 1.0 / half.cos()),
                    half.cos() * (wj * wn).sqrt() / wj,
                ),
                (turn(delta, 1.0), wn / wj),
            ];
            for &(jr, wr) in &joint {
                let template = flat(jr) * (1.0 / rho);
                rebuilt.push(
                    profile
                        .iter()
                        .map(|&((s, h), ratio)| (origin + axis * h + template * s, wr * ratio))
                        .collect(),
                );
            }
        }
        (_, Iso::Ruling) => {
            return Err(SolidError::surface_failed(
                OP,
                "a ruling has no closed-form continuation",
            ))
        }
    }
    for (r, row) in rebuilt.into_iter().enumerate() {
        for (k, (x, w)) in row.into_iter().enumerate() {
            put(slot_mut(&mut cpts, is_u, index(k + 1)?, r)?, x, w);
        }
    }
    Net::new(cpts, p, q, out.u_knots().to_vec(), out.v_knots().to_vec()).map(Some)
}

/// The control point `a` along `is_u`'s direction (u for true) in row `r` across it.
fn slot_mut(cp: &mut [Vec<CPoint>], is_u: bool, a: usize, r: usize) -> Result<&mut CPoint> {
    let (i, j) = if is_u { (a, r) } else { (r, a) };
    cp.get_mut(i)
        .and_then(|x| x.get_mut(j))
        .ok_or_else(|| SolidError::surface_failed(OP, "the control point is outside the net"))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;
    use crate::surface::cone::p1::Cone;

    /// The limit `certify_iso` holds point `j` of a parallel of `form` across `side` to, for
    /// a net of degree 1 across with one span (both rows the stencil, one de Boor level),
    /// the axis's origin at 0, written out from the count in `certify_iso`'s docs so a change
    /// to the count fails here: `(γ_18 A_j + γ_19 A_0 |w_j| / (|w_0| - γ_12 W_0)) (1 + γ_16)`,
    /// `A = 1.75 H + |height| W`.
    fn limit(form: &BSplineSurface, side: BoxSide, j: usize, height: f64) -> f64 {
        let net = form.gw_nurb().unwrap();
        let w = |c: &CPoint| if c.w == NOW { 1.0 } else { c.w };
        // (H, W) over the points whose index along the iso is `i`: the whole stencil
        let stencil = |i: usize| {
            let mut hw = (0.0_f64, 0.0_f64);
            for (a, row) in net.cpts().iter().enumerate() {
                for (b, c) in row.iter().enumerate() {
                    if (if side.is_u() { a } else { b }) == i {
                        hw.0 = hw.0.max(c.x.abs()).max(c.y.abs()).max(c.z.abs());
                        hw.1 = hw.1.max(w(c).abs());
                    }
                }
            }
            hw
        };
        let (hj, wwj) = stencil(j);
        let (h0, ww0) = stencil(0);
        let a = |h: f64, ww: f64| 1.75 * h + height.abs() * ww;
        // both rows share their weights here, so the iso's weight is the stencil's
        (gamma(18) * a(hj, wwj) + gamma(19) * a(h0, ww0) * wwj / (ww0 - gamma(12) * ww0))
            * (1.0 + gamma(16))
    }

    /// A cylinder of radius 1 about z, 60 degrees round, 1000 long: rows far larger than
    /// the turn's own points, so a limit taken at the rows' size would be about 1000 times
    /// the one at the iso's.
    fn long_cylinder() -> BSplineSurface {
        let k = Kernel::default();
        let s = Surface::from(Cone::new_cylinder_bounded(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            1.0,
            0.0,
            60.0,
            1000.0,
            false,
            false,
            false,
        ));
        crate::blend::fillet_section::station::as_b_spline(&k, &s)
            .unwrap()
            .unwrap()
            .into_owned()
    }

    /// The net with every control point of index `i` along the iso across `side` moved
    /// `delta` along the axis (its homogeneous `delta w`): the iso's point `i` moves by
    /// `delta`, the rest stay.
    fn moved(form: &BSplineSurface, side: BoxSide, i: usize, delta: f64) -> BSplineSurface {
        let net = form.gw_nurb().unwrap();
        let mut cpts = net.cpts();
        for (a, row) in cpts.iter_mut().enumerate() {
            for (b, c) in row.iter_mut().enumerate() {
                let along = if side.is_u() { a } else { b };
                if along == i {
                    let w = if c.w == NOW { 1.0 } else { c.w };
                    c.z += delta * w;
                }
            }
        }
        let (p, q) = net.degrees();
        BSplineSurface::from_gw_nurb(
            &Net::new(cpts, p, q, net.u_knots().to_vec(), net.v_knots().to_vec()).unwrap(),
        )
    }

    /// `certify_iso` refuses a parallel whose control point is off its plane by a fifth past
    /// the counted limit and passes one off by a fifth within it (#15039). The margin is the
    /// test's own rounding: the offset is computed at the size the limit bounds, so it is
    /// known only to a few of the limit's ulps-of-size, not to one ulp. The limit is at the
    /// stencil's size: both rows, 0 and 1000 along the axis, for the parallel at 500.
    #[test]
    fn the_turn_certificate_holds_the_iso_to_its_counted_rounding() {
        let form = long_cylinder();
        let frame = Frame {
            origin: Vec3::new(0.0, 0.0, 0.0),
            axis: Vec3::new(0.0, 0.0, 1.0),
            apex: None,
            size: 1.0,
            quarter: 0.0,
            tube_left: 0.0,
        };
        let dom = knot_box(&form).unwrap();
        let mut tested = 0;
        for side in BoxSide::ALL {
            let middle = side.point(&dom, 0.5);
            let (t, dir) = if side.is_u() {
                (middle.y, Direction::U)
            } else {
                (middle.x, Direction::V)
            };
            let iso = extract_iso_crv(form.gw_nurb().unwrap(), t, dir).unwrap();
            let pts = iso.cpts();
            // the parallel: every point at one height (the other side's iso is a ruling)
            let z = |c: &CPoint| c.z / if c.w == NOW { 1.0 } else { c.w };
            if pts.len() < 3 || pts.iter().any(|c| (z(c) - z(&pts[0])).abs() > 1e-9) {
                continue;
            }
            assert!(certify_iso(&form, side, middle, &frame, Iso::Parallel).is_ok());
            let lim = limit(&form, side, 1, z(&pts[0]));
            // the stencil's size, the rows at 0 and 1000, not the point's at 500
            assert!(lim > 1000.0 * 1.75 * gamma(18), "{side:?}: {lim:e}");
            let w1 = if pts[1].w == NOW { 1.0 } else { pts[1].w };
            let over = moved(&form, side, 1, 1.2 * lim / w1);
            assert!(
                certify_iso(&over, side, middle, &frame, Iso::Parallel).is_err(),
                "{side:?}: 1.2 times the limit passes"
            );
            let under = moved(&form, side, 1, 0.8 * lim / w1);
            assert!(
                certify_iso(&under, side, middle, &frame, Iso::Parallel).is_ok(),
                "{side:?}: 0.8 times the limit is refused"
            );
            tested += 1;
        }
        assert!(tested > 0, "no side's iso is the parallel");
    }
}
