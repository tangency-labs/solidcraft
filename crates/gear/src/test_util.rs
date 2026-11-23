//! What the gear crate's tests share: number comparisons, gear references and links, a system
//! planned from items, and timing checks that hold on a busy machine.

use std::time::Instant;

use crate::items::{GearItem, GearRef, ItemLink, LinkKind};
use crate::plan::system::{SystemPlan, plan_items};
use crate::plan::{Kind, Spec};

/// `a` and `b` within `tol`.
pub(crate) fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// `a` and `b` the same but for rounding (1e-9 of the larger, at least 1e-9).
pub(crate) fn close_rel(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

pub(crate) fn g(item: usize, gear: usize) -> GearRef {
    GearRef::new(item, gear)
}
pub(crate) fn mesh(a: GearRef, b: GearRef) -> ItemLink {
    ItemLink { a, b, kind: LinkKind::Mesh }
}
pub(crate) fn shaft(a: GearRef, b: GearRef) -> ItemLink {
    ItemLink { a, b, kind: LinkKind::Shaft }
}

/// The end of a bevel item's second line from (x, 0, 0): 20° up out of the xy plane.
pub(crate) fn bend(x: f64) -> [f64; 3] {
    let a = 20f64.to_radians();
    [x + x.signum() * 24.36 * a.cos(), 0.0, 24.36 * a.sin()]
}

/// The items planned with the default spec and no drive; it must plan without an error.
pub(crate) fn planned_system(items: &[GearItem], links: &[ItemLink]) -> SystemPlan {
    let sp = plan_items(items, links, &[], None, &Spec::default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(sp.error.is_none(), "{:?}\nnotes {:?}", sp.error, sp.notes);
    sp
}

/// The least of `n` runs of `f` (ms): the run other work disturbed least.
pub(crate) fn best_ms<T>(n: usize, mut f: impl FnMut() -> T) -> f64 {
    (0..n.max(1))
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(f());
            t.elapsed().as_secs_f64() * 1e3
        })
        .fold(f64::INFINITY, f64::min)
}

/// The reference job's time on an idle machine (ms, the `ci` profile, this crate's own laptop:
/// 0.64–0.71 ms a plan measured with other builds running, so a little over idle, which keeps
/// the limits strict): planning a 60/97 helical pair 20 times, the same kind of work (outlines,
/// splines, allocation) as the plans the timing tests time.
const REFERENCE_IDLE_MS: f64 = 13.0;

/// The reference job's best time now (ms).
pub(crate) fn reference_ms() -> f64 {
    let s = Spec { teeth: 60, teeth2: 97, ..Spec::defaults_for(Kind::Helical) };
    best_ms(5, || (0..20).filter(|_| crate::plan::plan(&s).is_ok()).count())
}

/// Asserts `ms` (a time measured in this test) is under `limit_ms`, the limit on an idle machine,
/// scaled by how much slower than idle the machine runs now: the reference job is timed in the
/// same test, so a machine loaded by other work (a parallel test run, a build) slows both alike.
pub(crate) fn assert_quick(label: &str, ms: f64, limit_ms: f64) {
    if ms < limit_ms {
        return;
    }
    let slowdown = (reference_ms() / REFERENCE_IDLE_MS).max(1.0);
    assert!(ms < limit_ms * slowdown, "{label}: {ms:.1} ms, over {limit_ms} ms × {slowdown:.2} (how much slower than idle this machine runs now)");
}

/// The reference job's time here (`cargo test --profile ci -p solidcraft-gear reference_job_time
/// -- --ignored --nocapture` on an idle machine sets [`REFERENCE_IDLE_MS`]).
#[test]
#[ignore]
fn reference_job_time() {
    let ms: Vec<f64> = (0..5).map(|_| reference_ms()).collect();
    eprintln!("reference job: {ms:.3?} ms (best of 5 runs each)");
}
