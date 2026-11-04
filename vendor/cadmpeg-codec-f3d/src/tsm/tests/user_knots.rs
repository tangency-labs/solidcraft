// SPDX-License-Identifier: Apache-2.0
use super::{parse_cage, QUAD_TOPOLOGY};
use cadmpeg_ir::subd::SubdUserKnot;

fn source(topology: &str, run: &str) -> String {
    format!("#TS0200\n{topology}{run}0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n")
}

#[test]
fn inherited_user_interval_uses_the_base_and_zero_remains_explicit() {
    let topology = QUAD_TOPOLOGY
        .replace("e 0 1\n", "e 0 0.25\n")
        .replace("e 2 1\n", "e 2 0.25\n");
    let text = source(&topology, "106ek -1\n106ek 0\n106ek 1\n106ek -1\n");
    let parsed = parse_cage(text.as_bytes()).expect("user-knot controls");
    assert!(parsed.unknown_record_kinds.is_empty());
    let edges = parsed.surface.cage.edges();
    assert_eq!(edges[0].user_knot, Some(SubdUserKnot::Inherit));
    assert_eq!(edges[0].effective_knot_interval(), Some(0.25));
    assert_eq!(edges[1].effective_knot_interval(), Some(0.0));
    assert!(matches!(
        edges[1].user_knot,
        Some(SubdUserKnot::Override(_))
    ));
    assert!(matches!(
        edges[2].user_knot,
        Some(SubdUserKnot::Override(_))
    ));
    assert_eq!(edges[2].effective_knot_interval(), Some(1.0));
}

#[test]
fn invalid_user_knot_runs_stay_unprojected() {
    let valid = "106ek -1\n106ek 0\n106ek 1\n106ek -1\n";
    let mut cases = vec![
        source(QUAD_TOPOLOGY, "106ek -1\n"),
        source(QUAD_TOPOLOGY, &(valid.to_owned() + "106ek -1\n")),
        source(QUAD_TOPOLOGY, "106ek -0.5\n106ek 0\n106ek 1\n106ek -1\n"),
        source(
            QUAD_TOPOLOGY,
            "106ek -1\n100edges\n106ek 0\n106ek 1\n106ek -1\n",
        ),
        source(
            &(QUAD_TOPOLOGY.to_owned() + "e\n"),
            &(valid.to_owned() + "106ek 0\n"),
        ),
    ];
    let displaced = QUAD_TOPOLOGY.replace("e 6 1\n", &(valid.to_owned() + "e 6 1\n"));
    cases.push(source(&displaced, ""));
    for text in cases {
        let parsed = parse_cage(text.as_bytes()).expect("native user-knot residue");
        assert!(parsed.unknown_record_kinds.contains_key("106ek"));
        assert!(parsed
            .surface
            .cage
            .edges()
            .iter()
            .all(|edge| edge.user_knot.is_none()));
    }
    for value in ["NaN", "inf", "-inf"] {
        let text = source(
            QUAD_TOPOLOGY,
            &format!("106ek {value}\n106ek 0\n106ek 1\n106ek -1\n"),
        );
        assert!(parse_cage(text.as_bytes()).is_err());
    }
}
