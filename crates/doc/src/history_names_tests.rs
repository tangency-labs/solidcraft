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
    names.iter().map(|n| Named { name: n.to_string(), aliases: Vec::new(), verified: true, origins: None }).collect()
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
