use solidcore_record::{Seg2, Vec2};

use super::*;

fn line(a: [f64; 2], b: [f64; 2]) -> Seg2 {
    Seg2::Line { a: Vec2::from(a), b: Vec2::from(b) }
}

fn stroke(path: Vec<Seg2>) -> FormatStroke {
    FormatStroke { path, closed: false, width_mm: None }
}

fn text(at: [f64; 2], text: &str, cap_mm: f64) -> FormatText {
    FormatText { at, text: text.into(), cap_mm, angle_deg: 0.0 }
}

fn refused(format: SheetFormat, why: &str) {
    let e = format.check().unwrap_err().to_string();
    assert!(e.contains(why), "{e}");
}

/// A format of lines, arcs and text on paper is kept, saved as JSON and read back the same; an
/// empty one is no format.
#[test]
fn a_format_on_paper_is_kept_and_reads_back() {
    let arc = Seg2::Arc { center: Vec2::new(100.0, 50.0), radius: 5.0, start: 0.0, sweep: std::f64::consts::TAU };
    let format = SheetFormat {
        strokes: vec![stroke(vec![line([10.0, 10.0], [410.0, 10.0]), line([410.0, 10.0], [410.0, 287.0])]), stroke(vec![arc])],
        texts: vec![text([250.0, 20.0], "ACME LTD", 5.0)],
    };
    format.check().unwrap();
    let back: SheetFormat = serde_json::from_str(&serde_json::to_string(&format).unwrap()).unwrap();
    assert_eq!(back, format);
    assert!(SheetFormat::default().is_empty());
    assert!(serde_json::from_str::<SheetFormat>(r#"{"strokes": [], "colour": "red"}"#).is_err(), "unknown fields are refused");
}

/// What an imported file can get wrong is refused, naming it: points off any paper, numbers that
/// aren't, an empty or broken arc, widths and text sizes no sheet prints, long or empty text, and
/// more than the caps.
#[test]
fn a_hostile_format_is_refused() {
    let one = |s: FormatStroke| SheetFormat { strokes: vec![s], texts: Vec::new() };
    refused(one(stroke(Vec::new())), "no pieces");
    for bad in [f64::NAN, f64::INFINITY, 1e9, -6000.0] {
        refused(one(stroke(vec![line([0.0, 0.0], [bad, 1.0])])), "lies past");
    }
    for (radius, sweep) in [(0.0, 1.0), (-1.0, 1.0), (1.0, 0.0), (1.0, 7.0), (f64::NAN, 1.0)] {
        refused(one(stroke(vec![Seg2::Arc { center: Vec2::ZERO, radius, start: 0.0, sweep }])), "lies past");
    }
    refused(one(FormatStroke { width_mm: Some(50.0), ..stroke(vec![line([0.0, 0.0], [1.0, 1.0])]) }), "width");
    let texts = |t: FormatText| SheetFormat { strokes: Vec::new(), texts: vec![t] };
    refused(texts(text([0.0, 0.0], "  ", 5.0)), "1 to 500 characters");
    refused(texts(text([0.0, 0.0], "two\nlines", 5.0)), "one line");
    refused(texts(text([0.0, 0.0], &"x".repeat(501), 5.0)), "1 to 500 characters");
    refused(texts(text([0.0, 0.0], "TITLE", 0.1)), "mm high");
    refused(texts(text([f64::NAN, 0.0], "TITLE", 5.0)), "sits on paper");
    refused(texts(FormatText { angle_deg: f64::INFINITY, ..text([0.0, 0.0], "TITLE", 5.0) }), "turned");
    let many = SheetFormat { strokes: vec![stroke(vec![line([0.0, 0.0], [1.0, 1.0])]); MAX_STROKES + 1], texts: Vec::new() };
    refused(many, "strokes");
    let long = SheetFormat { strokes: vec![stroke(vec![line([0.0, 0.0], [1.0, 1.0]); MAX_PIECES + 1])], texts: Vec::new() };
    refused(long, "pieces");
}
