//! #1080: a fillet or chamfer running past the faces beside its edge into an opening is built
//! as if the faces went on and cut by the opening (the blend-overflow decision, 237f0ae74), so it
//! never bridges it; one that stops short builds as it always did.

use std::f64::consts::PI;
use std::path::Path;

use serde_json::json;

use super::{run, volume};
use crate::Session;

fn open_fixture() -> Session {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/cubeish-chamfer-too-large.sc");
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    s
}

fn chamfer_error(s: &Session) -> Option<String> {
    s.model.results.iter().find(|r| r.name == "Chamfer1").and_then(|r| r.error.clone())
}

/// The report's design: a 20 mm cube with a 10 mm square opening into each of three faces,
/// then a 9 mm chamfer on the top edge over one side opening. The openings are 5 mm from the
/// edge, so the chamfer runs over their rims: it is cut by them. Of its section, the triangle
/// y + (20 − z) <= 9 (area 40.5, along 20: 810), the two openings crossing it hold 80 each
/// (the rig's fillets/1080-chamfer-runs-into-openings-cut-by-them): 4000 − (810 − 160) = 3350.
#[test]
fn chamfer_through_the_openings_is_cut_by_them() {
    let mut s = open_fixture();
    assert_eq!(chamfer_error(&s), None);
    assert!((volume(&mut s) - 3350.0).abs() < 1e-6, "{}", volume(&mut s));
}

/// Sizes that stop short of the openings build, and take away exactly the chamfer's triangle
/// along the 20 mm edge.
#[test]
fn chamfer_that_fits_still_builds() {
    let mut s = open_fixture();
    for d in [4.0, 5.0] {
        run(&mut s, "timeline.edit", json!({"feature": "Chamfer1", "set": {"distance": format!("{d} mm")}}));
        assert_eq!(chamfer_error(&s), None, "{d} mm fits");
        let want = 4000.0 - 0.5 * d * d * 20.0;
        assert!((volume(&mut s) - want).abs() < 1e-6, "{d} mm: {} (want {want})", volume(&mut s));
    }
}

/// #199's bar, 14 x 6 x 10, its four upright edges rounded: past a full round of its 6 mm ends
/// the two rounds on each end would cross. Refused, naming 3 mm: the kernel's refusal carries a
/// size and the edge it was found for (`SolidError::FilletRefused`), and the message names the
/// edge.
#[test]
fn a_round_past_a_full_round_is_refused_naming_the_largest_that_fits_and_its_edge() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 14, "width": 6, "height": 10, "corner": [14, 0, 0]}));
    let uprights = json!([[14, 0, 5], [28, 0, 5], [14, 6, 5], [28, 6, 5]]);
    for r in [3.001, 3.5] {
        let e = s.execute("solid.fillet", &json!({"edges": uprights, "radius": r})).unwrap_err().to_string();
        assert!(e.contains("the largest radius that fits is 3 mm"), "{r}: {e}");
        assert!(e.contains("allow, at the edge at ("), "{r}: {e}");
        assert_eq!(e.matches("that fits is").count(), 1, "{r}: {e}");
    }
    assert!((volume(&mut s) - 840.0).abs() < 1e-6, "the part stays as it was");
}

/// A rib 4 mm wide: blends of its two top edges meet in its top's middle at 2 mm and past that
/// would cross each other. Refused as too large, naming the largest size that fits.
#[test]
fn blends_crossing_each_other_are_refused_naming_the_largest_that_fits() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 4, "height": 20}));
    let edges = json!([[20, 0, 20], [20, 4, 20]]);
    let e = s.execute("solid.chamfer", &json!({"edges": edges, "distance": 3})).unwrap_err().to_string();
    assert!(e.contains("the largest distance that fits is 2 mm"), "{e}");
    let e = s.execute("solid.fillet", &json!({"edges": edges, "radius": 3})).unwrap_err().to_string();
    assert!(e.contains("the largest radius that fits is 2 mm"), "{e}");
    assert!((volume(&mut s) - 3200.0).abs() < 1e-6, "the part stays as it was");
}

/// #10849: an oversized `solid.fillet` reports SMLib's largest fitting radius directly in its
/// error payload (without searching by retrying builds), and applying that radius builds cleanly.
#[test]
fn oversized_fillet_reports_smlib_fit_size_that_applies_cleanly() {
    use solidcraft_kernel::BlendFit;

    // 14 x 6 x 10 bar, four upright edges: SMLib reports 3 mm directly, and applying 3 mm builds
    // the full round cleanly.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 14, "width": 6, "height": 10, "corner": [14, 0, 0]}));
    let uprights = json!([[14, 0, 5], [28, 0, 5], [14, 6, 5], [28, 6, 5]]);
    let err = s.execute("solid.fillet", &json!({"edges": uprights, "radius": 3.5})).unwrap_err();
    let Some(BlendFit::Radius(fit)) = err.largest_fit() else {
        panic!("expected a largest fitting radius: {err}");
    };
    assert!((fit - 3.0).abs() < 1e-9, "got {fit}: {err}");
    run(&mut s, "solid.fillet", json!({"edges": uprights, "radius": fit}));
    let full = 840.0 - (4.0 - std::f64::consts::PI) * 9.0 * 10.0;
    assert!((volume(&mut s) - full).abs() / full < 2e-4, "{} vs {full}", volume(&mut s));

    // 80 x 15 x 20 bar, two upright edges on the 15 mm end: SMLib reports the open-margin fit
    // size just below 7.5 mm directly, and applying it builds cleanly.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 15, "height": 20}));
    let end_edges = json!([[80, 0, 10], [80, 15, 10]]);
    let err = s.execute("solid.fillet", &json!({"edges": end_edges, "radius": 10})).unwrap_err();
    let Some(BlendFit::Radius(fit)) = err.largest_fit() else {
        panic!("expected a largest fitting radius: {err}");
    };
    assert!(fit > 7.4 && fit <= 7.5, "got {fit}: {err}");
    run(&mut s, "solid.fillet", json!({"edges": end_edges, "radius": fit}));
    assert!(volume(&mut s) < 24000.0 && volume(&mut s) > 20000.0, "{}", volume(&mut s));
}

// ---- fit: keep_edge: a concave round taller than its wall ends on
// the wall's top edge ----

/// A 20 x 20 plate, 2 thick (z from -2 to 0), with a rib 2 wide (x) and `h` high across it, its
/// ends flush with the plate's. The wall edges at its foot are [1, 0, 0] and [-1, 0, 0].
fn rib_on_plate(h: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10.0, -10.0], "p1": [10.0, 10.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2, "operation": "new", "direction": "negative", "body_name": "Main"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-1.0, -10.0], "p1": [1.0, 10.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": h, "operation": "join"}));
    s
}

/// What the keep-edge round adds per edge and mm of a wall `h` high at radius `r`: the section
/// between the wall, the floor and the circle of radius r tangent to the floor through the wall's
/// top edge, its centre c = sqrt(r^2 - (r - h)^2) from the wall. Integrated over the height, the
/// width the circle leaves at height z is c - sqrt(r^2 - (r - z)^2), which is
/// c h - (r^2 theta - (r - h) c) / 2 with theta = asin(c / r), the closed form
/// r c - (c (r - h) + r^2 theta) / 2 of the issue's figures.
fn keep_edge_section(r: f64, h: f64) -> f64 {
    let c = (r * r - (r - h) * (r - h)).sqrt();
    r * c - (c * (r - h) + r * r * (c / r).asin()) / 2.0
}

/// What the ordinary round adds (the wall's plane carried on, the top face grown to meet it) per
/// edge and mm: the same region and the strip between the wall and the circle's top, (r - c) h.
fn carried_on_section(r: f64, h: f64) -> f64 {
    let c = (r * r - (r - h) * (r - h)).sqrt();
    keep_edge_section(r, h) + (r - c) * h
}

fn keep_edge(edges: serde_json::Value, r: f64) -> serde_json::Value {
    json!({"edges": edges, "radius": r, "fit": "keep_edge", "tangent_chain": false})
}

#[test]
fn keep_edge_section_matches_the_issue_figures() {
    assert!((keep_edge_section(1.25, 1.0) - 0.3079642).abs() < 1e-7);
    assert!((carried_on_section(1.25, 1.0) - 0.3332194).abs() < 1e-7);
}

/// A single wall, the radius just over its height and well over it.
#[test]
fn keep_edge_round_on_a_single_wall_adds_the_circle_section() {
    for r in [1.02, 1.25, 2.0, 5.0] {
        let mut s = rib_on_plate(1.0);
        run(&mut s, "solid.fillet", keep_edge(json!([[1, 0, 0]]), r));
        let want = 840.0 + 20.0 * keep_edge_section(r, 1.0);
        let got = volume(&mut s);
        assert!((got - want).abs() < 1e-6, "R {r}: {got} vs {want}");
        let state = s.model.state();
        assert!(state.body("Main").unwrap().body.validity().is_empty());
    }
}

/// Both edges of a rib in one fillet, which the ordinary round refuses, are
/// 840 + 2 * 20 * 0.3079642 = 852.3185690 mm3.
#[test]
fn keep_edge_round_on_both_edges_of_a_rib() {
    let mut s = rib_on_plate(1.0);
    assert!(s.execute("solid.fillet", &json!({"edges": [[1, 0, 0], [-1, 0, 0]], "radius": 1.25})).is_err(), "the ordinary round refuses it");
    run(&mut s, "solid.fillet", keep_edge(json!([[1, 0, 0], [-1, 0, 0]]), 1.25));
    let got = volume(&mut s);
    assert!((got - 852.318_569_0).abs() < 1e-6, "{got}");
    assert!((got - (840.0 + 40.0 * keep_edge_section(1.25, 1.0))).abs() < 1e-9);
    // With the default tangent chain the same edges give the same body.
    let mut t = rib_on_plate(1.0);
    run(&mut t, "solid.fillet", json!({"edges": [[1, 0, 0], [-1, 0, 0]], "radius": 1.25, "fit": "keep_edge"}));
    assert!((volume(&mut t) - got).abs() < 1e-9);
}

/// A wall as high as the radius, or higher, is rounded as usual; so is an edge that is not
/// concave. The round tangent to both faces adds (1 - pi/4) r^2 per mm.
#[test]
fn keep_edge_leaves_other_edges_to_the_ordinary_round() {
    let mut s = rib_on_plate(1.0);
    run(&mut s, "solid.fillet", keep_edge(json!([[1, 0, 0], [-1, 0, 0]]), 1.0));
    let want = 840.0 + 40.0 * (1.0 - PI / 4.0);
    assert!((volume(&mut s) - want).abs() < 1e-6, "{} vs {want}", volume(&mut s));
}

/// The default is unchanged: no `fit` saves no `fit`, and `keep_edge` is saved by name.
#[test]
fn keep_edge_is_saved_reloaded_undone_and_redone() {
    let mut s = rib_on_plate(1.0);
    let before = volume(&mut s);
    let out = run(&mut s, "solid.fillet", keep_edge(json!([[1, 0, 0], [-1, 0, 0]]), 1.25));
    let id = out["feature"].as_u64().unwrap();
    let after = volume(&mut s);
    assert!(after > before + 12.0);
    let json = serde_json::to_string(&s.doc.features).unwrap();
    assert_eq!(json.matches("\"fit\":\"keep_edge\"").count(), 1, "{json}");
    assert!(run(&mut s, "timeline.inspect", json!({"feature": id}))["error"].is_null());

    let bytes = crate::io::write_design_container(&s.doc).unwrap();
    let mut reopened = Session::new(crate::io::read_design(&bytes).unwrap());
    assert!((volume(&mut reopened) - after).abs() < 1e-9);

    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - before).abs() < 1e-9);
    run(&mut s, "edit.redo", json!({}));
    assert!((volume(&mut s) - after).abs() < 1e-9);
    // Without `fit` the fillet is the ordinary one and saves none.
    let mut plain = rib_on_plate(1.0);
    run(&mut plain, "solid.fillet", json!({"edges": [[1, 0, 0]], "radius": 0.5}));
    assert!(!serde_json::to_string(&plain.doc.features).unwrap().contains("keep_edge"));
}

/// A top face that is curved (a cylinder about the rib's length) with the top edge straight: the
/// round meets the top edge, so the section is the same circle's. The rib's top bulges 0.2 over
/// the chord of 2.
#[test]
fn keep_edge_round_under_a_curved_top_face() {
    let (h, sag, r) = (1.0_f64, 0.2_f64, 1.25_f64);
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10.0, -10.0], "p1": [10.0, 10.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2, "operation": "new", "direction": "negative", "body_name": "Main"}));
    // The rib's section in the plane through y = 10 (x right, z up), run along -y over 20.
    let rc = (1.0 + sag * sag) / (2.0 * sag);
    let zc = h + sag - rc;
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 10, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}}));
    run(&mut s, "sketch.line", json!({"points": [[-1.0, 0.0], [1.0, 0.0], [1.0, h]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [0.0, zc], "start": [1.0, h], "end": [-1.0, h]}));
    run(&mut s, "sketch.line", json!({"points": [[-1.0, h], [-1.0, 0.0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20, "operation": "join"}));
    let segment = rc * rc * ((rc - sag) / rc).acos() - (rc - sag) * 1.0;
    let rib = (2.0 * h + segment) * 20.0;
    let joined = volume(&mut s);
    assert!((joined - (800.0 + rib)).abs() < 1e-6, "{joined} vs {}", 800.0 + rib);
    run(&mut s, "solid.fillet", keep_edge(json!([[1, 0, 0], [-1, 0, 0]]), r));
    let want = joined + 40.0 * keep_edge_section(r, h);
    assert!((volume(&mut s) - want).abs() < 1e-6, "{} vs {want}", volume(&mut s));
}

/// a ring (radius 13 to 14, 2 thick) with a tab 1 high over 0.8 rad across it, radial side
/// walls, its ends on the ring's own cylinders. The round on one wall is cut by those cylinders:
/// at a distance x from the wall's plane, which passes through the ring's axis, the ring is
/// sqrt(14^2 - x^2) - sqrt(13^2 - x^2) long, so the round adds the integral over its section
/// (x from 0 to c - sqrt(r^2 - (r - z)^2) at height z) of that length.
#[test]
fn keep_edge_round_between_curved_ends() {
    let (r, h) = (1.02_f64, 1.0_f64);
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 14}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 13}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2, "operation": "new", "direction": "negative", "body_name": "Main"}));
    let (c4, s4) = (0.4_f64.cos(), 0.4_f64.sin());
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[13.0 * c4, 13.0 * s4], [14.0 * c4, 14.0 * s4]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [14.0 * c4, -14.0 * s4], "end": [14.0 * c4, 14.0 * s4]}));
    run(&mut s, "sketch.line", json!({"points": [[14.0 * c4, -14.0 * s4], [13.0 * c4, -13.0 * s4]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [13.0 * c4, -13.0 * s4], "end": [13.0 * c4, 13.0 * s4]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": h, "operation": "join"}));
    let joined = volume(&mut s);
    let at = [13.5 * c4, 13.5 * s4, 0.0];
    assert!(s.execute("solid.fillet", &json!({"edges": [at], "radius": r, "tangent_chain": false})).is_err(), "the ordinary round is refused");
    run(&mut s, "solid.fillet", keep_edge(json!([at]), r));
    // sum over the round's section of the ring's length at each distance from the wall
    let c = (r * r - (r - h) * (r - h)).sqrt();
    let prim = |rho: f64, x: f64| 0.5 * (x * (rho * rho - x * x).sqrt() + rho * rho * (x / rho).asin());
    let n = 20000;
    let added: f64 = (0..n)
        .map(|i| {
            let z = h * (i as f64 + 0.5) / n as f64;
            let w = c - (r * r - (r - z) * (r - z)).sqrt();
            prim(14.0, w) - prim(13.0, w)
        })
        .sum::<f64>()
        * h
        / n as f64;
    let got = volume(&mut s) - joined;
    assert!((got - added).abs() < 1e-5, "{got} vs {added}");
    let state = s.model.state();
    assert!(state.body("Main").unwrap().body.validity().is_empty());
}

/// the round is cut by an opening in the floor, as the ordinary round is, not added over
/// it. A cut `depth` through the plate (z from -2 to 0) leaves the plate's top with a hole.
fn cut_through(s: &mut Session, sketch: impl FnOnce(&mut Session)) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    sketch(s);
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 5, "operation": "cut", "direction": "negative"}));
}

/// Area of the round's section over x in [x0, x1] from the wall (the wall at x = 0, c as in
/// `keep_edge_section`): the section's height at distance x is the circle's z(x) = r - sqrt(r^2 -
/// (c - x)^2) for x < c.
fn section_between(r: f64, h: f64, x0: f64, x1: f64) -> f64 {
    let c = (r * r - (r - h) * (r - h)).sqrt();
    let n = 200_000;
    (0..n)
        .map(|i| {
            let x = x0 + (x1 - x0) * (i as f64 + 0.5) / n as f64;
            if x >= c { 0.0 } else { r - (r * r - (c - x) * (c - x)).sqrt() }
        })
        .sum::<f64>()
        * (x1 - x0)
        / n as f64
}

#[test]
fn keep_edge_round_is_cut_by_a_hole_in_the_floor() {
    for r in [1.25, 3.0] {
        let h = 1.0;
        // A through hole of radius 0.8 centred 1 mm from the wall (x = 1) at y = 8: the round's
        // section there is x in [1, 1 + c]; the hole removes, per y, the chord of the circle.
        let mut s = rib_on_plate(h);
        cut_through(&mut s, |s| {
            run(s, "sketch.circle.center", json!({"center": [2.0, 8.0], "radius": 0.8}));
        });
        let before = volume(&mut s);
        run(&mut s, "solid.fillet", keep_edge(json!([[1, 0, 0]]), r));
        let added = volume(&mut s) - before;
        // Integrate over y the section area for x in the hole's span at y removed.
        let n = 4000;
        let mut lost = 0.0;
        for i in 0..n {
            let y = 7.2 + 1.6 * (i as f64 + 0.5) / n as f64;
            let half = (0.64_f64 - (y - 8.0) * (y - 8.0)).max(0.0).sqrt();
            lost += section_between(r, h, 2.0 - half - 1.0, 2.0 + half - 1.0).max(0.0) * 1.6 / n as f64;
        }
        let want = 20.0 * keep_edge_section(r, h) - lost;
        assert!((added - want).abs() < 2e-3, "R {r}: {added} vs {want}");
        assert!(added < 20.0 * keep_edge_section(r, h) - 1e-3, "R {r}: the hole is bridged");
    }
}

#[test]
fn keep_edge_round_is_cut_by_a_slot_and_a_partial_hole() {
    let (r, h) = (1.25_f64, 1.0_f64);
    let c = (r * r - (r - h) * (r - h)).sqrt();
    // A slot across the whole round's reach, 2 long in y at x from 1.2 to 3: the round's section
    // beyond x = 1.2 is removed over those 2 mm.
    let mut s = rib_on_plate(h);
    cut_through(&mut s, |s| {
        run(s, "sketch.rectangle.two_point", json!({"p0": [1.2, 5.0], "p1": [3.0, 7.0]}));
    });
    let before = volume(&mut s);
    run(&mut s, "solid.fillet", keep_edge(json!([[1, 0, 0]]), r));
    let want = 20.0 * keep_edge_section(r, h) - 2.0 * section_between(r, h, 0.2, c);
    assert!((volume(&mut s) - before - want).abs() < 2e-3, "{} vs {want}", volume(&mut s) - before);
    // A slot from x = 1.5 (0.5 from the wall) takes the section beyond 0.5 over its 2 mm.
    let mut s = rib_on_plate(h);
    cut_through(&mut s, |s| {
        run(s, "sketch.rectangle.two_point", json!({"p0": [1.5, -3.0], "p1": [4.0, -1.0]}));
    });
    let before = volume(&mut s);
    run(&mut s, "solid.fillet", keep_edge(json!([[1, 0, 0]]), r));
    let want = 20.0 * keep_edge_section(r, h) - 2.0 * section_between(r, h, 0.5, c);
    assert!((volume(&mut s) - before - want).abs() < 2e-3, "{} vs {want}", volume(&mut s) - before);
}

/// a plate with a rib whose right wall leans back at `theta` degrees to the floor (inside the
/// rib), `h` high, 20 long, the right wall's foot edge at (1.5, 0, 0).
fn leaning_rib(theta: f64, h: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10.0, -10.0], "p1": [10.0, 10.0]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2, "operation": "new", "direction": "negative", "body_name": "Main"}));
    let d = h / theta.to_radians().tan();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 10, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}}));
    run(&mut s, "sketch.line", json!({"points": [[-1.5, 0.0], [1.5, 0.0], [1.5 - d, h], [-1.5 + d, h]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20, "operation": "join"}));
    s
}

/// What the ordinary round adds per mm where the faces meet at `alpha` (radians, in the air): the
/// corner's area R^2 (cot(alpha/2) - (pi - alpha)/2).
fn ordinary_section(r: f64, alpha: f64) -> f64 {
    r * r * (1.0 / (alpha / 2.0).tan() - (PI - alpha) / 2.0)
}

/// Where the ordinary round's contact line is on the wall, keep_edge is the ordinary round: the
/// same volume as the default and the closed form. The wall's height `h` is under R (the old
/// scope) but at least R (1 + cos alpha) (the height of the ordinary round's contact line on the wall, R / tan(alpha/2) along it), so the ordinary round fits. The last case is the contact line at the top edge itself.
#[test]
fn keep_edge_is_the_ordinary_round_where_that_fits() {
    let r = 2.0;
    // (angle of the wall to the floor inside the rib, height); the faces' angle in the air is
    // 180 - theta degrees.
    for (theta, h) in [(45.0_f64, 1.0), (80.0, 1.8), (85.0, 1.9), (80.0, 2.0 * (1.0 + (100.0_f64).to_radians().cos()))] {
        let alpha = (180.0 - theta).to_radians();
        let mut base = leaning_rib(theta, h);
        let before = volume(&mut base);
        let edge = json!([[1.5, 0, 0]]);
        run(&mut base, "solid.fillet", json!({"edges": edge, "radius": r, "tangent_chain": false}));
        let ordinary = volume(&mut base);
        let mut s = leaning_rib(theta, h);
        run(&mut s, "solid.fillet", keep_edge(edge, r));
        let kept = volume(&mut s);
        let want = before + 20.0 * ordinary_section(r, alpha);
        assert!((ordinary - want).abs() < 1e-6 * want, "{theta} deg: default {ordinary} vs {want}");
        assert!((kept - ordinary).abs() < 1e-9, "{theta} deg h {h}: keep_edge {kept} vs default {ordinary}");
    }
}

/// What the keep-edge round adds per mm on a wall leaning back (the faces meet at `alpha` radians
/// in the air, a wall `h` high, its top edge at x = tx = -h / tan(theta) from the foot): the
/// circle of radius r tangent to the floor through (tx, h), centre (tx + c, r),
/// c = sqrt(r^2 - (r - h)^2), and the area between the wall, the floor and the arc: the integral
/// over z from 0 to h of (arc x at z) minus (wall x at z).
fn leaning_keep_edge_section(r: f64, h: f64, theta: f64) -> f64 {
    let tx = -h / theta.to_radians().tan();
    let c = (r * r - (r - h) * (r - h)).sqrt();
    let n = 400_000;
    (0..n)
        .map(|i| {
            let z = h * (i as f64 + 0.5) / n as f64;
            let arc = tx + c - (r * r - (r - z) * (r - z)).sqrt();
            arc + z / theta.to_radians().tan()
        })
        .sum::<f64>()
        * h
        / n as f64
}

/// just below the threshold on a wall leaning back, part of the keep-edge round stands over
/// the wall's foot, beyond the floor's outline; the wall bounds it, not the floor.
#[test]
fn keep_edge_round_on_a_wall_leaning_back_is_whole() {
    let r = 2.0_f64;
    for (theta, h) in [(80.0_f64, 0.4121759_f64), (60.0, 0.249)] {
        let mut s = leaning_rib(theta, h);
        let before = volume(&mut s);
        run(&mut s, "solid.fillet", keep_edge(json!([[1.5, 0, 0]]), r));
        let want = 20.0 * leaning_keep_edge_section(r, h, theta);
        let got = volume(&mut s) - before;
        assert!((got - want).abs() < 2e-3, "{theta} deg: {got} vs {want}");
    }
}

/// a shaft along x (radius 4 for x in [0, 6], radius 2 for x in [-8, 0]) with a flat
/// cutting away everything below y = -d; the inside corner of the shoulder x = 0 is a circle of
/// radius 2 cut in two unequal edges by the flat.
fn flatted_shaft(d: Option<f64>) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}}));
    run(&mut s, "sketch.line", json!({"points": [[6, 0], [6, 4], [0, 4], [0, 2], [-8, 2], [-8, 0]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.revolve",
        json!({"axis": {"origin": [0, 0, 0], "dir": [1, 0, 0]}, "angle": "360 deg", "operation": "new", "body_name": "Main"}),
    );
    if let Some(d) = d {
        run(&mut s, "sketch.create", json!({"plane": {"origin": [-9, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-5, -5], "p1": [-d, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 16, "direction": "positive", "operation": "cut"}));
    }
    s
}

/// keep_edge gives the ordinary round where it fits, all of the circle's edges, in every
/// row of the issue's closed-form table (the default round matches the integral to 1e-7).
#[test]
fn keep_edge_on_a_circle_split_by_a_flat_equals_the_default_round() {
    let rows = [
        (None, 403.159_751_6),
        (Some(1.0), 279.847_524_5),
        (Some(1.5), 315.124_776_1),
        (Some(1.8), 333.687_001_2),
        (Some(1.88), 338.122_500_4),
        (Some(1.9), 339.184_922_1),
        (Some(1.95), 341.740_333_7),
    ];
    for (d, want) in rows {
        for pick in [[0.0, 2.0, 0.0], [0.0, 0.0, 2.0]] {
            let mut plain = flatted_shaft(d);
            run(&mut plain, "solid.fillet", json!({"edges": [pick], "radius": 0.6, "tangent_chain": true}));
            let mut kept = flatted_shaft(d);
            run(&mut kept, "solid.fillet", json!({"edges": [pick], "radius": 0.6, "tangent_chain": true, "fit": "keep_edge"}));
            let (p, k) = (volume(&mut plain), volume(&mut kept));
            assert!((p - want).abs() < 1e-6, "d {d:?}: default {p} vs {want}");
            assert!((k - want).abs() < 1e-6, "d {d:?} pick {pick:?}: keep_edge {k} vs {want}");
        }
    }
}

/// without the tangent chain, keep_edge rounds the edge picked, not the other one.
#[test]
fn keep_edge_without_the_chain_rounds_the_picked_edge_of_a_split_circle() {
    for (pick, want) in [([0.0, 2.0, 0.0], 339.002_321_5), ([0.0, -1.0, 1.732_050_8], 338.484_375_5)] {
        let mut s = flatted_shaft(Some(1.9));
        run(&mut s, "solid.fillet", json!({"edges": [pick], "radius": 0.6, "tangent_chain": false, "fit": "keep_edge"}));
        let got = volume(&mut s);
        assert!((got - want).abs() < 1e-6, "pick {pick:?}: {got} vs {want}");
    }
}
