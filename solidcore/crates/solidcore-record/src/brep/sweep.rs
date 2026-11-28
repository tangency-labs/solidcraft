//! A screw-swept wall by its definition, as plain data: what a face's
//! [`Surface::Sweep`](super::Surface) holds (SolidCraft's docs/design/screw-surface-record.md).
//! A profile curve carried by a screw motion about an axis (a thread's flank, a coil), the
//! stations and half angles that fix the face's parameters across the turns, and the map from the
//! face's `(u, v)` to the definition's `(w, t)`, and the net's distance from it as the kernel
//! held it. The face's `nurbs` is the wall's net, stored once there.
//!
//! The kernel holds the same fields (`surface::procedural_sweep::ProceduralSweep` with a screw
//! motion) and `solidcore-convert` copies them; the kernel certifies the bound again before the
//! face is an operand, and never works to a smaller one than it proves. Nothing here evaluates, measures or compares
//! geometry: the checks are structural ([`SweepDefinition::validate`]).
//!
//! **The definition.** At `(w, t)`, on the span `[t_i, t_i+1]` of the stations, with
//! `s = 2 (t - t_i) / (t_i+1 - t_i) - 1` and `tau_i = tan(h_i / 2)` (`h_i` the span's half
//! angle), `theta = (t_i + t_i+1) / 2 + 2 atan(tau_i s)` and
//!
//! `S(w, t) = o + R_k(theta) (P(w) - o + e delta(theta)) + k pitch theta / (2 pi)`,
//!
//! `P` the profile, `R_k` the right-handed turn about the unit axis `k` through `o`, and `e`,
//! `delta` the radial law's direction and offset (none: `delta = 0`). Handedness is the sign of
//! the pitch about the right-handed axis: positive is a right-hand screw.

use serde::{Deserialize, Serialize};

use super::{Budget, Curve, Invalid, Result, knots, numbers, require, weights};

/// The most stations of one wall: `solidcore::sweep::screw::MAX_STATIONS` spans, and one more.
pub const MAX_STATIONS: usize = 12_001;
/// The most `(angle, offset)` pairs of a radial law (`solidcore::sweep::screw::MAX_LAW`).
pub const MAX_LAW: usize = 1024;
/// The widest half angle of a span: an eighth of a turn's span turns at most `pi / 8`
/// (`solidcore::surface::procedural_sweep::MAX_HALF_ANGLE`).
pub const MAX_HALF_ANGLE: f64 = std::f64::consts::TAU / 16.0;

/// A swept wall's definition. The face's `nurbs` is its net.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepDefinition {
    /// The profile `P(w)`: a 3D [`Curve::Spline`] or [`Curve::RationalSpline`].
    pub profile: Curve,
    /// How the profile is carried.
    pub motion: SweepMotion,
    /// The stations across the turns, radians, strictly ascending, at least two: the net's knots
    /// across the turns, and the wall's turn range.
    pub stations: Vec<f64>,
    /// Each span's half angle, one per span of `stations`, each in `(0, MAX_HALF_ANGLE]`.
    pub half_angles: Vec<f64>,
    /// From the face's `(u, v)` to the definition's `(w, t)`.
    pub map: SweepMap,
    /// The net's distance from the definition as the kernel held it, mm: its certified bound,
    /// with the rounding of any motion of the net since. A reader never takes it as proof: the
    /// kernel certifies the net again and keeps the larger of the two.
    pub bound: f64,
}

/// How a sweep carries its profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SweepMotion {
    /// About the axis through `origin` along the unit `axis`, rising `pitch` per turn (signed:
    /// positive is right-handed about `axis`), first moved by the radial `law`, if any.
    Screw {
        origin: [f64; 3],
        axis: [f64; 3],
        pitch: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        law: Option<RadialLaw>,
    },
}

/// The offset `delta(theta)` along the unit `direction`, square to the axis, piecewise linear
/// through (`angles`, `offsets`) (radians of turn, strictly ascending) and held at its first and
/// last offsets beyond them: a taper, or a run-out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadialLaw {
    pub direction: [f64; 3],
    pub angles: Vec<f64>,
    pub offsets: Vec<f64>,
}

/// `(w, t) = (a[0] + b[0] p, a[1] + b[1] q)`, `(p, q)` the face's `(u, v)`, or `(v, u)` when
/// `swapped`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SweepMap {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub swapped: bool,
}

impl Default for SweepMap {
    fn default() -> Self {
        Self {
            a: [0.0, 0.0],
            b: [1.0, 1.0],
            swapped: false,
        }
    }
}

/// Of length 1 within 10⁻⁹ (its squared length within `2·10⁻⁹` of 1: the record takes no roots).
fn unit(v: [f64; 3]) -> bool {
    (dot(v, v) - 1.0).abs() <= 2e-9
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| x * y).sum()
}

impl SweepDefinition {
    /// The definition's structure, counted against `budget`: a 3D spline or rational spline
    /// profile; a unit axis, and a law's unit direction square to it, within 10⁻⁹; a finite,
    /// non-zero pitch; at least two and at most [`MAX_STATIONS`] stations, strictly ascending;
    /// one half angle per span in `(0, MAX_HALF_ANGLE]`; a bound in `[0, 10³]`; a law of one to [`MAX_LAW`] strictly
    /// ascending angles with an offset each; a map of finite terms and non-zero scales; every
    /// number finite and within 10¹².
    ///
    /// # Errors
    ///
    /// [`Invalid`] naming the first check that fails.
    pub fn validate(&self, budget: &mut Budget) -> Result<()> {
        budget.add(1)?;
        match &self.profile {
            Curve::Spline { knots: k, points } => {
                knots(k, points.len(), budget)?;
                super::points(points, budget)?;
            }
            Curve::RationalSpline {
                knots: k,
                homogeneous,
            } => {
                knots(k, homogeneous.len(), budget)?;
                super::points(homogeneous, budget)?;
                weights(homogeneous)?;
            }
            _ => return Err(Invalid("a sweep's profile is a spline")),
        }
        match &self.motion {
            SweepMotion::Screw {
                origin,
                axis,
                pitch,
                law,
            } => {
                super::points(&[*origin, *axis], budget)?;
                numbers([*pitch])?;
                require(unit(*axis), "a sweep's axis is a unit vector")?;
                require(*pitch != 0.0, "a screw's pitch is not zero")?;
                if let Some(l) = law {
                    require(
                        !l.angles.is_empty()
                            && l.angles.len() <= MAX_LAW
                            && l.angles.len() == l.offsets.len(),
                        "a radial law has an offset at each of its angles",
                    )?;
                    budget.add(l.angles.len() + l.offsets.len())?;
                    super::points(&[l.direction], budget)?;
                    numbers(l.angles.iter().chain(&l.offsets).copied())?;
                    require(
                        l.angles.windows(2).all(|w| w.first() < w.get(1)),
                        "a radial law's angles ascend strictly",
                    )?;
                    require(
                        unit(l.direction) && dot(l.direction, *axis).abs() <= 1e-9,
                        "a radial law's direction is a unit vector square to the axis",
                    )?;
                }
            }
        }
        let n = self.stations.len();
        require(
            (2..=MAX_STATIONS).contains(&n),
            "a sweep has two to MAX_STATIONS stations",
        )?;
        require(
            self.half_angles.len() + 1 == n,
            "a sweep has one half angle per span",
        )?;
        budget.add(n + self.half_angles.len())?;
        numbers(self.stations.iter().copied())?;
        require(
            self.stations.windows(2).all(|w| w.first() < w.get(1)),
            "a sweep's stations ascend strictly",
        )?;
        require(
            self.half_angles
                .iter()
                .all(|&h| h > 0.0 && h <= MAX_HALF_ANGLE),
            "a sweep's spans turn more than nothing and at most an eighth of a turn",
        )?;
        require(
            self.bound.is_finite() && (0.0..=1e3).contains(&self.bound),
            "a sweep's bound",
        )?;
        let SweepMap { a, b, .. } = self.map;
        numbers(a.into_iter().chain(b))?;
        require(
            b.iter().all(|&x| x != 0.0),
            "a sweep's map has non-zero scales",
        )
    }
}
