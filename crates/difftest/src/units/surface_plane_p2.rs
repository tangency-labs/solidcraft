//! Runner for unit `surface.plane_p2`. C++: `solidcore_ffi::units::surface_plane_p2`
//! (`harness/cpp/shim/surface_plane_p2.cpp`). Rust: `solidcore::surface::plane::p2`
//! (methods of [`Plane`], and [`Surface::global_surface_intersect`] for the plane kind).
//!
//! Each case is one op on freshly built objects; both sides produce the record
//! documented in the FFI module (`rec`), compared value by value. Planes are built with
//! or without their NURB (upstream's `m_pNurb`, which `IntersectWithPlane`'s parallel
//! case tests). Curves are real `SmLine`, `SmEllipse`, `SmCircle` and B-spline curves.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::SurfaceIntersection;
use solidcore::surface::surface::Surface;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::surface_plane_p2 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

/// Generated cases per run at least (the ops are many and cheap).
const MIN_GENERATED: usize = 2000;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg.into())
}

// ---- inputs ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum MakeIn {
    Bounded {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    Normal {
        o: [f64; 3],
        n: [f64; 3],
    },
    Canonical {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlaneIn {
    make: MakeIn,
    #[serde(default)]
    nurb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SurfaceIn {
    p: usize,
    q: usize,
    /// `net[i][j] = [x, y, z]`.
    net: Vec<Vec<[f64; 3]>>,
    /// Optional weights (`w[i][j]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    w: Option<Vec<Vec<f64>>>,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
    Segment {
        a: [f64; 3],
        b: [f64; 3],
    },
    BSpline {
        dim: u32,
        p: usize,
        cpts: Vec<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        w: Option<Vec<f64>>,
        knots: Vec<f64>,
    },
    Ellipse {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        dom: [f64; 2],
        r: [f64; 2],
    },
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        dom: [f64; 2],
        r: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OtherIn {
    Same,
    Plane { plane: PlaneIn },
    BSpline { surface: SurfaceIn },
    /// A real `SmCone` / `SmCylinder` (`CreateCanonical`), over its natural domain when
    /// `natural`.
    Cone {
        frame: [[f64; 3]; 3],
        #[serde(default)]
        cylinder: bool,
        bot: f64,
        #[serde(default)]
        top: f64,
        #[serde(default)]
        height: f64,
        #[serde(default)]
        natural: bool,
    },
}

/// A database stream: the plane header, then the B-spline part from `surface`, as
/// upstream `WriteToDB` lays them out; `cut` drops trailing bytes of an ASCII stream, never
/// into the B-spline header or first control point (upstream reads those uninitialised; a
/// truncated binary stream is not comparable: upstream `SmDatabaseIO::ReadDouble` reads
/// past the end without failing and returns stale bytes, the port fails).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbIn {
    binary: bool,
    version: u32,
    /// `O X Y amin amax scale` (15 values).
    header: Vec<f64>,
    surface: SurfaceIn,
    #[serde(default = "one")]
    kt: u64,
    #[serde(default = "nine")]
    form: u64,
    #[serde(default)]
    rat: Option<i16>,
    #[serde(default)]
    cut: usize,
}

fn one() -> u64 {
    1
}

fn nine() -> u64 {
    9
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Offset {
        a: PlaneIn,
        dist: f64,
    },
    Transform {
        a: PlaneIn,
        frame: [[f64; 3]; 3],
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    TrimDomain {
        a: PlaneIn,
        dom: [f64; 4],
    },
    IsNurbPlane {
        s: SurfaceIn,
        scale: f64,
    },
    Intersect {
        #[serde(default)]
        global: bool,
        a: PlaneIn,
        da: [f64; 4],
        b: OtherIn,
        db: [f64; 4],
        #[serde(default)]
        tol: Option<f64>,
    },
    TrimCurve {
        a: PlaneIn,
        dom: [f64; 4],
        curve: CurveIn,
        tol: f64,
    },
    Line {
        a: PlaneIn,
        dom: [f64; 4],
        p: [[f64; 3]; 2],
        tol: f64,
    },
    Ellipse {
        a: PlaneIn,
        dom: [f64; 4],
        c: [f64; 3],
        r: [f64; 2],
        axes: [[f64; 3]; 2],
        ivl: [f64; 2],
        tol: f64,
    },
    ReadDb(DbIn),
}

// ---- C++ side -------------------------------------------------------------------------

fn ffi_plane(p: &PlaneIn) -> ffi::PlaneIn {
    let make = match p.make.clone() {
        MakeIn::Bounded {
            o,
            x,
            y,
            scale,
            domain,
        } => ffi::Make::Bounded {
            o,
            x,
            y,
            scale,
            domain,
        },
        MakeIn::Normal { o, n } => ffi::Make::Normal { o, n },
        MakeIn::Canonical { o, x, y } => ffi::Make::Canonical { o, x, y },
    };
    ffi::PlaneIn {
        make,
        nurb: p.nurb,
    }
}

fn ffi_surface(s: &SurfaceIn) -> ffi::SurfaceData {
    let net = s
        .net
        .iter()
        .enumerate()
        .map(|(i, r)| {
            r.iter()
                .enumerate()
                .map(|(j, c)| {
                    let w = s
                        .w
                        .as_ref()
                        .and_then(|w| w.get(i).and_then(|r| r.get(j)).copied())
                        .unwrap_or(NOW);
                    [c[0], c[1], c[2], w]
                })
                .collect()
        })
        .collect();
    ffi::SurfaceData {
        p: s.p,
        q: s.q,
        net,
        u: s.u.clone(),
        v: s.v.clone(),
    }
}

fn ffi_curve(c: &CurveIn) -> ffi::CurveIn {
    match c.clone() {
        CurveIn::Segment { a, b } => ffi::CurveIn::Segment(a, b),
        CurveIn::BSpline {
            dim,
            p,
            cpts,
            w,
            knots,
        } => ffi::CurveIn::BSpline(ffi::CurveData {
            dim,
            p,
            cpts: cpts
                .iter()
                .enumerate()
                .map(|(i, q)| {
                    let wi = w.as_ref().and_then(|w| w.get(i).copied()).unwrap_or(NOW);
                    [q[0], q[1], q[2], wi]
                })
                .collect(),
            knots,
        }),
        CurveIn::Ellipse { c, x, y, dom, r } => ffi::CurveIn::Ellipse { c, x, y, dom, r },
        CurveIn::Circle { c, x, y, dom, r } => ffi::CurveIn::Circle { c, x, y, dom, r },
    }
}

/// The bytes of a database stream.
fn db_bytes(d: &DbIn) -> Result<Vec<u8>, HarnessError> {
    if d.binary && d.cut > 0 {
        return Err(harness("truncated binary streams are not comparable"));
    }
    // A header of other than 15 values shifts the B-spline header into the wrong
    // fields: upstream then reads `n m p q rat` from failed extractions and uses them
    // uninitialised (UB-6; fuzz00842 segfaults or returns a different surface each
    // run), the port refuses the unreadable header.
    if d.header.len() != 15 {
        return Err(harness(
            "plane header must hold 15 values (upstream reads the B-spline header uninitialised)",
        ));
    }
    let s = &d.surface;
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))? as i64;
    let m = s
        .net
        .first()
        .map_or(0, Vec::len)
        .checked_sub(1)
        .ok_or_else(|| harness("empty net row"))? as i64;
    let rational = s.w.is_some();
    let rat = d.rat.unwrap_or(i16::from(rational));
    let mut vals: Vec<f64> = Vec::new();
    for (i, row) in s.net.iter().enumerate() {
        for (j, c) in row.iter().enumerate() {
            vals.extend_from_slice(c);
            if let Some(w) = &s.w {
                vals.push(w.get(i).and_then(|r| r.get(j)).copied().unwrap_or(1.0));
            }
        }
    }
    let n_pts = vals.len();
    vals.extend_from_slice(&s.u);
    vals.extend_from_slice(&s.v);
    let mut b = Vec::new();
    let mut min_len = 0;
    if d.binary {
        for x in &d.header {
            b.extend_from_slice(&x.to_ne_bytes());
        }
        b.extend_from_slice(&n.to_ne_bytes());
        b.extend_from_slice(&m.to_ne_bytes());
        b.extend_from_slice(&(s.p as i16).to_ne_bytes());
        b.extend_from_slice(&(s.q as i16).to_ne_bytes());
        b.extend_from_slice(&rat.to_ne_bytes());
        if d.version > 32 {
            b.extend_from_slice(&d.kt.to_ne_bytes());
            b.extend_from_slice(&d.form.to_ne_bytes());
        }
        for x in &vals {
            b.extend_from_slice(&x.to_ne_bytes());
        }
    } else {
        let mut t = String::new();
        for (k, x) in d.header.iter().enumerate() {
            let _ = write!(t, "{x:.17e}");
            // O, X, Y one per line; the domain corners on one line; the scales on one
            t.push(if matches!(k, 2 | 5 | 8 | 12 | 14) { '\n' } else { ' ' });
        }
        let _ = write!(t, "{n} {m}\n{} {}\n{rat}\n", s.p, s.q);
        if d.version > 32 {
            let _ = write!(t, "{}\n{}\n", d.kt, d.form);
        }
        let per = if rational { 4 } else { 3 };
        let (pts, knots) = vals.split_at(n_pts);
        for c in pts.chunks(per) {
            let s: Vec<String> = c.iter().map(|x| format!("{x:.17e}")).collect();
            t.push_str(&s.join(" "));
            t.push('\n');
            // upstream reads the B-spline header and first control point uninitialised
            // when they are unreadable: a cut must leave them whole
            if min_len == 0 {
                min_len = t.len();
            }
        }
        for x in knots {
            let _ = writeln!(t, "{x:.17e}");
        }
        b = t.into_bytes();
    }
    let len = b.len().saturating_sub(d.cut);
    if d.cut > 0 && len < min_len {
        return Err(harness("cut into the B-spline header (uninitialised upstream)"));
    }
    b.truncate(len);
    Ok(b)
}

fn ffi_op(i: &Inputs) -> Result<ffi::Op, HarnessError> {
    Ok(match i {
        Inputs::Offset { a, dist } => ffi::Op::Offset {
            a: ffi_plane(a),
            dist: *dist,
        },
        Inputs::Transform { a, frame, scale } => ffi::Op::Transform {
            a: ffi_plane(a),
            frame: *frame,
            scale: *scale,
        },
        Inputs::TrimDomain { a, dom } => ffi::Op::TrimDomain {
            a: ffi_plane(a),
            dom: *dom,
        },
        Inputs::IsNurbPlane { s, scale } => ffi::Op::IsNurbPlane {
            s: ffi_surface(s),
            scale: *scale,
        },
        Inputs::Intersect {
            global,
            a,
            da,
            b,
            db,
            tol,
        } => ffi::Op::Intersect {
            global: *global,
            a: ffi_plane(a),
            da: *da,
            b: match b {
                OtherIn::Same => ffi::OtherIn::Same,
                OtherIn::Plane { plane } => ffi::OtherIn::Plane(ffi_plane(plane)),
                OtherIn::BSpline { surface } => ffi::OtherIn::BSpline(ffi_surface(surface)),
                OtherIn::Cone {
                    frame,
                    cylinder,
                    bot,
                    top,
                    height,
                    natural,
                } => ffi::OtherIn::Cone {
                    frame: *frame,
                    cylinder: *cylinder,
                    bot: *bot,
                    top: *top,
                    height: *height,
                    natural: *natural,
                },
            },
            db: *db,
            tol: *tol,
        },
        Inputs::TrimCurve {
            a,
            dom,
            curve,
            tol,
        } => ffi::Op::TrimCurve {
            a: ffi_plane(a),
            dom: *dom,
            curve: ffi_curve(curve),
            tol: *tol,
        },
        Inputs::Line { a, dom, p, tol } => ffi::Op::Line {
            a: ffi_plane(a),
            dom: *dom,
            p: *p,
            tol: *tol,
        },
        Inputs::Ellipse {
            a,
            dom,
            c,
            r,
            axes,
            ivl,
            tol,
        } => ffi::Op::Ellipse {
            a: ffi_plane(a),
            dom: *dom,
            c: *c,
            r: *r,
            axes: *axes,
            ivl: *ivl,
            tol: *tol,
        },
        Inputs::ReadDb(d) => ffi::Op::ReadDb {
            binary: d.binary,
            version: d.version,
            bytes: db_bytes(d)?,
        },
    })
}

fn to_output(rec: Vec<f64>) -> Output {
    Output::new().values("rec", rec)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&ffi_op(i)?) {
        Ok(rec) => Ok(to_output(rec)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side ------------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn fl(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A failed Rust operation: the whole case is deferred, or the harness failed.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// Defers on [`SolidError::Unported`], else passes the result on.
fn defer<T>(r: solidcore::Result<T>) -> Result<solidcore::Result<T>, Fail> {
    if let Err(e) = &r {
        if let Some(d) = Output::unported(e) {
            return Err(Fail::Deferred(Box::new(d)));
        }
    }
    Ok(r)
}

/// `Ok(v)` with status `0` pushed, or the error status pushed (`None`).
fn st<T>(o: &mut Vec<f64>, r: solidcore::Result<T>) -> Result<Option<T>, Fail> {
    match defer(r)? {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(e) => {
            o.push(status(&e));
            Ok(None)
        }
    }
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_dom(o: &mut Vec<f64>, e: &Extent2d) {
    o.extend([e.min().x, e.min().y, e.max().x, e.max().y]);
}

fn put_ivl(o: &mut Vec<f64>, e: &Extent1d) {
    o.extend([e.min(), e.max()]);
}

fn put_surface_nurb(o: &mut Vec<f64>, s: Option<&NurbSurface>) {
    let Some(sur) = s else {
        o.push(0.0);
        return;
    };
    o.extend([
        1.0,
        sur.n() as f64,
        sur.m() as f64,
        sur.p() as f64,
        sur.q() as f64,
        sur.r() as f64,
        sur.s() as f64,
    ]);
    for row in sur.rows() {
        for c in row {
            o.extend([c.x, c.y, c.z, c.w]);
        }
    }
    o.extend_from_slice(sur.u_knots());
    o.extend_from_slice(sur.v_knots());
}

fn put_state(o: &mut Vec<f64>, a: &Plane) {
    let pos = a.position();
    o.push(a.type_code() as f64);
    put_vec(o, pos.origin());
    put_vec(o, pos.x_axis());
    put_vec(o, pos.y_axis());
    put_vec(o, pos.z_axis());
    put_dom(o, &a.step_uv_domain());
    let sc = a.uv_scale();
    o.extend([sc.x, sc.y]);
    let base = a.base();
    o.extend([
        fl(base.is_out_of_bounds_enabled()),
        f64::from(base.knot_type() as i32),
        f64::from(base.form() as i32),
    ]);
    put_surface_nurb(o, base.gw_nurb());
}

fn put_curve_nurb(o: &mut Vec<f64>, c: &BSplineCurve) {
    o.extend([f64::from(c.knot_type() as i32), f64::from(c.form() as i32)]);
    match c.gw_nurb() {
        None => o.push(0.0),
        Some(cur) => {
            o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
}

fn put_curve(o: &mut Vec<f64>, c: Option<&Curve>) -> Result<(), Fail> {
    let Some(c) = c else {
        o.push(0.0);
        return Ok(());
    };
    o.extend([1.0, c.type_code() as f64, c.dim() as f64]);
    match c.kind() {
        CurveKind::Line(l) => {
            put_vec(o, l.line_point());
            put_vec(o, l.line_vector());
            put_ivl(o, &l.step_interval());
            o.extend([l.line_scale(), fl(l.inside_out())]);
            put_curve_nurb(o, l.base());
        }
        CurveKind::Ellipse(e) => {
            let pos = e.position();
            put_vec(o, pos.origin());
            put_vec(o, pos.x_axis());
            put_vec(o, pos.y_axis());
            o.extend([e.x_radius(), e.y_radius()]);
            put_ivl(o, &e.step_interval());
            o.push(fl(e.inside_out()));
            put_curve_nurb(o, e.base());
        }
        CurveKind::BSpline(bs) => put_curve_nurb(o, bs),
        _ => return Err(Fail::Harness(harness("unexpected curve kind"))),
    }
    put_ivl(o, &c.natural_interval());
    Ok(())
}

fn put_intersection(o: &mut Vec<f64>, x: &SurfaceIntersection) -> Result<(), Fail> {
    o.push(x.curves_3d.len() as f64);
    for c in &x.curves_3d {
        put_curve(o, Some(c))?;
    }
    for uv in [&x.surface1_uv_curves, &x.surface2_uv_curves] {
        o.push(uv.len() as f64);
        o.extend(uv.iter().map(|c| fl(c.is_some())));
    }
    o.push(x.curve_types.len() as f64);
    o.extend(x.curve_types.iter().map(|t| f64::from(*t as i32)));
    o.push(x.deviations.len() as f64);
    o.extend_from_slice(&x.deviations);
    Ok(())
}

/// Builds a plane (with its NURB when asked; the status is ignored as
/// `GetOrCreateGwNurbPointer`'s).
fn rust_plane(p: &PlaneIn) -> Result<Plane, Fail> {
    let mut plane = match &p.make {
        MakeIn::Bounded {
            o,
            x,
            y,
            scale,
            domain,
        } => Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &ext(*domain),
        ),
        MakeIn::Normal { o, n } => Plane::from_normal(v3(*o), v3(*n)),
        MakeIn::Canonical { o, x, y } => {
            let frame = Axis2Placement::new(v3(*o), v3(*x), v3(*y));
            match defer(Plane::create_canonical(&frame))? {
                Ok(p) => p,
                Err(_) => return Err(Fail::Harness(harness("canonical plane failed"))),
            }
        }
    };
    if p.nurb && plane.base().gw_nurb().is_none() {
        let _ = defer(plane.make_nurb())?;
    }
    Ok(plane)
}

fn rust_surface(s: &SurfaceIn) -> Result<BSplineSurface, Fail> {
    let net = s
        .net
        .iter()
        .enumerate()
        .map(|(i, r)| {
            r.iter()
                .enumerate()
                .map(|(j, c)| {
                    let w = s
                        .w
                        .as_ref()
                        .and_then(|w| w.get(i).and_then(|r| r.get(j)).copied())
                        .unwrap_or(NOW);
                    CPoint::new(c[0], c[1], c[2], w)
                })
                .collect()
        })
        .collect();
    let sur = NurbSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface: {e}")))?;
    Ok(BSplineSurface::from_nurb(sur, false))
}

fn rust_curve(k: &Kernel, c: &CurveIn) -> Result<Curve, Fail> {
    let opts = EllipseOptions::default();
    let r = match c {
        CurveIn::Segment { a, b } => {
            Line::create_line_segment(3, v3(*a), v3(*b), None).map(Curve::from)
        }
        CurveIn::BSpline {
            dim,
            p,
            cpts,
            w,
            knots,
        } => {
            let pw: Vec<CPoint> = cpts
                .iter()
                .enumerate()
                .map(|(i, q)| {
                    let wi = w.as_ref().and_then(|w| w.get(i).copied()).unwrap_or(NOW);
                    CPoint::new(q[0], q[1], q[2], wi)
                })
                .collect();
            let n = pw.len().checked_sub(1).ok_or_else(|| harness("empty B-spline"))?;
            let m = knots.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            BSplineCurve::from_data(u64::from(*dim), n, m, *p, &pw, knots).map(Curve::from)
        }
        CurveIn::Ellipse { c, x, y, dom, r } => Ellipse::new(
            k,
            v3(*c),
            v3(*x),
            v3(*y),
            Extent1d::from_min_max(dom[0], dom[1]),
            r[0],
            r[1],
            &opts,
        )
        .map(Curve::from),
        CurveIn::Circle { c, x, y, dom, r } => Ellipse::new_circle(
            k,
            v3(*c),
            v3(*x),
            v3(*y),
            Extent1d::from_min_max(dom[0], dom[1]),
            *r,
            &opts,
        )
        .map(Curve::from),
    };
    match defer(r)? {
        Ok(c) => Ok(c),
        Err(e) => Err(Fail::Harness(harness(format!("curve: {e}")))),
    }
}

#[allow(clippy::too_many_lines)] // one arm per op
fn rust_op(k: &Kernel, i: &Inputs, o: &mut Vec<f64>) -> Result<(), Fail> {
    match i {
        Inputs::Offset { a, dist } => {
            let a = rust_plane(a)?;
            if let Some(p) = st(o, a.create_offset_surface(k, *dist))? {
                put_state(o, &p);
            }
        }
        Inputs::Transform { a, frame, scale } => {
            let mut a = rust_plane(a)?;
            let f = Axis2Placement::new(v3(frame[0]), v3(frame[1]), v3(frame[2]));
            st(o, a.transform(k, &f, scale.map(v3)))?;
            put_state(o, &a);
        }
        Inputs::TrimDomain { a, dom } => {
            let mut a = rust_plane(a)?;
            st(o, a.trim_with_domain(&ext(*dom)))?;
            put_state(o, &a);
        }
        Inputs::IsNurbPlane { s, scale } => {
            let s = rust_surface(s)?;
            o.push(0.0);
            match defer(Plane::is_nurb_surface_plane(k, &s, *scale))? {
                Ok(Some(p)) => {
                    o.extend([1.0, 1.0]);
                    put_state(o, &p);
                }
                Ok(None) => o.extend([0.0, 0.0]),
                // upstream's SER status returned through the SmBoolean: TRUE, no plane
                Err(_) => o.extend([1.0, 0.0]),
            }
        }
        Inputs::Intersect {
            global,
            a,
            da,
            b,
            db,
            tol,
        } => {
            // the shim passes `SmApproxTol3d(tol)`: |tol|, SM_USE_DEFAULT -> zone tol / 2
            let tol = tol.map(|t| approx_tol3d(k, t));
            let a = rust_plane(a)?;
            if *global {
                let sa = Surface::from(a);
                let mut natural_db = false;
                let other = match b {
                    OtherIn::Same => None,
                    OtherIn::Plane { plane } => Some(Surface::from(rust_plane(plane)?)),
                    OtherIn::BSpline { surface } => Some(Surface::from(rust_surface(surface)?)),
                    OtherIn::Cone {
                        frame,
                        cylinder,
                        bot,
                        top,
                        height,
                        natural,
                    } => {
                        natural_db = *natural;
                        let f = Axis2Placement::new(v3(frame[0]), v3(frame[1]), v3(frame[2]));
                        let c = if *cylinder {
                            Cone::create_canonical_cylinder(k, &f, *bot)
                        } else {
                            Cone::create_canonical(k, &f, *bot, *top, *height, false, false)
                        };
                        let Ok(c) = defer(c)? else {
                            // the cone's construction status (both sides report it alone)
                            o.push(-2.0);
                            return Ok(());
                        };
                        Some(Surface::from(c))
                    }
                };
                let other_ref = other.as_ref().unwrap_or(&sa);
                let db = if natural_db {
                    other_ref.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))?
                } else {
                    ext(*db)
                };
                let r = sa.global_surface_intersect(
                    k,
                    &ext(*da),
                    other_ref,
                    &db,
                    [true, true],
                    tol,
                    None,
                );
                if let Some(x) = st(o, r)? {
                    put_intersection(o, &x)?;
                }
            } else {
                let other = match b {
                    OtherIn::Same => None,
                    OtherIn::Plane { plane } => Some(rust_plane(plane)?),
                    OtherIn::BSpline { .. } | OtherIn::Cone { .. } => {
                        return Err(Fail::Harness(harness("IntersectWithPlane needs a plane")))
                    }
                };
                let other_ref = other.as_ref().unwrap_or(&a);
                let r = a.intersect_with_plane(k, &ext(*da), other_ref, &ext(*db), tol);
                if let Some((more, x)) = st(o, r)? {
                    o.push(fl(more));
                    put_intersection(o, &x)?;
                }
            }
        }
        Inputs::TrimCurve {
            a,
            dom,
            curve,
            tol,
        } => {
            let a = rust_plane(a)?;
            let c = rust_curve(k, curve)?;
            if let Some(cs) = st(o, a.trim_curve_to_plane_domain(k, &ext(*dom), &c, *tol))? {
                o.push(cs.len() as f64);
                for c in &cs {
                    put_curve(o, Some(c))?;
                }
            }
        }
        Inputs::Line { a, dom, p, tol } => {
            let a = rust_plane(a)?;
            let r = a.make_new_plane_line(&ext(*dom), v3(p[0]), v3(p[1]), *tol);
            match defer(r)? {
                Ok((c, found)) => {
                    o.extend([0.0, fl(found)]);
                    put_curve(o, c.as_ref())?;
                }
                Err(e) => return Err(Fail::Harness(harness(format!("line: {e}")))),
            }
        }
        Inputs::Ellipse {
            a,
            dom,
            c,
            r,
            axes,
            ivl,
            tol,
        } => {
            let a = rust_plane(a)?;
            let res = a.make_new_plane_ellipse(
                k,
                &ext(*dom),
                v3(*c),
                r[0],
                r[1],
                v3(axes[0]),
                v3(axes[1]),
                &Extent1d::from_min_max(ivl[0], ivl[1]),
                *tol,
            );
            if let Some(cs) = st(o, res)? {
                o.push(cs.len() as f64);
                for c in &cs {
                    put_curve(o, Some(c))?;
                }
            }
        }
        Inputs::ReadDb(d) => {
            let bytes = db_bytes(d)?;
            let mut r = DatabaseReader::new();
            r.open_for_read(
                bytes,
                if d.binary {
                    FileType::Binary
                } else {
                    FileType::Ascii
                },
            );
            if let Some(p) = st(o, Plane::read_from_db(&mut r, u64::from(d.version)))? {
                put_state(o, &p);
            }
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match rust_op(&k, i, &mut o) {
        Ok(()) => Ok(to_output(o)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generated cases ------------------------------------------------------------------

fn coord(rng: &mut Rng) -> f64 {
    // coordinates on a coarse grid sometimes, so coincidences happen
    if rng.chance(0.3) {
        rng.range_i64(-4, 4) as f64
    } else {
        rng.uniform(-10.0, 10.0)
    }
}

fn pt(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

fn unit_dir(rng: &mut Rng) -> [f64; 3] {
    if rng.chance(0.3) {
        let mut d = [0.0; 3];
        let i = rng.below(3) as usize;
        if let Some(c) = d.get_mut(i) {
            *c = if rng.chance(0.5) { 1.0 } else { -1.0 };
        }
        return d;
    }
    loop {
        let d = rng.point3(-1.0, 1.0);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l > 0.2 {
            return [d[0] / l, d[1] / l, d[2] / l];
        }
    }
}

/// An orthonormal pair.
fn frame(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    let x = unit_dir(rng);
    let mut t = unit_dir(rng);
    while cross(x, t).iter().map(|c| c * c).sum::<f64>() < 0.05 {
        t = unit_dir(rng);
    }
    let z = norm(cross(x, t));
    (x, cross(z, x))
}

/// A scale factor for the whole case: mostly 1, sometimes tiny or huge.
fn world_scale(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 1e-4,
        1 => 1e5,
        _ => 1.0,
    }
}

fn domain(rng: &mut Rng) -> [f64; 4] {
    let u0 = coord(rng);
    let v0 = coord(rng);
    let du = if rng.chance(0.05) { 1e-9 } else { rng.uniform(0.5, 8.0) };
    let dv = rng.uniform(0.5, 8.0);
    [u0, v0, u0 + du, v0 + dv]
}

fn gen_plane(rng: &mut Rng, s: f64) -> PlaneIn {
    let o = mul(pt(rng), s);
    let make = match rng.below(10) {
        0 => MakeIn::Normal {
            o,
            n: unit_dir(rng),
        },
        1 => {
            let (x, y) = frame(rng);
            MakeIn::Canonical { o, x, y }
        }
        _ => {
            let (x, mut y) = frame(rng);
            if rng.chance(0.1) {
                // slightly skewed axes: SetCanonical straightens them
                y = add(y, mul(x, 0.01));
            }
            let scale = if rng.chance(0.3) {
                [rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0)]
            } else {
                [1.0, 1.0]
            };
            MakeIn::Bounded {
                o,
                x,
                y,
                scale,
                domain: domain(rng).map(|v| v * s),
            }
        }
    };
    PlaneIn {
        make,
        nurb: rng.chance(0.5),
    }
}

/// Origin, X, Y, scale and domain of a plane input.
type PlaneData = ([f64; 3], [f64; 3], [f64; 3], [f64; 2], [f64; 4]);

/// Origin, X, Y, scale and domain of a plane input (for points on it); an infinite
/// plane gets a finite stand-in domain.
fn plane_data(p: &PlaneIn) -> PlaneData {
    match &p.make {
        MakeIn::Bounded {
            o,
            x,
            y,
            scale,
            domain,
        } => (*o, norm(*x), norm(*y), *scale, *domain),
        MakeIn::Canonical { o, x, y } => (*o, norm(*x), norm(*y), [1.0, 1.0], [-5.0, -5.0, 5.0, 5.0]),
        MakeIn::Normal { o, n } => {
            let n = norm(*n);
            let t = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            let x = norm(cross(n, t));
            (*o, x, cross(n, x), [1.0, 1.0], [-5.0, -5.0, 5.0, 5.0])
        }
    }
}

/// The point at `(u, v)` of a plane input.
fn at(p: &PlaneIn, u: f64, v: f64) -> [f64; 3] {
    let (o, x, y, s, _) = plane_data(p);
    add(add(o, mul(x, u * s[0])), mul(y, v * s[1]))
}

/// A domain for an op: the plane's, a sub-domain, or one sticking out.
fn op_domain(rng: &mut Rng, p: &PlaneIn) -> [f64; 4] {
    let (.., d) = plane_data(p);
    match rng.below(4) {
        0 => d,
        1 => {
            let a = rng.uniform(0.0, 0.4);
            let b2 = rng.uniform(0.6, 1.0);
            [
                d[0] + a * (d[2] - d[0]),
                d[1] + a * (d[3] - d[1]),
                d[0] + b2 * (d[2] - d[0]),
                d[1] + b2 * (d[3] - d[1]),
            ]
        }
        2 => [d[0] - 1.0, d[1], d[2] + 0.5, d[3]],
        _ => domain(rng),
    }
}

fn tol(rng: &mut Rng, s: f64) -> f64 {
    match rng.below(6) {
        0 => 0.0,
        1 => 1e-3 * s,
        2 => 0.5 * s,
        _ => 1e-6 * s,
    }
}

fn gen_bspline_surface(rng: &mut Rng, s: f64) -> SurfaceIn {
    // mostly bilinear or bi-quadratic planar patches, sometimes bent or rational
    let p = 1 + rng.below(2) as usize;
    let q = 1 + rng.below(2) as usize;
    let n1 = p + 1 + rng.below(2) as usize;
    let m1 = q + 1 + rng.below(2) as usize;
    let (x, y) = frame(rng);
    let o = mul(pt(rng), s);
    let su = rng.uniform(1.0, 6.0) * s;
    let sv = rng.uniform(1.0, 6.0) * s;
    let skew = if rng.chance(0.2) { rng.uniform(-0.5, 0.5) } else { 0.0 };
    let bend = if rng.chance(0.25) { rng.uniform(0.01, 1.0) * s } else { 0.0 };
    let z = cross(x, y);
    let net: Vec<Vec<[f64; 3]>> = (0..n1)
        .map(|i| {
            (0..m1)
                .map(|j| {
                    let a = i as f64 / (n1 - 1) as f64;
                    let c = j as f64 / (m1 - 1) as f64;
                    let h = if i > 0 && i + 1 < n1 { bend } else { 0.0 };
                    add(add(add(o, mul(x, a * su + c * skew * su)), mul(y, c * sv)), mul(z, h))
                })
                .collect()
        })
        .collect();
    let knots = |n1: usize, p: usize, lo: f64, hi: f64| -> Vec<f64> {
        let inner = n1 - p - 1;
        let mut k = vec![lo; p + 1];
        for i in 1..=inner {
            k.push(lo + (hi - lo) * i as f64 / (inner + 1) as f64);
        }
        k.extend(vec![hi; p + 1]);
        k
    };
    let (u0, v0) = if rng.chance(0.5) { (0.0, 0.0) } else { (coord(rng), coord(rng)) };
    let w = if rng.chance(0.15) {
        Some((0..n1).map(|_| (0..m1).map(|_| rng.uniform(0.5, 2.0)).collect()).collect())
    } else {
        None
    };
    SurfaceIn {
        p,
        q,
        net,
        w,
        u: knots(n1, p, u0, u0 + rng.uniform(0.5, 3.0)),
        v: knots(m1, q, v0, v0 + rng.uniform(0.5, 3.0)),
    }
}

/// A curve near the plane (in it, crossing it, or off it).
fn gen_curve(rng: &mut Rng, p: &PlaneIn, s: f64) -> CurveIn {
    let (_, x, y, sc, d) = plane_data(p);
    let z = cross(x, y);
    let off = if rng.chance(0.2) { rng.uniform(-1.0, 1.0) * s } else { 0.0 };
    let q = |rng: &mut Rng| {
        let u = rng.uniform(d[0] - 0.5 * (d[2] - d[0]), d[2] + 0.5 * (d[2] - d[0]));
        let v = rng.uniform(d[1] - 0.5 * (d[3] - d[1]), d[3] + 0.5 * (d[3] - d[1]));
        let _ = sc;
        add(at(p, u, v), mul(z, off))
    };
    match rng.below(5) {
        0 | 1 => {
            let a = q(rng);
            let mut b2 = q(rng);
            if a == b2 {
                b2 = add(b2, x);
            }
            CurveIn::Segment { a, b: b2 }
        }
        2 => {
            let n1 = 3 + rng.below(4) as usize;
            let pdeg = 1 + rng.below(3.min(n1 as u64 - 1)) as usize;
            let cpts: Vec<[f64; 3]> = (0..n1).map(|_| q(rng)).collect();
            let inner = n1 - pdeg - 1;
            let mut knots = vec![0.0; pdeg + 1];
            for i in 1..=inner {
                knots.push(i as f64 / (inner + 1) as f64);
            }
            knots.extend(vec![1.0; pdeg + 1]);
            CurveIn::BSpline {
                dim: 3,
                p: pdeg,
                cpts,
                w: None,
                knots,
            }
        }
        k => {
            let c = q(rng);
            let ang = rng.uniform(0.0, std::f64::consts::TAU);
            let (ca, sa) = (ang.cos(), ang.sin());
            let ex = add(mul(x, ca), mul(y, sa));
            let ey = add(mul(x, -sa), mul(y, ca));
            let ey = if rng.chance(0.3) { mul(ey, -1.0) } else { ey };
            let d0 = if rng.chance(0.5) { 0.0 } else { rng.uniform(-180.0, 180.0) };
            let len = if rng.chance(0.5) { 360.0 } else { rng.uniform(10.0, 359.0) };
            let r0 = rng.uniform(0.3, 5.0) * s;
            if k == 3 {
                CurveIn::Circle {
                    c,
                    x: ex,
                    y: ey,
                    dom: [d0, d0 + len],
                    r: r0,
                }
            } else {
                CurveIn::Ellipse {
                    c,
                    x: ex,
                    y: ey,
                    dom: [d0, d0 + len],
                    r: [r0, rng.uniform(0.3, 5.0) * s],
                }
            }
        }
    }
}

fn gen_frame(rng: &mut Rng, s: f64) -> [[f64; 3]; 3] {
    if rng.chance(0.1) {
        return [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    }
    let (x, y) = frame(rng);
    [mul(pt(rng), s), x, y]
}

#[allow(clippy::too_many_lines)] // one arm per op
fn gen_case(rng: &mut Rng) -> Inputs {
    let s = world_scale(rng);
    match rng.below(20) {
        0 => Inputs::Offset {
            a: gen_plane(rng, s),
            dist: rng.uniform(-5.0, 5.0) * s,
        },
        1 | 2 => Inputs::Transform {
            a: gen_plane(rng, s),
            frame: gen_frame(rng, s),
            scale: match rng.below(4) {
                0 => {
                    let k = rng.uniform(0.2, 4.0);
                    Some([k, k, k])
                }
                1 => Some([1.0, rng.uniform(0.5, 2.0), 1.0]),
                _ => None,
            },
        },
        3 => {
            let a = gen_plane(rng, s);
            let dom = op_domain(rng, &a);
            Inputs::TrimDomain { a, dom }
        }
        4 | 5 => Inputs::IsNurbPlane {
            s: gen_bspline_surface(rng, s),
            scale: *rng.choose(&[1.0, 10.0, 100.0, 1000.0]).unwrap_or(&1.0),
        },
        6..=9 => {
            let a = gen_plane(rng, s);
            let da = op_domain(rng, &a);
            let bp = match rng.below(6) {
                // parallel or coincident
                0 | 1 => {
                    let (o, x, y, sc, d) = plane_data(&a);
                    let z = cross(x, y);
                    let shift = if rng.chance(0.5) { 0.0 } else { rng.uniform(-1.0, 1.0) * s };
                    let shift = if rng.chance(0.3) { 1e-9 * s } else { shift };
                    let du = rng.uniform(-3.0, 3.0);
                    PlaneIn {
                        make: MakeIn::Bounded {
                            o: add(o, mul(z, shift)),
                            x,
                            y: if rng.chance(0.3) { mul(y, -1.0) } else { y },
                            scale: sc,
                            domain: [d[0] + du, d[1], d[2] + du, d[3]],
                        },
                        nurb: rng.chance(0.7),
                    }
                }
                // a plane through a point of A's domain (crossing A)
                2..=4 => {
                    let (.., d) = plane_data(&a);
                    let mid = at(
                        &a,
                        rng.uniform(d[0], d[2]),
                        rng.uniform(d[1], d[3]),
                    );
                    let (x, y) = frame(rng);
                    let h = rng.uniform(0.5, 6.0) * s;
                    let w = rng.uniform(0.5, 6.0) * s;
                    PlaneIn {
                        make: MakeIn::Bounded {
                            o: mid,
                            x,
                            y,
                            scale: [1.0, 1.0],
                            domain: [-h, -w, h, w],
                        },
                        nurb: rng.chance(0.5),
                    }
                }
                _ => gen_plane(rng, s),
            };
            let db = op_domain(rng, &bp);
            let b2 = if rng.chance(0.05) {
                OtherIn::Same
            } else {
                OtherIn::Plane { plane: bp }
            };
            Inputs::Intersect {
                global: rng.chance(0.3),
                a,
                da,
                b: b2,
                db,
                tol: if rng.chance(0.3) { Some(tol(rng, s)) } else { None },
            }
        }
        10..=13 => {
            let a = gen_plane(rng, s);
            let dom = op_domain(rng, &a);
            let curve = gen_curve(rng, &a, s);
            Inputs::TrimCurve {
                a,
                dom,
                curve,
                tol: if rng.chance(0.5) { 1e-6 * s } else { tol(rng, s).max(1e-9 * s) },
            }
        }
        14 | 15 => {
            let a = gen_plane(rng, s);
            let dom = op_domain(rng, &a);
            let (.., d) = plane_data(&a);
            let q = |rng: &mut Rng| {
                at(
                    &a,
                    rng.uniform(d[0] - 2.0, d[2] + 2.0),
                    rng.uniform(d[1] - 2.0, d[3] + 2.0),
                )
            };
            let p0 = q(rng);
            let p1 = if rng.chance(0.1) { p0 } else { q(rng) };
            Inputs::Line {
                a,
                dom,
                p: [p0, p1],
                tol: tol(rng, s),
            }
        }
        16..=18 => {
            let a = gen_plane(rng, s);
            let dom = op_domain(rng, &a);
            let (_, x, y, _, d) = plane_data(&a);
            let c = at(
                &a,
                rng.uniform(d[0] - 0.5, d[2] + 0.5),
                rng.uniform(d[1] - 0.5, d[3] + 0.5),
            );
            let ang = rng.uniform(0.0, std::f64::consts::TAU);
            let (ca, sa) = (ang.cos(), ang.sin());
            let ex = add(mul(x, ca), mul(y, sa));
            let ey = add(mul(x, -sa), mul(y, ca));
            let ey = if rng.chance(0.3) { mul(ey, -1.0) } else { ey };
            let rx = match rng.below(8) {
                0 => 0.0,
                _ => rng.uniform(0.2, 6.0) * s,
            };
            let ry = if rng.chance(0.5) { rx } else { rng.uniform(0.2, 6.0) * s };
            let d0 = if rng.chance(0.5) { 0.0 } else { rng.uniform(-180.0, 180.0) };
            let len = if rng.chance(0.5) { 360.0 } else { rng.uniform(10.0, 359.0) };
            Inputs::Ellipse {
                a,
                dom,
                c,
                r: [rx, ry],
                axes: [ex, ey],
                ivl: [d0, d0 + len],
                tol: tol(rng, s).max(1e-9 * s),
            }
        }
        _ => {
            let a = gen_plane(rng, 1.0);
            let (o, x, y, sc, d) = plane_data(&a);
            let mut header = Vec::new();
            header.extend_from_slice(&o);
            header.extend_from_slice(&x);
            header.extend_from_slice(&y);
            header.extend_from_slice(&d);
            header.extend_from_slice(&sc);
            let corner = |u: f64, v: f64| at(&a, u, v);
            let surface = SurfaceIn {
                p: 1,
                q: 1,
                net: vec![
                    vec![corner(d[0], d[1]), corner(d[0], d[3])],
                    vec![corner(d[2], d[1]), corner(d[2], d[3])],
                ],
                w: None,
                u: vec![d[0], d[0], d[2], d[2]],
                v: vec![d[1], d[1], d[3], d[3]],
            };
            let binary = rng.chance(0.5);
            Inputs::ReadDb(DbIn {
                binary,
                version: if rng.chance(0.5) { 30 } else { 33 },
                header,
                surface,
                kt: rng.below(5),
                form: rng.below(12),
                rat: if rng.chance(0.05) { Some(2) } else { None },
                cut: if !binary && rng.chance(0.15) {
                    1 + rng.below(40) as usize
                } else {
                    0
                },
            })
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.plane_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            if ffi_op(&i).is_ok_and(|op| ffi::encode(&op).is_ok()) {
                return serde_json::to_value(i).ok();
            }
        }
        None
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
