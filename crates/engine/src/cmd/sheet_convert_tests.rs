//! Convert to Sheet Metal: solid and shelled bodies with bend edges (with holes and round edges in
//! their faces), plates read from STEP files whose walls come in pieces, and plates with recesses.

use std::f64::consts::{FRAC_PI_2, PI, SQRT_2};

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

fn bbox(s: &mut Session) -> ([f64; 3], [f64; 3]) {
    let m = run(s, "inspect.measure", json!({}));
    let b = &m["bodies"][0]["bbox"];
    let v = |k: &str| [0, 1, 2].map(|i| b[k][i].as_f64().unwrap_or(f64::NAN));
    (v("min"), v("max"))
}

fn flat(s: &mut Session) -> Value {
    run(s, "sheet.flat_pattern", json!({}))
}

fn flat_size(s: &mut Session) -> (f64, f64) {
    let f = flat(s);
    let v = |i: usize| f["flat_size_mm"][i].as_f64().unwrap_or(f64::NAN);
    (v(0), v(1))
}

/// Either way round (the flat frame's x runs along the fixed face's longest edge).
fn same_size(got: (f64, f64), want: (f64, f64), tol: f64) -> bool {
    ((got.0 - want.0).abs() < tol && (got.1 - want.1).abs() < tol) || ((got.0 - want.1).abs() < tol && (got.1 - want.0).abs() < tol)
}

/// The bend table's rows as (angle in whole degrees, direction, length).
fn bend_rows(s: &mut Session) -> Vec<(i64, String, f64)> {
    let rows = flat(s)["bends"].as_array().cloned().unwrap_or_default();
    let row = |b: &Value| {
        (
            b["angle_deg"].as_f64().unwrap_or(0.0).round() as i64,
            b["direction"].as_str().unwrap_or("").to_string(),
            b["length"].as_f64().unwrap_or(0.0),
        )
    };
    rows.iter().map(row).collect()
}

fn rule(s: &mut Session, name: &str, t: f64, k: f64, r: f64, gap: f64) {
    let mm = |v: f64| format!("{v} mm");
    run(
        s,
        "sheet.manage_rules",
        json!({"name": name, "thickness": mm(t), "k_factor": format!("{k}"), "bend_radius": mm(r), "gap": mm(gap), "active": true}),
    );
}

/// Bend deduction: twice the outside setback less the allowance.
fn deduction(theta: f64, r: f64, t: f64, k: f64) -> f64 {
    2.0 * (r + t) * (theta / 2.0).tan() - theta * (r + k * t)
}

const BOTTOM_EDGES: [[f64; 3]; 4] = [[50.0, 0.0, 0.0], [100.0, 30.0, 0.0], [50.0, 60.0, 0.0], [0.0, 30.0, 0.0]];

/// A 100 x 60 x 30 box shelled to `t` with its top open.
fn tray(s: &mut Session, t: f64) {
    run(s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Tray"}));
    run(s, "solid.shell", json!({"faces": [[50, 30, 30]], "thickness": t}));
}

/// The folded tray's volume: base, four 90° bends and four walls cut back at the corners.
fn tray_volume(l: f64, w: f64, h: f64, t: f64, r: f64, gap: f64) -> f64 {
    let s = r + t;
    let cut = t + gap / SQRT_2;
    let base = (l - 2.0 * s) * (w - 2.0 * s);
    let bends = FRAC_PI_2 * (r + t / 2.0) * 2.0 * ((l - 2.0 * s) + (w - 2.0 * s));
    let walls = (h - s) * 2.0 * ((l - 2.0 * cut) + (w - 2.0 * cut));
    t * (base + bends + walls)
}

/// A strip `t` thick along the polyline `outer` (its material to the left of the way the line
/// runs, its ends cut square), extruded `len` along z as the body `Section`. Returns a point on
/// the edge at each corner of the polyline.
fn section(s: &mut Session, outer: &[(f64, f64)], t: f64, len: f64) -> Vec<[f64; 3]> {
    let n = outer.len();
    let dir = |i: usize| {
        let (a, b) = (outer[i], outer[i + 1]);
        let l = (b.0 - a.0).hypot(b.1 - a.1);
        ((b.0 - a.0) / l, (b.1 - a.1) / l)
    };
    let shift = |p: (f64, f64), d: (f64, f64)| (p.0 - d.1 * t, p.1 + d.0 * t);
    let mut inner = vec![shift(outer[0], dir(0))];
    for (i, &corner) in outer.iter().enumerate().take(n - 1).skip(1) {
        let (d0, d1) = (dir(i - 1), dir(i));
        let (p0, p1) = (shift(corner, d0), shift(corner, d1));
        let along = ((p1.0 - p0.0) * d1.1 - (p1.1 - p0.1) * d1.0) / (d0.0 * d1.1 - d0.1 * d1.0);
        inner.push((p0.0 + d0.0 * along, p0.1 + d0.1 * along));
    }
    inner.push(shift(outer[n - 1], dir(n - 2)));
    let pts: Vec<[f64; 2]> = outer.iter().chain(inner.iter().rev()).map(|p| [p.0, p.1]).collect();
    run(s, "sketch.create", json!({"plane": "XY", "name": "Section"}));
    run(s, "sketch.line", json!({"points": pts, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Section", "distance": len, "body_name": "Section"}));
    outer[1..n - 1].iter().map(|p| [p.0, p.1, len / 2.0]).collect()
}

/// A base 200 wide with walls 40 long at 72° and returns 15 long at 108° more, 300 long.
fn trapezoid_section(s: &mut Session, t: f64) -> Vec<[f64; 3]> {
    let (c, sn) = (72f64.to_radians().cos(), 72f64.to_radians().sin());
    let (x, y) = (100.0 + 40.0 * c, 40.0 * sn);
    section(s, &[(-x + 15.0, y), (-x, y), (-100.0, 0.0), (100.0, 0.0), (x, y), (x - 15.0, y)], t, 300.0)
}

#[test]
fn rotated_component_bend_picks_follow_connected_panels() {
    let mut sizes = Vec::new();
    for rotated in [false, true] {
        let mut s = Session::default();
        let component = run(&mut s, "component.create", json!({"name": "Bent part", "grounded": false}));
        rule(&mut s, "Test", 2.0, 0.5, 1.0, 0.5);
        let corners = bent_section(&mut s, &[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (90.0, 40.0)], 2.0, 1.0, 100.0);
        // The remote bend is picked on its tangent edge, where it meets the web.
        let bends = [corners[0].0, [60.0, 39.0, 50.0]];
        if rotated {
            run(&mut s, "occurrence.move", json!({"occurrence": component["occurrence"], "axis": [1, 0, 0], "angle": "90 deg"}));
        }
        run(&mut s, "component.activate", json!({"component": 0}));
        let world = |p: [f64; 3]| if rotated { [p[0], -p[2], p[1]] } else { p };
        let bends: Vec<_> = bends.into_iter().map(world).collect();
        run(&mut s, "sheet.convert", json!({"body": "Section", "face": world([30.0, 0.0, 50.0]), "bends": bends}));
        assert_eq!(bend_rows(&mut s).len(), 2);
        sizes.push(flat_size(&mut s));
    }
    assert!(same_size(sizes[0], sizes[1], 1e-6), "{sizes:?}");
}

#[test]
fn rotated_component_disconnected_bend_reports_world_point() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name": "Bent part", "grounded": false}));
    rule(&mut s, "Test", 2.0, 0.5, 1.0, 0.5);
    section(&mut s, &[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (90.0, 40.0)], 2.0, 100.0);
    run(&mut s, "occurrence.move", json!({"occurrence": component["occurrence"], "axis": [1, 0, 0], "angle": "90 deg"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let e = s.execute("sheet.convert", &json!({"body": "Section", "face": [30, -50, 0], "bends": [[60, -50, 40]]})).unwrap_err().to_string();
    assert!(e.contains("the bend at (60.000, -50.000, 40.000) does not connect"), "{e}");
}

#[test]
fn a_shelled_tray_keeps_its_walls_and_unfolds_by_the_k_factor() {
    let (t, k, r, gap) = (2.0, 0.44, 2.0, 0.5);
    let mut s = Session::default();
    // The shell's own thickness counts, not the rule's.
    rule(&mut s, "Test", 3.0, k, r, gap);
    tray(&mut s, t);
    let out = run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES}));
    assert!(out["warning"].is_null(), "{out}");
    assert!((flat(&mut s)["thickness"].as_f64().unwrap_or(0.0) - t).abs() < 1e-9);
    // Four 90° bends up, each as long as the base is between its setbacks (R + T at either end).
    let mut rows = bend_rows(&mut s);
    rows.sort_by(|a, b| a.2.total_cmp(&b.2));
    let want = [52.0, 52.0, 92.0, 92.0];
    assert!(rows.len() == 4 && rows.iter().zip(want).all(|(row, l)| row.0 == 90 && row.1 == "up" && (row.2 - l).abs() < 1e-6), "{rows:?}");
    let allowance = flat(&mut s)["bends"][0]["allowance"].as_f64().unwrap_or(0.0);
    assert!((allowance - FRAC_PI_2 * (r + k * t)).abs() < 1e-9, "{allowance}");
    // The envelope stays: the sharp edges became bends inside it.
    let (lo, hi) = bbox(&mut s);
    assert!(
        lo.iter().all(|v| v.abs() < 1e-6) && (hi[0] - 100.0).abs() < 1e-6 && (hi[1] - 60.0).abs() < 1e-6 && (hi[2] - 30.0).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // Flat: the outside lengths less two bend deductions each way.
    let bd = deduction(FRAC_PI_2, r, t, k);
    let want = (100.0 + 60.0 - 2.0 * bd, 60.0 + 60.0 - 2.0 * bd);
    assert!(same_size(flat_size(&mut s), want, 1e-6), "{:?} vs {want:?}", flat_size(&mut s));
    let v = volume(&mut s);
    let expect = tray_volume(100.0, 60.0, 30.0, t, r, gap);
    assert!((v - expect).abs() < 2e-3 * expect, "{v} vs {expect}");
    // Unfold: flat, one sheet thick, the same outline; refold gives the tray back.
    run(&mut s, "sheet.unfold", json!({}));
    let (lo, hi) = bbox(&mut s);
    assert!((hi[2] - lo[2] - t).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!(same_size((hi[0] - lo[0], hi[1] - lo[1]), want, 1e-6), "{lo:?} {hi:?}");
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - v).abs() < 1e-6 * v);
    // It exports like a sheet made from flanges: an outline and four bend lines up.
    let dxf = run(&mut s, "sheet.export_dxf", json!({}));
    assert_eq!(dxf["bends"], 4);
    let lines: Vec<&str> = dxf["dxf"].as_str().unwrap_or_default().lines().collect();
    assert_eq!(lines.windows(2).filter(|w| w[0].trim() == "8" && w[1].trim() == "BEND_UP").count(), 4, "four bend lines");
    assert_eq!(run(&mut s, "sheet.export_bend_table", json!({"format": "json"}))["bends"], 4);
}

#[test]
fn bends_picked_inside_and_a_fixed_face_inside_give_the_same_sheet() {
    let (t, k, r, gap) = (2.0, 0.44, 2.0, 0.5);
    let inner = [[50.0, 2.0, 2.0], [98.0, 30.0, 2.0], [50.0, 58.0, 2.0], [2.0, 30.0, 2.0]];
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, gap);
    tray(&mut s, t);
    run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES}));
    let (v, f) = (volume(&mut s), flat_size(&mut s));
    // The inner edges (2 mm in and up) are the same bends.
    let mut a = Session::default();
    rule(&mut a, "Test", t, k, r, gap);
    tray(&mut a, t);
    run(&mut a, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 0], "bends": inner}));
    assert!((volume(&mut a) - v).abs() < 1e-6 * v && same_size(flat_size(&mut a), f, 1e-9));
    // From the floor inside, the sheet's other skin is the fixed one: the bends turn down from it
    // and the flat is the inside lengths less the inside setbacks, L' + 2H' - 4R + 2BA, which is
    // the same blank.
    let mut b = Session::default();
    rule(&mut b, "Test", t, k, r, gap);
    tray(&mut b, t);
    run(&mut b, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 2], "bends": BOTTOM_EDGES}));
    assert!(bend_rows(&mut b).iter().all(|row| row.0 == 90 && row.1 == "down"), "{:?}", bend_rows(&mut b));
    let ba = FRAC_PI_2 * (r + k * t);
    let inside = (96.0 + 2.0 * 28.0 - 4.0 * r + 2.0 * ba, 56.0 + 2.0 * 28.0 - 4.0 * r + 2.0 * ba);
    assert!(same_size(flat_size(&mut b), inside, 1e-6) && same_size(inside, f, 1e-6), "{:?} {inside:?} {f:?}", flat_size(&mut b));
    assert!((volume(&mut b) - v).abs() < 1e-6 * v, "{} vs {v}", volume(&mut b));
    let (lo, hi) = bbox(&mut b);
    assert!(lo.iter().all(|x| x.abs() < 1e-6) && (hi[0] - 100.0).abs() < 1e-6 && (hi[2] - 30.0).abs() < 1e-6, "{lo:?} {hi:?}");
}

#[test]
fn a_solid_block_takes_its_rules_thickness_inward() {
    let (k, r, gap) = (0.44, 2.0, 0.5);
    for (name, t) in [(None, 2.0), (Some("Thick"), 3.0)] {
        // The active rule, or the one the conversion names.
        let mut s = Session::default();
        rule(&mut s, "Thick", 3.0, k, r, gap);
        rule(&mut s, "Test", 2.0, k, r, gap);
        run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Block"}));
        let mut p = json!({"body": "Block", "face": [50, 30, 0], "bends": BOTTOM_EDGES});
        if let Some(n) = name {
            p["rule"] = json!(n);
        }
        run(&mut s, "sheet.convert", p);
        assert!((flat(&mut s)["thickness"].as_f64().unwrap_or(0.0) - t).abs() < 1e-9);
        // The block's faces are the sheet's outside; its top is left out.
        let (lo, hi) = bbox(&mut s);
        assert!(
            lo.iter().all(|v| v.abs() < 1e-6) && (hi[0] - 100.0).abs() < 1e-6 && (hi[1] - 60.0).abs() < 1e-6 && (hi[2] - 30.0).abs() < 1e-6,
            "{lo:?} {hi:?}"
        );
        let bd = deduction(FRAC_PI_2, r, t, k);
        assert!(same_size(flat_size(&mut s), (160.0 - 2.0 * bd, 120.0 - 2.0 * bd), 1e-6), "{:?}", flat_size(&mut s));
        let expect = tray_volume(100.0, 60.0, 30.0, t, r, gap);
        assert!((volume(&mut s) - expect).abs() < 2e-3 * expect, "{} vs {expect}", volume(&mut s));
    }
}

#[test]
fn a_trapezoid_section_unfolds_through_72_and_108_degree_bends() {
    let (t, k, r) = (2.0, 0.5, 1.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    let bends = trapezoid_section(&mut s, t);
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [0, 0, 150], "bends": bends}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 4 && rows.iter().all(|row| row.1 == "up" && (row.2 - 300.0).abs() < 1e-9), "{rows:?}");
    assert_eq!((rows.iter().filter(|row| row.0 == 72).count(), rows.iter().filter(|row| row.0 == 108).count()), (2, 2), "{rows:?}");
    let (a72, a108) = (72f64.to_radians(), 108f64.to_radians());
    let across = 200.0 + 2.0 * 40.0 + 2.0 * 15.0 - 2.0 * deduction(a72, r, t, k) - 2.0 * deduction(a108, r, t, k);
    assert!(same_size(flat_size(&mut s), (across, 300.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // The section keeps its outside faces: the returns' tops stay 40 sin 72° up and the length
    // stays; the widest point is on the round of the 108° bend (outer radius R + T), whose
    // centre lies (R + T) / sin 36° from the sharp corner along the corner's bisector.
    let (lo, hi) = bbox(&mut s);
    let (c72, s72) = (a72.cos(), a72.sin());
    let rho = r + t;
    let bisector_x = (-c72 - 1.0) / (-c72 - 1.0).hypot(-s72);
    let wide = 2.0 * (100.0 + 40.0 * c72 + bisector_x * rho / 36f64.to_radians().sin() + rho);
    // (The box is measured on the tessellation: the round comes out a chord's sag inside.)
    assert!((hi[0] - lo[0] - wide).abs() < 2e-3 && (hi[1] - 40.0 * s72).abs() < 1e-6 && (hi[2] - lo[2] - 300.0).abs() < 1e-6, "{lo:?} {hi:?} {wide}");
    run(&mut s, "sheet.unfold", json!({}));
    let (lo, hi) = bbox(&mut s);
    let flat_box = [(hi[0] - lo[0], hi[1] - lo[1]), (hi[0] - lo[0], hi[2] - lo[2])];
    assert!(flat_box.iter().any(|b| same_size(*b, (across, 300.0), 1e-6)), "{lo:?} {hi:?}");
}

#[test]
fn a_z_section_bends_up_at_one_edge_and_down_at_the_other() {
    let (t, k, r) = (2.0, 0.5, 1.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    // Flange 60, web 40, flange 30 on one skin: its first corner is the outside of its bend,
    // its second the inside.
    let bends = section(&mut s, &[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (90.0, 40.0)], t, 100.0);
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [30, 0, 50], "bends": bends}));
    let mut directions: Vec<String> = bend_rows(&mut s).into_iter().map(|row| row.1).collect();
    directions.sort();
    assert_eq!(directions, ["down", "up"]);
    // Outside setbacks R + T at the first bend, inside setbacks R at the second, and a 90°
    // allowance for each: 60 + 40 + 30 - 2 (R + T) - 2 R + π (R + K T) = 128.2832.
    let across = 130.0 - 2.0 * (r + t) - 2.0 * r + PI * (r + k * t);
    assert!((across - 128.283185).abs() < 1e-6);
    assert!(same_size(flat_size(&mut s), (across, 100.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    let (lo, hi) = bbox(&mut s);
    assert!(
        lo.iter().all(|v| v.abs() < 1e-6) && (hi[0] - 90.0).abs() < 1e-6 && (hi[1] - 42.0).abs() < 1e-6 && (hi[2] - 100.0).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // Three flat strips and two quarter rounds on the neutral radius R + T/2.
    let expect = t * 100.0 * ((60.0 - (r + t)) + (40.0 - (r + t) - r) + (30.0 - r) + PI * (r + t / 2.0));
    assert!((volume(&mut s) - expect).abs() < 2e-3 * expect, "{} vs {expect}", volume(&mut s));
}

#[test]
fn a_tapered_tray_converts_with_72_degree_walls_and_slanted_corners() {
    let (t, k, r, gap) = (2.0, 0.5, 1.0, 0.5);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, gap);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Floor"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [200, 120]}));
    run(&mut s, "sketch.finish", json!({}));
    // Walls leaning out 18°: 72° bends from the floor.
    run(&mut s, "solid.extrude", json!({"sketch": "Floor", "distance": 40, "taper": "18 deg", "body_name": "Tray"}));
    let (lo, hi) = bbox(&mut s);
    assert!(hi[0] - lo[0] > 200.0 + 1.0, "the taper widens the top: {lo:?} {hi:?}");
    run(&mut s, "solid.shell", json!({"faces": [[100, 60, 40]], "thickness": t}));
    run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [100, 60, 0], "bends": [[100, 0, 0], [200, 60, 0], [100, 120, 0], [0, 60, 0]]}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 4 && rows.iter().all(|row| row.0 == 72 && row.1 == "up"), "{rows:?}");
    let slant = 40.0 / 72f64.to_radians().sin();
    let bd = deduction(72f64.to_radians(), r, t, k);
    assert!(same_size(flat_size(&mut s), (200.0 + 2.0 * slant - 2.0 * bd, 120.0 + 2.0 * slant - 2.0 * bd), 1e-6), "{:?}", flat_size(&mut s));
    // The outside stays; the rim is cut square to the sheet, as a blank is, so the inside edge
    // of a wall leaning out 18° stands T sin 18° above the solid's level rim.
    let (lo2, hi2) = bbox(&mut s);
    assert!((0..2).all(|i| (lo2[i] - lo[i]).abs() < 1e-6 && (hi2[i] - hi[i]).abs() < 1e-6) && lo2[2].abs() < 1e-6, "{lo2:?} {hi2:?}");
    assert!((hi2[2] - 40.0 - t * 18f64.to_radians().sin()).abs() < 1e-6, "{hi2:?}");
    let v = volume(&mut s);
    run(&mut s, "sheet.unfold", json!({}));
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - v).abs() < 1e-6 * v);
}

#[test]
fn the_sheet_follows_the_solid_through_parameter_changes_undo_and_reopening() {
    let (t, k, r) = (2.0, 0.44, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "100 mm"}));
    run(&mut s, "parameters.add", json!({"name": "T", "expression": "2 mm"}));
    run(&mut s, "solid.box", json!({"length": "L", "width": 60, "height": 30, "body_name": "Tray"}));
    run(&mut s, "solid.shell", json!({"faces": [[50, 30, 30]], "thickness": "T"}));
    run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES}));
    let bd = |t: f64| deduction(FRAC_PI_2, r, t, k);
    assert!(same_size(flat_size(&mut s), (160.0 - 2.0 * bd(t), 120.0 - 2.0 * bd(t)), 1e-6));
    // Longer: the right-hand bend edge moved from x = 100 to x = 140 and is found by its name.
    run(&mut s, "parameters.change", json!({"name": "L", "expression": "140 mm"}));
    assert!(s.model.results.iter().all(|x| x.error.is_none()), "{:?}", s.model.results.iter().map(|x| (&x.name, &x.error)).collect::<Vec<_>>());
    assert!(same_size(flat_size(&mut s), (200.0 - 2.0 * bd(t), 120.0 - 2.0 * bd(t)), 1e-6), "{:?}", flat_size(&mut s));
    let (_, hi) = bbox(&mut s);
    assert!((hi[0] - 140.0).abs() < 1e-6, "{hi:?}");
    // Thicker walls: the sheet takes the shell's new thickness.
    run(&mut s, "parameters.change", json!({"name": "T", "expression": "3 mm"}));
    assert!(s.model.results.iter().all(|x| x.error.is_none()));
    assert!((flat(&mut s)["thickness"].as_f64().unwrap_or(0.0) - 3.0).abs() < 1e-9);
    let thick = (200.0 - 2.0 * bd(3.0), 120.0 - 2.0 * bd(3.0));
    assert!(same_size(flat_size(&mut s), thick, 1e-6), "{:?}", flat_size(&mut s));
    run(&mut s, "edit.undo", json!({}));
    assert!(same_size(flat_size(&mut s), (200.0 - 2.0 * bd(t), 120.0 - 2.0 * bd(t)), 1e-6));
    run(&mut s, "edit.redo", json!({}));
    assert!(same_size(flat_size(&mut s), thick, 1e-6));
    // The design file keeps the bend edges, and reopens to the same sheet.
    let saved = s.doc.to_json();
    assert!(saved.contains("\"bends\""), "{saved}");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(same_size(flat_size(&mut reopened), thick, 1e-6));
    // Bend edges came with format 17: a build from before them asks for an update.
    let version = serde_json::from_str::<Value>(&saved).unwrap()["format"].as_str().and_then(solidcraft_doc::format::version_of);
    assert!(version >= Some(17), "{version:?}");
}

/// Designs from before bend edges open unchanged: a plate conversion saves without `bends`, as
/// it always did, and a format 16 file with one is the same document and the same sheet.
#[test]
fn designs_from_before_bend_edges_open_unchanged() {
    let mut s = Session::default();
    rule(&mut s, "Test", 2.0, 0.44, 2.0, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 2, "body_name": "Plate"}));
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [50, 30, 2]}));
    let mut older: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    assert!(!older.to_string().contains("\"bends\""), "{older}");
    older["format"] = json!("solidcraft/16");
    let doc = solidcraft_doc::Document::from_json(&older.to_string()).unwrap();
    // An older design keeps the numbering of the pieces it was saved with (#3902).
    let mut same = solidcraft_doc::Document::clone(&s.doc);
    same.features.iter_mut().for_each(|f| f.kernel_piece_order = true);
    assert_eq!(doc, same);
    let mut reopened = Session::new(doc);
    assert!(reopened.model.results.iter().all(|x| x.error.is_none()));
    assert!(same_size(flat_size(&mut reopened), (100.0, 60.0), 1e-9), "{:?}", flat_size(&mut reopened));
    assert!((volume(&mut reopened) - 100.0 * 60.0 * 2.0).abs() < 1e-6);
    assert_eq!(flat(&mut reopened), flat(&mut s));
}

#[test]
fn bend_edges_changed_in_an_edit_are_the_ones_bent() {
    let (t, k, r) = (2.0, 0.44, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    tray(&mut s, t);
    let made = run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES}));
    // Only the two long walls: a channel as long as the tray, 60 + 2 x 30 less two deductions
    // across. (By the names of the first two edges it would be the front and the right wall.)
    run(&mut s, "timeline.edit", json!({"feature": made["feature"], "set": {"bends": [BOTTOM_EDGES[0], BOTTOM_EDGES[2]]}}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 2 && rows.iter().all(|row| row.0 == 90 && row.1 == "up" && (row.2 - 100.0).abs() < 1e-6), "{rows:?}");
    let across = 120.0 - 2.0 * deduction(FRAC_PI_2, r, t, k);
    assert!(same_size(flat_size(&mut s), (100.0, across), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // Its end walls are no faces of the sheet any more: the channel is open at both ends.
    let expect = t * 100.0 * ((60.0 - 2.0 * (r + t)) + 2.0 * (30.0 - (r + t)) + PI * (r + t / 2.0));
    assert!((volume(&mut s) - expect).abs() < 2e-3 * expect, "{} vs {expect}", volume(&mut s));
}

/// A channel 40 wide with walls 30 high (outside sizes), 2 thick and 100 long, its two corners
/// drawn as chamfers, as a model without bend radii draws a bend: 4 along each outer face and 2
/// along each inner one, the chords of an R 2 bend's outside (R + T) and inside (R).
fn chamfered_channel(s: &mut Session) {
    let pts = [[0, 30], [0, 4], [4, 0], [36, 0], [40, 4], [40, 30], [38, 30], [38, 4], [36, 2], [4, 2], [2, 4], [2, 30]];
    run(s, "sketch.create", json!({"plane": "XY", "name": "Channel"}));
    run(s, "sketch.line", json!({"points": pts, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Channel", "distance": 100, "body_name": "Channel"}));
}

#[test]
fn a_chamfer_picked_in_its_middle_is_one_bend_at_the_virtual_sharp() {
    // The rule's radius bends, not the one the chamfers stand for.
    let (t, k, r) = (2.0, 0.5, 3.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    chamfered_channel(&mut s);
    // The middle of each outer chamfer.
    run(&mut s, "sheet.convert", json!({"body": "Channel", "face": [20, 0, 50], "bends": [[2, 2, 50], [38, 2, 50]]}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 2 && rows.iter().all(|row| row.0 == 90 && row.1 == "up" && (row.2 - 100.0).abs() < 1e-6), "{rows:?}");
    // The outside lengths to the virtual sharps less a 90° deduction each:
    // 30 + 40 + 30 - 2 (2 (R + T) - π/2 (R + K T)) = 100 - 2 (10 - 2π) = 92.5664.
    let across = 100.0 - 2.0 * deduction(FRAC_PI_2, r, t, k);
    assert!((across - 92.566371).abs() < 1e-6);
    assert!(same_size(flat_size(&mut s), (across, 100.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // The envelope of the sharp corners stays; the chamfers are gone into the bends.
    let (lo, hi) = bbox(&mut s);
    assert!(
        lo.iter().all(|v| v.abs() < 1e-6) && (hi[0] - 40.0).abs() < 1e-6 && (hi[1] - 30.0).abs() < 1e-6 && (hi[2] - 100.0).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // Three flat strips and two quarter rounds on the neutral radius R + T/2.
    let expect = t * 100.0 * (2.0 * (30.0 - (r + t)) + (40.0 - 2.0 * (r + t)) + PI * (r + t / 2.0));
    assert!((volume(&mut s) - expect).abs() < 2e-3 * expect, "{} vs {expect}", volume(&mut s));
    // The design keeps the chamfers' points and reopens to the same sheet.
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(same_size(flat_size(&mut reopened), (across, 100.0), 1e-6), "{:?}", flat_size(&mut reopened));

    // Picked on the inner skin, from the inside of the base: the same sheet, bent down.
    let mut b = Session::default();
    rule(&mut b, "Test", t, k, r, 0.5);
    chamfered_channel(&mut b);
    run(&mut b, "sheet.convert", json!({"body": "Channel", "face": [20, 2, 50], "bends": [[3, 3, 50], [37, 3, 50]]}));
    assert!(bend_rows(&mut b).iter().all(|row| row.0 == 90 && row.1 == "down"), "{:?}", bend_rows(&mut b));
    assert!(same_size(flat_size(&mut b), (across, 100.0), 1e-6), "{:?}", flat_size(&mut b));
    assert!((volume(&mut b) - expect).abs() < 2e-3 * expect, "{} vs {expect}", volume(&mut b));

    // Points on a chamfer's edges pick those edges, as before: four 45° bends with the
    // chamfers as narrow faces of the sheet.
    let mut e = Session::default();
    rule(&mut e, "Test", t, k, r, 0.5);
    chamfered_channel(&mut e);
    run(&mut e, "sheet.convert", json!({"body": "Channel", "face": [20, 0, 50], "bends": [[0, 4, 50], [4, 0, 50], [36, 0, 50], [40, 4, 50]]}));
    let rows = bend_rows(&mut e);
    assert!(rows.len() == 4 && rows.iter().all(|row| row.0 == 45), "{rows:?}");
    // A chamfer and one of its edges at once is refused.
    let mut f = Session::default();
    rule(&mut f, "Test", t, k, r, 0.5);
    chamfered_channel(&mut f);
    let both = f.execute("sheet.convert", &json!({"body": "Channel", "face": [20, 0, 50], "bends": [[2, 2, 50], [4, 0, 50]]}));
    let msg = both.err().map(|x| x.to_string()).unwrap_or_default();
    assert!(msg.contains("not both"), "{msg}");
}

/// A strip `t` thick along the polyline `outer` (its material to the left of the way the line
/// runs, as [`section`]), extruded `len` along z as the body `Section`, its corners bent as a
/// sheet metal model draws them: an arc of radius `ri` on the inside of each bend and one of
/// `ri + t` on the outside, about one axis. Returns, per corner, a point in the middle of its
/// outer arc and one in the middle of its inner arc.
fn bent_section(s: &mut Session, outer: &[(f64, f64)], t: f64, ri: f64, len: f64) -> Vec<([f64; 3], [f64; 3])> {
    let n = outer.len();
    let dir = |i: usize| {
        let (a, b) = (outer[i], outer[i + 1]);
        let l = (b.0 - a.0).hypot(b.1 - a.1);
        ((b.0 - a.0) / l, (b.1 - a.1) / l)
    };
    let left = |d: (f64, f64)| (-d.1, d.0);
    let at = |p: (f64, f64), d: (f64, f64), k: f64| (p.0 + d.0 * k, p.1 + d.1 * k);
    // Per corner: where its arc starts and ends on the line's skin and on the other skin, the
    // middles of those two arcs, and the middles of its outer and its inner arc.
    let mut corners = Vec::new();
    for (i, &c) in outer.iter().enumerate().take(n - 1).skip(1) {
        let (d0, d1) = (dir(i - 1), dir(i));
        let (cross, dot) = (d0.0 * d1.1 - d0.1 * d1.0, d0.0 * d1.0 + d0.1 * d1.1);
        let theta = cross.abs().atan2(dot);
        // Turning toward the material, this skin is the bend's outside.
        let (rp, ro) = if cross > 0.0 { (ri + t, ri) } else { (ri, ri + t) };
        let a = at(c, d0, -rp * (theta / 2.0).tan());
        let b = at(c, d1, rp * (theta / 2.0).tan());
        let centre = at(a, left(d0), if cross > 0.0 { rp } else { -rp });
        let (ai, bi) = (at(a, left(d0), t), at(b, left(d1), t));
        let l = (c.0 - centre.0).hypot(c.1 - centre.1);
        let mid = |r: f64| (centre.0 + (c.0 - centre.0) / l * r, centre.1 + (c.1 - centre.1) / l * r);
        corners.push((a, b, ai, bi, mid(rp), mid(ro), (mid(ri + t), mid(ri))));
    }
    let xy = |p: (f64, f64)| [p.0, p.1];
    run(s, "sketch.create", json!({"plane": "XY", "name": "Section"}));
    let mut from = outer[0];
    for &(a, b, _, _, m, ..) in &corners {
        run(s, "sketch.line", json!({"points": [xy(from), xy(a)]}));
        run(s, "sketch.arc.three_point", json!({"start": xy(a), "end": xy(b), "through": xy(m)}));
        from = b;
    }
    let (last, first) = (at(outer[n - 1], left(dir(n - 2)), t), at(outer[0], left(dir(0)), t));
    run(s, "sketch.line", json!({"points": [xy(from), xy(outer[n - 1]), xy(last)]}));
    from = last;
    for &(_, _, ai, bi, _, m, _) in corners.iter().rev() {
        run(s, "sketch.line", json!({"points": [xy(from), xy(bi)]}));
        run(s, "sketch.arc.three_point", json!({"start": xy(bi), "end": xy(ai), "through": xy(m)}));
        from = ai;
    }
    run(s, "sketch.line", json!({"points": [xy(from), xy(first), xy(outer[0])]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Section", "distance": len, "body_name": "Section"}));
    let z = len / 2.0;
    corners.iter().map(|&(.., (m, mi))| ([m.0, m.1, z], [mi.0, mi.1, z])).collect()
}

#[test]
fn a_bend_modelled_with_its_radius_is_one_bend_of_the_rules_radius() {
    // An L: a base 60 and a leg 40 to the sharp corner outside, 2 thick and 100 long, its
    // corner bent R 2 inside and R 4 outside. The rule bends R 3: its radius, not the model's.
    let (t, k, r) = (2.0, 0.5, 3.0);
    let l = |s: &mut Session| {
        rule(s, "Test", t, k, r, 0.5);
        bent_section(s, &[(0.0, 40.0), (0.0, 0.0), (60.0, 0.0)], t, 2.0, 100.0)[0]
    };
    let mut s = Session::default();
    let (outer, inner) = l(&mut s);
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [30, 0, 50], "bends": [outer]}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 1 && rows[0].0 == 90 && rows[0].1 == "up" && (rows[0].2 - 100.0).abs() < 1e-6, "{rows:?}");
    let allowance = flat(&mut s)["bends"][0]["allowance"].as_f64().unwrap_or(0.0);
    assert!((allowance - FRAC_PI_2 * (r + k * t)).abs() < 1e-9, "{allowance}");
    // The outside lengths to the virtual sharp less one 90° deduction:
    // 60 + 40 - (2 (R + T) - π/2 (R + K T)) = 100 - (10 - 2π) = 96.2832.
    let across = 100.0 - deduction(FRAC_PI_2, r, t, k);
    assert!((across - 96.283185).abs() < 1e-6);
    assert!(same_size(flat_size(&mut s), (across, 100.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // The envelope of the sharp corner stays; the round is the rule's, R + T outside.
    let (lo, hi) = bbox(&mut s);
    assert!(
        lo.iter().all(|v| v.abs() < 1e-6) && (hi[0] - 60.0).abs() < 1e-6 && (hi[1] - 40.0).abs() < 1e-6 && (hi[2] - 100.0).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // Two flat strips and a quarter round on the neutral radius R + T/2.
    let expect = t * 100.0 * ((60.0 - (r + t)) + (40.0 - (r + t)) + FRAC_PI_2 * (r + t / 2.0));
    assert!((volume(&mut s) - expect).abs() < 2e-3 * expect, "{} vs {expect}", volume(&mut s));
    // The design keeps the cylinder's point and reopens to the same sheet.
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(same_size(flat_size(&mut reopened), (across, 100.0), 1e-6), "{:?}", flat_size(&mut reopened));

    // The inner cylinder is the same bend; and from the inside of the base, the sheet's other
    // skin is the fixed one, the bend turning down from it, picked on either cylinder: the
    // inside lengths to the inner sharp, 58 + 38, less 2 R plus the allowance, the same blank.
    assert!((58.0 + 38.0 - 2.0 * r + allowance - across).abs() < 1e-9);
    for (face, bend, direction) in [([30.0, 0.0, 50.0], inner, "up"), ([30.0, 2.0, 50.0], outer, "down"), ([30.0, 2.0, 50.0], inner, "down")] {
        let mut b = Session::default();
        l(&mut b);
        run(&mut b, "sheet.convert", json!({"body": "Section", "face": face, "bends": [bend]}));
        assert!(bend_rows(&mut b).iter().all(|row| row.0 == 90 && row.1 == direction), "{face:?} {bend:?}: {:?}", bend_rows(&mut b));
        assert!(same_size(flat_size(&mut b), (across, 100.0), 1e-6), "{face:?} {bend:?}: {:?}", flat_size(&mut b));
        assert!((volume(&mut b) - expect).abs() < 2e-3 * expect, "{face:?} {bend:?}: {} vs {expect}", volume(&mut b));
    }
}

#[test]
fn a_channel_with_its_bends_modelled_folds_back_into_itself_under_its_own_radius() {
    // A channel 40 wide with walls 30 high to the sharp corners outside, 2 thick and 100 long,
    // bent R 2 inside, R 4 outside. Under a rule of that radius the sheet is the model.
    let (t, k, r) = (2.0, 0.44, 2.0);
    let channel = |s: &mut Session| {
        rule(s, "Test", t, k, r, 0.5);
        bent_section(s, &[(0.0, 30.0), (0.0, 0.0), (40.0, 0.0), (40.0, 30.0)], t, r, 100.0)
    };
    let mut s = Session::default();
    let corners = channel(&mut s);
    let solid = volume(&mut s);
    // Two flat walls, the base and two quarter rounds on the neutral radius R + T/2.
    let expect = t * 100.0 * (2.0 * (30.0 - (r + t)) + (40.0 - 2.0 * (r + t)) + PI * (r + t / 2.0));
    assert!((solid - expect).abs() < 2e-3 * expect, "{solid} vs {expect}");
    // One bend picked on its outer cylinder, the other on its inner one.
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [20, 0, 50], "bends": [corners[0].0, corners[1].1]}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 2 && rows.iter().all(|row| row.0 == 90 && row.1 == "up" && (row.2 - 100.0).abs() < 1e-6), "{rows:?}");
    // 30 + 40 + 30 less two deductions, 2 (R + T) - π/2 (R + K T) each.
    let across = 100.0 - 2.0 * deduction(FRAC_PI_2, r, t, k);
    assert!(same_size(flat_size(&mut s), (across, 100.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // The same solid as before: its volume and its box.
    assert!((volume(&mut s) - solid).abs() < 1e-3 * solid, "{} vs {solid}", volume(&mut s));
    let (lo, hi) = bbox(&mut s);
    assert!(
        lo.iter().all(|v| v.abs() < 1e-6) && (hi[0] - 40.0).abs() < 1e-6 && (hi[1] - 30.0).abs() < 1e-6 && (hi[2] - 100.0).abs() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // Unfold and refold keep it.
    run(&mut s, "sheet.unfold", json!({}));
    let (lo, hi) = bbox(&mut s);
    let flat_box = [(hi[0] - lo[0], hi[1] - lo[1]), (hi[0] - lo[0], hi[2] - lo[2]), (hi[1] - lo[1], hi[2] - lo[2])];
    assert!(flat_box.iter().any(|b| same_size(*b, (across, 100.0), 1e-6)), "{lo:?} {hi:?}");
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - solid).abs() < 1e-3 * solid);
}

/// A rule's values follow the sheet's own thickness (#4328): an L shell 2 thick, its bend
/// modelled R 2 inside, converted under the default rule (2.5 thick, bends R `Thickness`) is a
/// sheet 2 thick bent R 2, the solid unchanged; an edge flange added to it bends R 2 as well.
#[test]
fn a_converted_shell_takes_its_rules_values_at_its_own_thickness() {
    let mut s = Session::default();
    let (outer, _) = bent_section(&mut s, &[(0.0, 40.0), (0.0, 0.0), (60.0, 0.0)], 2.0, 2.0, 100.0)[0];
    let solid = volume(&mut s);
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [30, 0, 50], "bends": [outer]}));
    assert!((volume(&mut s) - solid).abs() < 1e-6 * solid, "{} vs {solid}", volume(&mut s));
    let f = flat(&mut s);
    assert_eq!((f["thickness"].as_f64(), f["bends"][0]["radius"].as_f64()), (Some(2.0), Some(2.0)), "{f}");
    run(&mut s, "sheet.flange", json!({"edges": [[0, 40, 50]], "height": 10}));
    let f = flat(&mut s);
    assert_eq!(f["bends"][1]["radius"].as_f64(), Some(2.0), "{f}");
}

#[test]
fn a_filleted_edge_of_a_solid_block_is_a_bend() {
    // A block's bottom edge rounded R 5: the faces either side meet at the old sharp edge,
    // the rule's thickness taken inward, an L with a 100 base and a 30 wall outside.
    let (t, k, r) = (2.0, 0.44, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 30, "body_name": "Block"}));
    run(&mut s, "solid.fillet", json!({"edges": [[100, 30, 0]], "radius": 5}));
    let h = 5.0 * (1.0 - 0.5f64.sqrt());
    run(&mut s, "sheet.convert", json!({"body": "Block", "face": [50, 30, 0], "bends": [[100.0 - h, 30.0, h]]}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 1 && rows[0].0 == 90 && rows[0].1 == "up" && (rows[0].2 - 60.0).abs() < 1e-6, "{rows:?}");
    let across = 130.0 - deduction(FRAC_PI_2, r, t, k);
    assert!(same_size(flat_size(&mut s), (across, 60.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
}

#[test]
fn edge_flanges_and_cuts_work_on_a_converted_sheet() {
    let mut s = Session::default();
    rule(&mut s, "Test", 2.0, 0.44, 2.0, 0.5);
    tray(&mut s, 2.0);
    run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES}));
    let v0 = volume(&mut s);
    // A return flange on the front wall's top edge.
    run(&mut s, "sheet.flange", json!({"edges": [[50, 0, 30]], "height": 10, "angle": 90}));
    let v1 = volume(&mut s);
    assert!(v1 > v0 + 500.0, "{v0} {v1}");
    assert_eq!(bend_rows(&mut s).len(), 5);
    // A hole cut in the flat stays in the base when the tray is folded again.
    run(&mut s, "sheet.unfold", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Hole"}));
    run(&mut s, "sketch.circle.center", json!({"center": [50, 30], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Hole", "distance": 10, "operation": "cut", "direction": "symmetric"}));
    run(&mut s, "sheet.refold", json!({}));
    assert_eq!(flat(&mut s)["cutouts"].as_array().map(Vec::len), Some(1));
    let hole = PI * 25.0 * 2.0;
    assert!((volume(&mut s) - (v1 - hole)).abs() < 2e-3 * v1, "{} vs {}", volume(&mut s), v1 - hole);
}

#[test]
fn conversions_that_cannot_work_say_why() {
    let mut s = Session::default();
    rule(&mut s, "Wide", 2.0, 0.44, 40.0, 0.5);
    rule(&mut s, "Test", 2.0, 0.44, 2.0, 0.5);
    tray(&mut s, 2.0);
    let err = |s: &mut Session, p: Value| s.execute("sheet.convert", &p).err().map(|e| e.to_string()).unwrap_or_default();
    // A bend that doesn't reach the fixed face through other bends.
    let e = err(&mut s, json!({"body": "Tray", "face": [50, 30, 0], "bends": [[0, 0, 15]]}));
    assert!(e.contains("does not connect"), "{e}");
    // No edge there, and no face there.
    let e = err(&mut s, json!({"body": "Tray", "face": [50, 30, 0], "bends": [[50, 30, 0]]}));
    assert!(e.contains("no edge"), "{e}");
    let e = err(&mut s, json!({"body": "Tray", "face": [50, 30, -20], "bends": BOTTOM_EDGES}));
    assert!(e.contains("no face"), "{e}");
    // A loop of bends.
    let mut bends: Vec<[f64; 3]> = BOTTOM_EDGES.to_vec();
    bends.push([0.0, 0.0, 15.0]);
    let e = err(&mut s, json!({"body": "Tray", "face": [50, 30, 0], "bends": bends}));
    assert!(e.contains("loop"), "{e}");
    // Walls shorter than their bends' setbacks (a 40 mm bend radius on 30 mm walls).
    let e = err(&mut s, json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES, "rule": "Wide"}));
    assert!(e.contains("setback"), "{e}");
    // The sheet's values are the rule's; an empty list of bends is not a plate.
    let e = err(&mut s, json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES, "thickness": 3}));
    assert!(e.contains("the sheet metal rule sets it"), "{e}");
    let e = err(&mut s, json!({"body": "Tray", "face": [50, 30, 0], "bends": []}));
    assert!(e.contains("1…1000"), "{e}");
    // Curved faces.
    let mut c = Session::default();
    run(&mut c, "solid.cylinder", json!({"radius": 10, "height": 30, "body_name": "Rod"}));
    let e = err(&mut c, json!({"body": "Rod", "face": [0, 0, 30], "bends": [[10, 0, 30]]}));
    assert!(e.contains("curved"), "{e}");
    // Nothing changed.
    assert!(s.model.state().sheets.is_empty() && c.model.state().sheets.is_empty());
}

/// One side face of a plate written to STEP: flat from `a` to `b`, or a piece of a round wall
/// about `c` from `a` to `b` (counter-clockwise seen from +z when `ccw`).
#[derive(Clone, Copy)]
enum Wall {
    Flat([f64; 2], [f64; 2]),
    Round([f64; 2], [f64; 2], [f64; 2], bool),
}

impl Wall {
    fn ends(&self) -> ([f64; 2], [f64; 2]) {
        match *self {
            Wall::Flat(a, b) | Wall::Round(a, b, _, _) => (a, b),
        }
    }
}

/// A round hole of radius `r` about `c` (clockwise seen from +z), its wall in `pieces` cylinder
/// faces, the first seam at `seam` degrees.
fn hole(c: [f64; 2], r: f64, pieces: usize, seam: f64) -> Vec<Wall> {
    let at = |i: usize| {
        let a = (seam - 360.0 * i as f64 / pieces as f64).to_radians();
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    };
    (0..pieces).map(|i| Wall::Round(at(i), at(i + 1), c, false)).collect()
}

/// A rectangle (counter-clockwise), its bottom side split in two faces when `split`.
fn rectangle(w: f64, h: f64, split: bool) -> Vec<Wall> {
    let mut walls = if split {
        vec![Wall::Flat([0.0, 0.0], [w / 2.0, 0.0]), Wall::Flat([w / 2.0, 0.0], [w, 0.0])]
    } else {
        vec![Wall::Flat([0.0, 0.0], [w, 0.0])]
    };
    walls.extend([Wall::Flat([w, 0.0], [w, h]), Wall::Flat([w, h], [0.0, h]), Wall::Flat([0.0, h], [0.0, 0.0])]);
    walls
}

/// The entities of a STEP file, numbered as they are added; vertices and vertical edges once.
#[derive(Default)]
struct Step {
    lines: Vec<String>,
    vertices: Vec<([i64; 3], usize)>,
    verticals: Vec<([i64; 2], usize)>,
}

impl Step {
    fn add(&mut self, entity: String) -> usize {
        self.lines.push(format!("#{}={entity};", self.lines.len() + 1));
        self.lines.len()
    }

    fn point(&mut self, p: [f64; 3]) -> usize {
        self.add(format!("CARTESIAN_POINT('',({:.12},{:.12},{:.12}))", p[0], p[1], p[2]))
    }

    fn frame(&mut self, o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> usize {
        let o = self.point(o);
        let z = self.add(format!("DIRECTION('',({:.12},{:.12},{:.12}))", z[0], z[1], z[2]));
        let x = self.add(format!("DIRECTION('',({:.12},{:.12},{:.12}))", x[0], x[1], x[2]));
        self.add(format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }

    fn vertex(&mut self, p: [f64; 2], z: f64) -> usize {
        let key = [p[0], p[1], z].map(|v| (v * 1e9).round() as i64);
        if let Some((_, v)) = self.vertices.iter().find(|(k, _)| *k == key) {
            return *v;
        }
        let point = self.point([p[0], p[1], z]);
        let v = self.add(format!("VERTEX_POINT('',#{point})"));
        self.vertices.push((key, v));
        v
    }

    fn line(&mut self, a: [f64; 3], b: [f64; 3], va: usize, vb: usize) -> usize {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let p = self.point(a);
        let dir = self.add(format!("DIRECTION('',({:.12},{:.12},{:.12}))", d[0] / l, d[1] / l, d[2] / l));
        let v = self.add(format!("VECTOR('',#{dir},{l:.12})"));
        let line = self.add(format!("LINE('',#{p},#{v})"));
        self.add(format!("EDGE_CURVE('',#{va},#{vb},#{line},.T.)"))
    }

    /// The wall's edge at height `z`.
    fn edge(&mut self, wall: Wall, z: f64) -> usize {
        let (a, b) = wall.ends();
        let (va, vb) = (self.vertex(a, z), self.vertex(b, z));
        match wall {
            Wall::Flat(..) => self.line([a[0], a[1], z], [b[0], b[1], z], va, vb),
            Wall::Round(_, _, c, ccw) => {
                let frame = self.frame([c[0], c[1], z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
                let r = (a[0] - c[0]).hypot(a[1] - c[1]);
                let circle = self.add(format!("CIRCLE('',#{frame},{r:.12})"));
                self.add(format!("EDGE_CURVE('',#{va},#{vb},#{circle},{})", if ccw { ".T." } else { ".F." }))
            }
        }
    }

    fn vertical(&mut self, p: [f64; 2], t: f64) -> usize {
        let key = p.map(|v| (v * 1e9).round() as i64);
        if let Some((_, e)) = self.verticals.iter().find(|(k, _)| *k == key) {
            return *e;
        }
        let (v0, v1) = (self.vertex(p, 0.0), self.vertex(p, t));
        let e = self.line([p[0], p[1], 0.0], [p[0], p[1], t], v0, v1);
        self.verticals.push((key, e));
        e
    }

    fn edge_loop(&mut self, edges: &[(usize, bool)]) -> usize {
        let oriented: Vec<String> = edges
            .iter()
            .map(|(e, forward)| format!("#{}", self.add(format!("ORIENTED_EDGE('',*,*,#{e},{})", if *forward { ".T." } else { ".F." }))))
            .collect();
        self.add(format!("EDGE_LOOP('',({}))", oriented.join(",")))
    }
}

/// STEP text of a plate `t` thick standing on z = 0, its outline `loops` (the outer one
/// counter-clockwise, holes clockwise, seen from +z) with one side face per wall: a plane, or a
/// cylinder piece. Hand-written, the way exporters that split closed faces write them.
fn plate_step(loops: &[Vec<Wall>], t: f64) -> String {
    let mut s = Step::default();
    let bottom: Vec<Vec<usize>> = loops.iter().map(|l| l.iter().map(|w| s.edge(*w, 0.0)).collect()).collect();
    let top: Vec<Vec<usize>> = loops.iter().map(|l| l.iter().map(|w| s.edge(*w, t)).collect()).collect();
    let mut faces = Vec::new();
    for (i, walls) in loops.iter().enumerate() {
        for (j, wall) in walls.iter().enumerate() {
            let (a, b) = wall.ends();
            let (va, vb) = (s.vertical(a, t), s.vertical(b, t));
            let lp = s.edge_loop(&[(bottom[i][j], true), (vb, true), (top[i][j], false), (va, false)]);
            let bound = s.add(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
            let (surface, sense) = match *wall {
                Wall::Flat(a, b) => {
                    let l = (b[0] - a[0]).hypot(b[1] - a[1]);
                    let (dx, dy) = ((b[0] - a[0]) / l, (b[1] - a[1]) / l);
                    let frame = s.frame([a[0], a[1], 0.0], [dy, -dx, 0.0], [dx, dy, 0.0]);
                    (s.add(format!("PLANE('',#{frame})")), true)
                }
                Wall::Round(a, _, c, ccw) => {
                    let frame = s.frame([c[0], c[1], 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
                    let r = (a[0] - c[0]).hypot(a[1] - c[1]);
                    (s.add(format!("CYLINDRICAL_SURFACE('',#{frame},{r:.12})")), ccw)
                }
            };
            faces.push(s.add(format!("ADVANCED_FACE('',(#{bound}),#{surface},{})", if sense { ".T." } else { ".F." })));
        }
    }
    for (z, edges, up) in [(t, &top, true), (0.0, &bottom, false)] {
        let mut bounds = Vec::new();
        for (i, es) in edges.iter().enumerate() {
            let walk: Vec<(usize, bool)> = if up { es.iter().map(|e| (*e, true)).collect() } else { es.iter().rev().map(|e| (*e, false)).collect() };
            let lp = s.edge_loop(&walk);
            bounds.push(format!("#{}", s.add(format!("{}('',#{lp},.T.)", if i == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" }))));
        }
        let frame = s.frame([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let plane = s.add(format!("PLANE('',#{frame})"));
        faces.push(s.add(format!("ADVANCED_FACE('',({}),#{plane},{})", bounds.join(","), if up { ".T." } else { ".F." })));
    }
    let faces: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
    let shell = s.add(format!("CLOSED_SHELL('',({}))", faces.join(",")));
    let brep = s.add(format!("MANIFOLD_SOLID_BREP('Plate',#{shell})"));
    s.add(format!("SHAPE_REPRESENTATION('',(#{brep}),$)"));
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('plate','',(''),(''),'','hand','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n{}\nENDSEC;\nEND-ISO-10303-21;\n",
        s.lines.join("\n")
    )
}

/// The plate inserted from a STEP file and converted, picked on its top face at `pick`.
fn convert_step_plate(tag: &str, loops: &[Vec<Wall>], t: f64, pick: [f64; 2]) -> Session {
    let path = std::env::temp_dir().join(format!("solidcraft-split-walls-{tag}-{}.step", std::process::id()));
    std::fs::write(&path, plate_step(loops, t)).unwrap();
    let mut s = Session::default();
    run(&mut s, "file.insert_step", json!({"path": path.to_string_lossy()}));
    let _ = std::fs::remove_file(&path);
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [pick[0], pick[1], t]}));
    s
}

/// The flat pattern's cut-outs that are whole circles, as (radius, centre).
fn flat_circles(s: &mut Session) -> Vec<(f64, [f64; 2])> {
    let shapes = flat(s)["cutout_shapes"].as_array().cloned().unwrap_or_default();
    let c = |v: &Value| [v["center"][0].as_f64().unwrap_or(f64::NAN), v["center"][1].as_f64().unwrap_or(f64::NAN)];
    shapes.iter().filter_map(|x| x.get("circle")).map(|x| (x["radius"].as_f64().unwrap_or(f64::NAN), c(x))).collect()
}

/// How many entities of each kind the flat pattern's DXF has.
fn dxf_entities(s: &mut Session) -> (usize, usize, usize) {
    let dxf = run(s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap_or("").replace("\r\n", "\n");
    // Group codes may be padded (`  0`): compare the code and value lines trimmed.
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let n = |kind: &str| lines.chunks(2).filter(|pair| pair == &["0", kind]).count();
    (n("LINE"), n("ARC"), n("CIRCLE"))
}

/// A plate whose round holes' walls are two half-cylinders each (as many exporters write a hole)
/// converts with the holes as circles. It was refused as "not a plate of even thickness": its
/// holes came out as polygons, whose area missed the holes' by more than the plate check allows.
#[test]
fn a_plate_whose_hole_walls_are_half_cylinders_converts_with_round_holes() {
    let (w, t, r) = (30.0, 2.0, 2.0);
    let centres = [[6.0, 6.0], [24.0, 6.0], [24.0, 24.0], [6.0, 24.0]];
    let mut loops = vec![rectangle(w, w, false)];
    loops.extend(centres.iter().map(|c| hole(*c, r, 2, 90.0)));
    let mut s = convert_step_plate("halves", &loops, t, [15.0, 15.0]);
    let want = (w * w - 4.0 * PI * r * r) * t;
    assert!((volume(&mut s) - want).abs() < 1e-4 * want, "{} vs {want}", volume(&mut s));
    let circles = flat_circles(&mut s);
    assert_eq!(circles.len(), 4, "{circles:?}");
    assert!(circles.iter().all(|(radius, _)| (radius - r).abs() < 1e-9), "{circles:?}");
    // The holes keep their places: 18 apart along the sides, 18·√2 across.
    let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let mut gaps: Vec<f64> = (0..4).flat_map(|i| (i + 1..4).map(move |j| (i, j))).map(|(i, j)| d(circles[i].1, circles[j].1)).collect();
    gaps.sort_by(f64::total_cmp);
    assert!(gaps[..4].iter().all(|g| (g - 18.0).abs() < 1e-9) && gaps[4..].iter().all(|g| (g - 18.0 * SQRT_2).abs() < 1e-9), "{gaps:?}");
    // The DXF: the outline's four lines and one circle per hole.
    assert_eq!(dxf_entities(&mut s), (4, 0, 4));
}

/// Holes of any radius whose walls come in two, three or four pieces, seams anywhere: each is one
/// circle.
#[test]
fn hole_walls_in_any_number_of_pieces_each_make_one_circle() {
    let (t, holes) = (2.0, [([20.0, 30.0], 5.0, 2, 0.0), ([50.0, 30.0], 3.0, 3, 37.0), ([80.0, 30.0], 8.0, 4, 10.0)]);
    let mut loops = vec![rectangle(100.0, 60.0, false)];
    loops.extend(holes.iter().map(|(c, r, pieces, seam)| hole(*c, *r, *pieces, *seam)));
    let mut s = convert_step_plate("pieces", &loops, t, [35.0, 10.0]);
    let want = (6000.0 - PI * (25.0 + 9.0 + 64.0)) * t;
    assert!((volume(&mut s) - want).abs() < 1e-4 * want, "{} vs {want}", volume(&mut s));
    let mut radii: Vec<f64> = flat_circles(&mut s).iter().map(|c| c.0).collect();
    radii.sort_by(f64::total_cmp);
    assert!(radii.len() == 3 && radii.iter().zip([3.0, 5.0, 8.0]).all(|(a, b)| (a - b).abs() < 1e-9), "{radii:?}");
    assert_eq!(dxf_entities(&mut s), (4, 0, 3));
}

/// Other walls such files split: rounded corners in two pieces each, a slot whose round ends are
/// in two pieces each, a straight side in two faces. Each corner and slot end is one arc, the
/// side one line.
#[test]
fn rounded_corners_slot_ends_and_split_sides_convert_as_single_arcs_and_lines() {
    let (t, rc, rs, rh) = (2.0, 6.0, 4.0, 5.0);
    let quarter = |c: [f64; 2], from: f64, pieces: usize| -> Vec<Wall> {
        let at = |i: usize| {
            let a = (from + 90.0 * i as f64 / pieces as f64).to_radians();
            [c[0] + rc * a.cos(), c[1] + rc * a.sin()]
        };
        (0..pieces).map(|i| Wall::Round(at(i), at(i + 1), c, true)).collect()
    };
    // 100 x 60, corners rounded by 6, the bottom side in two faces.
    let mut outer = vec![Wall::Flat([rc, 0.0], [50.0, 0.0]), Wall::Flat([50.0, 0.0], [100.0 - rc, 0.0])];
    outer.extend(quarter([100.0 - rc, rc], -90.0, 2));
    outer.push(Wall::Flat([100.0, rc], [100.0, 60.0 - rc]));
    outer.extend(quarter([100.0 - rc, 60.0 - rc], 0.0, 2));
    outer.push(Wall::Flat([100.0 - rc, 60.0], [rc, 60.0]));
    outer.extend(quarter([rc, 60.0 - rc], 90.0, 2));
    outer.push(Wall::Flat([0.0, 60.0 - rc], [0.0, rc]));
    outer.extend(quarter([rc, rc], 180.0, 2));
    // A slot 30 between its end centres, 8 wide, each end in two pieces (clockwise).
    let (a, b) = ([45.0, 30.0], [75.0, 30.0]);
    let slot = vec![
        Wall::Flat([a[0], a[1] + rs], [b[0], b[1] + rs]),
        Wall::Round([b[0], b[1] + rs], [b[0] + rs, b[1]], b, false),
        Wall::Round([b[0] + rs, b[1]], [b[0], b[1] - rs], b, false),
        Wall::Flat([b[0], b[1] - rs], [a[0], a[1] - rs]),
        Wall::Round([a[0], a[1] - rs], [a[0] - rs, a[1]], a, false),
        Wall::Round([a[0] - rs, a[1]], [a[0], a[1] + rs], a, false),
    ];
    let loops = vec![outer, slot, hole([20.0, 30.0], rh, 2, 90.0)];
    let mut s = convert_step_plate("sides", &loops, t, [20.0, 50.0]);
    let area = 6000.0 - (4.0 - PI) * rc * rc - (30.0 * 2.0 * rs + PI * rs * rs) - PI * rh * rh;
    assert!((volume(&mut s) - area * t).abs() < 1e-4 * area * t, "{} vs {}", volume(&mut s), area * t);
    // The outline: four lines and four quarter arcs; the slot: two lines and two half arcs; the
    // hole: a circle.
    assert_eq!(dxf_entities(&mut s), (6, 6, 1));
    let shapes = flat(&mut s)["cutout_shapes"].as_array().cloned().unwrap_or_default();
    let slot = shapes.iter().find_map(|x| x["segments"].as_array().cloned()).unwrap_or_default();
    let sweeps: Vec<f64> = slot.iter().filter_map(|x| x["arc"]["sweep_deg"].as_f64()).map(f64::abs).collect();
    assert!(slot.len() == 4 && sweeps.len() == 2 && sweeps.iter().all(|x| (x - 180.0).abs() < 1e-6), "{slot:?}");
}

/// Holes drilled here come out as circles too (their walls are half-cylinders as well), and a
/// plate with many small ones converts: nine 4 mm holes in a 30 x 30 x 2 plate were refused too.
#[test]
fn a_plate_with_holes_drilled_here_converts_with_round_holes() {
    let (w, t, r) = (30.0, 2.0, 2.0);
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": w, "width": w, "height": t, "body_name": "Plate"}));
    for x in [6.0, 15.0, 24.0] {
        for y in [6.0, 15.0, 24.0] {
            run(&mut s, "solid.hole", json!({"position": [x, y, t], "diameter": 2.0 * r}));
        }
    }
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [10.5, 10.5, t]}));
    let want = (w * w - 9.0 * PI * r * r) * t;
    assert!((volume(&mut s) - want).abs() < 1e-4 * want, "{} vs {want}", volume(&mut s));
    let circles = flat_circles(&mut s);
    assert!(circles.len() == 9 && circles.iter().all(|(radius, _)| (radius - r).abs() < 1e-9), "{circles:?}");
    assert_eq!(dxf_entities(&mut s), (4, 0, 9));
}

/// A box `l` x `w` x `t` with round holes (centre, diameter) drilled through it, converted,
/// picked on its top face.
fn convert_box(l: f64, w: f64, t: f64, holes: &[([f64; 2], f64)]) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": l, "width": w, "height": t, "body_name": "Plate"}));
    for (c, d) in holes {
        run(&mut s, "solid.hole", json!({"position": [c[0], c[1], t], "diameter": d}));
    }
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [l * 0.4, w * 0.25, t]}));
    s
}

/// A converted plate stays where the plate was, whichever of its faces is picked: the sheet
/// takes the plate's two faces, not the space one thickness beyond the face picked (a 100 x 60 x 2
/// plate on XY used to come out at z −2…0 picked on top, and 2…4 picked underneath).
#[test]
fn a_converted_plate_stays_where_the_plate_was() {
    for pick in [[40.0, 15.0, 2.0], [40.0, 15.0, 0.0]] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 2, "body_name": "Plate"}));
        let before = bbox(&mut s);
        run(&mut s, "sheet.convert", json!({"body": "Plate", "face": pick}));
        assert!(!s.model.state().sheets.is_empty(), "{pick:?}");
        let after = bbox(&mut s);
        let moved = before.0.iter().chain(&before.1).zip(after.0.iter().chain(&after.1)).any(|(a, b)| (a - b).abs() > 1e-9);
        assert!(!moved, "{pick:?}: {before:?} became {after:?}");
        assert!((volume(&mut s) - 12_000.0).abs() < 1e-6, "{pick:?}: {}", volume(&mut s));
    }
}

/// A long thin strip converts. 1200 x 100 x 1 was refused with "no outline on that face": the
/// outline's tolerance was a thousandth of the body's size (1.2), more than the thickness, so the
/// far side's triangles counted as the picked face's and no boundary was left.
#[test]
fn a_long_thin_strip_converts() {
    let mut s = convert_box(1200.0, 100.0, 1.0, &[]);
    assert!((volume(&mut s) - 120_000.0).abs() < 1e-6 * 120_000.0, "{}", volume(&mut s));
    assert!(same_size(flat_size(&mut s), (1200.0, 100.0), 1e-6), "{:?}", flat_size(&mut s));
    let t = flat(&mut s)["flat_size_mm"][2].as_f64().unwrap_or(f64::NAN);
    assert!((t - 1.0).abs() < 1e-9, "{t}");
    assert_eq!(dxf_entities(&mut s), (4, 0, 0));
}

/// Plates further past that, a thousandth of their size two to six times their thickness: a
/// full sheet, a foil, a long bar.
#[test]
fn plates_thousands_of_times_longer_than_thick_convert() {
    for (l, w, t) in [(3000.0, 1500.0, 0.5), (400.0, 100.0, 0.2), (6000.0, 20.0, 1.0)] {
        let mut s = convert_box(l, w, t, &[]);
        let want = l * w * t;
        assert!((volume(&mut s) - want).abs() < 1e-6 * want, "{l} x {w} x {t}: {} vs {want}", volume(&mut s));
        assert!(same_size(flat_size(&mut s), (l, w), 1e-6), "{l} x {w} x {t}: {:?}", flat_size(&mut s));
        assert_eq!(dxf_entities(&mut s), (4, 0, 0), "{l} x {w} x {t}");
    }
}

/// The strip with round holes down to its thickness across keeps every one as a circle. At 1200
/// long it was refused; just under the old limit, 990 long, it converted without the 1 mm hole:
/// the outline's chords could sag 0.99, more than that hole's radius, and it fell out.
#[test]
fn a_long_thin_strip_keeps_its_small_round_holes() {
    let d = [10.0, 4.0, 2.0, 1.0];
    for l in [990.0, 1200.0] {
        let holes: Vec<([f64; 2], f64)> = d.iter().enumerate().map(|(i, d)| ([l * (i as f64 + 1.0) / 5.0, 50.0], *d)).collect();
        let mut s = convert_box(l, 100.0, 1.0, &holes);
        let want = l * 100.0 - PI / 4.0 * d.iter().map(|d| d * d).sum::<f64>();
        assert!((volume(&mut s) - want).abs() < 1e-5 * want, "{l}: {} vs {want}", volume(&mut s));
        let mut radii: Vec<f64> = flat_circles(&mut s).iter().map(|c| c.0).collect();
        radii.sort_by(f64::total_cmp);
        assert!(radii.len() == 4 && radii.iter().zip([0.5, 1.0, 2.0, 5.0]).all(|(a, b)| (a - b).abs() < 1e-9), "{l}: {radii:?}");
        assert_eq!(dxf_entities(&mut s), (4, 0, 4), "{l}");
    }
}

/// A 60 x 40 x 1 plate on XY.
fn plate(s: &mut Session) {
    run(s, "solid.box", json!({"length": 60, "width": 40, "height": 1, "body_name": "Plate"}));
}

/// The plate with what `draw` sketches on its top cut `depth` deep into it (the profiles at
/// `profiles`, all when none).
fn engraved_plate(s: &mut Session, depth: f64, draw: impl Fn(&mut Session), profiles: &[[f64; 2]]) {
    plate(s);
    run(s, "sketch.create", json!({"plane": "XY", "offset": 1, "name": "Label"}));
    draw(s);
    run(s, "sketch.finish", json!({}));
    let mut cut = json!({"sketch": "Label", "distance": depth, "direction": "negative", "operation": "cut"});
    if !profiles.is_empty() {
        cut["profiles"] = profiles.iter().map(|p| json!({"point": p})).collect();
    }
    run(s, "solid.extrude", cut);
}

fn label_rectangle(s: &mut Session) {
    run(s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [30, 20]}));
}

fn contains(s: &Session, p: [f64; 3]) -> bool {
    s.model
        .state()
        .bodies
        .first()
        .is_some_and(|b| b.body.classify_point(solidcraft_geom::Vec3::new(p[0], p[1], p[2])).unwrap() == solidcraft_kernel::PointClass::Inside)
}

/// The LINE entities a DXF draws on `layer` (group codes and values alternate, a line each).
fn lines_on(dxf: &str, layer: &str) -> usize {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let mut in_line = false;
    let mut count = 0;
    for pair in lines.chunks(2) {
        match pair {
            ["0", kind] => in_line = *kind == "LINE",
            ["8", name] if in_line && *name == layer => count += 1,
            _ => {}
        }
    }
    count
}

/// A plate with an engraving converts: the engraving is a pocket of the sheet, so the sheet keeps
/// the plate's shape, and its outline is the flat pattern's pocket, on the DXF's pocket layer.
/// Picked on the engraved face, the pocket starts from the sheet's z = 0 (the face picked);
/// picked on the other face, it ends at z = T.
#[test]
fn a_plate_with_an_engraving_converts_with_the_engraving_as_a_pocket() {
    for (pick, layer) in [([45.0, 30.0, 1.0], "POCKET_BOTTOM_Z0.000000_TO_0.100000_MM"), ([45.0, 30.0, 0.0], "POCKET_TOP_Z0.900000_TO_1.000000_MM")] {
        let mut s = Session::default();
        engraved_plate(&mut s, 0.1, label_rectangle, &[]);
        run(&mut s, "sheet.convert", json!({"body": "Plate", "face": pick}));
        assert!(s.model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{pick:?}: {:?}", s.model.results);
        // By hand: 60·40·1 less the engraving's 20·10·0.1; the faces' 2·60·40 + 2·(60 + 40)·1 and
        // the engraving's walls, 2·(20 + 10)·0.1.
        let m = run(&mut s, "inspect.measure", json!({}));
        assert!((m["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN) - 2380.0).abs() < 1e-6, "{pick:?}: {m}");
        assert!((m["total"]["area_mm2"].as_f64().unwrap_or(f64::NAN) - 5006.0).abs() < 1e-6, "{pick:?}: {m}");
        // It stays in the face it was cut into, and the plate where it was (z 0…1).
        let (lo, hi) = bbox(&mut s);
        assert!(lo[2].abs() < 1e-9 && (hi[2] - 1.0).abs() < 1e-9, "{pick:?}: {lo:?} {hi:?}");
        assert!(!contains(&s, [20.0, 15.0, hi[2] - 0.05]) && contains(&s, [20.0, 15.0, hi[2] - 0.15]), "{pick:?}");
        assert!(contains(&s, [45.0, 30.0, hi[2] - 0.05]), "{pick:?}");
        // The flat pattern: the whole plate, no cut-out, and the engraving as a pocket 0.1 deep.
        let f = flat(&mut s);
        assert!(same_size(flat_size(&mut s), (60.0, 40.0), 1e-9), "{f}");
        assert_eq!(f["cutouts"], json!([]), "{f}");
        let pockets = f["pockets"].as_array().cloned().unwrap_or_default();
        assert_eq!(pockets.len(), 1, "{f}");
        assert!((pockets[0]["depth_mm"].as_f64().unwrap_or(f64::NAN) - 0.1).abs() < 1e-9, "{f}");
        let corners: Vec<(f64, f64)> =
            pockets[0]["outline"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
        let span = |k: fn(&(f64, f64)) -> f64| corners.iter().map(k).fold(f64::MIN, f64::max) - corners.iter().map(k).fold(f64::MAX, f64::min);
        assert!(same_size((span(|p| p.0), span(|p| p.1)), (20.0, 10.0), 1e-9), "{f}");
        // The DXF: the plate's outline and the engraving's, four lines each, on their layers.
        let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap().to_string();
        assert_eq!((lines_on(&dxf, "OUTLINE"), lines_on(&dxf, layer)), (4, 4), "{pick:?}: {dxf}");
        // Nothing is stored for it: reopened, the design builds the same sheet.
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((volume(&mut reopened) - 2380.0).abs() < 1e-6);
        assert_eq!(flat(&mut reopened), f);
    }
}

/// A plate large enough that its tessellation tolerance (1/1000 of its size, 0.72 mm here) is
/// more than its engraving's depth keeps the engraving too.
#[test]
fn a_large_plate_keeps_an_engraving_shallower_than_its_tessellation_tolerance() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 600, "width": 400, "height": 1, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 1, "name": "Label"}));
    label_rectangle(&mut s);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Label", "distance": 0.1, "direction": "negative", "operation": "cut"}));
    for pick in [[450.0, 300.0, 0.0], [450.0, 300.0, 1.0]] {
        let mut s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        run(&mut s, "sheet.convert", json!({"body": "Plate", "face": pick}));
        // 600·400·1 less the engraving's 20·10·0.1.
        assert!((volume(&mut s) - 239_980.0).abs() < 1e-3, "{pick:?}: {}", volume(&mut s));
        assert_eq!(flat(&mut s)["pockets"].as_array().map(Vec::len), Some(1), "{pick:?}");
    }
}

/// An engraved ring (an "O": a 5 mm circle round a 3 mm island) is one pocket with a hole; the
/// island stays at full thickness.
#[test]
fn an_engraved_ring_converts_as_a_pocket_round_its_island() {
    let mut s = Session::default();
    engraved_plate(
        &mut s,
        0.1,
        |s| {
            run(s, "sketch.circle.center", json!({"center": [20, 20], "radius": 5}));
            run(s, "sketch.circle.center", json!({"center": [20, 20], "radius": 3}));
        },
        &[[24.0, 20.0]],
    );
    // The extrude took the ring between the circles, not the island.
    assert!((volume(&mut s) - (2400.0 - 0.1 * PI * (25.0 - 9.0))).abs() < 1e-3, "{}", volume(&mut s));
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [45, 30, 1]}));
    // The ring's circles are followed within 0.01 mm (1/100 of the thickness): its area within
    // 2/3 of that times the circumferences.
    let want = 2400.0 - 0.1 * PI * (25.0 - 9.0);
    assert!((volume(&mut s) - want).abs() < 0.1 * 0.01 * 2.0 / 3.0 * 2.0 * PI * 8.0, "{} vs {want}", volume(&mut s));
    let (_, hi) = bbox(&mut s);
    assert!(!contains(&s, [24.0, 20.0, hi[2] - 0.05]) && contains(&s, [20.0, 20.0, hi[2] - 0.05]));
    let f = flat(&mut s);
    assert_eq!(f["pockets"].as_array().map(Vec::len), Some(1), "{f}");
    assert_eq!(f["pockets"][0]["cutouts"].as_array().map(Vec::len), Some(1), "{f}");
}

/// What a plate's recesses can't be is refused with the reason, and leaves the design as it was:
/// a recess deeper than half the plate, recesses in both faces, a plate raised in places (its
/// recesses larger than the face they are in); a plate with a boss is no plate, as before. A bend
/// across a kept engraving is refused.
#[test]
fn recesses_a_plate_cannot_keep_are_refused_with_the_reason() {
    let err = |s: &mut Session, face: [f64; 3]| {
        s.execute("sheet.convert", &json!({"body": "Plate", "face": face})).err().map(|e| e.to_string()).unwrap_or_default()
    };
    let mut s = Session::default();
    engraved_plate(&mut s, 0.6, label_rectangle, &[]);
    let e = err(&mut s, [45.0, 30.0, 1.0]);
    assert!(e.contains("0.600 mm deep, more than half the plate's 1.000 mm"), "{e}");
    let mut s = Session::default();
    engraved_plate(&mut s, 0.1, label_rectangle, &[]);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Under"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [40, 20], "p1": [50, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Under", "distance": 0.1, "operation": "cut"}));
    let e = err(&mut s, [45.0, 10.0, 1.0]);
    assert!(e.contains("recesses in both faces"), "{e}");
    // A recess 0.3 mm deep leaving a 2 mm rim: rather a plate 0.3 mm thinner with a raised rim.
    let mut s = Session::default();
    engraved_plate(
        &mut s,
        0.3,
        |s| {
            run(s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [58, 38]}));
        },
        &[],
    );
    let e = err(&mut s, [1.0, 1.0, 1.0]);
    assert!(e.contains("larger than the face they are in"), "{e}");
    // A 0.3 mm boss on the plate: the plate's top round it lies in no opening of the boss's top.
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 1, "name": "Boss"}));
    label_rectangle(&mut s);
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Boss", "distance": 0.3, "operation": "join"}));
    let e = err(&mut s, [20.0, 15.0, 1.3]);
    assert!(e.contains("not a plate of even thickness"), "{e}");
    assert!(s.model.state().sheets.is_empty());
    // A fold across a kept engraving; one clear of it bends.
    let mut s = Session::default();
    engraved_plate(&mut s, 0.1, label_rectangle, &[]);
    run(&mut s, "sheet.convert", json!({"body": "Plate", "face": [45, 30, 1]}));
    let (_, hi) = bbox(&mut s);
    let fold = |x: f64| json!({"points": [[x, 0, hi[2]], [x, 40, hi[2]]], "angle": 90});
    let e = s.execute("sheet.fold", &fold(20.0)).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("crosses a bend zone"), "{e}");
    run(&mut s, "sheet.fold", fold(45.0));
}

/// A bent part converted without bend edges is no plate, as before, whichever face is picked:
/// the faces of its legs inside its extent are no recesses. An L of two 2 mm plates at 90° (its
/// legs' faces lie beside its other faces, in no opening of them), a trapezoid section with
/// returns (faces of its legs on both sides of its base) and a tapered tray (its floor lies in
/// the opening of its rim, but filled it is no plate: its walls lean out).
#[test]
fn a_bent_part_without_bend_edges_is_still_no_plate() {
    let err = |s: &mut Session, body: &str, face: [f64; 3]| {
        s.execute("sheet.convert", &json!({"body": body, "face": face})).err().map(|e| e.to_string()).unwrap_or_default()
    };
    let no_plate = "not supported yet: converting bodies that are not a plate of even thickness without bend edges";
    let mut s = Session::default();
    section(&mut s, &[(0.0, 30.0), (0.0, 0.0), (50.0, 0.0)], 2.0, 40.0);
    for pick in [[25.0, 0.0, 20.0], [25.0, 2.0, 20.0], [1.0, 30.0, 20.0], [0.0, 15.0, 20.0], [2.0, 15.0, 20.0], [50.0, 1.0, 20.0]] {
        let e = err(&mut s, "Section", pick);
        assert!(e.contains(no_plate), "{pick:?}: {e}");
    }
    let mut s = Session::default();
    trapezoid_section(&mut s, 2.0);
    for pick in [[0.0, 0.0, 150.0], [0.0, 2.0, 150.0]] {
        let e = err(&mut s, "Section", pick);
        assert!(e.contains(no_plate), "{pick:?}: {e}");
    }
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Floor"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [200, 120]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Floor", "distance": 40, "taper": "18 deg", "body_name": "Tray"}));
    run(&mut s, "solid.shell", json!({"faces": [[100, 60, 40]], "thickness": 2}));
    let e = err(&mut s, "Tray", [100.0, 60.0, 0.0]);
    assert!(e.contains(no_plate), "{e}");
    assert!(s.model.state().sheets.is_empty());
}

/// The flat pattern's cut-outs that are no whole circle (slots), each by the middle of its arcs'
/// centres.
fn flat_slots(s: &mut Session) -> Vec<[f64; 2]> {
    let shapes = flat(s)["cutout_shapes"].as_array().cloned().unwrap_or_default();
    let centre = |segs: &Vec<Value>| {
        let c: Vec<[f64; 2]> = segs.iter().filter_map(|g| Some([g["arc"]["center"][0].as_f64()?, g["arc"]["center"][1].as_f64()?])).collect();
        let n = c.len().max(1) as f64;
        [c.iter().map(|p| p[0]).sum::<f64>() / n, c.iter().map(|p| p[1]).sum::<f64>() / n]
    };
    shapes.iter().filter_map(|x| x["segments"].as_array()).map(centre).collect()
}

/// A U channel 40 long (along z) and `t` thick: a web 100 wide on y = 0 and two legs 30 high,
/// outside lengths to the sharp corners. Returns a point on each outside corner (the bends).
fn channel(s: &mut Session, t: f64) -> Vec<[f64; 3]> {
    section(s, &[(0.0, 30.0), (0.0, 0.0), (100.0, 0.0), (100.0, 30.0)], t, 40.0)
}

/// The channel folded with two 90° bends, less `cut` mm² of its faces: the web and the legs
/// between their setbacks R + T, and two quarter rounds on the neutral radius R + T/2.
fn channel_volume(t: f64, r: f64, cut: f64) -> f64 {
    t * (40.0 * ((100.0 - 2.0 * (r + t)) + 2.0 * (30.0 - (r + t)) + PI * (r + t / 2.0)) - cut)
}

/// Holes and a slot in the faces of a channel converted by its bend edges are cut-outs of the
/// flat pattern, where the bend allowance puts them: in the fixed face (the web) and in the bent
/// ones (the legs). With bend edges, such faces were refused ("faces with curved edges").
#[test]
fn holes_and_a_slot_in_a_channels_faces_land_where_the_bends_put_them() {
    let (t, k, r) = (2.0, 0.4, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    let bends = channel(&mut s, t);
    // Two 6 mm holes in the web, 30 from either outside corner; one in the right leg and a slot
    // 6 wide and 20 long in the left one, each centred 18 up the leg's outside.
    run(&mut s, "solid.hole", json!({"position": [30, 0, 20], "diameter": 6}));
    run(&mut s, "solid.hole", json!({"position": [70, 0, 20], "diameter": 6}));
    run(&mut s, "solid.hole", json!({"position": [100, 18, 20], "diameter": 6, "depth": 3}));
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Slot"}));
    run(&mut s, "sketch.slot.overall", json!({"p0": [18, 10], "p1": [18, 30], "width": 6}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Slot", "distance": 3, "operation": "cut"}));
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [50, 0, 5], "bends": bends}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 2 && rows.iter().all(|row| row.0 == 90 && row.1 == "up" && (row.2 - 40.0).abs() < 1e-9), "{rows:?}");
    // T 2, R 2, K 0.4: BA = π/2 (R + K T) = 1.4 π, BD = 2 (R + T) - BA = 8 - 1.4 π = 3.6018; the
    // flat is the outside lengths less a BD per bend.
    let bd = deduction(FRAC_PI_2, r, t, k);
    assert!((bd - (8.0 - 1.4 * PI)).abs() < 1e-12);
    let across = 160.0 - 2.0 * bd;
    assert!(same_size(flat_size(&mut s), (across, 40.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // On one line across the flat: the right leg's hole, the web's two, 40 apart, and the slot.
    // From a leg's to the web's nearer one: 18 + 30 along the outside to the sharp, less the BD.
    let (circles, slots) = (flat_circles(&mut s), flat_slots(&mut s));
    assert!(circles.len() == 3 && circles.iter().all(|(radius, _)| (radius - 3.0).abs() < 1e-9) && slots.len() == 1, "{circles:?} {slots:?}");
    let mut centres: Vec<[f64; 2]> = circles.iter().map(|c| c.1).chain(slots).collect();
    centres.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let gaps: Vec<f64> = centres.windows(2).map(|w| w[1][0] - w[0][0]).collect();
    assert!(centres.iter().all(|c| (c[1] - centres[0][1]).abs() < 1e-6), "{centres:?}");
    assert!(gaps.len() == 3 && gaps.iter().zip([48.0 - bd, 40.0, 48.0 - bd]).all(|(g, want)| (g - want).abs() < 1e-6), "{gaps:?}");
    // Folded, flat and folded again, less three holes of radius 3 and the slot (14 x 6 and its
    // round ends).
    let cut = 3.0 * PI * 9.0 + 14.0 * 6.0 + PI * 9.0;
    let v = volume(&mut s);
    let want = channel_volume(t, r, cut);
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
    run(&mut s, "sheet.unfold", json!({}));
    let want = t * (across * 40.0 - cut);
    assert!((volume(&mut s) - want).abs() < 1e-4 * want, "{} vs {want}", volume(&mut s));
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - v).abs() < 1e-6 * v);
    // The DXF: a CIRCLE per hole, and the slot's round ends as ARCs.
    let (_, arcs, holes) = dxf_entities(&mut s);
    assert_eq!((arcs, holes), (2, 3));
}

/// Round edges of a bent face stay arcs: the legs' free corners rounded by 5, and a notch of
/// radius 4 in one leg's free edge. They were refused too.
#[test]
fn rounded_corners_and_a_notch_on_a_leg_stay_arcs_in_the_flat() {
    let (t, k, r) = (2.0, 0.4, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    let bends = channel(&mut s, t);
    run(&mut s, "solid.fillet", json!({"edges": [[1, 30, 0], [1, 30, 40], [99, 30, 0], [99, 30, 40]], "radius": 5}));
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Notch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 20], "radius": 4}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Notch", "distance": 3, "operation": "cut"}));
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [50, 0, 5], "bends": bends}));
    let across = 160.0 - 2.0 * deduction(FRAC_PI_2, r, t, k);
    assert!(same_size(flat_size(&mut s), (across, 40.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // Each corner takes (1 - π/4) 5², the notch half a disc of radius 4.
    let cut = 4.0 * (1.0 - PI / 4.0) * 25.0 + PI * 16.0 / 2.0;
    let v = volume(&mut s);
    let want = channel_volume(t, r, cut);
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
    run(&mut s, "sheet.unfold", json!({}));
    let want = t * (across * 40.0 - cut);
    assert!((volume(&mut s) - want).abs() < 1e-4 * want, "{} vs {want}", volume(&mut s));
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - v).abs() < 1e-6 * v);
    // The DXF: a quarter ARC at each corner and a half one at the notch, no cut-outs.
    let (_, arcs, holes) = dxf_entities(&mut s);
    assert_eq!((arcs, holes), (5, 0));
}

/// A hole that reaches into a bend has no one place in the flat, and a pocket deeper than half
/// the sheet is no engraving: both are refused, by their places. (The web's flat panel starts
/// R + T = 4 from the sharp; the hole reaches from 2 to 8. The pocket is 1.5 deep in the 2 thick
/// web.)
#[test]
fn a_hole_reaching_into_a_bend_and_a_pocket_are_refused_by_their_places() {
    let convert = |cut: &[(&str, Value)]| {
        let mut s = Session::default();
        rule(&mut s, "Test", 2.0, 0.4, 2.0, 0.5);
        let bends = channel(&mut s, 2.0);
        for (id, p) in cut {
            run(&mut s, id, p.clone());
        }
        let e = s.execute("sheet.convert", &json!({"body": "Section", "face": [50, 0, 5], "bends": bends})).err();
        assert!(s.model.state().sheets.is_empty());
        e.map(|e| e.to_string()).unwrap_or_default()
    };
    let e = convert(&[("solid.hole", json!({"position": [5, 0, 20], "diameter": 6}))]);
    assert!(e.contains("the hole at (5.000, 0.000, 20.000) reaches into a bend"), "{e}");
    let e = convert(&[
        ("sketch.create", json!({"plane": "XZ", "name": "Pocket"})),
        ("sketch.rectangle.two_point", json!({"p0": [40, -15], "p1": [60, -25]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"sketch": "Pocket", "distance": 1.5, "operation": "cut"})),
    ]);
    assert!(e.contains("the pocket at (50.000, 0.000, 20.000) in a face of the sheet ends 1.500 deep, more than half the sheet's 2.000"), "{e}");
}

/// The bounding box (low corner, high corner) of a flat pattern pocket's outline.
fn pocket_box(p: &Value) -> ([f64; 2], [f64; 2]) {
    let pts: Vec<[f64; 2]> = p["outline"].as_array().into_iter().flatten().filter_map(|q| Some([q[0].as_f64()?, q[1].as_f64()?])).collect();
    let lo = [pts.iter().map(|q| q[0]).fold(f64::MAX, f64::min), pts.iter().map(|q| q[1]).fold(f64::MAX, f64::min)];
    let hi = [pts.iter().map(|q| q[0]).fold(f64::MIN, f64::max), pts.iter().map(|q| q[1]).fold(f64::MIN, f64::max)];
    (lo, hi)
}

/// A bent part as another system models it, its bends with their radius, with what its faces
/// carry: a channel 80 wide with legs 30 high (outside, to the sharp corners), 2 thick, 60 long,
/// bent R 2 inside and R 4 outside, with a 6 mm hole 18 up its right leg, a label 20 x 10
/// engraved 0.5 deep into the outside of its web and one 10 x 20 into the inside of its left leg.
/// Converted from the outside of the web by its outer cylinders under a rule of the model's
/// radius, the sheet is the model again: the hole is a cut-out where the bend allowance puts it,
/// and each label is a pocket on its own skin of the sheet.
#[test]
fn a_channel_with_its_bends_modelled_keeps_its_hole_and_engravings_on_either_skin() {
    let (t, k, r) = (2.0, 0.4, 2.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    let corners = bent_section(&mut s, &[(0.0, 30.0), (0.0, 0.0), (80.0, 0.0), (80.0, 30.0)], t, r, 60.0);
    run(&mut s, "solid.hole", json!({"position": [80, 18, 20], "diameter": 6, "depth": 3}));
    // The web's label from its outside (y = 0) in, the leg's from its inside (x = 2) out.
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "WebLabel"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, -35], "p1": [50, -45]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "WebLabel", "distance": 0.5, "operation": "cut"}));
    run(&mut s, "sketch.create", json!({"plane": "YZ", "offset": 2, "name": "LegLabel"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [12, 10], "p1": [22, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "LegLabel", "distance": 0.5, "direction": "negative", "operation": "cut"}));
    // Two flat legs, the web and two quarter rounds on the neutral radius R + T/2, less the hole
    // and the labels.
    let cut = 9.0 * PI * t + 2.0 * 200.0 * 0.5;
    let solid = volume(&mut s);
    let model = t * 60.0 * (2.0 * (30.0 - (r + t)) + (80.0 - 2.0 * (r + t)) + PI * (r + t / 2.0)) - cut;
    assert!((solid - model).abs() < 2e-3 * model, "{solid} vs {model}");
    run(&mut s, "sheet.convert", json!({"body": "Section", "face": [65, 0, 10], "bends": [corners[0].0, corners[1].0]}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 2 && rows.iter().all(|row| row.0 == 90 && row.1 == "up" && (row.2 - 60.0).abs() < 1e-6), "{rows:?}");
    // 30 + 80 + 30 less two deductions; the sheet is the solid again.
    let bd = deduction(FRAC_PI_2, r, t, k);
    let across = 140.0 - 2.0 * bd;
    assert!(same_size(flat_size(&mut s), (across, 60.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    let v = volume(&mut s);
    assert!((v - solid).abs() < 1e-3 * solid, "{v} vs {solid}");
    // The labels stay in their skins: the web's in its outside, the leg's in its inside.
    assert!(!contains(&s, [40.0, 0.25, 40.0]) && contains(&s, [40.0, 0.75, 40.0]));
    assert!(!contains(&s, [1.75, 17.0, 20.0]) && contains(&s, [1.25, 17.0, 20.0]));
    // The flat: the hole, and a pocket per label, 0.5 deep from the web's skin (z 0) and from the
    // other one (z T). Along the outside from the web label's middle (40 in, 40 along) to the
    // hole's: 40 to the sharp and 18 up the leg, less a deduction, and 20 back along; to the leg
    // label's middle, 40 and 17 the other way.
    let circles = flat_circles(&mut s);
    assert!(circles.len() == 1 && (circles[0].0 - 3.0).abs() < 1e-9, "{circles:?}");
    let f = flat(&mut s);
    let pockets = f["pockets"].as_array().cloned().unwrap_or_default();
    assert_eq!(pockets.len(), 2, "{f}");
    let from = |p: &Value, z0: f64, z1: f64| {
        (p["z0_mm"].as_f64().unwrap_or(f64::NAN) - z0).abs() < 1e-9 && (p["z1_mm"].as_f64().unwrap_or(f64::NAN) - z1).abs() < 1e-9
    };
    let web = pockets.iter().find(|p| from(p, 0.0, 0.5)).expect("the web's label, from z 0");
    let leg = pockets.iter().find(|p| from(p, 1.5, 2.0)).expect("the leg's label, from z T");
    let middle = |p: &Value| {
        let (lo, hi) = pocket_box(p);
        ([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], (hi[0] - lo[0], hi[1] - lo[1]))
    };
    let ((w, w_size), (l, l_size), h) = (middle(web), middle(leg), circles[0].1);
    assert!(same_size(w_size, (20.0, 10.0), 1e-6) && same_size(l_size, (10.0, 20.0), 1e-6), "{w_size:?} {l_size:?}");
    let (to_hole, to_leg) = ([h[0] - w[0], h[1] - w[1]], [l[0] - w[0], l[1] - w[1]]);
    assert!(same_size((to_hole[0].abs(), to_hole[1].abs()), (58.0 - bd, 20.0), 1e-6), "{to_hole:?}");
    assert!(same_size((to_leg[0].abs(), to_leg[1].abs()), (57.0 - bd, 20.0), 1e-6), "{to_leg:?}");
    assert!(to_hole[0] * to_leg[0] + to_hole[1] * to_leg[1] < 0.0, "{to_hole:?} {to_leg:?}");
    // The DXF: the hole a CIRCLE, each label on the pocket layer of its depth.
    assert_eq!(dxf_entities(&mut s).2, 1);
    let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap_or_default().to_string();
    assert_eq!((lines_on(&dxf, "POCKET_BOTTOM_Z0.000000_TO_0.500000_MM"), lines_on(&dxf, "POCKET_TOP_Z1.500000_TO_2.000000_MM")), (4, 4));
    // Unfolded and folded again it keeps them.
    run(&mut s, "sheet.unfold", json!({}));
    let want = t * across * 60.0 - cut;
    assert!((volume(&mut s) - want).abs() < 1e-4 * want, "{} vs {want}", volume(&mut s));
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - v).abs() < 1e-6 * v);
}

/// Chamfer bends beside holes: [`chamfered_channel`] with a 6 mm hole in its web 20 from the
/// left sharp corner and one 18 up its right leg, the chamfers picked in their middles. The holes
/// land from the virtual sharps, where the faces were taken out to.
#[test]
fn holes_beside_chamfer_bends_land_from_the_virtual_sharps() {
    let (t, k, r) = (2.0, 0.5, 3.0);
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    chamfered_channel(&mut s);
    run(&mut s, "solid.hole", json!({"position": [20, 0, 30], "diameter": 6}));
    run(&mut s, "solid.hole", json!({"position": [40, 18, 70], "diameter": 6, "depth": 3}));
    run(&mut s, "sheet.convert", json!({"body": "Channel", "face": [20, 0, 80], "bends": [[2, 2, 50], [38, 2, 50]]}));
    let bd = deduction(FRAC_PI_2, r, t, k);
    let across = 100.0 - 2.0 * bd;
    assert!(same_size(flat_size(&mut s), (across, 100.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    // From the web's hole: 20 to the right sharp, 18 up the leg, less a deduction; 40 along.
    let circles = flat_circles(&mut s);
    assert!(circles.len() == 2 && circles.iter().all(|(radius, _)| (radius - 3.0).abs() < 1e-9), "{circles:?}");
    let d = [circles[1].1[0] - circles[0].1[0], circles[1].1[1] - circles[0].1[1]];
    assert!(same_size((d[0].abs(), d[1].abs()), (38.0 - bd, 40.0), 1e-6), "{d:?}");
    // Three flat strips and two quarter rounds on the neutral radius, less the holes.
    let expect = t * 100.0 * (2.0 * (30.0 - (r + t)) + (40.0 - 2.0 * (r + t)) + PI * (r + t / 2.0)) - 2.0 * t * 9.0 * PI;
    assert!((volume(&mut s) - expect).abs() < 2e-3 * expect, "{} vs {expect}", volume(&mut s));
}

/// The rule of [`rule`] with its corner relief: `shape` 8 mm across.
fn relief_rule(s: &mut Session, shape: &str) {
    rule(s, "Test", 2.0, 0.44, 2.0, 0.5);
    run(s, "sheet.manage_rules", json!({"name": "Test", "corner_relief_shape": shape, "corner_relief": "8 mm"}));
}

/// The flat outline's corners, from the low corner of their box.
fn outline_corners(s: &mut Session) -> Vec<[f64; 2]> {
    let f = flat(s);
    let loops = f["outline"].as_array().cloned().unwrap_or_default();
    let pts: Vec<[f64; 2]> =
        loops.iter().flat_map(|l| l.as_array().cloned().unwrap_or_default()).filter_map(|q| Some([q[0].as_f64()?, q[1].as_f64()?])).collect();
    let lo = [pts.iter().map(|q| q[0]).fold(f64::MAX, f64::min), pts.iter().map(|q| q[1]).fold(f64::MAX, f64::min)];
    pts.iter().map(|q| [q[0] - lo[0], q[1] - lo[1]]).collect()
}

/// Whether the flat outline has a corner at each of `want` (from the low corner of its box), the
/// flat as it lies or turned in its box: half round, or seen from its other face. (The flat frame
/// runs along a longest edge of the fixed face, which may be either of two, and from a shell's
/// inner skin it is seen from the other side.)
fn outline_has(s: &mut Session, want: &[[f64; 2]]) -> bool {
    let pts = outline_corners(s);
    let hi = [pts.iter().map(|q| q[0]).fold(0.0, f64::max), pts.iter().map(|q| q[1]).fold(0.0, f64::max)];
    let turned = |q: [f64; 2], (fx, fy): (bool, bool)| [if fx { hi[0] - q[0] } else { q[0] }, if fy { hi[1] - q[1] } else { q[1] }];
    [(false, false), (true, true), (true, false), (false, true)].iter().any(|&turn| {
        want.iter().all(|w| {
            pts.iter().any(|q| {
                let p = turned(*q, turn);
                (p[0] - w[0]).abs() < 1e-6 && (p[1] - w[1]).abs() < 1e-6
            })
        })
    })
}

/// The corners of a converted tray are a flanged tray's: closed with the rule's gap and given the
/// rule's corner relief, in the folded part and the flat pattern. The shelled tray of [`tray`] and
/// the same tray made from a base and four 30 mm edge flanges, T 2, R 2, K 0.44, gap 0.5, a relief
/// 8 mm across: the bends meet at the base's corners, R + T = 4 in from the sharps, and are
/// BA = π/2 (R + K T) = 1.44 π wide in the flat. A notch leaves them there (92 and 52 long); a
/// round or a square relief takes 4 off each bend at each corner and r²·π/4 or r² (r = 4) of the
/// base; a tear runs each bend on to its corner and cuts its end from there to where its wall's
/// closed end starts, g/√2 + T from the sharp: R − g/√2 longer. The conversion used to bend every
/// corner as a notch, whatever the rule said.
#[test]
fn a_converted_trays_corners_are_closed_and_relieved_as_a_flanged_trays_are() {
    let (t, r, k, gap) = (2.0, 2.0, 0.44, 0.5);
    let ba = FRAC_PI_2 * (r + k * t);
    let grow = r - gap / SQRT_2;
    // (bend lengths, flat area taken off the notch's at each corner)
    let cases = [
        ("notch", (92.0, 52.0), 0.0),
        ("round", (84.0, 44.0), 4.0 * PI + 8.0 * ba),
        ("rectangular", (84.0, 44.0), 16.0 + 8.0 * ba),
        ("tear", (92.0 + grow, 52.0 + grow), -grow * ba),
    ];
    let cut = t + gap / SQRT_2;
    let walls = 26.0 * 2.0 * ((100.0 - 2.0 * cut) + (60.0 - 2.0 * cut));
    let flat_notch = t * (92.0 * 52.0 + ba * 2.0 * (92.0 + 52.0) + walls);
    for (shape, (long, short), taken) in cases {
        let mut s = Session::default();
        relief_rule(&mut s, shape);
        tray(&mut s, t);
        let out = run(&mut s, "sheet.convert", json!({"body": "Tray", "face": [50, 30, 0], "bends": BOTTOM_EDGES}));
        assert!(out["warning"].is_null(), "{shape}: {out}");
        let mut lengths: Vec<f64> = bend_rows(&mut s).iter().map(|row| row.2).collect();
        lengths.sort_by(f64::total_cmp);
        let want = [short, short, long, long];
        assert!(lengths.len() == 4 && lengths.iter().zip(want).all(|(l, w)| (l - w).abs() < 1e-6), "{shape}: {lengths:?}");
        // The same tray made from flanges.
        let mut f = Session::default();
        relief_rule(&mut f, shape);
        run(&mut f, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut f, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]}));
        run(&mut f, "sketch.finish", json!({}));
        run(&mut f, "sheet.flange", json!({"sketch": "Base"}));
        run(&mut f, "sheet.flange", json!({"edges": [[50, 0, 2], [100, 30, 2], [50, 60, 2], [0, 30, 2]], "height": 30}));
        let mut flanged: Vec<f64> = bend_rows(&mut f).iter().map(|row| row.2).collect();
        flanged.sort_by(f64::total_cmp);
        assert!(lengths.iter().zip(&flanged).all(|(a, b)| (a - b).abs() < 1e-9), "{shape}: {lengths:?} vs {flanged:?}");
        // The flat patterns are congruent, corner for corner.
        let corners = outline_corners(&mut f);
        assert!(
            outline_corners(&mut s).len() == corners.len() && outline_has(&mut s, &corners),
            "{shape}: {:?} vs {corners:?}",
            outline_corners(&mut s)
        );
        let (v, vf) = (volume(&mut s), volume(&mut f));
        assert!((v - vf).abs() < 1e-6 * vf, "{shape}: {v} vs {vf}");
        run(&mut s, "sheet.unfold", json!({}));
        run(&mut f, "sheet.unfold", json!({}));
        let (flat_v, flat_f) = (volume(&mut s), volume(&mut f));
        assert!((flat_v - flat_f).abs() < 1e-6 * flat_f, "{shape}: {flat_v} vs {flat_f}");
        // The flat takes the relief off the notch's (whose walls end g/√2 + T from the sharps).
        let want = flat_notch - 4.0 * t * taken;
        assert!((flat_v - want).abs() < 0.1, "{shape}: flat {flat_v} vs {want}");
        run(&mut s, "sheet.refold", json!({}));
        assert!((volume(&mut s) - v).abs() < 1e-6 * v, "{shape}");
        // A round relief is an arc of radius 4 about each point where two bends meet: the base's
        // corners R + T in from the sharps, 92 and 52 apart.
        if shape == "round" {
            let dxf = run(&mut s, "sheet.export_dxf", json!({}))["dxf"].as_str().unwrap_or_default().to_string();
            let arcs: Vec<(solidcraft_geom::Vec2, f64)> = solidcraft_io::read_dxf(&dxf)
                .unwrap_or_default()
                .iter()
                .filter_map(|g| if let solidcraft_io::Geom2::Arc { c, a, .. } = g { Some((*c, a.dist(*c))) } else { None })
                .collect();
            assert_eq!(arcs.len(), 4, "{arcs:?}");
            let lo = solidcraft_geom::Vec2::new(
                arcs.iter().map(|(c, _)| c.x).fold(f64::MAX, f64::min),
                arcs.iter().map(|(c, _)| c.y).fold(f64::MAX, f64::min),
            );
            for (x, y) in [(0.0, 0.0), (92.0, 0.0), (92.0, 52.0), (0.0, 52.0)] {
                let at = lo + solidcraft_geom::Vec2::new(x, y);
                assert!(arcs.iter().any(|(c, r)| c.dist(at) < 1e-6 && (r - 4.0).abs() < 1e-6), "({x}, {y}): {arcs:?}");
            }
        }
    }
}

/// A plate 80 x 50, `plate` thick, with a wall 2 thick and 30 high standing on its edge y = 0
/// from `x0` to `x1`, joined (with a 2 mm plate, a shell).
fn bracket(s: &mut Session, x0: f64, x1: f64, plate: f64) {
    run(s, "solid.box", json!({"length": 80, "width": 50, "height": plate, "body_name": "Bracket"}));
    run(s, "solid.box", json!({"length": x1 - x0, "width": 2, "height": 30, "corner": [x0, 0, 0], "operation": "join", "targets": ["Bracket"]}));
}

/// A wall standing on part of an edge (an L-bracket whose flange covers part of the plate's edge,
/// a tab): the plate's edge face and the wall's outside are one face, the plate's edge running on
/// past the wall a strip T high. Its bend ends where the wall does, and the rule's bend relief
/// (2 wide, from 1 past the bend line through the plate's edge) is cut into the plate beside each
/// end of the bend that has more plate beside it, as for an edge flange. The strip, lower than the
/// bend's setback R + T = 4, was used up by it and the conversion refused; from the inner skin the
/// bend ended against the plate without a relief. T 2, R 2, K 0.44: BA = 1.44 π, BD = 8 − BA.
#[test]
fn a_bend_ends_where_its_wall_ends_with_the_rules_bend_relief_beside_it() {
    let (t, r, k) = (2.0, 2.0, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    let bd = deduction(FRAC_PI_2, r, t, k);
    // Folded and flat: the plate less the bend's setback under the wall and the reliefs (2 x 5
    // each), the wall past the bend (30 − 4 high), and the bend on its neutral radius or as its
    // allowance.
    let parts = |wall: f64, reliefs: f64| {
        let flat = 80.0 * 50.0 - 4.0 * wall - 10.0 * reliefs + 26.0 * wall;
        (t * flat + FRAC_PI_2 * t * (r + t / 2.0) * wall, t * (flat + ba * wall))
    };
    // From the flat's low corner (the wall's far edge, 26 + BA − 4 past the plate's edge).
    let edge = 26.0 + ba - 4.0;
    // The wall in the middle, picked from the outside or the inside: the same sheet, the reliefs
    // x 18 … 20 and 60 … 62, through the plate's edge to 1 past the bend line.
    for (face, bend, way) in [([40.0, 25.0, 0.0], [40.0, 0.0, 0.0], "up"), ([40.0, 25.0, 2.0], [40.0, 2.0, 2.0], "down")] {
        let mut s = Session::default();
        rule(&mut s, "Test", t, k, r, 0.5);
        bracket(&mut s, 20.0, 60.0, 2.0);
        let out = run(&mut s, "sheet.convert", json!({"body": "Bracket", "face": face, "bends": [bend]}));
        assert!(out["warning"].is_null(), "{way}: {out}");
        let rows = bend_rows(&mut s);
        assert!(rows.len() == 1 && rows[0].0 == 90 && rows[0].1 == way && (rows[0].2 - 40.0).abs() < 1e-6, "{way}: {rows:?}");
        assert!(same_size(flat_size(&mut s), (80.0, 80.0 - bd), 1e-6), "{way}: {:?}", flat_size(&mut s));
        let reliefs = [[18.0, edge], [18.0, edge + 5.0], [20.0, edge + 5.0], [60.0, edge + 5.0], [62.0, edge + 5.0], [62.0, edge]];
        assert!(outline_has(&mut s, &reliefs), "{way}: {:?}", outline_corners(&mut s));
        let (folded, flat) = parts(40.0, 2.0);
        let v = volume(&mut s);
        assert!((v - folded).abs() < 1e-4 * folded, "{way}: {v} vs {folded}");
        run(&mut s, "sheet.unfold", json!({}));
        assert!((volume(&mut s) - flat).abs() < 1e-6 * flat, "{way}: {} vs {flat}", volume(&mut s));
        run(&mut s, "sheet.refold", json!({}));
        assert!((volume(&mut s) - v).abs() < 1e-6 * v, "{way}");
    }
    // A wall at the plate's end: its bend runs from the plate's corner, one relief at its end.
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    bracket(&mut s, 0.0, 40.0, 2.0);
    run(&mut s, "sheet.convert", json!({"body": "Bracket", "face": [40, 25, 0], "bends": [[20, 0, 0]]}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 1 && (rows[0].2 - 40.0).abs() < 1e-6, "{rows:?}");
    assert!(outline_has(&mut s, &[[0.0, 0.0], [40.0, 0.0], [40.0, edge + 5.0], [42.0, edge + 5.0], [42.0, edge]]), "{:?}", outline_corners(&mut s));
    let (folded, _) = parts(40.0, 1.0);
    assert!((volume(&mut s) - folded).abs() < 1e-4 * folded, "{} vs {folded}", volume(&mut s));
    // A strip taller than the setback (a 5 mm plate, the rule's T 2 taken into the block) stays a
    // low wall of the sheet beside the wall, as before: one bend the plate's length, no relief.
    let mut s = Session::default();
    rule(&mut s, "Test", t, k, r, 0.5);
    bracket(&mut s, 20.0, 60.0, 5.0);
    run(&mut s, "sheet.convert", json!({"body": "Bracket", "face": [40, 25, 0], "bends": [[40, 0, 0]]}));
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 1 && (rows[0].2 - 80.0).abs() < 1e-6, "{rows:?}");
    let low = t * (80.0 * 46.0 + 80.0 * 1.0 + 40.0 * 25.0) + FRAC_PI_2 * t * (r + t / 2.0) * 80.0;
    assert!((volume(&mut s) - low).abs() < 1e-4 * low, "{} vs {low}", volume(&mut s));
}

/// Both together: a plate 100 x 60 x 2 with a wall along its whole edge y = 0 and one on its edge
/// x = 100 from y = 0 to 30 only, a round corner relief 8 across. The walls meet at the corner,
/// closed with the gap (each ends g/√2 + T from the other's outside) and relieved there; the
/// shorter one's bend ends at y = 30 with a bend relief beside it. In the flat from the plate's
/// corner: the plate from y = 4 and up to x = 96 beside the short wall, 100 past it; bends 96 − 4
/// and 26 − 4 long; the bend relief x 95 … 100, y 30 … 32.
#[test]
fn a_wall_on_part_of_an_edge_meets_a_wall_at_a_relieved_corner_and_is_relieved_at_its_end() {
    let (t, r, k, gap) = (2.0, 2.0, 0.44, 0.5);
    let ba = FRAC_PI_2 * (r + k * t);
    let mut s = Session::default();
    relief_rule(&mut s, "round");
    run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 2, "body_name": "Part"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 2, "height": 30, "operation": "join", "targets": ["Part"]}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 30, "height": 30, "corner": [98, 0, 0], "operation": "join", "targets": ["Part"]}));
    let out = run(&mut s, "sheet.convert", json!({"body": "Part", "face": [50, 30, 0], "bends": [[50, 0, 0], [100, 15, 0]]}));
    assert!(out["warning"].is_null(), "{out}");
    let mut lengths: Vec<f64> = bend_rows(&mut s).iter().map(|row| row.2).collect();
    lengths.sort_by(f64::total_cmp);
    assert!(lengths.len() == 2 && (lengths[0] - 22.0).abs() < 1e-6 && (lengths[1] - 92.0).abs() < 1e-6, "{lengths:?}");
    // The plate less the setbacks, the round relief and the bend relief; the walls 26 high, each
    // from the corner's closed end to its own end.
    let cut = t + gap / SQRT_2;
    let base = 100.0 * 56.0 - 4.0 * 26.0 - 4.0 * PI - 10.0;
    let walls = 26.0 * ((100.0 - cut) + (30.0 - cut));
    let folded = t * (base + walls) + FRAC_PI_2 * t * (r + t / 2.0) * 114.0;
    let v = volume(&mut s);
    assert!((v - folded).abs() < 1e-4 * folded, "{v} vs {folded}");
    // From the flat's low corner (the long wall's far edge, 26 + BA − 4 past y = 0): the bend
    // relief's corners.
    let edge = 26.0 + ba - 4.0;
    assert!(outline_has(&mut s, &[[95.0, edge + 30.0], [95.0, edge + 32.0], [100.0, edge + 32.0]]), "{:?}", outline_corners(&mut s));
    run(&mut s, "sheet.unfold", json!({}));
    let flat = t * (base + walls + ba * 114.0);
    assert!((volume(&mut s) - flat).abs() < 0.1, "{} vs {flat}", volume(&mut s));
    run(&mut s, "sheet.refold", json!({}));
    assert!((volume(&mut s) - v).abs() < 1e-6 * v);
}
