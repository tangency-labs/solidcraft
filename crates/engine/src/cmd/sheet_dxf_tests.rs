//! The flat pattern's DXF keeps ellipses and free-form curves as ELLIPSE and SPLINE entities, and
//! a laser-cutting shop can use it as it is: layers by role, bend notes, letters cut or etched,
//! one file per sheet body.

use solidcraft_geom::prelude::*;
use std::f64::consts::{PI, TAU};

use serde_json::{Value, json};
use solidcore_record::{Seg2, Vec2};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// A 100 × 60 base flange (the default 2.5 mm rule) with `curves` drawn in a sketch and cut through it.
fn plate_with(curves: &[(&str, Value)]) -> Session {
    plate_in(Session::default(), curves)
}

/// A 100 × 60 base flange in `s` (of its rule), with `curves` drawn in a sketch and cut through it.
fn plate_in(mut s: Session, curves: &[(&str, Value)]) -> Session {
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    if curves.is_empty() {
        return s;
    }
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cut"}));
    for (command, params) in curves {
        run(&mut s, command, params.clone());
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Cut", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    s
}

/// A session whose active rule is 2 mm sheet with an inner bend radius of 1 mm.
fn sheet_of_2mm() -> Session {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "Sheet2", "thickness": "2 mm", "k_factor": "0.5", "bend_radius": "1 mm", "active": true}));
    s
}

/// A 100 × 60 tray of 2 mm sheet with `curves` cut through its base: three 20 mm walls bend up and
/// one bends down.
fn tray_with(curves: &[(&str, Value)]) -> Session {
    let mut s = plate_in(sheet_of_2mm(), curves);
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2], [100, 30, 2], [50, 60, 2]], "height": 20}));
    run(&mut s, "sheet.flange", json!({"edges": [[0, 30, 2]], "height": 20, "flip": true}));
    s
}

/// What a tray is marked with: a Ø5 hole, a 20 × 5 slot and the label "AB-12", 10 mm high at (20, 28).
fn marks() -> Vec<(&'static str, Value)> {
    vec![
        ("sketch.circle.center", json!({"center": [15, 15], "diameter": 5})),
        ("sketch.rectangle.two_point", json!({"p0": [70, 40], "p1": [90, 45]})),
        ("sketch.text", json!({"text": "AB-12", "at": [20, 28], "height": 10})),
    ]
}

fn export(s: &mut Session, params: Value) -> String {
    run(s, "sheet.export_dxf", params)["dxf"].as_str().unwrap_or_default().to_string()
}

/// The entities of a DXF text: type and group codes.
type Entity = (String, Vec<(i32, String)>);

fn entities(dxf: &str) -> Vec<Entity> {
    let lines: Vec<&str> = dxf.lines().collect();
    let pairs: Vec<(i32, &str)> = lines.chunks(2).filter_map(|c| Some((c.first()?.trim().parse().ok()?, c.get(1)?.trim()))).collect();
    let start = pairs.iter().position(|p| *p == (2, "ENTITIES")).unwrap_or(0);
    let mut out: Vec<Entity> = Vec::new();
    for (code, value) in pairs.iter().skip(start + 1) {
        match (*code, *value) {
            (0, "ENDSEC") => break,
            (0, kind) => out.push((kind.to_string(), Vec::new())),
            _ => {
                if let Some(last) = out.last_mut() {
                    last.1.push((*code, value.to_string()));
                }
            }
        }
    }
    out
}

fn values(e: &Entity, code: i32) -> Vec<f64> {
    e.1.iter().filter(|(c, _)| *c == code).filter_map(|(_, v)| v.parse().ok()).collect()
}

fn count(es: &[Entity], kind: &str) -> usize {
    es.iter().filter(|e| e.0 == kind).count()
}

fn point(e: &Entity, code: i32) -> Vec2 {
    Vec2::new(values(e, code).first().copied().unwrap_or(f64::NAN), values(e, code + 10).first().copied().unwrap_or(f64::NAN))
}

/// All loops of the flat pattern (outlines, cut-outs, islands).
fn loops(s: &Session) -> Vec<solidcore_record::Loop2> {
    let regions = s.model.state().sheets[0].flat_regions().unwrap_or_default();
    regions.iter().flat_map(|r| std::iter::once(r.outer.clone()).chain(r.holes.clone())).collect()
}

/// The profiles of the DXF read into a new sketch: their areas, largest first.
fn reimported_areas(dxf: &str) -> Vec<f64> {
    let mut s = Session::default();
    let id = run(&mut s, "sketch.insert_dxf", json!({"text": dxf, "name": "Back"}))["sketch"].as_u64().unwrap_or(0);
    run(&mut s, "sketch.finish", json!({}));
    let mut areas: Vec<f64> = s.model.state().sketch(id).map(|ss| ss.profiles.iter().map(|p| p.area).collect()).unwrap_or_default();
    areas.sort_by(|a, b| b.total_cmp(a));
    areas
}

#[test]
fn an_elliptic_cut_out_is_one_ellipse() {
    let mut s = plate_with(&[("sketch.ellipse", json!({"center": [40, 30], "major": [55, 30], "minor_radius": 8}))]);
    let dxf = export(&mut s, json!({}));
    let es = entities(&dxf);
    // The four lines of the outline and one ELLIPSE: no other entity.
    assert_eq!((count(&es, "LINE"), count(&es, "ELLIPSE"), es.len()), (4, 1, 5), "{:?}", es.iter().map(|e| &e.0).collect::<Vec<_>>());
    // By hand: centre (40, 30), the major semi-axis 15 along x (to (55, 30) or (25, 30)), the
    // minor one 8, so the ratio is 8/15; the whole turn runs from parameter 0 to 2π.
    let e = es.iter().find(|e| e.0 == "ELLIPSE").unwrap();
    let (c, major) = (point(e, 10), point(e, 11));
    assert!(c.dist(Vec2::new(40.0, 30.0)) < 1e-9 && (major.x.abs() - 15.0).abs() < 1e-9 && major.y.abs() < 1e-9, "{c:?} {major:?}");
    assert!((values(e, 40)[0] - 8.0 / 15.0).abs() < 1e-9 && values(e, 41)[0] == 0.0 && (values(e, 42)[0] - TAU).abs() < 1e-9);
    assert_eq!(values(e, 230), [1.0]);
    // Our reader gets the same ellipse back, and the same plate: 6000 mm² less π·15·8.
    let read = solidcraft_io::read_dxf(&dxf).unwrap();
    assert!(read.iter().any(|g| matches!(g, solidcraft_io::Geom2::Ellipse { c, major, minor }
        if c.dist(Vec2::new(40.0, 30.0)) < 1e-9 && (major.dist(*c) - 15.0).abs() < 1e-9 && (minor - 8.0).abs() < 1e-9)));
    let areas = reimported_areas(&dxf);
    assert!(areas.len() == 2 && (areas[0] - (6000.0 - PI * 120.0)).abs() < 1e-6 && (areas[1] - PI * 120.0).abs() < 1e-6, "{areas:?}");
}

#[test]
fn a_spline_cut_out_is_the_splines_it_was() {
    let points = json!([[20, 15], [40, 45], [65, 40], [85, 20]]);
    let mut s = plate_with(&[("sketch.spline.fit_point", json!({"points": points})), ("sketch.line", json!({"points": [[85, 20], [20, 15]]}))]);
    let dxf = export(&mut s, json!({}));
    let es = entities(&dxf);
    // The cut-out's segments of the flat pattern, one entity each: its cubic pieces are SPLINEs
    // of degree 3 over the same four control points, its line a LINE.
    let hole: Vec<Seg2> = loops(&s).get(1).map(|l| l.segs.clone()).unwrap_or_default();
    let cubics: Vec<[Vec2; 4]> =
        hole.iter().filter_map(|g| if let Seg2::Cubic { p0, p1, p2, p3 } = g { Some([*p0, *p1, *p2, *p3]) } else { None }).collect();
    assert!(!cubics.is_empty() && cubics.len() + 1 == hole.len(), "{hole:?}");
    assert_eq!((count(&es, "LINE"), count(&es, "SPLINE"), es.len()), (5, cubics.len(), 5 + cubics.len()));
    for (spline, want) in es.iter().filter(|e| e.0 == "SPLINE").zip(&cubics) {
        assert_eq!((values(spline, 70), values(spline, 71), values(spline, 72), values(spline, 73)), (vec![8.0], vec![3.0], vec![8.0], vec![4.0]));
        assert_eq!(values(spline, 40), [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        let (xs, ys) = (values(spline, 10), values(spline, 20));
        assert!(want.iter().enumerate().all(|(i, p)| (xs[i] - p.x).abs() < 1e-9 && (ys[i] - p.y).abs() < 1e-9), "{xs:?} {ys:?} {want:?}");
    }
    // The same plate comes back.
    let areas = reimported_areas(&dxf);
    let plate = s.model.state().sheets[0].flat_regions().unwrap_or_default()[0].area();
    assert!((areas[0] - plate).abs() < 1e-6, "{areas:?} {plate}");
}

#[test]
fn a_g4_blend_cut_out_is_one_spline_of_degree_nine_and_closes() {
    // Three sides of a 20 × 20 square, closed on the right by a G4 blend from (40, 20) to (40, 40):
    // one Bézier of degree 9 over ten control points (solidcraft#5269).
    let mut s = plate_with(&[]);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cut"}));
    let mut line =
        |a: [i32; 2], b: [i32; 2]| run(&mut s, "sketch.line", json!({"points": [a, b]}))["curves"][0].as_str().unwrap_or_default().to_string();
    let (bottom, top) = (line([20, 20], [40, 20]), line([40, 40], [20, 40]));
    line([20, 40], [20, 20]);
    run(&mut s, "sketch.blend_curve", json!({"a": format!("{bottom}.end"), "b": format!("{top}.start"), "continuity": "g4"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Cut", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    let hole: Vec<Seg2> = loops(&s).get(1).map(|l| l.segs.clone()).unwrap_or_default();
    let blend: Vec<Vec2> = hole.iter().find_map(|g| if let Seg2::Bezier { n: 9, p } = g { Some(p[..10].to_vec()) } else { None }).unwrap_or_default();
    assert_eq!((blend.len(), hole.len()), (10, 4), "{hole:?}");

    // One SPLINE of degree 9 with all ten control points: its ends are the blend's own.
    let dxf = export(&mut s, json!({}));
    let es = entities(&dxf);
    assert_eq!((count(&es, "LINE"), count(&es, "SPLINE"), es.len()), (7, 1, 8));
    let spline = es.iter().find(|e| e.0 == "SPLINE").unwrap();
    assert_eq!((values(spline, 70), values(spline, 71), values(spline, 72), values(spline, 73)), (vec![8.0], vec![9.0], vec![20.0], vec![10.0]));
    let ctrl: Vec<Vec2> = values(spline, 10).into_iter().zip(values(spline, 20)).map(|(x, y)| Vec2::new(x, y)).collect();
    assert!(ctrl.len() == 10 && ctrl.iter().zip(&blend).all(|(a, b)| a.dist(*b) < 1e-9), "{ctrl:?} {blend:?}");
    let ends = [ctrl[0], ctrl[9]];
    assert!(ends.contains(&Vec2::new(40.0, 20.0)) && ends.contains(&Vec2::new(40.0, 40.0)), "{ends:?}");

    // The cut-out closes, so the same plate and the same cut-out come back.
    let areas = reimported_areas(&dxf);
    let plate = s.model.state().sheets[0].flat_regions().unwrap_or_default()[0].area();
    assert!(areas.len() == 2 && (areas[0] - plate).abs() < 1e-6 && (areas[1] - (6000.0 - plate)).abs() < 1e-6, "{areas:?} {plate}");
}

#[test]
fn text_is_splines_and_lines_not_chords() {
    let mut s = plate_with(&[("sketch.text", json!({"text": "AB", "at": [30, 20], "height": 12}))]);
    let dxf = export(&mut s, json!({}));
    let es = entities(&dxf);
    // One entity for each segment of the flat pattern: a straight one a LINE, a glyph's
    // quadratic and cubic curves SPLINEs of degree 2 and 3.
    let segs: Vec<Seg2> = loops(&s).iter().flat_map(|l| l.segs.clone()).collect();
    let n = |f: &dyn Fn(&Seg2) -> bool| segs.iter().filter(|g| f(g)).count();
    let (lines, quadratics, cubics) = (
        n(&|g| matches!(g, Seg2::Line { .. })),
        n(&|g| matches!(g, Seg2::Conic { w, .. } if (*w - 1.0).abs() < 1e-12)),
        n(&|g| matches!(g, Seg2::Cubic { .. })),
    );
    assert_eq!(lines + quadratics + cubics, segs.len(), "only lines and Béziers");
    assert_eq!((count(&es, "LINE"), count(&es, "SPLINE"), es.len()), (lines, quadratics + cubics, segs.len()));
    let degree = |d: f64| es.iter().filter(|e| e.0 == "SPLINE" && values(e, 71) == [d]).count();
    assert_eq!((degree(2.0), degree(3.0)), (quadratics, cubics));
    // The plate with the letters cut out comes back from the file with its area (the largest region; the islands inside the
    // letters come in no fixed order).
    let areas = reimported_areas(&dxf);
    let plate = s.model.state().sheets[0].flat_regions().unwrap_or_default().iter().map(|r| r.area()).fold(0.0, f64::max);
    assert!((areas[0] - plate).abs() < 1e-3, "{areas:?} {plate}");
}

#[test]
fn arcs_option_writes_only_lines_and_tangent_arcs_within_a_thousandth() {
    let mut s = plate_with(&[("sketch.ellipse", json!({"center": [40, 30], "major": [55, 30], "minor_radius": 8}))]);
    let dxf = export(&mut s, json!({"curves": "arcs"}));
    let es = entities(&dxf);
    assert!(es.iter().all(|e| e.0 == "LINE" || e.0 == "ARC"), "{:?}", es.iter().map(|e| &e.0).collect::<Vec<_>>());
    let arcs: Vec<(Vec2, f64, [Vec2; 2], Vec<Vec2>)> = es
        .iter()
        .filter(|e| e.0 == "ARC")
        .map(|e| {
            let (c, r, (a0, a1)) = (point(e, 10), values(e, 40)[0], (values(e, 50)[0].to_radians(), values(e, 51)[0].to_radians()));
            let sweep = (a1 - a0).rem_euclid(TAU);
            let samples = (0..=20).map(|k| c + Vec2::from_angle(a0 + sweep * f64::from(k) / 20.0) * r).collect::<Vec<Vec2>>();
            (c, r, [c + Vec2::from_angle(a0) * r, c + Vec2::from_angle(a1) * r], samples)
        })
        .collect();
    // Far fewer than the 336 chords of 0.001 mm.
    assert!(arcs.len() >= 8 && arcs.len() < 100, "{} arcs", arcs.len());
    // Distance to the ellipse ((x − 40)/15)² + ((y − 30)/8)² = 1, to first order |f| / |∇f|.
    let off = |p: Vec2| {
        let (x, y) = (p.x - 40.0, p.y - 30.0);
        let f = (x / 15.0).powi(2) + (y / 8.0).powi(2) - 1.0;
        f.abs() / Vec2::new(2.0 * x / 225.0, 2.0 * y / 64.0).len()
    };
    // (The joint of a biarc's two arcs lies within the tolerance of the curve, not on it.)
    for (_, _, ends, samples) in &arcs {
        let offs: Vec<f64> = samples.iter().map(|p| off(*p)).collect();
        assert!(offs.iter().all(|d| *d <= 1e-3 + 1e-6), "{ends:?}: {offs:?}");
    }
    // Tangent: at each joint the radii of the two arcs lie on one line.
    for (i, (c, r, ends, _)) in arcs.iter().enumerate() {
        for end in ends {
            let others: Vec<_> = arcs.iter().enumerate().filter(|(j, (_, _, e, _))| *j != i && e.iter().any(|q| q.dist(*end) < 1e-6)).collect();
            assert_eq!(others.len(), 1, "joint {end:?}");
            let (_, (c2, r2, _, _)) = others[0];
            assert!((*end - *c).cross(*end - *c2).abs() < 1e-6 * r * r2, "arcs are not tangent at {end:?}");
        }
    }
    // The plate, read back, is the exact one within the perimeter times 0.001 mm.
    let areas = reimported_areas(&dxf);
    assert!((areas[0] - (6000.0 - PI * 120.0)).abs() < 0.1, "{areas:?}");
}

#[test]
fn arcs_option_has_no_splines_for_text_and_the_curves_value_is_checked() {
    let mut s = plate_with(&[("sketch.text", json!({"text": "AB", "at": [30, 20], "height": 12}))]);
    let es = entities(&export(&mut s, json!({"curves": "ARCS"})));
    assert!(es.iter().all(|e| e.0 == "LINE" || e.0 == "ARC") && count(&es, "ARC") > 0);
    let exact = entities(&export(&mut s, json!({"curves": "exact"})));
    assert!(count(&exact, "SPLINE") > 0 && count(&exact, "ARC") == 0);
    assert!(s.execute("sheet.export_dxf", &json!({"curves": "chords"})).is_err());
}

#[test]
fn bend_lines_and_layers_are_still_there() {
    let mut s = plate_with(&[("sketch.ellipse", json!({"center": [40, 30], "major": [55, 30], "minor_radius": 8}))]);
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20}));
    let out = run(&mut s, "sheet.export_dxf", json!({}));
    let es = entities(out["dxf"].as_str().unwrap_or_default());
    let on = |layer: &str| es.iter().filter(|e| e.1.contains(&(8, layer.to_string()))).count();
    assert_eq!(on("BEND_UP") + on("BEND_DOWN"), out["bends"].as_u64().unwrap_or(0) as usize);
    // The ellipse is a cut-out, on CUT; the outline is the plate's.
    assert!(on("BEND_UP") + on("BEND_DOWN") > 0 && count(&es, "ELLIPSE") == 1 && on("CUT") == 1);
    assert_eq!(on("OUTLINE") + on("CUT"), es.len() - on("BEND_UP") - on("BEND_DOWN"));
}

/// The layer an entity is on.
fn layer_of(e: &Entity) -> String {
    e.1.iter().find(|(c, _)| *c == 8).map(|(_, v)| v.clone()).unwrap_or_default()
}

/// The text of an entity's group code.
fn string(e: &Entity, code: i32) -> String {
    e.1.iter().find(|(c, _)| *c == code).map(|(_, v)| v.clone()).unwrap_or_default()
}

/// What the entities of a layer are, without their handles and the layer: to compare layers of two files.
fn on_layer(es: &[Entity], layer: &str) -> Vec<(String, Vec<(i32, String)>)> {
    es.iter().filter(|e| layer_of(e) == layer).map(|e| (e.0.clone(), e.1.iter().filter(|(c, _)| ![5, 8].contains(c)).cloned().collect())).collect()
}

/// The LAYER table: name → (colour, linetype).
fn layer_table(dxf: &str) -> std::collections::BTreeMap<String, (i64, String)> {
    let lines: Vec<&str> = dxf.lines().collect();
    let pairs: Vec<(i32, &str)> = lines.chunks(2).filter_map(|c| Some((c.first()?.trim().parse().ok()?, c.get(1)?.trim()))).collect();
    let start = pairs.iter().position(|p| *p == (2, "LAYER")).unwrap_or(0);
    let mut records: Vec<Vec<(i32, &str)>> = Vec::new();
    for p in pairs.iter().skip(start + 1) {
        match *p {
            (0, "ENDTAB") => break,
            (0, _) => records.push(Vec::new()),
            _ => {
                if let Some(record) = records.last_mut() {
                    record.push(*p);
                }
            }
        }
    }
    records
        .iter()
        .filter_map(|r| {
            let get = |code: i32| r.iter().find(|(c, _)| *c == code).map(|(_, v)| *v);
            Some((get(2)?.to_string(), (get(62)?.parse().ok()?, get(6)?.to_string())))
        })
        .collect()
}

fn vec2(v: &Value) -> Vec2 {
    Vec2::new(v[0].as_f64().unwrap_or(f64::NAN), v[1].as_f64().unwrap_or(f64::NAN))
}

#[test]
fn the_flat_pattern_is_on_layers_by_role_and_inner_contours_come_first() {
    let mut s = tray_with(&marks());
    let out = run(&mut s, "sheet.export_dxf", json!({}));
    let dxf = out["dxf"].as_str().unwrap_or_default();
    let es = entities(dxf);
    let on = |layer: &str| es.iter().filter(|e| layer_of(e) == layer).collect::<Vec<_>>();
    // OUTLINE: the tray's outer contour alone, one entity for each of its segments (lines and arcs).
    let regions = s.model.state().sheets[0].flat_regions().unwrap_or_default();
    let plate =
        regions.iter().max_by(|a, b| a.area().total_cmp(&b.area())).map(|r| r.outer.clone()).unwrap_or(solidcore_record::Loop2 { segs: Vec::new() });
    assert_eq!(
        on("OUTLINE").len(),
        solidcraft_io::path_shapes(solidcraft_io::DxfPath::Closed(&plate), solidcraft_io::DxfPathStyle::Exact).unwrap().len(),
    );
    assert!(on("OUTLINE").iter().all(|e| e.0 == "LINE" || e.0 == "ARC"), "{:?}", on("OUTLINE").iter().map(|e| &e.0).collect::<Vec<_>>());
    // CUT: what is cut out inside it that is no text: the Ø5 hole as one CIRCLE, the slot's four lines.
    let cut = on("CUT");
    assert_eq!((count_of(&cut, "CIRCLE"), count_of(&cut, "LINE"), cut.len()), (1, 4, 5), "{:?}", cut.iter().map(|e| &e.0).collect::<Vec<_>>());
    // TEXT: the letters of "AB-12" and the counters of A and B, all within the label's box (5 letters
    // of at most 10 mm from (20, 28)).
    let letters = on("TEXT");
    assert!(letters.len() > 20, "{} letter entities", letters.len());
    for e in &letters {
        let p = point(e, 10);
        assert!(p.x > 19.0 && p.x < 80.0 && p.y > 27.0 && p.y < 39.0, "{} at {p:?}", e.0);
    }
    // The bend lines by direction: three walls up, one down.
    assert_eq!((on("BEND_UP").len(), on("BEND_DOWN").len(), out["bends"].as_u64()), (3, 1, Some(4)));
    // Inner contours are written before the outline, so a program that follows the file cuts inside-out.
    let last_inner = es.iter().rposition(|e| matches!(layer_of(e).as_str(), "CUT" | "TEXT"));
    let first_outline = es.iter().position(|e| layer_of(e) == "OUTLINE");
    let (Some(last_inner), Some(first_outline)) = (last_inner, first_outline) else { panic!("{last_inner:?} {first_outline:?}") };
    assert!(last_inner < first_outline, "an inner contour at {last_inner}, the outline from {first_outline}");
    // The layers, with the colours and linetypes the guide gives; nothing else is declared.
    let want = [
        ("0", 7, "Continuous"),
        ("OUTLINE", 7, "Continuous"),
        ("CUT", 4, "Continuous"),
        ("TEXT", 2, "Continuous"),
        ("BEND_UP", 1, "DASHED"),
        ("BEND_DOWN", 5, "DASHED"),
    ];
    assert_eq!(layer_table(dxf), want.map(|(n, c, l)| (n.to_string(), (c, l.to_string()))).into_iter().collect());
}

/// How many entities of `kind` there are among `list`.
fn count_of(list: &[&Entity], kind: &str) -> usize {
    list.iter().filter(|e| e.0 == kind).count()
}

#[test]
fn letters_can_be_etched_instead_of_cut() {
    let mut s = tray_with(&marks());
    let cut = entities(&export(&mut s, json!({})));
    let etched_dxf = export(&mut s, json!({"text": "etch"}));
    let etched = entities(&etched_dxf);
    // The same contours, on ETCH instead of TEXT: nothing of them is cut.
    assert!(!on_layer(&cut, "TEXT").is_empty());
    assert_eq!(on_layer(&etched, "ETCH"), on_layer(&cut, "TEXT"));
    assert!(on_layer(&etched, "TEXT").is_empty());
    // The hole and the slot are no text: they stay where they were, as do the outline and the bends.
    for layer in ["OUTLINE", "CUT", "BEND_UP", "BEND_DOWN"] {
        assert_eq!(on_layer(&etched, layer), on_layer(&cut, layer), "{layer}");
    }
    let layers = layer_table(&etched_dxf);
    assert_eq!(layers.get("ETCH"), Some(&(6, "Continuous".to_string())));
    assert!(!layers.contains_key("TEXT"));
    // `cut` is the default, and a value that is neither is refused.
    assert_eq!(export(&mut s, json!({"text": "cut"})), export(&mut s, json!({})));
    assert!(s.execute("sheet.export_dxf", &json!({"text": "fill"})).is_err());
}

#[test]
fn engraved_text_is_etched_and_other_pockets_keep_their_depth_layer() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [70, 40], "p1": [90, 45]}));
    run(&mut s, "sketch.text", json!({"text": "AB-12", "at": [20, 28], "height": 10}));
    run(&mut s, "sketch.finish", json!({}));
    // Part way through the sheet: pockets, 1 mm deep from the bottom.
    run(&mut s, "solid.extrude", json!({"sketch": "Marks", "distance": 1, "operation": "cut"}));
    let dxf = export(&mut s, json!({}));
    let es = entities(&dxf);
    // The rectangle is a pocket of the depth its layer names; the letters are a marking, on ETCH.
    let pocket = "POCKET_BOTTOM_Z0.000000_TO_1.000000_MM";
    let on = |layer: &str| es.iter().filter(|e| layer_of(e) == layer).collect::<Vec<_>>();
    assert_eq!((count_of(&on(pocket), "LINE"), on(pocket).len()), (4, 4));
    let letters = on("ETCH");
    assert!(letters.len() > 20, "{} letter entities", letters.len());
    for e in &letters {
        let p = point(e, 10);
        assert!(p.x > 19.0 && p.x < 80.0 && p.y > 27.0 && p.y < 39.0, "{} at {p:?}", e.0);
    }
    let layers = layer_table(&dxf);
    assert!(layers.contains_key(pocket) && layers.contains_key("ETCH") && !layers.contains_key("TEXT"), "{layers:?}");
    // `text` is about letters cut through: it changes nothing here.
    assert_eq!(on_layer(&entities(&export(&mut s, json!({"text": "etch"}))), "ETCH"), on_layer(&es, "ETCH"));
}

#[test]
fn a_bend_note_sits_beside_each_bend_line_when_asked_for() {
    let mut s = tray_with(&[]);
    let flat = run(&mut s, "sheet.flat_pattern", json!({}));
    let bends = flat["bends"].as_array().cloned().unwrap_or_default();
    assert_eq!(bends.len(), 4);
    // Not unless asked for: a program that follows the file would etch them.
    let plain = export(&mut s, json!({}));
    assert!(entities(&plain).iter().all(|e| e.0 != "TEXT") && !layer_table(&plain).contains_key("BEND_NOTES"));
    let dxf = export(&mut s, json!({"bend_notes": true}));
    let notes: Vec<Entity> = entities(&dxf).into_iter().filter(|e| e.0 == "TEXT").collect();
    assert_eq!(notes.len(), 4);
    assert!(notes.iter().all(|e| layer_of(e) == "BEND_NOTES"));
    assert_eq!(layer_table(&dxf).get("BEND_NOTES"), Some(&(8, "Continuous".to_string())));
    for b in &bends {
        // The walls bend by 90° round an inner radius of 1 mm: "B1 UP 90° R1" (`%%d` is the degree sign).
        let up = b["direction"] == "up";
        let text = format!("B{} {} 90%%d R1", b["bend"].as_u64().unwrap_or(0), if up { "UP" } else { "DOWN" });
        let Some(note) = notes.iter().find(|e| string(e, 1) == text) else {
            panic!("no note `{text}` among {:?}", notes.iter().map(|e| string(e, 1)).collect::<Vec<_>>())
        };
        // Centred (72 = 1), 3 mm high, parallel to the line: along x it reads left to right, along y upwards.
        let (start, end) = (vec2(&b["start"]), vec2(&b["end"]));
        let angle = if (end - start).x.abs() > (end - start).y.abs() { 0.0 } else { 90.0 };
        assert!(values(note, 72) == [1.0] && values(note, 40) == [3.0] && (values(note, 50)[0] - angle).abs() < 1e-9, "{note:?}");
        // On the left of the way it reads, half its height from the middle of the line.
        let off = point(note, 11) - (start + end) * 0.5;
        assert!(off.dist(Vec2::from_angle(angle.to_radians()).perp() * 1.5) < 1e-9, "{text}: {off:?}");
    }
    // The height is the caller's.
    let tall = entities(&export(&mut s, json!({"bend_notes": true, "note_height": 5})));
    let note = tall.iter().find(|e| e.0 == "TEXT");
    assert!(note.is_some_and(|e| values(e, 40) == [5.0]));
}

#[test]
fn a_bend_note_gives_the_included_angle_when_the_rule_reads_it_so() {
    // The included angle is 180° less the bend angle: 122° for a 58° bend, 90° for a 90° one.
    for (deg, included) in [(58.0, 122.0), (90.0, 90.0)] {
        let mut plain = Vec::new();
        for display in ["bend", "included"] {
            let mut s = sheet_of_2mm();
            run(&mut s, "sheet.manage_rules", json!({"name": "Sheet2", "angle_display": display}));
            let mut s = plate_in(s, &[]);
            run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2]], "height": 20, "angle": format!("{deg} deg")}));
            let dxf = export(&mut s, json!({"bend_notes": true}));
            let notes: Vec<String> = entities(&dxf).iter().filter(|e| e.0 == "TEXT").map(|e| string(e, 1)).collect();
            let shown = if display == "included" { included } else { deg };
            assert_eq!(notes, [format!("B1 UP {shown}%%d R1")], "{deg}° under {display}");
            plain.push(export(&mut s, json!({})));
        }
        // Without notes the setting changes nothing in the file.
        assert_eq!(plain.first(), plain.get(1), "{deg}°");
    }
}

/// A body whose base flange names its rule keeps that rule's notes whichever rule is active when
/// the DXF is written, and a bend down reads its included angle the same way.
#[test]
fn a_bend_note_follows_the_bodys_own_rule_and_bends_down_too() {
    let mut s = sheet_of_2mm();
    run(&mut s, "sheet.manage_rules", json!({"name": "Sheet2", "angle_display": "included"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base", "rule": "Sheet2"}));
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2]], "height": 20, "angle": "58 deg", "flip": true}));
    let notes = |s: &mut Session| -> Vec<String> {
        entities(&export(s, json!({"bend_notes": true}))).iter().filter(|e| e.0 == "TEXT").map(|e| string(e, 1)).collect()
    };
    assert_eq!(notes(&mut s), ["B1 DOWN 122%%d R1"]);
    run(&mut s, "sheet.manage_rules", json!({"name": "Other", "thickness": "2 mm", "angle_display": "bend", "active": true}));
    assert_eq!(notes(&mut s), ["B1 DOWN 122%%d R1"], "another active rule does not change this body's notes");
}

/// Two sheet bodies: a lid and, 200 mm away, a side wall with one bend.
fn lid_and_wall() -> Session {
    let mut s = Session::default();
    for (sketch, p0, p1, body) in [("Lid", [0, 0], [100, 60], "Lid"), ("Wall", [200, 0], [250, 40], "Side Wall")] {
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": sketch}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": sketch, "body_name": body}));
    }
    run(&mut s, "sheet.flange", json!({"edges": [[225, 0, 2.5]], "height": 15, "body": "Side Wall"}));
    s
}

#[test]
fn each_sheet_body_can_have_a_file_of_its_own() {
    let mut s = lid_and_wall();
    let r = run(&mut s, "sheet.export_dxf", json!({"per_body": true}));
    let files = r["files"].as_array().cloned().unwrap_or_default();
    assert_eq!(files.iter().map(|f| f["body"].as_str().unwrap_or_default()).collect::<Vec<_>>(), ["Lid", "Side Wall"]);
    assert_eq!(files.iter().map(|f| f["bends"].as_u64()).collect::<Vec<_>>(), [Some(0), Some(1)]);
    // Each is the file `body` asks for: the lid's has no bend line, the wall's has its one.
    for f in &files {
        let single = export(&mut s, json!({"body": f["body"]}));
        assert_eq!(f["dxf"].as_str(), Some(single.as_str()), "{}", f["body"]);
    }
    let bend_lines = |f: &Value| entities(f["dxf"].as_str().unwrap_or_default()).iter().filter(|e| layer_of(e) == "BEND_UP").count();
    assert_eq!((bend_lines(&files[0]), bend_lines(&files[1])), (0, 1));
    // With a path, each body is written to a file named after it, beside the path, and no text comes back.
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-dxf-per-body-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_default();
    let path = dir.join("panel.dxf").to_string_lossy().to_string();
    let r = run(&mut s, "sheet.export_dxf", json!({"per_body": true, "path": path}));
    let written = r["files"].as_array().cloned().unwrap_or_default();
    let names: Vec<_> = written
        .iter()
        .map(|f| std::path::Path::new(f["path"].as_str().unwrap_or_default()).file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    assert_eq!(names, [Some("panel-Lid.dxf".to_string()), Some("panel-Side Wall.dxf".to_string())]);
    for (f, written) in files.iter().zip(&written) {
        assert!(written.get("dxf").is_none());
        let on_disk = std::fs::read_to_string(written["path"].as_str().unwrap_or_default()).unwrap_or_default();
        assert_eq!(f["dxf"].as_str(), Some(on_disk.as_str()));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_export_options_are_refused_not_ignored() {
    let mut s = tray_with(&[]);
    for p in [
        json!({"text": "fill"}),
        json!({"text": 1}),
        json!({"bend_notes": "yes"}),
        json!({"bend_notes": 1}),
        json!({"note_height": 0}),
        json!({"note_height": -3}),
        json!({"note_height": 1e9}),
        json!({"note_height": "tall"}),
        json!({"per_body": "yes"}),
        json!({"per_body": true, "body": "Body1"}),
    ] {
        assert!(s.execute("sheet.export_dxf", &p).is_err(), "{p}");
    }
    // Nothing was left half done.
    assert!(s.execute("sheet.export_dxf", &json!({})).is_ok());
}

#[test]
fn the_file_of_a_body_is_named_after_it_and_no_two_share_a_name() {
    // (Windows writes the separator it adds as `\`.)
    let names = |path: &str, bodies: &[&str]| -> Vec<String> {
        super::body_paths(path, &bodies.iter().map(|b| b.to_string()).collect::<Vec<_>>()).into_iter().map(|p| p.replace('\\', "/")).collect()
    };
    assert_eq!(names("out/panel.dxf", &["Lid", "Side Wall"]), ["out/panel-Lid.dxf", "out/panel-Side Wall.dxf"]);
    // The extension is the path's, and .dxf when it has none.
    assert_eq!(names("out/panel", &["Lid"]), ["out/panel-Lid.dxf"]);
    assert_eq!(names("panel.DXF", &["Lid"]), ["panel-Lid.DXF"]);
    // What a file name cannot hold becomes `_`; a name that is nothing but that stays a name.
    assert_eq!(names("p.dxf", &["a/b:c", "  ", "wall."]), ["p-a_b_c.dxf", "p-body.dxf", "p-wall.dxf"]);
    // Two bodies that would share a file (in any case) get a number.
    assert_eq!(names("p.dxf", &["Lid", "lid", "LID", "Lid_2"]), ["p-Lid.dxf", "p-lid_2.dxf", "p-LID_3.dxf", "p-Lid_2_2.dxf"]);
}

/// #4314: a cutter that cuts every line gets a file without the bend lines, or with everything
/// on layer 0. The default keeps the role layers and the dashed bend lines.
#[test]
fn bend_lines_can_be_left_out_and_everything_put_on_layer_0() {
    let mut s = tray_with(&marks());
    let layer = |e: &Entity| e.1.iter().find(|(c, _)| *c == 8).map(|(_, v)| v.clone()).unwrap_or_default();
    let default = entities(&export(&mut s, json!({})));
    let bends = |es: &[Entity]| es.iter().filter(|e| matches!(layer(e).as_str(), "BEND_UP" | "BEND_DOWN")).count();
    assert_eq!(bends(&default), 4, "the tray's four bends");
    let without = entities(&export(&mut s, json!({"bend_lines": false})));
    assert_eq!(bends(&without), 0);
    assert_eq!(without.len(), default.len() - 4, "only the bend lines go");
    let single = entities(&export(&mut s, json!({"layers": "single"})));
    assert_eq!(single.len(), default.len());
    assert!(single.iter().all(|e| layer(e) == "0"), "{:?}", single.iter().map(layer).collect::<Vec<_>>());
    for bad in [json!({"bend_lines": "no"}), json!({"layers": "one"})] {
        assert!(s.execute("sheet.export_dxf", &bad).is_err(), "{bad}");
    }
}

/// The DXF of a sheet folded across a curved outer outline has the curve as drawn, not arcs fitted
/// to it, and no seams where the fold cut it: the blank, with its bend line where the bend is.
#[test]
fn a_curved_outline_folded_across_stays_its_curve_in_the_dxf() {
    let blanks: [(&str, &str, Value, &str); 3] = [
        ("circle", "sketch.circle.center", json!({"center": [0, 0], "radius": 50}), "CIRCLE"),
        ("ellipse", "sketch.ellipse", json!({"center": [0, 0], "major": [80, 0], "minor_radius": 40}), "ELLIPSE"),
        (
            "spline",
            "sketch.spline.fit_point",
            json!({"points": [[70, -5], [40, 40], [0, 40], [-50, 35], [-85, 0], [-40, -50], [0, -35], [35, -25]], "periodic": true}),
            "SPLINE",
        ),
    ];
    for (name, id, p, kind) in blanks {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Blank"}));
        run(&mut s, id, p);
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Blank"}));
        let flat_loops = loops(&s);
        let area: f64 = flat_loops.iter().map(|l| l.signed_area().abs()).sum();
        run(&mut s, "sheet.fold", json!({"points": [[10, -200, 2.5], [10, 200, 2.5]]}));
        let dxf = export(&mut s, json!({}));
        let es = entities(&dxf);
        assert!(count(&es, kind) >= 1, "{name}: no {kind}: {:?}", es.iter().map(|e| e.0.as_str()).collect::<Vec<_>>());
        if kind != "CIRCLE" {
            assert_eq!(count(&es, "ARC"), 0, "{name}: curves came back as fitted arcs");
        }
        // The only straight line is the bend line, across the sheet on the zone's centre line.
        let lines: Vec<&Entity> = es.iter().filter(|e| e.0 == "LINE").collect();
        assert_eq!(lines.len(), 1, "{name}: seams left in the outline");
        let (a, b) = (point(lines[0], 10), point(lines[0], 11));
        assert!((a.x - 10.0).abs() < 1e-6 && (b.x - 10.0).abs() < 1e-6, "{name}: bend line {a:?} to {b:?}");
        // The blank is the blank: as much of it as was drawn, exactly.
        let after: f64 = loops(&s).iter().map(|l| l.signed_area().abs()).sum();
        assert!((after - area).abs() < 1e-6 * area, "{name}: {after} vs {area}");
    }
}

/// A fold across the straight part of a curved outline keeps the curve as drawn as well: an arc
/// stays one arc, not the 0.001 mm chords a fold used to leave.
#[test]
fn a_curve_away_from_a_fold_stays_its_curve_in_the_dxf() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Blank"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 50], [-60, 50], [-60, -50], [0, -50]]}));
    run(&mut s, "sketch.arc.three_point", json!({"start": [0, -50], "through": [50, 0], "end": [0, 50]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Blank"}));
    run(&mut s, "sheet.fold", json!({"points": [[-20, -200, 2.5], [-20, 200, 2.5]]}));
    let es = entities(&export(&mut s, json!({})));
    assert_eq!(count(&es, "ARC"), 1, "{:?}", es.iter().map(|e| e.0.as_str()).collect::<Vec<_>>());
    assert!(count(&es, "LINE") < 12, "the arc went to chords: {} lines", count(&es, "LINE"));
}

/// Two stretches of one edge that bend to opposite sides are parted in the blank: the flat pattern
/// carries a zero-width cut at their junction, from the bend line out to the far edge, and the DXF
/// writes both of its lines on the outline. A blank still joined there cannot fold, one stretch up
/// and the next down (FR-7 #5932). Same-side stretches stay one piece.
#[test]
fn segments_on_opposite_sides_cut_the_blank_at_the_junction() {
    // A 100 x 60 sheet of the 2 mm rule (R 1, K 0.5): setback 3 mm, allowance pi, leg 17 mm.
    let flange = |flip: [bool; 2], heights: [&str; 2]| {
        json!({"type":"edge","edges":[{"point":[50,0,2],"segments":[
            {"from":0,"to":0.5,"angle":"90 deg","height":heights[0],"flip":flip[0]},
            {"from":0.5,"to":1,"angle":"90 deg","height":heights[1],"flip":flip[1]}]}],
            "height":"20 mm","angle":"90 deg"})
    };
    let mut s = plate_in(sheet_of_2mm(), &[]);
    run(&mut s, "sheet.flange", flange([false, true], ["20 mm", "20 mm"]));
    let (from, to) = (Vec2::new(50.0, 3.0), Vec2::new(50.0, 3.0 - (PI + 17.0)));
    let cut = |lp: &solidcore_record::Loop2, a: Vec2, b: Vec2| {
        lp.segs.iter().any(|s| matches!(s, Seg2::Line { a: p, b: q } if p.dist(a) < 1e-6 && q.dist(b) < 1e-6))
    };
    let outline_of = |s: &mut Session| -> Vec<(Vec2, Vec2)> {
        let es = entities(&export(s, json!({})));
        es.iter().filter(|e| e.0 == "LINE" && e.1.iter().any(|(c, v)| *c == 8 && v == "OUTLINE")).map(|e| (point(e, 10), point(e, 11))).collect()
    };
    let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    let both_ways = |lines: &[(Vec2, Vec2)], a: Vec2, b: Vec2| {
        lines.iter().any(|&(p, q)| near(p, a) && near(q, b)) && lines.iter().any(|&(p, q)| near(p, b) && near(q, a))
    };
    let no_errors = |s: &Session| {
        let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
        assert!(errors.is_empty(), "unfold: {errors:?}");
    };
    // The flat pattern: the outline runs out along the junction and back.
    let regions = s.model.state().sheets[0].flat_regions().unwrap_or_default();
    assert!(
        regions.iter().any(|r| cut(&r.outer, from, to) && cut(&r.outer, to, from)),
        "the blank is cut at the junction: {:?}",
        regions.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>()
    );
    // The DXF writes the same two lines, on the outline layer.
    let outline = outline_of(&mut s);
    assert!(both_ways(&outline, from, to), "the DXF has no junction cut: {outline:?}");
    // Unfolding builds the flat solid from the same regions: the slit must not stop it.
    run(&mut s, "sheet.unfold", json!({}));
    no_errors(&s);
    // Stretches of different heights: the far edge steps at the junction, so the cut's outer end is
    // that step's corner, an existing vertex of the blank. The cut still runs from the bend line to
    // the shorter stretch's far edge, and the DXF writes it.
    let mut differ = plate_in(sheet_of_2mm(), &[]);
    run(&mut differ, "sheet.flange", flange([false, true], ["20 mm", "15 mm"]));
    let step = Vec2::new(50.0, 3.0 - (PI + 12.0));
    let regions = differ.model.state().sheets[0].flat_regions().unwrap_or_default();
    assert!(
        regions.iter().any(|r| cut(&r.outer, from, step) && cut(&r.outer, step, from)),
        "the blank is not cut at the differing-height junction: {:?}",
        regions.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>()
    );
    let outline = outline_of(&mut differ);
    assert!(both_ways(&outline, from, step), "the DXF has no differing-height junction cut: {outline:?}");
    run(&mut differ, "sheet.unfold", json!({}));
    no_errors(&differ);
    // Same-side stretches of different heights: two bends, one piece, no cut at the junction.
    let mut same = plate_in(sheet_of_2mm(), &[]);
    run(&mut same, "sheet.flange", flange([false, false], ["20 mm", "15 mm"]));
    let regions = same.model.state().sheets[0].flat_regions().unwrap_or_default();
    assert!(!regions.iter().any(|r| cut(&r.outer, from, to) && cut(&r.outer, to, from)), "a same-side junction stays joined");
}

/// A notch through a junction's far edge takes both ends the junction cut was computed from off the
/// blank: the far edge at x = 50 is gone, and the bend-line end lies inside the material. The cut is
/// clipped to the material that remains, from the bend line out to the notch's edge, and written in
/// the flat loop and the DXF. Dropped, the blank stayed joined where the folded part separates
/// (FR-7 #5932).
#[test]
fn a_notch_through_a_junctions_far_edge_still_parts_the_blank() {
    let mut s = plate_in(sheet_of_2mm(), &[]);
    run(
        &mut s,
        "sheet.flange",
        json!({"type":"edge","edges":[{"point":[50,0,2],"segments":[
            {"from":0,"to":0.5,"angle":"90 deg","height":"20 mm","flip":false},
            {"from":0.5,"to":1,"angle":"90 deg","height":"20 mm","flip":true}]}],
            "height":"20 mm","angle":"90 deg"}),
    );
    // The notch: x 49…51, y -18…-10, in the flat frame. The flange's far edge is at y -17.14, so
    // the notch takes it and the material between it and y -10 away at the junction.
    run(&mut s, "sheet.unfold", json!({}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Notch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[49,-18],"p1":[51,-10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Notch","through_all":true,"operation":"cut","direction":"symmetric"}));
    // The bend line, and where the material left ends outward of it: the notch's edge.
    let bend = Vec2::new(50.0, 3.0);
    let left = Vec2::new(50.0, -10.0);
    let cut = |lp: &solidcore_record::Loop2, a: Vec2, b: Vec2| {
        lp.segs.iter().any(|s| matches!(s, Seg2::Line { a: p, b: q } if p.dist(a) < 1e-6 && q.dist(b) < 1e-6))
    };
    let regions = s.model.state().sheets[0].flat_regions().unwrap_or_default();
    assert!(
        regions.iter().any(|r| cut(&r.outer, bend, left) && cut(&r.outer, left, bend)),
        "the blank is not cut from the bend line to the notch's edge: {:?}",
        regions.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>()
    );
    let outline: Vec<(Vec2, Vec2)> = entities(&export(&mut s, json!({})))
        .iter()
        .filter(|e| e.0 == "LINE" && e.1.iter().any(|(c, v)| *c == 8 && v == "OUTLINE"))
        .map(|e| (point(e, 10), point(e, 11)))
        .collect();
    let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    let both_ways = |lines: &[(Vec2, Vec2)], a: Vec2, b: Vec2| {
        lines.iter().any(|&(p, q)| near(p, a) && near(q, b)) && lines.iter().any(|&(p, q)| near(p, b) && near(q, a))
    };
    assert!(both_ways(&outline, bend, left), "the DXF has no clipped junction cut: {outline:?}");
    // The cut leaves the part one piece, and refolding then unfolding builds it.
    run(&mut s, "sheet.refold", json!({}));
    run(&mut s, "sheet.unfold", json!({}));
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "unfold: {errors:?}");
}

/// A round cut over an opposite-side junction takes the far edge away across the junction: the
/// material left ends on the circle, so the junction cut runs from the bend line to the arc,
/// exactly where the circle crosses the junction line, in the flat loop and the DXF. The crossing
/// is found on the arc itself, not on a 0.001 mm chord of it: dropped, the blank stayed joined
/// where the folded part separates (FR-7 #5932).
#[test]
fn a_round_notch_over_a_junctions_far_edge_ends_the_cut_on_the_arc() {
    let mut s = plate_in(sheet_of_2mm(), &[]);
    run(
        &mut s,
        "sheet.flange",
        json!({"type":"edge","edges":[{"point":[50,0,2],"segments":[
            {"from":0,"to":0.5,"angle":"90 deg","height":"20 mm","flip":false},
            {"from":0.5,"to":1,"angle":"90 deg","height":"20 mm","flip":true}]}],
            "height":"20 mm","angle":"90 deg"}),
    );
    // The flange's far edge is y = 3 − π − 17. A Ø10 round cut centred on the junction there takes
    // that edge away across the junction and leaves the circle as the material's edge; the
    // junction line meets it at the circle's top, (50, far + 5).
    run(&mut s, "sheet.unfold", json!({}));
    let far = 3.0 - PI - 17.0;
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Notch"}));
    run(&mut s, "sketch.circle.center", json!({"center":[50, far], "radius":5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Notch","through_all":true,"operation":"cut","direction":"symmetric"}));
    let bend = Vec2::new(50.0, 3.0);
    let on_arc = Vec2::new(50.0, far + 5.0);
    let cut = |lp: &solidcore_record::Loop2, a: Vec2, b: Vec2| {
        lp.segs.iter().any(|s| matches!(s, Seg2::Line { a: p, b: q } if p.dist(a) < 1e-6 && q.dist(b) < 1e-6))
    };
    let regions = s.model.state().sheets[0].flat_regions().unwrap_or_default();
    assert!(
        regions.iter().any(|r| cut(&r.outer, bend, on_arc) && cut(&r.outer, on_arc, bend)),
        "the blank is not cut from the bend line to the arc: {:?}",
        regions.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>()
    );
    // That end is on the circle to rounding error, not the 0.001 mm inside a chord of it.
    let (centre, radius) = (Vec2::new(50.0, far), 5.0);
    let slit_end = regions.iter().flat_map(|r| r.outer.segs.iter()).find_map(|seg| match *seg {
        Seg2::Line { a, b } if a.dist(bend) < 1e-6 => Some(b),
        _ => None,
    });
    let end = slit_end
        .unwrap_or_else(|| panic!("no junction cut from the bend line: {:?}", regions.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>()));
    let off = ((end - centre).len() - radius).abs();
    assert!(off < 1e-9, "the cut ends {end:?}, off the arc by {off} mm");
    // The DXF writes both lines on the outline, and splits the arc there: an arc ends at the cut.
    let es = entities(&export(&mut s, json!({})));
    let outline: Vec<(Vec2, Vec2)> = es.iter().filter(|e| e.0 == "LINE" && layer_of(e) == "OUTLINE").map(|e| (point(e, 10), point(e, 11))).collect();
    let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    let both_ways = |lines: &[(Vec2, Vec2)], a: Vec2, b: Vec2| {
        lines.iter().any(|&(p, q)| near(p, a) && near(q, b)) && lines.iter().any(|&(p, q)| near(p, b) && near(q, a))
    };
    assert!(both_ways(&outline, bend, end), "the DXF has no junction cut to the arc: {outline:?}");
    let arcs: Vec<(Vec2, f64, [Vec2; 2])> = es
        .iter()
        .filter(|e| e.0 == "ARC" && layer_of(e) == "OUTLINE")
        .map(|e| {
            let (c, r) = (point(e, 10), values(e, 40)[0]);
            let (a0, a1) = (values(e, 50)[0].to_radians(), values(e, 51)[0].to_radians());
            (c, r, [c + Vec2::from_angle(a0) * r, c + Vec2::from_angle(a1) * r])
        })
        .collect();
    assert!(
        arcs.iter().any(|&(c, r, ends)| c.dist(centre) < 1e-6 && (r - radius).abs() < 1e-6 && ends.iter().any(|p| p.dist(end) < 1e-6)),
        "the DXF's arc is not split at the cut: {arcs:?}"
    );
    // The cut leaves the part one piece, and refolding then unfolding builds it.
    run(&mut s, "sheet.refold", json!({}));
    run(&mut s, "sheet.unfold", json!({}));
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "unfold: {errors:?}");
}

/// A re-entrant notch pattern leaves the junction line crossing material more than once: every
/// surviving bridge is parted, in the flat loop and the DXF, not only the stretch nearest the bend
/// line. With the two notches below, material is left at x = 50 from the bend line to y = -8 and
/// again from y = -10 to y = -15; cutting only the first left the second bridge joined, so the
/// folded part did not separate (FR-7 #5932).
#[test]
fn every_surviving_bridge_of_a_junction_is_cut() {
    let mut s = plate_in(sheet_of_2mm(), &[]);
    run(
        &mut s,
        "sheet.flange",
        json!({"type":"edge","edges":[{"point":[50,0,2],"segments":[
            {"from":0,"to":0.5,"angle":"90 deg","height":"20 mm","flip":false},
            {"from":0.5,"to":1,"angle":"90 deg","height":"20 mm","flip":true}]}],
            "height":"20 mm","angle":"90 deg"}),
    );
    run(&mut s, "sheet.unfold", json!({}));
    // Notch 1: x 49…101, y -10…-8: a slot from x = 49 out through the right edge, between the bend
    // line and the far edge. Notch 2: x 49…51, y -18…-15, across the far edge at the junction. The
    // material left at x = 50 is y 3…-8 and y -10…-15: two bridges, each one to part.
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Notch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[49,-10],"p1":[101,-8]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[49,-18],"p1":[51,-15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Notch","through_all":true,"operation":"cut","direction":"symmetric"}));
    let bridges = [(Vec2::new(50.0, 3.0), Vec2::new(50.0, -8.0)), (Vec2::new(50.0, -10.0), Vec2::new(50.0, -15.0))];
    let cut = |lp: &solidcore_record::Loop2, a: Vec2, b: Vec2| {
        lp.segs.iter().any(|s| matches!(s, Seg2::Line { a: p, b: q } if p.dist(a) < 1e-6 && q.dist(b) < 1e-6))
    };
    let regions = s.model.state().sheets[0].flat_regions().unwrap_or_default();
    for (a, b) in bridges {
        assert!(
            regions.iter().any(|r| cut(&r.outer, a, b) && cut(&r.outer, b, a)),
            "the blank is not cut across the bridge {a:?}–{b:?}: {:?}",
            regions.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>()
        );
    }
    let outline: Vec<(Vec2, Vec2)> = entities(&export(&mut s, json!({})))
        .iter()
        .filter(|e| e.0 == "LINE" && layer_of(e) == "OUTLINE")
        .map(|e| (point(e, 10), point(e, 11)))
        .collect();
    let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    let both_ways = |lines: &[(Vec2, Vec2)], a: Vec2, b: Vec2| {
        lines.iter().any(|&(p, q)| near(p, a) && near(q, b)) && lines.iter().any(|&(p, q)| near(p, b) && near(q, a))
    };
    for (a, b) in bridges {
        assert!(both_ways(&outline, a, b), "the DXF has no cut across the bridge {a:?}–{b:?}: {outline:?}");
    }
    // The cuts leave the part one piece, and refolding then unfolding builds it.
    run(&mut s, "sheet.refold", json!({}));
    run(&mut s, "sheet.unfold", json!({}));
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "unfold: {errors:?}");
}

/// A junction cut is the junction's own: on a C-shaped blank the junction line runs on past its tip
/// through the foot, and only the stretch from the bend line to the far edge belongs to this
/// junction. Cutting past the tip slit the foot, which the DXF carried as a cut in material the
/// bend never reaches (FR-7 #5932).
#[test]
fn a_junction_cut_stops_at_its_own_tip_on_a_c_shaped_blank() {
    let mut s = sheet_of_2mm();
    // A C-shaped blank: the bar y 0…100, the left arm x 0…30 down to y −40, the foot y −40…−30
    // across to x 200 and the opening x 30…200, y −30…0. The flange's edge is the bar's bottom edge
    // in the opening, x 30…200, so the junction at 20/170 of it is at x = 50.
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Base"}));
    run(&mut s, "sketch.line", json!({"points":[[0,100],[200,100],[200,0],[30,0],[30,-30],[200,-30],[200,-40],[0,-40]],"closed":true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch":"Base"}));
    run(
        &mut s,
        "sheet.flange",
        json!({"type":"edge","edges":[{"point":[100,0,2],"segments":[
            {"from":0,"to":20.0/170.0,"angle":"90 deg","height":"20 mm","flip":false},
            {"from":20.0/170.0,"to":1,"angle":"90 deg","height":"20 mm","flip":true}]}],
            "height":"20 mm","angle":"90 deg"}),
    );
    run(&mut s, "sheet.unfold", json!({}));
    // The far-edge notch of the fixture, across the junction's far edge (y −17.14): the material
    // left at x = 50 ends at y −10, while the foot at y −30…−40 lies past the junction's tip.
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Notch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[49,-18],"p1":[51,-10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Notch","through_all":true,"operation":"cut","direction":"symmetric"}));
    let bend = Vec2::new(50.0, 3.0);
    let left = Vec2::new(50.0, -10.0);
    let cut = |lp: &solidcore_record::Loop2, a: Vec2, b: Vec2| {
        lp.segs.iter().any(|s| matches!(s, Seg2::Line { a: p, b: q } if p.dist(a) < 1e-6 && q.dist(b) < 1e-6))
    };
    let regions = s.model.state().sheets[0].flat_regions().unwrap_or_default();
    assert!(
        regions.iter().any(|r| cut(&r.outer, bend, left) && cut(&r.outer, left, bend)),
        "the blank is not cut from the bend line to the notch's edge: {:?}",
        regions.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>()
    );
    // Nothing slits the foot: no cut crosses y −30…−40 at x = 50.
    let foot_cut = |lp: &solidcore_record::Loop2| {
        lp.segs.iter().any(|s| match *s {
            Seg2::Line { a, b } => (a.x - 50.0).abs() < 1e-6 && (b.x - 50.0).abs() < 1e-6 && a.y > -30.5 && a.y < -29.5 && b.y > -40.5 && b.y < -39.5,
            _ => false,
        })
    };
    assert!(
        !regions.iter().any(|r| foot_cut(&r.outer)),
        "the junction cut the foot past its own tip: {:?}",
        regions.iter().map(|r| r.outer.segs.len()).collect::<Vec<_>>()
    );
    // The DXF carries the junction's own cut and no cut across the foot.
    let outline: Vec<(Vec2, Vec2)> = entities(&export(&mut s, json!({})))
        .iter()
        .filter(|e| e.0 == "LINE" && layer_of(e) == "OUTLINE")
        .map(|e| (point(e, 10), point(e, 11)))
        .collect();
    let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    let both_ways = |lines: &[(Vec2, Vec2)], a: Vec2, b: Vec2| {
        lines.iter().any(|&(p, q)| near(p, a) && near(q, b)) && lines.iter().any(|&(p, q)| near(p, b) && near(q, a))
    };
    assert!(both_ways(&outline, bend, left), "the DXF has no clipped junction cut: {outline:?}");
    assert!(
        !outline.iter().any(|&(a, b)| (a.x - 50.0).abs() < 1e-6
            && (b.x - 50.0).abs() < 1e-6
            && a.y < -29.5
            && a.y > -40.5
            && b.y < -29.5
            && b.y > -40.5),
        "the DXF cuts the foot past the junction's tip: {outline:?}"
    );
    // The cut leaves the part one piece, and refolding then unfolding builds it.
    run(&mut s, "sheet.refold", json!({}));
    run(&mut s, "sheet.unfold", json!({}));
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "unfold: {errors:?}");
}

/// Stretches of one edge that bend to opposite sides at different angles have different setbacks,
/// so their bend lines are offset: the folded part separates over the strip between them, and the
/// blank has to be cut over that whole strip too. The cut starts at the inner (deeper) bend line,
/// not the farther-out one, which left the strip joined and the blank unable to fold (FR-7 #5932).
/// Its outer end is the nearer far edge: past that only the longer stretch's material remains.
#[test]
fn opposite_side_stretches_of_different_angles_cut_from_the_inner_bend_line() {
    let mut s = plate_in(sheet_of_2mm(), &[]);
    run(
        &mut s,
        "sheet.flange",
        json!({"type":"edge","edges":[{"point":[50,0,2],"segments":[
            {"from":0,"to":0.5,"angle":"90 deg","height":"20 mm","flip":false},
            {"from":0.5,"to":1,"angle":"45 deg","height":"20 mm","flip":true}]}],
            "height":"20 mm","angle":"90 deg"}),
    );
    // The 2 mm rule (R 1, K 0.5): the 90° stretch's setback is (R + T)·tan(45°) = 3 mm, so its bend
    // line is y = 3 and its far edge y = 3 − (π + 17); the 45° stretch's setback is
    // (R + T)·tan(22.5°) = 1.242640687 mm, so its bend line is y = 1.242640687 and its far edge
    // y = −19.085514953. The bend lines are 1.757359313 mm apart, and the 90° stretch's far edge is
    // the nearer one. The cut has to run from the inner bend line, y = 3, over that strip to the
    // nearer far edge; starting at the farther-out bend line, y = 1.242640687, left the strip
    // y 1.242640687…3 joined.
    let inner = Vec2::new(50.0, 3.0);
    let farther_out = Vec2::new(50.0, 3.0 * (PI / 8.0).tan());
    let nearer_far = Vec2::new(50.0, 3.0 - (PI + 17.0));
    let offset = inner.y - farther_out.y;
    assert!(offset > 1.0, "the test's bend lines are only {offset} mm apart, too close to show the strip: {farther_out:?}");
    let cut = |lp: &solidcore_record::Loop2, a: Vec2, b: Vec2| {
        lp.segs.iter().any(|s| matches!(s, Seg2::Line { a: p, b: q } if p.dist(a) < 1e-6 && q.dist(b) < 1e-6))
    };
    let regions = s.model.state().sheets[0].flat_regions().unwrap_or_default();
    assert!(
        regions.iter().any(|r| cut(&r.outer, inner, nearer_far) && cut(&r.outer, nearer_far, inner)),
        "the flat loop has no cut from the inner bend line {inner:?} to the nearer far edge {nearer_far:?}          ({offset} mm apart from the farther-out bend line {farther_out:?}): {:?}",
        regions
            .iter()
            .flat_map(|r| r.outer.segs.iter())
            .filter_map(|seg| match *seg {
                Seg2::Line { a, b } if (a.x - 50.0).abs() < 1e-6 && (b.x - 50.0).abs() < 1e-6 => Some((a.y, b.y)),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    // The DXF writes the same two lines, on the outline layer.
    let outline: Vec<(Vec2, Vec2)> = entities(&export(&mut s, json!({})))
        .iter()
        .filter(|e| e.0 == "LINE" && layer_of(e) == "OUTLINE")
        .map(|e| (point(e, 10), point(e, 11)))
        .collect();
    let near = |p: Vec2, q: Vec2| p.dist(q) < 1e-6;
    let both_ways = |lines: &[(Vec2, Vec2)], a: Vec2, b: Vec2| {
        lines.iter().any(|&(p, q)| near(p, a) && near(q, b)) && lines.iter().any(|&(p, q)| near(p, b) && near(q, a))
    };
    assert!(both_ways(&outline, inner, nearer_far), "the DXF has no cut from the inner bend line to the nearer far edge: {outline:?}");
    // The cut leaves the part one piece, and refolding then unfolding builds it.
    run(&mut s, "sheet.unfold", json!({}));
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "unfold: {errors:?}");
    run(&mut s, "sheet.refold", json!({}));
    run(&mut s, "sheet.unfold", json!({}));
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.is_empty(), "unfold: {errors:?}");
}

// Outputs recorded from main at 3fc6610e911f9a746a202d677a7d955522d92335.
// Fresh-process baseline and shared-conversion runs matched all 53 payloads byte for byte.
// Pins were migrated only after that comparison; fixtures and semantic assertions are unchanged.
// These byte pins are fixed expectations, never regenerated by the test.
#[cfg(not(target_arch = "wasm32"))]
mod byte_regressions {
    use super::*;
    use sha2::{Digest, Sha256};

    type ExpectedOutput = (&'static str, usize, &'static str);

    const EXPECTED_OUTPUTS: [ExpectedOutput; 53] = [
        ("circle-cutout-shapes.json", 76, "4fa69120876866f03de10436d0978326710e4e40b81d3dee9680c8bbcda497b8"),
        ("circle-two-halves-exact.dxf", 4182, "0fc9801a8dc9f05f757e22e578c35234b6386766260bd01a8c2171ebfdf7b91a"),
        ("circle-two-halves-arcs.dxf", 4182, "0fc9801a8dc9f05f757e22e578c35234b6386766260bd01a8c2171ebfdf7b91a"),
        ("round-ended-slot-exact.dxf", 4557, "58cbc3e017a59ad2578d8c880532d924b8a5c6d1339143b315353bc08c637cea"),
        ("round-ended-slot-arcs.dxf", 4557, "58cbc3e017a59ad2578d8c880532d924b8a5c6d1339143b315353bc08c637cea"),
        ("ellipse-exact.dxf", 4271, "7307505be802892dbc1cadcd6a1c5772f1a6e2eabe6b5ffb2d1750fa47e642ee"),
        ("ellipse-arcs.dxf", 15452, "a95cb6f62e6175f9872ceefb2758c8c6477ba20008dd1478c8d604b5203ee5fa"),
        ("cubic-spline-exact.dxf", 5180, "566f32b7b19ebc4d0a041e7bf1366d2818c4f2cb54facc363777a39303b304db"),
        ("cubic-spline-arcs.dxf", 12165, "505f4bea3f008cea2103ed9fd7ac7e7b6047c4cbb616ac93d4ffcaad76dca447"),
        ("degree-nine-exact.dxf", 5045, "524ead294fd37c1aa3eff8b12e1a624bc19478c0a34fcaaaf294881455aac208"),
        ("degree-nine-arcs.dxf", 12956, "6f851ccd685a6a56dea2c8c78fb0095f8361a8191be4869f5c6a381311ca8407"),
        ("text-outlines-exact.dxf", 12338, "630ddb205da831fec391dcd94ea093444fd76b28c5d688c8bcfa708778a60e4f"),
        ("text-outlines-arcs.dxf", 24602, "72f28236efbf6dc5a365d781ff8ebcebbded705a990c0ee5ba2993bd56c774d8"),
        ("tray-default.dxf", 25185, "6527eb381004943e75bf4d9843cf9881bcc266b43d09e7619ddfafabcdde3206"),
        ("tray-exact-cut-bends-true-roles.dxf", 26104, "a50ac49eeae670175fa58aa46fc2d9456c6ab7ae36c30dda27ec3e7abac5894f"),
        ("tray-exact-cut-bends-true-single.dxf", 24983, "b7c7f6219f49aa37a3273d21352fbfe1cbabd1a9e6f8e1d0ee9646ee6d20d269"),
        ("tray-exact-cut-bends-false-roles.dxf", 25562, "8ad87df33d66c85641cb5813e018af2f0226d9ef46adf93c391f352e7d0f875c"),
        ("tray-exact-cut-bends-false-single.dxf", 24467, "60b08bd964700ae826507802e4e4b594564788287ad0deccc80e7f7ce943e3f1"),
        ("tray-exact-etch-bends-true-roles.dxf", 26104, "3586c3a56095ae96b9d32a41232069a0afb8d2a6664ee9ae026e9b0ea9094b22"),
        ("tray-exact-etch-bends-true-single.dxf", 24983, "c84cf65dc80805cfd437986e7fd6d88405e101eab56b84ae17a10bf81970e4a6"),
        ("tray-exact-etch-bends-false-roles.dxf", 25562, "2c72706e815e13fe4522ec7d2f9b4e9d9fc49d7b55942dd067d0f6222990914c"),
        ("tray-exact-etch-bends-false-single.dxf", 24467, "b00afed1fbb9d158162de7baa869402136fe8880d2a109352d0d4a5019ce2d96"),
        ("tray-arcs-cut-bends-true-roles.dxf", 43292, "476dbdd7bbeb1caed904fabaafbb4ff87d032cc3ca6e6a0448e9d86520f0a9d7"),
        ("tray-arcs-cut-bends-true-single.dxf", 41802, "54ee355a13a6ea77ed9c8765e529ea738fd90218a7173d2ff077f0a355aa26df"),
        ("tray-arcs-cut-bends-false-roles.dxf", 42746, "dc8130ef5543cfa30620b61e749bc2afe49b32a6bf861be9e48c3ebfd5ed4a2f"),
        ("tray-arcs-cut-bends-false-single.dxf", 41282, "4bc6d502f63c8700634aed2e7b2b0e7af08d0d1e6756e6c5310eb841b26505d3"),
        ("tray-arcs-etch-bends-true-roles.dxf", 43292, "f1bbedef32767c1f9226c9f2aa00033d72757071ae90f8af84969fc4df804bc4"),
        ("tray-arcs-etch-bends-true-single.dxf", 41802, "91a17041d731d22033051899d7a1c381f79443086c84cae2f082bc1900ba933a"),
        ("tray-arcs-etch-bends-false-roles.dxf", 42746, "4b2937eabfe3f4bcf57b8c5354a202670286b0d918f7787923828809868dcb8f"),
        ("tray-arcs-etch-bends-false-single.dxf", 41282, "70d07397d9e1e3ccf5b001f8679e00945ffa8082a7d925c84578c12b6240effa"),
        ("rule-bend-down-exact-bends-true-roles.dxf", 4615, "d657821f11b049ca6285311166c796fab83fb87a1a92660a43e4f07366f3076e"),
        ("rule-bend-down-exact-bends-false-single.dxf", 3882, "ca50ce56ae7ff0da304665f009bf11370629761573dc6faebb371b9172294837"),
        ("rule-bend-down-arcs-bends-true-roles.dxf", 4615, "d657821f11b049ca6285311166c796fab83fb87a1a92660a43e4f07366f3076e"),
        ("rule-bend-down-arcs-bends-false-single.dxf", 3882, "ca50ce56ae7ff0da304665f009bf11370629761573dc6faebb371b9172294837"),
        ("rule-included-down-exact-bends-true-roles.dxf", 4616, "669a5a7a53eb9d620313d672e482e10a2b19b5685e6a3e9bc9ba167dc5cf9f95"),
        ("rule-included-down-exact-bends-false-single.dxf", 3883, "bbc5e932e299b02e3ad9132f66dad1dc6c6bcf50a7a0a52ee9054d2a61fba65d"),
        ("rule-included-down-arcs-bends-true-roles.dxf", 4616, "669a5a7a53eb9d620313d672e482e10a2b19b5685e6a3e9bc9ba167dc5cf9f95"),
        ("rule-included-down-arcs-bends-false-single.dxf", 3883, "bbc5e932e299b02e3ad9132f66dad1dc6c6bcf50a7a0a52ee9054d2a61fba65d"),
        ("pocket-and-engraved-text-exact.dxf", 21914, "7f5e54c231b0599a98e6a81ba1b817c9c30122aefb9871f09ddc4843afcfdf50"),
        ("pocket-and-engraved-text-arcs.dxf", 39072, "60383fed4c2a1a23a895a2efa89eed3dd1dcbb71c166b6da4d1d9d3af6612d33"),
        ("perforation-exact.dxf", 7551, "8aebb4d71acd4daae5c76bf8e44f2378a4464ac94b49df193acf06580b458776"),
        ("perforation-arcs.dxf", 7551, "8aebb4d71acd4daae5c76bf8e44f2378a4464ac94b49df193acf06580b458776"),
        ("per-body-0-exact.dxf", 4085, "3c6e72d726bdb21f92ce5078a3a1a3facfa40e07240092fa3d930d5247296eab"),
        ("per-body-1-exact.dxf", 4309, "b8241131da78e3d76ba7c66447d691656dbf37aa831c4ca0ee9e3de97b9022bf"),
        ("per-body-0-arcs.dxf", 4085, "3c6e72d726bdb21f92ce5078a3a1a3facfa40e07240092fa3d930d5247296eab"),
        ("per-body-1-arcs.dxf", 4309, "b8241131da78e3d76ba7c66447d691656dbf37aa831c4ca0ee9e3de97b9022bf"),
        ("writer-bmp-controls.dxf", 3491, "4fa9fc45abc252046a27031bbdd31aab187b35e18b52d7f4801c78002fbb57f5"),
        ("writer-bmp-cap.dxf", 5200, "5aa5cf722ed405a8c2a1c98ed98aec2356640dbeab0520fca83801ab9d1cb267"),
        ("unicode-early.txt", 159, "90c374179d2388832a9bbcafbbabd4c3887edde4989eedd9cfcbd48b4199b4f1"),
        ("unicode-first-supplementary.txt", 159, "e14a0d3eb9fabbc361d3804fb00830c013771ad79ba52f24bd12d7ea8a8a02a0"),
        ("unicode-last-scalar.txt", 166, "f931160242245ee3cdf4f9b551bb20c2a4883d52114208a6b66d83dffca4a484"),
        ("unicode-after-cap.txt", 161, "6b415a0446cf33531d23a6768790ea562441eb23b70092e77dea4ee99951c617"),
        ("unicode-far-after-cap.txt", 163, "87709acb08a4e729b4480182419ad843ab2c7887fc73dd23bc86a3e86f35beaa"),
    ];

    struct BytePins {
        expected: &'static [ExpectedOutput; 53],
        next: usize,
    }

    impl BytePins {
        fn new(expected: &'static [ExpectedOutput; 53]) -> Self {
            Self { expected, next: 0 }
        }

        fn put(&mut self, name: &str, bytes: &[u8]) {
            assert!(!bytes.is_empty(), "empty original-output case: {name}");
            let &(expected_name, expected_len, expected_sha256) =
                self.expected.get(self.next).unwrap_or_else(|| panic!("unexpected extra original-output case: {name}"));
            assert_eq!(name, expected_name, "original-output case {} changed order or name", self.next + 1);
            assert_eq!(bytes.len(), expected_len, "original byte length changed for {name}");
            let actual_sha256 = format!("{:x}", Sha256::digest(bytes));
            assert_eq!(actual_sha256, expected_sha256, "original bytes changed for {name}");
            self.next += 1;
        }

        fn dxf(&mut self, name: &str, text: &str) {
            assert!(text.contains("AC1015") && text.ends_with("  0\nEOF\n"), "incomplete DXF: {name}");
            self.put(&format!("{name}.dxf"), text.as_bytes());
        }

        fn finish(self) {
            assert_eq!(self.next, self.expected.len(), "every original-output case must be checked");
        }
    }

    fn modes(capture: &mut BytePins, name: &str, s: &mut Session) {
        for curves in ["exact", "arcs"] {
            let dxf = export(s, json!({"curves": curves}));
            let es = entities(&dxf);
            assert!(!es.is_empty());
            if curves == "arcs" {
                assert!(es.iter().all(|e| matches!(e.0.as_str(), "LINE" | "ARC" | "CIRCLE")));
            }
            capture.dxf(&format!("{name}-{curves}"), &dxf);
        }
    }

    fn g4_plate() -> Session {
        let mut s = plate_with(&[]);
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cut"}));
        let mut line = |a: [i32; 2], b: [i32; 2]| run(&mut s, "sketch.line", json!({"points": [a, b]}))["curves"][0].as_str().unwrap().to_string();
        let bottom = line([20, 20], [40, 20]);
        let top = line([40, 40], [20, 40]);
        line([20, 40], [20, 20]);
        run(&mut s, "sketch.blend_curve", json!({"a": format!("{bottom}.end"), "b": format!("{top}.start"), "continuity": "g4"}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "Cut", "through_all": true, "operation": "cut", "direction": "symmetric"}));
        let es = entities(&export(&mut s, json!({})));
        assert_eq!(es.iter().filter(|e| e.0 == "SPLINE" && values(e, 71) == [9.0]).count(), 1);
        s
    }

    fn rule_plate(display: &str) -> Session {
        let mut s = sheet_of_2mm();
        run(&mut s, "sheet.manage_rules", json!({"name": "Sheet2", "angle_display": display}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sheet.flange", json!({"sketch": "Base", "rule": "Sheet2"}));
        run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 2]], "height": 20, "angle": "58 deg", "flip": true}));
        s
    }

    #[test]
    fn flat_export_preserves_original_bytes() {
        let mut capture = BytePins::new(&EXPECTED_OUTPUTS);

        let mut circle = plate_with(&[("sketch.circle.center", json!({"center": [25, 25], "radius": 4}))]);
        let circular = loops(&circle).into_iter().find(|lp| lp.segs.iter().all(|s| matches!(s, Seg2::Arc { .. }))).unwrap();
        assert_eq!(circular.segs.len(), 2, "the baseline must exercise a circular hole made of two semicircles");
        assert!(circular.segs.iter().all(|s| matches!(s, Seg2::Arc { sweep, .. } if (sweep.abs() - PI).abs() < 1e-9)));
        assert_eq!(count(&entities(&export(&mut circle, json!({}))), "CIRCLE"), 1);
        let flat = run(&mut circle, "sheet.flat_pattern", json!({}));
        assert!(flat["cutout_shapes"].as_array().unwrap().iter().any(|v| v.get("circle").is_some()));
        capture.put("circle-cutout-shapes.json", &serde_json::to_vec(&flat["cutout_shapes"]).unwrap());
        modes(&mut capture, "circle-two-halves", &mut circle);

        let mut slot = plate_with(&[("sketch.slot.center_to_center", json!({"p0": [20, 20], "p1": [40, 20], "width": 6}))]);
        let es = entities(&export(&mut slot, json!({})));
        assert_eq!((count(&es, "LINE"), count(&es, "ARC"), es.len()), (6, 2, 8));
        modes(&mut capture, "round-ended-slot", &mut slot);

        let mut ellipse = plate_with(&[("sketch.ellipse", json!({"center": [40, 30], "major": [55, 30], "minor_radius": 8}))]);
        assert_eq!(count(&entities(&export(&mut ellipse, json!({}))), "ELLIPSE"), 1);
        modes(&mut capture, "ellipse", &mut ellipse);

        let mut spline = plate_with(&[
            ("sketch.spline.fit_point", json!({"points": [[20, 15], [40, 45], [65, 40], [85, 20]]})),
            ("sketch.line", json!({"points": [[85, 20], [20, 15]]})),
        ]);
        assert!(count(&entities(&export(&mut spline, json!({}))), "SPLINE") > 0);
        modes(&mut capture, "cubic-spline", &mut spline);
        modes(&mut capture, "degree-nine", &mut g4_plate());

        let mut text = plate_with(&[("sketch.text", json!({"text": "AB", "at": [30, 20], "height": 12}))]);
        assert!(count(&entities(&export(&mut text, json!({}))), "SPLINE") > 0);
        modes(&mut capture, "text-outlines", &mut text);

        let mut tray = tray_with(&marks());
        let default = export(&mut tray, json!({}));
        assert_eq!(default, export(&mut tray, json!({"curves": "exact", "text": "cut", "bend_lines": true, "layers": "roles"})));
        capture.dxf("tray-default", &default);
        for curves in ["exact", "arcs"] {
            for text in ["cut", "etch"] {
                for bend_lines in [true, false] {
                    for layers in ["roles", "single"] {
                        let dxf = export(
                            &mut tray,
                            json!({"curves": curves, "text": text, "bend_lines": bend_lines, "layers": layers, "bend_notes": true}),
                        );
                        let es = entities(&dxf);
                        assert_eq!(count(&es, "TEXT"), 4, "notes remain independent of bend lines");
                        if layers == "single" {
                            assert!(es.iter().all(|e| layer_of(e) == "0"));
                        } else {
                            let bends = es.iter().filter(|e| matches!(layer_of(e).as_str(), "BEND_UP" | "BEND_DOWN")).count();
                            assert_eq!(bends, if bend_lines { 4 } else { 0 });
                            assert!(es.iter().any(|e| layer_of(e) == if text == "cut" { "TEXT" } else { "ETCH" }));
                            let inner = es.iter().rposition(|e| matches!(layer_of(e).as_str(), "CUT" | "TEXT" | "ETCH")).unwrap();
                            let outline = es.iter().position(|e| layer_of(e) == "OUTLINE").unwrap();
                            assert!(inner < outline);
                        }
                        capture.dxf(&format!("tray-{curves}-{text}-bends-{bend_lines}-{layers}"), &dxf);
                    }
                }
            }
        }

        for display in ["bend", "included"] {
            let mut s = rule_plate(display);
            let before = export(&mut s, json!({"bend_notes": true}));
            run(
                &mut s,
                "sheet.manage_rules",
                json!({"name": "Other", "thickness": "2 mm", "angle_display": if display == "bend" { "included" } else { "bend" }, "active": true}),
            );
            assert_eq!(export(&mut s, json!({"bend_notes": true})), before, "the named body's rule wins over the active rule");
            for curves in ["exact", "arcs"] {
                for (bend_lines, layers) in [(true, "roles"), (false, "single")] {
                    let dxf = export(&mut s, json!({"curves": curves, "bend_notes": true, "bend_lines": bend_lines, "layers": layers}));
                    let notes: Vec<_> = entities(&dxf).iter().filter(|e| e.0 == "TEXT").map(|e| string(e, 1)).collect();
                    assert_eq!(notes, [if display == "included" { "B1 DOWN 122%%d R1" } else { "B1 DOWN 58%%d R1" }]);
                    capture.dxf(&format!("rule-{display}-down-{curves}-bends-{bend_lines}-{layers}"), &dxf);
                }
            }
        }

        let mut pocket = plate_with(&[]);
        run(&mut pocket, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
        run(&mut pocket, "sketch.rectangle.two_point", json!({"p0": [70, 40], "p1": [90, 45]}));
        run(&mut pocket, "sketch.text", json!({"text": "AB-12", "at": [20, 28], "height": 10}));
        run(&mut pocket, "sketch.finish", json!({}));
        run(&mut pocket, "solid.extrude", json!({"sketch": "Marks", "distance": 1, "operation": "cut"}));
        let es = entities(&export(&mut pocket, json!({})));
        assert_eq!(on_layer(&es, "POCKET_BOTTOM_Z0.000000_TO_1.000000_MM").len(), 4);
        assert!(!on_layer(&es, "ETCH").is_empty());
        modes(&mut capture, "pocket-and-engraved-text", &mut pocket);

        let mut perforated = plate_with(&[]);
        run(&mut perforated, "sheet.perforate", json!({"diameter": 2, "pitch": 12, "margin": 6, "pattern": "square"}));
        let es = entities(&export(&mut perforated, json!({})));
        assert!(es.iter().any(|e| layer_of(e) == "PERFORATION" && e.0 == "CIRCLE"));
        modes(&mut capture, "perforation", &mut perforated);

        let mut bodies = lid_and_wall();
        for curves in ["exact", "arcs"] {
            let out = run(&mut bodies, "sheet.export_dxf", json!({"per_body": true, "curves": curves}));
            let files = out["files"].as_array().unwrap();
            assert_eq!(files.iter().map(|f| f["body"].as_str().unwrap()).collect::<Vec<_>>(), ["Lid", "Side Wall"]);
            for (i, file) in files.iter().enumerate() {
                let text = file["dxf"].as_str().unwrap();
                assert_eq!(text, export(&mut bodies, json!({"body": file["body"], "curves": curves})));
                capture.dxf(&format!("per-body-{i}-{curves}"), text);
            }
        }

        // The writer is the reachable API for arbitrary TEXT. Sheet text is outlines and
        // bend notes are generated ASCII; no synthetic public command is added to inject text.
        use solidcraft_io::{DxfEntity, DxfLayer, DxfShape, IoError, write_dxf_drawing};
        let note = |text: String| DxfEntity {
            layer: "NOTES".into(),
            shape: DxfShape::Text { at: Vec2::new(30.0, 12.0), height: 3.0, angle_deg: 90.0, text },
        };
        let layers = [DxfLayer::new("NOTES", 8)];
        for (name, text) in
            [("writer-bmp-controls", "ASCII %%d %%p %%c ° 中 \u{ffff}\n\t\u{7f}".to_string()), ("writer-bmp-cap", "\u{ffff}".repeat(251))]
        {
            let dxf = write_dxf_drawing(&layers, &[note(text)]).unwrap();
            let es = entities(&dxf);
            let value = string(es.iter().find(|e| e.0 == "TEXT").unwrap(), 1);
            if name == "writer-bmp-cap" {
                assert_eq!(value, "\\U+FFFF".repeat(250));
            } else {
                // This parser trims the three replaced trailing controls; the full byte pin
                // below still includes every space in the actual group-code value.
                assert_eq!(value, "ASCII %%d %%p %%c \\U+00B0 \\U+4E2D \\U+FFFF");
                assert!(dxf.contains("\\U+FFFF   \n"));
            }
            capture.dxf(name, &dxf);
        }
        for (name, text, code, position) in [
            ("unicode-early", "中\u{2000b}".to_string(), "U+2000B", 2),
            ("unicode-first-supplementary", "\u{10000}".to_string(), "U+10000", 1),
            ("unicode-last-scalar", "\u{10ffff}".to_string(), "U+10FFFF", 1),
            ("unicode-after-cap", format!("{}\u{2000b}", "中".repeat(250)), "U+2000B", 251),
            ("unicode-far-after-cap", format!("{}\u{2000b}", "中".repeat(10_000)), "U+2000B", 10_001),
        ] {
            let entities = [DxfEntity { layer: "OUTLINE".into(), shape: DxfShape::Line(Vec2::ZERO, Vec2::X) }, note(text)];
            let error = write_dxf_drawing(&layers, &entities).unwrap_err();
            assert!(matches!(error, IoError::Invalid(_)));
            let message = error.to_string();
            assert!(message.contains("DXF TEXT entity 2 on layer \"NOTES\""));
            assert!(message.contains(code) && message.contains(&format!("at position {position}")));
            assert!(message.len() < 300);
            capture.put(&format!("{name}.txt"), message.as_bytes());
        }
        capture.finish();
    }
}

/// Independent caller-level values for the explicitly reviewed legacy contract.
/// The generic recognizer and its tests are not relaxed by these flat-pattern rules.
mod caller_circle_compatibility {
    use super::*;
    use solidcore_record::Loop2;
    use solidcraft_io::{DxfPathStyle, DxfShape};

    fn halves(center: Vec2, radius: f64, second_center: Vec2, second_radius: f64) -> Loop2 {
        Loop2 {
            segs: vec![
                Seg2::Arc { center, radius, start: 0.0, sweep: PI },
                Seg2::Arc { center: second_center, radius: second_radius, start: PI, sweep: PI },
            ],
        }
    }
    fn styles() -> [DxfPathStyle; 2] {
        [DxfPathStyle::Exact, DxfPathStyle::Arcs { tolerance_mm: 0.001 }]
    }
    fn expect_circle(lp: &Loop2, center: Vec2, radius: f64) {
        assert_eq!(super::super::loop_circle(lp), Some((center, radius)));
        let json = serde_json::to_value(super::super::loop_shape(lp)).unwrap();
        assert_eq!(json["circle"]["center"], json!([center.x, center.y]));
        assert_eq!(json["circle"]["radius"].as_f64(), Some(radius));
        for style in styles() {
            let shapes = super::super::flat_loop_shapes(lp, style).unwrap();
            assert_eq!(shapes.len(), 1);
            assert!(matches!(shapes[0], DxfShape::Circle { c, r } if c == center && r == radius), "{style:?}: {shapes:?}");
        }
    }
    fn expect_arcs(lp: &Loop2) {
        assert!(super::super::loop_circle(lp).is_none());
        assert!(solidcore_ops::prism::loop_circle(lp).is_none(), "generic recognition must remain strict");
        let json = serde_json::to_value(super::super::loop_shape(lp)).unwrap();
        assert!(json.get("circle").is_none());
        let segments = json["segments"].as_array().unwrap();
        assert_eq!(segments.len(), lp.segs.len());
        assert!(segments.iter().all(|s| s.get("arc").is_some()));
        for style in styles() {
            let shapes = super::super::flat_loop_shapes(lp, style).unwrap();
            assert_eq!(shapes.len(), lp.segs.len());
            assert!(shapes.iter().all(|s| matches!(s, DxfShape::Arc { .. })), "{style:?}: {shapes:?}");
        }
    }
    #[test]
    fn trimmed_cap_support_witness_is_one_flat_circle_but_not_a_generic_circle() {
        // Captured source supports from the existing original regression, not new expected output.
        let center = Vec2::new(25.0, 25.0);
        let radius = 1.9999999999999987;
        let second = Vec2::new(24.999999999999996, 25.0);
        let other_radius = 2.0000000000000004;
        assert!(center.dist(second) > 0.0 && center.dist(second) < 1e-9);
        assert!(other_radius > radius && other_radius - radius < 1e-9);
        let lp = halves(center, radius, second, other_radius);
        assert!(solidcore_ops::prism::loop_circle(&lp).is_none(), "unequal stored supports remain unequal in generic geom");
        expect_circle(&lp, center, radius);
    }
    #[test]
    fn center_distance_at_and_just_past_bound_keeps_both_arcs() {
        let center = Vec2::new(0.0, 0.0);
        let inside = 0.5e-9;
        expect_circle(&halves(center, 1.0, Vec2::new(inside, 0.0), 1.0), center, 1.0);
        for offset in [1e-9, f64::from_bits(1e-9_f64.to_bits() + 1)] {
            let second = Vec2::new(offset, 0.0);
            assert!(second.dist(center) >= 1e-9);
            expect_arcs(&halves(center, 1.0, second, 1.0));
        }
    }
    #[test]
    fn radius_difference_just_past_bound_keeps_both_arcs() {
        let center = Vec2::new(0.0, 0.0);
        let radius = 1.0;
        let inside = radius + 0.5e-9;
        let outside = f64::from_bits((radius + 1e-9_f64).to_bits() + 1);
        assert!(inside - radius < 1e-9);
        assert!(outside - radius > 1e-9);
        expect_circle(&halves(center, radius, center, inside), center, radius);
        expect_arcs(&halves(center, radius, center, outside));
    }
    #[test]
    fn angular_sum_just_past_one_turn_bound_keeps_both_arcs() {
        let center = Vec2::new(0.0, 0.0);
        let mut inside = halves(center, 1.0, center, 1.0);
        inside.segs[1] = Seg2::Arc { center, radius: 1.0, start: PI, sweep: PI + 0.5e-9 };
        expect_circle(&inside, center, 1.0);
        let total = f64::from_bits((TAU + 1e-9).to_bits() + 1);
        let second = total - PI;
        assert!(((PI + second).abs() - TAU).abs() > 1e-9);
        let outside = Loop2 {
            segs: vec![Seg2::Arc { center, radius: 1.0, start: 0.0, sweep: PI }, Seg2::Arc { center, radius: 1.0, start: PI, sweep: second }],
        };
        expect_arcs(&outside);
    }
    #[test]
    fn double_turn_is_four_arcs_in_both_flat_export_styles() {
        let center = Vec2::new(0.0, 0.0);
        let lp = Loop2 { segs: (0..4).map(|i| Seg2::Arc { center, radius: 1.0, start: f64::from(i) * PI, sweep: PI }).collect() };
        expect_arcs(&lp);
    }
    #[test]
    fn mixed_signs_with_net_one_turn_are_still_four_arcs() {
        let center = Vec2::new(0.0, 0.0);
        let lp = Loop2 {
            segs: [PI, PI, PI, -PI]
                .into_iter()
                .enumerate()
                .map(|(i, sweep)| Seg2::Arc { center, radius: 1.0, start: i as f64 * PI, sweep })
                .collect(),
        };
        assert_eq!([PI, PI, PI, -PI].iter().sum::<f64>(), TAU);
        expect_arcs(&lp);
    }
    #[test]
    fn caller_merge_never_bypasses_shared_converter_admission() {
        let center = Vec2::new(0.0, 0.0);
        let invalid_start = Loop2 { segs: vec![Seg2::Arc { center, radius: 1.0, start: f64::NAN, sweep: TAU }] };
        assert!(super::super::loop_circle(&invalid_start).is_some(), "historical helper ignores start; admission must not");
        for style in styles() {
            assert!(super::super::flat_loop_shapes(&invalid_start, style).is_err());
        }
        let valid = halves(center, 1.0, center, 1.0);
        assert!(super::super::flat_loop_shapes(&valid, DxfPathStyle::Arcs { tolerance_mm: f64::NAN }).is_err());
        // The existing shared path cap stays200000 even if a caller could merge to one entity.
        let over_budget = Loop2 { segs: vec![Seg2::Arc { center, radius: 1.0, start: 0.0, sweep: TAU / 200001.0 }; 200001] };
        assert!(super::super::loop_circle(&over_budget).is_some());
        for style in styles() {
            assert!(super::super::flat_loop_shapes(&over_budget, style).is_err());
        }
    }
}

// The sheet caller can certify one complete circle even when an individual arc
// collapses at writer precision. Generic path recognition retains its stricter
// support-identity and per-piece representation checks.
#[test]
fn legacy_caller_circle_keeps_an_admitted_sub_writer_precision_arc() {
    use solidcore_record::Loop2;
    use solidcraft_io::{DxfPath, DxfPathStyle, DxfShape, path_shapes};

    let center = Vec2::ZERO;
    let lp = Loop2 {
        segs: vec![
            Seg2::Arc { center, radius: 8.0, start: 0.0, sweep: 1e-16 },
            Seg2::Arc { center: Vec2::new(4e-15, 0.0), radius: 8.0, start: 1e-16, sweep: PI },
            Seg2::Arc { center, radius: 8.0, start: PI, sweep: PI },
        ],
    };
    assert_eq!(super::loop_circle(&lp), Some((center, 8.0)));
    assert!(solidcore_ops::prism::loop_circle(&lp).is_none(), "the generic recognizer must retain exact centre identity");
    let error = path_shapes(DxfPath::Closed(&lp), DxfPathStyle::Exact).unwrap_err();
    assert!(error.to_string().contains("arc endpoints collapse"));

    // Start with a real command-produced sheet's admitted rule/frame and replace only
    // its public source outline. This tests the SheetBody/flat_regions/export API,
    // not a claim about ordinary sketch-command or saved-design reachability.
    let session = plate_with(&[]);
    let mut sheet = session.model.state().sheets[0].clone();
    sheet.base.outer = lp.clone();
    let flat = sheet.flat_regions().expect("the public sheet flat-region route admits this outline");
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].outer.segs, lp.segs, "flat-region admission must preserve the actual witness pieces");
    assert_eq!(super::loop_circle(&flat[0].outer), Some((center, 8.0)));

    // Cam's written caller contract promises the same CIRCLE here. These are
    // expected old-caller oracles, not an observed RED/GREEN result.
    for style in [DxfPathStyle::Exact, DxfPathStyle::Arcs { tolerance_mm: 0.001 }] {
        let shapes = super::flat_loop_shapes(&flat[0].outer, style).expect("admitted legacy whole circle remains exportable");
        assert!(matches!(shapes.as_slice(), [DxfShape::Circle { c, r }] if *c == center && *r == 8.0));
    }
    let dxf = super::flat_dxf(&sheet, &super::DxfOptions::default()).expect("the real default flat-pattern exporter retains its whole-circle result");
    let es = entities(&dxf);
    assert_eq!((count(&es, "CIRCLE"), count(&es, "ARC"), count(&es, "LINE")), (1, 0, 0));
}
