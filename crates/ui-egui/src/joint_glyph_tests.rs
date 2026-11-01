//! Joint glyphs in a crowded assembly (#6404): a rigid joint draws a small square without the
//! motion axis and ring, glyphs shrink with the parts they join when zoomed out, and glyphs
//! piled on one another are drawn faint, except a picked one. Hovering a glyph names the joint
//! and its type.

use serde_json::json;

use crate::dialogs_assembly::{JointGlyph, joint_glyph_at, joint_glyph_plan, joint_glyph_tip};
use crate::scenario::Harness;

/// Components A (grounded), B and C, 20 × 20 × 10 boxes (diagonal 30); B joined to A by a
/// revolute "Hinge", C by a rigid "Glue".
fn harness() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    crate::test_support::grounded_boxes(&[("A", 0), ("B", 30), ("C", 60)], |id, p| h.app.run(id, p).unwrap());
    h.app
        .run("joint.create", json!({"type": "revolute", "name": "Hinge", "a": {"occurrence": "A:1", "face": [10, 10, 10]}, "b": {"occurrence": "B:1", "face": [40, 10, 0]}}))
        .unwrap();
    h.app
        .run("joint.create", json!({"type": "rigid", "name": "Glue", "a": {"occurrence": "A:1", "face": [20, 10, 5]}, "b": {"occurrence": "C:1", "face": [60, 10, 5]}}))
        .unwrap();
    h.frames(3);
    h
}

fn id(h: &Harness, name: &str) -> u64 {
    h.app.session.doc.assembly.joints.iter().find(|j| j.name == name).unwrap().id
}

/// The glyph plan with the camera `distance` mm from its target.
fn plan_at(h: &mut Harness, distance: f64) -> Vec<JointGlyph> {
    h.app.cam.distance = distance;
    h.frames(2);
    let rect = h.app.viewport.rect.expect("the viewport was laid out");
    joint_glyph_plan(&h.app, &crate::viewport::projection(&h.app, rect))
}

fn glyph(plan: &[JointGlyph], id: u64) -> &JointGlyph {
    plan.iter().find(|g| g.id == id).unwrap_or_else(|| panic!("no glyph for joint {id}: {plan:?}"))
}

#[test]
fn a_rigid_joint_draws_a_square_and_a_moving_joint_its_axis() {
    let mut h = harness();
    let (hinge, glue) = (id(&h, "Hinge"), id(&h, "Glue"));
    let plan = plan_at(&mut h, 120.0);
    assert!(!glyph(&plan, hinge).rigid, "a revolute joint keeps its axis and ring");
    assert!(glyph(&plan, glue).rigid, "a rigid joint has no motion to show");
    let order: Vec<u64> = plan.iter().map(|g| g.id).collect();
    assert_eq!(order, [hinge, glue], "moving joints are planned (and so drawn on top) before rigid ones");
}

#[test]
fn hovering_a_glyph_names_its_joint_and_type() {
    let mut h = harness();
    let (hinge, glue) = (id(&h, "Hinge"), id(&h, "Glue"));
    let plan = plan_at(&mut h, 120.0);
    let (at_hinge, at_glue) = (glyph(&plan, hinge).centre, glyph(&plan, glue).centre);
    assert_eq!(joint_glyph_at(&plan, at_glue + egui::vec2(3.0, -2.0)), Some(glue));
    assert_eq!(joint_glyph_at(&plan, at_hinge), Some(hinge));
    assert_eq!(joint_glyph_at(&plan, at_glue + egui::vec2(20.0, 0.0)), None, "off the glyph: no tooltip");
    assert_eq!(joint_glyph_tip(&h.app, glue).as_deref(), Some("Glue · Rigid"));
    assert_eq!(joint_glyph_tip(&h.app, hinge).as_deref(), Some("Hinge · Revolute"));
    // Piled up far out, the pointer names the glyph drawn on top, not the faded one under it.
    let far = plan_at(&mut h, 20_000.0);
    assert_eq!(joint_glyph_at(&far, glyph(&far, hinge).centre), Some(hinge), "{far:?}");
}

#[test]
fn zoomed_out_glyphs_shrink_with_the_parts_they_join() {
    let mut h = harness();
    let hinge = id(&h, "Hinge");
    // Close in, the screen share is the smaller: 5 % of the view height, as before.
    let near = plan_at(&mut h, 40.0);
    let view = h.app.cam.half_height() * 2.0 * 0.05;
    assert!(view < 30.0 * 0.25, "the test's close view is close enough: {view}");
    assert!((glyph(&near, hinge).len - view).abs() < 1e-9, "{near:?}");
    // Far out, a quarter of the joined boxes' diagonal (30 mm) caps it, whatever the zoom.
    for distance in [2_000.0, 20_000.0] {
        let far = plan_at(&mut h, distance);
        assert!(h.app.cam.half_height() * 2.0 * 0.05 > 7.5);
        assert!((glyph(&far, hinge).len - 7.5).abs() < 1e-6, "at {distance} mm: {far:?}");
    }
}

#[test]
fn piled_up_glyphs_fade_but_a_picked_one_does_not() {
    let mut h = harness();
    let (hinge, glue) = (id(&h, "Hinge"), id(&h, "Glue"));
    // Close in, the two glyphs sit apart and both draw in full.
    let near = plan_at(&mut h, 120.0);
    assert!(near.iter().all(|g| !g.faded), "{near:?}");
    // Far out, both land on a few screen points: the first is drawn, the one on it fades.
    let far = plan_at(&mut h, 20_000.0);
    assert!(glyph(&far, hinge).centre.distance(glyph(&far, glue).centre) < 3.5, "{far:?}");
    assert!(!glyph(&far, hinge).faded && glyph(&far, glue).faded, "{far:?}");
    // A picked joint comes first and never fades; the other one on it fades instead.
    h.app.tree.picked_joints.push(glue);
    let picked = plan_at(&mut h, 20_000.0);
    assert_eq!(picked[0].id, glue);
    assert!(!glyph(&picked, glue).faded && glyph(&picked, hinge).faded, "{picked:?}");
}
