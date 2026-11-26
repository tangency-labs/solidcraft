//! Exact movable corridors: the steps the selected feature can really move to.
//!
//! The dependency graph gives a corridor from what each feature names, but a move can also break
//! a feature through geometry alone: a sketch that projects a face a later cut puts a hole in
//! loses the hole's edges when it moves before the cut. So the steps of that corridor are tried
//! beside the window, each on a copy of the design with the `timeline.reorder` a drop runs, from
//! the feature outward until a move fails. The result is kept until the design changes, since a
//! change to any feature can change what a move breaks.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

/// How long a bracket takes to move to its checked place, in seconds.
const SETTLE: f64 = 0.18;
/// How fast the running light crosses the corridor, in steps per second.
const LIGHT_SPEED: f32 = 14.0;
/// How many steps the running light's fading tail is long.
const LIGHT_TAIL: f32 = 3.0;

/// One side of a corridor: the earlier steps or the later ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Low,
    High,
}

/// The farthest step a move still works at, trying `steps` in order (outward from the feature at
/// `current`) and stopping at the first that fails. `None` when `try_move` gives up (cancelled).
pub(crate) fn walk(current: usize, steps: impl Iterator<Item = usize>, mut try_move: impl FnMut(usize) -> Option<bool>) -> Option<usize> {
    let mut farthest = current;
    for step in steps {
        if !try_move(step)? {
            break;
        }
        farthest = step;
    }
    Some(farthest)
}

/// The checks under way for one feature.
struct Running {
    feature: u64,
    /// The dependency graph's corridor, in timeline steps: the checks only narrow it.
    known: (usize, usize),
    current: usize,
    started: f64,
    /// Each side's checked bound and when it came in.
    low: Option<(usize, f64)>,
    high: Option<(usize, f64)>,
    cancel: Arc<AtomicBool>,
    found: mpsc::Receiver<(Side, Option<usize>)>,
}

/// What to show for the selected feature's corridor this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shown {
    /// Where the brackets are, in timeline steps (fractional while they move).
    pub low: f32,
    pub high: f32,
    /// While a side is still checked: how far the running light has gone, in steps from that
    /// side's end of the known corridor towards the feature.
    pub light_low: Option<f32>,
    pub light_high: Option<f32>,
    /// The known corridor's ends and the feature's own step, for the running light.
    pub known: (usize, usize),
    pub current: usize,
}

impl Shown {
    /// How lit the alignment dot at step `step` is, 0 to 1.
    pub(crate) fn light_at(&self, step: usize) -> f32 {
        let lit = |head: f32, from_end: usize| {
            let behind = head - from_end as f32;
            if behind < 0.0 { 0.0 } else { (1.0 - behind / LIGHT_TAIL).max(0.0) }
        };
        // The feature's own step ends both runs: its dots in the other rows light up too.
        let low = (self.known.0..=self.current).contains(&step).then_some(()).and(self.light_low).map_or(0.0, |head| lit(head, step - self.known.0));
        let high =
            (self.current..=self.known.1).contains(&step).then_some(()).and(self.light_high).map_or(0.0, |head| lit(head, self.known.1 - step));
        low.max(high)
    }
}

/// The exact corridors of one design, and the checks under way.
#[derive(Default)]
pub(crate) struct Checks {
    /// The design the results belong to: a hash of its features and marker.
    key: u64,
    /// The design revision `key` was taken at, so the hash is only taken again after a change.
    revision: Option<u64>,
    done: HashMap<u64, (usize, usize)>,
    running: Option<Running>,
}

impl Checks {
    /// The checks of the design open in `session`, kept in the window's memory.
    pub(crate) fn of(ctx: &egui::Context, session: &solidcraft_engine::Session) -> Arc<std::sync::Mutex<Checks>> {
        let id = egui::Id::unique("timeline_2d_corridor_checks").with(session.uid);
        ctx.data_mut(|d| d.get_temp_mut_or_insert_with(id, || Arc::new(std::sync::Mutex::new(Checks::default()))).clone())
    }

    /// Follow the design and the selected `feature` (with the graph's corridor `known` around its
    /// step `current`): start checking it unless it is checked already; drop checks for anything
    /// else. Takes in what the checks found.
    pub(crate) fn follow(
        &mut self,
        session: &solidcraft_engine::Session,
        feature: Option<(u64, (usize, usize), usize)>,
        now: f64,
        wake: &egui::Context,
    ) {
        if self.revision != Some(session.revision) {
            self.revision = Some(session.revision);
            let key = design_key(&session.doc);
            if key != self.key {
                self.key = key;
                self.done.clear();
                self.stop();
            }
        }
        self.take_found(now);
        let Some((id, known, current)) = feature else {
            self.stop();
            return;
        };
        if self.running.as_ref().is_some_and(|r| r.feature == id) {
            return;
        }
        self.stop();
        if self.done.contains_key(&id) {
            return;
        }
        if known == (current, current) {
            self.done.insert(id, known);
            return;
        }
        self.start(session, id, known, current, now, wake);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start(&mut self, session: &solidcraft_engine::Session, id: u64, known: (usize, usize), current: usize, now: f64, wake: &egui::Context) {
        let cancel = Arc::new(AtomicBool::new(false));
        let (send, found) = mpsc::channel();
        for side in [Side::Low, Side::High] {
            let base = session.scratch();
            let (stopped, send, wake) = (cancel.clone(), send.clone(), wake.clone());
            let job = std::thread::Builder::new().name("corridor check".into()).spawn(move || {
                let try_move = |step: usize| {
                    if stopped.load(Ordering::Relaxed) {
                        return None;
                    }
                    let mut copy = base.scratch();
                    match copy.execute("timeline.reorder", &serde_json::json!({"feature": id, "position": step})) {
                        Ok(_) => Some(true),
                        // The design is going away: no answer, rather than a wrong one.
                        Err(solidcraft_engine::EngineError::Cancelled(_)) => None,
                        Err(_) => Some(false),
                    }
                };
                let bound = match side {
                    Side::Low => walk(current, (known.0..current).rev(), try_move),
                    Side::High => walk(current, current + 1..=known.1, try_move),
                };
                let _ = send.send((side, bound));
                wake.request_repaint();
            });
            if job.is_err() {
                // No thread to check on: the graph's corridor stands, and the engine still checks the drop.
                cancel.store(true, Ordering::Relaxed);
                return;
            }
        }
        self.running = Some(Running { feature: id, known, current, started: now, low: None, high: None, cancel, found });
    }

    /// In the browser there is no thread to check on: the graph's corridor stands, and the engine
    /// still checks the drop.
    #[cfg(target_arch = "wasm32")]
    fn start(&mut self, _: &solidcraft_engine::Session, _: u64, _: (usize, usize), _: usize, _: f64, _: &egui::Context) {}

    fn stop(&mut self) {
        if let Some(r) = self.running.take() {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn take_found(&mut self, now: f64) {
        let Some(r) = self.running.as_mut() else { return };
        while let Ok((side, bound)) = r.found.try_recv() {
            let Some(bound) = bound else {
                // Cancelled: nothing known, so nothing kept.
                self.running = None;
                return;
            };
            match side {
                Side::Low => r.low = Some((bound, now)),
                Side::High => r.high = Some((bound, now)),
            }
        }
        if let (Some((low, _)), Some((high, _))) = (r.low, r.high) {
            self.done.insert(r.feature, (low, high));
        }
    }

    /// The checked corridor of `feature` in timeline steps, once both sides are known.
    pub(crate) fn corridor(&self, feature: u64) -> Option<(usize, usize)> {
        self.done.get(&feature).copied()
    }

    /// Whether anything still moves: the window needs another frame.
    pub(crate) fn animating(&self, now: f64) -> bool {
        self.running.as_ref().is_some_and(|r| r.low.is_none_or(|(_, at)| now - at < SETTLE) || r.high.is_none_or(|(_, at)| now - at < SETTLE))
    }

    /// What to show for `feature` at `now`; `None` when it isn't checked or being checked.
    pub(crate) fn shown(&self, feature: u64, now: f64) -> Option<Shown> {
        if let Some(r) = self.running.as_ref().filter(|r| r.feature == feature) {
            let head = ((now - r.started) as f32 * LIGHT_SPEED).max(0.0);
            let side = |end: usize, found: Option<(usize, f64)>, span: usize| match found {
                None => (end as f32, Some(head % (span as f32 + LIGHT_TAIL))),
                Some((bound, at)) => {
                    let k = ((now - at) / SETTLE).clamp(0.0, 1.0) as f32;
                    let eased = 1.0 - (1.0 - k) * (1.0 - k);
                    (end as f32 + (bound as f32 - end as f32) * eased, None)
                }
            };
            let (low, light_low) = side(r.known.0, r.low, r.current - r.known.0);
            let (high, light_high) = side(r.known.1, r.high, r.known.1 - r.current);
            return Some(Shown { low, high, light_low, light_high, known: r.known, current: r.current });
        }
        let (low, high) = self.corridor(feature)?;
        Some(Shown { low: low as f32, high: high as f32, light_low: None, light_high: None, known: (low, high), current: low })
    }
}

/// A hash of what a move's outcome depends on: every feature and the history marker.
fn design_key(doc: &solidcraft_engine::doc::Document) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_vec(&(&doc.features, doc.marker)).unwrap_or_default().hash(&mut h);
    h.finish()
}
