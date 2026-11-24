//! To Object extents on curved targets, and shells outside/both on curved bodies.

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn body(s: &mut Session, b: &str) -> Value {
    run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0].clone()
}

fn stepped_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "step_height", "expression": "10 mm"}));
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 10, "body_name": "Block"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-5, -5], "p1": [5, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "step_height", "operation": "join"}));
    s
}

/// A column beside a step ends at the step's top, including when the To point lies on an
/// edge or corner shared with a vertical wall (#3729).
#[test]
fn extrude_to_a_planar_face_boundary_ends_at_its_plane() {
    for to in [[5, 0, 20], [5, 5, 20], [0, 0, 20], [1000, 1000, 20]] {
        let mut s = stepped_block();
        run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
        run(&mut s, "sketch.circle.center", json!({"center": [8, 0], "radius": 1.5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"to": to, "operation": "join"}));
        let b = body(&mut s, "Block");
        let top = b["bbox"]["max"][2].as_f64().unwrap();
        let volume = b["volume_mm3"].as_f64().unwrap();
        assert!((top - 20.0).abs() < 1e-6, "To {to:?}: z max {top}");
        assert!((volume - (5000.0 + 10.0 * PI * 1.5 * 1.5)).abs() < 0.1, "To {to:?}: volume {volume}");
    }
}

/// The face chosen at an edge must also be the stored reference: raising the step must raise
/// the end of the extrusion, including after a save/reload.
#[test]
fn extrude_to_a_planar_face_boundary_follows_it_after_reload() {
    for to in [[5, 0, 20], [5, 5, 20]] {
        let mut s = stepped_block();
        run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 30], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
        run(&mut s, "sketch.circle.center", json!({"center": [8, 0], "radius": 1.5}));
        run(&mut s, "sketch.finish", json!({}));
        // A negative reach with an offset past the target, away from the start.
        run(&mut s, "solid.extrude", json!({"to": to, "to_offset": 2, "operation": "new", "body_name": "Post"}));
        let b = body(&mut s, "Post");
        assert!((b["bbox"]["min"][2].as_f64().unwrap() - 18.0).abs() < 1e-6, "{b}");
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        let mut cold = Session::new(doc);
        for current in [&mut s, &mut cold] {
            run(current, "parameters.change", json!({"name": "step_height", "expression": "15 mm"}));
            let b = body(current, "Post");
            let volume = b["volume_mm3"].as_f64().unwrap();
            assert!((b["bbox"]["min"][2].as_f64().unwrap() - 23.0).abs() < 1e-6, "{b}");
            assert!((b["bbox"]["max"][2].as_f64().unwrap() - 30.0).abs() < 1e-6, "{b}");
            assert!((volume - 7.0 * PI * 1.5 * 1.5).abs() < 0.1, "{b}");
            assert!(current.model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", current.model.results);
        }
    }
}

/// A square post extruded up to a horizontal cylinder (axis along x, r 10, centre z 30): its
/// top takes the cylinder's shape, so the post is shortest in the middle.
#[test]
fn extrude_to_a_cylinder_follows_it() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [-20, 0, 30], "axis": [1, 0, 0], "radius": 10, "height": 40, "body_name": "Bar"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-4, -4], "p1": [4, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [0, 0, 20], "body_name": "Post"}));
    let b = body(&mut s, "Post");
    // Highest at the post's sides (y = ±4): z = 30 − √(100 − 16); the volume follows the arc.
    let top = b["bbox"]["max"][2].as_f64().unwrap_or(0.0);
    assert!((top - (30.0 - (100.0f64 - 16.0).sqrt())).abs() < 1e-3, "{b}");
    // ∫ over y of (30 − √(100 − y²)) × 8 (x width), y from −4 to 4.
    let n = 2000;
    let want: f64 = (0..n).map(|i| -4.0 + 8.0 * (i as f64 + 0.5) / n as f64).map(|y| (30.0 - (100.0 - y * y).sqrt()) * 8.0 * 8.0 / n as f64).sum();
    assert!((b["volume_mm3"].as_f64().unwrap_or(0.0) - want).abs() < want * 2e-3, "{} vs {want}", b["volume_mm3"]);
}

/// A round post extruded up to a sphere (r 15 at z 40): it stops on the sphere's underside.
#[test]
fn extrude_to_a_sphere_follows_it() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 40], "radius": 15, "body_name": "Ball"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [0, 0, 25], "body_name": "Post"}));
    let b = body(&mut s, "Post");
    let top = b["bbox"]["max"][2].as_f64().unwrap_or(0.0);
    assert!((top - (40.0 - (225.0f64 - 25.0).sqrt())).abs() < 1e-2, "{b}");
    // Volume: cylinder to z 25 plus the ring between the sphere cap and the plane z = 25.
    let cap = |r: f64| 40.0 - (225.0 - r * r).sqrt();
    let n = 2000;
    let want: f64 = (0..n).map(|i| 5.0 * (i as f64 + 0.5) / n as f64).map(|r| cap(r) * 2.0 * PI * r * 5.0 / n as f64).sum();
    assert!((b["volume_mm3"].as_f64().unwrap_or(0.0) - want).abs() < want * 2e-3, "{} vs {want}", b["volume_mm3"]);
}

/// Shell Outside and Both on a cylinder (open top): exact offsets of the curved wall.
#[test]
fn shell_outside_and_both_on_a_cylinder() {
    let shell = |dir: &str| {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "C"}));
        run(&mut s, "solid.shell", json!({"faces": [[0, 0, 20]], "thickness": 2, "direction": dir}));
        body(&mut s, "C")["volume_mm3"].as_f64().unwrap_or(0.0)
    };
    let out = PI * (144.0 * 22.0 - 100.0 * 20.0);
    assert!((shell("outside") - out).abs() < out * 1e-3, "{} vs {out}", shell("outside"));
    let both = PI * (121.0 * 21.0 - 81.0 * 19.0);
    assert!((shell("both") - both).abs() < both * 1e-3, "{} vs {both}", shell("both"));
}

/// A ring section revolved To Object up to a post standing in its path: it ends on the post's
/// round side (touching, not overlapping), between first contact and the post's centre.
#[test]
fn revolve_to_a_cylinder_follows_it() {
    for side in [1.0, -1.0] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"base": [0, 14.0 * side, -5], "radius": 4, "height": 15, "body_name": "Post"}));
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[10, 0], [15, 0], [15, 5], [10, 5]], "closed": true}));
        run(&mut s, "sketch.finish", json!({}));
        let r = s.execute("solid.revolve", &json!({"sketch": "S", "axis": "y", "to": [4, 14.0 * side, 2], "body_name": "Arc"}));
        if r.is_err() {
            continue; // the post on the side the revolve turns away from is past the turn
        }
        let st = s.model.state();
        let (arc, post) = (st.body("Arc").unwrap(), st.body("Post").unwrap());
        let overlap = solidcraft_kernel::boolean(&arc.body, &post.body, solidcraft_kernel::BoolOp::Intersect)
            .ok()
            .flatten()
            .map(|b| solidcraft_kernel::measure(&b).map(|m| m.volume).unwrap_or(0.0))
            .unwrap_or(0.0);
        assert!(overlap < 1e-3, "no overlap: {overlap}");
        let v = solidcraft_kernel::measure(&arc.body).unwrap().volume;
        let full = PI * (225.0 - 100.0) * 5.0;
        // The post is at a quarter turn one way (three quarters the other): the ring stops
        // short of the post's centre line, within its half-width of it.
        let turned = v / full * 360.0;
        let to_centre = if turned < 180.0 { 90.0 } else { 270.0 };
        assert!(turned < to_centre && turned > to_centre - 30.0, "{turned}°");
        return;
    }
    panic!("neither post was reached");
}

/// To Object picks the face that is at the picked point, not the first body with any face
/// near it. Here the pick is on a post (r 2.75) only 2 mm above a plate: the ridge must end
/// on the post. Before, the plate (within 5 % of its size of the pick) was taken as the
/// target, nothing trimmed the tool, and the ridge ran past the post.
#[test]
fn extrude_to_a_cylinder_beside_another_body() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-98, -5, 0], "length": 101, "width": 10, "height": 3, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "axis": [0, 0, 1], "radius": 2.75, "height": 10, "body_name": "Post"}));
    // A ridge above the plate, y -1.5..0, z 4..7, from the plate's far end towards the post.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [-98, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": "End"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-1.5, 4], "p1": [0, 7]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "End", "to": [-2.75, 0, 5], "operation": "new", "body_name": "Ridge"}));
    let b = body(&mut s, "Ridge");
    // Its end follows the post: farthest at the ridge's edge y = -1.5, x = -√(2.75² − 1.5²).
    let end = b["bbox"]["max"][0].as_f64().unwrap_or(f64::MAX);
    assert!((end + (2.75f64 * 2.75 - 1.5 * 1.5).sqrt()).abs() < 1e-3, "{b}");
    assert!((b["bbox"]["min"][0].as_f64().unwrap_or(0.0) + 98.0).abs() < 1e-6, "{b}");
}

/// A window cut To Object through the side of a tube (r 20 outside, 18 inside), up to the inside
/// face: it takes the wall under the profile and nothing else. Before, the tool ended in the
/// curved inner face: the tool less the tube kept only the air before the wall and cut nothing,
/// or the boolean failed on the coincident curved faces. Picking the outside face from inside
/// the wall cuts back to it.
#[test]
fn cut_to_the_far_side_of_a_curved_wall() {
    let tube = || {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 20, "body_name": "Tube"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Bore"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 18}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "Bore", "distance": 20, "operation": "cut"}));
        s
    };
    let volume = |s: &mut Session| body(s, "Tube")["volume_mm3"].as_f64().unwrap_or(0.0);
    let wall = |x: f64, r: f64| (r * r - x * x).sqrt();
    let n = 2000;
    let between = |r0: f64, r1: f64| -> f64 {
        (0..n).map(|i| -3.0 + 6.0 * (i as f64 + 0.5) / n as f64).map(|x| (wall(x, r1) - wall(x, r0)) * 4.0 * 6.0 / n as f64).sum()
    };
    // From outside the tube, through the wall, to its inside face.
    let mut s = tube();
    let whole = volume(&mut s);
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, -30, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Window"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-3, 8], "p1": [3, 12]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Window", "to": [0, -18, 10], "operation": "cut"}));
    let removed = whole - volume(&mut s);
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    // The same window cut 20 mm, ending in the bore: the tube's own facets set its volume.
    let mut plain = tube();
    run(&mut plain, "sketch.create", json!({"plane": {"origin": [0, -30, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Window"}));
    run(&mut plain, "sketch.rectangle.two_point", json!({"p0": [-3, 8], "p1": [3, 12]}));
    run(&mut plain, "sketch.finish", json!({}));
    run(&mut plain, "solid.extrude", json!({"sketch": "Window", "distance": -20, "operation": "cut"}));
    let reference = whole - volume(&mut plain);
    assert!((removed - reference).abs() < reference * 1e-6, "removed {removed}, a 20 mm cut {reference}");
    let want = between(18.0, 20.0);
    assert!((removed - want).abs() < want * 2e-2, "removed {removed}, want {want}");
    // From inside the wall (y = -19) back out to the outside face.
    let mut s = tube();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, -19, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Window"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-3, 8], "p1": [3, 12]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Window", "to": [0, -20, 10], "operation": "cut"}));
    let removed = whole - volume(&mut s);
    let want: f64 = (0..n).map(|i| -3.0 + 6.0 * (i as f64 + 0.5) / n as f64).map(|x| (wall(x, 20.0) - 19.0) * 4.0 * 6.0 / n as f64).sum();
    assert!((removed - want).abs() < want * 2e-2, "removed {removed}, want {want}");
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
}

/// A tube r 20 outside, 18 inside, 20 high, named "Tube".
fn tube() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 20, "body_name": "Tube"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Bore"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 18}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Bore", "distance": 20, "operation": "cut"}));
    s
}

/// A 6 x 4 window sketched on the plane y = `y`, facing the tube: x in [-3, 3], z in [8, 12].
fn window(s: &mut Session, y: f64) {
    run(s, "sketch.create", json!({"plane": {"origin": [0, y, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Window"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [-3, 8], "p1": [3, 12]}));
    run(s, "sketch.finish", json!({}));
}

/// The area between the x axis and the circle of radius `r` about Z over the window's x in
/// [-3, 3]: the integral of sqrt(r^2 - x^2), in closed form.
fn under_arc(r: f64) -> f64 {
    3.0 * (r * r - 9.0).sqrt() + r * r * (3.0 / r).asin()
}

/// The volume of the window's prism (4 tall) between the cylinders of radii `r0` and `r1` about
/// Z: the wall of the tube under the window is `wall_between(18, 20)`.
fn wall_between(r0: f64, r1: f64) -> f64 {
    4.0 * (under_arc(r1) - under_arc(r0))
}

/// The volume of the window's prism from y = -30 to the cylinder of radius `r` about Z.
fn post_to(r: f64) -> f64 {
    4.0 * (6.0 * 30.0 - under_arc(r))
}

/// How far a measured volume may lie from its closed form, relative: `volume_mm3` is SMLib's
/// exact measure of the B-rep (planes and cylinders here), not of the display mesh, so only its
/// numerical integration and the kernel's tolerance part them.
const EXACT: f64 = 1e-6;

/// #12617: a To offset on a curved face ends the extent that far past the face along the
/// travel, or that far short of it when negative, on the near face and on the far face alike.
/// Cuts from outside the tube: to the outside face 1 past it, and to the inside face 1 short
/// of it, both take the wall between r 19 and 20; to the inside face 1 past it takes the wall
/// (the bore is air); to the outside face 1 short of it takes nothing, with no error.
#[test]
fn a_to_offset_on_a_curved_face_ends_that_far_past_it() {
    let volume = |s: &mut Session| body(s, "Tube")["volume_mm3"].as_f64().unwrap_or(0.0);
    let cut = |to: [f64; 3], to_offset: f64| -> f64 {
        let mut s = tube();
        let whole = volume(&mut s);
        window(&mut s, -30.0);
        run(&mut s, "solid.extrude", json!({"sketch": "Window", "to": to, "to_offset": to_offset, "operation": "cut"}));
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "to {to:?} offset {to_offset}: {:?}", s.model.results);
        whole - volume(&mut s)
    };
    let outer_mm = wall_between(19.0, 20.0);
    let past_outer = cut([0.0, -20.0, 10.0], 1.0);
    assert!((past_outer - outer_mm).abs() < outer_mm * EXACT, "1 past the outside face removed {past_outer}, want {outer_mm}");
    let short_of_inner = cut([0.0, -18.0, 10.0], -1.0);
    assert!((short_of_inner - outer_mm).abs() < outer_mm * EXACT, "1 short of the inside face removed {short_of_inner}, want {outer_mm}");
    let wall = wall_between(18.0, 20.0);
    let past_inner = cut([0.0, -18.0, 10.0], 1.0);
    assert!((past_inner - wall).abs() < wall * EXACT, "1 past the inside face removed {past_inner}, want the wall {wall}");
    let short_of_outer = cut([0.0, -20.0, 10.0], -1.0);
    assert!(short_of_outer.abs() < 1e-6, "1 short of the outside face removed {short_of_outer}");
}

/// #12617: the input main once refused, kept as it was: a window sketched on y = -30, cut To the
/// tube's inside face at [0, -18, 10] with To offset 2. It builds now: the end lies 2 past the
/// inside face, in the bore, so the cut takes the wall under the window between r 18 and 20.
/// (The inside face's offset 2 the wrong way round lies exactly on the outside face.)
#[test]
fn a_to_offset_2_past_the_inside_face_of_a_tube_cuts_through_its_wall() {
    let mut s = tube();
    let whole = body(&mut s, "Tube")["volume_mm3"].as_f64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, -30, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-3, 8], "p1": [3, 12]}));
    run(&mut s, "sketch.finish", json!({}));
    s.execute("solid.extrude", &json!({"to": [0, -18, 10], "to_offset": 2.0, "operation": "cut"})).expect("the cut builds");
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let removed = whole - body(&mut s, "Tube")["volume_mm3"].as_f64().unwrap();
    let want = wall_between(18.0, 20.0);
    assert!((removed - want).abs() < want * EXACT, "removed {removed}, want the wall {want}");
}

/// #12617: a profile the offset surface misses is refused, not run out to the far end of the
/// extent: a window at x 19.2..19.8 crosses the tube's outside face (r 20) but not its offset 1
/// past it (r 19), so a New Body To that face with To offset 1 has nowhere to end.
#[test]
fn a_profile_the_offset_surface_misses_is_refused() {
    let mut s = tube();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, -30, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Edge"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [19.2, 8], "p1": [19.8, 12]}));
    run(&mut s, "sketch.finish", json!({}));
    let made =
        s.execute("solid.extrude", &json!({"sketch": "Edge", "to": [0, -20, 10], "to_offset": 1.0, "operation": "new_body", "body_name": "Post"}));
    assert!(made.is_err(), "a New Body the offset surface misses was made: {:?}", body(&mut s, "Post"));
}

/// #12617: a Join To Object on a curved face with a negative offset stops that far short of
/// the face: a post from y = -30 to the tube's outside face 2 short of it fills up to r 22.
#[test]
fn a_join_to_a_curved_face_with_an_offset_stops_that_far_short() {
    let volume = |s: &mut Session| body(s, "Tube")["volume_mm3"].as_f64().unwrap_or(0.0);
    let mut s = tube();
    let whole = volume(&mut s);
    window(&mut s, -30.0);
    run(&mut s, "solid.extrude", json!({"sketch": "Window", "to": [0.0, -20.0, 10.0], "to_offset": -2.0, "operation": "join"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let added = volume(&mut s) - whole;
    let want = post_to(22.0);
    assert!((added - want).abs() < want * EXACT, "added {added}, want {want}");
}

/// #12617: a New Body To Object on a curved face keeps its To offset too: a post from y = -30 to
/// the tube's outside face 2 short of it ends at r 22, not on the face.
#[test]
fn a_new_body_to_a_curved_face_with_an_offset_stops_that_far_short() {
    let mut s = tube();
    window(&mut s, -30.0);
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "Window", "to": [0.0, -20.0, 10.0], "to_offset": -2.0, "operation": "new_body", "body_name": "Post"}),
    );
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let made = body(&mut s, "Post")["volume_mm3"].as_f64().unwrap_or(0.0);
    let want = post_to(22.0);
    assert!((made - want).abs() < want * EXACT, "made {made}, want {want}");
}

/// A To offset that carries the end back across the start, away from a curved To face, ends at
/// that level as for a plane: the extrusion never meets the face, so it is not run through the
/// part and trimmed back to it. Here the counterbore's ceiling, extruded down to a point 1 mm up
/// the bore with an offset of -3, fills the counterbore from 1 mm to 3 mm. It used to keep the
/// whole run-through tool, a post far out of the plate's bottom, and SMLib refused that join on
/// an imported plate.
#[test]
fn a_to_offset_back_across_the_start_from_a_curved_face_ends_at_its_level() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-20, -20, 0], "length": 40, "width": 40, "height": 6, "body_name": "Plate"}));
    for (name, radius, depth) in [("Counterbore", 8.0, 3.0), ("Bore", 3.2, 6.0)] {
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": name}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": radius}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": name, "distance": depth, "operation": "cut"}));
    }
    let before = body(&mut s, "Plate")["volume_mm3"].as_f64().unwrap();
    let wall = [3.2 * 0.8_f64.cos(), 3.2 * 0.8_f64.sin(), 4.0];
    run(&mut s, "solid.extrude", json!({"face": [5, 0, 3], "to": wall, "to_offset": -3, "operation": "join"}));
    let b = body(&mut s, "Plate");
    assert!(b["bbox"]["min"][2].as_f64().unwrap().abs() < 1e-6, "nothing below the plate: {b}");
    let added = b["volume_mm3"].as_f64().unwrap() - before;
    let expected = PI * (8.0 * 8.0 - 3.2 * 3.2) * 2.0;
    assert!((added - expected).abs() / expected < 1e-3, "added {added}, expected {expected}");
}

/// #16713: a Cut To Object from a face of the body (the profile starts inside its material) up to
/// a curved face the body also has, a bore here, was refused with "nothing lies between the
/// profile and the To object": the tool less the body kept only the air beyond. It takes the
/// material between the profile and the bore's surface, as the preview shows.
#[test]
fn cut_to_a_curved_face_from_inside_the_material_takes_the_material_between() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 10, "body_name": "Block"}));
    // A bore along y, radius 2, its axis at z 6: its underside is 4 mm above the bottom.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, -10, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "Bore"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 6], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Bore", "distance": -20, "operation": "cut"}));
    let before = body(&mut s, "Block")["volume_mm3"].as_f64().unwrap_or(0.0);
    // A slot on the bottom face, 2 wide and 10 long, up to the bore's underside.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Slot"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-1, -5], "p1": [1, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Slot", "to": [0, 0, 4], "operation": "cut"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let removed = before - body(&mut s, "Block")["volume_mm3"].as_f64().unwrap_or(0.0);
    // The slot's material under the bore: over x in [-1, 1] the area between z = 0 and the
    // bore's underside z = 6 - sqrt(4 - x^2) is 12 - (sqrt(3) + 2 pi / 3) = 8.173554, times
    // the slot's 10 mm length.
    let want = 81.73554;
    assert!((removed - want).abs() < want * 1e-2, "removed {removed}, want {want}");
}

/// #16713: a Join To Object from the air up to a curved face, the underside of a bar, adds the
/// post between the profile and that surface: the bar and the post make one body.
#[test]
fn join_up_to_a_curved_face_adds_the_material_between() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [-20, 0, 30], "axis": [1, 0, 0], "radius": 10, "height": 40, "body_name": "Bar"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-4, -5], "p1": [4, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [0, 0, 20], "operation": "join"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let joined = body(&mut s, "Bar")["volume_mm3"].as_f64().unwrap_or(0.0);
    // The bar is 100 pi * 40 = 4000 pi. The post over x in [-4, 4], y in [-5, 5] runs from z = 0 up
    // to the bar's underside z = 30 - sqrt(100 - y^2): its section is
    // 300 - (25 sqrt(3) + 50 pi / 3) over y, so 8 * that = 2400 - 200 sqrt(3) - 400 pi / 3 =
    // 1634.7108 in volume. It lies wholly outside the bar, so the volumes add.
    let want = 4000.0 * PI + 1634.7108;
    assert!((joined - want).abs() < want * 1e-3, "joined {joined}, want {want}");
}

/// #17339: a Join To Object up to the side of a cone adds the post between the profile and that
/// side, whether the cone widens along its axis or narrows (its record's half angle is negative,
/// which the split at the To face's surface refused as "not supported yet").
#[test]
fn join_up_to_a_cone_that_narrows_or_widens_adds_the_material_between() {
    // Each case: the taper, and the volume the post adds. The post runs from y = -30 to the
    // cone's side y = -sqrt(r(z)^2 - x^2) with r(z) = 10 + z tan(taper), over x in [-2, 2] and z
    // in [8, 12]. Its section at height z is 4 * 30 less the circle's segment area over x in
    // [-2, 2], which is 2 sqrt(r^2 - 4) + r^2 asin(2 / r); the volume is that section integrated
    // over z from 8 to 12, worked out by hand. `side_y` is r(10) = 10 + 10 tan(taper), where the
    // cone's side meets the plane x = 0 at mid height.
    for (taper, side_y, want) in [(5.0f64, 10.874_886_635_259, 306.987_791_698_09), (-5.0f64, 9.125_113_364_741, 335.175_842_137_3)] {
        let mut s = Session::default();
        // A frustum of radius 10 at z 0, 20 tall: radius 10 + z tan(taper).
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "Base", "distance": 20, "taper": format!("{taper} deg"), "body_name": "Frustum"}));
        let before = body(&mut s, "Frustum")["volume_mm3"].as_f64().unwrap_or(0.0);
        // A 4 by 4 post from the plane y = -30 along +y, over x in [-2, 2] and z in [8, 12].
        run(&mut s, "sketch.create", json!({"plane": {"origin": [0, -30, 0], "x_dir": [-1, 0, 0], "y_dir": [0, 0, 1]}, "name": "P"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-2, 8], "p1": [2, 12]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [0, -side_y, 10], "operation": "join"}));
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "taper {taper}: {:?}", s.model.results);
        let added = body(&mut s, "Frustum")["volume_mm3"].as_f64().unwrap_or(0.0) - before;
        assert!((added - want).abs() < want * 1e-3, "taper {taper}: added {added}, want {want}");
    }
}

/// #16713: a profile that only partly lies over the To face is refused, not run on past it.
#[test]
fn cut_to_a_curved_face_the_profile_partly_misses_is_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [-20, 0, 30], "axis": [1, 0, 0], "radius": 10, "height": 40, "body_name": "Bar"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    // 24 across the bar's 20: the profile reaches past its side.
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-4, -12], "p1": [4, 12]}));
    run(&mut s, "sketch.finish", json!({}));
    let err = s.execute("solid.extrude", &json!({"sketch": "P", "to": [0, 0, 20], "body_name": "Post"})).expect_err("refused");
    assert!(err.to_string().contains("reaches past the To face"), "{err}");
}

/// #20389: a Cut To Object up to a round from the far side of the part ends at the round, not
/// where the tool first meets the round's cylinder extended (its back, inside the part, where
/// there is no round), with a To offset and with none (the profile overhangs the round, 1 below
/// the plate). A 40 by 30 by 5 plate with its corner at (40, 30) rounded R10, and a cut from the
/// plane x = -10 along +x over y in [22, 28] and z in [-1, 4]: in the plate, z in [0, 4], it
/// takes x from 0 to the round's offset cylinder x = 30 + sqrt(r^2 - (y - 20)^2), r = 10 + the
/// offset, so 4 times 6 * 30 plus the integral of sqrt(r^2 - u^2) over u in [2, 8]. Ending at
/// the cylinder's back, x = 30 - sqrt(...), it took 4 times 6 * 30 less that integral. The same
/// profile over z in [1, 4] lies wholly over the round: 3 times as much, with an offset and with
/// none (with none it ended at the plate's side x = 0, on the review of #20536).
#[test]
fn cut_to_a_round_from_the_far_side_ends_at_the_round() {
    // The profile from z = -1 overhangs the round below the plate; from z = 1 it lies wholly
    // over it, and the cut is 3 deep in z instead of 4.
    for (bottom, depth, to_offset) in [(-1, 4.0, -1.57f64), (-1, 4.0, 0.0), (1, 3.0, -1.57), (1, 3.0, 0.0)] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "Base", "distance": 5, "body_name": "Plate"}));
        run(&mut s, "solid.fillet", json!({"body": "Plate", "edges": [[40, 30, 2.5]], "radius": 10}));
        let before = body(&mut s, "Plate")["volume_mm3"].as_f64().unwrap_or(0.0);
        run(&mut s, "sketch.create", json!({"plane": {"origin": [-10, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": "P"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [22, bottom], "p1": [28, 4]}));
        run(&mut s, "sketch.finish", json!({}));
        let d = 10.0 / 2f64.sqrt();
        run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [30.0 + d, 20.0 + d, 2.5], "to_offset": to_offset, "operation": "cut"}));
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "z from {bottom}, offset {to_offset}: {:?}", s.model.results);
        let removed = before - body(&mut s, "Plate")["volume_mm3"].as_f64().unwrap_or(0.0);
        let r = 10.0 + to_offset;
        let primitive = |u: f64| u / 2.0 * (r * r - u * u).sqrt() + r * r / 2.0 * (u / r).asin();
        let want = depth * (180.0 + primitive(8.0) - primitive(2.0));
        assert!((removed - want).abs() < want * 1e-3, "z from {bottom}, offset {to_offset}: removed {removed}, want {want}");
    }
}

/// #20389: a New Body To Object up to a round from the far side of the part is one body, not
/// the piece up to the back of the round's cylinder and the piece beyond it, apart at a seam on
/// that cylinder. The plate and profile of `cut_to_a_round_from_the_far_side_ends_at_the_round`,
/// no offset: the body runs from x = -10 to the round, x = 30 + sqrt(100 - (y - 20)^2), over y in
/// [22, 28] and z in [-1, 4], so it holds 5 times 6 * 40 plus the integral of sqrt(100 - u^2)
/// over u in [2, 8].
#[test]
fn a_new_body_to_a_round_from_the_far_side_is_one_body() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Base", "distance": 5, "body_name": "Plate"}));
    run(&mut s, "solid.fillet", json!({"body": "Plate", "edges": [[40, 30, 2.5]], "radius": 10}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [-10, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": "P"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [22, -1], "p1": [28, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    let d = 10.0 / 2f64.sqrt();
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [30.0 + d, 20.0 + d, 2.5], "operation": "new_body", "body_name": "Post"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    assert_eq!(s.model.state().bodies.len(), 2, "the plate and one post");
    let primitive = |u: f64| u / 2.0 * (100.0 - u * u).sqrt() + 50.0 * (u / 10.0).asin();
    let want = 5.0 * (240.0 + primitive(8.0) - primitive(2.0));
    let got = body(&mut s, "Post")["volume_mm3"].as_f64().unwrap_or(0.0);
    assert!((got - want).abs() < want * 1e-3, "post {got}, want {want}");
}

/// A profile that reaches past the edges of a curved To face, where the face's surface extended
/// still lies across it, ends at that extended surface with no To offset as it does with one: a
/// 40 by 30 by 5 plate with its corner at (40, 30) rounded R10, and a Join from the plane x = 60
/// back along -x to the round, over y in [22, 28] and z in [-1, 4], 1 below the plate. It was
/// refused ("the profile reaches past the To face") while a To offset of 0.5 built. The post runs
/// from x = 60 to the round's cylinder x = 30 + sqrt(100 - (y - 20)^2) over all 5 of its height
/// (below the plate, to the cylinder extended), so it adds 5 times 6 * 30 less the integral of
/// sqrt(100 - u^2) over u in [2, 8].
#[test]
fn join_to_a_round_the_profile_overhangs_ends_at_its_extended_surface() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Base", "distance": 5, "body_name": "Plate"}));
    run(&mut s, "solid.fillet", json!({"body": "Plate", "edges": [[40, 30, 2.5]], "radius": 10}));
    let before = body(&mut s, "Plate")["volume_mm3"].as_f64().unwrap_or(0.0);
    // The sketch's x runs along -y, so its normal is -x.
    run(&mut s, "sketch.create", json!({"plane": {"origin": [60, 0, 0], "x_dir": [0, -1, 0], "y_dir": [0, 0, 1]}, "name": "P"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-28, -1], "p1": [-22, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    let d = 10.0 / 2f64.sqrt();
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [30.0 + d, 20.0 + d, 2.5], "operation": "join"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let added = body(&mut s, "Plate")["volume_mm3"].as_f64().unwrap_or(0.0) - before;
    let primitive = |u: f64| u / 2.0 * (100.0 - u * u).sqrt() + 50.0 * (u / 10.0).asin();
    let want = 5.0 * (180.0 - (primitive(8.0) - primitive(2.0)));
    assert!((added - want).abs() < want * 1e-3, "added {added}, want {want}");
}

/// #17707: two cylinders of radius 15 whose axes cross at 60°, joined. The tilted one's end face
/// lies 5 mm from where the axes cross, so the other cylinder cuts it and leaves two crescents
/// outside. Each crescent, extruded as a Join To Object up to the first cylinder's wall, adds the
/// sliver between them. The seams of the two cylinders end a few hundredths of a millimetre from
/// the crescents' tips, so each crescent has one very short projected edge; read as a straight
/// line, it put a flat side 1e-5 mm off the cylinder under the tool, and SMLib's check refused
/// the split (profile 1) or the join (profile 0).
///
/// Before the join the body has 8 faces, 18 edges and 12 vertices. The join takes the crescent's
/// face away (one face fewer) and its three edges: its arc of the end circle and the ellipse where
/// the end plane meets the first cylinder, one of them in two pieces where a seam ends on it.
/// The tilted cylinder's wall runs on down to the first cylinder and meets it in two ellipse arcs
/// that cross where the two walls touch, (±15, -75, 0) on the crescent's side: a new vertex
/// there. One seam, the tilted cylinder's (profile 0) or the first cylinder's (profile 1), now
/// ends on those arcs and splits one of them: three new edges. So 7 faces, 18 edges, 13 vertices,
/// one shell.
///
/// The sliver's volume is the integral over the crescent of the distance from the end face, along
/// the tilted axis, to the first cylinder's wall. In the end face's frame (u along x, v square to
/// it in the face, c = 5 sqrt(3) / 2 the height of the face's centre above the first axis) the
/// crescent is u^2 + v^2 <= 225 with u^2 + (c + v / 2)^2 >= 225, v from (c - sqrt 75) / 1.5 to
/// (c + sqrt 75) / 1.5, and the distance is (c + v / 2 - sqrt(225 - u^2)) / (sqrt(3) / 2). The
/// integral over u is closed form, the one over v by quadrature: 8.3467156 mm³.
#[test]
fn join_up_to_a_cylinder_from_a_face_it_cuts_adds_the_sliver_between() {
    let topology = |s: &Session| {
        let state = s.model.state();
        let b = &state.body("Body1").unwrap().body;
        (state.bodies.len(), b.face_count(), b.edge_count(), b.vertex_count(), b.record().shells.len())
    };
    // Whether the body has a vertex where the walls touch on the side x of sign `x`.
    let touch = |s: &Session, x: f64| {
        let state = s.model.state();
        let b = &state.body("Body1").unwrap().body;
        b.record().vertices.iter().any(|v| [v.point[0] - 15. * x, v.point[1] + 75., v.point[2]].iter().all(|d| d.abs() < 1e-6))
    };
    // Profile 0 is the crescent on +x, profile 1 the one on -x.
    for (profile, x) in [(0, 1.), (1, -1.)] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 15, "height": 40, "base": [0, -100, 0], "axis": "Y", "operation": "new"}));
        run(
            &mut s,
            "solid.cylinder",
            json!({"radius": 15, "height": 35, "base": [0, -77.5, 4.330127018922193], "axis": [0, -0.5, 0.8660254037844386], "operation": "join"}),
        );
        run(&mut s, "sketch.create", json!({"plane": {"face": [-14.5, -77.5, 4.330127018922193]}, "project_edges": true, "name": "S"}));
        run(&mut s, "sketch.finish", json!({}));
        let volume = |s: &Session| s.model.state().body("Body1").and_then(|b| b.body.volume_at(1e-4)).unwrap_or(f64::NAN);
        let before = volume(&s);
        assert_eq!(topology(&s), (1, 8, 18, 12, 1), "profile {profile}: before the join");
        assert!(!touch(&s, 1.) && !touch(&s, -1.), "profile {profile}: no vertex where the walls touch yet");
        let to = [-14.931150677683434, -78.64377441378278, -0.7504228399786044];
        let r = s.execute("solid.extrude", &json!({"to": to, "direction": "positive", "operation": "join", "profiles": [profile]}));
        assert!(r.is_ok() && s.model.results.iter().all(|r| r.error.is_none()), "profile {profile}: {r:?} {:?}", s.model.results);
        assert_eq!(topology(&s), (1, 7, 18, 13, 1), "profile {profile}: bodies, faces, edges, vertices, shells");
        assert!(touch(&s, x) && !touch(&s, -x), "profile {profile}: the new vertex is where the walls touch on its side");
        let (added, want) = (volume(&s) - before, 8.346_715_6);
        assert!((added - want).abs() < want * 1e-5, "profile {profile}: added {added}, want {want}");
    }
}

/// #13188: a stored taper of 0 is a straight extrusion, also for a profile with a hole.
#[test]
fn extrude_with_a_taper_of_zero_on_a_profile_with_a_hole_is_straight() {
    let volume = |taper: Option<&str>| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"distance": 10, "body_name": "Plate"});
        if let Some(t) = taper {
            p["taper"] = json!(t);
        }
        run(&mut s, "solid.extrude", p);
        body(&mut s, "Plate")["volume_mm3"].as_f64().unwrap()
    };
    let plain = volume(None);
    // Rectangle less the hole, 10 mm tall; the circle is faceted, so within a thousandth.
    let expected = (40.0 * 30.0 - PI * 25.0) * 10.0;
    assert!((plain - expected).abs() / expected < 1e-3, "{plain} vs {expected}");
    let zero = volume(Some("0 deg"));
    assert!((zero - plain).abs() < 1e-6 * plain, "taper 0 gives {zero}, none gives {plain}");
}

fn through_all_block() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-20, -20, -30], "length": 60, "width": 60, "height": 60, "body_name": "Blk"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

/// Through All is measured from where the extrude starts, so a start offset leaves no slab
/// uncut on either side (#16556).
#[test]
fn through_all_cut_with_a_start_offset_goes_all_the_way() {
    for direction in ["symmetric", "positive", "negative"] {
        for offset in [5, -5, 0, 40, -40] {
            let mut s = through_all_block();
            run(
                &mut s,
                "solid.extrude",
                json!({"sketch": "Sketch1", "through_all": true, "direction": direction, "start_offset": offset, "operation": "cut", "targets": ["Blk"]}),
            );
            let volume = body(&mut s, "Blk")["volume_mm3"].as_f64().unwrap();
            // A one-sided cut only removes what lies on its side of the start.
            let removed = 216000.0 - volume;
            let expected = match direction {
                "symmetric" => 24000.0,
                "positive" => 400.0 * (30.0 - f64::from(offset)).clamp(0.0, 60.0),
                _ => 400.0 * (f64::from(offset) + 30.0).clamp(0.0, 60.0),
            };
            assert!((removed - expected).abs() < 1e-3, "{direction} offset {offset}: removed {removed}, expected {expected}");
        }
    }
}

/// #16780: From and To points at two planes (not faces) above a board, the To plane within the
/// board's pick tolerance of its top face, run from the From plane to the To plane. The To plane
/// was taken for the board's top face, which is where the extrude starts, and the extrude refused.
#[test]
fn extrude_from_a_plane_to_a_plane_near_a_face_runs_between_the_planes() {
    for operation in ["new", "join"] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"corner": [-30, -30, 0], "length": 60, "width": 60, "height": 1.3, "body_name": "Board"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "P", "from": [0, 0, 1.3], "to": [0, 0, 3.6], "operation": operation, "body_name": "Glass"}));
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{operation}: {:?}", s.model.results);
        let name = if operation == "join" { "Board" } else { "Glass" };
        let b = body(&mut s, name);
        let top = b["bbox"]["max"][2].as_f64().unwrap_or(0.0);
        assert!((top - 3.6).abs() < 1e-6, "{operation}: top {top}");
        let glass = 400.0 * 2.3;
        let want = if operation == "join" { 3600.0 * 1.3 + glass } else { glass };
        let volume = b["volume_mm3"].as_f64().unwrap_or(0.0);
        assert!((volume - want).abs() < 1e-3 * want, "{operation}: volume {volume}, want {want}");
    }
}

/// A To point on the edge of a hole in a vertical wall (a vertex there) ends the extrude at that
/// point's height, whichever faces meet there (#12968).
#[test]
fn extrude_to_a_vertex_on_a_hole_in_a_wall_ends_at_its_height() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 400], "length": 100, "width": 60, "height": 250, "body_name": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [100, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}}));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 640], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "20 mm", "direction": "negative", "operation": "cut", "targets": ["Base"]}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 650], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [100, 0], "p1": [120, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"to": [100, 35, 640], "operation": "new", "body_name": "T"}));
    let b = body(&mut s, "T");
    let z = |k: &str| b["bbox"][k][2].as_f64().unwrap_or(f64::NAN);
    assert!((z("min") - 640.0).abs() < 1e-6 && (z("max") - 650.0).abs() < 1e-6, "{b}");
}

/// A design saved before #12968's fix named the wall on its To point: on a reload the point stays
/// on the vertex instead of moving into the wall, so the post still ends at 640.
#[test]
fn a_to_vertex_saved_with_the_walls_name_still_ends_at_its_height() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 400], "length": 100, "width": 60, "height": 250, "body_name": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [100, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}}));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 640], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "20 mm", "direction": "negative", "operation": "cut", "targets": ["Base"]}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 650], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [100, 0], "p1": [120, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"to": [100, 35, 640], "operation": "new", "body_name": "T"}));
    // The name a save before the fix gave the point: the wall's, as a point inside it gets.
    let f = s.doc.features.last().unwrap().clone();
    let mut kind = f.kind.clone();
    for p in kind.face_points_mut() {
        *p = solidcore_record::Vec3::new(100.0, 35.0, 630.0);
    }
    let wall = solidcraft_doc::face_point_names(&s.doc, &s.model.state(), &kind, f.component).unwrap();
    assert_eq!(wall, ["F1:+x"]);
    let mut doc: solidcraft_doc::Document = (*s.doc).clone();
    doc.features.last_mut().unwrap().face_names = wall;
    let mut cold = Session::new(solidcraft_doc::Document::from_json(&doc.to_json()).unwrap());
    let b = body(&mut cold, "T");
    let z = |k: &str| b["bbox"][k][2].as_f64().unwrap_or(f64::NAN);
    assert!((z("min") - 640.0).abs() < 1e-6 && (z("max") - 650.0).abs() < 1e-6, "{b}");
}

/// #15413: a stored taper of 0 (the Extrude dialog saves one) names the caps and sides as a
/// straight extrude's are, so the top and bottom of an extrude with a start offset, from the
/// sketch plane or from a From Object face, take a sketch that follows the offset.
#[test]
fn the_faces_of_an_extrude_with_a_taper_of_zero_and_a_start_offset_take_a_sketch() {
    use solidcraft_doc::config::FaceCertificate;
    use solidcraft_geom::PlaneGeom;
    for from in [None, Some(json!([20, 15, 10]))] {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "lift", "expression": "20 mm"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Base"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "PadSketch"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 5], "p1": [30, 25]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"sketch": "PadSketch", "distance": 10, "start_offset": "lift", "taper": "0 deg", "body_name": "Pad"});
        if let Some(from) = &from {
            p["from"] = from.clone();
        }
        let pad = run(&mut s, "solid.extrude", p)["feature"].as_u64().unwrap();
        // The Base's top (z = 10) when the extrude starts from it, else the sketch plane (z = 0).
        let level = if from.is_some() { 10.0 } else { 0.0 };
        // Bottom, top and the side along x = 30: which cap (`Some(high)`) or the side (`None`),
        // a point on the face, and its height above the start level less the lift.
        let faces = [("OnBottom", Some(false), 20.0, 0.0), ("OnTop", Some(true), 20.0, 10.0), ("OnSide", None, 30.0, 5.0)];
        let mut sketches = Vec::new();
        for (name, cap, x, rise) in faces {
            let p = json!({"plane": {"face": [x, 15.0, level + 20.0 + rise]}, "name": name});
            let id = s.execute("sketch.create", &p).unwrap_or_else(|e| panic!("from {from:?}, {name}: {e}"))["sketch"].as_u64().unwrap();
            run(&mut s, "sketch.finish", json!({}));
            let solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { anchor: Some(anchor), .. }, .. } =
                &s.doc.feature(id).unwrap().kind
            else {
                panic!("from {from:?}, {name}: the sketch is not attached to the face");
            };
            assert_eq!(anchor.target.origin_feature, pad, "from {from:?}, {name}");
            let certified = match &anchor.target.certificate {
                Some(FaceCertificate::ExtrudeCap { high, .. }) => cap == Some(*high),
                Some(FaceCertificate::ExtrudeSide { .. }) => cap.is_none(),
                _ => false,
            };
            assert!(certified, "from {from:?}, {name}: {:?}", anchor.target.certificate);
            sketches.push((name, cap, id, rise));
        }
        // A bigger lift moves the pad up 5 mm: each sketch goes with its face, also in the reopened design.
        run(&mut s, "parameters.change", json!({"name": "lift", "expression": "25 mm"}));
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        reopened.refresh();
        for current in [&s, &reopened] {
            assert!(current.model.results.iter().all(|r| r.error.is_none()), "from {from:?}: {:?}", current.model.results);
            let state = current.model.state();
            for (name, cap, id, rise) in &sketches {
                let plane = state.sketch(*id).unwrap().plane;
                if cap.is_none() {
                    assert!((plane.origin.x - 30.0).abs() < 1e-9 && plane.normal().x.abs() > 1.0 - 1e-12, "from {from:?}, {name}: {plane:?}");
                } else {
                    assert!((plane.origin.z - (level + 25.0 + rise)).abs() < 1e-9, "from {from:?}, {name}: {plane:?}");
                }
            }
        }
    }
}
