//! Standing up a Y-up file (#3). Many CAD programs save parts with Y up, so a STEP or IGES file
//! from them opens lying on its back in a Z-up design. After such a file opens, SolidCraft
//! offers to stand it up: a quarter turn about X (Y → Z) of the component the file came in as
//! (#1386), so an assembly's parts and instances turn together, in one step that can be undone.
//! Preferences › Design › "Offer to stand up Y-up files" turns the offer off; with Y up as the
//! default orientation it isn't needed. The file is never turned without asking.
//!
//! How it asks depends on the program the header names and on how the bodies rest (#7717):
//! `estimate` weighs their flat area facing down Z against their flat area across Y. A file from
//! an exporter known to save Y up (SolidWorks) gets the "Stand this part up?" dialog with a
//! reason line, or only the note below when its bodies clearly stand on Z already. A file from
//! any other program is often upright already, so by default it gets a small note over the view
//! ("Did this part open the wrong way up?"), which goes by itself after `NOTE_SECONDS` and never
//! blocks input, and only when its bodies seem to lie on their back; Preferences › Design ›
//! "For files from unknown programs" picks the dialog (always, as before) or no offer instead. SolidCraft's own files are never offered.
//! Either says "assembly" when the file brought more than one body or a component tree. The
//! context menu's "Turn Up Axis" fixes it any time.

use serde_json::json;

use crate::SolveApp;
use crate::prefs::StandUpUnknown;
use crate::ui_kit;

/// How long the note stays up when nobody is pointing at it, in seconds.
pub const NOTE_SECONDS: f64 = 10.0;

// Which way a just-opened file seems to stand (`upright`). A part rests on its flat faces: the
// planar area facing down Z (a Z-up file's base) is weighed against the larger of the planar
// areas facing down Y and up Y (a Y-up file's base, or its top when it was saved upside down),
// from the bodies' display triangles.

/// A flat face counts toward a direction when its normal is within this many degrees of it.
pub const FACING_DEGREES: f64 = 10.0;
/// A face counts as flat when every triangle's normal is within this many degrees of its first.
pub const PLANAR_DEGREES: f64 = 1.0;
/// One axis wins when its flat area (or, as the tie-breaker, the box's thinness) is at least this
/// many times the other's; anything closer is unclear.
pub const DOMINANCE: f64 = 1.5;
/// At most this many bodies are looked at, so a huge assembly isn't meshed all at once here.
pub const MAX_BODIES: usize = 64;
/// At most this many triangles are looked at in all; a larger mesh is sampled evenly.
pub const MAX_TRIANGLES: usize = 200_000;

/// Which axis a file's bodies seem to have up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upright {
    /// They rest on faces across Y: the file has Y up and lies on its back here.
    Y,
    /// They rest on faces facing down Z: already the right way up.
    Z,
    /// Nothing says either clearly.
    Unclear,
}

/// The flat area of `mesh` facing down Z, facing down Y and facing up Y, looking at no more than
/// `budget` triangles.
pub fn flat_areas(mesh: &solidcraft_engine::geom::Mesh, budget: usize) -> [f64; 3] {
    use solidcraft_engine::geom::Vec3;
    struct Face {
        first: Vec3,
        area: f64,
        flat: bool,
    }
    let count = mesh.triangles.len();
    if count == 0 || budget == 0 {
        return [0.0; 3];
    }
    let stride = count.div_ceil(budget).max(1);
    let planar = PLANAR_DEGREES.to_radians().cos();
    let mut faces: std::collections::HashMap<u32, Face> = std::collections::HashMap::new();
    for (i, tri) in mesh.triangles.iter().enumerate().step_by(stride) {
        let corner = |k: usize| tri.get(k).and_then(|&v| mesh.positions.get(v as usize)).copied();
        let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else { continue };
        let u = Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let v = Vec3::new(c.x - a.x, c.y - a.y, c.z - a.z);
        let n = u.cross(v);
        let len = n.len();
        if !len.is_finite() || len <= 0.0 {
            continue;
        }
        let unit = Vec3::new(n.x / len, n.y / len, n.z / len);
        let id = mesh.tri_face.get(i).copied().unwrap_or(u32::MAX);
        let face = faces.entry(id).or_insert(Face { first: unit, area: 0.0, flat: true });
        face.area += len * 0.5;
        face.flat &= face.first.dot(unit) >= planar;
    }
    let facing = FACING_DEGREES.to_radians().cos();
    let mut areas = [0.0; 3];
    for face in faces.values().filter(|f| f.flat && f.area.is_finite()) {
        let n = face.first;
        for (i, d) in [-n.z, -n.y, n.y].into_iter().enumerate() {
            if d >= facing
                && let Some(a) = areas.get_mut(i)
            {
                *a += face.area;
            }
        }
    }
    areas
}

/// Which axis is up, from the flat area facing down Z and the larger of those facing down and up
/// Y (`flat_areas`) and, when they are close, the box's size along X, Y and Z: a part lies on its
/// broad side, so the thin axis is up.
pub fn upright(z_down: f64, y_either: f64, size: [f64; 3]) -> Upright {
    if !(z_down.is_finite() && y_either.is_finite()) {
        return Upright::Unclear;
    }
    if y_either > 0.0 && y_either >= DOMINANCE * z_down {
        return Upright::Y;
    }
    if z_down > 0.0 && z_down >= DOMINANCE * y_either {
        return Upright::Z;
    }
    let [x, y, z] = size;
    if [x, y, z].iter().all(|v| v.is_finite() && *v >= 0.0) {
        if y * DOMINANCE <= z && y <= x {
            return Upright::Y;
        }
        if z * DOMINANCE <= y && z <= x {
            return Upright::Z;
        }
    }
    Upright::Unclear
}

/// Which axis the bodies named `bodies` in the current design seem to have up. Looks at most
/// `MAX_BODIES` bodies and `MAX_TRIANGLES` triangles; a body that can't be meshed adds nothing.
pub fn estimate(app: &SolveApp, bodies: &[String]) -> Upright {
    let world = app.session.world_state();
    let mut areas = [0.0_f64; 3];
    let mut bounds = solidcraft_engine::geom::Aabb3::EMPTY;
    let mut budget = MAX_TRIANGLES;
    for body in world.bodies.iter().filter(|b| bodies.contains(&b.name)).take(MAX_BODIES) {
        let mesh = body.mesh();
        for (sum, area) in areas.iter_mut().zip(flat_areas(&mesh, budget)) {
            *sum += area;
        }
        budget = budget.saturating_sub(mesh.triangles.len().min(budget));
        let b = mesh.bounds();
        bounds.extend([b.min, b.max]);
    }
    let size = [bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y, bounds.max.z - bounds.min.z];
    let [z_down, y_down, y_up] = areas;
    upright(z_down, y_down.max(y_up), size)
}

/// The bodies a just-opened file brought (`before`: body names before it opened, none for a
/// new design), and the import's component placement from the command's `result`: offer to
/// stand them up.
pub fn offer(app: &mut SolveApp, path: &str, before: &[String], result: Option<&serde_json::Value>) {
    let io = &solidcraft_engine::io::is_step_path;
    if !(io(path) || solidcraft_engine::io::is_iges_path(path)) || app.preferences.up_axis != "z" {
        return;
    }
    let text = |k: &str| result.and_then(|r| r.get(k)).and_then(serde_json::Value::as_str);
    // SolidCraft's own STEP exports already use Z up.
    if text("system") == Some("SolidCraft") || text("exporter_up") == Some("z") {
        return;
    }
    let bodies: Vec<String> = app.session.world_state().bodies.iter().map(|b| b.name.clone()).filter(|n| !before.contains(n)).collect();
    if bodies.is_empty() {
        return;
    }
    let occurrence = result.and_then(|r| r.get("occurrence")).and_then(serde_json::Value::as_u64);
    if !app.preferences.offer_stand_up {
        return;
    }
    // A header naming a Y-up exporter asks in the dialog, with the reason, unless the bodies
    // clearly stand on Z already (it may have been saved Z up): then only the note. The file is
    // never turned without asking. Any other program gets what Preferences › Design says: the
    // note unless the bodies clearly stand on Z (an unclear shape still gets it, so a Y-up file
    // is never left without an offer), the dialog always, or nothing. Ask is the behaviour from
    // before #7717 for every file: the dialog, without looking.
    let known_y_up = text("exporter_up") == Some("y");
    let note = match app.preferences.stand_up_unknown {
        StandUpUnknown::Ask => false,
        _ if known_y_up => estimate(app, &bodies) == Upright::Z,
        StandUpUnknown::Note if estimate(app, &bodies) != Upright::Z => true,
        StandUpUnknown::Note | StandUpUnknown::Off => return,
    };
    let subcomponents = result.and_then(|r| r.get("subcomponents")).and_then(serde_json::Value::as_u64).unwrap_or(0);
    app.stand_up_reason = known_y_up;
    app.stand_up_assembly = bodies.len() > 1 || subcomponents > 0;
    app.stand_up_note = note;
    app.stand_up_note_since = None;
    app.stand_up = Some(bodies);
    app.stand_up_occurrence = occurrence;
}

/// Turn the offered bodies a quarter turn about X (Y up becomes Z up).
pub fn stand_up(app: &mut SolveApp) -> Result<(), String> {
    app.stand_up_reason = false;
    app.stand_up_assembly = false;
    app.stand_up_note = false;
    app.stand_up_note_since = None;
    let Some(bodies) = app.stand_up.take() else { return Err("no file is waiting to be stood up".into()) };
    match app.stand_up_occurrence.take() {
        // The import's component turns about its parent's origin, contents and all, even when it
        // came in as the design's grounded first part (#2804): it is still being placed.
        Some(occurrence) => {
            app.run("occurrence.move", json!({ "occurrence": occurrence, "axis": [1, 0, 0], "angle": "90 deg", "move_grounded": true }))
        }
        None => app.run("solid.move", json!({ "bodies": bodies, "axis": [1, 0, 0], "angle": "90 deg" })),
    }
    .map_err(|e| e.to_string())?;
    app.animate_view("home");
    Ok(())
}

/// Dismiss the offer and keep the file as it opened ("Leave it", or the note's Close); the
/// preference stays on.
pub fn leave(app: &mut SolveApp) {
    app.stand_up = None;
    app.stand_up_reason = false;
    app.stand_up_occurrence = None;
    app.stand_up_assembly = false;
    app.stand_up_note = false;
    app.stand_up_note_since = None;
}

/// The offer is waiting as the note (drawn by the shared toast). No side effects.
pub(crate) fn note_pending(app: &SolveApp) -> bool {
    app.stand_up_note && app.stand_up.is_some()
}

/// The note due this frame, as its text, for the shared toast (`viewport::transient_messages`) to
/// draw with the buttons `note_labels`. Its clock starts the first frame it is drawn; after
/// `NOTE_SECONDS` without the pointer on it (`note_keep`) it goes, leaving the file as it opened.
pub(crate) fn note_due(app: &mut SolveApp, ctx: &egui::Context) -> Option<String> {
    if !note_pending(app) {
        return None;
    }
    let now = ctx.input(|i| i.time);
    let since = *app.stand_up_note_since.get_or_insert(now);
    let shown = now - since;
    if !shown.is_finite() || shown >= NOTE_SECONDS {
        leave(app);
        return None;
    }
    let text = if app.stand_up_assembly { "Did this assembly open the wrong way up?" } else { "Did this part open the wrong way up?" };
    Some(crate::i18n::tr_label(text).to_string())
}

/// The handles of the note's buttons, in order: the quarter turn, then turning the offer off. They
/// are published for scripts and are the dialog's handles too.
pub const NOTE_HANDLES: [&str; 2] = ["stand_up:stand", "stand_up:never"];

/// The handles of the note's close (×), which leaves the file as it opened (#7971):
/// `toast:stand_up:close`, as every toast's, and `stand_up:dismiss`, the handle of the Close
/// button it replaces, kept for scripts.
pub const NOTE_CLOSE_HANDLES: [&str; 2] = ["toast:stand_up:close", "stand_up:dismiss"];

/// The note's button labels, in the order of `NOTE_HANDLES`.
pub(crate) fn note_labels() -> [String; 2] {
    [crate::i18n::tr_label("Stand it up").to_string(), crate::i18n::tr_label("Don't ask again").to_string()]
}

/// The share of the note's time left now, 1 to 0, for the countdown ring around its close (×).
/// Pointing at it (`note_keep`) starts its time again, so the ring is full while it is pointed at.
pub(crate) fn note_left(app: &SolveApp, ctx: &egui::Context) -> f32 {
    let now = ctx.input(|i| i.time);
    app.stand_up_note_since.map_or(1.0, |since| crate::viewport::countdown_left(since, now, NOTE_SECONDS))
}

/// The note was drawn at `rect`: a pointer on it keeps it from going.
pub(crate) fn note_keep(app: &mut SolveApp, ctx: &egui::Context, rect: egui::Rect) {
    if ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pos| rect.contains(pos)) {
        app.stand_up_note_since = Some(ctx.input(|i| i.time));
    }
}

/// The note's button `index` (of `NOTE_HANDLES`) was clicked.
pub(crate) fn note_act(app: &mut SolveApp, index: usize) {
    match index {
        0 => {
            if let Err(e) = stand_up(app) {
                app.set_status(e, true);
            }
        }
        // As the dialog's Don't ask again: Preferences › Design turns the offer back on.
        1 => {
            leave(app);
            app.preferences.offer_stand_up = false;
        }
        _ => leave(app),
    }
}

/// The offer, under the toolbar, unless it is the note.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.stand_up.is_none() || app.stand_up_note {
        return;
    }
    let title = if app.stand_up_assembly { "Stand this assembly up?" } else { "Stand this part up?" };
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_stand_up"), "move", crate::i18n::tr_label(title))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 140.0))
        .actions("Stand it up", "Leave it", Some("Don't ask again"))
        .show(ctx, true, |ui| {
            ui_kit::text(
                ui,
                crate::i18n::tr_label("Files from many CAD programs have Y up, so they open lying on their back here (Z up)."),
                ui_kit::Text::Report("why", false),
            );
            if app.stand_up_reason {
                ui_kit::text(ui, crate::i18n::tr_label("Exported by SolidWorks, which usually saves Y up"), ui_kit::Text::Report("reason", false));
            }
            ui_kit::text(
                ui,
                crate::i18n::tr_label("Stand it up: a quarter turn about X, as one Move feature you can undo."),
                ui_kit::Text::Report("how", false),
            );
            ui_kit::text(ui, crate::i18n::tr_label("Preferences › Design turns the offer back on"), ui_kit::Text::Note("never"));
        });
    for (handle, key) in [("stand_up:stand", "dialog:ok"), ("stand_up:never", "dialog:apply"), ("stand_up:leave", "dialog:cancel")] {
        if let Some(at) = crate::scenario::handle_at(key) {
            crate::scenario::publish_handle(handle, at);
        }
    }
    match action {
        ui_kit::Action::Ok => {
            if let Err(e) = stand_up(app) {
                app.set_status(e, true);
            }
        }
        ui_kit::Action::Cancel => leave(app),
        ui_kit::Action::Apply => {
            leave(app);
            app.preferences.offer_stand_up = false;
        }
        ui_kit::Action::None if closed => leave(app),
        ui_kit::Action::None => {}
    }
}
