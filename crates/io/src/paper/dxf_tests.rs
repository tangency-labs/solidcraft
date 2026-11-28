use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_geom::Aabb2;
use solidcraft_geom::paper::{Dashes, Group, Item, ObjectKind, Picture, Scene};

use super::*;

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn stroke(y: f64, width_mm: f64, lengths_mm: Option<Vec<f64>>) -> Item {
    Item::Stroke {
        path: vec![Seg2::Line { a: p(10.0, y), b: p(100.0, y) }],
        closed: false,
        width_mm,
        pattern: lengths_mm.map(|lengths_mm| Dashes { lengths_mm, phase_mm: 0.0 }),
    }
}

/// Each entity's type, layer (8) and weight (370), and each layer's name, linetype (6), colour (62)
/// and weight, in file order.
#[allow(clippy::type_complexity)]
fn read(text: &str) -> (Vec<(String, String, Option<i32>)>, Vec<(String, String, i32, Option<i32>)>) {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let pairs: Vec<(&str, &str)> = lines.chunks(2).filter_map(|c| Some((c[0], *c.get(1)?))).collect();
    let (mut entities, mut layers, mut section, mut i) = (Vec::new(), Vec::new(), "", 0);
    while i < pairs.len() {
        let (code, value) = pairs[i];
        if code == "2" && i > 0 && pairs[i - 1] == ("0", "SECTION") {
            section = value;
        }
        if pairs[i] == ("0", "ENDSEC") {
            section = "";
        }
        let entity = section == "ENTITIES" && code == "0" && value != "ENDSEC";
        if entity || (section == "TABLES" && pairs[i] == ("0", "LAYER")) {
            let mut j = i + 1;
            let (mut name, mut layer, mut linetype, mut colour, mut weight) = (String::new(), String::new(), String::new(), 0, None);
            while j < pairs.len() && pairs[j].0 != "0" {
                match pairs[j] {
                    ("2", v) => name = v.to_string(),
                    ("8", v) => layer = v.to_string(),
                    ("6", v) => linetype = v.to_string(),
                    ("62", v) => colour = v.parse().unwrap_or(0),
                    ("370", v) => weight = v.parse().ok(),
                    _ => {}
                }
                j += 1;
            }
            if entity {
                entities.push((value.to_string(), layer, weight));
            } else if !name.is_empty() {
                layers.push((name, linetype, colour, weight));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    (entities, layers)
}

/// In a 0.5 mm line group: a 0.5 mm solid line is VISIBLE, a 0.25 mm one THIN, a two-length
/// dash HIDDEN (its linetype HIDDEN), a longer pattern DASHED; a fill's loops are on FILL and a
/// picture is left out and counted. Each entity carries its own width as a standard weight, each
/// layer its kind's; only the layers used are declared, in the writer's order.
#[test]
fn each_kind_of_line_is_on_its_own_layer() {
    let square = Loop2::polygon(&[p(120.0, 10.0), p(130.0, 10.0), p(130.0, 20.0), p(120.0, 20.0)]);
    let items = vec![
        stroke(10.0, 0.5, None),
        stroke(20.0, 0.25, None),
        stroke(30.0, 0.25, Some(vec![3.0, 0.75])),
        stroke(40.0, 0.25, Some(vec![12.0, 3.0, 0.5, 3.0])),
        Item::Fill { region: Region2 { outer: square, holes: vec![] } },
        Item::Picture(Picture { source: 1, rect: Aabb2 { min: p(150.0, 10.0), max: p(170.0, 30.0) }, opacity: 1.0 }),
    ];
    let scene = Scene::new(p(297.0, 210.0), vec![Group { object: 1, kind: ObjectKind::View, items }]);
    let (text, report) = paper_dxf(&scene.prepare().unwrap(), &|_| 0.5).unwrap();
    assert_eq!(report, DxfReport { pictures_left_out: 1 });
    let (entities, layers) = read(&text);
    let names: Vec<&str> = layers.iter().map(|(n, ..)| n.as_str()).collect();
    assert_eq!(names, ["0", "VISIBLE", "THIN", "HIDDEN", "DASHED", "FILL"], "{layers:?}");
    let layer = |name: &str| layers.iter().find(|(n, ..)| n == name).cloned().unwrap();
    assert_eq!(layer("VISIBLE"), ("VISIBLE".into(), "Continuous".into(), 7, Some(50)));
    assert_eq!(layer("HIDDEN"), ("HIDDEN".into(), "HIDDEN".into(), 8, Some(25)));
    assert_eq!(layer("DASHED").1, "DASHED");
    let on: Vec<(&str, Option<i32>)> = entities.iter().map(|(_, l, w)| (l.as_str(), *w)).collect();
    assert_eq!(on, [("VISIBLE", Some(50)), ("THIN", Some(25)), ("HIDDEN", Some(25)), ("DASHED", Some(25)), ("FILL", None)], "{entities:?}");
    assert!(text.contains("$INSUNITS"), "millimetres");
}

/// A line's ends are where the sheet has them, in millimetres; an arc is a polyline with a bulge.
#[test]
fn lines_and_arcs_keep_their_place_on_the_sheet() {
    let arc = Item::Stroke {
        path: vec![
            Seg2::Line { a: p(10.0, 10.0), b: p(50.0, 10.0) },
            Seg2::Arc { center: p(50.0, 20.0), radius: 10.0, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
        ],
        closed: false,
        width_mm: 0.5,
        pattern: None,
    };
    let scene = Scene::new(p(297.0, 210.0), vec![Group { object: 1, kind: ObjectKind::View, items: vec![arc] }]);
    let (text, _) = paper_dxf(&scene.prepare().unwrap(), &|_| 0.5).unwrap();
    let (entities, _) = read(&text);
    assert_eq!(entities.len(), 1, "one polyline: {entities:?}");
    assert!(entities[0].0.contains("POLYLINE"), "{entities:?}");
    // Its vertices (10, 20) and their bulges (42): the line, then a half turn, tan(π/4) = 1.
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let at = lines.iter().position(|l| *l == "AcDbPolyline").unwrap();
    let vertices: Vec<(&str, &str)> = lines[at + 1..]
        .chunks(2)
        .map(|c| (c[0], c[1]))
        .take_while(|(code, _)| *code != "0")
        .filter(|(code, _)| ["10", "20", "42"].contains(code))
        .collect();
    let expected =
        [("10", "10.0"), ("20", "10.0"), ("42", "0.0"), ("10", "50.0"), ("20", "10.0"), ("42", "1.0"), ("10", "50.0"), ("20", "30.0"), ("42", "0.0")];
    assert_eq!(vertices, expected);
}

/// A line group's wide line must be above 0.
#[test]
fn a_wide_line_of_nothing_is_refused() {
    let scene = Scene::new(p(297.0, 210.0), vec![Group { object: 1, kind: ObjectKind::View, items: vec![stroke(10.0, 0.5, None)] }]);
    for wide in [0.0, -1.0, f64::NAN] {
        assert!(paper_dxf(&scene.prepare().unwrap(), &|_| wide).is_err(), "{wide}");
    }
}

/// A view's hidden-line pass can leave a piece far shorter than the DXF grid of 1e-9 mm: a circle
/// seen edge on, cut beside the point where it turns back. Written, such a piece is a point.
/// Alone it adds no entity; between two lines it adds no vertex. The sheet still exports.
#[test]
fn a_piece_shorter_than_the_dxf_grid_is_not_written_and_does_not_refuse_the_sheet() {
    let (a, b) = (p(150.0, 200.0), p(150.0 + 3e-14, 200.0));
    let lone = Item::Stroke { path: vec![Seg2::Line { a, b }], closed: false, width_mm: 0.5, pattern: None };
    let through = Item::Stroke {
        path: vec![Seg2::Line { a: p(100.0, 200.0), b: a }, Seg2::Line { a, b }, Seg2::Line { a: b, b: p(150.0, 250.0) }],
        closed: false,
        width_mm: 0.5,
        pattern: None,
    };
    let scene = Scene::new(p(297.0, 297.0), vec![Group { object: 1, kind: ObjectKind::View, items: vec![lone, through] }]);
    let (text, _) = paper_dxf(&scene.prepare().unwrap(), &|_| 0.5).unwrap();
    let (entities, _) = read(&text);
    assert_eq!(entities.len(), 1, "one polyline, nothing for the lone piece: {entities:?}");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let at = lines.iter().position(|l| *l == "AcDbPolyline").unwrap();
    let vertices: Vec<(&str, &str)> = lines[at + 1..]
        .chunks(2)
        .map(|c| (c[0], c[1]))
        .take_while(|(code, _)| *code != "0")
        .filter(|(code, _)| ["10", "20"].contains(code))
        .collect();
    assert_eq!(vertices, [("10", "100.0"), ("20", "200.0"), ("10", "150.0"), ("20", "200.0"), ("10", "150.0"), ("20", "250.0")]);
}

#[test]
fn text_glyph_outline_with_zero_length_and_sliver_exports_on_text_layer() {
    use solidcraft_geom::Vec2Geom;
    use solidcraft_geom::paper::text::{Cluster, Embed, Font, FontFlags, FontGlyph, FontMetrics, PlacedGlyph, TextRun};

    // Font coordinates are distinct (`g0 != g1`), so `Scene::prepare` admits the glyph, while
    // placing it at `origin = (100, 100)` collapses `g0 -> g1` to a stored zero-length line
    // followed by a sub-grid `3e-14` mm sliver.
    let (g0, g1, g2, g3, g4) = (p(0.0, 0.0), p(1e-16, 0.0), p(3e-14, 0.0), p(20.0, 0.0), p(10.0, 20.0));
    let outline_loop = Loop2 {
        segs: vec![
            Seg2::Line { a: g0, b: g1 },
            Seg2::Line { a: g1, b: g2 },
            Seg2::Line { a: g2, b: g3 },
            Seg2::Line { a: g3, b: g4 },
            Seg2::Line { a: g4, b: g0 },
        ],
    };
    let metrics = FontMetrics {
        ascent: 900,
        descent: -200,
        cap_height: 700,
        bbox: [0, 0, 500, 700],
        italic_angle: 0.0,
        flags: FontFlags::default(),
        stem_v: 80,
    };
    let font = Font {
        id: [1; 32],
        bytes: std::sync::Arc::from(vec![1u8]),
        face: 0,
        postscript_name: "MadeUp".into(),
        family: "Made Up".into(),
        embed: Embed::Subset,
        num_glyphs: 2,
        units_per_em: 1000,
        metrics,
        glyphs: [(1, FontGlyph { advance: 600, outline: vec![outline_loop] })].into(),
    };
    let origin = p(100.0, 100.0);
    let run = TextRun {
        font: [1; 32],
        scale: 1.0,
        origin,
        angle: 0.0,
        slant: 0.0,
        glyphs: vec![PlacedGlyph { id: 1, x: 0.0, rise: 0.0, advance: 600.0 }],
        text: "A".into(),
        clusters: vec![Cluster { glyphs: 0..1, text: 0..1 }],
        rtl: false,
    };
    let placed_outlines: Vec<Loop2> = run.outlines(&font).collect();
    assert!(matches!(placed_outlines[0].segs[0], Seg2::Line { a, b } if a == b));
    assert!(matches!(placed_outlines[0].segs[1], Seg2::Line { a, b } if a != b && a.dist(b) < 1e-12));

    let mut scene = Scene::new(p(297.0, 210.0), vec![Group { object: 1, kind: ObjectKind::View, items: vec![Item::Text(run)] }]);
    scene.fonts.push(font);
    let (text, _) = paper_dxf(&scene.prepare().unwrap(), &|_| 0.5).unwrap();
    let (entities, layers) = read(&text);
    assert_eq!(layers.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>(), ["0", "TEXT"]);
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].1, "TEXT");
    let geom = crate::read_dxf(&text).unwrap();
    assert_eq!(geom.len(), 3, "{geom:?}");
    let (p0, q, r) = (origin + g0, origin + g3, origin + g4);
    for (ea, eb) in [(p0, q), (q, r), (r, p0)] {
        assert!(
            geom.iter().any(|g| matches!(g, crate::Geom2::Line(a, b) if a.dist(ea) < 1e-8 && b.dist(eb) < 1e-8)),
            "missing edge ({ea:?}, {eb:?}) in {geom:?}"
        );
    }
}

/// The sheet's border is drawn in fixed ISO 5457 widths, so its 0.7 mm outline is on VISIBLE and its
/// 0.35 mm grid and title-block cell lines on THIN in every line group, not read against the group's
/// wide line (0.35 mm read as wide at groups 0.25 and 0.35, 0.7 mm as thin from group 1).
#[test]
fn border_and_title_block_lines_keep_their_layer_in_every_line_group() {
    for wide in [0.25, 0.35, 0.5, 0.7, 1.0, 1.4, 2.0] {
        for kind in [ObjectKind::DrawingBorder, ObjectKind::TitleBlock] {
            let items = vec![stroke(10.0, 0.7, None), stroke(20.0, 0.35, None)];
            let scene = Scene::new(p(297.0, 210.0), vec![Group { object: 1, kind, items }]);
            let (text, _) = paper_dxf(&scene.prepare().unwrap(), &|_| wide).unwrap();
            let (entities, _) = read(&text);
            let on: Vec<&str> = entities.iter().map(|(_, l, _)| l.as_str()).collect();
            assert_eq!(on, ["VISIBLE", "THIN"], "{kind:?} in line group {wide}: {entities:?}");
        }
    }
}
