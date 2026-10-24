//! FFI wrappers for unit `offset.offset_geometry_creation`.
//!
//! Shim: `harness/cpp/shim/offset_offset_geometry_creation.cpp`
//! (`sc_offset_offset_geometry_creation_eval`). Upstream:
//! `source/SMLib/src/SmOffsetGeometryCreation.cpp`.
//!
//! Every call starts with two B-rep blocks (as in [`super::topology_vertexuse`]; the
//! second is preceded by a `has_b` flag and may be absent): the original B-rep of an
//! `SmOffsetExecutive` and the "offset" B-rep its edge uses are mapped to. `paths`
//! holds the `.smb` file of each file block (ignored otherwise). The output starts
//! with `0`, or with `-1 status` (`-3 status`) when the first (second) B-rep could not
//! be built, and nothing else follows.
//! Unit-local B-rep block `6 kind a b` builds a real `SmCylinder` (`kind=0`,
//! radius/height) or `SmTorus` (`kind=1`, major/minor radii), at the origin with
//! full angular domains. Dimensions are finite and positive. Unlike the shared
//! blocks, this block has no validate-only or gap-initialization flags.
//!
//! | [`Op`]   | after the blocks | outputs |
//! |----------|------------------|---------|
//! | `Export` |                  | per B-rep `len export` (`len 0` for an absent one), each export a `0`-prefixed graph as [`super::topology_vertexuse`] exports it; then per B-rep `n_faces` (`0` for an absent one) and per face (export order) `-1`, or `n byte*n`: the binary `WriteToDB` record (version 33) of a revolution, extrusion, cone, cylinder, sphere or torus, dispatched by the graph's native surface type code; then per B-rep `n_faces` and per face the state of its surface's cache after the build (`0` none, `1` tree not built, `2` tree built) |
//! | `Run`    | `dist approx make_analytics n_local (face surface)* inset extend n_shell face* n_pairs (eu eu)* n_ops ops*` | per op its code, a status and its outputs (below): an op's out-parameters follow a success status only, the states of existing topology (edge states, a face's domain and surface) follow every status |
//!
//! The run header takes any finite `dist` (zero included) and any finite `approx` that
//! is not negative (upstream neither crashes nor hangs at zero; a negative tolerance is
//! refused, no upstream caller passes one). It makes `SmOffsetGeometryCreation(dist,
//! approx)` (with `SetMakeAnalytics`), `SmLocalOperationGeometryCreation(dist, approx, faces,
//! surfaces)` from the `n_local` pairs, and an `SmOffsetExecutive` over the first
//! B-rep with `SetInset`, `SetExtendConvexEdges`, `SetShellFaces` (when `n_shell > 0`)
//! and its original-to-offset edge-use relation filled with the `n_pairs` pairs (a
//! many-to-many relation: an edge use mapped to two different offset edge uses makes
//! `GetOffsetEdgeuse` return null for it; a repeated pair adds nothing). A mark is locked for the run
//! (`SmNewMarkAndLock`). Before every op the executive's extended-surface cache is
//! seeded with every face surface of both B-reps and every store surface mapped to
//! itself, so `GetCreateExtendedSurface` returns the base surface unextended and the
//! non-returning `CreateExtendedSurface` loops (HANG-16, HANG-17) never run; the Rust
//! runner seeds the ported executive's cache the same way.
//!
//! An object handle is a triple `brep kind index`: `kind` 0 vertex, 1 edge, 2 face, 9
//! edge use, 10 face use (of B-rep `brep`, `index` in its export's traversal order), 3
//! an edge's curve, 4 a face's surface, 11 the UV trim curve stored on an edge use, 5 a
//! curve of the script's store, 7 a surface of the script's store (both in `add` order),
//! 6 a B-rep (`index` 0 or 1), 8 null. A status is `0` (success), the upstream `SmStatus`
//! code, or `-2` when the shim refused the op because upstream would dereference null
//! (an op that walks the topology needs every edge use of both B-reps to be a loop edge
//! use with edge, curve, vertex, mate, neighbours and a face with surface; `VertexOffset`
//! walks no loops and needs only a curve on each edge and, for a single edge, a face
//! with surface and a B-spline curve). `-6` ends
//! the script at an op nothing is compared at: a non-local `FaceuseOffset` of a face
//! whose concrete kind is not exported (the Rust side defers), or of an exactly
//! `SmSurfOfRevolution` surface when self-intersection is enabled (HANG-26: its offset
//! self-intersection takes 20 s to minutes, or never returns at large distances), and an
//! `EdgeuseOffset` with `|offset distance| > 1e6` (HANG-27). The difftest runner rejects
//! the hang cases from the Rust script before calling here; the `-6` stop is a safety net.
//!
//! The `n_local` pairs name store surfaces only (kind 7): `SmLocalOperationGeometryCreation`
//! owns its listed surfaces (with `make_analytics` it reparameterises and deletes each one
//! after copying it), so a face's own surface there is refused as invalid input. The
//! handles are resolved against the store as it stands at each `LocalFaceuseOffset`.
//!
//! Geometry outputs: `curve` is the fixture header's `put_curve` block (`-1` null,
//! `-2` not a B-spline curve, else `type dim has_nurb [n p m cpts knots] [line data]`);
//! `surface` is `-1` or `type u0 u1 v0 v1` then nine `0 x y z` / `status` samples on a
//! 3x3 grid of the natural domain; `edge_state` is `curve t0 t1 tol has_v [point]
//! has_w [point] n_eus uv_trim_curve*`.
//!
//! Script ops (`op args -> outputs`; `h` a handle, `c` a store-curve handle):
//!
//! | op | name | args | outputs |
//! |----|------|------|---------|
//! | 1  | add curve                  | curve block              | `0 index` |
//! | 2  | add surface                | surface block            | `0 index` |
//! | 3  | `SmEdge::SetCurve(copy, TRUE)` + `SetOwner` + `SetInterval(natural)` | `h(edge)|null c|null` | `status [edge_state]` |
//! | 4  | `SmEdgeuse::SetUVTrimCurve(copy, max_dist, TRUE)` | `h(eu)|null c|null max_dist` | `status [curve]` |
//! | 5  | `SmTopology::Mark`         | `h(eu)|null`             | `status [marked]` |
//! | 6  | getters                    |                          | `0 approx tan_rad tan_deg dist local_approx local_tan_deg local_dist exec_tan_deg inset extend n_shell` |
//! | 7  | `FaceuseOffset`            | `h(fu) skip`             | `status has_surface [surface] n (curve curve curve type dev)*` |
//! | 8  | `VertexOffset`             | `h(vertex) n h(edge)*`   | `status n curve* n orient* has_surface [surface]` |
//! | 9  | `UpdateFaceSurfaceDomain`  | `h(face) h(surface)`     | `status u0 u1 v0 v1 surface` |
//! | 10 | `ReplaceEdgeCurves`        | `h(eu) c|null c|null h(eu)|null` | `status delete_3d delete_uv edge_state` |
//! | 11 | `TrimExtendedEdge`         | `h(eu) h(eu)|null h(eu)|null h(eu)|null h(curve) c|null c|null` | `status good [t0 t1]` |
//! | 12 | `ExtendTrimEdges`          | `h(eu) h(eu)|null h(eu) h(eu)|null` | `status edge_state has_other [edge_state]` |
//! | 13 | `MateEdges`                | `h(eu)|null x4`          | `status has_off [edge_state] has_other [edge_state]` |
//! | 14 | `HaveTangentGaps`          | `n h(eu) h(eu)`          | `0 flag` |
//! | 15 | `EdgeuseOffset`            | `h(eu) h(eu)|null h(eu)|null h(eu)|null` | `status has_surface [surface] n curve* n orient* n_self has_off [edge_state] has_other [edge_state]` |
//! | 16 | `SmLocalOperationGeometryCreation::FaceuseOffset` | `h(fu) skip` | as op 7 |
//! | 17 | edge state                 | `h(edge)`                | `0 edge_state` |
//! | 18 | `SmEdge::SetTolerance(tol, FALSE, FALSE)` | `h(edge) tol` | `0 tol` |
//!
//! The validation here checks the layouts, the handle ranges and kinds, and that the
//! store curves given as UV curves are 2D and as 3D curves are 3D (what the functions
//! assume of their inputs); the null-dereference guards are the shim's.

use std::path::Path;

use crate::{call_with_buffer, FfiError};

/// Largest control point index (edge curves copied from the `pt_TestFiles` B-reps
/// have hundreds of control points, `Mouse.smb` up to 240).
pub const MAX_INDEX: usize = 4096;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e300;
/// Most ops in a script.
pub const MAX_OPS: usize = 64;
/// Most handles in one list.
pub const MAX_LIST: usize = 64;
/// Most samples of `HaveTangentGaps`.
pub const MAX_SAMPLES: i64 = 1000;

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
    AddCurve = 1,
    AddSurface = 2,
    SetEdgeCurve = 3,
    SetEuUv = 4,
    Mark = 5,
    Getters = 6,
    FaceuseOffset = 7,
    VertexOffset = 8,
    UpdateFaceDomain = 9,
    ReplaceEdgeCurves = 10,
    TrimExtendedEdge = 11,
    ExtendTrimEdges = 12,
    MateEdges = 13,
    HaveTangentGaps = 14,
    EdgeuseOffset = 15,
    LocalFaceuseOffset = 16,
    EdgeState = 17,
    SetEdgeTol = 18,
}

impl ScriptOp {
    /// The op's code.
    #[must_use]
    pub fn code(self) -> f64 {
        self as i64 as f64
    }

    fn from_code(c: i64) -> Option<Self> {
        Some(match c {
            1 => Self::AddCurve,
            2 => Self::AddSurface,
            3 => Self::SetEdgeCurve,
            4 => Self::SetEuUv,
            5 => Self::Mark,
            6 => Self::Getters,
            7 => Self::FaceuseOffset,
            8 => Self::VertexOffset,
            9 => Self::UpdateFaceDomain,
            10 => Self::ReplaceEdgeCurves,
            11 => Self::TrimExtendedEdge,
            12 => Self::ExtendTrimEdges,
            13 => Self::MateEdges,
            14 => Self::HaveTangentGaps,
            15 => Self::EdgeuseOffset,
            16 => Self::LocalFaceuseOffset,
            17 => Self::EdgeState,
            18 => Self::SetEdgeTol,
            _ => return None,
        })
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
    Surface = 7,
    Null = 8,
    Edgeuse = 9,
    Faceuse = 10,
    EuUv = 11,
}

impl HandleKind {
    /// The kind's code.
    #[must_use]
    pub fn code(self) -> i64 {
        self as i64
    }

    fn from_code(c: i64) -> Option<Self> {
        Some(match c {
            0 => Self::Vertex,
            1 => Self::Edge,
            2 => Self::Face,
            3 => Self::EdgeCurve,
            4 => Self::FaceSurface,
            5 => Self::Curve,
            6 => Self::Brep,
            7 => Self::Surface,
            8 => Self::Null,
            9 => Self::Edgeuse,
            10 => Self::Faceuse,
            11 => Self::EuUv,
            _ => return None,
        })
    }
}

mod sys {
    extern "C" {
        pub fn sc_offset_offset_geometry_creation_eval(
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

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

crate::reader::unit_reader!();

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
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

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    /// A signed integer in `lo..=hi`.
    fn int(&mut self, lo: i64, hi: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad(format!("bad integer {v} (expected {lo}..={hi})")));
        }
        Ok(v as i64)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
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

/// A B-spline curve block; returns its dimension and parameter range.
fn curve(rd: &mut Reader<'_>) -> Result<(usize, f64, f64), FfiError> {
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
    let (a, b) = check_knots(&knots, p, "curve")?;
    Ok((dim, a, b))
}

/// A B-rep block.
fn brep_block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    match rd.index(6)? {
        0 => {
            let p = Path::new(path);
            if p.extension().and_then(|e| e.to_str()) != Some("smb") || !p.is_file() {
                return Err(bad("B-rep file: need an existing .smb file"));
            }
            rd.flag()?;
        }
        1 => {
            let (_, a, b) = curve(rd)?;
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
        3 => {}
        6 => {
            rd.index(1)?;
            if rd.finite()? <= 0.0 || rd.finite()? <= 0.0 {
                return Err(bad("primitive dimensions must be positive"));
            }
            return Ok(());
        }
        _ => return Err(bad("unsupported B-rep fixture kind")),
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
    let mut rd = Reader::new(out);
    let [a, b] = export_blocks(&mut rd)?;
    Some((a?.to_vec(), b.map(<[f64]>::to_vec)))
}

fn export_blocks<'a>(rd: &mut Reader<'a>) -> Option<[Option<&'a [f64]>; 2]> {
    if rd.num().ok()? != 0.0 {
        return None;
    }
    let mut blocks = [None, None];
    let limit = rd.input.len();
    for block in &mut blocks {
        let len = rd.index(limit).ok()?;
        if len != 0 {
            let values = rd.take(len).ok()?;
            if values.first() != Some(&0.0) {
                return None;
            }
            *block = Some(values);
        }
    }
    blocks.first().copied().flatten()?;
    Some(blocks)
}

/// The per-face concrete surface records after an `Export` output's two blocks:
/// binary `WriteToDB` bytes in native face order (`None` for kinds represented by
/// the graph alone; an empty list for an absent B-rep). The graph's surface type
/// selects the reader. Counts must match the graph and bytes must be exact integers.
/// Returns `None` for a failed build, malformed record or cache state, or trailing
/// output.
#[must_use]
pub fn split_surface_records(out: &[f64]) -> Option<[Vec<Option<Vec<u8>>>; 2]> {
    split_export_tail(out).map(|(records, _)| records)
}

/// The per-face cache states after an `Export` output's surface records: the state of
/// each face surface's cache after the C++ build, in native face order (an empty list
/// for an absent B-rep): `0` no cache, `1` a cache whose tree is not built, `2` a cache
/// whose tree is built (`solidcore::surface::surface::CacheTreeState`). Returns `None`
/// as [`split_surface_records`].
#[must_use]
pub fn split_cache_tree_states(out: &[f64]) -> Option<[Vec<u8>; 2]> {
    split_export_tail(out).map(|(_, states)| states)
}

/// Per-face surface records and cache states of an `Export` output.
type ExportTail = ([Vec<Option<Vec<u8>>>; 2], [Vec<u8>; 2]);

fn split_export_tail(out: &[f64]) -> Option<ExportTail> {
    let mut rd = Reader::new(out);
    let blocks = export_blocks(&mut rd)?;
    let face_count = |block: Option<&[f64]>| -> Option<usize> {
        match block {
            Some(block) => {
                let mut header = Reader::new(block);
                header.skip(5).ok()?;
                header.index(out.len()).ok()
            }
            None => Some(0),
        }
    };
    let mut breps: [Vec<Option<Vec<u8>>>; 2] = [Vec::new(), Vec::new()];
    for (faces, block) in breps.iter_mut().zip(blocks) {
        let expected = face_count(block)?;
        let n = rd.index(out.len()).ok()?;
        if n != expected {
            return None;
        }
        for _ in 0..n {
            let len = rd.num().ok()?;
            if len == -1.0 {
                faces.push(None);
                continue;
            }
            if len.fract() != 0.0 || len <= 0.0 || len > out.len() as f64 {
                return None;
            }
            let bytes = rd
                .take(len as usize)
                .ok()?
                .iter()
                .map(|&b| (b.fract() == 0.0 && (0.0..=255.0).contains(&b)).then_some(b as u8))
                .collect::<Option<Vec<u8>>>()?;
            faces.push(Some(bytes));
        }
    }
    let mut states: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    for (faces, block) in states.iter_mut().zip(blocks) {
        let n = rd.index(out.len()).ok()?;
        if n != face_count(block)? {
            return None;
        }
        for _ in 0..n {
            let s = rd.num().ok()?;
            if s != 0.0 && s != 1.0 && s != 2.0 {
                return None;
            }
            faces.push(s as u8);
        }
    }
    rd.done().ok()?;
    Some((breps, states))
}

/// Per B-rep: `[vertices, edges, faces, edgeuses, faceuses]`.
type BrepCounts = [usize; 5];

/// The counts the script validation checks handles against.
struct Counts {
    breps: [Option<BrepCounts>; 2],
    /// The dimension of each store curve.
    curves: Vec<usize>,
    surfaces: usize,
}

/// A validated handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Handle {
    brep: i64,
    kind: HandleKind,
    index: usize,
}

impl Counts {
    fn of(c: BrepCounts, kind: HandleKind) -> usize {
        match kind {
            HandleKind::Vertex => c[0],
            HandleKind::Edge | HandleKind::EdgeCurve => c[1],
            HandleKind::Face | HandleKind::FaceSurface => c[2],
            HandleKind::Edgeuse | HandleKind::EuUv => c[3],
            HandleKind::Faceuse => c[4],
            _ => 0,
        }
    }

    /// A header handle naming a store surface that the script's ops add later
    /// (resolved against the store at each local op, null while absent): the kind
    /// and the index's sign are checked, its range is not.
    fn store_surface_lazy(rd: &mut Reader<'_>) -> Result<(), FfiError> {
        let _b = rd.int(-1, 2)?;
        let kind = rd.int(-1, 11)?;
        let i = rd.int(-1, i64::MAX)?;
        let kind = HandleKind::from_code(kind).ok_or_else(|| bad("bad handle kind"))?;
        match kind {
            HandleKind::Null => Ok(()),
            HandleKind::Surface if i >= 0 => Ok(()),
            HandleKind::Surface => Err(bad("negative handle index")),
            _ => Err(bad("handle of the wrong kind")),
        }
    }

    /// Reads and checks a handle; `None` for null.
    fn handle(&self, rd: &mut Reader<'_>) -> Result<Option<Handle>, FfiError> {
        let b = rd.int(-1, 2)?;
        let kind = rd.int(-1, 11)?;
        let i = rd.int(-1, i64::MAX)?;
        let kind = HandleKind::from_code(kind).ok_or_else(|| bad("bad handle kind"))?;
        if kind == HandleKind::Null {
            return Ok(None);
        }
        let index = usize::try_from(i).map_err(|_| bad("negative handle index"))?;
        match kind {
            HandleKind::Curve => {
                if index >= self.curves.len() {
                    return Err(bad("curve handle out of range"));
                }
            }
            HandleKind::Surface => {
                if index >= self.surfaces {
                    return Err(bad("surface handle out of range"));
                }
            }
            HandleKind::Brep => {
                let slot = usize::try_from(b).map_err(|_| bad("B-rep handle out of range"))?;
                if slot > 1 || index != slot || self.breps.get(slot).copied().flatten().is_none() {
                    return Err(bad("B-rep handle out of range"));
                }
            }
            _ => {
                let slot = usize::try_from(b).map_err(|_| bad("handle B-rep out of range"))?;
                let c = self
                    .breps
                    .get(slot)
                    .copied()
                    .flatten()
                    .ok_or_else(|| bad("handle names an absent B-rep"))?;
                if index >= Self::of(c, kind) {
                    return Err(bad("handle index out of range"));
                }
            }
        }
        Ok(Some(Handle {
            brep: b,
            kind,
            index,
        }))
    }

    /// A handle of one of `kinds` (or null when `allow_null`).
    fn kinded(
        &self,
        rd: &mut Reader<'_>,
        kinds: &[HandleKind],
        allow_null: bool,
    ) -> Result<Option<Handle>, FfiError> {
        let h = self.handle(rd)?;
        match h {
            None if allow_null => Ok(None),
            None => Err(bad("null handle not allowed here")),
            Some(h) if kinds.contains(&h.kind) => Ok(Some(h)),
            Some(_) => Err(bad("handle of the wrong kind")),
        }
    }

    /// A store curve of dimension `dim` (or null when `allow_null`).
    fn store_curve(
        &self,
        rd: &mut Reader<'_>,
        dim: usize,
        allow_null: bool,
    ) -> Result<(), FfiError> {
        if let Some(h) = self.kinded(rd, &[HandleKind::Curve], allow_null)? {
            if self.curves.get(h.index).copied() != Some(dim) {
                return Err(bad(format!("store curve must be {dim}D here")));
            }
        }
        Ok(())
    }

    fn edgeuse(&self, rd: &mut Reader<'_>, allow_null: bool) -> Result<Option<Handle>, FfiError> {
        self.kinded(rd, &[HandleKind::Edgeuse], allow_null)
    }
}

const EU: HandleKind = HandleKind::Edgeuse;

/// The run header and script.
#[allow(clippy::too_many_lines)]
fn script(rd: &mut Reader<'_>, mut c: Counts) -> Result<(), FfiError> {
    rd.finite()?; // the offset distance: any finite value, zero included
    let approx = rd.finite()?;
    if approx < 0.0 {
        return Err(bad("approximation tolerance must not be negative"));
    }
    rd.flag()?; // make_analytics
    let n_local = rd.index(MAX_LIST)?;
    for _ in 0..n_local {
        c.kinded(rd, &[HandleKind::Face], true)?;
        // a store surface only: the local creation owns the listed surfaces (with
        // analytics on it reparameterises and deletes each one after copying it), so a
        // face's own surface here corrupts its B-rep; the surface may be added by a
        // later op (null until then)
        Counts::store_surface_lazy(rd)?;
    }
    rd.flag()?; // inset
    rd.flag()?; // extend convex edges
    let n_shell = rd.index(MAX_LIST)?;
    for _ in 0..n_shell {
        c.kinded(rd, &[HandleKind::Face], true)?;
    }
    let n_pairs = rd.index(MAX_LIST)?;
    for _ in 0..n_pairs {
        // (the relation is many-to-many: a repeated edge use is a valid pair)
        c.edgeuse(rd, false)?.ok_or_else(|| bad("null pair"))?;
        c.edgeuse(rd, false)?.ok_or_else(|| bad("null pair"))?;
    }

    let n_ops = rd.index(MAX_OPS)?;
    for _ in 0..n_ops {
        let op = ScriptOp::from_code(rd.int(1, 18)?).ok_or_else(|| bad("bad script op"))?;
        match op {
            ScriptOp::AddCurve => {
                let (dim, _, _) = curve(rd)?;
                c.curves.push(dim);
            }
            ScriptOp::AddSurface => {
                surface(rd)?;
                c.surfaces += 1;
            }
            ScriptOp::SetEdgeCurve => {
                // (a null edge or curve is refused by the shim with -2)
                c.kinded(rd, &[HandleKind::Edge], true)?;
                c.store_curve(rd, 3, true)?;
            }
            ScriptOp::SetEuUv => {
                c.edgeuse(rd, true)?; // (a null edge use is refused by the shim with -2)
                c.store_curve(rd, 2, true)?;
                let d = rd.finite()?;
                if d < 0.0 {
                    return Err(bad("negative distance"));
                }
            }
            ScriptOp::Mark => {
                c.edgeuse(rd, true)?; // (a null edge use is refused by the shim with -2)
            }
            ScriptOp::Getters => {}
            ScriptOp::FaceuseOffset | ScriptOp::LocalFaceuseOffset => {
                c.kinded(rd, &[HandleKind::Faceuse], true)?;
                rd.flag()?;
            }
            ScriptOp::VertexOffset => {
                c.kinded(rd, &[HandleKind::Vertex], true)?;
                let n = rd.index(MAX_LIST)?;
                for _ in 0..n {
                    c.kinded(rd, &[HandleKind::Edge], true)?;
                }
            }
            ScriptOp::UpdateFaceDomain => {
                c.kinded(rd, &[HandleKind::Face], true)?;
                c.kinded(rd, &[HandleKind::Surface, HandleKind::FaceSurface], true)?;
            }
            ScriptOp::ReplaceEdgeCurves => {
                c.edgeuse(rd, true)?;
                c.store_curve(rd, 3, true)?;
                c.store_curve(rd, 2, true)?;
                c.edgeuse(rd, true)?;
            }
            ScriptOp::TrimExtendedEdge => {
                for _ in 0..4 {
                    c.edgeuse(rd, true)?;
                }
                if let Some(h) = c.kinded(rd, &[HandleKind::Curve, HandleKind::EdgeCurve], true)? {
                    if h.kind == HandleKind::Curve && c.curves.get(h.index).copied() != Some(3) {
                        return Err(bad("store curve must be 3D here"));
                    }
                }
                c.store_curve(rd, 2, true)?;
                c.store_curve(rd, 2, true)?;
            }
            ScriptOp::ExtendTrimEdges | ScriptOp::MateEdges | ScriptOp::EdgeuseOffset => {
                for _ in 0..4 {
                    c.kinded(rd, &[EU], true)?;
                }
            }
            ScriptOp::HaveTangentGaps => {
                rd.int(0, MAX_SAMPLES)?;
                c.edgeuse(rd, true)?;
                c.edgeuse(rd, true)?;
            }
            ScriptOp::EdgeState => {
                c.kinded(rd, &[HandleKind::Edge], true)?;
            }
            ScriptOp::SetEdgeTol => {
                c.kinded(rd, &[HandleKind::Edge], true)?;
                let t = rd.finite()?;
                if t < 0.0 {
                    return Err(bad("negative tolerance"));
                }
            }
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_offset_offset_geometry_creation_eval(
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
    if paths.iter().any(|p| p.contains('\n')) {
        return Err(bad("path with a newline"));
    }
    let joined = format!("{}\n{}", paths[0], paths[1]);
    let mut rd = Reader::new(input);
    brep_blocks(&mut rd, paths)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = raw(Op::Export, &joined, block)?;
    if op == Op::Export {
        rd.done()?;
        return Ok(export);
    }
    let Some((a, b)) = split_exports(&export) else {
        // a B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    let count5 = |ex: &[f64]| counts(ex).map(|c| [c[9], c[7], c[3], c[6], c[2]]);
    let c = Counts {
        breps: [count5(&a), b.as_deref().and_then(count5)],
        curves: Vec::new(),
        surfaces: 0,
    };
    if c.breps[0].is_none() {
        return Err(bad("first export unreadable"));
    }
    script(&mut rd, c)?;
    rd.done()?;
    raw(op, &joined, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export(faces: usize, other_faces: Option<usize>, tail: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0];
        for count in [Some(faces), other_faces] {
            if let Some(count) = count {
                let mut block = vec![0.0; 12];
                block[5] = count as f64;
                out.push(block.len() as f64);
                out.extend(block);
            } else {
                out.push(0.0);
            }
        }
        out.extend_from_slice(tail);
        out
    }

    #[test]
    fn surface_records_preserve_face_slots_both_arenas_and_all_byte_values() {
        let out = export(
            2,
            Some(1),
            &[2.0, -1.0, 3.0, 0.0, 1.0, 255.0, 1.0, 1.0, 42.0],
        );
        assert_eq!(
            split_surface_records(&out),
            Some([vec![None, Some(vec![0, 1, 255])], vec![Some(vec![42])],])
        );
        let out = export(1, None, &[1.0, -1.0, 0.0]);
        assert_eq!(split_surface_records(&out), Some([vec![None], vec![]]));
    }

    #[test]
    fn primitive_fixture_blocks_require_known_kinds_and_positive_finite_dimensions() {
        for kind in [0.0, 1.0] {
            for size in [0.01, 1.0, 100.0] {
                let input = [6.0, kind, size, size];
                let mut rd = Reader::new(&input);
                brep_block(&mut rd, "").unwrap();
                rd.done().unwrap();
            }
            for size in [-1.0, -0.0, 0.0, f64::NAN, f64::INFINITY, f64::MAX] {
                for input in [[6.0, kind, size, 1.0], [6.0, kind, 1.0, size]] {
                    assert!(brep_block(&mut Reader::new(&input), "").is_err());
                }
            }
        }
        for kind in [-1.0, 0.5, 2.0, f64::NAN] {
            assert!(brep_block(&mut Reader::new(&[6.0, kind, 1.0, 1.0]), "").is_err());
        }
    }

    #[test]
    fn surface_records_reject_malformed_counts_lengths_bytes_and_trailing_data() {
        for value in [-2.0, -0.5, 0.5, f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(split_surface_records(&export(1, None, &[value, -1.0, 0.0])).is_none());
            assert!(split_surface_records(&export(1, None, &[1.0, value, 0.0])).is_none());
        }
        for byte in [-1.0, 0.5, 256.0, f64::NAN, f64::INFINITY] {
            assert!(split_surface_records(&export(1, None, &[1.0, 1.0, byte, 0.0])).is_none());
        }
        for tail in [
            vec![],
            vec![1.0],
            vec![1.0, 2.0, 1.0],
            vec![1.0, 0.0, 0.0],
            vec![0.0, 0.0],
            vec![2.0, -1.0, -1.0, 0.0],
            vec![1.0, -1.0, 1.0, -1.0],
            vec![1.0, -1.0, 0.0, 17.0],
        ] {
            assert!(
                split_surface_records(&export(1, None, &tail)).is_none(),
                "{tail:?}"
            );
        }
        let valid = export(1, None, &[1.0, -1.0, 0.0]);
        for length in [f64::NAN, f64::INFINITY, f64::MAX, -1.0, 1.5] {
            let mut out = valid.clone();
            out[1] = length;
            assert!(split_exports(&out).is_none());
            assert!(split_surface_records(&out).is_none());
        }
    }
}
