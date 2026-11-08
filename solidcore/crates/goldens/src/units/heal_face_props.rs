//! Runner for unit `heal.face_props`: `SmFaceProps`, `SmLoopProps` and the
//! `SmLooptreeItem` tree.
//!
//! Ported from upstream `SmFaceProps.cpp` and `SmLoopProps.cpp` into
//! `solidcore::heal::face_props`; checked against the unit's golden. The one-face B-rep is
//! built record by record by [`rust_brep`] as upstream's `SmBrepConstructor` builds it
//! (the surface, owned by the face, the vertex loop or edge loops and holes, the face
//! tolerance).
//!
//! Two kinds of case. `props`: a script of ops on one face props object (`SetProps` and its
//! three stages, `ReSet`, `HasProps`, `HasProblems`, `IsPoint3dOnPole`, field seeds, and
//! `SmLoopProps::SetProps` of every loop); the outputs are each op's result and a snapshot of
//! the props after it (op 9: the loop props `SetProps_Stage3` left, with the `ClassifyLoops`
//! containment and orientations). `tree`: building a loop tree with `AddChild` / `AddSibling` /
//! `AddPartner` and `FindItem` queries (fake loop numbers).
//!
//! **Faces.** A plane or a surface of revolution (`CreateCanonical`: a cylinder, a cone with
//! or without apex, a sphere or a torus, with the swept angle either 360 degrees, a closed
//! surface, or below it, an open one that `AdjustSTEPUVDomain` trims), with a vertex loop
//! (at the origin, or at a pole), edge loops (an outer loop and holes of line edges) or circle
//! loops (one `SmCircle` edge each: latitude circles of a revolution, no-area loops on a
//! closed one, or circles on the plane). The face tolerance reaches `GetSingularities`
//! through `SmFace::GetTolerance`.
//!
//! **Stage 3.** The `s3gen####` cases ([`STAGE3_GENERATED`], their own seeds) draw faces whose
//! loops `ClassifyLoops` classifies: nested, side-by-side and misoriented loops on a plane,
//! latitude circles on cylinders, cones and spheres; the script runs Stage 3 (directly or
//! through `SetProps`), sometimes resets to Stage 2 and runs it again. The healer's calls
//! keep what the face queries leave on the B-rep for the next op, as upstream's
//! (`set_props_with_query_gaps`, `set_props_stage2_mut`, `set_props_stage3_mut`): a re-run
//! of `ClassifyLoops` reads the UV trim curves the first one made.
//!
//! **Scripts are guarded.** [`script_contract`] is the reference wrapper's input contract:
//! it rejects what crashes or is undefined upstream (including the scripts of UB-131:
//! `SetProps_Stage2` / `Stage3` on a stage past `GAPSa` with no face pointer,
//! `plan/upstream-issues.md`); the generator redraws until it holds. A fixture case with
//! `expect_rejected` is a regression case for the guard: it must be rejected (the error
//! `rejected`).
//!
//! Not reached: `m_lBadMissingPoles` (it needs a pole on a side in the surface's closed
//! direction with a `SM_CD_POLE_NO_VERTEX` classify curve end; a surface closed both ways
//! classifies its ends as seams) and the bad closed-loop counts (every built loop has a
//! closed vertex chain).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::error::EntityRef;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::heal::face_props::{
    orient_code, FaceProps, HealerOpType, ItemId, LoopObject, LoopProps, Looptree, LooptreeItem,
    PropStage,
};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::kernel::Kernel;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopId, Loopuse, LoopuseId, LoopuseStart, Region, Shell, VertexId, Vertexuse,
    VertexuseId, VertexuseOwner,
};

use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `heal.face_props` runner.
pub struct Runner;

/// The classification handles: loops, and the face, its surface and the two curves of
/// `SmFace::ClassifyLoops`' loop bisector.
#[derive(Debug, Clone, Copy, PartialEq)]
enum H {
    Topo(EntityRef),
    Surface(FaceId),
    Curve(bool),
    EdgeCurve(EdgeId),
    Trim(EdgeuseId),
    /// A temporary curve (the seam curves of `SmFace::HasSeamProblem`).
    Temp(usize),
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Topo(e) => e.class_kind(),
            Self::Surface(_) => ClassObjectKind::Surface,
            Self::Curve(_) | Self::EdgeCurve(_) | Self::Trim(_) | Self::Temp(_) => {
                ClassObjectKind::Curve
            }
        }
    }

    fn temporary(index: usize, kind: ClassObjectKind) -> Option<Self> {
        (kind == ClassObjectKind::Curve).then_some(Self::Temp(index))
    }
}

impl LoopObject for H {
    fn from_loop(l: LoopId) -> Self {
        Self::Topo(EntityRef::Loop(l))
    }

    fn as_loop(&self) -> Option<LoopId> {
        match self {
            Self::Topo(e) => e.as_loop(),
            _ => None,
        }
    }

    fn as_vertex(&self) -> Option<VertexId> {
        match self {
            Self::Topo(e) => e.as_vertex(),
            _ => None,
        }
    }

    fn from_face(f: FaceId) -> Option<Self> {
        Some(Self::Topo(EntityRef::Face(f)))
    }

    fn from_face_surface(f: FaceId) -> Option<Self> {
        Some(Self::Surface(f))
    }

    fn from_classify_curve(uv: bool) -> Option<Self> {
        Some(Self::Curve(uv))
    }

    fn from_topology(t: EntityRef) -> Option<Self> {
        Some(Self::Topo(t))
    }

    fn as_topology(&self) -> Option<EntityRef> {
        match self {
            Self::Topo(t) => Some(*t),
            _ => None,
        }
    }

    fn from_edge_curve(e: EdgeId) -> Option<Self> {
        Some(Self::EdgeCurve(e))
    }

    fn as_edge_curve(&self) -> Option<EdgeId> {
        match self {
            Self::EdgeCurve(e) => Some(*e),
            _ => None,
        }
    }

    fn from_trim_curve(eu: EdgeuseId) -> Option<Self> {
        Some(Self::Trim(eu))
    }

    fn as_trim_curve(&self) -> Option<EdgeuseId> {
        match self {
            Self::Trim(eu) => Some(*eu),
            _ => None,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// The generator of a surface of revolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum GenIn {
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        r: f64,
    },
}

/// The face's surface when it is not the plane `o`, `x`, `y`, `domain` of [`FaceIn`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SurfIn {
    Revolution {
        gen: GenIn,
        point: [f64; 3],
        axis: [f64; 3],
        #[serde(default)]
        swap: bool,
        /// Swept angle in degrees; below 360 the angle domain is adjusted to it.
        sweep: f64,
    },
}

/// The face's loops when they are not the one vertex loop at the origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum LoopsIn {
    Vertex { at: [f64; 3] },
    /// Edge loops (the first one outer, the others holes): corner points joined by lines.
    Edges { loops: Vec<Vec<[f64; 3]>> },
    /// Loops of one circle edge each (the first one outer).
    Circles { circles: Vec<CircleIn> },
}

/// A loop of one circle edge `SmCircle(c, x, y, d, r)` (`d` in degrees), its one vertex at
/// the circle's start.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct CircleIn {
    pub(crate) c: [f64; 3],
    pub(crate) x: [f64; 3],
    pub(crate) y: [f64; 3],
    pub(crate) d: [f64; 2],
    pub(crate) r: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FaceIn {
    pub(crate) tol: f64,
    pub(crate) o: [f64; 3],
    pub(crate) x: [f64; 3],
    pub(crate) y: [f64; 3],
    /// `[umin, vmin, umax, vmax]`.
    pub(crate) domain: [f64; 4],
    #[serde(default)]
    pub(crate) surf: Option<SurfIn>,
    #[serde(default)]
    pub(crate) loops: Option<LoopsIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // test inputs, built once per case
enum CaseIn {
    Props {
        face: FaceIn,
        /// `[op, p0, p1, p2, p3]` per op.
        ops: Vec<[f64; 5]>,
        /// A regression case for the FFI guard: both sides must reject the script.
        #[serde(default)]
        expect_rejected: bool,
    },
    Tree {
        /// `[loop, partner]` per item (`-1` pole vertex; partner `-2` none).
        items: Vec<[i64; 2]>,
        /// `[kind (0 child, 1 sibling), this, new]`.
        links: Vec<[i64; 3]>,
        /// `[start item, loop]`.
        finds: Vec<[i64; 2]>,
    },
}

// ---- the input contract ----------------------------------------------------------
//
// The reference wrapper accepts only scripts upstream runs without crashing or undefined
// behaviour; the generator redraws until a script keeps that contract, and the Rust side
// runs only such scripts (the guard's regression cases are fixtures).

/// Values per snapshot.
const SNAP_LEN: usize = 82;
/// Largest number of ops.
const MAX_OPS: usize = 64;
/// Largest number of tree items.
const MAX_ITEMS: usize = 32;
/// Number of seedable fields (ids `0..SEED_FIELDS`).
const SEED_FIELDS: i64 = 26;
/// Loops whose props ops `8` and `9` report.
const MAX_LOOP_SNAPS: usize = 4;
/// Largest number of loops.
const MAX_LOOPS: usize = 4;
/// Largest number of points per loop.
const MAX_LOOP_POINTS: usize = 8;
/// The smallest side of a plane domain with edge or circle loops a script may run
/// `SetProps_Stage3` on (UB-128: `SmFace::ClassifyLoops` crashes upstream on tiny
/// domains, not monotonically in the size, so the bound is a safe one).
const MIN_PLANE_SIDE_FOR_STAGE3: f64 = 1e-6;

fn sq_dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sq_len(a: [f64; 3]) -> f64 {
    a[0] * a[0] + a[1] * a[1] + a[2] * a[2]
}

/// Whether every value is finite and within `1e9`.
fn finite(vals: impl IntoIterator<Item = f64>) -> bool {
    vals.into_iter().all(|v| v.is_finite() && v.abs() <= 1e9)
}

/// Whether a circle `(c, x, y, d, r)` is one upstream builds: non-parallel axes, a radius
/// of at least `1e-3` and 1 to 360 degrees.
fn circle_ok(c: &[f64; 3], x: &[f64; 3], y: &[f64; 3], d: &[f64; 2], r: f64) -> bool {
    finite(c.iter().chain(x).chain(y).chain(d).copied().chain([r]))
        && sq_len(cross(*x, *y)) >= 1e-6
        && r >= 1e-3
        && d[1] - d[0] >= 1.0
        && d[1] - d[0] <= 360.0
}

/// The face contract: a face tolerance in `(0, 1e9]` (`SmFace::SetTolerance` turns a value
/// at or below zero into the default, which the raw field write does not), finite values,
/// a plane with non-parallel axes and a non-empty domain, a revolution about a non-zero
/// axis over 1 to 360 degrees of a generator line longer than `1e-3` or a circle
/// ([`circle_ok`]), and one to [`MAX_LOOPS`] loops: edge loops of three to
/// [`MAX_LOOP_POINTS`] points with edges longer than `1e-3`, or circle loops.
fn face_contract(face: &FaceIn) -> Result<(), &'static str> {
    if !face.tol.is_finite() || face.tol <= 0.0 || face.tol > 1e9 {
        return Err("face tolerance");
    }
    match &face.surf {
        None => {
            let (o, x, y, domain) = (&face.o, &face.x, &face.y, &face.domain);
            if !finite(o.iter().chain(x).chain(y).chain(domain).copied()) {
                return Err("face values");
            }
            if sq_len(cross(*x, *y)) < 1e-6 {
                return Err("plane axes");
            }
            if domain[2] <= domain[0] || domain[3] <= domain[1] {
                return Err("plane domain");
            }
        }
        Some(SurfIn::Revolution {
            gen,
            point,
            axis,
            sweep,
            ..
        }) => {
            if !finite(point.iter().chain(axis).copied()) || !finite([*sweep]) {
                return Err("revolution values");
            }
            if sq_len(*axis) < 1e-6 || !(1.0..=360.0).contains(sweep) {
                return Err("revolution axis or sweep");
            }
            match gen {
                GenIn::Line { a, b } => {
                    if !finite(a.iter().chain(b).copied()) || sq_dist(*a, *b) < 1e-6 {
                        return Err("generator line");
                    }
                }
                GenIn::Circle { c, x, y, d, r } => {
                    if !circle_ok(c, x, y, d, *r) {
                        return Err("generator circle");
                    }
                }
            }
        }
    }
    match &face.loops {
        None => {}
        Some(LoopsIn::Vertex { at }) => {
            if !finite(at.iter().copied()) {
                return Err("vertex loop point");
            }
        }
        Some(LoopsIn::Edges { loops }) => {
            if loops.is_empty() || loops.len() > MAX_LOOPS {
                return Err("loop count");
            }
            for l in loops {
                if l.len() < 3 || l.len() > MAX_LOOP_POINTS {
                    return Err("loop point count");
                }
                for (i, p) in l.iter().enumerate() {
                    let Some(q) = l.get((i + 1) % l.len()).copied() else {
                        return Err("loop point");
                    };
                    if !finite(p.iter().copied()) || sq_dist(*p, q) < 1e-6 {
                        return Err("loop point");
                    }
                }
            }
        }
        Some(LoopsIn::Circles { circles }) => {
            if circles.is_empty() || circles.len() > MAX_LOOPS {
                return Err("loop count");
            }
            if !circles.iter().all(|l| circle_ok(&l.c, &l.x, &l.y, &l.d, l.r)) {
                return Err("circle loop");
            }
        }
    }
    Ok(())
}

/// The `(stage, face pointer set)` pair of `SmFaceProps` the null pointer rule needs.
type State = (i64, bool);

/// `SM_PROPSTAGE_GAPSa`, `GAPS`, `2a`, `2`, `3a`, `3`.
const GAPS_A: i64 = 1;
const GAPS: i64 = 2;
const STAGE2_A: i64 = 3;
const STAGE2: i64 = 4;
const STAGE3_A: i64 = 5;
const STAGE3: i64 = 6;
/// `SM_HO_FIX_TOLSIZES`, `SM_HO_FIX_SPLITEDGE_ATSEAM`.
const HO_FIX_TOLSIZES: f64 = 5.0;
const HO_FIX_SPLITEDGE_ATSEAM: f64 = 19.0;

/// The states a stage call can leave, each with whether it returned success.
type Outcomes = Vec<(State, bool)>;

/// `SetProps_Gaps`: returns at once from `GAPSa` on; else `m_pFace = cpFace`.
fn sim_gaps(s: State) -> State {
    if s.0 >= GAPS_A {
        s
    } else {
        (GAPS, true)
    }
}

/// `SetProps_Stage2`; `None` when it would dereference a null `m_pFace` (UB-131).
fn sim_stage2(s: State, heal: f64) -> Option<Outcomes> {
    if s.0 >= STAGE2_A {
        return Some(vec![(s, true)]);
    }
    if heal < HO_FIX_TOLSIZES {
        return Some(vec![(s, false)]);
    }
    let mut s = s;
    if s.0 < GAPS || !s.1 {
        s = sim_gaps(s);
    }
    if !s.1 {
        return None;
    }
    // ReSet(2) then the body: it can stop at 2a on an error (a callee fails), else 2
    Some(vec![((STAGE2_A, true), false), ((STAGE2, true), true)])
}

/// `SetProps_Stage3`; `None` when it would dereference a null `m_pFace` (UB-131).
fn sim_stage3(s: State, heal: f64) -> Option<Outcomes> {
    if s.0 >= STAGE3_A {
        return Some(vec![(s, true)]);
    }
    if heal < HO_FIX_SPLITEDGE_ATSEAM {
        return Some(vec![(s, false)]);
    }
    let mut out: Outcomes = Vec::new();
    let mut go: Vec<State> = vec![s];
    if s.0 < STAGE2 || !s.1 {
        go.clear();
        for (t, ok) in sim_stage2(s, heal)? {
            if ok {
                go.push(t);
            } else {
                // SER: the failed status returns
                out.push((t, false));
            }
        }
    }
    for t in go {
        if !t.1 {
            return None;
        }
        out.push(((STAGE3_A, true), false));
        out.push(((STAGE3, true), true));
    }
    Some(out)
}

/// `SetProps(stage, face, index, heal)`: the calls it makes, in order, on every state it
/// can be in; `None` when one of them would dereference a null `m_pFace` (UB-131).
fn sim_set_props(s: State, stage: i64, heal: f64) -> Option<Vec<State>> {
    // SetProps_Gaps (HasProps_Stage1: stage >= GAPS)
    let cur: Vec<State> = [s]
        .into_iter()
        .map(|t| {
            if stage == GAPS || (stage > GAPS && t.0 < GAPS) {
                sim_gaps(t)
            } else {
                t
            }
        })
        .collect();
    // SetProps_Stage2 (HasProps_Stage2: stage >= 2 and the face is cpFace)
    let mut next = Vec::new();
    for t in &cur {
        if stage == STAGE2 || (stage > STAGE2 && !(t.0 >= STAGE2 && t.1)) {
            next.extend(sim_stage2(*t, heal)?.into_iter().map(|x| x.0));
        } else {
            next.push(*t);
        }
    }
    // SetProps_Stage3
    let mut out = Vec::new();
    for t in &next {
        if stage == STAGE3 {
            out.extend(sim_stage3(*t, heal)?.into_iter().map(|x| x.0));
        } else {
            out.push(*t);
        }
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
}

/// Whether no op of the script reaches a `SetProps_Stage2` or `SetProps_Stage3` that would
/// dereference a null `m_pFace` (UB-131: `SetProps_Gaps` returns early from `GAPSa` on, so
/// the re-run that `m_pFace != cpFace` asks for never sets the face), over every state the
/// script can be in.
fn null_face_free(ops: &[[f64; 5]]) -> bool {
    let mut states: Vec<State> = vec![(0, false)];
    for &[op, p0, p1, p2, _] in ops {
        let mut next: Vec<State> = Vec::new();
        for &s in &states {
            let r: Option<Vec<State>> = match op as i64 {
                0 => sim_set_props(s, p0 as i64, p1),
                1 => {
                    let c = p0 as i64;
                    let stage = if c <= GAPS {
                        0
                    } else if c <= STAGE2 {
                        GAPS
                    } else if c <= STAGE3 {
                        STAGE2
                    } else {
                        s.0
                    };
                    Some(vec![(stage, c > GAPS && s.1)])
                }
                5 if p0 as i64 == 0 => Some(vec![(p2 as i64, s.1)]),
                5 if p0 as i64 == 24 => Some(vec![(s.0, p2 != 0.0)]),
                6 => match p0 as i64 {
                    1 => Some(vec![sim_gaps(s)]),
                    2 => sim_stage2(s, p1).map(|v| v.into_iter().map(|x| x.0).collect()),
                    _ => sim_stage3(s, p1).map(|v| v.into_iter().map(|x| x.0).collect()),
                },
                _ => Some(vec![s]),
            };
            match r {
                Some(v) => next.extend(v),
                None => return false,
            }
        }
        next.sort_unstable();
        next.dedup();
        states = next;
    }
    true
}

/// The range of one seed (op `5`) value of field `field` (in `0..SEED_FIELDS`).
fn seed_ok(field: i64, v: f64) -> bool {
    match field {
        // m_ePropStage
        0 => (0.0..=6.0).contains(&v),
        // SmMixedLinkType
        7 | 8 => (0.0..=5.0).contains(&v),
        // SmHealNotYetFaceType
        21 => (0.0..=3.0).contains(&v),
        // SmBoolean / SmBooleanUL: FALSE, TRUE, UNSURE
        5 | 6 | 10 | 11 | 16 | 20 | 22 | 23 | 25 => (0.0..=2.0).contains(&v),
        // ULONG members: a negative value cast to ULONG is undefined
        3 | 9 | 12..=15 | 17..=19 => v >= 0.0,
        _ => true,
    }
}

/// The script contract: the face's ([`face_contract`]), at most [`MAX_OPS`] ops with
/// finite operands within `1e9` and per op its stage, heal op, bit mask, seed field and
/// value or stage call in range and a non-negative index; no `SetProps_Stage3` on a plane
/// domain with edge or circle loops and a side below [`MIN_PLANE_SIDE_FOR_STAGE3`]
/// (UB-128), and no null face dereference ([`null_face_free`], UB-131).
fn script_contract(face: &FaceIn, ops: &[[f64; 5]]) -> Result<(), &'static str> {
    face_contract(face)?;
    if ops.len() > MAX_OPS {
        return Err("op count");
    }
    for &[op, p0, p1, p2, p3] in ops {
        if [p0, p1, p2, p3].iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
            return Err("operand");
        }
        let op = op as i64;
        let ok = match op {
            0 => (0.0..=6.0).contains(&p0) && (0.0..=26.0).contains(&p1),
            1 | 2 => (0.0..=6.0).contains(&p0),
            // the bit mask is cast to ULONG
            3 => p0 >= 0.0,
            4 | 7 | 8 | 9 => true,
            5 => {
                (0.0..SEED_FIELDS as f64).contains(&p0)
                    && p0.fract() == 0.0
                    && seed_ok(p0 as i64, p2)
            }
            6 => (1.0..=3.0).contains(&p0) && (0.0..=26.0).contains(&p1),
            _ => false,
        };
        if !ok || (op == 0 || op == 6) && p2 < 0.0 {
            return Err("op");
        }
    }
    let loops = matches!(
        face.loops,
        Some(LoopsIn::Edges { .. } | LoopsIn::Circles { .. })
    );
    if face.surf.is_none() && loops {
        let d = &face.domain;
        let small = !((d[2] - d[0]) >= MIN_PLANE_SIDE_FOR_STAGE3
            && (d[3] - d[1]) >= MIN_PLANE_SIDE_FOR_STAGE3);
        let reaches = ops
            .iter()
            .any(|o| (o[0] as i64 == 0 && o[1] as i64 >= STAGE3_A) || (o[0] as i64 == 6 && o[1] as i64 == 3));
        if small && reaches {
            return Err("degenerate plane domain with edge loops reaches SetProps_Stage3 (UB-128)");
        }
    }
    if !null_face_free(ops) {
        return Err("script would dereference the null face of SetProps_Stage2/3 (UB-131)");
    }
    Ok(())
}

/// The loop tree contract: one to [`MAX_ITEMS`] items with loops in `-1..=1000` and
/// partners in `-2..=1000`, links (`0` child, `1` sibling) between two distinct items
/// that link each item at most once and leave no cycle, and finds from an item.
fn tree_contract(
    items: &[[i64; 2]],
    links: &[[i64; 3]],
    finds: &[[i64; 2]],
) -> Result<(), &'static str> {
    let n = items.len() as i64;
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err("item count");
    }
    let mut linked = vec![false; items.len()];
    for &[kind, a, c] in links {
        if !(0..=1).contains(&kind) || !(0..n).contains(&a) || !(0..n).contains(&c) || a == c {
            return Err("link");
        }
        let Some(flag) = usize::try_from(c).ok().and_then(|c| linked.get_mut(c)) else {
            return Err("link");
        };
        if *flag {
            return Err("item linked twice");
        }
        *flag = true;
    }
    // no cycles: the pointers AddChild / AddSibling leave; every item is the target of at
    // most one pointer, so there is a cycle exactly when the items reachable from the
    // items nothing points to are not all of them
    let mut child = vec![None::<usize>; items.len()];
    let mut sib = vec![None::<usize>; items.len()];
    for &[kind, a, c] in links {
        let (Ok(a), Ok(c)) = (usize::try_from(a), usize::try_from(c)) else {
            return Err("link");
        };
        let old = if kind == 0 { child.get(a) } else { sib.get(a) }
            .copied()
            .flatten();
        if let Some(slot) = sib.get_mut(c) {
            *slot = old;
        }
        let target = if kind == 0 {
            child.get_mut(a)
        } else {
            sib.get_mut(a)
        };
        if let Some(slot) = target {
            *slot = Some(c);
        }
    }
    let mut pointed = vec![false; items.len()];
    for t in child.iter().chain(sib.iter()).flatten() {
        if let Some(p) = pointed.get_mut(*t) {
            *p = true;
        }
    }
    let mut stack: Vec<usize> = (0..items.len())
        .filter(|i| !pointed.get(*i).copied().unwrap_or(true))
        .collect();
    let mut reached = 0usize;
    while let Some(i) = stack.pop() {
        reached += 1;
        stack.extend(child.get(i).copied().flatten());
        stack.extend(sib.get(i).copied().flatten());
        if reached > items.len() {
            return Err("cycle");
        }
    }
    if reached != items.len() {
        return Err("cycle");
    }
    if items
        .iter()
        .any(|&[l, partner]| !(-1..=1000).contains(&l) || !(-2..=1000).contains(&partner))
    {
        return Err("item");
    }
    if finds
        .iter()
        .any(|&[s, l]| !(0..n).contains(&s) || !(-1..=1000).contains(&l))
    {
        return Err("find");
    }
    Ok(())
}

/// The output of a regression case for the guard: the script must be outside the contract
/// (upstream crashes on these scripts).
fn rejection(face: &FaceIn, ops: &[[f64; 5]]) -> Result<Output, HarnessError> {
    match script_contract(face, ops) {
        Err(_) => Ok(Output::err("rejected")),
        Ok(()) => Err(harness("expected the input contract to reject the script")),
    }
}

// ---- Rust ---------------------------------------------------------------------------

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn rust_circle(
    k: &Kernel,
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    d: [f64; 2],
    r: f64,
) -> Result<Curve, HarnessError> {
    let e = Ellipse::new_circle(
        k,
        v3(c),
        v3(x),
        v3(y),
        Extent1d::from_min_max(d[0], d[1]),
        r,
        &EllipseOptions::default(),
    )
    .map_err(|e| harness(format!("circle: {e}")))?;
    Ok(Curve::from(e))
}

/// The face's surface, as `read_surface` of the shim builds it.
fn rust_surface(k: &Kernel, f: &FaceIn) -> Result<Surface, HarnessError> {
    match &f.surf {
        None => Ok(Surface::from(Plane::new(
            v3(f.o),
            v3(f.x),
            v3(f.y),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(f.domain[0], f.domain[1], f.domain[2], f.domain[3]),
        ))),
        Some(SurfIn::Revolution {
            gen,
            point,
            axis,
            swap,
            sweep,
        }) => {
            let g = match gen {
                GenIn::Line { a, b } => {
                    let mut l = Line::from_points(v3(*a), v3(*b), 3);
                    l.make_nurb()
                        .map_err(|e| harness(format!("line nurb: {e}")))?;
                    Curve::from(l)
                }
                GenIn::Circle { c, x, y, d, r } => rust_circle(k, *c, *x, *y, *d, *r)?,
            };
            let rev = SurfOfRevolution::create_canonical(k, g, v3(*point), v3(*axis), *swap)
                .map_err(|e| harness(format!("create_canonical: {e}")))?;
            let mut s = Surface::from(rev);
            if *sweep < 360.0 {
                let d = s.step_uv_domain();
                s.adjust_step_uv_domain(
                    k,
                    &Extent2d::from_bounds(d.u_min(), d.v_min(), d.u_min() + *sweep, d.v_max()),
                )
                .map_err(|e| harness(format!("adjust_step_uv_domain: {e}")))?;
            }
            Ok(s)
        }
    }
}

/// The id the next record of an arena gets, which the record's links name before it exists.
macro_rules! next_id {
    ($arena:expr, $id:ident) => {
        $id::from_index(u32::try_from($arena.len()).map_err(|_| harness("too many records"))?)
    };
}

/// The B-rep of the shim, record by record: `SmBrepConstructor::StartFace` (an up and a down
/// face use in the one shell, the face owning its surface), `StartAndEndSingleVertexLoop`
/// (a vertex loop use on a vertex) or `StartLoop` / `StartEdge` / `EndLoop` (per edge loop a
/// loop use whose edge use list starts at the first requested edge use, one `SmLine` edge per
/// edge use, a vertex per edge use at its edge's start; the first loop use outer, the others
/// inner), then `SetTolerance` of the face.
pub(crate) fn rust_brep(k: &Kernel, f: &FaceIn) -> Result<(Brep, FaceId), HarnessError> {
    let mut b = Brep::new();
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let region = b
        .add_region_record(Region {
            in_brep: true,
            ..Region::default()
        })
        .map_err(e)?;
    let shell = b
        .add_shell_record(Shell {
            faces: Vec::new(),
            region: Some(region),
            ..Shell::default()
        })
        .map_err(e)?;

    // the face and its surface, which the face owns
    let mut surface = rust_surface(k, f)?;
    surface.set_owner(Some(ObjectRef {
        type_code: fp2::TYPE,
        key: ObjectKey(1),
    }));
    let uv_domain = surface.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))?;
    let face = b
        .add_face_record(Face {
            surface: Some(surface),
            uv_domain,
            tolerance: f.tol,
            ..Face::new(shell)
        })
        .map_err(e)?;
    b.shell_mut(shell).map_err(e)?.faces.push(face);
    let up = next_id!(b.faceuses(), FaceuseId);
    // the down face use is the record after the up one (both ids are taken before either
    // record is added, so they cannot both come from the arena's length)
    let down = FaceuseId::from_index(
        u32::try_from(b.faceuses().len() + 1).map_err(|_| harness("too many records"))?,
    );
    for (mate, orientation) in [(down, OrientType::Same), (up, OrientType::Opposite)] {
        b.add_faceuse_record(Faceuse {
            shell: Some(shell),
            face: Some(face),
            mate: Some(mate),
            orientation,
            ..Faceuse::default()
        })
        .map_err(e)?;
    }
    b.face_mut(face).map_err(e)?.faceuse = Some(up);

    match &f.loops {
        None => rust_vertex_loop(&mut b, face, up, [0.0; 3], true)?,
        Some(LoopsIn::Vertex { at }) => rust_vertex_loop(&mut b, face, up, *at, true)?,
        Some(LoopsIn::Edges { loops }) => {
            for (j, pts) in loops.iter().enumerate() {
                rust_edge_loop(&mut b, face, up, pts, j == 0)?;
            }
        }
        Some(LoopsIn::Circles { circles }) => {
            for (j, c) in circles.iter().enumerate() {
                let curve = rust_circle(k, c.c, c.x, c.y, c.d, c.r)?;
                rust_curve_loop(k, &mut b, face, up, curve, j == 0)?;
            }
        }
    }
    Ok((b, face))
}

/// A loop record and its loop use on the up face use.
fn rust_loop_records(
    b: &mut Brep,
    face: FaceId,
    up: FaceuseId,
    start: LoopuseStart,
    outer: bool,
) -> Result<LoopuseId, HarnessError> {
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let l = next_id!(b.loops(), LoopId);
    let lu = b
        .add_loopuse_record(Loopuse {
            faceuse: Some(up),
            loop_: Some(l),
            mate: None,
            start,
            orientation: if outer {
                OrientType::Same
            } else {
                OrientType::Opposite
            },
        })
        .map_err(e)?;
    b.add_loop_record(Loop {
        face,
        edges: Vec::new(),
        loopuse: Some(lu),
    })
    .map_err(e)?;
    b.faceuse_mut(up).map_err(e)?.loopuses.push(lu);
    b.face_mut(face).map_err(e)?.loops.push(l);
    Ok(lu)
}

fn rust_vertex_loop(
    b: &mut Brep,
    face: FaceId,
    up: FaceuseId,
    at: [f64; 3],
    outer: bool,
) -> Result<(), HarnessError> {
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let lu = next_id!(b.loopuses(), LoopuseId);
    let vu = next_id!(b.vertexuses(), VertexuseId);
    let v = b.add_vertex(v3(at)).map_err(e)?;
    b.add_vertexuse_record(Vertexuse {
        vertex: Some(v),
        owner: VertexuseOwner::Loopuse(Some(lu)),
        ..Vertexuse::default()
    })
    .map_err(e)?;
    b.vertex_mut(v).map_err(e)?.vertexuses.push(vu);
    rust_loop_records(b, face, up, LoopuseStart::Vertexuse(Some(vu)), outer)?;
    Ok(())
}

fn rust_edge_loop(
    b: &mut Brep,
    face: FaceId,
    up: FaceuseId,
    pts: &[[f64; 3]],
    outer: bool,
) -> Result<(), HarnessError> {
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let n = pts.len();
    let lu = next_id!(b.loopuses(), LoopuseId);
    let eu0 = next_id!(b.edgeuses(), EdgeuseId);
    let vu0 = next_id!(b.vertexuses(), VertexuseId);
    let e0 = next_id!(b.edges(), EdgeId);
    let v0 = next_id!(b.vertices(), VertexId);
    let at = |i: usize| -> Result<[f64; 3], HarnessError> {
        pts.get(i % n).copied().ok_or_else(|| harness("loop point"))
    };
    let index = |base: u32, i: usize| -> Result<u32, HarnessError> {
        base.checked_add(u32::try_from(i % n).map_err(|_| harness("loop size"))?)
            .ok_or_else(|| harness("too many records"))
    };

    // the vertices, at the start of each edge
    for i in 0..n {
        b.add_vertex(v3(at(i)?)).map_err(e)?;
    }
    // the edges: lines p[i] -> p[i + 1] over their natural interval
    for i in 0..n {
        let curve = Curve::from(Line::from_points(v3(at(i)?), v3(at(i + 1)?), 3));
        let interval = curve.natural_interval();
        let tolerance = b.tolerance();
        b.add_edge_record(Edge {
            vertices: [
                VertexId::from_index(index(v0.index(), i)?),
                VertexId::from_index(index(v0.index(), i + 1)?),
            ],
            edgeuses: vec![EdgeuseId::from_index(index(eu0.index(), i)?)],
            interval,
            tolerance,
            curve: Some(curve),
        })
        .map_err(e)?;
    }
    // the vertex uses and the edge uses (counter-clockwise: the next requested edge use)
    for i in 0..n {
        let eu = EdgeuseId::from_index(index(eu0.index(), i)?);
        let v = VertexId::from_index(index(v0.index(), i)?);
        b.add_vertexuse_record(Vertexuse {
            vertex: Some(v),
            owner: VertexuseOwner::Edgeuse(Some(eu)),
            ..Vertexuse::default()
        })
        .map_err(e)?;
        b.vertex_mut(v)
            .map_err(e)?
            .vertexuses
            .push(VertexuseId::from_index(index(vu0.index(), i)?));
    }
    for i in 0..n {
        b.add_edgeuse_record(Edgeuse {
            edge: Some(EdgeId::from_index(index(e0.index(), i)?)),
            owner: EdgeuseOwner::Loopuse(Some(lu)),
            ccw: Some(EdgeuseId::from_index(index(eu0.index(), i + 1)?)),
            cw: Some(EdgeuseId::from_index(index(eu0.index(), i + n - 1)?)),
            vertexuse: Some(VertexuseId::from_index(index(vu0.index(), i)?)),
            orientation: OrientType::Same,
            ..Edgeuse::default()
        })
        .map_err(e)?;
    }
    let made = rust_loop_records(b, face, up, LoopuseStart::Edgeuse(Some(eu0)), outer)?;
    if made != lu {
        return Err(harness("loop use id"));
    }
    Ok(())
}

/// A loop of the one edge `curve` over its natural interval, as `StartLoop` / `StartEdge` /
/// `EndLoop` build it: one vertex at the curve's start (`EndLoop` evaluates the edge use's
/// start), both ends of the edge on it, the edge use its own neighbour.
fn rust_curve_loop(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    up: FaceuseId,
    curve: Curve,
    outer: bool,
) -> Result<(), HarnessError> {
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let lu = next_id!(b.loopuses(), LoopuseId);
    let eu = next_id!(b.edgeuses(), EdgeuseId);
    let vu = next_id!(b.vertexuses(), VertexuseId);
    let edge = next_id!(b.edges(), EdgeId);
    let interval = curve.natural_interval();
    let start = curve
        .evaluate_point(k, interval.min())
        .map_err(|err| harness(format!("circle start: {err}")))?;
    let v = b.add_vertex(start).map_err(e)?;
    let tolerance = b.tolerance();
    b.add_edge_record(Edge {
        vertices: [v, v],
        edgeuses: vec![eu],
        interval,
        tolerance,
        curve: Some(curve),
    })
    .map_err(e)?;
    b.add_vertexuse_record(Vertexuse {
        vertex: Some(v),
        owner: VertexuseOwner::Edgeuse(Some(eu)),
        ..Vertexuse::default()
    })
    .map_err(e)?;
    b.vertex_mut(v).map_err(e)?.vertexuses.push(vu);
    b.add_edgeuse_record(Edgeuse {
        edge: Some(edge),
        owner: EdgeuseOwner::Loopuse(Some(lu)),
        ccw: Some(eu),
        cw: Some(eu),
        vertexuse: Some(vu),
        orientation: OrientType::Same,
        ..Edgeuse::default()
    })
    .map_err(e)?;
    let made = rust_loop_records(b, face, up, LoopuseStart::Edgeuse(Some(eu)), outer)?;
    if made != lu {
        return Err(harness("loop use id"));
    }
    Ok(())
}

/// The down face use's side of [`rust_brep`]'s B-rep, as `SmBrepConstructor` builds it with
/// the up side (`StartLoop`, `StartEdge`, `EndLoop`, `StartAndEndSingleVertexLoop`): per loop
/// a mate loop use on the down face use with the same orientation; per edge use a mate edge
/// use of its edge (orientation opposite, its vertex use at the edge's end vertex, after the
/// up edge use in the edge's list) walking the loop the other way, the mate loop use starting
/// at the mate of the up loop use's first edge use; each vertex gets the vertex use of the
/// mate ending at it after its own. A vertex loop gets a second vertex use of its vertex.
/// [`rust_brep`] leaves this side out; units whose queries walk edge mates (an edge's
/// vertices, its faces) add it.
pub(crate) fn add_down_side(b: &mut Brep, face: FaceId) -> Result<(), HarnessError> {
    let e = |e: solidcore::SolidError| harness(e.to_string());
    let up = b.face(face).map_err(e)?.faceuse.ok_or_else(|| harness("face use"))?;
    let down = b.faceuse(up).map_err(e)?.mate.ok_or_else(|| harness("face use mate"))?;
    let loops = b.face(face).map_err(e)?.loops.clone();
    for l in loops {
        let lu1 = b.loop_(l).map_err(e)?.loopuse.ok_or_else(|| harness("loop use"))?;
        let (start, orientation) = {
            let r = b.loopuse(lu1).map_err(e)?;
            (r.start, r.orientation)
        };
        let lu2 = next_id!(b.loopuses(), LoopuseId);
        let start2 = match start {
            LoopuseStart::Vertexuse(Some(vu1)) => {
                let v = b.vertexuse(vu1).map_err(e)?.vertex.ok_or_else(|| harness("vertex"))?;
                let vu2 = next_id!(b.vertexuses(), VertexuseId);
                b.add_vertexuse_record(Vertexuse {
                    vertex: Some(v),
                    owner: VertexuseOwner::Loopuse(Some(lu2)),
                    ..Vertexuse::default()
                })
                .map_err(e)?;
                b.vertex_mut(v).map_err(e)?.vertexuses.push(vu2);
                LoopuseStart::Vertexuse(Some(vu2))
            }
            LoopuseStart::Edgeuse(Some(eu0)) => {
                // the up loop in order
                let mut eus = vec![eu0];
                let mut eu = eu0;
                loop {
                    eu = b.edgeuse(eu).map_err(e)?.ccw.ok_or_else(|| harness("ccw edge use"))?;
                    if eu == eu0 {
                        break;
                    }
                    eus.push(eu);
                }
                let n = eus.len();
                let m0 = b.edgeuses().len();
                let vm0 = b.vertexuses().len();
                let id = |base: usize, i: usize| -> Result<u32, HarnessError> {
                    u32::try_from(base + i % n).map_err(|_| harness("too many records"))
                };
                let mut edges = Vec::new();
                for (i, &eu) in eus.iter().enumerate() {
                    let edge = b.edgeuse(eu).map_err(e)?.edge.ok_or_else(|| harness("edge"))?;
                    let [_, end] = b.edge(edge).map_err(e)?.vertices;
                    b.add_vertexuse_record(Vertexuse {
                        vertex: Some(end),
                        owner: VertexuseOwner::Edgeuse(Some(EdgeuseId::from_index(id(m0, i)?))),
                        ..Vertexuse::default()
                    })
                    .map_err(e)?;
                    edges.push((edge, end));
                }
                for (i, &(edge, _)) in edges.iter().enumerate() {
                    b.add_edgeuse_record(Edgeuse {
                        edge: Some(edge),
                        owner: EdgeuseOwner::Loopuse(Some(lu2)),
                        ccw: Some(EdgeuseId::from_index(id(m0, i + n - 1)?)),
                        cw: Some(EdgeuseId::from_index(id(m0, i + 1)?)),
                        vertexuse: Some(VertexuseId::from_index(id(vm0, i)?)),
                        orientation: OrientType::Opposite,
                        ..Edgeuse::default()
                    })
                    .map_err(e)?;
                    b.edge_mut(edge)
                        .map_err(e)?
                        .edgeuses
                        .push(EdgeuseId::from_index(id(m0, i)?));
                }
                // EndLoop: the vertex at the start of edge use i also takes the vertex use of
                // the mate of edge use i - 1 (which ends there)
                for (i, &(_, end)) in edges.iter().enumerate() {
                    b.vertex_mut(end)
                        .map_err(e)?
                        .vertexuses
                        .push(VertexuseId::from_index(id(vm0, i)?));
                }
                LoopuseStart::Edgeuse(Some(EdgeuseId::from_index(id(m0, 0)?)))
            }
            _ => return Err(harness("loop use start")),
        };
        b.add_loopuse_record(Loopuse {
            faceuse: Some(down),
            loop_: Some(l),
            mate: Some(lu1),
            start: start2,
            orientation,
        })
        .map_err(e)?;
        b.loopuse_mut(lu1).map_err(e)?.mate = Some(lu2);
        b.faceuse_mut(down).map_err(e)?.loopuses.push(lu2);
    }
    Ok(())
}

fn status(r: solidcore::Result<()>) -> Result<f64, solidcore::SolidError> {
    match r {
        Ok(()) => Ok(1000.0),
        Err(e) if e.is_unported() => Err(e),
        Err(solidcore::SolidError::Upstream { code, .. }) => Ok(f64::from(code)),
        Err(e) => Err(e),
    }
}

pub(crate) fn snapshot<O: LoopObject>(p: &FaceProps<O>, face: FaceId) -> Vec<f64> {
    let t = |x: solidcore::heal::face_props::Tri| x.code() as f64;
    let mut o: Vec<f64> = vec![p.prop_stage.code() as f64];
    o.push(p.not_yet_face_type.code() as f64);
    o.push(t(p.bad_has_not_yet_prob));
    o.push(t(p.bad_degen_face));
    o.push(match p.face {
        None => 0.0,
        Some(f) if f == face => 1.0,
        Some(_) => 2.0,
    });
    o.push(p.face_index as f64);
    o.push(p.zone_tol_3d);
    o.push(p.orig_zone_tol_3d);
    o.push(t(p.bad_small_zone_tol_3d));
    o.push(t(p.bad_large_zone_tol_3d));
    o.push(f64::from(u8::from(p.has_surface)));
    o.push(p.sheet.code() as f64);
    o.push(t(p.closed_u));
    o.push(t(p.closed_v));
    o.push(t(p.closed_surf));
    o.push(f64::from(p.closed_val_u as i32));
    o.push(f64::from(p.closed_val_v as i32));
    o.push(p.poles_link.code() as f64);
    o.push(p.poles as f64);
    o.push(p.approx_poles as f64);
    o.push(p.no_vertex_poles as f64);
    o.push(p.bad_flat_corners as f64);
    o.push(f64::from(u8::from(p.crv_class_u.is_some())));
    o.push(f64::from(u8::from(p.crv_class_v.is_some())));
    o.push(t(p.bad_crossed_seam));
    o.push(f64::from(p.bad_crossed_seam_dir as i32));
    o.push(t(p.bad_missing_seam));
    o.push(f64::from(p.bad_missing_seam_dir as i32));
    o.push(t(p.bad_near_miss_seam));
    o.push(f64::from(p.bad_near_miss_seam_dir as i32));
    o.push(p.been_through_move_seam.code() as f64);
    o.push(p.been_through_split_edges_at_seam.code() as f64);
    o.push(p.been_through_split_faces_at_seam.code() as f64);
    o.push(p.closed_loop_cnt as f64);
    o.push(p.outer_loop_cnt as f64);
    o.push(t(p.bad_loop_problems));
    o.push(t(p.bad_outer_loop_order));
    o.push(p.bad_orient_loop_cnt as f64);
    o.push(p.bad_no_area_loop_cnt as f64);
    o.push(p.bad_nested_loop_cnt as f64);
    o.push(t(p.bad_unpaired_no_area_loop));
    o.push(p.bad_closed_3d_loop_cnt as f64);
    o.push(p.bad_closed_ptr_loop_cnt as f64);
    o.push(p.bad_missing_poles as f64);
    o.push(p.loop_props.len() as f64);
    o.push(p.pole_points.len() as f64);
    for i in 0..4 {
        match p.pole_points.get(i) {
            Some(v) => o.extend([v.x, v.y, v.z]),
            None => o.extend([0.0; 3]),
        }
    }
    o.push(p.natural_uv_domain.u_min());
    o.push(p.natural_uv_domain.v_min());
    o.push(p.natural_uv_domain.u_max());
    o.push(p.natural_uv_domain.v_max());
    o.push(p.flat_corner_points.len() as f64);
    for i in 0..4 {
        match p.flat_corner_points.get(i) {
            Some(v) => o.extend([v.x, v.y, v.z]),
            None => o.extend([0.0; 3]),
        }
    }
    o.push(t(p.bad_crossed_pole));
    o.push(f64::from(u8::from(p.crossed_pole_edge.is_some())));
    o.push(p.crossed_pole_parameter);
    o.resize(SNAP_LEN, 0.0);
    o
}

/// Op 8: `SmLoopProps::SetProps` of every loop of the face (the loop-props loop of
/// `SetProps_Stage3`): the loop count, then the props of the first loops.
fn loop_snapshot(
    k: &Kernel,
    b: &Brep,
    p: &FaceProps<H>,
    face: FaceId,
) -> Result<(f64, Vec<f64>), solidcore::SolidError> {
    let t = |x: solidcore::heal::face_props::Tri| x.code() as f64;
    let loops = solidcore::topology::face::p2::loops(b, face, false, None)?;
    let mut o: Vec<f64> = Vec::new();
    for l in loops.iter().take(MAX_LOOP_SNAPS) {
        let mut lp = LoopProps::new();
        lp.set_props(k, b, *l, p)?;
        o.push(lp.loopuse_type.code() as f64);
        o.push(t(lp.vertex_loop_on_pole));
        o.push(lp.edge_cnt as f64);
        o.push(lp.edgeuse_cnt as f64);
        o.push(t(lp.closed_3d));
        o.push(t(lp.closed_ptrs));
        o.push(t(lp.good_closed));
        o.push(lp.seam_crossing_cnt_u as f64);
        o.push(lp.seam_winding_cnt_u as f64);
        o.push(orient_code(lp.winding_orient_u) as f64);
        o.push(lp.seam_crossing_cnt_v as f64);
        o.push(lp.seam_winding_cnt_v as f64);
        o.push(orient_code(lp.winding_orient_v) as f64);
        o.push(t(lp.area_loop));
    }
    o.resize(SNAP_LEN, 0.0);
    Ok((loops.len() as f64, o))
}

/// Op 9: the loop props `SetProps_Stage3` left in the face props (the first loops): the
/// loop's index on the face, the `SmLoopProps::SetProps` props and the `ClassifyLoops`
/// containment and orientations.
fn stage3_loop_snapshot(b: &Brep, p: &FaceProps<H>, face: FaceId) -> Vec<f64> {
    let t = |x: solidcore::heal::face_props::Tri| x.code() as f64;
    let loops = b.face(face).map(|f| f.loops.clone()).unwrap_or_default();
    let mut o: Vec<f64> = Vec::new();
    for lp in p.loop_props.iter().take(MAX_LOOP_SNAPS) {
        let at = lp
            .loop_
            .and_then(|l| loops.iter().position(|x| *x == l))
            .map_or(-1.0, |i| i as f64);
        o.push(at);
        o.push(lp.loopuse_type.code() as f64);
        o.push(lp.edge_cnt as f64);
        o.push(t(lp.closed_3d));
        o.push(t(lp.closed_ptrs));
        o.push(t(lp.good_closed));
        o.push(t(lp.area_loop));
        o.push(lp.containment.code() as f64);
        o.push(orient_code(lp.loop_orient) as f64);
        o.push(orient_code(lp.desired_loop_orient) as f64);
        o.push(t(lp.good_orient));
    }
    o.resize(SNAP_LEN, 0.0);
    o
}

pub(crate) fn seed<O: LoopObject>(p: &mut FaceProps<O>, face: FaceId, field: i64, idx: i64, v: f64) {
    use solidcore::heal::face_props::{MixedLinkType as M, Tri};
    let tri = |x: f64| match x as i64 {
        0 => Tri::False,
        1 => Tri::True,
        _ => Tri::Unsure,
    };
    let mixed = |x: f64| M::from_code(x as i64).unwrap_or(M::Undef);
    match field {
        0 => p.prop_stage = PropStage::from_code(v as i64).unwrap_or(PropStage::Stage0),
        1 => p.zone_tol_3d = v,
        2 => p.orig_zone_tol_3d = v,
        3 => p.poles = v as u64,
        4 => {
            if let Some(pt) = usize::try_from(idx).ok().and_then(|i| p.pole_points.get_mut(i)) {
                *pt = Vec3::new(v, v * 0.5, -v);
            }
        }
        5 => p.bad_small_zone_tol_3d = tri(v),
        6 => p.bad_large_zone_tol_3d = tri(v),
        7 => p.sheet = mixed(v),
        8 => p.poles_link = mixed(v),
        9 => p.bad_flat_corners = v as u64,
        10 => p.bad_crossed_seam = tri(v),
        11 => p.bad_missing_seam = tri(v),
        12 => p.outer_loop_cnt = v as u64,
        13 => p.bad_orient_loop_cnt = v as u64,
        14 => p.bad_no_area_loop_cnt = v as u64,
        15 => p.bad_nested_loop_cnt = v as u64,
        16 => p.bad_unpaired_no_area_loop = tri(v),
        17 => p.bad_closed_3d_loop_cnt = v as u64,
        18 => p.bad_closed_ptr_loop_cnt = v as u64,
        19 => p.bad_missing_poles = v as u64,
        20 => p.bad_outer_loop_order = tri(v),
        21 => {
            p.not_yet_face_type =
                solidcore::heal::face_props::HealNotYetFaceType::from_code(v as i64)
                    .unwrap_or_default();
        }
        22 => p.closed_u = tri(v),
        23 => p.closed_v = tri(v),
        24 => p.face = if v == 0.0 { None } else { Some(face) },
        25 => p.bad_has_not_yet_prob = tri(v),
        _ => {}
    }
}

fn rust_props(f: &FaceIn, ops: &[[f64; 5]]) -> Result<Output, HarnessError> {
    script_contract(f, ops).map_err(harness)?;
    let k = Kernel::default();
    let (mut b, face) = rust_brep(&k, f)?;
    // the down face use's side, as `SmBrepConstructor` builds it: the seam classifications
    // snap edge-end hits to `GetOtherVertex`, which walks the edge use mates
    add_down_side(&mut b, face)?;
    let mut p: FaceProps<H> = FaceProps::new();
    let mut out: Vec<f64> = Vec::new();
    for o in ops {
        let op = o[0] as i64;
        let heal = HealerOpType::from_code(o[2] as i64).unwrap_or_default();
        let stage = PropStage::from_code(o[1] as i64).unwrap_or_default();
        let res: Result<f64, solidcore::SolidError> = match op {
            // the healer's calls on its B-rep: what the face queries leave on it (UV trim
            // curves, tolerance growth, gap caches) stays for the next op, as upstream
            0 => status(p.set_props_with_query_gaps(&k, &mut b, stage, face, o[3] as u64, heal)),
            1 => {
                p.reset(stage);
                Ok(0.0)
            }
            2 => Ok(f64::from(u8::from(
                p.has_props(stage, (o[2] != 0.0).then_some(face)),
            ))),
            3 => Ok(f64::from(u8::from(p.has_problems(o[1] as u64)))),
            4 => Ok(f64::from(u8::from(
                p.is_point3d_on_pole(Vec3::new(o[1], o[2], o[3])),
            ))),
            5 => {
                seed(&mut p, face, o[1] as i64, o[2] as i64, o[3]);
                Ok(0.0)
            }
            6 => match o[1] as i64 {
                1 => status(p.set_props_gaps(&k, &b, face, o[3] as u64, heal)),
                2 => status(p.set_props_stage2_mut(&k, &mut b, face, o[3] as u64, heal)),
                _ => status(p.set_props_stage3_mut(&k, &mut b, face, o[3] as u64, heal)),
            },
            7 => Ok(f64::from(u8::from(p.is_periodic_u())) + 2.0 * f64::from(u8::from(p.is_periodic_v()))),
            9 => {
                out.push(p.loop_props.len() as f64);
                out.extend(stage3_loop_snapshot(&b, &p, face));
                continue;
            }
            8 => match loop_snapshot(&k, &b, &p, face) {
                Ok((n, snap)) => {
                    out.push(n);
                    out.extend(snap);
                    continue;
                }
                Err(e) => Err(e),
            },
            _ => Err(solidcore::SolidError::invalid_input("runner", "op")),
        };
        match res {
            Ok(r) => {
                out.push(r);
                out.extend(snapshot(&p, face));
            }
            Err(e) => {
                return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
            }
        }
    }
    Ok(Output::new().values("v", out))
}

fn rust_tree(
    items: &[[i64; 2]],
    links: &[[i64; 3]],
    finds: &[[i64; 2]],
) -> Result<Output, HarnessError> {
    tree_contract(items, links, finds).map_err(harness)?;

    let lid = |i: i64| LoopId::from_index(i as u32);
    let mut t = Looptree::new();
    let mut ids: Vec<ItemId> = Vec::new();
    for i in items {
        let mut item = LooptreeItem::with_loop(if i[0] < 0 { None } else { Some(lid(i[0])) }, None, None);
        if i[1] != -2 {
            item.add_partner(if i[1] < 0 { None } else { Some(lid(i[1])) }, None, None);
        }
        ids.push(t.push(item));
    }
    let id = |i: i64| -> Result<ItemId, HarnessError> {
        usize::try_from(i)
            .ok()
            .and_then(|i| ids.get(i).copied())
            .ok_or_else(|| harness("item"))
    };
    for l in links {
        let (a, c) = (id(l[1])?, id(l[2])?);
        let r = if l[0] == 0 { t.add_child(a, c) } else { t.add_sibling(a, c) };
        r.map_err(|e| harness(e.to_string()))?;
    }
    let mut out: Vec<f64> = Vec::new();
    for f in finds {
        let found = t
            .find_item(id(f[0])?, if f[1] < 0 { None } else { Some(lid(f[1])) })
            .map_err(|e| harness(e.to_string()))?;
        let pos = |x: ItemId| ids.iter().position(|y| *y == x).map_or(-1.0, |p| p as f64);
        match found {
            Some(r) => {
                out.push(pos(r.item));
                out.push(r.parent.map_or(-1.0, pos));
            }
            None => out.extend([-1.0, -1.0]),
        }
    }
    for i in &ids {
        out.push(f64::from(u8::from(
            t.item(*i).map_err(|e| harness(e.to_string()))?.is_no_area_pair(),
        )));
    }
    Ok(Output::new().values("v", out))
}

fn rust(c: &CaseIn) -> Result<Output, HarnessError> {
    match c {
        CaseIn::Props {
            face,
            ops,
            expect_rejected,
        } => {
            if *expect_rejected {
                rejection(face, ops)
            } else {
                rust_props(face, ops)
            }
        }
        CaseIn::Tree {
            items,
            links,
            finds,
        } => rust_tree(items, links, finds),
    }
}

// ---- generation ---------------------------------------------------------------------

/// The pole points of a generated surface (a sphere's ends, a cone's apex).
fn pole_points(surf: Option<&SurfIn>) -> Vec<[f64; 3]> {
    let Some(SurfIn::Revolution { gen, .. }) = surf else {
        return Vec::new();
    };
    match gen {
        GenIn::Line { b, .. } if b[0] == 0.0 => vec![*b],
        GenIn::Circle { c, d, r, .. } if c[0] == 0.0 => {
            // the arc ends on the axis at +-90 degrees
            let mut v = Vec::new();
            if (d[0] + 90.0).abs() < 1e-9 {
                v.push([0.0, 0.0, -*r]);
            }
            if (d[1] - 90.0).abs() < 1e-9 {
                v.push([0.0, 0.0, *r]);
            }
            v
        }
        _ => Vec::new(),
    }
}

/// A generated revolution: a cylinder, a cone (with or without apex), a sphere or a torus,
/// closed (swept 360 degrees) or open.
fn gen_revolution(rng: &mut Rng) -> SurfIn {
    let gs = *rng.choose(&[1.0, 1.0, 1.0, 1.0, 1e-3, 1e3]).unwrap_or(&1.0);
    let r = rng.uniform(0.5, 3.0) * gs;
    let gen = match rng.below(4) {
        // a cylinder
        0 => GenIn::Line {
            a: [r, 0.0, rng.uniform(-2.0, 0.0) * gs],
            b: [r, 0.0, rng.uniform(0.5, 3.0) * gs],
        },
        // a cone; one in three has its apex on the axis (a pole)
        1 => {
            let r2 = if rng.chance(0.4) {
                0.0
            } else {
                r * rng.uniform(0.2, 2.0)
            };
            GenIn::Line {
                a: [r, 0.0, rng.uniform(-2.0, 0.0) * gs],
                b: [r2, 0.0, rng.uniform(0.5, 3.0) * gs],
            }
        }
        // a sphere: the arc reaches the axis at none, one or both ends (poles)
        2 => {
            let ends = [[-90.0, 90.0], [-90.0, 40.0], [-30.0, 90.0], [-30.0, 70.0], [-90.0, 90.0]];
            let d = *rng.choose(&ends).unwrap_or(&[-90.0, 90.0]);
            GenIn::Circle {
                c: [0.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 0.0, 1.0],
                d,
                r,
            }
        }
        // a torus: a full circle about an axis outside it, or an arc of it
        _ => {
            let d = if rng.chance(0.5) {
                [0.0, 360.0]
            } else {
                let d0 = rng.uniform(-150.0, 0.0);
                [d0, d0 + rng.uniform(60.0, 150.0)]
            };
            GenIn::Circle {
                c: [r * 2.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 0.0, 1.0],
                d,
                r,
            }
        }
    };
    let sweep = if rng.chance(0.45) {
        360.0
    } else {
        *rng.choose(&[90.0, 180.0, 270.0, 300.0]).unwrap_or(&270.0)
    };
    SurfIn::Revolution {
        gen,
        point: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        swap: rng.chance(0.3),
        sweep,
    }
}

/// A closed polygon of `n` corners about `c`; `ccw` false runs it clockwise (a hole).
fn gen_polygon(rng: &mut Rng, c: [f64; 3], radius: f64, ccw: bool) -> Vec<[f64; 3]> {
    let n = 3 + rng.below(4) as usize;
    let mut pts: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * (i as f64 + rng.uniform(0.0, 0.4)) / n as f64;
            let r = radius * rng.uniform(0.8, 1.0);
            [c[0] + r * a.cos(), c[1] + r * a.sin(), c[2]]
        })
        .collect();
    if !ccw {
        pts.reverse();
    }
    pts
}

fn gen_loops(rng: &mut Rng, poles: &[[f64; 3]]) -> Option<LoopsIn> {
    let pick_pole = |rng: &mut Rng| -> Option<[f64; 3]> { rng.choose(poles).copied() };
    match rng.below(10) {
        // the one vertex loop at the origin
        0..=3 => None,
        // a vertex loop at a pole or a random point
        4 | 5 => {
            let at = match pick_pole(rng) {
                Some(p) if rng.chance(0.8) => p,
                _ => rng.point3(-2.0, 2.0),
            };
            Some(LoopsIn::Vertex { at })
        }
        // an outer edge loop, and often holes
        _ => {
            let c = rng.point3(-1.0, 1.0);
            let radius = rng.uniform(1.0, 3.0);
            let mut outer = gen_polygon(rng, c, radius, true);
            if let (Some(p), true) = (pick_pole(rng), rng.chance(0.4)) {
                if let Some(first) = outer.first_mut() {
                    *first = p;
                }
            }
            let mut loops = vec![outer];
            if rng.chance(0.6) {
                let radius = rng.uniform(0.2, 0.7);
                loops.push(gen_polygon(rng, c, radius, false));
                if rng.chance(0.25) {
                    let c2 = [c[0] + 0.1, c[1] - 0.1, c[2]];
                    loops.push(gen_polygon(rng, c2, 0.15, false));
                }
            }
            Some(LoopsIn::Edges { loops })
        }
    }
}

pub(crate) fn gen_face(rng: &mut Rng) -> FaceIn {
    let tols = [
        1e-5,
        1e-6,
        2e-5,
        1e-4,
        1e-3,
        5e-6,
        1e-7,
        1e-5 + 1e-12,
        1e-5,
        1e-5,
        // large tolerances make every side of the surface (approximately) singular
        1e-2,
        1.0,
        1e3,
        1e9,
    ];
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let a = rng.uniform(-3.0, 0.0);
    let b = rng.uniform(-3.0, 0.0);
    let mut domain = [a, b, a + rng.uniform(0.5, 4.0), b + rng.uniform(0.5, 4.0)];
    if rng.chance(0.08) {
        // a degenerate domain: every side is a pole
        let t = *rng.choose(&[1e-300, 1e-100, 1e-14, 1e-9]).unwrap_or(&1e-14);
        domain = [0.0, 0.0, t, t];
    }
    let surf = rng.chance(0.55).then(|| gen_revolution(rng));
    let loops = gen_loops(rng, &pole_points(surf.as_ref()));
    FaceIn {
        tol: *rng.choose(&tols).unwrap_or(&1e-5),
        o: rng.point3(-2.0, 2.0),
        x,
        y,
        domain,
        surf,
        loops,
    }
}

fn gen_op(rng: &mut Rng) -> [f64; 5] {
    let heal_ops = [0, 1, 4, 5, 6, 12, 17, 18, 19, 20, 21, 26];
    let heal = f64::from(*rng.choose(&heal_ops).unwrap_or(&26));
    let stage = rng.range_i64(0, 6) as f64;
    match rng.below(16) {
        0 | 1 => [0.0, stage, heal, rng.range_i64(0, 5) as f64, 0.0],
        2 => [1.0, stage, 0.0, 0.0, 0.0],
        3 => [2.0, stage, rng.range_i64(0, 1) as f64, 0.0, 0.0],
        4 => [3.0, rng.range_i64(0, 7) as f64, 0.0, 0.0, 0.0],
        5 => {
            let p = rng.point3(-2.0, 2.0);
            [4.0, p[0], p[1], p[2], 0.0]
        }
        6 => [6.0, rng.range_i64(1, 3) as f64, heal, rng.range_i64(0, 5) as f64, 0.0],
        7 => [7.0, 0.0, 0.0, 0.0, 0.0],
        8 | 9 => [8.0, 0.0, 0.0, 0.0, 0.0],
        _ => {
            // a seed; the stage seed is followed by a face seed (a stage past the gaps
            // with no face would be dereferenced upstream, UB-131)
            let field = rng.range_i64(0, 25);
            let v = match field {
                1 | 2 => *rng.choose(&[1e-5, 1e-3, 0.0, 0.5]).unwrap_or(&1e-5),
                3 | 9 => rng.range_i64(0, 15) as f64,
                4 => rng.uniform(-1.0, 1.0),
                7 | 8 => rng.range_i64(0, 5) as f64,
                12..=15 | 17..=19 | 21 => rng.range_i64(0, 3) as f64,
                24 => 1.0,
                _ => rng.range_i64(0, 2) as f64,
            };
            [5.0, field as f64, rng.range_i64(0, 3) as f64, v, 0.0]
        }
    }
}

fn gen_props(rng: &mut Rng) -> CaseIn {
    let face = gen_face(rng);
    let n = rng.range_i64(1, 8);
    let mut ops: Vec<[f64; 5]> = Vec::new();
    if rng.chance(0.4) {
        // the stage 2 props first (the surface, closed, pole and corner classification),
        // so that the ops after it see real values
        ops.push([0.0, 4.0, 26.0, rng.range_i64(0, 5) as f64, 0.0]);
    }
    for _ in 0..n {
        let mut o = gen_op(rng);
        // seeds of the stage keep the face (a stage past the gaps with no face would be
        // dereferenced upstream) and stay below stage 2 (seeded stage 2 props have no seam
        // classifications, which stage 3 upstream dereferences)
        if o[0] == 5.0 && o[1] == 0.0 {
            o[3] = rng.range_i64(0, 2) as f64;
            ops.push([5.0, 24.0, 0.0, 1.0, 0.0]);
        }
        if o[0] == 1.0 && o[1] >= 3.0 {
            // a reset past the gaps keeps the face pointer but leaves a stage past the gaps
            // (upstream then dereferences a null face on the next stage 2)
            ops.push([5.0, 24.0, 0.0, 1.0, 0.0]);
        }
        ops.push(o);
    }
    if rng.chance(0.3) {
        ops.push([8.0, 0.0, 0.0, 0.0, 0.0]);
    }
    // seeded state is not what stage 3 upstream expects: scripts with seeds stop short of
    // the loop classification (heal op before Fix_SplitEdgesAtSeam)
    if ops.iter().any(|o| o[0] == 5.0) {
        for o in &mut ops {
            let stage3 = (o[0] == 0.0 && o[1] >= 5.0) || (o[0] == 6.0 && o[1] == 3.0);
            if stage3 && o[2] >= 19.0 {
                o[2] = 6.0;
            }
        }
    }
    CaseIn::Props {
        face,
        ops,
        expect_rejected: false,
    }
}

/// Generated `SetProps_Stage3` cases (named `s3gen####`, their own seeds): drawn so that
/// `ClassifyLoops` classifies the loops and Stage 3 runs to its end.
pub const STAGE3_GENERATED: usize = 300;

/// A circle loop about the z axis at height `z` (radius `r`), counter-clockwise seen from +z
/// unless `ccw` is false, over `d` degrees.
fn z_circle(c: [f64; 3], r: f64, ccw: bool, d: [f64; 2]) -> CircleIn {
    CircleIn {
        c,
        x: [1.0, 0.0, 0.0],
        y: if ccw { [0.0, 1.0, 0.0] } else { [0.0, -1.0, 0.0] },
        d,
        r,
    }
}

/// Loops on the plane `z = z0`: polygons or circles about one centre (outer, hole, island,
/// hole in the island) or side by side, each with a random orientation now and then, in a
/// random order now and then.
fn gen_plane_loops(rng: &mut Rng, z0: f64) -> LoopsIn {
    let c = [rng.uniform(-0.5, 0.5), rng.uniform(-0.5, 0.5), z0];
    let n = 1 + rng.below(4) as usize;
    let nested = rng.chance(0.7);
    let circles = rng.chance(0.4);
    let mut specs: Vec<([f64; 3], f64, bool)> = Vec::new();
    for i in 0..n {
        let (centre, radius) = if nested {
            (c, 3.0 * 0.6_f64.powi(i as i32) * rng.uniform(0.9, 1.0))
        } else {
            ([c[0] - 2.25 + 1.5 * i as f64, c[1], z0], 0.6)
        };
        // the nesting alternates the orientations: outer, hole, island, hole
        let mut ccw = !nested || i % 2 == 0;
        if rng.chance(0.2) {
            ccw = !ccw;
        }
        specs.push((centre, radius, ccw));
    }
    if n > 1 && rng.chance(0.2) {
        specs.swap(0, 1);
    }
    if circles {
        let open = rng.chance(0.1);
        LoopsIn::Circles {
            circles: specs
                .iter()
                .map(|&(centre, r, ccw)| {
                    let d = if open { [0.0, rng.uniform(200.0, 350.0)] } else { [0.0, 360.0] };
                    z_circle(centre, r, ccw, d)
                })
                .collect(),
        }
    } else {
        LoopsIn::Edges {
            loops: specs
                .iter()
                .map(|&(centre, r, ccw)| gen_polygon(rng, centre, r, ccw))
                .collect(),
        }
    }
}

/// A surface of revolution about the z axis with latitude circle loops on it: a cylinder, a
/// cone (with or without its apex) or a sphere, the circles at random heights and
/// orientations (no-area loops on a closed surface), now and then over less than a turn.
fn gen_revolution_circles(rng: &mut Rng) -> (SurfIn, LoopsIn) {
    let r = rng.uniform(0.5, 2.0);
    let n = 1 + rng.below(3) as usize;
    let mut heights: Vec<f64> = (0..n).map(|_| rng.uniform(0.05, 0.95)).collect();
    heights.sort_by(f64::total_cmp);
    let (gen, at): (GenIn, Vec<([f64; 3], f64)>) = match rng.below(3) {
        0 | 1 => {
            let z0 = rng.uniform(-2.0, 0.0);
            let z1 = rng.uniform(0.5, 2.0);
            let r1 = if rng.chance(0.5) {
                r
            } else if rng.chance(0.4) {
                0.0
            } else {
                r * rng.uniform(0.3, 1.8)
            };
            let at = heights
                .iter()
                .map(|t| {
                    let z = z0 + (z1 - z0) * t;
                    ([0.0, 0.0, z], r + (r1 - r) * ((z - z0) / (z1 - z0)))
                })
                .collect();
            (
                GenIn::Line {
                    a: [r, 0.0, z0],
                    b: [r1, 0.0, z1],
                },
                at,
            )
        }
        _ => {
            let at = heights
                .iter()
                .map(|t| {
                    let phi = (-80.0 + 160.0 * t).to_radians();
                    ([0.0, 0.0, r * phi.sin()], r * phi.cos())
                })
                .collect();
            let d = *rng
                .choose(&[[-90.0, 90.0], [-90.0, 90.0], [-90.0, 85.0], [-85.0, 90.0]])
                .unwrap_or(&[-90.0, 90.0]);
            (
                GenIn::Circle {
                    c: [0.0; 3],
                    x: [1.0, 0.0, 0.0],
                    y: [0.0, 0.0, 1.0],
                    d,
                    r,
                },
                at,
            )
        }
    };
    let sweep: f64 = if rng.chance(0.8) { 360.0 } else { *rng.choose(&[180.0, 270.0]).unwrap_or(&270.0) };
    let circles = at
        .iter()
        .map(|&(c, radius)| {
            let d = if sweep < 360.0 || rng.chance(0.1) {
                [0.0, sweep.min(rng.uniform(150.0, 350.0))]
            } else {
                [0.0, 360.0]
            };
            z_circle(c, radius, rng.chance(0.5), d)
        })
        .collect();
    (
        SurfIn::Revolution {
            gen,
            point: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            swap: rng.chance(0.3),
            sweep,
        },
        LoopsIn::Circles { circles },
    )
}

fn gen_stage3(rng: &mut Rng) -> CaseIn {
    let tol = *rng
        .choose(&[1e-5, 1e-5, 1e-5, 1e-6, 1e-4, 1e-3, 1e-2, 0.1, 1.0])
        .unwrap_or(&1e-5);
    let z0 = rng.uniform(-1.0, 1.0);
    let (surf, loops) = if rng.chance(0.5) {
        (None, gen_plane_loops(rng, z0))
    } else {
        let (s, l) = gen_revolution_circles(rng);
        (Some(s), l)
    };
    let face = FaceIn {
        tol,
        o: [0.0, 0.0, z0],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        domain: [-4.0, -4.0, 4.0, 4.0],
        surf,
        loops: Some(loops),
    };
    let heal = f64::from(*rng.choose(&[19, 20, 21, 26, 26, 26]).unwrap_or(&26));
    let index = rng.range_i64(0, 5) as f64;
    let mut ops: Vec<[f64; 5]> = Vec::new();
    if rng.chance(0.3) {
        ops.push([0.0, 4.0, 26.0, index, 0.0]);
    }
    if rng.chance(0.5) {
        ops.push([0.0, 6.0, heal, index, 0.0]);
    } else {
        ops.push([6.0, 3.0, heal, index, 0.0]);
    }
    ops.push([9.0, 0.0, 0.0, 0.0, 0.0]);
    if rng.chance(0.3) {
        ops.push([3.0, rng.range_i64(0, 7) as f64, 0.0, 0.0, 0.0]);
    }
    if rng.chance(0.2) {
        // a reset to Stage 2 then Stage 3 again
        ops.push([1.0, 5.0, 0.0, 0.0, 0.0]);
        ops.push([0.0, 6.0, heal, index, 0.0]);
        ops.push([9.0, 0.0, 0.0, 0.0, 0.0]);
    }
    if rng.chance(0.3) {
        ops.push([8.0, 0.0, 0.0, 0.0, 0.0]);
    }
    CaseIn::Props {
        face,
        ops,
        expect_rejected: false,
    }
}

fn gen_tree(rng: &mut Rng) -> CaseIn {
    let n = rng.range_i64(1, 8);
    let mut items: Vec<[i64; 2]> = Vec::new();
    for i in 0..n {
        let loop_ = if rng.chance(0.1) { -1 } else { i };
        let partner = if rng.chance(0.3) {
            if rng.chance(0.2) { -1 } else { 20 + i }
        } else {
            -2
        };
        items.push([loop_, partner]);
    }
    // a forest: item i (i >= 1) is linked once under an earlier item
    let mut links: Vec<[i64; 3]> = Vec::new();
    for i in 1..n {
        if rng.chance(0.9) {
            links.push([rng.range_i64(0, 1), rng.range_i64(0, i - 1), i]);
        }
    }
    let mut finds: Vec<[i64; 2]> = Vec::new();
    for _ in 0..rng.range_i64(1, 6) {
        let l = if rng.chance(0.1) {
            -1
        } else if rng.chance(0.2) {
            20 + rng.range_i64(0, n)
        } else {
            rng.range_i64(0, n)
        };
        finds.push([rng.range_i64(0, n - 1), l]);
    }
    CaseIn::Tree {
        items,
        links,
        finds,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "heal.face_props"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // redraw scripts outside the input contract (the guard cases are fixtures)
        for _ in 0..64 {
            let c = if rng.chance(0.85) {
                gen_props(rng)
            } else {
                gen_tree(rng)
            };
            if let CaseIn::Props { face, ops, .. } = &c {
                if script_contract(face, ops).is_err() {
                    continue;
                }
            }
            return serde_json::to_value(c).ok();
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        // the Stage 3 family, on its own seeds (the `gen####` cases are unchanged)
        let family = "heal.face_props/stage3";
        for i in 0..STAGE3_GENERATED {
            let seed = CaseContext::seed_for(family, i);
            let mut rng = Rng::new(seed);
            let c = gen_stage3(&mut rng);
            if let CaseIn::Props { face, ops, .. } = &c {
                if script_contract(face, ops).is_err() {
                    continue;
                }
            }
            let inputs = serde_json::to_value(c).map_err(|e| harness(e.to_string()))?;
            cases.push(Case {
                unit: self.id().to_string(),
                name: format!("s3gen{i:04}"),
                source: CaseSource::Generated { seed },
                inputs,
            });
        }
        Ok(cases)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
