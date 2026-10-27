//! Where SolidCraft spends its time: nested, timed spans on every thread, for SMLib's C API
//! calls (timed by the C API itself), kernel operations, feature evaluation, commands and the
//! stages of a STEP or IGES import.
//!
//! - **Compiled in** with the `enabled` feature, which every app turns on by default (its own
//!   `profile` feature), so a release can be profiled; a build without it records nothing and
//!   every call here is an empty inline function.
//! - **Off at run time** until [`set_enabled`] (`SOLIDCRAFT_PROFILE=1`, the CLI's `--profile`, the
//!   app's preference, the `diagnostics.profile` command). Off, a span costs one relaxed atomic
//!   load.
//! - **Two outputs:** an aggregate per span name ([`report`]: count, total, self time, longest),
//!   kept for the whole run, and the individual spans as a Chrome trace-event file
//!   ([`write_chrome_trace`], for chrome://tracing or Perfetto), capped at [`MAX_EVENTS`]
//!   (later spans still count in the aggregate; the report says how many were dropped).
//!
//! A span's self time is its time less its children's on the same thread. Its total counts only
//! its outermost occurrence on a thread's stack, so a command run by a command, or a boolean
//! inside a boolean, is not counted twice. Work a span hands to other threads shows there, under
//! those threads' own spans.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::time::Duration;

/// The profiler is compiled into this build.
pub const COMPILED: bool = cfg!(any(feature = "enabled", test));

/// Spans kept for the trace; later ones count in the aggregate only.
pub const MAX_EVENTS: usize = 200_000;

/// Distinct names [`intern`] keeps; later new names are all [`OTHER`].
pub const MAX_NAMES: usize = 4096;

/// The name of a span whose own name did not fit in [`MAX_NAMES`].
pub const OTHER: &str = "other";

/// One span name's aggregate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Row {
    /// What the span is: `smlib`, `kernel`, `feature`, `command`, `import`, …
    pub category: &'static str,
    pub name: &'static str,
    /// Times the span ran.
    pub count: u64,
    /// Its time, counting only its outermost occurrence on each thread's stack.
    pub total: Duration,
    /// Its time less the time of the spans inside it.
    pub self_time: Duration,
    /// Its longest single run.
    pub max: Duration,
}

/// What the profiler has recorded since it was last reset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// The profiler is compiled in.
    pub compiled: bool,
    /// It is recording now.
    pub enabled: bool,
    /// Wall time since the last reset (or since recording first started).
    pub wall: Duration,
    /// One row per span name, in no particular order.
    pub rows: Vec<Row>,
    /// Spans kept for the trace.
    pub events: usize,
    /// Spans past [`MAX_EVENTS`], left out of the trace (they are in the rows).
    pub dropped: u64,
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1e6).round() / 1e3
}

impl Report {
    /// The rows, the most self time first.
    pub fn sorted(&self) -> Vec<Row> {
        let mut rows = self.rows.clone();
        rows.sort_by(|a, b| b.self_time.cmp(&a.self_time).then(b.total.cmp(&a.total)).then(a.name.cmp(b.name)));
        rows
    }

    /// Self time per category, the most first: where the time went (SMLib or SolidCraft).
    pub fn by_category(&self) -> Vec<(&'static str, Duration)> {
        let mut out: Vec<(&'static str, Duration)> = Vec::new();
        for r in &self.rows {
            match out.iter_mut().find(|(c, _)| *c == r.category) {
                Some((_, d)) => *d += r.self_time,
                None => out.push((r.category, r.self_time)),
            }
        }
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        out
    }

    /// What was recorded after `earlier` (a report of the same run, taken before): counts and
    /// times are the differences. `max` is the longest run of the whole recording, which may
    /// be older than `earlier`.
    pub fn since(&self, earlier: &Report) -> Report {
        let rows = self
            .rows
            .iter()
            .filter_map(|r| {
                let before = earlier.rows.iter().find(|e| e.category == r.category && e.name == r.name);
                let row = match before {
                    Some(e) => Row {
                        count: r.count.saturating_sub(e.count),
                        total: r.total.saturating_sub(e.total),
                        self_time: r.self_time.saturating_sub(e.self_time),
                        ..*r
                    },
                    None => *r,
                };
                (row.count > 0).then_some(row)
            })
            .collect();
        Report {
            compiled: self.compiled,
            enabled: self.enabled,
            wall: self.wall.saturating_sub(earlier.wall),
            rows,
            events: self.events.saturating_sub(earlier.events),
            dropped: self.dropped.saturating_sub(earlier.dropped),
        }
    }

    /// The report as a text table with the `top` rows of most self time (0: all of them).
    pub fn table(&self, top: usize) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        if !self.compiled {
            s.push_str("profile: not compiled into this build (the `profile` feature is off)\n");
            return s;
        }
        let spans = self.events as u64 + self.dropped;
        let _ = writeln!(s, "profile: {:.1} ms wall, {spans} spans ({} not kept for the trace)", ms(self.wall), self.dropped);
        let all: f64 = self.rows.iter().map(|r| r.self_time.as_secs_f64()).sum();
        let parts: Vec<String> = self
            .by_category()
            .into_iter()
            .map(|(c, d)| {
                let share = if all > 0.0 { d.as_secs_f64() / all * 100.0 } else { 0.0 };
                format!("{c} {:.1} ms ({share:.1}%)", ms(d))
            })
            .collect();
        let _ = writeln!(s, "self time by category: {}", if parts.is_empty() { "nothing recorded".to_string() } else { parts.join(", ") });
        let sorted = self.sorted();
        let n = if top == 0 { sorted.len() } else { top.min(sorted.len()) };
        let _ = writeln!(s, "{:>11} {:>11} {:>8} {:>10}  span", "self ms", "total ms", "count", "max ms");
        for r in sorted.iter().take(n) {
            let _ = writeln!(s, "{:>11.3} {:>11.3} {:>8} {:>10.3}  {} {}", ms(r.self_time), ms(r.total), r.count, ms(r.max), r.category, r.name);
        }
        if n < sorted.len() {
            let _ = writeln!(s, "({} more span names)", sorted.len() - n);
        }
        s
    }
}

/// A point in the recording to report from ([`Mark::report`]): an import's own share.
#[derive(Clone, Debug)]
pub struct Mark(Report);

/// Mark the recording here.
pub fn mark() -> Mark {
    Mark(report())
}

impl Mark {
    /// What was recorded since the mark (all threads: work running beside it counts too).
    pub fn report(&self) -> Report {
        report().since(&self.0)
    }
}

/// A running span; it ends when dropped (also while a panic unwinds).
#[must_use = "a span measures until it is dropped"]
pub struct Span {
    #[cfg(any(feature = "enabled", test))]
    open: Option<imp::Open>,
}

impl Drop for Span {
    #[inline]
    fn drop(&mut self) {
        #[cfg(any(feature = "enabled", test))]
        if let Some(open) = self.open.take() {
            imp::close(&open);
        }
    }
}

/// Start a span `name` of `category` on this thread, when recording.
#[inline]
pub fn span(category: &'static str, name: &'static str) -> Span {
    #[cfg(any(feature = "enabled", test))]
    {
        Span { open: if enabled() { imp::open((category, name)) } else { None } }
    }
    #[cfg(not(any(feature = "enabled", test)))]
    {
        let _ = (category, name);
        Span {}
    }
}

/// Run `f` as a span.
#[inline]
pub fn scope<T>(category: &'static str, name: &'static str, f: impl FnOnce() -> T) -> T {
    let _span = span(category, name);
    f()
}

/// Record a span that just ended after `took`, timed by someone else (SMLib's C API times its
/// own calls): a child of the span running on this thread.
#[inline]
pub fn record(category: &'static str, name: &'static str, took: Duration) {
    #[cfg(any(feature = "enabled", test))]
    if enabled() {
        imp::record((category, name), u64::try_from(took.as_nanos()).unwrap_or(u64::MAX));
    }
    #[cfg(not(any(feature = "enabled", test)))]
    let _ = (category, name, took);
}

/// `name` as a name a span can take, kept for the rest of the run (at most [`MAX_NAMES`]
/// distinct ones; after that [`OTHER`]). For names only known at run time, such as SMLib's C
/// function names; spans named in code use literals.
pub fn intern(name: &str) -> &'static str {
    #[cfg(any(feature = "enabled", test))]
    {
        imp::intern(name)
    }
    #[cfg(not(any(feature = "enabled", test)))]
    {
        let _ = name;
        OTHER
    }
}

/// Is the profiler recording?
#[inline]
pub fn enabled() -> bool {
    #[cfg(any(feature = "enabled", test))]
    {
        imp::ON.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(any(feature = "enabled", test)))]
    {
        false
    }
}

/// Start or stop recording (nothing happens when the profiler is not compiled in). What was
/// recorded stays until [`reset`].
pub fn set_enabled(on: bool) {
    #[cfg(any(feature = "enabled", test))]
    imp::set_enabled(on);
    #[cfg(not(any(feature = "enabled", test)))]
    let _ = on;
}

/// Forget what was recorded; the wall time starts again now.
pub fn reset() {
    #[cfg(any(feature = "enabled", test))]
    imp::reset();
}

/// What was recorded since the last reset.
pub fn report() -> Report {
    #[cfg(any(feature = "enabled", test))]
    {
        imp::report()
    }
    #[cfg(not(any(feature = "enabled", test)))]
    {
        Report::default()
    }
}

/// Write the kept spans as a Chrome trace-event file (JSON object format: complete events, one
/// track per thread), for chrome://tracing or Perfetto.
pub fn write_chrome_trace(w: &mut dyn std::io::Write) -> std::io::Result<()> {
    #[cfg(any(feature = "enabled", test))]
    {
        imp::write_chrome_trace(w)
    }
    #[cfg(not(any(feature = "enabled", test)))]
    {
        w.write_all(br#"{"displayTimeUnit":"ms","traceEvents":[]}"#)
    }
}

#[cfg(any(feature = "enabled", test))]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
    use std::time::Duration;

    use serde_json::json;
    use web_time::Instant;

    use super::{MAX_EVENTS, MAX_NAMES, OTHER, Report, Row};

    pub(super) static ON: AtomicBool = AtomicBool::new(false);

    type Key = (&'static str, &'static str);

    /// Deeper spans on one thread are not recorded (a runaway recursion stays bounded).
    const MAX_DEPTH: usize = 512;
    /// Threads named in the trace.
    const MAX_THREADS: usize = 4096;

    #[derive(Clone, Copy, Default)]
    struct Agg {
        count: u64,
        total_ns: u64,
        self_ns: u64,
        max_ns: u64,
    }

    #[derive(Clone, Copy)]
    struct Event {
        key: Key,
        tid: u32,
        start_ns: u64,
        dur_ns: u64,
    }

    #[derive(Default)]
    struct Global {
        agg: HashMap<Key, Agg>,
        events: Vec<Event>,
        dropped: u64,
        threads: Vec<(u32, String)>,
        next_tid: u32,
        since_ns: u64,
    }

    static GLOBAL: LazyLock<Mutex<Global>> = LazyLock::new(|| Mutex::new(Global::default()));
    static NAMES: LazyLock<Mutex<HashMap<String, &'static str>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

    fn global() -> MutexGuard<'static, Global> {
        // A panic while the lock was held leaves plain data behind: keep using it.
        GLOBAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn epoch() -> Instant {
        static EPOCH: OnceLock<Instant> = OnceLock::new();
        *EPOCH.get_or_init(Instant::now)
    }

    fn now_ns() -> u64 {
        u64::try_from(epoch().elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    struct Frame {
        key: Key,
        child_ns: u64,
    }

    thread_local! {
        static STACK: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
        /// This thread's number in the trace (0: not given one yet).
        static TID: Cell<u32> = const { Cell::new(0) };
    }

    pub(super) struct Open {
        key: Key,
        start_ns: u64,
        depth: usize,
    }

    pub(super) fn open(key: Key) -> Option<Open> {
        let depth = STACK
            .try_with(|s| {
                let mut s = s.try_borrow_mut().ok()?;
                if s.len() >= MAX_DEPTH {
                    return None;
                }
                s.push(Frame { key, child_ns: 0 });
                Some(s.len() - 1)
            })
            .ok()
            .flatten()?;
        Some(Open { key, start_ns: now_ns(), depth })
    }

    pub(super) fn close(o: &Open) {
        let dur = now_ns().saturating_sub(o.start_ns);
        let (child, outermost) = STACK
            .try_with(|s| {
                let Ok(mut s) = s.try_borrow_mut() else { return (0, true) };
                let child = s.get(o.depth).map_or(0, |f| f.child_ns);
                // Spans opened inside this one and not closed yet (leaked) end with it.
                s.truncate(o.depth);
                let outermost = !s.iter().any(|f| f.key == o.key);
                if let Some(parent) = s.last_mut() {
                    parent.child_ns = parent.child_ns.saturating_add(dur);
                }
                (child, outermost)
            })
            .unwrap_or((0, true));
        add(o.key, o.start_ns, dur, dur.saturating_sub(child), outermost);
    }

    pub(super) fn record(key: Key, dur: u64) {
        let start = now_ns().saturating_sub(dur);
        let outermost = STACK
            .try_with(|s| {
                let Ok(mut s) = s.try_borrow_mut() else { return true };
                let outermost = !s.iter().any(|f| f.key == key);
                if let Some(parent) = s.last_mut() {
                    parent.child_ns = parent.child_ns.saturating_add(dur);
                }
                outermost
            })
            .unwrap_or(true);
        add(key, start, dur, dur, outermost);
    }

    fn thread_id(g: &mut Global) -> u32 {
        TID.try_with(|t| {
            if t.get() == 0 {
                g.next_tid = g.next_tid.saturating_add(1);
                t.set(g.next_tid);
                if g.threads.len() < MAX_THREADS {
                    g.threads.push((g.next_tid, thread_name(g.next_tid)));
                }
            }
            t.get()
        })
        .unwrap_or(0)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn thread_name(tid: u32) -> String {
        std::thread::current().name().map_or_else(|| format!("thread {tid}"), str::to_string)
    }

    #[cfg(target_arch = "wasm32")]
    fn thread_name(_: u32) -> String {
        "main".into()
    }

    fn add(key: Key, start_ns: u64, dur_ns: u64, self_ns: u64, outermost: bool) {
        let mut g = global();
        let tid = thread_id(&mut g);
        let a = g.agg.entry(key).or_default();
        a.count = a.count.saturating_add(1);
        if outermost {
            a.total_ns = a.total_ns.saturating_add(dur_ns);
        }
        a.self_ns = a.self_ns.saturating_add(self_ns);
        a.max_ns = a.max_ns.max(dur_ns);
        if g.events.len() < MAX_EVENTS {
            g.events.push(Event { key, tid, start_ns, dur_ns });
        } else {
            g.dropped = g.dropped.saturating_add(1);
        }
    }

    pub(super) fn intern(name: &str) -> &'static str {
        let mut names = NAMES.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(n) = names.get(name) {
            return n;
        }
        if names.len() >= MAX_NAMES {
            return OTHER;
        }
        let kept: &'static str = Box::leak(name.to_string().into_boxed_str());
        names.insert(name.to_string(), kept);
        kept
    }

    pub(super) fn set_enabled(on: bool) {
        let was = ON.swap(on, Ordering::Relaxed);
        if on && !was {
            let now = now_ns();
            let mut g = global();
            // The wall time starts with the first recording.
            if g.agg.is_empty() {
                g.since_ns = now;
            }
        }
    }

    pub(super) fn reset() {
        let now = now_ns();
        let mut g = global();
        g.agg.clear();
        g.events = Vec::new();
        g.dropped = 0;
        g.since_ns = now;
    }

    pub(super) fn report() -> Report {
        let now = now_ns();
        let g = global();
        let d = Duration::from_nanos;
        let rows = g
            .agg
            .iter()
            .map(|(k, a)| Row { category: k.0, name: k.1, count: a.count, total: d(a.total_ns), self_time: d(a.self_ns), max: d(a.max_ns) })
            .collect();
        Report {
            compiled: true,
            enabled: ON.load(Ordering::Relaxed),
            wall: d(now.saturating_sub(g.since_ns)),
            rows,
            events: g.events.len(),
            dropped: g.dropped,
        }
    }

    pub(super) fn write_chrome_trace(w: &mut dyn std::io::Write) -> std::io::Result<()> {
        // Copied out, so spans closing on other threads don't wait for the file.
        let (events, threads, dropped) = {
            let g = global();
            (g.events.clone(), g.threads.clone(), g.dropped)
        };
        let us = |ns: u64| ns as f64 / 1000.0;
        write!(w, "{{\"displayTimeUnit\":\"ms\",\"otherData\":{{\"dropped\":{dropped}}},\"traceEvents\":[")?;
        let mut first = true;
        let mut sep = |w: &mut dyn std::io::Write| -> std::io::Result<()> {
            if !first {
                w.write_all(b",\n")?;
            }
            first = false;
            Ok(())
        };
        sep(w)?;
        serde_json::to_writer(&mut *w, &json!({"ph": "M", "name": "process_name", "pid": 1, "tid": 0, "args": {"name": "SolidCraft"}}))?;
        for (tid, name) in &threads {
            sep(w)?;
            serde_json::to_writer(&mut *w, &json!({"ph": "M", "name": "thread_name", "pid": 1, "tid": tid, "args": {"name": name}}))?;
        }
        for e in &events {
            sep(w)?;
            serde_json::to_writer(
                &mut *w,
                &json!({"ph": "X", "cat": e.key.0, "name": e.key.1, "pid": 1, "tid": e.tid, "ts": us(e.start_ns), "dur": us(e.dur_ns)}),
            )?;
        }
        w.write_all(b"]}\n")
    }
}

#[cfg(test)]
mod tests;
