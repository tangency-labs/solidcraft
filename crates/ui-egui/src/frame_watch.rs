//! How long the window thread spends on each frame, by what the app is doing (#3053): the window
//! must stay responsive through every phase of opening a design. `SOLIDCRAFT_PROFILE_FRAMES=1`
//! logs every frame slower than [`SLOW_MS`] with its phase; tests read [`Watch::slowest`].

use crate::SolveApp;

/// A frame slower than this is logged (ms).
pub(crate) const SLOW_MS: f64 = 50.0;

#[derive(Default)]
pub(crate) struct Watch {
    began: Option<f64>,
    phase: &'static str,
    /// This frame's named steps so far, and when the last one ended (ms).
    steps: Vec<(&'static str, f64)>,
    last: f64,
    /// The slowest frame so far: ms, and the phase it was in.
    pub slowest: (f64, &'static str),
}

#[cfg(test)]
impl Watch {
    /// The last frame's named steps and their milliseconds (the scaling measurements read them).
    pub(crate) fn steps(&self) -> &[(&'static str, f64)] {
        &self.steps
    }
}

/// What the app is doing this frame, as far as opening a design goes.
pub(crate) fn phase(app: &SolveApp) -> &'static str {
    if let Some(p) = crate::file_load::phase(app) {
        return p;
    }
    if crate::open_meshing::active(app) { "loading parts" } else { "idle" }
}

/// The frame starts (before any of its work).
pub(crate) fn begin(app: &mut SolveApp) {
    let now = crate::now_ms();
    app.frame_watch.began = Some(now);
    app.frame_watch.last = now;
    app.frame_watch.steps.clear();
    app.frame_watch.phase = phase(app);
}

/// A named step of this frame's work ended: a slow frame is logged with them.
pub(crate) fn step(app: &mut SolveApp, name: &'static str) {
    let now = crate::now_ms();
    let w = &mut app.frame_watch;
    if w.began.is_some() && w.steps.len() < 32 {
        w.steps.push((name, now - w.last));
    }
    w.last = now;
}

/// The frame's work is done.
pub(crate) fn end(app: &mut SolveApp) {
    step(app, "the rest of the frame");
    let Some(began) = app.frame_watch.began.take() else { return };
    let ms = crate::now_ms() - began;
    // The phase the frame started in names it (a frame that finishes an open did that work).
    let phase = app.frame_watch.phase;
    if ms > app.frame_watch.slowest.0 {
        app.frame_watch.slowest = (ms, phase);
    }
    if ms > SLOW_MS && solidcraft_engine::geom::env::var_os("PROFILE_FRAMES").is_some() {
        let steps: Vec<String> = app.frame_watch.steps.iter().filter(|(_, t)| *t >= 1.0).map(|(n, t)| format!("{n} {t:.0}")).collect();
        solidcraft_engine::note!("SolidCraft slow frame: {ms:.0} ms ({phase}) [{}]", steps.join(", "));
    }
}
