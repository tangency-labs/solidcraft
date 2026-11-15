//! How often SMLib's full check runs while a model is evaluated. Every sweep, boolean and blend
//! checks its result in SMLib before it is decoded ([`Validation::EachStep`]); an evaluation can
//! instead leave those checks to one check of the bodies each feature keeps
//! ([`Validation::EachFeature`]), or skip them ([`Validation::Off`]). A body kept straight from
//! a step is checked at the end as that step would have checked it, on the step's own result
//! (kept with the body until then): the record a result is decoded to can pass SMLib's check
//! where the result itself fails it (a surface of revolution's polar curve, rebuilt on import).
//!
//! The evaluation installs its mode around a feature with [`validating`], as [`crate::watch`]
//! installs a watch: the operations a feature runs are reached through hundreds of public kernel
//! functions, and the mode is the evaluation's, not each operation's. Outside [`validating`]
//! (the kernel called directly, a preview, a test) every step is checked, as it always was. The
//! worker threads the kernel starts for a step run under the same mode ([`crate::par`]).

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use solidcraft_smlib::CheckLevel;

use crate::body::Body;
use crate::{KernelError, Result};

/// When SMLib's full check of a result runs during a model evaluation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Validation {
    /// After every sweep, boolean and blend: a step whose result fails the check fails the
    /// feature there (how every evaluation checked before the mode existed).
    EachStep,
    /// Once per feature: the steps inside a feature are not checked, and each body the feature
    /// makes or changes is checked before it is kept, so a feature that makes a body the check
    /// refuses still fails, with the check's reason.
    #[default]
    EachFeature,
    /// Never (experimental): a body the check would refuse is kept.
    Off,
}

impl Validation {
    pub const ALL: [Validation; 3] = [Validation::EachStep, Validation::EachFeature, Validation::Off];

    /// The mode's name: `each_step`, `each_feature` or `off` (as it is saved and sent).
    pub fn id(self) -> &'static str {
        match self {
            Validation::EachStep => "each_step",
            Validation::EachFeature => "each_feature",
            Validation::Off => "off",
        }
    }

    /// The mode named `id` ([`Validation::id`]), `None` for anything else.
    pub fn parse(id: &str) -> Option<Validation> {
        Validation::ALL.into_iter().find(|v| v.id() == id)
    }
}

/// One feature's work under a mode, shared with the worker threads it starts.
#[derive(Debug)]
pub(crate) struct Scope {
    mode: Validation,
    /// A step's check was left for the end of the feature ([`Validation::EachFeature`]).
    deferred: AtomicBool,
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Scope>>> = const { RefCell::new(None) };
}

/// Run `f` with `mode` deciding which checks the kernel work it does on this thread runs. Also
/// gives whether a step's check was left for later under [`Validation::EachFeature`]: the
/// caller then checks the bodies it keeps ([`check_kept`]).
pub fn validating<R>(mode: Validation, f: impl FnOnce() -> R) -> (R, bool) {
    let scope = Arc::new(Scope { mode, deferred: AtomicBool::new(false) });
    let r = within(Some(scope.clone()), f);
    (r, scope.deferred.load(Ordering::Relaxed))
}

/// The scope this thread's work runs in, to hand to the worker threads it starts.
pub(crate) fn current() -> Option<Arc<Scope>> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Run `f` in `scope` (one [`current`] gave on another thread), restoring this thread's own
/// afterwards, also when `f` panics.
pub(crate) fn within<R>(scope: Option<Arc<Scope>>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Option<Arc<Scope>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                CURRENT.with(|c| *c.borrow_mut() = previous);
            }
        }
    }
    let previous = CURRENT.with(|c| c.replace(scope));
    let _restore = Restore(Some(previous));
    f()
}

/// What a step does with SMLib's check of its result ([`step`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Check it now (outside [`validating`], and under [`Validation::EachStep`]).
    Check,
    /// Leave it to the end of the feature (under [`Validation::EachFeature`]): the body decoded
    /// from the result keeps it ([`Body::left_unchecked`]) for [`check_kept`].
    Defer,
    /// Never check it (under [`Validation::Off`]).
    Skip,
}

/// What the step about to check its result does. [`Step::Defer`] also tells the feature a check
/// was left for its end.
pub(crate) fn step() -> Step {
    CURRENT.with(|c| match c.borrow().as_deref() {
        None | Some(Scope { mode: Validation::EachStep, .. }) => Step::Check,
        Some(s @ Scope { mode: Validation::EachFeature, .. }) => {
            s.deferred.store(true, Ordering::Relaxed);
            Step::Defer
        }
        Some(Scope { mode: Validation::Off, .. }) => Step::Skip,
    })
}

/// How a step's own check takes its result.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Checked {
    /// As it is.
    AsIs,
    /// A boolean's: solids touching each other along an edge or at a vertex come back as one
    /// non-manifold body, checked as the separate solids its export holds (the boolean's rule).
    Combined,
}

/// A step's SMLib result whose check was left to the end of its feature: kept with the body
/// decoded from it (and only for that body's record) until [`check_kept`] takes it, so the check
/// at the end is the step's own, of the result the step made.
#[derive(Debug)]
pub(crate) struct Unchecked {
    record: std::sync::Weak<solidcore_record::brep::Brep>,
    /// The step, as its own refusal names it (`SMLib fillet`).
    what: String,
    how: Checked,
    result: std::sync::Mutex<Option<solidcraft_smlib::Brep>>,
    /// The records of the bodies the step combined, to name one that already fails the check.
    operands: Vec<Arc<solidcore_record::brep::Brep>>,
}

impl Unchecked {
    pub(crate) fn new(record: &Arc<solidcore_record::brep::Brep>, what: &str, how: Checked, result: solidcraft_smlib::Brep, operands: &[&Body]) -> Unchecked {
        let operands = operands.iter().map(|b| b.record.arc().clone()).collect();
        Unchecked { record: Arc::downgrade(record), what: what.to_string(), how, result: std::sync::Mutex::new(Some(result)), operands }
    }

    /// The step's result, taken (once), when this is `record`'s.
    fn take(&self, record: &Arc<solidcore_record::brep::Brep>) -> Option<solidcraft_smlib::Brep> {
        if !std::ptr::eq(self.record.as_ptr(), Arc::as_ptr(record)) {
            return None;
        }
        self.result.lock().ok().and_then(|mut r| r.take())
    }
}

/// SMLib's full check of `r` as a step takes it (`how`): the check's report.
pub(crate) fn check_result(r: &solidcraft_smlib::Brep, how: Checked) -> std::result::Result<solidcraft_smlib::CheckReport, String> {
    let check = match separate(r, how)? {
        Some(record) => solidcraft_smlib::Brep::from_record(&record, true).map_err(|e| format!("the separate solids: {e}"))?.check(CheckLevel::All),
        None => r.check(CheckLevel::All),
    };
    check.map_err(|e| format!("check: {e}"))
}

/// The record of the separate solids a boolean's result `r` exports to where it is not one
/// manifold solid ([`Checked::Combined`]), which its check takes; `None`: `r` is checked as it is.
fn separate(r: &solidcraft_smlib::Brep, how: Checked) -> std::result::Result<Option<solidcore_record::brep::Brep>, String> {
    let split = matches!(how, Checked::Combined) && !r.is_manifold_solid().map_err(|e| format!("check: {e}"))?;
    split.then(|| r.to_native().map_err(|e| format!("export: {e}"))).transpose()
}

/// What a step says of a result SMLib's check refuses, naming the operands whose own check
/// (`checks`, in order) fails too: the fault is then theirs.
pub(crate) fn refuses(checks: impl ExactSizeIterator<Item = solidcraft_smlib::Result<solidcraft_smlib::CheckReport>>) -> String {
    let n = checks.len();
    let name = |i| match (n, i) {
        (2, 0) => "the first operand".to_string(),
        (2, _) => "the second operand".to_string(),
        _ => format!("operand {i}"),
    };
    let failing: Vec<String> = checks.enumerate().filter(|(_, c)| c.as_ref().is_ok_and(|c| !c.is_valid())).map(|(i, _)| name(i)).collect();
    let whose = if failing.is_empty() { String::new() } else { format!(" ({} already fails it)", failing.join(" and ")) };
    format!("SMLib's check refuses the result{whose}")
}

/// SMLib's full check of a body a feature keeps, once a step of the feature left its check to
/// the end: an error with the check's reason unless it passes. A body decoded from a step's
/// result is checked as that step would have checked it, on its result; any other (a copy moved
/// into place, pieces joined without an operation) as SMLib takes it for the operations after it.
/// A mesh body has nothing to check.
pub fn check_kept(body: &Body) -> Result<()> {
    if body.is_mesh() {
        return Ok(());
    }
    match body.unchecked.as_deref().and_then(|u| u.take(body.record.arc()).map(|r| (u, r))) {
        Some((u, r)) => verdict(&u.what, check_result_owned(r, u.how), &u.operands),
        None => verdict("SMLib", body.smlib()?.check(CheckLevel::All).map_err(|e| format!("check: {e}")), &[]),
    }
}

/// What [`check_kept`] makes of SMLib's check of what step `what` made of `operands`.
fn verdict(what: &str, check: std::result::Result<solidcraft_smlib::CheckReport, String>, operands: &[Arc<solidcore_record::brep::Brep>]) -> Result<()> {
    match check {
        Ok(c) if c.is_valid() => Ok(()),
        Ok(c) => {
            let checks = operands.iter().map(|o| solidcraft_smlib::Brep::from_record(o, true).and_then(|s| s.check_owned(CheckLevel::All)));
            Err(KernelError::Failed(format!("{what}: {}: {c}", refuses(checks))))
        }
        Err(e) => Err(KernelError::Failed(format!("{what}: {e}"))),
    }
}

/// [`check_result`] of a step's result nobody else reads, which it takes: the token over this
/// thread's work stops it ([`solidcraft_smlib::Brep::check_owned`]).
fn check_result_owned(r: solidcraft_smlib::Brep, how: Checked) -> std::result::Result<solidcraft_smlib::CheckReport, String> {
    let check = match separate(&r, how)? {
        Some(record) => {
            drop(r);
            solidcraft_smlib::Brep::from_record(&record, true).map_err(|e| format!("the separate solids: {e}"))?.check_owned(CheckLevel::All)
        }
        None => r.check_owned(CheckLevel::All),
    };
    check.map_err(|e| format!("check: {e}"))
}

/// [`check_kept`] of a body, ready to run on another thread ([`detach_kept`]), under the token
/// of the work that detached it: stopping that work stops the check too.
pub struct KeptCheck(Kept, Option<solidcraft_smlib::stop::Stop>);

enum Kept {
    /// A mesh body: nothing to check.
    Nothing,
    /// The step's own result, taken from the body.
    Step { what: String, how: Checked, result: solidcraft_smlib::Brep, operands: Vec<Arc<solidcore_record::brep::Brep>> },
    /// A copy of the body's SMLib form, made when the check was detached.
    Copy(Result<solidcraft_smlib::Brep>),
}

/// [`check_kept`] of `body`, detached from it: the step's result is taken now, or the body's SMLib
/// form is copied now, so the check reads nothing the operations after it use and can run on
/// another thread beside them ([`KeptCheck::run`]).
pub fn detach_kept(body: &Body) -> KeptCheck {
    let stop = solidcraft_smlib::stop::current();
    if body.is_mesh() {
        return KeptCheck(Kept::Nothing, stop);
    }
    if let Some((u, result)) = body.unchecked.as_deref().and_then(|u| u.take(body.record.arc()).map(|r| (u, r))) {
        return KeptCheck(Kept::Step { what: u.what.clone(), how: u.how, result, operands: u.operands.clone() }, stop);
    }
    KeptCheck(Kept::Copy(body.smlib().and_then(|b| b.try_clone().map_err(|e| KernelError::smlib("SMLib copy", e)))), stop)
}

impl KeptCheck {
    /// The check: [`check_kept`]'s result for the body it was detached from.
    pub fn run(self) -> Result<()> {
        let KeptCheck(kept, stop) = self;
        solidcraft_smlib::stop::scoped(stop, || kept.run())
    }
}

impl Kept {
    fn run(self) -> Result<()> {
        match self {
            Kept::Nothing => Ok(()),
            // The step's result and the copy are the check's own: the work's token stops it.
            Kept::Step { what, how, result, operands } => verdict(&what, check_result_owned(result, how), &operands),
            Kept::Copy(copy) => verdict("SMLib", copy?.check_owned(CheckLevel::All).map_err(|e| format!("check: {e}")), &[]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_checked_outside_an_evaluation_and_under_each_step() {
        assert_eq!(step(), Step::Check);
        assert_eq!(validating(Validation::EachStep, step), (Step::Check, false));
        assert_eq!(validating(Validation::EachFeature, step), (Step::Defer, true));
        assert_eq!(validating(Validation::Off, step), (Step::Skip, false));
        // The thread's own mode comes back, also after a nested scope.
        let ((inner, outer), deferred) = validating(Validation::Off, || (validating(Validation::EachStep, step).0, step()));
        assert!(inner == Step::Check && outer == Step::Skip && !deferred);
        assert_eq!(step(), Step::Check);
    }

    /// A check detached under a token runs under it on whatever thread runs it (the feature
    /// check beside the evaluation): stopping the work stops the check (solidcore#3247).
    #[test]
    fn a_detached_check_stops_with_the_work_that_detached_it() {
        let body = crate::cylinder(solidcore_record::Vec3::new(0.0, 0.0, 0.0), solidcore_record::Vec3::Z, 3.0, 10.0).unwrap();
        let stop = solidcraft_smlib::stop::Stop::new();
        let detached = stop.within(|| detach_kept(&body));
        stop.stop();
        let r = crate::par::kernel_thread("check").spawn(move || detached.run()).unwrap().join().unwrap();
        assert!(r.as_ref().is_err_and(|e| e.to_string().contains("cancelled")), "{r:?}");
        let free = solidcraft_smlib::stop::Stop::new();
        let detached = free.within(|| detach_kept(&body));
        assert!(detached.run().is_ok(), "a token never stopped changes nothing");
    }

    #[test]
    fn the_mode_reaches_the_kernels_worker_threads() {
        let items = [0, 1, 2, 3];
        let (on_threads, deferred) = validating(Validation::EachFeature, || crate::par::map_on_threads(&items, 4, |_| step()));
        assert_eq!(on_threads, vec![Some(Step::Defer); 4]);
        assert!(deferred);
    }

    /// A box with one corner moved off its three edges: SMLib's check refuses it. Under each_step
    /// the boolean that takes it refuses its result there; under each_feature it is not checked,
    /// says the check was left for later, and the check of the body kept refuses it. Both name
    /// the operand that already fails the check (#17230), also when the check runs detached.
    #[test]
    fn a_body_the_check_refuses_is_refused_at_the_step_or_at_the_end() {
        use solidcore_record::Vec3;
        let good = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
        assert!(check_kept(&good).is_ok());
        let mut record = good.record().clone();
        record.vertices[0].point[2] += 0.5;
        let bad = Body::from_record(record).unwrap();
        let e = check_kept(&bad).unwrap_err().to_string();
        assert!(e.contains("SMLib's check refuses the result"), "{e}");
        let tool = crate::box_solid(Vec3::new(5.0, 5.0, 5.0), Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let (step, deferred) = validating(Validation::EachStep, || crate::boolean(&bad, &tool, crate::BoolOp::Union));
        let e = step.unwrap_err().to_string();
        assert!(e.contains("SMLib's check refuses the result (the first operand already fails it)") && !deferred, "{e}");
        let (joined, deferred) = validating(Validation::EachFeature, || crate::boolean(&bad, &tool, crate::BoolOp::Union));
        assert!(deferred);
        let joined = joined.unwrap().unwrap();
        let detached = detach_kept(&joined).run().unwrap_err().to_string();
        assert!(detached.contains("SMLib boolean: SMLib's check refuses the result (the first operand already fails it)"), "{detached}");
        let (joined, _) = validating(Validation::EachFeature, || crate::boolean(&bad, &tool, crate::BoolOp::Union));
        let e = check_kept(&joined.unwrap().unwrap()).unwrap_err().to_string();
        assert!(e.contains("SMLib boolean: SMLib's check refuses the result (the first operand already fails it)"), "{e}");
        // Only the operand that fails is named.
        let (joined, _) = validating(Validation::EachFeature, || crate::boolean(&tool, &bad, crate::BoolOp::Union));
        let e = check_kept(&joined.unwrap().unwrap()).unwrap_err().to_string();
        assert!(e.contains("(the second operand already fails it)"), "{e}");
        let (off, deferred) = validating(Validation::Off, || crate::boolean(&bad, &tool, crate::BoolOp::Union));
        assert!(off.is_ok_and(|b| b.is_some()) && !deferred);
    }

    #[test]
    fn modes_are_named_as_they_are_saved() {
        for v in Validation::ALL {
            assert_eq!(Validation::parse(v.id()), Some(v));
            assert_eq!(serde_json::to_value(v).ok(), Some(serde_json::json!(v.id())));
        }
        assert_eq!(Validation::parse("each step"), None);
        assert_eq!(Validation::default(), Validation::EachFeature);
    }
}
