//! Exact support extents, contact strips and the outline of a fabrication gusset.
//!
//! Not upstream: extracted from SolidCraft's attached gusset operation. Planar area
//! uses the existing certified profile integral; curved extents retain the bounded
//! positive-weight subdivision. The operation layer performs planar booleans.
//!
//! References: Farin, Curves and Surfaces for CAGD, sections 5.2 and 12.4;
//! Piegl and Tiller, The NURBS Book, sections 1.4 and 7.3.
//! Deviations: no upstream counterpart. Extent recursion is bounded to 24 halvings.

use super::profile::{self, Loop2, Region2, Seg2};
use crate::error::{Result, SolidError};
use crate::eval::Frame;
use crate::math::det;
use crate::math::{Transform, Vec2, Vec3};

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("prism::gusset", reason)
}

/// Evaluated dimensions in millimetres.
#[derive(Clone, Copy, Debug)]
pub struct Dimensions {
    /// Length on the first support.
    pub leg1: f64,
    /// Length on the second support.
    pub leg2: f64,
    /// Plate thickness.
    pub thickness: f64,
    /// Inner corner relief.
    pub relief: f64,
    /// Outer tip clipping.
    pub clip: f64,
    /// Signed offset along the joint.
    pub offset: f64,
}

/// The plate's side of the joint plane.
#[derive(Clone, Copy, Debug)]
pub enum Placement {
    /// Half the thickness on either side.
    Center,
    /// All thickness on the positive side.
    Positive,
    /// All thickness on the negative side.
    Negative,
}

/// Exact material in the source face frame, and its placement in the author frame.
#[derive(Clone, Debug)]
pub struct Face {
    /// Support plane in author coordinates.
    pub plane: Frame,
    /// Frame of the face regions.
    pub source: Frame,
    /// Trimmed material, including holes.
    pub regions: Vec<Region2>,
    /// Author coordinates to the source body.
    pub to_source: Transform,
    /// Source body coordinates to the author.
    pub to_author: Transform,
    /// Planar boolean tolerance in millimetres.
    pub tolerance: f64,
}

/// A contact strip in one support's region coordinates.
pub struct Contact {
    /// The strip to subtract the support material from.
    pub region: Region2,
    /// Maximum uncovered area, including the existing numerical allowance.
    pub maximum_uncovered: f64,
}

/// Exact profile and extrusion levels, with the two required contacts.
pub struct Plan {
    /// The profile frame in author coordinates.
    pub plane: Frame,
    /// Plate outline.
    pub profile: Region2,
    /// Lower extrusion level.
    pub start: f64,
    /// Upper extrusion level.
    pub end: f64,
    /// Strips that must be covered by the two supports.
    pub contacts: [Contact; 2],
}

/// Check that split support faces share the first face's plane.
pub fn same_plane(first: &Frame, other: &Frame, size: f64, tolerance: f64) -> Result<()> {
    let (o, _, _, n) = profile::axes(first);
    let (other_o, _, _, other_n) = profile::axes(other);
    let tilt = other_n.cross(n).length() * size.max(1.0);
    if !tilt.is_finite() || tilt > tolerance || (other_o - o).dot(n).abs() > tolerance {
        return Err(invalid("gusset support's faces don't lie in one plane"));
    }
    Ok(())
}

/// Project a saved support pick onto the current exact support plane.
pub fn support_pick(plane: &Frame, at: Vec3) -> Vec3 {
    let (origin, _, _, normal) = profile::axes(plane);
    crate::geom::geom_utility::p1::point_project_to_plane(at, origin, normal)
}

/// Signed area from the kernel's existing certified Green integral.
pub fn loop_signed_area(lp: &Loop2) -> Result<f64> {
    lp.signed_area().map(|area| area.value)
}

/// Orient exact segments without replacing any curved boundary.
pub fn counter_clockwise(lp: Loop2) -> Result<Loop2> {
    Ok(if loop_signed_area(&lp)? >= 0.0 {
        lp
    } else {
        lp.reversed()
    })
}

/// Total material area with holes removed, preserving the operation's summation order.
pub fn regions_area(regions: &[Region2]) -> Result<f64> {
    let mut total = 0.0;
    for region in regions {
        let mut area = loop_signed_area(&region.outer)?.abs();
        for hole in &region.holes {
            area -= loop_signed_area(hole)?.abs();
        }
        total += area;
    }
    Ok(total)
}

/// Join the pieces of one trimmed loop in the recorded traversal order.
pub fn chain(segs: Vec<Seg2>, tolerance: f64) -> Result<Loop2> {
    let close = 10.0 * tolerance;
    let mut iter = segs.into_iter();
    let first = iter
        .next()
        .ok_or_else(|| invalid("gusset support face has an empty loop"))?;
    let mut segs: Vec<_> = iter.collect();
    let mut out = vec![first];
    while !segs.is_empty() {
        let end = out
            .last()
            .map(Seg2::end)
            .ok_or_else(|| invalid("gusset support face has an empty loop"))?;
        let next = segs
            .iter()
            .position(|s| s.start().distance(end) <= close || s.end().distance(end) <= close);
        let i = next.ok_or_else(|| invalid("gusset support face has an open loop"))?;
        // Preserve swap_remove's traversal order, using checked accesses only.
        let s = *segs
            .get(i)
            .ok_or_else(|| invalid("gusset support face has an open loop"))?;
        let last = segs
            .pop()
            .ok_or_else(|| invalid("gusset support face has an open loop"))?;
        if let Some(slot) = segs.get_mut(i) {
            *slot = last;
        }
        out.push(if s.start().distance(end) <= close {
            s
        } else {
            s.reversed()
        });
    }
    if out
        .first()
        .zip(out.last())
        .is_none_or(|(a, b)| a.start().distance(b.end()) > close)
    {
        return Err(invalid("gusset support face has an open loop"));
    }
    Ok(Loop2 { segs: out })
}

fn finite2(p: Vec2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}
fn polar(t: f64) -> Vec2 {
    Vec2::new(det::cos(t), det::sin(t))
}
fn angle(p: Vec2) -> f64 {
    det::atan2(p.y, p.x)
}
fn seg_ends(s: &Seg2) -> (Vec2, Vec2) {
    (s.start(), s.end())
}
/// A Bézier-form piece's control points with their weights (1 for a polynomial piece). A
/// rational piece needs positive weights: then it lies in its control points' convex hull, which
/// bounds its extent; any other weight is refused.
fn bezier_form(s: &Seg2) -> Result<Option<Vec<(Vec2, f64)>>> {
    let control = match s {
        Seg2::Cubic { p0, p1, p2, p3 } => [*p0, *p1, *p2, *p3].map(|p| (p, 1.)).to_vec(),
        Seg2::Conic { a, apex, b, w } => vec![(*a, 1.), (*apex, *w), (*b, 1.)],
        Seg2::Bezier { n, p } => {
            if usize::from(*n) > profile::MAX_BEZIER_DEGREE {
                return Err(invalid("gusset support edge's Bézier degree is past 9"));
            }
            p.iter()
                .take(usize::from(*n) + 1)
                .map(|p| (*p, 1.))
                .collect()
        }
        Seg2::Line { .. } | Seg2::Arc { .. } => return Ok(None),
    };
    if control
        .iter()
        .any(|(p, w)| !finite2(*p) || !w.is_finite() || *w <= 0.)
    {
        return Err(invalid(
            "gusset support edge has a non-finite point or a weight that isn't positive",
        ));
    }
    Ok(Some(control))
}

/// How closely a Bézier piece's extent is resolved, mm: well inside the contact check's
/// tolerance, so the joint's placement is never off by an unresolved bulge.
const EXTENT_TOLERANCE: f64 = 1e-9;

/// The least and greatest of `e · p` over the exact piece `s`: a line's at its ends, an arc's
/// also where its tangent is perpendicular to `e`, a Bézier piece's from [`scalar_extent`].
pub fn seg_extent(s: &Seg2, e: Vec2) -> Result<(f64, f64)> {
    let (a, b) = seg_ends(s);
    let (mut lo, mut hi) = (a.dot(e).min(b.dot(e)), a.dot(e).max(b.dot(e)));
    match s {
        Seg2::Line { .. } => {}
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let (t0, t1) = if *sweep >= 0. {
                (*start, start + sweep)
            } else {
                (start + sweep, *start)
            };
            for extreme in [angle(e), angle(e) + std::f64::consts::PI] {
                let turns = ((t0 - extreme) / std::f64::consts::TAU).ceil();
                if extreme + turns * std::f64::consts::TAU <= t1 {
                    let v = (*center + polar(extreme) * *radius).dot(e);
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => {
            let control = bezier_form(s)?.unwrap_or_default();
            let (l, h) = scalar_extent(
                control.iter().map(|(p, w)| (p.dot(e) * w, *w)).collect(),
                24,
            )?;
            lo = lo.min(l);
            hi = hi.max(h);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Err(invalid("gusset support edge can't be measured"));
    }
    Ok((lo, hi))
}

/// The extent of a 1D rational Bézier with homogeneous control values `(w·v, w)`, every `w > 0`
/// ([`bezier_form`]). With positive weights the curve lies within its control values, and
/// within its end values once no control value is past them; halving narrows the gap. Each
/// half is accepted when its control values are within [`EXTENT_TOLERANCE`] of its end values,
/// so the result is the true extent to within that; a piece not resolved in `depth` halvings
/// is an error, never its unrefined hull.
pub(crate) fn scalar_extent(h: Vec<(f64, f64)>, depth: u32) -> Result<(f64, f64)> {
    if h.iter()
        .any(|(v, w)| !v.is_finite() || !w.is_finite() || *w <= 0.)
    {
        return Err(invalid(
            "gusset support edge has a weight that isn't positive",
        ));
    }
    let value = |p: &(f64, f64)| p.0 / p.1;
    let (lo, hi) = h
        .iter()
        .map(value)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, u), v| {
            (l.min(v), u.max(v))
        });
    let (first, last) = (h.first().map(value), h.last().map(value));
    let (Some(first), Some(last)) = (first, last) else {
        return Err(invalid("gusset support edge has an empty Bézier piece"));
    };
    let (elo, ehi) = (first.min(last), first.max(last));
    if lo >= elo - EXTENT_TOLERANCE && hi <= ehi + EXTENT_TOLERANCE {
        return Ok((
            elo.min(lo.max(elo - EXTENT_TOLERANCE)),
            ehi.max(hi.min(ehi + EXTENT_TOLERANCE)),
        ));
    }
    let Some(depth) = depth.checked_sub(1) else {
        return Err(invalid("gusset support edge's extent couldn't be resolved"));
    };
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut level = h;
    while let (Some(f), Some(l)) = (level.first().copied(), level.last().copied()) {
        left.push(f);
        right.push(l);
        level = level
            .windows(2)
            .filter_map(|w| {
                Some((
                    (w.first()?.0 + w.get(1)?.0) * 0.5,
                    (w.first()?.1 + w.get(1)?.1) * 0.5,
                ))
            })
            .collect();
    }
    right.reverse();
    let (l1, h1) = scalar_extent(left, depth)?;
    let (l2, h2) = scalar_extent(right, depth)?;
    Ok((l1.min(l2), h1.max(h2)))
}

impl Face {
    fn along(&self, d: Vec3) -> (Vec2, f64) {
        let ds = self.to_source.transform_vector(d);
        let (origin, x, y, _) = profile::axes(&self.source);
        let k = self.to_author.transform_point(origin).dot(d);
        (Vec2::new(x.dot(ds), y.dot(ds)), k)
    }
    fn interval(&self, d: Vec3) -> Result<(f64, f64)> {
        let (e, k) = self.along(d);
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for s in self
            .regions
            .iter()
            .flat_map(Region2::loops)
            .flat_map(|l| &l.segs)
        {
            let (a, b) = seg_extent(s, e)?;
            lo = lo.min(a);
            hi = hi.max(b);
        }
        if !lo.is_finite() || !hi.is_finite() {
            return Err(invalid("gusset support face has no measurable extent"));
        }
        Ok((lo + k, hi + k))
    }
}

fn polygon(points: &[Vec2]) -> Loop2 {
    Loop2 {
        segs: points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| Seg2::Line { a: *a, b: *b })
            .collect(),
    }
}

fn contact(
    face: &Face,
    origin: Vec3,
    leg: Vec3,
    direction: Vec3,
    along: [f64; 2],
    levels: [f64; 2],
) -> Result<Contact> {
    let [from, to] = along;
    let [start, end] = levels;
    if to <= from {
        return Err(invalid(
            "gusset contact was consumed by the relief/clipping",
        ));
    }
    let to_uv = |p| profile::to_local(&face.source, face.to_source.transform_point(p));
    let points = [
        to_uv(origin + leg * from + direction * start),
        to_uv(origin + leg * to + direction * start),
        to_uv(origin + leg * to + direction * end),
        to_uv(origin + leg * from + direction * end),
    ];
    Ok(Contact {
        region: Region2 {
            outer: counter_clockwise(polygon(&points))?,
            holes: Vec::new(),
        },
        maximum_uncovered: (to - from) * (end - start) * 1e-6 + 1e-8,
    })
}

fn unit(v: Vec3) -> Option<Vec3> {
    let length = v.length();
    (length > 1e-9 && length.is_finite()).then(|| v / length)
}

/// Construct the profile and exact contact strips; ops checks each strip's remaining area.
pub fn plan([a, b]: [&Face; 2], d: Dimensions, placement: Placement) -> Result<Plan> {
    let Dimensions {
        leg1: l1,
        leg2: l2,
        thickness: t,
        relief: r,
        clip: c,
        offset,
    } = d;
    for (value, positive) in [
        (l1, true),
        (l2, true),
        (t, true),
        (r, false),
        (c, false),
        (offset, false),
    ] {
        if !value.is_finite() || value.abs() > 1e6 || (positive && value < 0.01) {
            return Err(invalid(
                "gusset sizes must be 0.01…1e6 mm; offsets must be finite and within 1e6 mm",
            ));
        }
    }
    if r < 0.0 || c < 0.0 || r + c >= l1 || r + c >= l2 {
        return Err(invalid(
            "relief and clipping must be nonnegative and together shorter than each leg",
        ));
    }
    let (ao, _, _, n1) = profile::axes(&a.plane);
    let (bo, _, _, n2) = profile::axes(&b.plane);
    let dot = n1.dot(n2);
    let denom = 1.0 - dot * dot;
    if !denom.is_finite() || denom < 1e-8 {
        return Err(invalid(
            "gusset supports must not be parallel or nearly parallel",
        ));
    }
    let direction = unit(n1.cross(n2)).ok_or_else(|| invalid("gusset joint has no direction"))?;
    let u = unit(n2 - n1 * dot).ok_or_else(|| invalid("gusset first leg is degenerate"))?;
    let v = unit(n1 - n2 * dot).ok_or_else(|| invalid("gusset second leg is degenerate"))?;
    let (h1, h2) = (n1.dot(ao), n2.dot(bo));
    let mut origin = n1 * ((h1 - dot * h2) / denom) + n2 * ((h2 - dot * h1) / denom);
    let (ia, ib) = (a.interval(direction)?, b.interval(direction)?);
    let (lo, hi) = (ia.0.max(ib.0), ia.1.min(ib.1));
    if hi - lo < 0.01 {
        return Err(invalid("support faces have no common joint span"));
    }
    origin += direction * ((lo + hi) * 0.5 - origin.dot(direction) + offset);
    if ![origin.x, origin.y, origin.z].iter().all(|v| v.is_finite()) || origin.length() > 1e9 {
        return Err(invalid("gusset joint exceeds the coordinate limit"));
    }
    let (start, end) = match placement {
        Placement::Center => (-t / 2.0, t / 2.0),
        Placement::Positive => (0.0, t),
        Placement::Negative => (-t, 0.0),
    };
    let contacts = [
        contact(a, origin, u, direction, [r, l1 - c], [start, end])?,
        contact(b, origin, v, direction, [r, l2 - c], [start, end])?,
    ];
    let x = unit(u).ok_or_else(|| invalid("gusset plane is degenerate"))?;
    let n =
        unit(x.cross(direction.cross(u))).ok_or_else(|| invalid("gusset plane is degenerate"))?;
    let y = unit(n.cross(x)).ok_or_else(|| invalid("gusset plane is degenerate"))?;
    let plane = Frame {
        origin: [origin.x, origin.y, origin.z],
        x_axis: [x.x, x.y, x.z],
        y_axis: [y.x, y.y, y.z],
    };
    let (p1, p2) = (u * l1, v * l2);
    let diagonal = p2 - p1;
    let distance = diagonal.length();
    if distance <= 2.0 * c {
        return Err(invalid("outer clipping consumes the gusset diagonal"));
    }
    let mut points = vec![u * r];
    if c > 0.0 {
        points.extend([
            p1 - u * c,
            p1 + diagonal * (c / distance),
            p2 - diagonal * (c / distance),
            p2 - v * c,
        ]);
    } else {
        points.extend([p1, p2]);
    }
    if r > 0.0 {
        points.push(v * r);
    }
    let mut points: Vec<_> = points
        .into_iter()
        .map(|p| Vec2::new(p.dot(x), p.dot(y)))
        .collect();
    points.reverse();
    let outer = counter_clockwise(polygon(&points))?;
    if loop_signed_area(&outer)? < 1e-6 {
        return Err(invalid("gusset profile has no usable area"));
    }
    Ok(Plan {
        plane,
        profile: Region2 {
            outer,
            holes: Vec::new(),
        },
        start,
        end,
        contacts,
    })
}

#[cfg(test)]
pub(crate) fn conic_share(w: f64) -> Result<f64> {
    profile::conic_factor(w)
        .map(|(value, _)| value)
        .ok_or_else(|| invalid("gusset support edge has a conic weight that isn't positive"))
}
