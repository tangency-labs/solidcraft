//! A trace of the operations a run makes on this crate's bodies, for the kernel rig's corpus
//! (`solidcraft-cli kernel-cases --export`): each body taken from a record (its record), each
//! operation (its name, its operand bodies, its parameters with every pick by points also given
//! as the topology index it picked, and the bodies it made or why it failed), and each body
//! written back as a record. Off unless [`start`]ed; while off it costs an atomic load per
//! operation.
//!
//! Bodies are named by a number each gets when it is made ([`crate::Brep`]'s id), so a trace
//! says which operation's output went into which operation, and which record each became.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use serde_json::{Value, json};
use solidcore_record::brep as b;

use crate::sweeps::{Affine, Curve};
use crate::{Brep, Result};

static ON: AtomicBool = AtomicBool::new(false);
static NEXT: AtomicU64 = AtomicU64::new(1);
static LOG: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// One thing a traced run did, in the order it happened.
#[derive(Clone, Debug)]
pub enum Event {
    /// Body `id` was made from `record` ([`Brep::from_record`]).
    Import { id: u64, record: Box<b::Brep>, trusted: bool, keeping_splines: bool },
    /// An operation: its name, its operand bodies, its parameters, and the bodies it made
    /// (`Ok`) or its error's message (`Err`).
    Call { op: &'static str, inputs: Vec<u64>, params: Value, outputs: std::result::Result<Vec<u64>, String> },
    /// Body `id` was written as `record` ([`Brep::to_native`] and its kin).
    Export { id: u64, record: Box<b::Brep> },
}

/// Starts a trace, dropping anything an earlier one left.
pub fn start() {
    LOG.lock().unwrap_or_else(PoisonError::into_inner).clear();
    ON.store(true, Ordering::SeqCst);
}

/// Stops the trace and hands over what it recorded.
pub fn finish() -> Vec<Event> {
    ON.store(false, Ordering::SeqCst);
    std::mem::take(&mut *LOG.lock().unwrap_or_else(PoisonError::into_inner))
}

/// The names of solidcore's upstream fixes this crate turns on: the fix set every traced
/// operation ran with.
pub fn fixes() -> Vec<&'static str> {
    crate::core::fixes().flags().filter(|&(_, on)| on).map(|(name, _)| name).collect()
}

pub(crate) fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// A new body's number.
pub(crate) fn next_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn push(e: Event) {
    LOG.lock().unwrap_or_else(PoisonError::into_inner).push(e);
}

pub(crate) fn imported(r: &Result<Brep>, record: &b::Brep, trusted: bool, keeping_splines: bool) {
    if !on() {
        return;
    }
    if let Ok(body) = r {
        push(Event::Import { id: body.id, record: Box::new(record.clone()), trusted, keeping_splines });
    }
}

pub(crate) fn exported(body: &Brep, record: &b::Brep) {
    if on() {
        push(Event::Export { id: body.id, record: Box::new(record.clone()) });
    }
}

/// Records operation `op` on `inputs`, whose result `r` made the bodies `outputs` reads from it.
/// `params` is only built while tracing, after the operation ran.
pub(crate) fn call<T>(op: &'static str, inputs: &[&Brep], r: &Result<T>, outputs: impl FnOnce(&T) -> Vec<u64>, params: impl FnOnce() -> Value) {
    if !on() {
        return;
    }
    let outputs = match r {
        Ok(v) => Ok(outputs(v)),
        Err(e) => Err(e.to_string()),
    };
    push(Event::Call { op, inputs: inputs.iter().map(|b| b.id).collect(), params: params(), outputs });
}

/// [`call`] for an operation that makes one body.
pub(crate) fn made(op: &'static str, inputs: &[&Brep], r: &Result<Brep>, params: impl FnOnce() -> Value) {
    call(op, inputs, r, |b| vec![b.id], params);
}

/// [`call`] for an operation that may make no body.
pub(crate) fn maybe(op: &'static str, inputs: &[&Brep], r: &Result<Option<Brep>>, params: impl FnOnce() -> Value) {
    call(op, inputs, r, |b| b.iter().map(|b| b.id).collect(), params);
}

/// What a pick by points is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickKind {
    /// The edge nearest each point, unambiguous (`pick_edges`, the blends' picker).
    Edge,
    /// The face nearest each point, unambiguous (`pick_faces`, the offsets' picker).
    Face,
    /// The vertex nearest each point, unambiguous (`pick_vertices`, the setbacks' picker).
    Vertex,
    /// The nearest edge (`nearest_edge`, an extension's picker).
    NearestEdge,
    /// The nearest face (`face_at`, an untrim's picker).
    NearestFace,
}

/// What one point picked: the entity's index in the body's list of its kind, and where it is
/// (an edge's end points, a vertex's point), or why it picked none.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub index: Option<usize>,
    pub at: Vec<[f64; 3]>,
    pub miss: Option<&'static str>,
}

impl Pick {
    pub(crate) fn missed(why: &'static str) -> Pick {
        Pick { index: None, at: Vec::new(), miss: Some(why) }
    }
}

/// The points of a pick on `body` and what each picked: `{"points", "tolerance", "picked"}`,
/// each of `picked` `{"index", "at"}` or `{"miss"}`.
pub(crate) fn picks(body: &Brep, kind: PickKind, points: &[[f64; 3]], tolerance: f64) -> Value {
    let picked = match crate::core::picked::picks(body.core(), kind, points, tolerance) {
        Ok(p) => Value::Array(
            p.into_iter()
                .map(|p| match (p.index, p.miss) {
                    (Some(i), _) => json!({"index": i, "at": p.at}),
                    (None, why) => json!({"miss": why.unwrap_or("none")}),
                })
                .collect(),
        ),
        Err(e) => json!({"error": e.to_string()}),
    };
    let kind = match kind {
        PickKind::Edge | PickKind::NearestEdge => "edge",
        PickKind::Face | PickKind::NearestFace => "face",
        PickKind::Vertex => "vertex",
    };
    json!({"kind": kind, "points": points, "tolerance": tolerance, "picked": picked})
}

/// A profile, path or section curve as data.
pub(crate) fn curve(c: &Curve) -> Value {
    match c {
        Curve::Line { start, end } => json!({"line": {"start": start, "end": end}}),
        Curve::Arc { center, axis, reference, radius, start, end } => {
            json!({"arc": {"center": center, "axis": axis, "reference": reference, "radius": radius, "start": start, "end": end}})
        }
        Curve::Nurbs { degree, points, knots } => json!({"nurbs": {"degree": degree, "points": points, "knots": knots}}),
    }
}

pub(crate) fn curves(cs: &[Curve]) -> Value {
    Value::Array(cs.iter().map(curve).collect())
}

pub(crate) fn affine(a: &Affine) -> Value {
    json!({"m": a.m, "t": a.t})
}

/// A split tool's surface as data.
pub(crate) fn surface(s: &crate::ssi::Spec) -> Value {
    use crate::ssi::Spec;
    match s {
        Spec::Plane { origin, xaxis, yaxis, u, v } => json!({"plane": {"origin": origin, "xaxis": xaxis, "yaxis": yaxis, "u": u, "v": v}}),
        Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height } => {
            json!({"cylinder": {"origin": origin, "xaxis": xaxis, "yaxis": yaxis, "radius": radius, "angle_deg": angle_deg, "height": height}})
        }
        Spec::Cone { apex, xaxis, yaxis, half_angle_deg, height } => {
            json!({"cone": {"apex": apex, "xaxis": xaxis, "yaxis": yaxis, "half_angle_deg": half_angle_deg, "height": height}})
        }
        Spec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg } => {
            json!({"sphere": {"center": center, "xaxis": xaxis, "yaxis": yaxis, "radius": radius, "lon_deg": lon_deg, "lat_deg": lat_deg}})
        }
        Spec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg } => json!({"torus": {
            "center": center, "xaxis": xaxis, "yaxis": yaxis, "major_radius": major_radius, "minor_radius": minor_radius,
            "major_deg": major_deg, "minor_deg": minor_deg}}),
        Spec::Nurbs { degree, nu, nv, knots_u, knots_v, points, weights } => json!({"nurbs": {
            "degree": degree, "nu": nu, "nv": nv, "knots_u": knots_u, "knots_v": knots_v, "points": points, "weights": weights}}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A traced fillet names its operand, its output and the edge its point picked, by index and
    /// end points; the record its output was written as and the body read back from it are
    /// traced under their own numbers. Other tests' operations may land in the trace too: only
    /// these bodies' events are read.
    #[test]
    fn a_fillet_is_traced_with_its_pick_and_records() {
        let block = Brep::make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
        start();
        let round = block.fillet(&[[10.0, 10.0, 30.0]], 1e-3, 2.0).unwrap();
        let record = round.to_native().unwrap();
        let again = Brep::from_record(&record, true).unwrap();
        let events = finish();
        let call = events
            .iter()
            .find_map(|e| match e {
                Event::Call { op: "fillet", inputs, params, outputs } if inputs == &vec![block.id] => Some((params, outputs)),
                _ => None,
            })
            .unwrap();
        assert_eq!(call.1, &Ok(vec![round.id]));
        let picked = &call.0["edges"]["picked"][0];
        assert!(picked["index"].as_u64().is_some(), "{picked}");
        // The point (10, 10, 30) is on the top edge x = 10, z = 30, from y = 0 to y = 20.
        let mut ends: Vec<Vec<f64>> = picked["at"].as_array().unwrap().iter().map(|p| serde_json::from_value(p.clone()).unwrap()).collect();
        ends.sort_by(|a, b| a[1].total_cmp(&b[1]));
        assert_eq!(ends, vec![vec![10.0, 0.0, 30.0], vec![10.0, 20.0, 30.0]]);
        assert!(events.iter().any(|e| matches!(e, Event::Export { id, record: r } if *id == round.id && **r == record)));
        assert!(events.iter().any(|e| matches!(e, Event::Import { id, record: r, trusted: true, .. } if *id == again.id && **r == record)));
        assert_ne!(again.id, round.id);
    }
}
