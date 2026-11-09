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
