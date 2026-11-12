//! A multi-section blend: one B-spline surface through ordered cross-sections that leaves the
//! first and the last of them with the continuity their frames ask for.
//!
//! Upstream: none (not in SMLib). SolidCraft's `loft_blend` (`crates/kernel/src/loft_blend.rs`
//! at 52afe21) and the Hermite rows of its `bridge::section`, moved into the kernel so no
//! application builds the surface.
//!
//! | Rust               | Upstream |
//! |--------------------|----------|
//! | [`section_blend`]  | (none; SolidCraft's `loft_blend`) |
//! | [`SectionFrame`]   | (none; SolidCraft's `bridge::Frame`) |
//! | [`EndOrder`]       | (none; SolidCraft's `Continuity`) |
//! | `end_rows` (crate) | (none; the rows of SolidCraft's `bridge::section`) |
//! | `third_derivative` (crate) | (none; SolidCraft's `bridge::third_derivative`) |
//! | [`SectionBlendDefinition`], [`section_blend_surface`] | (none; the face's definition kept, #3140) |
//! | [`section_frames`], [`FrameOptions`] | (none; SolidCraft's `section_frames`, `bridge::frames`) |
//!
//! **The surface.** The sections run along u, each sampled at the same stations along v. Section
//! `i` sits at `u = i / (n - 1)` and the surface passes through every station of it: each
//! control row is interpolated through its stations along v (degree `min(3, stations - 1)`, the
//! kernel's global interpolation at uniform parameters with averaged knots). Along u the knots are
//! clamped and repeated `degree` times at every interior section, so every span is one Bezier.
//! A span's end rows are the Hermite rows (`end_rows`) of its two sections: position, then
//! tangent, curvature and change of curvature as the section holds them, written as the Bezier's
//! leading control rows at the span's parameter interval. The u degree is the largest
//! `c + c' - 1` over the spans (`c`, `c'` the conditions at its two ends); a span with fewer is
//! degree-elevated exactly (by its own row elevation: the rows are vectors of points before any
//! surface exists, so the kernel's surface and single-curve elevations do not fit them).
//!
//! **What each section holds (the definition, SolidCraft's).**
//! - The first and the last section hold what their [`EndOrder`] asks (G0 to G3) from the
//!   caller's frames: the tangent `leave * speed`, the curvature `bend * speed^2` and the change
//!   of curvature from `bend_change` (`third_derivative`: only its part square to the tangent
//!   holds the condition, the rest a reparametrisation). The speed is the station's chord to the
//!   neighbouring section `c`, over 3, times `1 + crown`, over the span's interval `1 / (n - 1)`:
//!   `d_u = leave * c / 3 * (1 + crown) * (n - 1)` for `n` sections, so the Bezier's first control
//!   offset is `c (1 + crown) / (3 m)` at span degree `m`. The last section's frames point back
//!   into the blend, so its odd derivatives turn round.
//! - An interior section holds its position and the run's derivatives from finite differences of
//!   the neighbouring sections' stations: the central first and second differences, and a third
//!   (central, or one-sided at the second and the last-but-one section) where four sections
//!   reach. So the joint is C3 in u with four or more sections and C2 with three, at every
//!   station.
//!
//! This is SolidCraft's definition of the blend, not an approximation of another surface: the
//! finite differences are the conditions the surface is built to, not estimates of a curve the
//! sections lie on, and no error against anything is claimed. What is exact: the surface passes
//! through every station; at every station of an end it holds the frames' conditions to rounding;
//! at every station of an interior section the spans on either side agree in their u derivatives
//! up to the order held. Between stations the end and joint curves are the v interpolants of the
//! stations' rows (the interpolation is linear and commutes with evaluation in u), not conditions
//! of their own.
//!
//! **Refused**, naming why: fewer than 2 or more than [`MAX_SECTIONS`] sections; sections of
//! different station counts, or a count outside [`MIN_STATIONS`] ..= [`MAX_STATIONS`]; a frame
//! that is not finite, or a `leave` of no length at an end that uses it; a crown outside
//! `-0.9 ..= 3`; two neighbouring sections that meet at a station (no room for a blend there).
//!
//! **The two floors.** Two tests guard divisions, and both refuse; neither adjusts an input or a
//! result to get past them. A tangent's length (an end's `leave`, and the first derivative in
//! `third_derivative`) is divided by: a length that is not finite and positive is refused,
//! never replaced by a default direction. Two neighbouring sections meeting at a station
//! (distance not positive) leave the span there no room: a speed of zero, and no tangent to
//! divide by at the next section; the blend is refused, never nudged apart. Both are exact tests,
//! not SolidCraft's `1e-12` and `1e-9` floors: a chord of `1e-12` is not refused, its speed is
//! about `1e-12` and the end rows collapse onto the section, a valid but degenerate surface.
//!
//! **The definition kept** ([`SectionBlendDefinition`]): the sections, the end orders and crowns,
//! and the motions the net took since it was made, in order. The net is [`section_blend`] of
//! them with each motion applied by [`BSplineSurface::transform`], bit for bit, so a re-certified
//! net is the rebuilt one exactly (bound 0), in any orientation of its parameter square.
//!
//! Deviations: no upstream counterpart. SolidCraft's `1e-12` floor on a tangent's length and its
//! `1e-9` floor on the distance between neighbouring sections are exact tests here (a length that
//! is not positive): what the floors protect is the division, and an exact test protects it
//! without a scale-dependent constant. Its one-sided third difference at the last-but-one section
//! has its sign turned (`f(i-2) - 3 f(i-1) + 3 f(i) - f(i+1)`, the negative of the change of
//! curvature it estimates); here it is the backward difference, exact on a cubic like the other
//! two. Its batched row solves are one solve per row. No
//! recursion.

use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::KnotType;
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use crate::nurbs::crv_fit::p1::{interp_given_params, interp_knot_vector};
use crate::nurbs::knots::KnotVector;
use crate::nurbs::math::NOW;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "sweep::section_blend";

/// Most cross-sections one blend runs through.
pub const MAX_SECTIONS: usize = 50;
/// Fewest stations a cross-section is sampled at (a straight line across it).
pub const MIN_STATIONS: usize = 2;
/// Most stations a cross-section is sampled at.
pub const MAX_STATIONS: usize = 257;

/// One station of a cross-section: its point and, at the first and last section, how the blend
/// leaves it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionFrame {
    /// The point.
    pub p: Point3,
    /// The direction the blend leaves in, towards the next section (at the last section, back
    /// into the blend, towards the section before). Its length is ignored.
    pub leave: Vec3,
    /// The curvature vector the blend takes along `leave` (zero for a straight start).
    pub bend: Vec3,
    /// The arc-length derivative of `bend` along `leave`.
    pub bend_change: Vec3,
}

/// The continuity a blend holds at its first or last section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOrder {
    /// Position.
    G0,
    /// Position and tangent direction.
    G1,
    /// And curvature.
    G2,
    /// And change of curvature.
    G3,
}

impl EndOrder {
    /// The conditions held: 1 to 4 (position, tangent, curvature, change of curvature).
    #[must_use]
    pub const fn conditions(self) -> usize {
        match self {
            Self::G0 => 1,
            Self::G1 => 2,
            Self::G2 => 3,
            Self::G3 => 4,
        }
    }
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The derivatives a section holds along the run at each station: `d[k]` is the `(k + 1)`-th,
/// as many as its conditions less one.
struct Held {
    p: Vec<Point3>,
    d: [Vec<Vec3>; 3],
}

/// The Bezier control rows at one end of a span of degree `m` over the parameter interval
/// `delta`, from the end's position `p` and its derivatives `d` (the first `conditions - 1` of
/// them used), as the span leaves the end (`backward` false: rows `0, 1, ..`) or arrives at it
/// (`backward` true: rows `m, m - 1, ..`, the odd derivatives read the other way): the leading
/// rows of a Bezier whose derivatives at that end are `d`. Row `k` comes back at index `k` from
/// the end.
///
/// # Errors
/// `InvalidInput` for more conditions than the degree carries or than `d` gives.
pub(crate) fn end_rows(
    p: Point3,
    d: &[Vec3],
    conditions: usize,
    m: usize,
    delta: f64,
    backward: bool,
) -> Result<Vec<Point3>> {
    if conditions == 0 || conditions > 4 || conditions > m + 1 || d.len() + 1 < conditions {
        return Err(invalid(
            "a span's end carries more conditions than its degree or its data",
        ));
    }
    #[allow(clippy::cast_precision_loss)] // a degree, at most 32
    let k = m as f64;
    let sign = if backward { -1.0 } else { 1.0 };
    let one = delta / k;
    let two = if m >= 2 {
        delta * delta / (k * (k - 1.0))
    } else {
        0.0
    };
    let three = if m >= 3 {
        delta * delta * delta / (k * (k - 1.0) * (k - 2.0))
    } else {
        0.0
    };
    let mut rows = vec![p];
    if conditions >= 2 {
        let d1 = *d.first().ok_or_else(|| invalid("a missing tangent"))?;
        let q1 = p + d1 * (sign * one);
        rows.push(q1);
        if conditions >= 3 {
            let d2 = *d.get(1).ok_or_else(|| invalid("a missing curvature"))?;
            let q2 = d2 * two + q1 * 2.0 - p;
            rows.push(q2);
            if conditions >= 4 {
                let d3 = *d
                    .get(2)
                    .ok_or_else(|| invalid("a missing change of curvature"))?;
                rows.push(d3 * (sign * three) + q2 * 3.0 - q1 * 3.0 + p);
            }
        }
    }
    Ok(rows)
}

/// The third derivative a curve needs at an end so that the arc-length derivative of its
/// curvature vector is `target`, from its first two derivatives `d1`, `d2` there: only its part
/// square to the tangent holds the condition, the rest is a reparametrisation and is left zero
/// (SolidCraft's `bridge::third_derivative`).
///
/// # Errors
/// `InvalidInput` for a tangent of no length or a result that is not finite.
pub(crate) fn third_derivative(d1: Vec3, d2: Vec3, target: Vec3) -> Result<Vec3> {
    let sigma = d1.length();
    // the floor on the tangent's length, divided by below: refused, never adjusted
    if !(sigma.is_finite() && sigma > 0.0) {
        return Err(invalid(
            "no room for the change of curvature: the tangent has no length",
        ));
    }
    let t = d1 * (1.0 / sigma);
    let tp = d2 * (1.0 / sigma) - d1 * (d1.dot(d2) / (sigma * sigma * sigma));
    let nn = d2 - t * d2.dot(t);
    let r = tp * (-d2.dot(t)) - t * d2.dot(tp);
    let x = (target * sigma.powi(5) - r * (sigma * sigma) + nn * (2.0 * d1.dot(d2)))
        * (1.0 / (sigma * sigma));
    if !(x.x.is_finite() && x.y.is_finite() && x.z.is_finite()) {
        return Err(invalid("the change of curvature is not finite"));
    }
    Ok(x - t * x.dot(t))
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// One exact degree elevation of a Bezier net of rows: the same curve in one row more.
fn elevate(rows: &[Vec<Point3>]) -> Vec<Vec<Point3>> {
    let degree = rows.len().saturating_sub(1);
    #[allow(clippy::cast_precision_loss)] // a degree, at most 32
    let up = degree as f64 + 1.0;
    let mut out: Vec<Vec<Point3>> = Vec::new();
    if let Some(first) = rows.first() {
        out.push(first.clone());
    }
    for i in 1..=degree {
        if let (Some(before), Some(here)) = (rows.get(i - 1), rows.get(i)) {
            #[allow(clippy::cast_precision_loss)] // a row index, at most 32
            let a = i as f64 / up;
            out.push(
                before
                    .iter()
                    .zip(here)
                    .map(|(x, y)| *x * a + *y * (1.0 - a))
                    .collect(),
            );
        }
    }
    if let Some(last) = rows.last() {
        out.push(last.clone());
    }
    out
}

/// The blend surface through `sections` (each a cross-section of the same stations, in order),
/// holding `orders[0]` at the first section and `orders[1]` at the last, `crown` scaling how fast
/// it leaves each (`-0.9 ..= 3`, 0 the plain blend). See the module's notes for the definition.
///
/// # Errors
/// `InvalidInput` for the refusals the module names; those of the interpolation and the surface's
/// construction.
#[allow(clippy::too_many_lines)] // one construction, step by step
pub fn section_blend(
    sections: &[Vec<SectionFrame>],
    orders: [EndOrder; 2],
    crown: [f64; 2],
) -> Result<BSplineSurface> {
    let run = sections.len();
    if !(2..=MAX_SECTIONS).contains(&run) {
        return Err(invalid("a blend runs through 2 to 50 cross-sections"));
    }
    let samples = sections.first().map_or(0, Vec::len);
    if !(MIN_STATIONS..=MAX_STATIONS).contains(&samples) {
        return Err(invalid("a blend's cross-sections take 2 to 257 stations"));
    }
    if crown
        .iter()
        .any(|c| !(c.is_finite() && (-0.9..=3.0).contains(c)))
    {
        return Err(invalid("a blend's crown is between -0.9 and 3"));
    }
    for section in sections {
        if section.len() != samples {
            return Err(invalid(
                "every cross-section of a blend has the same stations",
            ));
        }
        if section
            .iter()
            .any(|f| !(finite(f.p) && finite(f.leave) && finite(f.bend) && finite(f.bend_change)))
        {
            return Err(invalid("a blend's frame is not finite"));
        }
    }
    let point = |i: usize, s: usize| -> Result<Point3> {
        sections
            .get(i)
            .and_then(|sec| sec.get(s))
            .map(|f| f.p)
            .ok_or_else(|| invalid("a cross-section is missing a station"))
    };
    let frame = |i: usize, s: usize| -> Result<SectionFrame> {
        sections
            .get(i)
            .and_then(|sec| sec.get(s))
            .copied()
            .ok_or_else(|| invalid("a cross-section is missing a station"))
    };
    for i in 0..run - 1 {
        for s in 0..samples {
            // the floor on coincident sections (a zero speed, no tangent to divide by next):
            // refused, never nudged apart
            if point(i, s)?.distance(point(i + 1, s)?) <= 0.0 {
                return Err(invalid(
                    "two neighbouring cross-sections meet at a station: there is no room for a blend",
                ));
            }
        }
    }
    // the conditions at each section: the ends' orders; an interior one four where a fourth
    // section reaches for the third difference, else three
    let mut conditions = vec![3usize; run];
    if let Some(c) = conditions.first_mut() {
        *c = orders[0].conditions();
    }
    if let Some(c) = conditions.last_mut() {
        *c = orders[1].conditions();
    }
    for (i, c) in conditions.iter_mut().enumerate().take(run - 1).skip(1) {
        if i >= 2 || i + 2 < run {
            *c = 4;
        }
    }
    let degree = conditions
        .windows(2)
        .map(|w| match w {
            [a, b] => a + b - 1,
            _ => 0,
        })
        .max()
        .unwrap_or(1)
        .max(1);
    #[allow(clippy::cast_precision_loss)] // at most 50 sections
    let step = 1.0 / (run - 1) as f64;

    // what each section holds along the run
    let mut held: Vec<Held> = Vec::new();
    for (i, &c) in conditions.iter().enumerate() {
        let p: Vec<Point3> = (0..samples).map(|s| point(i, s)).collect::<Result<_>>()?;
        let mut d: [Vec<Vec3>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let first = i == 0;
        let last = i + 1 == run;
        for s in 0..samples {
            let (d1, d2, d3) = if first || last {
                let f = frame(i, s)?;
                let (other, crown_here, sign) = if first {
                    (point(1, s)?, crown[0], 1.0)
                } else {
                    (point(i - 1, s)?, crown[1], -1.0)
                };
                let length = f.leave.length();
                // the floor on the tangent's length, divided by below: refused, never defaulted
                if c >= 2 && !(length.is_finite() && length > 0.0) {
                    return Err(invalid("a blend's leave direction has no length"));
                }
                let leave = if length > 0.0 {
                    f.leave * (1.0 / length)
                } else {
                    f.leave
                };
                // a third of the station's chord to its neighbour, the crown, over the interval
                let speed = f.p.distance(other) / 3.0 * (1.0 + crown_here) / step;
                let d1 = leave * (sign * speed);
                let d2 = f.bend * (speed * speed);
                let d3 = if c >= 4 {
                    // the last section's frames point back into the blend: read it backwards
                    if first {
                        third_derivative(d1, d2, f.bend_change)?
                    } else {
                        third_derivative(-d1, d2, f.bend_change)? * -1.0
                    }
                } else {
                    Vec3::ZERO
                };
                (d1, d2, d3)
            } else {
                let before = point(i - 1, s)?;
                let here = point(i, s)?;
                let after = point(i + 1, s)?;
                let d1 = (after - before) * (1.0 / (2.0 * step));
                let d2 = (after - here * 2.0 + before) * (1.0 / (step * step));
                let d3 = if c >= 4 {
                    let cube = step * step * step;
                    match (i.checked_sub(2), i + 2 < run) {
                        (Some(k), true) => {
                            (point(i + 2, s)? - point(i + 1, s)? * 2.0 + point(i - 1, s)? * 2.0
                                - point(k, s)?)
                                * (1.0 / (2.0 * cube))
                        }
                        (None, true) => {
                            (point(i + 2, s)? - point(i + 1, s)? * 3.0 + here * 3.0 - before)
                                * (1.0 / cube)
                        }
                        // (the backward stencil f(i+1) - 3 f(i) + 3 f(i-1) - f(i-2): SolidCraft's
                        // has its sign turned, see Deviations)
                        (Some(k), false) => {
                            (after - here * 3.0 + before * 3.0 - point(k, s)?) * (1.0 / cube)
                        }
                        (None, false) => {
                            return Err(invalid(
                                "no fourth section for an interior change of curvature",
                            ))
                        }
                    }
                } else {
                    Vec3::ZERO
                };
                (d1, d2, d3)
            };
            if !(finite(d1) && finite(d2) && finite(d3)) {
                return Err(invalid(
                    "a section's derivatives along the run are not finite",
                ));
            }
            let [a, b, e] = &mut d;
            a.push(d1);
            b.push(d2);
            e.push(d3);
        }
        held.push(Held { p, d });
    }

    // the spans' control rows, each elevated to the run's degree; a span's first row is the one
    // before's last
    let mut rows: Vec<Vec<Point3>> = Vec::new();
    for j in 0..run - 1 {
        let (Some(left), Some(right)) = (held.get(j), held.get(j + 1)) else {
            return Err(invalid("a missing section"));
        };
        let (Some(&kl), Some(&kr)) = (conditions.get(j), conditions.get(j + 1)) else {
            return Err(invalid("a missing section"));
        };
        let m = kl + kr - 1;
        let mut span = vec![vec![Point3::ZERO; samples]; m + 1];
        for s in 0..samples {
            let take = |h: &Held| -> Result<(Point3, Vec<Vec3>)> {
                let p = *h.p.get(s).ok_or_else(|| invalid("a missing station"))?;
                let d =
                    h.d.iter()
                        .map(|v| {
                            v.get(s)
                                .copied()
                                .ok_or_else(|| invalid("a missing station"))
                        })
                        .collect::<Result<Vec<_>>>()?;
                Ok((p, d))
            };
            let (pl, dl) = take(left)?;
            let (pr, dr) = take(right)?;
            for (k, q) in end_rows(pl, &dl, kl, m, step, false)?
                .into_iter()
                .enumerate()
            {
                if let Some(slot) = span.get_mut(k).and_then(|r| r.get_mut(s)) {
                    *slot = q;
                }
            }
            for (k, q) in end_rows(pr, &dr, kr, m, step, true)?
                .into_iter()
                .enumerate()
            {
                if let Some(slot) = span.get_mut(m - k).and_then(|r| r.get_mut(s)) {
                    *slot = q;
                }
            }
        }
        while span.len() < degree + 1 {
            span = elevate(&span);
        }
        rows.extend(span.into_iter().skip(usize::from(j > 0)));
    }

    // each row interpolated through its stations along v
    #[allow(clippy::cast_precision_loss)] // at most 257 stations
    let params: Vec<f64> = (0..samples)
        .map(|s| s as f64 / (samples - 1) as f64)
        .collect();
    let degree_v = 3.min(samples - 1);
    let knots_v = interp_knot_vector(&params, degree_v)?;
    let kv = KnotVector::from_slice(&knots_v)?;
    let count_u = rows.len();
    let mut net = vec![Point3::ZERO; count_u * samples];
    for (i, row) in rows.iter().enumerate() {
        let curve = interp_given_params(row, &params, &kv, degree_v)?;
        for (s, c) in curve.cpts().iter().take(samples).enumerate() {
            // (exact tests: NOW is the stored "no weight" mark of a polynomial row, assigned,
            // never computed, and 1.0 an assigned unit weight; any other weight is a weighted
            // point)
            let q = if c.w == NOW || c.w == 1.0 {
                Point3::new(c.x, c.y, c.z)
            } else {
                Point3::new(c.x / c.w, c.y / c.w, c.z / c.w)
            };
            if let Some(slot) = net.get_mut(i * samples + s) {
                *slot = q;
            }
        }
    }

    // the run's knots: clamped, every interior section `degree` times
    let mut u_knots = vec![0.0];
    let mut u_mults = vec![degree + 1];
    for i in 1..run - 1 {
        #[allow(clippy::cast_precision_loss)] // at most 50 sections
        u_knots.push(i as f64 * step);
        u_mults.push(degree);
    }
    u_knots.push(1.0);
    u_mults.push(degree + 1);
    let (v_knots, v_mults) = unique(&knots_v);
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: degree,
        v_degree: degree_v,
        control_points: &net,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &u_mults,
        v_multiplicities: &v_mults,
        u_knots: &u_knots,
        v_knots: &v_knots,
        knot_type: KnotType::Unspecified,
        weights: None,
        uv_domain: None,
    })
}

/// A knot vector as its distinct knots and their multiplicities.
fn unique(knots: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut values: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &u in knots {
        match (values.last(), mults.last_mut()) {
            (Some(&last), Some(m)) if last == u => *m += 1,
            _ => {
                values.push(u);
                mults.push(1);
            }
        }
    }
    (values, mults)
}

const OP_RECERTIFY: &str = "sweep::section_blend::SectionBlendDefinition::recertify";

/// The blend surface through `sections` ([`section_blend`]) as a surface keeping its
/// definition ([`SectionBlendDefinition`]): what a face of the blend carries, and what
/// [`Surface::procedural_blend`] reads back and re-certifies.
///
/// # Errors
///
/// Those of [`section_blend`].
pub fn section_blend_surface(
    sections: Vec<Vec<SectionFrame>>,
    orders: [EndOrder; 2],
    crown: [f64; 2],
) -> Result<Surface> {
    let (definition, net) = SectionBlendDefinition::create(sections, orders, crown)?;
    let mut surface = Surface::from(net);
    surface.set_section_blend_definition(definition);
    Ok(surface)
}

/// One motion a blend's net took after it was made: the arguments of the
/// [`BSplineSurface::transform`] it took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    /// The rotation and move.
    pub rotate_n_move: Axis2Placement,
    /// The scale applied first, about the origin.
    pub scale: Option<Vec3>,
}

/// A section blend's definition, kept on its face's surface (its net): the sections, the end
/// orders and crowns, and the motions the net took since it was made, in order. The net is
/// [`section_blend`] of them with each motion applied by [`BSplineSurface::transform`], bit for
/// bit (bound 0). A motion is kept, not applied to the sections: moving the sections and
/// building again rounds differently from moving the net, so only the replay is exact.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionBlendDefinition {
    sections: Vec<Vec<SectionFrame>>,
    orders: [EndOrder; 2],
    crown: [f64; 2],
    motions: Vec<Motion>,
}

impl SectionBlendDefinition {
    /// The blend through `sections` ([`section_blend`]) and its definition, no motion yet.
    ///
    /// # Errors
    ///
    /// Those of [`section_blend`].
    pub fn create(
        sections: Vec<Vec<SectionFrame>>,
        orders: [EndOrder; 2],
        crown: [f64; 2],
    ) -> Result<(Self, BSplineSurface)> {
        let net = section_blend(&sections, orders, crown)?;
        Ok((
            Self {
                sections,
                orders,
                crown,
                motions: Vec::new(),
            },
            net,
        ))
    }

    /// A definition from its parts, as a record holds them; not checked here: the rebuild
    /// ([`SectionBlendDefinition::rebuild`]) refuses what [`section_blend`] refuses, and
    /// [`SectionBlendDefinition::recertify`] a net that is not the rebuilt one.
    #[must_use]
    pub const fn from_parts(
        sections: Vec<Vec<SectionFrame>>,
        orders: [EndOrder; 2],
        crown: [f64; 2],
        motions: Vec<Motion>,
    ) -> Self {
        Self {
            sections,
            orders,
            crown,
            motions,
        }
    }

    /// The cross-sections, as the blend was made through them (before any motion).
    #[must_use]
    pub fn sections(&self) -> &[Vec<SectionFrame>] {
        &self.sections
    }

    /// The continuity held at the first and the last section.
    #[must_use]
    pub const fn orders(&self) -> [EndOrder; 2] {
        self.orders
    }

    /// The crowns at the first and the last section.
    #[must_use]
    pub const fn crown(&self) -> [f64; 2] {
        self.crown
    }

    /// The motions the net took since it was made, in order.
    #[must_use]
    pub fn motions(&self) -> &[Motion] {
        &self.motions
    }

    /// The net's certified distance from the definition: 0, the net is the rebuilt surface.
    #[must_use]
    pub const fn bound(&self) -> f64 {
        0.0
    }

    /// The net the definition gives: [`section_blend`] of the sections, then each motion.
    ///
    /// # Errors
    ///
    /// Those of [`section_blend`] and of the transforms.
    pub fn rebuild(&self) -> Result<BSplineSurface> {
        let mut net = section_blend(&self.sections, self.orders, self.crown)?;
        for m in &self.motions {
            net.transform(&m.rotate_n_move, m.scale)?;
        }
        Ok(net)
    }

    /// Re-certifies `net` against this definition: the net is a B-spline surface, and the
    /// rebuilt surface ([`SectionBlendDefinition::rebuild`]) is it exactly, the same degrees,
    /// knots, control points and weights bit for bit in some orientation of the parameter
    /// square (the swap and reversals a face's net takes). Returns the distance proved, 0.
    ///
    /// # Errors
    ///
    /// `InvalidInput` naming what fails; those of the rebuild.
    pub fn recertify(&self, net: &Surface) -> Result<f64> {
        let refuse = |why| SolidError::invalid_input(OP_RECERTIFY, why);
        let SurfaceKind::BSpline(form) = net.kind() else {
            return Err(refuse("a section blend's net is a B-spline surface"));
        };
        let rebuilt = self.rebuild()?;
        match crate::surface::procedural_blend::corner::net_distance(&rebuilt, form) {
            Some(d) if d <= 0.0 => Ok(0.0),
            _ => Err(refuse("a section blend's net is not its definition's")),
        }
    }

    /// Moves the definition with its net (`Transform`): the motion is kept, to be replayed on
    /// the rebuilt surface. Any motion the net takes is followed, a scale that is not uniform
    /// among them: the definition is the blend then its motions, whatever they are.
    pub fn transform(&mut self, rotate_n_move: &Axis2Placement, scale: Option<Vec3>) {
        self.motions.push(Motion {
            rotate_n_move: *rotate_n_move,
            scale,
        });
    }
}

const OP_FRAMES: &str = "sweep::section_blend::section_frames";

fn frames_invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP_FRAMES, reason)
}

/// How an edge chain's frames are read ([`section_frames`]): SolidCraft's conventions, the
/// caller's to pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameOptions {
    /// The stations along the chain, [`MIN_STATIONS`] ..= [`MAX_STATIONS`], at even arc length
    /// (SolidCraft's placement).
    pub stations: usize,
    /// The least length the support face's tangent plane keeps of the unit `leave`, `0 ..= 1`
    /// (SolidCraft's is 0.2): a face whose plane keeps less is not left through.
    pub min_in_plane: f64,
}

/// The frames of a cross-section that is an edge chain of a body: at each station, the chain's
/// point, and the support face's shape read along `leave` there (SolidCraft's `section_frames`,
/// with its rules for the face and the stations passed in as [`FrameOptions`]).
///
/// - The chain is `chain` in order: each edge shares a vertex with the next (the first is
///   turned to run into the second).
/// - The support face is the face bounding every chain edge whose tangent plane, at the first
///   edge's middle, keeps most of the unit `leave`; refused where the most is below
///   `min_in_plane`.
/// - The stations lie at even arc length along the chain (each edge's [`Curve::length`], and its
///   parameter at a length from [`Curve::find_parameter_at_arc_length`]): where the definition is
///   read, not a certified quantity.
/// - At a station `p`, its foot on the face's surface `(u, v)` (by projection), the unit normal
///   `n`, the leave direction `w`, `leave`'s part in the tangent plane made unit, and the
///   parameter direction `(du, dv)` with `S_u du + S_v dv = w`. The surface's own derivatives to
///   third order give the parameter line's exactly: `c' = S_u du + S_v dv`,
///   `c'' = S_uu du^2 + 2 S_uv du dv + S_vv dv^2`,
///   `c''' = S_uuu du^3 + 3 S_uuv du^2 dv + 3 S_uvv du dv^2 + S_vvv dv^3`; `bend` is the curvature
///   vector `(c'' - t (t . c'')) / |c'|^2`, `t` the unit tangent, and `bend_change` its arc-length
///   derivative from `c'''`, each on the spans the line runs into where the station sits on a
///   knot. SolidCraft reads `c'`, `c''`, `c'''` by five-point differences of
///   step a thousandth of the chain: these are those derivatives without the stencils'
///   truncation.
///
/// # Errors
/// `InvalidInput` for stations or `min_in_plane` out of range, a `leave` not finite or of no
/// length, a chain that is empty or does not join end to start, an edge without a curve, no face
/// bounding the whole chain or none keeping `min_in_plane` of `leave`, a `leave` normal to the
/// face at a station, or a surface singular there; those of the lengths, projections and
/// evaluations.
pub fn section_frames(
    k: &crate::kernel::Kernel,
    b: &crate::topology::Brep,
    chain: &[crate::topology::EdgeId],
    leave: Vec3,
    opts: &FrameOptions,
) -> Result<Vec<SectionFrame>> {
    use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    use crate::topology::{edge, edgeuse::p1 as eu};
    if !(MIN_STATIONS..=MAX_STATIONS).contains(&opts.stations) {
        return Err(frames_invalid("a section's frames take 2 to 257 stations"));
    }
    if !(opts.min_in_plane.is_finite() && (0.0..=1.0).contains(&opts.min_in_plane)) {
        return Err(frames_invalid(
            "the least in-plane share is between 0 and 1",
        ));
    }
    if !finite(leave) {
        return Err(frames_invalid("a leave direction that is not finite"));
    }
    let leave = leave
        .unitize()
        .map_err(|_| frames_invalid("a leave direction of no length"))?;
    if chain.is_empty() {
        return Err(frames_invalid("an empty chain"));
    }
    // each edge's run: its curve, interval and whether it runs backwards in the chain
    let ends = |e| -> Result<(
        Option<crate::topology::VertexId>,
        Option<crate::topology::VertexId>,
    )> { Ok((edge::start_vertex(b, e)?, edge::end_vertex(b, e)?)) };
    let mut runs = Vec::new();
    let mut at: Option<crate::topology::VertexId> = None;
    for (i, &e) in chain.iter().enumerate() {
        let (s, t) = ends(e)?;
        let backwards = match (i, at) {
            // the first runs into the second
            (0, _) => match chain.get(1) {
                Some(&next) => {
                    let (ns, nt) = ends(next)?;
                    !(t.is_some() && (t == ns || t == nt))
                }
                None => false,
            },
            (_, Some(v)) if s == Some(v) => false,
            (_, Some(v)) if t == Some(v) => true,
            _ => {
                return Err(frames_invalid(
                    "a chain whose edges do not join end to start",
                ))
            }
        };
        at = if backwards { s } else { t };
        let curve = edge::evaluation_curve(b, e)?
            .ok_or_else(|| frames_invalid("a chain edge without a curve"))?;
        let ivl = edge::interval(b, e)?;
        runs.push((e, curve, ivl, backwards));
    }
    // the support face: every chain edge bounds it, and its plane keeps most of `leave`
    let faces_of = |e| -> Result<Vec<crate::topology::FaceId>> {
        let mut out = Vec::new();
        for u in edge::edgeuses(b, e, None, None)? {
            if let Some(f) = eu::face(b, u)? {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        Ok(out)
    };
    let (first_edge, first_curve, first_ivl, _) = runs
        .first()
        .ok_or_else(|| frames_invalid("an empty chain"))?;
    let mid = first_curve.evaluate_point(k, first_ivl.evaluate(0.5))?;
    let mut candidates = faces_of(*first_edge)?;
    for &(e, _, _, _) in runs.iter().skip(1) {
        let theirs = faces_of(e)?;
        candidates.retain(|f| theirs.contains(f));
    }
    let mut best: Option<(f64, crate::topology::FaceId)> = None;
    for f in candidates {
        let Some(surf) = crate::topology::face::p2::surface(b, f)? else {
            continue;
        };
        let Ok(foot) = crate::query::api_queries::surface_closest_point(k, surf, mid) else {
            continue;
        };
        let Some(&uv) = foot.uvs.first() else {
            continue;
        };
        let d = surf.evaluate(
            k,
            uv,
            1,
            1,
            EvaluateFlags {
                non_zero_tangents: false,
                ..EvaluateFlags::default()
            },
        )?;
        let (Some(su), Some(sv)) = (
            d.get(1).and_then(|r| r.first()),
            d.first().and_then(|r| r.get(1)),
        ) else {
            continue;
        };
        let Ok(n) = su.cross(*sv).unitize() else {
            continue;
        };
        let keeps = (leave - n * leave.dot(n)).length();
        if best.is_none_or(|(s, _)| keeps > s) {
            best = Some((keeps, f));
        }
    }
    let (keeps, face) = best.ok_or_else(|| frames_invalid("no face bounds the whole chain"))?;
    if keeps < opts.min_in_plane {
        return Err(frames_invalid(
            "the leave direction lies in no face at the chain: its tangent plane keeps less than the least asked",
        ));
    }
    let surf = crate::topology::face::p2::surface(b, face)?
        .ok_or_else(|| frames_invalid("the support face has no surface"))?;
    // the stations at even arc length
    let mut lengths = Vec::new();
    for (_, curve, ivl, _) in &runs {
        let chord = curve
            .evaluate_point(k, ivl.min())?
            .distance(curve.evaluate_point(k, ivl.max())?);
        let len = curve.length(k, ivl, 1e-9 * (1.0 + chord))?;
        if !(len.is_finite() && len >= 0.0) {
            return Err(frames_invalid("a chain edge whose length is not finite"));
        }
        lengths.push(len);
    }
    let total: f64 = lengths.iter().sum();
    if !(total.is_finite() && total > 0.0) {
        return Err(frames_invalid("a chain of no length"));
    }
    let mut out = Vec::new();
    #[allow(clippy::cast_precision_loss)] // at most MAX_STATIONS
    let last = (opts.stations - 1) as f64;
    for i in 0..opts.stations {
        #[allow(clippy::cast_precision_loss)] // at most MAX_STATIONS
        let s = total * (i as f64) / last;
        // the run the station is on, and how far along it
        let mut left = s;
        let mut pick = 0;
        for (j, &l) in lengths.iter().enumerate() {
            pick = j;
            if left <= l || j + 1 == lengths.len() {
                break;
            }
            left -= l;
        }
        let ((_, curve, ivl, backwards), &len) = (
            runs.get(pick)
                .ok_or_else(|| frames_invalid("a chain's run"))?,
            lengths
                .get(pick)
                .ok_or_else(|| frames_invalid("a chain's run"))?,
        );
        // `clamp(0.0, len)` without its panic
        let left = if left < 0.0 {
            0.0
        } else if left > len {
            len
        } else {
            left
        };
        let t = if *backwards {
            if left >= len {
                ivl.min()
            } else {
                curve
                    .find_parameter_at_arc_length(k, ivl.max(), -left)?
                    .end_param
            }
        } else if left >= len {
            ivl.max()
        } else {
            curve
                .find_parameter_at_arc_length(k, ivl.min(), left)?
                .end_param
        };
        // `clamp(ivl.min(), ivl.max())` without its panic on a reversed or NaN interval
        let t = if t < ivl.min() {
            ivl.min()
        } else if t > ivl.max() {
            ivl.max()
        } else {
            t
        };
        let p = curve.evaluate_point(k, t)?;
        out.push(frame_at(k, surf, p, leave)?);
    }
    Ok(out)
}

/// The frame at `p` on `surf` leaving along `leave`'s part in the tangent plane
/// ([`section_frames`]).
fn frame_at(
    k: &crate::kernel::Kernel,
    surf: &Surface,
    p: Point3,
    leave: Vec3,
) -> Result<SectionFrame> {
    use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    let foot = crate::query::api_queries::surface_closest_point(k, surf, p)?;
    let uv = *foot
        .uvs
        .first()
        .ok_or_else(|| frames_invalid("a station with no foot on the support face"))?;
    let d = surf.evaluate(
        k,
        uv,
        3,
        3,
        EvaluateFlags {
            non_zero_tangents: false,
            ..EvaluateFlags::default()
        },
    )?;
    let at = |i: usize, j: usize| -> Result<Vec3> {
        d.get(i)
            .and_then(|r| r.get(j))
            .copied()
            .ok_or_else(|| frames_invalid("a surface's derivatives"))
    };
    let (su, sv) = (at(1, 0)?, at(0, 1)?);
    let n = su
        .cross(sv)
        .unitize()
        .map_err(|_| frames_invalid("a support surface singular at a station"))?;
    let w = (leave - n * leave.dot(n))
        .unitize()
        .map_err(|_| frames_invalid("a leave direction normal to the support face at a station"))?;
    // S_u du + S_v dv = w, by its normal equations
    let (a, bb, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let det = a * c - bb * bb;
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(det > 0.0) {
        return Err(frames_invalid("a support surface singular at a station"));
    }
    let du = (c * w.dot(su) - bb * w.dot(sv)) / det;
    let dv = (a * w.dot(sv) - bb * w.dot(su)) / det;
    // the higher derivatives on the spans the line runs into: at a knot (a rational circle's
    // quarter) a surface's second and third derivatives differ from one span to the next, and
    // the line leaves along `w`, as SolidCraft's samples read it
    let side = |d: f64| {
        if d < 0.0 {
            crate::nurbs::knots::Side::Right
        } else {
            crate::nurbs::knots::Side::Left
        }
    };
    let sided = surf.evaluate(
        k,
        uv,
        3,
        3,
        EvaluateFlags {
            u_side: side(du),
            v_side: side(dv),
            non_zero_tangents: false,
            ..EvaluateFlags::default()
        },
    )?;
    let at = |i: usize, j: usize| -> Result<Vec3> {
        sided
            .get(i)
            .and_then(|r| r.get(j))
            .copied()
            .ok_or_else(|| frames_invalid("a surface's derivatives"))
    };
    let c1 = at(1, 0)? * du + at(0, 1)? * dv;
    let c2 = at(2, 0)? * (du * du) + at(1, 1)? * (2.0 * du * dv) + at(0, 2)? * (dv * dv);
    let c3 = at(3, 0)? * (du * du * du)
        + at(2, 1)? * (3.0 * du * du * dv)
        + at(1, 2)? * (3.0 * du * dv * dv)
        + at(0, 3)? * (dv * dv * dv);
    let (bend, bend_change) = shape(c1, c2, c3)?;
    if !(finite(bend) && finite(bend_change)) {
        return Err(frames_invalid("a station's shape is not finite"));
    }
    Ok(SectionFrame {
        p,
        leave: w,
        bend,
        bend_change,
    })
}

/// The curvature vector of a curve with derivatives `c1`, `c2`, `c3`, and its arc-length
/// derivative: with `sigma = |c1|` and `t = c1 / sigma`, `kappa = (c2 - t (t . c2)) / sigma^2`,
/// and `d kappa / ds = (d kappa / d tau) / sigma`, differentiating each factor (`sigma' = t . c2`,
/// `t' = (c2 - t (t . c2)) / sigma`).
fn shape(c1: Vec3, c2: Vec3, c3: Vec3) -> Result<(Vec3, Vec3)> {
    let sigma = c1.length();
    // the floor on the tangent's length, divided by below: refused, never adjusted
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(sigma.is_finite() && sigma > 0.0) {
        return Err(frames_invalid(
            "a parameter line that does not move at a station",
        ));
    }
    let t = c1 * (1.0 / sigma);
    let a2 = t.dot(c2);
    let across = c2 - t * a2;
    let kappa = across * (1.0 / (sigma * sigma));
    let dt = across * (1.0 / sigma);
    let da2 = dt.dot(c2) + t.dot(c3);
    let dsigma = a2;
    let dacross = c3 - dt * a2 - t * da2;
    let dkappa =
        dacross * (1.0 / (sigma * sigma)) - across * (2.0 * dsigma / (sigma * sigma * sigma));
    Ok((kappa, dkappa * (1.0 / sigma)))
}
