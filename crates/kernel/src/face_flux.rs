//! The flux of a field through a face of a B-rep record, by Green's theorem over the face's
//! pcurves: a quadrature for the sign of a shell's volume or a winding number. Moved here from
//! SolidCraft's record evaluator (`solidcraft_geom::brep::eval`), as it was: its pieces and
//! pcurve points are solidcore's (`solidcore_ops::pcurve`), so it belongs to the kernel adapter.
use solidcore_record::Vec3;
use solidcore_record::brep::Curve2;
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::prelude::*;

/// Four-point Gauss–Legendre nodes and weights on `[−1, 1]`.
const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
];

/// How many Gauss panels cover a v range.
const FLUX_PANELS: usize = 4;

/// The flux of `field` through a face, `∫∫ field(S)·(S_u × S_v) du dv` over the (u, v) region its
/// loops (record convention, each use its pcurve and direction) bound, by Green's theorem:
/// `∮ −G du` with `G(u, v) = ∫ field·N ds` from `v0` to `v`. A quadrature for the sign of a
/// shell's volume (`field = p / 3`) or a winding number, which only has to be right to well
/// under a half: no geometry is made from it. Loops round a periodic direction need no seam
/// edge, as a seam's two runs at constant u add nothing to `∮ G du`.
pub(crate) fn face_flux(s: &SurfaceEval, v0: f64, loops: &[Vec<(&Curve2, bool)>], field: &dyn Fn(Vec3) -> Vec3) -> Option<f64> {
    let g = |u: f64, v: f64| -> Option<f64> {
        let (mut sum, h) = (0.0, (v - v0) / FLUX_PANELS as f64);
        for panel in 0..FLUX_PANELS {
            let a = v0 + h * panel as f64;
            for (x, w) in GAUSS4 {
                let t = a + 0.5 * h * (x + 1.0);
                let (p, su, sv) = s.derivs(u, t)?;
                sum += 0.5 * h * w * field(p).dot(su.cross(sv));
            }
        }
        sum.is_finite().then_some(sum)
    };
    let mut total = 0.0;
    for (c, forward) in loops.iter().flatten() {
        // `∫ −G u′ dt` over the pcurve, Gauss–Legendre on each piece; a use against the pcurve's
        // direction runs it backwards.
        let sign = if *forward { 1.0 } else { -1.0 };
        for [a, b] in solidcore_ops::pcurve::flux_pieces(c)? {
            let (mid, half) = (0.5 * (a + b), 0.5 * (b - a));
            for (x, w) in GAUSS4 {
                let (p, d) = solidcore_ops::pcurve::eval_d1(c, mid + half * x)?;
                total -= sign * w * half * g(p[0], p[1])? * d[0];
            }
        }
    }
    total.is_finite().then_some(total)
}
