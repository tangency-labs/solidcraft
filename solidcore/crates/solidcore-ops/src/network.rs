//! Surfaces through curve networks: the Gordon surface of u-curves crossed by v-curves
//! ([`gordon`]), as the kernel builds and certifies it (`solidcore::nurbs::gordon`). The curves
//! are the record's ([`Curve`]), each over its natural range; ops checks the counts, converts,
//! hands the kernel the model's zone tolerance and names its refusals.

use solidcore::geom::tol::{context_zone_tol_3d, SmallTopoType};
use solidcore::nurbs::gordon::{gordon as kernel_gordon, NetworkRefusal, MAX_CURVES, MIN_CURVES};
use solidcore_record::brep::{Brep as Record, Curve};

use crate::session::run;
use crate::surface::{record, sheet, splines};
use crate::{Error, Result};

/// The refusal as ops names it.
fn refused(r: NetworkRefusal) -> Error {
    match r {
        NetworkRefusal::Rational => Error::Unsupported("a rational curve in a curve network"),
        NetworkRefusal::Crossings { u, v, count } => Error::NoCrossing { u, v, count },
        NetworkRefusal::Order => Error::Argument,
        NetworkRefusal::Gap { u, v, gap } => Error::NetworkGap { u, v, gap },
        NetworkRefusal::PastTolerance { bound } => Error::PastTolerance { bound },
    }
}

/// The Gordon surface through the u-curves `us` and the v-curves `vs`, one face, and the
/// certified bound within which every input curve lies on it (at its crossing parameter).
///
/// The network must be compatible: each family given in order, its curves running one way,
/// each u-curve crossing every v-curve once, and every v-curve crossing all the u-curves at one
/// parameter of theirs (and the other way round), within the model's zone tolerance; the
/// first and last curves of each family cross the other family at its curves' ends. The
/// surface's u runs along the u-curves, its v along the v-curves; across each family it is of
/// degree `min(3, count - 1)`. Each curve is taken over its natural range (a line from its
/// start to its end, a spline over its knots' domain).
///
/// # Errors
/// [`Error::Argument`] for fewer than two or more than 64 curves a family, a curve refused (a
/// circle, an ellipse, a traced intersection curve, numbers the kernel refuses), or a family
/// out of order; [`Error::Unsupported`] for a rational curve; [`Error::NoCrossing`] for a pair
/// that does not cross once; [`Error::NetworkGap`] for a pair not proved within the zone
/// tolerance at the common parameters; [`Error::PastTolerance`] for a surface not proved within
/// it of every curve; the kernel's status.
pub fn gordon(us: &[Curve], vs: &[Curve]) -> Result<(Record, f64)> {
    let family = MIN_CURVES..=MAX_CURVES;
    if !family.contains(&us.len()) || !family.contains(&vs.len()) {
        return Err(Error::Argument);
    }
    let mut bound = None;
    let made = run("network::gordon", |k| {
        let us = match splines(k, us)? {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        let vs = match splines(k, vs)? {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        };
        let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
        match kernel_gordon(k, &us, &vs, tol)? {
            Ok(g) => {
                bound = Some(g.bound);
                sheet(k, g.surface)
            }
            Err(r) => Ok(Err(refused(r))),
        }
    });
    let sheet = record(made)?;
    Ok((sheet, bound.ok_or(Error::Exception)?))
}

#[cfg(test)]
// The tests check against closed forms; ops itself computes none.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use solidcore::eval::SurfaceEval;

    fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
        Curve::Line { start, end }
    }

    const P: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [10.0, 10.0, 5.0],
    ];

    #[test]
    fn a_network_of_four_lines_is_the_bilinear_patch() {
        let (sheet, bound) = gordon(
            &[line(P[0], P[1]), line(P[2], P[3])],
            &[line(P[0], P[2]), line(P[1], P[3])],
        )
        .unwrap();
        assert!(bound <= 1e-9, "{bound}");
        assert_eq!(sheet.faces.len(), 1);
        let face = &sheet.faces[0];
        let eval =
            SurfaceEval::new(&solidcore_convert::surface(face.parameters()).unwrap()).unwrap();
        let [[u0, u1], [v0, v1]] = face.domain;
        for (a, b) in [(0.0, 0.0), (0.3, 0.7), (1.0, 0.5), (0.5, 1.0)] {
            let p = eval
                .point([u0 + a * (u1 - u0), v0 + b * (v1 - v0)])
                .unwrap();
            let want = |i: usize| {
                (1.0 - b) * ((1.0 - a) * P[0][i] + a * P[1][i])
                    + b * ((1.0 - a) * P[2][i] + a * P[3][i])
            };
            assert!(
                (p.x - want(0)).abs() < 1e-9
                    && (p.y - want(1)).abs() < 1e-9
                    && (p.z - want(2)).abs() < 1e-9,
                "{p:?} at ({a}, {b})"
            );
        }
    }

    #[test]
    fn mismatched_networks_are_refused_naming_the_pair() {
        let flat = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 10.0, 0.0],
        ];
        let us = [line(flat[0], flat[1]), line(flat[2], flat[3])];
        // the second v-curve stops short of the second u-curve
        assert_eq!(
            gordon(
                &us,
                &[line(flat[0], flat[2]), line(flat[1], [10.0, 5.0, 0.0])]
            ),
            Err(Error::NoCrossing {
                u: 1,
                v: 1,
                count: 0
            })
        );
        // a family out of order
        assert_eq!(
            gordon(
                &[line(flat[2], flat[3]), line(flat[0], flat[1])],
                &[line(flat[0], flat[2]), line(flat[1], flat[3])]
            ),
            Err(Error::Argument)
        );
        // one curve a family
        assert_eq!(gordon(&us[..1], &us), Err(Error::Argument));
    }

    #[test]
    fn a_network_of_cubic_splines_builds() {
        // two clamped cubic u-curves (x linear in their parameter, z bent), crossed at t = 0,
        // 1/2 and 1 by straight v-curves: crossings at common parameters on both families
        let zs = [[0.0, 2.0, -1.0, 0.0], [1.0, -2.0, 3.0, 1.0]];
        let ys = [0.0, 10.0];
        let cubic = |y: f64, z: [f64; 4]| Curve::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            points: (0..4)
                .map(|i| [10.0 * f64::from(i) / 3.0, y, z[i as usize]])
                .collect(),
        };
        // the cubic's point at t (Bernstein form)
        let at = |y: f64, z: [f64; 4], t: f64| {
            let s = 1.0 - t;
            let b = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
            [10.0 * t, y, (0..4).map(|i| b[i] * z[i]).sum::<f64>()]
        };
        let us: Vec<Curve> = (0..2).map(|k| cubic(ys[k], zs[k])).collect();
        let vs: Vec<Curve> = [0.0, 0.5, 1.0]
            .map(|t| line(at(ys[0], zs[0], t), at(ys[1], zs[1], t)))
            .to_vec();
        let (sheet, bound) = gordon(&us, &vs).unwrap();
        assert_eq!(sheet.faces.len(), 1);
        assert!(bound <= 1e-6, "{bound}");
    }
}
