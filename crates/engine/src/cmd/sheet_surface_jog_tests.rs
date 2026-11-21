//! Jog on a sheet made from surfaces (#6046, #6748): an offset step across a flat panel, made as
//! two folds. These are the cases of the flat-first Jog (#6047), the spec for the rebuild.

use std::f64::consts::FRAC_PI_2;

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

fn bbox(s: &mut Session) -> Value {
    run(s, "inspect.measure", json!({}))["bodies"][0]["bbox"].clone()
}

/// A w x h base flange on XY.
fn base(s: &mut Session, w: f64, h: f64) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [w, h]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Base"}));
}

/// A 120 x 60 plate of the rule 2 mm thick, R2, K 0.4.
fn jog_plate(s: &mut Session) {
    run(s, "sheet.manage_rules", json!({"name": "Jog", "active": true, "thickness": "2 mm", "k_factor": 0.4, "bend_radius": "2 mm"}));
    base(s, 120.0, 60.0);
}

/// The jog of a plate along x = 50 (the small side, x < 50, steps).
fn jog(s: &mut Session, extra: Value) -> Result<Value, crate::EngineError> {
    let mut q = json!({"points": [[50, 0, 2], [50, 60, 2]], "fixed": [100, 30, 2]});
    for (k, v) in extra.as_object().into_iter().flatten() {
        q[k] = v.clone();
    }
    s.execute("sheet.jog", &q)
}

fn z_range(s: &mut Session) -> (f64, f64) {
    let b = bbox(s);
    (b["min"][2].as_f64().unwrap_or(f64::NAN), b["max"][2].as_f64().unwrap_or(f64::NAN))
}

#[test]
fn jog_keeps_the_flat_size_and_makes_two_opposite_bends() {
    let ba = FRAC_PI_2 * (2.0 + 0.4 * 2.0);
    let mut s = Session::default();
    jog_plate(&mut s);
    let v0 = volume(&mut s);
    jog(&mut s, json!({"offset": 20})).unwrap();
    let f = flat(&mut s);
    assert!((f["flat_size_mm"][0].as_f64().unwrap_or(0.0) - 120.0).abs() < 1e-6, "{f}");
    assert!((f["flat_size_mm"][1].as_f64().unwrap_or(0.0) - 60.0).abs() < 1e-6, "{f}");
    let bends = f["bends"].as_array().cloned().unwrap_or_default();
    assert_eq!(bends.len(), 2, "{f}");
    let dirs: Vec<&str> = bends.iter().filter_map(|b| b["direction"].as_str()).collect();
    assert_eq!(dirs.iter().filter(|d| **d == "up").count(), 1, "{f}");
    assert_eq!(dirs.iter().filter(|d| **d == "down").count(), 1, "{f}");
    for b in &bends {
        assert!((b["angle_deg"].as_f64().unwrap_or(0.0) - 90.0).abs() < 1e-9, "{f}");
        assert!((b["length"].as_f64().unwrap_or(0.0) - 60.0).abs() < 1e-6, "{f}");
        assert!((b["allowance"].as_f64().unwrap_or(0.0) - ba).abs() < 1e-9, "{f}");
    }
    // One body; the bends' material is the sheet's own (two bends turned, none added).
    assert_eq!(s.model.state().bodies.len(), 1);
    let want = v0 - 2.0 * ba * 2.0 * 60.0 + 2.0 * FRAC_PI_2 * 2.0 * (2.0 + 1.0) * 60.0;
    assert!(rel(volume(&mut s), want) < 1e-4, "{} {want}", volume(&mut s));
}

#[test]
fn jog_step_height_is_the_offset_for_every_measure_and_angle() {
    // Outer faces (outside), inner faces (inside) or a face and the same face (faces).
    for angle in [90.0_f64, 45.0] {
        for (measure, offset, shift) in [("outside", 20.0, -2.0), ("inside", 16.0, 2.0), ("faces", 18.0, 0.0)] {
            let mut s = Session::default();
            jog_plate(&mut s);
            let q = json!({"offset": offset, "measure": measure, "angle": format!("{angle} deg")});
            jog(&mut s, q).unwrap_or_else(|e| panic!("{measure} {angle}: {e}"));
            // The plate's own lower face is z = 0; the stepped part's same face is H above it.
            let (lo, hi) = z_range(&mut s);
            let h = offset + shift;
            assert!(lo.abs() < 1e-6, "{measure} {angle}: {lo}");
            assert!((hi - (h + 2.0)).abs() < 1e-6, "{measure} {angle}: top {hi} want {}", h + 2.0);
        }
    }
    // The three measures describe the same step when their numbers differ by the thickness.
    let mut s = Session::default();
    jog_plate(&mut s);
    jog(&mut s, json!({"offset": 20, "measure": "outside"})).unwrap();
    let (_, outside) = z_range(&mut s);
    assert!((outside - 20.0).abs() < 1e-6, "outer faces exactly 20 apart: {outside}");
}

#[test]
fn jog_line_position_moves_the_step_by_half_the_span_and_the_span() {
    let ba = FRAC_PI_2 * (2.0 + 0.4 * 2.0);
    // At 90°, R2, T2, outside 20: H = 18 and the wall is 12 long.
    let span = 2.0 * ba + 12.0;
    // Where the two bends lie in the flat pattern: from the far end of the second to the line.
    let low_x = |position: &str| {
        let mut s = Session::default();
        jog_plate(&mut s);
        jog(&mut s, json!({"offset": 20, "position": position})).unwrap();
        let f = flat(&mut s);
        let xs: Vec<f64> = f["bends"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|b| [b["start"][0].as_f64().unwrap_or(f64::NAN), b["end"][0].as_f64().unwrap_or(f64::NAN)])
            .collect();
        xs.iter().cloned().fold(f64::MAX, f64::min)
    };
    // The bend table's lines are the bends' centre lines. Start: the zones run from the line
    // (x = 50) toward the side that steps, the span S long, the last one's centre BA / 2 short of it.
    let (start, middle, end) = (low_x("start"), low_x("middle"), low_x("end"));
    assert!((start - (50.0 - span + ba / 2.0)).abs() < 1e-6, "{start}");
    assert!((middle - start - span / 2.0).abs() < 1e-6, "{middle} {start} {span}");
    assert!((end - start - span).abs() < 1e-6, "{end} {start} {span}");
}

#[test]
fn jog_flip_steps_down_and_one_undo_removes_it() {
    let mut s = Session::default();
    jog_plate(&mut s);
    let before = s.doc.to_json();
    jog(&mut s, json!({"offset": 20, "flip": true})).unwrap();
    let (lo, hi) = z_range(&mut s);
    assert!((hi - 2.0).abs() < 1e-6 && (lo + 18.0).abs() < 1e-6, "{lo} {hi}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    assert!(flat(&mut s)["bends"].as_array().is_none_or(Vec::is_empty));
}

#[test]
fn jog_refuses_what_it_cannot_make_and_leaves_the_design_as_it_was() {
    let mut s = Session::default();
    jog_plate(&mut s);
    let before = s.doc.to_json();
    // At 90° the wall is H - 2R - T: an outside offset of 8 is H = 6 = 2R + T, no wall at all.
    let e = jog(&mut s, json!({"offset": 8})).unwrap_err().to_string();
    assert!(e.contains("more than 8.000 mm"), "{e}");
    let e = jog(&mut s, json!({"offset": 3, "measure": "inside"})).unwrap_err().to_string();
    assert!(e.contains("more than 4.000 mm"), "{e}");
    for bad in [
        json!({"offset": 20, "measure": "sideways"}),
        json!({"offset": 20, "position": "centerline"}),
        json!({"offset": "0 / 0 mm"}),
        json!({"offset": -5}),
        json!({"offset": 20, "angle": "0 deg"}),
        json!({"offset": 20, "angle": "180 deg"}),
        json!({"offset": 20, "radius": "-1 mm"}),
        json!({"measure": "outside"}),
        json!({"offset": 20, "points": [[50, 0, 40], [50, 60, 40]]}),
        json!({"offset": 20, "points": [[0.5, 0, 2], [0.5, 60, 2]], "fixed": [100, 30, 2]}),
    ] {
        assert!(jog(&mut s, bad.clone()).is_err(), "{bad}");
        assert_eq!(s.doc.to_json(), before, "{bad}");
    }
    // Lower than the smallest offset at 45°: shorter than the bends need.
    let e = jog(&mut s, json!({"offset": 3, "angle": "45 deg"})).unwrap_err().to_string();
    assert!(e.contains("more than 3.757 mm"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn jog_on_an_unfolded_sheet_asks_to_refold_it() {
    let mut s = Session::default();
    jog_plate(&mut s);
    run(&mut s, "sheet.unfold", json!({}));
    let e = jog(&mut s, json!({"offset": 20})).unwrap_err().to_string();
    assert!(e.contains("refold"), "{e}");
}

/// The bend lines' lowest x in the flat pattern.
fn low_bend_x(s: &mut Session) -> f64 {
    let f = flat(s);
    let xs: Vec<f64> = f["bends"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|b| [b["start"][0].as_f64().unwrap_or(f64::NAN), b["end"][0].as_f64().unwrap_or(f64::NAN)])
        .collect();
    xs.iter().cloned().fold(f64::MAX, f64::min)
}

#[test]
fn jog_along_a_sketch_line_keeps_the_larger_side_and_takes_the_rules_radius() {
    let ba = FRAC_PI_2 * (2.0 + 0.4 * 2.0);
    let mut s = Session::default();
    jog_plate(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 2, "name": "JogLine"}));
    run(&mut s, "sketch.line", json!({"points": [[50, 0], [50, 60]], "ids": ["jl"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.jog", json!({"sketch": "JogLine", "curve": "jl", "offset": 20}));
    // No fixed point: the larger side (x > 50) stays, the smaller steps up 20 outside to outside.
    let (lo, hi) = z_range(&mut s);
    assert!(lo.abs() < 1e-6 && (hi - 20.0).abs() < 1e-6, "{lo} {hi}");
    let b = bbox(&mut s);
    assert!((b["max"][0].as_f64().unwrap_or(0.0) - 120.0).abs() < 1e-6, "{b}");
    // Both bends at the rule's radius in the sheet's thickness, the same as the points give.
    let f = flat(&mut s);
    let bends = f["bends"].as_array().cloned().unwrap_or_default();
    assert_eq!(bends.len(), 2, "{f}");
    for b in &bends {
        assert!((b["allowance"].as_f64().unwrap_or(0.0) - ba).abs() < 1e-9, "{f}");
    }
    let by_line = low_bend_x(&mut s);
    let mut p = Session::default();
    jog_plate(&mut p);
    jog(&mut p, json!({"offset": 20})).unwrap();
    assert!((by_line - low_bend_x(&mut p)).abs() < 1e-6, "{by_line} {}", low_bend_x(&mut p));
}

#[test]
fn jog_on_a_flanges_wall_steps_its_top_and_keeps_the_flat() {
    let ba = FRAC_PI_2 * (2.0 + 0.4 * 2.0);
    let mut s = Session::default();
    jog_plate(&mut s);
    run(&mut s, "sheet.flange", json!({"edges": [[60, 0, 2]], "height": "30 mm", "angle": "90 deg"}));
    let (v0, f0, b0) = (volume(&mut s), flat(&mut s), bbox(&mut s));
    let (y0, z_top) = (b0["min"][1].as_f64().unwrap_or(f64::NAN), b0["max"][2].as_f64().unwrap_or(f64::NAN));
    // Across the wall's outer face, 20 below its top (the jog takes 2 x BA + 2 = 10.8 of it);
    // the base stays.
    let z = z_top - 20.0;
    let q = json!({"points": [[0, y0, z], [120, y0, z]], "fixed": [60, 30, 2], "offset": 10});
    run(&mut s, "sheet.jog", q);
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    assert_eq!(s.model.state().bodies.len(), 1);
    let f = flat(&mut s);
    for i in 0..2 {
        let (got, want) = (f["flat_size_mm"][i].as_f64().unwrap_or(f64::NAN), f0["flat_size_mm"][i].as_f64().unwrap_or(0.0));
        assert!((got - want).abs() < 1e-6, "{f}");
    }
    let bends = f["bends"].as_array().cloned().unwrap_or_default();
    assert_eq!(bends.len(), 3, "the flange's bend and the jog's two: {f}");
    for b in &bends {
        assert!((b["length"].as_f64().unwrap_or(0.0) - 120.0).abs() < 1e-6, "{f}");
    }
    let want = v0 - 2.0 * ba * 2.0 * 120.0 + 2.0 * FRAC_PI_2 * 2.0 * (2.0 + 1.0) * 120.0;
    assert!(rel(volume(&mut s), want) < 1e-4, "{} {want}", volume(&mut s));
    // The wall's top 20 steps 10 outside to outside, H = 8 face to the same face, across y; the
    // bends take 2R + T of the height, the wall between them 8 − 6 = 2 more.
    let b = bbox(&mut s);
    let dy = (b["max"][1].as_f64().unwrap_or(f64::NAN) - b["min"][1].as_f64().unwrap_or(f64::NAN))
        - (b0["max"][1].as_f64().unwrap_or(0.0) - b0["min"][1].as_f64().unwrap_or(0.0));
    assert!((dy - 8.0).abs() < 1e-6 || dy.abs() < 1e-6, "{b0} {b}");
}

#[test]
fn a_design_with_a_jog_saves_and_reopens_as_it_was() {
    let mut s = Session::default();
    jog_plate(&mut s);
    jog(&mut s, json!({"offset": 20, "position": "middle"})).unwrap();
    let (v, f, b) = (volume(&mut s), flat(&mut s), bbox(&mut s));
    let saved = s.doc.to_json();
    assert!(saved.contains("\"sheet_jog\""), "the feature saves as a jog");
    let mut t = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap_or_else(|e| panic!("{e}")));
    t.refresh();
    assert!(t.model.results.iter().all(|r| r.error.is_none()), "{:?}", t.model.results);
    assert!(rel(volume(&mut t), v) < 1e-9, "{} {v}", volume(&mut t));
    assert_eq!(bbox(&mut t), b);
    let g = flat(&mut t);
    assert_eq!(g["flat_size_mm"], f["flat_size_mm"]);
    assert_eq!(g["bends"], f["bends"]);
    assert_eq!(t.doc.to_json(), saved);
}

/// A jog line extends across its panel, as a fold's does; when the side that moves is two tabs,
/// the jog is refused, with the design as it was.
#[test]
fn a_jog_whose_moving_side_is_two_tabs_is_refused() {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "Jog", "active": true, "thickness": "2 mm", "k_factor": 0.4, "bend_radius": "2 mm"}));
    // A 120 x 60 plate with a 40 x 10 slot in from its x = 0 edge: two prongs there.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [120, 0], [120, 60], [0, 60], [0, 35], [40, 35], [40, 25], [0, 25]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    let before = s.doc.to_json();
    let e = jog(&mut s, json!({"offset": 20, "points": [[30, 0, 2], [30, 60, 2]]})).unwrap_err().to_string();
    assert!(e.contains("not supported yet: a jog whose first bend splits the sheet into separate tabs"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    // A line that ends inside the plate jogs it all the way across, as one along its full width.
    let mut short = Session::default();
    jog_plate(&mut short);
    jog(&mut short, json!({"offset": 20, "points": [[50, 0, 2], [50, 30, 2]]})).unwrap();
    let mut full = Session::default();
    jog_plate(&mut full);
    jog(&mut full, json!({"offset": 20})).unwrap();
    assert_eq!(bbox(&mut short), bbox(&mut full));
    assert_eq!(flat(&mut short)["bends"], flat(&mut full)["bends"]);
}

/// A jog decides which side stays as it is made and keeps it, as a fold does (#6748): made with
/// no fixed point on the 120 x 60 plate along x = 50, the larger side (x > 50) stays and the jog
/// stores that side, so the plate still reaches x = 120 and the smaller side steps in from x = 0.
/// Given the other side (`stays`), the smaller side stays at x = 0 and the larger steps instead.
#[test]
fn a_jog_stores_the_side_that_stays_and_takes_the_other_when_asked() {
    let line = json!([[50, 0, 2], [50, 60, 2]]);
    let mut s = Session::default();
    jog_plate(&mut s);
    run(&mut s, "sheet.jog", json!({"points": line, "offset": 20}));
    let side = match s.doc.features.last().map(|f| &f.kind) {
        Some(solidcraft_doc::FeatureKind::SheetJog { stays, .. }) => *stays,
        _ => None,
    };
    let side = side.unwrap_or_else(|| panic!("the jog stores no side"));
    let x = |s: &mut Session| {
        let b = bbox(s);
        (b["min"][0].as_f64().unwrap_or(f64::NAN), b["max"][0].as_f64().unwrap_or(f64::NAN))
    };
    let (lo, hi) = x(&mut s);
    assert!((hi - 120.0).abs() < 1e-6 && lo > 1.0, "{lo} {hi}");
    let other = if side == solidcraft_doc::FoldSide::Left { "right" } else { "left" };
    let mut o = Session::default();
    jog_plate(&mut o);
    run(&mut o, "sheet.jog", json!({"points": line, "offset": 20, "stays": other}));
    let (lo, hi) = x(&mut o);
    assert!(lo.abs() < 1e-6 && hi < 119.0, "{lo} {hi}");
}

/// As for a fold, a side given wins over a fixed point: with `stays`, `fixed` is neither read nor
/// kept, so even a point off the sheet is no reason to refuse the jog, and the jog is the one made
/// with `stays` alone.
#[test]
fn a_jog_given_a_side_ignores_its_fixed_point() {
    let line = json!([[50, 0, 2], [50, 60, 2]]);
    let mut s = Session::default();
    jog_plate(&mut s);
    run(&mut s, "sheet.jog", json!({"points": line, "offset": 20, "stays": "left", "fixed": [500, 500, 500]}));
    let mut o = Session::default();
    jog_plate(&mut o);
    run(&mut o, "sheet.jog", json!({"points": line, "offset": 20, "stays": "left"}));
    let kind = |s: &Session| s.doc.features.last().map(|f| f.kind.clone());
    assert!(matches!(kind(&s), Some(solidcraft_doc::FeatureKind::SheetJog { fixed: None, stays: Some(_), .. })), "{:?}", kind(&s));
    assert_eq!(kind(&s), kind(&o));
    assert_eq!(bbox(&mut s), bbox(&mut o));
}

/// As for a fold, a new jog refuses a fixed point that touches no material of the sheet and
/// leaves the design as it was (#263). A point on the sheet decides the side that stays as the
/// jog is made: the jog keeps that side, not the point.
#[test]
fn a_jog_refuses_a_fixed_point_off_the_sheet_and_keeps_only_its_side() {
    let mut s = Session::default();
    jog_plate(&mut s);
    let before = s.doc.features.len();
    let e = jog(&mut s, json!({"offset": 20, "fixed": [100, 30, 50]})).unwrap_err().to_string();
    assert!(e.contains("not on the sheet"), "{e}");
    assert_eq!(s.doc.features.len(), before, "nothing was added");
    jog(&mut s, json!({"offset": 20})).unwrap();
    let kept = s.doc.features.last().map(|f| f.kind.clone());
    assert!(matches!(kept, Some(solidcraft_doc::FeatureKind::SheetJog { fixed: None, stays: Some(_), .. })), "{kept:?}");
}
