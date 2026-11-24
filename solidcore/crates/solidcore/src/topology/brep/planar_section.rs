//! Planar sections: the curves where an infinite plane cuts a surface
//! ([`surface_planar_section_curves`]) or the faces of a B-rep
//! ([`create_planar_section_curves`]), with the bounded plane they intersect
//! ([`create_plane_from_bbox`]).
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/src/SmSurface.cpp`,
//! `source/SMLib/inc/SmBrep.h`, `source/SMLib/inc/SmSurface.h`.
//!
//! | Rust                                  | Upstream                                     |
//! |---------------------------------------|----------------------------------------------|
//! | [`create_planar_section_curves`], [`PlanarSectionOptions`], [`SectionOutputs`], [`PlanarSection`] | `SmBrep::CreatePlanarSectionCurves` |
//! | [`surface_planar_section_curves`], [`face_planar_section_curves`], [`SurfaceSection`] | `SmSurface::CreatePlanarSectionCurves` (no override) |
//! | [`create_plane_from_bbox`]            | `SmSurface::CreatePlaneFromBBox` (static)    |
//!
//! Deviations:
//! - **Outputs.** The optional output arrays are the [`SectionOutputs`] flags of
//!   [`PlanarSectionOptions`] and `Option` fields of the result; the curves are owned
//!   values, so upstream's delete lists and cleanup objects have no counterpart. The
//!   faces are [`FaceId`]s.
//! - **Face context.** A face's surface is intersected and dropped onto as an
//!   evaluation copy (keeping its owner and cache slot) through the B-rep's
//!   [`BrepFaceServices`], so its trimmed cache reaches the face as upstream's does
//!   through `GetFace()`; its lift reads the surface's own continuities and makes no
//!   cache. `smsurf_GetSurfaceCache(this)` before the intersection is that
//!   context's [`SurfaceSolveContext::surface_cache_without_get_tree`] (upstream only reads the
//!   cache's continuity there, and ignores it).
//! - **Statuses.** A face whose surface section fails is skipped, as upstream's
//!   `continue`; so is a face whose trimming fails. [`SolidError::Unported`] always
//!   propagates. `GlobalCoincidenceChecker`'s status is ignored, as upstream: its
//!   solutions are read as a failed check left them
//!   ([`Curve::global_coincidence_checker_with_partial`]). `IsNurbSurfacePlane` failing inside (upstream's `SER` there reads as a
//!   `TRUE` result with no plane) leaves no plane, so the surface has no section.
//! - **Getters.** Upstream's getters that report no status (`IsPlanar`, `IsDegenerate`,
//!   `IsEmbedded`, `GetEdges`, `GetCurve`, `GetInterval`) propagate only
//!   [`SolidError::Unported`] and [`SolidError::NotFound`] here; any other failure reads
//!   as `false` or an empty list or interval. A face edge without curve (upstream
//!   dereferences it) is `SM_ERR_NULL_POINTER`.
//! - **Forced UV curves.** As upstream, the replacement of a 3D section curve by the
//!   lift of its first drop is tied to the first drop curve (`kk == 0`), even when that
//!   one is rejected and a later one is kept (that one is then added).
//! - **Face tolerance.** A face's section with no `cpdOpt3DApproximationTol` is traced
//!   to the face's tolerance ([`face_planar_section_curves`], WR-723); upstream passes
//!   the null on, and the intersector traces to 1/1000 of the surface's size.
//! - **Covers of one piece.** Trimmed in 3D, a face's pieces that lie wholly on another of
//!   its pieces are dropped ([`distinct_pieces`]): on a surface that covers itself (a spline
//!   winding round one circle several times) upstream returns each cover of the face's
//!   piece, and one of them stops inside the face where the surface ends.
//! - **Not ported here.** The `SM_DEBUG_CODE` drawing blocks and
//!   `SM_VALIDATE_INTERSECTORS`.

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::{Extent1d, ExtentPointType};
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::{Solution, SolutionType};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::intersect::surface_drop_lib::p3::DropSolveContext;
use crate::kernel::Kernel;
use crate::math::{max2, max4, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::query::brep::BrepFaceServices;
use crate::query::face::p3::{surface_bounding_box, surface_is_planar};
use crate::surface::plane::Plane;
use crate::surface::surface::p1::SurfaceSolveContext;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::topology::brep::p1 as brep1;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge;
use crate::topology::face::{p2 as face2, p4 as face4};
use crate::topology::{Brep, FaceId};

const OP_PLANE: &str = "topology::brep::planar_section::create_plane_from_bbox";
const OP_SECTION: &str = "topology::brep::planar_section::create_planar_section_curves";

/// `SmSurface::CreatePlaneFromBBox`: the plane through `plane_point` with normal
/// `plane_normal`, bounded to a square twice the size of the bounding sphere of `bbox`
/// around the sphere centre's projection, as the analytic plane `IsNurbSurfacePlane`
/// makes of the bilinear surface through its corners. `None` when the plane misses the
/// box (by more than `opt_tol`, `0` scaling the tolerance to the box and the plane
/// point) or the bounding sphere.
///
/// # Errors
///
/// Those of `Unitize` (a zero normal), `MakeUnitOrthoVectors` and
/// `CreateBilinearSurface`; `SM_ERR` when the bilinear surface is not a plane;
/// [`SolidError::Unported`] from the plane recognition.
pub fn create_plane_from_bbox(
    k: &Kernel,
    bbox: &Extent3d,
    plane_point: Point3,
    plane_normal: Vec3,
    opt_tol: f64,
) -> Result<Option<Surface>> {
    // locals
    let sphere = bbox.sphere_bound();
    let (sph_center, sph_radius) = (sphere.center, sphere.radius);
    let unit_normal = plane_normal.unitize()?;

    // Check the closest approach of the plane and the box.

    // Figure up a tol first.
    let size1 = bbox.size().length_squared();
    let size2 = bbox.min().length_squared();
    let size3 = bbox.max().length_squared();
    let size4 = plane_point.length_squared();
    let size = max4(size1, size2, size3, size4);
    let scaled_zero = EFF_ZERO * (1.0 + size.sqrt());
    let tol = max2(opt_tol, scaled_zero);

    // (upstream measures with the normal as given, not the unitized one)
    if bbox.distance_to_plane(plane_point, plane_normal).abs() > tol {
        return Ok(None);
    }

    // Drop the point on the sphere onto the plane.
    let proj_point = sph_center.project_point_to_plane(plane_point, unit_normal);

    // If the distance to the plane is greater than the the sphere radius, no
    // intersection exists - just exit.
    if sph_radius < proj_point.distance(sph_center) {
        return Ok(None);
    }

    // Use the difference vector to create a coordinate system for the plane.
    let diff = proj_point - plane_point;
    let dist_to_cent = diff.length();
    let [_z_ax, x_ax, y_ax] = plane_normal.make_unit_ortho_vectors(Some(diff))?;

    // Now that we have the plane coordinate system create four points on the plane which
    // surround the projection of the sphere onto the plane.
    let proj_radius = sph_radius * 2.0; // Make the plane twice the size of the sphere
    let p00 = plane_point + (dist_to_cent - proj_radius) * x_ax - proj_radius * y_ax;
    let p10 = plane_point + (dist_to_cent + proj_radius) * x_ax - proj_radius * y_ax;
    let p11 = plane_point + (dist_to_cent + proj_radius) * x_ax + proj_radius * y_ax;
    let p01 = plane_point + (dist_to_cent - proj_radius) * x_ax + proj_radius * y_ax;

    // Now create the plane
    let bilinear = BSplineSurface::create_bilinear_surface(p00, p10, p01, p11)?;
    match Plane::is_nurb_surface_plane(k, &bilinear, 1.0) {
        Ok(Some(plane)) => Ok(Some(Surface::from(plane))),
        Ok(None) => Err(SolidError::topology_failed(
            OP_PLANE,
            "the bounding box's corners are not on a plane",
        )),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        // (an `SER` inside reads as `TRUE` and leaves no plane)
        Err(_) => Ok(None),
    }
}

/// The result of [`surface_planar_section_curves`].
#[derive(Debug, Clone, Default)]
pub struct SurfaceSection {
    /// `pOpt3DSectionCurves`: the 3D intersection curves.
    pub curves_3d: Vec<Curve>,
    /// `pOptUVSectionCurves`: their UV curves on the surface (`None` for an
    /// intersector that makes none).
    pub uv_curves: Vec<Option<Curve>>,
}

/// `SmSurface::CreatePlanarSectionCurves`: the curves where the infinite plane through
/// `plane_point` with normal `plane_normal` cuts `surface` over `uv_domain`. `bbox` is
/// the surface's `CalculateBoundingBox(uv_domain)`; the plane is bounded to it
/// ([`create_plane_from_bbox`], with `approx_tol_3d` or `0`) and intersected with the
/// surface (`GlobalSurfaceIntersect`, the surface's boundary edges used). `context`
/// (the face solve context and the surface's handle in it) serves a face-owned
/// surface's cache.
///
/// # Errors
///
/// Those of [`create_plane_from_bbox`], the surface cache and `GlobalSurfaceIntersect`.
pub fn surface_planar_section_curves(
    k: &Kernel,
    surface: &Surface,
    uv_domain: &Extent2d,
    bbox: &Extent3d,
    plane_point: Point3,
    plane_normal: Vec3,
    approx_tol_3d: Option<f64>,
    ang_tol_rad: Option<f64>,
    context: Option<(
        &mut dyn SurfaceSolveContext<CacheObject>,
        Option<CacheObject>,
    )>,
) -> Result<SurfaceSection> {
    // make temporary nurb plane which is larger than the surface BBox.
    let tol = approx_tol_3d.unwrap_or(0.0);
    let Some(plane) = create_plane_from_bbox(k, bbox, plane_point, plane_normal, tol)? else {
        // No intersection with box just return.
        return Ok(SurfaceSection::default());
    };
    let plane_domain = plane.natural_uv_domain_with_nurb(k)?;

    // intersect the surface with the plane
    // (the surface's bounding edges do intersect the plane, the plane's do not)
    let use_surface_edges = [true, false];
    let found = if let Some((context, handle)) = context {
        // get surface cache (upstream then reads its continuity and ignores it)
        context.surface_cache_without_get_tree(k, surface, handle)?;
        surface.global_surface_intersect_with_context(
            k,
            [handle, None],
            context,
            uv_domain,
            &plane,
            &plane_domain,
            use_surface_edges,
            approx_tol_3d,
            ang_tol_rad,
        )?
    } else {
        surface.global_surface_intersect(
            k,
            uv_domain,
            &plane,
            &plane_domain,
            use_surface_edges,
            approx_tol_3d,
            ang_tol_rad,
        )?
    };
    Ok(SurfaceSection {
        curves_3d: found.curves_3d,
        uv_curves: found.surface1_uv_curves,
    })
}

/// The section of face `f` by `plane` (its origin and z axis), as the plane cutter takes it:
/// none when the face's box `face_box` lies farther from the plane than `tol`, otherwise
/// [`face_planar_section_curves`] on the plane's point and normal.
///
/// # Errors
///
/// Those of [`face_planar_section_curves`].
#[allow(clippy::too_many_arguments)] // face_planar_section_curves' arguments, with the plane's
pub fn face_plane_section_curves(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    surface: &Surface,
    uv_domain: &Extent2d,
    plane: &Plane,
    face_box: &Extent3d,
    tol: f64,
    approx_tol_3d: Option<f64>,
    ang_tol_rad: Option<f64>,
) -> Result<Option<SurfaceSection>> {
    let at = plane.position().origin();
    let normal = plane.position().z_axis();
    if face_box.distance_to_plane(at, normal).abs() > tol {
        return Ok(None);
    }
    face_planar_section_curves(
        k,
        b,
        f,
        surface,
        uv_domain,
        at,
        normal,
        approx_tol_3d,
        ang_tol_rad,
    )
    .map(Some)
}

/// `pFace->GetSurface()->CreatePlanarSectionCurves(crContext, pFace->GetUVDomain(), ..)`
/// as [`create_planar_section_curves`] calls it on face `f` of `b`: `surface` is an
/// evaluation copy of the face's surface (same owner and cache slot) and `uv_domain`
/// the face's UV domain; the box is the surface's `CalculateBoundingBox(uv_domain)` and
/// the solves run through the B-rep's [`BrepFaceServices`].
///
/// The section curves are traced to `approx_tol_3d`, or to the face's tolerance when it
/// is `None` (WR-723). Upstream passes `None` on, and the
/// intersector then traces to 1/1000 of the surface's size, which on a round left the
/// curve up to 32 times the face's tolerance off it.
///
/// # Errors
///
/// As [`surface_planar_section_curves`], and those of the bounding box;
/// [`SolidError::NotFound`] for a dangling face.
pub fn face_planar_section_curves(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    surface: &Surface,
    uv_domain: &Extent2d,
    plane_point: Point3,
    plane_normal: Vec3,
    approx_tol_3d: Option<f64>,
    ang_tol_rad: Option<f64>,
) -> Result<SurfaceSection> {
    let approx_tol_3d = match approx_tol_3d {
        None => Some(face2::tolerance(b, f)?),
        tol => tol,
    };
    let bbox = surface_bounding_box(k, b, f, surface, uv_domain)?;
    let mut services = BrepFaceServices::new(&mut *b);
    surface_planar_section_curves(
        k,
        surface,
        uv_domain,
        &bbox,
        plane_point,
        plane_normal,
        approx_tol_3d,
        ang_tol_rad,
        Some((&mut services, Some(CacheObject::Surface(f)))),
    )
}

/// The outputs [`create_planar_section_curves`] is asked for (upstream's optional
/// output arrays).
#[derive(Debug, Clone, Copy, Default)]
pub struct SectionOutputs {
    /// `pOpt3DSectionCurves`.
    pub curves_3d: bool,
    /// `pOptUVSectionCurves`.
    pub uv_curves: bool,
    /// `pOptFacesOfUVCurves`.
    pub faces: bool,
}

/// The optional arguments of [`create_planar_section_curves`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanarSectionOptions {
    /// `cpdOpt3DApproximationTol`: the largest distance between a section curve and the
    /// surface. `None` takes each face's tolerance, for the coincidence checks and for
    /// the section itself (WR-723; upstream's intersector default is 1/1000 of the
    /// surface's size).
    pub approx_tol_3d: Option<f64>,
    /// `cpdOptAngleTolRadians`.
    pub ang_tol_rad: Option<f64>,
    /// The outputs asked for.
    pub outputs: SectionOutputs,
    /// `bForceUVClassify`: give a section curve without UV curve one by dropping it on
    /// the surface (replacing the 3D curve by the drop's lift); only with
    /// [`SectionOutputs::uv_curves`]. Defaults to `false`, as upstream's
    /// `bForceUVClassify` does (`SmBrep.h:542-552`). Without it, UV section curves
    /// requested for faces whose intersector makes no UV curve (planes, cylinders,
    /// cones, spheres) are silently dropped by the trim step, matching upstream
    /// (`SmBrep.cpp:5387`/`5480`, `SmFace.cpp:6990-7010`). Callers that need UV curves
    /// should set it `true`; callers that do not should request no UV output, as
    /// `SM_API`'s section function does.
    pub force_uv_classify: bool,
}

/// The result of [`create_planar_section_curves`].
#[derive(Debug, Clone, Default)]
pub struct PlanarSection {
    /// `pOpt3DSectionCurves`: the planar 3D curves (when asked for).
    pub curves_3d: Option<Vec<Curve>>,
    /// `pOptUVSectionCurves`: the UV trim curve of each section curve (when asked for).
    pub uv_curves: Option<Vec<Option<Curve>>>,
    /// `pOptFacesOfUVCurves`: the face of each UV curve, or of each 3D curve when a
    /// face gave no UV curves (when asked for).
    pub faces: Option<Vec<FaceId>>,
}

/// `SmBrep::CreatePlanarSectionCurves`: the section of `b` by the infinite plane
/// through `plane_point` with normal `plane_normal`. Every face's surface is
/// sectioned over the face's UV domain ([`surface_planar_section_curves`]); section
/// curves that are degenerate stay, those coincident with a section curve of an earlier
/// face or (on a planar face) with an embedded edge of the face are removed; with
/// `force_uv_classify` and the UV output, a curve without UV curve gets one; the curves are
/// then trimmed to the face (`CreateCurvesByTrimming`). Trimmed in 3D (without the UV
/// output), a piece that lies wholly on another piece of the same face is dropped
/// ([`distinct_pieces`]).
///
/// Note: without [`PlanarSectionOptions::force_uv_classify`], requested UV curves of
/// analytic faces (plane, cylinder, cone, sphere) are dropped; see that field.
///
/// Warning (upstream): if the plane crosses a surface seam you may get 2 sets of curves.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; `SM_ERR_NULL_POINTER` for a face without
/// surface; those of the planarity test, the embedded edge test and, with
/// `force_uv_classify`, of `DropAndTrimCurve`, the drop curve evaluations, `DropPoint`
/// and `LiftCurve` (upstream's `SER`); those of [`distinct_pieces`] on a face's 3D-trimmed
/// pieces; [`SolidError::Unported`].
pub fn create_planar_section_curves(
    k: &Kernel,
    b: &mut Brep,
    plane_point: Point3,
    plane_normal: Vec3,
    opts: &PlanarSectionOptions,
) -> Result<PlanarSection> {
    // init outputs
    let mut out_uv: Vec<Option<Curve>> = Vec::new();
    let mut out_faces: Vec<FaceId> = Vec::new();

    // the 3D section curves of the faces so far, which later section curves are checked
    // against (upstream's local array stands in when the caller asks for none, but only
    // the caller's array is appended to: without the 3D output it stays empty)
    let mut section_3d: Vec<Curve> = Vec::new();

    // get all faces
    let faces = brep1::faces(b, None)?;

    // for every face
    for f in faces {
        let uv_domain = face2::uv_domain(b, f)?;
        // the face's surface, as an evaluation copy (same owner and cache slot)
        let surface = face2::surface(b, f)?
            .ok_or_else(|| SolidError::topology_missing(OP_SECTION, "the face's surface"))?
            .clone_for_evaluation()?;
        let face_tol = face2::tolerance(b, f)?;

        let tol = opts.approx_tol_3d.unwrap_or(face_tol);

        let is_planar = status_free(surface_is_planar(k, b, f, &surface, tol), false)?;

        // create surface planar sections - put output curves into s3DCurves and sUVCurves
        let section = face_planar_section_curves(
            k,
            b,
            f,
            &surface,
            &uv_domain,
            plane_point,
            plane_normal,
            opts.approx_tol_3d,
            opts.ang_tol_rad,
        );
        let (mut curves_3d, mut uv_curves) = match section {
            Ok(s) => (s.curves_3d, s.uv_curves),
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => continue,
        };
        // (one UV entry per 3D curve, as upstream indexes them)
        uv_curves.resize_with(curves_3d.len(), || None);

        // for every surface/plane intersection (the list grows with forced UV curves)
        let mut index = 0;
        while index < curves_3d.len() {
            let Some(curve) = curves_3d.get(index) else {
                break;
            };
            let orig_3d = curve.clone();
            let ivl = orig_3d.natural_interval();
            let param_tol = 1.0 / 100.0 * (EFF_ZERO_SQRT + ivl.length());

            // skip degenerate curves - currently they stay in the output
            if status_free(orig_3d.is_degenerate(k, EFF_ZERO, None), false)? {
                index += 1;
                continue;
            }

            // remove curves coincident with existing solutions and embedded edges on planar
            // faces
            let is_full_coincidence = |other: &Curve, other_ivl: &Extent1d| -> Result<bool> {
                // (status ignored upstream: the solutions are read as the check
                // left them)
                let (found, status) = orig_3d.global_coincidence_checker_with_partial::<NoObject>(
                    k,
                    &ivl,
                    other,
                    other_ivl,
                    tol,
                    true,
                    [None, None],
                );
                if let Err(e) = status {
                    if e.is_unported_or_cancelled() {
                        return Err(e);
                    }
                }
                let found = found.solutions;
                Ok(match found.as_slice() {
                    [s] => {
                        s.solution_type == SolutionType::RangeOfValues
                            && ivl.classify_point(s.start.param(0)?, param_tol)
                                == ExtentPointType::Start
                            && ivl.classify_point(s.end.param(0)?, param_tol)
                                == ExtentPointType::End
                    }
                    _ => false,
                })
            };

            // for all accumulated 3dSection curves - check for coincidence
            let mut coin_with_section = false;
            for other in &section_3d {
                if is_full_coincidence(other, &other.natural_interval())? {
                    coin_with_section = true;
                    break;
                }
            }

            // skip section curves from embedded edges on planar faces
            let mut coin_with_embedded = false;
            if !coin_with_section && is_planar {
                // for every face->Embedded edge
                for e in status_free(face4::edges(b, f, None, false, None), Vec::new())? {
                    // skip nonEmbedded edges
                    if !status_free(edge::is_embedded(k, b, e, Some(f), true), false)? {
                        continue;
                    }

                    // check for coincidence
                    let face_curve =
                        status_free(edge::curve(b, e).map(Option::<&Curve>::cloned), None)?
                            .ok_or_else(|| {
                                SolidError::topology_missing(OP_SECTION, "the edge's curve")
                            })?;
                    let edge_ivl = status_free(edge::interval(b, e), Extent1d::new())?;
                    if is_full_coincidence(&face_curve, &edge_ivl)? {
                        coin_with_embedded = true;
                        break;
                    }
                }
            }

            if coin_with_section || coin_with_embedded {
                // remove the curve from the solution array, and move on to the next
                // solution (at the same index)
                curves_3d.remove(index);
                uv_curves.remove(index);
                continue;
            }

            // arrive here after culling duplicate and embedded solutions and skipping
            // degenerate solutions; next make sure 3DCurves have UVCurves
            if opts.force_uv_classify
                && opts.outputs.uv_curves
                && uv_curves.get(index).is_some_and(Option::is_none)
            {
                force_uv_curves(
                    k,
                    b,
                    f,
                    &surface,
                    &uv_domain,
                    face_tol,
                    index,
                    &orig_3d,
                    &mut curves_3d,
                    &mut uv_curves,
                )?;
            }
            index += 1;
        }

        // Trim curves to face (Calls SmFace::CurveOnClassify())
        let to_trim_3d: Vec<Option<Curve>> = curves_3d.into_iter().map(Some).collect();
        let trimmed = match face2::create_curves_by_trimming(
            k,
            b,
            f,
            opts.approx_tol_3d,
            Some(&to_trim_3d),
            opts.outputs.uv_curves.then_some(uv_curves.as_slice()),
            true,
            true,
        ) {
            Ok(t) => t,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => continue,
        };
        let trim_3d = trimmed.curves_3d.unwrap_or_default();
        let trim_uv = trimmed.uv_curves.unwrap_or_default();
        // Trimmed in 3D, a section curve that covers itself gives each of its covers of the
        // face's pieces (see [`distinct_pieces`]): keep one of each.
        let trim_3d = if opts.outputs.uv_curves {
            trim_3d
        } else {
            distinct_pieces(k, trim_3d, tol)?
        };

        // add results to output
        if opts.outputs.faces {
            if trim_uv.is_empty() {
                // [B179] If pOptFaces but no UV curves, save faces associated with 3d
                // curves.
                out_faces.extend(std::iter::repeat_n(f, trim_3d.len()));
            } else {
                // save the face associated with each section UVcurve
                out_faces.extend(std::iter::repeat_n(f, trim_uv.len()));
            }
        }
        if opts.outputs.curves_3d {
            section_3d.extend(trim_3d);
        }
        if opts.outputs.uv_curves {
            out_uv.extend(trim_uv);
        }
    }

    Ok(PlanarSection {
        curves_3d: opts.outputs.curves_3d.then_some(section_3d),
        uv_curves: opts.outputs.uv_curves.then_some(out_uv),
        faces: opts.outputs.faces.then_some(out_faces),
    })
}

/// Whether all of `curve` lies on `other` within `tol`: the coincidence check of `curve`
/// against `other`, read by [`covers`].
///
/// # Errors
///
/// Those of [`covers`].
fn lies_on(k: &Kernel, curve: &Curve, other: &Curve, tol: f64) -> Result<bool> {
    let (found, status) = curve.global_coincidence_checker_with_partial::<NoObject>(
        k,
        &curve.natural_interval(),
        other,
        &other.natural_interval(),
        tol,
        true,
        [None, None],
    );
    covers(k, curve, &found.solutions, status, tol)
}

/// Whether the solutions of a coincidence check of `curve` (`status` its result) cover it
/// within `tol`: one of their ranges of values runs from `curve`'s start to its end. A
/// range's end counts as the curve's end only where the curve's point at it is within `tol`
/// of the curve's own end point, so a range that stops short by more than `tol` does not
/// cover the curve. Other solutions, such as the touch a closed curve reports at its closing
/// point, do not count against it. The check keeps the solutions it found before an error,
/// so a failed check covers the curve when one of them does.
///
/// # Errors
///
/// [`SolidError::Unported`] and cancellation from the check, whatever it found; any other
/// error of the check when none of its ranges covers the curve; those of the curve's
/// evaluations and of the solutions' parameters.
fn covers(
    k: &Kernel,
    curve: &Curve,
    solutions: &[Solution<NoObject>],
    status: Result<()>,
    tol: f64,
) -> Result<bool> {
    let status = match status {
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        status => status,
    };
    let ivl = curve.natural_interval();
    let param_tol = 1.0 / 100.0 * (EFF_ZERO_SQRT + ivl.length());
    let (start, end) = (
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    );
    for s in solutions {
        if s.solution_type != SolutionType::RangeOfValues {
            continue;
        }
        let (t0, t1) = (s.start.param(0)?, s.end.param(0)?);
        if ivl.classify_point(t0, param_tol) == ExtentPointType::Start
            && ivl.classify_point(t1, param_tol) == ExtentPointType::End
            && curve.evaluate_point(k, t0)?.distance(start) <= tol
            && curve.evaluate_point(k, t1)?.distance(end) <= tol
        {
            return Ok(true);
        }
    }
    status.map(|()| false)
}

/// The pieces of one face's section, trimmed in 3D, with every piece that lies wholly on
/// another dropped. The pieces are taken longest first (of equal lengths, in their order),
/// and each is tested only against the pieces already kept, so every piece dropped lies
/// within `tol` of a piece that is kept; the kept pieces keep their order.
///
/// The surface section is traced over the face's whole UV domain, and a surface can cover
/// itself there: a spline that winds round a circle several times (as a coil's or a
/// thread's NURBS does) is cut by the plane in a curve that runs round the same circle as
/// often. Trimming in 3D classifies points, not parameters, so each cover of the face's
/// piece is kept; and the cover the curve ends in, at the surface's boundary, is kept up
/// to that end, which lies inside the face. Every piece the trimming keeps is a part of
/// the face's section, so the pieces that lie on another are those covers.
///
/// # Errors
///
/// Those of the pieces' lengths and of [`lies_on`].
fn distinct_pieces(k: &Kernel, pieces: Vec<Curve>, tol: f64) -> Result<Vec<Curve>> {
    if pieces.len() < 2 {
        return Ok(pieces);
    }
    let mut longest_first = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        longest_first.push((piece.length(k, &piece.natural_interval(), tol)?, i));
    }
    longest_first.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut kept: Vec<usize> = Vec::new();
    for (_, i) in longest_first {
        let Some(piece) = pieces.get(i) else {
            continue;
        };
        let mut on_kept = false;
        for &j in &kept {
            if let Some(other) = pieces.get(j) {
                if lies_on(k, piece, other, tol)? {
                    on_kept = true;
                    break;
                }
            }
        }
        if !on_kept {
            kept.push(i);
        }
    }
    Ok(pieces
        .into_iter()
        .enumerate()
        .filter_map(|(i, piece)| kept.contains(&i).then_some(piece))
        .collect())
}

/// The forced UV curves of section curve `index` (`bForceUVClassify`): the curve is
/// dropped onto the face's surface (`DropAndTrimCurve`); each drop curve whose surface
/// point near its middle drops back onto the original 3D curve and whose lift stays
/// within ten face tolerances of the surface gives a UV curve and the lift as its 3D
/// curve: the first drop curve's replaces the entry at `index`, later ones are added.
fn force_uv_curves(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    surface: &Surface,
    uv_domain: &Extent2d,
    face_tol: f64,
    index: usize,
    orig_3d: &Curve,
    curves_3d: &mut Vec<Curve>,
    uv_curves: &mut Vec<Option<Curve>>,
) -> Result<()> {
    let handle = Some(CacheObject::Surface(f));
    let mut services = BrepFaceServices::new(&mut *b);

    // create a 2d Parameter Curve from the 3D Intersection Curve (the trace runs on the
    // face's cache, `smsurf_GetSurfaceCache` inside `DoTrace`, held from the context)
    let mut cache = services.build_surface_cache(k, surface, handle)?;
    let drops = surface
        .drop_and_trim_curve_in_context::<NoObject, CacheObject>(
            k,
            Some(&mut cache),
            uv_domain,
            orig_3d,
            &orig_3d.natural_interval(),
            face_tol / 3.0,
            true,
            true,
            None,
            Some(DropSolveContext {
                solver: &mut services,
                handle,
            }),
        )?
        .uv_curves;

    // for every 2d Parameter Curve generated
    for (kk, drop) in drops.into_iter().enumerate() {
        // Test near mid point to see if it looks good before lifting.
        let drop_ivl = drop.natural_interval();
        let p = drop.evaluate_point(k, drop_ivl.evaluate(0.824))?;
        let uv = Point2::new(p.x, p.y);

        // let sPnt = Surface(pDrop(nearMidParam))
        let pnt = surface.evaluate_point(k, uv)?;

        // drop sPnt to 3D Intersection curve to prove the DropCurve is good
        let dropped = orig_3d.drop_point(
            k,
            &orig_3d.natural_interval(),
            pnt,
            None,
            face_tol * 100.0,
            None,
            SolverOperation::Minimize,
        )?;
        if !dropped.success {
            continue;
        }

        // skip UVCurves whose 3D Approximations exceed tolerance
        let lifted = surface.lift_curve(k, uv_domain, &drop, &drop_ivl, face_tol / 3.0, true)?;
        if lifted.max_approx_dev > face_tol * 10.0 {
            continue;
        }

        // for the 1st DropAndTrim Curve result replace current curve with 3D
        // Approximation and UVCurve in outputs; for subsequent DropAndTrim results add
        // the 3D Approximation curve and UVCurve to outputs
        if kk == 0 {
            if let Some(slot) = uv_curves.get_mut(index) {
                *slot = Some(drop);
            }
            if let Some(slot) = curves_3d.get_mut(index) {
                *slot = lifted.curve;
            }
        } else {
            curves_3d.push(lifted.curve);
            uv_curves.push(Some(drop));
        }
    }
    Ok(())
}

/// The value of an upstream getter that reports no status (`IsPlanar`, `IsDegenerate`,
/// `IsEmbedded`, `GetEdges`, `GetCurve`, `GetInterval`): [`SolidError::Unported`] and
/// [`SolidError::NotFound`] (a dangling id) propagate, any other failure reads as
/// `fallback`.
fn status_free<T>(r: Result<T>, fallback: T) -> Result<T> {
    match r {
        Ok(v) => Ok(v),
        Err(
            e @ (SolidError::Unported { .. }
            | SolidError::Cancelled { .. }
            | SolidError::NotFound { .. }),
        ) => Err(e),
        Err(_) => Ok(fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_10(k: &mut Kernel) -> Brep {
        crate::primitive::api_primitives::create_box(k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap()
    }

    #[test]
    fn plane_from_bbox_misses_a_far_box() {
        let k = Kernel::default();
        let bbox = Extent3d::from_points(Point3::ZERO, Point3::new(1.0, 1.0, 1.0));
        let far = create_plane_from_bbox(&k, &bbox, Point3::new(0.0, 0.0, 5.0), Vec3::Z, 0.0);
        assert!(far.unwrap().is_none());
        let through = create_plane_from_bbox(&k, &bbox, Point3::new(0.0, 0.0, 0.5), Vec3::Z, 0.0);
        assert!(through.unwrap().is_some());
        assert!(create_plane_from_bbox(&k, &bbox, Point3::ZERO, Vec3::ZERO, 0.0).is_err());
    }

    #[test]
    fn box_section_is_four_lines_ten_long() {
        let mut k = Kernel::default();
        let mut b = box_10(&mut k);
        let opts = PlanarSectionOptions {
            outputs: SectionOutputs {
                curves_3d: true,
                faces: true,
                ..SectionOutputs::default()
            },
            ..PlanarSectionOptions::default()
        };
        let r =
            create_planar_section_curves(&k, &mut b, Point3::new(5.0, 5.0, 5.0), Vec3::X, &opts)
                .unwrap();
        let curves = r.curves_3d.unwrap();
        assert_eq!(curves.len(), 4);
        assert_eq!(r.faces.unwrap().len(), 4);
        for c in &curves {
            let ivl = c.natural_interval();
            let a = c.evaluate_point(&k, ivl.min()).unwrap();
            let z = c.evaluate_point(&k, ivl.max()).unwrap();
            assert!((a.x - 5.0).abs() < 1e-9 && (z.x - 5.0).abs() < 1e-9);
            assert!((a.distance(z) - 10.0).abs() < 1e-9);
        }
        assert!(r.uv_curves.is_none());
    }

    /// A cylinder of radius `r` and height `h` whose rational NURBS winds round its circle
    /// `spans / 4` times (`spans` quarter arcs, one per knot span, u in [0, spans]; v in
    /// [0, 1] is the height), as a coil's or a thread's NURBS can.
    fn wound_cylinder(r: f64, h: f64, spans: usize) -> Surface {
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_4};
        let net = (0..=2 * spans)
            .map(|i| {
                // corners of the arcs (w = 1) and the corners of their squares (w = 1/sqrt 2)
                let (radius, w) = if i % 2 == 0 {
                    (r, 1.0)
                } else {
                    (r * 2.0_f64.sqrt(), FRAC_1_SQRT_2)
                };
                let (sin, cos) = (i as f64 * FRAC_PI_4).sin_cos();
                [0.0, h]
                    .map(|z| CPoint::new(w * radius * cos, w * radius * sin, w * z, w))
                    .to_vec()
            })
            .collect();
        let mut u = vec![0.0; 3];
        for knot in 1..spans {
            u.extend([knot as f64; 2]);
        }
        u.extend([spans as f64; 3]);
        let nurb = NurbSurface::new(net, 2, 1, u, vec![0.0, 0.0, 1.0, 1.0]).unwrap();
        Surface::from(BSplineSurface::from_nurb(nurb, false))
    }

    /// One face on [`wound_cylinder`] of radius `r`, height `h` and nine quarter arcs (2 1/4
    /// turns, u in [0, 9]), bounded in UV by the quadrilateral through `corners`
    /// (counter-clockwise), over the surface's whole UV domain, so that a section of the face
    /// is traced over all of the surface's turns.
    fn wound_face(k: &mut Kernel, r: f64, h: f64, corners: [(f64, f64); 4]) -> (Brep, FaceId) {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        use crate::topology::brep::p2::ArenaNotifyState;
        use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
        let surface = wound_cylinder(r, h, 9);
        let domain = surface.natural_uv_domain_with_nurb(k).unwrap();
        let uv_curves: Vec<Curve> = (0..4)
            .map(|i| {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                let line = BSplineCurve::create_line_segment(
                    2,
                    Point3::new(a.0, a.1, 0.0),
                    Point3::new(b.0, b.1, 0.0),
                )
                .unwrap();
                Curve::from(line)
            })
            .collect();
        let mut b = crate::topology::brep::p3::new_brep(
            k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )
        .unwrap();
        let mut state = ArenaNotifyState::new(&b);
        let mut env = state.env(None);
        let key_of = env.key_of;
        let this_face = |f: FaceId| key_of(crate::EntityRef::Face(f));
        let region = b.infinite_region();
        let input = FaceCurvesInput {
            loops: vec![4],
            curves_3d: None,
            uv_curves: Some(uv_curves),
            orientations: vec![OrientType::Same; 4],
            loop_points: Vec::new(),
        };
        let made = make_face_with_curves(
            k,
            &mut b,
            region,
            input,
            surface,
            &domain,
            OrientType::Same,
            None,
            &this_face,
            &mut env,
        )
        .unwrap();
        b.refresh_cached_topology();
        let f = made.face.unwrap();
        assert!(face2::uv_domain(&b, f).unwrap().are_equal(&domain, 1e-12));
        (b, f)
    }

    /// The section of `b` by the plane z = `z`: its curves' total length, and every curve
    /// end's distance to the nearest of `ends`, after checking that every curve stays on the
    /// circle of radius `r` about the z axis in that plane within the face's tolerance `tol`.
    fn section_on_circle(
        k: &Kernel,
        b: &mut Brep,
        r: f64,
        z: f64,
        tol: f64,
        ends: &[Point3],
    ) -> (usize, f64, Vec<f64>) {
        let opts = PlanarSectionOptions {
            outputs: SectionOutputs {
                curves_3d: true,
                ..SectionOutputs::default()
            },
            ..PlanarSectionOptions::default()
        };
        let curves = create_planar_section_curves(k, b, Point3::new(0.0, 0.0, z), Vec3::Z, &opts)
            .unwrap()
            .curves_3d
            .unwrap();
        let mut length = 0.0;
        let mut off_ends = Vec::new();
        for c in &curves {
            let ivl = c.natural_interval();
            length += c.length(k, &ivl, 1e-9).unwrap();
            for i in 0..=64 {
                let p = c
                    .evaluate_point(k, ivl.evaluate(f64::from(i) / 64.0))
                    .unwrap();
                let off = ((p.x * p.x + p.y * p.y).sqrt() - r)
                    .abs()
                    .max((p.z - z).abs());
                assert!(off <= tol, "a section point {off} off the circle");
            }
            for t in [ivl.min(), ivl.max()] {
                let p = c.evaluate_point(k, t).unwrap();
                off_ends.push(
                    ends.iter()
                        .map(|e| e.distance(p))
                        .fold(f64::INFINITY, f64::min),
                );
            }
        }
        (curves.len(), length, off_ends)
    }

    /// A face on a spline that winds round its circle (as a coil's or a thread's NURBS can)
    /// is cut in its own section once. The surface's section runs round the circle 2 1/4
    /// times; trimmed in 3D each turn kept its own copy of the face's piece, and the last,
    /// partial turn a copy from where the surface ends (at 0 degrees, inside the face) to
    /// the face's side, a loose end.
    ///
    /// - A band one quarter turn wide that climbs two turns, the parallelogram (0, 0),
    ///   (1, 0), (9, 1), (8, 1) in UV (u in [8 v, 8 v + 1] at height v), cut at v = 7/16:
    ///   one quarter arc, u in [3.5, 4.5], from -45 to 45 degrees about the axis, length
    ///   r pi / 2, both ends on the band's sides.
    /// - A ring one turn round, the rectangle u in [4, 8], v in [0, 1] (its sides at u = 4
    ///   and u = 8 meet in 3D along the line at 0 degrees), cut at mid-height: one full
    ///   circle, length 2 pi r, its ends on that line.
    ///
    /// The curves are traced to the face's tolerance, so they are checked to it: every
    /// sample on the circle, the ends at the sides, and the length within 2 a times it for
    /// an arc of angle a (a curve within `tol` of a radius-r arc measures at most (r + tol) a;
    /// twice that).
    #[test]
    fn a_face_on_a_spline_that_winds_round_its_circle_is_cut_once() {
        use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI};
        let mut k = Kernel::default();
        let (r, h) = (2.0, 4.0);
        let band = [(0.0, 0.0), (1.0, 0.0), (9.0, 1.0), (8.0, 1.0)];
        let ring = [(4.0, 0.0), (8.0, 0.0), (8.0, 1.0), (4.0, 1.0)];
        for (corners, v, angle, ends) in [
            (
                band,
                7.0 / 16.0,
                FRAC_PI_2,
                vec![
                    Point3::new(r * FRAC_1_SQRT_2, -r * FRAC_1_SQRT_2, h * 7.0 / 16.0),
                    Point3::new(r * FRAC_1_SQRT_2, r * FRAC_1_SQRT_2, h * 7.0 / 16.0),
                ],
            ),
            (ring, 0.5, 2.0 * PI, vec![Point3::new(r, 0.0, h * 0.5)]),
        ] {
            let (mut b, f) = wound_face(&mut k, r, h, corners);
            let tol = face2::tolerance(&b, f).unwrap();
            let (count, length, off_ends) = section_on_circle(&k, &mut b, r, h * v, tol, &ends);
            assert!(
                (length - r * angle).abs() <= 2.0 * angle * tol,
                "{corners:?}: {count} curves, length {length} for {}, tolerance {tol}",
                r * angle
            );
            assert!(
                off_ends.iter().all(|d| *d <= tol),
                "{corners:?}: ends off the face's sides: {off_ends:?}"
            );
            assert_eq!(count, 1, "{corners:?}: one curve");
        }
    }

    /// The arc of radius `r` about the z axis in z = 0 from angle 0 to `angle` (under pi),
    /// as one rational quadratic span on [0, 1].
    fn arc(r: f64, angle: f64) -> Curve {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::CPoint;
        let half = angle / 2.0;
        let w = half.cos();
        let points = vec![
            CPoint::new(r, 0.0, 0.0, 1.0),
            CPoint::new(w * r, w * r * half.tan(), 0.0, w),
            CPoint::new(r * angle.cos(), r * angle.sin(), 0.0, 1.0),
        ];
        let nurb = NurbCurve::new(points, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
        Curve::from(BSplineCurve::from_nurb(nurb, 3, false))
    }

    fn lengths(k: &Kernel, curves: &[Curve]) -> Vec<f64> {
        curves
            .iter()
            .map(|c| c.length(k, &c.natural_interval(), 1e-12).unwrap())
            .collect()
    }

    /// A piece that stops short of another's end by more than the tolerance does not cover
    /// it, however small a part of its interval that is: of an arc of radius 10 and angle 1
    /// and the same arc 0.5% shorter (0.05 short, against a tolerance of 1e-5), the shorter
    /// lies on the longer, the longer not on the shorter, and with the shorter first, the
    /// longer is the piece kept.
    #[test]
    fn a_piece_a_little_longer_than_another_is_kept() {
        let k = Kernel::default();
        let tol = 1e-5;
        let (long, short) = (arc(10.0, 1.0), arc(10.0, 0.995));
        assert!(lies_on(&k, &short, &long, tol).unwrap());
        assert!(!lies_on(&k, &long, &short, tol).unwrap());
        let kept = distinct_pieces(&k, vec![short, long], tol).unwrap();
        let kept = lengths(&k, &kept);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!((kept[0] - 10.0).abs() < 1e-9, "{kept:?}");
    }

    /// A piece is dropped only for lying on a piece that is kept. Of three arcs of angle 1 at
    /// radii 10, 10 + 0.6 tol and 10 + 1.2 tol, each lies on its neighbour but the outer
    /// not on the inner: the outer, longest, is kept; the middle lies on it and is dropped;
    /// the inner does not and is kept. Tested against every piece, the outer was dropped
    /// for lying on the middle one, dropped itself, and nothing kept was within `tol` of it.
    #[test]
    fn a_piece_is_dropped_only_for_lying_on_a_kept_one() {
        let k = Kernel::default();
        let tol = 1e-5;
        let (inner, middle, outer) = (
            arc(10.0, 1.0),
            arc(10.0 + 0.6 * tol, 1.0),
            arc(10.0 + 1.2 * tol, 1.0),
        );
        assert!(lies_on(&k, &middle, &inner, tol).unwrap());
        assert!(lies_on(&k, &outer, &middle, tol).unwrap());
        assert!(!lies_on(&k, &outer, &inner, tol).unwrap());
        let pieces = vec![inner.clone(), middle, outer.clone()];
        let kept = distinct_pieces(&k, pieces, tol).unwrap();
        assert_eq!(
            lengths(&k, &kept),
            lengths(&k, &[inner, outer]),
            "the inner and the outer arc, in their order"
        );
    }

    /// Exact copies keep the first, in its place among the others: of two copies each of an
    /// arc of radius 20 and angle 1/4 (length 5) and one of radius 10 and angle 1 (length
    /// 10), in turn, the first of each is kept, in that order.
    #[test]
    fn of_exact_copies_the_first_is_kept() {
        let k = Kernel::default();
        let tol = 1e-5;
        let (a, b) = (arc(10.0, 1.0), arc(20.0, 0.25));
        let kept = distinct_pieces(&k, vec![b.clone(), a.clone(), b, a], tol).unwrap();
        let got = lengths(&k, &kept);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(
            (got[0] - 5.0).abs() < 1e-9 && (got[1] - 10.0).abs() < 1e-9,
            "{got:?}"
        );
    }

    /// The range of values of a coincidence check from `t0` to `t1` on the checked curve.
    fn range(t0: f64, t1: f64) -> Solution<NoObject> {
        let mut s = Solution::new();
        s.solution_type = SolutionType::RangeOfValues;
        s.num_variables = 1;
        s.start.parameters[0] = t0;
        s.end.parameters[0] = t1;
        s
    }

    /// How a coincidence check's result is read, on an arc of radius 10 and angle 1 (on
    /// [0, 1]): a range over all of it covers it, also when the check then failed (the check
    /// keeps what it found before an error); with no such range a failed check passes its
    /// error on instead of reading as "not covered"; and an unported or cancelled check
    /// passes its error on whatever it found.
    #[test]
    fn a_failed_coincidence_check_is_not_read_as_not_covered() {
        let k = Kernel::default();
        let tol = 1e-5;
        let piece = arc(10.0, 1.0);
        let failed = || Err(SolidError::invalid_input("test", "the check failed"));
        let unported = || Err(SolidError::unported("test", "test"));
        let whole = [range(0.0, 1.0)];
        assert!(covers(&k, &piece, &whole, Ok(()), tol).unwrap());
        assert!(!covers(&k, &piece, &[], Ok(()), tol).unwrap());
        assert!(covers(&k, &piece, &whole, failed(), tol).unwrap());
        assert!(covers(&k, &piece, &[], failed(), tol).is_err());
        assert!(covers(&k, &piece, &[range(0.0, 0.5)], failed(), tol).is_err());
        assert!(covers(&k, &piece, &whole, unported(), tol).is_err());
        assert!(covers(&k, &piece, &[], unported(), tol).is_err());
    }
}
