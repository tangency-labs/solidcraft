//! How often SMLib's full check runs while a model is evaluated. An evaluation leaves the checks
//! of the sweeps, booleans and blends a feature runs to one check of each body the feature keeps
//! ([`Validation::EachFeature`], the default), can check every step's result as it is made
//! ([`Validation::EachStep`]), or skips them ([`Validation::Off`]). Every check runs once on the
//! body's decoded record as SMLib imports it (`Brep::from_record(record, true)`), the exact form
//! every later operation receives ([`Body::smlib`], #22138).
//!
//! The evaluation installs its mode around a feature with [`validating`], as [`crate::watch`]
//! installs a watch: the operations a feature runs are reached through hundreds of public kernel
//! functions, and the mode is the evaluation's, not each operation's. Outside [`validating`]
//! (the kernel called directly, a preview, a test) a step does not check its result: the check is
//! a test of the bodies an evaluation keeps, not part of each operation. A caller that wants every
//! step checked asks for it with `validating(Validation::EachStep, ..)`, and [`check_kept`] and
//! [`failing_check`] check a body on request. The worker threads the kernel starts for a step run
//! under the same mode ([`crate::par`]).

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use solidcore_record::brep::Brep as Record;
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
    /// Check it now (under [`Validation::EachStep`]).
    Check,
    /// Leave it to the end of the feature (under [`Validation::EachFeature`]): the body decoded
    /// from the result keeps it ([`Body::left_unchecked`]) for [`check_kept`].
    Defer,
    /// Don't check it (outside [`validating`], and under [`Validation::Off`]).
    Skip,
}

/// What the step about to check its result does. [`Step::Defer`] also tells the feature a check
/// was left for its end.
pub(crate) fn step() -> Step {
    CURRENT.with(|c| match c.borrow().as_deref() {
        Some(Scope { mode: Validation::EachStep, .. }) => Step::Check,
        Some(s @ Scope { mode: Validation::EachFeature, .. }) => {
            s.deferred.store(true, Ordering::Relaxed);
            Step::Defer
        }
        None | Some(Scope { mode: Validation::Off, .. }) => Step::Skip,
    })
}

/// Whether the evaluation skips SMLib's checks ([`Validation::Off`]), for a step that checks its
/// result at once under the other modes. Unlike [`step`], it never leaves a check for the end
/// of the feature.
pub(crate) fn skipped() -> bool {
    CURRENT.with(|c| matches!(c.borrow().as_deref(), Some(Scope { mode: Validation::Off, .. })))
}

/// A step whose check was left to the end of its feature: kept with the body decoded from it (and
/// only for that body's record) until [`check_kept`] or [`detach_kept`] takes it, so the check at
/// the end names the step and any operand that already failed the check.
#[derive(Debug)]
pub(crate) struct Unchecked {
    record: std::sync::Weak<solidcore_record::brep::Brep>,
    /// The step, as its own refusal names it (`SMLib fillet`).
    what: String,
    /// The records of the bodies the step combined (to name one that already fails the check): let
    /// go once the check takes them.
    operands: std::sync::Mutex<Option<Vec<Arc<Record>>>>,
}

impl Unchecked {
    pub(crate) fn new(record: &Arc<Record>, what: &str, operands: &[&Body]) -> Unchecked {
        let operands = operands.iter().map(|b| b.record.arc().clone()).collect();
        Unchecked { record: Arc::downgrade(record), what: what.to_string(), operands: std::sync::Mutex::new(Some(operands)) }
    }

    /// The step's operands, taken (once), when this is `record`'s.
    fn take(&self, record: &Arc<solidcore_record::brep::Brep>) -> Option<Vec<Arc<Record>>> {
        if !std::ptr::eq(self.record.as_ptr(), Arc::as_ptr(record)) {
            return None;
        }
        self.operands.lock().ok().and_then(|mut r| r.take())
    }
}

/// Outcome of checking a decoded record as [`Body::smlib`] imports it
/// (`Brep::from_record(record, true)`).
pub(crate) enum DecodedCheck {
    /// SMLib's full check accepted the body.
    Valid,
    /// SMLib refused the body — either its check failed or `Brep::from_record` rejected the
    /// record itself (for example a vertex or edge tolerance grown past 1 mm).
    Refused(String),
}

impl DecodedCheck {
    fn of(check: solidcraft_smlib::CheckReport) -> Self {
        if check.is_valid() { Self::Valid } else { Self::Refused(check.to_string()) }
    }
}

/// SMLib's full check of a decoded record as [`Body::smlib`] imports it for every later operation
/// (`Brep::from_record(record, true)`): the token over this thread's work stops it
/// ([`solidcraft_smlib::Brep::check_owned`]). Preserves [`KernelError::Internal`] when the
/// solidcore wrapper catches a panic.
pub(crate) fn check_decoded(what: &str, record: &Record) -> Result<DecodedCheck> {
    let brep = match solidcraft_smlib::Brep::from_record(record, true) {
        Ok(b) => b,
        Err(solidcraft_smlib::Error::Record(u)) => return Ok(DecodedCheck::Refused(u.to_string())),
        Err(e) => return Err(KernelError::smlib(format!("{what}: re-import"), e)),
    };
    brep.check_owned(CheckLevel::All).map(DecodedCheck::of).map_err(|e| KernelError::smlib(format!("{what}: check"), e))
}

/// What a step says of a result SMLib's check refuses, naming the operands (the input of a
/// blend) whose own check (`checks`, in order) fails too: the fault is then theirs.
pub(crate) fn refuses(checks: impl ExactSizeIterator<Item = solidcraft_smlib::Result<solidcraft_smlib::CheckReport>>) -> String {
    let n = checks.len();
    let name = |i: usize| match n {
        1 => "the input".to_string(),
        2 => ["the first operand", "the second operand"][i.min(1)].to_string(),
        _ => format!("operand {i}"),
    };
    let failing: Vec<String> = checks
        .enumerate()
        .filter(|(_, c)| matches!(c, Ok(r) if !r.is_valid()) || matches!(c, Err(solidcraft_smlib::Error::Record(_))))
        .map(|(i, _)| name(i))
        .collect();
    let whose = if failing.is_empty() { String::new() } else { format!(" ({} already fails it)", failing.join(" and ")) };
    format!("SMLib's check refuses the result{whose}")
}

/// SMLib's full check of a body a feature keeps, once a step of the feature left its check to
/// the end: an error with the check's reason unless it passes. Checked on the body's decoded
/// record as SMLib imports it for every later operation (`Brep::from_record(record, true)`). A
/// mesh body has nothing to check.
pub fn check_kept(body: &Body) -> Result<()> {
    if body.is_mesh() {
        return Ok(());
    }
    match body.unchecked.as_deref().and_then(|u| u.take(body.record.arc()).map(|o| (u, o))) {
        Some((u, operands)) => verdict(&u.what, check_decoded(&u.what, body.record()), &operands),
        None => verdict("SMLib", body.smlib()?.check(CheckLevel::All).map(DecodedCheck::of).map_err(|e| KernelError::smlib("SMLib: check", e)), &[]),
    }
}

/// What [`check_kept`] makes of SMLib's check of what step `what` made of `operands`.
fn verdict(what: &str, check: Result<DecodedCheck>, operands: &[Arc<Record>]) -> Result<()> {
    match check? {
        DecodedCheck::Valid => Ok(()),
        DecodedCheck::Refused(detail) => {
            let checks = operands.iter().map(|o| solidcraft_smlib::Brep::from_record(o, true).and_then(|s| s.check_owned(CheckLevel::All)));
            Err(KernelError::Failed(format!("{what}: {}: {detail}", refuses(checks))))
        }
    }
}

/// SMLib's check of a body as an operation on it takes it ([`Body::smlib`], at every test): what
/// it fails, or `None` when it passes or is not SMLib's to make (a mesh body, one SMLib refuses
/// outright: [`Body::smlib_refusal`] says why). A check that cannot run is the error why.
///
/// The check runs on a copy of the body's SMLib form, which it owns, so the token over this
/// thread's work stops it ([`solidcraft_smlib::Brep::check_owned`]), as it stops a feature's
/// detached check ([`KeptCheck`]): an import stopped part way stops in the check too.
pub fn failing_check(body: &Body) -> Result<Option<CheckFailure>> {
    if body.is_mesh() || body.smlib_refusal().is_some() {
        return Ok(None);
    }
    let copy = body.smlib()?.try_clone().map_err(|e| KernelError::smlib("SMLib copy", e))?;
    let check = copy.check_owned(solidcraft_smlib::CheckLevel::All).map_err(|e| KernelError::smlib("SMLib's check", e))?;
    Ok(CheckFailure::of(&check))
}

/// What SMLib's check failed in a body: how many tests failed, and each test that failed, where
/// the check reports it (its site: the test's number and name, and the kind of entity it failed
/// on), with how many times. Shown in a line ([`std::fmt::Display`]): `3 failed tests: SmEdge::Test
/// #7 Bad Edge Length: less than edge tolerance (×2); SmLoop::Test #2 Bad Loop Orient`, in the
/// order the check first reported them. Each failure's own values stay in the check's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckFailure {
    /// Failed tests, as the check counts them.
    pub failures: usize,
    /// The tests that failed, each once, in the order the check first reported them.
    pub tests: Vec<FailedTest>,
}

/// One test SMLib's check failed: its site in the check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedTest {
    /// The test as the check names it (`SmEdge::Test #7 Bad Edge Length: less than edge
    /// tolerance`), without the values of each failure.
    pub name: String,
    /// The test's number (`7`), when the check gives one.
    pub number: Option<u32>,
    /// The kinds of entity it failed on (`SmVertex`), each once, when the check names them.
    pub owners: Vec<String>,
    /// How many times it failed.
    pub times: usize,
}

impl CheckFailure {
    /// The failure `check` reports, `None` when it passes.
    pub fn of(check: &solidcraft_smlib::CheckReport) -> Option<CheckFailure> {
        if check.is_valid() {
            return None;
        }
        let mut tests: Vec<FailedTest> = Vec::new();
        // "AssertValid skipped" says why the second half of the check did not run: not a test failed.
        for line in check.text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("AssertValid skipped")) {
            // The test's name ends where its values or its owner start.
            let name =
                [line.find(" ("), line.find(" [owner")].into_iter().flatten().min().map_or(line, |end| line.get(..end).unwrap_or(line)).trim_end();
            let owner = line.rsplit_once("[owner ").and_then(|(_, rest)| rest.strip_suffix(']')).map(str::trim).filter(|o| !o.is_empty());
            let i = match tests.iter().position(|t| t.name == name) {
                Some(i) => i,
                None => {
                    let number = name.split_once("Test #").and_then(|(_, rest)| {
                        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                        digits.parse().ok()
                    });
                    tests.push(FailedTest { name: name.to_string(), number, owners: Vec::new(), times: 0 });
                    tests.len() - 1
                }
            };
            if let Some(t) = tests.get_mut(i) {
                t.times += 1;
                if let Some(owner) = owner.filter(|o| !t.owners.iter().any(|known| known == o)) {
                    t.owners.push(owner.to_string());
                }
            }
        }
        Some(CheckFailure { failures: check.failures, tests })
    }
}

impl std::fmt::Display for CheckFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = if self.failures == 1 { "" } else { "s" };
        write!(f, "{} failed test{s}: ", self.failures)?;
        for (i, t) in self.tests.iter().enumerate() {
            let sep = if i == 0 { "" } else { "; " };
            if t.times > 1 { write!(f, "{sep}{} (×{})", t.name, t.times)? } else { write!(f, "{sep}{}", t.name)? }
        }
        Ok(())
    }
}

/// [`check_kept`] of a body, ready to run on another thread ([`detach_kept`]), under the token
/// of the work that detached it: stopping that work stops the check too.
pub struct KeptCheck(Kept, Option<solidcraft_smlib::stop::Stop>);

enum Kept {
    /// A mesh body: nothing to check.
    Nothing,
    /// A step's decoded record, taken from the body with its operands.
    Step { what: String, record: Arc<Record>, operands: Vec<Arc<Record>> },
    /// A copy of the body's SMLib form, made when the check was detached.
    Copy(Box<Result<solidcraft_smlib::Brep>>),
}

/// [`check_kept`] of `body`, detached from it: the step's record and operands are taken now, or
/// the body's SMLib form is copied now, so the check reads nothing the operations after it use and
/// can run on another thread beside them ([`KeptCheck::run`]).
pub fn detach_kept(body: &Body) -> KeptCheck {
    let stop = solidcraft_smlib::stop::current();
    if body.is_mesh() {
        return KeptCheck(Kept::Nothing, stop);
    }
    if let Some((u, operands)) = body.unchecked.as_deref().and_then(|u| u.take(body.record.arc()).map(|o| (u, o))) {
        return KeptCheck(Kept::Step { what: u.what.clone(), record: body.record.arc().clone(), operands }, stop);
    }
    KeptCheck(Kept::Copy(Box::new(body.smlib().and_then(|b| b.try_clone().map_err(|e| KernelError::smlib("SMLib copy", e))))), stop)
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
            // Both branches own the SMLib form they check: the work's token stops them.
            Kept::Step { what, record, operands } => verdict(&what, check_decoded(&what, &record), &operands),
            Kept::Copy(copy) => {
                verdict("SMLib", (*copy)?.check_owned(CheckLevel::All).map(DecodedCheck::of).map_err(|e| KernelError::smlib("SMLib: check", e)), &[])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_checked_only_under_each_step() {
        assert_eq!(step(), Step::Skip, "outside an evaluation a step does not check its result");
        assert_eq!(validating(Validation::EachStep, step), (Step::Check, false));
        assert_eq!(validating(Validation::EachFeature, step), (Step::Defer, true));
        assert_eq!(validating(Validation::Off, step), (Step::Skip, false));
        // The thread's own mode comes back, also after a nested scope.
        let ((inner, outer), deferred) = validating(Validation::Off, || (validating(Validation::EachStep, step).0, step()));
        assert!(inner == Step::Check && outer == Step::Skip && !deferred);
        assert_eq!(step(), Step::Skip);
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

    /// #17689: [`failing_check`] checks a copy it owns, so a stopped token stops it (an import
    /// stopped part way), and it names each test the check failed, at its site, once.
    #[test]
    fn a_failing_check_stops_with_the_work_and_names_each_failed_test() {
        use solidcore_record::Vec3;
        let good = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let mut record = good.record().clone();
        record.vertices[0].point[2] += 0.5;
        let bad = Body::from_record(record).unwrap();
        let stop = solidcraft_smlib::stop::Stop::new();
        stop.stop();
        let r = stop.within(|| failing_check(&bad));
        assert!(r.as_ref().is_err_and(|e| e.to_string().contains("cancelled")), "{r:?}");
        // Run again with no token: the body's own SMLib form was not touched by the stopped check.
        let failure = failing_check(&bad).unwrap().expect("the moved corner fails the check");
        let report = bad.smlib().unwrap().check(solidcraft_smlib::CheckLevel::All).unwrap();
        assert_eq!(failure.failures, report.failures);
        let lines: Vec<&str> = report.text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("AssertValid skipped")).collect();
        assert_eq!(failure.tests.iter().map(|t| t.times).sum::<usize>(), lines.len(), "{}", report.text);
        for t in &failure.tests {
            assert_eq!(lines.iter().filter(|l| l.starts_with(&t.name)).count(), t.times, "{t:?}");
            assert_eq!(
                t.number.map(|n| n.to_string()),
                t.name.split_once("Test #").map(|(_, r)| r.chars().take_while(char::is_ascii_digit).collect())
            );
            assert!(t.owners.iter().all(|o| lines.iter().any(|l| l.starts_with(&t.name) && l.ends_with(&format!("[owner {o}]")))), "{t:?}");
        }
        assert!(failure.to_string().starts_with(&format!("{} failed test", report.failures)), "{failure}");
        assert_eq!(failing_check(&good).unwrap(), None, "the box passes");
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
        // The checks took the operands' records with the results: the bodies kept don't hold them.
        assert_eq!(Arc::strong_count(bad.record.arc()), 1);
        // A round on an edge away from the moved corner: the blend names its input the same way.
        let edge = [Vec3::new(10.0, 5.0, 10.0)];
        let (step, _) = validating(Validation::EachStep, || crate::fillet(&bad, &edge, 1.0));
        let e = step.unwrap_err().to_string();
        assert!(e.contains("SMLib's check refuses the result (the input already fails it)"), "{e}");
        let (round, deferred) = validating(Validation::EachFeature, || crate::fillet(&bad, &edge, 1.0));
        let e = check_kept(&round.unwrap()).unwrap_err().to_string();
        assert!(deferred && e.contains("SMLib fillet: SMLib's check refuses the result (the input already fails it)"), "{e}");
        let (off, deferred) = validating(Validation::Off, || crate::boolean(&bad, &tool, crate::BoolOp::Union));
        assert!(off.is_ok_and(|b| b.is_some()) && !deferred);
    }

    /// The SMLib steps of shell, offset faces, draft and thicken (`crate::offset_backend::run`)
    /// follow the mode as a boolean does (#17601). Offset Faces first heals the body through it:
    /// on a body SMLib's check already refuses, that step is refused there under each_step,
    /// saying the body failed it before; left to the end under each_feature, where the kept
    /// body's check refuses it; and built under off.
    #[test]
    fn the_offset_steps_on_a_body_the_check_refuses_follow_the_mode() {
        use solidcore_record::Vec3;
        let good = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let mut record = good.record().clone();
        record.vertices[0].point[2] += 0.05;
        // The side across the box from the moved corner, which stays flat.
        let far_x = if record.vertices[0].point[0] < 5.0 { 10.0 } else { 0.0 };
        let bad = Body::from_record(record).unwrap();
        assert!(check_kept(&bad).is_err());
        let heal = |b: &Body| crate::heal::heal(b, &[]);
        let (step, deferred) = validating(Validation::EachStep, || heal(&bad));
        let e = step.unwrap_err().to_string();
        assert!(e.contains("SMLib healing: SMLib's check refuses the result (the body already fails it)") && !deferred, "{e}");
        let (healed, deferred) = validating(Validation::EachFeature, || heal(&bad));
        assert!(deferred);
        let e = check_kept(&healed.unwrap()).unwrap_err().to_string();
        assert!(e.contains("SMLib healing: SMLib's check refuses the result"), "{e}");
        let (healed, deferred) = validating(Validation::Off, || heal(&bad));
        assert!(healed.is_ok() && !deferred, "{healed:?}");
        // Offset Faces under off is no longer refused by the check (SMLib's own operation may
        // still refuse a body this far off).
        let side = [Vec3::new(far_x, 5.0, 5.0)];
        let (off, _) = validating(Validation::Off, || crate::offset_faces(&bad, &side, 1.0));
        assert!(off.as_ref().err().is_none_or(|e| !e.to_string().contains("SMLib's check refuses")), "{off:?}");
        // A body the check accepts is moved under every mode, and passes the check kept.
        for mode in Validation::ALL {
            let (moved, _) = validating(mode, || crate::offset_faces(&good, &side, 1.0));
            assert!(moved.as_ref().is_ok_and(|m| check_kept(m).is_ok()), "{mode:?}: {moved:?}");
        }
    }

    /// Surface Offset skips SMLib's check of each offset face and of their stitch under off, as
    /// Offset Faces does (#17601). A box edge stored as a cubic that runs a, b, a, b along its
    /// line stops half way (zero speed there, as some imported trims nearly do): SMLib's check
    /// refuses the box, and the offset of a face beside that edge inherits the defect. The offset
    /// is refused under each_step and each_feature, which still check at the step, and made
    /// under off.
    #[test]
    fn surface_offset_of_a_face_the_check_refuses_is_made_under_off() {
        use solidcore_record::Vec3;
        use solidcore_record::brep::Curve;
        let good = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let mut record = good.record().clone();
        let edge = &mut record.edges[0];
        let Curve::Line { start, end } = edge.curve else { panic!("a box edge is a line: {:?}", edge.curve) };
        let at = |t: f64| std::array::from_fn::<f64, 3, _>(|i| start[i] + t * (end[i] - start[i]));
        let [t0, t1] = edge.interval;
        let (a, b) = (at(t0), at(t1));
        edge.curve = Curve::Spline { knots: vec![t0, t0, t0, t0, t1, t1, t1, t1], points: vec![a, b, a, b] };
        let face = edge.uses[0].face as usize;
        let bad = Body::from_record(record).unwrap();
        let e = check_kept(&bad).unwrap_err().to_string();
        assert!(e.contains("Bad SpeedPt"), "{e}");
        let offset = |b: &Body| crate::offset_surface(b, &[face], 1.0, 0.01);
        for mode in [Validation::EachStep, Validation::EachFeature] {
            let (refused, deferred) = validating(mode, || offset(&bad));
            let e = refused.unwrap_err().to_string();
            assert!(e.contains("SMLib offset: SMLib's check refuses the result") && e.contains("Bad SpeedPt") && !deferred, "{mode:?}: {e}");
        }
        let (off, deferred) = validating(Validation::Off, || offset(&bad));
        let off = off.unwrap();
        assert!(off.is_surface() && off.face_count() == 1 && !deferred, "{off:?}");
        let m = crate::measure(&off).unwrap();
        assert!((m.area - 100.0).abs() < 1e-6, "{m:?}");
        // A face the check accepts is offset under every mode.
        for mode in Validation::ALL {
            let (moved, _) = validating(mode, || offset(&good));
            assert!(moved.as_ref().is_ok_and(|m| m.face_count() == 1 && check_kept(m).is_ok()), "{mode:?}: {moved:?}");
        }
    }

    /// #22138: a step whose decoded record fails SMLib's check on re-import is refused by
    /// [`check_kept`] and [`detach_kept`] without blaming a valid operand, and stopping a detached
    /// step check cancels it.
    #[test]
    fn kept_check_refuses_a_steps_invalid_decoded_record() {
        use solidcore_record::Vec3;
        let good = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let mut record = good.record().clone();
        record.vertices[0].point[2] += 0.5;
        let bad = Body::from_record(record).unwrap();

        let bad_inline = bad.clone().left_unchecked("SMLib chamfer", &[&good]);
        let e = check_kept(&bad_inline).unwrap_err().to_string();
        assert!(e.contains("SMLib chamfer: SMLib's check refuses the result") && !e.contains("already fails it"), "{e}");

        let bad_detached = bad.clone().left_unchecked("SMLib chamfer", &[&good]);
        let e = detach_kept(&bad_detached).run().unwrap_err().to_string();
        assert!(e.contains("SMLib chamfer: SMLib's check refuses the result") && !e.contains("already fails it"), "{e}");

        // Stopping a detached step check cancels it.
        let stop = solidcraft_smlib::stop::Stop::new();
        let good_step = good.clone().left_unchecked("SMLib chamfer", &[]);
        let detached = stop.within(|| detach_kept(&good_step));
        stop.stop();
        let r = crate::par::kernel_thread("check").spawn(move || detached.run()).unwrap().join().unwrap();
        assert!(r.as_ref().is_err_and(|e| e.to_string().contains("cancelled")), "{r:?}");
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
