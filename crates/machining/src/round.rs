//! Swept volumes of round-nosed end mills: a ball (corner radius equal to the tool radius `R`) and a
//! bull-nose (corner radius `r`, at most `R`). With `a = R - r` and the tip at the origin, the body of
//! such a tool, about its axis, is the union of three parts:
//!
//! - a flat core: radius `a`, from the tip up to the flute length `L` (none for a ball);
//! - the rounded part: the body below height `min(r, L)`, that is the torus corner (the hemisphere for
//!   a ball), whose tube has radius `r` and whose circle has radius `a` at height `r`;
//! - a flat shank: radius `R`, from height `r` up to `L`.
//!
//! The core and the shank are flat cylinders, so their swept volumes are the flat end mill's exact
//! sweeps (the shank moved up by `r`). The rounded part is swept by sampling the tip along the move,
//! see [`RoundSweep`].

use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;

use crate::SimError;
use crate::sweep::{AXIS_EPS, ArcFixedAxis, EPS_MM, FlatTool, LinearFixedAxis, Sweep, chord_open, perp, push_clipped};

/// The most tip samples one piece takes. A guard against a pitch far below the piece length, not a
/// shortcut: the simulator cuts round-tool pieces of at most 2 mm, so this is 2 mm / 3e-5 mm.
pub const MAX_TIP_SAMPLES: usize = 1 << 16;

/// A round-nosed end mill with its tip at the origin and its axis along `+Z`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundTool {
    /// Tool radius `R`, mm.
    pub radius: f64,
    /// Corner radius `r`, mm: above zero and at most `radius`. Equal to `radius` is a ball.
    pub corner: f64,
    /// Flute length, mm, from the tip up the axis.
    pub length: f64,
}

impl RoundTool {
    /// The tool, or the reason it is refused: a radius, corner radius or flute length that is not
    /// positive and finite, or a corner radius above the tool radius.
    pub fn new(radius: f64, corner: f64, length: f64) -> Result<RoundTool, String> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(format!("tool radius must be positive and finite (got {radius})"));
        }
        if !(corner.is_finite() && corner > 0.0 && corner <= radius) {
            return Err(format!("corner radius must be above 0 and at most the tool radius {radius} mm (got {corner})"));
        }
        if !(length.is_finite() && length > 0.0) {
            return Err(format!("flute length must be positive and finite (got {length})"));
        }
        Ok(RoundTool { radius, corner, length })
    }

    /// The radius `a = R - r` of the flat core; zero for a ball.
    pub fn core_radius(&self) -> f64 {
        self.radius - self.corner
    }
}

/// The path of a round tool's tip over one piece of a move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoundPath {
    Line { from: Vec3, to: Vec3 },
    Arc(ArcFixedAxis),
}

/// One piece of a move cut with a round tool: its tip path and the tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundPiece {
    pub path: RoundPath,
    pub tool: RoundTool,
}

impl RoundPiece {
    /// The sweep of this piece. The rounded part is sampled at tip positions at most `pitch` mm apart
    /// along the path (see [`RoundSweep`] for the error). The pitch is the stock's ray spacing.
    pub fn sweep(&self, pitch: f64) -> Result<RoundSweep, SimError> {
        if !(pitch.is_finite() && pitch > 0.0) {
            return Err(SimError::BadInput("the sample pitch must be positive and finite".into()));
        }
        let t = self.tool;
        let shift = Vec3::Z * t.corner;
        let core = FlatTool::new(t.core_radius(), t.length, Vec3::Z);
        let shank = FlatTool::new(t.radius, t.length - t.corner, Vec3::Z);
        let mut parts: Vec<Box<dyn Sweep>> = Vec::with_capacity(2);
        let tips: Vec<Vec3> = match &self.path {
            RoundPath::Line { from, to } => {
                if let Some(c) = core {
                    parts.push(Box::new(LinearFixedAxis { from: *from, to: *to, tool: c }));
                }
                if let Some(s) = shank {
                    parts.push(Box::new(LinearFixedAxis { from: *from + shift, to: *to + shift, tool: s }));
                }
                let n = samples((*to - *from).len(), pitch)?;
                (0..=n).map(|k| *from + (*to - *from) * (k as f64 / n as f64)).collect()
            }
            RoundPath::Arc(arc) => {
                if let Some(c) = core {
                    parts.push(Box::new(arc.moved(Vec3::ZERO, c)));
                }
                if let Some(s) = shank {
                    parts.push(Box::new(arc.moved(shift, s)));
                }
                let n = samples(arc.length(), pitch)?;
                (0..=n).map(|k| arc.point(k as f64 / n as f64)).collect()
            }
        };
        Ok(RoundSweep { parts, tips, tool: t })
    }
}

/// The samples a piece of length `len` takes, `pitch` apart and at least one step.
fn samples(len: f64, pitch: f64) -> Result<usize, SimError> {
    let n = (len / pitch).ceil().max(1.0);
    if n.is_finite() && n <= MAX_TIP_SAMPLES as f64 {
        Ok(n as usize)
    } else {
        Err(SimError::BadInput(format!("a round tool piece of {len} mm is too long for a sample every {pitch} mm")))
    }
}

/// The swept volume of one piece of a round tool's move, for the stock's rays.
///
/// shortcut: the rounded part is swept at tip samples at most `pitch` apart, so the removed set is at
/// most `pitch / 2` short of the true sweep, normal to the surface (a tip between two samples is within
/// `pitch / 2` of one of them). The stock passes its finest ray spacing as the pitch. Along a ray that
/// grazes the surface the error measured along the ray is larger than that normal distance. Upgrade:
/// exact ray-torus windows (a quartic per tip).
///
/// shortcut: the chord of each sample is one interval (the rounded part is convex), and consecutive
/// chords are joined. For a straight piece the swept region is convex, so the join is inside it: exact
/// from inside, no overcut. For an arc the swept region is not convex, so the join overcuts by at most
/// the arc's sagitta over one pitch, `pitch^2 / (8 r)` for an arc of radius `r`. Upgrade: join only
/// across windows the true arc sweep proves.
///
/// shortcut: rays are cut along the tool axis or across it, the only directions the dexel stock casts
/// (its rays run along X, Y and Z with a 3-axis tool). An oblique ray gets no rounded part. Upgrade:
/// the general fiber of a convex body of revolution, with a root search on each side of its minimum.
pub struct RoundSweep {
    /// The flat core and shank: exact sweeps.
    parts: Vec<Box<dyn Sweep>>,
    /// The tool tip at each sample of the path, from start to end.
    tips: Vec<Vec3>,
    tool: RoundTool,
}

impl Sweep for RoundSweep {
    fn bounds(&self) -> Aabb3 {
        let base: Aabb3 = self.tips.iter().copied().collect();
        let top = self.tool.corner.min(self.tool.length);
        let r = self.tool.radius;
        // The rounded part reaches a radius out in X and Y, and from the lowest tip up to `top` in Z.
        let rounded =
            Aabb3 { min: Vec3::new(base.min.x - r, base.min.y - r, base.min.z), max: Vec3::new(base.max.x + r, base.max.y + r, base.max.z + top) };
        self.parts.iter().fold(rounded, |b, p| b.union(&p.bounds()))
    }

    fn ray(&self, o: Vec3, d: Vec3, t_range: (f64, f64), out: &mut Vec<(f64, f64)>) {
        for part in &self.parts {
            part.ray(o, d, t_range, out);
        }
        // Each tip's chord is pushed alone when it has no chord before it, and joined with the chord
        // before it otherwise. Every chord is then inside one pushed interval.
        let mut prev: Option<(f64, f64)> = None;
        for tip in &self.tips {
            let cur = rounded_chord(o, d, *tip, &self.tool);
            match (prev, cur) {
                (Some(p), Some(c)) => push_clipped(Some((p.0.min(c.0), p.1.max(c.1))), t_range, out),
                (None, Some(c)) => push_clipped(Some(c), t_range, out),
                _ => {}
            }
            prev = cur;
        }
    }
}

/// The interval of the ray `o + t d` (unclipped) inside the rounded part of a tool whose tip is at `p`:
/// the body below height `min(r, L)`, which is convex. A ray along the axis meets it in one window of
/// heights, from the corner's surface (or the tip, inside the core) up to `top`. A ray across the axis
/// sits at one height, where the rounded part is a disc. `None` when the ray misses it, and for an
/// oblique ray (see [`RoundSweep`]).
fn rounded_chord(o: Vec3, d: Vec3, p: Vec3, tool: &RoundTool) -> Option<(f64, f64)> {
    let len = d.len();
    if !(len > 0.0) {
        return None;
    }
    let w = o - p;
    let (r, rc, a) = (tool.radius, tool.corner, tool.core_radius());
    let top = rc.min(tool.length);
    let tol = AXIS_EPS * len.max(1.0);
    let wp = perp(w, Vec3::Z);
    let dp = perp(d, Vec3::Z);
    if dp.len() < tol {
        // Along the axis: the lateral distance is fixed. Past the radius the ray misses the body.
        let rho = wp.len();
        if rho > r + EPS_MM {
            return None;
        }
        let corner_lo = if rho <= a {
            0.0
        } else {
            let q = rc * rc - (rho - a) * (rho - a);
            if !(q > 2.0 * rc * EPS_MM) {
                return None;
            }
            rc - q.sqrt()
        };
        // A window starting exactly at the tip is not cut, as with the flat tool's open window.
        let lo = corner_lo.max(EPS_MM);
        if !(lo < top - EPS_MM) {
            return None;
        }
        let (h0, beta) = (w.z, d.z);
        let (t1, t2) = ((lo - h0) / beta, (top - h0) / beta);
        Some((t1.min(t2), t1.max(t2)))
    } else if d.z.abs() < tol {
        // Across the axis: the ray sits at height h0, where the rounded part is the disc of radius a + m,
        // m = sqrt(r^2 - (h0 - r)^2). The core lies inside that disc, so the disc is the whole chord.
        let h0 = w.z;
        if wp.cross(dp).len() / dp.len() > r + EPS_MM {
            return None;
        }
        // At height r the corner meets the shank, whose open window leaves that plane out: when the flute
        // continues above r, the plane is interior to the body and is cut here (its top is closed).
        let upper = if tool.length > rc { rc + EPS_MM } else { top - EPS_MM };
        if !(h0 > EPS_MM && h0 < upper) {
            return None;
        }
        let m = (rc * rc - (h0 - rc) * (h0 - rc)).max(0.0).sqrt();
        chord_open(wp, dp, a + m)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "round_tests.rs"]
mod tests;
