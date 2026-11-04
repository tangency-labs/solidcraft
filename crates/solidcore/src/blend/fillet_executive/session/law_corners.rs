//! A round turning at its radius law's corners, split there on the live session (not
//! upstream): the fork's `SmFilletExecutive::NoteRadiusCornerSurfaces`,
//! `SplitFilletFacesAtRadiusCorners` and `sm_ReplaceIsoCrvOnSurf`
//! (`source/SMLib/src/SmFilletExecutive.cpp`, tangency-labs/smlib@6eecf04, with 58d03ef's
//! copy of the pieces; WR-685's law-corner addendum, #2273).
//!
//! The circular section skins a round whose radius law has a corner piece by piece and
//! joins the pieces C0 there ([`crate::blend::fillet_standard_solver::p1::CircularCrossSectionFsg::piecewise_surface`]);
//! SMLib's check refuses a face (and the edges round it) with such a kink inside. The
//! merge gives the target copies of the fillet faces and drops the fillet B-rep, so the
//! surfaces are noted before it (with the pieces kept for them, where a corner dips or
//! peaks) and found again after it by their equality: each face on one is split at its
//! surface's C0 knots, its surface cut back to the side it lies on, and the new edges
//! along the turn given the iso curve of the surface they lie on, exactly. Where a corner
//! dips or peaks, the faces are then rebuilt as the union of the balls (`BuildLawCorners`,
//! 7babe17 with 62fcc82 and 58d03ef): each side its exact piece, the corner ball's zone a
//! face of its own at a dip, the sides ending on their crossing at a peak.
//!
//! | Rust | Fork |
//! |------|------|
//! | `FilletSession::note_radius_corner_surfaces` (crate) | `SmFilletExecutive::NoteRadiusCornerSurfaces` |
//! | `FilletSession::split_fillet_faces_at_radius_corners` (crate) | `SmFilletExecutive::SplitFilletFacesAtRadiusCorners` |
//! | `FilletSession::build_law_corners` (private) | `SmFilletExecutive::BuildLawCorners` |
//! | `replace_iso_crv_on_surf` (private) | `sm_ReplaceIsoCrvOnSurf` |
//! | `faces_of_pieces` (private) | `sm_PieceOfFace`, in `BuildLawCorners`' two loops |
//! | `edge_between` (private) | `sm_EdgeBetween` |
//!
//! Deviations: the noted copies and their pieces are one list of pairs
//! (`m_vRadiusCornerSurfaces`, `m_vRadiusCornerPieces`), owned by the session and emptied
//! by the split. The split's loop passes over a face the rebuild of an earlier one removed
//! (the fork's list would hold a deleted face there). With `Kernel::blend_face_log` on, a
//! dip's zone is marked as a corner ball's patch (#2489; not the fork's). No recursion.

use super::trim_at_creases::with_target_env_of;
use super::{FilletSession, SolverObject};
use crate::blend::definition_check::{BlendFaceKind, CornerKind};
use crate::blend::law_corners::{
    curve_between, find_law_corner_pieces, sphere_zone, LawCornerKind, LawCornerPieces,
};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::{Point3, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::Surface;
use crate::topology::brep::p1::faces as brep_faces;
use crate::topology::brep::p2::OldSurface;
use crate::topology::brep::p3::{self, orient_trimmed_surfaces};
use crate::topology::brep::p4::stitch_faces;
use crate::topology::brep::p7::{make_face_with_curves, unstitch, FaceCurvesInput};
use crate::topology::edge::{self, NotificationStores};
use crate::topology::face::p1::{calculate_uv_domain_from_uv_trim_curves, shrink_geometry};
use crate::topology::topology_traverser::Marks;
use crate::topology::vertex;
use crate::topology::{Brep, EdgeId, FaceId};

const OP: &str = "blend::fillet_executive::session::law_corners";

/// A fillet surface noted before the merge (a copy), with its kept pieces where a law
/// corner dips or peaks.
pub(super) type RadiusCornerSurface = (Surface, Option<LawCornerPieces>);

impl FilletSession<'_> {
    /// `NoteRadiusCornerSurfaces` (the fork's 6eecf04, 58d03ef): copies of the fillet
    /// surfaces of the solvers whose radius has a corner, where those surfaces turn (a C0
    /// knot), each with a copy of the pieces kept for it (matched by its corners,
    /// [`find_law_corner_pieces`]) where a corner dips or peaks.
    ///
    /// # Errors
    ///
    /// Those of the solvers' corners, the geometries' surfaces, the knot search and the
    /// pieces' match.
    pub(super) fn note_radius_corner_surfaces(&mut self, k: &Kernel) -> Result<()> {
        let mut noted = Vec::new();
        for index in 0..self.objects.solvers.len() {
            // `GetRadiusCornerParameters` (virtual): the variable radius solver's
            let geoms = match self.objects.solvers.get(index).and_then(Option::as_ref) {
                Some(SolverObject::VariableRadius(vr)) => {
                    if vr.radius_corner_parameters(k)?.is_empty() {
                        continue;
                    }
                    vr.curve_based.solver.fillet_geoms()
                }
                _ => continue,
            };
            for geom in geoms {
                // (the surface is the intermediate body's face's by now)
                let Some(surface) = self.objects.fillet_surface(geom)? else {
                    continue;
                };
                let (u, v) = surface.find_knots_by_max_continuity(k, ContinuityType::C0, false)?;
                if u.is_empty() && v.is_empty() {
                    continue;
                }
                let copy = surface.clone();
                // the exact pieces where a law corner dips or peaks: those kept with the
                // same corners
                let base = self
                    .objects
                    .solvers
                    .get_mut(index)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| SolidError::null_pointer(OP))?
                    .base_mut();
                let pieces = find_law_corner_pieces(k, &mut base.law_corner_pieces, &copy)?
                    .and_then(|i| base.law_corner_pieces.get(i))
                    .filter(|kept| kept.has_dip_or_peak())
                    .cloned();
                noted.push((copy, pieces));
            }
        }
        self.objects.radius_corner_surfaces.extend(noted);
        Ok(())
    }

    /// `SplitFilletFacesAtRadiusCorners` (the fork's 6eecf04): each face of the target on
    /// a surface noted by [`Self::note_radius_corner_surfaces`] split at its surface's C0
    /// knots (`SmBrep::SubdivideFaceAtDiscontinuities`), each resulting face's surface cut
    /// at the turns to the side the face lies on and the face shrunk back to it, and the
    /// split's edges given the iso curves they lie along. Empties the noted list.
    ///
    /// # Errors
    ///
    /// Those of the surface equality, the split, the knot search, the trims, the shrink and
    /// the curve replacement.
    pub(super) fn split_fillet_faces_at_radius_corners(&mut self, k: &mut Kernel) -> Result<()> {
        crate::profile::profile_scope!(k, "blend::session::split_at_radius_corners");
        let noted = std::mem::take(&mut self.objects.radius_corner_surfaces);
        if noted.is_empty() {
            return Ok(());
        }
        // the keys a caller-named target recorded before the split name its entities
        // afterwards; the split's new edge curves are named by the session's identity, as
        // the merge's are, and rebound to the target's naming after it (as `merge_fillet`
        // does, tangency-labs/solidcore#423), also after an error
        let recorded = self.objects.target.geometry_owner_keys().clone();
        let result = self.split_noted_faces(k, &noted);
        let naming = self.objects.target.naming();
        let serial = self.objects.target_serial;
        let session_owner = |e| p3::topology_ref(serial, e);
        let restored =
            super::piecewise::normalize_session_owners(self.objects.target, session_owner, |e| {
                super::piecewise::naming_owner(naming, recorded.get(&e).copied(), e)
                    .unwrap_or_else(|| session_owner(e))
            });
        result.and(restored)
    }

    /// The body of [`Self::split_fillet_faces_at_radius_corners`] on the noted surfaces.
    fn split_noted_faces(&mut self, k: &mut Kernel, noted: &[RadiusCornerSurface]) -> Result<()> {
        let serial = self.objects.target_serial;
        let key_of = move |entity| p3::topology_ref(serial, entity);
        for face in brep_faces(self.objects.target, None)? {
            // (a face of a round rebuilt at its law's corners below is gone)
            if !self.objects.target.face_list().contains(&face) {
                continue;
            }
            // the merge gives the target an exact copy of each fillet surface
            let Some(surface) = crate::topology::face::p2::surface(self.objects.target, face)?
            else {
                continue;
            };
            let mut fillet = None;
            for (index, (copy, _)) in noted.iter().enumerate() {
                if copy.approx_eq(k, surface)? {
                    fillet = Some(index);
                    break;
                }
            }
            let Some(index) = fillet else {
                continue;
            };
            let mut pieces = with_target_env_of(&mut self.objects, k, |k, b, env, _| {
                let key_of = env.key_of;
                let curve_ref = |e: EdgeId, kind: i64| ObjectRef {
                    key: key_of(EntityRef::Edge(e)).key,
                    type_code: kind,
                };
                crate::topology::brep::p10::subdivide_face_at_discontinuities(
                    k,
                    b,
                    env,
                    &curve_ref,
                    face,
                    ContinuityType::C0,
                )
            })?;
            if !pieces.contains(&face) {
                pieces.push(face);
            }
            // Each face's surface is cut at the turns to the side the face lies on, so the
            // turn is its boundary, then back to the face (whose trims end within tolerance
            // of the turn).
            let target = &mut *self.objects.target;
            for &piece in &pieces {
                let bx =
                    calculate_uv_domain_from_uv_trim_curves(k, target, piece, true, false, false)?
                        .domain;
                let mid = bx.mid();
                let surface = crate::topology::face::p2::surface(target, piece)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                let domain = surface.natural_uv_domain_with_nurb(k)?;
                let (mut lo, mut hi) = (domain.min(), domain.max());
                let (u, v) = surface.find_knots_by_max_continuity(k, ContinuityType::C0, false)?;
                for knot in u {
                    if knot <= lo.x || knot >= hi.x {
                        continue;
                    }
                    if knot < mid.x {
                        lo.x = knot;
                    } else {
                        hi.x = knot;
                    }
                }
                for knot in v {
                    if knot <= lo.y || knot >= hi.y {
                        continue;
                    }
                    if knot < mid.y {
                        lo.y = knot;
                    } else {
                        hi.y = knot;
                    }
                }
                let mut side = Extent2d::from_bounds(lo.x, lo.y, hi.x, hi.y);
                if side != domain {
                    target
                        .face_mut(piece)?
                        .surface
                        .as_mut()
                        .ok_or_else(|| SolidError::null_pointer(OP))?
                        .trim_with_domain(k, &mut side)?;
                }
                shrink_geometry(k, target, piece, &key_of)?;
            }

            // The split's edges lie along iso lines of the surface as it was, as curves on
            // that surface (with its turn inside); each becomes that iso curve itself.
            let mut edges = Vec::new();
            for &piece in &pieces {
                for e in crate::topology::face::p4::edges(target, piece, None, false, None)? {
                    if !edges.contains(&e) {
                        edges.push(e);
                    }
                }
            }
            let super::SessionObjects {
                target,
                notification_records,
                attributes,
                ..
            } = &mut self.objects;
            let mut stores = NotificationStores {
                records: notification_records,
                attributes,
                key_of: &key_of,
            };
            for e in edges {
                replace_iso_crv_on_surf(k, target, &mut stores, e)?;
            }

            // where the law dips or peaks, the exact round there (the faces are rebuilt)
            if let Some(kept) = noted.get(index).and_then(|(_, kept)| kept.as_ref()) {
                self.build_law_corners(k, &pieces, kept)?;
            }
        }
        Ok(())
    }

    /// `BuildLawCorners` (the fork's 7babe17, with 62fcc82's new faces after the unstitch
    /// and 58d03ef's corner edge tolerances): the exact round at the corners of a radius
    /// law where it dips or peaks, on `faces`, the faces of one round split there, with
    /// `kept` its exact pieces and corners.
    ///
    /// The round is the union of its balls. Each side of a corner is its own piece; the
    /// joined surface the faces were made on took one boundary row at each corner, while
    /// the sides end on the corner ball's touch circles of their own, through the same two
    /// rail points. Where the law dips the ball's sphere fills the zone between the two
    /// circles, a face of its own whose edges are the circles; where it peaks the sides run
    /// past each other and each ends on their crossing curve. The faces are unstitched,
    /// given their pieces and their corner edges, the zones added, and the lot stitched and
    /// oriented again. G1 at a dip, G0 at a peak: the shape the law's corner gives.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for pieces, corners and joined ends that do not match, for a face that is
    /// no piece's or a piece with two faces, and for lamina edges left by the stitch;
    /// [`SolidError::null_pointer`] (the fork's `NER`) for a piece without a face, a corner
    /// whose faces share no edge or have no edge between its rail points, or a peak whose
    /// pieces have no crossing between them; those of the steps. After the unstitch, an
    /// error leaves the round's faces unstitched, as the fork's does.
    fn build_law_corners(
        &mut self,
        k: &mut Kernel,
        faces: &[FaceId],
        kept: &LawCornerPieces,
    ) -> Result<()> {
        let count = kept.pieces.len();
        if count < 2 || kept.corners.len() + 1 != count || kept.joined_ends.len() != count {
            return Err(SolidError::sm_err(OP));
        }
        if !kept.has_dip_or_peak() {
            return Ok(());
        }
        let null = || SolidError::null_pointer(OP);
        let serial = self.objects.target_serial;
        let key_of = move |entity| p3::topology_ref(serial, entity);

        // each piece's face
        let target = &mut *self.objects.target;
        let of = faces_of_pieces(k, target, faces.iter().copied(), kept)?;

        // each corner's rail points: the ends of the edge its two faces share
        let mut tol = kept.tol_3d;
        let mut rails = Vec::new();
        for (i, corner) in kept.corners.iter().enumerate() {
            let mut ends = (Point3::ZERO, Point3::ZERO);
            if corner.kind != LawCornerKind::Matching {
                let (Some(&before), Some(&after)) = (of.get(i), of.get(i + 1)) else {
                    return Err(null());
                };
                let mut shared = None;
                for e in face_edges(target, before)? {
                    if edge::faces(target, e, None)?.contains(&after) {
                        shared = Some(e);
                        break;
                    }
                }
                let shared = shared.ok_or_else(null)?;
                ends = edge_ends(target, shared)?;
                tol = tol.max(edge::tolerance(target, shared)?);
            }
            rails.push(ends);
        }

        // the faces around the round, which it is stitched back to
        let mut stitch: Vec<FaceId> = Vec::new();
        for &f in &of {
            for e in face_edges(target, f)? {
                for g in edge::faces(target, e, None)? {
                    if !faces.contains(&g) && !stitch.contains(&g) {
                        stitch.push(g);
                    }
                }
            }
        }

        // unstitch the round's faces: they are deleted and come back as new faces, which may
        // take the old ones' ids, so the new ones are those not among the faces that stayed
        let stayed: Vec<FaceId> = brep_faces(target, None)?
            .into_iter()
            .filter(|f| !of.contains(f))
            .collect();
        // (not the fork's, #2489: with the blend face log on, each new face keeps its old
        // face's mark, which the session's store holds by the old face)
        let kinds = of
            .iter()
            .map(|&f| self.target_blend_face_kind(k, f))
            .collect::<Result<Vec<_>>>()?;
        let target = &mut *self.objects.target;
        unstitch(k, target, Some(&of))?;
        let after: Vec<FaceId> = brep_faces(target, None)?
            .into_iter()
            .filter(|f| !stayed.contains(f))
            .collect();
        let of = faces_of_pieces(k, target, after.into_iter(), kept)?;
        for (&f, kind) in of.iter().zip(kinds) {
            if let Some(kind) = kind {
                self.mark_target_blend_face(k, f, kind)?;
            }
        }
        stitch.extend_from_slice(&of);

        // each corner's edges, on the face before and the face after
        let mut zones = Vec::new();
        for (i, (corner, &(start, end))) in kept.corners.iter().zip(&rails).enumerate() {
            if corner.kind == LawCornerKind::Matching {
                continue;
            }
            let target = &mut *self.objects.target;
            let (Some(&before), Some(&after)) = (of.get(i), of.get(i + 1)) else {
                return Err(null());
            };
            let edges = [
                edge_between(target, before, start, end, tol)?.ok_or_else(null)?,
                edge_between(target, after, start, end, tol)?.ok_or_else(null)?,
            ];

            // the curve of each side's corner edge
            let curves: [Curve; 2] = if corner.kind == LawCornerKind::Dip {
                // each side's touch circle: its piece's end section at the corner
                let section = |j: usize| -> Result<Curve> {
                    let piece = kept.pieces.get(i + j).ok_or_else(null)?;
                    let domain = piece.natural_uv_domain_with_nurb(k)?;
                    let u = if j == 0 {
                        domain.max().x
                    } else {
                        domain.min().x
                    };
                    Ok(piece
                        .create_iso_parametric_curve(k, SurfParamType::U, u, 0.0, None, false)?
                        .curve)
                };
                [section(0)?, section(1)?]
            } else {
                // where the sides cross: the one curve from rail point to rail point
                let (Some(first), Some(second)) = (kept.pieces.get(i), kept.pieces.get(i + 1))
                else {
                    return Err(null());
                };
                let found = first.global_surface_intersect(
                    k,
                    &first.natural_uv_domain_with_nurb(k)?,
                    second,
                    &second.natural_uv_domain_with_nurb(k)?,
                    [true, true],
                    Some(kept.tol_3d / 4.0),
                    Some(20.0_f64.to_radians()),
                )?;
                let mut crossing = None;
                for curve in &found.curves_3d {
                    if crossing.is_some() {
                        break;
                    }
                    crossing = curve_between(k, curve, start, end, tol)?;
                }
                let crossing = crossing.ok_or_else(null)?;
                let copy = crossing.copy(k)?;
                [crossing, copy]
            };

            // each runs from its edge's start vertex to its end vertex
            {
                let super::SessionObjects {
                    target,
                    notification_records,
                    attributes,
                    ..
                } = &mut self.objects;
                let mut stores = NotificationStores {
                    records: notification_records,
                    attributes,
                    key_of: &key_of,
                };
                for (&e, curve) in edges.iter().zip(&curves) {
                    let (from, to) = edge_ends(target, e)?;
                    let oriented = curve_between(k, curve, from, to, tol)?.ok_or_else(null)?;
                    crate::topology::brep::p2::replace_curve(k, target, e, oriented, &mut stores)?;
                }
            }
            if corner.kind != LawCornerKind::Dip {
                continue;
            }

            // the zone of the corner ball's sphere between the two circles, a face of its
            // own: the sphere through both circles, which share their ends (the rails'
            // points at the corner)
            let [first, second] = &curves;
            let middle = |c: &Curve| -> Result<Point3> {
                let ivl = c.natural_interval();
                c.evaluate_point(k, 0.5 * (ivl.min() + ivl.max()))
            };
            let ivl = first.natural_interval();
            let two = first.evaluate_point(k, ivl.min())?;
            let one = first.evaluate_point(k, ivl.max())?;
            let zone = Surface::from(sphere_zone(
                corner.centre,
                corner.radius,
                one,
                two,
                middle(first)?,
                middle(second)?,
            )?);
            // outer loop, counterclockwise in (u, v): up the second circle (u = 1), down the
            // first
            let mut curves_3d = Vec::new();
            let mut uv_curves = Vec::new();
            let mut orientations = Vec::new();
            for (j, u) in [1.0, 0.0].into_iter().enumerate() {
                let iso = zone
                    .create_iso_parametric_curve(k, SurfParamType::U, u, 0.0, None, false)?
                    .curve;
                let ivl = iso.natural_interval();
                let line = BSplineCurve::create_line_segment(
                    2,
                    Point3::new(u, ivl.min(), 0.0),
                    Point3::new(u, ivl.max(), 0.0),
                )?;
                curves_3d.push(Some(iso));
                uv_curves.push(Curve::from(line));
                orientations.push(if j == 0 {
                    OrientType::Same
                } else {
                    OrientType::Opposite
                });
            }
            let input = FaceCurvesInput {
                loops: vec![2],
                curves_3d: Some(curves_3d),
                uv_curves: Some(uv_curves),
                orientations,
                loop_points: Vec::new(),
            };
            let domain = zone.natural_uv_domain_with_nurb(k)?;
            let made = with_target_env_of(&mut self.objects, k, |k, b, env, _| {
                let key_of = env.key_of;
                let face_ref = |f| key_of(EntityRef::Face(f));
                make_face_with_curves(
                    k,
                    b,
                    None,
                    input,
                    zone,
                    &domain,
                    OrientType::Same,
                    None,
                    &face_ref,
                    env,
                )
            })?;
            let zone_face = made.face.ok_or_else(null)?;
            // (not the fork's, #2489) with the blend face log on, the zone is marked as a
            // corner ball's patch, as the session marks the faces it made before the merge
            self.mark_target_blend_face(k, zone_face, BlendFaceKind::Corner(CornerKind::Ball))?;
            zones.push(zone_face);
            stitch.push(zone_face);
        }

        // each side its exact piece, cut down to the face (as the split cut the joined
        // surface: a piece runs on past the edge's ends, where its law carries on)
        for (piece, &f) in kept.pieces.iter().zip(&of) {
            with_target_env_of(&mut self.objects, k, |k, b, env, _| {
                let face_ref = (env.key_of)(EntityRef::Face(f));
                crate::topology::brep::p2::replace_surface(
                    k,
                    b,
                    env,
                    f,
                    piece.clone(),
                    face_ref,
                    OldSurface::Delete,
                    true,
                )
            })?;
            shrink_geometry(k, self.objects.target, f, &key_of)?;
        }

        // stitched back into the solid, and oriented
        let list: Vec<Option<FaceId>> = stitch.iter().copied().map(Some).collect();
        let stitched = stitch_faces(k, self.objects.target, tol, Some(&list), false)?;
        if stitched.lamina_edges != 0 {
            return Err(SolidError::sm_err(OP));
        }
        with_target_env_of(&mut self.objects, k, |k, b, env, _| {
            orient_trimmed_surfaces(k, b, env, &mut Marks::default(), true, false, None)
        })?;

        // the corners' edges, and their ends, are as near their faces as the pieces'
        // sections are to the balls' circles (the zone itself is exact, a crossing is traced
        // within a quarter of that)
        let target = &mut *self.objects.target;
        for (i, (corner, &(start, end))) in kept.corners.iter().zip(&rails).enumerate() {
            if corner.kind != LawCornerKind::Peak {
                continue;
            }
            let before = *of.get(i).ok_or_else(null)?;
            let crossing = edge_between(target, before, start, end, tol)?.ok_or_else(null)?;
            edge::set_tolerance(target, crossing, tol, true, true)?;
        }
        for &zone in &zones {
            for e in face_edges(target, zone)? {
                edge::set_tolerance(target, e, tol, true, true)?;
            }
        }
        Ok(())
    }
}

/// `sm_PieceOfFace` (the fork's 7babe17) for each of `faces`, as `BuildLawCorners` maps
/// them: the face of each of `kept`'s pieces, the piece whose joined end is first past the
/// middle of the face's uv box in u.
///
/// # Errors
///
/// `SM_ERR` for a face past every piece or a piece with two faces;
/// [`SolidError::null_pointer`] for a piece without a face; those of the uv box.
fn faces_of_pieces(
    k: &Kernel,
    b: &mut Brep,
    faces: impl Iterator<Item = FaceId>,
    kept: &LawCornerPieces,
) -> Result<Vec<FaceId>> {
    let mut of: Vec<Option<FaceId>> = kept.pieces.iter().map(|_| None).collect();
    for f in faces {
        let bx = calculate_uv_domain_from_uv_trim_curves(k, b, f, true, false, false)?.domain;
        let mid = 0.5 * (bx.min().x + bx.max().x);
        let piece = kept.joined_ends.iter().position(|&end| mid < end);
        match piece.and_then(|piece| of.get_mut(piece)) {
            Some(slot @ None) => *slot = Some(f),
            _ => return Err(SolidError::sm_err(OP)),
        }
    }
    of.into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| SolidError::null_pointer(OP))
}

/// The edges of face `f`, in the face's order (`SmFace::GetEdges`).
fn face_edges(b: &Brep, f: FaceId) -> Result<Vec<EdgeId>> {
    crate::topology::face::p4::edges(b, f, None, false, None)
}

/// The points of edge `e`'s start and end vertices.
///
/// # Errors
///
/// [`SolidError::null_pointer`] for an edge without both (the fork's `NER`).
fn edge_ends(b: &Brep, e: EdgeId) -> Result<(Point3, Point3)> {
    let null = || SolidError::null_pointer(OP);
    let start = edge::start_vertex(b, e)?.ok_or_else(null)?;
    let end = edge::end_vertex(b, e)?.ok_or_else(null)?;
    Ok((vertex::point(b, start)?, vertex::point(b, end)?))
}

/// `sm_EdgeBetween` (the fork's 7babe17): the edge of face `f` between the points `a` and
/// `b` (either way round, each end within `tol`), `None` when it has none; edges without
/// both vertices are passed over.
fn edge_between(brep: &Brep, f: FaceId, a: Point3, b: Point3, tol: f64) -> Result<Option<EdgeId>> {
    for e in face_edges(brep, f)? {
        let (Some(start), Some(end)) = (edge::start_vertex(brep, e)?, edge::end_vertex(brep, e)?)
        else {
            continue;
        };
        let (s, t) = (vertex::point(brep, start)?, vertex::point(brep, end)?);
        if (s.closer_than(tol, a) && t.closer_than(tol, b))
            || (s.closer_than(tol, b) && t.closer_than(tol, a))
        {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

/// `sm_ReplaceIsoCrvOnSurf` (the fork's 6eecf04): an edge whose curve is a curve on a
/// surface along an iso line of that surface gets that iso curve, exactly, as its curve
/// (its part between the edge's ends, its way round), where it runs between the edge's
/// own ends within the edge's tolerance; other edges are left as they are.
///
/// # Errors
///
/// Those of the evaluations, the iso curve, its trim and reversal, and
/// [`crate::topology::brep::p2::replace_curve`].
fn replace_iso_crv_on_surf(
    k: &mut Kernel,
    b: &mut Brep,
    stores: &mut NotificationStores<'_>,
    e: EdgeId,
) -> Result<()> {
    let Some(curve) = edge::curve(b, e)? else {
        return Ok(());
    };
    let CurveKind::CrvOnSurf(cos) = curve.kind() else {
        return Ok(());
    };
    // the edge's ends in the surface's parameters
    let ivl = edge::interval(b, e)?;
    let uv0 = cos.uv_curve().evaluate_point(k, ivl.min())?;
    let uv1 = cos.uv_curve().evaluate_point(k, ivl.max())?;
    let surface = cos.surface();
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let tol = EFF_ZERO_SQRT * (1.0 + domain.size().length());
    let (param, iso, from, to) = if (uv0.x - uv1.x).abs() <= tol {
        (SurfParamType::U, uv0.x, uv0.y, uv1.y)
    } else if (uv0.y - uv1.y).abs() <= tol {
        (SurfParamType::V, uv0.y, uv0.x, uv1.x)
    } else {
        return Ok(()); // not along an iso line
    };
    if (to - from).abs() <= tol {
        return Ok(());
    }
    let mut iso_curve: Curve = surface
        .create_iso_parametric_curve(k, param, iso, 0.0, None, false)?
        .curve;

    // the iso curve runs along the other parameter: keep the edge's part, its way round
    let natural = iso_curve.natural_interval();
    let mut part = Extent1d::from_min_max(
        natural.min().max(from.min(to)),
        natural.max().min(from.max(to)),
    );
    if part.length() <= tol {
        return Ok(());
    }
    iso_curve.trim(k, &mut part, TrimMode::Notify)?;
    if to < from {
        let whole = iso_curve.natural_interval();
        iso_curve.reverse_parameterization(k, &whole)?;
    }

    // it must run between the edge's own ends
    let end0 = curve.evaluate_point(k, ivl.min())?;
    let end1 = curve.evaluate_point(k, ivl.max())?;
    let iso_ivl = iso_curve.natural_interval();
    let iso0 = iso_curve.evaluate_point(k, iso_ivl.min())?;
    let iso1 = iso_curve.evaluate_point(k, iso_ivl.max())?;
    let edge_tol = edge::tolerance(b, e)?;
    if !iso0.closer_than(edge_tol, end0) || !iso1.closer_than(edge_tol, end1) {
        return Ok(());
    }
    crate::topology::brep::p2::replace_curve(k, b, e, iso_curve, stores)
}
