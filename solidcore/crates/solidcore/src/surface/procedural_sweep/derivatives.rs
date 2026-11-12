//! Higher screw derivatives by closed-form composition, without differencing the net.
//! Deviations: no upstream counterpart. Orders are bounded at four in either parameter.

use crate::error::{Result, SolidError};
use crate::math::{Point2, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;

use super::{checked, Motion, ProceduralSweep};

const OP: &str = "surface::procedural_sweep::derivatives";

impl ProceduralSweep {
    pub(super) fn screw_derivatives(
        &self,
        uv: Point2,
        du: usize,
        dv: usize,
        left: bool,
    ) -> Result<Vec<Vec<Vec3>>> {
        // `left` names the ending definition span. Side::Right names the ending
        // face span, so a negative t map reverses it; swapping moves t onto u.
        let side = if left == (self.map.b[1] < 0.0) {
            Side::Left
        } else {
            Side::Right
        };
        let flags = if self.map.swapped {
            EvaluateFlags {
                u_side: side,
                ..EvaluateFlags::default()
            }
        } else {
            EvaluateFlags {
                v_side: side,
                ..EvaluateFlags::default()
            }
        };
        self.screw_derivatives_with_flags(uv, du, dv, flags)
    }

    pub(crate) fn screw_derivatives_with_flags(
        &self,
        uv: Point2,
        du: usize,
        dv: usize,
        flags: EvaluateFlags,
    ) -> Result<Vec<Vec<Vec3>>> {
        if du > 4 || dv > 4 {
            return Err(SolidError::invalid_input(
                OP,
                "screw derivatives are bounded at four in each parameter",
            ));
        }
        if flags.only_upper_half && du != dv {
            return Err(SolidError::invalid_input(
                OP,
                "only the upper half needs equal orders",
            ));
        }
        let Motion::Screw(motion) = &self.motion else {
            return Err(SolidError::invalid_input(
                OP,
                "higher screw derivatives require a screw motion",
            ));
        };
        let wt = self.definition_parameters(uv);
        let domain = self.profile.natural_interval();
        let ordered = domain.min() <= domain.max();
        if !ordered {
            return Err(SolidError::invalid_input(
                OP,
                "a screw profile needs ordered parameter bounds",
            ));
        }
        let w = crate::math::clamp_between(wt.x, domain.min(), domain.max());
        let (dw, dt) = if self.map.swapped { (dv, du) } else { (du, dv) };
        let (w_side, t_side) = if self.map.swapped {
            (flags.v_side, flags.u_side)
        } else {
            (flags.u_side, flags.v_side)
        };
        let ending_w = (w_side == Side::Right) != (self.map.b[0] < 0.0);
        let ending_t = (t_side == Side::Right) != (self.map.b[1] < 0.0);
        let profile = if let Some(offset) = &self.offset {
            offset.derivatives(motion, w, dw)?
        } else {
            self.profile.evaluate(w, dw, !ending_w, false)?
        };
        let (turn, rotation) = self.checked_turn_rotation(wt.y, ending_t)?;
        let (theta, rate) = (turn.theta, turn.rate);
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return Err(SolidError::invalid_input(OP, "empty screw stations"));
        };
        let t = crate::math::clamp_between(wt.y, first, last);
        let index = self
            .stations
            .partition_point(|&s| if ending_t { s < t } else { s <= t })
            .saturating_sub(1)
            .min(self.stations.len().saturating_sub(2));
        let (Some(&a), Some(&b)) = (self.stations.get(index), self.stations.get(index + 1)) else {
            return Err(SolidError::invalid_input(OP, "missing screw span"));
        };
        let alpha = 2.0 * turn.tau / (b - a);
        let x = alpha * (t - 0.5 * (a + b));
        let den = 1.0 + x * x;
        let second = -4.0 * alpha * alpha * x / (den * den);
        let third = -4.0 * alpha * alpha * alpha * (1.0 - 3.0 * x * x) / (den * den * den);
        let fourth =
            48.0 * alpha * alpha * alpha * alpha * x * (1.0 - x * x) / (den * den * den * den);
        let slope = motion
            .law
            .as_ref()
            .map_or(0.0, |law| law.slope_over(first, last));
        let (shift, shift_rate) = motion.shift(theta, slope);
        let mut source = vec![vec![Vec3::ZERO; dt + 1]; dw + 1];
        for (i, row) in source.iter_mut().enumerate() {
            let p = profile
                .get(i)
                .copied()
                .ok_or_else(|| SolidError::invalid_input(OP, "missing profile derivative"))?;
            let arm = if i == 0 { p - motion.origin + shift } else { p };
            let r0 = rotation.turn(motion.axis, arm);
            let r1 = motion.axis.cross(r0)
                + if i == 0 {
                    rotation.turn(motion.axis, shift_rate)
                } else {
                    Vec3::ZERO
                };
            let r2 = motion.axis.cross(motion.axis.cross(r0))
                + if i == 0 {
                    motion.axis.cross(rotation.turn(motion.axis, shift_rate)) * 2.0
                } else {
                    Vec3::ZERO
                };
            let r3 = motion.axis.cross(motion.axis.cross(motion.axis.cross(r0)))
                + if i == 0 {
                    motion
                        .axis
                        .cross(motion.axis.cross(rotation.turn(motion.axis, shift_rate)))
                        * 3.0
                } else {
                    Vec3::ZERO
                };
            let r4 = motion
                .axis
                .cross(motion.axis.cross(motion.axis.cross(motion.axis.cross(r0))))
                + if i == 0 {
                    motion.axis.cross(
                        motion
                            .axis
                            .cross(motion.axis.cross(rotation.turn(motion.axis, shift_rate))),
                    ) * 4.0
                } else {
                    Vec3::ZERO
                };
            let theta_first = r1
                + if i == 0 {
                    motion.axis * (motion.pitch / std::f64::consts::TAU)
                } else {
                    Vec3::ZERO
                };
            for (j, value) in row.iter_mut().enumerate() {
                *value = match j {
                    0 => {
                        r0 + if i == 0 {
                            motion.origin + motion.axis * motion.lift(theta)
                        } else {
                            Vec3::ZERO
                        }
                    }
                    1 => theta_first * rate,
                    2 => r2 * (rate * rate) + theta_first * second,
                    3 => {
                        r3 * (rate * rate * rate) + r2 * (3.0 * rate * second) + theta_first * third
                    }
                    _ => {
                        r4 * checked::powi(rate, 4)?
                            + r3 * (6.0 * rate * rate * second)
                            + r2 * (3.0 * second * second + 4.0 * rate * third)
                            + theta_first * fourth
                    }
                };
            }
        }
        // The map's checked powers, formed once per definition and kept
        // ([`ProceduralSweep::checked_map_power`]): a power is a pure function of its
        // base and order, and a refused one is formed afresh where the partial that needs
        // it first is, so it carries the same error there.
        let power = |axis: usize, n: usize| self.checked_map_power(axis, n);
        let mut result = vec![vec![Vec3::ZERO; dv + 1]; du + 1];
        for (i, row) in result.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                let (w, t) = if self.map.swapped { (j, i) } else { (i, j) };
                let derivative = source
                    .get(w)
                    .and_then(|row| row.get(t))
                    .copied()
                    .ok_or_else(|| SolidError::invalid_input(OP, "missing screw derivative"))?;
                *value = derivative * (power(0, w)? * power(1, t)?);
                if ![value.x, value.y, value.z].iter().all(|x| x.is_finite()) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "screw derivatives overflowed",
                    ));
                }
            }
        }
        let point = result
            .first()
            .and_then(|row| row.first())
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "missing screw point"))?;
        self.verify_turned_point(uv, point, &turn, &rotation)?;
        Ok(result)
    }
}
