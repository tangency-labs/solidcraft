//! The Rust side of constructor scripts ([`step_import::script`]): runs a script on
//! [`BrepConstructor`] and reports the same blocks as the C++ shim
//! (`harness/cpp/shim/topology_brep_constructor.cpp`, layout in its header): the
//! geometry statuses, each call's status, a crash marker where upstream dereferences
//! null (the port's [`SolidError::InvalidInput`] of the constructor), the B-rep dump
//! (mode 0) or the `--step-corpus` stages after construction (mode 1).
//!
//! Shared by the `topology.brep_constructor` runner and `difftest --step-corpus
//! --constructor` (which also builds without the `cpp` feature).

use std::collections::BTreeMap;

use solidcore::curve::curve::Curve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Point3;
use solidcore::heal::heal_data::p1::api_heal_brep;
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep_constructor::{BrepConstructor, VertexNode};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, LoopuseStart, RegionId, ShellId,
    VertexId,
};
use solidcore::SolidError;

use step_import::model as sm;
use step_import::script::{Node, Op, Orient, RegionRef, Script, ScriptCurve};

use crate::step::{make_curve, make_surface};

/// The operation path of the constructor's own errors (upstream null dereferences).
const CONSTRUCTOR_OP: &str = "topology::brep_constructor";

/// Block tags (see the shim's header).
pub const TAG_GEOMETRY: f64 = 1.0;
/// An op's code and status.
pub const TAG_OP: f64 = 2.0;
/// The index of the op upstream crashed in.
pub const TAG_CRASH: f64 = 3.0;
/// The integer dump.
pub const TAG_TOPOLOGY: f64 = 4.0;
/// The float dump.
pub const TAG_VALUES: f64 = 5.0;
/// B-rep made, `IsManifold`.
pub const TAG_STATE: f64 = 6.0;

/// The status of a call skipped because its curve or surface was not made.
pub const SKIPPED: f64 = -3.0;

/// The Rust side stopped on unported code: `(unit, upstream symbol)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deferred(pub String, pub String);

/// How a Rust error maps to an upstream outcome.
enum Outcome {
    /// An upstream status code.
    Code(f64),
    /// Upstream dereferences null here (crashes).
    Crash,
}

fn outcome(e: &SolidError, notes: &mut Vec<String>) -> Result<Outcome, Deferred> {
    Ok(match e {
        SolidError::Unported { upstream, unit } => {
            return Err(Deferred((*unit).to_string(), (*upstream).to_string()))
        }
        SolidError::InvalidInput { operation, .. } if *operation == CONSTRUCTOR_OP => {
            Outcome::Crash
        }
        SolidError::InvalidInput { .. } => {
            Outcome::Code(f64::from(solidcore::error::SM_ERR_INVALID_INPUT))
        }
        SolidError::Upstream { code, .. } => Outcome::Code(f64::from(*code)),
        other => {
            if notes.len() < 20 {
                notes.push(other.to_string());
            }
            Outcome::Code(-1.0)
        }
    })
}

/// Output blocks being built.
#[derive(Debug, Default)]
struct Blocks(Vec<f64>);

impl Blocks {
    fn block(&mut self, tag: f64, v: &[f64]) {
        self.0.push(tag);
        self.0.push(v.len() as f64);
        self.0.extend_from_slice(v);
    }
}

fn p3(p: [f64; 3]) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

fn orient(o: Orient) -> OrientType {
    match o {
        Orient::Unknown => OrientType::Unknown,
        Orient::Same => OrientType::Same,
        Orient::Opposite => OrientType::Opposite,
    }
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

/// A 2D curve of the table, as the shim's `make_curve(.., 2, ..)`: a bounded line or a
/// B-spline (trimmed to a strict sub-interval of its knots), `z` set to zero.
fn make_curve_2d(c: &sm::Curve, bounds: Option<(f64, f64)>) -> Result<Curve, SolidError> {
    match c {
        sm::Curve::Line {
            origin,
            dir,
            magnitude,
        } => {
            let (lo, hi) = bounds.ok_or(SolidError::sm_err("difftest::constructor"))?;
            Ok(Curve::from(Line::bounded(
                Point3::new(origin[0], origin[1], 0.0),
                Point3::new(dir[0], dir[1], 0.0),
                &Extent1d::from_min_max(lo, hi),
                &BoundedLineOptions {
                    scale: *magnitude,
                    dim: 2,
                    ..BoundedLineOptions::default()
                },
            )))
        }
        sm::Curve::BSpline(b) => {
            let (k0, k1) = (
                b.knots.first().copied().unwrap_or(0.0),
                b.knots.last().copied().unwrap_or(0.0),
            );
            let trim = bounds
                .filter(|&(lo, hi)| !(lo == k0 && hi == k1))
                .map(|(lo, hi)| Extent1d::from_min_max(lo, hi));
            let pts: Vec<Point3> = b
                .points
                .iter()
                .map(|p| Point3::new(p[0], p[1], 0.0))
                .collect();
            let mults: Vec<usize> = b.mults.iter().map(|&m| m as usize).collect();
            Ok(Curve::from(BSplineCurve::create_canonical(
                &CanonicalData {
                    dim: 2,
                    degree: b.degree as usize,
                    control_points: &pts,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &mults,
                    knots: &b.knots,
                    knot_type: KnotType::Unspecified,
                    weights: b.weights.as_deref(),
                    trim,
                },
            )?))
        }
        _ => Err(SolidError::sm_err("difftest::constructor")),
    }
}

fn make_table_curve(k: &Kernel, c: &ScriptCurve) -> Result<Curve, SolidError> {
    if c.dim == 2 {
        make_curve_2d(&c.curve, c.bounds)
    } else {
        make_curve(k, &c.curve, c.bounds)
    }
}

/// The geometry status of a construction: `0` made, else its upstream status.
fn geometry_status<T>(r: &Result<T, SolidError>, notes: &mut Vec<String>) -> Result<f64, Deferred> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match outcome(e, notes)? {
            Outcome::Code(c) => Ok(c),
            Outcome::Crash => Ok(-1.0),
        },
    }
}

/// Runs `script` and returns its output blocks (mode 0 dump, mode 1 corpus stages at
/// `accuracy`) and, when the Rust side reached unported code, where (the blocks then
/// stop before the call or stage that reached it). `notes` collects the Rust errors
/// that have no upstream status.
pub fn run(
    script: &Script,
    mode: i32,
    accuracy: f64,
    notes: &mut Vec<String>,
) -> (Vec<f64>, Option<Deferred>) {
    let mut out = Blocks::default();
    let mut k = Kernel::default();
    let deferred = run_into(&mut k, script, mode, accuracy, notes, &mut out, None).err();
    (out.0, deferred)
}

/// Runs `script` and returns the kernel and the B-rep it built (none when no B-rep was
/// started or the run stopped on unported code, which is returned).
pub fn build_brep(script: &Script) -> (Kernel, Option<Brep>, Option<Deferred>) {
    let mut out = Blocks::default();
    let mut k = Kernel::default();
    let mut keep = None;
    let mut notes = Vec::new();
    let deferred = run_into(
        &mut k,
        script,
        0,
        0.0,
        &mut notes,
        &mut out,
        Some(&mut keep),
    )
    .err();
    (k, keep, deferred)
}

fn run_into(
    k: &mut Kernel,
    script: &Script,
    mode: i32,
    accuracy: f64,
    notes: &mut Vec<String>,
    out: &mut Blocks,
    keep: Option<&mut Option<Brep>>,
) -> Result<(), Deferred> {
    // geometry table: curves then surfaces
    let mut curves: Vec<Option<Curve>> = Vec::new();
    let mut surfaces: Vec<Option<Surface>> = Vec::new();
    let mut gs = Vec::new();
    for c in &script.curves {
        let r = make_table_curve(k, c);
        gs.push(geometry_status(&r, notes)?);
        curves.push(r.ok());
    }
    for s in &script.surfaces {
        let r = make_surface(k, s);
        gs.push(geometry_status(&r, notes)?);
        surfaces.push(r.ok());
    }
    out.block(TAG_GEOMETRY, &gs);

    let mut c = BrepConstructor::new();
    let mut made_brep = false;
    let mut ended: Option<Brep> = None;
    let mut regions: Vec<Option<RegionId>> = Vec::new();
    let mut edges: Vec<Option<EdgeId>> = Vec::new();
    let mut vertices: Vec<Option<VertexId>> = Vec::new();
    for (i, op) in script.ops.iter().enumerate() {
        // Ok(status) or Err(()) for a crash
        let status: Result<f64, ()> = match op {
            Op::StartBrep => {
                let r = c
                    .start_brep(k, solidcore::geom::tol::USE_DEFAULT)
                    .map(|_| ());
                if r.is_ok() {
                    made_brep = true;
                }
                pointer(&r, notes)?
            }
            Op::EndBrep => {
                let r = c.end_brep();
                let s = status(&r, notes)?;
                if let Ok(b) = r {
                    ended = Some(b);
                }
                s
            }
            Op::StartRegion { solid, outer } => {
                let outer = match outer {
                    RegionRef::Null => None,
                    RegionRef::Infinite => c.brep().and_then(Brep::infinite_region),
                    RegionRef::Index(j) => regions.get(*j).copied().flatten(),
                };
                let r = c.start_region(*solid, outer);
                regions.push(r.as_ref().ok().copied());
                pointer(&r, notes)?
            }
            Op::EndRegion => status(&c.end_region(), notes)?,
            Op::StartShell => status(&c.start_shell(), notes)?,
            Op::EndShell { stitch } => status(&c.end_shell(k, *stitch), notes)?,
            Op::StartFace(o) => pointer(&c.start_face(orient(*o)), notes)?,
            Op::EndFace => status(&c.end_face(), notes)?,
            Op::SetFaceSurface {
                surface,
                natural,
                domain,
            } => match surfaces.get_mut(*surface).and_then(Option::take) {
                None => Ok(SKIPPED),
                Some(s) => {
                    // `GetNaturalUVDomain`, which makes a lazy analytic's NURB
                    // (`Surface::natural_uv_domain_with_nurb`)
                    let d = if *natural {
                        s.natural_uv_domain_with_nurb(k)
                    } else {
                        Ok(Extent2d::from_bounds(
                            domain[0], domain[1], domain[2], domain[3],
                        ))
                    };
                    status(&d.and_then(|d| c.set_face_surface(s, &d)), notes)?
                }
            },
            Op::StartLoop(o) => pointer(&c.start_loop(orient(*o)), notes)?,
            Op::EndLoop => status(&c.end_loop(k), notes)?,
            Op::SingleVertexLoop(p) => pointer(&c.start_and_end_single_vertex_loop(p3(*p)), notes)?,
            Op::StartVertexOfLoop(p) => {
                let r = c.start_vertex_of_loop(p3(*p));
                vertices.push(r.as_ref().ok().copied());
                pointer(&r, notes)?
            }
            Op::StartEdge {
                orient: o,
                edge,
                start,
                end,
            } => {
                let get = |v: &[Option<VertexId>], x: &Option<usize>| {
                    x.and_then(|j| v.get(j).copied().flatten())
                };
                let e = edge.and_then(|j| edges.get(j).copied().flatten());
                let (s, t) = (get(&vertices, start), get(&vertices, end));
                let r = c.start_edge(k, orient(*o), e, s, t);
                edges.push(r.as_ref().ok().copied());
                pointer(&r, notes)?
            }
            Op::SetEdgeCurve { curve, uv } => match curve {
                None => status(&c.set_edge_curve(k, None, *uv), notes)?,
                Some(j) => match curves.get_mut(*j).and_then(Option::take) {
                    None => Ok(SKIPPED),
                    Some(cv) => status(&c.set_edge_curve(k, Some(cv), *uv), notes)?,
                },
            },
            Op::SetLimits(a, b) => {
                let node = |n: &Node| match n {
                    Node::Point(p) => VertexNode::from_point(p3(*p)),
                    Node::Parameter(t) => VertexNode::from_parameter(*t),
                    Node::Uv(p) => VertexNode::from_uv_point(Point2::new(p[0], p[1])),
                    Node::Empty => VertexNode::default(),
                };
                status(&c.set_limits(k, &node(a), &node(b)), notes)?
            }
            Op::EndEdge => status(&c.end_edge(), notes)?,
            Op::StartVertex(p) => {
                let r = c.start_vertex(p3(*p));
                vertices.push(r.as_ref().ok().copied());
                pointer(&r, notes)?
            }
            Op::EndVertex => {
                c.end_vertex();
                Ok(0.0)
            }
            Op::SwapRegions => {
                c.swap_regions();
                Ok(0.0)
            }
            Op::AdjustStepUvDomain { surface, domain } => {
                match surfaces.get_mut(*surface).and_then(Option::as_mut) {
                    None => Ok(SKIPPED),
                    Some(s) => {
                        let d = Extent2d::from_bounds(domain[0], domain[1], domain[2], domain[3]);
                        status(&s.adjust_step_uv_domain(k, &d), notes)?
                    }
                }
            }
        };
        let code = op_code(op);
        let Ok(st) = status else {
            out.block(TAG_CRASH, &[i as f64]);
            return Ok(());
        };
        out.block(TAG_OP, &[code, st]);
        if matches!(op, Op::EndBrep) && ended.is_some() {
            break;
        }
    }
    let is_manifold = c.is_manifold();
    let brep = ended.or_else(|| c.into_brep());
    out.block(
        TAG_STATE,
        &[
            if made_brep { 1.0 } else { 0.0 },
            if is_manifold { 1.0 } else { 0.0 },
        ],
    );
    let Some(mut b) = brep else {
        return Ok(());
    };
    if let Some(keep) = keep {
        *keep = Some(b);
        return Ok(());
    }
    if mode == 0 {
        let bound = 4 * script.ops.len() + 16;
        match dump(k, &b, bound) {
            Ok((t, v)) => {
                out.block(TAG_TOPOLOGY, &t);
                out.block(TAG_VALUES, &v);
            }
            Err(e) => {
                if let Outcome::Crash = outcome(&e, notes)? {
                    out.block(TAG_CRASH, &[script.ops.len() as f64]);
                } else {
                    notes.push(format!("dump: {e}"));
                }
            }
        }
    } else {
        stages(k, &mut b, accuracy, out, notes)?;
    }
    Ok(())
}

fn op_code(op: &Op) -> f64 {
    match op {
        Op::StartBrep => 0.0,
        Op::EndBrep => 1.0,
        Op::StartRegion { .. } => 2.0,
        Op::EndRegion => 3.0,
        Op::StartShell => 4.0,
        Op::EndShell { .. } => 5.0,
        Op::StartFace(_) => 6.0,
        Op::EndFace => 7.0,
        Op::SetFaceSurface { .. } => 8.0,
        Op::StartLoop(_) => 9.0,
        Op::EndLoop => 10.0,
        Op::SingleVertexLoop(_) => 11.0,
        Op::StartVertexOfLoop(_) => 12.0,
        Op::StartEdge { .. } => 13.0,
        Op::SetEdgeCurve { .. } => 14.0,
        Op::SetLimits(..) => 15.0,
        Op::EndEdge => 16.0,
        Op::StartVertex(_) => 17.0,
        Op::EndVertex => 18.0,
        Op::SwapRegions => 19.0,
        Op::AdjustStepUvDomain { .. } => 20.0,
    }
}

/// A call returning a pointer: `0` non-null, `1` `NULL`; `Err(())` a crash.
fn pointer<T>(
    r: &Result<T, SolidError>,
    notes: &mut Vec<String>,
) -> Result<Result<f64, ()>, Deferred> {
    Ok(match r {
        Ok(_) => Ok(0.0),
        Err(e) => match outcome(e, notes)? {
            Outcome::Code(_) => Ok(1.0),
            Outcome::Crash => Err(()),
        },
    })
}

/// A call returning a status; `Err(())` a crash.
fn status<T>(
    r: &Result<T, SolidError>,
    notes: &mut Vec<String>,
) -> Result<Result<f64, ()>, Deferred> {
    Ok(match r {
        Ok(_) => Ok(0.0),
        Err(e) => match outcome(e, notes)? {
            Outcome::Code(c) => Ok(c),
            Outcome::Crash => Err(()),
        },
    })
}

/// First-seen indices of one kind of entity.
struct Ix<T: Ord + Copy> {
    map: BTreeMap<T, f64>,
    order: Vec<T>,
}

impl<T: Ord + Copy> Ix<T> {
    fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            order: Vec::new(),
        }
    }

    fn get(&mut self, x: Option<T>) -> f64 {
        let Some(x) = x else {
            return -1.0;
        };
        if let Some(&i) = self.map.get(&x) {
            return i;
        }
        let i = self.order.len() as f64;
        self.map.insert(x, i);
        self.order.push(x);
        i
    }
}

/// An index of a computed neighbour; `-5` when the port fails to compute it.
fn computed(r: &Result<Option<EdgeuseId>, SolidError>, ix: &mut Ix<EdgeuseId>) -> f64 {
    match r {
        Ok(x) => ix.get(*x),
        Err(_) => -5.0,
    }
}

/// The integer and float dumps (layout in the shim's header).
fn dump(k: &Kernel, b: &Brep, bound: usize) -> Result<(Vec<f64>, Vec<f64>), SolidError> {
    let (mut t, mut v) = (Vec::new(), Vec::new());
    let mut regions: Ix<RegionId> = Ix::new();
    let mut shells: Ix<ShellId> = Ix::new();
    let mut faceuses: Ix<FaceuseId> = Ix::new();
    let mut faces: Ix<FaceId> = Ix::new();
    let mut loopuses: Ix<LoopuseId> = Ix::new();
    let mut loops: Ix<LoopId> = Ix::new();
    let mut edgeuses: Ix<EdgeuseId> = Ix::new();
    let mut edges: Ix<EdgeId> = Ix::new();
    let mut vertices: Ix<VertexId> = Ix::new();

    let rl = bp1::regions(b, None)?;
    t.push(rl.len() as f64);
    for r in rl {
        t.push(regions.get(Some(r)));
        let rec = b.region(r)?;
        t.push(f64::from(rec.is_void()));
        let sl = rec.get_shells(None, |_, _| Ok(true))?;
        t.push(sl.len() as f64);
        for s in sl {
            t.push(shells.get(Some(s)));
            let srec = b.shell(s)?;
            let fus = srec.is_faceuse_shell();
            t.push(if fus { 1.0 } else { 0.0 });
            if !fus {
                continue;
            }
            let mut ful = srec.get_faceuses()?;
            ful.truncate(bound);
            t.push(ful.len() as f64);
            for fu in ful {
                let furec = b.faceuse(fu)?;
                t.push(faceuses.get(Some(fu)));
                t.push(orient_code(furec.orientation));
                t.push(faces.get(furec.face));
                t.push(faceuses.get(furec.mate));
                let mut lul = furec.loopuses.clone();
                lul.truncate(bound);
                t.push(lul.len() as f64);
                for lu in lul {
                    let lurec = *b.loopuse(lu)?;
                    t.push(loopuses.get(Some(lu)));
                    t.push(orient_code(lurec.orientation));
                    let code = match lurec.start {
                        LoopuseStart::Unknown => 0.0,
                        LoopuseStart::Edgeuse(_) => 1.0,
                        LoopuseStart::Vertexuse(_) => 2.0,
                    };
                    t.push(code);
                    t.push(loops.get(lurec.loop_));
                    t.push(loopuses.get(lurec.mate));
                    match lurec.start {
                        LoopuseStart::Edgeuse(start) => {
                            let mut eul = Vec::new();
                            let mut closed = false;
                            let mut x = start;
                            while let Some(eu) = x {
                                if eul.len() >= bound {
                                    break;
                                }
                                eul.push(eu);
                                x = b.edgeuse(eu)?.ccw;
                                if x == start {
                                    closed = true;
                                    break;
                                }
                            }
                            if !closed {
                                t.push(-9.0);
                                continue;
                            }
                            t.push(eul.len() as f64);
                            for eu in eul {
                                let rec = b.edgeuse(eu)?;
                                t.push(edgeuses.get(Some(eu)));
                                t.push(orient_code(rec.orientation));
                                t.push(edges.get(rec.edge));
                                let vtx = match rec.vertexuse {
                                    Some(vu) => b.vertexuse(vu)?.vertex,
                                    None => None,
                                };
                                t.push(vertices.get(vtx));
                                let (cw, has_uv) = (rec.cw, rec.uv_trim_curve.is_some());
                                t.push(computed(&eu1::mate(b, eu), &mut edgeuses));
                                t.push(computed(&eu1::radial(b, eu), &mut edgeuses));
                                t.push(edgeuses.get(cw));
                                t.push(if has_uv { 1.0 } else { 0.0 });
                            }
                        }
                        LoopuseStart::Vertexuse(vu) => {
                            let vtx = match vu {
                                Some(vu) => b.vertexuse(vu)?.vertex,
                                None => None,
                            };
                            t.push(vertices.get(vtx));
                        }
                        LoopuseStart::Unknown => {}
                    }
                }
            }
        }
    }
    let fl = bp1::faces(b, None)?;
    let el = bp1::edges(b, None)?;
    let vl = bp1::vertices(b, None)?;
    t.push(fl.len() as f64);
    for f in fl {
        t.push(faces.get(Some(f)));
    }
    t.push(el.len() as f64);
    for e in el {
        t.push(edges.get(Some(e)));
    }
    t.push(vl.len() as f64);
    for x in vl {
        t.push(vertices.get(Some(x)));
    }

    let mut i = 0;
    while let Some(&e) = edges.order.get(i) {
        i += 1;
        let rec = b.edge(e)?;
        let mut eul = rec.edgeuses.clone();
        eul.truncate(bound);
        t.push(eul.len() as f64);
        for eu in eul {
            t.push(edgeuses.get(Some(eu)));
        }
        let ivl = rec.interval;
        t.push(if rec.curve.is_some() { 1.0 } else { 0.0 });
        v.push(ivl.min());
        v.push(ivl.max());
        v.push(rec.tolerance);
        let eval = rec.curve.is_some()
            && ivl.min() <= ivl.max()
            && ivl.min().abs() < 1.0e20
            && ivl.max().abs() < 1.0e20;
        t.push(if eval { 1.0 } else { 0.0 });
        if let (true, Some(c)) = (eval, rec.curve.as_ref()) {
            for x in [ivl.min(), ivl.max()] {
                match c.evaluate_point(k, x) {
                    Ok(p) => {
                        t.push(0.0);
                        v.extend_from_slice(&[p.x, p.y, p.z]);
                    }
                    Err(e) => {
                        let mut ignored = Vec::new();
                        t.push(match outcome(&e, &mut ignored) {
                            Ok(Outcome::Code(c)) => c,
                            _ => -1.0,
                        });
                        v.extend_from_slice(&[0.0, 0.0, 0.0]);
                    }
                }
            }
        }
    }
    for &x in &vertices.order {
        let rec = b.vertex(x)?;
        t.push(rec.vertexuses.len().min(bound) as f64);
        v.extend_from_slice(&[rec.point.x, rec.point.y, rec.point.z, rec.tolerance]);
    }
    for &f in &faces.order {
        let rec = b.face(f)?;
        t.push(if rec.surface.is_some() { 1.0 } else { 0.0 });
        let (lo, hi) = (rec.uv_domain.min(), rec.uv_domain.max());
        v.extend_from_slice(&[lo.x, lo.y, hi.x, hi.y, rec.tolerance]);
    }
    v.push(b.tolerance());
    Ok((t, v))
}

fn counts(k: &mut Kernel, b: &Brep) -> Result<Vec<f64>, SolidError> {
    Ok(vec![
        bp1::regions(b, None)?.len() as f64,
        bp1::shells(k, b, None)?.len() as f64,
        bp1::faces(b, None)?.len() as f64,
        bp1::edges(b, None)?.len() as f64,
        bp1::vertices(b, None)?.len() as f64,
    ])
}

/// The status of a stage call (as `step.rs` reports it).
fn stage_status<T>(r: &Result<T, SolidError>, notes: &mut Vec<String>) -> Result<f64, Deferred> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match outcome(e, notes)? {
            Outcome::Code(c) => Ok(c),
            Outcome::Crash => Ok(f64::from(solidcore::error::SM_ERR_INVALID_INPUT)),
        },
    }
}

/// The `--step-corpus` stages after construction (blocks 101..108).
fn stages(
    k: &mut Kernel,
    b: &mut Brep,
    accuracy: f64,
    out: &mut Blocks,
    notes: &mut Vec<String>,
) -> Result<(), Deferred> {
    let c = counts(k, b);
    stage_status(&c, notes)?;
    out.block(101.0, &c.unwrap_or_default());
    let m = brep_is_manifold_solid(b);
    let s = stage_status(&m, notes)?;
    out.block(102.0, &[s, f64::from(u8::from(m.unwrap_or(false)))]);
    let a = brep_area(k, b, accuracy);
    let s = stage_status(&a, notes)?;
    out.block(103.0, &[s, a.unwrap_or(0.0)]);
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let vol = brep_volume(k, b, accuracy, &attrs);
    let s = stage_status(&vol, notes)?;
    out.block(104.0, &[s, vol.unwrap_or(0.0)]);
    let h = api_heal_brep(k, Some(b));
    let s = stage_status(&h, notes)?;
    out.block(105.0, &[s]);
    let c = counts(k, b);
    stage_status(&c, notes)?;
    out.block(106.0, &c.unwrap_or_default());
    let m = brep_is_manifold_solid(b);
    let s = stage_status(&m, notes)?;
    out.block(107.0, &[s, f64::from(u8::from(m.unwrap_or(false)))]);
    let vol = brep_volume(k, b, accuracy, &attrs);
    let s = stage_status(&vol, notes)?;
    out.block(108.0, &[s, vol.unwrap_or(0.0)]);
    Ok(())
}

/// Splits output blocks into `(tag, values)`.
///
/// # Errors
///
/// A malformed block list.
pub fn blocks(out: &[f64]) -> Result<Vec<(i64, Vec<f64>)>, String> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < out.len() {
        let (Some(&tag), Some(&n)) = (out.get(i), out.get(i + 1)) else {
            return Err("truncated block".into());
        };
        if !(tag.fract() == 0.0 && n >= 0.0 && n.fract() == 0.0) {
            return Err(format!("bad block header {tag} {n}"));
        }
        let end = (i + 2)
            .checked_add(n as usize)
            .ok_or("block length overflow")?;
        let vals = out.get(i + 2..end).ok_or("truncated block")?;
        v.push((tag as i64, vals.to_vec()));
        i = end;
    }
    Ok(v)
}
