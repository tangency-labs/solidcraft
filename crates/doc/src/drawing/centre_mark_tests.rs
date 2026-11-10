use std::collections::BTreeSet;

use super::*;
use crate::drawing::Source;
use crate::drawing::view::{View, ViewKind, ViewPlace};
use crate::drawing_view::{ViewDirection, ViewOverrides};
use crate::ids::IdSource;
use solidcraft_geom::drawing_sheet::{Orientation, PaperSize, Sheet};
use solidcraft_geom::drawing_view::NamedView;

fn marked() -> (Drawing, u64) {
    let mut ids = IdSource::seeded(9419);
    let sheet = Sheet::standard(PaperSize::A3, Orientation::Landscape);
    let mut d = Drawing::new("Part", "en", "Sheet 1", sheet, None, &BTreeSet::new(), &mut ids).unwrap();
    let source = d.allocate_id(&mut ids).unwrap();
    d.sources.push(Source { id: source });
    let view = d.allocate_id(&mut ids).unwrap();
    d.add_views([Arc::new(View {
        id: view,
        sheet: d.sheets[0].id,
        kind: ViewKind::Base { source, view: ViewDirection::Named(NamedView::Front) },
        place: ViewPlace::At([50.0, 50.0]),
        settings: ViewOverrides { scale: Some("1:1".parse().unwrap()), ..ViewOverrides::default() },
        reference_shown_percent: 0.0,
    })]);
    let id = d.allocate_id(&mut ids).unwrap();
    d.add_centre_marks([Arc::new(CentreMark {
        id,
        view,
        edge: ModelEdge { body: "Part".into(), origin_feature: 1, name: "rim".into(), path: vec![7] },
        overshoot_mm: 2.0,
    })]);
    (d, id)
}

#[test]
fn saves_and_reopens_intent_and_removes_it_with_its_sheet() {
    let (mut d, id) = marked();
    d.validate().unwrap();
    let json = d.to_json().unwrap();
    let reopened = Drawing::from_json(&json).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.centre_mark(id).unwrap().edge.path, [7]);
    let mut without_marks = d.clone();
    without_marks.centre_marks.clear();
    let old_style = without_marks.to_json().unwrap();
    assert!(!old_style.contains("centre_marks"));
    assert!(Drawing::from_json(&old_style).unwrap().centre_marks.is_empty());
    let sheet = d.sheets[0].id;
    assert_eq!(d.on_sheet(sheet).centre_marks, [id]);
    d.remove_sheet(sheet);
    assert!(d.centre_marks.is_empty());
}

#[test]
fn marks_read_in_id_order_and_unknown_fields_are_refused() {
    let (mut d, id) = marked();
    let mut ids = IdSource::seeded(9420);
    let other = d.allocate_id(&mut ids).unwrap();
    let mut mark = d.centre_mark(id).unwrap().clone();
    mark.id = other;
    d.add_centre_marks([Arc::new(mark)]);
    let mut value: serde_json::Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
    value["centre_marks"].as_array_mut().unwrap().reverse();
    assert_eq!(Drawing::from_value(value.clone()).unwrap(), d);
    value["centre_marks"][0]["extra"] = serde_json::json!(true);
    assert!(Drawing::from_value(value).is_err());
}

#[test]
fn rejects_invalid_values_missing_views_and_duplicate_ids() {
    let (d, id) = marked();
    for value in [-1.0, f64::NAN, f64::INFINITY, MAX_SIDE_MM + 1.0] {
        let mut bad = d.clone();
        Arc::make_mut(&mut bad.centre_marks[0]).overshoot_mm = value;
        assert!(bad.validate().is_err());
    }
    let mut bad = d.clone();
    Arc::make_mut(&mut bad.centre_marks[0]).view = 999;
    assert!(bad.validate().is_err());
    let mut bad = d.clone();
    let view_id = bad.views[0].id;
    Arc::make_mut(&mut bad.centre_marks[0]).id = view_id;
    assert!(bad.validate().is_err());
    assert_eq!(d.centre_mark(id).unwrap().overshoot_mm, 2.0);
}

#[test]
fn centre_line_records_round_trip_and_follow_sheet_lifecycle() {
    use crate::drawing::centre_line::CentreLine;
    use crate::drawing::dimension::ModelPoint;
    let (mut d, _) = marked();
    let mut ids = IdSource::seeded(12345);
    let id = d.allocate_id(&mut ids).unwrap();
    let point = |name: &str| ModelPoint { body: "Part".into(), origin_feature: 1, edges: vec![name.into()], centre: true, path: vec![7] };
    d.add_centre_lines([Arc::new(CentreLine { id, view: d.views[0].id, a: point("rim-a"), b: point("rim-b"), overshoot_mm: 2.0 })]);
    d.validate().unwrap();
    let reopened = Drawing::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(reopened, d);
    assert_eq!(reopened.centre_line(id).unwrap().a.path, [7]);
    let sheet = d.sheets[0].id;
    assert_eq!(d.on_sheet(sheet).centre_lines, [id]);
    let mut invalid = d.clone();
    Arc::make_mut(&mut invalid.centre_lines[0]).overshoot_mm = f64::NAN;
    assert!(invalid.validate().is_err());
    let mut invalid = d.clone();
    Arc::make_mut(&mut invalid.centre_lines[0]).view = 999;
    assert!(invalid.validate().is_err());
    let mut invalid = d.clone();
    Arc::make_mut(&mut invalid.centre_lines[0]).a.edges.clear();
    let error = invalid.validate().unwrap_err().to_string();
    assert!(error.contains(&format!("centre line {}", ids::encode(id))), "{error}");
    d.remove_sheet(sheet);
    assert!(d.centre_lines.is_empty());
}
