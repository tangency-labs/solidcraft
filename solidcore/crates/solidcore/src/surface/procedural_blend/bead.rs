//! A weld bead's section (#15129, the welds epic): the circular arc through the two rails at
//! sagitta `h` over the chord between them. `h > 0` is a convex bead (the arc bulges away from
//! the edge), `h < 0` a concave one (it dips towards the edge); `h = 0` is the flat bead, the
//! chord itself (the chamfer's section). The rails sit at the chamfer's setbacks: a bead is a chamfer's sizes
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
//! A bead need not run the whole edge: its spans ([`BeadSpan`]) say where along the edge it
//! runs, each from one fraction of the edge's length to another, closed at both ends by the
//! plane square to the edge (a planar cap). Several spans on one edge break the bead into
//! pieces, as an intermittent weld is.
//!
//! Not upstream (solidcore's, #15129). Deviations: none. No recursion.

use crate::blend::fillet_executive::NoRoomReason;
use crate::error::blend::BlendFailure;
use crate::error::{Failure, Fit, FitMeasure, Result, SolidError};
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
    /// `InvalidInput` for a sagitta that is not finite (0 is the flat bead); for a
    /// sagitta past its bound ([`BeadDefinition::check`]) the refusal
    /// [`NoRoomReason::BeadCutsFace`] with the bound as its fit ([`FitMeasure::Sagitta`]).
    pub fn new(chamfer: ChamferDefinition, sagitta: f64) -> Result<Self> {
        if !sagitta.is_finite() {
            return Err(SolidError::invalid_input(OP, "a bead's sagitta is finite"));
        }
        let bead = Self { chamfer, sagitta };
        bead.check().map_err(|r| {
            Failure::new(OP, BlendFailure::NoRoom(NoRoomReason::BeadCutsFace))
                .with_fit(Fit::new(r.bound, FitMeasure::Sagitta, None))
                .into_error()
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
        sagitta_bound(self.chord(), self.toe_angles(), sagitta)
    }

    /// The open bound below which a convex bead does not overhang its toes: the arc leaves the
    /// chord at `phi` and the chord meets support i at `beta_i`, so it leans back over the face
    /// past the toe where `beta_i + phi > pi / 2`, `h > (c / 2) tan((pi / 2 - max(beta_i)) / 2)`
    /// (0 where a toe is already obtuse). The geometry allows such a bead up to [`Self::bound`];
    /// a blend that trims its faces at the rails takes the wrong side of a rail there (the
    /// weld's beads are built apart from the part and are not trimmed, so they are built).
    #[must_use]
    pub fn overhang_bound(&self) -> f64 {
        overhang_bound(self.chord(), self.toe_angles())
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

    /// The throat thickness `a` (ISO 2553's nominal throat; EN 1993-1-8's effective throat
    /// of a fillet weld): the distance from the root (the edge) to the weld's face, convexity
    /// excluded. For a chord or a convex bead it is the height of the triangle the root and the
    /// feet make, over the chord: `d_1 d_2 sin(alpha) / c`, whatever the legs. For a concave
    /// bead the face dips towards the root and the throat is the root's distance to the arc,
    /// `|root - centre| - R` where the circle's nearest point lies on the arc (the root over
    /// the chord between the feet: both toe angles under 90 degrees, as wherever the chamfer's
    /// offsets are positive), else the distance to the nearer foot.
    #[must_use]
    pub fn throat(&self) -> f64 {
        let [d1, _] = self.chamfer.setbacks();
        let c = self.chord();
        let height = triangle_height(self.chamfer.setbacks(), self.chamfer.wedge, c);
        if self.sagitta >= 0.0 {
            return height;
        }
        let Some((radius, _)) = BeadArc::radius_and_turn(c, self.sagitta) else {
            return height;
        };
        // the chord along x with its middle at 0, the root above it at (p, height), the
        // circle's centre below it, `radius - |h|` from the chord
        let [b1, _] = self.toe_angles();
        let p = -c / 2.0 + d1 * b1.cos();
        let h = self.sagitta.abs();
        let below = radius - h;
        let up = height + below;
        let reach = (p * p + up * up).sqrt();
        // the circle's nearest point to the root, `radius` from the centre towards it, lies on
        // the arc (over the chord) unless the root lies past a foot (an obtuse toe); then the
        // arc's nearest point is the nearer foot
        if reach > 0.0 && -below + radius * up / reach < 0.0 {
            let foot = |x: f64| ((p - x) * (p - x) + height * height).sqrt();
            return foot(-c / 2.0).min(foot(c / 2.0));
        }
        // `reach - radius` without subtracting two near-equal lengths (a shallow sagitta):
        // `reach^2 - radius^2 = p^2 + (height - h) (height - h + 2 radius)`
        (p * p + (height - h) * (height - h + 2.0 * radius)) / (reach + radius)
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
    /// or sagitta that is 0 or not finite, or a radius or angle that is not finite.
    #[must_use]
    pub fn radius_and_turn(chord: f64, sagitta: f64) -> Option<(f64, f64)> {
        let h = sagitta.abs();
        if !(chord.is_finite() && chord > 0.0 && h.is_finite() && h > 0.0) {
            return None;
        }
        let radius = (chord * chord / 4.0 + h * h) / (2.0 * h);
        let turn = 4.0 * (2.0 * h / chord).atan();
        (radius.is_finite() && turn.is_finite()).then_some((radius, turn))
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

    /// The unit tangent at angle `t`, the way the arc runs (from the first foot to the second).
    #[must_use]
    pub fn tangent(&self, t: f64) -> Vec3 {
        let (s, c) = t.sin_cos();
        self.y * c - self.x * s
    }

    /// The arc as two rational quadratic Bezier pieces, first foot to apex to second foot:
    /// control points `[foot, corner, apex, corner, foot]` and weights `[1, w, 1, w, 1]`, `w =
    /// cos(turn / 4)`, each corner where the arc's tangents at its piece's ends meet (at
    /// `radius / w` from the centre). Exact: a rational quadratic piece is a circular arc of
    /// any turn under `pi`, and each piece turns half the arc's (under `pi`, as the whole arc
    /// turns under `2 pi`).
    #[must_use]
    pub fn rows(&self) -> ([Point3; 5], [f64; 5]) {
        let half = self.turn / 2.0;
        let w = (half / 2.0).cos();
        let corner = |t: f64| {
            let (s, c) = t.sin_cos();
            self.centre + (self.x * c + self.y * s) * (self.radius / w)
        };
        (
            [
                self.point(0.0),
                corner(half / 2.0),
                self.point(half),
                corner(1.5 * half),
                self.point(self.turn),
            ],
            [1.0, w, 1.0, w, 1.0],
        )
    }
}

/// The height of the triangle the root and the feet at `setbacks` make at `wedge`, over the
/// chord `chord` between the feet: `d_1 d_2 sin(alpha) / c` (twice its area over its base).
fn triangle_height(setbacks: [f64; 2], wedge: f64, chord: f64) -> f64 {
    setbacks[0] * setbacks[1] * wedge.sin() / chord
}

/// The equal legs `z` that give a bead the throat `a` at `wedge` and `sagitta`: `z = a /
/// cos(alpha / 2)` for a chord or a convex bead (at 90 degrees `z = a sqrt 2`), `(a + |h|) /
/// cos(alpha / 2)` for a concave one (its face dips `|h|` towards the root along the
/// bisector). The inverse of [`BeadDefinition::throat`] for equal legs.
#[must_use]
pub fn equal_legs_for_throat(throat: f64, wedge: f64, sagitta: f64) -> f64 {
    let dip = if sagitta < 0.0 { sagitta.abs() } else { 0.0 };
    (throat + dip) / (wedge / 2.0).cos()
}

/// The sagitta's open bound over `chord` with the toe angles `toes` ([`BeadDefinition::bound`]
/// for the sign of `sagitta`): concave `(c / 2) tan(min(beta_i) / 2)`, convex `(c / 2)
/// tan((pi - max(beta_i)) / 2)`.
#[must_use]
pub fn sagitta_bound(chord: f64, toes: [f64; 2], sagitta: f64) -> f64 {
    let [b1, b2] = toes;
    let half = chord / 2.0;
    if sagitta < 0.0 {
        half * (b1.min(b2) / 2.0).tan()
    } else {
        half * ((PI - b1.max(b2)) / 2.0).tan()
    }
}

/// The convex sagitta at which the arc over `chord` leaves its steeper toe square to the face
/// ([`BeadDefinition::overhang_bound`]): `(c / 2) tan((pi / 2 - max(beta_i)) / 2)`, 0 where a
/// toe angle is already square or past it.
#[must_use]
pub fn overhang_bound(chord: f64, toes: [f64; 2]) -> f64 {
    let [b1, b2] = toes;
    let lean = (PI / 2.0 - b1.max(b2)).max(0.0);
    chord / 2.0 * (lean / 2.0).tan()
}

/// Where along its edge a bead runs: from `from` to `to`, fractions of the edge's length from
/// its start (`0 <= from < to <= 1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeadSpan {
    /// Where the bead starts.
    pub from: f64,
    /// Where it stops.
    pub to: f64,
}

impl BeadSpan {
    /// The whole edge.
    pub const WHOLE: Self = Self { from: 0.0, to: 1.0 };

    /// The span from `from` to `to`, checked.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an end that is not finite, outside `[0, 1]`, or a span of no length
    /// (`from >= to`).
    pub fn new(from: f64, to: f64) -> Result<Self> {
        if !(from.is_finite() && to.is_finite() && 0.0 <= from && from < to && to <= 1.0) {
            return Err(SolidError::invalid_input(
                OP,
                "a bead's span runs from 0 to 1 along its edge, its start before its end",
            ));
        }
        Ok(Self { from, to })
    }

    /// The span's share of the edge.
    #[must_use]
    pub fn share(&self) -> f64 {
        self.to - self.from
    }

    /// Spans along one edge, checked: each valid, in order along the edge, none overlapping
    /// the next. Two that meet end to start are one span: apart, each would end in its own
    /// cap on the same section.
    ///
    /// # Errors
    ///
    /// Those of [`Self::new`]; `InvalidInput` for no span, or spans out of order or
    /// overlapping.
    pub fn along(spans: &[(f64, f64)]) -> Result<Vec<Self>> {
        if spans.is_empty() {
            return Err(SolidError::invalid_input(
                OP,
                "a bead has at least one span",
            ));
        }
        let mut out: Vec<Self> = Vec::new();
        for &(from, to) in spans {
            let span = Self::new(from, to)?;
            if out.last().is_some_and(|last| span.from < last.to) {
                return Err(SolidError::invalid_input(
                    OP,
                    "a bead's spans are in order along the edge and do not overlap",
                ));
            }
            if let Some(last) = out.last_mut().filter(|last| last.to == span.from) {
                last.to = span.to;
            } else {
                out.push(span);
            }
        }
        Ok(out)
    }
}
