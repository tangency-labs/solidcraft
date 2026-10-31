//! Mouse gestures: a ring of tools that opens under the pointer when the right button is held and
//! the pointer moves [`OPEN_DRAG`] points, a different set in each studio, taken by touching a
//! tool's zone.
//!
//! Gestures work with the SolidWorks mouse mode only (the wheel orbits, Ctrl + wheel pans). While
//! they are on, the right button no longer orbits the view (`prefs::nav_mode`) and the optional
//! radial menu is off (`context_menu::track_hold` asks [`active`]). A right-click that does not
//! move still opens the usual context menu (or finishes the tool in progress); one that moves
//! far enough opens the ring instead, even while a tool is in progress, and the click that ends
//! it opens no menu ([`took_press`]). With gestures off nothing changes.
//!
//! The ring is 280 points across inside and 350 outside. Every tool has a zone, and only tools
//! have zones: five tools make five equal parts of the circle. Zones alternate between two tints,
//! with a third one on the last zone when their number is odd, so no two zones side by side
//! share a tint ([`tint`]). A tool is taken as soon as the pointer leaves the hub into its zone
//! ([`entered`]); the zones reach outward without limit.
//!
//! A [`Studio`] is the part of the application the person is working in: a tab of the Design
//! workspace, a sketch being edited, or the Render workspace. Each has its own default tools, and
//! [`Config`] (kept with the preferences, edited in [`editor`]) holds the person's own list for
//! any of them: how many tools and in which place, clockwise from the top.

use crate::ui_kit::{Container, Surface};
use std::collections::BTreeMap;

use egui::{Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::SolveApp;
use crate::context_menu::{Item, act, cmd};
use crate::theme::Tokens;

/// Most tools one ring may hold.
pub(crate) const MAX_TOOLS: usize = 12;
/// Radius of the hub: the ring is 280 points across inside.
pub(crate) const HUB: f32 = 140.0;
/// Outer radius: 350 points across. The zones themselves continue outward.
pub(crate) const RING: f32 = 175.0;
/// Space between two drawn zones, in degrees.
const GAP: f32 = 1.5;
/// Points sampled along one frame's pointer move when looking for the zone it touched.
const SAMPLES: usize = 48;
/// The only mouse mode gestures work with.
pub(crate) const MOUSE_MODE: &str = "solidworks";

// ---------------------------------------------------------------------------------------------
// Studios and their tools
// ---------------------------------------------------------------------------------------------

/// Where the person is working. Each studio has its own ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Studio {
    /// The Solid tab (and the tabs that have no ring of their own).
    Part,
    /// A sketch is being edited.
    Sketch,
    Surface,
    SheetMetal,
    Plastic,
    Mesh,
    /// The Render workspace.
    Render,
}

const PART: [&str; 8] = ["repeat", "delete", "solid.press_pull", "solid.hole", "sketch.create", "solid.extrude", "undo", "solid.move"];
const SKETCH: [&str; 8] =
    ["repeat", "delete", "sketch.line", "sketch.rectangle.two_point", "ui.finishSketch", "sketch.circle.center", "undo", "sketch.dimension"];
const SURFACE: [&str; 8] = ["repeat", "delete", "surface.extrude", "surface.loft", "sketch.create", "surface.patch", "undo", "surface.trim"];
const SHEET_METAL: [&str; 8] = ["repeat", "delete", "sheet.flange", "sheet.hem", "sketch.create", "sheet.fold", "undo", "sheet.flat_pattern"];
const PLASTIC: [&str; 8] = ["repeat", "delete", "plastic.boss", "plastic.lip", "sketch.create", "plastic.snap_fit", "undo", "plastic.rest"];
const MESH: [&str; 8] = ["repeat", "delete", "file.save_mesh", "inspect.measure", "inspect.section", "solid.move", "undo", "inspect.interference"];
const RENDER: [&str; 8] = [
    "render.start",
    "appearance.assign",
    "render.lights.add",
    "render.settings.set",
    "render.camera.fit",
    "render.compare",
    "ui.workspace.design",
    "undo",
];

impl Studio {
    pub(crate) const ALL: [Studio; 7] =
        [Studio::Part, Studio::Sketch, Studio::Surface, Studio::SheetMetal, Studio::Plastic, Studio::Mesh, Studio::Render];

    /// The studio the application is in now.
    pub(crate) fn of(app: &SolveApp) -> Studio {
        if crate::render_workspace::active(app) {
            Studio::Render
        } else if app.session.active_sketch.is_some() {
            Studio::Sketch
        } else {
            match app.ui.tab.as_str() {
                "SURFACE" => Studio::Surface,
                "SHEET METAL" => Studio::SheetMetal,
                "PLASTIC" => Studio::Plastic,
                "MESH" => Studio::Mesh,
                _ => Studio::Part,
            }
        }
    }

    /// The name the preferences keep this studio's list under.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Studio::Part => "part",
            Studio::Sketch => "sketch",
            Studio::Surface => "surface",
            Studio::SheetMetal => "sheet_metal",
            Studio::Plastic => "plastic",
            Studio::Mesh => "mesh",
            Studio::Render => "render",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Studio::Part => "Part",
            Studio::Sketch => "Sketch",
            Studio::Surface => "Surface",
            Studio::SheetMetal => "Sheet Metal",
            Studio::Plastic => "Plastic",
            Studio::Mesh => "Mesh",
            Studio::Render => "Render",
        }
    }

    /// The studio's tools as shipped, clockwise from the top.
    pub(crate) fn defaults(self) -> &'static [&'static str] {
        match self {
            Studio::Part => &PART,
            Studio::Sketch => &SKETCH,
            Studio::Surface => &SURFACE,
            Studio::SheetMetal => &SHEET_METAL,
            Studio::Plastic => &PLASTIC,
            Studio::Mesh => &MESH,
            Studio::Render => &RENDER,
        }
    }
}

/// Tools that are not commands: what they are called in a list of tools.
const SPECIALS: [(&str, &str); 3] = [("repeat", "Repeat"), ("delete", "Delete"), ("undo", "Undo")];
/// View actions (`ui.*`): id, label, icon.
const ACTIONS: [(&str, &str, &str); 5] = [
    ("ui.fit", "Fit", "fit"),
    ("ui.home", "Home View", "home"),
    ("ui.finishSketch", "Finish Sketch", "finish"),
    ("ui.workspace.design", "Design Workspace", "box"),
    ("ui.clear", "Clear Selection", ""),
];

/// A command's label without its shortcut hint: `Line (L)` is `Line`.
fn short(label: &str) -> &str {
    label.split(" (").next().unwrap_or(label).trim()
}

/// What a tool is called, or none when `id` is not a tool.
pub(crate) fn label_of(id: &str) -> Option<String> {
    if let Some((_, label)) = SPECIALS.iter().find(|(i, _)| *i == id) {
        return Some((*label).to_string());
    }
    if let Some((_, label, _)) = ACTIONS.iter().find(|(i, _, _)| *i == id) {
        return Some((*label).to_string());
    }
    solidcraft_engine::find_command(id).map(|spec| short(spec.label).to_string())
}

/// The tool `id` as a menu item (its state follows the design), or none for an unknown id.
pub(crate) fn item(app: &SolveApp, id: &str) -> Option<Item> {
    match id {
        "repeat" => Some(match &app.last_command {
            Some((_, label)) => act("ui.repeat", &format!("Repeat {label}"), "redo"),
            None => act("ui.repeat", "Repeat", "redo").on(false),
        }),
        "delete" => Some(act("ui.delete", "Delete", "delete").key("Del").on(!app.session.selection.is_empty())),
        "undo" => Some(cmd(app, "edit.undo", "Undo").on(!app.session.history.is_empty())),
        _ if crate::render_workspace::active(app)
            && matches!(id, "appearance.assign" | "render.settings.set" | "render.camera.fit" | "render.compare") =>
        {
            let key = match id {
                "appearance.assign" => "render.radial_assign",
                "render.settings.set" => "render.radial_save",
                "render.camera.fit" => "render.radial_fit",
                _ => "render.radial_compare",
            };
            Some(cmd(app, id, app.tr(key)))
        }
        _ => match ACTIONS.iter().find(|(i, _, _)| *i == id) {
            Some((i, label, icon)) => Some(act(i, label, icon)),
            None => solidcraft_engine::find_command(id).map(|spec| cmd(app, spec.id, short(spec.label))),
        },
    }
}

/// Every tool a ring can hold, as (id, label): the specials and view actions, then the commands
/// that sit on the toolbar.
pub(crate) fn catalog() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = SPECIALS.iter().map(|(i, l)| ((*i).to_string(), (*l).to_string())).collect();
    v.extend(ACTIONS.iter().map(|(i, l, _)| ((*i).to_string(), (*l).to_string())));
    for spec in solidcraft_engine::command_specs() {
        if !spec.tab.is_empty() && spec.planned.is_none() && !v.iter().any(|(i, _)| i == spec.id) {
            v.push((spec.id.to_string(), short(spec.label).to_string()));
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------
// The person's settings
// ---------------------------------------------------------------------------------------------

/// The gesture settings, kept with the preferences.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Gestures are on (they also need the SolidWorks mouse mode, see [`active`]).
    pub enabled: bool,
    /// A studio's own list of tool ids, clockwise from the top, by [`Studio::key`]. A studio
    /// without an entry uses its default tools.
    pub wheels: BTreeMap<String, Vec<String>>,
}

impl Config {
    /// The tool ids of `studio`: the person's list cleaned of unknown and repeated ids and cut
    /// to [`MAX_TOOLS`], or the studio's defaults when nothing usable is left.
    pub(crate) fn tools(&self, studio: Studio) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for id in self.wheels.get(studio.key()).into_iter().flatten() {
            if out.len() < MAX_TOOLS && label_of(id).is_some() && !out.contains(id) {
                out.push(id.clone());
            }
        }
        if out.is_empty() { studio.defaults().iter().map(|s| (*s).to_string()).collect() } else { out }
    }

    /// Whether `studio` has a list of its own.
    pub(crate) fn is_custom(&self, studio: Studio) -> bool {
        self.wheels.contains_key(studio.key())
    }

    pub(crate) fn set_tools(&mut self, studio: Studio, tools: Vec<String>) {
        self.wheels.insert(studio.key().to_string(), tools);
    }

    /// Puts `studio` back to its default tools.
    pub(crate) fn reset(&mut self, studio: Studio) {
        self.wheels.remove(studio.key());
    }
}

/// Gestures are on and the mouse mode allows them.
pub(crate) fn active(app: &SolveApp) -> bool {
    app.preferences.mouse_gestures.enabled && app.preferences.nav == MOUSE_MODE
}

/// The ring's tools in `studio` as menu items, unknown ids dropped.
pub(crate) fn items(app: &SolveApp, studio: Studio) -> Vec<Item> {
    app.preferences.mouse_gestures.tools(studio).iter().filter_map(|id| item(app, id)).collect()
}

// ---------------------------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------------------------

/// The zone of an `n`-tool ring a pointer offset from its centre is in: none inside the hub.
/// Zone 0 is centred on the top and the rest follow clockwise (screen y points down); each takes
/// `360 / n` degrees.
pub(crate) fn sector(d: Vec2, n: usize) -> Option<usize> {
    if n == 0 || !d.is_finite() || d.length() < HUB {
        return None;
    }
    let step = 360.0 / n as f32;
    let a = d.x.atan2(-d.y).to_degrees();
    let k = ((a + 360.0 + step / 2.0) / step).floor() as i64;
    Some(k.rem_euclid(n as i64) as usize)
}

/// The enabled tool whose zone the pointer touched moving straight from `from` to `to` around a
/// ring centred at `c` with `enabled.len()` tools: the first one on the way, so a fast stroke
/// between two frames still takes the tool it began toward. Zones that are off are passed over.
pub(crate) fn entered(c: Pos2, from: Pos2, to: Pos2, enabled: &[bool]) -> Option<usize> {
    if !(c.is_finite() && from.is_finite() && to.is_finite()) {
        return None;
    }
    let n = enabled.len();
    let on = |p: Pos2| sector(p - c, n).filter(|k| enabled.get(*k).copied().unwrap_or(false));
    let (f, d) = (from - c, to - from);
    // Leaving the hub: the zone is the one at the exit point, which a sample could step past.
    let a = d.dot(d);
    if f.length() < HUB && a > 1e-9 {
        let (b, cc) = (2.0 * f.dot(d), f.dot(f) - HUB * HUB);
        let disc = b * b - 4.0 * a * cc;
        if disc >= 0.0 {
            let t = (-b + disc.sqrt()) / (2.0 * a);
            if (0.0..=1.0).contains(&t) {
                // A hair past the edge, so rounding can't leave the point in the hub.
                if let Some(k) = on(c + (f + d * t) * 1.001) {
                    return Some(k);
                }
            }
        }
    }
    (1..=SAMPLES).find_map(|i| on(from + d * (i as f32 / SAMPLES as f32)))
}

/// Which of the three tints zone `k` of `n` takes: 0 and 1 alternate, and when `n` is odd the
/// last zone takes 2, so it differs from both its neighbours (the one before it and zone 0).
pub(crate) fn tint(n: usize, k: usize) -> usize {
    if n % 2 == 1 && n > 1 && k == n - 1 { 2 } else { k % 2 }
}

/// The three tints, from the theme.
fn palette(t: &Tokens) -> [Color32; 3] {
    [t.panel, t.panel.lerp_to_gamma(t.text, 0.30), t.panel.lerp_to_gamma(t.text, 0.55)]
}

/// How far (points) the pointer moves with the right button held before the ring opens; a press
/// that stays inside this is a right-click.
pub(crate) const OPEN_DRAG: f32 = 4.0;

/// Where a ring opened by a press at `at` is centred: there, or as near as keeps it on screen.
fn centre_for(at: Pos2, screen: Rect) -> Pos2 {
    let m = RING + 6.0;
    let x = if screen.width() > 2.0 * m { at.x.clamp(screen.left() + m, screen.right() - m) } else { screen.center().x };
    let y = if screen.height() > 2.0 * m { at.y.clamp(screen.top() + m, screen.bottom() - m) } else { screen.center().y };
    pos2(x, y)
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

/// Draws the ring centred at `c`, `scale` times its real size: one zone per tool, tinted
/// alternately, the zone the pointer is in lit, an icon in each, and in the hub the lit tool's
/// name (the studio's when none is).
pub(crate) fn paint(painter: &Painter, c: Pos2, scale: f32, studio: Studio, items: &[Item], pointer: Option<Pos2>, t: Tokens) {
    let n = items.len();
    if n == 0 {
        return;
    }
    let (hub, ring) = (HUB * scale, RING * scale);
    let hot = pointer.and_then(|p| sector((p - c) / scale, n));
    let tints = palette(&t);
    let step = 360.0 / n as f32;
    let gap = if n > 1 { GAP } else { 0.0 };
    painter.circle(c, hub, t.panel.gamma_multiply(0.5), Stroke::new(1.0, t.border));
    for (k, item) in items.iter().enumerate() {
        let lit = item.enabled && hot == Some(k);
        let base = tints[tint(n, k)];
        let (fill, edge) = match (item.enabled, lit) {
            (_, true) => (t.accent_soft, t.accent),
            (true, false) => (base, t.border),
            (false, false) => (base.gamma_multiply(0.6), t.border.gamma_multiply(0.7)),
        };
        let mid = k as f32 * step;
        zone(painter, c, hub, ring, mid - step / 2.0 + gap / 2.0, mid + step / 2.0 - gap / 2.0, fill, edge);
        if !item.icon.is_empty() {
            let a = mid.to_radians();
            let at = c + vec2(a.sin(), -a.cos()) * (hub + ring) / 2.0;
            let ink = if item.enabled { t.icon } else { t.border };
            let side = (24.0 * scale).max(12.0);
            crate::icons::paint_icon(painter, Rect::from_center_size(at, vec2(side, side)), &item.icon, f32::from(ink.a()) / 255.0);
        }
    }
    match hot.and_then(|k| items.get(k)).filter(|i| i.enabled) {
        Some(item) => painter.text(
            c,
            Align2::CENTER_CENTER,
            crate::i18n::tr_label(&item.label),
            FontId::proportional((t.font_title * scale).max(t.font_badge)),
            t.text,
        ),
        None => painter.text(
            c,
            Align2::CENTER_CENTER,
            crate::i18n::tr_label(studio.label()),
            FontId::proportional((t.font_intro * scale).max(t.font_badge)),
            t.text_dim,
        ),
    };
}

/// One zone of the ring, between `a0` and `a1` degrees clockwise from the top.
fn zone(painter: &Painter, c: Pos2, inner: f32, outer: f32, a0: f32, a1: f32, fill: Color32, edge: Color32) {
    let at = |deg: f32, r: f32| {
        let a = deg.to_radians();
        c + vec2(a.sin(), -a.cos()) * r
    };
    let steps = (((a1 - a0) / 3.0).ceil() as usize).clamp(2, 120);
    let mut mesh = Mesh::default();
    let mut outline: Vec<Pos2> = Vec::with_capacity(2 * (steps + 1));
    for i in 0..=steps {
        let deg = a0 + (a1 - a0) * i as f32 / steps as f32;
        mesh.colored_vertex(at(deg, inner), fill);
        mesh.colored_vertex(at(deg, outer), fill);
        outline.push(at(deg, outer));
        if i < steps {
            let k = 2 * i as u32;
            mesh.add_triangle(k, k + 1, k + 2);
            mesh.add_triangle(k + 1, k + 3, k + 2);
        }
    }
    for i in (0..=steps).rev() {
        outline.push(at(a0 + (a1 - a0) * i as f32 / steps as f32, inner));
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::closed_line(outline, Stroke::new(1.0, edge)));
}

// ---------------------------------------------------------------------------------------------
// The gesture
// ---------------------------------------------------------------------------------------------

/// What gestures keep between frames.
#[derive(Default)]
pub struct State {
    /// The ring that is open.
    wheel: Option<Wheel>,
    /// Where the right button went down, until the pointer has moved [`OPEN_DRAG`] from it.
    press: Option<Pos2>,
    /// The right button that is down (or was last) opened a ring: its release is no click.
    used: bool,
    /// The customization window.
    pub(crate) editor: Editor,
}

struct Wheel {
    centre: Pos2,
    studio: Studio,
    /// The pointer on the last frame: the move since then is what may touch a zone.
    from: Pos2,
    /// The pointer has been in the hub since the ring opened, so a zone it is in is one it moved
    /// into (a ring pushed off the screen's edge opens with the pointer outside its hub).
    armed: bool,
}

/// One frame of gestures: opens, runs and closes the ring, draws it, and shows the editor.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    track(app, ctx);
    paint_open(app, ctx);
    editor(app, ctx);
}

/// The right button last pressed opened the ring, so letting go of it is not a right-click.
pub(crate) fn took_press(app: &SolveApp) -> bool {
    app.gesture.used
}

/// Whether a ring is open.
#[cfg(test)]
pub(crate) fn is_open(app: &SolveApp) -> bool {
    app.gesture.wheel.is_some()
}

fn track(app: &mut SolveApp, ctx: &egui::Context) {
    if !active(app) {
        app.gesture.wheel = None;
        app.gesture.press = None;
        app.gesture.used = false;
        return;
    }
    let (pressed, down, pos, esc) = ctx.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Secondary),
            i.pointer.secondary_down(),
            i.pointer.latest_pos(),
            i.key_pressed(egui::Key::Escape),
        )
    });
    // Letting go, or Esc, leaves nothing half-way.
    if !down || esc {
        app.gesture.wheel = None;
        app.gesture.press = None;
        return;
    }
    let Some(pos) = pos else { return };
    // A press in the viewport waits to see whether it is a click or the start of a gesture.
    if pressed {
        app.gesture.used = false;
        app.gesture.press = (app.gesture.wheel.is_none() && app.viewport.mouse.is_some()).then_some(pos);
    }
    // Held and moved far enough: the ring opens where the button went down, whatever the pointer
    // carries (a tool or a dialog in progress included).
    if let Some(at) = app.gesture.press
        && (pos - at).length() >= OPEN_DRAG
    {
        app.gesture.press = None;
        let studio = Studio::of(app);
        if !items(app, studio).is_empty() {
            let centre = centre_for(at, ctx.content_rect());
            app.gesture.wheel = Some(Wheel { centre, studio, from: at, armed: (at - centre).length() < HUB });
            app.gesture.used = true;
        }
    }
    let Some((centre, studio, from, armed)) = app.gesture.wheel.as_mut().map(|w| {
        let from = w.from;
        w.from = pos;
        w.armed |= (from - w.centre).length() < HUB;
        (w.centre, w.studio, from, w.armed)
    }) else {
        return;
    };
    if !armed {
        return;
    }
    let list = items(app, studio);
    let enabled: Vec<bool> = list.iter().map(|i| i.enabled).collect();
    if let Some(k) = entered(centre, from, pos, &enabled)
        && let Some(tool) = list.get(k)
    {
        app.gesture.wheel = None;
        crate::context_menu::run_item(app, tool, centre);
    }
}

fn paint_open(app: &SolveApp, ctx: &egui::Context) {
    let Some(wheel) = &app.gesture.wheel else { return };
    let list = items(app, wheel.studio);
    let t = Tokens::get();
    let pointer = ctx.input(|i| i.pointer.latest_pos());
    let area = Rect::from_center_size(wheel.centre, vec2(2.0 * (RING + 6.0), 2.0 * (RING + 6.0)));
    Container::new(ctx, Surface::Canvas).area(egui::Id::new("sc_mouse_gesture")).fixed_pos(area.min).order(egui::Order::Foreground).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(area.size(), Sense::hover());
        paint(&ui.painter_at(r.expand(4.0)), wheel.centre, 1.0, wheel.studio, &list, pointer, t);
    });
}

// ---------------------------------------------------------------------------------------------
// Customization
// ---------------------------------------------------------------------------------------------

/// The customization window's state.
pub(crate) struct Editor {
    pub(crate) open: bool,
    pub(crate) studio: Studio,
}

impl Default for Editor {
    fn default() -> Self {
        Editor { open: false, studio: Studio::Part }
    }
}

/// A change to a list of tools.
#[derive(Clone, Debug, PartialEq)]
enum Op {
    Up(usize),
    Down(usize),
    Remove(usize),
    Add,
    Set(usize, String),
}

/// The first tool of the catalog that `list` doesn't hold.
fn next_unused(list: &[String]) -> Option<String> {
    catalog().into_iter().map(|(id, _)| id).find(|id| !list.contains(id))
}

/// Applies `op` to `list`; false when it was refused (a ring keeps 1 to [`MAX_TOOLS`] tools, each
/// of them known and none twice).
fn apply(list: &mut Vec<String>, op: Op) -> bool {
    match op {
        Op::Up(i) if i > 0 && i < list.len() => {
            list.swap(i, i - 1);
            true
        }
        Op::Down(i) if i + 1 < list.len() => {
            list.swap(i, i + 1);
            true
        }
        Op::Remove(i) if list.len() > 1 && i < list.len() => {
            list.remove(i);
            true
        }
        Op::Add if list.len() < MAX_TOOLS => match next_unused(list) {
            Some(id) => {
                list.push(id);
                true
            }
            None => false,
        },
        Op::Set(i, id) if i < list.len() && label_of(&id).is_some() && !list.contains(&id) => {
            list[i] = id;
            true
        }
        _ => false,
    }
}

/// The rows of Preferences › Navigation that belong to gestures: the switch, the button that
/// opens the editor, and what the mouse mode has to be.
pub(crate) fn nav_rows(app: &mut SolveApp, ui: &mut egui::Ui, t: &Tokens) {
    crate::ui_kit::settings_block(ui, crate::i18n::tr_label("Mouse gestures"), t.space_24 * 6.0 + t.space_6, |ui| {
        ui.horizontal_wrapped(|ui| {
            crate::ui_kit::checkbox(ui, crate::i18n::tr_label("Ring of tools on the right button"), &mut app.preferences.mouse_gestures.enabled);
            let customize = crate::ui_kit::button(ui, crate::i18n::tr_label("Customize…"), crate::ui_kit::Button::Normal);
            crate::scenario::publish_handle("gestures:customize", customize.rect.center());
            if customize.clicked() {
                app.gesture.editor.open = true;
                app.gesture.editor.studio = Studio::of(app);
            }
        });
        if app.preferences.nav != MOUSE_MODE {
            crate::ui_kit::text(
                ui,
                crate::i18n::tr_label("Mouse gestures only work with the SolidWorks mouse mode (the wheel orbits, Ctrl + wheel pans)."),
                crate::ui_kit::Text::Hint("mouse_gesture_mode"),
            );
            if crate::ui_kit::button(ui, crate::i18n::tr_label("Use the SolidWorks mouse mode"), crate::ui_kit::Button::Normal).clicked() {
                crate::prefs::choose_nav_preset(&mut app.preferences, crate::prefs::nav_preset(MOUSE_MODE));
            }
        } else if app.preferences.mouse_gestures.enabled {
            crate::ui_kit::text(
                ui,
                crate::i18n::tr_label(
                    "With gestures on, the right button no longer orbits the view: hold it and move to open the ring, then touch a tool's zone. A plain right-click still opens the menu.",
                ),
                crate::ui_kit::Text::Hint("mouse_gesture_help"),
            );
        }
    });
}

/// The customization window: pick a studio, then how many tools its ring holds, which, and in
/// which place (clockwise from the top), with a preview of the ring.
fn editor(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.gesture.editor.open {
        return;
    }
    let t = Tokens::get();
    let mut studio = app.gesture.editor.studio;
    let mut config = app.preferences.mouse_gestures.clone();
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_gesture_editor"), "settings", crate::i18n::tr_label("Mouse gestures"))
        .cards(3.0)
        .show_body(ctx, |ui| {
            crate::ui_kit::checkbox(ui, crate::i18n::tr_label("Ring of tools on the right button"), &mut config.enabled);
            if app.preferences.nav != MOUSE_MODE {
                ui.colored_label(
                    t.warning,
                    crate::i18n::tr_label("Mouse gestures only work with the SolidWorks mouse mode (the wheel orbits, Ctrl + wheel pans)."),
                );
            }
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr_label("Studio"));
                ui.scope(|ui| {
                    ui.set_max_width(200.0);
                    crate::ui_kit::dropdown_menu(ui, "gesture_studio", crate::i18n::tr_label(studio.label()), |ui| {
                        for s in Studio::ALL {
                            if crate::ui_kit::list_picker(ui, s.label(), crate::i18n::tr_label(s.label()), studio == s).clicked() {
                                studio = s;
                            }
                        }
                    });
                });
                if crate::ui_kit::button(
                    ui,
                    crate::i18n::tr_label("Restore this studio's tools"),
                    crate::ui_kit::Button::Enabled(config.is_custom(studio)),
                )
                .clicked()
                {
                    config.reset(studio);
                }
            });
            ui.separator();
            let mut list = config.tools(studio);
            let catalog = catalog();
            let mut op: Option<Op> = None;
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(330.0);
                    ui.label(
                        egui::RichText::new(format!("{} {}/{}", crate::i18n::tr_label("Tools, clockwise from the top"), list.len(), MAX_TOOLS))
                            .size(t.font_small)
                            .color(t.text_dim),
                    );
                    for i in 0..list.len() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}", i + 1));
                            let shown = label_of(&list[i]).unwrap_or_default();
                            ui.scope(|ui| {
                                ui.set_max_width(200.0);
                                let picker = crate::ui_kit::dropdown_menu_with_close_behavior(
                                    ui,
                                    ("gesture_tool", i),
                                    &shown,
                                    egui::PopupCloseBehavior::CloseOnClickOutside,
                                    |ui| {
                                        let filter_id = ui.id().with("filter");
                                        let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id).unwrap_or_default());
                                        crate::ui_kit::text_field(ui, &mut filter, crate::i18n::tr_label("Filter…"));
                                        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
                                        let needle = filter.trim().to_lowercase();
                                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                                            for (id, label) in &catalog {
                                                if !needle.is_empty()
                                                    && !label.to_lowercase().contains(&needle)
                                                    && !id.to_lowercase().contains(&needle)
                                                {
                                                    continue;
                                                }
                                                let taken = list.contains(id) && *id != list[i];
                                                let r =
                                                    ui.add_enabled_ui(!taken, |ui| crate::ui_kit::list_picker(ui, id, label, *id == list[i])).inner;
                                                if r.clicked() {
                                                    op = Some(Op::Set(i, id.clone()));
                                                    ui.close();
                                                }
                                            }
                                        });
                                    },
                                );
                                crate::scenario::publish_target(&format!("gestures:tool:{i}"), picker.rect);
                            });
                            if crate::ui_kit::button(ui, "↑", crate::ui_kit::Button::Enabled(i > 0)).clicked() {
                                op = Some(Op::Up(i));
                            }
                            if crate::ui_kit::button(ui, "↓", crate::ui_kit::Button::Enabled(i + 1 < list.len())).clicked() {
                                op = Some(Op::Down(i));
                            }
                            if crate::ui_kit::button(ui, "×", crate::ui_kit::Button::Enabled(list.len() > 1)).clicked() {
                                op = Some(Op::Remove(i));
                            }
                        });
                    }
                    if crate::ui_kit::button(ui, crate::i18n::tr_label("Add a tool"), crate::ui_kit::Button::Enabled(list.len() < MAX_TOOLS))
                        .clicked()
                    {
                        op = Some(Op::Add);
                    }
                });
                ui.separator();
                // The ring as it will look, a little smaller.
                let scale = 0.62;
                let side = 2.0 * RING * scale + 8.0;
                let (r, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
                // Every tool as available, whatever the design allows now.
                let preview: Vec<Item> = list
                    .iter()
                    .filter_map(|id| item(app, id))
                    .map(|mut i| {
                        i.enabled = true;
                        i
                    })
                    .collect();
                paint(&ui.painter_at(r), r.center(), scale, studio, &preview, None, t);
            });
            if let Some(op) = op
                && apply(&mut list, op)
            {
                config.set_tools(studio, list);
            }
        });
    app.gesture.editor.studio = studio;
    app.preferences.mouse_gestures = config;
    app.gesture.editor.open = !closed;
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn app() -> SolveApp {
        SolveApp::new(solidcraft_engine::Session::default(), Default::default())
    }

    fn on(n: usize) -> Vec<bool> {
        vec![true; n]
    }

    #[test]
    fn the_ring_is_280_inside_and_350_outside() {
        assert_eq!(HUB * 2.0, 280.0);
        assert_eq!(RING * 2.0, 350.0);
    }

    #[test]
    fn a_ring_of_n_tools_is_n_equal_parts_of_the_circle() {
        for n in 1..=MAX_TOOLS {
            let mut seen = vec![0usize; n];
            // Walk the whole circle: every zone is met, none is missing, and each is the same size.
            for deg in 0..3600 {
                let a = (deg as f32 / 10.0).to_radians();
                let k = sector(vec2(a.sin(), -a.cos()) * (HUB + 10.0), n).expect("outside the hub");
                seen[k] += 1;
            }
            let (lo, hi) = (seen.iter().min().copied().unwrap_or(0), seen.iter().max().copied().unwrap_or(0));
            assert!(lo > 0, "{n} tools: a part of the circle has no tool: {seen:?}");
            assert!(hi - lo <= 2, "{n} tools: parts of different sizes: {seen:?}");
        }
    }

    #[test]
    fn zones_are_centred_clockwise_from_the_top_and_the_hub_has_none() {
        let far = |x: f32, y: f32| vec2(x, y) * 400.0;
        for (n, expect) in [(4, [0, 1, 2, 3]), (8, [0, 2, 4, 6])] {
            let got = [sector(far(0.0, -1.0), n), sector(far(1.0, 0.0), n), sector(far(0.0, 1.0), n), sector(far(-1.0, 0.0), n)];
            let want: Vec<Option<usize>> = if n == 4 { expect.iter().map(|k| Some(*k)).collect() } else { expect.iter().map(|k| Some(*k)).collect() };
            assert_eq!(got.to_vec(), want, "{n} tools");
        }
        assert_eq!(sector(vec2(0.0, -(HUB - 1.0)), 5), None);
        assert_eq!(sector(vec2(0.0, -(HUB + 1.0)), 5), Some(0));
        assert_eq!(sector(vec2(f32::NAN, 200.0), 5), None);
        assert_eq!(sector(vec2(0.0, -200.0), 0), None, "no tools, no zones");
        assert_eq!(sector(vec2(0.0, -9000.0), 5), Some(0), "a zone reaches outward without limit");
        // Five tools: 72 degrees each, so the zone to the right (90 degrees) is the second.
        assert_eq!(sector(far(1.0, 0.0), 5), Some(1));
    }

    #[test]
    fn no_two_zones_side_by_side_share_a_tint() {
        for n in 2..=MAX_TOOLS {
            for k in 0..n {
                assert_ne!(tint(n, k), tint(n, (k + 1) % n), "{n} tools: zones {k} and {} match", (k + 1) % n);
            }
            let used = (0..n).map(|k| tint(n, k)).collect::<std::collections::BTreeSet<_>>().len();
            assert_eq!(used, if n % 2 == 1 { 3 } else { 2 }, "{n} tools: two tints, three when odd");
        }
        assert_eq!((0..5).map(|k| tint(5, k)).collect::<Vec<_>>(), [0, 1, 0, 1, 2]);
        assert_eq!((0..3).map(|k| tint(3, k)).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(tint(1, 0), 0);
    }

    #[test]
    fn the_three_tints_are_three_different_colours() {
        for t in [Tokens::dark(), Tokens::light()] {
            let p = palette(&t);
            assert!(p[0] != p[1] && p[1] != p[2] && p[0] != p[2], "{p:?}");
        }
    }

    #[test]
    fn touching_a_zone_takes_it_as_the_pointer_leaves_the_hub() {
        let c = pos2(500.0, 500.0);
        assert_eq!(entered(c, c, c + vec2(0.0, HUB - 2.0), &on(8)), None, "still in the hub");
        assert_eq!(entered(c, c, c + vec2(0.0, HUB + 2.0), &on(8)), Some(4), "two points past the hub's edge");
        assert_eq!(entered(c, c, c + vec2(HUB + 2.0, 0.0), &on(5)), Some(1));
    }

    #[test]
    fn a_fast_stroke_takes_the_zone_it_left_the_hub_into() {
        let c = pos2(0.0, 0.0);
        assert_eq!(entered(c, c, pos2(900.0, -300.0), &on(8)), Some(2), "the exit point decides, not the far end");
        assert_eq!(entered(c, c, pos2(300.0, -900.0), &on(8)), Some(0));
        assert_eq!(entered(c, c, pos2(-900.0, 900.0), &on(8)), Some(5));
        assert_eq!(entered(c, pos2(-10.0, 0.0), pos2(-600.0, 40.0), &on(8)), Some(6));
    }

    #[test]
    fn a_zone_that_is_off_is_passed_over() {
        let c = pos2(0.0, 0.0);
        let mut flags = on(8);
        flags[4] = false;
        assert_eq!(entered(c, c, pos2(0.0, 400.0), &flags), None, "straight down is off");
        // Sliding round the ring from the off zone into an enabled one takes the enabled one.
        assert_eq!(entered(c, pos2(0.0, 180.0), pos2(-180.0, 180.0), &flags), Some(5));
        assert_eq!(entered(c, c, pos2(0.0, 400.0), &[]), None);
    }

    #[test]
    fn hostile_numbers_touch_nothing() {
        let c = pos2(0.0, 0.0);
        for p in [pos2(f32::NAN, 0.0), pos2(f32::INFINITY, 5.0), pos2(0.0, f32::NEG_INFINITY)] {
            assert_eq!(entered(c, c, p, &on(8)), None);
            assert_eq!(entered(c, p, c, &on(8)), None);
            assert_eq!(entered(p, c, pos2(0.0, 400.0), &on(8)), None);
        }
        assert_eq!(entered(c, c, c, &on(8)), None, "a pointer that did not move");
    }

    #[test]
    fn studios_are_read_from_the_application() {
        let mut app = app();
        assert_eq!(Studio::of(&app), Studio::Part);
        for (tab, studio) in [
            ("SOLID", Studio::Part),
            ("SURFACE", Studio::Surface),
            ("MESH", Studio::Mesh),
            ("SHEET METAL", Studio::SheetMetal),
            ("PLASTIC", Studio::Plastic),
            ("UTILITIES", Studio::Part),
        ] {
            app.ui.tab = tab.into();
            assert_eq!(Studio::of(&app), studio, "{tab}");
        }
        app.ui.tab = "SURFACE".into();
        app.session.execute("sketch.create", &serde_json::json!({"plane": "XY"})).expect("a sketch opens");
        assert_eq!(Studio::of(&app), Studio::Sketch, "a sketch being edited beats the tab");
    }

    #[test]
    fn every_studio_ships_distinct_tools_that_exist() {
        let app = app();
        let mut sets = Vec::new();
        for studio in Studio::ALL {
            let ids = studio.defaults();
            let mut sorted = ids.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), ids.len(), "{studio:?}: no tool twice");
            for id in ids {
                assert!(label_of(id).is_some() && item(&app, id).is_some(), "{studio:?}: `{id}` is not a tool");
            }
            assert_eq!(items(&app, studio).len(), ids.len());
            sets.push(ids.to_vec());
        }
        for (i, a) in sets.iter().enumerate() {
            for b in &sets[i + 1..] {
                assert_ne!(a, b, "two studios share one ring");
            }
        }
        let keys: std::collections::BTreeSet<_> = Studio::ALL.iter().map(|s| s.key()).collect();
        assert_eq!(keys.len(), Studio::ALL.len(), "studio keys are unique");
    }

    #[test]
    fn repeat_delete_and_undo_follow_the_state_of_the_design() {
        let mut app = app();
        assert!(!item(&app, "repeat").expect("a tool").enabled, "nothing to repeat yet");
        assert!(!item(&app, "delete").expect("a tool").enabled, "nothing selected");
        assert!(!item(&app, "undo").expect("a tool").enabled, "nothing to undo");
        app.session.execute("solid.box", &serde_json::json!({"length": 10, "width": 10, "height": 10})).expect("a box");
        assert!(item(&app, "undo").expect("a tool").enabled, "a command ran: undo is on");
        assert!(item(&app, "no.such.tool").is_none());
    }

    #[test]
    fn a_studio_uses_its_defaults_until_the_person_changes_them() {
        let mut config = Config::default();
        assert!(!config.enabled, "gestures start off");
        assert_eq!(config.tools(Studio::Part), PART.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(!config.is_custom(Studio::Part));
        config.set_tools(Studio::Part, vec!["solid.extrude".into(), "undo".into(), "solid.hole".into(), "delete".into(), "repeat".into()]);
        assert_eq!(config.tools(Studio::Part).len(), 5, "five tools: five parts of the circle");
        assert_eq!(config.tools(Studio::Part)[0], "solid.extrude");
        assert!(config.is_custom(Studio::Part) && !config.is_custom(Studio::Sketch));
        assert_eq!(config.tools(Studio::Sketch).len(), 8, "other studios keep their own");
        config.reset(Studio::Part);
        assert_eq!(config.tools(Studio::Part).len(), 8);
    }

    #[test]
    fn a_hand_edited_list_is_cleaned_before_it_is_used() {
        let mut config = Config::default();
        let many: Vec<String> = (0..40).map(|_| "undo".to_string()).chain(catalog().into_iter().map(|(id, _)| id)).collect();
        config.set_tools(Studio::Mesh, many);
        let tools = config.tools(Studio::Mesh);
        assert!(tools.len() <= MAX_TOOLS, "{} tools", tools.len());
        let mut sorted = tools.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), tools.len(), "no tool twice");
        config.set_tools(Studio::Mesh, vec!["nonsense".into(), "".into(), "also.not.a.tool".into()]);
        assert_eq!(config.tools(Studio::Mesh), MESH.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "nothing usable: the defaults");
        config.set_tools(Studio::Mesh, vec![]);
        assert_eq!(config.tools(Studio::Mesh).len(), 8);
    }

    #[test]
    fn settings_round_trip_and_older_preferences_load() {
        let mut config = Config { enabled: true, ..Default::default() };
        config.set_tools(Studio::Sketch, vec!["sketch.line".into(), "undo".into(), "sketch.circle.center".into()]);
        let json = serde_json::to_string(&config).expect("serializes");
        assert_eq!(serde_json::from_str::<Config>(&json).expect("loads"), config);
        let none: Config = serde_json::from_str("{}").expect("an empty value is the default");
        assert_eq!(none, Config::default());
        assert!(
            serde_json::from_str::<Config>(r#"{"enabled":true,"wheels":{"part":["undo"],"future":["x"]}}"#).is_ok(),
            "unknown studios are kept, not fatal"
        );
    }

    #[test]
    fn customization_uses_the_shared_header_keeps_edits_and_has_no_resize_edges() {
        use crate::scenario::{Harness, handle_at, rect_of};
        use serde_json::json;
        for dark in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", json!({"width":1400,"height":900}));
            h.call("ui.set", json!({"dark":dark}));
            h.app.preferences.mouse_gestures.set_tools(Studio::Part, vec!["undo".into(), "solid.extrude".into()]);
            h.app.gesture.editor.open = true;
            h.app.gesture.editor.studio = Studio::Part;
            h.frames(6);
            let header = rect_of("dialog:header").unwrap();
            assert_eq!(header.height(), Tokens::get().form_header_height);
            assert_eq!(h.app.preferences.mouse_gestures.tools(Studio::Part).len(), 2);
            h.step(&json!({"widget":"Add a tool"})).unwrap();
            h.frames(4);
            assert_eq!(h.app.preferences.mouse_gestures.tools(Studio::Part).len(), 3);
            let window = rect_of("dialog:window").unwrap();
            for at in [window.left_center(), window.right_center(), window.right_bottom()] {
                h.call("ui.move", json!({"x":at.x,"y":at.y}));
                h.frames(2);
                assert!(!matches!(
                    h.output.cursor_icon,
                    egui::CursorIcon::ResizeHorizontal
                        | egui::CursorIcon::ResizeVertical
                        | egui::CursorIcon::ResizeNwSe
                        | egui::CursorIcon::ResizeNeSw
                ));
            }
            let close = handle_at("dialog:close").unwrap();
            h.call("ui.click", json!({"x":close.x,"y":close.y}));
            h.frames(2);
            assert!(!h.app.gesture.editor.open);
            assert_eq!(h.app.preferences.mouse_gestures.tools(Studio::Part).len(), 3, "closing retains edits");
        }
    }

    #[test]
    fn tool_search_keeps_focus_after_click_and_closes_only_when_finished() {
        use crate::scenario::{Harness, handle_at};
        use crate::test_support::{click, pointer};
        use serde_json::json;

        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.mouse_gestures.set_tools(Studio::Part, vec!["undo".into(), "solid.extrude".into()]);
        h.app.gesture.editor.open = true;
        h.app.gesture.editor.studio = Studio::Part;
        h.frames(6);
        click(&mut h, "gestures:tool:0");
        let search = h
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Filter…" => Some(text.pos + text.galley.size() * 0.5),
                _ => None,
            })
            .expect("the picker displays its search field");
        h.frame_with(pointer(search, true));
        h.frame_with(pointer(search, false));
        h.frames(2);
        assert!(egui::Popup::is_any_open(&h.ctx), "releasing a click in the search field must keep the picker open");

        h.step(&json!({"text":"solid.extrude"})).unwrap();
        h.frames(2);
        assert!(handle_at("dialog:list:solid.extrude").is_some(), "search matches command ids");
        assert!(handle_at("dialog:list:solid.box").is_none(), "unmatched tools are filtered out");
        click(&mut h, "dialog:list:solid.extrude");
        assert!(egui::Popup::is_any_open(&h.ctx), "a tool already in another slot is disabled");
        assert_eq!(h.app.preferences.mouse_gestures.tools(Studio::Part), ["undo", "solid.extrude"]);

        h.frame_with(pointer(search, true));
        h.frame_with(pointer(search, false));
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"Box"})).unwrap();
        h.frames(2);
        click(&mut h, "dialog:list:solid.box");
        assert!(!egui::Popup::is_any_open(&h.ctx), "choosing a tool closes the picker");
        assert_eq!(h.app.preferences.mouse_gestures.tools(Studio::Part), ["solid.box", "solid.extrude"]);

        click(&mut h, "gestures:tool:0");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert!(!egui::Popup::is_any_open(&h.ctx), "Escape dismisses the picker");
        click(&mut h, "gestures:tool:0");
        h.frame_with(pointer(egui::pos2(20.0, 400.0), true));
        h.frame_with(pointer(egui::pos2(20.0, 400.0), false));
        h.frames(2);
        assert!(!egui::Popup::is_any_open(&h.ctx), "clicking outside dismisses the picker");
    }

    #[test]
    fn the_editor_changes_how_many_tools_and_where_they_sit() {
        let mut list: Vec<String> = ["solid.extrude", "undo", "solid.hole"].iter().map(|s| s.to_string()).collect();
        assert!(apply(&mut list, Op::Down(0)));
        assert_eq!(list, ["undo", "solid.extrude", "solid.hole"]);
        assert!(apply(&mut list, Op::Up(2)));
        assert_eq!(list, ["undo", "solid.hole", "solid.extrude"]);
        assert!(!apply(&mut list, Op::Up(0)) && !apply(&mut list, Op::Down(2)), "can't move past the ends");
        assert!(apply(&mut list, Op::Add) && list.len() == 4, "a tool is added");
        assert!(!list[3].is_empty() && !list[..3].contains(&list[3]), "one the ring doesn't hold yet");
        assert!(apply(&mut list, Op::Set(0, "solid.fillet".into())));
        assert_eq!(list[0], "solid.fillet");
        assert!(!apply(&mut list, Op::Set(1, "solid.fillet".into())), "no tool twice");
        assert!(!apply(&mut list, Op::Set(1, "not.a.tool".into())), "only real tools");
        assert!(!apply(&mut list, Op::Set(9, "undo".into())), "no such place");
        assert!(apply(&mut list, Op::Remove(0)) && list.len() == 3);
        while list.len() > 1 {
            assert!(apply(&mut list, Op::Remove(0)));
        }
        assert!(!apply(&mut list, Op::Remove(0)), "a ring keeps at least one tool");
        while list.len() < MAX_TOOLS {
            assert!(apply(&mut list, Op::Add));
        }
        assert!(!apply(&mut list, Op::Add), "and at most {MAX_TOOLS}");
    }

    #[test]
    fn the_catalog_offers_real_tools_once_each() {
        let catalog = catalog();
        assert!(catalog.len() > 50);
        let mut ids: Vec<&str> = catalog.iter().map(|(i, _)| i.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "no tool twice");
        assert!(catalog.iter().all(|(id, label)| label_of(id).as_deref() == Some(label.as_str())));
        for studio in Studio::ALL {
            for id in studio.defaults() {
                // Shipped rings also contain planned actions and Render editors without toolbar destinations.
                let offered = solidcraft_engine::find_command(id).is_none_or(|spec| spec.planned.is_none() && !spec.tab.is_empty());
                assert_eq!(catalog.iter().any(|(i, _)| i == id), offered, "{studio:?}: `{id}` must follow the catalog's toolbar-tool policy");
            }
        }
    }

    // ---- the gesture itself ----

    /// A right-button drag through `show`, one pointer position per frame (the first presses).
    /// The button stays down unless `release` is set.
    fn drag(app: &mut SolveApp, path: &[Pos2], release: bool) {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        app.viewport.mouse = path.first().copied();
        let secondary =
            |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE };
        let frame = |app: &mut SolveApp, k: usize, events: Vec<egui::Event>| {
            let raw = egui::RawInput { time: Some(k as f64 * 0.02), screen_rect: Some(screen), events, ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| show(app, ui.ctx()));
            output.textures_delta.clear();
        };
        for (k, p) in path.iter().enumerate() {
            let mut events = vec![egui::Event::PointerMoved(*p)];
            if k == 0 {
                events.push(secondary(*p, true));
            }
            frame(app, k, events);
        }
        if release {
            let last = path.last().copied().unwrap_or(Pos2::ZERO);
            frame(app, path.len(), vec![secondary(last, false)]);
        }
    }

    fn gesture_app() -> SolveApp {
        let mut app = app();
        app.preferences.nav = MOUSE_MODE.into();
        app.preferences.mouse_gestures.enabled = true;
        app
    }

    const PRESS: Pos2 = pos2(800.0, 500.0);

    #[test]
    fn gestures_need_both_the_switch_and_the_solidworks_mouse_mode() {
        let mut app = app();
        assert!(!active(&app), "off by default");
        app.preferences.mouse_gestures.enabled = true;
        for nav in ["fusion", "inventor", "alias", "blender", "tinkercad", "openscad", "touchpad"] {
            app.preferences.nav = nav.into();
            assert!(!active(&app), "{nav}: gestures only work with the SolidWorks mouse mode");
        }
        app.preferences.nav = MOUSE_MODE.into();
        assert!(active(&app));
        app.preferences.mouse_gestures.enabled = false;
        assert!(!active(&app));
    }

    #[test]
    fn the_ring_opens_after_four_points_with_the_button_held_where_it_went_down() {
        let mut app = gesture_app();
        drag(&mut app, &[PRESS], false);
        assert!(!is_open(&app), "a press alone is not a gesture");
        drag(&mut app, &[PRESS, PRESS + vec2(OPEN_DRAG - 1.0, 0.0)], false);
        assert!(!is_open(&app), "three points is still a click");
        let mut app = gesture_app();
        drag(&mut app, &[PRESS, PRESS + vec2(OPEN_DRAG - 1.0, 0.0), PRESS + vec2(OPEN_DRAG, 0.0)], false);
        assert!(is_open(&app), "four points opens it");
        assert_eq!(app.gesture.wheel.as_ref().map(|w| w.centre), Some(PRESS), "centred where the button went down");
        assert!(app.tool.is_none() && app.dialog.is_none(), "nothing is taken inside the hub");
    }

    #[test]
    fn a_plain_right_click_leaves_the_menu_to_the_viewport_and_a_gesture_does_not() {
        let mut app = gesture_app();
        drag(&mut app, &[PRESS], true);
        assert!(!is_open(&app) && !took_press(&app), "no move: a right-click, the usual menu opens");
        let mut app = gesture_app();
        drag(&mut app, &[PRESS, PRESS + vec2(10.0, 0.0)], true);
        assert!(!is_open(&app) && took_press(&app), "a gesture's release is no click, so no menu");
        // The next press starts afresh.
        drag(&mut app, &[PRESS], true);
        assert!(!took_press(&app));
    }

    #[test]
    fn touching_a_zone_takes_the_tool_and_puts_the_ring_away() {
        let mut app = gesture_app();
        // Past the hub's edge, below: the bottom zone (Create Sketch).
        drag(&mut app, &[PRESS, PRESS + vec2(0.0, 20.0), PRESS + vec2(0.0, HUB + 3.0)], false);
        assert!(!is_open(&app));
        assert!(app.tool.is_some() || app.dialog.is_some(), "Create Sketch started");
    }

    #[test]
    fn a_fast_flick_cannot_skip_the_tool_between_frames() {
        let mut app = gesture_app();
        drag(&mut app, &[PRESS, PRESS + vec2(0.0, 12.0), PRESS + vec2(0.0, 420.0)], false);
        assert!(!is_open(&app) && (app.tool.is_some() || app.dialog.is_some()));
    }

    #[test]
    fn a_tool_that_is_off_is_not_taken_and_the_ring_stays_up() {
        let mut app = gesture_app();
        // Repeat, straight up, has nothing to repeat yet.
        drag(&mut app, &[PRESS, PRESS + vec2(0.0, -20.0), PRESS + vec2(0.0, -300.0)], false);
        assert!(is_open(&app) && app.tool.is_none() && app.dialog.is_none());
    }

    #[test]
    fn letting_go_puts_the_ring_away() {
        let mut app = gesture_app();
        drag(&mut app, &[PRESS, PRESS + vec2(0.0, 20.0)], true);
        assert!(!is_open(&app));
        assert!(app.tool.is_none() && app.dialog.is_none());
    }

    #[test]
    fn a_ring_of_five_takes_the_tool_in_the_direction_of_its_zone() {
        let mut app = gesture_app();
        app.preferences
            .mouse_gestures
            .set_tools(Studio::Part, ["undo", "delete", "sketch.create", "solid.extrude", "solid.move"].map(String::from).to_vec());
        assert_eq!(items(&app, Studio::Part).len(), 5);
        // 72 degrees a zone: the second (Delete) is centred at 72 degrees, to the right and up a
        // little. Delete needs a selection, so it is off: nothing is taken and the ring stays.
        let dir = vec2((72.0_f32).to_radians().sin(), -(72.0_f32).to_radians().cos());
        drag(&mut app, &[PRESS, PRESS + dir * 20.0, PRESS + dir * 200.0], false);
        assert!(is_open(&app), "the zone's tool is off");
        // The third (Create Sketch) is centred at 144 degrees: to the right and down.
        let dir = vec2((144.0_f32).to_radians().sin(), -(144.0_f32).to_radians().cos());
        let mut app = gesture_app();
        app.preferences
            .mouse_gestures
            .set_tools(Studio::Part, ["undo", "delete", "sketch.create", "solid.extrude", "solid.move"].map(String::from).to_vec());
        drag(&mut app, &[PRESS, PRESS + dir * 20.0, PRESS + dir * 200.0], false);
        assert!(!is_open(&app) && (app.tool.is_some() || app.dialog.is_some()), "Create Sketch started");
    }

    #[test]
    fn nothing_opens_while_gestures_are_off_or_the_mouse_mode_is_wrong() {
        let drag_down = [PRESS, PRESS + vec2(0.0, 20.0), PRESS + vec2(0.0, 300.0)];
        let mut app = gesture_app();
        app.preferences.mouse_gestures.enabled = false;
        drag(&mut app, &drag_down, false);
        assert!(!is_open(&app) && app.tool.is_none() && app.dialog.is_none());
        let mut app = gesture_app();
        app.preferences.nav = "fusion".into();
        drag(&mut app, &drag_down, false);
        assert!(!is_open(&app) && app.tool.is_none() && app.dialog.is_none());
    }

    #[test]
    fn the_ring_opens_even_while_a_tool_is_in_progress() {
        let mut app = gesture_app();
        app.start("sketch.create");
        assert!(app.tool.is_some() || app.dialog.is_some());
        drag(&mut app, &[PRESS, PRESS + vec2(OPEN_DRAG + 1.0, 0.0)], false);
        assert!(is_open(&app), "a tool in the pointer doesn't keep the right button while gestures are on");
    }

    #[test]
    fn the_radial_menu_is_off_only_while_gestures_are_active() {
        let held = |app: &mut SolveApp| {
            let ctx = egui::Context::default();
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
            app.viewport.mouse = Some(PRESS);
            for k in 0..40 {
                let events = if k == 0 {
                    vec![
                        egui::Event::PointerMoved(PRESS),
                        egui::Event::PointerButton {
                            pos: PRESS,
                            button: egui::PointerButton::Secondary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                } else {
                    vec![egui::Event::PointerMoved(PRESS)]
                };
                let raw = egui::RawInput { time: Some(k as f64 * 0.02), screen_rect: Some(screen), events, ..Default::default() };
                let mut out = ctx.run_ui(raw, |ui| {
                    crate::context_menu::show(app, ui.ctx());
                    show(app, ui.ctx());
                });
                out.textures_delta.clear();
            }
        };
        // Held 0.8 s with the radial menu on, the usual way opens its menu...
        let mut app = app();
        app.preferences.nav = MOUSE_MODE.into();
        app.preferences.radial_context_menu = true;
        held(&mut app);
        assert!(app.viewport.context_menu.is_some(), "gestures off: the radial menu opens as before");
        // ...with gestures on, only the ring.
        let mut app = gesture_app();
        app.preferences.radial_context_menu = true;
        held(&mut app);
        assert!(app.viewport.context_menu.is_none(), "gestures on: no radial menu");
        assert!(!is_open(&app), "held without moving: no ring either");
    }

    #[test]
    fn each_studio_opens_its_own_ring() {
        let mut app = gesture_app();
        let ids = |app: &SolveApp| items(app, Studio::of(app)).into_iter().map(|i| i.id).collect::<Vec<_>>();
        assert!(ids(&app).contains(&"solid.extrude".to_string()));
        app.ui.tab = "SHEET METAL".into();
        let sheet = ids(&app);
        assert!(sheet.contains(&"sheet.flange".to_string()) && !sheet.contains(&"solid.extrude".to_string()));
        app.ui.tab = "PLASTIC".into();
        assert!(ids(&app).contains(&"plastic.snap_fit".to_string()));
    }

    #[test]
    fn the_right_button_stops_orbiting_only_while_gestures_are_active() {
        use crate::prefs::{NavButton, nav_mode};
        let none = egui::Modifiers::NONE;
        let mut app = app();
        app.preferences.nav = MOUSE_MODE.into();
        assert!(nav_mode(&app, NavButton::Right, none).is_some(), "gestures off: the right button orbits as before");
        app.preferences.mouse_gestures.enabled = true;
        assert_eq!(nav_mode(&app, NavButton::Right, none), None, "gestures on: it opens the ring");
        assert_eq!(nav_mode(&app, NavButton::Middle, none), nav_mode(&gesture_app_off(), NavButton::Middle, none), "the other buttons are untouched");
        app.preferences.nav = "fusion".into();
        assert_eq!(
            nav_mode(&app, NavButton::Right, none),
            crate::prefs::nav_preset("fusion").mode(NavButton::Right, none),
            "other mouse modes keep theirs"
        );
    }

    fn gesture_app_off() -> SolveApp {
        let mut app = app();
        app.preferences.nav = MOUSE_MODE.into();
        app
    }
}
