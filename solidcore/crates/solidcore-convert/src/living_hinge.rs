//! The living hinge's record data and the kernel's, both ways, field for field: the spec goes to
//! the kernel, the plan comes back (`solidcore::living_hinge`).

use solidcore::living_hinge as k;
use solidcore_record::living_hinge::{
    HingeClip, HingeSlot, LivingHingePlan, LivingHingeSpec, SlotKind,
};

/// A record clip as the kernel's.
#[must_use]
pub fn clip(c: HingeClip) -> k::HingeClip {
    match c {
        HingeClip::RunOut => k::HingeClip::RunOut,
        HingeClip::Shorten => k::HingeClip::Shorten,
        HingeClip::Whole => k::HingeClip::Whole,
    }
}

/// A kernel clip as the record's.
#[must_use]
pub fn record_clip(c: k::HingeClip) -> HingeClip {
    match c {
        k::HingeClip::RunOut => HingeClip::RunOut,
        k::HingeClip::Shorten => HingeClip::Shorten,
        k::HingeClip::Whole => HingeClip::Whole,
    }
}

/// A record slot kind as the kernel's.
#[must_use]
pub fn kind(s: SlotKind) -> k::SlotKind {
    match s {
        SlotKind::Whole => k::SlotKind::Whole,
        SlotKind::Shortened => k::SlotKind::Shortened,
        SlotKind::RunOut => k::SlotKind::RunOut,
    }
}

/// A kernel slot kind as the record's.
#[must_use]
pub fn record_kind(s: k::SlotKind) -> SlotKind {
    match s {
        k::SlotKind::Whole => SlotKind::Whole,
        k::SlotKind::Shortened => SlotKind::Shortened,
        k::SlotKind::RunOut => SlotKind::RunOut,
    }
}

/// A record spec as the kernel's.
#[must_use]
pub fn spec(s: &LivingHingeSpec) -> k::LivingHingeSpec {
    k::LivingHingeSpec {
        slot_length: s.slot_length,
        bridge: s.bridge,
        row_spacing: s.row_spacing,
        slot_width: s.slot_width,
        margin: s.margin,
        clip: clip(s.clip),
    }
}

/// A kernel spec as the record's.
#[must_use]
pub fn record_spec(s: &k::LivingHingeSpec) -> LivingHingeSpec {
    LivingHingeSpec {
        slot_length: s.slot_length,
        bridge: s.bridge,
        row_spacing: s.row_spacing,
        slot_width: s.slot_width,
        margin: s.margin,
        clip: record_clip(s.clip),
    }
}

/// A record slot as the kernel's.
#[must_use]
pub fn slot(s: &HingeSlot) -> k::HingeSlot {
    k::HingeSlot {
        row: s.row,
        index: s.index,
        v: s.v,
        a0: s.a0,
        a1: s.a1,
        kind: kind(s.kind),
        crossing: s.crossing,
    }
}

/// A kernel slot as the record's.
#[must_use]
pub fn record_slot(s: &k::HingeSlot) -> HingeSlot {
    HingeSlot {
        row: s.row,
        index: s.index,
        v: s.v,
        a0: s.a0,
        a1: s.a1,
        kind: record_kind(s.kind),
        crossing: s.crossing,
    }
}

/// A record plan as the kernel's.
#[must_use]
pub fn plan(p: &LivingHingePlan) -> k::LivingHingePlan {
    k::LivingHingePlan {
        slots: p.slots.iter().map(slot).collect(),
        rows: p.rows,
        left_out: p.left_out,
        band_width: p.band_width,
    }
}

/// A kernel plan as the record's, with its lengths and area for slots `slot_width` wide.
#[must_use]
pub fn record_plan(p: &k::LivingHingePlan, slot_width: f64) -> LivingHingePlan {
    LivingHingePlan {
        slots: p.slots.iter().map(record_slot).collect(),
        rows: p.rows,
        left_out: p.left_out,
        cut_length: p.cut_length(),
        outline_length: p.outline_length(slot_width),
        open_area: p.open_area(slot_width),
        band_width: p.band_width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_and_a_plan_go_there_and_back() {
        for c in [HingeClip::RunOut, HingeClip::Shorten, HingeClip::Whole] {
            let s = LivingHingeSpec {
                slot_length: 20.0,
                bridge: 3.0,
                row_spacing: 1.5,
                slot_width: 0.2,
                margin: 2.0,
                clip: c,
            };
            assert_eq!(record_spec(&spec(&s)), s);
        }
        let p = LivingHingePlan {
            slots: [SlotKind::Whole, SlotKind::Shortened, SlotKind::RunOut]
                .into_iter()
                .zip(0_i64..)
                .map(|(kind, index)| HingeSlot {
                    row: 3,
                    index,
                    v: 35.0,
                    a0: -1.4,
                    a1: 18.4,
                    kind,
                    crossing: (kind == SlotKind::RunOut).then_some(0.0),
                })
                .collect(),
            rows: 1,
            left_out: 2,
            band_width: 40.0,
            ..Default::default()
        };
        let back = record_plan(&plan(&p), 0.2);
        let r: f64 = 0.1;
        let len = 19.8 + 19.8 + 18.4;
        assert!((back.cut_length - len).abs() < 1e-9, "{back:?}");
        assert!((back.outline_length - (2.0 * len + 5.0 * std::f64::consts::PI * r)).abs() < 1e-9);
        assert!((back.open_area - (len * 0.2 + 2.5 * std::f64::consts::PI * r * r)).abs() < 1e-9);
        let p = LivingHingePlan {
            cut_length: back.cut_length,
            outline_length: back.outline_length,
            open_area: back.open_area,
            ..p
        };
        assert_eq!(back, p);
    }
}
