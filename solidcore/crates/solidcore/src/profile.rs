//! Opt-in profiler of the kernel's operations (feature `profile`, off by default).
//!
//! Not upstream. An operation marks its steps with [`profile_scope!`]; with the feature on,
//! each scope records its name, its nesting, how often it ran, its time and what the
//! counted work ([`Counter`]) did inside it, aggregated by call path (a calling-context
//! tree: one record per distinct path of scope names, with a call count), so memory grows
//! with the number of distinct paths, never with the number of calls. With the feature
//! off the macros expand to nothing and the kernel has no profiler field: no code, no cost.
//!
//! The profiler only observes: no result reads it, and nothing it records feeds back into
//! an operation. Times come from `std::time::Instant`, used only here, only with the
//! feature on, and never on `wasm32` (where `Instant::now` panics): there the records
//! carry the counts and the nesting without times.
//!
//! [`crate::Kernel::profiler`] holds the records (shared through an [`Arc`], so a scope
//! can outlive the `&mut Kernel` borrow it was opened with, and so the cache kernels an
//! operation copies record into the same tree); [`crate::Kernel::take_profile`] returns
//! them and starts afresh, and [`Profile::write_jsonl`] writes them as JSON Lines to any
//! [`std::io::Write`]. The library never reads the environment or writes files itself:
//! a test or an example does (`examples/profile_report.rs` turns the JSONL into the
//! top self-time scopes and a folded-stacks file for flame graphs).
//!
//! # Record format (one JSON object per line)
//!
//! ```text
//! {"type":"scope","id":3,"parent":1,"depth":2,"name":"blend::executive::calculate_solver_geometry",
//!  "calls":1,"start_ns":1200,"dur_ns":53000,"self_ns":400,"counters":{"evaluations":812}}
//! {"type":"summary","name":"blend::executive::calculate_solver_geometry","count":1,
//!  "total_ns":53000,"self_ns":400,"self_counters":{"evaluations":12}}
//! ```
//!
//! A `scope` line is one call path: `parent` is the enclosing path's `id` (`null` at the
//! root), `calls` how often the path ran, `start_ns` when it first began (from the
//! profiler's creation), `dur_ns` its total time and `self_ns` that time less its
//! children's, `counters` the counted work done inside it (children included; zero counts
//! left out). A `summary` line totals one name over all its paths: `count` calls,
//! `total_ns` the time inside it (a recursive name counted once at its outermost level),
//! `self_ns` and `self_counters` outside its children. On `wasm32` every `*_ns` key is
//! absent.

#[cfg(feature = "profile")]
pub use enabled::*;

/// Opens a profiler scope named `$name` (a `&'static str`, by convention the module path
/// and the step, e.g. `"blend::executive::trim_solver_rails"`) on the kernel `$k` (a
/// `Kernel`, `&Kernel` or `&mut Kernel`) until the end of the enclosing block, early
/// returns and `?` included. The kernel is borrowed only while the scope opens, so the
/// block goes on using `$k` mutably. Expands to nothing without the `profile` feature.
#[cfg(feature = "profile")]
macro_rules! profile_scope {
    ($k:expr, $name:expr) => {
        let _profile_scope = $crate::profile::Profiler::scope(&$k.profiler, $name);
    };
}

/// Opens a profiler scope (expands to nothing: the `profile` feature is off).
#[cfg(not(feature = "profile"))]
macro_rules! profile_scope {
    ($k:expr, $name:expr) => {};
}

/// Adds `$n` to the profiler counter `$counter` (a [`Counter`] variant name) of the kernel
/// `$k`. Expands to nothing without the `profile` feature.
#[cfg(feature = "profile")]
macro_rules! profile_count {
    ($k:expr, $counter:ident, $n:expr) => {
        $crate::profile::Profiler::count(&$k.profiler, $crate::profile::Counter::$counter, $n)
    };
}

/// Adds to a profiler counter (expands to nothing: the `profile` feature is off).
#[cfg(not(feature = "profile"))]
macro_rules! profile_count {
    ($k:expr, $counter:ident, $n:expr) => {};
}

/// Records a diagnostic note only when explicitly captured by the profiler.
#[cfg(feature = "profile")]
macro_rules! profile_note {
    ($k:expr, $($arg:tt)*) => {
        $k.profiler.note(|| format!($($arg)*));
    };
}

#[cfg(not(feature = "profile"))]
macro_rules! profile_note {
    ($k:expr, $($arg:tt)*) => {};
}

pub(crate) use profile_note;
pub(crate) use profile_count;
pub(crate) use profile_scope;

#[cfg(feature = "profile")]
mod enabled {
    use core::sync::atomic::{AtomicU64, Ordering};
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

    /// The work the profiler counts inside each scope, besides time: machine independent,
    /// so two runs compare by them where their times differ.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Counter {
        /// Points evaluated on a procedural blend's definition
        /// ([`crate::surface::procedural_blend::BlendCounts::evaluations`]).
        Evaluations,
        /// Station solves of those definitions (`BlendCounts::solves`).
        Solves,
        /// The Newton steps of those solves (`BlendCounts::newton_steps`).
        NewtonSteps,
        /// Evaluations a blend's certified net answered for its definition
        /// (`BlendCounts::net_evaluations`).
        NetEvaluations,
        /// Passes of a fillet solver's trace loop (`c4_trace`: the first, and each
        /// recovery after a self-intersection or a cusp).
        TracePasses,
        /// Steps of a surface-surface intersection march.
        MarchSteps,
        /// Candidate pairs supplied to a topology coincidence phase, before incidence filters.
        CoincidenceCandidates,
        /// Geometric query invocations in a topology coincidence phase (including empty edge searches).
        CoincidenceQueries,
        /// Trim classification invocations after a successful coincidence projection.
        CoincidenceClassifications,
    }

    /// How many [`Counter`]s there are.
    pub const COUNTERS: usize = 9;

    impl Counter {
        /// Every counter, in the order of [`Profile`]'s counter arrays.
        pub const ALL: [Self; COUNTERS] = [
            Self::Evaluations,
            Self::Solves,
            Self::NewtonSteps,
            Self::NetEvaluations,
            Self::TracePasses,
            Self::MarchSteps,
            Self::CoincidenceCandidates,
            Self::CoincidenceQueries,
            Self::CoincidenceClassifications,
        ];

        /// The counter's key in the JSONL records.
        #[must_use]
        pub const fn name(self) -> &'static str {
            match self {
                Self::Evaluations => "evaluations",
                Self::Solves => "solves",
                Self::NewtonSteps => "newton_steps",
                Self::NetEvaluations => "net_evaluations",
                Self::TracePasses => "trace_passes",
                Self::MarchSteps => "march_steps",
                Self::CoincidenceCandidates => "coincidence_candidates",
                Self::CoincidenceQueries => "coincidence_queries",
                Self::CoincidenceClassifications => "coincidence_classifications",
            }
        }

        const fn index(self) -> usize {
            self as usize
        }
    }

    /// One call path of the calling-context tree.
    #[derive(Debug)]
    struct Node {
        name: &'static str,
        parent: Option<u32>,
        depth: u32,
        children: Vec<u32>,
        calls: u64,
        first_start_ns: Option<u64>,
        total_ns: u64,
        child_ns: u64,
        counters: [u64; COUNTERS],
        child_counters: [u64; COUNTERS],
    }

    /// An open scope.
    #[derive(Debug)]
    struct Frame {
        node: u32,
        start_ns: Option<u64>,
        counters: [u64; COUNTERS],
    }

    #[derive(Debug, Default)]
    struct State {
        nodes: Vec<Node>,
        roots: Vec<u32>,
        stack: Vec<Frame>,
        notes: Option<Vec<String>>,
    }

    impl State {
        /// The child path of `parent` named `name`, made on first use; `None` past
        /// `u32::MAX` paths (the scope then goes unrecorded).
        fn child(&mut self, parent: Option<u32>, name: &'static str) -> Option<u32> {
            let siblings = match parent {
                Some(p) => &self.nodes.get(p as usize)?.children,
                None => &self.roots,
            };
            let found = siblings
                .iter()
                .copied()
                .find(|&c| self.nodes.get(c as usize).is_some_and(|n| n.name == name));
            if found.is_some() {
                return found;
            }
            let id = u32::try_from(self.nodes.len()).ok()?;
            let depth = match parent {
                Some(p) => self.nodes.get(p as usize).map_or(0, |n| n.depth + 1),
                None => 0,
            };
            self.nodes.push(Node {
                name,
                parent,
                depth,
                children: Vec::new(),
                calls: 0,
                first_start_ns: None,
                total_ns: 0,
                child_ns: 0,
                counters: [0; COUNTERS],
                child_counters: [0; COUNTERS],
            });
            match parent.and_then(|p| self.nodes.get_mut(p as usize)) {
                Some(p) => p.children.push(id),
                None => self.roots.push(id),
            }
            Some(id)
        }

        /// Closes `frame` at `end_ns` with the counters at `counters`.
        fn close(&mut self, frame: &Frame, end_ns: Option<u64>, counters: &[u64; COUNTERS]) {
            let dur = match (frame.start_ns, end_ns) {
                (Some(s), Some(e)) => Some(e.saturating_sub(s)),
                _ => None,
            };
            let mut delta = [0u64; COUNTERS];
            for ((d, now), then) in delta.iter_mut().zip(counters).zip(&frame.counters) {
                *d = now.wrapping_sub(*then);
            }
            let Some(node) = self.nodes.get_mut(frame.node as usize) else {
                return;
            };
            node.calls = node.calls.saturating_add(1);
            if node.first_start_ns.is_none() {
                node.first_start_ns = frame.start_ns;
            }
            if let Some(dur) = dur {
                node.total_ns = node.total_ns.saturating_add(dur);
            }
            for (c, d) in node.counters.iter_mut().zip(&delta) {
                *c = c.saturating_add(*d);
            }
            let parent = node.parent;
            if let Some(p) = parent.and_then(|p| self.nodes.get_mut(p as usize)) {
                if let Some(dur) = dur {
                    p.child_ns = p.child_ns.saturating_add(dur);
                }
                for (c, d) in p.child_counters.iter_mut().zip(&delta) {
                    *c = c.saturating_add(*d);
                }
            }
        }
    }

    /// The profile's clock: nanoseconds since the profiler was made.
    #[cfg(not(target_arch = "wasm32"))]
    #[derive(Debug)]
    struct Clock(std::time::Instant);

    #[cfg(not(target_arch = "wasm32"))]
    impl Default for Clock {
        fn default() -> Self {
            Self(std::time::Instant::now())
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl Clock {
        fn now_ns(&self) -> Option<u64> {
            u64::try_from(self.0.elapsed().as_nanos()).ok()
        }
    }

    /// No clock on wasm32 (`Instant::now` panics there): the records carry no times.
    #[cfg(target_arch = "wasm32")]
    #[derive(Debug, Default)]
    struct Clock;

    #[cfg(target_arch = "wasm32")]
    impl Clock {
        #[allow(clippy::unused_self)] // the clock's signature off wasm32
        fn now_ns(&self) -> Option<u64> {
            None
        }
    }

    #[derive(Debug, Default)]
    struct Shared {
        state: Mutex<State>,
        counters: [AtomicU64; COUNTERS],
        clock: Clock,
    }

    impl Shared {
        fn lock(&self) -> MutexGuard<'_, State> {
            // a scope never panics while it holds the lock; a poisoned lock (a panic
            // elsewhere on the thread) still holds consistent records
            self.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        fn now_ns(&self) -> Option<u64> {
            self.clock.now_ns()
        }

        fn counters(&self) -> [u64; COUNTERS] {
            let mut out = [0u64; COUNTERS];
            for (o, c) in out.iter_mut().zip(&self.counters) {
                *o = c.load(Ordering::Relaxed);
            }
            out
        }
    }

    /// The profiler of one [`crate::Kernel`] (field [`crate::Kernel::profiler`]): the
    /// calling-context tree of the scopes opened on it and its counters. Cloning shares
    /// the records (the kernel copies an operation makes for its caches record into the
    /// caller's tree); one kernel, and the copies it makes, run on one thread at a time.
    #[derive(Debug, Clone, Default)]
    pub struct Profiler {
        shared: Arc<Shared>,
    }

    impl Profiler {
        /// Starts a fresh diagnostic capture. Normal profiles keep no per-call notes.
        /// At most 100,000 notes are retained, independently of the call-path profile.
        pub fn capture_notes(&self) {
            self.shared.lock().notes = Some(Vec::new());
        }

        /// Ends diagnostic capture and returns its notes in execution order.
        #[must_use]
        pub fn take_notes(&self) -> Vec<String> {
            self.shared.lock().notes.take().unwrap_or_default()
        }

        /// Adds a note, formatting it only during an explicit diagnostic capture.
        pub(crate) fn note(&self, make: impl FnOnce() -> String) {
            if let Some(notes) = self.shared.lock().notes.as_mut() {
                if notes.len() < 100_000 {
                    notes.push(make());
                }
            }
        }

        /// Opens a scope named `name` under the innermost open one; it closes when the
        /// returned guard drops. Prefer [`profile_scope!`](crate::profile), which keeps the
        /// guard to the end of the block.
        #[must_use = "the scope closes when the guard drops"]
        pub fn scope(&self, name: &'static str) -> ProfileScope {
            let start_ns = self.shared.now_ns();
            let counters = self.shared.counters();
            let mut state = self.shared.lock();
            let parent = state.stack.last().map(|f| f.node);
            let depth = state.stack.len();
            if let Some(node) = state.child(parent, name) {
                state.stack.push(Frame {
                    node,
                    start_ns,
                    counters,
                });
            }
            drop(state);
            ProfileScope {
                shared: Arc::clone(&self.shared),
                depth,
            }
        }

        /// Adds `n` to `counter`.
        #[inline]
        pub fn count(&self, counter: Counter, n: u64) {
            if let Some(c) = self.shared.counters.get(counter.index()) {
                c.fetch_add(n, Ordering::Relaxed);
            }
        }

        /// The records so far, which the profiler then forgets (its counters keep
        /// running). Call it outside every scope: a scope open across the call is
        /// recorded in neither profile.
        #[must_use]
        pub fn take(&self) -> Profile {
            let state = core::mem::take(&mut *self.shared.lock());
            Profile::from_nodes(&state.nodes, self.shared.now_ns().is_some())
        }
    }

    /// An open profiler scope; it closes when dropped (see [`Profiler::scope`]).
    #[derive(Debug)]
    pub struct ProfileScope {
        shared: Arc<Shared>,
        /// The stack depth below this scope's frame.
        depth: usize,
    }

    impl Drop for ProfileScope {
        fn drop(&mut self) {
            let end_ns = self.shared.now_ns();
            let counters = self.shared.counters();
            let mut state = self.shared.lock();
            // (scopes close in reverse order; a frame above this one, whose guard was
            // leaked, closes with it)
            while state.stack.len() > self.depth {
                let Some(frame) = state.stack.pop() else {
                    break;
                };
                state.close(&frame, end_ns, &counters);
            }
        }
    }

    /// One call path of a [`Profile`] (a `scope` line of the JSONL).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ScopeRecord {
        /// The path's index in [`Profile::scopes`].
        pub id: u32,
        /// The enclosing path's id; `None` at the root.
        pub parent: Option<u32>,
        /// The number of enclosing scopes.
        pub depth: u32,
        /// The scope's name.
        pub name: &'static str,
        /// How often the path ran.
        pub calls: u64,
        /// When it first began, from the profiler's creation (`None` without a clock).
        pub start_ns: Option<u64>,
        /// Its total time (`None` without a clock).
        pub dur_ns: Option<u64>,
        /// Its total time less its children's (`None` without a clock).
        pub self_ns: Option<u64>,
        /// The counted work inside it, children included, in [`Counter::ALL`] order.
        pub counters: [u64; COUNTERS],
    }

    /// One name of a [`Profile`] over all its paths (a `summary` line of the JSONL).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct NameSummary {
        /// The scope's name.
        pub name: &'static str,
        /// How often it ran.
        pub count: u64,
        /// The time inside it, a recursive name counted at its outermost level only
        /// (`None` without a clock).
        pub total_ns: Option<u64>,
        /// The time inside it less its children's (`None` without a clock).
        pub self_ns: Option<u64>,
        /// The counted work inside it less its children's, in [`Counter::ALL`] order.
        pub self_counters: [u64; COUNTERS],
    }

    /// What a [`Profiler`] recorded ([`crate::Kernel::take_profile`]).
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct Profile {
        /// The call paths, parents before their children.
        pub scopes: Vec<ScopeRecord>,
        /// One entry per name, the most self time first (by name without a clock).
        pub summary: Vec<NameSummary>,
    }

    impl Profile {
        fn from_nodes(nodes: &[Node], timed: bool) -> Self {
            let time = |ns: u64| timed.then_some(ns);
            let mut scopes = Vec::with_capacity(nodes.len());
            let mut summary: Vec<NameSummary> = Vec::new();
            for (id, node) in (0u32..).zip(nodes) {
                let self_ns = node.total_ns.saturating_sub(node.child_ns);
                let mut self_counters = [0u64; COUNTERS];
                for ((s, c), ch) in self_counters
                    .iter_mut()
                    .zip(&node.counters)
                    .zip(&node.child_counters)
                {
                    *s = c.saturating_sub(*ch);
                }
                scopes.push(ScopeRecord {
                    id,
                    parent: node.parent,
                    depth: node.depth,
                    name: node.name,
                    calls: node.calls,
                    start_ns: node.first_start_ns.filter(|_| timed),
                    dur_ns: time(node.total_ns),
                    self_ns: time(self_ns),
                    counters: node.counters,
                });
                // a recursive name's time counts once, at its outermost path
                let mut outermost = true;
                let mut up = node.parent;
                while let Some(p) = up.and_then(|p| nodes.get(p as usize)) {
                    if p.name == node.name {
                        outermost = false;
                        break;
                    }
                    up = p.parent;
                }
                let entry = if let Some(e) = summary.iter_mut().find(|e| e.name == node.name) {
                    e
                } else {
                    summary.push(NameSummary {
                        name: node.name,
                        count: 0,
                        total_ns: time(0),
                        self_ns: time(0),
                        self_counters: [0; COUNTERS],
                    });
                    let Some(e) = summary.last_mut() else {
                        continue;
                    };
                    e
                };
                entry.count = entry.count.saturating_add(node.calls);
                if outermost {
                    entry.total_ns = entry.total_ns.map(|t| t.saturating_add(node.total_ns));
                }
                entry.self_ns = entry.self_ns.map(|t| t.saturating_add(self_ns));
                for (e, s) in entry.self_counters.iter_mut().zip(&self_counters) {
                    *e = e.saturating_add(*s);
                }
            }
            summary.sort_by(|a, b| b.self_ns.cmp(&a.self_ns).then(a.name.cmp(b.name)));
            Self { scopes, summary }
        }

        /// Writes the profile as JSON Lines (the format in the module docs): every
        /// `scope` line, then every `summary` line.
        ///
        /// # Errors
        /// The writer's.
        pub fn write_jsonl(&self, w: &mut impl Write) -> io::Result<()> {
            for s in &self.scopes {
                write!(w, "{{\"type\":\"scope\",\"id\":{},\"parent\":", s.id)?;
                match s.parent {
                    Some(p) => write!(w, "{p}")?,
                    None => w.write_all(b"null")?,
                }
                write!(w, ",\"depth\":{},\"name\":", s.depth)?;
                write_json_str(w, s.name)?;
                write!(w, ",\"calls\":{}", s.calls)?;
                write_ns(w, "start_ns", s.start_ns)?;
                write_ns(w, "dur_ns", s.dur_ns)?;
                write_ns(w, "self_ns", s.self_ns)?;
                write_counters(w, "counters", &s.counters)?;
                w.write_all(b"}\n")?;
            }
            for e in &self.summary {
                w.write_all(b"{\"type\":\"summary\",\"name\":")?;
                write_json_str(w, e.name)?;
                write!(w, ",\"count\":{}", e.count)?;
                write_ns(w, "total_ns", e.total_ns)?;
                write_ns(w, "self_ns", e.self_ns)?;
                write_counters(w, "self_counters", &e.self_counters)?;
                w.write_all(b"}\n")?;
            }
            Ok(())
        }
    }

    fn write_ns(w: &mut impl Write, key: &str, ns: Option<u64>) -> io::Result<()> {
        match ns {
            Some(ns) => write!(w, ",\"{key}\":{ns}"),
            None => Ok(()),
        }
    }

    fn write_counters(w: &mut impl Write, key: &str, counters: &[u64; COUNTERS]) -> io::Result<()> {
        write!(w, ",\"{key}\":{{")?;
        let mut first = true;
        for (c, n) in Counter::ALL.iter().zip(counters) {
            if *n == 0 {
                continue;
            }
            if !first {
                w.write_all(b",")?;
            }
            first = false;
            write!(w, "\"{}\":{n}", c.name())?;
        }
        w.write_all(b"}")
    }

    fn write_json_str(w: &mut impl Write, s: &str) -> io::Result<()> {
        w.write_all(b"\"")?;
        for ch in s.chars() {
            match ch {
                '"' => w.write_all(b"\\\"")?,
                '\\' => w.write_all(b"\\\\")?,
                c if u32::from(c) < 0x20 => write!(w, "\\u{:04x}", u32::from(c))?,
                c => write!(w, "{c}")?,
            }
        }
        w.write_all(b"\"")
    }

    #[cfg(test)]
    mod tests {
        use crate::Kernel;

        fn inner(k: &mut Kernel, fail: bool) -> Result<(), &'static str> {
            profile_scope!(k, "inner");
            profile_count!(k, Evaluations, 3);
            if fail {
                return Err("an early return closes the scope");
            }
            k.memoize_curve_caches = !k.memoize_curve_caches;
            Ok(())
        }

        fn outer(k: &mut Kernel) {
            profile_scope!(k, "outer");
            let _ = inner(k, false);
            let _ = inner(k, true);
            profile_count!(k, Evaluations, 1);
        }

        #[test]
        fn scopes_aggregate_by_path_and_close_on_early_returns() {
            let mut k = Kernel::default();
            outer(&mut k);
            outer(&mut k);
            {
                profile_scope!(k, "inner");
            }
            let p = k.take_profile();
            let names: Vec<_> = p
                .scopes
                .iter()
                .map(|s| (s.name, s.parent, s.depth, s.calls))
                .collect();
            assert_eq!(
                names,
                [
                    ("outer", None, 0, 2),
                    ("inner", Some(0), 1, 4),
                    ("inner", None, 0, 1)
                ]
            );
            assert_eq!(p.scopes[0].counters[0], 14);
            assert_eq!(p.scopes[1].counters[0], 12);
            let outer = p.summary.iter().find(|e| e.name == "outer").unwrap();
            assert_eq!(outer.self_counters[0], 2);
            let inner = p.summary.iter().find(|e| e.name == "inner").unwrap();
            assert_eq!(inner.count, 5);
            assert!(k.take_profile().scopes.is_empty());
        }

        #[test]
        fn self_time_excludes_children_and_recursion_counts_once() {
            fn recurse(k: &mut Kernel, n: u32) {
                profile_scope!(k, "recurse");
                if n > 0 {
                    recurse(k, n - 1);
                }
            }
            let mut k = Kernel::default();
            recurse(&mut k, 3);
            let p = k.take_profile();
            assert_eq!(p.scopes.len(), 4);
            let e = &p.summary[0];
            assert_eq!((e.name, e.count), ("recurse", 4));
            #[cfg(not(target_arch = "wasm32"))]
            {
                assert_eq!(e.total_ns, p.scopes[0].dur_ns);
                let self_sum: u64 = p.scopes.iter().map(|s| s.self_ns.unwrap()).sum();
                assert_eq!(e.self_ns, Some(self_sum));
                assert!(self_sum <= p.scopes[0].dur_ns.unwrap());
            }
        }

        #[test]
        fn jsonl_lines_are_json_objects() {
            let mut k = Kernel::default();
            outer(&mut k);
            let mut out = Vec::new();
            k.take_profile().write_jsonl(&mut out).unwrap();
            let text = String::from_utf8(out).unwrap();
            let lines: Vec<_> = text.lines().collect();
            assert_eq!(lines.len(), 4);
            for line in lines {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                assert!(v["name"].is_string());
            }
            assert!(text.contains("\"counters\":{\"evaluations\":7}"));
        }
    }
}
