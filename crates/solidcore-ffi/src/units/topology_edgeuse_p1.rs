//! FFI wrappers for unit `topology.edgeuse_p1`.
//!
//! Shim: `harness/cpp/shim/topology_edgeuse_p1.cpp` (`sc_topology_edgeuse_p1_eval`).
//! Upstream: `source/SMLib/src/SmEdgeuse.cpp`, `source/SMLib/src/SmFaceuse.cpp`.
//!
//! B-rep ops ([`Op::is_brep`]) start with a B-rep block (the shim builds a fresh
//! `SmBrep` per call and deletes it after):
//!
//! | block                                         | B-rep                                     |
//! |-----------------------------------------------|-------------------------------------------|
//! | `0 rebuild validate_only init_gaps`           | `ReadFromFile(path, SM_ASCII, rebuild)`   |
//! | `1 curve t0 t1 validate_only init_gaps`       | `CreateWireEdgeFromCurve(curve, [t0, t1])`|
//! | `2 surface u0 v0 u1 v1 validate_only init_gaps` | `CreateFaceFromSurface(surface, dom)`   |
//! | `3 validate_only init_gaps`                   | the wire network of `prog_test` `my_test_topology` |
//! | `4 n points(x y z)*n validate_only init_gaps` | actual `MakeShellVertex` calls (0..64 vertices) |
//! | `5 has_face [surface u0 v0 u1 v1] networks validate_only init_gaps` | an optional face (as kind 2) and `networks` (0..=3) copies of the wire network in one B-rep |
//! | `6 surface u0 v0 u1 v1 validate_only init_gaps` | as kind 2 on a B-spline surface, every edge curve then an `SmCrvOnSurf` over its UV trim curve |
//! | `7 origin(x y z) length width height validate_only init_gaps` | the closed box solid of `SmApiCreateBox` (`SmPrimitiveCreation::CreateBox`; origin coordinates up to `1e4`, sizes in `1e-3..=1e4`) |
//! | `8 nv (x y z tol)*nv ne (x0 y0 z0 x1 y1 z1 tol)*ne validate_only init_gaps` | `MakeShellVertex` points and `CreateWireEdgeFromCurve` line segments, each with `SetTolerance(tol, FALSE, FALSE)` (up to [`MAX_POINTS`] vertices and [`MAX_LINES`] segments, tolerances in `0..=1e3`, segments longer than `1e-9`) |
//!
//! Kind `9` builds a native planar ellipse/circle face with exact rational UV trims:
//! `9 circle origin[3] axis rx ry angles[2] natural[2] inside_out oob validate_only init_gaps`.
//! An arc closes with a chord; `axis` is the omitted coordinate (0, 1 or 2).
//! Radii are `1e-3..=1e4`, origin magnitudes at most `1e4`, angular spans `1..=360`
//! degrees and NURB bounds within `[-1e4, 1e4]` with span at least `1e-3`.
//!
//! A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (`SmBSplineSurface`) or
//! `1 O[3] N[3]` (`SmPlane(O, N)`); a curve block `dim n p m cpts(x y z w) knots`
//! (`SmBSplineCurve`). Indices refer to the export's traversal order. Every B-rep op's
//! output starts with `0`, or with `-1 status` when the B-rep could not be built
//! (nothing else follows). `-2` in an output is "upstream would dereference null"
//! (the shim did not call it).
//!
//! | [`Op`]      | parameters after the block              | outputs |
//! |-------------|-----------------------------------------|---------|
//! | `Export`    |                                         | the graph (see [`counts`] and the difftest runner) |
//! | `EuQuery`   | `eu vertex`                             | `edge orient cw ccw has_ef has_ccw ef_init loop shell_eu vu lu fu face shell brep tol mate radial strut has_uv uv_owner has_fail fail_owner own_uv corner type kind_of*9` |
//! | `Connected` | `eu type index`                         | `connected` |
//! | `FuQuery`   | `fu tgt_only has_loops n loops*n`       | `shell region face mate orient type kind_of*9`, then lists: loop uses, edge uses, neighbours, neighbours skipping lamina edges |
//! | `Collect`   | `fu has_threshold threshold`            | `status` list |
//! | `UvLoopGap` | `eu`                                    | `gap` |
//! | `SetUv`     | `eu kind [curve] dmax delete has_fail fail_status` | `0` or `-2`, then the edge use's and its mate's state, the edge tolerance, the argument record's B-rep flag |
//! | `Gaps`      | `eu which flag want_out`                | per `which` (`0` `GetMaxEdgeFaceGap`, `1` `GetEdgeCCWEdgeGap`, `2` `CalcMaxEdgeFaceGap`, `3` `CalcEdgeCCWEdgeGap`) |
//! | `SetOrient` | `eu orientation`                        | `orient mate radial` |
//! | `CacheStates` |                                       | `n state*n`: per face (export order), its surface's cache slot: `0` none, `1` a cache whose tree is not built, `2` a cache whose tree is built ([`cache_states`]) |
//!
//! The graph export ends with a versioned extension: `3 infinite_region`, then
//! one raw void flag per region and one raw wire-edge-use/vertex-use head index
//! per shell (`-1` for other member classes), then a vertex-use index list per
//! vertex in native list order, then literal next/last indices per face use.
//! The original graph prefix and the
//! ten entity counts remain unchanged. This supplies shell state independently
//! of the shell operations being compared.
//!
//! A face's surface in the graph export may start with an exact analytic head,
//! `2 type version surf_oob gen_oob nbytes byte*nbytes` (an `SmSurfOfRevolution`: its
//! binary `WriteToDB` record, `harness/cpp/shim/sc_analytic_export.h`), followed by the
//! regular surface block (its B-spline part); other face surfaces have the regular
//! block only.
//!
//! Ellipse/circle edge blocks may start with `-3 type version dim oob nbytes byte*nbytes`,
//! followed by their independent B-spline block. The native record preserves the
//! stored NURB and its STEP mapping; UV trim curves and edge intervals are unchanged.
//! The reader checks the concrete kind, complete record consumption and base bits.
//!
//! `TrimSample` takes `eu fraction` (`0..=1`) and returns a status followed, on success,
//! by `curve_type edge_interval[2] trim_interval[2] t xyz[3] uv[2] lifted_xyz[3]`.
//! Both curves are evaluated at the same edge NURB parameter; `lifted_xyz` is the
//! face surface evaluated at the retained UV trim point.
//!
//! Geometry ops: `StepOff` (`surface dist P[3] N[3] u v jac` -> `status found F0 F1
//! J00 J01 J10 J11`), `DistDev` (`surface P[3] u v value obj want_dev` -> `dist dev`;
//! `obj` 0 null, 1 the surface, 2 a line), `Seam` (`n curve*n a b called` -> `seam`).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: existing
//! `.smb` files, well-formed clamped B-splines, planes with a non-zero normal,
//! parameters within 100 domain lengths of a surface's domain, plane-face domains
//! bounded by 1e6 for graph export (1e4 for other operations), indices inside the
//! B-rep's export (it builds the B-rep once more to count), finite inputs.

use std::path::Path;

use crate::validate::{check_knots, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e300;
/// Largest number of curves for `Seam` and of target loops for `FuQuery`.
pub const MAX_LIST: usize = 64;
/// Largest vertex count of a kind 8 block.
pub const MAX_POINTS: usize = 600;
/// Largest segment count of a kind 8 block.
pub const MAX_LINES: usize = 150;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    EuQuery = 1,
    Connected = 2,
    FuQuery = 3,
    Collect = 4,
    UvLoopGap = 5,
    SetUv = 6,
    Gaps = 7,
    StepOff = 8,
    DistDev = 9,
    Seam = 10,
    SetOrient = 11,
    CacheStates = 12,
    TrimSample = 13,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }

    /// True for the ops starting with a B-rep block.
    #[must_use]
    pub fn is_brep(self) -> bool {
        !matches!(self, Op::StepOff | Op::DistDev | Op::Seam)
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_edgeuse_p1_eval(
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

/// Sequential reader over the input (shared with unit `topology.edgeuse_p2`).
pub(crate) struct Reader<'a> {
    pub(crate) input: &'a [f64],
    pub(crate) at: usize,
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

    pub(crate) fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    pub(crate) fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    pub(crate) fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
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

    /// An index below `count`.
    pub(crate) fn below(&mut self, count: usize) -> Result<usize, FfiError> {
        let i = self.index(usize::MAX >> 12)?;
        if i >= count {
            return Err(bad(format!("index {i} out of range ({count})")));
        }
        Ok(i)
    }

    /// An index below `count` or `-1`.
    pub(crate) fn below_or_none(&mut self, count: usize) -> Result<(), FfiError> {
        let v = self.num()?;
        if v == -1.0 {
            return Ok(());
        }
        self.at -= 1;
        self.below(count).map(|_| ())
    }

    pub(crate) fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    pub(crate) fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
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
pub(crate) type Domain = Option<[f64; 4]>;

/// A surface block; returns its domain.
pub(crate) fn surface(rd: &mut Reader<'_>) -> Result<Domain, FfiError> {
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
pub(crate) fn curve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    curve_up_to(rd, MAX_INDEX)
}

/// A B-spline curve block with at most `max_index + 1` control points; returns its
/// parameter range.
pub(crate) fn curve_up_to(rd: &mut Reader<'_>, max_index: usize) -> Result<(f64, f64), FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    let n = rd.index(max_index)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(max_index + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, dim == 2, "curve")?;
    let knots = rd.finites(m + 1)?;
    check_knots(&knots, p, "curve")
}

/// A `uv` point within 100 domain lengths of `dom` (anywhere bounded for a plane).
fn uv(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    if let Some([u0, v0, u1, v1]) = dom {
        let (du, dv) = (u1 - u0, v1 - v0);
        if u < u0 - 100.0 * du || u > u1 + 100.0 * du || v < v0 - 100.0 * dv || v > v1 + 100.0 * dv
        {
            return Err(bad("uv too far outside the domain"));
        }
    } else if u.abs() > 1e6 || v.abs() > 1e6 {
        return Err(bad("plane uv too large"));
    }
    Ok(())
}

/// A B-rep block.
pub(crate) fn brep_block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    brep_block_with_plane_limit(rd, path, 1e4)
}

/// The surface and parameter domain of a face block (`2`, `6`, and `5` with a face):
/// whether the surface is a B-spline surface (not a plane).
pub(crate) fn face_block(rd: &mut Reader<'_>, plane_domain_limit: f64) -> Result<bool, FfiError> {
    let dom = surface(rd)?;
    let d = rd.finites(4)?;
    let (Some(&a), Some(&b2), Some(&c), Some(&e)) = (d.first(), d.get(1), d.get(2), d.get(3))
    else {
        return Err(bad("domain too short"));
    };
    if a >= c || b2 >= e {
        return Err(bad("face domain not ordered"));
    }
    match dom {
        Some([u0, v0, u1, v1]) => {
            if a < u0 || c > u1 || b2 < v0 || e > v1 {
                return Err(bad("face domain outside the surface's"));
            }
        }
        None => {
            if d.iter().any(|x| x.abs() > plane_domain_limit) {
                return Err(bad("plane face domain too large"));
            }
        }
    }
    Ok(dom.is_some())
}

/// Validates the fixed-size native ellipse-face fixture block (after kind 9).
/// Shared with the vertex-use export protocol, which uses its own reader.
pub(crate) fn ellipse_face_block(input: &[f64]) -> Result<usize, FfiError> {
    let mut rd = Reader { input, at: 0 };
    let circle = rd.flag()?;
    for _ in 0..3 {
        if rd.finite()?.abs() > 1e4 {
            return Err(bad("ellipse face: origin too far"));
        }
    }
    rd.index(2)?;
    let rx = rd.finite()?;
    let ry = rd.finite()?;
    if !(1e-3..=1e4).contains(&rx) || !(1e-3..=1e4).contains(&ry) || (circle && rx != ry) {
        return Err(bad("ellipse face: invalid radii"));
    }
    let a = rd.finite()?;
    let b = rd.finite()?;
    if a.abs() > 360.0 || b.abs() > 720.0 || !(1.0..=360.0).contains(&(b - a)) {
        return Err(bad("ellipse face: invalid angular interval"));
    }
    let a = rd.finite()?;
    let b = rd.finite()?;
    if a.abs() > 1e4 || b.abs() > 1e4 || b - a < 1e-3 {
        return Err(bad("ellipse face: invalid NURB interval"));
    }
    rd.flag()?;
    rd.flag()?;
    Ok(rd.at)
}

fn brep_block_with_plane_limit(rd: &mut Reader<'_>, path: &str, plane_domain_limit: f64) -> Result<(), FfiError> {
    match rd.index(9)? {
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
            face_block(rd, plane_domain_limit)?;
        }
        6 => {
            // kind 6 (edges as `SmCrvOnSurf`) needs an `SmBSplineSurface` base
            if !face_block(rd, plane_domain_limit)? {
                return Err(bad("CrvOnSurf face: the base must be a B-spline surface"));
            }
        }
        5 => {
            if rd.flag()? {
                face_block(rd, plane_domain_limit)?;
            }
            rd.index(3)?;
        }
        4 => {
            let count = rd.index(MAX_LIST)?;
            for _ in 0..count {
                rd.vec3()?;
            }
        }
        8 => {
            let tol = |rd: &mut Reader<'_>| -> Result<(), FfiError> {
                if !(0.0..=1e3).contains(&rd.finite()?) {
                    return Err(bad("points/lines: tolerance out of range"));
                }
                Ok(())
            };
            for _ in 0..rd.index(MAX_POINTS)? {
                rd.vec3()?;
                tol(rd)?;
            }
            for _ in 0..rd.index(MAX_LINES)? {
                let a = rd.vec3()?;
                let b = rd.vec3()?;
                let d = a.iter().zip(&b).map(|(p, q)| (q - p) * (q - p)).sum::<f64>();
                if d.sqrt() <= 1e-9 {
                    return Err(bad("points/lines: degenerate segment"));
                }
                tol(rd)?;
            }
        }
        9 => {
            rd.at += ellipse_face_block(rd.input.get(rd.at..).ok_or_else(|| bad("short ellipse face"))?)?;
        }
        7 => {
            for _ in 0..3 {
                if rd.finite()?.abs() > 1e4 {
                    return Err(bad("box: origin too far"));
                }
            }
            for _ in 0..3 {
                if !(1e-3..=1e4).contains(&rd.finite()?) {
                    return Err(bad("box: size out of range"));
                }
            }
        }
        _ => {}
    }
    rd.flag()?;
    rd.flag()?;
    Ok(())
}

/// The entity counts of an export, in the shim's order: regions, shells, face uses,
/// faces, loop uses, loops, edge uses, edges, vertex uses, vertices. `None` when the
/// export is a build failure.
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

/// The per-face cache slot states of a `CacheStates` output (after the build status
/// `0`): `0` no cache, `1` a cache whose tree is not built, `2` a cache whose tree is
/// built, in export face order. `None` for a failed build or a malformed output.
#[must_use]
pub fn cache_states(out: &[f64]) -> Option<Vec<u8>> {
    let (status, rest) = out.split_first()?;
    if *status != 0.0 {
        return None;
    }
    let (n, states) = rest.split_first()?;
    if n.fract() != 0.0 || *n < 0.0 || *n > states.len() as f64 || (*n as usize) != states.len() {
        return None;
    }
    states
        .iter()
        .map(|&v| {
            if v == 0.0 {
                Some(0)
            } else if v == 1.0 {
                Some(1)
            } else if v == 2.0 {
                Some(2)
            } else {
                None
            }
        })
        .collect()
}

/// `SmTopoTypes.h` codes `IsConnectedTo` dispatches on, with the export slot of their
/// kind.
const TARGET_TYPES: [(i64, usize); 10] = [
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

/// Checks the arguments after the B-rep block against the B-rep's counts.
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let [_, _, n_fu, _, _, n_loop, n_eu, _, _, n_v] = c;
    match op {
        Op::Export | Op::CacheStates => {}
        Op::EuQuery => {
            rd.below(n_eu)?;
            rd.below_or_none(n_v)?;
        }
        Op::Connected => {
            rd.below(n_eu)?;
            let t = rd.num()?;
            let i = rd.num()?;
            if i.fract() != 0.0 || i < -1.0 {
                return Err(bad("bad target index"));
            }
            if let Some(&(_, slot)) = TARGET_TYPES.iter().find(|(code, _)| *code as f64 == t) {
                let n = c.get(slot).copied().unwrap_or(0);
                if i >= 0.0 && i as usize >= n {
                    return Err(bad("target index out of range"));
                }
            } else if t.fract() != 0.0 || !(0.0..=1.0e6).contains(&t) {
                return Err(bad("bad target type"));
            }
        }
        Op::FuQuery => {
            rd.below(n_fu)?;
            rd.flag()?;
            rd.flag()?;
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.below(n_loop)?;
            }
        }
        Op::Collect => {
            rd.below(n_fu)?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::TrimSample => {
            rd.below(n_eu)?;
            if !(0.0..=1.0).contains(&rd.finite()?) { return Err(bad("trim sample outside edge")); }
        }
        Op::UvLoopGap => {
            rd.below(n_eu)?;
        }
        Op::SetUv => {
            rd.below(n_eu)?;
            let kind = rd.index(4)?;
            if kind == 2 || kind == 4 {
                curve(rd)?;
            }
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
            let s = rd.num()?;
            if s.fract() != 0.0 || s.abs() > 1.0e6 {
                return Err(bad("bad fail status"));
            }
        }
        Op::Gaps => {
            rd.below(n_eu)?;
            rd.index(3)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::SetOrient => {
            rd.below(n_eu)?;
            rd.index(2)?;
        }
        Op::StepOff | Op::DistDev | Op::Seam => return Err(bad("not a B-rep op")),
    }
    Ok(())
}

/// Checks a geometry op's input.
fn geom_args(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::StepOff => {
            let dom = surface(rd)?;
            rd.finite()?;
            rd.vec3()?;
            rd.vec3()?;
            uv(rd, dom)?;
            rd.flag()?;
        }
        Op::DistDev => {
            let dom = surface(rd)?;
            rd.vec3()?;
            uv(rd, dom)?;
            rd.finite()?;
            rd.index(2)?;
            rd.flag()?;
        }
        Op::Seam => {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                curve(rd)?;
            }
            let a = rd.finite()?;
            let b = rd.finite()?;
            if a > b {
                return Err(bad("edge domain not ordered"));
            }
            rd.flag()?;
        }
        _ => return Err(bad("not a geometry op")),
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_topology_edgeuse_p1_eval(
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

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    if !op.is_brep() {
        geom_args(op, &mut rd)?;
        rd.done()?;
        return raw(op, path, input);
    }
    // Export constructs the graph but does not run an edge-use geometry
    // operation. Actual native shell probes cover plane domains through 1e6;
    // retain the existing 1e4 bound for the other edge-use operations.
    let plane_domain_limit = if matches!(op, Op::Export | Op::CacheStates) {
        1e6
    } else {
        1e4
    };
    brep_block_with_plane_limit(&mut rd, path, plane_domain_limit)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = raw(Op::Export, path, block)?;
    if op == Op::Export {
        rd.done()?;
        return Ok(export);
    }
    let Some(c) = counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    brep_args(op, &mut rd, c)?;
    rd.done()?;
    raw(op, path, input)
}

/// An immutable fixture block and the genuine native export used to validate it.
///
/// Construct once for one case, then use it with the edge-use p2/p3 prepared
/// entry points. Each operation still rebuilds and destroys its own native body;
/// only an export is retained. Fields are private so callers cannot supply counts
/// or an unchecked export. File-backed fixtures must remain unchanged during a case.
#[derive(Debug)]
pub(crate) struct PreparedBrep {
    path: String,
    block: Vec<f64>,
    export: Vec<f64>,
}

impl PreparedBrep {
    /// Validate and export exactly this B-rep block through the ordinary native API.
    ///
    /// # Errors
    /// The same malformed-block and native errors as [`eval`] with [`Op::Export`].
    pub(crate) fn new(path: &str, block: &[f64]) -> Result<Self, FfiError> {
        let export = eval(Op::Export, path, block)?;
        Ok(Self {
            path: path.to_owned(),
            block: block.to_vec(),
            export,
        })
    }

    /// The original immutable fixture path.
    #[must_use]
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// The original immutable fixture block, compared bit for bit by runner memos.
    #[must_use]
    pub(crate) fn block(&self) -> &[f64] {
        &self.block
    }

    /// The native export produced by construction (including a build-failure record).
    #[must_use]
    pub(crate) fn export(&self) -> &[f64] {
        &self.export
    }
}

#[cfg(test)]
mod prepared_tests {
    use super::super::{topology_edgeuse_p2 as p2, topology_edgeuse_p3 as p3};
    use super::*;

    #[test]
    fn ellipse_face_guards_reject_malformed_input() {
        let valid = [0.0, 1.0, 2.0, 3.0, 2.0, 3.0, 1.0, 35.0, 245.0, 2.0, 7.0, 0.0, 1.0];
        assert_eq!(ellipse_face_block(&valid).unwrap(), valid.len());
        for end in 0..valid.len() { assert!(ellipse_face_block(&valid[..end]).is_err()); }
        for i in 0..valid.len() {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut bad = valid;
                bad[i] = value;
                assert!(ellipse_face_block(&bad).is_err(), "slot {i}");
            }
        }
        for (i, value) in [(0, 1.0), (4, 3.0), (5, 0.0), (6, -1.0), (8, 34.0), (8, 396.0), (10, 2.0), (11, 2.0)] {
            let mut bad = valid;
            bad[i] = value;
            assert!(ellipse_face_block(&bad).is_err(), "slot {i}");
        }
    }

    fn bits(r: Result<Vec<f64>, FfiError>) -> Result<Vec<u64>, String> {
        r.map(|v| v.into_iter().map(f64::to_bits).collect())
            .map_err(|e| format!("{e:?}"))
    }

    fn same(block: &[f64], tail: &[f64]) {
        let prepared = p2::PreparedBrep::new("", block).unwrap();
        let mut input = block.to_vec();
        input.extend_from_slice(tail);
        assert_eq!(
            bits(p2::eval(p2::Op::Tangent, "", &input)),
            bits(p2::eval_prepared(p2::Op::Tangent, &prepared, tail))
        );
    }

    #[test]
    fn prepared_validation_matches_default_and_owns_exact_block_bits() {
        let block = [3.0, 0.0, 0.0];
        same(&block, &[0.0, 1.0, 0.0, 0.0]);
        for tail in [
            vec![],
            vec![-1.0],
            vec![0.5],
            vec![f64::NAN],
            vec![1e30],
            vec![10000.0],
            vec![0.0, 1.0, 2.0, 0.0],
            vec![0.0, f64::INFINITY, 0.0, 0.0],
            vec![0.0, 1.0, 0.0, 0.0, 99.0],
        ] {
            same(&block, &tail);
        }
        // A genuine empty graph still rejects every selector, as ordinary eval does.
        same(&[4.0, 0.0, 0.0, 0.0], &[0.0, 1.0, 0.0, 0.0]);
        let mut path = String::from("fixture");
        let owned_path = PreparedBrep::new(&path, &block).unwrap();
        path.clear();
        assert_eq!(owned_path.path(), "fixture");
        let mut source = vec![3.0, -0.0, 0.0];
        let prepared = PreparedBrep::new("", &source).unwrap();
        source[1] = 1.0;
        assert_eq!(prepared.block()[1].to_bits(), (-0.0f64).to_bits());
        let positive = PreparedBrep::new("", &[3.0, 0.0, 0.0]).unwrap();
        assert_ne!(prepared.block()[1].to_bits(), positive.block()[1].to_bits());
        for block in [
            vec![],
            vec![3.0],
            vec![3.0, 2.0, 0.0],
            vec![3.0, 0.0, 0.0, 1.0],
            vec![f64::NAN],
        ] {
            assert_eq!(
                bits(eval(Op::Export, "", &block)),
                bits(PreparedBrep::new("", &block).map(|p| p.export().to_vec()))
            );
        }
    }

    #[test]
    fn prepared_operations_rebuild_independent_bodies_and_validate_p3_arguments() {
        let block = [
            2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0,
        ];
        let prepared = p3::PreparedBrep::new("", &block).unwrap();
        let tail = [0.0, 0.5, 0.0, 1.0, 0.0];
        let mut input = block.to_vec();
        input.extend(tail);
        let original = bits(p3::eval(p3::Op::Normalized, "", &input));
        assert_eq!(
            bits(p3::eval_prepared(p3::Op::Normalized, &prepared, &tail)),
            original
        );
        // GetOrCreate may create a UV curve, but no native body survives the call.
        let create = [0.0, 1.0, 1.0, 1.0];
        let mut create_input = block.to_vec();
        create_input.extend(create);
        assert_eq!(
            bits(p3::eval_prepared(p3::Op::GetOrCreate, &prepared, &create)),
            bits(p3::eval(p3::Op::GetOrCreate, "", &create_input))
        );
        assert_eq!(
            bits(p3::eval_prepared(p3::Op::Normalized, &prepared, &tail)),
            original
        );
        for tail in [
            vec![],
            vec![-1.0],
            vec![0.0, f64::NAN, 0.0, 1.0, 0.0],
            vec![0.0, 0.5, 0.0, 2.0, 0.0],
            vec![0.0, 0.5, 0.0, 1.0, 0.0, 9.0],
        ] {
            let mut input = block.to_vec();
            input.extend_from_slice(&tail);
            assert_eq!(
                bits(p3::eval(p3::Op::Normalized, "", &input)),
                bits(p3::eval_prepared(p3::Op::Normalized, &prepared, &tail))
            );
        }
    }

    #[test]
    fn prepared_file_change_refuses_stale_index_and_missing_file() {
        // Each test invocation owns one unique scratch directory; only copies change.
        let dir = std::env::temp_dir().join(format!("solidcore-prepared-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("fixture.smb");
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../upstream/TestFiles/pt_TestFiles");
        std::fs::copy(root.join("Mouse.smb"), &file).unwrap();
        let path = file.to_str().unwrap();
        let block = [0.0, 0.0, 0.0, 0.0];
        let prepared = p2::PreparedBrep::new(path, &block).unwrap();
        let prepared_p3 = p3::PreparedBrep::new(path, &block).unwrap();
        let old_count = counts(prepared.export()).unwrap()[6];
        std::fs::copy(root.join("plane_square.smb"), &file).unwrap();
        let current = eval(Op::Export, path, &block).unwrap();
        let new_count = counts(&current).unwrap()[6];
        assert!(old_count > new_count);
        let tail = [(old_count - 1) as f64, 1.0, 0.0, 0.0];
        assert!(matches!(
            p2::eval_prepared(p2::Op::Tangent, &prepared, &tail),
            Err(FfiError::Status(crate::SC_ERR_ARG))
        ));
        let tail = [(old_count - 1) as f64, 0.5, 0.0, 1.0, 0.0];
        assert!(matches!(
            p3::eval_prepared(p3::Op::Normalized, &prepared_p3, &tail),
            Err(FfiError::Status(crate::SC_ERR_ARG))
        ));
        // A missing file is still refused by the ordinary block parser, before
        // the prepared export or operation selector can be used.
        std::fs::remove_file(&file).unwrap();
        let tail = [f64::NAN];
        let mut input = block.to_vec();
        input.extend(tail);
        assert_eq!(
            bits(p2::eval(p2::Op::Tangent, path, &input)),
            bits(p2::eval_prepared(p2::Op::Tangent, &prepared, &tail))
        );
        std::fs::remove_dir(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn prepared_native_open_failure_keeps_original_early_return() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "solidcore-prepared-unreadable-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("fixture.smb");
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../upstream/TestFiles/pt_TestFiles");
        std::fs::copy(root.join("plane_square.smb"), &file).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o0)).unwrap();
        if std::fs::File::open(&file).is_ok() {
            // A privileged runner may ignore mode bits; do not pretend its native
            // open-failure path was exercised, and never use malformed contents.
            eprintln!("native open-failure branch not exercised: privileged file access");
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::remove_file(file).unwrap();
            std::fs::remove_dir(dir).unwrap();
            return;
        }
        let path = file.to_str().unwrap();
        let block = [0.0, 0.0, 0.0, 0.0];
        let prepared2 = p2::PreparedBrep::new(path, &block).unwrap();
        let prepared3 = p3::PreparedBrep::new(path, &block).unwrap();
        assert!(counts(prepared2.export()).is_none());
        assert!(counts(prepared3.export()).is_none());
        let tail = [f64::NAN];
        let mut input = block.to_vec();
        input.extend(tail);
        // The failed build is returned before operation arguments are inspected.
        assert_eq!(
            bits(p2::eval(p2::Op::Tangent, path, &input)),
            bits(p2::eval_prepared(p2::Op::Tangent, &prepared2, &tail))
        );
        assert_eq!(
            bits(p3::eval(p3::Op::Normalized, path, &input)),
            bits(p3::eval_prepared(p3::Op::Normalized, &prepared3, &tail))
        );
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
