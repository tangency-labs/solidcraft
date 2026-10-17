//! Runner for unit `surface.surf_of_extrusion_p2`: `SmSurfOfExtrusion` part 2
//! (`IntersectWithPlane`, `IntersectWithCylinder`, `IntersectWithSurfOfExtrusion`,
//! `GlobalSurfaceIntersect`, `IsNurbSurfaceSurfOfExtrusion`, `TrimWithDomain`,
//! `ReadFromDB`).
//!
//! C++: `solidcore_ffi::units::surface_surf_of_extrusion_p2` (upstream
//! `source/SMLib/src/SmSurfOfExtrusion.cpp`). Rust:
//! `solidcore::surface::surf_of_extrusion::p2`.
//!
//! A case is a surface source `src` (a surface built as in the
//! `surface.surf_of_extrusion_p1` runner, a database record written from data, or the
//! record of a surface of extrusion in an upstream test file) plus `op` and its
//! parameters; `is_nurb` cases have a test surface instead. Both sides produce the same
//! flat record (layout in the FFI module docs), split into `build` (the source status)
//! and `out` (the rest). Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`; `-1` on the
//! Rust side for an error upstream cannot produce.
//!
//! Deferred cases: `IntersectWithPlane` of a linear or coplanar generator whose
//! plane/plane intersection gives a line (`SmSurface::CurveOnClassify`,
//! `intersect.curve_class_p2`; a bounded plane the swept plane misses compares);
//! `GlobalSurfaceIntersect` needing the general intersector
//! (`intersect.adv_surface_intersector`, on a surface of extrusion that is not a
//! `Surface` yet).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_extrusion::p2::{ExtrusionIntersection, IntersectTarget};
use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
use solidcore::surface::surface::Surface;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore_ffi::units::surface_surf_of_extrusion_p2 as ffi;

use super::surface_surf_of_extrusion_p1::{
    add, cross3, dom4, ffi_surf, flag, frame, gen_generator, gen_surf, harness, normalize, nurb_of,
    put_curve, put_state, rust_surf, scale, status, unit_vec, v3, BuildIn, Frame, SurfIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1200;

/// `surface.surf_of_extrusion_p2` runner.
pub struct Runner;

/// `SmSurfOfExtrusion_TYPE` record versions generated.
const VERSIONS: &[u64] = &[29, 30, 32, 33, 38, 41, 45];

// ---- inputs -----------------------------------------------------------------------

/// A B-spline curve record (`SmBSplineCurve::ReadFromDB`): stored control points
/// (`dim` coordinates, then the weight when rational).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurveRec {
    p: i16,
    rat: i16,
    dim: i16,
    kt: u64,
    form: u64,
    pts: Vec<Vec<f64>>,
    knots: Vec<f64>,
}

/// A B-spline surface record (`SmBSplineSurface::ReadFromDB`), `v` fastest.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SurfRec {
    n: i64,
    m: i64,
    p: i16,
    q: i16,
    rat: i16,
    kt: u64,
    form: u64,
    pts: Vec<Vec<f64>>,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A surface of extrusion record (`SmSurfOfExtrusion::ReadFromDB`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordIn {
    binary: bool,
    version: u64,
    origin: [f64; 3],
    vec: [f64; 3],
    anal: [f64; 4],
    orient: u64,
    swap: i32,
    cp: [f64; 3],
    cv: [f64; 3],
    gen: CurveRec,
    surf: SurfRec,
}

/// A surface source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "src", rename_all = "snake_case")]
enum SrcIn {
    /// Built as in the p1 runner.
    Built { surf: SurfIn },
    /// A record written from data.
    Record { rec: Box<RecordIn> },
    /// The ASCII record of an upstream test file starting at 1-based `line` (after the
    /// `Object Type 15030` header).
    File {
        path: String,
        line: usize,
        version: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PlaneIn {
    Bounded {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        dom: [f64; 4],
    },
    Normal {
        o: [f64; 3],
        n: [f64; 3],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ConeKindIn {
    Canonical {
        bot: f64,
        top: f64,
        height: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
    Semi {
        bot: f64,
        semi: f64,
        #[serde(default)]
        height: Option<f64>,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
    Cylinder {
        radius: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConeIn {
    cone: ConeKindIn,
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
enum TargetIn {
    /// An `SmPlane`; `as_surface` passes it as a `Surface` on the Rust side.
    Plane {
        plane: PlaneIn,
        #[serde(default)]
        as_surface: bool,
    },
    Cone {
        cone: ConeIn,
    },
    Extrusion {
        other: Box<SrcIn>,
    },
    /// The NURB of a surface of extrusion as an `SmBSplineSurface`.
    Bspline {
        other: Box<SrcIn>,
    },
}

/// An inline NURB surface (stored control points, `v` fastest).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NurbIn {
    n: usize,
    m: usize,
    p: usize,
    q: usize,
    cpts: Vec<[f64; 4]>,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "test", rename_all = "snake_case")]
enum TestSourceIn {
    Raw { nurb: NurbIn },
    Src { of: Box<SrcIn> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestIn {
    source: TestSourceIn,
    #[serde(default)]
    transpose: bool,
    #[serde(default)]
    reparam: Option<[f64; 4]>,
    #[serde(default)]
    perturb: Option<(usize, usize, [f64; 3])>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State {},
    Plane {
        plane: PlaneIn,
        #[serde(default)]
        dom: Option<[f64; 4]>,
        #[serde(default)]
        other_dom: Option<[f64; 4]>,
        #[serde(default)]
        tol: Option<f64>,
    },
    Cylinder {
        cone: ConeIn,
        #[serde(default)]
        tol: Option<f64>,
    },
    Extrusion {
        other: Box<SrcIn>,
        #[serde(default)]
        tol: Option<f64>,
    },
    Global {
        #[serde(flatten)]
        target: TargetIn,
        #[serde(default)]
        dom: Option<[f64; 4]>,
        #[serde(default)]
        other_dom: Option<[f64; 4]>,
        #[serde(default)]
        tol: Option<f64>,
        #[serde(default)]
        ang: Option<f64>,
    },
    Trim {
        dom: [f64; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Inputs {
    IsNurb { is_nurb: TestIn, scale: f64 },
    Op {
        #[serde(flatten)]
        src: SrcIn,
        #[serde(flatten)]
        op: Box<OpIn>,
    },
}

// ---- records ----------------------------------------------------------------------

/// Writes a database stream in either format.
struct W {
    binary: bool,
    buf: Vec<u8>,
    mid: bool,
}

impl W {
    fn sep(&mut self) {
        if self.mid {
            self.buf.push(b' ');
        }
        self.mid = true;
    }
    fn text(&mut self, s: &str) {
        if !self.binary {
            self.sep();
            self.buf.extend(s.as_bytes());
        }
    }
    fn eol(&mut self) {
        if !self.binary {
            self.buf.push(b'\n');
            self.mid = false;
        }
    }
    fn ul(&mut self, v: u64) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(v.to_string().as_bytes());
        }
    }
    fn lg(&mut self, v: i64) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(v.to_string().as_bytes());
        }
    }
    fn bo(&mut self, v: i32) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(v.to_string().as_bytes());
        }
    }
    fn sh(&mut self, v: i16) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(v.to_string().as_bytes());
        }
    }
    fn db(&mut self, v: f64) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(format!("{v:e}").as_bytes());
        }
    }
    fn dbs(&mut self, v: &[f64]) {
        for &x in v {
            self.db(x);
        }
        self.eol();
    }
}

/// The bytes of a record (after the surface's own type header).
fn record_bytes(r: &RecordIn) -> Vec<u8> {
    let mut w = W {
        binary: r.binary,
        buf: Vec::new(),
        mid: false,
    };
    w.dbs(&r.origin);
    w.dbs(&r.vec);
    w.dbs(&r.anal);
    w.ul(r.orient);
    w.eol();
    w.bo(r.swap);
    w.eol();
    w.dbs(&r.cp);
    w.dbs(&r.cv);
    w.text("IwSurfOfExtrusion->GenCurve");
    w.eol();
    // the generator's type header
    let c = &r.gen;
    if w.binary {
        w.lg(14033);
        w.ul(c.dim as u64);
    } else {
        w.text("Object Type");
        w.lg(14033);
        w.eol();
        w.text("Curve Dim");
        w.ul(c.dim as u64);
        w.eol();
    }
    w.lg(c.pts.len() as i64 - 1);
    w.eol();
    w.sh(c.p);
    w.eol();
    w.sh(c.rat);
    w.eol();
    w.sh(c.dim);
    w.eol();
    if r.version > 32 {
        w.ul(c.kt);
        w.eol();
        w.ul(c.form);
        w.eol();
    }
    for p in &c.pts {
        w.dbs(p);
    }
    for &k in &c.knots {
        w.dbs(&[k]);
    }
    let s = &r.surf;
    w.lg(s.n);
    w.lg(s.m);
    w.eol();
    w.sh(s.p);
    w.sh(s.q);
    w.eol();
    w.sh(s.rat);
    w.eol();
    if r.version > 32 {
        w.ul(s.kt);
        w.eol();
        w.ul(s.form);
        w.eol();
    }
    for p in &s.pts {
        w.dbs(p);
    }
    for &k in &s.u {
        w.dbs(&[k]);
    }
    for &k in &s.v {
        w.dbs(&[k]);
    }
    w.buf
}

/// The bytes of an upstream file from 1-based `line` on.
fn file_bytes(path: &str, line: usize) -> Result<Vec<u8>, HarnessError> {
    let data = std::fs::read(Path::new(path)).map_err(|e| harness(format!("{path}: {e}")))?;
    let mut at = 0usize;
    let mut cur = 1usize;
    while cur < line {
        let nl = data
            .get(at..)
            .and_then(|d| d.iter().position(|&c| c == b'\n'))
            .ok_or_else(|| harness(format!("{path}: no line {line}")))?;
        at += nl + 1;
        cur += 1;
    }
    Ok(data.get(at..).unwrap_or(&[]).to_vec())
}

/// `(ascii, version, bytes)` of a record source.
fn src_bytes(s: &SrcIn) -> Result<Option<(bool, u64, Vec<u8>)>, HarnessError> {
    Ok(match s {
        SrcIn::Built { .. } => None,
        SrcIn::Record { rec } => Some((!rec.binary, rec.version, record_bytes(rec))),
        SrcIn::File {
            path,
            line,
            version,
        } => Some((true, *version, file_bytes(path, *line)?)),
    })
}

// ---- C++ side -------------------------------------------------------------------------

fn ffi_src(s: &SrcIn) -> Result<ffi::Src, HarnessError> {
    if let SrcIn::Built { surf } = s {
        return Ok(ffi::Src::Built(ffi_surf(surf)?));
    }
    let (ascii, version, bytes) = src_bytes(s)?.ok_or_else(|| harness("no record"))?;
    Ok(ffi::Src::Record {
        ascii,
        version,
        bytes,
    })
}

fn ffi_plane(p: &PlaneIn) -> ffi::PlaneIn {
    match p {
        PlaneIn::Bounded {
            o,
            x,
            y,
            scale,
            dom,
        } => ffi::PlaneIn::Bounded {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
            dom: *dom,
        },
        PlaneIn::Normal { o, n } => ffi::PlaneIn::Normal { o: *o, n: *n },
    }
}

fn ffi_cone(c: &ConeIn) -> ffi::ConeIn {
    ffi::ConeIn {
        kind: match c.cone {
            ConeKindIn::Canonical {
                bot,
                top,
                height,
                swap,
                inside,
            } => ffi::ConeKind::Canonical {
                bot,
                top,
                height,
                swap,
                inside,
            },
            ConeKindIn::Semi {
                bot,
                semi,
                height,
                swap,
                inside,
            } => ffi::ConeKind::Semi {
                bot,
                semi,
                height,
                swap,
                inside,
            },
            ConeKindIn::Cylinder { radius } => ffi::ConeKind::Cylinder { radius },
        },
        o: c.o,
        x: c.x,
        y: c.y,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let flat = match i {
        Inputs::IsNurb { is_nurb, scale } => {
            let t = ffi::TestSurf {
                source: match &is_nurb.source {
                    TestSourceIn::Raw { nurb } => ffi::TestSource::Raw(ffi::NurbIn {
                        n: nurb.n,
                        m: nurb.m,
                        p: nurb.p,
                        q: nurb.q,
                        cpts: nurb.cpts.clone(),
                        u: nurb.u.clone(),
                        v: nurb.v.clone(),
                    }),
                    TestSourceIn::Src { of } => ffi::TestSource::Src(ffi_src(of)?),
                },
                transpose: is_nurb.transpose,
                reparam: is_nurb.reparam,
                perturb: is_nurb.perturb,
            };
            ffi::is_nurb(&t, *scale)
        }
        Inputs::Op { src, op } => {
            let op = match &**op {
                OpIn::State {} => ffi::Op::State,
                OpIn::Plane {
                    plane,
                    dom,
                    other_dom,
                    tol,
                } => ffi::Op::Plane {
                    plane: ffi_plane(plane),
                    dom: *dom,
                    other_dom: *other_dom,
                    tol: *tol,
                },
                OpIn::Cylinder { cone, tol } => ffi::Op::Cylinder {
                    cone: ffi_cone(cone),
                    tol: *tol,
                },
                OpIn::Extrusion { other, tol } => ffi::Op::Extrusion {
                    other: ffi_src(other)?,
                    tol: *tol,
                },
                OpIn::Global {
                    target,
                    dom,
                    other_dom,
                    tol,
                    ang,
                } => ffi::Op::Global {
                    target: match target {
                        TargetIn::Plane { plane, .. } => ffi::Target::Plane(ffi_plane(plane)),
                        TargetIn::Cone { cone } => ffi::Target::Cone(ffi_cone(cone)),
                        TargetIn::Extrusion { other } => ffi::Target::Extrusion(ffi_src(other)?),
                        TargetIn::Bspline { other } => ffi::Target::BSpline(ffi_src(other)?),
                    },
                    dom: *dom,
                    other_dom: *other_dom,
                    tol: *tol,
                    ang: *ang,
                },
                OpIn::Trim { dom } => ffi::Op::Trim { dom: *dom },
            };
            ffi::surface_op(&ffi_src(src)?, &op)
        }
    }
    .map_err(|e| harness(e.to_string()))?;
    to_output(&flat)
}

/// Splits a record into `build` and `out`.
fn to_output(flat: &[f64]) -> Result<Output, HarnessError> {
    let (b, rest) = flat
        .split_first()
        .ok_or_else(|| harness("empty record"))?;
    Ok(Output::new()
        .values("build", vec![*b])
        .values("out", rest.to_vec()))
}

// ---- Rust side ------------------------------------------------------------------------

/// A failed Rust step: deferred (unported) or an `SmStatus` written to the record.
enum Stop {
    Deferred(Box<Output>),
    Status(f64),
}

fn stop(e: &solidcore::SolidError) -> Stop {
    Output::unported(e).map_or_else(|| Stop::Status(status(e)), |d| Stop::Deferred(Box::new(d)))
}

/// The surface of a source; `Ok(Err(..))` when it cannot be made.
fn rust_src(k: &Kernel, s: &SrcIn) -> Result<Result<SurfOfExtrusion, Stop>, HarnessError> {
    if let SrcIn::Built { surf } = s {
        return Ok(rust_surf(k, surf)?.map_err(|e| stop(&e)));
    }
    let (ascii, version, bytes) = src_bytes(s)?.ok_or_else(|| harness("no record"))?;
    let mut db = DatabaseReader::new();
    db.open_for_read(
        bytes,
        if ascii {
            FileType::Ascii
        } else {
            FileType::Binary
        },
    );
    db.set_version(version);
    Ok(SurfOfExtrusion::read_from_db(k, &mut db, version).map_err(|e| stop(&e)))
}

fn rust_plane(p: &PlaneIn) -> Plane {
    match p {
        PlaneIn::Bounded {
            o,
            x,
            y,
            scale,
            dom,
        } => Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &dom4(*dom),
        ),
        PlaneIn::Normal { o, n } => Plane::from_normal(v3(*o), v3(*n)),
    }
}

fn rust_cone(k: &Kernel, c: &ConeIn) -> solidcore::Result<Cone> {
    let a2p = Axis2Placement::new(v3(c.o), v3(c.x), v3(c.y));
    match c.cone {
        ConeKindIn::Canonical {
            bot,
            top,
            height,
            swap,
            inside,
        } => Cone::create_canonical(k, &a2p, bot, top, height, swap, inside),
        ConeKindIn::Semi {
            bot,
            semi,
            height,
            swap,
            inside,
        } => Cone::create_canonical_with_semi_angle(k, &a2p, bot, semi, height, swap, inside),
        ConeKindIn::Cylinder { radius } => Cone::create_canonical_cylinder(k, &a2p, radius),
    }
}

/// The intersection outputs (see the FFI module docs).
fn put_tsect(o: &mut Vec<f64>, r: &ExtrusionIntersection) {
    o.push(flag(r.needs_more_intersections));
    o.push(r.curves_3d.len() as f64);
    for c in &r.curves_3d {
        put_curve(o, Some(c));
    }
    o.push(r.curve_types.len() as f64);
    o.extend(r.curve_types.iter().map(|t| f64::from(*t as u8)));
    o.push(r.surface1_uv_curves.len() as f64);
    o.extend(r.surface1_uv_curves.iter().map(|c| flag(c.is_some())));
    o.push(r.surface2_uv_curves.len() as f64);
    o.extend(r.surface2_uv_curves.iter().map(|c| flag(c.is_some())));
    o.push(r.deviations.len() as f64);
    o.extend_from_slice(&r.deviations);
}

/// Pushes the status of an intersection and its outputs.
fn tsect(o: &mut Vec<f64>, r: solidcore::Result<ExtrusionIntersection>) -> Result<(), Stop> {
    match r {
        Ok(r) => {
            o.push(0.0);
            put_tsect(o, &r);
            Ok(())
        }
        Err(e) => match stop(&e) {
            Stop::Status(s) => {
                o.push(s);
                Ok(())
            }
            d @ Stop::Deferred(_) => Err(d),
        },
    }
}

/// The NURB of a surface as an `SmBSplineSurface` (`SmBSplineSurface(const gw_SURFACE*)`).
fn nurb_surface(s: &SurfOfExtrusion) -> Result<BSplineSurface, HarnessError> {
    s.base()
        .gw_nurb()
        .map(BSplineSurface::from_gw_nurb)
        .ok_or_else(|| harness("source surface has no NURB"))
}

/// Pushes a sub-object's status; `Ok(None)` when it failed (status written).
fn sub<T>(o: &mut Vec<f64>, r: Result<T, Stop>) -> Result<Option<T>, Stop> {
    match r {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(Stop::Status(s)) => {
            o.push(s);
            Ok(None)
        }
        Err(d) => Err(d),
    }
}

fn rust_op(
    k: &Kernel,
    s: &mut SurfOfExtrusion,
    op: &OpIn,
    o: &mut Vec<f64>,
) -> Result<Result<(), Stop>, HarnessError> {
    let natural = |s: &SurfOfExtrusion| s.base().natural_uv_domain();
    Ok(match op {
        OpIn::State {} => {
            o.push(0.0);
            put_state(o, s).map_err(|e| stop(&e))
        }
        OpIn::Plane {
            plane,
            dom,
            other_dom,
            tol,
        } => {
            let pl = rust_plane(plane);
            let d = dom.map_or_else(|| natural(s), dom4);
            let od = other_dom.map_or_else(|| pl.natural_uv_domain(), dom4);
            tsect(o, s.intersect_with_plane(k, &d, &pl, &od, *tol))
        }
        OpIn::Cylinder { cone, tol } => {
            match sub(o, rust_cone(k, cone).map_err(|e| stop(&e))) {
                Err(d) => Err(d),
                Ok(None) => Ok(()),
                Ok(Some(c)) => tsect(o, s.intersect_with_cylinder(k, &c, *tol)),
            }
        }
        OpIn::Extrusion { other, tol } => match sub(o, rust_src(k, other)?) {
            Err(d) => Err(d),
            Ok(None) => Ok(()),
            Ok(Some(other)) => tsect(o, s.intersect_with_surf_of_extrusion(k, &other, *tol)),
        },
        OpIn::Global {
            target,
            dom,
            other_dom,
            tol,
            ang,
        } => {
            let d = dom.map_or_else(|| natural(s), dom4);
            let r = match target {
                TargetIn::Plane { plane, as_surface } => {
                    o.push(0.0);
                    let pl = rust_plane(plane);
                    let od = other_dom.map_or_else(|| pl.natural_uv_domain(), dom4);
                    if *as_surface {
                        let srf = Surface::from(pl);
                        s.global_surface_intersect(
                            k,
                            &d,
                            IntersectTarget::Surface(&srf),
                            &od,
                            [true, true],
                            *tol,
                            *ang,
                        )
                    } else {
                        s.global_surface_intersect(
                            k,
                            &d,
                            IntersectTarget::Plane(&pl),
                            &od,
                            [true, true],
                            *tol,
                            *ang,
                        )
                    }
                }
                TargetIn::Cone { cone } => {
                    match sub(o, rust_cone(k, cone).map_err(|e| stop(&e))) {
                        Err(d) => return Ok(Err(d)),
                        Ok(None) => return Ok(Ok(())),
                        Ok(Some(c)) => {
                            let od = other_dom.map_or_else(
                                || c.rev().base().natural_uv_domain(),
                                dom4,
                            );
                            s.global_surface_intersect(
                                k,
                                &d,
                                IntersectTarget::Cone(&c),
                                &od,
                                [true, true],
                                *tol,
                                *ang,
                            )
                        }
                    }
                }
                TargetIn::Extrusion { other } => match sub(o, rust_src(k, other)?) {
                    Err(d) => return Ok(Err(d)),
                    Ok(None) => return Ok(Ok(())),
                    Ok(Some(e)) => {
                        let od = other_dom.map_or_else(|| natural(&e), dom4);
                        s.global_surface_intersect(
                            k,
                            &d,
                            IntersectTarget::SurfOfExtrusion(&e),
                            &od,
                            [true, true],
                            *tol,
                            *ang,
                        )
                    }
                },
                TargetIn::Bspline { other } => match sub(o, rust_src(k, other)?) {
                    Err(d) => return Ok(Err(d)),
                    Ok(None) => return Ok(Ok(())),
                    Ok(Some(e)) => {
                        let b = nurb_surface(&e)?;
                        let od = other_dom.map_or_else(|| b.natural_uv_domain(), dom4);
                        let srf = Surface::from(b);
                        s.global_surface_intersect(
                            k,
                            &d,
                            IntersectTarget::Surface(&srf),
                            &od,
                            [true, true],
                            *tol,
                            *ang,
                        )
                    }
                },
            };
            tsect(o, r)
        }
        OpIn::Trim { dom } => match s.trim_with_domain(k, &dom4(*dom)) {
            Ok(()) => {
                o.push(0.0);
                put_state(o, s).map_err(|e| stop(&e))
            }
            Err(e) => match stop(&e) {
                Stop::Status(st) => {
                    o.push(st);
                    Ok(())
                }
                d @ Stop::Deferred(_) => Err(d),
            },
        },
    })
}

/// The test surface of `IsNurbSurfaceSurfOfExtrusion`; `Ok(Err(..))` when a step
/// fails (statuses written).
fn rust_test(
    k: &Kernel,
    t: &TestIn,
    o: &mut Vec<f64>,
) -> Result<Result<Option<BSplineSurface>, Stop>, HarnessError> {
    let mut b = match &t.source {
        TestSourceIn::Raw { nurb } => {
            let mut net = Vec::new();
            for i in 0..=nurb.n {
                let row = (0..=nurb.m)
                    .map(|j| {
                        nurb.cpts
                            .get(i * (nurb.m + 1) + j)
                            .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                            .ok_or_else(|| harness("test NURB: too few points"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                net.push(row);
            }
            let sur = NurbSurface::new(net, nurb.p, nurb.q, nurb.u.clone(), nurb.v.clone())
                .map_err(|e| harness(format!("test NURB: {e}")))?;
            o.push(0.0);
            BSplineSurface::from_nurb(sur, false)
        }
        TestSourceIn::Src { of } => match rust_src(k, of)? {
            Ok(s) => {
                o.push(0.0);
                nurb_surface(&s)?
            }
            Err(Stop::Status(st)) => {
                o.push(st);
                return Ok(Ok(None));
            }
            Err(d) => return Ok(Err(d)),
        },
    };
    let push = |o: &mut Vec<f64>, r: solidcore::Result<()>| -> Result<(), Stop> {
        match r {
            Ok(()) => {
                o.push(0.0);
                Ok(())
            }
            Err(e) => match stop(&e) {
                Stop::Status(st) => {
                    o.push(st);
                    Ok(())
                }
                d @ Stop::Deferred(_) => Err(d),
            },
        }
    };
    if t.transpose {
        if let Err(d) = push(o, b.swap_uv()) {
            return Ok(Err(d));
        }
    }
    if let Some(d) = t.reparam {
        if let Err(d) = push(o, b.reparameterize(&dom4(d))) {
            return Ok(Err(d));
        }
    }
    if let Some((i, j, d)) = t.perturb {
        let p = b
            .get_or_create_gw_nurb()
            .and_then(|sur| sur.cpt_mut(i, j))
            .map_err(|_| harness("perturbation outside the test NURB"))?;
        p.x += d[0];
        p.y += d[1];
        p.z += d[2];
    }
    Ok(Ok(Some(b)))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match i {
        Inputs::IsNurb { is_nurb, scale } => match rust_test(&k, is_nurb, &mut o)? {
            Err(Stop::Deferred(d)) => return Ok(*d),
            Err(Stop::Status(_)) => return Err(harness("unexpected status stop")),
            Ok(None) => {}
            Ok(Some(t)) => {
                match SurfOfExtrusion::is_nurb_surface_surf_of_extrusion(&k, &t, *scale) {
                    Ok(Some(mut s)) => {
                        // whether the NURB was made, then the state with it made
                        let had = s.base().gw_nurb().is_some();
                        o.extend([0.0, 1.0, flag(had)]);
                        let made = if had { Ok(()) } else { s.make_nurb(&k) };
                        match made {
                            Ok(()) => {
                                o.push(0.0);
                                put_state(&mut o, &s).map_err(|e| harness(e.to_string()))?;
                            }
                            Err(e) => {
                                if let Some(d) = Output::unported(&e) {
                                    return Ok(d);
                                }
                                o.push(status(&e));
                            }
                        }
                    }
                    Ok(None) => o.extend([0.0, 0.0]),
                    Err(e) => {
                        if let Some(d) = Output::unported(&e) {
                            return Ok(d);
                        }
                        o.push(status(&e));
                    }
                }
            }
        },
        Inputs::Op { src, op } => match rust_src(&k, src)? {
            Err(Stop::Deferred(d)) => return Ok(*d),
            Err(Stop::Status(st)) => o.push(st),
            Ok(mut s) => {
                o.push(0.0);
                if let Err(stop) = rust_op(&k, &mut s, op, &mut o)? {
                    match stop {
                        Stop::Deferred(d) => return Ok(*d),
                        Stop::Status(_) => return Err(harness("unexpected status stop")),
                    }
                }
            }
        },
    }
    to_output(&o)
}

// ---- generation -----------------------------------------------------------------------

/// A built source with its Rust surface (to size the parameters).
fn gen_built(rng: &mut Rng, k: &Kernel) -> Option<(SurfIn, SurfOfExtrusion)> {
    for _ in 0..8 {
        let s = gen_surf(rng);
        if let Ok(Ok(b)) = rust_surf(k, &s) {
            return Some((s, b));
        }
    }
    None
}

/// A sample range of the STEP domain (the infinite extrusion clamped).
fn step_box(b: &SurfOfExtrusion) -> [f64; 4] {
    let d = b.step_uv_domain();
    let (v0, v1) = if d.max().y - d.min().y > 1e5 {
        (-3.0, 3.0)
    } else {
        (d.min().y, d.max().y)
    };
    [d.min().x, v0, d.max().x, v1]
}

/// A point of the surface at fractions `(a, b)` of its sample range.
fn surf_point(k: &Kernel, b: &SurfOfExtrusion, a: f64, c: f64) -> Option<[f64; 3]> {
    let [u0, v0, u1, v1] = step_box(b);
    let p = b
        .evaluate_step_point(k, Point2::new(u0 + a * (u1 - u0), v0 + c * (v1 - v0)))
        .ok()?;
    Some([p.x, p.y, p.z])
}

fn as_arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A plane through `o` with normal `n`: infinite, or bounded around `o` (a few sizes).
fn gen_plane(rng: &mut Rng, o: [f64; 3], n: [f64; 3], size: f64) -> PlaneIn {
    if rng.chance(0.4) {
        return PlaneIn::Normal { o, n };
    }
    let nn = normalize(n);
    let t = unit_vec(rng);
    let mut y = cross3(nn, t);
    if (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt() < 1e-3 {
        y = cross3(nn, [1.0, 0.0, 0.0]);
    }
    let y = normalize(y);
    let x = cross3(y, nn);
    let half = size * rng.uniform(0.3, 8.0);
    let sc = if rng.chance(0.2) {
        [rng.uniform(0.5, 3.0), rng.uniform(0.5, 3.0)]
    } else {
        [1.0, 1.0]
    };
    PlaneIn::Bounded {
        o,
        x,
        y,
        scale: sc,
        dom: [-half, -half * 0.7, half, half * 1.3],
    }
}

fn gen_tol(rng: &mut Rng) -> Option<f64> {
    match rng.below(10) {
        0 => Some(0.0),
        1 => Some(1e-3),
        2 => Some(-1e-6),
        3 => Some(1e-12),
        _ => None,
    }
}

#[allow(clippy::too_many_lines)] // one plane case generator per branch
fn gen_plane_op(rng: &mut Rng, k: &Kernel, b: &SurfOfExtrusion) -> Option<OpIn> {
    let e = as_arr(b.extrusion_vector());
    let el = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
    if el < 1e-12 {
        return None;
    }
    let en = scale(e, 1.0 / el);
    let size = {
        let [u0, v0, u1, v1] = step_box(b);
        let p0 = surf_point(k, b, 0.0, 0.0)?;
        let p1 = surf_point(k, b, 1.0, 1.0)?;
        let _ = (u0, v0, u1, v1);
        let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-3)
    };
    let cv = as_arr(b.curve_vec());
    let plane = match rng.below(10) {
        // parallel to the extrusion: directrices through the generator hits
        0..=4 => {
            let mut n = cross3(en, unit_vec(rng));
            if (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() < 1e-3 {
                n = cross3(en, [0.0, 0.0, 1.0]);
            }
            let n = normalize(n);
            let a = match rng.below(6) {
                0 => 0.0,
                1 => 1.0,
                _ => rng.unit(),
            };
            let p = surf_point(k, b, a, rng.unit())?;
            let off = match rng.below(5) {
                0 => 0.0,
                1 => size * 1e-9,
                _ => rng.uniform(-0.3, 0.3) * size,
            };
            gen_plane(rng, add(p, n, off), n, size)
        }
        // parallel to the generator's plane (perpendicular / independent generators)
        5..=7 if cv.iter().all(|x| x.is_finite()) => {
            let n = if rng.chance(0.5) { cv } else { scale(cv, -1.0) };
            let c = match rng.below(6) {
                0 => -0.3,
                1 => 1.3,
                2 => 0.0,
                3 => 1.0,
                _ => rng.unit(),
            };
            let p = surf_point(k, b, rng.unit(), c.clamp(0.0, 1.0))?;
            let shift = if c < 0.0 {
                -0.3 * size
            } else if c > 1.0 {
                0.3 * size
            } else if rng.chance(0.2) {
                1e-9 * size
            } else {
                0.0
            };
            let o = add(p, en, shift);
            if rng.chance(0.35) {
                // a bounded plane away from the generator: the pseudo boxes are disjoint
                let away = normalize(cross3(normalize(n), unit_vec(rng)));
                let PlaneIn::Bounded { x, y, scale, .. } = gen_plane(rng, o, n, size) else {
                    return None;
                };
                let half = 0.5 * size;
                PlaneIn::Bounded {
                    o: add(o, away, rng.uniform(5.0, 50.0) * size),
                    x,
                    y,
                    scale,
                    dom: [-half, -half, half, half],
                }
            } else {
                gen_plane(rng, o, n, size)
            }
        }
        // oblique
        _ => {
            let n = normalize(add(en, unit_vec(rng), rng.uniform(0.2, 2.0)));
            let p = surf_point(k, b, rng.unit(), rng.unit())?;
            gen_plane(rng, p, n, size)
        }
    };
    let dom = if rng.chance(0.15) {
        let d = b.base().natural_uv_domain();
        Some([d.min().x, d.min().y, d.max().x, d.max().y])
    } else {
        None
    };
    Some(OpIn::Plane {
        plane,
        dom,
        other_dom: None,
        tol: gen_tol(rng),
    })
}

/// A cone near the surface. For `IntersectWithCylinder` (`for_cylinder`) only cones whose
/// bottom iso circle upstream makes from defined values: an unbounded cylinder puts the
/// circle at `-SM_INFINITE_PARAMETER`, where the STEP inversion leaves its outputs
/// unset (the shim rejects those).
fn gen_cone(
    rng: &mut Rng,
    k: &Kernel,
    b: &SurfOfExtrusion,
    parallel: bool,
    for_cylinder: bool,
) -> Option<ConeIn> {
    let e = as_arr(b.extrusion_vector());
    if (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt() < 1e-12 {
        return None;
    }
    let e = normalize(e);
    let z = if parallel {
        if rng.chance(0.5) {
            e
        } else {
            scale(e, -1.0)
        }
    } else {
        unit_vec(rng)
    };
    let t = unit_vec(rng);
    let mut x = cross3(t, z);
    if (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt() < 1e-3 {
        x = cross3([1.0, 0.0, 0.0], z);
    }
    let x = normalize(x);
    let y = cross3(z, x);
    let o = surf_point(k, b, rng.unit(), rng.unit())?;
    let o = add(o, x, rng.uniform(-1.0, 1.0));
    let r = rng.uniform(0.2, 2.0);
    let cone = match rng.below(6) {
        0..=2 => ConeKindIn::Canonical {
            bot: r,
            top: if rng.chance(0.7) {
                r
            } else {
                r * rng.uniform(0.3, 1.8)
            },
            height: rng.uniform(0.5, 4.0),
            swap: rng.chance(0.3),
            inside: rng.chance(0.2),
        },
        3 => ConeKindIn::Canonical {
            bot: r,
            top: r * (1.0 + 1e-17),
            height: rng.uniform(0.5, 4.0),
            swap: false,
            inside: false,
        },
        4 => {
            let semi = if rng.chance(0.5) {
                0.0
            } else {
                rng.uniform(5.0, 40.0)
            };
            let height = if for_cylinder && semi == 0.0 {
                Some(rng.uniform(0.5, 3.0))
            } else {
                rng.chance(0.7).then(|| rng.uniform(0.5, 3.0))
            };
            ConeKindIn::Semi {
                bot: r,
                semi,
                height,
                swap: rng.chance(0.3),
                inside: rng.chance(0.2),
            }
        }
        _ if for_cylinder => ConeKindIn::Canonical {
            bot: r,
            top: r,
            height: rng.uniform(1e-3, 10.0),
            swap: rng.chance(0.5),
            inside: rng.chance(0.5),
        },
        _ => ConeKindIn::Cylinder { radius: r },
    };
    [o, x, y]
        .iter()
        .flatten()
        .all(|v| v.is_finite())
        .then_some(ConeIn { cone, o, x, y })
}

/// Another built surface swept along `b`'s extrusion (or not).
fn gen_other_ext(rng: &mut Rng, b: &SurfOfExtrusion, parallel: bool) -> SurfIn {
    let size = 1.0;
    let f: Frame = frame(rng, size);
    let gen = gen_generator(rng, &f, true, size);
    let e = as_arr(b.extrusion_vector());
    let vec = if parallel {
        scale(e, rng.uniform(-2.0, 2.0))
    } else {
        let (_, _, _, z) = f;
        z
    };
    SurfIn {
        gen,
        build: BuildIn::Canonical { vec },
        oob: false,
        swap: false,
    }
}

/// A record written from a built surface's data, with varied fields.
fn gen_record(rng: &mut Rng, k: &Kernel) -> Option<RecordIn> {
    let (_, b) = gen_built(rng, k)?;
    let gen = b.gen_curve()?;
    // the generator's NURB as an SmBSplineCurve record
    let gnurb = nurb_of(gen)?;
    let rat = gnurb.cpts().iter().any(|p| p.w != NOW);
    let pts: Vec<Vec<f64>> = gnurb
        .cpts()
        .iter()
        .map(|p| {
            if rat {
                vec![p.x, p.y, p.z, p.w]
            } else {
                vec![p.x, p.y, p.z]
            }
        })
        .collect();
    let gen_rec = CurveRec {
        p: gnurb.degree() as i16,
        rat: i16::from(rat),
        dim: 3,
        kt: rng.below(5),
        form: rng.below(7),
        pts,
        knots: gnurb.knots().to_vec(),
    };
    let sur = b.base().gw_nurb()?;
    let ix = sur.indices();
    let srat = sur.rows().flatten().any(|p| p.w != NOW);
    let spts: Vec<Vec<f64>> = sur
        .rows()
        .flatten()
        .map(|p| {
            if srat {
                vec![p.x, p.y, p.z, p.w]
            } else {
                vec![p.x, p.y, p.z]
            }
        })
        .collect();
    let surf = SurfRec {
        n: ix.n as i64,
        m: ix.m as i64,
        p: ix.p as i16,
        q: ix.q as i16,
        rat: i16::from(srat),
        kt: rng.below(5),
        form: rng.below(12),
        pts: spts,
        u: sur.u_knots().to_vec(),
        v: sur.v_knots().to_vec(),
    };
    let d = b.step_uv_domain();
    let mut anal = [d.min().x, d.min().y, d.max().x, d.max().y];
    if rng.chance(0.1) {
        // inverted: SetMinMax refuses it
        anal.swap(0, 2);
    }
    Some(RecordIn {
        binary: rng.chance(0.5),
        version: rng.choose(VERSIONS).copied().unwrap_or(38),
        origin: as_arr(b.origin()),
        vec: as_arr(b.extrusion_vector()),
        anal,
        orient: if rng.chance(0.1) {
            rng.below(6)
        } else {
            b.orientation() as u64
        },
        swap: match rng.below(10) {
            0 => 2,
            1 => -1,
            _ => i32::from(b.swap_uv()),
        },
        cp: as_arr(b.curve_point()).map(|x| if x.is_finite() { x } else { 0.0 }),
        cv: as_arr(b.curve_vec()).map(|x| if x.is_finite() { x } else { 0.0 }),
        gen: gen_rec,
        surf,
    })
}

#[allow(clippy::too_many_lines)] // one case generator per op
fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let pick = rng.below(100);
    if pick < 8 {
        // database records
        let rec = gen_record(rng, &k)?;
        let op = OpIn::State {};
        return Some(Inputs::Op {
            src: SrcIn::Record { rec: Box::new(rec) },
            op: Box::new(op),
        });
    }
    if pick < 18 {
        // IsNurbSurfaceSurfOfExtrusion
        let (s, b) = gen_built(rng, &k)?;
        let sur = b.base().gw_nurb()?;
        let ix = sur.indices();
        let perturb = rng.chance(0.25).then(|| {
            let i = rng.below(ix.n as u64 + 1) as usize;
            let j = rng.below(ix.m as u64 + 1) as usize;
            let mag = match rng.below(4) {
                0 => 1e-12,
                1 => 1e-9,
                2 => 1e-6,
                _ => 0.1,
            };
            let d = scale(unit_vec(rng), mag);
            (i, j, d)
        });
        let transpose = rng.chance(0.3);
        let perturb = perturb.map(|(i, j, d)| if transpose { (j, i, d) } else { (i, j, d) });
        let reparam = rng.chance(0.3).then(|| {
            let a = rng.uniform(-5.0, 5.0);
            let c = rng.uniform(-5.0, 5.0);
            [a, c, a + rng.uniform(0.5, 4.0), c + rng.uniform(0.5, 4.0)]
        });
        let scale_t = match rng.below(6) {
            0 => 0.0,
            1 => 1e3,
            2 => 1e-3,
            _ => 1.0,
        };
        return Some(Inputs::IsNurb {
            is_nurb: TestIn {
                source: TestSourceIn::Src {
                    of: Box::new(SrcIn::Built { surf: s }),
                },
                transpose,
                reparam,
                perturb,
            },
            scale: scale_t,
        });
    }
    let (s, b) = gen_built(rng, &k)?;
    let op = match pick {
        18..=59 => gen_plane_op(rng, &k, &b)?,
        60..=69 => {
            let parallel = rng.chance(0.3);
            let tol = match rng.below(5) {
                0 => Some(1e-3),
                1 => Some(-1e-6),
                2 => Some(1e-6),
                _ => None,
            };
            OpIn::Cylinder {
                cone: gen_cone(rng, &k, &b, parallel, true)?,
                tol,
            }
        }
        70..=77 => {
            let parallel = rng.chance(0.4);
            OpIn::Extrusion {
                other: Box::new(SrcIn::Built {
                    surf: gen_other_ext(rng, &b, parallel),
                }),
                tol: gen_tol(rng),
            }
        }
        78..=89 => {
            let target = match rng.below(5) {
                0 | 1 => {
                    let OpIn::Plane { plane, .. } = gen_plane_op(rng, &k, &b)? else {
                        return None;
                    };
                    TargetIn::Plane {
                        plane,
                        as_surface: rng.chance(0.5),
                    }
                }
                2 => TargetIn::Cone {
                    cone: gen_cone(rng, &k, &b, false, true)?,
                },
                3 => TargetIn::Extrusion {
                    other: Box::new(SrcIn::Built {
                        surf: gen_other_ext(rng, &b, false),
                    }),
                },
                _ => TargetIn::Bspline {
                    other: Box::new(SrcIn::Built {
                        surf: gen_other_ext(rng, &b, false),
                    }),
                },
            };
            // (a cone's bottom iso circle needs a tolerance its inversion can meet)
            let tol = if matches!(target, TargetIn::Cone { .. }) {
                rng.chance(0.5).then_some(1e-6)
            } else {
                gen_tol(rng)
            };
            OpIn::Global {
                target,
                dom: None,
                other_dom: None,
                tol,
                ang: rng.chance(0.3).then(|| rng.uniform(0.01, 1.0)),
            }
        }
        _ => {
            let d = b.base().natural_uv_domain();
            let (u0, v0, u1, v1) = (d.min().x, d.min().y, d.max().x, d.max().y);
            let (lu, lv) = (u1 - u0, v1 - v0);
            let dom = match rng.below(8) {
                0 => [u0, v0, u1, v1],
                1 => [u0 - 1e-9, v0, u1, v1 + 1e-9],
                2 => [u0 - 0.1 * lu, v0, u1, v1],
                3 => [u0 + 0.5 * lu, v0, u1, v1],
                _ => {
                    let a = u0 + rng.uniform(0.0, 0.4) * lu;
                    let c = v0 + rng.uniform(0.0, 0.4) * lv;
                    [a, c, a + rng.uniform(0.2, 0.6) * lu, c + rng.uniform(0.2, 0.6) * lv]
                }
            };
            OpIn::Trim { dom }
        }
    };
    Some(Inputs::Op {
        src: SrcIn::Built { surf: s },
        op: Box::new(op),
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surf_of_extrusion_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..8 {
            // (a non-finite number would not survive the JSON round trip)
            if let Some(v) = gen_case(rng).and_then(|i| serde_json::to_value(i).ok()) {
                if serde_json::from_value::<Inputs>(v.clone()).is_ok() {
                    return Some(v);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn op_name(i: &Inputs) -> String {
        match i {
            Inputs::IsNurb { .. } => "is_nurb".into(),
            Inputs::Op { src, op } => {
                let s = match src {
                    SrcIn::Built { .. } => "",
                    _ => "record:",
                };
                let o = match &**op {
                    OpIn::State {} => "state".to_string(),
                    OpIn::Plane { .. } => "plane".into(),
                    OpIn::Cylinder { .. } => "cylinder".into(),
                    OpIn::Extrusion { .. } => "extrusion".into(),
                    OpIn::Global { target, .. } => format!(
                        "global:{}",
                        match target {
                            TargetIn::Plane { .. } => "plane",
                            TargetIn::Cone { .. } => "cone",
                            TargetIn::Extrusion { .. } => "extrusion",
                            TargetIn::Bspline { .. } => "bspline",
                        }
                    ),
                    OpIn::Trim { .. } => "trim".into(),
                };
                format!("{s}{o}")
            }
        }
    }

    /// What the Rust side answers: deferred, a failed source, a failed op, or the
    /// op's first outputs (the intersections' `more` flag and curve count).
    fn outcome(i: &Inputs, out: &Output) -> String {
        if let Some(d) = &out.deferred {
            return format!("deferred:{}", d.unit);
        }
        let b = out.values.get("build").and_then(|v| v.first()).copied();
        let o = out.values.get("out").cloned().unwrap_or_default();
        if b != Some(0.0) {
            return format!("build:{b:?}");
        }
        match i {
            Inputs::IsNurb { .. } => format!("{:?}", o.get(..2)),
            Inputs::Op { op, .. } => match &**op {
                OpIn::Plane { .. } => format!("{:?}", o.get(..3)),
                OpIn::Cylinder { .. } | OpIn::Extrusion { .. } | OpIn::Global { .. } => {
                    format!("{:?}", o.get(..4))
                }
                _ => format!("{:?}", o.first()),
            },
        }
    }

    /// The generators reach every op, with answers that are compared (not deferred),
    /// curves found, statuses and needs-more answers.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..1200u64 {
            let mut rng = Rng::new(0x5eed_1000 + seed);
            let Some(i) = gen_case(&mut rng) else {
                continue;
            };
            let out = rust(&i).expect("harness");
            *tally.entry((op_name(&i), outcome(&i, &out))).or_default() += 1;
        }
        for (k, v) in &tally {
            eprintln!("{k:?} {v}");
        }
        // (GlobalSurfaceIntersect with a cone, an extrusion or a B-spline surface always
        // reaches the general intersector or the projected curve solve: deferred)
        for op in [
            "plane",
            "cylinder",
            "extrusion",
            "global:plane",
            "trim",
            "is_nurb",
            "record:state",
        ] {
            let compared = tally
                .iter()
                .any(|((o, k), _)| o == op && !k.starts_with("deferred"));
            assert!(compared, "{op} is never compared");
        }
    }
}
