//! A chamfer's sizes as its definition keeps them (#2486): two distances, a distance and an
//! angle, or a chord, with the wedge angle they were converted at. A chamfer is the section
//! generator's order 0, the straight chord between the feet of a point that stands off the
//! two supports by the offsets `r_1`, `r_2` (the ball's offsets); the sizes say what those
//! offsets were asked as.
//!
//! In the plane square to the edge, the supports meet at the wedge angle `alpha` (the angle
//! of the wedge the offsets' point sits in, `0 < alpha < pi`). A point at `r_1` off support
//! 1 and `r_2` off support 2 has its feet at the setbacks
//!
//! `d_1 = (r_2 + r_1 cos alpha) / sin alpha`, `d_2 = (r_1 + r_2 cos alpha) / sin alpha`
//!
//! from the edge, along supports 1 and 2, so `r_1 = (d_2 - d_1 cos alpha) / sin alpha` and
//! `r_2 = (d_1 - d_2 cos alpha) / sin alpha`. An offset that is not positive is a chamfer
//! whose distances fold: the chord would leave the wedge. A distance `d` along support 1 and
//! the chamfer's angle `theta` to it set `d_2 = d sin theta / sin(alpha + theta)` (the
//! triangle of the edge and the two feet), `alpha + theta < pi`; a chord `c` has equal
//! setbacks `d = c / (2 sin(alpha / 2))`.
//!
//! Between planes `alpha` is the planes' angle, and the sizes hold all along the edge.
//!
//! A chamfer's distances are setbacks: straight lines from the edge, in the plane square to it,
//! at every station (#13710). Where a support's section there is a circle, the straight-section
//! formula puts the feet past the setbacks on a convex support and short of them on a concave
//! one. On rims of surfaces sharing an axis and on straight edges along a plane or a parallel
//! cylinder, every station's sections are congruent, so [`feet_on_sections`] gives offsets
//! that hold the setbacks all along. The definition keeps them as the straight-section reading
//! of those offsets ([`ChamferDefinition::of_offsets`]), not the sizes asked (phase 1:
//! `blend::fillet_executive::session::two_offsets`, `blend::chamfer_sections`). Elsewhere the
//! sizes still hold only where the wedge was measured: the offsets, not the setbacks, are what
//! the definition keeps along the edge. Edges of one size on a tangent chain are one
//! chamfer, so one pair of offsets, and their rails meet where the edges run on into each
//! other: its wedge is that of its edges between planes, on which the sizes then hold all
//! along. Without one it is the narrowest at its edges' middles, whose offsets are the
//! smallest, so a setback is nowhere much past its size; unequal sizes can fold at it, so it
//! moves up to the least wedge at which the smallest setback keeps a thousand edge tolerances
//! at the chain's widest wedge, bounded from above by enclosures of the faces' normals along
//! each edge (`blend::chamfer_side::widest_wedge_bound`), not by samples. The sizes drift. A rim whose wedge runs from 63.4 to
//! 116.6 degrees, asked 1 and 2, is measured at 86.4 and has setbacks from 0.01 to 1.95 and
//! from 1.73 to 2.61. Setbacks that hold at every station are another definition. Edges
//! between planes of different wedges have no one pair: each keeps its own.
//!
//! Not upstream (solidcore's, #2486, #13710). Deviations: none. No recursion: the chord's
//! bisection is a fixed number of halvings.

use crate::error::{Result, SolidError};
use crate::math::PI;

const OP: &str = "surface::procedural_blend::chamfer";

/// How a chamfer's size was asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChamferSize {
    /// The setbacks from the edge along supports 1 and 2 (equal distances: the same twice).
    Distances([f64; 2]),
    /// The setback from the edge along support 1, and the angle (radians) between the
    /// chamfer and support 1.
    DistanceAngle {
        /// The setback along support 1.
        distance: f64,
        /// The chamfer's angle to support 1, `0 < angle < pi - wedge`.
        angle: f64,
    },
    /// The chord between the rails (equal setbacks; `SmApiChamferFillet`'s distance).
    Chord(f64),
}

/// A chamfer's sizes and the wedge angle they were converted to offsets at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChamferDefinition {
    /// The sizes as asked.
    pub size: ChamferSize,
    /// The angle between the supports at the edge, inside the wedge the chamfer cuts
    /// across, `0 < wedge < pi`.
    pub wedge: f64,
}

fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

impl ChamferDefinition {
    /// The sizes at the wedge `wedge`, checked: every size finite and positive, the wedge
    /// and the angle inside their ranges.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a size that is not finite and positive, a wedge outside
    /// `(0, pi)`, or an angle outside `(0, pi - wedge)`.
    pub fn new(size: ChamferSize, wedge: f64) -> Result<Self> {
        if !(wedge.is_finite() && wedge > 0.0 && wedge < PI) {
            return Err(SolidError::invalid_input(
                OP,
                "a chamfer's wedge angle is between 0 and pi",
            ));
        }
        let sizes_ok = match size {
            ChamferSize::Distances([a, b]) => positive(a) && positive(b),
            ChamferSize::DistanceAngle { distance, angle } => {
                positive(distance) && positive(angle) && angle + wedge < PI
            }
            ChamferSize::Chord(c) => positive(c),
        };
        if !sizes_ok {
            return Err(SolidError::invalid_input(
                OP,
                "a chamfer's sizes are finite and positive, its angle less than pi less the wedge's",
            ));
        }
        Ok(Self { size, wedge })
    }

    /// The sizes of the chamfer of `offsets` at `wedge`: its two setbacks.
    ///
    /// # Errors
    ///
    /// Those of [`Self::new`] (an offset that is not positive gives a setback that may
    /// not be).
    pub fn of_offsets(offsets: [f64; 2], wedge: f64) -> Result<Self> {
        let [r1, r2] = offsets;
        let (s, c) = wedge.sin_cos();
        Self::new(
            ChamferSize::Distances([(r2 + r1 * c) / s, (r1 + r2 * c) / s]),
            wedge,
        )
    }

    /// The setbacks from the edge along supports 1 and 2.
    #[must_use]
    pub fn setbacks(&self) -> [f64; 2] {
        match self.size {
            ChamferSize::Distances(d) => d,
            ChamferSize::DistanceAngle { distance, angle } => [
                distance,
                distance * angle.sin() / (self.wedge + angle).sin(),
            ],
            ChamferSize::Chord(c) => {
                let d = c / (2.0 * (0.5 * self.wedge).sin());
                [d, d]
            }
        }
    }

    /// The offsets `r_1`, `r_2` of the point whose feet are the chamfer's rails.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where an offset is not positive: the distances fold (the chord
    /// would leave the wedge; a second distance at most the first's `cos(wedge)`).
    pub fn offsets(&self) -> Result<[f64; 2]> {
        let [d1, d2] = self.setbacks();
        let (s, c) = self.wedge.sin_cos();
        let r = [(d2 - d1 * c) / s, (d1 - d2 * c) / s];
        if !(positive(r[0]) && positive(r[1])) {
            return Err(SolidError::invalid_input(
                OP,
                "the chamfer's distances fold: each must be more than the other's cosine of the wedge",
            ));
        }
        Ok(r)
    }

    /// Whether `offsets` are this chamfer's, to the rounding of computing them.
    #[must_use]
    pub fn holds(&self, offsets: [f64; 2]) -> bool {
        let Ok(r) = self.offsets() else {
            return false;
        };
        let [d1, d2] = self.setbacks();
        // the conversion's rounding: a few operations on the setbacks, over sin(wedge)
        let room = 64.0 * f64::EPSILON * (d1.abs() + d2.abs()) / self.wedge.sin();
        r.iter().zip(offsets).all(|(&a, b)| (a - b).abs() <= room)
    }

    /// The chamfer scaled by `factor` (a uniform scale): its lengths scale, its angles
    /// stay.
    #[must_use]
    pub fn scaled(&self, factor: f64) -> Self {
        let size = match self.size {
            ChamferSize::Distances([a, b]) => ChamferSize::Distances([a * factor, b * factor]),
            ChamferSize::DistanceAngle { distance, angle } => ChamferSize::DistanceAngle {
                distance: distance * factor,
                angle,
            },
            ChamferSize::Chord(c) => ChamferSize::Chord(c * factor),
        };
        Self {
            size,
            wedge: self.wedge,
        }
    }
}

/// One support's section at a station of a chamfer's edge, in the plane square to the edge
/// (#13710), in coordinates of that plane with the edge point at the origin: `t` the unit
/// tangent into the face, `w` the unit normal toward the other support, `k` the section's
/// signed curvature, `> 0` where its centre is on the `w` side (a convex support), `< 0` for a
/// socket or groove, 0 for a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChamferSection {
    /// The unit tangent into the face.
    pub t: [f64; 2],
    /// The unit normal toward the other support.
    pub w: [f64; 2],
    /// The signed curvature.
    pub k: f64,
}

/// The feet of a chamfer of straight setbacks on two sections, and the offsets of the point
/// that stands off both supports along their normals at the feet ([`feet_on_sections`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionFeet {
    /// The feet on supports 1 and 2.
    pub feet: [[f64; 2]; 2],
    /// The offsets `r_1`, `r_2`.
    pub offsets: [f64; 2],
    /// The point the offsets stand off at.
    pub point: [f64; 2],
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// The foot at straight distance `d` along `s` and its section's normal there toward the
/// `w` side: `F = d (cos b t + sin b w)`, `nu = cos 2b w - sin 2b t`, `b = asin(d k / 2)` (the
/// angle between a chord of the circle and its tangent at the edge). `None` where `|d k| >= 2`
/// (no point of the circle so far) or `d` is not finite and positive.
fn foot(s: &ChamferSection, d: f64) -> Option<([f64; 2], [f64; 2])> {
    let dk = d * s.k;
    if !(positive(d) && dk.is_finite() && dk.abs() < 2.0) {
        return None;
    }
    let (sb, cb) = (0.5 * dk).asin().sin_cos();
    let (s2, c2) = (2.0 * sb * cb, cb * cb - sb * sb);
    let f = [
        d * (cb * s.t[0] + sb * s.w[0]),
        d * (cb * s.t[1] + sb * s.w[1]),
    ];
    let nu = [c2 * s.w[0] - s2 * s.t[0], c2 * s.w[1] - s2 * s.t[1]];
    Some((f, nu))
}

/// The chamfer of setbacks `d` (straight lines from the edge) on `sections`: its feet, and
/// the offsets `r_1 = x(F_2 - F_1, nu_2) / x(nu_1, nu_2)`, `r_2 = x(F_2 - F_1, nu_1) / x(nu_1,
/// nu_2)` of the point that stands off support 1 along its normal at its foot and support 2
/// along its (`x` the cross product of the plane). With both sections lines it is
/// [`ChamferDefinition::offsets`]' straight-section formula. `None` unless each foot exists,
/// the normals cross at a sine of at least `least_cross`, both offsets are positive, and each
/// is below its support's radius of curvature where the support is convex.
#[must_use]
pub fn feet_on_sections(
    sections: [ChamferSection; 2],
    d: [f64; 2],
    least_cross: f64,
) -> Option<SectionFeet> {
    let [s1, s2] = sections;
    let (f1, nu1) = foot(&s1, d[0])?;
    let (f2, nu2) = foot(&s2, d[1])?;
    let x = cross(nu1, nu2);
    if !(x.is_finite() && x.abs() >= least_cross) {
        return None;
    }
    let g = [f2[0] - f1[0], f2[1] - f1[1]];
    let r = [cross(g, nu2) / x, cross(g, nu1) / x];
    for (ri, s) in r.iter().zip([s1, s2]) {
        if !positive(*ri) || (s.k > 0.0 && *ri * s.k >= 1.0) {
            return None;
        }
    }
    Some(SectionFeet {
        feet: [f1, f2],
        offsets: r,
        point: [f1[0] + r[0] * nu1[0], f1[1] + r[0] * nu1[1]],
    })
}

/// The halvings [`setback_for_chord`]'s bisection takes: fixed, so it stops.
const CHORD_HALVINGS: usize = 64;

/// The equal setbacks whose feet on `sections` are `chord` apart, on the branch where the
/// feet's distance grows with the setback: a doubling out from the straight-section setback
/// until the feet are at least `chord` apart, then a bisection of `CHORD_HALVINGS` (64) halvings.
/// `None` where no setback reaches it before a foot runs off its circle (`|d k| >= 2`), or the
/// distance is not growing at the root.
#[must_use]
pub fn setback_for_chord(sections: [ChamferSection; 2], chord: f64) -> Option<f64> {
    if !positive(chord) {
        return None;
    }
    let apart = |d: f64| -> Option<f64> {
        let (f1, _) = foot(&sections[0], d)?;
        let (f2, _) = foot(&sections[1], d)?;
        Some((f2[0] - f1[0]).hypot(f2[1] - f1[1]))
    };
    // the straight sections' setback: the chord over 2 sin(wedge / 2)
    let [s1, s2] = sections;
    let wedge = (s1.t[0] * s2.t[0] + s1.t[1] * s2.t[1])
        .clamp(-1.0, 1.0)
        .acos();
    let half = (0.5 * wedge).sin();
    if !positive(half) {
        return None;
    }
    let mut lo = 0.0;
    let mut hi = chord / (2.0 * half);
    let mut reached = false;
    for _ in 0..CHORD_HALVINGS {
        match apart(hi) {
            Some(a) if a >= chord => {
                reached = true;
                break;
            }
            Some(_) => {
                lo = hi;
                hi *= 2.0;
            }
            None => break,
        }
    }
    if !reached {
        // past a foot's circle: halve back toward the last setback short of the chord
        let top = sections
            .iter()
            .filter(|s| s.k != 0.0)
            .map(|s| 2.0 / s.k.abs())
            .fold(f64::INFINITY, f64::min);
        if !(top.is_finite() && top > lo) {
            return None;
        }
        hi = top * (1.0 - 1e-12);
        if !apart(hi).is_some_and(|a| a >= chord) {
            return None;
        }
    }
    for _ in 0..CHORD_HALVINGS {
        let mid = 0.5 * (lo + hi);
        match apart(mid) {
            Some(a) if a >= chord => hi = mid,
            Some(_) => lo = mid,
            None => return None,
        }
    }
    let d = hi;
    // growing at the root: the distance a little further on is more
    let on = apart(d * (1.0 + 1e-6))?;
    (on > chord).then_some(d)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-14 * (1.0 + b.abs())
    }

    #[test]
    fn a_right_angle_s_offsets_are_the_other_setbacks() {
        let c = ChamferDefinition::new(ChamferSize::Distances([1.0, 2.0]), 0.5 * PI).unwrap();
        let [r1, r2] = c.offsets().unwrap();
        assert!(close(r1, 2.0) && close(r2, 1.0), "{r1} {r2}");
        let back = ChamferDefinition::of_offsets([r1, r2], 0.5 * PI).unwrap();
        let [d1, d2] = back.setbacks();
        assert!(close(d1, 1.0) && close(d2, 2.0), "{d1} {d2}");
        assert!(c.holds([r1, r2]));
    }

    #[test]
    fn a_chord_s_setbacks_and_offsets() {
        // a chord of 1 on a square edge: setbacks 1/sqrt 2, offsets 1/sqrt 2
        let c = ChamferDefinition::new(ChamferSize::Chord(1.0), 0.5 * PI).unwrap();
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let [d1, d2] = c.setbacks();
        let [r1, r2] = c.offsets().unwrap();
        assert!(close(d1, h) && close(d2, h) && close(r1, h) && close(r2, h));
    }

    #[test]
    fn a_distance_and_angle_on_a_square_edge() {
        // 45 degrees on a square edge: the second setback is the first
        let c = ChamferDefinition::new(
            ChamferSize::DistanceAngle {
                distance: 2.0,
                angle: 0.25 * PI,
            },
            0.5 * PI,
        )
        .unwrap();
        let [d1, d2] = c.setbacks();
        assert!(close(d1, 2.0) && close(d2, 2.0), "{d1} {d2}");
        // 30 degrees: tan 30 times the first
        let c = ChamferDefinition::new(
            ChamferSize::DistanceAngle {
                distance: 3.0,
                angle: PI / 6.0,
            },
            0.5 * PI,
        )
        .unwrap();
        assert!(close(c.setbacks()[1], 3.0 * (PI / 6.0).tan()));
    }

    #[test]
    fn distances_that_fold_are_refused() {
        // a 60 degree wedge: the second distance must exceed half the first
        let c = ChamferDefinition::new(ChamferSize::Distances([2.0, 0.9]), PI / 3.0).unwrap();
        assert!(c.offsets().is_err());
        let c = ChamferDefinition::new(ChamferSize::Distances([2.0, 1.1]), PI / 3.0).unwrap();
        assert!(c.offsets().is_ok());
    }

    /// A line section along `t`, with `w` its normal toward the other support.
    fn line(t: [f64; 2], w: [f64; 2]) -> ChamferSection {
        ChamferSection { t, w, k: 0.0 }
    }

    #[test]
    fn straight_sections_give_the_straight_section_formula() {
        // a 60 degree wedge: support 1 along +x, support 2 at 60 degrees
        let a = PI / 3.0;
        let (sa, ca) = a.sin_cos();
        let s = [line([1.0, 0.0], [0.0, 1.0]), line([ca, sa], [sa, -ca])];
        let got = feet_on_sections(s, [1.5, 2.0], 0.1).unwrap().offsets;
        let want = ChamferDefinition::new(ChamferSize::Distances([1.5, 2.0]), a)
            .unwrap()
            .offsets()
            .unwrap();
        assert!(
            close(got[0], want[0]) && close(got[1], want[1]),
            "{got:?} {want:?}"
        );
    }

    /// The hemisphere of radius 5 at a right angle (#13710): the flat along +x from the rim,
    /// the ball up it. The offsets in closed form: `r_ball = R - 2R²(R - d_f)/(2R² - d_b²)`,
    /// `r_flat = d_b sqrt(1 - d_b²/4R²)(R - d_f)/(R - d_b²/2R)`.
    #[test]
    fn a_hemisphere_rim_s_offsets_are_its_closed_forms() {
        let r = 5.0_f64;
        let s = [
            line([1.0, 0.0], [0.0, 1.0]),
            ChamferSection {
                t: [0.0, 1.0],
                w: [1.0, 0.0],
                k: 1.0 / r,
            },
        ];
        for (df, db) in [(2.0_f64, 1.0_f64), (1.0, 2.0), (0.5, 0.5), (4.0, 4.0)] {
            let f = feet_on_sections(s, [df, db], 0.1).unwrap();
            let ball = r - 2.0 * r * r * (r - df) / (2.0 * r * r - db * db);
            let flat =
                db * (1.0 - db * db / (4.0 * r * r)).sqrt() * (r - df) / (r - db * db / (2.0 * r));
            assert!(
                close(f.offsets[0], flat) && close(f.offsets[1], ball),
                "{df} {db}: {f:?}"
            );
        }
        // 0.2 on the flat and 2 on the ball: the ball's offset would be negative
        assert!(feet_on_sections(s, [0.2, 2.0], 0.1).is_none());
        // past the ball's diameter: no foot
        assert!(feet_on_sections(s, [1.0, 10.0], 0.1).is_none());
    }

    /// A chord on a right angle with one circle of curvature `k`: `c² = 2d² - k d³`.
    #[test]
    fn a_chord_s_setback_on_one_curved_section() {
        for k in [0.2_f64, -0.2, 1.0 / 3.0] {
            let s = [
                line([1.0, 0.0], [0.0, 1.0]),
                ChamferSection {
                    t: [0.0, 1.0],
                    w: [1.0, 0.0],
                    k,
                },
            ];
            for d in [0.3_f64, 1.0, 1.5] {
                let c = (2.0 * d * d - k * d * d * d).sqrt();
                let got = setback_for_chord(s, c).unwrap();
                assert!((got - d).abs() <= 1e-13 * (1.0 + d), "k {k} d {d}: {got}");
            }
        }
        // two lines: the straight-section setback
        let s = [line([1.0, 0.0], [0.0, 1.0]), line([0.0, 1.0], [1.0, 0.0])];
        let got = setback_for_chord(s, 1.0).unwrap();
        assert!((got - std::f64::consts::FRAC_1_SQRT_2).abs() <= 1e-15);
        assert!(setback_for_chord(s, f64::NAN).is_none());
    }

    #[test]
    fn hostile_sizes_are_refused() {
        for (size, wedge) in [
            (ChamferSize::Distances([f64::NAN, 1.0]), 1.0),
            (ChamferSize::Distances([1.0, -1.0]), 1.0),
            (ChamferSize::Chord(0.0), 1.0),
            (ChamferSize::Chord(1.0), PI),
            (ChamferSize::Chord(1.0), f64::INFINITY),
            (
                ChamferSize::DistanceAngle {
                    distance: 1.0,
                    angle: 2.0,
                },
                2.0,
            ),
        ] {
            assert!(ChamferDefinition::new(size, wedge).is_err(), "{size:?}");
        }
    }
}
