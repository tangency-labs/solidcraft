//! FFI wrappers for unit `heal.face_props`.
//!
//! Shim: `harness/cpp/shim/heal_face_props.cpp`.
//! Upstream: `source/SMLib/src/SmFaceProps.cpp`, `SmLoopProps.cpp`, `inc/SmLoopProps.h`.
//!
//! | Rust      | shim                        | upstream                                  |
//! |-----------|-----------------------------|-------------------------------------------|
//! | [`run`]   | `sc_heal_face_props_run`    | `SmFaceProps` methods on a one-face B-rep |
//! | [`tree`]  | `sc_heal_face_props_tree`   | `SmLooptreeItem` building and `FindItem`  |
//!
//! Script input (all `f64`): `face_tol`, the surface block, the loops block, `n_ops`, then per
//! op `op p0 p1 p2 p3`: `0` `SetProps`(stage p0, heal op p1, index p2), `1` `ReSet`(stage p0),
//! `2` `HasProps`(stage p0, face p1 != 0), `3` `HasProblems`(bits p0), `4`
//! `IsPoint3dOnPole`(p0, p1, p2), `5` seed field p0 (index p1, value p2; see [`SEED_FIELDS`]),
//! `6` `SetProps_Gaps` / `Stage2` / `Stage3` directly (which p0 in 1..=3, heal op p1, index
//! p2), `7` `IsPeriodicU` + 2 `IsPeriodicV`, `8` `SmLoopProps::SetProps` of every loop of the
//! face (the loop-props loop of `SetProps_Stage3`, without its `ClassifyLoops`). Output per
//! op: the result then [`SNAP_LEN`] snapshot values (for op `8` the first [`MAX_LOOP_SNAPS`]
//! loops' props instead of the face snapshot).
//!
//! Surface block: `0 O[3] X[3] Y[3] umin vmin umax vmax` (`SmPlane`) or `1 gen P[3] A[3] swap
//! sweep` (`SmSurfOfRevolution::CreateCanonical`, generator `0 a[3] b[3]` line or `1 c[3]
//! x[3] y[3] d0 d1 r` circle; a `sweep` below 360 degrees then `AdjustSTEPUVDomain` to
//! `[u0, u0 + sweep]` by the generator interval, an open surface). Loops block: `0 P[3]` (one
//! vertex loop at P) or `1 n_loops` then per loop `n_pts pts[3 * n_pts]` (an edge loop of
//! `SmLine` edges `p[i] -> p[i + 1]`; the first loop is the outer one).
//!
//! Guards (inputs the shim or upstream cannot take): finite values, a face tolerance above
//! zero, a plane with non-parallel axes and a non-empty domain, a revolution that
//! `CreateCanonical` builds, loops of at least three points with edges longer than
//! `1e-3`, stage and heal op codes in range, field ids in range with values in the range
//! of their field (an unsigned member takes no negative value: the cast is undefined),
//! and scripts that would dereference the null face pointer of upstream's `SetProps_Stage2` /
//! `SetProps_Stage3` ([`validate`] simulates the stage and face pointer; UB-131,
//! `plan/upstream-issues.md`).

use crate::{call_with_buffer, FfiError};

/// Values per snapshot.
pub const SNAP_LEN: usize = 82;
/// Largest number of ops.
pub const MAX_OPS: usize = 64;
/// Largest number of tree items.
pub const MAX_ITEMS: usize = 32;
/// Number of seedable fields (ids `0..SEED_FIELDS`).
pub const SEED_FIELDS: i64 = 26;
/// Loops whose props op `8` reports.
pub const MAX_LOOP_SNAPS: usize = 4;
/// Values per loop in an op `8` snapshot.
pub const LOOP_SNAP_LEN: usize = 14;
/// Largest number of loops.
pub const MAX_LOOPS: usize = 4;
/// Largest number of points per loop.
pub const MAX_LOOP_POINTS: usize = 8;

mod sys {
    extern "C" {
        pub fn sc_heal_face_props_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_heal_face_props_tree(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// One script op.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Op {
    /// Op code.
    pub op: i64,
    /// Operands.
    pub p: [f64; 4],
}

/// The generator of a surface of revolution.
#[derive(Debug, Clone, PartialEq)]
pub enum Gen {
    /// `SmLine(a, b, 3)` with its NURB made.
    Line {
        /// Start point.
        a: [f64; 3],
        /// End point.
        b: [f64; 3],
    },
    /// `SmCircle(c, x, y, [d0, d1], r, 3)`.
    Circle {
        /// Centre.
        c: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Domain in degrees.
        d: [f64; 2],
        /// Radius.
        r: f64,
    },
}

/// The face's surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Surf {
    /// `SmPlane(o, x, y, (1, 1), SmExtent2d(domain))`.
    Plane {
        /// Origin.
        o: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Domain `[umin, vmin, umax, vmax]`.
        domain: [f64; 4],
    },
    /// `SmSurfOfRevolution::CreateCanonical(gen, point, axis, swap)`, its angle domain then
    /// adjusted to `sweep` degrees when that is below 360.
    Revolution {
        /// Generator.
        gen: Gen,
        /// Axis point.
        point: [f64; 3],
        /// Axis direction.
        axis: [f64; 3],
        /// Swap the parameter directions.
        swap: bool,
        /// Angle swept about the axis, in degrees.
        sweep: f64,
    },
}

/// The loops of the face.
#[derive(Debug, Clone, PartialEq)]
pub enum Loops {
    /// One vertex loop at the point.
    Vertex([f64; 3]),
    /// Edge loops (the first one outer): per loop its corner points, joined by line edges.
    Edges(Vec<Vec<[f64; 3]>>),
}

/// The face of the one-face B-rep.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceSpec {
    /// Face tolerance.
    pub tol: f64,
    /// The face's surface.
    pub surf: Surf,
    /// The face's loops.
    pub loops: Loops,
}

fn bad<T>(m: &str) -> Result<T, FfiError> {
    Err(FfiError::InvalidArgument(m.into()))
}

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

fn finite(vals: impl IntoIterator<Item = f64>) -> bool {
    vals.into_iter().all(|v| v.is_finite() && v.abs() <= 1e9)
}

/// Checks the face the way the shim needs.
pub fn validate_face(face: &FaceSpec) -> Result<(), FfiError> {
    // the face tolerance: `SmFace::SetTolerance` turns a value at or below zero into the
    // default, which the harness's raw field write does not
    if !face.tol.is_finite() || face.tol <= 0.0 || face.tol > 1e9 {
        return bad("face tolerance");
    }
    match &face.surf {
        Surf::Plane { o, x, y, domain } => {
            if !finite(o.iter().chain(x).chain(y).chain(domain).copied()) {
                return bad("face values");
            }
            if sq_len(cross(*x, *y)) < 1e-6 {
                return bad("plane axes");
            }
            if domain[2] <= domain[0] || domain[3] <= domain[1] {
                return bad("plane domain");
            }
        }
        Surf::Revolution {
            gen,
            point,
            axis,
            sweep,
            ..
        } => {
            if !finite(point.iter().chain(axis).copied()) || !finite([*sweep]) {
                return bad("revolution values");
            }
            if sq_len(*axis) < 1e-6 || !(1.0..=360.0).contains(sweep) {
                return bad("revolution axis or sweep");
            }
            match gen {
                Gen::Line { a, b } => {
                    if !finite(a.iter().chain(b).copied()) || sq_dist(*a, *b) < 1e-6 {
                        return bad("generator line");
                    }
                }
                Gen::Circle { c, x, y, d, r } => {
                    if !finite(c.iter().chain(x).chain(y).chain(d).copied().chain([*r]))
                        || sq_len(cross(*x, *y)) < 1e-6
                        || *r < 1e-3
                        || d[1] - d[0] < 1.0
                        || d[1] - d[0] > 360.0
                    {
                        return bad("generator circle");
                    }
                }
            }
        }
    }
    match &face.loops {
        Loops::Vertex(p) => {
            if !finite(p.iter().copied()) {
                return bad("vertex loop point");
            }
        }
        Loops::Edges(loops) => {
            if loops.is_empty() || loops.len() > MAX_LOOPS {
                return bad("loop count");
            }
            for l in loops {
                if l.len() < 3 || l.len() > MAX_LOOP_POINTS {
                    return bad("loop point count");
                }
                for (i, p) in l.iter().enumerate() {
                    let Some(q) = l.get((i + 1) % l.len()).copied() else {
                        return bad("loop point");
                    };
                    if !finite(p.iter().copied()) || sq_dist(*p, q) < 1e-6 {
                        return bad("loop point");
                    }
                }
            }
        }
    }
    Ok(())
}

/// The `(stage, face pointer set)` pair of `SmFaceProps` the null pointer test needs.
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
    let mut cur: Vec<State> = vec![s];
    // SetProps_Gaps (HasProps_Stage1: stage >= GAPS)
    cur = cur
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
    cur = next;
    // SetProps_Stage3
    let mut next = Vec::new();
    for t in &cur {
        if stage == STAGE3 {
            next.extend(sim_stage3(*t, heal)?.into_iter().map(|x| x.0));
        } else {
            next.push(*t);
        }
    }
    next.sort_unstable();
    next.dedup();
    Some(next)
}

/// Rejects scripts in which `SetProps_Stage2` or `SetProps_Stage3` would dereference a null
/// `m_pFace` (UB-131): `SetProps_Gaps` returns early from `GAPSa` on, so the re-run that
/// `m_pFace != cpFace` asks for never sets the face.
fn simulate(ops: &[Op]) -> Result<(), FfiError> {
    let mut states: Vec<State> = vec![(0, false)];
    for o in ops {
        let mut next: Vec<State> = Vec::new();
        for s in &states {
            let s = *s;
            let r: Option<Vec<State>> = match o.op {
                0 => {
                    let stage = o.p[0] as i64;
                    sim_set_props(s, stage, o.p[1])
                }
                1 => {
                    let c = o.p[0] as i64;
                    let stage = if c <= GAPS {
                        0
                    } else if c <= STAGE2 {
                        GAPS
                    } else if c <= STAGE3 {
                        STAGE2
                    } else {
                        s.0
                    };
                    Some(vec![(stage, if c <= GAPS { false } else { s.1 })])
                }
                5 if o.p[0] as i64 == 0 => Some(vec![(o.p[2] as i64, s.1)]),
                5 if o.p[0] as i64 == 24 => Some(vec![(s.0, o.p[2] != 0.0)]),
                6 => {
                    let heal = o.p[1];
                    let r = match o.p[0] as i64 {
                        1 => Some(vec![(sim_gaps(s), true)]),
                        2 => sim_stage2(s, heal),
                        _ => sim_stage3(s, heal),
                    };
                    r.map(|v| v.into_iter().map(|x| x.0).collect())
                }
                _ => Some(vec![s]),
            };
            match r {
                Some(v) => next.extend(v),
                None => {
                    return bad(
                        "script would dereference the null face of SetProps_Stage2/3 (UB-131)",
                    )
                }
            }
        }
        next.sort_unstable();
        next.dedup();
        states = next;
    }
    Ok(())
}

/// Whether `SetProps_Stage2(face, index, SM_HO_ALL)` on an `SmFaceProps` seeded with `seeds`
/// (`(field, value)` in order, as the op `5` seeds) would dereference a null `m_pFace`
/// (UB-131). Only the stage (field `0`) and the face pointer (field `24`) matter.
#[must_use]
pub fn stage2_derefs_null_face(seeds: &[(i64, f64)]) -> bool {
    let mut s: State = (0, false);
    for &(field, v) in seeds {
        match field {
            0 => s.0 = v as i64,
            24 => s.1 = v != 0.0,
            _ => {}
        }
    }
    // SM_HO_ALL is past every threshold the simulation tests
    sim_stage2(s, 1000.0).is_none()
}

/// The range check of one seed (op `5`) value of field `field` (the field id is checked by
/// the caller: `0..SEED_FIELDS`).
#[must_use]
pub fn seed_ok(field: i64, v: f64) -> bool {
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

/// Checks a script the way the shim needs (anything it would crash on).
pub fn validate(face: &FaceSpec, ops: &[Op]) -> Result<(), FfiError> {
    validate_face(face)?;
    if ops.len() > MAX_OPS {
        return bad("op count");
    }
    for o in ops {
        if o.p.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
            return bad("operand");
        }
        let ok = match o.op {
            0 => (0.0..=6.0).contains(&o.p[0]) && (0.0..=26.0).contains(&o.p[1]),
            1 | 2 => (0.0..=6.0).contains(&o.p[0]),
            // the bit mask is cast to ULONG
            3 => o.p[0] >= 0.0,
            4 | 7 | 8 => true,
            5 => {
                (0.0..SEED_FIELDS as f64).contains(&o.p[0])
                    && o.p[0].fract() == 0.0
                    && seed_ok(o.p[0] as i64, o.p[2])
            }
            6 => (1.0..=3.0).contains(&o.p[0]) && (0.0..=26.0).contains(&o.p[1]),
            _ => false,
        };
        if !ok || (o.op == 0 || o.op == 6) && o.p[2] < 0.0 {
            return bad("op");
        }
    }
    reject_degenerate_plane_stage3(face, ops)?;
    simulate(ops)
}

/// The smallest side of a plane domain (`SmPlane`, edge loops) the C++ survives `SetProps_Stage3`
/// on (UB-128). The crash is not monotone in the size (it crashes at 1e-300, 1e-100, 1e-14,
/// 1e-12 and 1e-10, survives at 1e-11 and 1e-9), so the bound is a safe one.
pub const MIN_PLANE_SIDE_FOR_STAGE3: f64 = 1e-6;

/// Rejects a script that can reach `SetProps_Stage3` (op `0` with stage `3a` or `3`, or op `6`
/// with `which == 3`) on a plane with edge loops whose domain has a side below
/// [`MIN_PLANE_SIDE_FOR_STAGE3`]: `SmFace::ClassifyLoops` ends in `SmCrvOnSurf::GetKnots` and a
/// SIGSEGV in `SmCurveClassification::IntersectUVDiscontinuties` (UB-128). A vertex loop, or a
/// script that stops at Stage 2, does not crash.
fn reject_degenerate_plane_stage3(face: &FaceSpec, ops: &[Op]) -> Result<(), FfiError> {
    let (Surf::Plane { domain, .. }, Loops::Edges(_)) = (&face.surf, &face.loops) else {
        return Ok(());
    };
    // `!(x >= bound)` also rejects a NaN side (already refused as non-finite)
    let small = !((domain[2] - domain[0]) >= MIN_PLANE_SIDE_FOR_STAGE3
        && (domain[3] - domain[1]) >= MIN_PLANE_SIDE_FOR_STAGE3);
    let reaches = ops.iter().any(|o| {
        (o.op == 0 && o.p[0] as i64 >= STAGE3_A) || (o.op == 6 && o.p[0] as i64 == 3)
    });
    if small && reaches {
        return bad("degenerate plane domain with edge loops reaches SetProps_Stage3 (UB-128)");
    }
    Ok(())
}

fn push_vec(input: &mut Vec<f64>, v: &[f64]) {
    input.extend_from_slice(v);
}

/// Appends the face block the shim reads (`face_tol`, the surface block, the loops block; see
/// the module docs) to `input`.
pub fn encode_face(face: &FaceSpec, input: &mut Vec<f64>) {
    input.push(face.tol);
    match &face.surf {
        Surf::Plane { o, x, y, domain } => {
            input.push(0.0);
            push_vec(input, o);
            push_vec(input, x);
            push_vec(input, y);
            push_vec(input, domain);
        }
        Surf::Revolution {
            gen,
            point,
            axis,
            swap,
            sweep,
        } => {
            input.push(1.0);
            match gen {
                Gen::Line { a, b } => {
                    input.push(0.0);
                    push_vec(input, a);
                    push_vec(input, b);
                }
                Gen::Circle { c, x, y, d, r } => {
                    input.push(1.0);
                    push_vec(input, c);
                    push_vec(input, x);
                    push_vec(input, y);
                    push_vec(input, d);
                    input.push(*r);
                }
            }
            push_vec(input, point);
            push_vec(input, axis);
            input.push(f64::from(u8::from(*swap)));
            input.push(*sweep);
        }
    }
    match &face.loops {
        Loops::Vertex(p) => {
            input.push(0.0);
            push_vec(input, p);
        }
        Loops::Edges(loops) => {
            input.push(1.0);
            input.push(loops.len() as f64);
            for l in loops {
                input.push(l.len() as f64);
                for p in l {
                    push_vec(input, p);
                }
            }
        }
    }
}

/// Runs the script; returns `[result, snapshot..]` per op.
pub fn run(face: &FaceSpec, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    validate(face, ops)?;
    let mut input = Vec::new();
    encode_face(face, &mut input);
    input.push(ops.len() as f64);
    for o in ops {
        input.push(o.op as f64);
        input.extend(o.p);
    }
    call_with_buffer(ops.len() * (SNAP_LEN + 1) + 1, |buf, len| unsafe {
        sys::sc_heal_face_props_run(
            input.as_ptr(),
            input.len() as i64,
            buf.as_mut_ptr(),
            buf.len() as i64,
            len,
        )
    })
}

/// A loop tree item: `loop_` (`-1` pole vertex) and `partner` (`-2` none, `-1` pole vertex).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeItem {
    /// Loop number or `-1`.
    pub loop_: i64,
    /// Partner loop number, `-1` or `-2`.
    pub partner: i64,
}

/// Builds the tree (`links`: `(kind 0 child / 1 sibling, this, new)`, each `new` linked at most
/// once and no cycles) and runs the `finds` `(start, loop)`; returns `(found, parent)` per find
/// (`-1` none) then the `IsNoArea_Pair` flag per item.
pub fn tree(
    items: &[TreeItem],
    links: &[(i64, i64, i64)],
    finds: &[(i64, i64)],
) -> Result<Vec<f64>, FfiError> {
    tree_validate(items, links, finds)?;
    let mut input = vec![items.len() as f64];
    for it in items {
        input.push(it.loop_ as f64);
        input.push(it.partner as f64);
    }
    input.push(links.len() as f64);
    for &(k, a, c) in links {
        input.extend([k as f64, a as f64, c as f64]);
    }
    input.push(finds.len() as f64);
    for &(s, l) in finds {
        input.extend([s as f64, l as f64]);
    }
    call_with_buffer(2 * finds.len() + items.len() + 1, |buf, len| unsafe {
        sys::sc_heal_face_props_tree(
            input.as_ptr(),
            input.len() as i64,
            buf.as_mut_ptr(),
            buf.len() as i64,
            len,
        )
    })
}

/// The guards of [`tree`].
pub fn tree_validate(
    items: &[TreeItem],
    links: &[(i64, i64, i64)],
    finds: &[(i64, i64)],
) -> Result<(), FfiError> {
    let n = items.len() as i64;
    if items.is_empty() || items.len() > MAX_ITEMS {
        return bad("item count");
    }
    let mut linked = vec![false; items.len()];
    for &(kind, a, c) in links {
        if !(0..=1).contains(&kind) || !(0..n).contains(&a) || !(0..n).contains(&c) || a == c {
            return bad("link");
        }
        let Some(flag) = usize::try_from(c).ok().and_then(|c| linked.get_mut(c)) else {
            return bad("link");
        };
        if *flag {
            return bad("item linked twice");
        }
        *flag = true;
    }
    // no cycles: simulate the pointers AddChild / AddSibling leave; every item is the
    // target of at most one pointer, so there is a cycle exactly when the items reachable
    // from the items nothing points to are not all of them
    let mut child = vec![None::<usize>; items.len()];
    let mut sib = vec![None::<usize>; items.len()];
    for &(kind, a, c) in links {
        let (Ok(a), Ok(c)) = (usize::try_from(a), usize::try_from(c)) else {
            return bad("link");
        };
        let old = if kind == 0 { child.get(a) } else { sib.get(a) }
            .copied()
            .flatten();
        if let Some(slot) = sib.get_mut(c) {
            *slot = old;
        }
        let target = if kind == 0 { child.get_mut(a) } else { sib.get_mut(a) };
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
            return bad("cycle");
        }
    }
    if reached != items.len() {
        return bad("cycle");
    }
    for it in items {
        if it.loop_ < -1 || it.loop_ > 1000 || !(-2..=1000).contains(&it.partner) {
            return bad("item");
        }
    }
    for &(s, l) in finds {
        if !(0..n).contains(&s) || !(-1..=1000).contains(&l) {
            return bad("find");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane() -> FaceSpec {
        FaceSpec {
            tol: 1e-5,
            surf: Surf::Plane {
                o: [0.0; 3],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                domain: [-1.0, -1.0, 1.0, 1.0],
            },
            loops: Loops::Vertex([0.0; 3]),
        }
    }

    fn op(op: i64, p: [f64; 4]) -> Op {
        Op { op, p }
    }

    /// UB-131: `ReSet(2a)` then `SetProps(2)` dereferences the null face upstream.
    #[test]
    fn rejects_reset_then_stage2() {
        let ops = [op(1, [3.0, 0.0, 0.0, 0.0]), op(0, [4.0, 26.0, 0.0, 0.0])];
        assert!(validate(&plane(), &ops).is_err());
    }

    /// UB-131: a seeded stage `GAPS` then Stage2.
    #[test]
    fn rejects_seeded_gaps_then_stage2() {
        let ops = [op(5, [0.0, 0.0, 2.0, 0.0]), op(6, [2.0, 26.0, 0.0, 0.0])];
        assert!(validate(&plane(), &ops).is_err());
    }

    /// UB-128: a plane with a degenerate domain and edge loops segfaults in Stage 3.
    #[test]
    fn rejects_degenerate_plane_with_edge_loops_at_stage3() {
        for t in [1e-300, 1e-100, 1e-14, 1e-12, 1e-10, 1e-9, 1e-7] {
            let mut f = plane();
            f.surf = Surf::Plane {
                o: [0.0; 3],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                domain: [0.0, 0.0, t, t],
            };
            f.loops = Loops::Edges(vec![vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]]);
            for o in [op(0, [6.0, 26.0, 0.0, 0.0]), op(6, [3.0, 26.0, 0.0, 0.0])] {
                assert!(validate(&f, &[o]).is_err(), "t = {t}");
            }
            // Stage 2 and a vertex loop do not crash
            assert!(validate(&f, &[op(0, [4.0, 26.0, 0.0, 0.0])]).is_ok());
            f.loops = Loops::Vertex([0.0; 3]);
            assert!(validate(&f, &[op(0, [6.0, 26.0, 0.0, 0.0])]).is_ok());
        }
    }

    #[test]
    fn accepts_the_face_seed_and_plain_scripts() {
        let ops = [
            op(5, [0.0, 0.0, 2.0, 0.0]),
            op(5, [24.0, 0.0, 1.0, 0.0]),
            op(6, [2.0, 26.0, 0.0, 0.0]),
        ];
        assert!(validate(&plane(), &ops).is_ok());
        assert!(validate(&plane(), &[op(0, [6.0, 26.0, 0.0, 0.0])]).is_ok());
        assert!(validate(&plane(), &[op(0, [4.0, 26.0, 0.0, 0.0])]).is_ok());
    }

    #[test]
    fn rejects_bad_tolerance_and_negative_unsigned_values() {
        let mut f = plane();
        f.tol = 0.0;
        assert!(validate(&f, &[]).is_err());
        f.tol = -1e-5;
        assert!(validate(&f, &[]).is_err());
        for field in [3.0, 9.0, 12.0, 15.0, 19.0] {
            assert!(validate(&plane(), &[op(5, [field, 0.0, -1.0, 0.0])]).is_err());
        }
        assert!(validate(&plane(), &[op(3, [-1.0, 0.0, 0.0, 0.0])]).is_err());
    }
}
