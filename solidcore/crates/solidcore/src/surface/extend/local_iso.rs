//! Common-frame Natural continuation; `Local` itself is unchanged. Not upstream.
use super::{op_err, Continuity, Local};
use crate::error::Result;
use crate::geom::vector3d::Vec3;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::gamma;
use crate::surface::side::BoxSide;

pub(super) struct Made {
    pub world: BSplineSurface,
    pub local: BSplineSurface,
    pub bound: f64,
}

/// Solve on a local continuation, returning the original controls bit for bit and
/// restoring only new controls. The actual restoration residual remains in the
/// geometric certificate, never a resolution or stop allowance.
pub(super) fn natural(source: &Local, side: BoxSide, origin: Vec3, e: f64) -> Result<Option<Made>> {
    let o = [origin.x, origin.y, origin.z];
    let mut translated = source.clone();
    let mut mag = 0.0_f64;
    for row in &mut translated.rows {
        for point in row {
            let w = if source.rational { point[3] } else { 1.0 };
            for (x, &offset) in point.iter_mut().take(3).zip(&o) {
                *x = (-offset).mul_add(w, *x);
                mag = mag.max(x.abs());
            }
        }
    }
    // Higham, Accuracy and Stability of Numerical Algorithms, 2nd ed.,
    // §§3.1–3.4: `gamma_1` bounds an FMA from its rounded result. Input weights
    // here are stored data (Local::of); reject other callers rather than silently
    // losing the origin times an uncertain weight.
    if source.err_w != 0.0 {
        return Err(op_err());
    }
    translated.err = (source.err + gamma(1) * mag) * (1.0 + gamma(4));
    translated.frame = None;
    let Some(local) = translated.extended(Continuity::Natural, e)? else {
        return Ok(None);
    };
    if !local.is_finite() {
        return Ok(None);
    }
    let mut world = local.clone();
    let (mut restore, mut wmin) = (0.0_f64, f64::INFINITY);
    for ((out, row), old) in world.rows.iter_mut().zip(&local.rows).zip(&source.rows) {
        if out.len() != row.len() || old.len() > out.len() {
            return Err(op_err());
        }
        for (i, (point, input)) in out.iter_mut().zip(row).enumerate() {
            if let Some(kept) = old.get(i) {
                *point = *kept;
                continue;
            }
            let w = if source.rational { input[3] } else { 1.0 };
            wmin = wmin.min(w);
            let mut err = [0.0_f64; 3];
            for (((out, &x), &offset), error) in point
                .iter_mut()
                .take(3)
                .zip(input.iter().take(3))
                .zip(&o)
                .zip(&mut err)
            {
                *out = offset.mul_add(w, x);
                // q = RN(out - offset*w), rho = RN(q - x). The restoration's
                // exact residual differs from rho by <= `gamma_1` (|q|+|rho|).
                // Neither operation rounds a separate world-sized product.
                let q = (-offset).mul_add(w, *out);
                let rho = q - x;
                *error = rho.abs() + gamma(1) * (q.abs() + rho.abs());
            }
            let [x, y, z] = err;
            // Three operations per residual bound; then three squares, two
            // sums, sqrt, and evaluation/inflation of the norm bound: `gamma_16`.
            restore = restore.max(Vec3::new(x, y, z).length() * (1.0 + gamma(16)));
        }
    }
    // Positive rational basis: largest homogeneous-coordinate residual / w_min.
    // local.bound() includes translation and continuation errors, including the
    // source prefix whose world representation is retained instead of restored.
    let bound = (local.bound() + restore / wmin) * (1.0 + gamma(4));
    world.frame = Some(bound);
    if !(world.is_finite() && bound.is_finite()) {
        return Ok(None);
    }
    Ok(Some(Made {
        // The knots are already supplied by Local's validated continuation.
        // from_gw_nurb would fix them up again using the enlarged box's scale:
        // a large bracketing probe can then merge the SOURCE's distinct knots.
        // The existing borrowed-net constructor preserves the supplied knots;
        // the Rust Surface value owns/shares its storage independently of that flag.
        world: BSplineSurface::from_nurb(world.surface(side)?, true),
        local: BSplineSurface::from_nurb(local.surface(side)?, true),
        bound,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_common_frame_continuation_keeps_the_source_even_for_large_probes() {
        let centre = Vec3::new(1e5, 1e5, 0.0);
        let (surface, _, _) = crate::blend::blend_corner::ball::ball_triangle_patch(
            centre,
            2.0,
            &[Vec3::X, Vec3::Y, Vec3::Z],
            0,
            false,
        )
        .unwrap();
        let net = surface.gw_nurb().unwrap();
        for side in [BoxSide::UMin, BoxSide::UMax] {
            let source = Local::of(net, side).unwrap();
            for e in [source.least_step().unwrap(), 1.0, 1e12] {
                let made = natural(&source, side, centre, e).unwrap().unwrap();
                assert!(
                    crate::surface::extend::keeps(net, made.world.gw_nurb().unwrap(), side)
                        .unwrap(),
                    "{side:?} by {e:e}"
                );
            }
        }
    }
}
