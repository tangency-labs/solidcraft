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

const NONE: PdfOptions = PdfOptions { layers: true, title: None, output_time: None };

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
    let leap_day = PdfOptions { layers: true, title: Some("Sheet 1".into()), output_time: Some(951_782_400) };
    let file = text(&pdf(&sheet, &leap_day));
    assert!(file.contains("/CreationDate (D:20000229000000Z)") && file.contains("/ModDate (D:20000229000000Z)"), "{file}");
    assert!(file.contains("/Title (Sheet 1)"), "{file}");
    assert_eq!(pdf(&sheet, &leap_day), pdf(&sheet, &leap_day));
    let last = PdfOptions { layers: true, title: Some(String::new()), output_time: Some(MAX_OUTPUT_TIME) };
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
    let rest = tail.strip_prefix(b"xref\n0 ").ok_or("xref offset is not the expected subsection")?;
    let end_of_count = rest.iter().position(|&c| c == b'\n').ok_or("truncated xref count")?;
    let count: usize =
        std::str::from_utf8(&rest[..end_of_count]).map_err(|_| "xref count is not ASCII")?.parse().map_err(|_| "invalid xref count")?;
    // Five fixed objects plus up to five distinct current ObjectKinds, and the free object.
    if !(6..=11).contains(&count) {
        return Err(format!("unexpected PDF object count {count}"));
    }
    let table = rest.get(end_of_count + 1..).ok_or("truncated xref table")?;
    let entry_bytes = count.checked_mul(20).ok_or("xref count overflow")?;
    let entries = table.get(..entry_bytes).ok_or("truncated xref entries")?;
    let (entries, remainder) = entries.as_chunks::<20>();
    if !remainder.is_empty() {
        return Err("xref entry does not occupy 20 bytes".into());
    }
    for (n, entry) in entries.iter().enumerate() {
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
        let bytes = pdf(&sheet, &PdfOptions { layers: true, title: Some(title.into()), output_time: Some(0) });
        assert!(check_xref(&bytes).is_ok(), "{title:?}: {:?}", check_xref(&bytes));
    }
}

#[test]
fn the_byte_offset_oracle_refuses_shifted_or_corrupted_references() {
    let bytes = pdf(&scene(p(420.0, 297.0), vec![]), &PdfOptions { layers: true, title: Some("\u{00c7}".into()), output_time: None });
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
    assert!(paper_pdf(&prepared, &PdfOptions { layers: true, title: Some("x".repeat(1000)), output_time: None }).is_ok());
    let e = paper_pdf(&prepared, &PdfOptions { layers: true, title: Some("x".repeat(1001)), output_time: None }).unwrap_err();
    assert!(e.to_string().contains("1000 characters"), "{e}");
    let e = paper_pdf(&prepared, &PdfOptions { layers: true, title: None, output_time: Some(MAX_OUTPUT_TIME + 1) }).unwrap_err();
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
/// P3: one optional-content group per nonempty kind, shared across repeats.
/// The scene's drawing order remains the content stream's paint order.
#[test]
fn pdf_layers_share_object_kinds_and_keep_noncontiguous_paint_order() {
    let path = |y: f64| vec![Seg2::Line { a: p(10.0, y), b: p(100.0, y) }];
    let sheet = Scene {
        size_mm: p(210.0, 297.0),
        groups: vec![
            Group { object: 10, kind: ObjectKind::TitleBlock, items: vec![stroke(path(10.0), false, 0.35, None)] },
            Group { object: 20, kind: ObjectKind::DrawingBorder, items: vec![stroke(path(20.0), false, 0.35, None)] },
            Group { object: 11, kind: ObjectKind::TitleBlock, items: vec![stroke(path(30.0), false, 0.35, None)] },
            Group { object: 30, kind: ObjectKind::SheetBorder, items: vec![] },
        ],
    };
    let bytes = pdf(&sheet, &NONE);
    assert!(check_xref(&bytes).is_ok(), "{:?}", check_xref(&bytes));
    assert_eq!(pdf(&sheet, &NONE), bytes, "layer order and object ids must be deterministic");
    let file = text(&bytes);
    assert!(file.contains("/OCProperties"), "the catalog must declare optional content");
    assert!(file.contains("/OCGs [6 0 R 7 0 R]"), "the same kind must reuse its OCG: {file}");
    assert!(file.contains("/Order [6 0 R 7 0 R]"), "the visible layers are ordered");
    assert!(file.contains("/ON [6 0 R 7 0 R]"), "both are visible by default");
    assert!(file.contains("/BaseState /ON"), "the default state is ON");
    assert!(file.contains("/Name (TITLE_BLOCK)") && file.contains("/Name (DRAWING_BORDER)"));
    assert!(!file.contains("/Name (SHEET_BORDER)"), "an empty group has no painted layer");
    assert!(file.contains("/Properties") && file.contains("/L0 6 0 R") && file.contains("/L1 7 0 R"));
    let stream = content(&bytes);
    let markers: Vec<_> = stream.lines().filter(|line| line.ends_with(" BDC") || *line == "EMC").collect();
    assert_eq!(markers, ["/OC /L0 BDC", "EMC", "/OC /L1 BDC", "EMC", "/OC /L0 BDC", "EMC"]);
    assert_eq!(stream.lines().filter(|line| *line == "S").count(), 3);
    assert!(check_number_grid(&stream).is_ok());
}

/// Every current paper kind has one independently named layer, without sorting
/// noncontiguous groups or losing graphics state across marked-content boundaries.
#[test]
fn pdf_layers_cover_all_five_kinds_and_preserve_paint_and_styles() {
    let line = |y: f64| vec![Seg2::Line { a: p(10.0, y), b: p(100.0, y) }];
    let dashed = Some(Dashes { lengths_mm: vec![4.0, 2.0], phase_mm: 0.0 });
    let square = Loop2::polygon(&[p(10.0, 30.0), p(20.0, 30.0), p(20.0, 40.0), p(10.0, 40.0)]);
    let sheet = Scene {
        size_mm: p(210.0, 297.0),
        groups: vec![
            Group { object: 10, kind: ObjectKind::TitleBlock, items: vec![stroke(line(10.0), false, 0.35, None)] },
            Group { object: 20, kind: ObjectKind::DrawingBorder, items: vec![stroke(line(20.0), false, 0.7, dashed.clone())] },
            Group { object: 30, kind: ObjectKind::PartsList, items: vec![Item::Fill { region: Region2 { outer: square, holes: vec![] } }] },
            Group { object: 40, kind: ObjectKind::RevisionTable, items: vec![stroke(line(50.0), false, 0.7, dashed)] },
            Group { object: 50, kind: ObjectKind::SheetBorder, items: vec![stroke(line(60.0), false, 0.35, None)] },
            Group { object: 11, kind: ObjectKind::TitleBlock, items: vec![stroke(line(70.0), false, 0.35, None)] },
            Group { object: 60, kind: ObjectKind::DrawingBorder, items: vec![] },
        ],
    };
    let bytes = pdf(&sheet, &NONE);
    assert_eq!(pdf(&sheet, &NONE), bytes);
    assert!(check_xref(&bytes).is_ok(), "{:?}", check_xref(&bytes));
    let file = text(&bytes);
    for key in ["OCGs", "Order", "ON"] {
        assert!(file.contains(&format!("/{key} [6 0 R 7 0 R 8 0 R 9 0 R 10 0 R]")), "{file}");
    }
    assert!(file.contains("/BaseState /ON"));
    assert_eq!(file.matches("/Type /OCG\n").count(), 5, "one OCG per kind, with no empty-group duplicate");
    // Literal expected names and references are independent of ObjectKind::layer.
    for (number, name) in [(6, "TITLE_BLOCK"), (7, "DRAWING_BORDER"), (8, "PARTS_LIST"), (9, "REVISION_TABLE"), (10, "SHEET_BORDER")] {
        let header = format!("{number} 0 obj\n");
        let object = file.split_once(&header).unwrap().1.split_once("endobj").unwrap().0;
        assert!(object.contains("/Type /OCG") && object.contains(&format!("/Name ({name})")), "{object}");
        assert_eq!(file.matches(&format!("/Name ({name})")).count(), 1);
        assert!(file.contains(&format!("/L{} {number} 0 R", number - 6)));
    }
    let stream = content(&bytes);
    let markers: Vec<_> = stream.lines().filter(|line| line.ends_with(" BDC") || *line == "EMC").collect();
    assert_eq!(
        markers,
        ["/OC /L0 BDC", "EMC", "/OC /L1 BDC", "EMC", "/OC /L2 BDC", "EMC", "/OC /L3 BDC", "EMC", "/OC /L4 BDC", "EMC", "/OC /L0 BDC", "EMC",]
    );
    let operators = ops(&stream);
    let paints: Vec<_> = operators.iter().map(|(op, _)| op.as_str()).filter(|op| ["S", "h", "f*"].contains(op)).collect();
    assert_eq!(paints, ["S", "S", "h", "f*", "S", "S", "S"]);
    let starts: Vec<_> = operators.iter().filter(|(op, _)| op == "m").map(|(_, n)| (n[0], n[1])).collect();
    assert_eq!(starts, [(10.0, 10.0), (10.0, 20.0), (10.0, 30.0), (10.0, 50.0), (10.0, 60.0), (10.0, 70.0)]);
    let styles: Vec<(&str, &[f64])> = operators.iter().filter(|(op, _)| op == "w" || op == "d").map(|(op, n)| (op.as_str(), n.as_slice())).collect();
    assert_eq!(styles, [("w", &[0.35][..]), ("w", &[0.7][..]), ("d", &[4.0, 2.0, 0.0][..]), ("w", &[0.35][..]), ("d", &[0.0][..])]);
    assert!(check_number_grid(&stream).is_ok());
    // A reviewer may retain these exact asserted bytes for independent PDF readers.
    if let Some(path) = std::env::var_os("SOLIDCRAFT_PDF_LAYER_REVIEW_OUTPUT") {
        let written = std::fs::write(&path, &bytes);
        assert!(written.is_ok(), "could not write the asserted layer fixture to {path:?}: {written:?}");
    }
}

/// P3: no phantom layer dictionary on a truly empty scene, and content
/// limits include all OCG markers instead of silently cutting them off.
#[test]
fn pdf_layers_obey_content_budget_and_skip_empty_groups() {
    let empty = scene(p(210.0, 297.0), vec![]);
    let bytes = pdf(&empty, &NONE);
    assert!(check_xref(&bytes).is_ok());
    assert!(!text(&bytes).contains("/OCProperties"));
    assert!(!content(&bytes).contains(" BDC"));
    let line = scene(p(210.0, 297.0), vec![stroke(vec![Seg2::Line { a: p(10.0, 10.0), b: p(100.0, 10.0) }], false, 0.35, None)]);
    let grid = line.prepare().unwrap().on_grid(Grid::TEN_NANOMETRES).unwrap();
    let full = super::content_with_limit(&grid.groups, MAX_DRAWING_BYTES).unwrap();
    assert!(full.contains("/OC /L0 BDC") && full.contains("EMC"));
    assert_eq!(super::content_with_limit(&grid.groups, full.len()).unwrap(), full);
    assert!(super::content_with_limit(&grid.groups, full.len() - 1).is_err());
}

/// Flat PDF is the same ordered vector paint, not a hidden or rasterized layer.
#[test]
fn flat_pdf_omits_optional_content_and_preserves_vector_paint() {
    let line = |y| vec![Seg2::Line { a: p(10.0, y), b: p(100.0, y) }];
    let sheet = Scene {
        size_mm: p(210.0, 297.0),
        groups: vec![
            Group { object: 1, kind: ObjectKind::TitleBlock, items: vec![stroke(line(10.0), false, 0.35, None)] },
            Group {
                object: 2,
                kind: ObjectKind::DrawingBorder,
                items: vec![stroke(line(20.0), false, 0.7, Some(Dashes { lengths_mm: vec![4.0, 2.0], phase_mm: 1.0 }))],
            },
            Group {
                object: 3,
                kind: ObjectKind::PartsList,
                items: vec![Item::Fill {
                    region: Region2 { outer: Loop2::polygon(&[p(10.0, 30.0), p(20.0, 30.0), p(20.0, 40.0), p(10.0, 40.0)]), holes: vec![] },
                }],
            },
            Group { object: 4, kind: ObjectKind::RevisionTable, items: vec![stroke(line(50.0), false, 0.7, None)] },
            Group { object: 5, kind: ObjectKind::SheetBorder, items: vec![] },
            Group {
                object: 6,
                kind: ObjectKind::TitleBlock,
                items: vec![stroke(vec![Seg2::Arc { center: p(50.0, 70.0), radius: 10.0, start: 0.0, sweep: FRAC_PI_2 }], false, 0.35, None)],
            },
        ],
    };
    let layered_options = PdfOptions { title: Some("Flat Ç sheet".into()), output_time: Some(0), ..PdfOptions::default() };
    assert!(layered_options.layers && PdfOptions::default().layers);
    let flat_options = PdfOptions { layers: false, ..layered_options.clone() };
    let layered = pdf(&sheet, &layered_options);
    let flat = pdf(&sheet, &flat_options);
    assert_eq!(pdf(&sheet, &flat_options), flat);
    assert!(check_xref(&flat).is_ok());
    let file = text(&flat);
    for forbidden in ["/OCProperties", "/OCGs", "/Type /OCG", "/Properties", "/BaseState", "/Order", "/ON"] {
        assert!(!file.contains(forbidden), "flat output contains {forbidden}");
    }
    let flat_stream = content(&flat);
    assert!(!flat_stream.contains("BDC") && !flat_stream.contains("EMC"));
    let without_layers =
        content(&layered).lines().filter(|line| !line.ends_with(" BDC") && *line != "EMC").map(|line| format!("{line}\n")).collect::<String>();
    assert_eq!(flat_stream, without_layers, "all geometry, styles and paint order retained");
    let operators = ops(&flat_stream);
    let paint: Vec<_> = operators.iter().filter(|(op, _)| ["S", "h", "f*"].contains(&op.as_str())).map(|(op, _)| op.as_str()).collect();
    assert_eq!(paint, ["S", "S", "h", "f*", "S", "S"]);
    assert!(operators.iter().any(|(op, _)| op == "c"), "arc stays vector cubic geometry");
    assert!(check_number_grid(&flat_stream).is_ok());
    // Dictionary entries are indented. Compare exact, uniquely present values
    // as bytes, retaining the title's UTF-16BE rather than lossy UTF-8 text.
    let property = |bytes: &[u8], key: &str| {
        bytes
            .split(|&byte| byte == b'\n')
            .map(|line| line.trim_ascii_start())
            .filter_map(|line| line.strip_prefix(key.as_bytes()))
            .filter(|value| value.first().is_some_and(u8::is_ascii_whitespace))
            .map(|value| value.trim_ascii().to_vec())
            .collect::<Vec<_>>()
    };
    let producer = format!("(SolidCraft {})", env!("CARGO_PKG_VERSION"));
    for (key, expected) in [
        ("/MediaBox", "[0 0 595.2756 841.8898]"),
        ("/Producer", producer.as_str()),
        ("/Title", "<FEFF0046006C00610074002000C7002000730068006500650074>"),
        ("/CreationDate", "(D:19700101000000Z)"),
        ("/ModDate", "(D:19700101000000Z)"),
    ] {
        let flat_value = property(&flat, key);
        let layered_value = property(&layered, key);
        assert_eq!(flat_value, [expected.as_bytes().to_vec()], "flat {key} is present once with the expected value");
        assert_eq!(layered_value, [expected.as_bytes().to_vec()], "layered {key} is present once with the expected value");
        assert_eq!(flat_value, layered_value, "{key}");
    }
    // Optional independent-reader fixture, after all ordinary acceptance assertions.
    if let Some(path) = std::env::var_os("SOLIDCRAFT_PDF_FLAT_REVIEW_OUTPUT") {
        let written = std::fs::write(&path, &flat);
        assert!(written.is_ok(), "could not write the asserted flat fixture to {path:?}: {written:?}");
        let layered_path = std::path::PathBuf::from(&path).with_extension("layered.pdf");
        let written = std::fs::write(&layered_path, &layered);
        assert!(written.is_ok(), "could not write the matching layered fixture to {layered_path:?}: {written:?}");
    }
}

#[test]
fn flat_pdf_empty_scene_and_content_budget_keep_their_boundaries() {
    let options = PdfOptions { layers: false, ..PdfOptions::default() };
    let empty = scene(p(210.0, 297.0), vec![]);
    assert_eq!(pdf(&empty, &options), pdf(&empty, &PdfOptions::default()), "empty groups invent no layer in either mode");
    let line = scene(p(210.0, 297.0), vec![stroke(vec![Seg2::Line { a: p(10.0, 10.0), b: p(100.0, 10.0) }], false, 0.35, None)]);
    let grid = line.prepare().unwrap().on_grid(Grid::TEN_NANOMETRES).unwrap();
    let flat = super::content_with_limit_and_layers(&grid.groups, None, MAX_DRAWING_BYTES).unwrap();
    assert_eq!(super::content_with_limit_and_layers(&grid.groups, None, flat.len()).unwrap(), flat);
    assert!(super::content_with_limit_and_layers(&grid.groups, None, flat.len() - 1).is_err());
    assert!(
        super::content_with_limit_and_layers(&grid.groups, Some(&[]), MAX_DRAWING_BYTES).is_err(),
        "missing enabled layer is not silently flattened"
    );
    assert!(super::content_with_limit(&grid.groups, flat.len()).is_err(), "markers remain charged on the layered path");
}

/// Cam's original public-safe P1 sample is a byte oracle independent of the new
/// flat path: no layer metadata or content marker may leak into this output.
#[test]
fn flat_pdf_reproduces_the_original_p1_a3_sample_bytes() {
    use sha2::Digest as _;
    let rect = |a: Vec2, b: Vec2| {
        let c = [a, p(b.x, a.y), b, p(a.x, b.y)];
        (0..4).map(|i| Seg2::Line { a: c[i], b: c[(i + 1) % 4] }).collect()
    };
    let centre = Some(Dashes { lengths_mm: vec![12.0, 3.0, 1.0, 3.0], phase_mm: 0.0 });
    let sheet = Scene {
        size_mm: p(420.0, 297.0),
        groups: vec![
            Group {
                object: 1,
                kind: ObjectKind::DrawingBorder,
                items: vec![
                    stroke(rect(p(20.0, 10.0), p(410.0, 287.0)), true, 0.7, None),
                    Item::Fill { region: Region2 { outer: Loop2 { segs: rect(p(0.0, 287.0), p(10.0, 297.0)) }, holes: vec![] } },
                    Item::Fill { region: Region2 { outer: Loop2 { segs: rect(p(410.0, 0.0), p(420.0, 10.0)) }, holes: vec![] } },
                ],
            },
            Group {
                object: 2,
                kind: ObjectKind::DrawingBorder,
                items: vec![
                    stroke(rect(p(120.0, 90.0), p(300.0, 210.0)), true, 0.5, None),
                    stroke(Loop2::circle(p(210.0, 150.0), 40.0).segs, true, 0.5, None),
                    stroke(vec![Seg2::Line { a: p(160.0, 150.0), b: p(260.0, 150.0) }], false, 0.25, centre.clone()),
                    stroke(vec![Seg2::Line { a: p(210.0, 100.0), b: p(210.0, 200.0) }], false, 0.25, centre),
                    stroke(vec![Seg2::Arc { center: p(120.0, 90.0), radius: 15.0, start: 0.0, sweep: FRAC_PI_2 }], false, 0.35, None),
                ],
            },
        ],
    };
    let options = PdfOptions { title: Some("A3 sample".into()), output_time: Some(1_800_000_000), layers: false };
    let bytes = pdf(&sheet, &options);
    assert_eq!(bytes.len(), 1063);
    assert_eq!(format!("{:x}", sha2::Sha256::digest(&bytes)), "0de09bc342566206514754fc2ed7836156fbe10ba444e8ebb66427b10dd89c02");
    assert_eq!(pdf(&sheet, &options), bytes);
    assert!(check_xref(&bytes).is_ok());
}

#[test]
fn adjacent_pdf_groups_share_one_run_without_reordering_noncontiguous_layers() {
    let groups = [
        (1, ObjectKind::TitleBlock, 10.0),
        (2, ObjectKind::TitleBlock, 20.0),
        (3, ObjectKind::DrawingBorder, 30.0),
        (4, ObjectKind::TitleBlock, 40.0),
    ]
    .into_iter()
    .map(|(object, kind, y)| Group { object, kind, items: vec![stroke(vec![Seg2::Line { a: p(10.0, y), b: p(20.0, y) }], false, 0.35, None)] })
    .collect();
    let sheet = Scene { size_mm: p(210.0, 297.0), groups };
    let bytes = pdf(&sheet, &NONE);
    assert_eq!(pdf(&sheet, &NONE), bytes);
    assert!(check_xref(&bytes).is_ok());
    let file = text(&bytes);
    assert_eq!(file.matches("/Type /OCG\n").count(), 2);
    for key in ["OCGs", "Order", "ON"] {
        assert!(file.contains(&format!("/{key} [6 0 R 7 0 R]")));
    }
    assert!(file.contains("/Name (TITLE_BLOCK)") && file.contains("/Name (DRAWING_BORDER)"));
    let stream = content(&bytes);
    let markers: Vec<_> = stream.lines().filter(|line| line.ends_with(" BDC") || *line == "EMC").collect();
    assert_eq!(markers, ["/OC /L0 BDC", "EMC", "/OC /L1 BDC", "EMC", "/OC /L0 BDC", "EMC"]);
    let operators = ops(&stream);
    let starts: Vec<_> = operators.iter().filter(|(op, _)| op == "m").map(|(_, n)| (n[0], n[1])).collect();
    assert_eq!(starts, [(10.0, 10.0), (10.0, 20.0), (10.0, 30.0), (10.0, 40.0)]);
    assert_eq!(operators.iter().filter(|(op, _)| op == "S").count(), 4);
    if let Some(path) = std::env::var_os("SOLIDCRAFT_PDF_COALESCING_REVIEW_OUTPUT") {
        let flat = pdf(&sheet, &PdfOptions { layers: false, ..NONE.clone() });
        let written = std::fs::write(&path, &bytes);
        assert!(written.is_ok(), "could not write the asserted coalescing fixture to {path:?}: {written:?}");
        let flat_path = std::path::PathBuf::from(&path).with_extension("flat.pdf");
        let written = std::fs::write(&flat_path, &flat);
        assert!(written.is_ok(), "could not write the matching flat fixture to {flat_path:?}: {written:?}");
    }
}

#[test]
fn empty_pdf_groups_do_not_break_adjacent_painted_runs_or_invent_layers() {
    for insert_empty in [false, true] {
        let mut groups = vec![Group { object: 0, kind: ObjectKind::SheetBorder, items: vec![] }];
        for (object, y) in [(1, 10.0), (2, 20.0)] {
            if insert_empty {
                groups.push(Group { object: 100 + object, kind: ObjectKind::DrawingBorder, items: vec![] });
            }
            groups.push(Group {
                object,
                kind: ObjectKind::TitleBlock,
                items: vec![stroke(vec![Seg2::Line { a: p(10.0, y), b: p(20.0, y) }], false, 0.35, None)],
            });
        }
        groups.push(Group { object: 200, kind: ObjectKind::RevisionTable, items: vec![] });
        let sheet = Scene { size_mm: p(210.0, 297.0), groups };
        let bytes = pdf(&sheet, &NONE);
        let file = text(&bytes);
        assert_eq!(file.matches("/Type /OCG\n").count(), 1);
        for absent in ["SHEET_BORDER", "DRAWING_BORDER", "REVISION_TABLE"] {
            assert!(!file.contains(&format!("/Name ({absent})")));
        }
        let stream = content(&bytes);
        let markers: Vec<_> = stream.lines().filter(|line| line.ends_with(" BDC") || *line == "EMC").collect();
        assert_eq!(markers, ["/OC /L0 BDC", "EMC"]);
        assert_eq!(ops(&stream).iter().filter(|(op, _)| op == "S").count(), 2);
    }
}

#[test]
fn a_merged_pdf_run_keeps_all_paint_and_shared_width_and_dash_state() {
    let line = |y| vec![Seg2::Line { a: p(10.0, y), b: p(20.0, y) }];
    let dashed = Some(Dashes { lengths_mm: vec![4.0, 2.0], phase_mm: 0.0 });
    let square = Loop2::polygon(&[p(10.0, 30.0), p(20.0, 30.0), p(20.0, 40.0), p(10.0, 40.0)]);
    let items = [
        stroke(line(10.0), false, 0.35, None),
        stroke(line(20.0), false, 0.7, dashed.clone()),
        Item::Fill { region: Region2 { outer: square, holes: vec![] } },
        stroke(line(50.0), false, 0.7, dashed),
        stroke(line(60.0), false, 0.7, None),
    ];
    let sheet = Scene {
        size_mm: p(210.0, 297.0),
        groups: items.into_iter().enumerate().map(|(i, item)| Group { object: i as u64, kind: ObjectKind::TitleBlock, items: vec![item] }).collect(),
    };
    let before = sheet.clone();
    let prepared = sheet.prepare().unwrap();
    let (bytes, report) = paper_pdf(&prepared, &NONE).unwrap();
    let flat_options = PdfOptions { layers: false, ..NONE.clone() };
    let (flat, flat_report) = paper_pdf(&prepared, &flat_options).unwrap();
    assert_eq!(sheet, before);
    assert_eq!(report, flat_report);
    let stream = content(&bytes);
    let markers: Vec<_> = stream.lines().filter(|line| line.ends_with(" BDC") || *line == "EMC").collect();
    assert_eq!(markers, ["/OC /L0 BDC", "EMC"]);
    let operators = ops(&stream);
    let paints: Vec<_> = operators.iter().filter(|(op, _)| ["S", "h", "f*"].contains(&op.as_str())).map(|(op, _)| op.as_str()).collect();
    assert_eq!(paints, ["S", "S", "h", "f*", "S", "S"]);
    let styles: Vec<_> = operators.iter().filter(|(op, _)| op == "w" || op == "d").map(|(op, n)| (op.as_str(), n.as_slice())).collect();
    assert_eq!(styles, [("w", &[0.35][..]), ("w", &[0.7][..]), ("d", &[4.0, 2.0, 0.0][..]), ("d", &[0.0][..])]);
    let starts: Vec<_> = operators.iter().filter(|(op, _)| op == "m").map(|(_, n)| (n[0], n[1])).collect();
    assert_eq!(starts, [(10.0, 10.0), (10.0, 20.0), (10.0, 30.0), (10.0, 50.0), (10.0, 60.0)]);
    let stripped = stream.lines().filter(|line| !line.ends_with(" BDC") && *line != "EMC").map(|line| format!("{line}\n")).collect::<String>();
    assert_eq!(stripped, content(&flat));
    assert!(check_xref(&bytes).is_ok() && check_xref(&flat).is_ok());
}

#[test]
fn merged_pdf_run_limits_charge_transitions_and_the_final_close() {
    let groups = [(1, ObjectKind::TitleBlock, 10.0), (2, ObjectKind::TitleBlock, 20.0), (3, ObjectKind::DrawingBorder, 30.0)]
        .into_iter()
        .map(|(object, kind, y)| Group { object, kind, items: vec![stroke(vec![Seg2::Line { a: p(10.0, y), b: p(20.0, y) }], false, 0.35, None)] })
        .collect();
    let sheet = Scene { size_mm: p(210.0, 297.0), groups };
    let prepared = sheet.prepare().unwrap();
    let grid = prepared.on_grid(Grid::TEN_NANOMETRES).unwrap();
    // Literal independent stream: adjacent objects retain two strokes within
    // L0; only the change to L1 closes and reopens marked content.
    let expected = concat!(
        "q 2.834645669 0 0 2.834645669 0 0 cm 0 J 0 j 4 M\n",
        "/OC /L0 BDC\n0.35 w\n10 10 m\n20 10 l\nS\n10 20 m\n20 20 l\nS\n",
        "EMC\n/OC /L1 BDC\n10 30 m\n20 30 l\nS\nEMC\nQ\n",
    );
    assert_eq!(super::content_with_limit(&grid.groups, expected.len()).unwrap(), expected);
    assert!(super::content_with_limit(&grid.groups, expected.len() - 1).is_err());
    for tail in ["EMC\n/OC /L1 BDC\n", "/OC /L1 BDC\n", "EMC\nQ\n"] {
        let prefix = expected.find(tail).unwrap();
        assert!(super::content_with_limit(&grid.groups, prefix + tail.len() - 1).is_err());
    }
}

// Many pages (#7626's follow-up): `paper_pdf_pages`, with `paper_pdf` its one-page call.

/// A sheet of `size` with a short stroke for each of `kinds`, one group each, in order.
fn sheet_of(size: Vec2, kinds: &[ObjectKind]) -> Scene {
    let line = |y: f64| stroke(vec![Seg2::Line { a: p(10.0, y), b: p(20.0, y) }], false, 0.35, None);
    let groups = kinds.iter().zip(1u64..).map(|(kind, object)| Group { object, kind: *kind, items: vec![line(10.0 * object as f64)] }).collect();
    Scene { size_mm: size, groups }
}

/// Each object of a file: its number and its text, from `N 0 obj` to `endobj`.
fn objects(bytes: &[u8]) -> Vec<(i32, String)> {
    text(bytes)
        .split("endobj")
        .filter_map(|chunk| {
            let at = chunk.find(" 0 obj")?;
            let number = chunk[..at].rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
            Some((number, chunk[at..].to_string()))
        })
        .collect()
}

/// Every content stream of a file, inflated, in file order (each starts after its dictionary,
/// where `endstream` never stands).
fn contents(bytes: &[u8]) -> Vec<String> {
    let start = b">>\nstream\n";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = bytes[from..].windows(start.len()).position(|w| w == start).map(|at| from + at) {
        out.push(content(&bytes[at..]));
        from = at + start.len();
    }
    out
}

/// A page's MediaBox width and height, in points.
fn media_box(object: &str) -> [f64; 2] {
    let at = object.find("/MediaBox [").unwrap() + "/MediaBox [".len();
    let n: Vec<f64> = object[at..].split(']').next().unwrap().split_whitespace().map(|n| n.parse().unwrap()).collect();
    [n[2], n[3]]
}

/// One page through the page builder is today's one-page call, byte for byte, with or without
/// layers, a title and a date; its objects keep their numbers: catalog 1, page tree 2, page 3,
/// content 4, Info 5, layers from 6.
#[test]
fn one_page_is_the_one_page_call_byte_for_byte() {
    let sheet = sheet_of(p(420.0, 297.0), &[ObjectKind::DrawingBorder, ObjectKind::TitleBlock, ObjectKind::DrawingBorder]);
    let prepared = sheet.prepare().unwrap();
    for layers in [true, false] {
        for (title, output_time) in [(None, None), (Some("Bracket — sheet 1".to_string()), Some(1_761_800_000))] {
            let options = PdfOptions { title, output_time, layers };
            let one = paper_pdf(&prepared, &options).unwrap();
            let (bytes, reports) = paper_pdf_pages(std::slice::from_ref(&prepared), &options).unwrap();
            assert_eq!((&bytes, &reports[..]), (&one.0, std::slice::from_ref(&one.1)), "{options:?}");
        }
    }
    let bytes = pdf(&sheet, &PdfOptions::default());
    let numbers: Vec<i32> = objects(&bytes).iter().map(|(n, _)| *n).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5, 6, 7]);
    let objects = objects(&bytes);
    assert!(
        objects[2].1.contains("/Type /Page")
            && !objects[2].1.contains("/Type /Pages")
            && objects[3].1.contains("stream")
            && objects[4].1.contains("/Producer")
    );
    assert!(objects[5].1.contains("/Type /OCG") && objects[6].1.contains("/Type /OCG"));
}

/// Pages follow in order, each its sheet's size, numbered page then content; the Info dictionary
/// follows them, then one layer for each kind across all pages, tagged as first painted, so a kind
/// is the same layer on every page. Each page lists only the layers it paints; a page painting
/// nothing lists none. Without layers each page's content is that sheet's one-page content.
#[test]
fn pages_follow_in_order_each_its_sheets_size_sharing_one_layer_a_kind() {
    use ObjectKind::{DrawingBorder, TitleBlock};
    let sheets = [sheet_of(p(420.0, 297.0), &[DrawingBorder, TitleBlock]), sheet_of(p(210.0, 297.0), &[TitleBlock]), sheet_of(p(297.0, 210.0), &[])];
    let prepared: Vec<_> = sheets.iter().map(|s| s.prepare().unwrap()).collect();
    let (bytes, reports) = paper_pdf_pages(&prepared, &PdfOptions::default()).unwrap();
    assert_eq!(reports.len(), 3);
    let objects = objects(&bytes);
    assert_eq!(objects.iter().map(|(n, _)| *n).collect::<Vec<_>>(), (1..=11).collect::<Vec<_>>());
    assert!(objects[1].1.contains("/Kids [3 0 R 5 0 R 7 0 R]") && objects[1].1.contains("/Count 3"), "{}", objects[1].1);
    for (page, size) in [(2, [420.0, 297.0]), (4, [210.0, 297.0]), (6, [297.0, 210.0])] {
        let [w, h] = media_box(&objects[page].1);
        assert!((w - size[0] * PT_PER_MM).abs() < 1e-3 && (h - size[1] * PT_PER_MM).abs() < 1e-3, "{}", objects[page].1);
    }
    assert!(objects[8].1.contains("/Producer"), "Info follows the pages");
    assert!(objects[9].1.contains(DrawingBorder.layer()), "{}", objects[9].1);
    assert!(objects[10].1.contains(TitleBlock.layer()), "{}", objects[10].1);
    assert!(objects[0].1.contains("/OCGs [10 0 R 11 0 R]"), "{}", objects[0].1);
    assert!(objects[2].1.contains("/L0 10 0 R") && objects[2].1.contains("/L1 11 0 R"));
    assert!(objects[4].1.contains("/L1 11 0 R") && !objects[4].1.contains("/L0"), "the second page paints only the title block");
    assert!(!objects[6].1.contains("/Properties"), "a page painting nothing lists no layers");
    let streams = contents(&bytes);
    assert_eq!(streams.len(), 3);
    assert!(streams[1].contains("/OC /L1 BDC") && !streams[1].contains("/L0"), "{}", streams[1]);
    // Without layers each page is its sheet's own content, and the pages hold no layer at all.
    let flat = PdfOptions { layers: false, ..PdfOptions::default() };
    let (bytes, _) = paper_pdf_pages(&prepared, &flat).unwrap();
    let alone: Vec<String> = sheets.iter().map(|s| content(&pdf(s, &flat))).collect();
    assert_eq!(contents(&bytes), alone);
    assert!(!text(&bytes).contains("/OCProperties") && !text(&bytes).contains("/Properties"));
    // The same sheets give the same bytes.
    assert_eq!(paper_pdf_pages(&prepared, &flat).unwrap().0, bytes);
}

/// No pages, more than a drawing has sheets, and any page that couldn't be written alone are
/// refused.
#[test]
fn pages_are_refused_past_their_limits() {
    let e = paper_pdf_pages(&[], &PdfOptions::default()).unwrap_err();
    assert!(e.to_string().contains("1 to 1000 pages"), "{e}");
    let sheet = scene(p(10.0, 10.0), vec![]);
    let many: Vec<_> = (0..=MAX_PAGES).map(|_| sheet.prepare().unwrap()).collect();
    let e = paper_pdf_pages(&many, &PdfOptions::default()).unwrap_err();
    assert!(e.to_string().contains("1 to 1000 pages"), "{e}");
    assert!(paper_pdf_pages(&many[..MAX_PAGES], &PdfOptions::default()).is_ok());
    let tiny = scene(p(1.0, 297.0), vec![]);
    let e = paper_pdf_pages(&[sheet.prepare().unwrap(), tiny.prepare().unwrap()], &PdfOptions::default()).unwrap_err();
    assert!(e.to_string().contains("at least 3 pt"), "{e}");
}
