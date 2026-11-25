//! Coincident accepts clicked line bodies as well as explicit sketch points: two lines become collinear (#18232).

mod support;

use serde_json::json;
use solidcraft_engine::sketch::ConstraintKind;
use solidcraft_ui_egui::scenario::Harness;

fn setup() -> Harness {
    support::sketch_lines(&[[[10, 10], [30, 10]], [[35, 15], [35, 40]]], Some(false))
}

#[test]
fn clicked_lines_become_collinear_in_both_orders() {
    for reverse in [false, true] {
        let mut h = setup();
        let before = serde_json::to_value(&h.app.session.doc).unwrap();
        let undo = h.app.session.undo.len();
        let picks = if reverse { [[35.0, 27.5], [20.0, 10.0]] } else { [[20.0, 10.0], [35.0, 27.5]] };
        h.run(
            &json!([{"start":"sketch.constraint.coincident"}, {"click":{"sketch":picks[0]}}, {"click":{"sketch":picks[1]}}, {"expect":{"errors":0}}]),
        )
        .unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
        let sk = h.app.session.doc.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        let collinear = sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Collinear { .. })).count();
        assert_eq!(collinear, 1, "Coincident on two lines makes them collinear");
        assert!(!sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Coincident { .. })), "no endpoint is joined");
        // Both lines lie on one line: the second pick's ends are on the first's line.
        let (first, second) = if reverse { ("l2", "l1") } else { ("l1", "l2") };
        let end = |id: &str| sk.point(sk.resolve_point(id).unwrap()).unwrap();
        let (a, b) = (end(&format!("{first}.start")), end(&format!("{first}.end")));
        let len = (b.x - a.x).hypot(b.y - a.y);
        let dir = ((b.x - a.x) / len, (b.y - a.y) / len);
        for q in [end(&format!("{second}.start")), end(&format!("{second}.end"))] {
            let off = q - a;
            assert!((off.x * dir.1 - off.y * dir.0).abs() < 1e-6, "{second} is on {first}'s line");
        }
        assert_eq!(h.app.session.undo.len(), undo + 1);
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
    let undo = h.app.session.undo.len();
    h.run(&json!([{"start":"sketch.constraint.coincident"},{"click":{"sketch":[20,10]}},{"click":{"sketch":[35,27.5]}}])).unwrap();
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), before);
    assert_eq!(h.app.session.undo.len(), undo);
}
