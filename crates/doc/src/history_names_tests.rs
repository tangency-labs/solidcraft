use super::*;

#[test]
fn piece_names_parse_into_stem_and_bounding_faces() {
    use crate::naming::{parent_piece, strip_edge_piece, strip_piece};
    use crate::piece_names::piece_parts;
    assert_eq!(strip_piece("F2:top#[F2:side:l4,F4:side:l4]"), "F2:top");
    assert_eq!(strip_piece("F2:top#[a]#[b]"), "F2:top");
    assert_eq!(parent_piece("F2:top#[a]#[b]"), Some("F2:top#[a]"));
    assert_eq!(strip_piece("F2:top#[a]#2"), "F2:top");
    assert_eq!(strip_piece("F2:top#2"), "F2:top", "an older design's numbered piece");
    assert_eq!(strip_piece("F2:top"), "F2:top");
    assert_eq!(parent_piece("F2:top"), None);
    assert_eq!(piece_parts("F2:top#[F2:side:l4,F5:blend:A#[x,y]|B]"), Some(("F2:top", vec!["F2:side:l4", "F5:blend:A#[x,y]|B"])));
    assert_eq!(piece_parts("F2:top#[]"), Some(("F2:top", vec![])));
    assert_eq!(strip_edge_piece("F2:side:l2|F2:top#[F4:side:l2]~2"), "F2:side:l2|F2:top");
    assert_eq!(strip_edge_piece("F2:side:l2#1|F2:top#2"), "F2:side:l2|F2:top");
    // Hostile names never panic and are their own stems.
    for n in ["", "#", "#[", "]", "a#[", "a]", "#[]", "a#[b", "x#[[]]]", "é#[ü]", "#1", "a#[b]#", "~", "a~"] {
        let _ = (strip_piece(n), parent_piece(n), piece_parts(n), strip_edge_piece(n));
    }
    assert_eq!(strip_piece("#[]"), "#[]");
}

fn named(names: &[&str]) -> Vec<Named> {
    names.iter().map(|n| Named { name: n.to_string(), verified: true, ..Named::default() }).collect()
}

#[test]
fn pieces_are_told_apart_by_the_faces_only_they_touch() {
    // A top face cut in two by a slot: each piece touches the front and back walls (common),
    // its own end wall and its own slot wall.
    let faces = named(&["top", "top", "front", "back", "left", "right", "slot:a", "slot:b"]);
    let adjacent = [(0, 2), (0, 3), (0, 4), (0, 6), (1, 2), (1, 3), (1, 5), (1, 7)];
    let names = qualify(faces, &adjacent);
    assert_eq!(names.faces[0], "top#[left,slot:a]");
    assert_eq!(names.faces[1], "top#[right,slot:b]");
    assert!(names.verified(0) && names.verified(1));
    // A stored name finds its piece, though the faces around it have changed since.
    assert_eq!(matches(&names, 0, "top#[left]"), Some(Match::Piece));
    assert_eq!(matches(&names, 1, "top#[left]"), Some(Match::Stem));
    assert_eq!(matches(&names, 0, "top"), Some(Match::Piece), "a piece of the named face");
    assert_eq!(matches(&names, 0, "top#[left,slot:a]"), Some(Match::Exact));
    assert_eq!(matches(&names, 2, "top"), None);
}

/// A pattern's copy names its faces after the original's with the instance after them
/// (`…@F9.1`, after any qualifier): a stored piece name of a copy finds its piece as the
/// original's does, in that copy only.
#[test]
fn a_pattern_copys_piece_names_match_as_the_originals_do() {
    let faces = named(&["top#[left,slot:a]@F9.1", "top#[right,slot:b]@F9.1", "front@F9.1", "top#[left,slot:a]@F9.2", "top#[left,slot:a]"]);
    let names = qualify(faces, &[]);
    // The faces around the piece have changed since the name was stored (other slot walls).
    let stored = "top#[left,slot:c]@F9.1";
    assert_eq!(matches(&names, 0, stored), Some(Match::Stem));
    assert_eq!(matches(&names, 1, stored), Some(Match::Stem));
    assert_eq!(matches(&names, 2, stored), None);
    assert_eq!(matches(&names, 3, stored), None, "another copy");
    assert_eq!(matches(&names, 4, stored), None, "the original");
    assert_eq!(matches(&names, 0, "top@F9.1"), Some(Match::Piece), "a piece of the copy's face");
    assert_eq!(matches(&names, 0, "top#[left,slot:a]@F9.1"), Some(Match::Exact));
    // Copies of copies carry one instance after another.
    let names = qualify(named(&["top#[left]@F9.1@F12.2", "top#[left]@F9.1@F12.1"]), &[]);
    assert_eq!(matches(&names, 0, "top#[slot:c]@F9.1@F12.2"), Some(Match::Stem));
    assert_eq!(matches(&names, 1, "top#[slot:c]@F9.1@F12.2"), None);
    // Hostile suffixes never panic.
    for n in ["@", "@F", "@F.", "@F1.", "a@F1.1@", "a#[b]@F1.1]", "@F1.1"] {
        let _ = matches(&names, 0, n);
    }
}

#[test]
fn pieces_nothing_tells_apart_are_numbered_and_unverified() {
    let faces = named(&["top", "top", "ring"]);
    let names = qualify(faces, &[(0, 2), (1, 2)]);
    assert_eq!(names.faces[0], "top#1");
    assert_eq!(names.faces[1], "top#2");
    assert!(!names.verified(0) && !names.verified(1));
}

/// #7990, change 4: pieces nothing around them tells apart, whose feature says where each
/// starts along its definition (a hole's wall's halves along its sketch circle), are numbered in
/// that order, whatever order the faces come in, and the names are exact. Where two would share a
/// place the order says nothing, and they are numbered as before.
#[test]
fn pieces_are_numbered_along_their_definition_where_the_feature_says() {
    let ranked = |order: [usize; 2]| {
        let mut faces = named(&["F3:side:c1", "F3:side:c1", "F1:top"]);
        for (face, place) in faces.iter_mut().zip(order) {
            face.ordinal = Some(place);
        }
        qualify(faces, &[(0, 2), (1, 2)])
    };
    let names = ranked([2, 1]);
    assert_eq!((names.faces[0].as_str(), names.faces[1].as_str()), ("F3:side:c1#2", "F3:side:c1#1"));
    assert!(names.verified(0) && names.verified(1));
    let names = ranked([1, 2]);
    assert_eq!((names.faces[0].as_str(), names.faces[1].as_str()), ("F3:side:c1#1", "F3:side:c1#2"));
    let names = ranked([1, 1]);
    assert!(!names.verified(0) && !names.verified(1));
}

/// #5860: a piece of a split face that is split again must not stack another qualifier on
/// its name: the name grew with every split (728 characters after eleven splits).
#[test]
fn a_piece_split_again_does_not_grow_its_name() {
    // A face split in two: the pieces are told apart by the walls only they touch.
    let first = qualify(named(&["top", "top", "wall:a", "wall:b"]), &[(0, 2), (1, 3)]);
    assert_eq!(first.faces[0], "top#[wall:a]");
    assert_eq!(first.faces[1], "top#[wall:b]");
    // The piece that touches wall:a is split again, and again: its name lists the walls
    // around it, not the qualifiers of the pieces it came from.
    let mut piece = first.faces[0].clone();
    for _ in 0..10 {
        let names = qualify(named(&[piece.as_str(), piece.as_str(), "wall:a", "wall:b"]), &[(0, 2), (1, 3)]);
        piece = names.faces[0].clone();
    }
    assert_eq!(piece, "top#[wall:a]");
}

/// A name has to stay within the length a command accepts for a face name (512 bytes), or
/// the face could not be named again.
#[test]
fn a_piece_with_many_faces_around_it_keeps_a_name_that_can_be_used() {
    let around: Vec<String> = (0..80).map(|i| format!("F{i}:blend:face:{i}")).collect();
    let mut faces: Vec<&str> = vec!["top", "top"];
    faces.extend(around.iter().map(String::as_str));
    // Each piece touches forty faces of its own: none of them tells the pieces apart.
    let adjacent: Vec<(usize, usize)> = (2..42).map(|i| (0, i)).chain((42..82).map(|i| (1, i))).collect();
    let names = qualify(named(&faces), &adjacent);
    assert!(names.faces.iter().all(|n| n.len() <= 512), "{:?}", names.faces);
    assert_ne!(names.faces[0], names.faces[1], "the pieces are still told apart");
}

/// A name saved in the older form (`F5:blend:F2:+x|F2:+z`) names the same face as today's
/// `F5:blend(F2:+x|F2:+z)`, so a stored name still finds the face, its piece and its stem.
#[test]
fn a_name_in_the_older_form_still_finds_the_face_it_names() {
    let names = qualify(named(&["F5:blend(F2:+x|F2:+z)#[F6:-y]", "F2:+x", "F2:+z"]), &[]);
    // The face is a piece of the blend face the stored name names.
    assert_eq!(matches(&names, 0, "F5:blend:F2:+x|F2:+z"), Some(Match::Piece), "the older form names the same face");
    assert_eq!(matches(&names, 0, "F5:blend(F2:+x|F2:+z)"), Some(Match::Piece), "the current form as well");
    // Another face of the design is not that face.
    assert_eq!(matches(&names, 1, "F5:blend:F2:+x|F2:+z"), None);
    // The inner face of an older design is found as well.
    let names = qualify(named(&["F3:inner(F2:top)#[F4:-y]", "F2:top"]), &[]);
    assert_eq!(matches(&names, 0, "F3:inner:F2:top"), Some(Match::Piece), "the older form names the same face");
    assert_eq!(matches(&names, 1, "F3:inner:F2:top"), None);
}

#[test]
fn a_merged_face_takes_the_shared_stem_or_the_first_name_and_answers_to_the_others() {
    let m = merged(named(&["top#[a]", "top#[b]"])).unwrap();
    assert_eq!(m.name, "top");
    assert_eq!(m.aliases, vec!["top#[a]".to_string(), "top#[b]".to_string()]);
    let m = merged(named(&["top#1", "top#2"])).unwrap();
    assert_eq!(m.name, "top", "numbered pieces joined again");
    let m = merged(named(&["F2:top", "F9:top"])).unwrap();
    assert_eq!(m.name, "F2:top");
    assert_eq!(m.aliases, vec!["F9:top".to_string()]);
    let names = qualify(vec![m], &[]);
    assert_eq!(matches(&names, 0, "F9:top"), Some(Match::Alias));
}

/// A side drawn in two collinear pieces is one face its feature made from both: each piece's role
/// names that very face, a piece of it once it is split. Joined with another feature's face, it
/// keeps them, and the other face's names are aliases only.
#[test]
fn a_face_made_from_several_roles_is_named_by_each_and_keeps_them_when_merged() {
    let side = Named { name: "F2:side:c1".into(), synonyms: vec!["F2:side:c2".into()], verified: true, ..Named::default() };
    let names = qualify(vec![side.clone()], &[]);
    assert_eq!(names.faces[0], "F2:side:c1");
    assert_eq!(matches(&names, 0, "F2:side:c2"), Some(Match::Exact));
    let other = Named { name: "F9:side:a".into(), synonyms: vec!["F9:side:b".into()], verified: true, ..Named::default() };
    let m = merged(vec![side.clone(), other]).unwrap();
    assert_eq!((m.name.as_str(), m.synonyms.as_slice()), ("F2:side:c1", ["F2:side:c2".to_string()].as_slice()));
    assert_eq!(m.aliases, vec!["F9:side:a".to_string(), "F9:side:b".to_string()]);
    let names = qualify(vec![m], &[]);
    assert_eq!(matches(&names, 0, "F2:side:c2"), Some(Match::Exact));
    assert_eq!(matches(&names, 0, "F9:side:b"), Some(Match::Alias));
    // Split by a cut: each piece is a piece of what either role names.
    let names = qualify(vec![side.clone(), side, Named { name: "F4:side:x".into(), ..Named::default() }], &[(0, 2)]);
    assert_eq!(names.faces[0], "F2:side:c1#[F4:side:x]");
    assert_eq!(matches(&names, 0, "F2:side:c2"), Some(Match::Piece));
}

/// #3330: a linked design's faces are named after the names that design gave them, so what the
/// name holds must mean nothing else in the assembly, while its pieces stay pieces.
#[test]
fn a_linked_designs_face_names_keep_their_pieces_and_nothing_else() {
    use crate::lineage::link_role;
    let role = |n: &str| link_role(7, n, 0);
    // A whole face is a digest of its name, the same on every build.
    assert_eq!(role("F2:top"), "link:eb25d870dc3c9480");
    assert_ne!(role("F2:side:c0"), role("F3:side:c0"));
    // No feature number, `@`, `|` or `#` of that design is left to be read as one of this design.
    let blend = role("F3:blend:F2:top|F2:side:l4@F5.1");
    assert!(blend.starts_with("link:") && !blend.contains(['@', '|', '#', 'F']), "{blend}");
    // A piece keeps its qualifier, the faces around it named as they are here.
    assert_eq!(role("F2:top#[F2:side:l4,F4:side:l4]#2"), format!("{}#[F7:{},F7:{}]#2", role("F2:top"), role("F2:side:l4"), role("F4:side:l4")));
    assert_eq!(role("F2:top#3"), format!("{}#3", role("F2:top")));
    let nested = format!("F7:{}", role("F2:top#[a]#[b]"));
    assert_eq!(crate::naming::strip_piece(&nested), format!("F7:{}", role("F2:top")));
    assert_eq!(crate::naming::parent_piece(&nested), Some(format!("F7:{}", role("F2:top#[a]")).as_str()));
    // Hostile names never panic, and qualifiers are read only so deep.
    assert!(role(&format!("F1:top{}", "#[a]".repeat(10_000))).len() < 2_000);
    for n in ["", "#", "#[", "]", "a#[", "#[]", "x#[[]]]", "#1", "a#[b]#", "é#[ü]"] {
        let _ = role(n);
    }
}

/// A face that was split when a name was stored and is whole again (#11731): the stored piece
/// is all of it, when the face still touches the faces that told the piece apart.
#[test]
fn a_piece_name_finds_its_face_once_it_is_whole_again() {
    let faces = named(&["top", "left", "right", "start"]);
    let names = qualify(faces, &[(0, 1), (0, 3)]);
    assert_eq!(names.faces[0], "top");
    assert_eq!(matches(&names, 0, "top#[left]"), Some(Match::Piece));
    assert_eq!(matches(&names, 0, "top#[left,start]"), Some(Match::Piece));
    // It no longer touches a face the qualifier names: some other piece, gone since.
    assert_eq!(matches(&names, 0, "top#[right]"), Some(Match::Stem));
    assert_eq!(matches(&names, 0, "top#[left]@F9.1"), None, "a copy's piece");
    // A numbered piece (nothing told it apart, or a tie) says nothing about which part it was.
    assert_eq!(matches(&names, 0, "top#1"), Some(Match::Stem));
    assert_eq!(matches(&names, 0, "top#[left]#2"), Some(Match::Stem));
    assert_eq!(matches(&names, 0, "top#[]"), Some(Match::Stem));
}

/// A fillet or chamfer replaces the edge between two faces with a face of its own, named after
/// that edge (`F3:blend:side|start`): the two no longer touch, and the blend is what tells a piece
/// of one from a piece of another. A piece told apart by the face the blend separated from is
/// still found, so the edge saved with it is too (#4325).
#[test]
fn a_piece_name_survives_the_blend_that_replaced_the_edge_it_was_told_apart_by() {
    // The side now meets the start cap only through the blend along their old edge.
    let faces = named(&["side", "start", "F3:blend:side|start"]);
    let names = qualify(faces, &[(0, 2), (1, 2)]);
    assert_eq!(names.faces[0], "side");
    assert_eq!(matches(&names, 0, "side#[start]"), Some(Match::Piece));
    // A face the blend does not round an edge with is not one the side is told apart by.
    assert_eq!(matches(&names, 0, "side#[top]"), Some(Match::Stem));
    assert_eq!(matches(&names, 0, "side#[F3:blend:side|top]"), Some(Match::Stem), "a blend of another edge");
}

/// A blend face's own name is the edge it replaced, so the faces that told a piece of one apart
/// sit inside that name (`F3:blend:side#[start]|start`); the face whole again since is the blend
/// of the sides without their qualifiers (#4325).
#[test]
fn a_piece_of_a_blend_face_finds_it_once_it_is_one_face_again() {
    let faces = named(&["F3:blend:side|start", "side", "start"]);
    let names = qualify(faces, &[(0, 1), (0, 2)]);
    assert_eq!(names.faces[0], "F3:blend:side|start");
    assert_eq!(matches(&names, 0, "F3:blend:side#[start]|start"), Some(Match::Piece));
    assert_eq!(matches(&names, 0, "F3:blend:side#[side]|start"), Some(Match::Piece));
    // A face the blend no longer touches, inside its name: not its piece.
    assert_eq!(matches(&names, 0, "F3:blend:side#[top]|start"), None);
    // A qualifier on the side that is whole anyway is not a piece at all.
    assert_eq!(matches(&names, 0, "F3:blend:side|start"), Some(Match::Exact));
    // A pattern copy's blend: the qualifier carries the copy's instance, as the faces around the
    // copy do, and the name compares within that copy only.
    let copied = qualify(named(&["F3:blend:side|start@F9.1", "side@F9.1", "start@F9.1"]), &[(0, 1), (0, 2)]);
    assert_eq!(matches(&copied, 0, "F3:blend:side#[start@F9.1]|start@F9.1"), Some(Match::Piece));
    assert_eq!(matches(&copied, 0, "F3:blend:side#[start@F9.1]|start@F9.2"), None, "another copy");
}

/// A piece of a blend face is found by the name the blend is written in today as well: the
/// qualifier that tells the piece apart sits inside the blend's name, on one of the edge's sides,
/// and the face whole again since is the blend of the sides without their qualifiers.
#[test]
fn a_piece_of_a_blend_face_is_found_by_the_name_the_blend_is_written_in_today() {
    let faces = named(&["F3:blend(side|start)", "side", "start"]);
    let names = qualify(faces, &[(0, 1), (0, 2)]);
    assert_eq!(names.faces[0], "F3:blend(side|start)");
    assert_eq!(matches(&names, 0, "F3:blend(side#[start]|start)"), Some(Match::Piece));
    assert_eq!(matches(&names, 0, "F3:blend(side#[side]|start)"), Some(Match::Piece));
    // A face the blend no longer touches, inside its name: not its piece.
    assert_eq!(matches(&names, 0, "F3:blend(side#[top]|start)"), None);
}

/// Two pieces of one face that a blend tells apart from the same face both answer to the stored
/// name: one through the face itself, one through the blend that replaced its edge. The lookup
/// chooses neither (#4325).
#[test]
fn two_pieces_told_apart_by_one_blend_both_answer_to_a_stored_name() {
    let doc = crate::Document {
        features: serde_json::from_value(serde_json::json!([
            {"id": 1, "name": "Box1", "type": "box", "corner": [0, 0, 0], "length": "40", "width": "30", "height": "10"},
            {"id": 2, "name": "Round", "type": "fillet", "edges": [[20, 0, 10]], "radius": "2"},
            // A slot through the front wall, top to bottom: the wall is two pieces, and each keeps
            // the round along its own top edge.
            {"id": 3, "name": "Slot", "type": "box", "corner": [15, -1, 0], "length": "10", "width": "12", "height": "12"},
            {"id": 4, "name": "SlotCut", "type": "combine", "target": "Body1", "tools": ["Body2"], "operation": "cut"}
        ]))
        .unwrap(),
        ..crate::Document::default()
    };
    let doc = crate::format::read(&doc.to_json()).unwrap();
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    let state = model.state();
    let body = state.body("Body1").unwrap();
    let names = crate::naming::names(body);
    let stored = "F1:-y#[F1:+z]";
    let answering: Vec<usize> =
        (0..names.faces.len()).filter(|face| matches(&names, *face, stored).is_some_and(|level| level >= Match::Piece)).collect();
    assert_eq!(answering.len(), 2, "{:?}", names.faces);
    assert!(crate::name_lookup::locate_face_on_body(&state, Some("Body1"), stored, Vec3::new(5., 0., 4.)).is_none(), "two faces answer");
}

/// A face named only by its index (a thread's groove, `F7:face:12`, unverified) never tells its
/// neighbours apart: the next evaluation can give that index to another face, so an exact piece
/// qualified by it could change its name or take another's. Pieces only such faces tell apart
/// are numbered and unverified, and a stored name qualified by an index finds no piece by its name
/// alone (an edge saved with one is found only where its pick still lies beside such a face,
/// #19455).
#[test]
fn a_face_named_by_its_index_never_qualifies_an_exact_piece() {
    let groove = |name: &str| Named { name: name.to_string(), verified: false, ..Named::default() };
    let mut faces = named(&["F2:side", "F2:side", "F2:end", "F2:start"]);
    faces.extend([groove("F7:face:12"), groove("F7:face:13")]);
    // Each piece touches one end of the part and a groove face of its own.
    let names = qualify(faces, &[(0, 2), (0, 4), (1, 3), (1, 5)]);
    assert_eq!(names.faces[0], "F2:side#[F2:end]");
    assert_eq!(names.faces[1], "F2:side#[F2:start]");
    assert!(names.verified(0) && names.verified(1));
    assert!(names.info.iter().all(|i| !i.bounds.iter().any(|b| b.contains(":face:"))), "{:?}", names.info);
    assert!(matches(&names, 0, "F2:side#[F7:face:12]").is_none_or(|m| m < Match::Piece), "an index never finds a piece");
    // Told apart by nothing but groove faces: numbered, and no longer exact.
    let mut faces = named(&["F2:side", "F2:side"]);
    faces.extend([groove("F7:face:12"), groove("F7:face:13")]);
    let names = qualify(faces, &[(0, 2), (1, 3)]);
    assert!(names.faces.iter().take(2).all(|n| !n.contains(":face:")), "{:?}", names.faces);
    assert_ne!(names.faces[0], names.faces[1]);
    assert!(!names.verified(0) && !names.verified(1));
}

/// A modelled thread in a hole wider than its minor diameter narrows the bore with a tool whose
/// faces have no roles. Its end is flush with the part's top face and merges into it: the top
/// face keeps its exact name (a sketch can attach to it and follows a taller part), and a face
/// made only of such a tool's faces still has none (#20594).
#[test]
fn a_top_face_keeps_its_exact_name_after_a_thread_narrows_the_hole() {
    let doc_of = |height: f64| {
        let doc = crate::Document {
            features: serde_json::from_value(serde_json::json!([
                {"id": 1, "name": "Cyl", "type": "cylinder", "base": [0, 0, 0], "radius": "25", "height": height.to_string()},
                {"id": 2, "name": "Bore", "type": "hole", "position": [0, 0, height], "direction": [0, 0, -1], "diameter": "8", "depth": "15"},
                {"id": 3, "name": "Tap", "type": "thread", "face": [4, 0, height - 5.0], "designation": "M8", "modeled": true}
            ]))
            .unwrap(),
            ..crate::Document::default()
        };
        crate::format::read(&doc.to_json()).unwrap()
    };
    let top_of = |model: &crate::Model, z: f64| {
        let state = model.state();
        let body = state.body("Body1").unwrap().clone();
        let tops: Vec<usize> =
            (0..body.body.face_count()).filter(|&i| body.body.planar_face(i).is_some_and(|p| (p.origin.z - z).abs() < 1e-6)).collect();
        (body, tops)
    };

    let doc = doc_of(25.0);
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "the thread builds");
    let (body, tops) = top_of(&model, 25.0);
    assert_eq!(tops.len(), 1, "one top face");
    let names = crate::naming::names(&body);
    assert!(names.verified(tops[0]), "the top face's name follows exact history: {:?}", names.faces);
    let target = doc.lineage_face_target(&body, tops[0], solidcore_record::Vec3::new(10.0, 0.0, 25.0)).expect("a sketch can attach to the top face");

    // A taller part: the attachment finds the top face at its new height.
    let taller = doc_of(35.0);
    let mut model = crate::Model::new();
    model.evaluate(&taller);
    let (body, tops) = top_of(&model, 35.0);
    assert_eq!(tops.len(), 1);
    assert_eq!(taller.configuration_face_indices(&body, &target).unwrap(), tops);

    // A face made only of an unnamed tool's faces has no exact name.
    let unnamed = Operand::Made(vec![Vec::new()]);
    let one = OperandNames { operand: &unnamed, body: None, names: Arc::default(), edges: OnceLock::new() };
    let only = FaceProv { origins: vec![FaceOrigin::Face { operand: 0, face: 0 }], change: solidcraft_kernel::history::Change::Kept };
    assert!(from_origins(&only, &[one], 1).is_none());
}

/// #22047: a piece whose neighbour list loses one adjacent face (e.g. a shortened bevel or corner
/// cut) is still uniquely identified by its surviving neighbours, both in one design and across a
/// link, while numbered ties, split pieces, pattern copies and zero-overlap whole faces are
/// refused.
#[test]
fn piece_overlap_and_resuffixed_lookup_survive_adjacent_neighbour_changes() {
    use crate::lineage::link_role;
    use crate::name_lookup::unique_resuffixed_on_body;

    // Two pieces of `side`: piece 0 touches 5 unique neighbours (including `bevel`); piece 1
    // touches `top` and `notch`.
    let before = qualify(
        named(&["side", "side", "bottom", "hole1", "hole2", "hole3", "bevel", "top", "notch"]),
        &[(0, 2), (0, 3), (0, 4), (0, 5), (0, 6), (1, 7), (1, 8)],
    );
    let stored = before.faces[0].clone();
    assert_eq!(stored, "side#[bevel,bottom,hole1,hole2,hole3]");

    // Shortening the bevel removes `bevel` from piece 0's bounds while the other 4 remain.
    let after =
        qualify(named(&["side", "side", "bottom", "hole1", "hole2", "hole3", "top", "notch"]), &[(0, 2), (0, 3), (0, 4), (0, 5), (1, 6), (1, 7)]);
    assert_eq!(matches(&after, 0, &stored), Some(Match::Stem));
    assert_eq!(piece_overlap(&after, 0, &stored), Some((4, 5)));
    assert_eq!(piece_overlap(&after, 1, &stored), Some((0, 5)));
    assert_eq!(unique_resuffixed_on_body(&after, &stored), Some(0));

    // Same across a linked component (`F3:link:<digest>#[F3:link:…,…]`).
    let linked = |n: &str| format!("F3:{}", link_role(3, n, 0));
    let stored_linked = linked(&stored);
    assert!(stored_linked.starts_with("F3:link:") && stored_linked.contains("#["));
    let linked_after_faces: Vec<String> = after.faces.iter().map(|n| linked(n)).collect();
    let linked_after_refs: Vec<&str> = linked_after_faces.iter().map(String::as_str).collect();
    let linked_after = qualify(named(&linked_after_refs), &[(0, 2), (0, 3), (0, 4), (0, 5), (1, 6), (1, 7)]);
    assert_eq!(matches(&linked_after, 0, &stored_linked), Some(Match::Stem));
    assert_eq!(piece_overlap(&linked_after, 0, &stored_linked), Some((4, 5)));
    assert_eq!(unique_resuffixed_on_body(&linked_after, &stored_linked), Some(0));

    // Split piece: two pieces on the same body both keep some of the stored neighbours -> refused.
    let split = qualify(named(&["side", "side", "bottom", "hole1", "hole2", "hole3"]), &[(0, 2), (0, 3), (1, 4), (1, 5)]);
    assert_eq!(piece_overlap(&split, 0, &stored), Some((2, 5)));
    assert_eq!(piece_overlap(&split, 1, &stored), Some((2, 5)));
    assert_eq!(unique_resuffixed_on_body(&split, &stored), None);

    // Whole face (unqualified again, #11731) -> refused even if some neighbours remain.
    let whole_unrelated = qualify(named(&["side", "other"]), &[(0, 1)]);
    assert_eq!(piece_overlap(&whole_unrelated, 0, &stored), None);
    assert_eq!(unique_resuffixed_on_body(&whole_unrelated, &stored), None);

    // Numbered pieces carry no neighbour identity -> refused.
    assert_eq!(piece_overlap(&after, 0, "side#1"), None);
    assert_eq!(piece_overlap(&after, 0, "side#[bottom,hole1]#2"), None);
    assert_eq!(unique_resuffixed_on_body(&after, "side#[bottom,hole1]#2"), None);

    // Pattern copies stay isolated by instance suffix (`…#[…]@F9.1`, suffix after qualifier),
    // including when copied neighbours carry the same instance suffix in `info.bounds`.
    let copy_after = qualify(
        named(&[
            "side#[bottom,hole1,hole2,hole3]@F9.1",
            "side#[back,top]@F9.1",
            "bottom@F9.1",
            "hole1@F9.1",
            "hole2@F9.1",
            "hole3@F9.1",
            "top@F9.1",
            "back@F9.1",
        ]),
        &[(0, 2), (0, 3), (0, 4), (0, 5), (1, 6), (1, 7)],
    );
    assert_eq!(piece_overlap(&copy_after, 0, "side#[bevel,bottom,hole1,hole2,hole3]@F9.1"), Some((4, 5)));
    assert_eq!(piece_overlap(&copy_after, 0, "side#[bevel,bottom]@F9.2"), None);
    assert_eq!(piece_overlap(&copy_after, 0, &stored), None);
    assert_eq!(unique_resuffixed_on_body(&copy_after, "side#[bevel,bottom,hole1,hole2,hole3]@F9.1"), Some(0));
    assert_eq!(unique_resuffixed_on_body(&copy_after, "side#[bevel,bottom,hole1,hole2,hole3]@F9.2"), None);
}

/// #22047: an unmoved planar face piece keeps resolving through `locate_face`, `locate_one`,
/// `face_has_name` and `lineage_face_indices` when an adjacent corner cut shortens and drops off
/// its boundary, while splitting that piece itself is still rejected as ambiguous.
#[test]
fn a_planar_face_piece_stays_resolved_when_an_adjacent_cut_shortens() {
    let build = |corner_len: &str, split_main: bool| {
        let mut features = vec![
            serde_json::json!({"id": 1, "name": "Base", "type": "box", "corner": [0, 0, 0], "length": "100", "width": "40", "height": "60"}),
            serde_json::json!({"id": 2, "name": "Hole1", "type": "hole", "position": [100, 12, 35], "direction": [-1, 0, 0], "diameter": "6", "depth": "20"}),
            serde_json::json!({"id": 3, "name": "Hole2", "type": "hole", "position": [100, 28, 35], "direction": [-1, 0, 0], "diameter": "6", "depth": "20"}),
            serde_json::json!({"id": 4, "name": "CornerCut", "type": "box", "corner": [50, -1, 50], "length": corner_len, "width": "42", "height": "12", "operation": "cut"}),
            serde_json::json!({"id": 5, "name": "LowerSlot", "type": "box", "corner": [85, -1, 10], "length": "20", "width": "42", "height": "10", "operation": "cut"}),
        ];
        if split_main {
            features.push(serde_json::json!({
                "id": 6, "name": "CenterSlot", "type": "box", "corner": [85, 18, 19], "length": "20", "width": "4", "height": "45", "operation": "cut"
            }));
        }
        let doc = crate::Document { features: serde_json::from_value(serde_json::Value::Array(features)).unwrap(), ..crate::Document::default() };
        let doc = crate::format::read(&doc.to_json()).unwrap();
        let mut model = crate::Model::new();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results);
        (doc, model)
    };

    let pick = Vec3::new(100.0, 20.0, 35.0);
    let (_, before_model) = build("52", false);
    let before_state = before_model.state();
    let before_body = before_state.body("Body1").unwrap();
    let before_names = crate::naming::names(before_body);
    let main_before = (0..before_body.body.face_count())
        .find(|&f| before_body.body.point_lies_on_face(f, pick, 1e-3).ok().flatten().is_some())
        .expect("main +x face at pick");
    let stored = before_names.faces[main_before].clone();
    assert!(stored.starts_with("F1:+x#[") && stored.contains("F4:-z"), "expected corner cut in qualifier: {stored}");

    // Shortening CornerCut so it stops at x = 80 drops F4:-z from the +x face's qualifier.
    let (after_doc, after_model) = build("30", false);
    let after_state = after_model.state();
    let after_body = after_state.body("Body1").unwrap();
    let after_names = crate::naming::names(after_body);
    let main_after = (0..after_body.body.face_count())
        .find(|&f| after_body.body.point_lies_on_face(f, pick, 1e-3).ok().flatten().is_some())
        .expect("main +x face after shortening corner cut");
    assert_ne!(after_names.faces[main_after], stored, "qualifier should have changed after edit");
    assert_eq!(matches(&after_names, main_after, &stored), Some(Match::Stem));

    let hit = crate::name_lookup::locate_face(&after_state, &stored, pick).expect("locate_face recovers resuffixed piece");
    assert_eq!(hit.face, main_after);
    assert!(hit.verified);
    let one = crate::name_lookup::locate_one(&after_state, Some("Body1"), &stored).expect("locate_one recovers resuffixed piece");
    assert_eq!(one.face, main_after);
    assert!(crate::name_lookup::face_has_name(after_body, main_after, &stored));
    assert_eq!(after_doc.lineage_face_indices(after_body, &stored).unwrap(), vec![main_after]);
    assert_eq!(crate::name_lookup::face_body(&after_state, &stored).map(|b| b.name.as_str()), Some("Body1"));

    // An exact-match body must win over an overlap-only body in `face_body`, matching `locate_face`.
    let mut exact_body = before_body.clone();
    exact_body.name = "ExactBody".into();
    let mut overlap_body = after_body.clone();
    overlap_body.name = "OverlapBody".into();
    let mut mixed = crate::ModelState::default();
    mixed.bodies = vec![overlap_body, exact_body];
    assert_eq!(crate::name_lookup::face_body(&mixed, &stored).map(|b| b.name.as_str()), Some("ExactBody"));

    // Splitting the main +x piece itself so two pieces each keep one hole must stay ambiguous.
    let (split_doc, split_model) = build("30", true);
    let split_state = split_model.state();
    let split_body = split_state.body("Body1").unwrap();
    assert!(crate::name_lookup::locate_face(&split_state, &stored, Vec3::new(100.0, 6.0, 35.0)).is_none());
    assert!(crate::name_lookup::locate_one(&split_state, Some("Body1"), &stored).is_err());
    assert!(split_doc.lineage_face_indices(split_body, &stored).is_err());
}
