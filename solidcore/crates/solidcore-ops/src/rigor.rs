//! How hard an operation works: one value an application stores with each operation and hands
//! back to ops, so a design can trade speed for rigor per operation (the design: SolidCraft's
//! `docs/design/rigor-dial.md`).
//!
//! A [`Rigor`] has four knobs, each settable on its own:
//! - **certification** ([`Certify`]): what is tested of the operands and required of the result,
//!   from nothing tested ([`Certify::Off`]) through [`Contract::Closed`] to [`Contract::Proved`];
//! - **accuracy** ([`Accuracy`]): the tolerance, chord and samples of the curves and surfaces
//!   an operation fits;
//! - **effort** ([`Effort`]): how far the iteration and subdivision limits of its solvers are
//!   scaled;
//! - **time**: a wall-clock budget. A call that runs past it returns [`Error::OutOfTime`] and
//!   no result: running out of time is a refusal, never a different answer.
//!
//! [`Rigor::dial`] maps one number in `0..=1` to all four, by a table ([`STOPS`]), not a
//! formula: the same dial gives the same knobs to the bit on every platform, and ops computes
//! nothing to get them. [`Rigor::LEGACY`] is what every call did before rigor existed; a
//! call given it does exactly what it did, so a design that records no rigor builds the faces
//! it always built.
//!
//! **How a family reads it.** An application runs an operation's calls [`within`] its rigor,
//! as it runs them within a [`crate::cancel::CancelToken`]. The scope applies the time budget
//! to every call in it; each family reads the knobs it has ([`current`]) and gives
//! [`Error::OutOfTime`] for a call the budget stopped ([`checked`]). The families that read
//! each knob so far:
//!
//! | knob | read by |
//! |---|---|
//! | certification | [`crate::boolean`] (union, subtract, intersect) |
//! | time | every call in the scope; [`Error::OutOfTime`] from [`crate::boolean`] |
//! | accuracy | none yet (recorded, not applied) |
//! | effort | none yet (recorded, not applied) |
//!
//! A knob a family does not read changes nothing it does. Without a scope every call runs as
//! [`Rigor::LEGACY`] says.
//!
//! **Threads.** The scope is this thread's, as a cancel token's: work that starts threads hands
//! them [`current`] and runs each [`within`] it.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::body::Contract;
use crate::cancel::{self, CancelToken};
use crate::{Error, Result};

/// What an operation tests of its operands and requires of its result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Certify {
    /// Nothing tested: a result with faces is a result, whatever the operands were. The
    /// fastest; an application that asks for it checks the result itself when it chooses
    /// ([`crate::Body::check`]).
    Off,
    /// [`Contract::Closed`]: a solid's result must be closed. What every call did before
    /// rigor existed.
    #[default]
    Closed,
    /// [`Contract::Proved`]: a solid's result must be proved valid and facing out.
    Proved,
}

impl Certify {
    /// The contract a family with no [`Certify::Off`] of its own runs under: [`Certify::Off`]
    /// runs as [`Contract::Closed`], which tests less than nothing never does.
    #[must_use]
    pub fn contract(self) -> Contract {
        match self {
            Certify::Off | Certify::Closed => Contract::Closed,
            Certify::Proved => Contract::Proved,
        }
    }
}

/// How closely an operation fits what it makes. `None`: the call's own value, as before rigor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Accuracy {
    /// The model tolerance (the kernel's zone tolerance, `1e-5` of its own), model units.
    pub tolerance: Option<f64>,
    /// The largest chord height of a curve or surface fit's sampling, model units.
    pub chord: Option<f64>,
    /// The fewest samples per span of a curve or surface fit.
    pub samples: Option<u32>,
}

/// How far an operation's solvers may go. `None`: the call's own limits, as before rigor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Effort {
    /// The iteration and subdivision limits as a percentage of the call's own (100: its own).
    /// A family scales each limit it has and keeps it at least 1.
    pub percent: Option<u32>,
}

/// How hard an operation works ([`crate::rigor`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rigor {
    /// What is tested and required.
    pub certify: Certify,
    /// How closely it fits.
    pub accuracy: Accuracy,
    /// How far its solvers go.
    pub effort: Effort,
    /// The wall-clock budget of its calls; `None`: no budget.
    pub time: Option<Duration>,
}

impl Default for Rigor {
    /// [`Rigor::LEGACY`].
    fn default() -> Self {
        Self::LEGACY
    }
}

/// One stop of the dial ([`Rigor::dial`]): the dial value it starts at, and its rigor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    /// The dial value from which this stop applies, up to the next stop's.
    pub from: f64,
    /// A short name an application may show for it.
    pub name: &'static str,
    /// Its rigor.
    pub rigor: Rigor,
}

/// The dial's stops, in increasing order of `from` and of every knob: a dial value takes the
/// last stop it reaches. Five, so a slider has five places that differ, and a value between
/// two stops is the lower one's.
pub const STOPS: [Stop; 5] = [
    Stop {
        from: 0.0,
        name: "Preview",
        rigor: Rigor {
            certify: Certify::Off,
            accuracy: Accuracy {
                tolerance: Some(1e-4),
                chord: Some(5e-2),
                samples: Some(8),
            },
            effort: Effort { percent: Some(25) },
            time: Some(Duration::from_secs(2)),
        },
    },
    Stop {
        from: 0.25,
        name: "Fast",
        rigor: Rigor {
            certify: Certify::Closed,
            accuracy: Accuracy {
                tolerance: Some(5e-5),
                chord: Some(2e-2),
                samples: Some(12),
            },
            effort: Effort { percent: Some(50) },
            time: Some(Duration::from_secs(10)),
        },
    },
    Stop {
        from: 0.5,
        name: "Standard",
        rigor: Rigor {
            certify: Certify::Closed,
            accuracy: Accuracy {
                tolerance: Some(1e-5),
                chord: Some(1e-2),
                samples: Some(16),
            },
            effort: Effort { percent: Some(100) },
            time: Some(Duration::from_secs(30)),
        },
    },
    Stop {
        from: 0.75,
        name: "Careful",
        rigor: Rigor {
            certify: Certify::Proved,
            accuracy: Accuracy {
                tolerance: Some(5e-6),
                chord: Some(5e-3),
                samples: Some(24),
            },
            effort: Effort { percent: Some(200) },
            time: Some(Duration::from_secs(120)),
        },
    },
    Stop {
        from: 1.0,
        name: "Proof",
        rigor: Rigor {
            certify: Certify::Proved,
            accuracy: Accuracy {
                tolerance: Some(1e-6),
                chord: Some(1e-3),
                samples: Some(32),
            },
            effort: Effort { percent: Some(400) },
            time: Some(Duration::from_secs(600)),
        },
    },
];

impl Rigor {
    /// What every call did before rigor existed: [`Certify::Closed`], every accuracy and
    /// effort knob the call's own, no time budget. Not a stop of the dial: a design that
    /// records no rigor builds with this, and a call given it does exactly what it did.
    pub const LEGACY: Rigor = Rigor {
        certify: Certify::Closed,
        accuracy: Accuracy {
            tolerance: None,
            chord: None,
            samples: None,
        },
        effort: Effort { percent: None },
        time: None,
    };

    /// The rigor of dial value `d`: the last of [`STOPS`] whose `from` it reaches.
    ///
    /// # Errors
    /// [`Error::Argument`] for a `d` that is not finite or outside `0..=1`.
    pub fn dial(d: f64) -> Result<Rigor> {
        Ok(Self::stop(d)?.rigor)
    }

    /// The stop of dial value `d` ([`Rigor::dial`]).
    ///
    /// # Errors
    /// As [`Rigor::dial`].
    pub fn stop(d: f64) -> Result<Stop> {
        if !(0.0..=1.0).contains(&d) {
            return Err(Error::Argument);
        }
        STOPS
            .iter()
            .rev()
            .find(|s| d >= s.from)
            .copied()
            .ok_or(Error::Argument)
    }

    /// Whether this is [`Rigor::LEGACY`].
    #[must_use]
    pub fn is_legacy(&self) -> bool {
        *self == Self::LEGACY
    }

    /// Fails for a knob no call can run with: a tolerance or chord that is not finite and
    /// positive, no samples, an effort of 0 %.
    ///
    /// # Errors
    /// [`Error::Argument`].
    pub fn check(&self) -> Result<()> {
        let positive = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v > 0.0);
        let ok = positive(self.accuracy.tolerance)
            && positive(self.accuracy.chord)
            && self.accuracy.samples != Some(0)
            && self.effort.percent != Some(0);
        if ok {
            Ok(())
        } else {
            Err(Error::Argument)
        }
    }
}

/// The rigor scope this thread's ops calls run in.
#[derive(Clone)]
struct Scope {
    rigor: Rigor,
    /// The deadlines over the scope, innermost last: each its budget and whether it fired.
    deadlines: Vec<(Duration, Arc<AtomicBool>)>,
    /// The application's own token over the scope as it began: a cancellation of it is the
    /// application's, never a deadline's.
    outer: Option<CancelToken>,
}

thread_local! {
    static CURRENT: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

/// A running time budget: cancels its token when the budget is spent, unless dropped first.
struct Deadline {
    token: CancelToken,
    fired: Arc<AtomicBool>,
    /// Dropped to tell the watch its scope ended.
    done: Option<mpsc::Sender<()>>,
    watch: Option<std::thread::JoinHandle<()>>,
}

impl Deadline {
    /// Starts a budget of `budget`. A zero budget is spent at once. Where no thread can be
    /// started (a browser without threads), the budget is not enforced: the call runs to its
    /// end, which is the answer without a budget, never another one.
    fn start(budget: Duration) -> Deadline {
        let token = CancelToken::new();
        let fired = Arc::new(AtomicBool::new(false));
        if budget.is_zero() {
            fired.store(true, Ordering::Relaxed);
            token.cancel();
            return Deadline {
                token,
                fired,
                done: None,
                watch: None,
            };
        }
        let (done, ended) = mpsc::channel::<()>();
        let (t, f) = (token.clone(), Arc::clone(&fired));
        let watch = std::thread::Builder::new()
            .name("solidcore-ops rigor deadline".into())
            .spawn(move || {
                if let Err(mpsc::RecvTimeoutError::Timeout) = ended.recv_timeout(budget) {
                    f.store(true, Ordering::Relaxed);
                    t.cancel();
                }
            })
            .ok();
        Deadline {
            token,
            fired,
            done: Some(done),
            watch,
        }
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        // the sender goes first: the watch wakes at once and ends
        self.done = None;
        if let Some(w) = self.watch.take() {
            drop(w.join());
        }
    }
}

/// Runs `f` with `rigor` over the ops calls it makes on this thread: its time budget over
/// all of them, its other knobs for each family that reads them ([`current`]). [`Rigor::LEGACY`]
/// runs `f` with no scope at all, exactly as before rigor existed. Within another rigor, the
/// inner one's knobs apply and both budgets run.
pub fn within<R>(rigor: &Rigor, f: impl FnOnce() -> R) -> R {
    /// The scope before, put back on drop.
    struct Restore(Option<Scope>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = previous);
        }
    }
    if rigor.is_legacy() && CURRENT.with(|c| c.borrow().is_none()) {
        return f();
    }
    let deadline = rigor.time.map(Deadline::start);
    let mut deadlines = CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .map(|s| s.deadlines.clone())
            .unwrap_or_default()
    });
    if let (Some(d), Some(budget)) = (&deadline, rigor.time) {
        deadlines.push((budget, Arc::clone(&d.fired)));
    }
    let scope = Scope {
        rigor: *rigor,
        deadlines,
        outer: cancel::current(),
    };
    let _restore = Restore(CURRENT.with(|c| c.replace(Some(scope))));
    match &deadline {
        Some(d) => cancel::within(&d.token, f),
        None => f(),
    }
}

/// The rigor over this thread's ops calls; `None` outside every scope (as [`Rigor::LEGACY`]).
#[must_use]
pub fn current() -> Option<Rigor> {
    CURRENT.with(|c| c.borrow().as_ref().map(|s| s.rigor))
}

/// The budget a deadline over this thread's ops calls spent, when one has and the application
/// did not cancel the work itself.
#[must_use]
pub fn out_of_time() -> Option<Duration> {
    CURRENT.with(|c| {
        let c = c.borrow();
        let s = c.as_ref()?;
        if s.outer.as_ref().is_some_and(CancelToken::is_cancelled) {
            return None;
        }
        s.deadlines
            .iter()
            .find(|(_, fired)| fired.load(Ordering::Relaxed))
            .map(|(budget, _)| *budget)
    })
}

/// `r` with a [`Error::Cancelled`] a spent budget caused given as [`Error::OutOfTime`]: what a
/// family returns for a call it ran within a rigor.
///
/// # Errors
/// `r`'s error, [`Error::OutOfTime`] in place of a deadline's cancellation.
pub fn checked<T>(r: Result<T>) -> Result<T> {
    match r {
        Err(Error::Cancelled) => match out_of_time() {
            Some(budget) => Err(Error::OutOfTime { budget }),
            None => Err(Error::Cancelled),
        },
        r => r,
    }
}

#[cfg(test)]
mod tests;
