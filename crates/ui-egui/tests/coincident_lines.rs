//! Coincident accepts clicked line bodies as well as explicit sketch points.

mod support;

use serde_json::json;
use solidcraft_engine::sketch::ConstraintKind;
use solidcraft_ui_egui::scenario::Harness;

fn setup() -> Harness {
    support::sketch_lines(&[[[10, 10], [30, 10]], [[35, 15], [35, 40]]], Some(false))
}

#[test]
fn clicked_lines_join_closest_endpoints_in_both_orders() {
    for reverse in [false, true] {
        let mut h = setup();
        let before = serde_json::to_value(&h.app.session.doc).unwrap();
        let undo = h.app.session.history.len();
        let picks = if reverse { [[35.0, 27.5], [20.0, 10.0]] } else { [[20.0, 10.0], [35.0, 27.5]] };
        h.run(
            &json!([{"start":"sketch.constraint.coincident"}, {"click":{"sketch":picks[0]}}, {"click":{"sketch":picks[1]}}, {"expect":{"errors":0}}]),
        )
        .unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
        let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        let a = sk.resolve_point("l1.end").unwrap();
        let b = sk.resolve_point("l2.start").unwrap();
        assert!(sk.point(a).unwrap().dist(sk.point(b).unwrap()) < 1e-6);
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Coincident {p,q} if (p,q)==(a,b) || (p,q)==(b,a))));
        let fixed_ref = if reverse { "l2.end" } else { "l1.start" };
        let expected = if reverse { [35.0, 40.0] } else { [10.0, 10.0] };
        let fixed = sk.point(sk.resolve_point(fixed_ref).unwrap()).unwrap();
        assert!((fixed.x - expected[0]).abs() < 1e-6 && (fixed.y - expected[1]).abs() < 1e-6);
        assert_eq!(h.app.session.history.len(), undo + 1);
        let after = serde_json::to_value(&h.app.session.doc).unwrap();
        h.run(&json!([{"call":"engine.execute","params":{"command":"edit.undo","params":{}}}])).unwrap();
        assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), before);
        h.run(&json!([{"call":"engine.execute","params":{"command":"edit.redo","params":{}}}])).unwrap();
        assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), after);
    }
}

#[test]
fn fixed_line_pair_rejects_without_changing_document_or_history() {
    let mut h = setup();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"sketch.constraint.fix","params":{"entity":"l1"}}},
        {"call":"engine.execute","params":{"command":"sketch.constraint.fix","params":{"entity":"l2"}}}
    ]))
    .unwrap();
    let before = serde_json::to_value(&h.app.session.doc).unwrap();
    let undo = h.app.session.history.len();
    h.run(&json!([{"start":"sketch.constraint.coincident"},{"click":{"sketch":[20,10]}},{"click":{"sketch":[35,27.5]}}])).unwrap();
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), before);
    assert_eq!(h.app.session.history.len(), undo);
}
