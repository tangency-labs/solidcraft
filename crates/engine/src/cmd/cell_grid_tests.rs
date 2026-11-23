use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn volume(s: &Session) -> f64 {
    let st = s.model.state();
    st.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum()
}

/// A `side` x `side` plate, `thick` thick, and a sketch of the same square on its bottom face.
fn plate(side: f64, thick: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": side, "width": side, "height": thick, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [side, side]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn cut(s: &mut Session, extra: Value) -> Result<Value, crate::EngineError> {
    let mut p = json!({"sketch": "Field", "shape": "square", "size": 10, "web": 2, "clip": "whole", "anchor": [6, 6, 0]});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    s.execute("solid.cell_grid", &p)
}

#[test]
fn square_cells_cut_to_a_depth_remove_exactly_their_volume() {
    // 62 x 62 x 3 plate; 10 mm cells on a 12 mm pitch centred at 6 + 12k: 25 whole cells.
    let mut s = plate(62.0, 3.0);
    cut(&mut s, json!({"depth": 1.5})).unwrap();
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 1.5)).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn through_all_cuts_the_whole_plate_and_gives_the_same_as_a_deep_depth() {
    let mut s = plate(62.0, 3.0);
    cut(&mut s, json!({"through_all": true})).unwrap();
    let through = volume(&s);
    assert!((through - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 3.0)).abs() < 1e-6, "{through}");
    let mut t = plate(62.0, 3.0);
    cut(&mut t, json!({"depth": 10})).unwrap();
    assert!((volume(&t) - through).abs() < 1e-6);
}

#[test]
fn a_keep_out_loses_the_cells_it_touches() {
    // A 12 mm square keep-out at 20..32 touches the cells centred 18 and 30 in both axes.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 20], "p1": [32, 32]}));
    run(&mut s, "sketch.finish", json!({}));
    cut(&mut s, json!({"through_all": true})).unwrap();
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 21.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn the_grid_follows_its_parameters() {
    let mut s = plate(62.0, 3.0);
    run(&mut s, "parameters.add", json!({"name": "cell", "expression": "10 mm"}));
    let r = cut(&mut s, json!({"through_all": true, "size": "cell"})).unwrap();
    let before = volume(&s);
    assert!((before - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 3.0)).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": "cell", "expression": "4 mm"}));
    // 4 mm cells on a 6 mm pitch centred at 6 + 6k, the last at 60: 10 x 10 whole cells.
    let after = volume(&s);
    assert!((after - (62.0 * 62.0 * 3.0 - 100.0 * 16.0 * 3.0)).abs() < 1e-6, "{after} {r}");
}

#[test]
fn square_cells_turned_45_degrees_make_a_diamond_layout() {
    // A 62 mm plate, 10 mm cells on a 12 mm pitch, anchored at the middle (31, 31). Turned 45 degrees
    // each cell is a diamond with a half-diagonal of 5 * sqrt(2) = 7.07 and the lattice steps 8.49
    // along the diagonals, so a diamond is whole when its centre is within 31 - 7.07 = 23.9 of the
    // middle on both axes: the centres (u, v) * 8.49 with u, v both even or both odd and |u|, |v| <= 2.
    let mut s = plate(62.0, 3.0);
    cut(&mut s, json!({"through_all": true, "anchor": [31, 31, 0], "angle": "45 deg"})).unwrap();
    let diamonds = 9.0 + 4.0;
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - diamonds * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
    // Straight, the same cells give the 5 x 5 square layout.
    let mut t = plate(62.0, 3.0);
    cut(&mut t, json!({"through_all": true, "anchor": [31, 31, 0]})).unwrap();
    assert!((volume(&t) - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&t));
}

#[test]
fn a_diamond_layout_follows_the_clip_at_the_edge() {
    let removed = |clip: &str| {
        let mut s = plate(62.0, 3.0);
        cut(&mut s, json!({"through_all": true, "anchor": [31, 31, 0], "angle": "45 deg", "clip": clip})).unwrap();
        62.0 * 62.0 * 3.0 - volume(&s)
    };
    let (whole, snap, free) = (removed("whole"), removed("snap"), removed("free"));
    assert!((whole - 13.0 * 100.0 * 3.0).abs() < 1e-6, "{whole}");
    assert!(snap > whole + 1.0, "snap keeps the edge cells cut along the grid's lines: {snap} vs {whole}");
    assert!(free > whole + 1.0, "free trims the edge cells to the field: {free} vs {whole}");
    assert!(free <= 62.0 * 62.0 * 3.0, "{free}");
}

#[test]
fn an_angle_driven_by_a_parameter_turns_the_grid_and_the_cut_follows() {
    let mut s = plate(62.0, 3.0);
    run(&mut s, "parameters.add", json!({"name": "turn", "expression": "0 deg"}));
    cut(&mut s, json!({"through_all": true, "anchor": [31, 31, 0], "angle": "turn"})).unwrap();
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
    run(&mut s, "parameters.change", json!({"name": "turn", "expression": "45 deg"}));
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 13.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
    run(&mut s, "parameters.change", json!({"name": "turn", "expression": "0 deg"}));
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn the_command_help_reads_the_real_cap_and_names_the_diamond_layout() {
    let help = crate::cmd::find_command("solid.cell_grid").map(|c| c.params).unwrap_or_default();
    assert!(help.contains(&format!("at most {} cells", solidcraft_doc::MAX_CUT_CELLS)), "{help}");
    assert!(!help.contains("10,000"), "{help}");
    assert!(help.contains("45 deg") && help.contains("diamond"), "{help}");
}

#[test]
fn a_symmetric_through_all_cut_goes_all_the_way_through_on_both_sides() {
    // The sketch is on the plate's bottom face: the whole thickness lies on one side of it, so a
    // reach halved between the two sides would stop short of the top.
    let mut s = plate(62.0, 10.0);
    cut(&mut s, json!({"through_all": true, "direction": "symmetric"})).unwrap();
    assert!((volume(&s) - (62.0 * 62.0 * 10.0 - 25.0 * 100.0 * 10.0)).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn every_body_as_the_target_is_reached_by_a_through_all_cut() {
    let mut s = plate(62.0, 3.0);
    cut(&mut s, json!({"through_all": true, "targets": ["*"]})).unwrap();
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn a_grid_that_cuts_nothing_says_so() {
    // The field is a sketch 10 mm above the plate and the cells run up from it, away from the plate.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "Field"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]}));
    run(&mut s, "sketch.finish", json!({}));
    let r = cut(&mut s, json!({"depth": 2})).unwrap();
    assert!(r["warning"].as_str().is_some_and(|w| w.contains("removes no material")), "{r}");
    assert!((volume(&s) - 62.0 * 62.0 * 3.0).abs() < 1e-6);
    // Cut down to the plate, the same grid takes material and says nothing of the kind.
    let r = cut(&mut s, json!({"depth": 10 + 2, "direction": "negative"})).unwrap();
    assert!(r["warning"].as_str().is_none_or(|w| !w.contains("removes no material")), "{r}");
    assert!(volume(&s) < 62.0 * 62.0 * 3.0 - 1.0);
}

#[test]
fn a_bare_size_in_an_inch_design_is_checked_as_inches() {
    // 0.0001 in is 0.00254 mm: over the least size, so it is no longer refused for its size.
    let mut s = plate(62.0, 3.0);
    run(&mut s, "document.units", json!({"units": "in"}));
    let e = cut(&mut s, json!({"through_all": true, "size": "0.0001", "web": "0.0001"})).unwrap_err().to_string();
    assert!(!e.contains("must be at least"), "{e}");
    // The same bare number in a millimetre design is.
    let mut t = plate(62.0, 3.0);
    let e = cut(&mut t, json!({"through_all": true, "size": "0.0001"})).unwrap_err().to_string();
    assert!(e.contains("must be at least"), "{e}");
}

#[test]
fn only_the_bodies_the_grid_reaches_decide_whether_it_meets_sheet_metal() {
    let mut s = plate(62.0, 3.0);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Flat"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [100, 0], "p1": [162, 62]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Flat"}));
    assert!(cut(&mut s, json!({"through_all": true})).is_err(), "no targets: every body is in reach");
    cut(&mut s, json!({"through_all": true, "targets": ["Plate"]})).unwrap();
}

#[test]
fn a_margin_keeps_whole_cells_clear_of_the_edge_and_zero_is_plain_whole() {
    // 10 mm cells centred 6 + 12k in a 62 mm field: the first column and row lie 1 mm from the
    // field's edge, the last 3 mm.
    let removed = |extra: Value| {
        let mut s = plate(62.0, 3.0);
        cut(&mut s, extra).unwrap();
        62.0 * 62.0 * 3.0 - volume(&s)
    };
    let all = 25.0 * 100.0 * 3.0;
    let plain = removed(json!({"through_all": true}));
    assert!((plain - all).abs() < 1e-6, "{plain}");
    assert!((removed(json!({"through_all": true, "margin": 0})) - plain).abs() < 1e-9);
    assert!((removed(json!({"through_all": true, "margin": 0.5})) - all).abs() < 1e-6);
    // Past 1 mm the first column and row go: 4 x 4 cells are left.
    assert!((removed(json!({"through_all": true, "margin": 1.5})) - 16.0 * 100.0 * 3.0).abs() < 1e-6);
    // Past 3 mm the last ones too: 3 x 3.
    assert!((removed(json!({"through_all": true, "margin": 3.5})) - 9.0 * 100.0 * 3.0).abs() < 1e-6);
}

#[test]
fn a_margin_follows_its_parameter() {
    let mut s = plate(62.0, 3.0);
    run(&mut s, "parameters.add", json!({"name": "clear", "expression": "0.5 mm"}));
    cut(&mut s, json!({"through_all": true, "margin": "clear"})).unwrap();
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 3.0)).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": "clear", "expression": "1.5 mm"}));
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 16.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn a_margin_that_is_negative_huge_or_without_whole_cells_is_refused() {
    let mut s = plate(62.0, 3.0);
    for extra in [
        json!({"margin": -1}),
        json!({"margin": "x"}),
        json!({"margin": 1e300}),
        json!({"margin": 1, "clip": "snap"}),
        json!({"margin": 1, "clip": "free"}),
    ] {
        let mut p = json!({"through_all": true});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            p[k] = v;
        }
        assert!(cut(&mut s, p.clone()).is_err(), "{p}");
    }
    assert!((volume(&s) - 62.0 * 62.0 * 3.0).abs() < 1e-6, "a refused grid cuts nothing");
}

#[test]
fn the_grid_is_rebuilt_when_the_sketch_changes() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"}));
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "62 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": r["corners"]["bottom_left"], "b": "origin"}));
    run(&mut s, "sketch.dimension", json!({"entities": [r["sides"]["bottom"]], "value": "w"}));
    run(&mut s, "sketch.dimension", json!({"entities": [r["sides"]["left"]], "value": "w"}));
    run(&mut s, "sketch.finish", json!({}));
    cut(&mut s, json!({"through_all": true})).unwrap();
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 25.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
    // A 38 mm field holds the cells centred at 6, 18 and 30: 3 x 3.
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "38 mm"}));
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 - 9.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn a_grid_over_the_warning_size_says_it_is_slow() {
    // 8 mm cells on a 12 mm pitch from (6, 6): 12 columns by 10 rows, 120 cells.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 146, "width": 122, "height": 3, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [146, 122]}));
    run(&mut s, "sketch.finish", json!({}));
    let r = cut(&mut s, json!({"through_all": true, "size": 8, "web": 4, "anchor": [6, 6, 0]})).unwrap();
    assert!(r["warning"].as_str().is_some_and(|w| w.contains("cells") && w.contains(&solidcraft_doc::SLOW_CUT_CELLS.to_string())), "{r}");
}

#[test]
fn more_cells_than_the_cap_are_refused_with_the_count_and_the_limit() {
    let mut s = plate(62.0, 3.0);
    let e = cut(&mut s, json!({"through_all": true, "size": 0.5, "web": 0.1})).unwrap_err().to_string();
    assert!(e.contains(&solidcraft_doc::MAX_CUT_CELLS.to_string()) && e.contains("cells") && e.contains("larger cells"), "{e}");
    assert!((volume(&s) - 62.0 * 62.0 * 3.0).abs() < 1e-6, "a refused grid cuts nothing");
}

#[test]
fn sheet_metal_is_refused_and_points_to_perforate() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Field"}));
    let e = cut(&mut s, json!({"through_all": true})).unwrap_err().to_string();
    assert!(e.contains("sheet.perforate"), "{e}");
}

#[test]
fn bad_parameters_are_refused_before_a_feature_is_made() {
    let mut s = plate(62.0, 3.0);
    for p in [
        json!({"depth": 0}),
        json!({"depth": -1}),
        json!({}),
        json!({"depth": 1, "size": 0}),
        json!({"depth": 1, "web": 0}),
        json!({"depth": 1, "shape": "polygon", "sides": 2}),
        json!({"depth": 1, "shape": "blob"}),
        json!({"depth": 1, "mode": "flat"}),
        json!({"through_all": true, "mode": "raised"}),
        json!({"depth": 1, "clip": "x"}),
    ] {
        assert!(cut(&mut s, p.clone()).is_err(), "{p}");
    }
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::CellGrid { .. })));
}

#[test]
fn raised_cells_add_the_volume_a_cut_of_the_same_field_removes() {
    // Same field and plan: the cut goes up into the plate, the raised cells stand below it.
    let mut s = plate(62.0, 3.0);
    let r = cut(&mut s, json!({"mode": "raised", "depth": 1.5, "direction": "negative"})).unwrap();
    assert!(r["warning"].is_null(), "{r}");
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 + 25.0 * 100.0 * 1.5)).abs() < 1e-6, "{}", volume(&s));
    assert_eq!(s.model.state().bodies.len(), 1, "the cells join the plate");
}

#[test]
fn raised_and_cut_cells_have_the_same_shape_and_count_in_every_clip() {
    for (shape, clip) in [("hexagon", "snap"), ("hexagon", "free"), ("circle", "whole"), ("triangle", "snap")] {
        let mut a = plate(62.0, 3.0);
        cut(&mut a, json!({"shape": shape, "clip": clip, "depth": 1.5})).unwrap();
        let removed = 62.0 * 62.0 * 3.0 - volume(&a);
        let mut b = plate(62.0, 3.0);
        cut(&mut b, json!({"shape": shape, "clip": clip, "mode": "raised", "depth": 1.5, "direction": "negative"})).unwrap();
        let added = volume(&b) - 62.0 * 62.0 * 3.0;
        assert!(removed > 1.0, "{shape} {clip}");
        assert!((removed - added).abs() < 1e-6 * removed, "{shape} {clip}: cut {removed} raised {added}");
    }
}

#[test]
fn raised_cells_with_no_body_to_join_are_a_new_body() {
    let field = || {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]}));
        run(&mut s, "sketch.finish", json!({}));
        s
    };
    let mut s = field();
    cut(&mut s, json!({"mode": "raised", "depth": 2})).unwrap();
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!((volume(&s) - 25.0 * 100.0 * 2.0).abs() < 1e-6, "{}", volume(&s));
    // A cut has nothing to cut there.
    assert!(cut(&mut field(), json!({"depth": 2})).is_err());
}

#[test]
fn raised_cells_stand_as_deep_as_depth_and_never_through_all() {
    let mut s = plate(62.0, 3.0);
    let e = cut(&mut s, json!({"mode": "raised", "through_all": true})).unwrap_err().to_string();
    assert!(e.contains("through_all") && e.contains("depth"), "{e}");
    assert!(cut(&mut s, json!({"mode": "raised"})).is_err(), "raised needs a depth");
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::CellGrid { .. })));
    // Symmetric: half the depth each side of the plane, so half of it stands outside the plate.
    cut(&mut s, json!({"mode": "raised", "depth": 2, "direction": "symmetric"})).unwrap();
    assert!((volume(&s) - (62.0 * 62.0 * 3.0 + 25.0 * 100.0)).abs() < 1e-6, "{}", volume(&s));
}

/// Two plates 3 thick side by side, `a` long from x = 0 and the rest of 62 from `b_from`, 62 wide, and the
/// 62 x 62 field sketched over both: 25 whole 10 mm cells on a 12 mm pitch, centred at 6 + 12k.
fn two_plates(a: f64, b_from: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": a, "width": 62, "height": 3, "body_name": "A"}));
    run(
        &mut s,
        "solid.box",
        json!({"corner": [b_from, 0, 0], "length": 62.0 - b_from, "width": 62, "height": 3, "body_name": "B", "operation": "new_body"}),
    );
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn volume_of(s: &Session, name: &str) -> f64 {
    let st = s.model.state();
    let b = st.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name}"));
    solidcraft_kernel::measure(&b.body).unwrap().volume
}

#[test]
fn raised_cells_over_two_separate_plates_leave_two_bodies_each_with_its_own_cells() {
    // Plate A spans x 0..30, B x 40..62. Cell columns span 1..11, 13..23, 25..35, 37..47, 49..59:
    // three columns meet only A, two only B, and none bridges the gap.
    let mut s = two_plates(30.0, 40.0);
    cut(&mut s, json!({"mode": "raised", "depth": 1.5, "direction": "negative", "targets": ["A", "B"]})).unwrap();
    let st = s.model.state();
    assert_eq!(st.bodies.len(), 2, "{:?}", st.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>());
    let cell = 100.0 * 1.5;
    assert!((volume_of(&s, "A") - (30.0 * 62.0 * 3.0 + 15.0 * cell)).abs() < 1e-6, "{}", volume_of(&s, "A"));
    assert!((volume_of(&s, "B") - (22.0 * 62.0 * 3.0 + 10.0 * cell)).abs() < 1e-6, "{}", volume_of(&s, "B"));
}

#[test]
fn a_raised_cell_that_bridges_two_plates_joins_them() {
    // The gap is 30..32: the column 25..35 meets both plates, so it makes them one body.
    let mut s = two_plates(30.0, 32.0);
    cut(&mut s, json!({"mode": "raised", "depth": 1.5, "direction": "negative", "targets": ["A", "B"]})).unwrap();
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!((volume(&s) - (60.0 * 62.0 * 3.0 + 25.0 * 100.0 * 1.5)).abs() < 1e-6, "{}", volume(&s));
}

/// Cuts grids of square cells (10 mm on a 12 mm pitch) through a 3 mm plate, from 250 to 1000
/// cells (a tool of more than 1024 cells fails in the kernel: 39 x 38 ended in "shell count"), and prints what each took. Cells are whole, so a `cols` x `rows` plate cuts exactly that many.
/// Run by hand, on a quiet machine: `cargo test --profile ci -p solidcraft-engine cell_grid_speed -- --ignored --nocapture`.
/// `CELL_GRID_SPEED=cols x rows,...` (for example `16x16,45x44`) picks other grids.
#[test]
#[ignore = "timing: run by hand"]
fn cell_grid_speed() {
    let spec = std::env::var("CELL_GRID_SPEED").unwrap_or_else(|_| "16x16,22x23,32x31".into());
    for grid in spec.split(',') {
        let Some((cols, rows)) = grid.split_once('x').and_then(|(c, r)| Some((c.trim().parse::<usize>().ok()?, r.trim().parse::<usize>().ok()?)))
        else {
            panic!("CELL_GRID_SPEED wants cols x rows, got {grid:?}");
        };
        let (w, h) = (12.0 * cols as f64, 12.0 * rows as f64);
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": w, "width": h, "height": 3, "body_name": "Plate"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [w, h]}));
        run(&mut s, "sketch.finish", json!({}));
        let t = std::time::Instant::now();
        let r = cut(&mut s, json!({"through_all": true}));
        let secs = t.elapsed().as_secs_f64();
        let removed = w * h * 3.0 - volume(&s);
        eprintln!("{cols}x{rows} = {} cells: cut in {secs:.2} s (volume removed {removed:.0}); {:?}", cols * rows, r.map(|v| v["warning"].clone()));
    }
}

/// Cuts hexagons of 5 mm through a 3 mm plate, from a few to about 300, and prints what it took.
/// Run by hand, on a quiet machine: `cargo test --profile ci -p solidcraft-engine cell_grid_hex_speed -- --ignored --nocapture`.
#[test]
#[ignore = "timing: run by hand"]
fn cell_grid_hex_speed() {
    for side in [40.0, 70.0, 100.0, 140.0] {
        let mut s = plate(side, 3.0);
        let t = std::time::Instant::now();
        let r = run(&mut s, "solid.cell_grid", json!({"sketch": "Field", "shape": "hexagon", "size": 5, "web": 1, "depth": 3, "clip": "whole"}));
        let secs = t.elapsed().as_secs_f64();
        let removed = 3.0 * side * side - volume(&s);
        let cells = (removed / (3.0 * 25.0 * 3f64.sqrt() / 2.0)).round();
        eprintln!("side {side}: ~{cells} cells cut in {secs:.2} s; warning {:?}; feature ms {:?}", r["warning"], r["ms"]);
    }
}

#[test]
fn a_grid_over_the_old_cap_is_cut_and_one_over_the_cap_is_refused() {
    // 12 mm pitch from (6, 6): a plate 12 x CAP+1 mm holds one row of CAP+1 cells, which is refused
    // before any cutting. 256 cells (16 x 16) were refused when the cap was 250; they are cut now.
    let mut s = plate(192.0, 3.0);
    cut(&mut s, json!({"through_all": true})).unwrap();
    assert!((volume(&s) - (192.0 * 192.0 * 3.0 - 256.0 * 100.0 * 3.0)).abs() < 1e-6, "{}", volume(&s));

    let over = solidcraft_doc::MAX_CUT_CELLS + 1;
    let len = 12.0 * over as f64;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": len, "width": 12, "height": 3, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [len, 12]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = cut(&mut s, json!({"through_all": true})).unwrap_err().to_string();
    assert!(e.contains(&format!("{over} cells")) && e.contains(&solidcraft_doc::MAX_CUT_CELLS.to_string()), "{e}");
}

#[test]
fn keepout_clip_trims_round_a_keep_out_and_defaults_to_clip() {
    let build = |extra: Value| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 62, "width": 62, "height": 3, "body_name": "Plate"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Field"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [62, 62]}));
        run(&mut s, "sketch.circle.center", json!({"center": [31, 31], "diameter": 14.2}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"sketch": "Field", "shape": "hexagon", "size": 8, "web": 2, "through_all": true, "clip": "snap"});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            p[k] = v;
        }
        run(&mut s, "solid.cell_grid", p);
        volume(&s)
    };
    let (plain, same, free) = (build(json!({})), build(json!({"keepout_clip": "snap"})), build(json!({"keepout_clip": "free"})));
    assert_eq!(plain, same);
    assert!(free < plain - 1.0, "free cuts into the keep-out's neighbours: {free} vs {plain}");
}

#[test]
fn keepout_clip_is_checked() {
    let mut s = plate(62.0, 3.0);
    assert!(cut(&mut s, json!({"through_all": true, "keepout_clip": "x"})).is_err());
    assert!(cut(&mut s, json!({"through_all": true, "margin": 1, "keepout_clip": "free"})).is_err());
}

#[test]
fn snap_inset_leaves_a_thicker_rim_and_needs_snap() {
    let build = |extra: Value| {
        let mut s = plate(62.0, 3.0);
        let mut p = json!({"sketch": "Field", "shape": "hexagon", "size": 8, "web": 2, "through_all": true, "clip": "snap"});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            p[k] = v;
        }
        let r = s.execute("solid.cell_grid", &p);
        (r.map(|_| volume(&s)), s)
    };
    let (plain, _) = build(json!({}));
    let (same, _) = build(json!({"snap_inset": 0}));
    let (inset, _) = build(json!({"snap_inset": 1}));
    assert_eq!(plain.as_ref().unwrap(), same.as_ref().unwrap());
    assert!(inset.unwrap() > plain.unwrap() + 1.0, "an inset cuts fewer cells");
    assert!(build(json!({"snap_inset": 1, "clip": "free"})).0.is_err());
    assert!(build(json!({"snap_inset": 1.5})).0.is_err());
    assert!(build(json!({"snap_inset": -1})).0.is_err());
}
