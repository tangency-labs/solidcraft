//! SMLib's check of an import's parts, run after the parts are read: on worker threads beside
//! whatever the caller does next (the design opens at the speed of the read), or as further
//! steps of a read with no threads ([`super::StepReading::step`]).
//!
//! Each part is checked once per class of the placements of its instances
//! ([`super::check_class`]): a part placed many times rigidly is checked once, and its warning
//! counts every instance of it. What the checks found ([`ImportChecks::issues`]) is in the order
//! the parts were read, whichever thread finished first, so a window shows the same warnings in
//! the same order every time.

use std::sync::{Arc, Condvar, Mutex, PoisonError};

use super::{ImportIssue, IssueKind};
use crate::body::Body;

/// One body to check: the first instance of a part under one class of its placements.
pub(super) struct Job {
    pub item: u64,
    /// The part's own name in the file.
    pub part: String,
    /// How the warning names it (`solid 'x'`, or with its placement that scales it).
    pub what: String,
    /// The body placed, as an operation on it takes it.
    pub body: Body,
    /// Instances of the part under this class of placement.
    pub times: usize,
}

/// What one check found.
#[derive(Clone, Debug)]
pub(super) enum Outcome {
    /// It passes (or SMLib refuses the body outright, which the read already said).
    Passes,
    /// It fails, or cannot run: why, worded for the warning.
    NeedsRepair(String),
    /// The token over the checks stopped it.
    Stopped,
}

/// Check `job`'s body under the token and validation mode of this thread.
pub(super) fn check(job: &Job) -> Outcome {
    let _span = solidcraft_profile::span("import", "step.check");
    match crate::validation::failing_check(&job.body) {
        Ok(None) => Outcome::Passes,
        Ok(Some(failure)) => Outcome::NeedsRepair(format!("SMLib's check fails it: {failure}")),
        Err(_) if solidcraft_smlib::stop::current().is_some_and(|s| s.is_stopped()) => Outcome::Stopped,
        Err(e) => Outcome::NeedsRepair(format!("SMLib's check could not run: {e}")),
    }
}

/// What the checks of an import found, once all have run.
#[derive(Clone, Debug)]
struct Settled {
    /// A `NeedsRepair` issue for each part a check fails or could not run on, in read order.
    issues: Arc<Vec<ImportIssue>>,
    /// A token stopped a check: the list is not complete.
    stopped: bool,
}

/// The issues of `jobs` given their outcomes (in the same order).
fn settle(jobs: &[Job], outcomes: &[Outcome]) -> Settled {
    let mut issues = Vec::new();
    let mut stopped = false;
    for (job, outcome) in jobs.iter().zip(outcomes) {
        match outcome {
            Outcome::Passes => {}
            Outcome::Stopped => stopped = true,
            Outcome::NeedsRepair(why) => {
                let msg = format!("{}: needs repair before it can be modelled: {why}", job.what);
                let detail = if job.times > 1 { format!("{msg} (×{})", job.times) } else { msg };
                issues.push(ImportIssue {
                    kind: IssueKind::NeedsRepair,
                    item: Some(job.item),
                    part: job.part.clone(),
                    faces: 0,
                    times: job.times,
                    detail,
                });
            }
        }
    }
    Settled { issues: Arc::new(issues), stopped }
}

/// Check `jobs` on up to `workers` threads, under the validation mode `scope`, under `over` too
/// when given, and under the run's own token; then say what they found.
fn run(shared: &Shared, jobs: &[Job], workers: usize, scope: Option<Arc<crate::validation::Scope>>, over: Option<solidcraft_smlib::stop::Stop>) {
    let own = shared.stop.clone();
    // Whatever happens here, a waiter is woken: a panic outside the per-check guard ends the run
    // as stopped (its list incomplete) instead of leaving `wait` blocked.
    let settled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let outcomes: Vec<Outcome> = solidcraft_smlib::stop::scoped(over, || {
            own.within(|| {
                crate::validation::within(scope, || {
                    crate::par::map_on_threads(jobs, workers.max(1), check)
                        .into_iter()
                        .map(|o| o.unwrap_or_else(|| Outcome::NeedsRepair("SMLib's check could not run: it failed in the kernel".into())))
                        .collect()
                })
            })
        });
        settle(jobs, &outcomes)
    }));
    shared.finish(settled.unwrap_or_else(|_| Settled { issues: Arc::default(), stopped: true }));
}

struct Shared {
    /// Stops the checks when nobody holds the import any more.
    stop: solidcraft_smlib::stop::Stop,
    settled: Mutex<Option<Settled>>,
    done: Condvar,
}

impl Shared {
    fn finish(&self, settled: Settled) {
        *self.settled.lock().unwrap_or_else(PoisonError::into_inner) = Some(settled);
        self.done.notify_all();
    }

    fn settled(&self) -> Option<Settled> {
        self.settled.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn wait(&self) -> Settled {
        let mut g = self.settled.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(s) = g.as_ref() {
                return s.clone();
            }
            g = self.done.wait(g).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Held by every copy of the import's report: the checks stop when the last copy goes.
struct Handle(Arc<Shared>);

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.stop.stop();
    }
}

/// SMLib's check of an import's parts ([`super::ImportReport::checks`]), running or done. Copies
/// share the one run; the run stops when no copy is left.
#[derive(Clone, Default)]
pub struct ImportChecks(Option<Arc<Handle>>);

impl std::fmt::Debug for ImportChecks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.issues() {
            None => f.write_str("ImportChecks(running)"),
            Some(issues) => write!(f, "ImportChecks({} issues)", issues.len()),
        }
    }
}

/// Two reports' checks are the same when they are one run (or neither has any).
impl PartialEq for ImportChecks {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl ImportChecks {
    fn shared(settled: Option<Settled>) -> (ImportChecks, Arc<Shared>) {
        let shared = Arc::new(Shared { stop: solidcraft_smlib::stop::Stop::new(), settled: Mutex::new(settled), done: Condvar::new() });
        (ImportChecks(Some(Arc::new(Handle(shared.clone())))), shared)
    }

    /// Checks that have already run, with these outcomes (a read with no threads ran them in its
    /// steps).
    pub(super) fn ran(jobs: &[Job], outcomes: &[Outcome]) -> ImportChecks {
        if jobs.is_empty() {
            return ImportChecks::default();
        }
        ImportChecks::shared(Some(settle(jobs, outcomes))).0
    }

    /// Start checking `jobs` on up to `workers` threads, under the calling thread's validation
    /// mode, and under `over` too when given (a token that stops them with the caller's work).
    /// With no threads (the browser, or none can start), they run here before this returns.
    pub(super) fn start(jobs: Vec<Job>, workers: usize, over: Option<solidcraft_smlib::stop::Stop>) -> ImportChecks {
        if jobs.is_empty() {
            return ImportChecks::default();
        }
        let (checks, shared) = ImportChecks::shared(None);
        let scope = crate::validation::current();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let jobs = Arc::new(jobs);
            let (jobs_t, shared_t, scope_t, over_t) = (jobs.clone(), shared.clone(), scope.clone(), over.clone());
            if crate::par::kernel_thread("step-checks").spawn(move || run(&shared_t, &jobs_t, workers, scope_t, over_t)).is_err() {
                run(&shared, &jobs, workers, scope, over);
            }
        }
        #[cfg(target_arch = "wasm32")]
        run(&shared, &jobs, workers, scope, over);
        checks
    }

    /// The checks ended stopped by a token: their list is not complete.
    pub fn stopped(&self) -> bool {
        self.0.as_ref().and_then(|h| h.0.settled()).is_some_and(|s| s.stopped)
    }

    /// No part was checked (none to check, or the report was read from a saved design).
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// The checks are still running.
    pub fn is_running(&self) -> bool {
        self.issues().is_none()
    }

    /// What the checks found, `None` while they run: a `NeedsRepair` issue for each part a check
    /// fails or cannot run on, in the order the parts were read.
    pub fn issues(&self) -> Option<Arc<Vec<ImportIssue>>> {
        match &self.0 {
            None => Some(Arc::default()),
            Some(h) => h.0.settled().map(|s| s.issues),
        }
    }

    /// [`Self::issues`], waiting for the checks to end; `None` when a token stopped them.
    pub fn wait(&self) -> Option<Arc<Vec<ImportIssue>>> {
        match &self.0 {
            None => Some(Arc::default()),
            Some(h) => {
                let s = h.0.wait();
                (!s.stopped).then_some(s.issues)
            }
        }
    }
}
