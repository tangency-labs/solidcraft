//! The C API's statuses: `SMLIB_OK`, its negative `SMLIB_ERR_*` codes and SMLib's
//! positive `SM_ERR*` statuses, and how a solidcore error maps onto them.
//!
//! Source: tangency-labs/smlib `capi/include/smlib.h` (the `SMLIB_*` codes) and
//! `capi/src/smlib_shim.h` (`status`), replay/1.0.1 at f8fd83e.
//!
//! | Rust | C API |
//! |------|-------|
//! | [`Status`] | the `int32_t` every function returns |
//! | [`Status::from_solid`] | `smlib_shim::status(SmApiStatus)` |
//!
//! Deviations: solidcore errors that carry no SMLib status map to the status their kind
//! stands for: [`SolidError::InvalidInput`] to `SM_ERR_INVALID_INPUT` (1007),
//! [`SolidError::NotFound`], [`SolidError::Degenerate`] and [`SolidError::NotConverged`] to
//! `SM_ERR` (1001), [`SolidError::Unported`] and [`SolidError::Unsupported`] to
//! `SMLIB_ERR_UNSUPPORTED`, and an internal invariant failure to `SMLIB_ERR_EXCEPTION`
//! (what an escaped C++ exception gives). The original error is kept by
//! [`crate::Smlib::last_error`].

use solidcore::error::{SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_OFFSET_WALLS_TOUCH};
use solidcore::SolidError;

/// A failed call's status: never [`Status::OK`] inside an `Err`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Status(i32);

/// The result of an API call: `Err` carries the status the C function returns.
pub type Result<T> = core::result::Result<T, Status>;

/// The fork's `SM_ERR_OFFSET_EDGE_VANISHES` (its `SmMessages.h`).
const SM_ERR_OFFSET_EDGE_VANISHES: i32 = 1043;

impl Status {
    /// `SMLIB_OK`.
    pub const OK: Self = Self(0);
    /// `SMLIB_ERR_EXCEPTION`: SMLib failed internally (a C++ exception; here an internal
    /// invariant of the port).
    pub const ERR_EXCEPTION: Self = Self(-1000);
    /// `SMLIB_ERR_BUFFER`: an output buffer was too small (not returned by this API, whose
    /// outputs are values).
    pub const ERR_BUFFER: Self = Self(-1001);
    /// `SMLIB_ERR_ARG`: an argument was rejected before SMLib saw it.
    pub const ERR_ARG: Self = Self(-1002);
    /// `SMLIB_ERR_EMPTY`: SMLib reported success but returned no body, or an empty one.
    pub const ERR_EMPTY: Self = Self(-1003);
    /// `SMLIB_ERR_NOT_SOLID`: solid inputs gave a result that is not a closed solid.
    pub const ERR_NOT_SOLID: Self = Self(-1100);
    /// `SMLIB_ERR_PICK_NONE`: a selection point selects nothing.
    pub const ERR_PICK_NONE: Self = Self(-1101);
    /// `SMLIB_ERR_PICK_AMBIGUOUS`: a selection point cannot decide.
    pub const ERR_PICK_AMBIGUOUS: Self = Self(-1102);
    /// `SMLIB_ERR_NO_ROOM`: a blend or offset does not fit.
    pub const ERR_NO_ROOM: Self = Self(-1103);
    /// `SMLIB_ERR_NO_CUT`.
    pub const ERR_NO_CUT: Self = Self(-1104);
    /// `SMLIB_ERR_UNSUPPORTED`: a case SMLib (or this port) does not handle.
    pub const ERR_UNSUPPORTED: Self = Self(-1105);
    /// `SMLIB_ERR_NOT_EXACT`.
    pub const ERR_NOT_EXACT: Self = Self(-1106);
    /// `SMLIB_ERR_NO_EFFECT`: SMLib reported success but changed nothing.
    pub const ERR_NO_EFFECT: Self = Self(-1107);
    /// `SMLIB_ERR_CORNER_APART`.
    pub const ERR_CORNER_APART: Self = Self(-1108);
    /// `SMLIB_ERR_WALL_CLOSES`.
    pub const ERR_WALL_CLOSES: Self = Self(-1109);
    /// `SMLIB_ERR_WALLS_TOUCH`: an offset whose walls would meet.
    pub const ERR_WALLS_TOUCH: Self = Self(-1110);
    /// `SMLIB_ERR_CORNER_UNSUPPORTED`.
    pub const ERR_CORNER_UNSUPPORTED: Self = Self(-1111);
    /// `SM_ERR`: SMLib's generic failure.
    pub const SM_ERR: Self = Self(SM_ERR);
    /// `SM_ERR_INVALID_INPUT`.
    pub const SM_ERR_INVALID_INPUT: Self = Self(SM_ERR_INVALID_INPUT);

    /// The C status code.
    #[must_use]
    pub const fn code(self) -> i32 {
        self.0
    }

    /// The status of code `code` (`SMLIB_*` or an SMLib status).
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        Self(code)
    }

    /// `smlib_shim::status`: an SMLib status as the C API's. An offset whose walls would
    /// meet is [`Status::ERR_WALLS_TOUCH`], one that would move a vertex past its edge's
    /// end [`Status::ERR_UNSUPPORTED`]; any other status unchanged.
    #[must_use]
    pub const fn from_sm_status(code: i32) -> Self {
        match code {
            SM_ERR_OFFSET_WALLS_TOUCH => Self::ERR_WALLS_TOUCH,
            SM_ERR_OFFSET_EDGE_VANISHES => Self::ERR_UNSUPPORTED,
            other => Self(other),
        }
    }

    /// The status a solidcore error stands for (see the module's deviations).
    #[must_use]
    pub fn from_solid(e: &SolidError) -> Self {
        if let Some(code) = e.sm_status() {
            return Self::from_sm_status(code);
        }
        match e {
            SolidError::InvalidInput { .. } => Self::SM_ERR_INVALID_INPUT,
            SolidError::NotFound { .. }
            | SolidError::Degenerate { .. }
            | SolidError::NotConverged { .. } => Self::SM_ERR,
            SolidError::Unported { .. } | SolidError::Unsupported { .. } => Self::ERR_UNSUPPORTED,
            _ => Self::ERR_EXCEPTION,
        }
    }
}

impl core::fmt::Display for Status {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_statuses_map_as_the_shim_maps_them() {
        assert_eq!(Status::from_sm_status(1042), Status::ERR_WALLS_TOUCH);
        assert_eq!(Status::from_sm_status(1043), Status::ERR_UNSUPPORTED);
        assert_eq!(Status::from_sm_status(1001), Status::SM_ERR);
        assert_eq!(Status::from_sm_status(1007).code(), 1007);
    }

    #[test]
    fn errors_without_a_status_map_to_their_kind() {
        let e = SolidError::invalid_input("x", "y");
        assert_eq!(Status::from_solid(&e), Status::SM_ERR_INVALID_INPUT);
    }
}
