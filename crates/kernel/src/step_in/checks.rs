//! SMLib's check of an import's parts, run after the parts are read: on worker threads beside
//! whatever the caller does next (the design opens at the speed of the read), or as further
//! steps of a read with no threads ([`super::StepReading::step`]).
//!
//! Each part is checked once per class of the placements of its instances
//! ([`super::check_class`]): a part placed many times rigidly is checked once, and its warning
//! counts every instance of it. What the checks found ([`ImportChecks::issues`]) is in the order
//! the parts were read, whichever thread finished first, so a window shows the same warnings in
//! the same order every time.
//!
//! A part whose check fails is then healed of its tolerance-scale defects
//! ([`super::parallel::heal`]), in the same run: decided by the part's own check, as the file
//! defines it, so it is the part every instance shares that is healed or not, whichever instance
//! was read first. Each of its instances is the healed part placed ([`ImportHeals`]), and each
//! class of its placements is checked again on it, so the warnings are the healed parts'. The
//! heals, like the warnings, are in read order whatever the threads, and are dropped with a run
//! a token stops.

use std::sync::{Arc, Condvar, Mutex, PoisonError};

use std::collections::HashMap;

use super::{IDENTITY, ImportIssue, ImportedBody, IssueKind, M4, parallel};
use crate::body::Body;
use crate::validation::CheckFailure;

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
    /// How the import's notes name the part (`solid 'x'`, or `item #n`).
    pub label: String,
    /// The placement `body` is the part at: the class's first instance's.
    pub m: M4,
    /// The part as the file defines it, which every instance places.
    pub source: Arc<parallel::Part>,
}

/// What the heal of an import's parts needs besides the check jobs: where each body of the
/// import is the part of which item, and how the reader dresses a part's body.
#[derive(Default)]
pub(super) struct Heals {
    /// Heal nothing: the preference is off ([`crate::heals_tolerance_defects`]), and the parts
    /// stay as read, with what their checks found.
    pub as_read: bool,
    /// Each body of the import, by its index: its item and placement.
    pub instances: Vec<(u64, M4)>,
    /// The file's colours, for the healed parts' faces.
    pub paints: Paints,
    /// How many bodies place each item (one placed more than once shares its display).
    pub uses: HashMap<u64, usize>,
}

/// The file's colours of items and faces, and their names and opacities
/// ([`super::dress`]).
#[derive(Default)]
pub(super) struct Paints {
    pub colors: HashMap<u64, [f32; 3]>,
    pub color_names: HashMap<u64, String>,
    pub opacity: HashMap<u64, f32>,
}

/// One body of an import replaced by its part healed ([`ImportHeals`]).
#[derive(Clone, Debug)]
pub struct HealedBody {
    /// The body's index in [`super::StepImport::bodies`].
    pub index: usize,
    /// The healed part, placed and dressed as the body was.
    pub body: Body,
    /// The file face of each of its faces.
    pub face_origin: Vec<u64>,
    /// For each of its faces, the face of the body as read it was (the heal only removes
    /// faces): what a reference to a face of the body as read finds in it.
    pub face_source: Vec<usize>,
}

/// The bodies of an import whose parts the checks healed, in the order of the import's bodies
/// ([`ImportChecks::heals`]).
#[derive(Debug)]
pub struct ImportHeals {
    bodies: Vec<HealedBody>,
    digest: u64,
}

impl ImportHeals {
    /// The healed body that replaces the import's body `index`, if its part was healed.
    pub fn body(&self, index: usize) -> Option<&HealedBody> {
        self.bodies.binary_search_by_key(&index, |b| b.index).ok().and_then(|i| self.bodies.get(i))
    }

    /// The healed bodies, by index.
    pub fn bodies(&self) -> &[HealedBody] {
        &self.bodies
    }

    /// A digest of what was healed (which bodies, and their faces' file faces). It does not hash
    /// the healed geometry: the heal of a file is the same every time (deterministic, whatever
    /// the threads), and a heal is only ever compared with heals of the same text, so equal heals
    /// of one file have equal digests.
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// `bodies` (an import's, by index) with the healed ones replaced.
    pub(super) fn apply(&self, bodies: &mut [ImportedBody]) {
        for h in &self.bodies {
            if let Some(b) = bodies.get_mut(h.index) {
                b.body = h.body.clone();
                b.face_origin.clone_from(&h.face_origin);
            }
        }
    }

    fn new(mut bodies: Vec<HealedBody>) -> ImportHeals {
        use std::hash::{Hash, Hasher};
        bodies.sort_by_key(|b| b.index);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        "import heals".hash(&mut h);
        for b in &bodies {
            (b.index, b.body.face_count(), &b.face_origin, &b.face_source).hash(&mut h);
        }
        // Never 0, which says "no heals" to a digest that folds this in.
        ImportHeals { digest: h.finish() | 1, bodies }
    }
}

/// What one check found.
#[derive(Clone, Debug)]
pub(super) enum Outcome {
    /// It passes (or SMLib refuses the body outright, which the read already said).
    Passes,
    /// It fails, and what SMLib's check found ([`Outcome::why`] words it for the warning): kept
    /// whole so the heal of the part reuses it rather than check the part again.
    NeedsRepair(CheckFailure),
    /// It cannot run: why, worded for the warning.
    CouldNotRun(String),
    /// The token over the checks stopped it.
    Stopped,
}

impl Outcome {
    /// Why the part needs repair, worded for the warning; `None` when it passes or was stopped.
    fn why(&self) -> Option<String> {
        match self {
            Outcome::NeedsRepair(failure) => Some(format!("SMLib's check fails it: {failure}")),
            Outcome::CouldNotRun(why) => Some(why.clone()),
            Outcome::Passes | Outcome::Stopped => None,
        }
    }
}

/// Check `job`'s body under the token and validation mode of this thread.
pub(super) fn check(job: &Job) -> Outcome {
    // On a worker thread: the span says which part it checks (#22473).
    let _span = solidcraft_profile::span("import", "step.check").arg("part", &job.label);
    check_body(&job.body)
}

/// SMLib's check of `body`, as [`check`] says it.
fn check_body(body: &Body) -> Outcome {
    match crate::validation::failing_check(body) {
        Ok(None) => Outcome::Passes,
        Ok(Some(failure)) => Outcome::NeedsRepair(failure),
        Err(_) if solidcraft_smlib::stop::current().is_some_and(|s| s.is_stopped()) => Outcome::Stopped,
        Err(e) => Outcome::CouldNotRun(format!("SMLib's check could not run: {e}")),
    }
}

/// What healing one item's part did.
#[derive(Default)]
struct ItemHeal {
    /// Its bodies, the healed part placed.
    bodies: Vec<HealedBody>,
    /// Its check jobs checked again on the healed part: job index and outcome.
    rechecked: Vec<(usize, Outcome)>,
    /// What the import says of it: healed, or a heal that failed in the kernel.
    note: Option<(IssueKind, String)>,
    stopped: bool,
}

/// The items to heal: each item one of whose checks fails or cannot run, with its jobs, in
/// read order.
fn to_heal(jobs: &[Job], outcomes: &[Outcome]) -> Vec<(u64, Vec<usize>)> {
    // A file whose checks all pass (most) costs one pass over the outcomes.
    let failing: std::collections::HashSet<u64> =
        jobs.iter().zip(outcomes).filter(|(_, o)| matches!(o, Outcome::NeedsRepair(_) | Outcome::CouldNotRun(_))).map(|(j, _)| j.item).collect();
    let mut items: Vec<(u64, Vec<usize>)> = Vec::new();
    let mut at: HashMap<u64, usize> = HashMap::new();
    for (k, job) in jobs.iter().enumerate().filter(|(_, j)| failing.contains(&j.item)) {
        match at.get(&job.item).and_then(|i| items.get_mut(*i)) {
            Some((_, of)) => of.push(k),
            None => {
                at.insert(job.item, items.len());
                items.push((job.item, vec![k]));
            }
        }
    }
    items
}

/// Heal the part of `item` (whose jobs are `of`) when its own check fails: decided on the part
/// as the file defines it (the check of a job at the identity is that check), whatever its
/// instances' placements. Kept, every instance of it is the healed part placed and dressed,
/// and each of its jobs is checked again on the healed part (at the identity, the heal's own
/// check of the body it kept).
///
/// The part's own check is handed to the heal, which would otherwise run it again: it is the
/// check of `part.body` itself. A job at the identity checks `part.body` placed there, which is
/// that very body ([`parallel::placed`] gives a clone, which shares its SMLib form), dressed
/// for display (the file's colours and names, a shared mesh: nothing the check reads); with no
/// such job it is checked here, on `part.body`.
fn heal_item(jobs: &[Job], outcomes: &[Outcome], item: u64, of: &[usize], heals: &Heals) -> ItemHeal {
    let _span = solidcraft_profile::span("import", "step.heal");
    let Some(first) = of.first().and_then(|k| jobs.get(*k)) else { return ItemHeal::default() };
    let part = &first.source;
    let own = match of.iter().find(|k| jobs.get(**k).is_some_and(|j| j.m == IDENTITY)) {
        Some(k) => outcomes.get(*k).cloned().unwrap_or(Outcome::Passes),
        None => check_body(&part.body),
    };
    let own = match own {
        Outcome::NeedsRepair(failure) => failure,
        Outcome::Stopped => return ItemHeal { stopped: true, ..ItemHeal::default() },
        Outcome::Passes | Outcome::CouldNotRun(_) => return ItemHeal::default(),
    };
    let (healed, note, kept, face_source) = match parallel::heal(part, item, Some(&own)) {
        parallel::Healed::Kept(healed, note, kept, face_source) => (healed, note, kept, face_source),
        parallel::Healed::NotKept => return ItemHeal::default(),
        parallel::Healed::Stopped => return ItemHeal { stopped: true, ..ItemHeal::default() },
        parallel::Healed::CouldNotRun(e) => {
            // A panic in the heal is the kernel's internal error: reported, as a check that
            // cannot run is. Any other failure leaves the check's verdict.
            if let crate::KernelError::Internal(why) = &e {
                log::warn!(target: "step", "the heal of item #{item} failed: {e}");
                let detail = format!("{}: the tolerance heal failed: kernel internal error: {why}", first.label);
                return ItemHeal { note: Some((IssueKind::NeedsRepair, detail)), ..ItemHeal::default() };
            }
            log::debug!(target: "step", "the heal of item #{item} could not run: {e}");
            return ItemHeal::default();
        }
    };
    let place = |m: &M4| solidcraft_smlib::stop::unstoppable(|| parallel::placed(&healed.body, m));
    let mut bodies = Vec::new();
    // A part placed more than once shows its first instance's mesh, placed.
    let mut shown: Option<(Arc<crate::body::SharedDisplay>, M4)> = None;
    for (index, (_, m)) in heals.instances.iter().enumerate().filter(|(_, (i, _))| *i == item) {
        // each placement runs unstoppable: a stop met between them ends the item, nothing kept
        if solidcraft_smlib::stop::current().is_some_and(|s| s.is_stopped()) {
            return ItemHeal { stopped: true, ..ItemHeal::default() };
        }
        let Ok(body) = place(m) else {
            log::debug!(target: "step", "the healed part of item #{item} could not be placed");
            return ItemHeal::default();
        };
        let body = super::dress(&heals.paints, body, item, &healed.face_origin);
        let body = match &shown {
            Some((shared, at)) => body.with_display_of(shared.clone(), super::mat_mul(m, &super::placement_inverse(at))),
            None if heals.uses.get(&item).is_some_and(|n| *n > 1) => {
                let shared = Arc::new(crate::body::SharedDisplay::new(body.clone()));
                shown = Some((shared.clone(), *m));
                body.with_display_of(shared, IDENTITY)
            }
            None => body,
        };
        bodies.push(HealedBody { index, body, face_origin: healed.face_origin.clone(), face_source: face_source.clone() });
    }
    let mut rechecked = Vec::new();
    for &k in of {
        let Some(job) = jobs.get(k) else { continue };
        let outcome = if job.m == IDENTITY {
            match &kept {
                None => Outcome::Passes,
                Some(failure) => Outcome::NeedsRepair(failure.clone()),
            }
        } else {
            match place(&job.m) {
                Ok(body) => check_body(&body),
                Err(e) => Outcome::CouldNotRun(format!("SMLib's check could not run: {e}")),
            }
        };
        if matches!(outcome, Outcome::Stopped) {
            return ItemHeal { stopped: true, ..ItemHeal::default() };
        }
        // The heal is the part every instance shares: a placement whose check passed on the
        // part as read and fails on the healed part keeps the part as read, for all of them.
        if matches!(outcomes.get(k), Some(Outcome::Passes)) && !matches!(outcome, Outcome::Passes) {
            log::debug!(target: "step", "the heal of item #{item} not kept: a placement that passed fails healed");
            return ItemHeal::default();
        }
        rechecked.push((k, outcome));
    }
    ItemHeal { bodies, rechecked, note: Some((IssueKind::Repaired, format!("{}: {note}", first.label))), stopped: false }
}

/// Heal the parts `outcomes` fail on ([`heal_item`]), on up to `workers` threads: the outcomes
/// checked again on the healed parts, the notes to say before each item's issues, the heals,
/// and whether a token stopped it.
fn heal_parts(
    jobs: &[Job],
    outcomes: &mut [Outcome],
    heals: &Heals,
    workers: usize,
) -> (HashMap<u64, (IssueKind, String)>, Option<Arc<ImportHeals>>, bool) {
    let items = if heals.as_read { Vec::new() } else { to_heal(jobs, outcomes) };
    if items.is_empty() {
        return (HashMap::new(), None, false);
    }
    let done: Vec<ItemHeal> = {
        let outcomes: &[Outcome] = outcomes;
        crate::par::map_on_threads(&items, workers.max(1), |(item, of)| heal_item(jobs, outcomes, *item, of, heals))
            .into_iter()
            .zip(&items)
            .map(|(done, (item, _))| {
                done.unwrap_or_else(|| {
                    let label = jobs.iter().find(|j| j.item == *item).map(|j| j.label.clone()).unwrap_or_default();
                    ItemHeal {
                        note: Some((IssueKind::NeedsRepair, format!("{label}: the tolerance heal failed: it failed in the kernel"))),
                        ..ItemHeal::default()
                    }
                })
            })
            .collect()
    };
    let mut notes = HashMap::new();
    let mut bodies = Vec::new();
    // A heal the token stopped may have ended as no heal: the run is stopped, whatever finished.
    let mut stopped = solidcraft_smlib::stop::current().is_some_and(|s| s.is_stopped());
    for (done, (item, _)) in done.into_iter().zip(&items) {
        stopped |= done.stopped;
        for (k, outcome) in done.rechecked {
            if let Some(slot) = outcomes.get_mut(k) {
                *slot = outcome;
            }
        }
        if let Some(note) = done.note {
            notes.insert(*item, note);
        }
        bodies.extend(done.bodies);
    }
    let heals = (!stopped && !bodies.is_empty()).then(|| Arc::new(ImportHeals::new(bodies)));
    (notes, heals, stopped)
}

/// What the checks of an import found, once all have run.
#[derive(Clone, Debug)]
struct Settled {
    /// A `NeedsRepair` issue for each part a check fails or could not run on, in read order.
    issues: Arc<Vec<ImportIssue>>,
    /// A token stopped a check: the list is not complete.
    stopped: bool,
    /// The bodies whose parts were healed (none when stopped).
    heals: Option<Arc<ImportHeals>>,
}

/// Heal what the checks of `jobs` fail ([`heal_parts`]) and say what they found: the issues of
/// `jobs` given their outcomes (in the same order), each healed item's note before its first.
fn settle(jobs: &[Job], mut outcomes: Vec<Outcome>, heals: &Heals, workers: usize) -> Settled {
    let (mut notes, healed, mut stopped) = if outcomes.iter().any(|o| matches!(o, Outcome::Stopped)) {
        (HashMap::new(), None, true)
    } else {
        heal_parts(jobs, &mut outcomes, heals, workers)
    };
    let mut issues = Vec::new();
    for (job, outcome) in jobs.iter().zip(&outcomes) {
        if let Some((kind, detail)) = notes.remove(&job.item) {
            issues.push(ImportIssue { kind, item: Some(job.item), part: job.part.clone(), paths: Vec::new(), faces: 0, times: 1, detail });
        }
        if matches!(outcome, Outcome::Stopped) {
            stopped = true;
        }
        if let Some(why) = outcome.why() {
            let msg = format!("{}: needs repair before it can be modelled: {why}", job.what);
            let detail = if job.times > 1 { format!("{msg} (×{})", job.times) } else { msg };
            issues.push(ImportIssue {
                kind: IssueKind::NeedsRepair,
                item: Some(job.item),
                part: job.part.clone(),
                paths: Vec::new(),
                faces: 0,
                times: job.times,
                detail,
            });
        }
    }
    let heals = if stopped { None } else { healed };
    Settled { issues: Arc::new(issues), stopped, heals }
}

/// Check `jobs` on up to `workers` threads, under the validation mode `scope`, under `over` too
/// when given, and under the run's own token; then say what they found.
fn run(
    shared: &Shared,
    jobs: &[Job],
    heals: &Heals,
    workers: usize,
    scope: Option<Arc<crate::validation::Scope>>,
    over: Option<solidcraft_smlib::stop::Stop>,
) {
    let own = shared.stop.clone();
    // Whatever happens here, a waiter is woken: a panic outside the per-check guard ends the run
    // as stopped (its list incomplete) instead of leaving `wait` blocked.
    let settled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        solidcraft_smlib::stop::scoped(over, || {
            own.within(|| {
                crate::validation::within(scope, || {
                    let outcomes = crate::par::map_on_threads(jobs, workers.max(1), check)
                        .into_iter()
                        .map(|o| o.unwrap_or_else(|| Outcome::CouldNotRun("SMLib's check could not run: it failed in the kernel".into())))
                        .collect();
                    settle(jobs, outcomes, heals, workers)
                })
            })
        })
    }));
    shared.finish(settled.unwrap_or_else(|_| Settled { issues: Arc::default(), stopped: true, heals: None }));
}

struct Shared {
    /// Stops the checks when nobody holds the import any more.
    stop: solidcraft_smlib::stop::Stop,
    settled: Mutex<Option<Settled>>,
    done: Condvar,
    /// Called once when the run ends ([`Shared::finish`]): a window's request for the frame its
    /// result needs, instead of polling [`ImportChecks::is_running`] while it runs.
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Shared {
    fn finish(&self, settled: Settled) {
        // The lock is released before `wake` is taken: `set_wake` locks them in the other order.
        *self.settled.lock().unwrap_or_else(PoisonError::into_inner) = Some(settled);
        let wake = self.wake.lock().unwrap_or_else(PoisonError::into_inner).take();
        self.done.notify_all();
        if let Some(wake) = wake {
            wake();
        }
    }

    /// Keep `wake` for [`Shared::finish`], unless the run ended already: `false` then, and the
    /// caller wakes itself.
    fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) -> bool {
        let mut slot = self.wake.lock().unwrap_or_else(PoisonError::into_inner);
        if self.settled.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
            return false;
        }
        if slot.is_none() {
            *slot = Some(wake);
        }
        true
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
        let shared = Arc::new(Shared {
            stop: solidcraft_smlib::stop::Stop::new(),
            settled: Mutex::new(settled),
            done: Condvar::new(),
            wake: Mutex::new(None),
        });
        (ImportChecks(Some(Arc::new(Handle(shared.clone())))), shared)
    }

    /// Checks that have ended, finding `issues` and healing `bodies`: for callers' tests, which
    /// cannot make a file whose part SMLib's check fails.
    #[doc(hidden)]
    pub fn ended_for_tests(issues: Vec<ImportIssue>, bodies: Vec<HealedBody>) -> ImportChecks {
        let heals = (!bodies.is_empty()).then(|| Arc::new(ImportHeals::new(bodies)));
        ImportChecks::shared(Some(Settled { issues: Arc::new(issues), stopped: false, heals })).0
    }

    /// Checks still running until the returned function ends them, finding `issues`: for
    /// callers' tests of what happens while an import's check runs.
    #[doc(hidden)]
    pub fn running_for_tests() -> (ImportChecks, impl FnOnce(Vec<ImportIssue>) + Send) {
        let (checks, shared) = ImportChecks::shared(None);
        (checks, move |issues: Vec<ImportIssue>| shared.finish(Settled { issues: Arc::new(issues), stopped: false, heals: None }))
    }

    /// Checks that have already run, with these outcomes (a read with no threads ran them in its
    /// steps).
    /// The heal of the parts they fail runs here, on this thread.
    pub(super) fn ran(jobs: &[Job], outcomes: Vec<Outcome>, heals: &Heals) -> ImportChecks {
        if jobs.is_empty() {
            return ImportChecks::default();
        }
        ImportChecks::shared(Some(settle(jobs, outcomes, heals, 1))).0
    }

    /// Start checking `jobs` on up to `workers` threads, then healing the parts they fail
    /// ([`Heals`]), under the calling thread's validation mode, and under `over` too when given
    /// (a token that stops them with the caller's work). With no threads (the browser, or none
    /// can start), they run here before this returns.
    pub(super) fn start(jobs: Vec<Job>, heals: Heals, workers: usize, over: Option<solidcraft_smlib::stop::Stop>) -> ImportChecks {
        if jobs.is_empty() {
            return ImportChecks::default();
        }
        let (checks, shared) = ImportChecks::shared(None);
        let scope = crate::validation::current();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let input = Arc::new((jobs, heals));
            let (input_t, shared_t, scope_t, over_t) = (input.clone(), shared.clone(), scope.clone(), over.clone());
            if crate::par::kernel_thread("step-checks").spawn(move || run(&shared_t, &input_t.0, &input_t.1, workers, scope_t, over_t)).is_err() {
                run(&shared, &input.0, &input.1, workers, scope, over);
            }
        }
        #[cfg(target_arch = "wasm32")]
        run(&shared, &jobs, &heals, workers, scope, over);
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

    /// Call `wake` once when the checks end, or at once if they have ended: a window asks for the
    /// one frame the result needs, instead of asking [`Self::is_running`] every frame while they
    /// run. A report with no run (nothing to check, or read from a saved design) wakes nothing.
    pub fn on_end(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        let Some(handle) = &self.0 else { return };
        if !handle.0.set_wake(wake.clone()) {
            wake();
        }
    }

    /// What the checks found, `None` while they run: a `NeedsRepair` issue for each part a check
    /// fails or cannot run on, in the order the parts were read.
    pub fn issues(&self) -> Option<Arc<Vec<ImportIssue>>> {
        match &self.0 {
            None => Some(Arc::default()),
            Some(h) => h.0.settled().map(|s| s.issues),
        }
    }

    /// The bodies whose parts the checks healed, once they have ended (`None` while they run,
    /// when a token stopped them, or when no part was healed).
    pub fn heals(&self) -> Option<Arc<ImportHeals>> {
        self.0.as_ref().and_then(|h| h.0.settled()).and_then(|s| s.heals)
    }

    /// [`Self::heals`], waiting for the checks to end.
    pub fn wait_heals(&self) -> Option<Arc<ImportHeals>> {
        self.0.as_ref().and_then(|h| h.0.wait().heals)
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
