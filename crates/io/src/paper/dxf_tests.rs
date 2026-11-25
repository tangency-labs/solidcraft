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
