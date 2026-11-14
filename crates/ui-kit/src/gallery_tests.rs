//! Table-driven layout/paint references in both themes. No window or GPU required. Every
//! specimen here is drawn from plain data; a host's own gallery (SolidCraft's
//! `kit_gallery_tests`) adds the specimens drawn from its data and checks them with
//! [`check_elements`].
use super::preferences_figures::{ThemeSample, paint_theme_thumbnail};
use super::*;
use egui::{Ui, vec2};
use solidcraft_theme::Tokens;

/// The editable state a specimen draws: two values, a selection and a check.
pub struct Sample {
    pub value: String,
    pub second: String,
    pub selected: usize,
    pub checked: bool,
}

/// The style names a host passes to [`text_emphasis`].
const SAMPLE_EMPHASIS: EmphasisLabels<'static> = EmphasisLabels { bold: "Bold", italic: "Italic", no_bold: "This font has no bold face" };

/// The paging arrows' names a host passes to [`theme_browser`].
const SAMPLE_ARROWS: PageLabels<'static> = PageLabels { previous: "Previous themes", next: "Next themes" };

/// One gallery specimen: its name, the published rect it fills, the handle inside it, and how
/// to draw it.
pub struct Element {
    pub name: &'static str,
    pub rect: &'static str,
    pub handle: Option<&'static str>,
    pub draw: fn(&mut Ui, &mut Sample),
}

/// A gallery [`Element`]: `element!(name, rect, handle, draw)`, or `element!(name, draw)` for one
/// that fills the gallery's allocation.
#[macro_export]
macro_rules! element {
    ($name:literal, $rect:literal, $handle:expr, $draw:expr) => {
        $crate::gallery::Element { name: $name, rect: $rect, handle: $handle, draw: $draw }
    };
    ($name:literal, $draw:expr) => {
        $crate::element!($name, "gallery:allocation", None, $draw)
    };
}

#[cfg(any(test, feature = "test-support"))]
fn navigation_sample(ui: &mut Ui, size: NavigationSize) {
    let metrics = NavigationMetrics::for_size(size);
    let inset = NAV_PADDING + NAV_BORDER;
    let menu_width = metrics.icon + NAV_MENU_LEFT + NAV_MENU_RIGHT + NAV_MENU_GAP + NAV_CHEVRON;
    let (rect, _) =
        ui.allocate_exact_size(vec2(2.0 * metrics.button + menu_width + 2.0 * NAV_GAP + 2.0 * inset, metrics.height()), egui::Sense::hover());
    navigation_surface(ui, rect, size, true);
    let mut x = rect.left() + inset;
    for (id, icon, kind, selected, width) in [
        ("pan", "pan", NavigationKind::Tool, true, metrics.button),
        ("fit", "fit", NavigationKind::Action, false, metrics.button),
        ("visual_style", "style_shaded_edges", NavigationKind::Menu, false, menu_width),
    ] {
        let bounds = egui::Rect::from_min_size(egui::pos2(x, rect.top() + inset), vec2(width, metrics.button));
        navigation_button(ui, bounds, NavigationButton { id, icon, label: id, kind, selected, labels: false, menu: kind == NavigationKind::Menu });
        if id == "fit" {
            navigation_edit_outline(ui, bounds, true, false);
        }
        x += width + NAV_GAP;
    }
}

// One entry per element/variant: names, rendering and geometry expectations stay together.
#[cfg(any(test, feature = "test-support"))]
pub const ELEMENTS: &[Element] = &[
    element!("theme_mode_segments", |ui, _| {
        theme_segments(
            ui,
            "gallery_mode",
            &[ThemeSegment::new("One theme", ThemeGlyph::Monitor, None), ThemeSegment::new("Follow system", ThemeGlyph::Contrast, None)],
            Some(1),
            false,
        );
    }),
    element!("theme_imported_filter", |ui, _| {
        theme_segments(ui, "gallery_imported", &[ThemeSegment::new("Imported", ThemeGlyph::None, Some(2))], Some(0), false);
    }),
    element!("theme_toolbar", |ui, _| {
        theme_toolbar(
            ui,
            "gallery_toolbar",
            68.0,
            0.0,
            |ui| {
                theme_segments(
                    ui,
                    "gallery_tone",
                    &[ThemeSegment::new("Light", ThemeGlyph::Sun, Some(9)), ThemeSegment::new("Dark", ThemeGlyph::Moon, Some(17))],
                    Some(0),
                    false,
                );
            },
            |ui| {
                theme_segments(
                    ui,
                    "gallery_view",
                    &[ThemeSegment::new("Carousel", ThemeGlyph::Carousel, None), ThemeSegment::new("Grid", ThemeGlyph::Grid, None)],
                    Some(0),
                    true,
                );
            },
        );
    }),
    element!("theme_browser_grid", |ui, _| {
        theme_browser(ui, "gallery_grid", 14, true, None, &SAMPLE_ARROWS, |ui, _, width| {
            theme_card(
                ui,
                &ThemeCard {
                    name: "Light",
                    note: "Built in",
                    options: None,
                    sample: ThemeSample::from_tokens(&Tokens::light()),
                    selected: false,
                    dark: false,
                    high: false,
                    day: false,
                    night: false,
                },
                width,
            );
        });
    }),
    element!("theme_segments", |ui, _| {
        theme_segments(
            ui,
            "gallery",
            &[ThemeSegment::new("Light", ThemeGlyph::Sun, Some(10)), ThemeSegment::new("Dark", ThemeGlyph::Moon, Some(17))],
            Some(0),
            false,
        );
    }),
    element!("theme_import_button", |ui, _| {
        theme_import_button(ui, "Import theme file…");
    }),
    element!("theme_slot", |ui, _| {
        theme_slot(
            ui,
            &ThemeSlot {
                title: "Day theme",
                name: "Light",
                hint: "Pick from below",
                aimed: true,
                night: false,
                width: ui.available_width(),
                sample: ThemeSample::from_tokens(&Tokens::light()),
            },
        );
    }),
    element!("theme_card", |ui, _| {
        theme_card(
            ui,
            &ThemeCard {
                name: "Dark",
                note: "Built in · High contrast",
                options: Some("Has options"),
                sample: ThemeSample::from_tokens(&Tokens::dark()),
                selected: true,
                dark: true,
                high: true,
                day: true,
                night: true,
            },
            118.0,
        );
    }),
    element!("theme_empty", |ui, _| {
        theme_empty(ui, "No theme matches these filters.", "Clear filters", || {});
    }),
    element!("theme_browser", |ui, _| {
        theme_browser(ui, "gallery_browser", 9, false, None, &SAMPLE_ARROWS, |ui, i, width| {
            theme_card(
                ui,
                &ThemeCard {
                    name: if i % 2 == 0 { "Light" } else { "Dark" },
                    note: "Built in",
                    options: None,
                    sample: ThemeSample::from_tokens(&Tokens::get()),
                    selected: i == 0,
                    dark: i % 2 != 0,
                    high: false,
                    day: false,
                    night: false,
                },
                width,
            );
        });
    }),
    element!("viewport_status_item", "viewport_status:sample:item", Some("viewport_status:sample:close"), |ui, _| {
        let item = StatusItem {
            id: "sample".into(),
            kind: StatusKind::Mode,
            name: "Draft".into(),
            setting: "Z · 1°".into(),
            has_key: true,
            editable: false,
        };
        viewport_status_item(ui, &item, &mut StatusPresentation::default());
    }),
    element!("sphere_thumbnail", |ui, _| {
        ui.horizontal(|ui| {
            for kind in [FinishKind::Matte, FinishKind::Satin, FinishKind::Gloss, FinishKind::Metal, FinishKind::Glass] {
                sphere_thumbnail(
                    ui,
                    &format!("gallery:sphere:{kind:?}"),
                    Sphere { color: Tokens::get().render_accent, kind, opacity: None },
                    Tokens::get().space_24 * 2.0,
                );
            }
        });
    }),
    element!("look_chain", "gallery:chain", Some("gallery:chain:body"), |ui, _| {
        let t = Tokens::get();
        look_chain(
            ui,
            "gallery:chain",
            ["Face", "Body", "Component", "Material"],
            [
                None,
                Some(Sphere { color: t.render_accent, kind: FinishKind::Satin, opacity: None }),
                None,
                Some(Sphere { color: t.control_border, kind: FinishKind::Metal, opacity: None }),
            ],
            LookStep::Body,
        );
    }),
    element!("look_chain_empty", |ui, _| {
        look_chain(ui, "gallery:empty-chain", ["Face", "Body", "Component", "Material"], [None; 4], LookStep::Face);
    }),
    element!("navigation_regular", |ui, _| navigation_sample(ui, NavigationSize::Regular)),
    element!("navigation_compact", |ui, _| navigation_sample(ui, NavigationSize::Compact)),
    element!("navigation_popup", |ui, _| {
        navigation_menu(ui, NAV_POPUP_WIDTH, |ui| {
            navigation_menu_heading(ui, "Visual style");
            let mut row = NavigationMenuRow::new("shaded", "Shaded", "style_shaded");
            row.selected = true;
            row.show(ui);
            navigation_menu_separator(ui);
            NavigationMenuRow::new("fit", "Fit", "fit").show(ui);
            let mut later = NavigationMenuRow::new("translucent", "Translucent", "style_translucent");
            later.enabled = false;
            later.detail = Some("Later");
            later.show(ui);
        });
    }),
    element!("navigation_settings", |ui, state| {
        settings_section(ui, "gallery_navigation", "Navigation bar", |ui| {
            text(ui, "Choose the controls on the bar at the bottom of the 3D view.", Text::Hint("gallery_navigation"));
            let width = ui.available_width();
            navigation_stage(ui, width);
            settings_card(ui, |ui| {
                text(ui, "Fit", Text::NameCell(true));
                text(ui, "Action · Runs once and never stays lit.", Text::Hint("gallery_navigation_action"));
            });
            navigation_tile(ui, "grid", "Grid", "style", false);
            row(ui, "Position", |ui| {
                segmented(ui, &mut state.selected, &["Left", "Center", "Right"], Default::default());
            });
            checkbox(ui, "Show navigation bar", &mut state.checked);
            button(ui, "Reset to default", Button::Normal);
        });
    }),
    element!("linear_color_field", |ui, _| {
        row(ui, "Zenith", |ui| linear_color_field(ui, &mut [0.5, 0.64, 0.81], 1e6));
    }),
    element!("section_method", |ui, _| {
        section_method(ui, "gallery_method", "Atmosphere", true, "GPU", |ui| {
            text(ui, "Participating medium", Text::Inline);
        });
    }),
    element!("segments_availability", |ui, state| {
        segmented(
            ui,
            &mut state.selected,
            &["World", "Colour", "Gradient", "Clear"],
            Segments { enabled: &[false, true, false, true], compact: true, ..Default::default() },
        );
    }),
    element!("read_only_text", |ui, _| {
        read_only_text(ui, "{\n  \"samples\": 256\n}");
    }),
    element!("method_section_sampling", |ui, state| {
        method_section(ui, "gallery_method", "Path tracing", "GPU", Some("Needs GPU path tracing"), |ui| {
            checkbox(ui, "Reduce noise", &mut state.checked);
        });
    }),
    element!("icon_action", |ui, _| {
        icon_action(ui, "Duplicate", "copy", true, false);
        icon_action(ui, "Remove", "delete", true, true);
    }),
    element!("aspect_grid", |ui, _| {
        aspect_grid(
            ui,
            &[
                AspectTile { id: "gallery:aspect:wide", label: "16:9", aspect: 16.0 / 9.0, selected: true },
                AspectTile { id: "gallery:aspect:square", label: "1:1", aspect: 1.0, selected: false },
                AspectTile { id: "gallery:aspect:tall", label: "9:16", aspect: 9.0 / 16.0, selected: false },
            ],
        );
    }),
    element!("frame_guides", |ui, _| {
        let (viewport, _) = ui.allocate_exact_size(vec2(200.0, 150.0), egui::Sense::hover());
        frame_guides(ui, viewport, viewport.shrink(20.0), FrameGuides { thirds: true, safe_area: true, centre_mark: true });
    }),
    element!("comparison_divider", |ui, _| {
        let (rect, _) = ui.allocate_exact_size(vec2(220.0, 100.0), egui::Sense::hover());
        let mut position = 0.5;
        comparison_divider(ui, rect, &mut position, "Comparison divider");
    }),
    element!("comparison_badge", |ui, _| {
        let (rect, _) = ui.allocate_exact_size(vec2(220.0, 100.0), egui::Sense::hover());
        comparison_badge(ui, rect, "Live preview", false);
    }),
    element!("tree_row_rect", |ui, _| {
        for (index, row) in [
            Row { label: "Objects", ..Default::default() },
            Row { label: "Output", subtitle: "1920 × 1080 · PNG", ..Default::default() },
            Row { compact: true, label: "Environment", subtitle: "Studio Soft 2K", ..Default::default() },
        ]
        .iter()
        .enumerate()
        {
            let rect = tree_row_rect(ui, row);
            assert_eq!(tree_row(ui, ui.scope_id().with(("tree_geometry", index)), row).rect, Some(rect));
        }
    }),
    element!("editor_section", |ui, _| {
        editor_section(ui, "edit", "Edit", true, |ui| {
            text(ui, "Colour", Text::Inline);
        });
    }),
    element!("method_section", |ui, _| {
        method_section(ui, "gpu_section", "Glass and absorption", "GPU", Some("Needs GPU path tracing"), |ui| {
            checkbox(ui, "Use physical glass", &mut false);
        });
    }),
    element!("vector3_field", |ui, _| {
        row(ui, "Origin", |ui| vector3_field(ui, &mut [0.0; 3], -1e6..=1e6, " mm"));
    }),
    element!("pending_section", |ui, _| {
        pending_section(ui, "upcoming", "Textures", "Soon");
    }),
    element!("inspector_header", |ui, _| {
        inspector_header(ui, "body", "Boss", "Body", InspectorNames { close: "sample:close", remove: "sample.remove", remove_label: "Remove" });
    }),
    element!("workspace_chip", |ui, _| {
        workspace_chip(ui, "Body");
    }),
    element!("section_heading", |ui, _| {
        section_heading(ui, "Edit");
    }),
    element!("checkbox_at", |ui, sample| {
        let side = Tokens::get().space_16;
        let (rect, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
        checkbox_at(ui, rect, "Enabled", &mut sample.checked);
    }),
    element!("settings_plugin_card", |ui, sample| {
        settings_plugin_card(
            ui,
            "sample",
            "Gear generator",
            "1.2.0",
            "Adds a parametric gear feature.",
            &mut sample.checked,
            &PluginCardLabels { show_in_folder: "Show in folder", enabled: "Enabled" },
        );
    }),
    element!("text_button", |ui, _| {
        text_button(ui, "Reset", true);
    }),
    element!("mixed_slider", |ui, _| {
        mixed_slider(ui, &mut 0.5, 0.0..=1.0, None);
    }),
    element!("searchable_icon_dropdown", |ui, _| {
        searchable_icon_dropdown(ui, "spin", "Replace…", &mut String::new(), &[]);
    }),
    element!("number_pair_preserving", |ui, _| {
        let (mut w, mut h) = (1920, 1080);
        number_pair_preserving(ui, "Size", [&mut w, &mut h], 1..=16384, 1.0, "");
    }),
    element!("progress_action", |ui, _| {
        progress_action(ui, "gallery:progress", "Rendering", "· 62%", 0.62, "Cancel");
    }),
    element!("settings_choice_row", |ui, _| {
        settings_choice_row(
            ui,
            &SettingsChoice { icon: None, key: "gallery:radio_row", title: "Preview", summary: "1280 × 720", detail: "Preview size" },
            true,
        );
    }),
    element!("icon_label_action", |ui, _| {
        icon_label_action(ui, "Render Image", Some("play"), true, true);
    }),
    element!("completion_text", |ui, _| {
        completion_text(ui, "Render 1 · 1920 × 1080", true);
        completion_text(ui, "No completed image", false);
    }),
    element!("tree_row", |ui, _| {
        let actions = [TreeAction { icon: "plus", hover_only: false, label: "Add", reason: None, target: "gallery:tree:add" }];
        tree_row(ui, ui.scope_id().with("tree_plain"), &Row { label: "Objects", fold: Some(true), actions: &actions, ..Default::default() });
        tree_row(
            ui,
            ui.scope_id().with("tree_subtitle"),
            &Row { label: "Output", subtitle: "1920 × 1080 · PNG", selected: true, ..Default::default() },
        );
    }),
    element!("tree_header", |ui, state| {
        tree_header(ui, "Scene", &mut state.selected, &["By object", "By look"], "Search objects");
        tree_row(
            ui,
            ui.scope_id().with("compact_environment"),
            &Row { compact: true, label: "Environment", icon: "environment_map", subtitle: "Studio Soft 2K", ..Default::default() },
        );
        tree_row(
            ui,
            ui.scope_id().with("compact_object"),
            &Row {
                compact: true,
                depth: 1,
                label: "Boss",
                swatch: Some(Tokens::get().accent),
                eye: Some(true),
                secondary: "Anodized Blue",
                selected: true,
                ..Default::default()
            },
        );
    }),
    element!("browser", |ui, _| {
        let mut state = browser::BrowserState::default();
        let header = browser::Header { title: "Objects", count: "2 objects", collapsible: true, search: Some("Search objects") };
        let _ = browser::Browser::new(&mut state, browser::Style::default(), 0).header(header).show(ui, |rows| {
            rows.row(browser::Node {
                id: 1,
                key: "layer",
                row: Row { fold: Some(true), eye: Some(true), icon: "folder", label: "Default", radio: Some(true), count: "2", ..Default::default() },
                click_folds: true,
                ..Default::default()
            });
            for (id, label, selected) in [(2, "Body 1", true), (3, "Body 2", false)] {
                rows.row(browser::Node {
                    id,
                    key: label,
                    row: Row {
                        depth: 1,
                        eye: Some(true),
                        icon: "body",
                        label,
                        name: true,
                        selected,
                        mark: if selected { Mark::Here } else { Mark::None },
                        ..Default::default()
                    },
                    select: browser::Selectable::Yes(0),
                    rename: browser::Rename::Inline,
                    ..Default::default()
                });
            }
        });
    }),
    element!("tree_row_disabled_actions", |ui, _| {
        let actions =
            [TreeAction { icon: "lookat", hover_only: true, label: "Solo", reason: Some("Not available yet"), target: "gallery:tree:solo" }];
        tree_row(ui, ui.scope_id().with("tree_actions"), &Row { label: "Light", actions: &actions, ..Default::default() });
    }),
    element!("selection_actions", |ui, _| {
        let actions = [
            StageAction { icon: Some("chain"), label: "Appearance Chain", enabled: true, primary: false, reason: None },
            StageAction { icon: Some("reorder"), label: "Replace All", enabled: true, primary: false, reason: None },
            StageAction { icon: Some("star"), label: "Favorite Look", enabled: true, primary: true, reason: None },
            StageAction { icon: Some("suppress"), label: "Hide", enabled: true, primary: false, reason: None },
        ];
        selection_actions(ui, "Boss", &actions);
    }),
    element!("catalog_scope", |ui, state| {
        catalog_scope(ui, |ui| {
            search_field(ui, ui.scope_id().with("catalog_search"), &mut state.value, "Search appearances");
            segmented(
                ui,
                &mut state.selected,
                &["In Design", "Library", "Recent", "★"],
                Segments { compact: true, proportional: true, workspace_accent: true, ..Default::default() },
            );
            ui.horizontal(|ui| {
                choice_chip(ui, "catalog_all", "All", true);
                choice_chip(ui, "catalog_metal", "Metal", false);
            });
            catalog_footer(ui, "Click to apply · drag onto the model");
        });
    }),
    element!("catalog_footer", |ui, _| {
        catalog_footer(ui, "Click to apply · drag onto the model");
    }),
    element!("sphere_preview", |ui, _| {
        sphere_preview(
            ui,
            "gallery:preview",
            Sphere { color: Tokens::get().render_accent, kind: FinishKind::Satin, opacity: None },
            "Satin",
            ["Library", "Used by 2"],
            |ui| searchable_icon_dropdown(ui, "spin", "Replace…", &mut String::new(), &[]),
        );
    }),
    element!("sphere_preview_with_source", |ui, _| {
        sphere_preview_with_source(
            ui,
            "gallery:preview-source",
            Sphere { color: Tokens::get().render_accent, kind: FinishKind::Satin, opacity: None },
            "Satin",
            "Used by 2",
            |ui| {
                compact_dropdown_menu(ui, "gallery:source", "In Design", |ui| {
                    text(ui, "Body look", Text::Inline);
                })
            },
            |ui| text_button(ui, "Replace…", true),
        );
    }),
    element!("mixed_scalar_field", |ui, _| {
        let mut value = 0.5;
        mixed_scalar_field(ui, &mut value, 0.0..=1.0, Some("Mixed"));
    }),
    element!("check_line", |ui, _s| {
        check_line(ui, "gallery:check_ok", "Ready", false);
    }),
    element!("check_line_warning", |ui, _s| {
        check_line(ui, "gallery:check_warning", "Transparency approximation", true);
    }),
    element!("check_line_error", |ui, _s| {
        check_status_line(ui, "gallery:check_error", "Image size exceeds the renderer limit", CheckStatus::Error);
    }),
    element!("check_status_line", |ui, _| {
        check_status_line(ui, "gallery:status_ok", "Ready", CheckStatus::Ok);
        check_status_line(ui, "gallery:status_warning", "Transparency approximation", CheckStatus::Warning);
        check_status_line(ui, "gallery:status_error", "Image size exceeds the renderer limit", CheckStatus::Error);
    }),
    element!("segmented_unavailable", |ui, s| {
        segmented(
            ui,
            &mut s.selected,
            &["Draft", "Custom"],
            Segments { disabled_reasons: &[Some("waiting for the quality contract"), None], ..Default::default() },
        );
    }),
    element!("review_and_apply", |ui, state| {
        ReviewAndApply {
            title: "Review native studio",
            name: "Soft enveloping studio",
            subtitle: "Native studio · whole scene",
            replaces_label: "Replaces",
            replaces: "Lighting, environment, exposure and stage",
            kept_label: "Keeps",
            kept: "Camera, sampling, dimensions, optical bindings and geometry",
            acknowledgement: "Confirm replacing saved state",
            acknowledge_handle: "review:acknowledge",
            apply: "Apply studio",
            secondary: None,
            notice: None,
        }
        .content(ui, &mut state.checked, |_| {});
    }),
    element!("catalog_footer_height", |ui, _| {
        let expected = catalog_footer_height(ui);
        catalog_footer(ui, "Click to apply to the selection.");
        assert_eq!(solidcraft_ui_registry::rect_of("catalog:footer").unwrap().height(), expected);
    }),
    element!("catalog_footer_with_tip", |ui, _| {
        catalog_footer_with_tip(ui, "Click to apply to the selection.", "Paint and drag assignment are not supported yet.");
    }),
    element!("catalog_layout", |ui, _| {
        ui.allocate_ui(egui::vec2(ui.available_width(), 300.0), |ui| {
            let layout = CatalogLayout::new(ui, "gallery:catalog", 3);
            layout.body(ui, |ui| {
                for index in 0..12 {
                    MenuItem::new("catalog_item", &format!("Surface {index}"), "grid").search_row(ui, "", "Authored map", None);
                }
            });
            layout.footer(ui, |ui| {
                catalog_footer(ui, "Select a surface to review.");
            });
        });
    }),
    element!("sphere_grid", |ui, _| {
        let sphere = Sphere { color: Tokens::get().render_accent, kind: FinishKind::Satin, opacity: None };
        let tiles = ["gallery:grid:a", "gallery:grid:b", "gallery:grid:c"].map(|id| SphereTile {
            id,
            name: "Satin",
            sphere,
            selected: id.ends_with('b'),
            users: 1,
            tip: "Satin",
            badge: None,
        });
        sphere_grid(ui, &tiles);
    }),
    element!("sphere_grid_drag", |ui, _| {
        let tiles = [SphereTile {
            id: "gallery:drag-grid",
            name: "Copper",
            sphere: Sphere { color: Tokens::get().render_accent, kind: FinishKind::Metal, opacity: None },
            selected: true,
            users: 1,
            tip: "Drag to apply",
            badge: None,
        }];
        sphere_grid_drag(ui, &tiles);
    }),
    element!("sphere_cursor", |ui, _| {
        let sphere = Sphere { color: Tokens::get().render_accent, kind: FinishKind::Satin, opacity: None };
        sphere_thumbnail(ui, "gallery:cursor-sample", sphere, Tokens::get().control_height);
        sphere_cursor(ui, sphere);
    }),
    element!("note_height", |ui, _| {
        let label = "A wrapped note reserves its measured height in a bounded card.";
        let _height = note_height(ui, label);
        text(ui, label, Text::Note("gallery:measured-note"));
    }),
    element!("sphere_tile", "gallery:tile", Some("gallery:tile"), |ui, _| {
        SphereTile {
            id: "gallery:tile",
            name: "Satin metal",
            sphere: Sphere { color: Tokens::get().render_accent, kind: FinishKind::Metal, opacity: None },
            selected: true,
            users: 3,
            tip: "Satin metal",
            badge: Some("Library"),
        }
        .show(ui);
    }),
    element!("strip_toggle", |ui, _| {
        ui.horizontal(|ui| {
            strip_toggle(ui, true, "Hide Shots strip");
            strip_toggle(ui, false, "Shots");
        });
    }),
    element!("strip_heading", |ui, _| {
        StripHeading::new(ui.painter(), "History").show(ui);
    }),
    element!("choice_chip_selected", |ui, _| {
        choice_chip(ui, "saved", "Hero", true);
    }),
    element!("add_choice", |ui, _| {
        add_choice(ui, "Save Shot");
    }),
    element!("compact_dropdown_menu", |ui, _| {
        compact_dropdown_menu(ui, "gallery_strip_size", "1920 × 1080", |ui| {
            list_picker(ui, "full_hd", "Full HD", true);
        });
    }),
    element!("thumbnail_choice", |ui, _| {
        ui.horizontal(|ui| {
            thumbnail_choice(ui, "hero", "Hero", ThumbnailChoice { selected: true, modified: true, ..Default::default() });
            thumbnail_choice(ui, "front", "Front", ThumbnailChoice { progress: Some(0.6), ..Default::default() });
            add_choice(ui, "Save Shot");
        });
    }),
    element!("settings_form", |ui, state| {
        settings_form(ui, Tokens::get().form_label_width, |ui| {
            row(ui, "Sample", |ui| {
                field(ui, &mut state.value);
            });
        });
    }),
    element!("below_row_control", |ui, state| {
        settings_form(ui, Tokens::get().form_label_width, |ui| {
            row(ui, "Autosave every", |ui| {
                field(ui, &mut state.value);
            });
            below_row_control(ui, "Autosave every", 260.0, |ui| settings_inline_note(ui, "Every 5 minutes"));
        });
    }),
    element!("named_control", "dialog:widget:Profile name", Some("dialog:Profile name"), |ui, state| {
        named_control(ui, "Profile name", |ui| {
            text_field(ui, &mut state.value, "Name this setup");
        });
    }),
    element!("settings_row_card", |ui, _| {
        let t = Tokens::get();
        for selected in [true, false] {
            settings_row_card(ui, selected, t.control_height + t.space_14, |ui| {
                text(ui, "Default", if selected { Text::SelectedNameCell } else { Text::NameCell(false) });
                if selected {
                    settings_badge(ui, "Active", false);
                } else {
                    button(ui, "Apply", Button::Normal);
                }
            });
            ui.add_space(t.space_8);
        }
    }),
    element!("settings_composition", |ui, _| {
        settings_heading(ui, "Interface", "Appearance and workspace");
        settings_section(ui, "gallery_settings", "Layout", |ui| {
            settings_pair(
                ui,
                |ui| {
                    text(ui, "Controls", Text::Inline);
                },
                |ui, ()| layout_preview(ui, LayoutPreview { compact: true, labels: false, history_in_browser: false }, "Where the panels go"),
            )
        });
    }),
    element!("settings_picture_choices", |ui, state| {
        let choices = [
            PictureChoice { key: "bottom", label: "Bottom", figure: super::preferences_figure_data::paint_history_bottom },
            PictureChoice { key: "left", label: "Left", figure: super::preferences_figure_data::paint_history_left },
        ];
        if let Some(selected) = settings_picture_choices(ui, "gallery_history", "History", &choices, state.selected % choices.len(), true) {
            state.selected = selected;
        }
        settings_picture_choices(ui, "gallery_history_disabled", "Disabled history", &choices, 0, false);
    }),
    element!("settings_picture_choices_sized", |ui, state| {
        let choices = [
            PictureChoice { key: "origin", label: "Origin", figure: super::preferences_figure_data::paint_orbit_origin },
            PictureChoice { key: "centre", label: "View center", figure: super::preferences_figure_data::paint_orbit_centre },
        ];
        let t = Tokens::get();
        if let Some(selected) = settings_picture_choices_sized(
            ui,
            "gallery_pivots",
            "Orbit around",
            &choices,
            state.selected % choices.len(),
            true,
            PictureChoiceSize { width: t.space_24 * 8.0 + t.space_4, aspect: 184.0 / 99.5, caption: false },
        ) {
            state.selected = selected;
        }
    }),
    element!("settings_permission_choices", |ui, state| {
        settings_choices(
            ui,
            &[
                SettingsChoice {
                    icon: None,
                    key: "gallery:ask",
                    title: "Ask first",
                    summary: "Confirm changes before they run.",
                    detail: "Full permission details",
                },
                SettingsChoice {
                    icon: None,
                    key: "gallery:edits",
                    title: "Allow edits",
                    summary: "Ask before running other tools.",
                    detail: "Full permission details",
                },
                SettingsChoice {
                    icon: Some("warning"),
                    key: "gallery:all",
                    title: "Allow all",
                    summary: "Run supported tools without asking.",
                    detail: "Full permission details",
                },
            ],
            &mut state.selected,
        );
    }),
    element!("picture_card", |ui, _| {
        use super::preferences_figure_data as figures;
        let gap = ui.spacing().item_spacing.x;
        let width = card_width(ui.available_width(), 4, gap);
        ui.horizontal(|ui| {
            picture_card(
                ui,
                &Card {
                    label: "Coarse",
                    sub: Some("fastest"),
                    selected: false,
                    enabled: true,
                    options: None,
                    check: false,
                    aspect: 120.0 / 64.0,
                    figure: &figures::paint_tess_coarse,
                },
                width,
            );
            picture_card(
                ui,
                &Card {
                    label: "Normal",
                    sub: Some("default"),
                    selected: true,
                    enabled: true,
                    options: None,
                    check: true,
                    aspect: 120.0 / 64.0,
                    figure: &figures::paint_tess_normal,
                },
                width,
            );
            picture_card(
                ui,
                &Card {
                    label: "Fine",
                    sub: Some("smoother"),
                    selected: false,
                    enabled: true,
                    options: Some("Has options"),
                    check: false,
                    aspect: 120.0 / 64.0,
                    figure: &figures::paint_tess_fine,
                },
                width,
            );
            picture_card(
                ui,
                &Card {
                    label: "8×",
                    sub: Some("not on this GPU"),
                    selected: false,
                    enabled: false,
                    options: None,
                    check: false,
                    aspect: 120.0 / 64.0,
                    figure: &figures::paint_aa_8x,
                },
                width,
            );
        });
    }),
    element!("toggle_tile", |ui, _| {
        use super::preferences_figure_data as figures;
        let gap = ui.spacing().item_spacing.x;
        let width = card_width(ui.available_width(), 3, gap);
        ui.horizontal(|ui| {
            toggle_tile(
                ui,
                &Card {
                    label: "Grid",
                    sub: None,
                    selected: true,
                    enabled: true,
                    options: None,
                    check: false,
                    aspect: 120.0 / 64.0,
                    figure: &figures::paint_view_grid,
                },
                width,
            );
            toggle_tile(
                ui,
                &Card {
                    label: "Ground shadow",
                    sub: None,
                    selected: false,
                    enabled: true,
                    options: None,
                    check: false,
                    aspect: 120.0 / 64.0,
                    figure: &figures::paint_view_shadow,
                },
                width,
            );
            toggle_tile(
                ui,
                &Card {
                    label: "Perspective",
                    sub: None,
                    selected: false,
                    enabled: false,
                    options: None,
                    check: false,
                    aspect: 120.0 / 64.0,
                    figure: &figures::paint_view_perspective,
                },
                width,
            );
        });
    }),
    element!("preset_card", |ui, _| {
        let icon = |p: &egui::Painter, r: egui::Rect, ink: egui::Color32| super::icons::paint_ink(p, r, "settings", ink);
        let width = ui.available_width();
        for (name, word, selected, enabled) in
            [("Performance", "Fastest", false, true), ("Quality", "Sharpest", true, true), ("Custom", "Your own mix", false, false)]
        {
            preset_card(ui, name, word, selected, enabled, &icon, width);
        }
    }),
    element!("help_icon", |ui, _| {
        ui.horizontal(|ui| {
            text(ui, "Anti-aliasing", Text::Hint("gallery_help"));
            help_icon(ui, "Applies when SolidCraft restarts.");
        });
    }),
    element!("preference_theme_thumbnails", |ui, _| {
        let (light, dark) = (ThemeSample::from_tokens(&Tokens::light()), ThemeSample::from_tokens(&Tokens::dark()));
        ui.horizontal(|ui| {
            for kind in 0..2 {
                let (rect, _) = ui.allocate_exact_size(vec2(88.0, 88.0 * 76.0 / 124.0), egui::Sense::hover());
                match kind {
                    0 => paint_theme_thumbnail(ui.painter(), rect, &light),
                    1 => paint_theme_thumbnail(ui.painter(), rect, &dark),
                    _ => {}
                }
            }
        });
    }),
    element!("preference_figures", |ui, _| {
        ui.horizontal_wrapped(|ui| {
            for (name, size, paint) in super::preferences_figure_data::FIGURES {
                let width = 96.0;
                let (rect, response) = ui.allocate_exact_size(vec2(width, width * size[1] / size[0]), egui::Sense::hover());
                paint(ui.painter(), rect);
                response.backing_on_hover_text(*name);
            }
        });
    }),
    element!("preference_option_cards", |ui, _| {
        ui.horizontal_wrapped(|ui| {
            for figure in [PreferenceFigure::Pivot(2), PreferenceFigure::UpAxis(true)] {
                option_card(ui, "Choice", Some("Example"), true, true, figure);
            }
            let t = Tokens::get();
            let width = t.space_24 * 5.0 + t.space_10;
            for (label, figure) in [("Fine", paint_tess_fine as fn(&egui::Painter, egui::Rect)), ("8×", paint_aa_8x)] {
                picture_card(
                    ui,
                    &Card { label, sub: None, selected: true, enabled: true, options: None, check: true, aspect: 120.0 / 64.0, figure: &figure },
                    width,
                );
            }
            toggle_tile(
                ui,
                &Card {
                    label: "Ambient occlusion",
                    sub: None,
                    selected: true,
                    enabled: true,
                    options: None,
                    check: false,
                    aspect: 120.0 / 64.0,
                    figure: &paint_ao_on,
                },
                width,
            );
        });
    }),
    element!("settings_font_row", |ui, sample| {
        settings_page(ui, |ui| {
            settings_font_row(ui, "Font", |ui, part| {
                if part == 0 {
                    named_control(ui, "Interface font", |ui| {
                        dropdown(ui, "gallery_preferences_font", &mut sample.selected, &["IBM Plex Sans (built in)", "Custom font"]);
                    });
                } else {
                    button(ui, "Choose font file…", Button::Normal);
                    button(ui, "Reset", Button::Ghost);
                }
            });
            font_specimen(ui, "The quick brown fox · 0123456789");
        });
    }),
    element!("preference_specimens", |ui, _| {
        settings_axis_row(
            ui,
            &AxisColumns { axis: "Axis", live_input: "Live input", speed: "Speed", source: "Source" },
            "Push X",
            0.6,
            [0.1, 0.2],
            |_| {},
            |_| {},
            |_| {},
        );
        settings_mouse_mapping(ui, &[(vec!["Shift".into(), "Middle drag".into()], "Orbit".into())], |_| {});
    }),
    element!("assistant_entry", |ui, _| {
        let t = Tokens::get();
        crate::Container::show_backing(ui, egui::Frame::NONE.fill(t.app_bar), |ui| {
            ui.horizontal(|ui| {
                for state in [
                    AssistantEntryState::Quiet,
                    AssistantEntryState::Ready,
                    AssistantEntryState::Busy,
                    AssistantEntryState::NeedsAuth,
                    AssistantEntryState::Stopped,
                    AssistantEntryState::Waiting(3),
                ] {
                    let (_, response) = ui.allocate_exact_size(vec2(t.space_24 + t.space_8, t.space_24 + t.space_8), egui::Sense::click());
                    assistant_entry(ui, &response, state, false, false);
                }
            });
        });
    }),
    element!("assistant_header", |ui, _| {
        assistant_header(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Assistant");
                assistant_badge(ui, "Ready", AssistantTone::Success);
            });
        });
    }),
    element!("assistant_controls", |ui, _| {
        assistant_controls(ui, |ui| {
            button(ui, "Connect", Button::Normal);
        });
    }),
    element!("assistant_badge", |ui, _| {
        ui.horizontal(|ui| {
            for (label, tone) in [
                ("Ready", AssistantTone::Success),
                ("Working", AssistantTone::Working),
                ("Waiting", AssistantTone::Warning),
                ("Error", AssistantTone::Error),
                ("Offline", AssistantTone::Neutral),
            ] {
                assistant_badge(ui, label, tone);
            }
        });
    }),
    element!("assistant_bubble", |ui, _| {
        assistant_bubble(ui, |ui| {
            ui.label("Set the plate thickness to 8 mm.");
        });
    }),
    element!("assistant_prose", |ui, _| {
        assistant_prose(ui, |ui| {
            ui.label("The thickness is now 8 mm. The design rebuilt successfully.");
        });
    }),
    element!("assistant_tool_status", |ui, _| {
        ui.horizontal(|ui| {
            for state in [AssistantToolState::Pending, AssistantToolState::Running, AssistantToolState::Done, AssistantToolState::Failed] {
                assistant_tool_status(ui, state);
            }
        });
    }),
    element!("assistant_tool", |ui, _| {
        assistant_tool_frame(ui, false, |ui| {
            ui.monospace("sample.action_a");
            assistant_tool_detail(ui, |ui| {
                ui.monospace("thickness = 8 mm");
            });
        });
    }),
    element!("assistant_tool_failed", |ui, _| {
        assistant_tool_frame(ui, false, |ui| {
            assistant_tool_row(ui, true, |ui| {
                ui.monospace("sample.action_b");
            });
        });
    }),
    element!("assistant_approval", |ui, _| {
        assistant_approval(ui, "Allow the Assistant to run this?", |ui| {
            ui.label("Change the design parameter.");
            assistant_actions(ui, |ui| {
                primary_button(ui, "Allow once");
                button(ui, "Always allow", Button::Normal);
                button(ui, "Deny", Button::Normal);
            });
        });
    }),
    element!("assistant_composer", |ui, s| {
        assistant_composer_footer(ui, |ui| {
            assistant_composer(ui, false, |ui| {
                assistant_input(ui, ui.scope_id().with("assistant_gallery"), &mut s.value, "Ask for a change…");
            });
        });
    }),
    element!("assistant_composer_focused", |ui, _| {
        assistant_composer(ui, true, |ui| {
            ui.label("Ask for a change…");
        });
    }),
    element!("assistant_send", |ui, _| {
        ui.horizontal(|ui| {
            assistant_send(ui, true, false, "Send");
            assistant_send(ui, true, true, "Stop");
            assistant_send(ui, false, false, "Send");
        });
    }),
    element!("assistant_empty", |ui, _| {
        assistant_empty(ui, "Set up an assistant", "Choose an installed agent in Preferences.", |ui| {
            button(ui, "Open Preferences", Button::Normal);
        });
    }),
    element!("assistant_banner", |ui, _| {
        assistant_banner(ui, AssistantTone::Warning, |ui| {
            ui.label("Paused after the configured step limit.");
        });
    }),
    element!("settings_tab_chip", |ui, _| {
        ui.horizontal_wrapped(|ui| {
            settings_tab_chip(ui, "Tab A", "Tab A", "placeholder", true);
            settings_tab_chip(ui, "Tab B", "Tab B", "placeholder", false);
        });
    }),
    element!("history_transport", |ui, _| {
        ui.horizontal(|ui| {
            let side = Tokens::get().space_24;
            for control in HistoryTransport::ALL {
                let (rect, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
                control.show(ui, rect, false, PlayLabels { play: "Play", stop: "Stop" });
            }
        });
    }),
    element!("history_marker", |ui, _| {
        let t = Tokens::get();
        let accent = solidcraft_theme::TabAccent::for_ramp(&t, solidcraft_theme::AccentRamp::Surface);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(t.space_24 * 7.0, t.space_12), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, t.panel);
            history_marker(ui.painter(), rect, true, accent);
            let (rect, _) = ui.allocate_exact_size(vec2(t.space_12, t.control_height), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, t.timeline);
            history_marker(ui.painter(), rect, false, accent);
        });
    }),
    element!("file_navigation", |ui, _| {
        FileView::navigation(ui, |ui| {
            FileView::back_to(ui, "gallery:file:back", "Back to {name}", "Sample Plate", "Esc");
            FileView::nav_row(ui, "gallery:file:start", "Start", "", true, true);
            FileView::nav_row(ui, "gallery:file:open", "Open", "Ctrl+O", false, true);
            FileView::separator(ui);
            FileView::nav_row(ui, "gallery:file:save", "Save", "Ctrl+S", false, false);
        });
    }),
    element!("file_action", |ui, _| {
        FileView::action_tile(ui, "gallery:file:new", ui.available_width(), "new", "New design", "Millimeters · opens in Design", true);
    }),
    element!("file_recovery_banner", |ui, _| {
        FileView::recovery_banner(
            ui,
            "gallery:recovery",
            super::file_view::RecoveryMessage {
                title: "1 design was recovered",
                before_filename: " after SolidCraft closed unexpectedly: ",
                filename: "enclosure-lid.sc",
                after_filename: ", 2 minutes ago.",
            },
            "Recover",
            Some("Discard"),
        );
    }),
    element!("file_recovery_review", |ui, _| {
        FileView::recovery_review(ui, "gallery:recovery-review", "Sample Plate", " · 14 features · 2 minutes ago", "Recover", "Discard");
    }),
    element!("file_recent_list", |ui, _| {
        FileView::recent_list(ui, |ui| {
            for id in ["first", "second"] {
                FileView::recent_row(
                    ui,
                    id,
                    RecentRow {
                        title: "Sample Plate",
                        path: "Designs/sample-plate.sc",
                        when: "Open now",
                        workspace: "Design",
                        thumbnail: None,
                        thumbnail_failed: false,
                        security: None,
                    },
                );
            }
        });
    }),
    element!("file_recent", |ui, _| {
        FileView::recent_row(
            ui,
            "gallery:file:recent",
            RecentRow {
                title: "Sample Plate",
                path: "Designs/sample-plate.sc",
                when: "Open now",
                workspace: "Design",
                thumbnail: None,
                thumbnail_failed: false,
                security: None,
            },
        );
    }),
    element!("file_sample", |ui, _| {
        FileView::sample_grid(
            ui,
            "gallery:file:sample",
            &[SampleCard { title: "Sample Plate", description: "Parametric plate with holes and a bored boss", icon: "new".into(), thumbnail: None }],
        );
    }),
    element!("file_recovery", |ui, _| {
        FileView::recovery_banner(
            ui,
            "gallery:recovery-summary",
            super::file_view::RecoveryMessage { title: "2 designs have unsaved changes", before_filename: "", filename: "", after_filename: "" },
            "Review",
            None,
        );
    }),
    element!("command_search", |ui, _| {
        let mut query = "fillet".to_owned();
        CommandSearch::field(
            ui,
            &mut query,
            false,
            true,
            true,
            SearchFieldGeometry {
                height: Tokens::get().control_height + Tokens::get().space_12,
                radius: CommandSearch::field_radius(1.0, true),
                progress: 1.0,
            },
            Some("S"),
            4096,
        );
        ui.add_space(Tokens::get().space_6);
        CommandSearch::sheet(ui, ui.available_width(), 1.0, false, |ui| {
            CommandSearch::heading(ui, "search.commands", 1);
            let mut row = MenuItem::new("sample.action_a", "Fillet", "placeholder");
            row.highlighted = true;
            row.shortcut = "F";
            row.search_row(ui, &query, "Tab A › Group", None);
            CommandSearch::detail(ui, "radius  6 mm");
        });
    }),
    element!("menu", |ui, _| {
        Menu::show(ui, 268.0, |ui| {
            Menu::heading(ui, "Body");
            MenuItem::new("gallery:normal", "Properties", "measure").show(ui);
            let mut item = MenuItem::new("gallery:hover", "Hide", "eye");
            item.highlighted = true;
            item.shortcut = "H";
            item.show(ui);
            item = MenuItem::new("gallery:open", "Appearance", "appearance");
            item.submenu = true;
            item.highlighted = true;
            item.show(ui);
            item = MenuItem::new("gallery:disabled", "Paste", "paste");
            item.enabled = false;
            item.shortcut = "Ctrl+V";
            item.show(ui);
            item = MenuItem::new("gallery:checked", "Show Origin", "origin");
            item.checked = Some(true);
            item.show(ui);
            item.checked = Some(false);
            item.id = "gallery:unchecked";
            item.show(ui);
            item = MenuItem::new("gallery:swatch", "Aluminum - Satin", "appearance");
            item.swatch = Some(Tokens::get().control_border);
            item.show(ui);
            Menu::separator(ui);
            item = MenuItem::new("gallery:danger", "Delete", "delete");
            item.danger = true;
            item.shortcut = "Del";
            item.show(ui);
        });
    }),
    element!("menu_submenu", |ui, _| {
        ui.horizontal_top(|ui| {
            Menu::show(ui, 244.0, |ui| {
                let mut item = MenuItem::new("parent", "Appearance", "appearance");
                item.submenu = true;
                item.highlighted = true;
                item.show(ui);
            });
            Menu::show(ui, 236.0, |ui| {
                Menu::heading(ui, "Appearance");
                let mut item = MenuItem::new("look", "Aluminum - Satin", "appearance");
                item.swatch = Some(Tokens::get().control_border);
                item.show(ui);
            });
        });
    }),
    element!("rename_menu", |ui, s| {
        rename_menu(ui, &mut s.value, &mut false);
    }),
    element!("radial_menu", |ui, _| {
        // The marking menu's eight slots, as a host fills them outside a sketch.
        let (r, _) = ui.allocate_exact_size(vec2(360.0, 360.0), egui::Sense::hover());
        let items = [
            Item::action("sample.action_a", "Repeat", "placeholder").key("R").on_if(false),
            Item::action("sample.action_b", "Delete", "placeholder").key("Del"),
            Item::action("sample.action_c", "Action C", "placeholder"),
            Item::action("sample.action_d", "Action D", "placeholder"),
            Item::action("sample.action_e", "Action E", "placeholder"),
            Item::action("sample.action_f", "Action F", "placeholder"),
            Item::action("sample.action_g", "Undo", "placeholder"),
            Item::action("sample.action_h", "Move/Copy", "placeholder"),
        ];
        radial_menu(ui, r.center(), &items, Some(3));
    }),
    element!("workspace_menu", |ui, _| {
        // A current, an available and a planned workspace, as a host lists them.
        let items = [
            Item::action("sample.space_a", "Space A", "placeholder").key("current"),
            Item::action("sample.space_b", "Space B", "placeholder"),
            Item::action("sample.space_c", "Space C", "placeholder").on(false),
        ];
        let style = |item: &Item| {
            let (slug, tabs, ramp) = match item.label.as_str() {
                "Space A" => ("a", Some("Tab A · Tab B"), None),
                "Space B" => ("b", Some("Tab C"), Some(solidcraft_theme::AccentRamp::Surface)),
                _ => ("c", None, None),
            };
            Some(WorkspaceStyle { slug, description: "A sample workspace", tabs, ramp })
        };
        workspace_menu(ui, &WorkspaceMenu { items: &items, style: &style, footer: "Every workspace shows the same sample." }, None, 550.0);
    }),
    element!("window", "dialog:footer", Some("dialog:ok"), |ui, _| {
        Window::new(ui.scope_id().with("window"), "settings", "Preferences").content(
            ui,
            true,
            |ui| {
                text(ui, "General", Text::Report("window", false));
            },
            |ui| {
                button(ui, "Restore defaults", Button::Normal);
            },
        );
    }),
    element!("window_pages", "dialog:pages", Some("gallery:page"), |ui, s| {
        panel::page_list(
            ui,
            &mut s.selected,
            &[WindowPage {
                group: Some("prefs.input"),
                icon: Icon::Kit("settings"),
                label: "ui.general",
                handle: "gallery:page",
                scroll: true,
                draw: &|ui, _: &mut ()| {
                    text(ui, "General", Text::Inline);
                },
            }],
            &mut (),
        );
    }),
    element!("confirm_warning", "dialog:footer", Some("dialog:apply"), |ui, _| {
        Confirm::new(ui.scope_id().with("confirm"), "status_warning", "Save changes?")
            .tone(Tone::Warning)
            .actions("Save", "Cancel", Some("Don't Save"))
            .content(ui, true, |ui| {
                text(ui, "Your changes will be lost.", Text::Report("message", false));
            });
    }),
    element!("confirm_danger", "dialog:footer", Some("dialog:ok"), |ui, _| {
        Confirm::new(ui.scope_id().with("confirm"), "delete", "Delete")
            .tone(Tone::Danger)
            .detail("Fillet2 · Chamfer1")
            .actions("Delete", "Cancel", None)
            .content(ui, true, |ui| {
                text(ui, "These features will be deleted:", Text::Report("message", false));
            });
    }),
    element!("confirm_info", "dialog:footer", Some("dialog:ok"), |ui, _| {
        Confirm::new(ui.scope_id().with("confirm"), "help", "Replace file?").actions("Replace", "Cancel", None).content(ui, true, |ui| {
            text(ui, "A file with this name already exists.", Text::Report("message", false));
        });
    }),
    element!("mono_note", |ui, _| {
        text(ui, "solid.fillet · replayable", Text::MonoNote("sample"));
    }),
    element!("color_thumbnail", |ui, _| {
        color_thumbnail(ui, ui.scope_id().with("thumb"), Tokens::get().accent, "Sample", ());
    }),
    element!("list_frame", |ui, _| {
        list_frame(ui, "sample", |ui| {
            text(ui, "Sample", Text::Cell);
        });
    }),
    element!("segmented_primary", |ui, s| {
        segmented(ui, &mut s.selected, &["U8", "F"], Segments { primary: true, ..Default::default() });
    }),
    element!("color_popover", |ui, _| {
        color_popover(ui, &mut [204, 208, 214]);
    }),
    element!("report_list", |ui, _| {
        report_list(ui, "sample", 120.0, |ui| {
            report_header(ui, "Internal error", 3, "2026-10-18 21:04:12", "Report", |ui| {
                bin_button(ui, "Delete");
            });
            text(ui, "solid.fillet · replayable", Text::MonoNote("sample"));
        });
    }),
    element!("path_chip", |ui, _| {
        path_chip(ui, "/reports", |ui| {
            button(ui, "Copy path", Button::Normal);
        });
    }),
    element!("empty_box", |ui, _| {
        empty_box(ui, "empty", "No crash reports.");
    }),
    element!("report_header", |ui, _| {
        report_header(ui, "Internal error", 3, "2026-10-18 21:04:12", "Report", |ui| {
            bin_button(ui, "Delete");
        });
    }),
    element!("parameter_import", |ui, _| {
        icon_label_button(ui, "Import…", "import_tray", true, false);
    }),
    element!("parameter_export_csv", |ui, _| {
        icon_label_button(ui, "Export CSV…", "export_tray", true, false);
    }),
    element!("parameter_export_json", |ui, _| {
        icon_label_button(ui, "Export JSON…", "export_tray", true, false);
    }),
    element!("icon_label_button", |ui, _| {
        icon_label_button(ui, "Copy path", "copy", true, false);
    }),
    element!("compact_icon_label_button", |ui, _| {
        ui.horizontal(|ui| {
            compact_icon_label_button(ui, "Paint", "appearance", true);
            compact_icon_label_button(ui, "Pick Up", "select", true);
        });
    }),
    element!("section_count", |ui, _| {
        section_count(ui, "count", "Parameters", false, Some(5), |_| {});
    }),
    element!("bin_button", |ui, _| {
        bin_button(ui, "Delete");
    }),
    element!("active_pill", |ui, _| {
        active_pill(ui);
    }),
    element!("library_cell", |ui, s| {
        library_cell(ui, ui.scope_id().with("library"), &mut s.value, "Default material", |ui| {
            button(ui, "Steel", Button::Normal);
        });
    }),
    element!("add_prompt", "dialog:table:add_prompt:add", Some("dialog:table:add_prompt:add"), |ui, _| {
        table_custom(ui, "add_prompt", &[Column::new("Name", 1.0).flex()], "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            table.add_prompt(ui, "Add configuration", "Add configuration");
        });
    }),
    element!("removable_header", |ui, _| {
        ui.allocate_ui(vec2(140.0, row_height()), |ui| {
            removable_header(ui, "Width");
        });
    }),
    element!("sort_header", |ui, _| {
        ui.allocate_ui(vec2(140.0, row_height()), |ui| {
            ui.set_min_size(vec2(140.0, row_height()));
            sort_header(ui, "Name", &mut Sort::Ascending);
        });
    }),
    element!("plus_header", |ui, _| {
        ui.allocate_ui(vec2(140.0, row_height()), |ui| {
            ui.set_min_size(vec2(140.0, row_height()));
            plus_header(ui, |_| {});
        });
    }),
    element!("table_custom", |ui, _| {
        table_custom(ui, "custom", &[Column::new("Name", 1.0)], "Empty", |ui, table| {
            table.headers(ui, |_, _| false);
            table.add_prompt(ui, "Add parameter", "Add parameter");
        });
    }),
    element!("table_row_height", |ui, _| {
        ui.allocate_space(vec2(table_row_height(), table_row_height()));
    }),
    element!("summary_row", |ui, s| {
        summary_row(ui, "Rule", "Drives width = 120 mm", &mut s.checked, ContentWidth::button(ui, "Edit"), |ui| {
            button(ui, "Edit", Button::Normal);
        });
    }),
    element!("compact_button", |ui, _| {
        compact_button(ui, "▲", "Move up", true, false);
    }),
    element!("error", |ui, _| {
        text(ui, "Invalid expression", Text::Error("gallery"));
    }),
    element!("table_flex", |ui, s| {
        table_custom(
            ui,
            "flex",
            &[Column::new("Name", 1.0).flex(), Column::new("Expression", 1.5).flex(), Column::new("Actions", 0.0).icon_only()],
            "Empty",
            |ui, table| {
                table.headers(ui, |_, _| false);
                table.row(ui, "row", false, |row| {
                    row.cell(|ui| {
                        text(ui, "Width", Text::Cell);
                    });
                    row.cell(|ui| {
                        cell_editor(ui, ui.scope_id().with("expression"), &mut s.value, "Expression", false);
                    });
                    row.cell(|ui| {
                        bin_button(ui, "Delete");
                    });
                });
                table.add_prompt(ui, "Add parameter", "Add parameter");
            },
        );
    }),
    element!("cell_name", |ui, _| {
        text(ui, "Width", Text::NameCell(false));
    }),
    element!("cell_name_strong", |ui, _| {
        text(ui, "Default", Text::NameCell(true));
    }),
    element!("text_heading", |ui, _| {
        text(ui, "Configuration rules", Text::Heading("gallery"));
    }),
    element!("cell_mono", |ui, _| {
        text(ui, "mm", Text::MonoCell);
    }),
    element!("cell_value", |ui, _| {
        text(ui, "80.0000 mm", Text::Value("gallery_value", false));
    }),
    element!("cell_value_error", |ui, _| {
        text(ui, "Unknown parameter", Text::Value("gallery_value_error", true));
    }),
    element!("scope", |ui, _| {
        scope(ui, |ui| {
            ui.allocate_space(vec2(row_height(), row_height()));
        });
    }),
    element!("row_height", |ui, _| {
        ui.allocate_space(vec2(row_height(), row_height()));
    }),
    element!("header_height", |ui, _| {
        ui.allocate_space(vec2(header_height(), header_height()));
    }),
    element!("slider_integer_without_editor", |ui, _| {
        slider(ui, &mut 12.0, 0.0..=100.0, Slider { show_value: false, integer: true, ..Default::default() });
    }),
    element!("canvas_value_field", |ui, s| {
        canvas_value_field(ui, ui.scope_id().with("canvas_value"), &mut s.value, false);
    }),
    element!("placement_field", |ui, s| {
        placement_field(ui, ui.scope_id().with("placement"), &mut s.value, "Distance");
    }),
    element!("bounded_cell_editor", |ui, s| {
        bounded_cell_editor(ui, ui.scope_id().with("name"), &mut s.value, "Name", false, 128);
    }),
    element!("password_field", |ui, s| {
        password_field(ui, ui.scope_id().with("secret"), &mut s.value, "API key");
    }),
    element!("number_field", |ui, _| {
        number_field(ui, &mut 3_u32, 1..=200, 1.0, "");
    }),
    element!("number_field_preserving", |ui, _| {
        number_field_preserving(ui, &mut 256_u32, 1..=64, 1.0, "");
    }),
    element!("number_fields", |ui, _| {
        row(ui, "Target", |ui| {
            number_fields(ui, &mut [-18.0, 125.5, -18.0], -1e9..=1e9, 0.1, "");
        });
    }),
    element!("slider_percent", |ui, _| {
        slider(ui, &mut 1.0, 0.5..=2.5, Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() });
    }),
    element!("dropdown_menu", |ui, _| {
        dropdown_menu(ui, "gallery_menu", "Preset", |ui| {
            list_picker(ui, "preset", "Preset", true);
        });
    }),
    element!("bounded_dropdown_menu", |ui, state| {
        let choices = [
            "CPU lens acceptance with a long captured shot name for the retained image",
            "GPU lens acceptance with a second long captured shot name for the result",
        ];
        let selected = state.selected.min(choices.len() - 1);
        let width = ui.available_width().min(Tokens::get().form_card_width * 0.75);
        let response = bounded_dropdown_menu(ui, "gallery_bounded_menu", choices[selected], width, |ui| {
            for (index, label) in choices.iter().enumerate() {
                let response = list_picker(ui, &format!("bounded:{index}"), label, selected == index);
                if response.clicked() {
                    state.selected = index;
                    ui.close();
                }
                response.backing_on_hover_text(*label);
            }
        });
        response.backing_on_hover_text(choices[selected]);
    }),
    element!("dropdown_menu_with_close_behavior", |ui, s| {
        dropdown_menu_with_close_behavior(ui, "gallery_editable_menu", "Preset", egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            text_field(ui, &mut s.value, "Filter…");
            if list_picker(ui, "preset", "Preset", true).clicked() {
                ui.close();
            }
        });
    }),
    element!("message_field", |ui, s| {
        message_field(ui, ui.scope_id().with("message"), &mut s.value, "Message");
    }),
    element!("text_lines", |ui, _| {
        text_lines(ui, "gallery", "Licence\nSecond line");
    }),
    element!("report_divider", "dialog:divider:gallery_report", None, |ui, _| {
        report_list(ui, "gallery_report", row_height() * 3.0, |ui| {
            text(ui, "Report message", Text::Report("gallery_report", false));
            report_divider(ui, "gallery_report");
        });
    }),
    element!("table", "dialog:table:gallery:row", None, |ui, s| {
        table_custom(ui, "gallery", &[Column::new("Name", 1.0), Column::new("Expression", 1.5)], "No rows", |ui, table| {
            table.headers(ui, |_, _| false);
            table.row(ui, "row", true, |row| {
                row.cell(|ui| {
                    text(ui, "Width", Text::Cell);
                });
                row.cell(|ui| {
                    cell_editor(ui, ui.scope_id().with("value"), &mut s.value, "", false);
                });
            });
        });
    }),
    element!("name_list", "dialog:table:gallery_names:Bracket.solidcraft", None, |ui, _| {
        name_list(ui, "gallery_names", &["Name", "Size", "Actions"], "No files", |list| {
            list.row("Bracket.solidcraft", |row| {
                row.name("Bracket.solidcraft");
                row.cell(|ui| text(ui, "12.3 KB", Text::Inline));
                row.cell(|ui| button(ui, "Open", Button::Small));
            });
        });
    }),
    element!("list_picker", "dialog:list:gallery", Some("dialog:list:gallery"), |ui, _| {
        list_picker(ui, "gallery", "Preset", true);
    }),
    element!("choice_chip", "dialog:list:gallery", Some("dialog:list:gallery"), |ui, _| {
        choice_chip(ui, "gallery", "Shot 1", true);
    }),
    element!("inline_text", "dialog:inline:Active", Some("dialog:inline:Active"), |ui, _| {
        text(ui, "Active", Text::Inline);
    }),
    element!("cell_text", "dialog:cell:Width", Some("dialog:cell:Width"), |ui, _| {
        text(ui, "Width", Text::Cell);
    }),
    element!("label_actions", |ui, _| {
        label_actions(ui, "Configuration", |ui| {
            button(ui, "Apply", Button::Normal);
        });
    }),
    element!("text", "dialog:report:gallery", Some("dialog:report:gallery"), |ui, _| {
        text(ui, "Validation report with retained units: 10 mm", Text::Report("gallery", false));
    }),
    element!("name_cell_with_hint", "dialog:cell:Width", Some("dialog:cell:Width"), |ui, _| {
        name_cell_with_hint(ui, "Width", false, "Double-click to rename");
    }),
    element!("copy_cell", |ui, _| {
        copy_cell(ui, "12.500 g", "Mass: 12.500 g", false, "Click to copy");
    }),
    element!("search_field", |ui, s| {
        search_field(ui, ui.scope_id().with("search"), &mut s.value, "Search");
    }),
    element!("menu_filter", |ui, s| {
        menu_filter(ui, ui.scope_id().with("menu_filter"), &mut s.value, "Filter");
    }),
    element!("cell_editor", |ui, s| {
        cell_editor(ui, ui.scope_id().with("cell"), &mut s.value, "Expression", true);
    }),
    element!("color_field", |ui, _| {
        color_field(ui, &mut [120, 150, 180]);
    }),
    element!("color_field_with_opacity", |ui, _| {
        color_field_with_opacity(ui, &mut [120, 150, 180], &mut 0.5);
    }),
    element!("mixed_color_field", |ui, _| {
        mixed_color_field(ui, &mut [120, 150, 180], Some(&mut 0.5), Some("Mixed"));
    }),
    element!("slider", |ui, _| {
        slider(ui, &mut 0.5, 0.0..=1.0, Default::default());
    }),
    element!("slider_disabled", |ui, _| {
        ui.add_enabled_ui(false, |ui| slider(ui, &mut 0.5, 0.0..=1.0, Default::default()));
    }),
    element!("scalar_field", |ui, _| {
        scalar_field(ui, &mut 0.5, 0.0..=1.0);
    }),
    element!("track_number", |ui, _| {
        track_number(ui, &mut 1350.0, 0.5, 1);
    }),
    element!("length_field", |ui, _| {
        length_field(ui, &mut 12.7, "in", 2);
    }),
    element!("bounded_length_field", |ui, _| {
        bounded_length_field(ui, &mut 12.7, "in", 2, 0.0..=25.4, 0.254);
    }),
    element!("angle_field", |ui, _| {
        angle_field(ui, &mut 45.0);
    }),
    element!("text_area", |ui, s| {
        text_area(ui, "Gallery", ui.scope_id().with("area"), &mut s.value, 5, 1024);
    }),
    element!("color_swatch", |ui, _| {
        color_swatch(ui, ui.scope_id().with("swatch"), Tokens::get().accent, true, "Sample", 1);
    }),
    element!("swatch_grid", |ui, _| {
        swatch_grid(ui, &[1, 2, 3], |ui, n| {
            color_swatch(ui, ui.scope_id().with(n), Tokens::get().accent, true, "Sample", *n);
        });
    }),
    element!("action_menu", |ui, _| {
        action_menu(ui, "Actions", |ui| {
            button(ui, "Add", Button::Normal);
        });
    }),
    element!("enabled_button", |ui, _| {
        button(ui, "Add", Button::Enabled(true));
    }),
    element!("header", "dialog:header", Some("dialog:close"), |ui, _| {
        header(ui, "fillet", "Fillet", || {});
    }),
    element!("disclosure", |ui, _| {
        let t = Tokens::get();
        for open in [true, false] {
            let (rect, _) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), egui::Sense::hover());
            disclosure(ui.painter(), rect.center(), open, t.form_dim);
        }
    }),
    element!("disclosure_button_at", |ui, _| {
        let t = Tokens::get();
        for open in [true, false] {
            let (rect, _) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), egui::Sense::hover());
            ui.push_id(open, |ui| disclosure_button_at(ui, rect, open, solidcraft_ui_text::tr_label(if open { "Collapse" } else { "Expand" })));
        }
    }),
    element!("panel_disclosure_button_at", |ui, _| {
        let t = Tokens::get();
        for open in [true, false] {
            let (rect, _) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), egui::Sense::hover());
            ui.push_id(open, |ui| panel_disclosure_button_at(ui, rect, open, solidcraft_ui_text::tr_label(if open { "Collapse" } else { "Expand" })));
        }
    }),
    element!("dropdown_indicator", |ui, _| {
        let t = Tokens::get();
        let (rect, _) = ui.allocate_exact_size(vec2(t.form_section_icon_size, t.form_section_icon_size), egui::Sense::hover());
        dropdown_indicator(ui.painter(), rect, t.form_dim);
    }),
    element!("collapsible_header_closed", "dialog:header", Some("dialog:collapse"), |ui, _| {
        collapsible_header(ui, "fillet", "Fillet", &mut true, || {});
    }),
    element!("section_open", "dialog:section:Advanced", Some("dialog:section:Advanced"), |ui, _| {
        section(ui, "gallery_open", "Advanced", true, |_| {});
    }),
    element!("collapsible_header", "dialog:header", Some("dialog:collapse"), |ui, _| {
        collapsible_header(ui, "fillet", "Fillet", &mut false, || {});
    }),
    element!("row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            field(ui, &mut s.value);
        });
    }),
    element!("sized_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        sized_row(ui, "Gallery", row_height() * 2.0, |ui| {
            text_field(ui, &mut s.value, "Name");
        });
    }),
    element!("expr_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        expr_row(ui, "Gallery", &mut s.value);
    }),
    element!("value_row", "dialog:row:Gallery", None, |ui, _| {
        value_row(ui, "Gallery", "XY plane");
    }),
    element!("count_row", "dialog:row:Gallery", Some("dialog:Gallery up"), |ui, _| {
        let mut count = "6".to_owned();
        count_row(ui, "Gallery", &mut count, &Count { min: 1, max: 10, eval: &|text: &str| text.trim().parse().ok() });
    }),
    element!("count_field", "dialog:row:Gallery", Some("dialog:Gallery down"), |ui, _| {
        row(ui, "Gallery", |ui| {
            let mut count = "1".to_owned();
            count_field(ui, &mut count, &Count { min: 1, max: 10, eval: &|text: &str| text.trim().parse().ok() });
        });
    }),
    element!("whole_number_field", "dialog:row:Gallery", Some("dialog:Gallery up"), |ui, _| {
        row(ui, "Gallery", |ui| {
            whole_number_field(ui, &mut 2_u32, 0..=6, "");
        });
    }),
    element!("hint_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        hint_row(ui, "Gallery", &mut s.value, "Value");
    }),
    element!("text_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        text_row(ui, "Gallery", &mut s.value, "Name");
    }),
    element!("choice_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        choice_row(ui, "Gallery", "gallery", &mut s.selected, &["One", "Two"]);
    }),
    element!("multiline_text_field", "dialog:widget:Gallery", Some("dialog:Gallery"), |ui, s| {
        multiline_text_field(ui, "Gallery", ui.scope_id().with("text"), &mut s.value);
    }),
    element!("searchable_dropdown", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            searchable_dropdown(ui, "gallery", "One", &mut String::new(), &[SearchOption { value: "one", label: "One", handle: None }], None, None);
        });
    }),
    element!("search_picker", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            search_picker(
                ui,
                SearchPicker {
                    id: "gallery_search_picker",
                    handles: "gallery:search_picker",
                    current: "One",
                    current_value: "one",
                    hint: "Search",
                    choices: &[SearchChoice { value: "one", label: "One", tip: "", group: None }],
                    custom: true,
                    note: None,
                    open_group: None,
                    enabled: true,
                },
            );
        });
    }),
    element!("text_emphasis", "dialog:row:Gallery", Some("dialog:Bold"), |ui, s| {
        row(ui, "Gallery", |ui| {
            text_emphasis(ui, &mut s.checked, &mut false, true, &SAMPLE_EMPHASIS);
        });
    }),
    element!("value_pair_named", "dialog:row:Position", Some("dialog:Distance X"), |ui, s| {
        value_pair(ui, "Position", [&mut s.value, &mut s.second], ["X", "Y"], Some(["Distance X", "Distance Y"]));
    }),
    element!("inline_name", |ui, s| {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_height()), egui::Sense::hover());
        inline_name(ui, rect, ui.scope_id().with("inline_name"), &mut s.value);
    }),
    element!("field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            field(ui, &mut s.value);
        });
    }),
    element!("focused_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            focused_field(ui, &mut s.value);
        });
    }),
    element!("hint_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            hint_field(ui, &mut s.value, "Length");
        });
    }),
    element!("checked_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            checked_field(ui, &mut s.value, "Length", s.checked);
        });
    }),
    element!("checked_row", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        checked_row(ui, "Gallery", &mut s.value, "Length", Some("Too small"));
    }),
    element!("warning_caption", "dialog:caption:Gallery", None, |ui, _| {
        warning_caption(ui, "Gallery", "Too small");
    }),
    element!("text_field_tip", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            text_field_tip(ui, &mut s.value, "Name", "What it is called");
        });
    }),
    element!("text_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            text_field(ui, &mut s.value, "Name");
        });
    }),
    element!("text_field_reserving", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            let t = Tokens::get();
            let reserve = t.space_24 * 2.0;
            let (_, response) = text_field_reserving(ui, &mut s.value, "Name", reserve);
            let counter = egui::Rect::from_min_max(egui::pos2(response.rect.right() - reserve, response.rect.top()), response.rect.max);
            ui.painter().text(
                counter.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}/64", s.value.chars().count()),
                egui::FontId::proportional(t.font_small),
                t.form_dim,
            );
        });
    }),
    element!("focused_text_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            focused_text_field(ui, &mut s.value, "Name");
        });
    }),
    element!("fraction_field", "dialog:row:Gallery", Some("dialog:fraction:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            fraction_field(ui, &mut 0.5);
        });
    }),
    element!("dropdown", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            dropdown(ui, "gallery", &mut s.selected, &["One", "Two"]);
        });
    }),
    element!("dropdown_icons", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            dropdown_icons(ui, "gallery_icons", &mut s.selected, &[("One", "one"), ("Two", "two")]);
        });
    }),
    element!("dropdown_previews", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            dropdown_previews(ui, "gallery_previews", &mut s.selected, &["One", "Two"], |ui, index| {
                ui.label(if index == 0 { "One" } else { "Two" });
            });
        });
    }),
    element!("dropdown_picked", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            dropdown_picked(ui, "gallery_picked", &mut s.selected, &["One", "Two"]);
        });
    }),
    element!("selection_field", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            selection_field(ui, "Point 1", Selection { active: true, filled: true });
        });
    }),
    element!("selection_field_with_sense", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            selection_field_with_sense(ui, "Select", Selection { active: true, filled: false }, egui::Sense::click());
        });
    }),
    element!("icon_button_at_with_sense", |ui, _| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(row_height(), row_height()), egui::Sense::hover());
        icon_button_at_with_sense(ui, rect, "close", "Close", egui::Sense::CLICK);
    }),
    element!("list_row_with_sense", "dialog:list:picked", Some("dialog:list:picked:remove"), |ui, _| {
        list_row_with_sense(ui, "picked", "Point 1", egui::Sense::CLICK);
    }),
    element!("selection_field_empty", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, _| {
        row(ui, "Gallery", |ui| {
            selection_field(ui, "Select", Selection { active: false, filled: false });
        });
    }),
    element!("segmented", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            segmented(ui, &mut s.selected, &["One", "Two"], Segments::default());
        });
    }),
    element!("segmented_compact", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            segmented(ui, &mut s.selected, &["Left", "Center", "Right"], Segments { compact: true, ..Default::default() });
        });
    }),
    element!("segmented_states", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            segmented(ui, &mut s.selected, &["One", "Two"], Segments { states: &[true, true], ..Default::default() });
        });
    }),
    element!("segmented_icons", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            segmented_icons(ui, &mut s.selected, &[IconSegment { key: "normal", icon: "normal", tip: "Normal".into() }], &[]);
        });
    }),
    element!("checkbox", "dialog:row:Gallery", Some("dialog:Gallery"), |ui, s| {
        row(ui, "Gallery", |ui| {
            checkbox(ui, "Option", &mut s.checked);
        });
    }),
    element!("checkbox_off", |ui, _| {
        checkbox(ui, "Option", &mut false);
    }),
    element!("checkbox_wrapped", |ui, s| {
        ui.scope(|ui| {
            ui.set_width(Tokens::get().space_24 * 8.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            checkbox(ui, "A long translated checkbox label stays readable on several lines", &mut s.checked);
        });
    }),
    element!("checkbox_disabled_on", |ui, _| {
        ui.add_enabled_ui(false, |ui| checkbox(ui, "Option", &mut true));
    }),
    element!("checkbox_disabled_off", |ui, _| {
        ui.add_enabled_ui(false, |ui| checkbox(ui, "Option", &mut false));
    }),
    element!("checkbox_indeterminate", |ui, _| {
        checkbox_indeterminate(ui, "Option", &mut false, true);
    }),
    element!("checkbox_disabled_mixed", |ui, _| {
        ui.add_enabled_ui(false, |ui| checkbox_indeterminate(ui, "Option", &mut false, true));
    }),
    element!("checkbox_icon", |ui, _| {
        checkbox_icon(ui, "Extrude", "extrude", &mut true);
    }),
    element!("checkbox_focused", |ui, _| {
        egui::Frame::new().inner_margin(Tokens::get().space_4).show(ui, |ui| {
            checkbox(ui, "Option", &mut true).request_focus();
        });
    }),
    element!("checkbox_focused_off", |ui, _| {
        egui::Frame::new().inner_margin(Tokens::get().space_4).show(ui, |ui| {
            checkbox(ui, "Option", &mut false).request_focus();
        });
    }),
    element!("checkbox_focused_mixed", |ui, _| {
        egui::Frame::new().inner_margin(Tokens::get().space_4).show(ui, |ui| {
            checkbox_indeterminate(ui, "Option", &mut false, true).request_focus();
        });
    }),
    element!("checkbox_hovered", |ui, _| {
        checkbox(ui, "Option", &mut false);
    }),
    element!("checkbox_hovered_on", |ui, _| {
        checkbox(ui, "Option", &mut true);
    }),
    element!("checkbox_hovered_mixed", |ui, _| {
        checkbox_indeterminate(ui, "Option", &mut false, true);
    }),
    element!("command_menu_button", |ui, _| {
        command_menu_button(ui, "save", "Save", "save", "Ctrl+S", true);
    }),
    element!("primary_button", "dialog:row:Gallery", Some("dialog:button:Create"), |ui, _| {
        row(ui, "Gallery", |ui| {
            primary_button(ui, "Create");
        });
    }),
    element!("wide_primary_button", |ui, _| {
        wide_primary_button(ui, "Finish Sketch");
    }),
    element!("persistent_button_shape", "gallery:persistent_button", Some("gallery:persistent_button"), |ui, _| {
        let t = Tokens::get();
        let (rect, response) = ui.allocate_exact_size(vec2(t.form_button_height * 3.0, t.form_button_height * 2.0), egui::Sense::click());
        ui.painter().add(persistent_button_shape(ui, &response, rect));
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Selected", crate::type_face::semibold(t.font_label), t.selected_ink);
        solidcraft_ui_registry::publish_target("gallery:persistent_button", rect);
    }),
    element!("native_button_focus", "gallery:native_button_focus", Some("gallery:native_button_focus"), |ui, _| {
        let t = Tokens::get();
        let (rect, response) = ui.allocate_exact_size(vec2(t.form_button_height * 3.0, t.form_button_height), egui::Sense::click());
        response.request_focus();
        ui.painter().rect_filled(rect, t.radius_sm, t.control_fill);
        ui.painter().add(native_button_focus(ui, &response, rect));
        solidcraft_ui_registry::publish_target("gallery:native_button_focus", rect);
    }),
    element!("button", "dialog:row:Gallery", Some("dialog:button:Gallery:Add"), |ui, _| {
        row(ui, "Gallery", |ui| {
            button(ui, "Add", Button::Normal);
        });
    }),
    element!("button_danger_outlined", "dialog:row:Gallery", Some("dialog:button:Gallery:Replace"), |ui, _| {
        row(ui, "Gallery", |ui| {
            button(ui, "Replace", Button::DangerOutlined);
        });
    }),
    element!("button_at", |ui, _| {
        let t = Tokens::get();
        let (rect, _) = ui.allocate_exact_size(vec2(t.space_24 * 5.0, t.file_action_height), egui::Sense::hover());
        button_at(ui, "gallery:button_at", rect, "Recover", Button::Primary);
    }),
    element!("small_button", "dialog:row:Gallery", Some("dialog:button:Gallery:×"), |ui, _| {
        row(ui, "Gallery", |ui| {
            button(ui, "×", Button::Small);
        });
    }),
    element!("checkbox_row", "sketch_palette:toggle:gallery", Some("sketch_palette:toggle:gallery"), |ui, _| {
        checkbox_row(ui, "gallery", "Option", true);
    }),
    element!("checkbox_grid", "dialog:checkbox:First", Some("dialog:First"), |ui, s| {
        checkbox_grid(ui, &mut [("First", &mut s.checked), ("Second", &mut false)], 2);
    }),
    element!("options", "dialog:checkbox:First", Some("dialog:First"), |ui, s| {
        options(ui, "gallery", &mut [("First", &mut s.checked), ("Second", &mut false)]);
    }),
    element!("footer", "dialog:footer", Some("dialog:ok"), |ui, _| {
        footer(ui, "OK", "Cancel", true, Footer::default());
    }),
    element!("footer_apply", "dialog:footer", Some("dialog:apply"), |ui, _| {
        footer(ui, "OK", "Cancel", true, Footer { apply: Some("Apply"), ..Default::default() });
    }),
    element!("footer_status", "dialog:footer", Some("dialog:ok"), |ui, _| {
        footer(ui, "OK", "Cancel", true, Footer { status: Some("Computing…"), ..Default::default() });
    }),
    element!("button_pair", "sketch_palette:look_at", Some("sketch_palette:look_at"), |ui, _| {
        button_pair(
            ui,
            "sketch_palette",
            [false, true],
            [PairButton { key: "look_at", label: "ui.look_at", icon: Some("lookat") }, PairButton { key: "slice", label: "Slice", icon: None }],
        );
    }),
    element!("icon_button_at", |ui, _| {
        let (rect, _) = ui.allocate_exact_size(vec2(row_height(), row_height()), egui::Sense::hover());
        icon_button_at(ui, rect, "close", "Close");
    }),
    element!("mirrored_icon_button_at", |ui, _| {
        let (rect, _) = ui.allocate_exact_size(vec2(row_height(), row_height()), egui::Sense::hover());
        mirrored_icon_button_at(ui, rect, "chevron_right", "Move left");
    }),
    element!("quiet_icon_button_at", |ui, _| {
        let (rect, _) = ui.allocate_exact_size(vec2(row_height(), row_height()), egui::Sense::hover());
        quiet_icon_button_at(ui, rect, "plus", "Add shortcut");
    }),
    element!("shortcut_binding", |ui, _| {
        ui.horizontal(|ui| {
            shortcut_binding(ui, "Ctrl+Shift+PageDown", "Not assigned", true);
            shortcut_binding(ui, "Option+Enter", "Not assigned", false);
            shortcut_binding(ui, "Ctrl++", "Not assigned", true);
        });
        shortcut_binding(ui, "", "Not assigned", true);
    }),
    element!("keyboard", |ui, _| {
        assert_eq!(keyboard(ui.ctx(), true), Action::None);
        ui.allocate_space(vec2(row_height(), row_height()));
    }),
    element!("list_row", "dialog:list:picked", Some("dialog:list:picked:remove"), |ui, _| {
        list_row(ui, "picked", "Point 1");
    }),
    element!("value_pair", "dialog:row:Position", Some("dialog:Y"), |ui, s| {
        value_pair(ui, "Position", [&mut s.value, &mut s.second], ["X", "Y"], None);
    }),
    element!("toggle_icon", "dialog:Alignment:Center", Some("dialog:Alignment:Center"), |ui, s| {
        toggle_icon(ui, "Alignment", "Center", &mut s.checked, "pattern_center");
    }),
    element!("tool_choices", "sketch_palette:feature:line", Some("sketch_palette:feature:line"), |ui, _| {
        tool_choices(ui, "Line", "line", &[ToolChoice { key: "line", icon: "normal", label: "Line", enabled: true }]);
    }),
    element!("status_header", "sketch_palette:meter", None, |ui, _| {
        ui.spacing_mut().item_spacing.y = 0.0;
        status_header(
            ui,
            "sketch_palette",
            "Sketch 1",
            StatusHeader { icon: "sketch", text: "2 degrees of freedom", complete: false, fraction: 0.5 },
        );
    }),
    element!("progress", "dialog:progress:gallery", None, |ui, _| {
        progress(ui, "gallery", 0.4);
    }),
    element!("steps", "dialog:step:gallery:0:done", None, |ui, _| {
        steps(ui, "gallery", &["Read", "Mesh", "Show"], 1, Some("40%"));
    }),
    element!("hint", "dialog:hint:gallery", Some("dialog:hint:gallery"), |ui, _| {
        text(ui, "Pick a point", Text::Hint("gallery"));
    }),
    element!("note", "dialog:note:gallery", None, |ui, _| {
        text(ui, "Counts include the original. Center balances copies; Flip chooses the extra side for even totals.", Text::Note("gallery"));
    }),
    element!("caption", "dialog:caption:Gallery", None, |ui, _| {
        caption(ui, "Gallery", "Construction");
    }),
    element!("status_line", "dialog:status:needs a joint that turns and one that slides", None, |ui, _| {
        status_line(ui, "needs a joint that turns and one that slides", StatusTone::Dim);
    }),
    element!("status_line_warning", |ui, _| {
        status_line(ui, "Pick two different components before creating a joint.", StatusTone::Warning);
    }),
    element!("status_line_error", |ui, _| {
        status_line(ui, "The interference check could not finish. Check the selected bodies.", StatusTone::Error);
    }),
    element!("figure_rows", "dialog:figure:gallery", Some("dialog:Gallery"), |ui, s| {
        let Sample { value, second, .. } = s;
        figure_rows(ui, "gallery", &mut [("Gallery", value), ("Second", second)], |painter, rect, slots| {
            let t = Tokens::get();
            painter.rect_stroke(rect.shrink(t.space_4), t.control_radius, egui::Stroke::new(1.0, t.form_dim), egui::StrokeKind::Inside);
            for slot in slots {
                let ink = if slot.hot { t.form_primary } else { t.form_dim };
                let y = slot.rect.center().y;
                painter.line_segment([egui::pos2(rect.center().x, y), egui::pos2(slot.rect.left(), y)], egui::Stroke::new(1.0, ink));
            }
        });
    }),
    element!("section", "dialog:section:Advanced", Some("dialog:section:Advanced"), |ui, _| {
        section(ui, "gallery", "Advanced", false, |_| {});
    }),
    element!("search_fold", "prefs:search:shortcuts", Some("prefs:search:shortcuts"), |ui, _| {
        search_fold(ui, "Shortcuts", 3, |_| {});
    }),
    element!("divider", "dialog:divider:gallery", None, |ui, _| {
        divider(ui, "gallery");
    }),
    element!("popup_list", |ui, _| {
        let anchor = ui.allocate_space(vec2(100.0, row_height())).1;
        popup_list(
            ui.ctx(),
            ui.scope_id().with("popup"),
            anchor,
            Some(0),
            &[PopupOption { kind: "parameter", text: "Width", detail: "10 mm", color: Tokens::get().text }],
            "Choose",
        );
        // The popup is on a separate layer; reserve its painted allocation for the same checks.
        ui.allocate_space(vec2(ui.available_width(), row_height() * 5.0));
    }),
];

#[cfg(any(test, feature = "test-support"))]
pub fn example(ui: &mut Ui, element: &Element) -> (&'static str, Option<&'static str>) {
    let mut sample = Sample { value: "12 mm".into(), second: "25 mm".into(), selected: 0, checked: true };
    let rect = ui.scope(|ui| (element.draw)(ui, &mut sample)).response.rect;
    if matches!(
        element.name,
        "menu" | "menu_submenu" | "rename_menu" | "workspace_menu" | "command_search" | "history_icons" | "history_names" | "navigation_popup"
    ) {
        // A popup's elevation is painted outside its layout rectangle. Reserve those
        // actual shadow bounds as part of the gallery sample, keeping the paint guard strict.
        ui.expand_to_include_rect(rect + Menu::frame().shadow.margin());
    }
    solidcraft_ui_registry::publish_rect("gallery:allocation", rect);
    (element.rect, element.handle)
}

#[test]
fn every_element_has_geometry_and_stays_in_its_allocation_in_both_themes() {
    check_elements(ELEMENTS, super::test_support::context);
}

/// Every element of `elements` publishes finite geometry and its handle, keeps its paint inside
/// its allocation, and keeps the row height of a form row, in both themes, each drawn in a fresh
/// `context()`.
#[cfg(any(test, feature = "test-support"))]
pub fn check_elements(elements: &[Element], context: fn() -> egui::Context) {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for element in elements {
            let name = element.name;
            let ctx = context();
            ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
            for pass in 0..3 {
                let events = if element.name.starts_with("checkbox_hovered") && pass > 0 {
                    vec![egui::Event::PointerMoved(solidcraft_ui_registry::rect_of("gallery:allocation").expect("hover allocation").center())]
                } else {
                    vec![]
                };
                solidcraft_ui_registry::clear_handles();
                let mut allocation = egui::Rect::NOTHING;
                let mut published = ("", None);
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 600.0))),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_width(320.0);
                        allocation = ui
                            .scope(|ui| {
                                scope(ui, |ui| {
                                    published = example(ui, element);
                                })
                            })
                            .response
                            .rect;
                    },
                );
                output.textures_delta.clear();
                let rect = solidcraft_ui_registry::rect_of(published.0).unwrap_or_else(|| panic!("{name}: missing {}", published.0));
                assert!(rect.is_finite() && rect.is_positive(), "{name}: {rect:?}");
                assert!(allocation.expand(1.0).contains_rect(rect), "{name}: {rect:?} outside {allocation:?}");
                if let Some(handle) = published.1 {
                    assert!(
                        rect.contains(solidcraft_ui_registry::handle_at(handle).unwrap_or_else(|| panic!("{name}: missing {handle}"))),
                        "{name}: handle outside {rect:?}"
                    );
                }
                if published.0.starts_with("dialog:row:") {
                    let expected_height = if name == "sized_row" { row_height() * 2.0 } else { Tokens::get().control_height };
                    assert_eq!(rect.height(), expected_height, "{name}");
                }
                for shape in output.shapes {
                    // Native icon-only buttons keep an empty text shape. It has no vertices;
                    // its sentinel bounds do not describe any painted pixels.
                    if matches!(&shape.shape, egui::Shape::Text(text) if text.galley.num_vertices == 0) {
                        continue;
                    }
                    let painted = shape.shape.visual_bounding_rect().intersect(shape.clip_rect);
                    if painted.is_positive() {
                        assert!(allocation.expand(1.0).contains_rect(painted), "{name}: paint {painted:?} outside {allocation:?}: {:?}", shape.shape);
                    }
                }
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn apply_does_not_submit_and_obeys_validation() {
    for enabled in [false, true] {
        let ctx = super::test_support::context();
        let mut action = Action::None;
        let mut draw = |ui: &mut Ui| {
            action = footer(ui, "OK", "Cancel", enabled, Footer { apply: Some("Apply"), ..Default::default() });
        };
        super::test_support::frame(&ctx, vec![], &mut draw);
        let at = solidcraft_ui_registry::handle_at("dialog:apply").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        assert_eq!(action, if enabled { Action::Apply } else { Action::None });
    }
}

#[test]
fn picked_item_remove_and_paired_values_have_independent_targets() {
    let ctx = super::test_support::context();
    let mut removed = false;
    let mut draw = |ui: &mut Ui| {
        removed = list_row(ui, "point-1", "Point 1").clicked();
    };
    super::test_support::frame(&ctx, vec![], &mut draw);
    let at = solidcraft_ui_registry::handle_at("dialog:list:point-1:remove").unwrap();
    super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
    super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
    assert!(removed);

    let mut x = String::new();
    let mut y = String::new();
    let mut draw = |ui: &mut Ui| {
        value_pair(ui, "Position", [&mut x, &mut y], ["X", "Y"], None);
    };
    super::test_support::frame(&ctx, vec![], &mut draw);
    let row = solidcraft_ui_registry::rect_of("dialog:row:Position").unwrap();
    let first = solidcraft_ui_registry::handle_at("dialog:X").unwrap();
    let second = solidcraft_ui_registry::handle_at("dialog:Y").unwrap();
    assert_eq!(row.height(), Tokens::get().control_height);
    assert!(row.contains(first) && row.contains(second) && first.x < second.x);
    super::test_support::frame(&ctx, super::test_support::pointer(second, true), &mut draw);
    super::test_support::frame(&ctx, super::test_support::pointer(second, false), &mut draw);
    super::test_support::frame(&ctx, vec![egui::Event::Text("25".into())], &mut draw);
    assert_eq!(x, "");
    assert_eq!(y, "25");
}

#[test]
fn gallery_rows_share_a_label_column_after_measurement_in_both_themes() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let ctx = super::test_support::context();
        let mut value = "3".to_owned();
        for _ in 0..3 {
            super::test_support::frame(&ctx, vec![], |ui| {
                row(ui, "Edges", |ui| {
                    selection_field(ui, "Edge 1", Selection { active: true, filled: true });
                });
                row(ui, "Counterbore depth", |ui| {
                    field(ui, &mut value);
                });
            });
        }
        let first = solidcraft_ui_registry::rect_of("dialog:control:Edges").unwrap();
        let second = solidcraft_ui_registry::rect_of("dialog:control:Counterbore depth").unwrap();
        assert_eq!(first.left(), second.left());
        assert_eq!(first.height(), Tokens::get().control_height);
        assert_eq!(second.height(), Tokens::get().control_height);
        let width = solidcraft_ui_registry::rect_of("dialog:label:Edges").unwrap().width();
        assert!(width >= Tokens::get().form_label_width && width <= Tokens::get().form_label_width * 1.6);
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn compound_row_actions_preserve_the_original_field_handle() {
    let ctx = super::test_support::context();
    let mut value = "3 mm".to_owned();
    let mut expected = None;
    super::test_support::frame(&ctx, vec![], |ui| {
        row(ui, "Mid Radius", |ui| {
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(Tokens::get().space_24 * 3.0, row_height()),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        field(ui, &mut value);
                    },
                );
                expected = solidcraft_ui_registry::handle_at("dialog:Mid Radius");
                button(ui, "×", Button::Small);
            });
        });
    });
    assert_eq!(solidcraft_ui_registry::handle_at("dialog:Mid Radius"), expected);
    assert!(expected.is_some());
    assert_ne!(solidcraft_ui_registry::handle_at("dialog:button:Mid Radius:×"), expected);
}

#[test]
fn emphasis_and_compact_modes_keep_independent_actions_in_both_themes() {
    for dark in [false, true] {
        let ctx = super::test_support::context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut bold = false;
        let mut italic = false;
        let mut selected = 0;
        let mut draw = |ui: &mut Ui| {
            row(ui, "Style", |ui| text_emphasis(ui, &mut bold, &mut italic, true, &SAMPLE_EMPHASIS));
            row(ui, "Alignment", |ui| {
                segmented(
                    ui,
                    &mut selected,
                    &["Long translated left", "Long translated centre", "Long translated right"],
                    Segments { compact: true, ..Default::default() },
                );
            });
        };
        super::test_support::frame(&ctx, vec![], &mut draw);
        for handle in ["dialog:Bold", "dialog:Italic", "dialog:Alignment:Long translated centre"] {
            let at = solidcraft_ui_registry::handle_at(handle).unwrap();
            super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
            super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        }
        assert!(bold && italic);
        assert_eq!(selected, 1);
    }
}

#[test]
fn table_cells_keep_alignment_fixed_rows_editing_and_action_targets() {
    for dark in [false, true] {
        let ctx = super::test_support::context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut first = "10 mm".to_owned();
        let mut second = "20 mm".to_owned();
        let mut removed = false;
        let mut draw = |ui: &mut Ui| {
            scope(ui, |ui| {
                table_custom(
                    ui,
                    "cells",
                    &[Column::new("Name", 1.0), Column::new("Expression", 1.5), Column::new("Actions", 0.7)],
                    "Empty",
                    |ui, table| {
                        table.headers(ui, |_, _| false);
                        for (name, value) in [("Width", &mut first), ("Height", &mut second)] {
                            table.row(ui, name, name == "Height", |row| {
                                row.cell(|ui| {
                                    text(ui, name, Text::Cell);
                                });
                                row.cell(|ui| {
                                    let response = cell_editor(ui, ui.scope_id().with("expression"), value, "Expression", false);
                                    solidcraft_ui_registry::publish_handle(&format!("gallery:cell:{name}"), response.rect.center());
                                    solidcraft_ui_registry::publish_rect(&format!("gallery:cell:{name}"), response.rect);
                                });
                                row.cell(|ui| {
                                    if button(ui, "Delete", Button::Small).clicked() {
                                        removed = true;
                                    }
                                });
                            });
                        }
                    },
                );
            });
        };
        for _ in 0..3 {
            super::test_support::frame(&ctx, vec![], &mut draw);
        }
        let first_rect = solidcraft_ui_registry::rect_of("gallery:cell:Width").unwrap();
        let second_rect = solidcraft_ui_registry::rect_of("gallery:cell:Height").unwrap();
        let header = solidcraft_ui_registry::rect_of("dialog:column:cells:Expression").unwrap();
        assert_eq!(first_rect.left(), header.left());
        assert_eq!(second_rect.left(), header.left());
        assert_eq!(first_rect.height(), Tokens::get().control_height);
        assert_eq!(second_rect.height(), Tokens::get().control_height);
        let at = solidcraft_ui_registry::handle_at("gallery:cell:Height").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        super::test_support::frame(&ctx, vec![egui::Event::Text("x".into())], &mut draw);
        let delete = solidcraft_ui_registry::handle_at("dialog:button:Actions:Delete").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(delete, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(delete, false), &mut draw);
        assert_eq!(first, "10 mm");
        assert!(second.contains('x'));
        assert!(removed);
    }
}

#[test]
fn popup_suggestions_and_selectable_lists_keep_click_selection() {
    for dark in [false, true] {
        let ctx = super::test_support::context();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut selected = None;
        let mut draw = |ui: &mut Ui| {
            selected = popup_list(
                ui.ctx(),
                egui::Id::unique("suggestions"),
                egui::Rect::from_min_size(egui::pos2(10.0, 10.0), vec2(220.0, 24.0)),
                Some(0),
                &[
                    PopupOption { kind: "parameter", text: "width", detail: "Parameter · 10 mm", color: Tokens::get().text },
                    PopupOption { kind: "parameter", text: "height", detail: "Parameter · 20 mm", color: Tokens::get().text },
                ],
                "Choose",
            );
        };
        for _ in 0..3 {
            super::test_support::frame(&ctx, vec![], &mut draw);
        }
        let at = solidcraft_ui_registry::handle_at("dialog:suggestion:1").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        assert_eq!(selected, Some(1));
        let mut picked = false;
        let mut draw = |ui: &mut Ui| {
            scope(ui, |ui| {
                if list_picker(ui, "preset", "Preset", picked).clicked() {
                    picked = !picked;
                }
            });
        };
        super::test_support::frame(&ctx, vec![], &mut draw);
        let at = solidcraft_ui_registry::handle_at("dialog:list:preset").unwrap();
        super::test_support::frame(&ctx, super::test_support::pointer(at, true), &mut draw);
        super::test_support::frame(&ctx, super::test_support::pointer(at, false), &mut draw);
        assert!(picked);
    }
}

#[test]
fn many_draggable_samples_wrap_inside_the_available_width() {
    let ctx = super::test_support::context();
    let mut rects = Vec::new();
    let mut bounds = egui::Rect::NOTHING;
    super::test_support::frame(&ctx, vec![], |ui| {
        ui.set_width(Tokens::get().form_label_width * 2.0);
        scope(ui, |ui| {
            bounds = ui.available_rect_before_wrap();
            swatch_grid(ui, &(0..20).collect::<Vec<_>>(), |ui, index| {
                let response = color_swatch(ui, ui.scope_id().with(index), Tokens::get().accent, false, "Sample", *index);
                rects.push(response.rect);
            });
        });
    });
    assert_eq!(rects.len(), 20);
    assert!(rects.iter().all(|rect| rect.right() <= bounds.right() && rect.width() == Tokens::get().control_height + Tokens::get().space_6));
    assert!(rects.last().unwrap().top() > rects.first().unwrap().top(), "samples must wrap to additional rows");
}

#[test]
fn gallery_covers_every_inline_element() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let names: std::collections::BTreeSet<_> = ELEMENTS.iter().map(|e| e.name).collect();
    assert_eq!(names.len(), ELEMENTS.len(), "duplicate gallery entries");
    for file in [
        "actions.rs",
        "checks.rs",
        "choice.rs",
        "color.rs",
        "disclosure.rs",
        "fields.rs",
        "framing.rs",
        "layout.rs",
        "look_chain.rs",
        "menu.rs",
        "metrics.rs",
        "preferences_cards.rs",
        "radial.rs",
        "reports.rs",
        "sphere.rs",
        "status.rs",
        "table.rs",
        "theme_picker.rs",
        "tree.rs",
    ] {
        let source = std::fs::read_to_string(root.join(file)).unwrap();
        let ast = syn::parse_file(&source).unwrap();
        for item in ast.items {
            if let syn::Item::Fn(item) = item
                && matches!(&item.vis, syn::Visibility::Restricted(v) if v.path.is_ident("crate"))
            {
                let name = item.sig.ident.to_string();
                // Inherited labels, state keys and accessors, parsers and size measurements do not render elements.
                if !matches!(
                    name.as_str(),
                    "dimension"
                        | "eye_drag_id"
                        | "expressions"
                        | "icon_label_width"
                        | "label"
                        | "literal"
                        | "parse_number"
                        | "placement_field_content_size"
                        | "scrub_step"
                        | "scrub_text"
                        | "scrubbed"
                        | "selection_ink"
                ) {
                    assert!(names.contains(name.as_str()), "{file}: {name} needs a gallery entry");
                }
            }
        }
    }
}

/// CPU-rendered review board using the actual gallery widgets, without a window.
#[test]
fn material_widgets_board_in_both_themes() {
    use super::test_support::{raster, textures_update};
    use std::collections::HashMap;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        let ctx = super::test_support::context();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(360.0, 240.0));
        let mut textures = HashMap::new();
        let mut image = None;
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                ui.set_width(320.0);
                ui.painter().rect_filled(bounds, 0.0, Tokens::get().panel);
                scope(ui, |ui| {
                    for name in ["sphere_thumbnail", "look_chain", "look_chain_empty"] {
                        example(ui, ELEMENTS.iter().find(|e| e.name == name).unwrap());
                        ui.add_space(Tokens::get().space_12);
                    }
                });
            });
            textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
        }
        let image = image.unwrap();
        assert!(image.pixels().any(|p| p.0[3] != 0));
        if let Some(dir) = std::env::var_os("SOLIDCRAFT_MATERIAL_WIDGET_EVIDENCE") {
            std::fs::create_dir_all(&dir).unwrap();
            image.save(std::path::Path::new(&dir).join(if dark { "material-widgets-dark.png" } else { "material-widgets-light.png" })).unwrap();
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn tree_icon_actions_keep_repeated_light_rows_independent() {
    let ctx = super::test_support::context();
    let mut ids = std::collections::HashSet::new();
    super::test_support::frame(&ctx, vec![], |ui| {
        for index in 0..3 {
            let target = format!("light:{index}:visibility");
            let actions = [TreeAction { icon: "eye", hover_only: false, label: "Hide", reason: Some("Not available yet"), target: &target }];
            let row = tree_row(ui, ui.scope_id().with(index), &Row { compact: true, label: "Directional", actions: &actions, ..Default::default() });
            for action in row.actions {
                assert!(ids.insert(action.id), "each light action must have its own widget identity");
            }
        }
    });
    assert_eq!(ids.len(), 3);
}

#[test]
fn card_looks_follow_selected_hover_disabled_and_focus() {
    use super::preferences_cards::{CardFill, CardInk, card_look};
    let rest = card_look(false, true, false, false);
    assert_eq!((rest.border, rest.fill, rest.ink, rest.bold, rest.alpha, rest.focus_ring), (1.0, CardFill::Face, CardInk::Text, false, 1.0, false));
    let hover = card_look(false, true, true, false);
    assert_eq!(hover.fill, CardFill::Hover);
    let selected = card_look(true, true, true, false);
    assert_eq!((selected.border, selected.fill, selected.ink, selected.bold), (2.0, CardFill::Selected, CardInk::Accent, true));
    let disabled = card_look(false, false, true, true);
    assert_eq!((disabled.fill, disabled.alpha, disabled.focus_ring), (CardFill::Face, 0.5, false), "a disabled card neither hovers nor shows focus");
    assert!(card_look(false, true, false, true).focus_ring);
    assert!(card_look(true, false, false, false).alpha < 1.0, "a selected card can be disabled too");
}

#[test]
fn cards_size_to_their_container_and_survive_hostile_sizes() {
    use super::preferences_cards::card_width;
    assert_eq!(card_width(320.0, 4, 10.0), 72.5);
    assert_eq!(card_width(320.0, 0, 10.0), 320.0);
    for bad in [0.0, -5.0, f32::NAN, f32::INFINITY, f32::MAX] {
        let w = card_width(bad, 3, 10.0);
        assert!(w.is_finite() && w >= 0.0, "{bad}: {w}");
    }
    // 400 wide: two arrows and their gaps leave 332, which fits three 88-wide items with gaps.
}

#[cfg(any(test, feature = "test-support"))]
pub fn sample_card<'a>(label: &'a str, selected: bool, enabled: bool) -> super::preferences_cards::Card<'a> {
    super::preferences_cards::Card {
        label,
        sub: Some("sub"),
        selected,
        enabled,
        options: None,
        check: selected,
        aspect: 120.0 / 64.0,
        figure: &super::preferences_figure_data::paint_tess_normal,
    }
}

#[test]
fn picture_cards_and_tiles_click_only_when_enabled_and_never_panic() {
    use super::preferences_cards::{picture_card, toggle_tile};
    let ctx = super::test_support::context();
    for tile in [false, true] {
        for enabled in [true, false] {
            let mut clicked = false;
            let mut at = None;
            for pass in 0..3 {
                let events = match (pass, at) {
                    (1, Some(p)) => super::test_support::pointer(p, true),
                    (2, Some(p)) => super::test_support::pointer(p, false),
                    _ => vec![],
                };
                super::test_support::frame(&ctx, events, |ui| {
                    let card = sample_card("Normal", false, enabled);
                    let response = if tile { toggle_tile(ui, &card, 120.0) } else { picture_card(ui, &card, 120.0) };
                    at = Some(response.rect.center());
                    assert!(response.rect.width() == 120.0 && response.rect.height() > 40.0);
                    clicked |= response.clicked();
                });
            }
            assert_eq!(clicked, enabled, "tile {tile}, enabled {enabled}");
            // Zero, negative and non-finite widths must not panic and must stay finite.
            super::test_support::frame(&ctx, vec![], |ui| {
                for width in [0.0, -3.0, f32::NAN, f32::INFINITY, 1.0e6] {
                    let response = if tile {
                        toggle_tile(ui, &sample_card("x", true, enabled), width)
                    } else {
                        picture_card(ui, &sample_card("x", true, enabled), width)
                    };
                    assert!(response.rect.is_finite());
                }
            });
        }
    }
}

#[test]
fn preset_card_and_help_icon_behave() {
    use super::preferences_cards::{help_icon, preset_card};
    let ctx = super::test_support::context();
    let icon = |p: &egui::Painter, r: egui::Rect, ink: egui::Color32| super::icons::paint_ink(p, r, "settings", ink);
    super::test_support::frame(&ctx, vec![], |ui| {
        for width in [0.0, 10.0, 204.0, f32::NAN] {
            let response = preset_card(ui, "Performance", "Fastest", true, true, &icon, width);
            assert!(response.rect.is_finite());
        }
        let help = help_icon(ui, "Help text");
        assert!(help.rect.width() > 8.0 && help.rect.width() < 20.0);
    });
}

/// The (i) shows its text after a click or a key, not only on hover: a click opens the popover, a
/// click outside closes it, and Enter or Space on the focused icon opens it again until Escape closes it.
#[test]
fn help_icon_opens_its_text_on_a_click_or_a_key() {
    use super::preferences_cards::help_icon;
    use super::test_support::{frame, frame_cursor, key, pointer};
    let ctx = super::test_support::context();
    let focus = std::cell::Cell::new(false);
    let mut draw = |ui: &mut Ui| {
        let response = help_icon(ui, "Help text");
        if focus.replace(false) {
            response.request_focus();
        }
    };
    let popover = || solidcraft_ui_registry::rect_of("prefs:help_icon:popover");
    frame(&ctx, vec![], &mut draw);
    let at = solidcraft_ui_registry::handle_at("prefs:help_icon").expect("the icon publishes its handle");
    assert_eq!(frame_cursor(&ctx, at, &mut draw), egui::CursorIcon::PointingHand);
    for pressed in [true, false] {
        frame(&ctx, pointer(at, pressed), &mut draw);
    }
    frame(&ctx, vec![], &mut draw);
    assert!(popover().is_some(), "a click on the icon opens the popover");
    let outside = egui::pos2(300.0, 390.0);
    for pressed in [true, false] {
        frame(&ctx, pointer(outside, pressed), &mut draw);
    }
    frame(&ctx, vec![], &mut draw);
    assert!(popover().is_none(), "a click outside closes the popover");
    focus.set(true);
    frame(&ctx, vec![], &mut draw);
    frame(&ctx, key(egui::Key::Enter), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(popover().is_some(), "Enter on the focused icon opens the popover");
    frame(&ctx, key(egui::Key::Escape), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(popover().is_none(), "Escape closes the popover");
    focus.set(true);
    frame(&ctx, vec![], &mut draw);
    frame(&ctx, key(egui::Key::Space), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(popover().is_some(), "Space on the focused icon opens the popover");
    frame(&ctx, key(egui::Key::Escape), &mut draw);
    frame(&ctx, vec![], &mut draw);
    assert!(popover().is_none(), "Escape closes the popover again");
}

/// Light and dark boards of the shared Preferences widgets and of every figure, rendered on the CPU.
/// Set `SOLIDCRAFT_PREFERENCE_WIDGET_EVIDENCE` to a directory to keep the PNGs.
#[test]
fn preference_widgets_and_figures_board_in_both_themes() {
    use super::test_support::{raster, textures_update};
    use std::collections::HashMap;
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for (board, names) in [
            ("widgets", &["picture_card", "toggle_tile", "preset_card", "help_icon", "theme_browser", "preference_theme_thumbnails"][..]),
            ("figures", &["preference_figures"][..]),
        ] {
            for scale in [1.0_f32, 2.0] {
                let ctx = super::test_support::context();
                ctx.set_pixels_per_point(scale);
                let bounds = egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(if board == "figures" { 700.0 } else { 360.0 }, if board == "figures" { 920.0 } else { 800.0 }),
                );
                let mut textures = HashMap::new();
                let mut image = None;
                for _ in 0..3 {
                    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ui| {
                        ui.set_width(bounds.width() - 40.0);
                        ui.painter().rect_filled(bounds, 0.0, Tokens::get().dialog_body);
                        ui.add_space(12.0);
                        scope(ui, |ui| {
                            for name in names {
                                example(ui, ELEMENTS.iter().find(|e| e.name == *name).unwrap());
                                ui.add_space(Tokens::get().space_12);
                            }
                        });
                    });
                    textures_update(&mut textures, &output.textures_delta);
                    output.textures_delta.clear();
                    image = Some(raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures));
                }
                let image = image.unwrap();
                assert!(image.pixels().any(|p| p.0[3] != 0));
                if let Some(dir) = std::env::var_os("SOLIDCRAFT_PREFERENCE_WIDGET_EVIDENCE") {
                    std::fs::create_dir_all(&dir).unwrap();
                    let suffix = if scale > 1.0 { "@2x" } else { "" };
                    image
                        .save(std::path::Path::new(&dir).join(format!("preferences-{board}-{}{suffix}.png", if dark { "dark" } else { "light" })))
                        .unwrap();
                }
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}
