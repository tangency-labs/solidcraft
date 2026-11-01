//! Sheet metal dialogs: Flange (base from profiles, edge flanges from picked edges, contour from
//! open sketch lines; the picks decide which), Hem, Unfold and Refold, Convert to Sheet Metal,
//! Sheet Metal Rules (the rule manager) and Create Flat Pattern (a flat view of the part with
//! its bend lines, the bend table and DXF export).

use crate::dialogs::put;
use crate::ui_kit::{choice_row, expr_row, hint_row};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Stroke, pos2, vec2};
use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::sheet::SheetRule;
use solidcraft_engine::doc::{FeatureKind, ProfileSel};
use solidcraft_engine::geom::{Vec2, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, edge_sel, face_sel, form_row, profile_indices};
use crate::selection::{AXES, BODIES, CURVES, EDGES, FACES, PLANAR_FACES, PROFILES, SelInput, SelectionExt as _, items, point_value as pt};
use crate::theme::Tokens;

const PERFORATION_PATTERNS: [&str; 2] = ["staggered", "square"];
const PERFORATION_LABELS: [&str; 2] = ["ui.perforate_staggered", "ui.perforate_square"];

const FOLD_POSITIONS: [&str; 4] = ["centerline", "start", "end", "mould"];
const FOLD_POSITION_LABELS: [&str; 4] = ["Centerline", "Start of Bend", "End of Bend", "Outside Mould Line"];
/// The flange types' messages, by the type's index (Base, Edge, Contour).
const FLANGE_TYPES: [&str; 3] = ["sheet.flange_type_base", "sheet.flange_type_edge", "sheet.flange_type_contour"];
const POSITIONS: [&str; 5] = ["inside", "outside", "middle", "material_outside", "tangent"];
const POSITION_LABELS: [&str; 5] = ["Inside", "Outside", "Middle", "Material Outside", "Tangent to Edge"];
/// Where an edge flange's height is measured from (`height_from`; the first is the default).
const HEIGHT_FROM: [&str; 3] = ["outer", "inner", "tangent"];
const HEIGHT_FROM_LABELS: [&str; 3] = ["Outer Sharp", "Inner Sharp", "Tangent"];
/// Rule fields the manager edits (parameter name, label).
pub(crate) const RULE_FIELDS: [(&str, &str); 8] = [
    ("thickness", "Thickness"),
    ("k_factor", "K Factor"),
    ("bend_radius", "Bend Radius"),
    ("relief_width", "Relief Width"),
    ("relief_depth", "Relief Depth"),
    ("corner_relief", "Corner Relief"),
    ("hem_gap", "Hem Gap"),
    ("gap", "Flange Gap"),
];
/// The corner relief shapes the manager offers (the rule's `corner_relief_shape`) and their labels.
pub(crate) const RELIEF_SHAPES: [&str; 4] = ["notch", "round", "rectangular", "tear"];
const RELIEF_SHAPE_LABELS: [&str; 4] = ["Notch", "Round", "Rectangular", "Tear"];
/// How the manager's rule reads bend angles in the DXF bend notes (the rule's `angle_display`)
/// and their labels.
pub(crate) const ANGLE_DISPLAYS: [&str; 2] = ["bend", "included"];
const ANGLE_DISPLAY_LABELS: [&str; 2] = ["Bend", "Included"];
/// The corner relief shapes an edge flange offers: the rule's (index 0, nothing of its own), then
/// the shapes of `RELIEF_SHAPES` in order.
/// Which way a further bend turns, against the bend before it.
const BEND_WAY_LABELS: [&str; 2] = ["Same way", "Other way"];
const FLANGE_RELIEF_LABELS: [&str; 5] = ["Rule", "Notch", "Round", "Rectangular", "Tear"];

crate::dialog_descriptor::dialog_descriptors! {
    #[derive(Clone, Debug)]
    pub enum Sm {
        Perforate("sheet.perforate", "ui.perforate_title", [("Body (last if none)", BODIES | FACES, false)],
            FeatureKind::SheetPerforate { diameter, pitch, margin, pattern, .. }) {
            diameter: String = "3 mm".into() => diameter.to_string(),
            pitch: String = "6 mm".into() => pitch.to_string(),
            margin: String = String::new() => margin.as_deref().map(str::to_string).unwrap_or_default(),
            pattern: usize = 0 => pattern.as_deref().and_then(|p| PERFORATION_PATTERNS.iter().position(|x| *x == p)).unwrap_or(0),
        } => json!({"diameter": diameter, "pitch": pitch, "pattern": PERFORATION_PATTERNS.get(*pattern).copied().unwrap_or("staggered")});
        Hem("sheet.hem", "HEM", [("Edges", EDGES, true)],
            FeatureKind::SheetHem { length, gap, flip, .. }) {
            length: String = "5 mm".into() => length.to_string(),
            gap: String = String::new() => gap.as_deref().map(str::to_string).unwrap_or_default(),
            flip: bool = false => *flip,
        } => json!({"length": length, "flip": flip});
        Fold("sheet.fold", "FOLD", [("Bend Line", AXES, false), ("Stationary Side", PLANAR_FACES, false)],
            FeatureKind::SheetFold { angle, radius, position, flip, .. }) {
            angle: String = "90 deg".into() => angle.to_string(),
            radius: String = String::new() => radius.as_deref().map(str::to_string).unwrap_or_default(),
            position: usize = 0 => FOLD_POSITIONS.iter().position(|x| x == position || (*x == "mould" && position == "mold")).unwrap_or(0),
            flip: bool = false => *flip,
        } => json!({"angle": angle, "position": FOLD_POSITIONS.get(*position).copied().unwrap_or("centerline"), "flip": flip});
        Path("sheet.path_flange", "PATH FLANGE", [("Edge", EDGES, false), ("Path", CURVES, true)],
            FeatureKind::SheetPath { radius, .. }) {
            radius: String = String::new() => radius.as_deref().map(str::to_string).unwrap_or_default(),
        } => json!({});
        ; custom {
            /// Base, edge or contour flange (`ty` follows the picks: profiles, edges, sketch lines).
            Flange {
                ty: usize,
                height: String,
                angle: String,
                radius: String,
                position: usize,
                /// Edge flanges: where the height is measured from (index into `HEIGHT_FROM`).
                height_from: usize,
                flip: bool,
                distance: String,
                reverse: bool,
                rule: usize,
                /// Edge flanges: the edges with an angle and/or a height of their own (the picked point,
                /// angle, height; an empty text means the dialog's), from the rows per edge.
                own: Vec<(Vec3, String, String)>,
                /// Edge flanges: the corner relief shape of their own (index into `FLANGE_RELIEF_LABELS`;
                /// 0 is the rule's) and its size (empty: the rule's).
                relief: usize,
                relief_size: String,
                /// Edge flanges: further bends (angle, height, turns the other way), each a flange on
                /// the far edge of the one before it (the command's `then`).
                then: Vec<(String, String, bool)>,
            },
            Unfold {
                refold: bool,
            },
            Convert {
                rule: usize,
            },
            /// The rule manager: the rule shown (index into the rules, or past the end for a new one).
            Rules {
                pick: usize,
                name: String,
                values: Vec<String>,
                /// The corner relief shape (index into `RELIEF_SHAPES`).
                relief: usize,
                /// How bend angles read (index into `ANGLE_DISPLAYS`).
                angles: usize,
                active: bool,
                loaded: Option<usize>,
            },
            /// The flat pattern of the picked (or last) sheet body.
            FlatPattern {
                data: Option<Result<Value, String>>,
                key: Option<(u64, Option<String>)>,
                status: Option<String>,
            },
        }
    }
}

impl Sm {
    pub fn title(&self) -> &'static str {
        match self {
            Sm::Flange { .. } => "FLANGE",
            Sm::Unfold { refold: false } => "UNFOLD",
            Sm::Unfold { refold: true } => "REFOLD",
            Sm::Convert { .. } => "CONVERT TO SHEET METAL",
            Sm::Rules { .. } => "SHEET METAL RULES",
            Sm::FlatPattern { .. } => "FLAT PATTERN",
            _ => self.simple_metadata().map_or("", |(_, title)| title),
        }
    }

    pub fn previews(&self) -> bool {
        !matches!(self, Sm::FlatPattern { .. })
    }

    /// The value on the canvas (the flange's follows its picks).
    pub fn primary(&mut self, inputs: &[SelInput]) -> Option<(&'static str, ValueKind, &mut String)> {
        if let Sm::Flange { ty, .. } = self {
            *ty = flange_type(inputs);
        }
        Some(match self {
            Sm::Flange { ty: 1, height, .. } => ("Height", ValueKind::Length, height),
            Sm::Flange { ty: 2, distance, .. } => ("Distance", ValueKind::Length, distance),
            Sm::Hem { length, .. } => ("Length", ValueKind::Length, length),
            Sm::Fold { angle, .. } => ("Angle", ValueKind::Angle, angle),
            _ => return None,
        })
    }
}

fn new_flange(app: &SolveApp, ty: usize) -> Sm {
    Sm::Flange {
        ty,
        height: crate::dialogs::length_default(&app.session, "10"),
        angle: "90 deg".into(),
        radius: String::new(),
        position: 0,
        height_from: 0,
        flip: false,
        distance: crate::dialogs::length_default(&app.session, "20"),
        reverse: false,
        rule: 0,
        own: Vec::new(),
        relief: 0,
        relief_size: String::new(),
        then: Vec::new(),
    }
}

/// The dialog for a sheet metal command.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let (sm, inputs) = match id {
        "sheet.flange" => (new_flange(app, 0), vec![SelInput::new("Profile / Edges", PROFILES | EDGES | CURVES, true)]),
        "sheet.unfold" => (Sm::Unfold { refold: false }, vec![SelInput::new("Body (last if none)", BODIES, false)]),
        "sheet.refold" => (Sm::Unfold { refold: true }, vec![SelInput::new("Body (last if none)", BODIES, false)]),
        "sheet.convert" => {
            (Sm::Convert { rule: 0 }, vec![SelInput::new("Fixed Face", PLANAR_FACES, false), SelInput::new("Bend Edges", EDGES, true)])
        }
        "sheet.manage_rules" => {
            let active = app.session.doc.sheet.active.clone();
            let rules = rule_list(app);
            let pick = active.and_then(|a| rules.iter().position(|r| r.name == a)).unwrap_or(0);
            (Sm::Rules { pick, name: String::new(), values: Vec::new(), relief: 0, angles: 0, active: true, loaded: None }, vec![])
        }
        "sheet.flat_pattern" => (Sm::FlatPattern { data: None, key: None, status: None }, vec![SelInput::new("Body (last if none)", BODIES, false)]),
        _ => Sm::simple_start(id)?,
    };
    Some((crate::length_input::defaults(&app.session.doc, Kind::Sheet(sm)), inputs))
}

/// The design's rules (the default steel rule when it has none).
fn rule_list(app: &SolveApp) -> Vec<SheetRule> {
    design_rules(&app.session.doc)
}

/// The rules the manager lists for this design: its own, or the default steel rule when it has
/// none.
pub(crate) fn design_rules(doc: &solidcraft_engine::doc::Document) -> Vec<SheetRule> {
    let r = &doc.sheet.rules;
    if r.is_empty() { vec![SheetRule::steel()] } else { r.clone() }
}

/// "(active rule)" and the rule names, for a rule box.
fn rule_names(app: &SolveApp) -> Vec<String> {
    std::iter::once("(active rule)".to_string()).chain(rule_list(app).into_iter().map(|r| r.name)).collect()
}

fn rule_combo(app: &SolveApp, ui: &mut egui::Ui, id: &str, rule: &mut usize) {
    let names = rule_names(app);
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    choice_row(ui, "Rule", id, rule, &refs);
}

/// The flange type the picks make: edges an edge flange, sketch lines a contour, else base.
fn flange_type(inputs: &[SelInput]) -> usize {
    let items = inputs.first().map(|i| i.items.as_slice()).unwrap_or(&[]);
    if items.iter().any(|x| matches!(x, Sel::Edge { .. })) {
        1
    } else if items.iter().any(|x| matches!(x, Sel::SketchCurve { .. })) {
        2
    } else {
        0
    }
}

/// The dialog's own rows (after the selection inputs); true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Sm, inputs: &mut [SelInput]) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    match k {
        Sm::Flange { ty, height, angle, radius, position, height_from, flip, distance, reverse, rule, own, relief, relief_size, then } => {
            *ty = flange_type(inputs);
            form_row(ui, "Type", |ui| {
                ui.label(FLANGE_TYPES.get(*ty).copied().map(crate::i18n::tr).unwrap_or(""));
            });
            match *ty {
                1 => {
                    enter |= expr_row(ui, "Height", height);
                    enter |= expr_row(ui, "Angle", angle);
                    choice_row(ui, "Bend Position", "sm_pos", position, &POSITION_LABELS);
                    choice_row(ui, "Height From", "sm_height_from", height_from, &HEIGHT_FROM_LABELS);
                    // Further bends: each a flange on the far edge of the one before it.
                    form_row(ui, "Further Bends", |ui| {
                        let add = crate::ui_kit::button(ui, "Add bend", crate::ui_kit::Button::Normal)
                            .on_hover_text(crate::i18n::tr_label("Another bend on the far edge of the last flange"));
                        if add.clicked() && then.len() < 16 {
                            then.push(("45 deg".into(), "20 mm".into(), false));
                        }
                    });
                    let mut drop = None;
                    for (n, (a, h, other)) in then.iter_mut().enumerate() {
                        let bend = n + 2;
                        // The bend's heading: what it makes, and a bin to take it out.
                        form_row(ui, &format!("Bend {bend}"), |ui| {
                            if crate::ui_kit::bin_button(ui, crate::i18n::tr_label("Remove this bend")).clicked() {
                                drop = Some(n);
                            }
                            ui.label(format!("{} · {}", a.trim(), h.trim()));
                        });
                        enter |= expr_row(ui, &format!("Bend {bend} angle"), a);
                        enter |= expr_row(ui, &format!("Bend {bend} height"), h);
                        let mut way = usize::from(*other);
                        choice_row(ui, &format!("Bend {bend} turns"), &format!("sm_then_way_{n}"), &mut way, &BEND_WAY_LABELS);
                        *other = way == 1;
                    }
                    if let Some(n) = drop {
                        then.remove(n);
                    }
                    crate::ui_kit::section(ui, "flange_advanced", "Advanced", false, |ui| {
                        enter |= hint_row(ui, "Bend Radius", radius, crate::i18n::tr_label("from the rule"));
                        // The relief at the corners these flanges make: the rule's unless set here.
                        choice_row(ui, "Corner Relief Shape", "sm_flange_relief", relief, &FLANGE_RELIEF_LABELS);
                        enter |= hint_row(ui, "Corner Relief", relief_size, crate::i18n::tr_label("from the rule"));
                        // With several edges, a row for each: an angle and a height of its own, left
                        // empty for the values above (shown as the hints).
                        let points = edge_point_list(inputs);
                        if points.len() > 1 || own.iter().any(|o| !(o.1.trim().is_empty() && o.2.trim().is_empty())) {
                            form_row(ui, "Per Edge", |ui| {
                                ui.label(RichText::new(crate::i18n::tr_label("angle, height")).weak())
                                    .on_hover_text("An edge's own angle and height. Empty: the values above.");
                            });
                            for (n, p) in points.iter().enumerate() {
                                let slot = own_slot(own, *p);
                                let Some((_, a, h)) = own.get_mut(slot) else { continue };
                                form_row(ui, &format!("Edge {}", n + 1), |ui| {
                                    ui.horizontal(|ui| {
                                        let w = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(0.0);
                                        for (text, hint, what, id) in
                                            [(a, angle.as_str(), "angle", "sm_edge_angle"), (h, height.as_str(), "height", "sm_edge_height")]
                                        {
                                            ui.push_id((id, n), |ui| {
                                                ui.allocate_ui_with_layout(
                                                    vec2(w, Tokens::get().control_height),
                                                    egui::Layout::left_to_right(egui::Align::Center),
                                                    |ui| {
                                                        enter |= crate::ui_kit::hint_field(ui, text, hint);
                                                    },
                                                );
                                                let at = ui.min_rect().center();
                                                crate::scenario::publish_handle(&format!("dialog:Edge {} {what}", n + 1), at);
                                            });
                                        }
                                    });
                                });
                            }
                        }
                    });
                }
                2 => {
                    enter |= expr_row(ui, "Distance", distance);

                    rule_combo(app, ui, "sm_rule", rule);
                }
                _ => rule_combo(app, ui, "sm_rule", rule),
            }
            if *ty == 2 {
                crate::ui_kit::options(ui, "sheet_options", &mut [("Flip", flip), ("Other side", reverse)]);
            } else {
                crate::ui_kit::options(ui, "sheet_options", &mut [("Flip", flip)]);
            }
        }
        Sm::Perforate { diameter, pitch, margin, pattern } => {
            enter |= expr_row(ui, "ui.diameter", diameter);
            enter |= expr_row(ui, "ui.perforate_pitch", pitch);
            enter |= hint_row(ui, "Margin", margin, crate::i18n::tr_label("ui.perforate_margin_default"));
            choice_row(ui, "Pattern", "sm_perforate_pattern", pattern, &PERFORATION_LABELS);
        }
        Sm::Hem { length, gap, flip } => {
            form_row(ui, "Type", |ui| {
                ui.label(crate::i18n::tr_label("Flat"));
            });
            enter |= expr_row(ui, "Length", length);
            enter |= hint_row(ui, "ui.sheet_gap", gap, crate::i18n::tr_label("from the rule"));
            crate::ui_kit::options(ui, "sheet_options", &mut [("Flip", flip)]);
        }
        Sm::Fold { angle, radius, position, flip } => {
            choice_row(ui, "Bend Line Position", "sm_fold_pos", position, &FOLD_POSITION_LABELS);
            enter |= expr_row(ui, "Angle", angle);
            enter |= hint_row(ui, "Bend Radius", radius, crate::i18n::tr_label("from the rule"));
            crate::ui_kit::options(ui, "sheet_options", &mut [("Flip", flip)]);
        }
        Sm::Path { radius } => {
            path_chain(app, inputs);
            // What the path makes: a flange per line, as long as the line (the sketch's lengths).
            let lengths = path_lengths(app, inputs);
            form_row(ui, "Flanges", |ui| {
                ui.label(if lengths.is_empty() {
                    crate::i18n::tr_label("pick the path").to_string()
                } else {
                    let each: Vec<String> = lengths.iter().map(|l| format!("{l:.1}")).collect();
                    format!("{} mm", each.join(" · "))
                });
            });
            crate::ui_kit::caption(ui, "Flanges", crate::i18n::tr_label("set by the sketch's lines"));
            enter |= hint_row(ui, "Bend Radius", radius, crate::i18n::tr_label("from the rule"));
        }
        Sm::Unfold { .. } => {}
        Sm::Convert { rule } => rule_combo(app, ui, "sm_conv_rule", rule),
        Sm::Rules { pick, name, values, relief, angles, active, loaded } => {
            let rules = rule_list(app);
            crate::rule_editor::rule_editor! {
                ui, "sm_rules", rules, pick, name, active, loaded;
                new_name: format!("Rule{}", rules.len() + 1),
                load: {
                    // A new rule starts from the first rule.
                    let base = rules.get(*pick).cloned().unwrap_or_else(|| rules.first().cloned().unwrap_or_else(SheetRule::steel));
                    *values = vec![
                        crate::length_input::stored(&app.session.doc, &base.thickness),
                        base.k_factor.clone(),
                        crate::length_input::stored(&app.session.doc, &base.bend_radius),
                        crate::length_input::stored(&app.session.doc, &base.relief_width),
                        crate::length_input::stored(&app.session.doc, &base.relief_depth),
                        crate::length_input::stored(&app.session.doc, &base.corner_relief),
                        crate::length_input::stored(&app.session.doc, &base.hem_gap),
                        crate::length_input::stored(&app.session.doc, &base.gap),
                    ];
                    *relief = RELIEF_SHAPES.iter().position(|s| s.eq_ignore_ascii_case(base.corner_relief_shape.trim())).unwrap_or(0);
                    *angles = ANGLE_DISPLAYS.iter().position(|d| d.eq_ignore_ascii_case(base.angle_display.trim())).unwrap_or(0);
                    *active = app.session.doc.sheet_rule(None).name == base.name || *pick >= rules.len();
                },
                fields: {
                    for ((key, label), v) in RULE_FIELDS.iter().zip(values.iter_mut()) {
                        if *key == "corner_relief" {
                            // The shape goes with its size, the row below.
                            choice_row(ui, "Corner Relief Shape", "sm_relief_shape", relief, &RELIEF_SHAPE_LABELS);
                        }
                        enter |= expr_row(ui, label, v);
                    }
                    choice_row(ui, "Angle Display", "sm_angle_display", angles, &ANGLE_DISPLAY_LABELS);
                }
            }
            if *pick < rules.len()
                && !app.session.doc.sheet.rules.is_empty()
                && crate::ui_kit::button(ui, crate::i18n::tr_label("Delete rule"), crate::ui_kit::Button::Normal).clicked()
            {
                let n = rules.get(*pick).map(|r| r.name.clone()).unwrap_or_default();
                match app.run("sheet.manage_rules", json!({"name": n, "delete": true})) {
                    Ok(_) => {
                        *pick = 0;
                        *loaded = None;
                    }
                    Err(e) => app.set_status(e, true),
                }
            }
        }
        Sm::FlatPattern { data, key, status } => {
            let body = inputs.first().and_then(|i| i.items.first()).and_then(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None });
            let want = (app.session.revision, body.clone());
            if key.as_ref() != Some(&want) {
                let mut p = json!({});
                if let Some(b) = &body {
                    p["body"] = json!(b);
                }
                *data = Some(app.session.execute("sheet.flat_pattern", &p).map_err(|e| e.to_string()));
                *key = Some(want);
            }
            match data {
                Some(Ok(v)) => {
                    let size = &v["flat_size_mm"];
                    form_row(ui, "Body", |ui| {
                        ui.label(v["body"].as_str().unwrap_or(""));
                    });
                    // One number per row, X and Y as in the DXF: on one row the panel's width cut the
                    // flat size off after its first side, and the second is the one people read most.
                    for (label, i) in [("Flat size X", 0), ("Flat size Y", 1), ("Thickness", 2)] {
                        form_row(ui, label, |ui| {
                            ui.label(crate::prefs::show_mm(app, size[i].as_f64().unwrap_or(0.0), 1));
                        });
                    }
                    let bends = v["bends"].as_array().cloned().unwrap_or_default();
                    form_row(ui, "Bends", |ui| {
                        ui.label(format!("{}", bends.len()));
                    });
                    for b in bends.iter().take(50) {
                        let up = b["direction"] == "up";
                        let (up_col, down_col) = bend_colors();
                        form_row(ui, &format!("  Bend {}", b["bend"].as_u64().unwrap_or(0)), |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} {:.1}°  R{}",
                                    if up { "Up" } else { "Down" },
                                    b["angle_deg"].as_f64().unwrap_or(0.0),
                                    crate::prefs::show_mm(app, b["radius"].as_f64().unwrap_or(0.0), 1)
                                ))
                                .color(if up { up_col } else { down_col }),
                            );
                        });
                    }
                    ui.vertical(|ui| {
                        if crate::ui_kit::button(ui, crate::i18n::tr_label("Export DXF…"), crate::ui_kit::Button::Normal).clicked() {
                            *status = Some(export_flat(app, v["body"].as_str().unwrap_or(""), "dxf"));
                        }
                    });
                    ui.vertical(|ui| {
                        if crate::ui_kit::button(ui, crate::i18n::tr_label("Export Bend Table…"), crate::ui_kit::Button::Normal).clicked() {
                            *status = Some(export_flat(app, v["body"].as_str().unwrap_or(""), "csv"));
                        }
                    });
                    if let Some(s) = status {
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(RichText::new(s.as_str()).color(t.text_dim)).wrap());
                        });
                    }
                }
                Some(Err(e)) => {
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(RichText::new(e.as_str()).color(t.error)).wrap());
                    });
                }
                None => {}
            }
        }
    }
    enter
}

/// The ribbon exports use the same save picker as the flat-pattern panel. Keep the
/// commands' default target (the last sheet body) and leave programmatic calls headless.
pub fn start_export(app: &mut SolveApp, id: &str) -> bool {
    let extension = match id {
        "sheet.export_dxf" => "dxf",
        "sheet.export_bend_table" => "csv",
        _ => return false,
    };
    if let Some(body) = app.session.model.state().sheets.last().map(|sheet| sheet.body.clone()) {
        export_flat(app, &body, extension);
    } else {
        // Let the command report its ordinary "no sheet body" error without opening a picker;
        // nothing starts, so the ribbon goes back to the tab it was started from (#10540).
        if app.run(id, json!({})).is_err() {
            app.restore_command_tab();
        }
    }
    true
}

fn export_flat(app: &mut SolveApp, body: &str, extension: &str) -> String {
    let name = format!("{}-{body}.{extension}", app.session.doc.name);
    let body = body.to_string();
    let command = if extension == "csv" { "sheet.export_bend_table" } else { "sheet.export_dxf" };
    app.pick_save(&name, &[extension], move |app, path| {
        let message = if let Some(path) = path {
            match app.run(command, json!({"body": body, "path": path})) {
                Ok(_) => {
                    app.set_status(format!("Exported {path}"), false);
                    format!("Saved {path}")
                }
                Err(e) => e,
            }
        } else {
            "Export cancelled".into()
        };
        if let Some(d) = app.dialog.as_mut()
            && let Kind::Sheet(Sm::FlatPattern { status, .. }) = &mut d.kind
        {
            *status = Some(message);
        }
    });
    "Choose an export file…".into()
}

// ---- commands ----

fn rule_value(app: &SolveApp, rule: usize) -> Option<String> {
    rule.checked_sub(1).and_then(|i| rule_list(app).get(i).map(|r| r.name.clone()))
}

/// A path flange's path is a chain: a line picked into Path brings every line joined to it end
/// to end, so one click takes the whole path.
fn path_chain(app: &SolveApp, inputs: &mut [SelInput]) {
    let edge = inputs.first().and_then(|i| i.items.edge_points().first().copied());
    let Some(input) = inputs.get_mut(1) else { return };
    let st = app.session.world_state();
    // The path is in the sketch its picks name (#3726).
    let Some(ss) = crate::dialogs::picks_sketch(app, &input.items, None).and_then(|k| st.sketch(k)) else { return };
    let mut ids: Vec<String> = Vec::new();
    for id in input.items.curve_ids() {
        let chain = match ss.sketch.curve_index(&id) {
            Some(ci) => {
                ss.sketch.chain(ci).into_iter().filter_map(|c| ss.sketch.curves.get(c)).filter(|c| !c.construction).map(|c| c.id.clone()).collect()
            }
            None => vec![id],
        };
        for id in chain {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    // In order along the path, so the flanges read from the edge out.
    if let Ok(mut order) = ss.chain_order(&ids) {
        // From the end nearer the picked edge.
        let near = |id: Option<&String>| {
            let segs = id.and_then(|id| ss.sketch.curve_index(id)).map(|ci| ss.sketch.segs(ci)).unwrap_or_default();
            let ends = [segs.first().map(|s| s.start()), segs.last().map(|s| s.end())];
            ends.into_iter().flatten().map(|p| edge.map_or(0.0, |e| ss.plane.to_world(p).dist(e))).fold(f64::INFINITY, f64::min)
        };
        if near(order.last()) < near(order.first()) {
            order.reverse();
        }
        ids = order;
    }
    let chained: Vec<Sel> = ids.iter().map(|id| crate::selection::curve_pick(id, ss.feature, app.session.active_sketch)).collect();
    if chained != input.items {
        input.items = chained;
    }
}

/// The lengths of the path's lines, in the order picked (mm).
fn path_lengths(app: &SolveApp, inputs: &[SelInput]) -> Vec<f64> {
    let st = app.session.world_state();
    let picks = items(inputs, 1);
    let Some(ss) = crate::dialogs::picks_sketch(app, picks, None).and_then(|k| st.sketch(k)) else { return Vec::new() };
    picks
        .curve_ids()
        .iter()
        .filter_map(|id| {
            let ci = ss.sketch.curve_index(id)?;
            Some(ss.sketch.segs(ci).iter().map(|s| s.start().dist(s.end())).sum())
        })
        .collect()
}

fn edge_points(inputs: &[SelInput]) -> Vec<Value> {
    items(inputs, 0).edge_points().into_iter().map(pt).collect()
}

/// The picked edges' points.
fn edge_point_list(inputs: &[SelInput]) -> Vec<Vec3> {
    items(inputs, 0).edge_points()
}

/// Where the own angle and height of the edge picked at `p` are kept (an empty entry on first use).
fn own_slot(own: &mut Vec<(Vec3, String, String)>, p: Vec3) -> usize {
    own.iter().position(|o| o.0.dist(p) < 1e-9).unwrap_or_else(|| {
        own.push((p, String::new(), String::new()));
        own.len() - 1
    })
}

/// The edges for `sheet.flange`: a plain point, or `{point, angle?, height?}` for an edge with
/// an angle and/or a height of its own.
fn flange_edges(inputs: &[SelInput], own: &[(Vec3, String, String)]) -> Vec<Value> {
    edge_point_list(inputs)
        .into_iter()
        .map(|p| {
            let (a, h) = own.iter().find(|o| o.0.dist(p) < 1e-9).map(|o| (o.1.trim(), o.2.trim())).unwrap_or(("", ""));
            if a.is_empty() && h.is_empty() {
                return pt(p);
            }
            let mut e = json!({"point": pt(p)});
            for (k, v) in [("angle", a), ("height", h)] {
                if !v.is_empty() {
                    e[k] = json!(v);
                }
            }
            e
        })
        .collect()
}

/// The commands the dialog's OK runs (`extra`: parameters kept from the feature being edited).
pub fn commands(app: &SolveApp, k: &Sm, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let s = &app.session;
    let (cmd, mut p): (&str, Value) = match k {
        Sm::Flange { height, angle, radius, position, height_from, flip, distance, reverse, rule, own, relief, relief_size, then, .. } => {
            let mut p = match flange_type(inputs) {
                1 => {
                    let mut p = json!({"type": "edge", "edges": flange_edges(inputs, own), "height": height, "angle": angle,
                        "position": POSITIONS.get(*position).copied().unwrap_or("inside")});
                    put(&mut p, &[("radius", radius)]);
                    if let Some(from) = HEIGHT_FROM.get(*height_from).filter(|_| *height_from > 0) {
                        p["height_from"] = json!(from);
                    }
                    if let Some(shape) = relief.checked_sub(1).and_then(|i| RELIEF_SHAPES.get(i)) {
                        p["corner_relief_shape"] = json!(shape);
                    }
                    put(&mut p, &[("corner_relief", relief_size)]);
                    if !then.is_empty() {
                        let mut bends = Vec::with_capacity(then.len());
                        for (n, (a, h, other)) in then.iter().enumerate() {
                            if h.trim().is_empty() {
                                return Err(format!("bend {} needs a height", n + 2));
                            }
                            let mut b = json!({"height": h});
                            put(&mut b, &[("angle", a)]);
                            // The bend's own side: the flange's, or the other one.
                            if *other {
                                b["flip"] = json!(!*flip);
                            }
                            bends.push(b);
                        }
                        p["then"] = json!(bends);
                    }
                    p
                }
                2 => {
                    let ids: Vec<String> = items(inputs, 0).curve_ids();
                    let st = s.world_state();
                    let sketch = items(inputs, 0)
                        .named_sketch()
                        .or_else(|| st.sketches.iter().rev().find(|ss| ids.iter().all(|id| ss.sketch.curve_index(id).is_some())).map(|ss| ss.feature))
                        .ok_or("pick lines of one sketch")?;
                    json!({"type": "contour", "sketch": sketch, "curves": ids, "distance": distance, "reverse": reverse})
                }
                _ => {
                    let ps: Vec<(u64, usize)> = items(inputs, 0).profiles();
                    let sketch = ps.first().map(|x| x.0).ok_or("select a profile, edges or sketch lines first")?;
                    let idx: Vec<usize> = ps.iter().filter(|x| x.0 == sketch).map(|x| x.1).collect();
                    json!({"type": "base", "sketch": sketch, "profiles": idx})
                }
            };
            p["flip"] = json!(flip);
            if let Some(r) = rule_value(app, *rule).filter(|_| p["type"] != "edge") {
                p["rule"] = json!(r);
            }
            ("sheet.flange", p)
        }
        Sm::Hem { gap, .. } => {
            let edges = edge_points(inputs);
            if edges.is_empty() {
                return Err("select sheet edges first".into());
            }
            let mut p = json!({"edges": edges});
            put(&mut p, &[("gap", gap)]);
            ("sheet.hem", p)
        }
        Sm::Fold { radius, .. } => {
            let mut p = json!({});
            match items(inputs, 0).first() {
                Some(Sel::SketchCurve { id, sketch: named }) => {
                    let st = s.world_state();
                    let sketch = named
                        .or_else(|| st.sketches.iter().rev().find(|ss| ss.sketch.curve_index(id).is_some()).map(|ss| ss.feature))
                        .ok_or("the line's sketch is gone")?;
                    p["sketch"] = json!(sketch);
                    p["curve"] = json!(id);
                }
                // An edited fold made from two points keeps them (in `extra`).
                _ if extra.contains_key("points") => {}
                _ => return Err("select the bend line (a sketch line) first".into()),
            }
            put(&mut p, &[("radius", radius)]);
            if let Some(Sel::Face { point, .. }) = items(inputs, 1).first() {
                p["fixed"] = pt(*point);
            }
            ("sheet.fold", p)
        }
        Sm::Path { radius } => {
            let edge = edge_points(inputs).into_iter().next().ok_or("select the sheet edge first")?;
            let picks = items(inputs, 1);
            let curves = picks.curve_ids();
            if curves.is_empty() {
                return Err("select the path (sketch lines across the edge)".into());
            }
            let sketch = crate::dialogs::picks_sketch(app, picks, None).ok_or("the path's sketch is gone")?;
            let mut p = json!({"edge": edge, "sketch": sketch, "curves": curves});
            put(&mut p, &[("radius", radius)]);
            ("sheet.path_flange", p)
        }
        Sm::Perforate { margin, .. } => {
            let mut p = json!({});
            put(&mut p, &[("margin", margin)]);
            match items(inputs, 0).first() {
                Some(Sel::Body { name }) => p["body"] = json!(name),
                Some(Sel::Face { body, .. }) => p["body"] = json!(body),
                _ => {}
            }
            ("sheet.perforate", p)
        }
        Sm::Unfold { refold } => {
            let mut p = json!({});
            if let Some(Sel::Body { name }) = items(inputs, 0).first() {
                p["body"] = json!(name);
            }
            (if *refold { "sheet.refold" } else { "sheet.unfold" }, p)
        }
        Sm::Convert { rule } => {
            let Some(Sel::Face { body, point, .. }) = items(inputs, 0).first() else {
                return Err("select the fixed face first (a plate's large face)".into());
            };
            let mut p = json!({"body": body, "face": pt(*point)});
            // The sharp edges of a solid or shelled body that become bends (a plate has none).
            let bends: Vec<Value> = items(inputs, 1).edge_points().into_iter().map(pt).collect();
            if !bends.is_empty() {
                p["bends"] = json!(bends);
            }
            if let Some(r) = rule_value(app, *rule) {
                p["rule"] = json!(r);
            }
            ("sheet.convert", p)
        }
        Sm::Rules { pick, name, values, relief, angles, active, .. } => {
            let rules = rule_list(app);
            let fields = RULE_FIELDS.iter().map(|(key, _)| *key).zip(values);
            let mut p = crate::rule_editor::params(rules.get(*pick).map(|r| r.name.as_str()), name, *active, fields)?;
            p["corner_relief_shape"] = json!(RELIEF_SHAPES.get(*relief).copied().unwrap_or("notch"));
            p["angle_display"] = json!(ANGLE_DISPLAYS.get(*angles).copied().unwrap_or("bend"));
            ("sheet.manage_rules", p)
        }
        Sm::FlatPattern { .. } => return Ok(Vec::new()),
    };
    k.fill_simple_params(&mut p);
    if let Value::Object(m) = &mut p {
        for (k, v) in extra {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(cmd.to_string(), p)])
}

/// The dialog editing a sheet metal feature, filled from it.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let s = &app.session;
    let st = s.world_state();
    let rule_index = |r: &Option<String>| r.as_ref().and_then(|n| rule_list(app).iter().position(|x| &x.name == n)).map_or(0, |i| i + 1);
    let mut extra = Map::new();
    let (k, items): (Sm, Vec<Sel>) = match kind {
        FeatureKind::SheetBase { sketch, profiles, rule, flip } => {
            let ps = st.sketch(sketch.0).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            let mut f = new_flange(app, 0);
            if let Sm::Flange { flip: fl, rule: r, .. } = &mut f {
                *fl = *flip;
                *r = rule_index(rule);
            }
            (f, ps.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect())
        }
        FeatureKind::SheetContour { sketch, curves, distance, rule, flip, reverse, .. } => {
            let mut f = new_flange(app, 2);
            if let Sm::Flange { flip: fl, rule: r, distance: d, reverse: rv, .. } = &mut f {
                *fl = *flip;
                *r = rule_index(rule);
                *d = distance.to_string();
                *rv = *reverse;
            }
            (f, curves.iter().map(|id| crate::selection::curve_pick(id, sketch.0, app.session.active_sketch)).collect())
        }
        FeatureKind::SheetFlange { edges, height, angle, radius, position, flip, body, corner_relief_shape, corner_relief, height_from } => {
            if let Some(b) = body {
                extra.insert("body".into(), json!(b));
            }
            // The edges, and the angle and height some have of their own (kept by the point the
            // edge's selection carries).
            let (mut picked, mut own) = (Vec::new(), Vec::new());
            for e in edges {
                let Some(sel) = edge_sel(s, e.point.0) else { continue };
                if let (Sel::Edge { point, .. }, true) = (&sel, e.angle.is_some() || e.height.is_some()) {
                    own.push((
                        *point,
                        e.angle.as_deref().map(str::to_string).unwrap_or_default(),
                        e.height.as_deref().map(str::to_string).unwrap_or_default(),
                    ));
                }
                picked.push(sel);
            }
            let f = Sm::Flange {
                ty: 1,
                height: height.to_string(),
                angle: angle.to_string(),
                radius: radius.as_deref().map(str::to_string).unwrap_or_default(),
                position: POSITIONS.iter().position(|p| p == position).unwrap_or(0),
                height_from: height_from
                    .as_deref()
                    .and_then(|name| HEIGHT_FROM.iter().position(|h| h.eq_ignore_ascii_case(name.trim())))
                    .unwrap_or(0),
                flip: *flip,
                distance: crate::dialogs::length_default(&app.session, "20"),
                reverse: false,
                rule: 0,
                own,
                relief: corner_relief_shape
                    .as_deref()
                    .and_then(|name| RELIEF_SHAPES.iter().position(|s| s.eq_ignore_ascii_case(name.trim())))
                    .map_or(0, |i| i + 1),
                relief_size: corner_relief.as_deref().map(str::to_string).unwrap_or_default(),
                then: Vec::new(),
            };
            (f, picked)
        }
        FeatureKind::SheetHem { edges, body, .. } => {
            if let Some(b) = body {
                extra.insert("body".into(), json!(b));
            }
            (Sm::simple_restore(kind)?, edges.iter().filter_map(|p| edge_sel(s, p.0)).collect())
        }
        FeatureKind::SheetFold { sketch, curve, a, b, fixed, body, .. } => {
            if let Some(bd) = body {
                extra.insert("body".into(), json!(bd));
            }
            let k = Sm::simple_restore(kind)?;
            let (_, mut ins) = start(app, "sheet.fold")?;
            match curve {
                Some(c) => {
                    if let Some(i) = ins.first_mut() {
                        i.items = vec![match sketch {
                            Some(k) => crate::selection::curve_pick(c, k.0, app.session.active_sketch),
                            None => Sel::SketchCurve { id: c.clone(), sketch: None },
                        }];
                    }
                }
                // A fold given by two points keeps them.
                None => {
                    extra.insert("points".into(), json!([pt(a.0), pt(b.0)]));
                }
            }
            if let Some(i) = ins.get_mut(1) {
                i.items = fixed.and_then(|f| face_sel(s, f.0)).into_iter().collect();
            }
            return Some((Kind::Sheet(k), ins, extra));
        }
        FeatureKind::SheetPath { edge, sketch, curves, body, .. } => {
            if let Some(bd) = body {
                extra.insert("body".into(), json!(bd));
            }
            let k = Sm::simple_restore(kind)?;
            let (_, mut ins) = start(app, "sheet.path_flange")?;
            if let Some(i) = ins.first_mut() {
                i.items = edge_sel(s, edge.0).into_iter().collect();
            }
            if let Some(i) = ins.get_mut(1) {
                i.items = curves.iter().map(|id| crate::selection::curve_pick(id, sketch.0, app.session.active_sketch)).collect();
            }
            return Some((Kind::Sheet(k), ins, extra));
        }
        FeatureKind::SheetPerforate { body, .. } => (Sm::simple_restore(kind)?, body.iter().map(|n| Sel::Body { name: n.to_string() }).collect()),
        FeatureKind::SheetUnfold { body, refold } => {
            (Sm::Unfold { refold: *refold }, body.iter().map(|n| Sel::Body { name: n.to_string() }).collect())
        }
        FeatureKind::SheetConvert { face, rule, bends, .. } => {
            let (_, mut ins) = start(app, "sheet.convert")?;
            if let Some(i) = ins.first_mut() {
                i.items = face_sel(s, face.0).into_iter().collect();
            }
            if let Some(i) = ins.get_mut(1) {
                i.items = bends.iter().filter_map(|p| edge_sel(s, p.0)).collect();
            }
            return Some((Kind::Sheet(Sm::Convert { rule: rule_index(rule) }), ins, extra));
        }
        _ => return None,
    };
    let id = match &k {
        Sm::Flange { .. } => "sheet.flange",
        Sm::Hem { .. } => "sheet.hem",
        Sm::Perforate { .. } => "sheet.perforate",
        Sm::Unfold { refold: false } => "sheet.unfold",
        Sm::Unfold { .. } => "sheet.refold",
        _ => "sheet.convert",
    };
    let (_, mut inputs) = start(app, id)?;
    if let Some(i) = inputs.first_mut() {
        i.items = items;
    }
    // Profiles stay a profile selection (`All` keeps them all).
    if let (FeatureKind::SheetBase { profiles: ProfileSel::All, .. }, Some(i)) = (kind, inputs.first())
        && i.items.is_empty()
    {
        extra.insert("profiles".into(), json!("all"));
    }
    Some((Kind::Sheet(k), inputs, extra))
}

// ---- view ----

/// The direction an edge flange grows from its first edge: out of the sheet's larger
/// neighbouring face (flipped with Flip). A hem's handle runs along the hem instead
/// ([`hem_handle`]).
pub fn arrow(app: &SolveApp, d: &Dialog) -> Option<Vec3> {
    let Kind::Sheet(k) = &d.kind else { return None };
    let flip = match k {
        Sm::Flange { ty: 1, flip, .. } => *flip,
        Sm::Hem { .. } => return hem_handle(app, d).map(|(_, dir)| dir),
        _ => return None,
    };
    let (n, _, _) = edge_frame(app, d)?;
    Some(if flip { -n } else { n })
}

/// The sheet at the dialog's first picked edge: the unit normal of the larger neighbouring face
/// (out of the sheet), the unit normal of the other neighbour (the edge's end face, pointing away
/// from the sheet), and the sheet's thickness along the first.
fn edge_frame(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3, f64)> {
    let Sel::Edge { body, index, point } = d.inputs.first()?.items.first()? else { return None };
    let st = app.session.world_state();
    let m = st.body(body)?.mesh();
    let faces = m.edge_faces.get(*index)?;
    let area = |f: u32| -> f64 {
        m.triangles
            .iter()
            .zip(&m.tri_face)
            .filter(|(_, tf)| **tf == f)
            .filter_map(|(t, _)| m.tri(t))
            .map(|[a, b, c]| (b - a).cross(c - a).len() * 0.5)
            .sum()
    };
    let normal = |f: u32| -> Option<Vec3> {
        m.triangles
            .iter()
            .zip(&m.tri_face)
            .filter(|(_, tf)| **tf == f)
            .filter_map(|(t, _)| m.tri(t))
            .min_by(|a, b| ((a[0] + a[1] + a[2]) / 3.0).dist(*point).total_cmp(&((b[0] + b[1] + b[2]) / 3.0).dist(*point)))
            .and_then(|[a, b, c]| (b - a).cross(c - a).normalized())
    };
    let f = faces.iter().copied().max_by(|a, b| area(*a).total_cmp(&area(*b)))?;
    let g = faces.iter().copied().find(|g| *g != f)?;
    let (n, end) = (normal(f)?, normal(g)?);
    // The end face spans the sheet's thickness along the normal.
    let heights = m.triangles.iter().zip(&m.tri_face).filter(|(_, tf)| **tf == g).filter_map(|(t, _)| m.tri(t)).flatten().map(|v| v.dot(n));
    let (lo, hi) = heights.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| (lo.min(h), hi.max(h)));
    let thickness = hi - lo;
    (thickness.is_finite() && thickness > 0.0).then_some((n, end, thickness))
}

/// Where a hem's handle starts and where it points (#5828). Its Length is measured from the outer
/// face of the fold to the hem's tip, so the handle starts on the centre line of the hem layer at
/// that face and runs along the hem toward the tip: square to the picked edge, back over the sheet.
/// The layer lies on the far side of the sheet from the larger face unless flipped. The Gap is
/// taken from the dialog when it holds a number; otherwise the layer sits flat against the sheet.
pub fn hem_handle(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    let Kind::Sheet(Sm::Hem { gap, flip, .. }) = &d.kind else { return None };
    let Sel::Edge { point, .. } = d.inputs.first()?.items.first()? else { return None };
    let (n, end, thickness) = edge_frame(app, d)?;
    let gap = app.session.doc.eval(gap, ValueKind::Length).ok().filter(|g| g.is_finite() && *g >= 0.0).unwrap_or(0.0);
    // The hem's side of the sheet, as a unit vector along the larger face's normal, and the
    // distance of the layer's centre line from the face the edge lies on.
    let (side, depth) = if *flip { (n, gap + thickness / 2.0) } else { (-n, thickness + gap + thickness / 2.0) };
    let start = *point + end * (thickness + gap) + side * depth;
    Some((start, -end))
}

/// Bend line colours: up and down (theme tokens, legible on either theme and in every colour
/// vision mode).
fn bend_colors() -> (Color32, Color32) {
    let t = crate::theme::Tokens::get();
    (t.bend_up, t.bend_down)
}

/// The flat pattern view: outline, cut-outs, perforations and bend lines (up and down in
/// their colours), fitted over the viewport. True when the dialog is a flat pattern.
pub fn overlay(app: &mut SolveApp, painter: &egui::Painter, rect: Rect, d: &Dialog) -> bool {
    let Kind::Sheet(Sm::FlatPattern { data, .. }) = &d.kind else { return false };
    let t = Tokens::get();
    let (up_col, down_col) = bend_colors();
    painter.rect_filled(rect, 0.0, t.panel);
    let Some(Ok(v)) = data else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "no sheet metal body",
            FontId::proportional(crate::theme::Tokens::get().font_heading),
            t.text_dim,
        );
        return true;
    };
    let read = |p: &Value| -> Option<(f64, f64)> { Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?)) };
    let loops: Vec<Vec<(f64, f64)>> =
        v["outline"].as_array().into_iter().flatten().map(|l| l.as_array().into_iter().flatten().filter_map(read).collect()).collect();
    let holes: Vec<Vec<(f64, f64)>> =
        v["cutouts"].as_array().into_iter().flatten().map(|l| l.as_array().into_iter().flatten().filter_map(read).collect()).collect();
    let mut lo = (f64::INFINITY, f64::INFINITY);
    let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in loops.iter().flatten() {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    if !(lo.0.is_finite() && hi.0 > lo.0 && hi.1 > lo.1) {
        return true;
    }
    // Fit all retained islands to the exposed viewport. The command panel can grow
    // to its width limit, so its actual edge, rather than a fixed reserve, bounds the view.
    let panel_left = painter
        .ctx()
        .memory(|m| m.area_rect(egui::Id::new(("sc_dialog", "FLAT PATTERN"))))
        .map(|panel| panel.left())
        .unwrap_or(rect.right() - crate::frame::DIALOG_MAX_W);
    let content = crate::shell::content_rect(app, rect);
    let area = Rect::from_min_max(content.min + vec2(40.0, 60.0), pos2(panel_left.min(content.right()) - 20.0, content.bottom() - 60.0));
    if area.width() <= 0.0 || area.height() <= 0.0 {
        return true;
    }
    if !app.flat_view.fitted {
        app.flat_view.fit(rect, area, lo, hi);
    }
    let cam = app.flat_view.cam;
    let scale = f64::from(rect.height()) / (2.0 * cam.half_height());
    let ox = f64::from(rect.center().x) - cam.target.x * scale;
    let oy = f64::from(rect.center().y) + cam.target.y * scale;
    let to = |p: (f64, f64)| -> Pos2 { pos2((ox + p.0 * scale) as f32, (oy - p.1 * scale) as f32) };
    for l in &loops {
        let pts: Vec<Pos2> = l.iter().map(|p| to(*p)).collect();
        if pts.len() >= 3 {
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.8, t.text)));
        }
    }
    for h in &holes {
        let pts: Vec<Pos2> = h.iter().map(|p| to(*p)).collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.4, t.text)));
        }
    }
    if let Some(body) = v["body"].as_str() {
        app.flat_view.cache_perforations(&app.session, body);
        let clip = rect.intersect(painter.clip_rect());
        let pixels_per_point = painter.ctx().pixels_per_point();
        for &(centre, radius) in &app.flat_view.perforations {
            let radius = (radius * scale) as f32;
            // Egui uses logical points; reject holes smaller than one physical pixel.
            if !radius.is_finite() || radius * 2.0 * pixels_per_point < 1.0 {
                continue;
            }
            let centre = to((centre.x, centre.y));
            if centre.x.is_finite() && centre.y.is_finite() && clip.intersects(Rect::from_center_size(centre, vec2(radius * 2.0, radius * 2.0))) {
                painter.circle_stroke(centre, radius, Stroke::new(1.4, t.text));
            }
        }
    }
    // A pocket retains material below (or above) the cut. Dashed contours and a depth
    // annotation distinguish it from the solid through-cut boundaries.
    let pockets: Vec<&Value> = v["pockets"].as_array().into_iter().flatten().collect();
    let mut pocket_annotations: Vec<(Pos2, Vec<String>)> = Vec::new();
    for pocket in &pockets {
        let outer: Vec<(f64, f64)> = pocket["outline"].as_array().into_iter().flatten().filter_map(read).collect();
        for contour in std::iter::once(&pocket["outline"]).chain(pocket["cutouts"].as_array().into_iter().flatten()) {
            let mut pts: Vec<Pos2> = contour.as_array().into_iter().flatten().filter_map(read).map(to).collect();
            if pts.len() >= 3 {
                pts.push(pts[0]);
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.6, t.accent), 3.0, 3.0));
            }
        }
        if !outer.is_empty() {
            let bounds = outer.iter().map(|p| to(*p)).fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(p, p)));
            let anchor = bounds.center_top() - vec2(0.0, 6.0);
            if let Some(label) = pocket_label(app, pocket) {
                if let Some((_, labels)) = pocket_annotations.iter_mut().find(|(at, _)| at.distance(anchor) < 1.0) {
                    if !labels.contains(&label) {
                        labels.push(label);
                    }
                } else {
                    pocket_annotations.push((anchor, vec![label]));
                }
            }
        }
    }
    for (anchor, labels) in pocket_annotations {
        painter.text(anchor, Align2::CENTER_BOTTOM, labels.join("\n"), FontId::proportional(crate::theme::Tokens::get().font_body), t.accent);
    }
    for b in v["bends"].as_array().into_iter().flatten() {
        let (Some(a), Some(e)) = (read(&b["start"]), read(&b["end"])) else { continue };
        let col = if b["direction"] == "up" { up_col } else { down_col };
        painter.extend(egui::Shape::dashed_line(&[to(a), to(e)], Stroke::new(1.6, col), 8.0, 5.0));
    }
    // Legend.
    let mut y = rect.top() + 14.0;
    painter.text(
        pos2(rect.left() + 14.0, y),
        Align2::LEFT_TOP,
        "FLAT PATTERN",
        FontId::proportional(crate::theme::Tokens::get().font_button),
        t.text,
    );
    for (label, col) in [("Bend up", up_col), ("Bend down", down_col)] {
        y += 20.0;
        painter.line_segment([pos2(rect.left() + 14.0, y + 7.0), pos2(rect.left() + 38.0, y + 7.0)], Stroke::new(2.0, col));
        painter.text(pos2(rect.left() + 44.0, y), Align2::LEFT_TOP, label, FontId::proportional(crate::theme::Tokens::get().font_body), t.text_dim);
    }
    if !pockets.is_empty() {
        y += 20.0;
        painter.extend(egui::Shape::dashed_line(
            &[pos2(rect.left() + 14.0, y + 7.0), pos2(rect.left() + 38.0, y + 7.0)],
            Stroke::new(1.6, t.accent),
            3.0,
            3.0,
        ));
        painter.text(
            pos2(rect.left() + 44.0, y),
            Align2::LEFT_TOP,
            "Pocket boundary (material remains)",
            FontId::proportional(crate::theme::Tokens::get().font_body),
            t.text_dim,
        );
    }
    true
}

/// The flat pattern's own 2D view: a top-down orthographic camera over the flat plane (x right,
/// y up, millimetres), so the shared navigation moves it like any other camera. The Design
/// camera is left alone while this has the viewport.
pub struct FlatView {
    pub cam: solidcraft_engine::render::Camera,
    /// The camera frames the pattern; cleared by Fit and Home, and whenever the dialog is closed.
    pub fitted: bool,
    /// Flat centres stay out of the command JSON and are reused while navigating.
    perforation_key: Option<(std::sync::Weak<solidcraft_engine::doc::ModelState>, String)>,
    perforations: Vec<(Vec2, f64)>,
}

impl Default for FlatView {
    fn default() -> Self {
        let mut cam = solidcraft_engine::render::Camera::default();
        cam.set_view(solidcraft_engine::render::StandardView::Top);
        cam.fov = 0.0;
        cam.up = solidcraft_engine::render::UpAxis::Z;
        FlatView { cam, fitted: false, perforation_key: None, perforations: Vec::new() }
    }
}

impl FlatView {
    fn cache_perforations(&mut self, session: &solidcraft_engine::Session, body: &str) {
        let state = session.world_state();
        let state_key = std::sync::Arc::downgrade(&state);
        if self.perforation_key.as_ref().is_some_and(|(cached, name)| cached.ptr_eq(&state_key) && name == body) {
            return;
        }
        self.perforation_key = Some((state_key, body.to_owned()));
        self.perforations.clear();
        let Some(sheet) = state.sheets.iter().find(|sheet| sheet.body == body) else { return };
        // Bound overlay storage and drawing work, including designs with many fields.
        const MAX_HOLES: usize = 1_000_000;
        for field in &sheet.perforations {
            if self.perforations.len() >= MAX_HOLES {
                break;
            }
            if !field.diameter.is_finite() || field.diameter <= 0.0 {
                continue;
            }
            let Ok(centres) = sheet.perforation_holes(field) else { continue };
            let remaining = MAX_HOLES.saturating_sub(self.perforations.len());
            self.perforations.extend(centres.into_iter().take(remaining).map(|centre| (centre, field.diameter / 2.0)));
        }
    }

    /// Frame the pattern's bounds `lo..hi` in `area`, the part of `rect` the command panel leaves.
    fn fit(&mut self, rect: Rect, area: Rect, lo: (f64, f64), hi: (f64, f64)) {
        let scale = (f64::from(area.width()) / (hi.0 - lo.0)).min(f64::from(area.height()) / (hi.1 - lo.1));
        self.cam.target.x = (lo.0 + hi.0) * 0.5 - f64::from(area.center().x - rect.center().x) / scale;
        self.cam.target.y = (lo.1 + hi.1) * 0.5 + f64::from(area.center().y - rect.center().y) / scale;
        self.cam.target.z = 0.0;
        self.cam.distance = f64::from(rect.height()) / (2.0 * scale) / 0.4;
        self.fitted = true;
    }
}

/// The Create Flat Pattern dialog is open, so the viewport shows its 2D view.
pub(crate) fn flat_active(app: &SolveApp) -> bool {
    matches!(app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Sheet(Sm::FlatPattern { .. })))
}

/// Camera input for the flat pattern: the shared navigation without orbit, driving the flat view
/// instead of the Design camera. The result reports nothing under the pointer, so the model
/// beneath neither hovers, selects nor navigates.
pub(crate) fn navigate(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect, resp: &egui::Response) -> crate::navigation::CameraInput {
    use crate::navigation::{NavCaps, handle};
    let design = std::mem::replace(&mut app.cam, app.flat_view.cam);
    let mut input = handle(app, ui, rect, resp, NavCaps { orbit: false, pick: false });
    app.flat_view.cam = std::mem::replace(&mut app.cam, design);
    app.viewport.mouse = None;
    input.inside = false;
    input.hover = None;
    input
}

fn pocket_label(app: &SolveApp, pocket: &Value) -> Option<String> {
    let depth = pocket["depth_mm"].as_f64()?;
    if !depth.is_finite() || depth <= 0.0 {
        return None;
    }
    let side = match pocket["side"].as_str()? {
        "top" => "Top",
        "bottom" => "Bottom",
        "internal" => "Internal",
        _ => return None,
    };
    Some(format!("{side} pocket: {} deep", crate::prefs::show_mm(app, depth, 1)))
}

#[cfg(test)]
#[path = "dialogs_sheet_tests.rs"]
mod tests;
