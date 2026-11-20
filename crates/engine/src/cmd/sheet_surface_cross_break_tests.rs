//! Cross Break on a sheet made from surfaces (#8345, #6748): two diagonal creases marked on a flat
//! panel, kept in its chart, listed on the flat and written to the DXF. The first two tests are
//! the cases of the flat-first Cross Break (#8346), the spec for the rebuild.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn flat(s: &mut Session) -> Value {
    run(s, "sheet.flat_pattern", json!({}))
}

fn dxf(s: &mut Session, p: Value) -> String {
    run(s, "sheet.export_dxf", p)["dxf"].as_str().unwrap_or_default().to_string()
}

/// A 0.8 mm sheet whose base flange is the closed outline `outline` on XY.
fn outline_blank(s: &mut Session, outline: Value) {
    run(s, "sheet.manage_rules", json!({"name": "Steel 0.8", "thickness": "0.8", "active": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Blank"}));
    run(s, "sketch.line", json!({"points": outline, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Blank"}));
}

/// A 2 mm rule (R2, K 0.4), a 100 x 60 base on XY and a 40 mm wall up from its y = 0 edge. Returns
/// the wall's outer face's y.
fn tray_wall(s: &mut Session) -> f64 {
    run(s, "sheet.manage_rules", json!({"name": "S2", "thickness": "2 mm", "bend_radius": "2 mm", "k_factor": "0.4", "active": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Base"}));
    run(s, "sheet.flange", json!({"edges": [[50, 0, 2]], "height": 40}));
    run(s, "inspect.measure", json!({}))["bodies"][0]["bbox"]["min"][1].as_f64().unwrap_or(f64::NAN)
}

/// The LINE entities of a DXF on `layer`: (x1, y1, x2, y2).
fn dxf_lines_on(dxf: &str, layer: &str) -> Vec<[f64; 4]> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i] == "0" && lines[i + 1] == "LINE" {
            let (mut on, mut p) = ("", [0.0; 4]);
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j] != "0" {
                let v = lines[j + 1].parse().unwrap_or(f64::NAN);
                match lines[j] {
                    "8" => on = lines[j + 1],
                    "10" => p[0] = v,
                    "20" => p[1] = v,
                    "11" => p[2] = v,
                    "21" => p[3] = v,
                    _ => {}
                }
                j += 2;
            }
            if on == layer {
                out.push(p);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// The cross breaks of the flat pattern's listing: (direction, [x1, y1, x2, y2] of each crease).
fn cross_break_lines(f: &Value) -> Vec<(String, Vec<[f64; 4]>)> {
    let n = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    f["cross_breaks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let lines = c["lines"].as_array().into_iter().flatten().map(|l| [n(&l[0][0]), n(&l[0][1]), n(&l[1][0]), n(&l[1][1])]).collect();
            (c["direction"].as_str().unwrap_or("").to_string(), lines)
        })
        .collect()
}

fn close(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// The creases the view draws, each as the middle of its pair (the top face's segment and the
/// bottom one's): a thickness's half into the material.
fn creases_in_the_material(s: &Session) -> Vec<(solidcore_record::Vec3, solidcore_record::Vec3)> {
    let st = s.model.state();
    let segments = st.surface_sheets.first().map(|sk| sk.cross_break_segments()).unwrap_or_default();
    segments.chunks(2).filter_map(|p| p.get(1).map(|q| ((p[0].0 + q.0) * 0.5, (p[0].1 + q.1) * 0.5))).collect()
}

fn inside(s: &Session, p: solidcore_record::Vec3) -> bool {
    s.model.state().bodies.first().is_some_and(|b| b.body.classify_point(p).ok() == Some(solidcraft_kernel::PointClass::Inside))
}

/// #8345: a cross break on a 300 x 200 panel is its two diagonals, 5 mm short of the corners: in
/// the flat pattern and on its own DXF layer, while the B-rep, the outline and the volume stay as
/// they were. A second one on the same panel, and a pick off the panel, are refused.
#[test]
fn cross_break_marks_the_diagonals_in_the_flat_pattern_and_dxf() {
    let mut s = Session::default();
    outline_blank(&mut s, json!([[0, 0], [300, 0], [300, 200], [0, 200]]));
    let base = s.doc.features.last().map(|f| f.id).unwrap_or_default();
    let (before, volume_before, outline_before) = (run(&mut s, "inspect.measure", json!({})), volume(&mut s), flat(&mut s)["outline"].clone());
    run(&mut s, "sheet.cross_break", json!({"face": [150, 100, 0.8], "inset": "5 mm"}));
    let f = flat(&mut s);
    // 5 mm along each diagonal of the 300 x 200 panel (whose length is √130000).
    let (dx, dy) = (5.0 * 300.0 / 130_000f64.sqrt(), 5.0 * 200.0 / 130_000f64.sqrt());
    let want = [[dx, dy, 300.0 - dx, 200.0 - dy], [dx, 200.0 - dy, 300.0 - dx, dy]];
    let listed = cross_break_lines(&f);
    assert_eq!(listed.len(), 1, "{}", f["cross_breaks"]);
    assert_eq!(listed[0].0, "down", "bows down by default");
    assert!(listed[0].1.len() == 2 && want.iter().all(|w| listed[0].1.iter().any(|l| close(l, w))), "{:?}", listed[0].1);
    // Listed by its panel's key: the base flange's panel is its feature's.
    assert_eq!(f["cross_breaks"][0]["panel"], json!(format!("F{base}")));
    assert_eq!(f["cross_breaks"][0]["depth"].as_f64(), Some(1.0), "the default depth");
    let d = dxf(&mut s, json!({}));
    let creases = dxf_lines_on(&d, "CROSS_BREAK_DOWN");
    assert!(creases.len() == 2 && want.iter().all(|w| creases.iter().any(|l| close(l, w))), "{creases:?}");
    assert!(dxf_lines_on(&d, "CROSS_BREAK_UP").is_empty());
    // A marking only: the same faces, volume and outline.
    assert_eq!(run(&mut s, "inspect.measure", json!({}))["bodies"][0]["faces"], before["bodies"][0]["faces"]);
    assert!(rel(volume(&mut s), volume_before) < 1e-12);
    assert_eq!(f["outline"], outline_before);
    // A cutter that would cut the bend lines gets none of the creases either.
    assert!(!dxf(&mut s, json!({"bend_lines": false})).contains("CROSS_BREAK"), "no crease without the bend lines");
    let doc = s.doc.to_json();
    for bad in [
        json!({"face": [150, 100, 0.8]}),
        json!({"face": [150, 100, 5]}),
        json!({"face": [400, 100, 0.8]}),
        json!({"face": [150, 100, 0.8], "direction": "sideways"}),
        json!({"face": [150, 100, 0.8], "inset": "-1 mm"}),
        json!({"face": [150, 100, 0.8], "inset": "200 mm"}),
        json!({"face": [150, 100, 0.8], "depth": "0 mm"}),
    ] {
        assert!(s.execute("sheet.cross_break", &bad).is_err(), "{bad} must be refused");
        assert_eq!(s.doc.to_json(), doc, "{bad} leaves the design as it was");
    }
}

/// #8345: a cross break bowing up, picked on the bottom face, stays with the sheet through
/// unfold, refold and save and reopen.
#[test]
fn cross_break_stays_with_its_panel_through_unfold_and_reopen() {
    let mut s = Session::default();
    outline_blank(&mut s, json!([[0, 0], [300, 0], [300, 200], [0, 200]]));
    run(&mut s, "sheet.cross_break", json!({"face": [100, 50, 0], "direction": "up", "depth": "2 mm"}));
    let first = cross_break_lines(&flat(&mut s));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, "up");
    assert_eq!(dxf_lines_on(&dxf(&mut s, json!({})), "CROSS_BREAK_UP").len(), 2);
    run(&mut s, "sheet.unfold", json!({}));
    assert_eq!(cross_break_lines(&flat(&mut s)), first, "unfolded, the creases stay where they were");
    run(&mut s, "sheet.refold", json!({}));
    assert_eq!(cross_break_lines(&flat(&mut s)), first);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap_or_else(|e| panic!("{e}")));
    reopened.refresh();
    assert!(reopened.model.results.iter().all(|r| r.error.is_none()), "{:?}", reopened.model.results);
    assert_eq!(cross_break_lines(&flat(&mut reopened)), first);
    assert_eq!(flat(&mut reopened)["cross_breaks"][0]["depth"].as_f64(), Some(2.0));
}

/// A cross break on a folded wall, not the base: it marks that panel (keyed by the flange and the
/// base panel's rim it stands on), lies on the wall's part of the flat corner to corner, and is
/// drawn on the wall's faces in the view. The body does not change.
#[test]
fn a_cross_break_on_a_flanges_wall_marks_that_panel() {
    let mut s = Session::default();
    let y0 = tray_wall(&mut s);
    let (volume_before, faces_before) = (volume(&mut s), run(&mut s, "inspect.measure", json!({}))["bodies"][0]["faces"].clone());
    let wall_flat = flat(&mut s);
    run(&mut s, "sheet.cross_break", json!({"face": [50, y0, 25]}));
    let f = flat(&mut s);
    let ids: Vec<u64> = s.doc.features.iter().map(|f| f.id).collect();
    let (base, flange) = (ids[ids.len() - 3], ids[ids.len() - 2]);
    let wall = f["cross_breaks"][0]["panel"].as_str().unwrap_or_default().to_string();
    assert!(wall.starts_with(&format!("F{flange}/F{base}/side.")), "{wall}");
    assert!(s.model.state().surface_sheets.first().is_some_and(|sk| sk.sheet.panels.iter().any(|p| p.name == wall)), "{wall}");
    // In the flat the wall lies past the base's y = 0 edge and its bend: both creases run there,
    // from one end of the wall to the other.
    let listed = cross_break_lines(&f);
    assert_eq!(listed.len(), 1);
    let zone_min_y = wall_flat["bend_zones"][0][2].as_f64().unwrap_or(f64::NAN);
    for l in &listed[0].1 {
        assert!(l[1] < zone_min_y + 1e-9 && l[3] < zone_min_y + 1e-9, "on the wall: {l:?} (its bend from y {zone_min_y})");
        assert!((l[0] - 0.0).abs() < 1e-6 && (l[2] - 100.0).abs() < 1e-6, "corner to corner: {l:?}");
    }
    // The view draws them on the wall: midway between their two segments is the wall's material,
    // and both lie in the wall's plane, up its height.
    let creases = creases_in_the_material(&s);
    assert_eq!(creases.len(), 2);
    for (a, b) in &creases {
        assert!(inside(&s, (*a + *b) * 0.5), "{a:?} {b:?}");
        assert!((a.y - (y0 + 1.0)).abs() < 1e-6 && (b.y - (y0 + 1.0)).abs() < 1e-6, "{a:?} {b:?}");
        assert!(a.z.min(b.z) > 3.9, "up the wall, past its bend: {a:?} {b:?}");
    }
    assert!(rel(volume(&mut s), volume_before) < 1e-12);
    assert_eq!(run(&mut s, "inspect.measure", json!({}))["bodies"][0]["faces"], faces_before);
    // The base takes one of its own; the wall a second one does not.
    run(&mut s, "sheet.cross_break", json!({"face": [50, 30, 2], "direction": "up"}));
    assert_eq!(cross_break_lines(&flat(&mut s)).len(), 2);
    let e = s.execute("sheet.cross_break", &json!({"face": [30, y0, 20]})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains(solidcraft_doc::CROSS_BREAK_TWICE), "{e}");
}

/// A fold across a panel with a cross break would run through its creases: refused, the design as
/// it was. A fold of another panel carries the cross break with its panel: the flat keeps it where
/// it was, and the view draws it on the panel where the fold put it.
#[test]
fn a_fold_through_a_cross_break_is_refused_and_one_elsewhere_carries_it() {
    let refused = |s: &mut Session, p: Value| {
        let doc = s.doc.to_json();
        let e = s.execute("sheet.fold", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains(solidcraft_doc::CROSS_BREAK_IN_FOLD), "{p}: {e}");
        assert_eq!(s.doc.to_json(), doc, "{p}: the refused fold leaves the design as it was");
    };
    // Across the base, through the base's creases.
    let mut s = Session::default();
    let y0 = tray_wall(&mut s);
    run(&mut s, "sheet.cross_break", json!({"face": [50, 30, 2]}));
    refused(&mut s, json!({"points": [[0, 40, 2], [100, 40, 2]], "fixed": [50, 50, 2]}));
    // Across the wall, its top turned in, through the wall's creases.
    let mut s = Session::default();
    tray_wall(&mut s);
    run(&mut s, "sheet.cross_break", json!({"face": [50, y0, 15]}));
    refused(&mut s, json!({"points": [[0, y0, 30], [100, y0, 30]]}));
    // Across the base by y = 50, the side with the wall moving: the wall's cross break goes with it.
    let listed = cross_break_lines(&flat(&mut s));
    run(&mut s, "sheet.fold", json!({"points": [[0, 50, 2], [100, 50, 2]], "fixed": [50, 55, 2]}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    assert_eq!(cross_break_lines(&flat(&mut s)), listed, "the flat keeps the wall's creases where they were");
    let creases = creases_in_the_material(&s);
    assert_eq!(creases.len(), 2);
    for (a, b) in &creases {
        assert!(inside(&s, (*a + *b) * 0.5), "on the wall where the fold put it: {a:?} {b:?}");
        assert!(a.y.max(b.y) > y0 + 3.0, "the wall moved: {a:?} {b:?}");
    }
}

#[test]
fn hostile_cross_break_params_never_panic() {
    let mut s = Session::default();
    outline_blank(&mut s, json!([[0, 0], [300, 0], [300, 200], [0, 200]]));
    for p in [
        json!({}),
        json!({"face": null}),
        json!({"face": [1e308, -1e308, 1e308]}),
        json!({"face": "middle"}),
        json!({"face": [150, 100]}),
        json!({"face": [150, 100, 0.8], "inset": "1e300 mm"}),
        json!({"face": [150, 100, 0.8], "depth": "1e300 mm"}),
        json!({"face": [150, 100, 0.8], "inset": "nowhere + 1"}),
        json!({"face": [150, 100, 0.8], "direction": 3}),
        json!({"face": [150, 100, 0.8], "body": 5}),
        json!({"face": [150, 100, 0.8], "body": ""}),
        json!({"face": [150, 100, 0.8], "body": "Nope"}),
        json!({"face": [150, 100, 0.8], "name": "x".repeat(10_000)}),
        json!([1, 2, 3]),
        json!("cross break"),
    ] {
        let _ = s.execute("sheet.cross_break", &p);
    }
}
