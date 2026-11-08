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
//!   with its reason and issue. A listed case is steady while it reproduces the rig's result,
//!   or, when its entry records `fails` (solidcore fails it differently from the rig), while it
//!   fails exactly so. A listed case that moves from that (it passes, or fails otherwise) is
//!   reported, so the list can shrink.
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
//! - `KERNEL_CASES_REPORT_ONLY`: set, the test reports without failing;
//! - `KERNEL_CASES_SMB`: a directory to write each run case's bodies to as `<case>.smb`, for
//!   its render (`solidcore-diag`): the final bodies, or the inputs of the refusal that ends it.

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
    blend_ordered, chamfer_sides_ordered, edge_faces_along_ordered, fillet_variable_ordered, Blend,
    ChamferSecond, Corners, SetBack, Turn,
};
use solidcore_ops::boolean::{boolean, union_all, BooleanOp};
use solidcore_ops::extend::{extend_sheet, Continuation};
use solidcore_ops::split::{split_faces, Tool};
use solidcore_ops::stitch::{merge_faces, stitch};
use solidcore_ops::sweep::{
    extrude, loft, revolve, screw_sweep, sweep_placed, sweep_stations, Affine, Curve, DraftCorner,
    Ends, SweepFrame,
};
use solidcore_ops::untrim::{untrim_face, Holes};
use solidcore_ops::{make_sphere, offset, session, Body, Order};
use solidcore_record::brep::{Brep as Record, Frame, Surface as RecordSurface};

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
    "split_faces",
    "extend_sheet",
    "draft",
    "thicken",
    "untrim_face",
    "boolean",
    "union_all",
    "extrude",
    "revolve",
    "loft",
    "sweep_stations",
    "sweep",
    "screw_sweep",
    "blend_dropping",
    "blend_setback",
    "fillet_variable",
    "chamfer_sides",
    "stitch",
    "split_body_by_sheet",
    "split_body",
    "merge_faces",
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
    /// For a case solidcore fails differently from the rig's recorded result: the failure it
    /// was listed for, as the run reports it. The case is steady while it fails exactly so,
    /// and moved when it passes or fails otherwise. Without it, a listed case is steady while
    /// it reproduces the rig's result.
    #[serde(default)]
    fails: Option<String>,
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

/// A pick's record indices, optionally bounded by its record order's length.
fn picked(op: &str, selection: &Value, length: Option<usize>) -> Result<Vec<usize>, Stop> {
    let bad = |what: &str| Stop::Fail(format!("{op}: parameter {what}"));
    selection["picked"]
        .as_array()
        .ok_or_else(|| bad("picked"))?
        .iter()
        .map(|pick| {
            if let Some(miss) = pick.get("miss") {
                let why = miss.as_str().ok_or_else(|| bad("miss"))?;
                if pick.get("index").is_some() {
                    return Err(bad("pick must hit or miss"));
                }
                return Err(Stop::Unsupported(format!("{op}: pick missed: {why}")));
            }
            let index = pick["index"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| bad("index"))?;
            if length.is_some_and(|length| index >= length) {
                return Err(bad("pick outside record order"));
            }
            Ok(index)
        })
        .collect()
}

/// Parses typed record picks without reordering them: misses are unsupported; bad data fails.
fn record_picks(
    op: &str,
    selection: &Value,
    kind: &str,
    length: usize,
) -> Result<Vec<usize>, Stop> {
    if selection["kind"].as_str() != Some(kind) {
        return Err(Stop::Fail(format!("{op}: parameter pick kind")));
    }
    picked(op, selection, Some(length))
}

/// Checks a single-pick operation's cardinality before classifying a traced miss.
fn record_pick(op: &str, selection: &Value, kind: &str, length: usize) -> Result<usize, Stop> {
    let bad = || Stop::Fail(format!("{op}: exactly one {kind} pick required"));
    if selection["picked"].as_array().map(Vec::len) != Some(1) {
        return Err(bad());
    }
    record_picks(op, selection, kind, length)?
        .first()
        .copied()
        .ok_or_else(bad)
}

/// Looks up the record order once; an unrepresentable result is unsupported, not a refusal.
fn ensure_order(slot: &mut Slot, op: &str) -> Result<Order, Stop> {
    ensure_order_with(slot, op, Body::to_record)
}

fn ensure_order_with(
    slot: &mut Slot,
    op: &str,
    export: impl FnOnce(&Body) -> solidcore_ops::Result<(Record, Order)>,
) -> Result<Order, Stop> {
    if slot.order.is_none() {
        // a body an operation made: its record order is the one its record is written in
        let (_, order) = export(&slot.body).map_err(|e| match e {
            solidcore_ops::Error::Unsupported(why) => {
                Stop::Unsupported(format!("{op}: to_record: {why}"))
            }
            e => Stop::Fail(format!("{op}: to_record: {e}")),
        })?;
        slot.order = Some(order);
    }
    slot.order
        .clone()
        .ok_or_else(|| Stop::Fail(format!("{op}: no record order")))
}

/// The picked faces' record indices, and the record order they are taken through: an operation
/// takes record indices with the body's [`Order`].
fn faces(slot: &mut Slot, pick: &Value) -> Result<(Vec<usize>, Order), Stop> {
    let record = picked("pick", pick, None)?;
    Ok((record, ensure_order(slot, "pick")?))
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

/// Converts the trace's planar or cylindrical tool to record parameters; angles alone
/// change units from the trace's degrees to the record's radians.
fn split_tool(t: &Value) -> Result<Tool, Stop> {
    let bad = |field: &str| Stop::Fail(format!("split: tool parameter {field}"));
    let object = t.as_object().ok_or_else(|| bad("object"))?;
    if object.len() != 1 {
        return Err(bad("exactly one named kind"));
    }
    let kind = object.keys().next().ok_or_else(|| bad("kind"))?;
    if kind.is_empty() {
        return Err(bad("kind name"));
    }
    let v3 = |p: &Value, key: &str| f64s::<3>(&p[key]).ok_or_else(|| bad(key));
    let v2 = |p: &Value, key: &str| f64s::<2>(&p[key]).ok_or_else(|| bad(key));
    if let Some(p) = t.get("plane") {
        return Ok(Tool {
            surface: RecordSurface::Plane {
                origin: v3(p, "origin")?,
                u_axis: v3(p, "xaxis")?,
                v_axis: v3(p, "yaxis")?,
            },
            domain: [v2(p, "u")?, v2(p, "v")?],
        });
    }
    if let Some(c) = t.get("cylinder") {
        return Ok(Tool {
            surface: RecordSurface::Cylinder {
                frame: Frame {
                    origin: v3(c, "origin")?,
                    x_axis: v3(c, "xaxis")?,
                    y_axis: v3(c, "yaxis")?,
                },
                radius: c["radius"].as_f64().ok_or_else(|| bad("radius"))?,
            },
            domain: [
                v2(c, "angle_deg")?.map(|angle| angle * (std::f64::consts::PI / 180.0)),
                v2(c, "height")?,
            ],
        });
    }
    Err(Stop::Unsupported(format!(
        "split: tool kind {kind} not ported"
    )))
}

/// Converts either split operation's ordered pair without aliasing the two outputs.
fn made_parts(
    result: solidcore_ops::Result<Option<(Body, Body)>>,
    no_split: &str,
) -> Result<Vec<Slot>, String> {
    let (back, front) = result
        .map_err(|e| e.to_string())?
        .ok_or_else(|| no_split.to_owned())?;
    Ok([back, front]
        .into_iter()
        .map(|body| Slot {
            body: Arc::new(body),
            order: None,
        })
        .collect())
}

/// Runs a traced call in output order, keeping a split's two bodies distinct.
fn call_outputs(c: &Value, ins: &mut [Slot]) -> Result<Result<Vec<Slot>, String>, Stop> {
    if c["op"].as_str() == Some("split_body") {
        if ins.len() != 1 {
            return Err(Stop::Fail("split_body: requires one body".to_owned()));
        }
        let tool = split_tool(&c["params"]["tool"])?;
        return Ok(made_parts(
            solidcore_ops::split::split_body(&ins[0].body, &tool),
            "tool does not divide the body",
        ));
    }
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
    Ok(made_parts(
        solidcore_ops::split::split_body_by_sheet(&body[0].body, &sheet.body, order, face),
        "sheet does not divide the body",
    ))
}

/// Runs one traced call: its output, or the operation's error as text.
fn call(c: &Value, ins: &mut [Slot]) -> Result<Result<Slot, String>, Stop> {
    call_with_copy(c, ins, Slot::copied)
}

/// Dispatches a single-output call while keeping preparation failures separate from refusals.
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
        "fillet_variable" => {
            if ins.len() != 1 {
                return Err(bad("one input"));
            }
            let mut source = first()?;
            ensure_order(&mut source, "fillet_variable")?;
            let s = copy_for_edit(&source)?;
            let order = s
                .order
                .as_ref()
                .ok_or_else(|| Stop::Fail("fillet_variable: missing copied order".to_owned()))?;
            let edge = record_pick("fillet_variable", &p["edge"], "edge", order.edges.len())?;
            let start = f64s::<3>(&p["start"]).ok_or_else(|| bad("start"))?;
            let degree = p["degree"]
                .as_u64()
                .and_then(|d| usize::try_from(d).ok())
                .ok_or_else(|| bad("degree"))?;
            let numbers = |key: &str| {
                p[key]
                    .as_array()
                    .and_then(|a| a.iter().map(Value::as_f64).collect::<Option<Vec<_>>>())
                    .ok_or_else(|| bad(key))
            };
            let knots = numbers("knots")?;
            let radii = numbers("radii")?;
            let r = fillet_variable_ordered(&s.body, order, edge, start, degree, &knots, &radii);
            in_place(Slot { order: None, ..s }, r)
        }
        "extend_sheet" => {
            if ins.len() != 1 {
                return Err(bad("one input"));
            }
            let mut source = first()?;
            ensure_order(&mut source, "extend_sheet")?;
            // Map the source record's indices through the preparation copy.
            let s = copy_for_edit(&source)?;
            let order = s
                .order
                .as_ref()
                .ok_or_else(|| Stop::Fail("extend_sheet: missing copied order".to_owned()))?;
            let edge = record_pick("extend_sheet", &p["edge"], "edge", order.edges.len())?;
            let distance = p["distance"].as_f64().ok_or_else(|| bad("distance"))?;
            let how = match p["how"].as_str().ok_or_else(|| bad("how"))? {
                "natural" => Continuation::Natural,
                "tangent" => Continuation::Tangent,
                "curvature" => Continuation::Curvature,
                how => {
                    return Err(Stop::Unsupported(format!(
                        "extend_sheet: continuation {how} not ported"
                    )))
                }
            };
            made(extend_sheet(&s.body, order, edge, distance, how))
        }
        "split_faces" => {
            if ins.len() != 1 {
                return Err(bad("one input"));
            }
            let mut s = first()?;
            let picked = p["faces"]
                .as_array()
                .ok_or_else(|| bad("faces"))?
                .iter()
                .map(|i| {
                    i.as_u64()
                        .and_then(|i| usize::try_from(i).ok())
                        .ok_or_else(|| bad("face index"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let tools = p["tools"]
                .as_array()
                .ok_or_else(|| bad("tools"))?
                .iter()
                .map(split_tool)
                .collect::<Result<Vec<_>, _>>()?;
            let order = ensure_order(&mut s, "split_faces")?;
            if picked.iter().any(|&i| order.faces.get(i).is_none()) {
                return Err(bad("face outside record order"));
            }
            match split_faces(&s.body, &order, &picked, &tools) {
                Ok(Some((body, _))) => made(Ok(body)),
                Ok(None) => Err("tools do not split any selected face".to_owned()),
                Err(e) => Err(e.to_string()),
            }
        }
        "offset_faces" => {
            let mut s = copy_for_edit(&first()?)?;
            let (picked, order) = faces(&mut s, &p["faces"])?;
            let d = p["distance"].as_f64().ok_or_else(|| bad("distance"))?;
            let r = offset::offset_faces(&s.body, &order, &picked, d);
            in_place(Slot { order: None, ..s }, r)
        }
        "draft" => {
            if ins.len() != 1 {
                return Err(bad("one input"));
            }
            let mut s = copy_for_edit(&first()?)?;
            let angle = p["angle"].as_f64().ok_or_else(|| bad("angle"))?;
            let origin = f64s::<3>(&p["neutral_origin"]).ok_or_else(|| bad("neutral_origin"))?;
            let normal = f64s::<3>(&p["neutral_normal"]).ok_or_else(|| bad("neutral_normal"))?;
            let pull = f64s::<3>(&p["pull"]).ok_or_else(|| bad("pull"))?;
            let selection = &p["faces"];
            if selection["kind"].as_str() != Some("face") {
                return Err(bad("faces.kind"));
            }
            let picks = selection["picked"]
                .as_array()
                .ok_or_else(|| bad("faces.picked"))?;
            for pick in picks {
                if pick["index"]
                    .as_u64()
                    .and_then(|i| usize::try_from(i).ok())
                    .is_none()
                {
                    return Err(bad("faces.index"));
                }
            }
            if s.order.is_none() {
                let (_, order) = s
                    .body
                    .to_record()
                    .map_err(|e| Stop::Fail(format!("order before draft: {e}")))?;
                s.order = Some(order);
            }
            let (picked, order) = faces(&mut s, selection)?;
            for &face in &picked {
                if order.faces.get(face).is_none() {
                    return Err(bad("face outside record order"));
                }
            }
            let result = offset::draft(&s.body, &order, &picked, origin, normal, pull, angle);
            in_place(Slot { order: None, ..s }, result)
        }
        "thicken" => {
            let s = copy_for_edit(&first()?)?;
            let t = p["thickness"].as_f64().ok_or_else(|| bad("thickness"))?;
            let sym = p["symmetric"].as_bool().ok_or_else(|| bad("symmetric"))?;
            let r = offset::thicken(&s.body, t, sym);
            in_place(Slot { order: None, ..s }, r)
        }
        "union_all" => {
            let bodies = ins
                .iter()
                .map(|s| {
                    let copied = copy_for_edit(s)?;
                    Arc::try_unwrap(copied.body).map_err(|_| {
                        Stop::Fail("copy before union_all: the copy is shared".to_owned())
                    })
                })
                .collect::<Result<Vec<_>, Stop>>()?;
            made(union_all(bodies))
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
        "sweep" => {
            if !ins.is_empty() {
                return Err(bad("no inputs"));
            }
            let curves = profile(&p["profile"]).ok_or_else(|| bad("profile"))?;
            let path = profile(&p["path"]).ok_or_else(|| bad("path"))?;
            let cap = p["cap"].as_bool().ok_or_else(|| bad("cap"))?;
            let frame = match p["frame"].as_str().ok_or_else(|| bad("frame"))? {
                "follow" => SweepFrame::Follow,
                "translate" => SweepFrame::Translate,
                other => return Err(Stop::Unsupported(format!("sweep frame {other}"))),
            };
            // The trace binds the body; placements and route are diagnostics, not outputs.
            made(sweep_placed(&curves, &path, frame, cap).map(|(body, _, _)| body))
        }
        "screw_sweep" => {
            if !ins.is_empty() {
                return Err(bad("no inputs"));
            }
            let curves = profile(&p["profile"]).ok_or_else(|| bad("profile"))?;
            let origin = f64s::<3>(&p["origin"]).ok_or_else(|| bad("origin"))?;
            let direction = f64s::<3>(&p["direction"]).ok_or_else(|| bad("direction"))?;
            let pitch = p["pitch"].as_f64().ok_or_else(|| bad("pitch"))?;
            let turns = p["turns"].as_f64().ok_or_else(|| bad("turns"))?;
            let tolerance = p["tolerance"].as_f64().ok_or_else(|| bad("tolerance"))?;
            let law = match p.get("law") {
                None => None,
                Some(v) => Some(
                    v.as_array()
                        .ok_or_else(|| bad("law"))?
                        .iter()
                        .map(|pair| {
                            let [turn, offset] = f64s::<2>(pair)?;
                            Some((turn, offset))
                        })
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| bad("law"))?,
                ),
            };
            made(screw_sweep(
                &curves,
                origin,
                direction,
                pitch,
                turns,
                law.as_deref(),
                tolerance,
            ))
        }
        "sweep_stations" => {
            if !ins.is_empty() {
                return Err(bad("no inputs"));
            }
            let curves = profile(&p["profile"]).ok_or_else(|| bad("profile"))?;
            let stations = p["stations"]
                .as_array()
                .ok_or_else(|| bad("stations"))?
                .iter()
                .map(|s| {
                    let m = s["m"].as_array()?;
                    let [x, y, z] = m.as_slice() else {
                        return None;
                    };
                    Some(Affine {
                        m: [f64s::<3>(x)?, f64s::<3>(y)?, f64s::<3>(z)?],
                        t: f64s::<3>(&s["t"])?,
                    })
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| bad("stations"))?;
            let ends = match p["ends"].as_str().ok_or_else(|| bad("ends"))? {
                "open" => Ends::Open,
                "capped" => Ends::Capped,
                "joined" => Ends::Joined,
                other => return Err(Stop::Unsupported(format!("sweep_stations ends {other}"))),
            };
            made(sweep_stations(&curves, &stations, ends))
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
        "blend_setback" => {
            if ins.len() != 1 {
                return Err(bad("one input"));
            }
            let mut source = first()?;
            ensure_order(&mut source, "blend_setback")?;
            let s = copy_for_edit(&source)?;
            let order = s
                .order
                .as_ref()
                .ok_or_else(|| Stop::Fail("blend_setback: missing copied order".to_owned()))?;
            let kind = match p["kind"].as_str() {
                Some("round") => Blend::Round,
                Some("curvature") => Blend::Curvature,
                Some("chord") => Blend::Chord,
                Some("chamfer") => Blend::Chamfer,
                _ => return Err(bad("kind")),
            };
            let edges = record_picks("blend_setback", &p["edges"], "edge", order.edges.len())?;
            let vertices = record_picks(
                "blend_setback",
                &p["corners"],
                "vertex",
                order.vertices.len(),
            )?;
            let numbers = |key: &str| {
                p[key]
                    .as_array()
                    .and_then(|a| a.iter().map(Value::as_f64).collect::<Option<Vec<_>>>())
                    .ok_or_else(|| bad(key))
            };
            let sizes = numbers("sizes")?;
            let distances = numbers("setbacks")?;
            if edges.is_empty()
                || vertices.is_empty()
                || sizes.len() != edges.len()
                || distances.len() != vertices.len()
            {
                return Err(bad("sizes and setbacks must match their picks"));
            }
            // Validate every supplied value before retaining the first pick of each entity,
            // as the traced wrapper does: a bad duplicate must not disappear.
            if sizes
                .iter()
                .chain(&distances)
                .any(|&x| !x.is_finite() || x <= 0.0 || x > solidcore_ops::blend::MAX_SIZE)
            {
                return Ok(Err(solidcore_ops::Error::Argument.to_string()));
            }
            let mut unique = Vec::new();
            let mut each = Vec::new();
            for (edge, size) in edges.into_iter().zip(sizes) {
                if !unique.contains(&edge) {
                    unique.push(edge);
                    each.push(size);
                }
            }
            let mut setbacks: Vec<SetBack> = Vec::new();
            for (vertex, distance) in vertices.into_iter().zip(distances) {
                if !setbacks.iter().any(|s| s.vertex == vertex) {
                    setbacks.push(SetBack { vertex, distance });
                }
            }
            let r = blend_ordered(
                &s.body,
                order,
                kind,
                &unique,
                &each,
                &setbacks,
                Corners::Filled,
                Turn::Mitre,
            )
            .map(|_| ());
            in_place(Slot { order: None, ..s }, r)
        }
        "blend_dropping" => {
            // a trace older than SolidCraft's turn names none: its rounds turned mitred
            let turn = match p.get("turn").map(Value::as_str) {
                None | Some(Some("mitre")) => Turn::Mitre,
                Some(Some("swept")) => Turn::Swept,
                Some(_) => return Err(bad("turn")),
            };
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
            let (edges, order) = faces(&mut s, &p["edges"])?;
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
            let r = blend_ordered(
                &s.body,
                &order,
                kind,
                &unique,
                &each,
                &[],
                Corners::Filled,
                turn,
            )
            .map(|_| ());
            in_place(Slot { order: None, ..s }, r)
        }
        "merge_faces" => {
            if ins.len() != 1 {
                return Err(bad("one input"));
            }
            let mut s = copy_for_edit(&first()?)?;
            let order = ensure_order(&mut s, "merge_faces")?;
            let keep = record_picks("merge_faces", &p["keep"], "edge", order.edges.len())?;
            let body = Arc::try_unwrap(s.body).map_err(|_| {
                Stop::Fail("copy before merge_faces: the copy is shared".to_owned())
            })?;
            made(merge_faces(body, &order, &keep))
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

/// One geometric seam expectation, independent of result entity numbering.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedSeam {
    #[serde(default)]
    body: usize,
    ends: [[f64; 3]; 2],
    normal_dot: f64,
    position_abs_mm: f64,
    normal_dot_abs: f64,
    max_edge_tolerance_mm: f64,
    max_gap_mm: f64,
}

/// Grade each named seam exactly once, including its angle, gaps and tolerance cap.
/// Endpoint pairs may be reversed; duplicate expectations cannot consume one edge twice.
fn grade_seams(result: &[Slot], expected: &Value) -> Result<(), Stop> {
    let seams: Vec<ExpectedSeam> = serde_json::from_value(expected.clone())
        .map_err(|e| Stop::Fail(format!("invalid seam expectations: {e}")))?;
    if seams.is_empty() {
        return Err(Stop::Fail("seam expectations are empty".into()));
    }
    let mut used = BTreeSet::new();
    for (number, seam) in seams.iter().enumerate() {
        if !seam.ends.iter().flatten().all(|x| x.is_finite())
            || !(-1.0..=1.0).contains(&seam.normal_dot)
            || ![
                seam.position_abs_mm,
                seam.normal_dot_abs,
                seam.max_edge_tolerance_mm,
                seam.max_gap_mm,
            ]
            .iter()
            .all(|x| x.is_finite() && *x >= 0.0)
        {
            return Err(Stop::Fail(format!("invalid seam {number}: {seam:?}")));
        }
        let slot = result
            .get(seam.body)
            .ok_or_else(|| Stop::Fail(format!("seam {number}: missing body {}", seam.body)))?;
        let (record, order) = slot
            .body
            .to_record()
            .map_err(|e| Stop::Fail(e.to_string()))?;
        let close = |a: [f64; 3], b: [f64; 3]| {
            a.iter()
                .zip(b)
                .all(|(x, y)| (x - y).abs() <= seam.position_abs_mm)
        };
        let matches: Vec<_> = record
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                let a = record.vertices[edge.vertices[0] as usize].point;
                let b = record.vertices[edge.vertices[1] as usize].point;
                (close(a, seam.ends[0]) && close(b, seam.ends[1]))
                    || (close(a, seam.ends[1]) && close(b, seam.ends[0]))
            })
            .collect();
        if matches.len() != 1 {
            return Err(Stop::Fail(format!(
                "seam {number}: {} edges match {:?}",
                matches.len(),
                seam.ends
            )));
        }
        let (edge_index, edge) = matches[0];
        if !used.insert((seam.body, edge_index)) {
            return Err(Stop::Fail(format!(
                "seam {number}: duplicate edge expectation"
            )));
        }
        if edge.uses.len() != 2
            || !edge.tolerance.is_finite()
            || edge.tolerance < 0.0
            || edge.tolerance > seam.max_edge_tolerance_mm
        {
            return Err(Stop::Fail(format!(
                "seam {number}: {} uses, tolerance {} (cap {})",
                edge.uses.len(),
                edge.tolerance,
                seam.max_edge_tolerance_mm
            )));
        }
        let mut normals = Vec::new();
        for edge_use in &edge.uses {
            let face = edge_use.face as usize;
            normals.push(
                slot.body
                    .face_normal_at_edge(&order, edge_index, face)
                    .map_err(|e| Stop::Fail(e.to_string()))?,
            );
            let samples = slot
                .body
                .edge_face_gap_samples(&order, edge_index, face)
                .map_err(|e| Stop::Fail(e.to_string()))?;
            if samples.is_empty()
                || samples.iter().any(|s| {
                    !s.distance.value.is_finite()
                        || !s.distance.bound.is_finite()
                        || s.distance.bound < 0.0
                        || s.distance.value > edge.tolerance + s.distance.bound
                        || s.distance.value + s.distance.bound > seam.max_gap_mm
                })
            {
                return Err(Stop::Fail(format!("seam {number}: invalid edge/face gap")));
            }
        }
        let dot: f64 = normals[0]
            .value
            .iter()
            .zip(normals[1].value)
            .map(|(a, b)| a * b)
            .sum();
        if !dot.is_finite() || (dot - seam.normal_dot).abs() > seam.normal_dot_abs {
            return Err(Stop::Fail(format!(
                "seam {number}: normal dot {dot} (expected {})",
                seam.normal_dot
            )));
        }
    }
    Ok(())
}

/// With `KERNEL_CASES_SMB` set, writes `bodies` there as `<case>.smb`, `<case>-1.smb`, ...
fn write_smb(e: &Value, bodies: &[Slot]) {
    let Some(dir) = std::env::var_os("KERNEL_CASES_SMB") else {
        return;
    };
    let case = e["case"].as_str().unwrap();
    for (i, s) in bodies.iter().enumerate() {
        let name = match i {
            0 => format!("{case}.smb"),
            _ => format!("{case}-{i}.smb"),
        };
        std::fs::write(Path::new(&dir).join(name), s.body.to_smb(true).unwrap()).unwrap();
    }
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
            (Err(_), Some(_)) => {
                write_smb(e, &ins);
                return Ok(None);
            }
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
    write_smb(e, &result);
    let got = measure(&result).map_err(Stop::Fail)?;
    if let Some(seams) = e["expected"].get("seams") {
        grade_seams(&result, seams)?;
    }
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

/// A wrong endpoint, angle, tolerance cap or duplicate seam cannot pass the grader.
#[test]
fn seam_expectations_grade_geometry_and_reject_bad_metadata() {
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [10.0; 3]).unwrap());
    let result = [Slot { body, order: None }];
    let good = serde_json::json!([{
        "ends": [[0, 0, 0], [10, 0, 0]],
        "normal_dot": 0.0,
        "position_abs_mm": 1e-9,
        "normal_dot_abs": 1e-9,
        "max_edge_tolerance_mm": 1.0,
        "max_gap_mm": 1e-9
    }]);
    grade_seams(&result, &good).unwrap();
    let mut reversed = good.clone();
    reversed[0]["ends"] = serde_json::json!([[10, 0, 0], [0, 0, 0]]);
    grade_seams(&result, &reversed).unwrap();
    for (field, value) in [
        ("ends", serde_json::json!([[0, 1, 0], [10, 1, 0]])),
        ("normal_dot", serde_json::json!(-0.5)),
        ("max_edge_tolerance_mm", serde_json::json!(0.0)),
        ("max_gap_mm", serde_json::json!(-1.0)),
        ("body", serde_json::json!(1)),
        ("position_abs_mm", serde_json::json!(-1.0)),
        ("normal_dot_abs", serde_json::json!("not a number")),
    ] {
        let mut wrong = good.clone();
        wrong[0][field] = value;
        assert!(grade_seams(&result, &wrong).is_err(), "accepted {wrong}");
    }
    for wrong in [
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!([good[0].clone(), good[0].clone()]),
    ] {
        assert!(grade_seams(&result, &wrong).is_err(), "accepted {wrong}");
    }
}

/// Inflated stored tolerances cannot hide a displaced support in the sampled gap check.
#[test]
fn seam_gaps_are_limited_independently_of_stored_tolerances() {
    use solidcore_record::brep::Surface;

    let body = solidcore_ops::make_box([0.0; 3], [10.0; 3]).unwrap();
    let (mut record, _) = body.to_record().unwrap();
    record.tolerance = 0.01;
    for face in &mut record.faces {
        match &mut face.surface {
            Surface::Plane { origin, .. } => {
                for x in origin {
                    *x += 1e-4;
                }
            }
            Surface::PlaneThrough {
                origin,
                u_point,
                v_point,
            } => {
                for point in [origin, u_point, v_point] {
                    for x in point {
                        *x += 1e-4;
                    }
                }
            }
            other => panic!("box support is not planar: {other:?}"),
        }
    }
    let (body, _) = Body::from_record(&record, true).unwrap();
    let result = [Slot {
        body: Arc::new(body),
        order: None,
    }];
    let loose = serde_json::json!([{
        "ends": [[0, 0, 0], [10, 0, 0]],
        "normal_dot": 0.0,
        "position_abs_mm": 1e-9,
        "normal_dot_abs": 1e-9,
        "max_edge_tolerance_mm": 0.01,
        "max_gap_mm": 0.01
    }]);
    grade_seams(&result, &loose).unwrap();
    let mut strict = loose;
    strict[0]["max_gap_mm"] = serde_json::json!(1e-9);
    assert!(grade_seams(&result, &strict).is_err());
}

#[test]
fn record_order_lookup_keeps_cached_selection_order_and_operand() {
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap());
    let (before, expected) = body.to_record().unwrap();
    let mut slot = Slot { body, order: None };
    let exported = ensure_order(&mut slot, "split_faces").unwrap();
    assert_eq!(exported.faces, expected.faces);
    assert_eq!(exported.edges, expected.edges);
    assert_eq!(exported.vertices, expected.vertices);
    slot.order.as_mut().unwrap().faces.rotate_left(2);
    let expected = slot.order.clone().unwrap();
    let cached = ensure_order_with(&mut slot, "pick", |_| {
        panic!("a cached record order must not be exported again")
    })
    .unwrap();
    assert_eq!(cached.faces, expected.faces);
    assert_eq!(cached.edges, expected.edges);
    assert_eq!(cached.vertices, expected.vertices);
    assert_eq!(slot.body.to_record().unwrap().0, before);
}

#[test]
fn record_order_lookup_distinguishes_unsupported_from_failed_preparation() {
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [10.0; 3]).unwrap());
    let before = body.to_record().unwrap().0;
    for error in [
        solidcore_ops::Error::Unsupported("unrepresentable result"),
        solidcore_ops::Error::Argument,
    ] {
        let unsupported = matches!(error, solidcore_ops::Error::Unsupported(_));
        let mut slot = Slot {
            body: body.clone(),
            order: None,
        };
        let result = ensure_order_with(&mut slot, "split_faces", |_| Err(error));
        match result {
            Err(Stop::Unsupported(why)) if unsupported => {
                assert!(why.contains("split_faces") && why.contains("unrepresentable result"));
            }
            Err(Stop::Fail(why)) if !unsupported => assert!(why.contains("split_faces")),
            other => panic!("wrong order-lookup classification: {other:?}"),
        }
        assert!(slot.order.is_none());
        assert_eq!(slot.body.to_record().unwrap().0, before);
    }
}

#[test]
fn chamfer_dispatch_preserves_order_direction_sizes_and_operand() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
fn blend_replay_keeps_the_record_edge_after_reordering_and_aliasing() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [20.0; 3]).unwrap());
    let (before, original) = body.to_record().unwrap();
    let selected = before
        .edges
        .iter()
        .position(|edge| {
            edge.vertices.iter().all(|&i| {
                let point = before.vertices[i as usize].point;
                point[1] == 0.0 && point[2] == 20.0
            })
        })
        .unwrap();
    let selected = original.edges[selected];
    for mode in 0..4 {
        let mut order = original.clone();
        match mode {
            1 => order.edges.rotate_left(3),
            2 => order.edges.insert(0, selected),
            _ => {}
        }
        // One record can name the same kernel edge more than once. Pick its last alias.
        let index = order
            .edges
            .iter()
            .rposition(|&edge| edge == selected)
            .unwrap();
        let input = Slot {
            body: Arc::clone(&body),
            order: (mode != 3).then_some(order),
        };
        let step = serde_json::json!({"op":"blend_dropping","params":{
            "kind":"round","sizes":[2.0],
            "edges":{"kind":"edge","picked":[{"index":index}]}
        }});
        let result = call(&step, &mut [input.clone()]).unwrap().unwrap();
        assert!(!Arc::ptr_eq(&result.body, &input.body));
        assert!(result.order.is_none());
        assert_eq!(result.body.counts().unwrap(), [7, 15, 10]);
        let expected = 8000.0 - 80.0 * (1.0 - std::f64::consts::FRAC_PI_4);
        assert!((result.body.volume(1e-9).unwrap() - expected).abs() / expected < 1e-9);
        let record = result.body.to_record().unwrap().0;
        for point in [[0., 2., 20.], [20., 2., 20.], [0., 0., 18.], [20., 0., 18.]] {
            assert!(
                record.vertices.iter().any(|v| v
                    .point
                    .iter()
                    .zip(point)
                    .all(|(a, b)| (a - b).abs() < 1e-9)),
                "mode {mode} rounded another edge: missing rail endpoint {point:?}"
            );
        }
        assert_eq!(input.body.to_record().unwrap().0, before);
    }
}

#[test]
fn repeated_blend_picks_keep_first_size_and_source() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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

/// A traced turn is read as the operation takes it: absent (a trace older than the turn) or
/// "mitre" the mitre, "swept" the swept turn, which SolidCraft refuses for a chamfer before the
/// kernel; any other value is bad replay data, never a refusal.
#[test]
fn blend_replay_reads_the_traced_turn() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [20.0; 3]).unwrap());
    let (record, order) = body.to_record().unwrap();
    let edge = record
        .edges
        .iter()
        .position(|e| {
            e.vertices.iter().all(|&i| {
                let p = record.vertices[i as usize].point;
                p[1] == 0.0 && p[2] == 20.0
            })
        })
        .unwrap();
    let input = Slot {
        body,
        order: Some(order),
    };
    let step = |kind: &str, turn: Option<Value>| {
        let mut params = serde_json::json!({
            "kind": kind, "sizes": [2.0], "edges": {"kind": "edge", "picked": [{"index": edge}]}
        });
        if let Some(turn) = turn {
            params["turn"] = turn;
        }
        serde_json::json!({"op": "blend_dropping", "params": params})
    };
    // one edge's round turns nowhere: each reading builds the same round
    let expected = 8000.0 - 80.0 * (1.0 - std::f64::consts::FRAC_PI_4);
    for turn in [None, Some("mitre"), Some("swept")] {
        let made = call(&step("round", turn.map(Value::from)), &mut [input.clone()])
            .unwrap()
            .unwrap();
        let v = made.body.volume(1e-9).unwrap();
        assert!((v - expected).abs() / expected < 1e-9, "{turn:?}: {v}");
    }
    let chamfer = call(&step("chamfer", Some("swept".into())), &mut [input.clone()]).unwrap();
    assert_eq!(
        chamfer.err(),
        Some(solidcore_ops::Error::Argument.to_string())
    );
    for bad in [Value::from("round"), Value::Null, Value::from(1)] {
        assert!(
            matches!(call(&step("round", Some(bad.clone())), &mut [input.clone()]),
                Err(Stop::Fail(why)) if why.contains("turn")),
            "{bad}"
        );
    }
}

fn handled_refusal_case() -> Value {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join("ladder/02-booleans/018-hole-tangent-to-side-face.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    assert_eq!(e["outcome"]["raw_counts"]["edges"], 21);
    run_case(&entry, &e).unwrap();
}

fn check_discarded_edit(record: Record, op: &str, params: &Value, made: &Value, retained: Value) {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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

/// The filled radius-5 corner with explicit setback8 already has a successful captured
/// result; the runner must replay it rather than skip its operation as unported.
#[test]
fn filled_setback_replays_the_captured_corner() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join("ladder/07-fillets/013-corner-setback.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    assert_eq!(e["rig"]["status"], "pass");
    assert!(e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["error"].is_null()));
    run_case(&entry, &e).unwrap();
}

fn filled_setback_input() -> (Value, Slot) {
    let entry = corpus().join("ladder/07-fillets/013-corner-setback.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    let record = serde_json::from_value(records(&entry, &e)["b2"].clone()).unwrap();
    let (body, order) = Body::from_record(&record, true).unwrap();
    let step = e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "blend_setback")
        .unwrap()
        .clone();
    (
        step,
        Slot {
            body: Arc::new(body),
            order: Some(order),
        },
    )
}

#[test]
fn filled_setback_maps_edges_and_vertices_independently_and_keeps_the_operand() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let (step, input) = filled_setback_input();
    let (before, _) = input.body.to_record().unwrap();
    let original = input.order.as_ref().unwrap();
    for permuted in [false, true] {
        let mut input = input.clone();
        let mut step = step.clone();
        if permuted {
            let mut order = original.clone();
            order.edges.rotate_left(5);
            order.vertices.reverse();
            for pick in step["params"]["edges"]["picked"].as_array_mut().unwrap() {
                let selected = original.edges[pick["index"].as_u64().unwrap() as usize];
                pick["index"] =
                    serde_json::json!(order.edges.iter().position(|&e| e == selected).unwrap());
            }
            let selected = original.vertices[3];
            step["params"]["corners"]["picked"][0]["index"] =
                serde_json::json!(order.vertices.iter().position(|&v| v == selected).unwrap());
            input.order = Some(order);
        }
        let output = call(&step, &mut [input.clone()]).unwrap().unwrap();
        assert!(!Arc::ptr_eq(&output.body, &input.body));
        assert!(output.order.is_none());
        let (record, _) = output.body.to_record().unwrap();
        assert_eq!(
            (
                record.faces.len(),
                record.edges.len(),
                record.vertices.len()
            ),
            (15, 36, 23)
        );
        let has = |point: [f64; 3]| {
            record
                .vertices
                .iter()
                .any(|v| v.point.iter().zip(point).all(|(a, b)| (a - b).abs() < 1e-9))
        };
        // The six rail ends are fixed by the radius and distance from the selected corner.
        for point in [
            [52., 35., 20.],
            [52., 40., 15.],
            [55., 32., 20.],
            [60., 32., 15.],
            [55., 40., 12.],
            [60., 35., 12.],
        ] {
            assert!(has(point), "missing setback rail {point:?}");
        }
        assert!(has([0., 0., 0.]));
        assert!(!has([60., 40., 20.]));
        assert_eq!(input.body.to_record().unwrap().0, before);
    }
}

#[test]
fn filled_setback_ordered_api_rejects_foreign_entities_without_editing() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let (_, input) = filled_setback_input();
    let before = input.body.to_record().unwrap().0;
    let original = input.order.as_ref().unwrap();
    for invalid in 0..4 {
        let mut order = original.clone();
        let mut edges = vec![3, 2, 4];
        let mut vertex = 3;
        match invalid {
            0 => order.edges[3] = solidcore::EdgeId::from_index(1_000_000),
            1 => order.vertices[3] = solidcore::VertexId::from_index(1_000_000),
            2 => edges[0] = order.edges.len(),
            _ => vertex = order.vertices.len(),
        }
        let r = blend_ordered(
            &input.body,
            &order,
            Blend::Round,
            &edges,
            &[5.0; 3],
            &[SetBack {
                vertex,
                distance: 8.0,
            }],
            Corners::Filled,
            Turn::Mitre,
        );
        assert!(
            matches!(r, Err(solidcore_ops::Error::Argument)),
            "{invalid}: {r:?}"
        );
        assert_eq!(input.body.to_record().unwrap().0, before);
    }
}

#[test]
fn filled_setback_duplicate_picks_keep_first_values_without_hiding_refusals() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let (step, input) = filled_setback_input();
    let expected = call(&step, &mut [input.clone()])
        .unwrap()
        .unwrap()
        .body
        .to_record()
        .unwrap()
        .0;
    let mut duplicate = step.clone();
    let edge = duplicate["params"]["edges"]["picked"][0].clone();
    duplicate["params"]["edges"]["picked"]
        .as_array_mut()
        .unwrap()
        .push(edge);
    duplicate["params"]["sizes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(2.0));
    let vertex = duplicate["params"]["corners"]["picked"][0].clone();
    duplicate["params"]["corners"]["picked"]
        .as_array_mut()
        .unwrap()
        .push(vertex);
    duplicate["params"]["setbacks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(4.0));
    let output = call(&duplicate, &mut [input.clone()]).unwrap().unwrap();
    assert_eq!(output.body.to_record().unwrap().0, expected);
    for key in ["sizes", "setbacks"] {
        let mut invalid = duplicate.clone();
        *invalid["params"][key]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap() = serde_json::json!(-1.0);
        assert!(call(&invalid, &mut [input.clone()]).unwrap().is_err());
    }
    for key in ["edges", "corners"] {
        let mut outside = step.clone();
        outside["params"][key]["picked"][0]["index"] = serde_json::json!(1_000_000);
        assert!(matches!(
            call(&outside, &mut [input.clone()]),
            Err(Stop::Fail(_))
        ));
        let mut missed = step.clone();
        missed["params"][key]["picked"][0] = serde_json::json!({"miss":"no entity near point"});
        assert!(matches!(
            call(&missed, &mut [input.clone()]),
            Err(Stop::Unsupported(_))
        ));
    }
    let mut no_room = step;
    no_room["params"]["setbacks"] = serde_json::json!([40.0]);
    assert!(call(&no_room, &mut [input]).unwrap().is_err());
}

/// The recorded operations already passed in SolidCraft; replay must reach their existing
/// kernel construction instead of skipping the variable-radius request.
#[test]
fn variable_fillet_replays_all_five_captured_laws() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let names = [
        "fillets/cov-fillet-variable-box-edge-1-to-5-on-its-cone",
        "ladder/07-fillets/005-variable-r5-to-r1",
        "ladder/07-fillets/006-variable-r5-to-zero",
        "ladder/07-fillets/007-variable-r5-to-1e-3",
        "ladder/07-fillets/012-variable-into-rounded-corner",
    ];
    let mut failures = Vec::new();
    for name in names {
        let entry = corpus().join(format!("{name}.json"));
        let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
        assert_eq!(e["rig"]["status"], "pass", "{name}");
        assert!(e["calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["error"].is_null()));
        match run_case(&entry, &e) {
            Ok(deviation) => println!("{name}: pass, relative volume deviation {deviation:?}"),
            Err(why) => failures.push(format!("{name}: {why:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn variable_fillet_step(edge: usize, start: [f64; 3]) -> Value {
    serde_json::json!({"op": "fillet_variable", "params": {
        "edge": {"kind": "edge", "picked": [{"index": edge}]},
        "start": start, "degree": 1,
        "knots": [0.0,0.0,0.25,0.75,1.0,1.0], "radii": [4.0,2.0,4.0,2.0]
    }})
}

#[test]
fn variable_fillet_replay_maps_the_copy_and_keeps_the_operand() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [20.0; 3]).unwrap());
    let (before, original) = body.to_record().unwrap();
    let edge = before
        .edges
        .iter()
        .position(|e| {
            e.vertices.iter().all(|&v| {
                let p = before.vertices[usize::try_from(v).unwrap()].point;
                p[1] == 20.0 && p[2] == 20.0
            })
        })
        .unwrap();
    let selected = original.edges[edge];
    for permuted in [false, true] {
        let mut order = original.clone();
        if permuted {
            order.edges.rotate_left(5);
            order.vertices.reverse();
        }
        let edge = order.edges.iter().position(|&e| e == selected).unwrap();
        let input = Slot {
            body: body.clone(),
            order: permuted.then_some(order),
        };
        for (point, from) in [([-10.0, 20.0, 20.0], 0.0), ([30.0, 20.0, 20.0], 20.0)] {
            let step = variable_fillet_step(edge, point);
            let output = call(&step, &mut [input.clone()]).unwrap().unwrap();
            assert!(!Arc::ptr_eq(&output.body, &body));
            assert!(output.order.is_none());
            assert_eq!(output.body.counts().unwrap()[0], 10);
            let (record, _) = output.body.to_record().unwrap();
            for want in [[from, 16.0, 20.0], [from, 20.0, 16.0]] {
                assert!(
                    record.vertices.iter().any(|v| v
                        .point
                        .iter()
                        .zip(want)
                        .all(|(a, b)| (a - b).abs() < 1e-9)),
                    "missing start rail {want:?}"
                );
            }
            assert_eq!(body.to_record().unwrap().0, before);
        }
    }
}

#[test]
fn variable_fillet_metadata_cannot_be_accepted_as_the_laws_refusal() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [20.0; 3]).unwrap());
    let (before, order) = body.to_record().unwrap();
    let input = Slot {
        body: body.clone(),
        order: Some(order),
    };
    let step = variable_fillet_step(0, [0.0; 3]);
    for parameter in ["edge", "start", "degree", "knots", "radii"] {
        let mut bad = step.clone();
        bad["params"].as_object_mut().unwrap().remove(parameter);
        assert!(
            matches!(call(&bad, &mut [input.clone()]), Err(Stop::Fail(_))),
            "{parameter}"
        );
    }
    for edge in [
        serde_json::json!({"kind": "face", "picked": [{"index":0}]}),
        serde_json::json!({"kind": "edge", "picked": []}),
        serde_json::json!({"kind": "edge", "picked": [{"index":0},{"index":1}]}),
        serde_json::json!({"kind": "edge", "picked": [{"index":999}]}),
        serde_json::json!({"kind": "edge", "picked": [{"index":0,"miss":"none"}]}),
    ] {
        let mut bad = step.clone();
        bad["params"]["edge"] = edge;
        assert!(matches!(
            call(&bad, &mut [input.clone()]),
            Err(Stop::Fail(_))
        ));
    }
    let mut missed = step.clone();
    missed["params"]["edge"]["picked"] = serde_json::json!([{"miss":"none"}]);
    assert!(
        matches!(call(&missed, &mut [input.clone()]), Err(Stop::Unsupported(why)) if why.contains("missed"))
    );
    for (key, value) in [
        ("degree", serde_json::json!(0)),
        ("radii", serde_json::json!([-1.0, 2.0])),
        ("knots", serde_json::json!([0, 1])),
        ("radii", serde_json::json!([])),
    ] {
        let mut refused = step.clone();
        refused["params"][key] = value;
        assert!(
            matches!(call(&refused, &mut [input.clone()]), Ok(Err(_))),
            "{key}"
        );
    }
    assert_eq!(body.to_record().unwrap().0, before);
}

fn sweep_replay_fixture(name: &str, op: &str) -> (PathBuf, Value, Value) {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join(format!("{name}.json"));
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    let step = e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == op)
        .unwrap()
        .clone();
    (entry, e, step)
}

#[test]
fn sweep_replay_runs_all_eleven_captured_paths_and_coils() {
    let names = [
        ("ladder/04-spline-profiles/003-pipe-along-bezier", "sweep"),
        (
            "ladder/04-spline-profiles/004-sweep-square-along-bezier",
            "sweep",
        ),
        ("surfaces/cov-sweep-line-along-a-line-plane", "sweep"),
        (
            "sweeps/1269-sweep-parallel-orientation-oblique-cylinder",
            "sweep",
        ),
        ("sweeps/2820-pipe-hollow-with-a-wall-along-a-bend", "sweep"),
        (
            "sweeps/3345-sweep-profile-mid-line-round-a-rounded-rectangle",
            "sweep",
        ),
        (
            "sweeps/3862-coil-square-wire-outside-the-diameter",
            "screw_sweep",
        ),
        (
            "sweeps/5323-sweep-circle-along-polyline-turning-in-two-planes",
            "sweep",
        ),
        (
            "sweeps/5324-sweep-square-round-closed-path-from-mid-edge",
            "sweep",
        ),
        (
            "sweeps/757-surface-sweep-slanted-line-round-a-quarter-arc",
            "sweep",
        ),
        ("sweeps/96-sweep-rectangle-round-a-mitred-corner", "sweep"),
    ];
    let mut failures = Vec::new();
    for (name, op) in names {
        let (entry, e, _) = sweep_replay_fixture(name, op);
        assert_eq!(e["rig"]["status"], "pass", "{name}");
        assert!(e["calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["error"].is_null()));
        match run_case(&entry, &e) {
            Ok(deviation) => println!("{name}: pass, relative volume deviation {deviation:?}"),
            Err(why) => failures.push(format!("{name}: {why:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn sweep_replay_keeps_follow_and_translate_frames_distinct() {
    let (_, _, mut step) = sweep_replay_fixture(
        "sweeps/757-surface-sweep-slanted-line-round-a-quarter-arc",
        "sweep",
    );
    for follow in [true, false] {
        step["params"]["frame"] = serde_json::json!(if follow { "follow" } else { "translate" });
        let output = call(&step, &mut []).unwrap().unwrap();
        assert!(output.order.is_none());
        assert_eq!(output.body.counts().unwrap(), [1, 4, 4]);
        assert_eq!(output.body.volume(1e-9).unwrap(), 0.0);
        let record = output.body.to_record().unwrap().0;
        let ends = [
            [20.0, 0.0, 0.0],
            [30.0, 0.0, 10.0],
            [0.0, 20.0, 0.0],
            if follow {
                [0.0, 30.0, 10.0]
            } else {
                [10.0, 20.0, 10.0]
            },
        ];
        for want in ends {
            assert!(
                record.vertices.iter().any(|v| v
                    .point
                    .iter()
                    .zip(want)
                    .all(|(a, b)| (a - b).abs() < 1e-9)),
                "frame follow={follow}, missing {want:?}"
            );
        }
        if follow {
            let area = 125.0 * std::f64::consts::SQRT_2 * std::f64::consts::PI;
            assert!((output.body.area(1e-9).unwrap() - area).abs() <= area * 1e-9);
        }
    }
}

#[test]
fn sweep_replay_forwards_oblique_path_and_caps() {
    let (_, _, mut step) = sweep_replay_fixture(
        "sweeps/1269-sweep-parallel-orientation-oblique-cylinder",
        "sweep",
    );
    for (x, z) in [(20.0, 20.0), (8.0, 12.0)] {
        step["params"]["path"] = serde_json::json!([{"line": {"start": [0,0,0], "end": [x,0,z]}}]);
        for cap in [true, false] {
            step["params"]["cap"] = serde_json::json!(cap);
            let output = call(&step, &mut []).unwrap().unwrap();
            assert_eq!(output.body.counts().unwrap()[0], if cap { 3 } else { 1 });
            let want = if cap {
                25.0 * std::f64::consts::PI * z
            } else {
                0.0
            };
            let got = output.body.volume(1e-9).unwrap();
            assert!(
                (got - want).abs() <= want * 1e-9,
                "{x}, {z}, cap={cap}: {got}"
            );
            let (lo, hi) = output.body.bounds().unwrap();
            for (a, b) in lo
                .into_iter()
                .zip([-5.0, -5.0, 0.0])
                .chain(hi.into_iter().zip([x + 5.0, 5.0, z]))
            {
                assert!((a - b).abs() < 1e-8, "bound {a} != {b}");
            }
        }
    }
}

fn sweep_replay_screw_step() -> Value {
    let corners = [[27, -11, 1], [31, -11, 1], [31, -11, 5], [27, -11, 5]];
    let profile: Vec<_> = (0..4)
        .map(|i| serde_json::json!({"line": {"start": corners[i], "end": corners[(i+1)%4]}}))
        .collect();
    serde_json::json!({"op": "screw_sweep", "inputs": [], "params": {
        "profile": profile, "origin": [7,-11,3], "direction": [0,0,1],
        "pitch": 8, "turns": 1, "tolerance": 1e-4
    }})
}

/// For a radial/axial rectangle the volume is integral r dr dz dtheta, independent of
/// the axial lift: area 16 times centroid radius (22 + radial offset) times total angle.
#[test]
fn sweep_replay_screw_keeps_axis_pitch_turns_origin_and_radial_law() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    for (sign, pitch, turns, offset) in [
        (1.0, 8.0, 1.0, 0.0),
        (-1.0, 8.0, 0.5, 0.0),
        (1.0, -8.0, 1.0, 3.0),
        (-1.0, -8.0, 0.5, 3.0),
    ] {
        let mut step = sweep_replay_screw_step();
        step["params"]["direction"] = serde_json::json!([0, 0, sign]);
        step["params"]["pitch"] = serde_json::json!(pitch);
        step["params"]["turns"] = serde_json::json!(turns);
        if offset != 0.0 {
            step["params"]["law"] = serde_json::json!([[0, offset], [turns, offset]]);
        }
        let output = call(&step, &mut []).unwrap().unwrap();
        assert!(output.order.is_none());
        let want = 32.0 * (22.0 + offset) * std::f64::consts::PI * turns;
        let got = output.body.volume(1e-9).unwrap();
        assert!(
            (got - want).abs() <= want * 1e-9,
            "sign={sign}, pitch={pitch}, turns={turns}, offset={offset}: {got} != {want}"
        );
        let rise: f64 = sign * pitch * turns;
        let (lo, hi) = output.body.bounds().unwrap();
        assert!((lo[2] - (1.0 + rise.min(0.0))).abs() < 1e-8, "{lo:?}");
        assert!((hi[2] - (5.0 + rise.max(0.0))).abs() < 1e-8, "{hi:?}");
    }
}

#[test]
fn sweep_replay_metadata_does_not_satisfy_a_refusal() {
    let (_, _, path) = sweep_replay_fixture("surfaces/cov-sweep-line-along-a-line-plane", "sweep");
    let screw = sweep_replay_screw_step();
    for (step, fields) in [
        (&path, &["profile", "path", "frame", "cap"][..]),
        (
            &screw,
            &[
                "profile",
                "origin",
                "direction",
                "pitch",
                "turns",
                "tolerance",
            ][..],
        ),
    ] {
        for field in fields {
            let mut bad = step.clone();
            bad["params"].as_object_mut().unwrap().remove(*field);
            assert!(matches!(call(&bad, &mut []), Err(Stop::Fail(_))), "{field}");
        }
    }
    let mut unknown = path.clone();
    unknown["params"]["frame"] = serde_json::json!("future_frame");
    assert!(
        matches!(call(&unknown, &mut []), Err(Stop::Unsupported(why)) if why.contains("future_frame"))
    );
    let mut malformed = path.clone();
    malformed["params"]["path"] = serde_json::json!([{"unknown_curve": {}}]);
    assert!(matches!(call(&malformed, &mut []), Err(Stop::Fail(_))));
    for law in [
        Value::Null,
        serde_json::json!([[0, 1, 2]]),
        serde_json::json!("bad"),
    ] {
        let mut bad = screw.clone();
        bad["params"]["law"] = law;
        assert!(matches!(call(&bad, &mut []), Err(Stop::Fail(_))));
    }
    let mut empty_path = path;
    empty_path["params"]["path"] = serde_json::json!([]);
    assert!(matches!(call(&empty_path, &mut []), Ok(Err(_))));
    for (field, value) in [
        ("pitch", serde_json::json!(0)),
        ("direction", serde_json::json!([0, 0, 0])),
        ("tolerance", serde_json::json!(0)),
        ("law", serde_json::json!([])),
        ("law", serde_json::json!([[0.5, 0], [0.2, 0]])),
    ] {
        let mut refused = screw.clone();
        refused["params"][field] = value;
        assert!(matches!(call(&refused, &mut []), Ok(Err(_))), "{field}");
    }
}

fn station_sweep_step() -> Value {
    serde_json::json!({"op": "sweep_stations", "inputs": [], "params": {
        "profile": [{"arc": {"center": [0,0,0], "axis": [0,0,1],
            "reference": [1,0,0], "radius": 10, "start": 0,
            "end": std::f64::consts::TAU}}],
        "stations": [
            {"m": [[1,0,0],[0,1,0],[0,0,1]], "t": [7,-11,3]},
            {"m": [[0.5,0,0],[0,0.5,0],[0,0,1]], "t": [7,-11,23]}
        ], "ends": "capped"
    }})
}

#[test]
fn station_sweep_replays_all_five_captured_cases() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let names = [
        "surfaces/cov-sweep-circle-scaled-half-is-a-frustum",
        "sweeps/771-sweep-end-scale-cone-frustum",
        "sweeps/771-sweep-one-guide-rail-cone-frustum",
        "sweeps/1269-sweep-twisted-square-ninety-degrees",
        "sweeps/2005-closed-sweep-twisted-new-body",
    ];
    let mut failures = Vec::new();
    for name in names {
        let entry = corpus().join(format!("{name}.json"));
        let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
        assert_eq!(e["rig"]["status"], "pass", "{name}");
        assert!(e["calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["error"].is_null()));
        if let Err(why) = run_case(&entry, &e) {
            failures.push(format!("{name}: {why:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[allow(clippy::disallowed_methods)] // Independent closed-form oracle.
fn station_sweep_forwards_scale_translation_and_end_caps() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let volume = 3500.0 * std::f64::consts::PI / 3.0;
    let wall = 15.0 * std::f64::consts::PI * 425.0_f64.sqrt();
    for capped in [true, false] {
        let mut step = station_sweep_step();
        step["params"]["ends"] = serde_json::json!(if capped { "capped" } else { "open" });
        let output = call(&step, &mut []).unwrap().unwrap();
        assert!(output.order.is_none());
        assert_eq!(output.body.counts().unwrap()[0], if capped { 3 } else { 1 });
        let got = output.body.volume(1e-9).unwrap();
        if capped {
            assert!((got - volume).abs() <= volume * 1e-9, "{got}");
        } else {
            assert_eq!(got, 0.0);
        }
        let want_area = wall
            + if capped {
                125.0 * std::f64::consts::PI
            } else {
                0.0
            };
        let area = output.body.area(1e-9).unwrap();
        assert!((area - want_area).abs() <= want_area * 1e-9, "{area}");
    }
}

#[test]
fn station_sweep_keeps_affine_axis_images_in_order() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let mut step = station_sweep_step();
    let corners = [[0, 0, 0], [2, 0, 0], [2, 3, 0], [0, 3, 0]];
    step["params"]["profile"] = Value::Array(
        (0..4)
            .map(|i| serde_json::json!({"line": {"start": corners[i], "end": corners[(i+1)%4]}}))
            .collect(),
    );
    for station in step["params"]["stations"].as_array_mut().unwrap() {
        station["m"] = serde_json::json!([[0, 2, 0], [-3, 0, 0], [0, 0, 1]]);
    }
    let output = call(&step, &mut []).unwrap().unwrap();
    assert_eq!(output.body.counts().unwrap(), [6, 12, 8]);
    assert!((output.body.volume(1e-9).unwrap() - 720.0).abs() <= 720.0e-9);
    assert!((output.body.area(1e-9).unwrap() - 592.0).abs() <= 592.0e-9);
    let record = output.body.to_record().unwrap().0;
    let points: Vec<_> = record.vertices.iter().map(|v| v.point).collect();
    for (axis, low, high) in [(0, -2.0, 7.0), (1, -11.0, -7.0), (2, 3.0, 23.0)] {
        let min = points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
        let max = points
            .iter()
            .map(|p| p[axis])
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((min - low).abs() <= 1e-9, "axis {axis}: {min}");
        assert!((max - high).abs() <= 1e-9, "axis {axis}: {max}");
    }
}

#[test]
fn station_sweep_metadata_is_distinct_from_operation_refusal() {
    let step = station_sweep_step();
    for parameter in ["profile", "stations", "ends"] {
        let mut bad = step.clone();
        bad["params"].as_object_mut().unwrap().remove(parameter);
        assert!(
            matches!(call(&bad, &mut []), Err(Stop::Fail(_))),
            "{parameter}"
        );
    }
    for matrix in [
        serde_json::json!([[1, 0, 0], [0, 1, 0]]),
        serde_json::json!([[1, 0], [0, 1, 0], [0, 0, 1]]),
        serde_json::json!([[1, 0, 0], [0, 1, 0], [0, 0, 1], [0, 0, 0]]),
    ] {
        let mut bad = step.clone();
        bad["params"]["stations"][0]["m"] = matrix;
        assert!(matches!(call(&bad, &mut []), Err(Stop::Fail(_))));
    }
    for value in [
        serde_json::json!([1, 2]),
        serde_json::json!([1, 2, 3, 4]),
        Value::Null,
    ] {
        let mut bad = step.clone();
        bad["params"]["stations"][0]["t"] = value;
        assert!(matches!(call(&bad, &mut []), Err(Stop::Fail(_))));
    }
    let mut unknown = step.clone();
    unknown["params"]["ends"] = serde_json::json!("future_end");
    assert!(
        matches!(call(&unknown, &mut []), Err(Stop::Unsupported(why)) if why.contains("future_end"))
    );
    let mut refused = step.clone();
    refused["params"]["stations"].as_array_mut().unwrap().pop();
    assert!(matches!(call(&refused, &mut []), Ok(Err(_))));
    let mut singular = step;
    singular["params"]["stations"][0]["m"] = serde_json::json!([[0, 0, 0], [0, 1, 0], [0, 0, 1]]);
    assert!(matches!(call(&singular, &mut []), Ok(Err(_))));
}

fn loft_step(name: &str) -> (PathBuf, Value, Value) {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
fn union_all_replays_an_odd_operand_list_without_consuming_inputs() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let mut inputs: Vec<Slot> = (0..3)
        .map(|x| Slot {
            body: Arc::new(solidcore_ops::make_box([f64::from(x), 0.0, 0.0], [2.0; 3]).unwrap()),
            order: None,
        })
        .collect();
    let before: Vec<_> = inputs
        .iter()
        .map(|s| s.body.to_record().unwrap().0)
        .collect();
    let step = serde_json::json!({"op": "union_all", "params": {}});
    let output = call(&step, &mut inputs).unwrap().unwrap();
    assert!((output.body.volume(1e-9).unwrap() - 16.0).abs() <= 16.0e-9);
    for (input, record) in inputs.iter().zip(before) {
        assert_eq!(input.body.to_record().unwrap().0, record);
        assert!((input.body.volume(1e-9).unwrap() - 8.0).abs() <= 8.0e-9);
    }
}

/// Replay every captured union-all trace against its recorded geometric expectations.
#[test]
#[ignore = "the 17 captured unions are slow; also covered by the full kernel rig"]
fn union_all_replays_captured_cases() {
    let fixes: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("fixes.json")).unwrap())
            .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
    let mut files = Vec::new();
    entries(&corpus(), &mut files);
    files.sort();
    let listed = known_failing();
    let mut checked = 0;
    for entry in files {
        let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
        if !e["calls"].as_array().is_some_and(|calls| {
            calls
                .iter()
                .any(|call| call["op"].as_str() == Some("union_all"))
        }) {
            continue;
        }
        let result = grade(&entry, &listed, Duration::from_secs(180));
        assert_eq!(
            result.kind,
            Kind::Pass,
            "{}: {}",
            entry.display(),
            result.note
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "the union-all capture corpus must remain covered"
    );
}

/// The four turn cases replay their traced `turn`: the boss's base rounds and the pocket's
/// floor rounds turn at the sharp upright edges mitred (the default) or swept, and the swept
/// turn at the pocket's sharp concave corners is refused where the rig's was. Graded as the
/// rig grades them (the volume to `expected.tolerance`, the records' counts), with its fixes.
#[test]
#[ignore = "replays corpus cases with the rig's fixes; run with the kernel rig (--ignored)"]
fn turn_cases_replay_their_traced_turn() {
    let fixes: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("fixes.json")).unwrap())
            .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
    let listed = known_failing();
    for (case, refused) in [
        ("cov-fillet-turn-mitre-boss-base-r1", false),
        ("cov-fillet-turn-swept-boss-base-r1", false),
        ("cov-fillet-turn-mitre-pocket-floor-corners-r1", false),
        ("cov-fillet-turn-swept-pocket-sharp-corner-refused-r1", true),
    ] {
        let entry = corpus().join(format!("fillets/{case}.json"));
        let graded = grade(&entry, &listed, Duration::from_secs(180));
        assert_eq!(graded.kind, Kind::Pass, "{case}: {}", graded.note);
        // a built case is graded on its volume; a refused one has none
        assert_eq!(graded.deviation.is_none(), refused, "{case}");
    }
}

#[test]
fn union_all_preparation_failure_is_not_an_operation_refusal() {
    let input = Slot {
        body: Arc::new(solidcore_ops::make_box([0.0; 3], [1.0; 3]).unwrap()),
        order: None,
    };
    let step = serde_json::json!({"op": "union_all", "params": {}, "error": "refused"});
    let copies = std::cell::Cell::new(0);
    let result = call_with_copy(&step, &mut [input.clone(), input], |slot| {
        copies.set(copies.get() + 1);
        if copies.get() == 2 {
            Ok(Err("second operand copy failed".to_owned()))
        } else {
            slot.copied()
        }
    });
    assert!(matches!(result, Err(Stop::Fail(why)) if why.contains("second operand copy failed")));
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
        "merge_faces",
        "extend_sheet",
        "fillet_variable",
        "blend_setback",
        "draft",
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
        serde_json::json!({"op": "extend_sheet", "params": {
            "edge": {"kind": "edge", "picked": [{"index": 0}]},
            "distance": 1, "how": "natural"
        }}),
        serde_json::json!({"op": "fillet_variable", "params": {}}),
        serde_json::json!({"op": "blend_setback", "params": {}}),
        serde_json::json!({"op": "merge_faces", "params": {
            "keep": {"kind": "edge", "picked": []}
        }}),
        serde_json::json!({"op": "draft", "params": {}}),
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join("booleans/3830-split-cup-by-extended-spline-surface.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    assert_eq!(e["result"].as_array().unwrap().len(), 3);
    assert!(run_case(&entry, &e).unwrap().is_some());
}

/// The corpus's stitch of two 10 x 10 sheets that share the edge x = 10, and its operand.
fn two_squares() -> (PathBuf, Value, Slot) {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
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

/// The captured two-face draft, with its operand's original record order.
fn draft_case() -> (PathBuf, Value, Value, Slot) {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join("booleans/cov-draft-two-adjacent-faces-meet-at-a-corner.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    let recs = records(&entry, &e);
    let record: Record = serde_json::from_value(recs["b2"].clone()).unwrap();
    let (body, order) = Body::from_record(&record, true).unwrap();
    let step = e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "draft")
        .unwrap()
        .clone();
    (
        entry,
        e,
        step,
        Slot {
            body: Arc::new(body),
            order: Some(order),
        },
    )
}

#[test]
#[allow(clippy::disallowed_methods)] // Independent closed-form oracle, not ops geometry.
fn draft_replay_preserves_faces_order_parameters_and_operand() {
    let (entry, _, step, input) = draft_case();
    let before = serde_json::to_value(input.body.to_record().unwrap().0).unwrap();
    for (rotate, neutral_z, sign) in [
        (false, 0.0, 1.0),
        (true, 0.0, 1.0),
        (true, 10.0, 1.0),
        (false, 0.0, -1.0),
    ] {
        let angle = sign * std::f64::consts::PI / 18.0;
        let tangent = angle.tan();
        let volume = 24000.0 - 70.0 * tangent * (200.0 - 20.0 * neutral_z)
            + tangent * tangent * (8000.0 / 3.0 - 400.0 * neutral_z + 20.0 * neutral_z * neutral_z);
        let mut input = input.clone();
        let mut step = step.clone();
        step["params"]["angle"] = serde_json::json!(angle);
        step["params"]["neutral_origin"] = serde_json::json!([0.0, 0.0, neutral_z]);
        if rotate {
            let order = input.order.as_mut().unwrap();
            let front = order.faces[3];
            let right = order.faces[4];
            order.faces.rotate_left(2);
            let picks: Vec<_> = [front, right].iter().map(|id| {
                serde_json::json!({"index": order.faces.iter().position(|face| face == id).unwrap()})
            }).collect();
            step["params"]["faces"]["picked"] = serde_json::json!(picks);
        }
        let out = call(&step, &mut [input.clone()]).unwrap().unwrap();
        assert_eq!(out.body.counts().unwrap(), [6, 12, 8]);
        let got = out.body.volume(VOLUME_ACCURACY).unwrap();
        assert!((got - volume).abs() <= volume * 1e-9, "{got} vs {volume}");
        let record = out.body.to_record().unwrap().0;
        let top: Vec<_> = record
            .vertices
            .iter()
            .filter(|v| (v.point[2] - 20.0).abs() < 1e-9)
            .collect();
        assert_eq!(top.len(), 4);
        let min_y = top.iter().map(|v| v.point[1]).fold(f64::INFINITY, f64::min);
        let max_x = top
            .iter()
            .map(|v| v.point[0])
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (min_y - (20.0 - neutral_z) * tangent).abs() < 1e-8,
            "front at {min_y}"
        );
        assert!(
            (max_x - (40.0 - (20.0 - neutral_z) * tangent)).abs() < 1e-8,
            "right at {max_x}"
        );
        assert_eq!(
            serde_json::to_value(input.body.to_record().unwrap().0).unwrap(),
            before
        );
    }
    let graded = grade(&entry, &BTreeMap::new(), Duration::from_secs(180));
    assert_eq!(graded.kind, Kind::Pass, "{}", graded.note);
}

#[test]
fn draft_replay_keeps_malformed_picks_and_parameters_out_of_refusals() {
    let (entry, e, step, input) = draft_case();
    for key in ["angle", "neutral_origin", "neutral_normal", "pull", "faces"] {
        let mut bad = step.clone();
        bad["params"].as_object_mut().unwrap().remove(key);
        assert!(
            matches!(call(&bad, &mut [input.clone()]), Err(Stop::Fail(_))),
            "missing {key}"
        );
    }
    for pick in [
        serde_json::json!({"kind": "face", "picked": [{"index": 6}]}),
        serde_json::json!({"kind": "face", "picked": [{"index": -1}]}),
        serde_json::json!({"kind": "face"}),
        serde_json::json!({"kind": "edge", "picked": [{"index": 0}]}),
    ] {
        let mut bad = step.clone();
        bad["params"]["faces"] = pick;
        assert!(matches!(
            call(&bad, &mut [input.clone()]),
            Err(Stop::Fail(_))
        ));
    }
    let mut refused = step.clone();
    refused["params"]["angle"] = serde_json::json!(0.0);
    assert!(matches!(call(&refused, &mut [input]), Ok(Err(_))));
    let mut bad = e.clone();
    let last = bad["calls"].as_array_mut().unwrap().last_mut().unwrap();
    last["params"]["faces"]["picked"][0]["index"] = serde_json::json!(6);
    last["error"] = serde_json::json!("expected operation refusal");
    assert!(matches!(
        run_case_with_records(&bad, || records(&entry, &e)),
        Err(Stop::Fail(_))
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
    let fails = listed.get(&key).and_then(|l| l.fails.clone());
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
    // the case runs on its own thread, with the fixes of the test that grades it
    let fixes = session::fixes();
    std::thread::spawn(move || {
        session::set_thread_fixes(fixes).unwrap();
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
            if let Some(f) = &fails {
                // listed for a failure, and it now gives the rig's result: off the list
                g.kind = Kind::KnownFailing;
                g.moved = true;
                g.note = format!("gives the rig's result now (listed failing with: {f})");
            } else if is_listed {
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
            // steady only when it fails exactly as it was listed for
            g.moved = is_listed && fails.as_deref() != Some(w.as_str());
            g.note = w;
        }
    }
    g
}

/// Changing the plane position moves the new edge, and a reordered face pick still cuts
/// the top; the original body record and its volume/area are unchanged.
#[test]
fn split_faces_forwards_the_tool_and_record_face_order() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap());
    let (before, original_order) = body.to_record().unwrap();
    let top = before
        .faces
        .iter()
        .position(|f| {
            matches!(f.surface,
        RecordSurface::Plane { origin, .. } if origin[2] == 30.0)
        })
        .unwrap();
    let selected = original_order.faces[top];
    for mode in 0..3 {
        let mut order = original_order.clone();
        if mode == 1 {
            order.faces.rotate_left(2);
        }
        let pick = order.faces.iter().position(|&f| f == selected).unwrap();
        for at in [3.0, 7.0] {
            let mut ins = [Slot {
                body: Arc::clone(&body),
                order: (mode != 2).then_some(order.clone()),
            }];
            let step = serde_json::json!({"op": "split_faces", "params": {
                "faces": [pick], "tools": [{"plane": {"origin": [at, 0, 0],
                    "xaxis": [0, 1, 0], "yaxis": [0, 0, 1], "u": [-1, 21], "v": [-1, 31]}}]
            }});
            let output = call(&step, &mut ins).unwrap().unwrap();
            assert!(output.order.is_none());
            assert_eq!(output.body.counts().unwrap(), [7, 15, 10]);
            assert!((output.body.volume(1e-9).unwrap() - 6000.0).abs() < 1e-6);
            assert!((output.body.area(1e-9).unwrap() - 2200.0).abs() < 1e-6);
            let result = output.body.to_record().unwrap().0;
            let added: Vec<_> = result
                .vertices
                .iter()
                .filter(|v| v.point[0] > 1e-7 && v.point[0] < 10.0 - 1e-7)
                .collect();
            assert_eq!(added.len(), 2);
            for v in added {
                assert!((v.point[0] - at).abs() < 1e-7);
                assert!((v.point[2] - 30.0).abs() < 1e-7);
            }
            assert_eq!(body.to_record().unwrap().0, before);
        }
    }
}

/// Distinct physical faces and finite tool domains change the observed cut, not just picks.
#[test]
fn split_faces_distinguishes_faces_and_finite_tool_extents() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap());
    let (before, order) = body.to_record().unwrap();
    for (face_axis, face_position, cut_axis, cut_position, origin, xaxis, yaxis, domain) in [
        (
            2,
            30.0,
            0,
            3.0,
            [3, 0, 0],
            [0, 1, 0],
            [0, 0, 1],
            [[-1, 21], [-1, 31]],
        ),
        (
            0,
            10.0,
            2,
            12.0,
            [0, 0, 12],
            [1, 0, 0],
            [0, 1, 0],
            [[-1, 11], [-1, 21]],
        ),
    ] {
        let pick = before
            .faces
            .iter()
            .position(|face| {
                matches!(face.surface,
            RecordSurface::Plane {origin,u_axis,v_axis}
                if origin[face_axis] == face_position
                && u_axis[face_axis] == 0.0 && v_axis[face_axis] == 0.0)
            })
            .unwrap();
        for bounds in [domain, [[-100, 100], [-100, 100]]] {
            let step = serde_json::json!({"op":"split_faces","params":{
                "faces":[pick],"tools":[{"plane":{"origin":origin,"xaxis":xaxis,
                    "yaxis":yaxis,"u":bounds[0],"v":bounds[1]}}]
            }});
            let out = call(
                &step,
                &mut [Slot {
                    body: Arc::clone(&body),
                    order: Some(order.clone()),
                }],
            )
            .unwrap()
            .unwrap();
            assert_eq!(out.body.counts().unwrap(), [7, 15, 10]);
            assert!((out.body.volume(1e-9).unwrap() - 6000.0).abs() < 1e-6);
            assert!((out.body.area(1e-9).unwrap() - 2200.0).abs() < 1e-6);
            let result = out.body.to_record().unwrap().0;
            let added: Vec<_> = result
                .vertices
                .iter()
                .filter(|v| (v.point[cut_axis] - cut_position).abs() < 1e-7)
                .collect();
            assert_eq!(added.len(), 2);
            for v in added {
                assert!((v.point[face_axis] - face_position).abs() < 1e-7);
            }
            assert_eq!(body.to_record().unwrap().0, before);
        }
        // The same infinite plane with a domain entirely outside the box cannot cut it.
        let miss = serde_json::json!({"op":"split_faces","params":{
            "faces":[pick],"tools":[{"plane":{"origin":origin,"xaxis":xaxis,
                "yaxis":yaxis,"u":[-10,-5],"v":[-10,-5]}}]
        }});
        assert!(
            matches!(call(&miss,&mut [Slot {body:Arc::clone(&body),order:Some(order.clone())}]),
            Ok(Err(why)) if why == "tools do not split any selected face")
        );
        assert_eq!(body.to_record().unwrap().0, before);
    }
}

/// Malformed metadata cannot be counted as unsupported coverage; a named unknown kind can.
#[test]
fn split_faces_bad_tool_metadata_is_a_replay_failure() {
    for tool in [
        serde_json::json!(null),
        serde_json::json!([]),
        serde_json::json!(7),
        serde_json::json!("plane"),
        serde_json::json!({}),
        serde_json::json!({"plane": null}),
        serde_json::json!({"cylinder": null}),
        serde_json::json!({"plane": {}, "cylinder": {}}),
    ] {
        assert!(matches!(split_tool(&tool), Err(Stop::Fail(_))), "{tool}");
    }
    assert!(matches!(split_tool(&serde_json::json!({"sphere": {}})),
        Err(Stop::Unsupported(why)) if why.contains("sphere")));
}

/// Non-default cylindrical frame, radius, angular subdomain and height survive trace parsing.
#[test]
fn split_faces_cylinder_parameters_keep_their_record_units() {
    let tool = split_tool(&serde_json::json!({"cylinder": {
        "origin": [1, 2, 3], "xaxis": [0, 1, 0], "yaxis": [-1, 0, 0],
        "radius": 7, "angle_deg": [-180, 90], "height": [-2, 8]
    }}))
    .unwrap();
    assert_eq!(
        tool,
        Tool {
            surface: RecordSurface::Cylinder {
                frame: Frame {
                    origin: [1.0, 2.0, 3.0],
                    x_axis: [0.0, 1.0, 0.0],
                    y_axis: [-1.0, 0.0, 0.0],
                },
                radius: 7.0
            },
            domain: [
                [-std::f64::consts::PI, std::f64::consts::FRAC_PI_2],
                [-2.0, 8.0]
            ],
        }
    );
}

/// Replays one captured face split through the same loading and grading path as the corpus.
fn replay_face_split_case(name: &str) {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join(format!("booleans/{name}.json"));
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    assert!(run_case(&entry, &e).unwrap().is_some());
}

/// The circular sketch splits the top face and preserves the block's volume and area.
#[test]
fn split_faces_replays_the_sketch_circle() {
    replay_face_split_case("cov-split-face-by-a-sketch-circle");
}

/// The cylinder's surface splits the block's face while the cylinder remains a final body.
#[test]
fn split_faces_replays_another_bodys_face() {
    replay_face_split_case("cov-split-face-by-another-bodys-face");
}

/// A plane's normal orders two unequal parts; both names must bind distinct bodies.
#[test]
fn split_body_keeps_distinct_parts_and_preserves_input() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [10.0; 3]).unwrap());
    let (before, order) = body.to_record().unwrap();
    for reversed in [false, true] {
        let z = if reversed { -1.0 } else { 1.0 };
        let step = serde_json::json!({"op": "split_body", "params": {"tool": {
            "plane": {"origin": [3,0,0], "xaxis": [0,1,0], "yaxis": [0,0,z],
                "u": [-1,11], "v": [-11,11]}
        }}});
        let parts = call_outputs(
            &step,
            &mut [Slot {
                body: Arc::clone(&body),
                order: Some(order.clone()),
            }],
        )
        .unwrap()
        .unwrap();
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
        assert_eq!(body.to_record().unwrap().0, before);
        for (name, volume) in ["back", "front"].into_iter().zip(want) {
            let mut traced = step.clone();
            traced["inputs"] = serde_json::json!(["solid"]);
            traced["outputs"] = serde_json::json!(["back", "front"]);
            let entry = serde_json::json!({
                "bodies": {"solid": {"trusted": true}}, "calls": [traced],
                "result": [{"made": name}],
                "expected": {"body_count":1,"volume_mm3":volume,"faces":6,
                    "edges":12,"vertices":8,"tolerance":{"rel":1e-9}}
            });
            let records = serde_json::json!({"solid":before})
                .as_object()
                .unwrap()
                .clone();
            assert!(run_case_with_records(&entry, || records.clone())
                .unwrap()
                .is_some());
        }
    }
}

/// Keeping the new split edge prevents merging only that seam, even after record reordering.
#[test]
fn merge_faces_keeps_record_edges_and_preserves_input() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap());
    let (record, order) = body.to_record().unwrap();
    let top = record
        .faces
        .iter()
        .position(|f| {
            matches!(f.surface,
        RecordSurface::Plane {origin,..} if origin[2]==30.0)
        })
        .unwrap();
    let split = serde_json::json!({"op":"split_faces","params":{
        "faces":[top],"tools":[{"plane":{"origin":[3,0,0],"xaxis":[0,1,0],
            "yaxis":[0,0,1],"u":[-1,21],"v":[-1,31]}}]
    }});
    let split = call(
        &split,
        &mut [Slot {
            body,
            order: Some(order),
        }],
    )
    .unwrap()
    .unwrap();
    let (before, original_order) = split.body.to_record().unwrap();
    let seam = before
        .edges
        .iter()
        .position(|edge| {
            edge.vertices.iter().all(|&i| {
                let p = before.vertices[i as usize].point;
                (p[0] - 3.0).abs() < 1e-7 && (p[2] - 30.0).abs() < 1e-7
            })
        })
        .unwrap();
    let seam_id = original_order.edges[seam];
    for mode in 0..3 {
        let mut order = original_order.clone();
        if mode == 1 {
            order.edges.rotate_left(5);
        }
        let pick = order.edges.iter().position(|&id| id == seam_id).unwrap();
        for keep_seam in [false, true] {
            let picked = if keep_seam {
                serde_json::json!([{"index":pick}])
            } else {
                serde_json::json!([])
            };
            let step = serde_json::json!({"op":"merge_faces","params":{"keep":{"kind":"edge","picked":picked}}});
            let out = call(
                &step,
                &mut [Slot {
                    body: Arc::clone(&split.body),
                    order: (mode != 2).then_some(order.clone()),
                }],
            )
            .unwrap()
            .unwrap();
            assert!(out.order.is_none());
            assert_eq!(
                out.body.counts().unwrap(),
                if keep_seam { [7, 15, 10] } else { [6, 12, 8] }
            );
            assert!((out.body.volume(1e-9).unwrap() - 6000.0).abs() < 1e-6);
            assert!((out.body.area(1e-9).unwrap() - 2200.0).abs() < 1e-6);
            assert_eq!(split.body.to_record().unwrap().0, before);
        }
    }
}

/// Invalid trace data is a replay failure; validly shaped but invalid geometry is a refusal.
#[test]
fn split_body_and_merge_faces_metadata_do_not_masquerade_as_refusals() {
    let body = Arc::new(solidcore_ops::make_box([0.0; 3], [10.0; 3]).unwrap());
    let input = Slot { body, order: None };
    for keep in [
        serde_json::json!(null),
        serde_json::json!({"kind":"face","picked":[]}),
        serde_json::json!({"kind":"edge"}),
        serde_json::json!({"kind":"edge","picked":[{}]}),
        serde_json::json!({"kind":"edge","picked":[{"index":100000}]}),
        serde_json::json!({"kind":"edge","picked":[{"miss":false}]}),
        serde_json::json!({"kind":"edge","picked":[{"miss":"missed","index":0}]}),
    ] {
        let step = serde_json::json!({"op":"merge_faces","params":{"keep":keep}});
        assert!(matches!(
            call(&step, &mut [input.clone()]),
            Err(Stop::Fail(_))
        ));
    }
    let missed = serde_json::json!({"op":"merge_faces","params":{
        "keep":{"kind":"edge","picked":[{"miss":"no edge at traced point"}]}
    }});
    assert!(matches!(
        call(&missed, &mut [input.clone()]),
        Err(Stop::Unsupported(_))
    ));
    for tool in [serde_json::json!(null), serde_json::json!({"plane":{}})] {
        let step = serde_json::json!({"op":"split_body","params":{"tool":tool}});
        assert!(matches!(
            call_outputs(&step, &mut [input.clone()]),
            Err(Stop::Fail(_))
        ));
    }
    let step = serde_json::json!({"op":"split_body","params":{"tool":{"plane":{
        "origin":[3,0,0],"xaxis":[0,0,0],"yaxis":[0,0,1],"u":[-1,11],"v":[-1,11]
    }}}});
    assert!(matches!(call_outputs(&step, &mut [input]), Ok(Err(_))));
}

#[test]
fn record_picks_preserve_order_and_duplicate_parameter_alignment() {
    let picks = serde_json::json!({"kind":"edge","picked":[
        {"index":1},{"index":1},{"index":0}
    ]});
    assert_eq!(record_picks("blend", &picks, "edge", 2).unwrap(), [1, 1, 0]);
    assert!(matches!(
        record_picks("blend", &picks, "edge", 1),
        Err(Stop::Fail(_))
    ));
    assert!(matches!(
        record_picks("blend", &picks, "vertex", 2),
        Err(Stop::Fail(_))
    ));
}

/// Captured split followed by merging its redundant faces, with the original grading.
#[test]
fn split_body_replays_face_plane() {
    replay_face_split_case("3447-split-along-plane-of-a-face");
}

/// The other captured split/merge workflow, using the body's own extended face as tool.
#[test]
fn split_body_replays_own_extended_face() {
    replay_face_split_case("4654-split-body-by-its-own-face-extended");
}

/// The captured quarter-cylinder sheet and its original record order.
fn extension_sheet_fixture() -> (PathBuf, Value, Slot) {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    let entry = corpus().join("surfaces/cov-extend-quarter-cylinder-along-its-axis.json");
    let e: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
    let record: Record = serde_json::from_value(records(&entry, &e)["b2"].clone()).unwrap();
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

fn sheet_extension_step(edge: usize, distance: f64, how: &str) -> Value {
    serde_json::json!({"op":"extend_sheet","params":{"edge":{"kind":"edge","picked":[{"index":edge}]},"distance":distance,"how":how}})
}

/// The exact captured extension is graded with its unchanged trace and expectations.
#[test]
fn extend_sheet_replays_the_quarter_cylinder() {
    let (entry, e, _) = extension_sheet_fixture();
    assert_eq!(e["rig"]["status"], "pass");
    assert!(e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["error"].is_null()));
    // A zero reference volume has no relative-deviation value; a refusal would error
    // because none of these calls expects one.
    assert_eq!(run_case(&entry, &e).unwrap(), None);
}

/// A picked top/bottom arc and distance determine the new height under any cached record order.
#[test]
fn extend_sheet_preserves_edge_order_distance_and_operand() {
    let (_, _, input) = extension_sheet_fixture();
    let (before, original_order) = input.body.to_record().unwrap();
    for top in [false, true] {
        let z = if top { 20.0 } else { 0.0 };
        let edge = before
            .edges
            .iter()
            .position(|edge| {
                edge.vertices.len() == 2
                    && edge
                        .vertices
                        .iter()
                        .all(|&v| (before.vertices[v as usize].point[2] - z).abs() < 1e-7)
            })
            .unwrap();
        let edge_id = original_order.edges[edge];
        for mode in 0..3 {
            let mut order = original_order.clone();
            if mode == 1 {
                order.edges.rotate_left(2);
            }
            let pick = order.edges.iter().position(|&id| id == edge_id).unwrap();
            for distance in [5.0, 9.0] {
                let step = sheet_extension_step(pick, distance, "natural");
                let out = call(
                    &step,
                    &mut [Slot {
                        body: Arc::clone(&input.body),
                        order: (mode != 2).then_some(order.clone()),
                    }],
                )
                .unwrap()
                .unwrap();
                assert!(out.order.is_none());
                assert_eq!(out.body.counts().unwrap(), [1, 4, 4]);
                let want_area = std::f64::consts::PI * 5.0 * (20.0 + distance);
                let area = out.body.area(1e-9).unwrap();
                assert!(
                    (area - want_area).abs() <= 1e-9 * want_area,
                    "{area} vs {want_area}"
                );
                let record = out.body.to_record().unwrap().0;
                assert!(
                    matches!(record.faces[0].surface,RecordSurface::Cylinder {radius,..} if (radius-10.0).abs()<1e-10)
                );
                let lo = if top { 0.0 } else { -distance };
                let hi = if top { 20.0 + distance } else { 20.0 };
                assert!(record
                    .vertices
                    .iter()
                    .all(|v| (v.point[2] - lo).abs() < 1e-7 || (v.point[2] - hi).abs() < 1e-7));
                assert!(record
                    .vertices
                    .iter()
                    .any(|v| (v.point[2] - lo).abs() < 1e-7));
                assert!(record
                    .vertices
                    .iter()
                    .any(|v| (v.point[2] - hi).abs() < 1e-7));
                assert_eq!(input.body.to_record().unwrap().0, before);
            }
        }
    }
}

/// Trace errors stay replay failures, while a parsed nonpositive distance is an API refusal.
#[test]
fn extend_sheet_metadata_and_refusal_are_distinct() {
    let (_, _, input) = extension_sheet_fixture();
    let step = sheet_extension_step(3, 5.0, "natural");
    for field in ["edge", "distance", "how"] {
        let mut bad = step.clone();
        bad["params"].as_object_mut().unwrap().remove(field);
        assert!(matches!(
            call(&bad, &mut [input.clone()]),
            Err(Stop::Fail(_))
        ));
    }
    for pick in [
        serde_json::json!({"kind":"face","picked":[{"index":3}]}),
        serde_json::json!({"kind":"edge","picked":[]}),
        serde_json::json!({"kind":"edge","picked":[{}]}),
        serde_json::json!({"kind":"edge","picked":[{"index":3},{"index":1}]}),
        serde_json::json!({"kind":"edge","picked":[{"index":100000}]}),
        serde_json::json!({"kind":"edge","picked":[{"miss":false}]}),
        serde_json::json!({"kind":"edge","picked":[{"miss":"missed","index":3}]}),
        serde_json::json!({"kind":"edge","picked":[{"miss":"missed"},{"index":3}]}),
    ] {
        let mut bad = step.clone();
        bad["params"]["edge"] = pick;
        assert!(matches!(
            call(&bad, &mut [input.clone()]),
            Err(Stop::Fail(_))
        ));
    }
    let mut missed = step.clone();
    missed["params"]["edge"] =
        serde_json::json!({"kind":"edge","picked":[{"miss":"no edge at traced point"}]});
    assert!(matches!(
        call(&missed, &mut [input.clone()]),
        Err(Stop::Unsupported(_))
    ));
    assert!(matches!(
        call(
            &sheet_extension_step(3, 5.0, "unported_mode"),
            &mut [input.clone()]
        ),
        Err(Stop::Unsupported(_))
    ));
    assert!(matches!(
        call(&sheet_extension_step(3, 0.0, "natural"), &mut [input]),
        Ok(Err(_))
    ));
}

#[test]
#[ignore = "the whole corpus is slow; run it with --ignored, KERNEL_CASES_AREA shards it"]
fn kernel_cases() {
    let fixes: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("fixes.json")).unwrap())
            .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
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
