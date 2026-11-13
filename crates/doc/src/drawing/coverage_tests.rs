use solidcore_record::Vec3;

use super::*;
use crate::drawing::dimension::{DimensionKind, ModelEdge, ModelFace, ModelPoint, Silhouette};
use crate::{FeatureKind, Operation, point_expr};
use solidcraft_geom::drawing_measure::{LinearDirection, SilhouetteSide};

/// A design of four boxes, F1 to F4, with F4 moved before F3 in the timeline.
fn design() -> Document {
    let mut doc = Document::new("coverage");
    for i in 0..4 {
        let kind = FeatureKind::Box {
            corner: point_expr(Vec3::new(20.0 * i as f64, 0.0, 0.0)),
            length: "10".into(),
            width: "10".into(),
            height: "10".into(),
            operation: Operation::NewBody,
            centred: false,
        };
        assert_eq!(doc.add_feature(kind, None).unwrap(), i + 1);
    }
    doc.features.swap(2, 3);
    doc
}

fn edge(name: &str) -> ModelEdge {
    ModelEdge { body: "Body1".into(), origin_feature: 1, name: name.into(), path: Vec::new() }
}

fn face(name: &str) -> ModelFace {
    ModelFace { body: "Body1".into(), origin_feature: 1, name: name.into(), path: Vec::new() }
}

fn corner(edges: &[&str]) -> ModelPoint {
    ModelPoint { body: "Body1".into(), origin_feature: 1, edges: edges.iter().map(|e| e.to_string()).collect(), centre: false, path: Vec::new() }
}

/// The feature an edge, a face or a corner by these names covers.
fn covers_edge(t: &Timeline, name: &str) -> Option<u64> {
    t.covers(Reference::Edge(&edge(name)))
}

fn covers_face(t: &Timeline, name: &str) -> Option<u64> {
    t.covers(Reference::Face(&face(name)))
}

/// An edge covers the latest feature in the timeline among its faces, not the highest id; a
/// corner the latest among all its edges' faces; a face its own feature.
#[test]
fn a_reference_covers_the_latest_feature_that_made_its_faces() {
    let t = Timeline::of(&design());
    assert_eq!(covers_edge(&t, "F1:+z|F2:top"), Some(2));
    assert_eq!(covers_edge(&t, "F3:side|F4:top"), Some(3), "F3 comes after F4");
    assert_eq!(covers_edge(&t, "F1:+x|F1:-y"), Some(1));
    assert_eq!(t.covers(Reference::Point(&corner(&["F1:+x|F1:-y", "F1:-y|F4:top"]))), Some(4));
    assert_eq!(covers_face(&t, "F2:top"), Some(2));
}

/// A pattern copy's face is the pattern's, a split piece's neighbours don't count, and a face of
/// no feature adds nothing.
#[test]
fn copies_pieces_and_faces_of_no_feature() {
    let t = Timeline::of(&design());
    assert_eq!(covers_face(&t, "F1:side:l1@F2.1"), Some(2));
    assert_eq!(covers_face(&t, "F1:top#[F3:side:l2,F4:side:l2]"), Some(1));
    assert_eq!(covers_edge(&t, "Body1:face:3|F2:top"), Some(2));
    assert_eq!(covers_edge(&t, "Body1:face:3|Body1:face:4"), None);
    assert_eq!(covers_face(&t, "Plate:rim.end"), None);
}

/// A reference that names a feature the timeline doesn't have covers nothing, and names that
/// don't read cover nothing.
#[test]
fn a_gone_feature_or_a_bad_name_covers_nothing() {
    let t = Timeline::of(&design());
    assert_eq!(covers_edge(&t, "F1:+x|F9:top"), None);
    for bad in ["", "|", "F1:blend:", "F1:top#[", &"F1:inner:".repeat(64), &"x".repeat(1 << 21)] {
        assert_eq!(covers_edge(&t, bad), None, "{}", bad.len());
        assert_eq!(covers_face(&t, bad), None, "{}", bad.len());
    }
    assert_eq!(t.covers(Reference::Point(&corner(&[]))), None);
}

/// A dimension's references are one list: its paths and what it covers are both read from it, each
/// reference's path and names from the same record.
#[test]
fn a_dimensions_paths_and_coverage_read_one_list() {
    let t = Timeline::of(&design());
    let on = |name: &str, path: &[u64]| Silhouette { face: ModelFace { path: path.to_vec(), ..face(name) }, side: SilhouetteSide::Near };
    let kind = DimensionKind::Silhouettes { a: on("F2:top", &[7]), b: on("F3:side", &[8, 9]), direction: LinearDirection::Aligned };
    assert_eq!(kind.paths(), [&[7][..], &[8, 9][..]]);
    assert_eq!(kind.references().into_iter().map(|r| t.covers(r)).collect::<Vec<_>>(), [Some(2), Some(3)]);
}

/// The feature a face's name says made it: its own, a copy's pattern, none for a face of no
/// feature or a name that doesn't read.
#[test]
fn a_face_is_made_by_the_feature_its_name_says() {
    assert_eq!(made_by("F2:top"), Some(2));
    assert_eq!(made_by("F1:side:l1@F2.1"), Some(2));
    assert_eq!(made_by("F1:top#[F3:side:l2,F4:side:l2]"), Some(1));
    for none in ["Body1:face:3", "Plate:rim.end", "", "F1:blend:"] {
        assert_eq!(made_by(none), None, "{none}");
    }
}

/// A cap is a whole face of the feature at its start, its end, or its sketch plane's top or bottom:
/// not a side, a copy, a piece, or another feature's.
#[test]
fn a_cap_is_a_whole_start_end_top_or_bottom_face_of_the_feature() {
    for cap in ["F3:start", "F3:end", "F3:top", "F3:bottom"] {
        assert!(is_cap(cap, 3), "{cap}");
        assert!(!is_cap(cap, 2), "{cap} isn't F2's");
    }
    for not in ["F3:side:l1", "F3:+z", "F3:top@F4.1", "F3:top#[F1:side:l2]", "F3:face:2", "", "F3:"] {
        assert!(!is_cap(not, 3), "{not}");
    }
}

/// Each feature's state, by the module's order of rules: suppressed and rolled back first, then
/// covered, then by how the views draw it. An annotation that names one feature twice covers it
/// once; one that covers two features is listed by each.
#[test]
fn each_state_comes_from_its_rule_in_order() {
    let mut doc = design();
    let order: Vec<u64> = doc.features.iter().map(|f| f.id).collect();
    assert_eq!(order, [1, 2, 4, 3]);
    let (top, side) = (edge("F1:+z|F2:top"), edge("F1:+x|F1:-y"));
    let annotations =
        || vec![("d1", vec![Reference::Edge(&top), Reference::Edge(&top)]), ("d2", vec![Reference::Edge(&top), Reference::Edge(&side)])];
    let mut drawn = Drawn::default();
    drawn.exact.extend([1, 2, 4]);
    drawn.inexact.extend([3, 4]);
    let states =
        |doc: &Document, drawn: &Drawn| of(doc, annotations(), drawn).into_iter().map(|f| (f.feature, f.state, f.by, f.why)).collect::<Vec<_>>();
    assert_eq!(
        states(&doc, &drawn),
        [
            (1, State::Covered, vec!["d2"], None),
            (2, State::Covered, vec!["d1", "d2"], None),
            (4, State::NotCovered, vec![], None),
            (3, State::Unknown, vec![], Some("the faces a view draws of it aren't named from exact history, so what it made can't be told")),
        ]
    );
    // Only reference parts draw F3, and nothing draws F4.
    let drawn = Drawn { reference: [3].into(), ..Drawn::default() };
    let left: Vec<_> = states(&doc, &drawn).into_iter().skip(2).map(|(_, state, _, why)| (state, why)).collect();
    assert_eq!(left, [(State::NotApplicable, Some("no view draws a face it made")), (State::NotApplicable, Some("only reference parts draw it"))]);
    // Suppressed F2 and rolled-back F3 are not applicable, covered or not.
    doc.features[1].suppressed = true;
    doc.marker = Some(3);
    let all = states(&doc, &drawn);
    assert_eq!((all[1].1, all[1].3), (State::NotApplicable, Some("it is suppressed")));
    assert_eq!((all[3].1, all[3].3), (State::NotApplicable, Some("it is rolled back")));
    assert_eq!(all[0].1, State::Covered);
}
