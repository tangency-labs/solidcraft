//! The router's frame geometry ([`solidcraft_kernel_api::Frames`]): frame-stock paths, member
//! trims and gusset plates, their values the record's (`solidcore_record::frame`).

use solidcore_record::frame::{Axis, Cut, Dimensions, End, Extent, MemberData, PieceData, Placement, Prepared, SupportData, SupportFrames};
use solidcore_record::{Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::{Frames, GussetFailure, MemberRefusal, OpKind, PathFailure, TrimRefusal, TrimTarget};

use super::{Which, which};
use crate::KernelError;
use crate::backend::Solidcore;

/// The most a stock member may lean from vertical and still have its section follow world up:
/// the kernel's. This build holds solidcore only, so it is solidcore's.
pub const FRAME_VERTICAL_TOLERANCE: f64 = <Solidcore as Frames>::VERTICAL_TOLERANCE;

/// The farthest (mm) a member's end may move from its line's start: solidcore's.
pub const FRAME_MAX_REACH: f64 = <Solidcore as Frames>::MAX_REACH;

/// One finite straight piece of stock from `a` to `b`.
pub fn stock_line(a: Vec3, b: Vec3) -> Result<PieceData, PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.stock_line(a, b),
    }
}

/// One open spatial arc from `a` to `b` on the circle of `center`, unit `normal` and `radius`.
pub fn stock_spatial_arc(a: Vec3, b: Vec3, center: Vec3, normal: Vec3, radius: f64) -> Result<PieceData, PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.stock_spatial_arc(a, b, center, normal, radius),
    }
}

/// A sketch line or arc on `plane`, placed in world coordinates.
pub fn stock_planar(plane: &Plane, segment: &Seg2) -> Result<PieceData, PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.stock_planar(plane, segment),
    }
}

/// The line from `start` to `end` over its parameter `interval`.
pub fn stock_retained_line(start: Vec3, end: Vec3, interval: [f64; 2]) -> Result<PieceData, PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.stock_retained_line(start, end, interval),
    }
}

/// The length of `piece` when it is a line; `None` for a bend.
pub fn stock_line_length(piece: &PieceData) -> Result<Option<f64>, PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.stock_line_length(piece),
    }
}

/// `pieces` turned as needed to run along one open tangent chain in the order given; unchanged
/// on a refusal.
pub fn orient_stock(pieces: &mut [PieceData]) -> Result<(), PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.orient_stock(pieces),
    }
}

/// A section's axes for a member along `direction`, y as near world up as it can be, turned by
/// `turn` about the path.
pub fn member_axes(direction: Vec3, turn: f64) -> Result<Option<(Vec3, Vec3)>, PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.member_axes(direction, turn),
    }
}

/// The section plane at the start of `first`, anchored at `at` and turned by `turn`.
pub fn stock_section_plane(first: &PieceData, at: Vec2, turn: f64) -> Result<Plane, PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.stock_section_plane(first, at, turn),
    }
}

/// Whether `region`, anchored at `at` and turned by `turn`, clears every bend of `path`.
pub fn check_bend_clearance(path: &[PieceData], region: &Region2, at: Vec2, turn: f64) -> Result<(), PathFailure> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.check_bend_clearance(path, region, at, turn),
    }
}

/// The member along the line from `start` to `end`, its section in `section` with the box
/// `lo`..`hi`.
pub fn member_along(section: &Plane, start: Vec3, end: Vec3, lo: Vec2, hi: Vec2) -> Result<MemberData, MemberRefusal> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.member_along(section, start, end, lo, hi),
    }
}

/// The middle line of `member` and where its `ends` cross it.
pub fn member_axis(member: &MemberData, ends: &[Option<Plane>; 2]) -> Result<Axis, MemberRefusal> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.member_axis(member, ends),
    }
}

/// The range `member`'s section is extruded over for its `ends`, and which ends are cut at a
/// slant.
pub fn member_extent(member: &MemberData, ends: &[Option<Plane>; 2]) -> Result<Extent, MemberRefusal> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.member_extent(member, ends),
    }
}

/// Where the ends of `member` are cut after trimming it to `targets`.
pub fn trim_member(member: &MemberData, ends: &[Option<Plane>; 2], end: End, targets: &[TrimTarget<'_>], gap: f64) -> Result<[Option<Cut>; 2], TrimRefusal> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.trim_member(member, ends, end, targets, gap),
    }
}

/// The plane a gusset support's faces share.
pub fn gusset_support_plane(first: Option<&Plane>, candidate: &Plane, size: f64, tolerance: f64) -> Result<Plane, GussetFailure<KernelError>> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.gusset_support_plane(first, candidate, size, tolerance),
    }
}

/// A gusset support's material from its faces' trimmed loops.
pub fn gusset_support(frames: &SupportFrames, faces: &[Vec<Vec<Seg2>>]) -> Result<SupportData, GussetFailure<KernelError>> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.gusset_support(frames, faces),
    }
}

/// The gusset plate between `supports`.
pub fn gusset_prepare(supports: [&SupportData; 2], dimensions: Dimensions, placement: Placement) -> Result<Prepared, GussetFailure<KernelError>> {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.gusset_prepare(supports, dimensions, placement),
    }
}

/// A saved support pick `at` put back on `plane`.
pub fn gusset_support_pick(plane: &Plane, at: Vec3) -> Vec3 {
    match which(OpKind::Frames) {
        Which::Solidcore => Solidcore.gusset_support_pick(plane, at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::{frame_member as stock, frame_trim as trim};
    use solidcraft_kernel_api::PathRefusal;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    #[test]
    fn routed_stock_paths_are_solidcores() {
        assert_eq!(FRAME_VERTICAL_TOLERANCE, stock::VERTICAL_TOLERANCE);
        assert_eq!(FRAME_MAX_REACH, trim::MAX_REACH);
        let line = stock_line(v(100.0, -200.0, 0.0), v(100.0, 0.0, 0.0)).unwrap();
        assert_eq!(line, stock::line(v(100.0, -200.0, 0.0), v(100.0, 0.0, 0.0)).unwrap().data());
        let arc = stock_spatial_arc(v(100.0, 0.0, 0.0), v(0.0, 100.0, 0.0), Vec3::ZERO, v(0.0, 0.0, 1.0), 100.0).unwrap();
        assert_eq!(stock_line_length(&line).unwrap(), Some(200.0));
        assert_eq!(stock_line_length(&arc).unwrap(), None);
        // picked back to front: both turned, as ops turns them
        let mut path = [arc, line];
        orient_stock(&mut path).unwrap();
        let mut direct = [stock::Piece::from_data(&arc).unwrap(), stock::Piece::from_data(&line).unwrap()];
        stock::orient(&mut direct).unwrap();
        assert_eq!(path, direct.map(|p| p.data()));
        let plane = stock_section_plane(&path[0], Vec2::new(3.0, 7.0), 0.5).unwrap();
        assert_eq!(plane, stock::section_plane(&direct[0], Vec2::new(3.0, 7.0), 0.5).unwrap());
        // a refusal keeps its kind and the kernel's words
        let refused = stock_line(Vec3::ZERO, Vec3::ZERO).unwrap_err();
        let words = stock::line(Vec3::ZERO, Vec3::ZERO).unwrap_err().to_string();
        assert_eq!(refused, PathFailure::Refused { why: PathRefusal::Line, words: words.clone() });
        assert_eq!(refused.to_string(), words);
        let mut gap = [line, stock_line(v(100.0, 10.0, 0.0), v(100.0, 20.0, 0.0)).unwrap()];
        let before = gap;
        let PathFailure::Refused { why, .. } = orient_stock(&mut gap).unwrap_err() else { panic!("a refusal") };
        assert_eq!(why, PathRefusal::Connection(1));
        assert_eq!(gap, before, "a refused traversal leaves its input unchanged");
    }

    #[test]
    fn routed_member_trims_are_solidcores() {
        let section = Plane { origin: v(100.0, 0.0, 0.0), x: v(0.0, 1.0, 0.0), y: v(0.0, 0.0, 1.0) };
        let (lo, hi) = (Vec2::new(-22.5, -22.5), Vec2::new(22.5, 22.5));
        let rail = member_along(&section, v(100.0, 0.0, 0.0), v(300.0, 0.0, 0.0), lo, hi).unwrap();
        let direct = trim::Member::along(&section, v(100.0, 0.0, 0.0), v(300.0, 0.0, 0.0), lo, hi).unwrap();
        assert_eq!(rail, direct.data());
        let across = Plane { origin: v(250.0, 0.0, 0.0), x: v(0.0, 1.0, 0.0), y: v(0.0, 0.0, 1.0) };
        let none = [None, None];
        let cuts = trim_member(&rail, &none, End::Nearer, &[TrimTarget::Plane(&across)], 0.0).unwrap();
        assert_eq!(cuts, trim::trim(&direct, &none, End::Nearer, &[trim::Target::Plane(&across)], 0.0).unwrap());
        let ends = [None, Some(across)];
        assert_eq!(member_axis(&rail, &ends).unwrap(), trim::axis(&direct, &ends).unwrap());
        assert_eq!(member_extent(&rail, &ends).unwrap(), trim::extent(&direct, &ends).unwrap());
        let along = Plane { origin: v(250.0, 0.0, 0.0), x: v(1.0, 0.0, 0.0), y: v(0.0, 0.0, 1.0) };
        assert_eq!(member_axis(&rail, &[None, Some(along)]), Err(MemberRefusal::RunsAlong));
        assert_eq!(trim_member(&rail, &none, End::Nearer, &[TrimTarget::Plane(&along)], 0.0), Err(TrimRefusal::MeetsNone));
    }
}
