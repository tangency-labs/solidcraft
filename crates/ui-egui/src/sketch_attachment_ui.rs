//! Reviewable sketch-plane rebinding. Saved lost planes are repair guides, never model geometry.

use egui::RichText;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::face_anchor::plane_face_anchor;
use solidcraft_engine::doc::{EmbossFace, FaceAttachmentStatus, FeatureKind, PlaneRef, mat_inverse};
use solidcraft_engine::geom::{Plane, Vec3};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit;

#[derive(Clone, Debug)]
pub struct Draft {
    pub sketch: u64,
    pub candidate: Option<Value>,
    pub label: String,
    pub error: Option<String>,
    pub author_path: Option<Vec<u64>>,
}

impl Draft {
    pub fn new(app: &SolveApp, sketch: u64) -> Self {
        let author_path = app.session.doc.feature(sketch).and_then(|feature| match &feature.kind {
            FeatureKind::Sketch { plane, .. } if plane_face_anchor(plane).is_some() => {
                plane_face_anchor(plane).map(|anchor| anchor.author_path.clone())
            }
            _ if feature.component == app.session.active_component => app.session.active_occurrence_path.clone(),
            _ => None,
        });
        Self { sketch, candidate: None, label: String::new(), error: None, author_path }
    }

    fn choose(&mut self, plane: Value, label: String) {
        self.candidate = Some(plane);
        self.label = label;
        self.error = None;
    }

    pub fn commands(&self) -> Option<Vec<(String, Value)>> {
        let mut params = json!({"sketch":self.sketch,"plane":self.candidate.as_ref()?});
        if let Some(path) = &self.author_path {
            params["author_path"] = json!(path);
        }
        Some(vec![("sketch.redefine".into(), params)])
    }

    pub fn choose_saved_fixed(&mut self, app: &SolveApp, world: Plane) -> Result<(), String> {
        let doc = app.session.placed_document();
        let owner = doc.feature(self.sketch).ok_or("Sketch no longer exists")?.component;
        let path = self.author_path.clone().unwrap_or_else(|| doc.first_path(owner));
        if self.author_path.is_none()
            && path.iter().any(|id| {
                doc.occurrences
                    .iter()
                    .find(|o| o.id == *id)
                    .is_none_or(|selected| doc.occurrences.iter().filter(|o| o.component == selected.component).count() != 1)
            })
        {
            return Err("Select an exact author placement before converting this plane".into());
        }
        let frame = EmbossFace::component_frame(&doc, owner, &path).map_err(|error| error.to_string())?;
        mat_inverse(&frame).ok_or("Sketch author frame is invalid")?;
        self.author_path = Some(path);
        // Public commands take world geometry. Session maps Custom planes into
        // this exact author placement once, including redefine's saved path.
        self.choose(fixed_plane(world), "Fixed plane: independent of the source body".into());
        Ok(())
    }
}

pub fn clear(app: &mut SolveApp) {
    app.viewport.dialog_rect = None;
    app.tree.redefine = None;
    app.tree.redefine_draft = None;
}

pub fn commands(app: &SolveApp) -> Option<Vec<(String, Value)>> {
    let draft = app.tree.redefine_draft.as_ref()?;
    (app.tree.redefine == Some(draft.sketch)).then(|| draft.commands()).flatten()
}

fn attachment(app: &SolveApp, sketch: u64) -> FaceAttachmentStatus {
    app.session.placed_document().sketch_face_attachment(sketch, &app.session.model)
}

fn fixed_plane(plane: Plane) -> Value {
    json!({"origin":plane.origin,"x_dir":plane.x,"y_dir":plane.y})
}

pub fn face_plane(app: &SolveApp, selection: &Sel) -> Option<Value> {
    match selection {
        Sel::Face { body, index, point } => {
            let doc = app.session.placed_document();
            let (_, _, _, path, _) = doc.body_frames_with_source(&app.session.model.state()).into_iter().find(|(world, _, _, _, _)| world == body)?;
            Some(json!({"face":point,"body":body,"index":index,"path":path}))
        }
        _ => None,
    }
}

/// What a sketch's plane is, for the Redefine Sketch Plane panel: `XY`, a construction plane's
/// name, or the plane it's offset from, turned from or flipped from (#5103).
pub(crate) fn plane_description(doc: &solidcraft_engine::doc::Document, plane: &PlaneRef) -> String {
    fn describe(doc: &solidcraft_engine::doc::Document, plane: &PlaneRef, depth: usize) -> String {
        let base = |b: &PlaneRef| if depth < 8 { describe(doc, b, depth + 1) } else { crate::i18n::tr("sketch_panel.plane_fixed").to_owned() };
        match plane {
            PlaneRef::Origin { name } => crate::i18n::format("sketch_panel.plane_origin", &[("plane", name)]),
            PlaneRef::Construction { feature } => {
                doc.feature(feature.0).map(|f| f.name.clone()).unwrap_or_else(|| crate::i18n::tr("sketch_panel.plane_fixed").to_owned())
            }
            PlaneRef::Offset { base: b, distance } => {
                crate::i18n::format("sketch_panel.plane_offset", &[("plane", &base(b)), ("distance", distance)])
            }
            PlaneRef::AtAngle { base: b, angle, .. } => crate::i18n::format("sketch_panel.plane_angle", &[("plane", &base(b)), ("angle", angle)]),
            PlaneRef::Flipped { base: b, .. } => crate::i18n::format("sketch_panel.plane_flipped", &[("plane", &base(b))]),
            _ => crate::i18n::tr("sketch_panel.plane_fixed").to_owned(),
        }
    }
    describe(doc, plane, 0)
}

fn face_label(app: &SolveApp, selection: &Sel) -> String {
    let Sel::Face { body, .. } = selection else { return String::new() };
    let placed = app.session.placed_document();
    if let Some((_, source, _, path, _)) =
        placed.body_frames_with_source(&app.session.model.state()).into_iter().find(|(world, _, _, _, _)| world == body)
    {
        format!("Face on {source} / {}", path_label(app, &path))
    } else {
        format!("Face on {body}")
    }
}

fn path_label(app: &SolveApp, path: &[u64]) -> String {
    if path.is_empty() {
        return "Root".into();
    }
    (1..=path.len())
        .map(|end| app.session.occurrence_placement(&path[..end]).map(|placement| placement.name).unwrap_or_else(|_| "Missing occurrence".into()))
        .collect::<Vec<_>>()
        .join(" / ")
}

pub fn panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(sketch) = app.tree.redefine else { return };
    if app.session.doc.feature(sketch).is_none() {
        clear(app);
        return;
    }
    let mut draft = app.tree.redefine_draft.take().filter(|draft| draft.sketch == sketch).unwrap_or_else(|| Draft::new(app, sketch));
    let _language = crate::i18n::scope(&app.preferences.language);
    let status = attachment(app, sketch);
    let name = app.session.doc.feature(sketch).map(|feature| feature.name.as_str()).unwrap_or("Sketch");
    let selected = app.session.selection.iter().find(|selection| matches!(selection, Sel::Face { .. })).cloned();
    let mut apply = false;
    let mut cancel = false;
    let t = Tokens::get();
    let viewport = crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    if !viewport.is_finite() || !viewport.is_positive() {
        app.tree.redefine_draft = Some(draft);
        return;
    }
    let side = if crate::sketch_palette::editing(app) && crate::sketch_palette::fits_beside_a_dialog(app, ctx) {
        crate::frame::Side::Left
    } else {
        crate::frame::Side::Right
    };
    let top = crate::shell::panel_top(app, viewport, viewport.bottom(), side);
    let docked = egui::pos2(side.anchor_x(viewport), top);
    let moved = crate::frame::dock_offset(ctx, "sc_redefine");
    let drag_bounds = crate::shell::tool_drag_rect(app, viewport);
    let anchor = crate::frame::offset_anchor(docked, moved, drag_bounds, side);
    let frame = ui_kit::Panel::feature_frame(ctx, app.ui.layout, side);
    let spec = solidcraft_engine::find_command("sketch.redefine");
    let command = spec.as_ref().map_or("", |s| crate::i18n::tr_label(s.label));
    let title = crate::i18n::format("sketch_panel.redefine_title", &[("command", command), ("name", name)]);
    let panel = ui_kit::Panel::new(&title, egui::Id::new("sc_redefine"), anchor, drag_bounds, side, frame);
    let response = panel.show(ctx, None, |ui| {
        let (closed, action) = panel.feature(
            ui,
            ui_kit::FeatureOptions {
                icon: spec.as_ref().map_or("", |s| s.icon),
                title: &title,
                drag_key: "sc_redefine",
                moved,
                docked,
                apply_label: None,
            },
            false,
            draft.candidate.is_some() && app.preview.error.is_none(),
            |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui_kit::text(
                    ui,
                    crate::i18n::tr_label("Choose a new plane. Sketch geometry and dimensions stay unchanged in its two-dimensional coordinates."),
                    ui_kit::Text::Hint("tool"),
                );
                let saved_plane = match &status {
                    FaceAttachmentStatus::Healthy { plane, target, path } => {
                        ui_kit::row(ui, "Attachment", |ui| {
                            ui.label(crate::i18n::format("sketch_panel.attached", &[("body", &target.body), ("path", &path_label(app, path))]));
                        });
                        Some(*plane)
                    }
                    FaceAttachmentStatus::Lost { message, repair_plane } => {
                        ui.label(RichText::new(crate::i18n::tr_label("Face attachment lost")).color(t.error));
                        ui.label(message);
                        if repair_plane.is_some() {
                            ui.label(crate::i18n::tr_label("The dashed sketch is a saved plane guide. Select a new face to repair the attachment."));
                        } else {
                            ui.label(crate::i18n::tr_label(
                                "The saved author occurrence is missing. Select a valid author placement before rebinding.",
                            ));
                        }
                        *repair_plane
                    }
                    FaceAttachmentStatus::Legacy { plane } => {
                        ui.label(crate::i18n::tr_label("Legacy face attachment. Rebinding to a certified face creates an exact attachment."));
                        *plane
                    }
                    FaceAttachmentStatus::Sheet { plane } => {
                        ui.label(crate::i18n::tr("sketch_panel.sheet_attachment"));
                        *plane
                    }
                    FaceAttachmentStatus::Fixed { plane } => {
                        // Name the plane it's on now: an origin or construction plane (#5103).
                        if let Some(FeatureKind::Sketch { plane: on, .. }) = app.session.doc.feature(sketch).map(|f| &f.kind) {
                            let now = plane_description(&app.session.doc, on);
                            ui_kit::row(ui, "Current plane", |ui| {
                                ui.label(now);
                            });
                        }
                        ui.label(crate::i18n::tr_label("Sketch plane has no body-face attachment."));
                        *plane
                    }
                };
                ui_kit::divider(ui, "attachment");
                if matches!(&status, FaceAttachmentStatus::Lost { repair_plane: None, .. }) {
                    let author = app.session.doc.feature(sketch).map(|feature| feature.component);
                    let available = author == Some(app.session.active_component);
                    if ui
                        .add_enabled_ui(available, |ui| {
                            ui_kit::button(ui, crate::i18n::tr_label("Use active author placement"), ui_kit::Button::Normal)
                        })
                        .inner
                        .clicked()
                    {
                        draft.author_path = Some(app.session.active_occurrence_path.clone().unwrap_or_default());
                        draft.error = None;
                    }
                    if available && let Some(path) = &draft.author_path {
                        ui.label(crate::i18n::format("sketch_panel.author", &[("path", &path_label(app, path))]));
                    }
                }
                ui_kit::row(ui, "Plane", |ui| {
                    let text = if draft.candidate.is_some() { draft.label.as_str() } else { crate::i18n::tr("dialog.select") };
                    let response = ui_kit::selection_field(ui, text, ui_kit::Selection { active: true, filled: draft.candidate.is_some() });
                    if response
                        .on_hover_text(crate::i18n::tr_label(
                            "Choose a new plane. Sketch geometry and dimensions stay unchanged in its two-dimensional coordinates.",
                        ))
                        .clicked()
                        && let Some(selection) = &selected
                        && let Some(plane) = face_plane(app, selection)
                    {
                        draft.choose(plane, face_label(app, selection));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    // The command takes the plane's id; the button and the choice show its name.
                    for plane in crate::prefs::OriginPlaneNames::PLANES {
                        let label = app.preferences.origin_plane_names.label(plane);
                        if ui_kit::button(ui, crate::i18n::format("sketch_panel.use_plane", &[("plane", label)]), ui_kit::Button::Normal).clicked() {
                            draft.choose(json!(plane), label.into());
                        }
                    }
                });
                for feature in &app.session.doc.features {
                    if matches!(feature.kind, FeatureKind::ConstructionPlane { .. })
                        && ui_kit::button(ui, &feature.name, ui_kit::Button::Normal).clicked()
                    {
                        draft.choose(json!(feature.name), feature.name.clone());
                    }
                }
                if ui
                    .add_enabled_ui(selected.is_some(), |ui| {
                        ui_kit::button(ui, crate::i18n::tr_label("Reattach selected face"), ui_kit::Button::Normal)
                    })
                    .inner
                    .clicked()
                    && let Some(selection @ Sel::Face { .. }) = &selected
                    && let Some(plane) = face_plane(app, selection)
                {
                    let label = face_label(app, selection);
                    draft.choose(plane, label);
                }
                if ui
                    .add_enabled_ui(saved_plane.is_some(), |ui| {
                        ui_kit::button(ui, crate::i18n::tr_label("Use saved fixed plane"), ui_kit::Button::Normal)
                    })
                    .inner
                    .clicked()
                    && let Some(plane) = saved_plane
                    && let Err(error) = draft.choose_saved_fixed(app, plane)
                {
                    draft.error = Some(error);
                }
                if ui
                    .add_enabled_ui(selected.is_some(), |ui| {
                        ui_kit::button(ui, crate::i18n::tr_label("Use selected face as fixed plane"), ui_kit::Button::Normal)
                    })
                    .inner
                    .clicked()
                    && let Some(selection) = &selected
                    && let Some(mut plane) = face_plane(app, selection)
                {
                    plane["fixed"] = json!(true);
                    draft.choose(plane, "Fixed plane: independent of the selected body".into());
                }
                if draft.candidate.is_some() {
                    ui_kit::divider(ui, "attachment");
                    ui_kit::row(ui, "Preview", |ui| {
                        ui.label(crate::i18n::tr_label(&draft.label));
                    });
                }
                if let Some(error) = draft.error.as_ref().or(app.preview.error.as_ref()) {
                    ui.label(RichText::new(error).color(t.error));
                }
                ui_kit::ComputeStatus { text: None, busy: app.preview.busy, applying: false }
            },
        );
        cancel |= closed || action == ui_kit::Action::Cancel;
        apply |= action == ui_kit::Action::Ok;
    });
    app.viewport.dialog_rect = response.map(|r| r.response.rect);
    if let Some(rect) = app.viewport.dialog_rect {
        crate::scenario::publish_rect("dialog:outer", rect);
        crate::scenario::publish_rect("dialog:card", rect);
    }
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        cancel = true;
        app.esc_handled = true;
    }
    if apply && let Some(commands) = draft.commands() {
        for (command, params) in commands {
            match app.run(&command, params) {
                Ok(_) => cancel = true,
                Err(error) => draft.error = Some(error),
            }
        }
    }
    if cancel {
        clear(app);
    } else {
        app.tree.redefine_draft = Some(draft);
    }
}

/// Only a lost reference's explicit saved author frame supplies a guide. It is
/// absent from ModelState, selection, snapping and all downstream evaluation.
pub fn lost_guide(app: &SolveApp) -> Option<(Plane, &solidcraft_engine::sketch::Sketch)> {
    let sketch = app.tree.redefine?;
    let FaceAttachmentStatus::Lost { repair_plane: Some(plane), .. } = attachment(app, sketch) else { return None };
    let FeatureKind::Sketch { sketch, .. } = &app.session.doc.feature(sketch)?.kind else { return None };
    Some((plane, sketch))
}

pub fn guide_lines(app: &SolveApp) -> Vec<Vec<Vec3>> {
    let Some((plane, sketch)) = lost_guide(app) else { return Vec::new() };
    (0..sketch.curves.len())
        .flat_map(|index| sketch.segs(index))
        .map(|segment| segment.polyline(0.05).into_iter().map(|point| plane.to_world(point)).collect())
        .collect()
}

pub fn display_guide_lines(app: &SolveApp, proj: &crate::viewport::Proj) -> Vec<Vec<Vec3>> {
    let Some((plane, sketch)) = lost_guide(app) else { return Vec::new() };
    let mut budget = solidcraft_engine::render::SamplingBudget::new(65_536, 262_144);
    (0..sketch.curves.len())
        .flat_map(|index| crate::sketch_display::curve_with_budget(sketch, index, &plane, proj, app.viewport.pixels_per_point(), &mut budget).parts)
        .collect()
}
