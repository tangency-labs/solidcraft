//! Designs a newer SolidCraft saved: they open, say so, and saving loses nothing.

use serde_json::{Value, json};

use crate::format::{DESIGN, FORMAT, format_string};
use crate::{Document, FeatureKind, Model, PlaneRef};

/// A real design: a sketched disc and its extrusion, and a parameter.
fn design() -> Document {
    use solidcraft_sketch::Sketch;
    let mut d = Document::new("Plate");
    d.set_param("depth", "5 mm", Some("mm"), None).unwrap();
    let mut sketch = Sketch::new();
    sketch.add_circle(solidcore_record::Vec2::new(0.0, 0.0), 10.0, None, None).unwrap();
    let base = d.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch }, Some("Base")).unwrap();
    d.add_feature(extrude(base), Some("Pad")).unwrap();
    d
}

fn extrude(sketch: u64) -> FeatureKind {
    FeatureKind::Extrude {
        thin: None,
        sketch: crate::SketchId(sketch),
        profiles: crate::ProfileSel::All,
        extent: serde_json::from_value(json!({"distance": "depth"})).unwrap(),
        operation: crate::Operation::NewBody,
        targets: vec![],
    }
}

fn saved(d: &Document) -> Value {
    serde_json::from_str(&d.to_json()).unwrap()
}

/// `(body name, volume)` of everything the design builds, and the error of each feature.
fn built(d: &Document) -> (Vec<(String, f64)>, Vec<Option<String>>) {
    let mut model = Model::default();
    model.evaluate(d);
    let bodies = model.state().bodies.iter().map(|b| (b.name.clone(), b.body.volume_at(0.01).unwrap())).collect();
    (bodies, model.results.iter().map(|r| r.error.clone()).collect())
}

fn newer() -> String {
    format_string(FORMAT + 1)
}

#[test]
fn one_format_newer_with_nothing_unknown_opens_with_the_same_bodies() {
    let original = design();
    let mut file = saved(&original);
    file["format"] = json!(newer());
    let opened = Document::from_json(&file.to_string()).unwrap();
    let (bodies, errors) = built(&opened);
    assert_eq!((bodies.clone(), errors), built(&original));
    assert_eq!(bodies.len(), 1);
    assert_eq!(opened.features, original.features);
    let notice = opened.newer_file.as_ref().unwrap().notice();
    assert_eq!(
        notice,
        format!("Saved by a newer SolidCraft (format {}, this version reads up to {FORMAT}). If something's missing, update.", FORMAT + 1)
    );
    assert!(opened.newer_file.as_ref().unwrap().not_understood.is_empty());
    // Saved, it still says what it is; read again, the same.
    let again = saved(&opened);
    assert_eq!(again["format"], json!(newer()));
    assert_eq!(Document::from_json(&opened.to_json()).unwrap(), opened);
}

#[test]
fn an_unknown_change_opens_with_a_warning_and_stays_on_the_file() {
    let original = design();
    let mut file = saved(&original);
    file["format_pending"] = json!(["a-later-change"]);
    let opened = Document::from_json(&file.to_string()).unwrap();
    let report = opened.newer_file.as_ref().unwrap();
    assert_eq!(report.unknown_changes, ["a-later-change"]);
    assert!(report.notice().contains("a-later-change") && report.notice().starts_with("Saved by a newer SolidCraft"), "{}", report.notice());
    assert_eq!(built(&opened), built(&original));
    let again = saved(&opened);
    assert_eq!(again["format"], json!(format_string(FORMAT)));
    assert!(again["format_pending"].as_array().unwrap().contains(&json!("a-later-change")));
    assert_eq!(Document::from_json(&opened.to_json()).unwrap(), opened);
}

/// The file, with a feature of a kind this build doesn't have between the sketch and the pad.
fn with_unknown_feature() -> Value {
    let mut file = saved(&design());
    file["format"] = json!(newer());
    file["format_pending"] = json!(["a-later-change"]);
    let future = json!({"id": 900, "name": "Lattice1", "type": "gyroid_lattice", "cell": 2.5, "nested": {"big": 1e300}, "suppressed": false});
    file["features"].as_array_mut().unwrap().insert(1, future);
    file
}

#[test]
fn an_unknown_feature_kind_opens_the_rest_and_reports_that_feature() {
    let file = with_unknown_feature();
    let opened = Document::from_json(&file.to_string()).unwrap();
    let report = opened.newer_file.as_ref().unwrap();
    assert!(report.not_understood.iter().any(|what| what.contains("Lattice1") && what.contains("gyroid_lattice")), "{:?}", report.not_understood);
    assert_eq!(opened.features.len(), 3);
    assert!(matches!(&opened.features[1].kind, FeatureKind::NewerFileFeature { raw } if raw.kind() == "gyroid_lattice"));
    let (bodies, errors) = built(&opened);
    assert_eq!(bodies.len(), 1, "the pad after it still builds");
    assert!(errors[0].is_none() && errors[2].is_none(), "{errors:?}");
    let why = errors[1].as_deref().unwrap();
    assert!(why.contains("newer SolidCraft") && why.contains("gyroid_lattice"), "{why}");
}

#[test]
fn saving_keeps_an_unknown_feature_as_the_file_had_it_and_the_files_number() {
    let file = with_unknown_feature();
    let opened = Document::from_json(&file.to_string()).unwrap();
    let again = saved(&opened);
    assert_eq!(again["format"], file["format"], "never a lower number");
    assert_eq!(again["format_pending"], file["format_pending"]);
    assert_eq!(again["features"][1], file["features"][1], "the feature as the file had it");
    assert_eq!(
        again["features"].as_array().unwrap().iter().map(|f| f["type"].clone()).collect::<Vec<_>>(),
        [json!("sketch"), json!("gyroid_lattice"), json!("extrude")]
    );
    assert_eq!(Document::from_json(&opened.to_json()).unwrap(), opened, "saved and read again, the same");
    // A rename here goes with it; the rest stays.
    let mut renamed = opened.clone();
    renamed.features[1].name = "Infill".into();
    let again = saved(&renamed);
    assert_eq!(again["features"][1]["name"], "Infill");
    assert_eq!(again["features"][1]["nested"], json!({"big": 1e300}));
}

#[test]
fn saving_keeps_fields_and_sections_this_version_doesnt_know() {
    let mut file = saved(&design());
    file["format"] = json!(newer());
    file["future_section"] = json!({"anything": [1, 2, 3]});
    file["features"][1]["draft_v2"] = json!({"angle": 3.0});
    file["params"][0]["future_note"] = json!("keep me");
    let opened = Document::from_json(&file.to_string()).unwrap();
    assert_eq!(built(&opened).0.len(), 1);
    let report = opened.newer_file.as_ref().unwrap();
    for field in ["future_section", "draft_v2", "future_note"] {
        assert!(report.not_understood.iter().any(|what| what.contains(field)), "{field}: {:?}", report.not_understood);
    }
    let again = saved(&opened);
    assert_eq!(again["future_section"], file["future_section"]);
    assert_eq!(again["features"][1]["draft_v2"], file["features"][1]["draft_v2"]);
    assert_eq!(again["params"][0]["future_note"], "keep me");
    // They follow their records when the design changes around them.
    let mut changed = opened.clone();
    let id = changed.features[1].id;
    changed.features.insert(0, changed.features[0].clone());
    changed.features[0].id = 777;
    assert_eq!(changed.feature(id).unwrap().name, "Pad");
    let again = saved(&changed);
    assert_eq!(again["features"][2]["draft_v2"], file["features"][1]["draft_v2"]);
    assert!(crate::newer_file::not_kept(&changed).is_empty());
}

#[test]
fn a_section_that_doesnt_read_is_left_out_named_and_kept() {
    let mut file = saved(&design());
    file["format"] = json!(newer());
    file["configs"] = json!({"columns": [{"unknown_future_column": "Body"}], "rules": {"unknown_future_rule": true}});
    let opened = Document::from_json(&file.to_string()).unwrap();
    assert_eq!(built(&opened).0.len(), 1);
    let report = opened.newer_file.as_ref().unwrap();
    assert!(report.not_understood.iter().any(|what| what.contains("configs")), "{:?}", report.not_understood);
    assert_eq!(saved(&opened)["configs"], file["configs"]);
    assert!(crate::newer_file::not_kept(&opened).is_empty());
}

#[test]
fn a_parameter_that_doesnt_read_is_left_out_named_and_kept() {
    let mut file = saved(&design());
    file["format"] = json!(newer());
    file["params"].as_array_mut().unwrap().push(json!({"name": "future", "expression": {"kind": "table"}}));
    let opened = Document::from_json(&file.to_string()).unwrap();
    assert_eq!(built(&opened).0.len(), 1);
    assert!(opened.newer_file.as_ref().unwrap().not_understood.iter().any(|what| what.contains("`future`")));
    let names: Vec<_> = saved(&opened)["params"].as_array().unwrap().iter().map(|p| p["name"].clone()).collect();
    assert!(names.contains(&json!("future")) && names.contains(&json!("depth")), "{names:?}");
}

#[test]
fn what_cant_go_back_after_an_edit_is_said() {
    let mut file = saved(&design());
    file["format"] = json!(newer());
    file["configs"] = json!({"columns": [{"unknown_future_column": "Body"}]});
    let opened = Document::from_json(&file.to_string()).unwrap();
    assert!(crate::newer_file::not_kept(&opened).is_empty());
    let mut edited = opened.clone();
    edited.favorites.insert("depth".into());
    assert!(crate::newer_file::not_kept(&edited).is_empty(), "an edit elsewhere costs nothing");
    let baseline: Value = saved(&edited);
    assert_eq!(baseline["configs"], file["configs"]);
}

#[test]
fn a_feature_this_version_reads_wrongly_is_kept_whole() {
    // A kind this build has, in a shape it doesn't: the feature can't read, and is not guessed at.
    let mut file = saved(&design());
    file["format"] = json!(newer());
    file["features"][1]["operation"] = json!("weave");
    let opened = Document::from_json(&file.to_string()).unwrap();
    assert!(matches!(&opened.features[1].kind, FeatureKind::NewerFileFeature { raw } if raw.kind() == "extrude"));
    assert!(built(&opened).1[1].as_deref().unwrap().contains("newer SolidCraft"));
    assert_eq!(saved(&opened)["features"][1], file["features"][1]);
}

#[test]
fn what_truly_cant_be_read_says_why() {
    for (text, says) in [
        ("not json", "HDR resource admission"),
        ("{\"format\":\"solidcraft/0\",\"name\":\"x\"}", "format 0"),
        ("{\"format\":\"someone-else/9\"}", "not a SolidCraft design (format `someone-else/9`)"),
    ] {
        let why = Document::from_json(text).unwrap_err().to_string();
        assert!(why.contains(says), "{text}: {why}");
    }
    // Two features of one id are damage whatever build saved them.
    let mut file = saved(&design());
    file["format"] = json!(newer());
    let same = file["features"][0].clone();
    file["features"].as_array_mut().unwrap().push(same);
    let why = Document::from_json(&file.to_string()).unwrap_err().to_string();
    assert!(why.contains(&format!("format {}", FORMAT + 1)) && why.contains("can't read it") && why.contains("damaged"), "{why}");
}

#[test]
fn a_file_this_build_fully_knows_is_not_flagged_and_damage_is_still_refused() {
    assert!(Document::from_json(&design().to_json()).unwrap().newer_file.is_none());
    assert_eq!(DESIGN.newer(FORMAT, None), None);
    let mut file = saved(&design());
    file["features"][1]["type"] = json!("gyroid_lattice");
    assert!(Document::from_json(&file.to_string()).is_err(), "of this build's own format, that is damage");
}
