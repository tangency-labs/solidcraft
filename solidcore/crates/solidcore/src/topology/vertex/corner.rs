//! The vertex placement checks behind `SmVertex::AssertValid` test 1: the vertex/face
//! gaps against the B-rep's intersection tolerance and the vertex's distance from the
//! corner its faces define (surface/surface/surface intersection, surface/surface
//! refinement or drop to one surface).
//!
//! Upstream: `source/SMLib/src/SmVertex.cpp`, `source/SMLib/inc/SmVertex.h`.
//!
//! | Rust                                  | Upstream                                  |
//! |---------------------------------------|-------------------------------------------|
//! | [`is_within_xsect_tol_3d_of_faces`]   | `SmVertex::IsWithinXSectTol3dOfFaces`     |
//! | [`has_parallel_faces`], [`ParallelFaces`] | `SmVertex::HasParallelFaces`          |
//! | [`calc_corner_from_faces`], [`FaceCorner`] | `SmVertex::CalcCornerFromFaces`      |
//! | [`is_on_face_corner`]                 | `SmVertex::IsOnFaceCorner`                |
//! | [`assert_test_vertex1`]               | `sm_AssertTestVertex1` (`SmVertex.cpp`)   |
//! | `vertex_scaled_zero` (private)        | `SmTol::GetScaledZero(const SmVertex&)`   |
//! | `brep_xsect_tol_3d` (private), [`xsect_tol_3d_of_brep`] | `SmTol::GetXSectTol3d(GetBrep())` (`SM_USE_OLDTOL`) |
//!
//! Deviations:
//! - **Out-params.** Optional outputs are always computed and returned in structs or
//!   tuples. `sm_AssertTestVertex1` is ported for the call `SmVertex::AssertValid`
//!   makes (`pOptMaxGap3d`, `pOptMaxGapFace` given, the corner outputs not), so the
//!   corner test runs only when the gap test passed.
//! - **Tolerances.** `SM_USE_NEWTOL` is not defined in the reference build, so the
//!   intersection tolerance is `2 * SmZoneTol3d(GetBrep()->GetTolerance())` (the zone
//!   tolerance constructed first, then doubled, as `SmTol`). A vertex outside any
//!   B-rep (`GetBrep()` null, which upstream dereferences) returns
//!   [`SolidError::InvalidInput`]. `SmTol::GetApproxTol3d(GetContext())` reads the
//!   zone tolerance of the vertex's context, the B-rep's, which upstream describes the
//!   model being worked on: here half the zone tolerance of the B-rep's own model size
//!   estimate (`approx_tol3d(k, 0.5 * brep_size_zone_tol_3d(k, b))`, the kernel
//!   context's when the B-rep stores the default), as the B-rep constructor reads it.
//! - **Temporary point.** `SmTemporaryChangeValue<SmPoint3d>` (the parallel-face branch
//!   of `CalcCornerFromFaces`) sets the vertex point and restores it afterwards; it is
//!   restored before any error is returned too.
//! - **Ignored statuses.** Upstream ignores the evaluations', `ComputeUVPoint`'s,
//!   `DropPoint`'s, `RefinePoint`'s and `LocalSurfaceSurfaceIntersect`'s statuses: a
//!   failed `ComputeUVPoint` leaves the previous UV, a failed evaluation the previous
//!   (initially undefined) vector, and a failed `AngleBetween` an angle of `0`. The port
//!   passes the statuses the fork passes on as it does (WR-663, tangency-labs/smlib
//!   7a35f33): `ComputeUVPoint` (after the surface check), the one-face `DropPoint`,
//!   the two-face `RefinePoint`, the three-face `LocalSurfaceSurfaceIntersect` and the
//!   parallel faces' point evaluations; `HasParallelFaces` skips a pair whose point,
//!   normal or angle cannot be evaluated, and `IsOnFaceCorner` takes a failed
//!   `CalcCornerFromFaces` as no best point.
//! - **Null pointers.** A face without surface where upstream dereferences it, and
//!   [`has_parallel_faces`] on a vertex without faces (upstream reads past an empty
//!   array), return [`SolidError::InvalidInput`]. `SM_DBG_WARN`, `SM_ASSERT_MSG` and the
//!   `SM_DEBUG_CODE` blocks are compiled out of the reference build.
//! - No input-driven recursion.

use crate::error::{ignored, Result, SolidError};
use crate::geom::tol;
use crate::intersect::surface_intersector::p1::{IntersectionPointType, SurfaceIntersector};
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point2, Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::knots::Side;
use crate::surface::surface::p1::map_to_2d;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::{vertexuse, Brep, FaceId, VertexId, VertexuseId};

const OP: &str = "topology::vertex::corner";

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The face of a vertex use (`pVertexuse->GetFaceuse()->GetFace()`), `None` without.
fn vertexuse_face(b: &Brep, vu: VertexuseId) -> Result<Option<FaceId>> {
    Ok(match vertexuse::faceuse(b, vu)? {
        Some(fu) => b.faceuse(fu)?.face,
        None => None,
    })
}

/// The surface of face `f`; upstream dereferences a null one.
fn face_surface(b: &Brep, f: FaceId) -> Result<&Surface> {
    b.face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| null("face without surface"))
}

/// `SmTol::GetScaledZero(const SmVertex&)` = `SM_EFF_ZERO * (1 + |point|max)`.
fn vertex_scaled_zero(b: &Brep, v: VertexId) -> Result<f64> {
    let p = b.vertex(v)?.point;
    Ok(tol::eff_zero() * (1.0 + p.max_dimension()))
}

/// `SmTol::GetXSectTol3d(GetBrep())` (`SM_USE_OLDTOL`) of the vertex's B-rep.
fn brep_xsect_tol_3d(k: &Kernel, b: &Brep, v: VertexId) -> Result<f64> {
    let brep = super::brep(b, v)?.ok_or_else(|| null("vertex outside a B-rep (GetBrep() null)"))?;
    Ok(xsect_tol_3d_of_brep(k, brep))
}

/// `SmTol::GetXSectTol3d(const SmBrep*)` (`SM_USE_OLDTOL`): `2 * GetZoneTol3d(pBrep)`,
/// in upstream's order: the B-rep's stored tolerance made a zone tolerance
/// (`SmZoneTol3d(double)`), doubled, then made an intersection tolerance
/// (`SmXSectTol3d(double)`).
#[must_use]
pub fn xsect_tol_3d_of_brep(k: &Kernel, brep: &Brep) -> f64 {
    tol::brep_xsect_tol_3d(k, brep, tol::SmallTopoType::NormalSize)
}

/// `SmVertex::IsWithinXSectTol3dOfFaces(&maxGap3d, &maxGapFace, bForceCalc)`: whether
/// every cached vertex/face gap of the vertex's uses is below the B-rep's
/// intersection tolerance, with the largest gap (`0` when none is above `0`) and its
/// face. `force_calc` clears every use's gap cache first, so all gaps are recomputed.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for a vertex
/// outside a B-rep; those of [`vertexuse::vertex_face_gap`].
pub fn is_within_xsect_tol_3d_of_faces(
    k: &Kernel,
    b: &mut Brep,
    v: VertexId,
    force_calc: bool,
) -> Result<(bool, f64, Option<FaceId>)> {
    // return value
    let mut rtn = true;

    // init output
    let mut max_gap_3d = 0.0;
    let mut max_gap_face = None;

    // locals (SM_USE_OLDTOL)
    let xsect_tol_3d = brep_xsect_tol_3d(k, b, v)?;
    let vertexuses = super::vertexuses(b, v)?;

    // when asked - clear Vertex/Face gap cache to force gap calculations for every
    // Vertexuse
    if force_calc {
        for &vu in &vertexuses {
            vertexuse::init_vertex_face_gap(b, vu)?;
        }
    }

    // for every Vertexuse
    for &vu in &vertexuses {
        // look for Vertex/Face gaps that exceed XSectTol3d
        if vertexuse::has_vertex_face_gap(b, vu)? {
            // side effect: Calc Vertex/Face gap on 1st fetch
            let gap = vertexuse::vertex_face_gap(k, b, vu, false)?
                .ok_or_else(|| null("vertex use without vertex/face gap"))?;
            let gap_3d = gap.gap_3d;

            // check for gaps that exceed XSectTol3d
            rtn &= gap_3d < xsect_tol_3d;

            // set output
            if max_gap_3d < gap_3d {
                max_gap_3d = gap_3d;
                max_gap_face = match gap.vertexuse {
                    Some(gvu) => vertexuse_face(b, gvu)?,
                    None => None,
                };
            }
        }
    }

    // all done
    Ok((rtn, max_gap_3d, max_gap_face))
}

/// The outputs of [`has_parallel_faces`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParallelFaces {
    /// The return value: some pair of faces has normals within the angle tolerance.
    pub parallel: bool,
    /// `pOptMinDihedralAngDeg`: the smallest angle (degrees) between face normals seen.
    pub min_dihedral_ang_deg: f64,
    /// `pOptMaxParallelFaceGap`: the largest gap between parallel face pairs.
    pub max_parallel_face_gap: f64,
    /// `pOptFace1`: the first face of that pair.
    pub face1: Option<FaceId>,
    /// `pOptFace2`: the second face of that pair.
    pub face2: Option<FaceId>,
    /// `pOptUV1`: the first face's gap UV point (undefined when no pair).
    pub uv1: Point2,
    /// `pOptUV2`: the second face's gap UV point (undefined when no pair).
    pub uv2: Point2,
}

/// The surface point and normal at a vertex use's gap UV. `false` when one cannot be
/// evaluated: the fork compares no such pair (tangency-labs/smlib 7a35f33; upstream
/// ignores both statuses and keeps the previous values).
fn gap_point_normal(k: &Kernel, surface: &Surface, uv: Point2, pn: &mut [Vec3; 2]) -> Result<bool> {
    match ignored(surface.evaluate_point(k, uv))? {
        Some(p) => pn[0] = p,
        None => return Ok(false),
    }
    match ignored(surface.evaluate_normal(k, uv, Side::Left, Side::Left))? {
        Some(n) => pn[1] = n,
        None => return Ok(false),
    }
    Ok(true)
}

/// `SmVertex::HasParallelFaces`: whether two of the vertex's faces have normals (at
/// their vertex/face gap points) within `ang_tol_deg` (`SM_ANG_TOL_DEG` when `None`)
/// of each other, with the smallest dihedral angle and the parallel pair of largest
/// gap. The gaps are taken from (and fill) the vertex uses' gap caches.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for a vertex
/// without faces or a face without surface (upstream dereferences them); those of
/// [`vertexuse::vertex_face_gap`].
pub fn has_parallel_faces(
    k: &Kernel,
    b: &mut Brep,
    v: VertexId,
    ang_tol_deg: Option<f64>,
) -> Result<ParallelFaces> {
    let mut out = ParallelFaces {
        parallel: false,
        min_dihedral_ang_deg: BIG_DOUBLE,
        max_parallel_face_gap: 0.0,
        face1: None,
        face2: None,
        uv1: Point2::UNDEF,
        uv2: Point2::UNDEF,
    };

    let mut max_parallel_face_gap = 0.0;
    let ang_tol_deg = ang_tol_deg.unwrap_or(tol::ANG_TOL_DEG);
    let mut pn1 = [Vec3::UNDEF; 2];
    let mut pn2 = [Vec3::UNDEF; 2];
    let faces = super::faces(b, v)?;

    let mut vertexuses = Vec::new();
    for &f in &faces {
        vertexuses.push(
            super::vertexuse_of_face(b, v, Some(f))?
                .ok_or_else(|| null("HasParallelFaces: face without vertex use"))?,
        );
    }

    // ii < GetSize()-1 reads past an empty array upstream
    let Some(last) = vertexuses.len().checked_sub(1) else {
        return Err(null("HasParallelFaces: vertex without faces"));
    };

    for (ii, &vu1) in vertexuses.iter().enumerate().take(last) {
        let face1 = vertexuse_face(b, vu1)?.ok_or_else(|| null("vertex use without face"))?;
        let Some(gap1) = vertexuse::vertex_face_gap(k, b, vu1, false)? else {
            continue;
        };
        if !gap_point_normal(k, face_surface(b, face1)?, gap1.face_uv, &mut pn1)? {
            continue;
        }

        for &vu2 in vertexuses.iter().skip(ii + 1) {
            let face2 = vertexuse_face(b, vu2)?.ok_or_else(|| null("vertex use without face"))?;
            let Some(gap2) = vertexuse::vertex_face_gap(k, b, vu2, false)? else {
                continue;
            };
            if !gap_point_normal(k, face_surface(b, face2)?, gap2.face_uv, &mut pn2)? {
                continue;
            }

            // AngleBetween reports 0 for a zero-length normal (status ignored upstream;
            // the fork does not compare the pair, tangency-labs/smlib 7a35f33)
            let Ok(dihedral_ang_rad) = pn1[1].angle_between(pn2[1]) else {
                continue;
            };
            let dihedral_ang_deg = rad_to_deg(dihedral_ang_rad);
            if out.min_dihedral_ang_deg > dihedral_ang_deg {
                out.min_dihedral_ang_deg = dihedral_ang_deg;
            }

            if dihedral_ang_deg < ang_tol_deg {
                out.parallel = true;

                let gap_3d = pn1[0].distance(pn2[0]);
                if max_parallel_face_gap <= gap_3d {
                    max_parallel_face_gap = gap_3d;
                    out.max_parallel_face_gap = gap_3d;
                    out.face1 = Some(face1);
                    out.face2 = Some(face2);
                    out.uv1 = gap1.face_uv;
                    out.uv2 = gap2.face_uv;
                }
            }
        }
    }

    Ok(out)
}

/// The outputs of [`calc_corner_from_faces`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceCorner {
    /// `rbFoundPoint`: the faces define a corner point.
    pub found_point: bool,
    /// `rdMaxGap3d`: when found, the distance of the vertex point from the corner
    /// (`-1` otherwise).
    pub max_gap_3d: f64,
    /// `rpMaxGapFace`: the face of the largest gap (set by the parallel-face branch).
    pub max_gap_face: Option<FaceId>,
    /// `rsBestPoint`: when found, the corner point (undefined otherwise).
    pub best_point: Point3,
}

/// `SmVertex::CalcCornerFromFaces`: the vertex position that best fits its faces'
/// surfaces. With parallel faces further apart than the approximation tolerance it is
/// the midpoint of their gap points; else, from the first up to three faces with
/// mutually non-parallel normals at the vertex, the surface/surface/surface
/// intersection (three), the refined surface/surface intersection point (two) or the
/// drop of the vertex point to the surface (one).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for a vertex
/// outside a B-rep and where upstream dereferences null; [`SolidError::Unported`] from
/// the evaluations and solves.
#[allow(clippy::too_many_lines)] // one upstream routine
pub fn calc_corner_from_faces(k: &Kernel, b: &mut Brep, v: VertexId) -> Result<FaceCorner> {
    let mut out = FaceCorner {
        found_point: false,
        max_gap_3d: -1.0,
        max_gap_face: None,
        best_point: Vec3::UNDEF,
    };

    let mut uv: Point2;
    // the target faces with their UVs and normals: at most three, the ones the corner
    // calculation handles, so upstream's default case (more) cannot happen
    let mut tgt: [Option<(FaceId, Point2, Vec3)>; 3] = [None; 3];
    let mut n = 0usize;

    let vertexuses = super::vertexuses(b, v)?;
    let faces = super::faces(b, v)?;

    if faces.is_empty() {
        return Ok(out);
    }

    // (upstream asks the B-rep's context, there this model's: half the zone tolerance
    // of this B-rep's own model size estimate)
    let approx_tol_3d = tol::approx_tol3d(k, 0.5 * tol::brep_size_zone_tol_3d(k, b));

    let parallel = has_parallel_faces(k, b, v, Some(tol::ANG_TOL_DEG))?;

    if parallel.parallel && parallel.max_parallel_face_gap > approx_tol_3d {
        let f1 = parallel
            .face1
            .ok_or_else(|| null("parallel faces without face1"))?;
        let f2 = parallel
            .face2
            .ok_or_else(|| null("parallel faces without face2"))?;
        // fork 7a35f33 (WR-663): SER(EvaluatePoint) for both points.
        let pt1 = face_surface(b, f1)?.evaluate_point(k, parallel.uv1)?;
        let pt2 = face_surface(b, f2)?.evaluate_point(k, parallel.uv2)?;

        out.found_point = true;
        out.best_point = (pt1 + pt2) / 2.0;

        // GetVertexuses appends to sVertexuses: every use is listed twice, the resets
        // repeat
        for &vu in vertexuses.iter().chain(vertexuses.iter()) {
            vertexuse::init_vertex_face_gap(b, vu)?;
        }

        // SmTemporaryChangeValue: the vertex point is the best point for the gap pass
        let saved = b.vertex(v)?.point;
        b.vertex_mut(v)?.point = out.best_point;
        let within = is_within_xsect_tol_3d_of_faces(k, b, v, true);
        b.vertex_mut(v)?.point = saved;
        let (_, gap, face) = within?;
        out.max_gap_3d = gap;
        out.max_gap_face = face;

        for &vu in vertexuses.iter().chain(vertexuses.iter()) {
            vertexuse::init_vertex_face_gap(b, vu)?;
        }

        // (the destructor restores the point here upstream)
        return Ok(out);
    }

    for &vu in &vertexuses {
        let Some(fu) = vertexuse::faceuse(b, vu)? else {
            continue;
        };

        // FALSE = Use GlobalSolve without touching UVTrimCurve. The fork's order: faces
        // without surface are skipped before the drop, whose status is passed on
        // (tangency-labs/smlib 7a35f33; upstream drops first and ignores the status).
        let face = b
            .faceuse(fu)?
            .face
            .ok_or_else(|| null("face use without face"))?;
        if b.face(face)?.surface.is_none() {
            continue;
        }
        uv = vertexuse::compute_uv_point(k, b, vu, false)?;
        let Some(surface) = b.face(face)?.surface.as_ref() else {
            continue;
        };

        let Some(surface_normal) = ignored(surface.evaluate_normal(k, uv, Side::Left, Side::Left))?
        else {
            continue;
        };

        if let Some(&(_, tgt_uv, _)) = tgt.iter().flatten().find(|t| t.0 == face) {
            let zone = crate::topology::face::p2::tolerance(b, face)?;
            let tol_2d = map_to_2d(k, zone, uv, surface)?;
            let close = tgt_uv.closer_than(tol_2d, uv);
            if close {
                continue;
            }
        }

        // switch with fall-through on the number of found targets
        if n <= 2 {
            // case 2: skip surfaces with normals parallel to 2nd target
            let skip2 = n == 2
                && tgt
                    .get(1)
                    .copied()
                    .flatten()
                    .is_some_and(|(_, _, t)| surface_normal.is_parallel_to(t, 1.0));
            // case 1: skip surfaces with normals parallel to 1st target
            let skip1 = !skip2
                && n >= 1
                && tgt
                    .first()
                    .copied()
                    .flatten()
                    .is_some_and(|(_, _, t)| surface_normal.is_parallel_to(t, 1.0));
            // case 0: arrive here - save the face as next target
            if !skip2 && !skip1 {
                if let Some(slot) = tgt.get_mut(n) {
                    *slot = Some((face, uv, surface_normal));
                    n += 1;
                }
            }
        }
        // default: the vertexuse goes to sChkVertexuses, which nothing reads
    }

    let f0 = match tgt {
        [None, _, _] => {
            // SM_DBG_WARN: unexpected number of TgtFaces
            return Ok(out);
        }

        [Some((f0, uv0, _)), None, _] => {
            // Drop Current Vertex->Point to Face->Surface
            let surface = face_surface(b, f0)?;
            let point = b.vertex(v)?.point;
            let opts = DropPointOptions {
                guess_uv: Some(uv0),
                ..Default::default()
            };
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            // (status passed on: the fork's SER, tangency-labs/smlib 7a35f33)
            let d = surface.drop_point::<NoObject>(k, point, &domain, &opts)?;
            uv = d.result_uv;
            out.max_gap_3d = d.gap;
            let found_answer = d.success;

            if !found_answer {
                return Ok(out);
            }
            f0
        }

        [Some((f0, uv0, _)), Some((f1, uv1, _)), None] => {
            // Refine Tgt DropPoints to Intersection Curve between two faces
            let surface1 = face_surface(b, f0)?;
            let surface2 = face_surface(b, f1)?;
            let ssi = SurfaceIntersector::new(
                k,
                surface1,
                &surface1.natural_uv_domain_with_nurb(k)?,
                surface2,
                &surface2.natural_uv_domain_with_nurb(k)?,
                false,
            )?;
            // CROSSING here works for Tangent curve cases as well
            let r = ssi.refine_point(
                k,
                uv0,
                uv1,
                IntersectionPointType::Crossing,
                None,
                None,
                false,
            );
            // (status passed on: the fork's SER, tangency-labs/smlib 7a35f33)
            let (found_answer, uvs) = match r? {
                Some(uvs) => (true, uvs),
                None => (false, [Point2::UNDEF; 2]),
            };

            if !found_answer {
                return Ok(out);
            }

            uv = uvs[0];
            f0
        }

        [Some((f0, uv0, _)), Some((f1, uv1, _)), Some((f2, uv2, _))] => {
            // find the surf/surf/surf intersection of the 3 face->Surfaces
            let d0 = crate::topology::face::p2::uv_domain(b, f0)?;
            let d1 = crate::topology::face::p2::uv_domain(b, f1)?;
            let d2 = crate::topology::face::p2::uv_domain(b, f2)?;
            let s0 = face_surface(b, f0)?;
            let s1 = face_surface(b, f1)?;
            let s2 = face_surface(b, f2)?;
            // (status passed on: the fork's SER, tangency-labs/smlib 7a35f33)
            let sol =
                s0.local_surface_surface_intersect(k, &d0, s1, &d1, s2, &d2, uv0, uv1, uv2)?;
            let Some(s) = sol else {
                return Ok(out);
            };
            uv = s.uv1;
            f0
        }
    };

    // SM_ASSERT_MSG(bFoundAnswer == TRUE) is compiled out

    out.found_point = true;

    // (status passed on: the fork's SER, tangency-labs/smlib 7a35f33)
    out.best_point = face_surface(b, f0)?.evaluate_point(k, uv)?;

    out.max_gap_3d = out.best_point.distance(b.vertex(v)?.point);

    Ok(out)
}

/// `SmVertex::IsOnFaceCorner`: whether the vertex lies within its scaled zero of the
/// corner point its faces define ([`calc_corner_from_faces`], status ignored), or the
/// faces define none. Returns `(on_corner, corner)`.
///
/// # Errors
///
/// The unported or cancelled errors of [`calc_corner_from_faces`] that carry no upstream
/// status of its own; any other failure is no best point, as upstream ignores the status.
pub fn is_on_face_corner(k: &Kernel, b: &mut Brep, v: VertexId) -> Result<(bool, FaceCorner)> {
    // look for a best vertex point position defined by the faces connected to the
    // vertex (status ignored: the outputs keep their initial values)
    let corner = match calc_corner_from_faces(k, b, v) {
        Ok(c) => c,
        // a failure with an upstream status leaves the initial outputs; as the fork, any
        // failure is no best point (tangency-labs/smlib 7a35f33)
        Err(e)
            if (e.operation() == OP && e.failed_routine_status().is_some())
                || !e.is_unported_or_cancelled() =>
        {
            FaceCorner {
                found_point: false,
                max_gap_3d: -1.0,
                max_gap_face: None,
                best_point: Vec3::UNDEF,
            }
        }
        Err(e) => return Err(e),
    };

    // check the gap
    let scaled_zero = vertex_scaled_zero(b, v)?;
    let rtn = !corner.found_point || tol::in_tol(corner.max_gap_3d, scaled_zero);

    // all done
    Ok((rtn, corner))
}

/// `sm_AssertTestVertex1(pVertex, &dMaxGap3d, &pMaxGapFace)` as `SmVertex::AssertValid`
/// calls it: `(passed, max_gap_3d, max_gap_face)`. Passed when every vertex/face gap is
/// below the B-rep's intersection tolerance ([`is_within_xsect_tol_3d_of_faces`]) and,
/// only then, the vertex lies on its faces' corner ([`is_on_face_corner`]).
///
/// # Errors
///
/// Those of [`is_within_xsect_tol_3d_of_faces`] and [`is_on_face_corner`].
pub fn assert_test_vertex1(
    k: &Kernel,
    b: &mut Brep,
    v: VertexId,
) -> Result<(bool, f64, Option<FaceId>)> {
    // Vertex/Face gaps <= XSectTol3d
    let (mut rtn, max_gap_3d, max_gap_face) = is_within_xsect_tol_3d_of_faces(k, b, v, false)?;

    // bRtn == FALSE when Vertex Point is more than ScaledZero from best corner pos
    // defined by connected faces
    if rtn {
        rtn &= is_on_face_corner(k, b, v)?.0;
    }

    // all done
    Ok((rtn, max_gap_3d, max_gap_face))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::create_box;

    /// A 1 x 2 x 3 box and its vertex at the origin corner.
    fn box_corner() -> (Kernel, Brep, VertexId) {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Vec3::ZERO, 1.0, 2.0, 3.0).unwrap();
        let v = b
            .vertices()
            .ids()
            .find(|&v| b.vertex(v).unwrap().point.distance(Vec3::ZERO) == 0.0)
            .unwrap();
        (k, b, v)
    }

    /// Moves the vertex by `d` and clears its uses' gap caches.
    fn move_vertex(b: &mut Brep, v: VertexId, d: Vec3) {
        let p = b.vertex(v).unwrap().point;
        b.vertex_mut(v).unwrap().point = p + d;
        for vu in super::super::vertexuses(b, v).unwrap() {
            vertexuse::init_vertex_face_gap(b, vu).unwrap();
        }
    }

    #[test]
    fn box_corner_passes() {
        let (k, mut b, v) = box_corner();
        let (within, gap, _) = is_within_xsect_tol_3d_of_faces(&k, &mut b, v, false).unwrap();
        assert!(within);
        assert_eq!(gap, 0.0);
        let (on, corner) = is_on_face_corner(&k, &mut b, v).unwrap();
        assert!(on);
        assert!(corner.found_point);
        assert!(corner.max_gap_3d.abs() <= vertex_scaled_zero(&b, v).unwrap());
        let (passed, gap, _) = assert_test_vertex1(&k, &mut b, v).unwrap();
        assert!(passed);
        assert_eq!(gap, 0.0);
        let p = has_parallel_faces(&k, &mut b, v, None).unwrap();
        assert!(!p.parallel);
        assert!((p.min_dihedral_ang_deg - 90.0).abs() < 1e-9);
    }

    // tangency-labs/smlib 7a35f33 (SmVertex::HasParallelFaces): a face pair whose
    // points or normals cannot be evaluated is not compared. Here one face's surface
    // is swapped for one that cannot be evaluated after its gap was cached.
    #[test]
    fn issue_2009_parallel_faces_skip_an_unevaluable_pair() {
        let (k, mut b, v) = box_corner();
        let _ = is_within_xsect_tol_3d_of_faces(&k, &mut b, v, false).unwrap();
        let f = super::super::faces(&b, v).unwrap()[0];
        b.face_mut(f).unwrap().surface = Some(Surface::from(
            crate::nurbs::b_spline_surface::BSplineSurface::default(),
        ));
        let on = has_parallel_faces(&Kernel::default(), &mut b, v, None).unwrap();
        assert!(!on.parallel, "{on:?}");
        assert!((on.min_dihedral_ang_deg - 90.0).abs() < 1e-9, "{on:?}");
    }

    #[test]
    fn vertex_off_its_faces_fails_the_gap_test() {
        let (k, mut b, v) = box_corner();
        // 1e-3 off the x = 0 face: beyond XSectTol3d = 2 * the B-rep tolerance
        move_vertex(&mut b, v, Vec3::new(1.0e-3, 0.0, 0.0));
        let xsect = brep_xsect_tol_3d(&k, &b, v).unwrap();
        assert!(xsect < 1.0e-3);
        let (passed, gap, face) = assert_test_vertex1(&k, &mut b, v).unwrap();
        assert!(!passed);
        assert!((gap - 1.0e-3).abs() <= 1e-15, "{gap}");
        assert!(face.is_some());
    }

    #[test]
    fn vertex_within_xsect_tol_but_off_the_corner_fails() {
        let (k, mut b, v) = box_corner();
        // 1e-7: below XSectTol3d, above the vertex's scaled zero
        move_vertex(&mut b, v, Vec3::new(0.0, 1.0e-7, 0.0));
        let (within, gap, _) = is_within_xsect_tol_3d_of_faces(&k, &mut b, v, false).unwrap();
        assert!(within);
        assert!((gap - 1.0e-7).abs() <= 1e-18, "{gap}");
        let (on, corner) = is_on_face_corner(&k, &mut b, v).unwrap();
        assert!(!on);
        assert!(corner.found_point);
        assert!(
            (corner.max_gap_3d - 1.0e-7).abs() <= 1e-15,
            "{}",
            corner.max_gap_3d
        );
        assert!(corner.best_point.distance(Vec3::ZERO) <= 1e-12);
        let (passed, gap, _) = assert_test_vertex1(&k, &mut b, v).unwrap();
        assert!(!passed);
        assert!((gap - 1.0e-7).abs() <= 1e-18, "{gap}");
    }
}
