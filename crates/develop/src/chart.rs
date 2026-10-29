//! How a panel's parameters flatten: where each point of the panel's parameter space lies once
//! its neutral surface, the surface the flat pattern is a development of, is laid flat. A panel
//! keeps its region in its own surface's parameters, so the surface's flow (its parameter
//! lines) maps exactly to the flat and back.

use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

use crate::fitted::{FlatMap, fit};
use crate::{Skin, Surface};

/// A segment a map does not keep is followed by chords this close to it (mm).
const CHORD_TOL: f64 = 1e-4;

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
    /// one angle (a ruling) stays a line, and a line of one distance (a circle round the axis)
    /// becomes an arc about the apex; anything else is followed by chords.
    Cone { shift: f64, scale: f64 },
    /// A free-form panel: its map fitted in its surface's own spline space. A straight segment
    /// of its parameters comes back as exact Bezier pieces; anything else is followed by chords.
    Fitted(Arc<FlatMap>),
}

impl Flatten {
    pub(crate) fn of(surface: &Surface, skin: &Skin) -> Result<Flatten, &'static str> {
        let (t, k) = (skin.thickness, skin.k_factor);
        match surface {
            Surface::Plane => Ok(Flatten::Same),
            &Surface::Cylinder { radius, material_inward } => {
                // The neutral surface lies K x T from the inside of the roll.
                let neutral = if material_inward { radius - (1.0 - k) * t } else { radius + k * t };
                if !(radius > 0.0 && neutral > 0.0 && neutral.is_finite()) {
                    return Err("a roll's radius must be more than the sheet's thickness");
                }
                Ok(Flatten::Roll { radius: neutral })
            }
            &Surface::Cone { half_angle, material_inward } => {
                if !(half_angle > 0.0 && half_angle < FRAC_PI_2) {
                    return Err("a cone's rulings make more than 0 and less than a quarter turn with its axis");
                }
                // Offset along the normal by h, a cone's apex moves along the axis and every
                // point's distance from it along its ruling changes by h / tan(half angle).
                let h = if material_inward { -(1.0 - k) * t } else { k * t };
                Ok(Flatten::Cone { shift: h / half_angle.tan(), scale: half_angle.sin() })
            }
            Surface::Spline(s) => Ok(Flatten::Fitted(Arc::new(fit(s, t, k)?))),
        }
    }

    /// The largest strain the flat leaves on the neutral surface: none but on a free-form panel.
    pub(crate) fn stretch(&self) -> f64 {
        match self {
            Flatten::Fitted(m) => m.stretch,
            _ => 0.0,
        }
    }

    pub(crate) fn point(&self, p: Vec2) -> Vec2 {
        match *self {
            Flatten::Same => p,
            Flatten::Roll { radius } => Vec2::new(p.x, p.y * radius),
            Flatten::Cone { shift, scale } => Vec2::from_angle(p.y * scale) * (p.x + shift),
            Flatten::Fitted(ref m) => m.point(p),
        }
    }

    /// The parameter direction `v` at `p`, flattened (the map's derivative there), as a unit
    /// vector.
    pub(crate) fn dir(&self, p: Vec2, v: Vec2) -> Option<Vec2> {
        match *self {
            Flatten::Same => v.normalized(),
            Flatten::Roll { radius } => Vec2::new(v.x, v.y * radius).normalized(),
            Flatten::Cone { shift, scale } => {
                let out = Vec2::from_angle(p.y * scale);
                (out * v.x + out.perp() * ((p.x + shift) * scale * v.y)).normalized()
            }
            Flatten::Fitted(ref m) => m.dir(p, v),
        }
    }

    /// The region flattened, and for every segment of each of its loops the index of the
    /// segment of the panel's own loop it came from: an arc on a roll becomes several conics,
    /// and a curve the map does not keep becomes chords.
    pub(crate) fn region(&self, r: &Region2) -> Result<(Region2, Vec<Vec<usize>>), &'static str> {
        if let Flatten::Cone { shift, .. } = *self {
            for lp in std::iter::once(&r.outer).chain(&r.holes) {
                if (&lp.polyline(CHORD_TOL)).into_iter().any(|p| !(p.x + shift > 0.0)) {
                    return Err("the cone's neutral surface would pass its apex inside the panel");
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
            (Flatten::Fitted(m), Seg2::Line { a, b }) => m.line(a, b, CHORD_TOL, out),
            (Flatten::Fitted(m), _) => {
                // Chords close enough in the flat: a parameter step moves a point by about the
                // map's rate of change there.
                let p = s.start();
                let rate = [Vec2::X, Vec2::Y].iter().map(|d| m.point(p).dist(m.point(p + *d * 1e-3)) * 1e3).fold(1.0, f64::max);
                let pts: Vec<Vec2> = s.polyline(CHORD_TOL / rate).into_iter().map(|p| m.point(p)).collect();
                out.extend(pts.windows(2).filter_map(|w| match w {
                    [a, b] => Some(Seg2::Line { a: *a, b: *b }),
                    _ => None,
                }));
            }
            (Flatten::Same, _) => out.push(*s),
            (Flatten::Roll { .. }, Seg2::Arc { center, radius, start, sweep }) => {
                out.extend(arc_as_conics(center, radius, start, sweep).iter().map(|c| self.controls(c)));
            }
            // An affine map keeps every Bezier and conic: its control points move.
            (Flatten::Roll { .. }, _) => out.push(self.controls(s)),
            (&Flatten::Cone { .. }, Seg2::Line { a, b }) if (a.y - b.y).abs() <= PARAM_TOL => {
                out.push(Seg2::Line { a: self.point(a), b: self.point(b) });
            }
            (&Flatten::Cone { shift, scale }, Seg2::Line { a, b }) if (a.x - b.x).abs() <= PARAM_TOL => {
                out.push(Seg2::Arc { center: Vec2::ZERO, radius: a.x + shift, start: a.y * scale, sweep: (b.y - a.y) * scale });
            }
            (&Flatten::Cone { shift, .. }, _) => {
                // Chords close enough in the flat: an angle step moves a point by its distance
                // from the apex times the step.
                let reach = 1.0 + s.start().x.abs().max(s.end().x.abs()) + shift.abs();
                let pts: Vec<Vec2> = s.polyline(CHORD_TOL / reach).into_iter().map(|p| self.point(p)).collect();
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
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: m(p0), p1: m(p1), p2: m(p2), p3: m(p3) },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: m(a), apex: m(apex), b: m(b), w },
            Seg2::Bezier { n, mut p } => {
                for q in p.iter_mut().take(usize::from(n) + 1) {
                    *q = m(*q);
                }
                Seg2::Bezier { n, p }
            }
        }
    }
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
                a: center + Vec2::from_angle(a0) * radius,
                apex: center + Vec2::from_angle(a0 + half) * (radius / half.cos()),
                b: center + Vec2::from_angle(a0 + step) * radius,
                w: half.cos(),
            }
        })
        .collect()
}
