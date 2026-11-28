//! WR-459 (#17169): the `blend.fillet_executive_p3` rows the kept target faces move only
//! by UV trims the clean-up no longer makes.
//!
//! With the flag retired, the clean-up after filleting leaves every edge between two of
//! the target's faces alone (`boolean::merge::p1`, `CleanupRelations::kept_faces`). Main's
//! clean-up visited those edges as merge candidates and, deciding about them, built the UV
//! trims their uses lacked; it merged none of them in these rows. So the result bodies are
//! the same bodies: the same vertices, edges and faces with the same geometry, tolerances
//! and mass, and only the count of uses holding a stored UV trim (a lazily built curve,
//! `topology::edgeuse::p3::get_or_create_uv_trim_curve`, made again when a query needs it)
//! is lower on those edges. [`ROWS`] lists, per row, each edge whose count moved with main's
//! count and this run's; [`rows_move_only_by_the_trims_main_built`] runs each row, checks
//! those counts, puts main's back and finds main's committed line under the goldens
//! comparator ([`crate::golden::diff`] with the unit's floors, the check `goldens` gates
//! on). [`MAIN_LINES`] holds those lines, copied from main's golden file; each hashes to
//! its row's [`ROWS`] hash. Not to the bit: main's own run of some of these rows already
//! sits a few ulps off its committed line, in the mass properties and in one geometry block
//! (inside the same comparator, so its golden file keeps the older bits; seen on
//! `source-box-union-edge0`, where this branch with main's counts put back is main's run
//! to the bit), and last bits may differ between machines.
//! The four `sphere_diff_3cylinders` rows are not here: they keep a sphere patch boundary
//! main merged (`units::blend_fillet_executive_p3::end_to_end`, `round_definition_rows`).
//! Nor are the ten rows the offset surface's extension from its base moves (#21288, WR-458):
//! main's line before WR-459 cannot be rebuilt from them any more. Each is certified on its
//! own by `units::blend_fillet_executive_p3::wr458_certificates`, one test per row.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::case::{repo_root, CaseContext};
use crate::golden::{diff, encode};
use crate::output::Output;
use crate::rng::fnv1a64;
use crate::UnitDiff;

/// The unit.
const UNIT: &str = "blend.fillet_executive_p3";

/// Main's committed lines of the [`ROWS`] rows (`crates/goldens/expected` at 77f72628af,
/// before this change re-blessed them), one per line, sorted by case.
const MAIN_LINES: &str = include_str!("wr459_rows/main_lines.jsonl");

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

/// One row: case, the hash of main's committed line (its golden file at 77f72628af, the
/// line in [`MAIN_LINES`]; [`record`]), and per
/// moved edge `(edge, main's stored-trim count, this run's)`.
type Row = (&'static str, u64, &'static [(usize, u8, u8)]);

/// The rows the keep moves, besides the four sphere rows: every other row of the unit
/// is main's to the bit (the unit's golden file).
const ROWS: &[Row] = &[
    (
        "corner3x2-union-inset-side-step-edges1-18-r1",
        0x6f92_2edf_9e0f_e43b,
        &[
            (17, 2, 0),
            (19, 2, 1),
            (20, 2, 0),
            (21, 2, 0),
            (25, 2, 1),
            (26, 2, 1),
            (29, 2, 1),
        ],
    ),
    (
        "corner3x2-union-inset-side-step-edges12-17-r1",
        0x8297_ae5e_6e56_93bd,
        &[
            (17, 2, 0),
            (18, 2, 1),
            (19, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 0),
            (23, 2, 0),
            (29, 2, 1),
        ],
    ),
    (
        "corner3x2-union-inset-side-step-edges13-19-r1",
        0x7ec4_e346_d8e6_8189,
        &[
            (5, 2, 1),
            (11, 2, 1),
            (18, 2, 0),
            (19, 2, 1),
            (20, 2, 1),
            (23, 2, 1),
        ],
    ),
    (
        "corner3x2-union-inset-side-step-edges14-27-r1",
        0x5654_23c8_2981_d8b0,
        &[
            (5, 2, 1),
            (10, 2, 1),
            (11, 2, 1),
            (12, 2, 1),
            (16, 2, 1),
            (20, 2, 1),
            (21, 2, 0),
            (22, 2, 0),
            (25, 2, 1),
        ],
    ),
    (
        "corner3x2-union-inset-side-step-edges15-19-r1",
        0x5fe2_f945_57cc_4392,
        &[
            (17, 2, 1),
            (20, 2, 0),
            (21, 2, 0),
            (22, 2, 0),
            (23, 2, 1),
            (24, 2, 1),
            (25, 2, 1),
        ],
    ),
    (
        "corner3x2-union-inset-side-step-edges15-27-r1",
        0x0a41_7dbc_55cc_7433,
        &[
            (15, 2, 1),
            (17, 2, 1),
            (18, 2, 0),
            (19, 2, 0),
            (20, 2, 0),
            (23, 2, 1),
            (25, 2, 1),
        ],
    ),
    (
        "corner3x2-union-overhang-corner-edges6-12-r1",
        0xa8a9_81ce_86cd_c0d6,
        &[
            (10, 2, 1),
            (15, 2, 1),
            (16, 2, 0),
            (17, 2, 0),
            (18, 2, 0),
            (19, 2, 1),
            (20, 2, 0),
            (21, 2, 0),
            (22, 2, 1),
            (23, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "corner3x2-union-overhang-corner-edges6-13-r1",
        0x9335_50c3_f546_cf36,
        &[
            (11, 2, 1),
            (12, 2, 1),
            (16, 2, 0),
            (17, 2, 1),
            (18, 2, 0),
            (19, 2, 1),
            (20, 2, 0),
            (21, 2, 0),
            (22, 2, 1),
            (23, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "corner3x2-union-overhang-front-edges15-22-r1",
        0xe724_6223_8133_7bc8,
        &[(16, 2, 1), (17, 2, 0), (21, 2, 1), (23, 2, 1)],
    ),
    (
        "corner3x2-union-overhang-front-edges15-24-r1",
        0xfd74_c3ad_7ab2_636e,
        &[
            (17, 2, 0),
            (18, 2, 0),
            (19, 2, 0),
            (20, 2, 1),
            (22, 2, 1),
            (25, 2, 1),
        ],
    ),
    (
        "issue_2176_tabbed_plate_corner_arc_r0_5",
        0x8709_bb82_003d_7825,
        &[(0, 2, 1), (2, 2, 1), (8, 2, 1), (9, 2, 1)],
    ),
    (
        "issue_2176_tabbed_plate_disc_wall_r0_5",
        0xf1ee_c3aa_024d_b6a2,
        &[
            (1, 2, 1),
            (2, 2, 1),
            (4, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (16, 2, 1),
        ],
    ),
    (
        "source-box-union-edge0",
        0x36a0_fd57_d902_5ecb,
        &[(0, 2, 1), (8, 2, 1)],
    ),
    (
        "source-box-union-edge1",
        0xb477_67ea_a229_21c9,
        &[
            (1, 2, 1),
            (4, 2, 1),
            (5, 2, 1),
            (6, 2, 1),
            (10, 2, 1),
            (14, 2, 1),
            (16, 2, 1),
        ],
    ),
    (
        "source-box-union-edge10",
        0x156a_38c2_3e62_ecc0,
        &[(1, 2, 1), (3, 2, 1), (6, 2, 1), (9, 2, 1), (10, 2, 1)],
    ),
    (
        "source-box-union-edge14",
        0x2e9d_a59b_ea4c_4e4d,
        &[
            (2, 2, 1),
            (5, 2, 1),
            (6, 2, 1),
            (7, 2, 1),
            (11, 2, 1),
            (14, 2, 1),
            (16, 2, 1),
        ],
    ),
    (
        "source-box-union-edge2",
        0x5f04_6c3d_d3f2_cf72,
        &[
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (10, 2, 1),
            (11, 2, 0),
            (12, 2, 1),
        ],
    ),
    (
        "source-box-union-edge5",
        0x9ced_3349_fbff_ef63,
        &[
            (1, 2, 1),
            (2, 2, 1),
            (4, 2, 1),
            (6, 2, 1),
            (8, 2, 0),
            (9, 2, 0),
            (10, 2, 1),
            (11, 2, 0),
            (12, 2, 1),
        ],
    ),
    (
        "source-box-union-edge7",
        0x23f9_a3ac_3711_7d26,
        &[
            (0, 2, 1),
            (1, 2, 1),
            (3, 2, 1),
            (6, 2, 1),
            (8, 2, 0),
            (10, 2, 0),
            (11, 2, 0),
            (13, 2, 0),
            (14, 2, 1),
            (15, 2, 1),
        ],
    ),
    (
        "source-file-l_shaped_box-edge12",
        0x6c8d_b11f_b913_0b14,
        &[(4, 2, 1), (12, 2, 1), (16, 2, 1)],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge12-r0.5",
        0x9783_2b20_b953_a877,
        &[
            (14, 2, 1),
            (15, 2, 0),
            (16, 2, 1),
            (17, 2, 0),
            (18, 2, 1),
            (19, 2, 0),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (26, 2, 1),
            (28, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge12-r1.0",
        0x50fe_50c2_fe0c_99ea,
        &[
            (14, 2, 1),
            (15, 2, 0),
            (16, 2, 1),
            (17, 2, 0),
            (18, 2, 1),
            (19, 2, 0),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (26, 2, 1),
            (28, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge12-r2.0",
        0xf259_0048_447f_e80e,
        &[
            (14, 2, 1),
            (15, 2, 0),
            (16, 2, 1),
            (17, 2, 0),
            (18, 2, 1),
            (19, 2, 0),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (26, 2, 1),
            (28, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge13-r0.5",
        0x123b_e06a_cf5c_5d67,
        &[
            (13, 2, 1),
            (17, 2, 1),
            (18, 2, 1),
            (19, 2, 0),
            (20, 2, 1),
            (21, 2, 0),
            (22, 2, 1),
            (23, 2, 1),
            (24, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge13-r1.0",
        0x2f2f_44e4_4a0a_73c6,
        &[
            (13, 2, 1),
            (17, 2, 1),
            (18, 2, 1),
            (19, 2, 0),
            (20, 2, 1),
            (21, 2, 0),
            (22, 2, 1),
            (23, 2, 1),
            (24, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge13-r2.0",
        0x231e_479c_fddc_184a,
        &[
            (13, 2, 1),
            (16, 2, 0),
            (17, 2, 1),
            (18, 2, 0),
            (19, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (24, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge14-r0.5",
        0xff45_b971_7aa2_9c97,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (15, 2, 1),
            (16, 2, 0),
            (18, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (25, 2, 1),
            (26, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge14-r1.0",
        0x2579_9258_b9b9_1e43,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (15, 2, 1),
            (16, 2, 0),
            (18, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (25, 2, 1),
            (26, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge14-r2.0",
        0x2549_0c6e_167d_2757,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (15, 2, 1),
            (16, 2, 0),
            (18, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (25, 2, 1),
            (26, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge16-r0.5",
        0x9e09_ee09_c173_fbfe,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (13, 2, 1),
            (17, 2, 0),
            (18, 2, 0),
            (19, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (23, 2, 1),
            (26, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge16-r1.0",
        0xb170_52c6_e3aa_471e,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (13, 2, 1),
            (17, 2, 0),
            (18, 2, 0),
            (19, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (23, 2, 1),
            (26, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge16-r2.0",
        0x7fe3_2495_fe35_f4f1,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (13, 2, 1),
            (17, 2, 0),
            (18, 2, 0),
            (19, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (23, 2, 1),
            (26, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge17-r0.5",
        0x67cd_ee6a_7625_99d8,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (17, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (25, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge17-r1.0",
        0xc244_a6d1_f0fe_2b2a,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (17, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (25, 2, 1),
            (28, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge17-r2.0",
        0x78ec_5571_f4f6_33f6,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (17, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
            (25, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge19-r0.5",
        0x7354_59db_c851_e005,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (13, 2, 1),
            (19, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (28, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge19-r1.0",
        0x578f_566c_f428_c513,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (13, 2, 1),
            (19, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge19-r2.0",
        0x390f_a71c_85ba_5086,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (13, 2, 1),
            (19, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge9-r0.5",
        0x4d2f_fc43_ad0b_5b2b,
        &[
            (15, 2, 0),
            (16, 2, 0),
            (17, 2, 1),
            (19, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge9-r1.0",
        0x3018_061e_99dc_0da3,
        &[
            (15, 2, 0),
            (16, 2, 0),
            (17, 2, 1),
            (19, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
        ],
    ),
    (
        "stubs3-overhang-union-nx1-multi-neighbour-end-face-edge9-r2.0",
        0xed5f_55d0_7ab3_b115,
        &[
            (15, 2, 0),
            (16, 2, 0),
            (17, 2, 1),
            (19, 2, 1),
            (20, 2, 1),
            (21, 2, 1),
            (22, 2, 1),
        ],
    ),
    (
        "stubs3-union-inset-side-step-edges13-15-r1",
        0x0115_c889_84f0_7696,
        &[
            (5, 2, 1),
            (11, 2, 1),
            (15, 2, 1),
            (19, 2, 0),
            (20, 2, 0),
            (21, 2, 0),
            (22, 2, 0),
            (23, 2, 0),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-union-inset-side-step-edges16-17-r1",
        0x976f_eb11_b30c_c41b,
        &[
            (13, 2, 0),
            (20, 2, 0),
            (21, 2, 0),
            (22, 2, 0),
            (23, 2, 0),
            (24, 2, 0),
            (25, 2, 1),
        ],
    ),
    (
        "stubs3-union-inset-side-step-edges16-18-r1",
        0x4a9f_35f8_88ef_dfe3,
        &[
            (3, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (13, 2, 0),
            (15, 2, 1),
            (17, 2, 0),
            (18, 2, 0),
            (19, 2, 0),
            (24, 2, 0),
            (25, 2, 0),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-union-inset-side-step-edges16-22-r1",
        0xc7eb_df52_c8bc_a55a,
        &[
            (14, 2, 1),
            (19, 2, 1),
            (20, 2, 0),
            (21, 2, 0),
            (22, 2, 1),
            (23, 2, 0),
            (24, 2, 1),
        ],
    ),
    (
        "stubs3-union-inset-side-step-edges17-22-r1",
        0x1759_6e07_6aaf_92d3,
        &[(13, 2, 1), (21, 2, 1), (22, 2, 1), (23, 2, 1), (24, 2, 0)],
    ),
    (
        "stubs3-union-overhang-corner-edges12-13-r1",
        0x2c1d_dde1_4b5c_7a30,
        &[
            (11, 2, 1),
            (12, 2, 1),
            (13, 2, 1),
            (16, 2, 1),
            (17, 2, 0),
            (18, 2, 0),
            (19, 2, 0),
            (20, 2, 1),
            (21, 2, 0),
            (22, 2, 0),
            (23, 2, 1),
            (24, 2, 1),
            (26, 2, 1),
            (27, 2, 1),
        ],
    ),
    (
        "stubs3-union-overhang-corner-edges14-22-r1",
        0x0155_be22_fd13_ba6c,
        &[
            (2, 2, 1),
            (3, 2, 1),
            (6, 2, 1),
            (10, 2, 1),
            (12, 2, 1),
            (16, 2, 0),
            (17, 2, 0),
            (21, 2, 1),
            (22, 2, 1),
            (24, 2, 1),
            (28, 2, 1),
        ],
    ),
    (
        "stubs3-union-overhang-corner-edges15-22-r1",
        0x457c_e9ef_23c9_42e9,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (13, 2, 1),
            (16, 2, 0),
            (17, 2, 0),
            (21, 2, 1),
            (22, 2, 1),
            (24, 2, 1),
            (28, 2, 1),
        ],
    ),
    (
        "stubs3-union-overhang-front-edges14-22-r1",
        0xc4ac_73f5_afb5_5ed1,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (14, 2, 1),
            (15, 2, 0),
            (20, 2, 1),
            (23, 2, 1),
            (28, 2, 1),
        ],
    ),
    (
        "stubs3-union-overhang-front-edges15-16-r1",
        0x0205_e348_ab1b_24a4,
        &[
            (6, 2, 1),
            (7, 2, 1),
            (8, 2, 1),
            (9, 2, 1),
            (12, 2, 1),
            (13, 2, 1),
            (16, 2, 0),
            (17, 2, 0),
            (18, 2, 0),
            (19, 2, 1),
            (21, 2, 1),
            (24, 2, 1),
            (26, 2, 1),
        ],
    ),
    (
        "sweep5-letter_g-e1-r1.0",
        0xd072_7ce2_51cb_460c,
        &[(14, 2, 1), (24, 2, 1), (27, 2, 1)],
    ),
];

/// Each edge's index in `result_geometry` of its stored-trim count
/// (`util::primitive::append`: per vertex its point and tolerance; per edge its tolerance,
/// interval, whether it has a curve and then three samples, its use count and the count
/// of uses holding a trim).
fn trim_count_at(g: &[f64], vertices: usize, edges: usize) -> Vec<usize> {
    let mut i = 4 * vertices;
    let mut at = Vec::with_capacity(edges);
    for _ in 0..edges {
        let has_curve = g[i + 3] == 1.0;
        i += 4;
        if has_curve {
            for _ in 0..3 {
                i += if g[i] == 0.0 { 4 } else { 1 };
            }
        }
        at.push(i + 1);
        i += 2;
    }
    at
}

/// The case's output.
fn run(u: &dyn UnitDiff, name: &str) -> Output {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let case = u
        .cases(&ctx)
        .unwrap()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no case {name}"));
    u.run_rust(&case)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn unit() -> Box<dyn UnitDiff> {
    crate::registry()
        .into_iter()
        .find(|u| u.id() == UNIT)
        .unwrap_or_else(|| panic!("no unit {UNIT}"))
}

/// Each [`ROWS`] row: every listed edge holds this run's count, lower than main's, and
/// with main's counts put back the row agrees with main's committed line under the goldens
/// comparator: the topology, the integers and the error fields exactly, every vertex, edge
/// and face sample and tolerance and the mass properties within the comparator's rounding.
/// So nothing but those counts moved beyond rounding.
#[test]
fn rows_move_only_by_the_trims_main_built() {
    let mut names: Vec<_> = ROWS.iter().map(|r| r.0).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ROWS.len(), "a row listed twice");
    let u = unit();
    let floors = u.abs_floors();
    let main = main_lines();
    assert_eq!(main.len(), ROWS.len(), "a main line per row");
    let mut failures = Vec::new();
    for &(name, main_hash, edges) in ROWS {
        let (raw, line) = main
            .get(name)
            .unwrap_or_else(|| panic!("{name}: no main line"));
        assert_eq!(fnv1a64(raw), main_hash, "{name}: not main's committed line");
        let mut out = run(u.as_ref(), name);
        let counts = out.ints["api_status_vertex_edge_face_counts"].clone();
        let (nv, ne) = (counts[1] as usize, counts[2] as usize);
        let g = out.values.get_mut("result_geometry").unwrap();
        let at = trim_count_at(g, nv, ne);
        for &(e, main, now) in edges {
            assert!(now < main, "{name}: edge {e} listed as not lowered");
            if g[at[e]] != f64::from(now) {
                failures.push(format!(
                    "{name}: edge {e} holds {} trims, not {now}",
                    g[at[e]]
                ));
            }
            g[at[e]] = f64::from(main);
        }
        if let Some(c) = diff(line, &encode(name, &out), floors) {
            failures.push(format!("{name}: not main's line with main's trims: {c:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Main's committed line of each [`ROWS`] row, by case: the raw text and its value.
fn main_lines() -> BTreeMap<String, (String, Value)> {
    MAIN_LINES
        .lines()
        .map(|raw| {
            let line: Value = serde_json::from_str(raw).unwrap();
            let name = line["case"].as_str().unwrap().to_string();
            (name, (raw.to_string(), line))
        })
        .collect()
}

/// Prints the hash of each [`ROWS`] row's committed line: run on main's golden file.
#[test]
#[ignore = "makes ROWS' hashes from main's golden file"]
fn record() {
    let golden = crate::golden::read(&crate::golden::default_dir(), UNIT).unwrap();
    for &(name, _, _) in ROWS {
        let line = golden.get(name).unwrap_or_else(|| panic!("no line {name}"));
        println!(
            "{name} {:#x}",
            fnv1a64(&serde_json::to_string(line).unwrap())
        );
    }
}
