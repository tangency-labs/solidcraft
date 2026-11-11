//! A sheet's control net: read, its control points moved, and the surface rebuilt to new degrees
//! and spans with the kernel's certified bound of how far it moved. The sheet is a record of one
//! untrimmed face on a non-rational B-spline surface (its face the whole of the surface's
//! natural box, as `surface::boundary` and `surface::ruled` make it); the geometry is the
//! kernel's. Ops checks the record and converts.

use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_fit::p1::approx_knots;
use solidcore::nurbs::geom::Rectangle;
use solidcore::nurbs::knots::{clamped_uniform, KnotVector, Side};
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::{Direction, Surface as NurbSurface};
use solidcore::nurbs::srf_deviation::certified_distance;
use solidcore::Vec3;
use solidcore_record::brep::{
    knot_domain, Brep, Curve2, Loop, Natural, Surface, MAX_SPLINE_DEGREE,
};

use crate::session::run;
use crate::surface::{record, sheet as sheet_of};
use crate::sweep::coord_ok;
use crate::{Error, Result};

/// The most sample points a rebuild fits (rows times columns).
pub const REBUILD_MAX_SAMPLES: usize = 1 << 20;

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

/// `sheet` with the control points `moves` names moved: `((i, j), to)` puts the point of the
/// i-th u index and j-th v index at `to` (a later move of one point wins). The knots and
/// degrees are kept; the result is a new one-face sheet on the moved surface, its whole box.
///
/// # Errors
/// As [`control_net`]; [`Error::Argument`] for an index past the net or a point not finite or
/// past the coordinate bound; the kernel's status otherwise.
pub fn move_cvs(sheet: &Brep, moves: &[((usize, usize), [f64; 3])]) -> Result<Brep> {
    let ((u, v), points) = untrimmed(sheet)?;
    let mut net = points.to_vec();
    for &((i, j), to) in moves {
        if !coord_ok(&to) {
            return Err(Error::Argument);
        }
        *net.get_mut(i)
            .and_then(|row| row.get_mut(j))
            .ok_or(Error::Argument)? = to;
    }
    let surface = kernel_surface(u, v, &net)?;
    record(run("surface_cvs::move_cvs", |k| {
        sheet_of(k, BSplineSurface::from_gw_nurb(&surface))
    }))
}

/// `sheet`'s surface refitted to degrees `(deg_u, deg_v)` with `(spans_u, spans_v)` uniform
/// spans on `[0, 1]²`, and the kernel's certified bound of how far the refit is from the
/// surface at every parameter ([`certified_distance`]), kept when within `tol`. The surface,
/// its box mapped onto `[0, 1]²`, is sampled at even steps of each parameter, four per
/// control point each way (at most [`REBUILD_MAX_SAMPLES`] in all), and fitted by least
/// squares through its boundary rows: each column of samples along u at those parameters,
/// then each row of the result along v ([`approx_knots`], The NURBS Book §9.4.3).
///
/// # Errors
/// As [`control_net`]; [`Error::Argument`] for a surface on knots not clamped (a periodic
/// one), a degree of 0 or past the record's most, no
/// spans, fewer than three or more than a record's control points either way, more samples
/// than [`REBUILD_MAX_SAMPLES`], or a `tol` not finite or negative; [`Error::PastTolerance`]
/// with the bound when it is past `tol`; the kernel's status otherwise.
pub fn rebuild(
    sheet: &Brep,
    deg_u: usize,
    deg_v: usize,
    spans_u: usize,
    spans_v: usize,
    tol: f64,
) -> Result<(Brep, f64)> {
    let ((u, v), points) = untrimmed(sheet)?;
    let degrees_ok = |d: usize| (1..=MAX_SPLINE_DEGREE).contains(&d);
    if !(degrees_ok(deg_u) && degrees_ok(deg_v) && tol.is_finite() && tol >= 0.0) {
        return Err(Error::Argument);
    }
    let count = |spans: usize, degree: usize| {
        spans
            .checked_add(degree)
            .filter(|c| (3..=4096).contains(c))
            .ok_or(Error::Argument)
    };
    let (nu, nv) = (count(spans_u, deg_u)?, count(spans_v, deg_v)?);
    let samples = |c: usize| c.checked_mul(4).and_then(|s| s.checked_add(1));
    let (su, sv) = (
        samples(nu).ok_or(Error::Argument)?,
        samples(nv).ok_or(Error::Argument)?,
    );
    if su.checked_mul(sv).is_none_or(|n| n > REBUILD_MAX_SAMPLES) {
        return Err(Error::Argument);
    }
    // even steps on [0, 1]: the distinct knots of a polyline of that many points
    let steps = |n: usize| -> Result<Vec<f64>> {
        let knots = clamped_uniform(n, 1).map_err(|_| Error::Argument)?;
        Ok(knots.get(1..=n).ok_or(Error::Argument)?.to_vec())
    };
    let (pu, pv) = (steps(su)?, steps(sv)?);
    let uk = clamped_uniform(nu, deg_u).map_err(|_| Error::Argument)?;
    let vk = clamped_uniform(nv, deg_v).map_err(|_| Error::Argument)?;
    let original = kernel_surface(u, v, points)?;
    // A surface on unclamped knots is not sampled: the kernel's surface evaluation keeps
    // upstream's clamped reading of a domain's ends (N_BasisEval), and the certified bound takes
    // clamped knots only.
    if !(clamped(u, original.p()) && clamped(v, original.q())) {
        return Err(Error::Argument);
    }
    let fitted = run("surface_cvs::rebuild", |_| {
        let mut unit = original.clone();
        let square = Rectangle {
            ul: 0.0,
            ur: 1.0,
            vb: 0.0,
            vt: 1.0,
        };
        unit.reparam_to_interval(square, Direction::U)?;
        unit.reparam_to_interval(square, Direction::V)?;
        // the samples, row a at u = pu[a], column b at v = pv[b]
        let mut grid: Vec<Vec<Vec3>> = Vec::new();
        for &a in &pu {
            grid.push(
                pv.iter()
                    .map(|&b| unit.eval(a, b, Side::Left, Side::Left))
                    .collect::<solidcore::Result<_>>()?,
            );
        }
        // along u, column by column: rows[i][b] the i-th u point of column b
        let knots_u = KnotVector::from_slice(&uk)?;
        let mut along_u: Vec<Vec<Vec3>> = vec![Vec::new(); nu];
        for b in 0..sv {
            let column: Vec<Vec3> = grid.iter().filter_map(|row| row.get(b).copied()).collect();
            let fit = approx_knots(&column, &pu, &knots_u, nu - 1, deg_u)?;
            for (row, p) in along_u.iter_mut().zip(fit.cpts()) {
                row.push(Vec3::new(p.x, p.y, p.z));
            }
        }
        // then along v, row by row
        let knots_v = KnotVector::from_slice(&vk)?;
        let mut net: Vec<Vec<CPoint>> = Vec::new();
        for row in &along_u {
            let fit = approx_knots(row, &pv, &knots_v, nv - 1, deg_v)?;
            net.push(fit.cpts().to_vec());
        }
        let fit = NurbSurface::new(net, deg_u, deg_v, uk.clone(), vk.clone())?;
        let bound = certified_distance(&unit, &fit)?;
        Ok(Ok((fit, bound)))
    })??;
    let (fit, bound) = fitted;
    if bound > tol {
        return Err(Error::PastTolerance { bound });
    }
    let made = record(run("surface_cvs::rebuild", |k| {
        sheet_of(k, BSplineSurface::from_gw_nurb(&fit))
    }))?;
    Ok((made, bound))
}

/// Whether the first and last `degree + 1` of `knots` are each one value.
fn clamped(knots: &[f64], degree: usize) -> bool {
    let flat =
        |ends: Option<&[f64]>| ends.is_some_and(|e| e.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(degree + 1);
    flat(knots.get(..=degree)) && flat(last.and_then(|s| knots.get(s..)))
}

/// The kernel's surface of a record net and its knots (degrees from the knot counts).
///
/// # Errors
/// [`Error::Argument`] for knots that do not fit the net.
fn kernel_surface(u: &[f64], v: &[f64], net: &[Vec<[f64; 3]>]) -> Result<NurbSurface> {
    let rows = net.len();
    let cols = net.first().map_or(0, Vec::len);
    let p = u.len().checked_sub(rows + 1).ok_or(Error::Argument)?;
    let q = v.len().checked_sub(cols + 1).ok_or(Error::Argument)?;
    let pw = net
        .iter()
        .map(|row| {
            row.iter()
                .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
                .collect()
        })
        .collect();
    NurbSurface::new(pw, p, q, u.to_vec(), v.to_vec()).map_err(|_| Error::Argument)
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

    /// The point of a sheet record's surface at `(u, v)`, by the kernel's evaluator.
    fn at(sheet: &Brep, u: f64, v: f64) -> [f64; 3] {
        let ((uk, vk), net) = untrimmed(sheet).unwrap();
        let p = kernel_surface(uk, vk, net)
            .unwrap()
            .eval(u, v, Side::Left, Side::Left)
            .unwrap();
        [p.x, p.y, p.z]
    }

    /// A ruled sheet between two cubics of six control points (interior knots 1/3 and 2/3).
    fn long_sheet() -> Brep {
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0];
        let row = |y: f64| Curve::Spline {
            knots: knots.clone(),
            points: (0..6)
                .map(|i| [f64::from(i) * 2.0, y, f64::from(i % 2)])
                .collect(),
        };
        ruled(&row(0.0), &row(5.0), false).unwrap()
    }

    #[test]
    fn moving_one_cv_changes_only_its_local_span() {
        let s = long_sheet();
        let net = control_net(&s).unwrap();
        // the direction along the curves has six points; the first of them reaches only the
        // first third of it
        let (along_u, first) = (net.rows == 6, (0, 0));
        let moved = move_cvs(&s, &[(first, [0.0, 0.0, 3.0])]).unwrap();
        let after = control_net(&moved).unwrap();
        assert_eq!((after.rows, after.cols), (net.rows, net.cols));
        assert_eq!(after.points[0], [0.0, 0.0, 3.0]);
        assert_eq!(&after.points[1..], &net.points[1..]);
        for i in 0..=20 {
            for j in 0..=20 {
                let (a, b) = (f64::from(i) / 20.0, f64::from(j) / 20.0);
                let along = if along_u { a } else { b };
                let (p, q) = (at(&s, a, b), at(&moved, a, b));
                if along >= 1.0 / 3.0 {
                    assert!(
                        p.iter().zip(q).all(|(x, y)| (x - y).abs() < 1e-12),
                        "moved outside its span at ({a}, {b}): {p:?} and {q:?}"
                    );
                }
            }
        }
        // the moved point's span did move
        let (a, b) = if along_u { (0.05, 0.0) } else { (0.0, 0.05) };
        assert_ne!(at(&s, a, b), at(&moved, a, b));
        // bad moves
        assert_eq!(
            move_cvs(&s, &[((net.rows, 0), [0.0; 3])]),
            Err(Error::Argument)
        );
        assert_eq!(
            move_cvs(&s, &[((0, 0), [f64::NAN, 0.0, 0.0])]),
            Err(Error::Argument)
        );
    }

    /// The Coons sheet of four bent cubic Bézier sides: a bicubic Bézier patch.
    fn bicubic() -> Brep {
        let side = |points: [[f64; 3]; 4]| Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            points: points.to_vec(),
        };
        crate::surface::boundary(&[
            side([
                [0.0, 0.0, 0.0],
                [3.0, 0.0, 2.0],
                [6.0, 0.0, -1.0],
                [9.0, 0.0, 0.0],
            ]),
            side([
                [9.0, 0.0, 0.0],
                [9.0, 3.0, 1.0],
                [9.0, 6.0, -2.0],
                [9.0, 9.0, 0.0],
            ]),
            side([
                [9.0, 9.0, 0.0],
                [6.0, 9.0, 2.0],
                [3.0, 9.0, 1.0],
                [0.0, 9.0, 0.0],
            ]),
            side([
                [0.0, 9.0, 0.0],
                [0.0, 6.0, -1.0],
                [0.0, 3.0, 2.0],
                [0.0, 0.0, 0.0],
            ]),
        ])
        .unwrap()
    }

    #[test]
    fn a_bicubic_rebuilt_into_its_own_space_is_itself() {
        let s = bicubic();
        let net = control_net(&s).unwrap();
        assert_eq!((net.rows, net.cols), (4, 4), "{net:?}");
        let (r, bound) = rebuild(&s, 3, 3, 1, 1, 1e-9).unwrap();
        assert!(bound < 1e-9, "{bound}");
        for i in 0..=10 {
            for j in 0..=10 {
                let (a, b) = (f64::from(i) / 10.0, f64::from(j) / 10.0);
                let (p, q) = (at(&s, a, b), at(&r, a, b));
                assert!(p.iter().zip(q).all(|(x, y)| (x - y).abs() <= bound));
            }
        }
    }

    #[test]
    fn a_rebuild_past_the_tolerance_is_refused_with_its_bound() {
        let s = bicubic();
        // the bent patch is no bilinear one
        assert!(matches!(
            rebuild(&s, 1, 1, 2, 2, 1e-6),
            Err(Error::PastTolerance { bound }) if bound > 1e-6
        ));
        // and a looser tolerance takes the same fit
        let (_, bound) = rebuild(&s, 2, 2, 2, 2, 100.0).unwrap();
        assert!(bound > 0.0 && bound <= 100.0);
        // bad arguments
        assert_eq!(rebuild(&s, 0, 3, 1, 1, 1.0), Err(Error::Argument));
        assert_eq!(rebuild(&s, 3, 3, 0, 1, 1.0), Err(Error::Argument));
        assert_eq!(rebuild(&s, 1, 1, 1, 1, 1.0), Err(Error::Argument));
        assert_eq!(rebuild(&s, 3, 3, 1, 1, f64::NAN), Err(Error::Argument));
    }

    #[test]
    fn a_surface_on_unclamped_knots_is_not_sampled() {
        // the bicubic's u knots made unclamped on the same domain [0, 1]: its face, box and
        // sides unchanged, so it reads as untrimmed, but its ends are no clamped ends
        let mut s = bicubic();
        let Surface::Spline { u_knots, .. } = &mut s.faces[0].surface else {
            panic!("a spline sheet")
        };
        *u_knots = vec![-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0];
        assert!(control_net(&s).is_ok());
        assert_eq!(rebuild(&s, 3, 3, 1, 1, 1.0), Err(Error::Argument));
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
