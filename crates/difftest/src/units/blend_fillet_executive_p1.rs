//! Runner for unit `blend.fillet_executive_p1`. C++:
//! `solidcore_ffi::units::blend_fillet_executive_p1`. Rust:
//! `solidcore::blend::fillet_executive::p1`.
//!
//! Two kinds of case. A law case (`law_eval`) builds a law (base, linear or B-spline,
//! with or without an edge map), evaluates it at a sequence of parameters over edge
//! intervals (forward or reversed), then dumps the extension the B-spline law cached
//! and `IsKindOf` answers. An object case (`object_eval`) runs a script of `NoteError`
//! calls on an `SmFilletErrorInfo`, or of those and the setters on a new
//! `SmFilletExecutive`, and dumps the resulting members.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_executive::p1::{
    ExecutiveObjects, FilletLaw, FilletLawKind, BSPLINE_FILLET_LAW_TYPE, FILLET_LAW_TYPE,
    LINEAR_FILLET_LAW_TYPE,
};
use solidcore::blend::fillet_executive::p3::{ErrorInfo, FilletExecutive};
use solidcore::blend::fillet_geom::p1::{
    FilStatus, FilletCornerType, FilletEdge, FilletEdgeType, FilletErrorType,
};
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::geom::vector3d::Point3;
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::OBJECT_TYPE;
use solidcore::math::EFF_ZERO;
use solidcore::nurbs::math::NOW;
use solidcore_ffi::units::blend_fillet_executive_p1 as ffi;

use super::blend_fillet_standard_solver_p1::{
    bcurve_of, dump_curve, push_crv, status, uniform_knots, InlineCrv,
};
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// `blend.fillet_executive_p1` runner.
pub struct Runner;

/// The law to build.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LawIn {
    /// `SmFilletLaw()`.
    Base,
    /// `SmLinearFilletLaw(r0, r1, map)`.
    Linear { r0: f64, r1: f64 },
    /// `SmBSplineFilletLaw(curve, map)` (`[0, 0]`, i.e. none, without a map).
    Bspline { c: InlineCrv },
}

/// One `Evaluate(t, ivl, reverse)` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Call {
    t: f64,
    ivl: [f64; 2],
    reverse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    law: LawIn,
    /// The edge map (`pOptEdgeMap` / `crEdgeMap`); `None` = not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    map: Option<[f64; 2]>,
    calls: Vec<Call>,
    /// Type codes for `IsKindOf`.
    kinds: Vec<i64>,
}

/// A case: a law (`law_eval`) or an object script (`object_eval`).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum CaseIn {
    Loader {
        loader: super::blend_fillet_executive_p2::LoaderScript,
    },
    Law(Inputs),
    Object(ObjectIn),
}

fn cpp_input(i: &Inputs) -> Vec<f64> {
    let mut o = Vec::new();
    match &i.law {
        LawIn::Linear { r0, r1 } => o.extend([0.0, *r0, *r1]),
        LawIn::Bspline { c } => {
            o.push(1.0);
            push_crv(&mut o, c);
        }
        LawIn::Base => o.push(2.0),
    }
    match i.map {
        Some([a, b]) => o.extend([1.0, a, b]),
        None => o.extend([0.0, 0.0, 0.0]),
    }
    o.push(i.calls.len() as f64);
    for c in &i.calls {
        o.extend([c.t, c.ivl[0], c.ivl[1], flag(c.reverse)]);
    }
    o.push(i.kinds.len() as f64);
    o.extend(i.kinds.iter().map(|&k| k as f64));
    o
}

pub(crate) fn to_output(flat: &[f64]) -> Output {
    let (status, rest) = flat
        .split_first()
        .map_or((f64::NAN, &[][..]), |(s, r)| (*s, r));
    Output::new()
        .values("status", vec![status])
        .values("out", rest.to_vec())
}

/// The law the shim builds. Linear laws go through `SmLinearFilletLaw(r0, r1,
/// pOptEdgeMap)` ([`FilletLaw::linear`], `blend.fillet_executive_p2`). The
/// `SmBSplineFilletLaw(pCurve, crEdgeMap)` constructor is not ported yet, so the
/// harness mirrors its one rule: an edge map shorter than `SM_EFF_ZERO` (or none)
/// means `[0, 1]` (`SmFilletExecutive.cpp:5682`).
fn law_of(i: &Inputs) -> Result<FilletLaw, HarnessError> {
    let map = i.map.map(|[a, b]| Extent1d::from_min_max(a, b));
    Ok(match &i.law {
        LawIn::Base => FilletLaw::new(FilletLawKind::Base),
        LawIn::Linear { r0, r1 } => FilletLaw::linear(*r0, *r1, map.as_ref()),
        LawIn::Bspline { c } => {
            let mut law = FilletLaw::new(FilletLawKind::BSpline {
                law_curve: bcurve_of(c)?,
                extended_law: None,
            });
            if let Some(map) = map.filter(|m| m.length() >= EFF_ZERO) {
                law.edge_map = map;
            }
            law
        }
    })
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut law = law_of(i)?;
    let mut o = vec![
        law.edge_map.min(),
        law.edge_map.max(),
        flag(law.is_linear_law().map_err(|e| HarnessError::new(e.to_string()))?),
        law.type_code() as f64,
    ];
    for c in &i.calls {
        let ivl = Extent1d::from_min_max(c.ivl[0], c.ivl[1]);
        match law.evaluate(c.t, &ivl, c.reverse) {
            Ok(v) => {
                o.push(0.0);
                o.extend(v);
            }
            Err(e) => {
                if let Some(out) = Output::unported(&e) {
                    return Ok(out);
                }
                o.push(status(&e));
            }
        }
    }
    dump_curve(&mut o, law.extended_curve());
    o.extend(i.kinds.iter().map(|&k| flag(law.is_kind_of(k))));
    Ok(Output::new().values("status", vec![0.0]).values("out", o))
}

// ---- generator ------------------------------------------------------------------------

/// A random clamped law curve: radii as x over `[a, b]`, degree 1..=5, sometimes rational
/// or with repeated interior knots.
fn gen_law_curve(rng: &mut Rng, scale: f64, a: f64, b: f64) -> InlineCrv {
    let p = 1 + rng.below(5) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|_| {
            let w = if rational {
                rng.uniform(0.5, 2.0)
            } else {
                NOW
            };
            let x = scale * rng.uniform(-0.5, 2.0);
            // the law reads x only; stray y/z must not matter
            let y = if rng.chance(0.3) { rng.uniform(-1.0, 1.0) } else { 0.0 };
            [x, y, 0.0, w]
        })
        .collect();
    let mut u = uniform_knots(n, p, a, b);
    if rng.chance(0.3) && n > p && p >= 2 {
        // repeat one interior knot (multiplicity 2 <= p)
        let i = p + 1 + rng.below((n - p) as u64) as usize;
        if let Some(&k) = u.get(i) {
            if let Some(next) = u.get_mut(i + 1) {
                if *next != b {
                    *next = k;
                }
            }
        }
    }
    InlineCrv { cpts, p, u }
}

fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(5) {
        0 => 10f64.powf(rng.uniform(-6.0, -2.0)),
        1 => 10f64.powf(rng.uniform(2.0, 6.0)),
        _ => 10f64.powf(rng.uniform(-1.0, 1.0)),
    }
}

fn gen_calls(rng: &mut Rng, domain: [f64; 2]) -> Vec<Call> {
    let n = 1 + rng.below(5) as usize;
    let len = domain[1] - domain[0];
    (0..n)
        .map(|_| {
            let (i0, i1) = if rng.chance(0.08) {
                let v = rng.uniform(-3.0, 3.0) * len.max(1e-3);
                (v, v)
            } else {
                let i0 = rng.uniform(-2.0, 2.0) * len.max(1e-3);
                (i0, i0 + rng.uniform(0.1, 3.0) * len.max(1e-3))
            };
            let t = match rng.below(10) {
                0..=5 => i0 + rng.unit() * (i1 - i0),
                6 => i0,
                7 => i1,
                // beyond the interval: the law is extended (or snaps when within tolerance)
                8 => i1 + rng.uniform(0.0, 3.0) * (i1 - i0),
                _ => i0 - rng.uniform(0.0, 3.0) * (i1 - i0),
            };
            let ivl = if rng.chance(0.1) { [i1, i0] } else { [i0, i1] };
            Call {
                t,
                ivl,
                reverse: rng.chance(0.4),
            }
        })
        .collect()
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_executive_p1"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let flat = match case.parse()? {
            CaseIn::Loader { loader } => super::blend_fillet_executive_p2::loader_cpp(&loader),
            CaseIn::Law(i) => ffi::law_eval(&cpp_input(&i)),
            CaseIn::Object(i) => ffi::object_eval(i.parts().0, &object_cpp_input(&i)),
        }
        .map_err(|e| HarnessError::new(e.to_string()))?;
        Ok(to_output(&flat))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        match case.parse()? {
            CaseIn::Loader { loader } => super::blend_fillet_executive_p2::loader_rust(&loader),
            CaseIn::Law(i) => rust(&i),
            CaseIn::Object(i) => object_rust(&i),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        if rng.chance(0.5) {
            return serde_json::to_value(gen_object(rng)).ok();
        }
        let scale = gen_scale(rng);
        let a = rng.uniform(-5.0, 5.0) * 10f64.powf(rng.uniform(-2.0, 2.0));
        let b = a + rng.uniform(0.1, 10.0) * 10f64.powf(rng.uniform(-2.0, 2.0));
        let (law, domain) = match rng.below(10) {
            0 => (LawIn::Base, [0.0, 1.0]),
            1..=4 => (
                LawIn::Linear {
                    r0: scale * rng.uniform(-1.0, 2.0),
                    r1: scale * rng.uniform(-1.0, 2.0),
                },
                [0.0, 1.0],
            ),
            _ => (
                LawIn::Bspline {
                    c: gen_law_curve(rng, scale, a, b),
                },
                [a, b],
            ),
        };
        let map = match rng.below(10) {
            0..=2 => None,
            3..=4 => Some(domain),
            5 => {
                // zero or tiny length around SM_EFF_ZERO: below it the constructors fall
                // back to [0, 1]
                let v = domain[0] + rng.unit() * (domain[1] - domain[0]);
                let len = if rng.chance(0.5) {
                    0.0
                } else {
                    10f64.powf(rng.uniform(-13.0, -11.0))
                };
                Some([v, v + len])
            }
            6 if rng.chance(0.5) => Some([domain[1], domain[0]]),
            _ => {
                let len = domain[1] - domain[0];
                let m0 = domain[0] + rng.uniform(-0.5, 0.8) * len;
                Some([m0, m0 + rng.uniform(0.05, 1.5) * len])
            }
        };
        let edge0 = rng.uniform(-10.0, 10.0);
        let edge1 = edge0 + rng.uniform(0.1, 20.0);
        let calls = gen_calls(rng, [edge0, edge1]);
        let mut kinds = vec![
            FILLET_LAW_TYPE,
            LINEAR_FILLET_LAW_TYPE,
            BSPLINE_FILLET_LAW_TYPE,
            OBJECT_TYPE,
        ];
        kinds.push(rng.range_i64(0, 20000));
        serde_json::to_value(Inputs {
            law,
            map,
            calls,
            kinds,
        })
        .ok()
    }
}

// ---- error info and executive object scripts -----------------------------------------------

/// A handle the shim turns into a pointer (`0` = NULL).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct H(u32);
impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        ClassObjectKind::Other
    }
}
impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Other
    }
}
/// The executive itself (`-1` in the output) and the objects its constructor makes.
const SELF: H = H(u32::MAX);
const PSEUDO: H = H(u32::MAX - 1);
const ATTRIBUTE: H = H(u32::MAX - 2);
/// The k-th fillet edge a script made is `H(MADE - k)` (`-2 - k` in the output).
const MADE: u32 = u32::MAX - 16;

fn opt(h: u32) -> Option<H> {
    (h != 0).then_some(H(h))
}

fn out_handle(h: Option<H>) -> f64 {
    match h {
        None => 0.0,
        Some(SELF) => -1.0,
        Some(H(h)) if h <= MADE && h > MADE - ffi::MAX_SCRIPT as u32 => {
            -2.0 - f64::from(MADE - h)
        }
        Some(H(h)) => f64::from(h),
    }
}

/// One call of a script (the shim's call kinds 0-5, 10-20).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
enum Step {
    /// `NoteError(eErrorCode, cComment)`.
    Note { code: u8, comment: Option<String> },
    /// `NoteError(pBadEdge, eStatus, eErrorCode, cComment)`.
    Edge {
        edge: u32,
        status: u8,
        code: u8,
        comment: Option<String>,
    },
    /// `NoteError(eErrorCode, eEdgeStatus, pFilletedEdge, pSideFace, pFilletEdge,
    /// pFilletSurface, cComment)` (`NoteFilletError` on an executive).
    Merge {
        code: u8,
        status: u8,
        edge: u32,
        face: u32,
        fillet_edge: u32,
        surface: u32,
        comment: Option<String>,
    },
    /// `NoteError(eErrorCode, eCornerType, eFilletStatus, cCornerName, ...)`
    /// (`NoteFilletError` on an executive).
    Corner {
        code: u8,
        corner: u8,
        status: u8,
        name: Option<String>,
        vertex: u32,
        surfaces: [u32; 2],
        fillet_vertices: [u32; 2],
        comment: Option<String>,
    },
    /// `NoteError(eErrorCode, eFilletStat, eFEType, pFilletEdge, cEdgeName, cComment)`.
    FilletEdge {
        code: u8,
        status: u8,
        fe_type: u8,
        fillet_edge: u32,
        name: Option<String>,
        comment: Option<String>,
    },
    /// `NoteError(eErrorCode, eFilletStatus, pFilletedEdge, rapFilSurfs, rapFilCurvs,
    /// rapFilEdges, rTsectPos, cComment)` (`NoteFilletError` on an executive).
    Intersection {
        code: u8,
        status: u8,
        edge: u32,
        surfaces: Vec<u32>,
        curves: Vec<u32>,
        edges: Vec<u32>,
        at: [f64; 3],
        comment: Option<String>,
    },
    /// `SetDoGlobalMerge`.
    GlobalMerge { on: bool },
    /// `SetDoTrimming`.
    Trimming { on: bool },
    /// `SetSelfIntersectionHandler`.
    Handler { h: u32 },
    /// `AddFilletSurfaceGenerator`.
    AddGenerator { h: u32 },
    /// `AddTopoEdges`.
    AddTopoEdge { h: u32 },
    /// `RemoveFromTopoEdges`.
    RemoveTopoEdge { h: u32 },
    /// An `m_vVertexCornerMap` entry.
    MapCorner { v: u32, c: u32 },
    /// An `m_vEdgeuseSolverMap` entry.
    MapSolver { eu: u32, s: u32 },
    /// `GetFilletCornerOfVertex(v)`.
    CornerOf { v: u32 },
    /// `GetFilletSolverOfEdgeuse(eu)`.
    SolverOf { eu: u32 },
    /// `NoteFilletError(code, pFilletEdge, cComment)` on a new fillet edge of `fe_type`
    /// and `status`.
    NoteFilletError {
        code: u8,
        fe_type: u8,
        status: u8,
        comment: Option<String>,
    },
}

/// A script on an `SmFilletErrorInfo` (`exec` its executive) or on a new
/// `SmFilletExecutive` (`target` its B-rep).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "object", rename_all = "snake_case")]
enum ObjectIn {
    ErrorInfo { exec: u32, script: Vec<Step> },
    Executive { target: u32, script: Vec<Step> },
}

impl ObjectIn {
    fn parts(&self) -> (ffi::ObjectOp, u32, &[Step]) {
        match self {
            Self::ErrorInfo { exec, script } => (ffi::ObjectOp::ErrorScript, *exec, script),
            Self::Executive { target, script } => (ffi::ObjectOp::ExecScript, *target, script),
        }
    }
}

fn push_text(o: &mut Vec<f64>, t: Option<&String>) {
    match t {
        None => o.push(-1.0),
        Some(t) => {
            o.push(t.len() as f64);
            o.extend(t.bytes().map(f64::from));
        }
    }
}

fn push_list(o: &mut Vec<f64>, hs: &[u32]) {
    o.push(hs.len() as f64);
    o.extend(hs.iter().map(|&h| f64::from(h)));
}

fn object_cpp_input(i: &ObjectIn) -> Vec<f64> {
    let (_, handle, script) = i.parts();
    let mut o = vec![f64::from(handle), script.len() as f64];
    for s in script {
        match s {
            Step::Note { code, comment } => {
                o.extend([0.0, f64::from(*code)]);
                push_text(&mut o, comment.as_ref());
            }
            Step::Edge {
                edge,
                status,
                code,
                comment,
            } => {
                o.extend([1.0, f64::from(*edge), f64::from(*status), f64::from(*code)]);
                push_text(&mut o, comment.as_ref());
            }
            Step::Merge {
                code,
                status,
                edge,
                face,
                fillet_edge,
                surface,
                comment,
            } => {
                o.extend([2.0, f64::from(*code), f64::from(*status)]);
                o.extend([*edge, *face, *fillet_edge, *surface].map(f64::from));
                push_text(&mut o, comment.as_ref());
            }
            Step::Corner {
                code,
                corner,
                status,
                name,
                vertex,
                surfaces,
                fillet_vertices,
                comment,
            } => {
                o.extend([3.0, f64::from(*code), f64::from(*corner), f64::from(*status)]);
                push_text(&mut o, name.as_ref());
                o.push(f64::from(*vertex));
                o.extend(surfaces.map(f64::from));
                o.extend(fillet_vertices.map(f64::from));
                push_text(&mut o, comment.as_ref());
            }
            Step::FilletEdge {
                code,
                status,
                fe_type,
                fillet_edge,
                name,
                comment,
            } => {
                o.extend([4.0, f64::from(*code), f64::from(*status), f64::from(*fe_type)]);
                o.push(f64::from(*fillet_edge));
                push_text(&mut o, name.as_ref());
                push_text(&mut o, comment.as_ref());
            }
            Step::Intersection {
                code,
                status,
                edge,
                surfaces,
                curves,
                edges,
                at,
                comment,
            } => {
                o.extend([5.0, f64::from(*code), f64::from(*status), f64::from(*edge)]);
                push_list(&mut o, surfaces);
                push_list(&mut o, curves);
                push_list(&mut o, edges);
                o.extend(*at);
                push_text(&mut o, comment.as_ref());
            }
            Step::GlobalMerge { on } => o.extend([10.0, flag(*on)]),
            Step::Trimming { on } => o.extend([11.0, flag(*on)]),
            Step::Handler { h } => o.extend([12.0, f64::from(*h)]),
            Step::AddGenerator { h } => o.extend([13.0, f64::from(*h)]),
            Step::AddTopoEdge { h } => o.extend([14.0, f64::from(*h)]),
            Step::RemoveTopoEdge { h } => o.extend([15.0, f64::from(*h)]),
            Step::MapCorner { v, c } => o.extend([16.0, f64::from(*v), f64::from(*c)]),
            Step::MapSolver { eu, s } => o.extend([17.0, f64::from(*eu), f64::from(*s)]),
            Step::CornerOf { v } => o.extend([18.0, f64::from(*v)]),
            Step::SolverOf { eu } => o.extend([19.0, f64::from(*eu)]),
            Step::NoteFilletError {
                code,
                fe_type,
                status,
                comment,
            } => {
                o.extend([20.0, f64::from(*code), f64::from(*fe_type), f64::from(*status)]);
                push_text(&mut o, comment.as_ref());
            }
        }
    }
    o
}

fn bad_code(what: &str) -> HarnessError {
    HarnessError::new(format!("{what} code outside its enum"))
}

fn error_type(code: u8) -> Result<FilletErrorType, HarnessError> {
    use FilletErrorType as E;
    [
        E::Ok,
        E::ExecUninitialized,
        E::InvalidInput,
        E::VertexProblem,
        E::EdgeProblem,
        E::BadInt,
        E::BadTrim,
        E::BadMerge,
    ]
    .get(usize::from(code))
    .copied()
    .ok_or_else(|| bad_code("SmFilletErrorType"))
}

fn fil_status(code: u8) -> Result<FilStatus, HarnessError> {
    FilStatus::from_code(i64::from(code)).ok_or_else(|| bad_code("SmFilStatus"))
}

fn edge_type(code: u8) -> Result<FilletEdgeType, HarnessError> {
    FilletEdgeType::from_code(i64::from(code)).ok_or_else(|| bad_code("SmFilletEdgeType"))
}

fn corner_type(code: u8) -> Result<FilletCornerType, HarnessError> {
    use FilletCornerType as C;
    [
        C::Unknown,
        C::Open,
        C::Degenerate,
        C::OneByOne,
        C::TwoByTwo,
        C::ThreeByTwoMixed,
        C::FourByThree,
        C::NByOneClosed,
        C::NByOne,
        C::NByTwo,
        C::NByN,
        C::NByNConvex,
        C::NByNConcave,
    ]
    .get(usize::from(code))
    .copied()
    .ok_or_else(|| bad_code("SmFilletCornerType"))
}

fn opts(hs: &[u32]) -> Vec<Option<H>> {
    hs.iter().map(|&h| opt(h)).collect()
}

/// Where a script's `NoteError` calls go: an error info, or an executive (whose
/// `NoteFilletError` overloads take calls 2, 3 and 5).
enum Sink<'a> {
    Info(&'a mut ErrorInfo<H>),
    Exec(&'a mut FilletExecutive<H>),
}

impl Sink<'_> {
    fn info(&mut self) -> &mut ErrorInfo<H> {
        match self {
            Self::Info(e) => e,
            Self::Exec(x) => &mut x.error,
        }
    }
}

/// The script's `NoteError` call, `false` for another call. `fv`/`loc` record that the
/// fillet vertices / location were set (uninitialised upstream before).
fn note(sink: &mut Sink<'_>, s: &Step, fv: &mut bool, loc: &mut bool) -> Result<bool, HarnessError> {
    match s {
        Step::Note { code, comment } => sink.info().note_error(error_type(*code)?, comment.as_deref()),
        Step::Edge {
            edge,
            status,
            code,
            comment,
        } => sink.info().note_edge_error(
            opt(*edge),
            fil_status(*status)?,
            error_type(*code)?,
            comment.as_deref(),
        ),
        Step::Merge {
            code,
            status,
            edge,
            face,
            fillet_edge,
            surface,
            comment,
        } => {
            let args = (
                error_type(*code)?,
                fil_status(*status)?,
                opt(*edge),
                opt(*face),
                opt(*fillet_edge),
                opt(*surface),
                comment.as_deref(),
            );
            match sink {
                Sink::Info(e) => e.note_merge_error(args.0, args.1, args.2, args.3, args.4, args.5, args.6),
                Sink::Exec(x) => {
                    x.note_fillet_merge_error(args.0, args.1, args.2, args.3, args.4, args.5, args.6);
                }
            }
        }
        Step::Corner {
            code,
            corner,
            status,
            name,
            vertex,
            surfaces,
            fillet_vertices,
            comment,
        } => {
            let args = (
                error_type(*code)?,
                corner_type(*corner)?,
                fil_status(*status)?,
                name.as_deref(),
                opt(*vertex),
                surfaces.map(opt),
                fillet_vertices.map(opt),
                comment.as_deref(),
            );
            match sink {
                Sink::Info(e) => e.note_corner_error(
                    args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7,
                ),
                Sink::Exec(x) => x.note_fillet_corner_error(
                    args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7,
                ),
            }
            *fv = true;
        }
        Step::FilletEdge {
            code,
            status,
            fe_type,
            fillet_edge,
            name,
            comment,
        } => sink.info().note_fillet_edge_error(
            error_type(*code)?,
            fil_status(*status)?,
            edge_type(*fe_type)?,
            opt(*fillet_edge),
            name.as_deref(),
            comment.as_deref(),
        ),
        Step::Intersection {
            code,
            status,
            edge,
            surfaces,
            curves,
            edges,
            at,
            comment,
        } => {
            let (code, status) = (error_type(*code)?, fil_status(*status)?);
            let (surfaces, curves, edges) = (opts(surfaces), opts(curves), opts(edges));
            let at = Point3::new(at[0], at[1], at[2]);
            match sink {
                Sink::Info(e) => e.note_intersection_error(
                    code,
                    status,
                    opt(*edge),
                    &surfaces,
                    &curves,
                    &edges,
                    at,
                    comment.as_deref(),
                ),
                Sink::Exec(x) => x.note_fillet_intersection_error(
                    code,
                    status,
                    opt(*edge),
                    &surfaces,
                    &curves,
                    &edges,
                    at,
                    comment.as_deref(),
                ),
            }
            *loc = true;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn push_out_text(o: &mut Vec<f64>, t: &str) {
    o.push(t.len() as f64);
    o.extend(t.bytes().map(f64::from));
}

fn put_error(o: &mut Vec<f64>, e: &ErrorInfo<H>, fv: bool, loc: bool) {
    o.extend([
        f64::from(e.code),
        f64::from(e.status),
        f64::from(e.corner_type as i32),
        f64::from(e.edge_type),
    ]);
    o.extend([e.executive, e.vertex, e.edge, e.side_face, e.end_face].map(out_handle));
    o.push(flag(fv));
    if fv {
        o.extend(e.fillet_vertices.map(out_handle));
    }
    for list in [&e.surfaces, &e.edges, &e.curves] {
        o.push(list.len() as f64);
        o.extend(list.iter().map(|&h| out_handle(h)));
    }
    o.push(flag(loc));
    if loc {
        let p = e.location.unwrap_or_default();
        o.extend([p.x, p.y, p.z]);
    }
    push_out_text(o, &e.comment);
    push_out_text(o, &e.entity_name);
}

/// One executive call (10-20), with query answers appended to `q`.
fn exec_step(
    x: &mut FilletExecutive<H>,
    s: &Step,
    made: &mut u32,
    q: &mut Vec<f64>,
) -> Result<(), HarnessError> {
    match s {
        Step::GlobalMerge { on } => x.set_do_global_merge(*on),
        Step::Trimming { on } => x.set_do_trimming(*on),
        Step::Handler { h } => x.set_self_intersection_handler(opt(*h)),
        Step::AddGenerator { h } => x.add_fillet_surface_generator(H(*h)),
        Step::AddTopoEdge { h } => x.topo_edges.push(opt(*h)),
        Step::RemoveTopoEdge { h } => x.remove_from_topo_edges(opt(*h)),
        Step::MapCorner { v, c } => {
            x.vertex_corner_map.insert(Some(H(*v)), solidcore::blend::fillet_executive::p3::VertexEntry::Corner(H(*c)));
        }
        Step::MapSolver { eu, s } => {
            x.edgeuse_solver_map.insert(H(*eu), H(*s));
        }
        Step::CornerOf { v } => q.push(out_handle(x.fillet_corner_of_vertex(H(*v)).map_err(|e| HarnessError::new(e.to_string()))?)),
        Step::SolverOf { eu } => q.push(out_handle(x.fillet_solver_of_edgeuse(H(*eu)))),
        Step::NoteFilletError {
            code,
            fe_type,
            status,
            comment,
        } => {
            let mut fe = FilletEdge::with_corner(None);
            fe.edge_type = edge_type(*fe_type)?;
            fe.status = fil_status(*status)?;
            let handle = H(MADE - *made);
            *made += 1;
            x.note_fillet_edge(
                error_type(*code)?,
                handle,
                (fe.name(), fe.edge_type, fe.status),
                comment.as_deref(),
            )
            .map_err(|e| HarnessError::new(e.to_string()))?;
        }
        _ => return Err(HarnessError::new("not an executive call")),
    }
    Ok(())
}

fn object_rust(i: &ObjectIn) -> Result<Output, HarnessError> {
    let (mut fv, mut loc) = (false, false);
    let mut o = vec![0.0];
    match i {
        ObjectIn::ErrorInfo { exec, script } => {
            let mut e = ErrorInfo::new(opt(*exec));
            for s in script {
                if !note(&mut Sink::Info(&mut e), s, &mut fv, &mut loc)? {
                    return Err(HarnessError::new("not an error-info call"));
                }
            }
            put_error(&mut o, &e, fv, loc);
        }
        ObjectIn::Executive { target, script } => {
            let objects = ExecutiveObjects {
                executive: SELF,
                pseudo_brep: PSEUDO,
                edge_attribute: ATTRIBUTE,
            };
            let mut x = FilletExecutive::new(opt(*target), objects);
            let (mut made, mut q) = (0, Vec::new());
            for s in script {
                if !note(&mut Sink::Exec(&mut x), s, &mut fv, &mut loc)? {
                    exec_step(&mut x, s, &mut made, &mut q)?;
                }
            }
            let op = x.options;
            o.extend(
                [
                    op.piecewise_merge,
                    op.topology_insertion,
                    op.global_merge,
                    op.trimming,
                    op.classification,
                ]
                .map(flag),
            );
            o.extend([out_handle(x.target), out_handle(x.fillet)]);
            o.extend([x.corners.len() as f64, x.solvers.len() as f64]);
            o.push(x.topo_edges.len() as f64);
            o.extend(x.topo_edges.iter().map(|&h| out_handle(h)));
            o.push(x.surface_generators.len() as f64);
            o.extend(x.surface_generators.iter().map(|&h| out_handle(Some(h))));
            o.push(out_handle(x.self_intersection_handler));
            o.push(flag(x.intersector.ignore_tangency));
            o.extend([
                x.intersector.this_approx_tol_3d(),
                x.intersector.this_ang_tol_rad(),
            ]);
            o.extend([flag(x.pseudo_brep.is_some()), flag(x.edge_attribute.is_some())]);
            put_error(&mut o, &x.error, fv, loc);
            o.push(q.len() as f64);
            o.extend(q);
        }
    }
    Ok(to_output(&o))
}

fn gen_handle(rng: &mut Rng) -> u32 {
    match rng.below(10) {
        0..=1 => 0,
        2..=8 => rng.below(6) as u32 + 1,
        _ => rng.range_i64(7, ffi::MAX_HANDLE as i64) as u32,
    }
}

/// A text: NULL, empty, short (ASCII or any Unicode), or long ASCII. Long texts stay
/// ASCII: upstream's 1023-byte cut may split a UTF-8 sequence, which a Rust string
/// cannot hold (a documented deviation).
fn gen_text(rng: &mut Rng) -> Option<String> {
    const WIDE: [char; 6] = ['é', 'ß', '€', '中', '🙂', '\u{7f}'];
    let len = match rng.below(10) {
        0..=1 => return None,
        2 => 0,
        3 => rng.range_i64(1000, ffi::MAX_TEXT as i64) as usize,
        _ => rng.below(40) as usize,
    };
    let wide = len < 200 && rng.chance(0.4);
    Some(
        (0..len)
            .map(|_| match rng.choose(&WIDE) {
                Some(&c) if wide && rng.chance(0.3) => c,
                _ => char::from(rng.range_i64(1, 126) as u8),
            })
            .collect(),
    )
}

fn gen_list(rng: &mut Rng) -> Vec<u32> {
    (0..rng.below(ffi::MAX_LIST as u64 + 1))
        .map(|_| gen_handle(rng))
        .collect()
}

/// A `SmFilletEdgeType` `GetName` writes a name for.
fn gen_named_edge_type(rng: &mut Rng) -> u8 {
    [1, 3, 4, 5, 6, 7, 9, 10, 11, 12]
        .get(rng.below(10) as usize)
        .copied()
        .unwrap_or(1)
}

fn gen_step(rng: &mut Rng, exec: bool) -> Step {
    let code = rng.below(8) as u8;
    let status = rng.below(13) as u8;
    match rng.below(if exec { 17 } else { 6 }) {
        0 => Step::Note {
            code,
            comment: gen_text(rng),
        },
        1 => Step::Edge {
            edge: gen_handle(rng),
            status,
            code,
            comment: gen_text(rng),
        },
        2 => Step::Merge {
            code,
            status,
            edge: gen_handle(rng),
            face: gen_handle(rng),
            fillet_edge: gen_handle(rng),
            surface: gen_handle(rng),
            comment: gen_text(rng),
        },
        3 => Step::Corner {
            code,
            corner: rng.below(13) as u8,
            status,
            name: gen_text(rng),
            vertex: gen_handle(rng),
            surfaces: [gen_handle(rng), gen_handle(rng)],
            fillet_vertices: [gen_handle(rng), gen_handle(rng)],
            comment: gen_text(rng),
        },
        4 => Step::FilletEdge {
            code,
            status,
            fe_type: rng.below(15) as u8,
            fillet_edge: gen_handle(rng),
            name: gen_text(rng),
            comment: gen_text(rng),
        },
        5 => Step::Intersection {
            code,
            status,
            edge: gen_handle(rng),
            surfaces: gen_list(rng),
            curves: gen_list(rng),
            edges: gen_list(rng),
            at: [0; 3].map(|_| rng.uniform(-1e3, 1e3) * 10f64.powf(rng.uniform(-6.0, 6.0))),
            comment: gen_text(rng),
        },
        6 => Step::GlobalMerge { on: rng.chance(0.5) },
        7 => Step::Trimming { on: rng.chance(0.5) },
        8 => Step::Handler { h: gen_handle(rng) },
        // No upstream caller adds a null generator; the Rust list holds handles only.
        9 => Step::AddGenerator {
            h: gen_handle(rng).max(1),
        },
        10 => Step::AddTopoEdge { h: gen_handle(rng) },
        11 => Step::RemoveTopoEdge { h: gen_handle(rng) },
        12 => Step::MapCorner {
            v: gen_handle(rng),
            c: gen_handle(rng),
        },
        13 => Step::MapSolver {
            eu: gen_handle(rng),
            s: gen_handle(rng),
        },
        14 => Step::CornerOf { v: gen_handle(rng) },
        15 => Step::SolverOf { eu: gen_handle(rng) },
        _ => Step::NoteFilletError {
            code,
            fe_type: gen_named_edge_type(rng),
            status,
            comment: gen_text(rng),
        },
    }
}

fn gen_object(rng: &mut Rng) -> ObjectIn {
    let exec = rng.chance(0.5);
    let n = rng.below(17);
    let script = (0..n).map(|_| gen_step(rng, exec)).collect();
    if exec {
        ObjectIn::Executive {
            target: gen_handle(rng),
            script,
        }
    } else {
        ObjectIn::ErrorInfo {
            exec: gen_handle(rng),
            script,
        }
    }
}
