use std::sync::Arc;

use super::*;
use solidcore_record::Vec3;

use crate::*;

/// A box, a cylinder through it, and the cut of one by the other: features 1, 2 and 3.
fn plate_and_pin() -> Document {
    let mut doc = Document::new("p");
    doc.add_feature(
        FeatureKind::Box {
            corner: point_expr(Vec3::ZERO),
            length: "10".into(),
            width: "10".into(),
            height: "10".into(),
            operation: Operation::NewBody,
            centred: false,
        },
        None,
    )
    .unwrap();
    doc.add_feature(
        FeatureKind::Cylinder {
            base: point_expr(Vec3::new(5.0, 5.0, 0.0)),
            axis: Dir(Vec3::Z),
            radius: "2".into(),
            height: "20".into(),
            operation: Operation::NewBody,
            references: Vec::new(),
        },
        None,
    )
    .unwrap();
    doc.add_feature(FeatureKind::Combine { target: "Body1".into(), tools: vec!["Body2".into()], operation: Operation::Cut, keep_tools: false }, None)
        .unwrap();
    doc
}

fn ids(doc: &Document) -> [u64; 3] {
    let id = |i: usize| doc.features[i].id;
    [id(0), id(1), id(2)]
}

fn evaluated(doc: &Document) -> Model {
    let mut m = Model::new();
    m.evaluate(doc);
    m
}

fn volume(m: &Model) -> f64 {
    let st = m.state();
    assert_eq!(st.bodies.len(), 1);
    solidcraft_kernel::measure(&st.bodies[0].body).unwrap().volume
}

fn reused(before: &Model, after: &Model, i: usize) -> bool {
    Arc::ptr_eq(&before.results[i].state, &after.results[i].state)
}

fn dial(d: f64) -> RigorSetting {
    RigorSetting { dial: Some(d), ..RigorSetting::default() }
}

/// A design that records no rigor builds every operation with the legacy rigor, saves no
/// `rigor`, and fingerprints as before: setting the legacy rigor as its default changes nothing
/// and the next evaluation reuses every result.
#[test]
fn a_design_with_no_rigor_builds_as_before() {
    let mut doc = plate_and_pin();
    assert!(doc.rigor.is_empty());
    for id in ids(&doc) {
        assert!(doc.rigor.built(id).is_legacy() && doc.rigor.inherits(id) && doc.rigor.digest(id).is_none());
    }
    assert!(!doc.to_json().contains("\"rigor\""), "no rigor saved");
    let mut m = evaluated(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    let v = volume(&m);
    assert!((v - (1000.0 - std::f64::consts::PI * 40.0)).abs() < 1e-3, "{v}");
    let before = m.clone();
    assert_eq!(doc.set_rigor_default(Some(RigorSetting::default())).unwrap(), Vec::<u64>::new());
    m.evaluate(&doc);
    for i in 0..3 {
        assert!(reused(&before, &m, i), "feature {i} built again at the legacy rigor");
    }
}

/// An inheriting operation builds again when the default changes; an overriding one keeps its
/// rigor and its result.
#[test]
fn a_default_change_rebuilds_only_what_inherits_it() {
    let mut doc = plate_and_pin();
    let [plate, pin, cut] = ids(&doc);
    doc.set_rigor_override(plate, Some(dial(0.0))).unwrap();
    let pinned = doc.rigor.ops[&plate].clone();
    let mut m = evaluated(&doc);
    let before = m.clone();

    let changed = doc.set_rigor_default(Some(dial(1.0))).unwrap();
    assert_eq!(changed, vec![pin, cut], "the inheriting features");
    assert_eq!(doc.rigor.ops[&plate], pinned, "the override is kept as it was");
    assert_eq!(doc.rigor.built(pin), RigorSetting { dial: Some(1.0), ..Default::default() }.resolve().unwrap());
    assert_eq!(doc.rigor.built(pin).certify, Certify::Proved);

    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    assert!(reused(&before, &m, 0), "the overriding plate is not built again");
    assert!(!reused(&before, &m, 1), "the inheriting pin is built again");
    assert!(!reused(&before, &m, 2), "so is the cut after it");
    // a proved cut of a sound plate is the same solid
    assert!((volume(&before) - volume(&m)).abs() < 1e-9);

    // back to inheriting: the plate follows the default now
    doc.set_rigor_override(plate, None).unwrap();
    assert_eq!(doc.rigor.built(plate), doc.rigor.default_built());
}

/// The records save and read back to the bit, `built` and all.
#[test]
fn rigor_records_round_trip() {
    let mut doc = plate_and_pin();
    let [_, pin, _] = ids(&doc);
    doc.set_rigor_default(Some(dial(0.6))).unwrap();
    doc.set_rigor_override(pin, Some(RigorSetting { dial: Some(0.3), time_ms: Some(1500), certify: Some(Certify::Proved), ..Default::default() }))
        .unwrap();
    let back = Document::from_json(&doc.to_json()).unwrap();
    assert_eq!(back.rigor, doc.rigor);
    let b = back.rigor.built(pin);
    assert_eq!((b.certify, b.time_ms, b.tolerance), (Certify::Proved, Some(1500), Some(5e-5)));
}

/// A saved `built` is what the operation is built with, whatever its `set` resolves to now:
/// a later kernel that maps the dial differently does not change a saved design.
#[test]
fn the_saved_built_value_is_what_builds() {
    let mut doc = plate_and_pin();
    let [_, pin, _] = ids(&doc);
    doc.set_rigor_override(pin, Some(dial(0.5))).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&doc.to_json()).unwrap();
    // as a design saved by a kernel whose 0.5 stop had other knobs
    json["rigor"]["ops"][pin.to_string()]["built"]["samples"] = 7.into();
    let back = Document::from_json(&json.to_string()).unwrap();
    assert_eq!(back.rigor.built(pin).samples, Some(7));
}

/// An operation that runs out of its time budget fails with a statement that it did, and no
/// result: never another one.
#[test]
fn out_of_time_fails_the_operation_and_says_so() {
    let mut doc = plate_and_pin();
    let [_, _, cut] = ids(&doc);
    doc.set_rigor_override(cut, Some(RigorSetting { time_ms: Some(0), ..Default::default() })).unwrap();
    let m = evaluated(&doc);
    assert!(m.results[0].error.is_none() && m.results[1].error.is_none());
    let e = m.results[2].error.as_deref().unwrap_or_default();
    assert!(e.contains("ran out of time"), "{e}");
}

#[test]
fn a_rigor_no_operation_runs_with_is_refused() {
    let mut doc = plate_and_pin();
    let [plate, ..] = ids(&doc);
    for bad in [
        RigorSetting { dial: Some(1.5), ..Default::default() },
        RigorSetting { dial: Some(f64::NAN), ..Default::default() },
        RigorSetting { tolerance: Some(-1.0), ..Default::default() },
        RigorSetting { samples: Some(0), ..Default::default() },
    ] {
        assert!(doc.set_rigor_override(plate, Some(bad.clone())).is_err(), "{bad:?}");
        assert!(doc.set_rigor_default(Some(bad)).is_err());
    }
    assert!(doc.rigor.is_empty(), "nothing set by a refusal");
    assert!(doc.set_rigor_override(999, Some(dial(0.5))).is_err(), "no such feature");
}
