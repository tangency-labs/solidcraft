use solidcore_record::Vec2;

use super::grouping::{
    anchor_groups_with_rel, constraint_entities, constraint_matches_selection, glyph_constraints, glyph_group, glyph_group_visible_when_selected,
};
use super::layout::{CURSOR_REACH, PlacementScene, glyph_label_rect, glyph_label_wrap_width, square_grid_dims};
use super::{GLYPH, MIN_GLYPH_PX_PER_MM, MIN_GLYPH_SPAN_PX, OVERLAY, constraint_glyphs_visible_at_scale, glyph_positions, glyph_px};

fn small() -> f32 {
    40.0
}

fn nearest_glyph<'a>(glyphs: impl IntoIterator<Item = (&'a str, egui::Pos2)>, pos: egui::Pos2, reach: f32) -> Option<(&'a str, egui::Pos2)> {
    glyphs.into_iter().filter(|(_, c)| c.distance(pos) <= reach).min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos)))
}

#[test]
fn glyphs_stay_visible_when_zoomed_in() {
    assert!(constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM, true, small));
    assert!(constraint_glyphs_visible_at_scale(10.0, true, small));
}

#[test]
fn glyphs_hide_below_the_zoom_threshold() {
    assert!(!constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM - f32::EPSILON, true, small));
    assert!(!constraint_glyphs_visible_at_scale(0.5, true, small));
    assert!(!constraint_glyphs_visible_at_scale(0.5, true, || MIN_GLYPH_SPAN_PX - 1.0));
}

#[test]
fn preference_off_keeps_glyphs_at_every_zoom() {
    assert!(constraint_glyphs_visible_at_scale(0.1, false, small));
    assert!(constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM - 1.0, false, small));
}

#[test]
fn glyphs_stay_while_a_large_sketch_fills_the_view() {
    assert!(constraint_glyphs_visible_at_scale(1.5, true, || 500.0 * 1.5));
    assert!(constraint_glyphs_visible_at_scale(0.05, true, || MIN_GLYPH_SPAN_PX));
}

#[test]
fn a_point_along_constraint_has_a_glyph_on_its_point() {
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind, Sketch};
    let mut sk = Sketch::new();
    let (Ok(a), Ok(b), Ok(q)) =
        (sk.add_point(Vec2::new(0.0, 0.0), None), sk.add_point(Vec2::new(10.0, 0.0), None), sk.add_point(Vec2::new(2.5, 0.0), None))
    else {
        panic!("points");
    };
    let Ok(l) = sk.add_curve(CurveKind::Line { a, b }, None) else { panic!("line") };
    let k = ConstraintKind::PointAlong { p: q, c: l, at: 0.25 };
    assert_eq!(constraint_entities(&k), (vec![l], vec![q]));
    sk.add_constraint(k.clone(), None).unwrap();
    let (groups, _) = anchor_groups_with_rel(&sk);
    assert_eq!(groups.len(), 1);
    assert!((groups[0].at.x - 2.5).abs() < 1e-9 && groups[0].at.y.abs() < 1e-9 && groups[0].dir.is_none());
    assert_eq!(crate::icons::markers::glyph_icon(k.name()), Some("c_midpoint"));
}

#[test]
fn glyph_size_follows_the_preference_and_defaults_to_the_old_size() {
    assert_eq!(glyph_px(crate::prefs::Prefs::default().constraint_glyph_scale), GLYPH);
    assert_eq!(GLYPH, 15.0, "the default badge keeps its size (#10794)");
    assert_eq!(glyph_px(2.0), 30.0);
    for hostile in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -4.0, 0.0, 1e9] {
        let px = glyph_px(hostile);
        assert!((GLYPH..=GLYPH * 3.0).contains(&px), "{hostile} gives {px}");
    }
}

#[test]
fn glyph_label_stays_clear_of_the_cursor() {
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let size = egui::vec2(140.0, 20.0);
    for gpx in [GLYPH, GLYPH * 3.0] {
        for x in [0.0, 5.0, 60.0, 400.0, 790.0, 800.0] {
            for y in [0.0, 5.0, 30.0, 300.0, 590.0, 600.0] {
                let glyph = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(gpx, gpx));
                for pointer in [glyph.center(), glyph.left_top(), glyph.right_bottom(), glyph.center_top()] {
                    let cursor = egui::Rect::from_min_size(pointer, egui::vec2(CURSOR_REACH, CURSOR_REACH));
                    let label = glyph_label_rect(pointer, glyph, size, view);
                    assert!(!label.intersects(cursor), "label {label:?} under the cursor at {pointer:?}");
                    assert!(!label.contains(pointer), "label {label:?} on the pointer {pointer:?}");
                }
            }
        }
    }
}

#[test]
fn glyph_label_goes_above_and_stays_in_the_view() {
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let size = egui::vec2(140.0, 20.0);
    let glyph = egui::Rect::from_center_size(egui::pos2(400.0, 300.0), egui::vec2(GLYPH, GLYPH));
    let label = glyph_label_rect(glyph.center(), glyph, size, view);
    assert!(label.bottom() < glyph.top(), "above the glyph: {label:?}");
    let glyph = egui::Rect::from_center_size(egui::pos2(795.0, 300.0), egui::vec2(GLYPH, GLYPH));
    assert!(view.contains_rect(glyph_label_rect(glyph.center(), glyph, size, view)));
}

#[test]
fn overlapping_glyphs_pick_the_nearest() {
    let reach = glyph_px(3.0) * 0.6;
    let glyphs = [("c1", egui::pos2(100.0, 100.0)), ("c2", egui::pos2(147.0, 100.0))];
    let pointer = egui::pos2(125.0, 100.0);
    assert!(glyphs.iter().all(|(_, c)| c.distance(pointer) <= reach), "the test needs overlapping reach");
    assert_eq!(nearest_glyph(glyphs, pointer, reach).map(|(id, _)| id), Some("c2"));
    assert_eq!(nearest_glyph(glyphs, egui::pos2(110.0, 100.0), reach).map(|(id, _)| id), Some("c1"));
    assert_eq!(nearest_glyph(glyphs, egui::pos2(400.0, 400.0), reach), None);
}

#[test]
fn hovered_instance_of_a_shared_glyph_is_the_one_under_the_pointer() {
    let far = egui::pos2(500.0, 400.0);
    let glyphs = [("parallel", egui::pos2(100.0, 100.0)), ("parallel", far)];
    let (_, at) = nearest_glyph(glyphs, far + egui::vec2(3.0, 2.0), GLYPH * 0.6).unwrap_or(("", egui::Pos2::ZERO));
    assert_eq!(at, far);
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let glyph = egui::Rect::from_center_size(at, egui::vec2(GLYPH, GLYPH));
    let label = glyph_label_rect(far, glyph, egui::vec2(140.0, 20.0), view);
    assert!(label.center().distance(far) < 100.0, "label {label:?} beside the hovered glyph");
}

#[test]
fn glyph_label_fits_a_narrow_view() {
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(120.0, 600.0));
    let size = egui::vec2(glyph_label_wrap_width(view) + 6.0, 40.0);
    for x in [0.0, 60.0, 120.0] {
        let glyph = egui::Rect::from_center_size(egui::pos2(x, 300.0), egui::vec2(GLYPH, GLYPH));
        let label = glyph_label_rect(glyph.center(), glyph, size, view);
        assert!(label.left() >= view.left() && label.right() <= view.right(), "label {label:?} in {view:?}");
    }
}

#[test]
fn glyph_label_stays_inside_a_short_view() {
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 120.0));
    for size in [egui::vec2(140.0, 80.0), egui::vec2(294.0, 80.0), egui::vec2(140.0, 200.0)] {
        for x in [0.0, 150.0, 300.0] {
            for y in [0.0, 10.0, 60.0, 110.0, 120.0] {
                let glyph = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(GLYPH, GLYPH));
                for pointer in [glyph.center(), glyph.left_top(), glyph.right_bottom()] {
                    let label = glyph_label_rect(pointer, glyph, size, view);
                    assert!(view.contains_rect(label), "label {label:?} of {size:?} outside {view:?}");
                    assert!(label.width() > 0.0 && label.height() > 0.0, "label {label:?} vanished");
                }
            }
        }
    }
}

#[test]
fn glyph_label_too_big_for_a_short_view_stays_clear_of_the_cursor() {
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 120.0));
    for size in [egui::vec2(140.0, 80.0), egui::vec2(294.0, 80.0), egui::vec2(140.0, 200.0)] {
        for x in [0.0, 5.0, 150.0, 290.0, 300.0] {
            for y in [0.0, 5.0, 60.0, 110.0, 120.0] {
                let glyph = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(GLYPH, GLYPH));
                for pointer in [glyph.center(), glyph.left_top(), glyph.right_bottom()] {
                    let cursor = egui::Rect::from_min_size(pointer, egui::vec2(CURSOR_REACH, CURSOR_REACH));
                    let label = glyph_label_rect(pointer, glyph, size, view);
                    assert!(view.contains_rect(label), "label {label:?} of {size:?} outside {view:?}");
                    assert!(!label.intersects(cursor), "label {label:?} of {size:?} under the cursor at {pointer:?}");
                    assert!(label.width() > 0.0 && label.height() > 0.0, "label {label:?} vanished");
                }
            }
        }
    }
}

#[test]
fn square_grid_dims_packs_most_squarely() {
    let expected = [
        (0, (0, 0)),
        (1, (1, 1)),
        (2, (2, 1)),
        (3, (2, 2)),
        (4, (2, 2)),
        (5, (3, 2)),
        (6, (3, 2)),
        (7, (3, 3)),
        (8, (3, 3)),
        (9, (3, 3)),
        (10, (4, 3)),
    ];
    for (n, dims) in expected {
        assert_eq!(square_grid_dims(n), dims, "count {n}");
    }
}

#[test]
fn anchor_groups_clusters_coincident_points_and_keeps_curves_distinct() {
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind, Sketch};
    let mut sk = Sketch::new();
    let (Ok(p0), Ok(p1), Ok(p2)) =
        (sk.add_point(Vec2::new(0.0, 0.0), None), sk.add_point(Vec2::new(0.0, 0.0), None), sk.add_point(Vec2::new(10.0, 0.0), None))
    else {
        panic!("points");
    };
    let Ok(l1) = sk.add_curve(CurveKind::Line { a: p0, b: p2 }, None) else { panic!("line") };
    sk.add_constraint(ConstraintKind::Coincident { p: p0, q: p1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: p1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l: l1 }, None).unwrap();
    let (groups, _) = anchor_groups_with_rel(&sk);
    assert_eq!(groups.len(), 2, "one coincident point cluster and one curve");
    assert_eq!(groups[0].constraint_ids.len(), 2, "Coincident and Fix share one cluster");
    assert_eq!(groups[1].constraint_ids.len(), 1, "Horizontal sits on curve l1");
}

/// Opt-in filtering ties visible glyphs to selected curves and points, including a
/// curve's endpoints. Without a selection the normal glyph list is untouched (#18817).
#[test]
fn selected_geometry_can_reveal_its_constraint_glyphs() {
    use solidcraft_engine::Sel;
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind, Sketch};
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(20.0, 0.0), None).unwrap();
    let other = sk.add_point(Vec2::new(0.0, 20.0), None).unwrap();
    let line = sk.add_curve(CurveKind::Line { a, b }, None).unwrap();
    let horizontal = ConstraintKind::Horizontal { l: line };
    let curve = Sel::SketchCurve { id: sk.curves[line].id.clone(), sketch: None };
    assert!(constraint_matches_selection(&sk, &horizontal, &[curve]));
    assert!(constraint_matches_selection(&sk, &horizontal, &[Sel::SketchPoint { id: sk.points[a].id.clone() }]));
    assert!(constraint_matches_selection(&sk, &horizontal, &[Sel::SketchPoint { id: sk.points[b].id.clone() }]));
    assert!(!constraint_matches_selection(&sk, &horizontal, &[]), "unselected geometry has no filtered glyph");
    assert!(
        !constraint_matches_selection(&sk, &horizontal, &[Sel::SketchPoint { id: sk.points[other].id.clone() }]),
        "unrelated points do not reveal every constraint"
    );
    let point_constraint = ConstraintKind::Fix { p: other };
    assert!(constraint_matches_selection(&sk, &point_constraint, &[Sel::SketchPoint { id: sk.points[other].id.clone() }]));
    assert!(!constraint_matches_selection(&sk, &point_constraint, &[Sel::SketchPoint { id: sk.points[a].id.clone() }]));
    assert!(!crate::UiState::default().constraint_glyphs_selected_only, "the old display remains the default");
}

/// A shared midpoint badge must remain visible if any represented constraint fails or
/// belongs to the selection, even when the first constraint is not the relevant one.
#[test]
fn grouped_midpoint_glyph_respects_all_members() {
    use solidcraft_engine::Sel;
    use solidcraft_engine::sketch::{Constraint, ConstraintKind, CurveKind, Sketch};
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    let c = sk.add_point(Vec2::new(5.0, -5.0), None).unwrap();
    let d = sk.add_point(Vec2::new(5.0, 5.0), None).unwrap();
    let mid = sk.add_point(Vec2::new(5.0, 0.0), None).unwrap();
    let other = sk.add_point(Vec2::new(20.0, 20.0), None).unwrap();
    let line1 = sk.add_curve(CurveKind::Line { a, b }, None).unwrap();
    let line2 = sk.add_curve(CurveKind::Line { a: c, b: d }, None).unwrap();
    for (id, line) in [("first", line1), ("second", line2)] {
        sk.constraints.push(Constraint {
            id: id.into(),
            kind: ConstraintKind::Midpoint { p: mid, l: line },
            param: None,
            driven: false,
            driven_param: None,
            text: None,
        });
    }
    assert_eq!(glyph_constraints(&sk).count(), 1, "both constraints share a glyph");
    assert_eq!(glyph_group(&sk, "first").len(), 2);
    let show = |selected: &[Sel], picked: Option<&str>, failing: &str| {
        glyph_group_visible_when_selected(&sk, "first", selected, picked, |id| id.as_str() == failing)
    };
    assert!(!show(&[], None, ""), "no matching selection hides the grouped badge");
    assert!(show(&[], None, "second"), "a failure in the second constraint shows the shared error glyph");
    assert!(show(&[], Some("second"), ""), "picking the second constraint reveals the group");
    assert!(show(&[Sel::SketchConstraint { id: "second".into() }], None, ""), "explicitly selecting the second constraint reveals the group");
    assert!(show(&[Sel::SketchCurve { id: sk.curves[line2].id.clone(), sketch: None }], None, ""), "selecting the second line reveals the group");
    assert!(!show(&[Sel::SketchPoint { id: sk.points[other].id.clone() }], None, ""), "unrelated geometry does not reveal the group");
}

#[test]
fn expanded_container_encloses_badge_and_nudges_neighbors_clear() {
    let view = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let badge = egui::Rect::from_center_size(egui::pos2(200.0, 150.0), egui::vec2(15.0, 15.0));
    let neighbor = egui::Rect::from_center_size(egui::pos2(228.0, 150.0), egui::vec2(15.0, 15.0));
    let box_size = egui::vec2(52.0, 52.0);
    let scene = PlacementScene { dim_texts: Vec::new(), points: Vec::new(), curves: Vec::new(), placed: Vec::new() };
    let exp = scene.place_expanded(badge, badge.center(), box_size, view, true, &[], &[neighbor], 4.0);
    assert!(exp.contains(badge.center()), "container {exp:?} must enclose badge {badge:?}");
    assert!(view.contains_rect(exp), "container {exp:?} stays inside view");
    assert!(!exp.intersects(neighbor), "container chose left/vertical alignment to avoid {neighbor:?}: {exp:?}");

    let nudged = scene.nudge_clear(
        egui::Rect::from_center_size(badge.center() + egui::vec2(10.0, 0.0), egui::vec2(15.0, 15.0)),
        &[exp.expand(4.0)],
        &[],
        view,
    );
    assert!(!nudged.intersects(exp.expand(4.0)), "nudged {nudged:?} clears expanded {exp:?}");
    assert!(view.contains_rect(nudged));
}

#[test]
fn constraint_grouping_hover_and_click_expand_flow() {
    use serde_json::json;
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [30, 0]], "connect": false, "infer": false})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 20], [30, 20]], "connect": false, "infer": false})).unwrap();
    h.app.run("sketch.constraint.horizontal_vertical", json!({"line": "l1", "mode": "horizontal"})).unwrap();
    h.app.run("sketch.constraint.parallel", json!({"a": "l1", "b": "l2"})).unwrap();
    h.app.run("sketch.constraint.equal", json!({"a": "l1", "b": "l2"})).unwrap();
    h.app.run("sketch.constraint.horizontal_vertical", json!({"line": "l2", "mode": "horizontal"})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.call("ui.camera", json!({"target": [15, 10, 0], "distance": 120}));
    h.frames(2);

    assert_eq!(OVERLAY.with(|c| c.borrow().collapsed_badges.len()), 2);
    assert!(glyph_positions().is_empty());

    let badge_pos = OVERLAY.with(|c| c.borrow().collapsed_badges[0].1.center());
    h.step(&json!({"move": [badge_pos.x, badge_pos.y]})).unwrap();
    h.frames(2);
    assert!(glyph_positions().is_empty(), "still collapsed before 200 ms");
    assert!(crate::scenario::rect_of("sketch:glyph_label").is_none(), "no hint chip on collapsed badge");

    h.frames(15);
    let opened = glyph_positions();
    assert_eq!(opened.len(), 3, "expanded group shows all 3 constraints: {opened:?}");
    for (i, (_, p)) in opened.iter().enumerate() {
        for (_, q) in opened.iter().skip(i + 1) {
            assert!(p.distance(*q) >= 16.0, "expanded grid glyphs never collide: {opened:?}");
        }
    }

    // Move away so the first badge collapses again, then click the second badge to expand immediately.
    h.step(&json!({"move": [10.0, 10.0]})).unwrap();
    h.frames(2);
    assert!(glyph_positions().is_empty(), "moving away collapses the expanded box");
    let second_badge = OVERLAY.with(|c| c.borrow().collapsed_badges[1].1.center());
    h.step(&json!({"click": [second_badge.x, second_badge.y]})).unwrap();
    h.frames(1);
    assert_eq!(glyph_positions().len(), 3, "clicking a collapsed badge expands it immediately");
}

#[test]
fn detach_targets_for_cluster_filters_only_matching_selected_curve() {
    use solidcraft_engine::sketch::{CurveKind, Sketch};

    use super::actions::detach_targets_for_cluster;
    use super::grouping::{find_point_cluster, point_cluster_map};

    let mut sk = Sketch::new();
    let p_shared = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    let p_a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let p_b = sk.add_point(Vec2::new(20.0, 10.0), None).unwrap();
    let p_c = sk.add_point(Vec2::new(10.0, 20.0), None).unwrap();
    let p_other = sk.add_point(Vec2::new(40.0, 0.0), None).unwrap();
    let p_other_end = sk.add_point(Vec2::new(50.0, 0.0), None).unwrap();
    let c1 = sk.add_curve(CurveKind::Line { a: p_a, b: p_shared }, None).unwrap();
    let c2 = sk.add_curve(CurveKind::Line { a: p_shared, b: p_b }, None).unwrap();
    let c3 = sk.add_curve(CurveKind::Line { a: p_shared, b: p_c }, None).unwrap();
    let c_other = sk.add_curve(CurveKind::Line { a: p_other, b: p_other_end }, None).unwrap();
    let id1 = sk.curves[c1].id.clone();
    let id2 = sk.curves[c2].id.clone();
    let id3 = sk.curves[c3].id.clone();
    let id_other = sk.curves[c_other].id.clone();

    let mut parent = point_cluster_map(&sk);
    let root = find_point_cluster(&mut parent, p_shared);

    // No curve selected -> all 3 connected curves.
    let (sel_none, all_targets) = detach_targets_for_cluster(&sk, &mut parent, root, None).expect("3 detach targets");
    assert_eq!(sel_none, None);
    let ids: Vec<&str> = all_targets.iter().map(|t| t.curve_id.as_str()).collect();
    assert_eq!(ids, vec![id1.as_str(), id2.as_str(), id3.as_str()]);

    // Selected curve touches the cluster -> only that single curve.
    let (sel_hit, one_target) = detach_targets_for_cluster(&sk, &mut parent, root, Some((&id2, c2))).expect("1 detach target");
    assert_eq!(sel_hit.as_deref(), Some(id2.as_str()));
    assert_eq!(one_target.len(), 1);
    assert_eq!(one_target[0].curve_id, id2);

    // Selected curve does NOT touch the cluster -> falls back to all connected curves.
    let (sel_miss, fallback_targets) = detach_targets_for_cluster(&sk, &mut parent, root, Some((&id_other, c_other))).expect("fallback targets");
    assert_eq!(sel_miss, None);
    assert_eq!(fallback_targets.len(), 3);
}

#[test]
fn point_hover_shows_grouped_detach_options_and_selected_curve_shows_one() {
    use serde_json::json;
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [20, 0]], "connect": true, "infer": false})).unwrap();
    h.app.run("sketch.line", json!({"points": [[20, 0], [30, 15]], "connect": true, "infer": false})).unwrap();
    h.app.run("sketch.line", json!({"points": [[20, 0], [20, -15]], "connect": true, "infer": false})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.call("ui.camera", json!({"target": [15, 0, 0], "distance": 120}));
    h.frames(2);

    // Clear any selection and hover the shared junction point (20, 0).
    h.app.session.selection.clear();
    h.step(&json!({"move": {"sketch": [20, 0]}})).unwrap();
    h.frames(2);
    assert!(OVERLAY.with(|c| c.borrow().detach_preview.is_none()), "point-only detach waits for 200 ms dwell");

    h.frames(15);
    let multi_items = OVERLAY.with(|c| c.borrow().detach_preview.as_ref().map(|p| (p.items.len(), p.selected_curve_id.clone())));
    assert_eq!(multi_items, Some((3, None)), "hovering a 3-way point shows all 3 detach options in one group");

    // Select a connected line and hover the same shared point -> immediately shows only 1 option.
    h.step(&json!({"move": [10.0, 10.0]})).unwrap();
    h.frames(2);
    h.app.session.selection = vec![solidcraft_engine::Sel::SketchCurve { id: "l1".into(), sketch: h.app.session.active_sketch }];
    h.step(&json!({"move": {"sketch": [20, 0]}})).unwrap();
    h.frames(2);
    let single_items = OVERLAY.with(|c| c.borrow().detach_preview.as_ref().map(|p| (p.items.len(), p.selected_curve_id.clone())));
    assert_eq!(single_items, Some((1, Some("l1".to_owned()))), "selected line shows only its single detach option immediately");
}
