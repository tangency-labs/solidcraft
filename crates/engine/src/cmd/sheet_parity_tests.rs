//! What `main` tested that this branch had no test for (#9245's parity review, #6748): a chamfer's
//! edges picked as Convert's bends, flanges on both sides of a slot cut into a wall (#7278), the
//! perforation physical properties not worked out yet, a saved rule with a bend radius of 0, the
//! side a fold's `stays` keeps put, and corner reliefs left out with a note. Each expected number
//! is worked by hand beside its assert.

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

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn flat(s: &mut Session) -> Value {
    run(s, "sheet.flat_pattern", json!({}))
}

fn bbox(s: &mut Session) -> Value {
    run(s, "inspect.measure", json!({}))["bodies"][0]["bbox"].clone()
}

/// Whether the session's first body holds `p` inside its material.
fn contains(s: &Session, p: [f64; 3]) -> bool {
    s.model
        .state()
        .bodies
        .first()
        .is_some_and(|b| b.body.classify_point(solidcore_record::Vec3::new(p[0], p[1], p[2])).unwrap() == solidcraft_kernel::PointClass::Inside)
}

/// A w x h base flange on XY.
fn base(s: &mut Session, w: f64, h: f64) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [w, h]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sheet.flange", json!({"sketch": "Base"}));
}

fn no_errors_or_warnings(s: &Session) -> bool {
    s.model.results.iter().all(|r| r.error.is_none() && r.warning.is_none())
}

fn no_errors(s: &Session) -> bool {
    s.model.results.iter().all(|r| r.error.is_none())
}

fn polygon_area(p: &[(f64, f64)]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1).sum::<f64>().abs() / 2.0
}

/// The flat pattern's area: its outline loops less its cut-outs.
fn flat_area(f: &Value) -> f64 {
    let loops = |k: &str| -> f64 {
        let all = f[k].as_array().cloned().unwrap_or_default();
        all.iter()
            .map(|l| {
                let pts: Vec<(f64, f64)> = l
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|q| (q[0].as_f64().unwrap_or(f64::NAN), q[1].as_f64().unwrap_or(f64::NAN)))
                    .collect();
                polygon_area(&pts)
            })
            .sum()
    };
    loops("outline") - loops("cutouts")
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

/// A channel 40 wide with walls 30 high (outside sizes), 2 thick and 100 long, its two corners
/// drawn as chamfers, as a model without bend radii draws a bend: 4 along each outer face and 2
/// along each inner one (`main`'s `chamfered_channel`).
fn chamfered_channel(s: &mut Session) {
    let pts = [[0, 30], [0, 4], [4, 0], [36, 0], [40, 4], [40, 30], [38, 30], [38, 4], [36, 2], [4, 2], [2, 4], [2, 30]];
    run(s, "sketch.create", json!({"plane": "XY", "name": "Channel"}));
    run(s, "sketch.line", json!({"points": pts, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": "Channel", "distance": 100, "body_name": "Channel"}));
}

/// `main`'s `a_chamfer_picked_in_its_middle_is_one_bend_at_the_virtual_sharp`, the part of it that
/// picks a chamfer's two edges: four 45° bends, the chamfers narrow faces of the sheet (its middle
/// picked is refused here, `a_bend_picked_on_a_chamfer_is_refused`). Convert takes the bottom's
/// outer face and every face a bend joins to it (the two chamfers, then the two walls), their
/// material inside, at the rule's T 2, R 3, K 0.5 (#6748).
///
/// Each bend turns 45° toward the material at a sharp edge of the outer faces: it is set back
/// s0 = (R + T) tan 22.5° = 5 (√2 − 1) = 2.071068 from it on each face, BA = (π/4)(R + K T) = π.
/// Between the setbacks the outer faces leave strips of 26 − s0 (each wall, 30 less the chamfer's
/// 4), 4√2 − 2 s0 (each chamfer) and 32 − 2 s0 (the bottom): 124 − 32√2 = 78.745166 in all, and the
/// flat is that with four BA, 124 − 32√2 + 4π = 91.311537 across, 100 long. K = 0.5 keeps the
/// volume: each bend's ring sector holds (π/8)((R + T)² − R²) = 2π = T·BA per mm, so the part is
/// T × 100 × 91.311537 = 18262.307324 mm³. Its outer faces stay the body's: the walls at x 0 and
/// 40 (above their bends' tangent lines, y 6.071 … 30), the bottom at y 0, the ends at z 0 and 100,
/// and each bend's outer round lies inside the corner its faces make: the box is the channel's,
/// 40 x 30 x 100. (`main` asserted only the four 45° rows; it had no flat for this pick.)
#[test]
fn a_chamfers_edges_picked_as_bends_are_four_45_degree_bends() {
    let (t, k, r) = (2.0, 0.5, 3.0);
    let rule = json!({"name": "Test", "thickness": "2 mm", "k_factor": "0.5", "bend_radius": "3 mm", "gap": "0.5 mm", "active": true});
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", rule.clone());
    chamfered_channel(&mut s);
    let edges = json!([[0, 4, 50], [4, 0, 50], [36, 0, 50], [40, 4, 50]]);
    run(&mut s, "sheet.convert", json!({"body": "Channel", "face": [20, 0, 50], "bends": edges}));
    assert!(no_errors(&s), "{:?}", s.model.results);
    // Four 45° bends the channel's length, turning toward the material behind the face picked:
    // down, as the flat is read from the sheet's surface.
    let rows = bend_rows(&mut s);
    assert!(rows.len() == 4 && rows.iter().all(|row| row.0 == 45 && row.1 == "down" && (row.2 - 100.0).abs() < 1e-6), "{rows:?}");
    let s0 = (r + t) * (PI / 8.0).tan();
    assert!((s0 - 5.0 * (SQRT_2 - 1.0)).abs() < 1e-12);
    let ba = PI / 4.0 * (r + k * t);
    let strips = 2.0 * (26.0 - s0) + 2.0 * (4.0 * SQRT_2 - 2.0 * s0) + (32.0 - 2.0 * s0);
    assert!((strips - (124.0 - 32.0 * SQRT_2)).abs() < 1e-12);
    let across = strips + 4.0 * ba;
    assert!((across - 91.311537).abs() < 1e-6);
    assert!(same_size(flat_size(&mut s), (across, 100.0), 1e-6), "{:?} vs {across}", flat_size(&mut s));
    let want = t * 100.0 * across;
    assert!((want - 18262.307324).abs() < 1e-6);
    assert!(rel(volume(&mut s), want) < 1e-7, "{} vs {want}", volume(&mut s));
    let b = bbox(&mut s);
    let at = |m: &str, i: usize| b[m][i].as_f64().unwrap_or(f64::NAN);
    for (i, hi) in [40.0, 30.0, 100.0].into_iter().enumerate() {
        assert!(at("min", i).abs() < 1e-6 && (at("max", i) - hi).abs() < 1e-6, "{b}");
    }
    // The design keeps the edges picked and reopens to the same sheet.
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(no_errors(&reopened), "{:?}", reopened.model.results);
    assert_eq!(bend_rows(&mut reopened).len(), 4);
    assert!(same_size(flat_size(&mut reopened), (across, 100.0), 1e-6), "{:?}", flat_size(&mut reopened));
    // A chamfer's middle picked with one of its edges is refused (on `main`: "not both"), naming
    // the middle: a chamfer is a face, and Convert bends edges.
    let mut both = Session::default();
    run(&mut both, "sheet.manage_rules", rule);
    chamfered_channel(&mut both);
    let e = both.execute("sheet.convert", &json!({"body": "Channel", "face": [20, 0, 50], "bends": [[2, 2, 50], [4, 0, 50]]}));
    let e = e.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("the bend at (2.000, 2.000, 50.000) is on a face, not an edge"), "{e}");
    assert!(both.model.state().surface_sheets.is_empty());
}

/// #7278's `edge_flanges_on_two_edges_of_one_cut_run_along_both` (`main`'s flat-model unit test),
/// on a sheet made from surfaces. A 200 x 100 plate (T 2, R 1, K 0.5; the rule's bend relief
/// Thickness = 2 wide and half of it, 1, deep) with a 40 high wall outside on its y = 0 edge: the
/// plate keeps all of itself, the wall's bend BA = (π/2)(R + K T) = π, its leg 40 − (R + T) = 37,
/// its outer face y = −3 and its inner face y = −1, up to z = 40. A cut takes a 40 x 15 slot out
/// of the wall's top, x 80 … 120, z 25 … 40. Flanges picked together on the slot's two sides, on
/// the wall's inner face, 15 high, each run along their side, z 25 … 40, and rise toward the plate
/// (+y) to y = −3 + 15 = 12: each a bend 15 x BA and a leg 15 x 12. The first one made must not
/// leave the second moving the whole wall's edge.
///
/// Inside, each flange's outer face is the slot's side (x 78 … 80 on the left, 120 … 122 on the
/// right), and its bend starts R + T = 3 into the wall, so the wall loses a strip 3 x 15 under it.
/// Outside, each bend starts at the slot's side and lies in the slot (x 81 … 83 and 117 … 119),
/// and the wall keeps its material. Either way the wall runs on below the slot's bottom, past each
/// bend's end at z = 25, across the bend's whole band, so the rule's relief is cut from it there:
/// 2 wide (z 23 … 25), from 1 inside its tangent line (x 77 inside, 80 outside, on the left) on
/// across the zone (π) and beside the flange as far as the flange reaches (12) and 2 further:
/// 2 (π + 15) each. (`main`, the flat model, cut it inside only; outside, its zone's end met the
/// wall below on a line, which this branch's flat lets go as it does wherever a panel runs on
/// past a bend's end.) The two reliefs stay apart (inside x 76 … 94.14 and 105.86 … 124; outside
/// 79 … 97.14 and 102.86 … 121).
///
/// Flat: the plate 20000, the wall's bend 200π, its leg 200 x 37 less the slot 600, the strips
/// 2 x 15 x setback and the reliefs 4 (π + 15), and the two flanges 2 x 15 (π + 12):
/// 27100 − 30·setback + 226π, 27010 + 226π = 27720.000 inside and 27100 + 226π = 27810.000
/// outside. K = 0.5 keeps the volume: 2 x the flat.
#[test]
fn edge_flanges_on_two_edges_of_one_cut_run_along_both() {
    for (position, setback, sides) in [("outside", 0.0, [82.0, 118.0]), ("inside", 3.0, [79.0, 121.0])] {
        let mut s = Session::default();
        run(&mut s, "sheet.manage_rules", json!({"name": "R", "thickness": "2 mm", "k_factor": "0.5", "bend_radius": "1 mm", "active": true}));
        base(&mut s, 200.0, 100.0);
        run(&mut s, "sheet.flange", json!({"edges": [[100, 0, 2]], "height": 40, "angle": 90, "position": "outside"}));
        run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Slot"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [80, 25], "p1": [120, 50]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "Slot", "through_all": true, "direction": "symmetric", "operation": "cut"}));
        // The plate, the wall's bend and its leg less the slot: 2 (20000 + 200π + 7400 − 600).
        let slotted = 2.0 * (26_800.0 + 200.0 * PI);
        assert!(rel(volume(&mut s), slotted) < 1e-6, "{position}: {} vs {slotted}", volume(&mut s));
        run(&mut s, "sheet.flange", json!({"edges": [[80, -1, 32.5], [120, -1, 32.5]], "height": 15, "angle": 90, "position": position}));
        assert!(no_errors_or_warnings(&s), "{position}: {:?}", s.model.results);
        let f = flat(&mut s);
        // The wall's bend, 200 long, and the two flanges' along the slot's sides, 15 each.
        let bends = f["bends"].as_array().cloned().unwrap_or_default();
        let mut lengths: Vec<f64> = bends.iter().map(|b| b["length"].as_f64().unwrap_or(f64::NAN)).collect();
        lengths.sort_by(f64::total_cmp);
        assert!(lengths.len() == 3 && [15.0, 15.0, 200.0].iter().zip(&lengths).all(|(w, g)| (w - g).abs() < 1e-9), "{position}: {lengths:?}");
        let area = 27_100.0 - 30.0 * setback + 226.0 * PI;
        let want = 20_000.0 + 200.0 * PI + 200.0 * 37.0 - 600.0 - 2.0 * 15.0 * setback - 4.0 * (PI + 15.0) + 2.0 * 15.0 * (PI + 12.0);
        assert!((area - want).abs() < 1e-9);
        assert!(rel(flat_area(&f), area) < 1e-9, "{position}: the flat is {} mm², not {area}", flat_area(&f));
        assert!(rel(volume(&mut s), 2.0 * area) < 1e-6, "{position}: {} vs {}", volume(&mut s), 2.0 * area);
        for x in sides {
            // Each flange stands along its side of the slot, z 25 … 40, its leg y 0 … 12.
            assert!(contains(&s, [x, 6.0, 32.5]) && !contains(&s, [x, 6.0, 20.0]) && !contains(&s, [x, 6.0, 41.0]), "{position}: x {x}");
        }
        // Inside, the strips under the bends are gone (the left bend's ring, round x 77, y 0, radii
        // 1 … 3, is 3.54 from this point); outside, the wall keeps them.
        let outside = position == "outside";
        assert!(contains(&s, [79.5, -2.5, 32.5]) == outside && contains(&s, [120.5, -2.5, 32.5]) == outside, "{position}");
        // The reliefs below the slot's bottom (z 23 … 25), the wall between and below them, and the
        // wall's top beside the slot, which neither flange moved.
        for x in [85.0, 115.0] {
            assert!(!contains(&s, [x, -2.0, 24.0]) && contains(&s, [x, -2.0, 22.0]), "{position}: x {x}");
        }
        assert!(contains(&s, [100.0, -2.0, 24.0]), "{position}");
        assert!(contains(&s, [40.0, -2.0, 39.0]) && contains(&s, [160.0, -2.0, 39.0]), "{position}");
    }
}

/// What component.properties says, refused.
fn properties_refused(s: &mut Session) -> String {
    s.execute("component.properties", &json!({})).err().map(|e| e.to_string()).unwrap_or_default()
}

/// Perforation physical properties of a hole on a roll are not worked out yet: refused. The sheet
/// is one roll, a contour flange's 180° arc (outer radius 56, T 1.5, K 0.5), developed to
/// π (54.5 + 0.5 x 1.5) = 173.573 x 100. A square field of 2 mm holes at pitch 10, margin 1 (each
/// hole's centre 2 clear of the edges) lays 17 x 10 = 170 holes on it: (173.573 − 4) / 10 = 16.96
/// gives 17 along, (100 − 4) / 10 = 9.6 gives 10 across, every one on the roll.
#[test]
fn perforation_properties_of_a_hole_on_a_roll_are_refused() {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "R", "thickness": 1.5, "k_factor": 0.5, "bend_radius": 1.5, "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Trough"}));
    run(&mut s, "sketch.arc.three_point", json!({"start": [56, 0], "end": [-56, 0], "through": [0, -56], "id": "a1"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Trough", "curves": ["a1"], "distance": 100, "rule": "R", "flip": true}));
    run(&mut s, "sheet.perforate", json!({"diameter": "2", "pitch": "10", "margin": "1", "pattern": "square"}));
    assert!(no_errors(&s), "{:?}", s.model.results);
    assert_eq!(flat(&mut s)["perforations"][0]["count"], 170);
    let body = s.model.state().surface_sheets[0].body.clone();
    let e = properties_refused(&mut s);
    assert!(e.contains(&format!("not supported yet: perforation physical properties of a hole on a roll of `{body}`")), "{e}");
}

/// Perforation physical properties of a hole a bend's end runs through are not worked out yet:
/// refused. The bar of `a_bend_relief_left_out_is_reported_on_the_flange` (T 2, R 1, K 0.5, a leg
/// x 0 … 20 standing up to y 60 past the bar's top edge, y = 30), under a rule whose bend relief is
/// 0 wide, perforated first with one hole of radius 1 at (20, 28.5) (a square field r + margin =
/// 1.5 round it), then flanged 90° inside on the top edge (x 20 … 160): the bend's zone in the flat
/// is y 27 … 27 + π (setback (R + T) tan 45° = 3, BA = (π/2)(R + K T) = π), x 20 … 160, and the leg
/// runs on past its end at x = 20 with no relief to part them. The hole lies half on the zone and
/// half on the leg: the bend's end runs through it.
#[test]
fn perforation_properties_of_a_hole_across_a_bends_end_are_refused() {
    let mut s = Session::default();
    run(
        &mut s,
        "sheet.manage_rules",
        json!({"name": "AL2", "thickness": "2 mm", "k_factor": 0.5, "bend_radius": "1 mm", "relief_width": "0 mm", "active": true}),
    );
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Bar"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [160, 0], [160, 30], [20, 30], [20, 60], [0, 60]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Bar", "body_name": "Bracket"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Spot"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [18.5, 27], "p1": [21.5, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.perforate", json!({"diameter": 2, "pitch": 30, "margin": 0.5, "pattern": "square", "sketch": "Spot"}));
    run(&mut s, "sheet.flange", json!({"edges": [[90, 30, 2]], "height": 15, "angle": 90}));
    let w = s.model.results.last().and_then(|r| r.warning.clone()).unwrap_or_default();
    assert!(w.contains("no bend relief at (20.000, 27.000, 2.000)"), "{w}");
    assert!(no_errors(&s), "{:?}", s.model.results);
    assert_eq!(flat(&mut s)["perforations"][0]["count"], 1);
    let e = properties_refused(&mut s);
    assert!(e.contains("not supported yet: a perforation hole across the end of a bend of `Bracket`"), "{e}");
}

/// Perforation physical properties of a hole two bends reach are not worked out yet: refused. A
/// 60 x 40 x 0.8 plate (R 0.8, K 0.44: BA = (π/2)(0.8 + 0.44 x 0.8) = 1.809557) perforated first
/// with one hole of radius 4 at (23, 20) (a square field r + margin = 4.5 round it), then folded
/// 90° on x = 20 and on x = 26, its right side staying: the zones are x 20 ± BA/2 = 19.095 …
/// 20.905 and 26 ± BA/2 = 25.095 … 26.905, and the hole, x 19 … 27, reaches into both.
#[test]
fn perforation_properties_of_a_hole_two_bends_reach_are_refused() {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "Panel 0.8", "thickness": "0.8", "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Blank"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [60, 40]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Blank", "body_name": "Panel"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Spot"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [18.5, 15.5], "p1": [27.5, 24.5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.perforate", json!({"diameter": 8, "pitch": 30, "margin": 0.5, "pattern": "square", "sketch": "Spot"}));
    for x in [20.0, 26.0] {
        run(&mut s, "sheet.fold", json!({"points": [[x, 0, 0], [x, 40, 0]], "angle": "90 deg", "position": "centerline", "fixed": [50, 20, 0]}));
    }
    assert!(no_errors(&s), "{:?}", s.model.results);
    assert_eq!(flat(&mut s)["perforations"][0]["count"], 1);
    let e = properties_refused(&mut s);
    assert!(e.contains("not supported yet: a perforation hole two bends reach on `Panel`"), "{e}");
}

/// A design saved with a rule whose bend radius is 0 (`main`, #15288, built such a bend as a
/// sharp outer face) is refused as it rebuilds: the base flange that reads the rule says why, with
/// the words every other place a radius of 0 is refused with (`sheet::KNIFE_EDGE`), and builds no
/// sheet, until the radius is set. Set to 1 mm, the base is the plate again: 100 x 60 x 2 = 12000
/// mm³.
#[test]
fn a_saved_rule_with_a_bend_radius_of_0_is_refused_as_the_design_rebuilds() {
    let knife =
        "a bend radius of 0 can't be made: no material bends to a knife edge, so give the smallest inside radius your material and tooling form";
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "R", "thickness": "2 mm", "k_factor": "0.5", "bend_radius": "1 mm", "active": true}));
    base(&mut s, 100.0, 60.0);
    assert!(no_errors_or_warnings(&s), "{:?}", s.model.results);
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    assert_eq!(saved["sheet"]["rules"][0]["name"], "R");
    saved["sheet"]["rules"][0]["bend_radius"] = json!("0 mm");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
    let errors: Vec<String> = reopened.model.results.iter().filter_map(|r| r.error.clone()).collect();
    assert!(errors.len() == 1 && errors[0].contains(knife), "{errors:?}");
    assert!(reopened.model.state().surface_sheets.is_empty());
    run(&mut reopened, "sheet.manage_rules", json!({"name": "R", "bend_radius": "1 mm"}));
    assert!(no_errors_or_warnings(&reopened), "{:?}", reopened.model.results);
    assert!(rel(volume(&mut reopened), 12_000.0) < 1e-9, "{}", volume(&mut reopened));
}

/// The side the last feature, a fold, keeps put.
fn fold_stays(s: &Session) -> Option<solidcraft_doc::FoldSide> {
    match s.doc.features.last().map(|f| &f.kind) {
        Some(solidcraft_doc::FeatureKind::SheetFold { stays, .. }) => *stays,
        _ => None,
    }
}

/// A fold's `stays` keeps that side of its line as drawn put, seen from the sheet's surface (a base
/// flange's top, so drawn along +y the line's left is x < 40, drawn along −y it is x > 40). The
/// 100 x 60 plate under the default rule (T 2.5, R 2.5, K 0.44), folded 90° up on x = 40 (the line
/// on its top face, the zone centered on it): BA = (π/2)(R + K T) = 1.8π = 5.654867, the zone
/// x 37.172567 … 42.827433, its axis T + R = 5 up.
/// - The part x < 40 stays, flat out to x = 0: the other turns up about x = 40 − BA/2, its outer
///   face at x = 40 − BA/2 + R + T = 42.172567 and its far edge at z = T + R + 60 − BA/2 =
///   62.172567 (its leg 100 − 40 − BA/2 long).
/// - The part x > 40 stays, flat out to x = 100: the other turns up about x = 40 + BA/2, its outer
///   face at x = 40 + BA/2 − R − T = 37.827433 and its far edge at z = T + R + 40 − BA/2 =
///   42.172567.
#[test]
fn a_folds_stays_keeps_that_side_of_its_line_as_drawn_put() {
    use solidcraft_doc::FoldSide::{Left, Right};
    let (t, r, k) = (2.5, 2.5, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    assert!((ba - 5.654867).abs() < 1e-6);
    let up = [[40.0, 0.0, 2.5], [40.0, 60.0, 2.5]];
    let down = [[40.0, 60.0, 2.5], [40.0, 0.0, 2.5]];
    let cases = [(up, "left", Left, true), (up, "right", Right, false), (down, "left", Left, false), (down, "right", Right, true)];
    for (points, stays, side, low_stays) in cases {
        let what = format!("drawn from y {} with {stays}", points[0][1]);
        let mut s = Session::default();
        base(&mut s, 100.0, 60.0);
        run(&mut s, "sheet.fold", json!({"points": points, "stays": stays}));
        assert!(no_errors_or_warnings(&s), "{what}: {:?}", s.model.results);
        assert_eq!(fold_stays(&s), Some(side), "{what}");
        let (lo, hi, top) = if low_stays {
            (0.0, 40.0 - ba / 2.0 + r + t, t + r + 60.0 - ba / 2.0)
        } else {
            (40.0 + ba / 2.0 - r - t, 100.0, t + r + 40.0 - ba / 2.0)
        };
        let b = bbox(&mut s);
        let at = |m: &str, i: usize| b[m][i].as_f64().unwrap_or(f64::NAN);
        assert!((at("min", 0) - lo).abs() < 1e-6 && (at("max", 0) - hi).abs() < 1e-6 && (at("max", 2) - top).abs() < 1e-6, "{what}: {b}");
        // The part that stays is where it was; the other is gone from there.
        assert!(contains(&s, [10.0, 30.0, 1.25]) == low_stays && contains(&s, [90.0, 30.0, 1.25]) != low_stays, "{what}");
    }
}

/// 2 mm aluminium (R 1, K 0.5, gap 0.5; `rule` adds to its settings), a 120 x 90 base, and a flange
/// 38.6 high at `deg` on each of its four edges (`main`'s `tray`).
fn alu_tray(s: &mut Session, deg: f64, rule: Value) {
    let mut r = json!({"name": "AL2", "thickness": "2 mm", "k_factor": 0.5, "bend_radius": "1 mm", "gap": "0.5 mm", "active": true});
    if let (Some(r), Some(extra)) = (r.as_object_mut(), rule.as_object()) {
        r.extend(extra.clone());
    }
    run(s, "sheet.manage_rules", r);
    base(s, 120.0, 90.0);
    run(s, "sheet.flange", json!({"edges": [[60, 0, 2], [120, 45, 2], [60, 90, 2], [0, 45, 2]], "height": 38.6, "angle": format!("{deg} deg")}));
}

/// `main`'s `a_relief_that_does_not_fit_is_left_out_with_a_note`, its tray: a round corner relief
/// that would take a whole bend away, or is too small to cut, is left out at each corner with a
/// note, and the part is the notched one. At 72° each bend is set back s0 = (R + T) tan 36° =
/// 2.179628 from its edge, so the bends between the corners run 90 − 2 s0 = 85.640745 (the short
/// edges) and 120 − 2 s0 = 115.640745. A round 200 across reaches 100 along each bend from its
/// corner, more than the short bend at every corner has; one 0 or −8 across is below the 0.01 mm a
/// relief must be. (`main`'s 100 mm relief, 50 along each bend from each corner, fits each corner
/// here: see the report of #9245's parity review.)
#[test]
fn a_corner_relief_that_does_not_fit_or_is_too_small_is_left_out_with_a_note() {
    let s0 = 3.0 * 36f64.to_radians().tan();
    assert!((90.0 - 2.0 * s0 - 85.640745).abs() < 1e-6 && 200.0 / 2.0 > 90.0 - 2.0 * s0);
    let mut notched = Session::default();
    alu_tray(&mut notched, 72.0, json!({}));
    assert!(no_errors_or_warnings(&notched), "{:?}", notched.model.results);
    let v0 = volume(&mut notched);
    for (size, why) in [
        ("200 mm", "a relief 200.000 mm across would take a whole bend away"),
        ("0 mm", "the relief size 0.0000 mm is below 0.01 mm"),
        ("-8 mm", "the relief size -8.0000 mm is below 0.01 mm"),
    ] {
        let mut s = Session::default();
        alu_tray(&mut s, 72.0, json!({"corner_relief_shape": "round", "corner_relief": size}));
        assert!(no_errors(&s), "{size}: {:?}", s.model.results);
        let w = s.model.results.last().and_then(|r| r.warning.clone()).unwrap_or_default();
        assert_eq!(w.matches("no corner relief at (").count(), 4, "{size}: {w}");
        assert_eq!(w.matches(why).count(), 4, "{size}: {w}");
        assert!(rel(volume(&mut s), v0) < 1e-12, "{size}: {} vs {v0}", volume(&mut s));
    }
}
