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
        label: None,
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

fn with_symbols_and_centres() -> (Drawing, [u64; 3]) {
    use crate::drawing::centre_line::CentreLine;
    use crate::drawing::dimension::ModelPoint;
    use crate::drawing::symbol::{Piece, Symbol};
    use solidcore_record::{Seg2, Vec2};
    use solidcraft_geom::paper::line::LineKind;

    let (mut d, mark) = marked();
    let mut ids = IdSource::seeded(13064);
    let line = d.allocate_id(&mut ids).unwrap();
    let point = |name: &str| ModelPoint { body: "Part".into(), origin_feature: 1, edges: vec![name.into()], centre: true, path: vec![7] };
    d.add_centre_lines([Arc::new(CentreLine { id: line, view: d.views[0].id, a: point("rim-a"), b: point("rim-b"), overshoot_mm: 2.0 })]);
    let symbol = d.allocate_id(&mut ids).unwrap();
    d.symbols.add([Arc::new(Symbol {
        id: symbol,
        name: "Target".into(),
        pieces: vec![Piece::Stroke { path: vec![Seg2::Line { a: Vec2::ZERO, b: Vec2::new(2.0, 0.0) }], closed: false, line: LineKind::Visible }],
    })]);
    (d, [mark, line, symbol])
}

#[test]
fn symbols_and_centre_annotations_keep_distinct_saved_ids() {
    let (mut d, [mark, line, _]) = with_symbols_and_centres();
    d.validate().unwrap();
    assert_eq!(Drawing::from_json(&d.to_json().unwrap()).unwrap(), d);
    for id in [mark, line] {
        let mut invalid = d.clone();
        Arc::make_mut(&mut invalid.symbols[0]).id = id;
        assert!(invalid.validate().is_err(), "a symbol and centre annotation cannot share an id");
    }
    let symbols = d.symbols.clone();
    d.remove_sheet(d.sheets[0].id);
    assert!(d.centre_marks.is_empty());
    assert!(d.centre_lines.is_empty());
    assert_eq!(d.symbols, symbols, "drawing-level definitions remain after sheet removal");
}

#[test]
fn known_annotations_remain_editable_in_a_newer_drawing() {
    use crate::format::DRAWING;
    use serde_json::{Value, json};

    let (d, _) = with_symbols_and_centres();
    let mut file: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
    file["format"] = json!(DRAWING.format_string(DRAWING.current() + 1));
    file["future_annotations"] = json!({"policy": "keep"});
    let mut opened = Drawing::from_value(file.clone()).unwrap();
    assert_eq!(opened.centre_marks, d.centre_marks);
    assert_eq!(opened.centre_lines, d.centre_lines);
    assert_eq!(opened.symbols, d.symbols);
    assert_eq!(opened.newer_file.as_ref().unwrap().not_understood, ["field `future_annotations`"]);

    Arc::make_mut(&mut opened.centre_marks[0]).overshoot_mm = 7.0;
    opened.centre_lines.clear();
    Arc::make_mut(&mut opened.symbols[0]).name = "Edited target".into();
    let saved = opened.to_json().unwrap();
    let reopened = Drawing::from_json(&saved).unwrap();
    assert_eq!(reopened.centre_marks, opened.centre_marks);
    assert!(reopened.centre_lines.is_empty());
    assert_eq!(reopened.symbols, opened.symbols);
    let saved: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved["future_annotations"], file["future_annotations"]);
    assert_eq!(saved["format"], file["format"]);
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
