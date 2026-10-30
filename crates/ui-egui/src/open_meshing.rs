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

use solidcraft_engine::doc::ModelState;

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
}

impl Meshing {
    /// `plans`: the bodies are drawn from their display plans; `shared`: large meshes are drawn
    /// from shared vertices (a GPU viewport), made here too.
    fn start(world: Arc<ModelState>, plans: bool, shared: bool) -> Meshing {
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
        let job = (world.clone(), ready.clone(), done.clone(), stop.clone());
        let spawned = threads()
            && std::thread::Builder::new()
                .name("open-meshing".into())
                .spawn(move || {
                    let (world, ready, done, stop) = job;
                    solidcraft_engine::kernel::for_each_on_threads(&first, 16, |&i| {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        if let Some(b) = world.bodies.get(i) {
                            // What the viewport draws and picks: a placed part's shared mesh, not a
                            // moved copy of it (its placements are drawn as instances of it).
                            let (mesh, _) = b.mesh_in_place();
                            // A large mesh's shared vertices too, so its first frame does not weld them.
                            if shared {
                                crate::viewport::warm_shared_tris(&mesh);
                            }
                            if plans {
                                let _ = b.display_plan();
                                let _ = b.plan_mesh_in_place_at(solidcraft_engine::doc::display_quality::current());
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
    let plans = app.viewport.gpu.is_some_and(|g| g.tess);
    let mut meshing = Meshing::start(app.session.world_state(), plans, app.viewport.gpu.is_some());
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
    let t = crate::theme::Tokens::get();
    let at = egui::pos2(view.left() + 12.0, view.bottom() - 12.0);
    Container::new(ctx, Surface::Toast)
        .area(egui::Id::new("open_meshing"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(at)
        .interactable(false)
        .show(ctx, |ui| {
            Container::popup_frame(ui.style()).fill(t.panel).show(ui, |ui| {
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
    let world = app.session.world_state();
    if !Arc::ptr_eq(&world, &m.world) {
        if m.complete {
            // The design changed after its open: the viewport draws it as ever.
            app.open_meshing = None;
            return;
        }
        // The design changed (an edit, an undo): mesh the new world; what is made is kept.
        let (framed, remember) = (m.framed, m.remember.clone());
        let plans = app.viewport.gpu.is_some_and(|g| g.tess);
        let mut next = Meshing::start(world.clone(), plans, app.viewport.gpu.is_some());
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
            eprintln!("SolidCraft open meshing: all {} bodies shown after {ms:.0} ms", world.bodies.len());
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

/// The world the viewport draws: while an opened design is being meshed, the bodies meshed so
/// far (the picture grows a few times a second), then all of them, in the order they came, until
/// the design changes; otherwise the session's.
pub(crate) fn shown_world(app: &SolveApp) -> Arc<ModelState> {
    let Some(m) = app.open_meshing.as_ref() else { return app.session.world_state() };
    if !Arc::ptr_eq(&app.session.world_state(), &m.world) {
        return app.session.world_state();
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
    let mut st = (*m.world).clone();
    st.bodies = order.iter().filter_map(|&i| m.world.bodies.get(i).cloned()).collect();
    if !m.first_shown && !st.bodies.is_empty() {
        m.first_shown = true;
        if profiling() {
            eprintln!("SolidCraft open meshing: first {} bodies shown after {:.0} ms", st.bodies.len(), (now - began) * 1000.0);
        }
    }
    m.shown = Some((done, Arc::new(st)));
    m.shown_at = now;
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
