//! SMLib B-reps out to SolidCraft's schema 4 B-rep (`solidcraft_geom::brep`, docs/native-brep.md),
//! and the primitive solids beyond the box.
//!
//! The C API hands out SMLib's B-rep as flat arrays (`smlib_brep_export`, laid out like USD's
//! BrepArray: analytic surfaces as their kinds, ranges in radians, converted end by end from
//! SMLib's NURB parameters). This module reads them as hostile input and maps them to schema 4:
//! - **Geometry:** each analytic surface to its schema kind (SMLib's cone of semi-angle 0 is its
//!   cylinder), NURBS as splines. Edge curves come as their own kinds, or as NURBS where SMLib
//!   holds the edge as one (its primitives' circles do). As a fallback a NURBS line (order 2,
//!   two points) and a rational quadratic circle or arc are recognised exactly and written as
//!   `line` / `circle` with their interval mapped; other NURBS edges stay splines. The report
//!   counts both.
//! - **Sides:** SMLib keeps a face's loops relative to its own natural normal, which may be
//!   opposite the STEP form's (a surface with SMLib's `insideOut` or `swapUV`). Schema 4 keeps
//!   them relative to the record's normal, outer loop counter-clockwise: a face whose outer loop
//!   comes out clockwise in the record's parameters has its loops reversed and its sides flipped.
//! - **Shells and regions:** SMLib keeps a closed boundary as two runtime shells, one per side,
//!   each face's use in a shell facing into that shell's region; schema 4 keeps one record, its
//!   oriented faces as the side in the region that is not material uses them (out of the
//!   material). A sheet's one shell holds both uses of each face: the `same` one is kept. The
//!   infinite region is never a void (SMLib marks it one).
//! - **Order:** faces, edges and vertices renumbered in the canonical walk; each edge's uses
//!   from that walk (`upper = !forward`), never SMLib's radial entry flags.
//! - **Pcurves:** computed from each edge record on its face's surface record
//!   (`solidcraft_geom::brep::pcurve`), never SMLib's UV trims, moved by whole periods into the
//!   face's domain.
//! - **Tolerances:** SMLib's intersection tolerance, or 10⁻⁶ of the body's size (at least 10⁻⁵ mm)
//!   when larger, for the body, every edge and every vertex.
use std::f64::consts::TAU;

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use solidcraft_geom::brep as b;

use crate::flat::{self, *};
use crate::history::Order;
use crate::import::Arrays;
use crate::{Brep, Result, error, ffi, init};

impl Brep {
    /// A cylinder about +Z: base centre, radius and height (mm, > 0).
    pub fn make_cylinder(origin: [f64; 3], radius: f64, height: f64) -> Result<Brep> {
        init()?;
        ffi::make_cylinder(&origin, radius, height).map(Brep).map_err(error)
    }

    /// A sphere: centre and radius (mm, > 0).
    pub fn make_sphere(center: [f64; 3], radius: f64) -> Result<Brep> {
        init()?;
        ffi::make_sphere(&center, radius).map(Brep).map_err(error)
    }

    /// A cone or frustum about +Z: base centre, base radius (> 0), top radius (≥ 0, not the base
    /// radius) and height (> 0), in mm.
    pub fn make_cone(origin: [f64; 3], base_radius: f64, top_radius: f64, height: f64) -> Result<Brep> {
        init()?;
        ffi::make_cone(&origin, base_radius, top_radius, height).map(Brep).map_err(error)
    }

    /// A ring torus about +Z: centre, major radius > minor radius > 0 (mm).
    pub fn make_torus(center: [f64; 3], major_radius: f64, minor_radius: f64) -> Result<Brep> {
        init()?;
        ffi::make_torus(&center, major_radius, minor_radius).map(Brep).map_err(error)
    }

    /// The flat arrays of this body that the mapping reads ([`Brep::export`]), copied out.
    pub fn arrays(&self) -> Result<Arrays> {
        let f = self.export()?;
        let mut out = Arrays::default();
        for &id in I64_IDS {
            out.ints.insert(id, f.get_i64(id)?);
        }
        for &id in F64_IDS {
            out.reals.insert(id, f.get_f64(id)?);
        }
        Ok(out)
    }

    /// This body as a schema 4 B-rep, validated.
    pub fn to_native(&self) -> std::result::Result<b::Brep, ExportError> {
        self.to_native_report().map(|(brep, _)| brep)
    }

    /// [`Brep::to_native`], with what the mapping kept as SMLib wrote it.
    pub fn to_native_report(&self) -> std::result::Result<(b::Brep, Report), ExportError> {
        let arrays = self.arrays().map_err(ExportError::Smlib)?;
        native(&arrays)
    }

    /// [`Brep::to_native`], with where each of SMLib's faces and edges went in the record's
    /// canonical order: what [`crate::history::Raw::history`] renumbers this body's history with.
    /// SMLib's export lists faces and edges in the body's own order (`SmBrep::GetFaces`,
    /// `GetEdges`), the order its history uses.
    pub fn to_native_ordered(&self) -> std::result::Result<(b::Brep, Order), ExportError> {
        let arrays = self.arrays().map_err(ExportError::Smlib)?;
        native_ordered(&arrays).map(|(brep, _, order)| (brep, order))
    }
}

/// What the mapping could not put in its own kind, or turned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// NURBS edge curves recognised as lines or circular arcs and written as those.
    pub recognised_edges: usize,
    /// NURBS edge curves kept as splines (neither a line nor a circular arc).
    pub nurbs_edges: usize,
    /// Faces whose loops were reversed (SMLib's natural normal against the record's).
    pub reversed_faces: usize,
}

/// Why an SMLib body has no schema 4 record.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportError {
    /// The SMLib call failed.
    Smlib(super::Error),
    /// SMLib holds something schema 4 (or this mapping) has no record for, named.
    Unsupported(String),
    /// The arrays are malformed or inconsistent.
    Invalid(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Smlib(e) => write!(f, "SMLib export: {e}"),
            ExportError::Unsupported(what) => write!(f, "SMLib export: not supported yet: {what}"),
            ExportError::Invalid(what) => write!(f, "SMLib export: {what}"),
        }
    }
}

impl std::error::Error for ExportError {}

type Res<T> = std::result::Result<T, ExportError>;

fn invalid(what: impl std::fmt::Display) -> ExportError {
    ExportError::Invalid(what.to_string())
}
fn unsupported(what: impl std::fmt::Display) -> ExportError {
    ExportError::Unsupported(what.to_string())
}

// ---- The arrays (ids and kinds from `crate::flat`, as in capi/include/smlib.h) ------------

/// The int64 arrays this mapping reads.
const I64_IDS: &[i32] = &[
    REGION_SHELL_COUNT,
    REGION_TYPE,
    SHELL_FACEUSE_COUNT,
    SHELL_WIRE_EDGE_COUNT,
    SHELL_POINT_TYPE,
    FACEUSE_FACE,
    FACEUSE_ORIENTATION,
    FACE_LOOP_COUNT,
    FACE_SURFACE_TYPE,
    LOOP_EDGEUSE_COUNT,
    LOOP_VERTEX,
    EDGEUSE_EDGE,
    EDGEUSE_ORIENTATION,
    EDGE_CURVE_TYPE,
    EDGE_VERTICES,
    EDGE_NURBS_VERTEX_COUNT,
    EDGE_NURBS_ORDER,
    FACE_NURBS_U_VERTEX_COUNT,
    FACE_NURBS_V_VERTEX_COUNT,
    FACE_NURBS_U_ORDER,
    FACE_NURBS_V_ORDER,
];

/// The double arrays this mapping reads.
const F64_IDS: &[i32] = &[
    TOLERANCE,
    EXTENT,
    FACE_RANGE,
    EDGE_RANGE,
    VERTEX_POSITION,
    EDGE_NURBS_CVS,
    EDGE_NURBS_KNOTS,
    EDGE_NURBS_WEIGHTS,
    EDGE_CIRCLE_CENTER,
    EDGE_CIRCLE_AXIS,
    EDGE_CIRCLE_REF_DIRECTION,
    EDGE_CIRCLE_RADIUS,
    EDGE_LINE_ORIGIN,
    EDGE_LINE_DIRECTION,
    EDGE_ELLIPSE_CENTER,
    EDGE_ELLIPSE_AXIS,
    EDGE_ELLIPSE_REF_DIRECTION,
    EDGE_ELLIPSE_X_RADIUS,
    EDGE_ELLIPSE_Y_RADIUS,
    FACE_NURBS_CVS,
    FACE_NURBS_U_KNOTS,
    FACE_NURBS_V_KNOTS,
    FACE_NURBS_WEIGHTS,
    FACE_SPHERE_CENTER,
    FACE_SPHERE_AXIS,
    FACE_SPHERE_REF_DIRECTION,
    FACE_SPHERE_RADIUS,
    FACE_PLANE_ORIGIN,
    FACE_PLANE_AXIS,
    FACE_PLANE_REF_DIRECTION,
    FACE_CYLINDER_ORIGIN,
    FACE_CYLINDER_AXIS,
    FACE_CYLINDER_REF_DIRECTION,
    FACE_CYLINDER_RADIUS,
    FACE_CONE_ORIGIN,
    FACE_CONE_AXIS,
    FACE_CONE_REF_DIRECTION,
    FACE_CONE_RADIUS,
    FACE_CONE_SEMI_ANGLE,
    FACE_TORUS_ORIGIN,
    FACE_TORUS_AXIS,
    FACE_TORUS_REF_DIRECTION,
    FACE_TORUS_MAJOR_RADIUS,
    FACE_TORUS_MINOR_RADIUS,
];

// The kind values as the arrays hold them (int64); a local item shadows the glob import.
const SURFACE_NURBS: i64 = flat::SURFACE_NURBS as i64;
const SURFACE_PLANE: i64 = flat::SURFACE_PLANE as i64;
const SURFACE_CYLINDER: i64 = flat::SURFACE_CYLINDER as i64;
const SURFACE_CONE: i64 = flat::SURFACE_CONE as i64;
const SURFACE_SPHERE: i64 = flat::SURFACE_SPHERE as i64;
const SURFACE_TORUS: i64 = flat::SURFACE_TORUS as i64;
const CURVE_NURBS: i64 = flat::CURVE_NURBS as i64;
const CURVE_LINE: i64 = flat::CURVE_LINE as i64;
const CURVE_CIRCLE: i64 = flat::CURVE_CIRCLE as i64;
const CURVE_ELLIPSE: i64 = flat::CURVE_ELLIPSE as i64;

/// Read access to the arrays: a missing array reads as empty.
#[derive(Clone, Copy)]
struct View<'a>(&'a Arrays);

impl View<'_> {
    fn i(&self, id: i32) -> &[i64] {
        self.0.ints.get(&id).map_or(&[], Vec::as_slice)
    }
    fn f(&self, id: i32) -> &[f64] {
        self.0.reals.get(&id).map_or(&[], Vec::as_slice)
    }
}

fn at<T: Copy>(a: &[T], k: usize, what: &str) -> Res<T> {
    a.get(k).copied().ok_or_else(|| invalid(format!("{what}: index {k} out of range")))
}
fn idx(x: i64, n: usize, what: &str) -> Res<usize> {
    usize::try_from(x).ok().filter(|i| *i < n).ok_or_else(|| invalid(format!("{what}: {x} out of range")))
}
fn count(x: i64, what: &str) -> Res<usize> {
    usize::try_from(x).ok().filter(|n| *n <= b::MAX_ITEMS).ok_or_else(|| invalid(format!("{what}: count {x}")))
}
fn index(i: usize) -> Res<u32> {
    u32::try_from(i).map_err(|_| invalid("index"))
}
fn v3(a: &[f64], k: usize, what: &str) -> Res<[f64; 3]> {
    Ok([at(a, 3 * k, what)?, at(a, 3 * k + 1, what)?, at(a, 3 * k + 2, what)?])
}
/// Consecutive blocks of `counts` covering `0..total`.
fn blocks(counts: &[i64], total: usize, what: &str) -> Res<Vec<std::ops::Range<usize>>> {
    let mut start = 0usize;
    let mut out = Vec::with_capacity(counts.len());
    for &c in counts {
        let end = start.checked_add(count(c, what)?).filter(|e| *e <= total).ok_or_else(|| invalid(format!("{what}: blocks overrun")))?;
        out.push(start..end);
        start = end;
    }
    if start != total {
        return Err(invalid(format!("{what}: blocks do not cover the array")));
    }
    Ok(out)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn arr(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
/// A frame from an origin, z axis and x axis (y = z × x).
fn frame(origin: [f64; 3], axis: [f64; 3], x: [f64; 3]) -> b::Frame {
    b::Frame { origin, x_axis: x, y_axis: cross(axis, x) }
}

// ---- Geometry ---------------------------------------------------------------------------

/// The schema record of each face's surface, in face order.
fn surfaces(flat: View, n_faces: usize) -> Res<Vec<b::Surface>> {
    let kinds = flat.i(FACE_SURFACE_TYPE);
    if kinds.len() != n_faces {
        return Err(invalid("face surface kinds"));
    }
    let mut k = [0usize; 6];
    let (mut cv, mut uk, mut vk, mut w) = (0usize, 0usize, 0usize, 0usize);
    let mut out = Vec::with_capacity(n_faces);
    for &kind in kinds {
        let slot = usize::try_from(kind).ok().filter(|s| *s < 6).ok_or_else(|| unsupported(format!("SMLib surface kind {kind}")))?;
        let i = at(&k, slot, "kind")?;
        if let Some(c) = k.get_mut(slot) {
            *c += 1;
        }
        out.push(match kind {
            SURFACE_PLANE => {
                let f = frame(
                    v3(flat.f(FACE_PLANE_ORIGIN), i, "plane")?,
                    v3(flat.f(FACE_PLANE_AXIS), i, "plane")?,
                    v3(flat.f(FACE_PLANE_REF_DIRECTION), i, "plane")?,
                );
                b::Surface::Plane { origin: f.origin, u_axis: f.x_axis, v_axis: f.y_axis }
            }
            SURFACE_CYLINDER => b::Surface::Cylinder {
                frame: frame(
                    v3(flat.f(FACE_CYLINDER_ORIGIN), i, "cylinder")?,
                    v3(flat.f(FACE_CYLINDER_AXIS), i, "cylinder")?,
                    v3(flat.f(FACE_CYLINDER_REF_DIRECTION), i, "cylinder")?,
                ),
                radius: at(flat.f(FACE_CYLINDER_RADIUS), i, "cylinder")?,
            },
            SURFACE_CONE => {
                let f = frame(
                    v3(flat.f(FACE_CONE_ORIGIN), i, "cone")?,
                    v3(flat.f(FACE_CONE_AXIS), i, "cone")?,
                    v3(flat.f(FACE_CONE_REF_DIRECTION), i, "cone")?,
                );
                let (radius, half) = (at(flat.f(FACE_CONE_RADIUS), i, "cone")?, at(flat.f(FACE_CONE_SEMI_ANGLE), i, "cone")?);
                // SMLib's cone of semi-angle 0 is its cylinder.
                if half == 0.0 {
                    b::Surface::Cylinder { frame: f, radius }
                } else {
                    b::Surface::Cone { frame: f, radius_at_origin: radius, half_angle: half }
                }
            }
            SURFACE_SPHERE => b::Surface::Sphere {
                frame: frame(
                    v3(flat.f(FACE_SPHERE_CENTER), i, "sphere")?,
                    v3(flat.f(FACE_SPHERE_AXIS), i, "sphere")?,
                    v3(flat.f(FACE_SPHERE_REF_DIRECTION), i, "sphere")?,
                ),
                radius: at(flat.f(FACE_SPHERE_RADIUS), i, "sphere")?,
            },
            SURFACE_TORUS => b::Surface::Torus {
                frame: frame(
                    v3(flat.f(FACE_TORUS_ORIGIN), i, "torus")?,
                    v3(flat.f(FACE_TORUS_AXIS), i, "torus")?,
                    v3(flat.f(FACE_TORUS_REF_DIRECTION), i, "torus")?,
                ),
                major_radius: at(flat.f(FACE_TORUS_MAJOR_RADIUS), i, "torus")?,
                minor_radius: at(flat.f(FACE_TORUS_MINOR_RADIUS), i, "torus")?,
            },
            SURFACE_NURBS => {
                let nu = count(at(flat.i(FACE_NURBS_U_VERTEX_COUNT), i, "NURBS face")?, "NURBS face")?;
                let nv = count(at(flat.i(FACE_NURBS_V_VERTEX_COUNT), i, "NURBS face")?, "NURBS face")?;
                let ou = count(at(flat.i(FACE_NURBS_U_ORDER), i, "NURBS face")?, "NURBS face")?;
                let ov = count(at(flat.i(FACE_NURBS_V_ORDER), i, "NURBS face")?, "NURBS face")?;
                let n = nu.checked_mul(nv).filter(|n| *n <= b::MAX_ITEMS).ok_or_else(|| invalid("NURBS face size"))?;
                let slice =
                    |a: &[f64], from: usize, len: usize| a.get(from..from + len).map(<[f64]>::to_vec).ok_or_else(|| invalid("NURBS face arrays"));
                let pts = slice(flat.f(FACE_NURBS_CVS), 3 * cv, 3 * n)?;
                let u_knots = slice(flat.f(FACE_NURBS_U_KNOTS), uk, nu + ou)?;
                let v_knots = slice(flat.f(FACE_NURBS_V_KNOTS), vk, nv + ov)?;
                let weights = if flat.f(FACE_NURBS_WEIGHTS).is_empty() { vec![1.0; n] } else { slice(flat.f(FACE_NURBS_WEIGHTS), w, n)? };
                cv += n;
                uk += nu + ou;
                vk += nv + ov;
                w += n;
                let point = |p: usize| -> [f64; 3] { [pts[3 * p], pts[3 * p + 1], pts[3 * p + 2]] };
                if weights.iter().all(|x| *x == 1.0) {
                    b::Surface::Spline { u_knots, v_knots, points: (0..nu).map(|a| (0..nv).map(|c| point(a * nv + c)).collect()).collect() }
                } else {
                    let h = |p: usize| {
                        let (q, wt) = (point(p), weights[p]);
                        [q[0] * wt, q[1] * wt, q[2] * wt, wt]
                    };
                    b::Surface::RationalSpline { u_knots, v_knots, homogeneous: (0..nu).map(|a| (0..nv).map(|c| h(a * nv + c)).collect()).collect() }
                }
            }
            other => return Err(unsupported(format!("SMLib surface kind {other}"))),
        });
    }
    Ok(out)
}

/// A NURBS curve: full knots, cartesian control points and weights.
struct Nurbs {
    knots: Vec<f64>,
    points: Vec<[f64; 3]>,
    weights: Vec<f64>,
}

impl Nurbs {
    fn record(&self) -> b::Curve {
        if self.weights.iter().all(|w| *w == 1.0) {
            b::Curve::Spline { knots: self.knots.clone(), points: self.points.clone() }
        } else {
            b::Curve::RationalSpline {
                knots: self.knots.clone(),
                homogeneous: self.points.iter().zip(&self.weights).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w]).collect(),
            }
        }
    }
}

/// A NURBS line (order 2, two points, equal weights) as a line, its parameter mapped onto the
/// line's: t ↦ (t − k₁)/(k₂ − k₁).
fn as_line(n: &Nurbs, range: [f64; 2]) -> Option<(b::Curve, [f64; 2])> {
    let ([p0, p1], [k0, k1, k2, k3]) = (<[[f64; 3]; 2]>::try_from(n.points.as_slice()).ok()?, <[f64; 4]>::try_from(n.knots.as_slice()).ok()?);
    if !(k0 == k1 && k2 == k3 && k1 < k2) || n.weights.first() != n.weights.get(1) || p0 == p1 {
        return None;
    }
    let s = |t: f64| (t - k1) / (k2 - k1);
    Some((b::Curve::Line { start: p0, end: p1 }, [s(range[0]), s(range[1])]))
}

/// A rational quadratic circle or circular arc as a circle: its frame's x axis at the curve's
/// start and its normal the curve's turning direction, so the angle grows along the curve; the
/// interval the angles of the range's ends. Checked: 65 samples lie on the circle within 10⁻⁹
/// of its radius (at least 1 mm).
fn as_circle(n: &Nurbs, range: [f64; 2]) -> Option<(b::Curve, [f64; 2])> {
    if n.knots.len() != n.points.len() + 3 || n.points.len() < 3 {
        return None;
    }
    let eval = b::eval::CurveEval::new(&n.record()).ok()?;
    let (a, z) = (*n.knots.get(2)?, *n.knots.get(n.points.len())?);
    let at = |t: f64| eval.point(t);
    let (p, q, r) = (at(a)?, at(a + (z - a) / 3.0)?, at(a + 2.0 * (z - a) / 3.0)?);
    // The circle through three points: centre p + (|w|² (u × w) × u + |u|² w × (u × w)) / (2 |u × w|²).
    let (u, w) = (q - p, r - p);
    let normal = u.cross(w);
    let nn = normal.dot(normal);
    if !(nn > 0.0) {
        return None;
    }
    let center = p + (normal.cross(u) * w.dot(w) + w.cross(normal) * u.dot(u)) * (0.5 / nn);
    let radius = (p - center).len();
    let k = normal.normalized()?;
    let slack = 1e-9 * radius.max(1.0);
    for i in 0..=64 {
        let s = at(a + (z - a) * f64::from(i) / 64.0)?;
        if ((s - center).len() - radius).abs() > slack || (s - center).dot(k).abs() > slack {
            return None;
        }
    }
    let x = (p - center).normalized()?;
    let y = k.cross(x);
    let angle = |t: f64| at(t).map(|s| (s - center).dot(y).atan2((s - center).dot(x)));
    // The angle at the range's start in [0, 2π), then unwrapped along the range.
    let mut a0 = angle(range[0])?;
    if a0 < 0.0 {
        a0 += TAU;
    }
    let (mut prev, mut total) = (a0, a0);
    for i in 1..=64 {
        let mut t = angle(range[0] + (range[1] - range[0]) * f64::from(i) / 64.0)?;
        while t - prev > std::f64::consts::PI {
            t -= TAU;
        }
        while t - prev < -std::f64::consts::PI {
            t += TAU;
        }
        total += t - prev;
        prev = t;
    }
    if !(total > a0) {
        return None;
    }
    Some((b::Curve::Circle { frame: b::Frame { origin: arr(center), x_axis: arr(x), y_axis: arr(y) }, radius }, [a0, total]))
}

/// Each edge's record and interval (in SMLib's direction along it), and how many NURBS edges
/// were recognised as lines or circles and how many stayed splines.
fn curves(flat: View, n_edges: usize) -> Res<(Vec<(b::Curve, [f64; 2])>, usize, usize)> {
    let kinds = flat.i(EDGE_CURVE_TYPE);
    let range = flat.f(EDGE_RANGE);
    if kinds.len() != n_edges || range.len() != 2 * n_edges {
        return Err(invalid("edge kinds or ranges"));
    }
    let (mut nurbs, mut lines, mut circles, mut ellipses) = (0usize, 0usize, 0usize, 0usize);
    let (mut cv, mut kn) = (0usize, 0usize);
    let (mut recognised, mut kept) = (0usize, 0usize);
    let mut out = Vec::with_capacity(n_edges);
    for (e, &kind) in kinds.iter().enumerate() {
        let r = [at(range, 2 * e, "edge range")?, at(range, 2 * e + 1, "edge range")?];
        out.push(match kind {
            CURVE_LINE => {
                let (o, d) = (v3(flat.f(EDGE_LINE_ORIGIN), lines, "line")?, v3(flat.f(EDGE_LINE_DIRECTION), lines, "line")?);
                lines += 1;
                (b::Curve::Line { start: o, end: [o[0] + d[0], o[1] + d[1], o[2] + d[2]] }, r)
            }
            CURVE_CIRCLE => {
                let f = frame(
                    v3(flat.f(EDGE_CIRCLE_CENTER), circles, "circle")?,
                    v3(flat.f(EDGE_CIRCLE_AXIS), circles, "circle")?,
                    v3(flat.f(EDGE_CIRCLE_REF_DIRECTION), circles, "circle")?,
                );
                let radius = at(flat.f(EDGE_CIRCLE_RADIUS), circles, "circle")?;
                circles += 1;
                (b::Curve::Circle { frame: f, radius }, r)
            }
            CURVE_ELLIPSE => {
                let f = frame(
                    v3(flat.f(EDGE_ELLIPSE_CENTER), ellipses, "ellipse")?,
                    v3(flat.f(EDGE_ELLIPSE_AXIS), ellipses, "ellipse")?,
                    v3(flat.f(EDGE_ELLIPSE_REF_DIRECTION), ellipses, "ellipse")?,
                );
                let radii = [at(flat.f(EDGE_ELLIPSE_X_RADIUS), ellipses, "ellipse")?, at(flat.f(EDGE_ELLIPSE_Y_RADIUS), ellipses, "ellipse")?];
                ellipses += 1;
                (b::Curve::Ellipse { frame: f, radii }, r)
            }
            CURVE_NURBS => {
                let n = count(at(flat.i(EDGE_NURBS_VERTEX_COUNT), nurbs, "NURBS edge")?, "NURBS edge")?;
                let o = count(at(flat.i(EDGE_NURBS_ORDER), nurbs, "NURBS edge")?, "NURBS edge")?;
                let cvs = flat.f(EDGE_NURBS_CVS).get(3 * cv..3 * (cv + n)).ok_or_else(|| invalid("NURBS edge points"))?;
                let knots = flat.f(EDGE_NURBS_KNOTS).get(kn..kn + n + o).ok_or_else(|| invalid("NURBS edge knots"))?.to_vec();
                let weights = match flat.f(EDGE_NURBS_WEIGHTS) {
                    [] => vec![1.0; n],
                    w => w.get(cv..cv + n).ok_or_else(|| invalid("NURBS edge weights"))?.to_vec(),
                };
                nurbs += 1;
                cv += n;
                kn += n + o;
                let c = Nurbs { knots, points: cvs.as_chunks::<3>().0.to_vec(), weights };
                // A fallback: SMLib exports lines, circles and ellipses as their own kinds.
                match as_line(&c, r).or_else(|| as_circle(&c, r)) {
                    Some(x) => {
                        recognised += 1;
                        x
                    }
                    None => {
                        kept += 1;
                        (c.record(), r)
                    }
                }
            }
            other => return Err(unsupported(format!("SMLib curve kind {other}"))),
        });
    }
    Ok((out, recognised, kept))
}

/// A domain side moved onto the natural range where it overshoots it by rounding (a sphere's
/// latitudes, a cone's apex).
fn onto(r: [f64; 2], natural: b::Natural) -> [f64; 2] {
    match natural {
        b::Natural::Range(lo, hi) => {
            let slack = 1e-12 * (1.0 + r[0].abs().max(r[1].abs()));
            let snap = |x: f64| {
                if x < lo && x >= lo - slack {
                    lo
                } else if x > hi && x <= hi + slack {
                    hi
                } else {
                    x
                }
            };
            [snap(r[0]), snap(r[1])]
        }
        b::Natural::Periodic => r,
    }
}

// ---- The mapping ----------------------------------------------------------------------------

/// One face before renumbering: loops relative to the record's normal, as (edge, forward).
struct Face {
    surface: b::Surface,
    domain: [[f64; 2]; 2],
    /// SMLib's natural normal is the record's.
    agrees: bool,
    loops: Vec<Loop>,
}

#[derive(Clone)]
enum Loop {
    Edges(Vec<(u32, bool)>),
    Vertex(u32),
}

/// One edge before renumbering, along its record's parameter.
struct Edge {
    curve: b::Curve,
    interval: [f64; 2],
    vertices: [u32; 2],
    /// The record's parameter runs against SMLib's edge direction.
    reversed: bool,
}

/// The schema 4 B-rep of SMLib's flat arrays: mapped, renumbered canonically and validated.
pub fn native(arrays: &Arrays) -> Res<(b::Brep, Report)> {
    native_ordered(arrays).map(|(brep, report, _)| (brep, report))
}

/// [`native`], with the canonical place of each array face and edge.
fn native_ordered(arrays: &Arrays) -> Res<(b::Brep, Report, Order)> {
    let flat = View(arrays);
    let mut report = Report::default();
    if arrays.ints.values().map(Vec::len).chain(arrays.reals.values().map(Vec::len)).any(|n| n > flat::MAX_LEN) {
        return Err(invalid("array too long"));
    }

    // Tolerances: SMLib's, or the size-relative floor where larger.
    let size = match flat.f(EXTENT) {
        [a, b, c, d, e, f] => ((d - a).powi(2) + (e - b).powi(2) + (f - c).powi(2)).sqrt(),
        _ => return Err(invalid("extent")),
    };
    if !size.is_finite() {
        return Err(invalid("extent"));
    }
    let floor = (1e-6 * size).max(1e-5);
    let tolerance = match flat.f(TOLERANCE) {
        [t] if t.is_finite() && *t > floor => *t,
        [_] => floor,
        _ => return Err(invalid("tolerance")),
    };

    // Topology blocks.
    let n_faceuses = flat.i(FACEUSE_FACE).len();
    let n_faces = flat.i(FACE_LOOP_COUNT).len();
    let n_loops = flat.i(LOOP_EDGEUSE_COUNT).len();
    let n_edgeuses = flat.i(EDGEUSE_EDGE).len();
    let n_edges = flat.i(EDGE_CURVE_TYPE).len();
    let n_vertices = flat.f(VERTEX_POSITION).len() / 3;
    if flat.f(VERTEX_POSITION).len() != 3 * n_vertices
        || flat.i(FACEUSE_ORIENTATION).len() != n_faceuses
        || flat.i(EDGEUSE_ORIENTATION).len() != n_edgeuses
        || flat.i(LOOP_VERTEX).len() != n_loops
        || flat.i(EDGE_VERTICES).len() != 2 * n_edges
        || [n_faceuses, n_faces, n_loops, n_edgeuses, n_edges, n_vertices].iter().any(|n| *n > b::MAX_ITEMS)
    {
        return Err(invalid("array lengths"));
    }
    let n_shells = flat.i(SHELL_FACEUSE_COUNT).len();
    if n_shells > 2 * b::MAX_SHELLS || flat.i(SHELL_WIRE_EDGE_COUNT).len() != n_shells || flat.i(SHELL_POINT_TYPE).len() != n_shells {
        return Err(invalid("shell arrays"));
    }
    if flat.i(SHELL_WIRE_EDGE_COUNT).iter().any(|c| *c != 0) || flat.i(SHELL_POINT_TYPE).iter().any(|c| *c != 0) {
        return Err(unsupported("wire and point shells from SMLib"));
    }
    let region_shells = blocks(flat.i(REGION_SHELL_COUNT), n_shells, "region shells")?;
    if region_shells.is_empty() || flat.i(REGION_TYPE).len() != region_shells.len() {
        return Err(invalid("regions"));
    }
    let shell_faceuses = blocks(flat.i(SHELL_FACEUSE_COUNT), n_faceuses, "shell faceuses")?;
    let face_loops = blocks(flat.i(FACE_LOOP_COUNT), n_loops, "face loops")?;
    let loop_edgeuses = blocks(flat.i(LOOP_EDGEUSE_COUNT), n_edgeuses, "loop edgeuses")?;

    // Edges along their records' parameters.
    let (records, recognised, kept) = curves(flat, n_edges)?;
    report.recognised_edges = recognised;
    report.nurbs_edges = kept;
    let mut edges = Vec::with_capacity(n_edges);
    for (e, (curve, [a, z])) in records.into_iter().enumerate() {
        if !(a.is_finite() && z.is_finite()) || a == z {
            return Err(invalid("edge interval"));
        }
        let start = index(idx(at(flat.i(EDGE_VERTICES), 2 * e, "edge vertices")?, n_vertices, "edge vertex")?)?;
        let end = index(idx(at(flat.i(EDGE_VERTICES), 2 * e + 1, "edge vertices")?, n_vertices, "edge vertex")?)?;
        let reversed = a > z;
        edges.push(Edge {
            curve,
            interval: if reversed { [z, a] } else { [a, z] },
            vertices: if reversed { [end, start] } else { [start, end] },
            reversed,
        });
    }

    // Faces: loops as SMLib keeps them, then turned where SMLib's normal is not the record's.
    let surfaces = surfaces(flat, n_faces)?;
    let range = flat.f(FACE_RANGE);
    if range.len() != 4 * n_faces {
        return Err(invalid("face ranges"));
    }
    let mut faces = Vec::with_capacity(n_faces);
    for (f, (surface, loops)) in surfaces.into_iter().zip(&face_loops).enumerate() {
        let r = range.get(4 * f..4 * f + 4).ok_or_else(|| invalid("face range"))?;
        let natural = surface.natural();
        let domain = [onto([r[0], r[2]], natural[0]), onto([r[1], r[3]], natural[1])];
        let mut out = Vec::with_capacity(loops.len());
        for l in loops.clone() {
            let uses = loop_edgeuses.get(l).ok_or_else(|| invalid("loop"))?;
            if uses.is_empty() {
                let v = idx(at(flat.i(LOOP_VERTEX), l, "loop vertex")?, n_vertices, "loop vertex")?;
                out.push(Loop::Vertex(index(v)?));
                continue;
            }
            let mut l = Vec::with_capacity(uses.len());
            for u in uses.clone() {
                let e = idx(at(flat.i(EDGEUSE_EDGE), u, "edgeuse")?, n_edges, "edgeuse edge")?;
                let same = at(flat.i(EDGEUSE_ORIENTATION), u, "edgeuse")? == 1;
                let edge = edges.get(e).ok_or_else(|| invalid("edge"))?;
                l.push((index(e)?, same != edge.reversed));
            }
            out.push(Loop::Edges(l));
        }
        faces.push(Face { surface, domain, agrees: true, loops: out });
    }
    for face in &mut faces {
        if outer_area(face, &edges, tolerance)? < 0.0 {
            face.agrees = false;
            for l in &mut face.loops {
                if let Loop::Edges(uses) = l {
                    uses.reverse();
                    uses.iter_mut().for_each(|u| u.1 = !u.1);
                }
            }
            report.reversed_faces += 1;
        }
    }

    // Shells: SMLib's two runtime sides of a closed boundary become one record, its faces as the
    // side in the region that is not material uses them.
    let material = |r: usize| r > 0 && flat.i(REGION_TYPE).get(r) == Some(&0);
    let mut region_of = vec![0usize; n_shells];
    for (r, shells) in region_shells.iter().enumerate() {
        for s in shells.clone() {
            if let Some(slot) = region_of.get_mut(s) {
                *slot = r;
            }
        }
    }
    let faceuse = |fu: usize| -> Res<(usize, bool)> {
        Ok((idx(at(flat.i(FACEUSE_FACE), fu, "faceuse")?, n_faces, "faceuse face")?, at(flat.i(FACEUSE_ORIENTATION), fu, "faceuse")? == 1))
    };
    let mut face_uses: Vec<Vec<(usize, bool)>> = vec![Vec::new(); n_faces];
    for (s, uses) in shell_faceuses.iter().enumerate() {
        for fu in uses.clone() {
            let (f, same) = faceuse(fu)?;
            face_uses.get_mut(f).ok_or_else(|| invalid("face"))?.push((s, same));
        }
    }
    let mut shell_of: Vec<Option<u32>> = vec![None; n_shells];
    let mut shells: Vec<Vec<(u32, bool)>> = Vec::new();
    for s in 0..n_shells {
        if shell_of.get(s).copied().flatten().is_some() {
            continue;
        }
        let uses = shell_faceuses.get(s).ok_or_else(|| invalid("shell"))?;
        let (f0, _) = faceuse(uses.start).map_err(|_| invalid("empty shell"))?;
        let mate = match face_uses.get(f0).map(Vec::as_slice) {
            Some([(a, _), (b, _)]) => {
                if *a == s {
                    *b
                } else {
                    *a
                }
            }
            _ => return Err(invalid("a face without its two uses")),
        };
        let side = if mate == s {
            s
        } else {
            let (rs, rm) = (region_of.get(s).copied().unwrap_or(0), region_of.get(mate).copied().unwrap_or(0));
            match (material(rs), material(rm)) {
                (false, true) => s,
                (true, false) => mate,
                _ => return Err(unsupported("a shell between two material regions or two empty ones")),
            }
        };
        let mut oriented = Vec::new();
        for fu in shell_faceuses.get(side).ok_or_else(|| invalid("shell"))?.clone() {
            let (f, same) = faceuse(fu)?;
            // A sheet lists both uses of each face in its one shell: keep the `same` one.
            if mate == s && !same {
                continue;
            }
            if !face_uses.get(f).is_some_and(|u| u.iter().any(|(o, _)| *o == mate)) {
                return Err(unsupported("a shell whose faces' other sides are in different shells"));
            }
            let agrees = faces.get(f).ok_or_else(|| invalid("face"))?.agrees;
            oriented.push((index(f)?, same == agrees));
        }
        let id = index(shells.len())?;
        shells.push(oriented);
        for k in [s, mate] {
            if let Some(slot) = shell_of.get_mut(k) {
                *slot = Some(id);
            }
        }
    }
    let mut regions = Vec::with_capacity(region_shells.len());
    for (r, block) in region_shells.iter().enumerate() {
        let mut listed = Vec::new();
        for s in block.clone() {
            let id = shell_of.get(s).copied().flatten().ok_or_else(|| invalid("shell"))?;
            if !listed.contains(&id) {
                listed.push(id);
            }
        }
        regions.push(b::Region { shells: listed, void: r > 0 && !material(r) });
    }
    // Shells renumbered in first appearance walking the regions.
    let mut order: Vec<u32> = Vec::new();
    for r in &regions {
        for s in &r.shells {
            if !order.contains(s) {
                order.push(*s);
            }
        }
    }
    let renumber: HashMap<u32, u32> = order.iter().enumerate().map(|(new, old)| Ok((*old, index(new)?))).collect::<Res<_>>()?;
    for r in &mut regions {
        r.shells.iter_mut().for_each(|s| *s = renumber.get(s).copied().unwrap_or(*s));
    }
    let shells: Vec<Vec<(u32, bool)>> = order.iter().filter_map(|s| shells.get(*s as usize).cloned()).collect();

    canonical(Parts { faces, edges, shells, regions }, n_vertices, flat, tolerance).map(|(brep, order)| (brep, report, order))
}

/// The mapped records before renumbering.
struct Parts {
    faces: Vec<Face>,
    edges: Vec<Edge>,
    shells: Vec<Vec<(u32, bool)>>,
    regions: Vec<b::Region>,
}

/// The record in canonical order: faces in first appearance through the shells, edges through
/// the faces (a face whose first oriented face is reversed walked backwards), vertices through
/// the edges and then the vertex loops, with each face's pcurves; validated. With it, the
/// canonical index of each array face and edge.
fn canonical(parts: Parts, n_vertices: usize, flat: View, tolerance: f64) -> Res<(b::Brep, Order)> {
    let Parts { faces, edges, shells, regions } = parts;
    let mut face_new: Vec<Option<u32>> = vec![None; faces.len()];
    let (mut face_order, mut direction) = (Vec::new(), Vec::new());
    for shell in &shells {
        for &(f, forward) in shell {
            let slot = face_new.get_mut(f as usize).ok_or_else(|| invalid("face"))?;
            if slot.is_none() {
                *slot = Some(index(face_order.len())?);
                face_order.push(f);
                direction.push(forward);
            }
        }
    }
    if face_order.len() != faces.len() {
        return Err(invalid("a face no shell uses"));
    }
    let mut edge_new: Vec<Option<u32>> = vec![None; edges.len()];
    let mut edge_order = Vec::new();
    let mut edge_uses: Vec<Vec<b::EdgeUseRef>> = Vec::new();
    for (fi, (&f, &forward)) in face_order.iter().zip(&direction).enumerate() {
        let face = faces.get(f as usize).ok_or_else(|| invalid("face"))?;
        for (li, l) in face.loops.iter().enumerate() {
            let Loop::Edges(uses) = l else { continue };
            let n = uses.len();
            for k in 0..n {
                let p = if forward { k } else { n - 1 - k };
                let &(e, fwd) = uses.get(p).ok_or_else(|| invalid("loop"))?;
                let slot = edge_new.get_mut(e as usize).ok_or_else(|| invalid("edge"))?;
                let id = match slot {
                    Some(id) => *id,
                    None => {
                        let id = index(edge_order.len())?;
                        *slot = Some(id);
                        edge_order.push(e);
                        edge_uses.push(Vec::new());
                        id
                    }
                };
                let at = b::EdgeUseRef { face: index(fi)?, r#loop: index(li)?, position: index(p)?, upper: !fwd };
                edge_uses.get_mut(id as usize).ok_or_else(|| invalid("edge"))?.push(at);
            }
        }
    }
    if edge_order.len() != edges.len() {
        return Err(unsupported("wire edges from SMLib"));
    }
    if edge_uses.iter().any(|u| u.len() > 2) {
        return Err(unsupported("the radial order of an edge with more than two uses"));
    }
    let mut vertex_new: Vec<Option<u32>> = vec![None; n_vertices];
    let mut vertex_order = Vec::new();
    let mut visit = |v: u32| -> Res<u32> {
        let slot = vertex_new.get_mut(v as usize).ok_or_else(|| invalid("vertex"))?;
        Ok(match slot {
            Some(id) => *id,
            None => {
                let id = index(vertex_order.len())?;
                *slot = Some(id);
                vertex_order.push(v);
                id
            }
        })
    };
    let mut edge_vertices = Vec::with_capacity(edge_order.len());
    for &e in &edge_order {
        let [a, z] = edges.get(e as usize).ok_or_else(|| invalid("edge"))?.vertices;
        edge_vertices.push([visit(a)?, visit(z)?]);
    }
    let mut loop_vertices: HashMap<(usize, usize), u32> = HashMap::new();
    for (fi, &f) in face_order.iter().enumerate() {
        for (li, l) in faces.get(f as usize).ok_or_else(|| invalid("face"))?.loops.iter().enumerate() {
            if let Loop::Vertex(v) = l {
                loop_vertices.insert((fi, li), visit(*v)?);
            }
        }
    }
    if vertex_order.len() != n_vertices {
        return Err(unsupported("vertices no edge or vertex loop uses"));
    }
    let edge_id = |e: u32| edge_new.get(e as usize).copied().flatten().ok_or_else(|| invalid("edge"));

    let mut edge_records = Vec::with_capacity(edge_order.len());
    for ((&e, vertices), uses) in edge_order.iter().zip(edge_vertices).zip(edge_uses) {
        let edge = edges.get(e as usize).ok_or_else(|| invalid("edge"))?;
        edge_records.push(b::Edge { curve: edge.curve.clone(), interval: edge.interval, vertices, tolerance, uses });
    }
    let mut face_records = Vec::with_capacity(face_order.len());
    for (fi, &f) in face_order.iter().enumerate() {
        let face = faces.get(f as usize).ok_or_else(|| invalid("face"))?;
        let renumbered: Vec<Loop> = face
            .loops
            .iter()
            .map(|l| match l {
                Loop::Edges(u) => u.iter().map(|&(e, fw)| Ok((edge_id(e)?, fw))).collect::<Res<Vec<_>>>().map(Loop::Edges),
                Loop::Vertex(v) => Ok(Loop::Vertex(*v)),
            })
            .collect::<Res<_>>()?;
        let mut curves = pcurves(&face.surface, face.domain, &renumbered, &edge_records, tolerance)?.into_iter();
        let mut loops = Vec::with_capacity(renumbered.len());
        for (li, l) in renumbered.iter().enumerate() {
            loops.push(match l {
                Loop::Vertex(_) => b::Loop::Vertex { vertex: *loop_vertices.get(&(fi, li)).ok_or_else(|| invalid("vertex loop"))? },
                Loop::Edges(uses) => {
                    let pcs = curves.next().ok_or_else(|| invalid("pcurves"))?;
                    if pcs.len() != uses.len() {
                        return Err(invalid("pcurves"));
                    }
                    b::Loop::Edges {
                        edges: uses.iter().zip(pcs).map(|(&(edge, forward), pcurve)| b::OrientedEdge { edge, forward, pcurve }).collect(),
                    }
                }
            });
        }
        face_records.push(b::Face { surface: face.surface.clone(), domain: face.domain, loops });
    }
    let positions = flat.f(VERTEX_POSITION);
    let vertices = vertex_order.iter().map(|&v| Ok(b::Vertex { point: v3(positions, v as usize, "vertex")?, tolerance })).collect::<Res<Vec<_>>>()?;
    let shells = shells
        .into_iter()
        .map(|s| {
            let faces = s
                .into_iter()
                .map(|(f, forward)| {
                    Ok(b::OrientedFace { face: face_new.get(f as usize).copied().flatten().ok_or_else(|| invalid("face"))?, forward })
                })
                .collect::<Res<Vec<_>>>()?;
            Ok(b::Shell::Faces { faces, wires: Vec::new() })
        })
        .collect::<Res<Vec<_>>>()?;
    let brep = b::Brep { version: b::VERSION, tolerance, regions, shells, faces: face_records, edges: edge_records, vertices };
    brep.validate().map_err(|e| invalid(format!("the mapped B-rep does not validate: {e}")))?;
    let placed = |new: &[Option<u32>]| {
        new.iter().map(|i| i.map(|i| i as usize).ok_or_else(|| invalid("an entity outside the canonical walk"))).collect::<Res<Vec<_>>>()
    };
    let order = Order { faces: placed(&face_new)?, edges: placed(&edge_new)? };
    Ok((brep, order))
}

/// A face's pcurves, edge loop by edge loop, from the plain-record routine on the face's
/// records, each loop moved by whole periods into the face's domain.
fn pcurves(surface: &b::Surface, domain: [[f64; 2]; 2], loops: &[Loop], edges: &[b::Edge], tolerance: f64) -> Res<Vec<Vec<b::Curve2>>> {
    let mut uses = Vec::new();
    for l in loops {
        let Loop::Edges(l) = l else { continue };
        let mut loop_uses = Vec::with_capacity(l.len());
        for &(e, forward) in l {
            let edge = edges.get(e as usize).ok_or_else(|| invalid("edge"))?;
            loop_uses.push(b::pcurve::EdgeUse { edge: e, curve: &edge.curve, interval: edge.interval, forward });
        }
        uses.push(loop_uses);
    }
    let mut curves = b::pcurve::pcurves(surface, &uses, tolerance).map_err(|e| invalid(format!("pcurve: {e}")))?;
    into_domain(surface, domain, &mut curves);
    Ok(curves)
}

/// The signed area of a face's outer loop in the record's parameters, from its pcurves
/// (positive: counter-clockwise, the record's convention); 0 where it has no edge loop.
fn outer_area(face: &Face, edges: &[Edge], tolerance: f64) -> Res<f64> {
    let Some(Loop::Edges(outer)) = face.loops.first() else { return Ok(0.0) };
    let records: Vec<b::Edge> =
        edges.iter().map(|e| b::Edge { curve: e.curve.clone(), interval: e.interval, vertices: e.vertices, tolerance, uses: Vec::new() }).collect();
    let curves = pcurves(&face.surface, face.domain, &[Loop::Edges(outer.clone())], &records, tolerance)?;
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for (pc, &(e, forward)) in curves.first().map(Vec::as_slice).unwrap_or(&[]).iter().zip(outer) {
        let edge = edges.get(e as usize).ok_or_else(|| invalid("edge"))?;
        let [a, z] = edge.interval;
        let mut run: Vec<[f64; 2]> = (0..=32).filter_map(|i| point2(pc, a + (z - a) * f64::from(i) / 32.0)).collect();
        if !forward {
            run.reverse();
        }
        pts.extend(run);
    }
    let n = pts.len();
    Ok(0.5 * (0..n).filter_map(|k| Some((pts.get(k)?, pts.get((k + 1) % n)?))).map(|(p, q)| p[0] * q[1] - q[0] * p[1]).sum::<f64>())
}

/// A pcurve's point at `t` (de Boor on the homogeneous coefficients).
fn point2(c: &b::Curve2, t: f64) -> Option<[f64; 2]> {
    let (knots, pts): (&[f64], Vec<[f64; 3]>) = match c {
        b::Curve2::Spline { knots, points } => (knots, points.iter().map(|p| [p[0], p[1], 1.0]).collect()),
        b::Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
    };
    let n = pts.len();
    let p = knots.len().checked_sub(n + 1)?;
    let t = t.clamp(*knots.get(p)?, *knots.get(n)?);
    let mut s = p;
    while s + 1 < n && *knots.get(s + 1)? <= t {
        s += 1;
    }
    let mut d: Vec<[f64; 3]> = (0..=p).map(|j| pts.get(s + j - p).copied()).collect::<Option<_>>()?;
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = s + j - p;
            let den = knots.get(i + p + 1 - r)? - knots.get(i)?;
            let a = if den == 0.0 { 0.0 } else { (t - knots.get(i)?) / den };
            let (lo, hi) = (*d.get(j - 1)?, *d.get(j)?);
            *d.get_mut(j)? = [0, 1, 2].map(|c| (1.0 - a) * lo[c] + a * hi[c]);
        }
    }
    let q = d.get(p)?;
    (q[2] != 0.0).then(|| [q[0] / q[2], q[1] / q[2]])
}

/// Moves each loop's pcurves by whole periods so the loop's middle is nearest the domain's.
fn into_domain(surface: &b::Surface, domain: [[f64; 2]; 2], curves: &mut [Vec<b::Curve2>]) {
    for (d, natural) in surface.natural().into_iter().enumerate() {
        if natural != b::Natural::Periodic {
            continue;
        }
        let [lo, hi] = domain.get(d).copied().unwrap_or([0.0, TAU]);
        for l in curves.iter_mut() {
            let (mut a, mut z) = (f64::INFINITY, f64::NEG_INFINITY);
            for x in l.iter().flat_map(|c| coordinates(c, d)) {
                a = a.min(x);
                z = z.max(x);
            }
            if !(a.is_finite() && z.is_finite()) {
                continue;
            }
            let k = ((0.5 * (lo + hi) - 0.5 * (a + z)) / TAU).round();
            if k != 0.0 && k.is_finite() {
                l.iter_mut().for_each(|c| shift(c, d, k * TAU));
            }
        }
    }
}

fn coordinates(c: &b::Curve2, d: usize) -> Vec<f64> {
    match c {
        b::Curve2::Spline { points, .. } => points.iter().filter_map(|p| p.get(d).copied()).collect(),
        b::Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().filter_map(|p| Some(p.get(d)? / p[2])).collect(),
    }
}

fn shift(c: &mut b::Curve2, d: usize, by: f64) {
    match c {
        b::Curve2::Spline { points, .. } => points.iter_mut().for_each(|p| {
            if let Some(x) = p.get_mut(d) {
                *x += by;
            }
        }),
        b::Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter_mut().for_each(|p| {
            let w = p[2];
            if let Some(x) = p.get_mut(d) {
                *x += by * w;
            }
        }),
    }
}

#[cfg(test)]
mod tests;
