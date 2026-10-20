//! FFI wrappers for unit `topology.brep_p2`.
//!
//! Shim: `harness/cpp/shim/topology_brep_p2.cpp` (`sc_topology_brep_p2_eval`).
//! Upstream: `source/SMLib/src/SmBrep.cpp`.
//!
//! Every input is a B-rep block of [`super::topology_vertexuse`] (kinds `0..=3`), then
//! `editing` (`-1` keep, `0` / `1` set `m_bEditingEnabled`), then the op's parameters.
//! Indices name entities in the export's traversal order. Every output starts with
//! `0`, or `-1 status` when the B-rep could not be built (nothing else follows). A
//! curve block is [`super::topology_vertexuse`]'s (`dim n p m cpts knots`), a surface
//! block too (`0 ..` B-spline, `1 O N` plane). An entity reference is `type index`
//! with an `SmTopoTypes.h` code (`-1 -1` null, `16011 0` the B-rep).
//!
//! | [`Op`]     | parameters | outputs |
//! |------------|------------|---------|
//! | `Export`   |            | the graph in [`super::topology_vertexuse`]'s export layout (faces, edges and vertices registered in B-rep list order first, the walk including the vertex lists) |
//! | `Extras`   |            | `tol infinite_region region_list_head editing`, per vertex `n vu*`, per shell its `m_tShellType` |
//! | `Query`    | `which`    | `0`: `n eu*` (`GetEdgeuses`), `1`: `n v*` (`GetShellVertices`), `2` `IsManifoldSolid`, `3` `IsSheet`, `4` `IsOrientedSheet`, `5`: `size edge` (`GetMinEdgeBBoxSize`) |
//! | `SetTol`   | `tol only_larger cascade` | `brep_tol`, every face, edge and vertex tolerance |
//! | `Rebuild`  |            | `status`, per edge use whether it has a UV trim curve, every edge tolerance |
//! | `Make`     | `which region v1 v2` (`0` `MakeShellVertexTopology`, `1` `MakeEdgeVertexTopology`, `2` `MakeEdgeTopology`; region `-1` null) | `status`, then the topology read back through fresh list queries: `n_regions`, per region `n_shells` and per shell `kind ref` (`1` vertex shell and its vertex, `2` wire shell and its first wire edge, `3` face-use shell, `0` unknown; ref `-1`), `n_vertices` and per vertex its vertex-use count, `n_edges` and per edge `start end shell shell` (the vertices and shells of the primary edge use and its mate; shells by position in the region-major shell list) |
//! | `Sector`   | `curve t0 t1 orient class type index t u v` | `status region` (`LocalCurveSector`) |
//! | `Merge`    | `face tol n curve*` | `status new_faces edges` (`MergeCurvesOnSurface`) |
//! | `Mate`     | `edge surface ext_factor uv_factor` | `status` (`MateLaminaEdgeToSurface`) |
//! | `Notify`   | `operation ref ref ref` | `0` |
//! | `Stale`    | `create modified` (face `i` gets an `SM_OC_TRIMSRF` cache when bit `i % 8` of `create` is set, `m_bFaceWasModified` from that bit of `modified`) | `status`, per face whether its cache survived (`DeleteStaleCaches`) |
//! | `Swap`     |            | `status` (`SwapUV`) |
//! | `Replace`  | `face surface keep create` (`keep`: `0` delete, `1` deleted list, `2` keep) | `status modified` (`ReplaceSurface`) |
//! | `RayFire`  | `P V tol has_limit limit` | `status region class` (`RayFire`) |
//! | `MateProbe` | as `Mate` | `status max_dist`: `MateLaminaEdgeToSurface`'s `dMaxDist` (`SmBrep.cpp:22200-22229`) after replaying its early returns (`:22166-22195`, status `SM_ERR` when one is taken), nothing changed |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: valid
//! B-rep blocks, indices inside the B-rep (it exports the B-rep once more to count),
//! object references whose type matches the point class for the face, edge and vertex
//! classes (`Sector`; any object for the other classes, which upstream rejects with
//! `SM_ERR` untouched), a null region only when the B-rep has an infinite region
//! (`Make`), a start vertex for the edge makers, finite values. Deliberate
//! restrictions: a `Sector` interval must have `t0 != t1` (an empty one makes the step
//! `0 / 0`, a NaN parameter for the curve evaluator; `SmExtent1d(t0, t1)` swaps an
//! inverted pair, as the runner's `Extent1d::from_min_max`); a `Mate` extension factor must not
//! be negative (a negative extension distance hangs the C++ in `N_SrfExtendByDist`,
//! `plan/upstream-issues.md` HANG-18), and a `Mate` is refused when the `MateProbe`
//! distance is at least `SM_EFF_ZERO` and its product with the factor is not finite, i.e.
//! only when upstream would get past its early returns and extend
//! (an edge whose squared distance to the mating surface overflows: `dMaxDist` is
//! infinite and `CreateExtendedSurface` never stops halving, HANG-9); a B-spline face of degree
//! at least 2 with `init_gaps` set is refused when a corner control point coordinate
//! exceeds `1e154` (the fixture's `GetVertexFaceGap` slows down from `1e155` and
//! stops returning around `1e156`, HANG-28); a reference `type -1` is not accepted (the null
//! reference is `-1 -1`). `Merge` and `Mate` with editing enabled run the whole upstream
//! algorithm: the callers keep their curves short of the tolerance (`Merge`) and pick
//! lamina edges of upstream files (`Mate`).

use crate::{call_with_buffer, FfiError};

use super::topology_vertexuse as vu;

crate::reader::unit_reader!(vu::MAX_COORD);

impl Reader<'_> {
    /// An index below `count` (as `topology_vertexuse`'s reader).
    fn below(&mut self, count: usize) -> Result<usize, FfiError> {
        let i = self.index(usize::MAX >> 12)?;
        if i >= count {
            return Err(bad(format!("index {i} out of range ({count})")));
        }
        Ok(i)
    }

    /// A `topology_vertexuse` block read by `read` (`vu::curve_at`, `vu::surface_at`)
    /// from the current position, which moves past it.
    fn block<T>(
        &mut self,
        read: impl FnOnce(&[f64], usize) -> Result<(T, usize), FfiError>,
    ) -> Result<T, FfiError> {
        let (v, at) = read(self.input, self.at)?;
        self.at = at;
        Ok(v)
    }
}

/// Largest number of curves of a `Merge`.
pub const MAX_CURVES: usize = 8;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Extras = 1,
    Query = 2,
    SetTol = 3,
    Rebuild = 4,
    Make = 5,
    Sector = 6,
    Merge = 7,
    Mate = 8,
    Notify = 9,
    Stale = 10,
    Swap = 11,
    Replace = 12,
    RayFire = 13,
    MateProbe = 14,
}

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p2_eval(
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

/// `SmTopoTypes.h` codes with their export slot.
const TYPES: [(i64, usize); 10] = [
    (16012, 0), // SmRegion_TYPE
    (16013, 1), // SmShell_TYPE
    (16022, 2), // SmFaceuse_TYPE
    (16002, 3), // SmFace_TYPE
    (16004, 4), // SmLoopuse_TYPE
    (16003, 5), // SmLoop_TYPE
    (16005, 6), // SmEdgeuse_TYPE
    (16016, 7), // SmEdge_TYPE
    (16006, 8), // SmVertexuse_TYPE
    (16021, 9), // SmVertex_TYPE
];
/// `SmBrep_TYPE`.
const BREP_TYPE: f64 = 16011.0;
/// `SM_PC_FACE`, `SM_PC_EDGE`, `SM_PC_VERTEX` with the type their object must have.
const CLASS_TYPES: [(f64, f64); 3] = [(102.0, 16002.0), (103.0, 16016.0), (104.0, 16021.0)];

fn int(rd: &mut Reader<'_>, lo: i64, hi: i64) -> Result<i64, FfiError> {
    let v = rd.num()?;
    if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
        return Err(bad(format!("bad integer {v} ({lo}..={hi})")));
    }
    Ok(v as i64)
}

/// `-1` or an index below `n`.
fn opt_below(rd: &mut Reader<'_>, n: usize) -> Result<Option<usize>, FfiError> {
    let v = int(rd, -1, i64::from(u32::MAX))?;
    if v < 0 {
        return Ok(None);
    }
    let v = v as usize;
    if v >= n {
        return Err(bad(format!("index {v} out of range ({n})")));
    }
    Ok(Some(v))
}

/// An entity reference `type index`; returns the type (`-1` null).
fn entity(rd: &mut Reader<'_>, c: &[usize; 10]) -> Result<f64, FfiError> {
    let t = rd.num()?;
    if t == -1.0 {
        int(rd, -1, -1)?;
        return Ok(t);
    }
    if t == BREP_TYPE {
        int(rd, 0, 0)?;
        return Ok(t);
    }
    let &(_, slot) = TYPES
        .iter()
        .find(|(code, _)| *code as f64 == t)
        .ok_or_else(|| bad("bad entity type"))?;
    rd.below(c.get(slot).copied().unwrap_or(0))?;
    Ok(t)
}

fn args(op: Op, rd: &mut Reader<'_>, c: &[usize; 10], has_infinite: bool) -> Result<(), FfiError> {
    let [n_r, _, _, n_f, _, _, _, n_e, _, n_v] = *c;
    match op {
        Op::Export | Op::Extras | Op::Rebuild | Op::Swap => {}
        Op::Stale => {
            int(rd, 0, 255)?;
            int(rd, 0, 255)?;
        }
        Op::Query => {
            rd.index(5)?;
        }
        Op::SetTol => {
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Make => {
            let which = rd.index(2)?;
            if opt_below(rd, n_r)?.is_none() && !has_infinite {
                return Err(bad("null region without infinite region"));
            }
            let v1 = opt_below(rd, n_v)?;
            let v2 = opt_below(rd, n_v)?;
            if which == 2 && (v1.is_none() || v2.is_none()) || which == 1 && v1.is_none() {
                return Err(bad("missing vertex"));
            }
        }
        Op::Sector => {
            if rd.index(3)? != 3 {
                return Err(bad("sector curve must be 3D"));
            }
            rd.at -= 1;
            rd.block(vu::curve_at)?;
            let t0 = rd.finite()?;
            let t1 = rd.finite()?;
            // SmExtent1d(t0, t1) swaps an inverted pair; only an empty one is undefined
            if t0 == t1 {
                return Err(bad("empty sector interval"));
            }
            rd.index(2)?;
            let class = rd.num()?;
            if class.fract() != 0.0 || !(100.0..=110.0).contains(&class) {
                return Err(bad("bad point class"));
            }
            let ty = entity(rd, c)?;
            // only the face / edge / vertex arms cast the object; every other class is
            // upstream's `default: SER(SM_ERR)` and never touches it
            if let Some(&(_, want)) = CLASS_TYPES.iter().find(|&&(k, _)| k == class) {
                if ty != want {
                    return Err(bad("point class and object type disagree"));
                }
            }
            rd.finites(3)?;
        }
        Op::Merge => {
            rd.below(n_f)?;
            rd.finite()?;
            let n = rd.index(MAX_CURVES)?;
            for _ in 0..n {
                if rd.index(3)? != 3 {
                    return Err(bad("merge curves must be 3D"));
                }
                rd.at -= 1;
                rd.block(vu::curve_at)?;
            }
        }
        Op::Mate | Op::MateProbe => {
            rd.below(n_e)?;
            rd.block(vu::surface_at)?;
            // a negative factor is a negative extension distance for
            // CreateExtendedSurface, which never returns (N_SrfExtendByDist; HANG-18)
            if rd.finite()? < 0.0 {
                return Err(bad("negative surface extension factor"));
            }
            rd.finite()?;
        }
        Op::Notify => {
            rd.index(16)?;
            for _ in 0..3 {
                entity(rd, c)?;
            }
        }
        Op::Replace => {
            rd.below(n_f)?;
            rd.block(vu::surface_at)?;
            rd.index(2)?;
            rd.flag()?;
        }
        Op::RayFire => {
            rd.vec3()?;
            rd.finites(3)?;
            rd.finite()?;
            rd.flag()?;
            rd.finite()?;
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_topology_brep_p2_eval(
                op as i32,
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

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader::new(input);
    rd.at = vu::brep_block_at(input, path)?;
    int(&mut rd, -1, 1)?;
    let head = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    gap_guard(head)?;
    let export = raw(Op::Export, path, head)?;
    if op == Op::Export {
        rd.done()?;
        return Ok(export);
    }
    let Some(c) = vu::counts(&export) else {
        // the B-rep could not be built: every op reports the same failure
        return Ok(export);
    };
    let extras = raw(Op::Extras, path, head)?;
    let has_infinite = extras.get(2).is_some_and(|&r| r >= 0.0);
    args(op, &mut rd, &c, has_infinite)?;
    rd.done()?;
    if op == Op::Mate {
        mate_guard(path, input)?;
    }
    raw(op, path, input)
}

/// Largest corner control point coordinate of a degree >= 2 B-spline face the fixture's gap
/// initialisation is run on (`plan/upstream-issues.md` HANG-28). With a corner (a vertex
/// position) far out, `SmVertexuse::GetVertexFaceGap` slows down and then stops returning.
/// Measured on the pod with 30 s timeouts (u-degree 2 and 3, 3 and 6 spans, v-degree 1 and
/// 2, the last corner raised on x, y and z): every shape returns in at most 0.09 s up to
/// `1.34e154` (about `sqrt(f64::MAX)`, where squared coordinates overflow); from `1e155`
/// the v-degree-1 shapes take 0.3-1.1 s; at `1e156` six do not return within 30 s (five on
/// x or y, one on z) and at `1.9e156` nine (every v-degree-1 shape but the six-span
/// u-degree-3 one); the review of d12f88e9 saw 103-114 s on z and no return within 120 s on
/// x. The bound keeps a margin below that slow band (the first bound, `2e156`, was inside
/// it). Planes, degree-1 surfaces and interior control points return even at `1e300`, and
/// the v-degree-2 shapes measured stay fast at `1.9e156`; the exact loop is not pinned down
/// (the pod cannot ptrace), so every degree >= 2 face with a corner past the bound is
/// refused.
const MAX_GAP_COORD: f64 = 1e154;

/// [`MAX_GAP_COORD`] for a rational net, applied both to the stored (homogeneous) corner
/// coordinate `w c` and to the Cartesian one `c`. Measured on the pod with 30 s timeouts
/// (u-degree 2 / 3, 3 / 6 spans, v-degree 1 / 2, x, y and z, corner weights `1e-3` to
/// `1e3`, the other weights `1e-3`, `1` and `1e3`): a rational corner with a large
/// coordinate costs a bounded 0.6-3.9 s on x (at `1e100` as at `1e151`; y and z stay below
/// 0.1 s), while a stored coordinate past `sqrt(f64::MAX)` can stop returning (Cartesian
/// `9.9e153` with weight 10, stored `9.9e154`, on y). The other weights do not change
/// either. The `1e3` factor below [`MAX_GAP_COORD`] is the margin.
const MAX_GAP_COORD_RATIONAL: f64 = 1e151;

/// The weight of a non-rational control point (`NOW`, as `topology_vertexuse` reads it).
const NOW: f64 = -f64::MAX;

/// Refuses a B-spline face block (`kind 2`, surface kind `0`) with `init_gaps` set, a
/// degree of at least 2 in u or v and a corner too far out: a non-rational corner
/// coordinate above [`MAX_GAP_COORD`], or for a rational net a corner whose stored
/// (homogeneous) or Cartesian coordinate is above [`MAX_GAP_COORD_RATIONAL`]. The net is
/// homogeneous (`w x, w y, w z, w`; `w = NOW` for a non-rational net) and the corner
/// control point is the vertex position, so a small weight moves the vertex out (`1e154`
/// stored with `w = 1e-3` is a vertex at `1e157`, which does not return) and a large one
/// moves the stored value out; `|w c| <= R` and `|c| <= R` is `|w c| <= R * min(1, w)`.
/// `head` is the validated B-rep block plus `editing`, so `init_gaps` is its
/// second-to-last value.
fn gap_guard(head: &[f64]) -> Result<(), FfiError> {
    let init_gaps = head.len().checked_sub(2).and_then(|i| head.get(i)) == Some(&1.0);
    if head.first() != Some(&2.0) || head.get(1) != Some(&0.0) || !init_gaps {
        return Ok(());
    }
    let (Some(&n), Some(&m), Some(&p), Some(&q)) =
        (head.get(4), head.get(5), head.get(6), head.get(7))
    else {
        return Err(bad("surface header too short"));
    };
    if p < 2.0 && q < 2.0 {
        return Ok(());
    }
    let (n, m) = (n as usize, m as usize);
    for (a, b) in [(0, 0), (0, m), (n, 0), (n, m)] {
        let at = 10 + 4 * (a * (m + 1) + b);
        let (Some(xyz), Some(&w)) = (head.get(at..at + 3), head.get(at + 3)) else {
            return Err(bad("surface net too short"));
        };
        // the reader bounds a rational weight to [1e-3, 1e3]
        let bound = if w == NOW {
            MAX_GAP_COORD
        } else {
            MAX_GAP_COORD_RATIONAL * w.min(1.0)
        };
        if xyz.iter().any(|&c| c != f64::MAX && c.abs() > bound) {
            return Err(bad("face too large for the gap initialisation"));
        }
    }
    Ok(())
}

/// Refuses a `Mate` whose extension distance `dMaxDist * dSurfaceExtensionFactor`
/// would not be finite (upstream returns before extending when editing is off, the
/// edge or a neighbour is not lamina/seam, the copy or distance computation fails, or
/// `dMaxDist < SM_EFF_ZERO`, so those run and their statuses are compared).
fn mate_guard(path: &str, input: &[f64]) -> Result<(), FfiError> {
    let probe = raw(Op::MateProbe, path, input)?;
    let (Some(&status), Some(&max_dist)) = (probe.get(1), probe.get(2)) else {
        return Err(bad("short MateProbe output"));
    };
    let factor = input
        .get(input.len().wrapping_sub(2))
        .copied()
        .ok_or_else(|| bad("input too short"))?;
    if status == 0.0 && max_dist >= 1e-12 && !(max_dist * factor).is_finite() {
        return Err(bad("non-finite surface extension distance"));
    }
    Ok(())
}
