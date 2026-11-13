//! SolidCraft's navigation slots on the kit's navigation bar: its catalogue, icons and commands,
//! and bars measured in its viewport.
use crate::navigation::NavCaps;
use crate::navigation_bar::*;
use crate::navigation_slots::{CATALOGUE, SELECTION_MODE_ICONS};
use crate::ui_kit::nav_bar_layout::measure;
use crate::ui_kit::{NavigationEdge, NavigationKind, NavigationSize};
use egui::{Rect, pos2, vec2};
use serde_json::json;

#[test]
fn selection_mode_icons_resolve_to_real_masters() {
    crate::navigation_slots::install();
    assert_eq!(SELECTION_MODE_ICONS.len(), 6);
    for (mode, icon) in SELECTION_MODE_ICONS {
        let resolved = crate::icon_svg::resolve(icon);
        assert!(!resolved.placeholder, "{mode}: {icon}");
        assert_eq!(resolved.master, *icon);
    }
}

#[test]
fn slot_icons_follow_current_style_and_display_toggle_meanings() {
    crate::navigation_slots::install();
    let slot = find("visual_style").unwrap();
    for (style, icon) in [(0, "style_shaded_edges"), (1, "style_shaded"), (2, "style_wireframe"), (3, "style_hidden_edges")] {
        assert_eq!(slot.current_icon(style), icon);
        let resolved = crate::icon_svg::resolve(icon);
        assert!(!resolved.placeholder, "{style}: {icon}");
        assert_eq!(resolved.master, if style == 0 { "style" } else { icon });
    }
    assert_eq!(slot.current_icon(u8::MAX), "style_shaded_edges");
    assert!(!crate::icon_svg::resolve("style_translucent").placeholder);
    for id in ["display", "grid", "ground_shadow", "ambient_occlusion"] {
        assert_eq!(find(id).unwrap().current_icon(2), id);
    }
}

#[test]
fn slot_catalogue_uses_registered_commands_and_existing_icons() {
    crate::navigation_slots::install();
    let mut ids = std::collections::BTreeSet::new();
    for slot in CATALOGUE {
        assert!(ids.insert(slot.id), "duplicate catalogue id {}", slot.id);
        match slot.command {
            Some(command) => assert!(solidcraft_engine::find_command(command).is_some(), "{}", slot.id),
            None => assert!(["projection", "display", "separator"].contains(&slot.id)),
        }
        if slot.kind != NavigationKind::Separator {
            assert!(!crate::icon_svg::resolve(slot.icon).placeholder, "{}: {}", slot.id, slot.icon);
        }
    }
}

#[test]
fn measured_bar_drops_labels_then_overflows_in_order_without_compass_overlap() {
    crate::navigation_slots::install();
    let prefs = NavigationBar { labels: Labels::Labels, ..Default::default() };
    for width in [5.0, 39.0, 46.0, 90.0, 160.0, 250.0, 310.0, 700.0, 1400.0] {
        for height in [30.0, 80.0, 130.0, 240.0, 720.0] {
            let canvas = Rect::from_min_size(pos2(20.0, 30.0), vec2(width, height));
            let bar = measure(canvas, &prefs, |slot| NavCaps::MODEL.shows(slot), |_| 50.0);
            let layout = crate::viewport_layout::Layout::with_content(
                canvas,
                canvas,
                &[],
                1.0,
                crate::prefs::NavigatorStyle::Cube,
                egui::Vec2::ZERO,
                bar.rect,
            );
            assert!(!layout.axis_view.intersect(bar.rect).is_positive());
            if !bar.rect.is_positive() {
                continue;
            }
            assert!(canvas.contains_rect(bar.rect));
            assert_eq!(bar.rect.height(), crate::ui_kit::NavigationMetrics::for_size(prefs.size).height());
            let mut previous = bar.rect.left();
            for (_, rect) in &bar.slots {
                assert!(bar.rect.contains_rect(*rect));
                assert!(rect.left() >= previous);
                previous = rect.right();
            }
            if bar.more.is_positive() {
                assert!(!bar.labels);
                assert!(bar.rect.contains_rect(bar.more));
                assert!(bar.more.left() >= previous);
            }
            let order: Vec<_> = bar.slots.iter().map(|(slot, _)| slot.id).chain(bar.overflow.iter().map(|slot| slot.id)).collect();
            assert_eq!(order, prefs.slots);
        }
    }
    let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 720.0));
    assert!(measure(wide, &prefs, |slot| NavCaps::MODEL.shows(slot), |_| 50.0).labels);
    let middle = Rect::from_min_size(pos2(0.0, 0.0), vec2(380.0, 720.0));
    let bar = measure(middle, &prefs, |slot| NavCaps::MODEL.shows(slot), |_| 50.0);
    assert!(!bar.labels && bar.overflow.is_empty(), "text disappears before slots fold");
}

#[test]
fn measured_bar_honours_size_position_and_ignores_unknown_slots() {
    crate::navigation_slots::install();
    let canvas = Rect::from_min_size(pos2(20.0, 30.0), vec2(700.0, 500.0));
    for size in [NavigationSize::Compact, NavigationSize::Regular] {
        for position in [Position::Centre, Position::Left, Position::Right] {
            let prefs: NavigationBar = serde_json::from_value(json!({"slots":["future", "pan", "fit"], "size":size, "position":position})).unwrap();
            let bar = measure(canvas, &prefs, |slot| NavCaps::MODEL.shows(slot), |_| 0.0);
            assert_eq!(bar.slots.iter().map(|(slot, _)| slot.id).collect::<Vec<_>>(), ["pan", "fit"]);
            assert!(bar.slots.iter().all(|(_, rect)| rect.height() == size.points()));
            match position {
                Position::Centre => assert_eq!(bar.rect.center().x, canvas.center().x),
                Position::Left => assert_eq!(bar.rect.left(), canvas.left() + crate::ui_kit::NAV_EDGE),
                Position::Right => assert_eq!(bar.rect.right(), canvas.right() - crate::ui_kit::NAV_EDGE),
            }
        }
    }
}

#[test]
fn a_bar_on_the_top_edge_sits_under_the_top_and_keeps_the_corner_controls_below_it() {
    crate::navigation_slots::install();
    use crate::ui_kit::NAV_BOTTOM;
    // Older settings have no edge and keep the bar at the bottom; a bad value recovers the same way.
    let old: NavigationBar = serde_json::from_value(json!({"position": "right"})).unwrap();
    assert_eq!(old.edge, NavigationEdge::Bottom);
    let bad: NavigationBar = serde_json::from_value(json!({"edge": "sideways"})).unwrap();
    assert_eq!(bad.edge, NavigationEdge::Bottom);
    assert!(validate(&json!({"edge": "sideways"})).is_err());
    assert!(validate(&json!({"edge": "top"})).is_ok());
    let canvas = Rect::from_min_size(pos2(20.0, 30.0), vec2(900.0, 600.0));
    for position in [Position::Centre, Position::Left, Position::Right] {
        for edge in [NavigationEdge::Bottom, NavigationEdge::Top] {
            let prefs: NavigationBar = serde_json::from_value(json!({"position": position, "edge": edge})).unwrap();
            assert_eq!(serde_json::to_value(&prefs).unwrap()["edge"], if edge == NavigationEdge::Top { "top" } else { "bottom" });
            let bar = measure(canvas, &prefs, |slot| NavCaps::MODEL.shows(slot), |_| 50.0);
            match edge {
                NavigationEdge::Bottom => assert_eq!(bar.rect.bottom(), canvas.bottom() - NAV_BOTTOM),
                NavigationEdge::Top => assert_eq!(bar.rect.top(), canvas.top() + NAV_BOTTOM),
            }
            for corner in [egui::Vec2::ZERO, vec2(120.0, 32.0)] {
                let layout =
                    crate::viewport_layout::Layout::with_content(canvas, canvas, &[], 1.0, crate::prefs::NavigatorStyle::Cube, corner, bar.rect);
                // The axis view keeps its full size: on top, it moves down past the bar instead.
                assert_eq!(layout.axis_view.width(), crate::viewport::axis_view_size(crate::prefs::NavigatorStyle::Cube));
                assert!(!layout.axis_view.intersect(bar.rect).is_positive(), "{position:?} {edge:?}");
                assert!(!layout.confirm.intersect(bar.rect).is_positive(), "{position:?} {edge:?}");
                assert!(!layout.confirm.intersect(layout.axis_view).is_positive(), "{position:?} {edge:?}");
            }
        }
    }
}

#[test]
fn navigation_slot_visuals_never_use_focus_as_a_fill() {
    crate::navigation_slots::install();
    use crate::ui_kit::{NavigationFill, navigation_visual};
    for kind in [NavigationKind::Action, NavigationKind::Tool, NavigationKind::Toggle, NavigationKind::Menu] {
        for keyboard_focus in [false, true] {
            assert_eq!(
                navigation_visual(kind, false, false, false, keyboard_focus),
                (NavigationFill::None, keyboard_focus),
                "{kind:?}: idle or released"
            );
            for (hovered, pressed) in [(true, false), (false, true), (true, true)] {
                assert_eq!(
                    navigation_visual(kind, false, hovered, pressed, keyboard_focus),
                    (if pressed { NavigationFill::Press } else { NavigationFill::Hover }, keyboard_focus),
                    "{kind:?}: hover or press"
                );
            }
            let active_fill = match kind {
                NavigationKind::Action => NavigationFill::None,
                NavigationKind::Menu => NavigationFill::Press,
                _ => NavigationFill::Active,
            };
            assert_eq!(
                navigation_visual(kind, true, false, false, keyboard_focus),
                (active_fill, keyboard_focus),
                "{kind:?}: active tool/toggle or open menu"
            );
        }
    }
}

#[test]
fn navigation_slot_pointer_clicks_release_focus_and_leave_no_fill() {
    crate::navigation_slots::install();
    use crate::ui_kit::{NavigationButton, navigation_button};
    use egui::{Color32, Event, RawInput, Shape};
    use solidcraft_theme::Tokens;

    fn fill(output: &egui::FullOutput, bounds: Rect) -> Option<Color32> {
        let mut fill = None;
        for shape in &output.shapes {
            crate::ui_kit::test_support::each_shape(&shape.shape, &mut |shape| {
                if let Shape::Rect(rect) = shape
                    && rect.rect == bounds
                    && rect.fill != Color32::TRANSPARENT
                {
                    fill = Some(rect.fill);
                }
            });
        }
        fill
    }

    for kind in [NavigationKind::Action, NavigationKind::Tool, NavigationKind::Toggle] {
        let ctx = crate::ui_kit::test_support::context();
        let bounds = Rect::from_min_size(pos2(20.0, 30.0), vec2(26.0, 26.0));
        let draw = |events, selected| {
            let mut focused = false;
            let mut clicked = false;
            let output = crate::ui_kit::test_support::raw_frame(
                &ctx,
                RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 400.0))),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    let response = navigation_button(
                        ui,
                        bounds,
                        NavigationButton { id: "regression", label: "Fit", icon: "fit", selected, kind, labels: false, menu: false },
                    );
                    focused = response.has_focus();
                    clicked = response.clicked();
                },
            );
            (output, focused, clicked)
        };
        draw(vec![], false);
        draw(vec![Event::PointerMoved(bounds.center())], false);
        let pressed = draw(crate::ui_kit::test_support::pointer(bounds.center(), true), false);
        assert_eq!(fill(&pressed.0, bounds), Some(Tokens::get().nav_press), "{kind:?}: pressed");
        let released = draw(crate::ui_kit::test_support::pointer(bounds.center(), false), false);
        assert!(released.2, "{kind:?}: real pointer activation");
        assert!(!released.1, "{kind:?}: pointer activation must surrender focus");
        let away = draw(vec![Event::PointerMoved(pos2(200.0, 200.0))], false);
        assert_eq!(fill(&away.0, bounds), None, "{kind:?}: no lingering fill");
        if kind != NavigationKind::Action {
            let active = draw(vec![], true);
            assert_eq!(fill(&active.0, bounds), Some(Tokens::get().selected_bg));
            draw(crate::ui_kit::test_support::pointer(bounds.center(), true), true);
            let released = draw(crate::ui_kit::test_support::pointer(bounds.center(), false), true);
            assert!(released.2 && !released.1);
            let off = draw(vec![Event::PointerMoved(pos2(200.0, 200.0))], false);
            assert_eq!(fill(&off.0, bounds), None, "{kind:?}: tool/toggle released");
        }
    }
}

#[test]
fn navigation_list_operations_check_indices_caps_duplicates_and_separators() {
    crate::navigation_slots::install();
    let mut bar = NavigationBar { slots: Vec::new(), ..Default::default() };
    assert!(bar.insert(1, "pan").is_err());
    assert!(bar.insert(0, "unknown").is_err());
    bar.insert(0, "pan").unwrap();
    bar.insert(0, "fit").unwrap();
    bar.insert(2, "home").unwrap();
    assert_eq!(bar.slots, ["fit", "pan", "home"]);
    assert!(bar.insert(0, "pan").is_err());
    assert_eq!(bar.move_to(0, 3).unwrap(), 2);
    assert_eq!(bar.slots, ["pan", "home", "fit"]);
    assert_eq!(bar.move_to(2, 0).unwrap(), 0);
    assert_eq!(bar.move_to(0, 1).unwrap(), 0);
    let before = bar.clone();
    for (from, gap) in [(usize::MAX, 0), (0, usize::MAX), (3, 0)] {
        assert!(bar.move_to(from, gap).is_err());
        assert_eq!(bar, before);
    }
    assert!(bar.remove(usize::MAX).is_err());
    bar.remove(1).unwrap();
    assert_eq!(bar.slots, ["fit", "home"]);
    for _ in 0..MAX_SEPARATORS {
        bar.insert(0, "separator").unwrap();
    }
    assert!(bar.insert(0, "separator").is_err());
    for slot in CATALOGUE {
        if bar.slots.len() == MAX_SLOTS {
            break;
        }
        if slot.id != "separator" && !bar.slots.iter().any(|id| id == slot.id) {
            bar.insert(bar.slots.len(), slot.id).unwrap();
        }
    }
    assert_eq!(bar.slots.len(), MAX_SLOTS);
    assert_eq!(bar.can_insert("pan"), Err("navbar.custom.full"));
    assert!(bar.insert(0, "pan").is_err());
    while !bar.slots.is_empty() {
        bar.remove(0).unwrap();
    }
    assert!(bar.remove(0).is_err());
    assert!(bar.move_to(0, 0).is_err());
}

#[test]
fn navigation_menu_options_round_trip_and_old_lists_load() {
    crate::navigation_slots::install();
    let old: NavigationBar = serde_json::from_value(json!({"slots":["visual_style","display","future"]})).unwrap();
    assert_eq!(old.slots, ["visual_style", "display", "future"]);
    assert_eq!(old.menu_display("visual_style"), MenuDisplay::IconOnly);
    assert_eq!(old.menu_display("selection_mode"), MenuDisplay::IconAndValue);
    assert_eq!(old.menu_display("render_mode"), MenuDisplay::IconAndValue);
    let value = json!({"slots":["visual_style","display"], "slot_options":{"visual_style":{"shows":"icon_and_value"}}});
    validate(&value).unwrap();
    let bar: NavigationBar = serde_json::from_value(value).unwrap();
    assert_eq!(bar.menu_display("visual_style"), MenuDisplay::IconAndValue);
    assert_eq!(serde_json::from_value::<NavigationBar>(serde_json::to_value(&bar).unwrap()).unwrap(), bar);
    for bad in [json!(null), json!([]), json!(true), json!({"shows":"future"}), json!({"shows":[]} )] {
        let bar: NavigationBar = serde_json::from_value(json!({"slot_options":{"visual_style":bad}})).unwrap();
        assert_eq!(bar.menu_display("visual_style"), MenuDisplay::IconOnly);
    }
    assert!(validate(&json!({"slot_options":[]})).is_err());
    assert!(validate(&json!({"slot_options":{"visual_style":{"shows":"future"}}})).is_err());
    assert!(validate(&json!({"slot_options":{"visual_style":{"hostile":1}}})).is_err());
}

#[test]
fn navigation_drop_index_handles_edges_empty_and_hostile_coordinates() {
    crate::navigation_slots::install();
    use crate::ui_kit::drop_index;
    for (x, gap) in [(-100.0, 0), (9.0, 0), (10.0, 1), (29.0, 1), (30.0, 2), (100.0, 2), (f32::NAN, 2), (f32::INFINITY, 2)] {
        assert_eq!(drop_index(x, [10.0, 30.0]), gap);
    }
    assert_eq!(drop_index(0.0, []), 0);
}

#[test]
fn navigation_menu_value_width_is_independent_of_global_button_labels() {
    crate::navigation_slots::install();
    let mut bar = NavigationBar { slots: vec!["visual_style".into(), "display".into()], ..Default::default() };
    let content = Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 200.0));
    let before = measure(content, &bar, |slot| NavCaps::MODEL.shows(slot), |_| 100.0);
    bar.slot_options.insert("visual_style".into(), SlotOptions { shows: MenuDisplay::IconAndValue });
    let value = measure(content, &bar, |slot| NavCaps::MODEL.shows(slot), |_| 100.0);
    assert_eq!(value.slots[0].1.width(), before.slots[0].1.width() + 100.0 + crate::ui_kit::NAV_MENU_GAP);
    assert_eq!(value.slots[1].1.width(), before.slots[1].1.width());
    bar.labels = Labels::Labels;
    let labels = measure(content, &bar, |slot| NavCaps::MODEL.shows(slot), |_| 100.0);
    assert_eq!(labels.slots.iter().map(|(_, rect)| *rect).collect::<Vec<_>>(), value.slots.iter().map(|(_, rect)| *rect).collect::<Vec<_>>());
}

#[test]
fn navigation_drop_adds_moves_removes_and_rejects_stale_sources() {
    crate::navigation_slots::install();
    let mut bar = NavigationBar { slots: vec!["separator".into(), "pan".into(), "separator".into()], ..Default::default() };
    let original = bar.slots.clone();
    assert_eq!(bar.drop_slot(&original, Some(2), "separator", Some(0)).unwrap(), Some(0));
    assert_eq!(bar.slots, ["separator", "separator", "pan"]);
    let before = bar.clone();
    assert!(bar.drop_slot(&original, Some(1), "pan", None).is_err());
    assert_eq!(bar, before);
    assert!(bar.drop_slot(&bar.slots.clone(), Some(0), "pan", None).is_err());
    let original = bar.slots.clone();
    assert_eq!(bar.drop_slot(&original, None, "fit", Some(3)).unwrap(), Some(3));
    let original = bar.slots.clone();
    assert_eq!(bar.drop_slot(&original, Some(3), "fit", None).unwrap(), None);
    assert_eq!(bar, before);
    assert_eq!(bar.drop_slot(&bar.slots.clone(), None, "fit", None).unwrap(), None);
}

#[test]
fn navigation_board_metrics_and_state_colors_are_shared_in_both_themes() {
    crate::navigation_slots::install();
    use crate::ui_kit::*;
    assert_eq!(NavigationSize::default(), NavigationSize::Regular);
    for (size, button, icon, height, radius, button_radius) in
        [(NavigationSize::Regular, 32.0, 18.0, 40.0, 10, 7), (NavigationSize::Compact, 26.0, 16.0, 34.0, 9, 6)]
    {
        let metrics = NavigationMetrics::for_size(size);
        assert_eq!(
            (metrics.button, metrics.icon, metrics.height(), metrics.radius, metrics.button_radius),
            (button, icon, height, radius, button_radius)
        );
    }
    assert_eq!((NAV_BORDER, NAV_PADDING, NAV_GAP, NAV_BOTTOM, NAV_EDGE), (1.0, 3.0, 2.0, 7.0, 12.0));
    assert_eq!((NAV_SEPARATOR, NAV_SEPARATOR_HEIGHT, NAV_MENU_LEFT, NAV_MENU_RIGHT, NAV_MENU_GAP, NAV_CHEVRON), (9.0, 18.0, 7.0, 6.0, 5.0, 12.0));
    assert_eq!((NAV_STAGE_HEIGHT, NAV_STAGE_INSET, NAV_TILE_HEIGHT, NAV_TILE_ADD), (96.0, 12.0, 32.0, 22.0));
    assert_eq!(
        (NAV_MARKER_HEIGHT, NAV_GHOST_OPACITY, NAV_TILE_DRAG_OPACITY, NAV_IDLE_OPACITY, NAV_FADE_SECONDS, NAV_TOOLTIP_DELAY),
        (24.0, 0.7, 0.55, 0.45, 0.12, 0.8)
    );
    for tokens in [solidcraft_theme::Tokens::light(), solidcraft_theme::Tokens::dark()] {
        let _theme = solidcraft_theme::scoped_tokens(tokens);
        assert_eq!(navigation_fill(&tokens, NavigationFill::None), None);
        assert_eq!(navigation_fill(&tokens, NavigationFill::Hover), Some(tokens.nav_hover));
        assert_eq!(navigation_fill(&tokens, NavigationFill::Press), Some(tokens.nav_press));
        assert_eq!(navigation_fill(&tokens, NavigationFill::Active), Some(tokens.selected_bg));
        let frame = navigation_popup_frame();
        assert_eq!(frame.fill, tokens.dialog_body);
        assert_eq!(frame.stroke, egui::Stroke::new(NAV_BORDER, tokens.dialog_border));
        assert_eq!(frame.corner_radius, egui::CornerRadius::same(10));
        assert_eq!(frame.inner_margin, egui::Margin::same(5));
        assert_eq!(frame.shadow, Container::menu_frame().shadow, "navigation inherits the shared popup elevation");
    }
    assert_eq!((NAV_POPUP_WIDTH, NAV_CONTEXT_WIDTH, NAV_MORE_WIDTH, NAV_POPUP_GAP), (244.0, 232.0, 200.0, 6.0));
    for (idle, opacity) in [(NavigationIdle::Always, 1.0), (NavigationIdle::Fade, 0.45), (NavigationIdle::Hide, 0.0)] {
        assert_eq!(navigation_opacity(idle, false), opacity);
        assert_eq!(navigation_opacity(idle, true), 1.0);
    }
}

#[test]
fn navigation_board_popup_width_includes_its_border_and_padding() {
    crate::navigation_slots::install();
    use crate::ui_kit::*;
    let ctx = crate::ui_kit::test_support::context();
    for tokens in [solidcraft_theme::Tokens::light(), solidcraft_theme::Tokens::dark()] {
        let _theme = solidcraft_theme::scoped_tokens(tokens);
        for width in [NAV_POPUP_WIDTH, NAV_CONTEXT_WIDTH, NAV_MORE_WIDTH] {
            let mut outer = Rect::NOTHING;
            let mut row = Rect::NOTHING;
            crate::ui_kit::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
                outer = ui
                    .scope(|ui| {
                        navigation_menu(ui, width, |ui| {
                            row = NavigationMenuRow::new("fit", "Fit", "fit").show(ui).rect;
                        })
                    })
                    .response
                    .rect;
            });
            assert_eq!(outer.width(), width);
            assert_eq!(row.height(), NAV_MENU_ROW);
            assert_eq!(row.left() - outer.left(), f32::from(NAV_POPUP_PADDING) + NAV_BORDER);
            assert_eq!(outer.right() - row.right(), f32::from(NAV_POPUP_PADDING) + NAV_BORDER);
        }
    }
}

/// A label longer than its menu row (the Display menu's "Perspective while orbiting, ortho on
/// faces", #8700) is cut with an ellipsis before the row's check, not drawn past the menu's edge;
/// a short one is drawn whole.
#[test]
fn a_long_menu_row_label_ends_in_an_ellipsis_inside_its_row() {
    crate::navigation_slots::install();
    use crate::ui_kit::*;
    let ctx = crate::ui_kit::test_support::context();
    for (label, selected, cut) in [
        ("Perspective while orbiting, ortho on faces", false, true),
        ("Perspective while orbiting, ortho on faces", true, true),
        ("Show tool at cursor", true, false),
    ] {
        let mut row = Rect::NOTHING;
        let output = crate::ui_kit::test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
            navigation_menu(ui, NAV_POPUP_WIDTH, |ui| {
                let mut item = NavigationMenuRow::new("row", label, "select");
                item.selected = selected;
                row = item.show(ui).rect;
            });
        });
        let text = crate::ui_kit::test_support::text_shapes(&output).find(|t| t.galley.text() == label).unwrap_or_else(|| panic!("{label} is drawn"));
        assert_eq!(text.galley.elided, cut, "{label}");
        let end = row.right() - NAV_MENU_INSET - if selected { solidcraft_theme::Tokens::get().space_14 } else { 0.0 };
        let drawn = text.pos.x + text.galley.size().x;
        assert!(drawn <= end, "{label}: ends at {drawn}, past {end}");
    }
}

#[test]
fn navigation_board_position_aliases_migrate_and_editor_alignment_follows_position() {
    crate::navigation_slots::install();
    use crate::ui_kit::nav_bar_layout::editor_layout;
    use crate::ui_kit::*;
    let stage = Rect::from_min_size(pos2(100.0, 200.0), vec2(640.0, NAV_STAGE_HEIGHT));
    for (legacy, current, position) in
        [("bottom", "centre", Position::Centre), ("bottom_left", "left", Position::Left), ("bottom_right", "right", Position::Right)]
    {
        let bar: NavigationBar = serde_json::from_value(json!({"position":legacy,"slots":["orbit","pan","fit","look_at","visual_style"]})).unwrap();
        assert_eq!(bar.position, position);
        assert_eq!(serde_json::to_value(&bar).unwrap()["position"], current);
        for size in [NavigationSize::Compact, NavigationSize::Regular] {
            let bar = NavigationBar { size, ..bar.clone() };
            let preview = editor_layout(stage, &bar, |_| 0.0);
            let live = measure(stage, &bar, |slot| NavCaps::MODEL.shows(slot), |_| 0.0);
            assert_eq!(preview.rect.bottom(), stage.bottom() - NAV_BORDER - NAV_STAGE_INSET);
            let top = editor_layout(stage, &NavigationBar { edge: NavigationEdge::Top, ..bar.clone() }, |_| 0.0);
            assert_eq!(top.rect.top(), stage.top() + NAV_BORDER + NAV_STAGE_INSET);
            assert_eq!(preview.rect.height(), NavigationMetrics::for_size(size).height());
            match position {
                Position::Centre => assert_eq!(preview.rect.center().x, stage.center().x),
                Position::Left => assert_eq!(preview.rect.left(), stage.left() + NAV_BORDER + NAV_STAGE_INSET),
                Position::Right => assert_eq!(preview.rect.right(), stage.right() - NAV_BORDER - NAV_STAGE_INSET),
            }
            assert_eq!(preview.slots.iter().map(|(slot, _)| slot.id).collect::<Vec<_>>(), bar.slots);
            for ((slot, rect), (live_slot, live_rect)) in preview.slots.iter().zip(&live.slots) {
                assert_eq!(slot.id, live_slot.id);
                assert_eq!(rect.width(), live_rect.width() + if slot.kind == NavigationKind::Menu { 2.0 * NAV_BORDER } else { 0.0 });
            }
        }
    }
}

#[test]
fn navigation_board_drops_menu_value_text_before_folding_slots() {
    crate::navigation_slots::install();
    let mut bar = NavigationBar { slots: vec!["pan".into(), "visual_style".into(), "display".into()], ..Default::default() };
    bar.slot_options.insert("visual_style".into(), SlotOptions { shows: MenuDisplay::IconAndValue });
    let wide = measure(Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 200.0)), &bar, |slot| NavCaps::MODEL.shows(slot), |_| 120.0);
    assert!(wide.values && wide.overflow.is_empty());
    let narrow = measure(Rect::from_min_size(egui::Pos2::ZERO, vec2(220.0, 200.0)), &bar, |slot| NavCaps::MODEL.shows(slot), |_| 120.0);
    assert!(!narrow.values && narrow.overflow.is_empty());
    let folded = measure(Rect::from_min_size(egui::Pos2::ZERO, vec2(100.0, 200.0)), &bar, |slot| NavCaps::MODEL.shows(slot), |_| 120.0);
    assert!(!folded.values && !folded.overflow.is_empty());
    assert_eq!(folded.more.size(), vec2(32.0, 32.0));
}
