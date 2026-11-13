//! App chrome: the application bar, the ribbon, the history bar, the view compass, the navigation
//! bar's buttons, the window's caption buttons and the theme picker's parts.

use super::{Group, Page};
use crate::state;
use egui::{Rect, pos2, vec2};
use solidcraft_theme::{AccentRamp, TabAccent, Tokens};
use solidcraft_ui_kit as kit;
use solidcraft_ui_kit::{app_bar, history_bar, ribbon, view_compass, window_chrome};

pub const APP_BAR: Page = Page {
    id: "app-bar",
    title: "Application bar",
    group: Group::Chrome,
    items: &[
        "app_bar::show",
        "app_bar::AppBar",
        "app_bar::BarButton",
        "app_bar::BarAction",
        "app_bar::BarOut",
        "app_bar::Handles",
        "app_bar::HEIGHT",
    ],
    what: "The bar across the top of the window: the mark and name, Home, File, the file actions, search, help, settings and Report a bug. The app describes it each frame and runs the actions it returns.",
    usage: r#"
let out = ui_kit::app_bar::show(ui, &ui_kit::app_bar::AppBar {
    name: "SolidCraft", file: "File", home, actions: vec![save, undo], help, settings, report, /* … */
}, bar_rect, content_rect);
for action in out.actions { /* BarAction::Home, File { rect }, Button { id }, … */ }
"#,
    dos: &["Leave out of a compact bar what has a shortcut elsewhere (`wide_only`)."],
    donts: &["Don't add buttons to the bar without the owner's review: it is the app's first impression."],
    demo: |ui, d| {
        let width = ui.available_width();
        let (bar, _) = ui.allocate_exact_size(vec2(width, app_bar::HEIGHT), egui::Sense::hover());
        let button = |id, icon, tip| app_bar::BarButton { id, icon, tip, enabled: true, narrow: true, wide_only: false };
        let data = app_bar::AppBar {
            mark: &|painter, at| solidcraft_look::glyphs::brand_mark(painter, at),
            name: "SolidCraft",
            home: app_bar::BarButton { narrow: false, ..button("home", "home", "Home") },
            file: "File",
            file_open: false,
            plugins: None,
            actions: vec![
                button("save", "save", "Save"),
                app_bar::BarButton { enabled: false, wide_only: true, ..button("undo", "undo", "Undo") },
                button("redo", "redo", "Redo"),
            ],
            quick: 0,
            help: app_bar::BarButton { narrow: false, ..button("help", "help", "Help") },
            settings: app_bar::BarButton { narrow: false, ..button("settings", "settings", "Settings") },
            report: app_bar::Report { label: "Report a bug", tip: "Report a problem", icon_only: false },
            search: false,
            search_shortcut: None,
            assistant: false,
            version: Some(app_bar::Version { text: "v0.40", url: "https://docs.solidcraft.io/" }),
            handles: app_bar::Handles {
                home: &["gallery:bar:home"],
                home_rect: &["gallery:bar:home"],
                file: &["gallery:bar:file"],
                file_rect: &["gallery:bar:file"],
                plugins: "gallery:bar:plugins",
                button: &["gallery:bar:"],
                separator: "gallery:bar:separator:",
                search: "gallery:bar:search",
                report: "gallery:bar:report",
                settings: "gallery:bar:settings",
                version: "gallery:bar:version",
            },
        };
        let out = app_bar::show(ui, &data, bar, bar);
        // The version would open a link; the gallery only says what was chosen.
        if let Some(action) = out.actions.first() {
            d.said(format!("{action:?}"));
        }
    },
};

pub const RIBBON: Page = Page {
    id: "ribbon",
    title: "Ribbon",
    group: Group::Chrome,
    items: &[
        "ribbon::show",
        "ribbon::Row",
        "ribbon::Group",
        "ribbon::Button",
        "ribbon::ButtonSize",
        "ribbon::Action",
        "ribbon::Handles",
        "ribbon::fit",
    ],
    what: "The row of command groups under the tabs. It fits the room it has: groups give up buttons one at a time, then collapse to one button, then move into the » menu, and none is dropped.",
    usage: r#"
let out = ui_kit::ribbon::show(ui, &ui_kit::ribbon::Row { groups, labels: true, accent, exit: false,
    reveal: None, more_tip: "More", handles }, rect, &mut |ui, group| false);
for action in out.actions { if let ui_kit::ribbon::Action::Run { id, .. } = action { run(&id) } }
"#,
    dos: &["Order a group's buttons by how often they are used: the last ones go first when room runs out."],
    donts: &["Don't add a button to a ribbon group without the owner's review and a 1280×720 screenshot."],
    demo: |ui, d| {
        let t = Tokens::get();
        let size = ribbon::ButtonSize { width: ribbon::BUTTON_MAX_WIDTH, min_width: ribbon::BUTTON_MIN_WIDTH };
        let button = |id: &'static str, label: &'static str, icon: &'static str| ribbon::Button {
            id,
            run: id,
            label,
            name: label,
            icon,
            size,
            enabled: true,
            pressed: false,
            exit: false,
            split: None,
            app_paints: false,
            covers: &[],
        };
        let group = |key: &'static str, label: &'static str, buttons: Vec<ribbon::Button<'static>>| ribbon::Group {
            key,
            name: label,
            label,
            tip: label,
            commands: buttons.iter().map(|b| b.id).collect(),
            buttons,
            cells: Vec::new(),
            icon: Some("box"),
            enabled: true,
            open: false,
            running: None,
            more: Vec::new(),
        };
        let row = ribbon::Row {
            groups: vec![
                group(
                    "create",
                    "Create",
                    vec![
                        button("sketch", "Sketch", "sketch"),
                        button("extrude", "Extrude", "extrude"),
                        button("revolve", "Revolve", "revolve"),
                        button("loft", "Loft", "loft"),
                    ],
                ),
                group(
                    "modify",
                    "Modify",
                    vec![button("fillet", "Fillet", "fillet"), button("chamfer", "Chamfer", "chamfer"), button("shell", "Shell", "shell")],
                ),
                group(
                    "inspect",
                    "Inspect",
                    vec![button("measure", "Measure", "measure"), ribbon::Button { enabled: false, ..button("section", "Section", "section") }],
                ),
            ],
            labels: true,
            accent: TabAccent::for_ramp(&t, AccentRamp::Main),
            exit: false,
            reveal: None,
            more_tip: "More",
            handles: ribbon::Handles {
                button: &["gallery:ribbon:"],
                button_rect: "gallery:ribbon:rect:",
                split: "gallery:ribbon:split:",
                cell: &["gallery:ribbon:cell:"],
                collapsed: "gallery:ribbon:collapsed:",
                section: "gallery:ribbon:section:",
                section_target: "gallery:ribbon:section_target:",
                group: "gallery:ribbon:group:",
                running: "gallery:ribbon:running:",
                reveal: "gallery:ribbon:reveal",
                more: "gallery:ribbon:more",
                more_row: "gallery:ribbon:more:",
                collapsed_count: "gallery:ribbon:collapsed_count",
                more_count: "gallery:ribbon:more_count",
                trimmed_count: "gallery:ribbon:trimmed_count",
                drag: "gallery:ribbon:drag",
                drag_remove: "gallery:ribbon:drag_remove",
            },
        };
        for (name, width) in [("Room for everything", ui.available_width().min(900.0)), ("Narrow: groups give up buttons and collapse", 300.0)] {
            state(ui, name, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(width, ribbon::GROUP_HEIGHT), egui::Sense::hover());
                ui.painter().rect_filled(rect, t.radius_sm, t.toolbar);
                let out = ribbon::show(ui, &row, rect, &mut |_, _| false);
                for action in out.actions {
                    d.said(format!("{action:?}"));
                }
            });
        }
    },
};

/// The history bar's sample steps.
struct Steps<'a> {
    steps: &'a [(&'a str, &'a str)],
    selected: usize,
}

impl history_bar::Contents for Steps<'_> {
    fn entries(&mut self, strip: &mut history_bar::Strip<'_, '_>) {
        let tip = || String::new();
        for (slot, (label, icon)) in self.steps.iter().enumerate() {
            strip.step(&history_bar::Step {
                id: slot as u64,
                slot,
                label,
                icon,
                icon_accent: None,
                state: history_bar::StepState { selected: slot == self.selected, rolled_back: slot >= 4, last_done: slot == 3, ..Default::default() },
                error: slot == 2,
                warning: false,
                ink: None,
                tip: &tip,
            });
        }
    }
}

pub const HISTORY_BAR: Page = Page {
    id: "history-bar",
    title: "History bar",
    group: Group::Chrome,
    items: &["history_bar::strip", "history_bar::Bar", "history_bar::Step", "history_bar::StepState", "history_bar::Contents", "history_bar::Action"],
    what: "The timeline of a design's features along the bottom: one chip per step, the marker that rolls the design back, steps that failed in red and steps after the marker faded.",
    usage: r#"
impl ui_kit::history_bar::Contents for MySteps {
    fn entries(&mut self, strip: &mut ui_kit::history_bar::Strip<'_, '_>) {
        for step in &self.steps { strip.step(&step.chip()); }
    }
}
let out = ui_kit::history_bar::strip(ui, &bar, bar_rect, rect, "timeline", &mut steps);
"#,
    dos: &["Turn `reorder` off for a history whose order cannot change."],
    donts: &["Don't reorder your data while drawing: act on `Action::Reorder` after the frame."],
    demo: |ui, d| {
        let t = Tokens::get();
        let width = ui.available_width().min(700.0);
        let (rect, _) = ui.allocate_exact_size(vec2(width, 44.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, t.radius_sm, t.toolbar);
        let accent = TabAccent::for_ramp(&t, AccentRamp::Main);
        let bar = history_bar::Bar {
            marker_tip: "Drag to roll back",
            accent,
            drag_accent: accent,
            marker: 4,
            steps: 6,
            floating: false,
            reorder: true,
            handles: history_bar::Handles { marker: "gallery:history:marker", step: "gallery:history:step:" },
        };
        let steps = [
            ("Sketch1", "sketch"),
            ("Extrude1", "extrude"),
            ("Fillet1", "fillet"),
            ("Shell1", "shell"),
            ("Chamfer1", "chamfer"),
            ("Pattern1", "pattern_rect"),
        ];
        let mut contents = Steps { steps: &steps, selected: d.selected.min(5) };
        let out = history_bar::strip(ui, &bar, rect, rect, "gallery_history", &mut contents);
        for action in out.actions {
            if let history_bar::Action::Select { step, .. } = action {
                d.selected = step as usize;
            }
            d.said(format!("{action:?}"));
        }
    },
};

pub const VIEW_COMPASS: Page = Page {
    id: "view-compass",
    title: "View compass",
    group: Group::Chrome,
    items: &["view_compass::show", "view_compass::Compass", "view_compass::ViewAxes", "view_compass::ViewAction", "view_compass::orbit"],
    what: "The axes in the corner of the 3D view: click a tip to look along that axis, Home to return, drag to orbit. It draws the camera's axes as given and returns what was chosen.",
    usage: r#"
let action = ui_kit::view_compass::show(ui, &ui_kit::view_compass::Compass {
    clip, centre, axes: camera_axes, hover: ui.ctx().pointer_hover_pos(), orbiting: false, snap_hint: true, home_label: "Home".into(),
});
"#,
    dos: &["Pass the pointer only when it may act on the compass."],
    donts: &["Don't move the camera yourself from the compass: hand its action to the one camera path."],
    demo: |ui, d| {
        let (rect, _) = ui.allocate_exact_size(vec2(220.0, 200.0), egui::Sense::hover());
        let (s, c) = (0.6_f64.sin(), 0.6_f64.cos());
        let axes = view_compass::ViewAxes { right: [c, s, 0.0], up: [-s * 0.5, c * 0.5, 0.866], back: [s * 0.866, -c * 0.866, 0.5] };
        let compass = view_compass::Compass {
            clip: rect,
            centre: rect.center(),
            axes,
            hover: ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p)),
            orbiting: false,
            snap_hint: true,
            home_label: "Home".into(),
        };
        if let Some(action) = view_compass::show(ui, &compass) {
            d.said(format!("{action:?}"));
        }
    },
};

pub const NAVIGATION: Page = Page {
    id: "navigation",
    title: "Navigation bar buttons",
    group: Group::Chrome,
    items: &[
        "navigation_surface",
        "navigation_button",
        "NavigationButton",
        "NavigationKind",
        "NavigationMetrics",
        "NavigationSize",
        "navigation_separator",
        "navigation_edit_outline",
    ],
    what: "The floating bar of view tools over the 3D view (pan, fit, visual style): its card and its buttons, regular or compact, a tool held down or an action.",
    usage: r#"
ui_kit::navigation_surface(ui, rect, ui_kit::NavigationSize::Regular, false);
ui_kit::navigation_button(ui, button_rect, ui_kit::NavigationButton {
    id: "pan", icon: "pan", label: "Pan", kind: ui_kit::NavigationKind::Tool, selected: true, labels: false, menu: false,
});
"#,
    dos: &["Use `Tool` for modes that stay on and `Action` for one-off commands."],
    donts: &["Don't put model commands here: it is for looking at the model."],
    demo: |ui, d| {
        for (name, size) in [("Regular", kit::NavigationSize::Regular), ("Compact", kit::NavigationSize::Compact)] {
            state(ui, name, |ui| {
                let metrics = kit::NavigationMetrics::for_size(size);
                let inset = kit::NAV_PADDING + kit::NAV_BORDER;
                let buttons = [
                    ("orbit", "orbit", "Orbit", kit::NavigationKind::Tool),
                    ("pan", "pan", "Pan", kit::NavigationKind::Tool),
                    ("zoom", "zoom", "Zoom", kit::NavigationKind::Tool),
                    ("fit", "fit", "Fit", kit::NavigationKind::Action),
                ];
                let width = buttons.len() as f32 * (metrics.button + kit::NAV_GAP) - kit::NAV_GAP + 2.0 * inset;
                let (rect, _) = ui.allocate_exact_size(vec2(width, metrics.height()), egui::Sense::hover());
                kit::navigation_surface(ui, rect, size, false);
                let mut x = rect.left() + inset;
                for (i, (id, icon, label, kind)) in buttons.into_iter().enumerate() {
                    let bounds = Rect::from_min_size(pos2(x, rect.top() + inset), vec2(metrics.button, metrics.button));
                    let selected = kind == kit::NavigationKind::Tool && d.segment == i;
                    if kit::navigation_button(ui, bounds, kit::NavigationButton { id, icon, label, kind, selected, labels: false, menu: false })
                        .clicked()
                    {
                        if kind == kit::NavigationKind::Tool {
                            d.segment = i;
                        }
                        d.said(label);
                    }
                    x += metrics.button + kit::NAV_GAP;
                }
            });
        }
    },
};

pub const WINDOW_CHROME: Page = Page {
    id: "window-chrome",
    title: "Window chrome",
    group: Group::Chrome,
    items: &[
        "window_chrome::caption_buttons",
        "window_chrome::TitleBar",
        "window_chrome::WindowAction",
        "window_chrome::resize_zones",
        "window_chrome::drag_area",
    ],
    what: "The window's own title bar on Windows and Linux: minimise, maximise and close at the right end, the empty bar moves the window, and resize zones along the edges. On macOS the system's traffic lights sit at the left.",
    usage: r#"
let layout = ui_kit::window_chrome::TitleBar { traffic_lights: false, caption_buttons: true }.layout(bar);
if let Some(action) = ui_kit::window_chrome::caption_buttons(ui, bar) { action.send(ui.ctx()); }
"#,
    dos: &["Send the action to the window with `WindowAction::send`."],
    donts: &["Don't draw caption buttons on macOS: the system draws them."],
    demo: |ui, d| {
        let t = Tokens::get();
        let width = ui.available_width().min(600.0);
        let (bar, _) = ui.allocate_exact_size(vec2(width, app_bar::HEIGHT), egui::Sense::hover());
        ui.painter().rect_filled(bar, 0.0, t.app_bar);
        // Here the buttons only say what they would do: the gallery's own window stays as it is.
        if let Some(action) = window_chrome::caption_buttons(ui, bar) {
            d.said(action.name());
        }
    },
};

pub const THEME_PICKER: Page = Page {
    id: "theme-picker",
    title: "Theme picker",
    group: Group::Chrome,
    items: &["theme_segments", "ThemeSegment", "ThemeGlyph", "theme_card", "ThemeCard", "ThemeSample", "theme_import_button", "theme_empty"],
    what: "The parts of Preferences › Appearance: segments that filter themes by tone, a card per theme with a picture of its palette, the import button and the empty state. The Theme panel of this gallery edits the same tokens.",
    usage: r#"
let tone = ui_kit::theme_segments(ui, "tone", &[ui_kit::ThemeSegment::new("Light", ui_kit::ThemeGlyph::Sun, Some(9)),
    ui_kit::ThemeSegment::new("Dark", ui_kit::ThemeGlyph::Moon, Some(17))], Some(0), false);
ui_kit::theme_card(ui, &ui_kit::ThemeCard { name: "Nord", note: "Built in", sample: ui_kit::ThemeSample::from_tokens(&tokens), /* … */ }, 160.0);
"#,
    dos: &["Draw a theme's card from its resolved tokens, so the picture is the theme."],
    donts: &["Don't hard-code a theme's colours in its card."],
    demo: |ui, d| {
        state(ui, "Tone segments", |ui| {
            let options =
                [kit::ThemeSegment::new("Light", kit::ThemeGlyph::Sun, Some(9)), kit::ThemeSegment::new("Dark", kit::ThemeGlyph::Moon, Some(17))];
            if let Some(i) = kit::theme_segments(ui, "gallery_tone", &options, Some(d.choice.min(1)), false) {
                d.choice = i;
            }
        });
        state(ui, "Cards", |ui| {
            ui.horizontal(|ui| {
                for (i, (name, tokens, dark)) in
                    [("Light", Tokens::light(), false), ("Dark", Tokens::dark(), true), ("Current", Tokens::get(), solidcraft_theme::is_dark())]
                        .into_iter()
                        .enumerate()
                {
                    let card = kit::ThemeCard {
                        name,
                        note: "Built in",
                        options: None,
                        sample: kit::ThemeSample::from_tokens(&tokens),
                        selected: d.swatch == i,
                        dark,
                        high: false,
                        day: false,
                        night: false,
                    };
                    if kit::theme_card(ui, &card, 150.0).clicked() {
                        d.swatch = i;
                        d.said(name);
                    }
                }
            });
        });
        state(ui, "Import and empty", |ui| {
            kit::theme_import_button(ui, "Import theme file…");
            kit::theme_empty(ui, "No themes match", "Clear filters", || {});
        });
    },
};
