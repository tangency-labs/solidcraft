//! Runner for unit `geom.global_solver`: the base `SmGlobalSolver` traversal
//! (`SolveIt` over hand-built trees with every solver operation), its culling and
//! subdivision hooks, and `AddSortedSolution` / `IdenticalSolutions`.
//!
//! C++: `solidcore_ffi::units::geom_global_solver` (upstream
//! `source/SMLib/src/SmGlobalSolver.cpp`). Rust: `solidcore::geom::global_solver`.
//!
//! A case is a program of ops run on both sides (the op list and encodings are
//! documented in the shim): up to three trees of boxes, one solver, then solves,
//! setter calls, sorted insertions and direct `BranchMayContainAnswers` /
//! `FindBestSubdivisionNode` / `IdenticalSolutions` queries. The base
//! `LocalSolve` records the visited leaf combinations, so a solve's output is
//! the culled, ordered list of node tuples. Solution objects are null, opaque
//! stand-ins or (rarely) an upstream line; the line's parameter interval is
//! unported on the Rust side, so cases that compare line solutions are deferred.
//!
//! Outputs: `ints` (statuses, flags, categories, counts, solution types, object
//! codes, node indices) and `floats` (best answer, tolerance, solution values and
//! parameters). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::global_solver::{
    GlobalSolve, GlobalSolver, OperationCategory, PreferredSolution, SolutionRequested,
    SolveRequest,
    SolverOperation, MAX_SOLVER_TREES,
};
use solidcore::geom::solution_array::{
    ObjectKind, Solution, SolutionEnd, SolutionObject, SolutionType, SortKey, MAX_PARAMS, MAX_TREES,
};
use solidcore::geom::tree::{AuxDataType, NodeId, Tree, TreeNode};
use solidcore::math::Vec3;
use solidcore::SolidError;
use solidcore_ffi::units::geom_global_solver as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `SM_BIG_DOUBLE`.
const BIG: f64 = 1.0e20;

/// `geom.global_solver` runner.
pub struct Runner;

/// An object code: `0..99` an upstream line, `1400..1499` an opaque stand-in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(i64);

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        if self.0 < 100 {
            ObjectKind::Curve
        } else {
            ObjectKind::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Node {
    p: i64,
    c1: i64,
    c2: i64,
    aux: i64,
    bbox: [f64; 6],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Sol {
    t: i64,
    nv: i64,
    no: i64,
    sv: f64,
    sp: [f64; MAX_PARAMS],
    ev: f64,
    ep: [f64; MAX_PARAMS],
    objs: [i64; MAX_TREES],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Op {
    Tree {
        nodes: Vec<Node>,
    },
    Solver {
        kind: i64,
        nvars: i64,
    },
    Set {
        sop: i64,
        req: i64,
        tol: f64,
        best: f64,
        cat: i64,
        at: f64,
        v: [[f64; 3]; 3],
    },
    Solve {
        sop: i64,
        req: i64,
        tol: f64,
        best: f64,
        v: [[f64; 3]; 3],
    },
    AddSorted {
        key: i64,
        combine: bool,
        s: Box<Sol>,
    },
    Dump,
    May {
        b: [i64; 3],
    },
    Best {
        b: [i64; 3],
    },
    Identical {
        a: Box<Sol>,
        b: Box<Sol>,
    },
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    ops: Vec<Op>,
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn enc_sol(v: &mut Vec<f64>, s: &Sol) {
    v.extend([s.t as f64, s.nv as f64, s.no as f64, s.sv]);
    v.extend(s.sp);
    v.push(s.ev);
    v.extend(s.ep);
    v.extend(s.objs.iter().map(|&o| o as f64));
    v.extend([-1.0; MAX_TREES]);
}

fn enc_vecs(v: &mut Vec<f64>, vecs: &[[f64; 3]; 3]) {
    for x in vecs {
        v.extend(x);
    }
}

fn prog(ops: &[Op]) -> Vec<f64> {
    let mut v = Vec::new();
    for op in ops {
        match op {
            Op::Tree { nodes } => {
                v.extend([1.0, nodes.len() as f64]);
                for n in nodes {
                    v.extend([n.p as f64, n.c1 as f64, n.c2 as f64, n.aux as f64]);
                    v.extend(n.bbox);
                }
            }
            Op::Solver { kind, nvars } => v.extend([2.0, *kind as f64, *nvars as f64]),
            Op::Set {
                sop,
                req,
                tol,
                best,
                cat,
                at,
                v: vecs,
            } => {
                v.extend([3.0, *sop as f64, *req as f64, *tol, *best, *cat as f64, *at]);
                enc_vecs(&mut v, vecs);
            }
            Op::Solve {
                sop,
                req,
                tol,
                best,
                v: vecs,
            } => {
                v.extend([4.0, *sop as f64, *req as f64, *tol, *best]);
                enc_vecs(&mut v, vecs);
            }
            Op::AddSorted { key, combine, s } => {
                v.extend([5.0, *key as f64, flag(*combine)]);
                enc_sol(&mut v, s);
            }
            Op::Dump => v.push(6.0),
            Op::May { b } => v.extend([7.0, b[0] as f64, b[1] as f64, b[2] as f64]),
            Op::Best { b } => v.extend([8.0, b[0] as f64, b[1] as f64, b[2] as f64]),
            Op::Identical { a, b } => {
                v.push(9.0);
                enc_sol(&mut v, a);
                enc_sol(&mut v, b);
            }
            Op::Reset => v.push(10.0),
        }
    }
    v
}

// ---- Rust side ------------------------------------------------------------

fn usz(v: i64) -> Result<usize, HarnessError> {
    usize::try_from(v).map_err(|_| harness("negative index"))
}

fn link(v: i64) -> Result<Option<NodeId>, HarnessError> {
    Ok(if v < 0 { None } else { Some(NodeId(usz(v)?)) })
}

fn build_tree(nodes: &[Node]) -> Result<Tree<Obj>, HarnessError> {
    let mut t = Tree::new(None);
    for n in nodes {
        let mut node = TreeNode::new();
        node.parent = link(n.p)?;
        node.child1 = link(n.c1)?;
        node.child2 = link(n.c2)?;
        node.aux_type = AuxDataType::from_code(n.aux as i32).ok_or_else(|| harness("aux type"))?;
        let b = n.bbox;
        node.bbox = Extent3d::from_bounds(b[0], b[1], b[2], b[3], b[4], b[5]);
        t.add_node(node);
    }
    t.set_top_node(Some(NodeId(0)));
    Ok(t)
}

fn to_sol(s: &Sol) -> Result<Solution<Obj>, HarnessError> {
    let t = SolutionType::from_code(s.t as i32).ok_or_else(|| harness("solution type"))?;
    let mut r = Solution::new();
    r.solution_type = t;
    r.num_variables = usz(s.nv)?;
    r.num_objects = usz(s.no)?;
    r.start = SolutionEnd {
        solution_value: s.sv,
        parameters: s.sp,
    };
    r.end = SolutionEnd {
        solution_value: s.ev,
        parameters: s.ep,
    };
    for (slot, &code) in r.objects.iter_mut().zip(&s.objs) {
        *slot = (code >= 0).then_some(Obj(code));
    }
    Ok(r)
}

fn put_end(of: &mut Vec<f64>, e: &SolutionEnd) {
    of.push(e.solution_value);
    of.extend(e.parameters);
}

fn vecs(v: &[[f64; 3]; 3]) -> Vec<Vec3> {
    v.iter().map(|&a| Vec3::from_array(a)).collect()
}

fn operation(code: i64) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(code as i32).ok_or_else(|| harness("solver operation"))
}

fn requested(code: i64) -> Result<SolutionRequested, HarnessError> {
    SolutionRequested::from_code(code as i32).ok_or_else(|| harness("solution requested"))
}

fn branch(b: &[i64; 3]) -> Result<[Option<NodeId>; MAX_SOLVER_TREES], HarnessError> {
    Ok([link(b[0])?, link(b[1])?, link(b[2])?])
}

enum RunError {
    Rust(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for RunError {
    fn from(e: SolidError) -> Self {
        Self::Rust(e)
    }
}

impl From<HarnessError> for RunError {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

/// Upstream status of a solver result: `0`, or the code of an upstream error.
fn status(r: Result<(), SolidError>) -> Result<i64, RunError> {
    match r {
        Ok(()) => Ok(0),
        Err(SolidError::Upstream { code, .. }) => Ok(i64::from(code)),
        Err(e) => Err(RunError::Rust(e)),
    }
}

fn run_rust(ops: &[Op]) -> Result<(Vec<i64>, Vec<f64>), RunError> {
    let mut trees = Vec::new();
    let mut seen_solver = false;
    for op in ops {
        match op {
            Op::Tree { nodes } if !seen_solver => trees.push(build_tree(nodes)?),
            Op::Tree { .. } => return Err(harness("tree after solver").into()),
            Op::Solver { .. } => seen_solver = true,
            _ => {}
        }
    }
    let tree = |i: usize| trees.get(i).ok_or_else(|| harness("missing tree"));
    let (mut oi, mut of) = (Vec::new(), Vec::new());
    let mut solver: Option<GlobalSolver<'_, Obj>> = None;
    for op in ops {
        if let Op::Solver { kind, nvars } = op {
            let nv = usz(*nvars)?;
            let mut s = match kind {
                0 => GlobalSolver::new(),
                1 => GlobalSolver::with_tree(tree(0)?, nv),
                _ => GlobalSolver::with_trees(tree(0)?, tree(1)?, nv),
            };
            if *kind == 3 {
                s.trees[2] = Some(tree(2)?);
                s.set_num_trees(3);
            }
            solver = Some(s);
            continue;
        }
        if let Op::Tree { .. } = op {
            continue;
        }
        let s = solver.as_mut().ok_or_else(|| harness("no solver"))?;
        match op {
            Op::Set {
                sop,
                req,
                tol,
                best,
                cat,
                at,
                v,
            } => {
                s.set_solver_operation(operation(*sop)?);
                s.set_solution_requested(requested(*req)?);
                s.set_3d_tolerance(*tol);
                s.set_best_answer_so_far_sq(*best);
                s.set_operation_category(
                    OperationCategory::from_code(*cat as i32).ok_or_else(|| harness("category"))?,
                );
                s.set_at_distance(*at);
                s.opt_vectors = vecs(v);
            }
            Op::Solve {
                sop,
                req,
                tol,
                best,
                v,
            } => {
                let mut arr = std::mem::take(&mut s.solutions);
                let opt_vectors = vecs(v);
                let request = SolveRequest {
                    operation: operation(*sop)?,
                    solution_requested: requested(*req)?,
                    tolerance_3d: *tol,
                    best_answer_so_far_sq: *best,
                    opt_vectors: &opt_vectors,
                };
                let r = s.solve_it(&request, &mut arr);
                s.solutions = arr;
                oi.push(status(r)?);
            }
            Op::AddSorted { key, combine, s: sol } => {
                let key = SortKey::from_code(*key as i32).ok_or_else(|| harness("sort key"))?;
                s.add_sorted_solution(&to_sol(sol)?, key, *combine)?;
            }
            Op::Dump => {
                oi.extend([
                    i64::from(s.category.code()),
                    i64::from(s.projected_operation),
                    s.solutions.len() as i64,
                ]);
                of.extend([s.best_answer_so_far_sq, s.at_distance_sq, s.tolerance_3d]);
                for sol in s.solutions.as_slice() {
                    oi.extend([
                        i64::from(sol.solution_type.code()),
                        sol.num_variables as i64,
                        sol.num_objects as i64,
                    ]);
                    oi.extend(sol.objects.iter().map(|o| o.map_or(-1, |o| o.0)));
                    oi.extend(sol.nodes.iter().map(|n| n.map_or(-1, |n| n.0 as i64)));
                    put_end(&mut of, &sol.start);
                    put_end(&mut of, &sol.end);
                }
            }
            Op::May { b } => {
                let r = s.branch_may_contain_answers(&branch(b)?)?;
                oi.push(i64::from(r));
            }
            Op::Best { b } => {
                let br = branch(b)?;
                oi.push(i64::from(s.are_ready_for_local_solve(&br)?));
                match s.find_best_subdivision_node(&br) {
                    Ok((i, swap)) => oi.extend([0, i as i64, i64::from(swap)]),
                    Err(e) => oi.push(status(Err(e))?),
                }
            }
            Op::Identical { a, b } => {
                let (pair, pref) = s.identical_solutions(&to_sol(a)?, &to_sol(b)?)?;
                oi.push(i64::from(pair.code()));
                oi.push(match pref {
                    None => 0,
                    Some(PreferredSolution::First) => 1,
                    Some(PreferredSolution::Second) => 2,
                });
            }
            Op::Reset => s.solutions.reset(),
            Op::Tree { .. } | Op::Solver { .. } => {}
        }
    }
    Ok((oi, of))
}

// ---- generation -----------------------------------------------------------

/// Operations whose culling or subdivision reads the second branch slot (the
/// shim's `two_tree_op`).
fn two_tree_op(op: i64) -> bool {
    matches!(op, 3 | 4 | 6 | 8 | 9 | 11..=19 | 23)
}

const RAYFIRE: i64 = 8;
const SIGNED_3D_MIN: i64 = 9;
const ROTATED: i64 = 17;
const PERSPECTIVE: i64 = 18;

/// A random box: centre in `[-6, 6]^3`, half sizes up to `size` (sometimes flat
/// or point sized).
fn rand_box(rng: &mut Rng, size: f64, near: Option<[f64; 3]>) -> [f64; 6] {
    let c = match near {
        Some(n) => {
            let d = rng.point3(-2.0, 2.0);
            [n[0] + d[0], n[1] + d[1], n[2] + d[2]]
        }
        None => rng.point3(-6.0, 6.0),
    };
    let mut h = [0.0; 3];
    for x in &mut h {
        *x = if rng.chance(0.08) { 0.0 } else { rng.uniform(0.02, size) };
    }
    [c[0] - h[0], c[1] - h[1], c[2] - h[2], c[0] + h[0], c[1] + h[1], c[2] + h[2]]
}

/// A tree in breadth-first order (children after their parent): every inner
/// node splits its box along one axis into two shrunk, possibly overlapping
/// halves. `max_nodes == 1` gives a single leaf.
fn gen_tree(rng: &mut Rng, max_nodes: usize, near: Option<[f64; 3]>) -> Vec<Node> {
    let top_size = *rng.choose(&[0.5, 2.0, 4.0]).unwrap_or(&2.0);
    let surface = rng.chance(0.2);
    let aux_of = |rng: &mut Rng| -> i64 {
        if surface {
            *rng.choose(&[3, 3, 3, 6, 4]).unwrap_or(&3)
        } else {
            *rng.choose(&[0, 1, 1, 2, 8]).unwrap_or(&1)
        }
    };
    let mut nodes = vec![Node {
        p: -1,
        c1: -1,
        c2: -1,
        aux: aux_of(rng),
        bbox: rand_box(rng, top_size, near),
    }];
    let depth_limit = rng.range_i64(0, 5);
    let mut depth = vec![0i64];
    let mut k = 0;
    while k < nodes.len() {
        let d = depth.get(k).copied().unwrap_or(0);
        if nodes.len() + 2 <= max_nodes && d < depth_limit && rng.chance(0.85) {
            let b = nodes.get(k).map_or([0.0; 6], |n| n.bbox);
            let axis = rng.below(3) as usize;
            let split = rng.uniform(0.3, 0.7);
            let overlap = if rng.chance(0.3) { rng.uniform(0.0, 0.2) } else { 0.0 };
            let mut lo = b;
            let mut hi = b;
            if let (Some(&a0), Some(&a1)) = (b.get(axis), b.get(axis + 3)) {
                let mid = a0 + split * (a1 - a0);
                if let Some(x) = lo.get_mut(axis + 3) {
                    *x = mid + overlap * (a1 - a0);
                }
                if let Some(x) = hi.get_mut(axis) {
                    *x = mid - overlap * (a1 - a0);
                }
            }
            for half in [&mut lo, &mut hi] {
                // shrink the other axes a little, as curve hulls do
                for j in 0..3 {
                    if j != axis && rng.chance(0.5) {
                        let (f0, f1) = (rng.uniform(0.0, 0.3), rng.uniform(0.0, 0.3));
                        let (Some(&h0), Some(&h1)) = (half.get(j), half.get(j + 3)) else {
                            continue;
                        };
                        let w = h1 - h0;
                        if let Some(x) = half.get_mut(j) {
                            *x += f0 * w;
                        }
                        if let Some(x) = half.get_mut(j + 3) {
                            *x -= f1 * w;
                        }
                    }
                }
            }
            let c1 = nodes.len() as i64;
            let c2 = c1 + 1;
            for (bbox, _) in [(lo, 0), (hi, 1)] {
                nodes.push(Node {
                    p: k as i64,
                    c1: -1,
                    c2: -1,
                    aux: aux_of(rng),
                    bbox,
                });
                depth.push(d + 1);
            }
            if let Some(n) = nodes.get_mut(k) {
                n.c1 = c1;
                n.c2 = c2;
            }
        }
        k += 1;
    }
    nodes
}

fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if l > 0.1 {
            return [p[0] / l, p[1] / l, p[2] / l];
        }
    }
}

fn axis_vec(rng: &mut Rng) -> [f64; 3] {
    match rng.below(4) {
        0 => [1.0, 0.0, 0.0],
        1 => [0.0, 1.0, 0.0],
        2 => [0.0, 0.0, 1.0],
        _ => unit_vec(rng),
    }
}

fn gen_vecs(rng: &mut Rng, sop: i64) -> [[f64; 3]; 3] {
    match sop {
        ROTATED => {
            let axis = axis_vec(rng);
            // a unit x axis perpendicular to the rotation axis
            let t = if axis[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            let x = [
                axis[1] * t[2] - axis[2] * t[1],
                axis[2] * t[0] - axis[0] * t[2],
                axis[0] * t[1] - axis[1] * t[0],
            ];
            let l = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
            [rng.point3(-3.0, 3.0), axis, [x[0] / l, x[1] / l, x[2] / l]]
        }
        PERSPECTIVE => {
            let n = axis_vec(rng);
            let p = rng.point3(-2.0, 2.0);
            let d = rng.uniform(15.0, 40.0);
            [p, n, [p[0] + d * n[0], p[1] + d * n[1], p[2] + d * n[2]]]
        }
        _ => [axis_vec(rng), if rng.chance(0.7) { axis_vec(rng) } else { rng.point3(-4.0, 4.0) }, axis_vec(rng)],
    }
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[0.0, 1e-6, 1e-3, 0.05, 0.3, 1.0]).unwrap_or(&0.0)
}

fn gen_best(rng: &mut Rng, sop: i64) -> f64 {
    match rng.below(6) {
        0 => 0.0,
        1 | 2 => BIG,
        3 if matches!(sop, 9 | 10 | 11 | 21 | 23) => -rng.uniform(0.0, 30.0),
        _ => rng.uniform(0.0, 60.0),
    }
}

/// A value from a small palette, often nudged by a tolerance-scale amount.
fn value(rng: &mut Rng) -> f64 {
    let base = *rng.choose(&[0.0, 1.0, 2.0, -1.0, 0.5, 0.25, 1e-3, 3.0, -2.5]).unwrap_or(&0.0);
    let base = if rng.chance(0.15) { rng.uniform(-10.0, 10.0) } else { base };
    let nudge = *rng
        .choose(&[0.0, 0.0, 0.0, 1e-14, 5e-13, 2e-12, 1e-9, 5e-7, 2e-6, 1e-4, 1e-2])
        .unwrap_or(&0.0);
    if rng.chance(0.5) {
        base + nudge
    } else {
        base - nudge
    }
}

/// A solution value: small positive deviations mostly (signed when asked).
fn sol_value(rng: &mut Rng, signed: bool) -> f64 {
    let v = match rng.below(8) {
        0 => 0.0,
        1 => 1e-12,
        2 => 1e-9,
        3 => rng.uniform(0.0, 0.01),
        4 => 1e20,
        _ => rng.uniform(0.0, 5.0),
    };
    if signed && rng.chance(0.4) {
        -v
    } else {
        v
    }
}

/// Objects a sorted-insertion program draws from.
struct Pool {
    codes: Vec<i64>,
}

impl Pool {
    fn new(rng: &mut Rng) -> Self {
        let mut codes = vec![1400, 1401];
        if rng.chance(0.1) {
            codes.extend([0, 1]);
        }
        Self { codes }
    }

    fn pick(&self, rng: &mut Rng) -> i64 {
        if rng.chance(0.3) {
            -1
        } else {
            *rng.choose(&self.codes).unwrap_or(&-1)
        }
    }
}

fn gen_sol(rng: &mut Rng, pool: &Pool, signed: bool, rayfire: bool) -> Sol {
    let t = match rng.below(10) {
        0..=4 => 1,
        5..=8 => 2,
        _ => *rng.choose(&[0, 3, 4]).unwrap_or(&1),
    };
    let nv = if rng.chance(0.08) { rng.range_i64(0, 10) } else { rng.range_i64(0, 3) };
    let no = rng.range_i64(0, 2);
    let mut sp = [0.0; MAX_PARAMS];
    let mut ep = [0.0; MAX_PARAMS];
    for (a, b) in sp.iter_mut().zip(ep.iter_mut()).take(nv as usize) {
        *a = value(rng);
        *b = if rng.chance(0.2) { *a } else { *a + rng.uniform(0.0, 2.0) };
    }
    let sv = sol_value(rng, signed);
    let ev = if rng.chance(0.5) { sv } else { sol_value(rng, signed) };
    let mut objs = [-1; MAX_TREES];
    let shared = pool.pick(rng);
    for (k, o) in objs.iter_mut().enumerate() {
        if (k as i64) < no {
            *o = if rng.chance(0.7) { shared } else { pool.pick(rng) };
        }
    }
    let mut s = Sol {
        t,
        nv,
        no,
        sv,
        sp,
        ev,
        ep,
        objs,
    };
    make_safe(&mut s, rayfire);
    s
}

/// The shim's safety rules: a first curve object needs non-null objects; the
/// RAYFIRE trim needs a null first object.
fn make_safe(s: &mut Sol, rayfire: bool) {
    if rayfire {
        s.objs[0] = -1;
    }
    if s.no > 0 && (0..100).contains(&s.objs[0]) {
        for (k, o) in s.objs.iter_mut().enumerate().take(s.no as usize) {
            if *o < 0 {
                *o = 1400 + k as i64;
            }
        }
    }
}

/// A solution close to `s`: same counts and objects, some values nudged.
fn near(rng: &mut Rng, s: &Sol, rayfire: bool) -> Sol {
    let mut o = s.clone();
    let nudge = |rng: &mut Rng, v: f64| -> f64 {
        if rng.chance(0.5) {
            v
        } else {
            v + *rng
                .choose(&[1e-14, -1e-13, 1e-12, 1e-9, -1e-7, 5e-7, 2e-5, -3e-5, 1e-4, 0.5, -1.0])
                .unwrap_or(&0.0)
        }
    };
    o.sv = nudge(rng, o.sv);
    o.ev = nudge(rng, o.ev);
    for (a, b) in o.sp.iter_mut().zip(o.ep.iter_mut()) {
        *a = nudge(rng, *a);
        *b = nudge(rng, *b);
    }
    if rng.chance(0.25) {
        o.t = *rng.choose(&[1, 2]).unwrap_or(&1);
    }
    // a point inside / at the ends of a range
    if rng.chance(0.25) && s.t == 2 {
        o.t = 1;
        let f = *rng.choose(&[0.0, 0.5, 1.0, 0.25]).unwrap_or(&0.5);
        for (a, (&lo, &hi)) in o.sp.iter_mut().zip(s.sp.iter().zip(&s.ep)) {
            *a = lo + f * (hi - lo);
        }
        o.sv = if rng.chance(0.5) { s.sv * 1e-3 } else { s.sv };
    }
    make_safe(&mut o, rayfire);
    o
}

/// A program of sorted insertions (and pairwise identity queries) under one
/// set of solver settings.
fn gen_sorted(rng: &mut Rng) -> Inputs {
    let pool = Pool::new(rng);
    let sop = if rng.chance(0.4) {
        *rng.choose(&[0, 3, 4, 8, 12, 13, 14, 15, 9, 11, 23]).unwrap_or(&0)
    } else {
        rng.range_i64(0, 24)
    };
    let cat = if rng.chance(0.7) {
        match sop {
            3 | 8 | 12 | 14 | 20 => 0,
            4 | 13 | 15 => 1,
            9 | 10 | 11 | 21 | 23 => 2,
            _ => 3,
        }
    } else {
        rng.range_i64(0, 3)
    };
    let signed = cat == 2;
    let rayfire = sop == RAYFIRE;
    let mut ops = vec![
        Op::Solver {
            kind: 0,
            nvars: rng.range_i64(0, 3),
        },
        Op::Set {
            sop,
            req: *rng.choose(&[1, 1, 0, 3]).unwrap_or(&1),
            tol: gen_tol(rng),
            best: gen_best(rng, sop),
            cat,
            at: rng.uniform(0.0, 3.0),
            v: gen_vecs(rng, sop),
        },
    ];
    let key = rng.range_i64(0, 4);
    let combine = rng.chance(0.7);
    let mut prev: Vec<Sol> = Vec::new();
    for _ in 0..rng.range_i64(2, 30) {
        let s = match prev.last() {
            Some(_) if rng.chance(0.45) => {
                let base = rng.choose(&prev).cloned().unwrap_or_else(|| gen_sol(rng, &pool, signed, rayfire));
                near(rng, &base, rayfire)
            }
            _ => gen_sol(rng, &pool, signed, rayfire),
        };
        prev.push(s.clone());
        if rng.chance(0.1) {
            let b = if prev.len() > 1 && rng.chance(0.5) {
                rng.choose(&prev).cloned().unwrap_or_else(|| s.clone())
            } else {
                near(rng, &s, rayfire)
            };
            ops.push(Op::Identical {
                a: Box::new(s.clone()),
                b: Box::new(b),
            });
        }
        ops.push(Op::AddSorted {
            key: if rng.chance(0.85) { key } else { rng.range_i64(0, 4) },
            combine: if rng.chance(0.9) { combine } else { !combine },
            s: Box::new(s),
        });
        if rng.chance(0.1) {
            ops.push(Op::Dump);
        }
        if rng.chance(0.03) {
            ops.push(Op::Reset);
        }
    }
    ops.push(Op::Dump);
    Inputs { ops }
}

/// A solve over generated trees, plus direct culling / subdivision queries.
fn gen_solve(rng: &mut Rng) -> Inputs {
    let sop = rng.range_i64(0, 24);
    let two = two_tree_op(sop);
    // solver tree count
    let ntrees: i64 = if two {
        if matches!(sop, RAYFIRE | SIGNED_3D_MIN) || rng.chance(0.9) {
            2
        } else {
            1
        }
    } else {
        *rng.choose(&[0, 1, 2, 2, 2, 3]).unwrap_or(&2)
    };
    let max_nodes = *rng.choose(&[7, 15, 31, 63]).unwrap_or(&15);
    let mut ops = Vec::new();
    let mut sizes = Vec::new();
    let tree_count = if ntrees == 0 { rng.range_i64(0, 1) } else { ntrees };
    let mut first_center: Option<[f64; 3]> = None;
    let mut centers = Vec::new();
    for i in 0..tree_count {
        let near = if rng.chance(0.75) { first_center } else { None };
        let nodes = if sop == RAYFIRE && i == 0 {
            gen_tree(rng, 1, None)
        } else {
            gen_tree(rng, max_nodes, near)
        };
        let b = nodes.first().map_or([0.0; 6], |n| n.bbox);
        let c = [0.5 * (b[0] + b[3]), 0.5 * (b[1] + b[4]), 0.5 * (b[2] + b[5])];
        if i == 0 {
            first_center = Some(if sop == RAYFIRE { [b[0], b[1], b[2]] } else { c });
        }
        centers.push(c);
        sizes.push(nodes.len() as i64);
        ops.push(Op::Tree { nodes });
    }
    let kind = if ntrees == 3 { 3 } else { ntrees };
    ops.push(Op::Solver {
        kind,
        nvars: rng.range_i64(0, 4),
    });
    let mut v = gen_vecs(rng, sop);
    if let (Some(o), Some(t)) = (first_center, centers.get(1)) {
        if matches!(sop, RAYFIRE | SIGNED_3D_MIN) && rng.chance(0.8) {
            // aim the ray / direction at the second tree
            let n = rng.point3(-0.3, 0.3);
            let d = [t[0] - o[0] + n[0], t[1] - o[1] + n[1], t[2] - o[2] + n[2]];
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if l > 1e-3 {
                v[0] = [d[0] / l, d[1] / l, d[2] / l];
            }
        }
    }
    let at = rng.uniform(0.0, 12.0);
    if sop == 6 || rng.chance(0.2) {
        // SetAtDistance (and the other setters) before the solve
        ops.push(Op::Set {
            sop,
            req: 1,
            tol: 0.0,
            best: 0.0,
            cat: 3,
            at,
            v,
        });
    }
    let tol = gen_tol(rng);
    let best = gen_best(rng, sop);
    let req = *rng.choose(&[1, 1, 0, 3]).unwrap_or(&1);
    ops.push(Op::Solve { sop, req, tol, best, v });
    ops.push(Op::Dump);

    // direct hook queries on random branches
    let pick = |rng: &mut Rng, i: usize| -> i64 {
        sizes.get(i).map_or(-1, |&n| rng.range_i64(0, n - 1))
    };
    for _ in 0..rng.range_i64(0, 4) {
        let qop = if rng.chance(0.8) { sop } else { rng.range_i64(0, 24) };
        let mut b = [-1i64; 3];
        for (i, slot) in b.iter_mut().enumerate().take(ntrees as usize) {
            *slot = pick(rng, i);
        }
        let ok_two = !two_tree_op(qop) || ntrees == 1 || ntrees == 2;
        if !ok_two || (ntrees == 0 && two_tree_op(qop)) {
            continue;
        }
        ops.push(Op::Set {
            sop: qop,
            req: 1,
            tol: gen_tol(rng),
            best: gen_best(rng, qop),
            cat: rng.range_i64(0, 3),
            at: rng.uniform(0.0, 12.0),
            v: gen_vecs(rng, qop),
        });
        ops.push(Op::May { b });
        let best_ok = if matches!(qop, RAYFIRE | SIGNED_3D_MIN) {
            ntrees == 2 && (qop != RAYFIRE || rayfire_best_safe(&ops, b))
        } else {
            true
        };
        if best_ok {
            ops.push(Op::Best { b });
        }
    }

    // sometimes add sorted solutions on top of the solve's node solutions
    if rng.chance(0.15) {
        let pool = Pool { codes: vec![1400] };
        for _ in 0..rng.range_i64(1, 5) {
            let s = gen_sol(rng, &pool, false, true);
            ops.push(Op::AddSorted {
                key: rng.range_i64(0, 4),
                combine: rng.chance(0.5),
                s: Box::new(s),
            });
        }
        ops.push(Op::Dump);
    }
    Inputs { ops }
}

/// The shim's RAYFIRE `FindBestSubdivisionNode` rule: the second slot's node
/// has children, or no slot's node does.
fn rayfire_best_safe(ops: &[Op], b: [i64; 3]) -> bool {
    let trees: Vec<&Vec<Node>> = ops
        .iter()
        .filter_map(|o| if let Op::Tree { nodes } = o { Some(nodes) } else { None })
        .collect();
    let has_children = |i: usize| -> bool {
        let idx = b.get(i).copied().unwrap_or(-1);
        trees
            .get(i)
            .and_then(|t| t.get(usize::try_from(idx).ok()?))
            .is_some_and(|n| n.c1 >= 0)
    };
    has_children(1) || (!has_children(0) && !has_children(1))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.global_solver"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (oi, of) = ffi::run(&prog(&i.ops)).map_err(|e| harness(e.to_string()))?;
        Ok(Output::new().ints("ints", oi).values("floats", of))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match run_rust(&i.ops) {
            Ok((oi, of)) => Ok(Output::new().ints("ints", oi).values("floats", of)),
            Err(RunError::Rust(e)) => {
                Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust_error: {e}"))))
            }
            Err(RunError::Harness(e)) => Err(e),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = if rng.chance(0.55) { gen_solve(rng) } else { gen_sorted(rng) };
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}

