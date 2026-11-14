//! SMLib's check of a feature's bodies beside the features after it.
//!
//! Under [`kernel::Validation::EachFeature`] a feature ends with SMLib's full check of the bodies
//! it keeps, and the next feature used to wait for it: a third to a half of an evaluation went to
//! those checks, on one core. [`Checks`] detaches each check from the bodies it reads
//! ([`kernel::detach_kept`]) and runs it on a thread of its own while the evaluation goes on. What the features after it produce stands only once it passes: their
//! cache entries are held until then, and a refusal takes the evaluation back to the refused
//! feature, which then fails with the check's error and warnings, as it did when the check ran
//! in line. The results are the same either way; only the time differs.

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
    pub saved: Saved,
    pub failure: Failure,
}

struct Running {
    index: usize,
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
                .map(|b| (b.name.clone(), kernel::detach_kept(&b.body)))
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
    pub fn start(
        &mut self,
        index: usize,
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
                self.running.push_back(Running { index, saved, failure, done });
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
                return Some(Refused { index: r.index, saved: r.saved, failure: Failure { error, ..r.failure } });
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
        assert!(c.start(0, saved(), failure(), later(Ok(()))).is_ok());
        c.store(0, entry(10), &cache, &kept);
        assert!(c.start(1, saved(), failure(), later(Err(KernelError::Failed("refused".into())))).is_ok());
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

    /// With no check running a result is stored at once; once the checks pass, so is every
    /// result that waited for them.
    #[test]
    fn passed_checks_store_what_waited() {
        let (cache, kept) = (Mutex::new(EvalCache::default()), KeptSlot(None));
        let mut c = checks();
        c.store(0, entry(10), &cache, &kept);
        assert_eq!(cache.lock().unwrap().len(), 1);
        assert!(c.start(1, saved(), failure(), later(Ok(()))).is_ok());
        c.store(1, entry(11), &cache, &kept);
        c.store(2, entry(12), &cache, &kept);
        assert!(c.settle(true, &cache, &kept).is_none());
        assert_eq!(cache.lock().unwrap().len(), 3);
    }
}
