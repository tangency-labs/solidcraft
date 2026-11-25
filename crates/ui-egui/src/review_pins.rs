//! Review pins: the review markups kept beside a design ([review markups](../../../docs/review-markups.md))
//! drawn on the model, with a card beside the chosen pin and the Review list along the right of the view.
//!
//! Review is a window setting, off until `view.review` turns it on (Inspect › Review). While it is on:
//! - a teardrop pin stands at each markup's place, its tip on the point marked: amber and numbered when
//!   open, red when an open one's face is gone, green with a tick when done, grey with a cross when
//!   rejected, faint grey when its face is hidden here (the history marker or another configuration
//!   leaves it out); the chosen one is larger, in the accent colour, with a ring;
//! - **Mark this** beside the navigation bar, and M, pin a new markup: M on what the pointer is on (else
//!   the middle of the view), the button on the next click. The card opens with the note box; nothing is
//!   in the set until Add, and Cancel or Esc leaves nothing;
//! - the card reads a markup: its status, what it is on, who and when, the thread, a reply box, its
//!   pictures, Screenshot, Attach image, Go there, Select, Edit <feature>, Delete, Resolve…, Reject… and
//!   Reopen;
//! - the Review list holds the set, newest first, filtered by All, Open, Done and Rejected; the pins
//!   follow the filter, and its eye hides every pin.
//!
//! The window only reads and writes the set through the `markup.*` commands. It lists the set again
//! when the set's file changes on the disk (an agent closed one), when the model changes (where each
//! face stands now), and after its own writes; in the browser, where there is no file to look at, every
//! two seconds. Which face a new pin means is decided by `markup.add` on the exact geometry: the window
//! sends the body and the point under the pointer, never a mesh triangle's face.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align, Color32, ColorImage, Id, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_theme::Tokens;

use crate::SolveApp;
use crate::ui_kit::{self, BackingTooltipExt as _, Container, Surface};

/// How often the set is listed again where its file can't be looked at (the browser), in seconds.
const BROWSER_POLL: f64 = 2.0;
/// How often the set's file is looked at for a change, in seconds.
const STAMP_POLL: f64 = 0.5;
/// Pictures kept as textures at once.
const THUMBS: usize = 32;
/// A picture's small copy, at most this many points each way.
const THUMB: Vec2 = vec2(96.0, 66.0);
/// The card's width, in points.
const CARD_WIDTH: f32 = 360.0;
/// The Review list's width, in points.
const LIST_WIDTH: f32 = 326.0;

/// Which markups the list and the pins show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Filter {
    All,
    #[default]
    Open,
    Done,
    Rejected,
}

impl Filter {
    fn keeps(self, row: &Row) -> bool {
        match self {
            Filter::All => true,
            Filter::Open => row.status == "open",
            Filter::Done => row.status == "resolved",
            Filter::Rejected => row.status == "rejected",
        }
    }
}

/// One message of a markup's thread, as `markup.list` gives it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Said {
    pub by: String,
    pub at: u64,
    pub text: String,
    pub reviewer: bool,
}

/// Where a markup's anchor stands now, as `markup.list` gives it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Found {
    pub state: String,
    pub at: Option<[f64; 3]>,
    pub was: Option<[f64; 3]>,
    pub why: Option<String>,
}

/// The camera a markup was marked from.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Seen {
    pub target: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub fov: f64,
}

impl Seen {
    /// A markup's view, kept with its angles in degrees, as the camera takes it (radians).
    fn in_radians(self) -> Self {
        Self { yaw: self.yaw.to_radians(), pitch: self.pitch.to_radians(), fov: self.fov.to_radians(), ..self }
    }
}

/// A picture of a markup.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Picture {
    /// Its entry in the design.
    pub file: String,
    pub name: String,
    /// The design holds its bytes (not after an undo past attaching it).
    pub in_design: bool,
}

/// How a closed markup ended.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Ended {
    pub by: String,
    pub reason: String,
}

/// One markup, as `markup.list` gives it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Row {
    pub id: u64,
    pub status: String,
    pub note: String,
    pub thread: Vec<Said>,
    pub anchor: Value,
    pub found: Found,
    pub view: Option<Seen>,
    pub images: Vec<Picture>,
    pub by: String,
    pub made: u64,
    pub outcome: Option<Ended>,
}

/// How a pin looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    Open,
    Gone,
    Done,
    Rejected,
    Hidden,
}

impl Row {
    /// Where its pin stands: where its anchor is now, else where it was.
    pub(crate) fn place(&self) -> Option<Vec3> {
        let at = self.found.at.or(self.found.was).or_else(|| point(self.anchor.get("at")?))?;
        let p = Vec3::new(at[0], at[1], at[2]);
        p.is_finite().then_some(p)
    }

    pub(crate) fn look(&self) -> Look {
        match (self.status.as_str(), self.found.state.as_str()) {
            (_, "hidden") => Look::Hidden,
            ("resolved", _) => Look::Done,
            ("rejected", _) => Look::Rejected,
            (_, "gone") => Look::Gone,
            _ => Look::Open,
        }
    }

    fn kind(&self) -> &str {
        self.anchor.get("kind").and_then(Value::as_str).unwrap_or("")
    }

    fn text_of(&self, key: &str) -> String {
        self.anchor.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
    }

    /// The feature that made what it is on, by its id.
    fn maker(&self) -> Option<u64> {
        match self.kind() {
            // A face names the feature that made it (`F<n>:` in its name); the anchor's maker is the body's.
            "face" => self
                .anchor
                .get("face")
                .and_then(Value::as_str)
                .and_then(|f| solidcraft_engine::doc::persistent_name::FaceName::parse(f).ok())
                .and_then(|f| f.made_by())
                .map(|f| f.0)
                .or_else(|| self.anchor.get("maker").and_then(Value::as_u64))
                .filter(|id| *id != 0),
            "edge" | "body" => self.anchor.get("maker").and_then(Value::as_u64).filter(|id| *id != 0),
            "feature" => self.anchor.get("feature").and_then(Value::as_u64),
            _ => None,
        }
    }
}

fn point(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    match a.as_slice() {
        [x, y, z] => Some([x.as_f64()?, y.as_f64()?, z.as_f64()?]),
        _ => None,
    }
}

/// The set as last listed.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Listing {
    pub set: String,
    pub markups: Vec<Row>,
}

/// What the list was made from: when any of it changes, the set is listed again.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    set: String,
    stamp: Option<(u64, u128)>,
    revision: u64,
    marker: Option<usize>,
    config: Option<String>,
}

/// A new markup, before Add puts it in the set.
#[derive(Clone, Debug)]
pub(crate) struct Draft {
    /// The point marked, on `body` or in space; none for a markup on the view.
    pub at: Option<Vec3>,
    pub body: Option<String>,
    /// The edge of `body` marked, by its lasting name (as `model.edges` lists it).
    pub edge: Option<String>,
    pub view: Value,
    pub text: String,
}

/// A question the card asks before it writes.
#[derive(Clone, Debug, PartialEq)]
enum Ask {
    Resolve(String),
    Reject(String),
    Delete,
}

/// A screenshot asked for a markup: the frame's token, the markup, and the view's place then.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shot {
    pub token: u64,
    pub markup: u64,
    view: Rect,
    scale: f32,
    /// When it is given up (ms): a frame that never comes (a hidden window, a dropped frame)
    /// must not keep the list and the card hidden.
    until: f64,
}

/// How long a screenshot waits for its frame, ms.
const SHOT_WAIT_MS: f64 = 3000.0;

/// Where the list, the card and the Mark this button were drawn last frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Drawn {
    pub list: Option<Rect>,
    pub card: Option<Rect>,
    pub mark: Option<Rect>,
}

/// The window's review state; never saved in the design.
#[derive(Default)]
pub(crate) struct State {
    /// `view.review`: the list, the pins, Mark this and M.
    pub open: bool,
    /// The eye: every pin taken off the model (the list stays).
    pub pins_hidden: bool,
    pub filter: Filter,
    pub chosen: Option<u64>,
    pub listing: Option<Listing>,
    /// Why the set could not be listed.
    pub error: Option<String>,
    pub draft: Option<Draft>,
    /// Mark this was pressed: the next click on the model places the pin.
    pub placing: bool,
    pub shot: Option<Shot>,
    pub drawn: Drawn,
    /// A picture chosen to attach, waiting for the picker to close: the markup, the file.
    attach: Option<(u64, String)>,
    reply: String,
    ask: Option<Ask>,
    key: Option<Key>,
    /// List again on the next frame (after this window wrote).
    stale: bool,
    listed_at: f64,
    stamped_at: f64,
    stamp: Option<(u64, u128)>,
    /// Where the pointer is now (the viewport's own hover stops while a card or panel covers it).
    pointer: Option<Pos2>,
    thumbs: HashMap<String, Option<(egui::TextureHandle, Vec2)>>,
}

impl State {
    pub(crate) fn chosen_row(&self) -> Option<&Row> {
        let id = self.chosen?;
        self.listing.as_ref()?.markups.iter().find(|m| m.id == id)
    }

    fn rows(&self) -> &[Row] {
        self.listing.as_ref().map_or(&[], |l| l.markups.as_slice())
    }

    /// Open, done and rejected counts.
    fn counts(&self) -> [usize; 3] {
        let rows = self.rows();
        let n = |w: &str| rows.iter().filter(|r| r.status == w).count();
        [n("open"), n("resolved"), n("rejected")]
    }
}

/// `view.review` ran: `visible` or, without it, the other way.
pub(crate) fn set_open(app: &mut SolveApp, visible: Option<bool>) {
    let open = visible.unwrap_or(!app.review.open);
    app.review.open = open;
    if open {
        app.review.stale = true;
    } else {
        app.review.draft = None;
        app.review.placing = false;
        app.review.ask = None;
        app.review.drawn = Drawn::default();
    }
}

/// Who writes from this window: the system's user name, else "reviewer".
pub(crate) fn me() -> String {
    ["USER", "USERNAME", "LOGNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok())
        .map(|n| n.trim().to_owned())
        .filter(|n| solidcraft_engine::io::markups::check_person(n).is_ok())
        .unwrap_or_else(|| "reviewer".to_owned())
}

/// The set's file, beside the design, while the design has one.
fn set_of(app: &SolveApp) -> Option<String> {
    app.session.path.as_deref().filter(|p| !p.trim().is_empty()).map(solidcraft_engine::io::markups::beside)
}

/// The file's length and change time, where there is a file system to ask.
fn stamp(set: &str) -> Option<(u64, u128)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let m = std::fs::metadata(set).ok()?;
        let at = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
        Some((m.len(), at))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = set;
        None
    }
}

/// Before the view takes this frame's input: the list, the card and Mark this shield the model under
/// them, and their footprints keep the view's other controls clear. Last frame's places stand in.
pub(crate) fn register(app: &mut SolveApp) {
    let drawn = if app.review.open { app.review.drawn } else { Drawn::default() };
    crate::shell::register_panel(app, "review:list", drawn.list);
    crate::shell::register_panel(app, "review:card", drawn.card);
    crate::shell::register_panel(app, "review:mark", drawn.mark);
}

/// Each frame while Review is on: list the set again when it or the model changed.
pub(crate) fn poll(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.review.open {
        return;
    }
    let now = ctx.input(|i| i.time);
    app.review.pointer = ctx.input(|i| i.pointer.latest_pos());
    let Some(set) = set_of(app) else {
        app.review.listing = None;
        app.review.key = None;
        app.review.error = None;
        return;
    };
    if now - app.review.stamped_at >= STAMP_POLL || app.review.stale {
        app.review.stamp = stamp(&set);
        app.review.stamped_at = now;
    }
    let key = Key {
        set,
        stamp: app.review.stamp,
        revision: app.session.revision,
        marker: app.session.doc.marker,
        config: app.session.doc.configs.active.clone(),
    };
    let browser = key.stamp.is_none() && now - app.review.listed_at >= BROWSER_POLL;
    if app.review.stale || browser || app.review.key.as_ref() != Some(&key) {
        list(app);
        app.review.key = Some(key);
        app.review.listed_at = now;
        app.review.stale = false;
    }
    if let Some((markup, file)) = app.review.attach.take() {
        if app.file_picker.is_some() {
            app.review.attach = Some((markup, file));
        } else {
            write(app, "markup.image", json!({"markup": markup, "file": file}));
        }
    }
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(STAMP_POLL * 2.0));
}

/// List the set. A read: it goes straight to the engine, so a refusal is said in the list, not as a
/// message each time it is asked.
fn list(app: &mut SolveApp) {
    match app.session.execute("markup.list", &json!({})) {
        Ok(v) => match serde_json::from_value::<Listing>(v) {
            Ok(mut l) => {
                l.markups.sort_by_key(|m| std::cmp::Reverse(m.id));
                if app.review.chosen.is_some_and(|id| !l.markups.iter().any(|m| m.id == id)) {
                    app.review.chosen = None;
                }
                app.review.listing = Some(l);
                app.review.error = None;
            }
            Err(e) => app.review.error = Some(e.to_string()),
        },
        Err(e) => app.review.error = Some(e.to_string()),
    }
}

/// Run a `markup.*` write as this window's person; its refusal is said like any command's.
fn write(app: &mut SolveApp, id: &str, mut params: Value) -> Option<Value> {
    if let Some(p) = params.as_object_mut() {
        p.insert("by".into(), Value::String(me()));
    }
    let r = app.run(id, params).ok();
    app.review.stale = true;
    r
}

/// M while Review is on: mark what the pointer is on. Returns whether it took the key.
pub(crate) fn take_key(app: &mut SolveApp, key: &str) -> bool {
    if key != "M" || !app.review.open || set_of(app).is_none() || crate::render_workspace::active(app) {
        return false;
    }
    let Some(rect) = app.viewport.rect else { return false };
    match app.review.pointer.or(app.viewport.mouse).filter(|p| rect.contains(*p) && !crate::shell::covered(app, *p)) {
        Some(at) => mark_at(app, rect, at),
        // Over a card or a panel: M is Mark this, and the next click on the model places the pin.
        None => start_placing(app),
    }
    true
}

/// A new markup's draft on what is under `at` (a face or an edge of a solid; else the view).
pub(crate) fn mark_at(app: &mut SolveApp, rect: Rect, at: Pos2) {
    let proj = crate::viewport::projection(app, rect);
    let hit = crate::viewport::pick(app, &proj, at).into_iter().find_map(|h| match h {
        crate::viewport::Hit::Face { body, point, .. } | crate::viewport::Hit::Vertex { body, point } => Some((body, None, point)),
        crate::viewport::Hit::Edge { body, index, point, .. } => Some((body, Some(index), point)),
        _ => None,
    });
    let (body, edge, at) = match hit {
        Some((b, e, p)) => {
            let edge = e.and_then(|i| edge_name(app, &b, i));
            (Some(b), edge, Some(p))
        }
        None => (None, None, None),
    };
    app.review.draft = Some(Draft { at, body, edge, view: view_now(app), text: String::new() });
    app.review.chosen = None;
    app.review.placing = false;
    app.review.ask = None;
}

/// The lasting name of edge `index` of the body `body`, as `markup.add` takes it.
fn edge_name(app: &SolveApp, body: &str, index: usize) -> Option<String> {
    let st = app.session.world_state().ok()?;
    let b = st.body(body)?;
    solidcraft_engine::doc::naming::edge_names(b).get(index).filter(|n| !n.is_empty()).cloned()
}

/// The body a markup names, else the one body its feature made (renamed since), as
/// `markup.list` finds it.
fn body_of<'a>(st: &'a solidcraft_engine::doc::ModelState, row: &Row) -> Option<&'a solidcraft_engine::doc::ModelBody> {
    let name = row.text_of("body");
    st.body(&name).filter(|b| !b.body.is_mesh()).or_else(|| {
        let maker = row.anchor.get("maker").and_then(Value::as_u64).filter(|m| *m != 0)?;
        let mut made = st.bodies.iter().filter(|b| b.feature == maker && !b.body.is_mesh());
        match (made.next(), made.next()) {
            (Some(b), None) => Some(b),
            _ => None,
        }
    })
}

/// The camera now, as a markup keeps it: angles in degrees.
fn view_now(app: &SolveApp) -> Value {
    let c = app.cam;
    json!({"target": [c.target.x, c.target.y, c.target.z], "yaw": c.yaw.to_degrees(), "pitch": c.pitch.to_degrees(),
           "distance": c.distance, "fov": app.view_fov().to_degrees()})
}

/// Go there: the camera the markup was marked from, else the same view turned onto its place.
fn go_there(app: &mut SolveApp, row: &Row) {
    go_to(app, row.view.map(Seen::in_radians), row.place());
}

/// The camera `view`, else the same view turned onto `place`.
fn go_to(app: &mut SolveApp, view: Option<Seen>, place: Option<Vec3>) {
    let mut to = app.cam;
    if let Some(v) = view {
        to.target = Vec3::new(v.target[0], v.target[1], v.target[2]);
        to.yaw = v.yaw;
        to.pitch = v.pitch;
        to.roll = 0.0;
        to.distance = v.distance;
    } else if let Some(p) = place {
        to.target = p;
    } else {
        return;
    }
    app.animate_to(to);
}

/// Select what the markup is on: the body found as `markup.list` finds it, so a renamed body
/// still selects.
fn select(app: &mut SolveApp, row: &Row) {
    let at = row.place().map_or(json!(null), |p| json!([p.x, p.y, p.z]));
    let item = match row.kind() {
        kind @ ("face" | "edge" | "body") => {
            let Ok(st) = app.session.world_state() else { return };
            let Some(b) = body_of(&st, row) else { return };
            let body = b.name.clone();
            match kind {
                "face" => {
                    let name = row.text_of("face");
                    let names = solidcraft_engine::doc::naming::face_names(b);
                    let Some(index) = names.iter().position(|n| !n.is_empty() && solidcraft_engine::doc::persistent_name::same_face(n, &name)) else {
                        return;
                    };
                    json!({"type": "face", "body": body, "index": index, "point": at})
                }
                // The edge by its point on it: of several edges with one name, the one marked.
                "edge" if !at.is_null() => json!({"type": "edge", "body": body, "point": at}),
                _ => json!({"type": "body", "name": body}),
            }
        }
        "feature" => json!({"type": "feature", "id": row.anchor.get("feature").cloned().unwrap_or(Value::Null)}),
        _ => return,
    };
    let _ = app.run("select.set", json!({"items": [item]}));
}

/// Words for what a markup is on.
pub(crate) fn on_words(app: &SolveApp, row: &Row) -> String {
    let feature = row.maker().and_then(|id| app.session.doc.feature(id)).map(|f| f.name.clone());
    let body = row.text_of("body");
    match (row.kind(), feature) {
        ("face", Some(f)) => fmt("review.on_face_by", &[("feature", &f), ("body", &body)]),
        ("face", None) => fmt("review.on_face", &[("body", &body)]),
        ("edge", _) => fmt("review.on_edge", &[("body", &body)]),
        ("body", _) => body,
        ("feature", f) => fmt("review.on_feature", &[("name", &f.unwrap_or_else(|| row.text_of("name")))]),
        ("occurrence", _) => fmt("review.on_component", &[("name", &row.text_of("name"))]),
        ("point", _) => solidcraft_ui_text::tr("review.on_point").to_owned(),
        _ => solidcraft_ui_text::tr("review.on_view").to_owned(),
    }
}

fn fmt(id: &str, args: &[(&str, &str)]) -> String {
    solidcraft_ui_text::format(id, args)
}

/// "5 min ago", from Unix seconds.
fn ago(at: u64) -> String {
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(at, |d| d.as_secs());
    let s = now.saturating_sub(at);
    let n = |v: u64| v.to_string();
    if s < 60 {
        solidcraft_ui_text::tr("review.ago_now").to_owned()
    } else if s < 3600 {
        fmt("review.ago_minutes", &[("n", &n(s / 60))])
    } else if s < 86_400 {
        fmt("review.ago_hours", &[("n", &n(s / 3600))])
    } else {
        fmt("review.ago_days", &[("n", &n(s / 86_400))])
    }
}

/// A row's second line in the list.
fn row_line(app: &SolveApp, row: &Row) -> (String, bool) {
    match (row.look(), &row.outcome) {
        (Look::Done, Some(o)) => (fmt("review.done_by", &[("by", &o.by), ("reason", &o.reason)]), false),
        (Look::Rejected, Some(o)) => (fmt("review.rejected_why", &[("reason", &o.reason)]), false),
        (Look::Gone, _) => (solidcraft_ui_text::tr("review.gone_line").to_owned(), true),
        (Look::Hidden, _) => (solidcraft_ui_text::tr("review.hidden_line").to_owned(), false),
        _ => {
            let mut words = vec![on_words(app, row)];
            let replies = row.thread.len().saturating_sub(1);
            if replies == 1 {
                words.push(solidcraft_ui_text::tr("review.one_reply").to_owned());
            } else if replies > 1 {
                words.push(fmt("review.replies", &[("count", &replies.to_string())]));
            }
            match row.images.len() {
                0 => {}
                1 => words.push(solidcraft_ui_text::tr("review.one_picture").to_owned()),
                n => words.push(fmt("review.pictures", &[("count", &n.to_string())])),
            }
            (words.join(" · "), false)
        }
    }
}

/// A pin's colour: theme tokens only.
pub(crate) fn colour(look: Look, chosen: bool) -> Color32 {
    let t = Tokens::get();
    if chosen {
        return t.accent;
    }
    match look {
        Look::Open => t.warning,
        Look::Gone => t.error,
        Look::Done => t.form_success,
        Look::Rejected => t.text_dim,
        Look::Hidden => t.text_dim.gamma_multiply(0.45),
    }
}

/// A teardrop with its tip at `tip`: a round head of radius `r` and two straight sides tangent to it,
/// as one convex outline.
pub(crate) fn teardrop(tip: Pos2, r: f32) -> Vec<Pos2> {
    let d = r * 1.65;
    let c = tip - vec2(0.0, d);
    // The sides touch the head where the radius makes a right angle with the line from the tip.
    let half = (r / d).clamp(-1.0, 1.0).acos();
    let down = std::f32::consts::FRAC_PI_2;
    let (from, to) = (down + half, down - half + std::f32::consts::TAU);
    let steps = 28;
    let mut points = Vec::with_capacity(steps + 2);
    points.push(tip);
    for i in 0..=steps {
        let a = from + (to - from) * i as f32 / steps as f32;
        points.push(c + vec2(a.cos(), a.sin()) * r);
    }
    points
}

/// The middle of a teardrop's head.
fn head(tip: Pos2, r: f32) -> Pos2 {
    tip - vec2(0.0, r * 1.65)
}

/// Paint one pin: its teardrop, then a number, a tick or a cross on its head.
pub(crate) fn paint_pin(painter: &egui::Painter, tip: Pos2, r: f32, fill: Color32, look: Look, number: Option<u64>, ring: bool) {
    let t = Tokens::get();
    let ink = t.on_accent;
    if ring {
        painter.circle_stroke(tip, r * 0.7, Stroke::new(2.5, fill));
    }
    painter.add(Shape::convex_polygon(teardrop(tip, r), fill, Stroke::new(2.0, ink)));
    let c = head(tip, r);
    let w = Stroke::new((r * 0.2).max(1.5), ink);
    match look {
        Look::Done => {
            painter.line(vec![c + vec2(-0.42 * r, 0.0), c + vec2(-0.12 * r, 0.32 * r), c + vec2(0.45 * r, -0.36 * r)], w);
        }
        Look::Rejected => {
            let k = 0.36 * r;
            painter.line_segment([c + vec2(-k, -k), c + vec2(k, k)], w);
            painter.line_segment([c + vec2(k, -k), c + vec2(-k, k)], w);
        }
        _ => {
            if let Some(n) = number {
                let size = if n >= 100 { r * 0.75 } else { r * 0.95 };
                painter.text(c, egui::Align2::CENTER_CENTER, n.to_string(), crate::ui_kit::type_face::bold(size), ink);
            }
        }
    }
}

/// Where the card stands: beside its pin (left of it when there is room, else right), inside the view
/// and clear of the list; with no pin on the screen, beside the list.
pub(crate) fn card_place(view: Rect, list: Option<Rect>, size: Vec2, spot: Option<Pos2>) -> Pos2 {
    let gap = 18.0;
    let room = match list {
        Some(l) if l.left() > view.left() => Rect::from_min_max(view.min, pos2(l.left() - gap / 2.0, view.bottom())),
        _ => view,
    };
    let (x, y) = match spot {
        Some(p) => {
            let left = p.x - gap - size.x;
            let x = if left >= room.left() { left } else { p.x + gap };
            (x, p.y - size.y * 0.35)
        }
        None => match list {
            Some(l) => (l.left() - gap - size.x, l.top()),
            None => (room.left() + gap, room.top() + gap),
        },
    };
    let x = x.clamp(room.left() + 4.0, (room.right() - size.x - 4.0).max(room.left() + 4.0));
    let y = y.clamp(room.top() + 4.0, (room.bottom() - size.y - 4.0).max(room.top() + 4.0));
    pos2(x, y)
}

/// Draw the review over the view: pins, Mark this, the list and the card. Called with the view's rect,
/// after the view took its input.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    if !app.review.open || crate::render_workspace::active(app) || !rect.is_positive() {
        app.review.drawn = Drawn::default();
        return;
    }
    let ctx = ui.ctx().clone();
    if (app.review.draft.is_some() || app.review.placing) && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.review.draft = None;
        app.review.placing = false;
    }
    if app.review.shot.is_some_and(|s| crate::now_ms() > s.until) {
        app.review.shot = None;
    }
    let shooting = app.review.shot.is_some();
    let proj = crate::viewport::projection(app, rect);
    let spots = pins(app, ui, &proj, rect);
    if app.review.placing {
        let r = ui.interact(rect, Id::unique("sc_review_place"), Sense::click());
        if r.clicked()
            && let Some(at) = r.interact_pointer_pos()
        {
            mark_at(app, rect, at);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let mut drawn = Drawn::default();
    if shooting {
        app.review.drawn = drawn;
        return;
    }
    drawn.mark = mark_button(app, ui, rect);
    drawn.list = review_list(app, &ctx, rect);
    let spot = match (&app.review.draft, app.review.chosen) {
        (Some(d), _) => d.at.and_then(|p| proj.to_screen(p)).filter(|p| rect.contains(*p)),
        (None, Some(id)) => spots.iter().find(|(m, _)| *m == id).map(|(_, p)| *p),
        _ => None,
    };
    if app.review.draft.is_some() || app.review.chosen_row().is_some() {
        drawn.card = card(app, &ctx, rect, drawn.list, spot);
    }
    app.review.drawn = drawn;
}

/// The pins on the model, and the one clicked chosen. Returns each drawn pin's tip on the screen.
fn pins(app: &mut SolveApp, ui: &mut egui::Ui, proj: &crate::viewport::Proj, rect: Rect) -> Vec<(u64, Pos2)> {
    let painter = ui.painter_at(rect);
    let r = 11.0;
    let mut spots = Vec::new();
    let mut clicked = None;
    if !app.review.pins_hidden {
        let filter = app.review.filter;
        let chosen = app.review.chosen;
        let mut rows: Vec<&Row> = app.review.rows().iter().filter(|m| filter.keeps(m) || Some(m.id) == chosen).collect();
        // Oldest first, the chosen last: newer pins and the chosen one stand in front.
        rows.sort_by_key(|m| (Some(m.id) == chosen, m.id));
        for m in rows {
            let Some(tip) = m.place().and_then(|p| proj.to_screen(p)).filter(|p| rect.contains(*p)) else { continue };
            let on = Some(m.id) == chosen;
            let size = if on { r * 1.25 } else { r };
            paint_pin(&painter, tip, size, colour(m.look(), on), m.look(), Some(m.id), on);
            let hit = Rect::from_center_size(head(tip, size), Vec2::splat(size * 2.4));
            solidcraft_ui_registry::publish_handle(&format!("review:pin:{}", m.id), head(tip, size));
            let resp = ui.interact(hit, Id::unique(("sc_review_pin", m.id)), Sense::click());
            if resp.clicked() {
                clicked = Some(m.id);
            }
            resp.backing_on_hover_text(m.note.lines().next().unwrap_or(""));
            spots.push((m.id, tip));
        }
    }
    if let Some(d) = &app.review.draft
        && let Some(tip) = d.at.and_then(|p| proj.to_screen(p)).filter(|p| rect.contains(*p))
    {
        paint_pin(&painter, tip, r * 1.25, Tokens::get().accent, Look::Open, None, true);
    }
    if let Some(id) = clicked {
        choose(app, id);
    }
    spots
}

fn choose(app: &mut SolveApp, id: u64) {
    if app.review.chosen != Some(id) {
        app.review.reply.clear();
        app.review.ask = None;
    }
    app.review.chosen = Some(id);
    app.review.draft = None;
    app.review.placing = false;
}

/// Mark this, just left of the navigation bar (or at the bottom middle of the view without it).
fn mark_button(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) -> Option<Rect> {
    let t = Tokens::get();
    let label = app.tr("review.mark").to_owned();
    let font = crate::ui_kit::type_face::semibold(t.font_body);
    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), font.clone(), t.on_accent).size().x);
    let size = vec2(text_w + 76.0, 34.0);
    let nav = app.viewport.layout.as_ref().map(|l| l.navigation).filter(|n| n.is_positive());
    let min = match nav {
        Some(n) => pos2(n.left() - t.space_8 - size.x, n.center().y - size.y / 2.0),
        None => pos2(rect.center().x - size.x / 2.0, rect.bottom() - size.y - t.space_12),
    };
    let button = Rect::from_min_size(pos2(min.x.max(rect.left() + t.space_8), min.y), size);
    if !rect.contains_rect(button) {
        return None;
    }
    let can = set_of(app).is_some();
    let resp = ui.interact(button, Id::unique("sc_review_mark"), if can { Sense::click() } else { Sense::hover() });
    solidcraft_ui_registry::publish_handle("review:mark", button.center());
    let painter = ui.painter_at(rect);
    let fill = if !can {
        t.text_dim
    } else if app.review.placing || resp.hovered() {
        t.accent.gamma_multiply(0.85)
    } else {
        t.accent
    };
    painter.rect_filled(button, f32::from(t.control_radius), fill);
    let pin = button.left_center() + vec2(18.0, 8.0);
    painter.add(Shape::convex_polygon(teardrop(pin, 6.5), t.on_accent, Stroke::NONE));
    painter.circle_filled(head(pin, 6.5), 2.2, fill);
    painter.text(button.left_center() + vec2(32.0, 0.0), egui::Align2::LEFT_CENTER, &label, font, t.on_accent);
    let chip = Rect::from_center_size(button.right_center() - vec2(20.0, 0.0), vec2(22.0, 20.0));
    painter.rect_stroke(chip, 4.0, Stroke::new(1.0, t.on_accent.gamma_multiply(0.8)), egui::StrokeKind::Inside);
    painter.text(chip.center(), egui::Align2::CENTER_CENTER, "M", crate::ui_kit::type_face::medium(t.font_small), t.on_accent);
    let tip = if can { app.tr("review.mark_tip") } else { app.tr("review.unsaved") };
    let resp = resp.backing_on_hover_text(tip);
    if resp.clicked() {
        if app.review.placing {
            app.review.placing = false;
        } else {
            start_placing(app);
        }
    }
    Some(button)
}

/// Mark this: the next click on the model places the new markup's pin.
fn start_placing(app: &mut SolveApp) {
    app.review.placing = true;
    app.review.draft = None;
    app.review.chosen = None;
    let hint = app.tr("review.placing").to_owned();
    app.set_status(hint, false);
}

/// The Review list along the right of the view, below the view navigator.
fn review_list(app: &mut SolveApp, ctx: &egui::Context, rect: Rect) -> Option<Rect> {
    let t = Tokens::get();
    let axis = crate::viewport::axis_rect(app, rect);
    let top =
        if axis.is_positive() && axis.right() > rect.right() - LIST_WIDTH - t.space_8 { axis.bottom() + t.space_8 } else { rect.top() + t.space_8 };
    let nav_top = app.viewport.layout.as_ref().map(|l| l.navigation).filter(|n| n.is_positive()).map_or(rect.bottom(), |n| n.top());
    let bottom = (nav_top - t.space_8).max(top + 120.0).min(rect.bottom() - t.space_8);
    let bounds = Rect::from_min_max(pos2(rect.right() - LIST_WIDTH - t.space_8, top), pos2(rect.right() - t.space_8, bottom));
    if !bounds.is_positive() || bounds.width() < 200.0 {
        return None;
    }
    let container = Container::floating_card(Surface::Panel);
    let mut chose = None;
    let mut close = false;
    let shown =
        container.area(Id::unique("sc_review_list")).movable(false).order(egui::Order::Middle).fixed_pos(bounds.min).constrain_to(bounds).show(
            ctx,
            |ui| {
                let frame = container.frame(ctx, t.dialog_body, crate::frame::Side::Right).inner_margin(t.space_8);
                container
                    .show_frame(ui, frame, bounds, |ui| {
                        ui_kit::scope(ui, |ui| {
                            ui.set_width(LIST_WIDTH - 2.0 * t.space_8 - 2.0);
                            ui.set_max_height(bounds.height() - 2.0 * t.space_8);
                            list_head(app, ui, &mut close);
                            list_filters(app, ui);
                            ui.add_space(t.space_4);
                            chose = list_rows(app, ui);
                            ui.add_space(t.space_4);
                            ui.add(egui::Label::new(RichText::new(app.tr("review.hint")).color(t.text_dim).size(t.font_small)).wrap());
                        })
                    })
                    .response
                    .rect
            },
        );
    if let Some(id) = chose {
        if app.review.chosen == Some(id) {
            app.review.chosen = None;
        } else {
            choose(app, id);
        }
    }
    if close {
        let _ = app.run("view.review", json!({"visible": false}));
    }
    solidcraft_ui_registry::publish_rect("review:list", shown.inner);
    Some(shown.inner)
}

fn list_head(app: &mut SolveApp, ui: &mut egui::Ui, close: &mut bool) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        ui.label(RichText::new(app.tr("review.title")).strong().size(t.font_subtitle).color(t.text));
        let (words, bad) = match (&app.review.error, set_of(app)) {
            (Some(_), _) => (app.tr("review.not_read").to_owned(), true),
            (None, None) => (app.tr("review.no_file").to_owned(), false),
            (None, Some(_)) if cfg!(target_arch = "wasm32") => (app.tr("review.in_browser").to_owned(), false),
            (None, Some(_)) => (app.tr("review.saved").to_owned(), false),
        };
        let r = ui.label(RichText::new(words).size(t.font_small).color(if bad { t.error } else { t.text_dim }));
        if let Some(e) = &app.review.error {
            r.backing_on_hover_text(e);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let x = small_icon(ui, "close", app.tr("review.close"));
            solidcraft_ui_registry::publish_handle("review:close", x.rect.center());
            if x.clicked() {
                *close = true;
            }
            let eye = if app.review.pins_hidden { "suppress" } else { "eye" };
            let tip = if app.review.pins_hidden { app.tr("review.show_pins") } else { app.tr("review.hide_pins") };
            let e = small_icon(ui, eye, tip);
            solidcraft_ui_registry::publish_handle("review:eye", e.rect.center());
            if e.clicked() {
                app.review.pins_hidden = !app.review.pins_hidden;
            }
        });
    });
}

fn list_filters(app: &mut SolveApp, ui: &mut egui::Ui) {
    let [open, done, rejected] = app.review.counts();
    let all = open + done + rejected;
    ui.horizontal_wrapped(|ui| {
        for (f, key, n, handle) in [
            (Filter::All, "review.filter_all", all, "all"),
            (Filter::Open, "review.filter_open", open, "open"),
            (Filter::Done, "review.filter_done", done, "done"),
            (Filter::Rejected, "review.filter_rejected", rejected, "rejected"),
        ] {
            let on = app.review.filter == f;
            let label = fmt(key, &[("count", &n.to_string())]);
            let b = ui_kit::button(ui, label, if on { ui_kit::Button::SmallSelected } else { ui_kit::Button::Small });
            solidcraft_ui_registry::publish_handle(&format!("review:filter:{handle}"), b.rect.center());
            if b.clicked() {
                app.review.filter = f;
            }
        }
    });
}

/// The rows, newest first; returns the one clicked.
fn list_rows(app: &mut SolveApp, ui: &mut egui::Ui) -> Option<u64> {
    let t = Tokens::get();
    let filter = app.review.filter;
    let rows: Vec<Row> = app.review.rows().iter().filter(|m| filter.keeps(m)).cloned().collect();
    if rows.is_empty() {
        ui.add(egui::Label::new(RichText::new(app.tr("review.empty")).color(t.text_dim).size(t.font_small)).wrap());
        return None;
    }
    let mut clicked = None;
    let max = (ui.max_rect().bottom() - ui.cursor().top() - 40.0).max(80.0);
    egui::ScrollArea::vertical().max_height(max).auto_shrink([false, true]).show(ui, |ui| {
        for m in &rows {
            let on = app.review.chosen == Some(m.id);
            let (line, bad) = row_line(app, m);
            let width = ui.available_width();
            let height = 48.0;
            let (row, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
            solidcraft_ui_registry::publish_handle(&format!("review:row:{}", m.id), row.center());
            let p = ui.painter();
            if on {
                p.rect(row, f32::from(t.control_radius), t.accent_soft, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
            } else if resp.hovered() {
                p.rect_filled(row, f32::from(t.control_radius), t.hover);
            }
            let look = m.look();
            paint_pin(p, row.left_center() + vec2(14.0, 12.0), 7.5, colour(look, on), look, None, false);
            let first = m.note.lines().next().unwrap_or("").to_owned();
            let text_left = row.left() + 32.0;
            let room = row.right() - 8.0 - text_left;
            let head = one_line(p, format!("{} · {first}", m.id), crate::ui_kit::type_face::semibold(t.font_label), t.text, room);
            p.galley(pos2(text_left, row.top() + 15.0 - head.size().y / 2.0), head, t.text);
            let tone = if bad { t.error } else { t.text_dim };
            let sub = one_line(p, line, crate::ui_kit::type_face::medium(t.font_small), tone, room);
            p.galley(pos2(text_left, row.top() + 33.0 - sub.size().y / 2.0), sub, tone);
            if resp.clicked() {
                clicked = Some(m.id);
            }
        }
    });
    clicked
}

/// The card beside the chosen pin, or the new markup's card.
fn card(app: &mut SolveApp, ctx: &egui::Context, view: Rect, list: Option<Rect>, spot: Option<Pos2>) -> Option<Rect> {
    let t = Tokens::get();
    let size = app.review.drawn.card.map_or(vec2(CARD_WIDTH, 380.0), |r| r.size());
    let at = card_place(view, list, size, spot);
    let container = Container::new(ctx, Surface::Canvas);
    let bounds = view;
    let mut out = None;
    let shown =
        container.area(Id::unique("sc_review_card")).movable(false).order(egui::Order::Middle).fixed_pos(at).constrain_to(bounds).show(ctx, |ui| {
            let frame = container.frame(ctx, t.dialog_body, crate::frame::Side::Left).inner_margin(t.space_8);
            container
                .show_frame(ui, frame, bounds, |ui| {
                    ui_kit::scope(ui, |ui| {
                        ui.set_width(CARD_WIDTH - 2.0 * t.space_8);
                        ui.set_max_height((view.height() - 16.0).max(200.0));
                        if app.review.draft.is_some() {
                            out = new_card(app, ui);
                        } else if let Some(row) = app.review.chosen_row().cloned() {
                            out = read_card(app, ui, &row);
                        }
                    })
                })
                .response
                .rect
        });
    match out {
        Some(Do::Close) => {
            app.review.chosen = None;
            app.review.draft = None;
        }
        Some(Do::Add) => add(app),
        Some(Do::Run(id, params)) => {
            write(app, &id, params);
            app.review.ask = None;
            app.review.reply.clear();
        }
        Some(Do::Delete(id)) => {
            if write(app, "markup.delete", json!({"markup": id})).is_some() {
                app.review.chosen = None;
            }
            app.review.ask = None;
        }
        Some(Do::GoThere(row)) => go_there(app, &row),
        Some(Do::Select(row)) => select(app, &row),
        Some(Do::Edit(id)) => app.edit_feature(id),
        Some(Do::Shoot(id)) => shoot(app, ctx, view, id),
        Some(Do::Attach(id)) => attach(app, id),
        None => {}
    }
    solidcraft_ui_registry::publish_rect("review:card", shown.inner);
    Some(shown.inner)
}

/// What a card's press asks for, done after the card is drawn.
enum Do {
    Close,
    Add,
    Run(String, Value),
    Delete(u64),
    GoThere(Box<Row>),
    Select(Box<Row>),
    Edit(u64),
    Shoot(u64),
    Attach(u64),
}

/// A note box: the growing message field, or a plain area where that is not built (the browser).
fn note_box(ui: &mut egui::Ui, id: Id, text: &mut String, hint: &str) -> egui::Response {
    #[cfg(not(target_arch = "wasm32"))]
    {
        ui_kit::message_field(ui, id, text, hint)
    }
    #[cfg(target_arch = "wasm32")]
    {
        ui_kit::text_area(ui, hint, id, text, 3, solidcraft_engine::io::markups::MAX_NOTE)
    }
}

/// The status as a pill: its words in the status colour on a faint wash of it.
fn pill(ui: &mut egui::Ui, words: &str, tone: Color32) {
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(words.to_owned(), crate::ui_kit::type_face::semibold(t.font_small), tone);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(14.0, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, rect.height() / 2.0, tone.gamma_multiply(0.18));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, tone);
}

/// One line of text cut with … to `width`.
fn one_line(painter: &egui::Painter, text: String, font: egui::FontId, colour: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text, egui::TextFormat::simple(font, colour));
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    painter.layout_job(job)
}

/// A quiet icon button the size of a line of text, with its hover words.
fn small_icon(ui: &mut egui::Ui, icon: &str, label: &str) -> egui::Response {
    let side = ui.spacing().interact_size.y;
    let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    ui_kit::quiet_icon_button_at(ui, rect, icon, label)
}

fn card_head(ui: &mut egui::Ui, status: &str, tone: Color32, title: &str, out: &mut Option<Do>) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        pill(ui, status, tone);
        ui.label(RichText::new(title).strong().size(t.font_body).color(t.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let x = small_icon(ui, "close", solidcraft_ui_text::tr("ui.close"));
            solidcraft_ui_registry::publish_handle("review:card:close", x.rect.center());
            if x.clicked() {
                *out = Some(Do::Close);
            }
        });
    });
}

fn new_card(app: &mut SolveApp, ui: &mut egui::Ui) -> Option<Do> {
    let t = Tokens::get();
    let mut out = None;
    let mut draft = app.review.draft.clone()?;
    let on = match (&draft.body, draft.at) {
        (Some(b), _) if draft.edge.is_some() => fmt("review.on_edge", &[("body", b)]),
        (Some(b), _) => b.clone(),
        (None, Some(_)) => app.tr("review.on_point").to_owned(),
        (None, None) => app.tr("review.on_view").to_owned(),
    };
    card_head(ui, app.tr("review.new"), t.accent, &on, &mut out);
    ui.add_space(t.space_4);
    let id = Id::unique("sc_review_note");
    let field = note_box(ui, id, &mut draft.text, app.tr("review.note_hint"));
    solidcraft_ui_registry::publish_handle("review:note", field.rect.center());
    if !field.has_focus() && draft.text.is_empty() && !ui.memory(|m| m.has_focus(id)) {
        field.request_focus();
    }
    ui.add_space(t.space_4);
    ui.horizontal(|ui| {
        let can = !draft.text.trim().is_empty();
        let add = ui.add_enabled_ui(can, |ui| ui_kit::button(ui, app.tr("review.add"), ui_kit::Button::Primary)).inner;
        solidcraft_ui_registry::publish_handle("review:add", add.rect.center());
        if add.clicked() {
            out = Some(Do::Add);
        }
        let cancel = ui_kit::button(ui, app.tr("review.cancel"), ui_kit::Button::Normal);
        solidcraft_ui_registry::publish_handle("review:cancel", cancel.rect.center());
        if cancel.clicked() {
            out = Some(Do::Close);
        }
    });
    if let Some(d) = app.review.draft.as_mut() {
        d.text = draft.text;
    }
    out
}

fn add(app: &mut SolveApp) {
    let Some(d) = app.review.draft.clone() else { return };
    let mut p = json!({"text": d.text.trim(), "view": d.view});
    if let Some(at) = d.at {
        p["at"] = json!([at.x, at.y, at.z]);
    }
    if let Some(b) = &d.body {
        p["body"] = json!(b);
    }
    if let Some(e) = &d.edge {
        p["edge"] = json!(e);
    }
    let made = write(app, "markup.add", p.clone()).or_else(|| {
        // What the pointer was on is not a solid body's face or edge (a mesh, a point in space):
        // keep the place, else the view, rather than lose the note.
        let mut p = p;
        p.as_object_mut().map(|o| (o.remove("body"), o.remove("edge")));
        if d.body.is_some() { write(app, "markup.add", p) } else { None }
    });
    if let Some(v) = made {
        app.review.draft = None;
        app.review.chosen = v.get("markup").and_then(Value::as_u64);
        app.review.filter = if app.review.filter == Filter::All { Filter::All } else { Filter::Open };
    }
}

fn read_card(app: &mut SolveApp, ui: &mut egui::Ui, row: &Row) -> Option<Do> {
    let t = Tokens::get();
    let mut out = None;
    let look = row.look();
    let (status, tone) = match look {
        Look::Open => (app.tr("review.status_open"), t.warning),
        Look::Gone => (app.tr("review.status_gone"), t.error),
        Look::Done => (app.tr("review.status_done"), t.form_success),
        Look::Rejected => (app.tr("review.status_rejected"), t.text_dim),
        Look::Hidden => (app.tr("review.status_hidden"), t.text_dim),
    };
    let title = format!("{} · {}", row.id, on_words(app, row));
    card_head(ui, status, tone, &title, &mut out);
    ui.label(RichText::new(fmt("review.marked", &[("by", &row.by), ("ago", &ago(row.made))])).size(t.font_small).color(t.text_dim));
    match look {
        Look::Gone => drop(ui.label(RichText::new(app.tr("review.gone_line")).size(t.font_small).color(t.error))),
        Look::Hidden => drop(ui.label(RichText::new(app.tr("review.hidden_line")).size(t.font_small).color(t.text_dim))),
        _ if row.found.state == "moved" => drop(ui.label(RichText::new(app.tr("review.moved_line")).size(t.font_small).color(t.text_dim))),
        _ => {}
    }
    ui.add_space(t.space_4);
    let thread_height = (ui.max_rect().height() * 0.45).max(90.0);
    egui::ScrollArea::vertical().id_salt("sc_review_thread").max_height(thread_height).auto_shrink([false, true]).stick_to_bottom(true).show(
        ui,
        |ui| {
            for said in &row.thread {
                let fill = if said.reviewer { t.hover } else { t.accent_soft };
                let indent = if said.reviewer { 0.0 } else { t.space_12 };
                let width = ui.available_width() - indent - 12.0;
                ui.horizontal_top(|ui| {
                    ui.add_space(indent);
                    let frame =
                        Container::new(ui.ctx(), Surface::Panel).edge_frame(fill).corner_radius(t.control_radius).inner_margin(egui::Margin::same(6));
                    Container::show_backing(ui, frame, |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            let who = format!("{} · {}", said.by, ago(said.at));
                            ui.label(RichText::new(who).size(t.font_small).strong().color(if said.reviewer { t.warning } else { t.accent }));
                            ui.add(egui::Label::new(RichText::new(&said.text).color(t.text)).wrap());
                        });
                    });
                });
                ui.add_space(t.space_2);
            }
        },
    );
    ui.add_space(t.space_4);
    pictures(app, ui, row, &mut out);
    ui.add_space(t.space_4);
    let id = Id::unique(("sc_review_reply", row.id));
    let mut reply = std::mem::take(&mut app.review.reply);
    let field = note_box(ui, id, &mut reply, app.tr("review.reply_hint"));
    solidcraft_ui_registry::publish_handle("review:reply", field.rect.center());
    let send_key = field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
    if send_key {
        // The Enter that sends is not part of the reply.
        while reply.ends_with('\n') {
            reply.pop();
        }
    }
    let send = ui.horizontal(|ui| {
        let can = !reply.trim().is_empty();
        let b = ui.add_enabled_ui(can, |ui| ui_kit::button(ui, app.tr("review.send"), ui_kit::Button::Small)).inner;
        solidcraft_ui_registry::publish_handle("review:send", b.rect.center());
        can && (b.clicked() || send_key)
    });
    if send.inner {
        out = Some(Do::Run("markup.reply".into(), json!({"markup": row.id, "text": reply.trim()})));
    }
    app.review.reply = reply;
    ui.add_space(t.space_4);
    tools(app, ui, row, &mut out);
    ui.add_space(t.space_4);
    closing(app, ui, row, &mut out);
    out
}

/// Go there, Select, Edit <feature> and Delete.
fn tools(app: &mut SolveApp, ui: &mut egui::Ui, row: &Row, out: &mut Option<Do>) {
    ui.horizontal_wrapped(|ui| {
        let go = ui_kit::button(ui, app.tr("review.go_there"), ui_kit::Button::Small);
        solidcraft_ui_registry::publish_handle("review:go_there", go.rect.center());
        if go.clicked() {
            *out = Some(Do::GoThere(Box::new(row.clone())));
        }
        if matches!(row.kind(), "face" | "edge" | "body" | "feature") && row.look() != Look::Gone && row.look() != Look::Hidden {
            let s = ui_kit::button(ui, app.tr("review.select"), ui_kit::Button::Small);
            solidcraft_ui_registry::publish_handle("review:select", s.rect.center());
            if s.clicked() {
                *out = Some(Do::Select(Box::new(row.clone())));
            }
        }
        if let Some((id, name)) = row.maker().and_then(|id| app.session.doc.feature(id)).map(|f| (f.id, f.name.clone())) {
            let e = ui_kit::button(ui, fmt("review.edit_feature", &[("name", &name)]), ui_kit::Button::Small);
            solidcraft_ui_registry::publish_handle("review:edit", e.rect.center());
            if e.clicked() {
                *out = Some(Do::Edit(id));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let d = ui_kit::button(ui, app.tr("review.delete"), ui_kit::Button::Ghost);
            solidcraft_ui_registry::publish_handle("review:delete", d.rect.center());
            if d.clicked() {
                app.review.ask = Some(Ask::Delete);
            }
        });
    });
}

/// Resolve… and Reject…, with their reason; Reopen for a closed one; and Delete's question.
fn closing(app: &mut SolveApp, ui: &mut egui::Ui, row: &Row, out: &mut Option<Do>) {
    let t = Tokens::get();
    match app.review.ask.clone() {
        Some(Ask::Delete) => {
            ui.label(RichText::new(app.tr("review.delete_ask")).color(t.text));
            ui.horizontal(|ui| {
                let yes = ui_kit::button(ui, app.tr("review.delete"), ui_kit::Button::DangerOutlined);
                solidcraft_ui_registry::publish_handle("review:delete_yes", yes.rect.center());
                if yes.clicked() {
                    *out = Some(Do::Delete(row.id));
                }
                if ui_kit::button(ui, app.tr("review.cancel"), ui_kit::Button::Normal).clicked() {
                    app.review.ask = None;
                }
            });
        }
        Some(Ask::Resolve(mut why)) | Some(Ask::Reject(mut why)) => {
            let resolving = matches!(app.review.ask, Some(Ask::Resolve(_)));
            let hint = if resolving { app.tr("review.resolve_hint") } else { app.tr("review.reject_hint") };
            let field = note_box(ui, Id::unique(("sc_review_reason", row.id)), &mut why, hint);
            solidcraft_ui_registry::publish_handle("review:reason", field.rect.center());
            ui.horizontal(|ui| {
                let (label, command) =
                    if resolving { (app.tr("review.resolve_go"), "markup.resolve") } else { (app.tr("review.reject_go"), "markup.reject") };
                let can = !why.trim().is_empty();
                let go = ui.add_enabled_ui(can, |ui| ui_kit::button(ui, label, ui_kit::Button::Primary)).inner;
                solidcraft_ui_registry::publish_handle("review:close_go", go.rect.center());
                if go.clicked() {
                    *out = Some(Do::Run(command.into(), json!({"markup": row.id, "reason": why.trim()})));
                }
                if ui_kit::button(ui, app.tr("review.cancel"), ui_kit::Button::Normal).clicked() {
                    app.review.ask = None;
                }
            });
            if app.review.ask.is_some() {
                app.review.ask = Some(if resolving { Ask::Resolve(why) } else { Ask::Reject(why) });
            }
        }
        None if row.status == "open" => {
            ui.columns(2, |cols| {
                let r = ui_kit::button(&mut cols[0], app.tr("review.resolve"), ui_kit::Button::Primary);
                solidcraft_ui_registry::publish_handle("review:resolve", r.rect.center());
                if r.clicked() {
                    app.review.ask = Some(Ask::Resolve(String::new()));
                }
                let j = ui_kit::button(&mut cols[1], app.tr("review.reject"), ui_kit::Button::Normal);
                solidcraft_ui_registry::publish_handle("review:reject", j.rect.center());
                if j.clicked() {
                    app.review.ask = Some(Ask::Reject(String::new()));
                }
            });
        }
        None => {
            let r = ui_kit::button(ui, app.tr("review.reopen"), ui_kit::Button::Normal);
            solidcraft_ui_registry::publish_handle("review:reopen", r.rect.center());
            if r.clicked() {
                *out = Some(Do::Run("markup.reopen".into(), json!({"markup": row.id})));
            }
        }
    }
}

/// The pictures' small copies, with Screenshot and Attach image beside them.
fn pictures(app: &mut SolveApp, ui: &mut egui::Ui, row: &Row, out: &mut Option<Do>) {
    let t = Tokens::get();
    ui.horizontal_wrapped(|ui| {
        for (n, p) in row.images.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(THUMB, Sense::click());
            match thumb(app, ui.ctx(), p) {
                Some((tex, size)) => {
                    let fit = rect.center() - size / 2.0;
                    egui::Image::new(&tex).paint_at(ui, Rect::from_min_size(fit, size));
                }
                None => {
                    ui.painter().rect_filled(rect, 6.0, t.panel);
                }
            }
            ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
            solidcraft_ui_registry::publish_handle(&format!("review:picture:{n}"), rect.center());
            resp.backing_on_hover_text(&p.name);
        }
        if row.images.len() < solidcraft_engine::io::markups::MAX_IMAGES {
            let s = ui_kit::button(ui, app.tr("review.screenshot"), ui_kit::Button::Small).backing_on_hover_text(app.tr("review.screenshot_tip"));
            solidcraft_ui_registry::publish_handle("review:screenshot", s.rect.center());
            if s.clicked() {
                *out = Some(Do::Shoot(row.id));
            }
            if cfg!(not(target_arch = "wasm32")) {
                let a = ui_kit::button(ui, app.tr("review.attach"), ui_kit::Button::Small);
                solidcraft_ui_registry::publish_handle("review:attach", a.rect.center());
                if a.clicked() {
                    *out = Some(Do::Attach(row.id));
                }
            }
        }
    });
}

/// A picture's small copy as a texture, made once from the design's bytes (at most [`THUMBS`]
/// kept).
fn thumb(app: &mut SolveApp, ctx: &egui::Context, picture: &Picture) -> Option<(egui::TextureHandle, Vec2)> {
    if !picture.in_design {
        return None;
    }
    let path = picture.file.as_str();
    if let Some(kept) = app.review.thumbs.get(path) {
        return kept.clone();
    }
    if app.review.thumbs.len() >= THUMBS {
        app.review.thumbs.clear();
    }
    let made = app.session.doc.markup_images.get(path).and_then(|bytes| {
        let img = image::load_from_memory(bytes).ok()?;
        let small = img.thumbnail((THUMB.x * 2.0) as u32, (THUMB.y * 2.0) as u32).to_rgba8();
        let (w, h) = (small.width() as usize, small.height() as usize);
        let pixels = ColorImage::from_rgba_unmultiplied([w, h], small.as_raw());
        let tex = ctx.load_texture(format!("review:{path}"), pixels, egui::TextureOptions::LINEAR);
        let scale = (THUMB.x / w as f32).min(THUMB.y / h as f32);
        Some((tex, vec2(w as f32, h as f32) * scale))
    });
    app.review.thumbs.insert(path.to_owned(), made.clone());
    made
}

/// Screenshot: the view as it is, without the card and the list, a few frames on.
fn shoot(app: &mut SolveApp, ctx: &egui::Context, view: Rect, markup: u64) {
    let Some(token) = app.shots.token.checked_add(1) else { return };
    app.shots.token = token;
    app.shots.queued.push((token, crate::now_ms() + 150.0, 0));
    // The window's picture starts at its content's corner.
    let view = view.translate(-ctx.content_rect().min.to_vec2());
    app.review.shot = Some(Shot { token, markup, view, scale: ctx.pixels_per_point(), until: crate::now_ms() + SHOT_WAIT_MS });
    ctx.request_repaint();
}

/// The window has a screenshot of its own waiting.
pub(crate) fn pending(app: &SolveApp) -> bool {
    app.review.shot.is_some()
}

/// A frame came: the one asked for is cut to the view and attached to its markup.
pub(crate) fn receive(app: &mut SolveApp, token: u64, image: Arc<ColorImage>) {
    let Some(shot) = app.review.shot.filter(|s| s.token == token) else { return };
    app.review.shot = None;
    let cut = crate::issue_report::ticket::crop(&image, Some(shot.view), shot.scale);
    let Some(png) = crate::issue_report::ticket::png(&cut) else { return };
    // The command takes a file: the picture goes through one in the temporary folder (in memory
    // in the browser), and its bytes end up in the design. The name can't be guessed, and a link
    // put there first is never followed.
    let random = {
        use std::hash::BuildHasher;
        let seed = (crate::now_ms().to_bits(), token);
        let a = std::collections::hash_map::RandomState::new().hash_one(seed);
        let b = std::collections::hash_map::RandomState::new().hash_one((a, seed));
        format!("{a:016x}{b:016x}")
    };
    let path = solidcraft_engine::io::vfs::temp_dir().join(format!("solidcraft-review-{random}-{token}.png"));
    let file = path.to_string_lossy().to_string();
    if solidcraft_engine::io::safe_write::write_atomic_no_follow(&path, &png).is_ok() {
        write(app, "markup.image", json!({"markup": shot.markup, "file": file, "name": "view.png"}));
        let _ = solidcraft_engine::io::vfs::remove(&file);
    }
}

/// Attach image…: a PNG or JPEG from the disk.
fn attach(app: &mut SolveApp, markup: u64) {
    let label = app.tr("review.pictures_label").to_owned();
    app.pick_file(&label, &["png", "jpg", "jpeg"], move |app, path| {
        if let Some(path) = path {
            app.review.attach = Some((markup, path));
        }
    });
}

#[cfg(test)]
#[path = "review_pins_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
