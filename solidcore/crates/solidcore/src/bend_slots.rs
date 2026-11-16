//! Exact, centred slots and open end reliefs along straight planar bend spans.
//!
//! This primitive knows the current 2D material and bend endpoints, never a CAD history or
//! a chosen world axis. Lines and circular arcs remain analytic. The caller keeps its sheet's
//! folded state and applies the returned cuts to its own retained material.
//!
//! Not upstream. Deviations: no recursion; at most 64 spans and 512 cut loops per plan.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing range tests

use std::f64::consts::{FRAC_PI_2, PI};

use crate::math::{Point2, Vec2};
use crate::prism::placement::LoopPlacement;
use crate::prism::profile::{Loop2, Region2, Seg2};

/// Largest number of bend spans one plan accepts.
pub const MAX_BENDS: usize = 64;
/// Largest number of exact cut loops one plan returns.
pub const MAX_CUTS: usize = 512;
/// Largest input profile segment or loop count.
pub const MAX_PROFILE_SEGMENTS: usize = 4096;
/// Smallest positive slot, bridge or relief dimension, in millimetres.
pub const MIN_SIZE: f64 = 1e-3;
const MAX_COORD: f64 = 1e8;
const TOL: f64 = 1e-7;

/// One current, uninterrupted straight bend centre line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendSpan {
    /// Current centre-line start.
    pub start: Point2,
    /// Current centre-line end.
    pub end: Point2,
}

/// Dimensions in millimetres. Slot length includes both end caps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendSlotOptions {
    /// Overall slot length, including end caps.
    pub slot_length: f64,
    /// Slot height and end-relief throat height.
    pub slot_width: f64,
    /// Material left between neighboring slots.
    pub bridge: f64,
    /// Minimum material between a slot and the inward end of an end relief.
    pub end_margin: f64,
    /// Depth to the closed throat's end-cap centre, or its rectangular end.
    pub relief_depth: f64,
    /// Entry-quarter-circle radius, independent of the throat's end-cap radius.
    pub corner_radius: f64,
    /// Semicircular slot and relief ends; false gives rectangular ends.
    pub rounded: bool,
    /// Include open reliefs at both bend ends.
    pub reliefs: bool,
}

/// The integer pattern chosen for one input bend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendSlotRow {
    /// Number of slots in this bend.
    pub count: usize,
    /// Current bend length.
    pub span: f64,
    /// First slot's start, along the bend from its start.
    pub start: f64,
    /// Material between the first slot and the first relief's inward reach.
    pub end_bridge: f64,
}

/// Exact cuts, with slots and open edge reliefs retained separately for verification.
#[derive(Clone, Debug, PartialEq)]
pub struct BendSlotPlan {
    /// Closed internal slot profiles.
    pub slots: Vec<Region2>,
    /// Open-edge relief profiles.
    pub reliefs: Vec<Region2>,
    /// One integer-pattern record per input span.
    pub rows: Vec<BendSlotRow>,
    /// Full-width bend bands to certify against the material with an exact planar difference.
    pub bend_bands: Vec<Region2>,
}

/// A refused dimension, span, budget or unsupported encounter with existing material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BendSlotError {
    /// Invalid option value.
    Parameter(&'static str),
    /// A particular input bend cannot hold the requested pattern.
    Bend {
        /// Zero-based input bend index.
        index: usize,
        /// Why this bend was refused.
        reason: &'static str,
    },
    /// The required cuts do not fit the current material.
    Material(&'static str),
    /// The exact material-coverage operation could not be completed.
    Geometry(String),
    /// A bounded input or output count was exceeded.
    Budget,
}

impl std::fmt::Display for BendSlotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parameter(why) | Self::Material(why) => write!(f, "bend slots: {why}"),
            Self::Bend { index, reason } => {
                write!(f, "bend slots: bend {}: {reason}", index.saturating_add(1))
            }
            Self::Geometry(why) => write!(f, "bend slots exact material check: {why}"),
            Self::Budget => write!(
                f,
                "bend slots exceed the 64-bend or 512-cut geometry budget"
            ),
        }
    }
}

impl std::error::Error for BendSlotError {}

fn valid_point(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.x.abs() < MAX_COORD && p.y.abs() < MAX_COORD
}

fn validate(spec: &BendSlotOptions) -> Result<(), BendSlotError> {
    if ![
        spec.slot_length,
        spec.slot_width,
        spec.bridge,
        spec.end_margin,
        spec.relief_depth,
        spec.corner_radius,
    ]
    .iter()
    .all(|v| v.is_finite() && *v >= 0.0 && *v < MAX_COORD)
    {
        return Err(BendSlotError::Parameter(
            "dimensions must be finite, nonnegative and below 100000000 mm",
        ));
    }
    if spec.slot_width < MIN_SIZE
        || spec.bridge < MIN_SIZE
        || spec.slot_length < spec.slot_width + MIN_SIZE
    {
        return Err(BendSlotError::Parameter(
            "slot width and bridge must be at least 0.001 mm; slot length must exceed width by 0.001 mm",
        ));
    }
    if spec.reliefs && (spec.relief_depth < MIN_SIZE || spec.corner_radius > spec.relief_depth) {
        return Err(BendSlotError::Parameter(
            "relief depth must be at least 0.001 mm and at least the entry corner radius",
        ));
    }
    if spec.corner_radius > 0.0 && spec.corner_radius < MIN_SIZE {
        return Err(BendSlotError::Parameter(
            "entry corner radius must be zero or at least 0.001 mm",
        ));
    }
    Ok(())
}

fn check_counts(material: &[Region2], bends: usize) -> Result<(), BendSlotError> {
    if bends == 0 {
        return Err(BendSlotError::Parameter("at least one bend is required"));
    }
    if bends > MAX_BENDS {
        return Err(BendSlotError::Budget);
    }
    let [field] = material else {
        return Err(BendSlotError::Material(
            "one connected flat material region is required",
        ));
    };
    if field.holes.len() > MAX_PROFILE_SEGMENTS {
        return Err(BendSlotError::Budget);
    }
    let count = field.holes.iter().fold(field.outer.segs.len(), |n, h| {
        n.saturating_add(h.segs.len())
    });
    if count > MAX_PROFILE_SEGMENTS {
        return Err(BendSlotError::Budget);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Frame {
    origin: Point2,
    d: Vec2,
    n: Vec2,
    angle: f64,
}

impl Frame {
    fn point(self, x: f64, y: f64) -> Point2 {
        self.origin + self.d * x + self.n * y
    }

    fn line(self, a: (f64, f64), b: (f64, f64)) -> Seg2 {
        Seg2::Line {
            a: self.point(a.0, a.1),
            b: self.point(b.0, b.1),
        }
    }

    fn arc(self, centre: (f64, f64), radius: f64, start: f64, sweep: f64) -> Seg2 {
        Seg2::Arc {
            center: self.point(centre.0, centre.1),
            radius,
            start: self.angle + start,
            sweep,
        }
    }
}

fn slot(frame: Frame, start: f64, spec: &BendSlotOptions) -> Region2 {
    let h = spec.slot_width / 2.0;
    let end = start + spec.slot_length;
    let segs = if spec.rounded {
        vec![
            frame.line((start + h, -h), (end - h, -h)),
            frame.arc((end - h, 0.0), h, -FRAC_PI_2, PI),
            frame.line((end - h, h), (start + h, h)),
            frame.arc((start + h, 0.0), h, FRAC_PI_2, PI),
        ]
    } else {
        vec![
            frame.line((start, -h), (end, -h)),
            frame.line((end, -h), (end, h)),
            frame.line((end, h), (start, h)),
            frame.line((start, h), (start, -h)),
        ]
    };
    Region2 {
        outer: Loop2 { segs },
        holes: Vec::new(),
    }
}

struct Relief {
    cut: Region2,
    mouth: Loop2,
    probe: Point2,
}

fn relief(frame: Frame, spec: &BendSlotOptions) -> Relief {
    let (h, r, depth) = (spec.slot_width / 2.0, spec.corner_radius, spec.relief_depth);
    let mouth_line = frame.line((0.0, h + r), (0.0, -h - r));
    let mut segs = vec![mouth_line];
    if r > 0.0 {
        segs.push(frame.arc((r, -h - r), r, PI, -FRAC_PI_2));
    }
    if depth > r {
        segs.push(frame.line((r, -h), (depth, -h)));
    }
    if spec.rounded {
        segs.push(frame.arc((depth, 0.0), h, -FRAC_PI_2, PI));
    } else {
        segs.push(frame.line((depth, -h), (depth, h)));
    }
    if depth > r {
        segs.push(frame.line((depth, h), (r, h)));
    }
    if r > 0.0 {
        segs.push(frame.arc((r, h + r), r, -FRAC_PI_2, -FRAC_PI_2));
    }
    Relief {
        cut: Region2 {
            outer: Loop2 { segs },
            holes: Vec::new(),
        },
        // A closed zero-area trace certifies coverage of the entire mouth on one outer edge.
        mouth: Loop2 {
            segs: vec![mouth_line, mouth_line.reversed()],
        },
        probe: frame.point(depth / 2.0, 0.0),
    }
}

fn disjoint(a: &Loop2, b: &Loop2) -> bool {
    a.placement(b, TOL) == Some(LoopPlacement::Outside)
        && b.placement(a, TOL) == Some(LoopPlacement::Outside)
}

fn certify(
    plan: &BendSlotPlan,
    relief_data: &[Relief],
    material: &[Region2],
) -> Result<(), BendSlotError> {
    let [field] = material else {
        return Err(BendSlotError::Material(
            "one connected flat material region is required",
        ));
    };
    for cut in &plan.slots {
        if cut.outer.placement(&field.outer, TOL) != Some(LoopPlacement::Inside) {
            return Err(BendSlotError::Material(
                "a slot crosses or touches the sheet boundary",
            ));
        }
    }
    for data in relief_data {
        if data.mouth.placement(&field.outer, TOL) != Some(LoopPlacement::Coincident)
            || field.outer.holds(data.probe) != Some(true)
        {
            return Err(BendSlotError::Material(
                "an end relief needs a straight exterior edge across its entire mouth",
            ));
        }
    }
    for band in &plan.bend_bands {
        if field.holes.iter().any(|hole| !disjoint(&band.outer, hole)) {
            return Err(BendSlotError::Material(
                "a selected bend is interrupted by an existing opening",
            ));
        }
    }
    let cuts: Vec<&Region2> = plan.slots.iter().chain(&plan.reliefs).collect();
    for (i, cut) in cuts.iter().enumerate() {
        if field.holes.iter().any(|hole| !disjoint(&cut.outer, hole)) {
            return Err(BendSlotError::Material(
                "a bend cut crosses or touches an existing opening",
            ));
        }
        for earlier in cuts.iter().take(i) {
            if !disjoint(&cut.outer, &earlier.outer) {
                return Err(BendSlotError::Material(
                    "generated bend cuts cross or touch; reduce their dimensions or change the bend selection",
                ));
            }
        }
    }
    Ok(())
}

/// Plan centred slots and matching end reliefs in the current planar material.
///
/// Every span is a current uninterrupted bend. Relief mouths must lie on straight exterior
/// edges. Cuts touching existing openings or each other are refused, never silently clipped.
/// Returns only exact lines and arcs, independent of span orientation or world placement.
/// The workflow must also subtract the material from `bend_bands` and `reliefs` and refuse
/// any nonempty result. This certifies the whole bands and throats against concave exterior
/// edges beyond the initial entry probes, using an exact planar boolean rather than samples.
pub fn plan(
    material: &[Region2],
    bends: &[BendSpan],
    spec: &BendSlotOptions,
) -> Result<BendSlotPlan, BendSlotError> {
    check_counts(material, bends.len())?;
    validate(spec)?;
    let pitch = spec.slot_length + spec.bridge;
    let cap = if spec.rounded {
        spec.slot_width / 2.0
    } else {
        0.0
    };
    let reach = if spec.reliefs {
        spec.relief_depth + cap
    } else {
        0.0
    };
    let reserve = reach + spec.end_margin;
    let mut out = BendSlotPlan {
        slots: Vec::new(),
        reliefs: Vec::new(),
        rows: Vec::new(),
        bend_bands: Vec::new(),
    };
    let mut relief_data = Vec::new();
    for (index, bend) in bends.iter().enumerate() {
        if !valid_point(bend.start) || !valid_point(bend.end) {
            return Err(BendSlotError::Bend {
                index,
                reason: "endpoints must be finite and within the coordinate limit",
            });
        }
        let delta = bend.end - bend.start;
        let span = delta.length();
        if !(span.is_finite() && span >= MIN_SIZE) {
            return Err(BendSlotError::Bend {
                index,
                reason: "the bend has no usable length",
            });
        }
        let quantity = ((span - 2.0 * reserve + spec.bridge) / pitch).floor();
        if !(quantity >= 1.0 && quantity <= MAX_CUTS as f64) {
            return Err(BendSlotError::Bend {
                index,
                reason: "no bounded slot row fits between the end reliefs and margins",
            });
        }
        let count = quantity as usize;
        let relief_count = if spec.reliefs { 2 } else { 0 };
        if out
            .slots
            .len()
            .saturating_add(out.reliefs.len())
            .saturating_add(count)
            .saturating_add(relief_count)
            > MAX_CUTS
        {
            return Err(BendSlotError::Budget);
        }
        let occupied =
            count as f64 * spec.slot_length + (count.saturating_sub(1)) as f64 * spec.bridge;
        let start = (span - occupied) / 2.0;
        let d = delta * (1.0 / span);
        let n = Vec2::new(-d.y, d.x);
        let frame = Frame {
            origin: bend.start,
            d,
            n,
            angle: d.y.atan2(d.x),
        };
        let h = spec.slot_width / 2.0;
        out.bend_bands.push(Region2 {
            outer: Loop2 {
                segs: vec![
                    frame.line((0.0, -h), (span, -h)),
                    frame.line((span, -h), (span, h)),
                    frame.line((span, h), (0.0, h)),
                    frame.line((0.0, h), (0.0, -h)),
                ],
            },
            holes: Vec::new(),
        });
        for k in 0..count {
            out.slots.push(slot(frame, start + k as f64 * pitch, spec));
        }
        if spec.reliefs {
            let far = Frame {
                origin: bend.end,
                d: -d,
                n: -n,
                angle: frame.angle + PI,
            };
            for at in [frame, far] {
                let data = relief(at, spec);
                out.reliefs.push(data.cut.clone());
                relief_data.push(data);
            }
        }
        out.rows.push(BendSlotRow {
            count,
            span,
            start,
            end_bridge: start - reach,
        });
    }
    certify(&out, &relief_data, material)?;
    Ok(out)
}

/// Largest source-bend count considered by exact edge discovery.
pub const MAX_SOURCE_BENDS: usize = 256;
/// Largest edge or face count considered by exact edge discovery.
pub const MAX_PICK_TOPOLOGY: usize = 16_384;

/// One folded source bend, expressed in the B-rep body's own frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldedBend {
    /// A point on its cylinder axis, at axial coordinate zero.
    pub origin: [f64; 3],
    /// Unit axial direction.
    pub axis: [f64; 3],
    /// Unit radial direction at the beginning of the bend.
    pub start: [f64; 3],
    /// Unit radial direction a positive quarter turn into the bend.
    pub toward: [f64; 3],
    /// Complete current axial range.
    pub span: [f64; 2],
    /// Inner cylinder radius.
    pub radius: f64,
    /// Material thickness.
    pub thickness: f64,
    /// Positive bend angle, in radians. Manual discovery supports at most a half turn.
    pub angle: f64,
}

/// One exact adjacent cylinder, read from B-rep topology rather than a display mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct BendCylinder {
    /// Cylinder frame (its axis is x cross y).
    pub frame: crate::eval::Frame,
    /// Cylinder radius.
    pub radius: f64,
}

fn unit3(v: crate::Vec3) -> bool {
    (v.length() - 1.0).abs() <= 1e-8
}

fn bounded3(v: crate::Vec3) -> bool {
    [v.x, v.y, v.z]
        .iter()
        .all(|x| x.is_finite() && x.abs() < MAX_COORD)
}

fn validate_folded(b: &FoldedBend) -> Result<(), BendSlotError> {
    let (o, d, s, n) = (
        crate::Vec3::from(b.origin),
        crate::Vec3::from(b.axis),
        crate::Vec3::from(b.start),
        crate::Vec3::from(b.toward),
    );
    if !bounded3(o)
        || !unit3(d)
        || !unit3(s)
        || !unit3(n)
        || d.dot(s).abs() > 1e-8
        || d.dot(n).abs() > 1e-8
        || s.dot(n).abs() > 1e-8
        || !b.span.iter().all(|x| x.is_finite() && x.abs() < MAX_COORD)
        || !(b.span[1] > b.span[0])
        || !(b.angle > 0.0 && b.angle <= PI)
        || !(b.radius >= 0.0 && b.radius < MAX_COORD)
        || !(b.thickness >= MIN_SIZE && b.thickness < MAX_COORD)
    {
        return Err(BendSlotError::Parameter(
            "invalid or unsupported folded bend for exact edge discovery",
        ));
    }
    Ok(())
}

fn curve_segment(
    curve: &crate::eval::Curve,
    interval: [f64; 2],
) -> Result<crate::nurbs::crv_basic::Curve, BendSlotError> {
    use crate::eval::Curve;
    use crate::nurbs::math::CPoint;
    let fail = || BendSlotError::Parameter("unsupported exact bend-edge curve");
    if !interval
        .iter()
        .all(|x| x.is_finite() && x.abs() < MAX_COORD)
        || !(interval[1] > interval[0])
    {
        return Err(fail());
    }
    // The analytic form is exact; a stored NURBS is trimmed by knot insertion, never sampled.
    let analytic = crate::eval::form::curve(curve, interval);
    let (form, trimmed) = analytic
        .as_ref()
        .map_or((curve, interval), |(form, interval)| (form, *interval));
    let (knots, points) = match form {
        Curve::Spline { knots, points } if points.len() <= 128 && knots.len() <= 160 => (
            knots,
            points
                .iter()
                .map(|p| CPoint::from_point(crate::Vec3::from(*p)))
                .collect::<Vec<_>>(),
        ),
        Curve::RationalSpline { knots, homogeneous }
            if homogeneous.len() <= 128 && knots.len() <= 160 =>
        {
            if !homogeneous
                .iter()
                .all(|h| h.iter().all(|v| v.is_finite()) && h[3] > 1e-12)
            {
                return Err(fail());
            }
            (
                knots,
                homogeneous
                    .iter()
                    .map(|h| CPoint::new(h[0], h[1], h[2], h[3]))
                    .collect(),
            )
        }
        _ => return Err(fail()),
    };
    let degree = knots
        .len()
        .checked_sub(points.len().saturating_add(1))
        .ok_or_else(fail)?;
    if !(1..=16).contains(&degree)
        || points.len() <= degree
        || !knots.iter().all(|x| x.is_finite())
        || knots.windows(2).any(|w| w.first() > w.get(1))
        || !points.iter().all(|p| bounded3(p.to_euclid()))
        || knots.get(degree).is_none_or(|lo| trimmed[0] < *lo)
        || knots.get(points.len()).is_none_or(|hi| trimmed[1] > *hi)
    {
        return Err(fail());
    }
    let curve =
        crate::nurbs::crv_basic::Curve::new(points, degree, knots.clone()).map_err(|_| fail())?;
    curve
        .extract_seg(trimmed[0], trimmed[1])
        .map_err(|_| fail())
}

/// Resolve an exact trimmed edge against its adjacent cylinders and current source bends.
///
/// A positive-weight rational control hull certifies the whole trimmed edge's axial range
/// and convex bend wedge. The implicit cylinder polynomial certifies its complete curve,
/// so a midpoint or a close display polyline cannot turn an unrelated edge into a bend.
/// `None` means no unique supported source bend (including ambiguity).
pub fn source_bend_for_edge(
    curve: &crate::eval::Curve,
    interval: [f64; 2],
    adjacent: &[BendCylinder],
    bends: &[FoldedBend],
    tolerance: f64,
) -> Result<Option<usize>, BendSlotError> {
    use crate::surface::implicit::{curve_lies_on, Implicit};
    if bends.len() > MAX_SOURCE_BENDS || adjacent.len() > 2 {
        return Err(BendSlotError::Budget);
    }
    if !(tolerance.is_finite() && (1e-9..=1e-3).contains(&tolerance)) {
        return Err(BendSlotError::Parameter(
            "invalid exact bend-edge tolerance",
        ));
    }
    for bend in bends {
        validate_folded(bend)?;
    }
    for cylinder in adjacent {
        if crate::eval::check::frame(&cylinder.frame).is_err()
            || !bounded3(crate::Vec3::from(cylinder.frame.origin))
            || !(cylinder.radius > 0.0 && cylinder.radius < MAX_COORD)
        {
            return Err(BendSlotError::Parameter("invalid adjacent bend cylinder"));
        }
    }
    let segment = curve_segment(curve, interval)?;
    let spline = crate::nurbs::b_spline_curve::BSplineCurve::from_gw_nurb(3, &segment);
    let mut found = None;
    for (index, bend) in bends.iter().enumerate() {
        let (o, d, s, n) = (
            crate::Vec3::from(bend.origin),
            crate::Vec3::from(bend.axis),
            crate::Vec3::from(bend.start),
            crate::Vec3::from(bend.toward),
        );
        let end_inward = s * bend.angle.sin() - n * bend.angle.cos();
        let fits = segment.cpts().iter().all(|p| {
            let q = p.to_euclid() - o;
            let along = q.dot(d);
            along >= bend.span[0] - tolerance
                && along <= bend.span[1] + tolerance
                && q.dot(n) >= -tolerance
                && q.dot(end_inward) >= -tolerance
        });
        if !fits {
            continue;
        }
        for cylinder in adjacent {
            let c = crate::Vec3::from(cylinder.frame.origin);
            let axis = crate::Vec3::from(cylinder.frame.x_axis)
                .cross(crate::Vec3::from(cylinder.frame.y_axis));
            if axis.cross(d).length() > 1e-8
                || (c - o).cross(d).length() > tolerance
                || ((cylinder.radius - bend.radius).abs() > tolerance
                    && (cylinder.radius - bend.radius - bend.thickness).abs() > tolerance)
            {
                continue;
            }
            let implicit = Implicit::Cone {
                origin: o,
                axis: d,
                radius: cylinder.radius,
                tan_angle: 0.0,
            };
            if !curve_lies_on(&spline, &implicit)
                .map_err(|e| BendSlotError::Geometry(e.to_string()))?
            {
                continue;
            }
            if found.is_some_and(|previous| previous != index) {
                return Ok(None);
            }
            found = Some(index);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]

    use super::*;

    fn options() -> BendSlotOptions {
        BendSlotOptions {
            slot_length: 8.0,
            slot_width: 1.2,
            bridge: 4.0,
            end_margin: 3.0,
            relief_depth: 4.0,
            corner_radius: 2.0,
            rounded: true,
            reliefs: true,
        }
    }

    fn field() -> Vec<Region2> {
        let p = |x, y| Point2::new(x, y);
        vec![Region2 {
            outer: Loop2 {
                segs: vec![
                    Seg2::Line {
                        a: p(0.0, -25.0),
                        b: p(88.0, -25.0),
                    },
                    Seg2::Line {
                        a: p(88.0, -25.0),
                        b: p(88.0, 25.0),
                    },
                    Seg2::Line {
                        a: p(88.0, 25.0),
                        b: p(0.0, 25.0),
                    },
                    Seg2::Line {
                        a: p(0.0, 25.0),
                        b: p(0.0, -25.0),
                    },
                ],
            },
            holes: Vec::new(),
        }]
    }

    fn bends() -> Vec<BendSpan> {
        vec![BendSpan {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(88.0, 0.0),
        }]
    }

    #[test]
    fn exact_round_slots_and_reliefs_have_the_analytic_areas() {
        let spec = options();
        let plan = plan(&field(), &bends(), &spec).unwrap();
        assert_eq!(plan.rows[0].count, 6);
        let slot_area = (spec.slot_length - spec.slot_width) * spec.slot_width
            + PI * spec.slot_width.powi(2) / 4.0;
        let relief_area = spec.relief_depth * spec.slot_width
            + 2.0 * spec.corner_radius.powi(2) * (1.0 - PI / 4.0)
            + PI * spec.slot_width.powi(2) / 8.0;
        for slot in plan.slots {
            assert!((slot.outer.signed_area().unwrap().value - slot_area).abs() < 1e-9);
        }
        for relief in plan.reliefs {
            assert!(relief.outer.is_closed(1e-9));
            assert!((relief.outer.signed_area().unwrap().value - relief_area).abs() < 1e-9);
        }
    }

    #[test]
    fn rectangular_caps_and_zero_entry_radius_are_valid() {
        let spec = BendSlotOptions {
            rounded: false,
            corner_radius: 0.0,
            ..options()
        };
        let plan = plan(&field(), &bends(), &spec).unwrap();
        assert!(plan.slots.iter().chain(&plan.reliefs).all(|r| r
            .outer
            .segs
            .iter()
            .all(|s| matches!(s, Seg2::Line { .. }))));
        assert!(
            (plan.slots[0].outer.signed_area().unwrap().value - spec.slot_length * spec.slot_width)
                .abs()
                < 1e-9
        );
        assert!(
            (plan.reliefs[0].outer.signed_area().unwrap().value
                - spec.relief_depth * spec.slot_width)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn row_geometry_follows_a_quarter_turn_of_its_material() {
        let turn = |p: Point2| Point2::new(17.0 - p.y, p.x - 9.0);
        let mut material = field();
        for region in &mut material {
            for segment in &mut region.outer.segs {
                if let Seg2::Line { a, b } = segment {
                    *a = turn(*a);
                    *b = turn(*b);
                }
            }
        }
        let spans: Vec<_> = bends()
            .iter()
            .map(|b| BendSpan {
                start: turn(b.start),
                end: turn(b.end),
            })
            .collect();
        let rotated = plan(&material, &spans, &options()).unwrap();
        assert_eq!(
            rotated.rows,
            plan(&field(), &bends(), &options()).unwrap().rows
        );
        assert!(rotated.slots.iter().all(|s| s.outer.is_closed(1e-8)));
    }

    #[test]
    fn invalid_sizes_counts_and_intersections_are_refused() {
        for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert!(plan(
                &field(),
                &bends(),
                &BendSlotOptions {
                    slot_width: bad,
                    ..options()
                }
            )
            .is_err());
        }
        assert!(plan(
            &field(),
            &bends(),
            &BendSlotOptions {
                end_margin: 100.0,
                ..options()
            }
        )
        .is_err());
        assert!(plan(
            &field(),
            &bends(),
            &BendSlotOptions {
                corner_radius: 5.0,
                ..options()
            }
        )
        .is_err());
        let mut spans = bends();
        spans.push(spans[0]);
        assert!(matches!(
            plan(&field(), &spans, &options()),
            Err(BendSlotError::Material(_))
        ));
        assert!(plan(&field(), &vec![bends()[0]; MAX_BENDS + 1], &options()).is_err());
    }

    #[test]
    fn existing_openings_are_kept_clear_and_reliefs_need_exterior_edges() {
        let mut material = field();
        let opening = plan(&material, &bends(), &options()).unwrap().slots[0]
            .outer
            .clone();
        material[0].holes.push(opening);
        assert!(plan(&material, &bends(), &options()).is_err());
        let spans = [BendSpan {
            start: Point2::new(5.0, 0.0),
            end: Point2::new(83.0, 0.0),
        }];
        assert!(plan(&field(), &spans, &options()).is_err());
        assert!(plan(
            &field(),
            &spans,
            &BendSlotOptions {
                reliefs: false,
                ..options()
            }
        )
        .is_ok());
    }

    fn folded_bend() -> FoldedBend {
        FoldedBend {
            origin: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            start: [1.0, 0.0, 0.0],
            toward: [0.0, 1.0, 0.0],
            span: [0.0, 10.0],
            radius: 2.0,
            thickness: 1.0,
            angle: FRAC_PI_2,
        }
    }

    fn cylinder() -> BendCylinder {
        BendCylinder {
            frame: crate::eval::Frame {
                origin: [0.0; 3],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius: 2.0,
        }
    }

    #[test]
    fn exact_trimmed_line_and_rational_end_arc_resolve_without_samples() {
        let bend = folded_bend();
        let cylinder = cylinder();
        let line = crate::eval::Curve::Line {
            start: [2.0, 0.0, 0.0],
            end: [2.0, 0.0, 10.0],
        };
        assert_eq!(
            source_bend_for_edge(
                &line,
                [0.0, 1.0],
                std::slice::from_ref(&cylinder),
                &[bend],
                1e-7
            )
            .unwrap(),
            Some(0)
        );
        let circle = crate::eval::Curve::Circle {
            frame: cylinder.frame.clone(),
            radius: 2.0,
        };
        let (form, interval) = crate::eval::form::curve(&circle, [0.0, FRAC_PI_2]).unwrap();
        assert_eq!(
            source_bend_for_edge(
                &form,
                interval,
                std::slice::from_ref(&cylinder),
                &[bend],
                1e-7
            )
            .unwrap(),
            Some(0)
        );
        // A long arc has the same end and midpoint supports but leaves the bend wedge.
        assert_eq!(
            source_bend_for_edge(&circle, [0.0, 3.0 * FRAC_PI_2], &[cylinder], &[bend], 1e-7)
                .unwrap(),
            None
        );
    }

    #[test]
    fn exact_bend_edge_resolution_refuses_ambiguous_coaxial_and_off_surface_curves() {
        let bend = folded_bend();
        let line = crate::eval::Curve::Line {
            start: [2.0, 0.0, 0.0],
            end: [2.0, 0.0, 10.0],
        };
        let other = FoldedBend {
            span: [20.0, 30.0],
            ..bend
        };
        assert_eq!(
            source_bend_for_edge(&line, [0.0, 1.0], &[cylinder()], &[other, bend], 1e-7).unwrap(),
            Some(1)
        );
        assert_eq!(
            source_bend_for_edge(&line, [0.0, 1.0], &[cylinder()], &[bend, bend], 1e-7).unwrap(),
            None
        );
        let false_form = crate::eval::Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            points: vec![[2.0, 0.0, 0.0], [1.8, 0.0, 5.0], [2.0, 0.0, 10.0]],
        };
        assert_eq!(
            source_bend_for_edge(&false_form, [0.0, 1.0], &[cylinder()], &[bend], 1e-7).unwrap(),
            None
        );
        assert!(source_bend_for_edge(&line, [0.0, 1.0], &[cylinder()], &[bend], f64::NAN).is_err());
        assert!(source_bend_for_edge(&line, [1.0, 0.0], &[cylinder()], &[bend], 1e-7).is_err());
    }
}
