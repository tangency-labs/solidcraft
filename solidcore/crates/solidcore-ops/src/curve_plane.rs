//! The plane a set of free curves lies in: the kernel's certified
//! [`solidcore::curve::plane_of::plane_of`] at the model's zone tolerance, for an extrusion
//! along a profile's normal or a section's plane. Ops checks the records and converts.

use solidcore::curve::plane_of::plane_of as kernel_plane_of;
use solidcore::geom::tol::{context_zone_tol_3d, SmallTopoType};
use solidcore_record::brep::{check_curve, Budget, Curve};

use crate::session::run;
use crate::{Error, Result};

/// A plane: a point of it and its unit normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePlane {
    /// A point of the plane (the curves' first point).
    pub origin: [f64; 3],
    /// The plane's unit normal.
    pub normal: [f64; 3],
}

/// The plane every point of `curves` lies within the model's zone tolerance of, certified by
/// the kernel ([`solidcore::curve::plane_of::plane_of`]: the points whose convex hulls hold
/// the curves, each distance bounded with its rounding); `None` when the curves are not
/// proved planar to it, or span no plane (all within it of one point or one line). A line is
/// read as its segment from `start` to `end`; a circle or an ellipse as the whole conic.
///
/// # Errors
/// [`Error::Argument`] for no curves, a curve the record refuses or a traced intersection
/// curve; the kernel's status otherwise.
pub fn plane_of(curves: &[Curve]) -> Result<Option<CurvePlane>> {
    if curves.is_empty() {
        return Err(Error::Argument);
    }
    let mut kernel_curves = Vec::new();
    for c in curves {
        if matches!(c, Curve::SurfaceIntersection { .. }) {
            return Err(Error::Argument);
        }
        check_curve(c, &mut Budget::input()).map_err(|_| Error::Argument)?;
        kernel_curves.push(solidcore_convert::curve(c).map_err(|_| Error::Argument)?);
    }
    run("curve_plane::plane_of", |k| {
        let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
        Ok(kernel_plane_of(&kernel_curves, tol)?.map(|p| CurvePlane {
            origin: [p.origin.x, p.origin.y, p.origin.z],
            normal: [p.normal.x, p.normal.y, p.normal.z],
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spline(points: &[[f64; 3]]) -> Curve {
        Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            points: points.to_vec(),
        }
    }

    #[test]
    fn a_planar_spline_and_line_give_their_plane() {
        let wave = spline(&[
            [0.0, 0.0, 5.0],
            [10.0, 4.0, 5.0],
            [20.0, -3.0, 5.0],
            [30.0, 2.0, 5.0],
        ]);
        let line = Curve::Line {
            start: [30.0, 2.0, 5.0],
            end: [0.0, 0.0, 5.0],
        };
        let plane = plane_of(&[wave, line]).unwrap().unwrap();
        assert_eq!(plane.origin, [0.0, 0.0, 5.0]);
        assert!(plane.normal[2].abs() > 1.0 - 1e-15, "{:?}", plane.normal);
    }

    #[test]
    fn a_twisted_spline_or_collinear_lines_give_none() {
        let twisted = spline(&[
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 1.0],
        ]);
        assert_eq!(plane_of(&[twisted]), Ok(None));
        let a = Curve::Line {
            start: [0.0, 0.0, 0.0],
            end: [1.0, 1.0, 1.0],
        };
        let b = Curve::Line {
            start: [2.0, 2.0, 2.0],
            end: [5.0, 5.0, 5.0],
        };
        assert_eq!(plane_of(&[a, b]), Ok(None));
        assert_eq!(plane_of(&[]), Err(Error::Argument));
    }
}
