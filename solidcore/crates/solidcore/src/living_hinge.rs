//! Living hinges: plans the staggered slots that let a flat plate bend, and returns each slot's
//! centre line with the counts and the cut length. This is the pure planning half of the living
//! hinge feature: it knows the slot pattern and where the plate's material is along each row, not
//! a body, a command or a cut.
//!
//! # The hinge frame
//! `u` runs along the slots (the bend axis), `v` across the rows. A slot is a stadium: every point
//! within `slot_width / 2` of its centre-line segment on its row, so its ends are round and every
//! clearance below is a distance from a centre-line point.
//!
//! # The lattice
//! With `L` the `slot_length` (tip to tip), `b` the `bridge`, `s` the `row_spacing` and
//! `P = L + b`, rows lie `s` apart and slot `j` of row `k` has its centre at
//! `u_c + P * (j + t_k / 2)`, `t_k` the row's offset from the middle ([`lattice`]). Each row is
//! therefore shifted half a pitch from its neighbours, and positions are always worked out from
//! the integers `(k, j)`, never by stepping, so two plans of one input agree bit for bit.
//!
//! # The edges
//! Along a row the material is a list of spans whose ends are either closed (solid plate carries
//! on past an edge of the area) or open (the plate's own edge, air past it). A slot keeps
//! `slot_width / 2 + margin` from a closed end; at an open end, of two neighbouring rows the one
//! whose slot centre is nearer the edge runs out through it and the other keeps at least
//! `min(bridge / 2, row_spacing - slot_width)` (or `margin`, where larger) of material, so no strip
//! hangs from the plate by one end. A piece shorter than the bridge inside the material is left
//! out ([`rows`]).
//!
//! Not upstream: SolidCraft's living hinge planner, a kernel primitive beside the cell grid
//! planner. No recursion. Inputs are hostile: sizes are checked, and a pattern that would reach
//! more than [`MAX_SLOTS`] lattice slots is refused before anything is allocated.

pub(crate) mod lattice;
pub(crate) mod near;
pub(crate) mod plan;
pub(crate) mod rows;
pub(crate) mod spans;

#[cfg(test)]
mod tests;

pub use crate::cell_grid::{MAX_CELLS as MAX_SLOTS, MAX_COORD, MIN_SIZE};
pub use near::{clean_crossing, component, near_set, near_union, outside, NearEdge};
pub use plan::{frame_box, near_loop, plan};
pub use rows::{plan_rows, plan_rows_zoned, RowSpan};
pub use spans::{chain_ids, clean_ends, row_crossings, row_spans, row_zone};

/// A field edge this near the plate's edge lies on it (mm): the gap a loop may have and still
/// close.
pub const ON_EDGE: f64 = 1e-4;
/// Two edges of a loop meeting at a turn up to this (radians) are one chain of the plate's edge.
pub const G1_TOL: f64 = 1e-6;
/// The shallowest crossing (radians, 60 degrees) a slot runs out through: the wedge of material
/// beside it is then no sharper than an equilateral corner.
pub const MIN_CROSSING: f64 = std::f64::consts::FRAC_PI_3;
/// How closely a conic or a Bézier edge is followed by its chords (mm, design 12): a chord stands
/// for the curve with its reach widened by this.
pub const CHORD: f64 = 1e-3;
/// How far inside its reach a slot may come and still be kept (mm): a rounding, not room.
pub const GRAZE: f64 = 1e-9;

/// What happens to slots the edges cross.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HingeClip {
    /// Of two neighbouring rows, the one whose slot centre is nearer the plate's edge runs out
    /// through it; the rest are shortened.
    #[default]
    RunOut,
    /// No slot runs out: a solid border along the plate's edge.
    Shorten,
    /// Only whole slots are cut.
    Whole,
}

/// The pattern's parameters, in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LivingHingeSpec {
    /// A slot's length tip to tip, round ends included.
    pub slot_length: f64,
    /// Material between two slot tips in one row.
    pub bridge: f64,
    /// Centre line to centre line between rows.
    pub row_spacing: f64,
    /// The cut's width (about a laser's kerf).
    pub slot_width: f64,
    /// The least material between a slot's end and every edge of the area.
    pub margin: f64,
    /// Slots the edges cross.
    pub clip: HingeClip,
}

impl LivingHingeSpec {
    /// Checks the sizes: every one finite and within [`MAX_COORD`], the straight part of a slot,
    /// the bridge, the width and the strip between rows at least [`MIN_SIZE`].
    pub fn check(&self) -> Result<(), LivingHingeError> {
        let named = [
            ("slot_length", self.slot_length),
            ("bridge", self.bridge),
            ("row_spacing", self.row_spacing),
            ("slot_width", self.slot_width),
            ("margin", self.margin),
        ];
        for (name, value) in named {
            if !value.is_finite() || value.abs() > MAX_COORD {
                return Err(LivingHingeError::Parameter(format!(
                    "{name} must be a finite length of at most {MAX_COORD} mm"
                )));
            }
        }
        if self.slot_width < MIN_SIZE {
            return Err(LivingHingeError::Parameter(format!(
                "slot_width must be at least {MIN_SIZE} mm"
            )));
        }
        if self.bridge < MIN_SIZE {
            return Err(LivingHingeError::Parameter(format!(
                "bridge must be at least {MIN_SIZE} mm"
            )));
        }
        if self.slot_length - self.slot_width < MIN_SIZE {
            return Err(LivingHingeError::Parameter(format!(
                "slot_length must exceed slot_width by at least {MIN_SIZE} mm"
            )));
        }
        if self.row_spacing - self.slot_width < MIN_SIZE {
            return Err(LivingHingeError::Parameter(format!(
                "row_spacing must exceed slot_width by at least {MIN_SIZE} mm"
            )));
        }
        if self.margin < 0.0 {
            return Err(LivingHingeError::Parameter(
                "margin must not be negative".into(),
            ));
        }
        Ok(())
    }

    /// The end radius `slot_width / 2`.
    pub fn radius(&self) -> f64 {
        self.slot_width / 2.0
    }

    /// The centre line of a whole slot, `slot_length - slot_width`.
    pub fn centre_line(&self) -> f64 {
        self.slot_length - self.slot_width
    }

    /// The pitch along a row, `slot_length + bridge`.
    pub fn pitch(&self) -> f64 {
        self.slot_length + self.bridge
    }

    /// The least material at the plate's edge: `min(bridge / 2, row_spacing - slot_width)`, never
    /// below [`MIN_SIZE`].
    pub fn least_material(&self) -> f64 {
        (self.bridge / 2.0)
            .min(self.row_spacing - self.slot_width)
            .max(MIN_SIZE)
    }

    /// The reach of the plate's edge from a centre line: `r + max(least material, margin)`.
    pub fn edge_reach(&self) -> f64 {
        self.radius() + self.least_material().max(self.margin)
    }
}

/// Why a living hinge could not be planned.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum LivingHingeError {
    #[error("living hinge: {0}")]
    /// A parameter is refused.
    Parameter(String),
    #[error("living hinge: the area: {0}")]
    /// The area is refused.
    Field(String),
    #[error("living hinge: the plate: {0}")]
    /// The plate is refused.
    Plate(String),
    #[error("living hinge: the area reaches {slots} lattice slots, more than the {max} allowed: use longer slots or a smaller area")]
    /// The area reaches more lattice slots than allowed.
    TooManySlots {
        /// The slots it reaches.
        slots: u64,
        /// The most allowed.
        max: usize,
    },
}

/// How a slot came about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// A lattice slot cut as it is.
    Whole,
    /// A lattice slot shortened by an edge.
    Shortened,
    /// A lattice slot that runs out through the plate's edge.
    RunOut,
}

/// One planned slot, in the hinge frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HingeSlot {
    /// The row index.
    pub row: i64,
    /// The lattice index along the row.
    pub index: i64,
    /// The row's `v`.
    pub v: f64,
    /// The centre line's lower end along `u`.
    pub a0: f64,
    /// The centre line's upper end along `u`.
    pub a1: f64,
    /// How it came about.
    pub kind: SlotKind,
    /// For a run-out, where it crosses the plate's edge.
    pub crossing: Option<f64>,
}

impl HingeSlot {
    /// The centre line's length inside the material: up to the crossing for a run-out.
    pub fn cut_length(&self) -> f64 {
        match self.crossing {
            Some(x) if (x - self.a0).abs() < (x - self.a1).abs() => self.a1 - x,
            Some(x) => x - self.a0,
            None => self.a1 - self.a0,
        }
    }
}

/// The plan: the slots and the report.
#[derive(Clone, Debug, PartialEq)]
pub struct LivingHingePlan {
    /// Slots sorted by row, then along it.
    pub slots: Vec<HingeSlot>,
    /// Rows holding at least one slot.
    pub rows: usize,
    /// Lattice slots that met the material and were left out.
    pub left_out: usize,
}

impl LivingHingePlan {
    /// Slots of one kind.
    pub fn count(&self, kind: SlotKind) -> usize {
        self.slots.iter().filter(|s| s.kind == kind).count()
    }

    /// The cut length along the centre lines, inside the material (mm).
    pub fn cut_length(&self) -> f64 {
        self.slots.iter().map(HingeSlot::cut_length).sum()
    }

    /// The area the slots open in the plate (mm²): the straight parts, a whole disc for each
    /// closed slot's two caps and half a disc for a run-out's one cap inside the material.
    pub fn open_area(&self, slot_width: f64) -> f64 {
        let r = slot_width / 2.0;
        let caps = self.count(SlotKind::Whole) + self.count(SlotKind::Shortened);
        let half_caps = self.count(SlotKind::RunOut);
        self.cut_length() * slot_width
            + std::f64::consts::PI * r * r * (caps as f64 + half_caps as f64 / 2.0)
    }

    /// The length of the slots' outlines inside the material (mm).
    pub fn outline_length(&self, slot_width: f64) -> f64 {
        let r = slot_width / 2.0;
        let caps = self.count(SlotKind::Whole) + self.count(SlotKind::Shortened);
        let half_caps = self.count(SlotKind::RunOut);
        2.0 * self.cut_length() + std::f64::consts::PI * r * (2.0 * caps as f64 + half_caps as f64)
    }
}
