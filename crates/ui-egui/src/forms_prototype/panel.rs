//! The Forms feature panel shares the application's normal panel and form controls.
use super::*;
use crate::ui_kit::{self, Container, Edge, Surface};

const PRIMITIVES: [(&str, &str); 8] = [
    ("box", "forms.prototype.box"),
    ("cylinder", "forms.prototype.cylinder"),
    ("quadball", "forms.prototype.quadball"),
    ("sphere", "forms.prototype.sphere"),
    ("pipe", "forms.prototype.pipe"),
    ("face", "forms.prototype.plane"),
    ("torus", "forms.prototype.torus"),
    ("tube", "forms.prototype.tube"),
];
pub(super) fn sync(app: &mut SolveApp) {
    if let Some(form) = &app.session.forms_prototype {
        app.forms_prototype.primitive = PRIMITIVES.iter().position(|(kind, _)| *kind == form.parameters.kind).unwrap_or(0);
    }
}
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let frame = ui_kit::Panel::feature_frame(ui.ctx(), app.ui.layout, crate::frame::Side::Right).inner_margin(t.space_12);
    Container::new(ui.ctx(), Surface::Feature)
        .edge("solidcraft_forms_panel", Edge::Right)
        .default_size(t.form_card_width + t.space_24)
        .size_range(t.form_card_width..=t.form_card_width * 1.5)
        .resizable(true)
        .frame(frame)
        .show(ui, |ui| {
            ui_kit::scope(ui, |ui| {
                let profile = app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_some());
                let pipe = app.session.forms_prototype.as_ref().is_some_and(|f| f.pipe.is_some());
                let imported = app.session.forms_prototype.as_ref().is_some_and(|f| f.imported.is_some());
                ui.heading(app.tr(if profile {
                    "forms.prototype.surface_edit_title"
                } else if pipe {
                    "forms.prototype.pipe_edit_title"
                } else if imported {
                    "forms.prototype.imported_title"
                } else {
                    "forms.prototype.edit_title"
                }));
                header(app, ui);
                if app.session.forms_prototype.is_none() {
                    return;
                }
                ui.separator();
                let busy = app.forms_prototype.pending.is_some() || app.forms_prototype.interchange.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    egui::ScrollArea::vertical().id_salt("forms_panel_scroll").auto_shrink([false, false]).show(ui, |ui| {
                        if tool_panel::active(app) {
                            tool_panel::show(app, ui);
                            return;
                        }
                        if profile {
                            surface_settings(app, ui);
                        } else if pipe {
                            pipe_settings(app, ui);
                        } else if imported {
                            if let Some(source) = app.session.forms_prototype.as_ref().and_then(|f| f.imported.as_ref()) {
                                ui.strong(&source.object);
                            }
                        } else {
                            section(app, ui, "forms.prototype.primitive_section", true, primitive);
                        }
                        ui.separator();
                        selection(app, ui);
                        ui.horizontal(|ui| {
                            let has_edges = app.forms_prototype.elements.iter().any(|e| matches!(e, Element::Edge(_)));
                            let r = ui.add_enabled(has_edges, egui::Button::new(app.tr("forms.prototype.edge_loop")));
                            crate::scenario::publish_handle("forms_select_loop", r.rect.center());
                            if r.clicked() {
                                selection::expand(app, false);
                            }
                            let r = ui.add_enabled(!app.forms_prototype.elements.is_empty(), egui::Button::new(app.tr("forms.prototype.connected")));
                            crate::scenario::publish_handle("forms_select_connected", r.rect.center());
                            if r.clicked() {
                                selection::expand(app, true);
                            }
                        });
                        ui.separator();
                        controls::transform_controls(app, ui);
                        ui.horizontal(|ui| {
                            if ui.button(app.tr("forms.prototype.fit")).clicked() {
                                fit(app);
                            }
                            if ui.button(app.tr("forms.prototype.select_all")).clicked() {
                                select_all(app);
                            }
                            let (undo, redo) = app
                                .session
                                .forms_prototype
                                .as_ref()
                                .map(|f| {
                                    let v = f.inspect();
                                    (v["undo"].as_u64().unwrap_or(0) > 0, v["redo"].as_u64().unwrap_or(0) > 0)
                                })
                                .unwrap_or_default();
                            if ui.add_enabled(undo, egui::Button::new(app.tr("forms.prototype.undo"))).clicked() {
                                run(app, "form.prototype.undo", json!({}));
                            }
                            if ui.add_enabled(redo, egui::Button::new(app.tr("forms.prototype.redo"))).clicked() {
                                run(app, "form.prototype.redo", json!({}));
                            }
                        });
                        ui.separator();
                        if !profile {
                            ui.strong(app.tr("forms.prototype.local_refinement"));
                            tools::show(app, ui);
                            if !pipe && !imported {
                                section(app, ui, "forms.prototype.parameters", false, controls::parameters);
                            }
                        }
                        section(app, ui, "forms.prototype.numeric_section", false, controls::numeric);
                        if !profile {
                            section(app, ui, "forms.prototype.topology_section", false, controls::topology);
                        }
                        section(app, ui, "forms.prototype.align_section", false, controls::project);
                        if !profile {
                            section(app, ui, "forms.prototype.symmetry_pivot", false, controls::symmetry);
                        }
                        section(app, ui, "forms.prototype.cad_section", false, controls::output);
                        section(app, ui, "forms.prototype.display_section", false, display);
                        section(app, ui, "forms.prototype.interchange", false, interchange::show);
                        ui.separator();
                        ui.add(
                            egui::Label::new(app.tr(if profile { "forms.prototype.surface_short_help" } else { "forms.prototype.short_help" }))
                                .wrap(),
                        );
                        if app.forms_prototype.setting_pivot {
                            ui.colored_label(t.accent, app.tr("forms.prototype.pivot_help"));
                        }
                        if let Some(error) = &app.forms_prototype.live.error {
                            ui.add(egui::Label::new(egui::RichText::new(error).color(t.warning)).wrap());
                        }
                        if let Some(error) = &app.forms_prototype.context.error {
                            ui.add(egui::Label::new(egui::RichText::new(error).color(t.warning)).wrap());
                        }
                        if let Some((text, _, true)) = &app.status {
                            ui.add(egui::Label::new(egui::RichText::new(text).color(t.error)).wrap());
                        }
                    });
                });
            });
        });
}
fn header(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let busy = app.forms_prototype.pending.is_some() || app.forms_prototype.interchange.is_some() || tool_panel::active(app);
        let r = ui.add_enabled_ui(!busy, |ui| ui_kit::button(ui, app.tr("forms.prototype.finish"), ui_kit::Button::Primary)).inner;
        crate::scenario::publish_handle("forms_finish", r.rect.center());
        if r.clicked() {
            finish::start(app, ui.ctx());
        }
        let r = ui.button(app.tr("cmd.form.prototype.close"));
        crate::scenario::publish_handle("forms_cancel", r.rect.center());
        if r.clicked() {
            app.forms_prototype.pending = None;
            app.forms_prototype.interchange = None;
            app.forms_prototype.tool_panel = None;
            run(app, "form.prototype.close", json!({}));
            app.ui.show_browser = true;
            app.ui.show_timeline = true;
        }
    });
    if app.forms_prototype.pending.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(app.tr("forms.prototype.fitting"));
        });
    }
    if app.forms_prototype.interchange.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(app.tr("forms.prototype.interchanging"));
        });
    }
    if tool_panel::active(app) {
        return;
    }
    let count = app.forms_prototype.selected.len().to_string();
    if let Some(form) = &app.session.forms_prototype
        && !form.parts.is_empty()
    {
        ui.label(crate::i18n::format("forms.prototype.parts_count", &[("count", &form.parts.len().to_string())]));
    }
    ui.horizontal(|ui| {
        if count == "1" {
            ui.label(app.tr("forms.prototype.one_point_selected"));
        } else {
            ui.label(crate::i18n::format("forms.prototype.selected_count", &[("count", &count)]));
        }
        if live_cad::busy(app) || context::busy(app) {
            ui.spinner().on_hover_text(app.tr("forms.prototype.fitting"));
        }
    });
}
fn section(app: &mut SolveApp, ui: &mut egui::Ui, key: &'static str, open: bool, content: fn(&mut SolveApp, &mut egui::Ui)) {
    let title = app.tr(key).to_owned();
    let r = egui::CollapsingHeader::new(title).id_salt(key).default_open(open).show(ui, |ui| content(app, ui));
    crate::scenario::publish_handle(key, r.header_response.rect.center());
}
fn selection(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.label(app.tr("forms.prototype.selection_filter"));
    ui.horizontal(|ui| {
        for (mode, key, handle) in [
            (Pick::All, "forms.prototype.all", "forms_pick_all"),
            (Pick::Vertex, "forms.prototype.vertices", "forms_pick_vertex"),
            (Pick::Edge, "forms.prototype.edges", "forms_pick_edge"),
            (Pick::Face, "forms.prototype.faces", "forms_pick_face"),
        ] {
            let r = ui.selectable_label(app.forms_prototype.pick == mode, app.tr(key));
            crate::scenario::publish_handle(handle, r.rect.center());
            if r.clicked() {
                app.forms_prototype.pick = mode;
                app.forms_prototype.selected.clear();
                app.forms_prototype.elements.clear();
                app.forms_prototype.selection = Default::default();
                app.forms_prototype.pivot = None;
            }
        }
    });
}
fn primitive(app: &mut SolveApp, ui: &mut egui::Ui) {
    let options: Vec<_> = PRIMITIVES.iter().map(|(_, key)| app.tr(key).to_owned()).collect();
    let label = app.tr("forms.prototype.primitive_label").to_owned();
    let r = ui_kit::choice_row(
        ui,
        &label,
        "forms_primitive_kind",
        &mut app.forms_prototype.primitive,
        &options.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    crate::scenario::publish_handle("forms_primitive", r.rect.center());
    let r = ui.button(app.tr("forms.prototype.create_primitive"));
    crate::scenario::publish_handle("forms_create_primitive", r.rect.center());
    if r.clicked() {
        let kind = PRIMITIVES.get(app.forms_prototype.primitive).map_or("box", |p| p.0);
        if run(app, "form.prototype.create", json!({"primitive":kind})) {
            opened(app);
        }
    }
    let r = ui.button(app.tr("forms.prototype.surface_create"));
    crate::scenario::publish_handle("forms_create_surface", r.rect.center());
    if r.clicked() {
        tool_panel::start(app, tool_panel::Mode::Surface);
    }
    let r = ui.button(app.tr("forms.prototype.pipe_create"));
    crate::scenario::publish_handle("forms_create_pipe", r.rect.center());
    if r.clicked() {
        tool_panel::start(app, tool_panel::Mode::Pipe);
    }
}
fn pipe_settings(app: &mut SolveApp, ui: &mut egui::Ui) {
    if let Some(pipe) = app.session.forms_prototype.as_ref().and_then(|f| f.pipe.as_ref()) {
        let source = match &pipe.source {
            solidcraft_engine::doc::forms::PipeSource::Sketch { sketch, .. } => {
                app.session.doc.feature(sketch.0).map(|f| f.name.clone()).unwrap_or_else(|| app.tr("forms.prototype.pipe_source_required").to_owned())
            }
            solidcraft_engine::doc::forms::PipeSource::Edges { body, .. } => body.0.clone(),
        };
        ui.label(crate::i18n::format("forms.prototype.pipe_summary", &[("source", &source)]));
        ui.add(egui::Label::new(app.tr(if pipe.follow_path { "forms.prototype.pipe_parametric" } else { "forms.prototype.pipe_sculpted" })).wrap());
    }
    let r = ui.button(app.tr("forms.prototype.pipe_settings"));
    crate::scenario::publish_handle("forms_pipe_settings", r.rect.center());
    if r.clicked() {
        tool_panel::start(app, tool_panel::Mode::Pipe);
    }
}
fn surface_settings(app: &mut SolveApp, ui: &mut egui::Ui) {
    if let Some(profile) = app.session.forms_prototype.as_ref().and_then(|f| f.profile.as_ref()) {
        ui.label(crate::i18n::format(
            "forms.prototype.surface_summary",
            &[("body", profile.source.body().as_ref()), ("u", &profile.segments[0].to_string()), ("v", &profile.segments[1].to_string())],
        ));
    }
    ui.add(egui::Label::new(app.tr("forms.prototype.surface_fixed_grid")).wrap());
    if app.session.forms_prototype.as_ref().is_some_and(|form| form.profile.as_ref().is_some_and(|p| p.partition) && form.cage.faces.len() > 1)
        && live_cad::report(app).is_some_and(|report| report.patches == 1)
    {
        ui.add(egui::Label::new(app.tr("forms.prototype.surface_single_face")).wrap());
    }
    let r = ui.button(app.tr("forms.prototype.surface_settings"));
    crate::scenario::publish_handle("forms_surface_settings", r.rect.center());
    if r.clicked() {
        tool_panel::start(app, tool_panel::Mode::Surface);
    }
}
fn display(app: &mut SolveApp, ui: &mut egui::Ui) {
    for (key, value) in [
        ("forms.prototype.show_points", &mut app.forms_prototype.points),
        ("forms.prototype.cage", &mut app.forms_prototype.cage),
        ("forms.prototype.wire", &mut app.forms_prototype.wire),
    ] {
        let label = crate::i18n::tr(key);
        ui.checkbox(value, label);
    }
    let mut level = app.session.forms_prototype.as_ref().map_or(3, |f| f.levels);
    let old = level;
    let label = app.tr("forms.prototype.smoothness").to_owned();
    ui_kit::choice_row(ui, &label, "forms_preview_detail", &mut level, &["0", "1", "2", "3", "4"]);
    if level != old {
        run(app, "form.prototype.quality", json!({"levels":level}));
    }
}
