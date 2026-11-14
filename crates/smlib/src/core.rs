//! The solidcore backend: one kernel for the process (SMLib's context is process-global too, and
//! some of its state, such as the fillet solver's latched step, lives for the whole process),
//! every call under its lock and a panic guard.
//!
//! Upstream fixes ([`fixes`]) are solidcore's opt-in corrections of SMLib defects that the C++
//! fork fixed in its own sources; each one turned on here says which fork commit it matches.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, PoisonError};

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::{CancelToken, Kernel, KernelOptions, Rng, UpstreamFixes};
use solidcore::mass::integrator::properties::{PropertyAttributes, brep_volume};
use solidcore::primitive::api_primitives as primitives;

use crate::{Error, Result};

pub(crate) mod blend;
#[cfg(test)]
mod blend_tests;
pub(crate) mod boolean;
#[cfg(test)]
mod boolean_tests;
mod convert;
#[cfg(test)]
mod convert_tests;
pub(crate) mod definition;
pub(crate) mod dump;
pub(crate) mod file;
pub(crate) mod history;
mod measure;
#[cfg(test)]
mod measure_tests;
pub(crate) mod merge;
#[cfg(test)]
mod merge_tests;
pub(crate) mod offset;
#[cfg(test)]
mod offset_tests;
pub(crate) mod picked;
pub(crate) mod split;
#[cfg(test)]
mod split_tests;
pub(crate) mod ssi;
pub(crate) mod sweep;
#[cfg(test)]
mod sweep_tests;
pub(crate) mod transform;
#[cfg(test)]
mod transform_tests;
pub(crate) mod trim;
#[cfg(test)]
mod trim_tests;

pub(crate) use convert::{from_arrays, to_arrays};
pub(crate) use measure::{
    area, bounds, check, check_with, classify_point, classify_points, edge_face_distance, is_manifold_solid, mass_properties, solid_count,
};

/// A solidcore body. Its own lock: solidcore's operations take `&mut Brep`, even to measure.
pub(crate) struct Body(pub(crate) Mutex<solidcore::topology::Brep>);

impl Body {
    pub(crate) fn new(b: solidcore::topology::Brep) -> Body {
        Body(Mutex::new(b))
    }
}

/// The fixes this backend runs with: solidcore's corrections as the kernel turns them on by
/// default (`UpstreamFixes::default()`), every one but those it holds back
/// (`UpstreamFixes::HELD_BACK`, each with the case and the numbers that show it broken in its
/// field's doc). A test checks none is left off but those.
pub(crate) fn fixes() -> UpstreamFixes {
    UpstreamFixes::default()
}

/// A new kernel, with this backend's fixes.
fn fresh() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes();
    k
}

/// The kernels calls run on. The lock is only held to take or put back a kernel, never during a
/// call: a call that runs long (or never returns) holds up nothing but its own body, so one
/// design's long operation never delays another's. Calls one after another all get the same
/// kernel; one that starts while another runs gets one of its own (solidcore keeps no global
/// state, and a body's identities are valid only in the kernel that issued them, so any kernel
/// takes any body).
///
/// A call stuck for good keeps its kernel and the thread it runs on, and costs nothing more: at
/// most `most_idle` kernels wait between calls, however many calls once ran side by side. A job
/// that starts again and sticks again holds one more each time, so at `most_running` calls a new
/// call fails, saying so, rather than grow without end or wait for calls that may never return.
pub(crate) struct Pool<K> {
    pub(crate) idle: Vec<K>,
    pub(crate) running: usize,
    pub(crate) most_idle: usize,
    pub(crate) most_running: usize,
}

impl<K> Pool<K> {
    /// A kernel for a call, the last used first; `None` when too many are running.
    pub(crate) fn take(&mut self, make: impl FnOnce() -> K) -> Option<K> {
        if self.running >= self.most_running {
            return None;
        }
        self.running += 1;
        Some(self.idle.pop().unwrap_or_else(make))
    }

    /// A call ended, handing back its kernel if it is fit to use again.
    pub(crate) fn put(&mut self, kernel: Option<K>) {
        self.running = self.running.saturating_sub(1);
        if let Some(kernel) = kernel
            && self.idle.len() < self.most_idle
        {
            self.idle.push(kernel);
        }
    }
}

/// Most calls at once. Far above what any work needs: an operation runs on at most 16 threads.
const MOST_RUNNING: usize = 256;

static POOL: Mutex<Pool<Kernel>> = Mutex::new(Pool { idle: Vec::new(), running: 0, most_idle: 8, most_running: MOST_RUNNING });

fn take_kernel() -> Option<Kernel> {
    POOL.lock().unwrap_or_else(PoisonError::into_inner).take(fresh)
}

fn put_kernel(k: Option<Kernel>) {
    POOL.lock().unwrap_or_else(PoisonError::into_inner).put(k);
}

/// What a call must not find from the calls before it: the same operation on the same bodies
/// answers the same on any kernel, however much that kernel did first. (Its draws of random rays,
/// the stations it remembers; marks, identities and what is only kept to reuse memory change no
/// answer. Tolerances and fixes are the same on every kernel.)
fn start_clean(k: &mut Kernel) {
    k.rng = Rng::new(KernelOptions::default().seed);
    *k.blend_station_memo.borrow_mut() = Default::default();
    k.fillet_point_step.set(None);
    k.upstream_ub_passed.clear();
}

/// A kernel kept out of use until dropped ([`crate::hold_a_kernel`]).
pub struct HeldKernel(Option<Kernel>);

impl Drop for HeldKernel {
    fn drop(&mut self) {
        put_kernel(self.0.take());
    }
}

pub(crate) fn hold_a_kernel() -> HeldKernel {
    HeldKernel(take_kernel())
}

/// Runs `f` on a kernel of its own. A panic in solidcore is caught: the kernel, which it may have
/// left half-updated, is dropped and the call fails with [`Error::Internal`]. A token over this
/// thread's work ([`crate::stop`]) stops it.
pub(crate) fn run<T: dump::Answer>(call: &'static str, f: impl FnOnce(&mut Kernel) -> std::result::Result<T, solidcore::SolidError>) -> Result<T> {
    run_on(call, &[], f)
}

thread_local! {
    /// How many solidcore calls this thread is inside: a panic now is caught ([`run_on`]).
    static IN_CALL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Is this thread inside a solidcore call, whose panic [`run_on`] catches? (Crash reports mark such
/// a panic recovered.)
pub(crate) fn in_call() -> bool {
    IN_CALL.with(std::cell::Cell::get) > 0
}

/// [`run`] for a call that reads the bodies `inputs`: should it fail while a dump folder is set,
/// they are left there ([`dump::failed`]).
pub(crate) fn run_on<T: dump::Answer>(
    call: &'static str,
    inputs: dump::Inputs<'_>,
    f: impl FnOnce(&mut Kernel) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    let stop = crate::stop::token();
    if stop.as_ref().is_some_and(CancelToken::is_cancelled) {
        return Err(Error::Cancelled(call));
    }
    call_on(call, inputs, stop, f)
}

/// [`run_on`], stopped by `stop`.
fn call_on<T: dump::Answer>(
    call: &'static str,
    inputs: dump::Inputs<'_>,
    stop: Option<CancelToken>,
    f: impl FnOnce(&mut Kernel) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    let Some(mut k) = take_kernel() else {
        log::warn!(target: "smlib", "{call}: refused, {MOST_RUNNING} kernel calls are still running");
        return Err(Error::Solidcore(format!("{call}: {MOST_RUNNING} kernel calls are still running")));
    };
    start_clean(&mut k);
    k.cancel = stop.clone();
    // A profiler span per call: where the kernel's time goes.
    let _span = solidcraft_profile::span("solidcore", call);
    let in_flight = dump::in_flight(call);
    struct Depth;
    impl Drop for Depth {
        fn drop(&mut self) {
            IN_CALL.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
    IN_CALL.with(|d| d.set(d.get() + 1));
    let depth = Depth;
    let caught = catch_unwind(AssertUnwindSafe(|| f(&mut k)));
    drop(depth);
    let r = match caught {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(Error::Solidcore(format!("{call}: {e}"))),
        Err(p) => {
            k = fresh();
            let why = p.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
            log::warn!(target: "smlib", "solidcore panicked in {call}: {why}");
            Err(Error::Internal(format!("{call}: {why}")))
        }
    };
    drop(in_flight);
    // Stopped, it is no failure to look into, and solidcore may have left the kernel part way.
    // No result either, even one solidcore finished first: the caller never has to ask whether
    // it came in time (as solidcore-ops' session answers).
    if stop.is_some_and(|s| s.is_cancelled()) {
        put_kernel(None);
        return match r {
            Err(Error::Internal(e)) => Err(Error::Internal(e)),
            _ => Err(Error::Cancelled(call)),
        };
    }
    let why = match &r {
        Ok(v) => v.refusal(),
        Err(e) => Some(e.to_string()),
    };
    if let Some(why) = why {
        // The kernel the call ran on, or the fresh one that replaced it after a panic.
        dump::failed(&k, call, &why, inputs);
    }
    k.cancel = None;
    put_kernel(Some(k));
    r
}

/// A call the capi checks for `SM_SUCCESS`, treating any other status as no answer: an error
/// that stands for an SMLib status exactly ([`SolidError::sm_status`](solidcore::SolidError::sm_status),
/// solidcore#1452) is `None`. Every other solidcore error
/// (unported code, invalid input to an adapter, a missing record...) is not a status the C++
/// path returns, so it fails the call instead of hiding as a skipped entity. So does a status
/// error whose cause chain holds a solidcore-side failure or a cancellation
/// ([`SolidError::is_status_answer`](solidcore::SolidError::is_status_answer)), the rule of
/// solidcore-ops' `session::answer`.
pub(crate) fn answer<T>(r: std::result::Result<T, solidcore::SolidError>) -> std::result::Result<Option<T>, solidcore::SolidError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_status_answer() => Ok(None),
        Err(e) => Err(e),
    }
}

/// Runs `f` on a kernel and `body`, its lock held for the call.
pub(crate) fn with_body<T: dump::Answer>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &mut solidcore::topology::Brep) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    // A body a panic interrupted may be half-edited: it is never used again.
    if body.0.is_poisoned() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    // No token stops it: stopped part way, solidcore may leave the body it borrows half-changed.
    call_on(call, &[("brep", body)], None, |k| f(k, &mut body.0.lock().unwrap_or_else(PoisonError::into_inner)))
}

/// [`with_body`] on a body only this call reads (one the caller consumes): the token over this
/// thread's work stops it, since what a stopped call leaves half-changed is never read again.
pub(crate) fn with_own_body<T: dump::Answer>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &mut solidcore::topology::Brep) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    if body.0.is_poisoned() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    let stop = crate::stop::token();
    if stop.as_ref().is_some_and(CancelToken::is_cancelled) {
        return Err(Error::Cancelled(call));
    }
    call_on(call, &[("brep", body)], stop, |k| f(k, &mut body.0.lock().unwrap_or_else(PoisonError::into_inner)))
}

/// An axis-aligned box from its lower corner (`SmApiCreateBox`, as `smlib_make_box` checks it:
/// every edge length positive).
pub(crate) fn make_box(origin: [f64; 3], size: [f64; 3]) -> Result<Body> {
    if !origin.iter().chain(&size).all(|x| x.is_finite()) || !size.iter().all(|&s| s > 0.0) {
        return Err(Error::Argument);
    }
    let [x, y, z] = size;
    run("create_box", |k| primitives::create_box(k, Vec3::new(origin[0], origin[1], origin[2]), x, y, z)).map(Body::new)
}

fn point(p: [f64; 3]) -> Result<Vec3> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    Ok(Vec3::new(p[0], p[1], p[2]))
}

/// A cylinder about +Z from its base centre (`SmApiCreateCylinder`, as `smlib_make_cylinder`
/// checks it: radius and height positive).
pub(crate) fn make_cylinder(origin: [f64; 3], radius: f64, height: f64) -> Result<Body> {
    let o = point(origin)?;
    if !(radius > 0.0 && height > 0.0 && radius.is_finite() && height.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_cylinder", |k| primitives::create_cylinder(k, o, radius, height)).map(Body::new)
}

/// A sphere (`SmApiCreateSphere`; radius positive).
pub(crate) fn make_sphere(center: [f64; 3], radius: f64) -> Result<Body> {
    let o = point(center)?;
    if !(radius > 0.0 && radius.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_sphere", |k| primitives::create_sphere(k, o, radius)).map(Body::new)
}

/// A cone or frustum about +Z (`SmApiCreateCone`; `smlib_make_cone`'s checks: base radius and
/// height positive, top radius at least 0 and not the base radius).
pub(crate) fn make_cone(origin: [f64; 3], base_radius: f64, top_radius: f64, height: f64) -> Result<Body> {
    let o = point(origin)?;
    let ok = base_radius > 0.0 && top_radius >= 0.0 && height > 0.0 && top_radius != base_radius;
    if !(ok && base_radius.is_finite() && top_radius.is_finite() && height.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_cone", |k| primitives::create_cone(k, o, base_radius, top_radius, height)).map(Body::new)
}

/// A ring torus about +Z (`SmApiCreateTorus`; major radius > minor radius > 0).
pub(crate) fn make_torus(center: [f64; 3], major_radius: f64, minor_radius: f64) -> Result<Body> {
    let o = point(center)?;
    if !(minor_radius > 0.0 && minor_radius < major_radius && major_radius.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_torus", |k| primitives::create_torus(k, o, major_radius, minor_radius)).map(Body::new)
}

/// The enclosed volume (`SmApiBrepComputeVolume`), its accuracy clamped as [`mass_properties`]
/// clamps it.
pub(crate) fn volume(body: &Body, relative_accuracy: f64) -> Result<f64> {
    let relative_accuracy = measure::accuracy(relative_accuracy)?;
    with_body("brep_volume", body, |k, b| {
        let store = AttributeStore::default();
        let attrs = PropertyAttributes { store: &store, key_of: &|_| ObjectKey(0) };
        brep_volume(k, b, relative_accuracy, &attrs)
    })
}

/// Face, edge and vertex counts.
pub(crate) fn counts(body: &Body) -> Result<[usize; 3]> {
    with_body("counts", body, |_, b| Ok([b.face_list().len(), b.edge_list().len(), b.vertex_list().len()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_upstream_fix_is_on_unless_shown_broken() {
        let f = fixes();
        let flags: Vec<(&str, bool)> = f.flags().collect();
        let off: Vec<&str> = flags.iter().filter(|(name, on)| !on && !UpstreamFixes::HELD_BACK.contains(name)).map(|(name, _)| *name).collect();
        assert!(off.is_empty(), "UpstreamFixes flags are off: {off:?}");
        for name in UpstreamFixes::HELD_BACK {
            assert!(flags.contains(&(*name, false)), "{name} is on or gone: drop it from HELD_BACK");
        }
    }

    #[test]
    fn a_box_measures_exactly() {
        let b = make_box([1.0, -2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
        assert_eq!(counts(&b).unwrap(), [6, 12, 8]);
        let v = volume(&b, 1e-8).unwrap();
        assert!((v - 6000.0).abs() < 6000.0 * 1e-9, "{v}");
    }

    /// An accuracy finer than 1e-8 or not positive is 1e-8, as `mass_properties` takes it: a
    /// sphere's volume at 0, -1 and 1e-15 is its volume at 1e-8, on its closed form.
    #[test]
    fn a_volume_accuracy_out_of_range_is_clamped() {
        let ball = make_sphere([1.0, 2.0, 3.0], 5.0).unwrap();
        let at = volume(&ball, 1e-8).unwrap();
        assert!((at - 4.0 / 3.0 * std::f64::consts::PI * 125.0).abs() < 1e-6, "{at}");
        for acc in [0.0, -1.0, 1e-15] {
            assert_eq!(volume(&ball, acc).ok(), Some(at), "accuracy {acc}");
        }
    }

    #[test]
    fn hostile_numbers_are_refused() {
        assert!(make_box([f64::NAN, 0.0, 0.0], [1.0; 3]).is_err());
        assert!(make_box([0.0; 3], [1.0, -1.0, 1.0]).is_err());
        let b = make_box([0.0; 3], [1.0; 3]).unwrap();
        assert!(volume(&b, f64::INFINITY).is_err());
    }

    #[test]
    fn a_panic_is_an_error_and_the_kernel_is_fresh_after() {
        let r: Result<()> = run("test", |_| panic!("boom"));
        assert!(matches!(r, Err(Error::Internal(ref m)) if m.contains("boom")), "{r:?}");
        assert!(make_box([0.0; 3], [1.0; 3]).is_ok());
    }
}
