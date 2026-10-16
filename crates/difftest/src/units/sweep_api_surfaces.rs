//! Runner for unit `sweep.api_surfaces`: the `SM_API` surface and skin entry points.
//!
//! C++: `solidcore_ffi::units::sweep_api_surfaces` (upstream
//! `source/SM_API/src/SmApiSurfaces.cpp`, `source/SM_API/src/SmApiPrimitives.cpp`).
//! Rust: `solidcore::sweep::api_surfaces`.
//!
//! A case is one API call (`op` plus its arguments). Curve arguments are built the same
//! way on both sides: raw NURB data (`SmBSplineCurve(gw_CURVE*, dim, FALSE)`), or the
//! `SM_API` curve constructors (`SmApiCreateCurve`, `SmApiCreateArc`,
//! `SmApiCreateCircle`, `SmApiCreateLineSegment`), ported in `primitive.api_curves`.
//! Both sides produce the flat record documented in the FFI module, split into keys:
//! `status` (`0` = `SM_SUCCESS`, else the `SmStatus`: `1001` `SM_ERR`, `1007`
//! `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce), the created surface (`srf_has`, `srf_flags`, `srf_dims`, `srf_cpts`,
//! `srf_u`, `srf_v`) and, for extrusions, the extruded curve's NURB afterwards
//! (`crv_has`, `crv_n`, `crv_cpts`).
//!
//! Cases reaching `SmPrimitiveCreation` (valid skin primitives, surfaces of
//! revolution of a curve, skins from faces) are deferred on the Rust side
//! (`primitive.primitive_creation_p2` / `_p3`); their input validation (`NULL` curve,
//! malformed profile structure, bad degree) is compared. Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::primitive::api_curves;
use solidcore::sweep::api_surfaces as api;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_ffi::units::sweep_api_surfaces::{self as ffi, Call, CurveSpec};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `sweep.api_surfaces` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

type P3 = [f64; 3];

/// A curve argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CrvIn {
    /// `NULL`.
    Null,
    /// Raw NURB data.
    Nurb {
        #[serde(default = "three")]
        dim: usize,
        p: usize,
        cpts: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
    /// `SmApiCreateCurve`.
    Curve { points: Vec<P3> },
    /// `SmApiCreateArc`.
    Arc {
        center: P3,
        radius: f64,
        start_deg: f64,
        end_deg: f64,
    },
    /// `SmApiCreateCircle` (B-spline).
    Circle { center: P3, radius: f64 },
    /// `SmApiCreateLineSegment`.
    Line { start: P3, end: P3 },
    /// `SmApiCreateCircle` (analytic `SmCircle`).
    AnalyticCircle { center: P3, radius: f64 },
}

fn three() -> usize {
    3
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Extrude {
        curve: CrvIn,
        vector: P3,
    },
    Sweep {
        curve: CrvIn,
        path: CrvIn,
    },
    Ruled {
        curve1: CrvIn,
        curve2: CrvIn,
    },
    Revolution {
        curve: CrvIn,
        origin: P3,
        axis: P3,
        angle_deg: f64,
    },
    Skin {
        profiles: Vec<CrvIn>,
    },
    SkinPrimitive {
        counts: Vec<u64>,
        curves: Vec<CrvIn>,
        degree: u64,
        #[serde(default = "yes")]
        cap_ends: bool,
    },
    SkinFromFaces {
        circles: Vec<(P3, f64)>,
        degree: u64,
    },
}

// ---- C++ side -----------------------------------------------------------------------

fn spec(c: &CrvIn) -> CurveSpec {
    match c {
        CrvIn::Null => CurveSpec::Null,
        CrvIn::Nurb {
            dim,
            p,
            cpts,
            knots,
        } => CurveSpec::Nurb {
            dim: *dim,
            p: *p,
            cpts: cpts.clone(),
            knots: knots.clone(),
        },
        CrvIn::Curve { points } => CurveSpec::ApiCurve {
            points: points.clone(),
        },
        CrvIn::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => CurveSpec::ApiArc {
            center: *center,
            radius: *radius,
            start_deg: *start_deg,
            end_deg: *end_deg,
        },
        CrvIn::Circle { center, radius } => CurveSpec::ApiCircle {
            center: *center,
            radius: *radius,
        },
        CrvIn::Line { start, end } => CurveSpec::LineSegment {
            start: *start,
            end: *end,
        },
        CrvIn::AnalyticCircle { center, radius } => CurveSpec::AnalyticCircle {
            center: *center,
            radius: *radius,
        },
    }
}

fn call_of(i: &Inputs) -> Call {
    match i {
        Inputs::Extrude { curve, vector } => Call::Extrude {
            curve: spec(curve),
            vector: *vector,
        },
        Inputs::Sweep { curve, path } => Call::Sweep {
            curve: spec(curve),
            path: spec(path),
        },
        Inputs::Ruled { curve1, curve2 } => Call::Ruled {
            curve1: spec(curve1),
            curve2: spec(curve2),
        },
        Inputs::Revolution {
            curve,
            origin,
            axis,
            angle_deg,
        } => Call::Revolution {
            curve: spec(curve),
            origin: *origin,
            axis: *axis,
            angle_deg: *angle_deg,
        },
        Inputs::Skin { profiles } => Call::Skin {
            profiles: profiles.iter().map(spec).collect(),
        },
        Inputs::SkinPrimitive {
            counts,
            curves,
            degree,
            cap_ends,
        } => Call::SkinPrimitive {
            counts: counts.clone(),
            curves: curves.iter().map(spec).collect(),
            degree: *degree,
            cap_ends: *cap_ends,
        },
        Inputs::SkinFromFaces { circles, degree } => Call::SkinFromFaces {
            circles: circles.clone(),
            degree: *degree,
        },
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let flat = ffi::run(&call_of(i)).map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

// ---- output splitting ---------------------------------------------------------------

struct Split<'a> {
    flat: &'a [f64],
    at: usize,
    out: Output,
}

impl Split<'_> {
    fn take(&mut self, k: usize) -> Result<Vec<f64>, HarnessError> {
        let part = self
            .at
            .checked_add(k)
            .and_then(|end| self.flat.get(self.at..end))
            .ok_or_else(|| harness(format!("record too short ({} values)", self.flat.len())))?;
        self.at += k;
        Ok(part.to_vec())
    }

    fn put(&mut self, key: &str, v: Vec<f64>) {
        let out = std::mem::take(&mut self.out);
        self.out = out.values(key, v);
    }

    fn vals(&mut self, key: &str, k: usize) -> Result<Vec<f64>, HarnessError> {
        let v = self.take(k)?;
        self.put(key, v.clone());
        Ok(v)
    }

    fn flag(&mut self, key: &str) -> Result<bool, HarnessError> {
        Ok(self.vals(key, 1)?.first().copied() == Some(1.0))
    }

    fn count(v: &[f64], i: usize) -> Result<usize, HarnessError> {
        let x = v.get(i).copied().unwrap_or(f64::NAN);
        if x.fract() != 0.0 || !(0.0..=1e6).contains(&x) {
            return Err(harness(format!("bad count {x} in record")));
        }
        Ok(x as usize)
    }

    /// `has` then `has_nurb oob borrowed knot_type form [n m p q r s cpts u v]`.
    fn surface(&mut self) -> Result<(), HarnessError> {
        if !self.flag("srf_has")? {
            return Ok(());
        }
        let flags = self.vals("srf_flags", 5)?;
        if flags.first().copied() != Some(1.0) {
            return Ok(());
        }
        let dims = self.vals("srf_dims", 6)?;
        let (n, m) = (Self::count(&dims, 0)?, Self::count(&dims, 1)?);
        let (r, s) = (Self::count(&dims, 4)?, Self::count(&dims, 5)?);
        self.vals("srf_cpts", 4 * (n + 1) * (m + 1))?;
        self.vals("srf_u", r + 1)?;
        self.vals("srf_v", s + 1)?;
        Ok(())
    }
}

/// Splits a flat record into keys (layout in the FFI module docs).
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut s = Split {
        flat,
        at: 0,
        out: Output::new(),
    };
    let ok = s.vals("status", 1)?.first().copied() == Some(0.0);
    match i {
        Inputs::Extrude { .. } => {
            if ok {
                s.surface()?;
            }
            if s.flag("crv_has")? {
                let n = s.vals("crv_n", 1)?;
                let n = Split::count(&n, 0)?;
                s.vals("crv_cpts", 4 * (n + 1))?;
            }
        }
        Inputs::Sweep { .. } | Inputs::Ruled { .. } | Inputs::Skin { .. } => {
            if ok {
                s.surface()?;
            }
        }
        Inputs::Revolution { .. } => {
            if ok {
                s.vals("has", 1)?;
            }
        }
        Inputs::SkinPrimitive { .. } => {
            if ok {
                s.vals("faces", 4)?;
            }
        }
        Inputs::SkinFromFaces { .. } => {
            if ok {
                s.vals("faces", 2)?;
            }
        }
    }
    if s.at != flat.len() {
        return Err(harness(format!(
            "record has {} trailing values",
            flat.len() - s.at
        )));
    }
    Ok(s.out)
}

// ---- Rust side ----------------------------------------------------------------------

fn pt(p: &P3) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

fn built<T>(r: solidcore::Result<T>, what: &str) -> Result<T, HarnessError> {
    r.map_err(|e| harness(format!("{what}: {e}")))
}

/// The B-spline curve of a curve argument (`None` for `NULL`).
fn bspline(c: &CrvIn) -> Result<Option<BSplineCurve>, HarnessError> {
    Ok(Some(match c {
        CrvIn::Null => return Ok(None),
        CrvIn::Nurb {
            dim,
            p,
            cpts,
            knots,
        } => {
            let pw = cpts
                .iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect();
            let cur = built(NurbCurve::new(pw, *p, knots.clone()), "nurb")?;
            BSplineCurve::from_nurb(cur, *dim as u64, false)
        }
        CrvIn::Curve { points } => {
            let pts: Vec<Point3> = points.iter().map(pt).collect();
            built(api_curves::create_curve(&pts), "api curve")?
        }
        CrvIn::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => built(
            api_curves::create_arc(pt(center), *radius, *start_deg, *end_deg),
            "arc",
        )?,
        CrvIn::Circle { center, radius } => {
            built(api_curves::create_circle(pt(center), *radius), "circle")?
        }
        CrvIn::Line { .. } | CrvIn::AnalyticCircle { .. } => {
            return Err(harness("not a B-spline curve"))
        }
    }))
}

fn need(c: Option<BSplineCurve>) -> Result<BSplineCurve, HarnessError> {
    c.ok_or_else(|| harness("NULL curve"))
}

/// The `SmCurve` of a curve argument (`None` for `NULL`).
fn any_curve(k: &Kernel, c: &CrvIn) -> Result<Option<SmCurve>, HarnessError> {
    Ok(match c {
        CrvIn::Line { start, end } => Some(SmCurve::from(built(
            api_curves::create_line_segment(pt(start), pt(end)),
            "line",
        )?)),
        CrvIn::AnalyticCircle { center, radius } => Some(SmCurve::from(built(
            api_curves::create_analytic_circle(k, pt(center), *radius),
            "analytic circle",
        )?)),
        other => bspline(other)?.map(SmCurve::from),
    })
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn dump_surface(o: &mut Vec<f64>, s: &BSplineSurface) {
    o.extend([
        1.0,
        flag(s.gw_nurb().is_some()),
        flag(s.is_out_of_bounds_enabled()),
        flag(s.is_nurb_borrowed()),
        f64::from(s.knot_type() as i32),
        f64::from(s.form() as i32),
    ]);
    if let Some(sur) = s.gw_nurb() {
        let ix = sur.indices();
        o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
        for row in sur.rows() {
            for p in row {
                o.extend([p.x, p.y, p.z, p.w]);
            }
        }
        o.extend_from_slice(sur.u_knots());
        o.extend_from_slice(sur.v_knots());
    }
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// Pushes the status then, on success, the surface.
fn surface_result(o: &mut Vec<f64>, r: solidcore::Result<BSplineSurface>) -> Result<(), SolidError> {
    match r {
        Ok(s) => {
            o.push(0.0);
            dump_surface(o, &s);
            Ok(())
        }
        Err(e) if e.is_unported() => Err(e),
        Err(e) => {
            o.push(status(&e));
            Ok(())
        }
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    let res: Result<(), SolidError> = match i {
        Inputs::Extrude { curve, vector } => {
            let mut c = need(bspline(curve)?)?;
            let r = api::create_extrude(&mut c, Vec3::new(vector[0], vector[1], vector[2]));
            let res = surface_result(&mut o, r);
            match c.gw_nurb() {
                Some(cur) => {
                    o.extend([1.0, cur.n() as f64]);
                    for p in cur.cpts() {
                        o.extend([p.x, p.y, p.z, p.w]);
                    }
                }
                None => o.push(0.0),
            }
            res
        }
        Inputs::Sweep { curve, path } => {
            let c = need(bspline(curve)?)?;
            let p = need(bspline(path)?)?;
            surface_result(&mut o, api::create_sweep(&c, &p))
        }
        Inputs::Ruled { curve1, curve2 } => {
            let c1 = need(bspline(curve1)?)?;
            let c2 = need(bspline(curve2)?)?;
            surface_result(&mut o, api::create_ruled_surface(&c1, &c2))
        }
        Inputs::Revolution {
            curve,
            origin,
            axis,
            angle_deg,
        } => {
            let c = bspline(curve)?;
            match api::create_surface_revolution(
                c.as_ref(),
                pt(origin),
                Vec3::new(axis[0], axis[1], axis[2]),
                *angle_deg,
            ) {
                Ok(_) => Err(SolidError::sm_err("runner: revolution cannot succeed yet")),
                Err(e) if e.is_unported() => Err(e),
                Err(e) => {
                    o.push(status(&e));
                    Ok(())
                }
            }
        }
        Inputs::Skin { profiles } => {
            let ps = profiles
                .iter()
                .map(|c| need(bspline(c)?))
                .collect::<Result<Vec<_>, _>>()?;
            surface_result(&mut o, api::create_skin(&k, &ps))
        }
        Inputs::SkinPrimitive {
            counts,
            curves,
            degree,
            cap_ends,
        } => {
            let cs = curves
                .iter()
                .map(|c| any_curve(&k, c))
                .collect::<Result<Vec<_>, _>>()?;
            let refs: Vec<Option<&SmCurve>> = cs.iter().map(Option::as_ref).collect();
            let counts = counts
                .iter()
                .map(|&c| usize::try_from(c).map_err(|_| harness("count too large")))
                .collect::<Result<Vec<_>, _>>()?;
            let degree = usize::try_from(*degree).map_err(|_| harness("degree too large"))?;
            match api::create_skin_primitive(&k, &counts, &refs, degree, *cap_ends) {
                Ok(r) => {
                    o.extend([
                        0.0,
                        1.0,
                        r.start_faces.len() as f64,
                        r.side_faces.len() as f64,
                        r.end_faces.len() as f64,
                    ]);
                    Ok(())
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => {
                    o.push(status(&e));
                    Ok(())
                }
            }
        }
        Inputs::SkinFromFaces { circles: _, degree } => {
            // The planar circle breps (`SmApiCreatePlanarCircle`) cannot be built in
            // Rust yet; the entry point is unported whatever its faces.
            let degree = usize::try_from(*degree).map_err(|_| harness("degree too large"))?;
            match api::create_skin_from_faces(&k, &Brep::default(), &[], degree) {
                Ok(r) => {
                    o.extend([0.0, 1.0, r.new_faces.len() as f64]);
                    Ok(())
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => {
                    o.push(status(&e));
                    Ok(())
                }
            }
        }
    };
    match res {
        Ok(()) => to_output(i, &o),
        Err(e) => match Output::unported(&e) {
            Some(out) => Ok(out),
            None => Err(harness(format!("rust: {e}"))),
        },
    }
}

// ---- generators ---------------------------------------------------------------------

/// A length scale: mostly moderate, sometimes tiny or huge.
fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 10f64.powi(-(rng.range_i64(3, 6) as i32)),
        1 => 10f64.powi(rng.range_i64(3, 6) as i32),
        _ => rng.uniform(0.2, 20.0),
    }
}

fn scaled(p: P3, s: f64, off: P3) -> P3 {
    [p[0] * s + off[0], p[1] * s + off[1], p[2] * s + off[2]]
}

/// A clamped knot vector of degree `p` for `n + 1` control points over `[a, b]`.
fn gen_knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let interior = n - p;
    let mut inner: Vec<f64> = if rng.chance(0.5) {
        (1..=interior)
            .map(|i| i as f64 / (interior + 1) as f64)
            .collect()
    } else {
        (0..interior).map(|_| rng.uniform(0.05, 0.95)).collect()
    };
    inner.sort_by(f64::total_cmp);
    let mut prev = f64::NEG_INFINITY;
    for t in &mut inner {
        if *t <= prev + 1e-3 {
            *t = prev + 1e-3;
        }
        prev = *t;
    }
    let mut k = vec![a; p + 1];
    k.extend(inner.iter().map(|t| a + (b - a) * t.min(0.999)));
    k.extend(vec![b; p + 1]);
    k
}

/// A wavy polyline-like point sequence of `n` points along x, in the plane `z`.
fn wave_points(rng: &mut Rng, n: usize, s: f64, off: P3, z: f64) -> Vec<P3> {
    (0..n)
        .map(|i| {
            let x = i as f64 * rng.uniform(0.5, 2.0) + i as f64;
            let y = rng.uniform(-2.0, 2.0);
            let dz = if rng.chance(0.3) {
                rng.uniform(-1.0, 1.0)
            } else {
                0.0
            };
            scaled([x, y, z + dz], s, off)
        })
        .collect()
}

/// A raw NURB curve (possibly rational, 2D, or degenerate).
fn gen_nurb(rng: &mut Rng, s: f64, off: P3, z: f64, allow_2d: bool) -> CrvIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(5) as usize;
    let rational = rng.chance(0.35);
    let two_d = allow_2d && rng.chance(0.3);
    let degenerate = rng.chance(0.08);
    let pts = wave_points(rng, n + 1, s, off, z);
    let first = pts.first().copied().unwrap_or([0.0; 3]);
    let cpts = pts
        .iter()
        .map(|q| {
            let q = if degenerate { first } else { *q };
            let w = if rational { rng.uniform(0.3, 3.0) } else { NOW };
            let (x, y, zz) = if rational {
                (q[0] * w, q[1] * w, q[2] * w)
            } else {
                (q[0], q[1], q[2])
            };
            if two_d {
                [x, y, NOZ, w]
            } else {
                [x, y, zz, w]
            }
        })
        .collect();
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-3.0, 3.0);
        (a, a + rng.uniform(0.1, 5.0))
    };
    let dim = if two_d { 2 } else { 3 };
    CrvIn::Nurb {
        dim,
        p,
        cpts,
        knots: gen_knots(rng, n, p, a, b),
    }
}

/// A B-spline curve argument in the plane `z` (raw NURB, `SmApiCreateCurve`, arc or
/// circle).
fn gen_bspline(rng: &mut Rng, s: f64, off: P3, z: f64, allow_2d: bool) -> CrvIn {
    match rng.below(6) {
        0 | 1 => gen_nurb(rng, s, off, z, allow_2d),
        2 | 3 => {
            let n = 4 + rng.below(7) as usize;
            let mut points = wave_points(rng, n, s, off, z);
            if rng.chance(0.08) {
                // the degenerate curve of the upstream tests: every point the same
                let first = points.first().copied().unwrap_or([0.0; 3]);
                points.fill(first);
            }
            CrvIn::Curve { points }
        }
        4 => {
            let a0 = rng.uniform(-180.0, 0.0);
            CrvIn::Arc {
                center: scaled([0.0, 0.0, z], s, off),
                radius: s * rng.uniform(0.5, 5.0),
                start_deg: a0,
                end_deg: a0 + rng.uniform(5.0, 360.0),
            }
        }
        _ => CrvIn::Circle {
            center: scaled([0.0, 0.0, z], s, off),
            radius: s * rng.uniform(0.5, 5.0),
        },
    }
}

fn gen_vector(rng: &mut Rng, s: f64) -> P3 {
    match rng.below(12) {
        0 => [0.0, 0.0, 0.0],
        // straddling SM_EFF_ZERO (1e-12) in length
        1 => [0.0, 0.0, 0.9e-12],
        2 => [0.0, 1.1e-12, 0.0],
        3 => [1e-12, 0.0, 0.0],
        4 => [0.0, 0.0, 1.0],
        _ => {
            let v = rng.point3(-1.0, 1.0);
            [v[0] * s, v[1] * s, v[2] * s]
        }
    }
}

fn gen_extrude(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let off = rng.point3(-10.0, 10.0);
    Inputs::Extrude {
        curve: gen_bspline(rng, s, off, 0.0, true),
        vector: gen_vector(rng, s),
    }
}

fn gen_sweep(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let off = rng.point3(-10.0, 10.0);
    let curve = gen_bspline(rng, s, off, 0.0, true);
    let path = match rng.below(5) {
        // a path rising out of the section's plane
        0 | 1 => {
            let n = 4 + rng.below(4) as usize;
            let points = (0..n)
                .map(|i| {
                    let t = i as f64;
                    scaled(
                        [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), 2.0 * t],
                        s,
                        off,
                    )
                })
                .collect();
            CrvIn::Curve { points }
        }
        // zero-length path (upstream test)
        2 if rng.chance(0.5) => CrvIn::Curve {
            points: vec![scaled([5.0, 5.0, 5.0], s, off); 4],
        },
        _ => {
            let z = rng.uniform(1.0, 10.0);
            gen_bspline(rng, s, off, z, true)
        }
    };
    Inputs::Sweep { curve, path }
}

fn gen_ruled(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let off = rng.point3(-10.0, 10.0);
    let curve1 = gen_bspline(rng, s, off, 0.0, true);
    let curve2 = if rng.chance(0.3) {
        // the same construction raised (upstream test)
        match &curve1 {
            CrvIn::Curve { points } => CrvIn::Curve {
                points: points
                    .iter()
                    .map(|q| [q[0], q[1], q[2] + 10.0 * s])
                    .collect(),
            },
            _ => gen_bspline(rng, s, off, 10.0, true),
        }
    } else {
        let z = rng.uniform(0.0, 10.0);
        gen_bspline(rng, s, off, z, true)
    };
    Inputs::Ruled { curve1, curve2 }
}

fn gen_skin(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let off = rng.point3(-10.0, 10.0);
    let k = match rng.below(10) {
        0 => rng.below(2) as usize,
        _ => 2 + rng.below(4) as usize,
    };
    let circles = rng.chance(0.5);
    let profiles = (0..k)
        .map(|i| {
            let z = i as f64 * rng.uniform(1.0, 5.0);
            if circles {
                CrvIn::Circle {
                    center: scaled([0.0, 0.0, z], s, off),
                    radius: s * rng.uniform(1.0, 5.0),
                }
            } else {
                gen_bspline(rng, s, off, z, true)
            }
        })
        .collect();
    Inputs::Skin { profiles }
}

fn gen_revolution(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let null = rng.chance(0.7);
    let curve = if null {
        CrvIn::Null
    } else {
        let points = (0..6)
            .map(|i| [2.0 * f64::from(i), rng.uniform(1.0, 7.0), 0.0].map(|x| x * s))
            .collect();
        CrvIn::Curve { points }
    };
    let axis = match rng.below(6) {
        0 | 1 => [1.0, 0.0, 0.0],
        2 => [0.0, 0.0, 0.0],
        _ => rng.point3(-1.0, 1.0),
    };
    Inputs::Revolution {
        curve,
        origin: if null { rng.point3(-5.0, 5.0) } else { [0.0; 3] },
        axis,
        angle_deg: rng.uniform(-360.0, 360.0),
    }
}

/// A closed profile curve at height `z` of any kind.
fn gen_profile_curve(rng: &mut Rng, z: f64, r: f64) -> CrvIn {
    match rng.below(4) {
        0 => CrvIn::AnalyticCircle {
            center: [0.0, 0.0, z],
            radius: r,
        },
        1 => CrvIn::Circle {
            center: [0.0, 0.0, z],
            radius: r,
        },
        2 => CrvIn::Line {
            start: [0.0, 0.0, z],
            end: [r, 0.0, z],
        },
        _ => CrvIn::Arc {
            center: [0.0, 0.0, z],
            radius: r,
            start_deg: 0.0,
            end_deg: 360.0,
        },
    }
}

fn gen_skin_primitive(rng: &mut Rng) -> Inputs {
    let np = rng.below(5) as usize;
    let mut counts: Vec<u64> = (0..np).map(|_| 1 + rng.below(2)).collect();
    let total: u64 = counts.iter().sum();
    let mut curves: Vec<CrvIn> = (0..total)
        .map(|i| {
            let r = rng.uniform(1.0, 6.0);
            gen_profile_curve(rng, i as f64 * 2.0, r)
        })
        .collect();
    let mut degree = 1 + rng.below(3);
    // break the structure in one of the ways the validation rejects
    match rng.below(9) {
        0 => degree = 0,
        1 => degree = 4 + rng.below(1000),
        2 => {
            if let Some(c) = counts.first_mut() {
                *c = 0;
            }
        }
        3 => {
            if let Some(c) = counts.last_mut() {
                *c += 1 + rng.below(3);
            }
        }
        4 => curves.push(gen_profile_curve(rng, 50.0, 2.0)),
        5 => {
            if !curves.is_empty() {
                let i = rng.below(curves.len() as u64) as usize;
                if let Some(c) = curves.get_mut(i) {
                    *c = CrvIn::Null;
                }
            }
        }
        6 => {
            curves.pop();
        }
        7 => counts.truncate(1),
        // a valid structure (deferred unless it has fewer than two profiles)
        _ => {}
    }
    Inputs::SkinPrimitive {
        counts,
        curves,
        degree,
        cap_ends: rng.chance(0.7),
    }
}

fn gen_skin_from_faces(rng: &mut Rng) -> Inputs {
    let n = 1 + rng.below(3) as usize;
    let circles = (0..n)
        .map(|i| ([0.0, 0.0, 10.0 * i as f64], rng.uniform(2.0, 8.0)))
        .collect();
    Inputs::SkinFromFaces {
        circles,
        degree: 1 + rng.below(3),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(40) {
        0..=9 => gen_extrude(rng),
        10..=17 => gen_sweep(rng),
        18..=25 => gen_ruled(rng),
        26..=31 => gen_skin(rng),
        32..=33 => gen_revolution(rng),
        34..=38 => gen_skin_primitive(rng),
        _ => gen_skin_from_faces(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "sweep.api_surfaces"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach success and failure of the compared ops on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..800u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let op = serde_json::to_value(&i).expect("json")["op"]
                .as_str()
                .expect("op")
                .to_string();
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("status:{}", st.unwrap_or(f64::NAN))
            };
            *tally.entry((op, kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            eprintln!("{k:?} {v}");
        }
        // (`CreateRuledSurface` has no failure reachable with valid curves)
        for op in ["extrude", "sweep", "ruled", "skin"] {
            let ok = tally.keys().any(|(o, k)| o == op && k == "status:0");
            assert!(ok, "{op} never succeeds");
            let failed = tally
                .keys()
                .any(|(o, k)| o == op && k.starts_with("status:") && k != "status:0");
            assert!(failed || op == "ruled", "{op} never fails");
        }
        let compared: usize = tally
            .iter()
            .filter(|((_, k), _)| k.starts_with("status:"))
            .map(|(_, v)| v)
            .sum();
        assert!(compared * 10 >= 800 * 8, "only {compared} compared cases");
    }
}
