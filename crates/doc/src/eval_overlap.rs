//! SMLib's check of a feature's bodies beside the features after it.
//!
//! Under [`kernel::Validation::EachFeature`] a feature ends with SMLib's full check of the bodies
//! it keeps, and the next feature used to wait for it: a third to a half of an evaluation went to
//! those checks, on one core. [`Checks`] detaches each check from the bodies it reads
//! ([`kernel::detach_kept`]) and runs it on a thread of its own while the evaluation goes on. What the features after it produce stands only once it passes: their
//! cache entries are held until then, and a refusal takes the evaluation back to the refused
//! feature, which then fails with the check's error and warnings, as it did when the check ran
//! in line. The results are the same either way; only the time differs.
//!
//! A window's evaluation does not wait for the checks still running when its last feature is
//! done ([`Pending`], `Model::set_background_checks`): the results show at once, and the checks
//! end beside the window. Their cache entries are held until then, a save keeps no model while
//! they run, and the next evaluation waits for them first. A refusal marks the refused feature and
//! every result after it stale and is carried into the next evaluation ([`Carried`]), which fails
//! that feature, on the same inputs, with the check's error and warnings, as the evaluation that
//! waited would have.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use solidcraft_kernel as kernel;

use super::{CacheEntry, EvalCache, FeatureResult, KeptSlot, ModelBody, ModelState};

/// The evaluation's loop state just before a feature was evaluated: what a refusal of its check
/// puts back before evaluating it again.
pub(super) struct Saved {
    pub state: Arc<ModelState>,
    pub reserved: std::collections::BTreeSet<String>,
    pub out_len: usize,
    pub reuse: bool,
    pub prev_fp: u64,
    /// The features' own inputs so far, without where they are placed (#17310).
    pub geo: u64,
    pub recomputed: usize,
    pub restored: usize,
    pub poses: HashMap<u64, u64>,
    pub moved: Vec<(u64, Vec<ModelBody>)>,
    pub failed: HashMap<u64, (String, String)>,
    pub rolled_results: HashMap<u64, (u64, FeatureResult)>,
}

/// How a feature whose check refused its bodies ends: as the check in line ended it.
#[derive(Clone)]
pub(super) struct Failure {
    pub error: kernel::KernelError,
    pub warning: Option<String>,
    pub import_report: Option<Arc<kernel::ImportReport>>,
    pub repair: Option<Arc<kernel::RepairReport>>,
    pub ms: f64,
}

/// A check that refused feature `index`'s bodies.
pub(super) struct Refused {
    pub index: usize,
    /// The refused feature and its cache key: the inputs its check refused.
    pub id: u64,
    pub key: u64,
    pub saved: Saved,
    pub failure: Failure,
}

struct Running {
    index: usize,
    id: u64,
    key: u64,
    saved: Saved,
    failure: Failure,
    done: std::thread::JoinHandle<kernel::Result<()>>,
}

pub(super) struct Checks {
    on: bool,
    /// Most checks running at once: the cores beside the evaluation's own.
    most: usize,
    running: VecDeque<Running>,
    /// Cache entries of features evaluated after a check that is still running, in order.
    held: Vec<(usize, CacheEntry)>,
    /// Features whose check refused their bodies, to fail again as it did.
    refused: HashMap<usize, Failure>,
}

impl Checks {
    pub fn new(validation: kernel::Validation) -> Checks {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        Checks {
            on: validation == kernel::Validation::EachFeature && cores > 1,
            most: cores.saturating_sub(1).max(1),
            running: VecDeque::new(),
            held: Vec::new(),
            refused: HashMap::new(),
        }
    }

    /// The check of the bodies `after` adds to `before`, detached from them so it can run beside
    /// the features after it ([`kernel::detach_kept`]); `None` when it runs in line.
    pub fn detach(&self, before: &ModelState, after: &ModelState) -> Option<Vec<(String, kernel::KeptCheck)>> {
        self.on.then(|| {
            after
                .bodies
                .iter()
                .filter(|b| !before.bodies.iter().any(|old| old.body.same_shape(&b.body)))
                .filter(|b| {
                    let reused = super::checked_thread_output(&b.body);
                    #[cfg(test)]
                    if reused {
                        super::cost_tests::count(|c| c.reused_checks += 1);
                    }
                    !reused
                })
                .map(|b| {
                    #[cfg(test)]
                    super::cost_tests::count(|c| c.detached_checks += 1);
                    (b.name.clone(), kernel::detach_kept(&b.body))
                })
                .collect()
        })
    }

    /// The failure feature `index` ends with, when its check refused it before.
    /// Whether feature `index`'s check refused its bodies, leaving the failure in place.
    pub fn refused_at(&self, index: usize) -> Option<&Failure> {
        self.refused.get(&index)
    }

    pub fn refused(&mut self, index: usize) -> Option<Failure> {
        self.refused.remove(&index)
    }

    /// Runs `check`, feature `index`'s check, on a thread of its own. `Err`: no thread could
    /// start, and this is the check's own result, made here.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        index: usize,
        id: u64,
        key: u64,
        saved: Saved,
        failure: Failure,
        check: impl FnOnce() -> kernel::Result<()> + Send + 'static,
    ) -> kernel::Result<()> {
        // Taken by whichever runs it: the thread, or this one when the thread cannot start.
        let shared = Arc::new(Mutex::new(Some(check)));
        let taken = shared.clone();
        let started = kernel::kernel_thread("feature-check").spawn(move || match taken.lock().ok().and_then(|mut c| c.take()) {
            Some(check) => check(),
            None => Ok(()),
        });
        match started {
            Ok(done) => {
                self.running.push_back(Running { index, id, key, saved, failure, done });
                Ok(())
            }
            Err(_) => match shared.lock().ok().and_then(|mut c| c.take()) {
                Some(check) => check(),
                None => Ok(()),
            },
        }
    }

    /// Stores the cache entry of feature `index` once no check before it, or of it, is running.
    pub fn store(&mut self, index: usize, entry: CacheEntry, cache: &Mutex<EvalCache>, kept: &KeptSlot) {
        if self.running.front().is_some_and(|r| r.index <= index) {
            self.held.push((index, entry));
        } else {
            entry.put(cache, kept);
        }
    }

    /// Takes the checks that ended (all of them with `wait`, or while too many run), in order:
    /// the first that refused, with what it puts back. Cache entries are stored as the checks
    /// before them pass.
    pub fn settle(&mut self, wait: bool, cache: &Mutex<EvalCache>, kept: &KeptSlot) -> Option<Refused> {
        while let Some(front) = self.running.front() {
            if !(wait || self.running.len() > self.most || front.done.is_finished()) {
                break;
            }
            let Some(r) = self.running.pop_front() else { break };
            let passed = match r.done.join() {
                Ok(check) => check,
                Err(_) => Err(kernel::KernelError::Internal(format!("{}: the check's thread stopped", r.index))),
            };
            if let Err(error) = passed {
                // What came after it was evaluated on bodies the check refuses: none of it stands.
                // The checks still running are left to end on their own.
                self.running.clear();
                self.held.retain(|(i, _)| *i < r.index);
                return Some(Refused { index: r.index, id: r.id, key: r.key, saved: r.saved, failure: Failure { error, ..r.failure } });
            }
            let upto = self.running.front().map_or(usize::MAX, |next| next.index);
            for (i, entry) in std::mem::take(&mut self.held) {
                if i < upto {
                    entry.put(cache, kept);
                } else {
                    self.held.push((i, entry));
                }
            }
        }
        None
    }

    /// Feature `index`'s check refused its bodies: it ends with `failure` when evaluated again.
    pub fn refuse(&mut self, index: usize, failure: Failure) {
        self.refused.insert(index, failure);
    }

    /// A check is still running.
    pub fn running(&self) -> bool {
        !self.running.is_empty()
    }

    /// Every check started has ended (its thread is done): settling them waits for nothing.
    fn ended(&self) -> bool {
        self.running.iter().all(|r| r.done.is_finished())
    }
}

/// A check refusal an evaluation that did not wait for its checks left to the next one: the
/// feature `id` on the inputs of cache key `key` ends with `failure`.
#[derive(Clone)]
pub(super) struct Carried {
    pub id: u64,
    pub key: u64,
    pub failure: Failure,
}

impl std::fmt::Debug for Carried {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Carried {{ id: {}, key: {:x}, error: {} }}", self.id, self.key, self.failure.error)
    }
}

/// The checks an evaluation left running when it returned ([`Checks::running`]), shared by the
/// model and its copies: whichever settles them first stores the cache entries that waited for
/// them, and each takes the same outcome.
pub(super) struct Pending {
    state: Mutex<PendingState>,
    /// Woken when the settling ends ([`PendingState::Settling`]).
    settled: std::sync::Condvar,
}

enum PendingState {
    Running(Checks),
    /// A caller is settling them, outside the lock (it waits for checks still running).
    Settling,
    /// Every check ended: the first refusal, if one refused.
    Ended(Option<Carried>),
}

impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pending")
    }
}

impl Pending {
    pub fn new(checks: Checks) -> Arc<Pending> {
        Arc::new(Pending { state: Mutex::new(PendingState::Running(checks)), settled: std::sync::Condvar::new() })
    }

    /// Once every check has ended (waiting for them with `wait`): `Some` of the first refusal, if
    /// one refused. `None` while a check runs and `wait` is false. The cache entries held for the
    /// checks go into `cache` as the checks before them pass, as [`Checks::settle`] stores them.
    ///
    /// The lock is never held while a check is waited for: the caller that settles takes the
    /// checks out and settles them outside it. Meanwhile a caller that does not wait is answered
    /// at once (`None`), and one that waits waits for that settling to end, then takes its
    /// outcome.
    pub fn outcome(&self, wait: bool, cache: &Mutex<EvalCache>, kept: &KeptSlot) -> Option<Option<Carried>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let settling = match &*state {
                PendingState::Ended(outcome) => return Some(outcome.clone()),
                PendingState::Settling => {
                    if !wait {
                        return None;
                    }
                    true
                }
                PendingState::Running(checks) => {
                    if !(wait || checks.ended()) {
                        return None;
                    }
                    false
                }
            };
            if settling {
                state = self.settled.wait(state).unwrap_or_else(|e| e.into_inner());
                continue;
            }
            let PendingState::Running(mut checks) = std::mem::replace(&mut *state, PendingState::Settling) else {
                return None;
            };
            drop(state);
            let refused = checks.settle(true, cache, kept).map(|r| Carried { id: r.id, key: r.key, failure: r.failure });
            let mut done = self.state.lock().unwrap_or_else(|e| e.into_inner());
            *done = PendingState::Ended(refused.clone());
            self.settled.notify_all();
            return Some(refused);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::CachedEval;
    use super::*;
    use kernel::KernelError;

    fn saved() -> Saved {
        Saved {
            state: Arc::new(ModelState::default()),
            reserved: Default::default(),
            out_len: 0,
            reuse: false,
            prev_fp: 0,
            geo: 0,
            recomputed: 0,
            restored: 0,
            poses: HashMap::new(),
            moved: Vec::new(),
            failed: HashMap::new(),
            rolled_results: HashMap::new(),
        }
    }

    fn failure() -> Failure {
        Failure { error: KernelError::Failed(String::new()), warning: Some("kept".into()), import_report: None, repair: None, ms: 1.0 }
    }

    fn entry(key: u64) -> CacheEntry {
        let whole = CachedEval {
            error: None,
            error_code: None,
            warning: None,
            import_report: None,
            repair: None,
            lost_projections: None,
            ms: 0.0,
            state: Arc::new(ModelState::default()),
        };
        CacheEntry { key, whole, bytes: 1, change: None }
    }

    fn checks() -> Checks {
        Checks { on: true, most: 4, running: VecDeque::new(), held: Vec::new(), refused: HashMap::new() }
    }

    /// Only an exact checked shape is excluded before detaching can copy its native form.
    #[test]
    fn checked_thread_outputs_are_excluded_before_the_native_check_is_detached() {
        use super::super::{EvalValidationGuard, cost_tests, remember_checked_thread_output};
        let body = kernel::box_solid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(2.0, 2.0, 2.0)).unwrap();
        let equal_geometry = kernel::box_solid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(2.0, 2.0, 2.0)).unwrap();
        let after = ModelState {
            bodies: vec![ModelBody::new("Checked".into(), body.clone(), 1), ModelBody::new("Other".into(), equal_geometry, 1)],
            ..ModelState::default()
        };
        let _scope = EvalValidationGuard::new(kernel::Validation::EachFeature);
        kernel::check_kept(&body).unwrap();
        remember_checked_thread_output("Checked", &body);
        let mut detached = None;
        let work = cost_tests::counted(|| detached = checks().detach(&ModelState::default(), &after));
        let detached = detached.unwrap();
        assert_eq!(work.detached_checks, 1, "only the other native shape is detached");
        assert_eq!(work.reused_checks, 1);
        assert_eq!(detached.len(), 1);
        assert_eq!(detached[0].0, "Other");
        assert!(super::super::run_kept(detached, Vec::new()).is_ok(), "the other result still runs its full check");
    }

    /// Keeping a body's name cannot validate a later output that has another shape record.
    #[test]
    fn a_changed_thread_output_still_detaches_its_final_check() {
        use super::super::{EvalValidationGuard, cost_tests, remember_checked_thread_output};
        let body = kernel::box_solid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(2.0, 2.0, 2.0)).unwrap();
        let changed = kernel::box_solid(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::new(3.0, 2.0, 2.0)).unwrap();
        let after = ModelState { bodies: vec![ModelBody::new("Part".into(), changed, 1)], ..ModelState::default() };
        let _scope = EvalValidationGuard::new(kernel::Validation::EachFeature);
        kernel::check_kept(&body).unwrap();
        remember_checked_thread_output("Part", &body);
        let mut detached = None;
        let work = cost_tests::counted(|| detached = checks().detach(&ModelState::default(), &after));
        assert_eq!(work.detached_checks, 1);
        assert_eq!(work.reused_checks, 0);
        assert!(super::super::run_kept(detached.unwrap(), Vec::new()).is_ok());
    }

    /// A check that ends after the evaluation went on, with `r`.
    fn later(r: kernel::Result<()>) -> impl FnOnce() -> kernel::Result<()> + Send + 'static {
        move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            r
        }
    }

    /// Feature 1's check refuses its bodies while feature 0's passes: feature 0's result is
    /// stored, the results of features 1 and 2 (evaluated on the refused bodies) are not, and
    /// feature 1 ends with the check's error and the warnings it had.
    #[test]
    fn a_refused_check_takes_back_what_came_after_it() {
        let (cache, kept) = (Mutex::new(EvalCache::default()), KeptSlot(None));
        let mut c = checks();
        assert!(c.start(0, 100, 10, saved(), failure(), later(Ok(()))).is_ok());
        c.store(0, entry(10), &cache, &kept);
        assert!(c.start(1, 101, 11, saved(), failure(), later(Err(KernelError::Failed("refused".into())))).is_ok());
        c.store(1, entry(11), &cache, &kept);
        c.store(2, entry(12), &cache, &kept);
        assert!(cache.lock().unwrap().is_empty(), "held while the checks run");
        let refused = c.settle(true, &cache, &kept).expect("the second check refuses");
        assert_eq!(refused.index, 1);
        assert_eq!(refused.failure.error, KernelError::Failed("refused".into()));
        assert_eq!(refused.failure.warning.as_deref(), Some("kept"));
        {
            let m = cache.lock().unwrap();
            assert!(m.states.map.contains_key(&10) && !m.states.map.contains_key(&11) && !m.states.map.contains_key(&12));
        }
        c.refuse(1, refused.failure);
        assert!(c.refused(1).is_some() && c.refused(1).is_none());
        assert!(c.settle(true, &cache, &kept).is_none());
    }

    /// Checks an evaluation left running ([`Pending`]): nothing is settled while they run unless
    /// asked to wait; then the results held for them are stored as the checks before them pass,
    /// and the first refusal is the outcome, naming its feature and the inputs it refused, for
    /// every model sharing them.
    #[test]
    fn checks_left_running_settle_once_with_the_first_refusal() {
        let (cache, kept) = (Mutex::new(EvalCache::default()), KeptSlot(None));
        let mut c = checks();
        let slow = |r: kernel::Result<()>| {
            move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                r
            }
        };
        assert!(c.start(0, 100, 10, saved(), failure(), slow(Ok(()))).is_ok());
        c.store(0, entry(10), &cache, &kept);
        assert!(c.start(1, 101, 11, saved(), failure(), slow(Err(KernelError::Failed("refused".into())))).is_ok());
        c.store(1, entry(11), &cache, &kept);
        c.store(2, entry(12), &cache, &kept);
        assert!(c.running());
        let pending = Pending::new(c);
        assert!(pending.outcome(false, &cache, &kept).is_none(), "still running: nothing settled");
        assert!(cache.lock().unwrap().is_empty());
        let refused = pending.outcome(true, &cache, &kept).expect("ended").expect("the second check refuses");
        assert_eq!((refused.id, refused.key), (101, 11));
        assert_eq!(refused.failure.error, KernelError::Failed("refused".into()));
        assert_eq!(refused.failure.warning.as_deref(), Some("kept"));
        {
            let m = cache.lock().unwrap();
            assert!(m.states.map.contains_key(&10) && !m.states.map.contains_key(&11) && !m.states.map.contains_key(&12));
        }
        // A copy sharing them takes the same outcome.
        let again = pending.outcome(false, &cache, &kept).expect("ended").expect("refused");
        assert_eq!((again.id, again.key), (101, 11));
    }

    /// A design whose join SMLib's check refuses: a box joined to an imported block with one
    /// edge off its face (the block already fails the check).
    fn refused_join() -> crate::Document {
        let mut doc = crate::Document::new("refused join");
        let step: Arc<str> = include_str!("../../../tests/step-regressions/block_edge_off_its_face.step").into();
        let kind = crate::FeatureKind::Import { file: "block.step".into(), step, components: Vec::new(), skip: Vec::new(), records: None };
        doc.add_feature(kind, Some("Block")).unwrap();
        doc
    }

    /// What an evaluation leaves to compare by: each feature's id, error and warning, and the
    /// bodies after the last one with their volumes. An import's warning is left out: its own
    /// check of the parts runs on after the import shows and joins the warning when settled,
    /// whenever that is (`Model::settle_import_checks`), which is not what this compares.
    fn outcome_of(m: &super::super::Model) -> Vec<String> {
        let mut out: Vec<String> = m
            .results
            .iter()
            .map(|r| {
                let warning = if r.import_report.is_some() || r.warning.as_deref().is_some_and(|w| w.starts_with("solid '")) {
                    None
                } else {
                    r.warning.clone()
                };
                format!("{} {:?} {warning:?} stale {}", r.id, r.error, r.stale)
            })
            .collect();
        for b in &m.state().bodies {
            let v = kernel::measure(&b.body).map(|p| format!("{:.9}", p.volume)).unwrap_or_default();
            out.push(format!("{} {v}", b.name));
        }
        out
    }

    /// The window's evaluation shows its results before the checks of its last features end
    /// (`Model::set_background_checks`). Settled, and evaluated again where a check refused,
    /// it ends with the results, errors and warnings of the evaluation that waited for them,
    /// for a design whose features pass and for one whose join the check refuses. While a check
    /// runs, a save keeps no model.
    #[test]
    fn checks_left_running_end_as_the_evaluation_that_waited() {
        use super::super::Model;
        let good = {
            let mut doc = crate::Document::new("good");
            for i in 0..4 {
                let kind = crate::FeatureKind::Box {
                    corner: crate::point_expr(solidcore_record::Vec3::new(i as f64 * 5.0, 0.0, 0.0)),
                    length: "8".into(),
                    width: "8".into(),
                    height: "8".into(),
                    operation: if i == 0 { crate::Operation::NewBody } else { crate::Operation::Join },
                    centred: false,
                };
                doc.add_feature(kind, None).unwrap();
            }
            doc
        };
        let mut refused = refused_join();
        refused
            .add_feature(
                crate::FeatureKind::Box {
                    corner: crate::point_expr(solidcore_record::Vec3::new(15.0, 25.0, 5.0)),
                    length: "10".into(),
                    width: "10".into(),
                    height: "10".into(),
                    operation: crate::Operation::Join,
                    centred: false,
                },
                Some("Join"),
            )
            .unwrap();
        for (name, doc) in [("good", &good), ("refused", &refused)] {
            let mut waited = Model::new();
            waited.evaluate(doc);
            let mut shown = Model::new();
            shown.set_background_checks(true);
            shown.evaluate(doc);
            if shown.checking_features() {
                assert!(shown.saved(doc).is_none(), "{name}: no model is kept while a check runs");
            }
            if shown.settle_feature_checks(true) {
                assert!(shown.is_stale(), "{name}: a refusal leaves the results stale until evaluated");
                shown.evaluate(doc);
            }
            assert!(!shown.checking_features() && !shown.settle_feature_checks(false));
            assert_eq!(outcome_of(&shown), outcome_of(&waited), "{name}");
            // The cache holds what the checks passed: evaluated again, nothing is computed.
            shown.evaluate(doc);
            assert_eq!(outcome_of(&shown), outcome_of(&waited), "{name}");
        }
    }

    /// With no check running a result is stored at once; once the checks pass, so is every
    /// result that waited for them.
    #[test]
    fn passed_checks_store_what_waited() {
        let (cache, kept) = (Mutex::new(EvalCache::default()), KeptSlot(None));
        let mut c = checks();
        c.store(0, entry(10), &cache, &kept);
        assert_eq!(cache.lock().unwrap().len(), 1);
        assert!(c.start(1, 101, 11, saved(), failure(), later(Ok(()))).is_ok());
        c.store(1, entry(11), &cache, &kept);
        c.store(2, entry(12), &cache, &kept);
        assert!(c.settle(true, &cache, &kept).is_none());
        assert_eq!(cache.lock().unwrap().len(), 3);
    }
}
