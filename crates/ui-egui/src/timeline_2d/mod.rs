//! Two-dimensional (2D) Timeline for SolidCraft.
//!
//! Provides multi-track component layout, two-tier header scrubber, dependency graph
//! visualization, movable brackets corridor, and feature grouping/folding.

pub mod assembly;
pub mod canvas;
mod corridor_check;
pub mod deps;
pub mod state;

use crate::SolveApp;
use egui::Ui;
use serde_json::json;

/// How strongly the alignment dots in empty cells show: faint enough to sit behind the chips.
pub(crate) const DOT_OPACITY: f32 = 0.35;

/// The 2D Timeline's own options, in the right-click menu of its header: the alignment dots,
/// activating a component by its row, and what unsuppressing brings back.
pub(crate) fn option_items(app: &SolveApp) -> Vec<crate::ui_kit::Item> {
    use crate::prefs::Timeline2dUnsuppressMode::{RestoreState, UnsuppressAll};
    use crate::ui_kit::Item;
    let p = &app.preferences;
    let tr = solidcraft_ui_text::tr;
    let mode = |mode: crate::prefs::Timeline2dUnsuppressMode, key: &str| {
        let item = Item::action("ui.timeline2dUnsuppress", tr(key), "suppress").with(json!({ "mode": mode }));
        if p.timeline_2d_unsuppress_mode == mode { item.key("current") } else { item }
    };
    // A check mark shows what is on, as for the unsuppress modes.
    let toggle = |id: &str, key: &str, icon: &str, on: bool| {
        let item = Item::action(id, tr(key), icon).with(json!({ "on": !on }));
        if on { item.key("current") } else { item }
    };
    vec![
        toggle("ui.timeline2dCellDots", "ui.timeline_2d_dots", "grid", p.timeline_2d_cell_dots),
        toggle("ui.timeline2dActivation", "ui.timeline_2d_activation", "component", p.timeline_2d_component_activation),
        mode(RestoreState, "ui.timeline_2d_unsuppress_restore"),
        mode(UnsuppressAll, "ui.timeline_2d_unsuppress_all"),
    ]
}

/// Draws the 2D Timeline into the area the panel host gives its `timeline-2d` panel, which owns
/// the frame, the header with the playback controls and the resizable top edge.
pub(crate) fn panel_contents(app: &mut SolveApp, ui: &mut Ui) {
    crate::browser::sync_reveals(app);
    canvas::paint_timeline_2d(app, ui, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;
    use serde_json::{Value, json};

    /// Verifies mouse hover detection, rail dragging, and vertical resizing of the 2D timeline dock.
    /// Right-clicking a chip outside a multi-selection selects only that chip, so Rename shows;
    /// right-clicking a chip inside the selection keeps the whole selection.
    #[test]
    fn test_right_click_outside_selection_selects_only_that_chip() {
        let mut state = state::Timeline2DState::default();
        state.select_single(1);
        state.toggle_selection(2);
        state.select_for_context_menu(2);
        assert_eq!(state.selected_features.len(), 2, "a chip inside the selection keeps it");
        state.select_for_context_menu(3);
        assert_eq!(state.selected_features.iter().copied().collect::<Vec<_>>(), vec![3]);
        assert_eq!(state.selection_anchor, Some(3));
    }

    /// A group card's tooltip comes from the message catalog, so it follows the reader's language.
    #[test]
    fn test_group_tooltip_follows_the_language() {
        crate::i18n::install();
        let feats = [(3, "Boss", "Base"), (5, "Rib", "Base")];
        let en = {
            let _en = solidcraft_ui_text::scope("en");
            state::Timeline2DState::format_group_tooltip("Ribs", &feats, false)
        };
        assert!(en.starts_with("Ribs (2)\nSteps 03–05 • Rolled back (inactive)"), "{en}");
        assert!(en.contains("Features in this group:") && en.contains("[05] Rib (Base)") && en.ends_with("group options."), "{en}");
        let de = {
            let _de = solidcraft_ui_text::scope("de");
            state::Timeline2DState::format_group_tooltip("Ribs", &feats[..1], true)
        };
        assert!(de.starts_with("Ribs (1)\nSchritt 03 • Aktiv"), "{de}");
        assert!(de.contains("Merkmale in dieser Gruppe:"), "{de}");
    }

    /// The outlines and the corridor follow the chip clicked last while it stays selected, not the
    /// smallest selected id; without such a chip they follow the first selected one.
    #[test]
    fn test_outlines_follow_the_last_clicked_feature_or_joint() {
        let mut state = state::Timeline2DState::default();
        state.select_single(50);
        state.toggle_selection(20);
        assert_eq!(state.focused_feature(), Some(20), "the feature clicked last");
        state.toggle_selection(20);
        assert_eq!(state.focused_feature(), Some(50), "the clicked feature left the selection");

        state.selected_joints = [5, 2].into_iter().collect();
        state.joint_anchor = Some(5);
        assert_eq!(state.focused_joint(), Some(5), "J05 clicked last, though J02 has the smaller id");
        state.joint_anchor = None;
        assert_eq!(state.focused_joint(), Some(2));
        state.selected_joints.clear();
        assert_eq!(state.focused_joint(), None);
    }

    /// A chip click selects the feature in the session, so Delete and the 3D view act on it, and a
    /// selection made elsewhere shows in the 2D Timeline.
    #[test]
    fn test_chip_click_selects_the_feature_in_the_session() {
        use solidcraft_engine::Sel;
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [30, 0, 0]})).unwrap();
        let (first, second) = (h.app.session.doc.features[0].clone(), h.app.session.doc.features[1].clone());
        h.app.run("select.clear", json!({})).unwrap();
        h.frames(3);

        h.step(&json!({"click": {"handle": format!("timeline_2d:{}", first.name)}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.selection, [Sel::Feature { id: first.id }]);

        // Wait out the double-click time: a double click opens the feature to edit.
        h.frames(30);
        h.step(&json!({"click": {"handle": format!("timeline_2d:{}", second.name)}, "ctrl": true})).unwrap();
        h.frames(2);
        assert_eq!(crate::timeline::selected_feature_ids(&h.app), [first.id, second.id]);

        h.app.run("select.set", json!({"items": [{"type": "feature", "id": second.id}]})).unwrap();
        h.frames(2);
        let state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert_eq!(state.selected_features, [second.id].into_iter().collect(), "the 2D Timeline shows the session selection");
    }

    /// With a dialog input that takes features active, a chip click fills the dialog and leaves
    /// the ordinary selection alone, as in the 1D timeline.
    #[test]
    fn test_chip_click_picks_the_feature_into_an_open_dialog() {
        use solidcraft_engine::Sel;
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.app.run("solid.box", json!({"length": 80, "width": 60, "height": 20})).unwrap();
        h.app.run("solid.cylinder", json!({"base": [20, 20, 20], "radius": 5, "height": 10, "operation": "join"})).unwrap();
        let id = h.app.session.doc.find_feature("Cylinder1").unwrap().id;
        h.app.run("select.clear", json!({})).unwrap();
        h.app.start("solid.pattern.rectangular");
        h.frames(4);
        h.step(&json!({"click": {"handle": "dialog:Object Type"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Object Type:Features"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline_2d:Cylinder1"}})).unwrap();
        h.frames(2);

        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs[0].items, vec![Sel::Feature { id }], "the dialog receives the feature");
        assert!(h.app.session.selection.is_empty(), "a dialog pick does not replace the ordinary selection");
    }

    /// The panel host's rectangle for `instance`, while it shows.
    fn panel_rect(h: &Harness, instance: &str) -> Option<egui::Rect> {
        let frame = h.app.universal_panel_frame()?;
        frame.layout.sections.iter().find(|s| s.instance.0 == instance).map(|s| crate::panel_runtime::egui_rect(s.outer))
    }

    /// The height the 2D Timeline's grid gets: the panel's body inside the content slot's inset.
    fn grid_height(h: &Harness) -> f32 {
        let frame = h.app.universal_panel_frame().unwrap();
        let body = frame.layout.sections.iter().find(|s| s.instance.0 == "timeline-2d").and_then(|s| s.body).unwrap();
        body.size.height - 2.0 * solidcraft_theme::Tokens::get().space_2
    }

    /// Drags the 2D Timeline panel's top edge by `dy` points (negative is up, taller).
    fn drag_top_edge(h: &mut Harness, dy: f32) {
        use crate::panel_runtime::{HostControlTarget, HostPointerTarget};
        let frame = h.app.universal_panel_frame().unwrap();
        let section = frame.layout.sections.iter().find(|s| s.instance.0 == "timeline-2d").unwrap().section.clone();
        let edge = frame
            .controls
            .iter()
            .find(|c| matches!(&c.target, HostControlTarget::Pointer(HostPointerTarget::Resize { section: s, .. }) if *s == section))
            .unwrap();
        let at = crate::panel_runtime::egui_rect(edge.bounds).center();
        h.call("ui.drag", json!({"x0": at.x, "y0": at.y, "x1": at.x, "y1": at.y + dy, "steps": 8}));
        h.frames(3);
    }

    /// Clicks the host control for `target` and lets the frame settle.
    fn click_control(h: &mut Harness, target: &crate::panel_runtime::HostControlTarget) {
        let frame = h.app.universal_panel_frame().unwrap();
        let control = frame.controls.iter().find(|c| c.target == *target).unwrap_or_else(|| panic!("no control {target:?}"));
        let at = crate::panel_runtime::egui_rect(control.bounds).center();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        h.frames(3);
    }

    /// Floating the 2D Timeline from its menu leaves it just above where it was docked, at its
    /// width and height, instead of at the free area's corner over the Browser.
    #[test]
    fn test_floating_the_2d_timeline_leaves_it_where_it_was() {
        use crate::panel_runtime::{HostControlTarget, HostSharedAction};
        use solidcraft_engine::panels::{InstanceId, SectionLocation, Zone};
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(3);
        let docked = panel_rect(&h, "timeline-2d").unwrap();
        let instance = InstanceId("timeline-2d".into());
        click_control(&mut h, &HostControlTarget::Pointer(crate::panel_runtime::HostPointerTarget::Menu { instance: instance.clone() }));
        click_control(&mut h, &HostControlTarget::Shared { instance: instance.clone(), action: HostSharedAction::Move(Zone::Floating) });
        let layout = h.app.panel_runtime().unwrap().layout().layout().clone();
        let section = layout.containing(&instance).unwrap();
        assert!(matches!(section.location, SectionLocation::Floating { .. }), "{:?}", section.location);
        let floated = panel_rect(&h, "timeline-2d").unwrap();
        let step = solidcraft_theme::Tokens::get().space_24;
        assert_eq!((floated.left(), floated.top()), (docked.left() + step, docked.top() - step));
        assert_eq!(floated.height(), docked.height());
    }

    /// A collapsed track's title never runs into its count badge: a short strip (a one-row
    /// timeline) gets the short title, and one too short for that shows no title.
    #[test]
    fn test_a_collapsed_track_title_fits_under_its_badge() {
        use super::assembly::collapsed_strip_title;
        let width = |s: &str| s.len() as f32 * 6.0;
        assert_eq!(collapsed_strip_title(false, 200.0, width), Some("Assembly & Joints"));
        assert_eq!(collapsed_strip_title(false, 60.0, width), Some("Assembly"));
        assert_eq!(collapsed_strip_title(true, 60.0, width), Some("Modeling"));
        assert_eq!(collapsed_strip_title(false, 30.0, width), None);
        assert_eq!(collapsed_strip_title(true, -5.0, width), None);
    }

    /// A corridor is checked outward from the feature and stops at the first move that fails,
    /// even when a farther one would work, so it stays one run of steps around the feature.
    #[test]
    fn a_checked_corridor_stops_at_the_first_move_that_fails() {
        use super::corridor_check::walk;
        let works = |ok: &'static [usize]| move |step: usize| Some(ok.contains(&step));
        assert_eq!(walk(5, (1..5).rev(), works(&[4, 3, 1])), Some(3));
        assert_eq!(walk(5, 6..=9, works(&[6, 7, 8, 9])), Some(9));
        assert_eq!(walk(5, 6..=9, works(&[])), Some(5), "no move works: only its own step");
        assert_eq!(walk(5, 6..=9, |_| None), None, "cancelled: no answer");
    }

    /// While a side is checked, the light runs from that end of the known corridor towards the
    /// feature and ends at the feature's own step (its dots in the other rows); the steps outside
    /// the corridor stay dark.
    #[test]
    fn the_running_light_moves_from_the_corridor_end_towards_the_feature() {
        let mut s = super::corridor_check::Shown { low: 2.0, high: 8.0, light_low: Some(1.0), light_high: None, known: (2, 8), current: 6 };
        assert!(s.light_at(3) == 1.0 && s.light_at(2) > 0.0 && s.light_at(2) < 1.0, "the head is one step in, its tail behind");
        assert_eq!(s.light_at(4), 0.0, "not reached yet");
        assert_eq!((s.light_at(6), s.light_at(7), s.light_at(1)), (0.0, 0.0, 0.0));
        s.light_low = Some(4.0);
        assert_eq!(s.light_at(6), 1.0, "the head reaches the feature's own step");
        s.light_low = None;
        s.light_high = Some(2.0);
        assert_eq!((s.light_at(6), s.light_at(9)), (1.0, 0.0), "from the other side too");
    }

    /// The graph can't see every way a move breaks a feature (a sketch that projects a face a
    /// later cut changes), so the corridor is checked with real moves on a copy of the design:
    /// given a corridor too wide, the check narrows it to the moves that work, and leaves the
    /// design as it was.
    #[test]
    fn a_checked_corridor_narrows_to_the_moves_that_work() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [30, 0, 0]})).unwrap();
        s.execute("solid.fillet", &json!({"edges": [[5, 0, 10]], "radius": 1})).unwrap();
        let fillet = s.doc.features[2].id;
        let before = s.doc.clone();
        let ctx = egui::Context::default();
        let mut checks = super::corridor_check::Checks::default();
        let mut found = None;
        for _ in 0..3000 {
            checks.follow(&s, Some((fillet, (0, 2), 2)), 0.0, &ctx);
            found = checks.corridor(fillet);
            if found.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(found, Some((1, 2)), "it can go before the second box, not before the one it rounds");
        assert_eq!(s.doc, before);
    }

    /// Collapsed while floating, the 2D Timeline's header shows at the floating panel's bottom
    /// edge, not in the middle of the view; expanding brings the panel back where it was.
    #[test]
    fn test_the_collapsed_floating_2d_timeline_keeps_its_bottom_edge() {
        use crate::panel_runtime::{HostControlTarget, HostPointerTarget, HostSharedAction};
        use solidcraft_engine::panels::{InstanceId, Zone};
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(3);
        let instance = InstanceId("timeline-2d".into());
        let menu = HostControlTarget::Pointer(HostPointerTarget::Menu { instance: instance.clone() });
        click_control(&mut h, &menu);
        click_control(&mut h, &HostControlTarget::Shared { instance: instance.clone(), action: HostSharedAction::Move(Zone::Floating) });
        let floating = panel_rect(&h, "timeline-2d").unwrap();
        click_control(&mut h, &menu);
        click_control(&mut h, &HostControlTarget::Shared { instance: instance.clone(), action: HostSharedAction::Collapse });
        let header = panel_rect(&h, "timeline-2d").unwrap();
        assert!(header.height() < 60.0, "{header:?}");
        assert_eq!(header.bottom(), floating.bottom());
        assert_eq!(header.left(), floating.left());
        click_control(&mut h, &menu);
        click_control(&mut h, &HostControlTarget::Shared { instance, action: HostSharedAction::Collapse });
        assert_eq!(panel_rect(&h, "timeline-2d").unwrap(), floating);
    }

    /// The 2D Timeline's menu places it at the bottom, floating or in its own window; it never
    /// joins another panel's tabs, where it would be too cramped to use.
    #[test]
    fn test_the_2d_timeline_menu_offers_no_join() {
        use crate::panel_runtime::{HostControlTarget, HostPointerTarget, HostSharedAction};
        use solidcraft_engine::panels::{InstanceId, Zone};
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(3);
        h.app.set_panel_native_adapter(Box::new(crate::panel_runtime::native::DesktopNativeAdapter::default())).unwrap();
        h.frames(2);
        let instance = InstanceId("timeline-2d".into());
        click_control(&mut h, &HostControlTarget::Pointer(HostPointerTarget::Menu { instance: instance.clone() }));
        let frame = h.app.universal_panel_frame().unwrap();
        let offered: Vec<_> = frame
            .controls
            .iter()
            .filter_map(|c| match &c.target {
                HostControlTarget::Shared { instance: owner, action } if *owner == instance => Some(action.clone()),
                _ => None,
            })
            .collect();
        assert!(offered.contains(&HostSharedAction::Move(Zone::Floating)), "{offered:?}");
        assert!(offered.contains(&HostSharedAction::Popout), "{offered:?}");
        assert!(!offered.iter().any(|a| matches!(a, HostSharedAction::Join(_))), "{offered:?}");
    }

    /// With the preference on, the panel host shows the 2D Timeline in History's place at the
    /// bottom; its top edge resizes it, down to one row, and turning the preference off brings
    /// History back.
    #[test]
    fn test_timeline_2d_panel_replaces_history_and_resizes() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(3);
        assert!(panel_rect(&h, "history").is_none());
        let start = panel_rect(&h, "timeline-2d").expect("the 2D Timeline shows");
        assert_eq!(grid_height(&h), canvas::DOCK_TOTAL_HEIGHT, "the grid gets its default height under the host's header");
        assert_eq!(crate::scenario::rect_of("history:card"), Some(start), "it stands in for History's rectangle");

        drag_top_edge(&mut h, -60.0);
        assert_eq!(panel_rect(&h, "timeline-2d").unwrap().height(), start.height() + 60.0);
        drag_top_edge(&mut h, 200.0);
        assert_eq!(grid_height(&h), canvas::MIN_DOCK_HEIGHT, "one row at least, below the host's header");

        h.app.preferences.timeline_2d = false;
        h.frames(3);
        assert!(panel_rect(&h, "timeline-2d").is_none());
        assert!(panel_rect(&h, "history").is_some(), "History is back");
    }

    /// A panel layout saved before the 2D Timeline panel existed gains it when the preference is
    /// turned on, in History's place.
    #[test]
    fn test_a_layout_saved_without_the_2d_panel_gains_it() {
        use solidcraft_engine::panels::{InstanceId, LayoutAction, Mutation};
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(3);
        let instance = InstanceId::new("timeline-2d").unwrap();
        let destroy = LayoutAction::Apply { expected_revision: None, mutations: vec![Mutation::Destroy { instance: instance.clone() }] };
        h.app.dispatch_panel_layout(destroy).unwrap();
        assert!(h.app.panel_runtime.as_ref().unwrap().runtime().layout().layout().instance(&instance).is_err());

        h.app.preferences.timeline_2d = true;
        h.frames(4);
        assert!(panel_rect(&h, "timeline-2d").is_some(), "the 2D Timeline is added");
        assert!(panel_rect(&h, "history").is_none());
    }

    /// Verifies opening, editing, and submitting the Rename Group dialog.
    #[test]
    fn test_rename_group_dialog() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [30, 0, 0]})).unwrap();
        let f0 = h.app.session.doc.features[0].id;
        let f1 = h.app.session.doc.features[1].id;

        let res = h
            .app
            .run_heavy(
                "timeline.group",
                json!({
                    "name": "Group 1",
                    "features": [f0, f1],
                }),
            )
            .unwrap();
        let gid = res["group"].as_u64().unwrap();

        let mut d = crate::dialogs::Dialog::rename_group(gid, "Group 1");
        assert_eq!(crate::dialogs::report_title(&d), "RENAME GROUP");

        if let crate::dialogs::Kind::RenameGroup { name, .. } = &mut d.kind {
            *name = "Custom Assembly".to_string();
        }
        let cmds = crate::dialogs::apply_commands(&h.app, &d).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].0, "timeline.rename_group");
        assert_eq!(cmds[0].1["group"], gid);
        assert_eq!(cmds[0].1["name"], "Custom Assembly");

        h.app.dialog = Some(d);
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
        h.frames(2);

        assert_eq!(h.app.session.doc.timeline_groups.iter().find(|g| g.id == gid).map(|g| g.name.as_str()), Some("Custom Assembly"));
    }

    /// Verifies that group deletion with dependents preserves the group and its contents when cancelled.
    #[test]
    fn test_group_delete_preserves_group_on_cancellation() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;

        // Create base box
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();
        let f0 = h.app.session.doc.features[0].id;

        // Group the box
        let res = h
            .app
            .run_heavy(
                "timeline.group",
                json!({
                    "name": "Box Group",
                    "features": [f0],
                }),
            )
            .unwrap();
        let gid = res["group"].as_u64().unwrap();

        // Simulate folder deletion with confirmation dialog (ConfirmDelete with group id attached)
        let contents = crate::timeline_left::folder_features(&h.app, &[gid]);
        h.app.dialog = Some(crate::dialogs::Dialog::confirm_delete_many(contents.clone(), vec![gid], vec!["Box1".into()], vec!["Fillet1".into()]));
        h.frames(2);

        // A confirmation dialog must be open
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::ConfirmDelete { .. })));

        // User cancels the deletion
        h.step(&json!({"click": {"handle": "dialog:cancel"}})).unwrap();
        h.frames(2);

        // The group and the feature must still exist intact because deletion was cancelled!
        assert!(h.app.session.doc.timeline_groups.iter().any(|g| g.id == gid), "Group must remain intact after cancellation");
        assert!(h.app.session.doc.features.iter().any(|f| f.id == f0), "Group feature must remain intact after cancellation");
        assert!(h.app.dialog.is_none());

        // Now test confirming deletion: both group and features are deleted atomically
        h.app.dialog = Some(crate::dialogs::Dialog::confirm_delete_many(contents, vec![gid], vec!["Box1".into()], vec!["Fillet1".into()]));
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
        h.frames(2);

        // Group and features are now removed
        assert!(!h.app.session.doc.timeline_groups.iter().any(|g| g.id == gid), "Group must be removed after confirmation");
        assert!(!h.app.session.doc.features.iter().any(|f| f.id == f0), "Feature must be removed after confirmation");
    }

    /// Verifies rendering of multi-component feature chips and vertical connector lines across tracks.
    #[test]
    fn test_multi_component_chips_and_connector() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;

        // 1. Box in root component 0
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10})).unwrap();

        // 2. Create Component 1
        let c1 = h.app.run("component.create", json!({"name": "Housing", "activate": true})).unwrap();
        let comp1 = c1["component"].as_u64().unwrap();

        // 3. Box in Component 1
        h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [5, 0, 0]})).unwrap();

        let b0 = h.app.session.model.state().bodies[0].name.clone();
        let b1 = h.app.session.model.state().bodies[1].name.clone();

        // 4. Combine b0 (in component 0) and b1 (in component 1)
        h.app.run("solid.combine", json!({"target": b0, "tools": [b1], "operation": "join"})).unwrap();

        let combine_feat = h.app.session.doc.features.last().unwrap();
        let affected = canvas::feature_affected_components(&h.app.session.doc, Some(&h.app.session.model), combine_feat);
        assert_eq!(affected, vec![0, comp1]);

        // 5. Chamfer in Component 1 on edge of combined body
        let m = h.app.session.model.state().bodies[0].mesh();
        let p0 = m.edges[0][0];
        let p1 = m.edges[0][1];
        let edge_mid = (p0 + p1) * 0.5;
        h.app
            .run(
                "solid.chamfer",
                json!({
                    "edges": [[edge_mid.x, edge_mid.y, edge_mid.z]],
                    "distance": 1.0,
                }),
            )
            .unwrap();

        let chamfer_feat = h.app.session.doc.features.last().unwrap();
        let chamfer_affected = canvas::feature_affected_components(&h.app.session.doc, Some(&h.app.session.model), chamfer_feat);
        assert!(chamfer_affected.contains(&0) && chamfer_affected.contains(&comp1));

        // Render frames and verify no panics
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(3);
    }

    #[test]
    fn test_chip_drag_horizontal_reorder() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;

        // 1. Box 1 in root component 0
        h.app.run("solid.box", json!({"name": "BoxA", "length": 20, "width": 20, "height": 10})).unwrap();
        let f0 = h.app.session.doc.features[0].id;

        // 2. Box 2 in root component 0 (independent, non-overlapping)
        h.app.run("solid.box", json!({"name": "BoxB", "length": 15, "width": 15, "height": 10, "corner": [50, 0, 0]})).unwrap();
        let f1 = h.app.session.doc.features[1].id;

        // Verify initial sequence: [BoxA, BoxB]
        assert_eq!(h.app.session.doc.features[0].id, f0);
        assert_eq!(h.app.session.doc.features[1].id, f1);

        // 3. Test horizontal reorder: move BoxB before BoxA
        let res = h.app.run_heavy(
            "timeline.reorder",
            json!({
                "feature": f1,
                "position": 0,
                "active": true,
            }),
        );
        assert!(res.is_ok(), "reorder BoxB to position 0 should succeed");
        assert_eq!(h.app.session.doc.features[0].id, f1);
        assert_eq!(h.app.session.doc.features[1].id, f0);

        // Render frames with drag state active
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(2);
    }

    #[test]
    fn test_suppressed_chips_and_dependency_cascade() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;

        // 1. Base box in root component
        h.app.run("solid.box", json!({"name": "BaseBox", "length": 30, "width": 30, "height": 15})).unwrap();
        let box_id = h.app.session.doc.features[0].id;

        // 2. Chamfer on an edge of BaseBox
        let m = h.app.session.model.state().bodies[0].mesh();
        let p0 = m.edges[0][0];
        let p1 = m.edges[0][1];
        let edge_mid = (p0 + p1) * 0.5;
        h.app
            .run(
                "solid.chamfer",
                json!({
                    "edges": [[edge_mid.x, edge_mid.y, edge_mid.z]],
                    "distance": 1.0,
                }),
            )
            .unwrap();
        let chamfer_id = h.app.session.doc.features[1].id;

        // 3. Verify dependency graph: chamfer depends on BaseBox
        let dep_graph = deps::DependencyGraph::from_doc(&h.app.session.doc);
        assert!(dep_graph.is_dependent(box_id, chamfer_id));
        assert!(dep_graph.is_prerequisite(chamfer_id, box_id));
        assert!(!dep_graph.is_suppressed(&h.app.session.doc, box_id));
        assert!(!dep_graph.is_suppressed(&h.app.session.doc, chamfer_id));

        // 4. Suppression cascade: suppressing BaseBox must include dependent chamfer
        let cascade = dep_graph.collect_suppression_cascade(box_id);
        assert!(cascade.contains(&box_id));
        assert!(cascade.contains(&chamfer_id));

        let res = h.app.run_heavy(
            "timeline.suppress",
            json!({
                "features": cascade,
                "suppressed": true,
            }),
        );
        assert!(res.is_ok());
        assert!(h.app.session.doc.feature(box_id).unwrap().suppressed);
        assert!(h.app.session.doc.feature(chamfer_id).unwrap().suppressed);
        assert!(dep_graph.is_suppressed(&h.app.session.doc, box_id));
        assert!(dep_graph.is_suppressed(&h.app.session.doc, chamfer_id));

        // 5. Unsuppression cascade: unsuppressing chamfer must restore required BaseBox
        let unsuppress_cascade = dep_graph.collect_unsuppression_cascade(&h.app.session.doc, chamfer_id);
        assert!(unsuppress_cascade.contains(&box_id));
        assert!(unsuppress_cascade.contains(&chamfer_id));

        let res = h.app.run_heavy(
            "timeline.suppress",
            json!({
                "features": unsuppress_cascade,
                "suppressed": false,
            }),
        );
        assert!(res.is_ok());
        assert!(!h.app.session.doc.feature(box_id).unwrap().suppressed);
        assert!(!h.app.session.doc.feature(chamfer_id).unwrap().suppressed);
        assert!(!dep_graph.is_suppressed(&h.app.session.doc, box_id));
        assert!(!dep_graph.is_suppressed(&h.app.session.doc, chamfer_id));

        // 6. Re-suppress BaseBox and render frames to exercise UI chip drawing (dashed border & 50% opacity slashed eye)
        let res = h.app.run_heavy(
            "timeline.suppress",
            json!({
                "features": cascade,
                "suppressed": true,
            }),
        );
        assert!(res.is_ok());

        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(3);
    }

    #[test]
    fn test_separately_suppressed_features_and_unsuppress_modes() {
        use crate::prefs::Timeline2dUnsuppressMode;
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;

        // 1. Branching DAG test: BaseBox -> Chamfer and BaseBox -> Fillet
        h.app.run("solid.box", json!({"name": "BaseBox", "length": 30, "width": 30, "height": 15})).unwrap();
        let box_id = h.app.session.doc.features[0].id;

        let m = h.app.session.model.state().bodies[0].mesh();
        let p0 = m.edges[0][0];
        let p1 = m.edges[0][1];
        let edge_mid = (p0 + p1) * 0.5;
        h.app
            .run(
                "solid.chamfer",
                json!({
                    "edges": [[edge_mid.x, edge_mid.y, edge_mid.z]],
                    "distance": 1.0,
                }),
            )
            .unwrap();
        let chamfer_id = h.app.session.doc.features[1].id;

        let m2 = h.app.session.model.state().bodies[0].mesh();
        let p0_2 = m2.edges[2][0];
        let p1_2 = m2.edges[2][1];
        let edge_mid_2 = (p0_2 + p1_2) * 0.5;
        h.app
            .run(
                "solid.fillet",
                json!({
                    "edges": [[edge_mid_2.x, edge_mid_2.y, edge_mid_2.z]],
                    "radius": 0.5,
                }),
            )
            .unwrap();
        let fillet_id = h.app.session.doc.features[2].id;

        let dep_graph = deps::DependencyGraph::from_doc(&h.app.session.doc);
        assert!(dep_graph.is_dependent(box_id, chamfer_id));
        assert!(dep_graph.is_dependent(box_id, fillet_id));

        let mut state = state::Timeline2DState::default();

        // 2. User separately suppresses Chamfer; Fillet remains active
        state.mark_separately_suppressed(chamfer_id);
        h.app.run_heavy("timeline.suppress", json!({"features": [chamfer_id], "suppressed": true})).unwrap();
        assert!(h.app.session.doc.feature(chamfer_id).unwrap().suppressed);
        assert!(!h.app.session.doc.feature(fillet_id).unwrap().suppressed);

        // 3. User now suppresses BaseBox (upstream prerequisite of both)
        state.mark_separately_suppressed(box_id);
        let box_cascade = dep_graph.collect_suppression_cascade(box_id);
        assert!(box_cascade.contains(&box_id));
        assert!(box_cascade.contains(&chamfer_id));
        assert!(box_cascade.contains(&fillet_id));
        h.app.run_heavy("timeline.suppress", json!({"features": box_cascade, "suppressed": true})).unwrap();
        assert!(h.app.session.doc.feature(box_id).unwrap().suppressed);
        assert!(h.app.session.doc.feature(chamfer_id).unwrap().suppressed);
        assert!(h.app.session.doc.feature(fillet_id).unwrap().suppressed);

        // 4. Test Mode 1: RestoreState (Default)
        // Unsuppressing BaseBox restores BaseBox and Fillet, but Chamfer was separately suppressed,
        // so Chamfer REMAINS SUPPRESSED!
        let restore_targets = dep_graph.collect_unsuppression_with_mode(
            &h.app.session.doc,
            &[box_id],
            Timeline2dUnsuppressMode::RestoreState,
            &state.separately_suppressed_features,
        );
        assert!(restore_targets.contains(&box_id));
        assert!(restore_targets.contains(&fillet_id));
        assert!(!restore_targets.contains(&chamfer_id), "separately suppressed feature must not be in restore list");
        state.unmark_separately_suppressed(box_id);
        h.app.run_heavy("timeline.suppress", json!({"features": restore_targets, "suppressed": false})).unwrap();

        assert!(!h.app.session.doc.feature(box_id).unwrap().suppressed, "BaseBox is restored");
        assert!(!h.app.session.doc.feature(fillet_id).unwrap().suppressed, "Fillet was not separately suppressed and is restored");
        assert!(h.app.session.doc.feature(chamfer_id).unwrap().suppressed, "Chamfer was separately suppressed and remains suppressed");

        // 5. User explicitly unsuppresses Chamfer
        let chamfer_unsuppress = dep_graph.collect_unsuppression_with_mode(
            &h.app.session.doc,
            &[chamfer_id],
            Timeline2dUnsuppressMode::RestoreState,
            &state.separately_suppressed_features,
        );
        assert_eq!(chamfer_unsuppress, vec![chamfer_id]);
        state.unmark_separately_suppressed(chamfer_id);
        h.app.run_heavy("timeline.suppress", json!({"features": chamfer_unsuppress, "suppressed": false})).unwrap();
        assert!(!h.app.session.doc.feature(chamfer_id).unwrap().suppressed);

        // 6. Test Mode 2: UnsuppressAll
        // Separately suppress Chamfer, then suppress BaseBox
        state.mark_separately_suppressed(chamfer_id);
        h.app.run_heavy("timeline.suppress", json!({"features": [chamfer_id], "suppressed": true})).unwrap();
        state.mark_separately_suppressed(box_id);
        h.app.run_heavy("timeline.suppress", json!({"features": [box_id, fillet_id], "suppressed": true})).unwrap();

        let all_targets = dep_graph.collect_unsuppression_with_mode(
            &h.app.session.doc,
            &[box_id],
            Timeline2dUnsuppressMode::UnsuppressAll,
            &state.separately_suppressed_features,
        );
        assert!(all_targets.contains(&box_id));
        assert!(all_targets.contains(&chamfer_id));
        assert!(all_targets.contains(&fillet_id));
        h.app.run_heavy("timeline.suppress", json!({"features": all_targets, "suppressed": false})).unwrap();
        assert!(!h.app.session.doc.feature(box_id).unwrap().suppressed);
        assert!(!h.app.session.doc.feature(chamfer_id).unwrap().suppressed);
        assert!(!h.app.session.doc.feature(fillet_id).unwrap().suppressed);

        // 7. Verify Preferences setting control via ui.prefs
        assert_eq!(h.app.preferences.timeline_2d_unsuppress_mode, Timeline2dUnsuppressMode::RestoreState);
        h.call("ui.prefs", json!({"timeline_2d_unsuppress_mode": "unsuppress_all"}));
        assert_eq!(h.app.preferences.timeline_2d_unsuppress_mode, Timeline2dUnsuppressMode::UnsuppressAll);
        h.call("ui.prefs", json!({"timeline_2d_unsuppress_mode": "restore_state"}));
        assert_eq!(h.app.preferences.timeline_2d_unsuppress_mode, Timeline2dUnsuppressMode::RestoreState);
    }

    /// Verifies text truncation, ellipsis formatting, and tooltip generation for timeline group headers.
    #[test]
    fn test_group_header_truncation_and_tooltip() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(1);

        let ctx = h.ctx.clone();
        let painter =
            egui::Painter::new(ctx.clone(), egui::LayerId::background(), egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1000.0)));

        let font = egui::FontId::proportional(10.0);
        let color = egui::Color32::WHITE;

        // 1. Long name in narrow width (48.0 pt for 2 columns): should be truncated with ellipsis
        let long_name = "Foundation Structures";
        let (fitted, is_cut) = canvas::fit_group_banner_title(&painter, long_name, 2, 48.0, font.clone(), color);
        assert!(is_cut, "Long name in 48pt width should be cut");
        assert!(fitted.contains('…'), "Fitted title should contain ellipsis");
        assert!(fitted.ends_with("(2)"), "Fitted title should preserve member count suffix");
        let measured_w = painter.layout_no_wrap(fitted.clone(), font.clone(), color).size().x;
        assert!(measured_w <= 48.0, "Measured width {} must fit within 48.0", measured_w);

        // 2. Wide width (300.0 pt): should NOT be cut
        let (wide_fitted, wide_is_cut) = canvas::fit_group_banner_title(&painter, long_name, 2, 300.0, font.clone(), color);
        assert!(!wide_is_cut, "Long name in 300pt width should not be cut");
        assert_eq!(wide_fitted, "Foundation Structures (2)");

        // 3. Extremely narrow width (10.0 pt): should not crash or exceed width
        let (tiny_fitted, tiny_is_cut) = canvas::fit_group_banner_title(&painter, long_name, 2, 10.0, font.clone(), color);
        assert!(tiny_is_cut);
        let tiny_w = painter.layout_no_wrap(tiny_fitted, font.clone(), color).size().x;
        assert!(tiny_w <= 10.0);

        // 4. Create features and group in a design to exercise rendering with truncation
        h.app.run("solid.box", json!({"length": 20.0, "width": 20.0, "height": 10.0})).unwrap();
        h.app.run("solid.box", json!({"length": 15.0, "width": 15.0, "height": 10.0, "corner": [30, 0, 0]})).unwrap();
        let box1_id = h.app.session.doc.features[0].id;
        let box2_id = h.app.session.doc.features[1].id;

        let res = h.app.run_heavy(
            "timeline.group",
            json!({
                "features": [box1_id, box2_id],
                "name": "Very Long Custom Group Name That Exceeds Two Columns",
            }),
        );
        assert!(res.is_ok());

        h.frames(3);
    }

    /// Verifies active component highlighting in header labels and clicking to switch the active component.
    #[test]
    fn test_active_component_highlighting_and_activation() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));

        // 1. Initial active component is Root (0)
        assert_eq!(h.app.session.active_component, 0);

        // Feature created while root is active belongs to component 0
        h.app.run("solid.box", json!({"length": 20.0, "width": 20.0, "height": 10.0})).unwrap();
        assert_eq!(h.app.session.doc.features.last().unwrap().component, 0);

        // 2. Create Component 1 "Housing" with activate: false
        let c1 = h.app.run("component.create", json!({"name": "Housing", "activate": false})).unwrap();
        let comp1 = c1["component"].as_u64().unwrap();
        assert_eq!(h.app.session.active_component, 0);

        // 3. Create Component 2 "Shaft" with activate: true
        let c2 = h.app.run("component.create", json!({"name": "Shaft", "activate": true})).unwrap();
        let comp2 = c2["component"].as_u64().unwrap();
        assert_eq!(h.app.session.active_component, comp2);

        // Feature created while comp2 is active belongs to comp2
        h.app.run("solid.box", json!({"length": 15.0, "width": 15.0, "height": 10.0, "corner": [30, 0, 0]})).unwrap();
        assert_eq!(h.app.session.doc.features.last().unwrap().component, comp2);

        // 4. Render frames to exercise paint_pinned_left with active radio and track wash
        h.frames(3);

        // 5. Exclusively activate component 1 "Housing"
        let res = h.app.run("component.activate", json!({"component": comp1}));
        assert!(res.is_ok());
        assert_eq!(h.app.session.active_component, comp1);

        // New feature created now automatically belongs to component 1!
        h.app.run("solid.box", json!({"length": 10.0, "width": 10.0, "height": 5.0, "corner": [60, 0, 0]})).unwrap();
        assert_eq!(h.app.session.doc.features.last().unwrap().component, comp1);

        // 6. Switch back to Root component (0)
        let res = h.app.run("component.activate", json!({"component": 0}));
        assert!(res.is_ok());
        assert_eq!(h.app.session.active_component, 0);

        h.frames(3);
    }

    #[test]
    fn test_corridor_coincident_marker_smart_inset() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(1);

        let ctx = h.ctx.clone();
        let painter =
            egui::Painter::new(ctx.clone(), egui::LayerId::background(), egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1000.0)));

        let color = egui::Color32::from_rgb(34, 197, 94);

        // 1. paint_bracket_close when coincident with marker_x: insets by 3px
        canvas::paint_bracket_close(&painter, 100.0, 20.0, 80.0, color, Some(100.0));

        // 2. paint_bracket_close when NOT coincident
        canvas::paint_bracket_close(&painter, 100.0, 20.0, 80.0, color, Some(200.0));

        // 3. paint_bracket_open when coincident with marker_x: insets by 3px
        canvas::paint_bracket_open(&painter, 50.0, 0.0, 20.0, 80.0, color, Some(50.0));

        // 4. paint_bracket_open when NOT coincident
        canvas::paint_bracket_open(&painter, 50.0, 0.0, 20.0, 80.0, color, Some(200.0));

        // 5. Render full frames with rollback marker to ensure integration works cleanly
        h.app.run("solid.box", json!({"length": 20.0, "width": 20.0, "height": 10.0})).unwrap();
        h.app.run("solid.box", json!({"length": 20.0, "width": 20.0, "height": 10.0, "corner": [30, 0, 0]})).unwrap();
        let _ = h.app.run_heavy("timeline.roll_to", json!({"position": 1}));
        h.frames(3);
    }

    #[test]
    fn test_timeline_2d_custom_options_and_cell_dots() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));

        // 1. The defaults: dots shown, a row's label activates its component.
        assert!(h.app.preferences.timeline_2d_cell_dots);
        assert!(h.app.preferences.timeline_2d_component_activation);

        // 2. The options are behind the 2D Timeline's settings button: each item flips its option.
        use crate::context_menu::{Target, items, run_item};
        use crate::panel_runtime::{HostControlTarget, HostPointerTarget};
        use solidcraft_engine::panels::{ControlId, InstanceId};
        h.app.home.open = false;
        h.frames(3);
        let button = |instance: &str, action: &str| {
            HostControlTarget::Pointer(HostPointerTarget::HeaderAction { instance: InstanceId(instance.into()), action: ControlId(action.into()) })
        };
        let bounds = |h: &Harness, target: &HostControlTarget| {
            let frame = h.app.universal_panel_frame().unwrap();
            frame.controls.iter().find(|c| c.target == *target).map(|c| crate::panel_runtime::egui_rect(c.bounds))
        };
        click_control(&mut h, &button("timeline-2d", "settings"));
        assert_eq!(h.app.menu.target, Target::Timeline2dOptions, "the settings button opens the options");
        let run = |h: &mut Harness, id: &str| {
            let item = items(&h.app, &Target::Timeline2dOptions).into_iter().find(|i| i.id == id).unwrap_or_else(|| panic!("the menu offers {id}"));
            run_item(&mut h.app, &item, egui::Pos2::ZERO);
        };
        run(&mut h, "ui.timeline2dCellDots");
        assert!(!h.app.preferences.timeline_2d_cell_dots);
        run(&mut h, "ui.timeline2dActivation");
        assert!(!h.app.preferences.timeline_2d_component_activation);
        run(&mut h, "ui.timeline2dActivation");
        assert!(h.app.preferences.timeline_2d_component_activation);
        let all = items(&h.app, &Target::Timeline2dOptions)
            .into_iter()
            .find(|i| i.params.as_ref().is_some_and(|p| p["mode"] == "unsuppress_all"))
            .expect("the menu offers both modes");
        run_item(&mut h.app, &all, egui::Pos2::ZERO);
        assert_eq!(h.app.preferences.timeline_2d_unsuppress_mode, crate::prefs::Timeline2dUnsuppressMode::UnsuppressAll);
        assert!(!items(&h.app, &Target::HistoryControls).iter().any(|i| i.id.starts_with("ui.timeline2d")), "the Play button's menu stays as it was");

        // 3. The view switch sits at the same place in both headers, and switches.
        let in_2d = bounds(&h, &button("timeline-2d", "timeline-view")).expect("the 2D Timeline has the switch");
        click_control(&mut h, &button("timeline-2d", "timeline-view"));
        assert!(!h.app.preferences.timeline_2d);
        h.frames(3);
        let in_1d = bounds(&h, &button("history", "timeline-view")).expect("the 1D History has the switch");
        assert!((in_1d.center().x - in_2d.center().x).abs() < 1.0, "the switch lines up: {in_1d:?} {in_2d:?}");
        click_control(&mut h, &button("history", "timeline-view"));
        assert!(h.app.preferences.timeline_2d);

        // A History on the left has no switch: the 2D Timeline shows only at the bottom.
        h.app.preferences.timeline_left = true;
        h.frames(3);
        assert!(bounds(&h, &button("history", "timeline-view")).is_none());
        h.app.preferences.timeline_left = false;
        h.app.preferences.timeline_2d_cell_dots = true;
        h.frames(3);

        // 4. Test disabling component activation from timeline
        let c1 = h.app.run("component.create", json!({"name": "TestPart", "activate": false})).unwrap();
        let comp1 = c1["component"].as_u64().unwrap();
        assert_eq!(h.app.session.active_component, 0);

        h.app.preferences.timeline_2d_component_activation = false;
        // Render frames with activation disabled
        h.frames(3);
        assert_eq!(h.app.session.active_component, 0);

        // Re-enable and verify component.activate
        h.app.preferences.timeline_2d_component_activation = true;
        let res = h.app.run("component.activate", json!({"component": comp1}));
        assert!(res.is_ok());
        assert_eq!(h.app.session.active_component, comp1);

        // 5. Test rendering frames with cell dots enabled and disabled
        h.app.run("solid.box", json!({"length": 20.0, "width": 20.0, "height": 10.0})).unwrap();
        h.app.preferences.timeline_2d_cell_dots = true;
        h.frames(2);
        h.app.preferences.timeline_2d_cell_dots = false;
        h.frames(2);
    }

    /// Verifies the modeling and assembly split tracks, joint chips, and splitter handle interactions.
    #[test]
    fn test_assembly_split_track_and_joints() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));

        // 1. Build two components A and B with boxes and a revolute joint
        crate::test_support::grounded_boxes(&[("A", 0), ("B", 30)], |id, p| h.app.run(id, p).unwrap());
        let res = h.app.run(
            "joint.create",
            json!({
                "type": "revolute",
                "name": "Hinge",
                "a": {"occurrence": "A:1", "face": [10, 10, 10]},
                "b": {"occurrence": "B:1", "face": [40, 10, 0]},
            }),
        );
        assert!(res.is_ok(), "{:?}", res.err());

        // 2. Discover visual joints
        let (component_ids, component_names) = assembly::collect_component_ids_and_names(&h.app.session.doc);
        let visual_joints = assembly::build_visual_joints(&h.app.session.doc, &component_ids, &component_names);

        assert_eq!(visual_joints.len(), 1);
        let j = &visual_joints[0];
        assert_eq!(j.name, "Hinge");
        assert_eq!(j.step_label, "J01");
        assert_eq!(j.kind, solidcraft_engine::doc::joints::JointKind::Revolute);

        // 3. Render in split view (both open)
        h.frames(3);

        // 4. Retrieve state from ui context and test collapse states
        let mut state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert!(!state.is_modeling_collapsed);
        assert!(!state.is_assembly_collapsed);
        assert!((state.modeling_split_ratio - 0.65).abs() < 1e-4);

        // Collapse assembly track into dock tab
        state.is_assembly_collapsed = true;
        state.store_scoped(&h.ctx, Some(h.app.session.uid));
        h.frames(3);

        // Collapse modeling track (re-opening assembly)
        state.is_assembly_collapsed = false;
        state.is_modeling_collapsed = true;
        state.store_scoped(&h.ctx, Some(h.app.session.uid));
        h.frames(3);

        // Restore split
        state.is_modeling_collapsed = false;
        state.store_scoped(&h.ctx, Some(h.app.session.uid));
        h.frames(3);

        let restored = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert!(!restored.is_modeling_collapsed);
        assert!(!restored.is_assembly_collapsed);
    }

    #[test]
    fn test_splitter_button_spacing_and_dynamic_drag_bar() {
        use super::assembly::SPLITTER_WIDTH;
        use super::canvas::{HEADER_TOTAL_HEIGHT, ROW_HEIGHT};

        // 1. One-row configuration (minimal height)
        let one_row_h = HEADER_TOTAL_HEIGHT + ROW_HEIGHT;
        assert_eq!(one_row_h, 106.0);

        let splitter_rect_1row = egui::Rect::from_min_size(egui::pos2(500.0, 100.0), egui::vec2(SPLITTER_WIDTH, one_row_h));
        let grip_center_y_1 = splitter_rect_1row.center().y;

        let max_h = HEADER_TOTAL_HEIGHT + 5.0 * ROW_HEIGHT;
        let t_factor_1 = ((splitter_rect_1row.height() - one_row_h) / (max_h - one_row_h)).clamp(0.0, 1.0);
        let base_grip_len = 24.0;
        let grip_h_1 = base_grip_len * (1.0 + 0.50 * t_factor_1);
        assert_eq!(t_factor_1, 0.0);
        assert_eq!(grip_h_1, 24.0);

        let btn_size = egui::vec2(SPLITTER_WIDTH - 2.0, 20.0);
        let gap = 7.0;
        let btn_upper_center_y_1 = grip_center_y_1 - grip_h_1 / 2.0 - gap - btn_size.y / 2.0;
        let btn_lower_center_y_1 = grip_center_y_1 + grip_h_1 / 2.0 + gap + btn_size.y / 2.0;
        let btn_upper_rect_1 = egui::Rect::from_center_size(egui::pos2(splitter_rect_1row.center().x, btn_upper_center_y_1), btn_size);
        let btn_lower_rect_1 = egui::Rect::from_center_size(egui::pos2(splitter_rect_1row.center().x, btn_lower_center_y_1), btn_size);
        let grip_rect_1 = egui::Rect::from_center_size(egui::pos2(splitter_rect_1row.center().x, grip_center_y_1), egui::vec2(4.0, grip_h_1));

        // Upper gap verification: distance between bottom of upper button and top of drag bar is exactly 7px
        let upper_gap_1 = grip_rect_1.top() - btn_upper_rect_1.bottom();
        assert!((upper_gap_1 - 7.0).abs() < 1e-4);

        // Lower gap verification: distance between bottom of drag bar and top of lower button is exactly 7px
        let lower_gap_1 = btn_lower_rect_1.top() - grip_rect_1.bottom();
        assert!((lower_gap_1 - 7.0).abs() < 1e-4);

        // Fits within one-row container with comfortable margins on top and bottom
        assert!(btn_upper_rect_1.top() > splitter_rect_1row.top());
        assert!(btn_lower_rect_1.bottom() < splitter_rect_1row.bottom());
        let margin_top_1 = btn_upper_rect_1.top() - splitter_rect_1row.top();
        let margin_bot_1 = splitter_rect_1row.bottom() - btn_lower_rect_1.bottom();
        assert_eq!(margin_top_1, 14.0);
        assert_eq!(margin_bot_1, 14.0);

        // 2. Multi-row configuration (expanded height)
        let multi_row_h = max_h; // 5 rows = 262.0 px
        let splitter_rect_multi = egui::Rect::from_min_size(egui::pos2(500.0, 100.0), egui::vec2(SPLITTER_WIDTH, multi_row_h));
        let grip_center_y_m = splitter_rect_multi.center().y;

        let t_factor_m = ((splitter_rect_multi.height() - one_row_h) / (max_h - one_row_h)).clamp(0.0, 1.0);
        let grip_h_m = base_grip_len * (1.0 + 0.50 * t_factor_m);
        assert_eq!(t_factor_m, 1.0);
        // Drag bar expands up to 50% of its one-row length (from 24.0 to 36.0 px)
        assert_eq!(grip_h_m, 36.0);

        let btn_upper_center_y_m = grip_center_y_m - grip_h_m / 2.0 - gap - btn_size.y / 2.0;
        let btn_lower_center_y_m = grip_center_y_m + grip_h_m / 2.0 + gap + btn_size.y / 2.0;
        let btn_upper_rect_m = egui::Rect::from_center_size(egui::pos2(splitter_rect_multi.center().x, btn_upper_center_y_m), btn_size);
        let btn_lower_rect_m = egui::Rect::from_center_size(egui::pos2(splitter_rect_multi.center().x, btn_lower_center_y_m), btn_size);
        let grip_rect_m = egui::Rect::from_center_size(egui::pos2(splitter_rect_multi.center().x, grip_center_y_m), egui::vec2(4.0, grip_h_m));

        // Spacing remains preserved at exactly 7px
        let upper_gap_m = grip_rect_m.top() - btn_upper_rect_m.bottom();
        let lower_gap_m = btn_lower_rect_m.top() - grip_rect_m.bottom();
        assert!((upper_gap_m - 7.0).abs() < 1e-4);
        assert!((lower_gap_m - 7.0).abs() < 1e-4);
    }

    /// A right-click on a joint inside a multi-joint selection keeps the selection, and the menu's
    /// Suppress and Delete act on every selected joint, as the feature menu does.
    #[test]
    fn test_joint_menu_acts_on_every_selected_joint() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        crate::test_support::grounded_boxes(&[("A", 0), ("B", 30), ("C", 60)], |id, p| h.app.run(id, p).unwrap());
        let mut joint = |name: &str, kind: &str, a: Value, b: Value| {
            h.app.run("joint.create", json!({"type": kind, "name": name, "a": a, "b": b})).unwrap()["joint"].as_u64().unwrap()
        };
        let j1 = joint("Hinge1", "revolute", json!({"occurrence": "A:1", "face": [10, 10, 10]}), json!({"occurrence": "B:1", "face": [40, 10, 0]}));
        let j2 = joint("Slider1", "slider", json!({"occurrence": "B:1", "face": [40, 10, 10]}), json!({"occurrence": "C:1", "face": [70, 10, 0]}));
        let j3 = joint("Hinge2", "revolute", json!({"occurrence": "C:1", "face": [70, 10, 10]}), json!({"occurrence": "B:1", "face": [40, 10, 0]}));
        h.app.tree.picked_joints = vec![j1, j2];
        h.frames(3);
        let suppressed = |h: &Harness, id: u64| h.app.session.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.suppressed);

        h.step(&json!({"click": {"handle": "assembly_2d:Hinge1"}, "button": "right"})).unwrap();
        h.step(&json!({"widget": "Suppress"})).unwrap();
        h.frames(2);
        assert_eq!([suppressed(&h, j1), suppressed(&h, j2), suppressed(&h, j3)], [Some(true), Some(true), Some(false)]);

        h.step(&json!({"click": {"handle": "assembly_2d:Hinge1"}, "button": "right"})).unwrap();
        h.step(&json!({"widget": "Delete"})).unwrap();
        h.frames(2);
        let left: Vec<u64> = h.app.session.doc.assembly.joints.iter().map(|j| j.id).collect();
        assert_eq!(left, [j3], "Delete removes both selected joints and keeps the other");
    }

    /// A joint chip click picks the joint in the browser, which the Delete key and the joint
    /// panels read, and a joint picked elsewhere shows in the 2D Timeline.
    #[test]
    fn test_joint_chip_click_picks_the_joint_in_the_browser() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        crate::test_support::grounded_boxes(&[("A", 0), ("B", 30), ("C", 60)], |id, p| h.app.run(id, p).unwrap());
        let mut joint = |name: &str, kind: &str, a: Value, b: Value| {
            h.app.run("joint.create", json!({"type": kind, "name": name, "a": a, "b": b})).unwrap()["joint"].as_u64().unwrap()
        };
        let j1 = joint("Hinge1", "revolute", json!({"occurrence": "A:1", "face": [10, 10, 10]}), json!({"occurrence": "B:1", "face": [40, 10, 0]}));
        let j2 = joint("Slider1", "slider", json!({"occurrence": "B:1", "face": [40, 10, 10]}), json!({"occurrence": "C:1", "face": [70, 10, 0]}));
        h.frames(3);
        // Tall enough for every track below the panel header.
        drag_top_edge(&mut h, -100.0);

        h.step(&json!({"click": {"handle": "assembly_2d:Hinge1"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.tree.picked_joints, [j1]);
        h.frames(30);
        h.step(&json!({"click": {"handle": "assembly_2d:Hinge1"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.tree.picked_joints, [j1], "a second plain click keeps the joint picked");
        h.frames(30);
        h.step(&json!({"click": {"handle": "assembly_2d:Slider1"}, "ctrl": true})).unwrap();
        h.frames(2);
        assert_eq!(h.app.tree.picked_joints, [j1, j2]);
        assert_eq!(crate::delete::selection_params(&h.app)["joints"], json!([j1, j2]), "the Delete key acts on both");

        h.app.tree.picked_joints = vec![j2];
        h.frames(2);
        let state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert_eq!(state.selected_joints, [j2].into_iter().collect(), "the 2D Timeline shows the browser's joint pick");

        h.frames(30);
        let feature = h.app.session.doc.features[0].name.clone();
        h.step(&json!({"click": {"handle": format!("timeline_2d:{feature}")}})).unwrap();
        h.frames(2);
        assert!(h.app.tree.picked_joints.is_empty(), "a plain feature click selects the feature alone");

        let (f1, f2) = (h.app.session.doc.features[0].id, h.app.session.doc.features[1].id);
        h.app.run_heavy("timeline.group", json!({"features": [f1, f2], "name": "Pair"})).unwrap();
        h.frames(3);
        let mut state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        let gid = h.app.session.doc.timeline_groups[0].id;
        state.set_group_folded(gid, true);
        state.store_scoped(&h.ctx, Some(h.app.session.uid));
        h.app.tree.picked_joints = vec![j1];
        h.frames(3);
        let header = crate::scenario::handle_at("group_card:Pair").unwrap();
        h.frames(30);
        h.step(&json!({"click": [header.x, header.y + 30.0]})).unwrap();
        h.frames(2);
        let state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert_eq!(state.selected_features, [f1, f2].into_iter().collect(), "a group body click selects its features");
        assert!(h.app.tree.picked_joints.is_empty(), "a group body click drops the joints picked before");
        assert!(state.selected_joints.is_empty());
    }

    #[test]
    fn test_assembly_joint_dependencies_and_highlighting() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));

        // Build 3 grounded boxes A, B, C
        crate::test_support::grounded_boxes(&[("A", 0), ("B", 30), ("C", 60)], |id, p| h.app.run(id, p).unwrap());

        // J1: A to B
        let j1 = h
            .app
            .run(
                "joint.create",
                json!({
                    "type": "revolute",
                    "name": "Hinge1",
                    "a": {"occurrence": "A:1", "face": [10, 10, 10]},
                    "b": {"occurrence": "B:1", "face": [40, 10, 0]},
                }),
            )
            .unwrap();
        let j1_id = j1["joint"].as_u64().unwrap();

        // J2: B to C
        let j2 = h
            .app
            .run(
                "joint.create",
                json!({
                    "type": "slider",
                    "name": "Slider1",
                    "a": {"occurrence": "B:1", "face": [40, 10, 10]},
                    "b": {"occurrence": "C:1", "face": [70, 10, 0]},
                }),
            )
            .unwrap();
        let j2_id = j2["joint"].as_u64().unwrap();

        // Select J2: J1 should be detected as prerequisite because it touches B
        h.app.tree.picked_joints = vec![j2_id];
        h.frames(3);

        // Select J1: J2 should be detected as downstream because it touches B
        h.app.tree.picked_joints = vec![j1_id];
        h.frames(3);

        // Test joint suppression toggle via joint.edit
        let res = h.app.run("joint.edit", json!({"joint": j1_id, "suppressed": true}));
        assert!(res.is_ok());
        h.frames(3);

        let res = h.app.run("joint.edit", json!({"joint": j1_id, "suppressed": false}));
        assert!(res.is_ok());
        h.frames(3);

        // The marker at the end (as End stores it) must stay at the end when a joint is added.
        let mut state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        state.assembly_marker_step = Some(2);
        state.store_scoped(&h.ctx, Some(h.app.session.uid));
        h.frames(2);

        // J3: C to B (primary C, secondary B) — shares only secondary track B with J1 (primary A, secondary B)
        let j3 = h
            .app
            .run(
                "joint.create",
                json!({
                    "type": "revolute",
                    "name": "Hinge2",
                    "a": {"occurrence": "C:1", "face": [70, 10, 10]},
                    "b": {"occurrence": "B:1", "face": [40, 10, 0]},
                }),
            )
            .unwrap();
        let j3_id = j3["joint"].as_u64().unwrap();
        h.frames(2);
        let state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert_eq!(state.assembly_marker_step, None, "a joint added with the marker at the end must not show as rolled back");

        // Select J3: both J1 and J2 should be detected as prerequisites, constraining J3's corridor
        h.app.tree.picked_joints = vec![j3_id];
        let mut state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        state.assembly_marker_step = Some(999); // Verify clamping when rendering
        state.store_scoped(&h.ctx, Some(h.app.session.uid));
        h.frames(3);

        let state = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert_eq!(state.assembly_marker_step, None, "a marker past the last joint is stored as the end");
    }

    #[test]
    fn test_min_height_scrollbar_clearance_and_component_context_actions() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1400, "height": 900}));
        h.frames(2);

        // 1. Verify minimum height leaves scrollbar clearance below the first row
        let one_row_bottom = canvas::HEADER_TOTAL_HEIGHT + canvas::ROW_HEIGHT;
        assert_eq!(canvas::MIN_DOCK_HEIGHT, one_row_bottom + canvas::SCROLLBAR_RESERVE_HEIGHT);
        assert!(canvas::MIN_DOCK_HEIGHT >= one_row_bottom + 8.0);

        // Dragging the top edge all the way down stops at the minimum height
        drag_top_edge(&mut h, 200.0);
        assert_eq!(grid_height(&h), canvas::MIN_DOCK_HEIGHT);

        // 2. Create a component and body to test component rename & show/hide
        let c1 = h.app.run("component.create", json!({"name": "SubPart", "activate": true})).unwrap();
        let comp_id = c1["component"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 15, "width": 15, "height": 10})).unwrap();
        h.frames(2);

        // Test Rename Component via RenameWhat::Component (same path as Browser context menu)
        crate::context_menu::start_rename(&mut h.app, crate::context_menu::RenameWhat::Component(comp_id), egui::pos2(80.0, 750.0));
        crate::context_menu::finish_rename(&mut h.app, Some("RenamedPart"), true).unwrap();
        assert_eq!(h.app.session.doc.components.iter().find(|c| c.id == comp_id).map(|c| c.name.as_str()), Some("RenamedPart"));

        // Test Show/Hide component via browser.visibility on its occurrence path
        let first_path = h.app.session.doc.first_path(comp_id);
        assert!(!first_path.is_empty());
        assert!(!h.app.session.occurrence_hidden(&first_path));

        h.app.run("browser.visibility", json!({"visible": false, "paths": [first_path.clone()]})).unwrap();
        assert!(h.app.session.occurrence_hidden(&first_path));
        h.frames(2);

        h.app.run("browser.visibility", json!({"visible": true, "paths": [first_path.clone()]})).unwrap();
        assert!(!h.app.session.occurrence_hidden(&first_path));
        h.frames(2);
    }

    /// Verifies rollback marker handle dragging, vertical line painting, and horizontal track panning.
    #[test]
    fn test_marker_line_handle_and_track_panning() {
        let mut h = Harness::new();
        h.app.preferences.timeline_2d = true;
        h.call("ui.resize", json!({"width": 1200, "height": 900}));

        // Create enough features so horizontal scrolling is possible in the modeling track (m_max_scroll_x > 0)
        for i in 0..16 {
            h.app
                .run(
                    "solid.box",
                    json!({
                        "name": format!("Box{}", i + 1),
                        "length": 10,
                        "width": 10,
                        "height": 10,
                        "corner": [i * 20, 0, 0],
                    }),
                )
                .unwrap();
        }
        // Position marker at step 8 so it is in the middle of the visible modeling viewport while scroll_x == 0
        h.app.run_heavy("timeline.roll_to", json!({"position": 8})).unwrap();
        h.frames(3);

        assert_eq!(h.app.session.doc.marker, Some(8));
        let st0 = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert_eq!(st0.scroll_x, 0.0);

        // 1. Grab the vertical timeline marker line in the track area and drag it left by 3 columns
        let line_handle = crate::scenario::handle_at("timeline_2d:marker_line").unwrap();
        h.call(
            "ui.drag",
            json!({
                "x0": line_handle.x,
                "y0": line_handle.y,
                "x1": line_handle.x - 3.0 * canvas::COL_WIDTH,
                "y1": line_handle.y,
                "steps": 6,
            }),
        );
        h.frames(2);

        // Marker should have moved to step 5, and track scroll_x must remain 0.0 (no panning)
        assert_eq!(h.app.session.doc.marker, Some(5));
        let st1 = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert_eq!(st1.scroll_x, 0.0, "Dragging the timeline vertical line must not pan the track");

        // 2. Grab a chip and drag it: must not pan the track
        let chip_handle = crate::scenario::handle_at("timeline_2d:Box2").unwrap();
        h.call(
            "ui.drag",
            json!({
                "x0": chip_handle.x,
                "y0": chip_handle.y,
                "x1": chip_handle.x - canvas::COL_WIDTH,
                "y1": chip_handle.y,
                "steps": 5,
            }),
        );
        h.frames(2);
        let st2 = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert_eq!(st2.scroll_x, 0.0, "Dragging a chip must not pan the track");

        // 3. Grab an empty area of the track (create a 2nd component so row 1 is empty at col 5) and drag to pan
        h.app.run_heavy("timeline.roll_to", json!({})).unwrap();
        h.app.run("component.create", json!({"name": "EmptyComp", "activate": true})).unwrap();
        h.frames(2);

        let box5_handle = crate::scenario::handle_at("timeline_2d:Box5").unwrap();
        let empty_pos = egui::pos2(box5_handle.x, box5_handle.y + canvas::ROW_HEIGHT);
        h.call(
            "ui.drag",
            json!({
                "x0": empty_pos.x,
                "y0": empty_pos.y,
                "x1": empty_pos.x - 100.0,
                "y1": empty_pos.y,
                "steps": 6,
            }),
        );
        h.frames(2);
        let st3 = state::Timeline2DState::load_scoped(&h.ctx, Some(h.app.session.uid));
        assert!(st3.scroll_x > 20.0, "Dragging an empty track area should pan the track, got scroll_x={}", st3.scroll_x);
    }

    #[test]
    fn test_state_select_range_edge_cases() {
        let mut state = state::Timeline2DState::default();

        // 1. Empty timeline_order: must not panic and must not insert
        state.select_range(&[], 42, false);
        assert!(state.selected_features.is_empty());

        // 2. Target not found in timeline_order: early return without change
        state.selected_features.insert(1);
        state.select_range(&[2, 3, 4], 99, false);
        assert_eq!(state.selected_features.len(), 1);
        assert!(state.selected_features.contains(&1));

        // 3. Anchor not set: falls back to target and initializes anchor
        state.selection_anchor = None;
        state.select_range(&[10, 20, 30, 40], 30, false);
        assert_eq!(state.selected_features.len(), 1);
        assert!(state.selected_features.contains(&30));
        assert_eq!(state.selection_anchor, Some(30));

        // 4. Anchor missing from timeline_order: falls back to target and updates anchor
        state.selection_anchor = Some(999);
        state.select_range(&[10, 20, 30, 40], 20, false);
        assert_eq!(state.selected_features.len(), 1);
        assert!(state.selected_features.contains(&20));
        assert_eq!(state.selection_anchor, Some(20));

        // Subsequent shift-click uses the newly set anchor (20)
        state.select_range(&[10, 20, 30, 40], 40, false);
        assert_eq!(state.selected_features, [20, 30, 40].into_iter().collect());

        // 5. Normal forward range: [20..=40]
        state.selection_anchor = Some(20);
        state.select_range(&[10, 20, 30, 40, 50], 40, false);
        assert_eq!(state.selected_features, [20, 30, 40].into_iter().collect());

        // 6. Reverse range: [40..=10]
        state.selection_anchor = Some(40);
        state.select_range(&[10, 20, 30, 40, 50], 10, false);
        assert_eq!(state.selected_features, [10, 20, 30, 40].into_iter().collect());

        // 7. Add range to existing selection
        state.select_range(&[10, 20, 30, 40, 50], 50, true);
        assert!(state.selected_features.contains(&50));
    }

    #[test]
    fn test_state_scoping_by_document_uid() {
        let ctx = egui::Context::default();
        let doc1_uid = Some(101u64);
        let doc2_uid = Some(202u64);

        let mut st1 = state::Timeline2DState::load_scoped(&ctx, doc1_uid);
        st1.selected_features.insert(42);
        st1.scroll_x = 120.0;
        st1.store_scoped(&ctx, doc1_uid);

        let mut st2 = state::Timeline2DState::load_scoped(&ctx, doc2_uid);
        st2.selected_features.insert(99);
        st2.scroll_x = 350.0;
        st2.store_scoped(&ctx, doc2_uid);

        let loaded1 = state::Timeline2DState::load_scoped(&ctx, doc1_uid);
        assert_eq!(loaded1.selected_features, [42].into_iter().collect());
        assert_eq!(loaded1.scroll_x, 120.0);

        let loaded2 = state::Timeline2DState::load_scoped(&ctx, doc2_uid);
        assert_eq!(loaded2.selected_features, [99].into_iter().collect());
        assert_eq!(loaded2.scroll_x, 350.0);

        let loaded_default = state::Timeline2DState::load_scoped(&ctx, None);
        assert!(loaded_default.selected_features.is_empty());
        assert_eq!(loaded_default.scroll_x, 0.0);
    }
}
