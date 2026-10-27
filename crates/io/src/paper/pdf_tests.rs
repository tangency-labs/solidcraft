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

/// Check this writer's single xref subsection using byte positions, never decoded text.
/// This intentionally checks our emitted layout, not arbitrary third-party PDFs.
fn check_xref(bytes: &[u8]) -> std::result::Result<usize, String> {
    let marker = b"startxref\n";
    let at = bytes.windows(marker.len()).rposition(|w| w == marker).ok_or("missing startxref")? + marker.len();
    let number = bytes.get(at..).ok_or("missing xref offset")?.split(|b| b.is_ascii_whitespace()).next().ok_or("missing xref offset")?;
    let start: usize = std::str::from_utf8(number).map_err(|_| "xref offset is not ASCII")?.parse().map_err(|_| "invalid xref offset")?;
    let tail = bytes.get(start..).ok_or("xref offset outside file")?;
    let table = tail.strip_prefix(b"xref\n0 6\n").ok_or("xref offset is not the expected subsection")?;
    let entries = table.get(..120).ok_or("truncated xref entries")?;
    for (n, entry) in entries.as_chunks::<20>().0.iter().enumerate() {
        if ![&b" \n"[..], &b"\r\n"[..]].contains(&&entry[18..]) {
            return Err(format!("entry {n} does not occupy 20 bytes"));
        }
        if n == 0 {
            if &entry[..18] != b"0000000000 65535 f" {
                return Err("invalid free entry".into());
            }
            continue;
        }
        if &entry[10..18] != b" 00000 n" {
            return Err(format!("object {n} is not in use"));
        }
        let offset: usize =
            std::str::from_utf8(&entry[..10]).map_err(|_| "object offset is not ASCII")?.parse().map_err(|_| "invalid object offset")?;
        let expected = format!("{n} 0 obj\n");
        if offset >= start || !bytes.get(offset..).is_some_and(|tail| tail.starts_with(expected.as_bytes())) {
            return Err(format!("object {n} does not start at byte {offset}"));
        }
    }
    Ok(start)
}

/// Geometry/paint use the 10 nm grid; the one unit transform has its own accuracy.
/// Expectations are independent of the writer's constants and formatting helper.
fn check_number_grid(content: &str) -> std::result::Result<(), String> {
    let spaced = content.replace('[', " [ ").replace(']', " ] ");
    let mut operands = Vec::new();
    let mut transforms = 0;
    for token in spaced.split_whitespace() {
        if let Ok(value) = token.parse::<f64>() {
            if !value.is_finite() || token.contains(['e', 'E']) || (value == 0.0 && token.starts_with('-')) {
                return Err(format!("invalid emitted number {token}"));
            }
            operands.push((token, value));
        } else if token != "[" && token != "]" {
            let decimals = if token == "cm" { 9 } else { 5 };
            if operands.iter().any(|(raw, _)| raw.split_once('.').is_some_and(|(_, fraction)| fraction.len() > decimals)) {
                return Err(format!("too many decimals for {token}: {operands:?}"));
            }
            if token == "cm" {
                transforms += 1;
                let values: Vec<_> = operands.iter().map(|(_, n)| *n).collect();
                let [sx, xy, yx, sy, tx, ty] = values.as_slice() else { return Err("cm needs six numbers".into()) };
                if sx != sy || [*xy, *yx, *tx, *ty] != [0.0; 4] {
                    return Err("the page transform must be one uniform scale, without a flip or translation".into());
                }
                // 72 / 25.4 = 360 / 127. Across 5,000 mm the written scale may
                // contribute at most 1e-6 mm; this does not certify a viewer or printer.
                if (sx * 127.0 / 360.0 - 1.0).abs() * 5000.0 > 1e-6 {
                    return Err("the written millimetre-to-point scale is insufficiently accurate".into());
                }
            }
            operands.clear();
        }
    }
    if transforms != 1 || !operands.is_empty() {
        return Err("expected exactly one complete page transform".into());
    }
    Ok(())
}

/// The trailer's `startxref` points at the table, and each entry at its `N 0 obj`.
#[test]
fn every_cross_reference_points_at_its_object() {
    let sheet = scene(p(420.0, 297.0), vec![stroke(Loop2::circle(p(100.0, 100.0), 40.0).segs, true, 0.7, None)]);
    for title in ["ASCII", "\u{00c7}", "\u{65e5}\u{672c}\u{1f642}", "(sheet 1) \\ title"] {
        let bytes = pdf(&sheet, &PdfOptions { title: Some(title.into()), output_time: Some(0) });
        assert!(check_xref(&bytes).is_ok(), "{title:?}: {:?}", check_xref(&bytes));
    }
}

#[test]
fn the_byte_offset_oracle_refuses_shifted_or_corrupted_references() {
    let bytes = pdf(&scene(p(420.0, 297.0), vec![]), &PdfOptions { title: Some("\u{00c7}".into()), output_time: None });
    let start = check_xref(&bytes).unwrap();
    let mut shifted = bytes.clone();
    shifted.insert(9, b' ');
    assert!(check_xref(&shifted).is_err(), "inserting a byte must invalidate the old offsets");
    let mut wrong = bytes;
    let first_object = start + b"xref\n0 6\n".len() + 20;
    wrong[first_object..first_object + 10].copy_from_slice(b"0000000001");
    assert!(check_xref(&wrong).is_err(), "an object offset into the header must fail");
}

#[test]
fn the_number_oracle_separates_page_scale_from_the_coordinate_grid() {
    let prefix = "q 2.834645669 0 0 2.834645669 0 0 cm ";
    let valid = format!("{prefix}0.35 w [4.2 1.05] 0.5 d 0 0 m 4999.12345 5000 l S Q");
    assert!(check_number_grid(&valid).is_ok());
    for bad in [
        valid.replace("4999.12345", "4999.123456"),
        valid.replace("0.35 w", "0.350001 w"),
        valid.replace("4.2 1.05", "4.200001 1.05"),
        valid.replace("2.834645669", "2.83465"),
        valid.replace("0.35 w", "3.5e-1 w"),
        valid.replace("0.35 w", "NaN w"),
        valid.replace("0 0 m", "-0 0 m"),
        format!("{prefix}{valid}"),
        "q 0 0 m 1 1 l S Q".into(),
    ] {
        assert!(check_number_grid(&bad).is_err(), "fault escaped the oracle: {bad}");
    }
}

/// Arcs are cubics within a ten-thousandth of a millimetre, from a 0.5 mm radius to 5 m, and a
/// parabola is its cubic; geometry/paint have five decimals, and the page scale is checked separately.
#[test]
fn arcs_are_cubics_within_a_ten_thousandth_of_a_millimetre() {
    for (c, radius, start, sweep) in [
        (p(2500.0, 2500.0), 0.5, 0.4, TAU),
        (p(2500.0, 2500.0), 40.0, 0.4, TAU),
        (p(2500.0, 2500.0), 300.0, 0.4, 2.5),
        (p(2500.0, 2500.0), 2000.0, 0.4, 0.3),
        (p(2500.0, 2500.0), 40.0, 0.4, -2.5),
        (p(0.0, 0.0), 5000.0, 0.0, FRAC_PI_2),
    ] {
        let arc = scene(p(5000.0, 5000.0), vec![stroke(vec![Seg2::Arc { center: c, radius, start, sweep }], false, 0.35, None)]);
        let content = content(&pdf(&arc, &NONE));
        let cubics = cubics(&content);
        assert!(cubics.len() >= (sweep.abs() / FRAC_PI_2).ceil() as usize, "{radius}: {}", cubics.len());
        assert!(cubics.first().unwrap()[0].dist(c + Vec2::from_angle(start) * radius) <= 1e-5);
        assert!(cubics.last().unwrap()[3].dist(c + Vec2::from_angle(start + sweep) * radius) <= 1e-5);
        for cubic in &cubics {
            for k in 0..=64 {
                let off = (bezier(*cubic, f64::from(k) / 64.0).dist(c) - radius).abs();
                assert!(off <= 1e-4, "r {radius}: {off} mm off at {k}/64");
            }
        }
        assert!(check_number_grid(&content).is_ok(), "{:?}: {content}", check_number_grid(&content));
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

// Small private limits exercise the real content writer without allocating a
// maximum-sized scene. They can tighten, never raise, the production ceiling.
#[test]
fn content_budget_includes_the_header_and_the_final_graphics_restore() {
    let expected = "q 2.834645669 0 0 2.834645669 0 0 cm 0 J 0 j 4 M\nQ\n";
    assert_eq!(super::content_with_limit(&[], expected.len()).unwrap(), expected);
    for limit in 0..expected.len() {
        assert!(super::content_with_limit(&[], limit).is_err(), "accepted {limit} bytes for an empty sheet's stream");
    }
}

#[test]
fn content_budget_stops_inside_one_long_path_before_its_paint_operator() {
    let path = GridPath {
        start: GridPoint { x: 0, y: 0 },
        pieces: (1..=128).map(|i| Piece::Line { to: GridPoint { x: i * 100_000, y: 0 } }).collect(),
        closed: false,
    };
    let mut writer = Content::new(96).unwrap();
    assert!(writer.path(&path).is_err(), "the path must stop before the whole item is built");
    assert!(writer.out.len() <= 96, "oversized partial content was allocated");
}

#[test]
fn content_budget_stops_inside_one_arcs_cubic_expansion() {
    // The arc is already in grid steps, as Piece::seg supplies it. A 5 m
    // quarter circle needs several cubics; even one input piece must be bounded.
    let mut writer = Content::new(64).unwrap();
    let arc = Seg2::Arc { center: Vec2::ZERO, radius: 500_000_000.0, start: 0.0, sweep: FRAC_PI_2 };
    assert!(writer.arc(arc, GridPoint { x: 0, y: 500_000_000 }).is_err());
    assert!(writer.out.len() <= 64);
}

#[test]
fn content_budget_is_cumulative_over_groups_and_fill_loops() {
    let square = Loop2::polygon(&[p(0.0, 0.0), p(2.0, 0.0), p(2.0, 2.0), p(0.0, 2.0)]);
    let source = Scene {
        size_mm: p(20.0, 20.0),
        groups: vec![
            Group { object: 1, kind: ObjectKind::DrawingBorder, items: vec![stroke(square.segs.clone(), true, 0.35, None)] },
            Group { object: 2, kind: ObjectKind::TitleBlock, items: vec![Item::Fill { region: Region2 { outer: square, holes: vec![] } }] },
        ],
    };
    let on = source.prepare().unwrap().on_grid(Grid::TEN_NANOMETRES).unwrap();
    let complete = super::content(&on.groups).unwrap();
    assert_eq!(super::content_with_limit(&on.groups, complete.len()).unwrap(), complete);
    assert!(super::content_with_limit(&on.groups, complete.len() - 1).is_err(), "the final Q is part of the cumulative limit");
    let first = super::content(&on.groups[..1]).unwrap();
    assert!(super::content_with_limit(&on.groups, first.len()).is_err(), "groups must share one allowance");
    assert_eq!(Content::new(usize::MAX).unwrap().max_bytes, MAX_DRAWING_BYTES);
}

#[test]
fn content_budget_rejects_an_append_before_mutating_existing_bytes() {
    let mut w = Content::new(4).unwrap();
    w.append("abc").unwrap();
    assert!(w.append("de").is_err());
    assert_eq!(w.out, "abc");
    w.append("d").unwrap();
    assert_eq!(w.out, "abcd");
    w.append("").unwrap();
    assert!(w.append("e").is_err());
    assert_eq!(w.out, "abcd");
}

#[test]
fn content_budget_reuses_the_original_formatter_at_integer_extremes() {
    for (n, expected) in [
        (0, "0"),
        (1, "0.00001"),
        (-1, "-0.00001"),
        (100_000, "1"),
        (-100_000, "-1"),
        (i64::MAX, "92233720368547.75807"),
        (i64::MIN, "-92233720368547.75808"),
    ] {
        let mut w = Content::new(expected.len()).unwrap();
        w.number(n).unwrap();
        assert_eq!(w.out, expected);
        let mut too_small = Content::new(expected.len() - 1).unwrap();
        assert!(too_small.number(n).is_err());
        assert!(too_small.out.is_empty());
    }
}
