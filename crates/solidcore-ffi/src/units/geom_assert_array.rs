//! FFI wrappers for unit `geom.assert_array`.
//!
//! Shim: `harness/cpp/shim/geom_assert_array.cpp`.
//! Upstream: `source/SMLib/inc/SmAssertArray.h`, `source/SMLib/src/SmAssertArray.cpp`.
//!
//! | Rust         | shim                             | upstream                                   |
//! |--------------|----------------------------------|--------------------------------------------|
//! | [`report`]   | `sc_geom_assert_array_report`    | `SmAssertReport::SmAssertReport` (+ `SM_GetAssertLabelList`), `SetValue`, `SetOwnerParam` |
//! | [`sequence`] | `sc_geom_assert_array_sequence`  | `SmAssertArray::AddUniqueDeleteDuplicate`, `Contains`, `GetLastAdd`, `SetLastAdd`, `SmAssertReport::operator==` |
//!
//! Object pointers are integer keys (`0` = `NULL`).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_assert_array_report(
            spec_i: *const i64,
            spec_d: *const f64,
            owner_ts: *const u8,
            owner_ts_len: i64,
            other_ts: *const u8,
            other_ts_len: i64,
            msg: *const u8,
            msg_len: i64,
            set_value: i32,
            value: f64,
            set_owner_param: i32,
            param: *const f64,
            out_i: *mut i64,
            out_d: *mut f64,
            str_lens: *mut i64,
            out_s: *mut u8,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;

        pub fn sc_geom_assert_array_sequence(
            n: i64,
            kinds: *const i32,
            flags: *const i32,
            spec_i: *const i64,
            spec_d: *const f64,
            results: *mut i64,
            last_add: *mut i64,
            out_elems: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Integer fields of a report per element of [`sequence`]'s output and of
/// [`Report::ints`]: ok, reporter, reporting type, list index, test index, assert type,
/// owner, owner type, other, other type.
pub const REPORT_INTS: usize = 10;

/// Inputs of one `SmAssertReport` constructor call.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spec {
    /// `pReporter` key (0 = NULL).
    pub reporter: i64,
    /// `lTestIndex`; must be inside the label list (see [`label_list_len`]).
    pub test_index: u64,
    /// `lTestClass`.
    pub test_class: i64,
    /// `pOwner` key.
    pub owner: i64,
    /// `lOwnerType`.
    pub owner_type: i64,
    /// `pOther` key.
    pub other: i64,
    /// `lOtherType`.
    pub other_type: i64,
    /// `lListIndex`.
    pub list_index: u64,
    /// `sTol3d` (converted with `SmTol3d(double)`).
    pub tol: f64,
    /// `dVal`.
    pub value: f64,
}

impl Spec {
    fn ints(&self) -> Result<[i64; 8], FfiError> {
        if self.test_index >= label_list_len(self.test_class, self.list_index) {
            return Err(FfiError::InvalidArgument(
                "test_index outside the label list".into(),
            ));
        }
        let ti = i64::try_from(self.test_index)
            .map_err(|_| FfiError::InvalidArgument("test_index".into()))?;
        let li = i64::try_from(self.list_index)
            .map_err(|_| FfiError::InvalidArgument("list_index".into()))?;
        // ULONG is 64-bit on the LP64 reference build; SM_TYPE is long.
        Ok([
            self.reporter,
            ti,
            self.test_class,
            self.owner,
            self.owner_type,
            self.other,
            self.other_type,
            li,
        ])
    }
}

/// Fields of a constructed report.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// See [`REPORT_INTS`].
    pub ints: [i64; REPORT_INTS],
    /// tol3d, value, owner param (2), other param (2), dist3d.
    pub doubles: [f64; 7],
    /// name, message, message for file, owner type string, other type string
    /// (`None` = NULL).
    pub strings: [Option<Vec<u8>>; 5],
}

fn str_arg(s: Option<&str>) -> Result<(*const u8, i64), FfiError> {
    match s {
        None => Ok((std::ptr::null(), -1)),
        Some(s) => {
            if s.as_bytes().contains(&0) {
                return Err(FfiError::InvalidArgument("string contains NUL".into()));
            }
            let n = i64::try_from(s.len()).map_err(|_| FfiError::InvalidArgument("len".into()))?;
            Ok((s.as_ptr(), n))
        }
    }
}

/// Constructs an `SmAssertReport` from `spec` and the optional strings, then calls
/// `SetValue(v)` when `set_value` is given and `SetOwnerParam(p)` when `owner_param`
/// is given, and returns the report's fields.
///
/// Fails with [`FfiError::InvalidArgument`] when `spec.test_index` is outside the label
/// list (upstream would read out of bounds; see [`label_list_len`]).
pub fn report(
    spec: &Spec,
    owner_type_string: Option<&str>,
    other_type_string: Option<&str>,
    message: Option<&str>,
    set_value: Option<f64>,
    owner_param: Option<[f64; 2]>,
) -> Result<Report, FfiError> {
    let si = spec.ints()?;
    let sd = [spec.tol, spec.value];
    let (owner_p, owner_n) = str_arg(owner_type_string)?;
    let (other_p, other_n) = str_arg(other_type_string)?;
    let (msg_p, msg_n) = str_arg(message)?;
    let param = owner_param.unwrap_or([0.0, 0.0]);
    let mut ints = [0i64; REPORT_INTS];
    let mut doubles = [0f64; 7];
    let mut lens = [0i64; 5];
    let bytes = call_with_buffer(256, |buf: &mut [u8], len| {
        // SAFETY: si (8), sd (2), param (2) are valid for reads; string pointers are
        // valid for their lengths (or NULL with -1); ints (10), doubles (7), lens (5)
        // and buf (buf.len()) are valid for writes; len is valid; the test index is
        // checked by `Spec::ints`.
        unsafe {
            sys::sc_geom_assert_array_report(
                si.as_ptr(),
                sd.as_ptr(),
                owner_p,
                owner_n,
                other_p,
                other_n,
                msg_p,
                msg_n,
                i32::from(set_value.is_some()),
                set_value.unwrap_or(0.0),
                i32::from(owner_param.is_some()),
                param.as_ptr(),
                ints.as_mut_ptr(),
                doubles.as_mut_ptr(),
                lens.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    let mut strings: [Option<Vec<u8>>; 5] = Default::default();
    let mut at = 0usize;
    for (slot, &n) in strings.iter_mut().zip(lens.iter()) {
        if n < 0 {
            continue;
        }
        let n = usize::try_from(n).map_err(|_| FfiError::InvalidArgument("len".into()))?;
        let s = bytes
            .get(at..at + n)
            .ok_or_else(|| FfiError::InvalidArgument("string lengths".into()))?;
        *slot = Some(s.to_vec());
        at += n;
    }
    Ok(Report {
        ints,
        doubles,
        strings,
    })
}

/// One operation on an `SmAssertArray` in [`sequence`].
#[derive(Debug, Clone, PartialEq)]
pub enum SeqOp {
    /// `AddUniqueDeleteDuplicate(new SmAssertReport(spec))`.
    Add(Spec),
    /// `SetLastAdd(flag)`.
    SetLastAdd(bool),
    /// `Contains(SmAssertReport(spec))`.
    Contains(Spec),
}

/// Result of [`sequence`].
#[derive(Debug, Clone, PartialEq)]
pub struct SeqResult {
    /// Per operation: `AddUniqueDeleteDuplicate` / `Contains` return (0/1); 0 for
    /// `SetLastAdd`.
    pub results: Vec<i64>,
    /// Per operation: `GetLastAdd()` afterwards.
    pub last_add: Vec<i64>,
    /// The final array, [`REPORT_INTS`] ints per report.
    pub elems: Vec<i64>,
}

/// Runs `ops` on one fresh `SmAssertArray` (report strings are NULL). Fails with
/// [`FfiError::InvalidArgument`] when a spec's test index is outside its label list.
pub fn sequence(ops: &[SeqOp]) -> Result<SeqResult, FfiError> {
    let n = ops.len();
    let mut kinds = Vec::with_capacity(n);
    let mut flags = Vec::with_capacity(n);
    let mut si = Vec::with_capacity(n * 8);
    let mut sd = Vec::with_capacity(n * 2);
    for op in ops {
        let (kind, flag, spec) = match op {
            SeqOp::Add(s) => (0, 0, Some(s)),
            SeqOp::SetLastAdd(f) => (1, i32::from(*f), None),
            SeqOp::Contains(s) => (2, 0, Some(s)),
        };
        kinds.push(kind);
        flags.push(flag);
        let default = Spec::default();
        let s = spec.unwrap_or(&default);
        si.extend_from_slice(&s.ints()?);
        sd.extend_from_slice(&[s.tol, s.value]);
    }
    let mut results = vec![0i64; n];
    let mut last_add = vec![0i64; n];
    let elems = call_with_buffer(n * REPORT_INTS + 1, |buf: &mut [i64], len| {
        // SAFETY: kinds/flags (n), si (8n), sd (2n) valid for reads; results and
        // last_add (n) and buf (buf.len()) valid for writes; len is valid; test
        // indices are checked by `Spec::ints`.
        unsafe {
            sys::sc_geom_assert_array_sequence(
                n as i64,
                kinds.as_ptr(),
                flags.as_ptr(),
                si.as_ptr(),
                sd.as_ptr(),
                results.as_mut_ptr(),
                last_add.as_mut_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    Ok(SeqResult {
        results,
        last_add,
        elems,
    })
}

/// Length of `sAssertDefault_list`.
const DEFAULT_LIST_LEN: u64 = 1;

/// `(SM_TYPE, list index (None = any), length)` of every label list
/// `SM_GetAssertLabelList` returns other than `sAssertDefault_list`. Lengths are the
/// symbol sizes of the arrays in `bin/cpp/lib/libSMLib.a` (`nm -S`) divided by
/// `sizeof(SmAssertReportLabel)` (32).
const LIST_LENS: &[(i64, Option<u64>, u64)] = &[
    (12082, None, 3),      // sAssertAxis2Placement_list
    (12084, Some(0), 2),   // sAssertPolarConversion_list
    (12084, Some(1), 4),   // sAssertPolarIsCurrent_list
    (14021, None, 1),      // sAssertCircle_list
    (14022, None, 3),      // sAssertEllipse_list
    (12070, None, 1),      // sAssertAttribute_list
    (14033, None, 17),     // sAssertBSplineCurve_list
    (14001, None, 1),      // sAssertCurve_list
    (12060, None, 3),      // sAssertAObject_list
    (14010, None, 4),      // sAssertLine_list
    (12150, None, 16),     // sAssertTree_list
    (12152, None, 4),      // sAssertHashTable_list
    (12153, None, 8),      // sAssertObjsInVoxels_list
    (12020, None, 3),      // sAssertSolutionArray_list
    (15002, None, 6),      // sAssertBSplineSurface_list
    (15011, None, 6),      // sAssertPlane_list
    (15012, None, 7),      // sAssertCone_list
    (15014, None, 6),      // sAssertSphere_list
    (15013, None, 4),      // sAssertCylinder_list
    (15015, None, 5),      // sAssertTorus_list
    (15020, None, 16),     // sAssertSurfOfRevolution_list
    (12088, None, 1),      // sAssertExtent1d_list
    (12090, None, 1),      // sAssertExtent2d_list
    (12092, None, 1),      // sAssertExtent3d_list
    (12096, None, 3),      // sAssertPeriodicExtent1d_list
    (12098, None, 2),      // sAssertPolarBox_list
    (12102, None, 2),      // sAssertPseudoBox_list
    (12104, None, 9),      // sAssertDerivSurfDefinition_list
    (15100, None, 6),      // sAssertSurfaceCache_list
    (15030, None, 28),     // sAssertSurfOfExtrusion_list
    (15001, None, 7),      // sAssertSurface_list
    (14048, None, 7),      // sAssertCrvOnSurf_list
    (14037, None, 6),      // sAssertProjectedCurve_list
    (16033, None, 1),      // sAssertFilletEdge_list
    (12094, None, 1),      // sAssertFilletBrep_list
    (16034, None, 3),      // sAssertFilletVertex_list
    (16032, None, 1),      // sAssertFilletVertexuse_list
    (16031, None, 1),      // sAssertFilletEdgeuse_list
    (12316, None, 7),      // sAssertBend_list
    (12318, None, 7),      // sAssertUnbend_list
    (15400, None, 3),      // sAssertOffsetSurface_list
    (14036, None, 2),      // sAssertOffsetCurve_list
    (16055, None, 1),      // sAssertAssembly_list
    (16056, None, 1),      // sAssertAssemblyInstance_list
    (16050, None, 1),      // sAssertSAGObject_list
    (16011, Some(0), 24),  // sAssertBrep_list
    (16011, Some(1), 147), // sAssertValidatePointers_list
    (16011, Some(2), 8),   // sAssertValidateTolerances_list
    (16011, Some(3), 6),   // sAssertCoincidentTopology_list
    (16011, Some(4), 8),   // sAssertSubTopology_list
    (16016, None, 18),     // sAssertEdge_list
    (16002, None, 40),     // sAssertFace_list
    (12312, None, 12),     // sAssertCurveClassification_list
    (12313, None, 1),      // sAssertCurveInterval_list
    (12308, None, 3),      // sAssertPointClassification_list
    (16001, None, 9),      // sAssertTopology_list
    (16010, None, 5),      // sAssertOwningTopology_list
    (16005, None, 28),     // sAssertEdgeuse_list
    (16022, None, 2),      // sAssertFaceuse_list
    (16006, None, 7),      // sAssertVertexuse_list
    (16021, None, 3),      // sAssertVertex_list
    (16003, None, 7),      // sAssertLoop_list
    (16004, None, 6),      // sAssertLoopuse_list
    (16012, None, 1),      // sAssertRegion_list
    (16013, None, 35),     // sAssertShell_list
    (18005, None, 9),      // sAssertPolyFace_list
    (18004, None, 2),      // sAssertCPolyFace_list
    (18006, None, 12),     // sAssertPolyLoop_list
    (18008, None, 6),      // sAssertPolyVertex_list
    (18007, None, 26),     // sAssertPolyEdge_list
    (18002, None, 3),      // sAssertPolyRegion_list
    (18003, None, 3),      // sAssertPolyShell_list
    (18001, None, 10),     // sAssertPolyBrep_list
    (12225, Some(0), 1),   // sTrimmingCheckLoopForMiniHourglass_list
    (12225, Some(1), 1),   // sTrimmingCheckTessellation_list
    (12225, Some(2), 5),   // sTrimmingCheckFace_list
    (12225, Some(3), 3),   // sTrimmingCheckLoop_list
    (14053, None, 7),      // sAssertCrvInVolume_list
    (15218, None, 6),      // sAssertSrfInVolume_list
    (22001, None, 9),      // sAssertVolume_list
    (22004, None, 2),      // sAssertBSplineVolume_list
    (22008, None, 1),      // sAssertTransform_list
    (22006, None, 7),      // sAssertBendVolume_list
    (22011, None, 6),      // sAssertTwistVolume_list
    (22010, None, 7),      // sAssertUnbendVolume_list
];

/// Number of labels in `SM_GetAssertLabelList(test_class, list_index)` (upstream has
/// no bounds; the wrappers use this to reject out-of-range test indices).
pub fn label_list_len(test_class: i64, list_index: u64) -> u64 {
    LIST_LENS
        .iter()
        .find(|(t, li, _)| *t == test_class && li.is_none_or(|li| li == list_index))
        .map_or(DEFAULT_LIST_LEN, |(_, _, n)| *n)
}
