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

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Whether `p` lies within `tol` of the plane `normal · x = offset` (`normal` unit).
#[must_use]
pub fn point_on_plane(normal: [f64; 3], offset: f64, p: [f64; 3], tol: f64) -> bool {
    (dot(normal, p) - offset).abs() <= tol
}

/// Whether a planar face with unit normal `face_normal`, whose boundary has the corners
/// `corners`, lies on the plane `normal · x = offset` (`normal` unit) within `tol`: the normals
/// point the same way and every corner is on the plane. The normals alone do not settle it on a
/// large face: two within 1e-9 in dot (4.5e-5 rad) still part by 0.045 across a metre.
#[must_use]
pub fn coplanar(
    normal: [f64; 3],
    offset: f64,
    face_normal: [f64; 3],
    corners: &[[f64; 3]],
    tol: f64,
) -> bool {
    dot(normal, face_normal) > 1.0 - 1e-9
        && !corners.is_empty()
        && corners
            .iter()
            .all(|p| point_on_plane(normal, offset, *p, tol))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_is_coplanar_only_when_every_corner_is_on_the_plane() {
        let z = [0.0, 0.0, 1.0];
        let flat = [
            [0.0, 0.0, 10.0],
            [1000.0, 0.0, 10.0],
            [1000.0, 1000.0, 10.0],
            [0.0, 1000.0, 10.0],
        ];
        assert!(coplanar(z, 10.0, z, &flat, 1e-3));
        assert!(!coplanar(z, 10.0, [0.0, 0.0, -1.0], &flat, 1e-3));
        assert!(point_on_plane(z, 10.0, [5.0, 5.0, 10.0 + 5e-4], 1e-3));
        // Tilted 4e-5 rad about x through (0, 0, 10): the normals pass a 1e-9 dot test and the
        // planes agree at the origin, but the far side of the 1000 face is 0.04 off.
        let t: f64 = 4e-5;
        let tilted_n = [0.0, -t.sin(), t.cos()];
        assert!(dot(z, tilted_n) > 1.0 - 1e-9);
        let tilted: Vec<[f64; 3]> = flat
            .iter()
            .map(|p| [p[0], p[1], 10.0 + p[1] * t.tan()])
            .collect();
        assert!((dot(tilted_n, [0.0, 0.0, 10.0]) - 10.0).abs() < 1e-3);
        assert!(!coplanar(z, 10.0, tilted_n, &tilted, 1e-3));
        assert!(!point_on_plane(
            z,
            10.0,
            [0.0, 1000.0, 10.0 + 1000.0 * t.tan()],
            1e-3
        ));
    }

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
