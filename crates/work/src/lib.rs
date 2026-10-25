//! Deterministic work accounting for commands and frames, without wall-clock thresholds.
//!
//! [`record`] and [`Context`] are no-ops without the `capture` feature; normal builds have no
//! collector, thread-local storage or clocks. Tests enable the feature as a dev-dependency.
//! Captures are inclusive: a nested capture's work belongs to its enclosing capture too.
//! Workers must explicitly inherit [`Context::current`] and finish before a report is used as
//! a budget. A report with an outstanding worker context or lost counts is incomplete.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// A unit of actual work, recorded where it is performed rather than where it is requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Counter {
    ModelEvaluations,
    FeatureFingerprints,
    FingerprintBytesHashed,
    FeatureJsonBytes,
    ImportDigests,
    ImportedBytesHashed,
    FeatureEvaluations,
    BodyTransforms,
    ReferenceMeshBuilds,
    DisplayPlanBuilds,
    DisplayMeshBuilds,
    MeshTransforms,
    DisplayPlanTransforms,
    JointSolves,
    JointMobilityAnalyses,
    JointPlacementSteps,
    JointPlacementInserts,
    JointFaceMeasurements,
    MotionTraceSamples,
    SketchSolves,
    SketchAnalyses,
    ModelSceneBuilds,
    DisplayTriangleBytes,
    NamePreparationJobs,
    NamePreparations,
}

impl Counter {
    pub const ALL: [Self; 25] = [
        Self::ModelEvaluations,
        Self::FeatureFingerprints,
        Self::FingerprintBytesHashed,
        Self::FeatureJsonBytes,
        Self::ImportDigests,
        Self::ImportedBytesHashed,
        Self::FeatureEvaluations,
        Self::BodyTransforms,
        Self::ReferenceMeshBuilds,
        Self::DisplayPlanBuilds,
        Self::DisplayMeshBuilds,
        Self::MeshTransforms,
        Self::DisplayPlanTransforms,
        Self::JointSolves,
        Self::JointMobilityAnalyses,
        Self::JointPlacementSteps,
        Self::JointPlacementInserts,
        Self::JointFaceMeasurements,
        Self::MotionTraceSamples,
        Self::SketchSolves,
        Self::SketchAnalyses,
        Self::ModelSceneBuilds,
        Self::DisplayTriangleBytes,
        Self::NamePreparationJobs,
        Self::NamePreparations,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ModelEvaluations => "model_evaluations",
            Self::FeatureFingerprints => "feature_fingerprints",
            Self::FingerprintBytesHashed => "fingerprint_bytes_hashed",
            Self::FeatureJsonBytes => "feature_json_bytes",
            Self::ImportDigests => "import_digests",
            Self::ImportedBytesHashed => "imported_bytes_hashed",
            Self::FeatureEvaluations => "feature_evaluations",
            Self::BodyTransforms => "body_transforms",
            Self::ReferenceMeshBuilds => "reference_mesh_builds",
            Self::DisplayPlanBuilds => "display_plan_builds",
            Self::DisplayMeshBuilds => "display_mesh_builds",
            Self::MeshTransforms => "mesh_transforms",
            Self::DisplayPlanTransforms => "display_plan_transforms",
            Self::JointSolves => "joint_solves",
            Self::JointMobilityAnalyses => "joint_mobility_analyses",
            Self::JointPlacementSteps => "joint_placement_steps",
            Self::JointPlacementInserts => "joint_placement_inserts",
            Self::JointFaceMeasurements => "joint_face_measurements",
            Self::MotionTraceSamples => "motion_trace_samples",
            Self::SketchSolves => "sketch_solves",
            Self::SketchAnalyses => "sketch_analyses",
            Self::ModelSceneBuilds => "model_scene_builds",
            Self::DisplayTriangleBytes => "display_triangle_bytes",
            Self::NamePreparationJobs => "name_preparation_jobs",
            Self::NamePreparations => "name_preparations",
        }
    }
}

/// Fixed-size work totals. Check [`Counts::complete`] before asserting a work budget.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    values: [u64; Counter::ALL.len()],
    complete: bool,
}

impl Counts {
    pub fn get(&self, counter: Counter) -> u64 {
        self.values.get(counter as usize).copied().unwrap_or(0)
    }

    /// False if collection overflowed, exceeded the nesting limit or still has worker contexts.
    pub fn complete(&self) -> bool {
        self.complete
    }

    pub fn iter(&self) -> impl Iterator<Item = (Counter, u64)> + '_ {
        Counter::ALL.into_iter().map(|counter| (counter, self.get(counter)))
    }

    #[cfg(feature = "capture")]
    fn add(&mut self, counter: Counter, amount: u64) {
        if let Some(value) = self.values.get_mut(counter as usize) {
            let (sum, overflow) = value.overflowing_add(amount);
            *value = if overflow { u64::MAX } else { sum };
            self.complete &= !overflow;
        } else {
            self.complete = false;
        }
    }
}

impl std::fmt::Debug for Counts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut fields = f.debug_struct("Work");
        fields.field("complete", &self.complete);
        for (counter, value) in self.iter() {
            fields.field(counter.name(), &value);
        }
        fields.finish()
    }
}

/// Record units already available at a work site. Do not calculate data solely to count it.
#[inline]
pub fn record(counter: Counter, amount: u64) {
    #[cfg(feature = "capture")]
    enabled::record(counter, amount);
    #[cfg(not(feature = "capture"))]
    let _ = (counter, amount);
}

/// An explicit handoff to joined workers. A clone kept after capture makes its report incomplete.
#[derive(Clone, Default)]
pub struct Context {
    #[cfg(feature = "capture")]
    scopes: Vec<enabled::Scope>,
}

impl Context {
    #[inline]
    pub fn current() -> Self {
        #[cfg(feature = "capture")]
        {
            enabled::context()
        }
        #[cfg(not(feature = "capture"))]
        {
            Self::default()
        }
    }

    /// Inherit this context while running `f`, without double-counting scopes already entered.
    #[inline]
    pub fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        #[cfg(feature = "capture")]
        {
            enabled::run(self, f)
        }
        #[cfg(not(feature = "capture"))]
        {
            f()
        }
    }
}

/// Run an operation, including nested captures and explicitly joined workers, and return its work.
/// Available only with `capture`, so a disabled collector cannot masquerade as zero work.
#[cfg(feature = "capture")]
pub fn capture<T>(f: impl FnOnce() -> T) -> (T, Counts) {
    enabled::capture(f)
}

#[cfg(feature = "capture")]
mod enabled {
    use super::{Context, Counter, Counts};
    use std::cell::Cell;
    use std::sync::{Arc, Mutex, MutexGuard};

    pub(super) type Scope = Arc<Mutex<Counts>>;
    const MAX_DEPTH: usize = 16;

    thread_local! {
        static ACTIVE: Cell<Vec<Scope>> = const { Cell::new(Vec::new()) };
    }

    fn counts(scope: &Scope) -> MutexGuard<'_, Counts> {
        match scope.lock() {
            Ok(counts) => counts,
            Err(poisoned) => {
                let mut counts = poisoned.into_inner();
                counts.complete = false;
                counts
            }
        }
    }

    struct Restore(Vec<Scope>);

    impl Drop for Restore {
        fn drop(&mut self) {
            let mut previous = Some(std::mem::take(&mut self.0));
            if ACTIVE.try_with(|active| active.set(previous.take().unwrap_or_default())).is_err() {
                for scope in previous.unwrap_or_default() {
                    counts(&scope).complete = false;
                }
            }
        }
    }

    pub(super) fn record(counter: Counter, amount: u64) {
        if amount == 0 {
            return;
        }
        let _ = ACTIVE.try_with(|active| {
            let scopes = active.take();
            for scope in &scopes {
                counts(scope).add(counter, amount);
            }
            active.set(scopes);
        });
    }

    pub(super) fn context() -> Context {
        ACTIVE
            .try_with(|active| {
                let scopes = active.take();
                let context = Context { scopes: scopes.clone() };
                active.set(scopes);
                context
            })
            .unwrap_or_default()
    }

    pub(super) fn run<T>(context: &Context, f: impl FnOnce() -> T) -> T {
        let entered = ACTIVE.try_with(|active| {
            let previous = active.take();
            let mut scopes = previous.clone();
            for scope in &context.scopes {
                if scopes.iter().any(|active| Arc::ptr_eq(active, scope)) {
                    continue;
                }
                if scopes.len() < MAX_DEPTH {
                    scopes.push(scope.clone());
                } else {
                    counts(scope).complete = false;
                    for active in &scopes {
                        counts(active).complete = false;
                    }
                }
            }
            active.set(scopes);
            previous
        });
        let Ok(previous) = entered else {
            for scope in &context.scopes {
                counts(scope).complete = false;
            }
            return f();
        };
        let restore = Restore(previous);
        let value = f();
        drop(restore);
        value
    }

    pub(super) fn capture<T>(f: impl FnOnce() -> T) -> (T, Counts) {
        let scope = Arc::new(Mutex::new(Counts { complete: true, ..Counts::default() }));
        let context = Context { scopes: vec![scope.clone()] };
        let value = context.run(f);
        drop(context);
        // Keep the snapshot and completion check under one lock: a worker must not finish
        // between an earlier snapshot and the check, making an incomplete snapshot look final.
        let stored = counts(&scope);
        let report = Counts { complete: stored.complete && Arc::strong_count(&scope) == 1, ..*stored };
        drop(stored);
        (value, report)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn overflowing_counts_are_incomplete() {
            let (_, work) = capture(|| {
                record(Counter::FeatureJsonBytes, u64::MAX);
                record(Counter::FeatureJsonBytes, 1);
            });
            assert_eq!(work.get(Counter::FeatureJsonBytes), u64::MAX);
            assert!(!work.complete());
        }

        #[test]
        fn excessive_nesting_cannot_pass_a_budget() {
            fn nested(left: usize) {
                let (_, report) = capture(|| {
                    if left > 0 {
                        nested(left - 1);
                    }
                    record(Counter::ModelEvaluations, 1);
                });
                assert!(!report.complete());
            }
            nested(MAX_DEPTH);
            assert!(capture(|| ()).1.complete());
        }
    }
}

#[cfg(all(test, feature = "capture"))]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn counters_have_unique_names_and_slots() {
        let (_, work) = capture(|| {
            for (i, counter) in Counter::ALL.into_iter().enumerate() {
                record(counter, i as u64 + 1);
            }
        });
        assert!(work.complete());
        for (i, counter) in Counter::ALL.into_iter().enumerate() {
            assert_eq!(work.get(counter), i as u64 + 1);
            assert!(Counter::ALL.iter().filter(|other| other.name() == counter.name()).count() == 1);
        }
    }

    #[test]
    fn nested_captures_are_inclusive_and_contexts_do_not_double_count() {
        let (_, outer) = capture(|| {
            record(Counter::ModelEvaluations, 1);
            let (_, inner) = capture(|| Context::current().run(|| record(Counter::ModelEvaluations, 2)));
            assert_eq!(inner.get(Counter::ModelEvaluations), 2);
            assert!(inner.complete());
            record(Counter::ModelEvaluations, 4);
        });
        assert_eq!(outer.get(Counter::ModelEvaluations), 7);
        assert!(outer.complete());
    }

    #[test]
    fn outstanding_contexts_are_not_silent_zero_work() {
        let (context, work) = capture(Context::current);
        assert!(!work.complete());
        drop(context);
        assert!(capture(|| ()).1.complete());
    }

    #[test]
    fn unwinding_restores_the_enclosing_capture() {
        let (_, work) = capture(|| {
            let caught = std::panic::catch_unwind(|| {
                capture(|| {
                    record(Counter::SketchSolves, 1);
                    panic!("test capture restoration");
                });
            });
            assert!(caught.is_err());
            record(Counter::SketchSolves, 1);
        });
        assert!(work.complete());
        assert_eq!(work.get(Counter::SketchSolves), 2);
        assert_eq!(capture(|| ()).1.get(Counter::SketchSolves), 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn joined_workers_are_counted_and_unrelated_threads_are_isolated() {
        let (_, work) = capture(|| {
            let context = Context::current();
            let worker = std::thread::spawn(move || context.run(|| record(Counter::NamePreparations, 3)));
            let unrelated = std::thread::spawn(|| record(Counter::NamePreparations, 100));
            worker.join().unwrap();
            unrelated.join().unwrap();
        });
        assert!(work.complete());
        assert_eq!(work.get(Counter::NamePreparations), 3);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_worker_finishing_during_capture_cannot_certify_an_older_snapshot() {
        use std::sync::{Arc, Barrier};
        for _ in 0..64 {
            let (worker, work) = capture(|| {
                let context = Context::current();
                let start = Arc::new(Barrier::new(2));
                let worker_start = start.clone();
                let worker = std::thread::spawn(move || {
                    worker_start.wait();
                    context.run(|| record(Counter::FeatureEvaluations, 1));
                });
                start.wait();
                worker
            });
            worker.join().unwrap();
            if work.complete() {
                assert_eq!(work.get(Counter::FeatureEvaluations), 1);
            }
        }
    }

    #[test]
    fn uncaptured_work_is_not_retained() {
        record(Counter::BodyTransforms, 10);
        let work = capture(|| ()).1;
        assert!(work.complete());
        assert!(work.iter().all(|(_, value)| value == 0));
    }

    #[test]
    fn a_missing_report_cannot_pass_as_a_complete_zero_budget() {
        assert!(!Counts::default().complete());
        assert!(capture(|| ()).1.complete());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn capture_during_thread_teardown_is_incomplete_instead_of_panicking() {
        use std::cell::Cell;
        use std::sync::mpsc::{Sender, channel};

        struct AtExit(Sender<(u32, bool)>);
        impl Drop for AtExit {
            fn drop(&mut self) {
                let (value, report) = capture(|| {
                    record(Counter::ModelEvaluations, 1);
                    42
                });
                let _ = self.0.send((value, report.complete()));
            }
        }
        thread_local! {
            static AFTER_WORK: Cell<Option<AtExit>> = const { Cell::new(None) };
        }
        let (send, receive) = channel();
        std::thread::spawn(move || {
            // ACTIVE is initialized second, so it is destroyed before AFTER_WORK.
            AFTER_WORK.set(Some(AtExit(send)));
            assert!(capture(|| ()).1.complete());
        })
        .join()
        .unwrap();
        assert_eq!(receive.recv().unwrap(), (42, false));
    }
}
