//! Swept volumes of round-nosed end mills: a ball (corner radius equal to the tool radius `R`) and a
//! bull-nose (corner radius `r`, below `R`). With `a = R - r` and the tip at the origin, the body of
//! such a tool, about its axis, is the union of three parts:
//!
//! - a flat core: radius `a`, from the tip up to the flute length `L` (none for a ball);
//! - the rounded part: the torus corner, whose tube has radius `r` and whose circle has radius `a`
//!   at height `r` (the hemisphere for a ball), kept below height `min(r, L)`;
//! - a flat shank: radius `R`, from height `r` up to `L`.
//!
//! The core and the shank are flat cylinders, so their swept volumes are the flat end mill's exact
//! sweeps (the shank moved up by `r`). The rounded part is swept by sampling the tip along the move,
//! see [`RoundSweep`].

use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;

use crate::sweep::{AXIS_EPS, ArcFixedAxis, EPS_MM, FlatTool, LinearFixedAxis, Sweep, chord_open, perp, push_clipped};

/// The most tip samples one piece of a move takes. A longer piece is sampled coarser; see
/// [`RoundSweep`].
pub const MAX_TIP_SAMPLES: usize = 1 << 14;

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
    /// along the path. Every piece's sampled union lies within the true sweep, and every point of the
    /// true sweep lies within `pitch / 2` of it (the body moves as a translate, so a tip between two
    /// samples is at most `pitch / 2` from one of them).
    pub fn sweep(&self, pitch: f64) -> Result<RoundSweep, crate::SimError> {
        if !(pitch.is_finite() && pitch > 0.0) {
            return Err(crate::SimError::BadInput("the sample pitch must be positive and finite".into()));
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
                let n = samples((*to - *from).len(), pitch);
                (0..=n).map(|k| *from + (*to - *from) * (k as f64 / n as f64)).collect()
            }
            RoundPath::Arc(arc) => {
                if let Some(c) = core {
                    parts.push(Box::new(arc.moved(Vec3::ZERO, c)));
                }
                if let Some(s) = shank {
                    parts.push(Box::new(arc.moved(shift, s)));
                }
                let n = samples(arc.length(), pitch);
                (0..=n).map(|k| arc.point(k as f64 / n as f64)).collect()
            }
        };
        Ok(RoundSweep { parts, tips, tool: t })
    }
}

/// The samples a piece of length `len` takes, `pitch` apart, at least one step and at most
/// [`MAX_TIP_SAMPLES`].
fn samples(len: f64, pitch: f64) -> usize {
    let n = (len / pitch).ceil();
    if n.is_finite() { (n.max(1.0) as usize).min(MAX_TIP_SAMPLES) } else { MAX_TIP_SAMPLES }
}

/// The swept volume of one piece of a round tool's move, for the stock's rays.
///
/// shortcut: the rounded part is the union of its stationary positions at the sampled tip positions,
/// so the removed set is at most `pitch / 2` short of the true sweep, normal to the surface. The stock
/// passes the pitch of its finest ray spacing, so the error stays under one ray spacing. A piece longer
/// than `MAX_TIP_SAMPLES` pitches is sampled coarser, so its error grows with its length. Upgrade:
/// exact ray-torus windows (a quartic per tip) or subdivision of long arcs.
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
        let top = Aabb3 { min: base.min + Vec3::Z * self.tool.length, max: base.max + Vec3::Z * self.tool.length };
        let rounded = base.union(&top).inflated(self.tool.radius);
        self.parts.iter().fold(rounded, |b, p| b.union(&p.bounds()))
    }

    fn ray(&self, o: Vec3, d: Vec3, t_range: (f64, f64), out: &mut Vec<(f64, f64)>) {
        for part in &self.parts {
            part.ray(o, d, t_range, out);
        }
        for tip in &self.tips {
            rounded_fiber(o, d, *tip, &self.tool, t_range, out);
        }
    }
}

/// The parts of the ray `o + t d` inside the rounded part of a tool whose tip is at `p`: the points
/// within `r` of the corner circle (radius `a` at height `r`), at heights from 0 to `min(r, L)`. A
/// ray along the axis meets it in one interval; a ray across the axis, at one height, in the annulus
/// of that height, which is one or two intervals.
fn rounded_fiber(o: Vec3, d: Vec3, p: Vec3, tool: &RoundTool, t_range: (f64, f64), out: &mut Vec<(f64, f64)>) {
    if !(d.len() > 0.0) {
        return;
    }
    let w = o - p;
    let h0 = w.z;
    let beta = d.z;
    let a = tool.core_radius();
    let rc = tool.corner;
    let top = rc.min(tool.length);
    let lateral = perp(d, Vec3::Z).len();
    if lateral < AXIS_EPS * d.len().max(1.0) {
        // Along the axis: the lateral distance is fixed, so the heights are one window.
        let rho = perp(w, Vec3::Z).len();
        let q = rc * rc - (rho - a) * (rho - a);
        if !(q > 0.0) {
            return;
        }
        let h_lo = rc - q.sqrt();
        if !(h_lo < top) {
            return;
        }
        let (t1, t2) = ((h_lo - h0) / beta, (top - h0) / beta);
        push_clipped(Some((t1.min(t2), t1.max(t2))), t_range, out);
    } else if beta.abs() < AXIS_EPS * d.len().max(1.0) {
        // Across the axis: the ray sits at height h0, where the rounded part is the annulus between
        // radii a - m and a + m, m = sqrt(r^2 - (h0 - r)^2).
        // At height r the corner meets the shank, whose open window leaves that plane out: when the flute
        // continues above r, the plane is interior to the body and is cut here (its top is closed).
        let upper = if tool.length > rc { rc + EPS_MM } else { top - EPS_MM };
        if !(h0 > EPS_MM && h0 < upper) {
            return;
        }
        let m = (rc * rc - (h0 - rc) * (h0 - rc)).max(0.0).sqrt();
        let wp = perp(w, Vec3::Z);
        let Some(outer) = chord_open(wp, d, a + m) else { return };
        match (a - m > 0.0).then(|| chord_open(wp, d, a - m)).flatten() {
            Some((l2, h2)) => {
                push_clipped(Some((outer.0, l2)), t_range, out);
                push_clipped(Some((h2, outer.1)), t_range, out);
            }
            None => push_clipped(Some(outer), t_range, out),
        }
    }
}

#[cfg(test)]
#[path = "round_tests.rs"]
mod tests;
