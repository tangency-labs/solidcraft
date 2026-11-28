//! Primitive solid creation (`SmPrimitiveCreation`): shared option types and
//! the creation state and path helpers in [`p1`], partial primitives, tapered
//! extrusions, curve, pipe and planar-path sweeps, skins and face blends in [`p2`],
//! and sweeps and skinning in [`p3`]. Topology assembly still awaits the B-rep
//! construction units.
//!
//! Upstream: `source/SMLib/inc/SmPrimitiveCreation.h`.
//!
//! | Rust              | Upstream          |
//! |-------------------|-------------------|
//! | [`SweepOptions`]  | `SmSweepOptions`  |
//!
//! Deviations: co-ported by `sweep.api_brep` (only `SmSweepOptions`); `SmBoolean`
//! flags are `bool`. [`SweepOptions::profile_at_path_start`] is the SolidCraft fork's
//! `bProfileAtPathStart` (tangency-labs/smlib 2dd71a0, WR-522, #1450), off by default;
//! [`SweepOptions::up`] the fork's upright sweep (no SMLib counterpart), `None` by default.

pub mod model;
pub mod p1;
pub mod p2;
pub mod p3;

/// Independent flags of a curve sweep (`SmSweepOptions`); every flag defaults to
/// `false`, as upstream's member initialisers (and [`SweepOptions::up`] to `None`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // upstream's five SmBoolean members and the fork's one
pub struct SweepOptions {
    /// `bCapEndsArg`: cap both ends of the result (only for a closed profile).
    pub cap_ends: bool,
    /// `bTranslationalSweep`: the profile only translates along the path and keeps
    /// its orientation.
    pub translational_sweep: bool,
    /// `bMoveProfileCenterToPath`: move the profile so that its center is at the
    /// start of the path.
    pub move_profile_center_to_path: bool,
    /// `bOrientProfilePerpendicularToPath`: orient the profile perpendicular to
    /// the path at its start.
    pub orient_profile_perpendicular_to_path: bool,
    /// `bMoveToProfile`: move the result back to the original position of the
    /// profile (otherwise it lies at the path).
    pub move_to_profile: bool,
    /// `bProfileAtPathStart` (the SolidCraft fork's option, tangency-labs/smlib
    /// 2dd71a0, #1450; not in SMLib 1.0.1): sweep the profile where it lies. The
    /// path's start is its reference point, so the profile keeps its place relative to
    /// the path (`move_profile_center_to_path` and `move_to_profile` then change
    /// nothing). Off, as upstream has no such option: the profile's reference point
    /// (its first curve's start, or its box centre) is moved onto the path's start.
    pub profile_at_path_start: bool,
    /// Not in SMLib (the SolidCraft fork's upright sweep): keep the profile upright, in the
    /// global system: at every section the profile's frame keeps this direction made square
    /// to the path's tangent, so the profile turns only about it as the path turns (a screen
    /// or a canopy along a closed outline). `None`: the projection normal frame (upstream's).
    /// A path running along it is refused; a translational sweep ignores it.
    pub up: Option<crate::math::Vec3>,
}

// Supported public paths; numbered modules are implementation partitions.
pub use p1::{create_rectangle, Boolean2d, PrimitiveCreation};
