//! The first and second derivatives of the unit surface normal.
//!
//! Upstream: `source/SMLib/src/NL_SrfAdv.cpp`, `source/SMLib/inc/NL_SrfAdv.h`.
//!
//! | Rust                                         | Upstream                 |
//! |----------------------------------------------|--------------------------|
//! | [`eval_pt_normal_deriv`], [`NormalDerivs`]   | `N_SrfEvalPtNormalDeriv` |
//!
//! Upstream's surface extension by a distance (`ST_surxsdr`, the core of
//! `N_SrfExtendByDist`) is not ported: surfaces are extended exactly by
//! [`crate::surface::extend`].
//!
//! Deviations:
//! - [`eval_pt_normal_deriv`] returns the five derivatives in a struct instead of
//!   `ND[k][l]`.
//! - Errors are [`SolidError::Nlib`] values with the upstream `NL_*_ERR` number
//!   instead of a flag plus the global `NL_ERROR` record.

use crate::error::{NlibCode, Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::index::oob;
use crate::nurbs::knots::Side;
use crate::nurbs::math::{float_op_is_bad, FloatOp};
use crate::nurbs::srf_basic::Surface;

const OP_NDER: &str = "nurbs::srf_adv::p2::eval_pt_normal_deriv";

/// Derivatives of the unit normal `N(u, v)` (upstream `ND[k][l]`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NormalDerivs {
    /// `N_u` (`ND[1][0]`).
    pub nu: Vec3,
    /// `N_v` (`ND[0][1]`).
    pub nv: Vec3,
    /// `N_uu` (`ND[2][0]`).
    pub nuu: Vec3,
    /// `N_uv` (`ND[1][1]`).
    pub nuv: Vec3,
    /// `N_vv` (`ND[0][2]`).
    pub nvv: Vec3,
}

/// First and second derivatives of the unit normal `N = (S_u x S_v) / |S_u x S_v|`
/// at `(u, v)` (`N_SrfEvalPtNormalDeriv`), from the surface derivatives up to order
/// three. `ufl` / `vfl` select the side of a knot (for discontinuous surfaces).
///
/// # Errors
///
/// [`NlibCode::Numerical`] when `1 / |S_u x S_v|` is out of range (degenerate
/// point); the errors of [`Surface::derivs`].
pub fn eval_pt_normal_deriv(
    sur: &Surface,
    u: f64,
    v: f64,
    ufl: Side,
    vfl: Side,
) -> Result<NormalDerivs> {
    const OP: &str = OP_NDER;

    // Get surface derivatives.
    let sd = sur.derivs(u, v, ufl, vfl, true, 3, 3)?;
    let d = |k: usize, l: usize| -> Result<Vec3> {
        sd.get(k)
            .and_then(|row| row.get(l))
            .copied()
            .ok_or_else(|| oob(OP))
    };

    // Derivatives of the cross product S_u x S_v.
    let s = d(1, 0)?.cross(d(0, 1)?);

    let a = d(2, 0)?.cross(d(0, 1)?);
    let b = d(1, 0)?.cross(d(1, 1)?);
    let su_v = a + b;

    let a = d(1, 1)?.cross(d(0, 1)?);
    let b = d(1, 0)?.cross(d(0, 2)?);
    let sv_v = a + b;

    let a = d(3, 0)?.cross(d(0, 1)?);
    let b = d(2, 0)?.cross(d(1, 1)?) * 2.0;
    let c = d(1, 0)?.cross(d(2, 1)?);
    let suu_v = (a + b) + c;

    let a = d(2, 1)?.cross(d(0, 1)?);
    let b = d(2, 0)?.cross(d(0, 2)?);
    let c = d(1, 0)?.cross(d(1, 2)?);
    let suv_v = (a + b) + c;

    let a = d(1, 2)?.cross(d(0, 1)?);
    let b = d(1, 1)?.cross(d(0, 2)?) * 2.0;
    let c = d(1, 0)?.cross(d(0, 3)?);
    let svv_v = (a + b) + c;

    // Derivatives of |S_u x S_v|.
    let sm = s.dot(s).sqrt();
    if float_op_is_bad(1.0, sm, FloatOp::Division) {
        return Err(SolidError::nlib(OP, NlibCode::Numerical));
    }
    let os = 1.0 / sm;
    let mut mlt = os;

    let su = mlt * s.dot(su_v);
    let sv = mlt * s.dot(sv_v);
    let suu = mlt * (s.dot(suu_v) + su_v.dot(su_v) - su * su);
    let suv = mlt * (s.dot(suv_v) + su_v.dot(sv_v) - su * sv);
    let svv = mlt * (s.dot(svv_v) + sv_v.dot(sv_v) - sv * sv);

    // Derivatives of the unit normal.
    mlt = os * os;
    let nu = (su_v * sm - s * su) * mlt;
    let nv = (sv_v * sm - s * sv) * mlt;

    let ss = sm * sm;
    mlt *= os;

    let fac_b = 2.0 * sm * su;
    let fac_c = 2.0 * su * su - sm * suu;
    let nuu = ((suu_v * ss - su_v * fac_b) + s * fac_c) * mlt;

    let fac_b = sm * sv;
    let fac_c = sm * su;
    let fac_d = 2.0 * su * sv - sm * suv;
    let nuv = (((suv_v * ss - su_v * fac_b) - sv_v * fac_c) + s * fac_d) * mlt;

    let fac_b = 2.0 * sm * sv;
    let fac_c = 2.0 * sv * sv - sm * svv;
    let nvv = ((svv_v * ss - sv_v * fac_b) + s * fac_c) * mlt;

    Ok(NormalDerivs {
        nu,
        nv,
        nuu,
        nuv,
        nvv,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::math::{CPoint, NOW};

    #[test]
    fn normal_derivs_of_a_plane_vanish() {
        let net = (0..=1)
            .map(|i| {
                (0..=1)
                    .map(|j| CPoint::new(f64::from(i), f64::from(j), 0.0, NOW))
                    .collect()
            })
            .collect();
        let s = Surface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let nd = eval_pt_normal_deriv(&s, 0.3, 0.6, Side::Left, Side::Left).unwrap();
        for v in [nd.nu, nd.nv, nd.nuu, nd.nuv, nd.nvv] {
            assert_eq!(v.length(), 0.0);
        }
    }

    #[test]
    fn degenerate_normal_is_an_error() {
        let net = (0..=1)
            .map(|_| {
                (0..=1)
                    .map(|j| CPoint::new(0.0, f64::from(j), 0.0, NOW))
                    .collect()
            })
            .collect();
        let s = Surface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let e = eval_pt_normal_deriv(&s, 0.3, 0.6, Side::Left, Side::Left);
        assert!(matches!(
            e,
            Err(SolidError::Nlib {
                code: NlibCode::Numerical,
                ..
            })
        ));
    }
}
