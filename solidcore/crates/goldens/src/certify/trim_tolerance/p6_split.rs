//! The independent golden changes from #21326 (WR-647 part B): an E-E-E or F-F-F range
//! input that ends strictly inside a curve interval no longer labels the rest of that
//! interval with the input when the rest is off the input's edge (`intersect::curve_class::p6`,
//! `part_lies_on`). The derivation of every row is in the re-bless commit (c6f47ada51).

use super::GoldenChange;

/// Whole `heal.face_props` rows from main before the split.
pub(super) const P6_HEAL_MAIN: &str = include_str!("p6_split_heal_main.jsonl");

/// Whole `intersect.curve_class_p5` rows from main before the split.
pub(super) const P6_CURVE_CLASS_MAIN: &str = include_str!("p6_split_curve_class_main.jsonl");

pub(super) const P6_HEAL_CHANGES: &[GoldenChange] = &[
    GoldenChange {
        case: "gen0616",
        old_hash: 0xc28ed0f2c43a2b57,
        new_hash: 0x75a5ff5a6ce20c12,
        reason: "Split on a Line edge; the parts beyond lie 5.6e4 to 1.6e5 tolerances from its carrier line. Stage 3 status and every op status unchanged; only snapshot labels move.",
    },
    GoldenChange {
        case: "gen1066",
        old_hash: 0x5025af1f295be3eb,
        new_hash: 0xcad9132e446ba8cf,
        reason: "Split on a Line edge; the part beyond ends 2.258 from its carrier line against a tolerance of 1.00001. Stage 3 status and every op status unchanged; only snapshot labels move.",
    },
];

pub(super) const P6_CURVE_CLASS_CHANGES: &[GoldenChange] = &[
    GoldenChange {
        case: "topo0019",
        old_hash: 0x76fa385b2fe6aa7d,
        new_hash: 0x5a5208ad225a7f8c,
        reason: "2 intervals to 4, split at input.min() and input.max(); the outer pieces keep Nothing. Line on a box edge: the outer ends lie 3.5e3 tolerances from its carrier line. Status and solution tail unchanged.",
    },
    GoldenChange {
        case: "topo0031",
        old_hash: 0x11c2bbdcb297e9f7,
        new_hash: 0x0720d084093fc876,
        reason: "2 intervals to 4, split at input.min() and input.max(); the outer pieces keep Nothing. Line on a box edge: the outer ends lie 5.04e3 tolerances from its carrier line. Status and solution tail unchanged.",
    },
    GoldenChange {
        case: "topo0036",
        old_hash: 0xc47e68d2239c7856,
        new_hash: 0x37ef786f1bffc923,
        reason: "2 intervals to 4, split at input.min() and input.max(); the outer pieces keep Nothing. Line on a box edge: the outer ends lie 4.31e4 tolerances from its carrier line. Status and solution tail unchanged.",
    },
    GoldenChange {
        case: "topo0038",
        old_hash: 0x798e9cf522be34a4,
        new_hash: 0xac196c53d6faaec1,
        reason: "2 intervals to 4, split at input.min() and input.max(); the outer pieces keep Nothing. Line on a box edge: the outer ends lie 1.86e4 tolerances from its carrier line. Status and solution tail unchanged.",
    },
    GoldenChange {
        case: "topo0045",
        old_hash: 0xb384c7eb1e600bcc,
        new_hash: 0xe7e36f1eecb946c9,
        reason: "2 intervals to 4, split at input.min() and input.max(); the outer pieces keep Nothing. Line on a box edge: the outer ends lie 3.1e3 tolerances from its carrier line. Status and solution tail unchanged.",
    },
    GoldenChange {
        case: "topo0058",
        old_hash: 0xb7ecbef4cd623c5e,
        new_hash: 0x6a577cbb62a6efcb,
        reason: "2 intervals to 4, split at input.min() and input.max(); the outer pieces keep Nothing. Line on a box edge: the outer ends lie 6.93e3 tolerances from its carrier line. Status and solution tail unchanged.",
    },
    GoldenChange {
        case: "topo0225",
        old_hash: 0x22dee9ebca8f09e4,
        new_hash: 0x20cce9a33393d701,
        reason: "2 intervals to 4, split at input.min() and input.max(); the outer pieces keep Nothing. Line on a box edge: the outer ends lie 5.03e3 tolerances from its carrier line. Status and solution tail unchanged.",
    },
    GoldenChange {
        case: "topo0228",
        old_hash: 0x2e6fc4d20eebb22b,
        new_hash: 0x9ac5506171788582,
        reason: "2 intervals to 4, split at input.min() and input.max(); the outer pieces keep Nothing. Line on a box edge: the outer ends lie 3.18e5 tolerances from its carrier line. Status and solution tail unchanged.",
    },
];
