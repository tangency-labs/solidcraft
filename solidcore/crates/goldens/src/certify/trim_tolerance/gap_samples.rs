//! The independent `heal.gap` changes from #20376. A fixed parameter contributes one
//! grid position. Varying spline spans share their endpoints and keep their midpoints.

use super::GoldenChange;

/// Whole rows from main before the sampling fix (eea94c41b).
pub(super) const GAP_MAIN: &str = include_str!("gap_main.jsonl");

pub(super) const GAP_CHANGES: &[GoldenChange] = &[
    GoldenChange {
        case: "crv_pt_bspline_degenerate_interval",
        old_hash: 0xfa49498fa19a4b98,
        new_hash: 0x176dc704ad70cef0,
        reason: "At t=0.5 the quadratic gives (1.5,0.5,0), one sample 1.5 from (1.5,2,0).",
    },
    GoldenChange {
        case: "gen0007",
        old_hash: 0x4f2328f3a0668197,
        new_hash: 0xecb9aad04f31857a,
        reason: "The curve on the surface has a point interval. Its eight slots represent the same parameter.",
    },
    GoldenChange {
        case: "gen0027",
        old_hash: 0xb420aa654943f02c,
        new_hash: 0x2eb8f362eea5a385,
        reason: "Fixed V and two U spans give five U positions. Mode 1 formerly reported an empty grid.",
    },
    GoldenChange {
        case: "gen0061",
        old_hash: 0xc5b30a00c6a4b26f,
        new_hash: 0x05a38ef8bbf31772,
        reason: "The bounded surface has fixed U. Its 4 by 3 grid reduces to one U by three V positions.",
    },
    GoldenChange {
        case: "gen0069",
        old_hash: 0x8e504ff3b00d7f3d,
        new_hash: 0xa931946b8f8663d5,
        reason: "The circle interval has one parameter. Mode 1 formerly reported an empty grid.",
    },
    GoldenChange {
        case: "gen0082",
        old_hash: 0x913d03b6ad0aff3f,
        new_hash: 0x501406652aaaf790,
        reason: "The curve on the surface has a point interval. Its three slots represent the same parameter.",
    },
    GoldenChange {
        case: "gen0086",
        old_hash: 0x69aebae1a8480013,
        new_hash: 0x6c5ca471bf3cfa0f,
        reason: "Fixed U gives one position. The two ends and midpoint of the V span give three.",
    },
    GoldenChange {
        case: "gen0103",
        old_hash: 0xdfc91c3c34992f82,
        new_hash: 0x146b49631f707edf,
        reason: "The curve on the surface has a point interval. Its eight slots represent the same parameter.",
    },
    GoldenChange {
        case: "gen0112",
        old_hash: 0xc666199ec0b3df78,
        new_hash: 0x136f1a5971701e19,
        reason: "Fixed U and two V spans give five V positions. Mode 1 formerly reported an empty grid.",
    },
    GoldenChange {
        case: "gen0195",
        old_hash: 0x8c03cc2b2f0b7dd1,
        new_hash: 0xa29aac27f6ce4daf,
        reason: "V is fixed on the plane. Its U span takes 20 equal subdivisions and both ends give 21 positions.",
    },
    GoldenChange {
        case: "gen0197",
        old_hash: 0xc34eb3db4aaa700f,
        new_hash: 0x63d7be2280eb1fb1,
        reason: "V is fixed on the plane. Its U span takes 20 equal subdivisions and both ends give 21 positions.",
    },
    GoldenChange {
        case: "gen0202",
        old_hash: 0x0fc52671ba767bed,
        new_hash: 0x9d618cb99f1f18c1,
        reason: "Fixed U gives one position. The two ends and midpoint of the V span give three.",
    },
    GoldenChange {
        case: "gen0241",
        old_hash: 0x8010bc1395ca5991,
        new_hash: 0xe22b7f13aad81161,
        reason: "Fixed U gives one position. The two ends and midpoint of the V span give three.",
    },
    GoldenChange {
        case: "gen0242",
        old_hash: 0xe8f8653ed65f9d5f,
        new_hash: 0x56a11aba3ccf0017,
        reason: "The bounded surface has fixed V. Its 20 by 4 grid reduces to 20 U positions by one V.",
    },
    GoldenChange {
        case: "gen0269",
        old_hash: 0x22eb7721be76acd8,
        new_hash: 0xaa9b78dc1fb8f116,
        reason: "V is fixed on the plane. Four subdivisions give five U positions. Mode 1 formerly reported an empty grid.",
    },
    GoldenChange {
        case: "gen0274",
        old_hash: 0xe2011c84c84e2713,
        new_hash: 0xdcf4141194234fa4,
        reason: "U is fixed. The extended V interval spans four knot intervals, giving nine endpoints and midpoints in all.",
    },
    GoldenChange {
        case: "gen0288",
        old_hash: 0xebf3ad69bb061457,
        new_hash: 0xa75fecf35540d1fa,
        reason: "The line interval has one parameter, so its gap uses one sample.",
    },
    GoldenChange {
        case: "gen0307",
        old_hash: 0x8647321b94198ad0,
        new_hash: 0xc2d13add5450ac76,
        reason: "The bounded surface has fixed U. Its 7 by 5 grid reduces to one U by five V positions.",
    },
    GoldenChange {
        case: "gen0321",
        old_hash: 0xe9813aca299df6a8,
        new_hash: 0xddc01ce24204ea87,
        reason: "The curve on the surface has a point interval. Its three slots represent the same parameter.",
    },
    GoldenChange {
        case: "gen0334",
        old_hash: 0x35d2d6fe2873cdf2,
        new_hash: 0x5ae47e08b2b92d66,
        reason: "The curve on the surface has a point interval. Its 20 slots represent the same parameter.",
    },
    GoldenChange {
        case: "gen0370",
        old_hash: 0xb1304cd31e707985,
        new_hash: 0x284c06b0ea7f674c,
        reason: "Fixed U and two V spans give five V positions. Mode 1 formerly reported an empty grid.",
    },
    GoldenChange {
        case: "gen0381",
        old_hash: 0x13c0f99fc871d263,
        new_hash: 0x99f36c26d17daff7,
        reason: "The ellipse interval has one parameter, so its gap uses one sample.",
    },
    GoldenChange {
        case: "gen0384",
        old_hash: 0xc9f638280c73a4db,
        new_hash: 0x85748d9c4f161969,
        reason: "U is fixed. The three V spans share their endpoints, giving seven endpoints and midpoints in all.",
    },
];
