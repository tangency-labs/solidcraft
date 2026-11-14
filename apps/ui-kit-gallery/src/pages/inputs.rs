//! Inputs: text, numbers, sliders, checkboxes, choices, colours, search and form rows.

use super::{Group, Page};
use crate::{disabled, state};
use solidcraft_ui_kit as kit;

const COUNT: kit::Count<'static> = kit::Count { min: 1, max: 24, eval: &|text: &str| text.trim().parse().ok() };

pub const TEXT_FIELD: Page = Page {
    id: "text-field",
    title: "Text field",
    group: Group::Inputs,
    items: &["text_field", "text_field_tip", "hint_field", "checked_field", "focused_text_field"],
    what: "A one-line box for a name or other plain text. It draws in the form's control style, selects its text when Tab lands in it, and shows its hint while empty.",
    usage: r#"
// Returns (Enter pressed, changed).
let (entered, changed) = ui_kit::text_field(ui, &mut name, "Name");
// With a hover text explaining the value:
ui_kit::text_field_tip(ui, &mut name, "Name", "Shown in the Browser");
"#,
    dos: &["Put it in a `row` so its label lines up with the form's label column.", "Give a hint that says what goes in it, not \"Enter text\"."],
    donts: &[
        "Don't use it for numbers or expressions: use `number_field`, `length_field` or `expr_row`.",
        "Don't build a raw `egui::TextEdit`: it misses the kit's focus, context menu and colours.",
    ],
    demo: |ui, d| {
        state(ui, "Default", |ui| {
            let (entered, changed) = kit::text_field(ui, &mut d.text, "Name");
            if entered || changed {
                d.said(format!("(entered: {entered}, changed: {changed})"));
            }
        });
        state(ui, "Empty, with its hint", |ui| {
            kit::text_field(ui, &mut d.empty, "Name");
        });
        state(ui, "With a hover text (hover it)", |ui| {
            kit::text_field_tip(ui, &mut d.text, "Name", "Shown in the Browser and the timeline");
        });
        state(ui, "Error", |ui| {
            kit::checked_row(ui, "Name", &mut d.empty, "Required", Some("A name is required"));
        });
        disabled(ui, |ui| {
            kit::text_field(ui, &mut d.text, "Name");
        });
    },
};

pub const TEXT_AREA: Page = Page {
    id: "text-area",
    title: "Text area and password",
    group: Group::Inputs,
    items: &["multiline_text_field", "text_area", "password_field", "read_only_text"],
    what: "Text that runs over several lines (a description, a note), a password that stays hidden, and a read-only code box for text the user copies but does not edit.",
    usage: r#"
ui_kit::multiline_text_field(ui, "Description", ui.scope_id().with("desc"), &mut text);
ui_kit::text_area(ui, "Notes", ui.scope_id().with("notes"), &mut text, 4, 2000); // rows, character limit
ui_kit::password_field(ui, ui.scope_id().with("key"), &mut secret, "API key");
ui_kit::read_only_text(ui, &json);
"#,
    dos: &["Give `text_area` a character limit that suits what is stored.", "Use `read_only_text` for logs, file contents and diagnostics."],
    donts: &["Don't show secrets in a plain text field.", "Don't make a text area the only way to enter structured data."],
    demo: |ui, d| {
        state(ui, "Multi-line", |ui| {
            kit::multiline_text_field(ui, "Description", ui.scope_id().with("multi"), &mut d.long);
        });
        state(ui, "Text area (4 rows, 200 characters)", |ui| {
            kit::text_area(ui, "Notes", ui.scope_id().with("area"), &mut d.long, 4, 200);
        });
        state(ui, "Password", |ui| {
            kit::password_field(ui, ui.scope_id().with("password"), &mut d.password, "API key");
        });
        state(ui, "Read-only", |ui| {
            kit::read_only_text(ui, "{\n  \"op\": \"solid.fillet\",\n  \"radius\": \"2 mm\"\n}");
        });
        disabled(ui, |ui| {
            kit::password_field(ui, ui.scope_id().with("password_off"), &mut d.password, "API key");
        });
    },
};

pub const NUMBER_FIELD: Page = Page {
    id: "number-field",
    title: "Number fields",
    group: Group::Inputs,
    items: &["number_field", "length_field", "angle_field", "whole_number_field", "scalar_field", "vector3_field", "number_fields"],
    what: "Boxes for a number: drag to scrub, type to set. `length_field` and `angle_field` show the design's units; `vector3_field` edits X, Y and Z together.",
    usage: r#"
ui_kit::number_field(ui, &mut value, 0.0..=1.0, 0.01, "");   // range, drag speed, suffix
ui_kit::length_field(ui, &mut mm, "mm", 2);                   // units, decimals
ui_kit::angle_field(ui, &mut degrees);
ui_kit::whole_number_field(ui, &mut count, 1..=100, "");
ui_kit::vector3_field(ui, &mut xyz, -1000.0..=1000.0, "mm");
"#,
    dos: &["Use the field that matches the quantity, so units and rounding are right.", "Clamp with the range, not after the fact."],
    donts: &["Don't parse numbers out of a `text_field`.", "Don't put a unit in the label when the field shows it."],
    demo: |ui, d| {
        state(ui, "Number (0 to 1)", |ui| {
            kit::row(ui, "Opacity", |ui| kit::number_field(ui, &mut d.number, 0.0..=1.0, 0.01, ""));
        });
        state(ui, "Length", |ui| {
            kit::row(ui, "Depth", |ui| kit::length_field(ui, &mut d.length, "mm", 2));
        });
        state(ui, "Angle", |ui| {
            kit::row(ui, "Draft", |ui| kit::angle_field(ui, &mut d.angle));
        });
        state(ui, "Whole number", |ui| {
            kit::row(ui, "Teeth", |ui| kit::whole_number_field(ui, &mut d.whole, 1..=200, ""));
        });
        state(ui, "Scalar", |ui| {
            kit::row(ui, "Scale", |ui| kit::scalar_field(ui, &mut d.number, 0.0..=10.0));
        });
        state(ui, "Vector", |ui| {
            kit::row(ui, "Offset", |ui| kit::vector3_field(ui, &mut d.vector, -1000.0..=1000.0, "mm"));
        });
        disabled(ui, |ui| {
            kit::row(ui, "Depth", |ui| kit::length_field(ui, &mut d.length, "mm", 2));
        });
    },
};

pub const COUNT_FIELD: Page = Page {
    id: "count-field",
    title: "Count field",
    group: Group::Inputs,
    items: &["count_field", "count_row", "Count"],
    what: "A whole count with − and + steppers, such as the copies of a pattern. It takes an expression evaluator, so a parameter name works too.",
    usage: r#"
let limits = ui_kit::Count { min: 1, max: 24, eval: &|text| text.trim().parse().ok() };
ui_kit::count_row(ui, "Copies", &mut text, &limits);
"#,
    dos: &["Set the real minimum and maximum: the steppers stop there."],
    donts: &["Don't use a slider for a count of things."],
    demo: |ui, d| {
        state(ui, "Default", |ui| {
            if kit::count_row(ui, "Copies", &mut d.count, &COUNT) {
                d.said(format!("count = {}", d.count));
            }
        });
        disabled(ui, |ui| {
            kit::count_row(ui, "Copies", &mut d.count, &COUNT);
        });
    },
};

pub const SLIDER: Page = Page {
    id: "slider",
    title: "Slider",
    group: Group::Inputs,
    items: &["slider", "Slider", "mixed_slider"],
    what: "A value picked along a range, with its value at the right. For settings where the feel matters more than the exact number (opacity, blur, quality).",
    usage: r#"
ui_kit::slider(ui, &mut value, 0.0..=1.0, ui_kit::Slider::default());
ui_kit::slider(ui, &mut scale, 0.5..=2.5, ui_kit::Slider {
    step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default()
});
ui_kit::mixed_slider(ui, &mut value, 0.0..=1.0, Some("Mixed")); // several objects differ
"#,
    dos: &["Format the value the way the user thinks of it (percent, mm).", "Use `mixed_slider` when a selection holds different values."],
    donts: &["Don't add a slider to a dialog for a model dimension: dimensions scrub by dragging their label."],
    demo: |ui, d| {
        state(ui, "Default", |ui| {
            kit::row(ui, "Opacity", |ui| kit::slider(ui, &mut d.number, 0.0..=1.0, kit::Slider::default()));
        });
        state(ui, "Percent with a step", |ui| {
            kit::row(ui, "Scale", |ui| {
                kit::slider(
                    ui,
                    &mut d.fraction,
                    0.0..=2.0,
                    kit::Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                )
            });
        });
        state(ui, "Mixed", |ui| {
            kit::row(ui, "Roughness", |ui| kit::mixed_slider(ui, &mut d.number, 0.0..=1.0, Some("Mixed")));
        });
        disabled(ui, |ui| {
            kit::row(ui, "Opacity", |ui| kit::slider(ui, &mut d.number, 0.0..=1.0, kit::Slider::default()));
        });
    },
};

pub const CHECKBOX: Page = Page {
    id: "checkbox",
    title: "Checkbox",
    group: Group::Inputs,
    items: &["checkbox", "checkbox_indeterminate", "checkbox_row", "checkbox_grid", "options"],
    what: "An on/off setting with its label. A mixed box shows a selection whose items differ; a grid lays several out in columns.",
    usage: r#"
ui_kit::checkbox(ui, "Show origin", &mut on);
ui_kit::checkbox_indeterminate(ui, "Visible", &mut on, mixed);
ui_kit::checkbox_grid(ui, &mut [("Faces", &mut faces), ("Edges", &mut edges)], 2);
"#,
    dos: &["Label it with what is on when it is checked.", "Use the mixed state when several selected items differ."],
    donts: &["Don't use a checkbox for a choice between two named options: use `segmented`.", "Don't negate the label (\"Don't show…\")."],
    demo: |ui, d| {
        state(ui, "On and off (hover them)", |ui| {
            if kit::checkbox(ui, "Show origin", &mut d.checked).changed() {
                d.said(format!("Show origin = {}", d.checked));
            }
            kit::checkbox(ui, "Show grid", &mut d.second);
        });
        state(ui, "Mixed", |ui| {
            kit::checkbox_indeterminate(ui, "Visible", &mut d.third, true);
        });
        state(ui, "Grid of two columns", |ui| {
            kit::checkbox_grid(ui, &mut [("Faces", &mut d.checked), ("Edges", &mut d.second), ("Vertices", &mut d.third)], 2);
        });
        disabled(ui, |ui| {
            kit::checkbox(ui, "Show origin", &mut d.checked);
            kit::checkbox(ui, "Show grid", &mut d.second);
        });
    },
};

pub const SEGMENTED: Page = Page {
    id: "segmented",
    title: "Segmented control",
    group: Group::Inputs,
    items: &["segmented", "Segments", "segmented_icons", "IconSegment"],
    what: "Two to five mutually exclusive options side by side, all visible at once. An option can be unavailable and say why.",
    usage: r#"
ui_kit::segmented(ui, &mut selected, &["One side", "Two sides", "Symmetric"], ui_kit::Segments::default());
ui_kit::segmented(ui, &mut selected, &options, ui_kit::Segments {
    enabled: &[true, false, true],
    disabled: &[None, Some("Needs a closed profile"), None],
    ..Default::default()
});
"#,
    dos: &["Keep labels short; use icons only where the icon is well known.", "Give a disabled option its reason."],
    donts: &["Don't use it for more than five options: use a `dropdown`.", "Don't hide unavailable options: fade them and explain."],
    demo: |ui, d| {
        state(ui, "Default", |ui| {
            kit::row(ui, "Direction", |ui| {
                if kit::segmented(ui, &mut d.segment, &["One side", "Two sides", "Symmetric"], kit::Segments::default()) {
                    d.said(format!("selected = {}", d.segment));
                }
            });
        });
        state(ui, "With an unavailable option (hover it)", |ui| {
            kit::row(ui, "Operation", |ui| {
                kit::segmented(
                    ui,
                    &mut d.choice,
                    &["Join", "Cut", "New body"],
                    kit::Segments { enabled: &[true, false, true], disabled: &[None, Some("Nothing to cut yet"), None], ..Default::default() },
                );
            });
        });
        state(ui, "Icons", |ui| {
            kit::row(ui, "Style", |ui| {
                kit::segmented_icons(
                    ui,
                    &mut d.selected,
                    &[
                        kit::IconSegment { key: "shaded", icon: "style_shaded", tip: "Shaded".into() },
                        kit::IconSegment { key: "translucent", icon: "style_translucent", tip: "Translucent".into() },
                        kit::IconSegment { key: "wireframe", icon: "style_wireframe", tip: "Wireframe".into() },
                    ],
                    &[],
                );
            });
        });
        disabled(ui, |ui| {
            kit::row(ui, "Direction", |ui| kit::segmented(ui, &mut d.segment, &["One side", "Two sides"], kit::Segments::default()));
        });
    },
};

pub const DROPDOWN: Page = Page {
    id: "dropdown",
    title: "Dropdown",
    group: Group::Inputs,
    items: &["dropdown", "dropdown_icons", "searchable_dropdown", "SearchOption", "dropdown_menu", "choice_row"],
    what: "One choice from a list that is too long to show at once. The searchable one filters as you type, for lists of materials, fonts or parameters.",
    usage: r#"
ui_kit::dropdown(ui, "units", &mut selected, &["mm", "cm", "in"]);
ui_kit::choice_row(ui, "Units", "units", &mut selected, &["mm", "cm", "in"]); // with its label
let picked: Option<String> = ui_kit::searchable_dropdown(ui, "material", current, &mut filter, &options, None, None);
"#,
    dos: &["Use `choice_row` in a form so the label lines up.", "Switch to `searchable_dropdown` past about fifteen options."],
    donts: &["Don't use a dropdown for two or three options: use `segmented`.", "Don't open an `egui::ComboBox`: it is not the kit's popup."],
    demo: |ui, d| {
        state(ui, "Default (open it)", |ui| {
            if kit::choice_row(ui, "Units", "units", &mut d.selected, &["Millimetres", "Centimetres", "Inches"]).changed() {
                d.said(format!("selected = {}", d.selected));
            }
        });
        state(ui, "With icons", |ui| {
            kit::row(ui, "Style", |ui| {
                kit::dropdown_icons(
                    ui,
                    "style",
                    &mut d.choice,
                    &[("Shaded", "style_shaded"), ("Hidden edges", "style_hidden_edges"), ("Wireframe", "style_wireframe")],
                )
            });
        });
        state(ui, "Searchable", |ui| {
            kit::row(ui, "Material", |ui| {
                let options = [
                    kit::SearchOption { value: "aluminium", label: "Aluminium 6061", handle: None },
                    kit::SearchOption { value: "steel", label: "Steel, mild", handle: None },
                    kit::SearchOption { value: "abs", label: "ABS plastic", handle: None },
                    kit::SearchOption { value: "pla", label: "PLA plastic", handle: None },
                ];
                if let Some(picked) = kit::searchable_dropdown(ui, "material", "Aluminium 6061", &mut d.filter, &options, None, None) {
                    d.said(format!("picked {picked}"));
                }
            });
        });
        disabled(ui, |ui| {
            kit::choice_row(ui, "Units", "units_off", &mut d.selected, &["Millimetres", "Centimetres", "Inches"]);
        });
    },
};

pub const COLOR: Page = Page {
    id: "color",
    title: "Colour field and swatches",
    group: Group::Inputs,
    items: &["color_field", "color_field_with_opacity", "mixed_color_field", "color_swatch", "swatch_grid"],
    what: "A colour box that opens the kit's picker, and swatches for the colours already in the design, which can be dragged onto things.",
    usage: r#"
ui_kit::color_field(ui, &mut rgb);                         // [u8; 3]
ui_kit::swatch_grid(ui, &colours, |ui, c| {
    ui_kit::color_swatch(ui, ui.scope_id().with(c.id), c.colour, c.id == picked, &c.name, c.id);
});
"#,
    dos: &["Show the design's own colours as swatches before the full picker."],
    donts: &["Don't hard-code colours for UI: take them from `Tokens`."],
    demo: |ui, d| {
        state(ui, "Colour field (click it)", |ui| {
            kit::row(ui, "Colour", |ui| kit::color_field(ui, &mut d.rgb));
        });
        state(ui, "Swatches", |ui| {
            let t = solidcraft_theme::Tokens::get();
            let colours = [t.accent, t.axis_x, t.axis_y, t.axis_z, t.warning, t.error];
            let mut clicked = None;
            kit::swatch_grid(ui, &[0usize, 1, 2, 3, 4, 5], |ui, i| {
                if kit::color_swatch(ui, ui.scope_id().with(("swatch", *i)), colours[*i], d.swatch == *i, "Drag onto a face", *i).clicked() {
                    clicked = Some(*i);
                }
            });
            if let Some(i) = clicked {
                d.swatch = i;
                d.said(format!("swatch {i}"));
            }
        });
        disabled(ui, |ui| {
            kit::row(ui, "Colour", |ui| kit::color_field(ui, &mut d.rgb));
        });
    },
};

pub const SEARCH_FIELD: Page = Page {
    id: "search-field",
    title: "Search field",
    group: Group::Inputs,
    items: &["search_field", "menu_filter"],
    what: "A field that filters a list as you type, with a magnifier and a clear button. `menu_filter` is the compact one at the top of a long menu.",
    usage: r#"
ui_kit::search_field(ui, ui.scope_id().with("search"), &mut query, "Search parameters");
ui_kit::menu_filter(ui, ui.scope_id().with("filter"), &mut query, "Filter");
"#,
    dos: &["Filter as the user types; no Search button.", "Say what is searched in the hint."],
    donts: &["Don't clear the query when the list refreshes."],
    demo: |ui, d| {
        state(ui, "Default", |ui| {
            kit::search_field(ui, ui.scope_id().with("search"), &mut d.search, "Search parameters");
        });
        state(ui, "Menu filter", |ui| {
            kit::menu_filter(ui, ui.scope_id().with("filter"), &mut d.search, "Filter");
        });
        disabled(ui, |ui| {
            kit::search_field(ui, ui.scope_id().with("search_off"), &mut d.search, "Search parameters");
        });
    },
};

pub const CHIPS: Page = Page {
    id: "chips",
    title: "Chips and toggles",
    group: Group::Inputs,
    items: &["choice_chip", "add_choice", "toggle_icon", "tool_choices", "ToolChoice"],
    what: "Small selectable chips for a choice among named things (shots, presets), an icon that toggles a mode on and off, and a palette of tools.",
    usage: r#"
if ui_kit::choice_chip(ui, "shot1", "Shot 1", selected).clicked() { /* select */ }
if ui_kit::add_choice(ui, "Add shot").clicked() { /* add */ }
ui_kit::toggle_icon(ui, "Alignment", "Centre", &mut centred, "pattern_center");
"#,
    dos: &["End a chip list with `add_choice` when the user can add more."],
    donts: &["Don't use chips for settings that are just on or off: use a `checkbox`."],
    demo: |ui, d| {
        state(ui, "Choice chips", |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, name) in ["Shot 1", "Shot 2", "Shot 3"].iter().enumerate() {
                    if kit::choice_chip(ui, &format!("shot{i}"), name, d.choice == i).clicked() {
                        d.choice = i;
                        d.said(format!("{name} chosen"));
                    }
                }
                if kit::add_choice(ui, "Add shot").clicked() {
                    d.said("add");
                }
            });
        });
        state(ui, "Toggle icon", |ui| {
            kit::toggle_icon(ui, "Alignment", "Centre", &mut d.checked, "pattern_center");
        });
        state(ui, "Tool choices", |ui| {
            let choices = [
                kit::ToolChoice { key: "line", icon: "line", label: "Line", enabled: true },
                kit::ToolChoice { key: "arc", icon: "arc3", label: "Arc", enabled: true },
                kit::ToolChoice { key: "spline", icon: "spline", label: "Spline", enabled: false },
            ];
            if let Some(key) = kit::tool_choices(ui, "Draw", "line", &choices) {
                d.said(format!("tool {key}"));
            }
        });
        disabled(ui, |ui| {
            kit::choice_chip(ui, "shot_off", "Shot 1", true);
        });
    },
};

pub const ROWS: Page = Page {
    id: "rows",
    title: "Form rows",
    group: Group::Inputs,
    items: &["row", "text_row", "hint_row", "checked_row", "choice_row", "count_row", "value_pair", "settings_form", "caption", "warning_caption"],
    what: "A label in the form's label column and a control beside it. Every dialog is built from rows, so labels line up and the keyboard moves through them in order.",
    usage: r#"
ui_kit::row(ui, "Depth", |ui| ui_kit::length_field(ui, &mut depth, "mm", 2));
ui_kit::text_row(ui, "Name", &mut name, "Body1");
let (changed, response) = ui_kit::checked_row(ui, "Name", &mut name, "Required", warning);
ui_kit::value_pair(ui, "Position", [&mut x, &mut y], ["X", "Y"], None);
ui_kit::caption(ui, "Depth", "Measured from the sketch plane");
"#,
    dos: &["Use a `row` for every labelled control in a dialog.", "Put an explanation under its row with `caption`, aligned to the label."],
    donts: &[
        "Don't lay out labels and fields with `ui.horizontal`.",
        "Don't add rows to an existing dialog without the owner's review and a 1280×720 screenshot.",
    ],
    demo: |ui, d| {
        state(ui, "Rows", |ui| {
            kit::text_row(ui, "Name", &mut d.text, "Body1");
            kit::hint_row(ui, "Tag", &mut d.empty, "Optional");
            kit::row(ui, "Depth", |ui| kit::length_field(ui, &mut d.length, "mm", 2));
            kit::caption(ui, "Depth", "Measured from the sketch plane");
        });
        state(ui, "Error", |ui| {
            kit::checked_row(ui, "Name", &mut d.empty, "Required", Some("A name is required"));
            kit::warning_caption(ui, "Name", "Names must be unique in the design");
        });
        state(ui, "Value pair", |ui| {
            let mut x = format!("{}", d.vector[0]);
            let mut y = format!("{}", d.vector[1]);
            kit::value_pair(ui, "Position", [&mut x, &mut y], ["X", "Y"], None);
        });
        state(ui, "Settings form (a wider label column)", |ui| {
            kit::settings_form(ui, 160.0, |ui| {
                kit::choice_row(ui, "Default units", "rows_units", &mut d.selected, &["Millimetres", "Inches"]);
            });
        });
        disabled(ui, |ui| {
            kit::text_row(ui, "Name", &mut d.text, "Body1");
        });
    },
};
