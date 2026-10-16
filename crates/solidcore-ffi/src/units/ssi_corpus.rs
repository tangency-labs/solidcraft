//! FFI wrappers for difftest's `--ssi-corpus` mode (not a plan unit).
//!
//! Shim: `harness/cpp/shim/ssi_corpus.cpp`. Upstream: `SmSurface::GlobalSurfaceIntersect`
//! (`source/SMLib/src/SmSurface.cpp` and the `SmPlane`, `SmCone`, `SmSphere`,
//! `SmSurfOfRevolution` overrides), driven the way jyviko's SSI corpus runner
//! (`runner/ssi_runner.cpp`, <https://github.com/jyviko/ssi-corpus>, MIT OR Apache-2.0)
//! drives it in its `gsi` mode.
//!
//! | Rust          | shim                                                  | upstream                 |
//! |---------------|-------------------------------------------------------|--------------------------|
//! | [`intersect`] | `sc_ssi_corpus_intersect`, `sc_ssi_corpus_take`       | `GlobalSurfaceIntersect` |

use crate::{check, FfiError, SC_ERR_BUFFER};

/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;

/// Shim status: a surface could not be built (the corpus runner's exit code 3).
pub const SC_ERR_BUILD: i32 = -1003;

/// Largest control net accepted (`nu * nv`).
const MAX_NET: usize = 1 << 22;

mod sys {
    extern "C" {
        pub fn sc_ssi_corpus_intersect(
            s1: *const f64,
            n1: i64,
            s2: *const f64,
            n2: i64,
            tol: f64,
            use_default_tol: i32,
            n_samples: i32,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_ssi_corpus_take(out: *mut f64, cap: i64, out_len: *mut i64) -> i32;
    }
}

/// One corpus surface, as the corpus runner's `build_surface` reads it.
#[derive(Debug, Clone, PartialEq)]
pub enum SsiSurface {
    /// `SmPlane(origin, xaxis, yaxis, (1, 1), [u0,u1] x [v0,v1])`.
    Plane {
        /// Origin.
        origin: [f64; 3],
        /// X axis.
        xaxis: [f64; 3],
        /// Y axis.
        yaxis: [f64; 3],
        /// `[u0, u1]`.
        u: [f64; 2],
        /// `[v0, v1]`.
        v: [f64; 2],
    },
    /// `SmCylinder` from `origin + z * h0`, angles in degrees, height `h1 - h0`.
    Cylinder {
        /// Origin of the axis.
        origin: [f64; 3],
        /// X axis.
        xaxis: [f64; 3],
        /// Y axis.
        yaxis: [f64; 3],
        /// Radius.
        radius: f64,
        /// Angle interval (degrees).
        angle: [f64; 2],
        /// Height interval along the axis.
        height: [f64; 2],
    },
    /// `SmCone::CreateCanonical` of the apex-based cone `radius(h) = h tan(half_angle)`.
    Cone {
        /// Apex.
        apex: [f64; 3],
        /// X axis.
        xaxis: [f64; 3],
        /// Y axis.
        yaxis: [f64; 3],
        /// Half angle (degrees).
        half_angle_deg: f64,
        /// Height interval along the axis, from the apex.
        height: [f64; 2],
    },
    /// `SmSphere` over the analytic domain `lon x lat` (degrees).
    Sphere {
        /// Centre.
        center: [f64; 3],
        /// X axis.
        xaxis: [f64; 3],
        /// Y axis.
        yaxis: [f64; 3],
        /// Radius.
        radius: f64,
        /// Longitude interval.
        lon: [f64; 2],
        /// Latitude interval.
        lat: [f64; 2],
    },
    /// `SmTorus` over the analytic domain `major x minor` (degrees).
    Torus {
        /// Centre.
        center: [f64; 3],
        /// X axis.
        xaxis: [f64; 3],
        /// Y axis.
        yaxis: [f64; 3],
        /// Major radius.
        major_radius: f64,
        /// Minor radius.
        minor_radius: f64,
        /// Major angle interval.
        major: [f64; 2],
        /// Minor angle interval.
        minor: [f64; 2],
    },
    /// `SmBSplineSurface` over an NLib surface filled directly.
    Nurbs {
        /// Degrees `(p, q)`.
        degree: [usize; 2],
        /// U knots (`nu + p + 1`).
        knots_u: Vec<f64>,
        /// V knots (`nv + q + 1`).
        knots_v: Vec<f64>,
        /// Control points in u.
        nu: usize,
        /// Control points in v.
        nv: usize,
        /// Cartesian control points, row-major (`i * nv + j`, `i` the u index).
        points: Vec<[f64; 3]>,
        /// Weights, same layout.
        weights: Vec<f64>,
    },
}

/// A corpus surface and its optional trim box `[u0, u1, v0, v1]` in natural parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct SsiSurfaceIn {
    /// The surface.
    pub surface: SsiSurface,
    /// The case's optional `domain`.
    pub domain: Option<[f64; 4]>,
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(format!("ssi_corpus: {msg}"))
}

fn finite(v: &[f64]) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad("non-finite value"))
    }
}

impl SsiSurfaceIn {
    /// The shim's flat encoding (layout in `harness/cpp/shim/ssi_corpus.cpp`), after
    /// rejecting what the C++ construction cannot take safely: non-finite values, degrees
    /// outside `1..=NL_MAXDEG`, inconsistent net / knot sizes, decreasing knots and
    /// non-positive weights.
    ///
    /// # Errors
    ///
    /// [`FfiError::InvalidArgument`] for such input.
    pub fn encode(&self) -> Result<Vec<f64>, FfiError> {
        let mut o = Vec::new();
        match &self.surface {
            SsiSurface::Plane {
                origin,
                xaxis,
                yaxis,
                u,
                v,
            } => {
                o.push(1.0);
                o.extend(origin.iter().chain(xaxis).chain(yaxis).chain(u).chain(v));
            }
            SsiSurface::Cylinder {
                origin,
                xaxis,
                yaxis,
                radius,
                angle,
                height,
            } => {
                o.push(2.0);
                o.extend(origin.iter().chain(xaxis).chain(yaxis));
                o.push(*radius);
                o.extend(angle.iter().chain(height));
            }
            SsiSurface::Cone {
                apex,
                xaxis,
                yaxis,
                half_angle_deg,
                height,
            } => {
                o.push(3.0);
                o.extend(apex.iter().chain(xaxis).chain(yaxis));
                o.push(*half_angle_deg);
                o.extend(height);
            }
            SsiSurface::Sphere {
                center,
                xaxis,
                yaxis,
                radius,
                lon,
                lat,
            } => {
                o.push(4.0);
                o.extend(center.iter().chain(xaxis).chain(yaxis));
                o.push(*radius);
                o.extend(lon.iter().chain(lat));
            }
            SsiSurface::Torus {
                center,
                xaxis,
                yaxis,
                major_radius,
                minor_radius,
                major,
                minor,
            } => {
                o.push(5.0);
                o.extend(center.iter().chain(xaxis).chain(yaxis));
                o.extend([*major_radius, *minor_radius]);
                o.extend(major.iter().chain(minor));
            }
            SsiSurface::Nurbs {
                degree,
                knots_u,
                knots_v,
                nu,
                nv,
                points,
                weights,
            } => {
                let [p, q] = *degree;
                if !(1..=MAX_DEGREE).contains(&p) || !(1..=MAX_DEGREE).contains(&q) {
                    return Err(bad("degree outside 1..=NL_MAXDEG"));
                }
                if *nu < p + 1 || *nv < q + 1 {
                    return Err(bad("too few control points for the degree"));
                }
                let n = nu.checked_mul(*nv).ok_or_else(|| bad("net too large"))?;
                if n > MAX_NET || points.len() != n || weights.len() != n {
                    return Err(bad("net size does not match nu * nv"));
                }
                if knots_u.len() != nu + p + 1 || knots_v.len() != nv + q + 1 {
                    return Err(bad("knot count does not match n + p + 1"));
                }
                finite(knots_u)?;
                finite(knots_v)?;
                if knots_u
                    .windows(2)
                    .chain(knots_v.windows(2))
                    .any(|w| match w {
                        [a, b] => a > b,
                        _ => false,
                    })
                {
                    return Err(bad("decreasing knots"));
                }
                if weights.iter().any(|w| !(w.is_finite() && *w > 0.0)) {
                    return Err(bad("non-positive weight"));
                }
                o.push(6.0);
                for x in [p, q, *nu, *nv] {
                    o.push(x as f64);
                }
                o.extend(knots_u.iter().chain(knots_v));
                for (pt, w) in points.iter().zip(weights) {
                    o.extend(pt);
                    o.push(*w);
                }
            }
        }
        match self.domain {
            Some(d) => {
                o.push(1.0);
                o.extend(d);
            }
            None => o.extend([0.0; 5]),
        }
        finite(&o)?;
        Ok(o)
    }
}

/// Runs `SmSurface::GlobalSurfaceIntersect` of `s1` with `s2` (natural domains or the
/// trim boxes, edges on, `SmApproxTol3d(tol)` or `NULL` for `None`, no angle tolerance) in
/// the shim and returns its record (layout in `harness/cpp/shim/ssi_corpus.cpp`), with
/// every curve sampled at `n_samples` parameters.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`SsiSurfaceIn::encode`] rejects or
/// `n_samples < 2`; [`FfiError::Status`] for a shim failure ([`SC_ERR_BUILD`]: a surface
/// could not be built).
pub fn intersect(
    s1: &SsiSurfaceIn,
    s2: &SsiSurfaceIn,
    tol: Option<f64>,
    n_samples: i32,
) -> Result<Vec<f64>, FfiError> {
    if n_samples < 2 {
        return Err(bad("n_samples < 2"));
    }
    let tol_val = tol.unwrap_or(0.0);
    if !tol_val.is_finite() {
        return Err(bad("non-finite tolerance"));
    }
    let e1 = s1.encode()?;
    let e2 = s2.encode()?;
    let n1 = i64::try_from(e1.len()).map_err(|_| bad("surface 1 too large"))?;
    let n2 = i64::try_from(e2.len()).map_err(|_| bad("surface 2 too large"))?;
    let mut len: i64 = 0;
    // SAFETY: `e1`/`e2` are valid for `n1`/`n2` reads and `len` is a valid, writable i64
    // for the duration of the call; the shim reads within those bounds only.
    let st = unsafe {
        sys::sc_ssi_corpus_intersect(
            e1.as_ptr(),
            n1,
            e2.as_ptr(),
            n2,
            tol_val,
            i32::from(tol.is_none()),
            n_samples,
            &raw mut len,
        )
    };
    check(st)?;
    let n = usize::try_from(len).map_err(|_| bad("shim returned a negative length"))?;
    let mut buf = vec![0.0f64; n.max(1)];
    let cap = i64::try_from(buf.len()).map_err(|_| bad("record too large"))?;
    let mut got: i64 = 0;
    // SAFETY: `buf` is valid for `cap` writes and `got` is a valid, writable i64; the shim
    // writes at most `cap` values (sc_copy_out).
    let st = unsafe { sys::sc_ssi_corpus_take(buf.as_mut_ptr(), cap, &raw mut got) };
    if st == SC_ERR_BUFFER {
        return Err(FfiError::Status(st));
    }
    check(st)?;
    let got = usize::try_from(got).map_err(|_| bad("shim returned a negative length"))?;
    if got > buf.len() {
        return Err(bad("shim returned more than the capacity"));
    }
    buf.truncate(got);
    Ok(buf)
}
