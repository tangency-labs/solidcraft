//! A living hinge as data: the parameters of staggered slots over a sketch area on a plate, and the
//! plan the kernel returns (`solidcore::living_hinge`): the slots in the hinge frame, the rows and
//! the lattice slots left out. Plain data: the lattice, the edge rules and the report's sums are
//! the kernel's.
use serde::{Deserialize, Serialize};

/// What happens to slots the edges cross.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub margin: f64,
    /// Slots the edges cross.
    #[serde(default)]
    pub clip: HingeClip,
}

/// How a slot came about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    /// A lattice slot cut as it is.
    Whole,
    /// A lattice slot shortened by an edge.
    Shortened,
    /// A lattice slot that runs out through the plate's edge.
    RunOut,
}

/// One planned slot, in the hinge frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crossing: Option<f64>,
}

/// The plan: the slots, the rows holding one and the lattice slots left out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LivingHingePlan {
    /// Slots sorted by row, then along it.
    pub slots: Vec<HingeSlot>,
    /// Rows holding at least one slot.
    pub rows: usize,
    /// Lattice slots that met the material and were left out.
    pub left_out: usize,
    /// The centre lines' length inside the material (mm, design 8.1).
    #[serde(default)]
    pub cut_length: f64,
    /// The slots' outlines' length inside the material (mm).
    #[serde(default)]
    pub outline_length: f64,
    /// The slots' area inside the material (mm²).
    #[serde(default)]
    pub open_area: f64,
    /// `V`, the area's extent across the slots (mm): the arc length the hinge bends over.
    #[serde(default)]
    pub band_width: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_reads_back_as_it_was_written() {
        let plan = LivingHingePlan {
            slots: vec![
                HingeSlot {
                    row: 0,
                    index: 5,
                    v: 30.5,
                    a0: -1.4,
                    a1: 18.4,
                    kind: SlotKind::RunOut,
                    crossing: Some(0.0),
                },
                HingeSlot {
                    row: 0,
                    index: 6,
                    v: 30.5,
                    a0: 21.6,
                    a1: 41.4,
                    kind: SlotKind::Whole,
                    crossing: None,
                },
            ],
            rows: 1,
            left_out: 0,
            cut_length: 37.6,
            outline_length: 76.0,
            open_area: 7.6,
            band_width: 40.0,
        };
        let text = serde_json::to_string(&plan).unwrap();
        assert!(text.contains("\"run_out\"") && !text.contains("\"crossing\":null"));
        assert_eq!(
            serde_json::from_str::<LivingHingePlan>(&text).unwrap(),
            plan
        );
    }

    #[test]
    fn margin_and_clip_may_be_left_out() {
        let spec: LivingHingeSpec = serde_json::from_str(
            r#"{"slot_length": 20, "bridge": 3, "row_spacing": 1.5, "slot_width": 0.2}"#,
        )
        .unwrap();
        let want = LivingHingeSpec {
            slot_length: 20.0,
            bridge: 3.0,
            row_spacing: 1.5,
            slot_width: 0.2,
            margin: 0.0,
            clip: HingeClip::RunOut,
        };
        assert_eq!(spec, want);
    }
}
