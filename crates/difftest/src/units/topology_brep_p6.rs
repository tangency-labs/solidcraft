//! Native B-rep p6 operations over independently imported raw topology.
//! Closure passes compare actual statuses, output reset, shell ownership, context
//! mark advancement/locks, and every face-use mark. Copy compares only empty inputs.
//! Op 3 runs `MakeFaceTopology` one or more times (each on the B-rep the previous
//! call left) with real loops of edge uses, loop vertices, surfaces and UV curves,
//! then compares every call's status and outputs, the context marks and the whole
//! post-operation graph (the `topology.brep_p10` traversal, with edge marks).

use std::collections::BTreeMap;

use super::topology_brep_p10::{orient_code, Graph};
use super::topology_vertexuse::{self as shared, BrepIn, CrvIn, SrfIn};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::context::{MarkLock, MarkType};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p6::{self, FaceTopologyOptions};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology::Topology;
use solidcore::topology::topology_traverser::{Entity, Marks};
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, Region, RegionId, ShellId,
    VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_brep_p6::{self as ffi, Request};
use solidcore_ffi::units::topology_vertexuse::counts;

/// Differential runner for `topology.brep_p6`.
pub struct Runner;

/// One edge of a kind-5 network: between points `a` and `b`, a line unless a curve
/// is given.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct NetEdge {
    a: usize,
    b: usize,
    #[serde(default)]
    curve: Option<CrvIn>,
}

/// A kind-5 fixture: shell vertices at `points`, joined by `edges`.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct NetIn {
    points: Vec<[f64; 3]>,
    edges: Vec<NetEdge>,
}

/// One `MakeFaceTopology` call. Edge uses are `path` steps (network point pairs:
/// the `pick`-th edge use, in export order, running from the first point to the
/// second) or, without a path, raw export indices; loop vertices are network points
/// (`loop_points`) followed by raw vertex indices.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct FaceIn {
    #[serde(default)]
    surface: Option<SrfIn>,
    #[serde(default = "minus_one")]
    region: i64,
    #[serde(default)]
    calc_trims: bool,
    #[serde(default)]
    verify: bool,
    #[serde(default)]
    opposite: bool,
    #[serde(default)]
    loops: Vec<i64>,
    #[serde(default)]
    path: Vec<Vec<usize>>,
    #[serde(default)]
    edgeuses: Vec<i64>,
    #[serde(default)]
    loop_points: Vec<usize>,
    #[serde(default)]
    vertices: Vec<i64>,
    #[serde(default)]
    uv_curves: Option<Vec<Option<CrvIn>>>,
}

fn minus_one() -> i64 {
    -1
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Inputs {
    #[serde(default)]
    brep: Option<BrepIn>,
    #[serde(default)]
    net: Option<NetIn>,
    #[serde(default)]
    faces: Vec<FaceIn>,
    op: i32,
    #[serde(default)]
    seed: i64,
    #[serde(default)]
    edges: Vec<i64>,
    #[serde(default = "one")]
    repeat: i32,
    #[serde(default)]
    lock_mask: i32,
    #[serde(default)]
    flags: i32,
}

// Match Graph::find: a live object allocated after the input walk is -3,
// distinct from null (-1), even if its arena index is otherwise representable.
fn original_index(index: u32, original_count: usize) -> i64 {
    if (index as usize) < original_count {
        i64::from(index)
    } else {
        -3
    }
}

fn one() -> i32 {
    1
}
fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn brep_of(i: &Inputs) -> Result<&BrepIn, HarnessError> {
    i.brep.as_ref().ok_or_else(|| harness("no B-rep input"))
}

fn block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut out = Vec::new();
    if let Some(net) = &i.net {
        out.push(5.0);
        out.push(net.points.len() as f64);
        for p in &net.points {
            out.extend(p);
        }
        out.push(net.edges.len() as f64);
        for e in &net.edges {
            out.extend([e.a as f64, e.b as f64]);
            match &e.curve {
                None => out.push(0.0),
                Some(c) => {
                    out.push(1.0);
                    shared::push_curve(&mut out, c)?;
                }
            }
        }
        out.extend([0.0, 0.0]);
        return Ok(out);
    }
    match brep_of(i)? {
        BrepIn::File { rebuild, .. } => out.extend([0.0, f64::from(*rebuild)]),
        BrepIn::Wire { curve, t0, t1 } => {
            out.push(1.0);
            shared::push_curve(&mut out, curve)?;
            out.extend([t0, t1]);
        }
        BrepIn::Face { surface, dom } => {
            out.push(2.0);
            shared::push_surface(&mut out, surface)?;
            out.extend(dom);
        }
        BrepIn::Network => out.push(3.0),
    }
    out.extend([0.0, 0.0]);
    Ok(out)
}

// Unit-local ASCII topology fixtures use the same native file reader as upstream
// files. Keep their path under this fixture directory, without parent components.
fn path_of(i: &Inputs) -> Result<String, HarnessError> {
    if i.net.is_some() {
        return Ok(String::new());
    }
    let brep = brep_of(i)?;
    if let BrepIn::File { file, .. } = brep {
        if file.starts_with("fixtures/topology.brep_p6/") {
            let path = std::path::Path::new(file);
            if path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(harness("invalid local fixture path"));
            }
            let path = crate::case::repo_root().join(path);
            if !path.is_file() {
                return Err(harness("local fixture not found"));
            }
            return Ok(path.to_string_lossy().into_owned());
        }
    }
    shared::path_of(brep)
}

fn request(i: &Inputs, c: [usize; 10]) -> Request {
    Request {
        op: i.op,
        seed: shared::sel(i.seed, c[2]),
        indices: i.edges.iter().map(|e| shared::sel(*e, c[7])).collect(),
        repeat: i.repeat,
        lock_mask: i.lock_mask,
        flags: i.flags,
    }
}

const MARKS: [MarkType; 5] = [
    MarkType::Mark,
    MarkType::Mark2,
    MarkType::Mark3,
    MarkType::MarkIo,
    MarkType::MarkAssert,
];

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p6"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        if i.op == 3 {
            return make_faces_cpp(&i);
        }
        let path = path_of(&i)?;
        let input = block(&i)?;
        let fixture = ffi::export(&path, &input).map_err(harness)?;
        let Some(c) = counts(&fixture.graph) else {
            return Ok(Output::rejected("native fixture constructor"));
        };
        let request = request(&i, c);
        let output = ffi::eval(&path, &input, &request).map_err(harness)?;
        if output.first().is_some_and(|v| *v == -1.0 || *v == -2.0) {
            return Ok(Output::rejected(format!(
                "native fixture constructor: {output:?}"
            )));
        }
        if request.op != 0 {
            return Ok(Output::new().ints(
                "state",
                output.into_iter().map(|v| v as i64).collect::<Vec<_>>(),
            ));
        }
        // op 0: the fixed state (status, outputs, marks of each face use, face-use
        // shells, counts), then the tagged post-split graph
        let header = 3 + MARKS.len() * (2 + c[2]) + c[2] + 7;
        let mut ints: Vec<i64> = output
            .get(..header)
            .ok_or_else(|| harness("short output"))?
            .iter()
            .map(|v| *v as i64)
            .collect();
        let mut values = Vec::new();
        for pair in output.get(header..).unwrap_or(&[]).chunks(2) {
            let [tag, v] = pair else {
                return Err(harness("odd tagged output"));
            };
            if *tag == 0.0 {
                ints.extend([0, *v as i64]);
            } else {
                ints.push(1);
                values.push(*v);
            }
        }
        Ok(Output::new().ints("state", ints).values("geom", values))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        if i.op == 3 {
            return make_faces_rust(&i);
        }
        let fixture = ffi::export(&path_of(&i)?, &block(&i)?).map_err(harness)?;
        let c = counts(&fixture.graph).ok_or_else(|| harness("fixture construction failed"))?;
        let request = request(&i, c);
        let (mut b, _) = shared::import(&fixture.graph)?;
        // SmBrep.cpp:549-552: even an empty native B-rep owns a void
        // infinite region. Re-express that input fixture, not an empty arena.
        b.set_key(ObjectKey(0xB000_0000));
        let mut destination = Brep::new();
        destination.set_key(ObjectKey(0xD000_0000));
        let infinite = destination
            .add_region_record(Region {
                in_brep: true,
                is_void: 1,
                ..Region::default()
            })
            .map_err(harness)?;
        destination.set_infinite_region_record(Some(infinite));
        let mut k = Kernel::default();
        let mut marks = Marks::default();
        let mut outer = if i.lock_mask == 0 {
            None
        } else {
            Some(
                MarkLock::new(std::slice::from_mut(&mut k.context), 0, i.lock_mask)
                    .map_err(harness)?,
            )
        };
        let before = MARKS.map(|mt| k.context.current_mark(mt));
        let mut body = Vec::new();
        let mut status = 0;
        // harness env: the runner holds no attribute state (default region records,
        // empty store; see RegionNotifyState)
        let mut notify_state = solidcore::topology::brep::p2::RegionNotifyState::new(&b);
        let mut face_records = seeded_records(&b);
        let mut face_attributes = AttributeStore::default();
        for _ in 0..request.repeat {
            let result = match request.op {
                0 => p6::test_closure_split_region_attempt(
                    &mut k,
                    &mut b,
                    &mut marks,
                    &mut notify_state.env(),
                    i.flags & 1 != 0,
                    FaceuseId::from_index(request.seed as u32),
                )
                .and_then(|attempt| {
                    body = vec![
                        attempt
                            .shell
                            .map_or(-1, |s| original_index(s.index(), c[1])),
                        attempt
                            .region
                            .map_or(-1, |r| original_index(r.index(), c[0])),
                    ];
                    attempt.status
                }),
                1 => {
                    let edges: Vec<_> = request
                        .indices
                        .iter()
                        .map(|e| EdgeId::from_index(*e as u32))
                        .collect();
                    p6::copy_edges(&mut k, &b, &edges, &mut destination).map(|copied| {
                        body = if i.flags & 2 != 0 {
                            vec![copied.len() as i64]
                        } else {
                            vec![]
                        };
                    })
                }
                2 => {
                    let surface = (i.flags & 4 == 0).then(|| {
                        Surface::from(Plane::from_normal(
                            Vec3::new(2.0, -3.0, 5.0),
                            Vec3::new(0.0, 0.0, 1.0),
                        ))
                    });
                    let options = FaceTopologyOptions {
                        calculate_trims: i.flags & 1 != 0,
                        verify_radial_sectors: i.flags & 16 != 0,
                        ..FaceTopologyOptions::default()
                    };
                    let mut env = NotifyEnv {
                        records: &mut face_records,
                        attributes: &mut face_attributes,
                        context: None,
                        key_of: &key_of,
                    };
                    p6::make_face_topology(
                        &mut k,
                        &mut b,
                        &mut marks,
                        &mut env,
                        surface,
                        if i.flags & 8 == 0 {
                            OrientType::Same
                        } else {
                            OrientType::Opposite
                        },
                        &options,
                    )
                    .map(|f| {
                        body = vec![
                            1,
                            i64::from(f.shell.is_some()),
                            i64::from(f.region.is_some()),
                        ];
                    })
                }
                _ => return Err(harness("invalid operation")),
            };
            if let Err(error) = result {
                if let Some(output) = Output::unported(&error) {
                    if let Some(lock) = &mut outer {
                        lock.release(std::slice::from_mut(&mut k.context))
                            .map_err(harness)?;
                    }
                    return Ok(output);
                }
                if let SolidError::Upstream { code, .. } = error {
                    status = i64::from(code);
                    if request.op == 2 {
                        body = vec![0, 0, 0];
                    }
                    break;
                }
                return Err(harness(error));
            }
        }
        let mut output = vec![status];
        output.extend(body);
        for (mt, previous) in MARKS.into_iter().zip(before) {
            output.push(k.context.current_mark(mt).wrapping_sub(previous) as i64);
            output.push(i64::from(k.context.is_mark_locked(mt)));
            for fu in b.faceuses().ids().take(c[2]) {
                output.push(i64::from(marks.is_marked(&k, Entity::Faceuse(fu), mt)));
            }
        }
        for (_, fu) in b.faceuses().iter().take(c[2]) {
            output.push(fu.shell.map_or(-1, |s| original_index(s.index(), c[1])));
        }
        output.extend([
            b.faces().len() as i64,
            b.edges().len() as i64,
            b.vertices().len() as i64,
            destination.edges().len() as i64,
            destination.regions().len() as i64,
            destination
                .infinite_region()
                .map_or(-1, |r| i64::from(r.index())),
            i64::from(destination.region(infinite).map_err(harness)?.is_void()),
        ]);
        if let Some(lock) = &mut outer {
            lock.release(std::slice::from_mut(&mut k.context))
                .map_err(harness)?;
        }
        if request.op != 0 {
            return Ok(Output::new().ints("state", output));
        }
        // op 0: the whole post-split graph
        let g = Graph::walk(&b).map_err(harness)?;
        let mut w = W::default();
        export_graph(&b, &g, &mut w).map_err(harness)?;
        output.extend(w.ints);
        Ok(Output::new().ints("state", output).values("geom", w.values))
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        if rng.chance(0.4) {
            return serde_json::to_value(gen_faces(rng)).ok();
        }
        let brep = shared::gen_brep(rng);
        let op = match &brep {
            BrepIn::Network | BrepIn::Wire { .. } => 1 + rng.below(2),
            _ => rng.below(3),
        } as i32;
        let edges = if op == 1 && rng.chance(0.7) {
            (0..=rng.below(3)).map(|_| rng.below(20) as i64).collect()
        } else {
            vec![]
        };
        let i = Inputs {
            brep: Some(brep),
            net: None,
            faces: Vec::new(),
            op,
            seed: rng.below(20) as i64,
            edges,
            repeat: 1 + rng.below(2) as i32,
            lock_mask: *rng.choose(&[0, 1, 2, 4, 8, 16])?,
            flags: rng.below(32) as i32,
        };
        serde_json::to_value(i).ok()
    }
}

// ---- op 3: MakeFaceTopology ---------------------------------------------------------------

/// The identity of each entity under which the runner notifies it: a face is
/// `(SmFace_TYPE, 1 + index)`, the owner key the fixture import gives face surfaces;
/// every other kind gets its own key range.
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, index) = match e {
        EntityRef::Face(x) => {
            return ObjectRef {
                type_code: solidcore::topology::face::p2::TYPE,
                key: ObjectKey(1 + u64::from(x.index())),
            }
        }
        EntityRef::Brep(x) => (16011, 1u64, x.index()),
        EntityRef::Region(x) => (16012, 2, x.index()),
        EntityRef::Shell(x) => (16013, 3, x.index()),
        EntityRef::Faceuse(x) => (16022, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey((kind << 40) + u64::from(index)),
    }
}

/// `SmPlane(O, N)` or the B-spline surface of the shim's `read_surface`.
fn surface_of(s: &SrfIn) -> Result<Surface, HarnessError> {
    match s {
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(
            Vec3::new(o[0], o[1], o[2]),
            Vec3::new(n[0], n[1], n[2]),
        ))),
        SrfIn::Bspline(b) => {
            let net = b
                .net
                .iter()
                .map(|row| row.iter().map(|p| CPoint::new(p[0], p[1], p[2], p[3])).collect())
                .collect();
            let nl = NlSurface::new(net, b.p, b.q, b.u.clone(), b.v.clone()).map_err(harness)?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = b.oob;
            let form = BSplineSurfaceForm::from_code(u64::from(b.form))
                .ok_or_else(|| harness("bad surface form"))?;
            s.set_b_spline_surface_form(form);
            Ok(Surface::from(s))
        }
    }
}

/// The `SmBSplineCurve` of the shim's `read_curve`.
fn curve_of(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let nl = NlCurve::new(pw, c.p, c.u.clone()).map_err(harness)?;
    Ok(BSplineCurve::from_nurb(nl, c.dim, false))
}

/// Each call's edge use and loop vertex export indices.
struct Resolved {
    edgeuses: Vec<i64>,
    vertices: Vec<i64>,
}

fn point_of(b: &Brep, v: Option<VertexuseId>) -> Option<Vec3> {
    let vertex = b.vertexuse(v?).ok()?.vertex?;
    Some(b.vertex(vertex).ok()?.point)
}

/// Resolve network point paths and loop points on the imported (pre-operation)
/// B-rep, whose arena indices are the export's.
fn resolve(i: &Inputs, b: &Brep) -> Result<Vec<Resolved>, HarnessError> {
    let point = |k: usize| -> Result<Vec3, HarnessError> {
        let net = i.net.as_ref().ok_or_else(|| harness("path without network"))?;
        let p = net.points.get(k).ok_or_else(|| harness("path point out of range"))?;
        Ok(Vec3::new(p[0], p[1], p[2]))
    };
    let mut out = Vec::new();
    for f in &i.faces {
        let mut edgeuses = Vec::new();
        for step in &f.path {
            let (Some(&a), Some(&z)) = (step.first(), step.get(1)) else {
                return Err(harness("path step needs two points"));
            };
            let (pa, pz) = (point(a)?, point(z)?);
            let pick = step.get(2).copied().unwrap_or(0);
            let mut found = Vec::new();
            for (id, eu) in b.edgeuses().iter() {
                let mate = solidcore::topology::edgeuse::p1::mate(b, id).map_err(harness)?;
                let end = mate.and_then(|m| b.edgeuse(m).ok()).and_then(|m| m.vertexuse);
                if point_of(b, eu.vertexuse) == Some(pa) && point_of(b, end) == Some(pz) {
                    found.push(i64::from(id.index()));
                }
            }
            edgeuses.push(
                *found
                    .get(pick)
                    .ok_or_else(|| harness(format!("no edge use {a} -> {z} #{pick}")))?,
            );
        }
        edgeuses.extend(&f.edgeuses);
        let mut vertices = Vec::new();
        for &k in &f.loop_points {
            let p = point(k)?;
            let v = b
                .vertices()
                .iter()
                .find(|(_, v)| v.point == p)
                .ok_or_else(|| harness("no vertex at loop point"))?
                .0;
            vertices.push(i64::from(v.index()));
        }
        vertices.extend(&f.vertices);
        out.push(Resolved { edgeuses, vertices });
    }
    Ok(out)
}

fn face_flags(f: &FaceIn) -> i32 {
    i32::from(f.calc_trims) | (i32::from(f.verify) << 1) | (i32::from(f.opposite) << 2)
}

fn make_faces_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let path = path_of(i)?;
    let block = block(i)?;
    let fixture = ffi::export(&path, &block).map_err(harness)?;
    if counts(&fixture.graph).is_none() {
        return Ok(Output::rejected("native fixture constructor"));
    }
    let (b, _) = shared::import(&fixture.graph)?;
    let resolved = resolve(i, &b)?;
    let mut specs = Vec::new();
    for (f, r) in i.faces.iter().zip(resolved) {
        let surface = match &f.surface {
            None => None,
            Some(s) => {
                let mut v = Vec::new();
                shared::push_surface(&mut v, s)?;
                Some(v)
            }
        };
        let uv_curves = match &f.uv_curves {
            None => None,
            Some(curves) => Some(
                curves
                    .iter()
                    .map(|c| {
                        c.as_ref()
                            .map(|c| {
                                let mut v = Vec::new();
                                shared::push_curve(&mut v, c).map(|()| v)
                            })
                            .transpose()
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        };
        specs.push(ffi::FaceSpec {
            surface,
            region: f.region,
            flags: face_flags(f),
            loops: f.loops.clone(),
            edgeuses: r.edgeuses,
            vertices: r.vertices,
            uv_curves,
        });
    }
    let out = ffi::make_faces(&path, &block, &specs).map_err(harness)?;
    match out.first().copied() {
        Some(-1.0) => return Ok(Output::rejected("native fixture constructor")),
        Some(-2.0) => {
            return Ok(Output::rejected(
                "cpp-guard: MakeFaceTopology would dereference null",
            ))
        }
        _ => {}
    }
    let n = *out.first().ok_or_else(|| harness("empty output"))? as usize;
    let header = 1 + 4 * n + 10;
    let mut ints: Vec<i64> = out
        .get(..header)
        .ok_or_else(|| harness("short output"))?
        .iter()
        .map(|v| *v as i64)
        .collect();
    let mut values = Vec::new();
    for pair in out.get(header..).unwrap_or(&[]).chunks(2) {
        let [tag, v] = pair else {
            return Err(harness("odd tagged output"));
        };
        if *tag == 0.0 {
            ints.extend([0, *v as i64]);
        } else {
            ints.push(1);
            values.push(*v);
        }
    }
    Ok(Output::new().ints("state", ints).values("geom", values))
}

/// The tagged export stream: integers (each after a `0` tag) and doubles (a `1` tag
/// in the integer list, the value in the double list).
#[derive(Default)]
struct W {
    ints: Vec<i64>,
    values: Vec<f64>,
    /// The kernel the samples are evaluated with (a default one, as the shim's context).
    kernel: Kernel,
}

impl W {
    fn i(&mut self, v: i64) {
        self.ints.extend([0, v]);
    }
    fn d(&mut self, v: f64) {
        self.ints.push(1);
        self.values.push(v);
    }
    fn list(&mut self, v: &[i64]) {
        self.i(v.len() as i64);
        for &x in v {
            self.i(x);
        }
    }
    fn point(&mut self, r: &solidcore::Result<Vec3>) {
        match *r {
            Ok(p) => {
                self.i(0);
                self.d(p.x);
                self.d(p.y);
                self.d(p.z);
            }
            Err(SolidError::Upstream { code, .. }) => self.i(i64::from(code)),
            Err(_) => self.i(-1),
        }
    }
    fn curve(&mut self, c: Option<&Curve>) {
        let Some(c) = c else {
            self.i(0);
            return;
        };
        self.i(1);
        self.i(c.type_code());
        self.i(c.dim() as i64);
        let ivl = c.natural_interval();
        self.d(ivl.min());
        self.d(ivl.max());
        for k in 0..=4 {
            let t = ivl.min() + 0.25 * f64::from(k) * (ivl.max() - ivl.min());
            self.point(&c.evaluate_point(&self.kernel, t));
        }
    }
    fn surface(&mut self, s: Option<&Surface>, owner: EntityRef) {
        let Some(s) = s else {
            self.i(0);
            return;
        };
        self.i(1);
        self.i(s.type_code());
        let dom = s.natural_uv_domain();
        let (lo, hi) = (dom.min(), dom.max());
        self.d(lo.x);
        self.d(lo.y);
        self.d(hi.x);
        self.d(hi.y);
        for a in 0..=2 {
            for bb in 0..=2 {
                let uv = Point2::new(
                    lo.x + 0.5 * f64::from(a) * (hi.x - lo.x),
                    lo.y + 0.5 * f64::from(bb) * (hi.y - lo.y),
                );
                self.point(&s.evaluate_point(&self.kernel, uv));
            }
        }
        self.i(i64::from(s.owner() == Some(key_of(owner))));
    }
}

const LOOPUSE_TYPE: i64 = 16004;
const EDGEUSE_TYPE: i64 = 16005;
const VERTEXUSE_TYPE: i64 = 16006;
const SHELL_TYPE: i64 = 16013;
const UNKNOWN_TYPE: i64 = 16999;

/// The shim's `post::export_graph`.
#[allow(clippy::too_many_lines)]
fn export_graph(b: &Brep, g: &Graph, w: &mut W) -> Result<(), SolidError> {
    use solidcore::topology::edgeuse::p1 as eu1;
    use solidcore::topology::shell::{ShellMembers, ShellType};
    use solidcore::topology::{EdgeuseOwner, LoopuseStart, VertexuseOwner};
    w.d(b.tolerance());
    for n in [
        g.regions.items.len(),
        g.shells.items.len(),
        g.faceuses.items.len(),
        g.faces.items.len(),
        g.loopuses.items.len(),
        g.loops.items.len(),
        g.edgeuses.items.len(),
        g.edges.items.len(),
        g.vertexuses.items.len(),
        g.vertices.items.len(),
    ] {
        w.i(n as i64);
    }
    for &id in &g.regions.items {
        let r = b.region(id)?;
        w.i(i64::from(b.infinite_region() == Some(id)));
        w.i(i64::from(r.is_void));
        let v: Vec<i64> = r
            .shells
            .iter()
            .map(|&x| g.shells.find(Some(x), ShellId::index))
            .collect();
        w.list(&v);
    }
    for &id in &g.shells.items {
        let s = b.shell(id)?;
        w.i(g.regions.find(s.region, RegionId::index));
        w.i(s.shell_type.code());
        let v: Vec<i64> = match (&s.shell_type, &s.members) {
            (ShellType::Faceuse, ShellMembers::Faceuses(list)) => list
                .iter()
                .map(|&x| g.faceuses.find(Some(x), FaceuseId::index))
                .collect(),
            (ShellType::Faceuse, _) => Vec::new(),
            (ShellType::Wire, ShellMembers::Edgeuse(x)) => {
                vec![g.edgeuses.find(Some(*x), EdgeuseId::index)]
            }
            (ShellType::Vertex, ShellMembers::Vertexuse(x)) => {
                vec![g.vertexuses.find(Some(*x), VertexuseId::index)]
            }
            (_, ShellMembers::Empty) => vec![-1],
            _ => vec![-4],
        };
        w.list(&v);
    }
    for &id in &g.faceuses.items {
        let f = b.faceuse(id)?;
        w.i(g.shells.find(f.shell, ShellId::index));
        w.i(g.faces.find(f.face, FaceId::index));
        w.i(g.faceuses.find(f.mate, FaceuseId::index));
        w.i(orient_code(f.orientation));
        let v: Vec<i64> = f
            .loopuses
            .iter()
            .map(|&x| g.loopuses.find(Some(x), LoopuseId::index))
            .collect();
        w.list(&v);
    }
    for &id in &g.faces.items {
        let f = b.face(id)?;
        w.surface(f.surface.as_ref(), EntityRef::Face(id));
        w.i(g.faceuses.find(f.faceuse, FaceuseId::index));
        let (lo, hi) = (f.uv_domain.min(), f.uv_domain.max());
        w.d(lo.x);
        w.d(lo.y);
        w.d(hi.x);
        w.d(hi.y);
        w.i(i64::from(f.rectangular_trim));
        w.d(f.tolerance);
    }
    for &id in &g.loopuses.items {
        let l = b.loopuse(id)?;
        w.i(g.faceuses.find(l.faceuse, FaceuseId::index));
        w.i(g.loops.find(l.loop_, LoopId::index));
        w.i(g.loopuses.find(l.mate, LoopuseId::index));
        let (ty, start) = match l.start {
            LoopuseStart::Edgeuse(x) => (EDGEUSE_TYPE, g.edgeuses.find(x, EdgeuseId::index)),
            LoopuseStart::Vertexuse(x) => {
                (VERTEXUSE_TYPE, g.vertexuses.find(x, VertexuseId::index))
            }
            LoopuseStart::Unknown => (UNKNOWN_TYPE, -1),
        };
        w.i(ty);
        w.i(start);
        w.i(orient_code(l.orientation));
    }
    for &id in &g.loops.items {
        w.i(g.loopuses.find(b.loop_(id)?.loopuse, LoopuseId::index));
    }
    for &id in &g.edgeuses.items {
        let e = b.edgeuse(id)?;
        w.i(g.edges.find(e.edge, EdgeId::index));
        let (ty, owner) = match e.owner {
            EdgeuseOwner::Loopuse(x) => (LOOPUSE_TYPE, g.loopuses.find(x, LoopuseId::index)),
            EdgeuseOwner::Shell(x) => (SHELL_TYPE, g.shells.find(x, ShellId::index)),
            EdgeuseOwner::Unknown => (UNKNOWN_TYPE, -1),
        };
        w.i(ty);
        w.i(owner);
        w.i(g.edgeuses.find(e.cw, EdgeuseId::index));
        w.i(g.edgeuses.find(e.ccw, EdgeuseId::index));
        w.i(g.vertexuses.find(e.vertexuse, VertexuseId::index));
        w.i(orient_code(e.orientation));
        w.i(g.edgeuses.find(eu1::edgeuse_next(b, id)?, EdgeuseId::index));
        w.i(g.edgeuses.find(eu1::edgeuse_last(b, id)?, EdgeuseId::index));
        w.curve(e.uv_trim_curve.as_ref());
    }
    for &id in &g.edges.items {
        let e = b.edge(id)?;
        w.d(e.tolerance);
        w.d(e.interval.min());
        w.d(e.interval.max());
        let v: Vec<i64> = e
            .edgeuses
            .iter()
            .map(|&x| g.edgeuses.find(Some(x), EdgeuseId::index))
            .collect();
        w.list(&v);
        w.curve(e.curve.as_ref());
    }
    for &id in &g.vertexuses.items {
        let u = b.vertexuse(id)?;
        w.i(g.vertices.find(u.vertex, VertexId::index));
        let (ty, owner) = match u.owner {
            VertexuseOwner::Edgeuse(x) => (EDGEUSE_TYPE, g.edgeuses.find(x, EdgeuseId::index)),
            VertexuseOwner::Loopuse(x) => (LOOPUSE_TYPE, g.loopuses.find(x, LoopuseId::index)),
            VertexuseOwner::Shell(x) => (SHELL_TYPE, g.shells.find(x, ShellId::index)),
            VertexuseOwner::Unknown => (UNKNOWN_TYPE, -1),
        };
        w.i(ty);
        w.i(owner);
    }
    for &id in &g.vertices.items {
        let v = b.vertex(id)?;
        w.d(v.point.x);
        w.d(v.point.y);
        w.d(v.point.z);
        w.d(v.tolerance);
        let l: Vec<i64> = v
            .vertexuses
            .iter()
            .map(|&x| g.vertexuses.find(Some(x), VertexuseId::index))
            .collect();
        w.list(&l);
    }
    Ok(())
}

fn make_faces_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let fixture = ffi::export(&path_of(i)?, &block(i)?).map_err(harness)?;
    counts(&fixture.graph).ok_or_else(|| harness("fixture construction failed"))?;
    let (mut b, _) = shared::import(&fixture.graph)?;
    b.set_key(ObjectKey(0xB000_0000));
    let resolved = resolve(i, &b)?;
    let mut records = seeded_records(&b);
    let mut attributes = AttributeStore::default();
    let mut k = Kernel::default();
    let mut marks = Marks::default();
    let before = MARKS.map(|mt| k.context.current_mark(mt));
    let mut calls = Vec::new();
    for (n, (f, r)) in i.faces.iter().zip(&resolved).enumerate() {
        let surface = f.surface.as_ref().map(surface_of).transpose()?;
        let uv = match &f.uv_curves {
            None => None,
            Some(curves) => Some(
                curves
                    .iter()
                    .map(|c| c.as_ref().map(curve_of).transpose())
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        };
        let loops: Vec<usize> = f.loops.iter().map(|&x| x as usize).collect();
        let edgeuses: Vec<EdgeuseId> = r
            .edgeuses
            .iter()
            .map(|&x| EdgeuseId::from_index(x as u32))
            .collect();
        let vertices: Vec<VertexId> = r
            .vertices
            .iter()
            .map(|&x| VertexId::from_index(x as u32))
            .collect();
        let options = FaceTopologyOptions {
            region: (f.region >= 0).then(|| RegionId::from_index(f.region as u32)),
            edge_loops: &loops,
            edgeuses: &edgeuses,
            loop_vertices: &vertices,
            verify_radial_sectors: f.verify,
            calculate_trims: f.calc_trims,
            uv_curves: uv.as_deref(),
            surface_key: Some(ObjectKey(0x5000_0000 + n as u64)),
        };
        let mut env = NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &key_of,
        };
        let orientation = if f.opposite {
            OrientType::Opposite
        } else {
            OrientType::Same
        };
        let attempt = match p6::make_face_topology_attempt(
            &mut k,
            &mut b,
            &mut marks,
            &mut env,
            surface,
            orientation,
            &options,
        ) {
            Ok(attempt) => attempt,
            Err(error) => return Output::unported(&error).ok_or_else(|| harness(error)),
        };
        let status = match &attempt.status {
            Ok(()) => 0,
            Err(SolidError::Upstream { code, .. }) => i64::from(*code),
            Err(error) => return Err(harness(error)),
        };
        calls.push((status, attempt.face, attempt.shell, attempt.region));
        if status != 0 {
            break;
        }
    }
    let g = Graph::walk(&b).map_err(harness)?;
    let mut w = W::default();
    w.ints.push(calls.len() as i64);
    for (status, face, shell, region) in calls {
        w.ints.extend([
            status,
            g.faces.find(face, FaceId::index),
            g.shells.find(shell, ShellId::index),
            g.regions.find(region, RegionId::index),
        ]);
    }
    for (mt, previous) in MARKS.into_iter().zip(before) {
        w.ints
            .push(k.context.current_mark(mt).wrapping_sub(previous) as i64);
        w.ints.push(i64::from(k.context.is_mark_locked(mt)));
    }
    export_graph(&b, &g, &mut w).map_err(harness)?;
    Ok(Output::new().ints("state", w.ints).values("geom", w.values))
}

/// A default `SmTopology` record for every object of `b` (and the B-rep), keyed by
/// [`key_of`]; the port adds those of the objects it makes.
fn seeded_records(b: &Brep) -> BTreeMap<ObjectKey, Topology> {
    let mut all = vec![EntityRef::Brep(solidcore::topology::BrepId::from_index(0))];
    all.extend(b.regions().ids().map(EntityRef::Region));
    all.extend(b.shells().ids().map(EntityRef::Shell));
    all.extend(b.faceuses().ids().map(EntityRef::Faceuse));
    all.extend(b.faces().ids().map(EntityRef::Face));
    all.extend(b.loopuses().ids().map(EntityRef::Loopuse));
    all.extend(b.loops().ids().map(EntityRef::Loop));
    all.extend(b.edgeuses().ids().map(EntityRef::Edgeuse));
    all.extend(b.edges().ids().map(EntityRef::Edge));
    all.extend(b.vertexuses().ids().map(EntityRef::Vertexuse));
    all.extend(b.vertices().ids().map(EntityRef::Vertex));
    all.into_iter()
        .map(|e| (key_of(e).key, Topology::default()))
        .collect()
}

// ---- op 3 generator ---------------------------------------------------------------------

fn plane_in(o: [f64; 3], n: [f64; 3]) -> SrfIn {
    SrfIn::Plane { o, n }
}

/// A closed path through network points (`[from, to]` steps).
fn cycle(points: &[usize]) -> Vec<Vec<usize>> {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &b)| vec![a, b])
        .collect()
}

fn net_of(points: Vec<[f64; 3]>, edges: &[(usize, usize)]) -> NetIn {
    NetIn {
        points,
        edges: edges
            .iter()
            .map(|&(a, b)| NetEdge { a, b, curve: None })
            .collect(),
    }
}

fn plain_face(surface: SrfIn, cycles: &[&[usize]]) -> FaceIn {
    FaceIn {
        surface: Some(surface),
        region: -1,
        calc_trims: false,
        verify: false,
        opposite: false,
        loops: cycles.iter().map(|c| c.len() as i64).collect(),
        path: cycles.iter().flat_map(|c| cycle(c)).collect(),
        edgeuses: Vec::new(),
        loop_points: Vec::new(),
        vertices: Vec::new(),
        uv_curves: None,
    }
}

/// Random face-construction sequences on wire networks: squares (with a hole or a
/// loop vertex), shared edges, open boxes, flat and saddle B-spline patches, at
/// random scales and offsets, with random flags and orientations.
fn gen_faces(rng: &mut Rng) -> Inputs {
    let s = 10f64.powf(rng.uniform(-4.0, 3.0));
    let (x, y, z) = (
        rng.uniform(-2.0, 2.0) * s,
        rng.uniform(-2.0, 2.0) * s,
        rng.uniform(-2.0, 2.0) * s,
    );
    let at = |u: f64, v: f64, w: f64| [x + u * s, y + v * s, z + w * s];
    let up = plane_in([x, y, z], [0.0, 0.0, 1.0]);
    let sq_edges = [(0, 1), (1, 2), (2, 3), (3, 0)];
    let square = vec![
        at(0.0, 0.0, 0.0),
        at(1.0, 0.0, 0.0),
        at(1.0, 1.0, 0.0),
        at(0.0, 1.0, 0.0),
    ];
    let (net, mut faces) = match rng.below(6) {
        0 => {
            let mut points = square;
            let mut f = plain_face(up, &[&[0, 1, 2, 3]]);
            if rng.chance(0.4) {
                points.push(at(rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9), 0.0));
                f.loop_points = vec![4];
            }
            (net_of(points, &sq_edges), vec![f])
        }
        1 => {
            let mut points = square;
            points.extend([
                at(0.25, 0.25, 0.0),
                at(0.75, 0.25, 0.0),
                at(0.75, 0.75, 0.0),
                at(0.25, 0.75, 0.0),
            ]);
            let mut edges = sq_edges.to_vec();
            edges.extend([(4, 5), (5, 6), (6, 7), (7, 4)]);
            (
                net_of(points, &edges),
                vec![plain_face(up, &[&[0, 1, 2, 3], &[4, 7, 6, 5]])],
            )
        }
        2 => {
            let mut points = square;
            points.extend([at(2.0, 0.0, 0.0), at(2.0, 1.0, 0.0)]);
            let mut edges = sq_edges.to_vec();
            edges.extend([(1, 4), (4, 5), (5, 2)]);
            (
                net_of(points, &edges),
                vec![
                    plain_face(up.clone(), &[&[0, 1, 2, 3]]),
                    plain_face(up, &[&[1, 4, 5, 2]]),
                ],
            )
        }
        3 => {
            let points = vec![
                at(0.0, 0.0, 0.0),
                at(1.0, 0.0, 0.0),
                at(1.0, 1.0, 0.0),
                at(0.0, 1.0, 0.0),
                at(0.0, 0.0, 1.0),
                at(1.0, 0.0, 1.0),
                at(1.0, 1.0, 1.0),
                at(0.0, 1.0, 1.0),
            ];
            let edges = [
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 0),
                (4, 5),
                (5, 6),
                (6, 7),
                (7, 4),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ];
            let sides: [([f64; 3], [f64; 3], [usize; 4]); 5] = [
                (at(0.0, 0.0, 0.0), [0.0, 0.0, -1.0], [0, 3, 2, 1]),
                (at(0.0, 0.0, 0.0), [0.0, -1.0, 0.0], [0, 1, 5, 4]),
                (at(1.0, 0.0, 0.0), [1.0, 0.0, 0.0], [1, 2, 6, 5]),
                (at(0.0, 1.0, 0.0), [0.0, 1.0, 0.0], [3, 7, 6, 2]),
                (at(0.0, 0.0, 0.0), [-1.0, 0.0, 0.0], [0, 4, 7, 3]),
            ];
            let k = 1 + rng.below(5) as usize;
            let faces = sides
                .iter()
                .take(k)
                .map(|(o, n, c)| plain_face(plane_in(*o, *n), &[c]))
                .collect();
            (net_of(points, &edges), faces)
        }
        _ => {
            // a bilinear patch through the corners (flat or a saddle)
            let h = if rng.chance(0.5) {
                0.0
            } else {
                rng.uniform(-1.0, 1.0)
            };
            let corners = [
                at(0.0, 0.0, 0.0),
                at(1.0, 0.0, 0.0),
                at(1.0, 1.0, h),
                at(0.0, 1.0, 0.0),
            ];
            let w = |p: [f64; 3]| [p[0], p[1], p[2], solidcore::nurbs::math::NOW];
            let patch = SrfIn::Bspline(shared::BssIn {
                oob: false,
                form: 0,
                net: vec![
                    vec![w(corners[0]), w(corners[3])],
                    vec![w(corners[1]), w(corners[2])],
                ],
                p: 1,
                q: 1,
                u: vec![0.0, 0.0, 1.0, 1.0],
                v: vec![0.0, 0.0, 1.0, 1.0],
            });
            (
                net_of(corners.to_vec(), &sq_edges),
                vec![plain_face(patch, &[&[0, 1, 2, 3]])],
            )
        }
    };
    for f in &mut faces {
        f.calc_trims = rng.chance(0.3);
        f.verify = rng.chance(0.3);
        if rng.chance(0.2) {
            f.region = 0;
        }
        if rng.chance(0.25) {
            // the other side: reversed loops on the opposite orientation
            f.opposite = true;
            let mut at_step = 0usize;
            for &n in &f.loops {
                let end = (at_step + n as usize).min(f.path.len());
                if let Some(chunk) = f.path.get_mut(at_step..end) {
                    chunk.reverse();
                    for step in chunk {
                        step.reverse();
                    }
                }
                at_step = end;
            }
        }
    }
    Inputs {
        brep: None,
        net: Some(net),
        faces,
        op: 3,
        seed: 0,
        edges: Vec::new(),
        repeat: 1,
        lock_mask: 0,
        flags: 0,
    }
}
