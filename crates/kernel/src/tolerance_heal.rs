//! Healing a part's tolerance-scale defects, the ones SMLib's full check fails on an imported
//! part (edges shorter than their tolerance, the sliver faces between two edges that lie
//! within tolerance of each other, curve pieces of no length): solidcore-ops' heal, which
//! decides every edit on a certified bound, grows no tolerance, and keeps its result only
//! when the check of it fails fewer tests and no new kind ([`solidcore_ops::heal`]). Run when
//! a STEP part is read and by Repair.
//!
//! solidcore-ops decides on its check of the healed body it holds; what the part becomes is
//! that body's record brought into SMLib ([`Body::from_record`]), which is not the same body
//! bit for bit. So the body kept is checked again as it is kept, by SMLib
//! ([`crate::validation::failing_check`]), and stands only when SMLib takes it, and its check
//! fails fewer tests than the check that decided the heal and no kind of test more often, and
//! the same against SMLib's own check of the body as it was (when the healed body still fails
//! some test: one that passes is better than any).
//!
//! Whether imports and Repair heal at all is the app's preference
//! ([`set_heals_tolerance_defects`]): on by default; off, an import keeps a part its check
//! fails as read (with the check's warning) and Repair starts from the part as it is, as
//! before the heal.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::body::Body;
use crate::validation::CheckFailure;
use crate::{KernelError, Result};

/// Whether imports and Repair heal tolerance-scale defects ([`heals_tolerance_defects`]).
static HEALS: AtomicBool = AtomicBool::new(true);

/// Whether a STEP read from now on heals the parts SMLib's check fails, and Repair heals a part
/// first (as the app sets [`crate::RepairOptions::heal_first`]): the preference
/// `heal_tolerance_defects`, on unless [`set_heals_tolerance_defects`] turned it off.
pub fn heals_tolerance_defects() -> bool {
    HEALS.load(Ordering::Relaxed)
}

/// Heal (`true`, the default) or keep as read the parts of the STEP files read from now on, and
/// have Repair heal first or not ([`heals_tolerance_defects`]). A read already done keeps what it
/// was read with: the cache of recent reads keeps the two apart.
pub fn set_heals_tolerance_defects(on: bool) {
    HEALS.store(on, Ordering::Relaxed);
}

/// Holds the tolerance heal preference for a test that turns it ([`heal_preference_for_tests`]):
/// tests holding one run one at a time, and the preference is on again when it drops.
#[doc(hidden)]
pub struct HealPreferenceForTests {
    _held: std::sync::MutexGuard<'static, ()>,
}

impl Drop for HealPreferenceForTests {
    fn drop(&mut self) {
        set_heals_tolerance_defects(true);
    }
}

/// For tests that turn the process-wide preference: waits for any other such test, then sets
/// the preference to `on` (turn it again with [`set_heals_tolerance_defects`] while held). Under
/// nextest each test is its own process; under `cargo test` this keeps such tests apart.
#[doc(hidden)]
pub fn heal_preference_for_tests(on: bool) -> HealPreferenceForTests {
    static HELD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let held = HELD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    set_heals_tolerance_defects(on);
    HealPreferenceForTests { _held: held }
}

/// What [`heal_tolerance_defects`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToleranceHeal {
    /// Edges squeezed into one of their vertices.
    pub squeezed_edges: usize,
    /// Edges re-ended at a vertex a squeeze kept.
    pub re_ended_edges: usize,
    /// Sliver faces between two coincident edges removed.
    pub collapsed_faces: usize,
    /// Pieces of no length cut out of edge curves.
    pub cut_pieces: usize,
    /// The largest certified distance any of those edits moved the part's boundary, mm.
    pub largest_bound: f64,
    /// Failed tests of the check before (the one that decided the heal) and after (SMLib's
    /// check of the healed body as kept).
    pub failed: [usize; 2],
    /// For each face of the healed part, the face of the part it was.
    pub face_source: Vec<usize>,
    /// SMLib's check of the healed body as kept ([`crate::validation::failing_check`]): what
    /// it fails, `None` when it passes. A caller that checks the same body reads it here
    /// rather than checking again.
    pub check: Option<CheckFailure>,
}

impl ToleranceHeal {
    /// The heal in a line, for an import note or a repair report.
    pub fn summary(&self) -> String {
        let mut done = Vec::new();
        if self.squeezed_edges > 0 {
            done.push(format!("{} edges shorter than their tolerance collapsed into their vertices", self.squeezed_edges));
        }
        if self.collapsed_faces > 0 {
            done.push(format!("{} sliver faces between coincident edges removed", self.collapsed_faces));
        }
        if self.cut_pieces > 0 {
            done.push(format!("{} curve pieces of no length cut out", self.cut_pieces));
        }
        format!(
            "{} (nothing moved more than {:.2e} mm; no tolerance raised); SMLib's check fails {} tests, was {}",
            done.join(", "),
            self.largest_bound,
            self.failed[1],
            self.failed[0]
        )
    }
}

/// Why [`heal_tolerance_defects`] gave no answer ([`KernelError::Heal`]). Either way the body
/// stands as it was: a heal that did not finish says nothing about the body.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum HealFailure {
    /// The token over this thread's work ([`solidcraft_smlib::stop`]) stopped it.
    #[error("the heal was stopped")]
    Stopped,
    /// The heal, or the check it runs, failed, and this is why.
    #[error("the heal could not run: {0}")]
    CouldNotRun(String),
}

/// `body` healed of its tolerance-scale defects (module docs) and what was done, or `None`
/// when there is nothing the heal can prove or its result would not be better (the body as it
/// is then stands). A mesh body, or one solidcore cannot convert, is `None`. So is a healed
/// body SMLib refuses, or whose SMLib check (module docs) fails as many tests as the check
/// that decided the heal, or some kind of test more often.
///
/// The heal runs under the token over this thread's work ([`solidcraft_smlib::stop::token`]),
/// which solidcore-ops polls in its kernel's long loops, as SMLib's own calls do: work stopped
/// part way stops in the heal too.
///
/// # Errors
/// [`KernelError::Heal`]: [`HealFailure::Stopped`] when the token stops it,
/// [`HealFailure::CouldNotRun`] for the heal's other failures, and for SMLib's check of the
/// healed body that cannot run; [`KernelError::Internal`] for a panic in it; and the healed
/// record's. A failure met while the token is stopped is [`HealFailure::Stopped`], whatever
/// failed: as the import's check takes it.
pub fn heal_tolerance_defects(body: &Body) -> Result<Option<(Body, ToleranceHeal)>> {
    let Some((ops, order)) = body.ops_body() else {
        return Ok(None);
    };
    let heal = || solidcore_ops::heal::heal_tolerance_defects(ops, order);
    let healed = match solidcraft_smlib::stop::token() {
        Some(token) => solidcore_ops::cancel::within(&token, heal),
        None => heal(),
    };
    let (healed, report) = healed.map_err(|e| match e {
        solidcore_ops::Error::Cancelled => KernelError::Heal(HealFailure::Stopped),
        // a panic the session caught stays the kernel's internal error, as SMLib's do
        solidcore_ops::Error::Internal(m) => KernelError::Internal(format!("heal: {m}")),
        _ if stopped() => KernelError::Heal(HealFailure::Stopped),
        e => KernelError::Heal(HealFailure::CouldNotRun(e.to_string())),
    })?;
    let Some(healed) = healed else {
        log::debug!(target: "repair", "tolerance heal not kept: {:?}", report.why_not);
        return Ok(None);
    };
    // A healed body comes with the check that decided it (solidcore-ops checks the body only
    // when its heal proved an edit, and always then).
    let Some(before) = report.before.as_ref() else {
        log::debug!(target: "repair", "tolerance heal not kept: no check of the body decided it");
        return Ok(None);
    };
    // the healed part keeps the part's sticky veto on regularizing its faces
    let out = Body::from_record(healed.record)?.with_color(body.color()).inherit_regularization_protection(body);
    // The body kept is SMLib's of the healed record, not the body solidcore-ops checked: it
    // stands only on its own check.
    if let Some(refused) = out.smlib_refusal() {
        log::debug!(target: "repair", "tolerance heal not kept: SMLib refuses the healed body: {}", refused.why);
        return Ok(None);
    }
    let check = crate::validation::failing_check(&out).map_err(|e| match e {
        e @ KernelError::Internal(_) => e,
        _ if stopped() => KernelError::Heal(HealFailure::Stopped),
        e => KernelError::Heal(HealFailure::CouldNotRun(format!("SMLib's check of the healed body: {e}"))),
    })?;
    if let Some(why) = worse(before, check.as_ref()) {
        log::debug!(target: "repair", "tolerance heal not kept: {why}");
        return Ok(None);
    }
    // The deciding check above is solidcore-ops' of the body it converted. SMLib's own check of
    // the body as it is must be bettered as well: a heal is kept only when both say so.
    if check.is_some() {
        let own = crate::validation::failing_check(body).map_err(|e| match e {
            e @ KernelError::Internal(_) => e,
            _ if stopped() => KernelError::Heal(HealFailure::Stopped),
            e => KernelError::Heal(HealFailure::CouldNotRun(format!("SMLib's check of the body: {e}"))),
        })?;
        if let Some(why) = own.as_ref().map_or(Some("SMLib's check passes the body".to_string()), |own| worse_than(own, check.as_ref())) {
            log::debug!(target: "repair", "tolerance heal not kept: {why}");
            return Ok(None);
        }
    }
    let heal = ToleranceHeal {
        squeezed_edges: report.squeezed_edges,
        re_ended_edges: report.re_ended_edges,
        collapsed_faces: report.collapsed_faces,
        cut_pieces: report.cut_pieces,
        largest_bound: report.largest_bound,
        failed: [before.failures, check.as_ref().map_or(0, |c| c.failures)],
        face_source: healed.face_source,
        check,
    };
    Ok(Some((out, heal)))
}

/// Whether the token over this thread's work is stopped.
fn stopped() -> bool {
    solidcraft_smlib::stop::current().is_some_and(|s| s.is_stopped())
}

/// Why the check of the body kept (`after`, `None` when it passes) is not better than the
/// check that decided the heal (`before`), or `None` when it is: it fails fewer tests, and no
/// kind of test (its owner's class and number, `SmEdge::Test #7`) more often.
fn worse(before: &solidcore_ops::check::CheckReport, after: Option<&CheckFailure>) -> Option<String> {
    let mut was: BTreeMap<String, usize> = BTreeMap::new();
    for line in before.text.lines() {
        if let Some(k) = kind(line) {
            *was.entry(k).or_insert(0) += 1;
        }
    }
    worse_by_kind(&was, before.failures, after)
}

/// [`worse`], against SMLib's own check of the body (`before`).
fn worse_than(before: &CheckFailure, after: Option<&CheckFailure>) -> Option<String> {
    let mut was: BTreeMap<String, usize> = BTreeMap::new();
    for t in &before.tests {
        if let Some(k) = kind(&t.name) {
            *was.entry(k).or_insert(0) += t.times;
        }
    }
    worse_by_kind(&was, before.failures, after)
}

/// Whether `after` fails a kind of test more often than `was` counts, or as many tests as
/// `failures` or more.
fn worse_by_kind(was: &BTreeMap<String, usize>, failures: usize, after: Option<&CheckFailure>) -> Option<String> {
    let after = after?;
    let before_failures = failures;
    let mut now: BTreeMap<String, usize> = BTreeMap::new();
    for t in &after.tests {
        if let Some(k) = kind(&t.name) {
            *now.entry(k).or_insert(0) += t.times;
        }
    }
    let more: Vec<String> = now.into_iter().filter(|(k, n)| *n > was.get(k).copied().unwrap_or(0)).map(|(k, _)| k).collect();
    if !more.is_empty() {
        return Some(format!("SMLib's check of the healed body fails more tests of a kind than the body's: {}", more.join(", ")));
    }
    (after.failures >= before_failures)
        .then(|| format!("SMLib's check of the healed body fails {} tests, the body's {}", after.failures, before_failures))
}

/// A failed test's kind, `SmEdge::Test #7`, from its line or its name.
fn kind(line: &str) -> Option<String> {
    let (owner, rest) = line.trim().split_once("::Test #")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| format!("{owner}::Test #{digits}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::FailedTest;

    fn failed(tests: &[(&str, usize)]) -> CheckFailure {
        CheckFailure {
            failures: tests.iter().map(|t| t.1).sum(),
            tests: tests.iter().map(|&(name, times)| FailedTest { name: name.into(), number: None, owners: Vec::new(), times }).collect(),
        }
    }

    /// #17689: the body kept stands only when SMLib's check of it fails fewer tests than the
    /// check that decided the heal and no kind of test more often.
    #[test]
    fn a_healed_body_is_kept_only_when_its_check_is_better_at_every_kind() {
        let before = solidcore_ops::check::CheckReport {
            failures: 3,
            text: "SmEdge::Test #7 Bad Edge Length: less than edge tolerance (len=1e-4) [owner SmEdge]\n\
                   SmEdge::Test #7 Bad Edge Length: less than edge tolerance (len=2e-4) [owner SmEdge]\n\
                   SmFace::Test #34 Face has partially Coincident Edges [owner SmFace]\n"
                .into(),
        };
        assert_eq!(worse(&before, None), None, "a check that passes is better");
        assert_eq!(worse(&before, Some(&failed(&[("SmEdge::Test #7 Bad Edge Length: less than edge tolerance", 2)]))), None);
        let new_kind = worse(&before, Some(&failed(&[("SmLoop::Test #2 Bad Loop Orient", 1)])));
        assert!(new_kind.is_some_and(|w| w.contains("SmLoop::Test #2")), "a kind the body did not fail");
        let more = worse(&before, Some(&failed(&[("SmFace::Test #34 Face has partially Coincident Edges", 2)])));
        assert!(more.is_some_and(|w| w.contains("SmFace::Test #34")), "a kind failed more often");
        let as_many = worse(&before, Some(&failed(&[("SmEdge::Test #7 Bad Edge Length", 2), ("SmFace::Test #34 Face has", 1)])));
        assert!(as_many.is_some_and(|w| w.contains("fails 3 tests, the body's 3")), "as many failed tests");
    }

    /// #17708 review: the healed body is also compared with SMLib's own check of the body as
    /// it was, by the same rule.
    #[test]
    fn a_healed_body_is_kept_only_when_better_than_smlibs_own_check() {
        let own = failed(&[("SmEdge::Test #7 Bad Edge Length", 2), ("SmFace::Test #34 Face has", 1)]);
        assert_eq!(worse_than(&own, None), None);
        assert_eq!(worse_than(&own, Some(&failed(&[("SmEdge::Test #7 Bad Edge Length", 1)]))), None);
        assert!(worse_than(&own, Some(&failed(&[("SmLoop::Test #2 Bad Loop Orient", 1)]))).is_some_and(|w| w.contains("SmLoop::Test #2")));
        assert!(worse_than(&own, Some(&failed(&[("SmFace::Test #34 Face has", 2)]))).is_some_and(|w| w.contains("SmFace::Test #34")));
        assert!(worse_than(&own, Some(&failed(&[("SmEdge::Test #7 Bad Edge Length", 3)]))).is_some_and(|w| w.contains("SmEdge::Test #7")));
    }
}
