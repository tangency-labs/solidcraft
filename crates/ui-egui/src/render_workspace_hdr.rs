//! HDR selection is local until the engine captures its verified bytes into the design.
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_engine::doc::render_resources::HdrId;

use super::{ImageMethod, cpu, studio};
use crate::{SolveApp, ui_kit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Cpu,
    Gpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub(super) struct Metadata {
    pub sha256: HdrId,
    pub width: u32,
    pub height: u32,
    pub encoded_bytes: u64,
    pub primaries: Option<[f64; 8]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum Input {
    #[default]
    Empty,
    FilePending {
        path: String,
    },
    FileCaptured {
        path: String,
        metadata: Metadata,
    },
    EmbeddedResource {
        resource: HdrId,
    },
}

impl Input {
    pub fn resource(&self) -> Option<HdrId> {
        match self {
            Self::FileCaptured { metadata, .. } => Some(metadata.sha256),
            Self::EmbeddedResource { resource } => Some(*resource),
            Self::Empty | Self::FilePending { .. } => None,
        }
    }

    pub fn embedded(&self) -> bool {
        matches!(self, Self::EmbeddedResource { .. })
    }

    fn path(&self) -> &str {
        match self {
            Self::FilePending { path } | Self::FileCaptured { path, .. } => path,
            Self::Empty | Self::EmbeddedResource { .. } => "",
        }
    }
}

pub(super) fn input(app: &SolveApp, target: Target) -> &Input {
    match target {
        Target::Cpu => &app.render_workspace.view.cpu.hdr,
        Target::Gpu => &app.render_workspace.view.studio.hdr,
    }
}

fn set_input(app: &mut SolveApp, target: Target, input: Input) {
    match target {
        Target::Cpu => {
            app.render_workspace.view.cpu.hdr = input;
            app.render_workspace.view.cpu.checked = None;
        }
        Target::Gpu => app.render_workspace.view.studio.hdr = input,
    }
    app.render_workspace.view.dirty = true;
}

pub(super) fn active(app: &SolveApp) -> Option<Target> {
    let view = &app.render_workspace.view;
    match view.method {
        ImageMethod::CpuRaytrace if view.cpu.environment == cpu::Environment::Hdr => Some(Target::Cpu),
        ImageMethod::GpuRaytrace if view.studio.environment == studio::Environment::Hdr => Some(Target::Gpu),
        _ => None,
    }
}

fn valid_path(path: &str) -> bool {
    !path.trim().is_empty() && path.len() <= 4096 && !path.chars().any(char::is_control)
}

/// No host is consulted and no document mutation occurs. A failed inspection keeps the input.
pub(super) fn inspect(app: &mut SolveApp, target: Target, path: String) -> Result<(), String> {
    if !valid_path(&path) {
        return Err(app.tr("render.studio_hdr_required").to_string());
    }
    let reply = app.run("render.hdr.inspect", json!({"path":path}))?;
    let metadata: Metadata = serde_json::from_value(reply).map_err(|error| error.to_string())?;
    set_input(app, target, Input::FileCaptured { path, metadata });
    Ok(())
}

/// The picker belongs to this document revision, setup, method and input selection.
/// Cancelling or answering after any of those changed cannot replace a newer selection.
pub(super) fn pick(app: &mut SolveApp, target: Target) {
    let document = std::sync::Arc::downgrade(&app.session.doc);
    let revision = app.session.revision;
    let method = app.render_workspace.view.method;
    let name = app.render_workspace.view.name.clone();
    let before = input(app, target).clone();
    let selected = active(app);
    app.pick_open_for(&["hdr"], move |app, path| {
        let Some(path) = path else { return };
        if app.session.revision != revision
            || !std::sync::Weak::ptr_eq(&document, &std::sync::Arc::downgrade(&app.session.doc))
            || app.render_workspace.view.method != method
            || app.render_workspace.view.name != name
            || active(app) != selected
            || input(app, target) != &before
        {
            return;
        }
        if let Err(error) = inspect(app, target, path) {
            app.set_status(error, true);
        }
    });
}

/// Runtime requests use the same expected digest in Check and Start. Embedded resources
/// remain usable with temporary body bindings without ever reconstructing an external path.
pub(super) fn request(app: &SolveApp, target: Target) -> Result<Value, String> {
    match input(app, target) {
        Input::FileCaptured { path, metadata } if valid_path(path) => {
            let mut value = json!({"path":path,"expected_sha256":metadata.sha256});
            if target == Target::Gpu {
                value["kind"] = json!("hdr");
            }
            Ok(value)
        }
        Input::EmbeddedResource { resource } => {
            if app.session.doc.render.resources.get(resource).is_none() {
                return Err(app.tr("render.hdr_resource_missing").to_string());
            }
            let mut value = json!({"resource":resource});
            if target == Target::Gpu {
                value["kind"] = json!("embedded_hdr");
            }
            Ok(value)
        }
        Input::Empty | Input::FilePending { .. } | Input::FileCaptured { .. } => Err(app.tr("render.hdr_inspect_required").to_string()),
    }
}

fn metadata(app: &SolveApp, target: Target) -> Option<Metadata> {
    match input(app, target) {
        Input::FileCaptured { metadata, .. } => Some(*metadata),
        Input::EmbeddedResource { resource } => {
            let entry = app.session.doc.render.resources.get(resource)?;
            let info = entry.info();
            Some(Metadata { sha256: *resource, width: info.width, height: info.height, encoded_bytes: info.encoded_bytes, primaries: info.primaries })
        }
        Input::Empty | Input::FilePending { .. } => None,
    }
}

pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui, target: Target) {
    if input(app, target).embedded() {
        ui.label(app.tr("render.hdr_embedded"));
        ui.small(app.tr("render.hdr_embedded_help"));
    } else {
        let mut path = input(app, target).path().to_string();
        let hint = app.tr("render.studio_hdr_required").to_string();
        let field = ui_kit::bounded_cell_editor(ui, ui.id().with("hdr_source_path"), &mut path, &hint, false, 4096);
        if field.changed() {
            set_input(app, target, Input::FilePending { path });
        }
        let captured = matches!(input(app, target), Input::FileCaptured { .. });
        ui.small(app.tr(if captured { "render.hdr_file_captured" } else { "render.hdr_inspect_required" }));
        if ui_kit::button(ui, app.tr("render.hdr_inspect"), ui_kit::Button::Enabled(valid_path(input(app, target).path()))).clicked() {
            let path = input(app, target).path().to_string();
            if let Err(error) = inspect(app, target, path) {
                app.set_status(error, true);
            }
        }
    }
    if ui_kit::button(ui, app.tr("render.studio_hdr_browse"), ui_kit::Button::Normal).clicked() {
        pick(app, target);
    }
    if let Some(metadata) = metadata(app, target) {
        ui.label(format!("{} × {} px · {:.2} MiB", metadata.width, metadata.height, metadata.encoded_bytes as f64 / 1_048_576.0));
        let primaries = if metadata.primaries == Some(solidcraft_engine::doc::render_resources::SRGB_D65_PRIMARIES) {
            "render.hdr_primaries_srgb"
        } else if metadata.primaries.is_some() {
            "render.hdr_primaries_declared"
        } else {
            "render.hdr_primaries_undeclared"
        };
        ui.small(app.tr(primaries));
        let title = app.tr("render.hdr_details").to_string();
        ui_kit::section(ui, "hdr_source_details", &title, false, |ui| {
            ui.label(format!("SHA-256: {}", metadata.sha256));
            if let Some([rx, ry, gx, gy, bx, by, wx, wy]) = metadata.primaries {
                ui.label(format!("CIE xy · R ({rx:.6}, {ry:.6}) · G ({gx:.6}, {gy:.6})"));
                ui.label(format!("CIE xy · B ({bx:.6}, {by:.6}) · W ({wx:.6}, {wy:.6})"));
            }
        });
    }
}
