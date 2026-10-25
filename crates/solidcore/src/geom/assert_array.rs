//! `AssertValid` bookkeeping: one failed-check report ([`AssertReport`]) with its
//! static label, and the duplicate-free report list ([`AssertArray`]) the
//! `SM_ASSERT_*_REPORT` macros fill.
//!
//! Upstream: `source/SMLib/inc/SmAssertArray.h`, `source/SMLib/src/SmAssertArray.cpp`.
//!
//! | Rust                                   | Upstream                                         |
//! |----------------------------------------|--------------------------------------------------|
//! | [`AssertType`]                         | `SmAssertType`                                   |
//! | [`AssertReportLabel`]                  | `SmAssertReportLabel`                            |
//! | [`labels`] (`labels::CIRCLE`, ...)     | the `extern SmAssertReportLabel s*_list[]` tables |
//! | [`assert_label_list`]                  | `SM_GetAssertLabelList`                          |
//! | [`ObjectKey`]                          | the `const void *` owner / other / reporter pointers |
//! | [`AssertReport`]                       | `SmAssertReport`                                 |
//! | [`ReportSpec`]                         | the arguments of `SmAssertReport::SmAssertReport` |
//! | [`AssertReport::new`]                  | `SmAssertReport::SmAssertReport`                 |
//! | `PartialEq for AssertReport`           | `SmAssertReport::operator==`                     |
//! | [`AssertReport::set_value`]            | `SmAssertReport::SetValue`                       |
//! | [`AssertReport::set_owner_param`]      | `SmAssertReport::SetOwnerParam`                  |
//! | (`Drop`)                               | `SmAssertReport::~SmAssertReport`                |
//! | [`AssertArray`]                        | `SmAssertArray`                                  |
//! | [`AssertArray::contains`]              | `SmAssertArray::Contains`                        |
//! | [`AssertArray::add_unique_delete_duplicate`] | `SmAssertArray::AddUniqueDeleteDuplicate`  |
//! | [`AssertArray::last_add`]              | `SmAssertArray::GetLastAdd`                      |
//! | [`AssertArray::set_last_add`]          | `SmAssertArray::SetLastAdd`                      |
//! | [`AssertArray::len`], [`AssertArray::get`], [`AssertArray::reports`] | `SmTArray<SmAssertReport*>::GetSize`, `GetAt`, `GetReports` |
//!
//! Deviations:
//! - Object pointers become [`ObjectKey`]s (`None` = `NULL`); reports compare them by
//!   value exactly as upstream compares pointers. Type codes stay plain `i64`
//!   (`SM_TYPE`), list and test indices `u64` (`ULONG`).
//! - The label tables, declared `extern` in `SmAssertArray.h` but defined beside each
//!   `AssertValid` method, are all ported here (module [`labels`]) so that the units
//!   porting those methods share one copy.
//! - `SmAssertReport::SmAssertReport` indexes the label table without a bounds check;
//!   [`AssertReport::new`] returns [`SolidError::InvalidInput`] for a test index past
//!   the end of the table instead of reading out of bounds.
//! - The optional message is an owned string ([`Cow`]) instead of a borrowed pointer.
//! - `AddUniqueDeleteDuplicate(NULL)` (never produced by the macros) is not
//!   representable: the report is passed by value. A duplicate report is dropped,
//!   as upstream deletes it.
//! - `SM_DEBUG_CODE` members (`m_file_name`, `m_line_num`, the debug drawing in the
//!   constructor) are not ported; the reference build does not define it.
//! - Report membership uses a call-owned ordered index over exactly the fields
//!   compared by `operator==`; the report vector retains insertion order. This
//!   avoids quadratic duplicate searches when validating a large invalid B-rep.

pub mod labels;

use std::borrow::Cow;
use std::collections::BTreeSet;

use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::math::UNDEF_DOUBLE;

/// Category of an assert report (`SmAssertType`), used to group failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum AssertType {
    /// `SM_AT_UNKNOWN`: an uninitialized value.
    #[default]
    Unknown,
    /// `SM_AT_ANGLE`: an angle is less than a tolerance.
    Angle,
    /// `SM_AT_BOX`: box check.
    Box,
    /// `SM_AT_CACHE`: data consistency.
    Cache,
    /// `SM_AT_CLOSED`: closedness.
    Closed,
    /// `SM_AT_COINCIDENCE`: coincidence.
    Coincidence,
    /// `SM_AT_DEGENERATE`: degeneracy.
    Degenerate,
    /// `SM_AT_DIRECTION`: direction.
    Direction,
    /// `SM_AT_DISTANCE`: distance.
    Distance,
    /// `SM_AT_DOMAIN`: domain.
    Domain,
    /// `SM_AT_FLAG`: flags are set correctly.
    Flag,
    /// `SM_AT_GEOMETRIC`: a geometric property.
    Geometric,
    /// `SM_AT_TOPOLOGICAL`: a topology graph property.
    Topological,
    /// `SM_AT_GAP`: a gap is less than a tolerance.
    Gap,
    /// `SM_AT_INSIDE`: insideness.
    Inside,
    /// `SM_AT_KNOTS`: number of knots.
    Knots,
    /// `SM_AT_LIST`: list membership.
    List,
    /// `SM_AT_MARK`: marks.
    Mark,
    /// `SM_AT_MINMAX`: min is less than max.
    Minmax,
    /// `SM_AT_PARAMETERIZATION`: parameterization issues.
    Parameterization,
    /// `SM_AT_POINTER`: pointers.
    Pointer,
    /// `SM_AT_POINTS`: points are within tolerance.
    Points,
    /// `SM_AT_RADIUS`: x and y radius agree.
    Radius,
    /// `SM_AT_SCALE`: scale.
    Scale,
    /// `SM_AT_SIZE`: size.
    Size,
    /// `SM_AT_TYPE`: data types are correct.
    Type,
    /// `SM_AT_VALUES`: values.
    Values,
    /// `SM_AT_VECTOR`: a vector has length.
    Vector,
    /// `SM_AT_UNIT_VECTOR`: a vector is unit length.
    UnitVector,
    /// `SM_AT_POLE`: poles.
    Pole,
    /// `SM_AT_NESTED_TEST`: the object must pass a nested test.
    NestedTest,
    /// `SM_AT_NESTED_PROPERTY`: the object must have a nested property.
    NestedProperty,
}

impl AssertType {
    /// The upstream enumerator value (`SM_AT_UNKNOWN` = 0, in declaration order).
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// One static label of an `AssertValid` check (`SmAssertReportLabel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssertReportLabel {
    /// Category of the check (`m_eAssertType`).
    pub assert_type: AssertType,
    /// Short identifier (`m_pName`).
    pub name: &'static str,
    /// Detailed message (`m_pMessage`; `None` = `NULL`).
    pub message: Option<&'static str>,
    /// Message without values, for log files (`m_pMessageForFile`; `None` = `NULL`).
    pub message_for_file: Option<&'static str>,
}

/// `SM_GetAssertLabelList`: the label table for checks run by an object of type
/// `type_code`; `list_index` picks one of several tables (only `SmPolarConversion`,
/// `SmBrep` and `SmTrimmingTools` have more than one). Unknown types and list indices
/// give [`labels::DEFAULT`].
#[must_use]
pub fn assert_label_list(type_code: i64, list_index: u64) -> &'static [AssertReportLabel] {
    labels::label_list(type_code, list_index)
}

/// Identity of an object named by a report (upstream compares raw object pointers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectKey(pub u64);

/// The inputs of one report (`SmAssertReport::SmAssertReport` arguments).
#[derive(Debug, Clone, PartialEq)]
pub struct ReportSpec {
    /// Object whose `AssertValid` ran the check (`pReporter`).
    pub reporter: Option<ObjectKey>,
    /// Index of the check in its label table (`lTestIndex`).
    pub test_index: u64,
    /// Type of the class running the check, selects the label table (`lTestClass`).
    pub test_class: i64,
    /// Object the check is about (`pOwner`).
    pub owner: Option<ObjectKey>,
    /// Type code of the owner (`lOwnerType`).
    pub owner_type: i64,
    /// Type name of the owner (`sOwnerTypeString`).
    pub owner_type_string: Option<&'static str>,
    /// Optional second object, e.g. for gap checks (`pOther`).
    pub other: Option<ObjectKey>,
    /// Type code of the other object (`lOtherType`).
    pub other_type: i64,
    /// Type name of the other object (`sOtherTypeString`).
    pub other_type_string: Option<&'static str>,
    /// Tolerance used by the check (`sTol3d`, already a tolerance value: see
    /// [`tol3d`](crate::geom::tol::tol3d)); [`UNDEF_DOUBLE`] = none.
    pub tol3d: f64,
    /// Value checked against the tolerance (`dVal`); [`UNDEF_DOUBLE`] = none.
    pub value: f64,
    /// Which label table of `test_class` (`lListIndex`).
    pub list_index: u64,
    /// Message replacing the label's (`pOptMessage`; `None` or empty = use the label).
    pub message: Option<Cow<'static, str>>,
}

impl Default for ReportSpec {
    fn default() -> Self {
        Self {
            reporter: None,
            test_index: 0,
            test_class: 0,
            owner: None,
            owner_type: 0,
            owner_type_string: None,
            other: None,
            other_type: 0,
            other_type_string: None,
            tol3d: UNDEF_DOUBLE,
            value: UNDEF_DOUBLE,
            list_index: 0,
            message: None,
        }
    }
}

/// One failed `AssertValid` check (`SmAssertReport`).
#[derive(Debug, Clone)]
pub struct AssertReport {
    /// `true` = the check passed (`m_bOK`; always `false` for a new report).
    pub ok: bool,
    /// Object whose `AssertValid` produced the report (`m_pReporter`).
    pub reporter: Option<ObjectKey>,
    /// Type of the reporting class (`m_lReportingType`).
    pub reporting_type: i64,
    /// Label table index of the reporting type (`m_lListIndex`).
    pub list_index: u64,
    /// Index of the check in its label table (`m_lTestIndex`).
    pub test_index: u64,
    /// Category of the check (`m_eAssertType`).
    pub assert_type: AssertType,
    /// Name of the check (`m_pName`).
    pub name: &'static str,
    /// Description of the check (`m_pMessage`; `None` = `NULL`).
    pub message: Option<Cow<'static, str>>,
    /// Description without values (`m_pMessageForFile`; `None` = `NULL`).
    pub message_for_file: Option<&'static str>,
    /// Object the check is about (`m_pOwner`).
    pub owner: Option<ObjectKey>,
    /// Type code of the owner (`m_lOwnerType`).
    pub owner_type: i64,
    /// Type name of the owner (`m_sOwnerTypeString`).
    pub owner_type_string: Option<&'static str>,
    /// Optional second object (`m_pOther`).
    pub other: Option<ObjectKey>,
    /// Type code of the other object (`m_lOtherType`).
    pub other_type: i64,
    /// Type name of the other object (`m_sOtherTypeString`).
    pub other_type_string: Option<&'static str>,
    /// Tolerance of the check (`m_sTol3d`).
    pub tol3d: f64,
    /// Checked value (`m_dValue`).
    pub value: f64,
    /// Parameter on the owner (`m_sOwnerParam`; uninitialized when new).
    pub owner_param: Point2,
    /// Parameter on the other object (`m_sOtherParam`; uninitialized when new).
    pub other_param: Point2,
    /// 3D distance (`m_dDist3d`; [`UNDEF_DOUBLE`] when new).
    pub dist3d: f64,
}

impl AssertReport {
    /// `SmAssertReport::SmAssertReport`: a failed report for check `spec.test_index`
    /// of the label table `assert_label_list(spec.test_class, spec.list_index)`.
    ///
    /// # Errors
    /// [`SolidError::InvalidInput`] when `spec.test_index` is past the end of the table
    /// (upstream reads out of bounds).
    pub fn new(spec: ReportSpec) -> Result<Self> {
        let list = assert_label_list(spec.test_class, spec.list_index);
        let label = usize::try_from(spec.test_index)
            .ok()
            .and_then(|i| list.get(i))
            .ok_or(SolidError::InvalidInput {
                operation: "geom::assert_array::AssertReport::new",
                reason: "test index is past the end of the assert label list",
            })?;
        let message = match spec.message {
            Some(m) if !m.is_empty() => Some(m),
            _ => label.message.map(Cow::Borrowed),
        };
        let mut owner_param = Point2::default();
        owner_param.set_uninitialized();
        let mut other_param = Point2::default();
        other_param.set_uninitialized();
        Ok(Self {
            ok: false,
            reporter: spec.reporter,
            reporting_type: spec.test_class,
            list_index: spec.list_index,
            test_index: spec.test_index,
            assert_type: label.assert_type,
            name: label.name,
            message,
            message_for_file: label.message_for_file,
            owner: spec.owner,
            owner_type: spec.owner_type,
            owner_type_string: spec.owner_type_string,
            other: spec.other,
            other_type: spec.other_type,
            other_type_string: spec.other_type_string,
            tol3d: spec.tol3d,
            value: spec.value,
            owner_param,
            other_param,
            dist3d: UNDEF_DOUBLE,
        })
    }

    /// `SmAssertReport::SetValue`.
    pub fn set_value(&mut self, value: f64) {
        self.value = value;
    }

    /// `SmAssertReport::SetOwnerParam`.
    pub fn set_owner_param(&mut self, owner_param: Point2) {
        self.owner_param = owner_param;
    }
}

/// `SmAssertReport::operator==`: same reporting type, list index, test index, category,
/// pass flag, owner and other object (tolerances, values and strings are ignored).
impl PartialEq for AssertReport {
    fn eq(&self, other: &Self) -> bool {
        self.reporting_type == other.reporting_type
            && self.list_index == other.list_index
            && self.test_index == other.test_index
            && self.assert_type == other.assert_type
            && self.ok == other.ok
            && self.owner == other.owner
            && self.other == other.other
    }
}

impl Eq for AssertReport {}

type ReportIdentity = (
    i64,
    u64,
    u64,
    AssertType,
    bool,
    Option<ObjectKey>,
    Option<ObjectKey>,
);

fn report_identity(report: &AssertReport) -> ReportIdentity {
    (
        report.reporting_type,
        report.list_index,
        report.test_index,
        report.assert_type,
        report.ok,
        report.owner,
        report.other,
    )
}

/// The reports of an `AssertValid` run (`SmAssertArray`), without duplicates when
/// filled through [`AssertArray::add_unique_delete_duplicate`].
#[derive(Debug, Clone, Default)]
pub struct AssertArray {
    reports: Vec<AssertReport>,
    identities: BTreeSet<ReportIdentity>,
    last_add: bool,
}

impl AssertArray {
    /// An empty array (`SmAssertArray()`; `m_bLastAdd` = `FALSE`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of reports (`GetSize`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.reports.len()
    }

    /// `true` when there are no reports.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reports.is_empty()
    }

    /// Report `index` (`GetAt`), `None` past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&AssertReport> {
        self.reports.get(index)
    }

    /// Every report, in insertion order (`GetReports`).
    #[must_use]
    pub fn reports(&self) -> &[AssertReport] {
        &self.reports
    }

    /// `SmAssertArray::Contains`: `true` when a report equal to `report` (see
    /// `PartialEq for AssertReport`) is already in the array.
    #[must_use]
    pub fn contains(&self, report: &AssertReport) -> bool {
        self.identities.contains(&report_identity(report))
    }

    /// `SmAssertArray::AddUniqueDeleteDuplicate`: appends `report` unless an equal
    /// report is already present (then `report` is dropped). Returns `true` when it was
    /// added and records the same flag for [`AssertArray::last_add`].
    pub fn add_unique_delete_duplicate(&mut self, report: AssertReport) -> bool {
        self.last_add = false;
        if self.identities.insert(report_identity(&report)) {
            self.reports.push(report);
            self.last_add = true;
            true
        } else {
            false
        }
    }

    /// `SmAssertArray::GetLastAdd`: whether the last
    /// [`add_unique_delete_duplicate`](AssertArray::add_unique_delete_duplicate) added
    /// its report.
    #[must_use]
    pub fn last_add(&self) -> bool {
        self.last_add
    }

    /// `SmAssertArray::SetLastAdd`.
    pub fn set_last_add(&mut self, last_add: bool) {
        self.last_add = last_add;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(test_class: i64, test_index: u64, owner: u64) -> ReportSpec {
        ReportSpec {
            reporter: Some(ObjectKey(owner)),
            test_index,
            test_class,
            owner: Some(ObjectKey(owner)),
            ..ReportSpec::default()
        }
    }

    #[test]
    fn label_lists_dispatch() {
        assert_eq!(assert_label_list(labels::CIRCLE_TYPE, 0), labels::CIRCLE);
        assert_eq!(assert_label_list(labels::CIRCLE_TYPE, 7), labels::CIRCLE);
        assert_eq!(
            assert_label_list(labels::BREP_TYPE, 3),
            labels::COINCIDENT_TOPOLOGY
        );
        assert_eq!(assert_label_list(labels::BREP_TYPE, 5), labels::DEFAULT);
        assert_eq!(
            assert_label_list(labels::POLAR_CONVERSION_TYPE, 1),
            labels::POLAR_IS_CURRENT
        );
        assert_eq!(assert_label_list(-1, 0), labels::DEFAULT);
        assert_eq!(labels::DEFAULT.len(), 1);
        for &(t, i) in labels::LABELLED {
            assert!(!assert_label_list(t, i).is_empty());
        }
    }

    #[test]
    fn new_report_from_label() {
        let r = AssertReport::new(spec(labels::CIRCLE_TYPE, 0, 5)).unwrap();
        assert!(!r.ok);
        assert_eq!(r.assert_type, AssertType::Radius);
        assert_eq!(r.name, "Radius");
        assert_eq!(
            r.message.as_deref(),
            Some("m_dRadiusAtXAxis and m_dRadiusAtYAxis are equal")
        );
        assert_eq!(r.message_for_file, None);
        assert_eq!(r.owner_param, Point2::UNDEF);
        assert_eq!(r.dist3d.to_bits(), UNDEF_DOUBLE.to_bits());
        assert_eq!(r.tol3d.to_bits(), UNDEF_DOUBLE.to_bits());
        assert!(AssertReport::new(spec(labels::CIRCLE_TYPE, 1, 5)).is_err());
    }

    #[test]
    fn optional_message_overrides_unless_empty() {
        let mut s = spec(labels::LINE_TYPE, 0, 1);
        s.message = Some(Cow::Borrowed(""));
        let r = AssertReport::new(s.clone()).unwrap();
        assert_eq!(
            r.message.as_deref(),
            labels::LINE.first().and_then(|l| l.message)
        );
        s.message = Some(Cow::Owned("custom".to_string()));
        assert_eq!(
            AssertReport::new(s).unwrap().message.as_deref(),
            Some("custom")
        );
    }

    #[test]
    fn poly_loop_label_without_message() {
        let i = labels::POLY_LOOP
            .iter()
            .position(|l| l.message.is_none())
            .unwrap();
        let r = AssertReport::new(spec(labels::POLY_LOOP_TYPE, i as u64, 1)).unwrap();
        assert_eq!(r.message, None);
    }

    #[test]
    fn equality_and_unique_add() {
        let a = AssertReport::new(spec(labels::BREP_TYPE, 2, 9)).unwrap();
        let mut b = a.clone();
        b.set_value(3.0);
        b.set_owner_param(Point2::new(1.0, 2.0));
        b.reporter = Some(ObjectKey(77));
        assert_eq!(a, b);
        let mut c = a.clone();
        c.other = Some(ObjectKey(1));
        assert_ne!(a, c);

        let mut arr = AssertArray::new();
        assert!(!arr.last_add());
        assert!(arr.add_unique_delete_duplicate(a.clone()));
        assert!(arr.last_add());
        assert!(!arr.add_unique_delete_duplicate(b));
        assert!(!arr.last_add());
        assert!(arr.add_unique_delete_duplicate(c.clone()));
        assert_eq!(arr.len(), 2);
        assert!(arr.contains(&c));
        assert_eq!(arr.get(1), Some(&c));
        arr.set_last_add(false);
        assert!(!arr.last_add());
    }
}
