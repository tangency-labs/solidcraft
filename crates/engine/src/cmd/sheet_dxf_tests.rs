//! The flat pattern's DXF keeps ellipses and free-form curves as ELLIPSE and SPLINE entities, and
//! a laser-cutting shop can use it as it is: layers by role, bend notes, letters cut or etched,
//! one file per sheet body.

use std::f64::consts::{PI, TAU};

use serde_json::{Value, json};
use solidcraft_geom::{Seg2, Vec2};

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
fn loops(s: &Session) -> Vec<solidcraft_geom::Loop2> {
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
        regions.iter().max_by(|a, b| a.area().total_cmp(&b.area())).map(|r| r.outer.clone()).unwrap_or(solidcraft_geom::Loop2 { segs: Vec::new() });
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

// Outputs recorded from the pre-conversion exporter at 0312808b92d4523894d2af57c99401c65886e24d.
// A fresh-process comparison matched all 53 payloads on 2026-10-26 before conversion changes.
// These original byte pins are fixed expectations, never regenerated by the test.
#[cfg(not(target_arch = "wasm32"))]
mod byte_regressions {
    use super::*;
    use sha2::{Digest, Sha256};

    type ExpectedOutput = (&'static str, usize, &'static str);

    const EXPECTED_OUTPUTS: [ExpectedOutput; 53] = [
        ("circle-cutout-shapes.json", 48, "c634654c54b427c376b74eb30facd6c6d4cfa41ad4ae5dd51d3b31afc76ed458"),
        ("circle-two-halves-exact.dxf", 3111, "c98ab7a7e331e312525ad99234bb48d5135e38d36a72f8b4bf2be6d3182dffd7"),
        ("circle-two-halves-arcs.dxf", 3111, "c98ab7a7e331e312525ad99234bb48d5135e38d36a72f8b4bf2be6d3182dffd7"),
        ("round-ended-slot-exact.dxf", 3486, "ce06b45cc49ce900d82f9a8b532ccac2751fa039abc1fb40046c4ce13c77deb1"),
        ("round-ended-slot-arcs.dxf", 3486, "ce06b45cc49ce900d82f9a8b532ccac2751fa039abc1fb40046c4ce13c77deb1"),
        ("ellipse-exact.dxf", 3199, "6a8b64f71ee0c4b1fd7b9d63ab0709c6c1878a65cf05281220b49730684a4f89"),
        ("ellipse-arcs.dxf", 14381, "afe1c9fa34d3982108a8d56d2d11af9b14c8788385e04222256f71819686597d"),
        ("cubic-spline-exact.dxf", 4109, "fedda740d4e0e38c501730385adcd42b635887fef300d6a9a59e48c160b9fa19"),
        ("cubic-spline-arcs.dxf", 11094, "209e16b90beb40b99f54d29de98affee686ab97d906078976cc55ba1e99fdf77"),
        ("degree-nine-exact.dxf", 3974, "a32cc2e579b520e1d0d4e47a37bf58e6800da9758b2b4d95b34ec55048cd4705"),
        ("degree-nine-arcs.dxf", 11885, "42eeadbdc9963a7b9602a47789a02b98951c6c2d0c628198760a4c5eeddd5b1c"),
        ("text-outlines-exact.dxf", 11267, "23953beaa0a0ab2b9ac6938b9705d8f9b88ad3df02e6972a5013aaaab644611f"),
        ("text-outlines-arcs.dxf", 23531, "c3adba4518e067d1c78d5e63c6203029cb741630f14447e43ee26f96fb20876c"),
        ("tray-default.dxf", 24381, "1239b1d78db1f5c3e5a3d6a0cfd43530e51f5548f669d3411b94fd179b1592c6"),
        ("tray-exact-cut-bends-true-roles.dxf", 25300, "c58cf63afb438c80943541cfaf85b1b40c8cc2a08f7ded78ecd826b8d6159394"),
        ("tray-exact-cut-bends-true-single.dxf", 24167, "f8ede11b566e9f8bc547fec9e7fda816a8d50b51c2c9e47eaa474ca303e6090f"),
        ("tray-exact-cut-bends-false-roles.dxf", 24758, "90a3f799c989acc69c5d968486bd20eea4eca33630d1849183c3f32f780c05b4"),
        ("tray-exact-cut-bends-false-single.dxf", 23651, "cc6498088bd2edcb9ccf115c665dc3b5fb4dc56bf4c7366ac2f1ced1a922cb16"),
        ("tray-exact-etch-bends-true-roles.dxf", 25300, "df421a23079401e89ba029620e2e377a4a5c4cc1a386391abeb0ad2885814d47"),
        ("tray-exact-etch-bends-true-single.dxf", 24167, "03606ca21dcc3f8bc3f68f6fc0af8f459162dab6ed6b444f10b67f229876f093"),
        ("tray-exact-etch-bends-false-roles.dxf", 24758, "0cd8654b1da7505ba13124f9f0a0ecfeb4687a0c0fcaa25dd87edabac1f63c3a"),
        ("tray-exact-etch-bends-false-single.dxf", 23651, "14a3ea13772c7f64675c6ff0a017df56664cd89a48ecced2c600a07cba95c8d6"),
        ("tray-arcs-cut-bends-true-roles.dxf", 42484, "e298e9eb113995884a4764210463b86b57ec30080213b182aedcee9e46d1912b"),
        ("tray-arcs-cut-bends-true-single.dxf", 40982, "2346420e716e8325fd513919f323c1d22fa871694e88b720f034d2018ff18743"),
        ("tray-arcs-cut-bends-false-roles.dxf", 41938, "6988b20d8030ee46071ad0c8e86f65d54d1be3755b7ead04a2bb8c21021defde"),
        ("tray-arcs-cut-bends-false-single.dxf", 40462, "4d9b84ba711f7421bcd1c6833d12ad47f3da16ac949f32347b8392459f0495d1"),
        ("tray-arcs-etch-bends-true-roles.dxf", 42484, "e0eda322c4c75619de535bfd279f4e5616d22f30fdfa7ee0c65b86366ac5b796"),
        ("tray-arcs-etch-bends-true-single.dxf", 40982, "19dd6d5cdfbf8177f28d74fd29f1760c54b122071dc61401df86f16bfd744fa3"),
        ("tray-arcs-etch-bends-false-roles.dxf", 41938, "41022f19af172e40307dd37abac43982979bc42992071643dab48de2a59aba01"),
        ("tray-arcs-etch-bends-false-single.dxf", 40462, "8e6a94cb3a126de3268783e55bde1c7c8c145a202384bbf446ef336f1cdc31e7"),
        ("rule-bend-down-exact-bends-true-roles.dxf", 4065, "b927b9ffe6e49e6289e719b57a8ad8e204fa44dbeb0a23fcdf65aa5a116dbe87"),
        ("rule-bend-down-exact-bends-false-single.dxf", 3308, "f97cc60bbe0cab1b75bf7965d750565b2593183e029699915c8ac6bd3ac0b9c0"),
        ("rule-bend-down-arcs-bends-true-roles.dxf", 4065, "b927b9ffe6e49e6289e719b57a8ad8e204fa44dbeb0a23fcdf65aa5a116dbe87"),
        ("rule-bend-down-arcs-bends-false-single.dxf", 3308, "f97cc60bbe0cab1b75bf7965d750565b2593183e029699915c8ac6bd3ac0b9c0"),
        ("rule-included-down-exact-bends-true-roles.dxf", 4066, "3ee6825be836054c2869ba959d611292d34c8b109746ba4b98b82f6cdc064aa2"),
        ("rule-included-down-exact-bends-false-single.dxf", 3309, "515ab9cace64fe66758cfb601b7d9614f2e62d35d8a1efbeb8ef1d6890130d1d"),
        ("rule-included-down-arcs-bends-true-roles.dxf", 4066, "3ee6825be836054c2869ba959d611292d34c8b109746ba4b98b82f6cdc064aa2"),
        ("rule-included-down-arcs-bends-false-single.dxf", 3309, "515ab9cace64fe66758cfb601b7d9614f2e62d35d8a1efbeb8ef1d6890130d1d"),
        ("pocket-and-engraved-text-exact.dxf", 20843, "df0b996889dc152e66b01b6d56f35337edd109b15d55ab5693ea86c079d14a22"),
        ("pocket-and-engraved-text-arcs.dxf", 37997, "8d92294fc2ab84d73fbae2df73f5060252db9b54f8c0c040ebf05f6102568add"),
        ("perforation-exact.dxf", 6480, "703ac2570c91da7d4fa41e889e9f03e48611c9fee91876325be877ab7c2aa24c"),
        ("perforation-arcs.dxf", 6480, "703ac2570c91da7d4fa41e889e9f03e48611c9fee91876325be877ab7c2aa24c"),
        ("per-body-0-exact.dxf", 3014, "19bda8ed16b26471a3b417147d7cc4642fda9bdea548f6c61725100944fbdbc6"),
        ("per-body-1-exact.dxf", 3734, "0c7d54b955202fd01cc4771cacbfe7cdb4fbbbfd6944fbba3fc1f1718ce6f148"),
        ("per-body-0-arcs.dxf", 3014, "19bda8ed16b26471a3b417147d7cc4642fda9bdea548f6c61725100944fbdbc6"),
        ("per-body-1-arcs.dxf", 3734, "0c7d54b955202fd01cc4771cacbfe7cdb4fbbbfd6944fbba3fc1f1718ce6f148"),
        ("writer-bmp-controls.dxf", 2430, "4fd801becee559ff017c0c84d80b881d8da7cc9e960ea76e00ef311035bcfd73"),
        ("writer-bmp-cap.dxf", 4138, "d17379aa2ad648e6d356df5b58f2222e28472554248a29407cf932507fafdcfd"),
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
