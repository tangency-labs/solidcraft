//! solidcore's frame geometry ([`Frames`]): each the solidcore-ops call this crate's callers made
//! before the interface existed. A piece, member or support crosses the interface as its data
//! (`solidcore_record::frame`) and is rebuilt into ops' handle from it field for field
//! (`from_data`), so every call is the one made before. Refusals keep the kernel's words whole.

use solidcore_ops::Error;
use solidcore_ops::{frame_gusset as gusset, frame_member as stock, frame_trim as trim};
use solidcore_record::frame::{Axis, Cut, Dimensions, End, Extent, MemberData, PieceData, Placement, Prepared, SupportData, SupportFrames};
use solidcore_record::{Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::{Frames, GussetFailure, MemberRefusal, PathFailure, PathRefusal, TrimRefusal, TrimTarget};

use super::Solidcore;
use crate::KernelError;

/// A stock path's refusal, its kind and the kernel's words.
fn path_refusal(why: &stock::Refusal) -> PathRefusal {
    match *why {
        stock::Refusal::Line => PathRefusal::Line,
        stock::Refusal::Arc => PathRefusal::Arc,
        stock::Refusal::Frame => PathRefusal::Frame,
        stock::Refusal::Connection(i) => PathRefusal::Connection(i),
        stock::Refusal::Tangency(i) => PathRefusal::Tangency(i),
        stock::Refusal::Closed => PathRefusal::Closed,
        stock::Refusal::Bend(i) => PathRefusal::Bend(i),
        stock::Refusal::Profile => PathRefusal::Profile,
        stock::Refusal::Count => PathRefusal::Count,
    }
}

fn path_failure(e: stock::Failure) -> PathFailure {
    match e {
        stock::Failure::Geometry(why) => PathFailure::Refused { why: path_refusal(&why), words: why.to_string() },
        stock::Failure::Kernel(e) => PathFailure::Failed(e.to_string()),
    }
}

fn piece(data: &PieceData) -> Result<stock::Piece, PathFailure> {
    stock::Piece::from_data(data).map_err(path_failure)
}

fn pieces(path: &[PieceData]) -> Result<Vec<stock::Piece>, PathFailure> {
    path.iter().map(piece).collect()
}

fn made(p: Result<stock::Piece, stock::Failure>) -> Result<PieceData, PathFailure> {
    p.map(|p| p.data()).map_err(path_failure)
}

fn member_refusal(why: trim::Refusal) -> MemberRefusal {
    match why {
        trim::Refusal::Degenerate => MemberRefusal::Degenerate,
        trim::Refusal::RunsAlong => MemberRefusal::RunsAlong,
        trim::Refusal::WrongWay => MemberRefusal::WrongWay,
        trim::Refusal::TooFar => MemberRefusal::TooFar,
        trim::Refusal::EndsCross => MemberRefusal::EndsCross,
        trim::Refusal::FacesAway => MemberRefusal::FacesAway,
    }
}

fn member(data: &MemberData) -> Result<trim::Member, MemberRefusal> {
    trim::Member::from_data(data).map_err(member_refusal)
}

/// A gusset's failure, sorted as the gusset feature has always sorted it: a refusal's words as
/// they are, input the kernel takes as invalid, a caught panic or broken invariant as internal,
/// anything else as a failed operation.
fn gusset_failure(error: Error) -> GussetFailure<KernelError> {
    match error {
        Error::Refused { reason, .. } | Error::Unsupported(reason) => GussetFailure::Refused(reason.to_string()),
        Error::Argument | Error::Profile(_) => GussetFailure::Kernel(KernelError::Invalid(error.to_string())),
        Error::Internal(message) => GussetFailure::Kernel(KernelError::Internal(message)),
        error @ Error::Invariant { defect: true, .. } => GussetFailure::Kernel(KernelError::Internal(error.to_string())),
        other => GussetFailure::Kernel(KernelError::Failed(other.to_string())),
    }
}

impl Frames for Solidcore {
    const VERTICAL_TOLERANCE: f64 = stock::VERTICAL_TOLERANCE;
    const MAX_REACH: f64 = trim::MAX_REACH;

    fn stock_line(&self, a: Vec3, b: Vec3) -> Result<PieceData, PathFailure> {
        made(stock::line(a, b))
    }

    fn stock_spatial_arc(&self, a: Vec3, b: Vec3, center: Vec3, normal: Vec3, radius: f64) -> Result<PieceData, PathFailure> {
        made(stock::spatial_arc(a, b, center, normal, radius))
    }

    fn stock_planar(&self, plane: &Plane, segment: &Seg2) -> Result<PieceData, PathFailure> {
        made(stock::planar(plane, segment))
    }

    fn stock_retained_line(&self, start: Vec3, end: Vec3, interval: [f64; 2]) -> Result<PieceData, PathFailure> {
        made(stock::retained_line(start, end, interval))
    }

    fn stock_line_length(&self, p: &PieceData) -> Result<Option<f64>, PathFailure> {
        piece(p)?.line_length().map_err(path_failure)
    }

    fn orient_stock(&self, path: &mut [PieceData]) -> Result<(), PathFailure> {
        let mut oriented = pieces(path)?;
        stock::orient(&mut oriented).map_err(path_failure)?;
        for (d, p) in path.iter_mut().zip(oriented) {
            *d = p.data();
        }
        Ok(())
    }

    fn member_axes(&self, direction: Vec3, turn: f64) -> Result<Option<(Vec3, Vec3)>, PathFailure> {
        stock::member_axes(direction, turn).map_err(path_failure)
    }

    fn stock_section_plane(&self, first: &PieceData, at: Vec2, turn: f64) -> Result<Plane, PathFailure> {
        stock::section_plane(&piece(first)?, at, turn).map_err(path_failure)
    }

    fn check_bend_clearance(&self, path: &[PieceData], region: &Region2, at: Vec2, turn: f64) -> Result<(), PathFailure> {
        stock::check_bend_clearance(&pieces(path)?, region, at, turn).map_err(path_failure)
    }

    fn member_along(&self, section: &Plane, start: Vec3, end: Vec3, lo: Vec2, hi: Vec2) -> Result<MemberData, MemberRefusal> {
        trim::Member::along(section, start, end, lo, hi).map(|m| m.data()).map_err(member_refusal)
    }

    fn member_axis(&self, m: &MemberData, ends: &[Option<Plane>; 2]) -> Result<Axis, MemberRefusal> {
        trim::axis(&member(m)?, ends).map_err(member_refusal)
    }

    fn member_extent(&self, m: &MemberData, ends: &[Option<Plane>; 2]) -> Result<Extent, MemberRefusal> {
        trim::extent(&member(m)?, ends).map_err(member_refusal)
    }

    fn trim_member(
        &self,
        m: &MemberData,
        ends: &[Option<Plane>; 2],
        end: End,
        targets: &[TrimTarget<'_>],
        gap: f64,
    ) -> Result<[Option<Cut>; 2], TrimRefusal> {
        let refused = |target: Option<usize>| move |why| TrimRefusal::Member { target, why };
        let trimmed = member(m).map_err(refused(None))?;
        // The target members rebuilt first, each refusal naming its target as the trim's would.
        let mut members = Vec::with_capacity(targets.len());
        for (k, t) in targets.iter().enumerate() {
            members.push(match t {
                TrimTarget::Member { member: data, .. } => Some(member(data).map_err(refused(Some(k)))?),
                TrimTarget::Plane(_) => None,
            });
        }
        let mut kernel_targets = Vec::with_capacity(targets.len());
        for (k, (t, m)) in targets.iter().zip(&members).enumerate() {
            kernel_targets.push(match (t, m) {
                (TrimTarget::Member { ends, side, .. }, Some(member)) => trim::Target::Member { member, ends, side: *side },
                (TrimTarget::Plane(plane), _) => trim::Target::Plane(plane),
                // Every member target was rebuilt above, in order.
                (TrimTarget::Member { .. }, None) => return Err(refused(Some(k))(MemberRefusal::Degenerate)),
            });
        }
        trim::trim(&trimmed, ends, end, &kernel_targets, gap).map_err(|why| match why {
            trim::TrimRefusal::Member { target, why } => TrimRefusal::Member { target, why: member_refusal(why) },
            trim::TrimRefusal::SameEnd { first, second, end } => TrimRefusal::SameEnd { first, second, end },
            trim::TrimRefusal::MeetsNone => TrimRefusal::MeetsNone,
        })
    }

    fn gusset_support_plane(&self, first: Option<&Plane>, candidate: &Plane, size: f64, tolerance: f64) -> Result<Plane, GussetFailure<KernelError>> {
        gusset::support_plane(first, candidate, size, tolerance).map_err(gusset_failure)
    }

    fn gusset_support(&self, frames: &SupportFrames, faces: &[Vec<Vec<Seg2>>]) -> Result<SupportData, GussetFailure<KernelError>> {
        gusset::support(frames, faces).map(|s| s.data()).map_err(gusset_failure)
    }

    fn gusset_prepare(
        &self,
        [a, b]: [&SupportData; 2],
        dimensions: Dimensions,
        placement: Placement,
    ) -> Result<Prepared, GussetFailure<KernelError>> {
        let a = gusset::Support::from_data(a).map_err(gusset_failure)?;
        let b = gusset::Support::from_data(b).map_err(gusset_failure)?;
        gusset::prepare([&a, &b], dimensions, placement).map_err(gusset_failure)
    }

    fn gusset_support_pick(&self, plane: &Plane, at: Vec3) -> Vec3 {
        gusset::support_pick(plane, at)
    }
}
