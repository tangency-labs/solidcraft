//! The Appearance dialog: library swatches (named looks), the looks in this design, a colour
//! picker with opacity, and the objects to apply to (bodies, or faces). A swatch dragged onto a
//! body or face in the viewport, or onto a body's row in the Browser, applies at once; Apply
//! (and OK) gives the picked objects the current look.

use egui::Color32;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::pbr_appearance::PbrParameters;

use crate::SolveApp;
use crate::dialogs::{Kind, combo};
use crate::selection::{BODIES, FACES, SelInput};
use crate::ui_kit;
use crate::viewport::Hit;

/// A named look: colour and opacity.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub preset: Option<PresetMetadata>,
    pub name: String,
    pub category: String,
    pub color: [u8; 3],
    pub opacity: f64,
    pub pbr: Option<PbrParameters>,
}

/// Transient library admission metadata, never saved as a second catalog.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetMetadata {
    pub id: String,
    pub available: bool,
    pub requirements: Vec<String>,
    pub preview_note: String,
}

fn library_look(x: &Value) -> Option<Look> {
    let preset = if let Some(id) = x.get("id") {
        let id = id.as_str()?;
        let note = x.get("preview_note")?.as_str()?;
        let requirements = x.get("requirements")?.as_array()?;
        if id.is_empty() || id.len() > 128 || note.len() > 1024 || requirements.len() > 8 {
            return None;
        }
        let requirements = requirements
            .iter()
            .map(|value| {
                let value = value.as_str()?;
                (value.len() <= 128).then(|| value.to_string())
            })
            .collect::<Option<Vec<_>>>()?;
        Some(PresetMetadata { id: id.to_string(), available: x.get("available")?.as_bool()?, requirements, preview_note: note.to_string() })
    } else {
        None
    };
    Some(Look {
        preset,
        name: x.get("name")?.as_str()?.to_string(),
        category: match x.get("category").and_then(Value::as_str).unwrap_or("Library") {
            "wood" => "Wood",
            "metal" => "Metal",
            "plastic" => "Plastic",
            "glass" => "Glass",
            "solar" => "Solar",
            "regolith" => "Regolith",
            other => other,
        }
        .to_string(),
        color: parse_color(x.get("color")?)?,
        opacity: x.get("opacity").and_then(Value::as_f64).unwrap_or(1.0),
        pbr: x.get("pbr").and_then(|p| serde_json::from_value(p.clone()).ok()),
    })
}

fn choose_look(k: &mut Ap, look: &Look) -> Result<(), String> {
    if let Some(preset) = &look.preset
        && !preset_available(look)
    {
        return Err(format!("{}: {} ({})", look.name, preset.preview_note, preset.requirements.join(", ")));
    }
    k.color = look.color;
    k.opacity = look.opacity;
    k.pbr = look.pbr;
    k.look = Some(look.name.clone());
    k.preset_id = look.preset.as_ref().map(|p| p.id.clone());
    k.note = look.preset.as_ref().map(|p| p.preview_note.clone()).filter(|note| !note.is_empty());
    k.remove_override = false;
    Ok(())
}

fn preset_available(look: &Look) -> bool {
    look.preset.as_ref().is_none_or(|preset| {
        preset.available
            && preset.requirements.len() == 1
            && preset.requirements.first().is_some_and(|requirement| requirement == "opaque_pbr")
            && look.pbr.is_some()
            && look.opacity == 1.0
    })
}

/// Editing leaves a literal owned label, with no live recipe selection.
fn detach_preset(k: &mut Ap) {
    k.preset_id = None;
}

fn matches_library_filter(look: &Look, category: Option<&str>, query: &str, only_available: bool) -> bool {
    category.is_none_or(|category| look.category == category)
        && (look.name.to_lowercase().contains(query) || look.preset.as_ref().is_some_and(|p| p.id.to_lowercase().contains(query)))
        && (!only_available || preset_available(look))
}

/// Built-in looks, used until the engine offers a library (`appearance.library`).
pub(crate) fn builtin() -> Vec<Look> {
    let l =
        |cat: &str, name: &str, c: [u8; 3], o: f64| Look { preset: None, name: name.into(), category: cat.into(), color: c, opacity: o, pbr: None };
    vec![
        l("Metal", "Aluminum - Satin", [196, 199, 204], 1.0),
        l("Metal", "Steel - Brushed", [160, 164, 170], 1.0),
        l("Metal", "Stainless Steel", [182, 186, 190], 1.0),
        l("Metal", "Brass", [196, 160, 80], 1.0),
        l("Metal", "Copper", [184, 115, 72], 1.0),
        l("Metal", "Chrome", [222, 226, 232], 1.0),
        l("Metal", "Anodized Black", [42, 44, 48], 1.0),
        l("Plastic", "ABS - White", [236, 236, 232], 1.0),
        l("Plastic", "ABS - Black", [34, 34, 36], 1.0),
        l("Plastic", "ABS - Grey", [128, 130, 134], 1.0),
        l("Plastic", "Polycarbonate - Clear", [210, 226, 240], 0.35),
        l("Plastic", "Nylon - Natural", [228, 222, 204], 1.0),
        l("Plastic", "Rubber - Black", [26, 26, 26], 1.0),
        l("Paint", "Paint - Red", [196, 40, 36], 1.0),
        l("Paint", "Paint - Orange", [232, 120, 30], 1.0),
        l("Paint", "Paint - Yellow", [236, 196, 40], 1.0),
        l("Paint", "Paint - Green", [52, 140, 72], 1.0),
        l("Paint", "Paint - Blue", [36, 92, 180], 1.0),
        l("Paint", "Paint - Purple", [112, 64, 156], 1.0),
        l("Wood", "Oak", [190, 150, 100], 1.0),
        l("Wood", "Walnut", [110, 76, 50], 1.0),
        l("Glass", "Glass - Clear", [220, 236, 244], 0.25),
        l("Glass", "Glass - Smoked", [90, 96, 104], 0.5),
    ]
}

/// The library: the engine's when it has one, else the built-in looks.
pub fn library(app: &mut SolveApp) -> Vec<Look> {
    let from_engine = app.session.execute("appearance.library", &json!({})).ok().and_then(|v| {
        let list = v.get("appearances").or(v.get("library")).unwrap_or(&v).as_array()?.clone();
        let looks: Vec<Look> = list.iter().filter_map(library_look).collect();
        (!looks.is_empty()).then_some(looks)
    });
    from_engine.unwrap_or_else(builtin)
}

/// "#rrggbb" or [r, g, b].
pub fn parse_color(v: &Value) -> Option<[u8; 3]> {
    if let Some(s) = v.as_str() {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 {
            return None;
        }
        let byte = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
        return Some([byte(0)?, byte(2)?, byte(4)?]);
    }
    let a = v.as_array()?;
    let c = |i: usize| a.get(i).and_then(Value::as_u64).filter(|x| *x <= 255).map(|x| x as u8);
    Some([c(0)?, c(1)?, c(2)?])
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[derive(Clone, Debug)]
pub struct Ap {
    pub preset_id: Option<String>,
    pub only_available: bool,
    pub local: crate::dialogs_library::LocalLibrary,
    /// Apply to faces instead of bodies.
    pub faces: bool,
    pub color: [u8; 3],
    pub opacity: f64,
    pub pbr: Option<PbrParameters>,
    /// The look picked last (its name), if the colour came from one.
    pub look: Option<String>,
    /// Library category shown (0: all).
    pub category: usize,
    /// Library names can be searched without changing the selected look.
    pub search: String,
    /// A Browser component target, inherited by its bodies unless they have overrides.
    pub component: Option<String>,
    pub remove_override: bool,
    /// The last successful immediate Apply/drop, so OK does not duplicate its undo step.
    pub applied: Option<Value>,
    /// What the last drop or Apply did.
    pub note: Option<String>,
}

/// The dialog for the Appearance command.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "appearance.assign").then(|| {
        let faces = app.session.selection.iter().any(|s| matches!(s, Sel::Face { .. }))
            && !app.session.selection.iter().any(|s| matches!(s, Sel::Body { .. }));
        let state = crate::open_meshing::session_world(&app.session);
        let body_look = |body: &solidcraft_engine::doc::ModelBody| {
            app.session.doc.model_body_look(body).or_else(|| {
                (body.body.paint().is_some() || body.imported.is_some() || body.body.color().is_some()).then(|| {
                    let color =
                        app.session.doc.body_color(body).map(|rgb| rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)).unwrap_or([196, 199, 204]);
                    solidcraft_engine::doc::appearance::Look::custom(color, app.session.doc.model_body_opacity(body))
                })
            })
        };
        let existing = app.session.selection.iter().find_map(|sel| match sel {
            Sel::Body { name } => state.body(name).and_then(body_look),
            Sel::Face { body, index, .. } => state
                .body(body)
                .and_then(|b| app.session.doc.face_colors(b).into_iter().find(|(i, _)| i == index).map(|(_, look)| look).or_else(|| body_look(b))),
            _ => None,
        });
        (
            Kind::Appearance(Ap {
                preset_id: None,
                only_available: false,
                local: Default::default(),
                faces,
                color: existing.as_ref().map_or([196, 199, 204], |l| l.color),
                opacity: existing.as_ref().map_or(1.0, |l| l.opacity),
                pbr: existing.as_ref().and_then(|l| l.pbr),
                look: existing.map(|l| l.name),
                category: 0,
                search: String::new(),
                component: None,
                remove_override: false,
                applied: None,
                note: None,
            }),
            vec![SelInput::new("Objects", if faces { FACES } else { BODIES }, true)],
        )
    })
}

/// The command giving `targets` (bodies or faces) a look.
fn apply_params(targets: &[Sel], color: [u8; 3], opacity: f64, look: Option<&str>, pbr: Option<PbrParameters>) -> Option<Value> {
    let bodies: Vec<String> = targets.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect();
    let faces: Vec<Value> = targets
        .iter()
        .filter_map(|x| if let Sel::Face { body, point, .. } = x { Some(json!({"body": body, "point": [point.x, point.y, point.z]})) } else { None })
        .collect();
    if bodies.is_empty() && faces.is_empty() {
        return None;
    }
    let mut p = json!({"color": hex(color), "pbr": pbr});
    if !bodies.is_empty() {
        p["bodies"] = json!(bodies);
    }
    if !faces.is_empty() {
        p["faces"] = json!(faces);
        // An engine without face looks: their bodies.
        if bodies.is_empty() && solidcraft_engine::find_command("appearance.list").is_none() {
            let mut owners: Vec<String> =
                targets.iter().filter_map(|x| if let Sel::Face { body, .. } = x { Some(body.clone()) } else { None }).collect();
            owners.dedup();
            p["bodies"] = json!(owners);
        }
    }
    p["opacity"] = json!(opacity);
    if let Some(n) = look {
        p["name"] = json!(n);
    }
    Some(p)
}

/// The commands OK runs: the current look on the picked objects (nothing when none are picked).
pub fn commands(k: &Ap, inputs: &[SelInput]) -> Vec<(String, Value)> {
    // Explicit library material imports only copy definitions. Closing their mode
    // must not also assign an unrelated appearance draft to the selected bodies.
    // Remove override already ran immediately, so it also has no second undo step.
    if k.remove_override || k.local.materials {
        return Vec::new();
    }
    let items = inputs.first().map(|i| i.items.as_slice()).unwrap_or(&[]);
    let p = selected_params(k, items);
    if p.as_ref().is_some_and(|p| k.applied.as_ref() == Some(p)) {
        return Vec::new();
    }
    p.map(|p| vec![("appearance.assign".to_string(), p)]).unwrap_or_default()
}

pub(super) fn selected_params(k: &Ap, items: &[Sel]) -> Option<Value> {
    let mut p = if let Some(component) = &k.component {
        Some(json!({"components": [component], "color": hex(k.color), "opacity": k.opacity, "name": k.look}))
    } else {
        apply_params(items, k.color, k.opacity, k.look.as_deref(), k.pbr)
    }?;
    // All entry points apply the complete owned response, including explicit clearing.
    p["pbr"] = json!(k.pbr);
    if let Some(id) = &k.preset_id {
        if let Some(object) = p.as_object_mut() {
            object.remove("name");
        }
        p["appearance"] = json!(id);
    }
    Some(p)
}

/// Removing an override restores the inherited component or material appearance.
fn clear_params(k: &Ap, items: &[Sel]) -> Option<Value> {
    let mut p = selected_params(k, items)?;
    if let Some(obj) = p.as_object_mut() {
        obj.remove("appearance");
        obj.remove("name");
        obj.remove("color");
        obj.remove("opacity");
        obj.remove("pbr");
    }
    Some(p)
}

/// What a swatch dropped now would land on: the face or body under the cursor in the viewport,
/// or the bodies of the Browser row under it.
fn drop_target(app: &SolveApp, faces: bool) -> Vec<Sel> {
    match &app.viewport.hover {
        Some(Hit::Face { body, index, point }) if faces => vec![Sel::Face { body: body.clone(), index: *index, point: *point }],
        Some(Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. }) => vec![Sel::Body { name: body.clone() }],
        _ => app.viewport.hover_bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect(),
    }
}

/// The looks in this design (colour and how many items wear it): the engine's list when it has
/// one, else the body colours.
fn in_design(app: &mut SolveApp) -> Vec<(Look, usize)> {
    if let Ok(v) = app.session.execute("appearance.list", &json!({}))
        && let Some(list) = v.get("in_design").and_then(Value::as_array)
    {
        let count = |x: &Value, k: &str| x.get(k).and_then(Value::as_array).map_or(0, Vec::len);
        return list
            .iter()
            .filter_map(|x| {
                Some((
                    Look {
                        preset: None,
                        name: x.get("name").and_then(Value::as_str).unwrap_or("Custom").to_string(),
                        category: "In This Design".into(),
                        color: parse_color(x.get("color")?)?,
                        opacity: x.get("opacity").and_then(Value::as_f64).unwrap_or(1.0),
                        pbr: x.get("pbr").and_then(|p| serde_json::from_value(p.clone()).ok()),
                    },
                    count(x, "bodies") + count(x, "faces") + count(x, "components"),
                ))
            })
            .collect();
    }
    let mut out: Vec<(Look, usize)> = Vec::new();
    for l in app.session.doc.appearances.bodies.values() {
        let look = Look { preset: None, name: l.name.clone(), category: "In This Design".into(), color: l.color, opacity: l.opacity, pbr: l.pbr };
        match out.iter_mut().find(|(x, _)| *x == look) {
            Some(e) => e.1 += 1,
            None => out.push((look, 1)),
        }
    }
    out
}

fn c32(c: [u8; 3], opacity: f64) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (opacity.clamp(0.0, 1.0) * 255.0) as u8)
}

/// The dialog's rows (after the Objects input); applies drops and Apply at once.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Ap, inputs: &mut [SelInput]) -> bool {
    // Apply To: bodies or faces (the input follows).
    if let Some(component) = &k.component {
        ui_kit::row(ui, "Apply To", |ui| ui_kit::text(ui, &format!("Component: {component}"), ui_kit::Text::Cell));
        if ui_kit::row(ui, "", |ui| ui_kit::button(ui, crate::i18n::tr_label("Select bodies or faces instead"), ui_kit::Button::Normal)).clicked() {
            k.component = None;
        }
    } else {
        let mut mode = usize::from(k.faces);
        ui_kit::row(ui, "Apply To", |ui| combo(ui, "ap_mode", &["Bodies", "Faces"], &mut mode));
        if (mode == 1) != k.faces {
            k.faces = mode == 1;
            if let Some(input) = inputs.first_mut() {
                *input = SelInput::new("Objects", if k.faces { FACES } else { BODIES }, true);
            }
        }
    }
    ui_kit::row(ui, "Colour", |ui| {
        let mut rgb = k.color;
        if ui_kit::color_field(ui, &mut rgb).changed() {
            k.color = rgb;
            detach_preset(k);
            k.look = None;
            k.remove_override = false;
        }
        ui_kit::text(ui, &k.look.clone().unwrap_or_else(|| hex(k.color)), ui_kit::Text::MonoCell);
    });
    ui_kit::row(ui, "Opacity", |ui| {
        if ui.add_enabled_ui(k.pbr.is_none(), |ui| ui_kit::slider(ui, &mut k.opacity, 0.0..=1.0, Default::default())).inner.changed() {
            detach_preset(k);
            k.remove_override = false;
        }
    });
    let mut enabled = k.pbr.is_some();
    let toggle = ui_kit::row(ui, app.tr("appearance.surface_response"), |ui| ui_kit::checkbox(ui, app.tr("appearance.pbr_opaque"), &mut enabled))
        .on_hover_text(app.tr("appearance.pbr_help"));
    crate::scenario::publish_handle("appearance:pbr", toggle.rect.center());
    if toggle.changed() {
        detach_preset(k);
        k.pbr = enabled.then(PbrParameters::default);
        if enabled {
            k.opacity = 1.0;
        }
        k.remove_override = false;
    }
    if let Some(parameters) = k.pbr {
        let mut metallic = parameters.metallic();
        let mut roughness = parameters.roughness();
        let mut ior = parameters.dielectric_ior();
        for (label, handle, value, range) in [
            ("appearance.metallic", "appearance:metallic", &mut metallic, 0.0..=1.0),
            ("appearance.roughness", "appearance:roughness", &mut roughness, 0.045..=1.0),
            ("appearance.dielectric_ior", "appearance:ior", &mut ior, 1.0..=3.0),
        ] {
            let response = ui_kit::row(ui, app.tr(label), |ui| ui_kit::scalar_field(ui, value, range));
            crate::scenario::publish_handle(handle, response.rect.center());
            if response.changed() {
                detach_preset(k);
                k.remove_override = false;
            }
        }
        if let Ok(parameters) = PbrParameters::new(metallic, roughness, ior) {
            k.pbr = Some(parameters);
        }
    }
    let picked: Vec<Sel> = inputs.first().map(|i| i.items.clone()).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        let has_target = !picked.is_empty() || k.component.is_some();
        if ui_kit::button(ui, "Apply to selection", ui_kit::Button::Enabled(has_target)).clicked()
            && let Some(p) = selected_params(k, &picked)
        {
            k.remove_override = false;
            k.note = Some(match app.run("appearance.assign", p.clone()) {
                Ok(_) => {
                    k.applied = Some(p);
                    "Appearance applied".into()
                }
                Err(e) => e,
            });
        }
        if ui_kit::button(ui, "Remove override", ui_kit::Button::Enabled(has_target))
            .on_hover_text(crate::i18n::tr_label("Restore the inherited component or physical material appearance"))
            .clicked()
            && let Some(p) = clear_params(k, &picked)
        {
            k.note = Some(match app.run("appearance.assign", p) {
                Ok(_) => {
                    k.remove_override = true;
                    k.applied = None;
                    "Inherited appearance restored".into()
                }
                Err(e) => e,
            });
        }
    });
    crate::dialogs_library::rows(app, ui, k, inputs);
    // In this design.
    let used = in_design(app);
    if !used.is_empty() {
        ui_kit::section_count(ui, "in_design", "In This Design", true, Some(used.len()), |ui| {
            let items: Vec<_> = used.iter().enumerate().collect();
            ui_kit::swatch_grid(ui, &items, |ui, (i, (look, n))| {
                if ui_kit::color_swatch(
                    ui,
                    egui::Id::new(("ap_used", *i)),
                    c32(look.color, look.opacity),
                    k.color == look.color && k.opacity == look.opacity && k.pbr == look.pbr,
                    &format!("{} · {n} item(s)", look.name),
                    look.clone(),
                )
                .clicked()
                    && let Err(error) = choose_look(k, look)
                {
                    k.note = Some(error);
                }
            });
        });
    }
    // The library, by category.
    let lib = library(app);
    let mut cats: Vec<String> = vec!["All".into()];
    for l in &lib {
        if !cats.contains(&l.category) {
            cats.push(l.category.clone());
        }
    }
    let refs: Vec<&str> = cats.iter().map(String::as_str).collect();
    ui_kit::row(ui, "Library", |ui| combo(ui, "ap_cat", &refs, &mut k.category));
    let search = ui_kit::row(ui, "Search", |ui| {
        let id = ui.make_persistent_id("ap_search");
        ui_kit::search_field(ui, id, &mut k.search, "Appearance name")
    });
    crate::scenario::publish_handle("appearance:search", search.rect.center());
    ui_kit::row(ui, "", |ui| ui_kit::checkbox(ui, app.tr("appearance.only_available"), &mut k.only_available));
    let want = cats.get(k.category).cloned().unwrap_or_default();
    let query = k.search.trim().to_lowercase();
    let shown: Vec<(usize, &Look)> = lib
        .iter()
        .enumerate()
        .filter(|(_, l)| matches_library_filter(l, (k.category != 0).then_some(want.as_str()), &query, k.only_available))
        .collect();
    if shown.is_empty() {
        ui_kit::text(ui, crate::i18n::tr_label("No matching appearances"), ui_kit::Text::Note("appearances_empty"));
    }
    ui_kit::swatch_grid(ui, &shown, |ui, (i, l)| {
        let picked_look = k.look.as_deref() == Some(l.name.as_str()) && k.pbr == l.pbr;
        let available = preset_available(l);
        let tip = l.preset.as_ref().map_or_else(
            || format!("{} (drag onto a body or face)", l.name),
            |p| format!("{}\n{}\n{}", l.name, p.preview_note, p.requirements.join(", ")),
        );
        ui.add_enabled_ui(available, |ui| {
            let response = ui_kit::color_swatch(ui, egui::Id::new(("ap_lib", *i)), c32(l.color, l.opacity), picked_look, &tip, (*l).clone());
            if let Some(preset) = &l.preset {
                crate::scenario::publish_handle(&format!("appearance:preset:{}", preset.id), response.rect.center());
            }
            if response.clicked()
                && let Err(error) = choose_look(k, l)
            {
                k.note = Some(error);
            }
        })
        .response
        .on_hover_text(tip);
    });
    // A swatch dropped on the model or the Browser.
    let released = ui.input(|i| i.pointer.any_released());
    if released && let Some(look) = egui::DragAndDrop::take_payload::<Look>(ui.ctx()) {
        if let Err(error) = choose_look(k, &look) {
            k.note = Some(error);
            return false;
        }
        let targets = drop_target(app, k.faces);
        if let Some(mut p) = apply_params(&targets, look.color, look.opacity, k.look.as_deref(), look.pbr) {
            if let Some(pbr) = look.pbr {
                p["pbr"] = json!(pbr);
            }
            if let Some(id) = &k.preset_id {
                if let Some(object) = p.as_object_mut() {
                    object.remove("name");
                }
                p["appearance"] = json!(id);
            }
            k.note = Some(match app.run("appearance.assign", p.clone()) {
                Ok(_) => {
                    k.applied = Some(p);
                    format!("{} on {}", look.name, target_names(&targets))
                }
                Err(e) => e,
            });
        }
    }
    if let Some(n) = &k.note {
        ui_kit::text(ui, n, ui_kit::Text::Report("appearance_status", false));
    }
    false
}

fn target_names(targets: &[Sel]) -> String {
    targets
        .iter()
        .map(|x| match x {
            Sel::Body { name } => name.clone(),
            Sel::Face { body, .. } => format!("a face of {body}"),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use solidcraft_engine::Session;
    use solidcraft_engine::geom::Vec3;

    use super::*;
    use crate::Services;
    use crate::dialogs::apply_commands;

    fn app() -> SolveApp {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]})).unwrap();
        SolveApp::new(s, Services::default())
    }

    #[test]
    fn clicked_pbr_editor_commits_once_and_reopens_without_changing_mass() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":1400}));
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        let mass = h.app.session.execute("component.properties", &json!({})).unwrap();
        let before = h.app.session.undo.len();
        h.app.start("appearance.assign");
        h.frames(3);
        h.step(&json!({"click":{"handle":"appearance:pbr"}})).unwrap();
        h.step(&json!({"widget":"Apply to selection"})).unwrap();
        assert!(h.app.session.doc.appearances.bodies["Body1"].pbr.is_some());
        assert_eq!(h.app.session.undo.len(), before + 1);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(h.app.session.undo.len(), before + 1);
        assert_eq!(h.app.session.execute("component.properties", &json!({})).unwrap(), mass);
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        h.app.start("appearance.assign");
        let Kind::Appearance(k) = &h.app.dialog.as_ref().unwrap().kind else { panic!("appearance editor") };
        assert_eq!(k.pbr, Some(PbrParameters::default()));
        assert_eq!(k.opacity, 1.0);
        let params = selected_params(k, &h.app.dialog.as_ref().unwrap().inputs[0].items).unwrap();
        assert_eq!(params["pbr"]["roughness"], 0.5);
    }

    #[test]
    fn colours_parse_both_ways() {
        assert_eq!(parse_color(&json!("#0a1B2c")), Some([10, 27, 44]));
        assert_eq!(parse_color(&json!([1, 2, 3])), Some([1, 2, 3]));
        assert_eq!(parse_color(&json!([1, 2, 300])), None);
        assert_eq!(parse_color(&json!("#12345")), None);
        assert_eq!(hex([10, 27, 44]), "#0a1b2c");
    }

    /// Pre-selected bodies are the objects; OK gives them the picked look.
    #[test]
    fn ok_applies_the_look_to_the_objects() {
        let mut a = app();
        a.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
        a.start("appearance.assign");
        let mut d = a.dialog.clone().unwrap();
        assert_eq!(d.inputs[0].items.len(), 2);
        if let Kind::Appearance(k) = &mut d.kind {
            k.color = [196, 40, 36];
            k.look = Some("Paint - Red".into());
        }
        let c = apply_commands(&a, &d).unwrap();
        assert_eq!(c[0].0, "appearance.assign");
        assert_eq!(c[0].1["color"], "#c42824");
        assert_eq!(c[0].1["bodies"], json!(["Body1", "Body2"]));
        for (id, p) in c {
            a.run(&id, p).unwrap();
        }
        assert_eq!(a.session.doc.appearances.bodies.get("Body2").map(|l| l.color), Some([196, 40, 36]));
        let used = in_design(&mut a);
        assert_eq!(used.len(), 1);
        assert_eq!(used[0].0.color, [196, 40, 36]);
        assert_eq!(used[0].0.name, "Paint - Red");
        assert_eq!(used[0].1, 2);
    }

    /// A swatch dropped on a face in face mode targets the face; otherwise its body.
    #[test]
    fn drops_land_on_what_is_under_the_cursor() {
        let mut a = app();
        let p = Vec3::new(5.0, 5.0, 10.0);
        a.viewport.hover = Some(Hit::Face { body: "Body1".into(), index: 0, point: p });
        assert_eq!(drop_target(&a, false), vec![Sel::Body { name: "Body1".into() }]);
        assert_eq!(drop_target(&a, true), vec![Sel::Face { body: "Body1".into(), index: 0, point: p }]);
        a.viewport.hover = None;
        a.viewport.hover_bodies = vec!["Body2".into()];
        assert_eq!(drop_target(&a, false), vec![Sel::Body { name: "Body2".into() }]);
        let face = apply_params(&[Sel::Face { body: "Body1".into(), index: 0, point: p }], [1, 2, 3], 0.5, None, None).unwrap();
        assert_eq!(face["faces"][0]["body"], "Body1");
        assert_eq!(face["opacity"], 0.5);
        assert!(!library(&mut a).is_empty());
    }

    #[test]
    fn face_preselection_keeps_the_face_and_remove_survives_ok() {
        let mut a = app();
        let point = Vec3::new(5.0, 5.0, 10.0);
        let selected = vec![Sel::Face { body: "Body1".into(), index: 0, point }];
        a.session.selection = selected.clone();
        a.start("appearance.assign");
        let mut d = a.dialog.clone().unwrap();
        assert_eq!(d.inputs[0].items, selected);
        let Kind::Appearance(k) = &mut d.kind else { panic!("appearance dialog") };
        assert!(k.faces);
        k.color = [10, 20, 30];
        k.opacity = 0.25;
        for (id, p) in commands(k, &d.inputs) {
            a.run(&id, p).unwrap();
        }
        assert_eq!(a.session.doc.appearances.faces.len(), 1);
        assert!(a.session.doc.appearances.bodies.is_empty());
        a.run("appearance.assign", clear_params(k, &d.inputs[0].items).unwrap()).unwrap();
        k.remove_override = true;
        assert!(commands(k, &d.inputs).is_empty());
        assert!(a.session.doc.appearances.faces.is_empty());
        a.run("edit.undo", json!({})).unwrap();
        assert_eq!(a.session.doc.appearances.faces.len(), 1);
    }

    #[test]
    fn reused_design_look_preserves_transparency_and_name() {
        let mut a = app();
        a.run("appearance.assign", json!({"bodies": ["Body1"], "color": "#abcdef", "appearance": "Tinted glass", "opacity": 0.25})).unwrap();
        let used = in_design(&mut a);
        let look = &used[0].0;
        assert_eq!(look.opacity, 0.25);
        assert_eq!(look.name, "Tinted glass");
        a.session.selection = vec![Sel::Body { name: "Body1".into() }];
        a.start("appearance.assign");
        let Kind::Appearance(k) = &a.dialog.as_ref().unwrap().kind else { panic!("appearance dialog") };
        assert_eq!(k.opacity, 0.25);
        assert_eq!(k.look.as_deref(), Some("Tinted glass"));
        let p = apply_params(&[Sel::Body { name: "Body2".into() }], look.color, look.opacity, Some(&look.name), look.pbr).unwrap();
        a.run("appearance.assign", p).unwrap();
        assert_eq!(a.session.doc.appearances.bodies.get("Body1"), a.session.doc.appearances.bodies.get("Body2"));
    }

    #[test]
    fn header_chevron_folds_and_restores_appearance_inputs() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        h.app.start("appearance.assign");
        h.frames(3);
        let before = h.app.dialog.clone().unwrap();
        h.step(&json!({"click":{"handle":"dialog:collapse"}})).unwrap();
        assert!(h.app.dialog.as_ref().unwrap().collapsed);
        assert!(h.widget("Appearance name").is_none());
        h.step(&json!({"click":{"handle":"dialog:collapse"}})).unwrap();
        let after = h.app.dialog.as_ref().unwrap();
        assert!(!after.collapsed);
        assert_eq!(format!("{:?}", after.inputs), format!("{:?}", before.inputs));
        let (Kind::Appearance(before), Kind::Appearance(after)) = (&before.kind, &after.kind) else { panic!("appearance dialog") };
        assert_eq!(before.color, after.color);
        assert_eq!(before.opacity, after.opacity);
        assert_eq!(before.search, after.search);
    }

    #[test]
    fn real_frames_and_unchanged_apply_keep_precise_opacity() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Body1"],"color":"#abcdef","opacity":0.123456})).unwrap();
        for cancel in [true, false] {
            h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
            h.app.start("appearance.assign");
            h.frames(3);
            let Kind::Appearance(appearance) = &h.app.dialog.as_ref().unwrap().kind else { panic!("appearance dialog") };
            assert_eq!(appearance.opacity, 0.123456);
            h.step(&json!({"key":if cancel {"Escape"} else {"Enter"}})).unwrap();
            assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
            assert_eq!(h.app.session.doc.appearances.bodies["Body1"].opacity, 0.123456);
        }
    }

    #[test]
    fn immediate_apply_then_ok_has_one_undo_and_retains_precise_opacity() {
        let mut a = app();
        a.run("appearance.assign", json!({"bodies": ["Body1"], "color": "#abcdef", "opacity": 0.123456})).unwrap();
        a.session.selection = vec![Sel::Body { name: "Body1".into() }];
        a.start("appearance.assign");
        let mut d = a.dialog.clone().unwrap();
        let Kind::Appearance(k) = &mut d.kind else { panic!("appearance dialog") };
        assert_eq!(commands(k, &d.inputs)[0].1["opacity"], json!(0.123456));
        k.color = [1, 2, 3];
        let p = selected_params(k, &d.inputs[0].items).unwrap();
        a.run("appearance.assign", p.clone()).unwrap();
        k.applied = Some(p);
        assert!(commands(k, &d.inputs).is_empty());
        a.run("edit.undo", json!({})).unwrap();
        let restored = a.session.doc.appearances.bodies.get("Body1").unwrap();
        assert_eq!(restored.color, [171, 205, 239]);
        assert_eq!(restored.opacity, 0.123456);
        k.opacity = 0.5;
        assert_eq!(commands(k, &d.inputs).len(), 1, "changed look still applies on OK");
        k.opacity = 0.123456;
        d.inputs[0].items = vec![Sel::Body { name: "Body2".into() }];
        assert_eq!(commands(k, &d.inputs).len(), 1, "changed target still applies on OK");
    }

    #[test]
    fn inventory_rows_controls_sections_and_component_mode_remain_reachable() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":2000}));
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.run("appearance.assign", json!({"bodies":["Body1"],"color":"#abcdef","opacity":0.25,"appearance":"Tinted glass"})).unwrap();
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        h.app.start("appearance.assign");
        h.frames(5);
        for handle in [
            "dialog:Objects",
            "dialog:clear:Objects",
            "dialog:Apply To",
            "dialog:color:Colour",
            "dialog:slider:Opacity",
            "appearance:pbr",
            "appearance:search",
            "dialog:section:Local library",
            "dialog:section:In This Design",
            "dialog:Library",
            "dialog:button::Apply to selection",
            "dialog:button::Remove override",
            "library:export",
            "dialog:ok",
            "dialog:cancel",
            "dialog:collapse",
        ] {
            assert!(crate::scenario::handle_at(handle).is_some(), "missing {handle}");
        }
        for label in ["Objects", "Apply To", "Colour", "Opacity", "Surface response", "Library", "Search"] {
            assert!(crate::scenario::rect_of(&format!("dialog:row:{label}")).is_some(), "missing row {label}");
        }
        h.step(&json!({"click":{"handle":"dialog:Apply To"}})).unwrap();
        h.frames(2);
        for option in ["Bodies", "Faces"] {
            assert!(crate::scenario::handle_at(&format!("dialog:Apply To:{option}")).is_some());
        }
        h.step(&json!({"click":{"handle":"dialog:Apply To:Bodies"}})).unwrap();
        h.step(&json!({"click":{"handle":"appearance:pbr"}})).unwrap();
        for handle in ["appearance:metallic", "appearance:roughness", "appearance:ior"] {
            assert!(crate::scenario::handle_at(handle).is_some(), "missing {handle}");
        }
        let Kind::Appearance(k) = &mut h.app.dialog.as_mut().unwrap().kind else { panic!("Appearance") };
        k.component = Some("Example component".into());
        k.note = Some("Appearance applied".into());
        h.frames(4);
        assert!(h.widget("Appearance applied").is_some());
        assert!(crate::scenario::handle_at("dialog:button::Select bodies or faces instead").is_some());
        assert!(crate::scenario::handle_at("dialog:cell:Component: Example component").is_some());
        h.step(&json!({"widget":"Select bodies or faces instead"})).unwrap();
        assert!(crate::scenario::handle_at("dialog:Apply To").is_some());
        h.step(&json!({"click":{"handle":"dialog:clear:Objects"}})).unwrap();
        assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
        h.step(&json!({"click":{"handle":"dialog:color:Colour"}})).unwrap();
        h.frames(3);
        for handle in [
            "color:format:U8",
            "color:format:F",
            "color:copy",
            "color:channel:R",
            "color:channel:G",
            "color:channel:B",
            "color:sv",
            "color:hue",
            "color:current",
        ] {
            assert!(crate::scenario::handle_at(handle).is_some(), "missing {handle}");
        }
    }
    fn pack_entry(available: bool) -> Value {
        json!({"id":"starter.wood.test-preview","name":"Uniform wood preview","category":"Wood","color":[110,76,50],"opacity":1.0,
            "pbr":{"metallic":0.0,"roughness":0.6,"dielectric_ior":1.5},"available":available,
            "requirements":["opaque_pbr"],"preview_note":"Uniform color preview; no grain maps."})
    }

    #[test]
    fn pack_ui_filter_and_selection_keep_exact_id_and_explicit_preview_note() {
        let mut app = app();
        app.start("appearance.assign");
        let Kind::Appearance(k) = &mut app.dialog.as_mut().unwrap().kind else { panic!("appearance editor") };
        let look = library_look(&pack_entry(true)).unwrap();
        assert!(matches_library_filter(&look, Some("Wood"), "uniform", true));
        assert!(matches_library_filter(&look, None, "starter.wood", true));
        assert!(!matches_library_filter(&look, Some("Metal"), "", false));
        choose_look(k, &look).unwrap();
        assert_eq!(k.look.as_deref(), Some("Uniform wood preview"));
        assert!(k.note.as_ref().unwrap().contains("no grain maps"));
        let params = selected_params(k, &[Sel::Body { name: "Body1".into() }]).unwrap();
        assert_eq!(params["appearance"], "starter.wood.test-preview");
        assert_eq!(params["opacity"], 1.0);
        assert_eq!(params["pbr"]["roughness"], 0.6);
    }

    #[test]
    fn unavailable_pack_recipe_refuses_before_editor_document_or_history_mutation() {
        let mut app = app();
        app.start("appearance.assign");
        let doc = app.session.doc.clone();
        let history = (app.session.undo.len(), app.session.redo.len(), app.session.is_dirty());
        let Kind::Appearance(k) = &mut app.dialog.as_mut().unwrap().kind else { panic!("appearance editor") };
        let before = format!("{k:?}");
        let mut value = pack_entry(false);
        value["id"] = json!("starter.glass.test-recipe");
        value["requirements"] = json!(["refraction", "certified_thin_or_closed"]);
        value["preview_note"] = json!("Requires certified glass transport; alpha is not refraction.");
        let look = library_look(&value).unwrap();
        assert!(choose_look(k, &look).unwrap_err().contains("certified_thin_or_closed"));
        assert_eq!(format!("{k:?}"), before);
        assert!(!matches_library_filter(&look, None, "", true));
        assert!(matches_library_filter(&look, None, "", false));
        assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
        assert_eq!(history, (app.session.undo.len(), app.session.redo.len(), app.session.is_dirty()));
        let mut invalid = pack_entry(true);
        invalid.as_object_mut().unwrap().remove("available");
        assert!(library_look(&invalid).is_none(), "pack metadata cannot silently become a legacy look");
        value["available"] = json!(true);
        assert!(choose_look(k, &library_look(&value).unwrap()).is_err(), "unsupported optical requirements cannot masquerade as an opaque preset");
    }

    #[test]
    fn actual_opaque_pack_ui_selection_applies_once_and_undo_preserves_physics() {
        let mut app = app();
        let look = library(&mut app)
            .into_iter()
            .find(|look| look.preset.as_ref().is_some_and(|p| p.available && p.id.starts_with("starter.metal.")))
            .expect("opaque pack entry");
        app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        let physical = app.session.execute("component.properties", &json!({})).unwrap();
        let before = app.session.undo.len();
        app.start("appearance.assign");
        let mut dialog = app.dialog.clone().unwrap();
        let Kind::Appearance(k) = &mut dialog.kind else { panic!("appearance editor") };
        choose_look(k, &look).unwrap();
        let params = selected_params(k, &dialog.inputs[0].items).unwrap();
        assert_eq!(params["appearance"], look.preset.as_ref().unwrap().id);
        app.run("appearance.assign", params.clone()).unwrap();
        k.applied = Some(params);
        assert!(commands(k, &dialog.inputs).is_empty(), "OK must not duplicate immediate Apply");
        assert_eq!(app.session.undo.len(), before + 1);
        assert_eq!(app.session.execute("component.properties", &json!({})).unwrap(), physical);
        app.run("edit.undo", json!({})).unwrap();
        assert!(!app.session.doc.appearances.bodies.contains_key("Body1"));
        assert_eq!(app.session.execute("component.properties", &json!({})).unwrap(), physical);
    }

    #[test]
    fn actual_pack_provider_all_ids_requirements_and_disabled_glass_survive_ui_parsing() {
        use solidcraft_engine::doc::appearance_pack::STARTER_PACK;
        let mut app = app();
        let looks = library(&mut app);
        for preset in &STARTER_PACK {
            let matching: Vec<_> = looks.iter().filter(|look| look.preset.as_ref().is_some_and(|p| p.id == preset.id)).collect();
            assert_eq!(matching.len(), 1, "actual provider entry missing from UI: {}", preset.id);
            let look = matching[0];
            let metadata = look.preset.as_ref().unwrap();
            assert_eq!(metadata.preview_note, preset.preview_note);
            assert_eq!(serde_json::to_value(&metadata.requirements).unwrap(), serde_json::to_value(preset.requirements).unwrap());
            assert_eq!(preset_available(look), preset.is_opaque());
        }
        let pack: Vec<_> = looks.iter().filter(|look| look.preset.is_some()).collect();
        assert_eq!(pack.len(), 40);
        assert_eq!(pack.iter().filter(|look| preset_available(look)).count(), 34);
        assert_eq!(pack.iter().filter(|look| !preset_available(look)).count(), 6);
        for (name, color, opacity) in solidcraft_engine::doc::appearance::LIBRARY {
            let look = looks.iter().find(|look| look.name == name).unwrap();
            assert_eq!((look.color, look.opacity, look.pbr, look.preset.as_ref()), (color, opacity, None, None));
        }
        // The six published finishes remain independent of starter metadata.
        for (name, color, metallic, roughness) in solidcraft_engine::doc::appearance::OPAQUE_FINISHES {
            let matching: Vec<_> = looks.iter().filter(|look| look.name == name).collect();
            assert_eq!(matching.len(), 1);
            let look = matching[0];
            assert_eq!(look.color, color);
            assert_eq!(look.opacity, 1.0);
            assert_eq!(look.pbr, Some(PbrParameters::new(metallic, roughness, 1.5).unwrap()));
            assert!(look.preset.is_none() && preset_available(look));
        }
        assert_eq!(looks.len(), 66);
        assert_eq!(looks.iter().filter(|look| preset_available(look)).count(), 60);
        assert_eq!(looks.iter().filter(|look| preset_available(look) && look.opacity == 1.0).count(), 58);
        assert_eq!(looks.iter().filter(|look| look.opacity != 1.0).count(), 2);
    }

    #[test]
    fn manual_edit_of_selected_or_reopened_pack_look_does_not_resolve_preset_again() {
        use solidcraft_engine::doc::appearance_pack::STARTER_PACK;
        for reopened in [false, true] {
            let mut app = app();
            let preset = STARTER_PACK.iter().find(|p| p.is_opaque()).unwrap();
            app.run("appearance.assign", json!({"bodies":["Body1"],"appearance":preset.id})).unwrap();
            app.session.selection = vec![Sel::Body { name: "Body1".into() }];
            app.start("appearance.assign");
            let mut dialog = app.dialog.clone().unwrap();
            let Kind::Appearance(k) = &mut dialog.kind else { panic!("appearance editor") };
            if !reopened {
                k.preset_id = Some(preset.id.into());
            }
            assert_eq!(k.look.as_deref(), Some(preset.name));
            detach_preset(k);
            k.pbr = None;
            k.opacity = 0.5;
            let params = selected_params(k, &dialog.inputs[0].items).unwrap();
            assert!(params.get("appearance").is_none());
            app.run("appearance.assign", params).unwrap();
            let result = app.session.doc.appearances.bodies.get("Body1").unwrap();
            assert!(result.pbr.is_none());
            assert_eq!(result.opacity, 0.5);
        }
    }

    #[test]
    fn manual_edit_of_published_finish_keeps_scalar_override_after_selection_and_editor_reopen() {
        for (name, ..) in solidcraft_engine::doc::appearance::OPAQUE_FINISHES {
            for reopened in [false, true] {
                let mut app = app();
                let look = library(&mut app).into_iter().find(|look| look.name == name).unwrap();
                app.run("appearance.assign", json!({"bodies":["Body1"],"appearance":name})).unwrap();
                let physical = app.session.execute("component.properties", &json!({})).unwrap();
                app.session.selection = vec![Sel::Body { name: "Body1".into() }];
                app.start("appearance.assign");
                let mut dialog = app.dialog.clone().unwrap();
                let Kind::Appearance(k) = &mut dialog.kind else { panic!("appearance editor") };
                if !reopened {
                    choose_look(k, &look).unwrap();
                }
                assert_eq!(k.look.as_deref(), Some(name));
                assert!(k.pbr.is_some() && k.preset_id.is_none());
                detach_preset(k);
                assert_eq!(k.look.as_deref(), Some(name));
                assert!(k.preset_id.is_none());
                k.pbr = None;
                k.opacity = 0.5;
                let params = selected_params(k, &dialog.inputs[0].items).unwrap();
                assert_eq!(params["name"], name);
                assert!(params.get("color").is_some());
                let undo = app.session.undo.len();
                app.run("appearance.assign", params).unwrap();
                let result = app.session.doc.appearances.bodies.get("Body1").unwrap();
                assert_eq!(result.name, name);
                assert!(result.pbr.is_none());
                assert_eq!(result.opacity, 0.5);
                assert_eq!(app.session.undo.len(), undo + 1);
                assert_eq!(app.session.execute("component.properties", &json!({})).unwrap(), physical);
            }
        }
    }
    #[test]
    fn clicked_starter_swatch_applies_once_and_disabled_glass_cannot_replace_it() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":1400}));
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
        let mass = h.app.session.execute("component.properties", &json!({})).unwrap();
        let before = h.app.session.undo.len();
        h.app.start("appearance.assign");
        let Kind::Appearance(k) = &mut h.app.dialog.as_mut().unwrap().kind else { panic!("appearance editor") };
        k.search = "starter.metal.gold-warm".into();
        h.frames(3);
        h.step(&json!({"click":{"handle":"appearance:preset:starter.metal.gold-warm"}})).unwrap();
        h.step(&json!({"widget":"Apply to selection"})).unwrap();
        assert_eq!(h.app.session.doc.appearances.bodies["Body1"].name, "Metal - Warm Reflective Gold");
        assert_eq!(h.app.session.doc.appearances.bodies["Body1"].pbr.unwrap().metallic(), 1.0);
        assert_eq!(h.app.session.undo.len(), before + 1);
        let Kind::Appearance(k) = &mut h.app.dialog.as_mut().unwrap().kind else { panic!("appearance editor") };
        k.search = "starter.glass.clear-soda-lime".into();
        let selected = (k.preset_id.clone(), k.color, k.opacity, k.pbr);
        h.frames(3);
        h.step(&json!({"click":{"handle":"appearance:preset:starter.glass.clear-soda-lime"}})).unwrap();
        let Kind::Appearance(k) = &h.app.dialog.as_ref().unwrap().kind else { panic!("appearance editor") };
        assert_eq!((k.preset_id.clone(), k.color, k.opacity, k.pbr), selected);
        h.step(&json!({"widget":"OK"})).unwrap();
        assert_eq!(h.app.session.undo.len(), before + 1);
        assert_eq!(h.app.session.execute("component.properties", &json!({})).unwrap(), mass);
    }

    #[test]
    fn selectable_library_entries_exclude_unavailable_starter_glass_recipes() {
        let mut h = crate::scenario::Harness::new();
        let listed = h.app.session.execute("appearance.library", &json!({})).unwrap();
        let rows = listed["appearances"].as_array().unwrap();
        let available = rows.iter().filter(|row| row.get("available").and_then(Value::as_bool) != Some(false)).count();
        let selectable: Vec<_> = library(&mut h.app).into_iter().filter(preset_available).collect();
        assert_eq!(selectable.len(), available);
        for preset in &solidcraft_engine::doc::appearance_pack::STARTER_PACK {
            assert_eq!(selectable.iter().any(|look| look.name == preset.name), preset.is_opaque());
        }
    }

    #[test]
    fn applying_and_clearing_owned_recipe_labels_never_resolves_the_recipe() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"body_name":"Owned","length":10,"width":10,"height":10})).unwrap();
        for name in ["starter.glass.clear-soda-lime", "Glass Recipe - Low Iron Clear", "Metal - Warm Reflective Gold"] {
            h.app.run("appearance.assign", json!({"bodies":["Owned"],"name":name,"color":[31,61,91],"pbr":null})).unwrap();
            let original = h.app.session.doc.appearances.bodies["Owned"].clone();
            h.app.session.selection = vec![Sel::Body { name: "Owned".into() }];
            h.app.start("appearance.assign");
            let dialog = h.app.dialog.as_ref().unwrap();
            let Kind::Appearance(state) = &dialog.kind else { panic!("appearance editor expected") };
            let params = selected_params(state, &dialog.inputs[0].items).unwrap();
            let clear = clear_params(state, &dialog.inputs[0].items).unwrap();
            assert_eq!(params["name"], name);
            assert!(params.get("appearance").is_none());
            assert_eq!(params["pbr"], Value::Null);
            h.app.run("appearance.assign", params).unwrap();
            assert_eq!(h.app.session.doc.appearances.bodies["Owned"], original);
            assert!(clear.get("name").is_none() && clear.get("appearance").is_none());
            h.app.run("appearance.assign", clear).unwrap();
            assert!(!h.app.session.doc.appearances.bodies.contains_key("Owned"));
        }
    }

    #[test]
    fn dropped_scalar_and_pbr_requests_match_ok_without_an_extra_undo() {
        for response in [None, Some(PbrParameters::new(0.7, 0.25, 1.5).unwrap())] {
            let mut a = app();
            a.session.selection = vec![Sel::Body { name: "Body1".into() }];
            a.start("appearance.assign");
            let mut dialog = a.dialog.clone().unwrap();
            let Kind::Appearance(state) = &mut dialog.kind else { panic!("appearance editor") };
            state.look = Some("Owned swatch".into());
            state.color = [31, 61, 91];
            state.opacity = 1.0;
            state.pbr = response;
            let params = apply_params(&dialog.inputs[0].items, state.color, state.opacity, state.look.as_deref(), state.pbr).unwrap();
            assert!(params.get("pbr").is_some(), "the complete owned response is explicit");
            assert_eq!(params["pbr"], json!(response));
            let original = a.session.doc.clone();
            let undo = a.session.undo.len();
            a.run("appearance.assign", params.clone()).unwrap();
            state.applied = Some(params);
            assert_eq!(a.session.undo.len(), undo + 1);
            assert!(commands(state, &dialog.inputs).is_empty(), "OK after a drop is not another edit");
            a.run("edit.undo", json!({})).unwrap();
            assert_eq!(a.session.doc, original);
        }
    }
}
