//! Where a command's time goes inside the kernel (#1833): wall time in booleans and in
//! tessellation, measured on this thread while [`capture`] runs and not at all otherwise (one
//! thread-local check per timed call).
//!
//! Only the outermost timed call counts: a tessellation a boolean does to check its result is
//! the boolean's time, so the two never add up to more than the command took. Work a call hands
//! to other threads is inside that call's wall time.

use std::cell::Cell;

/// Kernel time spent while a [`capture`] ran, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KernelTimes {
    pub boolean_ms: f64,
    pub tessellate_ms: f64,
}

/// What a timed call is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Boolean,
    Tessellate,
}

/// Opt-in stage timings for difficult booleans. No clocks or topology walks when disabled;
/// stderr failures are ignored so a detached diagnostic pipe cannot interrupt modelling.
pub(crate) struct BooleanTrace {
    started: Option<web_time::Instant>,
    last_ms: f64,
    id: usize,
    kind: &'static str,
    op: crate::BoolOp,
    faces: (usize, usize),
}

impl BooleanTrace {
    pub(crate) fn new(kind: &'static str, a: &crate::Body, b: &crate::Body, op: crate::BoolOp) -> Self {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        let enabled = *ENABLED.get_or_init(|| solidcraft_geom::env::var("BOOLEAN_TRACE").is_some_and(|v| v == "1"));
        let mut trace = Self {
            started: enabled.then(web_time::Instant::now),
            last_ms: 0.,
            id: if enabled { NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) } else { 0 },
            kind,
            op,
            faces: if enabled { (a.face_count(), b.face_count()) } else { (0, 0) },
        };
        trace.mark("start");
        trace
    }

    pub(crate) fn mark(&mut self, stage: &'static str) {
        use std::io::Write;
        let Some(started) = &self.started else { return };
        let ms = started.elapsed().as_secs_f64() * 1000.;
        let _ = writeln!(
            std::io::stderr().lock(),
            "boolean-trace id={} kind={} op={:?} faces={},{} stage={stage} ms={:.3} total_ms={ms:.3}",
            self.id,
            self.kind,
            self.op,
            self.faces.0,
            self.faces.1,
            ms - self.last_ms
        );
        self.last_ms = ms;
    }
}

impl Drop for BooleanTrace {
    fn drop(&mut self) {
        self.mark("done");
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    /// A capture is running on this thread.
    on: bool,
    /// A timed call is running (nested ones aren't timed).
    busy: bool,
    times: KernelTimes,
}

thread_local! {
    static STATE: Cell<State> = const { Cell::new(State { on: false, busy: false, times: KernelTimes { boolean_ms: 0.0, tessellate_ms: 0.0 } }) };
}

/// Run `f`, and the kernel time it spent in booleans and tessellation on this thread. Captures
/// nest: an inner capture's time is the outer one's too.
pub fn capture<T>(f: impl FnOnce() -> T) -> (T, KernelTimes) {
    struct Restore(State);
    impl Drop for Restore {
        fn drop(&mut self) {
            let inner = STATE.with(Cell::get).times;
            let mut outer = self.0;
            if outer.on {
                outer.times.boolean_ms += inner.boolean_ms;
                outer.times.tessellate_ms += inner.tessellate_ms;
            }
            STATE.with(|s| s.set(outer));
        }
    }
    let outer = STATE.with(Cell::get);
    STATE.with(|s| s.set(State { on: true, busy: outer.busy, times: KernelTimes::default() }));
    let restore = Restore(outer);
    let r = f();
    let times = STATE.with(Cell::get).times;
    drop(restore);
    (r, times)
}

/// Count kernel time spent on other threads for this thread's running capture (when one runs
/// and no timed call does): work this thread handed out and waited for. The caller keeps the
/// sum within the wall time it waited, so the totals still never pass what the command took.
pub fn credit(times: KernelTimes) {
    let ok = |x: f64| if x.is_finite() && x > 0.0 { x } else { 0.0 };
    STATE.with(|s| {
        let mut st = s.get();
        if st.on && !st.busy {
            st.times.boolean_ms += ok(times.boolean_ms);
            st.times.tessellate_ms += ok(times.tessellate_ms);
            s.set(st);
        }
    });
}

/// Time `f` as `kind` when a capture is running and no timed call is.
pub(crate) fn timed<T>(kind: Kind, f: impl FnOnce() -> T) -> T {
    let st = STATE.with(Cell::get);
    if !st.on || st.busy {
        return f();
    }
    struct Busy {
        kind: Kind,
        started: web_time::Instant,
    }
    impl Drop for Busy {
        fn drop(&mut self) {
            let ms = self.started.elapsed().as_secs_f64() * 1000.0;
            STATE.with(|s| {
                let mut st = s.get();
                st.busy = false;
                match self.kind {
                    Kind::Boolean => st.times.boolean_ms += ms,
                    Kind::Tessellate => st.times.tessellate_ms += ms,
                }
                s.set(st);
            });
        }
    }
    STATE.with(|s| s.set(State { busy: true, ..st }));
    let busy = Busy { kind, started: web_time::Instant::now() };
    let r = f();
    drop(busy);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin(ms: u64) {
        let t = web_time::Instant::now();
        while t.elapsed().as_millis() < u128::from(ms) {}
    }

    #[test]
    fn nothing_is_timed_outside_a_capture() {
        timed(Kind::Boolean, || spin(2));
        let ((), t) = capture(|| ());
        assert_eq!(t, KernelTimes::default());
    }

    #[test]
    fn only_the_outermost_call_counts() {
        let started = web_time::Instant::now();
        let ((), t) = capture(|| {
            timed(Kind::Boolean, || {
                spin(3);
                timed(Kind::Tessellate, || spin(3));
            });
            timed(Kind::Tessellate, || spin(2));
        });
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        assert!(t.boolean_ms >= 6.0 && t.tessellate_ms >= 2.0, "{t:?}");
        // The tessellation inside the boolean is not counted twice.
        assert!(t.boolean_ms + t.tessellate_ms <= elapsed + 1e-6, "{t:?} in {elapsed} ms");
    }

    #[test]
    fn time_credited_from_other_threads_counts_only_inside_a_capture() {
        let t = KernelTimes { boolean_ms: 4.0, tessellate_ms: 1.0 };
        credit(t);
        let ((), got) = capture(|| {
            credit(t);
            credit(KernelTimes { boolean_ms: f64::NAN, tessellate_ms: -3.0 });
            // Inside a timed call it would count twice: it is the call's time already.
            timed(Kind::Boolean, || credit(t));
        });
        assert_eq!((got.boolean_ms >= 4.0, got.boolean_ms < 8.0, got.tessellate_ms), (true, true, 1.0), "{got:?}");
    }

    #[test]
    fn inner_captures_add_to_the_outer_one_and_a_panic_restores_it() {
        let ((), outer) = capture(|| {
            let ((), inner) = capture(|| timed(Kind::Boolean, || spin(2)));
            assert!(inner.boolean_ms >= 2.0);
            let _ = std::panic::catch_unwind(|| capture(|| timed(Kind::Tessellate, || panic!("kernel"))));
        });
        assert!(outer.boolean_ms >= 2.0, "{outer:?}");
        let st = STATE.with(Cell::get);
        assert!(!st.on && !st.busy);
    }
    #[test]
    fn a_faceted_boolean_is_boolean_time_and_a_reference_mesh_tessellation() {
        use crate::{BoolOp, box_solid, faceted_boolean};
        use solidcraft_geom::Vec3;
        let a = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let tool = box_solid(Vec3::new(5.0, 5.0, 5.0), Vec3::new(15.0, 15.0, 15.0)).unwrap().tessellate(0.1).unwrap();
        // The thread groove's route: the faceting it does first is the boolean's time too.
        let (r, t) = capture(|| faceted_boolean(&a, &tool, BoolOp::Cut, 0.01));
        assert!(r.unwrap().is_some());
        assert!(t.boolean_ms > 0.0 && t.tessellate_ms == 0.0, "{t:?}");
        let (m, t) = capture(|| a.reference_display_mesh_angular(0.01, crate::DISPLAY_ANGLE));
        assert!(!m.unwrap().triangles.is_empty());
        assert!(t.tessellate_ms > 0.0 && t.boolean_ms == 0.0, "{t:?}");
    }
}
