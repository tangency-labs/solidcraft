//! Tutorial mode: a window listing SolidCraft's functions. Clicking one builds a sample model in
//! a tutorial tab and performs the function on it, so you see what it does; "Try it yourself"
//! sets up the same sample and starts the command for you to use.
//!
//! Each lesson is a setup script (the sample it needs) and a demo script (the function itself),
//! both in the command-script format (`[{command, params}]`), in `tutorial_lessons.json`.
//!
//! The pointer that acts a lesson out is this module's own, not [`crate::agent_cursor`]'s. They
//! look alike and share [`crate::agent_cursor::stops`], but they are for different things: the
//! agent pointer shows what an agent is doing to the design and is held to a time budget per
//! request, while this one is teaching and takes as long as a person needs to follow it, and
//! shows the keys a person would press.

use std::collections::VecDeque;

use egui::Pos2;
use serde_json::{Value, json};
use solidcraft_engine::Session;

use crate::SolveApp;
use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::{self, Text};

/// The lessons: one per command the ribbon shows (`tutorial_lessons.json`), each a sample to set
/// up and the function performed on it.
const LESSONS_JSON: &str = include_str!("tutorial_lessons.json");
/// A small picture for the canvas and decal lessons (base64 PNG).
const IMAGE: &str = include_str!("tutorial_image.b64");

type Steps = Vec<(String, Value)>;

pub struct Lesson {
    /// The command it teaches (its toolbar button, and what "Try it yourself" starts).
    pub command: String,
    /// "Model › Create": the ribbon tab and panel it sits in.
    pub group: String,
    pub title: String,
    pub about: String,
    setup: Steps,
    /// The function performed; `None`: the lesson opens the command's own panel.
    demo: Option<Steps>,
}

/// Every lesson, in ribbon order.
pub fn lessons() -> &'static [Lesson] {
    static LESSONS: std::sync::OnceLock<Vec<Lesson>> = std::sync::OnceLock::new();
    LESSONS.get_or_init(|| parse(LESSONS_JSON).unwrap_or_default())
}

/// The groups, in ribbon order.
pub fn groups() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for l in lessons() {
        if !out.contains(&l.group.as_str()) {
            out.push(&l.group);
        }
    }
    out
}

/// A step: `["command", {params}]` or `{"command", "params"}`.
fn parse_step(v: &Value) -> Option<(String, Value)> {
    match v {
        Value::Array(a) => Some((a.first()?.as_str()?.to_string(), a.get(1).cloned().unwrap_or(json!({})))),
        Value::Object(o) => Some((o.get("command")?.as_str()?.to_string(), o.get("params").cloned().unwrap_or(json!({})))),
        _ => None,
    }
}

fn steps(v: &Value) -> Steps {
    v.as_array().map(|a| a.iter().filter_map(parse_step).collect()).unwrap_or_default()
}

/// Where a command sits on the ribbon, as the ribbon arranges it: this base lays the registry's
/// tabs out itself (`crate::workspace`), so "SOLID / MODIFY" in the registry is "Model › Modify"
/// here. A command the ribbon does not show (`mesh.convert`) keeps the registry's own names.
fn group_of(c: &solidcraft_engine::CommandSpec) -> String {
    let (tab, panel) = crate::workspace::destination(c).unwrap_or((c.tab, c.panel));
    if tab.is_empty() {
        return crate::workspace::word_cased("MESH MODIFY");
    }
    format!("{} \u{203a} {}", crate::workspace::word_cased(crate::workspace::tab_label(tab)), crate::workspace::word_cased(panel))
}

fn parse(src: &str) -> Result<Vec<Lesson>, String> {
    let d: Value = serde_json::from_str(src).map_err(|e| e.to_string())?;
    let samples = d.get("samples").cloned().unwrap_or_default();
    let specs = solidcraft_engine::command_specs();
    let mut out: Vec<(usize, Lesson)> = Vec::new();
    for l in d.get("lessons").and_then(Value::as_array).into_iter().flatten() {
        let Some(command) = l.get("command").and_then(Value::as_str) else { continue };
        let at = specs.iter().position(|c| c.id == command).unwrap_or(usize::MAX);
        let spec = specs.get(at);
        let group = spec.map(|c| group_of(c)).unwrap_or_else(|| crate::workspace::word_cased("MESH MODIFY"));
        let setup = match l.get("setup") {
            Some(Value::String(name)) => steps(samples.get(name).unwrap_or(&Value::Null)),
            Some(v) => steps(v),
            None => Vec::new(),
        };
        let demo = match l.get("demo") {
            Some(Value::String(s)) if s == "open" => None,
            Some(v) => Some(steps(v)),
            None => None,
        };
        out.push((
            at,
            Lesson {
                command: command.to_string(),
                group,
                title: l
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| spec.map(|c| c.label.to_string()))
                    .unwrap_or_else(|| command.to_string()),
                about: l.get("about").and_then(Value::as_str).unwrap_or_default().to_string(),
                setup,
                demo,
            },
        ));
    }
    // Ribbon order: by tab, then the command's place in the registry (which is the order the
    // panel lists them in).
    out.sort_by_key(|(at, l)| (groups_order(&l.group), *at));
    Ok(out.into_iter().map(|(_, l)| l).collect())
}

/// Where a group sits: the ribbon's own tab order (`crate::workspace::TABS`), then its panels in
/// command order. A group whose tab the ribbon does not show goes last.
fn groups_order(group: &str) -> usize {
    let tab = group.split(" \u{203a} ").next().unwrap_or("");
    crate::workspace::TABS
        .iter()
        .position(|t| crate::workspace::word_cased(crate::workspace::tab_label(t)) == tab)
        .unwrap_or(crate::workspace::TABS.len())
}

/// The folder lesson files go in (sample STEP, STL and design files).
fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft_tutorial_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// `{TMP}` (the lesson folder) and `{PNG}` (the sample picture) filled in.
fn expand(v: &Value) -> Value {
    match v {
        Value::String(s) if s == "{PNG}" => Value::String(IMAGE.trim().to_string()),
        Value::String(s) if s.contains("{TMP}") => Value::String(s.replace("{TMP}", &temp_dir().to_string_lossy().replace('\\', "/"))),
        Value::Array(a) => Value::Array(a.iter().map(expand).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), expand(v))).collect()),
        other => other.clone(),
    }
}

/// A sample file for the insert lessons: a 20 × 15 × 10 block as STEP, STL or a design.
fn sample_file(name: &str) -> Result<(), String> {
    let path = temp_dir().join(name).to_string_lossy().replace('\\', "/");
    let mut s = Session::default();
    s.doc_mut().name = "Block".into();
    s.execute("solid.box", &json!({"length": 20, "width": 15, "height": 10, "name": "Block", "body_name": "Block"})).map_err(|e| e.to_string())?;
    let r = match name.rsplit('.').next().unwrap_or("") {
        "stl" | "3mf" => s.execute("file.save_mesh", &json!({"path": path})),
        "solidcraft" => s.execute("file.save_as", &json!({"path": path})),
        _ => s.execute("file.export", &json!({"path": path})),
    };
    r.map(|_| ()).map_err(|e| e.to_string())
}

/// Run steps on a session; the last result.
fn run_steps(s: &mut Session, list: &Steps) -> Result<Value, String> {
    let mut last = Value::Null;
    for (i, (cmd, params)) in list.iter().enumerate() {
        let params = expand(params);
        if cmd == "tutorial.file" {
            sample_file(params.get("name").and_then(Value::as_str).unwrap_or("Block.step"))
                .map_err(|e| format!("step {} (sample file): {e}", i + 1))?;
            continue;
        }
        last = s.execute(cmd, &params).map_err(|e| format!("step {} ({cmd}): {e}", i + 1))?;
    }
    Ok(last)
}

/// Tutorial mode's state.
#[derive(Default)]
pub struct State {
    pub open: bool,
    pub lesson: Option<usize>,
    /// What the last lesson's function returned, or why it failed.
    pub result: Option<Result<String, String>>,
    /// The lesson being acted out with the tutorial pointer and keys.
    pub player: Option<Player>,
    /// The pointer's place (kept between lessons) and its click ripples.
    pub pointer: Option<Pos2>,
    ripples: Vec<(Pos2, f64)>,
    /// Keys shown as pressed: the keycaps and when they appeared.
    keys: Option<(Vec<String>, f64)>,
    /// The window's search text.
    filter: String,
    /// The lesson whose group was last opened in the list.
    opened_for: Option<usize>,
    /// Speaks the lesson's explanation and what the demo is doing (`crate::narration`).
    pub narrator: crate::narration::Narrator,
}

/// Speak a line of tutorial narration when the preference is on; otherwise do nothing. Returns
/// the neural voice's sequence number for the line (for pacing the demo to it), or `None`.
fn narrate(app: &mut SolveApp, text: &str, interrupt: bool) -> Option<u64> {
    if app.preferences.tutorial_narration { app.tutorial.narrator.speak(text, interrupt) } else { None }
}

/// A lesson acted out: the pointer clicks the command's toolbar button (its shortcut shown as
/// pressed keys), clicks each pick on the model, types the values, and then the command runs.
pub struct Player {
    steps: VecDeque<(String, Value)>,
    leg: Option<Leg>,
    /// Waiting before the next step (app time, seconds).
    wait_until: f64,
    last: Value,
    /// How many demo steps have started, so the spoken commentary can lead with "First",
    /// "Next" or "Finally" (`crate::narration`).
    step_no: usize,
    /// Pace the demo to the neural voice: wait for a spoken line (by its sequence number) to
    /// begin (before the pointer acts) or finish (before the next step), with a deadline so a
    /// stalled engine never freezes the demo. `None` when not pacing (system voice, or
    /// narration off).
    gate: Option<(Phase, u64, f64)>,
    /// A step whose pointer action is waiting for its spoken line to begin playing.
    pending_leg: Option<(String, Value)>,
    /// The sequence number of the current step's spoken cue, so the next step waits for it to
    /// finish. `None` for the system voice, or between steps.
    cue_seq: Option<u64>,
}

/// Which edge of a spoken line the demo waits for before moving on.
#[derive(Clone, Copy)]
enum Phase {
    /// The line has begun playing -- then the pointer may act it out.
    Start,
    /// The line has finished -- then the next step may begin.
    End,
}

/// A safety cap (seconds) on waiting for one spoken line, so a stalled voice never freezes the
/// demo. Generous, because the first line also waits for the model to load; a failed engine does
/// not reach this (it stops reporting progress, and the demo then runs unpaced).
const LINE_MAX: f64 = 30.0;

struct Leg {
    command: String,
    params: Value,
    /// Where the pointer goes, and the keys shown when it clicks there.
    stops: VecDeque<(Pos2, Vec<String>)>,
    from: Pos2,
    started: f64,
    clicked: bool,
}

/// Seconds to glide to a stop, to hold the click, and between steps. Slower than the agent
/// pointer's, which has a time budget: this one is being followed by a person.
const GLIDE: f64 = 0.9;
const CLICK: f64 = 0.6;
const PAUSE: f64 = 0.5;

/// Parameters a person types into a command's panel, in the order they'd type them.
/// `crate::narration` reads the same list, so the spoken value matches the keys shown.
pub const TYPED: &[&str] = &[
    "distance",
    "radius",
    "diameter",
    "thickness",
    "length",
    "width",
    "height",
    "major",
    "minor",
    "factor",
    "count",
    "count1",
    "angle",
    "value",
    "sides",
    "size",
    "text",
    "designation",
];

/// A shortcut ("Shift+B") as keycaps.
fn keycaps(shortcut: &str) -> Vec<String> {
    shortcut.split('+').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect()
}

/// What a person types for a command: its first typed value, then Enter.
fn typed_keys(params: &Value) -> Vec<String> {
    for k in TYPED {
        let text = match params.get(k) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => continue,
        };
        return vec![text, "Enter".into()];
    }
    Vec::new()
}

impl Leg {
    fn new(app: &SolveApp, command: String, params: Value) -> Leg {
        let id = solidcraft_engine::find_command(&command).map_or(command.clone(), |c| c.id.to_string());
        let places = crate::agent_cursor::stops(app, &id, &params);
        let button = crate::scenario::handle_at(&format!("toolbar:{id}")).or_else(|| {
            solidcraft_engine::find_command(&id)
                .and_then(crate::workspace::destination)
                .and_then(|(t, p)| crate::scenario::handle_at(&format!("panel:{t}:{p}")))
        });
        let shortcut = crate::keymap::effective(app, &id).map(|k| keycaps(&k)).unwrap_or_default();
        let typed = typed_keys(&params);
        let mut stops: VecDeque<(Pos2, Vec<String>)> = VecDeque::new();
        for (i, p) in places.iter().enumerate() {
            let keys = if i == 0 && Some(*p) == button { shortcut.clone() } else { Vec::new() };
            stops.push_back((*p, keys));
        }
        // The values are typed after the last pick (or straight after the button).
        if !typed.is_empty() {
            match stops.back_mut() {
                Some((_, k)) if k.is_empty() => *k = typed,
                Some((p, _)) => {
                    let p = *p;
                    stops.push_back((p, typed));
                }
                None => {}
            }
        }
        Leg { command, params, stops, from: app.tutorial.pointer.unwrap_or_else(|| start_point(app)), started: app.now, clicked: false }
    }
}

fn start_point(app: &SolveApp) -> Pos2 {
    app.viewport.rect.map(|r| r.center()).unwrap_or(egui::pos2(600.0, 400.0))
}

fn ease(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t)) as f32
}

/// A lesson's demo as (command, params) steps.
fn demo_steps(l: &Lesson) -> VecDeque<(String, Value)> {
    l.demo.iter().flatten().map(|(c, p)| (c.clone(), expand(p))).collect()
}

/// Act out a lesson: set up its sample, then play its demo with the pointer and keys.
pub fn play(app: &mut SolveApp, i: usize) {
    let Some(l) = lessons().get(i) else { return };
    app.tutorial.lesson = Some(i);
    app.home.open = false;
    // Frame the finished result, so everything the function makes is in view from the start.
    let framed = build(i, true).ok().and_then(|(done, _)| framing(&done));
    match build(i, false) {
        Ok((s, _)) => {
            show_design(app, s);
            match framed {
                Some(b) => app.fit_bounds(b),
                None => app.fit_view(),
            }
            app.cam_anim = None;
            app.tutorial.result = None;
            // A lesson without a demo opens the command's own panel.
            let steps = if l.demo.is_some() { demo_steps(l) } else { VecDeque::from([(l.command.clone(), json!({OPEN: true}))]) };
            // A frame first, so the toolbar and the view have their places.
            app.tutorial.player = Some(Player {
                steps,
                leg: None,
                wait_until: app.now + PAUSE,
                last: Value::Null,
                step_no: 0,
                gate: None,
                pending_leg: None,
                cue_seq: None,
            });
            // Explain the tool and where to find it; the interrupt clears whatever the last
            // lesson was still saying.
            let shortcut = crate::keymap::effective(app, &l.command);
            let line = crate::narration::intro(&l.title, &l.about, &l.group, shortcut.as_deref());
            let intro_seq = narrate(app, &line, true);
            // While the neural voice is speaking, hold the first step until the explanation ends.
            if let Some(seq) = intro_seq
                && let Some(p) = app.tutorial.player.as_mut()
            {
                p.gate = Some((Phase::End, seq, app.now + LINE_MAX));
            }
        }
        Err(e) => {
            app.tutorial.player = None;
            app.tutorial.result = Some(Err(e));
        }
    }
}

/// The view box for a lesson's design: its bounds with room around them, and at least 80 mm
/// across, so small samples aren't seen from too close.
fn framing(s: &Session) -> Option<solidcraft_engine::geom::Aabb3> {
    use solidcore_record::Vec3;
    let b = solidcraft_engine::view::bounds(s).ok()?;
    if b.is_empty() {
        return None;
    }
    let c = Vec3::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0, (b.min.z + b.max.z) / 2.0);
    let mut h = [(b.max.x - b.min.x) / 2.0 * 1.35, (b.max.y - b.min.y) / 2.0 * 1.35, (b.max.z - b.min.z) / 2.0 * 1.35];
    let diag = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt();
    if diag < 40.0 {
        let k = 40.0 / diag.max(1e-6);
        h = h.map(|x| (x * k).max(10.0));
    }
    Some(solidcraft_engine::geom::Aabb3 { min: Vec3::new(c.x - h[0], c.y - h[1], c.z - h[2]), max: Vec3::new(c.x + h[0], c.y + h[1], c.z + h[2]) })
}

/// A step that opens its command's panel instead of running it.
const OPEN: &str = "tutorial_open";

/// A click on a ribbon command while tutorial mode is open: its lesson is acted out instead of
/// the command starting, and the click is used up (`true`). The hook is on the ribbon's own
/// `start_at` and `start_pinned`, which is where a person clicking a button arrives; the demo
/// runs its commands through `run` and `start`, so a lesson never catches its own steps.
pub fn toolbar_start(app: &mut SolveApp, id: &str) -> bool {
    if !app.tutorial.open {
        return false;
    }
    let Some(i) = lessons().iter().position(|l| l.command == id) else { return false };
    play(app, i);
    true
}

/// Move the player along (each frame).
pub fn step(app: &mut SolveApp, ctx: &egui::Context) {
    let now = app.now;
    app.tutorial.ripples.retain(|(_, t)| now - t < 0.6);
    if app.tutorial.keys.as_ref().is_some_and(|(_, t)| now - t > 1.6) {
        app.tutorial.keys = None;
    }
    if !app.tutorial.ripples.is_empty() || app.tutorial.keys.is_some() {
        ctx.request_repaint();
    }
    let Some(mut player) = app.tutorial.player.take() else { return };
    ctx.request_repaint();
    if now < player.wait_until {
        app.tutorial.player = Some(player);
        return;
    }
    // Pace to the neural voice: hold here until the spoken line reaches the awaited edge (or the
    // deadline passes, so a stalled voice never freezes the demo).
    if let Some((phase, seq, deadline)) = player.gate {
        if now < deadline && !gate_reached(app, phase, seq) {
            app.tutorial.player = Some(player);
            return;
        }
        player.gate = None;
        // A pointer action that was waiting for its line to begin: start it now, freshly timed.
        if let Some((cmd, params)) = player.pending_leg.take() {
            player.leg = Some(Leg::new(app, cmd, params));
            app.tutorial.player = Some(player);
            return;
        }
    }
    let Some(mut leg) = player.leg.take() else {
        match player.steps.pop_front() {
            Some((cmd, params)) => {
                // Name the tool and any value as the pointer reaches it (queued after the intro).
                let first = player.step_no == 0;
                let last = player.steps.is_empty();
                player.step_no += 1;
                let label = solidcraft_engine::find_command(&cmd).map_or(cmd.as_str(), |c| c.label).to_string();
                let line = crate::narration::step_cue(&label, &params, first, last);
                // Pacing: speak the cue, then hold the pointer until it begins; otherwise act now.
                match narrate(app, &line, false) {
                    Some(s) => {
                        player.cue_seq = Some(s);
                        player.gate = Some((Phase::Start, s, app.now + LINE_MAX));
                        player.pending_leg = Some((cmd, params));
                    }
                    None => player.leg = Some(Leg::new(app, cmd, params)),
                }
            }
            None => {
                let text = summary(&player.last);
                let line = crate::narration::result(&text);
                narrate(app, &line, false);
                app.tutorial.result = Some(Ok(text));
                return;
            }
        }
        app.tutorial.player = Some(player);
        return;
    };
    match leg.stops.front().cloned() {
        Some((to, keys)) => {
            if !leg.clicked {
                let t = (now - leg.started) / GLIDE;
                app.tutorial.pointer = Some(leg.from + (to - leg.from) * ease(t));
                if t >= 1.0 {
                    leg.clicked = true;
                    leg.started = now;
                    app.tutorial.ripples.push((to, now));
                    if !keys.is_empty() {
                        app.tutorial.keys = Some((keys, now));
                    }
                }
            } else if now - leg.started >= CLICK {
                leg.stops.pop_front();
                leg.from = to;
                leg.started = now;
                leg.clicked = false;
            }
            player.leg = Some(leg);
        }
        None => {
            // Every place clicked: the command runs.
            if leg.params.get(OPEN).is_some() {
                app.start(&leg.command);
                app.tutorial.result = Some(Ok(String::new()));
                return;
            }
            match app.run(&leg.command, leg.params.clone()) {
                Ok(v) => {
                    player.last = v;
                    // A tutorial's design is scratch: never "unsaved", so never autosaved.
                    app.session.mark_saved();
                }
                Err(e) => {
                    app.tutorial.result = Some(Err(format!("{}: {e}", leg.command)));
                    return;
                }
            }
            // Pacing: hold the next step until this one's spoken line finishes; otherwise a beat.
            match player.cue_seq.take() {
                Some(s) => player.gate = Some((Phase::End, s, now + LINE_MAX)),
                None => player.wait_until = now + PAUSE,
            }
        }
    }
    app.tutorial.player = Some(player);
}

/// Whether the spoken line `seq` has reached `phase` (begun or finished). Where the neural voice
/// is not the one speaking (`progress` is `None`), there is nothing to wait for.
fn gate_reached(app: &SolveApp, phase: Phase, seq: u64) -> bool {
    match app.tutorial.narrator.progress() {
        Some((started, done)) => match phase {
            Phase::Start => started >= seq,
            Phase::End => done >= seq,
        },
        None => true,
    }
}

/// Draw the tutorial pointer, its click ripples and the pressed keys over everything.
pub fn paint(app: &SolveApp, ctx: &egui::Context) {
    if !app.tutorial.open {
        return;
    }
    let t = crate::theme::Tokens::get();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::unique("sc_tutorial_pointer")));
    let accent = t.manipulator;
    for (at, t0) in &app.tutorial.ripples {
        let k = ((app.now - t0) / 0.6).clamp(0.0, 1.0) as f32;
        painter.circle_stroke(*at, 6.0 + 24.0 * k, egui::Stroke::new(3.0 * (1.0 - k) + 0.5, accent.gamma_multiply(1.0 - k)));
    }
    if let Some(p) = app.tutorial.pointer.filter(|_| app.tutorial.player.is_some() || !app.tutorial.ripples.is_empty()) {
        let pts: Vec<Pos2> = [(0.0, 0.0), (0.0, 19.0), (5.0, 14.5), (8.5, 22.0), (12.0, 20.5), (8.5, 13.5), (14.5, 13.5)]
            .iter()
            .map(|(x, y)| p + egui::vec2(*x, *y))
            .collect();
        painter.add(egui::Shape::convex_polygon(pts.clone(), egui::Color32::WHITE, egui::Stroke::NONE));
        painter.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.5, egui::Color32::BLACK)));
        let label = painter.layout_no_wrap(crate::i18n::tr("tutorial.pointer").to_string(), egui::FontId::proportional(t.font_small), egui::Color32::BLACK);
        let r = egui::Rect::from_min_size(p + egui::vec2(16.0, 20.0), label.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(r, 4.0, accent);
        painter.galley(r.min + egui::vec2(5.0, 2.0), label, egui::Color32::BLACK);
    }
    // Pressed keys: keycaps at the bottom of the view.
    if let (Some((keys, t0)), Some(view)) = (&app.tutorial.keys, app.viewport.rect) {
        let fade = (1.0 - ((app.now - t0 - 1.2) / 0.4).clamp(0.0, 1.0)) as f32;
        let font = egui::FontId::proportional(t.font_page_heading);
        let caps: Vec<_> = keys.iter().map(|k| painter.layout_no_wrap(k.clone(), font.clone(), egui::Color32::BLACK)).collect();
        let gap = 14.0;
        let widths: Vec<f32> = caps.iter().map(|g| g.size().x.max(18.0) + 24.0).collect();
        let w: f32 = widths.iter().sum::<f32>() + gap * caps.len().saturating_sub(1) as f32;
        let mut x = view.center().x - w / 2.0;
        let y = view.bottom() - 120.0;
        for (i, (g, cw)) in caps.into_iter().zip(widths).enumerate() {
            if i > 0 {
                painter.text(
                    egui::pos2(x - gap / 2.0, y + 22.0),
                    egui::Align2::CENTER_CENTER,
                    "+",
                    egui::FontId::proportional(t.font_intro),
                    egui::Color32::WHITE.gamma_multiply(fade),
                );
            }
            let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw, 44.0));
            painter.rect_filled(r.translate(egui::vec2(0.0, 4.0)), 7.0, egui::Color32::from_gray(90).gamma_multiply(fade));
            painter.rect_filled(r, 7.0, egui::Color32::from_gray(245).gamma_multiply(fade));
            painter.rect_stroke(r, 7.0, egui::Stroke::new(1.5, accent.gamma_multiply(fade)), egui::StrokeKind::Inside);
            painter.galley(r.center() - g.size() / 2.0, g, egui::Color32::BLACK.gamma_multiply(fade));
            x += cw + gap;
        }
    }
}

/// A lesson's design: the sample, then (when `demo`) the function performed on it.
pub fn build(i: usize, demo: bool) -> Result<(Session, Value), String> {
    let l = lessons().get(i).ok_or("no such lesson")?;
    let mut s = Session::default();
    s.doc_mut().name = format!("{} \u{b7} {}", crate::i18n::tr("tutorial.title"), l.title);
    run_steps(&mut s, &l.setup).map_err(|e| format!("setting up the sample: {e}"))?;
    s.mark_saved();
    s.undo.clear();
    let mut last = Value::Null;
    if demo
        && let Some(d) = &l.demo
    {
        last = run_steps(&mut s, d)?;
        s.mark_saved();
    }
    Ok((s, last))
}

/// Whether a design is a tutorial's scratch design (replaced in place by the next lesson).
fn is_tutorial_design(app: &SolveApp) -> bool {
    app.session.doc.name.starts_with(&format!("{} \u{b7} ", crate::i18n::tr("tutorial.title")))
}

/// Show a lesson's design: in place of the tutorial (or a blank) tab, otherwise in a new tab.
fn show_design(app: &mut SolveApp, s: Session) {
    crate::dialogs::cancel(app);
    app.tool = None;
    if is_tutorial_design(app) {
        crate::documents::replace_active(app, s);
    } else {
        crate::documents::adopt(app, s);
    }
}

/// Run a lesson: perform it (`try_it` false) or set it up and start its command.
pub fn run(app: &mut SolveApp, i: usize, try_it: bool) {
    app.tutorial.player = None;
    app.tutorial.lesson = Some(i);
    app.home.open = false;
    match build(i, !try_it) {
        Ok((s, last)) => {
            let framed = framing(&s);
            show_design(app, s);
            match framed {
                Some(b) => app.fit_bounds(b),
                None => app.fit_view(),
            }
            if let Some(l) = lessons().get(i) {
                let shortcut = crate::keymap::effective(app, &l.command);
                let line = crate::narration::intro(&l.title, &l.about, &l.group, shortcut.as_deref());
                narrate(app, &line, true);
            }
            app.tutorial.result = Some(Ok(summary(&last)));
            if try_it
                && let Some(l) = lessons().get(i)
            {
                app.start(&l.command);
            }
        }
        Err(e) => app.tutorial.result = Some(Err(e)),
    }
}

/// A short, readable account of a command's result.
fn summary(v: &Value) -> String {
    let Some(o) = v.as_object() else { return String::new() };
    let mut out = Vec::new();
    let num = |x: &Value| x.as_f64().map(|f| format!("{f:.2}"));
    if let Some(b) = o.get("bodies").and_then(Value::as_array).and_then(|b| b.first()).filter(|b| b.is_object()) {
        for (k, label, unit) in
            [("volume_mm3", "tutorial.volume", "mm\u{b3}"), ("area_mm2", "tutorial.area", "mm\u{b2}")]
        {
            if let Some(n) = b.get(k).and_then(num) {
                out.push(format!("{}: {n} {unit}", crate::i18n::tr(label)));
            }
        }
        if let (Some(lo), Some(hi)) = (b.pointer("/bbox/min").and_then(Value::as_array), b.pointer("/bbox/max").and_then(Value::as_array)) {
            let d: Vec<String> = lo.iter().zip(hi).filter_map(|(a, b)| Some(format!("{:.1}", b.as_f64()? - a.as_f64()?))).collect();
            out.push(format!("{}: {} mm", crate::i18n::tr("tutorial.size"), d.join(" \u{d7} ")));
        }
    }
    if let Some(c) = o.get("center").and_then(Value::as_array) {
        let c: Vec<String> = c.iter().filter_map(num).collect();
        out.push(format!("{}: ({})", crate::i18n::tr("tutorial.centre"), c.join(", ")));
    }
    if let Some(m) = o.get("mass_g").and_then(num) {
        out.push(format!("{}: {m} g", crate::i18n::tr("tutorial.mass")));
    }
    out.join("\n")
}

/// The tutorial window: what the lesson shown is about, and the list of every function.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.tutorial.open {
        return;
    }
    let mut pick: Option<(usize, bool)> = None;
    // Down the left, clear of the ribbon above it, of the history bar below it and of the
    // command panels that open at the top right, and movable from there.
    let at = ctx.content_rect().left_bottom() + egui::vec2(12.0, -(crate::theme::Tokens::get().space_24 * 2.5));
    let (closed, _) = ui_kit::Window::new(egui::Id::unique("sc_tutorial"), "help", crate::i18n::tr("tutorial.title"))
        .cards(1.6)
        .placed(egui::Align2::LEFT_BOTTOM, at)
        .actions("", crate::i18n::tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui.spacing_mut().item_spacing.y = crate::theme::Tokens::get().space_10;
            ui_kit::text(ui, crate::i18n::tr("tutorial.intro"), Text::Hint("intro"));
            if ui_kit::checkbox(ui, crate::i18n::tr("tutorial.narration"), &mut app.preferences.tutorial_narration)
                .backing_on_hover_text(crate::i18n::tr("tutorial.narration_hint"))
                .changed()
                && !app.preferences.tutorial_narration
            {
                app.tutorial.narrator.stop();
            }
            if let Some(i) = app.tutorial.lesson
                && let Some(l) = lessons().get(i)
            {
                ui_kit::text(ui, crate::i18n::tr_label(&l.title), Text::Heading("lesson"));
                ui_kit::text(ui, crate::i18n::tr_label(&l.about), Text::Note("about"));
                if let Some(key) = crate::keymap::effective(app, &l.command).filter(|k| !k.is_empty()) {
                    ui_kit::text(ui, &format!("{}: {key}", crate::i18n::tr("tutorial.shortcut")), Text::Note("shortcut"));
                }
                if app.tutorial.player.is_some() {
                    ui_kit::text(ui, crate::i18n::tr("tutorial.watching"), Text::Note("watching"));
                }
                match &app.tutorial.result {
                    Some(Ok(text)) if !text.is_empty() => {
                        ui_kit::text(ui, text, Text::MonoNote("result"));
                    }
                    Some(Err(e)) => {
                        ui_kit::text(ui, e, Text::Error("result"));
                    }
                    _ => {}
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = crate::theme::Tokens::get().space_12;
                    if ui_kit::primary_button(ui, crate::i18n::tr("tutorial.replay")).clicked() {
                        pick = Some((i, false));
                    }
                    if ui_kit::text_button(ui, crate::i18n::tr("tutorial.try_it"), true).clicked() {
                        pick = Some((i, true));
                    }
                    if ui_kit::text_button(ui, crate::i18n::tr("tutorial.next"), i + 1 < lessons().len()).clicked() {
                        pick = Some((i + 1, false));
                    }
                });
                ui_kit::divider(ui, "lesson");
            }
            ui_kit::search_field(ui, egui::Id::unique("sc_tutorial_find"), &mut app.tutorial.filter, crate::i18n::tr("tutorial.find_hint"));
            ui_kit::text(ui, &crate::i18n::tr("tutorial.count").replace("{n}", &lessons().len().to_string()), Text::Note("count"));
            let needle = app.tutorial.filter.trim().to_lowercase();
            let current = app.tutorial.lesson.and_then(|i| lessons().get(i)).map(|l| l.group.as_str());
            let newly_picked = app.tutorial.opened_for != app.tutorial.lesson;
            app.tutorial.opened_for = app.tutorial.lesson;
            let rows = ui_kit::row_height() * 12.0;
            ui_kit::report_list(ui, "tutorial", rows, |ui| {
                let mut any = false;
                for g in groups() {
                    let found: Vec<(usize, &Lesson)> = lessons()
                        .iter()
                        .enumerate()
                        .filter(|(_, l)| l.group == g)
                        .filter(|(_, l)| {
                            needle.is_empty()
                                || l.title.to_lowercase().contains(&needle)
                                || l.command.contains(&needle)
                                || l.about.to_lowercase().contains(&needle)
                        })
                        .collect();
                    if found.is_empty() {
                        continue;
                    }
                    any = true;
                    // Searching opens every group with a match; otherwise the lesson's own group.
                    let open = !needle.is_empty() || (current == Some(g) && newly_picked);
                    ui_kit::section(ui, ("tutorial_group", g), &format!("{} ({})", crate::i18n::tr_label(g), found.len()), open, |ui| {
                        for (i, l) in found {
                            let r = ui_kit::list_picker(ui, &l.command, crate::i18n::tr_label(&l.title), app.tutorial.lesson == Some(i));
                            crate::scenario::publish_handle(&format!("tutorial:{}", l.command), r.rect.center());
                            if r.clicked() {
                                pick = Some((i, false));
                            }
                        }
                    });
                }
                if !any {
                    ui_kit::empty_box(ui, "none", crate::i18n::tr("tutorial.nothing_found"));
                }
            });
        });
    if closed {
        app.tutorial.open = false;
        app.tutorial.narrator.stop();
    }
    if let Some((i, try_it)) = pick {
        if try_it {
            run(app, i, true);
        } else {
            play(app, i);
        }
    }
}

/// Start tutorial mode (Home's Tutorial button): the window, with the first lesson performed.
pub fn start(app: &mut SolveApp) {
    app.tutorial.open = true;
    app.home.open = false;
    play(app, app.tutorial.lesson.unwrap_or(0));
}

/// Control channel: `{open?: bool, lesson?: command id or index, try?: bool}` → the state.
pub fn control(app: &mut SolveApp, p: &Value) -> Result<Value, String> {
    if let Some(o) = p.get("open").and_then(Value::as_bool) {
        app.tutorial.open = o;
        if o {
            app.home.open = false;
        } else {
            app.tutorial.narrator.stop();
        }
    }
    if let Some(n) = p.get("narrate").and_then(Value::as_bool) {
        app.preferences.tutorial_narration = n;
        if !n {
            app.tutorial.narrator.stop();
        }
    }
    if let Some(l) = p.get("lesson") {
        let i = match l {
            Value::Number(n) => n.as_u64().map(|n| n as usize),
            Value::String(s) => lessons().iter().position(|x| x.command == *s || x.title.eq_ignore_ascii_case(s)),
            _ => None,
        }
        .filter(|i| *i < lessons().len())
        .ok_or("no such lesson")?;
        app.tutorial.open = true;
        if p.get("try").and_then(Value::as_bool).unwrap_or(false) {
            run(app, i, true);
        } else if p.get("animate").and_then(Value::as_bool).unwrap_or(true) {
            play(app, i);
        } else {
            run(app, i, false);
        }
    }
    Ok(json!({
        "open": app.tutorial.open,
        "playing": app.tutorial.player.is_some(),
        "narration": app.preferences.tutorial_narration,
        "lesson": app.tutorial.lesson.and_then(|i| lessons().get(i)).map(|l| l.command.as_str()),
        "result": match &app.tutorial.result { Some(Ok(t)) => json!({"ok": t}), Some(Err(e)) => json!({"error": e}), None => Value::Null },
        "lessons": lessons().iter().map(|l| json!({"command": l.command, "title": l.title, "group": l.group})).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn every_lesson_builds_its_sample_and_performs_its_function() {
        let mut failed = Vec::new();
        for (i, l) in lessons().iter().enumerate() {
            assert!(groups().contains(&l.group.as_str()), "{}: unknown group", l.title);
            assert!(solidcraft_engine::find_command(&l.command).is_some(), "{}: no command {}", l.title, l.command);
            if let Err(e) = build(i, false) {
                failed.push(format!("{} (setup): {e}", l.title));
            }
            if let Err(e) = build(i, true) {
                failed.push(format!("{}: {e}", l.title));
            }
        }
        assert!(failed.is_empty(), "lessons failed:\n{}", failed.join("\n"));
    }

    /// Every function the ribbon shows is a function someone may want shown to them. The ribbon
    /// here is laid out by `crate::workspace`, not by the registry's own tab and panel, so that
    /// is what decides which commands need a lesson.
    #[test]
    fn every_ribbon_command_has_a_lesson() {
        let missing: Vec<&str> = solidcraft_engine::command_specs()
            .iter()
            .filter(|c| crate::workspace::destination(c).is_some())
            // A planned command answers "not supported yet": there is nothing to show of it.
            .filter(|c| c.planned.is_none())
            .filter(|c| !lessons().iter().any(|l| l.command == c.id))
            .map(|c| c.id)
            .collect();
        assert!(missing.is_empty(), "no lesson for: {}", missing.join(", "));
        let mut seen = std::collections::BTreeSet::new();
        assert!(lessons().iter().all(|l| seen.insert(l.command.as_str())), "a command has two lessons");
    }

    #[test]
    fn the_keys_shown_are_the_shortcut_and_the_typed_value() {
        assert_eq!(keycaps("Shift+B"), ["Shift", "B"]);
        assert_eq!(typed_keys(&json!({"edges": [[0, 0, 0]], "radius": 4})), ["4", "Enter"]);
        assert_eq!(typed_keys(&json!({"angle": "30 deg"})), ["30 deg", "Enter"]);
        assert!(typed_keys(&json!({"bodies": ["Plate"]})).is_empty());
        // Every demo step is a command.
        for l in lessons().iter().filter(|l| l.demo.is_some()) {
            assert!(!demo_steps(l).is_empty(), "{}", l.title);
            assert!(demo_steps(l).iter().all(|(c, _)| solidcraft_engine::find_command(c).is_some()), "{}", l.title);
        }
    }

    #[test]
    fn the_demo_changes_the_design() {
        let i = lessons().iter().position(|l| l.command == "solid.box").unwrap();
        assert!(build(i, false).unwrap().0.doc.features.is_empty());
        assert_eq!(build(i, true).unwrap().0.doc.features.len(), 1);
    }
}
