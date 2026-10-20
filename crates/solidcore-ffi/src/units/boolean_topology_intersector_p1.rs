//! FFI wrappers for unit `boolean.topology_intersector_p1`.
//!
//! Shim: `harness/cpp/shim/boolean_topology_intersector_p1.cpp`
//! (`sc_boolean_topology_intersector_p1_eval`). Upstream:
//! `source/SMLib/src/SmTopologyIntersector.cpp`.
//!
//! Every call starts with two B-rep blocks (as in [`super::topology_vertexuse`]; the
//! second is preceded by a `has_b` flag and may be absent): the primary B-rep and the
//! other B-rep of an `SmTopologyIntersector`. `paths` holds the `.smb` file of each
//! file block (ignored otherwise). The output starts with `0`, or with `-1 status`
//! (`-3 status`) when the first (second) B-rep could not be built, and nothing else
//! follows.
//!
//! | [`Op`]   | after the blocks | outputs |
//! |----------|------------------|---------|
//! | `Export` |                  | per B-rep `len export` (`len 0` for an absent one), each export a `0`-prefixed graph as [`super::topology_vertexuse`] exports it |
//! | `Run`    | `ctor approx ang n_ops ops*` | per op its code then its outputs (below) |
//!
//! `ctor` 0 makes the intersector with both B-reps and the tolerances, 1 with the
//! context only and then sets them (`SetPrimaryBrep`, `SetOtherBrep`,
//! `SetThisApproxTol3d`, `SetThisAngTolRad`).
//!
//! An object handle is a triple `brep kind index`: `kind` 0 vertex, 1 edge, 2 face
//! (of B-rep `brep`, `index` in its export's traversal order), 3 an edge's curve, 4 a
//! face's surface, 5 a curve of the shim's store (the curves registered by
//! `RegisterExistingSSI`, in registration order: the 3D curve, then the UV curves
//! given), 6 a B-rep (`index` 0 or 1), 8 null. Outputs use the same triples (`-1 8 -1`
//! null, `-1 -3 -1` an object outside both B-reps). `-2` in an output slot is "upstream
//! would dereference null" (the shim did not call it).
//!
//! Script ops (`op args -> outputs`; `h` a handle, `pc` a point classification
//! `class h`, `pcf` a classification with `pre_snap deviation t_param params_set`):
//!
//! | op | name | args | outputs |
//! |----|------|------|---------|
//! | 1  | `Relate`                   | `h h`                    | `status tol tol` (the tolerances of the two objects after the call; `-1` for a null or non-topology object) |
//! | 2  | `GetOtherMate`             | `h`                      | `h` |
//! | 3  | `GetThisMate`              | `h`                      | `h` |
//! | 4  | `GetBrepMate`              | `h`                      | `h` |
//! | 5  | `IsMatedPair`              | `h h`                    | `flag` |
//! | 6  | `GetMappedBrepObjects`     | `type`                   | `n h*` sorted by handle (upstream: hash order) |
//! | 7  | `GetCommonEdges`           |                          | `n h* n h*` |
//! | 8  | `GetCommonVertices`        |                          | `n h* n h*` |
//! | 9  | `SetSubset` + `GetSubset`  | `n h*`                   | `status` then `-1` (no subset) or `n h*` sorted |
//! | 11 | `FixupEndPointClassMates`  | `dim tol pc pc`          | `status pc pc` |
//! | 12 | `FixupMidPointClassMates`  | `dim tol pc pc`          | `status pc pc` |
//! | 13 | getters                    |                          | `h(primary) h(other) approx ang` |
//! | 14 | `SetThisApproxTol3d`, `SetThisAngTolRad` | `approx ang` | `approx ang` |
//! | 15 | `SetPrimaryBrep`, `SetOtherBrep` | `h h`              | `h h` |
//! | 16 | `RegisterExistingSSI`      | `h h n (curve has_uv1 [curve] has_uv2 [curve] type dev)*` | `status` |
//! | 17 | `CheckIfHaveExistingSSI`   | `h h`                    | `status have n (curve curve curve type dev)*` (`put_curve` blocks) |
//! | 18 | `ClearExistingSSI`         |                          | `status` |
//! | 19 | `sm_CanImprint`            | `type`                   | `flag` |
//! | 20 | `sm_FindFirstLowestDimensionSol` | `n h*`             | `index` |
//! | 21 | `sm_SetEdgeClass`          | `h t0 t1 tol`            | `status size pcf pcf pcf` |
//! | 22 | `sm_CheckInteriorEdge`     | `h t tol`                | `status interior` |
//! | 23 | `sm_TestAndSplitTwoEdges`  | `h h tol`                | `status dist min_unstitched n_verts n_edges1 n_edges2` |
//! | 24 | `sm_TestAndImprintFace`    | `h x y z tol`            | `status reason dist has_vertex` |
//! | 25 | `VertexFaceIntersect`      | `n h* n h* manifold ignore` | `status n h* n h* max min n flag*` |
//! | 26 | `EdgeFaceIntersect`        | `n h* n h* manifold ignore` | `status n_verts n h*(split) n_new n h*(faces) max min` |
//! | 27 | `FaceFaceIntersect`        | `n h* n h* n h* ignore nesting` | `status n h* n_faces n_edges max min` |
//! | 28 | `FaceFaceCoincidence`      | `n h* n h* ignore nesting` | `status n h* n h* max min` |
//! | 29 | `ResolveIntersections`     |                          | `status n_pv n_pe` |
//! | 30 | `ImprintIntersections`     |                          | `status n_pv n_pe` |
//! | 31 | `sm_IntersectShellVertices` | `first swap`            | `status n_pv n_pe` |
//! | 32 | `sm_IntersectWireLaminaEdges` | `first lamina swap`   | `status n_pv n_pe` |
//! | 33 | new `SmProtoTopologyManager` | `capture override`     | `status tol` (`GetTolerance`) |
//!
//! Ops 21, 23 to 28 emit their outputs only on a success status (the port cannot
//! report the partly filled outputs of a failed call).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: existing
//! `.smb` files, well-formed clamped B-splines, handles inside the B-reps' exports (it
//! builds the B-reps once more to count), the registered curves that exist, edges with a
//! curve and vertices and faces with a surface where upstream dereferences them, no
//! handle-taking op after an op that may delete topology (23 to 28, 30), ops 7 and 8
//! only while the intersector's primary B-rep is set and 27 to 30 only while both are
//! (either constructor sets both from the blocks, then `SetBreps` may change them; they
//! dereference them unchecked); ops 29 to 33 need the
//! manager (33) and the second B-rep. Negative tolerances and inverted classification
//! intervals are accepted, as upstream.

use std::path::Path;

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e300;
/// Most ops in a script.
pub const MAX_OPS: usize = 64;
/// Most handles in one list.
pub const MAX_LIST: usize = 64;

const NOW: f64 = -f64::MAX;
const NOZ: f64 = f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Run = 1,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// Script op codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ScriptOp {
    Relate = 1,
    OtherMate = 2,
    ThisMate = 3,
    BrepMate = 4,
    IsMated = 5,
    MappedBrep = 6,
    CommonEdges = 7,
    CommonVertices = 8,
    SetSubset = 9,
    FixupEnd = 11,
    FixupMid = 12,
    Getters = 13,
    SetTols = 14,
    SetBreps = 15,
    RegisterSsi = 16,
    CheckSsi = 17,
    ClearSsi = 18,
    CanImprint = 19,
    Lowest = 20,
    SetEdgeClass = 21,
    CheckInterior = 22,
    SplitTwo = 23,
    ImprintFace = 24,
    VertexFace = 25,
    EdgeFace = 26,
    FaceFace = 27,
    FaceFaceCoin = 28,
    Resolve = 29,
    Imprint = 30,
    ShellVerts = 31,
    WireLamina = 32,
    MgrNew = 33,
}

impl ScriptOp {
    /// The op's code.
    #[must_use]
    pub fn code(self) -> i64 {
        self as i64
    }

    /// The op of a code.
    #[must_use]
    pub fn from_code(code: i64) -> Option<Self> {
        use ScriptOp::{
            BrepMate, CanImprint, CheckInterior, CheckSsi, ClearSsi, CommonEdges, CommonVertices,
            EdgeFace, FaceFace, FaceFaceCoin, FixupEnd, FixupMid, Getters, Imprint, ImprintFace,
            IsMated, Lowest, MappedBrep, MgrNew, OtherMate, RegisterSsi, Relate, Resolve, SetBreps,
            SetEdgeClass, SetSubset, SetTols, ShellVerts, SplitTwo, ThisMate, VertexFace,
            WireLamina,
        };
        const ALL: [ScriptOp; 32] = [
            Relate,
            OtherMate,
            ThisMate,
            BrepMate,
            IsMated,
            MappedBrep,
            CommonEdges,
            CommonVertices,
            SetSubset,
            FixupEnd,
            FixupMid,
            Getters,
            SetTols,
            SetBreps,
            RegisterSsi,
            CheckSsi,
            ClearSsi,
            CanImprint,
            Lowest,
            SetEdgeClass,
            CheckInterior,
            SplitTwo,
            ImprintFace,
            VertexFace,
            EdgeFace,
            FaceFace,
            FaceFaceCoin,
            Resolve,
            Imprint,
            ShellVerts,
            WireLamina,
            MgrNew,
        ];
        ALL.iter().copied().find(|o| o.code() == code)
    }

    /// Whether the op may delete or split topology (no handle-taking op may follow).
    #[must_use]
    pub fn mutates(self) -> bool {
        matches!(
            self,
            Self::SplitTwo
                | Self::ImprintFace
                | Self::VertexFace
                | Self::EdgeFace
                | Self::FaceFace
                | Self::FaceFaceCoin
                | Self::Imprint
        )
    }
}

/// Handle kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum HandleKind {
    Vertex = 0,
    Edge = 1,
    Face = 2,
    EdgeCurve = 3,
    FaceSurface = 4,
    Curve = 5,
    Brep = 6,
    Null = 8,
}

mod sys {
    extern "C" {
        pub fn sc_boolean_topology_intersector_p1_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

pub(crate) fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input.
pub(crate) struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    pub(crate) fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    pub(crate) fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    pub(crate) fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    /// A signed integer in `lo..=hi`.
    pub(crate) fn int(&mut self, lo: i64, hi: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad(format!("bad integer {v} (expected {lo}..={hi})")));
        }
        Ok(v as i64)
    }

    pub(crate) fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// Checks a clamped knot vector of degree `p`.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64), FfiError> {
    let m = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad(format!("{what}: no knots")))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let short = || bad(format!("{what}: knots too short"));
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above degree + 1")));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    Ok((a, b))
}

/// Checks `k` control points sharing their rational marker (`z = NOZ` allowed for
/// curves when `planar`).
fn check_points(rd: &mut Reader<'_>, k: usize, planar: bool, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if z == NOZ && !planar {
            return Err(bad(format!("{what}: 2D data")));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        let z = if z == NOZ { 0.0 } else { z };
        if [x, y, z]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// The parameter domain `[u0, v0, u1, v1]` of a surface block, `None` for a plane.
type Domain = Option<[f64; 4]>;

/// A surface block; returns its domain.
fn surface(rd: &mut Reader<'_>) -> Result<Domain, FfiError> {
    let kind = rd.index(1)?;
    if kind == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Ok(None);
    }
    rd.flag()?;
    rd.index(11)?;
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    check_points(rd, (n + 1) * (m + 1), false, "surface")?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok(Some([u0, v0, u1, v1]))
}

/// A B-spline curve block; returns its parameter range.
fn curve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, dim == 2, "curve")?;
    let knots = rd.finites(m + 1)?;
    check_knots(&knots, p, "curve")
}

/// A B-rep block.
fn brep_block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    match rd.index(3)? {
        0 => {
            let p = Path::new(path);
            if p.extension().and_then(|e| e.to_str()) != Some("smb") || !p.is_file() {
                return Err(bad("B-rep file: need an existing .smb file"));
            }
            rd.flag()?;
        }
        1 => {
            let (a, b) = curve(rd)?;
            let t0 = rd.finite()?;
            let t1 = rd.finite()?;
            if !(a <= t0 && t0 < t1 && t1 <= b) {
                return Err(bad("wire edge interval outside the curve's"));
            }
        }
        2 => {
            let dom = surface(rd)?;
            let d = rd.finites(4)?;
            let (Some(&a), Some(&b2), Some(&c), Some(&e)) =
                (d.first(), d.get(1), d.get(2), d.get(3))
            else {
                return Err(bad("domain too short"));
            };
            if a >= c || b2 >= e {
                return Err(bad("face domain not ordered"));
            }
            if let Some([u0, v0, u1, v1]) = dom {
                if a < u0 || c > u1 || b2 < v0 || e > v1 {
                    return Err(bad("face domain outside the surface's"));
                }
            }
        }
        _ => {}
    }
    rd.flag()?;
    rd.flag()?;
    Ok(())
}

/// The two B-rep blocks of a call.
fn brep_blocks(rd: &mut Reader<'_>, paths: [&str; 2]) -> Result<(), FfiError> {
    brep_block(rd, paths[0])?;
    if rd.flag()? {
        brep_block(rd, paths[1])?;
    }
    Ok(())
}

/// The entity counts of an export block (`0 validate_only counts[10] ...`), in the
/// shim's order: regions, shells, face uses, faces, loop uses, loops, edge uses, edges,
/// vertex uses, vertices.
#[must_use]
pub fn counts(export: &[f64]) -> Option<[usize; 10]> {
    if export.first() != Some(&0.0) {
        return None;
    }
    let mut c = [0usize; 10];
    for (slot, v) in c.iter_mut().zip(export.get(2..12)?) {
        *slot = *v as usize;
    }
    Some(c)
}

/// Splits an `Export` output into the two B-reps' export blocks (`None` for an absent
/// second B-rep); `None` when the output is a build failure.
#[must_use]
pub fn split_exports(out: &[f64]) -> Option<(Vec<f64>, Option<Vec<f64>>)> {
    if out.first() != Some(&0.0) {
        return None;
    }
    let mut at = 1usize;
    let mut blocks: Vec<Option<Vec<f64>>> = Vec::new();
    for _ in 0..2 {
        let len = *out.get(at)? as usize;
        at += 1;
        if len == 0 {
            blocks.push(None);
            continue;
        }
        let block = out.get(at..at + len)?.to_vec();
        at += len;
        blocks.push(Some(block));
    }
    let a = blocks.first().cloned().flatten()?;
    let b = blocks.get(1).cloned().flatten();
    Some((a, b))
}

/// The counts the script validation checks handles against.
pub(crate) struct Counts {
    /// Per B-rep: `[vertices, edges, faces]`; `None` for an absent B-rep.
    breps: [Option<[usize; 3]>; 2],
    /// Curves registered so far.
    curves: usize,
}

impl Counts {
    /// A handle triple; returns its kind.
    pub(crate) fn handle(
        &self,
        rd: &mut Reader<'_>,
        allow_null: bool,
    ) -> Result<HandleKind, FfiError> {
        let b = rd.int(-1, 2)?;
        let kind = rd.int(-1, 8)?;
        let i = rd.int(-1, i64::from(u32::MAX))?;
        let kind = match kind {
            0 => HandleKind::Vertex,
            1 => HandleKind::Edge,
            2 => HandleKind::Face,
            3 => HandleKind::EdgeCurve,
            4 => HandleKind::FaceSurface,
            5 => HandleKind::Curve,
            6 => HandleKind::Brep,
            8 => HandleKind::Null,
            _ => return Err(bad("bad handle kind")),
        };
        match kind {
            HandleKind::Null => {
                if !allow_null {
                    return Err(bad("null handle not allowed here"));
                }
            }
            HandleKind::Curve => {
                if i < 0 || i as usize >= self.curves {
                    return Err(bad("curve handle out of range"));
                }
            }
            HandleKind::Brep => {
                if !(0..=1).contains(&i) || self.breps.get(i as usize).copied().flatten().is_none()
                {
                    return Err(bad("B-rep handle out of range"));
                }
            }
            _ => {
                if !(0..=1).contains(&b) {
                    return Err(bad("handle B-rep out of range"));
                }
                let c = self
                    .breps
                    .get(b as usize)
                    .copied()
                    .flatten()
                    .ok_or_else(|| bad("handle names an absent B-rep"))?;
                let n = match kind {
                    HandleKind::Vertex => c[0],
                    HandleKind::Edge | HandleKind::EdgeCurve => c[1],
                    _ => c[2],
                };
                if i < 0 || i as usize >= n {
                    return Err(bad("handle index out of range"));
                }
            }
        }
        Ok(kind)
    }

    pub(crate) fn handle_of(
        &self,
        rd: &mut Reader<'_>,
        allowed: &[HandleKind],
        allow_null: bool,
    ) -> Result<HandleKind, FfiError> {
        let k = self.handle(rd, allow_null)?;
        if k != HandleKind::Null && !allowed.contains(&k) {
            return Err(bad("handle of the wrong kind"));
        }
        Ok(k)
    }

    pub(crate) fn list(
        &self,
        rd: &mut Reader<'_>,
        allowed: &[HandleKind],
        allow_null: bool,
    ) -> Result<(), FfiError> {
        let n = rd.index(MAX_LIST)?;
        for _ in 0..n {
            self.handle_of(rd, allowed, allow_null)?;
        }
        Ok(())
    }
}

/// A point classification `class h`.
fn point_class(rd: &mut Reader<'_>, c: &Counts) -> Result<(), FfiError> {
    rd.int(100, 110)?; // SmPointClassificationType
    c.handle(rd, true)?;
    Ok(())
}

/// The script state a later op's validation depends on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScriptState {
    /// The constructor (0: with B-reps and maps, 1: context only, no maps).
    pub ctor: usize,
    /// An op that may delete or split topology ran (no handle-taking op may follow).
    pub mutated: bool,
    /// A proto topology manager is installed.
    pub has_mgr: bool,
    /// The intersector's B-reps are set.
    pub have_breps: [bool; 2],
    /// No op may follow (the last op ran).
    pub closed: bool,
    /// The intersector's current approximation tolerance.
    pub approx: f64,
    /// The largest magnitude of the B-rep blocks' numbers (markers excluded).
    pub block_max_abs: f64,
}

/// The script ops of a later part of the unit (`boolean.topology_intersector_p2`),
/// validated by that part's binding on top of this one's.
pub(crate) trait ExtraOps {
    /// The largest op code.
    fn max_code(&self) -> i64;
    /// Validates the arguments of op `code`; `Ok(false)` for an unknown code.
    ///
    /// # Errors
    ///
    /// [`FfiError::InvalidArgument`] for an invalid op.
    fn op(
        &self,
        code: i64,
        rd: &mut Reader<'_>,
        c: &mut Counts,
        st: &mut ScriptState,
    ) -> Result<bool, FfiError>;
}

/// Checks the script after the B-rep blocks.
#[allow(clippy::too_many_lines)]
fn script(
    rd: &mut Reader<'_>,
    mut c: Counts,
    extra: Option<&dyn ExtraOps>,
    block_max_abs: f64,
) -> Result<(), FfiError> {
    use HandleKind::{Brep, Edge, Face, FaceSurface, Vertex};
    // ctor: both constructors set both B-reps (1 through SetPrimaryBrep / SetOtherBrep)
    let ctor = rd.index(1)?;
    let mut approx = rd.finite()?; // approx (a negative value is accepted, as upstream)
    rd.finite()?; // ang
    let n_ops = rd.index(MAX_OPS)?;
    let mut mutated = false;
    let mut has_mgr = false;
    // the intersector's B-reps: both from the blocks with either constructor (1 sets
    // them with SetPrimaryBrep / SetOtherBrep), then whatever SetBreps sets
    let mut have_breps = [true, c.breps[1].is_some()];
    let any = [
        Vertex,
        Edge,
        Face,
        HandleKind::EdgeCurve,
        FaceSurface,
        HandleKind::Curve,
        Brep,
    ];
    for _ in 0..n_ops {
        let code = rd.int(1, extra.map_or(33, |x| x.max_code().max(33)))?;
        let Some(op) = ScriptOp::from_code(code) else {
            let extra = extra.ok_or_else(|| bad("bad script op"))?;
            let mut st = ScriptState {
                ctor,
                mutated,
                has_mgr,
                have_breps,
                closed: false,
                approx,
                block_max_abs,
            };
            if !extra.op(code, rd, &mut c, &mut st)? {
                return Err(bad("bad script op"));
            }
            mutated = st.mutated;
            has_mgr = st.has_mgr;
            have_breps = st.have_breps;
            if st.closed {
                rd.done()?;
                return Ok(());
            }
            continue;
        };
        let takes_handles = !matches!(
            op,
            ScriptOp::Getters
                | ScriptOp::SetTols
                | ScriptOp::ClearSsi
                | ScriptOp::CanImprint
                | ScriptOp::Resolve
                | ScriptOp::Imprint
                | ScriptOp::ShellVerts
                | ScriptOp::WireLamina
                | ScriptOp::MgrNew
                | ScriptOp::CommonEdges
                | ScriptOp::CommonVertices
        );
        if mutated && takes_handles {
            return Err(bad("handle-taking op after a mutating op"));
        }
        // the ops that dereference the intersector's B-reps without checking
        // GetCommonEdges / GetCommonVertices read only m_pBrep
        if matches!(op, ScriptOp::CommonEdges | ScriptOp::CommonVertices) && !have_breps[0] {
            return Err(bad("op dereferences a null intersector B-rep"));
        }
        if matches!(
            op,
            ScriptOp::FaceFace | ScriptOp::FaceFaceCoin | ScriptOp::Resolve | ScriptOp::Imprint
        ) && (!have_breps[0] || !have_breps[1])
        {
            return Err(bad("op dereferences a null intersector B-rep"));
        }
        match op {
            ScriptOp::Relate | ScriptOp::IsMated => {
                c.handle_of(rd, &any, true)?;
                c.handle_of(rd, &any, true)?;
            }
            ScriptOp::OtherMate | ScriptOp::ThisMate | ScriptOp::BrepMate => {
                c.handle_of(rd, &any, true)?;
            }
            ScriptOp::MappedBrep => {
                rd.index(100_000)?;
            }
            ScriptOp::CommonEdges
            | ScriptOp::CommonVertices
            | ScriptOp::Getters
            | ScriptOp::ClearSsi => {}
            ScriptOp::SetSubset | ScriptOp::Lowest => c.list(rd, &any, true)?,
            ScriptOp::FixupEnd | ScriptOp::FixupMid => {
                let dim = rd.index(3)?;
                if dim < 2 {
                    return Err(bad("point dimension must be 2 or 3"));
                }
                rd.finite()?; // tolerance (negative accepted, as upstream)
                point_class(rd, &c)?;
                point_class(rd, &c)?;
            }
            ScriptOp::SetTols => {
                approx = rd.finite()?;
                rd.finite()?;
            }
            ScriptOp::SetBreps => {
                have_breps[0] = c.handle_of(rd, &[Brep], true)? != HandleKind::Null;
                have_breps[1] = c.handle_of(rd, &[Brep], true)? != HandleKind::Null;
            }
            ScriptOp::RegisterSsi => {
                c.handle_of(rd, &[FaceSurface], true)?;
                c.handle_of(rd, &[FaceSurface], true)?;
                let n = rd.index(8)?;
                for _ in 0..n {
                    curve(rd)?;
                    c.curves += 1;
                    for _ in 0..2 {
                        if rd.flag()? {
                            curve(rd)?;
                            c.curves += 1;
                        }
                    }
                    rd.index(6)?; // SmTsectCurveType: 0..=6
                    rd.finite()?;
                }
            }
            ScriptOp::CheckSsi => {
                c.handle_of(rd, &[FaceSurface], true)?;
                c.handle_of(rd, &[FaceSurface], true)?;
            }
            ScriptOp::CanImprint => {
                rd.int(100, 110)?;
            }
            ScriptOp::SetEdgeClass => {
                // (an inverted interval and a negative tolerance are accepted, as
                // upstream: SmExtent1d(t0, t1) swaps the ends)
                c.handle_of(rd, &[Edge], false)?;
                rd.finite()?;
                rd.finite()?;
                rd.finite()?;
            }
            ScriptOp::CheckInterior => {
                c.handle_of(rd, &[Edge], false)?;
                rd.finite()?;
                rd.finite()?;
            }
            ScriptOp::SplitTwo => {
                c.handle_of(rd, &[Edge], false)?;
                c.handle_of(rd, &[Edge], false)?;
                rd.finite()?;
            }
            ScriptOp::ImprintFace => {
                c.handle_of(rd, &[Face], false)?;
                rd.vec3()?;
                rd.finite()?;
            }
            ScriptOp::VertexFace => {
                c.list(rd, &[Vertex], true)?;
                c.list(rd, &[Face], true)?;
                rd.flag()?;
                rd.flag()?;
            }
            ScriptOp::EdgeFace => {
                c.list(rd, &[Edge], true)?;
                c.list(rd, &[Face], true)?;
                rd.flag()?;
                rd.flag()?;
            }
            ScriptOp::FaceFace => {
                c.list(rd, &[Vertex], false)?;
                c.list(rd, &[Face], false)?;
                c.list(rd, &[Face], false)?;
                rd.flag()?;
                rd.flag()?;
            }
            ScriptOp::FaceFaceCoin => {
                c.list(rd, &[Edge], true)?;
                c.list(rd, &[Face], false)?;
                rd.flag()?;
                rd.flag()?;
            }
            ScriptOp::Resolve | ScriptOp::Imprint => {
                if !has_mgr {
                    return Err(bad("no proto topology manager"));
                }
            }
            ScriptOp::ShellVerts => {
                if !has_mgr {
                    return Err(bad("no proto topology manager"));
                }
                rd.index(1)?;
                rd.flag()?;
            }
            ScriptOp::WireLamina => {
                if !has_mgr {
                    return Err(bad("no proto topology manager"));
                }
                rd.index(1)?;
                rd.flag()?;
                rd.flag()?;
            }
            ScriptOp::MgrNew => {
                if has_mgr || c.breps[1].is_none() {
                    return Err(bad("manager already made or no second B-rep"));
                }
                rd.finite()?; // capture distance (negative accepted, as upstream)
                rd.finite()?;
                has_mgr = true;
            }
        }
        if op.mutates() {
            mutated = true;
        }
    }
    Ok(())
}

/// A shim entry point (`sc_boolean_topology_intersector_p1_eval` or a later part's).
pub(crate) type Entry = unsafe extern "C" fn(
    i32,
    *const std::ffi::c_char,
    *const f64,
    i64,
    *mut f64,
    i64,
    *mut i64,
) -> i32;

fn raw(entry: Entry, op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            entry(
                op.code(),
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs `op` on `input` (see the module docs); `paths` are the `.smb` files of the
/// two file B-rep blocks (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, paths: [&str; 2], input: &[f64]) -> Result<Vec<f64>, FfiError> {
    eval_with(
        sys::sc_boolean_topology_intersector_p1_eval,
        None,
        op,
        paths,
        input,
    )
}

/// [`eval`] through `entry`, with the later part's ops `extra`.
///
/// # Errors
///
/// As [`eval`].
pub(crate) fn eval_with(
    entry: Entry,
    extra: Option<&dyn ExtraOps>,
    op: Op,
    paths: [&str; 2],
    input: &[f64],
) -> Result<Vec<f64>, FfiError> {
    if paths.iter().any(|p| p.contains('\n')) {
        return Err(bad("path with a newline"));
    }
    let joined = format!("{}\n{}", paths[0], paths[1]);
    let mut rd = Reader { input, at: 0 };
    brep_blocks(&mut rd, paths)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = raw(entry, Op::Export, &joined, block)?;
    if op == Op::Export {
        rd.done()?;
        return Ok(export);
    }
    let Some((a, b)) = split_exports(&export) else {
        // a B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    let count3 = |ex: &[f64]| counts(ex).map(|c| [c[9], c[7], c[3]]);
    let c = Counts {
        breps: [count3(&a), b.as_deref().and_then(count3)],
        curves: 0,
    };
    if c.breps[0].is_none() {
        return Err(bad("first export unreadable"));
    }
    // (NOW / NOZ markers are +-f64::MAX)
    let block_max_abs = block
        .iter()
        .map(|v| v.abs())
        .filter(|v| *v < f64::MAX)
        .fold(0.0, f64::max);
    script(&mut rd, c, extra, block_max_abs)?;
    rd.done()?;
    raw(entry, op, &joined, input)
}
