use solidcore_record::Vec3;

use super::*;
use crate::{FeatureKind, Operation, point_expr};

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
        };
        assert_eq!(doc.add_feature(kind, None).unwrap(), i + 1);
    }
    doc.features.swap(2, 3);
    doc
}

/// An edge covers the latest feature in the timeline among its faces, not the highest id; a
/// corner the latest among all its edges' faces; a face its own feature.
#[test]
fn a_reference_covers_the_latest_feature_that_made_its_faces() {
    let t = Timeline::of(&design());
    assert_eq!(t.covers(&Reference::Edge("F1:+z|F2:top")), Some(2));
    assert_eq!(t.covers(&Reference::Edge("F3:side|F4:top")), Some(3), "F3 comes after F4");
    assert_eq!(t.covers(&Reference::Edge("F1:+x|F1:-y")), Some(1));
    let corner = ["F1:+x|F1:-y".to_string(), "F1:-y|F4:top".to_string()];
    assert_eq!(t.covers(&Reference::Corner(&corner)), Some(4));
    assert_eq!(t.covers(&Reference::Face("F2:top")), Some(2));
}

/// A pattern copy's face is the pattern's, a split piece's neighbours don't count, and a face of
/// no feature adds nothing.
#[test]
fn copies_pieces_and_faces_of_no_feature() {
    let t = Timeline::of(&design());
    assert_eq!(t.covers(&Reference::Face("F1:side:l1@F2.1")), Some(2));
    assert_eq!(t.covers(&Reference::Face("F1:top#[F3:side:l2,F4:side:l2]")), Some(1));
    assert_eq!(t.covers(&Reference::Edge("Body1:face:3|F2:top")), Some(2));
    assert_eq!(t.covers(&Reference::Edge("Body1:face:3|Body1:face:4")), None);
    assert_eq!(t.covers(&Reference::Face("Plate:rim.end")), None);
}

/// A reference that names a feature the timeline doesn't have covers nothing, and names that
/// don't read cover nothing.
#[test]
fn a_gone_feature_or_a_bad_name_covers_nothing() {
    let t = Timeline::of(&design());
    assert_eq!(t.covers(&Reference::Edge("F1:+x|F9:top")), None);
    for bad in ["", "|", "F1:blend:", "F1:top#[", &"F1:inner:".repeat(64), &"x".repeat(1 << 21)] {
        assert_eq!(t.covers(&Reference::Edge(bad)), None, "{}", bad.len());
        assert_eq!(t.covers(&Reference::Face(bad)), None, "{}", bad.len());
    }
    assert_eq!(t.covers(&Reference::Corner(&[])), None);
}
