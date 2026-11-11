//! A sheet's control net: read, its control points moved, and the surface rebuilt to new degrees
//! and spans with the kernel's certified bound of how far it moved. The sheet is a record of one
//! untrimmed face on a non-rational B-spline surface (its face the whole of the surface's
//! natural box, as `surface::boundary` and `surface::ruled` make it); the geometry is the
//! kernel's. Ops checks the record and converts.

use solidcore_record::brep::{knot_domain, Brep, Curve2, Loop, Natural, Surface};

use crate::{Error, Result};

/// A sheet's control net: `rows` points along u by `cols` along v, `points[i * cols + j]` the
/// point of the i-th u index and the j-th v index.
#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    /// Control points along u.
    pub rows: usize,
    /// Control points along v.
    pub cols: usize,
    /// The points, row by row (u index major).
    pub points: Vec<[f64; 3]>,
}

/// The control net of `sheet`'s surface.
///
/// # Errors
/// [`Error::Argument`] for a record that is not one face on a non-rational B-spline surface, or
/// whose net is not rectangular; [`Error::Unsupported`] for a trimmed face (see [`untrimmed`]).
pub fn control_net(sheet: &Brep) -> Result<Net> {
    let (_, points) = untrimmed(sheet)?;
    let rows = points.len();
    let cols = points.first().map_or(0, Vec::len);
    if cols == 0 || points.iter().any(|r| r.len() != cols) {
        return Err(Error::Argument);
    }
    Ok(Net {
        rows,
        cols,
        points: points.iter().flatten().copied().collect(),
    })
}

/// The knots `(u, v)` and net of the one face of `sheet`, when it is untrimmed: one face on a
/// non-rational B-spline surface (no separate NURBS form), its domain the surface's natural
/// box, one loop, and every use's pcurve on a side of that box (a spline pcurve whose control
/// points all share the side's u or v: by their convex hull, the whole pcurve lies on it).
///
/// # Errors
/// [`Error::Argument`] for a record of more or fewer faces, another surface or a rational one;
/// [`Error::Unsupported`] for a face that is trimmed (or not proved untrimmed: a rational
/// pcurve, a pcurve off the box's sides, holes).
#[allow(clippy::type_complexity)]
pub(crate) fn untrimmed(sheet: &Brep) -> Result<((&[f64], &[f64]), &[Vec<[f64; 3]>])> {
    let [face] = sheet.faces.as_slice() else {
        return Err(Error::Argument);
    };
    let (
        Surface::Spline {
            u_knots,
            v_knots,
            points,
        },
        None,
    ) = (&face.surface, &face.nurbs)
    else {
        return Err(Error::Argument);
    };
    let cols = points.first().map_or(0, Vec::len);
    let (Natural::Range(u0, u1), Natural::Range(v0, v1)) = (
        knot_domain(u_knots, points.len()),
        knot_domain(v_knots, cols),
    ) else {
        return Err(Error::Argument);
    };
    let trimmed = Err(Error::Unsupported("a trimmed face"));
    if face.domain != [[u0, u1], [v0, v1]] {
        return trimmed;
    }
    let [Loop::Edges { edges }] = face.loops.as_slice() else {
        return trimmed;
    };
    for used in edges {
        let Curve2::Spline { points: uv, .. } = &used.pcurve else {
            return trimmed;
        };
        let side = |k: usize, at: f64| uv.iter().all(|p| p.get(k) == Some(&at));
        if !(side(0, u0) || side(0, u1) || side(1, v0) || side(1, v1)) {
            return trimmed;
        }
    }
    Ok(((u_knots, v_knots), points))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::ruled;
    use solidcore_record::brep::Curve;

    /// The ruled sheet between two cubic splines, a one-face untrimmed sheet.
    fn sheet() -> Brep {
        let a = Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            points: vec![
                [0.0, 0.0, 0.0],
                [3.0, 0.0, 2.0],
                [6.0, 0.0, -1.0],
                [9.0, 0.0, 0.0],
            ],
        };
        let b = Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            points: vec![
                [0.0, 8.0, 1.0],
                [3.0, 8.0, 0.0],
                [6.0, 8.0, 2.0],
                [9.0, 8.0, 1.0],
            ],
        };
        ruled(&a, &b, false).unwrap()
    }

    #[test]
    fn a_ruled_sheets_net_is_its_two_curves() {
        let net = control_net(&sheet()).unwrap();
        assert_eq!(net.rows * net.cols, net.points.len());
        // one direction runs along the curves (4 points), the other across (2)
        assert!(
            (net.rows, net.cols) == (4, 2) || (net.rows, net.cols) == (2, 4),
            "{net:?}"
        );
        for corner in [
            [0.0, 0.0, 0.0],
            [9.0, 0.0, 0.0],
            [0.0, 8.0, 1.0],
            [9.0, 8.0, 1.0],
        ] {
            assert!(net.points.contains(&corner), "{corner:?} in {net:?}");
        }
    }

    #[test]
    fn a_record_of_other_faces_or_a_trimmed_face_is_refused() {
        let mut two = sheet();
        let face = two.faces[0].clone();
        two.faces.push(face);
        assert_eq!(control_net(&two), Err(Error::Argument));
        let mut smaller = sheet();
        smaller.faces[0].domain[0][1] *= 0.5;
        assert_eq!(
            control_net(&smaller),
            Err(Error::Unsupported("a trimmed face"))
        );
    }
}
