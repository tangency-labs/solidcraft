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
//! | [`end_rows`]       | (none; the rows of SolidCraft's `bridge::section`) |
//!
//! **The surface.** The sections run along u, each sampled at the same stations along v. Section
//! `i` sits at `u = i / (n - 1)` and the surface passes through every station of it: each
//! control row is interpolated through its stations along v (degree `min(3, stations - 1)`, the
//! kernel's global interpolation at uniform parameters with averaged knots). Along u the knots are
//! clamped and repeated `degree` times at every interior section, so every span is one Bezier.
//! A span's end rows are the Hermite rows ([`end_rows`]) of its two sections: position, then
//! tangent, curvature and change of curvature as the section holds them, written as the Bezier's
//! leading control rows at the span's parameter interval. The u degree is the largest
//! `c + c' - 1` over the spans (`c`, `c'` the conditions at its two ends); a span with fewer is
//! degree-elevated exactly.
//!
//! **What each section holds (the definition, SolidCraft's).**
//! - The first and the last section hold what their [`EndOrder`] asks (G0 to G3) from the
//!   caller's frames: the tangent `leave * speed`, the curvature `bend * speed^2` and the change
//!   of curvature from `bend_change` ([`third_derivative`]: only its part square to the tangent
//!   holds the condition, the rest a reparametrisation), `speed` a third of the station's chord to
//!   the neighbouring section times `1 + crown`, over the span's interval. The last section's
//!   frames point back into the blend, so its odd derivatives turn round.
//! - An interior section holds its position and the run's derivatives from finite differences of
//!   the neighbouring sections' stations: the central first and second differences, and a third
//!   (central, or one-sided at the second and the last-but-one section) where four sections
//!   reach. So the joint is C3 in u with four or more sections and C2 with three.
//!
//! This is SolidCraft's definition of the blend, not an approximation of another surface: the
//! finite differences are the conditions the surface is built to, not estimates of a curve the
//! sections lie on, and no error against anything is claimed. What is exact: the surface passes
//! through every station; at the ends it holds the frames' conditions to rounding; at an interior
//! section the spans on either side agree in their u derivatives up to the order held.
//!
//! **Refused**, naming why: fewer than 2 or more than [`MAX_SECTIONS`] sections; sections of
//! different station counts, or a count outside [`MIN_STATIONS`] ..= [`MAX_STATIONS`]; a frame
//! that is not finite, or a `leave` of no length at an end that uses it; a crown outside
//! `-0.9 ..= 3`; two neighbouring sections that meet at a station (no room for a blend there).
//!
//! Deviations: no upstream counterpart. SolidCraft's `1e-12` floor on a tangent's length and its
//! `1e-9` floor on the distance between neighbouring sections are exact tests here (a length that
//! is not positive); its batched row solves are one solve per row. No recursion.

use crate::error::{Result, SolidError};
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::KnotType;
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use crate::nurbs::crv_fit::p1::{interp_given_params, interp_knot_vector};
use crate::nurbs::knots::KnotVector;
use crate::nurbs::math::NOW;

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
/// the end. Shared with a bridge's cross-sections.
///
/// # Errors
/// `InvalidInput` for more conditions than the degree carries or than `d` gives.
pub fn end_rows(
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
pub fn third_derivative(d1: Vec3, d2: Vec3, target: Vec3) -> Result<Vec3> {
    let sigma = d1.length();
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
    let mut out: Vec<Vec<Point3>> = Vec::with_capacity(rows.len() + 1);
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
    let mut held: Vec<Held> = Vec::with_capacity(run);
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
                        (Some(k), false) => {
                            (point(k, s)? - before * 3.0 + here * 3.0 - after) * (1.0 / cube)
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
