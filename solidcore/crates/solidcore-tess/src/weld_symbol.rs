//! A welding symbol's constructions on the record's plane types.
//!
//! The geometry is a kernel primitive ([`solidcore::weld_symbol`]); this module is the thin
//! entry for clients that hold [`solidcore_record`] types: it converts in, calls the kernel and
//! converts the result back, nothing more.

use solidcore_convert::{record_seg2, record_vec2, vec2};
use solidcore_record::{Seg2, Vec2};

pub use solidcore::weld_symbol::WeldContour;

use crate::{Result, TessError};

fn invalid(error: solidcore::SolidError) -> TessError {
    TessError::Invalid(error.to_string())
}

/// The contour mark over the weld face at `face`: a line for a flush contour, a sixty-degree arc
/// of radius `1.5 * size` for a convex or concave one. `tangent` runs along the face and
/// `outward` leaves it, both unit and perpendicular. See [`solidcore::weld_symbol::weld_contour_mark`].
/// # Errors
/// A point or direction that is not finite, a direction that is not a unit vector, directions
/// that are not perpendicular, or a `size` that is not finite and positive.
pub fn weld_contour_mark(
    face: Vec2,
    tangent: Vec2,
    outward: Vec2,
    size: f64,
    contour: WeldContour,
) -> Result<Seg2> {
    solidcore::weld_symbol::weld_contour_mark(
        vec2(face),
        vec2(tangent),
        vec2(outward),
        size,
        contour,
    )
    .map(|segment| record_seg2(&segment))
    .map_err(invalid)
}

/// The depth of a sixty-degree V groove of the given opening `width`.
/// # Errors
/// A `width` that is not finite and positive.
pub fn v_groove_height(width: f64) -> Result<f64> {
    solidcore::weld_symbol::v_groove_height(width).map_err(invalid)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::float_cmp)]

    use super::*;

    #[test]
    fn matches_the_kernel_function() {
        let d = 1.0 / 2.0_f64.sqrt();
        let (face, tangent, outward) = (Vec2::new(0.5, 0.5), Vec2::new(d, -d), Vec2::new(d, d));
        for contour in [
            WeldContour::Flush,
            WeldContour::Convex,
            WeldContour::Concave,
        ] {
            let kernel = solidcore::weld_symbol::weld_contour_mark(
                vec2(face),
                vec2(tangent),
                vec2(outward),
                3.0,
                contour,
            )
            .unwrap();
            let record = weld_contour_mark(face, tangent, outward, 3.0, contour).unwrap();
            assert_eq!(record, record_seg2(&kernel));
        }
        assert_eq!(
            v_groove_height(5.0).unwrap(),
            solidcore::weld_symbol::v_groove_height(5.0).unwrap()
        );
    }

    #[test]
    fn refusals_are_errors() {
        let (t, n) = (Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0));
        assert!(weld_contour_mark(Vec2::ZERO, t, n, f64::NAN, WeldContour::Flush).is_err());
        assert!(weld_contour_mark(Vec2::ZERO, t, t, 1.0, WeldContour::Flush).is_err());
        assert!(v_groove_height(-1.0).is_err());
    }
}
