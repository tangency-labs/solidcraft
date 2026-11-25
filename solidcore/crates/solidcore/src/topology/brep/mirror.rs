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
//! search (`make_missing_nurbs`), where upstream's bounding box makes them lazily. Each
//! face keeps its UV domain across
//! `ReplaceSurface` (upstream's `SetSurface` gives it the new surface's whole natural
//! domain), as `Transform` keeps it when it moves a surface in place: the mirrored surface
//! keeps the face's parameters. The debug drawing code is omitted. No recursion is
//! introduced here.
//!
//! [`transform_matrix`] runs upstream's steps and branches in order: the low-work
//! test (where its `AngleBetween` fails, `degenerate_low_work_transform` applies
//! upstream's reading of the matrix only when the result passes the Level 2 check),
//! `SVDecompose` (status ignored upstream; a failure here is `SM_ERR` without
//! touching the B-rep, where upstream reads the arrays the failed call left), then
//! the `Transform` / `Mirror` sequence, every status folded with `std::max` (the error
//! with the largest code is returned; one without an exact status counts as `SM_ERR`).
//! After the decomposition, a failed `SetCanonical` or `AngleBetween` is returned, as
//! the fork does (WR-658), where upstream reads the angle as `0`.

use crate::error::topology::TopologyFailure;
use crate::error::{ignore_status, Failure, Result, SolidError};
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
use crate::topology::brep::validation::{check_brep_with_keys, CheckLevel};
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
            .ok_or_else(|| SolidError::topology(OP, TopologyFailure::EdgeWithoutCurve))?
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
        // Not upstream: the face's UV domain, which `SetSurface` resets to its new surface's
        // natural domain. The mirrored surface is the NURB with its control points reflected,
        // in the same parameters, and its analytic copy keeps them too (it proves the same
        // points at the same parameters, or stays an exact copy, WR-450). So the face keeps
        // the rectangle it had, as `Transform` keeps it when it moves a surface in place.
        let domain = b.face(face)?.uv_domain;
        let tmp_surface = b
            .face(face)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::topology(OP, TopologyFailure::FaceWithoutSurface))?
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
        // (the face's own domain back, before its UV trims are made again)
        b.face_mut(face)?.uv_domain = domain;

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
    if e.is_unported_or_cancelled() || matches!(e, SolidError::PreparationRequired { .. }) {
        return Err(e);
    }
    let code = |e: &SolidError| e.sm_status().unwrap_or(crate::error::SM_ERR);
    if worst.as_ref().is_none_or(|w| code(&e) > code(w)) {
        *worst = Some(e);
    }
    Ok(())
}

/// Not upstream (WR-658, #16956): the low-work branch of [`transform_matrix`] when
/// `AngleBetween` of row 0 × row 1 and row 2 fails, that is when either is within the
/// zero length (`SM_IS_ZERO_SQUARED`): the matrix is singular, or scales some
/// direction by about `1e-6` or less. Upstream reads the failed angle as `0` and
/// transforms by the unit rows scaled by their lengths. That is the matrix itself when
/// the unit rows 0 and 1 are perpendicular within `SM_EFF_ZERO` (the branch's own test
/// is absolute, so short rows pass it at any angle) and row 2 is zero to working
/// precision or lies along their cross product within `SM_EFF_ZERO_RAD`. Only then is
/// the transform made, first on a [standing copy](Brep::clone_standing_for_self) in a
/// trial kernel with the same tolerances and fixes and no callbacks (so nobody is
/// notified of it), and it is applied to `b` only when the copy passes the Level 2
/// check (its geometry owners named by the body's naming scheme, as
/// [`standing`](crate::topology::brep::validation::standing) names them): a planar
/// sheet under a rank-2 map can stay a valid sheet, while a solid
/// collapses or shrinks below its tolerances. Otherwise, and for rows of zero or
/// non-finite length, a non-finite translation, sheared unit rows or a row 2 off their
/// cross product (matrices upstream's branch would not reproduce), `angle_error` is
/// returned and `b` is untouched. The fork passes `angle_error` on in every case.
///
/// # Errors
///
/// `angle_error` as above; those of [`Brep::clone_standing_for_self`], of the copy's
/// [`transform`](crate::topology::brep::p5::transform) and of the final transform;
/// an unported or cancelled error of the check.
#[allow(clippy::too_many_arguments)]
fn degenerate_low_work_transform(
    k: &mut Kernel,
    b: &mut Brep,
    disp: Vec3,
    [row0, row1, row2]: [Vec3; 3],
    scale_tolerances: bool,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    angle_error: SolidError,
) -> Result<()> {
    let row0_len = row0.length();
    let row1_len = row1.length();
    let row2_len = row2.length();
    let positive = |l: f64| l.is_finite() && l > 0.0;
    let finite = disp.x.is_finite() && disp.y.is_finite() && disp.z.is_finite();
    if !(positive(row0_len) && positive(row1_len) && row2_len.is_finite() && finite) {
        return Err(angle_error);
    }
    let x = row0 / row0_len;
    let y = row1 / row1_len;
    let along = x.dot(y).abs() <= EFF_ZERO
        && (crate::geom::tol::is_zero_squared(row2.length_squared())
            || x.cross(y)
                .angle_between(row2 / row2_len)
                .is_ok_and(|angle| angle <= EFF_ZERO_RAD));
    if !along {
        return Err(angle_error);
    }
    let input_scale = Vec3::new(row0_len, row1_len, row2_len);
    let input_transform = Axis2Placement::new(disp, x, y);
    let mut trial = Kernel {
        tolerances: k.tolerances,
        fixes: k.fixes,
        memoize_curve_caches: k.memoize_curve_caches,
        cancel: k.cancel.clone(),
        ..Kernel::default()
    };
    let mut copy = b.clone_standing_for_self()?;
    transform_with_notifications(
        &mut trial,
        &mut copy,
        &input_transform,
        Some(input_scale),
        scale_tolerances,
        None,
    )?;
    // the geometry owners are the identities the body's naming scheme names its
    // entities by (as `check_brep` and `standing` read them): a read body's recorded
    // keys, not the arena references, or every edge fails its owner test
    let naming = copy.naming();
    let recorded = copy.geometry_owner_keys().clone();
    let keys =
        |e: EntityRef| crate::topology::owner_identity_of(naming, recorded.get(&e).copied(), e);
    let valid = match check_brep_with_keys(&mut trial, &mut copy, CheckLevel::Level2, &keys) {
        Ok(check) => check.is_valid(),
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => false,
    };
    if !valid {
        return Err(angle_error);
    }
    transform_with_notifications(
        k,
        b,
        &input_transform,
        Some(input_scale),
        scale_tolerances,
        notifications,
    )
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
        // a failed AngleBetween reports 0 upstream; the fork (60a8a51) passes the status
        // on. Here the degenerate matrix is applied only where the result is valid.
        let angle_check = match row0.cross(row1).angle_between(row2) {
            Ok(angle) => angle,
            Err(e) => {
                return degenerate_low_work_transform(
                    k,
                    b,
                    disp,
                    [row0, row1, row2],
                    scale_tolerances,
                    notifications,
                    e,
                );
            }
        };

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
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(e) => {
            return Err(
                Failure::new(OP, TopologyFailure::MatrixNotDecomposed { class: e.kind() })
                    .caused_by(e)
                    .into_error(),
            )
        }
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
    transform_v.set_canonical(origin, vx / vx_len, vy / vy_len)?;

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
    // (fork 60a8a51 passes both statuses on: `SER`)
    let angle_v = vx.cross(vy).angle_between(vz)?;
    let mirror_v = angle_v > HALF_PI;

    let angle_ut = utx.cross(uty).angle_between(utz)?;
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
        transform_ut.set_canonical(origin, utx / utx_len, uty / uty_len)?;
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

        transform_ut.set_canonical(disp, v_x, v_y)?;
        // the defaults: no scale, bScaleTol = TRUE
        fold_status(
            &mut worst,
            transform_with_notifications(k, b, &transform_ut, None, true, notifications),
        )?;
    } else {
        // transform without mirror
        transform_ut.set_canonical(disp, utx / utx_len, uty / uty_len)?;
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

    fn concave_arc_round(k: &mut Kernel) -> Brep {
        use crate::blend::fillet_executive::session::FilletSession;
        use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
        use crate::curve::composite_curve::OffsetCornerType;
        use crate::curve::curve::Curve;
        use crate::curve::ellipse::Ellipse;
        use crate::curve::line::Line;
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::geom::extent1d::Extent1d;
        use crate::math::{Point3, Vec3};
        use crate::query::api_queries::edge_closest_point;
        use crate::sweep::api_brep::TaperExtrude;
        use crate::sweep::extrude::{extrude, Draft, ExtrudeTolerances};
        let p = [
            Point3::new(-20., 0., 0.),
            Point3::new(20., 0., 0.),
            Point3::new(20., 0., -10.),
            Point3::new(-20., 0., -10.),
        ];
        let line = |a, b| Curve::from(Line::create_line_segment(3, a, b, None).unwrap());
        let angle = 100.0_f64.atan2(20.).to_degrees();
        let arc = Ellipse::create_canonical_circle(
            k,
            &Axis2Placement::new(Point3::new(0., 0., -110.), Vec3::X, Vec3::Z),
            10_400.0_f64.sqrt(),
            Some(&Extent1d::from_min_max(angle, 180. - angle)),
            None,
        )
        .unwrap();
        let curves = [
            line(p[0], p[1]),
            line(p[1], p[2]),
            Curve::from(arc),
            line(p[3], p[0]),
        ];
        let TaperExtrude::Solid(mut body) = extrude(
            k,
            &curves,
            Vec3::Y,
            10.,
            Some(Draft {
                angle_deg: 12.,
                corner: OffsetCornerType::CurveExtension,
            }),
            true,
            &ExtrudeTolerances {
                loop_gap: 1e-6,
                parallel: 1e-9,
            },
        )
        .unwrap() else {
            panic!("draft refused")
        };
        let at = Point3::new(
            0.0,
            10.0,
            -110.0 + 10_400.0_f64.sqrt() + 10.0 * 12.0_f64.to_radians().tan(),
        );
        let edge = *body
            .edge_list()
            .iter()
            .find(|&&edge| {
                edge_closest_point(k, &body, edge, at).is_ok_and(|r| r.closest.distance < 0.01)
            })
            .unwrap();
        let mut session = FilletSession::new(k, &mut body).unwrap();
        session
            .prepare_per_edge_constant_radius(
                k,
                &[edge],
                &[0.2],
                &[FilletSurfaceGeneratorType::Circular],
                1,
                1.,
            )
            .unwrap();
        session.do_fillet(k).unwrap();
        drop(session);
        crate::topology::brep::p5::stitch_and_orient(
            k,
            &mut body,
            &crate::topology::brep::p5::StitchAndOrientOptions::default(),
        )
        .unwrap();
        body.refresh_cached_topology();
        body
    }

    /// A round on a drafted concave circular wall keeps its revolution parameterization
    /// when copied and reflected. The profile, draft and radius are independently chosen.
    #[test]
    fn mirrored_revolution_patches_keep_their_parameterization() {
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::mass::integrator::properties::{brep_volume, PropertyAttributes};
        use crate::math::Point3;
        use crate::topology::brep::mirror::mirror;
        use crate::topology::brep::valid_and_outward_failure;

        let mut k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        let mut body = concave_arc_round(&mut k);
        // Reload the exact round as modelling operations do before copying the body.
        let mut rounds = 0;
        let mut parameter_error = 0.0_f64;
        for face in body.face_list().to_vec() {
            let source = body.face(face).unwrap().surface.as_ref().unwrap();
            if matches!(
                source.procedural_blend(),
                Some(crate::surface::procedural_blend::BlendRef::Exact { .. })
            ) {
                rounds += 1;
                let record =
                    crate::surface::procedural_blend::convert::exact_to_record(&k, source).unwrap();
                assert_eq!(
                    record.net.kind,
                    crate::surface::procedural_blend::record::ExactNetKindRecord::Revolution
                );
                let mut restored =
                    crate::surface::procedural_blend::convert::exact_from_record(&k, &record)
                        .unwrap();
                let domain = body.face(face).unwrap().uv_domain;
                for u in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    for v in [0.0, 0.5, 1.0] {
                        let uv = domain.evaluate(u, v);
                        let expected = source.evaluate_point(&k, uv).unwrap();
                        let step_uv = restored.convert_uv_from_nurbs_to_step(&k, uv).unwrap();
                        let actual = restored.evaluate_step_point(&k, step_uv).unwrap();
                        let error = (actual - expected).length();
                        parameter_error = parameter_error.max(error);
                        assert!(error < 1e-9);
                    }
                }
                restored.set_owner(source.owner());
                restored.set_owner_zone_tol_3d(Some(body.face(face).unwrap().tolerance));
                body.face_mut(face).unwrap().surface = Some(restored);
            }
        }
        assert_eq!(rounds, 1);
        assert_eq!(valid_and_outward_failure(&mut k, &mut body).unwrap(), None);
        let checked =
            crate::topology::brep::validation::check_brep(&mut k, &mut body, CheckLevel::Level2)
                .unwrap();
        assert!(checked.is_valid(), "{:?}", checked.lines());
        let attributes = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &attributes,
            key_of: &|_| ObjectKey(0),
        };
        let before_volume = brep_volume(&k, &mut body, 1e-6, &attrs).unwrap();
        assert!(before_volume > 0.0);
        let vertices = p1::vertices(&body, None).unwrap();
        let before: Vec<Point3> = vertices
            .iter()
            .map(|&v| body.vertex(v).unwrap().point)
            .collect();
        let plane = Axis2Placement::new(Point3::ZERO, Vec3::X, Vec3::Z);
        mirror(&mut k, &mut body, &plane).unwrap();
        assert_eq!(valid_and_outward_failure(&mut k, &mut body).unwrap(), None);
        let checked =
            crate::topology::brep::validation::check_brep(&mut k, &mut body, CheckLevel::Level2)
                .unwrap();
        assert!(checked.is_valid(), "{:?}", checked.lines());
        let after_volume = brep_volume(&k, &mut body, 1e-6, &attrs).unwrap();
        assert!((after_volume - before_volume).abs() < 1e-6 * before_volume);
        let mut vertex_error = 0.0_f64;
        for (&vertex, point) in vertices.iter().zip(before) {
            let expected = Point3::new(point.x, -point.y, point.z);
            let error = (body.vertex(vertex).unwrap().point - expected).length();
            vertex_error = vertex_error.max(error);
            assert!(error < 1e-10);
        }
        eprintln!("source volume={before_volume:.12}, mirrored volume={after_volume:.12}, parameter error={parameter_error:e}, vertex error={vertex_error:e}");
    }

    /// A shallow cylinder's support must reload before measuring its top round (#16297).
    #[test]
    fn shallow_arc_top_round_reloads_as_a_closed_solid_with_analytic_volume() {
        use crate::blend::fillet_executive::session::FilletSession;
        use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
        use crate::curve::curve::Curve;
        use crate::curve::ellipse::Ellipse;
        use crate::curve::line::Line;
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::mass::integrator::properties::{brep_volume, PropertyAttributes};
        use crate::math::Point3;
        use crate::query::api_queries::edge_closest_point;
        use crate::surface::procedural_blend::convert::{exact_from_record, exact_to_record};
        use crate::sweep::api_brep::TaperExtrude;
        use crate::sweep::extrude::{extrude, ExtrudeTolerances};
        use crate::topology::brep::validation::{check_brep, CheckLevel};

        let mut k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        // Original geometry, constructed here without loading a reporter's design:
        // a 30 x 10 mm profile with a 0.5 mm crown, extruded 10 mm along Z.
        // R = (15^2 + 0.5^2) / (2 * 0.5) = 225.25 mm; centre (15, -214.75).
        let p = [
            Point3::new(0., 0., 0.),
            Point3::new(30., 0., 0.),
            Point3::new(30., 10., 0.),
            Point3::new(0., 10., 0.),
        ];
        let line = |a, b| Curve::from(Line::create_line_segment(3, a, b, None).unwrap());
        let arc =
            Ellipse::circle_from_3_points(&k, p[2], Point3::new(15., 10.5, 0.), p[3], 3, false)
                .unwrap();
        let curves = [
            line(p[0], p[1]),
            line(p[1], p[2]),
            Curve::from(arc),
            line(p[3], p[0]),
        ];
        let TaperExtrude::Solid(mut body) = extrude(
            &mut k,
            &curves,
            Vec3::Z,
            10.,
            None,
            true,
            &ExtrudeTolerances {
                loop_gap: 1e-6,
                parallel: 1e-9,
            },
        )
        .unwrap() else {
            panic!("shallow arc extrusion refused")
        };
        let at = Point3::new(15., 10.5, 10.);
        let edge = *body
            .edge_list()
            .iter()
            .find(|&&edge| {
                edge_closest_point(&k, &body, edge, at).is_ok_and(|r| r.closest.distance < 1e-6)
            })
            .unwrap();
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_per_edge_constant_radius(
                &mut k,
                &[edge],
                &[2.],
                &[FilletSurfaceGeneratorType::Circular],
                1,
                1.,
            )
            .unwrap();
        session.do_fillet(&mut k).unwrap();
        drop(session);
        crate::topology::brep::p5::stitch_and_orient(
            &mut k,
            &mut body,
            &crate::topology::brep::p5::StitchAndOrientOptions::default(),
        )
        .unwrap();
        body.refresh_cached_topology();

        let mut rounds = 0;
        for face in body.face_list().to_vec() {
            let source = body.face(face).unwrap().surface.as_ref().unwrap();
            if matches!(
                source.procedural_blend(),
                Some(crate::surface::procedural_blend::BlendRef::Exact { .. })
            ) {
                rounds += 1;
                let record = exact_to_record(&k, source).unwrap();
                // This derives and builds the cylinder support. Before the endpoint-tangent
                // fix, the shallow arc's recovered centre/radius failed `evaluates_as`.
                let mut restored = exact_from_record(&k, &record).unwrap();
                restored.set_owner(source.owner());
                restored.set_owner_zone_tol_3d(Some(body.face(face).unwrap().tolerance));
                body.face_mut(face).unwrap().surface = Some(restored);
            }
        }
        assert_eq!(rounds, 1);
        assert!(crate::topology::brep::is_manifold_solid(&body).unwrap());
        assert_eq!(
            crate::topology::brep::valid_and_outward_failure(&mut k, &mut body).unwrap(),
            None
        );
        let checked = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap();
        assert!(checked.is_valid(), "{:?}", checked.lines());

        // Independently integrate horizontal sections, keeping the end planes x=0,30.
        // A(rho) = 30*(-214.75) + 15*sqrt(rho^2-225) + rho^2*asin(15/rho).
        // Below z=8: rho=R. On the 2 mm round: rho=R-2+sqrt(4-(z-8)^2).
        // V = 8*A(R) + integral_0^(pi/2) A(R-2+2*cos(t))*2*cos(t) dt.
        // Composite Simpson integration at 512 and 1024 intervals agrees within 3e-10
        // mm^3, giving 3074.25085063447 mm^3 (unrounded: 3100.02221869620 mm^3).
        let attributes = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &attributes,
            key_of: &|_| ObjectKey(0),
        };
        let volume = brep_volume(&k, &mut body, 1e-6, &attrs).unwrap();
        let expected = 3074.25085063447;
        // Absolute 0.0001 mm^3 tolerance, about 3.3e-8 relative.
        assert!((volume - expected).abs() < 1e-4, "volume={volume:.12}");
    }

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

    /// A reflection keeps each face's UV domain, as a rotation does. A box (x 6 to 8, y ±0.25,
    /// z 2 to 4.5) less a whole torus (radii 7 and 1 about z, its centre circle at z = 4)
    /// leaves the torus a face over a sliver of its surface; mirrored in y = 0
    /// (`SmBrep::Transform` of diag(1, −1, 1)), its surface is the torus's NURB with its
    /// control points reflected, in the same parameters, so the face's rectangle is the same
    /// one. `SetSurface` gave it the new surface's whole natural domain.
    #[test]
    fn a_reflection_keeps_each_faces_uv_domain() {
        use crate::boolean::api_brep::boolean_difference;
        use crate::mass::integrator::properties::brep_volume_default;
        use crate::primitive::api_primitives::create_torus;
        use crate::topology::face::p1::calculate_uv_domain_from_uv_trim_curves;

        let mut k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        let block = create_box(&mut k, Vec3::new(6.0, -0.25, 2.0), 2.0, 0.5, 2.5).unwrap();
        let torus = create_torus(&mut k, Vec3::new(0.0, 0.0, 4.0), 7.0, 1.0).unwrap();
        let mut b = boolean_difference(&mut k, block, torus).unwrap();
        let faces = p1::faces(&b, None).unwrap();
        let before: Vec<_> = faces
            .iter()
            .map(|&f| b.face(f).unwrap().uv_domain)
            .collect();
        // the groove's face covers a sliver of its surface
        let narrow = faces.iter().zip(&before).any(|(&f, d)| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let n = s.natural_uv_domain_with_nurb(&k).unwrap();
            d.size().x < 0.1 * n.size().x
        });
        assert!(narrow, "no face narrower than its surface: {before:?}");
        let volume = brep_volume_default(&k, &mut b, 1e-8).unwrap();

        let matrix = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        transform_matrix(&mut k, &mut b, &matrix, true).unwrap();
        assert!(level2_valid(&mut k, &mut b));
        let mirrored = brep_volume_default(&k, &mut b, 1e-8).unwrap();
        assert!(
            (mirrored - volume).abs() <= 1e-9 * volume,
            "{mirrored} {volume}"
        );
        for (&f, d) in faces.iter().zip(&before) {
            assert_eq!(b.face(f).unwrap().uv_domain, *d, "face {f:?}");
            // and the rectangle holds the face's UV trims, made again on the new surface
            let trims = calculate_uv_domain_from_uv_trim_curves(&k, &mut b, f, true, false, false)
                .unwrap()
                .domain;
            let tol = 1e-9 * d.size().length();
            assert!(
                d.contains_point(trims.min(), tol) && d.contains_point(trims.max(), tol),
                "face {f:?}: trims {trims:?} outside {d:?}"
            );
        }
    }

    /// A cylinder of radius 10 and height 100 at x = 50, mirrored in the plane x = 0: its two
    /// end circles stay circles of radius 10 (not the rational B-splines of their NURBs,
    /// WR-21139), and each edge parameter gives the mirror of the point it gave before, so
    /// the edge intervals stay valid. A second mirror gives the source points back.
    #[test]
    fn mirrors_a_cylinder_s_circles_as_circles() {
        use crate::curve::curve::p2::CurveKind;
        use crate::primitive::api_primitives::create_cylinder;
        let mut k = Kernel::default();
        let mut b = create_cylinder(&mut k, Vec3::new(50.0, 0.0, 0.0), 10.0, 100.0).unwrap();
        let samples = |k: &Kernel, b: &Brep| -> Vec<Vec<Vec3>> {
            p1::edges(b, None)
                .unwrap()
                .iter()
                .map(|&e| {
                    let edge = b.edge(e).unwrap();
                    let (lo, hi) = (edge.interval.min(), edge.interval.max());
                    let curve = edge.curve.as_ref().unwrap();
                    [0.0, 0.2, 0.5, 0.7, 1.0]
                        .iter()
                        .map(|f| curve.evaluate_point(k, lo + f * (hi - lo)).unwrap())
                        .collect()
                })
                .collect()
        };
        let circles = |b: &Brep| -> usize {
            p1::edges(b, None)
                .unwrap()
                .iter()
                .filter(
                    |&&e| match b.edge(e).unwrap().curve.as_ref().unwrap().kind() {
                        CurveKind::Line(_) => false,
                        CurveKind::Ellipse(c) => {
                            assert!(c.is_circle_class() && (c.radius() - 10.0).abs() < 1e-12);
                            true
                        }
                        other => panic!("a curved edge is not a circle: {other:?}"),
                    },
                )
                .count()
        };
        assert_eq!(circles(&b), 2);
        let before = samples(&k, &b);
        // the plane x = 0 (normal +X)
        let plane = Axis2Placement::new(Vec3::ZERO, Vec3::Y, Vec3::Z);
        mirror(&mut k, &mut b, &plane).unwrap();
        assert_eq!(circles(&b), 2);
        for (got, want) in samples(&k, &b).iter().zip(&before) {
            for (q, p) in got.iter().zip(want) {
                let expected = Vec3::new(-p.x, p.y, p.z);
                assert!(
                    (*q - expected).length() < 1e-9,
                    "{q:?}, expected {expected:?}"
                );
            }
        }
        mirror(&mut k, &mut b, &plane).unwrap();
        assert_eq!(circles(&b), 2);
        for (got, want) in samples(&k, &b).iter().zip(&before) {
            for (q, p) in got.iter().zip(want) {
                assert!((*q - *p).length() < 1e-9, "{q:?}, expected {p:?}");
            }
        }
    }

    fn points(b: &Brep) -> Vec<Vec3> {
        p1::vertices(b, None)
            .unwrap()
            .iter()
            .map(|&v| b.vertex(v).unwrap().point)
            .collect()
    }

    fn counts(b: &Brep) -> [usize; 3] {
        [
            p1::faces(b, None).unwrap().len(),
            p1::edges(b, None).unwrap().len(),
            p1::vertices(b, None).unwrap().len(),
        ]
    }

    fn level2_valid(k: &mut Kernel, b: &mut Brep) -> bool {
        crate::topology::brep::validation::standing(k, b)
            .unwrap()
            .check
            .is_valid()
    }

    /// The rotation rows of the goldens' singular matrices (`mat0046`): rows 0 and 1
    /// perpendicular unit vectors, row 2 zero, so `AngleBetween` fails.
    fn rank_two(t: [f64; 3]) -> [[f64; 4]; 4] {
        let (a, c) = (0.3_f64, 0.8_f64);
        let (ca, sa, cc, sc) = (a.cos(), a.sin(), c.cos(), c.sin());
        [
            [ca, sa, 0.0, 0.0],
            [-sa * cc, ca * cc, sc, 0.0],
            [0.0, 0.0, 0.0, 0.0],
            [t[0], t[1], t[2], 1.0],
        ]
    }

    #[test]
    fn a_singular_matrix_refuses_a_solid_and_leaves_it_untouched() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Vec3::new(1.0, 2.0, 3.0), 2.0, 3.0, 4.0).unwrap();
        let before = points(&b);
        let r = transform_matrix(&mut k, &mut b, &rank_two([1.0, -2.0, 0.5]), true);
        assert!(zero_length(&r), "{r:?}");
        assert_eq!(points(&b), before);
        assert_eq!(counts(&b), [6, 12, 8]);
        assert!(level2_valid(&mut k, &mut b));
    }

    #[test]
    fn a_tiny_uniform_scale_refuses_a_solid_it_would_shrink_below_its_tolerances() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Vec3::ZERO, 2.0, 3.0, 4.0).unwrap();
        let before = points(&b);
        let s = 5e-7;
        let m = [
            [s, 0.0, 0.0, 0.0],
            [0.0, s, 0.0, 0.0],
            [0.0, 0.0, s, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let r = transform_matrix(&mut k, &mut b, &m, true);
        assert!(zero_length(&r), "{r:?}");
        assert_eq!(points(&b), before);
        assert_eq!(counts(&b), [6, 12, 8]);
    }

    /// A matrix the decomposition cannot take (a NaN entry) fails as
    /// `MatrixNotDecomposed`, keeps the decomposition's error as its cause and takes its
    /// class from it, with status `SM_ERR`; the box is left as it was (review of #21132).
    #[test]
    fn a_matrix_the_decomposition_refuses_keeps_its_error_as_the_cause() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Vec3::ZERO, 2.0, 3.0, 4.0).unwrap();
        let before = points(&b);
        let m = [
            [f64::NAN, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let e = transform_matrix(&mut k, &mut b, &m, true).unwrap_err();
        let cause = e.cause().expect("the decomposition's error is the cause");
        assert_eq!(cause.operation(), "geom::matrix::Matrix::sv_decompose");
        assert_eq!(cause.kind(), crate::error::ErrorKind::InvalidInput);
        assert_eq!(
            e.topology_failure(),
            Some(&TopologyFailure::MatrixNotDecomposed {
                class: crate::error::ErrorKind::InvalidInput
            })
        );
        assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
        assert_eq!(e.failed_routine_status(), Some(crate::error::SM_ERR));
        assert_eq!(points(&b), before);
        assert_eq!(counts(&b), [6, 12, 8]);
    }

    fn zero_length(r: &Result<()>) -> bool {
        matches!(r, Err(SolidError::InvalidInput { reason, .. }) if *reason == "zero-length vector")
    }

    #[test]
    fn a_degenerate_matrix_upstreams_branch_would_not_reproduce_is_refused() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Vec3::ZERO, 2.0, 3.0, 4.0).unwrap();
        let before = points(&b);
        // short rows 0 and 1 at 45 degrees: their dot (8.1e-13) passes the branch's
        // absolute test, their cross product fails AngleBetween, and the unit rows'
        // frame would apply a different (valid) map
        let a = 0.9e-6;
        let sheared = [
            [a, 0.0, 0.0, 0.0],
            [a, a, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        // row 2 off the cross product of perpendicular short rows 0 and 1
        let off_axis = [
            [1e-7, 0.0, 0.0, 0.0],
            [0.0, 1e-7, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        // a non-finite translation
        let mut nan_t = rank_two([0.0; 3]);
        nan_t[3][0] = f64::NAN;
        for m in [sheared, off_axis, nan_t] {
            let r = transform_matrix(&mut k, &mut b, &m, true);
            assert!(zero_length(&r), "{m:?}: {r:?}");
            assert_eq!(points(&b), before);
            assert_eq!(counts(&b), [6, 12, 8]);
        }
    }

    #[test]
    fn a_singular_matrix_maps_a_planar_sheet_to_its_exact_image() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_plane(
            &mut k,
            Vec3::new(0.5, -1.0, 0.0),
            3.0,
            2.0,
        )
        .unwrap();
        let before = points(&b);
        let t = [1.0, -2.0, 0.5];
        let m = rank_two(t);
        transform_matrix(&mut k, &mut b, &m, true).unwrap();
        assert_eq!(counts(&b), [1, 4, 4]);
        let image = |p: Vec3| {
            Vec3::new(
                p.x * m[0][0] + p.y * m[1][0] + p.z * m[2][0] + t[0],
                p.x * m[0][1] + p.y * m[1][1] + p.z * m[2][1] + t[1],
                p.x * m[0][2] + p.y * m[1][2] + p.z * m[2][2] + t[2],
            )
        };
        for (q, p) in points(&b).into_iter().zip(before) {
            assert!((q - image(p)).length() < 1e-12, "{q:?} {:?}", image(p));
        }
        assert!(level2_valid(&mut k, &mut b));
    }

    #[test]
    fn a_degenerate_matrix_with_a_zero_row_is_refused_before_any_copy() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_plane(&mut k, Vec3::ZERO, 3.0, 2.0).unwrap();
        let before = points(&b);
        let mut m = rank_two([0.0; 3]);
        m[1] = [0.0, 0.0, 0.0, 0.0];
        let r = transform_matrix(&mut k, &mut b, &m, true);
        assert!(zero_length(&r), "{r:?}");
        assert_eq!(points(&b), before);
        assert_eq!(counts(&b), [1, 4, 4]);
    }
}
