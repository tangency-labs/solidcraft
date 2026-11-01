//! Retained round-tube end cuts and their cylindrical developments. No display mesh is used.

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb3, Vec2, Vec3};

use crate::{DocError, ModelBody, Result};

pub const MAX_CUTTERS: usize = 16;
pub const MAX_TEMPLATE_SEGMENTS: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Start,
    End,
}

impl End {
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

/// Local datum at an original path endpoint; positive station goes into the member.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Terminal {
    pub origin: Vec3,
    pub axis: Vec3,
    pub x: Vec3,
    pub y: Vec3,
    pub length: f64,
    /// Conservative bounds of all the other swept path pieces, including their section.
    pub other: Vec<Aabb3>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cylinder {
    pub origin: Vec3,
    pub axis: Vec3,
    pub radius: f64,
    pub length: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cut {
    pub end: End,
    pub cylinders: Vec<Cylinder>,
    pub depth_bound: f64,
}

/// Evaluated physical-member data. Only member creation and retained coping produce it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoundMember {
    pub radius: f64,
    pub wall: f64,
    pub start: Option<Terminal>,
    pub end: Option<Terminal>,
    pub straight: bool,
    pub cuts: Vec<Cut>,
}

#[derive(Clone, Debug)]
pub(crate) struct CertifiedMember {
    pub shape: solidcraft_kernel::Body,
    pub data: RoundMember,
}

fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

impl RoundMember {
    pub fn terminal(&self, end: End) -> Result<&Terminal> {
        match end {
            End::Start => &self.start,
            End::End => &self.end,
        }
        .as_ref()
        .ok_or_else(|| invalid("not supported yet: this end has no straight cylindrical terminal"))
    }

    /// Validate cache data before any allocation driven by its counts or numeric values.
    pub(crate) fn validate(&self) -> Result<()> {
        if !self.radius.is_finite()
            || !self.wall.is_finite()
            || !(0.005..=5000.0).contains(&self.radius)
            || self.wall <= 0.0
            || self.wall >= self.radius
            || self.cuts.len() > 2
        {
            return Err(invalid("invalid round-member record"));
        }
        for t in [&self.start, &self.end].into_iter().flatten() {
            let finite = [t.origin, t.axis, t.x, t.y].iter().all(|p| p.is_finite() && p.len() <= 2e9);
            if !finite
                || !t.length.is_finite()
                || t.length <= 1e-6
                || t.length > 2e9
                || (t.axis.len() - 1.0).abs() > 1e-8
                || (t.x.len() - 1.0).abs() > 1e-8
                || (t.y.len() - 1.0).abs() > 1e-8
                || t.x.cross(t.y).dist(t.axis) > 1e-8
                || t.x.dot(t.y).abs() > 1e-8
                || t.other.len() > crate::frame::MAX_MEMBERS
                || t.other.iter().any(|b| b.is_empty() || !b.min.is_finite() || !b.max.is_finite())
            {
                return Err(invalid("invalid cylindrical terminal record"));
            }
        }
        for cut in &self.cuts {
            if cut.cylinders.is_empty() || cut.cylinders.len() > MAX_CUTTERS || !cut.depth_bound.is_finite() || cut.depth_bound <= 0.0 {
                return Err(invalid("invalid retained cope record"));
            }
            for c in &cut.cylinders {
                c.validate()?;
            }
        }
        Ok(())
    }

    /// Prepare a cut only when every cutter covers the original end disk. The entire removed
    /// interval then connects to that end, and the exit envelope selects the surviving side.
    /// A smaller/off-axis mate can leave holes or several branches; refuse it explicitly.
    pub fn prepare(&self, end: End, mut cylinders: Vec<Cylinder>) -> Result<Cut> {
        self.validate()?;
        if cylinders.is_empty() || cylinders.len() > MAX_CUTTERS || self.cuts.iter().any(|c| c.end == end) {
            return Err(invalid("a cope needs 1..16 cutters and an end without an existing cope (edit its feature instead)"));
        }
        let t = self.terminal(end)?;
        let mut depth = 0.0_f64;
        for c in &mut cylinders {
            c.validate()?;
            let q = t.origin - c.origin;
            let along = q.dot(c.axis);
            let off = (q - c.axis * along).len();
            let cosine = t.axis.dot(c.axis);
            let sine = (1.0 - cosine * cosine).max(0.0).sqrt();
            if off > 1e-7 || c.radius < self.radius || sine < 1e-3 {
                return Err(invalid(
                    "not supported yet: cope axes must meet at the selected endpoint, with mate OD at least member OD and joint sine >= 0.001",
                ));
            }
            // Define the retained cutter about the exact joint datum. This removes only
            // sub-micrometre sketch roundoff; both the solid cut and template use this axis.
            c.origin = t.origin - c.axis * along;
            // Covers every radial point of the stock, not just the sampled outer rim.
            let bound = (c.radius + cosine.abs() * self.radius) / sine;
            if bound >= t.length - 1e-5 {
                return Err(invalid("the cope reaches the next bend or the other end"));
            }
            let lo = along + (cosine * bound).min(0.0) - self.radius * sine;
            let hi = along + (cosine * bound).max(0.0) + self.radius * sine;
            if lo <= 1e-5 || hi >= c.length - 1e-5 {
                return Err(invalid("not supported yet: a mating tube end cap may intersect the cope; use a longer straight mating member"));
            }
            let end = c.origin + c.axis * c.length;
            let tool_box = Aabb3 { min: c.origin.min(end), max: c.origin.max(end) }.inflated(c.radius);
            if t.other.iter().any(|b| b.meets(&tool_box)) {
                return Err(invalid("not supported yet: the cutter may also meet a bend or another portion of this member"));
            }
            depth = depth.max(bound);
        }
        if self.straight && self.cuts.iter().any(|c| c.depth_bound + depth >= t.length - 1e-5) {
            return Err(invalid("the two end treatments may cross"));
        }
        Ok(Cut { end, cylinders, depth_bound: depth })
    }

    /// Uniform sampling with a conservative Lipschitz error bound, including cutter switches
    /// and equal-radius fishmouth cusps. x is arc length on the OD; y is positive into stock.
    pub fn template(&self, end: End, seam: f64, tolerance: f64) -> Result<Vec<Vec2>> {
        self.validate()?;
        if !seam.is_finite() || seam.abs() > 1e6 || !tolerance.is_finite() || !(0.001..=1.0).contains(&tolerance) {
            return Err(invalid("seam must be a finite angle within +/-1e6 rad; tolerance must be 0.001..1 mm"));
        }
        let t = self.terminal(end)?;
        let cut = self.cuts.iter().find(|c| c.end == end).ok_or_else(|| invalid("this member end has no retained cope"))?;
        let mut lipschitz = 0.0_f64;
        for c in &cut.cylinders {
            let cosine = t.axis.dot(c.axis).clamp(-1.0, 1.0);
            let sine = (1.0 - cosine * cosine).sqrt();
            if sine < 1e-3 || c.radius < self.radius {
                return Err(invalid("invalid retained cope geometry"));
            }
            // t(theta) = [cos(a)*w + sqrt(w^2 + sin(a)^2*(R^2-r^2))]/sin(a)^2,
            // w = radial(theta).mate_axis. Thus |t'| <= r*(1+|cos(a)|)/sin(a).
            lipschitz = lipschitz.max(self.radius * (1.0 + cosine.abs()) / sine);
        }
        // Axial interpolation error <= L*h/2. Radial sag <= r*h^2/8. Give each
        // half the tolerance, so the wrapped 3D chord has error strictly below tolerance.
        let step = (tolerance / lipschitz).min((4.0 * tolerance / self.radius).sqrt());
        let count = (std::f64::consts::TAU / step).ceil().max(16.0);
        if !count.is_finite() || count > MAX_TEMPLATE_SEGMENTS as f64 {
            return Err(invalid(format!("the sampling bound needs more than {MAX_TEMPLATE_SEGMENTS} segments; increase tolerance")));
        }
        let n = count as usize;
        let mut points = Vec::with_capacity(n + 1);
        let watcher = solidcraft_kernel::watch::current();
        for i in 0..=n {
            if i % 128 == 0 && watcher.as_ref().is_some_and(|w| w.cancelled()) {
                return Err(invalid("cope template cancelled"));
            }
            let theta = seam + std::f64::consts::TAU * (if i == n { 0.0 } else { i as f64 / n as f64 });
            let radial = (t.x * theta.cos() + t.y * theta.sin()) * self.radius;
            let mut station = 0.0_f64;
            for c in &cut.cylinders {
                let cosine = t.axis.dot(c.axis);
                let a = 1.0 - cosine * cosine;
                let w = radial.dot(c.axis);
                let d = w * w + a * (c.radius * c.radius - self.radius * self.radius);
                station = station.max((cosine * w + d.max(0.0).sqrt()) / a);
            }
            if !station.is_finite() || station < 0.0 || station > cut.depth_bound + 1e-5 {
                return Err(invalid("the cope station is outside its retained terminal"));
            }
            points.push(Vec2::new(self.radius * std::f64::consts::TAU * i as f64 / n as f64, station));
        }
        Ok(points)
    }
}

impl Cylinder {
    fn validate(&self) -> Result<()> {
        if !self.origin.is_finite()
            || self.origin.len() > 2e9
            || !self.axis.is_finite()
            || (self.axis.len() - 1.0).abs() > 1e-8
            || !self.radius.is_finite()
            || !(0.005..=5000.0).contains(&self.radius)
            || !self.length.is_finite()
            || !(1e-6..=2e9).contains(&self.length)
        {
            return Err(invalid("invalid cope cutter cylinder"));
        }
        Ok(())
    }
}

impl ModelBody {
    /// A physical round tube whose geometry has not been changed outside the retained member
    /// operations. Equality of bounds or face counts is deliberately insufficient.
    pub fn round_member(&self) -> Result<&RoundMember> {
        let record = self.round_member.as_ref().filter(|r| !self.transferred && r.shape.same_shape(&self.body));
        let data = &record.ok_or_else(|| invalid("not supported yet: select an unmodified round-tube frame member or a retained cope result"))?.data;
        data.validate()?;
        Ok(data)
    }

    pub(crate) fn certify_round_member(&mut self, data: RoundMember) {
        self.round_member = Some(std::sync::Arc::new(CertifiedMember { shape: self.body.clone(), data }));
    }
}
