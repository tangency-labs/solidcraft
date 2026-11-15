//! Face, shell and region bounding boxes, face point and edge-crossing classification,
//! and the checked curve evaluation API entry point.
//!
//! Upstream: `source/SMLib/src/SmFace.cpp`, `source/SMLib/src/SmShell.cpp`,
//! `source/SMLib/src/SmRegion.cpp`, `source/SM_API/src/SmApiCurves.cpp`,
//! `source/SMLib/inc/SmFace.h`, `source/SMLib/inc/SmCurveClass.h`.
//!
//! | Rust                                  | Upstream                         |
//! |---------------------------------------|----------------------------------|
//! | [`evaluate_curve`]                    | `SmApiEvaluateCurve`             |
//! | [`calculate_bounding_box`]            | `SmFace::CalculateBoundingBox`   |
//! | [`calculate_shell_bounding_box_into`], [`calculate_shell_bounding_box`] | `SmShell::CalculateBoundingBox` |
//! | [`calculate_region_bounding_box_into`], [`calculate_region_bounding_box`] | `SmRegion::CalculateBoundingBox` |
//! | [`point_3d_classify`], [`Point3dClassifyOptions`] | `SmFace::Point3DClassify` |
//! | [`edge_curve_on_classify`], [`EdgeCurveOnOptions`], [`CurveOnClassification`] | `SmFace::EdgeCurveOnClassify` |
//! | [`ClassifyProblem`]                   | `SmClassifyProblem` (CO-PORTED; re-exported by `query::face::p3`) |
//!
//! Deviations: [`point_3d_classify`] and [`edge_curve_on_classify`] first refuse a face
//! with a coordinate above `1e150` (HANG-29,
//! [`crate::query::face::p3::check_face_coordinates`]); upstream does not return there.
//! A nullable curve is `Option<&Curve>`; `SmApiEvaluateCurve`'s three
//! optional output pointers become one returned `[position, d1, d2]` (all three are
//! evaluated upstream too). The face box returns its box (upstream writes its output
//! only on success). The shell and region boxes initialise `output` first and keep
//! the union of the children that succeeded before a failing child, as upstream's
//! in/out box does; [`calculate_shell_bounding_box`] / [`calculate_region_bounding_box`]
//! return the box instead. Shell and region boxes take `&mut Kernel` because the wire
//! edge traversal claims a context mark. The surface box of a face expands by the
//! face's own tolerance (what `SmBSplineSurface::CalculateBoundingBox` reads through
//! `GetFace()`): arena surfaces hold no owning face, so the B-spline/plane/revolution
//! box is computed unexpanded and expanded here, as the override's last step does.
//! `Point3DClassify` builds the surface's trimmed surface cache first
//! (`smsurf_GetSurfaceCache`, SmFace.cpp:12792) and hands it to `PointClassify`, which
//! upstream finds again through `SmCacheMgr::GetObjectCache`: built for a face with
//! loops, its face containment is done, so `PointClassify` classifies through the
//! leaf node containing the point before the vertex and loop tests (`query.face_p1`
//! `gen0160`: C++ FACE from an inside leaf, the vertex test alone gives VERTEX). The
//! cache is obtained with the face ([`face_surface_cache`]:
//! `CreateUVTrimCurves` and the face walk change the B-rep as upstream's do). Upstream switches the cache's point test off around the point
//! solves (SmFace.cpp:12801); the Rust solves take no cache, so the global solve is
//! [`Surface::global_point_solve_point_test_off`] (its face check keeps every
//! solution without fetching a cache); the cache check-out has nothing to act on. `EdgeCurveOnClassify` lifts a 2D curve to a
//! curve on the face's surface only when an upward edge use of the face is found (upstream lifts it
//! first and ignores `SetAll`'s status). No recursion: the enclosure of an edge's gap to a
//! plane (`edge_parts`, not upstream) halves Bézier pieces on an explicit stack, at most 40
//! times each and 1024 times per edge, then leaves the gap unproved.

use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::ignored;
use crate::geom::certified::{above, gamma};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::tol::{xsect_tol_3d_from_zones, zone_tol3d};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::intersect::point_class::{ClassGeometry, PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::{NOW, NOZ};
use crate::query::brep::LazyBrep;
use crate::query::face::p2::{face_surface_cache, FaceSurfaceCache};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::volume::Volume;
use crate::topology::face::p2 as fp2;
use crate::topology::shell::{self, ShellType};
use crate::topology::{
    edge, edgeuse::p1 as eu1, topology_traverser, vertex, Brep, EdgeId, EdgeuseId, FaceId, LoopId,
    RegionId, ShellId,
};
use crate::{EntityRef, Result, SolidError};

const OP_CURVE: &str = "query::face::p1::evaluate_curve";
const OP_FACE_BOX: &str = "query::face::p1::calculate_bounding_box";
const OP_SHELL_BOX: &str = "query::face::p1::calculate_shell_bounding_box";
const OP_REGION_BOX: &str = "query::face::p1::calculate_region_bounding_box";
const OP_POINT: &str = "query::face::p1::point_3d_classify";
const OP_EDGE_CURVE: &str = "query::face::p1::edge_curve_on_classify";

// ---- SmApiEvaluateCurve ------------------------------------------------------------------

/// `SmApiEvaluateCurve`: position, first and second derivative of `curve` at
/// `parameter`, which must lie in the curve's natural interval (inclusive). Evaluates
/// from the left with nonzero tangents (upstream's defaults).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] (`SM_ERR_INVALID_INPUT`) for a null curve or a parameter
/// outside the natural interval (NaN included); those of the curve's evaluator.
pub fn evaluate_curve(k: &Kernel, curve: Option<&Curve>, parameter: f64) -> Result<[Vec3; 3]> {
    let curve = curve.ok_or_else(|| SolidError::invalid_input(OP_CURVE, "null curve"))?;

    // Negated in-range test so a NaN parameter is rejected too.
    let domain = curve.natural_interval();
    if !(parameter >= domain.min() && parameter <= domain.max()) {
        return Err(SolidError::invalid_input(
            OP_CURVE,
            "parameter outside natural domain",
        ));
    }

    let values = curve.evaluate(k, parameter, 2, true, true)?;
    let mut iter = values.into_iter();
    let missing = || SolidError::invalid_input(OP_CURVE, "curve returned too few derivatives");
    Ok([
        iter.next().ok_or_else(missing)?,
        iter.next().ok_or_else(missing)?,
        iter.next().ok_or_else(missing)?,
    ])
}

// ---- bounding boxes ------------------------------------------------------------------------

fn tight_face_box(k: &Kernel, lb: &mut LazyBrep<'_>, f: FaceId) -> Result<Extent3d> {
    let mut face_bbox = Extent3d::new();
    crate::query::face::p3::calculate_tight_bounding_box_into(k, lb, f, &mut face_bbox)?;
    Ok(face_bbox)
}

/// Not upstream (WR-755): the tolerance and the box within which the surfaces of two faces
/// are compared by their definitions: the larger face tolerance, and the two faces
/// together (where the surfaces must agree).
///
/// # Errors
///
/// Those of the face tolerances and bounding boxes.
pub fn surface_comparison_extent(
    k: &Kernel,
    brep: &Brep,
    face: FaceId,
    other: &Brep,
    other_face: FaceId,
) -> Result<(f64, Extent3d)> {
    let tol = fp2::tolerance(brep, face)?.max(fp2::tolerance(other, other_face)?);
    let region = calculate_bounding_box(k, brep, face, false)?
        .union(&calculate_bounding_box(k, other, other_face, false)?);
    Ok((tol, region))
}

/// Not upstream: whether the definitions show the surfaces of two faces to be different
/// surfaces: two different analytic kinds ([`Surface::is_other_analytic_kind`]), two planes
/// whose faces part by more than their tolerance band ([`planes_part`]), or two quadrics of
/// one kind that [`Surface::same_analytic_surface`] tells apart within
/// [`surface_comparison_extent`]. Faces on such surfaces meet in curves and points, however
/// parallel they leave a common edge. `false` where the definitions cannot tell (a B-spline,
/// an offset, an extrusion, a revolution): the faces may be on one surface.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without surface; those of
/// [`surface_comparison_extent`].
pub fn faces_on_other_surfaces(
    k: &Kernel,
    brep: &Brep,
    face: FaceId,
    other: &Brep,
    other_face: FaceId,
) -> Result<bool> {
    fn surface(b: &Brep, f: FaceId) -> Result<&Surface> {
        fp2::surface(b, f)?
            .ok_or_else(|| SolidError::invalid_input(OP_FACE_BOX, "face without surface"))
    }
    let (a, b) = (surface(brep, face)?, surface(other, other_face)?);
    if a.is_other_analytic_kind(b) {
        return Ok(true);
    }
    if let (SurfaceKind::Plane(pa), SurfaceKind::Plane(pb)) = (a.kind(), b.kind()) {
        return planes_part(k, brep, face, pb, other, other_face, pa);
    }
    // of one kind, only quadrics have definitions that tell two surfaces apart
    let quadric = |s: &Surface| {
        matches!(
            s.kind(),
            SurfaceKind::Cone(_) | SurfaceKind::Sphere(_) | SurfaceKind::Torus(_)
        )
    };
    if !(quadric(a) && quadric(b)) {
        return Ok(false);
    }
    let (tol, region) = surface_comparison_extent(k, brep, face, other, other_face)?;
    Ok(a.same_analytic_surface(b, tol, &region) == Some(false))
}

/// The most halvings of Bézier pieces while one edge's gap to a plane is enclosed
/// ([`edge_parts`]).
const PLANE_GAP_HALVINGS: usize = 1024;
/// The most halvings of one Bézier piece of an edge ([`edge_parts`]).
const PLANE_GAP_DEPTH: usize = 40;

/// Not upstream: whether the planar faces `face` (on `face_plane`) and `other_face` (on
/// `other_plane`) part by more than the band of their two zone tolerances: a point of either
/// face is proved to lie farther than the band from the other's plane. The gap to a plane is
/// affine, so over a planar face it is largest on the face's boundary, the exact edges. An
/// exact edge lies on the face's plane, within the edge's tolerance `τ_e` of its curve (the
/// tube, validity invariant 6), not on the curve: the curve may stand `τ_e + τ_f` off the
/// plane (an edge 3e-5 off a plane two faces share, under a 3e-5 tolerance, parts nothing).
/// So each edge curve is dropped onto the face's own plane along its normal, and the gap read
/// there, on the trace the other plane leaves in it ([`PlaneLevel`]); the exact edge is
/// within `τ_e` of that point in the plane, where the gap changes by at most `τ_e sin φ` (φ
/// between the normals), which the edge's band adds. Each edge's gap is enclosed
/// ([`edge_parts`]). A gap proved past the band shows the planes part there (two walls through
/// a common edge at 3.3e-7 rad are 2.8e-5 apart 85 mm from it); faces not proved apart (a
/// coincident strip, two planes crossing at an angle within the tolerance of each other across
/// the strip, an edge whose gap is not enclosed) are left to the sector.
fn planes_part(
    k: &Kernel,
    brep: &Brep,
    face: FaceId,
    other_plane: &crate::surface::plane::Plane,
    other: &Brep,
    other_face: FaceId,
    face_plane: &crate::surface::plane::Plane,
) -> Result<bool> {
    let band = zone_tol3d(k, fp2::tolerance(brep, face)?)
        + zone_tol3d(k, fp2::tolerance(other, other_face)?);
    Ok(plane_gap(k, brep, face, face_plane, other_plane, band)?
        || plane_gap(k, other, other_face, other_plane, face_plane, band)?)
}

/// Whether an edge of `face` (on `own`) is proved to leave the band about `plane`, read on
/// `own` (see [`planes_part`]): `faces_band` and `τ_e sin φ` for the edge's tolerance.
fn plane_gap(
    k: &Kernel,
    b: &Brep,
    face: FaceId,
    own: &crate::surface::plane::Plane,
    plane: &crate::surface::plane::Plane,
    faces_band: f64,
) -> Result<bool> {
    let Some(level) = PlaneLevel::new(own, plane) else {
        return Ok(false);
    };
    for e in topology_traverser::face_edges(b, face)? {
        let rec = b.edge(e)?;
        let Some(curve) = rec.curve.as_ref() else {
            continue;
        };
        let band = faces_band + zone_tol3d(k, edge::tolerance(b, e)?) * level.sin_phi;
        if edge_parts(curve, &rec.interval, &level, band)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The gap to a plane `B` of the point `x` dropped onto a plane `A` along `A`'s normal, as the
/// affine function `a . x - c`: with unit normals `n_A`, `n_B` and origins `o_A`, `o_B`,
/// `a = n_B - (n_A . n_B) n_A` and `c = n_B . o_B - (n_A . n_B) (n_A . o_A)`. `|a|` is the
/// sine of the angle between the planes; parallel planes give `a = 0` and the constant gap
/// between them. The coefficients carry bounds on their rounding (Higham's `gamma_k`, as in
/// [`crate::geom::certified`]): each normal unitised within `gamma_6` per component, their
/// products and sums each rounding once more, bounded together by `gamma_40` of the sizes.
#[derive(Debug, Clone, Copy)]
struct PlaneLevel {
    /// `a`, as computed.
    a: [f64; 3],
    /// A bound on the rounding of each component of `a`.
    a_err: f64,
    /// `c`, as computed.
    c: f64,
    /// A bound on the rounding of `c`.
    c_err: f64,
    /// An upper bound of `|a|`: the sine of the angle between the planes.
    sin_phi: f64,
}

impl PlaneLevel {
    /// `None` for a plane whose normal or origin is not finite, or whose normal is not proved
    /// non-zero.
    fn new(
        own: &crate::surface::plane::Plane,
        other: &crate::surface::plane::Plane,
    ) -> Option<Self> {
        let unit = |p: &crate::surface::plane::Plane| {
            let pos = p.position();
            let (o, z) = (pos.origin(), pos.z_axis());
            let len = (z.x * z.x + z.y * z.y + z.z * z.z).sqrt();
            let ok = above(len, 0.0) && len.is_finite();
            ok.then_some(([z.x / len, z.y / len, z.z / len], [o.x, o.y, o.z]))
        };
        let ((na, oa), (nb, ob)) = (unit(own)?, unit(other)?);
        let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        let s = dot(na, nb);
        let a = [nb[0] - s * na[0], nb[1] - s * na[1], nb[2] - s * na[2]];
        let c = dot(nb, ob) - s * dot(na, oa);
        let norm1 = |v: [f64; 3]| v[0].abs() + v[1].abs() + v[2].abs();
        let a_err = gamma(40);
        let c_err = gamma(40) * 2.0 * (norm1(oa) + norm1(ob));
        let sin_phi = dot(a, a).sqrt() * (1.0 + gamma(4)) + 2.0 * a_err;
        let finite = a.iter().all(|v| v.is_finite()) && c.is_finite() && c_err.is_finite();
        finite.then_some(Self {
            a,
            a_err,
            c,
            c_err,
            sin_phi,
        })
    }

    /// `a . x - c w` for the homogeneous point `(x, w)` (`x` the weighted coordinates), and a
    /// bound on its rounding with the coordinates and the weight within `err` each.
    fn homogeneous(&self, x: [f64; 3], w: f64, err: f64) -> (f64, f64) {
        let [a0, a1, a2] = self.a;
        let terms = [a0 * x[0], a1 * x[1], a2 * x[2], -(self.c * w)];
        let value = terms[0] + terms[1] + terms[2] + terms[3];
        // four products and three sums; the coefficients' and the coordinates' errors
        let size = terms[0].abs() + terms[1].abs() + terms[2].abs() + terms[3].abs();
        let a1n = a0.abs() + a1.abs() + a2.abs() + 3.0 * self.a_err;
        let bound = gamma(4) * size
            + self.a_err * (x[0].abs() + x[1].abs() + x[2].abs())
            + self.c_err * w.abs()
            + (a1n + self.c.abs() + self.c_err) * err;
        (value, bound)
    }

    /// A lower bound of the gap of the point whose homogeneous gap is `num` (within `num_err`)
    /// and weight `den` (within `den_err`); 0 where nothing positive is proved.
    fn lower(num: f64, num_err: f64, den: f64, den_err: f64) -> f64 {
        let top = num.abs() - num_err;
        let bottom = den + den_err;
        if !(above(top, 0.0) && above(den - den_err, 0.0) && bottom.is_finite()) {
            return 0.0;
        }
        // the difference, the sum and the quotient each round once
        top * (1.0 - gamma(3)) / bottom
    }

    /// An upper bound of the same gap (see [`Self::lower`]); infinite where the weight is not
    /// proved positive.
    fn upper(num: f64, num_err: f64, den: f64, den_err: f64) -> f64 {
        if !above(den - den_err, 0.0) {
            return f64::INFINITY;
        }
        (num.abs() + num_err) * (1.0 + gamma(3)) / (den - den_err)
    }
}

/// Whether the edge on `curve` over `ivl` is proved to leave the band `band` of the gap
/// `level`: a point of the curve within the interval whose certified gap exceeds the band
/// (rounded up for the sum that made it). A decision on enclosures, never on
/// samples. A line's gap is affine in its parameter, so its ends decide. A curve held as a
/// B-spline (a B-spline, an ellipse or circle, a hyperbola, a parabola: their points are the
/// B-spline's) is cut to the interval and into its Bézier pieces, on which the gap is the
/// rational Bernstein function `Σ D_i B_i / Σ W_i B_i` (`D_i` the gap of the homogeneous
/// control point, `W_i` its weight). A piece's ends are points of the edge, and with positive
/// weights every value lies in the hull of the `D_i / W_i` (Piegl and Tiller, *The NURBS
/// Book*, 2nd ed., §4.4); a piece whose hull stays within the band holds no proof and is
/// dropped, the others are halved by de Casteljau (Farouki and Rajan 1987) until an end
/// proves the gap: at most [`PLANE_GAP_DEPTH`] times each and [`PLANE_GAP_HALVINGS`] times in
/// all, then not proved. `false` (not proved) for other curves, an interval outside the
/// curve's, or weights not all positive.
///
/// # Errors
///
/// Unported or cancelled calls of the B-spline's cutting.
fn edge_parts(curve: &Curve, ivl: &Extent1d, level: &PlaneLevel, band: f64) -> Result<bool> {
    // the band as summed may round below the exact sum of its terms
    let band = band * (1.0 + gamma(2));
    if !band.is_finite() {
        return Ok(false);
    }
    let base = match curve.kind() {
        CurveKind::Line(line) => return Ok(line_parts(line, ivl, level, band)),
        CurveKind::BSpline(c) => c,
        CurveKind::Ellipse(c) => c.base(),
        CurveKind::Hyperbola(c) => c.base(),
        CurveKind::Parabola(c) => c.base(),
        _ => return Ok(false),
    };
    let Some(nurb) = base.nurb.as_ref() else {
        return Ok(false);
    };
    let (u0, um) = nurb.param_bounds();
    let (t0, t1) = (ivl.min(), ivl.max());
    if !(u0 <= t0 && t0 < t1 && t1 <= um) {
        return Ok(false);
    }
    let mut most = 0.0_f64;
    let mut w_min = f64::INFINITY;
    for cp in nurb.cpts() {
        if cp.z == NOZ {
            return Ok(false);
        }
        let w = if cp.w == NOW { 1.0 } else { cp.w };
        most = most
            .max(cp.x.abs())
            .max(cp.y.abs())
            .max(cp.z.abs())
            .max(w.abs());
        w_min = w_min.min(w);
    }
    if !(most.is_finite() && above(w_min, 0.0)) {
        return Ok(false);
    }
    let cut;
    let held = if t0 == u0 && t1 == um {
        nurb
    } else {
        let Some(c) = ignored(nurb.extract_seg(t0, t1))? else {
            return Ok(false);
        };
        cut = c;
        &cut
    };
    let Some(beziers) = ignored(held.decompose_bez())? else {
        return Ok(false);
    };
    // the knot insertions that cut the interval out and the pieces apart: each is a convex
    // combination (three roundings in its ratio, two in its sum), at most 3 (p + 1) deep,
    // of homogeneous coordinates no larger than the curve's
    let err = gamma(5 * (3 * held.degree() + 5)) * most;
    let mut stack = Vec::new();
    for bez in beziers.iter().rev() {
        let (mut d, mut w, mut d_err) = (Vec::new(), Vec::new(), 0.0_f64);
        for cp in bez.cpts() {
            let wi = if cp.w == NOW { 1.0 } else { cp.w };
            let (v, e) = level.homogeneous([cp.x, cp.y, cp.z], wi, err);
            d.push(v);
            w.push(wi);
            d_err = d_err.max(e);
        }
        stack.push(GapPiece {
            d,
            w,
            d_err,
            w_err: err,
            depth: 0,
        });
    }
    let mut halvings = 0;
    while let Some(piece) = stack.pop() {
        if piece.end_beyond(band) {
            return Ok(true);
        }
        if piece.hull_within(band) || piece.depth >= PLANE_GAP_DEPTH {
            continue;
        }
        if halvings >= PLANE_GAP_HALVINGS {
            break;
        }
        halvings += 1;
        let (left, right) = piece.halves();
        stack.push(right);
        stack.push(left);
    }
    Ok(false)
}

/// Whether an end of the line edge over `ivl` lies past `band` from the plane `level`: the gap
/// is affine along the line, so its ends hold its largest value. The ends are evaluated as the
/// line evaluates (`P + s scale V`, `s` the STEP parameter of the NURB parameter), so their
/// rounding is bounded by `gamma_16` of the point's and the vector's sizes over the
/// parameters met (the conversion's five roundings, the evaluation's three).
fn line_parts(
    line: &crate::curve::line::Line,
    ivl: &Extent1d,
    level: &PlaneLevel,
    band: f64,
) -> bool {
    let (t0, t1) = (ivl.min(), ivl.max());
    let canon = line.canonical();
    let domain = if line.is_bounded() {
        let nat = line.natural_interval();
        if !(nat.min() <= t0 && t1 <= nat.max()) {
            return false;
        }
        canon.domain.min().abs() + canon.domain.max().abs()
    } else {
        0.0
    };
    let reach = domain + t0.abs() + t1.abs();
    let (p, v) = (canon.point, canon.vector);
    let size = p.x.abs().max(p.y.abs()).max(p.z.abs());
    let step = v.x.abs().max(v.y.abs()).max(v.z.abs());
    let p_err = gamma(16) * (size + 2.0 * reach * step);
    if !p_err.is_finite() {
        return false;
    }
    [t0, t1].into_iter().any(|t| {
        let x = line.evaluate_point(t);
        let (num, num_err) = level.homogeneous([x.x, x.y, x.z], 1.0, p_err);
        above(PlaneLevel::lower(num, num_err, 1.0, 0.0), band)
    })
}

/// A Bézier piece of an edge's gap to a plane ([`edge_parts`]): the gaps `d` of its
/// homogeneous control points and their weights `w`, each within its bound, and how often it
/// was halved.
#[derive(Debug, Clone)]
struct GapPiece {
    d: Vec<f64>,
    w: Vec<f64>,
    d_err: f64,
    w_err: f64,
    depth: usize,
}

impl GapPiece {
    /// Whether an end of the piece, a point of the edge, is proved past `band`.
    fn end_beyond(&self, band: f64) -> bool {
        [
            (self.d.first(), self.w.first()),
            (self.d.last(), self.w.last()),
        ]
        .into_iter()
        .any(|end| match end {
            (Some(&d), Some(&w)) => above(PlaneLevel::lower(d, self.d_err, w, self.w_err), band),
            _ => false,
        })
    }

    /// Whether every point of the piece is proved within `band` (the hull of its gaps).
    fn hull_within(&self, band: f64) -> bool {
        self.d
            .iter()
            .zip(&self.w)
            .all(|(&d, &w)| PlaneLevel::upper(d, self.d_err, w, self.w_err) <= band)
    }

    /// Its two halves at the middle of its parameter (de Casteljau): each of the degree's
    /// levels averages once, which adds `gamma` of the degree plus one of the largest value.
    fn halves(&self) -> (Self, Self) {
        let largest = |v: &[f64], e: f64| v.iter().fold(0.0_f64, |m, x| m.max(x.abs())) + e;
        let grow = gamma(self.d.len() + 1);
        let d_err = self.d_err + grow * largest(&self.d, self.d_err);
        let w_err = self.w_err + grow * largest(&self.w, self.w_err);
        let (dl, dr) = de_casteljau_halves(&self.d);
        let (wl, wr) = de_casteljau_halves(&self.w);
        let half = |d, w| Self {
            d,
            w,
            d_err,
            w_err,
            depth: self.depth + 1,
        };
        (half(dl, wl), half(dr, wr))
    }
}

/// The Bernstein coefficients of the two halves of the polynomial with coefficients `c`.
fn de_casteljau_halves(c: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut work = c.to_vec();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while let (Some(&first), Some(&last)) = (work.first(), work.last()) {
        left.push(first);
        right.push(last);
        work = work
            .iter()
            .zip(work.iter().skip(1))
            .map(|(a, b)| 0.5 * (a + b))
            .collect();
    }
    right.reverse();
    (left, right)
}

const NORMAL_BOX: BoundingBoxRequest = BoundingBoxRequest {
    normal: true,
    pseudo: false,
    polar: false,
};

/// `SmFace::CalculateBoundingBox(rFaceBBox, bTight)` (`tight` defaults to `false`
/// upstream): with `tight`, `CalculateTightBoundingBox`. Otherwise, when the face's
/// surface is linear in U or V, the union of its vertex points (each expanded by the
/// vertex tolerance) and its edge curve boxes over the edge intervals (each expanded by
/// the edge tolerance); else the surface's box over the face's UV domain, expanded by
/// the face tolerance.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without surface (upstream dereferences null)
/// or a vertex use without vertex; those of the curve and surface boxes and of the
/// face's vertex and edge lists; with `tight`, those of
/// [`crate::query::face::p3::calculate_tight_bounding_box`].
pub fn calculate_bounding_box<'a>(
    k: &Kernel,
    b: impl Into<LazyBrep<'a>>,
    f: FaceId,
    tight: bool,
) -> Result<Extent3d> {
    calculate_bounding_box_in(k, &mut b.into(), f, tight)
}

/// [`calculate_bounding_box`] on a [`LazyBrep`] the caller keeps (for the B-rep's box,
/// which runs it for every face).
///
/// # Errors
///
/// Those of [`calculate_bounding_box`].
pub fn calculate_bounding_box_in(
    k: &Kernel,
    lb: &mut LazyBrep<'_>,
    f: FaceId,
    tight: bool,
) -> Result<Extent3d> {
    // when asked - call expensive TightBoundingBox
    if tight {
        return tight_face_box(k, lb, f);
    }
    let b = lb.get();

    let mut face_box = Extent3d::new();

    let surface = fp2::surface(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_FACE_BOX, "face without surface"))?;
    // (GetDegree makes a missing NURB: a lazy surface of extrusion is still linear in V)
    let u_deg = surface.degree_with_nurb(k, SurfParamType::U)?;
    let v_deg = surface.degree_with_nurb(k, SurfParamType::V)?;

    // If one of the directions of the surface is linear we can compute the bounding box
    // using the edges and vertices.
    if u_deg == 1 || v_deg == 1 {
        for v in super::p2::face_vertices(b, f, false, None)? {
            let mut v_box = Extent3d::from_point(vertex::point(b, v)?);
            v_box.expand_absolute(vertex::tolerance(b, v)?);
            face_box = face_box.union(&v_box);
        }

        // Now load the Edge Tree
        for e in topology_traverser::face_edges(b, f)? {
            if let Some(curve) = edge::curve(b, e)? {
                let mut e_box = curve
                    .calculate_bounding_box(k, &edge::interval(b, e)?, NORMAL_BOX, true)?
                    .normal
                    .ok_or_else(|| SolidError::invalid_input(OP_FACE_BOX, "missing edge box"))?;
                e_box.expand_absolute(edge::tolerance(b, e)?);
                face_box = face_box.union(&e_box);
            }
        }
    } else {
        // Get the bounding box of the surface
        face_box = surface_box(k, surface, &fp2::uv_domain(b, f)?, fp2::tolerance(b, f)?)?;
    }

    // Set output and return
    Ok(face_box)
}

/// `pSurface->CalculateBoundingBox(GetUVDomain(), &box)` of a face's surface (expanded by
/// the zone tolerance, the default): the B-spline override (also that of planes, cones,
/// spheres, tori and surfaces of revolution) expands its normal box by the face tolerance as its last
/// step; the other kinds dispatch as is.
fn surface_box(
    k: &Kernel,
    surface: &Surface,
    uv_domain: &crate::geom::extent2d::Extent2d,
    face_tolerance: f64,
) -> Result<Extent3d> {
    let missing = || SolidError::invalid_input(OP_FACE_BOX, "missing surface box");
    match surface.kind() {
        SurfaceKind::BSpline(_)
        | SurfaceKind::Plane(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::SurfOfExtrusion(_)
        | SurfaceKind::Cone(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_) => {
            let mut normal = surface
                .calculate_bounding_box(k, uv_domain, [true, false, false], false)?
                .normal_box
                .ok_or_else(missing)?;
            normal.expand_absolute(face_tolerance);
            Ok(normal)
        }
        _ => surface
            .calculate_bounding_box(k, uv_domain, [true, false, false], true)?
            .normal_box
            .ok_or_else(missing),
    }
}

/// `SmShell::CalculateBoundingBox(rBBox, bTight)` (`tight` defaults to `false`): `output`
/// is initialised, then holds a vertex shell's vertex point, the union of a wire
/// shell's edge boxes ([`crate::query::edge::calculate_bounding_box`] over
/// [`shell::wire_edges`]) or the union of a face shell's face boxes
/// ([`calculate_bounding_box`] of each face use's face, in face-use list order); it
/// stays empty for any other shell type.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a vertex shell without vertex or a face use without
/// face (upstream dereferences null); those of the child boxes and lists. `output` then
/// holds the union of the children before the failing one.
pub fn calculate_shell_bounding_box_into(
    k: &mut Kernel,
    b: &Brep,
    id: ShellId,
    tight: bool,
    output: &mut Extent3d,
) -> Result<()> {
    output.init();

    let s = b.shell(id)?;
    match s.shell_type {
        ShellType::Vertex => {
            let v = s.vertex(b)?.ok_or_else(|| {
                SolidError::invalid_input(OP_SHELL_BOX, "vertex shell without vertex")
            })?;
            output.add_point(vertex::point(b, v)?);
        }
        ShellType::Wire => {
            for e in shell::wire_edges(k, b, id)? {
                let this_box =
                    crate::query::edge::calculate_bounding_box(k, b, e, NORMAL_BOX, tight)?
                        .normal
                        .ok_or_else(|| {
                            SolidError::invalid_input(OP_SHELL_BOX, "missing edge box")
                        })?;
                *output = output.union(&this_box);
            }
        }
        ShellType::Faceuse => {
            for fu in s.get_faceuses()? {
                let f = b.faceuse(fu)?.face.ok_or_else(|| {
                    SolidError::invalid_input(OP_SHELL_BOX, "face use without face")
                })?;
                let this_box = calculate_bounding_box(k, b, f, tight)?;
                *output = output.union(&this_box);
            }
        }
        ShellType::Unknown | ShellType::Other(_) => {}
    }

    Ok(())
}

/// [`calculate_shell_bounding_box_into`], returning the box.
///
/// # Errors
///
/// Those of [`calculate_shell_bounding_box_into`].
pub fn calculate_shell_bounding_box(
    k: &mut Kernel,
    b: &Brep,
    id: ShellId,
    tight: bool,
) -> Result<Extent3d> {
    let mut output = Extent3d::new();
    calculate_shell_bounding_box_into(k, b, id, tight, &mut output)?;
    Ok(output)
}

/// `SmRegion::CalculateBoundingBox(rBBox, bTight)` (`tight` defaults to `false`): `output`
/// is initialised, then holds the union of the region's shell boxes
/// ([`calculate_shell_bounding_box_into`]) in shell list order.
///
/// # Errors
///
/// `SM_ERR`
/// ([`QueryFailure::Failed`](crate::error::query::QueryFailure::Failed)) for a region without shells; those of the shell
/// boxes. `output` then holds the union of the shells before the failing one.
pub fn calculate_region_bounding_box_into(
    k: &mut Kernel,
    b: &Brep,
    id: RegionId,
    tight: bool,
    output: &mut Extent3d,
) -> Result<()> {
    output.init();

    let shells = b.region(id)?.get_shells(None, |_, _| Ok(false))?;
    if shells.is_empty() {
        return Err(SolidError::query_failed(
            OP_REGION_BOX,
            "the region has no shells",
        ));
    }
    let mut this_box = Extent3d::new();
    for s in shells {
        calculate_shell_bounding_box_into(k, b, s, tight, &mut this_box)?;
        *output = output.union(&this_box);
    }

    Ok(())
}

/// [`calculate_region_bounding_box_into`], returning the box.
///
/// # Errors
///
/// Those of [`calculate_region_bounding_box_into`].
pub fn calculate_region_bounding_box(
    k: &mut Kernel,
    b: &Brep,
    id: RegionId,
    tight: bool,
) -> Result<Extent3d> {
    let mut output = Extent3d::new();
    calculate_region_bounding_box_into(k, b, id, tight, &mut output)?;
    Ok(output)
}

// ---- SmFace::Point3DClassify ---------------------------------------------------------------

/// The optional arguments of [`point_3d_classify`]; [`Default`] gives upstream's
/// defaults (`pOptGuessUV = NULL`, `bTgtLoopsOnly = FALSE`, `pOptTgtLoops = NULL`,
/// `bUVSpaceOkay = TRUE`).
#[derive(Debug, Clone, Copy)]
pub struct Point3dClassifyOptions<'a> {
    /// `pOptGuessUV`: a guess for a local point drop before the global one.
    pub guess_uv: Option<Point2>,
    /// `bTgtLoopsOnly`: only classify against [`Point3dClassifyOptions::tgt_loops`].
    pub tgt_loops_only: bool,
    /// `pOptTgtLoops`: with `tgt_loops_only`, the loops to classify against (`None` or
    /// empty: the outer loop).
    pub tgt_loops: Option<&'a [LoopId]>,
    /// `bUVSpaceOkay`: UV-space classification may be tried when the 3D one is doubtful.
    pub uv_space_okay: bool,
}

impl Default for Point3dClassifyOptions<'_> {
    fn default() -> Self {
        Self {
            guess_uv: None,
            tgt_loops_only: false,
            tgt_loops: None,
            uv_space_okay: true,
        }
    }
}

/// The geometry and stored topology tolerances of one B-rep, for a classification of
/// its entities.
struct BrepGeometry<'a> {
    b: &'a Brep,
}

impl ClassGeometry<EntityRef> for BrepGeometry<'_> {
    fn curve(&self, o: EntityRef) -> Option<&Curve> {
        match o {
            EntityRef::Edge(e) => edge::curve(self.b, e).ok().flatten(),
            _ => None,
        }
    }

    fn surface(&self, o: EntityRef) -> Option<&Surface> {
        match o {
            EntityRef::Face(f) => fp2::surface(self.b, f).ok().flatten(),
            _ => None,
        }
    }

    fn volume(&self, _o: EntityRef) -> Option<&Volume> {
        None
    }

    fn point(&self, o: EntityRef) -> Option<Point3> {
        match o {
            EntityRef::Vertex(v) => vertex::point(self.b, v).ok(),
            _ => None,
        }
    }

    fn classification_vertex_point(&self, v: EntityRef) -> Option<Point3> {
        self.point(v)
    }

    fn topology_zone_tol_3d(&self, o: EntityRef) -> Option<f64> {
        match o {
            EntityRef::Vertex(v) => vertex::tolerance(self.b, v).ok(),
            EntityRef::Edge(e) => edge::tolerance(self.b, e).ok(),
            EntityRef::Face(f) => fp2::tolerance(self.b, f).ok(),
            _ => None,
        }
    }
}

/// The UV start of the first solution of a point solve.
fn solution_uv(parameters: &[f64]) -> Result<Point2> {
    let missing = || SolidError::invalid_input(OP_POINT, "point solve without parameters");
    Ok(Point2::new(
        *parameters.first().ok_or_else(missing)?,
        *parameters.get(1).ok_or_else(missing)?,
    ))
}

struct FaceSurfaceCacheGuard(Option<FaceSurfaceCache>);

impl Drop for FaceSurfaceCacheGuard {
    fn drop(&mut self) {
        if let Some(lease) = self.0.take() {
            lease.release();
        }
    }
}

/// `SmFace::Point3DClassify`: classifies the 3D point `point` against face `f` into `pc`:
/// the point is dropped onto the face's surface within `src_zone_tol_3d` (a local drop
/// from [`Point3dClassifyOptions::guess_uv`] first, else the global drop over the face's
/// UV domain); a drop outside the UV domain (by more than a thousandth of its diagonal)
/// leaves `pc` unknown; else the UV point is classified by
/// [`super::p2::point_classify`], `pc`'s gap becomes the distance from `point` to the
/// surface point, and a classification to the face, an edge or a vertex whose gap
/// exceeds the intersection tolerance of `src_zone_tol_3d` and the classified object's
/// zone tolerance is reset to unknown. `pc` starts unknown on this face with source
/// tolerance `src_zone_tol_3d`; its other members are kept unless written.
///
/// `b` is mutable because [`super::p2::point_classify`] is.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without surface (upstream dereferences null)
/// or with a coordinate above `1e150` (HANG-29); those of the point solves and of
/// [`super::p2::point_classify`]. `pc` keeps what was
/// written before the failure.
pub fn point_3d_classify(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    point: Point3,
    src_zone_tol_3d: f64,
    do_boundary_intersections: bool,
    pc: &mut PointClassification<EntityRef>,
    opts: Point3dClassifyOptions<'_>,
) -> Result<()> {
    point_3d_classify_at(
        k,
        b,
        f,
        point,
        src_zone_tol_3d,
        do_boundary_intersections,
        pc,
        opts,
    )
    .map(|_| ())
}

/// [`point_3d_classify`], also giving the face's (u, v) the point was dropped to and classified
/// at: `None` where the drop found none or fell outside the UV domain (`pc` stays unknown).
///
/// # Errors
///
/// Those of [`point_3d_classify`].
#[allow(clippy::too_many_arguments)]
pub fn point_3d_classify_at(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    point: Point3,
    src_zone_tol_3d: f64,
    do_boundary_intersections: bool,
    pc: &mut PointClassification<EntityRef>,
    opts: Point3dClassifyOptions<'_>,
) -> Result<Option<Point2>> {
    let uv_domain = fp2::uv_domain(b, f)?;
    // HANG-29: refuse coordinates the point classification cannot handle
    crate::query::face::p3::check_face_coordinates(k, b, f, OP_POINT)?;

    // init output - SM_PC_UNKNOWN is used for OUTSIDE the face's trim boundary
    pc.set_class_object(PointClassType::Unknown, None);
    pc.set_face(Some(EntityRef::Face(f)));
    pc.set_src_zone_tol_3d(src_zone_tol_3d);

    // SmSurfaceCache *pTSC = smsurf_GetSurfaceCache(pSurface): the surface's trimmed
    // surface cache (built with its face containment when the face has loops), else
    // its surface cache. PointClassify below finds the trimmed one again
    // (SmCacheMgr::GetObjectCache) and classifies through its leaf nodes first.
    let mut lease = FaceSurfaceCacheGuard(match face_surface_cache(k, b, f) {
        Ok(lease) => Some(lease),
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(e) if e.upstream_status() == Some(crate::error::SM_ERR_NULL_POINTER) => {
            return Err(SolidError::invalid_input(OP_POINT, "face without surface"));
        }
        Err(_) => None,
    });

    let uv = {
        let surface = fp2::surface(b, f)?
            .ok_or_else(|| SolidError::invalid_input(OP_POINT, "face without surface"))?;

        // when given a hint - use it in LocalSolve
        let mut found = None;
        if let Some(guess) = opts.guess_uv {
            // intersect 3dPoint with surface
            let r = surface.local_point_solve::<NoObject>(
                k,
                &uv_domain,
                SolverOperation::Intersect,
                point,
                guess,
            )?;
            if r.found {
                found = Some(solution_uv(&r.solution.start.parameters)?);
            }
        }

        // when Local Solve did not get an answer - try global solves
        if let Some(uv) = found {
            uv
        } else {
            // intersect 3DPoint with surface; no retries with larger tolerances
            // (the trimmed cache's point testing is off for the solves)
            let solutions = surface.global_point_solve_point_test_off::<NoObject>(
                k,
                Some(NoObject),
                &uv_domain,
                SolverOperation::Intersect,
                point,
                src_zone_tol_3d,
                None,
                SolutionRequested::Single,
            )?;
            let Some(first) = solutions.as_slice().first() else {
                // Point may not be inside of the boundary of the face.
                return Ok(None);
            };
            solution_uv(&first.start.parameters)?
        }
    };

    // low work - UVPoint not contained by Face's domain
    let size = uv_domain.size().length();
    if !uv_domain.contains_point(uv, size / 1000.0) {
        return Ok(None);
    }

    // else pass the call along to PointClassify to classify the UVPoint
    super::p2::point_classify(
        k,
        b,
        f,
        uv,
        pc,
        super::p2::PointClassifyOptions {
            src_zone_tol_3d,
            do_boundary_intersections,
            use_uv_classification: false,
            tgt_loops_only: opts.tgt_loops_only,
            tgt_loops: opts.tgt_loops,
            uv_space_okay: opts.uv_space_okay,
            trim_cache: lease
                .0
                .as_mut()
                .and_then(|l| l.trimmed.then_some(&mut l.cache)),
        },
    )?;

    // PointClassify set the gap to that of the surface point; use the distance between
    // the 3D point and its surface point instead.
    let surface = fp2::surface(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_POINT, "face without surface"))?;
    if let Some(surface_point) = ignored(surface.evaluate_point(k, uv))? {
        pc.set_gap_3d(point.distance(surface_point));
    }

    // If we classified to something but not within the specified tolerance, return no
    // classification.
    if pc.point_class() != PointClassType::Unknown && pc.point_class() != PointClassType::Region {
        let geometry = BrepGeometry { b };
        let xsect_tol_3d =
            xsect_tol_3d_from_zones(k, src_zone_tol_3d, pc.obj_zone_tol_3d(k, &geometry)?);
        if pc.gap_3d(k, &geometry, None)? > xsect_tol_3d {
            pc.set_class_object(PointClassType::Unknown, None);
        }
    }

    Ok(Some(uv))
}

/// The point of face `f`'s surface nearest `point`, when `point` lies on the face itself within
/// `tolerance` (on the face's trimmed area, its edges or its vertices), else `None`. The point
/// is dropped onto the surface and classified against the face's own trim loops by
/// [`point_3d_classify_at`], so a point on another face of the same surface does not pass; the
/// drop must also lie within `tolerance` of `point`.
///
/// # Errors
///
/// Those of [`point_3d_classify_at`].
pub fn point_on_face(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    point: Point3,
    tolerance: f64,
) -> Result<Option<Point3>> {
    let mut pc = PointClassification::new(zone_tol3d(k, tolerance));
    let Some(uv) = point_3d_classify_at(
        k,
        b,
        f,
        point,
        tolerance,
        true,
        &mut pc,
        Point3dClassifyOptions::default(),
    )?
    else {
        return Ok(None);
    };
    if !matches!(
        pc.point_class(),
        PointClassType::Face | PointClassType::Edge | PointClassType::Vertex
    ) {
        return Ok(None);
    }
    let surface = fp2::surface(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_POINT, "face without surface"))?;
    let Some(nearest) = ignored(surface.evaluate_point(k, uv))? else {
        return Ok(None);
    };
    Ok((point.distance(nearest) <= tolerance).then_some(nearest))
}

// ---- SmFace::EdgeCurveOnClassify -----------------------------------------------------------

/// Why a sector classification may be wrong (`SmClassifyProblem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClassifyProblem {
    /// `SM_CP_NONE`: the classification can be trusted.
    #[default]
    None = 0,
    /// `SM_CP_VERTEX_ON_SINGULARITY`.
    VertexOnSingularity = 1,
    /// `SM_CP_TOGGLED_CLASSIFICATION`: growing step-offs changed it once.
    ToggledClassification = 2,
    /// `SM_CP_SHORT_SECTOR_EDGEUSE1`: the first edge use is shorter than the step-off.
    ShortSectorEdgeuse1 = 3,
    /// `SM_CP_SHORT_SECTOR_EDGEUSE2`: the second edge use is shorter than the step-off.
    ShortSectorEdgeuse2 = 4,
    /// `SM_CP_SHORT_INTERVAL`: the curve interval is shorter than the step-off.
    ShortInterval = 5,
    /// `SM_CP_SEESAW_CLASSIFICATION`: growing step-offs changed it more than once.
    SeesawClassification = 6,
    /// `SM_CP_UNRESOLVED_CLASSIFICATION`: the iteration limit was reached.
    UnresolvedClassification = 7,
}

/// The answers of [`edge_curve_on_classify`] (`rbIsInsideFace`, `rbPossibleProblem`,
/// `reClassifyProblem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CurveOnClassification {
    /// Whether the classified curve segment is inside the face.
    pub inside: bool,
    /// Whether a classification reported a possible problem.
    pub possible_problem: bool,
    /// The problem of the last classification that reported one.
    pub problem: ClassifyProblem,
}

/// The arguments of [`edge_curve_on_classify`] after the edge and curve.
#[derive(Debug, Clone, Copy)]
pub struct EdgeCurveOnOptions {
    /// `dEdgeParam`: the edge parameter of the edge/curve intersection.
    pub edge_param: f64,
    /// `crCurveInterval`: the range of interest of the curve.
    pub curve_interval: Extent1d,
    /// `eCurveOrientation`: [`OrientType::Same`] for the curve segment after the
    /// intersection, [`OrientType::Opposite`] for the one before.
    pub curve_orientation: OrientType,
    /// `d3dCurveTolerance`: the zone tolerance of the curve's object.
    pub curve_tolerance: f64,
    /// `bUVSpaceOkay` (upstream default `TRUE`).
    pub uv_space_okay: bool,
    /// `lFaceIndx`, for debugging a target face (upstream default `SM_UNDEF_ULONG`).
    pub face_index: Option<u64>,
}

impl EdgeCurveOnOptions {
    /// The required arguments with upstream's defaults for the others.
    #[must_use]
    pub const fn new(
        edge_param: f64,
        curve_interval: Extent1d,
        curve_orientation: OrientType,
        curve_tolerance: f64,
    ) -> Self {
        Self {
            edge_param,
            curve_interval,
            curve_orientation,
            curve_tolerance,
            uv_space_okay: true,
            face_index: None,
        }
    }
}

/// `SmFace::SectorClassifyCurveOn` with the same edge use as both sector bounds (in
/// opposite directions), the edge's tolerance for both and no singularity.
fn sector_classify_curve_on(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    edgeuse: EdgeuseId,
    edge_tolerance: f64,
    curve: &Curve,
    opts: &EdgeCurveOnOptions,
) -> Result<CurveOnClassification> {
    use crate::query::face::p3::{SectorClassifyInput, SectorEdgeuse};
    let eu = |orientation| SectorEdgeuse {
        edgeuse,
        param: opts.edge_param,
        zone_tol_3d: edge_tolerance,
        orientation,
    };
    // `lFaceIndx` only selects debug output upstream
    let class = crate::query::face::p3::sector_classify_curve_on(
        k,
        b,
        f,
        &SectorClassifyInput {
            eu1: eu(OrientType::Same),
            eu2: eu(OrientType::Opposite),
            curve,
            curve_interval: opts.curve_interval,
            curve_orientation: opts.curve_orientation,
            curve_zone_tol_3d: opts.curve_tolerance,
            on_singularity: false,
            singularity_uv: Point2::new(0.0, 0.0),
            uv_space_okay: opts.uv_space_okay,
        },
    )?;
    Ok(CurveOnClassification {
        inside: class.inside,
        possible_problem: class.possible_problem,
        problem: class.problem,
    })
}

/// The 3D curve of a 2D `curve` on face `f`'s surface (`SmCrvOnSurf::SetAll`).
fn lift_to_face(k: &Kernel, b: &Brep, f: FaceId, curve: &Curve) -> Result<Curve> {
    let surface = fp2::surface(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_EDGE_CURVE, "face without surface"))?;
    Ok(Curve::new(CurveKind::CrvOnSurf(Box::new(CrvOnSurf::new(
        k,
        curve.clone_for_evaluation()?,
        surface.clone_for_evaluation()?,
        None,
        0,
    )?))))
}

/// `SmFace::EdgeCurveOnClassify`: classifies the segment of `curve` that crosses the
/// face's boundary at `edge` (at [`EdgeCurveOnOptions::edge_param`]) as inside or outside
/// the face, against every upward edge use of the edge on this face (a seam edge has
/// two; an outside answer does not stop the search). A possible problem is passed on;
/// the first inside answer ends the search. A 2D `curve` is taken as a curve on the
/// face's surface.
///
/// `b` is mutable because the sector classification is.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` for a null `edge`; [`SolidError::InvalidInput`] for a face with
/// a coordinate above `1e150` (HANG-29); those of the edge use list, the curve lift
/// and the sector classification
/// ([`crate::query::face::p3::sector_classify_curve_on`]).
pub fn edge_curve_on_classify(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    edge: Option<EdgeId>,
    curve: &Curve,
    opts: &EdgeCurveOnOptions,
) -> Result<CurveOnClassification> {
    let edge = edge.ok_or_else(|| SolidError::query_missing(OP_EDGE_CURVE, "the edge"))?;

    // init outputs
    let mut out = CurveOnClassification::default();

    let edgeuses = edge::edgeuses(b, edge, None, None)?;
    let upward = fp2::upward_faceuse(b, f)?;
    // HANG-29: refuse coordinates the sector classification cannot handle
    crate::query::face::p3::check_face_coordinates(k, b, f, OP_EDGE_CURVE)?;

    // work in 3d: a 2D curve is taken as an SmCrvOnSurf on this face's surface
    let mut lifted = None;

    // for every edgeuse
    for eu in edgeuses {
        // skip all but the upward edgeuses (closed curves get 2) connected to this face
        if eu1::face(b, eu)? != Some(f) || eu1::faceuse(b, eu)? != upward {
            continue;
        }

        // Have found upward edgeuse connected to this face - classify curve relative to it
        let eu_tol = edge::tolerance(b, edge)?;
        let this_curve = if curve.dim() == 2 {
            if lifted.is_none() {
                lifted = Some(lift_to_face(k, b, f, curve)?);
            }
            lifted
                .as_ref()
                .ok_or_else(|| SolidError::invalid_input(OP_EDGE_CURVE, "missing lifted curve"))?
        } else {
            curve
        };
        let mine = sector_classify_curve_on(k, b, f, eu, eu_tol, this_curve, opts)?;

        // pass on any detected problems
        if mine.possible_problem {
            out.possible_problem = true;
            out.problem = mine.problem;
        }

        // pass on the answer
        if mine.inside {
            out.inside = true;
            return Ok(out);
        }

        // not all done: a seam edge has two upward edge uses connected to this face
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::extent2d::Extent2d;
    use crate::geom::vector2d::Vec2;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW, NOZ};
    use crate::surface::plane::Plane;
    use crate::topology::Face;

    #[test]
    fn lift_to_face_preserves_borrowed_close_uv_knots_and_derivatives() {
        let k = Kernel::default();
        let mut b = Brep::default();
        let shell = b.add_shell().unwrap();
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let z = 2.0;
        let plane = Plane::new(
            Point3::new(0.0, 0.0, z),
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &domain,
        );
        let face = b
            .add_face_record(Face {
                surface: Some(Surface::from(plane)),
                uv_domain: domain,
                tolerance: 1e-6,
                ..Face::new(shell)
            })
            .unwrap();

        // SmBSplineCurve's constructor/Copy use SM_EFF_ZERO = 1e-12.
        // sm_FixupKnotVector collapses gaps below max(scaled_zero(0),
        // scaled_zero(1)) = 2e-12, not the surface's SM_EFF_ZERO_PARAM.
        let delta = 1e-12;
        let raw = NurbCurve::new(
            [(0.1, 0.2), (0.35, 0.2), (0.5, 0.4), (0.7, 0.6), (0.9, 0.8)]
                .into_iter()
                .map(|(x, y)| CPoint::new(x, y, NOZ, NOW))
                .collect(),
            2,
            vec![0.0, 0.0, 0.0, delta, 0.5, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let uv = Curve::from(BSplineCurve::from_nurb(raw, 2, true));
        let sample = 0.75 * delta;
        let direct = uv.evaluate(&k, sample, 1, false, false).unwrap();
        let repaired = uv.clone().evaluate(&k, sample, 1, false, false).unwrap();
        assert!((repaired[0] - direct[0]).length() > 0.01);
        assert!((repaired[1] - direct[1]).length() > 1e10);

        let lifted = lift_to_face(&k, &b, face, &uv).unwrap();
        for t in [0.0, sample, delta, 0.25] {
            let expected = uv.evaluate(&k, t, 1, false, false).unwrap();
            let actual = lifted.evaluate(&k, t, 1, false, false).unwrap();
            let point = Point3::new(expected[0].x, expected[0].y, z);
            let tangent = Vec3::new(expected[1].x, expected[1].y, 0.0);
            assert!((actual[0] - point).length() <= 1e-12);
            assert!((actual[1] - tangent).length() <= 1e-12 * tangent.length().max(1.0));
            let point_only = lifted.evaluate_point(&k, t).unwrap();
            assert!((point_only - point).length() <= 1e-12);
        }
    }

    #[test]
    fn evaluate_curve_rejects_null_nan_and_outside() {
        let k = &Kernel::default();
        assert!(evaluate_curve(k, None, 0.0).is_err());
        let c: Curve = Line::from_points(Vec3::new(0., 0., 0.), Vec3::new(1., 2., 3.), 3).into();
        assert!(evaluate_curve(k, Some(&c), f64::NAN).is_err());
        assert!(evaluate_curve(k, Some(&c), 2.0).is_err());
        let [p, d1, _] = evaluate_curve(k, Some(&c), 0.5).unwrap();
        assert_eq!(p, Vec3::new(0.5, 1.0, 1.5));
        assert_eq!(d1, Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn region_without_shells_is_sm_err() {
        let mut k = Kernel::default();
        let mut b = Brep::default();
        let r = b
            .add_region_record(crate::topology::Region::default())
            .unwrap();
        let mut out = Extent3d::from_point(Vec3::new(1., 2., 3.));
        let e = calculate_region_bounding_box_into(&mut k, &b, r, false, &mut out).unwrap_err();
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR), "{e:?}");
        assert_eq!(out, Extent3d::new());
    }

    #[test]
    fn null_edge_is_null_pointer() {
        let k = Kernel::default();
        let mut b = Brep::default();
        let c: Curve = Line::from_points(Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.), 3).into();
        let opts = EdgeCurveOnOptions::new(
            0.0,
            Extent1d::from_min_max(0.0, 1.0),
            OrientType::Same,
            1e-6,
        );
        let e = edge_curve_on_classify(&k, &mut b, FaceId::from_index(0), None, &c, &opts);
        assert!(e.is_err());
    }

    /// The plane through the line along `chord` through `origin`, turned out of z = 0 by
    /// `theta` about it towards `side` (a unit vector in z = 0 across the chord): its normal is
    /// `chord x (side cos theta + Z sin theta)`, so a point `s` across the chord in z = 0 lies
    /// `s sin theta` from it.
    fn tilted_plane(origin: Point3, chord: Vec3, side: Vec3, theta: f64) -> Plane {
        let domain = Extent2d::from_bounds(-100.0, -100.0, 100.0, 100.0);
        Plane::new(
            origin,
            chord,
            side * theta.cos() + Vec3::Z * theta.sin(),
            Vec2::new(1.0, 1.0),
            &domain,
        )
    }

    /// The plane z = 0, which the test curves lie in.
    fn flat() -> Plane {
        tilted_plane(Point3::new(0.0, 0.0, 0.0), Vec3::X, Vec3::Y, 0.0)
    }

    /// The largest distance from `plane` read at the ten points of the sampled check this
    /// replaced (the interval's ends and eight points between, evenly in the parameter).
    fn sampled_gap(k: &Kernel, c: &Curve, ivl: &Extent1d, plane: &Plane) -> f64 {
        let pos = plane.position();
        (0..=9)
            .map(|i| {
                let p = c
                    .evaluate_point(k, ivl.evaluate(f64::from(i) / 9.0))
                    .unwrap();
                (p - pos.origin()).dot(pos.z_axis()).abs()
            })
            .fold(0.0, f64::max)
    }

    /// A curved boundary whose gap to the other plane is past the band only near its middle,
    /// between the points a sampled check reads (the reviewer's case): the quadratic
    /// `y = 4 h t (1 - t)` over the chord from (0, 0) to (2, 0), against the plane turned
    /// about the chord by `theta`, `h sin theta = 1.005 band`. Its largest gap, at `t = 1/2`,
    /// is past the band; the ten samples read at most 80/81 of it, within the band. The
    /// enclosure proves the gap at the first halving, whose end is the middle. At
    /// `h sin theta = 0.99 band` the halves' hulls stay within the band: not proved.
    #[test]
    fn a_curved_edge_parts_from_a_plane_between_samples() {
        let k = Kernel::default();
        let band = 2e-5;
        let h = 10.0;
        let bump = |h: f64| {
            let raw = NurbCurve::new(
                vec![
                    CPoint::new(0.0, 0.0, 0.0, NOW),
                    CPoint::new(1.0, 2.0 * h, 0.0, NOW),
                    CPoint::new(2.0, 0.0, 0.0, NOW),
                ],
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            )
            .unwrap();
            Curve::from(BSplineCurve::from_nurb(raw, 3, false))
        };
        let c = bump(h);
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        for (ratio, parts) in [(1.005, true), (0.99, false)] {
            let theta = (ratio * band / h).asin();
            let plane = tilted_plane(Point3::new(0.0, 0.0, 0.0), Vec3::X, Vec3::Y, theta);
            let mid = c.evaluate_point(&k, 0.5).unwrap();
            let mid_gap = (mid - plane.position().origin())
                .dot(plane.position().z_axis())
                .abs();
            assert!((mid_gap / band - ratio).abs() < 1e-9, "{ratio}: {mid_gap}");
            // the sampled check saw no gap past the band
            assert!(sampled_gap(&k, &c, &ivl, &plane) < band, "{ratio}");
            let level = PlaneLevel::new(&flat(), &plane).unwrap();
            assert_eq!(
                edge_parts(&c, &ivl, &level, band).unwrap(),
                parts,
                "{ratio}"
            );
        }
    }

    /// The same on a rational curve: the quarter circle of radius `r` from (r, 0) to (0, r)
    /// (weights 1, sqrt 2 / 2, 1) against the plane turned by `theta` about its chord; the
    /// arc's middle is `r (1 - 1 / sqrt 2)` across the chord. And an edge on part of a curve:
    /// cut to its first quarter, which ends 0.78 as far across the chord as the middle, the
    /// edge stays within the band however far the rest of the curve leaves it.
    #[test]
    fn a_circular_edge_parts_from_a_plane_by_its_enclosure() {
        let k = Kernel::default();
        let band = 2e-5;
        let r = 50.0;
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let raw = NurbCurve::new(
            vec![
                CPoint::new(r, 0.0, 0.0, 1.0),
                CPoint::new(r * w, r * w, 0.0, w),
                CPoint::new(0.0, r, 0.0, 1.0),
            ],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let c = Curve::from(BSplineCurve::from_nurb(raw, 3, false));
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let across = r * (1.0 - w);
        let side = Vec3::new(-w, -w, 0.0);
        let chord = Vec3::new(-w, w, 0.0);
        for (ratio, parts) in [(1.005, true), (0.99, false)] {
            let theta = (ratio * band / across).asin();
            let plane = tilted_plane(Point3::new(r, 0.0, 0.0), chord, side, theta);
            let level = PlaneLevel::new(&flat(), &plane).unwrap();
            assert_eq!(
                edge_parts(&c, &ivl, &level, band).unwrap(),
                parts,
                "{ratio}"
            );
        }
        // the first quarter of the parameter: its far end is the farthest across the chord
        let quarter = Extent1d::from_min_max(0.0, 0.25);
        let end = c.evaluate_point(&k, 0.25).unwrap();
        let end_across = (end - Point3::new(r, 0.0, 0.0)).dot(side).abs();
        let theta = (1.005 * band / across).asin();
        let plane = tilted_plane(Point3::new(r, 0.0, 0.0), chord, side, theta);
        let level = PlaneLevel::new(&flat(), &plane).unwrap();
        assert!(end_across * theta.sin() < band);
        assert!(!edge_parts(&c, &quarter, &level, band).unwrap());
    }
}
