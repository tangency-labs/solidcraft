//! Dense ports of the common feature forms. Commands and parameter construction stay in dialogs.
use super::*;
use crate::ui_kit;
use crate::ui_kit::{choice_row, expr_row, hint_row};

pub(super) fn command(kind: &Kind) -> Option<&'static str> {
    match kind {
        Kind::Material { .. } => Some("material.assign"),
        Kind::Appearance(_) => Some("appearance.assign"),
        Kind::Motion(k) => Some(match k {
            crate::dialogs_motion::Mo::ContactSet { .. } => "contact.create",
            crate::dialogs_motion::Mo::MotionStudy { .. } => "motion.study",
            crate::dialogs_motion::Mo::Explode { .. } => "explode.create",
            crate::dialogs_motion::Mo::Configurations { .. } => "config.table",
        }),
        Kind::Part(_) => Some("parts.insert"),
        Kind::PartSize(_) => Some("parts.resize"),
        Kind::SaveMesh(k) => Some(crate::dialogs_save_mesh::command_id(k)),
        Kind::ViewDxf { .. } => Some("file.export_view_dxf"),
        Kind::InsertDrawing(p) => Some(if p.command.ends_with("dxf") { "sketch.insert_dxf" } else { "sketch.insert_svg" }),
        Kind::Sketch => Some("sketch.create"),
        Kind::SketchIntersect { .. } => Some("sketch.intersect"),
        Kind::SketchProject { .. } => Some("sketch.project"),
        Kind::SketchModify(k) => Some(crate::sketch_modify_ui::command(k)),
        Kind::Text(_) => Some("sketch.edit_text"),
        Kind::TextMove(_) => Some("sketch.move"),
        Kind::EditParam { .. } => Some("parameters.change"),
        Kind::Measure { .. } => Some("inspect.measure"),
        Kind::Section { .. } => Some("inspect.section"),
        Kind::Rename { .. } => Some("timeline.rename"),
        Kind::ConfirmDelete { .. } => Some("timeline.delete"),
        Kind::Extrude { .. } => Some("solid.extrude"),
        Kind::Fillet { chamfer, .. } => Some(if *chamfer { "solid.chamfer" } else { "solid.fillet" }),
        Kind::Hole { .. } => Some("solid.hole"),
        Kind::PatternRect { .. } => Some("solid.pattern.rectangular"),
        Kind::PatternCirc { .. } => Some("solid.pattern.circular"),
        Kind::Revolve { .. } => Some("solid.revolve"),
        Kind::Mirror => Some("solid.mirror"),
        Kind::Loft { .. } => Some("solid.loft"),
        Kind::Sweep { .. } => Some("solid.sweep"),
        Kind::Midplane => Some("construct.plane.midplane"),
        Kind::MeshConvert { .. } => Some("mesh.convert"),
        Kind::Construct { command, .. } => Some(command),
        Kind::PathPlane { .. } => Some("construct.plane.along_path"),
        Kind::Split => Some("solid.split_body"),
        Kind::DeleteFaces => Some("face.delete"),
        Kind::Move { .. } => Some("solid.move"),
        Kind::Thread { .. } => Some("solid.thread"),
        Kind::ReplaceFace => Some("solid.replace_face"),
        Kind::BoundaryFill { .. } => Some("solid.boundary_fill"),
        Kind::Align { .. } => Some("solid.align"),
        Kind::Remove => Some("solid.remove"),
        Kind::PathPattern { .. } => Some("solid.pattern.path"),
        Kind::Combine { .. } => Some("solid.combine"),
        Kind::Rib { web, .. } => Some(if *web { "solid.web" } else { "solid.rib" }),
        Kind::Primitive { cmd, .. } => Some(cmd),
        Kind::Surface(k) => Some(match k {
            crate::dialogs_surface::Surf::Extrude { .. } => "surface.extrude",
            crate::dialogs_surface::Surf::Revolve { .. } => "surface.revolve",
            crate::dialogs_surface::Surf::Sweep(_) => "surface.sweep",
            crate::dialogs_surface::Surf::Loft { .. } => "surface.loft",
            crate::dialogs_surface::Surf::Blend { .. } => "surface.blend",
            crate::dialogs_surface::Surf::Boundary { .. } => "surface.boundary",
            crate::dialogs_surface::Surf::Patch { .. } => "surface.patch",
            crate::dialogs_surface::Surf::Unstitch => "surface.unstitch",
            crate::dialogs_surface::Surf::ReverseNormal => "surface.reverse_normal",
            crate::dialogs_surface::Surf::Ruled { .. } => "surface.ruled",
            crate::dialogs_surface::Surf::Trim { .. } => "surface.trim",
            crate::dialogs_surface::Surf::Untrim { .. } => "surface.untrim",
            crate::dialogs_surface::Surf::SplitFace => "solid.split_face",
            _ => k.simple_metadata().map_or("", |(command, _)| command),
        }),
        Kind::Sheet(k) => match k {
            crate::dialogs_sheet::Sm::Flange { .. } => Some("sheet.flange"),
            crate::dialogs_sheet::Sm::Unfold { refold } => Some(if *refold { "sheet.refold" } else { "sheet.unfold" }),
            crate::dialogs_sheet::Sm::Convert { .. } => Some("sheet.convert"),
            crate::dialogs_sheet::Sm::Rules { .. } => Some("sheet.manage_rules"),
            crate::dialogs_sheet::Sm::FlatPattern { .. } => Some("sheet.flat_pattern"),
            _ => k.simple_metadata().map(|(command, _)| command),
        },
        Kind::Frame(_) => Some("frame.member"),
        Kind::Plastic(k) => match k {
            crate::dialogs_plastic::Pl::SnapFit { .. } => Some("plastic.snap_fit"),
            crate::dialogs_plastic::Pl::Assign { .. } => Some("plastic.assign_rule"),
            crate::dialogs_plastic::Pl::Rules { .. } => Some("plastic.manage_rules"),
            _ => k.simple_metadata().map(|(command, _)| command),
        },
        Kind::Assembly(k) => match k {
            crate::dialogs_assembly::Asm::NewComponent { .. } => Some("component.create"),
            crate::dialogs_assembly::Asm::Joint(f) => Some(if f.as_built { "joint.as_built" } else { "joint.create" }),
            crate::dialogs_assembly::Asm::JointOrigin { .. } => Some("joint.origin"),
            crate::dialogs_assembly::Asm::RigidGroup => Some("joint.rigid_group"),
            crate::dialogs_assembly::Asm::MotionLink { .. } => Some("joint.motion_link"),
            crate::dialogs_assembly::Asm::Coupling(f) => Some(f.command),
            crate::dialogs_assembly::Asm::Cam(_) => Some("joint.cam"),
            crate::dialogs_assembly::Asm::Trace(_) => Some("motion.trace"),
            crate::dialogs_assembly::Asm::Drive { .. } => Some("joint.drive"),
            crate::dialogs_assembly::Asm::Interference { .. } => Some("inspect.interference"),
        },
        _ => kind.simple_metadata().map(|(command, _)| command),
    }
}

const FILLET_TYPES: [(&str, &str); 5] = [
    ("Constant radius", "constant"),
    ("Chord length", "chord"),
    ("Variable radius", "variable"),
    ("Curvature continuous (G2)", "curvature"),
    ("Spline", "spline"),
];

/// The most instances a solid pattern makes in one direction (the document's own limit).
const MAX_PATTERN_INSTANCES: i64 = 1000;

/// A solid pattern's quantity row, with step arrows from 1 to `MAX_PATTERN_INSTANCES`. The
/// count goes to the command as typed, so it is evaluated as the document evaluates it.
fn quantity_row(ui: &mut egui::Ui, doc: &solidcraft_engine::doc::Document, label: &str, value: &mut String) -> bool {
    let eval = |text: &str| doc.eval(text.trim(), solidcraft_engine::doc::expr::Kind::Unitless).ok();
    ui_kit::count_row(ui, label, value, &ui_kit::Count { min: 1, max: MAX_PATTERN_INSTANCES, eval: &eval })
}

fn fillet_type(extra: &serde_json::Map<String, Value>) -> usize {
    let value = extra.get("type").and_then(Value::as_str).unwrap_or("constant");
    if value == "g2" { 3 } else { FILLET_TYPES.iter().position(|(_, key)| *key == value).unwrap_or(0) }
}

/// Defaults are introduced only for an explicit new type choice, never for a no-op edit.
fn choose_fillet_type(extra: &mut serde_json::Map<String, Value>, index: usize, radius: &str) {
    let Some((_, key)) = FILLET_TYPES.get(index) else { return };
    if index == 0 {
        extra.remove("type");
    } else {
        extra.insert("type".into(), json!(key));
    }
    if index != 0 {
        extra.remove("fit");
        extra.remove("corner");
    }
    if index == 2 {
        extra.entry("radius2").or_insert_with(|| json!("1 mm"));
        extra.entry("mid").or_insert_with(|| json!([]));
    }
    if index == 4 {
        let radii = spline_radii(extra, radius);
        extra.insert("radii".into(), json!(radii));
    }
}

pub(super) fn rows(app: &mut SolveApp, ctx: &egui::Context, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    if matches!(d.kind, Kind::Appearance(_)) {
        ui.spacing_mut().item_spacing.y = t.space_10;
    }
    if matches!(d.kind, Kind::Sketch | Kind::SketchModify(_) | Kind::Text(_) | Kind::TextMove(_) | Kind::EditParam { .. }) {
        let prompt = if let Some(input) = d.active_input() {
            tr(hint(input)).to_owned()
        } else {
            let key = match d.kind {
                Kind::Text(_) => "sketch_panel.text_hint",
                Kind::TextMove(_) => "sketch_panel.move_hint",
                Kind::EditParam { .. } => "sketch_panel.dimension_hint",
                _ => "dialog.selection_guidance",
            };
            tr(key).to_owned()
        };
        ui_kit::text(ui, &prompt, ui_kit::Text::Hint("tool")).on_hover_text(&prompt);
        if matches!(&d.kind, Kind::SketchModify(k) if k.previews()) {
            let label = d.active_input().map(|input| tr(input.label)).unwrap_or_else(|| tr("Objects"));
            let step = crate::i18n::format(
                "dialog.selection_step",
                &[("step", &(d.active + 1).to_string()), ("total", &d.inputs.len().to_string()), ("input", label)],
            );
            ui_kit::text(ui, &step, ui_kit::Text::Hint("step")).on_hover_text(tr("dialog.selection_guidance"));
        }
    }
    let manage_hint = match &d.kind {
        Kind::Material { .. } => Some("manage.material_hint"),
        Kind::Appearance(_) => Some("manage.appearance_hint"),
        Kind::Motion(crate::dialogs_motion::Mo::Configurations { .. }) => Some("manage.config_hint"),
        Kind::Sheet(crate::dialogs_sheet::Sm::Rules { .. }) | Kind::Plastic(crate::dialogs_plastic::Pl::Rules { .. }) => Some("manage.rules_hint"),
        Kind::Plastic(crate::dialogs_plastic::Pl::Assign { .. }) => Some("manage.assign_hint"),
        _ => None,
    };
    if let Some(key) = manage_hint {
        ui_kit::text(ui, tr(key), ui_kit::Text::Hint("manage"));
    }
    placement_row(d, ui, app);
    if crate::hole_refs::takes(d) {
        // Each reference edge with its distance (#37, #6380).
        enter |= crate::hole_refs::input_rows(d, ui, &app.session, app.preferences.origin_plane_names);
    } else {
        input_rows(d, ui, &app.session, app.preferences.origin_plane_names);
    }
    d.sync_material(&app.session);
    // The Hole dialog's dropdowns preview each choice on a copy of the form as it stands.
    let hole_form = matches!(d.kind, Kind::Hole { .. }).then(|| (d.kind.clone(), to_object_on(&d.inputs)));
    match &mut d.kind {
        Kind::Material { index, editor } => crate::material_editor::rows(app, ui, index, editor),
        Kind::Appearance(k) => enter |= crate::dialogs_appearance::rows(app, ui, k, &mut d.inputs),
        Kind::Motion(k) => enter |= crate::dialogs_motion::rows(app, ui, k, &mut d.inputs),
        Kind::Part(k) => enter |= crate::dialogs_parts::rows(app, ui, k, &d.inputs),
        Kind::PartSize(k) => enter |= crate::dialogs_part_size::rows(app, ui, k),
        Kind::SaveMesh(k) => enter |= crate::dialogs_save_mesh::rows(app, ui, k, &mut d.inputs),
        Kind::ViewDxf { dashed, tolerance } => crate::dialogs_view_dxf::rows(app, ui, dashed, tolerance),
        Kind::InsertDrawing(p) => crate::dialogs_insert_drawing::rows(ui, p),
        Kind::Sketch => {
            if d.error.is_some() && matches!(d.inputs.first().and_then(|i| i.items.first()), Some(Sel::Face { .. })) {
                ui_kit::text(
                    ui,
                    tr("This face cannot provide a durable attachment. Choose another face or explicitly use a fixed plane."),
                    ui_kit::Text::Hint("attachment"),
                );
                if ui_kit::button(ui, tr("Use fixed plane"), ui_kit::Button::Normal).clicked() {
                    d.extra.insert("fixed_plane".into(), json!(true));
                }
            }
        }
        Kind::SketchIntersect { bodies, projection_link } => {
            let mut selected = if *bodies { 1 } else { 0 };
            let changed = ui_kit::row(ui, tr("ui.sketch_intersect_selection_filter"), |ui| {
                ui_kit::segmented(ui, &mut selected, &[tr("Faces"), tr("Bodies")], Default::default())
            });
            if changed && let Some(input) = d.inputs.first_mut() {
                *bodies = selected == 1;
                input.accept = if *bodies { BODIES } else { FACES };
                input.items.clear();
            }
            ui_kit::row(ui, tr("ui.sketch_intersect_projection_link"), |ui| ui_kit::checkbox(ui, "", projection_link));
        }
        Kind::SketchProject { bodies, projection_link } => {
            let mut selected = if *bodies { 1 } else { 0 };
            let changed = ui_kit::row(ui, tr("ui.sketch_intersect_selection_filter"), |ui| {
                ui_kit::segmented(ui, &mut selected, &[tr("Geometry"), tr("Bodies")], Default::default())
            });
            if changed && let Some(input) = d.inputs.first_mut() {
                *bodies = selected == 1;
                input.accept = if *bodies { BODIES } else { super::PROJECT_GEOMETRY };
                input.items.clear();
            }
            ui_kit::row(ui, tr("ui.sketch_intersect_projection_link"), |ui| ui_kit::checkbox(ui, "", projection_link));
        }
        Kind::SketchModify(k) => enter |= crate::sketch_modify_ui::rows(ui, k, &app.session.doc),
        Kind::Text(k) => enter |= crate::sketch_text::rows(ui, k, &mut d.inputs, &mut d.active),
        Kind::TextMove(k) => enter |= crate::sketch_text_move::rows(ui, k),
        Kind::EditParam { name, expr } => {
            enter |= ui_kit::row(ui, name, |ui| ui_kit::focused_field(ui, expr));
        }
        Kind::Extrude {
            distance,
            direction,
            operation,
            auto_op,
            smart,
            distance2,
            taper,
            taper_present,
            to_offset,
            start,
            all,
            thin,
            wall,
            wall_side,
            wall2,
        } => {
            form_row(ui, "Direction", |ui| {
                // One Side, Two Sides, Symmetric (a flip is a negative distance).
                let order = [0usize, TWO_SIDES, 2];
                let reversed = *direction == 1 || *direction == TWO_SIDES_REVERSED;
                let shown = match *direction {
                    1 => 0,
                    TWO_SIDES_REVERSED => TWO_SIDES,
                    d => d,
                };
                let mut pos = order.iter().position(|o| *o == shown).unwrap_or(0);
                combo(ui, "ex_dir", &["One Side", "Two Sides", "Symmetric"], &mut pos);
                if pos != order.iter().position(|o| *o == shown).unwrap_or(0) {
                    *direction = match (order.get(pos).copied().unwrap_or(0), reversed) {
                        (0, true) => 1,
                        (TWO_SIDES, true) => TWO_SIDES_REVERSED,
                        (d, _) => d,
                    };
                    // Selecting Symmetric applies the current total-distance semantics.
                    d.extra.insert("convention".into(), json!(solidcraft_engine::doc::ExtrudeConvention::TotalDistance));
                    // An explicit direction ends the automatic choices (#1257).
                    smart.manual_dir = true;
                    *auto_op = false;
                }
            });
            let cur = if to_object_on(&d.inputs) { 1 } else { usize::from(*all) * 2 };
            let mut ext = cur;
            form_row(ui, "Extent Type", |ui| {
                combo(ui, "ex_ext", &["Distance", "To Object", "All"], &mut ext);
                *all = ext == 2;
                set_to_object(&mut d.inputs, &mut d.active, ext == 1);
            });
            if ext == 0 {
                enter |= expr_row(ui, "Distance", distance);
            }
            if ext == 1 {
                enter |= expr_row(ui, "To Offset", to_offset);
            }
            if two_sided(*direction) {
                enter |= expr_row(ui, "Distance 2", distance2);
            }
            // A choice made by hand sticks, the shown one included (#6901): Extrude's own choice
            // stops before the next frame makes it.
            if crate::op_auto::op_row(ui, "ex_op", operation) {
                *auto_op = false;
            }
            ui_kit::section(ui, "extrude_options", "Options", true, |ui| {
                let was_thin = *thin;
                let was_direction = *direction;
                let mut reverse = *direction == 1 || *direction == TWO_SIDES_REVERSED;
                if *direction < 2 || two_sided(*direction) {
                    ui_kit::checkbox_grid(ui, &mut [("Thin Extrude", thin), ("Reverse Direction", &mut reverse)], 2);
                    *direction = match (two_sided(*direction), reverse) {
                        (true, true) => TWO_SIDES_REVERSED,
                        (true, false) => TWO_SIDES,
                        (false, r) => usize::from(r),
                    };
                    if *direction != was_direction {
                        smart.manual_dir = true;
                        *auto_op = false;
                    }
                } else {
                    ui_kit::checkbox_grid(ui, &mut [("Thin Extrude", thin)], 2);
                }
                if *thin != was_thin {
                    set_thin(&mut d.inputs, *thin);
                }
                if *thin {
                    choice_row(ui, "Wall Location", "ex_wall", wall_side, &WALL_LABELS);
                    enter |= expr_row(ui, "Wall Thickness", wall);
                    if *wall_side == 3 {
                        enter |= expr_row(ui, "Thickness Outside", wall2);
                    }
                }
            });
            let advanced = !start.is_empty() || object_on(&d.inputs, FROM_OBJECT) || *taper_present;
            ui_kit::section(ui, "extrude_advanced", "Advanced", advanced, |ui| {
                let old = if object_on(&d.inputs, FROM_OBJECT) { 2 } else { usize::from(!start.is_empty()) };
                let mut st = old;
                form_row(ui, "Start", |ui| {
                    combo(ui, "ex_start", &["Profile Plane", "Offset", "From Object"], &mut st);
                    if st != old {
                        set_object(&mut d.inputs, &mut d.active, FROM_OBJECT, st == 2);
                        if st == 1 && start.is_empty() {
                            *start = length_default(&app.session, "5");
                        }
                        if st == 0 {
                            start.clear();
                        }
                    }
                });
                if st != 0 {
                    enter |= expr_row(ui, "Offset", start);
                }
                if *direction < 2 {
                    form_row(ui, "Taper Angle", |ui| {
                        let before = taper.clone();
                        enter |= field(ui, taper);
                        if *taper != before {
                            *taper_present = !taper.trim().is_empty();
                        }
                    });
                }
            });
        }
        Kind::Fillet { radius, chamfer, chain, ctype, distance2, angle, flip } => {
            if *chamfer {
                choice_row(ui, "Chamfer Type", "ch_type", ctype, &CHAMFER_TYPES);
            }
            enter |= expr_row(ui, if *chamfer { if *ctype == 0 { "Distance" } else { "Distance 1" } } else { "Radius" }, radius);
            let mut rtype = fillet_type(&d.extra);
            if !*chamfer {
                form_row(ui, "Type", |ui| {
                    let previous = rtype;
                    combo(ui, "fi_type", &FILLET_TYPES.map(|(label, _)| label), &mut rtype);
                    if previous != rtype {
                        choose_fillet_type(&mut d.extra, rtype, radius);
                    }
                });
                form_row(ui, "Continuity", |ui| {
                    let mut continuity = usize::from(rtype == 3);
                    if ui_kit::segmented(ui, &mut continuity, &["G1", "G2"], Default::default()) {
                        rtype = if continuity == 1 { 3 } else { 0 };
                        choose_fillet_type(&mut d.extra, rtype, radius);
                        // G1 explicitly returns to rolling corners. G2 describes the edge blend,
                        // whereas the separate advanced Corner control describes a vertex patch.
                        d.extra.remove("corner");
                    }
                });
            }
            if *chamfer && *ctype == 1 {
                enter |= expr_row(ui, "Distance 2", distance2);
            }
            if *chamfer && *ctype == 2 {
                enter |= expr_row(ui, "Angle", angle);
            }
            if *chamfer && *ctype > 0 {
                form_row(ui, "Flip", |ui| check(ui, flip));
            }
            ui_kit::section(ui, "fillet_options", "Options", true, |ui| {
                ui_kit::checkbox_grid(ui, &mut [("Tangent Chain", chain), ("Preview", &mut d.preview_enabled)], 2);
            });
            if !*chamfer {
                ui.add_enabled_ui(matches!(rtype, 2 | 4), |ui| {
                    ui_kit::section(ui, "variable_radius", "Variable Radius", false, |ui| {
                        if !matches!(rtype, 2 | 4) {
                            ui.add_enabled(false, egui::Label::new(tr("Choose Variable radius or Spline in Type.")).wrap());
                        }
                        if rtype == 2 {
                            form_row(ui, "End Radius", |ui| {
                                enter |= extra_field(ui, &mut d.extra, "radius2", &length_default(&app.session, "1"));
                            });
                            // Which end of the first edge takes the start radius.
                            form_row(ui, "Swap Ends", |ui| {
                                extra_check(ui, &mut d.extra, "swap_ends");
                            });
                            // Radii between the ends: position along the edge (from the start end)
                            // and radius.
                            let mut mids = mid_points(&d.extra);
                            let mut remove = None;
                            for (k, (position, r)) in mids.iter_mut().enumerate() {
                                form_row(ui, "Mid Radius", |ui| {
                                    ui.horizontal(|ui| {
                                        ui_kit::fraction_field(ui, position)
                                            .on_hover_text(tr("Position along the edge, 0 at the start end, 1 at the other"));
                                        let width = (ui.available_width() - t.space_24 - t.space_4).max(0.0);
                                        ui.allocate_ui_with_layout(vec2(width, t.space_24), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            enter |= field(ui, r);
                                        });
                                        if ui_kit::button(ui, "×", ui_kit::Button::Small).on_hover_text(tr("Remove this point")).clicked() {
                                            remove = Some(k);
                                        }
                                    });
                                });
                            }
                            if let Some(k) = remove {
                                mids.remove(k);
                            }
                            form_row(ui, "", |ui| {
                                if ui_kit::button(ui, tr("Add Mid Radius"), ui_kit::Button::Normal).clicked() && mids.len() < 20 {
                                    let t = if mids.is_empty() { 0.5 } else { (mids.iter().map(|m| m.0).fold(0.0, f64::max) + 1.0) * 0.5 };
                                    mids.push((t.min(0.99), radius.clone()));
                                }
                            });
                            d.extra.insert("mid".into(), json!(mids.iter().map(|(t, r)| json!([t, r])).collect::<Vec<_>>()));
                            form_row(ui, "Smooth", |ui| {
                                extra_check(ui, &mut d.extra, "smooth");
                            });
                        }
                        if rtype == 4 {
                            // The radius along the edge as a B-spline: its control radii from the
                            // start end, also dragged by their handles on the edge.
                            form_row(ui, "Swap Ends", |ui| {
                                extra_check(ui, &mut d.extra, "swap_ends");
                            });
                            let mut radii = spline_radii(&mut d.extra, radius);
                            let n = radii.len();
                            for (k, r) in radii.iter_mut().enumerate() {
                                enter |= expr_row(ui, if k + 1 == n { "End Radius" } else { "Control Radius" }, r);
                            }
                            d.extra.insert("radii".into(), json!(radii));
                        }
                    });
                })
                .response
                .on_disabled_hover_text(tr("Choose Variable radius or Spline in Type."));
                ui_kit::section(ui, "fillet_advanced", "Advanced", false, |ui| {
                    let constant = rtype == 0;
                    ui.add_enabled_ui(constant, |ui| {
                        let fit = form_row(ui, "Radius Fit", |ui| {
                            extra_choice(
                                ui,
                                &mut d.extra,
                                "fi_fit",
                                "fit",
                                &[("Exact", "exact"), ("Reduce to Fit", "reduce"), ("Taper to Fit", "taper")],
                            )
                        });
                        if fit == 1
                            && let Some((lo, hi)) = app.preview.applied
                        {
                            form_row(ui, "Applied Radius", |ui| {
                                let shown = if hi > lo {
                                    format!("{} – {}", crate::prefs::show_mm_bare(app, lo), crate::prefs::show_mm(app, hi, 1))
                                } else {
                                    crate::prefs::show_mm(app, lo, 1)
                                };
                                ui.label(RichText::new(shown).color(t.warning));
                            });
                        }
                    });
                    let exact = d.extra.get("fit").and_then(Value::as_str).is_none_or(|fit| fit == "exact");
                    ui.add_enabled_ui(constant && exact, |ui| {
                        form_row(ui, "Corner", |ui| {
                            extra_choice(ui, &mut d.extra, "fi_corner", "corner", &[("Rolling", "rolling"), ("G2", "g2")]);
                        });
                    })
                    .response
                    .on_hover_text(tr("G2 corners require a constant radius and Exact fit. G2 corner construction is not supported yet."));
                    if !constant {
                        ui.label(RichText::new(tr("Fit and G2 corners require Constant radius.")).color(t.text_dim).size(t.font_small));
                    }
                });
            }
        }
        Kind::PatternRect { count, spacing, count2, spacing2 } => {
            enter |= quantity_row(ui, &app.session.doc, "Quantity", count);
            enter |= expr_row(ui, "Spacing", spacing);
            enter |= quantity_row(ui, &app.session.doc, "Quantity 2", count2);
            enter |= expr_row(ui, "Spacing 2", spacing2);
            if d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES) {
                feature_pattern_alignment_row(ui, &mut d.extra, 0);
                feature_pattern_alignment_row(ui, &mut d.extra, 1);
            }
        }
        Kind::PatternCirc { count, angle } => {
            enter |= quantity_row(ui, &app.session.doc, "Quantity", count);
            enter |= expr_row(ui, "Total angle", angle);
            if d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES) {
                feature_pattern_alignment_row(ui, &mut d.extra, 0);
            }
        }
        Kind::Hole { kind, opts, .. } => {
            let doc = &app.session.doc;
            let eval = |e: &str, k| doc.eval(e, k).ok();
            // Each option shows the hole it makes while hovered (dialogs_hole::preview).
            let preview = |ui: &mut egui::Ui, choice| {
                if let Some((form, to_object)) = &hole_form {
                    crate::dialogs_hole::preview(ui, form, *to_object, choice, &eval);
                }
            };
            use crate::dialogs_hole::Choice;
            form_row(ui, "Extents", |ui| {
                let cur = if to_object_on(&d.inputs) { 1 } else { usize::from(opts.all) * 2 };
                let mut ext = cur;
                ui_kit::dropdown_previews(ui, "hole_ext", &mut ext, &["Distance", "To Object", "All"], |ui, i| preview(ui, Choice::Extents(i)));
                opts.all = ext == 2;
                set_to_object(&mut d.inputs, &mut d.active, ext == 1);
            });
            if opts.multiple {
                // Every point of the picked points' sketches (standalone points and
                // circle centres), not just the picked ones.
                form_row(ui, "All Sketch Points", |ui| {
                    extra_check(ui, &mut d.extra, "all_points");
                });
                // The holes go into the part the sketch touches; Flip drills the
                // other way.
                form_row(ui, "Flip", |ui| {
                    extra_check(ui, &mut d.extra, "flip");
                });
            }
            form_row(ui, "Hole Type", |ui| {
                ui_kit::dropdown_previews(ui, "hole_kind", kind, &HOLE_LABELS, |ui, i| preview(ui, Choice::Type(i)));
            });
            form_row(ui, "Hole Tap Type", |ui| {
                let mut tap = usize::from(opts.tapped);
                ui_kit::dropdown_previews(ui, "hole_tap", &mut tap, &["Simple", "Tapped"], |ui, i| preview(ui, Choice::Tapped(i == 1)));
                opts.tapped = tap == 1;
            });
            if opts.tapped {
                enter |= crate::dialogs_thread::hole_rows(ui, &app.session.doc, &mut opts.thread, &mut opts.custom_thread);
                ui_kit::checkbox_grid(ui, &mut [("Full thread depth", &mut opts.thread_full_depth)], 1);
                crate::dialogs_thread::handedness_row(ui, &mut opts.handedness, "hole_handedness");
            }
            // Through all, the tip lies past the part.
            if !opts.all {
                form_row(ui, "Drill Point", |ui| {
                    let mut tip = usize::from(opts.angled);
                    ui_kit::dropdown_previews(ui, "hole_tip", &mut tip, &["Flat", "Angle"], |ui, i| preview(ui, Choice::Angled(i == 1)));
                    opts.angled = tip == 1;
                });
            }
            // The section through the hole, each size beside its dimension (#2171).
            let to_object = to_object_on(&d.inputs);
            enter |= crate::dialogs_hole::rows(ui, &mut d.kind, to_object, &eval);
        }
        _ => enter |= additional_rows(app, d, ui),
    }
    error_rows(app, ctx, d, ui);
    enter
}

fn extra_segments(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>, key: &str, choices: &[(&str, &str)]) {
    let current = extra.get(key).and_then(Value::as_str).and_then(|value| choices.iter().position(|(_, key)| *key == value)).unwrap_or(0);
    let mut selected = current;
    let labels: Vec<_> = choices.iter().map(|(label, _)| *label).collect();
    if ui_kit::segmented(ui, &mut selected, &labels, Default::default()) && selected != current {
        if selected == 0 {
            extra.remove(key);
        } else if let Some((_, value)) = choices.get(selected) {
            extra.insert(key.into(), json!(value));
        }
    }
}

/// Only user changes add optional JSON keys, preserving no-op feature edits.
fn extra_options(ui: &mut egui::Ui, id: &str, extra: &mut serde_json::Map<String, Value>, choices: &[(&str, &str)]) {
    let mut values: Vec<bool> = choices.iter().map(|(_, key)| extra.get(*key).and_then(Value::as_bool).unwrap_or(false)).collect();
    let before = values.clone();
    let mut items: Vec<_> = choices.iter().zip(values.iter_mut()).map(|((label, _), value)| (*label, value)).collect();
    ui_kit::options(ui, id, &mut items);
    for (((_, key), value), before) in choices.iter().zip(values).zip(before) {
        if value != before {
            extra.insert((*key).into(), json!(value));
        }
    }
}

/// The message a primitive's size field is labelled with, by its parameter name: "Section size",
/// not `section_size`.
pub(crate) fn primitive_label(key: &str) -> &str {
    match key {
        "length" => "ui.length",
        "width" => "ui.width",
        "height" => "ui.height",
        "diameter" => "ui.diameter",
        "radius" => "ui.radius",
        "top_diameter" => "ui.top_diameter",
        "top_radius" => "ui.top_radius",
        "major" => "ui.major_radius",
        "minor" => "ui.minor_radius",
        "revolutions" => "ui.revolutions",
        "pitch" => "ui.pitch",
        "section_size" => "ui.section_size",
        _ => key,
    }
}

/// The coil's section, where the section sits on the diameter, the way it turns and where it
/// starts (#3862). A default stays out of the parameters, so OK on an unchanged edit keeps the coil.
fn coil_rows(ui: &mut egui::Ui, extra: &mut serde_json::Map<String, Value>) -> bool {
    const POSITIONS: [(&str, &str); 3] = [("Inside", "inside"), ("Center", "center"), ("Outside", "outside")];
    form_row(ui, "Section", |ui| extra_segments(ui, extra, "section", &[("Circular", "circular"), ("Square", "square")]));
    form_row(ui, "Section position", |ui| {
        let current =
            extra.get("section_position").and_then(Value::as_str).and_then(|v| POSITIONS.iter().position(|(_, key)| *key == v)).unwrap_or(1);
        let mut selected = current;
        if ui_kit::segmented(ui, &mut selected, &POSITIONS.map(|(label, _)| label), Default::default()) && selected != current {
            match POSITIONS.get(selected) {
                Some((_, value)) if *value != "center" => {
                    extra.insert("section_position".into(), json!(value));
                }
                _ => {
                    extra.remove("section_position");
                }
            }
        }
    });
    let mut angle = extra.get("start_angle").and_then(Value::as_str).unwrap_or_default().to_string();
    let before = angle.clone();
    let enter = hint_row(ui, "Start angle", &mut angle, "0 deg");
    if angle != before {
        if angle.trim().is_empty() {
            extra.remove("start_angle");
        } else {
            extra.insert("start_angle".into(), json!(angle));
        }
    }
    extra_options(ui, "coil_options", extra, &[("Clockwise", "clockwise")]);
    enter
}

fn additional_rows(app: &mut SolveApp, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let mut enter = false;
    let through_point = offset_through_point(d).is_some();
    match &mut d.kind {
        Kind::Revolve { angle, operation, .. } => {
            let cur = usize::from(to_object_on(&d.inputs));
            let mut ty = cur;
            form_row(ui, "Type", |ui| {
                ui_kit::segmented(ui, &mut ty, &["Angle", "To Object"], Default::default());
                set_to_object(&mut d.inputs, &mut d.active, ty == 1);
            });
            if ty == 0 {
                // One Side, Two Sides (a second angle), Symmetric (the angle split).
                let cur = if d.extra.contains_key("angle2") {
                    1
                } else if d.extra.get("direction").is_some() {
                    2
                } else {
                    0
                };
                let mut dir = cur;
                form_row(ui, "Direction", |ui| {
                    ui_kit::segmented(ui, &mut dir, &["One Side", "Two Sides", "Symmetric"], Default::default());
                    if dir != cur {
                        d.extra.remove("angle2");
                        d.extra.remove("direction");
                        match dir {
                            1 => {
                                d.extra.insert("angle2".into(), json!("90 deg"));
                            }
                            2 => {
                                d.extra.insert("direction".into(), json!("symmetric"));
                            }
                            _ => {}
                        }
                    }
                });
                enter |= expr_row(ui, "Angle", angle);
                if dir == 1 {
                    form_row(ui, "Angle 2", |ui| {
                        enter |= extra_field(ui, &mut d.extra, "angle2", "90 deg");
                    });
                }
            }
            crate::op_auto::op_row(ui, "rv_op", operation);
        }
        Kind::Shell { thickness } => {
            // The direction below says which side the wall grows to; the label stays short enough
            // for the label column.
            enter |= expr_row(ui, "Thickness", thickness);
            form_row(ui, "Direction", |ui| {
                extra_segments(ui, &mut d.extra, "direction", &[("Inside", "inside"), ("Outside", "outside"), ("Both", "both")]);
            });
            extra_options(ui, "shell_options", &mut d.extra, &[("Tangent Chain", "tangent_chain")]);
        }
        Kind::Draft { angle } => {
            enter |= expr_row(ui, "Angle", angle);
        }
        Kind::Mirror => {
            // Feature replay retains the source operation; Join is for body copies.
            if !d.inputs.first().is_some_and(|i| i.accept == selection::FEATURES) {
                form_row(ui, "Operation", |ui| {
                    let mut op = usize::from(d.extra.get("combine").and_then(Value::as_bool).unwrap_or(false));
                    ui_kit::segmented(ui, &mut op, &["New Body", "Join"], Default::default());
                    d.extra.insert("combine".into(), json!(op == 1));
                });
            }
        }
        Kind::OffsetPlane { offset } => {
            if !through_point {
                enter |= expr_row(ui, "Distance", offset);
            }
        }
        Kind::AnglePlane { angle } => {
            enter |= expr_row(ui, "Angle", angle);
        }
        Kind::PathPlane { t, .. } => {
            enter |= expr_row(ui, "Position (0 to 1)", t);
        }
        Kind::MeshConvert { tolerance } => {
            form_row(ui, "Tolerance", |ui| enter |= field(ui, tolerance));
        }
        Kind::Construct { t: Some(t), .. } => {
            form_row(ui, "Position (0 to 1)", |ui| enter |= field(ui, t));
        }
        Kind::Split | Kind::ReplaceFace | Kind::Remove | Kind::Midplane | Kind::ChatContext | Kind::Construct { t: None, .. } => {}
        Kind::Rib { thickness, depth, flip, normal, .. } => {
            form_row(ui, "Direction", |ui| {
                let mut i = usize::from(*normal);
                ui_kit::segmented(ui, &mut i, &["In plane", "Normal"], Default::default());
                *normal = i == 1;
            });
            enter |= expr_row(ui, "Thickness", thickness);
            enter |= hint_row(ui, "Depth", depth, tr("to the next face"));
            ui_kit::options(ui, "feature_options", &mut [("Flip", flip)]);
        }
        Kind::Emboss { depth, deboss } => {
            enter |= expr_row(ui, "Depth", depth);
            form_row(ui, "Type", |ui| {
                let mut i = usize::from(*deboss);
                ui_kit::segmented(ui, &mut i, &["Emboss", "Deboss"], Default::default());
                *deboss = i == 1;
            });
            ui.add(egui::Label::new(crate::i18n::tr("ui.emboss_wrap_note")).wrap());
        }
        Kind::Align { flip, components, axes: [ax, ay, az] } => {
            // Where on the picked face or edge to snap: as picked (a face turns to meet
            // the other, an edge at its middle), its centre, or a round one's centre.
            for (label, key) in [("From Snap", "from_snap"), ("To Snap", "to_snap")] {
                form_row(ui, label, |ui| {
                    extra_choice(ui, &mut d.extra, key, key, &[("As Picked", "picked"), ("Center", "center"), ("Circle Center", "circle")]);
                });
            }
            if *components {
                ui_kit::options(ui, "feature_options", &mut [("Flip", flip)]);
            } else {
                ui_kit::options(ui, "feature_options", &mut [("Flip", flip), ("X", ax), ("Y", ay), ("Z", az)]);
            }
            // A part inside a subassembly placed more than once sits where that shared
            // definition says: aligning it moves it in every placement.
            if *components && align_moves_shared(&app.session, d.inputs.first().map_or(&[], |i| i.items.as_slice())) {
                let t = Tokens::get();
                ui.add(egui::Label::new(RichText::new(tr(ALIGN_SHARED_WARNING)).color(t.warning).size(t.font_small)).wrap());
            }
        }
        Kind::BoundaryFill { operation, cells, .. } => {
            form_row(ui, "Cells", |_| {});
            if cells.is_empty() {
                ui_kit::status_line(ui, "Pick the tool bodies", ui_kit::StatusTone::Dim);
            }
            for (i, (_, label, on)) in cells.iter_mut().enumerate() {
                form_row(ui, &format!("Cell {}", i + 1), |ui| {
                    ui_kit::checkbox(ui, label.as_str(), on);
                });
            }
            choice_row(ui, "Operation", "bf_op", operation, &OP_LABELS);
            extra_options(ui, "boundary_options", &mut d.extra, &[("Remove Tools", "remove_tools")]);
        }
        Kind::PathPattern { count, spacing } => {
            enter |= quantity_row(ui, &app.session.doc, "Quantity", count);
            enter |= expr_row(ui, if d.extra.contains_key("distance") { "Distance" } else { "Spacing" }, spacing);
        }
        Kind::Pipe { diameter, wall, operation } => {
            enter |= expr_row(ui, "Diameter", diameter);
            enter |= hint_row(ui, "Wall", wall, tr("solid"));
            choice_row(ui, "Operation", "pp_op", operation, &OP_LABELS);
        }
        Kind::Stock { margin } => {
            enter |= expr_row(ui, "Margin", margin);
        }
        Kind::Scale { factor } => {
            enter |= expr_row(ui, "Scale factor", factor);
        }
        Kind::OffsetFaces { distance } => {
            enter |= expr_row(ui, "Distance", distance);
        }
        Kind::Thread { designation, length, suggested, of, handedness } => {
            let mut full = length.trim().is_empty();
            let was_full = full;
            let mut modeled = d.extra.get("modeled").and_then(Value::as_bool).unwrap_or(false);
            let before = modeled;
            let (submitted, reset) = crate::dialogs_thread::rows(ui, designation, suggested, &mut modeled, &mut full);
            crate::dialogs_thread::handedness_row(ui, handedness, "thread_handedness");
            if modeled != before {
                d.extra.insert("modeled".into(), json!(modeled));
            }
            if full != was_full {
                *length = if full { String::new() } else { length_default(&app.session, "10") };
            }
            enter |= submitted;
            if reset {
                of.clear();
            }
            if !full {
                enter |= expr_row(ui, "Length", length);
            }
        }
        Kind::Loft { operation, rail_fit } => {
            crate::op_auto::op_row(ui, "lf_op", operation);
            crate::dialogs_sweep::rail_fit_row(ui, "lf", rail_fit);
        }
        Kind::Sweep { operation, opts } => {
            enter |= crate::dialogs_sweep::rows_with(ui, opts, "sw", |ui| {
                crate::op_auto::op_row(ui, "sw_op", operation);
            });
        }
        k @ Kind::Move { .. } => {
            enter |= crate::dialogs_move::rows(ui, k);
            extra_options(ui, "move_options", &mut d.extra, &[("Create Copy", "copy")]);
        }
        Kind::Primitive { cmd, fields, operation, at, .. } => {
            let from_sketch = *cmd == "solid.coil" && d.inputs.first().is_some_and(|i| i.accept & selection::PROFILES != 0 && !i.items.is_empty());
            for (k, v) in fields.iter_mut() {
                if from_sketch && (*k == "diameter" || *k == "section_size") {
                    continue;
                }
                enter |= expr_row(ui, primitive_label(k), v);
            }
            if *cmd == "solid.coil" {
                enter |= coil_rows(ui, &mut d.extra);
            }
            if at.is_none() {
                enter |= crate::primitive_placement::rows(ui, &app.session, cmd, &mut d.extra);
            }
            if let Some((p, _)) = *at {
                form_row(ui, "Position", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z));
                        // A cylinder's place is its Face pick, cleared there.
                        if *cmd != "solid.cylinder"
                            && ui_kit::button(ui, "×", ui_kit::Button::Small).on_hover_text(tr("Place at the origin instead")).clicked()
                        {
                            *at = None;
                        }
                    });
                });
            }
            crate::op_auto::op_row(ui, "pr_op", operation);
        }
        Kind::Combine { operation, keep_tools } => {
            form_row(ui, "Operation", |ui| {
                let mut op = operation.saturating_sub(1);
                combo(ui, "cb_op", &OP_LABELS[1..], &mut op);
                *operation = op + 1;
            });
            ui_kit::section(ui, "combine_options", "Options", true, |ui| {
                let mut new_component = d.extra.get("new_component").and_then(Value::as_bool).unwrap_or(false);
                let before = new_component;
                ui_kit::checkbox_grid(ui, &mut [("New Component", &mut new_component), ("Keep Tools", keep_tools)], 2);
                if new_component != before {
                    d.extra.insert("new_component".into(), json!(new_component));
                }
            });
        }
        Kind::Surface(k) => enter |= crate::dialogs_surface::rows(ui, k),
        Kind::Sheet(k) => enter |= crate::dialogs_sheet::rows(app, ui, k, &mut d.inputs),
        Kind::Plastic(k) => enter |= crate::dialogs_plastic::rows(app, ui, k, &d.inputs),
        Kind::Frame(k) => enter |= crate::dialogs_frame::rows(app, ui, k, &d.inputs),
        Kind::Assembly(k) => enter |= crate::dialogs_assembly::rows(app, ui, k, &mut d.inputs, &mut d.active),
        Kind::Measure { result, show, .. } => measure_rows(app, ui, result.as_ref(), show),
        Kind::Section { offset, flip, hatch } => {
            enter |= expr_row(ui, "Distance", offset);
            ui_kit::options(ui, "feature_options", &mut [("Flip", flip), ("Hatch", hatch)]);
            if section_turns_off(&app.session, &d.inputs) && !d.extra.contains_key("plane") {
                ui_kit::text(ui, tr("section.off_hint"), ui_kit::Text::Hint("section_off"));
            }
        }
        Kind::Rename { name, .. } => {
            form_row(ui, "Name", |ui| {
                enter |= ui_kit::focused_text_field(ui, name, "");
            });
        }
        _ => {}
    }
    enter
}

fn fmt_mm(app: &SolveApp, v: &Value) -> String {
    v.as_f64().map(|x| crate::prefs::show_mm(app, x, 1)).unwrap_or_default()
}

/// The measurement in the dialog: each item's size, then the distance (a pair of round items
/// offers centre to centre, closest and farthest), its ΔX/Y/Z and the angle between two items.
fn measure_rows(app: &SolveApp, ui: &mut egui::Ui, r: Option<&Value>, show: &mut Option<Shown>) {
    let Some(r) = r else {
        ui_kit::text(ui, tr("pick one or two vertices, edges or faces"), ui_kit::Text::Hint("measure"));
        return;
    };
    if let Some(e) = r.get("error").and_then(Value::as_str) {
        ui_kit::text(ui, e, ui_kit::Text::Report("measure_error", true));
        return;
    }
    let mm = |v: &Value| fmt_mm(app, v);
    let value = |ui: &mut egui::Ui, label: &str, text: &str| {
        form_row(ui, label, |ui| {
            ui_kit::text(ui, text, ui_kit::Text::Cell);
        });
    };
    for (i, it) in r["items"].as_array().into_iter().flatten().enumerate() {
        let lines = crate::quick_measure::item_lines(app, it);
        for (k, line) in lines.into_iter().enumerate() {
            let label = if k == 0 { crate::i18n::format("measure.item", &[("number", &(i + 1).to_string())]) } else { String::new() };
            value(ui, &label, &line);
        }
    }
    if let Some((now, d, a, b)) = shown_distance(r, *show) {
        if r.get("center_distance").is_some() {
            // Centre, min and max all listed; the chosen one is drawn and broken into ΔX/Y/Z.
            for (which, label, key) in
                [(Shown::Centre, "Centre to centre", "center_distance"), (Shown::Min, "Min", "distance_mm"), (Shown::Max, "Max", "max")]
            {
                let Some(v) = r.get(key).filter(|v| v.is_number()) else { continue };
                if ui_kit::list_picker(ui, key, &format!("{}  {}", tr(label), mm(v)), now == which).clicked() {
                    *show = Some(which);
                }
            }
        } else {
            let text = crate::prefs::show_mm(app, d, 1);
            form_row(ui, crate::i18n::tr_label("Distance"), |ui| {
                let size = crate::theme::Tokens::get().form_close_size;
                // The value keeps the room left of the button, so the button stays inside the row.
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - size).max(0.0));
                    ui_kit::text(ui, &text, ui_kit::Text::Cell);
                });
                let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                let copy = ui_kit::icon_button_at(ui, rect, "copy", crate::i18n::tr_label("Copy"));
                crate::scenario::publish_handle("measure:copy:distance", copy.rect.center());
                if copy.clicked() {
                    ui.ctx().copy_text(text.clone());
                }
            });
        }
        let delta = b - a;
        for (l, v) in [("ΔX", delta.x), ("ΔY", delta.y), ("ΔZ", delta.z)] {
            value(ui, l, &crate::prefs::show_mm(app, v.abs(), 1));
        }
    }
    if let Some(a) = r["angle_deg"].as_f64()
        && r["items"].as_array().is_some_and(|v| v.len() == 2)
    {
        value(ui, crate::i18n::tr_label("Angle"), &format!("{a:.3}°"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{self, Harness};

    const COMMANDS: [&str; 6] =
        ["solid.extrude", "solid.fillet", "solid.chamfer", "solid.hole", "solid.pattern.rectangular", "solid.pattern.circular"];

    fn sample(h: &mut Harness) {
        h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":"5 mm"})).unwrap();
    }

    #[test]
    fn common_forms_fit_both_layouts_and_scales_and_pin_footer_at_small_heights() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for scale in [1.0, 2.0] {
                for height in [950.0, 720.0] {
                    let mut h = Harness::new();
                    h.call("ui.set", json!({"layout": mode}));
                    h.app.preferences.ui_scale = scale;
                    h.call("ui.resize", json!({"width": 1400.0 / scale, "height": height / scale}));
                    sample(&mut h);
                    for command in COMMANDS {
                        h.step(&json!({"start":command})).unwrap();
                        // Force the long Hole variant too: counterbore, tapping and multiple picks.
                        if let Some(Dialog { kind: Kind::Hole { kind, opts, .. }, .. }) = h.app.dialog.as_mut() {
                            *kind = 1;
                            opts.tapped = true;
                            opts.multiple = true;
                        }
                        h.frames(8);
                        let card = scenario::rect_of("dialog:card").unwrap();
                        let viewport = h.app.viewport.rect.unwrap();
                        assert!(
                            viewport.expand(1.0).contains_rect(card),
                            "{command} {mode:?} scale={scale} height={height}: {card:?} outside {viewport:?}"
                        );
                        for action in ["dialog:ok", "dialog:cancel"] {
                            let rect = scenario::rect_of(action).unwrap();
                            assert!(
                                card.contains_rect(rect),
                                "{command} {mode:?} scale={scale} height={height}: clipped {action} {rect:?} card={card:?}"
                            );
                        }
                        assert!(crate::shell::covered_rects(&h.app).contains(&card));
                        assert!(card.width() <= 301.0, "{card:?}");
                        h.step(&json!({"widget":"Cancel"})).unwrap();
                        assert!(h.app.dialog.is_none(), "{command}");
                    }
                }
            }
        }
    }

    #[test]
    fn tiny_windows_do_not_panic_or_publish_non_finite_cards() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for size in [(1.0, 1.0), (40.0, 80.0), (180.0, 240.0)] {
                let mut h = Harness::new();
                h.call("ui.set", json!({"layout": mode}));
                sample(&mut h);
                h.call("ui.resize", json!({"width":size.0,"height":size.1}));
                for command in COMMANDS {
                    h.app.start(command);
                    assert!(h.app.dialog.is_some(), "{command} must actually open");
                    h.frames(3);
                    if let Some(rect) = h.app.viewport.dialog_rect {
                        assert!(rect.is_finite() && rect.width() >= 0.0 && rect.height() >= 0.0, "{command}: {rect:?}");
                    }
                    cancel(&mut h.app);
                }
            }
        }
    }

    #[test]
    fn fillet_options_and_continuity_keep_the_existing_command_parameters() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        sample(&mut h);
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        h.step(&json!({"click":{"handle":"dialog:Continuity:G2"}})).unwrap();
        assert_eq!(h.app.dialog.as_ref().unwrap().extra.get("type"), Some(&json!("curvature")));
        h.step(&json!({"click":{"handle":"dialog:Tangent Chain"}})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().unwrap().kind, Kind::Fillet { chain: false, .. }));
        h.step(&json!({"click":{"handle":"dialog:section:Options"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Tangent Chain").is_none());
        h.step(&json!({"click":{"handle":"dialog:section:Options"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Tangent Chain").is_some());
    }
    fn fillet_harness() -> Harness {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":1000}));
        sample(&mut h);
        let edge = edge_sel(&h.app.session, Vec3::new(5.0, 0.0, 5.0)).unwrap();
        h.app.session.selection = vec![edge];
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(4);
        h
    }

    fn parameters(h: &Harness) -> Value {
        let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands.first().unwrap().0, "solid.fillet");
        commands.first().unwrap().1.clone()
    }

    fn choose(h: &mut Harness, row: &str, option: &str) {
        h.step(&json!({"click":{"handle":format!("dialog:{row}")}})).unwrap();
        h.step(&json!({"click":{"handle":format!("dialog:{row}:{option}")}})).unwrap();
        h.frames(2);
    }

    #[test]
    fn every_type_choice_produces_its_supported_command_without_changing_defaults() {
        for (label, key) in FILLET_TYPES {
            let mut h = fillet_harness();
            let original = parameters(&h);
            h.frames(3);
            assert_eq!(parameters(&h), original, "merely drawing the default form is lossless");
            choose(&mut h, "Type", label);
            let p = parameters(&h);
            assert_eq!(p.get("type").and_then(Value::as_str).unwrap_or("constant"), key, "{p}");
            assert_eq!(p["radius"], original["radius"]);
            assert_eq!(p["edges"], original["edges"]);
            assert!(p.get("preview").is_none() && p.get("preview_enabled").is_none());
            if key == "variable" {
                assert_eq!(p["radius2"], json!("1 mm"));
                assert!(p["start"].is_array());
            }
            if key == "spline" {
                assert_eq!(p["radii"].as_array().unwrap().len(), 4);
                assert_eq!(p["knots"].as_array().unwrap().len(), 9);
                assert!(p["start"].is_array());
            }
            assert!(scenario::handle_at("dialog:Continuity:G3").is_none());
        }
    }

    #[test]
    fn continuity_and_advanced_controls_map_to_edge_type_fit_and_corner() {
        let mut h = fillet_harness();
        assert!(scenario::handle_at("dialog:Radius Fit").is_none(), "Advanced starts collapsed");
        h.step(&json!({"click":{"handle":"dialog:section:Advanced"}})).unwrap();
        choose(&mut h, "Corner", "G2");
        assert_eq!(parameters(&h)["corner"], json!("g2"));
        choose(&mut h, "Corner", "Rolling");
        assert!(parameters(&h).get("corner").is_none());
        choose(&mut h, "Radius Fit", "Reduce to Fit");
        assert_eq!(parameters(&h)["fit"], json!("reduce"));
        choose(&mut h, "Radius Fit", "Taper to Fit");
        assert_eq!(parameters(&h)["fit"], json!("taper"));
        choose(&mut h, "Radius Fit", "Exact");
        assert!(parameters(&h).get("fit").is_none());
        h.step(&json!({"click":{"handle":"dialog:Continuity:G2"}})).unwrap();
        assert_eq!(parameters(&h)["type"], json!("curvature"));
        h.step(&json!({"click":{"handle":"dialog:Continuity:G1"}})).unwrap();
        let p = parameters(&h);
        assert!(p.get("type").is_none() && p.get("corner").is_none());
        choose(&mut h, "Type", "Chord length");
        h.step(&json!({"click":{"handle":"dialog:Continuity:G1"}})).unwrap();
        assert!(parameters(&h).get("type").is_none(), "even an already highlighted G1 explicitly selects Constant radius");
    }

    #[test]
    fn variable_controls_toggle_keep_mid_radii_and_preview_is_ui_only() {
        let mut h = fillet_harness();
        let original = parameters(&h);
        h.step(&json!({"click":{"handle":"dialog:Preview"}})).unwrap();
        h.frames(3);
        assert!(!h.app.dialog.as_ref().unwrap().preview_enabled);
        assert!(!h.app.preview.active && !h.app.preview.busy);
        assert_eq!(parameters(&h), original);
        h.step(&json!({"click":{"handle":"dialog:Preview"}})).unwrap();
        assert!(h.app.dialog.as_ref().unwrap().preview_enabled);
        assert_eq!(parameters(&h), original);
        assert!(scenario::handle_at("dialog:End Radius").is_none());
        choose(&mut h, "Type", "Variable radius");
        assert!(scenario::handle_at("dialog:End Radius").is_none(), "the section stays collapsed after a type choice");
        h.step(&json!({"click":{"handle":"dialog:section:Variable Radius"}})).unwrap();
        assert!(scenario::handle_at("dialog:End Radius").is_some());
        assert!(scenario::handle_at("dialog:Smooth").is_some(), "Smooth is a supported variable-radius option even before adding a middle radius");
        h.step(&json!({"widget":"Add Mid Radius"})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Smooth"}})).unwrap();
        let saved = parameters(&h);
        assert_eq!(saved["mid"].as_array().unwrap().len(), 1);
        assert_eq!(saved["smooth"], json!(true));
        h.step(&json!({"click":{"handle":"dialog:section:Variable Radius"}})).unwrap();
        h.frames(2);
        assert!(scenario::handle_at("dialog:Mid Radius").is_none());
        assert_eq!(parameters(&h), saved, "folding keeps all values and commands");
        assert!(scenario::handle_at("dialog:section:Setbacks").is_none());
        assert!(scenario::handle_at("dialog:Result").is_none());
    }

    #[test]
    fn edges_field_paints_zero_one_and_many_and_footer_follows_validity() {
        fn contains_text(shape: &egui::Shape, expected: &str) -> bool {
            match shape {
                egui::Shape::Text(text) => text.galley.job.text == expected,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains_text(shape, expected)),
                _ => false,
            }
        }
        let mut h = fillet_harness();
        let picks: Vec<_> = [Vec3::new(5.0, 0.0, 5.0), Vec3::new(0.0, 5.0, 5.0), Vec3::new(10.0, 5.0, 5.0)]
            .into_iter()
            .map(|p| edge_sel(&h.app.session, p).unwrap())
            .collect();
        for (count, text) in [(0, "Select edges"), (1, "1 selected"), (3, "3 selected")] {
            h.app.dialog.as_mut().unwrap().inputs.first_mut().unwrap().items = picks.iter().take(count).cloned().collect();
            h.frames(3);
            if count <= 1 {
                let tree = h.output.accesskit_update.as_ref().unwrap();
                let disabled = tree
                    .nodes
                    .iter()
                    .find_map(|(_, node)| (node.role() == egui::accesskit::Role::Button && node.label() == Some("OK")).then(|| node.is_disabled()))
                    .unwrap();
                assert_eq!(disabled, count == 0, "OK follows the actual required edge selection");
            }
            let mut output = h.ctx.run_ui(
                egui::RawInput {
                    time: Some(h.ctx.input(|input| input.time)),
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 1000.0))),
                    ..Default::default()
                },
                |ui| h.app.ui(ui),
            );
            let painted = output.shapes.iter().any(|shape| contains_text(&shape.shape, text));
            output.textures_delta.clear();
            assert!(painted, "the actual field must paint {text}");
            let field = scenario::rect_of("dialog:field:Edges").unwrap();
            assert!(
                (field.width() - scenario::rect_of("dialog:control:Edges").unwrap().width()).abs() < 0.02,
                "clear is inside the full field (allow egui subpixel rounding)"
            );
            assert!(h.widget("OK").is_some());
            if count == 0 {
                h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
                assert!(h.app.dialog.is_some(), "invalid OK stays disabled");
            }
        }
    }
}

#[cfg(test)]
#[path = "dialogs_all_tests.rs"]
mod all_tests;

#[cfg(test)]
#[path = "dialogs_sketch_tests.rs"]
mod sketch_tests;
