//! Primitive solid creation (`SmPrimitiveCreation`): shared option types. The
//! creation methods themselves live in the `p1`..`p3` units (not ported yet).
//!
//! Upstream: `source/SMLib/inc/SmPrimitiveCreation.h`.
//!
//! | Rust              | Upstream          |
//! |-------------------|-------------------|
//! | [`SweepOptions`]  | `SmSweepOptions`  |
//!
//! Deviations: co-ported by `sweep.api_brep` (only `SmSweepOptions`); `SmBoolean`
//! flags are `bool`.

/// Independent flags of a curve sweep (`SmSweepOptions`); every flag defaults to
/// `false`, as upstream's member initialisers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // upstream's five SmBoolean members
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
}
