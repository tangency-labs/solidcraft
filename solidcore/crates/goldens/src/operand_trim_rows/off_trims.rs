// The trims the operand-trim rows' bodies hold off their edge's tolerance on edges no
// moved value touches, each with main's (7d4445c0c) record block for its edge use
// (`topology.brep_local_operation`'s record; `-1.0`: main held no trim there). fc0000's,
// fc0008's and fc0025's (#19409) are gone: WR-450's put-back no longer runs them, and
// without it no trim of theirs is off its edge's tolerance (tangency-labs/solidcraft#21301).
const OFF_TRIMS: &[OffTrim] = &[
    OffTrim {
        case: "fc0006",
        prefix: "",
        edge: 11,
        edge_use: 2,
        main: &[-1.0],
        added: Some("#19409"),
    },
    OffTrim {
        case: "fc0006",
        prefix: "",
        edge: 10,
        edge_use: 2,
        main: &[-1.0],
        added: Some("#19409"),
    },
    OffTrim {
        case: "fc0006",
        prefix: "",
        edge: 22,
        edge_use: 2,
        main: &[-1.0],
        added: Some("#19409"),
    },
];
