//! Runner for unit `geom.assert_array`: assert reports, their label tables and the
//! duplicate-free report array.
//!
//! C++: `solidcore_ffi::units::geom_assert_array` (upstream `SmAssertReport`,
//! `SmAssertArray`, `SM_GetAssertLabelList`; `source/SMLib/src/SmAssertArray.cpp`).
//! Rust: `solidcore::geom::assert_array`.
//!
//! Two operations:
//! * `report`: construct one report (label lookup, optional message, tolerance
//!   conversion), optionally `SetValue` / `SetOwnerParam`, and compare every field.
//!   Strings are compared byte for byte as integer arrays (`[-1]` = `NULL`); `list_len`
//!   compares the label list length (C++: symbol size of the compiled array).
//! * `sequence`: `AddUniqueDeleteDuplicate` / `SetLastAdd` / `Contains` on one array,
//!   comparing every return value, `GetLastAdd` after each step, and the final array.
//!
//! Object pointers are integer keys (`0` = `NULL`). Every run uses at least
//! [`MIN_GENERATED`] generated inputs (more with `--seeds`); the generated inputs
//! also walk every entry of every label table.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::assert_array::{
    assert_label_list, labels, AssertArray, AssertReport, ObjectKey, ReportSpec,
};
use solidcore::geom::tol::{tol3d, USE_DEFAULT};
use solidcore::geom::vector2d::Point2;
use solidcore::math::UNDEF_DOUBLE;
use solidcore::SolidError;
use solidcore_ffi::units::geom_assert_array::{self as ffi, SeqOp, Spec};

use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `geom.assert_array` runner.
pub struct Runner;

/// Owner / other type strings selectable by index (`_T("NULL")` is what the
/// `SM_ASSERT_*` macros pass for a missing other object).
const TYPE_STRINGS: [&str; 6] = ["SmCircle", "SmBrep", "NULL", "", "SmFace", "Sm Edge use"];

/// Inputs of one report constructor call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SpecIn {
    #[serde(default)]
    reporter: i64,
    #[serde(default)]
    test_index: u64,
    #[serde(default)]
    test_class: i64,
    #[serde(default)]
    owner: i64,
    #[serde(default)]
    owner_type: i64,
    #[serde(default)]
    other: i64,
    #[serde(default)]
    other_type: i64,
    #[serde(default)]
    list_index: u64,
    #[serde(default = "undef")]
    tol: f64,
    #[serde(default = "undef")]
    value: f64,
    /// Index into [`TYPE_STRINGS`]; `None` = NULL.
    #[serde(default)]
    owner_ts: Option<usize>,
    /// Index into [`TYPE_STRINGS`]; `None` = NULL.
    #[serde(default)]
    other_ts: Option<usize>,
    /// `pOptMessage`; `None` = NULL.
    #[serde(default)]
    message: Option<String>,
}

fn undef() -> f64 {
    UNDEF_DOUBLE
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SeqIn {
    Add(SpecIn),
    SetLastAdd(bool),
    Contains(SpecIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Report {
        spec: SpecIn,
        #[serde(default)]
        set_value: Option<f64>,
        #[serde(default)]
        owner_param: Option<[f64; 2]>,
    },
    Sequence {
        ops: Vec<SeqIn>,
    },
}

fn type_string(i: Option<usize>) -> Result<Option<&'static str>, HarnessError> {
    match i {
        None => Ok(None),
        Some(i) => TYPE_STRINGS
            .get(i)
            .copied()
            .map(Some)
            .ok_or_else(|| HarnessError::new(format!("type string index {i}"))),
    }
}

fn str_ints(s: Option<&[u8]>) -> Vec<i64> {
    match s {
        None => vec![-1],
        Some(b) => b.iter().map(|&c| i64::from(c)).collect(),
    }
}

fn key(k: i64) -> Option<ObjectKey> {
    (k != 0).then_some(ObjectKey(k as u64))
}

fn key_int(k: Option<ObjectKey>) -> i64 {
    k.map_or(0, |ObjectKey(k)| k as i64)
}

fn normalise(e: &SolidError) -> String {
    match e {
        SolidError::InvalidInput { .. } => "invalid_input".into(),
        SolidError::Upstream { code, .. } => format!("status_{code}"),
        other => format!("rust_error: {other}"),
    }
}

// ---- C++ side -------------------------------------------------------------------

fn ffi_spec(s: &SpecIn) -> Spec {
    Spec {
        reporter: s.reporter,
        test_index: s.test_index,
        test_class: s.test_class,
        owner: s.owner,
        owner_type: s.owner_type,
        other: s.other,
        other_type: s.other_type,
        list_index: s.list_index,
        tol: s.tol,
        value: s.value,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let herr = |e: solidcore_ffi::FfiError| HarnessError::new(e.to_string());
    match i {
        Inputs::Report {
            spec,
            set_value,
            owner_param,
        } => {
            let r = ffi::report(
                &ffi_spec(spec),
                type_string(spec.owner_ts)?,
                type_string(spec.other_ts)?,
                spec.message.as_deref(),
                *set_value,
                *owner_param,
            )
            .map_err(herr)?;
            let [name, message, mff, owner_ts, other_ts] = &r.strings;
            Ok(Output::new()
                .int(
                    "list_len",
                    ffi::label_list_len(spec.test_class, spec.list_index) as i64,
                )
                .ints("r", r.ints.to_vec())
                .values("d", r.doubles.to_vec())
                .ints("name", str_ints(name.as_deref()))
                .ints("message", str_ints(message.as_deref()))
                .ints("message_for_file", str_ints(mff.as_deref()))
                .ints("owner_type_string", str_ints(owner_ts.as_deref()))
                .ints("other_type_string", str_ints(other_ts.as_deref())))
        }
        Inputs::Sequence { ops } => {
            let ops: Vec<SeqOp> = ops
                .iter()
                .map(|o| match o {
                    SeqIn::Add(s) => SeqOp::Add(ffi_spec(s)),
                    SeqIn::SetLastAdd(f) => SeqOp::SetLastAdd(*f),
                    SeqIn::Contains(s) => SeqOp::Contains(ffi_spec(s)),
                })
                .collect();
            let r = ffi::sequence(&ops).map_err(herr)?;
            Ok(Output::new()
                .ints("results", r.results)
                .ints("last_add", r.last_add)
                .ints("elems", r.elems))
        }
    }
}

// ---- Rust side ------------------------------------------------------------------

fn rust_spec(s: &SpecIn) -> Result<ReportSpec, HarnessError> {
    Ok(ReportSpec {
        reporter: key(s.reporter),
        test_index: s.test_index,
        test_class: s.test_class,
        owner: key(s.owner),
        owner_type: s.owner_type,
        owner_type_string: type_string(s.owner_ts)?,
        other: key(s.other),
        other_type: s.other_type,
        other_type_string: type_string(s.other_ts)?,
        // the shim converts with SmTol3d(double)
        tol3d: tol3d(s.tol),
        value: s.value,
        list_index: s.list_index,
        message: s.message.clone().map(Cow::Owned),
    })
}

fn report_ints(r: &AssertReport) -> Vec<i64> {
    vec![
        i64::from(r.ok),
        key_int(r.reporter),
        r.reporting_type,
        r.list_index as i64,
        r.test_index as i64,
        i64::from(r.assert_type.code()),
        key_int(r.owner),
        r.owner_type,
        key_int(r.other),
        r.other_type,
    ]
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Report {
            spec,
            set_value,
            owner_param,
        } => {
            let list_len = assert_label_list(spec.test_class, spec.list_index).len() as i64;
            let mut r = match AssertReport::new(rust_spec(spec)?) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(normalise(&e))))
                }
            };
            if let Some(v) = set_value {
                r.set_value(*v);
            }
            if let Some(p) = owner_param {
                r.set_owner_param(Point2::from_array(*p));
            }
            let d = vec![
                r.tol3d,
                r.value,
                r.owner_param.x,
                r.owner_param.y,
                r.other_param.x,
                r.other_param.y,
                r.dist3d,
            ];
            Ok(Output::new()
                .int("list_len", list_len)
                .ints("r", report_ints(&r))
                .values("d", d)
                .ints("name", str_ints(Some(r.name.as_bytes())))
                .ints("message", str_ints(r.message.as_deref().map(str::as_bytes)))
                .ints(
                    "message_for_file",
                    str_ints(r.message_for_file.map(str::as_bytes)),
                )
                .ints(
                    "owner_type_string",
                    str_ints(r.owner_type_string.map(str::as_bytes)),
                )
                .ints(
                    "other_type_string",
                    str_ints(r.other_type_string.map(str::as_bytes)),
                ))
        }
        Inputs::Sequence { ops } => {
            let mut arr = AssertArray::new();
            let mut results = Vec::with_capacity(ops.len());
            let mut last_add = Vec::with_capacity(ops.len());
            for op in ops {
                let res = match op {
                    SeqIn::Add(s) | SeqIn::Contains(s) => {
                        let mut spec = rust_spec(s)?;
                        // the sequence shim passes NULL strings
                        spec.owner_type_string = None;
                        spec.other_type_string = None;
                        spec.message = None;
                        let r = match AssertReport::new(spec) {
                            Ok(r) => r,
                            Err(e) => {
                                return Ok(Output::unported(&e)
                                    .unwrap_or_else(|| Output::err(normalise(&e))))
                            }
                        };
                        if matches!(op, SeqIn::Add(_)) {
                            arr.add_unique_delete_duplicate(r)
                        } else {
                            arr.contains(&r)
                        }
                    }
                    SeqIn::SetLastAdd(f) => {
                        arr.set_last_add(*f);
                        false
                    }
                };
                results.push(i64::from(res));
                last_add.push(i64::from(arr.last_add()));
            }
            let elems: Vec<i64> = arr.reports().iter().flat_map(report_ints).collect();
            Ok(Output::new()
                .ints("results", results)
                .ints("last_add", last_add)
                .ints("elems", elems))
        }
    }
}

// ---- generation -----------------------------------------------------------------

/// A `(type code, list index)`: mostly a labelled one, sometimes an unknown type or
/// an out-of-range list index (both give the default list).
fn label_key(rng: &mut Rng) -> (i64, u64) {
    let r = rng.unit();
    if r < 0.85 {
        rng.choose(labels::LABELLED).copied().unwrap_or((0, 0))
    } else if r < 0.92 {
        let ty = match rng.below(5) {
            0 => 0,
            1 => -1,
            2 => 11001, // SmObject_TYPE (no table)
            3 => rng.range_i64(10_000, 30_000),
            _ => i64::MAX,
        };
        (ty, rng.below(3))
    } else {
        let ty = *rng
            .choose(&[
                labels::BREP_TYPE,
                labels::POLAR_CONVERSION_TYPE,
                labels::TRIMMING_TOOLS_TYPE,
                labels::CIRCLE_TYPE,
            ])
            .unwrap_or(&0);
        let li = match rng.below(3) {
            0 => rng.range_i64(0, 8) as u64,
            1 => u64::from(u32::MAX),
            _ => i64::MAX as u64,
        };
        (ty, li)
    }
}

fn object_key(rng: &mut Rng, pool: i64) -> i64 {
    match rng.below(8) {
        0 => 0,
        1 => i64::MAX,
        2 => -1,
        _ => rng.range_i64(1, pool),
    }
}

fn number(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => UNDEF_DOUBLE,
        1 => USE_DEFAULT,
        2 => 0.0,
        3 => -0.0,
        4 => rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(3, 15) as i32)),
        5 => f64::from(rng.range_i64(-5, 5) as i32),
        _ => rng.uniform(-100.0, 100.0),
    }
}

fn message(rng: &mut Rng) -> Option<String> {
    match rng.below(5) {
        0 => Some(String::new()),
        1 => Some(format!("custom message {}", rng.below(100))),
        _ => None,
    }
}

fn type_string_idx(rng: &mut Rng) -> Option<usize> {
    if rng.chance(0.3) {
        None
    } else {
        Some(rng.below(TYPE_STRINGS.len() as u64) as usize)
    }
}

fn gen_spec(rng: &mut Rng, pool: i64) -> SpecIn {
    let (test_class, list_index) = label_key(rng);
    let len = assert_label_list(test_class, list_index).len() as u64;
    let owner = object_key(rng, pool);
    SpecIn {
        reporter: if rng.chance(0.6) {
            owner
        } else {
            object_key(rng, pool)
        },
        test_index: rng.below(len.max(1)),
        test_class,
        owner,
        owner_type: *rng
            .choose(&[test_class, 0, labels::FACE_TYPE, -7])
            .unwrap_or(&0),
        other: if rng.chance(0.5) {
            0
        } else {
            object_key(rng, pool)
        },
        other_type: rng.range_i64(0, 20_000),
        list_index,
        tol: number(rng),
        value: number(rng),
        owner_ts: type_string_idx(rng),
        other_ts: type_string_idx(rng),
        message: message(rng),
    }
}

/// A spec equal (by `SmAssertReport::operator==`) to `s`, or differing in one field.
fn variant(rng: &mut Rng, s: &SpecIn, pool: i64) -> SpecIn {
    let mut v = s.clone();
    match rng.below(9) {
        // fields operator== ignores
        0 => v.reporter = object_key(rng, pool),
        1 => v.tol = number(rng),
        2 => v.value = number(rng),
        3 => v.owner_type = rng.range_i64(0, 20_000),
        // fields it compares
        4 => v.owner = object_key(rng, pool),
        5 => v.other = object_key(rng, pool),
        6 => {
            let len = assert_label_list(v.test_class, v.list_index).len() as u64;
            v.test_index = rng.below(len.max(1));
        }
        7 => {
            let (c, l) = label_key(rng);
            let len = assert_label_list(c, l).len() as u64;
            v.test_class = c;
            v.list_index = l;
            v.test_index = rng.below(len.max(1));
        }
        _ => {}
    }
    v
}

fn gen_sequence(rng: &mut Rng) -> Inputs {
    let pool = rng.range_i64(2, 6);
    let base: Vec<SpecIn> = (0..rng.range_i64(1, 4))
        .map(|_| gen_spec(rng, pool))
        .collect();
    let n = rng.range_i64(1, 24);
    let mut ops = Vec::new();
    for _ in 0..n {
        let b = rng.choose(&base).cloned().unwrap_or_default();
        let s = variant(rng, &b, pool);
        ops.push(match rng.below(10) {
            0 | 1 => SeqIn::SetLastAdd(rng.chance(0.5)),
            2 | 3 => SeqIn::Contains(s),
            _ => SeqIn::Add(s),
        });
    }
    Inputs::Sequence { ops }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.35) {
        return gen_sequence(rng);
    }
    Inputs::Report {
        spec: gen_spec(rng, 1000),
        set_value: rng.chance(0.3).then(|| number(rng)),
        owner_param: rng
            .chance(0.3)
            .then(|| [rng.uniform(-10.0, 10.0), number(rng)]),
    }
}

/// One `report` case per entry of every label table, plus the default table.
fn table_cases() -> Vec<Case> {
    let mut keys: Vec<(i64, u64)> = labels::LABELLED.to_vec();
    keys.push((0, 0));
    let mut out = Vec::new();
    for (class, li) in keys {
        let len = assert_label_list(class, li).len() as u64;
        for ti in 0..len {
            let inputs = Inputs::Report {
                spec: SpecIn {
                    reporter: 1,
                    test_index: ti,
                    test_class: class,
                    list_index: li,
                    owner: 1,
                    owner_type: class,
                    other_ts: Some(2),
                    ..SpecIn::default()
                },
                set_value: None,
                owner_param: None,
            };
            if let Ok(v) = serde_json::to_value(inputs) {
                out.push(Case {
                    unit: "geom.assert_array".into(),
                    name: format!("table_{class}_{li}_{ti}"),
                    source: CaseSource::Inline,
                    inputs: v,
                });
            }
        }
    }
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.assert_array"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &ctx)?;
        v.extend(table_cases());
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
