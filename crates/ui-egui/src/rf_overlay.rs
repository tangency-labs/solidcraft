//! Wi-Fi coverage in the 3D view (`inspect.rf_coverage`, #20161).
//!
//! - **The heatmap** is a textured quad on the slice plane ([`gpu_image`]), depth-tested like a
//!   canvas. While it is on and no Section Analysis is, the view is cut just above the slice
//!   ([`clip`]), so ceilings don't hide it and the walls show as cut faces.
//! - **Each emitter** is a marker on the heatmap. Dragging one runs the command again with the
//!   emitter moved: on a coarser grid while it moves, then on the full grid on release. The
//!   engine keeps the sliced walls, so only the paths are traced again.
//! - **The card** at the bottom right picks the device, the band, the emitter's height, the slice
//!   height and which bodies make walls, adds an emitter, and clears the heatmap. It shows the
//!   coverage figures and the colour key.

use std::cell::Cell;

use egui::{Color32, Pos2, Stroke};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::cmd::rf_coverage::{DEVICES, RfOverlay};
use solidcraft_ui_text::tr;

use crate::SolveApp;
use crate::gpu::GpuImage;
use crate::ui_kit::{DetailEvent, DetailKey, DetailSetting, DetailValue, Details};
use crate::viewport::Proj;

const CMD: &str = "inspect.rf_coverage";
/// The heatmap's image id: far from the canvases', which count up from 1.
const IMAGE_ID: u64 = u64::MAX - 0x5246;
/// How far below the slice the heatmap lies (mm), so the cut just above it doesn't take it.
const BELOW_MM: f64 = 1.0;
/// How near the pointer must be to a marker to pick it, in points.
const PICK_PX: f32 = 14.0;
/// The grid step while an emitter moves, at least (mm).
const DRAG_STEP_MM: f64 = 150.0;

thread_local! {
    /// The emitter being dragged, by index.
    static DRAG: Cell<Option<usize>> = const { Cell::new(None) };
}

fn overlay(app: &SolveApp) -> Option<&RfOverlay> {
    app.session.rf_coverage.as_deref()
}

/// The heatmap as a GPU quad on the slice plane.
pub fn gpu_image(app: &SolveApp) -> Option<GpuImage> {
    let o = overlay(app)?;
    let (w, h) = (o.grid.columns, o.grid.rows);
    // The engine's first row is the grid's lowest y; an image's first row is its top.
    let mut rgba = Vec::with_capacity(o.rgba.len());
    for row in (0..h).rev() {
        rgba.extend_from_slice(o.rgba.get(row * w * 4..(row + 1) * w * 4)?);
    }
    let [x0, y0] = o.grid.origin;
    let (x1, y1) = (x0 + w as f64 * o.grid.step, y0 + h as f64 * o.grid.step);
    let z = (o.height - BELOW_MM) as f32;
    let corners = [[x0 as f32, y0 as f32, z], [x1 as f32, y0 as f32, z], [x1 as f32, y1 as f32, z], [x0 as f32, y1 as f32, z]];
    Some(GpuImage {
        id: IMAGE_ID,
        version: o.version,
        size: [u32::try_from(w).ok()?, u32::try_from(h).ok()?],
        rgba: std::sync::Arc::new(rgba),
        corners,
        opacity: 1.0,
    })
}

/// The cut just above the slice while the heatmap is on: (origin, normal) of a plane that hides
/// what lies above it. `None` while a Section Analysis is on: the user's section wins.
pub fn clip(app: &SolveApp) -> Option<(Vec3, Vec3)> {
    let o = overlay(app)?;
    app.session.section.is_empty().then(|| (Vec3::new(0.0, 0.0, o.height), Vec3::new(0.0, 0.0, 1.0)))
}

/// The ribbon button: turn the heatmap off when it is on, else on with one emitter in the middle
/// of the model.
pub fn toggle(app: &mut SolveApp) {
    if overlay(app).is_some() {
        if let Err(e) = app.run(CMD, json!({"clear": true})) {
            app.set_status(e, true);
        }
        return;
    }
    let Ok(b) = solidcraft_engine::view::bounds(&app.session) else {
        app.set_status(tr("ui.rf_coverage_nothing"), true);
        return;
    };
    let c = b.center();
    let params = json!({"emitters": [{"at": [c.x, c.y], "height": 1000, "device": "udr7"}], "band": "5"});
    match app.run(CMD, params) {
        Ok(_) => app.set_status(tr("ui.rf_coverage_hint"), false),
        Err(e) => app.set_status(e, true),
    }
}

/// Where emitter `i` of `o` is on screen.
fn marker(o: &RfOverlay, proj: &Proj, i: usize) -> Option<Pos2> {
    let e = o.emitters.get(i)?;
    proj.to_screen(Vec3::new(e.at[0], e.at[1], o.height))
}

/// The emitter under `pos`, nearest first.
fn under(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<usize> {
    let o = overlay(app)?;
    (0..o.emitters.len())
        .filter_map(|i| Some((i, marker(o, proj, i)?.distance(pos))))
        .filter(|(_, d)| *d <= PICK_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Start dragging the emitter under `pos`; false when there is none.
pub fn start_drag(app: &SolveApp, proj: &Proj, pos: Pos2) -> bool {
    let Some(i) = under(app, proj, pos) else { return false };
    DRAG.with(|d| d.set(Some(i)));
    true
}

pub fn dragging() -> bool {
    DRAG.with(Cell::get).is_some()
}

/// The parameters of the heatmap shown, with emitter `i` at world `at`.
fn moved(o: &RfOverlay, i: usize, at: [f64; 2]) -> Option<Value> {
    let mut p = o.params.clone();
    let e = p.get_mut("emitters")?.get_mut(i)?;
    // An `at` with a height keeps it as `height`.
    if let Some(z) = e.get("at").and_then(|a| a.get(2)).and_then(Value::as_f64)
        && e.get("height").is_none()
    {
        e["height"] = json!(z);
    }
    e["at"] = json!(at);
    p.as_object_mut()?.remove("png");
    Some(p)
}

/// Move the dragged emitter to where the pointer at `pos` meets the slice plane.
pub fn drag_to(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    let Some(i) = DRAG.with(Cell::get) else { return };
    let Some(o) = overlay(app) else {
        DRAG.with(|d| d.set(None));
        return;
    };
    let (origin, dir) = proj.ray(pos);
    if dir.z.abs() < 1e-9 {
        return;
    }
    let t = (o.height - origin.z) / dir.z;
    let hit = origin + dir * t;
    if !hit.is_finite() || t < 0.0 {
        return;
    }
    let Some(keep) = moved(o, i, [hit.x, hit.y]) else { return };
    // A coarser grid while it moves keeps the drag smooth; the release works out the full one.
    let mut p = keep.clone();
    let step = p.get("grid_step").and_then(Value::as_f64).unwrap_or(75.0);
    p["grid_step"] = json!(step.max(DRAG_STEP_MM));
    p["points"] = json!([]);
    if app.session.execute(CMD, &p).is_ok()
        && let Some(shown) = app.session.rf_coverage.as_mut()
    {
        // The full parameters, with the emitter where it now is, for the release.
        std::sync::Arc::make_mut(shown).params = keep;
    }
}

/// Finish the drag: the full grid where the emitter now is.
pub fn end_drag(app: &mut SolveApp) {
    let Some(_) = DRAG.with(|d| d.take()) else { return };
    let Some(p) = overlay(app).map(|o| o.params.clone()) else { return };
    if let Err(e) = app.run(CMD, p) {
        app.set_status(e, true);
    }
}

/// Draw the emitters' markers over the view.
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(o) = overlay(app) else { return };
    let t = solidcraft_theme::Tokens::get();
    let dragged = DRAG.with(Cell::get);
    for (i, e) in o.emitters.iter().enumerate() {
        let Some(p) = marker(o, proj, i) else { continue };
        let r = if dragged == Some(i) { 9.0 } else { 7.0 };
        painter.circle(p, r, t.overlay, Stroke::new(3.0, t.accent));
        painter.circle_filled(p, 2.5, t.accent);
        painter.text(p + egui::vec2(r + 4.0, 0.0), egui::Align2::LEFT_CENTER, &e.name, egui::FontId::proportional(t.font_small), t.text);
    }
}

fn length(app: &SolveApp, value: f64) -> DetailValue {
    DetailValue::Length { value, units: app.session.doc.units.clone(), decimals: app.preferences.length_decimals }
}

fn setting(id: &str, label: &str, value: DetailValue) -> DetailSetting {
    DetailSetting { id: id.into(), label: tr(label).to_string(), value, enabled: true }
}

/// The components a body choice offers: the design's top-level ones.
fn components(app: &SolveApp) -> Vec<String> {
    app.session.doc.components.iter().filter(|c| c.parent == 0).map(|c| c.name.clone()).collect()
}

const BANDS: [&str; 3] = ["2.4", "5", "6"];

/// The card: settings, the coverage figures and the key.
pub fn details(app: &SolveApp) -> Option<Details> {
    let o = overlay(app)?;
    let first = o.params.get("emitters").and_then(|e| e.get(0));
    let device = first.and_then(|e| e.get("device")).and_then(Value::as_str);
    let height = o.emitters.first().map_or(1000.0, |e| e.height);
    let band = BANDS.iter().position(|b| *b == o.band.name()).unwrap_or(1);
    let mut devices: Vec<String> = DEVICES.iter().map(|d| d.name.to_string()).collect();
    devices.push(tr("ui.rf_coverage_custom").to_string());
    let device = device.and_then(|id| DEVICES.iter().position(|d| d.id == id)).unwrap_or(DEVICES.len());
    let comps = components(app);
    let mut walls = vec![tr("ui.rf_coverage_all_bodies").to_string()];
    walls.extend(comps.iter().cloned());
    let scoped = o.params.get("components").and_then(|c| c.get(0)).and_then(Value::as_str);
    let wall = scoped.and_then(|c| comps.iter().position(|n| n == c)).map_or(0, |i| i + 1);
    let mut d = Details { caption: Some(tr("ui.rf_coverage_caption").to_string()), ..Default::default() };
    d.settings = vec![
        setting("device", "ui.rf_coverage_device", DetailValue::Choice { selected: device, options: devices }),
        setting("band", "ui.rf_coverage_band", DetailValue::Choice { selected: band, options: BANDS.iter().map(|b| format!("{b} GHz")).collect() }),
        setting("height", "ui.rf_coverage_height", length(app, height)),
        setting("slice", "ui.rf_coverage_slice", length(app, o.height)),
        setting("walls", "ui.rf_coverage_walls", DetailValue::Choice { selected: wall, options: walls }),
    ];
    if let Some([good, fair, p5]) = o.summary {
        d.readout = vec![
            (tr("ui.rf_coverage_fast").to_string(), format!("{good:.0}%")),
            (tr("ui.rf_coverage_video").to_string(), format!("{fair:.0}%")),
            (tr("ui.rf_coverage_p5").to_string(), format!("{p5:.0} dBm")),
        ];
    }
    d.key = o
        .legend
        .iter()
        .enumerate()
        .map(|(i, (from, [r, g, b]))| DetailKey {
            colour: Color32::from_rgb(*r, *g, *b),
            pattern: false,
            label: if from.is_finite() {
                format!("≥ {from:.0} dBm")
            } else {
                format!("< {:.0} dBm", o.legend.get(i.wrapping_sub(1)).map_or(-80.0, |(t, _)| *t))
            },
            value: String::new(),
            numeric: true,
            pattern_colour: None,
        })
        .collect();
    d.actions = vec![("add".into(), tr("ui.rf_coverage_add").to_string()), ("clear".into(), tr("ui.rf_coverage_clear").to_string())];
    Some(d)
}

/// A change on the card, as the command's new parameters.
fn changed(app: &SolveApp, id: &str, value: &DetailValue) -> Option<Value> {
    let o = overlay(app)?;
    let mut p = o.params.clone();
    p.as_object_mut()?.remove("png");
    let scalar = |v: &DetailValue| match v {
        DetailValue::Length { value, .. } | DetailValue::Number { value, .. } => Some(*value),
        _ => None,
    };
    let choice = |v: &DetailValue| if let DetailValue::Choice { selected, .. } = v { Some(*selected) } else { None };
    match id {
        "device" => {
            let pick = DEVICES.get(choice(value)?);
            for e in p.get_mut("emitters")?.as_array_mut()? {
                let e = e.as_object_mut()?;
                match pick {
                    Some(d) => {
                        e.remove("eirp_dbm");
                        e.insert("device".into(), json!(d.id));
                    }
                    None => {
                        e.remove("device");
                        e.insert("eirp_dbm".into(), json!(20));
                    }
                }
            }
        }
        "band" => p["band"] = json!(BANDS.get(choice(value)?)?),
        "height" => {
            for e in p.get_mut("emitters")?.as_array_mut()? {
                e["height"] = json!(scalar(value)?);
            }
        }
        "slice" => p["slice_height"] = json!(scalar(value)?),
        "walls" => {
            let o = p.as_object_mut()?;
            match choice(value)?.checked_sub(1).and_then(|i| components(app).get(i).cloned()) {
                Some(c) => {
                    o.insert("components".into(), json!([c]));
                    o.insert("rf_materials".into(), json!(true));
                }
                None => {
                    o.remove("components");
                    o.remove("rf_materials");
                }
            }
        }
        _ => return None,
    }
    Some(p)
}

/// Act on the card.
pub fn dispatch(app: &mut SolveApp, event: DetailEvent) {
    let params = match event {
        DetailEvent::Change(id, value) => changed(app, &id, &value),
        DetailEvent::Action(id) if id == "clear" => Some(json!({"clear": true})),
        DetailEvent::Action(id) if id == "add" => overlay(app).and_then(|o| {
            // A new emitter beside the first, a metre east, with its settings.
            let mut p = o.params.clone();
            p.as_object_mut()?.remove("png");
            let list = p.get_mut("emitters")?.as_array_mut()?;
            let mut next = list.first()?.clone();
            let at = o.emitters.first()?.at;
            next["at"] = json!([at[0] + 1000.0, at[1]]);
            next.as_object_mut()?.remove("name");
            list.push(next);
            Some(p)
        }),
        _ => None,
    };
    if let Some(p) = params
        && let Err(e) = app.run(CMD, p)
    {
        app.set_status(e, true);
    }
}

/// The card over the bottom right of the view while the heatmap is on.
pub fn card(app: &mut SolveApp, ui: &mut egui::Ui, viewport: egui::Rect) {
    let Some(description) = details(app) else { return };
    let t = solidcraft_theme::Tokens::get();
    let container = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::ViewportStatus);
    let mut events = Vec::new();
    container
        .area(egui::Id::unique("rf_coverage_card"))
        .order(egui::Order::Foreground)
        .movable(false)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .fixed_pos(viewport.right_bottom() + egui::vec2(-t.space_12, -t.space_12))
        .show(ui.ctx(), |ui| {
            let frame = container.frame(ui.ctx(), t.overlay, crate::frame::Side::Right).inner_margin(t.space_8);
            frame.show(ui, |ui| {
                ui.set_width(280.0);
                events = crate::ui_kit::details(ui, "rf_coverage_card", &description, viewport.height() * 0.7, None);
            });
        });
    for event in events {
        dispatch(app, event);
    }
}

#[cfg(test)]
#[path = "rf_overlay_tests.rs"]
mod tests;
