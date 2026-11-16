//! The slot lattice (design 5): rows `row_spacing` apart, slots one pitch apart along a row, each
//! row shifted half a pitch from its neighbours, centred on the area's box in the hinge frame.

use super::{LivingHingeError, LivingHingeSpec, MAX_COORD, MAX_SLOTS};

/// The box of the area in the hinge frame: `[u_min, u_max] x [v_min, v_max]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameBox {
    /// Least `u`.
    pub u_min: f64,
    /// Greatest `u`.
    pub u_max: f64,
    /// Least `v`.
    pub v_min: f64,
    /// Greatest `v`.
    pub v_max: f64,
}

/// The lattice of one feature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lattice {
    u_c: f64,
    v_c: f64,
    /// `(n - 1) / 2` for a centred lattice (rows numbered from 0), 0 for an anchored one.
    shift: f64,
    pitch: f64,
    spacing: f64,
    half_line: f64,
    /// The rows tried: every `k` with `v_min < v_k < v_max`.
    pub rows: (i64, i64),
}

impl Lattice {
    /// The lattice centred on `b` (design 5.1, 5.2): `n = max(1, floor((V - w - 2m) / s) + 1)` sets
    /// the parity, row `k` lies at `v_c + s * (k - (n - 1) / 2)`.
    pub fn centred(spec: &LivingHingeSpec, b: &FrameBox) -> Result<Self, LivingHingeError> {
        check_box(b)?;
        let height = b.v_max - b.v_min;
        let fit = ((height - spec.slot_width - 2.0 * spec.margin) / spec.row_spacing).floor();
        let n = if fit >= 0.0 { fit + 1.0 } else { 1.0 };
        let centre = ((b.u_min + b.u_max) / 2.0, (b.v_min + b.v_max) / 2.0);
        Self::build(spec, b, centre, (n - 1.0) / 2.0)
    }

    /// The lattice anchored at `(u, v)` (design 5.2): a slot of row 0 is centred there.
    pub fn anchored(
        spec: &LivingHingeSpec,
        b: &FrameBox,
        anchor: (f64, f64),
    ) -> Result<Self, LivingHingeError> {
        check_box(b)?;
        if !(anchor.0.abs() <= MAX_COORD && anchor.1.abs() <= MAX_COORD) {
            return Err(LivingHingeError::Parameter(format!(
                "the anchor must lie within {MAX_COORD} mm of the origin"
            )));
        }
        Self::build(spec, b, anchor, 0.0)
    }

    fn build(
        spec: &LivingHingeSpec,
        b: &FrameBox,
        (u_c, v_c): (f64, f64),
        shift: f64,
    ) -> Result<Self, LivingHingeError> {
        spec.check()?;
        let s = spec.row_spacing;
        let pitch = spec.pitch();
        // Bound in floats before any integer is formed (design 5.3).
        let bound = ((b.v_max - b.v_min) / s + 2.0) * ((b.u_max - b.u_min) / pitch + 2.0);
        if !(bound <= MAX_SLOTS as f64) {
            return Err(LivingHingeError::TooManySlots {
                slots: if bound.is_finite() {
                    bound as u64
                } else {
                    u64::MAX
                },
                max: MAX_SLOTS,
            });
        }
        // Rows strictly inside (v_min, v_max).
        let lo = ((b.v_min - v_c) / s + shift).floor() as i64 + 1;
        let hi = ((b.v_max - v_c) / s + shift).ceil() as i64 - 1;
        Ok(Self {
            u_c,
            v_c,
            shift,
            pitch,
            spacing: s,
            half_line: spec.centre_line() / 2.0,
            rows: (lo, hi),
        })
    }

    /// Row `k`'s offset `t_k` from the middle.
    pub fn t(&self, k: i64) -> f64 {
        k as f64 - self.shift
    }

    /// Row `k`'s `v`.
    pub fn row_v(&self, k: i64) -> f64 {
        self.v_c + self.spacing * self.t(k)
    }

    /// The centre of slot `j` of row `k` along `u`.
    pub fn centre(&self, k: i64, j: i64) -> f64 {
        self.u_c + self.pitch * (j as f64 + self.t(k) / 2.0)
    }

    /// Slot `j` of row `k`'s centre line `[c - l/2, c + l/2]`.
    pub fn line(&self, k: i64, j: i64) -> (f64, f64) {
        let c = self.centre(k, j);
        (c - self.half_line, c + self.half_line)
    }

    /// Every `j` of row `k` whose centre line meets `[u_lo, u_hi]` (design 5.3).
    pub fn slots_meeting(&self, k: i64, u_lo: f64, u_hi: f64) -> (i64, i64) {
        let half = self.t(k) / 2.0;
        let first = ((u_lo - self.u_c - self.half_line) / self.pitch - half).ceil() as i64;
        let last = ((u_hi - self.u_c + self.half_line) / self.pitch - half).floor() as i64;
        (first, last)
    }
}

fn check_box(b: &FrameBox) -> Result<(), LivingHingeError> {
    let all = [b.u_min, b.u_max, b.v_min, b.v_max];
    if !all.iter().all(|x| x.abs() <= MAX_COORD) {
        return Err(LivingHingeError::Field(format!(
            "it must lie within {MAX_COORD} mm of the origin"
        )));
    }
    if !(b.u_min < b.u_max && b.v_min < b.v_max) {
        return Err(LivingHingeError::Field("it has no area".into()));
    }
    Ok(())
}
