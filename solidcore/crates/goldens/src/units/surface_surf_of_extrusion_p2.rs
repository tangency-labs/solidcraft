//! Runner for unit `surface.surf_of_extrusion_p2`: `SmSurfOfExtrusion` part 2
//! (`IntersectWithPlane`, `IntersectWithCylinder`, `IntersectWithSurfOfExtrusion`,
//! `GlobalSurfaceIntersect`, `IsNurbSurfaceSurfOfExtrusion`, `TrimWithDomain`,
//! `ReadFromDB`).
//!
//! Ported from upstream `source/SMLib/src/SmSurfOfExtrusion.cpp`; checked against its
//! golden. Rust:
//! `solidcore::surface::surf_of_extrusion::p2`.
//!
//! A case is a surface source `src` (a surface built as in the
//! `surface.surf_of_extrusion_p1` runner, a database record written from data, or the
//! record of a surface of extrusion in an upstream test file) plus `op` and its
//! parameters; `is_nurb` cases have a test surface instead (with a `query`, an
//! `SmSurface` query runs on the recognised surface while its NURB is still lazy, through
//! the common `Surface` dispatch on the Rust side). The Rust side produces a flat
//! record, split into `build` (the source status)
//! and `out` (the rest). Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`; `-1` on the
//! Rust side for an error upstream cannot produce.
//!
//! Deferred cases: `IntersectWithPlane` of a linear or coplanar generator whose
//! plane/plane intersection gives a line, while `SmSurface::CurveOnClassify`
//! (`intersect.curve_class_p2`) is unported (a bounded plane the swept plane misses
//! compares). `GlobalSurfaceIntersect` runs through the common `Surface` dispatch (a plane not
//! `as_surface` through the typed `SurfOfExtrusion::global_surface_intersect`), which
//! takes the general intersector where a special case needs more.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_extrusion::p2::{ExtrusionIntersection, IntersectTarget};
use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
use solidcore::surface::surface::p1::crossed_seam_or_pole;
use solidcore::surface::surface::Surface;
use solidcore::topology::database_io::{DatabaseReader, FileType};

use super::surface_surf_of_extrusion_p1::{
    add, cross3, dom4, flag, frame, gen_generator, gen_surf, harness, normalize, nurb_of,
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

/// An `SmSurface` query on a recognised surface whose NURB is still lazy (`OP_LAZY`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
enum QueryIn {
    EvalSimple {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default)]
        u_left: bool,
        #[serde(default)]
        v_left: bool,
    },
    EvalFirst {
        uv: [f64; 2],
        #[serde(default)]
        u_left: bool,
        #[serde(default)]
        v_left: bool,
    },
    BBox {
        dom: [f64; 4],
        #[serde(default)]
        expand: bool,
    },
    DegenPoint {
        tol: f64,
        #[serde(default)]
        dom: Option<[f64; 4]>,
    },
    DegenCurve {
        ang: f64,
        #[serde(default)]
        dom: Option<[f64; 4]>,
        tol: f64,
    },
    Planar {
        tol: f64,
    },
    NumCpts {
        dir: u8,
    },
    Bilinear,
    Parallel {
        v: [f64; 3],
    },
    Knots {
        dir: u8,
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    Seam {
        prev: [f64; 2],
        curr: [f64; 2],
        dom: [f64; 4],
    },
    TessSplits {
        dir: u8,
        dom: [f64; 4],
        chord: f64,
        angle: f64,
        max_side: f64,
        #[serde(default)]
        diagonals: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Inputs {
    IsNurb {
        is_nurb: TestIn,
        scale: f64,
        /// Run this query on the recognised surface (its NURB not made) instead of
        /// making the NURB and reporting the state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<QueryIn>,
    },
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
            put_state(k, o, s).map_err(|e| stop(&e))
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
            // SmSurfOfExtrusion::GlobalSurfaceIntersect: its special cases, else the
            // general intersection (SmSurface::GlobalSurfaceIntersect) of both surfaces.
            // A plane not `as_surface` takes the typed entry; every other target goes
            // through the common Surface dispatch.
            let d = dom.map_or_else(|| natural(s), dom4);
            let this = Surface::from(s.clone());
            let (other, od) = match target {
                TargetIn::Plane { plane, as_surface } => {
                    o.push(0.0);
                    let pl = rust_plane(plane);
                    let od = other_dom.map_or_else(|| pl.natural_uv_domain(), dom4);
                    if !*as_surface {
                        let r = s.global_surface_intersect(
                            k,
                            &d,
                            IntersectTarget::Plane(&pl),
                            &od,
                            [true, true],
                            *tol,
                            *ang,
                        );
                        return Ok(tsect(o, r));
                    }
                    (Surface::from(pl), od)
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
                            (Surface::from(c), od)
                        }
                    }
                }
                TargetIn::Extrusion { other } => match sub(o, rust_src(k, other)?) {
                    Err(d) => return Ok(Err(d)),
                    Ok(None) => return Ok(Ok(())),
                    Ok(Some(e)) => {
                        let od = other_dom.map_or_else(|| natural(&e), dom4);
                        (Surface::from(e), od)
                    }
                },
                TargetIn::Bspline { other } => match sub(o, rust_src(k, other)?) {
                    Err(d) => return Ok(Err(d)),
                    Ok(None) => return Ok(Ok(())),
                    Ok(Some(e)) => {
                        let b = nurb_surface(&e)?;
                        let od = other_dom.map_or_else(|| b.natural_uv_domain(), dom4);
                        (Surface::from(b), od)
                    }
                },
            };
            let r = this
                .global_surface_intersect(k, &d, &other, &od, [true, true], *tol, *ang)
                .map(|x| ExtrusionIntersection {
                    needs_more_intersections: false,
                    curves_3d: x.curves_3d,
                    surface1_uv_curves: x.surface1_uv_curves,
                    surface2_uv_curves: x.surface2_uv_curves,
                    curve_types: x.curve_types,
                    deviations: x.deviations,
                });
            tsect(o, r)
        }
        OpIn::Trim { dom } => match s.trim_with_domain(k, &dom4(*dom)) {
            Ok(()) => {
                o.push(0.0);
                put_state(k, o, s).map_err(|e| stop(&e))
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
        Inputs::IsNurb {
            is_nurb,
            scale,
            query,
        } => match rust_test(&k, is_nurb, &mut o)? {
            Err(Stop::Deferred(d)) => return Ok(*d),
            Err(Stop::Status(_)) => return Err(harness("unexpected status stop")),
            Ok(None) => {}
            Ok(Some(t)) => {
                match SurfOfExtrusion::is_nurb_surface_surf_of_extrusion(&k, &t, *scale) {
                    Ok(Some(mut s)) => {
                        // whether the NURB was made, then the state with it made
                        let had = s.base().gw_nurb().is_some();
                        o.extend([0.0, 1.0, flag(had)]);
                        if let Some(q) = query {
                            if let Err(d) = rust_query(&k, &Surface::from(s), q, &mut o) {
                                return Ok(*d);
                            }
                            return to_output(&o);
                        }
                        let made = if had { Ok(()) } else { s.make_nurb(&k) };
                        match made {
                            Ok(()) => {
                                o.push(0.0);
                                put_state(&k, &mut o, &s).map_err(|e| harness(e.to_string()))?;
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

/// A query's status and outputs (layouts in the FFI docs); `Err` with the (boxed) deferred output
/// for an unported callee.
fn rust_query(k: &Kernel, s: &Surface, q: &QueryIn, o: &mut Vec<f64>) -> Result<(), Box<Output>> {
    fn st<T>(o: &mut Vec<f64>, r: solidcore::Result<T>) -> Result<Option<T>, Box<Output>> {
        match r {
            Ok(v) => {
                o.push(0.0);
                Ok(Some(v))
            }
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Err(Box::new(d));
                }
                o.push(status(&e));
                Ok(None)
            }
        }
    }
    fn truth(o: &mut Vec<f64>, r: solidcore::Result<bool>) -> Result<(), Box<Output>> {
        match r {
            Ok(b) => o.push(flag(b)),
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Err(Box::new(d));
                }
                o.push(-1.0);
            }
        }
        Ok(())
    }
    let flags = |u_left: bool, v_left: bool| EvaluateFlags {
        u_side: if u_left { Side::Left } else { Side::Right },
        v_side: if v_left { Side::Left } else { Side::Right },
        only_upper_half: false,
        non_zero_tangents: true,
    };
    let opt = |d: Option<[f64; 4]>| d.map(dom4);
    match *q {
        QueryIn::EvalSimple {
            uv,
            hu,
            hv,
            u_left,
            v_left,
        } => {
            let r = s.evaluate_simple(k, Point2::new(uv[0], uv[1]), hu, hv, flags(u_left, v_left));
            if let Some(m) = st(o, r)? {
                for row in m {
                    for v in row {
                        o.extend([v.x, v.y, v.z]);
                    }
                }
            }
        }
        QueryIn::EvalFirst { uv, u_left, v_left } => {
            let mut d = [[Vec3::new(0.0, 0.0, 0.0); 2]; 2];
            let r = s.evaluate_simple_first_into(
                k,
                Point2::new(uv[0], uv[1]),
                flags(u_left, v_left),
                &mut d,
            );
            if st(o, r)?.is_some() {
                for v in d.iter().flatten() {
                    o.extend([v.x, v.y, v.z]);
                }
            }
        }
        QueryIn::BBox { dom, expand } => {
            let r = s.calculate_bounding_box(k, &dom4(dom), [true, false, false], expand);
            if let Some(b) = st(o, r)? {
                let b = b.normal_box.unwrap_or_default();
                for p in [b.min(), b.max()] {
                    o.extend([p.x, p.y, p.z]);
                }
            }
        }
        QueryIn::DegenPoint { tol, dom } => {
            truth(o, s.is_degenerate_point(k, tol, opt(dom).as_ref()))?;
        }
        QueryIn::DegenCurve { ang, dom, tol } => {
            truth(o, s.is_degenerate_curve(k, ang, opt(dom).as_ref(), tol))?;
        }
        QueryIn::Planar { tol } => match s.is_planar(k, tol) {
            Ok(p) => {
                o.push(flag(p.is_planar));
                o.extend([p.point.x, p.point.y, p.point.z]);
                o.extend([p.normal.x, p.normal.y, p.normal.z, p.deviation]);
            }
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Err(Box::new(d));
                }
                o.push(-1.0);
            }
        },
        QueryIn::NumCpts { dir } => {
            let n = param_code(dir).map_or(0, |p| s.number_control_points(k, p));
            o.push(n as f64);
        }
        QueryIn::Bilinear => o.push(flag(s.is_bilinear(k))),
        QueryIn::Parallel { v } => {
            truth(o, s.is_parallel_to_vector(k, Vec3::new(v[0], v[1], v[2])))?;
        }
        QueryIn::Knots { dir, ivl } => {
            let ivl = ivl.map(|[a, b]| Extent1d::from_min_max(a, b));
            let r = param_code(dir).and_then(|p| s.knots_with_nurb(k, p, ivl.as_ref()));
            if let Some((knots, mults)) = st(o, r)? {
                o.push(knots.len() as f64);
                o.extend_from_slice(&knots);
                o.push(mults.len() as f64);
                o.extend(mults.iter().map(|&m| m as f64));
            }
        }
        QueryIn::Seam { prev, curr, dom } => {
            let r = crossed_seam_or_pole(
                k,
                s,
                Point2::new(prev[0], prev[1]),
                Point2::new(curr[0], curr[1]),
                &dom4(dom),
            );
            truth(o, r)?;
        }
        QueryIn::TessSplits {
            dir,
            dom,
            chord,
            angle,
            max_side,
            diagonals,
        } => {
            let r = param_code(dir).and_then(|p| {
                s.find_tessellation_splits(k, p, &dom4(dom), chord, angle, max_side, diagonals)
            });
            if let Some(splits) = st(o, r)? {
                o.push(splits.len() as f64);
                o.extend_from_slice(&splits);
            }
        }
    }
    Ok(())
}

/// The `SmSurfParamType` of code `c`.
fn param_code(c: u8) -> solidcore::Result<SurfParamType> {
    SurfParamType::from_code(u64::from(c))
        .ok_or_else(|| solidcore::SolidError::invalid_input("difftest", "parameter type"))
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
            query: None,
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

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::math::EFF_ZERO_SQRT;
    use std::collections::BTreeMap;

    /// The generator segment's start, its direction, the sweep vector and the plane of
    /// `gen1053`, from its inputs.
    fn gen1053_frame(v: &Value) -> [Vec3; 6] {
        let p = |x: &Value| Vec3::new(x[0].as_f64().unwrap(), x[1].as_f64().unwrap(), x[2].as_f64().unwrap());
        let cpts = &v["surf"]["gen"]["cpts"];
        let (c0, c1) = (p(&cpts[0]), p(&cpts[1]));
        let plane = &v["plane"];
        [c0, c1 - c0, p(&v["surf"]["build"]["vec"]), p(&plane["o"]), p(&plane["x"]), p(&plane["y"])]
    }

    /// The position of `q` in the frame of the extrusion of segment `c0 + a g` along `w`:
    /// `(a, distance off the extrusion's plane, distance off the target plane)`.
    fn on_strip(f: &[Vec3; 6], q: Vec3) -> (f64, f64, f64) {
        let [c0, g, w, o, x, y] = *f;
        let n = g.cross(w);
        let n = n * (1.0 / n.length());
        let r = q - c0;
        let off = r.dot(n);
        let r = r - n * off;
        // r = a g + b w in the extrusion's plane
        let (gg, gw, ww) = (g.dot(g), g.dot(w), w.dot(w));
        let (rg, rw) = (r.dot(g), r.dot(w));
        let a = (rg * ww - rw * gw) / (gg * ww - gw * gw);
        let pn = x.cross(y);
        let pn = pn * (1.0 / pn.length());
        (a, off.abs(), (q - o).dot(pn).abs())
    }

    /// #18342: `gen1053` intersects the extrusion of a 6.9e-4 long line segment with a
    /// plane. Its one intersection curve is now the whole chord across the extrusion: it
    /// ends on both planes (within 1e-9), one end on the edge swept by the segment's start
    /// (`a` = 0) and the other on the edge swept by its far end (`a` = 1), each within
    /// `SM_EFF_ZERO_SQRT` (1e-6, the bound of the operation's tolerance) in 3D. The first is
    /// 3.9e-7 past the `a` = 0 edge, the second 6.7e-7 inside the `a` = 1 edge, so a curve
    /// cut short inside the strip fails. The old row's two curves ended at points on the
    /// planes but up to 7.6e4 outside the swept segment; they fail the same check.
    #[test]
    fn gen1053_intersection_ends_lie_on_the_extrusion() {
        let case = crate::case::regression_case(&Runner, "gen1053");
        let f = gen1053_frame(&case.inputs);
        let i: Inputs = case.parse().unwrap();
        let out = rust(&i).unwrap();
        let o = out.values.get("out").unwrap().clone();
        let glen = f[1].length();
        // the 3D distance past the swept segment's ends of a point at `a`
        let outside = |a: f64| (-a).max(a - 1.0).max(0.0) * glen;
        // status, status, one curve, ..., its two end points
        assert_eq!((o[0], o[1], o[2], o[9]), (0.0, 0.0, 1.0, 2.0), "{o:?}");
        let mut ends = Vec::new();
        for at in [10usize, 14] {
            let (a, off, off_plane) = on_strip(&f, Vec3::new(o[at], o[at + 1], o[at + 2]));
            assert!(outside(a) <= EFF_ZERO_SQRT, "{at}: {a:e}");
            assert!(off < 1e-9 && off_plane < 1e-9, "{at}: {off:e} {off_plane:e}");
            ends.push(a);
        }
        // the whole chord: one end on each swept edge, within the tolerance in 3D
        ends.sort_by(f64::total_cmp);
        assert!(ends[0].abs() * glen <= EFF_ZERO_SQRT, "{ends:?}");
        assert!((ends[1] - 1.0).abs() * glen <= EFF_ZERO_SQRT, "{ends:?}");
        // the old row's ends that the new one replaced (measured with the rules off)
        for q in [
            Vec3::new(1092.3570614157895, 5731.732333415699, -3560.217776102151),
            Vec3::new(360.0721726580956, -4029.0217729108645, -2500.4128777293017),
        ] {
            let (a, _, _) = on_strip(&f, q);
            assert!(outside(a) > 1.0, "{q:?}: {a:e}");
        }
    }

    /// WR-51: `GlobalSurfaceIntersect` cases where the boundary search dropped its
    /// touching point for a start point that then traced nothing, so upstream found
    /// no curve. The touching point is now the answer, certified on both surfaces and
    /// within the intersector's tolerance. The cases ran at the default tolerance before
    /// tangency-labs/solidcraft#17720, a thousandth of the first surface's size
    /// `|P(min) P(mid)| + |P(mid) P(max)|` over its domain (`first_surface_tolerance`);
    /// they are asked for at that tolerance here, since the default is now sized from
    /// both surfaces and far finer.
    #[test]
    fn wr51_restored_touching_points_are_certified_contacts() {
        use solidcore::intersect::surface_intersector::SurfaceIntersector;
        let names = [
            "global_extrusion_skew",
            "global_bspline",
            "gen0463",
            "gen0581",
            "gen0626",
            "gen1018",
        ];
        for name in names {
            let case = crate::case::regression_case(&Runner, name);
            let Inputs::Op { src, op } = case.parse().unwrap() else {
                panic!("{name}: expected an operation");
            };
            let OpIn::Global {
                target,
                dom,
                other_dom,
                tol,
                ang,
            } = &*op
            else {
                panic!("{name}: expected GlobalSurfaceIntersect");
            };
            let k = Kernel::default();
            let first = rust_src(&k, &src)
                .unwrap()
                .unwrap_or_else(|_| panic!("{name}: the extrusion builds"));
            let d = dom.map_or_else(|| first.base().natural_uv_domain(), dom4);
            let (other, od) = match target {
                TargetIn::Cone { cone } => {
                    let c = rust_cone(&k, cone).unwrap();
                    let od = other_dom.map_or_else(|| c.rev().base().natural_uv_domain(), dom4);
                    (Surface::from(c), od)
                }
                TargetIn::Extrusion { other } => {
                    let e = rust_src(&k, other).unwrap().unwrap_or_else(|_| panic!("{name}"));
                    let od = other_dom.map_or_else(|| e.base().natural_uv_domain(), dom4);
                    (Surface::from(e), od)
                }
                TargetIn::Bspline { other } => {
                    let e = rust_src(&k, other).unwrap().unwrap_or_else(|_| panic!("{name}"));
                    let b = nurb_surface(&e).unwrap();
                    let od = other_dom.map_or_else(|| b.natural_uv_domain(), dom4);
                    (Surface::from(b), od)
                }
                TargetIn::Plane { .. } => panic!("{name}: no plane target here"),
            };
            let this = Surface::from(first);
            let default_tol = SurfaceIntersector::new(&k, &this, &d, &other, &od, false)
                .unwrap()
                .this_approx_tol_3d;
            let coarse = first_surface_tolerance(&k, &this, &d);
            assert!(tol.is_some() || default_tol < coarse, "{name}");
            let tol = tol.unwrap_or(coarse);
            let r = this
                .global_surface_intersect(&k, &d, &other, &od, [true, true], Some(tol), *ang)
                .unwrap();
            crate::units::intersect_surface_intersector_p5::certify_restored_touching_point(
                &k,
                [&this, &other],
                [&d, &od],
                tol,
                &r,
            );
        }
    }

    /// At the first surface's size over its unbounded domain (the default tolerance
    /// before tangency-labs/solidcraft#17720, here asked for explicitly: the default is
    /// now sized from both surfaces' generators and far finer), this case returns a
    /// coarse touching point; its finite status and coordinates must remain observable
    /// despite NOW weights.
    #[test]
    fn review_global_touching_point_is_checked_outside_infinite_sums() {
        use crate::units::surface_surf_of_extrusion_p1::GenIn;
        use solidcore::intersect::surface_intersector::SurfaceIntersector;
        use solidcore::nurbs::math::gamma;
        use solidcore::surface::surface::p2::TsectCurveType;

        fn circle_end(s: &SrcIn, end: bool, height: f64) -> Vec3 {
            let SrcIn::Built { surf } = s else {
                panic!("expected the generated canonical extrusion");
            };
            let GenIn::Circle { c, x, y, d, r } = &surf.gen else {
                panic!("expected its circular generator");
            };
            let BuildIn::Canonical { vec } = &surf.build else {
                panic!("expected the canonical sweep vector");
            };
            let (sin, cos) = d[usize::from(end)].to_radians().sin_cos();
            v3(*c) + v3(*x) * (*r * cos) + v3(*y) * (*r * sin) + v3(*vec) * height
        }

        let case = crate::case::regression_case(&Runner, "gen1140");
        let inputs: Inputs = case.parse().unwrap();
        let Inputs::Op { src, op } = &inputs else {
            panic!("expected GlobalSurfaceIntersect");
        };
        let OpIn::Global {
            target: TargetIn::Extrusion { other },
            dom: None,
            other_dom: None,
            tol: None,
            ang,
        } = &**op
        else {
            panic!("expected the generated default-tolerance extrusion pair");
        };

        let k = Kernel::default();
        let first = rust_src(&k, src)
            .unwrap()
            .unwrap_or_else(|_| panic!("first generated extrusion must build"));
        let second = rust_src(&k, other)
            .unwrap()
            .unwrap_or_else(|_| panic!("second generated extrusion must build"));
        let d1 = first.base().natural_uv_domain();
        let d2 = second.base().natural_uv_domain();
        let s1 = Surface::from(first.clone());
        let s2 = Surface::from(second.clone());
        let default_tol = SurfaceIntersector::new(&k, &s1, &d1, &s2, &d2, false)
            .unwrap()
            .this_approx_tol_3d;
        let expected_tol = first.extrusion_vector().length() * d1.v_interval().length() / 1000.0;
        let coarse = first_surface_tolerance(&k, &s1, &d1);
        assert!((coarse - expected_tol).abs() <= gamma(32) * expected_tol);
        assert!(default_tol < 1e-3 * expected_tol, "{default_tol} {expected_tol}");
        let default_tol = coarse;

        let result = s1
            .global_surface_intersect(&k, &d1, &s2, &d2, [true, true], Some(coarse), *ang)
            .unwrap();
        assert_eq!(result.curves_3d.len(), 1);
        assert_eq!(result.curve_types, [TsectCurveType::Touching]);
        assert_eq!(result.surface1_uv_curves.len(), 1);
        assert_eq!(result.surface2_uv_curves.len(), 1);
        assert_eq!(result.deviations.len(), 1);
        let uv1 = result.surface1_uv_curves[0]
            .as_ref()
            .unwrap()
            .evaluate_point(&k, 0.5)
            .unwrap();
        let uv2 = result.surface2_uv_curves[0]
            .as_ref()
            .unwrap()
            .evaluate_point(&k, 0.5)
            .unwrap();
        assert_eq!(uv1.x, d1.min().x);
        assert_eq!(uv2.x, d2.max().x);
        assert!(d1.contains_point(Point2::new(uv1.x, uv1.y), 0.0));
        assert!(d2.contains_point(Point2::new(uv2.x, uv2.y), 0.0));

        // At these generator endpoints the exact definitions are the circle ends
        // plus the sweep vector times v. No spline fit supplies these reference points.
        let p1 = circle_end(src, false, uv1.y);
        let p2 = circle_end(other, true, uv2.y);
        let midpoint = (p1 + p2) * 0.5;
        let mut scale = 1.0_f64;
        for source in [&first, &second] {
            let net = source.base().gw_nurb().unwrap();
            assert_eq!(net.degrees(), (2, 1));
            for p in net.rows().flatten() {
                let weight = if p.w == NOW { 1.0 } else { p.w };
                assert!(weight > 0.0);
                scale = scale.max((p.x.abs() + p.y.abs() + p.z.abs()) / weight);
            }
        }
        // Two degree-(2,1) tensor de Boor evaluations, their rational divisions,
        // the closed-form endpoint evaluations and midpoint use fewer than 128
        // rounded operations per coordinate. Huge sweep endpoints set the floor.
        let rounding = gamma(128) * scale;
        let deviation = result.deviations[0];
        assert!(deviation.is_finite() && deviation > k.tolerances.approx_tol_3d);
        assert!(deviation < default_tol);
        assert!((deviation - p1.distance(p2)).abs() <= rounding);

        let mut asked = inputs.clone();
        if let Inputs::Op { op, .. } = &mut asked {
            if let OpIn::Global { tol, .. } = &mut **op {
                *tol = Some(coarse);
            }
        }
        let case = Case {
            inputs: serde_json::to_value(&asked).unwrap(),
            ..case
        };
        let out = Runner.run_rust(&case).unwrap();
        assert!(out.error.is_none() && out.deferred.is_none() && out.rejected.is_none());
        assert_eq!(out.values["build"], [0.0]);
        let geometry_is_valid = |output: &Output| {
            let raw = &output.values["out"];
            raw.len() == 32
                && raw.iter().all(|v| v.is_finite())
                && raw[..4] == [0.0, 0.0, 0.0, 1.0]
                && raw[4] == 1.0
                && raw[5] == result.curves_3d[0].type_code() as f64
                && raw[6..8] == [0.0, 1.0]
                && raw[8..11] == [1.0, 1.0, 2.0]
                && raw[14] == NOW
                && raw[18] == NOW
                && raw[19..31]
                    == [4.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
                && Vec3::new(raw[11], raw[12], raw[13]).distance(midpoint) <= rounding
                && raw[11..14] == raw[15..18]
                && (raw[31] - p1.distance(p2)).abs() <= rounding
                && Vec3::new(raw[11], raw[12], raw[13]).distance(p1) <= raw[31] + rounding
                && Vec3::new(raw[11], raw[12], raw[13]).distance(p2) <= raw[31] + rounding
        };
        assert!(geometry_is_valid(&out));

        // Mutation controls: the existing infinite block sums hide both changes,
        // while the independent status/definition checks reject them.
        let encoded = crate::golden::encode(&case.name, &out);
        for (index, wrong) in [(1, 1001.0), (11, midpoint.x + 1.0)] {
            let mut corrupted = out.clone();
            corrupted.values.get_mut("out").unwrap()[index] = wrong;
            assert_eq!(crate::golden::encode(&case.name, &corrupted), encoded);
            assert!(!geometry_is_valid(&corrupted));
        }
    }

    /// The default tolerance before tangency-labs/solidcraft#17720: a thousandth of
    /// `|P(min) P(mid)| + |P(mid) P(max)|` of `s` over `d` (upstream's, from the first
    /// surface alone).
    fn first_surface_tolerance(k: &Kernel, s: &Surface, d: &Extent2d) -> f64 {
        let at = |p| s.evaluate_point(k, p).unwrap();
        let (a, m, b) = (at(d.min()), at(d.evaluate(0.5, 0.5)), at(d.max()));
        (a.distance(m) + m.distance(b)) / 1000.0
    }

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

    /// The extrusion and plane of a generated case whose extrusion lies in the plane:
    /// the extrusion, its domain, the plane and its domain, and the case's tolerance.
    fn coincident_plane_case(
        k: &Kernel,
        name: &str,
    ) -> (SurfOfExtrusion, Extent2d, Plane, Extent2d, Option<f64>) {
        let case = crate::case::regression_case(&Runner, name);
        let Inputs::Op { src, op } = case.parse().unwrap() else {
            panic!("{name}: expected an operation");
        };
        let s = rust_src(k, &src)
            .unwrap()
            .unwrap_or_else(|_| panic!("{name}: the extrusion builds"));
        let natural = s.base().natural_uv_domain();
        match &*op {
            OpIn::Plane {
                plane,
                dom,
                other_dom,
                tol,
            } => {
                let pl = rust_plane(plane);
                let od = other_dom.map_or_else(|| pl.natural_uv_domain(), dom4);
                (s, dom.map_or(natural, dom4), pl, od, *tol)
            }
            OpIn::Global {
                target: TargetIn::Plane { plane, .. },
                dom,
                other_dom,
                tol,
                ..
            } => {
                let pl = rust_plane(plane);
                let od = other_dom.map_or_else(|| pl.natural_uv_domain(), dom4);
                (s, dom.map_or(natural, dom4), pl, od, *tol)
            }
            _ => panic!("{name}: expected a plane intersection"),
        }
    }

    /// The end points of every curve, in a fixed order (for comparing two answers).
    fn curve_ends(k: &Kernel, r: &ExtrusionIntersection) -> Vec<[f64; 6]> {
        let mut ends: Vec<[f64; 6]> = r
            .curves_3d
            .iter()
            .map(|c| {
                let ivl = c.natural_interval();
                let a = c.evaluate(k, ivl.min(), 0, true, true).unwrap()[0];
                let b = c.evaluate(k, ivl.max(), 0, true, true).unwrap()[0];
                let (a, b) = if (a.x, a.y, a.z) <= (b.x, b.y, b.z) { (a, b) } else { (b, a) };
                [a.x, a.y, a.z, b.x, b.y, b.z]
            })
            .collect();
        ends.sort_by(|p, q| p.partial_cmp(q).unwrap());
        ends
    }

    /// gen0355 (a bounded plane overlapping part of the extrusion) and gen0970 (an
    /// unbounded plane): the extrusion lies in the plane, and the special case used to
    /// answer `needs_more` with no curve, leaving the general intersector a τ-dependent
    /// answer. It now answers the coincident patch's boundary: `RegionBoundary` curves
    /// in the plane, inside its domain, forming closed loops, the same for every τ.
    #[test]
    fn extrusion_in_plane_answers_its_coincident_patch() {
        use solidcore::surface::surface::p2::TsectCurveType;
        for name in ["gen0355", "gen0970"] {
            let k = Kernel::default();
            let (s, d, pl, od, tol) = coincident_plane_case(&k, name);
            let r = s.intersect_with_plane(&k, &d, &pl, &od, tol).unwrap();
            assert!(!r.needs_more_intersections, "{name}: answered");
            assert!(!r.curves_3d.is_empty(), "{name}: some boundary");
            assert_eq!(r.curve_types.len(), r.curves_3d.len(), "{name}");
            assert_eq!(r.deviations.len(), r.curves_3d.len(), "{name}");
            assert!(
                r.curve_types.contains(&TsectCurveType::RegionBoundary),
                "{name}: {:?}",
                r.curve_types
            );
            let n = pl.position().z_axis();
            let o = pl.position().origin();
            let mut ends = Vec::new();
            for (c, t) in r.curves_3d.iter().zip(&r.curve_types) {
                let ivl = c.natural_interval();
                for i in 0..=8 {
                    let p = c
                        .evaluate(&k, ivl.min() + ivl.length() * f64::from(i) / 8.0, 0, true, true)
                        .unwrap()[0];
                    let scale = 1.0 + p.max_dimension() + o.max_dimension();
                    assert!(n.dot(p - o).abs() <= 1e-9 * scale, "{name}: in the plane");
                    let uv = pl.project_point_to_uv_domain(p);
                    assert!(
                        od.u_interval().min() - 1e-9 * scale <= uv.x
                            && uv.x <= od.u_interval().max() + 1e-9 * scale
                            && od.v_interval().min() - 1e-9 * scale <= uv.y
                            && uv.y <= od.v_interval().max() + 1e-9 * scale,
                        "{name}: inside the plane's domain"
                    );
                }
                if *t == TsectCurveType::RegionBoundary {
                    for t in [ivl.min(), ivl.max()] {
                        ends.push(c.evaluate(&k, t, 0, true, true).unwrap()[0]);
                    }
                }
            }
            // every boundary end meets another boundary's end: the loops close
            for (i, p) in ends.iter().enumerate() {
                let scale = 1.0 + p.max_dimension();
                let met = ends
                    .iter()
                    .enumerate()
                    .any(|(j, q)| j != i && (*p - *q).length() <= 1e-9 * scale);
                assert!(met, "{name}: open boundary end {p:?}");
            }
            // τ-independent. The special case caps τ at 1e-6, so these are five
            // distinct effective tolerances (larger values would all act as 1e-6).
            let first = curve_ends(&k, &r);
            for t in [1e-6, 1e-7, 1e-8, 1e-9, 1e-10] {
                let rt = s.intersect_with_plane(&k, &d, &pl, &od, Some(t)).unwrap();
                assert_eq!(rt.curve_types, r.curve_types, "{name}: types at τ = {t}");
                let other = curve_ends(&k, &rt);
                assert_eq!(other.len(), first.len(), "{name}: τ = {t}");
                for (a, b) in first.iter().zip(&other) {
                    for (x, y) in a.iter().zip(b) {
                        assert!(
                            (x - y).abs() <= 1e-9 * (1.0 + x.abs().max(y.abs())),
                            "{name}: τ = {t}: {a:?} vs {b:?}"
                        );
                    }
                }
            }
        }
    }

    /// The rows this change moved, answered: the special case declined
    /// (`needs_more`, no curve) where the extrusion lies in the plane, and now answers
    /// the coincident region's boundary (an empty one where the region misses the
    /// plane's domain).
    const COINCIDENT_ROWS: [&str; 12] = [
        "gen0158",
        "gen0163",
        "gen0301",
        "gen0355",
        "gen0404",
        "gen0566",
        "gen0576",
        "gen0654",
        "gen0666",
        "gen0787",
        "gen0970",
        "line_plane_containing",
    ];

    /// Rows whose extrusion lies in the plane but whose region no certificate covers
    /// (a folded generator swept a finite length inside the domain): the special case
    /// still declines, as upstream.
    const DECLINED_ROWS: [&str; 3] = ["gen0260", "gen1043", "gen1048"];

    /// The plane's UV frame of a case: `E` in UV, `mu` across the sweep and `nu`
    /// along it (in units of the extrusion parameter), for UV points.
    struct TestFrame {
        eu: f64,
        ew: f64,
    }

    impl TestFrame {
        fn new(pl: &Plane, e: Vec3) -> TestFrame {
            let pos = *pl.position();
            let sc = pl.uv_scale();
            TestFrame {
                eu: pos.x_axis().dot(e) / sc.x,
                ew: pos.y_axis().dot(e) / sc.y,
            }
        }
        fn mu(&self, p: Point2) -> f64 {
            -self.ew * p.x + self.eu * p.y
        }
        fn nu(&self, p: Point2) -> f64 {
            (self.eu * p.x + self.ew * p.y) / (self.eu * self.eu + self.ew * self.ew)
        }
    }

    /// The Euclidean control points of `c` over `ivl` (positive weights asserted:
    /// the curve lies in their convex hull).
    fn net_3d(k: &Kernel, c: &solidcore::curve::curve::Curve, ivl: Extent1d) -> Vec<Vec3> {
        net_canon(k, c, ivl).control_points
    }

    /// The canonical form of `c` trimmed to `ivl` (positive weights asserted).
    fn net_canon(
        k: &Kernel,
        c: &solidcore::curve::curve::Curve,
        ivl: Extent1d,
    ) -> solidcore::nurbs::b_spline_curve::p2::CurveCanonical {
        use solidcore::curve::curve::CurveKind;
        use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
        let mut piece = c.clone();
        let mut t = ivl;
        piece.trim(k, &mut t, TrimMode::Silent).unwrap();
        let base = match piece.kind() {
            CurveKind::BSpline(b) => b.clone(),
            CurveKind::Line(l) => l.base().clone(),
            CurveKind::Ellipse(e) => e.base().clone(),
            CurveKind::Hyperbola(h) => h.base().clone(),
            CurveKind::Parabola(p) => p.base().clone(),
            _ => panic!("an unexpected curve kind"),
        };
        let canon = base.canonical().unwrap();
        assert!(canon.weights.iter().all(|w| *w > 0.0));
        canon
    }

    /// The UV images of [`net_3d`].
    fn net_uv(k: &Kernel, c: &solidcore::curve::curve::Curve, ivl: Extent1d, pl: &Plane) -> Vec<Point2> {
        net_3d(k, c, ivl)
            .into_iter()
            .map(|p| pl.project_point_to_uv_domain(p))
            .collect()
    }

    /// Brackets the least of `f(C(s))` over `ivl` without sampling for acceptance:
    /// the lower bound is the least over the control points of a subdivision (convex
    /// hull), the upper bound the least evaluated value; the piece with the lowest
    /// hull bound is split until the bracket is within `width`.
    fn bracket_min(
        k: &Kernel,
        gen: &solidcore::curve::curve::Curve,
        ivl: Extent1d,
        pl: &Plane,
        f: &dyn Fn(Point2) -> f64,
        width: f64,
    ) -> (f64, f64) {
        let at = |s: f64| f(pl.project_point_to_uv_domain(gen.evaluate(k, s, 0, true, true).unwrap()[0]));
        let hull_min = |iv: Extent1d| {
            net_uv(k, gen, iv, pl)
                .into_iter()
                .map(f)
                .fold(f64::INFINITY, f64::min)
        };
        let mut pieces = vec![(hull_min(ivl), ivl)];
        let mut upper = at(ivl.min()).min(at(ivl.max()));
        for _ in 0..4000 {
            pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (lo, iv) = pieces[0];
            if upper - lo <= width {
                return (lo, upper);
            }
            pieces.remove(0);
            let m = iv.mid();
            upper = upper.min(at(m));
            for half in [Extent1d::from_min_max(iv.min(), m), Extent1d::from_min_max(m, iv.max())] {
                pieces.push((hull_min(half), half));
            }
        }
        panic!("no bracket within {width}");
    }

    /// The domain box cut to `alpha <= mu <= beta` (test-side Sutherland-Hodgman).
    fn slab_polygon(od: &Extent2d, f: &TestFrame, alpha: f64, beta: f64) -> Vec<Point2> {
        let mut poly = vec![
            Point2::new(od.u_interval().min(), od.v_interval().min()),
            Point2::new(od.u_interval().max(), od.v_interval().min()),
            Point2::new(od.u_interval().max(), od.v_interval().max()),
            Point2::new(od.u_interval().min(), od.v_interval().max()),
        ];
        for (bound, above) in [(alpha, true), (beta, false)] {
            let inside = |p: Point2| if above { f.mu(p) >= bound } else { f.mu(p) <= bound };
            let mut next = Vec::new();
            for i in 0..poly.len() {
                let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
                if inside(p) {
                    next.push(p);
                }
                if inside(p) != inside(q) {
                    let t = (bound - f.mu(p)) / (f.mu(q) - f.mu(p));
                    next.push(Point2::new(p.x + t * (q.x - p.x), p.y + t * (q.y - p.y)));
                }
            }
            poly = next;
        }
        poly
    }

    /// Asserts the answer's curves are exactly `expected` UV segments, each once
    /// (ends within `1e-9` relative, straight), so no curve repeats or overlaps and
    /// none is missing.
    fn assert_uv_segments(k: &Kernel, name: &str, r: &ExtrusionIntersection, pl: &Plane, expected: &[(Point2, Point2)]) {
        let near = |a: Point2, b: Point2| {
            (a - b).length() <= 1e-9 * (1.0 + a.x.abs().max(a.y.abs()).max(b.x.abs()).max(b.y.abs()))
        };
        let got: Vec<(Point2, Point2)> = r
            .curves_3d
            .iter()
            .map(|c| {
                let ivl = c.natural_interval();
                let ev = |t: f64| pl.project_point_to_uv_domain(c.evaluate(k, t, 0, true, true).unwrap()[0]);
                let (a, b) = (ev(ivl.min()), ev(ivl.max()));
                // straight: every control point on the chord (the convex hull), to the
                // rounding of points at their coordinates' scale (claim precision: the
                // answer's segments are degree 1 with two control points, the chord's
                // ends, so `off` is the rounding of a 2x2 cross product of projected
                // points, a few gamma(1) of the coordinates; measured 0)
                let chord = b - a;
                let len = chord.length();
                assert!(len > 0.0, "{name}: a segment of no length");
                for q in net_uv(k, c, ivl, pl) {
                    let scale = 1.0 + a.x.abs().max(a.y.abs()).max(b.x.abs()).max(b.y.abs());
                    let off = (chord.x * (q.y - a.y) - chord.y * (q.x - a.x)).abs() / len;
                    let t = (chord.x * (q.x - a.x) + chord.y * (q.y - a.y)) / (len * len);
                    let along = (-1e-12..=1.0 + 1e-12).contains(&t);
                    assert!(off <= 1e-13 * scale && along, "{name}: a straight segment");
                }
                (a, b)
            })
            .collect();
        assert_eq!(got.len(), expected.len(), "{name}: {got:?} vs {expected:?}");
        let mut used = vec![false; got.len()];
        for &(p, q) in expected {
            let hit = got.iter().enumerate().position(|(i, &(a, b))| {
                !used[i] && ((near(a, p) && near(b, q)) || (near(a, q) && near(b, p)))
            });
            let i = hit.unwrap_or_else(|| panic!("{name}: missing {p:?}-{q:?} in {got:?}"));
            used[i] = true;
        }
    }

    /// Certifies every moved row's answer as the boundary of the coincident region
    /// `R = I ∩ D` (`I` the generator swept over `V`, `D` the plane's domain), with
    /// no sampled acceptance. Every row's generator lies in the plane over its whole
    /// interval (control points within `1e-12`, convex hull), and the row is one of:
    /// - **empty**: the domain is apart from the surface (the UV box of its control
    ///   net, or across the sweep), so `R` is empty and so is the answer;
    /// - **slab**: every `nu` of `D` cut to the slab `alpha <= mu <= beta` lies within
    ///   `[nu_max + vmin, nu_min + vmax]` (`nu` bounded on the control points), so `R`
    ///   is that convex polygon. `alpha`, `beta` (the generator's extremes of `mu`)
    ///   are bracketed by subdivision hulls and evaluated values, and the answer must
    ///   be exactly the polygon's edges, each once;
    /// - **quad**: `mu` is monotone on the control polygon (variation diminishing:
    ///   every slice of `I` is one interval) and both copies' control points lie in
    ///   `D`, so `R = I`, bounded by the two copies and the two end directrices; the
    ///   answer must be exactly those four curves.
    #[test]
    fn moved_coincident_rows_are_certified() {
        use solidcore::surface::surface::p2::TsectCurveType;
        let mut forms = BTreeMap::new();
        for name in COINCIDENT_ROWS {
            let k = Kernel::default();
            let (s, d, pl, od, tol) = coincident_plane_case(&k, name);
            let r = s.intersect_with_plane(&k, &d, &pl, &od, tol).unwrap();
            assert!(!r.needs_more_intersections, "{name}: answered");
            assert!(
                r.curve_types.iter().all(|t| *t == TsectCurveType::RegionBoundary),
                "{name}: {:?}",
                r.curve_types
            );
            let n = pl.position().z_axis();
            let o = pl.position().origin();
            let gen = s.gen_curve().unwrap();
            let e = s.extrusion_vector();
            // the generator's parameter is the surface's v when its UV are swapped
            let (s_ivl, v_ivl) = if s.swap_uv() {
                (d.v_interval(), d.u_interval())
            } else {
                (d.u_interval(), d.v_interval())
            };
            let (vmin, vmax) = (v_ivl.min(), v_ivl.max());
            // the generator lies in the plane over its whole interval (the solver's
            // profile certificate, `plane_patch::certify_in_plane`, admits a control
            // point within SM_EFF_ZERO = 1e-12 of the net's coordinates; the pin
            // restates it at the scale of the point and the origin; measured 2.6e-16)
            for p in net_3d(&k, gen, s_ivl) {
                let bound = 1e-12 * (1.0 + p.max_dimension() + o.max_dimension());
                assert!(n.dot(p - o).abs() <= bound, "{name}: generator off the plane");
            }
            // and so does the sweep (the sweep certificate: |n . E| within
            // SM_EFF_ZERO |E|; measured 4e-16)
            assert!(n.dot(e).abs() <= 1e-12 * e.length(), "{name}: sweep off the plane");
            let f = TestFrame::new(&pl, e);
            let net = net_uv(&k, gen, s_ivl, &pl);

            if r.curves_3d.is_empty() {
                let net3 = nurb_surface(&s).unwrap();
                let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
                let (_, _, cps) = net3.control_points().unwrap();
                for c in &cps {
                    let w = if c.w == NOW { 1.0 } else { c.w };
                    let p = Vec3::new(c.x / w, c.y / w, c.z / w);
                    let uv = pl.project_point_to_uv_domain(p);
                    lo = [lo[0].min(uv.x), lo[1].min(uv.y)];
                    hi = [hi[0].max(uv.x), hi[1].max(uv.y)];
                }
                let apart = hi[0] < od.u_interval().min()
                    || lo[0] > od.u_interval().max()
                    || hi[1] < od.v_interval().min()
                    || lo[1] > od.v_interval().max();
                // or apart across the sweep: mu(S(s, v)) = mu(C(s)), so the
                // generator's control net bounds the whole surface's mu range
                let (glo, ghi) = net
                    .iter()
                    .map(|p| f.mu(*p))
                    .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
                let corners = slab_polygon(&od, &f, f64::MIN, f64::MAX);
                let (dlo, dhi) = corners
                    .iter()
                    .map(|p| f.mu(*p))
                    .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
                let margin = 1e-9 * (1.0 + glo.abs().max(ghi.abs()).max(dlo.abs()).max(dhi.abs()));
                let across = ghi + margin < dlo || glo - margin > dhi;
                assert!(apart || across, "{name}: an empty answer over the domain");
                forms.insert(name, "empty");
                continue;
            }

            // the quad: monotone mu, both copies in the domain
            let mus: Vec<f64> = net.iter().map(|p| f.mu(*p)).collect();
            let rising = mus.windows(2).all(|w| w[1] >= w[0]) && mus[0] < mus[mus.len() - 1];
            let falling = mus.windows(2).all(|w| w[1] <= w[0]) && mus[0] > mus[mus.len() - 1];
            // a flat stretch (along the sweep) is bounded by its copies only while they
            // do not overlap: every run of flat steps spans less nu than the sweep
            let mut flats_short = true;
            let mut run_lo_hi: Option<(f64, f64)> = None;
            for (i, p) in net.iter().enumerate() {
                let nu = f.nu(*p);
                let in_run = i > 0 && mus[i] == mus[i - 1];
                run_lo_hi = match (run_lo_hi, in_run) {
                    (Some((a, b)), true) => Some((a.min(nu), b.max(nu))),
                    _ => Some((nu, nu)),
                };
                let (a, b) = run_lo_hi.unwrap();
                flats_short &= !in_run || b - a < vmax - vmin;
            }
            let in_dom = |p: Point2| {
                let slack = 1e-12 * (1.0 + p.x.abs().max(p.y.abs()));
                od.u_interval().min() - slack <= p.x
                    && p.x <= od.u_interval().max() + slack
                    && od.v_interval().min() - slack <= p.y
                    && p.y <= od.v_interval().max() + slack
            };
            let e_uv = Point2::new(f.eu, f.ew);
            let copies_inside = net
                .iter()
                .all(|p| in_dom(*p + e_uv * vmin) && in_dom(*p + e_uv * vmax));
            if (rising || falling) && flats_short && copies_inside {
                assert_eq!(r.curves_3d.len(), 4, "{name}");
                let ends: Vec<Point2> = [s_ivl.min(), s_ivl.max()]
                    .iter()
                    .map(|&t| pl.project_point_to_uv_domain(gen.evaluate(&k, t, 0, true, true).unwrap()[0]))
                    .collect();
                let mut straight = Vec::new();
                let mut copies = Vec::new();
                for c in &r.curves_3d {
                    let ivl = c.natural_interval();
                    let ev = |t: f64| pl.project_point_to_uv_domain(c.evaluate(&k, t, 0, true, true).unwrap()[0]);
                    let (a, b, m) = (ev(ivl.min()), ev(ivl.max()), ev(ivl.mid()));
                    let scale = 1.0 + a.x.abs().max(a.y.abs());
                    if ((a + b) / 2.0 - m).length() <= 1e-9 * scale && ((b - a).length() - (e_uv * (vmax - vmin)).length()).abs() <= 1e-9 * scale {
                        straight.push(c);
                    } else {
                        copies.push(c);
                    }
                }
                let expected: Vec<(Point2, Point2)> = ends
                    .iter()
                    .map(|&p| (p + e_uv * vmin, p + e_uv * vmax))
                    .collect();
                let directrices = ExtrusionIntersection {
                    curves_3d: straight.into_iter().cloned().collect(),
                    ..Default::default()
                };
                assert_uv_segments(&k, name, &directrices, &pl, &expected);
                // the copies: the generator's net translated by v E
                assert_eq!(copies.len(), 2, "{name}");
                let mut seen = Vec::new();
                for c in copies {
                    let cn = net_uv(&k, c, c.natural_interval(), &pl);
                    assert_eq!(cn.len(), net.len(), "{name}");
                    let v = f.nu(cn[0]) - f.nu(net[0]);
                    let v = if (v - vmin).abs() <= (v - vmax).abs() { vmin } else { vmax };
                    for (q, p) in cn.iter().zip(&net) {
                        let x = *p + e_uv * v;
                        assert!((*q - x).length() <= 1e-9 * (1.0 + x.x.abs().max(x.y.abs())), "{name}: copy at {v}");
                    }
                    seen.push(v);
                }
                seen.sort_by(f64::total_cmp);
                assert_eq!(seen, vec![vmin, vmax], "{name}");
                forms.insert(name, "quad");
                continue;
            }

            // the slab: alpha and beta bracketed; the cover; the polygon's edges
            let span = net
                .iter()
                .map(|p| f.mu(*p).abs())
                .fold(1.0_f64, f64::max);
            let width = 1e-12 * span;
            let (alo, ahi) = bracket_min(&k, gen, s_ivl, &pl, &|p| f.mu(p), width);
            let (bnlo, bnhi) = bracket_min(&k, gen, s_ivl, &pl, &|p| -f.mu(p), width);
            let (alpha, beta) = ((alo + ahi) / 2.0, -(bnlo + bnhi) / 2.0);
            let poly = slab_polygon(&od, &f, alpha, beta);
            assert!(poly.len() >= 3, "{name}: the slab meets the domain");
            let (nlo, nhi) = net
                .iter()
                .map(|p| f.nu(*p))
                .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
            for p in &poly {
                let slack = 1e-12 * (1.0 + f.nu(*p).abs() + nlo.abs().max(nhi.abs()) + vmin.abs().max(vmax.abs()));
                assert!(f.nu(*p) - nhi >= vmin - slack && f.nu(*p) - nlo <= vmax + slack, "{name}: the cover");
            }
            let expected: Vec<(Point2, Point2)> = (0..poly.len())
                .map(|i| (poly[i], poly[(i + 1) % poly.len()]))
                .filter(|(a, b)| (*a - *b).length() > 1e-9 * (1.0 + a.x.abs().max(a.y.abs())))
                .collect();
            assert_uv_segments(&k, name, &r, &pl, &expected);
            // the matching above is relative to the points' scale (about 1e6 on the
            // unbounded planes); pin the fold lines' mu, and the side pieces' fixed
            // coordinate, to the solver's error terms and the rounding of evaluating
            // and projecting points at the coordinates' scale (u = eps / 2,
            // gamma(n) = n u / (1 - n u)):
            // - mu = dmu . (X - O) with dmu = (eu dw - ew du) (the solver's frame);
            // - a control value of a range piece (`plane_patch::net_points` and
            //   `Affine::at`): the piece's net is within the convex hull of the
            //   generator's (homogeneous points and weights are convex combinations),
            //   so its error is at most
            //   E = |dmu|_1 (gamma(6p + 4) (max w |P|_1 + |P|_1 max w) / min w
            //       + (gamma(8) + gamma(1)) (|P|_1 + |O|_1));
            // - the solver's fold value is an evaluation within its bracket width
            //   (`plane_patch::extreme_width`: 1e-12 max |mu| + 2 E) above the true
            //   extreme, the test's alpha within half its bracket (lower hull value,
            //   upper evaluation);
            // - an evaluation of the generator (the solver's and the bracket's, de
            //   Boor with at most 3p operations per coordinate), its projection to UV
            //   (a subtraction, a dot product of three and a division) and its mu (two
            //   products and a sum) is within |dmu|_1 gamma(3p + 16) (|P|_1 + |O|_1);
            // - a fold line's end X is its stored control point (a clamped end), placed
            //   at the solver's mu on a side (the free coordinate: three operations),
            //   mapped to space (three), projected back (four) and its mu taken (two):
            //   within |dmu|_1 gamma(12) (|X|_1 + |O|_1)
            let gamma = |n: f64| {
                let u = f64::EPSILON / 2.0;
                n * u / (1.0 - n * u)
            };
            let canon = net_canon(&k, gen, s_ivl);
            let deg = canon.degree as f64;
            let w_of = |i: usize| canon.weights.get(i).copied().unwrap_or(1.0);
            let (w_min, w_max, hom_max, p_max) = canon.control_points.iter().enumerate().fold(
                (f64::MAX, 0.0_f64, 0.0_f64, 0.0_f64),
                |(lo, hi, h, m), (i, p)| {
                    let n1 = p.x.abs() + p.y.abs() + p.z.abs();
                    (lo.min(w_of(i)), hi.max(w_of(i)), h.max(w_of(i) * n1), m.max(n1))
                },
            );
            let pos = *pl.position();
            let sc = pl.uv_scale();
            let dmu = pos.y_axis() * (f.eu / sc.y) - pos.x_axis() * (f.ew / sc.x);
            let dmu1 = dmu.x.abs() + dmu.y.abs() + dmu.z.abs();
            let o1 = o.x.abs() + o.y.abs() + o.z.abs();
            let value_err = dmu1
                * (gamma(6.0 * deg + 4.0) * (hom_max + p_max * w_max) / w_min
                    + (gamma(8.0) + gamma(1.0)) * (p_max + o1));
            let mu_scale = net.iter().map(|p| f.mu(*p).abs()).fold(0.0_f64, f64::max) + value_err;
            let solver_width = 1e-12 * mu_scale + 2.0 * value_err;
            let half_brackets = [(ahi - alo) / 2.0, (bnhi - bnlo) / 2.0];
            let mut on_bounds = [0, 0];
            for c in &r.curves_3d {
                let ivl = c.natural_interval();
                let ev3 = |t: f64| c.evaluate(&k, t, 0, true, true).unwrap()[0];
                let (a3, b3) = (ev3(ivl.min()), ev3(ivl.max()));
                let (a, b) = (pl.project_point_to_uv_domain(a3), pl.project_point_to_uv_domain(b3));
                let coords = a.x.abs().max(a.y.abs()).max(b.x.abs()).max(b.y.abs());
                let round = 1e-14 * coords;
                let x1 = [a3, b3]
                    .iter()
                    .map(|p| p.x.abs() + p.y.abs() + p.z.abs())
                    .fold(0.0_f64, f64::max);
                let gen_round = dmu1 * gamma(3.0 * deg + 16.0) * (p_max + o1);
                let end_round = dmu1 * gamma(12.0) * (x1 + o1);
                for (i, bound) in [alpha, beta].into_iter().enumerate() {
                    let close = |p: Point2| (f.mu(p) - bound).abs() <= 1e-6 * (1.0 + bound.abs()) + round;
                    if close(a) && close(b) {
                        let tight = half_brackets[i] + solver_width + 2.0 * gen_round + end_round;
                        assert!(
                            (f.mu(a) - bound).abs() <= tight && (f.mu(b) - bound).abs() <= tight,
                            "{name}: a fold line off mu = {bound}"
                        );
                        on_bounds[i] += 1;
                    }
                }
                for (fixed, sides) in [
                    ((a.x, b.x), [od.u_interval().min(), od.u_interval().max()]),
                    ((a.y, b.y), [od.v_interval().min(), od.v_interval().max()]),
                ] {
                    for side in sides {
                        let close = |x: f64| (x - side).abs() <= 1e-6 * (1.0 + side.abs());
                        if close(fixed.0) && close(fixed.1) {
                            // claim precision: the solver snaps a side piece's fixed
                            // coordinate to the side (`plane_patch::side_point`), so it
                            // is off only by mapping the end to space and projecting it
                            // back (gamma(8) of the coordinates); measured 2.8e-14 abs
                            let tight = 1e-12 * (1.0 + side.abs()) + round;
                            assert!(
                                (fixed.0 - side).abs() <= tight && (fixed.1 - side).abs() <= tight,
                                "{name}: a side piece off its side {side}"
                            );
                        }
                    }
                }
            }
            let expected_on = |bound: f64| {
                expected
                    .iter()
                    .filter(|(a, b)| {
                        let close = |p: Point2| (f.mu(p) - bound).abs() <= 1e-6 * (1.0 + bound.abs()) + 1e-14 * p.x.abs().max(p.y.abs());
                        close(*a) && close(*b)
                    })
                    .count()
            };
            assert_eq!(on_bounds, [expected_on(alpha), expected_on(beta)], "{name}");
            assert!(on_bounds.iter().sum::<usize>() >= 1, "{name}: no fold line pinned");
            forms.insert(name, "slab");
        }
        // every moved row is certified, in the form its geometry takes
        let expected = [
            ("gen0158", "empty"),
            ("gen0163", "slab"),
            ("gen0301", "quad"),
            ("gen0355", "slab"),
            ("gen0404", "empty"),
            ("gen0566", "empty"),
            ("gen0576", "slab"),
            ("gen0654", "slab"),
            ("gen0666", "slab"),
            ("gen0787", "slab"),
            ("gen0970", "slab"),
            ("line_plane_containing", "quad"),
        ];
        assert_eq!(forms, expected.into_iter().collect::<BTreeMap<_, _>>());
    }

    /// gen0355, the row #18179 needs: a bounded plane over part of an infinite
    /// zigzag extrusion. Its region is the strip between the generator's extremes
    /// across the sweep (a polyline: attained at control points), not the lines at
    /// its ends inside the strip.
    #[test]
    fn gen0355_answers_the_strip_between_its_extremes() {
        let k = Kernel::default();
        let (s, d, pl, od, tol) = coincident_plane_case(&k, "gen0355");
        let r = s.intersect_with_plane(&k, &d, &pl, &od, tol).unwrap();
        let n = pl.position().z_axis();
        let e = s.extrusion_vector();
        let m = n.cross(e) / e.length();
        let gen = s.gen_curve().unwrap();
        assert!(!s.swap_uv());
        // a polyline: its extremes across the sweep are at its control points
        let net: Vec<f64> = net_3d(&k, gen, d.u_interval()).iter().map(|p| m.dot(*p)).collect();
        let (lo, hi) = net.iter().fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
        // the hull bounds are attained: both are values of the generator at its
        // breakpoints, so they are its exact extremes
        let ivl = d.u_interval();
        let at: Vec<f64> = (0..=4)
            .map(|i| m.dot(gen.evaluate(&k, ivl.min() + ivl.length() * f64::from(i) / 4.0, 0, true, true).unwrap()[0]))
            .collect();
        for x in [lo, hi] {
            assert!(at.iter().any(|y| (x - y).abs() <= 1e-12 * (1.0 + x.abs())), "{x} in {at:?}");
        }
        // (the strip's ends, as measured in review)
        assert!((lo - 1.3668).abs() < 1e-4 && (hi - 2.9180).abs() < 1e-4, "{lo} {hi}");
        let mut long = Vec::new();
        for c in &r.curves_3d {
            let ivl = c.natural_interval();
            let a = c.evaluate(&k, ivl.min(), 0, true, true).unwrap()[0];
            let b = c.evaluate(&k, ivl.max(), 0, true, true).unwrap()[0];
            if (b - a).length() > 10.0 {
                assert!((m.dot(a) - m.dot(b)).abs() <= 1e-9 * (1.0 + a.max_dimension()));
                long.push(m.dot(a));
            }
        }
        long.sort_by(f64::total_cmp);
        assert_eq!(long.len(), 2, "{long:?}");
        assert!((long[0] - lo).abs() <= 1e-9 * (1.0 + lo.abs()), "{long:?} vs {lo}");
        assert!((long[1] - hi).abs() <= 1e-9 * (1.0 + hi.abs()), "{long:?} vs {hi}");
    }

    /// Rows outside both certificates keep upstream's answer (`needs_more`, no curve).
    #[test]
    fn rows_beyond_the_certificates_decline() {
        for name in DECLINED_ROWS {
            let k = Kernel::default();
            let (s, d, pl, od, tol) = coincident_plane_case(&k, name);
            let r = s.intersect_with_plane(&k, &d, &pl, &od, tol).unwrap();
            assert!(r.needs_more_intersections, "{name}");
            assert!(r.curves_3d.is_empty(), "{name}");
        }
    }

    /// The surfaces, domains and answer of a `GlobalSurfaceIntersect` regression case, as
    /// the case runs it (a plane target not taken as a surface through the extrusion's
    /// plane path).
    fn global_answer(
        k: &Kernel,
        name: &str,
    ) -> (
        [Surface; 2],
        [Extent2d; 2],
        ExtrusionIntersection,
    ) {
        let case = crate::case::regression_case(&Runner, name);
        let Inputs::Op { src, op } = case.parse().unwrap() else {
            panic!("{name}: expected an operation");
        };
        let OpIn::Global {
            target,
            dom,
            other_dom,
            tol,
            ang,
        } = &*op
        else {
            panic!("{name}: expected GlobalSurfaceIntersect");
        };
        let first = rust_src(k, &src)
            .unwrap()
            .unwrap_or_else(|_| panic!("{name}: the extrusion builds"));
        let d = dom.map_or_else(|| first.base().natural_uv_domain(), dom4);
        let (other, od) = match target {
            TargetIn::Plane { plane, as_surface } => {
                let pl = rust_plane(plane);
                let od = other_dom.map_or_else(|| pl.natural_uv_domain(), dom4);
                if !*as_surface {
                    let target = IntersectTarget::Plane(&pl);
                    let r = first
                        .global_surface_intersect(k, &d, target, &od, [true, true], *tol, *ang)
                        .unwrap();
                    return ([Surface::from(first), Surface::from(pl)], [d, od], r);
                }
                (Surface::from(pl), od)
            }
            TargetIn::Cone { cone } => {
                let c = rust_cone(k, cone).unwrap();
                let od = other_dom.map_or_else(|| c.rev().base().natural_uv_domain(), dom4);
                (Surface::from(c), od)
            }
            TargetIn::Extrusion { other } => {
                let e = rust_src(k, other).unwrap().unwrap_or_else(|_| panic!("{name}"));
                let od = other_dom.map_or_else(|| e.base().natural_uv_domain(), dom4);
                (Surface::from(e), od)
            }
            TargetIn::Bspline { other } => {
                let e = rust_src(k, other).unwrap().unwrap_or_else(|_| panic!("{name}"));
                let b = nurb_surface(&e).unwrap();
                let od = other_dom.map_or_else(|| b.natural_uv_domain(), dom4);
                (Surface::from(b), od)
            }
        };
        let this = Surface::from(first);
        let x = this
            .global_surface_intersect(k, &d, &other, &od, [true, true], *tol, *ang)
            .unwrap();
        let r = ExtrusionIntersection {
            needs_more_intersections: false,
            curves_3d: x.curves_3d,
            surface1_uv_curves: x.surface1_uv_curves,
            surface2_uv_curves: x.surface2_uv_curves,
            curve_types: x.curve_types,
            deviations: x.deviations,
        };
        ([this, other], [d, od], r)
    }

    /// tangency-labs/solidcraft#17720: the rows whose curve count the default from both
    /// surfaces moved, replayed through the tube certificate: each curve is one
    /// component, except in these known rows (tangency-labs/solidcraft#18488):
    /// - `gen0197`, `gen0401`, `gen0467`, `gen0510`: the intersector splits one component
    ///   at a knot line of a degree-1 profile, and the pieces meet end to end on it;
    /// - `gen0188`, `gen1119`: curves 1 and 2 end inside both domains, with an arc of their
    ///   component in no curve between them (main leaves the same arc uncovered).
    #[test]
    fn moved_count_rows_are_certified_components() {
        let each = |n: usize| -> Vec<Vec<usize>> { (0..n).map(|i| vec![i]).collect() };
        let rows = [
            ("gen0010", each(2), vec![]),
            ("gen0035", each(2), vec![]),
            ("gen0247", each(2), vec![]),
            ("gen0581", each(2), vec![]),
            ("gen0586", each(5), vec![]),
            ("gen0652", each(4), vec![]),
            ("gen0730", each(2), vec![]),
            ("gen0874", each(3), vec![]),
            ("gen1055", each(1), vec![]),
            ("gen1107", each(2), vec![]),
            ("gen1140", each(2), vec![]),
            ("global_cylinder_class", each(2), vec![]),
            ("gen0197", vec![vec![0, 1]], vec![]),
            ("gen0401", vec![vec![0, 2, 1]], vec![]),
            ("gen0467", vec![vec![0, 1]], vec![]),
            ("gen0510", vec![vec![0, 1]], vec![]),
            ("gen0188", each(1), vec![1, 2]),
            ("gen1119", each(1), vec![1, 2]),
        ];
        for (name, chains, open) in rows {
            let k = Kernel::default();
            let (s, d, r) = global_answer(&k, name);
            let chains: Vec<&[usize]> = chains.iter().map(Vec::as_slice).collect();
            crate::units::intersect_surface_intersector_p5::assert_certified_components(
                &k,
                [&s[0], &s[1]],
                [&d[0], &d[1]],
                &r.curves_3d,
                [&r.surface1_uv_curves, &r.surface2_uv_curves],
                &r.curve_types,
                &chains,
                &open,
            );
        }
    }
}
