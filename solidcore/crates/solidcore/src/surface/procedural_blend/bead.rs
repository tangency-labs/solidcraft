//! A weld bead's section (#15129, the welds epic): the circular arc through the two rails at
//! sagitta `h` over the chord between them. `h > 0` is a convex bead (the arc bulges away from
//! the edge), `h < 0` a concave one (it dips towards the edge); `h = 0` is the chord, a chamfer,
//! and is not a bead. The rails sit at the chamfer's setbacks: a bead is a chamfer's sizes
//! ([`ChamferDefinition`]) and its sagitta.
//!
//! In the plane square to the edge, the supports meet at the wedge angle `alpha`; the feet sit
//! at the setbacks `d_1`, `d_2` from the edge, so the chord between them is
//! `c = sqrt(d_1^2 + d_2^2 - 2 d_1 d_2 cos alpha)` and meets support i at the toe angle
//! `beta_i` of the triangle the edge and the feet make (`beta_1 + beta_2 = pi - alpha`).
//!
//! An arc at sagitta `h` over a chord `c` has radius `R = (c^2 / 4 + h^2) / (2 |h|)` and turns
//! through the central angle `Theta = 4 atan(2 |h| / c)`; it leaves the chord at each toe at
//! the angle `phi = Theta / 2`, so `|h| = (c / 2) tan(phi / 2)`. It stays clear of the faces
//! while:
//! - concave: `phi < beta_i` at both toes (past it the arc cuts into the face), so
//!   `|h| < (c / 2) tan(min(beta_i) / 2)`;
//! - convex: `phi < pi - beta_i` at both toes (past it the arc reaches the face beyond the toe),
//!   so `h < (c / 2) tan((pi - max(beta_i)) / 2)`. A convex arc may overhang its toes
//!   (`h > c / 2`, `phi > pi / 2`): it is refused only where it reaches the face.
//!
//! Both bounds are open: a sagitta at the bound touches the face. [`BeadDefinition::new`] refuses
//! a sagitta past its bound and names the bound ([`SagittaRefused`]).
//!
//! Not upstream (solidcore's, #15129). Deviations: none. No recursion.

use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::math::PI;

use super::chamfer::ChamferDefinition;

const OP: &str = "surface::procedural_blend::bead";

/// A sagitta past the bound its side allows: the bound (mm, unsigned), an open one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SagittaRefused {
    /// The sagitta asked.
    pub asked: f64,
    /// The largest `|h|` on the asked side that builds: the open bound.
    pub bound: f64,
}

/// A weld bead's sizes: the rails' setbacks (a chamfer's sizes) and the sagitta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeadDefinition {
    /// The setbacks of the rails and the wedge they were converted at.
    pub chamfer: ChamferDefinition,
    /// The arc's height over the chord's middle (mm): `> 0` convex, `< 0` concave.
    pub sagitta: f64,
}

impl BeadDefinition {
    /// The bead of `chamfer`'s setbacks at sagitta `sagitta`, checked.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a sagitta that is not finite or is 0 (the chamfer itself);
    /// [`BeadDefinition::check`]'s refusal, with its bound, for a sagitta past it.
    pub fn new(chamfer: ChamferDefinition, sagitta: f64) -> Result<Self> {
        if !(sagitta.is_finite() && sagitta != 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "a bead's sagitta is finite and not 0 (0 is the chamfer)",
            ));
        }
        let bead = Self { chamfer, sagitta };
        bead.check().map_err(|r| {
            SolidError::invalid_input(
                OP,
                if r.asked > 0.0 {
                    "a convex bead's arc reaches the face past its toe"
                } else {
                    "a concave bead's arc cuts into the face"
                },
            )
        })?;
        Ok(bead)
    }

    /// The chord between the rails.
    #[must_use]
    pub fn chord(&self) -> f64 {
        let [d1, d2] = self.chamfer.setbacks();
        (d1 * d1 + d2 * d2 - 2.0 * d1 * d2 * self.chamfer.wedge.cos())
            .max(0.0)
            .sqrt()
    }

    /// The toe angles `[beta_1, beta_2]` between the chord and supports 1 and 2.
    #[must_use]
    pub fn toe_angles(&self) -> [f64; 2] {
        let [d1, d2] = self.chamfer.setbacks();
        let (s, c) = self.chamfer.wedge.sin_cos();
        [(d2 * s).atan2(d1 - d2 * c), (d1 * s).atan2(d2 - d1 * c)]
    }

    /// The open bound on `|h|` on the side of `sagitta`'s sign: concave below
    /// `(c / 2) tan(min(beta_i) / 2)`, convex below `(c / 2) tan((pi - max(beta_i)) / 2)`.
    #[must_use]
    pub fn bound(&self, sagitta: f64) -> f64 {
        let [b1, b2] = self.toe_angles();
        let half = self.chord() / 2.0;
        if sagitta < 0.0 {
            half * (b1.min(b2) / 2.0).tan()
        } else {
            half * ((PI - b1.max(b2)) / 2.0).tan()
        }
    }

    /// Whether the sagitta is inside its bound.
    ///
    /// # Errors
    ///
    /// [`SagittaRefused`] with the bound when it is not (a NaN bound refuses too).
    pub fn check(&self) -> core::result::Result<(), SagittaRefused> {
        let bound = self.bound(self.sagitta);
        if self.sagitta.abs() < bound {
            Ok(())
        } else {
            Err(SagittaRefused {
                asked: self.sagitta,
                bound,
            })
        }
    }

    /// The bead's section area in the plane square to the edge: the triangle the edge and the
    /// feet make, plus the circular segment between arc and chord (convex) or less it
    /// (concave).
    #[must_use]
    pub fn section_area(&self) -> f64 {
        let [d1, d2] = self.chamfer.setbacks();
        let triangle = 0.5 * d1 * d2 * self.chamfer.wedge.sin();
        let arc = BeadArc::radius_and_turn(self.chord(), self.sagitta);
        let segment = arc.map_or(0.0, |(r, turn)| 0.5 * r * r * (turn - turn.sin()));
        triangle + self.sagitta.signum() * segment
    }
}

/// One section's arc: its circle's centre and radius, the unit axes `x` (from the centre to
/// the first foot) and `y` (square to it in the section's plane, towards the arc), and the
/// angle it turns through from `x` towards `y` to the second foot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeadArc {
    /// The circle's centre.
    pub centre: Point3,
    /// The circle's radius, `(c^2 / 4 + h^2) / (2 |h|)`.
    pub radius: f64,
    /// The unit vector from the centre to the first foot.
    pub x: Vec3,
    /// The unit vector square to `x` in the section's plane, towards the arc's apex.
    pub y: Vec3,
    /// The central angle from the first foot to the second, `4 atan(2 |h| / c)`.
    pub turn: f64,
}

impl BeadArc {
    /// The radius and central angle of the arc at `sagitta` over `chord`; `None` for a chord
    /// or sagitta that is 0 or not finite.
    #[must_use]
    pub fn radius_and_turn(chord: f64, sagitta: f64) -> Option<(f64, f64)> {
        let h = sagitta.abs();
        if !(chord.is_finite() && chord > 0.0 && h.is_finite() && h > 0.0) {
            return None;
        }
        Some((
            (chord * chord / 4.0 + h * h) / (2.0 * h),
            4.0 * (2.0 * h / chord).atan(),
        ))
    }

    /// The arc from foot `f1` to foot `f2` at `sagitta`, bulging along `convex` for a positive
    /// sagitta: `convex` is any vector in the section's plane on the convex side of the chord
    /// (away from the edge), `along` the edge's direction (the plane's normal).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for feet that coincide, a sagitta of 0, or a `convex` along the chord or
    /// the edge.
    pub fn new(f1: Point3, f2: Point3, along: Vec3, convex: Vec3, sagitta: f64) -> Result<Self> {
        let chord = f2 - f1;
        let (radius, turn) = Self::radius_and_turn(chord.length(), sagitta)
            .ok_or_else(|| SolidError::invalid_input(OP, "a bead's chord and sagitta"))?;
        let c = chord.unitize()?;
        let n = along.unitize()?;
        // the convex side, square to the chord and the edge
        let side = convex - c * convex.dot(c) - n * convex.dot(n);
        let toward = side.unitize()?;
        let middle = f1 + chord * 0.5;
        let apex = middle + toward * sagitta;
        let centre = apex - toward * (sagitta.signum() * radius);
        let x = (f1 - centre).unitize()?;
        let to_apex = apex - centre;
        let y = (to_apex - x * to_apex.dot(x)).unitize()?;
        Ok(Self {
            centre,
            radius,
            x,
            y,
            turn,
        })
    }

    /// The point at angle `t` from `x` towards `y`.
    #[must_use]
    pub fn point(&self, t: f64) -> Point3 {
        let (s, c) = t.sin_cos();
        self.centre + self.x * (self.radius * c) + self.y * (self.radius * s)
    }
}
