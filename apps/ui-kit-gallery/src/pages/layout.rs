//! Layout and sections: collapsible sections, disclosure arrows, text styles, windows, details.

use super::{Group, Page};
use crate::state;
use egui::vec2;
use solidcraft_theme::Tokens;
use solidcraft_ui_kit as kit;

pub const SECTION: Page = Page {
    id: "section",
    title: "Sections",
    group: Group::Layout,
    items: &["section", "section_count", "editor_section", "method_section", "section_heading", "divider", "search_fold"],
    what: "A titled part of a dialog or panel that folds open and closed, optionally with a count. Headings and dividers split a long form without folding.",
    usage: r#"
ui_kit::section(ui, "advanced", "Advanced", false, |ui| { /* rows */ });            // id, title, open at first
ui_kit::section_count(ui, "params", "Parameters", true, Some(rows.len()), |ui| { /* … */ });
ui_kit::section_heading(ui, "Output");
ui_kit::divider(ui, "after_output");
"#,
    dos: &["Fold rarely used settings into a closed \"Advanced\" section.", "Show the count when a section lists things."],
    donts: &["Don't fold the settings every use needs.", "Don't nest sections."],
    demo: |ui, d| {
        state(ui, "Open and closed (click the titles)", |ui| {
            kit::section(ui, "gallery_general", "General", true, |ui| {
                kit::text_row(ui, "Name", &mut d.text, "");
            });
            kit::section(ui, "gallery_advanced", "Advanced", false, |ui| {
                kit::checkbox(ui, "Keep tools", &mut d.second);
            });
        });
        state(ui, "With a count", |ui| {
            kit::section_count(ui, "gallery_count", "Parameters", false, Some(d.rows.len()), |ui| {
                for (name, value) in &d.rows {
                    kit::text(ui, &format!("{name} = {value}"), kit::Text::Inline);
                }
            });
        });
        state(ui, "Editor section", |ui| {
            kit::editor_section(ui, "gallery_edit", "Edit", true, |ui| {
                kit::row(ui, "Colour", |ui| kit::color_field(ui, &mut d.rgb));
            });
        });
        state(ui, "Heading and divider", |ui| {
            kit::section_heading(ui, "Output");
            kit::divider(ui, "gallery_divider");
        });
    },
};

pub const DISCLOSURE: Page = Page {
    id: "disclosure",
    title: "Disclosure and strip toggle",
    group: Group::Layout,
    items: &["disclosure", "disclosure_button_at", "panel_disclosure_button_at", "dropdown_indicator", "strip_toggle"],
    what: "The arrow that shows something folds, painted where a component needs it, and the toggle that shows or hides a strip along a panel's edge.",
    usage: r#"
ui_kit::disclosure(ui.painter(), centre, open, t.form_dim);
if ui_kit::strip_toggle(ui, expanded, "Shots").clicked() { expanded = !expanded; }
"#,
    dos: &["Point it down when open, right when closed, as every kit section does."],
    donts: &["Don't draw your own triangles or chevrons for folding."],
    demo: |ui, d| {
        let t = Tokens::get();
        state(ui, "Open and closed", |ui| {
            ui.horizontal(|ui| {
                for open in [true, false] {
                    let (rect, _) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), egui::Sense::hover());
                    kit::disclosure(ui.painter(), rect.center(), open, t.form_dim);
                }
            });
        });
        state(ui, "Strip toggle (click it)", |ui| {
            if kit::strip_toggle(ui, d.checked, if d.checked { "Hide Shots strip" } else { "Shots" }).clicked() {
                d.checked = !d.checked;
            }
        });
    },
};

pub const TEXT: Page = Page {
    id: "text",
    title: "Text styles",
    group: Group::Layout,
    items: &["text", "Text", "text_lines", "paint_text", "status_line"],
    what: "Every text a form shows, in the kit's styles: notes, reports, errors, headings, cells and values. One call, with the style saying what the text is.",
    usage: r#"
ui_kit::text(ui, "Counts include the original.", ui_kit::Text::Note("pattern"));
ui_kit::text(ui, "Validation report: 10 mm", ui_kit::Text::Report("check", false));
ui_kit::text(ui, "The sketch is not closed.", ui_kit::Text::Error("sketch"));
"#,
    dos: &[
        "Pick the style for what the text is; its size and colour follow the theme.",
        "Put text through `solidcraft_ui_text` so it can be translated.",
    ],
    donts: &["Don't set font sizes or colours with `RichText` in a form."],
    demo: |ui, _| {
        for (name, style) in [
            ("Heading", kit::Text::Heading("g")),
            ("Note", kit::Text::Note("g")),
            ("Mono note", kit::Text::MonoNote("g")),
            ("Report", kit::Text::Report("g", false)),
            ("Hint", kit::Text::Hint("g")),
            ("Error", kit::Text::Error("g")),
            ("Inline", kit::Text::Inline),
            ("Cell", kit::Text::Cell),
            ("Mono cell", kit::Text::MonoCell),
            ("Value", kit::Text::Value("g", false)),
        ] {
            state(ui, name, |ui| {
                kit::text(ui, "Counts include the original; Flip chooses the side for even totals.", style);
            });
        }
        state(ui, "Lines", |ui| {
            kit::text_lines(ui, "gallery_lines", "solid.fillet radius=2 mm\nsolid.chamfer distance=1 mm");
        });
    },
};

pub const WINDOW: Page = Page {
    id: "window",
    title: "Window",
    group: Group::Layout,
    items: &["Window", "Container", "Surface", "Panel", "DialogButtons", "page_list", "WindowPage"],
    what: "The movable system window every dialog uses: its header with icon and title, a bounded body and the footer. Every outer window, popup and panel goes through `Container`, so they all look and dock the same.",
    usage: r#"
let (closed, action) = ui_kit::Window::new(egui::Id::unique("export"), "export", "Export")
    .actions("Export", "Cancel", None)
    .show(ctx, valid, |ui| { /* rows */ });
"#,
    dos: &["Open every window through `Window` or a `Container` surface.", "Size the body by rows (`page_rows`), not pixels."],
    donts: &["Don't use `egui::Window`, `Area`, `SidePanel` or `CentralPanel` directly: `cargo xtask ui-containers` fails."],
    demo: |ui, d| {
        ui.set_max_width(460.0);
        let (closed, action) = kit::Window::new(ui.scope_id().with("window"), "settings", "Export").actions("Export", "Cancel", None).content(
            ui,
            true,
            |ui| {
                kit::choice_row(ui, "Format", "export_format", &mut d.selected, &["STEP", "STL", "3MF"]);
                kit::checkbox(ui, "Include hidden bodies", &mut d.second);
            },
            |_| {},
        );
        if closed || action != kit::Action::None {
            d.said(format!("closed: {closed}, action: {action:?}"));
        }
    },
};

pub const DETAILS: Page = Page {
    id: "details",
    title: "Details",
    group: Group::Layout,
    items: &["details", "Details", "DetailSetting", "DetailValue", "DetailKey", "DetailFinding", "DetailEvent"],
    what: "An analysis's details card from plain data: its settings, a colour key, findings the user can click to show in the model, a readout and actions. It returns what the user changed or clicked.",
    usage: r#"
let events = ui_kit::details(ui, "draft", &ui_kit::Details {
    settings: vec![ui_kit::DetailSetting { id: "angle".into(), label: "Draft angle".into(),
        value: ui_kit::DetailValue::Number { value: 3.0, min: 0.0, max: 45.0, suffix: "°".into() }, enabled: true }],
    findings_title: "Faces".into(),
    ..Default::default()
}, 400.0, None);
"#,
    dos: &["Describe the card as data and act on the events; keep no state in it."],
    donts: &["Don't build analysis cards by hand."],
    demo: |ui, d| {
        let t = Tokens::get();
        let description = kit::Details {
            caption: Some("Draft analysis".into()),
            settings: vec![kit::DetailSetting {
                id: "angle".into(),
                label: "Draft angle".into(),
                value: kit::DetailValue::Number { value: d.angle.min(45.0), min: 0.0, max: 45.0, suffix: "°".into() },
                enabled: true,
            }],
            key: vec![
                kit::DetailKey {
                    colour: t.agent_ok,
                    pattern: false,
                    pattern_colour: None,
                    label: "Positive".into(),
                    value: "12 faces".into(),
                    numeric: true,
                },
                kit::DetailKey {
                    colour: t.error,
                    pattern: true,
                    pattern_colour: None,
                    label: "Negative".into(),
                    value: "3 faces".into(),
                    numeric: true,
                },
            ],
            findings_title: "Faces under the angle".into(),
            findings_count: Some("2".into()),
            findings: vec![
                kit::DetailFinding { id: "f1".into(), label: "Face 4".into(), value: "1.2°".into(), warning: true, target: true },
                kit::DetailFinding { id: "f2".into(), label: "Face 9".into(), value: "0.4°".into(), warning: true, target: true },
            ],
            readout: vec![("Pull direction".into(), "+Z".into())],
            actions: vec![("export".into(), "Export report".into())],
        };
        ui.set_max_width(420.0);
        for event in kit::details(ui, "gallery_details", &description, 420.0, None) {
            if let kit::DetailEvent::Change(id, kit::DetailValue::Number { value, .. }) = &event
                && id == "angle"
            {
                d.angle = *value;
            }
            d.said(format!("{event:?}"));
        }
    },
};
