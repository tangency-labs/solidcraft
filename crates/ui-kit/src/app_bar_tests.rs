//! The application bar from plain data, no app: what each click returns.
use super::*;

#[test]
fn app_bar_spaces_repeat_sanitize_persist_and_have_sixteen_point_slots() {
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
        assert!(!reset.items.iter().any(|p| p.id.starts_with("space:")));
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

#[test]
fn app_bar_edit_palette_has_three_distinct_levels_and_readable_foreground() {
    use solidcraft_theme::contrast_ratio;
    let mut onyx = Tokens::dark();
    let source: serde_json::Value = serde_json::from_str(include_str!("../../look/themes/onyx.json")).unwrap();
    let color = |name: &str| {
        let text = source["colors"][name].as_str().unwrap().trim_start_matches('#');
        egui::Color32::from_rgb(
            u8::from_str_radix(&text[..2], 16).unwrap(),
            u8::from_str_radix(&text[2..4], 16).unwrap(),
            u8::from_str_radix(&text[4..6], 16).unwrap(),
        )
    };
    onyx.app_bar = color("app_bar");
    onyx.app_bar_text = color("app_bar_text");
    for (name, t) in [("dark", Tokens::dark()), ("light", Tokens::light()), ("Onyx", onyx)] {
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
        assert!(!edited.items.iter().any(|p| p.id.starts_with("separator:")));
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
        let expected = if pill { 22.0 } else { 28.0 };
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
        assert_eq!(control_radius(control), if pill { 11 } else { 6 });
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
        assert_eq!(
            reset.items.iter().filter(|p| p.id.starts_with("separator:")).count(),
            if matches!(preset, Preset::Default | Preset::Full) { 2 } else { 0 }
        );
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
fn app_bar_edit_mode_keeps_normal_fit_and_report_version_geometry() {
    let rig = BarRig::new(1280.0);
    let snapshot =
        |out: &BarOut| (out.edit_items.iter().map(|p| (p.id.clone(), p.zone, p.left, p.right, p.locked)).collect::<Vec<_>>(), out.overflow.clone());
    rig.frame(Vec::new());
    let before = snapshot(rig.out.borrow().as_ref().unwrap());
    editing(&rig.ctx, true, None);
    rig.frame(Vec::new());
    let after = snapshot(rig.out.borrow().as_ref().unwrap());
    assert_eq!(before, after, "edit elevation has no layout inputs");
    assert!(after.0.iter().any(|p| p.0 == "report"));
    assert!(after.0.iter().any(|p| p.0 == "version"));
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
            Preset::Minimal => assert_eq!(shown, ["file", "save", "undo", "redo", "settings", "help"]),
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
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(self.width, HEIGHT));
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

const VIEW_MENU: [TabMenuRow<'static>; 3] = [
    TabMenuRow::new("vclose", "Close view", "Ctrl+W"),
    TabMenuRow::new("vothers", "Close other views", "").needing_others().separated(),
    TabMenuRow::new("vright", "Close views to the right", "").needing_others(),
];
const PLUS_MENU: [TabMenuRow<'static>; 4] = [
    TabMenuRow::heading("Open as a view"),
    TabMenuRow::new("plus_sketch", "Sketch", "").with_icon("sketch", Some(egui::Color32::GREEN)),
    TabMenuRow::new("plus_dup", "Duplicate this view", "").with_icon("duplicate", None).separated(),
    TabMenuRow::new("plus_reopen", "Reopen closed view", "Ctrl+Shift+T").with_icon("reopen", None),
];
const SWITCHER_MENU: [TabMenuRow<'static>; 1] = [TabMenuRow::new("as_tabs", "Show views as tabs", "")];

const VIEWS: [ViewTab<'static>; 3] = [
    ViewTab {
        name: "Model",
        icon: "model",
        accent: egui::Color32::BLUE,
        underline: egui::Color32::BLUE,
        state: ViewState::Normal,
        active: true,
        tip: "Model",
    },
    ViewTab {
        name: "Sketch 2",
        icon: "sketch",
        accent: egui::Color32::GREEN,
        underline: egui::Color32::GREEN,
        state: ViewState::SourceChanged,
        active: false,
        tip: "Sketch 2",
    },
    ViewTab {
        name: "Sheet 1",
        icon: "sheet",
        accent: egui::Color32::RED,
        underline: egui::Color32::RED,
        state: ViewState::Deleted,
        active: false,
        tip: "Sheet 1",
    },
];

const TAB_MENU: [TabMenuRow<'static>; 2] = [TabMenuRow::new("close", "Close", ""), TabMenuRow::new("others", "Close Others", "").needing_others()];

/// Three document tabs, the first selected, in a strip 600 wide.
struct TabsRig {
    ctx: egui::Context,
    time: std::cell::Cell<f64>,
    /// The first design has the three [`VIEWS`], shown as sub-tabs (`Some(true)`) or behind a
    /// switcher (`Some(false)`); the strip then has more room.
    views: Option<bool>,
    /// The + has a menu.
    plus: bool,
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
            views,
            plus,
            rename: rename.map(|(k, _)| k),
            text: std::cell::RefCell::new(text),
            focused: std::cell::Cell::new(false),
        };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&self, events: Vec<egui::Event>) -> Vec<TabAction> {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let tab = |name: &'static str| DocTab { name, list_name: name, dirty: false, place: "Not saved yet", security: None, views: &[] };
        let first = DocTab { views: if self.views.is_some() { &VIEWS } else { &[] }, ..tab("One") };
        let (mut text, mut focused) = (self.text.borrow().clone(), self.focused.get());
        let mut strip = TabStrip {
            tabs: vec![first, tab("Two"), tab("Three")],
            selected: 0,
            shown_selected: Some(0),
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
            view_text: ViewText {
                close_view: "Close view",
                close_design: "Close design",
                new_view: "Open another view",
                more: "{n} more",
                switch_view: "Switch view",
            },
            view_menu: &VIEW_MENU,
            view_right_row: "vright",
            view_rename: self.rename.map(|k| (k, TabRename { text: &mut text, focused: &mut focused, max: 8 })),
            plus_menu: if self.plus { &PLUS_MENU } else { &[] },
            switcher_menu: &SWITCHER_MENU,
            tear_off_below: 40.0,
        };
        let (x0, x1) = if self.views.is_some() { (20.0, 780.0) } else { (100.0, 700.0) };
        let mut actions = Vec::new();
        crate::test_support::raw_frame(&self.ctx, input, |ui| {
            actions = tabs(ui, &mut strip, Rect::from_min_size(Pos2::ZERO, vec2(800.0, HEIGHT)), x0, x1);
        });
        *self.text.borrow_mut() = text;
        self.focused.set(focused);
        actions
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
    let from = rig.at("t:view:0");
    let to = solidcraft_ui_registry::rect_of("t:view:2").map(|r| r.right_center() - vec2(4.0, 0.0)).expect("view 2 is drawn");
    let mut actions = rig.frame(vec![egui::Event::PointerMoved(from)]);
    actions.extend(rig.frame(vec![press(from, true)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(from + vec2(10.0, 0.0))]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(to)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(to)]));
    actions.extend(rig.frame(vec![press(to, false)]));
    assert_eq!(actions, vec![TabAction::ReorderView { from: 0, to: 2 }]);
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
    let row = solidcraft_ui_registry::handle_at("t:menu:plus_sketch").expect("the menu's row is drawn");
    assert!(row.y > HEIGHT && row.x >= plus.left(), "the menu opens under the + at {row:?}");
    assert_eq!(rig.press(row, egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_sketch".into() }]);
    // A press and release in one frame (a click driven by a script) leaves the menu open too.
    let at = rig.at("t:vnew");
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    rig.frame(vec![button(true), button(false)]);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("t:menu:plus_sketch").is_some());
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
    let (sketch, dup) = (rig.at("t:menu:plus_sketch"), rig.at("t:menu:plus_dup"));
    assert!(dup.y - sketch.y > 32.0, "a separator sits above Duplicate: {sketch:?} {dup:?}");
    assert_eq!(rig.press(dup, egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_dup".into() }]);
    rig.press(rig.at("t:vnew"), egui::PointerButton::Primary);
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.press(rig.at("t:menu:plus_reopen"), egui::PointerButton::Primary), vec![TabAction::PlusMenu { id: "plus_reopen".into() }]);
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
    for glyph in ["model", "sketch", "sheet", "render", "animation", "rename", "window", "close", "duplicate", "reopen"] {
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

/// A hand-derived uniform stack: logo 82, square controls 28, dividers 1 and gaps 4.
#[test]
fn app_bar_measured_groups_project_their_controls() {
    use crate::app_bar_layout::application::{GAP, Span, spec};
    let spec = spec(1920.0, 0.0, 138.0, measured_bar());
    let placed = layout(&spec);
    let g = placed.application.as_ref().unwrap();
    assert_eq!(spec.gap, GAP);
    assert_eq!(g.leading, Some(Span { left: 86.0, right: 114.0 }));
    assert_eq!(g.home, Span { left: 118.0, right: 146.0 });
    assert_eq!(g.file, Span { left: 155.0, right: 195.0 });
    assert_eq!(g.actions, [Some(213.0), Some(245.0), Some(277.0)]);
    assert_eq!(g.quick, Some((309.0, 32.0)));
    assert_eq!(g.tabs_start, 460.0);
    assert_eq!(g.report, Span { left: 1547.0, right: 1640.0 });
    assert_eq!(g.version, Some(Span { left: 1644.0, right: 1698.0 }));
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
    assert_eq!(g.offer.right, 1920.0 - 138.0);
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
            for bits in 0..512 {
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
                    let spec = crate::app_bar_layout::application::spec(width as f32, left, if has(7) { 138.0 } else { 0.0 }, m);
                    let placed = layout(&spec);
                    assert_sound(&spec, &placed);
                    for item in spec.items.iter().filter(|i| i.locked || i.never_overflow) {
                        assert!(!placed.overflow.contains(&item.id), "locked {} at {width}/{bits}/{quick}", item.id);
                        if spec.width - spec.captions_left - spec.captions_right >= 232.0 {
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
            let spec = spec_with_items((width as f32, 78.0, 138.0), measurements, &saved, &["save", "undo", "redo"], &quick, &["donate", "discord"]);
            let placed = layout(&spec);
            assert_sound(&spec, &placed);
            let g = placed.application.unwrap();
            for (i, x) in &g.quick_slots {
                let slot = placed.placements[index_of(&spec, &format!("quick:{i}"))].unwrap();
                assert!(*x - ITEM_HEIGHT / 2.0 >= slot.x - EPS && *x + ITEM_HEIGHT / 2.0 <= slot.right() + EPS);
            }
        }
    }
}

// Re-captured because application-bar spacing was simplified: one four-point gap, no
// per-item layout offsets. Regenerate with SOLIDCRAFT_RECAPTURE_APP_BAR=1 and the app_bar test
// filter. Each of the same 120 cases records field lengths and millipoint positions/widths.
#[rustfmt::skip]
const APP_BAR_ROWS: &[&[i32]] = &[
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 362000, 6000, 362000, 28000, 1, 0, 2, 200281, 362000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 0
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 362000, 6000, 362000, 28000, 1, 0, 2, 200281, 362000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 1
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 394000, 6000, 394000, 28000, 1, 0, 2, 239594, 394000, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 480 / 2
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 362000, 6000, 362000, 28000, 1, 0, 2, 200281, 362000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 4
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 362000, 6000, 362000, 28000, 1, 0, 2, 200281, 362000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 8
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 330000, 6000, 330000, 28000, 1, 0, 2, 200281, 330000, 0, 4, 362000, 3000, 390000, 31000, 0, 4, 64000, 3000, 100281, 31000], // 480 / 16
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 452000, 3000, 480000, 31000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 420000, 3000, 448000, 31000, 2, 434000, 17000, 0, 0, 2, 214281, 32000, 4, 388000, 6000, 388000, 28000, 1, 0, 2, 232281, 388000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 32
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 362000, 6000, 362000, 28000, 1, 0, 2, 200281, 362000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 64
    &[0, 0, 4, 32000, 3000, 68281, 31000, 2, 50141, 17000, 4, 72281, 3000, 100281, 31000, 2, 86281, 17000, 0, 0, 0, 0, 4, 314000, 3000, 342000, 31000, 2, 328000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 3000, 310000, 31000, 2, 296000, 17000, 0, 0, 0, 4, 250000, 6000, 250000, 28000, 1, 0, 2, 104281, 250000, 0, 0, 0, 4, 32000, 3000, 68281, 31000], // 480 / 128
    &[0, 0, 4, 32000, 3000, 68281, 31000, 2, 50141, 17000, 4, 72281, 3000, 100281, 31000, 2, 86281, 17000, 0, 0, 0, 0, 4, 314000, 3000, 342000, 31000, 2, 328000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 3000, 310000, 31000, 2, 296000, 17000, 0, 0, 0, 4, 250000, 6000, 250000, 28000, 1, 0, 2, 104281, 250000, 0, 0, 0, 4, 32000, 3000, 68281, 31000], // 480 / 255
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 330000, 6000, 330000, 28000, 1, 0, 2, 200281, 330000, 0, 4, 362000, 3000, 390000, 31000, 0, 4, 64000, 3000, 100281, 31000], // 480 / 28
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 4, 362000, 3000, 390000, 31000, 2, 376000, 17000, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 330000, 6000, 330000, 28000, 1, 0, 2, 200281, 330000, 4, 362000, 3000, 390000, 31000, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 3
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 362000, 6000, 362000, 28000, 1, 0, 2, 200281, 362000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 5
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 69000, 3000, 105281, 31000, 2, 87141, 17000, 4, 109281, 3000, 137281, 31000, 2, 123281, 17000, 4, 141281, 3000, 169281, 31000, 2, 155281, 17000, 4, 173281, 3000, 201281, 31000, 2, 187281, 17000, 4, 377594, 3000, 405594, 31000, 2, 391594, 17000, 4, 64000, 9000, 65000, 25000, 0, 0, 0, 4, 205281, 9000, 206281, 25000, 0, 0, 0, 4, 409594, 6000, 431594, 28000, 2, 420594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 345594, 3000, 373594, 31000, 2, 359594, 17000, 4, 435594, 3000, 480000, 31000, 2, 457797, 17000, 0, 4, 345594, 6000, 345594, 28000, 1, 0, 2, 210281, 345594, 0, 0, 0, 4, 69000, 3000, 105281, 31000], // 480 / 6
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 362000, 6000, 362000, 28000, 1, 0, 2, 200281, 362000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 9
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 330000, 6000, 330000, 28000, 1, 0, 2, 200281, 330000, 0, 4, 362000, 3000, 390000, 31000, 0, 4, 64000, 3000, 100281, 31000], // 480 / 17
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 330000, 6000, 330000, 28000, 1, 0, 2, 200281, 330000, 0, 4, 362000, 3000, 390000, 31000, 0, 4, 64000, 3000, 100281, 31000], // 480 / 24
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 452000, 3000, 480000, 31000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 420000, 3000, 448000, 31000, 2, 434000, 17000, 0, 0, 2, 214281, 32000, 4, 388000, 6000, 388000, 28000, 1, 0, 2, 232281, 388000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 33
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 426000, 3000, 454000, 31000, 2, 440000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 458000, 6000, 480000, 28000, 2, 469000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 394000, 3000, 422000, 31000, 2, 408000, 17000, 0, 0, 0, 4, 362000, 6000, 362000, 28000, 1, 0, 2, 200281, 362000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 65
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 452000, 3000, 480000, 31000, 2, 466000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 420000, 3000, 448000, 31000, 2, 434000, 17000, 0, 0, 2, 214281, 32000, 4, 388000, 6000, 388000, 28000, 1, 0, 2, 232281, 388000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 480 / 96
    &[0, 0, 4, 32000, 3000, 68281, 31000, 2, 50141, 17000, 4, 72281, 3000, 100281, 31000, 2, 86281, 17000, 0, 0, 0, 0, 4, 314000, 3000, 342000, 31000, 2, 328000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 3000, 310000, 31000, 2, 296000, 17000, 0, 0, 0, 4, 250000, 6000, 250000, 28000, 1, 0, 2, 104281, 250000, 0, 0, 0, 4, 32000, 3000, 68281, 31000], // 480 / 129
    &[0, 0, 4, 32000, 3000, 68281, 31000, 2, 50141, 17000, 4, 72281, 3000, 100281, 31000, 2, 86281, 17000, 0, 0, 0, 0, 4, 314000, 3000, 342000, 31000, 2, 328000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 3000, 310000, 31000, 2, 296000, 17000, 0, 0, 0, 4, 250000, 6000, 250000, 28000, 1, 0, 2, 104281, 250000, 0, 0, 0, 4, 32000, 3000, 68281, 31000], // 480 / 160
    &[0, 0, 4, 32000, 3000, 68281, 31000, 2, 50141, 17000, 4, 72281, 3000, 100281, 31000, 2, 86281, 17000, 0, 0, 0, 0, 4, 314000, 3000, 342000, 31000, 2, 328000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 3000, 310000, 31000, 2, 296000, 17000, 0, 0, 0, 4, 250000, 6000, 250000, 28000, 1, 0, 2, 104281, 250000, 0, 0, 0, 4, 32000, 3000, 68281, 31000], // 480 / 188
    &[0, 0, 4, 32000, 3000, 68281, 31000, 2, 50141, 17000, 4, 72281, 3000, 100281, 31000, 2, 86281, 17000, 0, 0, 0, 0, 4, 314000, 3000, 342000, 31000, 2, 328000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 282000, 3000, 310000, 31000, 2, 296000, 17000, 0, 0, 0, 4, 250000, 6000, 250000, 28000, 1, 0, 2, 104281, 250000, 0, 0, 0, 4, 32000, 3000, 68281, 31000], // 480 / 254
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 467844, 6000, 467844, 28000, 1, 0, 2, 239594, 467844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 640 / 0
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 0, 0, 0, 0, 0, 0, 4, 435844, 3000, 463844, 31000, 2, 449844, 17000, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 403844, 6000, 403844, 28000, 1, 0, 2, 200281, 403844, 4, 435844, 3000, 463844, 31000, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 1
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 586000, 3000, 614000, 31000, 2, 600000, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 618000, 6000, 640000, 28000, 2, 629000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 554000, 3000, 582000, 31000, 2, 568000, 17000, 0, 0, 0, 4, 554000, 6000, 554000, 28000, 1, 0, 2, 239594, 554000, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 640 / 2
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 451438, 3000, 479438, 31000, 2, 465438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 483438, 6000, 591594, 28000, 2, 537516, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 419438, 3000, 447438, 31000, 2, 433438, 17000, 4, 595594, 3000, 640000, 31000, 2, 617797, 17000, 0, 4, 419438, 6000, 419438, 28000, 1, 0, 2, 239594, 419438, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 640 / 4
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 4, 373250, 6000, 442844, 28000, 2, 408047, 17000, 4, 442844, 10500, 455844, 23500, 2, 449344, 17000, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 341250, 6000, 341250, 28000, 1, 0, 2, 200281, 341250, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 8
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 435844, 6000, 435844, 28000, 1, 0, 2, 239594, 435844, 0, 4, 435844, 3000, 463844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 640 / 16
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 586000, 3000, 614000, 31000, 2, 600000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 618000, 6000, 640000, 28000, 2, 629000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 554000, 3000, 582000, 31000, 2, 568000, 17000, 0, 0, 2, 214281, 32000, 4, 522000, 6000, 522000, 28000, 1, 0, 2, 360281, 522000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 32
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 69000, 3000, 105281, 31000, 2, 87141, 17000, 4, 109281, 3000, 137281, 31000, 2, 123281, 17000, 4, 141281, 3000, 169281, 31000, 2, 155281, 17000, 4, 173281, 3000, 201281, 31000, 2, 187281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 4, 64000, 9000, 65000, 25000, 0, 0, 0, 4, 205281, 9000, 206281, 25000, 0, 0, 0, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 467844, 6000, 467844, 28000, 1, 0, 2, 210281, 467844, 0, 0, 0, 4, 69000, 3000, 105281, 31000], // 640 / 64
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 448000, 3000, 476000, 31000, 2, 462000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 480000, 6000, 502000, 28000, 2, 491000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 416000, 3000, 444000, 31000, 2, 430000, 17000, 0, 0, 0, 4, 384000, 6000, 384000, 28000, 1, 0, 2, 200281, 384000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 128
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 474000, 3000, 502000, 31000, 2, 488000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 442000, 3000, 470000, 31000, 2, 456000, 17000, 0, 0, 2, 214281, 32000, 4, 410000, 6000, 410000, 28000, 1, 0, 2, 264281, 410000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 255
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 4, 385844, 6000, 410844, 28000, 2, 398344, 17000, 4, 410844, 10500, 423844, 23500, 2, 417344, 17000, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 353844, 6000, 353844, 28000, 1, 0, 2, 200281, 353844, 0, 4, 435844, 3000, 463844, 31000, 0, 4, 64000, 3000, 100281, 31000], // 640 / 28
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 69000, 3000, 105281, 31000, 2, 87141, 17000, 4, 109281, 3000, 137281, 31000, 2, 123281, 17000, 4, 141281, 3000, 169281, 31000, 2, 155281, 17000, 4, 173281, 3000, 201281, 31000, 2, 187281, 17000, 4, 586000, 3000, 614000, 31000, 2, 600000, 17000, 4, 64000, 9000, 65000, 25000, 0, 0, 0, 4, 205281, 9000, 206281, 25000, 0, 4, 367000, 3000, 550000, 31000, 2, 458500, 17000, 4, 618000, 6000, 640000, 28000, 2, 629000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 554000, 3000, 582000, 31000, 2, 568000, 17000, 0, 0, 0, 4, 367000, 6000, 367000, 28000, 1, 0, 2, 210281, 367000, 4, 367000, 3000, 550000, 31000, 0, 0, 4, 69000, 3000, 105281, 31000], // 640 / 3
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 0, 0, 0, 0, 0, 0, 4, 435844, 3000, 463844, 31000, 2, 449844, 17000, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 403844, 6000, 403844, 28000, 1, 0, 2, 200281, 403844, 4, 435844, 3000, 463844, 31000, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 5
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 537594, 3000, 565594, 31000, 2, 551594, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 569594, 6000, 591594, 28000, 2, 580594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 505594, 3000, 533594, 31000, 2, 519594, 17000, 4, 595594, 3000, 640000, 31000, 2, 617797, 17000, 0, 4, 505594, 6000, 505594, 28000, 1, 0, 2, 239594, 505594, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 640 / 6
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 0, 0, 0, 0, 0, 0, 4, 435844, 3000, 463844, 31000, 2, 449844, 17000, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 403844, 6000, 403844, 28000, 1, 0, 2, 200281, 403844, 4, 435844, 3000, 463844, 31000, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 9
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 0, 0, 0, 0, 0, 0, 4, 435844, 3000, 463844, 31000, 2, 449844, 17000, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 371844, 6000, 371844, 28000, 1, 0, 2, 200281, 371844, 4, 435844, 3000, 463844, 31000, 4, 403844, 3000, 431844, 31000, 0, 4, 64000, 3000, 100281, 31000], // 640 / 17
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 4, 385844, 6000, 410844, 28000, 2, 398344, 17000, 4, 410844, 10500, 423844, 23500, 2, 417344, 17000, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 353844, 6000, 353844, 28000, 1, 0, 2, 200281, 353844, 0, 4, 435844, 3000, 463844, 31000, 0, 4, 64000, 3000, 100281, 31000], // 640 / 24
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 586000, 3000, 614000, 31000, 2, 600000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 618000, 6000, 640000, 28000, 2, 629000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 554000, 3000, 582000, 31000, 2, 568000, 17000, 0, 0, 2, 214281, 32000, 4, 522000, 6000, 522000, 28000, 1, 0, 2, 360281, 522000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 33
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 499844, 3000, 527844, 31000, 2, 513844, 17000, 0, 0, 0, 0, 0, 0, 4, 435844, 3000, 463844, 31000, 2, 449844, 17000, 4, 531844, 6000, 640000, 28000, 2, 585922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 467844, 3000, 495844, 31000, 2, 481844, 17000, 0, 0, 0, 4, 403844, 6000, 403844, 28000, 1, 0, 2, 200281, 403844, 4, 435844, 3000, 463844, 31000, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 65
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 586000, 3000, 614000, 31000, 2, 600000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 618000, 6000, 640000, 28000, 2, 629000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 554000, 3000, 582000, 31000, 2, 568000, 17000, 0, 0, 2, 214281, 32000, 4, 522000, 6000, 522000, 28000, 1, 0, 2, 360281, 522000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 96
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 448000, 3000, 476000, 31000, 2, 462000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 480000, 6000, 502000, 28000, 2, 491000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 416000, 3000, 444000, 31000, 2, 430000, 17000, 0, 0, 0, 4, 384000, 6000, 384000, 28000, 1, 0, 2, 200281, 384000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 129
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 474000, 3000, 502000, 31000, 2, 488000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 442000, 3000, 470000, 31000, 2, 456000, 17000, 0, 0, 2, 214281, 32000, 4, 410000, 6000, 410000, 28000, 1, 0, 2, 264281, 410000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 160
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 474000, 3000, 502000, 31000, 2, 488000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 442000, 3000, 470000, 31000, 2, 456000, 17000, 0, 0, 2, 214281, 32000, 4, 410000, 6000, 410000, 28000, 1, 0, 2, 264281, 410000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 188
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 474000, 3000, 502000, 31000, 2, 488000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 442000, 3000, 470000, 31000, 2, 456000, 17000, 0, 0, 2, 214281, 32000, 4, 410000, 6000, 410000, 28000, 1, 0, 2, 264281, 410000, 0, 0, 0, 4, 64000, 3000, 100281, 31000], // 640 / 254
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 827844, 6000, 827844, 28000, 1, 0, 2, 239594, 827844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 0
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 640844, 3000, 823844, 31000, 2, 732344, 17000, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 640844, 6000, 640844, 28000, 1, 0, 2, 239594, 640844, 4, 640844, 3000, 823844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 1
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 946000, 3000, 974000, 31000, 2, 960000, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 978000, 6000, 1000000, 28000, 2, 989000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 914000, 3000, 942000, 31000, 2, 928000, 17000, 0, 0, 0, 4, 914000, 6000, 914000, 28000, 1, 0, 2, 239594, 914000, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 2
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 811438, 3000, 839438, 31000, 2, 825438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 843438, 6000, 951594, 28000, 2, 897516, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 779438, 3000, 807438, 31000, 2, 793438, 17000, 4, 955594, 3000, 1000000, 31000, 2, 977797, 17000, 0, 4, 779438, 6000, 779438, 28000, 1, 0, 2, 239594, 779438, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 4
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 4, 639531, 6000, 708250, 28000, 2, 673891, 17000, 4, 708250, 10500, 721250, 23500, 2, 714750, 17000, 4, 733250, 6000, 802844, 28000, 2, 768047, 17000, 4, 802844, 10500, 815844, 23500, 2, 809344, 17000, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 639531, 6000, 639531, 28000, 1, 0, 2, 239594, 639531, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 8
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 795844, 6000, 795844, 28000, 1, 0, 2, 239594, 795844, 0, 4, 795844, 3000, 823844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 16
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 0, 0, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 2, 248594, 32000, 4, 827844, 6000, 827844, 28000, 1, 0, 2, 399594, 827844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 32
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 423031, 9000, 424031, 25000, 0, 0, 0, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 827844, 6000, 827844, 28000, 1, 0, 2, 428031, 827844, 0, 0, 0, 4, 286750, 3000, 323031, 31000], // 1000 / 64
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 721844, 3000, 749844, 31000, 2, 735844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 753844, 6000, 862000, 28000, 2, 807922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 689844, 3000, 717844, 31000, 2, 703844, 17000, 0, 0, 0, 4, 689844, 6000, 689844, 28000, 1, 0, 2, 239594, 689844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 128
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 808000, 3000, 836000, 31000, 2, 822000, 17000, 0, 0, 0, 0, 0, 0, 4, 589000, 3000, 772000, 31000, 2, 680500, 17000, 4, 840000, 6000, 862000, 28000, 2, 851000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 776000, 3000, 804000, 31000, 2, 790000, 17000, 0, 0, 2, 214281, 32000, 4, 525000, 6000, 525000, 28000, 1, 0, 2, 360281, 525000, 4, 589000, 3000, 772000, 31000, 4, 557000, 3000, 585000, 31000, 0, 4, 64000, 3000, 100281, 31000], // 1000 / 255
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 811438, 3000, 839438, 31000, 2, 825438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 843438, 6000, 951594, 28000, 2, 897516, 17000, 4, 559125, 6000, 627844, 28000, 2, 593484, 17000, 4, 627844, 10500, 640844, 23500, 2, 634344, 17000, 4, 652844, 6000, 722438, 28000, 2, 687641, 17000, 4, 722438, 10500, 735438, 23500, 2, 728938, 17000, 4, 779438, 3000, 807438, 31000, 2, 793438, 17000, 4, 955594, 3000, 1000000, 31000, 2, 977797, 17000, 0, 4, 559125, 6000, 559125, 28000, 1, 0, 2, 239594, 559125, 0, 4, 747438, 3000, 775438, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 28
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 946000, 3000, 974000, 31000, 2, 960000, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 727000, 3000, 910000, 31000, 2, 818500, 17000, 4, 978000, 6000, 1000000, 28000, 2, 989000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 914000, 3000, 942000, 31000, 2, 928000, 17000, 0, 0, 0, 4, 727000, 6000, 727000, 28000, 1, 0, 2, 239594, 727000, 4, 727000, 3000, 910000, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 3
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 811438, 3000, 839438, 31000, 2, 825438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 592438, 3000, 775438, 31000, 2, 683938, 17000, 4, 843438, 6000, 951594, 28000, 2, 897516, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 779438, 3000, 807438, 31000, 2, 793438, 17000, 4, 955594, 3000, 1000000, 31000, 2, 977797, 17000, 0, 4, 592438, 6000, 592438, 28000, 1, 0, 2, 239594, 592438, 4, 592438, 3000, 775438, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 5
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 897594, 3000, 925594, 31000, 2, 911594, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 929594, 6000, 951594, 28000, 2, 940594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 865594, 3000, 893594, 31000, 2, 879594, 17000, 4, 955594, 3000, 1000000, 31000, 2, 977797, 17000, 0, 4, 865594, 6000, 865594, 28000, 1, 0, 2, 239594, 865594, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 6
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 640844, 3000, 823844, 31000, 2, 732344, 17000, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 4, 452531, 6000, 521250, 28000, 2, 486891, 17000, 4, 521250, 10500, 534250, 23500, 2, 527750, 17000, 4, 546250, 6000, 615844, 28000, 2, 581047, 17000, 4, 615844, 10500, 628844, 23500, 2, 622344, 17000, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 452531, 6000, 452531, 28000, 1, 0, 2, 239594, 452531, 4, 640844, 3000, 823844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 9
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 640844, 3000, 823844, 31000, 2, 732344, 17000, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 608844, 6000, 608844, 28000, 1, 0, 2, 239594, 608844, 4, 640844, 3000, 823844, 31000, 4, 608844, 3000, 636844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 17
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 4, 607531, 6000, 676250, 28000, 2, 641891, 17000, 4, 676250, 10500, 689250, 23500, 2, 682750, 17000, 4, 701250, 6000, 770844, 28000, 2, 736047, 17000, 4, 770844, 10500, 783844, 23500, 2, 777344, 17000, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 607531, 6000, 607531, 28000, 1, 0, 2, 239594, 607531, 0, 4, 795844, 3000, 823844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 24
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 4, 640844, 3000, 823844, 31000, 2, 732344, 17000, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 2, 248594, 32000, 4, 640844, 6000, 640844, 28000, 1, 0, 2, 399594, 640844, 4, 640844, 3000, 823844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 33
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 423031, 9000, 424031, 25000, 0, 4, 640844, 3000, 823844, 31000, 2, 732344, 17000, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 0, 4, 640844, 6000, 640844, 28000, 1, 0, 2, 428031, 640844, 4, 640844, 3000, 823844, 31000, 0, 0, 4, 286750, 3000, 323031, 31000], // 1000 / 65
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 859844, 3000, 887844, 31000, 2, 873844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 583031, 9000, 584031, 25000, 0, 0, 0, 4, 891844, 6000, 1000000, 28000, 2, 945922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 827844, 3000, 855844, 31000, 2, 841844, 17000, 0, 0, 2, 437031, 32000, 4, 827844, 6000, 827844, 28000, 1, 0, 2, 588031, 827844, 0, 0, 0, 4, 286750, 3000, 323031, 31000], // 1000 / 96
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 721844, 3000, 749844, 31000, 2, 735844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 502844, 3000, 685844, 31000, 2, 594344, 17000, 4, 753844, 6000, 862000, 28000, 2, 807922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 689844, 3000, 717844, 31000, 2, 703844, 17000, 0, 0, 0, 4, 502844, 6000, 502844, 28000, 1, 0, 2, 239594, 502844, 4, 502844, 3000, 685844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 129
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 721844, 3000, 749844, 31000, 2, 735844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 0, 0, 4, 753844, 6000, 862000, 28000, 2, 807922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 689844, 3000, 717844, 31000, 2, 703844, 17000, 0, 0, 2, 248594, 32000, 4, 689844, 6000, 689844, 28000, 1, 0, 2, 399594, 689844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1000 / 160
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 64000, 3000, 100281, 31000, 2, 82141, 17000, 4, 104281, 3000, 132281, 31000, 2, 118281, 17000, 4, 136281, 3000, 164281, 31000, 2, 150281, 17000, 4, 168281, 3000, 196281, 31000, 2, 182281, 17000, 4, 721844, 3000, 749844, 31000, 2, 735844, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 753844, 6000, 862000, 28000, 2, 807922, 17000, 0, 0, 0, 0, 4, 563250, 6000, 632844, 28000, 2, 598047, 17000, 4, 632844, 10500, 645844, 23500, 2, 639344, 17000, 4, 689844, 3000, 717844, 31000, 2, 703844, 17000, 0, 0, 2, 214281, 32000, 4, 531250, 6000, 531250, 28000, 1, 0, 2, 360281, 531250, 0, 4, 657844, 3000, 685844, 31000, 0, 4, 64000, 3000, 100281, 31000], // 1000 / 188
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 69000, 3000, 105281, 31000, 2, 87141, 17000, 4, 109281, 3000, 137281, 31000, 2, 123281, 17000, 4, 141281, 3000, 169281, 31000, 2, 155281, 17000, 4, 173281, 3000, 201281, 31000, 2, 187281, 17000, 4, 759594, 3000, 787594, 31000, 2, 773594, 17000, 4, 64000, 9000, 65000, 25000, 0, 0, 0, 4, 365281, 9000, 366281, 25000, 0, 0, 0, 4, 791594, 6000, 813594, 28000, 2, 802594, 17000, 4, 507281, 6000, 576000, 28000, 2, 541641, 17000, 4, 576000, 10500, 589000, 23500, 2, 582500, 17000, 4, 601000, 6000, 670594, 28000, 2, 635797, 17000, 4, 670594, 10500, 683594, 23500, 2, 677094, 17000, 4, 727594, 3000, 755594, 31000, 2, 741594, 17000, 4, 817594, 3000, 862000, 31000, 2, 839797, 17000, 2, 219281, 32000, 4, 507281, 6000, 507281, 28000, 1, 0, 2, 370281, 507281, 0, 4, 695594, 3000, 723594, 31000, 0, 4, 69000, 3000, 105281, 31000], // 1000 / 254
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 1107844, 6000, 1107844, 28000, 1, 0, 2, 239594, 1107844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 0
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 920844, 3000, 1103844, 31000, 2, 1012344, 17000, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 920844, 6000, 920844, 28000, 1, 0, 2, 239594, 920844, 4, 920844, 3000, 1103844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 1
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1226000, 3000, 1254000, 31000, 2, 1240000, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1258000, 6000, 1280000, 28000, 2, 1269000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1194000, 3000, 1222000, 31000, 2, 1208000, 17000, 0, 0, 0, 4, 1194000, 6000, 1194000, 28000, 1, 0, 2, 239594, 1194000, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 2
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1091438, 3000, 1119438, 31000, 2, 1105438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1123438, 6000, 1231594, 28000, 2, 1177516, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1059438, 3000, 1087438, 31000, 2, 1073438, 17000, 4, 1235594, 3000, 1280000, 31000, 2, 1257797, 17000, 0, 4, 1059438, 6000, 1059438, 28000, 1, 0, 2, 239594, 1059438, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 4
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 4, 919531, 6000, 988250, 28000, 2, 953891, 17000, 4, 988250, 10500, 1001250, 23500, 2, 994750, 17000, 4, 1013250, 6000, 1082844, 28000, 2, 1048047, 17000, 4, 1082844, 10500, 1095844, 23500, 2, 1089344, 17000, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 919531, 6000, 919531, 28000, 1, 0, 2, 239594, 919531, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 8
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 1075844, 6000, 1075844, 28000, 1, 0, 2, 239594, 1075844, 0, 4, 1075844, 3000, 1103844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 16
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 0, 0, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 2, 248594, 32000, 4, 1107844, 6000, 1107844, 28000, 1, 0, 2, 399594, 1107844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 32
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 423031, 9000, 424031, 25000, 0, 0, 0, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 1107844, 6000, 1107844, 28000, 1, 0, 2, 428031, 1107844, 0, 0, 0, 4, 286750, 3000, 323031, 31000], // 1280 / 64
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1001844, 3000, 1029844, 31000, 2, 1015844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1033844, 6000, 1142000, 28000, 2, 1087922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 969844, 3000, 997844, 31000, 2, 983844, 17000, 0, 0, 0, 4, 969844, 6000, 969844, 28000, 1, 0, 2, 239594, 969844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 128
    &[4, 32000, 3000, 60000, 31000, 2, 46000, 17000, 4, 69000, 3000, 105281, 31000, 2, 87141, 17000, 4, 109281, 3000, 137281, 31000, 2, 123281, 17000, 4, 141281, 3000, 169281, 31000, 2, 155281, 17000, 4, 173281, 3000, 201281, 31000, 2, 187281, 17000, 4, 1039594, 3000, 1067594, 31000, 2, 1053594, 17000, 4, 64000, 9000, 65000, 25000, 0, 0, 0, 4, 365281, 9000, 366281, 25000, 0, 4, 820594, 3000, 1003594, 31000, 2, 912094, 17000, 4, 1071594, 6000, 1093594, 28000, 2, 1082594, 17000, 4, 600281, 6000, 669000, 28000, 2, 634641, 17000, 4, 669000, 10500, 682000, 23500, 2, 675500, 17000, 4, 694000, 6000, 763594, 28000, 2, 728797, 17000, 4, 763594, 10500, 776594, 23500, 2, 770094, 17000, 4, 1007594, 3000, 1035594, 31000, 2, 1021594, 17000, 4, 1097594, 3000, 1142000, 31000, 2, 1119797, 17000, 2, 219281, 32000, 4, 600281, 6000, 600281, 28000, 1, 0, 2, 370281, 600281, 4, 820594, 3000, 1003594, 31000, 4, 788594, 3000, 816594, 31000, 0, 4, 69000, 3000, 105281, 31000], // 1280 / 255
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1091438, 3000, 1119438, 31000, 2, 1105438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1123438, 6000, 1231594, 28000, 2, 1177516, 17000, 4, 839125, 6000, 907844, 28000, 2, 873484, 17000, 4, 907844, 10500, 920844, 23500, 2, 914344, 17000, 4, 932844, 6000, 1002438, 28000, 2, 967641, 17000, 4, 1002438, 10500, 1015438, 23500, 2, 1008938, 17000, 4, 1059438, 3000, 1087438, 31000, 2, 1073438, 17000, 4, 1235594, 3000, 1280000, 31000, 2, 1257797, 17000, 0, 4, 839125, 6000, 839125, 28000, 1, 0, 2, 239594, 839125, 0, 4, 1027438, 3000, 1055438, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 28
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1226000, 3000, 1254000, 31000, 2, 1240000, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 1007000, 3000, 1190000, 31000, 2, 1098500, 17000, 4, 1258000, 6000, 1280000, 28000, 2, 1269000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1194000, 3000, 1222000, 31000, 2, 1208000, 17000, 0, 0, 0, 4, 1007000, 6000, 1007000, 28000, 1, 0, 2, 239594, 1007000, 4, 1007000, 3000, 1190000, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 3
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1091438, 3000, 1119438, 31000, 2, 1105438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 872438, 3000, 1055438, 31000, 2, 963938, 17000, 4, 1123438, 6000, 1231594, 28000, 2, 1177516, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1059438, 3000, 1087438, 31000, 2, 1073438, 17000, 4, 1235594, 3000, 1280000, 31000, 2, 1257797, 17000, 0, 4, 872438, 6000, 872438, 28000, 1, 0, 2, 239594, 872438, 4, 872438, 3000, 1055438, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 5
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1177594, 3000, 1205594, 31000, 2, 1191594, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1209594, 6000, 1231594, 28000, 2, 1220594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1145594, 3000, 1173594, 31000, 2, 1159594, 17000, 4, 1235594, 3000, 1280000, 31000, 2, 1257797, 17000, 0, 4, 1145594, 6000, 1145594, 28000, 1, 0, 2, 239594, 1145594, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 6
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 920844, 3000, 1103844, 31000, 2, 1012344, 17000, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 4, 732531, 6000, 801250, 28000, 2, 766891, 17000, 4, 801250, 10500, 814250, 23500, 2, 807750, 17000, 4, 826250, 6000, 895844, 28000, 2, 861047, 17000, 4, 895844, 10500, 908844, 23500, 2, 902344, 17000, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 732531, 6000, 732531, 28000, 1, 0, 2, 239594, 732531, 4, 920844, 3000, 1103844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 9
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 920844, 3000, 1103844, 31000, 2, 1012344, 17000, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 888844, 6000, 888844, 28000, 1, 0, 2, 239594, 888844, 4, 920844, 3000, 1103844, 31000, 4, 888844, 3000, 916844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 17
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 4, 887531, 6000, 956250, 28000, 2, 921891, 17000, 4, 956250, 10500, 969250, 23500, 2, 962750, 17000, 4, 981250, 6000, 1050844, 28000, 2, 1016047, 17000, 4, 1050844, 10500, 1063844, 23500, 2, 1057344, 17000, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 887531, 6000, 887531, 28000, 1, 0, 2, 239594, 887531, 0, 4, 1075844, 3000, 1103844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 24
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 4, 920844, 3000, 1103844, 31000, 2, 1012344, 17000, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 2, 248594, 32000, 4, 920844, 6000, 920844, 28000, 1, 0, 2, 399594, 920844, 4, 920844, 3000, 1103844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 33
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 423031, 9000, 424031, 25000, 0, 4, 920844, 3000, 1103844, 31000, 2, 1012344, 17000, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 0, 4, 920844, 6000, 920844, 28000, 1, 0, 2, 428031, 920844, 4, 920844, 3000, 1103844, 31000, 0, 0, 4, 286750, 3000, 323031, 31000], // 1280 / 65
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1139844, 3000, 1167844, 31000, 2, 1153844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 583031, 9000, 584031, 25000, 0, 0, 0, 4, 1171844, 6000, 1280000, 28000, 2, 1225922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1107844, 3000, 1135844, 31000, 2, 1121844, 17000, 0, 0, 2, 437031, 32000, 4, 1107844, 6000, 1107844, 28000, 1, 0, 2, 588031, 1107844, 0, 0, 0, 4, 286750, 3000, 323031, 31000], // 1280 / 96
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1001844, 3000, 1029844, 31000, 2, 1015844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 782844, 3000, 965844, 31000, 2, 874344, 17000, 4, 1033844, 6000, 1142000, 28000, 2, 1087922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 969844, 3000, 997844, 31000, 2, 983844, 17000, 0, 0, 0, 4, 782844, 6000, 782844, 28000, 1, 0, 2, 239594, 782844, 4, 782844, 3000, 965844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 129
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1001844, 3000, 1029844, 31000, 2, 1015844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 0, 0, 4, 1033844, 6000, 1142000, 28000, 2, 1087922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 969844, 3000, 997844, 31000, 2, 983844, 17000, 0, 0, 2, 248594, 32000, 4, 969844, 6000, 969844, 28000, 1, 0, 2, 399594, 969844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 160
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 953438, 3000, 981438, 31000, 2, 967438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 0, 0, 4, 985438, 6000, 1093594, 28000, 2, 1039516, 17000, 4, 701125, 6000, 769844, 28000, 2, 735484, 17000, 4, 769844, 10500, 782844, 23500, 2, 776344, 17000, 4, 794844, 6000, 864438, 28000, 2, 829641, 17000, 4, 864438, 10500, 877438, 23500, 2, 870938, 17000, 4, 921438, 3000, 949438, 31000, 2, 935438, 17000, 4, 1097594, 3000, 1142000, 31000, 2, 1119797, 17000, 2, 248594, 32000, 4, 701125, 6000, 701125, 28000, 1, 0, 2, 399594, 701125, 0, 4, 889438, 3000, 917438, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1280 / 188
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1039594, 3000, 1067594, 31000, 2, 1053594, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 583031, 9000, 584031, 25000, 0, 0, 0, 4, 1071594, 6000, 1093594, 28000, 2, 1082594, 17000, 4, 787281, 6000, 856000, 28000, 2, 821641, 17000, 4, 856000, 10500, 869000, 23500, 2, 862500, 17000, 4, 881000, 6000, 950594, 28000, 2, 915797, 17000, 4, 950594, 10500, 963594, 23500, 2, 957094, 17000, 4, 1007594, 3000, 1035594, 31000, 2, 1021594, 17000, 4, 1097594, 3000, 1142000, 31000, 2, 1119797, 17000, 2, 437031, 32000, 4, 787281, 6000, 787281, 28000, 1, 0, 2, 588031, 787281, 0, 4, 975594, 3000, 1003594, 31000, 0, 4, 286750, 3000, 323031, 31000], // 1280 / 254
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1747844, 6000, 1747844, 28000, 1, 0, 2, 239594, 1747844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 0
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 1560844, 3000, 1743844, 31000, 2, 1652344, 17000, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1560844, 6000, 1560844, 28000, 1, 0, 2, 239594, 1560844, 4, 1560844, 3000, 1743844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 1
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1866000, 3000, 1894000, 31000, 2, 1880000, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1898000, 6000, 1920000, 28000, 2, 1909000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1834000, 3000, 1862000, 31000, 2, 1848000, 17000, 0, 0, 0, 4, 1834000, 6000, 1834000, 28000, 1, 0, 2, 239594, 1834000, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 2
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1731438, 3000, 1759438, 31000, 2, 1745438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1763438, 6000, 1871594, 28000, 2, 1817516, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1699438, 3000, 1727438, 31000, 2, 1713438, 17000, 4, 1875594, 3000, 1920000, 31000, 2, 1897797, 17000, 0, 4, 1699438, 6000, 1699438, 28000, 1, 0, 2, 239594, 1699438, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 4
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 4, 1559531, 6000, 1628250, 28000, 2, 1593891, 17000, 4, 1628250, 10500, 1641250, 23500, 2, 1634750, 17000, 4, 1653250, 6000, 1722844, 28000, 2, 1688047, 17000, 4, 1722844, 10500, 1735844, 23500, 2, 1729344, 17000, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1559531, 6000, 1559531, 28000, 1, 0, 2, 239594, 1559531, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 8
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1715844, 6000, 1715844, 28000, 1, 0, 2, 239594, 1715844, 0, 4, 1715844, 3000, 1743844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 16
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 0, 0, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 2, 248594, 32000, 4, 1747844, 6000, 1747844, 28000, 1, 0, 2, 399594, 1747844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 32
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 423031, 9000, 424031, 25000, 0, 0, 0, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1747844, 6000, 1747844, 28000, 1, 0, 2, 428031, 1747844, 0, 0, 0, 4, 286750, 3000, 323031, 31000], // 1920 / 64
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1641844, 3000, 1669844, 31000, 2, 1655844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1673844, 6000, 1782000, 28000, 2, 1727922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1609844, 3000, 1637844, 31000, 2, 1623844, 17000, 0, 0, 0, 4, 1609844, 6000, 1609844, 28000, 1, 0, 2, 239594, 1609844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 128
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1679594, 3000, 1707594, 31000, 2, 1693594, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 583031, 9000, 584031, 25000, 0, 4, 1460594, 3000, 1643594, 31000, 2, 1552094, 17000, 4, 1711594, 6000, 1733594, 28000, 2, 1722594, 17000, 4, 1240281, 6000, 1309000, 28000, 2, 1274641, 17000, 4, 1309000, 10500, 1322000, 23500, 2, 1315500, 17000, 4, 1334000, 6000, 1403594, 28000, 2, 1368797, 17000, 4, 1403594, 10500, 1416594, 23500, 2, 1410094, 17000, 4, 1647594, 3000, 1675594, 31000, 2, 1661594, 17000, 4, 1737594, 3000, 1782000, 31000, 2, 1759797, 17000, 2, 437031, 32000, 4, 1240281, 6000, 1240281, 28000, 1, 0, 2, 588031, 1240281, 4, 1460594, 3000, 1643594, 31000, 4, 1428594, 3000, 1456594, 31000, 0, 4, 286750, 3000, 323031, 31000], // 1920 / 255
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1731438, 3000, 1759438, 31000, 2, 1745438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1763438, 6000, 1871594, 28000, 2, 1817516, 17000, 4, 1479125, 6000, 1547844, 28000, 2, 1513484, 17000, 4, 1547844, 10500, 1560844, 23500, 2, 1554344, 17000, 4, 1572844, 6000, 1642438, 28000, 2, 1607641, 17000, 4, 1642438, 10500, 1655438, 23500, 2, 1648938, 17000, 4, 1699438, 3000, 1727438, 31000, 2, 1713438, 17000, 4, 1875594, 3000, 1920000, 31000, 2, 1897797, 17000, 0, 4, 1479125, 6000, 1479125, 28000, 1, 0, 2, 239594, 1479125, 0, 4, 1667438, 3000, 1695438, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 28
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1866000, 3000, 1894000, 31000, 2, 1880000, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 1647000, 3000, 1830000, 31000, 2, 1738500, 17000, 4, 1898000, 6000, 1920000, 28000, 2, 1909000, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1834000, 3000, 1862000, 31000, 2, 1848000, 17000, 0, 0, 0, 4, 1647000, 6000, 1647000, 28000, 1, 0, 2, 239594, 1647000, 4, 1647000, 3000, 1830000, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 3
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1731438, 3000, 1759438, 31000, 2, 1745438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 1512438, 3000, 1695438, 31000, 2, 1603938, 17000, 4, 1763438, 6000, 1871594, 28000, 2, 1817516, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1699438, 3000, 1727438, 31000, 2, 1713438, 17000, 4, 1875594, 3000, 1920000, 31000, 2, 1897797, 17000, 0, 4, 1512438, 6000, 1512438, 28000, 1, 0, 2, 239594, 1512438, 4, 1512438, 3000, 1695438, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 5
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1817594, 3000, 1845594, 31000, 2, 1831594, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1849594, 6000, 1871594, 28000, 2, 1860594, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1785594, 3000, 1813594, 31000, 2, 1799594, 17000, 4, 1875594, 3000, 1920000, 31000, 2, 1897797, 17000, 0, 4, 1785594, 6000, 1785594, 28000, 1, 0, 2, 239594, 1785594, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 6
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 1560844, 3000, 1743844, 31000, 2, 1652344, 17000, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 4, 1372531, 6000, 1441250, 28000, 2, 1406891, 17000, 4, 1441250, 10500, 1454250, 23500, 2, 1447750, 17000, 4, 1466250, 6000, 1535844, 28000, 2, 1501047, 17000, 4, 1535844, 10500, 1548844, 23500, 2, 1542344, 17000, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1372531, 6000, 1372531, 28000, 1, 0, 2, 239594, 1372531, 4, 1560844, 3000, 1743844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 9
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 1560844, 3000, 1743844, 31000, 2, 1652344, 17000, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1528844, 6000, 1528844, 28000, 1, 0, 2, 239594, 1528844, 4, 1560844, 3000, 1743844, 31000, 4, 1528844, 3000, 1556844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 17
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 0, 0, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 4, 1527531, 6000, 1596250, 28000, 2, 1561891, 17000, 4, 1596250, 10500, 1609250, 23500, 2, 1602750, 17000, 4, 1621250, 6000, 1690844, 28000, 2, 1656047, 17000, 4, 1690844, 10500, 1703844, 23500, 2, 1697344, 17000, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1527531, 6000, 1527531, 28000, 1, 0, 2, 239594, 1527531, 0, 4, 1715844, 3000, 1743844, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 24
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 4, 1560844, 3000, 1743844, 31000, 2, 1652344, 17000, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 2, 248594, 32000, 4, 1560844, 6000, 1560844, 28000, 1, 0, 2, 399594, 1560844, 4, 1560844, 3000, 1743844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 33
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 423031, 9000, 424031, 25000, 0, 4, 1560844, 3000, 1743844, 31000, 2, 1652344, 17000, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 0, 4, 1560844, 6000, 1560844, 28000, 1, 0, 2, 428031, 1560844, 4, 1560844, 3000, 1743844, 31000, 0, 0, 4, 286750, 3000, 323031, 31000], // 1920 / 65
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1779844, 3000, 1807844, 31000, 2, 1793844, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 583031, 9000, 584031, 25000, 0, 0, 0, 4, 1811844, 6000, 1920000, 28000, 2, 1865922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1747844, 3000, 1775844, 31000, 2, 1761844, 17000, 0, 0, 2, 437031, 32000, 4, 1747844, 6000, 1747844, 28000, 1, 0, 2, 588031, 1747844, 0, 0, 0, 4, 286750, 3000, 323031, 31000], // 1920 / 96
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1641844, 3000, 1669844, 31000, 2, 1655844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 234594, 9000, 235594, 25000, 0, 4, 1422844, 3000, 1605844, 31000, 2, 1514344, 17000, 4, 1673844, 6000, 1782000, 28000, 2, 1727922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1609844, 3000, 1637844, 31000, 2, 1623844, 17000, 0, 0, 0, 4, 1422844, 6000, 1422844, 28000, 1, 0, 2, 239594, 1422844, 4, 1422844, 3000, 1605844, 31000, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 129
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1641844, 3000, 1669844, 31000, 2, 1655844, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 0, 0, 4, 1673844, 6000, 1782000, 28000, 2, 1727922, 17000, 0, 0, 0, 0, 0, 0, 0, 0, 4, 1609844, 3000, 1637844, 31000, 2, 1623844, 17000, 0, 0, 2, 248594, 32000, 4, 1609844, 6000, 1609844, 28000, 1, 0, 2, 399594, 1609844, 0, 0, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 160
    &[4, 61313, 3000, 89313, 31000, 2, 75313, 17000, 4, 98313, 3000, 134594, 31000, 2, 116453, 17000, 4, 138594, 3000, 166594, 31000, 2, 152594, 17000, 4, 170594, 3000, 198594, 31000, 2, 184594, 17000, 4, 202594, 3000, 230594, 31000, 2, 216594, 17000, 4, 1593438, 3000, 1621438, 31000, 2, 1607438, 17000, 4, 93313, 9000, 94313, 25000, 0, 0, 0, 4, 394594, 9000, 395594, 25000, 0, 0, 0, 4, 1625438, 6000, 1733594, 28000, 2, 1679516, 17000, 4, 1341125, 6000, 1409844, 28000, 2, 1375484, 17000, 4, 1409844, 10500, 1422844, 23500, 2, 1416344, 17000, 4, 1434844, 6000, 1504438, 28000, 2, 1469641, 17000, 4, 1504438, 10500, 1517438, 23500, 2, 1510938, 17000, 4, 1561438, 3000, 1589438, 31000, 2, 1575438, 17000, 4, 1737594, 3000, 1782000, 31000, 2, 1759797, 17000, 2, 248594, 32000, 4, 1341125, 6000, 1341125, 28000, 1, 0, 2, 399594, 1341125, 0, 4, 1529438, 3000, 1557438, 31000, 0, 4, 98313, 3000, 134594, 31000], // 1920 / 188
    &[4, 249750, 3000, 277750, 31000, 2, 263750, 17000, 4, 286750, 3000, 323031, 31000, 2, 304891, 17000, 4, 327031, 3000, 355031, 31000, 2, 341031, 17000, 4, 359031, 3000, 387031, 31000, 2, 373031, 17000, 4, 391031, 3000, 419031, 31000, 2, 405031, 17000, 4, 1679594, 3000, 1707594, 31000, 2, 1693594, 17000, 4, 281750, 9000, 282750, 25000, 0, 0, 0, 4, 583031, 9000, 584031, 25000, 0, 0, 0, 4, 1711594, 6000, 1733594, 28000, 2, 1722594, 17000, 4, 1427281, 6000, 1496000, 28000, 2, 1461641, 17000, 4, 1496000, 10500, 1509000, 23500, 2, 1502500, 17000, 4, 1521000, 6000, 1590594, 28000, 2, 1555797, 17000, 4, 1590594, 10500, 1603594, 23500, 2, 1597094, 17000, 4, 1647594, 3000, 1675594, 31000, 2, 1661594, 17000, 4, 1737594, 3000, 1782000, 31000, 2, 1759797, 17000, 2, 437031, 32000, 4, 1427281, 6000, 1427281, 28000, 1, 0, 2, 588031, 1427281, 0, 4, 1615594, 3000, 1643594, 31000, 0, 4, 286750, 3000, 323031, 31000], // 1920 / 254
];
