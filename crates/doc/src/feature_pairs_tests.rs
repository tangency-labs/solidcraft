use super::*;

/// F2 and F3 pair with F20 and F30, and body Plate with Plate (copy); F5 has no pair.
fn pairs() -> FeaturePairs {
    FeaturePairs {
        features: [(2, 20), (3, 30)].into_iter().collect(),
        bodies: [("Plate".to_string(), "Plate (copy)".to_string())].into_iter().collect(),
    }
}

fn edge(name: &str, origin_feature: u64, body: &str, path: &[u64]) -> ModelEdge {
    ModelEdge { body: body.into(), origin_feature, name: name.into(), path: path.to_vec() }
}

/// An edge of the part placed through [7] moves to the part placed through [9]: each feature its
/// name names, its body and the feature that made the body through the pairs.
#[test]
fn an_edge_moves_through_the_pairs() {
    let moved = pairs().edge(&edge("F2:top|F3:side:l4~1", 2, "Plate", &[7]), &[7], &[9]);
    assert_eq!(moved, Some(edge("F20:top|F30:side:l4~1", 20, "Plate (copy)", &[9])));
    let face = ModelFace { body: "Plate".into(), origin_feature: 2, name: "F3:side:l12".into(), path: vec![7] };
    let moved = pairs().face(&face, &[7], &[9]);
    assert_eq!(moved, Some(ModelFace { body: "Plate (copy)".into(), origin_feature: 20, name: "F30:side:l12".into(), path: vec![9] }));
    let corner = ModelPoint {
        body: "Plate".into(),
        origin_feature: 2,
        edges: vec!["F2:top|F3:side:l1".into(), "F2:top|F3:side:l2".into()],
        centre: false,
        path: vec![7],
    };
    let moved = pairs().point(&corner, &[7], &[9]).unwrap();
    assert_eq!(
        (moved.edges, moved.origin_feature, moved.path),
        (vec!["F20:top|F30:side:l1".to_string(), "F20:top|F30:side:l2".to_string()], 20, vec![9])
    );
    // The design's own body moves onto a placed part, and back.
    assert_eq!(pairs().edge(&edge("F2:top|F3:side:l4", 2, "Plate", &[]), &[], &[9]).map(|e| e.path), Some(vec![9]));
}

/// Nothing moves that the pairs can't take: a name naming F5 anywhere in it, a body or a making
/// feature with no pair, or a reference placed through another path than the part moved (one
/// inside it, or another part).
#[test]
fn an_edge_with_no_pair_doesnt_move() {
    let p = pairs();
    for name in ["F5:blend(F2:top|F3:side:l4)", "F2:top|F5:end", "F2:top@F5.1", "F2:top#[F3:side:l1,F5:end]|F3:side:l2"] {
        assert_eq!(p.edge(&edge(name, 2, "Plate", &[7]), &[7], &[9]), None, "{name}");
    }
    assert_eq!(p.edge(&edge("F2:top|F3:side:l4", 5, "Plate", &[7]), &[7], &[9]), None, "made by F5");
    assert_eq!(p.edge(&edge("F2:top|F3:side:l4", 2, "Other", &[7]), &[7], &[9]), None, "another body");
    for path in [&[7, 1][..], &[8], &[]] {
        assert_eq!(p.edge(&edge("F2:top|F3:side:l4", 2, "Plate", path), &[7], &[9]), None, "{path:?}");
    }
}

/// One component on both sides: nothing but the path changes.
#[test]
fn the_same_component_moves_only_its_path() {
    let same =
        FeaturePairs { features: [(2, 2), (3, 3)].into_iter().collect(), bodies: [("Plate".to_string(), "Plate".to_string())].into_iter().collect() };
    let e = edge("F2:top|F3:side:l4", 2, "Plate", &[7]);
    assert_eq!(same.edge(&e, &[7], &[9]), Some(ModelEdge { path: vec![9], ..e }));
}

/// Seeded names of every shape the grammar writes: feature faces, blends round an edge, shells'
/// inner walls, pattern copies, pieces told apart by their neighbours or a number, body faces, and
/// sketch curve ids and indices that only look like ids (`l12`, `face:12`), in edges of one to
/// three faces with or without a tie. Each is built twice, once with the pairs' ids written in: a
/// name whose features all pair rewrites to exactly that one, and reads back; one naming F5
/// doesn't move.
#[test]
fn seeded_names_rewrite_exactly_their_feature_ids() {
    let pick = |seed: u64, salt: u64, n: u64| ((seed ^ salt).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % n;
    let p = pairs();
    // A face of `seed`'s shape, with each feature id through `id`, nested at most `depth` deep.
    fn face(pick: &dyn Fn(u64, u64, u64) -> u64, seed: u64, salt: u64, depth: u32, id: &dyn Fn(u64) -> u64, body: &dyn Fn(&str) -> String) -> String {
        let f = |s: u64| id([2, 3, 5][pick(seed, salt + s, 3) as usize]);
        let n = pick(seed, salt + 7, 20);
        let base = match pick(seed, salt, if depth == 0 { 3 } else { 5 }) {
            0 => format!("F{}:side:l{n}", f(1)),
            1 => format!("{}:face:{n}", body("Plate")),
            2 => format!("F{}:top", f(1)),
            3 => format!(
                "F{}:blend({}|{})",
                f(1),
                face(pick, seed, salt * 3 + 11, depth - 1, id, body),
                face(pick, seed, salt * 3 + 12, depth - 1, id, body)
            ),
            _ => format!("F{}:inner({})", f(1), face(pick, seed, salt * 3 + 13, depth - 1, id, body)),
        };
        match pick(seed, salt + 9, 4) {
            0 => format!("{base}@F{}.{n}", f(2)),
            1 => format!("{base}#[F{}:top,F{}:side:l{n}]", f(3), f(4)),
            2 => format!("{base}#{n}"),
            _ => base,
        }
    }
    let mut moved = 0;
    for seed in 1..=600u64 {
        let pick = |a: u64, b: u64, c: u64| pick(a, b, c);
        let named = |id: &dyn Fn(u64) -> u64, body: &dyn Fn(&str) -> String| {
            let faces: Vec<String> = (0..1 + pick(seed, 1, 3)).map(|k| face(&pick, seed, 100 * (k + 1), 2, id, body)).collect();
            let tie = if pick(seed, 2, 2) == 0 { format!("~{}", pick(seed, 3, 4)) } else { String::new() };
            format!("{}{tie}", faces.join("|"))
        };
        let name = named(&|id| id, &|b| b.to_string());
        let all_pair = !named(&|id| if id == 5 { 0 } else { id }, &|b| b.to_string()).contains("F0");
        let got = p.edge(&edge(&name, 2, "Plate", &[7]), &[7], &[9]).map(|e| e.name);
        if all_pair {
            let want = named(&|id| p.features.get(&id).copied().unwrap_or(id), &|b| p.bodies.get(b).cloned().unwrap_or_default());
            assert_eq!(got.as_deref(), Some(want.as_str()), "seed {seed}: {name}");
            assert_eq!(EdgeName::parse(&want).map(|e| e.to_string()).as_deref(), Ok(want.as_str()), "seed {seed}: reads back");
            moved += 1;
        } else {
            assert_eq!(got, None, "seed {seed}: {name} names F5");
        }
    }
    // Both kinds are made: some names move and some don't.
    assert!(moved > 50 && moved < 550, "{moved} of 600 moved");
}

/// A note moves whole, beside its references: its anchor's and every branch's, or nothing when one
/// of them names a feature with no pair; a note on its view alone moves its branches.
#[test]
fn a_note_moves_its_anchor_and_every_branch() {
    use crate::drawing::note::{Anchor, Branch, Note};
    let note = |anchor: Anchor, branches: Vec<Branch>| Note {
        id: 1,
        anchor,
        at: [0.0, 0.0],
        content: solidcraft_geom::cell::Content(vec![solidcraft_geom::cell::Part::Text("A".into())]),
        cap_mm: None,
        branches,
    };
    let on = |name: &str, path: &[u64]| edge(name, 2, "Plate", path);
    let moved = note(Anchor::View(4), vec![Branch::Edge(on("F2:top|F3:side:l4", &[7]))]).moved(&pairs(), &[7], &[9]);
    assert_eq!(moved, Some(note(Anchor::View(4), vec![Branch::Edge(edge("F20:top|F30:side:l4", 20, "Plate (copy)", &[9]))])));
    let anchored = note(Anchor::Edge { view: 4, edge: on("F2:top|F3:side:l4", &[7]) }, Vec::new()).moved(&pairs(), &[7], &[9]);
    let anchor = Anchor::Edge { view: 4, edge: edge("F20:top|F30:side:l4", 20, "Plate (copy)", &[9]) };
    assert_eq!(anchored.map(|n| n.anchor), Some(anchor), "the anchor's edge moves too");
    let half = note(Anchor::Edge { view: 4, edge: on("F2:top|F3:side:l4", &[7]) }, vec![Branch::Edge(on("F2:top|F5:end", &[7]))]);
    assert_eq!(half.moved(&pairs(), &[7], &[9]), None, "a branch naming F5 keeps the whole note");
}

/// A dimension moves its references and the model dimension it shows; when that model dimension's
/// feature has no pair the dimension still moves, showing none.
#[test]
fn a_dimension_moves_with_its_model_dimension() {
    use crate::drawing::dimension::{Dimension, DimensionKind, ModelDimension, TextPlace};
    use solidcraft_geom::drawing_measure::{LinearDirection, Reading};
    let length = |path: &[u64], name: &str, feature: u64, body: &str, shown: u64| Dimension {
        id: 1,
        view: 4,
        kind: DimensionKind::Length { edge: edge(name, feature, body, path), direction: LinearDirection::Horizontal, reading: Reading::Projected },
        offset_mm: -10.0,
        text: TextPlace::Centred,
        decimals: 1,
        override_text: None,
        last_good: None,
        from_model: Some(ModelDimension::Feature { feature: shown, param: "Distance".into() }),
    };
    let moved = length(&[7], "F2:top|F3:side:l4", 2, "Plate", 2).moved(&pairs(), &[7], &[9]);
    assert_eq!(moved, Some(length(&[9], "F20:top|F30:side:l4", 20, "Plate (copy)", 20)));
    let unshown = length(&[7], "F2:top|F3:side:l4", 2, "Plate", 5).moved(&pairs(), &[7], &[9]).unwrap();
    assert_eq!((unshown.kind, unshown.from_model), (length(&[9], "F20:top|F30:side:l4", 20, "Plate (copy)", 20).kind, None));
}

/// A model dimension moves to its feature's pair, with its constraint id or label as it is, and
/// goes when the feature has no pair.
#[test]
fn a_model_dimension_moves_to_its_features_pair() {
    use crate::drawing::dimension::ModelDimension;
    let sketch = ModelDimension::Sketch { sketch: 3, constraint: "d1".into() };
    assert_eq!(sketch.moved(&pairs()), Some(ModelDimension::Sketch { sketch: 30, constraint: "d1".into() }));
    let feature = ModelDimension::Feature { feature: 2, param: "Distance".into() };
    assert_eq!(feature.moved(&pairs()), Some(ModelDimension::Feature { feature: 20, param: "Distance".into() }));
    assert_eq!(ModelDimension::Feature { feature: 5, param: "Distance".into() }.moved(&pairs()), None);
}
