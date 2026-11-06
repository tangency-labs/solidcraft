//! The kernel rig's corpus (`crates/solidcore/tests/kernel_cases/`, its README), replayed
//! through this crate's public operations and graded against each case's `rig` values: what
//! SolidCraft's kernel rig measured at the corpus's pin.
//!
//! Each case is one of:
//! - **pass**: every operation refused or ran as the rig's trace says, and the final bodies'
//!   count is the rig's, their face, edge and vertex counts are the records' (`outcome.raw_counts`:
//!   the rig's own merge faces across analytic surfaces), and their volume is within the case's
//!   tolerance (`expected.tolerance`, else the rig's relative 1e-3);
//! - **fail**: otherwise, with what differs;
//! - **unsupported**: the kernel alone does not account for the result (`operation_expressible`
//!   false), the rig itself rated it unsupported, or the conversion does not carry a record yet;
//! - **unported**: an operation this crate does not have yet, named, so the case is not run;
//! - **known-failing**: listed in `known_failing.json` beside the corpus, the one list, each entry
//!   with its reason and issue. A listed case whose result moves from the rig's is reported, so
//!   the list can shrink.
//!
//! The corpus's rules: `transform` is a copy, then the copy moved in place; `tagged` changes no
//! geometry and its history is not graded; picks are record indices, passed to the operations
//! with the body's record [`Order`] (the one face index convention).
//! Shell, face offset, thicken and stitch copy their inputs before editing, as SolidCraft does.
//! A copy gets its own entity IDs; an existing pick order follows the kernel's copy map.
//!
//! The test is `#[ignore]`d, out of the workspace run: run it with `--ignored`.
//!
//! Environment:
//! - `KERNEL_CASES_AREA`: comma-separated areas (a prefix each: `ladder` takes every tier) to run
//!   only those, the CI shards (#2394);
//! - `KERNEL_CASES_CASE`: run only cases whose name contains it;
//! - `KERNEL_CASES_TIMEOUT_S`: a case's time limit, 180 s by default; one past it is a hang, and
//!   the cases after it are not run (it holds the process kernel);
//! - `KERNEL_CASES_REPORT`: a file to write the per-area table and the lists to;
//! - `KERNEL_CASES_REPORT_ONLY`: set, the test reports without failing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::cast_precision_loss
)] // test

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use serde_json::Value;
use solidcore_ops::blend::{
    blend, chamfer_sides_ordered, edge_faces_along_ordered, Blend, ChamferSecond, Corners,
};
use solidcore_ops::boolean::{boolean, BooleanOp};
use solidcore_ops::stitch::stitch;
use solidcore_ops::sweep::{extrude, loft, revolve, Curve, DraftCorner};
use solidcore_ops::untrim::{untrim_face, Holes};
use solidcore_ops::{make_sphere, offset, session, Body, Order};
use solidcore_record::brep::Brep as Record;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases")
}

/// The operations this crate runs; any other is unported. A family stacked onto ops adds its
/// operations here and to [`call`].
const PORTED: &[&str] = &[
    "copy",
    "tagged",
    "transform",
    "make_sphere",
    "shell",
    "offset_faces",
    "thicken",
    "untrim_face",
    "boolean",
    "extrude",
    "revolve",
    "loft",
    "blend_dropping",
    "chamfer_sides",
    "stitch",
    "split_body_by_sheet",
];

/// The relative tolerance the rig grades volume and area with when a case states none.
const DEFAULT_REL: f64 = 1e-3;

/// The volume's relative accuracy asked of the kernel.
const VOLUME_ACCURACY: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Pass,
    Fail,
    Unsupported,
    Unported,
    KnownFailing,
}

/// One case's grade, and what to say about it.
#[derive(Debug)]
struct Graded {
    area: String,
    case: String,
    kind: Kind,
    note: String,
    /// The operations not ported, for an unported case (a listed one included).
    unported: Vec<String>,
    /// The largest relative volume difference from the rig, for a run case.
    deviation: Option<f64>,
    /// For a listed case: whether its result moved from the rig's.
    moved: bool,
    /// Whether it ran past its time limit.
    hang: bool,
}

/// An entry of the known-failing list.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Listed {
    case: String,
    issue: String,
    reason: String,
}

fn known_failing() -> BTreeMap<String, Listed> {
    let text = std::fs::read_to_string(corpus().join("known_failing.json")).unwrap();
    let list: Vec<Listed> = serde_json::from_str(&text).unwrap();
    let mut out = BTreeMap::new();
    for l in list {
        assert!(!l.reason.is_empty() && !l.issue.is_empty(), "{}", l.case);
        let case = l.case.clone();
        assert!(out.insert(case.clone(), l).is_none(), "{case} listed twice");
    }
    out
}

fn entries(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            entries(&p, out);
        } else if p.extension().is_some_and(|x| x == "json") && p.parent() != Some(&corpus()) {
            out.push(p);
        }
    }
}

fn records(entry: &Path, e: &Value) -> serde_json::Map<String, Value> {
    let Some(name) = e["records"].as_str() else {
        return serde_json::Map::new();
    };
    let gz = std::fs::File::open(entry.parent().unwrap().join(name)).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// A body of the run, and the record order its picks are taken through, once known.
#[derive(Clone)]
struct Slot {
    body: Arc<Body>,
    order: Option<Order>,
}

impl Slot {
    fn copied(&self) -> Result<Result<Self, String>, Stop> {
        let copied = match &self.order {
            Some(order) => self
                .body
                .copy_with_order(order)
                .map(|(body, order)| (body, Some(order))),
            None => self.body.copy().map(|body| (body, None)),
        };
        match copied {
            Ok((body, order)) => Ok(Ok(Self {
                body: Arc::new(body),
                order,
            })),
            // Bad replay metadata is not a kernel refusal and must never satisfy one.
            Err(solidcore_ops::Error::Record(why)) => {
                Err(Stop::Fail(format!("copied input record order: {why}")))
            }
            Err(e) => Ok(Err(e.to_string())),
        }
    }
}

/// Why a case stopped before grading.
#[derive(Debug)]
enum Stop {
    Unsupported(String),
    Fail(String),
}

fn f64s<const N: usize>(v: &Value) -> Option<[f64; N]> {
    let a = v.as_array()?;
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64()?;
    }
    (a.len() == N).then_some(out)
}

/// A pick's record indices.
fn picked(p: &Value) -> Result<Vec<usize>, Stop> {
    let mut out = Vec::new();
    for x in p["picked"].as_array().into_iter().flatten() {
        match x["index"].as_u64() {
            Some(i) => out.push(usize::try_from(i).unwrap()),
            None => return Err(Stop::Unsupported(format!("a pick missed: {x}"))),
        }
    }
    Ok(out)
}

/// The picked faces' record indices, and the record order they are taken through: an operation
/// takes record indices with the body's [`Order`].
fn faces(slot: &mut Slot, pick: &Value) -> Result<(Vec<usize>, Order), Stop> {
    let record = picked(pick)?;
    if slot.order.is_none() {
        // a body an operation made: its record order is the one its record is written in
        let (_, order) = slot
            .body
            .to_record()
            .map_err(|e| Stop::Unsupported(format!("to_record for a pick: {e}")))?;
        slot.order = Some(order);
    }
    let order = slot
        .order
        .clone()
        .ok_or_else(|| Stop::Fail("no record order".to_owned()))?;
    Ok((record, order))
}

/// A traced profile: its curves as the operation takes them (`line`, `arc`, `nurbs`).
fn profile(v: &Value) -> Option<Vec<Curve>> {
    let mut out = Vec::new();
    for seg in v.as_array()? {
        out.push(if let Some(l) = seg.get("line") {
            Curve::Line {
                start: f64s::<3>(&l["start"])?,
                end: f64s::<3>(&l["end"])?,
            }
        } else if let Some(a) = seg.get("arc") {
            Curve::Arc {
                center: f64s::<3>(&a["center"])?,
                axis: f64s::<3>(&a["axis"])?,
                reference: f64s::<3>(&a["reference"])?,
                radius: a["radius"].as_f64()?,
                start: a["start"].as_f64()?,
                end: a["end"].as_f64()?,
            }
        } else {
            let n = seg.get("nurbs")?;
            Curve::Nurbs {
                degree: usize::try_from(n["degree"].as_u64()?).ok()?,
                points: n["points"]
                    .as_array()?
                    .iter()
                    .map(f64s::<4>)
                    .collect::<Option<Vec<_>>>()?,
                knots: n["knots"]
                    .as_array()?
                    .iter()
                    .map(Value::as_f64)
                    .collect::<Option<Vec<_>>>()?,
            }
        });
    }
    Some(out)
}

/// A body an operation made: no record order yet (it is taken when a later pick needs it).
fn made(b: solidcore_ops::Result<Body>) -> Result<Slot, String> {
    b.map(|body| Slot {
        body: Arc::new(body),
        order: None,
    })
    .map_err(|e| e.to_string())
}

/// Runs a traced call in output order, keeping a split's two bodies distinct.
fn call_outputs(c: &Value, ins: &mut [Slot]) -> Result<Result<Vec<Slot>, String>, Stop> {
    if c["op"].as_str() != Some("split_body_by_sheet") {
        return call(c, ins).map(|r| r.map(|out| vec![out]));
    }
    let bad = |what: &str| Stop::Fail(format!("split_body_by_sheet: {what}"));
    if ins.len() != 2 {
        return Err(bad("requires a body and a sheet"));
    }
    let face = c["params"]["face"]
        .as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .ok_or_else(|| bad("parameter face"))?;
    let (body, sheet) = ins.split_at_mut(1);
    let sheet = &mut sheet[0];
    if sheet.order.is_none() {
        let (_, order) = sheet
            .body
            .to_record()
            .map_err(|e| bad(&format!("sheet record order: {e}")))?;
        sheet.order = Some(order);
    }
    let order = sheet.order.as_ref().unwrap();
    if order.faces.get(face).is_none() {
        return Err(bad("face outside sheet record order"));
    }
    Ok(
        match solidcore_ops::split::split_body_by_sheet(&body[0].body, &sheet.body, order, face) {
            Ok(Some((back, front))) => Ok([back, front]
                .into_iter()
                .map(|body| Slot {
                    body: Arc::new(body),
                    order: None,
                })
                .collect()),
            Ok(None) => Err("sheet does not divide the body".to_owned()),
            Err(e) => Err(e.to_string()),
        },
    )
}

/// Runs one traced call: its output, or the operation's error as text.
fn call(c: &Value, ins: &mut [Slot]) -> Result<Result<Slot, String>, Stop> {
    call_with_copy(c, ins, Slot::copied)
}

/// Dispatches a single-output call with an injectable preparation copy; replay failures
/// remain separate from a traced operation refusal.
fn call_with_copy(
    c: &Value,
    ins: &mut [Slot],
    copy: impl Fn(&Slot) -> Result<Result<Slot, String>, Stop>,
) -> Result<Result<Slot, String>, Stop> {
    let p = &c["params"];
    let bad = |what: &str| Stop::Fail(format!("{}: parameter {what}", c["op"]));
    let op = c["op"].as_str().unwrap();
    let first = || ins.first().cloned().ok_or_else(|| bad("input"));
    // An internal preparation failure is not a refusal of the traced edit.
    let copy_for_edit =
        |s: &Slot| copy(s)?.map_err(|why| Stop::Fail(format!("copy before {op}: {why}")));
    let in_place =
        |slot: Slot, r: solidcore_ops::Result<()>| r.map(|()| slot).map_err(|e| e.to_string());
    Ok(match op {
        "copy" => return copy(&first()?),
        "tagged" => Ok(first()?),
        "transform" => {
            let s = first()?;
            let m = p["matrix"].as_array().ok_or_else(|| bad("matrix"))?;
            let mut matrix = [[0.0; 4]; 4];
            for (row, v) in matrix.iter_mut().zip(m) {
                *row = f64s::<4>(v).ok_or_else(|| bad("matrix"))?;
            }
            let out = copy_for_edit(&s)?;
            let r = out.body.transform(&matrix);
            in_place(out, r)
        }
        "make_sphere" => {
            let center = f64s::<3>(&p["center"]).ok_or_else(|| bad("center"))?;
            let radius = p["radius"].as_f64().ok_or_else(|| bad("radius"))?;
            make_sphere(center, radius)
                .map(|b| Slot {
                    body: Arc::new(b),
                    order: None,
                })
                .map_err(|e| e.to_string())
        }
        "shell" => {
            let mut s = copy_for_edit(&first()?)?;
            let (open, order) = faces(&mut s, &p["open_faces"])?;
            let t = p["thickness"].as_f64().ok_or_else(|| bad("thickness"))?;
            let r = offset::shell(&s.body, &order, &open, t);
            in_place(Slot { order: None, ..s }, r)
        }
        "offset_faces" => {
            let mut s = copy_for_edit(&first()?)?;
            let (picked, order) = faces(&mut s, &p["faces"])?;
            let d = p["distance"].as_f64().ok_or_else(|| bad("distance"))?;
            let r = offset::offset_faces(&s.body, &order, &picked, d);
            in_place(Slot { order: None, ..s }, r)
        }
        "thicken" => {
            let s = copy_for_edit(&first()?)?;
            let t = p["thickness"].as_f64().ok_or_else(|| bad("thickness"))?;
            let sym = p["symmetric"].as_bool().ok_or_else(|| bad("symmetric"))?;
            let r = offset::thicken(&s.body, t, sym);
            in_place(Slot { order: None, ..s }, r)
        }
        "untrim_face" => {
            let holes = p.get("holes").ok_or_else(|| bad("holes"))?;
            let outer = p["outer"].as_bool().ok_or_else(|| bad("outer"))?;
            let in_place = p["in_place"].as_bool().ok_or_else(|| bad("in_place"))?;
            let pick = p.get("face").ok_or_else(|| bad("face"))?;
            let mut s = first()?;
            let (picked, order) = faces(&mut s, pick)?;
            let [face] = picked.as_slice() else {
                return Err(bad("one face"));
            };
            if order.faces.get(*face).is_none() {
                return Err(bad("face index"));
            }
            if !holes.is_null() {
                return Err(Stop::Unsupported(
                    "untrim_face hole point picks have no recorded edge indices".to_owned(),
                ));
            }
            // The public untrim operation copies its operand and maps the recorded IDs itself.
            made(untrim_face(
                &s.body,
                &order,
                *face,
                Holes::All,
                outer,
                in_place,
            ))
        }
        "boolean" => {
            let operation = match p["op"].as_str() {
                Some("union") => BooleanOp::Union,
                Some("subtract") => BooleanOp::Subtract,
                Some("intersect") => BooleanOp::Intersect,
                _ => return Err(bad("op")),
            };
            let [a, b] = ins else {
                return Err(bad("two inputs"));
            };
            let owned = |s: &Slot| {
                s.body
                    .copy()
                    .map_err(|e| Stop::Fail(format!("copy before boolean: {e}")))
            };
            match boolean(owned(a)?, owned(b)?, operation) {
                Ok(Some(body)) => Ok(Slot {
                    body: Arc::new(body),
                    order: None,
                }),
                Ok(None) => Err("the boolean left nothing".to_owned()),
                Err(e) => Err(e.to_string()),
            }
        }
        "loft" => {
            let sections = p["sections"]
                .as_array()
                .ok_or_else(|| bad("sections"))?
                .iter()
                .map(profile)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| bad("sections"))?;
            let degree = p["degree"]
                .as_u64()
                .and_then(|d| u32::try_from(d).ok())
                .ok_or_else(|| bad("degree"))?;
            let cap = p["cap"].as_bool().ok_or_else(|| bad("cap"))?;
            made(loft(&sections, degree, cap))
        }
        "extrude" => {
            let curves = profile(&p["profile"]).ok_or_else(|| bad("profile"))?;
            let corner = match p["corner"].as_str() {
                Some("curvature") | None => DraftCorner::Curvature,
                Some("tangent") => DraftCorner::Tangent,
                Some("round") => DraftCorner::Round,
                Some(_) => return Err(bad("corner")),
            };
            made(extrude(
                &curves,
                f64s::<3>(&p["direction"]).ok_or_else(|| bad("direction"))?,
                p["distance"].as_f64().ok_or_else(|| bad("distance"))?,
                p["draft"].as_f64().unwrap_or(0.0),
                corner,
                p["cap"].as_bool().ok_or_else(|| bad("cap"))?,
            ))
        }
        "revolve" => {
            let curves = profile(&p["profile"]).ok_or_else(|| bad("profile"))?;
            made(revolve(
                &curves,
                f64s::<3>(&p["origin"]).ok_or_else(|| bad("origin"))?,
                f64s::<3>(&p["direction"]).ok_or_else(|| bad("direction"))?,
                p["angle"].as_f64().ok_or_else(|| bad("angle"))?,
                p["cap"].as_bool().ok_or_else(|| bad("cap"))?,
            ))
        }
        "chamfer_sides" => {
            let distance = p["distance"].as_f64().ok_or_else(|| bad("distance"))?;
            let direction = f64s::<3>(&p["first"]).ok_or_else(|| bad("first"))?;
            let second = match (
                p["second"]["distance"].as_f64(),
                p["second"]["angle"].as_f64(),
            ) {
                (Some(d), None) => ChamferSecond::Distance(d),
                (None, Some(a)) => ChamferSecond::Angle(a),
                _ => return Err(bad("second")),
            };
            let mut s = copy_for_edit(&first()?)?;
            let (edges, order) = faces(&mut s, &p["edges"])?;
            let mut along = Vec::new();
            for &edge in &edges {
                let pair = edge_faces_along_ordered(&s.body, &order, edge, direction)
                    .map_err(|e| Stop::Fail(format!("chamfer face query: {e}")))?;
                let [(a, da), (b, db)] = pair;
                along.push(if da >= db { a } else { b });
            }
            let r = chamfer_sides_ordered(&s.body, &order, &edges, &along, distance, second);
            in_place(Slot { order: None, ..s }, r)
        }
        "blend_dropping" => {
            if p.get("turn").is_some() {
                return Err(Stop::Unsupported(
                    "blend_dropping with a corner turn: the ops blend takes none".to_owned(),
                ));
            }
            let kind = match p["kind"].as_str() {
                Some("round") => Blend::Round,
                Some("curvature") => Blend::Curvature,
                Some("chord") => Blend::Chord,
                Some("chamfer") => Blend::Chamfer,
                _ => return Err(bad("kind")),
            };
            let sizes: Vec<f64> = p["sizes"]
                .as_array()
                .ok_or_else(|| bad("sizes"))?
                .iter()
                .map(Value::as_f64)
                .collect::<Option<_>>()
                .ok_or_else(|| bad("sizes"))?;
            let mut s = copy_for_edit(&first()?)?;
            let (edges, _) = faces(&mut s, &p["edges"])?;
            let one_size = sizes.len() == 1;
            if !(one_size || sizes.len() == edges.len()) {
                return Err(bad("sizes"));
            }
            // SolidCraft validates every supplied size before its picker retains the first
            // point for each edge. A bad later size must not disappear with its duplicate.
            if sizes.iter().any(|&size| {
                !size.is_finite() || size <= 0.0 || size > solidcore_ops::blend::MAX_SIZE
            }) {
                return Ok(Err(solidcore_ops::Error::Argument.to_string()));
            }
            let mut unique = Vec::new();
            let mut each = Vec::new();
            for (i, edge) in edges.into_iter().enumerate() {
                if !unique.contains(&edge) {
                    unique.push(edge);
                    each.push(
                        *sizes
                            .get(if one_size { 0 } else { i })
                            .ok_or_else(|| bad("sizes"))?,
                    );
                }
            }
            let r = blend(&s.body, kind, &unique, &each, &[], Corners::Filled).map(|_| ());
            in_place(Slot { order: None, ..s }, r)
        }
        "stitch" => {
            let s = copy_for_edit(&first()?)?;
            let t = p["tolerance"].as_f64().ok_or_else(|| bad("tolerance"))?;
            let body = Arc::try_unwrap(s.body)
                .map_err(|_| Stop::Fail("copy before stitch: the copy is shared".to_owned()))?;
            // whether the result closed into a solid is not traced: the body is the output
            made(stitch(body, t).map(|(b, _closed)| b))
        }
        other => {
            return Err(Stop::Fail(format!(
                "{other} is listed as ported but not run"
            )))
        }
    })
}

/// What the case's final bodies measure.
#[derive(Debug, Default)]
struct Measured {
    bodies: usize,
    counts: [usize; 3],
    volume: Option<f64>,
}

fn measure(result: &[Slot]) -> Result<Measured, String> {
    let mut m = Measured {
        bodies: result.len(),
        ..Measured::default()
    };
    let mut volume = Some(0.0);
    for s in result {
        // Counted as the record writes the body, the rig's convention (`outcome.raw_counts`): a
        // line where the body touches itself is one edge in the kernel's list and one per pair
        // of faces in the record.
        let (record, _) = s.body.to_record().map_err(|e| format!("record: {e}"))?;
        let c = [
            record.faces.len(),
            record.edges.len(),
            record.vertices.len(),
        ];
        for (a, b) in m.counts.iter_mut().zip(c) {
            *a += b;
        }
        volume = match (volume, s.body.volume(VOLUME_ACCURACY)) {
            (Some(v), Ok(x)) => Some(v + x),
            _ => None,
        };
    }
    m.volume = volume;
    Ok(m)
}

/// The case's tolerance on a volume: relative, or absolute.
fn within(got: f64, want: f64, tol: &Value) -> bool {
    let d = (got - want).abs();
    if let Some(a) = tol["abs_mm"].as_f64() {
        return d <= a;
    }
    d <= tol["rel"].as_f64().unwrap_or(DEFAULT_REL) * want.abs()
}

/// Runs a case's calls and grades the result against `rig` (or, for a hand-written case
/// without one, `expected`): `Ok(deviation)` for a match.
fn run_case(entry: &Path, e: &Value) -> Result<Option<f64>, Stop> {
    run_case_with_records(e, || records(entry, e))
}

/// Loads named operand records, binds each call's outputs in order, and grades the selected
/// final bodies; the record supplier also permits closed-form tests without corpus files.
fn run_case_with_records(
    e: &Value,
    records: impl FnOnce() -> serde_json::Map<String, Value>,
) -> Result<Option<f64>, Stop> {
    let calls = e["calls"].as_array().unwrap();
    // Check the entire trace before a terminal refusal or an unsupported import can
    // stop replay. A later malformed marker must not turn into an unchecked pass.
    let mut terminal = false;
    let continued_flags = calls
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let op = c["op"].as_str().unwrap();
            let continued = match c.get("continued") {
                Some(v) => {
                    let continued = v.as_bool().ok_or_else(|| {
                        Stop::Fail(format!("call {i} ({op}): continued must be a boolean"))
                    })?;
                    if !c["error"].is_string() {
                        return Err(Stop::Fail(format!(
                            "call {i} ({op}): continued requires an expected error"
                        )));
                    }
                    if terminal {
                        return Err(Stop::Fail(format!(
                            "call {i} ({op}): continued follows a terminal refusal"
                        )));
                    }
                    continued
                }
                None => false,
            };
            terminal |= c["error"].is_string() && !continued;
            Ok(continued)
        })
        .collect::<Result<Vec<_>, Stop>>()?;
    let recs = records();
    let mut bodies: HashMap<String, Slot> = HashMap::new();
    for (name, how) in e["bodies"].as_object().into_iter().flatten() {
        let record: Record = serde_json::from_value(recs[name].clone())
            .map_err(|x| Stop::Fail(format!("{name}: record: {x}")))?;
        let trusted = how["trusted"].as_bool().unwrap_or(true);
        match Body::from_record(&record, trusted) {
            Ok((b, order)) => {
                bodies.insert(
                    name.clone(),
                    Slot {
                        body: Arc::new(b),
                        order: Some(order),
                    },
                );
            }
            Err(solidcore_ops::Error::Unsupported(w)) => {
                return Err(Stop::Unsupported(format!("{name}: the conversion: {w}")))
            }
            Err(x) => return Err(Stop::Fail(format!("{name}: from_record: {x}"))),
        }
    }
    for (i, (c, continued)) in calls.iter().zip(continued_flags).enumerate() {
        let op = c["op"].as_str().unwrap();
        let mut ins = Vec::new();
        for n in c["inputs"].as_array().unwrap() {
            let n = n.as_str().unwrap();
            ins.push(
                bodies
                    .get(n)
                    .cloned()
                    .ok_or_else(|| Stop::Fail(format!("call {i} ({op}): no body {n}")))?,
            );
        }
        let traced_error = c.get("error").and_then(Value::as_str);
        match (call_outputs(c, &mut ins)?, traced_error) {
            (Ok(out), None) => {
                let names = c["outputs"].as_array().ok_or_else(|| {
                    Stop::Fail(format!("call {i} ({op}): outputs must be an array"))
                })?;
                if names.len() != out.len() {
                    return Err(Stop::Fail(format!(
                        "call {i} ({op}): {} output names for {} bodies",
                        names.len(),
                        out.len()
                    )));
                }
                for (n, body) in names.iter().zip(out) {
                    let name = n.as_str().ok_or_else(|| {
                        Stop::Fail(format!("call {i} ({op}): output name must be a string"))
                    })?;
                    bodies.insert(name.to_owned(), body);
                }
            }
            // A handled refusal is an attempt, not the case's outcome. Keep checking the
            // later operations and final geometry; an unmarked refusal still ends the case.
            (Err(_), Some(_)) if continued => {}
            (Err(_), Some(_)) => return Ok(None),
            (Err(x), None) => return Err(Stop::Fail(format!("call {i} ({op}) refused: {x}"))),
            (Ok(_), Some(want)) => {
                return Err(Stop::Fail(format!(
                    "call {i} ({op}) ran where the rig's refused: {want}"
                )))
            }
        }
    }
    let mut result = Vec::new();
    for r in e["result"].as_array().unwrap() {
        let n = r
            .get("made")
            .or_else(|| r.get("operand"))
            .and_then(Value::as_str)
            .unwrap();
        result.push(
            bodies
                .get(n)
                .cloned()
                .ok_or_else(|| Stop::Fail(format!("no final body {n}")))?,
        );
    }
    let got = measure(&result).map_err(Stop::Fail)?;
    let tol = &e["expected"]["tolerance"];
    let want = match e.get("rig") {
        Some(r) if r["measured"].is_object() => &r["measured"],
        Some(r) if !r["status"].is_null() => {
            return Err(Stop::Fail(format!(
                "ran where the rig stopped: {}",
                r["error"]
            )))
        }
        // a hand-written case: its expected values
        _ => &e["expected"],
    };
    let mut diffs = Vec::new();
    if let Some(n) = want["body_count"].as_u64() {
        if n != got.bodies as u64 {
            diffs.push(format!("bodies {} (rig {n})", got.bodies));
        }
    }
    // the rig's own counts merge faces across analytic surfaces; the records' are the kernel's
    let counts = match &e["outcome"]["raw_counts"] {
        r if r.is_object() => r,
        _ => want,
    };
    for (k, g) in ["faces", "edges", "vertices"].iter().zip(got.counts) {
        if let Some(n) = counts[*k].as_u64() {
            if n != g as u64 {
                diffs.push(format!("{k} {g} (rig {n})"));
            }
        }
    }
    let mut deviation = None;
    if let Some(v) = want["volume_mm3"].as_f64() {
        match got.volume {
            Some(g) => {
                if v != 0.0 {
                    deviation = Some((g - v).abs() / v.abs());
                }
                if !within(g, v, tol) {
                    diffs.push(format!("volume {g} (rig {v})"));
                }
            }
            None => diffs.push(format!("volume not measured (rig {v})")),
        }
    }
    if diffs.is_empty() {
        Ok(deviation)
    } else {
        Err(Stop::Fail(diffs.join(", ")))
    }
}

#[test]
fn chamfer_dispatch_preserves_order_direction_sizes_and_operand() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    for (direction, second, x, z) in [
        (1.0, serde_json::json!({"distance": 3.0}), 15.0, 17.0),
        (-1.0, serde_json::json!({"distance": 3.0}), 17.0, 15.0),
        (
            1.0,
            serde_json::json!({"angle": std::f64::consts::FRAC_PI_6}),
            15.0,
            17.113_248_654_051_87, // 20 - 5 / sqrt(3), the 30-degree cut.
        ),
    ] {
        let body = Arc::new(solidcore_ops::make_box([0.0; 3], [20.0; 3]).unwrap());
        let (record, mut order) = body.to_record().unwrap();
        let edge = record
            .edges
            .iter()
            .position(|e| {
                e.vertices.iter().all(|&i| {
                    let p = record.vertices[i as usize].point;
                    p[0] == 20.0 && p[2] == 20.0
                })
            })
            .unwrap();
        let id = order.edges[edge];
        order.edges.rotate_left(3);
        order.faces.rotate_left(2);
        let pick = order.edges.iter().position(|&e| e == id).unwrap();
        let input = Slot {
            body: body.clone(),
            order: Some(order),
        };
        let out = call(
            &serde_json::json!({"op": "chamfer_sides", "params": {
                "edges": {"picked": [{"index": pick}]}, "first": [0, 0, direction],
                "distance": 5.0, "second": second
            }}),
            &mut [input],
        )
        .unwrap()
        .unwrap();
        assert!(out.order.is_none());
        let (made, made_order) = out.body.to_record().unwrap();
        let (_, mapped) = out.body.copy_with_order(&made_order).unwrap();
        assert!(
            made_order
                .edges
                .iter()
                .zip(&mapped.edges)
                .any(|(a, b)| a != b),
            "the measurement-copy check must exercise renumbered entities"
        );
        for edge in 0..made.edges.len() {
            let pair =
                edge_faces_along_ordered(&out.body, &made_order, edge, [0.0, 0.0, 1.0]).unwrap();
            for (face, lean) in pair {
                assert!(made.edges[edge]
                    .uses
                    .iter()
                    .any(|u| u.face as usize == face));
                let normal = out
                    .body
                    .face_normal_at_edge(&made_order, edge, face)
                    .unwrap();
                assert!((lean - normal.value[2]).abs() < 1e-12);
            }
        }
        for target in [[x, 0.0, 20.0], [20.0, 0.0, z]] {
            assert!(
                made.vertices.iter().any(|v| v
                    .point
                    .iter()
                    .zip(target)
                    .all(|(a, b)| (a - b).abs() < 1e-7)),
                "missing {target:?}"
            );
        }
        let removed = 0.5 * (20.0 - x) * (20.0 - z) * 20.0;
        assert!((out.body.volume(1e-9).unwrap() - (8000.0 - removed)).abs() < 1e-6);
        assert!((body.volume(1e-9).unwrap() - 8000.0).abs() < 1e-6);
    }
}

#[test]
fn repeated_blend_picks_keep_first_size_and_source() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let record = solidcore_ops::make_box([0.0; 3], [20.0; 3])
        .unwrap()
        .to_record()
        .unwrap()
        .0;
    let edge = record
        .edges
        .iter()
        .position(|e| {
            e.vertices.iter().all(|&i| {
                let p = record.vertices[i as usize].point;
                p[0] == 20.0 && p[2] == 20.0
            })
        })
        .unwrap();
    for sizes in [serde_json::json!([2.0, 7.0]), serde_json::json!([2.0])] {
        check_discarded_edit(
            record.clone(),
            "blend_dropping",
            &serde_json::json!({
                "kind": "chamfer", "edges": {"picked": [{"index": edge}, {"index": edge}]},
                "sizes": sizes
            }),
            &serde_json::json!({"body_count": 1, "volume_mm3": 7960.0}),
            serde_json::json!({"body_count": 1, "volume_mm3": 8000.0}),
        );
    }
}

#[test]
fn repeated_blend_picks_do_not_hide_invalid_later_sizes() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let record = solidcore_ops::make_box([0.0; 3], [20.0; 3])
        .unwrap()
        .to_record()
        .unwrap()
        .0;
    for sizes in [
        serde_json::json!([2.0, -7.0]),
        serde_json::json!([2.0, 1_000_001.0]),
    ] {
        let mut records = serde_json::Map::new();
        records.insert("input".to_owned(), serde_json::to_value(&record).unwrap());
        let e = serde_json::json!({"bodies": {"input": {"trusted": true}},
            "calls": [{"op": "blend_dropping", "inputs": ["input"], "error": "invalid size",
                "params": {"kind": "chamfer", "edges": {"picked": [{"index": 0}, {"index": 0}]}, "sizes": sizes}}]});
        assert!(run_case_with_records(&e, || records).unwrap().is_none());
    }
}

fn handled_refusal_case() -> Value {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    serde_json::json!({
        "bodies": {},
        "calls": [
            {"op": "make_sphere", "inputs": [],
             "params": {"center": [0, 0, 0], "radius": -1},
             "error": "invalid radius", "continued": true},
            {"op": "make_sphere", "inputs": [],
             "params": {"center": [0, 0, 0], "radius": 1}, "outputs": ["b1"]}
        ],
        "result": [{"made": "b1"}],
        "expected": {"body_count": 1, "volume_mm3": 4.0 * std::f64::consts::PI / 3.0,
                     "tolerance": {"rel": 1e-9}}
    })
}

#[test]
fn a_handled_refusal_still_grades_the_final_geometry() {
    let e = handled_refusal_case();
    let deviation = run_case(Path::new("unused.json"), &e).unwrap();
    assert!(
        deviation.is_some(),
        "the final sphere's volume must be measured"
    );
    let mut wrong = e;
    wrong["expected"]["body_count"] = serde_json::json!(2);
    assert!(
        matches!(run_case(Path::new("unused.json"), &wrong), Err(Stop::Fail(why)) if why.contains("bodies 1"))
    );
}

#[test]
fn a_handled_refusal_does_not_hide_a_later_unexpected_refusal() {
    let mut e = handled_refusal_case();
    e["calls"][1]["params"]["radius"] = serde_json::json!(-2);
    assert!(
        matches!(run_case(Path::new("unused.json"), &e), Err(Stop::Fail(why)) if why.contains("call 1 (make_sphere) refused"))
    );
}

#[test]
fn a_handled_refusal_must_actually_refuse() {
    let mut e = handled_refusal_case();
    e["calls"][0]["params"]["radius"] = serde_json::json!(1);
    assert!(
        matches!(run_case(Path::new("unused.json"), &e), Err(Stop::Fail(why)) if why.contains("ran where the rig's refused"))
    );
}

#[test]
fn an_unmarked_refusal_keeps_the_existing_terminal_behavior() {
    let mut e = handled_refusal_case();
    e["calls"][0].as_object_mut().unwrap().remove("continued");
    assert_eq!(run_case(Path::new("unused.json"), &e).unwrap(), None);
}

#[test]
fn malformed_continuation_markers_fail_before_replaying_the_call() {
    for marker in [serde_json::json!("true"), Value::Null] {
        let mut e = handled_refusal_case();
        e["calls"][0]["continued"] = marker;
        assert!(
            matches!(run_case(Path::new("unused.json"), &e), Err(Stop::Fail(why)) if why.contains("continued must be a boolean"))
        );
    }
    let mut successful_call = handled_refusal_case();
    successful_call["calls"][0]
        .as_object_mut()
        .unwrap()
        .remove("error");
    assert!(
        matches!(run_case(Path::new("unused.json"), &successful_call), Err(Stop::Fail(why)) if why.contains("continued requires"))
    );
}

#[test]
fn a_final_handled_refusal_still_grades_the_retained_body() {
    let mut e = handled_refusal_case();
    e["calls"].as_array_mut().unwrap().swap(0, 1);
    e["expected"]["body_count"] = serde_json::json!(2);
    assert!(
        matches!(run_case(Path::new("unused.json"), &e), Err(Stop::Fail(why)) if why.contains("bodies 1"))
    );
}

#[test]
fn a_terminal_refusal_cannot_hide_later_invalid_markers() {
    for marker in [
        serde_json::json!("true"),
        serde_json::json!(true),
        serde_json::json!(false),
    ] {
        let mut e = handled_refusal_case();
        e["calls"][0].as_object_mut().unwrap().remove("continued");
        e["calls"][1]["error"] = serde_json::json!("refused");
        e["calls"][1]["continued"] = marker;
        assert!(
            matches!(run_case(Path::new("unused.json"), &e), Err(Stop::Fail(why)) if why.contains("continued"))
        );
    }
}

/// The final bodies are counted as their records write them, the convention of the rig's
/// `raw_counts` (#2817). Here the hole is tangent to the front face, so the body touches itself
/// along a line: the kernel keeps that line as one edge with four uses, the record writes it once
/// per pair of faces (its Touching rule), 21 edges where the kernel's list has 20.
#[test]
fn a_body_touching_itself_along_a_line_is_counted_as_its_record() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join("ladder/02-booleans/018-hole-tangent-to-side-face.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    assert_eq!(e["outcome"]["raw_counts"]["edges"], 21);
    run_case(&entry, &e).unwrap();
}

fn check_discarded_edit(record: Record, op: &str, params: &Value, made: &Value, retained: Value) {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let mut records = serde_json::Map::new();
    records.insert("input".to_owned(), serde_json::to_value(record).unwrap());
    let mut e = serde_json::json!({
        "bodies": {"input": {"trusted": true}},
        "calls": [{"op": op, "inputs": ["input"], "params": params,
                   "outputs": ["discarded"]}],
        "result": [{"made": "discarded"}],
        "expected": made
    });
    e["expected"]["tolerance"] = serde_json::json!({"rel": 1e-9});
    // First establish that the operation really builds its different, closed-form result.
    assert!(run_case_with_records(&e, || records.clone())
        .unwrap()
        .is_some());
    // The application can discard that result and keep its original operand instead.
    e["result"] = serde_json::json!([{"operand": "input"}]);
    e["expected"] = retained;
    e["expected"]["tolerance"] = serde_json::json!({"rel": 1e-9});
    run_case_with_records(&e, || records.clone()).unwrap();
}

#[test]
fn a_discarded_shell_preserves_the_retained_box() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let record = solidcore_ops::make_box([0.0; 3], [30.0, 20.0, 10.0])
        .unwrap()
        .to_record()
        .unwrap()
        .0;
    check_discarded_edit(
        record,
        "shell",
        &serde_json::json!({"open_faces": {"picked": []}, "thickness": -2.0}),
        &serde_json::json!({"body_count": 1, "volume_mm3": 6000.0 - 26.0 * 16.0 * 6.0}),
        serde_json::json!({"body_count": 1, "volume_mm3": 6000.0}),
    );
}

#[test]
fn a_discarded_face_offset_preserves_the_retained_box() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let record = solidcore_ops::make_box([0.0; 3], [30.0, 20.0, 10.0])
        .unwrap()
        .to_record()
        .unwrap()
        .0;
    let top = record
        .faces
        .iter()
        .position(|f| matches!(f.surface, solidcore_record::brep::Surface::Plane { origin, .. } if origin[2] == 10.0))
        .unwrap();
    check_discarded_edit(
        record,
        "offset_faces",
        &serde_json::json!({"faces": {"picked": [{"index": top}]}, "distance": 2.0}),
        &serde_json::json!({"body_count": 1, "volume_mm3": 30.0 * 20.0 * 12.0}),
        serde_json::json!({"body_count": 1, "volume_mm3": 6000.0}),
    );
}

#[test]
fn a_copied_edit_preserves_the_supplied_face_pick_order() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap());
    let (record, mut order) = body.to_record().unwrap();
    let top = record
        .faces
        .iter()
        .position(|f| matches!(f.surface, solidcore_record::brep::Surface::Plane { origin, .. } if origin[2] == 10.0))
        .unwrap();
    let top_id = order.faces[top];
    // An imported record's index mapping need not be the body's current list order.
    order.faces.rotate_left(1);
    let pick = order.faces.iter().position(|&f| f == top_id).unwrap();
    let mut input = Slot {
        body: body.clone(),
        order: Some(order),
    };
    for step in [
        serde_json::json!({"op": "copy"}),
        serde_json::json!({"op": "transform", "params": {"matrix": [
            [1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]
        ]}}),
    ] {
        input = call(&step, &mut [input]).unwrap().unwrap();
    }
    let out = call(
        &serde_json::json!({"op": "offset_faces", "params": {
            "faces": {"picked": [{"index": pick}]}, "distance": 2.0
        }}),
        &mut [input],
    )
    .unwrap()
    .unwrap();
    assert!((out.body.volume(1e-9).unwrap() - 7200.0).abs() <= 7200.0 * 1e-9);
    let max_z = out
        .body
        .to_record()
        .unwrap()
        .0
        .vertices
        .iter()
        .map(|v| v.point[2])
        .fold(f64::NEG_INFINITY, f64::max);
    assert!((max_z - 12.0).abs() <= 12.0 * 1e-9, "top is at {max_z}");
    assert!((body.volume(1e-9).unwrap() - 6000.0).abs() <= 6000.0 * 1e-9);
}

fn untrim_input(name: &str) -> (PathBuf, Value, Slot) {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join(format!("surfaces/{name}.json"));
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    let name = e["bodies"].as_object().unwrap().keys().next().unwrap();
    let record: Record = serde_json::from_value(records(&entry, &e)[name].clone()).unwrap();
    let (body, order) = Body::from_record(&record, true).unwrap();
    (
        entry,
        e,
        Slot {
            body: Arc::new(body),
            order: Some(order),
        },
    )
}

#[test]
fn all_hole_untrim_restores_the_analytic_area_and_preserves_its_operand() {
    for (name, area) in [
        ("cov-untrim-sheet-hole-in-place", 400.0),
        (
            "cov-untrim-cylinder-sheet-hole-in-place",
            100.0 * std::f64::consts::PI,
        ),
        ("cov-untrim-solid-top-face-copied-out", 400.0),
    ] {
        let (entry, e, mut input) = untrim_input(name);
        let before = input.body.to_record().unwrap().0;
        let before_area = input.body.area(1e-9).unwrap();
        let mut step = e["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["op"] == "untrim_face")
            .unwrap()
            .clone();
        // Reorder the solid's faces so this pick cannot accidentally use a kernel-list index.
        if name.ends_with("copied-out") {
            let order = input.order.as_mut().unwrap();
            assert!(order.faces.len() > 1);
            order.faces.rotate_left(1);
            step["params"]["face"]["picked"][0]["index"] = serde_json::json!(order.faces.len() - 1);
        }
        let output = call(&step, &mut [input.clone()]).unwrap().unwrap();
        let got_area = output.body.area(1e-9).unwrap();
        assert!((got_area - area).abs() <= area * 1e-9, "{name}: {got_area}");
        assert_eq!(output.body.counts().unwrap(), [1, 4, 4], "{name}");
        if name.ends_with("copied-out") {
            let record = output.body.to_record().unwrap().0;
            assert!(!record.vertices.is_empty());
            for vertex in &record.vertices {
                assert!(
                    (vertex.point[2] - 10.0).abs() <= 1e-9,
                    "not the top face: {vertex:?}"
                );
            }
        }
        assert_eq!(input.body.to_record().unwrap().0, before, "{name}");
        assert_eq!(input.body.area(1e-9).unwrap(), before_area, "{name}");
        run_case(&entry, &e).unwrap();
    }
}

#[test]
fn all_hole_untrim_rejects_missing_parameters_and_keeps_point_holes_unsupported() {
    let (_, e, input) = untrim_input("cov-untrim-sheet-hole-in-place");
    let step = e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "untrim_face")
        .unwrap();
    for parameter in ["holes", "outer", "in_place", "face"] {
        let mut missing = step.clone();
        missing["params"].as_object_mut().unwrap().remove(parameter);
        assert!(
            matches!(call(&missing, &mut [input.clone()]), Err(Stop::Fail(_))),
            "{parameter}"
        );
    }
    let mut named = step.clone();
    named["params"]["holes"] = serde_json::json!([[6.0, 10.0, 0.0]]);
    assert!(
        matches!(call(&named, &mut [input]), Err(Stop::Unsupported(why)) if why.contains("hole"))
    );
}

#[test]
fn an_untrim_face_index_past_the_record_cannot_satisfy_a_refusal() {
    let (entry, mut e, _) = untrim_input("cov-untrim-sheet-hole-in-place");
    let step = e["calls"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["op"] == "untrim_face")
        .unwrap();
    step["params"]["face"]["picked"][0]["index"] = serde_json::json!(1);
    step.as_object_mut().unwrap().remove("outputs");
    step["error"] = serde_json::json!("expected operation refusal");
    assert!(matches!(run_case(&entry, &e), Err(Stop::Fail(why)) if why.contains("face")));
}

#[test]
fn untrim_point_holes_do_not_hide_missing_required_parameters() {
    let (_, e, input) = untrim_input("cov-untrim-sheet-hole-in-place");
    let step = e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "untrim_face")
        .unwrap();
    for parameter in ["outer", "in_place", "face"] {
        let mut missing = step.clone();
        missing["params"]["holes"] = serde_json::json!([[6.0, 10.0, 0.0]]);
        missing["params"].as_object_mut().unwrap().remove(parameter);
        assert!(
            matches!(call(&missing, &mut [input.clone()]), Err(Stop::Fail(_))),
            "{parameter}"
        );
    }
}

fn loft_step(name: &str) -> (PathBuf, Value, Value) {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join(format!("ladder/04-spline-profiles/{name}.json"));
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    let step = e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "loft")
        .unwrap()
        .clone();
    (entry, e, step)
}

#[test]
#[allow(clippy::disallowed_methods)] // Independent closed-form test oracle, not ops geometry.
fn a_loft_replays_the_frustum_and_forwards_its_cap() {
    let (entry, e, step) = loft_step("006-loft-circle-frustum");
    let expected_volume = 7000.0 * std::f64::consts::PI;
    let side_area = 30.0 * std::f64::consts::PI * 1000.0_f64.sqrt();
    for cap in [true, false] {
        let mut step = step.clone();
        step["params"]["cap"] = serde_json::json!(cap);
        let output = call(&step, &mut []).unwrap().unwrap();
        let volume = output.body.volume(1e-9).unwrap();
        if cap {
            assert!((volume - expected_volume).abs() <= expected_volume * 1e-9);
            assert_eq!(output.body.counts().unwrap()[0], 3);
        } else {
            assert_eq!(volume, 0.0);
            assert_eq!(output.body.counts().unwrap()[0], 1);
            let area = output.body.area(1e-9).unwrap();
            assert!((area - side_area).abs() <= side_area * 1e-9);
        }
    }
    run_case(&entry, &e).unwrap();
}

#[test]
fn a_loft_forwards_degree_for_the_three_circle_waist() {
    let (_, _, step) = loft_step("007-loft-three-circles");
    for (degree, factor) in [(1, 28000.0 / 3.0), (2, 22400.0 / 3.0)] {
        let mut step = step.clone();
        step["params"]["degree"] = serde_json::json!(degree);
        let output = call(&step, &mut []).unwrap().unwrap();
        let volume = output.body.volume(1e-9).unwrap();
        let expected = factor * std::f64::consts::PI;
        assert!(
            (volume - expected).abs() <= expected * 1e-9,
            "degree {degree}: {volume}"
        );
    }
}

#[test]
fn loft_parameters_are_required_and_refused_degrees_are_not_defaulted() {
    let (_, _, step) = loft_step("006-loft-circle-frustum");
    for parameter in ["sections", "degree", "cap"] {
        let mut missing = step.clone();
        missing["params"].as_object_mut().unwrap().remove(parameter);
        assert!(
            matches!(call(&missing, &mut []), Err(Stop::Fail(_))),
            "{parameter}"
        );
    }
    let mut invalid_section = step.clone();
    invalid_section["params"]["sections"][1] = serde_json::json!([{"unknown_curve": {}}]);
    assert!(matches!(
        call(&invalid_section, &mut []),
        Err(Stop::Fail(_))
    ));
    for degree in [0, 4] {
        let mut refused = step.clone();
        refused["params"]["degree"] = serde_json::json!(degree);
        assert!(
            matches!(call(&refused, &mut []), Ok(Err(_))),
            "degree {degree}"
        );
    }
}

#[test]
fn a_pre_edit_copy_failure_cannot_count_as_the_edits_refusal() {
    let input = Slot {
        body: Arc::new(solidcore_ops::make_box([0.0; 3], [1.0; 3]).unwrap()),
        order: None,
    };
    for op in [
        "copy",
        "transform",
        "shell",
        "offset_faces",
        "thicken",
        "chamfer_sides",
        "stitch",
    ] {
        let step = serde_json::json!({
            "op": op, "error": "expected edit refusal",
            "params": {"matrix": [
                [1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]
            ], "distance": 1, "first": [0,0,1], "second": {"distance": 2}}
        });
        // Inject failure at the copy boundary: no edit has been attempted yet.
        let result = call_with_copy(&step, &mut [input.clone()], |_| {
            Ok(Err("input copy failed".to_owned()))
        });
        match result {
            Ok(Err(why)) if op == "copy" => assert_eq!(why, "input copy failed"),
            Err(Stop::Fail(why)) if op != "copy" => {
                assert!(why.contains("input copy failed"), "{why}");
            }
            Err(other) => panic!("{op}: wrong copy failure: {other:?}"),
            Ok(Err(why)) => panic!("{op}: copy failure became the edit's refusal: {why}"),
            Ok(Ok(_)) => panic!("{op}: failed copy accepted"),
        }
    }
}

#[test]
fn an_invalid_copy_order_cannot_count_as_an_expected_refusal() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [1.0; 3]).unwrap());
    let (_, mut order) = body.to_record().unwrap();
    order
        .faces
        .push(solidcore::topology::FaceId::from_index(1_000_000));
    let input = Slot {
        body,
        order: Some(order),
    };
    for mut step in [
        serde_json::json!({"op": "copy"}),
        serde_json::json!({"op": "transform", "params": {"matrix": [
            [1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]
        ]}}),
        serde_json::json!({"op": "shell", "params": {
            "open_faces": {"picked": []}, "thickness": -0.1
        }}),
        serde_json::json!({"op": "offset_faces", "params": {
            "faces": {"picked": []}, "distance": 0.1
        }}),
        serde_json::json!({"op": "thicken", "params": {
            "thickness": 0.1, "symmetric": false
        }}),
        serde_json::json!({"op": "chamfer_sides", "params": {
            "distance": 1, "first": [0,0,1], "second": {"distance": 2}
        }}),
        serde_json::json!({"op": "stitch", "params": {"tolerance": 1e-5}}),
    ] {
        step["error"] = serde_json::json!("expected refusal");
        match call(&step, &mut [input.clone()]) {
            Err(Stop::Fail(why)) => assert!(why.contains("record order"), "{why}"),
            Err(other) => panic!("wrong mapping failure: {other:?}"),
            Ok(Err(why)) => panic!("mapping error became an operation refusal: {why}"),
            Ok(Ok(_)) => panic!("invalid mapping was accepted"),
        }
    }
}

#[test]
fn a_missing_adjacent_face_cannot_count_as_a_chamfer_refusal() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [20.0; 3]).unwrap());
    let (_, mut order) = body.to_record().unwrap();
    let pair = edge_faces_along_ordered(&body, &order, 0, [0.0, 0.0, 1.0]).unwrap();
    order.faces.remove(pair[0].0);
    let input = Slot {
        body: body.clone(),
        order: Some(order),
    };
    let step = serde_json::json!({
        "op": "chamfer_sides", "error": "expected chamfer refusal",
        "params": {
            "edges": {"picked": [{"index": 0}]}, "first": [0, 0, 1],
            "distance": 5.0, "second": {"distance": 3.0}
        }
    });
    match call(&step, &mut [input]) {
        Err(Stop::Fail(why)) => assert!(why.contains("chamfer face query"), "{why}"),
        Err(other) => panic!("wrong face-query failure: {other:?}"),
        Ok(Err(why)) => panic!("face-query error became a chamfer refusal: {why}"),
        Ok(Ok(_)) => panic!("missing adjacent-face mapping was accepted"),
    }
    assert!((body.volume(1e-9).unwrap() - 8000.0).abs() < 1e-6);
}

#[test]
fn a_discarded_thicken_preserves_the_retained_sheet() {
    let mut k = solidcore::Kernel::default();
    let line = solidcore::curve::line::Line::create_line_segment(
        3,
        solidcore::Vec3::ZERO,
        solidcore::Vec3::new(30.0, 0.0, 0.0),
        None,
    )
    .unwrap();
    let curve = solidcore::curve::curve::Curve::from(line);
    let mut sheet = solidcore::sweep::api_brep::create_linear_sweep(
        &mut k,
        &[Some(&curve)],
        solidcore::Vec3::new(0.0, 1.0, 0.0),
        20.0,
        false,
    )
    .unwrap();
    let record = solidcore_convert::brep::to_record(&mut k, &mut sheet)
        .unwrap()
        .0;
    check_discarded_edit(
        record,
        "thicken",
        &serde_json::json!({"thickness": 2.0, "symmetric": false}),
        &serde_json::json!({"body_count": 1, "volume_mm3": 30.0 * 20.0 * 2.0}),
        serde_json::json!({"body_count": 1, "faces": 1, "edges": 4, "vertices": 4}),
    );
}

/// The sheet normal orders unequal box parts; grading each output name catches aliasing,
/// and neither the solid nor the tool record may change.
#[test]
fn split_sheet_keeps_distinct_parts_in_normal_order_and_preserves_inputs() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    for reversed in [false, true] {
        let mut k = solidcore::Kernel::default();
        let (a, b) = if reversed { (11.0, -1.0) } else { (-1.0, 11.0) };
        let line = solidcore::curve::line::Line::create_line_segment(
            3,
            solidcore::Vec3::new(3.0, a, -1.0),
            solidcore::Vec3::new(3.0, b, -1.0),
            None,
        )
        .unwrap();
        let curve = solidcore::curve::curve::Curve::from(line);
        let mut sheet = solidcore::sweep::api_brep::create_linear_sweep(
            &mut k,
            &[Some(&curve)],
            solidcore::Vec3::new(0.0, 0.0, 1.0),
            12.0,
            false,
        )
        .unwrap();
        let record = solidcore_convert::brep::to_record(&mut k, &mut sheet)
            .unwrap()
            .0;
        let (sheet, order) = Body::from_record(&record, true).unwrap();
        let mut ins = [
            Slot {
                body: Arc::new(solidcore_ops::make_box([0.0; 3], [10.0; 3]).unwrap()),
                order: None,
            },
            Slot {
                body: Arc::new(sheet),
                order: Some(order),
            },
        ];
        let before: Vec<_> = ins.iter().map(|s| s.body.to_record().unwrap().0).collect();
        let step = serde_json::json!({"op": "split_body_by_sheet", "params": {"face": 0}});
        let parts = call_outputs(&step, &mut ins).unwrap().unwrap();
        assert_eq!(parts.len(), 2);
        assert!(!Arc::ptr_eq(&parts[0].body, &parts[1].body));
        let want = if reversed {
            [700.0, 300.0]
        } else {
            [300.0, 700.0]
        };
        for (part, volume) in parts.iter().zip(want) {
            assert!((part.body.volume(1e-9).unwrap() - volume).abs() < 1e-7);
            assert_eq!(part.body.counts().unwrap(), [6, 12, 8]);
            assert!(part.order.is_none());
        }
        let after: Vec<_> = ins.iter().map(|s| s.body.to_record().unwrap().0).collect();
        assert_eq!(before, after);
        // Grade each traced output separately: duplicating one body under both names fails.
        let records = serde_json::json!({"solid": before[0], "sheet": before[1]})
            .as_object()
            .unwrap()
            .clone();
        for (name, volume) in ["back", "front"].into_iter().zip(want) {
            let e = serde_json::json!({
                "bodies": {"solid": {"trusted": true}, "sheet": {"trusted": true}},
                "calls": [{"op": "split_body_by_sheet", "inputs": ["solid", "sheet"],
                    "params": {"face": 0}, "outputs": ["back", "front"]}],
                "result": [{"made": name}],
                "expected": {"body_count": 1, "volume_mm3": volume,
                    "faces": 6, "edges": 12, "vertices": 8, "tolerance": {"rel": 1e-9}}
            });
            assert!(run_case_with_records(&e, || records.clone())
                .unwrap()
                .is_some());
        }
    }
}

/// The recorded spline sheet cuts the cup into two parts and remains as the third body.
#[test]
fn split_sheet_replays_the_cup_and_retains_the_tool() {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join("booleans/3830-split-cup-by-extended-spline-surface.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    assert_eq!(e["result"].as_array().unwrap().len(), 3);
    assert!(run_case(&entry, &e).unwrap().is_some());
}

/// The corpus's stitch of two 10 x 10 sheets that share the edge x = 10, and its operand.
fn two_squares() -> (PathBuf, Value, Slot) {
    session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join("surfaces/cov-stitch-two-squares-along-a-shared-edge.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    let record: Record = serde_json::from_value(records(&entry, &e)["b1"].clone()).unwrap();
    let (body, order) = Body::from_record(&record, true).unwrap();
    let input = Slot {
        body: Arc::new(body),
        order: Some(order),
    };
    (entry, e, input)
}

/// The traced stitch sews a copy of its operand along the shared edge and leaves the operand as
/// it came in; the whole case replays to the rig's values.
#[test]
fn a_stitch_sews_a_copy_at_the_traced_tolerance() {
    let (entry, e, input) = two_squares();
    let step = &e["calls"][1];
    assert_eq!(step["op"], "stitch");
    let out = call(step, &mut [input.clone()]).unwrap().unwrap();
    // the shared edge is one edge of both faces, and its two vertex pairs are one each
    assert_eq!(out.body.counts().unwrap(), [2, 7, 6]);
    assert_eq!(input.body.counts().unwrap(), [2, 8, 8]);
    run_case(&entry, &e).unwrap();
}

/// A tolerance the operation refuses is its refusal; a call without one is bad replay data.
#[test]
fn a_stitch_refusal_is_the_operations_and_a_missing_tolerance_is_not() {
    let (_, _, input) = two_squares();
    let step = |params: Value| serde_json::json!({"op": "stitch", "params": params});
    assert!(matches!(
        call(
            &step(serde_json::json!({"tolerance": 0.0})),
            &mut [input.clone()]
        ),
        Ok(Err(_))
    ));
    assert!(matches!(
        call(&step(serde_json::json!({})), &mut [input]),
        Err(Stop::Fail(why)) if why.contains("tolerance")
    ));
}

fn grade(entry: &Path, listed: &BTreeMap<String, Listed>, timeout: Duration) -> Graded {
    let e: Value = serde_json::from_str(&std::fs::read_to_string(entry).unwrap()).unwrap();
    let area = e["area"].as_str().unwrap().to_owned();
    let case = e["case"].as_str().unwrap().to_owned();
    let key = format!("{area}/{case}");
    let mut g = Graded {
        area,
        case,
        kind: Kind::Pass,
        note: String::new(),
        unported: Vec::new(),
        deviation: None,
        moved: false,
        hang: false,
    };
    let is_listed = listed.contains_key(&key);
    let rig_status = e["rig"]["status"].as_str().unwrap_or("").to_owned();
    if !e["operation_expressible"].as_bool().unwrap_or(false) {
        g.kind = if is_listed {
            Kind::KnownFailing
        } else {
            Kind::Unsupported
        };
        g.note = format!("not the kernel's alone: {}", e["why_not"]);
        return g;
    }
    let mut unported: BTreeSet<String> = BTreeSet::new();
    for c in e["calls"].as_array().unwrap() {
        let op = c["op"].as_str().unwrap();
        if !PORTED.contains(&op) {
            unported.insert(op.to_owned());
        }
    }
    if !unported.is_empty() {
        g.unported = unported.into_iter().collect();
        g.kind = if is_listed {
            Kind::KnownFailing
        } else {
            Kind::Unported
        };
        g.note = format!("unported: {}", g.unported.join(", "));
        return g;
    }
    let (tx, rx) = mpsc::channel();
    let path = entry.to_path_buf();
    let started = Instant::now();
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(|| run_case(&path, &e));
        let _ = tx.send(r);
    });
    let r = match rx.recv_timeout(timeout) {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => Err(Stop::Fail("panicked".to_owned())),
        Err(_) => {
            g.kind = if is_listed {
                Kind::KnownFailing
            } else {
                Kind::Fail
            };
            g.note = format!("hang: past {} s", started.elapsed().as_secs());
            g.moved = is_listed;
            g.hang = true;
            g.note
                .push_str(" (the run stops here: the case holds the process kernel)");
            return g;
        }
    };
    match r {
        Ok(dev) => {
            g.deviation = dev;
            if is_listed {
                g.kind = Kind::KnownFailing;
                "the rig's result (still failing as listed)".clone_into(&mut g.note);
            } else if rig_status == "unsupported" {
                g.kind = Kind::Unsupported;
                "rated unsupported by the rig".clone_into(&mut g.note);
            }
        }
        Err(Stop::Unsupported(w)) => {
            g.kind = if is_listed {
                Kind::KnownFailing
            } else {
                Kind::Unsupported
            };
            g.note = w;
        }
        Err(Stop::Fail(w)) => {
            g.kind = if is_listed {
                Kind::KnownFailing
            } else {
                Kind::Fail
            };
            g.moved = is_listed;
            g.note = w;
        }
    }
    g
}

#[test]
#[ignore = "the whole corpus is slow; run it with --ignored, KERNEL_CASES_AREA shards it"]
fn kernel_cases() {
    let fixes: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("fixes.json")).unwrap())
            .unwrap();
    session::set_fixes(fixes.iter().map(String::as_str)).unwrap();
    let listed = known_failing();
    let areas: Option<Vec<String>> = std::env::var("KERNEL_CASES_AREA").ok().map(|a| {
        a.split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    });
    let only = std::env::var("KERNEL_CASES_CASE").ok();
    let timeout = Duration::from_secs(
        std::env::var("KERNEL_CASES_TIMEOUT_S")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(180),
    );
    let mut files = Vec::new();
    entries(&corpus(), &mut files);
    files.sort();
    let root = corpus();
    let selected: Vec<PathBuf> = files
        .into_iter()
        .filter(|f| {
            let rel = f
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let area_ok = areas.as_ref().is_none_or(|a| {
                a.iter()
                    .any(|x| rel == *x || rel.starts_with(&format!("{x}/")))
            });
            area_ok && only.as_ref().is_none_or(|c| rel.contains(c.as_str()))
        })
        .collect();
    // every listed case exists
    let all: BTreeSet<String> = {
        let mut f = Vec::new();
        entries(&root, &mut f);
        f.iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    };
    for k in listed.keys() {
        assert!(
            all.contains(k),
            "known_failing.json lists {k}, which the corpus does not have"
        );
    }

    let mut graded = Vec::new();
    let mut not_run = 0;
    for f in &selected {
        if graded.last().is_some_and(|g: &Graded| g.hang) {
            not_run += 1;
            continue;
        }
        graded.push(grade(f, &listed, timeout));
    }

    let report = report(&graded, &listed, not_run);
    println!("{report}");
    if let Ok(p) = std::env::var("KERNEL_CASES_REPORT") {
        std::fs::write(p, &report).unwrap();
    }
    let failed: Vec<_> = graded.iter().filter(|g| g.kind == Kind::Fail).collect();
    let moved: Vec<_> = graded.iter().filter(|g| g.moved).collect();
    if std::env::var_os("KERNEL_CASES_REPORT_ONLY").is_none() {
        assert!(
            failed.is_empty() && moved.is_empty() && not_run == 0,
            "{} failed, {} listed moved, {not_run} not run",
            failed.len(),
            moved.len()
        );
    }
}

fn report(graded: &[Graded], listed: &BTreeMap<String, Listed>, not_run: usize) -> String {
    let mut by_area: BTreeMap<&str, [usize; 5]> = BTreeMap::new();
    let mut dev: BTreeMap<&str, f64> = BTreeMap::new();
    let mut ops: BTreeMap<&str, usize> = BTreeMap::new();
    for g in graded {
        let row = by_area.entry(&g.area).or_default();
        row[g.kind as usize] += 1;
        if let Some(d) = g.deviation {
            let m = dev.entry(&g.area).or_default();
            *m = m.max(d);
        }
        for o in &g.unported {
            *ops.entry(o).or_default() += 1;
        }
    }
    let mut s = String::new();
    writeln!(s, "| Area | Cases | pass | fail | unsupported | unported | known-failing | max rel. volume dev. |").unwrap();
    writeln!(s, "|---|---|---|---|---|---|---|---|").unwrap();
    let mut total = [0; 5];
    for (a, row) in &by_area {
        let n: usize = row.iter().sum();
        for (t, r) in total.iter_mut().zip(row) {
            *t += r;
        }
        let d = dev
            .get(a)
            .map_or_else(|| "-".to_owned(), |d| format!("{d:.1e}"));
        writeln!(
            s,
            "| {a} | {n} | {} | {} | {} | {} | {} | {d} |",
            row[0], row[1], row[2], row[3], row[4]
        )
        .unwrap();
    }
    let n: usize = total.iter().sum();
    writeln!(
        s,
        "| **all** | {n} | {} | {} | {} | {} | {} | |",
        total[0], total[1], total[2], total[3], total[4]
    )
    .unwrap();
    if not_run > 0 {
        writeln!(s, "\n{not_run} case(s) not run after a hang.").unwrap();
    }
    writeln!(s, "\nUnported operations (cases needing each):").unwrap();
    for (o, n) in &ops {
        writeln!(s, "- {o}: {n}").unwrap();
    }
    let mut section = |title: &str, f: &dyn Fn(&Graded) -> bool| {
        let v: Vec<_> = graded.iter().filter(|g| f(g)).collect();
        if !v.is_empty() {
            writeln!(s, "\n{title} ({}):", v.len()).unwrap();
            for g in v {
                writeln!(s, "- {}/{}: {}", g.area, g.case, g.note).unwrap();
            }
        }
    };
    section("Fail", &|g| g.kind == Kind::Fail);
    section(
        "Known-failing whose result moved from the rig's (check: fixed, or a new failure)",
        &|g| g.moved,
    );
    section("Unsupported, run", &|g| {
        g.kind == Kind::Unsupported && !g.note.starts_with("not the kernel")
    });
    let ran: Vec<_> = graded
        .iter()
        .filter(|g| {
            g.kind == Kind::KnownFailing && listed.contains_key(&format!("{}/{}", g.area, g.case))
        })
        .collect();
    writeln!(s, "\nKnown-failing ({}):", ran.len()).unwrap();
    for g in ran {
        let l = &listed[&format!("{}/{}", g.area, g.case)];
        writeln!(s, "- {}/{} [{}]: {}", g.area, g.case, l.issue, g.note).unwrap();
    }
    s
}
