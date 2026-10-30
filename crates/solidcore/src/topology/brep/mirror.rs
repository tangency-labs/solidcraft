//! `SmBrep::Mirror`: mirrors a B-rep about the XY plane of an axis placement.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp` (`SmBrep::Mirror`, `sm_ReverseReflectionEdgeuses`,
//! `sm_SwapReflectionShells`), `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust                                            | Upstream                                   |
//! |-------------------------------------------------|--------------------------------------------|
//! | [`mirror`], [`mirror_with_notifications`]       | `SmBrep::Mirror`                           |
//! | [`transform_matrix`], [`transform_matrix_with_notifications`] | `SmBrep::Transform(const double[4][4], SmBoolean)` |
//! | `fold_status` (private)                         | `eStat = std::max(..., eStat)`             |
//! | `Axis2Placement::mirror_point`                  | `SmAxis2Placement::MirrorPoint`            |
//! | `Axis2Placement::mirror_vector`                 | `SmAxis2Placement::MirrorVector`           |
//! | `Curve::create_mirror_curve`                    | `SmCurve::CreateMirrorCurve` (virtual)     |
//! | `Surface::create_mirror_surface`                | `SmSurface::CreateMirrorSurface` (virtual) |
//! | `BSplineSurface::create_mirror_surface`         | `SmBSplineSurface::CreateMirrorSurface`    |
//! | `OffsetSurface::create_mirror_surface`          | `SmOffsetSurface::CreateMirrorSurface`     |
//! | `CrvOnSurf::create_mirror_curve`                | `SmCrvOnSurf::CreateMirrorCurve`           |
//! | `CrvInVolume::create_mirror_curve`              | `SmCrvInVolume::CreateMirrorCurve`         |
//!
//! (`BSplineCurve::create_mirror_curve`, `SrfInVolume::create_mirror_surface`,
//! `topology::brep::p1::reverse_reflection_edgeuses` and `swap_reflection_shells` were
//! ported before this unit.)
//!
//! Deviations: the steps run in upstream's order with its statuses: the editing
//! notifications of the vertices, edges and faces (`Notify` statuses ignored), then
//! `SER` on every curve and surface creation, `CopyAndAddAnalytics`, `ReplaceSurface`,
//! `RemoveUVTrimCurves` and `CreateUVTrimCurves`. An edge's old curve and a face's
//! temporary mirror surface are dropped where upstream deletes them. A curve or surface
//! wrapping a volume (`SmCrvInVolume`, `SmSrfInVolume`) mirrors its own copy of the
//! volume (parts are held by value), where upstream also edits a volume the object does
//! not own. Composite edges do not exist in the arena model (`sEdges` is the edge
//! alone). The NURBs of analytic surfaces without one are made before the infinite region
//! search (`make_missing_nurbs`), where upstream's bounding box makes them lazily. The
//! debug drawing code is omitted. No recursion is introduced here.
//!
//! [`transform_matrix`] runs upstream's steps and branches in order: the low-work
//! test, `SVDecompose` (status ignored upstream; a failure here is `SM_ERR` without
//! touching the B-rep, where upstream reads the arrays the failed call left), then
//! the `Transform` / `Mirror` sequence, every status folded with `std::max` (the error
//! with the largest code is returned; one without an exact status counts as `SM_ERR`).
//! A failed `AngleBetween` reads as angle `0`, as upstream sets it.

use crate::error::{ignore_status, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::matrix::Matrix;
use crate::kernel::Kernel;
use crate::math::{Vec3, EFF_ZERO, EFF_ZERO_RAD, HALF_PI};
use crate::query::brep::BrepQuery;
use crate::surface::surface::p2::SurfaceKind;
use crate::topology::brep::p1::{
    curve_owners, reverse_reflection_edgeuses, surface_owners, swap_reflection_shells,
    SetRegionVoidType,
};
use crate::topology::brep::p5::{
    brep_faces, brep_regions, brep_vertices, find_and_set_infinite_region, notify_edge_pre_edit,
    notify_face_pre_edit, notify_pre_edit, notify_vertex_pre_edit, plain_edit, replace_surface,
    set_edge_curve_owner, set_joined_edge_curve, set_vertex_point, transform_owner_key,
    transform_with_notifications, with_notifications, EditNotifyEnv,
};
use crate::topology::Brep;
use crate::EntityRef;

const OP: &str = "topology::brep::mirror::mirror";

/// `SmBrep::Mirror(crMirrorPlane)`: mirrors every vertex point, edge curve and face
/// surface about the XY plane of `mirror_plane`, reverses the radial order of the edge
/// uses and swaps the shells of the face uses (a reflection turns the B-rep inside out),
/// rebuilds the UV trim curves and, with more than one region, finds the infinite
/// region again.
///
/// # Errors
///
/// `SM_ERR` for an edge curve or face surface of a kind without `CreateMirrorCurve` /
/// `CreateMirrorSurface` (see [`crate::curve::curve::Curve::create_mirror_curve`],
/// [`crate::surface::surface::Surface::create_mirror_surface`]); `InvalidInput` where
/// upstream dereferences null (an edge without curve, a face without surface or
/// face uses); those of the callees.
pub fn mirror(k: &mut Kernel, b: &mut Brep, mirror_plane: &Axis2Placement) -> Result<()> {
    mirror_with_notifications(k, b, mirror_plane, None)
}

/// [`mirror`] notifying through the caller's environment `notifications` (a plain edit
/// under the kernel's identities when `None`).
///
/// # Errors
///
/// Those of [`mirror`].
pub fn mirror_with_notifications(
    k: &mut Kernel,
    b: &mut Brep,
    mirror_plane: &Axis2Placement,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    if let Some(env) = notifications {
        mirror_in(k, b, mirror_plane, env)
    } else {
        plain_edit(k, b, |k, b, env| mirror_in(k, b, mirror_plane, env))
    }
}

fn null() -> SolidError {
    SolidError::null_pointer(OP)
}

fn mirror_in(
    k: &mut Kernel,
    b: &mut Brep,
    mirror_plane: &Axis2Placement,
    env: &mut EditNotifyEnv<'_>,
) -> Result<()> {
    let owner_keys = env.key_of;
    let mut notifications = Some(env);

    // notify Brep of editing operation
    ignore_status(notify_pre_edit(k, b, notifications.as_deref_mut()))?;

    // mirror every vertex->Point
    for vertex in brep_vertices(b)? {
        let point = b.vertex(vertex)?.point;
        ignore_status(notify_vertex_pre_edit(
            k,
            b,
            vertex,
            notifications.as_deref_mut(),
        ))?;
        set_vertex_point(
            k,
            b,
            vertex,
            mirror_plane.mirror_point(point),
            notifications.as_deref_mut(),
        )?;
    }

    // mirror every edge - reverse radial orderings: for every Brep->Curve
    for edge in curve_owners(b, None, &|e| {
        transform_owner_key(b, EntityRef::Edge(e), owner_keys(EntityRef::Edge(e)))
    })? {
        ignore_status(notify_edge_pre_edit(
            k,
            b,
            edge,
            notifications.as_deref_mut(),
        ))?;
        let new_curve = b
            .edge(edge)?
            .curve
            .as_ref()
            .ok_or_else(null)?
            .create_mirror_curve(k, mirror_plane)?;
        // SetCurve(pNewCurve, FALSE, FALSE) (status ignored), then SetOwner(pEdge); the
        // old curve is dropped here (upstream deletes it after the edge use reversal)
        set_joined_edge_curve(
            k,
            b,
            edge,
            Some(new_curve),
            false,
            notifications.as_deref_mut(),
        )?;
        set_edge_curve_owner(b, edge, notifications.as_deref_mut())?;
        reverse_reflection_edgeuses(b, edge)?;
    }

    // mirror every face: for every Brep->Surface
    for face in surface_owners(b, None, &|f| {
        transform_owner_key(b, EntityRef::Face(f), owner_keys(EntityRef::Face(f)))
    })? {
        ignore_status(notify_face_pre_edit(
            k,
            b,
            face,
            notifications.as_deref_mut(),
        ))?;
        let tmp_surface = b
            .face(face)?
            .surface
            .as_ref()
            .ok_or_else(null)?
            .create_mirror_surface(k, mirror_plane)?;

        // Copy pTmpSurface for new face, when possible as an analytic surface
        let new_surface = tmp_surface.copy_and_add_analytics(k)?;
        drop(tmp_surface);
        let mut modified = Vec::new();
        replace_surface(
            k,
            b,
            face,
            new_surface,
            false,
            Some(&mut modified),
            true,
            notifications.as_deref_mut(),
        )?;

        // for every Face using pSurface - remove UVTrimCurves, swap Faceuse shells
        for f in modified {
            with_notifications(b, notifications.as_deref_mut(), None, |b, env, _| {
                crate::topology::face::p1::remove_uv_trim_curves_with_notifications(b, f, env)
            })?;
            swap_reflection_shells(b, f)?;
        }
    }

    // Brep->Faces - rebuild UVTrimCurves
    for f in brep_faces(b)? {
        with_notifications(b, notifications.as_deref_mut(), None, |b, env, _| {
            crate::topology::face::p3::create_uv_trim_curves_notified(
                k,
                b,
                f,
                true,
                None,
                Some(env),
            )
            .map(|_| ())
        })?;
    }

    // Brep->Regions - FindAndSetInfiniteRegion
    if brep_regions(b)?.len() > 1 {
        make_missing_nurbs(k, b)?;
        // SmTemporaryChangeValue<SmBoolean> sChange(m_bEditingEnabled, FALSE)
        let previous = b.editing_enabled();
        b.set_editing_enabled(false);
        let query = BrepQuery::of_brep(b);
        let result = find_and_set_infinite_region(
            k,
            b,
            &query,
            None,
            SetRegionVoidType::InfiniteRegionSwap,
            None,
        );
        b.set_editing_enabled(previous);
        result?;
    }
    Ok(())
}

/// Makes the NURB of every analytic face surface that has none. Upstream's first
/// `SmBSplineSurface::CalculateBoundingBox` makes it through the virtual
/// `GetOrCreateGwNurbPointer` (`FindAndSetInfiniteRegion` calls it); the port's bounding
/// box is a `const` method that returns `SM_ERR` instead (finding
/// `query.brep--boolean-results-need-their-analytic-nurbs`), so the NURBs are made here,
/// where upstream makes them, just before that call.
fn make_missing_nurbs(k: &Kernel, b: &mut Brep) -> Result<()> {
    for f in brep_faces(b)? {
        let Some(surface) = b.face_mut(f)?.surface.as_mut() else {
            continue;
        };
        match surface.kind_mut() {
            SurfaceKind::Plane(s) if s.base().gw_nurb().is_none() => s.make_nurb()?,
            SurfaceKind::Cone(s) if s.rev().base().gw_nurb().is_none() => s.make_nurb(k)?,
            SurfaceKind::Sphere(s) if s.base().base().gw_nurb().is_none() => s.make_nurb(k)?,
            SurfaceKind::Torus(s) if s.base().base().gw_nurb().is_none() => s.make_nurb(k)?,
            SurfaceKind::SurfOfRevolution(s) if s.base().gw_nurb().is_none() => s.make_nurb(k)?,
            SurfaceKind::SurfOfExtrusion(s) if s.base().gw_nurb().is_none() => s.make_nurb(k)?,
            _ => {}
        }
    }
    Ok(())
}

/// Keeps the status of a step whose `SmStatus` upstream folds in with `std::max`:
/// `worst` holds the error with the largest status code so far (the first one on a
/// tie). Unported dependencies and preparation requests still propagate.
fn fold_status(worst: &mut Option<SolidError>, r: Result<()>) -> Result<()> {
    let Err(e) = r else { return Ok(()) };
    if e.is_unported() || matches!(e, SolidError::PreparationRequired { .. }) {
        return Err(e);
    }
    let code = |e: &SolidError| e.sm_status().unwrap_or(crate::error::SM_ERR);
    if worst.as_ref().is_none_or(|w| code(&e) > code(w)) {
        *worst = Some(e);
    }
    Ok(())
}

/// `SmBrep::Transform(const double[4][4], bScaleTol)`: applies the affine map
/// `p' = p·A + t` to a B-rep, where `A` is the upper-left 3x3 block of `matrix`
/// (rows `matrix[0..3]`, columns `0..3`) and `t` its fourth row (row-vector
/// convention). The fourth column is ignored.
///
/// Without shear or reflection (rows 0 and 1 perpendicular within `SM_EFF_ZERO` and
/// row 0 × row 1 along row 2 within `SM_EFF_ZERO_RAD`) this is one
/// [`transform`](crate::topology::brep::p5::transform) by the unit rows scaled by
/// their lengths. Otherwise `A = V D Uᵀ` by [`Matrix::sv_decompose`] and the B-rep is
/// transformed by `V` (scaled by its row lengths), mirrored when `V` is left-handed,
/// then transformed by `Uᵀ` scaled by `D`, mirrored and translated when `Uᵀ` is
/// left-handed. A non-uniform scale turns every geometry into NURBS. `scale_tolerances`
/// is passed to every scaling transform.
///
/// # Errors
///
/// As upstream, every step runs even when an earlier one fails, and the error with
/// the largest upstream status is returned; those of
/// [`transform`](crate::topology::brep::p5::transform), [`mirror`] and
/// [`Matrix::sv_decompose`] (whose status upstream ignores).
pub fn transform_matrix(
    k: &mut Kernel,
    b: &mut Brep,
    matrix: &[[f64; 4]; 4],
    scale_tolerances: bool,
) -> Result<()> {
    transform_matrix_with_notifications(k, b, matrix, scale_tolerances, None)
}

/// [`transform_matrix`] notifying every step through the caller's environment
/// `notifications` (each step is its own plain edit under the kernel's identities when
/// `None`, as upstream's separate `Transform` and `Mirror` calls).
///
/// # Errors
///
/// Those of [`transform_matrix`].
pub fn transform_matrix_with_notifications(
    k: &mut Kernel,
    b: &mut Brep,
    matrix: &[[f64; 4]; 4],
    scale_tolerances: bool,
    mut notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    const OP: &str = "topology::brep::mirror::transform_matrix";
    let [m0, m1, m2, m3] = matrix;
    // Setup 3x3 for SVD decomposition
    let row0 = Vec3::new(m0[0], m0[1], m0[2]);
    let row1 = Vec3::new(m1[0], m1[1], m1[2]);
    let row2 = Vec3::new(m2[0], m2[1], m2[2]);

    // Set up displacement
    let disp = Vec3::new(m3[0], m3[1], m3[2]);

    // Low work case: No sheer or reflection?
    if row0.dot(row1).abs() < EFF_ZERO {
        // a failed AngleBetween reports 0
        let angle_check = row0.cross(row1).angle_between(row2).unwrap_or(0.0);

        if angle_check <= EFF_ZERO_RAD {
            let row0_len = row0.length();
            let row1_len = row1.length();
            let row2_len = row2.length();

            let input_scale = Vec3::new(row0_len, row1_len, row2_len);
            let input_transform = Axis2Placement::new(disp, row0 / row0_len, row1 / row1_len);

            // Use SmBrep::Transform with no shear/reflection
            return transform_with_notifications(
                k,
                b,
                &input_transform,
                Some(input_scale),
                scale_tolerances,
                notifications,
            );
        }
    }

    let mut sv = Matrix::from_row_major(
        3,
        3,
        vec![
            row0.x, row0.y, row0.z, row1.x, row1.y, row1.z, row2.x, row2.y, row2.z,
        ],
    )?;

    // Decompose matrix (upstream ignores the status; `sU` and `sDDiag` then hold
    // whatever the failed call left)
    let (d_diag, mut ut) = match sv.sv_decompose() {
        Ok(r) => r,
        Err(e) if e.is_unported() => return Err(e),
        Err(_) => return Err(SolidError::sm_err(OP)),
    };
    ut.transpose()?;
    let row = |m: &Matrix, r: usize| -> Result<Vec3> {
        let v = m
            .row(r)
            .ok_or_else(|| SolidError::port_invariant(OP, "3x3 matrix row"))?;
        match v {
            [x, y, z] => Ok(Vec3::new(*x, *y, *z)),
            _ => Err(SolidError::port_invariant(OP, "3x3 matrix row")),
        }
    };
    let d = |i: usize| -> Result<f64> {
        d_diag
            .get(i)
            .copied()
            .ok_or_else(|| SolidError::port_invariant(OP, "3 singular values"))
    };

    // Build Axis2Placement and scaling vectors for transforms
    let vx = row(&sv, 0)?;
    let vy = row(&sv, 1)?;
    let vz = row(&sv, 2)?;

    let vx_len = vx.length();
    let vy_len = vy.length();
    let vz_len = vz.length();

    let mut transform_v = Axis2Placement::IDENTITY;
    let scale_v = Vec3::new(vx_len, vy_len, vz_len);
    let origin = Vec3::new(0.0, 0.0, 0.0);
    // upstream ignores SetCanonical's status
    let _ = transform_v.set_canonical(origin, vx / vx_len, vy / vy_len);

    let utx = row(&ut, 0)?;
    let uty = row(&ut, 1)?;
    let utz = row(&ut, 2)?;

    let utx_len = utx.length();
    let uty_len = uty.length();
    let utz_len = utz.length();

    let mut transform_ut = Axis2Placement::IDENTITY;
    let scale_ut = Vec3::new(utx_len * d(0)?, uty_len * d(1)?, utz_len * d(2)?);

    // Transforms below are rigid, need to check if we require a mirror to fix the
    // handedness of the orthonormal decomposition.
    let angle_v = vx.cross(vy).angle_between(vz).unwrap_or(0.0);
    let mirror_v = angle_v > HALF_PI;

    let angle_ut = utx.cross(uty).angle_between(utz).unwrap_or(0.0);
    let mirror_ut = angle_ut > HALF_PI;

    let mut worst = None;

    // Transform the brep, applying the mirrors as necessary. Note that the translation
    // comes at the very end.
    fold_status(
        &mut worst,
        transform_with_notifications(
            k,
            b,
            &transform_v,
            Some(scale_v),
            scale_tolerances,
            notifications.as_deref_mut(),
        ),
    )?;
    if mirror_v {
        fold_status(
            &mut worst,
            mirror_with_notifications(k, b, &transform_v, notifications.as_deref_mut()),
        )?;
    }

    if mirror_ut {
        let _ = transform_ut.set_canonical(origin, utx / utx_len, uty / uty_len);
        fold_status(
            &mut worst,
            transform_with_notifications(
                k,
                b,
                &transform_ut,
                Some(scale_ut),
                scale_tolerances,
                notifications.as_deref_mut(),
            ),
        )?;
        fold_status(
            &mut worst,
            mirror_with_notifications(k, b, &transform_ut, notifications.as_deref_mut()),
        )?;

        let v_x = Vec3::new(1.0, 0.0, 0.0);
        let v_y = Vec3::new(0.0, 1.0, 0.0);

        let _ = transform_ut.set_canonical(disp, v_x, v_y);
        // the defaults: no scale, bScaleTol = TRUE
        fold_status(
            &mut worst,
            transform_with_notifications(k, b, &transform_ut, None, true, notifications),
        )?;
    } else {
        // transform without mirror
        let _ = transform_ut.set_canonical(disp, utx / utx_len, uty / uty_len);
        fold_status(
            &mut worst,
            transform_with_notifications(
                k,
                b,
                &transform_ut,
                Some(scale_ut),
                scale_tolerances,
                notifications,
            ),
        )?;
    }

    // all done
    worst.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::create_box;
    use crate::topology::brep::p1;

    #[test]
    fn mirrors_a_box_in_a_plane_through_its_side() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Vec3::new(1.0, 2.0, 3.0), 2.0, 3.0, 4.0).unwrap();
        let vertices = p1::vertices(&b, None).unwrap();
        let before: Vec<Vec3> = vertices
            .iter()
            .map(|&v| b.vertex(v).unwrap().point)
            .collect();
        // the plane x = 2 (normal +X)
        let plane = Axis2Placement::new(Vec3::new(2.0, 0.0, 0.0), Vec3::Y, Vec3::Z);
        mirror(&mut k, &mut b, &plane).unwrap();
        for (&v, p) in vertices.iter().zip(&before) {
            let q = b.vertex(v).unwrap().point;
            assert!((q.x - (4.0 - p.x)).abs() < 1e-12 && q.y == p.y && q.z == p.z);
        }
        // every edge curve follows its vertices
        for e in p1::edges(&b, None).unwrap() {
            let edge = b.edge(e).unwrap();
            let curve = edge.curve.as_ref().unwrap();
            let start = curve.evaluate_point(&k, edge.interval.min()).unwrap();
            let end = curve.evaluate_point(&k, edge.interval.max()).unwrap();
            let on_vertex = |p: Vec3| {
                p1::vertices(&b, None)
                    .unwrap()
                    .iter()
                    .any(|&v| (b.vertex(v).unwrap().point - p).length() < 1e-9)
            };
            assert!(on_vertex(start) && on_vertex(end));
        }
        // a second mirror restores the box
        mirror(&mut k, &mut b, &plane).unwrap();
        for (&v, p) in vertices.iter().zip(&before) {
            assert!((b.vertex(v).unwrap().point - *p).length() < 1e-12);
        }
    }
}
