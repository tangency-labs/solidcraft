use serde_json::{Value, json};

use crate::FeatureKind;
use solidcore_record::living_hinge::HingeClip;

fn kind(v: Value) -> FeatureKind {
    serde_json::from_value(v).unwrap()
}

#[test]
fn a_living_hinge_with_every_field_saves_unchanged() {
    let v = json!({
        "type": "living_hinge",
        "sketch": 3,
        "profiles": {"type": "indices", "indices": [0, 1]},
        "slot_length": "20",
        "bridge": "3",
        "row_spacing": "2",
        "slot_width": "0.3",
        "angle": "90",
        "anchor": [10.0, 5.0, 0.0],
        "clip": "shorten",
        "margin": "1",
        "targets": ["Plate"]
    });
    assert_eq!(serde_json::to_value(kind(v.clone())).unwrap(), v);
}

#[test]
fn a_living_hinge_without_its_optional_fields_gets_the_defaults() {
    let v = json!({
        "type": "living_hinge",
        "sketch": 3,
        "profiles": {"type": "indices", "indices": [0]},
        "slot_length": "20",
        "bridge": "3",
        "row_spacing": "2"
    });
    let k = kind(v.clone());
    let FeatureKind::LivingHinge { slot_width, angle, anchor, clip, margin, targets, .. } = &k else {
        panic!("not a living hinge: {k:?}");
    };
    assert!(slot_width.is_none() && angle.is_none() && anchor.is_none() && margin.is_none() && targets.is_empty());
    assert_eq!(*clip, HingeClip::RunOut);
    // The clip is always written; nothing else is added.
    let mut want = v;
    want["clip"] = json!("run_out");
    assert_eq!(serde_json::to_value(&k).unwrap(), want);
}

#[test]
fn a_living_hinge_has_its_tags() {
    let k = kind(json!({
        "type": "living_hinge",
        "sketch": 3,
        "profiles": {"type": "indices", "indices": [0]},
        "slot_length": "20",
        "bridge": "3",
        "row_spacing": "2"
    }));
    assert_eq!(k.kind_key(), "living_hinge");
    assert_eq!(k.type_name(), "LivingHingeFeature");
    assert_eq!(k.base_name(), "LivingHinge");
    assert_eq!(FeatureKind::type_name_to_kind_key("LivingHingeFeature"), Some("living_hinge"));
}

/// A 100 x 60 x 3 mm plate under the XY plane (its top in the plane), a sketch on XY of the
/// rectangle from `a` to `c`, and a living hinge of `extra` fields over it: (document, hinge id).
fn plate_and_hinge(a: solidcore_record::Vec2, c: solidcore_record::Vec2, extra: Value) -> (crate::Document, u64) {
    let mut doc = plate();
    let id = hinge_over(&mut doc, a, c, extra);
    (doc, id)
}

/// A document of the 100 x 60 x 3 mm plate alone.
fn plate() -> crate::Document {
    use solidcore_record::Vec3;
    let mut doc = crate::Document::new("hinge");
    let plate = FeatureKind::Box {
        corner: crate::point_expr(Vec3::new(0.0, 0.0, -3.0)),
        length: "100 mm".into(),
        width: "60 mm".into(),
        height: "3 mm".into(),
        operation: crate::Operation::NewBody,
        centred: false,
    };
    doc.add_feature(plate, Some("Plate")).unwrap();
    doc
}

/// A sketch on XY of the rectangle from `a` to `c` and a living hinge of `extra` fields over it,
/// added to `doc`: the hinge's id.
fn hinge_over(doc: &mut crate::Document, a: solidcore_record::Vec2, c: solidcore_record::Vec2, extra: Value) -> u64 {
    use solidcore_record::Vec2;
    let mut sk = solidcraft_sketch::Sketch::new();
    let p = [a, Vec2::new(c.x, a.y), c, Vec2::new(a.x, c.y)].map(|q| sk.add_point(q, None).unwrap());
    for k in 0..4 {
        sk.add_line_pts(p[k], p[(k + 1) % 4], None).unwrap();
    }
    let sketch = doc.add_feature(FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    let mut v =
        json!({"type": "living_hinge", "sketch": sketch, "profiles": {"type": "all"}, "slot_length": "10", "bridge": "3", "row_spacing": "2"});
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    doc.add_feature(kind(v), Some("Hinge")).unwrap()
}

/// The model of `doc`, the hinge's result and the volume of every body after it.
fn evaluate(doc: &crate::Document, id: u64) -> (crate::FeatureResult, f64) {
    let mut model = crate::Model::default();
    model.evaluate(doc);
    let r = model.results.iter().find(|r| r.id == id).unwrap().clone();
    let volume = model.state().bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum();
    (r, volume)
}

#[test]
fn a_living_hinge_cuts_its_slots_through_the_plate() {
    use solidcore_record::Vec2;
    for (a, c, angle) in [(Vec2::new(30.0, 20.0), Vec2::new(70.0, 40.0), "0"), (Vec2::new(40.0, 10.0), Vec2::new(60.0, 50.0), "90")] {
        let (doc, id) = plate_and_hinge(a, c, json!({"angle": angle}));
        let (r, volume) = evaluate(&doc, id);
        assert!(r.error.is_none(), "{angle}: {:?}", r.error);
        // The slots take at most their width out of each row spacing: a tenth of the area.
        let removed = 100.0 * 60.0 * 3.0 - volume;
        assert!(removed > 20.0 && removed < 40.0 * 20.0 * 3.0 * 0.1 + 1e-6, "{angle}: {removed}");
    }
}

#[test]
fn a_living_hinge_beside_the_plate_is_refused() {
    use solidcore_record::Vec2;
    let (doc, id) = plate_and_hinge(Vec2::new(200.0, 0.0), Vec2::new(240.0, 20.0), json!({}));
    assert!(evaluate(&doc, id).0.error.is_some());
}

#[test]
fn a_living_hinge_over_the_cap_is_refused_before_cutting() {
    use solidcore_record::Vec2;
    let (doc, id) = plate_and_hinge(Vec2::new(10.0, 10.0), Vec2::new(90.0, 50.0), json!({"slot_length": "2", "bridge": "1", "row_spacing": "0.5"}));
    let e = evaluate(&doc, id).0.error.unwrap_or_default();
    assert!(e.contains(&format!("more than the {} a living hinge cuts", crate::MAX_CUT_SLOTS)), "{e}");
}

#[test]
fn a_living_hinge_reports_what_it_cut() {
    use solidcore_record::Vec2;
    let (doc, id) = plate_and_hinge(Vec2::new(30.0, 20.0), Vec2::new(70.0, 40.0), json!({}));
    let mut model = crate::Model::default();
    model.evaluate(&doc);
    let r = model.results.iter().find(|r| r.id == id).unwrap();
    assert!(r.error.is_none(), "{:?}", r.error);
    let report = r.state.living_hinge_report(id).expect("the hinge's report");
    assert!(report.slots > 0 && report.rows > 0 && report.rows <= report.slots, "{report:?}");
    assert_eq!(report.whole + report.shortened + report.run_out, report.slots, "{report:?}");
    assert!(report.cut_length > 0.0 && report.outline_length > report.cut_length, "{report:?}");
    assert!(report.open_area > 0.0, "{report:?}");
    assert!(report.band_width > 0.0 && report.band_width <= 40.0 + 1e-9, "{report:?}");
}

#[test]
fn a_living_hinge_on_sheet_metal_is_refused() {
    use solidcore_record::Vec2;
    let mut doc = crate::Document::new("hinge");
    let mut sk = solidcraft_sketch::Sketch::new();
    let p = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 60.0), Vec2::new(0.0, 60.0)].map(|q| sk.add_point(q, None).unwrap());
    for k in 0..4 {
        sk.add_line_pts(p[k], p[(k + 1) % 4], None).unwrap();
    }
    let sketch = doc.add_feature(FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    doc.add_feature(FeatureKind::SheetBase { sketch: crate::SketchId(sketch), profiles: crate::ProfileSel::All, rule: None, flip: false }, None)
        .unwrap();
    let id = hinge_over(&mut doc, Vec2::new(30.0, 20.0), Vec2::new(70.0, 40.0), json!({}));
    let e = evaluate(&doc, id).0.error.unwrap_or_default();
    assert!(e.contains("cannot cut sheet metal"), "{e}");
}

#[test]
fn a_living_hinge_on_a_chamfered_plate_warns_and_cuts() {
    use solidcore_record::Vec2;
    let mut doc = plate();
    doc.add_feature(kind(json!({"type": "chamfer", "edges": [[50.0, 0.0, 0.0]], "distance": "1"})), Some("Chamfer")).unwrap();
    let id = hinge_over(&mut doc, Vec2::new(30.0, 20.0), Vec2::new(70.0, 40.0), json!({}));
    let (r, _) = evaluate(&doc, id);
    assert!(r.error.is_none(), "{:?}", r.error);
    let w = r.warning.unwrap_or_default();
    assert!(w.contains("neither flat nor square to the sketch"), "{w}");
}
