use super::*;

fn face(s: &str) -> FaceName {
    let f = FaceName::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
    assert_eq!(f.to_string(), s, "writes back");
    f
}

fn edge(s: &str) -> EdgeName {
    let e = EdgeName::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
    assert_eq!(e.to_string(), s, "writes back");
    e
}

fn made(feature: u64, role: Role) -> FaceName {
    FaceName { base: Base::Made { feature: FeatureId(feature), role }, suffixes: Vec::new() }
}

fn other(role: &str) -> Role {
    Role::Other(role.into())
}

#[test]
fn roles_are_typed_and_the_rest_kept_as_written() {
    assert_eq!(face("F2:top"), made(2, Role::Top));
    assert_eq!(face("F4:side#1").stem(), made(4, Role::Wall));
    assert_eq!(face("F2:side:l4"), made(2, Role::Side { profile: None, curve: "l4".into() }));
    assert_eq!(face("F2:side:s1.l4"), made(2, Role::Side { profile: Some(1), curve: "l4".into() }));
    assert_eq!(face("F3:+x"), made(3, Role::BoxFace { axis: Axis::X, high: true }));
    assert_eq!(face("F3:-z"), made(3, Role::BoxFace { axis: Axis::Z, high: false }));
    assert_eq!(face("F2:face:7"), made(2, Role::Index(7)));
    assert_eq!(face("F9:cap:high:0"), made(9, other("cap:high:0")));
    assert_eq!(face("F12:boss:side:3"), made(12, other("boss:side:3")));
    assert_eq!(face("Body1:face:3").base, Base::BodyFace { body: "Body1".into(), index: 3 });
    assert_eq!(face("Plate:rim.end").base, Base::Other("Plate:rim.end".into()));
    // Numbers that would not write back the same stay text.
    assert_eq!(face("F2:side:s01.l4"), made(2, Role::Side { profile: None, curve: "s01.l4".into() }));
}

#[test]
fn pieces_copies_and_numbered_pieces() {
    let p = face("F2:top#[F2:side:l2,F4:side:l2]");
    let Some(Suffix::Piece(q)) = p.suffixes.first() else { panic!("{p:?}") };
    assert_eq!(q.around().len(), 2);
    assert_eq!(p.stem(), made(2, Role::Top));
    assert!(matches!(face("F2:top#[F4:side:l2]#1").suffixes.as_slice(), [Suffix::Piece(Qualifier { tie: Some(1), .. })]));
    assert_eq!(face("F2:top#2").suffixes, vec![Suffix::Numbered(2)]);
    let c = face("F2:side:l1@F7.2");
    assert_eq!(c.suffixes, vec![Suffix::Copy { feature: FeatureId(7), instance: 2 }]);
    assert_eq!(c.feature(), Some(FeatureId(7)));
    assert_eq!(face("F2:side:l1@F7.2#[F8:x]").stem(), c);
    assert!(Qualifier::new(Vec::new(), None).is_none());
}

/// The fillet rounds an edge of a piece the slot made before it: the qualifier is the
/// piece's (corpus `naming-split-pieces`).
#[test]
fn a_blend_of_an_earlier_piece_keeps_the_qualifier_inside() {
    let f = face("F5:blend:F2:side:l2|F2:top#[F2:side:l2,F4:side:l2]");
    let Base::Blend { feature: FeatureId(5), rounds: Rounds::Edge(e) } = &f.base else { panic!("{f:?}") };
    assert!(f.suffixes.is_empty());
    assert_eq!(e.faces[1].stem(), made(2, Role::Top));
    assert_eq!(e.faces[1].suffixes.len(), 1);
}

/// A slot cut after the fillet splits the fillet face: the qualifier, naming the later slot,
/// is the fillet face's.
#[test]
fn a_blend_split_by_a_later_cut_takes_the_qualifier() {
    let f = face("F4:blend:F3:+x|F3:+z#[F3:-y,F5:-y]");
    let Base::Blend { feature: FeatureId(4), rounds: Rounds::Edge(e) } = &f.base else { panic!("{f:?}") };
    assert_eq!(e.faces[1], made(3, Role::BoxFace { axis: Axis::Z, high: true }));
    assert!(matches!(f.suffixes.as_slice(), [Suffix::Piece(_)]));
    // Copied by a later pattern, then split: both are the blend face's, in order.
    let g = face("F4:blend:F3:+x|F3:+z@F6.1#[F7:a]");
    assert!(matches!(g.suffixes.as_slice(), [Suffix::Copy { feature: FeatureId(6), .. }, Suffix::Piece(_)]));
    assert_eq!(g.feature(), Some(FeatureId(6)));
}

/// What the text cannot tell: a later cut told apart only by older faces reads as a piece of
/// the inner face.
#[test]
fn a_later_cut_told_apart_by_older_faces_reads_as_the_inner_faces() {
    let f = face("F4:blend:F3:+x|F3:+z#[F3:-y]");
    let Base::Blend { rounds: Rounds::Edge(e), .. } = &f.base else { panic!("{f:?}") };
    assert!(f.suffixes.is_empty() && e.faces[1].suffixes.len() == 1);
}

#[test]
fn edges_split_into_their_two_faces() {
    let e = edge("F2:side:l1|F2:side:l2");
    assert_eq!(e.faces[0], made(2, Role::Side { profile: None, curve: "l1".into() }));
    assert_eq!(edge("F2:side:l1|F2:top~1").tie, Some(1));
    // The blend face first: the whole blend name, then the slot's wall.
    let e = edge("F4:blend:F3:+x|F3:+z#[F3:-y,F5:-y]|F5:-y");
    assert_eq!(e.faces[0].to_string(), "F4:blend:F3:+x|F3:+z#[F3:-y,F5:-y]");
    assert_eq!(e.faces[1], made(5, Role::BoxFace { axis: Axis::Y, high: false }));
    // The blend face second.
    let e = edge("F2:side:l1|F5:blend:F2:side:l2|F2:top#[F2:side:l2,F4:side:l2]");
    assert!(matches!(e.faces[1].base, Base::Blend { feature: FeatureId(5), .. }));
    // A blend of a blend's edge.
    let e = edge("F6:blend:F4:blend:a|b|c|d");
    let Base::Blend { rounds: Rounds::Edge(inner), .. } = &e.faces[0].base else { panic!("{e:?}") };
    assert!(matches!(inner.faces[0].base, Base::Blend { feature: FeatureId(4), .. }));
    assert_eq!(e.faces[1].base, Base::Other("d".into()));
    // A `~k` ending the edge numbers it, not the blend's edge.
    let e = edge("F2:x|F5:blend:a|b~1");
    assert_eq!(e.tie, Some(1));
    assert!(matches!(&e.faces[1].base, Base::Blend { rounds: Rounds::Edge(b), .. } if b.tie.is_none()));
    // A blend named by its edge's index.
    assert_eq!(edge("F5:blend:3|F2:top").faces[0].base, Base::Blend { feature: FeatureId(5), rounds: Rounds::Index(3) });
}

#[test]
fn inner_walls_nest_a_face() {
    let f = face("F3:inner:F2:top#[F2:side:l2]");
    let Base::Inner { feature: FeatureId(3), offsets } = &f.base else { panic!("{f:?}") };
    assert_eq!(offsets.stem(), made(2, Role::Top));
}

#[test]
fn what_made_a_face_and_its_kind() {
    assert_eq!(made_by("F2:side:l1@F7.2"), Some(2));
    assert_eq!(made_by("F3:inner:F5:blend:a|b"), Some(3));
    assert_eq!(made_by("Body1:face:3"), None);
    assert_eq!(split_feature("F12:boss:side:3"), Some((12, "boss:side:3")));
    // A blend named only inside another name is not one.
    assert!(is_blend("F5:blend:F2:x|F2:y#[F6:z]"));
    assert!(!is_blend("F2:top#[F5:blend:a|b]"));
    assert!(!is_blend("F3:inner:F5:blend:a|b"));
    // A copy only inside a blend's edge doesn't make the blend face a copy.
    assert!(is_copy("F2:side:l1@F7.2#[F8:x]"));
    assert!(!is_copy("F5:blend:F2:x@F3.1|F2:y"));
    assert!(is_numbered_piece("F2:top#2") && is_numbered_piece("F2:top#[F4:x]#1"));
    assert!(!is_numbered_piece("F5:blend:F2:top#2|F2:x") && !is_numbered_piece("F2:top#[F4:x]"));
    assert!(is_wall("F4:side") && !is_wall("F4:side#1") && !is_wall("F5:blend:F2:x|F4:side"));
}

/// A shell's inner wall split by a later cut: the qualifier, naming the cut, is the wall's.
#[test]
fn an_inner_wall_split_by_a_later_cut_takes_the_qualifier() {
    let f = face("F3:inner:F2:top#[F2:side:l2,F6:x]");
    assert!(matches!(f.suffixes.as_slice(), [Suffix::Piece(_)]));
    assert_eq!(f.stem().to_string(), "F3:inner:F2:top");
}

#[test]
fn features_in_text_order() {
    let ids = |s: &str| face(s).features().into_iter().map(|f| f.0).collect::<Vec<_>>();
    assert_eq!(ids("F3:inner:F2:blend:F1:+y|F1:-x"), vec![3, 2, 1, 1]);
    assert_eq!(ids("F82:-z@F84.2"), vec![82, 84]);
    assert_eq!(ids("F2:top#[F2:side:l2,F4:side:l2]"), vec![2, 2, 4]);
    assert_eq!(ids("F1:side:曲線F10"), vec![1]);
    assert!(ids("Body1:face:3").is_empty());
}

#[test]
fn feature_ids_map_everywhere_but_role_text() {
    let ids = std::collections::BTreeMap::from([(2, 12), (5, 15)]);
    let mapped = map_features("F5:blend:F2:side:l2|F2:top#[F2:side:l2,F4:side:l2]@F2.1", &ids);
    assert_eq!(mapped.as_deref(), Some("F15:blend:F12:side:l2|F12:top#[F12:side:l2,F4:side:l2]@F12.1"));
    // Where an id sits: a name's ids must map, a qualifier's may stay.
    let f = face("F2:top#[F9:x]");
    let strict = f.map(
        &mut |id, place| match place {
            Place::Name => ids.get(&id.0).map(|n| FeatureId(*n)),
            Place::Qualifier => Some(ids.get(&id.0).map_or(id, |n| FeatureId(*n))),
        },
        &mut |_| None,
    );
    assert_eq!(strict.map(|f| f.to_string()).as_deref(), Some("F12:top#[F9:x]"));
    assert!(face("F9:top").map(&mut |id, _| ids.get(&id.0).map(|n| FeatureId(*n)), &mut |_| None).is_none());
    let body = face("Body1:face:3").map(&mut |id, _| Some(id), &mut |b| (b == "Body1").then(|| "Body2".to_string()));
    assert_eq!(body.map(|f| f.to_string()).as_deref(), Some("Body2:face:3"));
}

#[test]
fn errors_say_where() {
    assert_eq!(EdgeName::parse("F2:top|"), Err(NameError::Expected { at: 7, what: "a face" }));
    assert_eq!(FaceName::parse("F2:top#[F3:x"), Err(NameError::Expected { at: 12, what: "`,` or `]` in a piece's faces" }));
    assert_eq!(FaceName::parse("a|b"), Err(NameError::Expected { at: 1, what: "the end of the name" }));
    assert_eq!(FaceName::parse(""), Err(NameError::Expected { at: 0, what: "a face" }));
}

#[test]
fn hostile_names_never_panic() {
    let long = "x".repeat(MAX_NAME_LEN + 1);
    let deep = format!("{}x", "F1:inner:".repeat(64));
    let deep_pieces = format!("x{}{}", "#[x".repeat(64), "]".repeat(64));
    for s in [
        "",
        "|",
        "||",
        "#[",
        "#[]",
        "x#[",
        "x#[]",
        "x#[y",
        "x#[y,",
        "]",
        "x]",
        "F1:blend:",
        "F1:blend:x",
        "F1:blend:x|",
        "@F",
        "x@F1.",
        "x@F",
        "x~",
        "x|y~",
        "x|y~01",
        "F01:top",
        "x#0x",
        "F99999999999999999999999:top",
        "x#99999999999",
        "\u{1F600}|\u{1F601}",
        long.as_str(),
        deep.as_str(),
        deep_pieces.as_str(),
    ] {
        if let Ok(f) = FaceName::parse(s) {
            assert_eq!(f.to_string(), s);
        }
        if let Ok(e) = EdgeName::parse(s) {
            assert_eq!(e.to_string(), s);
        }
    }
    assert_eq!(FaceName::parse(&long), Err(NameError::TooLong));
    assert_eq!(FaceName::parse(&deep), Err(NameError::TooDeep));
    assert_eq!(FaceName::parse(&deep_pieces), Err(NameError::TooDeep));
    // An open boundary's edge has one face; an edge three faces meet at, three.
    assert_eq!(edge("F2:top").faces.len(), 1);
    assert_eq!(edge("x|y|z").faces.len(), 3);
    // A blend's edge has two faces at most; a third is the outer edge's.
    let e = edge("F5:blend:a|b|c");
    assert!(matches!(&e.faces[0].base, Base::Blend { rounds: Rounds::Edge(inner), .. } if inner.faces.len() == 2) && e.faces.len() == 2);
    // A fillet on an open boundary's edge.
    assert!(matches!(face("F1:blend:F10:start").base, Base::Blend { rounds: Rounds::Edge(ref e), .. } if e.faces.len() == 1));
}
