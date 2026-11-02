//! Keep features on the live session (not upstream): the SolidCraft fork's
//! `source/SMLib/src/SmFilletKeepFeatures.cpp` (tangency-labs/smlib `eb883ad`,
//! `fixes.fillet_keep_features`, WR-703). A feature standing inside a round's band (the strip
//! of face the round takes, from the edge to its contact line) stays: a boss or a cut whose
//! footprint is an inner loop of a face the round rolls on.
//!
//! The round is built along its whole edge as if the feature were not there (the intermediate
//! body is the sheet of fillet and corner faces). [`FilletSession::keep_features_in_bands`]
//! then trims it in place:
//!   1. the feature's walls are carried to the round: a cut's own wall faces, a boss's side
//!      faces continued down from its footprint into the body (its footprint swept along the
//!      face's normal; the walls must be upright to a planar face);
//!   2. each sheet face is sectioned by each wall (exactly by a planar wall, by a surface
//!      intersection otherwise), the crease kept where it lies on both;
//!   3. the sheet face is split along the creases and the pieces bounded by one feature's
//!      creases alone go; a boss's continued walls are split too and keep their pieces up to
//!      the footprint;
//!   4. the kept sheet's boundary is imprinted on the body's faces, the faces it encloses
//!      round the blended edges are deleted, stopping at a kept boss's footprint, and the
//!      sheet and the boss's continued walls are sewn in, each continued wall joined to the
//!      wall it continues.
//!
//! Nothing is subtracted.
//!
//! | Rust                                     | Fork                                     |
//! |------------------------------------------|------------------------------------------|
//! | `FilletSession::keep_features_in_bands`  | `SmFilletExecutive::KeepFeaturesInBands` |
//! | `edge_on_surface` (private)              | `kf_EdgeOnSurface`                       |
//! | `edge_along` (private)                   | `kf_EdgeAlong`                           |
//! | `creases` (private)                      | `kf_Creases`                             |
//! | `pieces_in_face` (private)               | `kf_PiecesInFace`                        |
//! | `upright` (private)                      | `kf_Upright`                             |
//! | `Feature` (private)                      | `kf_Feature`                             |
//!
//! `kf_PlaneOf`, `kf_DistanceToSurface` and `kf_EdgeCurve` are the fork's repeats of the cliff
//! overflow's helpers: [`plane_of`], [`distance_to_surface`] and [`edge_curve`] here.
//!
//! Deviations:
//! - **A refusal keeps the target.** The target is copied before its first edit and put back
//!   when the step fails (the fork leaves whatever it reached), as the subtraction does.
//! - **The trace.** The fork's `SM_ERR_MESSAGE` naming a reached feature isn't kept: the
//!   port's kernel has no message channel for a step that goes on.
//! - **Merging on a face.** The fork's `MergeCurvesOnSurface` takes the face's surface; the
//!   port's takes the face, whose surface no other face of these bodies shares.
//! - **Ordering.** The fork's `std::set` and `std::map` of faces are ordered by address; here
//!   by id, used for membership and per-face lists only.

use std::collections::{BTreeMap, BTreeSet};

use super::{invalid, FilletObject, FilletSession};
use crate::blend::fillet_cliff_overflow::{distance_to_surface, edge_curve, plane_of, Refusal};
use crate::blend::fillet_geom::p1::{FilletErrorType, FilletGeomType};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::ids;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::object::ObjectRef;
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::curve_class::p2::ResetOptions;
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::knots::Side;
use crate::primitive::primitive_creation::p1::PrimitiveCreation;
use crate::query::face::p2::curve_on_classify;
use crate::surface::surface::p2::TsectCurveType;
use crate::surface::surface::Surface;
use crate::topology::brep::p3::{
    delete_face, orient_trimmed_surfaces, with_kernel_callbacks, DeleteFaceOptions,
};
use crate::topology::brep::planar_section::face_planar_section_curves;
use crate::topology::face::p3::{IntersectObject, IntersectObjects};
use crate::topology::topology_traverser::Marks;
use crate::topology::{edge, edgeuse::p1 as eu1, face, loopuse, Brep, EdgeId, FaceId};

const OP: &str = "blend::fillet_executive::session::keep_features";

fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `kf_EdgeOnSurface`: whether the edge lies on `f`'s surface: its ends and three points
/// between within `tol`.
fn edge_on_surface(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    surface_body: &Brep,
    f: FaceId,
    tol: f64,
) -> Result<bool> {
    let curve = edge::curve(b, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let ivl = edge::interval(b, e)?;
    for i in 0..=4u32 {
        let p = curve.evaluate_point(k, ivl.evaluate(f64::from(i) / 4.0))?;
        if distance_to_surface(k, surface_body, f, p)? > tol {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `kf_EdgeAlong`: whether the edge lies along one of `curves`: its ends and three points
/// between within `tol`.
fn edge_along(k: &Kernel, b: &Brep, e: EdgeId, curves: &[Curve], tol: f64) -> Result<bool> {
    let curve = edge::curve(b, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let ivl = edge::interval(b, e)?;
    for c in curves {
        let mut on = true;
        for i in 0..=4u32 {
            let p = curve.evaluate_point(k, ivl.evaluate(f64::from(i) / 4.0))?;
            let dropped = c.drop_point(
                k,
                &c.natural_interval(),
                p,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )?;
            if !(dropped.success && dropped.distance <= tol) {
                on = false;
                break;
            }
        }
        if on {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `kf_Creases`: the curves where the sheet face's surface meets the wall's: a planar wall
/// sections it exactly, any other wall meets it by the surface intersection over both faces'
/// domains (within `tol`), its touching points left out.
fn creases(
    k: &Kernel,
    sheet: &mut Brep,
    sheet_face: FaceId,
    wall_body: &Brep,
    wall: FaceId,
    tol: f64,
) -> Result<Vec<Curve>> {
    // Copies without an owner: the pieces are classified in both faces below, so the
    // intersection needs no trimmed cache of the faces.
    let mut sheet_surface = face::p2::surface(sheet, sheet_face)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone_for_evaluation()?;
    sheet_surface.set_owner(None);
    let mut wall_surface = face::p2::surface(wall_body, wall)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone_for_evaluation()?;
    wall_surface.set_owner(None);
    let sheet_domain = face::p2::uv_domain(sheet, sheet_face)?;
    if let Some(frame) = plane_of(k, &wall_surface)? {
        let section = face_planar_section_curves(
            k,
            sheet,
            sheet_face,
            &sheet_surface,
            &sheet_domain,
            frame.origin,
            frame.z,
            Some(tol),
            None,
        )?;
        return Ok(section.curves_3d);
    }
    let wall_domain = face::p2::uv_domain(wall_body, wall)?;
    let section = sheet_surface.global_surface_intersect(
        k,
        &sheet_domain,
        &wall_surface,
        &wall_domain,
        [true, true],
        Some(tol),
        None,
    )?;
    Ok(section
        .curves_3d
        .into_iter()
        .zip(section.curve_types)
        .filter(|(_, kind)| *kind != TsectCurveType::Touching)
        .map(|(curve, _)| curve)
        .collect())
}

/// `kf_PiecesInFace`: the pieces of `curve` that lie inside the face (its runs classified in
/// the face), as trimmed copies. Runs along the face's boundary, or outside it, are left out.
fn pieces_in_face(k: &Kernel, b: &Brep, f: FaceId, curve: &Curve, tol: f64) -> Result<Vec<Curve>> {
    let natural = curve.natural_interval();
    let curves = [curve.copy(k)?];
    let objects = IntersectObjects {
        b,
        other: b,
        side: false,
        curves_3d: &curves,
        uv1: &[],
        uv2: &[],
    };
    let mut class = CurveClassification::new(k);
    class.reset(
        k,
        &ResetOptions {
            curve: Some(IntersectObject::Curve(0)),
            interval: Some(natural),
            zone_tol_3d: tol,
            ..ResetOptions::default()
        },
    )?;
    curve_on_classify(k, b, f, &objects, true, &mut class, true, false, true)?;
    let closed = curve.is_closed(k, &natural, tol)?;
    let mut out = Vec::new();
    for (j, interval) in class.intervals().iter().enumerate() {
        if interval.mid.point_class() != PointClassType::Face {
            continue;
        }
        let mut run = class.sub_interval(j)?;
        let a = curve.evaluate_point(k, run.min())?;
        let c = curve.evaluate_point(k, run.max())?;
        if (a - c).length() <= tol && !closed {
            continue;
        }
        let mut piece = curve.copy(k)?;
        if run != piece.natural_interval() {
            piece.trim(k, &mut run, TrimMode::Notify)?;
        }
        out.push(piece);
    }
    Ok(out)
}

/// `kf_Upright`: whether every normal of the face at a 5 x 5 grid over its domain is square to
/// `up`: a wall made of lines along `up` (a prism's side, a cylinder about `up`).
fn upright(k: &Kernel, b: &Brep, f: FaceId, up: Vec3, ang_tol: f64) -> Result<bool> {
    let surface = face::p2::surface(b, f)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let domain = face::p2::uv_domain(b, f)?;
    let size = domain.size();
    let min = domain.min();
    for i in 0..=4u32 {
        for j in 0..=4u32 {
            let uv = crate::math::Point2::new(
                min.x + size.x * f64::from(i) / 4.0,
                min.y + size.y * f64::from(j) / 4.0,
            );
            let normal = surface.evaluate_normal(k, uv, Side::Right, Side::Right)?;
            if normal.dot(up).abs() > ang_tol {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// `kf_Feature`: a feature on a face a round rolls on: its footprint (an inner loop of the
/// face) and its walls.
struct Feature {
    /// Copies of the footprint's edges.
    rim_curves: Vec<Curve>,
    /// A boss (its rim concave), else a cut.
    boss: bool,
    /// The faces carried to the round: a cut's walls in the target, a boss's continued walls in
    /// the sheet.
    walls: Vec<FaceId>,
    /// Where its walls meet the round.
    creases: Vec<Curve>,
}

/// The edit-notification reference of a new edge's curve in `b`.
fn curve_ref_of(serial: Option<u64>) -> impl Fn(EdgeId, i64) -> ObjectRef {
    move |e: EdgeId, kind: i64| ObjectRef {
        key: match serial {
            Some(s) => crate::topology::brep::p3::topology_ref(s, EntityRef::Edge(e)).key,
            None => loopuse::notify_ref(EntityRef::Edge(e)).key,
        },
        type_code: kind,
    }
}

/// `MergeCurvesOnSurface` of `curves` on the face: the new faces and the curves' edges.
fn merge_on_face(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    tol: f64,
    curves: &[Curve],
    through: Option<Surface>,
) -> Result<(Vec<FaceId>, Vec<EdgeId>)> {
    let mut owned = curves.to_vec();
    with_kernel_callbacks(k, b, |k, b, env| {
        let curve_ref = curve_ref_of(b.notify_serial());
        crate::topology::brep::p10::merge_curves_on_surface_through_surface(
            k, b, env, &curve_ref, f, tol, &mut owned, None, through,
        )
    })
}

/// `DeleteFace(face, TRUE)`.
fn delete(k: &mut Kernel, b: &mut Brep, f: FaceId) -> Result<()> {
    with_kernel_callbacks(k, b, |k, b, env| {
        let mut marks = Marks::default();
        delete_face(
            k,
            b,
            env,
            &mut marks,
            f,
            DeleteFaceOptions {
                delete_edges_vertices: true,
                ..DeleteFaceOptions::default()
            },
        )
    })
    .map(drop)
}

/// The face's domain from its trim curves (`CalculateUVDomainFromUVTrimCurves`, `SetUVDomain`).
fn domain_from_trims(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<()> {
    let found = face::p1::calculate_uv_domain_from_uv_trim_curves(k, b, f, true, false, false)?;
    face::p2::set_uv_domain(b, f, &found.domain)
}

/// What the refusals say.
const NOT_UPRIGHT: &str = "Fillet Error: a boss inside a round's band, kept, whose walls are not upright to a planar face it stands on (its walls continued are not built yet)";
const NO_ROUND: &str = "Fillet Error: a kept boss's wall continued down meets no round";
const NOT_CLOSED: &str = "Fillet Error: a round kept round a feature does not close on the body (its rails and creases enclose no part of it)";
const NOT_SEWN: &str =
    "Fillet Error: a round kept round a feature does not sew into the body along its rails and creases";

impl FilletSession<'_> {
    /// The fork's `SmFilletExecutive::KeepFeaturesInBands` (`eb883ad`): trim the rounds in place
    /// round the features their bands reach, and sew them into the target.
    ///
    /// `Ok(false)`, and nothing is changed, when no band reaches a feature: the fillet is
    /// merged as before. Refused: a boss whose walls aren't upright to a planar face it stands
    /// on (`NotExact`), a sheet whose boundary doesn't close on the body (`BadMerge`). A
    /// refusal leaves the target as it was.
    ///
    /// # Errors
    /// The refusals as `SM_ERR`; those of the steps.
    pub(super) fn keep_features_in_bands(&mut self, k: &mut Kernel) -> Result<bool> {
        let saved = (*self.objects.target).clone();
        let result = self.keep_features_steps(k);
        if result.is_err() {
            *self.objects.target = saved;
        }
        result
    }

    #[allow(clippy::too_many_lines)] // one fork routine
    fn keep_features_steps(&mut self, k: &mut Kernel) -> Result<bool> {
        let sheet_tol = self
            .objects
            .intermediate
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?
            .tolerance();
        let solvers = self.executive.solvers.clone();

        let mut tol = sheet_tol;
        let mut ang_tol = EFF_ZERO_SQRT;
        // the faces the rounds roll on, and the blended edges: a loop holding one is the round's
        // own edge (a branch's saddle on a pipe), not a feature's footprint
        let mut supports: Vec<FaceId> = Vec::new();
        let mut blended_edges: Vec<EdgeId> = Vec::new();
        for &solver in &solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let base = self.objects.solver(id)?.base();
            tol = smos_max(tol, base.approx_tol_3d);
            ang_tol = smos_max(ang_tol, base.ang_tol_rad);
            let eu = base
                .edgeuse(0)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            blended_edges.push(
                eu1::edge(self.objects.target, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?,
            );
            for &g in &base.fillet_geoms {
                let geom = self.objects.pseudo.geom(g)?;
                if geom.fillet_geom_type() != FilletGeomType::Default {
                    continue;
                }
                for ii in 0..2 {
                    let Some(rail) = geom.rail(ii) else { continue };
                    let Some(f) = self.objects.pseudo.fillet_edge(rail)?.original_face() else {
                        continue;
                    };
                    if !supports.contains(&f) {
                        supports.push(f);
                    }
                }
            }
        }
        let on_tol = 10.0 * tol;
        // A crease off a planar wall is exact; any other is fitted, within a tenth of the
        // fillet's tolerance, as the fillet's own fitted sections are.
        let crease_tol = 0.1 * tol;

        let mut sheet = self
            .objects
            .intermediate
            .take()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let result = self.keep_features_on(
            k,
            &mut sheet,
            &supports,
            &blended_edges,
            tol,
            ang_tol,
            on_tol,
            crease_tol,
        );
        // the sheet stays the session's unless it was sewn in
        if !matches!(result, Ok(true)) {
            self.objects.intermediate = Some(sheet);
        }
        result
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one fork routine
    fn keep_features_on(
        &mut self,
        k: &mut Kernel,
        sheet: &mut Brep,
        supports: &[FaceId],
        blended_edges: &[EdgeId],
        tol: f64,
        ang_tol: f64,
        on_tol: f64,
        crease_tol: f64,
    ) -> Result<bool> {
        // the sheet's extent, and the depth below a face to which a boss's walls are continued:
        // past the whole sheet
        let sheet_box = crate::query::brep::calculate_bounding_box(
            k,
            &*sheet,
            crate::query::brep::BoundingBoxOptions::default(),
        )?;
        let (lo, hi) = (sheet_box.min(), sheet_box.max());
        let corners: Vec<Point3> = (0..8u32)
            .map(|c| {
                Point3::new(
                    if c & 1 != 0 { hi.x } else { lo.x },
                    if c & 2 != 0 { hi.y } else { lo.y },
                    if c & 4 != 0 { hi.z } else { lo.z },
                )
            })
            .collect();
        let sheet_size = (hi - lo).length();

        // the sheet's own faces (a boss's continued walls are built in it below)
        let sheet_faces = crate::topology::brep::p1::faces(sheet, None)?;

        // 1: the features, each footprint an inner loop of a support face, and their walls
        let mut features: Vec<Feature> = Vec::new();
        for &support in supports {
            for l in face::p2::loops(self.objects.target, support, false, None)? {
                if loopuse::is_outer_loop(self.objects.target, l)? {
                    continue;
                }
                let rim: Vec<EdgeId> = loopuse::loop_edges(self.objects.target, l)?
                    .into_iter()
                    .flatten()
                    .collect();
                if rim.is_empty() || rim.iter().any(|e| blended_edges.contains(e)) {
                    continue;
                }
                let mut concave = 0usize;
                let mut walls: Vec<FaceId> = Vec::new();
                let mut manifold = true;
                for &e in &rim {
                    if is_concave_edge(k, self.objects.target, e)? {
                        concave += 1;
                    }
                    let faces: Vec<FaceId> = edge::faces(self.objects.target, e, None)?;
                    let [a, b] = faces[..] else {
                        manifold = false;
                        break;
                    };
                    let wall = if a == support { b } else { a };
                    if !walls.contains(&wall) {
                        walls.push(wall);
                    }
                }
                // a footprint whose rim is all of one kind: a boss's, or a cut's
                if !manifold || (concave != 0 && concave != rim.len()) {
                    continue;
                }
                let boss = concave != 0;
                let mut rim_curves = Vec::with_capacity(rim.len());
                for &e in &rim {
                    rim_curves.push(edge_curve(k, self.objects.target, e)?);
                }
                if !boss {
                    features.push(Feature {
                        rim_curves,
                        boss,
                        walls,
                        creases: Vec::new(),
                    });
                    continue;
                }
                // A boss's walls continued down from its footprint: the footprint swept into
                // the body, along the planar face's normal, past the whole sheet.
                let support_surface = face::p2::surface(self.objects.target, support)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let frame = plane_of(k, support_surface)?;
                let mut is_upright = frame.is_some();
                let (at, normal) = frame.map_or((Point3::ZERO, Vec3::ZERO), |p| (p.origin, p.z));
                for &wall in &walls {
                    if is_upright {
                        is_upright = upright(k, self.objects.target, wall, normal, ang_tol)?;
                    }
                }
                let first_wall = *walls.first().ok_or_else(|| SolidError::null_pointer(OP))?;
                let in_wall =
                    face::p2::calculate_an_internal_point(k, self.objects.target, first_wall)?;
                let up = if (in_wall - at).dot(normal) >= 0.0 {
                    normal
                } else {
                    -normal
                };
                if !is_upright {
                    // judged below, only if the band reaches it
                    features.push(Feature {
                        rim_curves,
                        boss,
                        walls: Vec::new(),
                        creases: Vec::new(),
                    });
                    continue;
                }
                let mut depth = 0.0;
                for &corner in &corners {
                    depth = smos_max(depth, (at - corner).dot(up));
                }
                depth += sheet_size;
                // Each rim curve swept in two halves: every continued wall a patch whose crease
                // with the round runs from one of its sides to the other (a closed rim swept
                // whole is a tube, which its crease would go round).
                let mut profile = Vec::with_capacity(2 * rim_curves.len());
                for rim_curve in &rim_curves {
                    let ivl = rim_curve.natural_interval();
                    let mid = ivl.evaluate(0.5);
                    for h in 0..2 {
                        let mut half = rim_curve.copy(k)?;
                        let mut part = if h == 0 {
                            crate::geom::extent1d::Extent1d::from_min_max(ivl.min(), mid)
                        } else {
                            crate::geom::extent1d::Extent1d::from_min_max(mid, ivl.max())
                        };
                        half.trim(k, &mut part, TrimMode::Notify)?;
                        profile.push(half);
                    }
                }
                let before: BTreeSet<FaceId> = crate::topology::brep::p1::faces(sheet, None)?
                    .into_iter()
                    .collect();
                // the sweep leaves the B-rep it builds in closed to editing: the sheet is
                // edited on below
                let editing = sheet.editing_enabled();
                let mut sweep = PrimitiveCreation::new(sheet.infinite_region(), 0, true);
                let swept = sweep.create_linear_sweep(k, sheet, profile, -up, depth, false);
                sheet.set_editing_enabled(editing);
                swept?;
                let walls = crate::topology::brep::p1::faces(sheet, None)?
                    .into_iter()
                    .filter(|f| !before.contains(f))
                    .collect();
                features.push(Feature {
                    rim_curves,
                    boss,
                    walls,
                    creases: Vec::new(),
                });
            }
        }
        if features.is_empty() {
            return Ok(false);
        }

        // 2: the creases, each where a feature's wall meets a sheet face, kept on both
        let mut sheet_boxes: Vec<Extent3d> = Vec::with_capacity(sheet_faces.len());
        for &f in &sheet_faces {
            sheet_boxes.push(crate::query::api_queries::face_bounding_box(
                k, sheet, f, false,
            )?);
        }
        let mut sheet_creases: BTreeMap<FaceId, Vec<Curve>> = BTreeMap::new();
        let mut wall_creases: BTreeMap<FaceId, Vec<Curve>> = BTreeMap::new();
        // per continued wall of a boss, the round's surface its creases lie on (when one)
        let mut wall_round: BTreeMap<FaceId, Option<FaceId>> = BTreeMap::new();
        for feature in &mut features {
            if feature.boss && feature.walls.is_empty() {
                // a boss whose walls aren't upright: reached when its footprint's rim crosses a
                // sheet face's box
                let mut rim_box = Extent3d::default();
                for rim_curve in &feature.rim_curves {
                    let ivl = rim_curve.natural_interval();
                    for j in 0..=8u32 {
                        rim_box.add_point(
                            rim_curve.evaluate_point(k, ivl.evaluate(f64::from(j) / 8.0))?,
                        );
                    }
                }
                if sheet_boxes
                    .iter()
                    .any(|b| !b.are_disjoint(&rim_box, on_tol))
                {
                    self.note_keep_refusal(FilletErrorType::NotExact, NOT_UPRIGHT);
                    return Err(SolidError::sm_err(OP));
                }
                continue;
            }
            for wall in feature.walls.clone() {
                let wall_body: Brep = if feature.boss {
                    sheet.clone()
                } else {
                    (*self.objects.target).clone()
                };
                let wall_box =
                    crate::query::api_queries::face_bounding_box(k, &wall_body, wall, false)?;
                for (sheet_box, &sheet_face) in sheet_boxes.iter().zip(&sheet_faces) {
                    if sheet_box.are_disjoint(&wall_box, on_tol) {
                        continue;
                    }
                    let found = creases(k, sheet, sheet_face, &wall_body, wall, crease_tol)?;
                    let mut on_wall = Vec::new();
                    for curve in &found {
                        on_wall.extend(pieces_in_face(k, &wall_body, wall, curve, tol)?);
                    }
                    for curve in &on_wall {
                        for piece in pieces_in_face(k, sheet, sheet_face, curve, tol)? {
                            sheet_creases
                                .entry(sheet_face)
                                .or_default()
                                .push(piece.clone());
                            if feature.boss {
                                wall_creases.entry(wall).or_default().push(piece.clone());
                                match wall_round.get(&wall) {
                                    None => {
                                        wall_round.insert(wall, Some(sheet_face));
                                    }
                                    Some(&Some(other)) if other != sheet_face => {
                                        wall_round.insert(wall, None);
                                    }
                                    _ => {}
                                }
                            }
                            feature.creases.push(piece);
                        }
                    }
                }
            }
        }
        // the continued walls of the bosses no band reaches leave the sheet
        for feature in &mut features {
            if !feature.boss || !feature.creases.is_empty() {
                continue;
            }
            for wall in std::mem::take(&mut feature.walls) {
                delete(k, sheet, wall)?;
            }
        }
        let kept: Vec<Feature> = features
            .into_iter()
            .filter(|feature| !feature.creases.is_empty())
            .collect();
        if kept.is_empty() {
            return Ok(false);
        }

        // 3: each sheet face split along its creases; the pieces over a footprint, bounded by
        // its feature's creases alone, go
        for &sheet_face in &sheet_faces {
            let Some(curves) = sheet_creases.get(&sheet_face) else {
                continue;
            };
            let (new_faces, _) = merge_on_face(k, sheet, sheet_face, tol, curves, None)?;
            let mut all = vec![sheet_face];
            all.extend(new_faces);
            for piece in all {
                domain_from_trims(k, sheet, piece)?;
                let edges = face::p4::edges(sheet, piece, None, false, None)?;
                let mut over = false;
                for feature in &kept {
                    let mut every = !edges.is_empty();
                    for &e in &edges {
                        if !every {
                            break;
                        }
                        every = edge_along(k, sheet, e, &feature.creases, on_tol)?;
                    }
                    if every {
                        over = true;
                        break;
                    }
                }
                if over {
                    delete(k, sheet, piece)?;
                }
            }
        }
        // a boss's continued walls split along their creases, each keeping its piece up to the
        // footprint
        for feature in &kept {
            if !feature.boss {
                continue;
            }
            for wall in feature.walls.clone() {
                let Some(curves) = wall_creases.get(&wall) else {
                    self.note_keep_refusal(FilletErrorType::BadMerge, NO_ROUND);
                    return Err(SolidError::sm_err(OP));
                };
                let through = match wall_round.get(&wall) {
                    Some(&Some(round)) if sheet.face(round).is_ok() => {
                        face::p2::surface(sheet, round)?
                            .map(Surface::clone_for_evaluation)
                            .transpose()?
                    }
                    _ => None,
                };
                let (new_faces, _) = merge_on_face(k, sheet, wall, tol, curves, through)?;
                let mut all = vec![wall];
                all.extend(new_faces);
                for piece in all {
                    domain_from_trims(k, sheet, piece)?;
                    let mut up_to_footprint = false;
                    for e in face::p4::edges(sheet, piece, None, false, None)? {
                        if edge_along(k, sheet, e, &feature.rim_curves, on_tol)? {
                            up_to_footprint = true;
                            break;
                        }
                    }
                    if !up_to_footprint {
                        delete(k, sheet, piece)?;
                    }
                }
            }
        }

        // 4: the kept sheet's boundary imprinted on the target faces it lies on (a boss's creases
        // and its continued walls' tops at its footprint are left out: they meet no target face
        // to cut)
        let mut boss_creases: Vec<Curve> = Vec::new();
        for feature in &kept {
            if feature.boss {
                boss_creases.extend(feature.creases.iter().cloned());
                boss_creases.extend(feature.rim_curves.iter().cloned());
            }
        }
        let sheet_edges = crate::topology::brep::p1::edges(sheet, None)?;
        let mut stops: BTreeSet<EdgeId> = BTreeSet::new();
        for f in crate::topology::brep::p1::faces(self.objects.target, None)? {
            let mut on: Vec<Curve> = Vec::new();
            for &e in &sheet_edges {
                if !eu1::edge_is_lamina(sheet, e)? {
                    continue;
                }
                if edge_along(k, sheet, e, &boss_creases, on_tol)? {
                    continue;
                }
                if !edge_on_surface(k, sheet, e, self.objects.target, f, on_tol)? {
                    continue;
                }
                on.push(edge_curve(k, sheet, e)?);
            }
            if on.is_empty() {
                continue;
            }
            let (_, on_edges) = merge_on_face(k, self.objects.target, f, tol, &on, None)?;
            stops.extend(on_edges);
        }
        // a kept boss stands on its footprint: the faces the blends take stop there
        for e in crate::topology::brep::p1::edges(self.objects.target, None)? {
            for feature in &kept {
                if feature.boss
                    && edge_along(k, self.objects.target, e, &feature.rim_curves, on_tol)?
                {
                    stops.insert(e);
                    break;
                }
            }
        }

        // the faces round the blended edges, up to the stops: the material the blends take off
        let mut blended_curves: Vec<Curve> = Vec::with_capacity(blended_edges.len());
        for &e in blended_edges {
            blended_curves.push(edge_curve(k, self.objects.target, e)?);
        }
        let mut gone: BTreeSet<FaceId> = BTreeSet::new();
        let mut front: Vec<FaceId> = Vec::new();
        for e in crate::topology::brep::p1::edges(self.objects.target, None)? {
            if stops.contains(&e) || !edge_along(k, self.objects.target, e, &blended_curves, tol)? {
                continue;
            }
            for f in edge::faces(self.objects.target, e, None)? {
                if gone.insert(f) {
                    front.push(f);
                }
            }
        }
        while let Some(f) = front.pop() {
            for e in face::p4::edges(self.objects.target, f, None, false, None)? {
                if stops.contains(&e) {
                    continue;
                }
                for other in edge::faces(self.objects.target, e, None)? {
                    if gone.insert(other) {
                        front.push(other);
                    }
                }
            }
        }
        let target_faces = crate::topology::brep::p1::faces(self.objects.target, None)?.len();
        if gone.is_empty() || gone.len() >= target_faces {
            self.note_keep_refusal(FilletErrorType::BadMerge, NOT_CLOSED);
            return Err(SolidError::sm_err(OP));
        }
        for f in gone {
            delete(k, self.objects.target, f)?;
        }

        // the fillet edges' attribute stays with the executive's own B-reps
        if let (Some(serial), Some(FilletObject::Attribute(handle))) =
            (sheet.notify_serial(), self.executive.edge_attribute)
        {
            for &e in &sheet_edges {
                if sheet.edge(e).is_err() {
                    continue;
                }
                let key = crate::topology::brep::p3::topology_ref(serial, EntityRef::Edge(e)).key;
                if self
                    .objects
                    .attributes
                    .find_attribute(key, ids::ALL_FILLET_EDGES)
                    == Some(handle)
                {
                    self.objects
                        .attributes
                        .remove_attribute(key, handle, false)?;
                }
            }
        }

        // the sheet and the bosses' continued walls sewn in, and the body oriented
        let kept_faces = crate::topology::brep::p1::faces(sheet, None)?;
        crate::blend::feature_executive::copy_faces_carrying(
            k,
            sheet,
            &kept_faces,
            self.objects.target,
            false,
        )?;
        let target_tol = tol;
        let stitched = crate::topology::brep::p4::stitch_faces(
            k,
            self.objects.target,
            target_tol,
            None,
            false,
        )?;
        let mut maybe_open = false;
        if stitched.lamina_edges == 0 {
            let oriented = with_kernel_callbacks(k, self.objects.target, |k, b, env| {
                let mut marks = Marks::default();
                orient_trimmed_surfaces(k, b, env, &mut marks, true, false, None)
            })?;
            maybe_open = oriented.maybe_not_closed_solid;
        }
        if stitched.lamina_edges != 0
            || maybe_open
            || !crate::topology::brep::p2::is_manifold_solid(self.objects.target)?
        {
            self.note_keep_refusal(FilletErrorType::BadMerge, NOT_SEWN);
            return Err(SolidError::sm_err(OP));
        }
        // each boss's wall and its continuation one face again: only the edges along its
        // footprint go
        let mut keep = Vec::new();
        let mut any_rim = false;
        for e in crate::topology::brep::p1::edges(self.objects.target, None)? {
            let mut rim = false;
            for feature in &kept {
                if feature.boss
                    && !rim
                    && edge_along(k, self.objects.target, e, &feature.rim_curves, on_tol)?
                {
                    rim = true;
                }
            }
            if rim {
                any_rim = true;
            } else {
                keep.push(e);
            }
        }
        if any_rim {
            crate::topology::brep::p5::remove_topological_edges_and_vertices(
                k,
                self.objects.target,
                Some(&keep),
                None,
            )?;
        }
        Ok(true)
    }

    fn note_keep_refusal(&mut self, code: FilletErrorType, comment: &'static str) {
        self.note_refusal(Refusal {
            code,
            reason: None,
            comment,
        });
    }
}
