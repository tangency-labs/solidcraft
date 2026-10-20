//! Differential runner for unit `topology.brep_p4`: `SmBrep::DeleteEdge`,
//! `DeleteVertex`, `ShrinkGeometry`, `StitchFaces` and `~SmBrep` on real B-reps.
//!
//! The B-rep comes from the shared fixture (`sc_topology_fixture.h`): the C++ side
//! builds it, the Rust side imports the `topology.topology_traverser` export of the same
//! block record by record (with the vertex use lists), so the target indices name the
//! same objects on both sides. Compared: the status, and on success (for the edits also
//! after a failed status, which can come after part of the topology changed) the ten
//! record counts of the B-rep afterwards (`StitchFaces` adds its outputs; the
//! destructor its context flag) and every record of the B-rep, field by field (see
//! [`Rec`]): links
//! (labels), lists in their order, orientations, flags, domains, tolerances, points,
//! intervals, UV points. Cases whose Rust path reaches an unported callee are deferred.
//!
//! **Record naming.** A record that existed before the operation keeps the index of
//! the first walk on both sides (the Rust arena id; the shim tags the C++ object).
//! Records the operation made are renumbered by [`canonicalize`] (the same function on
//! both sides) from their contents and links, so the order the two sides created or
//! reached them in does not matter. The `Check` operation runs nothing and compares
//! the imported model with the C++ records, which proves the encodings agree while the
//! operations still defer.
use super::topology_vertexuse::{self as shared, BrepIn};
use crate::case::CaseContext;
use crate::output::Topology;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::error::{EntityRef, SolidError};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p4::{self, DeleteEnv};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::topology::Topology as SmTopologyRecord;
use solidcore::topology::{
    ArenaId, Brep, EdgeId, EdgeuseOwner, FaceId, LoopuseStart, ShellId, VertexId, VertexuseId,
    VertexuseOwner,
};
use solidcore_ffi::units::topology_brep_p4::{self as ffi, Damage, Op, Request, NULL_DEREFERENCE};
use solidcore_ffi::units::topology_vertexuse::counts;
use std::collections::BTreeMap;

/// Differential runner for `topology.brep_p4`.
pub struct Runner;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OpIn {
    DeleteEdge,
    DeleteVertex,
    ShrinkGeometry,
    StitchFaces,
    Destroy,
    /// No operation: the imported model against the C++ records.
    Check,
}

/// A hand-made defect of the built B-rep (the fixtures build whole B-reps only), put in
/// before the operation on both sides. Inputs upstream dereferences `NULL` on are refused
/// by the shim (the guard) and the case is `rejected`; every other damaged input runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DamageIn {
    /// The face with this index loses its surface (`delete_edge`, `check`).
    NoSurface(i64),
    /// The shell with this index loses its region (`delete_edge`, `delete_vertex`,
    /// `check`).
    NoRegion(i64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    op: OpIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    damage: Option<DamageIn>,
    #[serde(default = "yes")]
    editing: bool,
    /// The target edge or vertex index in walk order.
    #[serde(default)]
    target: i64,
    /// The face whose surface to keep (`DeleteEdge`) or to stitch, or `-1`.
    #[serde(default = "minus_one")]
    keep: i64,
    #[serde(default)]
    tol: f64,
    #[serde(default)]
    flag: bool,
}

fn yes() -> bool {
    true
}
fn minus_one() -> i64 {
    -1
}
fn err(e: &(impl ToString + ?Sized)) -> HarnessError {
    HarnessError::new(e.to_string())
}

/// The shared encoder (`topology_vertexuse::push_surface`) writes the knot counts as
/// `len - 1` unchecked, so a B-spline surface without knots (a fuzz mutation) is
/// malformed input here rather than an overflow.
fn check_knots(surface: &impl Serialize) -> Result<(), HarnessError> {
    let s = serde_json::to_value(surface).map_err(|e| err(&e))?;
    if let Some(b) = s.get("bspline") {
        for k in ["u", "v"] {
            if b.get(k)
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
            {
                return Err(err("surface without knots"));
            }
        }
    }
    Ok(())
}

/// The fixture block of `brep` (validate-only off, no cached gaps).
fn block(brep: &BrepIn) -> Result<(String, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match brep {
        BrepIn::File { rebuild, .. } => v.extend([0.0, f64::from(u8::from(*rebuild))]),
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            shared::push_curve(&mut v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            check_knots(surface)?;
            v.push(2.0);
            shared::push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::Network => v.push(3.0),
    }
    v.extend([0.0, 0.0]);
    Ok((path_of(brep)?, v))
}

/// The file of a B-rep: under `TestFiles/pt_TestFiles`, or with the prefix `fixture:`
/// one of this unit's hand-built files in `fixtures/topology.brep_p4` (struts, a loop
/// split by a bridge edge, two holes sharing an edge, a face inside a hole, wire paths).
fn path_of(brep: &BrepIn) -> Result<String, HarnessError> {
    if let BrepIn::File { file, .. } = brep {
        if let Some(name) = file.strip_prefix("fixture:") {
            if name.contains(['/', '\\']) {
                return Err(err("fixture file names have no directories"));
            }
            let p = crate::case::repo_root()
                .join("fixtures/topology.brep_p4")
                .join(name);
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    shared::path_of(brep)
}

fn request(i: &Inputs) -> Request {
    Request {
        damage: i.damage.map(|d| match d {
            DamageIn::NoSurface(f) => Damage::NoSurface(f),
            DamageIn::NoRegion(s) => Damage::NoRegion(s),
        }),
        op: match i.op {
            OpIn::DeleteEdge => Op::DeleteEdge,
            OpIn::DeleteVertex => Op::DeleteVertex,
            OpIn::ShrinkGeometry => Op::ShrinkGeometry,
            OpIn::StitchFaces => Op::StitchFaces,
            OpIn::Destroy => Op::Destroy,
            OpIn::Check => Op::Check,
        },
        editing: i.editing,
        target: i.target,
        keep: i.keep,
        tol: i.tol,
        flag: i.flag,
    }
}

/// The imported fixture with the vertex use lists in their raw order.
fn import(
    f: &solidcore_ffi::units::topology_topology_traverser::Fixture,
) -> Result<Brep, HarnessError> {
    let (mut b, _) = shared::import(&f.graph)?;
    for (i, list) in f.vertexuses.iter().enumerate() {
        b.vertex_mut(VertexId::from_index(i as u32))
            .map_err(|e| err(&e))?
            .vertexuses = list.iter().map(|&j| VertexuseId::from_index(j)).collect();
    }
    Ok(b)
}

/// The record counts of the shim's walk, in its kind order.
fn record_counts(b: &Brep) -> [usize; 10] {
    [
        b.regions().len(),
        b.shells().len(),
        b.faceuses().len(),
        b.faces().len(),
        b.loopuses().len(),
        b.loops().len(),
        b.edgeuses().len(),
        b.edges().len(),
        b.vertexuses().len(),
        b.vertices().len(),
    ]
}

// ---- the records ---------------------------------------------------------------------------

/// The first label of a record the operation made.
const NEW_BASE: i64 = 1_000_000;

/// The kinds in the walk order of the shim.
const KINDS: [&str; 10] = [
    "regions",
    "shells",
    "faceuses",
    "faces",
    "loopuses",
    "loops",
    "edgeuses",
    "edges",
    "vertexuses",
    "vertices",
];

/// A field of a record: a plain integer, a link to a record of a kind, a list of such
/// links (a count, then the labels) or a real.
#[derive(Clone, Copy)]
enum Slot {
    I,
    R(usize),
    L(usize),
    F,
}
use Slot::{F, I, L, R};

/// The fields of each kind, in the order the shim writes them (`emit_records`).
const LAYOUT: [&[Slot]; 10] = [
    // region: in the B-rep
    &[I],
    // shell: region
    &[R(0)],
    // face use: shell, face, mate, orientation, loop uses
    &[R(1), R(3), R(2), I, L(4)],
    // face: face use, domain (u0 v0 u1 v1), rectangular trim, tolerance, has a surface,
    // surface owned by the face, natural domain of the surface (u0 v0 u1 v1)
    &[R(2), F, F, F, F, I, F, I, I, F, F, F, F],
    // loop use: face use, loop, mate, start type, start edge use, start vertex use,
    // orientation
    &[R(2), R(5), R(4), I, R(6), R(8), I],
    // loop: loop use
    &[R(4)],
    // edge use: edge, owner type, owner loop use, owner shell, cw, ccw, vertex use,
    // orientation, has a UV trim curve
    &[R(7), I, R(4), R(1), R(6), R(6), R(8), I, I],
    // edge: tolerance, interval, edge uses, has a curve
    &[F, F, F, L(6), I],
    // vertex use: vertex, owner type, owner edge use, loop use, shell, UV point
    &[R(9), I, R(6), R(4), R(1), F, F],
    // vertex: point, tolerance, vertex uses
    &[F, F, F, F, L(8)],
];

/// One record: its label, its integer fields (links are labels, `-1` null; a list is
/// its count and its labels), the kind each integer links to (if it does) and its reals.
#[derive(Debug, Clone, PartialEq)]
struct Rec {
    label: i64,
    ints: Vec<i64>,
    links: Vec<Option<usize>>,
    floats: Vec<f64>,
}

impl Rec {
    fn new(label: i64) -> Self {
        Self {
            label,
            ints: Vec::new(),
            links: Vec::new(),
            floats: Vec::new(),
        }
    }
    fn i(&mut self, x: i64) {
        self.ints.push(x);
        self.links.push(None);
    }
    fn r(&mut self, kind: usize, x: i64) {
        self.ints.push(x);
        self.links.push(Some(kind));
    }
    fn l(&mut self, kind: usize, xs: &[i64]) {
        self.i(xs.len() as i64);
        for &x in xs {
            self.r(kind, x);
        }
    }
    fn f(&mut self, x: f64) {
        self.floats.push(x);
    }
}

type Model = [Vec<Rec>; 10];

/// The records the shim wrote (`emit_records`), as given by [`LAYOUT`].
fn parse_records(v: &[f64]) -> Result<Model, HarnessError> {
    let mut at = 0usize;
    let mut next = || -> Result<f64, HarnessError> {
        let x = *v.get(at).ok_or_else(|| err("short record output"))?;
        at += 1;
        Ok(x)
    };
    let mut model: Model = Default::default();
    for (slots, out) in LAYOUT.iter().zip(model.iter_mut()) {
        let n = next()? as usize;
        for _ in 0..n {
            let mut rec = Rec::new(next()? as i64);
            for slot in *slots {
                match *slot {
                    I => rec.i(next()? as i64),
                    R(k) => rec.r(k, next()? as i64),
                    L(k) => {
                        let count = next()? as usize;
                        let mut xs = Vec::new();
                        for _ in 0..count {
                            xs.push(next()? as i64);
                        }
                        rec.l(k, &xs);
                    }
                    F => rec.f(next()?),
                }
            }
            out.push(rec);
        }
    }
    if at != v.len() {
        return Err(err("trailing record output"));
    }
    Ok(model)
}

/// The label of record `index` of a kind with `pre` records before the operation.
fn label(pre: usize, index: u32) -> i64 {
    let index = index as usize;
    if index < pre {
        index as i64
    } else {
        NEW_BASE + (index - pre) as i64
    }
}

/// The labels of the records of the Rust B-rep (`pre`: the record counts per kind
/// before the operation).
struct Labels {
    pre: [usize; 10],
}

impl Labels {
    fn id<I: ArenaId>(&self, kind: usize, id: I) -> i64 {
        label(self.pre.get(kind).copied().unwrap_or(0), id.index())
    }
    /// A link: `-1` when null.
    fn at<I: ArenaId>(&self, kind: usize, id: Option<I>) -> i64 {
        id.map_or(-1, |id| self.id(kind, id))
    }
    fn ids<I: ArenaId>(&self, kind: usize, ids: &[I]) -> Vec<i64> {
        ids.iter().map(|&id| self.id(kind, id)).collect()
    }
}

fn orient(o: OrientType) -> i64 {
    match o {
        OrientType::Unknown => 0,
        OrientType::Same => 1,
        OrientType::Opposite => 2,
    }
}

/// The records of the Rust B-rep, with the layout of the shim's. `pre` are the record
/// counts before the operation.
fn rust_records(b: &Brep, pre: [usize; 10]) -> Model {
    let lb = Labels { pre };
    let (mut regions, mut shells, mut faceuses, mut faces) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut loopuses, mut loops, mut edgeuses) = (Vec::new(), Vec::new(), Vec::new());
    let (mut edges, mut vertexuses, mut vertices) = (Vec::new(), Vec::new(), Vec::new());
    for (id, r) in b.regions().iter() {
        let mut rec = Rec::new(lb.id(0, id));
        rec.i(i64::from(r.in_brep));
        regions.push(rec);
    }
    for (id, s) in b.shells().iter() {
        let mut rec = Rec::new(lb.id(1, id));
        rec.r(0, lb.at(0, s.region));
        shells.push(rec);
    }
    for (id, f) in b.faceuses().iter() {
        let mut rec = Rec::new(lb.id(2, id));
        rec.r(1, lb.at(1, f.shell));
        rec.r(3, lb.at(3, f.face));
        rec.r(2, lb.at(2, f.mate));
        rec.i(orient(f.orientation));
        rec.l(4, &lb.ids(4, &f.loopuses));
        faceuses.push(rec);
    }
    for (id, f) in b.faces().iter() {
        let mut rec = Rec::new(lb.id(3, id));
        rec.r(2, lb.at(2, f.faceuse));
        let (lo, hi) = (f.uv_domain.min(), f.uv_domain.max());
        for x in [lo.x, lo.y, hi.x, hi.y] {
            rec.f(x);
        }
        rec.i(i64::from(f.rectangular_trim));
        rec.f(f.tolerance);
        rec.i(i64::from(f.surface.is_some()));
        let owned = f.surface.as_ref().and_then(Surface::owner) == Some(face_owner(id.index()));
        rec.i(i64::from(owned));
        let natural = f.surface.as_ref().map_or([0.0; 4], |s| {
            let d = s.natural_uv_domain();
            [d.min().x, d.min().y, d.max().x, d.max().y]
        });
        for x in natural {
            rec.f(x);
        }
        faces.push(rec);
    }
    for (id, l) in b.loopuses().iter() {
        let mut rec = Rec::new(lb.id(4, id));
        rec.r(2, lb.at(2, l.faceuse));
        rec.r(5, lb.at(5, l.loop_));
        rec.r(4, lb.at(4, l.mate));
        let (ty, eu, vu) = match l.start {
            LoopuseStart::Unknown => (0, -1, -1),
            LoopuseStart::Edgeuse(e) => (1, lb.at(6, e), -1),
            LoopuseStart::Vertexuse(v) => (2, -1, lb.at(8, v)),
        };
        rec.i(ty);
        rec.r(6, eu);
        rec.r(8, vu);
        rec.i(orient(l.orientation));
        loopuses.push(rec);
    }
    for (id, l) in b.loops().iter() {
        let mut rec = Rec::new(lb.id(5, id));
        rec.r(4, lb.at(4, l.loopuse));
        loops.push(rec);
    }
    for (id, e) in b.edgeuses().iter() {
        let mut rec = Rec::new(lb.id(6, id));
        rec.r(7, lb.at(7, e.edge));
        let (ty, lu, sh) = match e.owner {
            EdgeuseOwner::Unknown => (0, -1, -1),
            EdgeuseOwner::Loopuse(l) => (1, lb.at(4, l), -1),
            EdgeuseOwner::Shell(s) => (2, -1, lb.at(1, s)),
        };
        rec.i(ty);
        rec.r(4, lu);
        rec.r(1, sh);
        rec.r(6, lb.at(6, e.cw));
        rec.r(6, lb.at(6, e.ccw));
        rec.r(8, lb.at(8, e.vertexuse));
        rec.i(orient(e.orientation));
        rec.i(i64::from(e.uv_trim_curve.is_some()));
        edgeuses.push(rec);
    }
    for (id, e) in b.edges().iter() {
        let mut rec = Rec::new(lb.id(7, id));
        rec.f(e.tolerance);
        rec.f(e.interval.min());
        rec.f(e.interval.max());
        rec.l(6, &lb.ids(6, &e.edgeuses));
        rec.i(i64::from(e.curve.is_some()));
        edges.push(rec);
    }
    for (id, v) in b.vertexuses().iter() {
        let mut rec = Rec::new(lb.id(8, id));
        rec.r(9, lb.at(9, v.vertex));
        let (ty, eu, lu, sh) = match v.owner {
            VertexuseOwner::Unknown => (0, -1, -1, -1),
            VertexuseOwner::Edgeuse(e) => (1, lb.at(6, e), -1, -1),
            VertexuseOwner::Loopuse(l) => (2, -1, lb.at(4, l), -1),
            VertexuseOwner::Shell(s) => (3, -1, -1, lb.at(1, s)),
        };
        rec.i(ty);
        rec.r(6, eu);
        rec.r(4, lu);
        rec.r(1, sh);
        rec.f(v.uv_point.x);
        rec.f(v.uv_point.y);
        vertexuses.push(rec);
    }
    for (id, v) in b.vertices().iter() {
        let mut rec = Rec::new(lb.id(9, id));
        rec.f(v.point.x);
        rec.f(v.point.y);
        rec.f(v.point.z);
        rec.f(v.tolerance);
        rec.l(8, &lb.ids(8, &v.vertexuses));
        vertices.push(rec);
    }
    [
        regions, shells, faceuses, faces, loopuses, loops, edgeuses, edges, vertexuses, vertices,
    ]
}

/// Renumbers the records the operation made, the same way on both sides.
///
/// A new record has no name both sides agree on: C++ finds the objects in its walk
/// order, Rust made them in the order of its arena. Its class is refined until stable
/// from what is known of it: its fields, with the links to old records as they are
/// and the links to new records by the class of their target (colour refinement); a
/// new record is then labelled `NEW_BASE + class`. New records that cannot be told
/// apart by their fields and links are interchangeable and keep a common label.
fn canonicalize(m: &mut Model) {
    let mut class: Vec<BTreeMap<i64, i64>> = m
        .iter()
        .map(|recs| {
            recs.iter()
                .filter(|r| r.label >= NEW_BASE)
                .map(|r| (r.label, 0))
                .collect()
        })
        .collect();
    let total: usize = class.iter().map(BTreeMap::len).sum();
    let mut seen = vec![usize::from(total > 0); m.len()];
    for _ in 0..=total {
        let mut counts = Vec::new();
        let mut next = Vec::new();
        for recs in m.iter() {
            let sigs: Vec<(Vec<i64>, i64)> = recs
                .iter()
                .filter(|r| r.label >= NEW_BASE)
                .map(|r| (signature(r, &class), r.label))
                .collect();
            let mut uniq: Vec<&Vec<i64>> = sigs.iter().map(|(s, _)| s).collect();
            uniq.sort();
            uniq.dedup();
            counts.push(uniq.len());
            next.push(
                sigs.iter()
                    .map(|(s, l)| (*l, uniq.binary_search(&s).unwrap_or(0) as i64))
                    .collect::<BTreeMap<_, _>>(),
            );
        }
        class = next;
        if counts == seen {
            break;
        }
        seen = counts;
    }
    for (recs, own) in m.iter_mut().zip(&class) {
        for r in recs.iter_mut() {
            for (x, l) in r.ints.iter_mut().zip(&r.links) {
                if let Some(k) = l {
                    *x = renamed(&class, *k, *x);
                }
            }
            if r.label >= NEW_BASE {
                r.label = NEW_BASE + own.get(&r.label).copied().unwrap_or(0);
            }
        }
        recs.sort_by(|a, b| {
            let bits = |r: &Rec| r.floats.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
            (a.label, &a.ints, bits(a)).cmp(&(b.label, &b.ints, bits(b)))
        });
    }
}

/// A link to record `x` of a kind, with a new record named by its class.
fn renamed(class: &[BTreeMap<i64, i64>], kind: usize, x: i64) -> i64 {
    if x >= NEW_BASE {
        NEW_BASE
            + class
                .get(kind)
                .and_then(|c| c.get(&x))
                .copied()
                .unwrap_or(0)
    } else {
        x
    }
}

/// What is known of a new record: its fields, with the links to new records by class.
fn signature(r: &Rec, class: &[BTreeMap<i64, i64>]) -> Vec<i64> {
    let mut sig: Vec<i64> = r
        .ints
        .iter()
        .zip(&r.links)
        .map(|(&x, l)| l.map_or(x, |k| renamed(class, k, x)))
        .collect();
    sig.extend(r.floats.iter().map(|f| f.to_bits() as i64));
    sig
}

/// The topology record and the real fields of a model, for the comparison.
fn records_output(o: Output, m: &Model) -> Output {
    let mut t = Topology::new();
    let mut o = o;
    for (name, recs) in KINDS.iter().zip(m.iter()) {
        t = t.entities(name, recs.iter().map(|r| r.label).collect::<Vec<_>>());
        t = t.adjacency(name, recs.iter().map(|r| r.ints.clone()).collect());
        let floats: Vec<f64> = recs.iter().flat_map(|r| r.floats.iter().copied()).collect();
        o = o.values(&format!("{name}_values"), floats);
    }
    o.topology(t)
}

/// The operations whose records are compared after a failed status (the shim's ops 0-2).
fn edits(op: OpIn) -> bool {
    matches!(
        op,
        OpIn::DeleteEdge | OpIn::DeleteVertex | OpIn::ShrinkGeometry
    )
}

fn status_output(status: f64) -> Output {
    Output::new().values("status", vec![status])
}

fn rust_status(e: &SolidError) -> Result<Output, HarnessError> {
    if let Some(o) = Output::unported(e) {
        return Ok(o);
    }
    match e {
        SolidError::Upstream { code, .. } => Ok(status_output(f64::from(*code))),
        other => Err(err(&format!("rust: {other}"))),
    }
}

/// Generated cases per run (the default 16 hardly reach every operation).
const MIN_GENERATED: usize = 160;

/// A one-face B-rep for `ShrinkGeometry` whose trim domain lies inside, across or
/// wholly outside the surface's natural domain (the disjoint case makes
/// `SmExtent2d::Intersect` fail and `Init()` the face domain).
fn shrink_face(rng: &mut Rng) -> BrepIn {
    let surface = shared::gen_surface(rng);
    let [u0, v0, u1, v1] = shared::domain(&surface);
    let (du, dv) = (u1 - u0, v1 - v0);
    let dom = match rng.below(3) {
        // inside, within a thousandth of the size of the natural boundary (snapped)
        0 => [
            u0 + du * 1e-4,
            v0 + dv * 2e-4,
            u1 - du * 1e-4,
            v1 - dv * 3e-4,
        ],
        // across the natural boundary
        1 => [u0 - du * 0.25, v0 + dv * 0.1, u1 - du * 0.5, v1 + dv * 0.2],
        // wholly outside (disjoint)
        _ => [u1 + du * 0.5, v1 + dv * 0.5, u1 + du * 1.5, v1 + dv * 1.5],
    };
    BrepIn::Face { surface, dom }
}

/// The owner of the surface of face `index`: the face's arena identity, which
/// `SmBrep::GetSurfaces` (`topology::brep::p1::surfaces`) tests the owner against.
fn face_owner(index: u32) -> ObjectRef {
    arena_identity(CacheObject::Face(FaceId::from_index(index)))
}

/// The importer names an owned surface's face `1 + index`; the B-rep names it by its
/// arena identity ([`face_owner`]). Surfaces the C++ face owns stay owned.
fn own_surfaces(b: &mut Brep) -> Result<(), HarnessError> {
    let ids: Vec<FaceId> = b.faces().ids().collect();
    for f in ids {
        let imported = ObjectRef {
            type_code: solidcore::topology::face::p2::TYPE,
            key: ObjectKey(1 + u64::from(f.index())),
        };
        if let Some(s) = b.face_mut(f).map_err(|e| err(&e))?.surface.as_mut() {
            if s.owner() == Some(imported) {
                let _ = s.set_owner(Some(face_owner(f.index())));
            }
        }
    }
    Ok(())
}

/// The damage of the shim, on the imported model: the face's surface is dropped, or the
/// shell's region link is cleared (the region keeps listing the shell, as upstream's does).
fn damage(b: &mut Brep, d: Option<DamageIn>) -> Result<(), HarnessError> {
    let index = |i: i64| u32::try_from(i).map_err(|_| err("damaged record index"));
    match d {
        None => {}
        Some(DamageIn::NoSurface(f)) => {
            b.face_mut(FaceId::from_index(index(f)?))
                .map_err(|e| err(&e))?
                .surface = None;
        }
        Some(DamageIn::NoRegion(s)) => {
            b.shell_mut(ShellId::from_index(index(s)?))
                .map_err(|e| err(&e))?
                .region = None;
        }
    }
    Ok(())
}

fn key_of(e: EntityRef) -> ObjectRef {
    let (t, i) = match e {
        EntityRef::Edge(x) => (1u64, u64::from(x.index())),
        EntityRef::Vertex(x) => (2, u64::from(x.index())),
        EntityRef::Edgeuse(x) => (3, u64::from(x.index())),
        EntityRef::Vertexuse(x) => (4, u64::from(x.index())),
        // the owner the importer gives a face's surface (`import` in `topology_vertexuse`)
        EntityRef::Face(x) => return face_owner(x.index()),
        _ => (6, 0),
    };
    ObjectRef {
        type_code: 0,
        key: ObjectKey((t << 32) | i),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p4"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (path, blk) = block(&i.brep)?;
        let out = ffi::eval(&path, &blk, request(&i)).map_err(|e| err(&e))?;
        let Some((&status, rest)) = out.split_first() else {
            return Err(err("empty shim output"));
        };
        if status == -1.0 {
            return Ok(Output::rejected("fixture B-rep did not build"));
        }
        if status == NULL_DEREFERENCE {
            // the guard: upstream dereferences NULL on this damaged B-rep, nothing was run
            return Ok(Output::rejected(
                "upstream dereferences NULL on the damaged B-rep (FFI guard)",
            ));
        }
        if i.op == OpIn::Destroy {
            return Ok(status_output(status).values("doing_boolean", rest.to_vec()));
        }
        let mut o = status_output(status);
        // the records follow a success and, for the edits, a failed status too
        if status == 0.0 || edits(i.op) {
            // the four outputs of `StitchFaces`, the ten counts, the records
            let n_extra = if i.op == OpIn::StitchFaces { 4 } else { 0 };
            let (extra, rest) = rest.split_at(n_extra.min(rest.len()));
            let (cnt, records) = rest.split_at(10.min(rest.len()));
            if i.op == OpIn::StitchFaces {
                o = o.values("stitch", extra.to_vec());
            }
            o = o.ints("counts", cnt.iter().map(|&x| x as i64).collect::<Vec<_>>());
            let mut model = parse_records(records)?;
            canonicalize(&mut model);
            o = records_output(o, &model);
        }
        Ok(o)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (path, blk) = block(&i.brep)?;
        let f = ffi::export(&path, &blk).map_err(|e| err(&e))?;
        if counts(&f.graph).is_none() {
            return Ok(Output::new());
        }
        let mut b = import(&f)?;
        own_surfaces(&mut b)?;
        damage(&mut b, i.damage)?;
        b.set_editing_enabled(i.editing);
        let pre = record_counts(&b);
        let mut k = Kernel::default();
        let mut records: BTreeMap<ObjectKey, SmTopologyRecord> = BTreeMap::new();
        let mut attributes = AttributeStore::default();
        let mut env = DeleteEnv {
            records: &mut records,
            attributes: &mut attributes,
            key_of: &key_of,
        };
        let keep = u32::try_from(i.keep).ok().map(FaceId::from_index);
        let target = u32::try_from(i.target).unwrap_or(u32::MAX);
        let r = match i.op {
            OpIn::DeleteEdge => {
                // the shim passes `keep_face->GetSurface()`: NULL for a face without one
                let keep_surface = keep.filter(|&f| b.face(f).is_ok_and(|r| r.surface.is_some()));
                p4::delete_edge(
                    &mut k,
                    &mut b,
                    &mut env,
                    EdgeId::from_index(target),
                    keep_surface,
                )
                .map(|()| None)
            }
            OpIn::DeleteVertex => {
                p4::delete_vertex(&mut k, &mut b, &mut env, VertexId::from_index(target))
                    .map(|()| None)
            }
            OpIn::ShrinkGeometry => p4::shrink_geometry(&k, &mut b).map(|()| None),
            OpIn::StitchFaces => {
                let faces = [keep];
                p4::stitch_faces(
                    &mut k,
                    &mut b,
                    i.tol,
                    keep.is_some().then_some(&faces[..]),
                    i.flag,
                )
                .map(|s| {
                    Some(vec![
                        f64::from(s.stitched_edges),
                        f64::from(s.lamina_edges),
                        s.max_vertex_edge_gap,
                        s.max_edge_face_trim_curve_gap,
                    ])
                })
            }
            OpIn::Check => Ok(None),
            OpIn::Destroy => {
                return match p4::destroy(&mut k, &mut b) {
                    Ok(()) => Ok(status_output(0.0).values(
                        "doing_boolean",
                        vec![f64::from(u8::from(k.context.doing_boolean()))],
                    )),
                    Err(e) => rust_status(&e),
                };
            }
        };
        match r {
            Ok(extra) => {
                let mut o = status_output(0.0);
                if let Some(x) = extra {
                    o = o.values("stitch", x);
                }
                let counts = record_counts(&b)
                    .iter()
                    .map(|&n| n as i64)
                    .collect::<Vec<_>>();
                let mut model = rust_records(&b, pre);
                canonicalize(&mut model);
                Ok(records_output(o.ints("counts", counts), &model))
            }
            Err(e) => {
                // a failed edit leaves the records it changed (the shim walks the B-rep
                // after a failed status of the edits as well)
                let o = rust_status(&e)?;
                if edits(i.op) && matches!(e, SolidError::Upstream { .. }) {
                    let counts = record_counts(&b)
                        .iter()
                        .map(|&n| n as i64)
                        .collect::<Vec<_>>();
                    let mut model = rust_records(&b, pre);
                    canonicalize(&mut model);
                    return Ok(records_output(o.ints("counts", counts), &model));
                }
                Ok(o)
            }
        }
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // `None` ends the generated list, so draw again past an unusable draw (a
        // fixture that does not build, or no edge / vertex to delete)
        (0..64).find_map(|_| try_generate(rng))
    }
}

/// One generated case, or `None` when the draw is unusable.
fn try_generate(rng: &mut Rng) -> Option<Value> {
    let op = match rng.below(14) {
        0..=4 => OpIn::DeleteEdge,
        5..=6 => OpIn::DeleteVertex,
        7..=9 => OpIn::ShrinkGeometry,
        10 => OpIn::StitchFaces,
        11 => OpIn::Destroy,
        _ => OpIn::Check,
    };
    let brep = if op == OpIn::ShrinkGeometry && rng.chance(0.5) {
        shrink_face(rng)
    } else {
        shared::gen_brep(rng)
    };
    let (path, blk) = block(&brep).ok()?;
    let f = ffi::export(&path, &blk).ok()?;
    let c = counts(&f.graph)?;
    let n = match op {
        OpIn::DeleteEdge => c[7],
        OpIn::DeleteVertex => c[9],
        _ => 1,
    };
    if n == 0 {
        return None;
    }
    let target = rng.below(n as u64) as i64;
    let keep = if c[3] > 0 && rng.chance(0.4) {
        rng.below(c[3] as u64) as i64
    } else {
        -1
    };
    let tol = *rng.choose(&[1.0e-6, 1.0e-3, 0.1, 0.0])?;
    let i = Inputs {
        brep,
        op,
        damage: None,
        editing: rng.below(8) != 0,
        target,
        keep,
        tol,
        flag: rng.chance(0.5),
    };
    serde_json::to_value(i).ok()
}
