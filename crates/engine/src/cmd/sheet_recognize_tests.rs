//! Recognize Sheet Metal (#6748): a body that is sheet metal already becomes the rule's sheet
//! with no picks, its own thickness and modelled bends measured exactly from its faces; a body of
//! two thicknesses, and a surface body, are no sheet to recognize.

use std::f64::consts::FRAC_PI_2;

use super::*;

/// An L 2 thick drawn as a sheet metal model draws it: outer lines 40 and 60 from the corner at
/// the origin, its corner rounded R 2 inside and R 4 outside, 100 long.
fn modelled_l(s: &mut Session) {
    bent_section(s, &[(0.0, 40.0), (0.0, 0.0), (60.0, 0.0)], 2.0, 2.0, 100.0);
}

/// The L measures 2 thick with one bend of inside radius 2. Its two sides' legs are as large
/// (36 and 56 by 100 each, between the bend's tangent lines and their ends), and its outside is
/// the larger by its bend's (π/2)·4 against (π/2)·2 by 100: the sheet's surface is the outside,
/// y = 0.
#[test]
fn a_modelled_l_measures_its_thickness_and_bend_radius() {
    let mut s = Session::default();
    modelled_l(&mut s);
    let m = run(&mut s, "sheet.measure_body", json!({"body": "Section"}));
    assert!((m["thickness_mm"].as_f64().unwrap_or(f64::NAN) - 2.0).abs() < 1e-9, "{m}");
    let radii: Vec<f64> = m["bend_radii_mm"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
    assert!(radii.len() == 1 && radii.iter().all(|r| (r - 2.0).abs() < 1e-9), "{m}");
    assert!(m["surface"][1].as_f64().is_some_and(|y| y.abs() < 1e-9), "{m}");
}

/// `new_rule` makes a rule of the body's 2 mm and R 2, its K-factor the active rule's (0.44),
/// and the sheet takes it: the body comes back as it was, its volume unchanged, and its flat is
/// the outside lengths less one deduction, 40 + 60 − (2·4·tan 45° − (π/2)(2 + 0.44·2)), 100
/// wide.
#[test]
fn a_new_rule_from_the_body_gives_the_body_back() {
    let mut s = Session::default();
    rule(&mut s, "Test", 3.0, 0.44, 5.0, 0.5);
    modelled_l(&mut s);
    let before = volume(&mut s);
    run(&mut s, "sheet.recognize", json!({"body": "Section", "new_rule": "From the L"}));
    let made = s.doc.sheet.rules.iter().find(|r| r.name == "From the L").cloned();
    assert!(made.as_ref().is_some_and(|r| r.thickness == "2 mm" && r.bend_radius == "2 mm" && r.k_factor == "0.44"), "{made:?}");
    assert!((volume(&mut s) - before).abs() < 1e-6 * before, "{} vs {before}", volume(&mut s));
    let across = 100.0 - deduction(FRAC_PI_2, 2.0, 2.0, 0.44);
    assert!(same_size(flat_size(&mut s), (across, 100.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // A rule of that name is there now; `rule` and `new_rule` together are refused.
    let mut again = Session::default();
    modelled_l(&mut again);
    rule(&mut again, "Taken", 2.0, 0.44, 2.0, 0.5);
    for bad in [json!({"body": "Section", "new_rule": "Taken"}), json!({"body": "Section", "rule": "Taken", "new_rule": "Other"})] {
        assert!(again.execute("sheet.recognize", &bad).is_err(), "{bad}");
    }
    assert_eq!(again.doc.sheet.rules.len(), 1, "a refused call makes no rule");
}

/// The rule wins over the body: a 3 mm rule of R 3 recognizes the 2 mm L as its own sheet, the
/// rule's thickness taken in from the outside, bent R 3 at the outside's virtual sharp: 40 + 60
/// less 2·6·tan 45° − (π/2)(3 + 0.44·3).
#[test]
fn a_recognized_body_takes_the_rules_thickness_and_radius() {
    let mut s = Session::default();
    rule(&mut s, "Thick", 3.0, 0.44, 3.0, 0.5);
    modelled_l(&mut s);
    run(&mut s, "sheet.recognize", json!({"body": "Section"}));
    assert!((flat(&mut s)["thickness"].as_f64().unwrap_or(0.0) - 3.0).abs() < 1e-9);
    let across = 100.0 - deduction(FRAC_PI_2, 3.0, 3.0, 0.44);
    assert!(same_size(flat_size(&mut s), (across, 100.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
}

/// An L whose leg is 2 thick and whose wall is 3 is no one sheet: Recognize refuses it, naming
/// both thicknesses. A surface body has no thickness at all: Recognize refuses it, and Convert
/// takes it.
#[test]
fn a_body_of_two_thicknesses_or_none_is_no_sheet_to_recognize() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Two"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [60, 0], [60, 2], [3, 2], [3, 30], [0, 30]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Two", "distance": 100, "body_name": "Two"}));
    for id in ["sheet.recognize", "sheet.measure_body"] {
        let e = s.execute(id, &json!({"body": "Two"})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("2.000 and 3.000 mm thick"), "{id}: {e}");
    }
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Open"}));
    let made = run(&mut s, "sketch.line", json!({"points": [[200, 40], [200, 0], [260, 0], [260, 30]]}));
    let curves = made["created"]["curves"].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Open", "curves": curves, "distance": 100, "body_name": "Open"}));
    let e = s.execute("sheet.recognize", &json!({"body": "Open"})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("not sheet metal") && e.contains("Convert takes its surfaces"), "{e}");
    assert!(s.model.state().surface_sheets.is_empty(), "nothing recognized");
    run(&mut s, "sheet.convert", json!({"body": "Open", "bends": [[200, 0, 50], [260, 0, 50]]}));
    assert_eq!(s.model.state().surface_sheets.len(), 1);
}

/// A face picked on a placed component is read where the component shows it: the command's
/// picks go into the body's own frame before it runs (`frames::to_local`), and the body is read
/// there. The L of `a_modelled_l_measures_its_thickness_and_bend_radius` in a component moved 50
/// along x: the pick at (80, 2, 50) is its long leg's inside face, y = 2 (x 54 … 110 as shown, 4 …
/// 60 in the component). Read from that face, the sheet's surface is the inside, y = 2 (unpicked,
/// the outside, y = 0), 2 thick with one bend of inside radius 2; Recognize with the same pick and
/// `new_rule` makes the rule of the body's 2 mm and R 2, and the sheet.
#[test]
fn a_face_picked_on_a_placed_component_is_read_where_it_is_shown() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    modelled_l(&mut s);
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [50, 0, 0]}));
    let m = run(&mut s, "sheet.measure_body", json!({"body": "Section", "face": [80, 2, 50]}));
    assert!((m["thickness_mm"].as_f64().unwrap_or(f64::NAN) - 2.0).abs() < 1e-9, "{m}");
    let radii: Vec<f64> = m["bend_radii_mm"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
    assert!(radii.len() == 1 && radii.iter().all(|r| (r - 2.0).abs() < 1e-9), "{m}");
    assert!(m["surface"][1].as_f64().is_some_and(|y| (y - 2.0).abs() < 1e-9), "{m}");
    run(&mut s, "sheet.recognize", json!({"body": "Section", "face": [80, 2, 50], "new_rule": "From the L"}));
    let made = s.doc.sheet.rules.iter().find(|r| r.name == "From the L").cloned();
    assert!(made.as_ref().is_some_and(|r| r.thickness == "2 mm" && r.bend_radius == "2 mm"), "{made:?}");
    assert_eq!(s.model.state().surface_sheets.len(), 1);
}

/// A list of faces given as anything but a list is refused, the design left as it was: Convert's
/// `faces`, `skip` and `engrave`, and Recognize's `skip`. A face name given alone as `faces` was
/// dropped, and with no face picked Convert took the whole body.
#[test]
fn a_list_of_faces_given_as_anything_else_is_refused() {
    let mut s = Session::default();
    modelled_l(&mut s);
    for (id, key) in [("sheet.convert", "faces"), ("sheet.convert", "skip"), ("sheet.convert", "engrave"), ("sheet.recognize", "skip")] {
        let before = s.doc.to_json();
        let mut p = json!({"body": "Section"});
        p[key] = json!("F1:top");
        let e = s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains(&format!("`{key}` must list faces")), "{id} {key}: {e}");
        assert_eq!(s.doc.to_json(), before, "{id} {key}");
        assert!(s.model.state().surface_sheets.is_empty(), "{id} {key}");
    }
}

/// A `length` x `width` channel `t` thick (R = T, K 0.5) with two inside walls `height` high on
/// its long edges, made with this build's sheet metal; with `holes` (centres, diameter `d`) cut
/// through its base in the flat and folded again. Written to STEP and inserted into a new design
/// as a plain solid, with the same rule `T` active.
fn channel_read_back(length: f64, width: f64, t: f64, height: f64, holes: &[[f64; 2]], d: f64) -> Session {
    let rule = json!({"name": "T", "thickness": format!("{t} mm"), "active": true, "bend_radius": format!("{t} mm"), "k_factor": 0.5});
    let mut a = Session::default();
    run(&mut a, "sheet.manage_rules", rule.clone());
    run(&mut a, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut a, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [length, width]}));
    run(&mut a, "sketch.finish", json!({}));
    run(&mut a, "sheet.flange", json!({"type": "base", "sketch": "Base"}));
    let edges = json!([[length / 2.0, 0, t], [length / 2.0, width, t]]);
    run(&mut a, "sheet.flange", json!({"type": "edge", "height": height, "angle": 90, "position": "inside", "edges": edges}));
    if !holes.is_empty() {
        run(&mut a, "sheet.unfold", json!({}));
        run(&mut a, "sketch.create", json!({"plane": "XY", "name": "Holes"}));
        for c in holes {
            run(&mut a, "sketch.circle.center", json!({"center": c, "diameter": d}));
        }
        run(&mut a, "sketch.finish", json!({}));
        run(&mut a, "solid.extrude", json!({"sketch": "Holes", "through_all": true, "direction": "symmetric", "operation": "cut"}));
        run(&mut a, "sheet.refold", json!({}));
    }
    let path = std::env::temp_dir().join(format!("solidcraft-channel-read-back-{}-{}.step", holes.len(), std::process::id()));
    run(&mut a, "file.export", json!({"path": path.to_string_lossy()}));
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_file(&path);
    run(&mut s, "sheet.manage_rules", rule);
    s
}

/// `a`'s difference from `b`, relative to `b`.
fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// Recognizes the first body under rule `T`, with no face and no bends picked.
fn recognize_by_itself(s: &mut Session) {
    let body = s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    run(s, "sheet.recognize", json!({"body": body, "rule": "T"}));
}

/// The bends' lengths, longest first.
fn bend_lengths(s: &mut Session) -> Vec<f64> {
    let mut bends: Vec<f64> = flat(s)["bends"].as_array().into_iter().flatten().map(|b| b["length"].as_f64().unwrap_or(f64::NAN)).collect();
    bends.sort_by(|x, y| y.total_cmp(x));
    bends
}

/// #7582 (#7531): a 120 x 90 channel with two 30 mm inside walls (T 1, R 1, K 0.5), written to
/// STEP, inserted as a plain solid and recognized with only the body and the rule: both
/// modelled bends are found by themselves, each 120 long. Each bend starts R + T = 2 in from the
/// base's edge, BA = (π/2)(R + K T) = 0.75π, each wall's straight leg 30 − (R + T) = 28. Flat:
/// 120 x (90 − 4) of base, 2 x 120 x BA of bend, 2 x 120 x 28 of wall = 17605.486678 mm²,
/// 120 by 86 + 2(BA + 28) across. With K = 0.5 the folded volume is the flat area times T.
#[test]
fn a_channel_read_back_from_step_finds_its_bends_by_itself() {
    let (t, r) = (1.0, 1.0);
    let (ba, leg) = (FRAC_PI_2 * (r + 0.5 * t), 30.0 - (r + t));
    let area = 120.0 * (90.0 - 2.0 * (r + t)) + 2.0 * 120.0 * (ba + leg);
    let mut s = channel_read_back(120.0, 90.0, t, 30.0, &[], 0.0);
    recognize_by_itself(&mut s);
    assert!(rel(volume(&mut s), area * t) < 1e-9, "{} vs {}", volume(&mut s), area * t);
    let across = 90.0 - 2.0 * (r + t) + 2.0 * (ba + leg);
    assert!(same_size(flat_size(&mut s), (120.0, across), 1e-6), "{:?} vs 120 x {across}", flat_size(&mut s));
    let (got, _) = flat_area_and_bends(&mut s);
    assert!(rel(got, area) < 1e-9, "{got} vs {area}");
    let bends = bend_lengths(&mut s);
    assert!(bends.len() == 2 && bends.iter().all(|b| (b - 120.0).abs() < 1e-6), "{bends:?}");
}

/// #8512 (#7531): a 300 x 200 channel 1.5 thick with two 40 mm inside walls (R 1.5, K 0.5) and
/// 459 holes of 5 mm cut through its base in the flat, a 27 x 17 grid at a 10 mm pitch from
/// (20, 20), folded again, written to STEP and recognized with its bends found by themselves: a
/// body of many faces. Flat: the base 300 x (200 − 2(R + T)), the bends 2 x 300 x BA with
/// BA = (π/2)(R + K T) = 1.125π, the walls 2 x 300 x (40 − (R + T)), less 459 x π 2.5²:
/// 73508.131116 mm², 300 by 194 + 2(BA + 37) across. With K = 0.5 the folded volume is the flat
/// area times T. Both bends, 300 long, and every hole as a circle of R 2.5 come back.
#[test]
fn a_perforated_channel_read_back_from_step_finds_its_bends_and_holes() {
    let (t, r) = (1.5, 1.5);
    let (ba, leg) = (FRAC_PI_2 * (r + 0.5 * t), 40.0 - (r + t));
    let area = 300.0 * (200.0 - 2.0 * (r + t)) + 2.0 * 300.0 * (ba + leg) - 459.0 * std::f64::consts::PI * 2.5 * 2.5;
    let holes: Vec<[f64; 2]> = (0..27).flat_map(|i| (0..17).map(move |j| [20.0 + 10.0 * f64::from(i), 20.0 + 10.0 * f64::from(j)])).collect();
    let mut s = channel_read_back(300.0, 200.0, t, 40.0, &holes, 5.0);
    recognize_by_itself(&mut s);
    assert!(rel(volume(&mut s), area * t) < 1e-6, "{} vs {}", volume(&mut s), area * t);
    let across = 200.0 - 2.0 * (r + t) + 2.0 * (ba + leg);
    assert!(same_size(flat_size(&mut s), (300.0, across), 1e-6), "{:?} vs 300 x {across}", flat_size(&mut s));
    let bends = bend_lengths(&mut s);
    assert!(bends.len() == 2 && bends.iter().all(|b| (b - 300.0).abs() < 1e-6), "{bends:?}");
    let shapes = flat(&mut s)["cutout_shapes"].as_array().cloned().unwrap_or_default();
    let round = shapes.iter().filter(|c| c["circle"]["radius"].as_f64().is_some_and(|d| (d - 2.5).abs() < 1e-6)).count();
    assert!(shapes.len() == 459 && round == 459, "{} cut-outs, {round} of them circles of R 2.5", shapes.len());
}

/// #9682 (#9644): an L section 60 x 40 x 100, 2 thick, its bend modelled (inside R 2, outside
/// R 4), with a slot cut through the bend between 200° and 220° round its axis, 20 long, and
/// recognized under a rule of T 2, R 2, K 0.44: the slot is a cut-out of the bend zone, not lost.
/// The rule's radius is the modelled one, so the solid is kept: (56·2 + 36·2 + (π/4)(4² − 2²))
/// x 100 = 19342.477796, less the slot, a 20° wedge of the bend between r 2 and 4, 20 long:
/// 20·π(4² − 2²)·20/360 = 41.887902. Flat: BA = (π/2)(R + K T) = 1.44π, the legs 60 − 4 and
/// 40 − 4, so 100 x (56 + 36 + BA) less the slot's 20/90 of the zone across, 20 long:
/// 20·BA·20/90 = 20.106193, 9632.283149 mm², 56 + 36 + BA by 100. Unfolded, the sheet is that
/// flat drawn T deep.
#[test]
fn a_slot_through_a_modelled_bend_stays_a_cut_out_when_the_body_is_recognized() {
    let (t, ba) = (2.0, FRAC_PI_2 * (2.0 + 0.44 * 2.0));
    let pi = std::f64::consts::PI;
    let solid = (56.0 * 2.0 + 36.0 * 2.0 + pi / 4.0 * 12.0) * 100.0 - 20.0 * pi * 12.0 * 20.0 / 360.0;
    let area = 100.0 * (56.0 + 36.0 + ba) - 20.0 * ba * 20.0 / 90.0;
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "Rule", "thickness": "2.0 mm", "active": true, "bend_radius": "2.0 mm", "k_factor": 0.44}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "L"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 40], [0, 4]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [4, 4], "start": [0, 4], "end": [4, 0]}));
    run(&mut s, "sketch.line", json!({"points": [[4, 0], [60, 0], [60, 2], [4, 2]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [4, 4], "start": [2, 4], "end": [4, 2]}));
    run(&mut s, "sketch.line", json!({"points": [[2, 4], [2, 40], [0, 40]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "L", "distance": 100, "body_name": "Angle"}));
    // The wedge from the bend's axis (4, 4) out past r 8, at 200° and 220°.
    let at = |deg: f64| [4.0 + 8.0 * deg.to_radians().cos(), 4.0 + 8.0 * deg.to_radians().sin()];
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 40], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Slot"}));
    run(&mut s, "sketch.line", json!({"points": [[4, 4], at(200.0), at(220.0)], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Slot", "distance": 20, "operation": "cut"}));
    run(&mut s, "sheet.recognize", json!({"body": "Angle", "rule": "Rule"}));
    assert!(rel(volume(&mut s), solid) < 1e-6, "{} vs {solid}", volume(&mut s));
    assert!(same_size(flat_size(&mut s), (56.0 + 36.0 + ba, 100.0), 1e-6), "{:?} vs {} x 100", flat_size(&mut s), 56.0 + 36.0 + ba);
    let shapes = flat(&mut s)["cutout_shapes"].as_array().cloned().unwrap_or_default();
    assert_eq!(shapes.len(), 1, "{shapes:?}");
    run(&mut s, "sheet.unfold", json!({}));
    assert!(rel(volume(&mut s) / t, area) < 1e-6, "flat {} vs {area}", volume(&mut s) / t);
}
