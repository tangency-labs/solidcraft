//! The application bar from plain data, no app: what each click returns.
use super::*;

#[test]
fn app_bar_cursor_policy_and_done_use_the_shared_native_controls() {
    assert_eq!(drag_cursor(true), egui::CursorIcon::Grabbing);
    assert_eq!(drag_cursor(false), if cfg!(target_os = "windows") { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Grab });
    let ctx = crate::test_support::context();
    // Exercise the active primary-label palette and its shared contrast correction,
    // rather than the formerly forced dark-theme ink.
    let t = Tokens { on_accent: Tokens::light().form_ink, ..Tokens::dark() };
    let _theme = solidcraft_theme::scoped_panel_tokens(t);
    let rect = Rect::from_min_size(pos2(20.0, 20.0), vec2(90.0, 32.0));
    let render = || {
        crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
            assert_eq!(tray_done(ui, rect, "Done").rect, rect);
            crate::button_at(ui, "panel_apply_reference", rect.translate(vec2(0.0, 50.0)), "Apply", crate::Button::Primary);
        })
    };
    render();
    render();
    let output = render();
    let mut labels = Vec::new();
    for clipped in &output.shapes {
        crate::test_support::each_shape(&clipped.shape, &mut |shape| {
            if let egui::Shape::Text(text) = shape {
                let colors: Vec<_> = text
                    .galley
                    .job
                    .sections
                    .iter()
                    .filter(|section| !section.byte_range.is_empty())
                    .map(|section| {
                        text.override_text_color.unwrap_or(if section.format.color == egui::Color32::PLACEHOLDER {
                            text.fallback_color
                        } else {
                            section.format.color
                        })
                    })
                    .collect();
                labels.push((text.galley.job.text.clone(), colors));
            }
        });
    }
    let done = labels.iter().find(|(label, _)| label == "Done").map(|(_, colors)| colors);
    let apply = labels.iter().find(|(label, _)| label == "Apply").map(|(_, colors)| colors);
    assert!(done.is_some_and(|colors| !colors.is_empty()), "Done paints a label: {labels:?}");
    assert_eq!(done, apply, "Done uses the panel primary-label palette including contrast correction");
}

#[test]
fn app_bar_minimal_is_exact_and_object_identity_does_not_affect_selection() {
    use crate::app_bar_items::{ItemId, Layout, Preset, Zone};
    let defaults = Layout::default_for(["solid.box", "solid.extrude"]);
    let mut layout = defaults.clone();
    layout.add_space(Zone::End, None, &defaults);
    layout.add_separator(Zone::End, None, &defaults);
    let objects = |layout: &Layout| {
        layout
            .items
            .iter()
            .filter(|p| matches!(ItemId::parse(&p.id), Some(ItemId::Separator(_) | ItemId::Space(_))))
            .map(|p| p.id.clone())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let previous = objects(&layout);
    layout.set_preset(Preset::Minimal, &defaults);
    assert!(objects(&layout).is_disjoint(&previous));
    for (zone, expected) in [
        (Zone::Start, vec!["space", "file", "separator", "save", "undo", "redo", "separator", "space"]),
        (Zone::End, vec!["space", "help", "settings"]),
    ] {
        let rows: Vec<_> =
            layout.items.iter().filter(|p| p.shown && p.zone == zone).map(|p| p.id.split_once(':').map_or(p.id.as_str(), |(kind, _)| kind)).collect();
        assert_eq!(rows, expected);
    }
    assert!(layout.items.iter().filter(|p| p.id.starts_with("quick:")).all(|p| !p.shown));
    assert_eq!(layout.preset(&defaults), Preset::Minimal);
    assert!(!layout.set_shown("file", false, &defaults));
    let mut renamed = layout.clone();
    for (n, p) in renamed.items.iter_mut().enumerate() {
        if let Some((kind @ ("space" | "separator"), _)) = p.id.split_once(':') {
            p.id = format!("{kind}:{}", n + 100);
        }
    }
    assert_eq!(renamed.preset(&defaults), Preset::Minimal);
    renamed.add_separator(Zone::End, None, &defaults);
    assert_eq!(renamed.preset(&defaults), Preset::Custom);
    let mut extra_space = layout.clone();
    extra_space.add_space(Zone::End, None, &defaults);
    assert_eq!(extra_space.preset(&defaults), Preset::Custom);
    let mut reordered = layout.clone();
    assert!(reordered.move_step("file", true, &defaults));
    assert_eq!(reordered.preset(&defaults), Preset::Custom);
    let previous = objects(&layout);
    layout.set_preset(Preset::Minimal, &defaults);
    assert!(objects(&layout).is_disjoint(&previous));
    assert_eq!(layout.preset(&defaults), Preset::Minimal);
}

#[test]
fn app_bar_layout_background_is_quiet_but_assistant_and_focus_keep_full_ink() {
    let t = Tokens::get();
    for hovered in [false, true] {
        let normal = edit_item_background(t, Control::Documents, hovered);
        for control in [Control::Separator, Control::Space] {
            assert_eq!(edit_item_background(t, control, hovered), normal.gamma_multiply(0.4));
        }
    }
    let ctx = crate::test_support::context();
    let rect = Rect::from_min_size(pos2(20.0, 20.0), vec2(ITEM_HEIGHT, ITEM_HEIGHT));
    let render = |control| {
        crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
            let response = ui.interact(rect, egui::Id::unique("assistant_ink"), Sense::hover());
            paint_control(ui, rect, control, &response, 1.0);
        })
    };
    // Kit contexts deliberately have no host icon artwork. The real status mark shares
    // the base SVG's passive painter, so its token colour verifies that disabling the
    // interaction did not multiply either mark or icon opacity.
    let assistant = render(Control::Assistant { state: crate::AssistantEntryState::Ready, open: false });
    assert!(
        assistant.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Circle(c) if c.radius == 4.0 && c.fill == Tokens::get().agent_ok)),
        "the Assistant painter preserves full-strength token ink"
    );
    editing(&ctx, true, None);
    let line = render(Control::Separator);
    assert!(
        line.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == Tokens::get().app_bar_text)),
        "the separator line itself keeps full foreground ink"
    );
}

#[test]
fn app_bar_focus_ring_uses_the_background_geometry_and_theme_accent() {
    let ctx = crate::test_support::context();
    let square = Rect::from_min_size(pos2(10.0, 10.0), vec2(ITEM_HEIGHT, ITEM_HEIGHT));
    for (rect, radius) in [
        (square, ITEM_RADIUS),
        (Rect::from_min_size(square.min, vec2(80.0, ITEM_HEIGHT)), ITEM_RADIUS),
        (Rect::from_min_size(pos2(10.0, 10.0), vec2(90.0, PILL_HEIGHT)), ITEM_PILL_RADIUS),
        (Rect::from_min_size(pos2(10.0, 10.0), vec2(74.0, 46.0)), 9),
    ] {
        assert_eq!(focus_ring_geometry(rect, radius), (rect, radius));
        let out = crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| paint_focus(ui.painter(), rect, radius));
        assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == rect && r.corner_radius == egui::CornerRadius::same(radius) && r.stroke == Stroke::new(1.5, Tokens::get().accent) && r.stroke_kind == egui::StrokeKind::Middle)));
    }
}

#[test]
fn app_bar_spaces_repeat_sanitize_persist_and_have_eleven_point_slots() {
    use crate::app_bar_items::{ItemId, Layout, Placement, Preset, Zone as SavedZone};
    use crate::app_bar_layout::application::{SPACE_WIDTH, spec_with_items};
    let defaults = Layout::default();
    let mut saved = defaults.clone();
    let a = saved.add_space(SavedZone::Start, Some("file"), &defaults).unwrap();
    let b = saved.add_space(SavedZone::End, None, &defaults).unwrap();
    assert_ne!(a, b);
    assert_eq!(ItemId::parse(&a).unwrap().id(), a);
    assert_eq!(saved.preset(&defaults), Preset::Custom);
    let spec = spec_with_items((1920.0, 0.0, 0.0), measured_bar(), &saved, &["save", "undo", "redo"], &[], &["donate", "discord"]);
    let p = layout(&spec);
    for id in [&a, &b] {
        let item = index_of(&spec, id);
        assert_eq!(p.placements[item].unwrap().width, SPACE_WIDTH);
        assert_eq!(spec.items[item].priority, 0);
    }
    assert!(saved.set_shown(&a, false, &defaults));
    assert!(!saved.items.iter().any(|p| p.id == a));
    assert_eq!(Layout::from_value(&saved.to_value(), &defaults), saved);
    for preset in [Preset::Default, Preset::Minimal, Preset::Full, Preset::Compact] {
        let mut reset = saved.clone();
        reset.set_preset(preset, &defaults);
        assert_eq!(reset.items.iter().filter(|p| p.id.starts_with("space:")).count(), if preset == Preset::Minimal { 3 } else { 0 });
    }
    saved.items.extend((0..1000).map(|_| Placement { id: b.clone(), zone: SavedZone::End, shown: true }));
    saved.items.push(Placement { id: "space:NaN".into(), zone: SavedZone::End, shown: true });
    saved.sanitize(&defaults);
    assert!(saved.items.len() <= 64);
    assert_eq!(saved.items.iter().map(|p| &p.id).collect::<std::collections::BTreeSet<_>>().len(), saved.items.len());
    assert!(saved.items.iter().filter(|p| p.id.starts_with("space:")).all(|p| matches!(ItemId::parse(&p.id), Some(ItemId::Space(_)))));
}

#[test]
fn app_bar_old_canonical_quick_layout_loses_only_its_automatic_third_divider() {
    use crate::app_bar_items::{Layout, Placement, Zone};
    let defaults = Layout::default_for(["solid.box"]);
    let mut old = defaults.clone();
    let at = old.items.iter().position(|p| p.id.starts_with("quick:")).unwrap();
    old.items.insert(at, Placement { id: "separator:3".into(), zone: Zone::Start, shown: true });
    let mut value = old.to_value();
    value["version"] = 1.into();
    assert_eq!(Layout::from_value(&value, &defaults), defaults);
    old.items.iter_mut().find(|p| p.id == "home").unwrap().shown = false;
    let mut value = old.to_value();
    value["version"] = 1.into();
    assert_eq!(Layout::from_value(&value, &defaults), old, "custom layout objects are preserved");
}

/// The kit's own Dark and Light. The look's crate checks its Onyx theme the same way
/// (`app_bar_edit_palette_reads_on_onyx`), since the theme files are the look's.
#[test]
fn app_bar_edit_palette_has_three_distinct_levels_and_readable_foreground() {
    use solidcraft_theme::contrast_ratio;
    for (name, t) in [("dark", Tokens::dark()), ("light", Tokens::light())] {
        let p = edit_palette(t);
        let a = contrast_ratio(p.card, p.item);
        let b = contrast_ratio(p.item, p.hover);
        let foreground = contrast_ratio(t.app_bar_text, p.hover);
        assert!(a >= 1.25 && b >= 1.25, "{name}: separate levels {a}/{b}");
        assert!(foreground >= 4.5, "{name}: hovered text remains legible {foreground}");
        eprintln!("app-bar edit contrast {name}: card/item {a:.2}:1, item/hover {b:.2}:1, foreground/hover {foreground:.2}:1");
    }
}

#[test]
fn app_bar_default_dividers_migrate_once_and_removal_survives_loading() {
    use crate::app_bar_items::{Layout, Placement, Preset, Zone};
    let defaults = Layout::default();
    let ids: Vec<_> = defaults.items.iter().filter(|p| p.zone == Zone::Start).map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["documents", "home", "separator:1", "file", "save", "undo", "redo", "separator:2"]);
    let mut old = defaults.to_value();
    old.as_object_mut().unwrap().remove("version");
    old["items"].as_array_mut().unwrap().retain(|p| !p["id"].as_str().unwrap().starts_with("separator:"));
    assert_eq!(Layout::from_value(&old, &defaults), defaults);
    old["items"].as_array_mut().unwrap().push(serde_json::json!({"id":"separator:1","zone":"end","shown":true}));
    let loaded = Layout::from_value(&old, &defaults);
    assert!(
        loaded.items.iter().any(|p| p.id == "separator:0" && p.zone == Zone::End),
        "old tray-created separator survives with a non-colliding identity"
    );
    assert!(loaded.items.iter().any(|p| p.id == "separator:1" && p.zone == Zone::Start));
    let mut edited = defaults.clone();
    edited.items.push(Placement { id: "future:item".into(), zone: Zone::Start, shown: true });
    assert!(edited.move_before("separator:1", Zone::End, Some("settings"), &defaults));
    assert!(edited.set_shown("separator:2", false, &defaults));
    edited.sanitize(&defaults);
    assert!(!edited.items.iter().any(|p| p.id == "separator:2"));
    assert_eq!(Layout::from_value(&edited.to_value(), &defaults), edited);
    assert_eq!(edited.preset(&defaults), Preset::Custom);
    edited.set_preset(Preset::Default, &defaults);
    assert!(edited.items.iter().any(|p| p.id == "separator:2"));
    for preset in [Preset::Minimal, Preset::Compact] {
        edited.set_preset(preset, &defaults);
        assert_eq!(edited.items.iter().filter(|p| p.id.starts_with("separator:")).count(), if preset == Preset::Minimal { 2 } else { 0 });
        assert_eq!(Layout::from_value(&edited.to_value(), &defaults), edited);
        assert_eq!(edited.preset(&defaults), preset);
    }
}

#[test]
fn app_bar_shared_control_shapes_have_square_or_pill_height_and_a_common_centre() {
    let ctx = crate::test_support::context();
    let _ = crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |_| {});
    let t = Tokens::get();
    let pill = Pill {
        id: "donate",
        icon: "help",
        label: "Donate",
        labelled: true,
        tip: "",
        hide_tip: "",
        name: "Donate",
        hide_name: "Hide",
        fill: t.accent,
        ink: t.on_accent,
    };
    for control in [
        Control::Icon { icon: "save", size: 18.0, enabled: true },
        Control::Command { icon: "save", enabled: true },
        Control::File { label: "File", enabled: true, open: false },
        Control::Version("v1.0"),
        Control::Documents,
        Control::Separator,
        Control::Space,
        Control::Assistant { state: crate::AssistantEntryState::Quiet, open: false },
        Control::Report { label: "Report a bug", labelled: true },
        Control::Report { label: "Report a bug", labelled: false },
        Control::Pill { data: pill, lift: 0.0, veil: 0.0, focused: false },
    ] {
        let pill = matches!(control, Control::Report { .. } | Control::Pill { .. });
        let expected = if pill { 20.0 } else { 26.0 };
        assert_eq!(control_size(&ctx, control).y, expected);
        if icon_control(control) {
            assert_eq!(control_size(&ctx, control).x, expected);
        }
        let painted = item_rect(Rect::from_center_size(pos2(100.0, 17.0), vec2(80.0, 12.0)), control);
        assert_eq!(painted.height(), expected);
        if icon_control(control) {
            assert_eq!(painted.width(), expected);
        }
        assert_eq!(painted.center(), pos2(100.0, 17.0));
        assert_eq!(control_radius(control), if pill { 10 } else { 5 });
    }
}

#[test]
fn app_bar_hover_lift_snaps_converges_and_handles_hostile_elapsed_time() {
    for (from, to) in [(0.0_f32, -2.0_f32), (-2.0, 0.0)] {
        let mut previous = from;
        for tick in 0..=12 {
            let value = hover_step(from, to, tick as f32 / 100.0);
            assert!(value >= from.min(to) && value <= from.max(to));
            assert!((value - to).abs() <= (previous - to).abs());
            previous = value;
        }
        assert_eq!(previous, to);
    }
    assert_eq!(hover_step(-1.6, -2.0, 0.0), -2.0);
    assert_eq!(hover_step(0.0, -2.0, f32::NAN), -2.0);
    assert_eq!(hover_step(0.0, -2.0, f32::INFINITY), -2.0);
    assert_eq!(hover_step(f32::NAN, 0.0, 0.1), 0.0);
}

#[test]
fn app_bar_hover_paint_keeps_hits_at_rest_and_disables_lift_during_drag() {
    let ctx = crate::test_support::context();
    editing(&ctx, true, None);
    let rest = Rect::from_min_size(pos2(40.0, 10.0), vec2(ITEM_HEIGHT, ITEM_HEIGHT));
    let render = |time, hover, held| {
        dragging(&ctx, held);
        let output = crate::test_support::raw_frame(
            &ctx,
            egui::RawInput { time: Some(time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))), ..Default::default() },
            |ui| {
                let hit = ui.interact(rest, egui::Id::unique("hover_hit"), Sense::hover());
                paint_hover(ui, "save", rest, ITEM_RADIUS, hover, |ui| {
                    ui.painter().rect_filled(rest, ITEM_RADIUS, Tokens::get().control_fill);
                });
                assert_eq!(hit.rect, rest);
            },
        );
        let body = output
            .shapes
            .iter()
            .find(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == rest && r.fill == Tokens::get().control_fill))
            .unwrap();
        (body.transform_after_tessellation * rest.min, output.shapes.len())
    };
    let idle = render(0.0, false, false);
    assert_eq!(idle.0, rest.min);
    render(0.01, true, false);
    let hovered = render(0.15, true, false);
    assert_eq!(hovered.0, rest.min - vec2(0.0, 2.0));
    assert!(hovered.1 > idle.1, "only hovering adds a faint shadow");
    let held = render(0.16, true, true);
    assert_eq!(held.0, rest.min);
    assert_eq!(held.1, idle.1);
}

#[test]
fn app_bar_shared_native_paint_preserves_interaction_visibility_and_clicks() {
    let ctx = crate::test_support::context();
    let rest = Rect::from_min_size(pos2(40.0, 10.0), vec2(32.0, 26.0));
    let id = egui::Id::unique("native_bar_control");
    let mut clicked = false;
    for pressed in [None, Some(true), Some(false)] {
        let mut events = vec![egui::Event::PointerMoved(rest.center())];
        if let Some(pressed) = pressed {
            events.push(egui::Event::PointerButton {
                pos: rest.center(),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        let output = crate::test_support::raw_frame(
            &ctx,
            egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))), focused: true, ..Default::default() },
            |ui| {
                let response = interaction_only(ui, |ui| {
                    assert!(ui.is_visible());
                    ui.painter().rect_filled(rest, 4, Tokens::get().warning);
                    ui.interact(rest, id, Sense::click())
                });
                clicked |= response.clicked();
                assert!(ctx.read_response(id).is_some());
                paint_control(ui, rest, Control::Documents, &response, 1.0);
            },
        );
        assert!(
            !output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == Tokens::get().warning)),
            "the old paint is discarded, not hidden input"
        );
    }
    assert!(clicked);
}

#[test]
fn app_bar_documents_migrate_move_and_hide() {
    use crate::app_bar_items::{Layout, Zone};
    let defaults = Layout::default();
    let mut legacy = defaults.clone();
    legacy.items.retain(|p| p.id != "documents" && p.id != "file");
    assert_eq!(Layout::from_value(&legacy.to_value(), &defaults), defaults);
    let mut saved = defaults.clone();
    assert!(saved.move_before("documents", Zone::End, Some("settings"), &defaults));
    assert!(saved.set_shown("documents", false, &defaults));
    assert_eq!(Layout::from_value(&saved.to_value(), &defaults), saved);
    assert!(saved.set_shown("documents", true, &defaults));
    assert_eq!(saved.items.iter().find(|p| p.id == "documents").unwrap().zone, Zone::End);
}

#[test]
fn app_bar_separators_repeat_renumber_remove_and_bound_hostile_input() {
    use crate::app_bar_items::{DragRelease, ItemId, Layout, MAX_ITEMS, Placement, Preset, Zone, drag_release};
    let defaults = Layout::default();
    let mut saved = defaults.clone();
    let first = saved.add_separator(Zone::Start, Some("save"), &defaults).unwrap();
    let second = saved.add_separator(Zone::End, None, &defaults).unwrap();
    assert_ne!(first, second);
    assert!(saved.move_before(&first, Zone::End, Some(&second), &defaults));
    assert_eq!(drag_release(&first, true, true, false, false), DragRelease::Remove);
    assert_eq!(drag_release("separator", false, false, true, false), DragRelease::Insert);
    assert_eq!(drag_release("separator", false, true, false, false), DragRelease::Return);
    assert!(saved.set_shown(&first, false, &defaults));
    assert!(!saved.items.iter().any(|p| p.id == first));
    saved.items.extend((0..1000).map(|_| Placement { id: second.clone(), zone: Zone::Start, shown: true }));
    saved.items.push(Placement { id: "separator:hostile".into(), zone: Zone::End, shown: true });
    saved.sanitize(&defaults);
    assert_eq!(saved.items.len(), MAX_ITEMS);
    let ids: std::collections::BTreeSet<_> = saved.items.iter().map(|p| &p.id).collect();
    assert_eq!(ids.len(), saved.items.len());
    assert!(saved.items.iter().filter(|p| p.id.starts_with("separator:")).all(|p| matches!(ItemId::parse(&p.id), Some(ItemId::Separator(_)))));
    assert!(saved.add_separator(Zone::End, None, &defaults).is_none());
    assert_eq!(Layout::from_value(&saved.to_value(), &defaults), saved);
    let mut malformed = Layout { items: vec![Placement { id: "separator:NaN".into(), zone: Zone::Start, shown: true }] };
    malformed.sanitize(&defaults);
    assert!(malformed.items.iter().any(|p| p.id == "separator:0"));
    for preset in [Preset::Default, Preset::Minimal, Preset::Full, Preset::Compact] {
        let mut reset = saved.clone();
        reset.set_preset(preset, &defaults);
        assert_eq!(reset.items.iter().filter(|p| p.id.starts_with("separator:")).count(), if preset == Preset::Compact { 0 } else { 2 });
    }
}

#[test]
fn app_bar_full_separator_inventory_keeps_file_and_overflows_separators_first() {
    use crate::app_bar_items::{Layout, Zone};
    use crate::app_bar_layout::application::spec_with_items;
    let defaults = Layout::default();
    let mut saved = defaults.clone();
    while saved.add_separator(Zone::Start, Some("file"), &defaults).is_some() {}
    saved.move_before("file", Zone::End, None, &defaults);
    for width in [480.0, 1280.0, 3840.0] {
        let spec = spec_with_items((width, 0.0, 138.0), measured_bar(), &saved, &["save", "undo", "redo"], &[], &["donate", "discord"]);
        let placed = layout(&spec);
        assert_sound(&spec, &placed);
        assert!(placed.placements[index_of(&spec, "file")].is_some());
        if placed.overflow.iter().any(|id| !id.starts_with("separator:")) {
            assert!(spec.items.iter().zip(&placed.placements).filter(|(item, _)| item.id.starts_with("separator:")).all(|(_, p)| p.is_none()));
        }
    }
}

#[test]
fn app_bar_preset_selection_is_derived_with_first_match_winning() {
    use crate::app_bar_items::{Layout, Placement, Preset, Zone};
    // A hidden default also proves Full independently of the all-shown default's alias.
    for hidden_default in [false, true] {
        let mut defaults = Layout::default();
        defaults.items.iter_mut().find(|p| p.id == "version").unwrap().shown = !hidden_default;
        for value in [Preset::Default, Preset::Minimal, Preset::Full, Preset::Compact] {
            let mut current = defaults.clone();
            current.set_preset(value, &defaults);
            let expected = if value == Preset::Full && !hidden_default { Preset::Default } else { value };
            assert_eq!(current.preset(&defaults), expected);
            current.items.push(Placement { id: "future:item".into(), zone: Zone::End, shown: true });
            assert_eq!(current.preset(&defaults), expected);
            let before = current.clone();
            current.set_preset(Preset::Custom, &defaults);
            assert_eq!(current, before);
            assert!(!current.set_shown("file", false, &defaults));
            assert_eq!(current.preset(&defaults), expected);
            current.move_before("file", Zone::End, None, &defaults);
            assert_eq!(current.preset(&defaults), Preset::Custom);
            current.set_preset(value, &defaults);
            current.add_separator(Zone::Start, None, &defaults);
            assert_eq!(current.preset(&defaults), Preset::Custom);
        }
    }
}

#[test]
fn app_bar_edit_mode_changes_only_separator_widths() {
    let rig = BarRig::new(1280.0);
    let snapshot =
        |out: &BarOut| (out.edit_items.iter().map(|p| (p.id.clone(), p.zone, p.left, p.right, p.locked)).collect::<Vec<_>>(), out.overflow.clone());
    rig.frame(Vec::new());
    let before = snapshot(rig.out.borrow().as_ref().unwrap());
    editing(&rig.ctx, true, None);
    rig.frame(Vec::new());
    let after = snapshot(rig.out.borrow().as_ref().unwrap());
    assert_eq!(before.1, after.1, "everything still fits in this wide case");
    for normal in &before.0 {
        if normal.4 {
            continue;
        } // Flexible document tabs use the space left after widened separators.
        let edited = after.0.iter().find(|p| p.0 == normal.0).unwrap();
        let delta = if normal.0.starts_with("separator:") { 10.0 } else { 0.0 };
        assert_eq!(edited.3 - edited.2, normal.3 - normal.2 + delta);
    }
    assert!(after.0.iter().any(|p| p.0 == "report"));
    assert!(after.0.iter().any(|p| p.0 == "version"));
}

#[test]
fn app_bar_separator_mode_changes_glide_and_resize_still_snaps() {
    // The bar's helpers and these checks never lock the context inside its own lock: a worker's
    // repaint between the two reads froze the window (#21091).
    let ((), nested) = egui::Context::record_nested_locks(separator_mode_changes_glide_and_resize_still_snaps);
    assert!(nested.is_empty(), "{} nested context locks, the first:\n{}", nested.len(), nested.first().map_or("", String::as_str));
}

fn separator_mode_changes_glide_and_resize_still_snaps() {
    let mut rig = BarRig::new(1920.0);
    let mut style = rig.ctx.global_style().as_ref().clone();
    style.animation_time = 0.15;
    rig.ctx.set_global_style(style);
    let snapshot = |rig: &BarRig| {
        let out = rig.out.borrow();
        let item = out.as_ref().unwrap().edit_items.iter().find(|p| p.id == "save").unwrap();
        let final_rect =
            Rect::from_min_size(pos2(item.left, (height(editing_active(&rig.ctx)) - ITEM_HEIGHT) / 2.0), vec2(item.right - item.left, ITEM_HEIGHT));
        // Outside a frame cumulative_frame_nr already points to the next frame, so
        // inspect the completed paint state rather than calling the in-frame helper.
        let key = motion_key(&rig.ctx);
        let painted = rig.ctx.data(|data| data.get_temp::<BarMotion>(key).unwrap().items.get("save").unwrap().drawn);
        (final_rect, Rect::from_min_size(painted, final_rect.size()))
    };
    let separator_gap = |rig: &BarRig| {
        let out = rig.out.borrow();
        let items = &out.as_ref().unwrap().edit_items;
        let painted = |id: &str| {
            let item = items.iter().find(|p| p.id == id).unwrap();
            let rect = Rect::from_min_size(
                pos2(item.left, (height(editing_active(&rig.ctx)) - ITEM_HEIGHT) / 2.0),
                vec2(item.right - item.left, ITEM_HEIGHT),
            );
            let key = motion_key(&rig.ctx);
            rig.ctx.data(|data| {
                let state = data.get_temp::<BarMotion>(key).unwrap();
                let item = state.items.get(id).unwrap();
                Rect::from_min_size(item.drawn, vec2(if id.starts_with("separator:") { item.drawn_width } else { rect.width() }, rect.height()))
            })
        };
        assert!(painted("separator:1").right() <= painted("file").left(), "a growing or shrinking separator cannot overlap its gliding neighbour");
    };
    rig.frame(Vec::new());
    for active in [true, false] {
        let previous = snapshot(&rig).1;
        editing(&rig.ctx, active, None);
        rig.frame(Vec::new());
        let (target, initial) = snapshot(&rig);
        separator_gap(&rig);
        assert_ne!(target.min, previous.min);
        assert_eq!(initial.min, previous.min, "mode changes retain the previous painted position");
        for _ in 0..4 {
            rig.frame(Vec::new());
            separator_gap(&rig);
        }
        let middle = snapshot(&rig).1;
        assert!(middle.left() > target.left().min(previous.left()) && middle.left() < target.left().max(previous.left()));
        for _ in 0..16 {
            rig.frame(Vec::new());
            separator_gap(&rig);
        }
        assert_eq!(snapshot(&rig).1, target);
    }
    editing(&rig.ctx, true, None);
    rig.width += 100.0;
    rig.frame(Vec::new());
    let (target, drawn) = snapshot(&rig);
    assert_eq!(target, drawn, "a resize cancels the simultaneous edit-mode displacement");
}

#[test]
fn app_bar_animated_step_converges_without_overshoot_and_handles_invalid_timing() {
    for (from, to) in [(0.0, 100.0), (100.0, -100.0)] {
        let mut previous = from;
        for tick in 0..=15 {
            let value = animated_step(from, to, tick as f32 / 100.0, 0.15);
            assert!(value >= from.min(to) && value <= from.max(to));
            assert!((value - to).abs() <= (previous - to).abs());
            previous = value;
        }
        assert_eq!(previous, to);
    }
    assert_eq!(animated_step(99.6, 100.0, 0.0, 0.15), 100.0);
    assert_eq!(animated_step(0.0, 100.0, -1.0, 0.15), 0.0);
    for (dt, duration) in [(f32::NAN, 0.15), (0.01, 0.0), (0.01, f32::NAN), (f32::INFINITY, 0.15)] {
        assert_eq!(animated_step(0.0, 100.0, dt, duration), 100.0);
    }
    assert_eq!(animated_step(f32::NAN, 10.0, 0.01, 0.15), 10.0);
    assert!(animated_step(10.0, f32::NAN, 0.01, 0.15).is_finite());
    assert!(animated_step(f32::MAX, -f32::MAX, 0.05, 0.15).is_finite());
}

#[test]
fn app_bar_displacement_changes_paint_only_and_skips_resizing_and_appearing_items() {
    let ctx = crate::test_support::context();
    let mut style = ctx.global_style().as_ref().clone();
    style.animation_time = 0.15;
    ctx.set_global_style(style);
    let render = |time, x, width, present| {
        let final_rect = Rect::from_min_size(pos2(x, 5.0), vec2(32.0, 24.0));
        let mut drawn = final_rect;
        let output = crate::test_support::raw_frame(
            &ctx,
            egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 400.0))), time: Some(time), ..Default::default() },
            |ui| {
                begin_motion(&ctx, width);
                if present {
                    let hit = paint_motion(ui, "save", final_rect, |ui| {
                        let response = ui.interact(final_rect, egui::Id::unique("motion_hit"), Sense::click());
                        ui.painter().rect_filled(final_rect, 7, Tokens::get().accent);
                        response.rect
                    });
                    assert_eq!(hit, final_rect, "hit testing retains the layout rectangle");
                    drawn = drawn_rect(&ctx, "save", final_rect);
                }
            },
        );
        (drawn, output)
    };
    assert_eq!(render(0.0, 10.0, 800.0, true).0.left(), 10.0);
    assert_eq!(render(0.02, 110.0, 800.0, true).0.left(), 10.0);
    let (middle, output) = render(0.07, 110.0, 800.0, true);
    assert!(middle.left() > 10.0 && middle.left() < 110.0);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect.left() == 110.0 && ((shape.transform_after_tessellation * rect.rect.min).x - middle.left()).abs() < 0.01)), "the offset is paint-only and applied after pixel snapping");
    assert_eq!(render(0.25, 110.0, 800.0, true).0.left(), 110.0);
    assert_eq!(render(0.26, 210.0, 900.0, true).0.left(), 210.0, "resize snaps");
    let _ = render(0.27, 210.0, 900.0, false);
    assert_eq!(render(0.28, 310.0, 900.0, true).0.left(), 310.0, "appearing items snap");
    let previous = render(0.30, 410.0, 900.0, true).0;
    assert_eq!(render(0.35, 510.0, 900.0, true).0.min, previous.min, "retargeting starts where the last frame was painted");
    assert_eq!(render(0.55, 510.0, 900.0, true).0.left(), 510.0);
    let key = motion_key(&ctx);
    assert!(!ctx.data(|data| data.get_temp::<BarMotion>(key).unwrap().items.get("save").unwrap().active));
}

#[test]
fn app_bar_tray_badges_use_the_shared_tick() {
    let ctx = crate::test_support::context();
    let cell = Rect::from_min_size(pos2(100.0, 100.0), vec2(74.0, 46.0));
    let output = crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
        let badge = tray_badge(ui, cell);
        assert_eq!(badge.right(), cell.right() + 6.0);
        assert_eq!(badge.top(), cell.top() - 6.0);
    });
    assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Path(p) if p.points.len() == 3 && matches!(p.stroke.color, egui::epaint::ColorMode::Solid(ink) if ink == tray_accent().1))), "the badge paints a tick, not an unresolved icon placeholder");
}

#[test]
fn app_bar_file_migrates_moves_but_never_disappears() {
    use crate::app_bar_items::{Layout, Preset, Zone, can_remove};
    let defaults = Layout::default();
    assert_eq!(
        defaults.items.iter().take(7).map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["documents", "home", "separator:1", "file", "save", "undo", "redo"]
    );
    let mut legacy = defaults.clone();
    legacy.items.retain(|p| p.id != "file");
    assert_eq!(Layout::from_value(&legacy.to_value(), &defaults), defaults);
    let mut saved = defaults.clone();
    assert!(saved.move_before("file", Zone::End, Some("settings"), &defaults));
    assert!(!can_remove("file"));
    assert!(!saved.set_shown("file", false, &defaults));
    let file = saved.items.iter_mut().find(|p| p.id == "file").unwrap();
    file.shown = false;
    let loaded = Layout::from_value(&saved.to_value(), &defaults);
    let file = loaded.items.iter().find(|p| p.id == "file").unwrap();
    assert!(file.shown);
    assert_eq!(file.zone, Zone::End);
    for preset in [Preset::Default, Preset::Minimal, Preset::Full, Preset::Compact] {
        saved.set_preset(preset, &defaults);
        assert!(saved.items.iter().find(|p| p.id == "file").unwrap().shown);
    }
}

#[test]
fn app_bar_drag_out_and_release_decisions_include_file_spring_back() {
    use crate::app_bar_items::{DragRelease, drag_out, drag_release, is_drag};
    assert!(!drag_out(34.0, 34.0));
    assert!(drag_out(34.1, 34.0));
    assert!(!drag_out(f32::NAN, 34.0));
    assert!(!drag_out(100.0, f32::INFINITY));
    assert!(!is_drag(3.0, 3.0));
    assert!(is_drag(5.1, 0.0));
    assert_eq!(drag_release("save", true, true, false, false), DragRelease::Remove);
    assert_eq!(drag_release("file", true, true, false, false), DragRelease::Return);
    assert_eq!(drag_release("save", false, true, false, false), DragRelease::Return);
    assert_eq!(drag_release("file", true, false, true, false), DragRelease::Insert);
    assert_eq!(drag_release("save", true, false, true, true), DragRelease::Return);
}

#[test]
fn app_bar_edit_moves_before_after_and_between_zones() {
    use crate::app_bar_items::{Layout, Zone};
    let defaults = Layout::default_for(["solid.box"]);
    let mut layout = defaults.clone();
    assert!(layout.move_before("redo", Zone::Start, Some("save"), &defaults));
    assert_eq!(
        layout.items.iter().filter(|p| p.zone == Zone::Start).map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["documents", "home", "separator:1", "file", "redo", "save", "undo", "quick:solid.box", "separator:2"]
    );
    assert!(layout.move_after("redo", "undo", &defaults));
    assert!(layout.move_before("save", Zone::End, Some("settings"), &defaults));
    assert_eq!(layout.items.iter().find(|p| p.id == "save").unwrap().zone, Zone::End);
    assert!(layout.move_before("quick:solid.box", Zone::End, None, &defaults));
    assert_eq!(layout.items.iter().rfind(|p| p.zone == Zone::End).unwrap().id, "quick:solid.box");
    assert!(layout.move_step("settings", true, &defaults));
    assert!(layout.move_step("settings", false, &defaults));
    assert!(layout.set_shown("undo", false, &defaults));
    assert!(!layout.set_shown("undo", false, &defaults));
    assert!(layout.set_shown("undo", true, &defaults));
    let saved = layout.to_value();
    assert_eq!(Layout::from_value(&saved, &defaults), layout);
}

#[test]
fn app_bar_edit_presets_and_hostile_operations_keep_sanitize_guarantees() {
    use crate::app_bar_items::{Layout, MAX_ITEMS, Placement, Preset, Zone};
    let defaults = Layout::default_for(["solid.box"]);
    let mut layout = defaults.clone();
    layout.items.extend((0..5000).map(|i| Placement { id: format!("future:{i}"), zone: Zone::End, shown: false }));
    layout.items.insert(0, Placement { id: "file".into(), zone: Zone::End, shown: false });
    layout.items.push(Placement { id: "save".into(), zone: Zone::End, shown: false });
    for id in ["logo", "tabs", "more", "update", "managed", "leading", "captions", "unknown", "", "\n", &"x".repeat(10000)] {
        assert!(!layout.move_before(id, Zone::End, None, &defaults));
        assert!(!layout.set_shown(id, false, &defaults));
        assert!(!layout.move_after("save", id, &defaults));
    }
    assert!(layout.items.len() <= MAX_ITEMS);
    assert_eq!(layout.items.iter().filter(|p| p.id == "save").count(), 1);
    let clean = layout.clone();
    assert!(!layout.move_before("save", Zone::Start, Some("settings"), &defaults), "wrong-zone target is stale");
    assert!(!layout.move_before("save", Zone::Start, Some("file"), &defaults), "File was saved in End");
    assert_eq!(layout, clean);
    for preset in [Preset::Minimal, Preset::Full, Preset::Compact, Preset::Default] {
        layout.set_preset(preset, &defaults);
        let shown: Vec<_> = layout.items.iter().filter(|p| p.shown).map(|p| p.id.as_str()).collect();
        match preset {
            Preset::Minimal => assert_eq!(
                shown.iter().map(|id| id.split_once(':').map_or(*id, |(kind, _)| kind)).collect::<Vec<_>>(),
                ["space", "file", "separator", "save", "undo", "redo", "separator", "space", "space", "help", "settings"]
            ),
            Preset::Full | Preset::Default | Preset::Custom => assert!(defaults.items.iter().all(|p| shown.contains(&p.id.as_str()))),
            Preset::Compact => assert!(["donate", "discord", "report", "version"].iter().all(|id| !shown.contains(id))),
        }
        assert!(layout.items.iter().any(|p| p.id == "future:0" && !p.shown));
        let mut sanitized = layout.clone();
        sanitized.sanitize(&defaults);
        assert_eq!(layout, sanitized);
    }
}

#[test]
fn app_bar_drop_slots_cross_zones_reject_locked_parts_and_ignore_tiny_drags() {
    use crate::app_bar_items::{DropItem, Zone, drop_slot, is_drag};
    let row = |id: &str, zone, left, right, locked| DropItem { id: id.into(), zone, left, right, locked };
    let items = vec![
        row("logo", Zone::Start, 0.0, 30.0, true),
        row("save", Zone::Start, 35.0, 65.0, false),
        row("undo", Zone::Start, 70.0, 100.0, false),
        row("tabs", Zone::Start, 110.0, 200.0, true),
        row("settings", Zone::End, 210.0, 240.0, false),
        row("help", Zone::End, 245.0, 275.0, false),
        row("more", Zone::End, 280.0, 310.0, true),
    ];
    assert!(!is_drag(2.0, 2.0));
    assert!(is_drag(3.0, 4.0));
    assert!(!is_drag(f32::NAN, f32::INFINITY));
    let before = drop_slot(72.0, 155.0, &items, "save").unwrap();
    assert_eq!((before.zone, before.before.as_deref(), before.x), (Zone::Start, Some("undo"), 70.0));
    let across = drop_slot(211.0, 155.0, &items, "save").unwrap();
    assert_eq!((across.zone, across.before.as_deref(), across.x), (Zone::End, Some("settings"), 210.0));
    let end = drop_slot(276.0, 155.0, &items, "save").unwrap();
    assert_eq!((end.zone, end.before, end.x), (Zone::End, None, 275.0));
    assert_eq!(drop_slot(31.0, 155.0, &items, "file").unwrap().before.as_deref(), Some("save"));
    assert_eq!(drop_slot(105.0, 155.0, &items, "file").unwrap().before, None);
    for x in [10.0, 155.0, 295.0, f32::NAN, f32::INFINITY] {
        assert!(drop_slot(x, 155.0, &items, "save").is_none());
    }
    assert!(drop_slot(15.0, f32::NAN, &items, "save").is_none());
    let empty = drop_slot(250.0, 155.0, &[], "save").unwrap();
    assert_eq!((empty.zone, empty.before, empty.x), (Zone::End, None, 250.0));
}

const HANDLES: Handles<'static> = Handles {
    home: &["t:home"],
    home_rect: &["t:home"],
    file: &["t:file"],
    file_rect: &["t:file"],
    button: &["t:"],
    separator: "t:separator:",
    search: "t:search",
    report: "t:report",
    pill: "t:",
    settings: "t:settings",
    version: "t:version",
};

fn button(id: &'static str, enabled: bool, wide_only: bool) -> BarButton<'static> {
    BarButton { id, icon: "save", tip: id, enabled, narrow: true, wide_only }
}

/// A bar `width` wide: Save, a disabled Undo (left out of a compact bar), Help, Settings.
struct BarRig {
    ctx: egui::Context,
    width: f32,
    name: &'static str,
    time: std::cell::Cell<f64>,
    out: std::cell::RefCell<Option<BarOut>>,
}

impl BarRig {
    fn new(width: f32) -> BarRig {
        BarRig::named(width, "App")
    }

    /// The bar with the app's name `name`.
    fn named(width: f32, name: &'static str) -> BarRig {
        let rig = BarRig { ctx: crate::test_support::context(), width, name, time: std::cell::Cell::new(0.0), out: Default::default() };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&self, events: Vec<egui::Event>) -> Vec<BarAction> {
        self.frame_output(events).0
    }

    /// A frame: what was chosen, and what was drawn.
    fn frame_output(&self, events: Vec<egui::Event>) -> (Vec<BarAction>, egui::FullOutput) {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 400.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let bar = AppBar {
            items: None,
            quick_ids: &[],
            mark: &|_, _| {},
            name: self.name,
            leading: None,
            home: BarButton { id: "home", icon: "home", tip: "Home", enabled: true, narrow: false, wide_only: false },
            file: "File",
            file_open: false,
            actions: vec![button("save", true, false), button("undo", false, true)],
            quick: 2,
            help: BarButton { id: "help", icon: "help", tip: "Help", enabled: true, narrow: false, wide_only: false },
            settings: BarButton { id: "settings", icon: "settings", tip: "Settings", enabled: true, narrow: false, wide_only: false },
            report: Report { label: "Report", tip: "Report a problem", icon_only: false },
            pills: vec![],
            search: false,
            search_shortcut: Some("S"),
            assistant: true,
            version: Some(Version { text: "v1.0", url: "https://example.invalid/notes" }),
            handles: HANDLES,
        };
        let mut actions = Vec::new();
        let output = crate::test_support::raw_frame(&self.ctx, input, |ui| {
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(self.width, height(editing_active(&self.ctx))));
            let out = show(ui, &bar, rect, rect);
            actions = out.actions.clone();
            actions.extend(overflow_menu(ui.ctx(), &out.overflow_items, out.overflow_button));
            *self.out.borrow_mut() = Some(out);
        });
        (actions, output)
    }

    fn click(&self, name: &str) -> Vec<BarAction> {
        let at = solidcraft_ui_registry::handle_at(name).or_else(|| solidcraft_ui_registry::rect_of(name).map(|r| r.center()));
        let at = at.unwrap_or_else(|| panic!("{name} is drawn"));
        self.time.set(self.time.get() + 1.0);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = self.frame(vec![egui::Event::PointerMoved(at)]);
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), press(true)]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), press(false)]));
        actions
    }
}

/// Each control returns its own action, a disabled one nothing; the places left to the app come
/// back in the output.
#[test]
fn the_bar_returns_what_was_clicked() {
    let rig = BarRig::new(1200.0);
    assert_eq!(rig.click("t:home"), vec![BarAction::Home]);
    let file = solidcraft_ui_registry::rect_of("t:file").expect("File is drawn");
    assert_eq!(rig.click("t:file"), vec![BarAction::File { rect: file }]);
    assert_eq!(rig.click("t:save"), vec![BarAction::Button { id: "save".into() }]);
    assert_eq!(rig.click("t:undo"), Vec::new(), "a disabled button runs nothing");
    assert_eq!(rig.click("t:settings"), vec![BarAction::Button { id: "settings".into() }]);
    assert!(matches!(rig.click("t:help").as_slice(), [BarAction::Help { .. }]));
    assert_eq!(rig.click("t:report"), vec![BarAction::Report]);
    assert_eq!(rig.click("t:version"), vec![BarAction::Version]);
    let out = rig.out.borrow();
    let out = out.as_ref().expect("drawn");
    let (first, step) = out.quick.expect("room for the quick-access buttons");
    assert!(first > file.right() && step > 0.0);
    assert!(out.tabs_start > first + step, "the tabs follow the quick-access buttons");
    assert!(out.assistant.is_some_and(|a| a.left() == out.controls_left), "the assistant starts the End stack without a special leading gap");
}

/// At 600 points the name and lower priority End controls yield before Undo and quick access.
#[test]
fn a_compact_bar_keeps_only_what_fits() {
    let rig = BarRig::new(480.0);
    assert!(solidcraft_ui_registry::rect_of("t:save").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:undo").is_some(), "Undo fits after lower priorities give way");
    assert!(rig.out.borrow().as_ref().is_some_and(|out| out.quick.is_some() && out.quick_count == 2));
    assert!(rig.out.borrow().as_ref().is_some_and(|out| out.overflow.contains(&"version".to_string())));
    assert!(solidcraft_ui_registry::rect_of("appbar:overflow").is_some());
    assert!(solidcraft_ui_registry::handle_at("t:version").is_none(), "no version on a narrow bar");
}

/// A long app name ends before Home: Home follows the name as drawn, not a place sized for one
/// name.
#[test]
fn home_follows_a_long_app_name() {
    let name = "A Considerably Longer Application";
    let rig = BarRig::named(1200.0, name);
    let (_, output) = rig.frame_output(Vec::new());
    let text = crate::test_support::text_shapes(&output)
        .find(|shape| shape.galley.text() == name)
        .map(|shape| shape.visual_bounding_rect())
        .expect("the name is drawn");
    let home = solidcraft_ui_registry::rect_of("t:home").expect("Home is drawn");
    assert!(text.right() < home.left(), "the name ends at {}, Home starts at {}", text.right(), home.left());
}

/// Geometry re-captured because spacing was simplified to uniform four-point stacks.
/// Bits select search, icon-only
/// report, version, two pills, assistant, five quick buttons, long name and Windows captions.
/// Five widths exercise fitting, compression and overflow; variants include none, each bit,
/// all bits and desktop defaults (28). Pairs cover search with/without pills, report/version,
/// quick/captions and long-name/quick; crowded mixes (188, 254, 255) cover space pressure.
#[test]
fn app_bar_old_show_golden() {
    let recapture = std::env::var_os("SOLIDCRAFT_RECAPTURE_APP_BAR").as_deref() == Some(std::ffi::OsStr::new("1"));
    let mut rows = Vec::new();
    let mut case = 0;

    for width in [480.0, 640.0, 1000.0, 1280.0, 1920.0] {
        for bits in [0, 1, 2, 4, 8, 16, 32, 64, 128, 255, 28, 3, 5, 6, 9, 17, 24, 33, 65, 96, 129, 160, 188, 254] {
            let ctx = crate::test_support::context();
            let has = |bit| bits & (1 << bit) != 0;
            let bar = AppBar {
                items: None,
                quick_ids: &[],
                mark: &|_, _| {},
                name: if has(6) { "A Considerably Longer Application" } else { "App" },
                leading: None,
                home: button("home", true, false),
                file: "File",
                file_open: false,
                actions: vec![button("save", true, false), button("undo", false, true), button("redo", true, true)],
                quick: if has(5) { 5 } else { 0 },
                help: button("help", true, false),
                settings: button("settings", true, false),
                report: Report { label: "Report a bug", tip: "Report a problem", icon_only: has(1) },
                pills: if has(3) {
                    ["donate", "discord"]
                        .into_iter()
                        .map(|id| Pill {
                            id,
                            icon: "help",
                            label: id,
                            labelled: true,
                            tip: id,
                            hide_tip: "Hide",
                            name: id,
                            hide_name: "Hide",
                            fill: egui::Color32::WHITE,
                            ink: egui::Color32::BLACK,
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                search: has(0),
                search_shortcut: Some("Ctrl+K"),
                assistant: has(4),
                version: has(2).then_some(Version { text: "v1.0", url: "https://example.invalid/notes" }),
                handles: HANDLES,
            };
            solidcraft_ui_registry::clear_handles();
            let mut snapshot = Vec::new();
            let number = |v: f32| (v * 1000.0).round() as i32;
            let rect = |r: Option<Rect>| r.map_or_else(Vec::new, |r| vec![number(r.min.x), number(r.min.y), number(r.max.x), number(r.max.y)]);
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 400.0))), ..Default::default() };
            crate::test_support::raw_frame(&ctx, input, |ui| {
                let r = Rect::from_min_size(Pos2::ZERO, vec2(width, HEIGHT));
                let content = Rect::from_min_max(r.min, r.max - vec2(if has(7) { 138.0 } else { 0.0 }, 0.0));
                let out = show(ui, &bar, r, content);
                for name in [
                    "home",
                    "file",
                    "save",
                    "undo",
                    "redo",
                    "help",
                    "separator:brand",
                    "separator:quick_access",
                    "separator:documents",
                    "search",
                    "report",
                    "donate",
                    "donate:hide",
                    "discord",
                    "discord:hide",
                    "settings",
                    "version",
                ] {
                    let name = format!("t:{name}");
                    snapshot.push(rect(solidcraft_ui_registry::rect_of(&name)));
                    snapshot.push(solidcraft_ui_registry::handle_at(&name).map_or_else(Vec::new, |p| vec![number(p.x), number(p.y)]));
                }
                snapshot.push(out.quick.map_or_else(Vec::new, |(x, step)| vec![number(x), number(step)]));
                snapshot.push(rect(Some(out.offer)));
                snapshot.push(vec![i32::from(out.offer_labeled)]);
                snapshot.push(vec![number(out.tabs_start), number(out.controls_left)]);
                snapshot.push(rect(out.search));
                snapshot.push(rect(out.assistant));
                snapshot.push(rect(out.leading));
                snapshot.push(rect(Some(out.file)));
                assert!(out.actions.is_empty(), "idle bar returned actions at {width}, variant {bits}: {:?}", out.actions);
            });
            if recapture {
                let values: Vec<_> = snapshot.iter().flat_map(|field| std::iter::once(field.len() as i32).chain(field.iter().copied())).collect();
                rows.push(format!("    &[{}], // {width} / {bits}", values.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")));
                case += 1;
                continue;
            }
            let mut expected = APP_BAR_ROWS[case];
            for (field, actual) in snapshot.iter().enumerate() {
                let (&len, rest) = expected.split_first().expect("recorded field length");
                assert_eq!(actual.len(), len as usize, "width {width}, variant {bits}, field {field}: presence");
                let (values, rest) = rest.split_at(actual.len());
                for (&a, &e) in actual.iter().zip(values) {
                    assert!((a - e).abs() <= 1, "width {width}, variant {bits}, field {field}: {actual:?} != {values:?}");
                }
                expected = rest;
            }
            assert!(expected.is_empty(), "width {width}, variant {bits}: unchecked fields");
            case += 1;
        }
    }
    assert_eq!(case, 120);
    if recapture {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app_bar_tests.rs");
        let source = std::fs::read_to_string(&path).unwrap();
        let at = source.rfind("const APP_BAR_ROWS: &[&[i32]] = &[").unwrap();
        let end = at + source.get(at..).unwrap().find("\n];").unwrap() + "\n];".len();
        let table = format!("const APP_BAR_ROWS: &[&[i32]] = &[\n{}\n];", rows.join("\n"));
        std::fs::write(path, format!("{}{}{}", &source[..at], table, &source[end..])).unwrap();
        eprintln!("re-captured {} application-bar golden rows from the uniform application spec", rows.len());
    }
}
const TAB_HANDLES: TabHandles<'static> = TabHandles {
    tab: "t:tab:",
    close: "t:close:",
    more: "t:more",
    new: "t:new",
    rename: "t:rename",
    menu_row: "t:menu:",
    more_row: "t:list:",
    view: "t:view:",
    view_close: "t:vclose:",
    view_new: "t:vnew",
    view_more: "t:vmore",
    switcher: "t:switcher",
};

const VIEW_MENU: [TabMenuRow<'static>; 4] = [
    TabMenuRow::new("move_to_window", "Move to window", "").for_local_view().with_children(&[TabMenuRow::new("move_new", "New window", "")]),
    TabMenuRow::new("vclose", "Close view", "Ctrl+W"),
    TabMenuRow::new("vothers", "Close other views", "").needing_others().separated(),
    TabMenuRow::new("vright", "Close views to the right", "").needing_others(),
];
const PLUS_MENU: [TabMenuRow<'static>; 4] = [
    TabMenuRow::heading("Open as a view"),
    TabMenuRow::new("plus_render", "Render", "").with_icon("render", Some(egui::Color32::GREEN)),
    TabMenuRow::new("plus_dup", "Duplicate this view", "").with_icon("duplicate", None).separated(),
    TabMenuRow::new("plus_reopen", "Reopen closed view", "Ctrl+Shift+T").with_icon("reopen", None),
];
const SWITCHER_MENU: [TabMenuRow<'static>; 1] = [TabMenuRow::new("as_tabs", "Show views as tabs", "")];

const LAYOUT_MENU: [TabMenuRow<'static>; 8] = [
    TabMenuRow::new("layout.single", "Single view", ""),
    TabMenuRow::new("layout.side_by_side", "Two side by side", ""),
    TabMenuRow::new("layout.stacked", "Two stacked", ""),
    TabMenuRow::new("layout.large_and_two", "One large and two small", ""),
    TabMenuRow::new("layout.grid", "Two by two", ""),
    TabMenuRow::new("fill.angle", "A new angle of this view", ""),
    TabMenuRow::new("fill.standard", "Standard angles", ""),
    TabMenuRow::new("fill.ask", "A choice in each empty cell", ""),
];

const VIEWS: [ViewTab<'static>; 3] = [
    ViewTab {
        compass: None,
        sketch_mode: false,
        name: "Model",
        icon: "model",
        accent: egui::Color32::BLUE,
        underline: egui::Color32::BLUE,
        state: ViewState::Normal,
        active: true,
        on_screen: false,
        window_name: None,
        window_number: None,
        to_window: None,
        tip: "Model",
    },
    ViewTab {
        compass: None,
        sketch_mode: false,
        name: "Render 2",
        icon: "render",
        accent: egui::Color32::GREEN,
        underline: egui::Color32::GREEN,
        state: ViewState::SourceChanged,
        active: false,
        on_screen: false,
        window_name: None,
        window_number: None,
        to_window: None,
        tip: "Render 2",
    },
    ViewTab {
        compass: None,
        sketch_mode: false,
        name: "Sheet 1",
        icon: "sheet",
        accent: egui::Color32::RED,
        underline: egui::Color32::RED,
        state: ViewState::Deleted,
        active: false,
        on_screen: false,
        window_name: None,
        window_number: None,
        to_window: None,
        tip: "Sheet 1",
    },
];

const TAB_MENU: [TabMenuRow<'static>; 2] = [TabMenuRow::new("close", "Close", ""), TabMenuRow::new("others", "Close Others", "").needing_others()];

/// Three document tabs, the first selected, in a strip 600 wide.
struct TabsRig {
    ctx: egui::Context,
    time: std::cell::Cell<f64>,
    screen_height: f32,
    /// The first design has the three [`VIEWS`], shown as sub-tabs (`Some(true)`) or behind a
    /// switcher (`Some(false)`); the strip then has more room.
    views: Option<bool>,
    room: Option<f32>,
    single_view: bool,
    name: &'static str,
    detached: bool,
    clear: bool,
    layout: Option<(usize, usize)>,
    pop_out: bool,
    /// The + has a menu.
    plus: bool,
    plus_rows: &'static [TabMenuRow<'static>],
    /// The name of this view is being typed: its place, and the text and focus the strip edits.
    rename: Option<usize>,
    text: std::cell::RefCell<String>,
    focused: std::cell::Cell<bool>,
}

impl TabsRig {
    fn new() -> TabsRig {
        TabsRig::with(None)
    }

    fn with(views: Option<bool>) -> TabsRig {
        TabsRig::build(views, false, None)
    }

    /// A rig with `views` whose + opens a menu when `plus`, and view `rename` (with `text`) being renamed.
    fn build(views: Option<bool>, plus: bool, rename: Option<(usize, &str)>) -> TabsRig {
        let text = rename.map_or(String::new(), |(_, text)| text.to_string());
        let rig = TabsRig {
            ctx: crate::test_support::context(),
            time: std::cell::Cell::new(0.0),
            screen_height: 400.0,
            views,
            room: None,
            single_view: false,
            name: "One",
            detached: false,
            clear: false,
            layout: None,
            pop_out: false,
            plus,
            plus_rows: &PLUS_MENU,
            rename: rename.map(|(k, _)| k),
            text: std::cell::RefCell::new(text),
            focused: std::cell::Cell::new(false),
        };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&self, events: Vec<egui::Event>) -> Vec<TabAction> {
        self.frame_output(events).0
    }

    fn frame_output(&self, events: Vec<egui::Event>) -> (Vec<TabAction>, egui::FullOutput) {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, self.screen_height))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let tab =
            |name: &'static str| DocTab { name, list_name: name, dirty: false, status: None, place: "Not saved yet", security: None, views: &[] };
        let mut views = VIEWS;
        if self.detached {
            views[1].to_window = Some(1);
            views[1].window_name = Some("Window 2");
            views[1].window_number = Some(2);
        }
        let views = if self.single_view || self.pop_out {
            &views[..1]
        } else if self.views.is_some() {
            &views[..]
        } else {
            &[]
        };
        let first = DocTab { views, ..tab(self.name) };
        let layout = self.layout.map(|(visible, window_count)| layout_test_data(visible, window_count));
        let (mut text, mut focused) = (self.text.borrow().clone(), self.focused.get());
        let mut strip = TabStrip {
            tabs: if self.pop_out { vec![first] } else { vec![first, tab("Two"), tab("Three")] },
            selected: 0,
            shown_selected: Some(0),
            pop_out: self.pop_out,
            title_bar: false,
            double_click_renames: true,
            rename: None,
            menu: &TAB_MENU,
            close_name: "Close tab",
            close_tip: "Close",
            more_tip: "All",
            new_tip: "New",
            id: egui::Id::unique("test_tabs"),
            handles: TAB_HANDLES,
            views_as_tabs: self.views == Some(true),
            switcher_always: false,
            clear_view_tabs: self.clear,
            view_text: ViewText {
                close_view: "Close view",
                close_design: "Close design",
                new_view: "Open another view",
                more: "{n} more",
                switch_view: "Switch view",
            },
            view_menu: &VIEW_MENU,
            view_right_row: "vright",
            view_rename_row: "vrename",
            view_rename: self.rename.map(|k| (k, TabRename { text: &mut text, focused: &mut focused, max: 8 })),
            plus_menu: if self.plus { self.plus_rows } else { &[] },
            layout_menu: if layout.is_some() { &LAYOUT_MENU } else { &[] },
            view_layout: layout.as_ref(),
            layout_current: "layout.single",
            layout_tip: "Layout",
            layout_fill: "fill.angle",
            layout_fill_heading: "",
            switcher_menu: &SWITCHER_MENU,
            tear_off_below: 40.0,
        };
        let (x0, x1) = if self.views.is_some() { (20.0, 780.0) } else { (100.0, 700.0) };
        let x1 = self.room.map_or(x1, |room| x0 + room);
        let mut actions = Vec::new();
        let output = crate::test_support::raw_frame(&self.ctx, input, |ui| {
            actions = tabs(ui, &mut strip, Rect::from_min_size(Pos2::ZERO, vec2(800.0, HEIGHT)), x0, x1);
        });
        *self.text.borrow_mut() = text;
        self.focused.set(focused);
        (actions, output)
    }

    fn press(&self, at: Pos2, button: egui::PointerButton) -> Vec<TabAction> {
        self.time.set(self.time.get() + 1.0);
        let event = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = self.frame(vec![egui::Event::PointerMoved(at)]);
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), event(true)]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), event(false)]));
        actions
    }

    fn at(&self, name: &str) -> Pos2 {
        solidcraft_ui_registry::rect_of(name)
            .map(|r| r.center())
            .or_else(|| solidcraft_ui_registry::handle_at(name))
            .unwrap_or_else(|| panic!("{name} is drawn"))
    }
}

#[test]
fn app_bar_document_and_view_tabs_use_the_shared_hover_and_held_cursors() {
    for (views, target) in [(None, "t:tab:1"), (Some(true), "t:view:1")] {
        let rig = TabsRig::with(views);
        let at = rig.at(target);
        for _ in 0..2 {
            let (_, output) = rig.frame_output(vec![egui::Event::PointerMoved(at)]);
            assert_eq!(output.platform_output.cursor_icon, drag_cursor(false));
        }
        let event = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let (_, held) = rig.frame_output(vec![event(true)]);
        assert_eq!(held.platform_output.cursor_icon, drag_cursor(true));
        let (_, dragged) = rig.frame_output(vec![egui::Event::PointerMoved(at + vec2(20.0, 0.0))]);
        assert_eq!(dragged.platform_output.cursor_icon, drag_cursor(true));
        rig.frame(vec![event(false)]);
    }
}

/// A click selects a tab, its × closes it, + asks for a new one, a row of a tab's menu returns its
/// id, and a tab dragged past another is moved there.
#[test]
fn the_tab_strip_returns_what_was_clicked_and_dragged() {
    let rig = TabsRig::new();
    assert_eq!(rig.press(rig.at("t:tab:1"), egui::PointerButton::Primary), vec![TabAction::Select(1)]);
    assert_eq!(rig.press(rig.at("t:close:0"), egui::PointerButton::Primary), vec![TabAction::Close(0)]);
    assert_eq!(rig.press(rig.at("t:new"), egui::PointerButton::Primary), vec![TabAction::New]);
    rig.press(rig.at("t:tab:2"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:others"), egui::PointerButton::Primary), vec![TabAction::Menu { tab: 2, id: "others".into() }]);
    // Drag the first tab past the last.
    let from = rig.at("t:tab:0");
    let to = solidcraft_ui_registry::rect_of("t:tab:2").map(|r| r.right_center() - vec2(4.0, 0.0)).expect("tab 2 is drawn");
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    let mut actions = rig.frame(vec![egui::Event::PointerMoved(from)]);
    actions.extend(rig.frame(vec![button(from, true)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(from + vec2(10.0, 0.0))]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(to)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(to)]));
    actions.extend(rig.frame(vec![button(to, false)]));
    assert_eq!(actions, vec![TabAction::Reorder { from: 0, to: 2 }]);
}

fn file_item(id: &str, enabled: bool) -> FileItem {
    FileItem { id: id.to_string(), label: format!("Item {id}"), key: String::new(), icon: "open".to_string(), enabled }
}

/// The File menu drawn for a frame with `events`: what it returned, and where each text was.
fn file_menu_frame(ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> (FileMenuOut, Vec<(String, Pos2)>) {
    let rows = vec![FileRow::Item(file_item("new", true)), FileRow::Item(file_item("save", false)), FileRow::Separator];
    file_menu_frame_of(ctx, &rows, 600.0, time, events)
}

/// The File menu with `rows` in an 800 by `height` window, drawn for a frame with `events`.
fn file_menu_frame_of(ctx: &egui::Context, rows: &[FileRow], height: f32, time: f64, events: Vec<egui::Event>) -> (FileMenuOut, Vec<(String, Pos2)>) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, height))),
        time: Some(time),
        events,
        focused: true,
        ..Default::default()
    };
    let mut out = FileMenuOut::default();
    let output = crate::test_support::raw_frame(ctx, input, |ui| {
        out = file_menu(ui.ctx(), rows, pos2(100.0, HEIGHT), egui::Id::unique("test_file_menu"), false);
    });
    let texts = crate::test_support::text_shapes(&output).map(|s| (s.galley.text().to_string(), s.visual_bounding_rect().center())).collect();
    (out, texts)
}

/// A File menu taller than the window stays inside it, and its last row scrolls into view.
#[test]
fn a_file_menu_taller_than_the_window_scrolls_its_rows() {
    let ctx = crate::test_support::context();
    let rows: Vec<FileRow> = (0..40).map(|i| FileRow::Item(file_item(&format!("r{i}"), true))).collect();
    let height = 360.0;
    file_menu_frame_of(&ctx, &rows, height, 0.0, Vec::new());
    file_menu_frame_of(&ctx, &rows, height, 0.1, Vec::new());
    let menu = ctx.memory(|m| m.area_rect(egui::Id::unique("test_file_menu"))).expect("the menu is laid out");
    assert!(menu.bottom() <= height, "the menu ends at {} inside a {height} high window", menu.bottom());
    let shown = |texts: &[(String, Pos2)], text: &str| texts.iter().any(|(t, p)| t == text && menu.contains(*p));
    let over = menu.center();
    let mut texts = Vec::new();
    for i in 0..20 {
        let wheel = egui::Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        };
        (_, texts) = file_menu_frame_of(&ctx, &rows, height, 0.2 + f64::from(i) * 0.1, vec![egui::Event::PointerMoved(over), wheel]);
    }
    assert!(shown(&texts, "Item r39"), "scrolling shows the last row");
    assert!(!shown(&texts, "Item r0"), "and the first has scrolled out");
}

/// A row returns its id and closes the menu; a disabled one returns nothing and keeps it open;
/// a click elsewhere closes it.
#[test]
fn the_file_menu_returns_the_row_chosen() {
    let ctx = crate::test_support::context();
    let mut time = 0.0;
    let mut click = |at: Pos2| {
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let mut last = FileMenuOut::default();
        for events in [vec![egui::Event::PointerMoved(at)], vec![egui::Event::PointerMoved(at), press(true)], vec![press(false)]] {
            time += 0.5;
            let (out, _) = file_menu_frame(&ctx, time, events);
            if out.chosen.is_some() || out.close {
                last = out;
            }
        }
        last
    };
    // An area is laid out on its first frame and painted from the next.
    file_menu_frame(&ctx, 0.0, Vec::new());
    let (_, texts) = file_menu_frame(&ctx, 0.1, Vec::new());
    let at = |text: &str| texts.iter().find(|(t, _)| t == text).map(|(_, p)| *p).unwrap_or_else(|| panic!("{text} is painted"));
    let (new, save) = (at("Item new"), at("Item save"));
    assert_eq!(click(save), FileMenuOut::default(), "a disabled row chooses nothing and the menu stays");
    assert_eq!(click(new), FileMenuOut { chosen: Some("new".into()), close: true });
    assert_eq!(click(pos2(700.0, 500.0)), FileMenuOut { chosen: None, close: true }, "a click elsewhere closes it");
}

/// The switcher returns its click and publishes where it is.
#[test]
fn the_workspace_switcher_returns_its_click() {
    let ctx = crate::test_support::context();
    let ws = Rect::from_min_size(Pos2::ZERO, vec2(88.0, 117.0));
    let data = Switcher {
        name: "Design",
        caption: "Workspace",
        icon: "ws_design",
        tip: "Change",
        open: false,
        floating: false,
        labels: true,
        folded: false,
        stage: false,
        accent: Tokens::get().accent,
        handle_rect: "t:ws",
        handle_target: "t:ws_target",
    };
    let mut clicked = Vec::new();
    for (i, events) in
        [vec![egui::Event::PointerMoved(ws.center())], vec![press(ws.center(), true)], vec![press(ws.center(), false)]].into_iter().enumerate()
    {
        let input = egui::RawInput { time: Some(i as f64 * 0.1), events, focused: true, ..Default::default() };
        crate::test_support::raw_frame(&ctx, input, |ui| clicked.push(switcher(ui, &data, ws)));
    }
    assert_eq!(clicked, [false, false, true]);
    assert_eq!(solidcraft_ui_registry::rect_of("t:ws"), Some(ws));
}

fn press(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

/// A design with several views is a pill where the app asks for sub-tabs and the bar has room for
/// the active view and the "N more" button; otherwise a switcher. One view or none is a plain tab,
/// unless the app asks for the switcher always: then one view has it too, but never with sub-tabs.
#[test]
fn the_selected_tab_falls_back_to_a_switcher_where_the_pill_has_no_room() {
    assert_eq!(selected_layout(0, true, false, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(1, true, false, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(1, false, false, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(3, true, false, 300.0, 300.0), SelectedLayout::Pill);
    assert_eq!(selected_layout(3, true, false, 300.0, 299.0), SelectedLayout::Switcher);
    assert_eq!(selected_layout(3, false, false, 300.0, 900.0), SelectedLayout::Switcher);
    assert_eq!(selected_layout(1, false, true, 300.0, 900.0), SelectedLayout::Switcher);
    assert_eq!(selected_layout(1, true, true, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(0, false, true, 300.0, 900.0), SelectedLayout::Plain);
    assert_eq!(selected_layout(3, true, true, 300.0, 300.0), SelectedLayout::Pill);
}

/// The sub-tabs all show when they fit; otherwise a run of neighbours that holds the shown view
/// stays and the "N more" button takes its room.
#[test]
fn a_pill_that_is_too_narrow_keeps_the_active_view_in_sight() {
    let widths = [60.0; 6];
    let fits = 6.0 * (60.0 + VIEW_GAP);
    assert_eq!(view_window(&widths, 3, fits, 50.0), (0, 6));
    // Room for three sub-tabs and the button: the run starts at the first that lets the active one in.
    let room = 3.0 * (60.0 + VIEW_GAP) + 50.0;
    assert_eq!(view_window(&widths, 1, room, 50.0), (0, 3));
    assert_eq!(view_window(&widths, 4, room, 50.0), (2, 3));
    assert_eq!(view_window(&widths, 5, room, 50.0), (3, 3));
    // Less room than one sub-tab: the active view still shows.
    assert_eq!(view_window(&widths, 4, 10.0, 50.0), (4, 1));
    assert_eq!(view_window(&[], 0, -5.0, 50.0), (0, 0));
}

/// A design tab is torn off only when released well below the bar, and never where the app turns it off.
#[test]
fn a_tab_is_torn_off_only_well_below_the_bar() {
    assert!(!is_tear_off(40.0, 34.0, 40.0));
    assert!(!is_tear_off(74.0, 34.0, 40.0));
    assert!(is_tear_off(75.0, 34.0, 40.0));
    assert!(!is_tear_off(500.0, 34.0, 0.0));
}

/// A sub-tab dropped between others lands at the slot nearest its left edge, and the marker sits
/// where the others leave room for it.
#[test]
fn a_view_dragged_along_its_design_lands_in_the_nearest_slot() {
    let widths = [50.0, 70.0, 60.0];
    // Dragging the first sub-tab: the others are 70 and 60 wide.
    assert_eq!(drop_slot(0.0, 10.0, &widths, 0, 0), 0);
    assert_eq!(drop_slot(85.0, 10.0, &widths, 0, 0), 1);
    assert_eq!(drop_slot(400.0, 10.0, &widths, 0, 0), 2);
    assert_eq!(slot_left(10.0, &widths, 0, 0, 0), 10.0);
    assert_eq!(slot_left(10.0, &widths, 0, 0, 1), 80.0);
    assert_eq!(slot_left(10.0, &widths, 0, 0, 2), 140.0);
    // A window that starts at the second sub-tab counts places from there.
    assert_eq!(slot_left(10.0, &widths[1..], 1, 2, 1), 10.0);
    assert_eq!(slot_left(10.0, &widths[1..], 1, 2, 2), 80.0);
}

/// The sub-tabs of the selected design report a click, the close button, the + (no menu: a new
/// view), a right-click menu row and a drag to a new place.
#[test]
fn the_sub_tabs_return_what_was_clicked_and_dragged() {
    let rig = TabsRig::with(Some(true));
    assert_eq!(rig.press(rig.at("t:view:1"), egui::PointerButton::Primary), vec![TabAction::SelectView(1)]);
    assert_eq!(rig.press(rig.at("t:vclose:0"), egui::PointerButton::Primary), vec![TabAction::CloseView(0)]);
    assert_eq!(rig.press(rig.at("t:vnew"), egui::PointerButton::Primary), vec![TabAction::NewView]);
    assert_eq!(rig.press(rig.at("t:close:0"), egui::PointerButton::Primary), vec![TabAction::Close(0)]);
    rig.press(rig.at("t:view:2"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:vclose"), egui::PointerButton::Primary), vec![TabAction::ViewMenu { view: 2, id: "vclose".into() }]);
    // Drag the first sub-tab past the last.
    let origin = solidcraft_ui_registry::rect_of("t:view:0").expect("view 0 is drawn");
    let from = origin.center();
    let to = solidcraft_ui_registry::rect_of("t:view:2").map(|r| r.right_center() - vec2(4.0, 0.0)).expect("view 2 is drawn");
    let mut actions = rig.frame(vec![egui::Event::PointerMoved(from)]);
    actions.extend(rig.frame(vec![press(from, true)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(from + vec2(3.0, 0.0))]));
    assert!(actions.is_empty(), "three pixels stay a click");
    let pointer = from + vec2(4.0, 0.0);
    assert_eq!(rig.frame(vec![egui::Event::PointerMoved(pointer)]), vec![TabAction::DockView { view: 0, origin, pointer, released: false }]);
    assert_eq!(rig.frame(vec![egui::Event::PointerMoved(to)]), vec![TabAction::DockView { view: 0, origin, pointer: to, released: false }]);
    assert_eq!(
        rig.frame(vec![press(to, false)]),
        vec![TabAction::DockView { view: 0, origin, pointer: to, released: true }, TabAction::ReorderView { from: 0, to: 2 }]
    );
}

#[test]
fn a_view_drag_reports_release_outside_the_client_and_escape_cancels_it() {
    let rig = TabsRig::with(Some(true));
    let origin = solidcraft_ui_registry::rect_of("t:view:0").expect("view 0 is drawn");
    let from = origin.center();
    let outside = pos2(850.0, from.y);
    let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    for cancel in [false, true] {
        rig.frame(vec![egui::Event::PointerMoved(from)]);
        rig.frame(vec![press(from, true)]);
        rig.frame(vec![egui::Event::PointerMoved(from + vec2(10.0, 0.0))]);
        assert!(rig.frame(vec![egui::Event::PointerMoved(outside)]).contains(&TabAction::DockView {
            view: 0,
            origin,
            pointer: outside,
            released: false
        }));
        if cancel {
            assert!(rig.frame(vec![escape.clone()]).is_empty());
            assert!(rig.frame(vec![egui::Event::PointerMoved(outside + vec2(4.0, 0.0))]).is_empty(), "a cancelled press stays cancelled");
        }
        let actions = rig.frame(vec![press(outside, false)]);
        assert_eq!(actions.contains(&TabAction::DockView { view: 0, origin, pointer: outside, released: true }), !cancel);
    }
}

#[test]
fn touch_lifts_a_view_tab_only_after_the_hold_threshold() {
    let rig = TabsRig::with(Some(true));
    let origin = solidcraft_ui_registry::rect_of("t:view:0").expect("view 0 is drawn");
    let pointer = origin.center();
    let touch = |phase| egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(1), phase, pos: pointer, force: None };
    rig.frame(vec![egui::Event::PointerMoved(pointer)]);
    assert!(rig.frame(vec![touch(egui::TouchPhase::Start), press(pointer, true)]).is_empty());
    rig.time.set(rig.time.get() + 0.200);
    assert!(rig.frame(Vec::new()).is_empty());
    rig.time.set(rig.time.get() + 0.025);
    assert_eq!(rig.frame(Vec::new()), vec![TabAction::DockView { view: 0, origin, pointer, released: false }]);
    assert_eq!(
        rig.frame(vec![touch(egui::TouchPhase::End), press(pointer, false)]),
        vec![TabAction::DockView { view: 0, origin, pointer, released: true }]
    );
}

#[test]
fn space_lifts_a_focused_local_view_tab_without_selecting_it() {
    let rig = TabsRig::with(Some(true));
    let origin = solidcraft_ui_registry::rect_of("t:view:1").expect("view 1 is drawn");
    assert_eq!(rig.press(origin.center(), egui::PointerButton::Primary), vec![TabAction::SelectView(1)]);
    let space = egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    assert_eq!(rig.frame(vec![space]), vec![TabAction::LiftView { view: 1, origin }]);
}

#[test]
fn a_view_drop_slot_shifts_only_neighbours_at_or_after_the_local_index() {
    let rig = TabsRig::with(Some(true));
    rig.ctx.global_style_mut(|style| style.animation_time = 0.0);
    let strip = egui::Id::unique("test_tabs");
    let first = rig.at("t:view:0");
    let second = rig.at("t:view:1");
    let second_rect = solidcraft_ui_registry::rect_of("t:view:1").unwrap();
    let plus = rig.at("t:vnew");
    set_view_drag_slot(&rig.ctx, strip, Some(1), 38.0);
    rig.frame(Vec::new());
    assert_eq!(rig.at("t:view:0"), first);
    assert_eq!(rig.at("t:view:1"), second + vec2(38.0 + VIEW_GAP, 0.0));
    let gap = view_drag_gap(&rig.ctx, strip, 1).unwrap();
    assert_eq!(gap.left_top(), second_rect.left_top());
    assert_eq!(gap.size(), vec2(38.0, VIEW_HEIGHT));
    assert_eq!(rig.at("t:vnew"), plus + vec2(38.0 + VIEW_GAP, 0.0));
    set_view_drag_slot(&rig.ctx, strip, None, 0.0);
    rig.frame(Vec::new());
    assert_eq!(rig.at("t:view:1"), second);
    assert_eq!(rig.at("t:vnew"), plus);
    assert!(view_drag_gap(&rig.ctx, strip, 1).is_none());
}

/// With a view switcher the views list in a menu: a row picks a view, the footer's rows are the app's.
#[test]
fn the_view_switcher_lists_every_view() {
    let rig = TabsRig::with(Some(false));
    assert!(solidcraft_ui_registry::rect_of("t:view:0").is_none(), "no sub-tabs behind a switcher");
    rig.press(rig.at("t:switcher"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:switcher:row:2"), egui::PointerButton::Primary), vec![TabAction::SelectView(2)]);
    rig.press(rig.at("t:switcher"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:as_tabs"), egui::PointerButton::Primary), vec![TabAction::SwitcherMenu { id: "as_tabs".into() }]);
}

/// The first extra view is reachable from a one-view design, including the default switcher preference.
#[test]
fn a_single_view_tab_opens_the_add_view_menu_with_either_tab_preference() {
    for as_tabs in [false, true] {
        for clear in [false, true] {
            let mut rig = TabsRig::build(Some(as_tabs), true, None);
            rig.single_view = true;
            rig.clear = clear;
            rig.frame(Vec::new());
            let plus = solidcraft_ui_registry::rect_of("t:vnew").expect("a single-view design has an add-view button");
            if !as_tabs || !clear {
                let close = solidcraft_ui_registry::rect_of("t:close:0").expect("the plain tab retains its close button");
                assert!(!plus.intersects(close), "add-view and close keep separate hit targets");
            }
            assert!(rig.press(plus.center(), egui::PointerButton::Primary).is_empty());
            rig.frame(Vec::new());
            rig.frame(Vec::new());
            assert_eq!(rig.press(rig.at("t:menu:plus_render"), egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_render".into() }]);
        }
    }
}

/// A click on the + opens its menu and leaves it open: two frames later its rows are drawn under the
/// +'s left edge, and a row returns its id. A press in the frame that opened the menu is not a press
/// outside it.
#[test]
fn the_plus_menu_stays_open_after_its_click() {
    let rig = TabsRig::build(Some(true), true, None);
    let plus = solidcraft_ui_registry::rect_of("t:vnew").expect("the + is drawn");
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let row = solidcraft_ui_registry::handle_at("t:menu:plus_render").expect("the menu's row is drawn");
    assert!(row.y > HEIGHT && row.x >= plus.left(), "the menu opens under the + at {row:?}");
    assert_eq!(rig.press(row, egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_render".into() }]);
    // A press and release in one frame (a click driven by a script) leaves the menu open too.
    let at = rig.at("t:vnew");
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    rig.frame(vec![button(true), button(false)]);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:plus_render").is_some());
}

/// The + menu's header is drawn but never chosen, every row (with a glyph or without) returns its
/// id, and each row is published at its place under the header.
#[test]
fn the_plus_menu_has_a_header_icon_rows_and_a_separator() {
    let rig = TabsRig::build(Some(true), true, None);
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:Open as a view").is_none(), "the header is not a row");
    let (render, dup) = (rig.at("t:menu:plus_render"), rig.at("t:menu:plus_dup"));
    assert!(dup.y - render.y > 32.0, "a separator sits above Duplicate: {render:?} {dup:?}");
    assert_eq!(rig.press(dup, egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_dup".into() }]);
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:plus_reopen"), egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_reopen".into() }]);
}

const SHEET_PLUS_MENU: [TabMenuRow<'static>; 1] = [TabMenuRow::new("sheet", "Sheet", "")
    .with_children(&[TabMenuRow::new("sheet.new", "New sheet", ""), TabMenuRow::new("sheet.open:drawing/sheet", "Drawing / Sheet 1", "")])];

#[test]
fn plus_sheet_submenu_hover_survives_the_gap_and_returns_the_child_action() {
    let mut rig = TabsRig::build(Some(true), true, None);
    rig.plus_rows = &SHEET_PLUS_MENU;
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let parent = rig.at("t:menu:sheet");
    assert!(rig.frame(vec![egui::Event::PointerMoved(parent)]).is_empty(), "hovering Sheet opens its children without choosing the parent");
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let parent_rect = solidcraft_ui_registry::rect_of("t:menu:sheet").expect("Sheet parent stays drawn");
    let child_rect = solidcraft_ui_registry::rect_of("t:menu:sheet.open:drawing/sheet").expect("the existing Sheet child is drawn");
    let gap_x = if child_rect.left() > parent_rect.right() {
        (parent_rect.right() + child_rect.left()) * 0.5
    } else {
        assert!(child_rect.right() < parent_rect.left(), "the flyout opens beside the parent");
        (child_rect.right() + parent_rect.left()) * 0.5
    };
    let gap = pos2(gap_x, parent_rect.center().y);
    assert!(rig.frame(vec![egui::Event::PointerMoved(gap)]).is_empty());
    assert!(solidcraft_ui_registry::rect_of("t:menu:sheet.open:drawing/sheet").is_some(), "crossing the gap keeps the same flyout open");
    let child = rig.at("t:menu:sheet.open:drawing/sheet");
    assert_eq!(rig.press(child, egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "sheet.open:drawing/sheet".into() }]);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:menu:sheet.open:drawing/sheet").is_none(), "choosing a child closes the menu");
}

#[test]
fn plus_sheet_submenu_arrow_right_and_enter_choose_the_child_action() {
    let mut rig = TabsRig::build(Some(true), true, None);
    rig.plus_rows = &SHEET_PLUS_MENU;
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    rig.frame(vec![egui::Event::PointerGone]);
    let key = |key, pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
    let parent_rect = solidcraft_ui_registry::rect_of("t:menu:sheet").expect("Sheet parent is drawn");
    let focused_rect = || rig.ctx.memory(|m| m.focused()).and_then(|id| rig.ctx.read_response(id)).map(|response| response.rect);
    for _ in 0..32 {
        if focused_rect() == Some(parent_rect) {
            break;
        }
        assert!(rig.frame(vec![key(egui::Key::Tab, true)]).is_empty());
        rig.frame(vec![key(egui::Key::Tab, false)]);
    }
    assert_eq!(focused_rect(), Some(parent_rect), "Tab reaches the Sheet row before entering its submenu");
    assert!(rig.frame(vec![key(egui::Key::ArrowRight, true)]).is_empty());
    rig.frame(vec![key(egui::Key::ArrowRight, false)]);
    // read_response reports the previous paint; let the flyout's invisible sizing pass
    // settle before comparing its focused response with the published visible row.
    rig.frame(Vec::new());
    let child_rect = solidcraft_ui_registry::rect_of("t:menu:sheet.new").expect("the first Sheet child is drawn");
    assert_eq!(focused_rect(), Some(child_rect), "ArrowRight transfers keyboard focus to the first child");
    assert_eq!(rig.frame(vec![key(egui::Key::Enter, true)]), vec![TabAction::PlusMenu { id: "sheet.new".into() }]);
}

/// The row builders set only what they say, and the glyphs the menus use exist.
#[test]
fn menu_row_builders_and_glyphs() {
    let plain = TabMenuRow::new("a", "A", "Ctrl+A");
    assert!(!plain.heading && !plain.separator_before && !plain.needs_others && plain.icon.is_empty() && plain.accent.is_none());
    let head = TabMenuRow::heading("H");
    assert!(head.heading && head.id == "H" && head.label == "H");
    let row = plain.separated().needing_others().with_icon("close", None);
    assert!(row.separator_before && row.needs_others && row.icon == "close");
    for glyph in ["model", "sheet", "render", "animation", "rename", "close", "duplicate", "tiles", "reopen"] {
        assert!(!glyph_lines(glyph).is_empty(), "{glyph} has strokes");
    }
}

/// A view's menu opens under that view's left edge with its key hints, on any view; the rows that
/// need other views are off for a design's only view, and "to the right" is off for its last view.
#[test]
fn a_view_menu_opens_under_its_view_and_turns_off_rows_that_do_not_apply() {
    let rig = TabsRig::with(Some(true));
    let view = solidcraft_ui_registry::rect_of("t:view:1").expect("view 1 is drawn");
    rig.press(rig.at("t:view:1"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let close = solidcraft_ui_registry::handle_at("t:menu:vclose").expect("the menu is drawn");
    assert!(close.y > HEIGHT && close.x >= view.left(), "the menu opens under view 1 at {close:?}");
    assert_eq!(rig.press(rig.at("t:menu:vright"), egui::PointerButton::Primary), vec![TabAction::ViewMenu { view: 1, id: "vright".into() }]);
    // The last view has no views to its right: that row does nothing.
    rig.press(rig.at("t:view:2"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(rig.press(rig.at("t:menu:vright"), egui::PointerButton::Primary).is_empty());
    rig.press(rig.at("t:view:2"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:vothers"), egui::PointerButton::Primary), vec![TabAction::ViewMenu { view: 2, id: "vothers".into() }]);
}

#[test]
fn shift_f10_opens_the_menu_of_the_focused_view_tab() {
    let mut rig = TabsRig::with(Some(true));
    rig.clear = true;
    rig.frame(Vec::new());
    rig.press(rig.at("t:view:0"), egui::PointerButton::Primary);
    rig.frame(vec![egui::Event::Key { key: egui::Key::F10, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT }]);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:menu:vclose").is_some());
}

#[test]
fn detached_view_has_a_window_mark_and_retains_its_own_click_targets() {
    let mut rig = TabsRig::with(Some(true));
    let normal = solidcraft_ui_registry::rect_of("t:view:1").expect("view is drawn");
    rig.detached = true;
    rig.frame(Vec::new());
    let detached = solidcraft_ui_registry::rect_of("t:view:1").expect("detached view stays in the strip");
    let mark = solidcraft_ui_registry::rect_of("t:view:1:window").expect("detached view has a window mark");
    let close = solidcraft_ui_registry::rect_of("t:vclose:1").expect("detached view can still be closed");
    assert!(detached.width() > normal.width());
    assert!(detached.contains_rect(mark) && detached.contains_rect(close));
    assert!(mark.right() < close.left(), "the window mark does not cover the close button");
    assert_eq!(rig.press(mark.center(), egui::PointerButton::Primary), vec![TabAction::SelectView(1)]);
    assert_eq!(rig.press(close.center(), egui::PointerButton::Primary), vec![TabAction::CloseView(1)]);
    rig.press(mark.center(), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:vclose").is_some());
    assert!(solidcraft_ui_registry::handle_at("t:menu:move_to_window").is_none());
}

#[test]
fn pop_out_pill_keeps_one_visible_view_with_no_design_close() {
    let mut rig = TabsRig::with(Some(false));
    rig.pop_out = true;
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:view:0").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:vclose:0").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:vnew").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:close:0").is_none());
    assert!(solidcraft_ui_registry::handle_at("t:new").is_none());
}

#[test]
fn view_window_submenu_returns_the_child_action() {
    let rig = TabsRig::with(Some(true));
    rig.press(rig.at("t:view:1"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let row = rig.at("t:menu:move_to_window");
    assert!(rig.press(row, egui::PointerButton::Primary).is_empty());
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let child = rig.at("t:menu:move_new");
    assert!(child.x > row.x, "the submenu opens beside the parent row");
    assert_eq!(rig.press(child, egui::PointerButton::Primary), vec![TabAction::ViewMenu { view: 1, id: "move_new".into() }]);
}

/// A view being renamed shows its name field in its sub-tab: Enter keeps the name (cut to the
/// longest taken), Escape gives it up.
#[test]
fn a_view_is_renamed_in_its_sub_tab() {
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    let rig = TabsRig::build(Some(true), false, Some((1, "A much longer name")));
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:rename").is_some(), "the field is drawn in the sub-tab");
    assert_eq!(rig.text.borrow().chars().count(), 8, "the name is cut to the longest taken");
    assert_eq!(rig.frame(vec![key(egui::Key::Enter)]), vec![TabAction::ViewRenameDone { view: 1 }]);
    let rig = TabsRig::build(Some(true), false, Some((2, "Sheet 1")));
    rig.frame(Vec::new());
    assert_eq!(rig.frame(vec![key(egui::Key::Escape)]), vec![TabAction::ViewRenameCancelled]);
}

#[test]
fn a_narrow_switcher_keeps_its_targets_inside_the_tab() {
    let mut rig = TabsRig::with(Some(false));
    rig.room = Some(304.0);
    rig.name = "A design name much wider than the space left for its tab";
    rig.frame(Vec::new());
    let switcher = solidcraft_ui_registry::rect_of("t:switcher").expect("switcher remains available");
    let close = solidcraft_ui_registry::rect_of("t:close:0").expect("close remains available");
    assert!(switcher.is_positive() && switcher.left() >= 20.0 && switcher.right() <= 108.0);
    assert!(close.right() <= 108.0 && !close.intersects(switcher));
    assert!(rig.press(switcher.center(), egui::PointerButton::Primary).is_empty());
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:switcher:row:1"), egui::PointerButton::Primary), vec![TabAction::SelectView(1)]);
}

#[test]
fn renaming_a_view_remains_editable_in_the_switcher_fallback() {
    let mut rig = TabsRig::build(Some(true), false, Some((1, "Sketch")));
    rig.room = Some(304.0);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:switcher").is_some(), "the narrow strip falls back to its switcher");
    let field = solidcraft_ui_registry::rect_of("t:rename").expect("the requested editor remains visible");
    let switcher = solidcraft_ui_registry::rect_of("t:switcher").unwrap();
    assert!(switcher.contains_rect(field));
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    assert_eq!(rig.frame(vec![key(egui::Key::Enter)]), vec![TabAction::ViewRenameDone { view: 1 }]);
    let mut single = TabsRig::build(Some(false), false, Some((0, "Model")));
    single.single_view = true;
    single.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:rename").is_some(), "the single-view tab exposes its requested rename field");
    assert_eq!(single.frame(vec![key(egui::Key::Enter)]), vec![TabAction::ViewRenameDone { view: 0 }]);
    let rig = TabsRig::build(Some(false), false, Some((2, "Sheet")));
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:rename").is_some(), "switcher mode can rename too");
    assert_eq!(rig.frame(vec![key(egui::Key::Escape)]), vec![TabAction::ViewRenameCancelled]);
}

#[test]
fn changing_the_design_discards_its_open_view_popup() {
    let rig = TabsRig::with(Some(true));
    rig.press(rig.at("t:view:1"), egui::PointerButton::Secondary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:vclose").is_some());
    clear_view_popup(&rig.ctx, egui::Id::unique("test_tabs"));
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:vclose").is_none());
}

// The layout engine: pure arithmetic over a spec, no egui.
use crate::app_bar_layout::{Item, MAX_ITEMS, Placed, Spec, Zone, default_bar, layout};

const EPS: f32 = 0.05;

/// The default bar `width` wide with Windows' caption buttons on the right.
fn default_spec(width: f32) -> Spec {
    default_bar::spec(width, 0.0, 138.0)
}

fn index_of(spec: &Spec, id: &str) -> usize {
    spec.items.iter().position(|item| item.id == id).expect("the item is in the spec")
}

/// Every placement lies in the bar's room, none has a negative or non-finite width, and no two
/// overlap (the overflow button and the centre item included).
fn assert_sound(spec: &Spec, placed: &Placed) {
    let clamp = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, crate::app_bar_layout::MAX_WIDTH) };
    let (left, right) = (clamp(spec.captions_left), clamp(spec.width) - clamp(spec.captions_right));
    assert_eq!(placed.placements.len(), spec.items.len());
    let mut spans: Vec<(f32, f32, String)> = Vec::new();
    for (p, item) in placed.placements.iter().zip(&spec.items) {
        if let Some(p) = p {
            assert!(p.width.is_finite() && p.width >= 0.0 && p.x.is_finite(), "{}: {p:?}", item.id);
            spans.push((p.x, p.right(), item.id.clone()));
        }
    }
    if let Some((x, w)) = placed.overflow_button {
        assert!(w >= 0.0 && x.is_finite());
        spans.push((x, x + w, "overflow".into()));
    }
    // An item narrower than the tolerance takes no room (its width is lost in the float rounding
    // of a wide neighbour), so it may sit anywhere inside the others' span.
    spans.retain(|(x, r, _)| r - x > EPS);
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (x, r, id) in &spans {
        assert!(*x >= left - EPS && *r <= right.max(left) + EPS, "{id} spans {x}..{r} outside {left}..{right}");
    }
    for pair in spans.windows(2) {
        assert!(pair[0].1 <= pair[1].0 + EPS, "{} ({}) overlaps {} ({})", pair[0].2, pair[0].1, pair[1].2, pair[1].0);
    }
}

fn assert_edit_cards(spec: &Spec, placed: &Placed) {
    use crate::app_bar_layout::application::CARD_PADDING;
    let g = placed.application.as_ref().unwrap();
    for (zone, card) in [Zone::Start, Zone::End].into_iter().zip(g.cards) {
        let Some(card) = card else { continue };
        let slots: Vec<_> = spec
            .items
            .iter()
            .zip(&placed.placements)
            .filter_map(|(item, slot)| {
                (!item.locked && item.zone == zone && !matches!(item.id.as_str(), "managed" | "offered")).then_some(slot.as_ref()).flatten()
            })
            .collect();
        let first = slots.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let last = slots.iter().map(|p| p.right()).fold(f32::NEG_INFINITY, f32::max);
        assert!(card.left <= first + EPS && first - card.left <= CARD_PADDING + EPS);
        if first - spec.captions_left >= CARD_PADDING {
            assert!((first - CARD_PADDING - card.left).abs() <= EPS, "the logo/tabs gap gives the first item full paint padding");
        } else {
            // At an impossibly narrow left boundary the engine may omit even the logo.
            // Paint clips at the traffic-light/screen edge rather than moving File.
            assert!((card.left - spec.captions_left).abs() <= EPS);
        }
        assert!((last + CARD_PADDING - card.right).abs() <= EPS, "{zone:?} has full right padding at width {}: {card:?}, last {last}", spec.width);
        assert!(card.left >= spec.captions_left - EPS);
        assert!(card.right <= (spec.width - spec.captions_right).max(spec.captions_left) + EPS);
        for (item, p) in spec.items.iter().zip(&placed.placements) {
            if (item.locked || matches!(item.id.as_str(), "managed" | "offered"))
                && let Some(p) = p
            {
                assert!(card.right <= p.x + EPS || p.right() <= card.left + EPS, "card {card:?} overlaps fixed {} {p:?}", item.id);
            }
        }
        if let Some((x, width)) = placed.overflow_button {
            assert!(card.right <= x + EPS || x + width <= card.left + EPS);
        }
    }
}

#[test]
fn app_bar_edit_separator_delta_and_paint_cards_stay_between_chrome_at_every_width() {
    use crate::app_bar_items::{Layout, Zone as SavedZone};
    use crate::app_bar_layout::application::{EDIT_SEPARATOR_WIDTH, SEPARATOR_WIDTH, SPACE_WIDTH, set_editing, spec_with_items};
    let defaults = Layout::default_for(["a", "b", "c", "d", "e"]);
    for mixed in [false, true] {
        let mut saved = defaults.clone();
        if mixed {
            saved.move_before("file", SavedZone::End, Some("version"), &defaults);
            saved.move_before("separator:1", SavedZone::End, Some("version"), &defaults);
        }
        saved.add_space(SavedZone::Start, None, &defaults);
        saved.add_space(SavedZone::End, None, &defaults);
        for width in 200..=3840 {
            let normal = spec_with_items(
                (width as f32, 78.0, 138.0),
                measured_bar(),
                &saved,
                &["save", "undo", "redo"],
                &["a", "b", "c", "d", "e"],
                &["donate", "discord"],
            );
            let mut bare = normal.clone();
            bare.items.retain(|p| !p.id.starts_with("separator:"));
            let plain = layout(&bare);
            set_editing(&mut bare, true);
            let decorated = layout(&bare);
            assert_eq!(plain.placements, decorated.placements, "paint cards consume no layout space");
            assert_eq!(plain.overflow, decorated.overflow);
            assert_eq!(plain.centre, decorated.centre);
            assert_eq!(plain.overflow_button, decorated.overflow_button);
            let mut edited = normal.clone();
            set_editing(&mut edited, true);
            for (a, b) in normal.items.iter().zip(&edited.items) {
                if a.id.starts_with("separator:") {
                    assert_eq!(a.ladder, [SEPARATOR_WIDTH]);
                    assert_eq!(b.ladder, [EDIT_SEPARATOR_WIDTH]);
                } else {
                    assert_eq!(a, b, "other items' fit ladders are identical");
                    if a.id.starts_with("space:") {
                        assert_eq!(a.ladder, [SPACE_WIDTH]);
                    }
                }
            }
            let delta = edited.items.iter().map(|i| i.ladder[0]).sum::<f32>() - normal.items.iter().map(|i| i.ladder[0]).sum::<f32>();
            assert_eq!(delta, 2.0 * (EDIT_SEPARATOR_WIDTH - SEPARATOR_WIDTH));
            for spec in [&normal, &edited] {
                let p = layout(spec);
                assert_sound(spec, &p);
                assert_edit_cards(spec, &p);
                for (item, slot) in spec.items.iter().zip(&p.placements) {
                    if item.zone == Zone::End
                        && let Some(slot) = slot
                    {
                        assert!(
                            slot.right() + spec.gap <= spec.width - spec.captions_right + EPS,
                            "the End stack leaves one gap before window buttons in both modes"
                        );
                    }
                }
            }
            set_editing(&mut edited, false);
            assert_eq!(normal, edited, "leaving edit mode restores the exact spec");
        }
    }
    assert_eq!(height(true), height(false));
}

/// The default bar never overlaps and stays inside its room at any width, with the caption
/// buttons on the right (Windows) or the traffic lights on the left (macOS).
#[test]
fn the_default_bar_never_overlaps_at_any_width() {
    for width in 200..=3840 {
        let spec = default_spec(width as f32);
        assert_sound(&spec, &layout(&spec));
        let mac = default_bar::spec(width as f32, 78.0, 0.0);
        assert_sound(&mac, &layout(&mac));
    }
}

/// On a wide bar every item has its fullest step, Start items run left to right in today's order
/// and End items likewise, with the tabs between them.
#[test]
fn a_wide_bar_shows_everything_in_todays_order() {
    let spec = default_spec(3840.0);
    let placed = layout(&spec);
    assert!(placed.overflow.is_empty() && placed.overflow_button.is_none());
    let mut last = f32::MIN;
    for zone in [Zone::Start, Zone::Centre, Zone::End] {
        for item in spec.items.iter().filter(|i| i.zone == zone) {
            let p = placed.placements[index_of(&spec, &item.id)].expect("placed on a wide bar");
            assert_eq!(p.step, 0, "{} is at its fullest", item.id);
            if zone != Zone::Centre {
                assert!((p.width - item.ladder[0]).abs() < EPS, "{} keeps its fullest width", item.id);
            }
            assert!(p.x >= last - EPS, "{} is out of order ({} < {last})", item.id, p.x);
            last = p.right();
        }
    }
    let (x, w) = placed.centre.expect("the tabs have a place");
    assert!(w > 1000.0, "the tabs take the spare room, got {w}");
    let start = spec.items.iter().filter(|i| i.zone == Zone::Start).count();
    let widths: f32 = spec.items.iter().filter(|i| i.zone == Zone::Start).map(|i| i.ladder[0]).sum();
    assert!((x - widths - start as f32 * spec.gap).abs() < EPS, "tabs follow the uniform stack: {x}");
    assert_eq!(placed.placements[0].expect("the logo").x, 0.0);
    let end = placed.placements.iter().flatten().map(|p| p.right()).fold(0.0_f32, f32::max);
    assert!((end - (3840.0 - 138.0)).abs() < EPS, "the stack ends at the caption buttons");
}

/// As the bar narrows the version goes first, then the pills, then the Report a bug label shrinks.
#[test]
fn items_give_way_in_priority_order() {
    let (mut version_at, mut pills_at, mut report_label_at) = (0, 0, 0);
    let mut previous: Vec<String> = Vec::new();
    for width in (200..=3840).rev() {
        let spec = default_spec(width as f32);
        let placed = layout(&spec);
        let now = placed.overflow.clone();
        assert!(now.starts_with(&previous), "the overflow only grows as the bar narrows ({width}): {previous:?} then {now:?}");
        previous = now.clone();
        if version_at == 0 && now.iter().any(|id| id == "version") {
            version_at = width;
        }
        if pills_at == 0 && now.iter().any(|id| id == "donate" || id == "discord") {
            pills_at = width;
        }
        if report_label_at == 0 && placed.placements[index_of(&spec, "report")].is_some_and(|p| p.step > 0) {
            report_label_at = width;
        }
    }
    assert!(version_at > pills_at, "the version leaves at {version_at}, the pills at {pills_at}");
    assert!(pills_at > report_label_at, "the pills leave at {pills_at}, the report label at {report_label_at}");
    assert!(report_label_at > 0, "the report label does collapse on a narrow bar");
    assert_eq!(layout(&default_spec(version_at as f32)).overflow.last().map(String::as_str), Some("version"), "separators yield before the version");
}

/// Locked items and the tabs' least width are kept at every width where they fit at all.
#[test]
fn locked_items_and_the_tabs_minimum_are_always_kept() {
    for width in 480..=3840 {
        let spec = default_spec(width as f32);
        let placed = layout(&spec);
        for item in spec.items.iter().filter(|i| i.locked || i.never_overflow) {
            assert!(placed.placements[index_of(&spec, &item.id)].is_some(), "{} is kept at {width}", item.id);
            assert!(!placed.overflow.contains(&item.id));
        }
        let (_, w) = placed.centre.expect("the tabs are kept");
        assert!(w >= default_bar::TABS_MIN - EPS, "the tabs keep their least at {width}, got {w}");
    }
}

/// Hostile specs never panic and never overlap: NaN, infinite and negative widths, an empty list,
/// repeated ids, empty ladders, a bar too narrow for its locked items and ten thousand items.
#[test]
fn hostile_specs_never_panic_or_overlap() {
    for width in 0..200 {
        let spec = default_spec(width as f32);
        assert_sound(&spec, &layout(&spec));
    }
    let nasty = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 0.0, 1.0e30, -1.0e30, 40.0, f32::MIN_POSITIVE];
    for &w in &nasty {
        for &c in &nasty {
            let mut spec = default_spec(w);
            spec.captions_left = c;
            spec.captions_right = c;
            spec.gap = c;
            spec.overflow_button_width = w;
            assert_sound(&spec, &layout(&spec));
            for item in &mut spec.items {
                item.ladder = vec![w, c, w];
            }
            assert_sound(&spec, &layout(&spec));
        }
    }
    let empty =
        Spec { width: 800.0, captions_left: 0.0, captions_right: 0.0, gap: 4.0, overflow_button_width: 30.0, items: Vec::new(), application: None };
    let placed = layout(&empty);
    assert!(placed.placements.is_empty() && placed.centre.is_none() && placed.overflow.is_empty());
    // A repeated id and a second centre item are ignored; an empty ladder is a zero width.
    let mut spec = default_spec(1000.0);
    spec.items.push(Item::new("save", Zone::End, &[34.0], 1));
    spec.items.push(Item::new("other-tabs", Zone::Centre, &[50.0], 1));
    spec.items.push(Item::new("no-ladder", Zone::Start, &[], 1));
    let placed = layout(&spec);
    assert_sound(&spec, &placed);
    let n = spec.items.len();
    assert!(placed.placements[n - 3].is_none() && placed.placements[n - 2].is_none());
    // Ten thousand items in every zone: past the cap nothing is placed.
    for width in [0.0, 100.0, 1000.0, 5000.0, 1.0e9] {
        let items = (0..10_000)
            .map(|i| {
                let zone = [Zone::Start, Zone::Centre, Zone::End][i % 3];
                Item::new(&format!("item{i}"), zone, &[30.0 + (i % 7) as f32, 20.0], (i % 251) as u8)
            })
            .collect();
        let spec = Spec { width, captions_left: 10.0, captions_right: 10.0, gap: 2.0, overflow_button_width: 24.0, items, application: None };
        let placed = layout(&spec);
        assert_sound(&spec, &placed);
        assert!(placed.placements.iter().skip(MAX_ITEMS).all(|p| p.is_none()));
    }
}

/// A small deterministic generator (an LCG) drives random specs: all sound, all repeatable.
#[test]
fn random_specs_never_overlap() {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move |n: u64| {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % n
    };
    for round in 0..2000 {
        let count = next(20) as usize;
        let items = (0..count)
            .map(|i| {
                let zone = [Zone::Start, Zone::Centre, Zone::End][next(3) as usize];
                let ladder: Vec<f32> = (0..next(4)).map(|_| next(200) as f32).collect();
                let id = format!("i{}", if next(10) == 0 { 0 } else { i });
                let mut item = Item::new(&id, zone, &ladder, next(256) as u8);
                item.locked = next(4) == 0;
                item.flexible = next(2) == 0;
                item
            })
            .collect();
        let spec = Spec {
            width: next(1500) as f32,
            captions_left: next(150) as f32,
            captions_right: next(150) as f32,
            gap: next(10) as f32,
            overflow_button_width: next(60) as f32,
            items,
            application: None,
        };
        let placed = layout(&spec);
        assert_sound(&spec, &placed);
        assert_eq!(placed, layout(&spec), "round {round} is deterministic");
    }
}

/// The same spec always gives the same answer, and equal priorities give way in source order.
#[test]
fn layout_is_deterministic() {
    for width in [320.0, 640.0, 900.0, 1280.0, 1920.0] {
        let spec = default_spec(width);
        assert_eq!(layout(&spec), layout(&spec.clone()));
    }
    let mut spec =
        Spec { width: 100.0, captions_left: 0.0, captions_right: 0.0, gap: 0.0, overflow_button_width: 10.0, items: Vec::new(), application: None };
    for id in ["a", "b", "c", "d"] {
        spec.items.push(Item::new(id, Zone::Start, &[40.0], 5));
    }
    let placed = layout(&spec);
    assert_eq!(placed.overflow, ["a", "b"], "equal priorities leave in source order");
    assert!(placed.placements[2].is_some() && placed.placements[3].is_some());
}

/// Exercise the newly projected pill targets and app-owned leading/search/assistant spaces
/// through real egui input. Existing bar tests cover the other action variants.
#[test]
fn app_bar_projected_pills_return_clicks_and_hide_clicks() {
    let ctx = crate::test_support::context();
    let bar = AppBar {
        items: None,
        quick_ids: &[],
        mark: &|_, _| {},
        name: "App",
        leading: Some(vec2(24.0, 24.0)),
        home: button("home", true, false),
        file: "File",
        file_open: false,
        actions: vec![button("save", true, false)],
        quick: 5,
        help: button("help", true, false),
        settings: button("settings", true, false),
        report: Report { label: "Report a bug", tip: "Report", icon_only: false },
        pills: ["donate", "discord"]
            .into_iter()
            .map(|id| Pill {
                id,
                icon: "help",
                label: id,
                labelled: true,
                tip: id,
                hide_tip: "Hide",
                name: id,
                hide_name: "Hide",
                fill: egui::Color32::WHITE,
                ink: egui::Color32::BLACK,
            })
            .collect(),
        search: true,
        search_shortcut: Some("Ctrl+K"),
        assistant: true,
        version: None,
        handles: HANDLES,
    };
    let time = std::cell::Cell::new(0.0);
    let frame = |events| {
        solidcraft_ui_registry::clear_handles();
        time.set(time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 400.0))),
            time: Some(time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let mut actions = Vec::new();
        crate::test_support::raw_frame(&ctx, input, |ui| {
            let r = Rect::from_min_size(pos2(17.25, 0.0), vec2(1800.0, HEIGHT));
            let out = show(ui, &bar, r, r.shrink2(vec2(78.0, 0.0)));
            assert!(out.leading.is_some() && out.search.is_some() && out.assistant.is_some());
            actions = out.actions;
        });
        actions
    };
    frame(Vec::new());
    for id in ["donate", "discord"] {
        for hide in [false, true] {
            let name = format!("t:{id}{}", if hide { ":hide" } else { "" });
            let at = solidcraft_ui_registry::handle_at(&name).expect("pill target");
            frame(vec![egui::Event::PointerMoved(at)]);
            frame(vec![press(at, true)]);
            let actions = frame(vec![press(at, false)]);
            assert_eq!(actions, vec![if hide { BarAction::HidePill { id: id.into() } } else { BarAction::Pill { id: id.into() } }]);
        }
    }
}

fn measured_bar() -> crate::app_bar_layout::application::Measurements {
    crate::app_bar_layout::application::Measurements {
        origin: 0.0,
        name: 50.0,
        file: 40.0,
        leading: Some(24.0),
        actions: vec![false, true, true],
        quick: 5,
        report: 60.0,
        report_icon_only: false,
        pills: vec![45.0, 45.0],
        search: Some((160.0, 42.0)),
        assistant: true,
        version: Some(30.0),
        offered: Some(80.0),
        managed: None,
        pixels_per_point: 1.0,
    }
}

/// A hand-derived uniform stack: logo 80, square controls 26, dividers 1 and gaps 4.
#[test]
fn app_bar_measured_groups_project_their_controls() {
    use crate::app_bar_layout::application::{GAP, Span, spec};
    let spec = spec(1920.0, 0.0, 138.0, measured_bar());
    let placed = layout(&spec);
    let g = placed.application.as_ref().unwrap();
    assert_eq!(spec.gap, GAP);
    assert_eq!(g.leading, Some(Span { left: 84.0, right: 110.0 }));
    assert_eq!(g.home, Span { left: 114.0, right: 140.0 });
    assert_eq!(g.file, Span { left: 149.0, right: 189.0 });
    assert_eq!(g.actions, [Some(206.0), Some(236.0), Some(266.0)]);
    assert_eq!(g.quick, Some((296.0, 30.0)));
    assert_eq!(g.tabs_start, 438.0);
    // The same gap as between items now also precedes the caption buttons.
    assert_eq!(g.report, Span { left: 1546.0 - GAP, right: 1640.0 - GAP });
    assert_eq!(g.version, Some(Span { left: 1644.0 - GAP, right: 1698.0 - GAP }));
    for zone in [Zone::Start, Zone::End] {
        let slots: Vec<_> = spec.items.iter().zip(&placed.placements).filter(|(i, _)| i.zone == zone).filter_map(|(_, p)| *p).collect();
        for pair in slots.windows(2) {
            assert!((pair[1].x - pair[0].right() - GAP).abs() < EPS);
        }
    }
}

/// Reserving transients without link pills must keep the same wide-bar anchors too.
#[test]
fn app_bar_wide_transients_keep_the_established_positions_without_links() {
    use crate::app_bar_layout::application::{GAP, spec};
    let mut m = measured_bar();
    m.pills.clear();
    m.managed = Some(120.0);
    let spec = spec(1920.0, 0.0, 138.0, m);
    let p = layout(&spec);
    assert!(p.overflow.is_empty());
    let g = p.application.unwrap();
    assert!(g.reserved_offers);
    assert_eq!(g.offer.right, 1920.0 - 138.0 - GAP);
    assert_eq!(g.managed.unwrap().right + GAP, g.offer.left);
    assert_eq!(g.offer.right - g.offer.left, 80.0);
    assert!(g.tabs_end + GAP <= g.managed.unwrap().left);
}

#[test]
fn app_bar_measured_pills_snap_in_window_coordinates() {
    use crate::app_bar_layout::application::spec;
    let mut m = measured_bar();
    m.origin = 17.25;
    m.pixels_per_point = 2.0;
    m.pills = vec![55.3, 56.7];
    let spec = spec(1920.0, 78.0, 0.0, m);
    let p = layout(&spec);
    let g = p.application.as_ref().unwrap();
    for (i, span) in g.pills.iter().enumerate() {
        let slot = p.placements[index_of(&spec, &format!("pill:{i}"))].unwrap();
        assert_eq!((span.left, span.right), (slot.x, slot.right()), "projection adds no per-pill margins or offsets");
    }
}

/// Hostile measurements produce bounded work, finite drawing coordinates and repeatable results.
#[test]
fn app_bar_hostile_measurements_are_finite_and_bounded() {
    use crate::app_bar_layout::application::spec;
    let nasty = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 0.0, f32::MAX, f32::MIN_POSITIVE, 640.0];
    for &width in &nasty {
        for &v in &nasty {
            let mut m = measured_bar();
            m.origin = v;
            m.name = v;
            m.file = v;
            m.leading = Some(v);
            m.actions = vec![false; 10_000];
            m.quick = usize::MAX;
            m.report = v;
            m.pills = vec![v; 10_000];
            m.search = Some((v, v));
            m.version = Some(v);
            m.offered = Some(v);
            m.pixels_per_point = v;
            let spec = spec(width, v, v, m);
            assert!(spec.items.len() <= MAX_ITEMS);
            let p = layout(&spec);
            assert_eq!(p, layout(&spec));
            for p in p.placements.iter().flatten() {
                assert!(p.x.is_finite() && p.width.is_finite() && p.width >= 0.0);
            }
            let g = p.application.expect("geometry");
            assert!(g.actions.len() <= MAX_ITEMS);
            for x in [g.mark, g.name, g.tabs_start, g.controls_left, g.help] {
                assert!(x.is_finite());
            }
            let spans = [Some(g.home), Some(g.file), Some(g.report), Some(g.settings), g.leading, g.search, g.assistant, g.version];
            for s in spans.into_iter().flatten().chain(g.pills) {
                assert!(s.left.is_finite() && s.right.is_finite() && s.right >= s.left, "{s:?}");
            }
        }
    }
}

/// Every combination of the real bar's options, including transient reservations, uses the
/// same no-overlap contract as the plain engine. Check each integer width, including captions.
#[test]
fn app_bar_application_never_overlaps_at_any_width() {
    for left in [0.0, 78.0] {
        for quick in [0, 5, 24] {
            for bits in 0..1024 {
                for width in 200..=3840 {
                    let has = |bit| bits & (1 << bit) != 0;
                    let mut m = measured_bar();
                    m.name = if has(6) { 380.0 } else { 50.0 };
                    m.quick = quick;
                    m.search = has(0).then_some((160.0, 42.0));
                    m.report_icon_only = has(1);
                    m.version = has(2).then_some(30.0);
                    m.pills = if has(3) { vec![100.0, 106.0] } else { Vec::new() };
                    m.assistant = has(4);
                    m.offered = has(5).then_some(110.0);
                    m.managed = has(8).then_some(160.0);
                    let mut spec = crate::app_bar_layout::application::spec(width as f32, left, if has(7) { 138.0 } else { 0.0 }, m);
                    crate::app_bar_layout::application::set_editing(&mut spec, has(9));
                    let placed = layout(&spec);
                    assert_sound(&spec, &placed);
                    for item in spec.items.iter().filter(|i| i.locked || i.never_overflow) {
                        assert!(!placed.overflow.contains(&item.id), "locked {} at {width}/{bits}/{quick}", item.id);
                        // The same four-point gap also separates the End stack from chrome.
                        if spec.width - spec.captions_left - spec.captions_right - spec.gap >= 232.0 {
                            assert!(placed.placements[index_of(&spec, &item.id)].is_some(), "locked {} fits at {width}/{bits}/{quick}", item.id);
                        }
                    }
                    let g = placed.application.as_ref().expect("geometry");
                    assert!(g.quick_count <= quick);
                    // Actual painted slots must stay inside their corresponding engine allocation.
                    for (id, span) in [("search", g.search), ("assistant", g.assistant), ("managed", g.managed)] {
                        if let Some(span) = span.filter(|_| id != "managed" || g.reserved_offers) {
                            let slot = placed.placements[index_of(&spec, id)].expect("visible slot");
                            assert!(span.left >= slot.x - EPS && span.right <= slot.right() + EPS);
                        }
                    }
                    if placed.overflow.iter().any(|id| id == "offered") {
                        assert_eq!(g.offer.left, g.offer.right, "hidden update cannot paint over tabs");
                    }
                }
            }
        }
    }
}

#[test]
fn app_bar_overflow_follows_bar_order_and_quick_buttons_leave_from_the_end() {
    let mut m = measured_bar();
    m.quick = 24;
    let spec = crate::app_bar_layout::application::spec(400.0, 0.0, 0.0, m);
    let p = layout(&spec);
    let indices: Vec<_> = p.overflow.iter().map(|id| index_of(&spec, id)).collect();
    assert!(indices.windows(2).all(|p| p[0] < p[1]));
    let mut m = measured_bar();
    m.quick = 24;
    let p = layout(&crate::app_bar_layout::application::spec(1100.0, 0.0, 0.0, m));
    let g = p.application.expect("geometry");
    assert!(g.quick_count > 0 && g.quick_count < 24, "this case retains a prefix of quick buttons");
    for i in g.quick_count..24 {
        assert!(p.overflow.contains(&format!("quick:{i}")));
    }
}

#[test]
fn app_bar_more_menu_returns_the_original_action_and_closes() {
    let rig = BarRig::new(250.0);
    rig.click("appbar:overflow");
    rig.frame(Vec::new());
    let rows = rig.out.borrow().as_ref().expect("bar").overflow_items.clone();
    let report = rows.iter().find(|r| r.id == "report").expect("report overflowed");
    assert_eq!(report.action, BarAction::Report);
    assert_eq!(rig.click("menu:appbar:overflow:report"), [BarAction::Report]);
    assert!(!rig.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_appbar_overflow_open")).unwrap_or(false)));
    rig.click("appbar:overflow");
    rig.frame(Vec::new());
    assert!(rig.click("menu:appbar:overflow:action:1").is_empty(), "disabled undo cannot run");
    assert!(rig.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_appbar_overflow_open")).unwrap_or(false)));
    rig.frame(vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    assert!(!rig.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_appbar_overflow_open")).unwrap_or(false)));
    rig.click("appbar:overflow");
    rig.frame(Vec::new());
    // The uniform layout may put more rows in this menu, including at its bottom edge.
    // Click above the menu rather than assuming a particular number of overflow rows.
    let at = pos2(2.0, 2.0);
    for pressed in [true, false] {
        rig.frame(vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
        ]);
    }
    assert!(!rig.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("sc_appbar_overflow_open")).unwrap_or(false)));
}

mod app_bar_items {
    use crate::app_bar_items::{ItemId, Layout, Placement, Zone};
    use serde_json::json;

    #[test]
    fn defaults_match_the_actual_drawing_order() {
        let layout = Layout::default_for(["solid.box", "solid.fillet"]);
        let ids = |zone| layout.items.iter().filter(|p| p.zone == zone).map(|p| p.id.as_str()).collect::<Vec<_>>();
        assert_eq!(
            ids(Zone::Start),
            ["documents", "home", "separator:1", "file", "save", "undo", "redo", "quick:solid.box", "quick:solid.fillet", "separator:2"]
        );
        assert_eq!(ids(Zone::End), ["donate", "discord", "assistant", "search", "settings", "help", "report", "version"]);
        assert!(layout.items.iter().all(|p| p.shown));
        for item in &layout.items {
            assert_eq!(ItemId::parse(&item.id).unwrap().id(), item.id);
        }
        assert_eq!(Layout::default_for(["", "\0", &"x".repeat(95)]), Layout::default());
    }

    #[test]
    fn sanitize_keeps_unknowns_and_first_placement_and_restores_missing_items() {
        let defaults = Layout::default();
        let mut layout = Layout {
            items: vec![
                Placement { id: "save".into(), zone: Zone::End, shown: false },
                Placement { id: "future:item".into(), zone: Zone::Start, shown: true },
                Placement { id: "save".into(), zone: Zone::Start, shown: true },
            ],
        };
        layout.sanitize(&defaults);
        assert_eq!(layout.items.iter().filter(|p| p.id == "save").count(), 1);
        assert!(layout.items.iter().any(|p| p.id == "save" && p.zone == Zone::End && !p.shown));
        assert!(layout.items.iter().any(|p| p.id == "future:item"));
        assert!(defaults.items.iter().filter(|d| !d.id.starts_with("separator:")).all(|d| layout.items.iter().any(|p| p.id == d.id)));
        let once = layout.clone();
        layout.sanitize(&defaults);
        assert_eq!(layout, once);
    }

    #[test]
    fn hostile_rows_are_bounded_without_starving_known_items() {
        let defaults = Layout::default();
        let mut layout = Layout { items: (0..10_000).map(|i| Placement { id: format!("future:{i}"), zone: Zone::Start, shown: true }).collect() };
        layout.items.splice(
            0..0,
            ["".to_string(), "a".repeat(101), "\0home".to_string(), "🦀".repeat(100)].map(|id| Placement { id, zone: Zone::End, shown: true }),
        );
        layout.sanitize(&defaults);
        assert_eq!(layout.items.len(), 64);
        assert!(defaults.items.iter().filter(|d| !d.id.starts_with("separator:")).all(|d| layout.items.iter().any(|p| p.id == d.id)));
        assert!(layout.items.iter().any(|p| p.id == "🦀".repeat(100)));
        assert!(layout.items.iter().all(|p| !p.id.is_empty() && p.id.chars().count() <= 100 && !p.id.chars().any(char::is_control)));
    }

    #[test]
    fn json_round_trip_and_malformed_rows() {
        let defaults = Layout::default_for(["solid.box"]);
        let mut layout = defaults.clone();
        layout.items.reverse();
        layout.items[0].shown = false;
        layout.items.push(Placement { id: "future".into(), zone: Zone::End, shown: false });
        assert_eq!(Layout::from_value(&layout.to_value(), &defaults), layout);
        for value in [
            json!(null),
            json!([]),
            json!({"items":[7, {}, {"id":"save","zone":"centre","shown":true}, {"id":"update","zone":"start","shown":false}]}),
        ] {
            assert_eq!(Layout::from_value(&value, &defaults), defaults);
        }
    }
}

#[test]
fn app_bar_saved_defaults_use_the_unchanged_geometry_path() {
    use crate::app_bar_layout::application::{spec, spec_with_items};
    let quick = ["a", "b", "c", "d", "e"];
    let defaults = crate::app_bar_items::Layout::default_for(quick);
    for width in [200.0, 480.0, 640.0, 1280.0, 1920.0, 3840.0] {
        assert_eq!(
            spec(width, 0.0, 138.0, measured_bar()),
            spec_with_items((width, 0.0, 138.0), measured_bar(), &defaults, &["save", "undo", "redo"], &quick, &["donate", "discord"])
        );
    }
}

#[test]
fn app_bar_saved_items_move_hide_and_overflow_independently() {
    use crate::app_bar_items::{Layout, Zone as SavedZone};
    use crate::app_bar_layout::application::spec_with_items;
    let quick = ["a", "b", "c", "d", "e"];
    let mut saved = Layout::default_for(quick);
    saved.items.iter_mut().find(|p| p.id == "version").unwrap().shown = false;
    let minor = spec_with_items((3840.0, 0.0, 0.0), measured_bar(), &saved, &["save", "undo", "redo"], &quick, &["donate", "discord"]);
    assert!(index_of(&minor, "home") < index_of(&minor, "file"));
    assert!(index_of(&minor, "pill:1") < index_of(&minor, "assistant"));
    assert!(index_of(&minor, "assistant") < index_of(&minor, "offered"));
    saved.items.reverse();
    for p in &mut saved.items {
        if p.id == "undo" {
            p.shown = false;
        }
        if p.id == "quick:a" || p.id == "save" {
            p.zone = SavedZone::End;
        }
    }
    let spec = |width| spec_with_items((width, 0.0, 0.0), measured_bar(), &saved, &["save", "undo", "redo"], &quick, &["donate", "discord"]);
    let wide = spec(3840.0);
    let placed = layout(&wide);
    let g = placed.application.as_ref().unwrap();
    assert!(g.actions[1].is_none());
    assert!(g.actions[0].unwrap() > g.tabs_end);
    assert_eq!(g.quick_slots.len(), 5);
    assert!(g.quick_slots.iter().find(|(i, _)| *i == 0).unwrap().1 > g.tabs_end);
    assert!(g.quick_slots.iter().find(|(i, _)| *i == 4).unwrap().1 < g.quick_slots.iter().find(|(i, _)| *i == 3).unwrap().1);
    let narrow = spec(300.0);
    let placed = layout(&narrow);
    assert!(!placed.overflow.is_empty());
    assert!(!narrow.items.iter().any(|i| i.id == "action:1"));
    assert!(!placed.overflow.iter().any(|i| i == "action:1"));
    assert_sound(&narrow, &placed);
}

#[test]
fn app_bar_random_sanitized_layouts_never_overlap() {
    use crate::app_bar_items::{Layout, Placement, Zone as SavedZone};
    use crate::app_bar_layout::application::spec_with_items;
    let quick = ["a", "b", "c", "d", "e"];
    let defaults = Layout::default_for(quick);
    let mut seed = 0x19404_u64;
    let mut random = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        seed >> 32
    };
    for _ in 0..16 {
        let mut saved = defaults.clone();
        for i in (1..saved.items.len()).rev() {
            let j = random() as usize % (i + 1);
            saved.items.swap(i, j);
        }
        for p in &mut saved.items {
            p.zone = if random() & 1 == 0 { SavedZone::Start } else { SavedZone::End };
            p.shown = !random().is_multiple_of(3);
        }
        saved.items.push(Placement { id: "future:item".into(), zone: SavedZone::End, shown: true });
        saved.items.push(saved.items[0].clone());
        saved.items.remove(random() as usize % saved.items.len());
        saved.add_separator(SavedZone::Start, None, &defaults);
        saved.add_separator(SavedZone::End, None, &defaults);
        saved.add_space(SavedZone::Start, None, &defaults);
        saved.add_space(SavedZone::End, None, &defaults);
        saved.sanitize(&defaults);
        assert!(saved.items.iter().any(|p| p.id == "file" && p.shown), "random hide/removal attempts cannot lose File");
        for width in 200..=3840 {
            let mut measurements = measured_bar();
            measurements.managed = Some(130.0);
            let mut spec =
                spec_with_items((width as f32, 78.0, 138.0), measurements, &saved, &["save", "undo", "redo"], &quick, &["donate", "discord"]);
            for editing in [false, true] {
                crate::app_bar_layout::application::set_editing(&mut spec, editing);
                let placed = layout(&spec);
                assert_sound(&spec, &placed);
                assert_edit_cards(&spec, &placed);
                let g = placed.application.as_ref().unwrap();
                for (i, x) in &g.quick_slots {
                    let slot = placed.placements[index_of(&spec, &format!("quick:{i}"))].unwrap();
                    assert!(*x - ITEM_HEIGHT / 2.0 >= slot.x - EPS && *x + ITEM_HEIGHT / 2.0 <= slot.right() + EPS);
                }
            }
        }
    }
}

// Re-captured to give the End stack the same four-point gap before window chrome as
// between other neighbours. Regenerate with SOLIDCRAFT_RECAPTURE_APP_BAR=1 and the app_bar test
// filter. Each of the same 120 cases records field lengths and millipoint positions/widths.
#[rustfmt::skip]
const APP_BAR_ROWS: &[&[i32]] = &[
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 366000, 7000, 366000, 27000, 1, 0, 2, 190281, 366000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 0
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 366000, 7000, 366000, 27000, 1, 0, 2, 190281, 366000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 1
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 396000, 7000, 396000, 27000, 1, 0, 2, 229594, 396000, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 480 / 2
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 366000, 7000, 366000, 27000, 1, 0, 2, 190281, 366000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 4
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 366000, 7000, 366000, 27000, 1, 0, 2, 190281, 366000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 8
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 336000, 7000, 336000, 27000, 1, 0, 2, 190281, 336000, 0, 4, 366000, 4000, 392000, 30000, 0, 4, 60000, 4000, 96281, 30000], // 480 / 16
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 450000, 4000, 476000, 30000, 2, 463000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 420000, 4000, 446000, 30000, 2, 433000, 17000, 0, 0, 2, 203281, 30000, 4, 390000, 7000, 390000, 27000, 1, 0, 2, 250281, 390000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 32
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 366000, 7000, 366000, 27000, 1, 0, 2, 190281, 366000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 64
    &[0, 0, 4, 30000, 4000, 66281, 30000, 2, 48141, 17000, 4, 70281, 4000, 96281, 30000, 2, 83281, 17000, 0, 0, 0, 0, 4, 312000, 4000, 338000, 30000, 2, 325000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 4000, 308000, 30000, 2, 295000, 17000, 0, 0, 0, 4, 252000, 7000, 252000, 27000, 1, 0, 2, 100281, 252000, 0, 0, 0, 4, 30000, 4000, 66281, 30000], // 480 / 128
    &[0, 0, 4, 30000, 4000, 66281, 30000, 2, 48141, 17000, 4, 70281, 4000, 96281, 30000, 2, 83281, 17000, 0, 0, 0, 0, 4, 312000, 4000, 338000, 30000, 2, 325000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 4000, 308000, 30000, 2, 295000, 17000, 0, 0, 0, 4, 252000, 7000, 252000, 27000, 1, 0, 2, 100281, 252000, 0, 0, 0, 4, 30000, 4000, 66281, 30000], // 480 / 255
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 336000, 7000, 336000, 27000, 1, 0, 2, 190281, 336000, 0, 4, 366000, 4000, 392000, 30000, 0, 4, 60000, 4000, 96281, 30000], // 480 / 28
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 4, 366000, 4000, 392000, 30000, 2, 379000, 17000, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 336000, 7000, 336000, 27000, 1, 0, 2, 190281, 336000, 4, 366000, 4000, 392000, 30000, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 3
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 366000, 7000, 366000, 27000, 1, 0, 2, 190281, 366000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 5
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 65000, 4000, 101281, 30000, 2, 83141, 17000, 4, 105281, 4000, 131281, 30000, 2, 118281, 17000, 4, 135281, 4000, 161281, 30000, 2, 148281, 17000, 4, 165281, 4000, 191281, 30000, 2, 178281, 17000, 4, 377594, 4000, 403594, 30000, 2, 390594, 17000, 4, 60000, 9000, 61000, 25000, 0, 0, 0, 4, 195281, 9000, 196281, 25000, 0, 0, 0, 4, 407594, 7000, 427594, 27000, 2, 417594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 347594, 4000, 373594, 30000, 2, 360594, 17000, 4, 431594, 4000, 476000, 30000, 2, 453797, 17000, 0, 4, 347594, 7000, 347594, 27000, 1, 0, 2, 200281, 347594, 0, 0, 0, 4, 65000, 4000, 101281, 30000], // 480 / 6
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 366000, 7000, 366000, 27000, 1, 0, 2, 190281, 366000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 9
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 336000, 7000, 336000, 27000, 1, 0, 2, 190281, 336000, 0, 4, 366000, 4000, 392000, 30000, 0, 4, 60000, 4000, 96281, 30000], // 480 / 17
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 336000, 7000, 336000, 27000, 1, 0, 2, 190281, 336000, 0, 4, 366000, 4000, 392000, 30000, 0, 4, 60000, 4000, 96281, 30000], // 480 / 24
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 450000, 4000, 476000, 30000, 2, 463000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 420000, 4000, 446000, 30000, 2, 433000, 17000, 0, 0, 2, 203281, 30000, 4, 390000, 7000, 390000, 27000, 1, 0, 2, 250281, 390000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 33
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 426000, 4000, 452000, 30000, 2, 439000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 456000, 7000, 476000, 27000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 396000, 4000, 422000, 30000, 2, 409000, 17000, 0, 0, 0, 4, 366000, 7000, 366000, 27000, 1, 0, 2, 190281, 366000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 65
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 450000, 4000, 476000, 30000, 2, 463000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 420000, 4000, 446000, 30000, 2, 433000, 17000, 0, 0, 2, 203281, 30000, 4, 390000, 7000, 390000, 27000, 1, 0, 2, 250281, 390000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 480 / 96
    &[0, 0, 4, 30000, 4000, 66281, 30000, 2, 48141, 17000, 4, 70281, 4000, 96281, 30000, 2, 83281, 17000, 0, 0, 0, 0, 4, 312000, 4000, 338000, 30000, 2, 325000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 4000, 308000, 30000, 2, 295000, 17000, 0, 0, 0, 4, 252000, 7000, 252000, 27000, 1, 0, 2, 100281, 252000, 0, 0, 0, 4, 30000, 4000, 66281, 30000], // 480 / 129
    &[0, 0, 4, 30000, 4000, 66281, 30000, 2, 48141, 17000, 4, 70281, 4000, 96281, 30000, 2, 83281, 17000, 0, 0, 0, 0, 4, 312000, 4000, 338000, 30000, 2, 325000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 4000, 308000, 30000, 2, 295000, 17000, 0, 0, 0, 4, 252000, 7000, 252000, 27000, 1, 0, 2, 100281, 252000, 0, 0, 0, 4, 30000, 4000, 66281, 30000], // 480 / 160
    &[0, 0, 4, 30000, 4000, 66281, 30000, 2, 48141, 17000, 4, 70281, 4000, 96281, 30000, 2, 83281, 17000, 0, 0, 0, 0, 4, 312000, 4000, 338000, 30000, 2, 325000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 4000, 308000, 30000, 2, 295000, 17000, 0, 0, 0, 4, 252000, 7000, 252000, 27000, 1, 0, 2, 100281, 252000, 0, 0, 0, 4, 30000, 4000, 66281, 30000], // 480 / 188
    &[0, 0, 4, 30000, 4000, 66281, 30000, 2, 48141, 17000, 4, 70281, 4000, 96281, 30000, 2, 83281, 17000, 0, 0, 0, 0, 4, 312000, 4000, 338000, 30000, 2, 325000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 4000, 308000, 30000, 2, 295000, 17000, 0, 0, 0, 4, 252000, 7000, 252000, 27000, 1, 0, 2, 100281, 252000, 0, 0, 0, 4, 30000, 4000, 66281, 30000], // 480 / 254
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 466844, 7000, 466844, 27000, 1, 0, 2, 229594, 466844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 640 / 0
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 0, 0, 0, 0, 0, 0, 4, 436844, 4000, 462844, 30000, 2, 449844, 17000, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 406844, 7000, 406844, 27000, 1, 0, 2, 190281, 406844, 4, 436844, 4000, 462844, 30000, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 1
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 586000, 4000, 612000, 30000, 2, 599000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 616000, 7000, 636000, 27000, 2, 626000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 556000, 4000, 582000, 30000, 2, 569000, 17000, 0, 0, 0, 4, 556000, 7000, 556000, 27000, 1, 0, 2, 229594, 556000, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 640 / 2
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 448438, 4000, 474438, 30000, 2, 461438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 478438, 7000, 587594, 27000, 2, 533016, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 418438, 4000, 444438, 30000, 2, 431438, 17000, 4, 591594, 4000, 636000, 30000, 2, 613797, 17000, 0, 4, 418438, 7000, 418438, 27000, 1, 0, 2, 229594, 418438, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 640 / 4
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 4, 370250, 7000, 440844, 27000, 2, 405547, 17000, 4, 440844, 10000, 454844, 24000, 2, 447844, 17000, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 340250, 7000, 340250, 27000, 1, 0, 2, 190281, 340250, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 8
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 436844, 7000, 436844, 27000, 1, 0, 2, 229594, 436844, 0, 4, 436844, 4000, 462844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 640 / 16
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 586000, 4000, 612000, 30000, 2, 599000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 616000, 7000, 636000, 27000, 2, 626000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 556000, 4000, 582000, 30000, 2, 569000, 17000, 0, 0, 2, 203281, 30000, 4, 526000, 7000, 526000, 27000, 1, 0, 2, 340281, 526000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 32
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 65000, 4000, 101281, 30000, 2, 83141, 17000, 4, 105281, 4000, 131281, 30000, 2, 118281, 17000, 4, 135281, 4000, 161281, 30000, 2, 148281, 17000, 4, 165281, 4000, 191281, 30000, 2, 178281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 4, 60000, 9000, 61000, 25000, 0, 0, 0, 4, 195281, 9000, 196281, 25000, 0, 0, 0, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 466844, 7000, 466844, 27000, 1, 0, 2, 200281, 466844, 0, 0, 0, 4, 65000, 4000, 101281, 30000], // 640 / 64
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 65000, 4000, 101281, 30000, 2, 83141, 17000, 4, 105281, 4000, 131281, 30000, 2, 118281, 17000, 4, 135281, 4000, 161281, 30000, 2, 148281, 17000, 4, 165281, 4000, 191281, 30000, 2, 178281, 17000, 4, 358844, 4000, 384844, 30000, 2, 371844, 17000, 4, 60000, 9000, 61000, 25000, 0, 0, 0, 4, 195281, 9000, 196281, 25000, 0, 0, 0, 4, 388844, 7000, 498000, 27000, 2, 443422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 328844, 4000, 354844, 30000, 2, 341844, 17000, 0, 0, 0, 4, 328844, 7000, 328844, 27000, 1, 0, 2, 200281, 328844, 0, 0, 0, 4, 65000, 4000, 101281, 30000], // 640 / 128
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 472000, 4000, 498000, 30000, 2, 485000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 442000, 4000, 468000, 30000, 2, 455000, 17000, 0, 0, 2, 203281, 30000, 4, 412000, 7000, 412000, 27000, 1, 0, 2, 280281, 412000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 255
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 4, 384844, 7000, 410844, 27000, 2, 397844, 17000, 4, 410844, 10000, 424844, 24000, 2, 417844, 17000, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 354844, 7000, 354844, 27000, 1, 0, 2, 190281, 354844, 0, 4, 436844, 4000, 462844, 30000, 0, 4, 60000, 4000, 96281, 30000], // 640 / 28
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 586000, 4000, 612000, 30000, 2, 599000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 369000, 4000, 552000, 30000, 2, 460500, 17000, 4, 616000, 7000, 636000, 27000, 2, 626000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 556000, 4000, 582000, 30000, 2, 569000, 17000, 0, 0, 0, 4, 369000, 7000, 369000, 27000, 1, 0, 2, 229594, 369000, 4, 369000, 4000, 552000, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 640 / 3
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 0, 0, 0, 0, 0, 0, 4, 436844, 4000, 462844, 30000, 2, 449844, 17000, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 406844, 7000, 406844, 27000, 1, 0, 2, 190281, 406844, 4, 436844, 4000, 462844, 30000, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 5
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 537594, 4000, 563594, 30000, 2, 550594, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 567594, 7000, 587594, 27000, 2, 577594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 507594, 4000, 533594, 30000, 2, 520594, 17000, 4, 591594, 4000, 636000, 30000, 2, 613797, 17000, 0, 4, 507594, 7000, 507594, 27000, 1, 0, 2, 229594, 507594, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 640 / 6
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 0, 0, 0, 0, 0, 0, 4, 436844, 4000, 462844, 30000, 2, 449844, 17000, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 406844, 7000, 406844, 27000, 1, 0, 2, 190281, 406844, 4, 436844, 4000, 462844, 30000, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 9
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 0, 0, 0, 0, 0, 0, 4, 436844, 4000, 462844, 30000, 2, 449844, 17000, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 376844, 7000, 376844, 27000, 1, 0, 2, 190281, 376844, 4, 436844, 4000, 462844, 30000, 4, 406844, 4000, 432844, 30000, 0, 4, 60000, 4000, 96281, 30000], // 640 / 17
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 4, 384844, 7000, 410844, 27000, 2, 397844, 17000, 4, 410844, 10000, 424844, 24000, 2, 417844, 17000, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 354844, 7000, 354844, 27000, 1, 0, 2, 190281, 354844, 0, 4, 436844, 4000, 462844, 30000, 0, 4, 60000, 4000, 96281, 30000], // 640 / 24
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 586000, 4000, 612000, 30000, 2, 599000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 616000, 7000, 636000, 27000, 2, 626000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 556000, 4000, 582000, 30000, 2, 569000, 17000, 0, 0, 2, 203281, 30000, 4, 526000, 7000, 526000, 27000, 1, 0, 2, 340281, 526000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 33
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 496844, 4000, 522844, 30000, 2, 509844, 17000, 0, 0, 0, 0, 0, 0, 4, 436844, 4000, 462844, 30000, 2, 449844, 17000, 4, 526844, 7000, 636000, 27000, 2, 581422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 466844, 4000, 492844, 30000, 2, 479844, 17000, 0, 0, 0, 4, 406844, 7000, 406844, 27000, 1, 0, 2, 190281, 406844, 4, 436844, 4000, 462844, 30000, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 65
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 586000, 4000, 612000, 30000, 2, 599000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 616000, 7000, 636000, 27000, 2, 626000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 556000, 4000, 582000, 30000, 2, 569000, 17000, 0, 0, 2, 203281, 30000, 4, 526000, 7000, 526000, 27000, 1, 0, 2, 340281, 526000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 96
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 448000, 4000, 474000, 30000, 2, 461000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 478000, 7000, 498000, 27000, 2, 488000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 418000, 4000, 444000, 30000, 2, 431000, 17000, 0, 0, 0, 4, 388000, 7000, 388000, 27000, 1, 0, 2, 190281, 388000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 129
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 472000, 4000, 498000, 30000, 2, 485000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 442000, 4000, 468000, 30000, 2, 455000, 17000, 0, 0, 2, 203281, 30000, 4, 412000, 7000, 412000, 27000, 1, 0, 2, 280281, 412000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 160
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 472000, 4000, 498000, 30000, 2, 485000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 442000, 4000, 468000, 30000, 2, 455000, 17000, 0, 0, 2, 203281, 30000, 4, 412000, 7000, 412000, 27000, 1, 0, 2, 280281, 412000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 188
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 472000, 4000, 498000, 30000, 2, 485000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 442000, 4000, 468000, 30000, 2, 455000, 17000, 0, 0, 2, 203281, 30000, 4, 412000, 7000, 412000, 27000, 1, 0, 2, 280281, 412000, 0, 0, 0, 4, 60000, 4000, 96281, 30000], // 640 / 254
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 826844, 7000, 826844, 27000, 1, 0, 2, 229594, 826844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 0
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 639844, 4000, 822844, 30000, 2, 731344, 17000, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 639844, 7000, 639844, 27000, 1, 0, 2, 229594, 639844, 4, 639844, 4000, 822844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 1
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 946000, 4000, 972000, 30000, 2, 959000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 976000, 7000, 996000, 27000, 2, 986000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 916000, 4000, 942000, 30000, 2, 929000, 17000, 0, 0, 0, 4, 916000, 7000, 916000, 27000, 1, 0, 2, 229594, 916000, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 2
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 808438, 4000, 834438, 30000, 2, 821438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 838438, 7000, 947594, 27000, 2, 893016, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 778438, 4000, 804438, 30000, 2, 791438, 17000, 4, 951594, 4000, 996000, 30000, 2, 973797, 17000, 0, 4, 778438, 7000, 778438, 27000, 1, 0, 2, 229594, 778438, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 4
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 4, 634531, 7000, 704250, 27000, 2, 669391, 17000, 4, 704250, 10000, 718250, 24000, 2, 711250, 17000, 4, 730250, 7000, 800844, 27000, 2, 765547, 17000, 4, 800844, 10000, 814844, 24000, 2, 807844, 17000, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 634531, 7000, 634531, 27000, 1, 0, 2, 229594, 634531, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 8
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 796844, 7000, 796844, 27000, 1, 0, 2, 229594, 796844, 0, 4, 796844, 4000, 822844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 16
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 0, 0, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 2, 237594, 30000, 4, 826844, 7000, 826844, 27000, 1, 0, 2, 379594, 826844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 32
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 413031, 9000, 414031, 25000, 0, 0, 0, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 826844, 7000, 826844, 27000, 1, 0, 2, 418031, 826844, 0, 0, 0, 4, 282750, 4000, 319031, 30000], // 1000 / 64
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 718844, 4000, 744844, 30000, 2, 731844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 748844, 7000, 858000, 27000, 2, 803422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 688844, 4000, 714844, 30000, 2, 701844, 17000, 0, 0, 0, 4, 688844, 7000, 688844, 27000, 1, 0, 2, 229594, 688844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 128
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 808000, 4000, 834000, 30000, 2, 821000, 17000, 0, 0, 0, 0, 0, 0, 4, 591000, 4000, 774000, 30000, 2, 682500, 17000, 4, 838000, 7000, 858000, 27000, 2, 848000, 17000, 0, 0, 0, 0, 4, 509000, 7000, 535000, 27000, 2, 522000, 17000, 4, 535000, 10000, 549000, 24000, 2, 542000, 17000, 4, 778000, 4000, 804000, 30000, 2, 791000, 17000, 0, 0, 2, 203281, 30000, 4, 479000, 7000, 479000, 27000, 1, 0, 2, 340281, 479000, 4, 591000, 4000, 774000, 30000, 4, 561000, 4000, 587000, 30000, 0, 4, 60000, 4000, 96281, 30000], // 1000 / 255
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 808438, 4000, 834438, 30000, 2, 821438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 838438, 7000, 947594, 27000, 2, 893016, 17000, 4, 556125, 7000, 625844, 27000, 2, 590984, 17000, 4, 625844, 10000, 639844, 24000, 2, 632844, 17000, 4, 651844, 7000, 722438, 27000, 2, 687141, 17000, 4, 722438, 10000, 736438, 24000, 2, 729438, 17000, 4, 778438, 4000, 804438, 30000, 2, 791438, 17000, 4, 951594, 4000, 996000, 30000, 2, 973797, 17000, 0, 4, 556125, 7000, 556125, 27000, 1, 0, 2, 229594, 556125, 0, 4, 748438, 4000, 774438, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 28
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 946000, 4000, 972000, 30000, 2, 959000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 729000, 4000, 912000, 30000, 2, 820500, 17000, 4, 976000, 7000, 996000, 27000, 2, 986000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 916000, 4000, 942000, 30000, 2, 929000, 17000, 0, 0, 0, 4, 729000, 7000, 729000, 27000, 1, 0, 2, 229594, 729000, 4, 729000, 4000, 912000, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 3
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 808438, 4000, 834438, 30000, 2, 821438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 591438, 4000, 774438, 30000, 2, 682938, 17000, 4, 838438, 7000, 947594, 27000, 2, 893016, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 778438, 4000, 804438, 30000, 2, 791438, 17000, 4, 951594, 4000, 996000, 30000, 2, 973797, 17000, 0, 4, 591438, 7000, 591438, 27000, 1, 0, 2, 229594, 591438, 4, 591438, 4000, 774438, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 5
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 897594, 4000, 923594, 30000, 2, 910594, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 927594, 7000, 947594, 27000, 2, 937594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 867594, 4000, 893594, 30000, 2, 880594, 17000, 4, 951594, 4000, 996000, 30000, 2, 973797, 17000, 0, 4, 867594, 7000, 867594, 27000, 1, 0, 2, 229594, 867594, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 6
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 639844, 4000, 822844, 30000, 2, 731344, 17000, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 4, 447531, 7000, 517250, 27000, 2, 482391, 17000, 4, 517250, 10000, 531250, 24000, 2, 524250, 17000, 4, 543250, 7000, 613844, 27000, 2, 578547, 17000, 4, 613844, 10000, 627844, 24000, 2, 620844, 17000, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 447531, 7000, 447531, 27000, 1, 0, 2, 229594, 447531, 4, 639844, 4000, 822844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 9
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 639844, 4000, 822844, 30000, 2, 731344, 17000, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 609844, 7000, 609844, 27000, 1, 0, 2, 229594, 609844, 4, 639844, 4000, 822844, 30000, 4, 609844, 4000, 635844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 17
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 4, 604531, 7000, 674250, 27000, 2, 639391, 17000, 4, 674250, 10000, 688250, 24000, 2, 681250, 17000, 4, 700250, 7000, 770844, 27000, 2, 735547, 17000, 4, 770844, 10000, 784844, 24000, 2, 777844, 17000, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 604531, 7000, 604531, 27000, 1, 0, 2, 229594, 604531, 0, 4, 796844, 4000, 822844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 24
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 4, 639844, 4000, 822844, 30000, 2, 731344, 17000, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 2, 237594, 30000, 4, 639844, 7000, 639844, 27000, 1, 0, 2, 379594, 639844, 4, 639844, 4000, 822844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 33
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 413031, 9000, 414031, 25000, 0, 4, 639844, 4000, 822844, 30000, 2, 731344, 17000, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 0, 4, 639844, 7000, 639844, 27000, 1, 0, 2, 418031, 639844, 4, 639844, 4000, 822844, 30000, 0, 0, 4, 282750, 4000, 319031, 30000], // 1000 / 65
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 856844, 4000, 882844, 30000, 2, 869844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 563031, 9000, 564031, 25000, 0, 0, 0, 4, 886844, 7000, 996000, 27000, 2, 941422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 826844, 4000, 852844, 30000, 2, 839844, 17000, 0, 0, 2, 426031, 30000, 4, 826844, 7000, 826844, 27000, 1, 0, 2, 568031, 826844, 0, 0, 0, 4, 282750, 4000, 319031, 30000], // 1000 / 96
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 718844, 4000, 744844, 30000, 2, 731844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 501844, 4000, 684844, 30000, 2, 593344, 17000, 4, 748844, 7000, 858000, 27000, 2, 803422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 688844, 4000, 714844, 30000, 2, 701844, 17000, 0, 0, 0, 4, 501844, 7000, 501844, 27000, 1, 0, 2, 229594, 501844, 4, 501844, 4000, 684844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 129
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 718844, 4000, 744844, 30000, 2, 731844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 0, 0, 4, 748844, 7000, 858000, 27000, 2, 803422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 688844, 4000, 714844, 30000, 2, 701844, 17000, 0, 0, 2, 237594, 30000, 4, 688844, 7000, 688844, 27000, 1, 0, 2, 379594, 688844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1000 / 160
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 60000, 4000, 96281, 30000, 2, 78141, 17000, 4, 100281, 4000, 126281, 30000, 2, 113281, 17000, 4, 130281, 4000, 156281, 30000, 2, 143281, 17000, 4, 160281, 4000, 186281, 30000, 2, 173281, 17000, 4, 718844, 4000, 744844, 30000, 2, 731844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 748844, 7000, 858000, 27000, 2, 803422, 17000, 4, 510250, 7000, 536250, 27000, 2, 523250, 17000, 4, 536250, 10000, 550250, 24000, 2, 543250, 17000, 4, 562250, 7000, 632844, 27000, 2, 597547, 17000, 4, 632844, 10000, 646844, 24000, 2, 639844, 17000, 4, 688844, 4000, 714844, 30000, 2, 701844, 17000, 0, 0, 2, 203281, 30000, 4, 480250, 7000, 480250, 27000, 1, 0, 2, 340281, 480250, 0, 4, 658844, 4000, 684844, 30000, 0, 4, 60000, 4000, 96281, 30000], // 1000 / 188
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 65000, 4000, 101281, 30000, 2, 83141, 17000, 4, 105281, 4000, 131281, 30000, 2, 118281, 17000, 4, 135281, 4000, 161281, 30000, 2, 148281, 17000, 4, 165281, 4000, 191281, 30000, 2, 178281, 17000, 4, 759594, 4000, 785594, 30000, 2, 772594, 17000, 4, 60000, 9000, 61000, 25000, 0, 0, 0, 4, 345281, 9000, 346281, 25000, 0, 0, 0, 4, 789594, 7000, 809594, 27000, 2, 799594, 17000, 4, 507281, 7000, 577000, 27000, 2, 542141, 17000, 4, 577000, 10000, 591000, 24000, 2, 584000, 17000, 4, 603000, 7000, 673594, 27000, 2, 638297, 17000, 4, 673594, 10000, 687594, 24000, 2, 680594, 17000, 4, 729594, 4000, 755594, 30000, 2, 742594, 17000, 4, 813594, 4000, 858000, 30000, 2, 835797, 17000, 2, 208281, 30000, 4, 507281, 7000, 507281, 27000, 1, 0, 2, 350281, 507281, 0, 4, 699594, 4000, 725594, 30000, 0, 4, 65000, 4000, 101281, 30000], // 1000 / 254
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 1106844, 7000, 1106844, 27000, 1, 0, 2, 229594, 1106844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 0
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 919844, 4000, 1102844, 30000, 2, 1011344, 17000, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 919844, 7000, 919844, 27000, 1, 0, 2, 229594, 919844, 4, 919844, 4000, 1102844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 1
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1226000, 4000, 1252000, 30000, 2, 1239000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1256000, 7000, 1276000, 27000, 2, 1266000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1196000, 4000, 1222000, 30000, 2, 1209000, 17000, 0, 0, 0, 4, 1196000, 7000, 1196000, 27000, 1, 0, 2, 229594, 1196000, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 2
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1088438, 4000, 1114438, 30000, 2, 1101438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1118438, 7000, 1227594, 27000, 2, 1173016, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1058438, 4000, 1084438, 30000, 2, 1071438, 17000, 4, 1231594, 4000, 1276000, 30000, 2, 1253797, 17000, 0, 4, 1058438, 7000, 1058438, 27000, 1, 0, 2, 229594, 1058438, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 4
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 4, 914531, 7000, 984250, 27000, 2, 949391, 17000, 4, 984250, 10000, 998250, 24000, 2, 991250, 17000, 4, 1010250, 7000, 1080844, 27000, 2, 1045547, 17000, 4, 1080844, 10000, 1094844, 24000, 2, 1087844, 17000, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 914531, 7000, 914531, 27000, 1, 0, 2, 229594, 914531, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 8
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 1076844, 7000, 1076844, 27000, 1, 0, 2, 229594, 1076844, 0, 4, 1076844, 4000, 1102844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 16
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 0, 0, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 2, 237594, 30000, 4, 1106844, 7000, 1106844, 27000, 1, 0, 2, 379594, 1106844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 32
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 413031, 9000, 414031, 25000, 0, 0, 0, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 1106844, 7000, 1106844, 27000, 1, 0, 2, 418031, 1106844, 0, 0, 0, 4, 282750, 4000, 319031, 30000], // 1280 / 64
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 998844, 4000, 1024844, 30000, 2, 1011844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1028844, 7000, 1138000, 27000, 2, 1083422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 968844, 4000, 994844, 30000, 2, 981844, 17000, 0, 0, 0, 4, 968844, 7000, 968844, 27000, 1, 0, 2, 229594, 968844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 128
    &[4, 30000, 4000, 56000, 30000, 2, 43000, 17000, 4, 65000, 4000, 101281, 30000, 2, 83141, 17000, 4, 105281, 4000, 131281, 30000, 2, 118281, 17000, 4, 135281, 4000, 161281, 30000, 2, 148281, 17000, 4, 165281, 4000, 191281, 30000, 2, 178281, 17000, 4, 1039594, 4000, 1065594, 30000, 2, 1052594, 17000, 4, 60000, 9000, 61000, 25000, 0, 0, 0, 4, 345281, 9000, 346281, 25000, 0, 4, 822594, 4000, 1005594, 30000, 2, 914094, 17000, 4, 1069594, 7000, 1089594, 27000, 2, 1079594, 17000, 4, 600281, 7000, 670000, 27000, 2, 635141, 17000, 4, 670000, 10000, 684000, 24000, 2, 677000, 17000, 4, 696000, 7000, 766594, 27000, 2, 731297, 17000, 4, 766594, 10000, 780594, 24000, 2, 773594, 17000, 4, 1009594, 4000, 1035594, 30000, 2, 1022594, 17000, 4, 1093594, 4000, 1138000, 30000, 2, 1115797, 17000, 2, 208281, 30000, 4, 600281, 7000, 600281, 27000, 1, 0, 2, 350281, 600281, 4, 822594, 4000, 1005594, 30000, 4, 792594, 4000, 818594, 30000, 0, 4, 65000, 4000, 101281, 30000], // 1280 / 255
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1088438, 4000, 1114438, 30000, 2, 1101438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1118438, 7000, 1227594, 27000, 2, 1173016, 17000, 4, 836125, 7000, 905844, 27000, 2, 870984, 17000, 4, 905844, 10000, 919844, 24000, 2, 912844, 17000, 4, 931844, 7000, 1002438, 27000, 2, 967141, 17000, 4, 1002438, 10000, 1016438, 24000, 2, 1009438, 17000, 4, 1058438, 4000, 1084438, 30000, 2, 1071438, 17000, 4, 1231594, 4000, 1276000, 30000, 2, 1253797, 17000, 0, 4, 836125, 7000, 836125, 27000, 1, 0, 2, 229594, 836125, 0, 4, 1028438, 4000, 1054438, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 28
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1226000, 4000, 1252000, 30000, 2, 1239000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 1009000, 4000, 1192000, 30000, 2, 1100500, 17000, 4, 1256000, 7000, 1276000, 27000, 2, 1266000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1196000, 4000, 1222000, 30000, 2, 1209000, 17000, 0, 0, 0, 4, 1009000, 7000, 1009000, 27000, 1, 0, 2, 229594, 1009000, 4, 1009000, 4000, 1192000, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 3
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1088438, 4000, 1114438, 30000, 2, 1101438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 871438, 4000, 1054438, 30000, 2, 962938, 17000, 4, 1118438, 7000, 1227594, 27000, 2, 1173016, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1058438, 4000, 1084438, 30000, 2, 1071438, 17000, 4, 1231594, 4000, 1276000, 30000, 2, 1253797, 17000, 0, 4, 871438, 7000, 871438, 27000, 1, 0, 2, 229594, 871438, 4, 871438, 4000, 1054438, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 5
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1177594, 4000, 1203594, 30000, 2, 1190594, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1207594, 7000, 1227594, 27000, 2, 1217594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1147594, 4000, 1173594, 30000, 2, 1160594, 17000, 4, 1231594, 4000, 1276000, 30000, 2, 1253797, 17000, 0, 4, 1147594, 7000, 1147594, 27000, 1, 0, 2, 229594, 1147594, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 6
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 919844, 4000, 1102844, 30000, 2, 1011344, 17000, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 4, 727531, 7000, 797250, 27000, 2, 762391, 17000, 4, 797250, 10000, 811250, 24000, 2, 804250, 17000, 4, 823250, 7000, 893844, 27000, 2, 858547, 17000, 4, 893844, 10000, 907844, 24000, 2, 900844, 17000, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 727531, 7000, 727531, 27000, 1, 0, 2, 229594, 727531, 4, 919844, 4000, 1102844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 9
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 919844, 4000, 1102844, 30000, 2, 1011344, 17000, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 889844, 7000, 889844, 27000, 1, 0, 2, 229594, 889844, 4, 919844, 4000, 1102844, 30000, 4, 889844, 4000, 915844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 17
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 4, 884531, 7000, 954250, 27000, 2, 919391, 17000, 4, 954250, 10000, 968250, 24000, 2, 961250, 17000, 4, 980250, 7000, 1050844, 27000, 2, 1015547, 17000, 4, 1050844, 10000, 1064844, 24000, 2, 1057844, 17000, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 884531, 7000, 884531, 27000, 1, 0, 2, 229594, 884531, 0, 4, 1076844, 4000, 1102844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 24
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 4, 919844, 4000, 1102844, 30000, 2, 1011344, 17000, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 2, 237594, 30000, 4, 919844, 7000, 919844, 27000, 1, 0, 2, 379594, 919844, 4, 919844, 4000, 1102844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 33
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 413031, 9000, 414031, 25000, 0, 4, 919844, 4000, 1102844, 30000, 2, 1011344, 17000, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 0, 4, 919844, 7000, 919844, 27000, 1, 0, 2, 418031, 919844, 4, 919844, 4000, 1102844, 30000, 0, 0, 4, 282750, 4000, 319031, 30000], // 1280 / 65
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1136844, 4000, 1162844, 30000, 2, 1149844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 563031, 9000, 564031, 25000, 0, 0, 0, 4, 1166844, 7000, 1276000, 27000, 2, 1221422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1106844, 4000, 1132844, 30000, 2, 1119844, 17000, 0, 0, 2, 426031, 30000, 4, 1106844, 7000, 1106844, 27000, 1, 0, 2, 568031, 1106844, 0, 0, 0, 4, 282750, 4000, 319031, 30000], // 1280 / 96
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 998844, 4000, 1024844, 30000, 2, 1011844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 781844, 4000, 964844, 30000, 2, 873344, 17000, 4, 1028844, 7000, 1138000, 27000, 2, 1083422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 968844, 4000, 994844, 30000, 2, 981844, 17000, 0, 0, 0, 4, 781844, 7000, 781844, 27000, 1, 0, 2, 229594, 781844, 4, 781844, 4000, 964844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 129
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 998844, 4000, 1024844, 30000, 2, 1011844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 0, 0, 4, 1028844, 7000, 1138000, 27000, 2, 1083422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 968844, 4000, 994844, 30000, 2, 981844, 17000, 0, 0, 2, 237594, 30000, 4, 968844, 7000, 968844, 27000, 1, 0, 2, 379594, 968844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 160
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 950438, 4000, 976438, 30000, 2, 963438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 0, 0, 4, 980438, 7000, 1089594, 27000, 2, 1035016, 17000, 4, 698125, 7000, 767844, 27000, 2, 732984, 17000, 4, 767844, 10000, 781844, 24000, 2, 774844, 17000, 4, 793844, 7000, 864438, 27000, 2, 829141, 17000, 4, 864438, 10000, 878438, 24000, 2, 871438, 17000, 4, 920438, 4000, 946438, 30000, 2, 933438, 17000, 4, 1093594, 4000, 1138000, 30000, 2, 1115797, 17000, 2, 237594, 30000, 4, 698125, 7000, 698125, 27000, 1, 0, 2, 379594, 698125, 0, 4, 890438, 4000, 916438, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1280 / 188
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1039594, 4000, 1065594, 30000, 2, 1052594, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 563031, 9000, 564031, 25000, 0, 0, 0, 4, 1069594, 7000, 1089594, 27000, 2, 1079594, 17000, 4, 787281, 7000, 857000, 27000, 2, 822141, 17000, 4, 857000, 10000, 871000, 24000, 2, 864000, 17000, 4, 883000, 7000, 953594, 27000, 2, 918297, 17000, 4, 953594, 10000, 967594, 24000, 2, 960594, 17000, 4, 1009594, 4000, 1035594, 30000, 2, 1022594, 17000, 4, 1093594, 4000, 1138000, 30000, 2, 1115797, 17000, 2, 426031, 30000, 4, 787281, 7000, 787281, 27000, 1, 0, 2, 568031, 787281, 0, 4, 979594, 4000, 1005594, 30000, 0, 4, 282750, 4000, 319031, 30000], // 1280 / 254
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1746844, 7000, 1746844, 27000, 1, 0, 2, 229594, 1746844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 0
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 1559844, 4000, 1742844, 30000, 2, 1651344, 17000, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1559844, 7000, 1559844, 27000, 1, 0, 2, 229594, 1559844, 4, 1559844, 4000, 1742844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 1
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1866000, 4000, 1892000, 30000, 2, 1879000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1896000, 7000, 1916000, 27000, 2, 1906000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1836000, 4000, 1862000, 30000, 2, 1849000, 17000, 0, 0, 0, 4, 1836000, 7000, 1836000, 27000, 1, 0, 2, 229594, 1836000, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 2
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1728438, 4000, 1754438, 30000, 2, 1741438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1758438, 7000, 1867594, 27000, 2, 1813016, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1698438, 4000, 1724438, 30000, 2, 1711438, 17000, 4, 1871594, 4000, 1916000, 30000, 2, 1893797, 17000, 0, 4, 1698438, 7000, 1698438, 27000, 1, 0, 2, 229594, 1698438, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 4
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 4, 1554531, 7000, 1624250, 27000, 2, 1589391, 17000, 4, 1624250, 10000, 1638250, 24000, 2, 1631250, 17000, 4, 1650250, 7000, 1720844, 27000, 2, 1685547, 17000, 4, 1720844, 10000, 1734844, 24000, 2, 1727844, 17000, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1554531, 7000, 1554531, 27000, 1, 0, 2, 229594, 1554531, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 8
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1716844, 7000, 1716844, 27000, 1, 0, 2, 229594, 1716844, 0, 4, 1716844, 4000, 1742844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 16
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 0, 0, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 2, 237594, 30000, 4, 1746844, 7000, 1746844, 27000, 1, 0, 2, 379594, 1746844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 32
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 413031, 9000, 414031, 25000, 0, 0, 0, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1746844, 7000, 1746844, 27000, 1, 0, 2, 418031, 1746844, 0, 0, 0, 4, 282750, 4000, 319031, 30000], // 1920 / 64
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1638844, 4000, 1664844, 30000, 2, 1651844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1668844, 7000, 1778000, 27000, 2, 1723422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1608844, 4000, 1634844, 30000, 2, 1621844, 17000, 0, 0, 0, 4, 1608844, 7000, 1608844, 27000, 1, 0, 2, 229594, 1608844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 128
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1679594, 4000, 1705594, 30000, 2, 1692594, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 563031, 9000, 564031, 25000, 0, 4, 1462594, 4000, 1645594, 30000, 2, 1554094, 17000, 4, 1709594, 7000, 1729594, 27000, 2, 1719594, 17000, 4, 1240281, 7000, 1310000, 27000, 2, 1275141, 17000, 4, 1310000, 10000, 1324000, 24000, 2, 1317000, 17000, 4, 1336000, 7000, 1406594, 27000, 2, 1371297, 17000, 4, 1406594, 10000, 1420594, 24000, 2, 1413594, 17000, 4, 1649594, 4000, 1675594, 30000, 2, 1662594, 17000, 4, 1733594, 4000, 1778000, 30000, 2, 1755797, 17000, 2, 426031, 30000, 4, 1240281, 7000, 1240281, 27000, 1, 0, 2, 568031, 1240281, 4, 1462594, 4000, 1645594, 30000, 4, 1432594, 4000, 1458594, 30000, 0, 4, 282750, 4000, 319031, 30000], // 1920 / 255
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1728438, 4000, 1754438, 30000, 2, 1741438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1758438, 7000, 1867594, 27000, 2, 1813016, 17000, 4, 1476125, 7000, 1545844, 27000, 2, 1510984, 17000, 4, 1545844, 10000, 1559844, 24000, 2, 1552844, 17000, 4, 1571844, 7000, 1642438, 27000, 2, 1607141, 17000, 4, 1642438, 10000, 1656438, 24000, 2, 1649438, 17000, 4, 1698438, 4000, 1724438, 30000, 2, 1711438, 17000, 4, 1871594, 4000, 1916000, 30000, 2, 1893797, 17000, 0, 4, 1476125, 7000, 1476125, 27000, 1, 0, 2, 229594, 1476125, 0, 4, 1668438, 4000, 1694438, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 28
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1866000, 4000, 1892000, 30000, 2, 1879000, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 1649000, 4000, 1832000, 30000, 2, 1740500, 17000, 4, 1896000, 7000, 1916000, 27000, 2, 1906000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1836000, 4000, 1862000, 30000, 2, 1849000, 17000, 0, 0, 0, 4, 1649000, 7000, 1649000, 27000, 1, 0, 2, 229594, 1649000, 4, 1649000, 4000, 1832000, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 3
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1728438, 4000, 1754438, 30000, 2, 1741438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 1511438, 4000, 1694438, 30000, 2, 1602938, 17000, 4, 1758438, 7000, 1867594, 27000, 2, 1813016, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1698438, 4000, 1724438, 30000, 2, 1711438, 17000, 4, 1871594, 4000, 1916000, 30000, 2, 1893797, 17000, 0, 4, 1511438, 7000, 1511438, 27000, 1, 0, 2, 229594, 1511438, 4, 1511438, 4000, 1694438, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 5
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1817594, 4000, 1843594, 30000, 2, 1830594, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1847594, 7000, 1867594, 27000, 2, 1857594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1787594, 4000, 1813594, 30000, 2, 1800594, 17000, 4, 1871594, 4000, 1916000, 30000, 2, 1893797, 17000, 0, 4, 1787594, 7000, 1787594, 27000, 1, 0, 2, 229594, 1787594, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 6
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 1559844, 4000, 1742844, 30000, 2, 1651344, 17000, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 4, 1367531, 7000, 1437250, 27000, 2, 1402391, 17000, 4, 1437250, 10000, 1451250, 24000, 2, 1444250, 17000, 4, 1463250, 7000, 1533844, 27000, 2, 1498547, 17000, 4, 1533844, 10000, 1547844, 24000, 2, 1540844, 17000, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1367531, 7000, 1367531, 27000, 1, 0, 2, 229594, 1367531, 4, 1559844, 4000, 1742844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 9
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 1559844, 4000, 1742844, 30000, 2, 1651344, 17000, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1529844, 7000, 1529844, 27000, 1, 0, 2, 229594, 1529844, 4, 1559844, 4000, 1742844, 30000, 4, 1529844, 4000, 1555844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 17
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 0, 0, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 4, 1524531, 7000, 1594250, 27000, 2, 1559391, 17000, 4, 1594250, 10000, 1608250, 24000, 2, 1601250, 17000, 4, 1620250, 7000, 1690844, 27000, 2, 1655547, 17000, 4, 1690844, 10000, 1704844, 24000, 2, 1697844, 17000, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1524531, 7000, 1524531, 27000, 1, 0, 2, 229594, 1524531, 0, 4, 1716844, 4000, 1742844, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 24
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 4, 1559844, 4000, 1742844, 30000, 2, 1651344, 17000, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 2, 237594, 30000, 4, 1559844, 7000, 1559844, 27000, 1, 0, 2, 379594, 1559844, 4, 1559844, 4000, 1742844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 33
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 413031, 9000, 414031, 25000, 0, 4, 1559844, 4000, 1742844, 30000, 2, 1651344, 17000, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 0, 4, 1559844, 7000, 1559844, 27000, 1, 0, 2, 418031, 1559844, 4, 1559844, 4000, 1742844, 30000, 0, 0, 4, 282750, 4000, 319031, 30000], // 1920 / 65
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1776844, 4000, 1802844, 30000, 2, 1789844, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 563031, 9000, 564031, 25000, 0, 0, 0, 4, 1806844, 7000, 1916000, 27000, 2, 1861422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1746844, 4000, 1772844, 30000, 2, 1759844, 17000, 0, 0, 2, 426031, 30000, 4, 1746844, 7000, 1746844, 27000, 1, 0, 2, 568031, 1746844, 0, 0, 0, 4, 282750, 4000, 319031, 30000], // 1920 / 96
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1638844, 4000, 1664844, 30000, 2, 1651844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 224594, 9000, 225594, 25000, 0, 4, 1421844, 4000, 1604844, 30000, 2, 1513344, 17000, 4, 1668844, 7000, 1778000, 27000, 2, 1723422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1608844, 4000, 1634844, 30000, 2, 1621844, 17000, 0, 0, 0, 4, 1421844, 7000, 1421844, 27000, 1, 0, 2, 229594, 1421844, 4, 1421844, 4000, 1604844, 30000, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 129
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1638844, 4000, 1664844, 30000, 2, 1651844, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 0, 0, 4, 1668844, 7000, 1778000, 27000, 2, 1723422, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1608844, 4000, 1634844, 30000, 2, 1621844, 17000, 0, 0, 2, 237594, 30000, 4, 1608844, 7000, 1608844, 27000, 1, 0, 2, 379594, 1608844, 0, 0, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 160
    &[4, 59313, 4000, 85313, 30000, 2, 72313, 17000, 4, 94313, 4000, 130594, 30000, 2, 112453, 17000, 4, 134594, 4000, 160594, 30000, 2, 147594, 17000, 4, 164594, 4000, 190594, 30000, 2, 177594, 17000, 4, 194594, 4000, 220594, 30000, 2, 207594, 17000, 4, 1590438, 4000, 1616438, 30000, 2, 1603438, 17000, 4, 89313, 9000, 90313, 25000, 0, 0, 0, 4, 374594, 9000, 375594, 25000, 0, 0, 0, 4, 1620438, 7000, 1729594, 27000, 2, 1675016, 17000, 4, 1338125, 7000, 1407844, 27000, 2, 1372984, 17000, 4, 1407844, 10000, 1421844, 24000, 2, 1414844, 17000, 4, 1433844, 7000, 1504438, 27000, 2, 1469141, 17000, 4, 1504438, 10000, 1518438, 24000, 2, 1511438, 17000, 4, 1560438, 4000, 1586438, 30000, 2, 1573438, 17000, 4, 1733594, 4000, 1778000, 30000, 2, 1755797, 17000, 2, 237594, 30000, 4, 1338125, 7000, 1338125, 27000, 1, 0, 2, 379594, 1338125, 0, 4, 1530438, 4000, 1556438, 30000, 0, 4, 94313, 4000, 130594, 30000], // 1920 / 188
    &[4, 247750, 4000, 273750, 30000, 2, 260750, 17000, 4, 282750, 4000, 319031, 30000, 2, 300891, 17000, 4, 323031, 4000, 349031, 30000, 2, 336031, 17000, 4, 353031, 4000, 379031, 30000, 2, 366031, 17000, 4, 383031, 4000, 409031, 30000, 2, 396031, 17000, 4, 1679594, 4000, 1705594, 30000, 2, 1692594, 17000, 4, 277750, 9000, 278750, 25000, 0, 0, 0, 4, 563031, 9000, 564031, 25000, 0, 0, 0, 4, 1709594, 7000, 1729594, 27000, 2, 1719594, 17000, 4, 1427281, 7000, 1497000, 27000, 2, 1462141, 17000, 4, 1497000, 10000, 1511000, 24000, 2, 1504000, 17000, 4, 1523000, 7000, 1593594, 27000, 2, 1558297, 17000, 4, 1593594, 10000, 1607594, 24000, 2, 1600594, 17000, 4, 1649594, 4000, 1675594, 30000, 2, 1662594, 17000, 4, 1733594, 4000, 1778000, 30000, 2, 1755797, 17000, 2, 426031, 30000, 4, 1427281, 7000, 1427281, 27000, 1, 0, 2, 568031, 1427281, 0, 4, 1619594, 4000, 1645594, 30000, 0, 4, 282750, 4000, 319031, 30000], // 1920 / 254
];

#[test]
fn clear_view_tabs_distinguish_the_four_placements() {
    let ground = VIEWS[0];
    let hidden = VIEWS[1];
    let card = ViewTab { on_screen: true, ..hidden };
    let elsewhere = ViewTab { to_window: Some(2), window_name: Some("Inspection"), window_number: Some(3), ..hidden };
    assert_eq!(view_look(&ground), ViewLook::Ground);
    assert_eq!(view_look(&card), ViewLook::Card);
    assert_eq!(view_look(&hidden), ViewLook::Hidden);
    assert_eq!(view_look(&elsewhere), ViewLook::Elsewhere);
    assert_eq!(window_badge(&elsewhere), "3");
    assert!(!remote_name_hidden(&elsewhere, CollapseStage::Full));
    assert!(remote_name_hidden(&elsewhere, CollapseStage::WindowNumber));
}

#[test]
fn clear_view_tabs_collapse_in_order_with_and_without_another_window() {
    for count in 2..=12 {
        for elsewhere in [false, true] {
            let focused = 120.0;
            let icon_tabs = (count - 1) as f32 * 30.0;
            let idle_names = (count - 1 - usize::from(elsewhere)) as f32 * 30.0;
            let window_names = if elsewhere { 24.0 } else { 0.0 };
            let icons = focused + icon_tabs + 24.0;
            let window_number = icons + idle_names;
            let full = window_number + window_names;
            let widths = [full, window_number, icons, focused + 28.0];
            assert_eq!(collapse_stage(widths, full), CollapseStage::Full);
            assert_eq!(collapse_stage(widths, full - 0.25), if elsewhere { CollapseStage::WindowNumber } else { CollapseStage::Icons });
            assert_eq!(collapse_stage(widths, icons - 0.25), CollapseStage::Overflow);
        }
    }
}

#[test]
fn elsewhere_view_has_only_a_return_button_on_hover() {
    let mut rig = TabsRig::with(Some(true));
    rig.detached = true;
    rig.clear = true;
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("t:vclose:0").is_some());
    assert!(solidcraft_ui_registry::rect_of("t:vclose:1").is_none());
    assert!(solidcraft_ui_registry::rect_of("t:view:1:return").is_none());
    assert!(solidcraft_ui_registry::rect_of("t:view:1:window").is_some());
    let elsewhere = rig.at("t:view:1");
    rig.frame(vec![egui::Event::PointerMoved(elsewhere)]);
    assert!(solidcraft_ui_registry::rect_of("t:view:1:window").is_none());
    assert!(solidcraft_ui_registry::rect_of("t:vclose:1").is_none());
    assert_eq!(rig.press(rig.at("t:view:1:return"), egui::PointerButton::Primary), vec![TabAction::ReturnView(1)]);
}

fn layout_test_data(visible: usize, window_count: usize) -> crate::view_layout_menu::LayoutMenu {
    use crate::view_layout_menu::{LayoutMenu, LayoutText, LayoutView, LayoutWindow};
    LayoutMenu {
        text: LayoutText::default(),
        saved: Vec::new(),
        matching_saved: None,
        design: "One".into(),
        window: 0,
        native: true,
        visible,
        link: "off".into(),
        can_link: true,
        clean: false,
        windows: (0..window_count as u32)
            .map(|id| LayoutWindow {
                id,
                name: format!("Window {}", id + 1),
                display: None,
                views: vec![LayoutView { id: u64::from(id) + 10, name: "Model".into(), kind: "model".into(), accent: egui::Color32::BLUE }],
            })
            .collect(),
    }
}

#[test]
fn layout_glyph_has_a_ground_and_cells_for_each_preset() {
    let interior = Rect::from_min_size(Pos2::ZERO, vec2(14.0, 14.0));
    for (key, count) in [("single", 1), ("side_by_side", 2), ("stacked", 2), ("large_and_two", 3), ("grid", 4)] {
        let cells = layout_cells(interior, 1.0, key);
        assert_eq!(cells.len(), count);
        assert_eq!(cells[0].left_top(), interior.left_top());
        assert!(cells.iter().all(|cell| cell.is_positive() && interior.contains_rect(*cell)));
        for i in 0..cells.len() {
            assert!(cells.iter().skip(i + 1).all(|other| !cells[i].intersects(*other)));
        }
    }
    assert_eq!(layout_cells(interior, 1.0, "side_by_side")[1].top(), interior.top());
    assert_eq!(layout_cells(interior, 1.0, "stacked")[1].left(), interior.left());
}

#[test]
fn layout_menu_shows_sections_for_current_window_and_views() {
    use crate::view_layout_menu::{Sections, sections};
    assert_eq!(sections(&layout_test_data(1, 1)), Sections { equal: false, windows: false, link: false, detach: true });
    assert_eq!(sections(&layout_test_data(2, 1)), Sections { equal: true, windows: false, link: true, detach: true });
    assert_eq!(sections(&layout_test_data(1, 2)), Sections { equal: false, windows: true, link: false, detach: false });
    assert_eq!(sections(&layout_test_data(2, 2)), Sections { equal: true, windows: true, link: true, detach: false });
    let mut web = layout_test_data(1, 1);
    web.native = false;
    assert!(!sections(&web).detach);
}

#[test]
fn adding_a_window_expands_the_layout_menu_to_its_extra_rows() {
    let mut rig = TabsRig::with(Some(true));
    rig.clear = true;
    rig.layout = Some((1, 1));
    rig.screen_height = 900.0;
    rig.frame(Vec::new());
    rig.press(rig.at("view:layout"), egui::PointerButton::Primary);
    rig.frame(Vec::new());

    rig.layout = Some((2, 2));
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    let reset = rig.at("menu:layout.reset_cameras");
    assert_eq!(rig.press(reset, egui::PointerButton::Primary), vec![TabAction::LayoutMenu { id: "layout.reset_cameras".into() }]);
}

#[test]
fn layout_menu_keeps_glyph_choices_open_and_escape_restores_the_button() {
    let mut rig = TabsRig::with(Some(true));
    rig.clear = true;
    rig.layout = Some((2, 1));
    rig.frame(Vec::new());
    let button = rig.at("view:layout");
    assert!(rig.press(button, egui::PointerButton::Primary).is_empty());
    rig.frame(Vec::new());
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    assert_eq!(rig.frame(vec![key(egui::Key::Enter)]), vec![TabAction::LayoutMenu { id: "layout.single".into() }]);
    let side_by_side = rig.at("layout.side_by_side");
    assert_eq!(rig.press(side_by_side, egui::PointerButton::Primary), vec![TabAction::LayoutMenu { id: "layout.side_by_side".into() }]);
    assert!(solidcraft_ui_registry::rect_of("layout.side_by_side").is_some());
    rig.frame(vec![key(egui::Key::Escape)]);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("layout.side_by_side").is_none());
    assert!(rig.frame(vec![key(egui::Key::Enter)]).is_empty());
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::rect_of("layout.side_by_side").is_some());
}

#[test]
fn sketch_header_keeps_the_editing_chip_inside_its_control_free_room() {
    let ctx = crate::test_support::context();
    for width in [240.0, 100.0, 70.0, 60.0, 42.0, 16.0] {
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(width, 28.0));
        let output = crate::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
            paint_sketch_header(ui.painter(), rect, "Model · Sketch 1", "Editing");
        });
        let texts: Vec<_> = crate::test_support::text_shapes(&output).map(|text| text.galley.text().to_string()).collect();
        if width == 240.0 {
            assert!(texts.iter().any(|text| text == "Model · Sketch 1"));
        }
        if width >= 70.0 {
            assert!(texts.iter().any(|text| text == "Editing"), "width={width}: the state remains readable");
        }
        let chip = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(shape) if shape.fill == Tokens::get().chip => Some(shape.rect),
                _ => None,
            })
            .unwrap();
        assert_eq!(chip.height(), 18.0);
        assert!(rect.contains_rect(chip), "width={width}: {chip:?} must leave header controls free");
    }
    for t in [Tokens::light(), Tokens::dark()] {
        assert!(
            solidcraft_theme::contrast_ratio(t.sketch_ink, t.sketch_soft) >= 4.5,
            "sketch titles follow the theme's small-text contrast policy on their actual header surface"
        );
    }
}
