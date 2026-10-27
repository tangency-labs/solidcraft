use std::f64::consts::{FRAC_PI_2, TAU};
use std::io::Read;

use solidcraft_geom::paper::{Dashes, Group, Item, ObjectKind, Scene};
use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

use super::*;

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn scene(size: Vec2, items: Vec<Item>) -> Scene {
    Scene { size_mm: size, groups: vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items }] }
}

fn stroke(path: Vec<Seg2>, closed: bool, width_mm: f64, pattern: Option<Dashes>) -> Item {
    Item::Stroke { path, closed, width_mm, pattern }
}

fn pdf(scene: &Scene, options: &PdfOptions) -> Vec<u8> {
    paper_pdf(&scene.prepare().unwrap(), options).unwrap().0
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The page's content stream, inflated.
fn content(bytes: &[u8]) -> String {
    let at = bytes.windows(7).position(|w| w == b"stream\n").unwrap() + 7;
    let end = at + bytes[at..].windows(10).position(|w| w == b"endstream\n" || w == b"\nendstream").unwrap();
    let mut out = String::new();
    flate2::read::ZlibDecoder::new(&bytes[at..end]).read_to_string(&mut out).unwrap();
    out
}

/// The content's operators, each with its operands, numbers read as numbers.
fn ops(content: &str) -> Vec<(String, Vec<f64>)> {
    let spaced = content.replace('[', " [ ").replace(']', " ] ");
    let mut ops = Vec::new();
    let mut operands = Vec::new();
    for token in spaced.split_whitespace() {
        match token.parse::<f64>() {
            Ok(n) => operands.push(n),
            Err(_) if token == "[" || token == "]" => {}
            Err(_) => ops.push((token.to_string(), std::mem::take(&mut operands))),
        }
    }
    ops
}

/// Each cubic in the content as its four points, in millimetres.
fn cubics(content: &str) -> Vec<[Vec2; 4]> {
    let (mut at, mut out) = (p(0.0, 0.0), Vec::new());
    for (op, n) in ops(content) {
        match op.as_str() {
            "m" | "l" => at = p(n[0], n[1]),
            "c" => {
                out.push([at, p(n[0], n[1]), p(n[2], n[3]), p(n[4], n[5])]);
                at = p(n[4], n[5]);
            }
            _ => {}
        }
    }
    out
}

fn bezier([p0, p1, p2, p3]: [Vec2; 4], t: f64) -> Vec2 {
    let s = 1.0 - t;
    p0 * (s * s * s) + p1 * (3.0 * s * s * t) + p2 * (3.0 * s * t * t) + p3 * (t * t * t)
}

const NONE: PdfOptions = PdfOptions { title: None, output_time: None };

/// An A3 sheet's page is A3 in points, and the PDF says who made it.
#[test]
fn an_a3_sheet_is_its_size_in_points() {
    let bytes = pdf(&scene(p(420.0, 297.0), vec![]), &NONE);
    let file = text(&bytes);
    assert!(file.starts_with("%PDF-"), "{}", &file[..20]);
    assert!(file.contains("/MediaBox [0 0 1190.5511 841.8898]"), "{file}");
    assert!(file.contains(&format!("/Producer (SolidCraft {})", env!("CARGO_PKG_VERSION"))), "{file}");
    assert!(!file.contains("/Title"), "no title asked for: {file}");
}

/// A 100 mm edge at 1:2 is a 50 mm line on paper: 141.732 pt across the page.
#[test]
fn fifty_millimetres_on_paper_are_141_732_points() {
    let line = scene(p(210.0, 297.0), vec![stroke(vec![Seg2::Line { a: p(10.0, 20.0), b: p(60.0, 20.0) }], false, 0.5, None)]);
    let ops = ops(&content(&pdf(&line, &NONE)));
    let cm = ops.iter().find(|(op, _)| op == "cm").unwrap();
    assert_eq!(cm.1[..4], [cm.1[0], 0.0, 0.0, cm.1[0]], "one scale, millimetres to points");
    let ends: Vec<f64> = ops.iter().filter(|(op, _)| op == "m" || op == "l").map(|(_, n)| n[0]).collect();
    assert_eq!(ends, [10.0, 60.0]);
    assert!(((ends[1] - ends[0]) * cm.1[0] - 141.732).abs() < 1e-3, "{}", (ends[1] - ends[0]) * cm.1[0]);
}

/// The same scene gives the same bytes; the dates come only from `output_time`, in UTC.
#[test]
fn the_same_scene_gives_the_same_bytes_and_dates_come_only_from_output_time() {
    let sheet = scene(p(210.0, 297.0), vec![stroke(vec![Seg2::Line { a: p(10.0, 20.0), b: p(60.0, 20.0) }], false, 0.5, None)]);
    assert_eq!(pdf(&sheet, &NONE), pdf(&sheet, &NONE));
    assert!(!text(&pdf(&sheet, &NONE)).contains("Date"));
    let leap_day = PdfOptions { title: Some("Sheet 1".into()), output_time: Some(951_782_400) };
    let file = text(&pdf(&sheet, &leap_day));
    assert!(file.contains("/CreationDate (D:20000229000000Z)") && file.contains("/ModDate (D:20000229000000Z)"), "{file}");
    assert!(file.contains("/Title (Sheet 1)"), "{file}");
    assert_eq!(pdf(&sheet, &leap_day), pdf(&sheet, &leap_day));
    let last = PdfOptions { title: Some(String::new()), output_time: Some(MAX_OUTPUT_TIME) };
    let file = text(&pdf(&sheet, &last));
    assert!(file.contains("(D:99991231235959Z)") && !file.contains("/Title"), "{file}");
}

/// The trailer's `startxref` points at the table, and each entry at its `N 0 obj`.
#[test]
fn every_cross_reference_points_at_its_object() {
    let sheet = scene(p(420.0, 297.0), vec![stroke(Loop2::circle(p(100.0, 100.0), 40.0).segs, true, 0.7, None)]);
    let bytes = pdf(&sheet, &PdfOptions { title: Some("Ç".into()), output_time: Some(0) });
    let file = text(&bytes);
    let start: usize = file.rsplit("startxref").next().unwrap().split_whitespace().next().unwrap().parse().unwrap();
    assert!(file[start..].starts_with("xref\n0 6\n"), "{}", &file[start..start + 20]);
    let entries = &file[start + "xref\n0 6\n".len()..];
    for (n, entry) in entries.as_bytes().chunks(20).take(6).enumerate().skip(1) {
        let entry = std::str::from_utf8(entry).unwrap();
        let offset: usize = entry[..10].parse().unwrap();
        assert!(entry[10..].starts_with(" 00000 n"), "{entry:?}");
        assert!(file[offset..].starts_with(&format!("{n} 0 obj")), "object {n} at {offset}");
    }
}

/// Arcs are cubics within a ten-thousandth of a millimetre, from a 0.5 mm radius to 2 m, and a
/// parabola is its cubic; every number has at most five decimals.
#[test]
fn arcs_are_cubics_within_a_ten_thousandth_of_a_millimetre() {
    for (radius, sweep) in [(0.5, TAU), (40.0, TAU), (300.0, 2.5), (2000.0, 0.3)] {
        let c = p(2500.0, 2500.0);
        let arc = scene(p(5000.0, 5000.0), vec![stroke(vec![Seg2::Arc { center: c, radius, start: 0.4, sweep }], false, 0.35, None)]);
        let content = content(&pdf(&arc, &NONE));
        let cubics = cubics(&content);
        assert!(cubics.len() >= (sweep / FRAC_PI_2).ceil() as usize, "{radius}: {}", cubics.len());
        for cubic in &cubics {
            for k in 0..=64 {
                let off = (bezier(*cubic, f64::from(k) / 64.0).dist(c) - radius).abs();
                assert!(off <= ARC_TOLERANCE_MM, "r {radius}: {off} mm off at {k}/64");
            }
        }
        assert!(content.split_whitespace().all(|t| t.split('.').nth(1).is_none_or(|d| d.len() <= 5)), "{content}");
    }
    let parabola = scene(
        p(100.0, 100.0),
        vec![stroke(vec![Seg2::Conic { a: p(10.0, 10.0), apex: p(30.0, 50.0), b: p(50.0, 10.0), w: 1.0 }], false, 0.35, None)],
    );
    let cubic = cubics(&content(&pdf(&parabola, &NONE)))[0];
    for k in 0..=16 {
        let t = f64::from(k) / 16.0;
        let s = 1.0 - t;
        let on = p(10.0, 10.0) * (s * s) + p(30.0, 50.0) * (2.0 * s * t) + p(50.0, 10.0) * (t * t);
        assert!(bezier(cubic, t).dist(on) <= 1e-5, "{t}");
    }
}

/// Strokes and fills are painted as SVG and PNG paint them: butt caps, miter joins, a miter limit
/// of 4, each width and dash pattern set once until it changes, a closed outline closed, fills
/// even-odd.
#[test]
fn strokes_and_fills_are_painted_as_the_other_writers_paint_them() {
    let dashed = Some(Dashes { lengths_mm: vec![4.2, 1.05], phase_mm: 0.5 });
    let line = |y: f64| vec![Seg2::Line { a: p(10.0, y), b: p(100.0, y) }];
    let sheet = scene(
        p(210.0, 297.0),
        vec![
            stroke(line(10.0), false, 0.35, None),
            stroke(line(20.0), false, 0.35, dashed.clone()),
            stroke(line(30.0), false, 0.35, dashed),
            stroke(line(40.0), false, 0.7, None),
            stroke(Loop2::polygon(&[p(10.0, 50.0), p(50.0, 50.0), p(50.0, 90.0)]).segs, true, 0.7, None),
            Item::Fill { region: Region2 { outer: Loop2::circle(p(150.0, 150.0), 20.0), holes: vec![Loop2::circle(p(150.0, 150.0), 10.0)] } },
        ],
    );
    let content = content(&pdf(&sheet, &NONE));
    assert!(content.starts_with("q 2.834645669 0 0 2.834645669 0 0 cm 0 J 0 j 4 M\n") && content.ends_with("Q\n"), "{content}");
    let ops = ops(&content);
    let set: Vec<(&str, &[f64])> = ops.iter().filter(|(op, _)| op == "w" || op == "d").map(|(op, n)| (op.as_str(), n.as_slice())).collect();
    assert_eq!(set, [("w", &[0.35][..]), ("d", &[4.2, 1.05, 0.5][..]), ("w", &[0.7][..]), ("d", &[0.0][..])], "{content}");
    let paints: Vec<&str> = ops.iter().map(|(op, _)| op.as_str()).filter(|op| ["S", "s", "f*", "h"].contains(op)).collect();
    assert_eq!(paints, ["S", "S", "S", "S", "s", "h", "h", "f*"], "{content}");
}

/// A page under 3 pt a side, a title over 1,000 characters and a date past the year 9999 are
/// refused.
#[test]
fn what_cannot_be_written_is_refused() {
    let tiny = scene(p(1.0, 297.0), vec![]);
    let e = paper_pdf(&tiny.prepare().unwrap(), &NONE).unwrap_err();
    assert!(e.to_string().contains("at least 3 pt"), "{e}");
    let sheet = scene(p(210.0, 297.0), vec![]);
    let prepared = sheet.prepare().unwrap();
    assert!(paper_pdf(&prepared, &PdfOptions { title: Some("x".repeat(1000)), output_time: None }).is_ok());
    let e = paper_pdf(&prepared, &PdfOptions { title: Some("x".repeat(1001)), output_time: None }).unwrap_err();
    assert!(e.to_string().contains("1000 characters"), "{e}");
    let e = paper_pdf(&prepared, &PdfOptions { title: None, output_time: Some(MAX_OUTPUT_TIME + 1) }).unwrap_err();
    assert!(e.to_string().contains("9999"), "{e}");
}
