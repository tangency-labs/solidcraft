// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::neg_cmp_op_on_partial_ord)]
//! Cutter geometry for conical threads and screw points, with no topology or modelling state.
//! The caller sequences sweeps, lofts, revolutions and booleans through solidcore-ops.

use crate::eval::Frame;
use crate::prism::profile::{Loop2, Seg2};
use crate::{Result, SolidError, Vec2, Vec3};

/// Which helices to cut.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThreadHand {
    /// A right-handed groove.
    Right,
    /// A left-handed groove.
    Left,
    /// One groove in each direction.
    Both,
}

/// Where the shank groove ends relative to its physical face.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShankEnd {
    /// The shank ends at a free face.
    Free,
    /// The shank ends against a shoulder.
    Closed,
    /// The groove ends within the shank.
    Inside,
}

/// Where the thread leaves a cone for a cylindrical shank.
#[derive(Clone, Copy)]
pub struct Shank {
    /// Requested end, in axial coordinates.
    pub end: f64,
    /// The shank’s actual end, in axial coordinates.
    pub physical_end: f64,
    /// How the thread meets the end.
    pub kind: ShankEnd,
    /// Requested axial length of the fade at this end.
    pub runout: f64,
    /// Whether to reduce groove depth near the cone’s small end.
    pub fade: bool,
}

/// Cone, axial limits and nominal groove profile for a conical thread.
#[derive(Clone, Copy)]
pub struct ConicalThread {
    /// Origin of the axial coordinates in model space.
    pub axis_point: [f64; 3],
    /// Unit direction of increasing axial coordinate.
    pub axis: [f64; 3],
    /// Requested thread limits in increasing axial order.
    pub span: (f64, f64),
    /// Axial coordinate of the cone’s wide rim.
    pub wide: f64,
    /// Axial coordinate of the cone’s small end.
    pub small: f64,
    /// Change in stock radius per unit along the axis.
    pub slope: f64,
    /// Stock radius at the wide rim.
    pub radius: f64,
    /// Axial distance between adjacent grooves.
    pub pitch: f64,
    /// Nominal groove depth.
    pub depth: f64,
    /// Half the axial width of the nominal groove root.
    pub flat: f64,
    /// Angles of the low and high axial flanks, in radians.
    pub flanks: [f64; 2],
    /// Number of parallel grooves per hand.
    pub starts: u32,
    /// Hand of the groove or pair of grooves.
    pub hand: ThreadHand,
    /// The adjoining shank, if the cone is a screw point.
    pub shank: Option<Shank>,
}

/// A closed outline and its plane in model coordinates.
pub struct Profile {
    /// The profile’s placement in model coordinates.
    pub frame: Frame,
    /// The closed profile in the frame’s coordinates.
    pub outline: Loop2,
}

/// A groove or crest loft through transverse stations.
pub struct Loft {
    /// Corresponding cross-sections in axial order.
    pub sections: Vec<Profile>,
    /// Degree of the loft along the axis.
    pub degree: u32,
    /// Apex extending the first section, when required.
    pub apex: Option<[f64; 3]>,
    /// Map from the loft’s working scale to model coordinates, if scaled.
    pub placement: Option<[[f64; 4]; 4]>,
}

/// The crossed cutter's envelope and the core and crests it leaves behind.
pub struct Crossed {
    /// Meridian of the solid left beneath the grooves.
    pub core: Profile,
    /// One group of lofts per connected crest.
    pub lobes: Vec<Vec<Loft>>,
    /// Meridian of the cutter envelope.
    pub stock: Profile,
    /// Unit direction used to revolve the meridians.
    pub axis: [f64; 3],
}

/// One helix start and sense of the meridian cutter.
pub struct Screw {
    /// Meridian cross-section of the tool.
    pub profile: Profile,
    /// Axis with orientation chosen for this hand.
    pub axis: [f64; 3],
    /// Signed axial advance in one turn.
    pub lead: f64,
}

/// The shared law, trim profile and optional exact root for a meridian cutter.
pub struct Swept {
    /// One sweep per start and hand.
    pub tools: Vec<Screw>,
    /// Total number of turns in each sweep.
    pub turns: f64,
    /// Radial offsets in increasing turn order.
    pub law: Vec<(f64, f64)>,
    /// Planes bounding the cut at the low and high axial ends.
    pub trim: [Frame; 2],
    /// Meridian of the exact root to keep, for a screw point.
    pub core: Option<Profile>,
}

/// Select a construction from the clearance between the groove and the axis.
pub enum Grooves {
    /// Sweep a meridian profile along one or more helices.
    Swept(Swept),
    /// Loft one or more transverse grooves.
    Lofted(Vec<Loft>),
    /// Build the complement around the core and crossed crests.
    Crossed(Crossed),
}

/// Why a conical cutter cannot be planned.
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// The cone's slope is too steep for the limiting flank.
    #[error(
        "the cone is too steep for this thread's flanks: its half-angle must be below {0:.2}°"
    )]
    TooSteep(f64),
    /// A separate geometric refusal from the kernel.
    #[error(transparent)]
    Geometry(#[from] SolidError),
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("conical thread", reason)
}

fn frame(origin: Vec3, x: Vec3, y: Vec3) -> Frame {
    Frame {
        origin: origin.to_array(),
        x_axis: x.to_array(),
        y_axis: y.to_array(),
    }
}

fn across(k: Vec3) -> Result<Vec3> {
    k.cross(if k.x.abs() < 0.9 { Vec3::X } else { Vec3::Y })
        .unitize()
}

fn polygon(points: &[Vec2]) -> Loop2 {
    let segs = points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
        .map(|(a, b)| Seg2::Line { a, b })
        .collect();
    Loop2 { segs }
}

#[derive(Clone, Copy)]
struct ShankSpan {
    tip: f64,
    tip_radius: f64,
    height: f64,
    dir: f64,
    full: f64,
    last: f64,
    free: bool,
    fade: Option<PointFade>,
}

impl ShankSpan {
    fn root_spans(self, radius: f64, depth: f64) -> Vec<RootSpan> {
        let Self {
            height, full, last, ..
        } = self;
        let z = |h| self.tip + self.dir * h;
        let at = |h| radius - depth * self.fade.map_or(1.0, |fade| fade.fraction(h));
        let mut spans = Vec::new();
        let blend_end = self.fade.and_then(PointFade::shank_end).unwrap_or(height);
        if blend_end > height {
            spans.push(RootSpan::new(
                (z(height), at(height)),
                (z(blend_end), at(blend_end)),
            ));
        }
        if full > blend_end {
            spans.push(RootSpan::new(
                (z(blend_end), at(blend_end)),
                (z(full), at(full)),
            ));
        }
        if last > full {
            spans.push(RootSpan::new((z(full), at(full)), (z(last), radius)));
        }
        spans
    }
}

fn shank_span(t: &ConicalThread, end: Shank, clearance: f64) -> Result<ShankSpan> {
    let tip = t.small;
    let height = (t.wide - tip).abs();
    let dir = (t.wide - tip).signum();
    let tip_radius = t.radius + (tip - t.wide) * t.slope;
    let requested = (end.end - tip) * dir;
    let physical = (end.physical_end - tip) * dir;
    let (full, last) = match end.kind {
        ShankEnd::Free => ((requested - end.runout).max(height), requested),
        ShankEnd::Closed => {
            let full = requested - clearance;
            (full, (full + end.runout).min(physical))
        }
        ShankEnd::Inside => (requested, requested + end.runout),
    };
    if !(height > 0.0
        && end.runout.is_finite()
        && end.runout >= 0.0
        && physical >= requested
        && full >= height
        && last > height)
    {
        return Err(invalid(
            "the shank thread is too short between the point and shoulder",
        ));
    }
    Ok(ShankSpan {
        tip,
        tip_radius,
        height,
        dir,
        full,
        last,
        free: end.kind == ShankEnd::Free,
        fade: end
            .fade
            .then(|| PointFade::new(height, t.slope.abs(), t.depth, t.pitch, full)),
    })
}

/// One axial interval with a straight root in the meridian plane.
#[derive(Clone, Copy)]
struct RootSpan {
    low: f64,
    high: f64,
    root_low: f64,
    root_high: f64,
}

impl RootSpan {
    fn new(a: (f64, f64), b: (f64, f64)) -> Self {
        let ((low, root_low), (high, root_high)) = if a.0 < b.0 { (a, b) } else { (b, a) };
        Self {
            low,
            high,
            root_low,
            root_high,
        }
    }
}

/// Smooth a corner in a radial law as it reaches a constant value. The
/// value at the corner is exact. The excess is under 9% of the blend width.
fn point_rim_blend(at: f64, rim: f64, width: f64) -> f64 {
    if at <= rim - width || at >= rim + width {
        return at.min(rim);
    }
    let t = (at - rim + width) / (2.0 * width);
    at - width * t * t + 4.0 * width * (t * (1.0 - t)).powi(2)
}

/// Fade to half the radial rise until nominal depth becomes possible.
/// If the cone ends first, reach nominal depth on the shank.
#[derive(Clone, Copy)]
enum PointFade {
    Point {
        end: f64,
        width: f64,
    },
    Shank {
        rim: f64,
        rim_fraction: f64,
        end: f64,
    },
}

impl PointFade {
    fn new(height: f64, slope: f64, depth: f64, pitch: f64, full_end: f64) -> Self {
        let knee = 2.0 * depth / slope;
        if knee <= height {
            Self::Point {
                end: knee,
                width: point_blend_width(knee, pitch),
            }
        } else {
            Self::Shank {
                rim: height,
                rim_fraction: 0.5 * height * slope / depth,
                end: full_end.min(height + 0.5 * pitch),
            }
        }
    }

    fn fraction(self, h: f64) -> f64 {
        match self {
            Self::Point { end, width } => point_depth_fraction(h, end, width),
            Self::Shank {
                rim,
                rim_fraction,
                end,
            } => {
                if h <= rim || end <= rim {
                    rim_fraction * h.min(rim) / rim
                } else {
                    rim_fraction + (1.0 - rim_fraction) * ((h - rim) / (end - rim)).min(1.0)
                }
            }
        }
    }

    fn fairing(self) -> Option<(f64, f64)> {
        match self {
            Self::Point { end, width } => Some((end - width, end)),
            Self::Shank { .. } => None,
        }
    }

    fn shank_end(self) -> Option<f64> {
        match self {
            Self::Point { .. } => None,
            Self::Shank { end, .. } => Some(end),
        }
    }
}

fn point_blend_width(length: f64, pitch: f64) -> f64 {
    (0.12 * pitch).min(length * 0.25)
}

/// Fair the depth law just before it reaches the full-depth plateau so
/// the loft does not overshoot into the core across the change in slope.
fn point_depth_fraction(at: f64, fade_length: f64, width: f64) -> f64 {
    let start = fade_length - width;
    if at <= start || at >= fade_length {
        return (at / fade_length).min(1.0);
    }
    let t = (at - start) / width;
    (start + width * (t + t * t * (1.0 - t))) / fade_length
}

fn polar_arc(radius: f64, start: f64, end: f64) -> Seg2 {
    Seg2::Arc {
        center: Vec2::ZERO,
        radius,
        start,
        sweep: end - start,
    }
}

fn polar_flank([r, s]: [f64; 2], [start, end]: [f64; 2]) -> Seg2 {
    let [u, v] = [start, end].map(|angle| Vec2::new(angle.cos(), angle.sin()));
    let tangent = |p: Vec2, radius| p * (s - r) + Vec2::new(-p.y, p.x) * (radius * (end - start));
    Seg2::Cubic {
        p0: u * r,
        p1: u * r + tangent(u, r) / 3.0,
        p2: v * s - tangent(v, s) / 3.0,
        p3: v * s,
    }
}

fn polar_spiral(r: [f64; 2], a: [f64; 2]) -> impl Iterator<Item = Seg2> {
    (0..5).map(move |i| {
        let lerp = |pair: [f64; 2], i| pair[0] + (pair[1] - pair[0]) * (f64::from(i) / 5.0);
        polar_flank([lerp(r, i), lerp(r, i + 1)], [lerp(a, i), lerp(a, i + 1)])
    })
}

/// One cross-section of a groove. Keep the standard crest opening as the
/// groove shallows and bring the root flat out to meet the flanks.
fn point_groove_section(root: f64, outer: f64, low: [f64; 2], high: [f64; 2]) -> Loop2 {
    let [lo_root, lo_outer] = low.map(|a| -a);
    let [hi_root, hi_outer] = high;
    Loop2 {
        segs: polar_spiral([root, outer], [lo_root, lo_outer])
            .chain([
                polar_arc(outer, lo_outer, 0.0),
                polar_arc(outer, 0.0, hi_outer),
            ])
            .chain(polar_spiral([outer, root], [hi_outer, hi_root]))
            .chain([polar_arc(root, hi_root, 0.0), polar_arc(root, 0.0, lo_root)])
            .collect(),
    }
}

/// The stock and depth laws used by transverse grooves, measured from the small end.
struct PointGroove {
    tip_radius: f64,
    slope: f64,
    height: f64,
    depth: f64,
    fade: Option<PointFade>,
    runout: Option<(f64, f64)>,
    pitch: f64,
}

impl PointGroove {
    fn stock(&self, h: f64) -> f64 {
        self.tip_radius + h.min(self.height) * self.slope
    }

    fn depth_at(&self, h: f64) -> f64 {
        self.depth * self.fade.map_or(1.0, |fade| fade.fraction(h))
    }

    fn runout_at(&self, h: f64) -> f64 {
        self.runout.map_or(1.0, |(start, end)| {
            let length = end - start;
            point_depth_fraction(
                (end - h).max(0.0),
                length,
                point_blend_width(length, self.pitch),
            )
        })
    }
}

/// Find a lobe's end just inside the core, where its remaining volume is
/// buried. A finite cap there keeps the skin away from a degenerate section.
fn crossed_lobe_end(
    mut outside: f64,
    mut inside: f64,
    exposed: impl Fn(f64) -> f64,
) -> Option<f64> {
    if exposed(inside) <= 0.0 {
        return None;
    }
    if exposed(outside) >= 0.0 {
        return Some(outside);
    }
    for _ in 0..36 {
        let mid = (outside + inside) * 0.5;
        if exposed(mid) > 0.0 {
            inside = mid;
        } else {
            outside = mid;
        }
    }
    Some(inside)
}

fn crossed_lobe_section(
    skirt: f64,
    base: f64,
    [a, b]: [f64; 2],
    [lo, hi]: [f64; 2],
    scale: f64,
) -> Loop2 {
    let width = (a - lo * (skirt - base)).min(b - hi * (skirt - base));
    let top = base + (a / lo).min(b / hi);
    let symmetric = (lo - hi).abs() < 1e-12;
    if symmetric {
        let width = width * scale;
        return Loop2 {
            segs: polar_spiral([skirt, top], [-width, 0.0])
                .chain(polar_spiral([top, skirt], [0.0, width]))
                .chain(std::iter::once(polar_arc(skirt, width, -width)))
                .collect(),
        };
    }
    // With unequal flanks the tighter bound can change across the slice.
    // Align the skin by angle to keep the profiles in the same order.
    let radius = |w: f64| {
        (base + ((a - w) / lo).min((b - w) / hi))
            .max(skirt)
            .min(top)
    };
    let side = |from, to, sign: f64| {
        (0..5).map(move |i| {
            let at = |j| from + (to - from) * (f64::from(j) / 5.0);
            let angles = [at(i), at(i + 1)];
            polar_flank(angles.map(radius), angles.map(|w| w * sign * scale))
        })
    };
    let arc_angle = |i| width * scale * (1.0 - f64::from(i) * 0.5);
    Loop2 {
        segs: side(width, 0.0, -1.0)
            .chain(side(0.0, width, 1.0))
            .chain((0..4).map(|i| polar_arc(skirt, arc_angle(i), arc_angle(i + 1))))
            .collect(),
    }
}

fn meridian_profile(frame: Frame, outline: Loop2) -> Result<Profile> {
    let outline = if outline.signed_area()?.value < 0.0 {
        outline.reversed()
    } else {
        outline
    };
    Ok(Profile { frame, outline })
}

/// Build the complement of the two crossing grooves as a round core and
/// the crests left between opposite flanks. Cutting with its complement
/// avoids intersecting two spline roots that nearly coincide at the apex.
fn crossed_point_groove(
    t: &ConicalThread,
    groove: &PointGroove,
    meridian: Frame,
    finish: f64,
    dir: f64,
    tangents: [f64; 2],
) -> Result<Crossed> {
    let flat = t.flat;
    let (origin, x, y, _) = crate::prism::profile::axes(&meridian);

    let pitch = groove.pitch;
    let starts = t.starts.max(1);
    let period = std::f64::consts::TAU / f64::from(starts);
    let scale = period / pitch;
    let axis = y * dir;
    let binormal = axis.cross(x);
    let point = |h| origin + y * h;
    let baseline = |h| groove.stock(h) - groove.depth * groove.runout_at(h);
    let root = |h| groove.stock(h) - groove.depth_at(h) * groove.runout_at(h);

    let blend = groove.fade.and_then(PointFade::fairing);
    let ease = groove
        .runout
        .map(|(start, end)| (start, start + point_blend_width(end - start, pitch)));
    let mut knots = vec![0.0, groove.height, finish];
    knots.extend(blend.into_iter().flat_map(|(a, b)| [a, b]));
    knots.extend(ease.into_iter().flat_map(|(a, b)| [a, b]));
    knots.extend(groove.runout.map(|(_, end)| end));
    knots.extend(groove.fade.and_then(PointFade::shank_end));
    knots.sort_by(f64::total_cmp);
    knots.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let at = |h| Vec2::new(root(h), h);
    let line = |a, b| Seg2::Line { a, b };
    let mut segs = Vec::new();
    if root(0.0) > 1e-9 {
        segs.push(line(Vec2::ZERO, at(0.0)));
    }
    for pair in knots.windows(2) {
        let [a, b] = pair else { continue };
        let (a, b) = (*a, *b);
        if [blend, ease]
            .into_iter()
            .flatten()
            .any(|(lo, hi)| a >= lo && b <= hi)
        {
            // Four samples recover the cubic meridian exactly within a blend.
            let (p0, p3) = (at(a), at(b));
            let (q1, q2) = (
                at(a + (b - a) / 3.0) * 27.0 - p0 * 8.0 - p3,
                at(b - (b - a) / 3.0) * 27.0 - p0 - p3 * 8.0,
            );
            segs.push(Seg2::Cubic {
                p0,
                p1: (q1 * 2.0 - q2) / 18.0,
                p2: (q2 * 2.0 - q1) / 18.0,
                p3,
            });
        } else {
            segs.push(line(at(a), at(b)));
        }
    }
    segs.extend([
        line(at(finish), Vec2::new(0.0, finish)),
        line(Vec2::new(0.0, finish), Vec2::ZERO),
    ]);
    let core = meridian_profile(meridian.clone(), Loop2 { segs })?;

    let skirt_depth = |h| (0.043 * pitch).min(0.12 * root(h));
    let [lo, hi] = tangents;
    // A quadratic skin stays monotone where the tighter asymmetric flank
    // changes across successive sections.
    let degree = if (lo - hi).abs() < 1e-12 { 3 } else { 2 };
    let flank_peak = (pitch - 2.0 * flat) * (lo / (lo + hi) - 0.5);
    let mut lobes = Vec::new();
    for spoke in 0..2 * starts {
        let angle = 0.5 * period * f64::from(spoke);
        let x = x * angle.cos() + binormal * angle.sin();
        let phase = 0.5 * f64::from(1 - spoke % 2);
        let first_turn = (-groove.height / pitch - phase - 0.5).ceil() as i32;
        let last_turn = ((finish - groove.height) / pitch - phase + 0.5).floor() as i32;
        for turn in first_turn..=last_turn {
            let center = groove.height + pitch * (f64::from(turn) + phase);
            let peak = center + dir * flank_peak;
            let search = [
                (center - 0.5 * pitch).max(0.0),
                (center + 0.5 * pitch).min(finish),
            ];
            if search[0] >= search[1] {
                continue;
            }
            // Each bisector sees a mirrored pair of flanks on either side.
            // At any radius the tighter pair sets the crest's half-width.
            let gap = |h: f64| {
                let shift = dir * (h - center);
                [pitch * 0.5 + shift - flat, pitch * 0.5 - shift - flat]
            };
            let top = |h| {
                let [a, b] = gap(h);
                baseline(h) + (a / lo).min(b / hi)
            };
            let exposed = |h| top(h) - (root(h) - 0.22 * skirt_depth(h));
            let [low, high] = search;
            let mid = peak.max(low).min(high);
            let (Some(a), Some(b)) = (
                crossed_lobe_end(low, mid, exposed),
                crossed_lobe_end(high, mid, exposed),
            ) else {
                continue;
            };
            let apex = (a <= 1e-9 && groove.tip_radius <= 1e-6).then(|| point(0.0));
            let first = if apex.is_some() {
                (0.012 * pitch).max(0.015).min(b * 0.1)
            } else {
                a
            };
            let mut splits = [first, peak, groove.height, b]
                .into_iter()
                .filter(|h| *h >= first && *h <= b)
                .collect::<Vec<_>>();
            splits.sort_by(f64::total_cmp);
            splits.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
            let mut pieces = Vec::new();
            for pair in splits.windows(2) {
                let [a, b] = pair else { continue };
                let (a, b) = (*a, *b);
                // A short lobe's buried cap can have edges near the loft's
                // absolute stitching tolerance. Build it at pitch scale and
                // return it to part coordinates before joining it to the core.
                let span = b - a;
                let magnification = if span < 0.025 * pitch {
                    (pitch / span).min(100.0)
                } else {
                    1.0
                };
                let origin = if magnification > 1.0 {
                    point(a)
                } else {
                    Vec3::ZERO
                };
                let local = |p| (p - origin) * magnification;
                let sections = (0..=8)
                    .map(|i| {
                        let h = a + span * f64::from(i) / 8.0;
                        let skirt = root(h) - skirt_depth(h);
                        let section = crossed_lobe_section(
                            skirt * magnification,
                            baseline(h) * magnification,
                            gap(h).map(|g| g * magnification),
                            tangents,
                            scale / magnification,
                        );
                        Profile {
                            frame: frame(local(point(h)), x, axis.cross(x)),
                            outline: section,
                        }
                    })
                    .collect::<Vec<_>>();
                let placement = (magnification > 1.0).then(|| {
                    let s = magnification.recip();
                    [
                        [s, 0.0, 0.0, 0.0],
                        [0.0, s, 0.0, 0.0],
                        [0.0, 0.0, s, 0.0],
                        [origin.x, origin.y, origin.z, 1.0],
                    ]
                });
                pieces.push(Loft {
                    sections,
                    degree,
                    apex: apex.filter(|_| a == first).map(local).map(Vec3::to_array),
                    placement,
                });
            }
            if !pieces.is_empty() {
                lobes.push(pieces);
            }
        }
    }
    let margin = 0.055 * pitch;
    let stock = meridian_profile(
        meridian,
        polygon(&[
            Vec2::new(0.0, -margin),
            Vec2::new(groove.tip_radius + margin, -margin),
            Vec2::new(groove.tip_radius + margin, 0.0),
            Vec2::new(groove.stock(groove.height) + margin, groove.height),
            Vec2::new(groove.stock(finish) + margin, finish),
            Vec2::new(0.0, finish),
        ]),
    )?;
    Ok(Crossed {
        core,
        lobes,
        stock,
        // Match the positive turn about the axis of the oriented meridian.
        axis: (-y).to_array(),
    })
}

/// Near the axis a full-size meridian cutter would pass through the axis.
/// Build the groove from sections normal to the axis instead. Each tooth
/// keeps its angular opening as its radial height approaches zero.
fn transverse_grooves(t: &ConicalThread, shank: ShankSpan, tangents: [f64; 2]) -> Result<Grooves> {
    let ConicalThread {
        radius,
        depth,
        flat,
        slope,
        wide,
        ..
    } = *t;
    let (tip, tip_radius, height, dir, fixed_end, end_at) = (
        shank.tip,
        shank.tip_radius,
        shank.height,
        shank.dir,
        shank.full,
        shank.last,
    );
    let (pitch, starts) = (t.pitch, t.starts.max(1));
    let lead = pitch * f64::from(starts);
    // Keep the end of the ruled apex cap large enough for its trimmed
    // faces to tessellate on fine pitches as well.
    let sharp = tip_radius <= 1e-6;
    let groove = PointGroove {
        tip_radius,
        height,
        slope: slope.abs(),
        depth,
        fade: shank.fade,
        runout: (fixed_end < end_at).then_some((fixed_end, end_at)),
        pitch,
    };
    let first = if sharp {
        (0.012 * pitch).max(0.015)
    } else {
        0.0
    };
    let finish = end_at + if shank.free { 0.016 * pitch } else { 0.0 };
    if height <= first || finish <= height || (finish - first) / lead > 190.0 || radius <= depth {
        return Err(invalid(
            "the screw point or its thread length does not fit the groove near the axis",
        ));
    }
    let steps = |a: f64, b: f64| (((b - a) / lead * 24.0).ceil() as u32).max(3);
    let stations = |a, b, first_index| {
        let count = steps(a, b);
        (first_index..=count).map(move |i| a + (b - a) * f64::from(i) / f64::from(count))
    };
    let k = Vec3::from_array(t.axis);
    let x = across(k)?;
    let y = k.cross(x);
    let axis_at = |z| Vec3::from_array(t.axis_point) + k * z;
    let span = point_blend_width(height, pitch);
    if t.hand == ThreadHand::Both {
        let plane = frame(axis_at(tip), x, k * dir);
        return Ok(Grooves::Crossed(crossed_point_groove(
            t, &groove, plane, finish, dir, tangents,
        )?));
    }
    let hand = if t.hand == ThreadHand::Left {
        -1.0
    } else {
        1.0
    };
    let scale = std::f64::consts::TAU / lead;
    let mut tools = Vec::new();
    let [lo, hi] = if hand > 0.0 {
        tangents
    } else {
        [tangents[1], tangents[0]]
    };
    let clearance = ((0.98 * pitch - 2.0 * flat) / (lo + hi) - depth).min(0.045 * pitch);
    for start in 0..starts {
        let mut splits = vec![height, finish];
        if fixed_end > height && fixed_end < finish && fixed_end < end_at {
            splits.push(fixed_end);
        }
        if let Some(h) = groove
            .fade
            .and_then(PointFade::shank_end)
            .filter(|h| *h > height && *h < finish)
        {
            splits.push(h);
        }
        splits.sort_by(f64::total_cmp);
        splits.dedup();
        let sections = stations(first, height, 0)
            .chain(
                splits
                    .windows(2)
                    .filter_map(|pair| match pair {
                        [a, b] => Some(stations(*a, *b, 1)),
                        _ => None,
                    })
                    .flatten(),
            )
            .map(|h| {
                let z = tip + dir * h;
                let phase = scale * (hand * (z - wide) + f64::from(start) * pitch);
                let u = x * phase.cos() + y * phase.sin();
                let fraction = point_rim_blend(h, height, span) / height;
                let stock = if sharp {
                    radius * fraction
                } else {
                    tip_radius + (radius - tip_radius) * fraction
                };
                let d = groove.depth_at(h);
                let runout = groove.runout_at(h);
                // The cutter stays open outside a flat tip where its depth
                // reaches zero. A sharp tip's cutter collapses onto the pole.
                let open = if sharp {
                    d
                } else {
                    d + tip_radius.min(depth) * (1.0 - fraction).max(0.0)
                };
                let overshoot = clearance * (1.0 - (-10.0 * open).exp());
                let extents = |tan| {
                    [
                        (flat + (depth * runout - d * runout) * tan) * scale,
                        (flat + (depth * runout + overshoot) * tan) * scale,
                    ]
                };
                let section = point_groove_section(
                    stock - d * runout,
                    stock + overshoot,
                    extents(lo),
                    extents(hi),
                );
                Profile {
                    frame: frame(axis_at(z), u, k.cross(u)),
                    outline: section,
                }
            })
            .collect::<Vec<_>>();
        tools.push(Loft {
            sections,
            degree: 3,
            apex: sharp.then(|| axis_at(tip).to_array()),
            placement: None,
        });
    }
    Ok(Grooves::Lofted(tools))
}

/// Design a conical groove cutter from an oriented axis and a thread profile.
///
/// # Errors
/// Refuses an invalid profile, a cutter that reaches the axis at full depth,
/// or a cut whose length or slope makes it incompatible with the profile.
pub fn grooves(t: &ConicalThread) -> std::result::Result<Grooves, PlanError> {
    let ConicalThread {
        pitch,
        depth,
        flat,
        slope,
        wide,
        radius,
        ..
    } = *t;
    let lead = pitch * f64::from(t.starts.max(1));
    let [flank_lo, flank_hi] = t.flanks;
    let finite = [
        pitch, depth, flat, slope, wide, radius, t.small, t.span.0, t.span.1, flank_lo, flank_hi,
    ]
    .into_iter()
    .chain(t.axis)
    .chain(t.axis_point)
    .all(f64::is_finite);
    if !(finite
        && pitch > 1e-3
        && depth > 0.0
        && flat > 0.0
        && radius > depth
        && slope != 0.0
        && t.starts <= 16
        && t.span.0 < t.span.1)
    {
        return Err(invalid("invalid cone or thread profile").into());
    }
    let limiting_flank = if slope < 0.0 { flank_lo } else { flank_hi };
    let limit = std::f64::consts::FRAC_PI_2 - limiting_flank;
    if slope.abs().atan() >= limit {
        return Err(PlanError::TooSteep(limit.to_degrees()));
    }
    let margin = 0.025 * pitch;
    let outer = depth + margin;
    let (left, right) = (flat + outer * flank_lo.tan(), flat + outer * flank_hi.tan());
    if !(left + right < pitch * 0.98) {
        return Err(
            invalid("the thread profile does not fit between its turns on this cone").into(),
        );
    }
    let r = |z: f64| radius + (z - wide) * slope;
    let (base, tip) = if t.shank.is_some() {
        (wide, t.small)
    } else if slope < 0.0 {
        t.span
    } else {
        (t.span.1, t.span.0)
    };
    let shank = t
        .shank
        .map(|end| shank_span(t, end, if wide == t.span.0 { right } else { left }))
        .transpose()?;
    let mut spans = shank
        .as_ref()
        .map_or_else(Vec::new, |s| s.root_spans(radius, depth));
    let width = left + right;
    let slope_of = |s: &RootSpan| (s.root_high - s.root_low) / (s.high - s.low);
    let bite = 1.02
        * spans
            .iter()
            .map(|span| slope_of(span).abs())
            .fold(slope.abs(), f64::max)
        * flat
        + 0.001 * pitch;
    let fade = shank.and_then(|s| s.fade);
    let exact_root = shank.is_some();
    // A transverse cutter can stay clear of the axis even where the
    // wider swept cutter would extend into it across a flank.
    let tip_root = r(tip) - depth;
    let sweep_clearance = bite.max(slope.abs() * width + 2e-6);
    if let Some(shank) =
        shank.filter(|_| tip_root <= sweep_clearance && (fade.is_some() || tip_root > 2e-6))
    {
        return Ok(transverse_grooves(
            t,
            shank,
            [flank_hi.tan(), flank_lo.tan()],
        )?);
    }
    if let Some(fade) = fade {
        let dir = (base - tip).signum();
        let height = (base - tip).abs();
        let knee = match fade {
            PointFade::Point { end, .. } => end,
            PointFade::Shank { .. } => height,
        };
        let z = if knee == height {
            base
        } else {
            tip + dir * knee
        };
        let root = r(z) - depth * fade.fraction(knee);
        if z != base {
            spans.push(RootSpan::new((base, radius - depth), (z, root)));
        }
        spans.push(RootSpan::new((z, root), (tip, r(tip))));
    } else {
        let margin = if exact_root {
            2e-6
        } else {
            slope.abs() * width + 2e-6
        };
        if r(tip) - depth <= margin {
            return Err(invalid(if exact_root {
                "the full groove depth would reach the screw point's axis; choose a shallower thread or fade the point depth"
            } else {
                "the full groove depth would reach the cone's axis; shorten the thread or choose a shallower thread"
            }).into());
        }
        spans.push(RootSpan::new(
            (base, r(base) - depth),
            (tip, r(tip) - depth),
        ));
    }
    spans.sort_by(|a, b| a.low.total_cmp(&b.low));
    let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
        return Err(invalid("the thread is too short").into());
    };
    if (last.high - first.low) / lead > 190.0 {
        return Err(invalid("a modelled thread is limited to 190 turns").into());
    }

    let (begin, finish) = (
        first.low - right - 0.001 * pitch,
        last.high + left + 0.001 * pitch,
    );
    let turns = (finish - begin) / lead;
    let tool_root = |z, root: f64| {
        if fade.is_some() && z == tip {
            r(z) - depth
        } else {
            root.max(2e-6)
        }
    };
    let (root, shear, law) = if exact_root {
        // One continuous sweep for the shank and point. Its root follows the
        // piecewise-linear radius at the centre of the profile's root flat.
        // Hold the law outside the part so the cutter stays clear of the axis.
        let root = tool_root(first.low, first.root_low);
        let law = std::iter::once((0.0, 0.0))
            .chain(std::iter::once(((first.low - begin) / lead, 0.0)))
            .chain(spans.iter().map(|span| {
                (
                    (span.high - begin) / lead,
                    tool_root(span.high, span.root_high) - root,
                )
            }))
            .chain(std::iter::once((
                turns,
                tool_root(last.high, last.root_high) - root,
            )))
            .collect::<Vec<_>>();
        (root, 0.0, law)
    } else {
        let slope = slope_of(first);
        let root = first.root_low + slope * (begin - first.low);
        // For a cone alone the same shear everywhere keeps R(z) - depth
        // across the whole root flat, in addition to its centre.
        (
            root,
            slope,
            vec![(0.0, 0.0), (turns, slope * (finish - begin))],
        )
    };
    // The radial law places the profile's centre. Across its flat, the
    // local radius may vary with the axial offset. Allow the cutter to
    // reach farther in there, then let the core set the exact root.
    let mut meridian = Vec::new();
    if exact_root {
        meridian.push((-flat, -bite));
    }
    meridian.extend([(-flat, 0.0), (-left, outer)]);
    if exact_root {
        // The wider side of the stock can be farther out than the radius at the
        // centre of the profile. Keep both outer corners above the surface,
        // without making the cutter wider than a pitch.
        let clear = outer + slope.abs() * left.max(right);
        meridian.extend([(-left, clear), (right, clear)]);
    }
    meridian.extend([(right, outer), (flat, 0.0)]);
    if exact_root {
        meridian.push((flat, -bite));
    }
    let section = meridian
        .into_iter()
        .map(|(z, offset)| Vec2::new(root + shear * z + offset, z))
        .collect::<Vec<_>>();
    let k = Vec3::from_array(t.axis);
    let u = across(k)?;
    let hands: &[f64] = match t.hand {
        ThreadHand::Right => &[1.0],
        ThreadHand::Left => &[-1.0],
        ThreadHand::Both => &[1.0, -1.0],
    };
    let count = t.starts.max(1);
    let axis_point = Vec3::from_array(t.axis_point) + k * begin;
    let mut tools = Vec::new();
    for &hand in hands {
        for start in 0..count {
            let phase = std::f64::consts::TAU
                * (hand * (begin - wide) / lead + f64::from(start) / f64::from(count));
            let x = u * phase.cos() + k.cross(u) * phase.sin();
            tools.push(Screw {
                profile: Profile {
                    frame: frame(axis_point, x, k),
                    outline: polygon(&section),
                },
                axis: (k * hand).to_array(),
                lead: lead * hand,
            });
        }
    }
    let lo = begin - pitch;
    let hi = finish + pitch;
    let profile = [
        Vec2::new(0.0, lo),
        Vec2::new(first.root_low, lo),
        Vec2::new(first.root_low, first.low),
    ]
    .into_iter()
    .chain(
        spans
            .iter()
            .map(|span| Vec2::new(span.root_high, span.high)),
    )
    .chain([Vec2::new(last.root_high, hi), Vec2::new(0.0, hi)])
    .collect::<Vec<_>>();
    let core = exact_root.then(|| Profile {
        frame: frame(Vec3::from_array(t.axis_point), u, k),
        outline: polygon(&profile),
    });
    let trim = [first.low, last.high]
        .map(|z| frame(Vec3::from_array(t.axis_point) + k * z, u, k.cross(u)));
    Ok(Grooves::Swept(Swept {
        tools,
        turns,
        law,
        trim,
        core,
    }))
}
