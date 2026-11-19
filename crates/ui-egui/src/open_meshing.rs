//! A design that has just opened shows its parts as they are meshed (#3053).
//!
//! Opening hands the design to the window once its timeline is evaluated (or taken from its
//! file) and placed; the display meshes are made afterwards, off the UI thread. Until they all
//! are, the viewport draws the bodies already meshed ([`shown_world`]), so the first parts show
//! within moments and the window stays responsive while the rest come in. The first placement of
//! every part is meshed first: its other placements are drawn from that mesh. The view is framed from
//! the bodies' outlines meanwhile, and framed again from the meshes when they are all made, unless
//! the camera was moved in between. Editing the design meanwhile meshes the new world instead.
//!
//! The picture lists the bodies in the order they were meshed, and is kept once complete until
//! the design changes: the viewport's scene keeps the blocks it built for the bodies already
//! shown and builds only the new ones, and finishing does not build the whole scene again. It is
//! remade at most every [`REFRESH`] seconds, and less often while a remake takes longer.
//!
//! Display only: commands, picking of what is drawn and exports read the session's world as ever.

use crate::ui_kit::{Container, Surface};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use solidcraft_engine::doc::{ModelBody, ModelState};

use crate::SolveApp;

/// How often a growing picture is remade at most (s).
const REFRESH: f64 = 0.25;
/// A remake waits this many times as long as the last scene build took, so remakes take at most
/// a small share of the window thread.
const REFRESH_SHARE: f64 = 6.0;

pub(crate) struct Meshing {
    /// The world being meshed (the session's when it started).
    world: Arc<ModelState>,
    /// The bodies of `world` whose display (mesh, and plans when drawn from them) is made, in
    /// the order they were made.
    ready: Arc<Mutex<Vec<usize>>>,
    done: Arc<AtomicUsize>,
    /// All are meshed and framed: the picture is kept until the design changes.
    complete: bool,
    stop: Arc<AtomicBool>,
    /// The picture so far: the bodies shown, and how many were ready when it was made.
    shown: Option<(usize, Arc<ModelState>)>,
    shown_at: f64,
    /// The camera the outline framing left (a refit at the end only replaces that one).
    framed: Option<crate::Camera>,
    /// Draw the design's Home picture when all is meshed.
    remember: Option<String>,
    /// When it started, and whether the first bodies have been shown (`SOLIDCRAFT_PROFILE_OPEN`).
    began: Option<f64>,
    first_shown: bool,
    /// The parts placed more than once, made ahead for the scenes that draw them as instances.
    parts: crate::viewport::WarmedParts,
    /// What the viewport drew with when it started ([`Caps`]): a viewport that changes (its GPU
    /// pipelines ready after the design opened) meshes again for the new one.
    caps: Caps,
}

/// What a viewport draws bodies from, which the meshing makes ahead: display plans (`plans`),
/// shared vertices (`shared`, a GPU viewport) and instances' shared edges (`edges`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Caps {
    plans: bool,
    shared: bool,
    edges: bool,
}

impl Caps {
    fn of(app: &SolveApp) -> Caps {
        let gpu = app.viewport.gpu;
        Caps { plans: gpu.is_some_and(|g| g.tess), shared: gpu.is_some(), edges: gpu.is_some_and(|g| g.edges) }
    }
}

impl Meshing {
    /// `plans`: the bodies are drawn from their display plans; `shared`: large meshes and parts
    /// placed more than once are drawn from shared vertices (a GPU viewport), made here too, with
    /// the parts' shared edges when the renderer draws instances' edges (`edges`).
    fn start(world: Arc<ModelState>, caps: Caps) -> Meshing {
        let Caps { plans, shared, edges } = caps;
        let n = world.bodies.len();
        let ready = Arc::new(Mutex::new(Vec::with_capacity(n)));
        let (done, stop) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        // The first placement of each part first: the others only move its mesh.
        let mut seen = std::collections::HashSet::new();
        let (mut first, mut rest) = (Vec::new(), Vec::new());
        for (i, b) in world.bodies.iter().enumerate() {
            if seen.insert(b.definition_name().to_string()) { first.push(i) } else { rest.push(i) }
        }
        first.extend(rest);
        let repeated = repeated_parts(&world);
        let parts = crate::viewport::WarmedParts::default();
        let job = (world.clone(), ready.clone(), done.clone(), stop.clone(), repeated, parts.clone());
        let spawned = threads()
            && std::thread::Builder::new()
                .name("open-meshing".into())
                .spawn(move || {
                    let (world, ready, done, stop, repeated, parts) = job;
                    solidcraft_engine::kernel::for_each_on_threads(&first, 16, |&i| {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        if let Some(b) = world.bodies.get(i) {
                            let q = solidcraft_engine::doc::display_quality::current();
                            warm(b, plans, shared, &repeated, q);
                            if shared && b.placed_definition().is_some_and(|id| repeated.contains(&id)) {
                                warm_part(b, &parts, edges, q);
                            }
                        }
                        if let Ok(mut r) = ready.lock() {
                            r.push(i);
                        }
                        done.fetch_add(1, Ordering::Release);
                    });
                })
                .is_ok();
        if !spawned {
            // No thread (the web build): the viewport meshes as it draws, as before. Every body
            // is listed ready too, or the picture stays empty with "Loading parts n/n" (#10198).
            if let Ok(mut r) = ready.lock() {
                r.extend(0..n);
            }
            done.store(n, Ordering::Release);
        }
        Meshing {
            world,
            ready,
            done,
            stop,
            complete: false,
            shown: None,
            shown_at: f64::NEG_INFINITY,
            framed: None,
            remember: None,
            began: None,
            first_shown: false,
            parts,
            caps,
        }
    }

    fn finished(&self) -> bool {
        self.done.load(Ordering::Acquire) >= self.world.bodies.len()
    }
}

impl Drop for Meshing {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Start meshing the session's world off the UI thread (an opened design), framing the view from
/// the bodies' outlines; `remember`: then draw the Home picture of that file.
pub(crate) fn start(app: &mut SolveApp, remember: Option<String>) {
    app.open_meshing_next = false;
    let mut meshing = Meshing::start(crate::open_meshing::session_world(&app.session), Caps::of(app));
    meshing.remember = remember;
    app.open_meshing = Some(meshing);
    app.fit_view();
    if let Some(m) = app.open_meshing.as_mut() {
        m.framed = Some(app.cam);
    }
}

/// Take the camera as the one the open framed, after a fit the app itself resolved (the view is
/// framed again as the design goes in, once the layout is known): `tick` frames the meshes only
/// while the camera is still the one it framed, which is how the user taking the view over keeps
/// their own framing.
pub(crate) fn note_framed(app: &mut SolveApp) {
    if let Some(m) = app.open_meshing.as_mut().filter(|m| !m.complete && m.framed.is_some()) {
        m.framed = Some(app.cam);
    }
}

/// How far the meshing is: bodies done, and of how many.
pub(crate) fn progress(app: &SolveApp) -> Option<(usize, usize)> {
    app.open_meshing.as_ref().filter(|m| !m.complete).map(|m| (m.done.load(Ordering::Acquire).min(m.world.bodies.len()), m.world.bodies.len()))
}

/// A small "Loading parts n/N" note at the foot of the viewport while bodies are being meshed:
/// a part not drawn yet can't be picked yet either, and this says why.
pub(crate) fn show(app: &SolveApp, ctx: &egui::Context) {
    // The operations panel shows it instead.
    if app.preferences.operations_panel {
        return;
    }
    let (Some((done, total)), Some(view)) = (progress(app), app.viewport.rect) else { return };
    let t = solidcraft_theme::Tokens::get();
    let at = egui::pos2(view.left() + 12.0, view.bottom() - 12.0);
    Container::new(ctx, Surface::Toast)
        .area(egui::Id::unique("open_meshing"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(at)
        .interactable(false)
        .show(ctx, |ui| {
            let frame = Container::popup_frame(ui.style()).fill(t.panel);
            Container::show_backing(ui, frame, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new(format!("Loading parts {done}/{total}")).color(t.text));
                });
            });
        });
}

/// Whether bodies are still being meshed after an open.
pub(crate) fn active(app: &SolveApp) -> bool {
    app.open_meshing.as_ref().is_some_and(|m| !m.complete)
}

/// Whether the view is framed from outlines: bodies being meshed, or about to be.
pub(crate) fn framing(app: &SolveApp) -> bool {
    active(app) || app.open_meshing_next
}

/// Per frame: follow the session's world, show more as it is meshed, and frame the finished
/// picture.
pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(m) = app.open_meshing.as_ref() else { return };
    let world = crate::open_meshing::session_world(&app.session);
    let caps = Caps::of(app);
    let changed = !Arc::ptr_eq(&world, &m.world);
    if changed && m.complete {
        // The design changed after its open: the viewport draws it as ever.
        app.open_meshing = None;
        return;
    }
    if changed || (m.caps != caps && !m.complete) {
        // The design changed (an edit, an undo), or the viewport did (its GPU pipelines ready
        // after the open started): mesh the new world for it; what is made is kept.
        let (framed, remember) = (m.framed, m.remember.clone());
        let mut next = Meshing::start(world.clone(), caps);
        (next.framed, next.remember) = (framed, remember);
        app.open_meshing = Some(next);
    }
    let Some(m) = app.open_meshing.as_ref().filter(|m| !m.complete) else { return };
    if m.finished() && m.shown.as_ref().is_some_and(|(n, _)| *n >= m.world.bodies.len()) {
        let (framed, remember, began) = (m.framed, m.remember.clone(), m.began);
        if let Some(m) = app.open_meshing.as_mut() {
            m.complete = true;
        }
        crate::operations::ended(app, "meshing", crate::operations::Outcome::Done);
        if profiling() {
            let ms = began.map_or(0.0, |t| (ctx.input(|i| i.time) - t) * 1000.0);
            solidcraft_engine::note!("SolidCraft open meshing: all {} bodies shown after {ms:.0} ms", world.bodies.len());
        }
        // Framed from outlines and not moved since: frame the meshes.
        if framed == Some(app.cam) {
            app.fit_view();
        }
        crate::frame_watch::step(app, "frame the meshes");
        if let Some(key) = remember {
            crate::home::remember_source(&mut app.home, &app.session, &key);
        }
        crate::frame_watch::step(app, "home picture");
        return;
    }
    ctx.request_repaint();
}

/// The session's placed model, as every panel, pick and hover reads it. A model that cannot be
/// placed shows no placed bodies, and the viewport says why ([`placement_error`]); the session
/// keeps the failure, so it is not placed again on every read.
pub(crate) fn session_world(s: &solidcraft_engine::Session) -> Arc<ModelState> {
    s.world_state().unwrap_or_default()
}

/// Why the session's model cannot be placed, if it cannot.
pub(crate) fn placement_error(s: &solidcraft_engine::Session) -> Option<String> {
    s.world_state().err().map(|e| e.to_string())
}

/// The world the viewport draws: while an opened design is being meshed, the bodies meshed so
/// far (the picture grows a few times a second), then all of them, in the order they came, until
/// the design changes; otherwise the session's ([`session_world`]).
pub(crate) fn shown_world(app: &SolveApp) -> Arc<ModelState> {
    let world = session_world(&app.session);
    let Some(m) = app.open_meshing.as_ref() else { return world };
    if !Arc::ptr_eq(&world, &m.world) {
        return world;
    }
    m.shown.as_ref().map(|(_, st)| st.clone()).unwrap_or_else(|| Arc::new(ModelState::default()))
}

/// Remake the picture so far, at most every [`REFRESH`] seconds and no sooner than
/// [`REFRESH_SHARE`] times the last scene build (the bodies shown before keep their place, so the
/// viewport builds only the new ones).
pub(crate) fn refresh(app: &mut SolveApp, now: f64) {
    let build_s = app.viewport.build_ms / 1000.0;
    let Some(m) = app.open_meshing.as_mut().filter(|m| !m.complete) else { return };
    let order: Vec<usize> = match m.ready.lock() {
        Ok(r) => r.clone(),
        Err(_) => return,
    };
    let done = order.len();
    let all = done >= m.world.bodies.len();
    let wait = REFRESH.max(build_s * REFRESH_SHARE);
    if m.shown.as_ref().is_some_and(|(n, _)| *n == done) || (now - m.shown_at < wait && !all) {
        return;
    }
    let began = *m.began.get_or_insert(now);
    // Bodies are only ever added to `order`: the picture before holds its first bodies. Where
    // nothing else holds that picture any more, the new bodies are added to it; otherwise the
    // bodies so far are copied, never the world's every body (copying the world and dropping its
    // bodies again took most of a second over the open of a design of 56 000 bodies).
    let before = m.shown.take().and_then(|(n, st)| Arc::try_unwrap(st).ok().map(|st| (n, st)));
    let st = match before {
        Some((n, mut st)) if n <= done && st.bodies.len() == n => {
            st.bodies.extend(order.get(n..).unwrap_or_default().iter().filter_map(|&i| m.world.bodies.get(i).cloned()));
            st
        }
        _ => m.world.with_bodies(order.iter().filter_map(|&i| m.world.bodies.get(i).cloned()).collect()),
    };
    if !m.first_shown && !st.bodies.is_empty() {
        m.first_shown = true;
        if profiling() {
            solidcraft_engine::note!("SolidCraft open meshing: first {} bodies shown after {:.0} ms", st.bodies.len(), (now - began) * 1000.0);
        }
    }
    m.shown = Some((done, Arc::new(st)));
    m.shown_at = now;
}

/// The parts placed more than once in `world` (by [`ModelBody::placed_definition`]).
pub(crate) fn repeated_parts(world: &ModelState) -> std::collections::HashSet<usize> {
    let mut placements: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for id in world.bodies.iter().filter_map(ModelBody::placed_definition) {
        *placements.entry(id).or_default() += 1;
    }
    placements.into_iter().filter(|(_, n)| *n > 1).map(|(id, _)| id).collect()
}

/// Make what the viewport draws `b` from, so its first frames do not make it on the window's
/// thread: the reference mesh (a placed part's shared one), with its shared vertices for a GPU
/// viewport (`shared`), and the display plan's mesh, which a finer display quality draws (a placed
/// copy's is its definition's, made once for every copy). A body drawn from display plans
/// (`plans`) has its GPU plan made too, unless it is a placement of a part placed more than once
/// (`repeated`): those are drawn as instances of the part's triangles, never from plans of their
/// own, and each such plan is a moved copy of the part's (a large assembly placed a hundred times
/// took tens of gigabytes while it opened). `q` is the display quality the viewport draws at.
pub(crate) fn warm(
    b: &ModelBody,
    plans: bool,
    shared: bool,
    repeated: &std::collections::HashSet<usize>,
    q: solidcraft_engine::doc::display_quality::DisplayQuality,
) {
    use solidcraft_engine::doc::display_quality::DisplayQuality;
    let (mesh, _) = b.mesh_in_place();
    // A part placed more than once has its shared vertices made with its part ([`warm_part`]).
    if shared && b.placed_definition().is_none_or(|id| !repeated.contains(&id)) {
        crate::viewport::warm_shared_tris(&mesh);
    }
    // Its box, which framing the view reads for every body once they are all shown: worked out
    // there, on the window thread, it moved every point of every placement (a second for a
    // hundred copies of a large assembly).
    let _ = b.mesh_bounds();
    if plans && b.placed_definition().is_none_or(|id| !repeated.contains(&id)) {
        let _ = b.gpu_plan_at(q);
    }
    if plans || q != DisplayQuality::Normal {
        let _ = b.plan_mesh_in_place_at(q);
    }
}

/// Make the part `b` places (its mesh as the viewport draws it at display quality `q`) for the
/// scenes that draw it as an instance ([`crate::viewport::warm_part`]).
fn warm_part(b: &ModelBody, parts: &crate::viewport::WarmedParts, edges: bool, q: solidcraft_engine::doc::display_quality::DisplayQuality) {
    use solidcraft_engine::doc::display_quality::DisplayQuality;
    let mesh = if q == DisplayQuality::Normal { b.mesh_in_place().0 } else { b.plan_mesh_in_place_at(q).unwrap_or_else(|| b.mesh_in_place()).0 };
    crate::viewport::warm_part(parts, &mesh, edges);
}

/// The parts the design being opened made ahead, while it is the one shown.
pub(crate) fn warmed_parts(app: &SolveApp) -> Option<crate::viewport::WarmedParts> {
    app.open_meshing.as_ref().filter(|m| Arc::ptr_eq(&m.world, &session_world(&app.session))).map(|m| m.parts.clone())
}

/// Whether the meshes may be made on a thread of their own. The web build cannot start one; a
/// test says no to do the same ([`without_threads`]).
fn threads() -> bool {
    #[cfg(test)]
    if NO_THREADS.with(std::cell::Cell::get) {
        return false;
    }
    true
}

#[cfg(test)]
thread_local! {
    static NO_THREADS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Show only the bodies `shown` of the world being opened (by their index in it), as the picture
/// made while the others are still being meshed.
#[cfg(test)]
pub(crate) fn show_only(app: &mut SolveApp, shown: &[usize]) {
    let Some(m) = app.open_meshing.as_mut() else { return };
    let st = m.world.with_bodies(shown.iter().filter_map(|&i| m.world.bodies.get(i).cloned()).collect());
    m.shown = Some((shown.len(), Arc::new(st)));
}

/// Mark the bodies `ready` of the world being opened (by their index in it, in that order) as
/// the ones meshed so far, as the meshing thread does.
#[cfg(test)]
pub(crate) fn set_ready(app: &mut SolveApp, ready: &[usize]) {
    let Some(m) = app.open_meshing.as_mut() else { return };
    if let Ok(mut r) = m.ready.lock() {
        *r = ready.to_vec();
    }
}

/// Run `f` as the web build runs: meshing cannot start a thread.
#[cfg(test)]
pub(crate) fn without_threads<R>(f: impl FnOnce() -> R) -> R {
    NO_THREADS.with(|t| t.set(true));
    let r = f();
    NO_THREADS.with(|t| t.set(false));
    r
}

fn profiling() -> bool {
    solidcraft_engine::geom::env::var_os("PROFILE_OPEN").is_some()
}
