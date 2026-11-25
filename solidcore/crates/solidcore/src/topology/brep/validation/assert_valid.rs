//! `SmBrep::AssertValid`: the whole-body topology check, its walk over every region,
//! shell, face, loop, edge and vertex, and the `check_brep` report SolidCraft reads.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `SmSAGObject.cpp`, `SmOwningTopology.cpp`,
//! `SmTopology.cpp`, `SmRegion.cpp`, `SmShell.cpp`, `SmFace.cpp`, `SmFaceuse.cpp`,
//! `SmLoop.cpp`, `SmLoopuse.cpp`, `SmEdge.cpp`, `SmEdgeuse.cpp`, `SmVertex.cpp`,
//! `SmVertexuse.cpp`, `SmTopologyTraverser.cpp`, `SmCurve.cpp` (the virtual
//! `AssertValid`), `source/SMLib/inc/SmAssertArray.h` (the report macros).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`assert_valid`] | `SmBrep::AssertValid` |
//! | [`check_brep`], [`check_brep_with_keys`], [`BrepCheck`] | the capi `smlib_brep_check` sequence (`ValidatePointers`, then `AssertValid`) |
//! | [`CheckLevel`] | `SmAssertTestLevel` (`SM_LEVEL_0`..`SM_LEVEL_2`) |
//! | `brep::sub_topology` (private) | `SmBrep::AssertSubTopology`, the `SmBrep` branch of `SmTopologyTraverser::GetSubTopology` |
//! | `brep::tolerances` (private) | `SmBrep::ValidateAndUpdateTolerances(TRUE, ...)` |
//! | [`validate_and_update_tolerances`], [`ToleranceGaps`] | `SmBrep::ValidateAndUpdateTolerances` |
//! | `brep::collect_regions`, `brep::adjacent_regions` (private) | `SmTopologyTraverser::CollectRegions`, `SmRegion::GetAdjacentRegions` |
//! | `base::topology`, `base::owning` (private) | `SmTopology::AssertValid`, `SmOwningTopology::AssertValid`, `SmSAGObject::AssertValid` |
//! | `shell::region`, `shell::shell` (private) | `SmRegion::AssertValid`, `SmShell::AssertValid`, `sm_GetShellVertices`, `sm_GetShellFaceuses`, `SmEdgeuse::GetConnectedEdgeuses` |
//! | `face::face`, `face::faceuse` (private) | `SmFace::AssertValid` (level 0 and the level-2 tests whose callees exist), `SmFaceuse::AssertValid`, `sm_AssertTestFace39` |
//! | `loops::loop_`, `loops::loopuse` (private) | `SmLoop::AssertValid`, `SmLoopuse::AssertValid` |
//! | `edge::edge`, `edge::edgeuse`, `edge::curve` (private) | `SmEdge::AssertValid`, `sm_AssertTestEdge14`, `SmEdgeuse::AssertValid`, the virtual `SmCurve::AssertValid` |
//! | `vertex::vertex`, `vertex::vertexuse` (private) | `SmVertex::AssertValid`, `SmVertexuse::AssertValid` |
//! | [`curve_assert_valid`] | the virtual `SmCurve::AssertValid` of a free-standing curve |
//! | `curves` (private) | `SmOffsetCurve`, `SmProjectedCurve`, `SmHermiteCurve`, `SmCrvOnSurf`, `SmCrvInVolume::AssertValid` |
//! | `seams` (private) | the callees other packages port (see that module) |
//!
//! Deviations: object pointers are arena identities; the report helpers implement the
//! release `SM_ASSERT_*_REPORT` macros (a test runs when the level is at least its
//! own, its predicate and value are evaluated lazily, and a passed test clears
//! `GetLastAdd`). `SM_LEVEL_GIVEN` (4) runs every test and skips the base classes, as
//! upstream; release builds ignore `pTestRequests`. Context identity, mark bounds and
//! the doubly linked list rings are intrinsic to the arenas, so those tests always pass
//! (they still run, in native order); list membership and owner back links are checked
//! against indices built once per call. The walks are bounded loops (no recursion);
//! the radial edge-use walks stop with [`SolidError::InvalidInput`] after visiting more
//! uses than the edge has (upstream loops forever). Calls into checks not yet ported go
//! through `seams` and return [`SolidError::Unported`]. Warning text (`WARN`,
//! `SE_MSG`, `smos_WriteBuffer`) and debug drawing are omitted. `CalculateBoundingBox`
//! in the validate-only tolerance pass is not called (its size is used only when
//! fixing). Statuses upstream ignores are ignored here too. [`BrepCheck`]'s summary
//! (failures, lines, count, validity) leaves out the warnings `AssertValid` reports
//! without failing ([`AssertReport::is_warning`], `SmBrep` test 21), as the capi does,
//! and omits the owner tag of a report without owner type; the report lists keep
//! them. [`check_brep`] refuses a B-rep of [`NamingScheme::Unknown`] (#2013).

mod base;
mod brep;
mod curves;
mod edge;
mod face;
mod loops;
mod seams;
mod shell;
mod tangent_sections;
mod vertex;

use std::borrow::Cow;
use std::collections::BTreeMap;

use super::{
    object_info, validate_pointers_excluding, validate_pointers_traced, Object, PointerCheck,
};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::assert_array::{
    obj_type_string, AssertArray, AssertReport, ObjectKey, ReportSpec,
};
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::UNDEF_DOUBLE;
use crate::topology::{Brep, BrepId, NamingScheme};

/// The existing virtual curve check for owned geometry outside an edge or UV trim.
/// A curve-bounded surface uses it for its contained 3D and UV boundary curves.
pub(crate) fn standalone_curve_assert_valid(
    k: &Kernel,
    curve: &mut crate::curve::curve::Curve,
    list: Option<&mut AssertArray>,
    level: u8,
    key: ObjectKey,
    polar_key: ObjectKey,
) -> Result<bool> {
    edge::curve(k, curve, list, level, key, polar_key, None, None)
}

/// `SM_LEVEL_GIVEN`: every test, no base classes.
const LEVEL_GIVEN: u8 = 4;

/// The test level of a check (`SmAssertTestLevel`): `Level0` runs the fewest tests,
/// `Level2` all of them. SolidCraft's "Basic" check is `Level0`, "Full" is `Level2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CheckLevel {
    /// `SM_LEVEL_0`.
    Level0 = 0,
    /// `SM_LEVEL_1`: adds the geometry checks.
    Level1 = 1,
    /// `SM_LEVEL_2`: every test.
    Level2 = 2,
}

impl CheckLevel {
    /// The upstream enumerator value (0, 1 or 2).
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// What a check holds the tolerance pass to ([`check_brep_scoped`], [`assert_valid_scoped`]).
///
/// [`ValidityScope::Body`] is the rule for every body: a vertex's gaps to its edges and faces
/// within half its tolerance, an edge's gap to its faces within its tolerance. A body a stitch
/// produced ([`ValidityScope::Stitched`]) may also carry a gap past those, up to the sum of the
/// two zones it spans (vertex and edge, vertex and face, edge and face), where the tolerance ball
/// that covers it is proven to reach no topology it does not bound
/// ([`pointers::vertex_ball_reaches`], [`pointers::edge_tube_reaches`]: boxes, never samples; a
/// proof that cannot be made leaves the gap reported).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidityScope {
    /// The rule for every body.
    #[default]
    Body,
    /// A body a stitch produced.
    Stitched,
}

/// The result of [`check_brep`]: the pointer check, then (when it found nothing) the
/// whole-body `AssertValid`, as the capi `smlib_brep_check` runs them.
///
/// The report lists are upstream's, warnings included; the summary
/// ([`BrepCheck::failures`], [`BrepCheck::lines`], [`BrepCheck::failure_count`],
/// [`BrepCheck::is_valid`]) leaves the warnings out ([`AssertReport::is_warning`]:
/// `SmBrep` test 21, which `AssertValid` reports without failing), as the capi does;
/// [`BrepCheck::warnings`] lists them.
#[derive(Debug, Clone)]
pub struct BrepCheck {
    /// What `ValidatePointers` concluded.
    pub pointers: PointerCheck,
    /// The pointer check's reports, in native order.
    pub pointer_reports: AssertArray,
    /// `AssertValid`'s return; `None` when it was skipped because the pointer check
    /// reported failures.
    pub assert_valid: Option<bool>,
    /// `AssertValid`'s reports, in native order.
    pub reports: AssertArray,
}

impl BrepCheck {
    /// The failed reports (`!m_bOK`) that are not warnings
    /// ([`AssertReport::is_warning`]): pointer reports first, then `AssertValid`'s.
    pub fn failures(&self) -> impl Iterator<Item = &AssertReport> {
        self.pointer_reports
            .reports()
            .iter()
            .chain(self.reports.reports())
            .filter(|r| is_failure(r))
    }

    /// The warnings ([`AssertReport::is_warning`]: reported, `!m_bOK`, without failing
    /// the check), in the order of [`BrepCheck::failures`].
    pub fn warnings(&self) -> impl Iterator<Item = &AssertReport> {
        self.pointer_reports
            .reports()
            .iter()
            .chain(self.reports.reports())
            .filter(|r| !r.ok && r.is_warning())
    }

    /// The number of report lines ([`BrepCheck::lines`]), including the synthetic
    /// lines for a failed status without a report; warnings do not count.
    #[must_use]
    pub fn failure_count(&self) -> usize {
        self.lines().len()
    }

    /// `true` when the check produced no failure line: a body whose only reports are
    /// warnings ([`BrepCheck::warnings`]) is valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.failure_count() == 0
    }

    /// One line per failure (warnings left out), as the capi formats them:
    /// `<reporting type>::Test #<n> <name>: <message> [owner <owner type>]` (without the
    /// owner tag when the report names no owner type), plus
    /// `SmBrep::ValidatePointers failed without a report` when the pointer check did not
    /// pass and reported no failure, `AssertValid skipped: ValidatePointers failed` when it was
    /// skipped, and `SmBrep::AssertValid failed without a report` when `AssertValid`
    /// returned `false` without reporting a failure.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.classified()
            .into_iter()
            .map(|(class, report)| match (class, report) {
                (_, Some(r)) => report_line(r),
                (FailureClass::Pointers(_), None) => {
                    "SmBrep::ValidatePointers failed without a report".to_string()
                }
                (FailureClass::AssertValidSkipped, None) => {
                    "AssertValid skipped: ValidatePointers failed".to_string()
                }
                (FailureClass::AssertValidWithoutReport | FailureClass::Report { .. }, None) => {
                    "SmBrep::AssertValid failed without a report".to_string()
                }
            })
            .collect()
    }

    /// How many failure lines ([`BrepCheck::lines`]) of each [`FailureClass`] the check
    /// has.
    #[must_use]
    pub fn failure_classes(&self) -> BTreeMap<FailureClass, usize> {
        let mut out = BTreeMap::new();
        for (class, _) in self.classified() {
            let n: &mut usize = out.entry(class).or_default();
            *n = n.saturating_add(1);
        }
        out
    }

    /// The failure classes `self` has more lines of than `before` has, each with how many
    /// more, in [`FailureClass`] order: what a body has failing that `before` (the body it
    /// was made from) did not. A class `before` has as many lines of, or more, is not
    /// listed.
    #[must_use]
    pub fn failures_beyond(&self, before: &BrepCheck) -> Vec<(FailureClass, usize)> {
        let had = before.failure_classes();
        self.failure_classes()
            .into_iter()
            .filter_map(|(class, n)| {
                let more = n.saturating_sub(had.get(&class).copied().unwrap_or(0));
                (more > 0).then_some((class, more))
            })
            .collect()
    }

    /// Name the first check that gained a failure, including a failed check without a report.
    pub(crate) fn first_new_failure_name(&self, before: &Self) -> Option<&'static str> {
        let (class, _) = self.failures_beyond(before).into_iter().next()?;
        Some(match class {
            FailureClass::Report { .. } => self
                .failures()
                .find(|r| FailureClass::of(r) == class)
                .map_or("the edited B-rep failed validation", |r| r.name),
            FailureClass::Pointers(_) => "ValidatePointers failed",
            FailureClass::AssertValidSkipped => {
                "AssertValid was skipped after ValidatePointers failed"
            }
            FailureClass::AssertValidWithoutReport => "AssertValid failed without a report",
        })
    }

    /// The failure lines in [`BrepCheck::lines`] order, each with its class and its report
    /// (`None` for a synthetic line).
    fn classified(&self) -> Vec<(FailureClass, Option<&AssertReport>)> {
        let mut out = Vec::new();
        let pointer_failures = self.pointer_reports.reports().iter().any(is_failure);
        if !self.pointers.is_passed() && !pointer_failures {
            out.push((FailureClass::Pointers(self.pointers), None));
        }
        out.extend(
            self.pointer_reports
                .reports()
                .iter()
                .filter(|r| is_failure(r))
                .map(|r| (FailureClass::of(r), Some(r))),
        );
        if pointer_failures {
            out.push((FailureClass::AssertValidSkipped, None));
            return out;
        }
        if self.assert_valid == Some(false) && !self.reports.reports().iter().any(is_failure) {
            out.push((FailureClass::AssertValidWithoutReport, None));
        }
        out.extend(
            self.reports
                .reports()
                .iter()
                .filter(|r| is_failure(r))
                .map(|r| (FailureClass::of(r), Some(r))),
        );
        out
    }
}

/// What one failure line of a [`BrepCheck`] is, leaving out where: the test that failed
/// and the type of the object it is about, not which object (an operation that rebuilds a
/// body's entities gives them new identities, so a failure is compared with its input's
/// by class: [`BrepCheck::failures_beyond`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FailureClass {
    /// A failed report: its reporting type, label table, test and owner type.
    Report {
        /// [`AssertReport::reporting_type`].
        reporting_type: i64,
        /// [`AssertReport::list_index`].
        list_index: u64,
        /// [`AssertReport::test_index`].
        test_index: u64,
        /// [`AssertReport::owner_type`].
        owner_type: i64,
    },
    /// `ValidatePointers` did not pass ([`PointerCheck`]) and reported no failure.
    Pointers(PointerCheck),
    /// `AssertValid` was skipped: `ValidatePointers` reported a failure.
    AssertValidSkipped,
    /// `AssertValid` failed without a report.
    AssertValidWithoutReport,
}

impl FailureClass {
    fn of(r: &AssertReport) -> Self {
        Self::Report {
            reporting_type: r.reporting_type,
            list_index: r.list_index,
            test_index: r.test_index,
            owner_type: r.owner_type,
        }
    }
}

/// A report the summary counts: failed (`!m_bOK`) and not a warning.
fn is_failure(r: &AssertReport) -> bool {
    !r.ok && !r.is_warning()
}

/// The capi line of one report; the owner tag only when the report names an owner type.
fn report_line(r: &AssertReport) -> String {
    let head = format!(
        "{}::Test #{} {}: {}",
        obj_type_string(r.reporting_type),
        r.test_index,
        r.name,
        r.format_log_message(),
    );
    match r.owner_type_string {
        Some(owner) => format!("{head} [owner {owner}]"),
        None => head,
    }
}

/// [`check_brep_with_keys`] with the geometry-owner identities of `b`'s naming scheme
/// ([`Brep::owner_identity`]): the Euler operators' notification identities for a
/// [`NamingScheme::Notify`](crate::topology::NamingScheme::Notify) B-rep; for a
/// [`NamingScheme::Caller`](crate::topology::NamingScheme::Caller) B-rep the caller
/// key recorded where the entity was built or its geometry bound (an imported body,
/// [`crate::topology::brep::p3::api_brep_copy`], an operation on either), else the
/// arena identities ([`crate::topology::brep_cache::arena_identity`]), which the
/// binders name an entity with no recorded key by too.
///
/// A B-rep of [`NamingScheme::Unknown`] (a bare [`Brep::new`] or
/// [`Brep::new_with_infinite_region`] built on without recording a scheme) is refused:
/// its builders left geometry unowned or named it by the caller's own identities, not
/// by the arena identities a check would fall back to, so the check reported false
/// failures with no sign of the cause (every edge's curve owner,
/// `SmBrep::ValidatePointers` test 61).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a [`NamingScheme::Unknown`] B-rep (record its
/// scheme with [`Brep::set_naming`] or [`Brep::use_arena_naming`], or pass the
/// builder's identities to [`check_brep_with_keys`]); those of
/// [`check_brep_with_keys`].
pub fn check_brep(k: &mut Kernel, b: &mut Brep, level: CheckLevel) -> Result<BrepCheck> {
    check_brep_scoped(k, b, level, ValidityScope::Body)
}

/// [`check_brep`] holding the tolerance pass to `scope`.
///
/// # Errors
///
/// As [`check_brep`].
pub fn check_brep_scoped(
    k: &mut Kernel,
    b: &mut Brep,
    level: CheckLevel,
    scope: ValidityScope,
) -> Result<BrepCheck> {
    Ok(check_brep_counted(k, b, level, scope)?.0)
}

/// (not upstream) [`check_brep_scoped`] in full, the verify mode: with every memo and
/// result-invariant shortcut off ([`Kernel::memoize_curve_caches`] off for the call), so
/// nothing an earlier check or operation worked out about `b`'s geometry
/// ([`crate::geom::check_memo`]) is read back. The memos never change a result, so this
/// reports what [`check_brep_scoped`] does, only slower; it is there to show that.
///
/// # Errors
///
/// As [`check_brep`].
pub fn check_brep_full(
    k: &mut Kernel,
    b: &mut Brep,
    level: CheckLevel,
    scope: ValidityScope,
) -> Result<BrepCheck> {
    let memoize = std::mem::replace(&mut k.memoize_curve_caches, false);
    let out = check_brep_scoped(k, b, level, scope);
    k.memoize_curve_caches = memoize;
    out
}

/// [`check_brep_scoped`], also saying how many `ValidatePointers` passes its `AssertValid` ran
/// (0 when [`check_brep_with_keys`] skipped it, or when `AssertValid` did not run).
fn check_brep_counted(
    k: &mut Kernel,
    b: &mut Brep,
    level: CheckLevel,
    scope: ValidityScope,
) -> Result<(BrepCheck, usize)> {
    let naming = b.naming();
    if naming == NamingScheme::Unknown {
        return Err(SolidError::invalid_input(
            "topology::brep::validation::check_brep",
            "the B-rep records no naming scheme, so its geometry owners cannot be checked; \
             set one (Brep::set_naming, Brep::use_arena_naming) or use check_brep_with_keys",
        ));
    }
    let recorded = b.geometry_owner_keys().clone();
    let keys =
        |e: EntityRef| crate::topology::owner_identity_of(naming, recorded.get(&e).copied(), e);
    check_with_keys_counted(k, b, level, &keys, scope)
}

/// The capi `smlib_brep_check(brep, level)`: [`validate_pointers`](super::validate_pointers) at `level` (its
/// default `SM_NO_WALK`) into its own report list; when it reports no failure, [`assert_valid`] at `level` with the
/// walk. `keys` gives the identities the geometry owners use (as for
/// [`validate_pointers`](super::validate_pointers)).
///
/// (not upstream) `AssertValid` starts with `ValidatePointers` again, at level 0 with the
/// walk. With [`Kernel::memoize_curve_caches`] that second pass is skipped when the first
/// reported nothing, checked every face whole and left the body unwritten
/// ([`Brep::write_stamps`]): the level changes no pointer test, and the walk only drops
/// `CheckFace`'s loop checks, so on the same body the second pass would report nothing,
/// succeed and write nothing.
///
/// (not upstream, #21105) Where a face of `b` lacks its UV trim curves, the whole check runs
/// on a copy standing for `b` (`standing_where_trims_lack`), so the trims it builds and the
/// tolerances their attaching raises stay off `b`.
///
/// # Errors
///
/// Those of [`validate_pointers`](super::validate_pointers) and [`assert_valid`], including
/// [`crate::SolidError::Unported`] from a check not yet ported.
pub fn check_brep_with_keys(
    k: &mut Kernel,
    b: &mut Brep,
    level: CheckLevel,
    keys: &dyn Fn(EntityRef) -> ObjectRef,
) -> Result<BrepCheck> {
    check_brep_with_keys_scoped(k, b, level, keys, ValidityScope::Body)
}

/// [`check_brep_with_keys`] holding the tolerance pass to `scope`.
///
/// # Errors
///
/// As [`check_brep_with_keys`].
pub fn check_brep_with_keys_scoped(
    k: &mut Kernel,
    b: &mut Brep,
    level: CheckLevel,
    keys: &dyn Fn(EntityRef) -> ObjectRef,
    scope: ValidityScope,
) -> Result<BrepCheck> {
    Ok(check_with_keys_counted(k, b, level, keys, scope)?.0)
}

/// [`check_brep_with_keys_scoped`], also saying how many `ValidatePointers` passes its
/// `AssertValid` ran (0 or 1).
fn check_with_keys_counted(
    k: &mut Kernel,
    b: &mut Brep,
    level: CheckLevel,
    keys: &dyn Fn(EntityRef) -> ObjectRef,
    scope: ValidityScope,
) -> Result<(BrepCheck, usize)> {
    use crate::geom::check_memo;
    let mut standing = standing_where_trims_lack(b)?;
    let b = standing.as_mut().unwrap_or(b);
    // not upstream: the incremental check ([`crate::geom::check_memo`]): the geometric
    // tests of entities whose neighbourhoods passed them before are skipped, and a body
    // found valid with no report at all leaves its neighbourhoods clean
    let salt = check_memo::salt(k, level.code(), scope == ValidityScope::Stitched);
    if let Some(salt) = salt {
        check_memo::begin(k, check_memo::Stars::of(b, salt));
    }
    let ran = check_once(k, b, level, keys, scope);
    let after = match (&ran, salt) {
        (Ok((c, _, true)), Some(salt))
            if c.pointers.is_passed()
                && c.assert_valid == Some(true)
                && c.pointer_reports.is_empty()
                && c.reports.is_empty() =>
        {
            Some(check_memo::Stars::of(b, salt))
        }
        _ => None,
    };
    check_memo::end(k, after);
    ran.map(|(c, passes, _)| (c, passes))
}

/// One run of [`check_with_keys_counted`] on the body it checks, also saying whether the
/// pointer check found every face whole.
fn check_once(
    k: &mut Kernel,
    b: &mut Brep,
    level: CheckLevel,
    keys: &dyn Fn(EntityRef) -> ObjectRef,
    scope: ValidityScope,
) -> Result<(BrepCheck, usize, bool)> {
    let mut pointer_reports = AssertArray::new();
    let before = b.write_stamps();
    let mut faces_whole = true;
    let (pointers, exclusion_failed) = validate_pointers_traced(
        k,
        b,
        Some(&mut pointer_reports),
        level.code(),
        false,
        keys,
        &mut faces_whole,
        false,
    )?;
    let pointers_repeat = !(k.memoize_curve_caches
        && pointers.is_passed()
        && !exclusion_failed
        && faces_whole
        && !pointer_reports.reports().iter().any(is_failure)
        && b.write_stamps() == before);
    let mut reports = AssertArray::new();
    let assert_valid = if pointer_reports.reports().iter().any(is_failure) {
        None
    } else {
        Some(assert_valid_with(
            k,
            b,
            Some(&mut reports),
            level.code(),
            true,
            keys,
            scope,
            pointers_repeat,
        )?)
    };
    // Not upstream (#21105): a warning the first pointer pass reported (a turned-back
    // curve end, test 147) is said once. The second pass (`AssertValid`'s level-0
    // `ValidatePointers`, run when the first could not be skipped) reports it again; it is
    // dropped there, so the check says the same whether that pass ran or was skipped.
    let repeated: Vec<usize> = (0..reports.len())
        .rev()
        .filter(|&i| {
            reports
                .get(i)
                .is_some_and(|r| r.is_warning() && pointer_reports.contains(r))
        })
        .collect();
    for i in repeated {
        reports.remove_at(i);
    }
    let passes = usize::from(assert_valid.is_some() && pointers_repeat);
    Ok((
        BrepCheck {
            pointers,
            pointer_reports,
            assert_valid,
            reports,
        },
        passes,
        faces_whole,
    ))
}

/// `SmBrep::AssertValid(pAList, eTestLevel, eWalkTree)`: the B-rep checks (base
/// classes, `ValidatePointers` at level 0, the validate-only tolerance pass, Brep
/// tests 0-21; not upstream's `ContainsCoincidentTopology`, see `brep`) and, with `walk`,
/// `AssertValid` of every region, shell, face, loop, edge and vertex. Failed checks are appended to `reports`
/// in native order. (not upstream, #21105) Where a face of `b` lacks its UV trim curves, it
/// runs on a copy standing for `b`, as [`check_brep_with_keys`] does. `level` is `SM_LEVEL_0`..`SM_LEVEL_GIVEN` (0..4). `keys` gives the
/// geometry owners' identities (as for [`validate_pointers`](super::validate_pointers)). Returns `false` when a
/// check failed (at once when `ValidatePointers` does not succeed).
///
/// # Errors
///
/// [`crate::SolidError::InvalidInput`] for a level above 4 or broken topology links
/// upstream would dereference; [`crate::SolidError::Unported`] from a check not yet
/// ported; those of the geometric callees.
pub fn assert_valid(
    k: &mut Kernel,
    b: &mut Brep,
    reports: Option<&mut AssertArray>,
    level: u8,
    walk: bool,
    keys: &dyn Fn(EntityRef) -> ObjectRef,
) -> Result<bool> {
    assert_valid_scoped(k, b, reports, level, walk, keys, ValidityScope::Body)
}

/// [`assert_valid`] holding the tolerance pass to `scope`.
///
/// # Errors
///
/// As [`assert_valid`].
pub fn assert_valid_scoped(
    k: &mut Kernel,
    b: &mut Brep,
    reports: Option<&mut AssertArray>,
    level: u8,
    walk: bool,
    keys: &dyn Fn(EntityRef) -> ObjectRef,
    scope: ValidityScope,
) -> Result<bool> {
    if level > LEVEL_GIVEN {
        return Err(super::invalid("invalid assert test level"));
    }
    let mut standing = standing_where_trims_lack(b)?;
    let b = standing.as_mut().unwrap_or(b);
    assert_valid_with(k, b, reports, level, walk, keys, scope, true)
}

/// (not upstream, #21105) The B-rep a check runs on in place of `b`: a copy standing for it,
/// its faces' definitions kept ([`Brep::clone_standing_for_self_keeping_definitions`]), where
/// one of its faces lacks UV trim curves
/// ([`face_uv_curves_present`](crate::topology::trimming_tools::face_uv_curves_present)),
/// else `None`, and the check runs on `b`. The walk builds a face's missing trims (`CheckFace`,
/// a vertex use's UV point), and attaching a trim raises the tolerances of its edge and
/// vertices by an amount that depends on the trims the edge already has, so a check run on
/// `b` would leave those trims and raises, in the walk's order, on the body it was asked
/// about. On the copy the check builds and reads the same trims and reports the same, and `b`
/// keeps none of it: no trims, no raised tolerances, and none of the gaps or repairs the check
/// stores. A body that has its trims, the one a caller makes once
/// ([`create_missing_uv_trim_curves`](crate::topology::brep::p2::create_missing_uv_trim_curves)),
/// is checked on itself and builds none. A face whose trims cannot be looked up (a broken
/// loop link) counts as lacking them: the check, on the copy, is what reports the link.
///
/// # Errors
///
/// Those of the copy's geometry copies.
fn standing_where_trims_lack(b: &Brep) -> Result<Option<Brep>> {
    for &f in b.face_list() {
        if !matches!(
            crate::topology::trimming_tools::face_uv_curves_present(b, f),
            Ok(true)
        ) {
            return Ok(Some(b.clone_standing_for_self_keeping_definitions()?));
        }
    }
    Ok(None)
}

/// [`assert_valid_scoped`]; without `pointers`, its `ValidatePointers` pass is taken as
/// succeeding with no report (for [`check_brep_with_keys`], which has just run one that proves
/// it).
#[allow(clippy::too_many_arguments)]
fn assert_valid_with(
    k: &mut Kernel,
    b: &mut Brep,
    reports: Option<&mut AssertArray>,
    level: u8,
    walk: bool,
    keys: &dyn Fn(EntityRef) -> ObjectRef,
    scope: ValidityScope,
    pointers: bool,
) -> Result<bool> {
    if level > LEVEL_GIVEN {
        return Err(super::invalid("invalid assert test level"));
    }
    let index = base::Index::new(b);
    let mut cx = Cx {
        rep: Rep {
            array: reports,
            level,
            stitched: scope == ValidityScope::Stitched,
        },
        keys,
        index,
        edge_sets: edge::EdgeSets::default(),
    };
    brep::brep(k, b, &mut cx, walk, pointers)
}

pub use brep::ToleranceGaps;

/// `SmBrep::ValidateAndUpdateTolerances(bValidateOnly, rbTolerancesViolatedOrUpdated,
/// .., pAList, eTestLevel)`: for every face's upward-faceuse edge use (an edge use and
/// its mate once) the vertex/edge and vertex/face gaps at both ends and the edge/face
/// gap. Unless `validate_only`, a vertex with a gap over half its tolerance (not upstream,
/// WR-679: upstream takes the whole tolerance) is grown to twice its larger gap, and an
/// edge with an edge/face gap over its tolerance is grown (with its vertices) to twice the
/// gap when the gap is under the B-rep's box diagonal / 800; tolerances only grow. The
/// gaps still over half the vertex tolerance (tests 0-3) or over the edge tolerance (test
/// 4) are then reported into `reports` (list 2 of `SmBrep`, about the edge use) at
/// `level` (0..4), in native order. Returns whether a gap was over its tolerance and the largest vertex gaps.
///
/// # Errors
///
/// [`crate::error::topology::TopologyFailure::Failed`] (`SM_ERR`) for an edge use end
/// without vertex (upstream's `NER`, after the reports and updates of the earlier edge
/// uses);
/// [`SolidError::InvalidInput`] for a level above 4 or broken topology links upstream
/// would dereference; those of the gap and bounding box computations.
pub fn validate_and_update_tolerances(
    k: &mut Kernel,
    b: &mut Brep,
    validate_only: bool,
    reports: Option<&mut AssertArray>,
    level: u8,
) -> Result<ToleranceGaps> {
    if level > LEVEL_GIVEN {
        return Err(super::invalid("invalid assert test level"));
    }
    let mut rep = Rep {
        array: reports,
        level,
        stitched: false,
    };
    let (ok, gaps) = brep::validate_and_update(k, b, &mut rep, validate_only)?;
    if !ok {
        return Err(SolidError::topology(
            "topology::brep::validation::validate_and_update_tolerances",
            crate::error::topology::TopologyFailure::VertexuseWithoutVertex,
        ));
    }
    Ok(gaps)
}

/// The virtual `AssertValid(pAList, eTestLevel, SM_NO_WALK)` of a curve outside any
/// B-rep (as the edge and edge-use checks run it on their curves): the checks of
/// `curve`'s class and, unless `level` is `SM_LEVEL_GIVEN` (4), of its base classes,
/// with the contained objects of a compound curve checked as upstream. The curve
/// reports as `key`; an ellipse's polar converter and contained objects report under
/// `key ^ (n << 48)` (`n = 1, 2, ..` in walk order), so keep bits 48.. of `key`
/// clear. Failed checks are appended to `reports` in native order. Returns `true`
/// when every check passed.
///
/// # Errors
///
/// [`crate::SolidError::InvalidInput`] for a level above 4 or where upstream's
/// behaviour is undefined (an offset curve without a base curve; a report past the
/// end of its label table); [`crate::SolidError::Unported`] from a check not yet
/// ported; those of the geometric callees.
pub fn curve_assert_valid(
    k: &Kernel,
    curve: &mut crate::curve::curve::Curve,
    reports: Option<&mut AssertArray>,
    level: u8,
    key: ObjectKey,
) -> Result<bool> {
    if level > LEVEL_GIVEN {
        return Err(super::invalid("invalid assert test level"));
    }
    let mut walk = curves::Walk::new(key);
    let polar_key = walk.next_key()?;
    edge::curve_in_walk(
        k, curve, reports, level, key, polar_key, None, None, &mut walk,
    )
}

/// The state of one `AssertValid` run: the reports, the requested level, the geometry
/// owner identities and the list membership index.
struct Cx<'a> {
    rep: Rep<'a>,
    keys: &'a dyn Fn(EntityRef) -> ObjectRef,
    index: base::Index,
    edge_sets: edge::EdgeSets,
}

impl Cx<'_> {
    /// Whether the base-class checks run (`eTestLevel != SM_LEVEL_GIVEN`).
    const fn bases(&self) -> bool {
        self.rep.level != LEVEL_GIVEN
    }
}

/// The reports of a run and its level (`pAList`, `eTestLevel`).
struct Rep<'a> {
    array: Option<&'a mut AssertArray>,
    level: u8,
    /// The tolerance pass runs under [`ValidityScope::Stitched`].
    stitched: bool,
}

/// The key of a curve or surface object (see the `seams` key note): the shim's
/// identity space and index.
fn geometry_key(space: u64, index: u32) -> ObjectKey {
    ObjectKey((space << 32) | u64::from(index))
}

/// One report of the release macros: who runs it, about which object, and its
/// tolerance, value and optional message.
struct Test<'m> {
    class: i64,
    list: u64,
    index: u64,
    reporter: Object,
    owner: Object,
    owner_type: i64,
    tol: f64,
    message: Option<Cow<'m, str>>,
}

impl Rep<'_> {
    /// `SM_ASSERT_TEST_NEED`: the test of level `test_level` runs
    /// (`!(eTestLevel < TestLevel)`).
    const fn need(&self, test_level: u8) -> bool {
        self.level >= test_level
    }

    /// `SM_ASSERT_TEST_NEED(.., test_level)` (no test requests): the level is
    /// `SM_LEVEL_0`..`SM_LEVEL_2` and at least `test_level`.
    const fn test_need(&self, test_level: u8) -> bool {
        self.level <= 2 && self.level >= test_level
    }

    /// `SM_ASSERT_BOOLEAN_REPORT(index, ..)` in a method of class `class` run by
    /// `this`: `ok` is the already level-gated predicate.
    fn this_bool(
        &mut self,
        b: &Brep,
        class: i64,
        this: impl Into<Object>,
        index: u64,
        ok: bool,
    ) -> Result<bool> {
        let this = this.into();
        self.add(
            b,
            ok,
            Test {
                class,
                list: 0,
                index,
                reporter: this,
                owner: this,
                owner_type: class,
                tol: UNDEF_DOUBLE,
                message: None,
            },
            || UNDEF_DOUBLE,
        )
    }

    /// `SM_ASSERT_VALUE_REPORT(index, .., tol, value, ..)` in a method of class
    /// `class` run by `this`; `value` is evaluated only when the test fails.
    fn this_value(
        &mut self,
        b: &Brep,
        class: i64,
        this: impl Into<Object>,
        index: u64,
        ok: bool,
        tol: f64,
        value: impl FnOnce() -> f64,
    ) -> Result<bool> {
        let this = this.into();
        self.add(
            b,
            ok,
            Test {
                class,
                list: 0,
                index,
                reporter: this,
                owner: this,
                owner_type: class,
                tol,
                message: None,
            },
            value,
        )
    }

    /// The release `SM_ASSERT_OBJ_*_REPORT` body: on a pass clears `GetLastAdd`, on a
    /// failure adds the report (when there is a list) and returns `false`.
    fn add(
        &mut self,
        b: &Brep,
        ok: bool,
        test: Test<'_>,
        value: impl FnOnce() -> f64,
    ) -> Result<bool> {
        let Some(array) = self.array.as_deref_mut() else {
            return Ok(ok);
        };
        if ok {
            array.set_last_add(false);
            return Ok(true);
        }
        let (reporter, _, _) = object_info(b, test.reporter)?;
        let (owner, _, owner_name) = object_info(b, test.owner)?;
        array.add_unique_delete_duplicate(AssertReport::new(ReportSpec {
            reporter: Some(reporter),
            test_index: test.index,
            test_class: test.class,
            owner: Some(owner),
            owner_type: test.owner_type,
            owner_type_string: Some(owner_name),
            other: None,
            other_type: 0,
            other_type_string: Some("NULL"),
            tol3d: crate::geom::tol::tol3d(test.tol),
            value: value(),
            list_index: test.list,
            message: test.message.map(|m| Cow::Owned(m.into_owned())),
        })?);
        Ok(false)
    }
}

/// The B-rep object (`this` of `SmBrep::AssertValid`).
const fn brep_object() -> Object {
    Object::Topology(EntityRef::Brep(BrepId::from_index(0)))
}

/// The class codes of the methods running the tests (`GetClassType()`).
mod class {
    pub(super) use crate::geom::assert_array::labels::{
        BREP_TYPE as BREP, EDGEUSE_TYPE as EDGEUSE, EDGE_TYPE as EDGE, FACEUSE_TYPE as FACEUSE,
        FACE_TYPE as FACE, LOOPUSE_TYPE as LOOPUSE, LOOP_TYPE as LOOP,
        OWNING_TOPOLOGY_TYPE as OWNING_TOPOLOGY, SHELL_TYPE as SHELL, TOPOLOGY_TYPE as TOPOLOGY,
        VERTEXUSE_TYPE as VERTEXUSE, VERTEX_TYPE as VERTEX,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::geom::assert_array::labels::BREP_TYPE;
    use crate::geom::extent1d::Extent1d;
    use crate::geom::vector3d::Vec3;
    use crate::nurbs::b_spline_curve::BSplineCurve;

    /// An edge use end without vertex makes the tolerance update an `SM_ERR` that says
    /// so.
    #[test]
    fn an_edge_use_end_without_vertex_is_a_typed_failure() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::ZERO,
            1.0,
            1.0,
            1.0,
        )
        .unwrap();
        let e = b.edge_list()[0];
        let eu = b.edge(e).unwrap().edgeuses[0];
        let vu = crate::topology::edgeuse::p1::vertexuse(&b, eu)
            .unwrap()
            .unwrap();
        b.vertexuse_mut(vu).unwrap().vertex = None;
        let err = validate_and_update_tolerances(&mut k, &mut b, true, None, 0).unwrap_err();
        assert_eq!(err.upstream_status(), Some(crate::error::SM_ERR), "{err:?}");
        assert_eq!(
            err.operation(),
            "topology::brep::validation::validate_and_update_tolerances"
        );
        assert_eq!(
            err.topology_failure(),
            Some(&crate::error::topology::TopologyFailure::VertexuseWithoutVertex)
        );
    }

    /// A `SmBrep::AssertValid` check (`assert_valid`) whose list holds the failed report
    /// of `SmBrep` test `index` (list 0) with owner type `owner`.
    fn check(assert_valid: bool, index: u64, owner: Option<&'static str>) -> BrepCheck {
        let mut reports = AssertArray::new();
        reports.add_unique_delete_duplicate(
            AssertReport::new(ReportSpec {
                test_index: index,
                test_class: BREP_TYPE,
                owner_type_string: owner,
                ..ReportSpec::default()
            })
            .unwrap(),
        );
        BrepCheck {
            pointers: PointerCheck::Passed,
            pointer_reports: AssertArray::new(),
            assert_valid: Some(assert_valid),
            reports,
        }
    }

    /// A report naming no owner type has no owner tag (#2013).
    #[test]
    fn line_omits_an_absent_owner_type() {
        let line = "SmBrep::Test #19 Region Connectivity: Regions found by topology \
                    traversal should be the same as the Brep's region list";
        assert_eq!(check(false, 19, None).lines(), [line]);
        let tagged = format!("{line} [owner SmBrep]");
        assert_eq!(check(false, 19, Some("SmBrep")).lines(), [tagged]);
    }

    /// The warning (test 21) stays listed and reachable but is not a failure line.
    #[test]
    fn warning_is_not_a_failure() {
        let c = check(true, 21, Some("SmBrep"));
        assert!(c.is_valid());
        assert_eq!(c.failure_count(), 0);
        assert_eq!(c.failures().count(), 0);
        let warnings: Vec<u64> = c.warnings().map(|r| r.test_index).collect();
        assert_eq!(warnings, [21]);
        assert_eq!(c.reports.len(), 1);
    }

    /// `AssertValid` returning `false` with only a warning listed still gives a line, so
    /// `is_valid` never contradicts it; a failure gives its own line instead.
    #[test]
    fn failed_assert_valid_with_only_a_warning_is_invalid() {
        let c = check(false, 21, Some("SmBrep"));
        assert_eq!(c.lines(), ["SmBrep::AssertValid failed without a report"]);
        assert!(!c.is_valid());
        let lines = check(false, 22, Some("SmBrep")).lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("SmBrep::Test #22 "), "{lines:?}");
    }

    /// A 10 mm box in a kernel of its own, with the check's memos and shortcuts on or off.
    fn checked_box(memoize: bool) -> (Kernel, Brep) {
        let mut k = Kernel {
            memoize_curve_caches: memoize,
            ..Kernel::default()
        };
        let b = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::ZERO,
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        (k, b)
    }

    /// `check_brep` of `b`, with how many `ValidatePointers` passes `AssertValid` ran.
    fn check_counting(k: &mut Kernel, b: &mut Brep) -> (BrepCheck, usize) {
        check_brep_counted(k, b, CheckLevel::Level2, ValidityScope::Body).unwrap()
    }

    /// The whole check, as the reports and statuses print.
    fn printed(check: &BrepCheck) -> String {
        format!("{check:?}")
    }

    /// The second `ValidatePointers` (in `AssertValid`) is skipped after a first pass that
    /// reported nothing and wrote nothing, and the check says what it said with it.
    #[test]
    fn a_pointer_pass_that_writes_nothing_is_not_run_again() {
        let (mut k, mut b) = checked_box(true);
        // (a first check may store what the body lacks, such as its gaps)
        check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        let (skipped, passes) = check_counting(&mut k, &mut b);
        assert_eq!(passes, 0, "the second pointer pass ran");
        assert!(skipped.is_valid(), "{:?}", skipped.lines());

        let (mut k, mut b) = checked_box(false);
        check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        let (full, passes) = check_counting(&mut k, &mut b);
        assert_eq!(passes, 1, "without the shortcut the second pass runs");
        assert_eq!(printed(&skipped), printed(&full));
    }

    /// A check leaves a body it has checked before as it was (#21105): the gaps it stores
    /// are stored already, the tolerance pass takes `bValidateOnly` as its argument instead
    /// of setting and restoring it on the body, and no curve is repaired. Compared by
    /// value, not by write stamps: the vertex/face gap refresh clears the caches of the
    /// vertex's other uses on the face and stores only its own (`CalcVertexFaceGap`), so a
    /// later read of a sibling computes its gap again and stores the same value.
    #[test]
    fn a_repeated_check_leaves_the_body_as_it_was() {
        for memoize in [true, false] {
            let (mut k, mut b) = checked_box(memoize);
            check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            let before = format!("{b:?}");
            let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            assert!(check.is_valid(), "{:?}", check.lines());
            assert!(
                format!("{b:?}") == before,
                "a repeated check changed the body (memoize {memoize})"
            );
        }
    }

    /// What attaching a UV trim writes, entity by entity: each edge use's own trim slot and
    /// drop record, and the edge and vertex tolerances an attached trim raises. Not the gaps
    /// ([`written_fields`]): #21105 keeps a check's stored gaps as derived values.
    fn trims_and_tolerances(b: &Brep) -> Vec<String> {
        let mut out = Vec::new();
        for &e in b.edge_list() {
            let edge = b.edge(e).unwrap();
            out.push(format!("{e:?} {:x}", edge.tolerance.to_bits()));
            for &eu in &edge.edgeuses {
                let u = b.edgeuse(eu).unwrap();
                out.push(format!(
                    "{eu:?} {:?} {:?}",
                    u.uv_trim_curve, u.drop_curve_fail
                ));
            }
        }
        for &v in b.vertex_list() {
            let tolerance = b.vertex(v).unwrap().tolerance;
            out.push(format!("{v:?} {:x}", tolerance.to_bits()));
        }
        out
    }

    /// #21105 step 1: a check never creates a UV trim on the body it was given. A body whose
    /// faces lack trims keeps its trims and tolerances through `check_brep` at levels 2 and 0
    /// and through `assert_valid` with the walk, and a second check reports what the first
    /// did (invariant 11 of `docs/validity-and-tolerances.md`).
    #[test]
    fn a_check_of_faces_without_trims_leaves_their_trims_and_tolerances() {
        for level in [CheckLevel::Level2, CheckLevel::Level0] {
            let (mut k, mut b) = box_and_cylinder(true);
            let lacking = b
                .face_list()
                .iter()
                .filter(|&&f| {
                    !crate::topology::trimming_tools::face_uv_curves_present(&b, f).unwrap()
                })
                .count();
            assert!(lacking > 0, "the united body already has every trim");
            let before = trims_and_tolerances(&b);
            let first = check_brep(&mut k, &mut b, level).unwrap();
            assert!(first.is_valid(), "{level:?}: {:?}", first.lines());
            assert_eq!(
                trims_and_tolerances(&b),
                before,
                "{level:?}: the check wrote trims or tolerances on the body"
            );
            let second = check_brep(&mut k, &mut b, level).unwrap();
            assert_eq!(printed(&second), printed(&first), "{level:?}");
            assert_eq!(trims_and_tolerances(&b), before, "{level:?}: second check");
        }
        let (mut k, mut b) = box_and_cylinder(true);
        let before = trims_and_tolerances(&b);
        let naming = b.naming();
        let recorded = b.geometry_owner_keys().clone();
        let keys =
            |e: EntityRef| crate::topology::owner_identity_of(naming, recorded.get(&e).copied(), e);
        assert!(assert_valid(&mut k, &mut b, None, 2, true, &keys).unwrap());
        assert_eq!(
            trims_and_tolerances(&b),
            before,
            "AssertValid with the walk wrote trims or tolerances on the body"
        );
        // a level past 4 is refused before any copy is made
        let err = assert_valid(&mut k, &mut b, None, 5, true, &keys).unwrap_err();
        assert!(matches!(err, SolidError::InvalidInput { .. }), "{err:?}");
    }

    /// Replaces the curve of the box's first edge by a cubic B-spline along the same line
    /// whose second control point lies 0.001 of the edge behind its start: `ValidatePointers` takes the
    /// polygon for a reversal and straightens it, a write to the body.
    fn bend_first_edge(k: &Kernel, b: &mut Brep) -> crate::topology::EdgeId {
        use crate::nurbs::b_spline_curve::p1::{
            BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
        };
        let e = b.edge_list()[0];
        let ivl = b.edge(e).unwrap().interval;
        // (a copy of a curve has no owner: read it from the edge's own curve)
        let owner = b.edge(e).unwrap().curve.as_ref().unwrap().owner();
        let old = b.edge(e).unwrap().curve.clone().unwrap();
        let start = old.evaluate_point(k, ivl.min()).unwrap();
        let end = old.evaluate_point(k, ivl.max()).unwrap();
        let along = end - start;
        let control = [
            start,
            start - along * 0.001,
            start + along * 0.3,
            start + along * 0.7,
            end,
        ];
        let curve = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &control,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[4, 1, 4],
            knots: &[ivl.min(), 0.5 * (ivl.min() + ivl.max()), ivl.max()],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap();
        let mut bent = Curve::from(curve);
        bent.set_owner(owner, None);
        b.edge_mut(e).unwrap().curve = Some(bent);
        e
    }

    /// The second polygon point of edge `e`'s B-spline curve.
    fn second_control_point(b: &Brep, e: crate::topology::EdgeId) -> crate::math::Point3 {
        let crate::curve::curve::CurveKind::BSpline(base) =
            b.edge(e).unwrap().curve.as_ref().unwrap().kind()
        else {
            panic!("the bent edge is a B-spline");
        };
        base.gw_nurb().unwrap().cpts()[1].to_euclid()
    }

    /// A reversed curve end is reported, not repaired (#21105 step 2): the check leaves the
    /// edge curve as it is and says so in one warning (test 147, which does not fail the
    /// check), the same with and without the memoized pass. The bent curve doubles back
    /// along its line, so its speed vanishes inside it: the check now sees that too
    /// (`SmBSplineCurve` test 12), where upstream straightened the end first and then
    /// checked the straightened curve. The explicit fix path
    /// (`ValidateGeometry(bFixTolerances = TRUE)`) still straightens the end, and the
    /// body it leaves checks valid with no warning.
    #[test]
    fn a_reversed_end_is_reported_not_repaired() {
        let (mut k, mut b) = checked_box(true);
        let e = bend_first_edge(&k, &mut b);
        let bent = second_control_point(&b, e);
        let (reported, _) = check_counting(&mut k, &mut b);
        assert_eq!(
            second_control_point(&b, e),
            bent,
            "the check repaired the reversed end"
        );
        let warnings: Vec<_> = reported.warnings().map(|r| r.test_index).collect();
        assert_eq!(warnings, [147], "{reported:?}");
        let failures: Vec<_> = reported
            .failures()
            .map(|r| (r.reporting_type, r.test_index))
            .collect();
        assert_eq!(
            failures,
            [(crate::geom::assert_array::labels::B_SPLINE_CURVE_TYPE, 12)],
            "{:?}",
            reported.lines()
        );

        let (mut k, mut full_b) = checked_box(false);
        let e = bend_first_edge(&k, &mut full_b);
        let (full, _) = check_counting(&mut k, &mut full_b);
        assert_eq!(printed(&reported), printed(&full));
        assert_eq!(second_control_point(&full_b, e), bent);

        let naming = b.naming();
        let recorded = b.geometry_owner_keys().clone();
        let keys =
            |e: EntityRef| crate::topology::owner_identity_of(naming, recorded.get(&e).copied(), e);
        super::super::validate_geometry(&k, &mut b, e, true, &keys).unwrap();
        assert_ne!(
            second_control_point(&b, e),
            bent,
            "the fix path left the end reversed"
        );
        let (fixed, _) = check_counting(&mut k, &mut b);
        assert!(fixed.is_valid(), "{:?}", fixed.lines());
        assert_eq!(fixed.warnings().count(), 0, "{:?}", fixed.lines());
    }

    /// A face without UV trims is checked on a copy standing for the body, which the first
    /// pass shares between faces. A second pass would make its own copy from the same body
    /// and check the same faces the same way, so the copy leaves the pass whole (#21131);
    /// the check says what the full check says.
    #[test]
    fn a_face_checked_on_a_standing_copy_leaves_the_pass_whole() {
        let strip = |b: &mut Brep| {
            let f = b.face_list()[0];
            for l in crate::topology::face::p2::loops(b, f, false, None).unwrap() {
                let lu = b.loop_(l).unwrap().loopuse.unwrap();
                for eu in crate::topology::loopuse::edgeuses(b, lu, None, None).unwrap() {
                    b.edgeuse_mut(eu).unwrap().uv_trim_curve = None;
                }
            }
        };
        let (mut k, mut b) = checked_box(true);
        check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        strip(&mut b);
        assert!(
            !crate::topology::trimming_tools::face_uv_curves_present(&b, b.face_list()[0]).unwrap()
        );
        assert!(
            pass_whole(&k, &b),
            "a face checked on the standing copy left the pass not whole"
        );
        let (standing, _) = check_counting(&mut k, &mut b);
        assert!(standing.assert_valid.is_some(), "{:?}", standing.lines());

        let (mut k, mut b) = checked_box(false);
        check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        strip(&mut b);
        let (full, _) = check_counting(&mut k, &mut b);
        assert_eq!(printed(&standing), printed(&full));
    }

    /// Whether the first pointer pass of `b`'s check (on a copy, so `b` is left as it is)
    /// checks every face to the end (`faces_whole`).
    fn pass_whole(k: &Kernel, b: &Brep) -> bool {
        let naming = b.naming();
        let recorded = b.geometry_owner_keys().clone();
        let keys =
            |e: EntityRef| crate::topology::owner_identity_of(naming, recorded.get(&e).copied(), e);
        let mut copy = b.clone_standing_for_self().unwrap();
        let mut whole = true;
        let mut reports = AssertArray::new();
        let _ = validate_pointers_traced(
            k,
            &mut copy,
            Some(&mut reports),
            2,
            false,
            &keys,
            &mut whole,
            false,
        );
        whole
    }

    /// A 10 mm cube united with a cylinder of radius 4 standing on its x = 10 face: a
    /// boolean result whose new faces have no UV trim curves yet.
    fn box_and_cylinder(memoize: bool) -> (Kernel, Brep) {
        let mut k = Kernel {
            memoize_curve_caches: memoize,
            ..Kernel::default()
        };
        let bx = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::ZERO,
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let cyl = crate::primitive::api_primitives::create_cylinder(
            &mut k,
            crate::math::Point3::new(10.0, 5.0, 0.0),
            4.0,
            10.0,
        )
        .unwrap();
        let b = crate::boolean::api_brep::boolean_union(&mut k, bx, cyl).unwrap();
        (k, b)
    }

    /// The fields a check may write, entity by entity: edge use trims and gaps, edge and
    /// vertex tolerances, vertex use gaps and UV points.
    fn written_fields(b: &Brep) -> Vec<String> {
        let mut out = Vec::new();
        for &e in b.edge_list() {
            let edge = b.edge(e).unwrap();
            out.push(format!(
                "{e:?} {:x} {:?}",
                edge.tolerance.to_bits(),
                edge.interval
            ));
            for &eu in &edge.edgeuses {
                let u = b.edgeuse(eu).unwrap();
                out.push(format!(
                    "{eu:?} {:?} {:?} {:?} {:?}",
                    u.uv_trim_curve, u.max_edge_face_gap, u.edge_ccw_edge_gap, u.drop_curve_fail
                ));
                if let Some(vu) = u.vertexuse {
                    let v = b.vertexuse(vu).unwrap();
                    out.push(format!(
                        "{vu:?} {:?} {:?} {:?}",
                        v.vertex_edge_gap, v.vertex_face_gap, v.uv_point
                    ));
                }
            }
        }
        for &v in b.vertex_list() {
            out.push(format!(
                "{v:?} {:x}",
                b.vertex(v).unwrap().tolerance.to_bits()
            ));
        }
        out
    }

    /// #21131: the first check of a body whose faces have no UV trims yet checks those faces
    /// on a copy standing for the body and writes nothing to the body in its first pointer
    /// pass, so the second pass is skipped. The check, and the body after it, are those of the
    /// full check.
    #[test]
    fn a_first_check_on_faces_without_trims_runs_one_pointer_pass() {
        let (mut k, mut b) = box_and_cylinder(true);
        assert!(
            b.face_list()
                .iter()
                .any(|&f| !crate::topology::trimming_tools::face_uv_curves_present(&b, f).unwrap()),
            "the united body already has every trim"
        );
        let (once, passes) = check_counting(&mut k, &mut b);
        assert!(once.is_valid(), "{:?}", once.lines());
        assert_eq!(
            passes, 0,
            "the second pointer pass ran after a first pass that wrote nothing"
        );

        let (mut k, mut full_b) = box_and_cylinder(false);
        let (full, passes) = check_counting(&mut k, &mut full_b);
        assert_eq!(passes, 1, "without the shortcut the second pass runs");
        assert_eq!(printed(&once), printed(&full));
        assert_eq!(written_fields(&b), written_fields(&full_b));
    }

    /// #18212: the two edges of face 9 of the `blend.feature_executive` case
    /// `single_face_fillet1019_6` (its base body) that start at the same vertex: e21, a
    /// line along -y, and e24, a cubic B-spline. e24 leaves the vertex with a hook (its
    /// tangent turns back within its first two spans, 1e-5 long), then runs off at about
    /// 38 degrees to the line. The hook makes every piece of e24 that holds its start
    /// half closed, so the coincidence check splits e24 toward the vertex again and
    /// again (`split_in_three`).
    const E24_POINTS: [[f64; 3]; 49] = [
        [-23.583494549520516, -18.522825108202344, 20.76275230723294],
        [-23.58349459143391, -18.52282395378618, 20.762751828162166],
        [-23.583494595634157, -18.5228233855534, 20.762751780152975],
        [-23.583494490683165, -18.52282618259411, 20.762752979743382],
        [-23.583494480323253, -18.522826458695306, 20.76275309815723],
        [-23.583494475854078, -18.522826775110488, 20.76275314923958],
        [-23.583494459603386, -18.52282701089772, 20.762753334984925],
        [-23.57975849664189, -18.577033463388723, 20.80545537693825],
        [-23.576299150809056, -18.59481795336009, 20.840961491732475],
        [-23.570566298018644, -18.641506058425936, 20.891588722963668],
        [-23.54654350148155, -18.837146673962945, 21.103735749857037],
        [-23.50653855063268, -19.044679767419034, 21.327508136798212],
        [-23.455165778582174, -19.239079127244253, 21.53590303455785],
        [-23.40349431437229, -19.434608766052875, 21.745509584317777],
        [-23.335658662649085, -19.63623910179359, 21.96038290867134],
        [-23.256275701436827, -19.828640610325692, 22.164139357777195],
        [-23.074565734169486, -20.269053405258784, 22.630543950552934],
        [-22.819172928322157, -20.701735998971227, 23.082036679774845],
        [-22.487751803872957, -21.10604782090713, 23.495808567678164],
        [-22.340142761229725, -21.286121076842413, 23.6800951678386],
        [-22.184240193141484, -21.453672645044414, 23.849943504779972],
        [-22.005844048743693, -21.621357645936385, 24.017970032779992],
        [-21.887249900768204, -21.7328312423876, 24.1296706695438],
        [-21.767080516242284, -21.836521080702738, 24.232709389564906],
        [-21.63461668383053, -21.940843293861924, 24.335365007227068],
        [-21.50189328039518, -22.04536993281161, 24.438221784883392],
        [-21.3695751656428, -22.14064516919224, 24.53098268154603],
        [-21.22296483491195, -22.236366133425825, 24.62298129408234],
        [-21.07615235958569, -22.3322190765963, 24.715106753220912],
        [-20.932020914683036, -22.41755908610193, 24.795953912783766],
        [-20.771654379641078, -22.502666145431935, 24.875125579042553],
        [-20.61121003287725, -22.58781449970435, 24.95433566032149],
        [-20.455930010450004, -22.661498076158615, 25.021442324367364],
        [-20.282825292484432, -22.733707553710197, 25.085375223486487],
        [-20.168212803509658, -22.78151738839882, 25.127705142517772],
        [-20.056935280731047, -22.823934953036566, 25.164421683625797],
        [-19.937305755801546, -22.86516127680678, 25.19910297484565],
        [-19.81778136440151, -22.906351369814335, 25.233753787247217],
        [-19.702341267930038, -22.942088438398383, 25.262790246539474],
        [-19.57848521208208, -22.976010541556747, 25.28910281100417],
        [-19.454798265032633, -23.009886328642242, 25.315379449198392],
        [-19.33589327912489, -23.038354204575455, 25.336152176336377],
        [-19.208765003590884, -23.06433026986784, 25.353476642490936],
        [-19.081869599573096, -23.090258752624514, 25.37076937396897],
        [-18.960376411451612, -23.11093009443519, 25.382777570192548],
        [-18.83112589494237, -23.128431954946468, 25.390636145229386],
        [-18.72734100533557, -23.142485505447727, 25.3969463818512],
        [-18.627420157321055, -23.1532564794986, 25.399999883347043],
        [-18.522831575897523, -23.161612169449725, 25.399999999996652],
    ];
    /// e24's knots: its breakpoints, the ends four times and the others three times
    /// (a C0 piecewise cubic).
    const E24_BREAKS: [f64; 17] = [
        1.3321546162e-5,
        2.2457257262e-5,
        2.335906065e-5,
        0.207344008492958,
        1.076095040715497,
        1.949897175135157,
        3.950056337993551,
        4.840891464011217,
        5.433100634861487,
        6.026470275991541,
        6.620658048186671,
        7.215134127251169,
        7.608736241961278,
        8.001992448923703,
        8.394711717387553,
        8.78671160905571,
        9.101477588963798,
    ];
    const E21_POINTS: [[f64; 3]; 2] = [
        [-23.583494648739176, -18.522825582433015, 20.762751173157692],
        [-23.58349464873918, -23.58349464873912, 20.76275117315766],
    ];
    const E21_KNOTS: [f64; 4] = [
        0.6216137551118551,
        0.6216137551118551,
        0.6548401635662747,
        0.6548401635662747,
    ];
    const E21_POINT: [f64; 3] = [-23.583494648739105, 76.15431973709104, 20.762751173158243];
    const E21_VECTOR: [f64; 3] = [-7.464240906760062e-16, -1.0, -5.8314382084062985e-15];
    const E21_DOMAIN: [f64; 2] = [0.6216137551118551, 0.6548401635662747];
    const E21_SCALE: f64 = 152.3086394741821;

    /// e21 (the line) and e24 (the B-spline) with their edge intervals.
    fn issue_18212_edges() -> ((Curve, Extent1d), (Curve, Extent1d)) {
        use crate::curve::line::Line;
        use crate::nurbs::math::{CPoint, NOW};
        let cpoint = |p: &[f64; 3]| CPoint {
            x: p[0],
            y: p[1],
            z: p[2],
            w: NOW,
        };
        let e24_points: Vec<CPoint> = E24_POINTS.iter().map(cpoint).collect();
        let last = E24_BREAKS.len() - 1;
        let e24_knots: Vec<f64> = E24_BREAKS
            .iter()
            .enumerate()
            .flat_map(|(i, &u)| std::iter::repeat_n(u, if i == 0 || i == last { 4 } else { 3 }))
            .collect();
        assert_eq!(e24_knots.len(), 53);
        let e24 = BSplineCurve::from_data(3, 48, 52, 3, &e24_points, &e24_knots).unwrap();
        let e24_interval = Extent1d::from_min_max(E24_BREAKS[0], E24_BREAKS[last]);
        let e21_points: Vec<CPoint> = E21_POINTS.iter().map(cpoint).collect();
        let e21_base = BSplineCurve::from_data(3, 1, 3, 1, &e21_points, &E21_KNOTS).unwrap();
        let [x, y, z] = E21_POINT;
        let [vx, vy, vz] = E21_VECTOR;
        let e21 = Line::from_parts(
            e21_base,
            Vec3::new(x, y, z),
            Vec3::new(vx, vy, vz),
            Extent1d::from_min_max(E21_DOMAIN[0], E21_DOMAIN[1]),
            E21_SCALE,
            false,
        );
        let e21_interval = Extent1d::from_min_max(E21_DOMAIN[0], E21_DOMAIN[1]);
        (
            (Curve::from(e21), e21_interval),
            (Curve::from(e24), e24_interval),
        )
    }

    /// #18212: test 34 does not report e21 and e24, which only share a vertex, as
    /// partially coincident at any tolerance. Before, it did at every tolerance whose
    /// split put a piece of e24 inside the stretch next to the vertex where the two
    /// run within the tolerance of each other (1.4e-3, 1.4897e-3, 2.5e-3, 4e-3 and
    /// 1e-2 among the issue's), and not at the tolerances between.
    #[test]
    fn face34_edges_that_only_share_a_vertex_are_not_partially_coincident() {
        let k = Kernel::default();
        let ((c21, i21), (c24, i24)) = issue_18212_edges();
        let issue = [
            1e-4, 5e-4, 1e-3, 1.3e-3, 1.4e-3, 1.4897e-3, 2e-3, 2.5e-3, 2.889e-3, 4e-3, 1e-2,
        ];
        // and 200 tolerances spaced evenly in log from 1e-5 to 2e-2
        let sweep = (0..200).map(|i| 1e-5 * 2000.0_f64.powf(f64::from(i) / 199.0));
        for t in issue.into_iter().chain(sweep) {
            let found = face::partially_coincident(&k, &c21, &i21, &c24, &i24, t).unwrap();
            assert!(!found, "tol {t:e}");
            let found = face::partially_coincident(&k, &c24, &i24, &c21, &i21, t).unwrap();
            assert!(!found, "tol {t:e}, e24 first");
        }
    }

    /// #18212, the counter-cases: edges that really share a stretch stay partially
    /// coincident, from the shared vertex on (the first half of e21 against e21) and
    /// through the hook's splits (e24 over [1, 3] against e24, all of e24 against
    /// itself).
    #[test]
    fn face34_edges_that_share_a_stretch_stay_partially_coincident() {
        let k = Kernel::default();
        let ((c21, i21), (c24, i24)) = issue_18212_edges();
        let half = Extent1d::from_min_max(i21.min(), 0.5 * (i21.min() + i21.max()));
        let middle = Extent1d::from_min_max(1.0, 3.0);
        for t in [1e-5, 1.4e-3, 2.889e-3, 4e-3, 1e-2] {
            let found = face::partially_coincident(&k, &c21, &half, &c21, &i21, t).unwrap();
            assert!(found, "tol {t:e}, half of e21");
            let found = face::partially_coincident(&k, &c24, &middle, &c24, &i24, t).unwrap();
            assert!(found, "tol {t:e}, e24 over [1, 3]");
            let found = face::partially_coincident(&k, &c24, &i24, &c24, &i24, t).unwrap();
            assert!(found, "tol {t:e}, all of e24");
        }
    }
}
