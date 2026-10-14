//! FFI wrappers for unit `surface.surface_cache_p3`.
//!
//! Shim: `harness/cpp/shim/surface_surface_cache_p3.cpp` (`sc_surface_surface_cache_p3_eval`).
//! Upstream: `source/SMLib/src/SmSurfaceCache.cpp`.
//!
//! Notation as in [`super::surface_surface_cache_p2`]: `S` a surface `kind ..` (`0 B`:
//! B-spline; `1 o x y sx sy D`: bounded `SmPlane`; `2 B`: `SmCurveBoundedSurface` over the
//! B-spline), `N` a bare NURB `n m p q r s cpts u v`, `V` a vector, `D` a domain `umin vmin
//! umax vmax`, `X` a box `min max`, `O` the cache options `chord angtol_rad aspect max_side
//! min_side min_ratio start_fast_angle_deg`. A start node `T` is `aux D X class chsat angsat
//! surf [aux 3: N basis(V V V) lo0 hi0 lo1 hi1 lo2 hi2 uc0 uc1 vc0 vc1]` (`surf` puts the
//! cache's surface on the node).
//!
//! | [`Op`]       | upstream                                         | parameters                 |
//! |--------------|--------------------------------------------------|----------------------------|
//! | `SubdivNode` | `SubdivideNode` on a hand-built start node        | `S O T dir_v only`         |
//! | `SubdivTol`  | `SubdivideToTolerances` on a hand-built start node | `S O T`                   |
//! | `Find`       | `SubdivideNode` per listed split, then `FindUVNodes` | `S O T ns (node dir_v only)x ns D poles` |
//! | `Build`      | `BuildTree(NULL)` on a new cache                  | `S O`                      |
//! | `Get`        | `GetTree()` on a new cache                        | `S O`                      |
//!
//! The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`; for `Get`, `1` for
//! a NULL tree) and the op's outputs (see the shim). [`eval`] accepts only what upstream
//! handles without undefined behaviour or unbounded work: well-formed clamped surfaces, planes
//! with independent axes and moderate scales; tolerances that keep the subdivision bounded
//! (as for `SubdivideByBlock` in part 2); a start node that is an `SM_AD_BEZIER_SURFACE` node
//! with its Bezier patch (a single span whose domain is the node's domain; bicubic unless
//! the cache surface is a B-spline: upstream approximates into its memory) or an
//! `SM_AD_AUX_DATA` node (holding the cache's surface only over a B-spline: upstream casts
//! it); a pseudo box basis of independent unit vectors with ordered intervals; valid
//! enumerators; splits of existing leaves only (upstream asserts). `SubdivTol` starts from an
//! `SM_AD_AUX_DATA` node only when it holds the B-spline surface (the tolerance test reads it).
//! A plane counts as a B-spline here (an `SmPlane` is an `SmBSplineSurface`).

use super::surface_surface_cache_p1::{bad, bspline_block, surface, Reader, Srf};
use crate::{call_with_buffer, FfiError};

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SubdivideNode`.
    SubdivNode,
    /// `SubdivideToTolerances`.
    SubdivTol,
    /// `SubdivideNode` splits, then `FindUVNodes`.
    Find,
    /// `BuildTree`.
    Build,
    /// `GetTree`.
    Get,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::SubdivNode => 0,
            Op::SubdivTol => 1,
            Op::Find => 2,
            Op::Build => 3,
            Op::Get => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_surface_cache_p3_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Most splits a `Find` script may list.
const MAX_SPLITS: usize = 24;

/// `SM_AD_BEZIER_SURFACE`.
const AUX_BEZIER: usize = 3;
/// `SM_AD_AUX_DATA`.
const AUX_DATA: usize = 6;

/// The surface kinds of an `S` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    BSpline,
    Plane,
    CurveBounded,
}

/// Squared length of a vector.
fn len2(v: [f64; 3]) -> f64 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// An `S` block; returns its kind and natural domain.
fn surface_spec(rd: &mut Reader<'_>) -> Result<(Kind, Srf), FfiError> {
    match rd.index(2)? {
        1 => {
            rd.vec3()?;
            let x = rd.vec3()?;
            let y = rd.vec3()?;
            let (lx, ly) = (len2(x), len2(y));
            if !(1e-6..=1e6).contains(&lx) || !(1e-6..=1e6).contains(&ly) {
                return Err(bad("plane: axis length out of range"));
            }
            if len2(cross(x, y)) < 1e-6 * lx * ly {
                return Err(bad("plane: axes (nearly) parallel"));
            }
            for _ in 0..2 {
                let s = rd.finite()?;
                if !(1e-3..=1e3).contains(&s) {
                    return Err(bad("plane: scale out of range"));
                }
            }
            let d = rd.domain()?;
            if d[2] - d[0] < 1e-6 || d[3] - d[1] < 1e-6 || d.iter().any(|v| v.abs() > 1e4) {
                return Err(bad("plane: domain size out of range"));
            }
            Ok((
                Kind::Plane,
                Srf {
                    dom: d,
                    u_distinct: 2,
                    v_distinct: 2,
                },
            ))
        }
        k => {
            let srf = bspline_block(rd)?;
            Ok((
                if k == 0 {
                    Kind::BSpline
                } else {
                    Kind::CurveBounded
                },
                srf,
            ))
        }
    }
}

/// The cache options `O`, with tolerances that keep the subdivision bounded for a surface
/// whose coordinates reach `size`: the smallest node side (`ratio` of the cache domain) caps
/// the leaves at about `1 / ratio^2`, and the length tolerances are relative to `size`.
fn options(rd: &mut Reader<'_>, size: f64) -> Result<(), FfiError> {
    let v = rd.finites(7)?;
    let get = |i: usize| v.get(i).copied().unwrap_or(0.0);
    let (chord, ang, aspect, max_side, min_side, ratio) =
        (get(0), get(1), get(2), get(3), get(4), get(5));
    if chord < 0.0 || (chord > 0.0 && chord < 1e-3 * size) {
        return Err(bad("options: chord tolerance too small for the surface"));
    }
    if ang < 0.0 || (ang > 0.0 && ang < 0.05) {
        return Err(bad("options: angle tolerance too small"));
    }
    if aspect < 0.0 || (aspect > 0.0 && aspect < 1.5) {
        return Err(bad("options: aspect ratio too small"));
    }
    if max_side < 0.0 || (max_side > 0.0 && max_side < 1e-2 * size) {
        return Err(bad("options: max side length too small for the surface"));
    }
    if min_side < 0.0 {
        return Err(bad("options: negative min side length"));
    }
    if !(1e-2..=0.5).contains(&ratio) {
        return Err(bad("options: min side length ratio out of range"));
    }
    Ok(())
}

/// The largest coordinate magnitude of the control points of the NURB at `at` (`n m p q r s
/// cpts ..`, already validated), or of a plane's corners; at least `1e-12`.
fn net_size(input: &[f64], at: usize) -> f64 {
    let get = |i: usize| input.get(i).copied().unwrap_or(0.0);
    let (n, m) = (get(at) as usize, get(at + 1) as usize);
    let mut size: f64 = 1e-12;
    for k in 0..(n + 1) * (m + 1) {
        let b = at + 6 + 4 * k;
        size = size.max(get(b).abs()).max(get(b + 1).abs()).max(get(b + 2).abs());
    }
    size
}

/// An `S` block and its size ([`net_size`]; a plane: its origin plus its scaled axes
/// over the domain).
fn sized_surface(rd: &mut Reader<'_>) -> Result<(Kind, Srf, f64), FfiError> {
    let at = rd.at;
    let (kind, srf) = surface_spec(rd)?;
    let size = match kind {
        // `kind oob form N`
        Kind::BSpline | Kind::CurveBounded => net_size(rd.input, at + 3),
        Kind::Plane => {
            let get = |i: usize| rd.input.get(at + 1 + i).copied().unwrap_or(0.0);
            let o = [get(0), get(1), get(2)].map(f64::abs);
            let reach = [get(11), get(12), get(13), get(14)]
                .iter()
                .fold(0.0_f64, |a, v| a.max(v.abs()));
            let ax = len2([get(3), get(4), get(5)]).sqrt() * get(9).abs();
            let ay = len2([get(6), get(7), get(8)]).sqrt() * get(10).abs();
            (o[0].max(o[1]).max(o[2]) + reach * (ax + ay)).max(1e-12)
        }
    };
    Ok((kind, srf, size))
}

/// A start node `T` over a surface of kind `kind`; returns its aux type and the size of
/// its patch ([`net_size`], `0` without one).
fn start_node(rd: &mut Reader<'_>, kind: Kind, srf: &Srf) -> Result<(usize, f64), FfiError> {
    let aux = rd.index(AUX_DATA)?;
    if aux != AUX_BEZIER && aux != AUX_DATA {
        return Err(bad("start node: aux type must be 3 or 6"));
    }
    let d = rd.domain_in(srf, false)?;
    let lo = rd.vec3()?;
    let hi = rd.vec3()?;
    if lo.iter().zip(hi.iter()).any(|(l, h)| l > h) {
        return Err(bad("start node: box not ordered"));
    }
    rd.index(3)?; // node class
    rd.flag()?;
    rd.flag()?;
    let surf = rd.flag()?;
    // (upstream casts the node's surface to an SmBSplineSurface: an SmPlane is one)
    if aux == AUX_DATA && surf && kind == Kind::CurveBounded {
        return Err(bad("start node: an aux node with the surface needs a B-spline or plane"));
    }
    let mut patch_size = 0.0;
    if aux == AUX_BEZIER {
        let at = rd.at;
        patch_size = net_size(rd.input, at);
        let n = rd.index(64)?;
        let m = rd.index(64)?;
        let p = rd.index(64)?;
        let q = rd.index(64)?;
        rd.at = at;
        let bez = surface(rd)?;
        if n != p || m != q {
            return Err(bad("start node: the patch must be a single span"));
        }
        if kind == Kind::CurveBounded && (p != 3 || q != 3) {
            return Err(bad("start node: patches of non B-spline caches are bicubic"));
        }
        if bez.dom != d {
            return Err(bad("start node: the patch domain must be the node domain"));
        }
        let b = [rd.vec3()?, rd.vec3()?, rd.vec3()?];
        for v in b {
            if (len2(v) - 1.0).abs() > 1e-9 {
                return Err(bad("start node: pseudo box basis vectors must be unit"));
            }
        }
        let c = cross(b[0], b[1]);
        let vol = c[0] * b[2][0] + c[1] * b[2][1] + c[2] * b[2][2];
        if vol.abs() < 1e-3 {
            return Err(bad("start node: pseudo box basis (nearly) dependent"));
        }
        for _ in 0..3 {
            let lo = rd.finite()?;
            let hi = rd.finite()?;
            if lo > hi {
                return Err(bad("start node: pseudo box interval not ordered"));
            }
        }
        for _ in 0..4 {
            rd.index(13)?; // SmContinuityType
        }
    }
    Ok((aux, patch_size))
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::SubdivNode => {
            let (kind, srf, size) = sized_surface(rd)?;
            options(rd, size)?;
            start_node(rd, kind, &srf)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::SubdivTol => {
            let (kind, srf, size) = sized_surface(rd)?;
            let opts_at = rd.at;
            rd.finites(7)?;
            let start_at = rd.at;
            let node_size = start_node(rd, kind, &srf)?.1;
            // the tolerance test reads an SM_AD_AUX_DATA node's surface as a B-spline NURB
            let aux = rd.input.get(start_at).copied().unwrap_or(0.0);
            let surf = rd.input.get(start_at + 14).copied().unwrap_or(0.0);
            if aux == AUX_DATA as f64 && (surf == 0.0 || kind == Kind::CurveBounded) {
                return Err(bad(
                    "subdivide to tolerances: an aux start node needs the B-spline or plane surface",
                ));
            }
            // the tolerances bound the subdivision of the start node's patch too
            let end = rd.at;
            rd.at = opts_at;
            options(rd, size.max(node_size))?;
            rd.at = end;
        }
        Op::Find => {
            let (kind, srf, size) = sized_surface(rd)?;
            options(rd, size)?;
            start_node(rd, kind, &srf)?;
            let ns = rd.index(MAX_SPLITS)?;
            // the leaves of the tree so far (splits add two nodes)
            let mut leaf = vec![true];
            for _ in 0..ns {
                let which = rd.index(leaf.len().saturating_sub(1))?;
                if !leaf.get(which).copied().unwrap_or(false) {
                    return Err(bad("find: only leaves can be split"));
                }
                rd.flag()?;
                rd.flag()?;
                if let Some(l) = leaf.get_mut(which) {
                    *l = false;
                }
                leaf.push(true);
                leaf.push(true);
            }
            rd.domain()?;
            rd.flag()?;
        }
        Op::Build | Op::Get => {
            let (_, _, size) = sized_surface(rd)?;
            options(rd, size)?;
        }
    }
    Ok(())
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    validate_op(op, &mut rd)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(262_144, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped surfaces, planes with
        // independent axes, domains inside the surface domain, bounded tolerances, start
        // nodes whose data upstream reads and writes safely, splits of existing leaves,
        // every enumerator the shim passes to upstream); buf is valid for buf.len() writes
        // and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_cache_p3_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
