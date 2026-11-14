//! How a panel's parameters flatten: where each point of the panel's parameter space lies once
//! its neutral surface, the surface the flat pattern is a development of, is laid flat. A panel
//! keeps its region in its own surface's parameters, so the surface's flow (its parameter
//! lines) maps exactly to the flat and back.
//!
//! Not upstream (SolidCraft's sheet development, moved into the kernel, docs/layers.md §1).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::f64::consts::{FRAC_PI_2, TAU};

use super::plane::{from_angle, Planar};
use super::seg::SegMeasure;
use super::{PanelRefusal, Sheet, Surface};
use crate::math::Vec2;
use crate::prism::profile::{Loop2, Region2, Seg2};

/// A segment a map does not keep is followed by chords, or cubic pieces, this close to it (mm).
const CHORD_TOL: f64 = 1e-4;

/// Most cubic pieces a line across a cone's parameters is followed by ([`spiral`]): more than a
/// sheet under 10⁹ mm across ever needs. A line that needs more is refused
/// ([`PanelRefusal::ConeSpiral`]), so every spiral is followed within [`CHORD_TOL`].
const MAX_SPIRAL_PIECES: usize = 4096;

/// Parameters this close are one: a line of one angle on a cone, or of one distance.
const PARAM_TOL: f64 = 1e-12;

/// From a panel's parameters to its neutral surface laid flat.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Flatten {
    /// A planar panel: its parameters are its plane's coordinates, and the neutral surface has
    /// its shape.
    Same,
    /// A cylinder: length along the axis stays, and the angle round it becomes arc length on
    /// the neutral circle of this radius. The map is affine, so a line stays a line, a circular
    /// arc becomes exact elliptical conics, and every Bezier and conic keeps its kind.
    Roll { radius: f64 },
    /// A cone: (distance from the apex along a ruling, angle round the axis) to polar
    /// coordinates in the flat, the distance moved by `shift` (the neutral surface's offset
    /// along the rulings) and the angle scaled by `scale` (the sine of the half angle). A line of
    /// one angle (a ruling) stays a line, a line of one distance (a circle round the axis)
    /// becomes an arc about the apex, and any other line becomes a spiral, followed by cubic
    /// pieces ([`spiral`]); anything else is followed by chords.
    Cone { shift: f64, scale: f64 },
}

impl Flatten {
    pub(crate) fn of(surface: &Surface, sheet: &Sheet) -> Result<Flatten, PanelRefusal> {
        let (t, k) = (sheet.thickness, sheet.k_factor);
        match surface {
            Surface::Plane => Ok(Flatten::Same),
            &Surface::Cylinder {
                radius,
                material_inward,
            } => {
                // The neutral surface lies K x T from the inside of the roll.
                let neutral = if material_inward {
                    radius - (1.0 - k) * t
                } else {
                    radius + k * t
                };
                if !(radius > 0.0 && neutral > 0.0 && neutral.is_finite()) {
                    return Err(PanelRefusal::RollRadius);
                }
                Ok(Flatten::Roll { radius: neutral })
            }
            &Surface::Cone {
                half_angle,
                material_inward,
            } => {
                if !(half_angle > 0.0 && half_angle < FRAC_PI_2) {
                    return Err(PanelRefusal::ConeHalfAngle);
                }
                // Offset along the normal by h, a cone's apex moves along the axis and every
                // point's distance from it along its ruling changes by h / tan(half angle).
                let h = if material_inward {
                    -(1.0 - k) * t
                } else {
                    k * t
                };
                Ok(Flatten::Cone {
                    shift: h / half_angle.tan(),
                    scale: half_angle.sin(),
                })
            }
        }
    }

    pub(crate) fn point(&self, p: Vec2) -> Vec2 {
        match *self {
            Flatten::Same => p,
            Flatten::Roll { radius } => Vec2::new(p.x, p.y * radius),
            Flatten::Cone { shift, scale } => from_angle(p.y * scale) * (p.x + shift),
        }
    }

    /// The parameters of a point of the neutral surface laid flat: [`Flatten::point`] undone. A
    /// cone's angle is the one nearest `near` (radians round its axis).
    pub(crate) fn param(&self, p: Vec2, near: f64) -> Vec2 {
        match *self {
            Flatten::Same => p,
            Flatten::Roll { radius } => Vec2::new(p.x, p.y / radius),
            Flatten::Cone { shift, scale } => {
                let turn = p.y.atan2(p.x);
                let turn = turn + ((near * scale - turn) / TAU).round() * TAU;
                Vec2::new(p.len() - shift, turn / scale)
            }
        }
    }

    /// The parameter direction `v` at `p`, flattened (the map's derivative there), as a unit
    /// vector.
    pub(crate) fn dir(&self, p: Vec2, v: Vec2) -> Option<Vec2> {
        match *self {
            Flatten::Same => v.normalized(),
            Flatten::Roll { radius } => Vec2::new(v.x, v.y * radius).normalized(),
            Flatten::Cone { shift, scale } => {
                let out = from_angle(p.y * scale);
                (out * v.x + out.perp() * ((p.x + shift) * scale * v.y)).normalized()
            }
        }
    }

    /// The region flattened, and for every segment of each of its loops the index of the
    /// segment of the panel's own loop it came from: an arc on a roll becomes several conics,
    /// a line across a cone's parameters several cubics, and any other curve the map does not
    /// keep chords.
    pub(crate) fn region(&self, r: &Region2) -> Result<(Region2, Vec<Vec<usize>>), PanelRefusal> {
        if let Flatten::Cone { shift, .. } = *self {
            for lp in std::iter::once(&r.outer).chain(&r.holes) {
                // Its least x by a box that holds it (see [`super::layout::loop_box`]).
                if !(super::layout::loop_box(lp).min.x + shift > 0.0) {
                    return Err(PanelRefusal::ConeApex);
                }
            }
        }
        if let Flatten::Cone { shift, scale } = *self {
            // Every line [`Flatten::seg`] follows by a spiral, within its piece budget.
            for s in std::iter::once(&r.outer)
                .chain(&r.holes)
                .flat_map(|lp| &lp.segs)
            {
                if let Seg2::Line { a, b } = *s {
                    if is_spiral(a, b)
                        && spiral_pieces(a, b, shift, scale) > MAX_SPIRAL_PIECES as f64
                    {
                        return Err(PanelRefusal::ConeSpiral);
                    }
                }
            }
        }
        let mut origins = Vec::new();
        let mut map_loop = |lp: &Loop2| {
            let (mut segs, mut from) = (Vec::new(), Vec::new());
            for (i, s) in lp.segs.iter().enumerate() {
                let before = segs.len();
                self.seg(s, &mut segs);
                from.extend(std::iter::repeat_n(i, segs.len() - before));
            }
            origins.push(from);
            Loop2 { segs }
        };
        let outer = map_loop(&r.outer);
        let holes = r.holes.iter().map(&mut map_loop).collect();
        Ok((Region2 { outer, holes }, origins))
    }

    fn seg(&self, s: &Seg2, out: &mut Vec<Seg2>) {
        match (self, *s) {
            (Flatten::Same, _) => out.push(*s),
            (
                Flatten::Roll { .. },
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                },
            ) => {
                out.extend(
                    arc_as_conics(center, radius, start, sweep)
                        .iter()
                        .map(|c| self.controls(c)),
                );
            }
            // An affine map keeps every Bezier and conic: its control points move.
            (Flatten::Roll { .. }, _) => out.push(self.controls(s)),
            (&Flatten::Cone { .. }, Seg2::Line { a, b }) if (a.y - b.y).abs() <= PARAM_TOL => {
                out.push(Seg2::Line {
                    a: self.point(a),
                    b: self.point(b),
                });
            }
            (&Flatten::Cone { shift, scale }, Seg2::Line { a, b })
                if (a.x - b.x).abs() <= PARAM_TOL =>
            {
                out.push(Seg2::Arc {
                    center: Vec2::ZERO,
                    radius: a.x + shift,
                    start: a.y * scale,
                    sweep: (b.y - a.y) * scale,
                });
            }
            (&Flatten::Cone { shift, scale }, Seg2::Line { a, b }) => {
                out.extend(spiral(a, b, shift, scale));
            }
            (&Flatten::Cone { shift, .. }, _) => {
                // Chords close enough in the flat: an angle step moves a point by its distance
                // from the apex times the step.
                let reach = 1.0 + s.start().x.abs().max(s.end().x.abs()) + shift.abs();
                let pts: Vec<Vec2> = s
                    .polyline(CHORD_TOL / reach)
                    .into_iter()
                    .map(|p| self.point(p))
                    .collect();
                out.extend(pts.windows(2).filter_map(|w| match w {
                    [a, b] => Some(Seg2::Line { a: *a, b: *b }),
                    _ => None,
                }));
            }
        }
    }

    /// A segment with its control points mapped: exact for an affine map.
    fn controls(&self, s: &Seg2) -> Seg2 {
        let m = |p: Vec2| self.point(p);
        match *s {
            Seg2::Line { a, b } => Seg2::Line { a: m(a), b: m(b) },
            Seg2::Arc { .. } => *s,
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic {
                p0: m(p0),
                p1: m(p1),
                p2: m(p2),
                p3: m(p3),
            },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic {
                a: m(a),
                apex: m(apex),
                b: m(b),
                w,
            },
            Seg2::Bezier { n, mut p } => {
                for q in p.iter_mut().take(usize::from(n) + 1) {
                    *q = m(*q);
                }
                Seg2::Bezier { n, p }
            }
        }
    }
}

/// A line from `a` to `b` across both of a cone's parameters (distance from the apex, angle round
/// the axis), flattened by [`Flatten::Cone`]'s `shift` and `scale`: an Archimedean spiral, its
/// distance from the apex `r = r₀ + t Δr` and its angle `θ = θ₀ + t Δθ` both running evenly along
/// the line's `t`. No rational curve is a spiral, so it is followed by cubic pieces over equal
/// steps of `t`, each with the spiral's point and rate at both its ends (Hermite). A piece over a
/// step `h` lies within `h⁴/384 · max|c⁗|` of the spiral `c` at the same `t` (the cubic's
/// remainder), and `c⁗ = (Δθ⁴ r − 4 i Δθ³ Δr) e^{iθ}`, so `|c⁗| ≤ |Δθ|³ √(Δθ² r² + 16 Δr²)` at
/// the line's furthest `r`, one of its ends: it takes as many pieces as keep that within
/// [`CHORD_TOL`], which the pieces still are from the spiral, not on it.
fn spiral(a: Vec2, b: Vec2, shift: f64, scale: f64) -> Vec<Seg2> {
    let (dr, dt) = (b.x - a.x, (b.y - a.y) * scale);
    // Past the budget only where [`Flatten::region`] has refused the panel already.
    let n = spiral_pieces(a, b, shift, scale).min(MAX_SPIRAL_PIECES as f64) as usize;
    // The spiral's point, and its rate along the line's t, a fraction t of the way along.
    let at = |t: f64| {
        let p = a * (1.0 - t) + b * t;
        let out = from_angle(p.y * scale);
        (
            out * (p.x + shift),
            out * dr + out.perp() * ((p.x + shift) * dt),
        )
    };
    (0..n)
        .map(|k| {
            let (t0, t1) = (
                k as f64 / n as f64,
                if k + 1 == n {
                    1.0
                } else {
                    (k + 1) as f64 / n as f64
                },
            );
            let ((p0, d0), (p3, d3)) = (at(t0), at(t1));
            let step = (t1 - t0) / 3.0;
            Seg2::Cubic {
                p0,
                p1: p0 + d0 * step,
                p2: p3 - d3 * step,
                p3,
            }
        })
        .collect()
}

/// Whether [`Flatten::seg`] follows the line from `a` to `b` on a cone by a [`spiral`]: it runs
/// across both parameters (its guards' order, a NaN taking the spiral too).
fn is_spiral(a: Vec2, b: Vec2) -> bool {
    !((a.y - b.y).abs() <= PARAM_TOL) && !((a.x - b.x).abs() <= PARAM_TOL)
}

/// How many cubic pieces keep the [`spiral`] from `a` to `b` within [`CHORD_TOL`] by the
/// Hermite remainder's bound: at least one, whatever the bound (a NaN one too); not capped.
fn spiral_pieces(a: Vec2, b: Vec2, shift: f64, scale: f64) -> f64 {
    let (dr, dt) = (b.x - a.x, (b.y - a.y) * scale);
    let far = (a.x + shift).abs().max((b.x + shift).abs());
    let fourth = dt.abs().powi(3) * (dt * dt * far * far + 16.0 * dr * dr).sqrt();
    (fourth / (384.0 * CHORD_TOL)).powf(0.25).ceil().max(1.0)
}

/// A circular arc as rational quadratic pieces of at most a quarter turn each: exact, and kept
/// exact by any affine map.
pub(crate) fn arc_as_conics(center: Vec2, radius: f64, start: f64, sweep: f64) -> Vec<Seg2> {
    let n = (sweep.abs() / FRAC_PI_2).ceil().clamp(1.0, 8.0) as usize;
    let step = sweep / n as f64;
    (0..n)
        .map(|i| {
            let a0 = start + step * i as f64;
            let half = step / 2.0;
            Seg2::Conic {
                a: center + from_angle(a0) * radius,
                apex: center + from_angle(a0 + half) * (radius / half.cos()),
                b: center + from_angle(a0 + step) * radius,
                w: half.cos(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{spiral_pieces, Flatten, PanelRefusal};
    use crate::develop::plane::Planar;
    use crate::math::Vec2;
    use crate::prism::profile::{Loop2, Region2, Seg2};

    /// A line across both of a cone's parameters flattens to the spiral it is, followed by cubic
    /// pieces. On a cone of scale 1/2 and no shift, the line from (100, 0) to (200, 1) runs from
    /// radius 100 at 0 rad to radius 200 at 0.5 rad: Δr = 100, Δθ = 0.5. The spiral's fourth
    /// derivative is at most 0.5³ √(0.5² · 200² + 16 · 100²) = 0.125 √170000 = 51.539, so a piece
    /// over 1/n of the line is within 51.539 / (384 n⁴) of it: 1.036e-4 for n = 6, and 5.59e-5 for
    /// n = 7, the first within 1e-4. Each piece is checked at nine points against the spiral at
    /// the same place along the line. The single chord it was, from (100, 0) to
    /// 200 (cos 0.5, sin 0.5), lay 12.66 mm from it at its middle.
    #[test]
    fn a_line_across_a_cones_parameters_flattens_to_its_spiral() {
        let cone = Flatten::Cone {
            shift: 0.0,
            scale: 0.5,
        };
        let (a, b) = (Vec2::new(100.0, 0.0), Vec2::new(200.0, 1.0));
        let mut out = Vec::new();
        cone.seg(&Seg2::Line { a, b }, &mut out);
        assert_eq!(out.len(), 7, "{out:?}");
        for (k, s) in out.iter().enumerate() {
            assert!(matches!(s, Seg2::Cubic { .. }), "{s:?}");
            for i in 0..=8 {
                let x = f64::from(i) / 8.0;
                let off = s
                    .point_at(x)
                    .dist(cone.point(a.lerp(b, (k as f64 + x) / 7.0)));
                assert!(off <= 5.6e-5, "piece {k} at {x}: {off}");
            }
        }
        assert_eq!(out.first().map(Seg2::start), Some(cone.point(a)));
        assert_eq!(out.last().map(Seg2::end), Some(cone.point(b)));
    }

    /// A spiral past the piece budget is refused, not followed less closely. On a cone laid flat
    /// at scale 1, the line from (10¹⁰, 0) to (2·10¹⁰, 6) has Δr = 10¹⁰, Δθ = 6 and its furthest
    /// point 2·10¹⁰ from the apex: its fourth derivative is bounded by
    /// 6³ √(6² · 4·10²⁰ + 16 · 10²⁰) = 216 · √(1.6·10²²) = 2.732·10¹³, so it needs
    /// ⌈(2.732·10¹³ / (384 · 10⁻⁴))^¼⌉ = 5165 pieces, past 4096. The same triangle 100 times
    /// smaller has a bound 100 times less, 2.732·10¹¹, and needs
    /// ⌈(2.732·10¹¹ / (384 · 10⁻⁴))^¼⌉ = 1634: it is laid flat, its other two sides a line (one
    /// angle) and an arc (one distance).
    #[test]
    fn a_spiral_past_the_piece_budget_is_refused() {
        let cone = Flatten::Cone {
            shift: 0.0,
            scale: 1.0,
        };
        let triangle = |s: f64| Region2 {
            outer: Loop2 {
                segs: vec![
                    Seg2::Line {
                        a: Vec2::new(1e10 * s, 0.0),
                        b: Vec2::new(2e10 * s, 0.0),
                    },
                    Seg2::Line {
                        a: Vec2::new(2e10 * s, 0.0),
                        b: Vec2::new(2e10 * s, 6.0),
                    },
                    Seg2::Line {
                        a: Vec2::new(2e10 * s, 6.0),
                        b: Vec2::new(1e10 * s, 0.0),
                    },
                ],
            },
            holes: Vec::new(),
        };
        let pieces = spiral_pieces(Vec2::new(2e10, 6.0), Vec2::new(1e10, 0.0), 0.0, 1.0);
        assert_eq!(pieces, 5165.0);
        assert_eq!(cone.region(&triangle(1.0)), Err(PanelRefusal::ConeSpiral));
        let small = spiral_pieces(Vec2::new(2e8, 6.0), Vec2::new(1e8, 0.0), 0.0, 1.0);
        assert_eq!(small, 1634.0);
        let (flat, from) = cone.region(&triangle(0.01)).unwrap();
        assert_eq!(flat.outer.segs.len(), 1 + 1 + 1634);
        assert_eq!(from, vec![[vec![0, 1], vec![2; 1634]].concat()]);
    }
}
