//! A convex blend past the faces it rolls on, trimmed at its creases in place (not
//! upstream): the geometry steps of the fork's `TrimAtCreases`.
//!
//! Not upstream: the SolidCraft fork's `source/SMLib/src/SmFilletOverflow.cpp`
//! (tangency-labs/smlib#79 step 6: 99e57b8, 668fe9f, bacd0bb, 2d4b154, bc8c44d, de5df14 and
//! 8832ead, at 5e8c9c1 on `replay/1.0.1`), behind `fixes.fillet_overflow_trim_at_creases`
//! (WR-695). A blend whose rails leave their faces across cliffs keeps its surface, built
//! on the faces continued, and the sharp edge moves: each sheet face is sectioned by the
//! body's faces past the cliffs ([`creases`]) and split along the creases inside both
//! faces ([`pieces_in_face`]); a support face whose kept rail runs past its boundary into
//! the material is carried on along its plane ([`extend`], [`make_extension_face`]). The
//! session's `trim_at_creases` (`crate::blend::fillet_executive::session`) runs the steps
//! and sews the sheet in.
//!
//! | Rust                         | Fork                         |
//! |------------------------------|------------------------------|
//! | [`plane_of`]                 | `sm_PlaneOf`                 |
//! | [`distance_to_surface`]      | `sm_DistanceToSurface`       |
//! | [`edge_on_surface`]          | `sm_EdgeOnSurface`           |
//! | [`creases`]                  | `sm_Creases`                 |
//! | [`pieces_in_face`]           | `sm_PiecesInFace`            |
//! | [`in_material`]              | `sm_InMaterial`              |
//! | [`LoopCurve`]                | `sm_LoopCurve`               |
//! | [`Extension`]                | `sm_Extension`               |
//! | `curve_ends` (private)       | `sm_CurveEnds`               |
//! | `add_curve` (private)        | `sm_AddCurve`                |
//! | [`edge_curve`]               | `sm_EdgeCurve`               |
//! | `side_line_through` (private) | `sm_SideLineThrough`        |
//! | `OffRun` (private)           | `sm_OffRun`                  |
//! | `edge_piece` (private)       | `sm_EdgePiece`               |
//! | `edge_arcs` (private)        | `sm_EdgeArcs`                |
//! | `loop_polygon` (private)     | `sm_LoopPolygon`             |
//! | `plane_axes` (private)       | `sm_PlaneAxes`               |
//! | [`extend`], [`Rail`]         | `sm_Extend`                  |
//! | [`make_extension_face`]      | `sm_MakeExtensionFace`       |
//!
//! Deviations:
//! - **Sets.** The fork's pointer-keyed `std::set<SmEdge*>` of blended edges is a slice in
//!   the solvers' order, read for membership and, where the nearest one is looked for, in
//!   that order (the fork's is address order; a tie is kept by the first either way).
//! - **Rails across B-reps.** [`extend`]'s rails are the sheet's edges and its face the
//!   body's: each rail comes as a [`Rail`] (its curve over its interval and its two
//!   vertices, compared by id only), so the body alone is lent.
//! - **Ownership.** Curves are owned values; the fork's `SmObjDelete` cleanup is the drop.
//! - **Bounded walks.** The chain walk (`for(;;)` in the fork) consumes a run each step and
//!   stops when none joins; the way round the face keeps the fork's bound (its edge
//!   count). There is no recursion.
//! - **Refusals.** `rcWhy` is the returned [`ExtendOutcome::Refused`] comment.

use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec2, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{BrepFaceServices, BrepQuery, SideObject, SidedFaceServices};
use crate::surface::plane::Plane;
use crate::surface::surface::p2::TsectCurveType;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::brep::planar_section::face_planar_section_curves;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge::NotifyEnv;
use crate::topology::face::p2::classify_free_curve_on_face;
use crate::topology::{edge, face, vertex, Brep, BrepId, EdgeId, FaceId, VertexId};

const OP: &str = "blend::fillet_overflow";

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min` of two doubles.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `sm_PlaneOf`: the plane a planar surface lies on, by a point and its unit normal (an
/// `SmPlane`, or any other surface SMLib finds planar: a loft's planar wall); `None` when
/// it is not planar (`IsPlanar(SM_EFF_ZERO, &at, &normal)`).
///
/// # Errors
/// `SM_ERR` for a zero normal; those of `IsPlanar`.
pub fn plane_of(k: &Kernel, surface: &Surface) -> Result<Option<(Point3, Vec3)>> {
    let planar = surface.is_planar(k, EFF_ZERO)?;
    if !planar.is_planar {
        return Ok(None);
    }
    let normal = planar
        .normal
        .unitize()
        .map_err(|_| SolidError::sm_err(OP))?;
    Ok(Some((planar.point, normal)))
}

/// `sm_DistanceToSurface`: the distance from `point` to face `f`'s surface: to its plane
/// when planar, else to the surface over its own natural domain (`BIG_DOUBLE` when the
/// drop fails).
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for a face without surface; those of the drop.
pub fn distance_to_surface(k: &Kernel, b: &Brep, f: FaceId, point: Point3) -> Result<f64> {
    let surface = face::p2::surface(b, f)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    if let Some((at, normal)) = plane_of(k, surface)? {
        return Ok((point - at).dot(normal).abs());
    }
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let dropped =
        surface.drop_point::<NoObject>(k, point, &domain, &DropPointOptions::default())?;
    Ok(if dropped.success {
        dropped.gap
    } else {
        BIG_DOUBLE
    })
}

/// `sm_EdgeOnSurface`: whether edge `e` of `eb` lies on face `f` of `fb`'s surface: its
/// ends and three points between within `tol`.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for an edge without curve; those of the distances.
pub fn edge_on_surface(
    k: &Kernel,
    eb: &Brep,
    e: EdgeId,
    fb: &Brep,
    f: FaceId,
    tol: f64,
) -> Result<bool> {
    let curve = edge::curve(eb, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let ivl = edge::interval(eb, e)?;
    let mut on = true;
    let mut i = 0_u32;
    while i <= 4 && on {
        let p = curve.evaluate_point(k, ivl.evaluate(f64::from(i) / 4.0))?;
        i += 1;
        on = distance_to_surface(k, fb, f, p)? <= tol;
    }
    Ok(on)
}

/// `sm_Creases`: the curves where sheet face `sheet` of `sheet_b` meets face `f` of `fb`:
/// a planar face's plane sections it exactly (`CreatePlanarSectionCurves`), any other
/// surface meets it by SMLib's surface intersection over both faces' domains, its touching
/// points left out. The 3D curves.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for a face without surface; those of the section and the
/// intersection.
pub fn creases(
    k: &Kernel,
    sheet_b: &mut Brep,
    sheet: FaceId,
    fb: &mut Brep,
    f: FaceId,
    tol: f64,
) -> Result<Vec<Curve>> {
    let sheet_surface = face::p2::surface(sheet_b, sheet)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone_for_evaluation()?;
    let other = face::p2::surface(fb, f)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone_for_evaluation()?;
    let sheet_domain = face::p2::uv_domain(sheet_b, sheet)?;
    if let Some((at, normal)) = plane_of(k, &other)? {
        let section = face_planar_section_curves(
            k,
            sheet_b,
            sheet,
            &sheet_surface,
            &sheet_domain,
            at,
            normal,
            Some(tol),
            None,
        )?;
        return Ok(section.curves_3d);
    }
    let other_domain = face::p2::uv_domain(fb, f)?;
    // both faces' surfaces through their faces' trimmed caches (the solve's boundary
    // curves), as upstream's surfaces reach their faces
    let mut sided = SidedFaceServices(vec![
        BrepFaceServices::new(sheet_b),
        BrepFaceServices::new(fb),
    ]);
    let all = sheet_surface.global_surface_intersect_with_context(
        k,
        [
            Some(SideObject(0, CacheObject::Surface(sheet))),
            Some(SideObject(1, CacheObject::Surface(f))),
        ],
        &mut sided,
        &sheet_domain,
        &other,
        &other_domain,
        [true, true],
        Some(tol),
        None,
    )?;
    let mut out = Vec::new();
    for (i, curve) in all.curves_3d.iter().enumerate() {
        if all.curve_types.get(i) == Some(&TsectCurveType::Touching) {
            continue;
        }
        out.push(curve.copy(k)?);
    }
    Ok(out)
}

/// `sm_PiecesInFace`: the pieces of `curve` that lie inside face `f` of `b` (its runs
/// classified in the face, `CurveOnClassify(TRUE, .., TRUE)`), as trimmed copies appended
/// to `pieces`. Runs along the face's boundary, or outside it, are left out, and so is a
/// run whose ends meet unless the curve is closed.
///
/// # Errors
/// Those of the classification, the evaluations and the trims.
pub fn pieces_in_face(
    k: &Kernel,
    curve: &Curve,
    b: &mut Brep,
    f: FaceId,
    tol: f64,
    pieces: &mut Vec<Curve>,
) -> Result<()> {
    let natural = curve.natural_interval();
    let runs = classify_free_curve_on_face(k, b, f, curve, natural, tol, true)?;
    for run in runs {
        if run.mid != PointClassType::Face {
            continue;
        }
        let ivl = run.interval;
        let a = curve.evaluate_point(k, ivl.min())?;
        let bb = curve.evaluate_point(k, ivl.max())?;
        if a.distance(bb) <= tol && !curve.is_closed(k, &natural, tol)? {
            continue;
        }
        let mut piece = curve.copy(k)?;
        if ivl != piece.natural_interval() {
            let mut trim = ivl;
            piece.trim(k, &mut trim, TrimMode::Notify)?;
        }
        pieces.push(piece);
    }
    Ok(())
}

/// `sm_InMaterial`: whether `point` lies in the material of `b` (a solid region, not the
/// infinite one or a void), and whether it lies on its boundary
/// (`Point3DClassify(point, GetZoneTol3d(b), TRUE)`). Returns `(in, on_boundary)`.
///
/// The classification resolves `b`'s records under the B-rep's issued identity when it
/// has one (as the fillet session classifies against its bodies), else at id `0`.
///
/// # Errors
/// Those of the classification.
pub fn in_material(k: &Kernel, b: &mut Brep, point: Point3) -> Result<(bool, bool)> {
    let zone = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
    let id = match b.notify_serial() {
        Some(serial) => {
            BrepId::from_index(u32::try_from(serial).map_err(|_| SolidError::sm_err(OP))?)
        }
        None => BrepId::from_index(0),
    };
    let class = BrepQuery::of_brep_with_id(id, b).point_3d_classify(k, b, point, zone, true)?;
    match class.point_class() {
        PointClassType::Region => {
            let inside = match class.object() {
                Some(EntityRef::Region(r)) => {
                    b.infinite_region() != Some(r) && b.region(r)?.is_void() == 0
                }
                _ => false,
            };
            Ok((inside, false))
        }
        PointClassType::Unknown => Ok((false, false)),
        _ => Ok((false, true)),
    }
}

/// `sm_LoopCurve`: a curve of a loop, owned until a face takes it, and how it runs.
#[derive(Debug, Clone)]
pub struct LoopCurve {
    /// The curve.
    pub curve: Curve,
    /// How it runs in the loop.
    pub orient: OrientType,
}

/// `sm_Extension`: a support face continued to the rails that run past its foot into the
/// material: the loop on its plane and the face's edges along that loop.
#[derive(Debug, Clone)]
pub struct Extension {
    /// A point of the face's plane.
    pub at: Point3,
    /// The plane's unit normal.
    pub normal: Vec3,
    /// The loop, as it runs.
    pub lp: Vec<LoopCurve>,
    /// The face's edges the loop runs along (the material it stops at).
    pub along: Vec<EdgeId>,
    /// Pieces of the face's edges it runs along, by their ends.
    pub along_pieces: Vec<(Point3, Point3)>,
}

impl Extension {
    fn new(at: Point3, normal: Vec3) -> Self {
        Self {
            at,
            normal,
            lp: Vec::new(),
            along: Vec::new(),
            along_pieces: Vec::new(),
        }
    }
}

/// `sm_CurveEnds`: the two ends of a curve over its natural interval.
fn curve_ends(k: &Kernel, curve: &Curve) -> Result<(Point3, Point3)> {
    let ivl = curve.natural_interval();
    Ok((
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    ))
}

/// `sm_AddCurve`: adds `curve` to `lp`, run from `from` to `to`.
fn add_curve(
    k: &Kernel,
    lp: &mut Vec<LoopCurve>,
    curve: Curve,
    from: Point3,
    to: Point3,
) -> Result<()> {
    let (start, end) = curve_ends(k, &curve)?;
    let same = start.distance(from) + end.distance(to);
    let opposite = start.distance(to) + end.distance(from);
    let orient = if same <= opposite {
        OrientType::Same
    } else {
        OrientType::Opposite
    };
    lp.push(LoopCurve { curve, orient });
    Ok(())
}

/// `sm_EdgeCurve`: a copy of the edge's curve over the edge's interval.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for an edge without curve; those of the copy and the trim.
pub fn edge_curve(k: &Kernel, b: &Brep, e: EdgeId) -> Result<Curve> {
    let curve = edge::curve(b, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let mut copy = curve.copy(k)?;
    let ivl = edge::interval(b, e)?;
    if ivl != copy.natural_interval() {
        let mut trim = ivl;
        copy.trim(k, &mut trim, TrimMode::Notify)?;
    }
    Ok(copy)
}

/// The start and end vertices of an edge (`GetStartVertex`, `GetEndVertex`).
fn ends_of(b: &Brep, e: EdgeId) -> Result<(VertexId, VertexId)> {
    let s = edge::start_vertex(b, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let t = edge::end_vertex(b, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    Ok((s, t))
}

/// `sm_SideLineThrough`: the side edge of face `f` whose line holds `point` (a straight
/// edge of the face that is not blended, `point` within `tol` of the line it lies on), and
/// its end nearer `point`. `None` when none; `SM_ERR` for two.
fn side_line_through(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    point: Point3,
    blended: &[EdgeId],
    tol: f64,
) -> Result<Option<(EdgeId, VertexId)>> {
    let mut found = None;
    for e in face::p4::edges(b, f, None, false, None)? {
        if blended.contains(&e) {
            continue;
        }
        let Some(curve) = edge::curve(b, e)? else {
            continue;
        };
        if !curve.is_linear(k, EFF_ZERO, false)?.linear {
            continue;
        }
        let (sv, ev) = ends_of(b, e)?;
        let (a, bb) = (vertex::point(b, sv)?, vertex::point(b, ev)?);
        let dir = bb - a;
        let len = dir.length();
        if len <= tol {
            continue;
        }
        let dir = dir / len;
        let to = point - a;
        let off = to - dir * to.dot(dir);
        if off.length() > tol {
            continue;
        }
        if found.is_some() {
            return Err(SolidError::sm_err(OP)); // two side lines through one point
        }
        let foot = if point.distance(a) <= point.distance(bb) {
            sv
        } else {
            ev
        };
        found = Some((e, foot));
    }
    Ok(found)
}

/// `sm_OffRun`: a run of a kept rail off its support face, in the material: its curve, and
/// at each end the sheet vertex it ends at (where the rail ends there) or the face's edge
/// it crosses there.
#[derive(Debug, Clone)]
struct OffRun {
    curve: Curve,
    ends: [Point3; 2],
    vertices: [Option<VertexId>; 2],
    crosses: [Option<EdgeId>; 2],
}

/// The drop of `point` onto `curve` over its natural interval (`DropPoint(.., NULL,
/// SM_BIG_DOUBLE, NULL, ..)`): its parameter, or `SM_ERR` when it fails or lies more than
/// `10 tol` off.
fn param_on(k: &Kernel, curve: &Curve, point: Point3, tol: f64) -> Result<f64> {
    let dropped = curve.drop_point(
        k,
        &curve.natural_interval(),
        point,
        None,
        BIG_DOUBLE,
        None,
        SolverOperation::Minimize,
    )?;
    if !dropped.success || dropped.distance > 10.0 * tol {
        return Err(SolidError::sm_err(OP));
    }
    Ok(dropped.parameter)
}

/// `sm_EdgePiece`: a copy of edge `e`'s curve from `from` to `to` (both on it); `None` for
/// a piece of no length.
fn edge_piece(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    from: Point3,
    to: Point3,
    tol: f64,
) -> Result<Option<Curve>> {
    let mut curve = edge_curve(k, b, e)?;
    let t = [
        param_on(k, &curve, from, tol)?,
        param_on(k, &curve, to, tol)?,
    ];
    let [t0, t1] = t;
    let mut ivl = Extent1d::new();
    ivl.set_min_max(smos_min(t0, t1), smos_max(t0, t1))?;
    if ivl.length() <= EFF_ZERO {
        return Ok(None);
    }
    if ivl != curve.natural_interval() {
        curve.trim(k, &mut ivl, TrimMode::Notify)?;
    }
    Ok(Some(curve))
}

/// `sm_EdgeArcs`: the pieces of edge `e` from `from` to `to` (both on it), each way along
/// it that does not leave it: the one between them, and round through its vertex as well
/// where it closes on itself.
fn edge_arcs(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    from: Point3,
    to: Point3,
    tol: f64,
) -> Result<Vec<Vec<Curve>>> {
    let curve = edge_curve(k, b, e)?;
    let all = curve.natural_interval();
    let t = [
        param_on(k, &curve, from, tol)?,
        param_on(k, &curve, to, tol)?,
    ];
    let [t0, t1] = t;
    let (lo, hi) = (smos_min(t0, t1), smos_max(t0, t1));
    let piece = |a: f64, bb: f64, arc: &mut Vec<Curve>| -> Result<()> {
        if bb - a <= EFF_ZERO * smos_max(1.0, all.length()) {
            return Ok(());
        }
        let mut p = curve.copy(k)?;
        let ivl = Extent1d::from_min_max(a, bb);
        if ivl != p.natural_interval() {
            let mut trim = ivl;
            p.trim(k, &mut trim, TrimMode::Notify)?;
        }
        arc.push(p);
        Ok(())
    };
    let mut arcs = Vec::new();
    // as the arc runs from `from`: from t0 to t1
    let mut between = Vec::new();
    piece(lo, hi, &mut between)?;
    if t0 > t1 {
        between.reverse();
    }
    arcs.push(between);
    let (sv, ev) = ends_of(b, e)?;
    if sv == ev {
        let mut to_end = Vec::new();
        let mut from_start = Vec::new();
        piece(hi, all.max(), &mut to_end)?;
        piece(all.min(), lo, &mut from_start)?;
        // from t0 round through the vertex to t1
        let round = if t0 >= t1 {
            to_end.into_iter().chain(from_start).collect()
        } else {
            from_start.into_iter().chain(to_end).collect()
        };
        arcs.push(round);
    }
    Ok(arcs)
}

/// `sm_LoopPolygon`: a loop's sampled polygon on the plane `(at, x, y)`, in order (16
/// points per curve, its end left out).
fn loop_polygon(
    k: &Kernel,
    lp: &[LoopCurve],
    at: Point3,
    x: Vec3,
    y: Vec3,
    uv: &mut Vec<Point2>,
) -> Result<()> {
    for c in lp {
        let ivl = c.curve.natural_interval();
        for i in 0..16_u32 {
            let t = if c.orient == OrientType::Same {
                f64::from(i) / 16.0
            } else {
                1.0 - f64::from(i) / 16.0
            };
            let p = c.curve.evaluate_point(k, ivl.evaluate(t))?;
            let d = p - at;
            uv.push(Point2::new(d.dot(x), d.dot(y)));
        }
    }
    Ok(())
}

/// `a[1]` when `second`, else `a[0]`.
const fn pick<T: Copy>(a: &[T; 2], second: bool) -> T {
    if second {
        a[1]
    } else {
        a[0]
    }
}

/// The signed area of a polygon (twice it: `sum a.x b.y - b.x a.y`).
fn twice_area(uv: &[Point2]) -> f64 {
    let mut area = 0.0;
    for (p, a) in uv.iter().enumerate() {
        if let Some(bb) = uv.get((p + 1) % uv.len()) {
            area += a.x * bb.y - bb.x * a.y;
        }
    }
    area
}

/// `sm_PlaneAxes`: axes in the plane square to `normal`.
fn plane_axes(normal: Vec3) -> Result<(Vec3, Vec3)> {
    let mut axis = Vec3::new(1.0, 0.0, 0.0);
    if normal.y.abs() < normal.x.abs() && normal.y.abs() <= normal.z.abs() {
        axis = Vec3::new(0.0, 1.0, 0.0);
    } else if normal.z.abs() < normal.x.abs() {
        axis = Vec3::new(0.0, 0.0, 1.0);
    }
    let x = normal
        .cross(axis)
        .unitize()
        .map_err(|_| SolidError::sm_err(OP))?;
    let y = normal.cross(x);
    Ok((x, y))
}

/// A kept rail of the sheet that lies on the face [`extend`] continues: its curve over its
/// interval ([`edge_curve`]) and its start and end vertices in the sheet.
#[derive(Debug, Clone)]
pub struct Rail {
    /// The rail's curve over its edge's interval.
    pub curve: Curve,
    /// Its start vertex in the sheet.
    pub start: VertexId,
    /// Its end vertex in the sheet.
    pub end: VertexId,
    /// The start vertex's point.
    pub start_point: Point3,
    /// The end vertex's point.
    pub end_point: Point3,
}

/// What [`extend`] makes of one face.
#[derive(Debug)]
pub enum ExtendOutcome {
    /// The extensions (none when no rail runs off the face into the material).
    Extended(Vec<Extension>),
    /// Not an extension this builds: the fork's `rcWhy`.
    Refused(&'static str),
}

/// A way round the face's boundary: its edges and the vertices between them.
struct Way {
    edges: Vec<EdgeId>,
    verts: Vec<VertexId>,
}

/// `sm_Extend`: the extension of the planar support face `f` of `target` to the kept
/// rails on its plane that run off it into the material: a chamfer's leg past its wall's
/// foot (the wall carries on down to the leg), an end rib under a lip standing on the end
/// face (the end face carries on under the lip). Each chain of such runs ends where it
/// crosses the face's boundary, or at a vertex of the sheet on the line of a side edge of
/// the face (the walls beside it meet it there), joined to that edge's end along its line.
/// The loop runs along the chain and back along the face's boundary, off the blended and
/// side edges, the way round whose inside, beside the chain, lies in the material.
///
/// # Errors
/// `SM_ERR` for two side lines through a chain's end, a zero axis or tangent, a chain end
/// that no blended edge drops onto, and a piece of an edge its end is not on; those of the
/// classifications, drops, copies and trims.
#[allow(clippy::too_many_lines)] // one fork routine
pub fn extend(
    k: &Kernel,
    target: &mut Brep,
    f: FaceId,
    rails: &[Rail],
    blended: &[EdgeId],
    tol: f64,
) -> Result<ExtendOutcome> {
    // the runs of the rails off the face, in the material
    let mut runs: Vec<OffRun> = Vec::new();
    for rail in rails {
        let curve = &rail.curve;
        let all = curve.natural_interval();
        let classes = classify_free_curve_on_face(k, target, f, curve, all, tol, true)?;
        for class in classes {
            if class.mid != PointClassType::Unknown {
                continue;
            }
            let run = class.interval;
            let a = curve.evaluate_point(k, run.min())?;
            let bb = curve.evaluate_point(k, run.max())?;
            if a.distance(bb) <= tol {
                continue;
            }
            let mid = curve.evaluate_point(k, run.evaluate(0.5))?;
            let (inside, _) = in_material(k, target, mid)?;
            if !inside {
                continue; // in the open: cut off along a crease elsewhere
            }
            let mut off = curve.copy(k)?;
            if run != off.natural_interval() {
                let mut trim = run;
                off.trim(k, &mut trim, TrimMode::Notify)?;
            }
            let mut piece = OffRun {
                curve: off,
                ends: [a, bb],
                vertices: [None, None],
                crosses: [None, None],
            };
            let classes_at = [class.start, class.end];
            let params = [run.min(), run.max()];
            for (slot, (&t, &(pc, cross))) in params.iter().zip(classes_at.iter()).enumerate() {
                let near = EFF_ZERO_SQRT * smos_max(1.0, all.length());
                let rail_end = if (t - all.min()).abs() <= near {
                    Some((rail.start, rail.start_point))
                } else if (t - all.max()).abs() <= near {
                    Some((rail.end, rail.end_point))
                } else {
                    None
                };
                let end = piece.ends.get(slot).copied().unwrap_or(Vec3::UNDEF);
                match rail_end {
                    Some((v, p)) if p.distance(end) <= 10.0 * tol => {
                        if let Some(s) = piece.vertices.get_mut(slot) {
                            *s = Some(v);
                        }
                    }
                    _ => {
                        if pc == PointClassType::Edge {
                            if let Some(s) = piece.crosses.get_mut(slot) {
                                *s = cross;
                            }
                        }
                    }
                }
                if piece.vertices.get(slot).copied().flatten().is_none()
                    && piece.crosses.get(slot).copied().flatten().is_none()
                {
                    return Ok(ExtendOutcome::Refused(
                        "Fillet Error: a blend's rail runs off a face into the material across a vertex of the face",
                    ));
                }
            }
            runs.push(piece);
        }
    }
    if runs.is_empty() {
        return Ok(ExtendOutcome::Extended(Vec::new()));
    }

    let surface = face::p2::surface(target, f)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let Some((at, normal)) = plane_of(k, surface)? else {
        return Ok(ExtendOutcome::Refused(
            "Fillet Error: a blend's rail runs past the boundary of a face that is not planar into the material",
        ));
    };
    let (x, y) = plane_axes(normal)?;
    let face_edges = face::p4::edges(target, f, None, false, None)?;

    let run =
        |i: usize| -> Result<&OffRun> { runs.get(i).ok_or_else(|| SolidError::null_pointer(OP)) };
    let mut extensions = Vec::new();
    // the chains of runs, joined at the sheet's vertices; each run once
    let mut used = vec![false; runs.len()];
    for r0 in 0..runs.len() {
        if used.get(r0).copied().unwrap_or(true) {
            continue;
        }
        // the chain as (run, reversed) pairs, its ends the first run's start and the last
        // run's end
        let mut chain: Vec<(usize, bool)> = vec![(r0, false)];
        if let Some(u) = used.get_mut(r0) {
            *u = true;
        }
        for side in 0..2 {
            loop {
                let (at_run, at_rev) = if side == 0 {
                    chain.last().copied()
                } else {
                    chain.first().copied()
                }
                .ok_or_else(|| SolidError::null_pointer(OP))?;
                let end = if side == 0 {
                    usize::from(!at_rev)
                } else {
                    usize::from(at_rev)
                };
                let Some(v) = run(at_run)?.vertices.get(end).copied().flatten() else {
                    break;
                };
                let mut next = None;
                for (q, other) in runs.iter().enumerate() {
                    if used.get(q).copied().unwrap_or(true) {
                        continue;
                    }
                    if other.vertices[0] == Some(v) {
                        next = Some((q, side != 0));
                        break;
                    }
                    if other.vertices[1] == Some(v) {
                        next = Some((q, side == 0));
                        break;
                    }
                }
                let Some((q, rev)) = next else {
                    break;
                };
                if let Some(u) = used.get_mut(q) {
                    *u = true;
                }
                if side == 0 {
                    chain.push((q, rev));
                } else {
                    chain.insert(0, (q, rev));
                }
            }
        }
        // the chain's two ends: [0] its start, [1] its end
        let &(first, first_rev) = chain.first().ok_or_else(|| SolidError::null_pointer(OP))?;
        let &(last, last_rev) = chain.last().ok_or_else(|| SolidError::null_pointer(OP))?;
        let (rf, rl) = (run(first)?, run(last)?);
        let ends = [pick(&rf.ends, first_rev), pick(&rl.ends, !last_rev)];
        let verts = [pick(&rf.vertices, first_rev), pick(&rl.vertices, !last_rev)];
        let crosses = [pick(&rf.crosses, first_rev), pick(&rl.crosses, !last_rev)];
        if verts[0].is_some() && verts[0] == verts[1] {
            return Ok(ExtendOutcome::Refused(
                "Fillet Error: a blend's rail runs all round past a face's boundary into the material",
            ));
        }
        // where each end meets the face's boundary: its crossing, or its side line's end on
        // the face
        let mut sides: [Option<EdgeId>; 2] = [None, None];
        let mut feet: [Option<VertexId>; 2] = [None, None];
        for ((cross, &end), (side, foot_slot)) in crosses
            .iter()
            .zip(ends.iter())
            .zip(sides.iter_mut().zip(feet.iter_mut()))
        {
            if cross.is_some() {
                continue;
            }
            match side_line_through(k, target, f, end, blended, 10.0 * tol)? {
                Some((s, foot)) => {
                    *side = Some(s);
                    *foot_slot = Some(foot);
                }
                None => {
                    return Ok(ExtendOutcome::Refused(
                        "Fillet Error: a blend's rail past a face's boundary ends off the lines of the face's side edges",
                    ));
                }
            }
        }
        // The ways round the face's boundary from the chain's end to its start: from a foot
        // vertex, or from a crossing to either end of the edge crossed, on along edges that
        // are neither blended nor side edges, to the start's foot or crossed edge.
        let mut ways: Vec<Way> = Vec::new();
        let mut starts: Vec<VertexId> = Vec::new();
        match (feet[1], crosses[1]) {
            (Some(foot), _) => starts.push(foot),
            (None, Some(c)) => {
                let (s, t) = ends_of(target, c)?;
                starts.push(s);
                starts.push(t);
            }
            (None, None) => return Err(SolidError::null_pointer(OP)),
        }
        let touches = |e: EdgeId, v: VertexId| -> Result<bool> {
            let (s, t) = ends_of(target, e)?;
            Ok(s == v || t == v)
        };
        for &start in &starts {
            // a start edge's own direction, from a foot: each edge at it
            let mut firsts: Vec<Option<EdgeId>> = Vec::new();
            if feet[1].is_some() {
                for &e in &face_edges {
                    if Some(e) != sides[0]
                        && Some(e) != sides[1]
                        && !blended.contains(&e)
                        && touches(e, start)?
                    {
                        firsts.push(Some(e));
                    }
                }
            } else {
                firsts.push(None); // leaving the crossed edge at `start`
            }
            for first_edge in firsts {
                let mut way = Way {
                    edges: Vec::new(),
                    verts: vec![start],
                };
                let mut at_v = start;
                let mut from = if first_edge.is_none() {
                    crosses[1]
                } else {
                    None
                };
                let mut next = first_edge;
                let mut done = false;
                let mut steps = 0;
                while steps <= face_edges.len() && !done {
                    steps += 1;
                    // arrived: at the start's foot, or at an end of the edge the start
                    // crosses
                    if next.is_none() {
                        let at_foot = feet[0] == Some(at_v);
                        let at_cross = match (crosses[0], from) {
                            (Some(c0), Some(fr)) => fr != c0 && touches(c0, at_v)?,
                            _ => false,
                        };
                        if at_foot || at_cross {
                            done = true;
                            break;
                        }
                    }
                    if next.is_none() {
                        for &e in &face_edges {
                            if Some(e) == from
                                || Some(e) == sides[0]
                                || Some(e) == sides[1]
                                || blended.contains(&e)
                                || Some(e) == crosses[1]
                                || !touches(e, at_v)?
                            {
                                continue;
                            }
                            if next.is_some() {
                                next = None; // a branch: no one way
                                break;
                            }
                            next = Some(e);
                        }
                        if next.is_none() {
                            break;
                        }
                    }
                    let Some(e) = next else {
                        break;
                    };
                    if crosses[0] == Some(e) {
                        done = true; // the start's crossing lies on this edge
                        break;
                    }
                    way.edges.push(e);
                    at_v = edge::other_vertex(target, e, Some(at_v))?
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    way.verts.push(at_v);
                    from = Some(e);
                    next = None;
                }
                if done {
                    ways.push(way);
                }
            }
        }

        // the chain's curves, as the loop runs, into a new candidate
        let chain_loop = |ext: &mut Extension| -> Result<()> {
            for &(r, rev) in &chain {
                let piece = run(r)?;
                let copy = piece.curve.copy(k)?;
                add_curve(
                    k,
                    &mut ext.lp,
                    copy,
                    pick(&piece.ends, rev),
                    pick(&piece.ends, !rev),
                )?;
            }
            Ok(())
        };
        let mut candidates: Vec<Extension> = Vec::new();
        // Both ends crossing one edge: back along that edge between them, either way round
        // it when it closes on itself (a pin's rim under a bar's bottom), or the one way
        // along it.
        if let (Some(c0), Some(c1)) = (crosses[0], crosses[1]) {
            if c0 == c1 {
                let arcs = edge_arcs(k, target, c1, ends[1], ends[0], tol)?;
                for arc in arcs {
                    let mut ext = Extension::new(at, normal);
                    chain_loop(&mut ext)?;
                    let mut from = ends[1];
                    for piece in arc {
                        let (a, bb) = curve_ends(k, &piece)?;
                        let to = if a.distance(from) <= bb.distance(from) {
                            bb
                        } else {
                            a
                        };
                        add_curve(k, &mut ext.lp, piece, from, to)?;
                        ext.along_pieces.push((from, to));
                        from = to;
                    }
                    if takes_off(k, target, &ext, &runs, &chain, (at, normal, x, y), blended)? {
                        candidates.push(ext);
                    }
                }
            }
        }
        // each way's loop; the one whose inside beside the chain lies in the material
        for way in &ways {
            let mut ext = Extension::new(at, normal);
            chain_loop(&mut ext)?;
            // the end to the way's first vertex
            let here = ends[1];
            let first_v = way
                .verts
                .first()
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let first_p = vertex::point(target, *first_v)?;
            if here.distance(first_p) > tol {
                let piece = match crosses[1] {
                    Some(c) => edge_piece(k, target, c, here, first_p, tol)?,
                    None => Some(Curve::from(BSplineCurve::create_line_segment(
                        3, here, first_p,
                    )?)),
                };
                if let Some(piece) = piece {
                    add_curve(k, &mut ext.lp, piece, here, first_p)?;
                }
                if crosses[1].is_some() {
                    ext.along_pieces.push((here, first_p));
                }
            }
            for (i, &e) in way.edges.iter().enumerate() {
                let curve = edge_curve(k, target, e)?;
                let (va, vb) = (
                    way.verts
                        .get(i)
                        .ok_or_else(|| SolidError::null_pointer(OP))?,
                    way.verts
                        .get(i + 1)
                        .ok_or_else(|| SolidError::null_pointer(OP))?,
                );
                let (pa, pb) = (vertex::point(target, *va)?, vertex::point(target, *vb)?);
                add_curve(k, &mut ext.lp, curve, pa, pb)?;
                ext.along.push(e);
            }
            // the way's last vertex to the start
            let last_v = way
                .verts
                .last()
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let last_p = vertex::point(target, *last_v)?;
            if last_p.distance(ends[0]) > tol {
                let piece = match crosses[0] {
                    Some(c) => edge_piece(k, target, c, last_p, ends[0], tol)?,
                    None => Some(Curve::from(BSplineCurve::create_line_segment(
                        3, last_p, ends[0],
                    )?)),
                };
                if let Some(piece) = piece {
                    add_curve(k, &mut ext.lp, piece, last_p, ends[0])?;
                }
                if crosses[0].is_some() {
                    ext.along_pieces.push((last_p, ends[0]));
                }
            }
            if takes_off(k, target, &ext, &runs, &chain, (at, normal, x, y), blended)? {
                candidates.push(ext);
            }
        }
        if candidates.len() != 1 {
            return Ok(ExtendOutcome::Refused(
                "Fillet Error: a blend's rail past a face's boundary does not close on the face in one way",
            ));
        }
        extensions.extend(candidates);
    }
    Ok(ExtendOutcome::Extended(extensions))
}

/// The fork's `fTakesOff` of `sm_Extend`: whether a candidate loop's inside, beside the
/// chain's middle, lies in the material, on the side of the chain the blend takes off
/// (toward the blended edge's nearest point, seen in the plane: the material may lie on
/// both sides, a lip over the end).
#[allow(clippy::too_many_arguments)]
fn takes_off(
    k: &Kernel,
    target: &mut Brep,
    ext: &Extension,
    runs: &[OffRun],
    chain: &[(usize, bool)],
    (at, normal, x, y): (Point3, Vec3, Vec3, Vec3),
    blended: &[EdgeId],
) -> Result<bool> {
    let mut uv = Vec::new();
    loop_polygon(k, &ext.lp, at, x, y, &mut uv)?;
    let area = twice_area(&uv);
    let &(r, rev) = chain.first().ok_or_else(|| SolidError::null_pointer(OP))?;
    let mid_run = runs.get(r).ok_or_else(|| SolidError::null_pointer(OP))?;
    let ivl = mid_run.curve.natural_interval();
    let d = mid_run
        .curve
        .evaluate(k, ivl.evaluate(0.5), 1, true, false)?;
    let [p, d1, ..] = *d.as_slice() else {
        return Err(SolidError::sm_err(OP));
    };
    let tan = if rev { -d1 } else { d1 };
    let len = mid_run.ends[0].distance(mid_run.ends[1]);
    let inward = normal
        .cross(tan)
        .unitize()
        .map_err(|_| SolidError::sm_err(OP))?; // left of the run in the plane, as the loop runs
    let side = if area > 0.0 { 1.0 } else { -1.0 };
    let probe = p + inward * (side * 0.01 * len);
    let (mut inside, _) = in_material(k, target, probe)?;
    let mut nearest = BIG_DOUBLE;
    let mut toward = Vec3::UNDEF;
    for &e in blended {
        let curve = edge::curve(target, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let dropped = curve.drop_point(
            k,
            &edge::interval(target, e)?,
            p,
            None,
            BIG_DOUBLE,
            None,
            SolverOperation::Minimize,
        )?;
        if !dropped.success || dropped.distance >= nearest {
            continue;
        }
        nearest = dropped.distance;
        toward = curve.evaluate_point(k, dropped.parameter)?;
    }
    if nearest == BIG_DOUBLE {
        return Err(SolidError::sm_err(OP));
    }
    let mut off = toward - p;
    off = off - normal * off.dot(normal);
    if off.dot(inward) * side <= 0.0 {
        inside = false;
    }
    Ok(inside)
}

/// `sm_MakeExtensionFace`: the face on the plane through `ext.at` square to `ext.normal`
/// bounded by `ext`'s loop (whose curves it takes), in `b`'s infinite region: the plane
/// over the loop's box, notified through `env` (a hundredth of its diagonal and ten tolerances more), the loop
/// anticlockwise.
///
/// # Errors
/// `SM_ERR` for an empty loop or a zero axis; those of `MakeFaceWithCurves`.
pub fn make_extension_face(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    mut ext: Extension,
    tol: f64,
) -> Result<FaceId> {
    let (x, y) = plane_axes(ext.normal)?;
    // the loop's points in the plane's coordinates, in its order: its box and signed area
    let mut uv = Vec::new();
    loop_polygon(k, &ext.lp, ext.at, x, y, &mut uv)?;
    let Some(&p0) = uv.first() else {
        return Err(SolidError::sm_err(OP));
    };
    let mut bbox = Extent2d::from_points(p0, p0);
    for &p in &uv {
        bbox.add_point(p);
    }
    let area = twice_area(&uv);
    if area < 0.0 {
        ext.lp.reverse();
        for c in &mut ext.lp {
            c.orient = if c.orient == OrientType::Same {
                OrientType::Opposite
            } else {
                OrientType::Same
            };
        }
    }
    let margin = 0.01 * bbox.size().length() + 10.0 * tol;
    let domain = Extent2d::from_bounds(
        bbox.min().x - margin,
        bbox.min().y - margin,
        bbox.max().x + margin,
        bbox.max().y + margin,
    );
    let plane = Plane::new(ext.at, x, y, Vec2::new(1.0, 1.0), &domain);
    let surface = Surface::from(plane);
    let natural = surface.natural_uv_domain_with_nurb(k)?;
    let input = FaceCurvesInput {
        loops: vec![ext.lp.len()],
        curves_3d: Some(ext.lp.iter().map(|c| Some(c.curve.clone())).collect()),
        uv_curves: None,
        orientations: ext.lp.iter().map(|c| c.orient).collect(),
        loop_points: Vec::new(),
    };
    let region_id = b.infinite_region();
    let key_of = env.key_of;
    let face_ref = |f: FaceId| key_of(EntityRef::Face(f));
    let made = make_face_with_curves(
        k,
        b,
        region_id,
        input,
        surface,
        &natural,
        OrientType::Same,
        None,
        &face_ref,
        env,
    )?;
    made.face.ok_or_else(|| SolidError::null_pointer(OP))
}
