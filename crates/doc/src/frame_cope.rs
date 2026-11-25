//! Retained round-tube end cuts and their cylindrical developments: the record and its
//! certificate. Every quantity (how a mate fits an end, how far a cut reaches, the unwrapped cut)
//! is the kernel's, through `solidcore_ops::tube_cope`. No display mesh is used.

use serde::{Deserialize, Serialize};
use solidcore_ops::tube_cope::{self as ops, Refusal, TemplateRefusal};
use solidcore_record::{Vec2, Vec3};

use crate::{DocError, ModelBody, Result};

/// Most mating members one cope may cut against.
pub const MAX_CUTTERS: usize = 16;
/// Most segments an exported template profile may have.
pub const MAX_TEMPLATE_SEGMENTS: usize = 65_536;

/// Which end of the original ordered member path a cope treats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Start,
    End,
}

impl End {
    /// The word commands and messages use for this end: `start` or `end`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

/// An axis-aligned box, as the kernel gave it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl From<ops::Box3> for Bounds {
    fn from(b: ops::Box3) -> Self {
        Self { min: b.min, max: b.max }
    }
}

/// Local datum at an uncoped stock end's section centre; positive station goes into the member.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Terminal {
    pub origin: Vec3,
    pub axis: Vec3,
    pub x: Vec3,
    pub y: Vec3,
    pub length: f64,
    /// Conservative bounds of all the other swept path pieces, including their section.
    pub other: Vec<Bounds>,
}

impl Terminal {
    /// The kernel's terminal, with the bounds of the member's other path pieces.
    pub(crate) fn new(t: ops::Terminal, other: Vec<Bounds>) -> Self {
        Self { origin: t.origin, axis: t.axis, x: t.x, y: t.y, length: t.length, other }
    }

    fn ops(&self) -> ops::Terminal {
        ops::Terminal { origin: self.origin, axis: self.axis, x: self.x, y: self.y, length: self.length }
    }
}

/// A mating member's outside cylinder, the retained cutter of a cope.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cylinder {
    pub origin: Vec3,
    pub axis: Vec3,
    pub radius: f64,
    pub length: f64,
}

/// A cutter's member, for refusals, and how far its own retained copes reach in from its start
/// and end (0 where an end is not coped).
#[derive(Clone, Debug, Default)]
pub struct Mate {
    pub name: String,
    pub start: f64,
    pub end: f64,
}

/// One retained end cut: the treated end, its cutters and how far it reaches into the stock.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cut {
    pub end: End,
    pub cylinders: Vec<Cylinder>,
    pub depth_bound: f64,
}

/// Evaluated physical-member data. Only member creation and retained coping produce it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoundMember {
    pub radius: f64,
    pub wall: f64,
    pub start: Option<Terminal>,
    pub end: Option<Terminal>,
    pub straight: bool,
    pub cuts: Vec<Cut>,
}

/// A retained cut unwrapped onto the tube's outside: x is arc length on the OD from the seam,
/// y the station into the stock; the seam's radial direction and the circumference.
#[derive(Clone, Debug)]
pub struct Unwrapped {
    pub points: Vec<Vec2>,
    pub seam_direction: Vec3,
    pub circumference: f64,
    /// The tube's outside diameter (mm), as the kernel gave it.
    pub outer_diameter: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct CertifiedMember {
    pub shape: solidcraft_kernel::Body,
    pub data: RoundMember,
}

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

/// The refusal a person reads for a mate the kernel will not use, naming the mate and what failed.
fn refusal(r: Refusal, end: End, name: &str) -> DocError {
    invalid(match r {
        Refusal::OffAxis { distance } => format!(
            "not supported yet: the {} end is {distance:.6} mm from the axis of `{name}`; a cope needs the member drawn to its mate's axis, not its wall",
            end.label()
        ),
        Refusal::Smaller { mate_diameter, tube_diameter } => format!(
            "not supported yet: `{name}` (OD {mate_diameter:.3} mm) is smaller than this member (OD {tube_diameter:.3} mm); a mate's OD must be at least the member's"
        ),
        Refusal::Parallel { sine } => {
            format!("not supported yet: `{name}` runs almost parallel to this member (joint sine {sine:.6}, at least {} needed)", ops::MIN_SINE)
        }
        Refusal::PastTerminal => "the cope reaches the next bend or the other end".into(),
        Refusal::MateEnd => "not supported yet: a mating tube end cap may intersect the cope; use a longer straight mating member".into(),
        Refusal::MateCope => {
            format!("not supported yet: this joint reaches a coped end of `{name}`; keep the joint clear of that cope or cope this member first")
        }
        Refusal::MeetsOther => "not supported yet: the cutter may also meet a bend or another portion of this member".into(),
    })
}

impl RoundMember {
    /// The straight cylindrical terminal at `end`; an end on a bend has none and is refused.
    pub fn terminal(&self, end: End) -> Result<&Terminal> {
        match end {
            End::Start => &self.start,
            End::End => &self.end,
        }
        .as_ref()
        .ok_or_else(|| invalid("not supported yet: this end has no straight cylindrical terminal"))
    }

    /// Validate cache data before any allocation driven by its counts or numeric values.
    pub(crate) fn validate(&self) -> Result<()> {
        if !self.radius.is_finite()
            || !self.wall.is_finite()
            || !(0.005..=5000.0).contains(&self.radius)
            || self.wall <= 0.0
            || self.wall >= self.radius
            || self.cuts.len() > 2
        {
            return Err(invalid("invalid round-member record"));
        }
        for t in [&self.start, &self.end].into_iter().flatten() {
            let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
            if !ops::terminal_ok(&t.ops())
                || t.other.len() > crate::frame::MAX_MEMBERS
                || t.other.iter().any(|b| !finite(b.min) || !finite(b.max) || b.min.x > b.max.x || b.min.y > b.max.y || b.min.z > b.max.z)
            {
                return Err(invalid("invalid cylindrical terminal record"));
            }
        }
        for cut in &self.cuts {
            if cut.cylinders.is_empty() || cut.cylinders.len() > MAX_CUTTERS || !cut.depth_bound.is_finite() || cut.depth_bound <= 0.0 {
                return Err(invalid("invalid retained cope record"));
            }
            for c in &cut.cylinders {
                c.validate()?;
            }
        }
        Ok(())
    }

    /// Prepare a cut only when every cutter covers the original end disk. The entire removed
    /// interval then connects to that end, and the exit envelope selects the surviving side.
    /// A smaller/off-axis mate can leave holes or several branches; refuse it explicitly.
    /// `mates` names each cutter's member and how far its own copes reach in from each end.
    pub fn prepare(&self, end: End, mut cylinders: Vec<Cylinder>, mates: &[Mate]) -> Result<Cut> {
        self.validate()?;
        if cylinders.is_empty() || cylinders.len() > MAX_CUTTERS || mates.len() != cylinders.len() || self.cuts.iter().any(|c| c.end == end) {
            return Err(invalid("a cope needs 1..16 cutters and an end without an existing cope (edit its feature instead)"));
        }
        let t = self.terminal(end)?;
        let others: Vec<ops::Box3> = t.other.iter().map(|b| ops::Box3 { min: b.min, max: b.max }).collect();
        let mut depth = 0.0_f64;
        for (c, mate) in cylinders.iter_mut().zip(mates) {
            c.validate()?;
            let cutter =
                ops::Mate { origin: c.origin, axis: c.axis, radius: c.radius, length: c.length, coped_start: mate.start, coped_end: mate.end };
            let fit = ops::fit(&t.ops(), self.radius, &cutter, &others)
                .map_err(|e| invalid(format!("the cope could not be fitted: {e}")))?
                .map_err(|r| refusal(r, end, &mate.name))?;
            // The retained cutter is about the exact joint datum: the kernel removed only
            // sub-micrometre sketch roundoff, and both the solid cut and the template use it.
            c.origin = fit.origin;
            depth = depth.max(fit.depth);
        }
        if self.straight && self.cuts.iter().any(|c| ops::ends_cross(c.depth_bound, depth, t.length)) {
            return Err(invalid("the two end treatments may cross"));
        }
        Ok(Cut { end, cylinders, depth_bound: depth })
    }

    /// The retained cut at `end` unwrapped from `seam` (radians from the end's x) within
    /// `tolerance` mm, by the kernel's bounded uniform sampling (cutter switches and equal-radius
    /// cusps included).
    pub fn template(&self, end: End, seam: f64, tolerance: f64) -> Result<Unwrapped> {
        self.validate()?;
        if !seam.is_finite() || seam.abs() > 1e6 || !tolerance.is_finite() || !(0.001..=1.0).contains(&tolerance) {
            return Err(invalid("seam must be a finite angle within +/-1e6 rad; tolerance must be 0.001..1 mm"));
        }
        let t = self.terminal(end)?.ops();
        let cut = self.cuts.iter().find(|c| c.end == end).ok_or_else(|| invalid("this member end has no retained cope"))?;
        let mates: Vec<(Vec3, f64)> = cut.cylinders.iter().map(|c| (c.axis, c.radius)).collect();
        let cope = ops::Cope { end: &t, radius: self.radius, mates: &mates, depth: cut.depth_bound };
        let watcher = solidcraft_kernel::watch::current();
        let cancelled = || watcher.as_ref().is_some_and(|w| w.cancelled());
        let unwrapped = ops::template(&cope, seam, tolerance, MAX_TEMPLATE_SEGMENTS, &cancelled)
            .map_err(|_| invalid("invalid retained cope geometry"))?
            .map_err(|r| {
                invalid(match r {
                    TemplateRefusal::TooManySegments => {
                        format!("the sampling bound needs more than {MAX_TEMPLATE_SEGMENTS} segments; increase tolerance")
                    }
                    TemplateRefusal::OutsideDepth => "the cope station is outside its retained terminal".into(),
                    TemplateRefusal::Cancelled => "cope template cancelled".into(),
                })
            })?;
        Ok(Unwrapped {
            points: unwrapped.points.into_iter().map(|[x, y]| Vec2::new(x, y)).collect(),
            seam_direction: unwrapped.seam_direction,
            circumference: unwrapped.circumference,
            outer_diameter: unwrapped.outer_diameter,
        })
    }
}

impl Cylinder {
    fn validate(&self) -> Result<()> {
        let cutter = ops::Mate { origin: self.origin, axis: self.axis, radius: self.radius, length: self.length, coped_start: 0.0, coped_end: 0.0 };
        if !ops::mate_ok(&cutter) {
            return Err(invalid("invalid cope cutter cylinder"));
        }
        Ok(())
    }
}

impl ModelBody {
    /// A physical round tube whose geometry has not been changed outside the retained member
    /// operations. Equality of bounds or face counts is deliberately insufficient.
    pub fn round_member(&self) -> Result<&RoundMember> {
        let record = self.round_member.as_ref().filter(|r| !self.transferred && r.shape.same_shape(&self.body));
        let data = &record.ok_or_else(|| invalid("not supported yet: select an unmodified round-tube frame member with corner None and no finishing options, or a retained cope result"))?.data;
        data.validate()?;
        Ok(data)
    }

    pub(crate) fn certify_round_member(&mut self, data: RoundMember) {
        self.round_member = Some(std::sync::Arc::new(CertifiedMember { shape: self.body.clone(), data }));
    }
}

/// Only a frame with no finishing route can certify its emitted bodies as pristine stock.
/// Auto/cornered/fitted frames still build normally, but cannot supply nominal cope datums.
pub(crate) fn stock_eligible(kind: &crate::FeatureKind) -> bool {
    matches!(kind, crate::FeatureKind::FrameMember { corner, corners, fit_ends, coped, flip, gap, .. }
        if corner.is_none() && corners.is_empty() && !*fit_ends && !*coped && !*flip && gap.is_none())
}
