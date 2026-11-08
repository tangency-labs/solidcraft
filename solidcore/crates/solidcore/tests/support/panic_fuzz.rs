//! The panic fuzz's harness (#2490): a seeded PRNG, hostile values, and a runner that
//! calls each generated case on its own thread under `catch_unwind`, records every
//! panic's message and location through a panic hook (a panic library code catches
//! itself is a finding too: the web build is panic=abort), and reports a case that runs past
//! the hang limit as a hang finding (its kernel's [`solidcore::CancelToken`] is
//! cancelled, and a case that still does not return is left running).
//!
//! Shared by `tests/panic_fuzz.rs` (the kernel's entry points) and step-brep's
//! `tests/panic_fuzz.rs` (STEP import). Environment:
//! - `SOLIDCORE_PANIC_FUZZ_CASES`: cases per family (default: each family's CI count).
//! - `SOLIDCORE_PANIC_FUZZ_SEED`: the base seed (default 2490).
//! - `SOLIDCORE_PANIC_FUZZ_HANG_SECS`: the hang limit per case (default 60 s in a debug
//!   build, so slower CI machines don't flake, and 5 s with optimisations).
//! - `SOLIDCORE_PANIC_FUZZ_HANGS=fail`: a hang fails the run too. By default hangs are only
//!   printed, so the tests verdict never depends on the machines speed.
//! - `SOLIDCORE_PANIC_FUZZ_REPRO=<family>:<case seed>`: run only that case of that family
//!   (the seed a finding lists), printing its input.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    dead_code
)] // test helpers

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt::{Debug, Write as _};
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::Once;
use std::time::{Duration, Instant};

/// splitmix64: small, seedable, and the same on every platform.
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value in `0..n` (`0` for `n == 0`).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// `true` with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// A value in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A value in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// One of `items`.
    pub fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())].clone()
    }

    /// A hostile value: one of the special values with probability `p`, else a size
    /// in `[lo, hi)`.
    pub fn hostile(&mut self, p: f64, lo: f64, hi: f64) -> f64 {
        if self.chance(p) {
            self.pick(HOSTILE)
        } else {
            self.range(lo, hi)
        }
    }
}

/// NaN, infinities, zeros, negatives, subnormals, huge and tiny values.
pub const HOSTILE: &[f64] = &[
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    0.0,
    -0.0,
    -1.0,
    -1e-3,
    5e-324,
    1e-310,
    f64::MIN_POSITIVE,
    1e-300,
    1e-200,
    1e-15,
    1e-9,
    f64::EPSILON,
    1e15,
    1e100,
    1e300,
    -1e300,
    f64::MAX,
    f64::MIN,
];

/// The seed of case `index` from the base seed.
pub fn case_seed(base: u64, index: u64) -> u64 {
    Rng(base ^ index.wrapping_mul(0xd1b5_4a32_d192_ed03)).next_u64()
}

/// The environment variable `name` as a `u64`: `None` when it is unset; panics when it
/// is set to anything else (`10_000`, `1e4`, `10k`, empty, not UTF-8), which would
/// otherwise run the default silently.
fn env_u64(name: &str) -> Option<u64> {
    // not UTF-8: the lossy form keeps a replacement character, so it fails to parse too
    let raw = std::env::var_os(name)?;
    Some(parse_env_u64(name, &raw.to_string_lossy()))
}

/// `value` of the environment variable `name` as a `u64` (surrounding whitespace
/// ignored); panics, naming the variable and the value, on any other form.
pub fn parse_env_u64(name: &str, value: &str) -> u64 {
    value
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("{name}={value:?}: expected a u64, e.g. `10000` ({e})"))
}

/// `SOLIDCORE_PANIC_FUZZ_HANGS`'s value: `fail` (spaces trimmed) makes hangs findings; any
/// other value panics (`1`, `true` or `FAIL` would otherwise leave hangs only printed).
pub fn parse_hangs(value: &str) -> bool {
    match value.trim() {
        "fail" => true,
        _ => panic!("SOLIDCORE_PANIC_FUZZ_HANGS={value:?}: expected `fail` (or unset)"),
    }
}

/// `SOLIDCORE_PANIC_FUZZ_REPRO`'s `<family>:<u64 seed>`, split; panics on any other form
/// (a malformed spec would otherwise skip every family, or run the CI set, silently).
pub fn parse_repro(spec: &str) -> (&str, u64) {
    let parsed = spec
        .split_once(':')
        .and_then(|(f, seed)| Some((f.trim(), seed.trim().parse::<u64>().ok()?)))
        .filter(|(f, _)| !f.is_empty());
    match parsed {
        Some(p) => p,
        None => panic!(
            "SOLIDCORE_PANIC_FUZZ_REPRO={spec:?}: expected `<family>:<u64 seed>`, \
             e.g. `booleans:12`"
        ),
    }
}

/// The run's settings, read from the environment.
#[derive(Clone, Debug)]
pub struct Settings {
    pub cases: u64,
    pub base_seed: u64,
    pub hang: Duration,
    /// `Some(seed)` when `SOLIDCORE_PANIC_FUZZ_REPRO` names this family.
    pub repro: Option<u64>,
    /// `SOLIDCORE_PANIC_FUZZ_REPRO` names another family: run nothing.
    pub skip: bool,
    /// `SOLIDCORE_PANIC_FUZZ_HANGS=fail`: hangs are findings, not just printed.
    pub fail_on_hang: bool,
}

impl Settings {
    /// The settings of `family`, whose CI run is `ci_cases` cases.
    pub fn for_family(family: &str, ci_cases: u64) -> Self {
        // a debug build runs the slowest CI cases in ~10 s (under 2 s optimised)
        let hang_default = if cfg!(debug_assertions) { 60 } else { 5 };
        let (repro, skip) = match std::env::var("SOLIDCORE_PANIC_FUZZ_REPRO") {
            Ok(spec) => {
                let (f, seed) = parse_repro(&spec);
                if f == family {
                    (Some(seed), false)
                } else {
                    (None, true)
                }
            }
            Err(_) => (None, false),
        };
        Self {
            cases: env_u64("SOLIDCORE_PANIC_FUZZ_CASES").unwrap_or(ci_cases),
            base_seed: env_u64("SOLIDCORE_PANIC_FUZZ_SEED").unwrap_or(2490),
            hang: Duration::from_secs(
                env_u64("SOLIDCORE_PANIC_FUZZ_HANG_SECS").unwrap_or(hang_default),
            ),
            repro,
            skip,
            fail_on_hang: std::env::var_os("SOLIDCORE_PANIC_FUZZ_HANGS")
                .is_some_and(|v| parse_hangs(&v.to_string_lossy())),
        }
    }
}

thread_local! {
    static FUZZING: Cell<bool> = const { Cell::new(false) };
    static LAST_PANIC: RefCell<Option<Panic>> = const { RefCell::new(None) };
}

/// One caught panic.
#[derive(Clone, Debug)]
pub struct Panic {
    pub message: String,
    /// `file:line:col` the panic reports (the caller of a `#[track_caller]` std call).
    pub location: String,
    /// The innermost backtrace frame in this workspace's crates, when there is one.
    pub frame: Option<String>,
    /// Library code caught it (`catch_unwind`) and the case returned normally; with
    /// panic=abort (the web build) it aborts all the same.
    pub caught: bool,
}

impl Panic {
    /// The key findings are grouped by: the workspace frame when there is one (a panic
    /// inside std reports std's location), else the location.
    fn key(&self) -> String {
        match &self.frame {
            Some(frame) if !self.location.contains("crates/") => frame.clone(),
            _ => self.location.clone(),
        }
    }
}

/// The innermost frame of a backtrace in a workspace crate (`crates/...`).
fn workspace_frame(bt: &str) -> Option<String> {
    let mut function = None;
    for line in bt.lines() {
        let line = line.trim();
        if let Some(at) = line.strip_prefix("at ") {
            if let (Some(f), Some(i)) = (&function, at.find("crates/")) {
                let path = &at[i..];
                if !path.contains("/tests/") {
                    return Some(format!("{path} ({f})"));
                }
            }
        } else if let Some((_, f)) = line.split_once(": ") {
            function = Some(f.to_string());
        }
    }
    None
}

fn install_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !FUZZING.with(Cell::get) {
                default(info);
                return;
            }
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string payload>".into());
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_default();
            let frame = if location.contains("crates/") {
                None
            } else {
                workspace_frame(&std::backtrace::Backtrace::force_capture().to_string())
            };
            LAST_PANIC.with(|p| {
                *p.borrow_mut() = Some(Panic {
                    message,
                    location,
                    frame,
                    caught: false,
                });
            });
        }));
    });
}

/// A finding: one distinct panic location (or a hang), with its first reproducer.
#[derive(Clone, Debug)]
pub struct Finding {
    pub panic: Option<Panic>,
    pub seed: u64,
    pub input: String,
    pub count: usize,
}

/// What a case does: build the input from `rng` (deterministically), describe it, and
/// run it. `cancel` is to be set on the case's kernel so a hang can be cut short.
pub trait Family: Send + Sync + 'static {
    /// The input of one case, built from its seed.
    type Input: Debug + Clone + Send + 'static;
    /// The family's name (for the report and `SOLIDCORE_PANIC_FUZZ_REPRO`).
    fn name(&self) -> &'static str;
    /// Build one case's input.
    fn generate(&self, rng: &mut Rng) -> Self::Input;
    /// Run it. Errors are fine; only panics are findings.
    fn run(input: Self::Input, cancel: &solidcore::CancelToken);
}

/// How one case ended.
pub enum Outcome {
    /// Returned (with `Ok` or `Err`).
    Done,
    /// Panicked.
    Panicked(Panic),
    /// Still running at the hang limit. `detached`: it did not return within the grace
    /// period after its token was cancelled either, and is left running.
    Hang { detached: bool },
}

/// Runs one input on its own thread.
pub fn run_one<F: Family>(input: F::Input, hang: Duration) -> Outcome {
    install_hook();
    let (tx, rx) = mpsc::channel();
    let token = solidcore::CancelToken::new();
    let thread_token = token.clone();
    let spawned = std::thread::Builder::new()
        .name("panic-fuzz-case".into())
        .stack_size(256 << 20)
        .spawn(move || {
            FUZZING.with(|f| f.set(true));
            let result = panic::catch_unwind(AssertUnwindSafe(|| F::run(input, &thread_token)));
            // A panic the library caught itself (`catch_unwind` turning it into a value)
            // returns normally here, but aborts under the web build's panic=abort: any
            // panic the hook recorded is a finding, whichever way `run` ended.
            let recorded = LAST_PANIC.with(|p| p.borrow_mut().take());
            let outcome = match (result, recorded) {
                (Ok(()), None) => Outcome::Done,
                (Ok(()), Some(p)) => Outcome::Panicked(Panic { caught: true, ..p }),
                (Err(_), Some(p)) => Outcome::Panicked(p),
                (Err(payload), None) => Outcome::Panicked(Panic {
                    message: payload
                        .downcast_ref::<String>()
                        .cloned()
                        .unwrap_or_else(|| "<no hook record>".into()),
                    location: String::new(),
                    frame: None,
                    caught: false,
                }),
            };
            let _ = tx.send(outcome);
        })
        .expect("spawn a fuzz case thread");
    if let Ok(outcome) = rx.recv_timeout(hang) {
        let _ = spawned.join();
        return outcome;
    }
    // cancel, give it a moment to unwind; a case that ignores the token is left running
    // (detached) so the run goes on
    token.cancel();
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Outcome::Panicked(p)) => Outcome::Panicked(p),
        Ok(_) => Outcome::Hang { detached: false },
        Err(_) => Outcome::Hang { detached: true },
    }
}

/// Runs `family`'s cases and returns its findings, keyed by panic location (`hang` for
/// hangs), each with its first reproducing seed and input.
pub fn fuzz<F: Family>(family: &F, settings: &Settings) -> BTreeMap<String, Finding> {
    let mut findings: BTreeMap<String, Finding> = BTreeMap::new();
    if settings.skip {
        return findings;
    }
    // each family its own seeds (FNV-1a of the name), so families don't share inputs
    let family_seed = family
        .name()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
    let seeds: Vec<u64> = match settings.repro {
        Some(seed) => vec![seed],
        None => (0..settings.cases)
            .map(|i| case_seed(settings.base_seed ^ family_seed, i))
            .collect(),
    };
    let started = Instant::now();
    let mut slowest = (Duration::ZERO, 0);
    let mut hangs = 0;
    let mut detached_threads = 0;
    for seed in seeds.iter().copied() {
        let input = family.generate(&mut Rng(seed));
        if settings.repro.is_some() {
            eprintln!("{} case {seed}: {input:#?}", family.name());
        }
        let shown = format!("{input:?}");
        let case_started = Instant::now();
        let outcome = run_one::<F>(input, settings.hang);
        let took = case_started.elapsed();
        if took > slowest.0 {
            slowest = (took, seed);
        }
        if took > settings.hang / 2 && !matches!(outcome, Outcome::Hang { .. }) {
            eprintln!(
                "panic fuzz: {} case {seed} took {took:?} (hang limit {:?})",
                family.name(),
                settings.hang
            );
        }
        let (key, panic) = match outcome {
            Outcome::Done => continue,
            Outcome::Panicked(p) => (p.key(), Some(p)),
            Outcome::Hang { detached } => {
                hangs += 1;
                if detached {
                    detached_threads += 1;
                }
                eprintln!(
                    "panic fuzz: {} case {seed} still running at the hang limit {:?}{}; \
                     repro: SOLIDCORE_PANIC_FUZZ_REPRO={}:{seed}\n  input: {}",
                    family.name(),
                    settings.hang,
                    if detached {
                        ", ignores its cancel token (left running)"
                    } else {
                        ""
                    },
                    family.name(),
                    shown.chars().take(1500).collect::<String>()
                );
                if !settings.fail_on_hang {
                    continue;
                }
                (format!("hang #{hangs} (> {:?})", settings.hang), None)
            }
        };
        findings
            .entry(key)
            .and_modify(|f| f.count += 1)
            .or_insert(Finding {
                panic,
                seed,
                input: shown,
                count: 1,
            });
    }
    eprintln!(
        "panic fuzz: {}: {} cases in {:?}, slowest {:?} (case {}), {} hangs \
         ({} threads left running), {} distinct findings",
        family.name(),
        seeds.len(),
        started.elapsed(),
        slowest.0,
        slowest.1,
        hangs,
        detached_threads,
        findings.len()
    );
    findings
}

/// Fails listing every finding (location, message, count, reproducing seed and input).
pub fn assert_no_findings(family: &str, findings: &BTreeMap<String, Finding>) {
    if findings.is_empty() {
        return;
    }
    let mut report = format!(
        "panic fuzz `{family}`: {} distinct findings\n",
        findings.len()
    );
    for (key, f) in findings {
        let what = match &f.panic {
            Some(p) if p.caught => format!(
                "panic at {} `{}`, caught by library code (aborts under panic=abort)",
                p.location, p.message
            ),
            Some(p) => format!("panic at {} `{}`", p.location, p.message),
            None => "hang".to_string(),
        };
        let input: String = f.input.chars().take(1500).collect();
        let _ = write!(
            report,
            "\n- {key}\n  {what} ({} cases)\n  repro: SOLIDCORE_PANIC_FUZZ_REPRO={family}:{}\n  input: {input}\n",
            f.count, f.seed
        );
    }
    panic!("{report}");
}
