//! SMLib B-reps out to SolidCraft's schema 4 B-rep (`solidcraft_geom::brep`, docs/native-brep.md),
//! and the primitive solids beyond the box.
//!
//! The C API hands out SMLib's B-rep as flat arrays (`smlib_brep_export`, laid out like USD's
//! BrepArray: analytic surfaces as their kinds, ranges in radians, converted end by end from
//! SMLib's NURB parameters). This module reads them as hostile input and maps them to schema 4:
//! - **Geometry:** each analytic surface to its schema kind (SMLib's cone of semi-angle 0 is its
//!   cylinder), NURBS as splines. Edge curves come as their own kinds, or as NURBS where SMLib
//!   holds the edge as one; nothing is recognised. The report counts the NURBS edges.
//! - **Sides:** SMLib keeps a face's loops relative to its own natural normal, which may be
//!   opposite the STEP form's (a surface with SMLib's `insideOut` or `swapUV`). Schema 4 keeps
//!   them relative to the record's normal, outer loop counter-clockwise: a face whose outer loop
//!   comes out clockwise in the record's parameters has its loops reversed and its sides flipped.
//! - **Shells and regions:** SMLib keeps a closed boundary as two runtime shells, one per side,
//!   each face's use in a shell facing into that shell's region; schema 4 keeps one record, its
//!   oriented faces as the side in the region that is not material uses them (out of the
//!   material). A sheet's one shell holds both uses of each face: the `same` one is kept. The
//!   infinite region is never a void (SMLib marks it one).
//! - **Touching:** an edge or vertex solids touching each other share is copied once per solid;
//!   an edge one solid touches itself along (four or more uses in a shell) is copied once per
//!   pair of faces SMLib's radial order puts across one sector round it, so every edge bounds
//!   two faces.
//! - **Order:** faces, edges and vertices renumbered in the canonical walk; each edge's uses
//!   from that walk (`upper = !forward`), never SMLib's radial entry flags.
//! - **Pcurves:** on a B-spline face, SMLib's own UV trims, exactly: the face's parameters are
//!   SMLib's, and each trim's knots are moved onto its edge record's parameter where that is an
//!   affine image of SMLib's (a spline edge's is SMLib's own; a line's the line's STEP
//!   parameter, which the C side maps the trims onto). SMLib keeps an analytic surface's trims
//!   on the surface's internal rational B-spline, whose angular parameter is not the angle
//!   (SmCylinder.h: "the map from one to the other is not linear"), and a circle's or ellipse's
//!   on its NURBS parameter, not its angle; those pcurves are computed from the edge record on
//!   the face's surface record (`solidcraft_geom::brep::pcurve`). Every loop is moved by whole
//!   periods into the face's domain.
//! - **Tolerances:** each edge's and vertex's own as SMLib reports it (`EDGE_TOLERANCE`,
//!   `VERTEX_TOLERANCE`), never below a floor of 10⁻⁶ of the body's size (at least 10⁻⁵ mm),
//!   which is the body's; from a library without them, SMLib's intersection tolerance (or the
//!   floor where larger) for the body, every edge and every vertex.
use std::f64::consts::TAU;

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use solidcraft_geom::brep as b;

use crate::flat::{self, *};
use crate::history::{Order, Piece};
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
    EDGEUSE_NEXT_RADIAL,
    EDGEUSE_RADIAL_ENTRY,
    EDGEUSE_NURBS_VERTEX_COUNT,
    EDGEUSE_NURBS_ORDER,
    EDGE_CURVE_TYPE,
    EDGE_VERTICES,
    EDGE_NURBS_VERTEX_COUNT,
    EDGE_NURBS_ORDER,
    FACE_NURBS_U_VERTEX_COUNT,
    FACE_NURBS_V_VERTEX_COUNT,
    FACE_NURBS_U_ORDER,
    FACE_NURBS_V_ORDER,
    FACE_NURBS_FORM,
    EDGE_NURBS_FORM,
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
    EDGEUSE_NURBS_CVS,
    EDGEUSE_NURBS_KNOTS,
    EDGEUSE_NURBS_WEIGHTS,
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
    EDGE_TOLERANCE,
    VERTEX_TOLERANCE,
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
/// A frame from an origin, z axis and x axis (y = z × x).
fn frame(origin: [f64; 3], axis: [f64; 3], x: [f64; 3]) -> b::Frame {
    b::Frame { origin, x_axis: x, y_axis: cross(axis, x) }
}

// ---- Geometry ---------------------------------------------------------------------------

/// The schema record of each face's surface, in face order, and its NURBS form when the arrays give
/// one (`FACE_NURBS_FORM`: an analytic face with the NURBS SMLib holds it as).
fn surfaces(flat: View, n_faces: usize) -> Res<Vec<(b::Surface, Option<b::Surface>)>> {
    let kinds = flat.i(FACE_SURFACE_TYPE);
    if kinds.len() != n_faces {
        return Err(invalid("face surface kinds"));
    }
    // The NURBS blocks, in face order: each NURBS face's surface and each face's NURBS form.
    struct At {
        j: usize,
        cv: usize,
        uk: usize,
        vk: usize,
        w: usize,
    }
    let next_nurbs = |at_: &mut At| -> Res<b::Surface> {
        let j = at_.j;
        let nu = count(at(flat.i(FACE_NURBS_U_VERTEX_COUNT), j, "NURBS face")?, "NURBS face")?;
        let nv = count(at(flat.i(FACE_NURBS_V_VERTEX_COUNT), j, "NURBS face")?, "NURBS face")?;
        let ou = count(at(flat.i(FACE_NURBS_U_ORDER), j, "NURBS face")?, "NURBS face")?;
        let ov = count(at(flat.i(FACE_NURBS_V_ORDER), j, "NURBS face")?, "NURBS face")?;
        let n = nu.checked_mul(nv).filter(|n| *n <= b::MAX_ITEMS).ok_or_else(|| invalid("NURBS face size"))?;
        let slice = |v: &[f64], from: usize, len: usize| v.get(from..from + len).map(<[f64]>::to_vec).ok_or_else(|| invalid("NURBS face arrays"));
        let pts = slice(flat.f(FACE_NURBS_CVS), 3 * at_.cv, 3 * n)?;
        let u_knots = slice(flat.f(FACE_NURBS_U_KNOTS), at_.uk, nu + ou)?;
        let v_knots = slice(flat.f(FACE_NURBS_V_KNOTS), at_.vk, nv + ov)?;
        let weights = if flat.f(FACE_NURBS_WEIGHTS).is_empty() { vec![1.0; n] } else { slice(flat.f(FACE_NURBS_WEIGHTS), at_.w, n)? };
        *at_ = At { j: j + 1, cv: at_.cv + n, uk: at_.uk + nu + ou, vk: at_.vk + nv + ov, w: at_.w + n };
        let point = |p: usize| -> [f64; 3] { [pts[3 * p], pts[3 * p + 1], pts[3 * p + 2]] };
        Ok(if weights.iter().all(|x| *x == 1.0) {
            b::Surface::Spline { u_knots, v_knots, points: (0..nu).map(|r| (0..nv).map(|c| point(r * nv + c)).collect()).collect() }
        } else {
            let h = |p: usize| {
                let (q, wt) = (point(p), weights[p]);
                [q[0] * wt, q[1] * wt, q[2] * wt, wt]
            };
            b::Surface::RationalSpline { u_knots, v_knots, homogeneous: (0..nu).map(|r| (0..nv).map(|c| h(r * nv + c)).collect()).collect() }
        })
    };
    let mut nurbs_at = At { j: 0, cv: 0, uk: 0, vk: 0, w: 0 };
    let forms = flat.i(FACE_NURBS_FORM);
    if !forms.is_empty() && forms.len() != n_faces {
        return Err(invalid("face NURBS forms"));
    }
    let mut k = [0usize; 6];
    let mut out = Vec::with_capacity(n_faces);
    for (f, &kind) in kinds.iter().enumerate() {
        let slot = usize::try_from(kind).ok().filter(|s| *s < 6).ok_or_else(|| unsupported(format!("SMLib surface kind {kind}")))?;
        let i = at(&k, slot, "kind")?;
        if let Some(c) = k.get_mut(slot) {
            *c += 1;
        }
        let surface = match kind {
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
            SURFACE_NURBS => next_nurbs(&mut nurbs_at)?,
            other => return Err(unsupported(format!("SMLib surface kind {other}"))),
        };
        // A NURBS face's own block came first; a form follows the analytic face's canonical entry.
        let form = if kind != SURFACE_NURBS && forms.get(f) == Some(&1) { Some(next_nurbs(&mut nurbs_at)?) } else { None };
        out.push((surface, form));
    }
    Ok(out)
}

/// A circle's or ellipse's frame turned (y reversed) where needed so that its angle grows along
/// `form`, the NURBS the edge runs on over `range`: what [`b::Edge::curve_interval`] reads (an
/// ellipse's radii scale its axes, not the way its angle turns).
fn along(f: b::Frame, form: &b::Curve, range: [f64; 2]) -> Res<b::Frame> {
    let eval = b::eval::CurveEval::new(form).map_err(|e| invalid(format!("circle NURBS form: {e}")))?;
    let [t0, t1] = range;
    let (p, q) = (eval.point(t0), eval.point(t0 + 1e-3 * (t1 - t0)));
    let (Some(p), Some(q)) = (p, q) else { return Err(invalid("circle NURBS form")) };
    let (o, x, y) = (Vec3::from(f.origin), Vec3::from(f.x_axis), Vec3::from(f.y_axis));
    let angle = |s: Vec3| (s - o).dot(y).atan2((s - o).dot(x));
    let step = (angle(q) - angle(p)).rem_euclid(TAU);
    Ok(if step > std::f64::consts::PI { b::Frame { y_axis: [-f.y_axis[0], -f.y_axis[1], -f.y_axis[2]], ..f } } else { f })
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

/// How a UV trim's parameter (SMLib's edge parameter, as the arrays carry it) becomes the edge
/// record's: t ↦ scale · t + offset, scale > 0. None where the record's parameter is not an
/// affine image of SMLib's (a circle or ellipse, whose angle is not its NURBS parameter).
type TrimMap = Option<[f64; 2]>;

/// Each edge's record and interval (in SMLib's direction along it) and its [`TrimMap`], and how
/// many edges are NURBS.
fn curves(flat: View, n_edges: usize) -> Res<(Vec<(b::Curve, Option<b::Curve>, [f64; 2], TrimMap)>, usize)> {
    let kinds = flat.i(EDGE_CURVE_TYPE);
    let range = flat.f(EDGE_RANGE);
    if kinds.len() != n_edges || range.len() != 2 * n_edges {
        return Err(invalid("edge kinds or ranges"));
    }
    let forms = flat.i(EDGE_NURBS_FORM);
    if !forms.is_empty() && forms.len() != n_edges {
        return Err(invalid("edge NURBS forms"));
    }
    let (mut nurbs, mut lines, mut circles, mut ellipses) = (0usize, 0usize, 0usize, 0usize);
    let (mut cv, mut kn) = (0usize, 0usize);
    // The next NURBS block, in edge order: a NURBS edge's curve or a circle's or ellipse's form.
    let mut next_nurbs = || -> Res<Nurbs> {
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
        Ok(Nurbs { knots, points: cvs.as_chunks::<3>().0.to_vec(), weights })
    };
    let mut kept = 0usize;
    let mut out = Vec::with_capacity(n_edges);
    for (e, &kind) in kinds.iter().enumerate() {
        let r = [at(range, 2 * e, "edge range")?, at(range, 2 * e + 1, "edge range")?];
        out.push(match kind {
            CURVE_LINE => {
                let (o, d) = (v3(flat.f(EDGE_LINE_ORIGIN), lines, "line")?, v3(flat.f(EDGE_LINE_DIRECTION), lines, "line")?);
                lines += 1;
                // The C side maps a line's trims onto the line's STEP parameter, the record's.
                (b::Curve::Line { start: o, end: [o[0] + d[0], o[1] + d[1], o[2] + d[2]] }, None, r, Some([1.0, 0.0]))
            }
            CURVE_CIRCLE => {
                let f = frame(
                    v3(flat.f(EDGE_CIRCLE_CENTER), circles, "circle")?,
                    v3(flat.f(EDGE_CIRCLE_AXIS), circles, "circle")?,
                    v3(flat.f(EDGE_CIRCLE_REF_DIRECTION), circles, "circle")?,
                );
                let radius = at(flat.f(EDGE_CIRCLE_RADIUS), circles, "circle")?;
                circles += 1;
                // A circle with its NURBS form is parameterised on it: its range and the trims
                // on it are that NURBS's, so the trims carry over as they are.
                if forms.get(e) == Some(&1) {
                    let form = next_nurbs()?.record();
                    (b::Curve::Circle { frame: along(f, &form, r)?, radius }, Some(form), r, Some([1.0, 0.0]))
                } else {
                    (b::Curve::Circle { frame: f, radius }, None, r, None)
                }
            }
            CURVE_ELLIPSE => {
                let f = frame(
                    v3(flat.f(EDGE_ELLIPSE_CENTER), ellipses, "ellipse")?,
                    v3(flat.f(EDGE_ELLIPSE_AXIS), ellipses, "ellipse")?,
                    v3(flat.f(EDGE_ELLIPSE_REF_DIRECTION), ellipses, "ellipse")?,
                );
                let radii = [at(flat.f(EDGE_ELLIPSE_X_RADIUS), ellipses, "ellipse")?, at(flat.f(EDGE_ELLIPSE_Y_RADIUS), ellipses, "ellipse")?];
                ellipses += 1;
                // Like a circle: on its NURBS form, the trims on it carry over as they are.
                if forms.get(e) == Some(&1) {
                    let form = next_nurbs()?.record();
                    (b::Curve::Ellipse { frame: along(f, &form, r)?, radii }, Some(form), r, Some([1.0, 0.0]))
                } else {
                    (b::Curve::Ellipse { frame: f, radii }, None, r, None)
                }
            }
            CURVE_NURBS => {
                let c = next_nurbs()?;
                // A NURBS edge stays the NURBS SMLib holds, its trims on its own parameter: SMLib
                // gives lines, circles and ellipses as their own kinds, and nothing is recognised here.
                kept += 1;
                (c.record(), None, r, Some([1.0, 0.0]))
            }
            other => return Err(unsupported(format!("SMLib curve kind {other}"))),
        });
    }
    Ok((out, kept))
}

/// Each edgeuse record's UV trim as SMLib holds it (`EDGEUSE_NURBS_*`), along its edgeuse (the
/// C side writes a trim whose edgeuse runs against its edge reversed, on the reversed parameter
/// a + b − t of the edge's interval [a, b]); none where the arrays carry none for it (an analytic
/// face's, or an analytic edge's).
fn trims(flat: View, n_edgeuses: usize) -> Res<Vec<Option<b::Curve2>>> {
    let (counts, orders) = (flat.i(EDGEUSE_NURBS_VERTEX_COUNT), flat.i(EDGEUSE_NURBS_ORDER));
    if counts.is_empty() && orders.is_empty() {
        return Ok(vec![None; n_edgeuses]);
    }
    if counts.len() != n_edgeuses || orders.len() != n_edgeuses {
        return Err(invalid("UV trim arrays"));
    }
    let (cvs, knots, weights) = (flat.f(EDGEUSE_NURBS_CVS), flat.f(EDGEUSE_NURBS_KNOTS), flat.f(EDGEUSE_NURBS_WEIGHTS));
    let (mut cv, mut kn) = (0usize, 0usize);
    let mut out = Vec::with_capacity(n_edgeuses);
    for (&n, &o) in counts.iter().zip(orders) {
        let (n, o) = (count(n, "UV trim")?, count(o, "UV trim")?);
        if n == 0 || o == 0 {
            out.push(None);
            continue;
        }
        if o < 2 || n < o {
            return Err(invalid("UV trim order"));
        }
        let pts = cvs.get(2 * cv..2 * (cv + n)).ok_or_else(|| invalid("UV trim points"))?;
        let k = knots.get(kn..kn + n + o).ok_or_else(|| invalid("UV trim knots"))?.to_vec();
        let w = match weights {
            [] => vec![1.0; n],
            w => w.get(cv..cv + n).ok_or_else(|| invalid("UV trim weights"))?.to_vec(),
        };
        cv += n;
        kn += n + o;
        let pts = pts.as_chunks::<2>().0;
        out.push(Some(if w.iter().all(|x| *x == 1.0) {
            b::Curve2::Spline { knots: k, points: pts.to_vec() }
        } else {
            b::Curve2::RationalSpline { knots: k, homogeneous: pts.iter().zip(&w).map(|(p, w)| [p[0] * w, p[1] * w, *w]).collect() }
        }));
    }
    if 2 * cv != cvs.len() || kn != knots.len() {
        return Err(invalid("UV trim arrays: blocks do not cover them"));
    }
    Ok(out)
}

/// A trim along its edge, its parameter moved onto the edge record's ([`TrimMap`]): one whose
/// edgeuse runs against the edge (`same` false) reversed exactly (control points in reverse
/// order, knots t ↦ a + b − t), then the knots mapped. SMLib keeps a trim on its edge's
/// interval; where the two ends differ by the rounding of SMLib's own or this remap (within the
/// schema's 10⁻⁹ of the interval's magnitude, at least 1), the knots at the trim's ends are the
/// edge record's interval ends, so the pcurve is defined there; farther apart is an error.
fn mapped(trim: &b::Curve2, same: bool, [scale, offset]: [f64; 2], [t0, t1]: [f64; 2]) -> Res<b::Curve2> {
    fn along<T: Clone>(knots: &[f64], points: &[T], same: bool) -> (Vec<f64>, Vec<T>) {
        if same {
            return (knots.to_vec(), points.to_vec());
        }
        let (a, b) = (knots.first().copied().unwrap_or(0.0), knots.last().copied().unwrap_or(0.0));
        (knots.iter().rev().map(|t| a + b - t).collect(), points.iter().rev().cloned().collect())
    }
    let map = |k: Vec<f64>, n: usize| -> Res<Vec<f64>> {
        let mut k: Vec<f64> = k.into_iter().map(|t| scale * t + offset).collect();
        let degree = k.len().checked_sub(n + 1).ok_or_else(|| invalid("UV trim knots"))?;
        let (a, z) = (at(&k, degree, "UV trim knots")?, at(&k, n, "UV trim knots")?);
        let slack = 1e-9 * t0.abs().max(t1.abs()).max(1.0);
        if (a - t0).abs() > slack || (z - t1).abs() > slack {
            return Err(invalid(format!("a UV trim on [{a}, {z}], its edge on [{t0}, {t1}]")));
        }
        k.iter_mut().filter(|x| **x == a).for_each(|x| *x = t0);
        k.iter_mut().filter(|x| **x == z).for_each(|x| *x = t1);
        Ok(k)
    };
    Ok(match trim {
        b::Curve2::Spline { knots, points } => {
            let (k, points) = along(knots, points, same);
            b::Curve2::Spline { knots: map(k, points.len())?, points }
        }
        b::Curve2::RationalSpline { knots, homogeneous } => {
            let (k, homogeneous) = along(knots, homogeneous, same);
            b::Curve2::RationalSpline { knots: map(k, homogeneous.len())?, homogeneous }
        }
    })
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
#[derive(Clone)]
struct Face {
    /// The NURBS form the face's (u, v) are on (`FACE_NURBS_FORM`), when it differs from `surface`.
    nurbs: Option<b::Surface>,
    surface: b::Surface,
    domain: [[f64; 2]; 2],
    /// SMLib's natural normal is the record's.
    agrees: bool,
    loops: Vec<Loop>,
    /// The array edgeuse record of each use, loop by loop as `loops` (none for a vertex loop).
    records: Vec<Vec<usize>>,
    /// Each use's pcurve, SMLib's own UV trim on the record's parameters, loop by loop as
    /// `loops`: on a B-spline face, for an edge whose record's parameter is an affine image of
    /// SMLib's; none elsewhere.
    given: Vec<Vec<Option<b::Curve2>>>,
}

#[derive(Clone)]
enum Loop {
    Edges(Vec<(u32, bool)>),
    Vertex(u32),
}

/// One edge before renumbering, along its record's parameter.
#[derive(Clone)]
struct Edge {
    curve: b::Curve,
    /// The NURBS form the edge's parameter is on (`EDGE_NURBS_FORM`), when it differs from `curve`.
    nurbs: Option<b::Curve>,
    interval: [f64; 2],
    vertices: [u32; 2],
    /// The record's parameter runs against SMLib's edge direction.
    reversed: bool,
    /// How SMLib's UV trims of this edge map onto the record's parameter.
    trim_map: TrimMap,
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

    // Tolerances exactly as SMLib gives them: each edge's and vertex's own, the body's the smallest
    // of them (only what needs more carries more). Without per-item tolerances, every tolerance is
    // the largest SMLib reports for the body. Nothing of ours is added: the record's tolerances go
    // back into SMLib, which makes its approximations to them.
    let largest = match flat.f(TOLERANCE) {
        [t] if t.is_finite() && *t > 0.0 => *t,
        _ => return Err(invalid("tolerance")),
    };
    let own = |id: i32, n: usize| -> Res<Option<Vec<f64>>> {
        match flat.f(id) {
            [] => Ok(None),
            t if t.len() == n && t.iter().all(|x| x.is_finite() && *x > 0.0) => Ok(Some(t.to_vec())),
            _ => Err(invalid("edge or vertex tolerances")),
        }
    };
    let n_vertex_points = flat.f(VERTEX_POSITION).len() / 3;
    let edge_tolerances = own(EDGE_TOLERANCE, flat.i(EDGE_CURVE_TYPE).len())?;
    let vertex_tolerances = own(VERTEX_TOLERANCE, n_vertex_points)?;
    let tolerance = match (&edge_tolerances, &vertex_tolerances) {
        (Some(e), Some(v)) => e.iter().chain(v).copied().fold(largest, f64::min),
        _ => largest,
    };
    let tolerances = Tolerances { body: tolerance, edges: edge_tolerances, vertices: vertex_tolerances };

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
    let (records, kept) = curves(flat, n_edges)?;
    report.nurbs_edges = kept;
    let mut edges = Vec::with_capacity(n_edges);
    for (e, (curve, nurbs, [a, z], trim_map)) in records.into_iter().enumerate() {
        if !(a.is_finite() && z.is_finite()) || a == z {
            return Err(invalid("edge interval"));
        }
        let start = index(idx(at(flat.i(EDGE_VERTICES), 2 * e, "edge vertices")?, n_vertices, "edge vertex")?)?;
        let end = index(idx(at(flat.i(EDGE_VERTICES), 2 * e + 1, "edge vertices")?, n_vertices, "edge vertex")?)?;
        let reversed = a > z;
        edges.push(Edge {
            curve,
            nurbs,
            interval: if reversed { [z, a] } else { [a, z] },
            vertices: if reversed { [end, start] } else { [start, end] },
            reversed,
            trim_map,
        });
    }

    // Faces: loops as SMLib keeps them, then turned where SMLib's normal is not the record's.
    let surfaces = surfaces(flat, n_faces)?;
    let trims = trims(flat, n_edgeuses)?;
    let range = flat.f(FACE_RANGE);
    if range.len() != 4 * n_faces {
        return Err(invalid("face ranges"));
    }
    let mut faces = Vec::with_capacity(n_faces);
    for (f, ((surface, nurbs), loops)) in surfaces.into_iter().zip(&face_loops).enumerate() {
        let r = range.get(4 * f..4 * f + 4).ok_or_else(|| invalid("face range"))?;
        // A face with a NURBS form is parameterised on it: its range and trims are that NURBS's.
        let params = nurbs.as_ref().unwrap_or(&surface);
        let natural = params.natural();
        let domain = [onto([r[0], r[2]], natural[0]), onto([r[1], r[3]], natural[1])];
        // SMLib's own trims are taken where the arrays carry them: B-spline faces, NURBS forms, and
        // planes (mapped onto the plane's own (u, v) by the export).
        let spline = matches!(params, b::Surface::Spline { .. } | b::Surface::RationalSpline { .. } | b::Surface::Plane { .. });
        let mut out = Vec::with_capacity(loops.len());
        let mut records = Vec::with_capacity(loops.len());
        let mut given = Vec::with_capacity(loops.len());
        for l in loops.clone() {
            let uses = loop_edgeuses.get(l).ok_or_else(|| invalid("loop"))?;
            if uses.is_empty() {
                let v = idx(at(flat.i(LOOP_VERTEX), l, "loop vertex")?, n_vertices, "loop vertex")?;
                out.push(Loop::Vertex(index(v)?));
                records.push(Vec::new());
                given.push(Vec::new());
                continue;
            }
            let mut l = Vec::with_capacity(uses.len());
            let mut g = Vec::with_capacity(uses.len());
            records.push(uses.clone().collect());
            for u in uses.clone() {
                let e = idx(at(flat.i(EDGEUSE_EDGE), u, "edgeuse")?, n_edges, "edgeuse edge")?;
                let same = at(flat.i(EDGEUSE_ORIENTATION), u, "edgeuse")? == 1;
                let edge = edges.get(e).ok_or_else(|| invalid("edge"))?;
                l.push((index(e)?, same != edge.reversed));
                let trim = trims.get(u).ok_or_else(|| invalid("edgeuse"))?.as_ref();
                g.push(match (spline, edge.trim_map, trim) {
                    (true, Some(map), Some(trim)) => Some(mapped(trim, same, map, edge.interval)?),
                    _ => None,
                });
            }
            out.push(Loop::Edges(l));
            given.push(g);
        }
        faces.push(Face { nurbs, surface, domain, agrees: true, loops: out, records, given });
    }
    for face in &mut faces {
        if outer_area(face, &edges, largest)? < 0.0 {
            face.agrees = false;
            for l in &mut face.loops {
                if let Loop::Edges(uses) = l {
                    uses.reverse();
                    uses.iter_mut().for_each(|u| u.1 = !u.1);
                }
            }
            face.records.iter_mut().for_each(|r| r.reverse());
            face.given.iter_mut().for_each(|g| g.reverse());
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
    // Each record shell is one closed boundary of material: the faces of one runtime shell on the
    // material side, as the empty side uses them. Solids touching along an edge or at a vertex
    // share their empty side's runtime shell but not their material side's, so each gets its own
    // record shell (the edges and vertices they share are split below). A sheet's one shell holds
    // both uses of each face: the `same` one is kept.
    let mut groups: Vec<Group> = Vec::new();
    for (s, uses) in shell_faceuses.iter().enumerate() {
        for fu in uses.clone() {
            let (f, same) = faceuse(fu)?;
            let key = match face_uses.get(f).map(Vec::as_slice) {
                Some([(a, _), (b, _)]) if a == b => {
                    if !same {
                        continue;
                    }
                    Side::Sheet(*a)
                }
                Some([(a, _), (b, _)]) => {
                    let other = if *a == s { *b } else { *a };
                    let (here, there) = (region_of.get(s).copied().unwrap_or(0), region_of.get(other).copied().unwrap_or(0));
                    match (material(here), material(there)) {
                        (false, true) => Side::Solid(other),
                        (true, false) => continue,
                        _ => return Err(unsupported("a face between two material regions or two empty ones")),
                    }
                }
                _ => return Err(invalid("a face without its two uses")),
            };
            let agrees = faces.get(f).ok_or_else(|| invalid("face"))?.agrees;
            let at = match groups.iter().position(|g| g.side == key) {
                Some(at) => at,
                None => {
                    groups.push(Group { side: key, empty: Vec::new(), faces: Vec::new() });
                    groups.len() - 1
                }
            };
            let group = groups.get_mut(at).ok_or_else(|| invalid("shell"))?;
            if !group.empty.contains(&s) {
                group.empty.push(s);
            }
            group.faces.push((index(f)?, same == agrees));
        }
    }
    let mut regions = record_regions(&region_shells, &region_of, &groups, material)?;
    let shells: Vec<Vec<(u32, bool)>> = groups.into_iter().map(|g| g.faces).collect();
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

    // Round each edge: SMLib's radial order of its uses, and the side of each face the material
    // is on, which say which faces bound the same material where a solid touches itself.
    let material_same =
        face_uses.iter().map(|uses| uses.iter().find(|(s, _)| material(region_of.get(*s).copied().unwrap_or(0))).map(|(_, same)| *same)).collect();
    let radial = radial(flat, n_edgeuses, n_edges, material_same)?;

    let parts = Parts { faces, edges, shells, regions, radial };
    let made = match canonical(parts.clone(), n_vertices, flat, &tolerances) {
        // SMLib's own tolerance for an edge or vertex can understate what its approximated
        // geometry needs here (a blend's end curve 2.3e-5 mm off its plane at 2e-5): then the
        // largest SMLib reports for the body, everywhere, as without per-edge tolerances.
        Err(e) if tolerances.edges.is_some() || tolerances.vertices.is_some() => {
            let uniform = Tolerances { body: largest, edges: None, vertices: None };
            canonical(parts, n_vertices, flat, &uniform).map_err(|_| e)
        }
        other => other,
    };
    made.map(|(brep, order)| (brep, report, order))
}

/// Which runtime shell a record shell is made from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The material side of a closed boundary.
    Solid(usize),
    /// A sheet's one shell, both uses of each face in it.
    Sheet(usize),
}

/// One record shell: its runtime shell, the runtime shells on its empty side, and its faces as
/// the empty side uses them.
struct Group {
    side: Side,
    empty: Vec<usize>,
    faces: Vec<(u32, bool)>,
}

/// The record's regions, each listing its record shells (`groups` by index), outer first.
///
/// A material region is SMLib's own. An empty one is too, except where solids touch along a
/// closed chain of edges round a pocket of empty space (a loft whose wide end is flush with a
/// block's sides cuts the slab above it loose): SMLib keeps the pocket as a void of its own,
/// bounded by faces of more than one solid, or of one solid on both sides of where it touches
/// itself. The record splits what the solids share, so no shell bounds the pocket on its own:
/// each solid's one closed boundary faces both the pocket and the empty space round the solids,
/// and the pocket is part of that space. Empty regions a solid's boundary faces together are one
/// record region: the infinite region where it is among them, otherwise the void whose outer
/// shell bounds it alone (its solids face no other region of the set).
fn record_regions(
    region_shells: &[std::ops::Range<usize>],
    region_of: &[usize],
    groups: &[Group],
    material: impl Fn(usize) -> bool,
) -> Res<Vec<b::Region>> {
    let n = region_shells.len();
    let region = |s: usize| region_of.get(s).copied().ok_or_else(|| invalid("shell region"));
    // The empty regions each solid's boundary faces, joined (each set's root its lowest region).
    let mut root: Vec<usize> = (0..n).collect();
    fn find(root: &[usize], mut r: usize) -> usize {
        while let Some(&up) = root.get(r).filter(|up| **up != r) {
            r = up;
        }
        r
    }
    for g in groups {
        let mut it = g.empty.iter();
        let Some(&first) = it.next() else { continue };
        let mut a = find(&root, region(first)?);
        for &s in it {
            let b = find(&root, region(s)?);
            let (lo, hi) = (a.min(b), a.max(b));
            if let Some(slot) = root.get_mut(hi) {
                *slot = lo;
            }
            a = lo;
        }
    }
    let set: Vec<usize> = (0..n).map(|r| find(&root, r)).collect();
    // A region's outer runtime shell bounds it alone when every solid it faces faces no region
    // but this one.
    let alone = |r: usize| -> Res<bool> {
        let Some(outer) = region_shells.get(r).and_then(|b| (b.start < b.end).then_some(b.start)) else { return Ok(false) };
        for g in groups.iter().filter(|g| g.empty.contains(&outer)) {
            for &s in &g.empty {
                if region(s)? != r {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    };
    // The record region of each set of empty regions.
    let mut kept = vec![None; n];
    for r in 0..n {
        let s = set.get(r).copied().unwrap_or(r);
        if material(r) || s != r {
            continue;
        }
        let members: Vec<usize> = (0..n).filter(|q| set.get(*q) == Some(&r)).collect();
        let keep = if members.len() == 1 || members.contains(&0) {
            r
        } else {
            let mut candidates = Vec::new();
            for &q in &members {
                if alone(q)? {
                    candidates.push(q);
                }
            }
            match candidates.as_slice() {
                [q] => *q,
                _ => return Err(unsupported("solids touching round pockets of empty space that no one shell bounds")),
            }
        };
        if let Some(slot) = kept.get_mut(r) {
            *slot = Some(keep);
        }
    }
    let mut regions = Vec::with_capacity(n);
    for (r, block) in region_shells.iter().enumerate() {
        let mut listed: Vec<u32> = Vec::new();
        let list = |s: usize, listed: &mut Vec<u32>| -> Res<()> {
            for (g, group) in groups.iter().enumerate() {
                let here = match group.side {
                    Side::Solid(m) => m == s || group.empty.contains(&s),
                    Side::Sheet(a) => a == s,
                };
                let id = index(g)?;
                if here && !listed.contains(&id) {
                    listed.push(id);
                }
            }
            Ok(())
        };
        if material(r) {
            for s in block.clone() {
                list(s, &mut listed)?;
            }
        } else {
            let s = set.get(r).copied().unwrap_or(r);
            if kept.get(s).copied().flatten() != Some(r) {
                continue;
            }
            // This region's shells first, its outer shell the record region's, then the pockets'.
            let members = std::iter::once(r).chain((0..n).filter(|q| *q != r && set.get(*q) == Some(&s)));
            for q in members {
                for s in region_shells.get(q).cloned().unwrap_or_default() {
                    list(s, &mut listed)?;
                }
            }
        }
        regions.push(b::Region { shells: listed, void: r > 0 && !material(r) });
    }
    Ok(regions)
}

/// The mapped records before renumbering.
#[derive(Clone)]
struct Parts {
    faces: Vec<Face>,
    edges: Vec<Edge>,
    shells: Vec<Vec<(u32, bool)>>,
    regions: Vec<b::Region>,
    radial: Option<Radial>,
}

/// SMLib's order of the faces round each edge (`EDGEUSE_NEXT_RADIAL`, `EDGEUSE_RADIAL_ENTRY`),
/// read where the arrays have it. Each edgeuse record is a face's two sides along the edge; a
/// right-hand traversal round the edge meets one side of each record, then its other side, then
/// the next record. Between two records lies one sector of space round the edge, empty or
/// material.
#[derive(Clone)]
struct Radial {
    /// The next record round the edge.
    next: Vec<usize>,
    /// The traversal enters the record from its face's `same` side (SMLib's top edgeuse, the
    /// one on the faceuse the surface normal points out of) and leaves from the other.
    top_entry: Vec<bool>,
    /// Per array face: whether the material is on its `same` side; none for a face with no
    /// material on either side (a sheet's).
    material_same: Vec<Option<bool>>,
}

/// The radial arrays, checked: in range, entries 0 or 1, each edge's records one cycle or more.
/// None where the arrays have neither (the export of a library without them).
fn radial(flat: View, n_edgeuses: usize, n_edges: usize, material_same: Vec<Option<bool>>) -> Res<Option<Radial>> {
    let (next, entry) = (flat.i(EDGEUSE_NEXT_RADIAL), flat.i(EDGEUSE_RADIAL_ENTRY));
    if next.is_empty() && entry.is_empty() {
        return Ok(None);
    }
    if next.len() != n_edgeuses || entry.len() != n_edgeuses {
        return Err(invalid("radial arrays"));
    }
    let edge = flat.i(EDGEUSE_EDGE).iter().map(|e| idx(*e, n_edges, "edgeuse edge")).collect::<Res<Vec<_>>>()?;
    let next = next.iter().map(|n| idx(*n, n_edgeuses, "next radial edgeuse")).collect::<Res<Vec<_>>>()?;
    // Each record is some record's next (the lists are cycles), on its own edge.
    let mut reached = vec![false; n_edgeuses];
    for (u, n) in next.iter().enumerate() {
        let slot = reached.get_mut(*n).ok_or_else(|| invalid("next radial edgeuse"))?;
        if *slot || edge.get(u) != edge.get(*n) {
            return Err(invalid("a radial list that is not a cycle round its edge"));
        }
        *slot = true;
    }
    let top_entry = entry
        .iter()
        .map(|x| match x {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid("radial entry")),
        })
        .collect::<Res<Vec<_>>>()?;
    Ok(Some(Radial { next, top_entry, material_same }))
}

/// The record in canonical order: faces in first appearance through the shells, edges through
/// the faces (a face whose first oriented face is reversed walked backwards), vertices through
/// the edges and then the vertex loops, with each face's pcurves; validated. With it, the
/// canonical index of each array face and edge.
/// The record's tolerances: the body's, and each array edge's and vertex's own where given.
struct Tolerances {
    body: f64,
    edges: Option<Vec<f64>>,
    vertices: Option<Vec<f64>>,
}

impl Tolerances {
    fn edge(&self, e: usize) -> f64 {
        self.edges.as_ref().and_then(|t| t.get(e)).copied().unwrap_or(self.body)
    }

    fn vertex(&self, v: usize) -> f64 {
        self.vertices.as_ref().and_then(|t| t.get(v)).copied().unwrap_or(self.body)
    }
}

fn canonical(parts: Parts, n_vertices: usize, flat: View, tolerances: &Tolerances) -> Res<(b::Brep, Order)> {
    let tolerance = tolerances.body;
    let Parts { mut faces, mut edges, shells, regions, radial } = parts;
    let n_edges = edges.len();
    // A copy made by the splits keeps the tolerance of the array edge or vertex it copies.
    let (mut edge_source, vertex_source) = split_shared(&mut faces, &mut edges, &shells, n_vertices)?;
    split_touching(&mut faces, &mut edges, &mut edge_source, radial.as_ref())?;
    let n_vertices = vertex_source.len();
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
        return Err(unsupported("a solid touching itself along an edge"));
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
        let source = edge_source.get(e as usize).copied().ok_or_else(|| invalid("edge"))?;
        edge_records.push(b::Edge {
            nurbs: edge.nurbs.clone(),
            curve: edge.curve.clone(),
            interval: edge.interval,
            vertices,
            tolerance: tolerances.edge(source),
            uses,
        });
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
        let mut curves =
            pcurves(face.nurbs.as_ref().unwrap_or(&face.surface), face.domain, &renumbered, &face.given, &edge_records, tolerance)?.into_iter();
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
        face_records.push(b::Face { nurbs: face.nurbs.clone(), surface: face.surface.clone(), domain: face.domain, loops });
    }
    let positions = flat.f(VERTEX_POSITION);
    let vertices = vertex_order
        .iter()
        .map(|&v| {
            let at = vertex_source.get(v as usize).copied().ok_or_else(|| invalid("vertex"))?;
            Ok(b::Vertex { point: v3(positions, at, "vertex")?, tolerance: tolerances.vertex(at) })
        })
        .collect::<Res<Vec<_>>>()?;
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
    let edge_new = placed(&edge_new)?;
    // SMLib's edges split where solids touch: every piece, with the faces it bounds.
    let mut pieces = Vec::new();
    for (e, &canonical) in edge_new.iter().enumerate() {
        let edge = edge_source.get(e).copied().ok_or_else(|| invalid("edge"))?;
        if edge_source.iter().filter(|s| **s == edge).count() > 1 {
            let record = brep.edges.get(canonical).ok_or_else(|| invalid("edge"))?;
            let faces: std::collections::BTreeSet<usize> = record.uses.iter().map(|u| u.face as usize).collect();
            pieces.push(Piece { edge, canonical, faces: faces.into_iter().collect() });
        }
    }
    pieces.sort_by_key(|p| (p.edge, p.canonical));
    let mut edges_out = Vec::with_capacity(n_edges);
    for e in 0..n_edges {
        edges_out
            .push(pieces.iter().find(|p| p.edge == e).map_or_else(|| edge_new.get(e).copied().ok_or_else(|| invalid("edge")), |p| Ok(p.canonical))?);
    }
    let order = Order { faces: placed(&face_new)?, edges: edges_out, pieces };
    Ok((brep, order))
}

/// Edges and vertices that solids touching each other share (faces of more than one shell use
/// them), split into one copy per shell, so each shell is a closed manifold boundary of its own.
/// The geometry is unchanged. Returns, for every edge and vertex after the split, the array edge
/// and vertex it is (a copy's original).
fn split_shared(faces: &mut [Face], edges: &mut Vec<Edge>, shells: &[Vec<(u32, bool)>], n_vertices: usize) -> Res<(Vec<usize>, Vec<usize>)> {
    let mut shell_of = vec![None; faces.len()];
    for (k, shell) in shells.iter().enumerate() {
        for &(f, _) in shell {
            *shell_of.get_mut(f as usize).ok_or_else(|| invalid("face"))? = Some(k);
        }
    }
    let mut edge_source: Vec<usize> = (0..edges.len()).collect();
    // Edges: the first shell to use one keeps it, every other gets a copy.
    let mut edge_shell: Vec<Option<usize>> = vec![None; edges.len()];
    let mut copies: HashMap<(u32, usize), u32> = HashMap::new();
    for (f, face) in faces.iter_mut().enumerate() {
        let Some(k) = shell_of.get(f).copied().flatten() else { continue };
        for l in &mut face.loops {
            let Loop::Edges(uses) = l else { continue };
            for (e, _) in uses.iter_mut() {
                let slot = edge_shell.get_mut(*e as usize).ok_or_else(|| invalid("edge"))?;
                match slot {
                    None => *slot = Some(k),
                    Some(owner) if *owner == k => {}
                    Some(_) => {
                        let copy = match copies.get(&(*e, k)) {
                            Some(c) => *c,
                            None => {
                                let c = index(edges.len())?;
                                edges.push(edges.get(*e as usize).ok_or_else(|| invalid("edge"))?.clone());
                                edge_source.push(*e as usize);
                                edge_shell.push(Some(k));
                                copies.insert((*e, k), c);
                                c
                            }
                        };
                        *e = copy;
                    }
                }
            }
        }
    }
    // Vertices: likewise, through the edges (each now in one shell) and the vertex loops.
    let mut vertex_source: Vec<usize> = (0..n_vertices).collect();
    let mut vertex_shell: Vec<Option<usize>> = vec![None; n_vertices];
    let mut vertex_copies: HashMap<(u32, usize), u32> = HashMap::new();
    let mut place = |v: &mut u32, k: usize| -> Res<()> {
        let slot = vertex_shell.get_mut(*v as usize).ok_or_else(|| invalid("vertex"))?;
        match slot {
            None => *slot = Some(k),
            Some(owner) if *owner == k => {}
            Some(_) => {
                let copy = match vertex_copies.get(&(*v, k)) {
                    Some(c) => *c,
                    None => {
                        let c = index(vertex_source.len())?;
                        vertex_source.push(*v as usize);
                        vertex_shell.push(Some(k));
                        vertex_copies.insert((*v, k), c);
                        c
                    }
                };
                *v = copy;
            }
        }
        Ok(())
    };
    for (e, edge) in edges.iter_mut().enumerate() {
        let Some(k) = edge_shell.get(e).copied().flatten() else { continue };
        for v in &mut edge.vertices {
            place(v, k)?;
        }
    }
    for (f, face) in faces.iter_mut().enumerate() {
        let Some(k) = shell_of.get(f).copied().flatten() else { continue };
        for l in &mut face.loops {
            if let Loop::Vertex(v) = l {
                place(v, k)?;
            }
        }
    }
    Ok((edge_source, vertex_source))
}

/// Edges one shell uses more than twice, where a solid touches itself along a line (a hole
/// tangent to its outline, holes touching each other), split so that every edge bounds two faces:
/// SMLib's radial order round the edge pairs each record with the next one across a sector, and
/// each pair gets its own copy of the edge. The pairs are those across a material sector, so the
/// surface is cut apart along the line round each wedge of material, unless that would give one
/// face two copies (a face on both sides of the line, its loop running along one copy and back
/// along the other), which SMLib refuses (its check's tests 33 and 34, coincident edges in a
/// face): then the pairs across the empty sectors, so that face meets itself across the line as
/// SMLib's crack edges do, one edge used twice. The vertices at the ends stay shared, the points
/// where the boundary meets itself: copies would give a face whose loop passes the point twice two
/// coincident vertices, which SMLib also refuses (test 32). The geometry is unchanged; each copy
/// is added to `edge_source` with its original's source.
fn split_touching(faces: &mut [Face], edges: &mut Vec<Edge>, edge_source: &mut Vec<usize>, radial: Option<&Radial>) -> Res<()> {
    // The records using each edge, and where each record is: (face, loop, position).
    let mut records_of: Vec<Vec<usize>> = vec![Vec::new(); edges.len()];
    let mut place: HashMap<usize, (usize, usize, usize)> = HashMap::new();
    for (f, face) in faces.iter().enumerate() {
        for (li, (l, records)) in face.loops.iter().zip(&face.records).enumerate() {
            let Loop::Edges(uses) = l else { continue };
            if records.len() != uses.len() {
                return Err(invalid("loop records"));
            }
            for (p, (&(e, _), &r)) in uses.iter().zip(records).enumerate() {
                records_of.get_mut(e as usize).ok_or_else(|| invalid("edge"))?.push(r);
                if place.insert(r, (f, li, p)).is_some() {
                    return Err(invalid("an edgeuse record in two loops"));
                }
            }
        }
    }
    let crowded: Vec<usize> = (0..edges.len()).filter(|e| records_of.get(*e).is_some_and(|r| r.len() > 2)).collect();
    if crowded.is_empty() {
        return Ok(());
    }
    let radial = radial.ok_or_else(|| unsupported("a solid touching itself along an edge, without SMLib's radial order"))?;
    let face_of = |r: usize| place.get(&r).map(|p| p.0).ok_or_else(|| invalid("edgeuse record"));
    let material_same = |r: usize| -> Res<bool> {
        radial.material_same.get(face_of(r)?).copied().flatten().ok_or_else(|| unsupported("a sheet touching itself along an edge"))
    };
    let top_entry = |r: usize| radial.top_entry.get(r).copied().ok_or_else(|| invalid("radial entry"));
    // Whether the sector the traversal leaves a record into, and the one it enters a record
    // from, is material: it leaves from the side it did not enter.
    let after = |r: usize| -> Res<bool> { Ok(material_same(r)? != top_entry(r)?) };
    let before = |r: usize| -> Res<bool> { Ok(material_same(r)? == top_entry(r)?) };

    for e in crowded {
        let mine = records_of.get(e).cloned().unwrap_or_default();
        let start = *mine.first().ok_or_else(|| invalid("edge"))?;
        // The edge's radial list from one of its records, keeping this edge's (another solid's
        // faces may lie between them where solids touch each other too).
        let mut cycle = Vec::with_capacity(mine.len());
        let mut r = start;
        for _ in 0..radial.next.len() {
            if mine.contains(&r) {
                cycle.push(r);
            }
            r = radial.next.get(r).copied().ok_or_else(|| invalid("next radial edgeuse"))?;
            if r == start {
                break;
            }
        }
        if r != start || cycle.len() != mine.len() {
            return Err(invalid("a radial list that does not go once round its edge's uses"));
        }
        let k = cycle.len();
        let at = |i: usize| cycle.get(i % k).copied().ok_or_else(|| invalid("radial list"));
        // Sectors alternate material and space, each seen alike from the records on its sides.
        let mut alternate = k % 2 == 0;
        for i in 0..k {
            let (a, b) = (at(i)?, at(i + 1)?);
            alternate &= after(a)? == before(b)? && after(a)? != after(b)?;
        }
        if !alternate {
            return Err(unsupported("a solid touching itself along an edge whose faces do not alternate between material and space round it"));
        }
        // The pairs across material sectors, unless one face would get two copies; then those
        // across empty sectors, unless that does too.
        let material = if after(at(0)?)? { 0 } else { 1 };
        let one_copy_per_face = |first: usize| -> Res<bool> {
            let mut pair_of: HashMap<usize, usize> = HashMap::new();
            for i in 0..k {
                let f = face_of(at(first + i)?)?;
                if *pair_of.entry(f).or_insert(i / 2) != i / 2 {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        let first = if one_copy_per_face(material)? || !one_copy_per_face(1 - material)? { material } else { 1 - material };
        for pair in 0..k / 2 {
            let (a, b) = (at(first + 2 * pair)?, at(first + 2 * pair + 1)?);
            let copy = if pair == 0 {
                e
            } else {
                let c = edges.len();
                edges.push(edges.get(e).ok_or_else(|| invalid("edge"))?.clone());
                edge_source.push(edge_source.get(e).copied().ok_or_else(|| invalid("edge"))?);
                c
            };
            for r in [a, b] {
                let (f, li, p) = place.get(&r).copied().ok_or_else(|| invalid("edgeuse record"))?;
                let slot = faces
                    .get_mut(f)
                    .and_then(|face| face.loops.get_mut(li))
                    .and_then(|l| if let Loop::Edges(u) = l { u.get_mut(p) } else { None })
                    .ok_or_else(|| invalid("edgeuse record"))?;
                slot.0 = index(copy)?;
            }
        }
    }
    Ok(())
}

/// A face's pcurves, edge loop by edge loop: SMLib's own UV trims where `given` (loop by loop as
/// `loops`) has them, the plain-record routine on the face's records for the rest; each loop moved
/// by whole periods into the face's domain.
fn pcurves(
    surface: &b::Surface,
    domain: [[f64; 2]; 2],
    loops: &[Loop],
    given: &[Vec<Option<b::Curve2>>],
    edges: &[b::Edge],
    tolerance: f64,
) -> Res<Vec<Vec<b::Curve2>>> {
    if given.len() != loops.len() {
        return Err(invalid("UV trims of a face"));
    }
    let mut uses = Vec::new();
    for (l, g) in loops.iter().zip(given) {
        let Loop::Edges(l) = l else { continue };
        if g.len() != l.len() {
            return Err(invalid("UV trims of a loop"));
        }
        let mut loop_uses = Vec::with_capacity(l.len());
        for (&(e, forward), given) in l.iter().zip(g) {
            let edge = edges.get(e as usize).ok_or_else(|| invalid("edge"))?;
            loop_uses.push(b::pcurve::EdgeUse {
                edge: e,
                curve: edge.nurbs.as_ref().unwrap_or(&edge.curve),
                interval: edge.interval,
                forward,
                tolerance: edge.tolerance,
                given: given.as_ref(),
            });
        }
        uses.push(loop_uses);
    }
    let mut curves = b::pcurve::pcurves_within(surface, &uses, tolerance, Some(domain)).map_err(|e| invalid(format!("pcurve: {e}")))?;
    into_domain(surface, domain, &mut curves);
    Ok(curves)
}

/// The signed area of a face's outer loop in the record's parameters, from its pcurves
/// (positive: counter-clockwise, the record's convention); 0 where it has no edge loop.
fn outer_area(face: &Face, edges: &[Edge], tolerance: f64) -> Res<f64> {
    let Some(Loop::Edges(outer)) = face.loops.first() else { return Ok(0.0) };
    let records: Vec<b::Edge> = edges
        .iter()
        .map(|e| b::Edge { nurbs: e.nurbs.clone(), curve: e.curve.clone(), interval: e.interval, vertices: e.vertices, tolerance, uses: Vec::new() })
        .collect();
    let given = face.given.first().map(std::slice::from_ref).unwrap_or(&[]);
    let curves = pcurves(face.nurbs.as_ref().unwrap_or(&face.surface), face.domain, &[Loop::Edges(outer.clone())], given, &records, tolerance)?;
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
