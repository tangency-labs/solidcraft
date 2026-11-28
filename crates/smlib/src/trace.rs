//! A trace of the operations a run makes on this crate's bodies, for the kernel rig's corpus
//! (`solidcraft-cli kernel-cases --export`): each body taken from a record (its record), each
//! operation (its name, its operand bodies, its parameters with every pick by points also given
//! as the topology index it picked, and the bodies it made or why it failed), and each body
//! written back as a record. A failed operation's event keeps its kernel call's stable code
//! ([`crate::failure`]) beside its error.
//!
//! A trace is one run's ([`Run`]): the work [`Run::within`] it, on this thread and on the
//! threads that work hands it to ([`current`], [`scoped`]), and nothing another thread runs at the
//! same time, so two runs side by side (a report's rebuild beside the window's own work, #17489)
//! never mix. Off unless a run is over the thread; while off it costs a thread-local read per
//! operation. [`start`] and [`finish`] trace a run over the calling thread.
//!
//! Bodies are named by a number each gets when it is made ([`crate::Brep`]'s id), so a trace
//! says which operation's output went into which operation, and which record each became.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use solidcore_record::brep as b;

use crate::sweeps::{Affine, Curve};
use crate::{Brep, Result};

static NEXT: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The run traced on this thread, if any.
    static CURRENT: RefCell<Option<Run>> = const { RefCell::new(None) };
}

/// One run's trace: the events of the work run [`Run::within`] it, wherever it runs.
#[derive(Clone, Debug, Default)]
pub struct Run(Arc<Mutex<Log>>);

/// A run's events, and the bodies it knows: made from a record in it, or made by an operation
/// it traced.
#[derive(Debug, Default)]
struct Log {
    events: Vec<Event>,
    known: std::collections::HashSet<u64>,
}

impl Run {
    /// A run that has traced nothing yet.
    pub fn new() -> Run {
        Run::default()
    }

    /// Runs `f` traced in this run, on this thread (and on the threads its work hands the run
    /// to), with any run over this thread before put back after it.
    pub fn within<R>(&self, f: impl FnOnce() -> R) -> R {
        scoped(Some(self.clone()), f)
    }

    /// What the run has traced so far, taken: the run goes on empty.
    pub fn take(&self) -> Vec<Event> {
        let mut log = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        log.known.clear();
        std::mem::take(&mut log.events)
    }

    fn push(&self, e: Event) {
        let mut log = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match &e {
            Event::Import { id, .. } => {
                log.known.insert(*id);
            }
            Event::Call { outputs: Ok(made), .. } => log.known.extend(made.iter().copied()),
            _ => {}
        }
        log.events.push(e);
    }

    /// Whether the run knows body `id`: made from a record in it, or by an operation it traced.
    fn knows(&self, id: u64) -> bool {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).known.contains(&id)
    }
}

/// The run traced on this thread, to hand to the threads its work starts ([`scoped`]).
pub fn current() -> Option<Run> {
    CURRENT.try_with(|c| c.borrow().clone()).ok().flatten()
}

/// Runs `f` with `run` (or none) over this thread, putting back what was over it after, on a
/// return or a panic.
pub fn scoped<R>(run: Option<Run>, f: impl FnOnce() -> R) -> R {
    struct Back(Option<Run>);
    impl Drop for Back {
        fn drop(&mut self) {
            let back = self.0.take();
            let _ = CURRENT.try_with(|c| *c.borrow_mut() = back);
        }
    }
    let before = CURRENT.with(|c| std::mem::replace(&mut *c.borrow_mut(), run));
    let _back = Back(before);
    f()
}

/// One thing a traced run did, in the order it happened.
#[derive(Clone, Debug)]
pub enum Event {
    /// Body `id` was made from `record` ([`Brep::from_record`]).
    Import { id: u64, record: Box<b::Brep>, trusted: bool, keeping_splines: bool },
    /// An operation: its name, its operand bodies, its parameters, and the bodies it made
    /// (`Ok`) or its error's message (`Err`); for a failed one, the stable code of the kernel
    /// call it failed with ([`crate::failure`]), when one failed in it.
    Call {
        op: &'static str,
        inputs: Vec<u64>,
        params: Value,
        outputs: std::result::Result<Vec<u64>, String>,
        code: Option<crate::failure::FailureCode>,
    },
    /// Body `id` was written as `record` ([`Brep::to_native`] and its kin).
    Export { id: u64, record: Box<b::Brep> },
}

/// Starts a run's trace over the calling thread, in place of any it had: what it and the work it
/// hands to other threads run is traced until [`finish`].
pub fn start() {
    let _ = CURRENT.try_with(|c| *c.borrow_mut() = Some(Run::new()));
}

/// Stops the calling thread's trace ([`start`]) and hands over what it recorded.
pub fn finish() -> Vec<Event> {
    CURRENT.try_with(|c| c.borrow_mut().take()).ok().flatten().map(|r| r.take()).unwrap_or_default()
}

/// The names of solidcore's upstream fixes this crate turns on: the fix set every traced
/// operation ran with.
pub fn fixes() -> Vec<&'static str> {
    crate::core::fixes().flags().filter(|&(_, on)| on).map(|(name, _)| name).collect()
}

pub(crate) fn on() -> bool {
    CURRENT.try_with(|c| c.borrow().is_some()).unwrap_or(false)
}

/// A new body's number.
pub(crate) fn next_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn push(e: Event) {
    if let Some(run) = current() {
        run.push(e);
    }
}

pub(crate) fn imported(r: &Result<Brep>, record: &b::Brep, trusted: bool, keeping_splines: bool) {
    if !on() {
        return;
    }
    if let Ok(body) = r {
        push(Event::Import { id: body.id, record: Box::new(record.clone()), trusted, keeping_splines });
    }
}

/// Associate an operation's output with the record returned to the application. Normally this
/// is an export; when the kernel proves the operation was a no-op it is the original record.
pub fn exported(body: &Brep, record: &b::Brep) {
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
    let (outputs, code) = match r {
        Ok(v) => (Ok(outputs(v)), None),
        // the kernel call it failed with: the last that failed on this thread, when this
        // operation's error is that call's or was made from it
        Err(e) => {
            let message = e.to_string();
            let code = crate::failure::last().filter(|(_, m)| !m.is_empty() && message.contains(m.as_str())).map(|(c, _)| c);
            (Err(message), code)
        }
    };
    // An operand made before the run (a feature's input, built by the features before it) comes
    // in as its record, so the trace replays from what the operation took.
    if let Some(run) = current() {
        for body in inputs.iter().filter(|b| !run.knows(b.id)) {
            // written outside the run: the record is no export of the run's
            if let Ok(record) = scoped(None, || body.to_native()) {
                run.push(Event::Import { id: body.id, record: Box::new(record), trusted: true, keeping_splines: false });
            }
        }
    }
    push(Event::Call { op, inputs: inputs.iter().map(|b| b.id).collect(), params: params(), outputs, code });
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
    /// The nearest face as it is trimmed (an untrim's picker).
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
                Event::Call { op: "fillet", inputs, params, outputs, .. } if inputs == &vec![block.id] => Some((params, outputs)),
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

    #[test]
    fn transform_face_records_the_resolved_pick_and_motion() {
        let block = Brep::make_box([0.0; 3], [10.0; 3]).unwrap();
        let index = block.face_points().unwrap().iter().position(|p| p[2] == 10.0).unwrap();
        // A pick on the rim: record the face supplied by the caller, never an ambiguous re-pick.
        let face = crate::PickedFace { index, at: [5.0, 0.0, 10.0], tolerance: 0.02 };
        let origin = [0.0, 0.0, 10.0];
        let axis = [1.0, 0.0, 0.0];
        let angle = std::f64::consts::FRAC_PI_4;
        let translate = [0.0, 0.0, 2.0];
        let run = Run::new();
        let (roof, changed) = run.within(|| block.transform_face(face, translate, origin, axis, angle)).unwrap();
        assert!(changed);
        let events = run.take();
        let (params, outputs) = events
            .iter()
            .find_map(|e| match e {
                Event::Call { op: "transform_face", params, outputs, .. } => Some((params, outputs)),
                _ => None,
            })
            .unwrap();
        assert_eq!(outputs, &Ok(vec![roof.id]));
        assert_eq!(
            params,
            &json!({
                "face": {"kind": "face", "points": [face.at], "tolerance": face.tolerance, "picked": [{"index": index, "at": []}]},
                "origin": origin, "axis": axis, "angle": angle, "translate": translate
            })
        );
    }

    /// Two runs traced side by side on two threads keep their own events: each has its own
    /// fillet, never the other's, and a thread with no run over it traces nothing.
    #[test]
    fn runs_side_by_side_do_not_mix() {
        let traced = |depth: f64| {
            std::thread::spawn(move || {
                let block = Brep::make_box([0.0; 3], [10.0, 20.0, depth]).unwrap();
                let run = Run::new();
                let round = run.within(|| block.fillet(&[[10.0, 10.0, depth]], 1e-3, 2.0).unwrap());
                let untraced = block.fillet(&[[10.0, 10.0, depth]], 1e-3, 1.0).unwrap();
                (block.id, round.id, untraced.id, run.take())
            })
        };
        let (a, b) = (traced(30.0), traced(40.0));
        for (block, round, untraced, events) in [a.join().unwrap(), b.join().unwrap()] {
            let calls: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Call { op: "fillet", inputs, outputs, .. } => Some((inputs.clone(), outputs.clone())),
                    _ => None,
                })
                .collect();
            assert_eq!(calls, vec![(vec![block], Ok(vec![round]))], "only this run's fillet");
            assert!(!events.iter().any(|e| matches!(e, Event::Call { outputs: Ok(o), .. } if o.contains(&untraced))));
        }
    }

    /// A traced operation that fails keeps the stable code of the kernel call it failed with:
    /// the code the thread's last failure has ([`crate::failure`]).
    #[test]
    fn a_failed_call_keeps_its_kernel_code() {
        let wall = Brep::make_box([0.0; 3], [30.0, 2.0, 10.0]).unwrap();
        start();
        let e = wall.fillet(&[[15.0, 0.0, 10.0], [15.0, 2.0, 10.0]], 1e-3, 1.5).unwrap_err();
        let events = finish();
        let (code, _) = crate::failure::last().unwrap();
        let traced = events.iter().find_map(|ev| match ev {
            Event::Call { op: "fillet", outputs: Err(m), code, .. } => Some((m.clone(), *code)),
            _ => None,
        });
        assert_eq!(traced, Some((e.to_string(), Some(code))));
    }
}
