//! Round-tube copes: a tube's straight end cut to fit the outside cylinders of mating tubes.
//! How a mate fits the end (the distance of its axis from the endpoint, the joint's sine, how far
//! the cut reaches into the tube and where it lies along the mate), and the cut unwrapped onto the
//! tube's outside as a 1:1 template with a bounded interpolation error.
//!
//! The cut is where each mate's outside cylinder leaves the tube. At section angle `theta` the
//! tube's outside point is `radial = r * (x*cos(theta) + y*sin(theta))`. For a mate of radius `R`
//! whose axis `m` meets the tube's at the endpoint, with `c = axis . m`, `w = radial . m` and
//! `a = 1 - c^2`, the point `radial + t*axis` leaves the cylinder at the larger root
//! `t = (c*w + sqrt(w^2 + a*(R^2 - r^2))) / a`. The stock beyond the largest such `t` over the
//! mates survives.
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`terminal_at`]   | (none)   |
//! | [`fit`]           | (none)   |
//! | [`template`]      | (none)   |
//! | [`ends_cross`]    | (none)   |
//! | [`line_box`]      | (none)   |
//! | [`arc_box`]       | (none)   |
//!
//! Deviations: no upstream counterpart.

use crate::error::{Result, SolidError};
use crate::geom::vector3d::sqrt;
use crate::math::Vec3;

const OP: &str = "query::tube_cope";

/// Sketch roundoff (mm) by which a mate's axis may miss the endpoint and still be snapped to it.
pub const AXIS_SNAP: f64 = 1e-7;
/// The smallest joint sine a cope takes: below it the cut runs too far along the tube.
pub const MIN_SINE: f64 = 1e-3;
/// Clearance (mm) a cut keeps from a mate's ends and from the tube's other end or bend.
const CLEAR: f64 = 1e-5;
/// Fewest template segments, however loose the tolerance.
const MIN_SEGMENTS: f64 = 16.0;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// A tube's straight end: the original path endpoint, the unit direction into the stock, the
/// section's unit `x` and `y` there (`y = axis × x`), and the straight length from the endpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Terminal {
    /// The original path endpoint.
    pub origin: Vec3,
    /// Unit direction from the endpoint into the stock.
    pub axis: Vec3,
    /// The section's unit x.
    pub x: Vec3,
    /// The section's unit y, `axis × x`.
    pub y: Vec3,
    /// The straight length from the endpoint (mm).
    pub length: f64,
}

/// A mate's outside cylinder: its start, the unit direction from it, radius and length, and how
/// far the mate's own copes reach in from its start and its end (0 where an end is not coped).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mate {
    /// The cylinder's start, on its axis.
    pub origin: Vec3,
    /// Unit direction from the start.
    pub axis: Vec3,
    /// Outside radius (mm).
    pub radius: f64,
    /// Length (mm).
    pub length: f64,
    /// How far the mate's copes reach in from its start (mm).
    pub coped_start: f64,
    /// How far the mate's copes reach in from its end (mm).
    pub coped_end: f64,
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    /// The smallest corner.
    pub min: Vec3,
    /// The largest corner.
    pub max: Vec3,
}

impl Box3 {
    fn meets(&self, o: &Box3) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
            && self.min.z <= o.max.z
            && o.min.z <= self.max.z
    }
}

fn padded(min: Vec3, max: Vec3, pad: f64) -> Box3 {
    let p = Vec3::new(pad, pad, pad);
    Box3 {
        min: min - p,
        max: max + p,
    }
}

fn lower(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn upper(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
}

/// A box holding a straight swept piece from `a` to `b` whose section reaches `pad` from its
/// centreline.
#[must_use]
pub fn line_box(a: Vec3, b: Vec3, pad: f64) -> Box3 {
    padded(lower(a, b), upper(a, b), pad)
}

/// A box holding a circular bend through `a` about `center` whose section reaches `pad` from its
/// centreline: the ball of the bend's radius plus `pad` about its centre.
#[must_use]
pub fn arc_box(a: Vec3, center: Vec3, pad: f64) -> Box3 {
    padded(center, center, a.distance(center) + pad)
}

fn unit(v: Vec3) -> bool {
    v.is_finite_vec() && (v.length() - 1.0).abs() <= 1e-8
}

/// `t` with its axes rescaled to exactly unit length. [`terminal_ok`] accepts axes up to 1e-8 off
/// unit length, but the cut's formulas assume unit axes, and near [`MIN_SINE`] the sine taken from
/// `1 - cosine^2` magnifies that error.
fn unit_axes(t: &Terminal) -> Result<Terminal> {
    Ok(Terminal {
        axis: t.axis.unitize()?,
        x: t.x.unitize()?,
        y: t.y.unitize()?,
        ..*t
    })
}

trait Finite {
    fn is_finite_vec(&self) -> bool;
}

impl Finite for Vec3 {
    fn is_finite_vec(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.length() <= 2e9
    }
}

/// Whether `t` is a usable end: finite, unit and orthonormal axes with `axis = x × y`, and a
/// straight length in (1e-6, 2e9] mm.
#[must_use]
pub fn terminal_ok(t: &Terminal) -> bool {
    t.origin.is_finite_vec()
        && unit(t.axis)
        && unit(t.x)
        && unit(t.y)
        && t.x.cross(t.y).distance(t.axis) <= 1e-8
        && t.x.dot(t.y).abs() <= 1e-8
        && t.length.is_finite()
        && t.length > 1e-6
        && t.length <= 2e9
}

/// Whether `m` is a usable mate cylinder: finite, a unit axis, radius in [0.005, 5000] mm, length
/// in [1e-6, 2e9] mm and copes reaching no less than 0 and no further than its length.
#[must_use]
pub fn mate_ok(m: &Mate) -> bool {
    m.origin.is_finite_vec()
        && unit(m.axis)
        && m.radius.is_finite()
        && (0.005..=5000.0).contains(&m.radius)
        && m.length.is_finite()
        && (1e-6..=2e9).contains(&m.length)
        && (0.0..=m.length).contains(&m.coped_start)
        && (0.0..=m.length).contains(&m.coped_end)
}

/// The end of a swept path as a [`Terminal`], from the section plane there (`origin`, unit `x`
/// and `y`, its normal `x × y` along the path) and the straight length from it. At the path's
/// start the stock lies along the normal; at its end (`at_end`) it lies against it, and `y`
/// turns over so that `y = axis × x` still.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a plane whose axes are not unit and orthogonal, or a length
/// out of range.
pub fn terminal_at(origin: Vec3, x: Vec3, y: Vec3, length: f64, at_end: bool) -> Result<Terminal> {
    let normal = x.cross(y);
    let t = if at_end {
        Terminal {
            origin,
            axis: -normal,
            x,
            y: -y,
            length,
        }
    } else {
        Terminal {
            origin,
            axis: normal,
            x,
            y,
            length,
        }
    };
    if terminal_ok(&t) {
        Ok(t)
    } else {
        Err(invalid(
            "the section plane at a tube's end is not orthonormal or its length is out of range",
        ))
    }
}

/// Why a mate cannot cut a tube's end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Refusal {
    /// The mate's axis passes this far (mm) from the endpoint.
    OffAxis {
        /// The distance from the endpoint to the mate's axis (mm).
        distance: f64,
    },
    /// The mate is smaller than the tube.
    Smaller {
        /// The mate's outside diameter (mm).
        mate_diameter: f64,
        /// The tube's outside diameter (mm).
        tube_diameter: f64,
    },
    /// The mate runs almost parallel to the tube.
    Parallel {
        /// The joint's sine.
        sine: f64,
    },
    /// The cut may reach the tube's other end or its next bend.
    PastTerminal,
    /// The cut may meet one of the mate's end caps.
    MateEnd,
    /// The joint reaches one of the mate's own copes.
    MateCope,
    /// The mate's cylinder may also meet a bend or another portion of the tube.
    MeetsOther,
}

/// A mate fitted to a tube's end: its cylinder start moved onto the line through the endpoint
/// square to its axis (removing only sketch roundoff), and how far into the tube the cut can
/// reach, over every point of the tube's section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// The cylinder's start, snapped so its axis passes through the endpoint.
    pub origin: Vec3,
    /// How far from the endpoint into the tube the cut can reach (mm).
    pub depth: f64,
}

/// Fit mate `m` to end `t` of a tube of outside radius `radius`, whose other swept pieces lie in
/// the boxes `others`. A mate the cut cannot use is answered as a [`Refusal`].
///
/// The cut covers the whole end disk only when the mate's axis meets the endpoint, the mate is at
/// least as large as the tube and the joint is not nearly parallel; then its removed part joins
/// the end. Over the tube's section it reaches at most `(R + |c|*r)/sine` into the tube, and spans
/// `along + min(0, c*depth) - r*sine .. along + max(0, c*depth) + r*sine` along the mate, where
/// `along` is the endpoint's station on the mate's axis.
///
/// # Errors
/// [`SolidError::InvalidInput`] for an end, mate or radius out of range.
pub fn fit(
    t: &Terminal,
    radius: f64,
    m: &Mate,
    others: &[Box3],
) -> Result<core::result::Result<Fit, Refusal>> {
    if !terminal_ok(t) || !mate_ok(m) || !radius.is_finite() || !(0.005..=5000.0).contains(&radius)
    {
        return Err(invalid(
            "a tube end, mate cylinder or tube radius is out of range",
        ));
    }
    let t = &unit_axes(t)?;
    let axis = m.axis.unitize()?;
    let q = t.origin - m.origin;
    let along = q.dot(axis);
    let distance = (q - axis * along).length();
    let cosine = t.axis.dot(axis);
    let sine = sqrt(1.0 - cosine * cosine);
    if distance > AXIS_SNAP {
        return Ok(Err(Refusal::OffAxis { distance }));
    }
    if m.radius < radius {
        return Ok(Err(Refusal::Smaller {
            mate_diameter: m.radius * 2.0,
            tube_diameter: radius * 2.0,
        }));
    }
    if sine < MIN_SINE {
        return Ok(Err(Refusal::Parallel { sine }));
    }
    let origin = t.origin - axis * along;
    let depth = (m.radius + cosine.abs() * radius) / sine;
    if depth >= t.length - CLEAR {
        return Ok(Err(Refusal::PastTerminal));
    }
    let lo = along + (cosine * depth).min(0.0) - radius * sine;
    let hi = along + (cosine * depth).max(0.0) + radius * sine;
    if lo <= CLEAR || hi >= m.length - CLEAR {
        return Ok(Err(Refusal::MateEnd));
    }
    if lo <= m.coped_start + CLEAR || hi >= m.length - m.coped_end - CLEAR {
        return Ok(Err(Refusal::MateCope));
    }
    let tool = line_box(origin, origin + axis * m.length, m.radius);
    if others.iter().any(|b| b.meets(&tool)) {
        return Ok(Err(Refusal::MeetsOther));
    }
    Ok(Ok(Fit { origin, depth }))
}

/// A cope unwrapped onto a tube's outside.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    /// `[arc length on the outside from the seam, station into the stock]`, both mm, from the
    /// seam once round to the seam again (both seam copies).
    pub points: Vec<[f64; 2]>,
    /// The unit radial direction of the seam.
    pub seam_direction: Vec3,
    /// The tube's outside circumference (mm).
    pub circumference: f64,
    /// The tube's outside diameter (mm).
    pub outer_diameter: f64,
}

/// Why a cope cannot be unwrapped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TemplateRefusal {
    /// The sampling bound needs more than the allowed segments.
    TooManySegments,
    /// A station left the cut's retained depth: the record does not describe this cut.
    OutsideDepth,
    /// The caller stopped it.
    Cancelled,
}

/// A retained cope: the tube's end and outside radius, its mates (each a unit axis and outside
/// radius, every axis through the endpoint) and how far the cut reaches into the tube.
#[derive(Clone, Copy, Debug)]
pub struct Cope<'a> {
    /// The coped end.
    pub end: &'a Terminal,
    /// The tube's outside radius (mm).
    pub radius: f64,
    /// Each mate's unit axis and outside radius.
    pub mates: &'a [(Vec3, f64)],
    /// How far the cut reaches into the tube from the endpoint (mm).
    pub depth: f64,
}

/// `cope` unwrapped from `seam` (radians from the end's `x`, right-handed about its axis) with at
/// most `tolerance` mm between the drawn and the true cut, in at most `max_segments` segments.
/// `cancelled` is asked every 128 points.
///
/// Each exit station has the derivative bound `L = r*(1 + |c|)/sine`, and their maximum the
/// largest of those, even at cusps and where one mate takes over from another. With angular step
/// `h` the axial interpolation error is at most `L*h/2` and the wrapped radial sag at most
/// `r*h^2/8`; each gets half the tolerance.
///
/// # Errors
/// [`SolidError::InvalidInput`] for an end, radius, mate, seam or tolerance out of range.
pub fn template(
    cope: &Cope<'_>,
    seam: f64,
    tolerance: f64,
    max_segments: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<core::result::Result<Template, TemplateRefusal>> {
    use core::f64::consts::TAU;
    let Cope {
        end: t,
        radius,
        mates,
        depth,
    } = *cope;
    if !terminal_ok(t)
        || !radius.is_finite()
        || !(0.005..=5000.0).contains(&radius)
        || mates.is_empty()
        || !seam.is_finite()
        || seam.abs() > 1e6
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || !depth.is_finite()
        || depth <= 0.0
    {
        return Err(invalid(
            "a cope template's end, radius, mates, seam, tolerance or depth is out of range",
        ));
    }
    let t = &unit_axes(t)?;
    let mut lipschitz = 0.0_f64;
    for &(axis, r) in mates {
        if !unit(axis) {
            return Err(invalid(
                "a cope template's mate is not a unit axis at least the tube's size, off parallel",
            ));
        }
        let cosine = t.axis.dot(axis.unitize()?).clamp(-1.0, 1.0);
        let sine = sqrt(1.0 - cosine * cosine);
        if !r.is_finite() || sine < MIN_SINE || r < radius {
            return Err(invalid(
                "a cope template's mate is not a unit axis at least the tube's size, off parallel",
            ));
        }
        lipschitz = lipschitz.max(radius * (1.0 + cosine.abs()) / sine);
    }
    let step = (tolerance / lipschitz).min(sqrt(4.0 * tolerance / radius));
    let count = (TAU / step).ceil().max(MIN_SEGMENTS);
    if !count.is_finite() || count > max_segments as f64 {
        return Ok(Err(TemplateRefusal::TooManySegments));
    }
    #[allow(clippy::cast_sign_loss)] // finite and at least MIN_SEGMENTS
    let n = count as usize;
    let mut points = Vec::new();
    for i in 0..=n {
        if i % 128 == 0 && cancelled() {
            return Ok(Err(TemplateRefusal::Cancelled));
        }
        let theta = seam + TAU * (if i == n { 0.0 } else { i as f64 / n as f64 });
        let radial = (t.x * theta.cos() + t.y * theta.sin()) * radius;
        let mut station = 0.0_f64;
        for &(axis, r) in mates {
            let axis = axis.unitize()?;
            let cosine = t.axis.dot(axis);
            let a = 1.0 - cosine * cosine;
            let w = radial.dot(axis);
            let d = w * w + a * (r * r - radius * radius);
            station = station.max((cosine * w + sqrt(d)) / a);
        }
        if !station.is_finite() || station < 0.0 || station > depth + CLEAR {
            return Ok(Err(TemplateRefusal::OutsideDepth));
        }
        points.push([radius * TAU * i as f64 / n as f64, station]);
    }
    Ok(Ok(Template {
        points,
        seam_direction: t.x * seam.cos() + t.y * seam.sin(),
        circumference: TAU * radius,
        outer_diameter: 2.0 * radius,
    }))
}

/// Whether the two end cuts of one straight tube, reaching `start` and `end` mm in from its two
/// ends, may cross: within its straight `length` they must stay [`CLEAR`] apart. Anything not
/// finite counts as crossing.
#[must_use]
pub fn ends_cross(start: f64, end: f64, length: f64) -> bool {
    !(start.is_finite() && end.is_finite() && length.is_finite()) || start + end >= length - CLEAR
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
    const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    /// The start of a straight 200 mm tube along +X from the origin, section x = +Y.
    fn start() -> Terminal {
        terminal_at(Vec3::new(0.0, 0.0, 0.0), Y, Z, 200.0, false).unwrap()
    }

    /// A mate along +Y through the origin, 300 long, from y = -150.
    fn mate(radius: f64) -> Mate {
        Mate {
            origin: Vec3::new(0.0, -150.0, 0.0),
            axis: Y,
            radius,
            length: 300.0,
            coped_start: 0.0,
            coped_end: 0.0,
        }
    }

    #[test]
    fn a_path_end_turns_its_section_round_into_the_stock() {
        let t = terminal_at(Vec3::new(5.0, 0.0, 0.0), Y, Z, 10.0, true).unwrap();
        assert_eq!((t.axis, t.x, t.y), (-X, Y, -Z));
        assert!(terminal_at(Vec3::new(0.0, 0.0, 0.0), Y, Y, 10.0, false).is_err());
    }

    #[test]
    fn a_square_mate_reaches_its_radius_in_and_spans_the_tube_along_itself() {
        // c = 0, sine = 1: depth = R; along = 150, so the joint spans 150 -/+ r on the mate.
        let fit = fit(&start(), 20.0, &mate(30.0), &[]).unwrap().unwrap();
        assert_eq!(
            fit,
            Fit {
                origin: Vec3::new(0.0, -150.0, 0.0),
                depth: 30.0
            }
        );
        let mut coped = mate(30.0);
        coped.coped_start = 130.0;
        assert_eq!(
            super::fit(&start(), 20.0, &coped, &[]).unwrap(),
            Err(Refusal::MateCope)
        );
        coped.coped_start = 129.0;
        assert!(super::fit(&start(), 20.0, &coped, &[]).unwrap().is_ok());
    }

    #[test]
    fn refusals_carry_what_failed() {
        let mut off = mate(30.0);
        off.origin = Vec3::new(20.0, -150.0, 0.0);
        assert_eq!(
            fit(&start(), 20.0, &off, &[]).unwrap(),
            Err(Refusal::OffAxis { distance: 20.0 })
        );
        assert_eq!(
            fit(&start(), 20.0, &mate(15.0), &[]).unwrap(),
            Err(Refusal::Smaller {
                mate_diameter: 30.0,
                tube_diameter: 40.0
            })
        );
        let mut along = mate(30.0);
        along.axis = X;
        along.origin = Vec3::new(-150.0, 0.0, 0.0);
        assert!(matches!(
            fit(&start(), 20.0, &along, &[]).unwrap(),
            Err(Refusal::Parallel { .. })
        ));
        let others = [line_box(
            Vec3::new(0.0, 100.0, 0.0),
            Vec3::new(10.0, 100.0, 0.0),
            20.0,
        )];
        assert_eq!(
            fit(&start(), 20.0, &mate(30.0), &others).unwrap(),
            Err(Refusal::MeetsOther)
        );
    }

    fn equal_square(depth: f64) -> (Terminal, [(Vec3, f64); 1], f64) {
        (start(), [(Y, 20.0)], depth)
    }

    #[test]
    fn equal_square_tubes_unwrap_to_r_times_abs_cos() {
        // R = r, c = 0: the exit station is |w| = r*|cos(theta)|. L = r, so with tolerance 0.05
        // the step is 0.05/20 and n = ceil(2*pi/0.0025) = 2514.
        let (end, mates, depth) = equal_square(20.0);
        let cope = Cope {
            end: &end,
            radius: 20.0,
            mates: &mates,
            depth,
        };
        let t = template(&cope, 0.0, 0.05, 65_536, &|| false)
            .unwrap()
            .unwrap();
        assert_eq!(t.points.len(), 2515);
        for (i, p) in t.points.iter().enumerate() {
            let theta = core::f64::consts::TAU * (i % 2514) as f64 / 2514.0;
            assert!((p[1] - 20.0 * theta.cos().abs()).abs() < 1e-9, "{i}: {p:?}");
        }
        assert!((t.circumference - 40.0 * core::f64::consts::PI).abs() < 1e-12);
        assert_eq!(t.seam_direction, Y);
    }

    #[test]
    fn a_tight_tolerance_or_a_cancel_refuses_the_template() {
        let (end, mates, depth) = equal_square(20.0);
        let cope = Cope {
            end: &end,
            radius: 20.0,
            mates: &mates,
            depth,
        };
        assert_eq!(
            template(&cope, 0.0, 1e-6, 65_536, &|| false).unwrap(),
            Err(TemplateRefusal::TooManySegments)
        );
        assert_eq!(
            template(&cope, 0.0, 0.05, 65_536, &|| true).unwrap(),
            Err(TemplateRefusal::Cancelled)
        );
        let shallow = Cope {
            depth: 10.0,
            ..cope
        };
        assert_eq!(
            template(&shallow, 0.0, 0.05, 65_536, &|| false).unwrap(),
            Err(TemplateRefusal::OutsideDepth)
        );
    }

    /// A mate axis 1e-8 short of unit length, nearly along the tube (sine 0.0014), from `-axis`
    /// so its axis passes through the endpoint. Unnormalised, the cut's depth comes out as
    /// 7.1067 and its station at radial +Y as 7.0707; with the axis rescaled both are
    /// `(R + c*r)/sine = 7.1428536` for equal radii 0.005.
    fn near_unit_mate() -> Mate {
        let axis = Vec3::new(0.999_999_01, 0.0014, 0.0);
        Mate {
            origin: -axis,
            axis,
            radius: 0.005,
            length: 20.0,
            coped_start: 0.0,
            coped_end: 0.0,
        }
    }

    #[test]
    fn a_mate_axis_a_hair_off_unit_length_is_rescaled_before_the_cut() {
        let m = near_unit_mate();
        assert!(unit(m.axis) && (m.axis.length() - 1.0).abs() > 9e-9);
        let n = m.axis / m.axis.length();
        let sine = sqrt(1.0 - n.x * n.x);
        let exact = (0.005 + n.x * 0.005) / sine;
        assert!((exact - 7.142_853_57).abs() < 1e-8, "{exact}");
        // The terminal is 7.12 long: the true cut reaches past it.
        let t = terminal_at(Vec3::new(0.0, 0.0, 0.0), Y, Z, 7.12, false).unwrap();
        assert_eq!(fit(&t, 0.005, &m, &[]).unwrap(), Err(Refusal::PastTerminal));
        // Long enough, the fit's depth is the normalised one.
        let t = terminal_at(Vec3::new(0.0, 0.0, 0.0), Y, Z, 7.2, false).unwrap();
        let f = fit(&t, 0.005, &m, &[]).unwrap().unwrap();
        assert!((f.depth - exact).abs() < 1e-9, "{}", f.depth);
        // The template's first point is the seam at radial +Y (the end's x): the same station.
        let mates = [(m.axis, 0.005)];
        let cope = Cope {
            end: &t,
            radius: 0.005,
            mates: &mates,
            depth: f.depth,
        };
        let tpl = template(&cope, 0.0, 0.01, 65_536, &|| false)
            .unwrap()
            .unwrap();
        assert!(
            (tpl.points[0][1] - exact).abs() < 1e-9,
            "{}",
            tpl.points[0][1]
        );
        assert!((tpl.outer_diameter - 0.01).abs() < 1e-15);
    }

    #[test]
    fn end_cuts_cross_when_they_meet_within_the_clearance() {
        // Separated, touching and overlapping cuts on a 100 mm straight.
        assert!(!ends_cross(30.0, 40.0, 100.0));
        assert!(ends_cross(50.0, 50.0, 100.0));
        assert!(ends_cross(50.0, 50.0 - CLEAR / 2.0, 100.0));
        assert!(ends_cross(60.0, 50.0, 100.0));
        assert!(ends_cross(f64::NAN, 0.0, 100.0));
        assert!(ends_cross(0.0, 0.0, f64::INFINITY));
    }
}
