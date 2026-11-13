//! A face press owns its original ray and waits for qualified native ownership.
use super::*;
use crate::navigation::CameraInput;

pub(super) fn enabled(app: &SolveApp) -> bool {
    cfg!(not(target_arch = "wasm32"))
        && (app.preferences.forms_exact_face_picking
            || app.session.forms_prototype.as_ref().and_then(|form| form.hierarchy.as_ref()).is_some_and(|h| h.has_boundary_collapse()))
}

pub(super) fn owns_pointer(app: &SolveApp) -> bool {
    enabled(app)
        && !tool_panel::active(app)
        && (tools::edge_pick(app).is_some() || !tools::active(app) && matches!(app.forms_prototype.pick, Pick::All | Pick::Face))
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) use jobs::State;
pub(super) use jobs::{before_input, cancel, direct, insert, pending, tick};
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) use jobs::{hold_for_test, query_for_test, response_for_test, solve_for_test};

#[cfg(target_arch = "wasm32")]
mod jobs {
    use super::*;

    pub(crate) fn before_input(_: &mut SolveApp, _: &egui::Ui) {}
    pub(crate) fn cancel(_: &mut SolveApp) {}
    pub(crate) fn direct(_: &mut SolveApp, _: &egui::Ui, _: &Proj, _: &CameraInput) -> bool {
        false
    }
    pub(crate) fn insert(_: &mut SolveApp, _: &egui::Ui, _: &Proj, _: &CameraInput) -> bool {
        false
    }
    pub(crate) fn pending(_: &SolveApp) -> bool {
        false
    }
    pub(crate) fn tick(_: &mut SolveApp, _: &egui::Ui, _: &Proj, _: &CameraInput, _: bool) -> bool {
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod jobs {
    use super::*;
    use face_pick::{Answer, CaptureError, Owner, Query};
    use solidcraft_engine::geom::brep::ray::{ClipDepth, FrontError, ProofError};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, TryRecvError};

    type Result = std::result::Result<Answer, FrontError>;

    #[derive(Default)]
    pub(crate) struct State {
        serial: u64,
        gesture: Option<Gesture>,
        worker: Option<Worker>,
        cached_query: Option<Arc<Query>>,
        #[cfg(test)]
        gate: Arc<AtomicBool>,
    }

    struct Worker {
        serial: u64,
        done: Receiver<Result>,
    }

    struct Gesture {
        serial: u64,
        projection: Proj,
        query: Arc<Query>,
        stop: Arc<AtomicBool>,
        press: Sample,
        scripted: bool,
        intent: Intent,
    }

    impl Drop for Gesture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    enum Intent {
        Direct(Box<Direct>),
        Insert(tools::EdgePick),
    }

    struct Direct {
        pick: Pick,
        axis: Axis,
        orientation: usize,
        scale: bool,
        setting_pivot: bool,
        pivot: Option<Vec3>,
        greedy: bool,
        region_enabled: bool,
        soft: (bool, String, usize),
        snap: (bool, String),
        free_alt: bool,
        active: Option<Element>,
        before: Option<BTreeSet<Element>>,
        region: Option<selection::RegionDrag>,
        motion: Motion,
    }

    impl Direct {
        fn new(app: &SolveApp, projection: &Proj, press: Sample) -> Self {
            let s = &app.forms_prototype;
            let region = (!s.setting_pivot).then(|| selection::region_intent(app, projection, press.position, press.mods)).flatten();
            Self {
                pick: s.pick,
                axis: s.axis,
                orientation: s.orientation,
                scale: s.scale,
                setting_pivot: s.setting_pivot,
                pivot: s.pivot,
                greedy: app.preferences.forms_greedy_selection,
                region_enabled: app.preferences.forms_region_selection,
                soft: (s.soft.enabled, s.soft.radius.clone(), s.soft.falloff),
                snap: (s.translation_snap.enabled, s.translation_snap.increment.clone()),
                free_alt: app.preferences.manipulator_free_alt,
                active: selection::active(app),
                before: region.is_none().then(|| s.elements.clone()),
                region,
                motion: Motion { latest: press, released: false, moved: false, shift: press.mods.shift },
            }
        }

        fn current(&self, app: &SolveApp, projection: &Proj) -> bool {
            let s = &app.forms_prototype;
            !tools::active(app)
                && !tool_panel::active(app)
                && self.pick == s.pick
                && self.axis == s.axis
                && self.orientation == s.orientation
                && self.scale == s.scale
                && self.setting_pivot == s.setting_pivot
                && self.pivot == s.pivot
                && self.greedy == app.preferences.forms_greedy_selection
                && self.region_enabled == app.preferences.forms_region_selection
                && (self.soft.0, self.soft.1.as_str(), self.soft.2) == (s.soft.enabled, s.soft.radius.as_str(), s.soft.falloff)
                && (self.snap.0, self.snap.1.as_str()) == (s.translation_snap.enabled, s.translation_snap.increment.as_str())
                && self.free_alt == app.preferences.manipulator_free_alt
                && self.active == selection::active(app)
                && self.before.as_ref().is_none_or(|before| *before == s.elements)
                && self.region.as_ref().is_none_or(|drag| drag.current(app, projection))
        }

        fn record(&mut self, ui: &egui::Ui, input: &CameraInput, press: Sample) -> bool {
            if self.motion.released {
                return true;
            }
            let sample = if input.primary_down {
                input.hover.map(|position| Sample { position, mods: input.mods })
            } else {
                let Some(release) = Sample::button(ui, false) else { return false };
                self.motion.released = true;
                Some(release)
            };
            if let Some(sample) = sample.filter(|sample| sample.position.is_finite()) {
                if input.primary_down && !self.motion.moved {
                    self.motion.shift |= sample.mods.shift;
                }
                self.motion.moved |= sample.position.distance(press.position) >= 2.0;
                self.motion.latest = sample;
                if let Some(region) = &mut self.region {
                    region.extend(sample.position);
                }
            }
            true
        }
    }

    struct Motion {
        latest: Sample,
        released: bool,
        moved: bool,
        shift: bool,
    }

    #[derive(Clone, Copy)]
    struct Sample {
        position: Pos2,
        mods: egui::Modifiers,
    }

    impl Sample {
        fn button(ui: &egui::Ui, want_pressed: bool) -> Option<Self> {
            ui.input(|i| {
                i.events.iter().rev().find_map(|event| match *event {
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers } if pressed == want_pressed => {
                        Some(Self { position: pos, mods: modifiers })
                    }
                    _ => None,
                })
            })
        }

        fn press(app: &SolveApp, ui: &egui::Ui, projection: &Proj, input: &CameraInput) -> Option<Self> {
            Self::button(ui, true).or_else(|| input.hover.map(|position| Self { position, mods: input.mods })).filter(|sample| {
                sample.position.is_finite()
                    && projection.rect.contains(sample.position)
                    && !input.axes.contains(sample.position)
                    && !crate::shell::covered(app, sample.position)
                    && ui.ctx().layer_id_at(sample.position).is_none_or(|layer| layer == ui.layer_id())
            })
        }
    }

    pub(crate) fn cancel(app: &mut SolveApp) {
        app.forms_prototype.face_press.gesture = None;
    }

    pub(crate) fn before_input(app: &mut SolveApp, ui: &egui::Ui) {
        if !enabled(app) {
            app.forms_prototype.face_press.cached_query = None;
        }
        if ui.input(|i| {
            i.pointer.primary_pressed()
                || i.pointer.secondary_pressed()
                || i.events.iter().any(|event| matches!(event, egui::Event::Key { pressed: true, .. }))
        }) {
            cancel(app);
        }
    }

    pub(crate) fn pending(app: &SolveApp) -> bool {
        app.forms_prototype.face_press.gesture.is_some()
    }

    pub(crate) fn direct(app: &mut SolveApp, ui: &egui::Ui, projection: &Proj, input: &CameraInput) -> bool {
        let pick = app.forms_prototype.pick;
        if !enabled(app) || !matches!(pick, Pick::All | Pick::Face) {
            return false;
        }
        let Some(press) = Sample::press(app, ui, projection, input) else { return true };
        if pick == Pick::All
            && let Some(hit) = surface::nearest_with(app, projection, press.position, Pick::Vertex)
                .or_else(|| surface::nearest_with(app, projection, press.position, Pick::Edge))
        {
            begin_drag_hit(app, projection, press.position, press.mods, hit);
            return true;
        }
        selection::cancel_region_work(app);
        start(app, ui.ctx(), *projection, press, |app| Intent::Direct(Box::new(Direct::new(app, projection, press))));
        true
    }

    pub(crate) fn insert(app: &mut SolveApp, ui: &egui::Ui, projection: &Proj, input: &CameraInput) -> bool {
        if !enabled(app) {
            return false;
        }
        if let (Some(press), Some(key)) = (Sample::press(app, ui, projection, input), tools::edge_pick(app)) {
            start(app, ui.ctx(), *projection, press, |_| Intent::Insert(key));
        }
        true
    }

    fn start(app: &mut SolveApp, ctx: &egui::Context, projection: Proj, press: Sample, intent: impl FnOnce(&SolveApp) -> Intent) {
        let query = match query_for_press(app) {
            Ok(query) => query,
            Err(error) => {
                let key = match error {
                    CaptureError::FormNotReady | CaptureError::ContextNotReady => "forms.face_pick.not_ready",
                    CaptureError::ContextWithoutCad => "forms.face_pick.context_missing_cad",
                    CaptureError::GeometryBudget | CaptureError::Allocation => "forms.face_pick.limit",
                };
                status(app, key);
                return;
            }
        };
        let intent = intent(app);
        let state = &mut app.forms_prototype.face_press;
        state.serial = state.serial.wrapping_add(1);
        state.gesture =
            Some(Gesture { serial: state.serial, projection, query, stop: Default::default(), press, scripted: app.synthetic_primary_press, intent });
        launch(app, ctx);
    }

    fn query_for_press(app: &mut SolveApp) -> std::result::Result<Arc<Query>, CaptureError> {
        let cached = app.forms_prototype.face_press.cached_query.take();
        let query = match cached.filter(|query| query.is_current(app)) {
            Some(query) => query,
            None => Arc::new(Query::capture(app)?),
        };
        // The worker owns its Query. A late preparation for a replaced source
        // can only populate that old Query, never the editor's replacement.
        app.forms_prototype.face_press.cached_query = Some(query.clone());
        Ok(query)
    }

    #[cfg(not(test))]
    fn worker_gate() -> Arc<AtomicBool> {
        static ACTIVE: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();
        ACTIVE.get_or_init(Default::default).clone()
    }

    fn launch(app: &mut SolveApp, ctx: &egui::Context) {
        let state = &mut app.forms_prototype.face_press;
        let Some(gesture) = state.gesture.as_ref().filter(|_| state.worker.is_none()) else { return };
        #[cfg(test)]
        let gate = state.gate.clone();
        #[cfg(not(test))]
        let gate = worker_gate();
        let Some(permit) = finish::WorkerPermit::acquire(gate) else { return };
        let (sender, done) = mpsc::channel();
        let (query, projection, pos, serial) = (gesture.query.clone(), gesture.projection, gesture.press.position, gesture.serial);
        let stop = gesture.stop.clone();
        let wake = ctx.clone();
        match std::thread::Builder::new().name("form-face-pick".into()).spawn(move || {
            let _permit = permit;
            let result = solve(&query, &projection, pos, &stop);
            let _ = sender.send(result);
            wake.request_repaint();
        }) {
            Ok(_) => state.worker = Some(Worker { serial, done }),
            Err(_) => {
                cancel(app);
                status(app, "forms.face_pick.worker_stopped");
            }
        }
    }

    fn solve(query: &Query, projection: &Proj, pos: Pos2, stop: &AtomicBool) -> Result {
        if stop.load(Ordering::Relaxed) {
            return Ok(Answer::Unresolved);
        }
        let (origin, direction) = projection.ray(pos);
        let xyz = |p: Vec3| [p.x, p.y, p.z];
        let Some(clip) = ClipDepth::from_projection(projection.vp.0, xyz(origin), xyz(direction)) else { return Ok(Answer::Unresolved) };
        let prepared = query.prepare()?;
        if stop.load(Ordering::Relaxed) {
            return Ok(Answer::Unresolved);
        }
        let answer = prepared.front(origin, direction, clip.range())?;
        Ok(match answer {
            Answer::Hit { depth, .. } | Answer::Foreign { depth } | Answer::Occluded { depth } if !clip.contains(depth) => Answer::Unresolved,
            other => other,
        })
    }

    fn current(app: &SolveApp, ui: &egui::Ui, projection: &Proj, input: &CameraInput, blocked: bool, gesture: &Gesture) -> bool {
        enabled(app)
            && !blocked
            && input.mode.is_none()
            && (gesture.scripted || ui.input(|i| i.focused))
            && gesture.projection.cam == projection.cam
            && gesture.projection.vp == projection.vp
            && gesture.projection.rect == projection.rect
            && app.viewport.rect == Some(projection.rect)
            && gesture.query.is_current(app)
            && match &gesture.intent {
                Intent::Direct(direct) => direct.current(app, projection),
                Intent::Insert(key) => tools::edge_pick(app) == Some(*key) && !tool_panel::active(app),
            }
    }

    pub(crate) fn tick(app: &mut SolveApp, ui: &egui::Ui, projection: &Proj, input: &CameraInput, blocked: bool) -> bool {
        if app.forms_prototype.face_press.gesture.as_ref().is_some_and(|gesture| !current(app, ui, projection, input, blocked, gesture)) {
            cancel(app);
        }
        if let Some(gesture) = &mut app.forms_prototype.face_press.gesture
            && let Intent::Direct(direct) = &mut gesture.intent
            && !direct.record(ui, input, gesture.press)
        {
            cancel(app);
        }
        let result = app.forms_prototype.face_press.worker.as_ref().and_then(|worker| match worker.done.try_recv() {
            Ok(answer) => Some((worker.serial, Ok(answer))),
            Err(TryRecvError::Disconnected) => Some((worker.serial, Err(()))),
            Err(TryRecvError::Empty) => None,
        });
        if let Some((serial, answer)) = result {
            let state = &mut app.forms_prototype.face_press;
            state.worker = None;
            if state.gesture.as_ref().is_some_and(|gesture| gesture.serial == serial)
                && let Some(mut gesture) = state.gesture.take()
            {
                return apply(app, ui.ctx(), projection, &mut gesture, answer);
            }
        }
        launch(app, ui.ctx());
        if pending(app) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
            if input.inside {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Progress);
            }
        }
        false
    }

    fn apply(app: &mut SolveApp, ctx: &egui::Context, projection: &Proj, gesture: &mut Gesture, answer: std::result::Result<Result, ()>) -> bool {
        let answer = match answer {
            Ok(Ok(answer)) => answer,
            Ok(Err(FrontError::GeometryBudget | FrontError::Proof(ProofError::WorkExhausted | ProofError::Allocation))) => {
                status(app, "forms.face_pick.limit");
                return false;
            }
            Ok(Err(_)) => Answer::Unresolved,
            Err(()) => {
                status(app, "forms.face_pick.worker_stopped");
                return false;
            }
        };
        if answer == Answer::Unresolved {
            status(app, "forms.face_pick.unresolved");
            return false;
        }
        clear_pick_error(app);
        let press = gesture.press;
        match (&mut gesture.intent, answer) {
            (Intent::Direct(direct), Answer::Hit { owner, .. }) => {
                let hit = match owner {
                    Owner::Face(id) => Element::Face(id),
                    Owner::Surface => Element::Surface,
                };
                begin_drag_hit(app, projection, press.position, press.mods, hit);
                enable_shift_extrusion(app, direct.motion.shift);
                pull_with_modifiers(app, direct.motion.latest.position, direct.motion.latest.mods);
                true
            }
            (Intent::Direct(direct), Answer::Miss) => {
                if let Some(region) = direct.region.take() {
                    transform_feedback::clear(app);
                    selection::accept_region_intent(app, ctx, projection, region, !direct.motion.released);
                } else {
                    clear_empty_press(app, press.mods);
                }
                false
            }
            (Intent::Insert(key), Answer::Hit { owner: Owner::Face(face), .. }) => {
                tools::commit_face(app, ctx, face, *key);
                false
            }
            (Intent::Insert(_), Answer::Hit { owner: Owner::Surface, .. }) => {
                status(app, "forms.face_pick.insert_face_required");
                false
            }
            _ => false,
        }
    }

    fn status(app: &mut SolveApp, key: &'static str) {
        app.set_status(app.tr(key).to_owned(), true);
    }

    fn clear_pick_error(app: &mut SolveApp) {
        let Some((text, _, true)) = app.status.as_ref() else { return };
        let pick_errors = [
            "forms.face_pick.context_missing_cad",
            "forms.face_pick.insert_face_required",
            "forms.face_pick.limit",
            "forms.face_pick.not_ready",
            "forms.face_pick.unresolved",
            "forms.face_pick.worker_stopped",
        ];
        if pick_errors.iter().any(|key| app.tr(key) == text) {
            app.status = None;
        }
    }

    #[cfg(test)]
    pub(in crate::forms_prototype) fn hold_for_test(app: &SolveApp) -> Option<finish::WorkerPermit> {
        finish::WorkerPermit::acquire(app.forms_prototype.face_press.gate.clone())
    }

    #[cfg(test)]
    pub(crate) fn query_for_test(app: &mut SolveApp) -> std::result::Result<Arc<Query>, CaptureError> {
        query_for_press(app)
    }

    #[cfg(test)]
    pub(crate) fn response_for_test(app: &mut SolveApp) -> Option<mpsc::Sender<Result>> {
        let state = &mut app.forms_prototype.face_press;
        let serial = state.gesture.as_ref()?.serial;
        if state.worker.is_some() {
            return None;
        }
        let (sender, done) = mpsc::channel();
        state.worker = Some(Worker { serial, done });
        Some(sender)
    }

    #[cfg(test)]
    pub(crate) fn solve_for_test(app: &SolveApp) -> Option<Result> {
        let gesture = app.forms_prototype.face_press.gesture.as_ref()?;
        Some(solve(&gesture.query, &gesture.projection, gesture.press.position, &gesture.stop))
    }
}
