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
fn a_built_in_shape_still_needs_its_size() {
    // The schema leaves `size` optional, since a sketched cell has none, so the handler is what
    // refuses a built-in shape without one: each of the five shapes, with every other field given.
    let mut s = plate(62.0, 3.0);
    for shape in ["hexagon", "square", "triangle", "circle", "polygon"] {
        let p = json!({"sketch": "Field", "shape": shape, "web": 2, "depth": 1.5});
        let e = s.execute("solid.cell_grid", &p).unwrap_err().to_string();
        assert!(e.contains("size"), "{shape}: {e}");
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

/// A sketch `name` holding the cell, in its own coordinates, 500 mm above the plane (away from the
/// plate, so nothing is projected into it): an L of lines with a semicircular arc closing its
/// tall arm.
///
///   (0,0) -> (8,0) -> (8,3) -> (3,3) -> (3,8), the arc through (1.5,9.5) to (0,8), (0,8) -> (0,0).
///
/// Area: the bar 8 x 3 = 24, the arm 3 x 5 = 15 and the half disc of radius 1.5, 9 pi / 8 = 3.534,
/// so A = 39 + 9 pi / 8 = 42.534. The centroid (the origin of a cell when `cell_origin` is not
/// given) is x = (24 * 4 + 15 * 1.5 + 3.534 * 1.5) / A = 2.9107 and, taking the half disc's first
/// moment about y = 8 as 2/3 r^3 = 2.25, y = (24 * 1.5 + 15 * 5.5 + 3.534 * 8 + 2.25) / A = 3.5036.
fn l_cell(s: &mut Session, name: &str) {
    run(s, "sketch.create", json!({"plane": "XY", "offset": 500, "name": name}));
    run(s, "sketch.line", json!({"points": [[0, 0], [8, 0], [8, 3], [3, 3], [3, 8]]}));
    run(s, "sketch.arc.three_point", json!({"start": [3, 8], "end": [0, 8], "through": [1.5, 9.5]}));
    run(s, "sketch.line", json!({"points": [[0, 8], [0, 0]]}));
    run(s, "sketch.finish", json!({}));
}

fn l_area() -> f64 {
    39.0 + 9.0 * std::f64::consts::PI / 8.0
}

/// A 100 x 100 x 3 plate with the field sketched on its bottom face and the L cell in "Cell".
fn l_plate() -> Session {
    let mut s = plate(100.0, 3.0);
    l_cell(&mut s, "Cell");
    s
}

/// A sketched grid on the plate: pitch 20 x 20, stagger 0.5, web 2, cell (0,0) with its origin at
/// (3, 4), so each cell spans x -2.9107..5.0893 and y -3.5036..5.9964 about its origin. Whole clip is
/// the default for a sketched cell and is not given.
fn sketched(s: &mut Session, extra: Value) -> Result<Value, crate::EngineError> {
    let mut p = json!({"sketch": "Field", "shape": "sketch", "cell_sketch": "Cell", "pitch_x": 20, "pitch_y": 20, "stagger": 0.5, "web": 2, "anchor": [3, 4, 0]});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    s.execute("solid.cell_grid", &p)
}

/// `base` with the keys of `extra` set over it.
fn with(mut base: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        base[k] = v;
    }
    base
}

fn cell_grid_clip(s: &Session) -> Option<solidcore_record::cell_grid::Clip> {
    s.doc.features.iter().find_map(|f| match &f.kind {
        solidcraft_doc::FeatureKind::CellGrid { clip, .. } => Some(*clip),
        _ => None,
    })
}

#[test]
fn a_sketched_l_cell_cut_on_a_staggered_lattice_removes_exactly_its_volume() {
    // Rows are 20 apart and row j is shifted 10 j along; origins are (3 + 20 i + 10 j, 4 + 20 j).
    // Whole cells in the 100 x 100 field: rows j = 0..4 (the cell spans y 0.4964 + 20 j to
    // 9.9964 + 20 j; row 5 would end at 109.99, row -1 starts below 0). In an even row x runs
    // 0.0893 + 20 k to 8.0893 + 20 k: k = 0..4 (k = 5 ends at 108), five cells. In an odd row the
    // shift of 10 gives 10.0893 + 20 k to 18.0893 + 20 k: k = 0..4 ends at 98.09, five cells, and
    // k = -1 starts at -9.9. So 5 x 5 = 25 cells and the cut takes 25 * A * 1.5 from the
    // 100 x 100 x 3 plate.
    let mut s = l_plate();
    sketched(&mut s, json!({"depth": 1.5})).unwrap();
    let expect = 100.0 * 100.0 * 3.0 - 25.0 * l_area() * 1.5;
    assert!((volume(&s) - expect).abs() < 1e-3, "{} vs {expect}", volume(&s));
    assert_eq!(cell_grid_clip(&s), Some(solidcore_record::cell_grid::Clip::Whole), "a sketched cell defaults to whole");
}

#[test]
fn a_sketched_l_cell_raised_adds_the_volume_the_cut_removes() {
    // The same 25 cells, standing below the plate's bottom face and joined to it.
    let mut s = l_plate();
    sketched(&mut s, json!({"mode": "raised", "depth": 1.5, "direction": "negative"})).unwrap();
    let expect = 100.0 * 100.0 * 3.0 + 25.0 * l_area() * 1.5;
    assert!((volume(&s) - expect).abs() < 1e-3, "{} vs {expect}", volume(&s));
    assert_eq!(s.model.state().bodies.len(), 1, "the cells join the plate");
}

#[test]
fn a_sketched_cell_follows_its_sketch_and_its_parameters() {
    let mut s = l_plate();
    run(&mut s, "parameters.add", json!({"name": "px", "expression": "20 mm"}));
    sketched(&mut s, json!({"depth": 1.5, "pitch_x": "px"})).unwrap();
    // Pitch 25: even rows x 0.0893 + 25 k (k = 0..3, k = 4 ends at 108), odd rows 12.5 shifted:
    // 12.5 + 0.0893 + 25 k ends at 8.0893 + 12.5 + 25 k <= 100 for k = 0..3: 4 cells per row,
    // 20 cells in the five rows.
    run(&mut s, "parameters.change", json!({"name": "px", "expression": "25 mm"}));
    let expect = 100.0 * 100.0 * 3.0 - 20.0 * l_area() * 1.5;
    assert!((volume(&s) - expect).abs() < 1e-3, "{} vs {expect}", volume(&s));
}

#[test]
fn a_cell_sketch_must_hold_exactly_one_closed_profile() {
    let mut s = l_plate();
    // Two separate squares: two profiles.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 600, "name": "Two"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [4, 4]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [14, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = sketched(&mut s, json!({"depth": 1, "cell_sketch": "Two"})).unwrap_err().to_string();
    assert!(e.contains("Two") && e.contains("2 closed profiles"), "{e}");
    // A sketch with only an open line: no profile.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 700, "name": "Open"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [5, 0], [5, 5]]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = sketched(&mut s, json!({"depth": 1, "cell_sketch": "Open"})).unwrap_err().to_string();
    assert!(e.contains("Open"), "{e}");
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::CellGrid { .. })));
}

#[test]
fn a_sketched_cell_needs_its_sketch_and_its_pitch() {
    let mut s = l_plate();
    let mut p = json!({"sketch": "Field", "shape": "sketch", "cell_sketch": "Cell", "pitch_x": 20, "pitch_y": 20, "web": 2, "depth": 1});
    for gone in ["cell_sketch", "pitch_x", "pitch_y"] {
        let mut q = p.clone();
        q.as_object_mut().unwrap().remove(gone);
        let e = s.execute("solid.cell_grid", &q).unwrap_err().to_string();
        assert!(e.contains(gone), "{gone}: {e}");
    }
    p["cell_sketch"] = json!("Nowhere");
    assert!(s.execute("solid.cell_grid", &p).unwrap_err().to_string().contains("Nowhere"));
    // The field's own sketch is a sketch with a profile, so it is a cell (a 100 mm square the lattice cannot hold).
    p["cell_sketch"] = json!("Field");
    assert!(s.execute("solid.cell_grid", &p).is_err());
    // The sketch-only parameters are refused with a built-in shape.
    for extra in [
        json!({"cell_sketch": "Cell"}),
        json!({"pitch_x": 20}),
        json!({"stagger": 0.5}),
        json!({"flip": true}),
        json!({"cell_angle": 10}),
        json!({"cell_origin": [0, 0]}),
    ] {
        let e = cut(&mut s, with(json!({"depth": 1, "shape": "hexagon"}), extra.clone())).unwrap_err().to_string();
        assert!(e.contains("shape: sketch"), "{extra}: {e}");
    }
}

#[test]
fn a_pitch_too_tight_for_the_cell_and_the_web_is_refused_as_a_layout() {
    // The cell is 8 wide: at pitch 9 the neighbours are 1 mm apart, closer than the 2 mm web.
    let mut s = l_plate();
    let e = sketched(&mut s, json!({"depth": 1, "pitch_x": 9, "pitch_y": 40, "stagger": 0})).unwrap_err().to_string();
    assert!(e.contains("the layout"), "{e}");
    // A pitch smaller than the cell's width overlaps outright.
    let e = sketched(&mut s, json!({"depth": 1, "pitch_x": 5, "pitch_y": 40, "stagger": 0})).unwrap_err().to_string();
    assert!(e.contains("the layout"), "{e}");
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::CellGrid { .. })));
}

#[test]
fn snap_is_refused_for_a_sketched_cell_and_free_is_allowed() {
    let mut s = l_plate();
    for p in [json!({"clip": "snap"}), json!({"keepout_clip": "snap"}), json!({"clip": "free", "keepout_clip": "snap"})] {
        let e = sketched(&mut s, with(json!({"depth": 1}), p.clone())).unwrap_err().to_string();
        assert!(e.contains("snap") && e.contains("whole or free"), "{p}: {e}");
    }
    sketched(&mut s, json!({"depth": 1, "clip": "free"})).unwrap();
    assert_eq!(cell_grid_clip(&s), Some(solidcore_record::cell_grid::Clip::Free));
    // No cell of this lattice crosses the field's edge, so free keeps the same 25 whole cells.
    assert!((volume(&s) - (100.0 * 100.0 * 3.0 - 25.0 * l_area())).abs() < 1e-3, "{}", volume(&s));
}

#[test]
fn a_sketched_grids_size_sides_and_dot_grid_are_not_used() {
    let mut s = l_plate();
    sketched(&mut s, json!({"depth": 1.5, "size": 0, "sides": 1, "dot_grid": "square"})).unwrap();
    let expect = 100.0 * 100.0 * 3.0 - 25.0 * l_area() * 1.5;
    assert!((volume(&s) - expect).abs() < 1e-3, "{} vs {expect}", volume(&s));
}

#[test]
fn a_sketched_cell_turned_flipped_or_given_an_origin_is_still_planned_exactly() {
    // A cell origin at the profile's corner (0, 0) and the anchor at (3, 4): the cell spans x 3..11,
    // y 4..13.5, so every cell is 8 wide and 9.5 tall as before and the count follows from it.
    // Rows 4 + 20 j .. 13.5 + 20 j: j = 0..4 (j = 4 ends at 93.5; the next starts at 104). Even rows
    // 3 + 20 k .. 11 + 20 k, k = 0..4 (ends 91); odd rows 13 + 20 k .. 21 + 20 k, k = 0..4 (ends
    // 101: out), so k = 0..3: 4 cells. Rows 0, 2, 4 have 5, rows 1, 3 have 4: 5 * 3 + 4 * 2 = 23.
    let mut s = l_plate();
    sketched(&mut s, json!({"depth": 1.5, "cell_origin": [0, 0]})).unwrap();
    let expect = 100.0 * 100.0 * 3.0 - 23.0 * l_area() * 1.5;
    assert!((volume(&s) - expect).abs() < 1e-3, "{} vs {expect}", volume(&s));
    // Flipped (alternate cells mirrored) and turned cells keep the area of every cell: the planner
    // checks them exactly, so the volume is a whole number of cell areas.
    for extra in [json!({"flip": true}), json!({"cell_angle": 90})] {
        let mut t = l_plate();
        sketched(&mut t, with(json!({"depth": 1.5, "pitch_x": 30, "pitch_y": 30, "stagger": 0}), extra.clone())).unwrap();
        let removed = 100.0 * 100.0 * 3.0 - volume(&t);
        let cells = removed / (l_area() * 1.5);
        assert!((cells - cells.round()).abs() < 1e-6 && cells >= 1.0, "{extra}: {cells} cells");
    }
}

#[test]
fn hostile_sketched_cell_parameters_are_refused_without_a_feature() {
    let mut s = l_plate();
    for p in [
        json!({"pitch_x": -20}),
        json!({"pitch_x": 0}),
        json!({"pitch_y": 0}),
        json!({"pitch_y": -1}),
        json!({"pitch_x": f64::NAN}),
        json!({"pitch_y": f64::INFINITY}),
        json!({"pitch_x": 1e300}),
        json!({"pitch_x": 1e-9}),
        json!({"pitch_x": "twenty"}),
        json!({"stagger": 1.5}),
        json!({"stagger": -0.1}),
        json!({"stagger": "2 mm"}),
        json!({"cell_angle": "ten"}),
        json!({"cell_angle": 1e300}),
        json!({"flip": "yes"}),
        json!({"cell_origin": [1]}),
        json!({"cell_origin": [1, 2, 3]}),
        json!({"cell_origin": [1e12, 0]}),
        json!({"cell_origin": ["a", "b"]}),
        json!({"cell_origin": 5}),
        json!({"cell_sketch": true}),
        json!({"cell_sketch": 999}),
        json!({"web": -1}),
        json!({"web": f64::NAN}),
        json!({"depth": f64::NAN}),
        json!({"clip": "snap"}),
    ] {
        assert!(sketched(&mut s, with(json!({"depth": 1}), p.clone())).is_err(), "{p}");
    }
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::CellGrid { .. })));
}

#[test]
fn the_command_help_names_the_sketched_cell() {
    let help = crate::cmd::find_command("solid.cell_grid").map(|c| c.params).unwrap_or_default();
    for word in ["sketch", "cell_sketch", "pitch_x", "pitch_y", "stagger", "cell_angle", "flip", "cell_origin", "whole with shape sketch"] {
        assert!(help.contains(word), "{word}: {help}");
    }
}
