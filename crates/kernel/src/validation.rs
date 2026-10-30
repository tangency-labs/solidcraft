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
    record: std::sync::Weak<solidcraft_geom::brep::Brep>,
    /// The step, as its own refusal names it (`SMLib fillet`).
    what: String,
    how: Checked,
    result: std::sync::Mutex<Option<solidcraft_smlib::Brep>>,
}

impl Unchecked {
    pub(crate) fn new(record: &Arc<solidcraft_geom::brep::Brep>, what: &str, how: Checked, result: solidcraft_smlib::Brep) -> Unchecked {
        Unchecked { record: Arc::downgrade(record), what: what.to_string(), how, result: std::sync::Mutex::new(Some(result)) }
    }

    /// The step's result, taken (once), when this is `record`'s.
    fn take(&self, record: &Arc<solidcraft_geom::brep::Brep>) -> Option<solidcraft_smlib::Brep> {
        if !std::ptr::eq(self.record.as_ptr(), Arc::as_ptr(record)) {
            return None;
        }
        self.result.lock().ok().and_then(|mut r| r.take())
    }
}

/// SMLib's full check of `r` as a step takes it (`how`): the check's report.
pub(crate) fn check_result(r: &solidcraft_smlib::Brep, how: Checked) -> std::result::Result<solidcraft_smlib::CheckReport, String> {
    fn failed(what: &'static str) -> impl Fn(&dyn std::fmt::Display) -> String {
        move |e| format!("{what}: {e}")
    }
    match how {
        Checked::Combined if !r.is_manifold_solid().map_err(|e| failed("check")(&e))? => {
            let record = r.to_native().map_err(|e| failed("export")(&e))?;
            let split = solidcraft_smlib::Brep::from_record(&record, true).map_err(|e| failed("the separate solids")(&e))?;
            split.check(solidcraft_smlib::CheckLevel::All).map_err(|e| failed("check")(&e))
        }
        _ => r.check(solidcraft_smlib::CheckLevel::All).map_err(|e| failed("check")(&e)),
    }
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
    let step = body.unchecked.as_deref().and_then(|u| u.take(&body.record).map(|r| (u, r)));
    let (what, check) = match &step {
        Some((u, r)) => (u.what.as_str(), check_result(r, u.how)),
        None => ("SMLib", body.smlib()?.check(solidcraft_smlib::CheckLevel::All).map_err(|e| format!("check: {e}"))),
    };
    match check {
        Ok(c) if c.is_valid() => Ok(()),
        Ok(c) => Err(KernelError::Failed(format!("{what}: SMLib's check refuses the result: {c}"))),
        Err(e) => Err(KernelError::Failed(format!("{what}: {e}"))),
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

    #[test]
    fn the_mode_reaches_the_kernels_worker_threads() {
        let items = [0, 1, 2, 3];
        let (checked, deferred) = validating(Validation::EachFeature, || {
            crate::par::map("validation", &items, true, |_| Ok(step())).into_iter().collect::<Result<Vec<Step>>>()
        });
        assert_eq!(checked.ok(), Some(vec![Step::Defer; 4]));
        assert!(deferred);
        let (on_threads, deferred) = validating(Validation::EachFeature, || crate::par::map_on_threads(&items, 4, |_| step()));
        assert_eq!(on_threads, vec![Some(Step::Defer); 4]);
        assert!(deferred);
    }

    /// A box with one corner moved off its three edges: SMLib's check refuses it. Under each_step
    /// the boolean that takes it refuses its result there; under each_feature it is not checked,
    /// says the check was left for later, and the check of the body kept refuses it.
    #[cfg(feature = "smlib")]
    #[test]
    fn a_body_the_check_refuses_is_refused_at_the_step_or_at_the_end() {
        use solidcraft_geom::Vec3;
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
        assert!(check_kept(&joined).unwrap_err().to_string().contains("SMLib's check refuses the result"));
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
