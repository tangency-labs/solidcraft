//! FFI wrappers for unit `intersect.adv_surface_intersector`.
//!
//! Shim: `harness/cpp/shim/intersect_adv_surface_intersector.cpp`
//! (`sc_intersect_adv_surface_intersector_run`). Upstream:
//! `source/SMLib/src/SmAdvSurfaceIntersector.cpp`,
//! `source/SMLib/inc/SmAdvSurfaceIntersector.h`.
//!
//! [`run`] builds surfaces A and B (B may be A itself), optionally a hand-built
//! decomposition tree in a surface cache of each (one shared tree when B is A), the
//! intersector `SmAdvSurfaceIntersector(A, dom1, B, dom2, fillet)` with `m_apTrees` set to
//! those trees, and a point arena; then runs the [`Op`]s in order. The record is, per op,
//! `op status [values]`, then the state.
//!
//! The state is `closest uvs[0] uvs[1] node[0] node[1] separate trace_dir ang_tol
//! approx_tol tol3d operation ncurvepoints` (nodes as indices into tree 0 / tree 1, `-1`
//! null). A point is the 80 values of
//! [`super::intersect_surface_intersector_p1`]. A solution list is `n` then per solution
//! `type nvars start_value start[10] end_value end[10] node0 node1`. A conic is `type a b
//! origin(x y) theta`.
//!
//! | [`Op`]        | upstream                                              | values |
//! |---------------|-------------------------------------------------------|--------|
//! | `State`       | the members                                           | state |
//! | `Set`         | `m_dCurveTraceDirection`, `m_dThisAngTolRad`, `m_dThisApproxTol3d`, `m_d3dTolerance`, `m_eSolverOperation` | |
//! | `Push`        | `AddPointToCurve(point)`                              | |
//! | `Sing`        | `ComputeSingularityPoint(uvs, point, prev or NULL)`   | point |
//! | `Dupin`       | `MakeDupin(eval, normal, ..)`                         | eval matrix; on success `k1vec k2vec k1 k2 efg lmn nabove conics nbelow conics` |
//! | `Solve`       | `SolveIt(op, req, tol, best, NULL, array)`            | solutions |
//! | `BestSub`     | `FindBestSubdivisionNode(branch, ..)`                 | `index swap` |
//! | `Gauss`       | `BranchIsGaussSeparate(branch, ..)`                   | flag |
//! | `Local`       | `LocalSolve(branch, ..)` into the program's array     | `more` solutions state |
//! | `Interior`    | `FindInteriorCurves(..)`                              | `ncurves` state |
//! | `SolveBranch` | `SolveBranch(branch)` into the program's array        | solutions state |
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`] (the `Solve`
//! best answer may be up to `1e20`), surfaces as in `intersect.surface_intersector_p1`,
//! trees of at most [`MAX_NODES`] nodes (node 0 the top, children both absent or two
//! distinct later nodes pointing back, leaves Bezier patches, patches and heads with a
//! polar box of non-zero vectors and patches with a pseudo box basis of determinant at
//! least `1e-6`, well clear of `SetBasis`' `1e-12` rejection),
//! at most [`MAX_POINTS`] points and [`MAX_OPS`] ops; tree ops need trees. Ops upstream
//! would crash on are not run (status [`REJECTED`]): `Push` of a linked point, `Local`
//! on a node that is not a patch, and `Sing` that would find more than 8 singular
//! directions.

use super::intersect_surface_intersector_p1::{encode_surface, SurfaceIn, POINT_LEN};
use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_adv_surface_intersector_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Most points per program.
pub const MAX_POINTS: usize = 64;
/// Most ops per program.
pub const MAX_OPS: usize = 64;
/// Most nodes per tree.
pub const MAX_NODES: usize = 64;
/// Most polar box vectors per node.
pub const MAX_POLAR: usize = 8;
/// The op status of an op the shim does not run.
pub const REJECTED: f64 = -2.0;

/// A pseudo box: basis vectors and `[min, max]` intervals.
pub type PseudoBoxIn = ([[f64; 3]; 3], [[f64; 2]; 3]);

/// A tree node.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeIn {
    /// Parent index, `-1` for the top.
    pub parent: i64,
    /// First child index, `-1` for none.
    pub c1: i64,
    /// Second child index, `-1` for none.
    pub c2: i64,
    /// `SmAuxDataType`: 3 (patch), 4 (head) or 6 (aux data).
    pub aux: u8,
    /// `m_sBBox` as `min max`.
    pub bbox: [f64; 6],
    /// `m_sUVDomain`.
    pub dom: [f64; 4],
    /// Vectors added to the polar box (`AddVector3d`), in order.
    pub polar: Vec<[f64; 3]>,
    /// The patch's pseudo box: basis vectors and `[min, max]` intervals (patches only).
    pub pseudo: Option<PseudoBoxIn>,
}

/// An op (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// The state.
    State,
    /// Sets the tracing direction, tolerances and solver operation (0, 2 or 3).
    Set {
        /// `m_dCurveTraceDirection`.
        dir: f64,
        /// `m_dThisAngTolRad`.
        ang: f64,
        /// `m_dThisApproxTol3d`.
        approx: f64,
        /// `m_d3dTolerance`.
        tol: f64,
        /// `m_eSolverOperation`.
        op: u8,
    },
    /// `AddPointToCurve(point i)`.
    Push(usize),
    /// `ComputeSingularityPoint` of point `i` with optional previous point.
    Sing {
        /// The point.
        i: usize,
        /// The previous point.
        prev: Option<usize>,
    },
    /// `MakeDupin`.
    Dupin {
        /// The evaluation matrix `[i][j]`.
        eval: Box<[[[f64; 3]; 3]; 3]>,
        /// The reference normal.
        normal: [f64; 3],
    },
    /// `SolveIt`.
    Solve {
        /// Operation (0, 2 or 3).
        op: u8,
        /// Solutions requested (0..=3).
        req: u8,
        /// 3D tolerance.
        tol: f64,
        /// Best answer so far (squared).
        best: f64,
    },
    /// `FindBestSubdivisionNode` of (node of tree 0, node of tree 1).
    BestSub(usize, usize),
    /// `BranchIsGaussSeparate`.
    Gauss(usize, usize),
    /// `LocalSolve`.
    Local(usize, usize),
    /// `FindInteriorCurves`.
    Interior,
    /// `SolveBranch`.
    SolveBranch(usize, usize),
}

/// A program (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// Surface A.
    pub a: SurfaceIn,
    /// Surface B, `None` for A itself.
    pub b: Option<SurfaceIn>,
    /// Domain of A.
    pub dom1: [f64; 4],
    /// Domain of B.
    pub dom2: [f64; 4],
    /// `bFromFilletIntersector`.
    pub fillet: bool,
    /// Trees of A and B (`None` for B when B is A).
    pub trees: Option<(Vec<NodeIn>, Option<Vec<NodeIn>>)>,
    /// Points ([`POINT_LEN`] values each).
    pub points: Vec<Vec<f64>>,
    /// Ops.
    pub ops: Vec<Op>,
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("value not finite or too large"))
    }
}

/// Checks the shape rules of the module docs.
fn check_tree(t: &[NodeIn]) -> Result<(), FfiError> {
    let n = t.len();
    if n == 0 || n > MAX_NODES {
        return Err(bad("tree size"));
    }
    let idx = |i: i64| usize::try_from(i).ok().filter(|&k| k < n);
    for (k, nd) in t.iter().enumerate() {
        if !matches!(nd.aux, 3 | 4 | 6) {
            return Err(bad("aux type"));
        }
        match (k, idx(nd.parent)) {
            (0, None) if nd.parent == -1 => {}
            (0, _) => return Err(bad("top node parent")),
            (_, Some(p)) if p < k => {
                let pn = t.get(p).ok_or_else(|| bad("parent"))?;
                if pn.c1 != k as i64 && pn.c2 != k as i64 {
                    return Err(bad("parent does not point back"));
                }
            }
            _ => return Err(bad("parent")),
        }
        match (nd.c1, nd.c2) {
            (-1, -1) => {
                if nd.aux != 3 {
                    return Err(bad("leaf is not a patch"));
                }
            }
            (a, b) => {
                let (Some(a), Some(b)) = (idx(a), idx(b)) else {
                    return Err(bad("child index"));
                };
                if a <= k || b <= k || a == b {
                    return Err(bad("child order"));
                }
                let back = |c: usize| t.get(c).is_some_and(|cn| cn.parent == k as i64);
                if !back(a) || !back(b) {
                    return Err(bad("child does not point back"));
                }
            }
        }
        if nd.polar.len() > MAX_POLAR || (nd.polar.is_empty() && nd.aux != 6) {
            return Err(bad("polar box"));
        }
        if nd.polar.iter().any(|v| v.iter().all(|&x| x == 0.0)) {
            return Err(bad("zero polar box vector"));
        }
        match (&nd.pseudo, nd.aux) {
            (Some(([a, b, c], _)), 3) => {
                // well clear of SetBasis' linear independence test (|det| < 1e-12)
                let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]);
                if det.is_nan() || det.abs() < 1e-6 {
                    return Err(bad("pseudo box basis not independent"));
                }
            }
            (None, 3) => return Err(bad("patch without pseudo box")),
            (Some(_), _) => return Err(bad("pseudo box on a non-patch")),
            (None, _) => {}
        }
    }
    Ok(())
}

struct Enc {
    v: Vec<f64>,
    npoints: usize,
    sizes: Option<(usize, usize)>,
}

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.v.push(finite(x)?);
        Ok(())
    }
    fn nums(&mut self, xs: &[f64]) -> Result<(), FfiError> {
        xs.iter().try_for_each(|&x| self.num(x))
    }
    fn int(&mut self, x: usize) {
        self.v.push(x as f64);
    }
    fn sint(&mut self, x: i64) {
        self.v.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.v.push(if b { 1.0 } else { 0.0 });
    }
    fn pt(&mut self, i: usize) -> Result<(), FfiError> {
        if i >= self.npoints {
            return Err(bad("point index"));
        }
        self.int(i);
        Ok(())
    }
    fn branch(&mut self, a: usize, b: usize) -> Result<(), FfiError> {
        let Some((n0, n1)) = self.sizes else {
            return Err(bad("tree op without trees"));
        };
        if a >= n0 || b >= n1 {
            return Err(bad("node index"));
        }
        self.int(a);
        self.int(b);
        Ok(())
    }
    fn sop(&mut self, op: u8) -> Result<(), FfiError> {
        if !matches!(op, 0 | 2 | 3) {
            return Err(bad("solver operation"));
        }
        self.int(usize::from(op));
        Ok(())
    }

    fn tree(&mut self, t: &[NodeIn]) -> Result<(), FfiError> {
        check_tree(t)?;
        self.int(t.len());
        for nd in t {
            self.sint(nd.parent);
            self.sint(nd.c1);
            self.sint(nd.c2);
            self.int(usize::from(nd.aux));
            self.nums(&nd.bbox)?;
            self.nums(&nd.dom)?;
            self.int(nd.polar.len());
            for v in &nd.polar {
                self.nums(v)?;
            }
            if let Some((basis, ivls)) = &nd.pseudo {
                for v in basis {
                    self.nums(v)?;
                }
                for i in ivls {
                    self.nums(i)?;
                }
            }
        }
        Ok(())
    }

    fn point(&mut self, p: &[f64]) -> Result<(), FfiError> {
        if p.len() != POINT_LEN {
            return Err(bad("point length"));
        }
        let code = |x: f64, hi: f64| x >= 0.0 && x <= hi && x.fract() == 0.0;
        let ok = p
            .iter()
            .take(5)
            .enumerate()
            .all(|(k, &t)| code(t, if k == 0 { 7.0 } else { 5.0 }));
        if !ok {
            return Err(bad("point type codes"));
        }
        self.nums(p)
    }

    fn op(&mut self, op: &Op) -> Result<(), FfiError> {
        match op {
            Op::State => self.int(1),
            Op::Set {
                dir,
                ang,
                approx,
                tol,
                op,
            } => {
                self.int(2);
                self.nums(&[*dir, *ang, *approx, *tol])?;
                self.sop(*op)?;
            }
            Op::Push(i) => {
                self.int(3);
                self.pt(*i)?;
            }
            Op::Sing { i, prev } => {
                self.int(4);
                self.pt(*i)?;
                self.flag(prev.is_some());
                self.pt(prev.unwrap_or(*i))?;
            }
            Op::Dupin { eval, normal } => {
                self.int(5);
                for row in eval.iter() {
                    for v in row {
                        self.nums(v)?;
                    }
                }
                self.nums(normal)?;
            }
            Op::Solve { op, req, tol, best } => {
                if self.sizes.is_none() {
                    return Err(bad("tree op without trees"));
                }
                if *req > 3 || !best.is_finite() || best.abs() > 1e20 {
                    return Err(bad("solve request"));
                }
                self.int(6);
                self.sop(*op)?;
                self.int(usize::from(*req));
                self.num(*tol)?;
                self.v.push(*best);
            }
            Op::BestSub(a, b) => {
                self.int(7);
                self.branch(*a, *b)?;
            }
            Op::Gauss(a, b) => {
                self.int(8);
                self.branch(*a, *b)?;
            }
            Op::Local(a, b) => {
                self.int(9);
                self.branch(*a, *b)?;
            }
            Op::Interior => {
                if self.sizes.is_none() {
                    return Err(bad("tree op without trees"));
                }
                self.int(10);
            }
            Op::SolveBranch(a, b) => {
                self.int(11);
                self.branch(*a, *b)?;
            }
        }
        Ok(())
    }
}

/// Encodes `prog` for the shim.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected program.
pub fn encode(prog: &Program) -> Result<Vec<f64>, FfiError> {
    if prog.ops.len() > MAX_OPS || prog.points.len() > MAX_POINTS {
        return Err(bad("too many ops or points"));
    }
    let mut v = Vec::new();
    encode_surface(&mut v, &prog.a)?;
    match &prog.b {
        None => v.push(2.0),
        Some(b) => encode_surface(&mut v, b)?,
    }
    let mut e = Enc {
        v,
        npoints: prog.points.len(),
        sizes: None,
    };
    e.nums(&prog.dom1)?;
    e.nums(&prog.dom2)?;
    e.flag(prog.fillet);
    match &prog.trees {
        None => e.flag(false),
        Some((t0, t1)) => {
            e.flag(true);
            e.tree(t0)?;
            match (t1, &prog.b) {
                (None, None) => e.sizes = Some((t0.len(), t0.len())),
                (Some(t1), Some(_)) => {
                    e.tree(t1)?;
                    e.sizes = Some((t0.len(), t1.len()));
                }
                _ => return Err(bad("tree B must be given exactly when B is not A")),
            }
        }
    }
    e.int(prog.points.len());
    for p in &prog.points {
        e.point(p)?;
    }
    e.int(prog.ops.len());
    for op in &prog.ops {
        e.op(op)?;
    }
    Ok(e.v)
}

/// Runs `prog` (see the module docs) and returns the record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn run(prog: &Program) -> Result<Vec<f64>, FfiError> {
    let input = encode(prog)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(4096, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_intersect_adv_surface_intersector_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
