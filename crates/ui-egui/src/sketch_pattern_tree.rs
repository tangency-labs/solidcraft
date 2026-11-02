//! Sketch patterns are children of their sketch, rather than separate timeline features.

use serde_json::json;
use solidcraft_engine::{Sel, sketch::SketchPatternKind};

use crate::{
    SolveApp,
    browser::{Row, draw_row},
};

pub(crate) fn has_patterns(app: &SolveApp, sketch: u64) -> bool {
    app.session.doc.sketch(sketch).is_ok_and(|sk| !sk.patterns.is_empty())
}

/// Shared by the parts browser and the vertical feature history. The stored document is the
/// source, so rolled-back sketches retain their children without showing preview-only copies.
/// `dim`: the sketch is hidden (or a placement above it), so its patterns fade with it.
pub(crate) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, sketch: u64, depth: usize, scope: &str, occurrence: Option<(u64, &[u64])>, dim: bool) {
    let patterns: Vec<_> = app.session.doc.sketch(sketch).map(|sk| sk.patterns.clone()).unwrap_or_default();
    for pattern in patterns {
        let (name, icon) = match pattern.kind {
            SketchPatternKind::Rectangular { .. } => ("Rectangular Pattern", "sketch_rect_pattern"),
            SketchPatternKind::Circular { .. } => ("Circular Pattern", "sketch_circ_pattern"),
        };
        let suffix = pattern.id.strip_prefix("pat").unwrap_or(&pattern.id);
        let label = format!("{} {suffix}", crate::i18n::tr_label(name));
        let active = app.session.active_sketch == Some(sketch);
        // The sources are inputs to the pattern: selecting its row must only delete copies.
        // Select point copies when the point itself is a source; curve endpoints go with curves.
        let sources: std::collections::BTreeSet<_> = pattern.entities.iter().collect();
        let copies: Vec<_> = pattern
            .instances
            .iter()
            .flat_map(|instance| &instance.curves)
            .map(|id| Sel::SketchCurve { id: id.clone(), sketch: None })
            .chain(
                pattern
                    .instances
                    .iter()
                    .flat_map(|instance| &instance.points)
                    .filter(|(source, _)| sources.contains(source))
                    .map(|(_, id)| Sel::SketchPoint { id: id.clone() }),
            )
            .collect();
        let selected = active && !copies.is_empty() && copies.iter().all(|copy| app.session.selection.contains(copy));
        let response = draw_row(
            ui,
            ui.id().with(("sketch_pattern", sketch, &pattern.id)),
            &Row { depth, icon, label: &label, name: true, selected, dim, ..Default::default() },
        );
        if let Some(rect) = response.rect {
            crate::scenario::publish_handle(&format!("{scope}:sketch-pattern:{sketch}:{}", pattern.id), rect.center());
        }
        if response.double {
            if let Some((component, path)) = occurrence {
                let _ = app.run("component.activate", json!({"component":component,"occurrence":path.last(),"path":path}));
            }
            app.edit_sketch(sketch);
            if app.session.active_sketch == Some(sketch) {
                crate::sketch_modify_ui::edit_pattern(app, &pattern.id);
            }
        } else if response.clicked {
            let items = if active { copies } else { vec![Sel::Feature { id: sketch }] };
            let _ = app.run("select.set", json!({"items":items}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialogs::Kind, scenario::Harness, sketch_modify_ui::Mod};

    fn harness(left_history: bool) -> (Harness, u64, String, String) {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        h.app.preferences.timeline_left = left_history;
        h.app.run("sketch.create", json!({"plane":"XY","name":"Patterned Sketch"})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        h.app.run("sketch.line", json!({"points":[[10,5],[15,5]],"ids":["source"]})).unwrap();
        let rectangular = h
            .app
            .run("sketch.pattern.rectangular", json!({"entities":["source"],"count":4,"spacing":20,"alignment":"center","reversed":true}))
            .unwrap();
        let circular = h.app.run("sketch.pattern.circular", json!({"entities":["source"],"center":"origin","count":3})).unwrap();
        h.app.finish_sketch();
        h.frames(6);
        (h, sketch, rectangular["pattern"].as_str().unwrap().into(), circular["pattern"].as_str().unwrap().into())
    }

    #[test]
    fn browser_patterns_fold_with_the_sketch_and_double_click_opens_the_saved_editor() {
        let (mut h, sketch, rectangular, circular) = harness(false);
        let handle = format!("browser:sketch-pattern:{sketch}:{rectangular}");
        let child = crate::scenario::handle_at(&handle).unwrap();
        let parent = crate::scenario::handle_at("row:Patterned Sketch").unwrap();
        assert!(child.y > parent.y, "patterns belong below their sketch");
        assert!(crate::scenario::handle_at(&format!("browser:sketch-pattern:{sketch}:{circular}")).is_some());
        h.step(&json!({"click":{"handle":"fold:Patterned Sketch"}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at(&handle).is_none());
        h.step(&json!({"click":{"handle":"fold:Patterned Sketch"}})).unwrap();
        // Let the fold clicks leave egui's triple-click interval before double-clicking a child.
        h.frames(40);
        h.step(&json!({"click":{"handle":handle},"double":true})).unwrap();
        h.frames(3);
        assert_eq!(h.app.session.active_sketch, Some(sketch), "selection={:?}, status={:?}", h.app.session.selection, h.app.status);
        let dialog = h.app.dialog.as_ref().unwrap();
        assert_eq!(dialog.extra["pattern"], rectangular);
        assert!(matches!(&dialog.kind, Kind::SketchModify(Mod::PatternRect { count, reversed: true, .. }) if count == "4"));
    }

    #[test]
    fn left_history_lists_both_pattern_kinds_and_tracks_deletion_and_undo() {
        let (mut h, sketch, rectangular, circular) = harness(true);
        let handle = format!("timeline:sketch-pattern:{sketch}:{circular}");
        assert!(crate::scenario::handle_at(&format!("timeline:sketch-pattern:{sketch}:{rectangular}")).is_some());
        assert!(crate::scenario::handle_at(&handle).is_some());
        h.step(&json!({"click":{"handle":"timeline-sketch-fold:Patterned Sketch"}})).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at(&handle).is_none());
        h.step(&json!({"click":{"handle":"timeline-sketch-fold:Patterned Sketch"}})).unwrap();
        h.frames(40);
        h.step(&json!({"click":{"handle":handle},"double":true})).unwrap();
        h.frames(3);
        assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::SketchModify(Mod::PatternCirc { .. })));
        assert_eq!(h.app.dialog.as_ref().unwrap().extra["pattern"], circular);
        h.step(&json!({"key":"Escape"})).unwrap();
        h.app.run("sketch.delete", json!({"entities":["source"]})).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at(&handle).is_none());
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(3);
        assert!(crate::scenario::handle_at(&handle).is_some());
    }

    #[test]
    fn deleting_a_pattern_row_preserves_sources_and_other_patterns() {
        for left_history in [false, true] {
            for circular in [false, true] {
                for entities in [json!(["source"]), json!(["marker"]), json!(["source", "marker"])] {
                    let mut h = Harness::new();
                    h.app.home.open = false;
                    h.app.ui.show_browser = true;
                    h.app.preferences.timeline_left = left_history;
                    h.app.run("sketch.create", json!({"plane":"XY","name":"Patterned Sketch"})).unwrap();
                    let sketch = h.app.session.active_sketch.unwrap();
                    h.app.run("sketch.line", json!({"points":[[10,5],[15,5]],"ids":["source"]})).unwrap();
                    h.app.run("sketch.point", json!({"point":[10,10],"id":"marker"})).unwrap();
                    h.app.run("sketch.constraint.fix", json!({"entities":["source","marker"]})).unwrap();
                    let sibling = h.app.run("sketch.pattern.rectangular", json!({"entities":entities,"count":2,"spacing":50})).unwrap();
                    let command = if circular { "sketch.pattern.circular" } else { "sketch.pattern.rectangular" };
                    let params = if circular {
                        json!({"entities":entities,"count":3,"center":"origin"})
                    } else {
                        json!({"entities":entities,"count":3,"spacing":20})
                    };
                    let target = h.app.run(command, params).unwrap();
                    let sk = h.app.session.doc.sketch(sketch).unwrap();
                    let pattern = sk.patterns.iter().find(|p| p.id == target["pattern"]).unwrap().clone();
                    let before = serde_json::to_value(sk).unwrap();
                    h.frames(6);
                    let scope = if left_history { "timeline" } else { "browser" };
                    h.step(&json!({"click":{"handle":format!("{scope}:sketch-pattern:{sketch}:{}", pattern.id)}})).unwrap();
                    h.step(&json!({"key":"Delete"})).unwrap();
                    h.frames(3);

                    let sk = h.app.session.doc.sketch(sketch).unwrap();
                    assert!(sk.curve_index("source").is_some(), "source curve lost: {scope}, {command}, {entities}");
                    assert!(sk.point_index("marker").is_some(), "source point lost: {scope}, {command}, {entities}");
                    assert_eq!(sk.patterns.len(), 1, "only the selected pattern should go");
                    assert_eq!(sk.patterns[0].id, sibling["pattern"]);
                    for instance in &pattern.instances {
                        assert!(instance.curves.iter().all(|id| sk.curve_index(id).is_none()));
                        assert!(instance.points.iter().all(|(_, id)| sk.point_index(id).is_none()));
                        assert!(instance.constraints.iter().all(|id| sk.constraints.iter().all(|c| &c.id != id)));
                    }
                    let report = h.app.run("sketch.inspect", json!({})).unwrap();
                    assert_eq!(report["dof"], 0, "fixed sources and the surviving pattern stay constrained");
                    let after = serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap();
                    h.app.run("edit.undo", json!({})).unwrap();
                    assert_eq!(serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap(), before);
                    h.app.run("edit.redo", json!({})).unwrap();
                    assert_eq!(serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap(), after);
                }
            }
        }
    }

    /// Where a text starts, drawn inside `within`.
    fn text_left(h: &Harness, text: &str, within: egui::Rect) -> f32 {
        let mut at = None;
        for clipped in &h.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |s| {
                if let egui::Shape::Text(t) = s
                    && t.galley.text() == text
                    && within.contains(t.pos + t.galley.rect.center().to_vec2())
                {
                    at = Some(t.pos.x + t.galley.rect.left());
                }
            });
        }
        at.unwrap_or_else(|| panic!("{text} in {within:?}"))
    }

    /// #8718: in the vertical History a sketch's pattern rows step in from the sketch's name as far
    /// as they do in the browser, with the eye at the rows' end.
    #[test]
    fn left_history_patterns_step_in_from_their_sketch_like_the_browser() {
        let (mut h, sketch, rectangular, _) = harness(true);
        let label = format!("{} {}", crate::i18n::tr_label("Rectangular Pattern"), rectangular.strip_prefix("pat").unwrap_or(&rectangular));
        h.frame();
        let browser = crate::scenario::rect_of("browser:card").unwrap();
        let in_browser = text_left(&h, &label, browser) - text_left(&h, "Patterned Sketch", browser);
        let sketch_row = crate::scenario::rect_of("history:feature:Patterned Sketch").unwrap();
        let pattern_y = crate::scenario::handle_at(&format!("timeline:sketch-pattern:{sketch}:{rectangular}")).unwrap().y;
        let pattern_row = egui::Rect::from_x_y_ranges(sketch_row.x_range(), pattern_y - 8.0..=pattern_y + 8.0);
        let in_history = text_left(&h, &label, pattern_row) - text_left(&h, "Patterned Sketch", sketch_row);
        assert!((in_history - in_browser).abs() <= 2.0, "History steps in {in_history}, the browser {in_browser}");
    }
}
