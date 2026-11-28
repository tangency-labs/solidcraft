//! A profile picked by the curves around it, and a sketch on the cap such a profile makes, keep
//! working when the sketch's outline is edited so that no profile is bounded by exactly those
//! curves any more: a line that splits the region, a side deleted and drawn again, a notch
//! trimmed into the outline. The feature falls back to the best match and warns; it no longer
//! fails and takes its body (and everything after it) with it.

use serde_json::{Value, json};
use solidcraft_geom::MeshGeom;

use crate::Session;

const HOLE: f64 = std::f64::consts::PI * 16.0 * 10.0;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &Session) -> f64 {
    s.model.state().bodies.iter().map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-3)).unwrap().measure().volume).sum()
}

fn result(s: &Session, name: &str) -> (Option<String>, Option<String>) {
    let id = s.doc.find_feature(name).unwrap().id;
    let r = s.model.result(id).unwrap();
    (r.error.clone(), r.warning.clone())
}

/// A 40 × 20 plate with a hole: the outline `l1`–`l4` (its width is the parameter `W`) and a
/// circle, the region between them picked by a point and extruded 10 mm.
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 10], "radius": 4}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "W"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "10 mm", "profiles": [{"point": [10, 10]}], "name": "E1"}));
    assert_eq!(result(&s, "E1"), (None, None));
    assert!((volume(&s) - (8000.0 - HOLE)).abs() < 0.5, "{}", volume(&s));
    s
}

fn edit(s: &mut Session, steps: &[(&str, Value)]) {
    run(s, "sketch.edit", json!({"sketch": "S1"}));
    for (id, p) in steps {
        run(s, id, p.clone());
    }
    run(s, "sketch.finish", json!({}));
}

fn assert_survives(s: &Session, feature: &str, expected: f64, says: &str) {
    let (error, warning) = result(s, feature);
    assert_eq!(error, None, "{feature} keeps its body");
    let warning = warning.unwrap_or_default();
    assert!(warning.contains(says), "{feature}: {warning}");
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!((volume(s) - expected).abs() < 0.5, "{feature}: volume {} (expected {expected})", volume(s));
}

/// A line across the picked region cuts it in two: both pieces together are the region that was
/// picked, so the plate keeps its whole volume (with its hole, which stays a hole).
#[test]
fn an_extrude_survives_a_line_splitting_its_profile() {
    for x in [20, 15] {
        let mut s = plate();
        edit(&mut s, &[("sketch.line", json!({"points": [[x, 0], [x, 20]]}))]);
        assert_survives(&s, "E1", 8000.0 - HOLE, "is cut by another curve now; using the 2 pieces inside it");
    }
}

/// A side deleted and drawn again is a new curve: the profile that shares the other three sides
/// is the plate, and the warning names the side that went and the one that came.
#[test]
fn an_extrude_survives_a_side_deleted_and_drawn_again() {
    for (side, line) in [("l2", [[40, 0], [40, 20]]), ("l3", [[40, 20], [0, 20]])] {
        let mut s = plate();
        edit(&mut s, &[("sketch.delete", json!({"entities": [side]})), ("sketch.line", json!({"points": line}))]);
        assert_survives(&s, "E1", 8000.0 - HOLE, &format!("`{side}` no longer on it"));
        assert!(result(&s, "E1").1.unwrap().contains("shares 3 of its 4 curves"));
    }
}

/// A notch trimmed out of the top side adds curves to the outline: the profile still has all four
/// sides and takes the notch with it.
#[test]
fn an_extrude_survives_a_notch_trimmed_into_its_outline() {
    let mut s = plate();
    edit(
        &mut s,
        &[("sketch.line", json!({"points": [[10, 20], [10, 25], [20, 25], [20, 20]]})), ("sketch.trim", json!({"curve": "l3", "at": [15, 20]}))],
    );
    assert_survives(&s, "E1", 8000.0 + 500.0 - HOLE, "using the profile that shares 4 of its 4 curves");
}

/// Dimension edits still match exactly, with no warning; an outline replaced altogether matches
/// nothing, and that is still an error rather than a guess.
#[test]
fn exact_matches_stay_quiet_and_nothing_plausible_is_an_error() {
    let mut s = plate();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "60 mm"}));
    assert_eq!(result(&s, "E1"), (None, None));
    assert!((volume(&s) - (12000.0 - HOLE)).abs() < 0.5);
    edit(
        &mut s,
        &[("sketch.delete", json!({"entities": ["l1", "l2", "l3", "l4"]})), ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [50, 20]}))],
    );
    let (error, _) = result(&s, "E1");
    assert!(error.is_some_and(|e| e.contains("no profile of S1 is bounded by exactly")));
    assert!(s.model.state().bodies.is_empty());
}

/// The public example of #9368: a 40 x 20 rectangle with a circle tangent to its top and bottom
/// (radius 10 at (20, 10)), or a ring offset 1 mm around a circle of radius 9 with `offset`.
/// `Gauche` is the region left of the circle, `Disque` (joined) the disc or the ring.
fn rectangle_and_circle(offset: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 10], "radius": if offset { 9 } else { 10 }}));
    if offset {
        run(&mut s, "sketch.offset", json!({"curves": ["c1"], "distance": 1, "side": [20, 25]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "10 mm", "profiles": [{"point": [3, 10]}], "name": "Gauche"}));
    let disque = if offset { [20.0, 19.5] } else { [20.0, 15.0] };
    run(&mut s, "solid.extrude", json!({"distance": "10 mm", "profiles": [{"point": disque}], "operation": "join", "name": "Disque"}));
    s
}

/// A line from the top side to the circle cuts a sliver off the left region and gives the circle
/// more arcs: the disc is still the same region, so it is found without a word, and the left region
/// is still found, as its two pieces.
#[test]
fn an_extrude_survives_a_line_ending_on_a_circle_split_by_tangents() {
    let mut s = rectangle_and_circle(false);
    edit(&mut s, &[("sketch.line", json!({"points": [[14, 20], [14, 18]]}))]);
    assert_eq!(result(&s, "Disque"), (None, None));
    let (error, warning) = result(&s, "Gauche");
    assert_eq!(error, None);
    assert!(warning.unwrap_or_default().contains("2 pieces"));
    assert!((volume(&s) - 5570.6).abs() < 0.5, "volume {}", volume(&s));
}

/// Same sketch with a ring offset from a circle: the loops name the offset's output. The line
/// cuts the left region (its closed loop is taken as two pieces, with a warning), and the ring,
/// only given more arcs, is found as it was.
#[test]
fn an_extrude_with_an_offset_curve_survives_a_line_splitting_its_profile() {
    let mut s = rectangle_and_circle(true);
    edit(&mut s, &[("sketch.line", json!({"points": [[14, 20], [14, 18]]}))]);
    let (error, warning) = result(&s, "Gauche");
    assert_eq!(error, None);
    assert!(warning.unwrap_or_default().contains("2 pieces"));
    assert_eq!(result(&s, "Disque"), (None, None));
    assert!((volume(&s) - 3026.2).abs() < 0.5, "volume {}", volume(&s));
}

/// A block `E1` with a sketch `S2` on its top cap and a hole cut from it, `E2`.
fn block_with_hole() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "40 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "W"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "10 mm", "name": "E1"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}, "name": "S2"}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "3 mm", "direction": "negative", "operation": "cut", "name": "E2"}));
    for f in ["E1", "S2", "E2"] {
        assert_eq!(result(&s, f).0, None, "{f}");
    }
    s
}

fn hole_volume() -> f64 {
    std::f64::consts::PI * 9.0 * 3.0
}

fn assert_hole_follows(s: &Session, block: f64, sketch_warns: bool) {
    let (error, warning) = result(s, "S2");
    assert_eq!(error, None, "S2 keeps its face");
    if sketch_warns {
        let warning = warning.unwrap_or_default();
        assert!(warning.contains("the face this sketch sits on, the top cap of E1"), "{warning}");
        assert!(warning.contains("using the top cap of E1 as it is now"), "{warning}");
    }
    assert_eq!(result(s, "E2").0, None, "the hole after it is still cut");
    let id = s.doc.find_feature("S2").unwrap().id;
    let plane = s.model.state().sketch(id).unwrap().plane;
    assert!((plane.origin.z - 10.0).abs() < 1e-9, "{plane:?}");
    assert!((volume(s) - (block - hole_volume())).abs() < 0.5, "volume {} (expected {})", volume(s), block - hole_volume());
}

/// The base sketch cut by a line still makes one block with one top cap.
#[test]
fn a_sketch_on_a_cap_survives_a_line_splitting_the_base_profile() {
    let mut s = block_with_hole();
    edit(&mut s, &[("sketch.line", json!({"points": [[20, 0], [20, 20]]}))]);
    assert_hole_follows(&s, 8000.0, false);
}

/// A side of the base sketch deleted and drawn again: the cap is bounded by a new curve.
#[test]
fn a_sketch_on_a_cap_survives_a_base_side_deleted_and_drawn_again() {
    let mut s = block_with_hole();
    edit(&mut s, &[("sketch.delete", json!({"entities": ["l2"]})), ("sketch.line", json!({"points": [[40, 0], [40, 20]]}))]);
    assert_hole_follows(&s, 8000.0, true);
    assert!(result(&s, "S2").1.unwrap().contains("`l2` no longer around it"));
}

/// A corner notch trimmed into the base sketch.
#[test]
fn a_sketch_on_a_cap_survives_a_notch_trimmed_into_the_base_outline() {
    let mut s = block_with_hole();
    edit(
        &mut s,
        &[
            ("sketch.line", json!({"points": [[0, 15], [5, 15], [5, 20]]})),
            ("sketch.trim", json!({"curve": "l4", "at": [0, 18]})),
            ("sketch.trim", json!({"curve": "l3", "at": [2, 20]})),
        ],
    );
    assert_hole_follows(&s, 8000.0 - 25.0 * 10.0, true);
}

/// The base extrude picked by a point (stored by its curves) and the sketch on its cap both
/// survive the outline being edited.
#[test]
fn a_picked_profile_and_the_sketch_on_its_cap_survive_together() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "10 mm", "profiles": [{"point": [10, 10]}], "name": "E1"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}, "name": "S2"}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "3 mm", "direction": "negative", "operation": "cut", "name": "E2"}));
    edit(&mut s, &[("sketch.delete", json!({"entities": ["l2"]})), ("sketch.line", json!({"points": [[40, 0], [40, 20]]}))]);
    assert!(result(&s, "E1").1.is_some_and(|w| w.contains("`l2` no longer on it")));
    assert_hole_follows(&s, 8000.0, true);
    // Saved and opened again, it evaluates the same way.
    let reopened = Session::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap());
    assert_hole_follows(&reopened, 8000.0, true);
}

/// The halves of the lens (upper picks) and of the lower crescent (lower picks).
const TWIN_PICKS: [[f64; 2]; 4] = [[-1.26, 19.7], [1.26, 19.7], [-0.5, 16.5], [0.5, 16.5]];

/// Two circles of radius `r` 0.6 mm apart and a fixed line through both centres, drawn on from
/// below and ending just above the lower circle: on r = 3.04 mm the line cuts the lens and the
/// lower crescent into mirror twins, which the curves around them can't tell apart.
fn twin_sketch(r: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "r", "expression": r}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "s"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 19.4], "radius": 3.04}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "type": "diameter", "value": "2*r"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 20], "radius": 3.04}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c2"], "type": "diameter", "value": "2*r"}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "c1.center", "fixed": true}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "c2.center", "fixed": true}));
    run(&mut s, "sketch.line", json!({"points": [[0, 1.75], [0, 22.5]]}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "l1", "fixed": true}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

/// [`twin_sketch`] with the regions at `picks` picked by points and extruded 2 mm as `E`.
fn twin_regions(r: &str, picks: &[[f64; 2]]) -> Session {
    let mut s = twin_sketch(r);
    let profiles: Vec<Value> = picks.iter().map(|p| json!({"point": p})).collect();
    run(&mut s, "solid.extrude", json!({"sketch": "s", "profiles": profiles, "distance": 2, "name": "E"}));
    assert_eq!(result(&s, "E").0, None, "E builds on r = {r}");
    s
}

/// The radius grows until the line's end lies inside the lens, so the line no longer cuts it and
/// the sketch has other regions. The extrude still builds the regions under its points: the lens
/// once (both its halves were picked) and the two halves of the crescent, as an extrude made on
/// the new radius does (#22390).
#[test]
fn twin_regions_picked_by_points_follow_a_change_of_the_regions() {
    let mut s = twin_regions("3.04 mm", &TWIN_PICKS);
    let saved = serde_json::to_value(&s.doc.find_feature("E").unwrap().kind).unwrap().to_string();
    assert!(saved.contains("\"points\""), "the picks are saved by their points: {saved}");
    let before = volume(&s);
    run(&mut s, "parameters.change", json!({"name": "r", "expression": "3.45 mm"}));
    assert_eq!(result(&s, "E").0, None);
    let changed = volume(&s);
    let built = volume(&twin_regions("3.45 mm", &TWIN_PICKS[1..]));
    assert!((changed - built).abs() < 1e-3, "{changed} after the change, {built} built so (before {before})");
    assert!(changed > before, "the regions grew with r: {before} to {changed}");
}

/// The profile indices of the regions under `points` in sketch `s`, as a script or an importer
/// that matched the regions itself gives them.
fn indices_at(s: &Session, points: &[[f64; 2]]) -> Vec<usize> {
    let id = s.doc.find_feature("s").unwrap().id;
    let st = s.model.state();
    let profiles = &st.sketch(id).unwrap().profiles;
    points.iter().map(|p| solidcraft_sketch::profile_at_point(profiles, solidcore_record::Vec2::new(p[0], p[1])).unwrap()).collect()
}

/// The same twins picked by their indices, as Maverick's translator picks them: after the change
/// the extrude builds the regions an extrude made on the new radius builds, with no error.
#[test]
fn twin_regions_picked_by_index_follow_a_change_of_the_regions() {
    let mut s = twin_sketch("3.04 mm");
    let indices = indices_at(&s, &TWIN_PICKS);
    assert_eq!(indices.iter().collect::<std::collections::BTreeSet<_>>().len(), 4, "four regions: {indices:?}");
    run(&mut s, "solid.extrude", json!({"sketch": "s", "profiles": indices, "distance": 2, "name": "E"}));
    assert_eq!(result(&s, "E").0, None);
    let saved = serde_json::to_value(&s.doc.find_feature("E").unwrap().kind).unwrap().to_string();
    assert!(saved.contains("\"sided_curves\""), "the picks are saved by the sides of their curves: {saved}");
    assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap(), *s.doc, "the saved picks reopen");
    let before = volume(&s);
    let by_points = volume(&twin_regions("3.04 mm", &TWIN_PICKS));
    assert!((before - by_points).abs() < 1e-3, "{before} by index, {by_points} by points");
    run(&mut s, "parameters.change", json!({"name": "r", "expression": "3.45 mm"}));
    assert_eq!(result(&s, "E").0, None);
    let changed = volume(&s);
    let built = volume(&twin_regions("3.45 mm", &TWIN_PICKS[1..]));
    assert!((changed - built).abs() < 1e-3, "{changed} after the change, {built} built so (before {before})");
}

/// A fixed circle `c1` (radius 10) cut in half by a fixed line `l1`, and a circle `c2` of radius
/// `r2` with its centre fixed at (3, 10). On r2 = 2 mm `c2` cuts `c1` but not `l1`: only the right
/// half touches it, so the curves around the right half (`c1`, `l1`, `c2`) name it alone.
fn half_and_bite(r2: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "r2", "expression": r2}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "s"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "c1", "fixed": true}));
    run(&mut s, "sketch.line", json!({"points": [[0, -15], [0, 15]]}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "l1", "fixed": true}));
    run(&mut s, "sketch.circle.center", json!({"center": [3, 10], "radius": 2}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c2"], "type": "diameter", "value": "2*r2"}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "c2.center", "fixed": true}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

/// #23291: the right half, picked by a point or by its index, is saved by its curves and the side
/// of each it lies on. On r2 = 4 mm `c2` crosses `l1` too, so both halves are bounded by `c1`,
/// `l1` and `c2`; the extrude still builds the right half (the one right of `l1`), as an extrude
/// made on 4 mm does, where it failed with "2 profiles of s are bounded by exactly the curves".
#[test]
fn a_region_a_change_makes_a_twin_of_another_keeps_building() {
    let built = {
        let mut s = half_and_bite("4 mm");
        run(&mut s, "solid.extrude", json!({"sketch": "s", "profiles": [{"point": [5, 0]}], "distance": 2, "name": "E"}));
        volume(&s)
    };
    for by_index in [false, true] {
        let mut s = half_and_bite("2 mm");
        let profiles = if by_index { json!(indices_at(&s, &[[5.0, 0.0]])) } else { json!([{"point": [5, 0]}]) };
        run(&mut s, "solid.extrude", json!({"sketch": "s", "profiles": profiles, "distance": 2, "name": "E"}));
        assert_eq!(result(&s, "E").0, None);
        let saved = serde_json::to_value(&s.doc.find_feature("E").unwrap().kind).unwrap().to_string();
        assert!(saved.contains("\"curves\"") && saved.contains("\"sides\""), "saved by its curves and their sides: {saved}");
        assert_eq!(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap(), *s.doc, "the saved pick reopens");
        let before = volume(&s);
        run(&mut s, "parameters.change", json!({"name": "r2", "expression": "4 mm"}));
        assert_eq!(result(&s, "E").0, None, "by index: {by_index}");
        let changed = volume(&s);
        assert!((changed - built).abs() < 1e-3, "by index: {by_index}: {changed} after the change, {built} built so (before {before})");
        assert!(changed < before, "the bite grew into the right half: {before} to {changed}");
    }
}
