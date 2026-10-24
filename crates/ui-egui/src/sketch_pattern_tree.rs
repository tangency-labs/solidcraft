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
        let curves: Vec<_> = app
            .session
            .doc
            .sketch(sketch)
            .map(|sk| {
                pattern
                    .entities
                    .iter()
                    .chain(pattern.instances.iter().flat_map(|i| &i.curves))
                    .map(|id| {
                        if sk.curve_index(id).is_some() {
                            Sel::SketchCurve { id: id.clone(), sketch: None }
                        } else {
                            Sel::SketchPoint { id: id.clone() }
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let selected = active && !curves.is_empty() && curves.iter().all(|curve| app.session.selection.contains(curve));
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
            let items = if active { curves } else { vec![Sel::Feature { id: sketch }] };
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
}
