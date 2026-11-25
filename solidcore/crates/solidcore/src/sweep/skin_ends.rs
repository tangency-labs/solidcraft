//! Not upstream: a skin that leaves an end section along a given direction, each point as far
//! as its own way to the next section asks.
//!
//! Upstream's skin with cross-boundary derivatives (`N_CreateSkinSrfBoundaryContinuity`) takes
//! a derivative strip per end and, for a direction alone (`NL_G1`), one length for the whole
//! section: the chordal length of the skin along the section's ends. Where two parts of a
//! section are near the next section and far from it, that one length is too long for the near
//! part: the skin runs past the next section there and turns back.
//!
//! [`skin_with_end_tangents`] gives every control point of the end section its own length.
//! With the sections made compatible, control point `j` of the end section `P[j]` and of its
//! neighbour `Q[j]` are the ends of one row of the skin across the sections, and the skin leaves
//! `P[j]` with the derivative
//!
//! ```text
//! D[j] = weight * |Q[j] - P[j]| / dt * direction
//! ```
//!
//! `dt` the step of the skinning parameter between the two sections. `|Q[j] - P[j]| / dt` is
//! the speed of the straight row from `P[j]` to `Q[j]`, so a weight of 1 along the row's own
//! direction is that straight row, and between two sections on parallel planes, one square
//! above the other, a row stays between the planes for every weight up to 3 at both ends (its
//! Bezier points then lie between them). In homogeneous coordinates the strip's rows are
//! `(w P, w)` and `(w (P + D), w)`, so the derivative of the weight across the end is zero and
//! the skin's derivative there is `sum R[j](v) D[j]`, `R` the section's rational basis: along
//! `direction` at every point of the section, never only at the control points.
//!
//! The strips are handed to [`create_skin_srf_boundary_continuity_mapped`] as `NL_C1` data
//! (the derivative itself), with the sections already compatible and the skinning parameters
//! the lengths were measured with, so the one skinning implementation interpolates them.
//!
//! A skin of planar sections takes its directions from the sections' planes ([`SkinEnd`],
//! [`end_tangents_of`]): square to the end section's plane, on the side the next section's
//! centre is on. A centre within the body's tolerance of that plane is in it, and the end has
//! no side to leave to: that is refused, never guessed.
//!
//! | Rust                        | Upstream |
//! |-----------------------------|----------|
//! | [`EndTangent`]              | (none)   |
//! | [`skin_with_end_tangents`]  | (none; builds the `der`, `ts` of `N_CreateSkinSrfBoundaryContinuity`) |
//! | [`skinned_surface_with_end_tangents`] | (none; the conversions of `CreateSkinnedSurface`) |
//! | [`SkinEnd`], [`end_tangents_of`] | (none) |

use crate::error::{Result, SolidError};
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p1::create_nlib_curve;
use crate::nurbs::crv_basic::{Boundary, Curve};
use crate::nurbs::crv_fit::p1::{DataDir, ParamMethod};
use crate::nurbs::crv_tool::p2::{make_compatible_knot_tol_mapped, KnotMap};
use crate::nurbs::geom::dist_pt_pt;
use crate::nurbs::math::CPoint;
use crate::nurbs::srf_adv::p5::{
    create_skin_srf_boundary_continuity_mapped, BoundaryContinuity, SkinBoundaryOptions,
};
use crate::nurbs::srf_basic::{Direction, Surface};
use crate::nurbs::srf_fit::interp_params;

const OP: &str = "sweep::skin_ends::skin_with_end_tangents";

/// How a skin leaves an end section: along `direction`, each control point with `weight` times
/// the speed of its straight row to the next section.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EndTangent {
    /// The direction the skin runs on in at this section (from the first section into the
    /// skin, into the last section from the skin); its length does not count.
    pub direction: Vec3,
    /// The length of the derivative over the speed of the straight row (above 0; 1 is that
    /// speed).
    pub weight: f64,
}

/// How a skin of planar sections leaves an end section.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SkinEnd {
    /// As the interpolation through the sections runs: no condition.
    #[default]
    Free,
    /// Square to the section's plane, towards the side the next section is on, each point
    /// with `weight` times the speed of its straight row to the next section
    /// ([`EndTangent::weight`]).
    Square {
        /// The length of the derivative over the speed of the straight row (above 0).
        weight: f64,
    },
}

/// The end tangents `ends` ask for of a skin whose sections lie on planes: `sections` each
/// section's plane as a point of it (its loops' centre) and its unit normal, either way round,
/// `None` for a section that is not planar. A [`SkinEnd::Square`] end leaves along its plane's
/// normal turned to the side the neighbouring section's centre is on (for the last section:
/// away from the one before it).
///
/// # Errors
///
/// `InvalidInput` for a square end without two sections, an end section or its neighbour that
/// is not planar, and a neighbour's centre within `tol` of the end section's plane (or not
/// comparable with it): the end has no side to leave to.
pub(crate) fn end_tangents_of(
    sections: &[Option<(Point3, Vec3)>],
    ends: [SkinEnd; 2],
    tol: f64,
) -> Result<[Option<EndTangent>; 2]> {
    const OP: &str = "sweep::skin_ends::end_tangents_of";
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let plane = |i: Option<usize>| -> Result<(Point3, Vec3)> {
        i.and_then(|i| sections.get(i))
            .copied()
            .flatten()
            .ok_or_else(|| bad("an end square to its section needs planar sections"))
    };
    let last = sections.len().checked_sub(1);
    let before_last = last.and_then(|l| l.checked_sub(1));
    let one = |end: SkinEnd, at: Option<usize>, beside: Option<usize>, arriving: bool| {
        let SkinEnd::Square { weight } = end else {
            return Ok(None);
        };
        let (centre, normal) = plane(at)?;
        let (other, _) = plane(beside)?;
        // The height of the neighbour's centre over the end section's plane; the skin runs
        // from the first section to its neighbour, and from the neighbour into the last.
        let run = if arriving {
            centre - other
        } else {
            other - centre
        };
        let side = normal.dot(run);
        // (written to accept: NaN fails both comparisons)
        let direction = if side > tol {
            normal
        } else if side < -tol {
            -normal
        } else {
            return Err(bad(
                "the next section's centre lies in the end section's plane: no side to leave to",
            ));
        };
        Ok(Some(EndTangent { direction, weight }))
    };
    let [start, end] = ends;
    Ok([
        one(start, Some(0), Some(1), false)?,
        one(end, last, before_last, true)?,
    ])
}

/// [`skin_with_end_tangents`] through B-spline `cross_sections`, which become the skin's
/// isoparameter curves the way [`BSplineSurface::create_skinned_surface`]'s do by default
/// (`SurfParamType::U`), `approx_tol` its `es`; with `want_maps` also where each section's
/// knots went in the skin's knots along the sections ([`KnotMap`], one per section).
///
/// # Errors
///
/// Those of [`skin_with_end_tangents`] and of the curves' conversion.
pub(crate) fn skinned_surface_with_end_tangents(
    cross_sections: &[BSplineCurve],
    ends: [Option<EndTangent>; 2],
    degree: usize,
    approx_tol: f64,
    section_params: Option<&[f64]>,
    want_maps: bool,
) -> Result<(BSplineSurface, Option<Vec<Option<KnotMap>>>)> {
    let mut curves: Vec<Curve> = cross_sections
        .iter()
        .map(create_nlib_curve)
        .collect::<Result<_>>()?;
    let mut maps: Option<Vec<Option<KnotMap>>> = want_maps.then(|| {
        curves
            .iter()
            .map(|c| Some(KnotMap::identity(c.knots())))
            .collect()
    });
    let skin = skin_with_end_tangents(
        &mut curves,
        ends,
        degree,
        approx_tol,
        Direction::U,
        section_params,
        maps.as_deref_mut(),
    )?;
    Ok((BSplineSurface::from_gw_nurb(&skin), maps))
}

/// The skin through the cross sections `cur[0..=k]` that leaves `cur[0]` as `ends[0]` says
/// and arrives at `cur[k]` as `ends[1]` says (`None`: that end is free), of degree `deg` across
/// the sections where they and the end conditions allow it (lower for fewer conditions than
/// `deg + 1`). `es` above 0 removes knots across the sections within that tolerance, as the
/// skin without end conditions does; 0 interpolates.
///
/// The sections are made compatible in place (and `maps`, one per section when given, composed
/// with where each section's knots went). `params` are the sections' skinning parameters,
/// increasing from 0 to 1; `None` takes the chord-length parameters of the compatible control
/// points, as the skin without end conditions does. With [`Direction::U`] the sections are
/// v-curves (skinning in u), with [`Direction::V`] u-curves.
///
/// # Errors
///
/// `InvalidInput` for fewer than two sections, no end condition at all, [`Direction::UV`], a
/// direction that is not finite or has no length, a weight that is not finite and above 0,
/// `params` of another length than the sections, and two end sections with no parameter step
/// between them; the errors of the compatibility, parametrisation and skinning routines.
pub(crate) fn skin_with_end_tangents(
    cur: &mut [Curve],
    ends: [Option<EndTangent>; 2],
    deg: usize,
    es: f64,
    dir: Direction,
    params: Option<&[f64]>,
    maps: Option<&mut [Option<KnotMap>]>,
) -> Result<Surface> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let k = cur
        .len()
        .checked_sub(1)
        .filter(|k| *k >= 1)
        .ok_or_else(|| bad("a skin with an end condition needs two sections"))?;
    if dir == Direction::UV {
        return Err(bad("the skinning direction is u or v"));
    }
    let [start, end] = ends;
    if start.is_none() && end.is_none() {
        return Err(bad("no end condition"));
    }
    let leave = |e: Option<EndTangent>| -> Result<Option<(Vec3, f64)>> {
        let Some(e) = e else { return Ok(None) };
        // (written to accept: a NaN weight or direction fails the comparison)
        if !(e.weight > 0.0 && e.weight.is_finite()) {
            return Err(bad("an end condition's weight is a number above 0"));
        }
        let unit = e
            .direction
            .unitize()
            .ok()
            .filter(|u| u.x.is_finite() && u.y.is_finite() && u.z.is_finite())
            .ok_or_else(|| bad("an end condition's direction has no length"))?;
        Ok(Some((unit, e.weight)))
    };
    let (start, end) = (leave(start)?, leave(end)?);

    // The sections in one form, as the skinning brings them: 3D, rational all or none.
    for c in cur.iter_mut() {
        if !c.is_3d() {
            c.to_3d();
        }
        if c.is_rational() {
            c.prune_rat(false);
        }
    }
    if cur.iter().any(Curve::is_rational) {
        for c in cur.iter_mut() {
            if !c.is_rational() {
                c.non_rat_to_rat();
            }
        }
    }
    make_compatible_knot_tol_mapped(cur, 0.0, maps)?;

    // The skinning parameters the lengths are measured with.
    let ts: Vec<f64> = match params {
        Some(p) if p.len() == k + 1 => p.to_vec(),
        Some(_) => return Err(bad("one skinning parameter for each section")),
        None => {
            let net: Vec<Vec<CPoint>> = cur.iter().map(|c| c.cpts().to_vec()).collect();
            interp_params(&net, ParamMethod::ChordLength, DataDir::U)?
        }
    };
    if ts.len() != k + 1 {
        return Err(bad("one skinning parameter for each section"));
    }

    // The derivative strip of an end: the end section and its copy moved by each control
    // point's own derivative (linear across, over a unit knot span: the strip's derivative
    // across the section is the difference of its two rows).
    let strip = |at: usize, beside: usize, (unit, weight): (Vec3, f64), last: bool| {
        let section = cur.get(at).ok_or_else(|| bad("an end section"))?;
        let neighbour = cur.get(beside).ok_or_else(|| bad("an end section"))?;
        let (ta, tb) = match (ts.get(at), ts.get(beside)) {
            (Some(a), Some(b)) => (*a, *b),
            _ => return Err(bad("an end section's parameter")),
        };
        let dt = (tb - ta).abs();
        if !(dt > 0.0 && dt.is_finite()) {
            return Err(bad("no parameter step between an end section and the next"));
        }
        let row: Vec<CPoint> = section.cpts().to_vec();
        if neighbour.cpts().len() != row.len() {
            return Err(bad("the sections are not compatible"));
        }
        let mut moved = Vec::new();
        for (p, q) in row.iter().zip(neighbour.cpts()) {
            let speed = dist_pt_pt(p.to_euclid(), q.to_euclid()) / dt;
            // (a finite weight over a tiny step, or a point near the largest coordinate, can
            // still overflow: refused, never carried into the strip)
            let length = weight * speed;
            if !length.is_finite() {
                return Err(bad("an end derivative that is not a finite number"));
            }
            let d = unit * length;
            // The first section's strip ends at the section (its last row), the last
            // section's starts there.
            let m = p.translate(if last { d } else { -d });
            if ![m.x, m.y, m.z, m.w].iter().all(|c| c.is_finite()) {
                return Err(bad("an end derivative that is not a finite number"));
            }
            moved.push(m);
        }
        let net = if last {
            vec![row, moved]
        } else {
            vec![moved, row]
        };
        let mut s = Surface::new(
            net,
            1,
            section.degree(),
            vec![0.0, 0.0, 1.0, 1.0],
            section.knots().to_vec(),
        )?;
        if dir == Direction::V {
            s.swap_uv();
        }
        Ok(s)
    };
    let d0 = start.map(|e| strip(0, 1, e, false)).transpose()?;
    let d1 = end.map(|e| strip(k, k - 1, e, true)).transpose()?;

    // One condition per section and per end: no more degree than they carry.
    let conditions = k + usize::from(d0.is_some()) + usize::from(d1.is_some());
    let mut opt = SkinBoundaryOptions {
        der: Some([d0, d1]),
        cmp: true,
        cnt: [BoundaryContinuity::C1, BoundaryContinuity::C1],
        bnd: if dir == Direction::U {
            [Boundary::Right, Boundary::Left]
        } else {
            [Boundary::Top, Boundary::Bottom]
        },
        rail: None,
        deg: deg.min(conditions),
        es,
        dir,
        ts: Some(ts),
        kns: None,
    };
    create_skin_srf_boundary_continuity_mapped(cur, &mut opt, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::knots::Side;
    use crate::nurbs::math::NOW;

    const UP: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    /// A polyline `x -> (x + shift, 0, z)` over three spans, degree 1.
    fn line(z: f64, shift: f64) -> Curve {
        let pts = (0..4u8)
            .map(|j| CPoint::new(f64::from(j) + shift, 0.0, z, NOW))
            .collect();
        Curve::new(pts, 1, vec![0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0]).unwrap()
    }

    /// A quarter circle of radius `r` about the z axis at height `z`, rational quadratic.
    fn quarter(z: f64, r: f64) -> Curve {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let pts = vec![
            CPoint::new(r, 0.0, z, 1.0),
            CPoint::new(r * w, r * w, z * w, w),
            CPoint::new(0.0, r, z, 1.0),
        ];
        Curve::new(pts, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap()
    }

    fn tangent(weight: f64) -> EndTangent {
        EndTangent {
            direction: UP,
            weight,
        }
    }

    #[test]
    fn two_sections_are_the_hermite_rows() {
        // Sections 4 apart and 3 aside: every row's chord is 5. Both ends leave along z.
        let (w0, w1) = (0.9, 1.8);
        let mut cs = vec![line(0.0, 0.0), line(4.0, 3.0)];
        let s = skin_with_end_tangents(
            &mut cs,
            [Some(tangent(w0)), Some(tangent(w1))],
            3,
            0.0,
            Direction::U,
            None,
            None,
        )
        .unwrap();
        assert_eq!(s.degrees(), (3, 1));
        assert_eq!(s.u_knots(), [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        for (j, x) in [0.0, 1.0, 2.0, 3.0].into_iter().enumerate() {
            let row: Vec<Vec3> = (0..4).map(|i| s.cpt(i, j).unwrap().to_euclid()).collect();
            // One Bezier span: P, P + D0 / 3, Q - D1 / 3, Q, with |D| = weight * chord.
            let want = [
                Vec3::new(x, 0.0, 0.0),
                Vec3::new(x, 0.0, w0 * 5.0 / 3.0),
                Vec3::new(x + 3.0, 0.0, 4.0 - w1 * 5.0 / 3.0),
                Vec3::new(x + 3.0, 0.0, 4.0),
            ];
            for (got, want) in row.iter().zip(want) {
                assert!((*got - want).length() < 1e-14, "{j}: {got:?} {want:?}");
            }
        }
    }

    #[test]
    fn weight_one_along_the_rows_is_the_straight_skin() {
        // Each row runs along z already: its Hermite cubic with the row's own speed at both
        // ends is the straight row, at thirds.
        let mut cs = vec![line(0.0, 0.0), line(6.0, 0.0)];
        let s = skin_with_end_tangents(
            &mut cs,
            [Some(tangent(1.0)), Some(tangent(1.0))],
            3,
            0.0,
            Direction::U,
            None,
            None,
        )
        .unwrap();
        for (j, x) in [0.0, 1.0, 2.0, 3.0].into_iter().enumerate() {
            for (i, z) in [0.0, 2.0, 4.0, 6.0].into_iter().enumerate() {
                let p = s.cpt(i, j).unwrap().to_euclid();
                assert!((p - Vec3::new(x, 0.0, z)).length() < 1e-14);
            }
        }
    }

    #[test]
    fn each_point_takes_its_own_length() {
        // The second section is tilted: its rows' chords run from 2 to 8.
        let far = Curve::new(
            (0..4u8)
                .map(|j| CPoint::new(f64::from(j), 0.0, 2.0 + 2.0 * f64::from(j), NOW))
                .collect(),
            1,
            vec![0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0],
        )
        .unwrap();
        let mut cs = vec![line(0.0, 0.0), far];
        let ends = [Some(tangent(1.5)), None];
        let s = skin_with_end_tangents(&mut cs, ends, 3, 0.0, Direction::U, None, None).unwrap();
        // One end condition and two sections: three conditions, a quadratic.
        assert_eq!(s.degrees(), (2, 1));
        for (j, x) in [0.0, 1.0, 2.0, 3.0].into_iter().enumerate() {
            let chord = 2.0 + 2.0 * x;
            let d = s.cpt(1, j).unwrap().to_euclid() - s.cpt(0, j).unwrap().to_euclid();
            // A quadratic Bezier's first leg is half its end derivative.
            assert!(
                (d - UP * (1.5 * chord / 2.0)).length() < 1e-14,
                "{j}: {d:?}"
            );
        }
    }

    #[test]
    fn rational_sections_leave_along_the_direction_everywhere() {
        let mut cs = vec![quarter(0.0, 2.0), quarter(3.0, 2.0), quarter(7.0, 3.0)];
        let ts = [0.0, 0.375, 1.0];
        let s = skin_with_end_tangents(
            &mut cs,
            [Some(tangent(0.7)), Some(tangent(1.2))],
            3,
            0.0,
            Direction::U,
            Some(&ts),
            None,
        )
        .unwrap();
        // Through every section.
        for (c, u) in cs.iter().zip(ts) {
            for v in [0.0, 0.3, 1.0] {
                let p = s.eval(u, v, Side::Left, Side::Left).unwrap();
                let q = c.eval(v, Side::Left).unwrap();
                assert!((p - q).length() < 1e-12, "{u} {v}");
            }
        }
        // The first two rows carry the first section's weights, so the weight does not change
        // across the end and the derivative there is a blend of the rows' own, all along z.
        for j in 0..3 {
            let (a, b) = (s.cpt(0, j).unwrap(), s.cpt(1, j).unwrap());
            assert!((a.w - b.w).abs() < 1e-15);
            let d = b.to_euclid() - a.to_euclid();
            assert!(
                d.z > 0.0 && d.cross(UP).length() < 1e-14 * d.length(),
                "{d:?}"
            );
            let (a, b) = (s.cpt(4, j).unwrap(), s.cpt(3, j).unwrap());
            assert!((a.w - b.w).abs() < 1e-15);
            let d = a.to_euclid() - b.to_euclid();
            assert!(
                d.z > 0.0 && d.cross(UP).length() < 1e-14 * d.length(),
                "{d:?}"
            );
        }
    }

    #[test]
    fn v_curves_skin_the_same_surface_with_u_and_v_swapped() {
        let build = |dir| {
            let mut cs = vec![line(0.0, 0.0), line(4.0, 3.0), line(9.0, 1.0)];
            let ends = [Some(tangent(1.1)), Some(tangent(0.6))];
            skin_with_end_tangents(&mut cs, ends, 3, 0.0, dir, None, None).unwrap()
        };
        let (a, b) = (build(Direction::U), build(Direction::V));
        for (u, v) in [(0.0, 0.0), (0.2, 0.9), (0.5, 0.5), (1.0, 0.1)] {
            let p = a.eval(u, v, Side::Left, Side::Left).unwrap();
            let q = b.eval(v, u, Side::Left, Side::Left).unwrap();
            assert!((p - q).length() < 1e-13, "{u} {v}");
        }
    }

    #[test]
    fn given_parameters_set_the_speed() {
        // The first step is a quarter of the parameter: the straight row's speed is 4 chords.
        let mut cs = vec![line(0.0, 0.0), line(2.0, 0.0), line(8.0, 0.0)];
        let ts = [0.0, 0.25, 1.0];
        let s = skin_with_end_tangents(
            &mut cs,
            [Some(tangent(1.0)), Some(tangent(1.0))],
            3,
            0.0,
            Direction::U,
            Some(&ts),
            None,
        )
        .unwrap();
        let knots = s.u_knots().to_vec();
        let p = 3.0;
        let first = knots[4] - knots[1];
        let d = s.cpt(1, 0).unwrap().to_euclid() - s.cpt(0, 0).unwrap().to_euclid();
        assert!(
            (d * (p / first) - UP * (2.0 / 0.25)).length() < 1e-12,
            "{d:?}"
        );
    }

    #[test]
    fn a_square_end_leaves_to_the_side_of_the_next_section() {
        let z = |h: f64, n: f64| Some((Point3::new(0.0, 0.0, h), Vec3::new(0.0, 0.0, n)));
        let square = SkinEnd::Square { weight: 1.5 };
        // Normals either way round: both ends run on upwards.
        for (n0, n1) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
            let planes = [z(0.0, n0), z(1.0, 1.0), z(5.0, n1)];
            let [a, b] = end_tangents_of(&planes, [square; 2], 1e-6).unwrap();
            assert_eq!(a, Some(tangent(1.5)));
            assert_eq!(b, Some(tangent(1.5)));
        }
        // Downwards, and one end only.
        let planes = [z(0.0, 1.0), z(-2.0, 1.0)];
        let [a, b] = end_tangents_of(&planes, [square, SkinEnd::Free], 1e-6).unwrap();
        assert_eq!(a.map(|t| t.direction), Some(-UP));
        assert_eq!(b, None);
        // Free ends ask nothing of the sections.
        let free = end_tangents_of(&[None, None], [SkinEnd::Free; 2], 1e-6).unwrap();
        assert_eq!(free, [None, None]);
    }

    #[test]
    fn a_square_end_with_no_side_to_leave_to_is_refused() {
        let at = |x: f64, h: f64| Some((Point3::new(x, 0.0, h), UP));
        let square = [SkinEnd::Square { weight: 1.0 }, SkinEnd::Free];
        // The neighbour's centre in the plane, within the tolerance of it, or nowhere.
        for h in [0.0, 5e-7, -5e-7, f64::NAN] {
            let planes = [at(0.0, 0.0), at(9.0, h)];
            assert!(end_tangents_of(&planes, square, 1e-6).is_err(), "{h}");
        }
        assert!(end_tangents_of(&[at(0.0, 0.0), at(9.0, 2e-6)], square, 1e-6).is_ok());
        // Not planar, or not two sections.
        assert!(end_tangents_of(&[None, at(0.0, 1.0)], square, 1e-6).is_err());
        assert!(end_tangents_of(&[at(0.0, 0.0), None], square, 1e-6).is_err());
        assert!(end_tangents_of(&[at(0.0, 0.0)], square, 1e-6).is_err());
        assert!(end_tangents_of(&[], square, 1e-6).is_err());
    }

    #[test]
    fn a_strip_point_that_overflows_is_refused() {
        // The derivative is finite, the moved point is not: sections near the lowest z, each
        // row 1e150 long along x over the parameter step 1e-155 (speed 1e305), weight 10
        // (a derivative of 1e306), and the start's strip moved that far below -1.79e308.
        let low = -1.79e308;
        let mut cs = vec![line(low, 0.0), line(low, 1e150)];
        let ts = [0.0, 1e-155];
        let ends = [
            Some(EndTangent {
                direction: UP,
                weight: 10.0,
            }),
            None,
        ];
        let e = skin_with_end_tangents(&mut cs, ends, 3, 0.0, Direction::U, Some(&ts), None)
            .unwrap_err();
        assert!(
            matches!(e, SolidError::InvalidInput { reason, .. }
                if reason == "an end derivative that is not a finite number"),
            "{e:?}"
        );
    }

    #[test]
    fn hostile_end_conditions_are_refused() {
        let cs = || vec![line(0.0, 0.0), line(4.0, 0.0)];
        let run = |ends, dir, params: Option<&[f64]>| {
            skin_with_end_tangents(&mut cs(), ends, 3, 0.0, dir, params, None)
        };
        let t = |direction, weight| Some(EndTangent { direction, weight });
        assert!(run([None, None], Direction::U, None).is_err());
        assert!(run([Some(tangent(1.0)), None], Direction::UV, None).is_err());
        for weight in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(
                run([t(UP, weight), None], Direction::U, None).is_err(),
                "{weight}"
            );
        }
        // A finite weight whose derivative overflows.
        assert!(run([t(UP, f64::MAX), None], Direction::U, None).is_err());
        assert!(run([None, t(UP, f64::MAX)], Direction::U, None).is_err());
        for direction in [Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 1.0)] {
            assert!(run([None, t(direction, 1.0)], Direction::U, None).is_err());
        }
        assert!(run(
            [Some(tangent(1.0)), None],
            Direction::U,
            Some(&[0.0, 0.5, 1.0])
        )
        .is_err());
        assert!(run([Some(tangent(1.0)), None], Direction::U, Some(&[0.0, 0.0])).is_err());
        assert!(run(
            [Some(tangent(1.0)), None],
            Direction::U,
            Some(&[0.0, f64::NAN])
        )
        .is_err());
        let one = &mut [line(0.0, 0.0)];
        let ends = [Some(tangent(1.0)), None];
        assert!(skin_with_end_tangents(one, ends, 3, 0.0, Direction::U, None, None).is_err());
    }
}
