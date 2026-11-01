//! General linear (4x4 matrix) transforms from a `uvw` parameter space to an `xyz`
//! projection space, the simplest volume map.
//!
//! Upstream: `source/SMLib/inc/SmTransform.h`, `source/SMLib/src/SmTransform.cpp`.
//!
//! | Rust                                              | Upstream                                              |
//! |---------------------------------------------------|-------------------------------------------------------|
//! | [`Transform`]                                     | `SmTransform`                                         |
//! | [`Transform::IDENTITY`], `Default`                | `SmTransform(const SmContext* = NULL)` (identity)     |
//! | [`Transform::new`]                                | `SmTransform(cpContext, ToOrigin, ToX, ToY, cpToZ)`   |
//! | `Clone` / `Copy`                                  | `SmTransform(const SmTransform&, SmBoolean)`, `Copy`  |
//! | `Drop` (implicit)                                 | `~SmTransform`                                        |
//! | [`Transform::approx_eq`]                          | `operator==` (the transform's own members)            |
//! | [`Transform::matrix`]                             | `m_adT` (field access)                                |
//! | [`Transform::natural_param_domain`]               | `GetNaturalParamDomain`                               |
//! | [`Transform::to_origin`], [`Transform::to_disp`]  | `GetToOrigin`, `GetToDisp`                            |
//! | [`Transform::to_x_axis`], [`Transform::to_y_axis`], [`Transform::to_z_axis`] | `GetToXAxis`, `GetToYAxis`, `GetToZAxis` |
//! | [`Transform::set_to_origin`], [`Transform::set_to_disp`] | `SetToOrigin`, `SetToDisp` (vector and three-double forms) |
//! | [`Transform::set_to_x_axis`], [`Transform::set_to_y_axis`], [`Transform::set_to_z_axis`] | `SetToXAxis`, `SetToYAxis`, `SetToZAxis` (both forms) |
//! | [`Transform::init_ext`]                           | `InitExt`                                             |
//! | [`Transform::init`]                               | `Init`                                                |
//! | [`Transform::set_canonical`]                      | `SetCanonical`                                        |
//! | [`Transform::transform_vector`]                   | `TransformVector`                                     |
//! | [`Transform::inv_transform_point`]                | `InvTransformPoint`                                   |
//! | [`Transform::inv_transform_vector`]               | `InvTransformVector`                                  |
//! | [`Transform::concat_transform`]                   | `ConcatTransform`                                     |
//! | [`Transform::scale_simple`]                       | `ScaleSimple`                                         |
//! | [`Transform::mirror_simple`]                      | `MirrorSimple`                                        |
//! | [`Transform::transform_simple`]                   | `TransformSimple`                                     |
//! | [`Transform::invert_simple`]                      | `InvertSimple`                                        |
//! | [`Transform::set_axis2_placement`]                | `SetAxis2Placement`                                   |
//! | [`Transform::evaluate_simple`]                    | `EvaluateSimple`                                      |
//! | [`Transform::evaluate_bounding_box_simple`], [`Transform::evaluate_bounding_box_simple_with_kernel`] | `EvaluateBoundingBoxSimple` |
//! | [`Transform::inv_evaluate_guess_point_simple`]    | `InvEvaluateGuessPointSimple`                         |
//! | [`Transform::global_point_solve_simple`]          | `GlobalPointSolveSimple`                              |
//! | [`Transform::local_point_solve_simple`]           | `LocalPointSolveSimple`                               |
//! | [`Transform::is_bounded_simple`]                  | `IsBoundedSimple`                                     |
//! | [`Transform::is_curve_in_param_domain_simple`]    | `IsCurveInParamDomainSimple`                          |
//! | [`Transform::has_discontinuities_simple`]         | `HasDiscontinuitiesSimple`                            |
//! | [`Transform::read_from_db`]                       | `ReadFromDB`                                          |
//! | [`Transform::TYPE`], [`Transform::type_code`], [`Transform::type_string`], [`Transform::class_string`] | `GetType`, `GetTypeString`, `GetClassString` (`SM_COMMON`) |
//! | [`Transform::is_kind_of`]                         | `IsKindOf`                                            |
//!
//! Deviations:
//! - **Volume base.** `SmTransform` derives from `SmVolume` (context, attributes,
//!   compounding next/orient maps). That base belongs to the `surface.volume_*`
//!   units, which will hold a [`Transform`] for the transform part. Here a
//!   [`Transform`] is the matrix only: the constructors and `Copy` copy the matrix
//!   (the base part of the copy constructor, with its `bSimpleMapOnly` flag, is the
//!   volume's), and [`Transform::approx_eq`] is the member comparison
//!   `operator==` runs after `SmVolume::operator==` (same type, equal compounding
//!   maps) has passed. The volume base is [`crate::surface::volume::Volume`] with
//!   kind `Transform`: [`Transform::read_from_db`] returns that volume, and
//!   `EvaluateBoundingBoxSimple` takes the volume's `SmVolume::EvaluatePoint` as a
//!   function.
//! - **Pseudo boxes.** `EvaluateBoundingBoxSimple` keeps upstream's in/out pointers
//!   as `Option<&mut _>` outputs (the unbounded branch only resets intervals, so the
//!   outputs' previous state matters). A requested output without a parameter
//!   pseudo box is [`SolidError::InvalidInput`] (upstream dereferences the null
//!   pointer, `plan/findings.md`).
//! - **WR-8.** By default [`Transform::evaluate_bounding_box_simple`] keeps upstream's
//!   aligned-basis interval formula (`scale(1 / |axis|)`, `translate(o · axis)`, and
//!   axis-to-basis permutation); [`Transform::evaluate_bounding_box_simple_with_kernel`]
//!   maps the exact box image behind `Kernel::fixes.transform_aligned_bbox_image`
//!   (`plan/upstream-issues/WR-8--addendum-2026-10-31-aligned-bbox-image.md`).
//! - Out-params become return values. `InvTransformPoint`, `InvTransformVector`
//!   and `InvEvaluateGuessPointSimple` return `Err` (upstream `SM_ERR`, code 1001)
//!   for a singular axes sub-matrix, where upstream leaves the output unset.
//!   `EvaluateSimple` returns the `(n+1)^3` derivative array. `GlobalPointSolveSimple`
//!   returns `Ok(())` for upstream's `rbFoundAnswer = TRUE` and `Err` (`SM_ERR`)
//!   for `FALSE`, after updating the solution array exactly as upstream does in
//!   both cases; the volume itself (`m_apObjects[0] = this`) is passed in as the
//!   caller's object handle.
//! - `IsCurveInParamDomainSimple` ignores its curve upstream and takes none here;
//!   `HasDiscontinuitiesSimple`'s optional `SmDiscontinuities3d*` becomes an `Option`
//!   (`bCalcGeometric` is unused upstream and dropped).
//! - `GetToXAxis` and friends return copies; writes go through the setters.
//! - `GetMemoryUsed`, `Dump`, `DrawAxis`, `AssertValid`, `WriteToDB` and the members
//!   upstream never reaches from the kernel (`TransformPoint`, `MirrorSimple`,
//!   `RotateAboutAxis*`, `DecomposeToAngles`, ... listed in
//!   `harness/callgraph/unused_functions.txt`) are not part of this unit.

use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p1::inverse_3x3;
use crate::geom::geom_utility::p2::{inverse_4x4, mult_4x4};
use crate::geom::pseudo_box::PseudoBox;
use crate::geom::solution_array::{Solution, SolutionArray, SolutionObject, SolutionType};
use crate::geom::tol::is_zero;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::INFINITE_PARAMETER;
use crate::surface::volume::p1::{
    is_kind_of as volume_is_kind_of, Discontinuities3d, Volume, VolumeKind,
};
use crate::surface::volume::p2::ignore_read;
use crate::topology::database_io::{DatabaseReader, FileType};

/// `GW_MAX_DERIV`: the highest derivative `EvaluateSimple` accepts.
pub const MAX_DERIV: usize = 5;

/// A general linear transform, the 4x4 matrix `T` of `SmTransform`, mapping a
/// parameter-space point `[u v w 1]` to the projection-space point `[u v w 1] * T`.
///
/// Rows 0..2 hold the images of the `u`, `v`, `w` unit vectors (the "to" axes),
/// row 3 the image of the origin (the displacement); column 3 is the projective
/// part, `(0, 0, 0, 1)` for affine transforms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    m: [[f64; 4]; 4],
}

impl Default for Transform {
    /// The identity transform (`SmTransform()`).
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// `SmTransform_TYPE` (`VOL_BASE_TYPE + 8`).
    pub const TYPE: i64 = 22008;

    /// The identity transform (`SmTransform()` / `Init`).
    pub const IDENTITY: Self = Self {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// The transform taking the parameter axes to `x_axis`, `y_axis`, `z_axis`
    /// (`x_axis × y_axis` when `None`) and the parameter origin to `origin`
    /// (`SmTransform(cpContext, ToOrigin, ToXAxis, ToYAxis, cpToZAxis)`).
    #[must_use]
    pub fn new(origin: Point3, x_axis: Vec3, y_axis: Vec3, z_axis: Option<Vec3>) -> Self {
        // SmVolume(cpContext) leaves m_adT uninitialised; SetCanonical writes every entry.
        let mut t = Self::IDENTITY;
        t.set_canonical(origin, x_axis, y_axis, z_axis);
        t
    }

    /// The 4x4 matrix `m_adT[row][col]`.
    #[must_use]
    pub const fn matrix(&self) -> &[[f64; 4]; 4] {
        &self.m
    }

    fn get_row(&self, i: usize) -> Vec3 {
        self.m
            .get(i)
            .map_or(Vec3::ZERO, |r| Vec3::new(r[0], r[1], r[2]))
    }

    fn set_row(&mut self, i: usize, v: Vec3) {
        if let Some(r) = self.m.get_mut(i) {
            r[0] = v.x;
            r[1] = v.y;
            r[2] = v.z;
        }
    }

    /// Tolerant equality of every matrix entry (`SM_IS_ZERO(a - b)`): the
    /// transform-specific part of `operator==`.
    #[must_use]
    pub fn approx_eq(&self, other: &Self) -> bool {
        self.m
            .iter()
            .flatten()
            .zip(other.m.iter().flatten())
            .all(|(a, b)| is_zero(a - b))
    }

    /// The parameter domain, unbounded in every direction (`GetNaturalParamDomain`).
    #[must_use]
    pub const fn natural_param_domain(&self) -> Extent3d {
        Extent3d::from_bounds(
            -INFINITE_PARAMETER,
            -INFINITE_PARAMETER,
            -INFINITE_PARAMETER,
            INFINITE_PARAMETER,
            INFINITE_PARAMETER,
            INFINITE_PARAMETER,
        )
    }

    /// The image of the parameter origin, row 3 (`GetToOrigin`).
    #[must_use]
    pub fn to_origin(&self) -> Point3 {
        self.get_row(3)
    }

    /// The image of the `u` unit vector, row 0 (`GetToXAxis`).
    #[must_use]
    pub fn to_x_axis(&self) -> Vec3 {
        self.get_row(0)
    }

    /// The image of the `v` unit vector, row 1 (`GetToYAxis`).
    #[must_use]
    pub fn to_y_axis(&self) -> Vec3 {
        self.get_row(1)
    }

    /// The image of the `w` unit vector, row 2 (`GetToZAxis`).
    #[must_use]
    pub fn to_z_axis(&self) -> Vec3 {
        self.get_row(2)
    }

    /// The displacement, same as [`Transform::to_origin`] (`GetToDisp`).
    #[must_use]
    pub fn to_disp(&self) -> Point3 {
        self.to_origin()
    }

    /// Sets the projective column to `(0, 0, 0, 1)` (`InitExt`).
    pub fn init_ext(&mut self) {
        self.m[0][3] = 0.0;
        self.m[2][3] = 0.0;
        self.m[1][3] = 0.0;
        self.m[3][3] = 1.0;
    }

    /// Sets row 3 (`SetToOrigin`).
    pub fn set_to_origin(&mut self, v: Vec3) {
        self.set_row(3, v);
    }

    /// Sets row 0 (`SetToXAxis`).
    pub fn set_to_x_axis(&mut self, v: Vec3) {
        self.set_row(0, v);
    }

    /// Sets row 1 (`SetToYAxis`).
    pub fn set_to_y_axis(&mut self, v: Vec3) {
        self.set_row(1, v);
    }

    /// Sets row 2 (`SetToZAxis`).
    pub fn set_to_z_axis(&mut self, v: Vec3) {
        self.set_row(2, v);
    }

    /// Sets the displacement, row 3 (`SetToDisp`).
    pub fn set_to_disp(&mut self, v: Vec3) {
        self.set_to_origin(v);
    }

    /// Resets to the identity transform (`Init`).
    pub fn init(&mut self) {
        self.set_to_x_axis(Vec3::new(1.0, 0.0, 0.0));
        self.set_to_y_axis(Vec3::new(0.0, 1.0, 0.0));
        self.set_to_z_axis(Vec3::new(0.0, 0.0, 1.0));
        self.set_to_disp(Vec3::ZERO);
        self.init_ext();
    }

    /// Sets the axes and displacement; `z_axis` defaults to `x_axis × y_axis`. Non-unit
    /// or non-orthogonal axes add scaling or shear (`SetCanonical`).
    pub fn set_canonical(
        &mut self,
        origin: Point3,
        x_axis: Vec3,
        y_axis: Vec3,
        z_axis: Option<Vec3>,
    ) {
        let z = z_axis.unwrap_or_else(|| x_axis.cross(y_axis));
        self.set_to_disp(origin);
        self.set_to_x_axis(x_axis);
        self.set_to_y_axis(y_axis);
        self.set_to_z_axis(z);
        self.init_ext();
    }

    /// Maps a parameter-space vector to projection space, ignoring the displacement
    /// (`TransformVector`).
    #[must_use]
    pub fn transform_vector(&self, v: Vec3) -> Vec3 {
        let x = self.to_x_axis();
        let y = self.to_y_axis();
        let z = self.to_z_axis();
        Vec3::new(
            v.x * x.x + v.y * y.x + v.z * z.x,
            v.x * x.y + v.y * y.y + v.z * z.y,
            v.x * x.z + v.y * y.z + v.z * z.z,
        )
    }

    /// Maps a projection-space point back to parameter space (`InvTransformPoint`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the axes sub-matrix is singular.
    pub fn inv_transform_point(&self, p: Point3) -> Result<Point3> {
        let mid = p - self.to_disp();
        self.inv_transform_vector(mid)
            .map_err(|_| SolidError::sm_err("geom::transform::Transform::inv_transform_point"))
    }

    /// Maps a projection-space vector back to parameter space through the inverse
    /// of the axes sub-matrix (`InvTransformVector`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the axes sub-matrix is singular (`smgu_3x3Inverse` fails).
    pub fn inv_transform_vector(&self, v: Vec3) -> Result<Vec3> {
        let inv = inverse_3x3(
            &self.to_x_axis().to_array(),
            &self.to_y_axis().to_array(),
            &self.to_z_axis().to_array(),
        )
        .map_err(|_| SolidError::sm_err("geom::transform::Transform::inv_transform_vector"))?;
        let [i0, i1, i2] = inv.map(Vec3::from_array);
        let t_inv = Self::new(Vec3::ZERO, i0, i1, Some(i2));
        Ok(t_inv.transform_vector(v))
    }

    /// Appends `other` to this transform sequence: `T = T * other`
    /// (`ConcatTransform`).
    pub fn concat_transform(&mut self, other: &Self) {
        self.m = mult_4x4(&self.m, &other.m);
    }

    /// Scales the projection space by `scale` per coordinate, about `center` (the
    /// origin when `None`) (`ScaleSimple`).
    pub fn scale_simple(&mut self, scale: Vec3, center: Option<Point3>) {
        let mut x = self.to_x_axis();
        let mut y = self.to_y_axis();
        let mut z = self.to_z_axis();
        let mut d = self.to_disp();

        x.x *= scale.x;
        y.x *= scale.x;
        z.x *= scale.x;

        x.y *= scale.y;
        y.y *= scale.y;
        z.y *= scale.y;

        x.z *= scale.z;
        y.z *= scale.z;
        z.z *= scale.z;

        if let Some(c) = center {
            d.x += -x.x * c.x - y.x * c.y - z.x * c.z;
            d.y += -x.y * c.x - y.y * c.y - z.y * c.z;
            d.z += -x.z * c.x - y.z * c.y - z.z * c.z;
        }

        self.set_to_x_axis(x);
        self.set_to_y_axis(y);
        self.set_to_z_axis(z);
        self.set_to_disp(d);
    }

    /// Appends the mirror about the plane through `plane_pt` with normal
    /// `plane_normal` (projection space) to this transform (`MirrorSimple`).
    ///
    /// # Errors
    ///
    /// The error of [`Vec3::unitize`] for a zero normal (upstream's `SER` on
    /// `Unitize`); the transform is then unchanged.
    pub fn mirror_simple(&mut self, plane_pt: Point3, plane_normal: Vec3) -> Result<()> {
        // unitize Plane Normal
        let n = plane_normal.unitize()?;

        // Build mirror transform            = [1-2NxNx  -2NxNy  -2NxNz  0]
        //                                     [ -2NyNy 1-2NyNy  -2NyNz  0]
        //  with D = crPlanePt.Dot(pNormal) ;  [ -2NzNx  -2NzNy 1-2NzNz  0]
        //                                     [  2D*Nx   2D*Ny   2D*Nz  1]
        let d = plane_pt.dot(n);
        let mut mirror = Self::IDENTITY;
        mirror.set_to_x_axis(Vec3::new(
            1.0 - 2.0 * n.x * n.x,
            -2.0 * n.x * n.y,
            -2.0 * n.x * n.z,
        ));
        mirror.set_to_y_axis(Vec3::new(
            -2.0 * n.y * n.x,
            1.0 - 2.0 * n.y * n.y,
            -2.0 * n.y * n.z,
        ));
        mirror.set_to_z_axis(Vec3::new(
            -2.0 * n.z * n.x,
            -2.0 * n.z * n.y,
            1.0 - 2.0 * n.z * n.z,
        ));
        mirror.set_to_disp(Vec3::new(2.0 * d * n.x, 2.0 * d * n.y, 2.0 * d * n.z));

        // Mirror current transform
        self.concat_transform(&mirror);
        Ok(())
    }

    /// Appends the rigid motion `rotate_n_move` and then the optional scaling
    /// (`TransformSimple`).
    pub fn transform_simple(&mut self, rotate_n_move: &Axis2Placement, scale: Option<Vec3>) {
        let mut t = Self::IDENTITY;
        t.set_axis2_placement(rotate_n_move);
        self.concat_transform(&t);
        if let Some(s) = scale {
            self.scale_simple(s, None);
        }
    }

    /// Inverts the 4x4 matrix in place (`InvertSimple`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the matrix is singular; the transform is then unchanged.
    pub fn invert_simple(&mut self) -> Result<()> {
        self.m = inverse_4x4(&self.m)
            .map_err(|_| SolidError::sm_err("geom::transform::Transform::invert_simple"))?;
        Ok(())
    }

    /// Sets this transform to the rigid motion of `placement` (`SetAxis2Placement`).
    pub fn set_axis2_placement(&mut self, placement: &Axis2Placement) {
        let z = placement.z_axis();
        self.set_to_x_axis(placement.x_axis());
        self.set_to_y_axis(placement.y_axis());
        self.set_to_z_axis(z);
        self.set_to_disp(placement.origin());
        self.init_ext();
    }

    /// Position and partial derivatives at `param_point` up to order
    /// `highest_deriv`, as the `(n+1)^3` array `D[u][v][w]` (index
    /// `(u*(n+1)+v)*(n+1)+w`, `n = highest_deriv`). Only the position and the three
    /// first derivatives (the axes) are non-zero (`EvaluateSimple`).
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` when `highest_deriv > MAX_DERIV`.
    pub fn evaluate_simple(&self, param_point: Point3, highest_deriv: usize) -> Result<Vec<Vec3>> {
        if highest_deriv > MAX_DERIV {
            return Err(SolidError::InvalidInput {
                operation: "geom::transform::Transform::evaluate_simple",
                reason: "highest derivative exceeds GW_MAX_DERIV",
            });
        }
        let n1 = highest_deriv + 1;
        let mut ders = vec![Vec3::ZERO; n1 * n1 * n1];
        let sv = |u: usize, v: usize, w: usize| (u * n1 + v) * n1 + w;

        let x = self.to_x_axis();
        let y = self.to_y_axis();
        let z = self.to_z_axis();
        let d = self.to_disp();

        if let Some(p) = ders.get_mut(0) {
            *p = d + param_point.x * x + param_point.y * y + param_point.z * z;
        }
        if highest_deriv >= 1 {
            for (idx, axis) in [(sv(1, 0, 0), x), (sv(0, 1, 0), y), (sv(0, 0, 1), z)] {
                if let Some(slot) = ders.get_mut(idx) {
                    *slot = axis;
                }
            }
        }
        Ok(ders)
    }

    /// Projection-space bounding boxes of the parameter-space box `param_box`
    /// (`EvaluateBoundingBoxSimple`), written into the requested outputs using
    /// [`Kernel::default`] (see [`Self::evaluate_bounding_box_simple_with_kernel`]).
    ///
    /// The output pseudo box takes the basis of `param_pseudo_box` mapped through the
    /// transform (kept as is when that basis is rejected). When `param_pseudo_box` is
    /// aligned with the parameter axes the intervals are the parameter intervals
    /// scaled by `1/|axis|` and shifted by `origin·axis` (upstream's formula, see
    /// `plan/findings.md`), permuted/negated to the pseudo box's basis order, and
    /// `proj_box` circumscribes the result. For an unaligned pseudo box and an
    /// unbounded `param_box` both outputs are made unbounded (the pseudo box keeps
    /// its previous basis, as upstream); for a bounded `param_box` both outputs
    /// grow by the images of its corners under `evaluate_point` (the volume's
    /// compounding `SmVolume::EvaluatePoint`, whose failures leave the undefined
    /// point). With neither output requested nothing happens.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when an output is requested without
    /// `param_pseudo_box` (upstream dereferences the null pointer);
    /// [`SolidError::Unported`] from `evaluate_point`.
    pub fn evaluate_bounding_box_simple(
        &self,
        param_box: &Extent3d,
        param_pseudo_box: Option<&PseudoBox>,
        proj_box: Option<&mut Extent3d>,
        proj_pseudo_box: Option<&mut PseudoBox>,
        evaluate_point: impl FnMut(Point3) -> Result<Point3>,
    ) -> Result<()> {
        self.evaluate_bounding_box_simple_with_kernel(
            &Kernel::default(),
            param_box,
            param_pseudo_box,
            proj_box,
            proj_pseudo_box,
            evaluate_point,
        )
    }

    /// Projection-space bounding boxes of the parameter-space box `param_box`
    /// (`EvaluateBoundingBoxSimple`), honouring
    /// `k.fixes.transform_aligned_bbox_image` (WR-8).
    ///
    /// With `k.fixes.transform_aligned_bbox_image` off (the default), behaves
    /// identically to [`Self::evaluate_bounding_box_simple`]. When enabled and
    /// `param_pseudo_box` is aligned with the parameter axes on a non-singular
    /// transform, maps each parameter interval by multiplying by `|axis|` and
    /// translating by `o_param * |axis|` (where `o_param` is [`Self::to_origin`]
    /// expressed in the transform's axis basis), permutes parameter axis `idx` into
    /// basis slot `|axis_map[idx]| - 1` (negated when `axis_map[idx] < 0`), and
    /// circumscribes an axis-aligned `proj_pseudo_box` into `proj_box` by basis
    /// (WR-5); singular transforms fall back to the general unbounded/corner branch.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when an output is requested without
    /// `param_pseudo_box` (upstream dereferences the null pointer);
    /// [`SolidError::Unported`] from `evaluate_point`.
    pub fn evaluate_bounding_box_simple_with_kernel<F>(
        &self,
        k: &Kernel,
        param_box: &Extent3d,
        param_pseudo_box: Option<&PseudoBox>,
        mut proj_box: Option<&mut Extent3d>,
        mut proj_pseudo_box: Option<&mut PseudoBox>,
        mut evaluate_point: F,
    ) -> Result<()>
    where
        F: FnMut(Point3) -> Result<Point3>,
    {
        // no work - no output
        if proj_box.is_none() && proj_pseudo_box.is_none() {
            return Ok(());
        }
        let Some(param_pb) = param_pseudo_box else {
            return Err(SolidError::InvalidInput {
                operation: "geom::transform::Transform::evaluate_bounding_box_simple",
                reason: "an output needs a parameter-space pseudo box",
            });
        };

        // init output
        if let Some(b) = proj_box.as_deref_mut() {
            b.init();
        }
        // init the ProjSpace pseudo box with the param pseudo box basis vectors
        // projected to ProjSpace and uninitialized intervals
        let mut this_proj_pb = *param_pb;
        let [b0, b1, b2] = *this_proj_pb.basis();
        let (sx, sy, sz) = (
            self.transform_vector(b0),
            self.transform_vector(b1),
            self.transform_vector(b2),
        );
        // upstream ignores the status
        let basis_ok = this_proj_pb.set_basis(sx, sy, sz).is_ok();
        this_proj_pb.init();

        let axis_map = PseudoBox::new().are_aligned(param_pb);
        let aligned_origin =
            if k.fixes.transform_aligned_bbox_image && basis_ok && axis_map.is_some() {
                self.inv_transform_vector(self.to_origin()).ok()
            } else {
                None
            };
        if let (true, Some(axis_map), Some(o_param)) = (
            k.fixes.transform_aligned_bbox_image,
            axis_map,
            aligned_origin,
        ) {
            let x_length = self.to_x_axis().length();
            let y_length = self.to_y_axis().length();
            let z_length = self.to_z_axis().length();

            let mut ivls = [
                *param_box
                    .u_interval()
                    .scale(x_length)
                    .translate(o_param.x * x_length),
                *param_box
                    .v_interval()
                    .scale(y_length)
                    .translate(o_param.y * y_length),
                *param_box
                    .w_interval()
                    .scale(z_length)
                    .translate(o_param.z * z_length),
            ];

            if axis_map != [1, 2, 3] {
                let mut tmps = [Extent1d::new(); 3];
                for (src_ivl, m) in ivls.into_iter().zip(axis_map) {
                    let slot = (m.unsigned_abs() as usize).wrapping_sub(1);
                    if let Some(dst) = tmps.get_mut(slot) {
                        let mut v = src_ivl;
                        *dst = if m > 0 { v } else { *v.negate() };
                    }
                }
                ivls = tmps;
            }

            let [i0, i1, i2] = ivls;
            this_proj_pb.set_intervals(i0, i1, i2);

            if let Some(pb) = proj_pseudo_box {
                *pb = this_proj_pb;
            }
            if let Some(b) = proj_box {
                let mut k_circ = Kernel::default();
                k_circ.fixes.circumscribe_axis_map_by_basis = true;
                b.circumscribe_with(&this_proj_pb, &k_circ);
            }
        } else if let (false, Some(axis_map)) = (k.fixes.transform_aligned_bbox_image, axis_map) {
            // get Ivls of mapping ParamBox directly to ProjSpace
            let o = self.to_origin();
            let x = self.to_x_axis();
            let x_length = x.length();
            let y = self.to_y_axis();
            let y_length = y.length();
            let z = self.to_z_axis();
            let z_length = z.length();

            let mut ivls = [
                *param_box
                    .u_interval()
                    .scale(1. / x_length)
                    .translate(o.dot(x)),
                *param_box
                    .v_interval()
                    .scale(1. / y_length)
                    .translate(o.dot(y)),
                *param_box
                    .w_interval()
                    .scale(1. / z_length)
                    .translate(o.dot(z)),
            ];

            // when the bases need to be swapped and/or negated to match the param
            // pseudo box basis orientations
            if axis_map != [1, 2, 3] {
                let mut tmps = [Extent1d::new(); 3];
                for (tmp, m) in tmps.iter_mut().zip(axis_map) {
                    let i = (m.unsigned_abs() as usize).wrapping_sub(1);
                    if let Some(ivl) = ivls.get_mut(i) {
                        // upstream's Negate() also negates ivls[i] in place
                        *tmp = if m > 0 { *ivl } else { *ivl.negate() };
                    }
                }
                ivls = tmps;
            }

            let [i0, i1, i2] = ivls;
            this_proj_pb.set_intervals(i0, i1, i2);

            if let Some(pb) = proj_pseudo_box {
                *pb = this_proj_pb;
            }
            if let Some(b) = proj_box {
                b.circumscribe(&this_proj_pb);
            }
        } else if !param_box.is_bounded() {
            // return unbounded BBoxes
            if let Some(b) = proj_box {
                b.set_unbounded();
            }
            if let Some(pb) = proj_pseudo_box {
                pb.set_unbounded();
            }
        } else {
            if let Some(pb) = proj_pseudo_box.as_deref_mut() {
                *pb = this_proj_pb;
            }
            // accumulate the corner mappings into the output boxes (an affine map
            // takes convex polygons to convex polygons)
            for ii in [0.0, 1.0] {
                for jj in [0.0, 1.0] {
                    for kk in [0.0, 1.0] {
                        let uvw = param_box.evaluate(ii, jj, kk);
                        // EvaluatePoint (status ignored: a fresh, undefined point)
                        let xyz = match evaluate_point(uvw) {
                            Ok(p) => p,
                            Err(e) if e.is_unported() => return Err(e),
                            Err(_) => Vec3::UNDEF,
                        };
                        if let Some(b) = proj_box.as_deref_mut() {
                            b.add_point(xyz);
                        }
                        if let Some(pb) = proj_pseudo_box.as_deref_mut() {
                            pb.add_point(xyz);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The parameter point mapping to `proj_point`, as the single guess of the
    /// Newton solvers (`InvEvaluateGuessPointSimple`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the axes sub-matrix is singular.
    pub fn inv_evaluate_guess_point_simple(&self, proj_point: Point3) -> Result<Vec<Point3>> {
        Ok(vec![self.inv_transform_point(proj_point)?])
    }

    /// Solves for the parameter point mapping to `proj_point`
    /// (`GlobalPointSolveSimple`). `solutions` is resized to one entry; on success
    /// that entry becomes a single-value solution with the three parameters,
    /// value `0` and object `this` (the volume's handle); otherwise its type becomes
    /// [`SolutionType::Unknown`]. Other fields of the entry are left as they were.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the transform is not invertible (upstream `rbFoundAnswer =
    /// FALSE`).
    pub fn global_point_solve_simple<O: SolutionObject>(
        &self,
        proj_point: Point3,
        solutions: &mut SolutionArray<O>,
        this: O,
    ) -> Result<()> {
        solutions.set_size(1);
        let res = self.inv_transform_point(proj_point);
        let Some(sol) = solutions.get_mut(0) else {
            return Err(SolidError::sm_err(
                "geom::transform::Transform::global_point_solve_simple",
            ));
        };
        match res {
            Ok(puvw) => {
                sol.solution_type = SolutionType::SingleValue;
                sol.num_variables = 3;
                sol.start.solution_value = 0.0;
                sol.start.parameters[0] = puvw.x;
                sol.start.parameters[1] = puvw.y;
                sol.start.parameters[2] = puvw.z;
                sol.num_objects = 1;
                sol.objects[0] = Some(this);
                Ok(())
            }
            Err(e) => {
                sol.solution_type = SolutionType::Unknown;
                Err(e)
            }
        }
    }

    /// Solves for the parameter point mapping to `proj_point`; the guess is not
    /// needed for a linear map (`LocalPointSolveSimple`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the transform is not invertible.
    pub fn local_point_solve_simple<O: SolutionObject>(
        &self,
        proj_point: Point3,
        this: O,
    ) -> Result<Solution<O>> {
        let mut solutions = SolutionArray::new();
        self.global_point_solve_simple(proj_point, &mut solutions, this)?;
        solutions.get(0).copied().ok_or_else(|| {
            SolidError::sm_err("geom::transform::Transform::local_point_solve_simple")
        })
    }

    /// A transform is never bounded (`IsBoundedSimple`).
    #[must_use]
    pub const fn is_bounded_simple(&self) -> bool {
        false
    }

    /// Every curve lies in the (unbounded) parameter domain
    /// (`IsCurveInParamDomainSimple`).
    #[must_use]
    pub const fn is_curve_in_param_domain_simple(&self) -> bool {
        true
    }

    /// A transform has no internal discontinuities (`HasDiscontinuitiesSimple`);
    /// `disconts` (upstream's optional `pOptDisconts`) is reset.
    pub fn has_discontinuities_simple(&self, disconts: Option<&mut Discontinuities3d>) -> bool {
        if let Some(d) = disconts {
            d.init();
        }
        false
    }

    /// Reads a transform volume from a database stream: the four matrix rows, then
    /// the volume base data (`ReadFromDB`), whose status upstream ignores.
    ///
    /// # Errors
    ///
    /// A failed binary matrix read; the base read's undefined cases
    /// ([`SolidError::Unsupported`]) and [`SolidError::Unported`].
    pub fn read_from_db(db: &mut DatabaseReader, version: u64) -> Result<Volume> {
        let mut t = Self::IDENTITY;
        if db.file_type() == FileType::Ascii {
            for r in 0..4 {
                if let Some(stream) = db.in_stream_mut() {
                    for c in 0..4 {
                        if let Some(v) = stream.extract_double() {
                            if let Some(cell) = t.m.get_mut(r).and_then(|row| row.get_mut(c)) {
                                *cell = v;
                            }
                        }
                    }
                }
                db.go_to_next_line();
            }
        } else {
            for r in 0..4 {
                for c in 0..4 {
                    let v = db.read_double()?;
                    if let Some(cell) = t.m.get_mut(r).and_then(|row| row.get_mut(c)) {
                        *cell = v;
                    }
                }
            }
        }
        let mut volume = Volume::new(VolumeKind::Transform(t));
        // read base class data (status ignored)
        ignore_read(volume.read_base_from_db(db, version))?;
        Ok(volume)
    }

    /// `SmTransform_TYPE` (`GetType`).
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        Self::TYPE
    }

    /// `"SmTransform_TYPE"` (`GetTypeString`).
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        "SmTransform_TYPE"
    }

    /// `"SmTransform"` (`GetClassString`).
    #[must_use]
    pub const fn class_string(&self) -> &'static str {
        "SmTransform"
    }

    /// True when a transform is a `t` (`IsKindOf`): `SmTransform_TYPE` or a base
    /// class's type (`SmVolume::IsKindOf`).
    #[must_use]
    pub const fn is_kind_of(&self, t: i64) -> bool {
        t == Self::TYPE || volume_is_kind_of(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::solution_array::ObjectKind;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Vol;

    impl SolutionObject for Vol {
        fn kind(&self) -> ObjectKind {
            ObjectKind::Volume
        }
    }

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn map_point(t: &Transform, p: Point3) -> Point3 {
        t.evaluate_simple(p, 0).unwrap()[0]
    }

    fn assert_corner_sets_match(actual: &[Point3; 8], expected: &[Point3; 8], tol: f64) {
        let mut used = [false; 8];
        for a in actual {
            let idx = (0..8)
                .find(|&i| !used[i] && (expected[i] - *a).length() < tol)
                .expect("every pseudo-box corner should match a mapped corner");
            used[idx] = true;
        }
    }

    fn aabb_of_points(pts: &[Point3; 8]) -> Extent3d {
        let mut b = Extent3d::new();
        for p in pts {
            b.add_point(*p);
        }
        b
    }

    #[test]
    fn identity_and_canonical() {
        let t = Transform::default();
        assert_eq!(t, Transform::IDENTITY);
        let c = Transform::new(v(1.0, 2.0, 3.0), v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0), None);
        assert_eq!(c.to_z_axis(), v(0.0, 0.0, 1.0));
        assert_eq!(c.to_disp(), v(1.0, 2.0, 3.0));
        assert_eq!(c.matrix()[3], [1.0, 2.0, 3.0, 1.0]);
        let mut i = c;
        i.init();
        assert_eq!(i, Transform::IDENTITY);
    }

    #[test]
    fn evaluate_and_inverse_round_trip() {
        let t = Transform::new(v(1.0, -2.0, 0.5), v(2.0, 0.0, 0.0), v(0.0, 3.0, 1.0), None);
        let d = t.evaluate_simple(v(0.25, -1.0, 4.0), 1).unwrap();
        assert_eq!(d.len(), 8);
        assert_eq!(d[4], t.to_x_axis());
        assert_eq!(d[2], t.to_y_axis());
        assert_eq!(d[1], t.to_z_axis());
        let back = t.inv_transform_point(d[0]).unwrap();
        assert!((back - v(0.25, -1.0, 4.0)).length() < 1e-12);
        assert!(t.evaluate_simple(Vec3::ZERO, MAX_DERIV + 1).is_err());
    }

    #[test]
    fn singular_and_solvers() {
        let flat = Transform::new(Vec3::ZERO, v(1.0, 0.0, 0.0), v(2.0, 0.0, 0.0), None);
        assert!(flat.inv_transform_vector(v(1.0, 1.0, 1.0)).is_err());
        let mut sols: SolutionArray<Vol> = SolutionArray::new();
        assert!(flat
            .global_point_solve_simple(Vec3::ZERO, &mut sols, Vol)
            .is_err());
        assert_eq!(sols.len(), 1);
        assert_eq!(sols.get(0).unwrap().solution_type, SolutionType::Unknown);

        let t = Transform::new(v(1.0, 1.0, 1.0), v(0.0, 1.0, 0.0), v(-1.0, 0.0, 0.0), None);
        let s = t.local_point_solve_simple(v(1.0, 2.0, 3.0), Vol).unwrap();
        assert_eq!(s.solution_type, SolutionType::SingleValue);
        assert_eq!(s.num_variables, 3);
        assert_eq!(s.objects[0], Some(Vol));
        assert_eq!(&s.start.parameters[..3], &[1.0, 0.0, 2.0]);
    }

    #[test]
    fn concat_invert_scale() {
        let mut a = Transform::new(v(1.0, 2.0, 3.0), v(0.0, 1.0, 0.0), v(0.0, 0.0, 1.0), None);
        let b = a;
        a.invert_simple().unwrap();
        a.concat_transform(&b);
        assert!(a.approx_eq(&Transform::IDENTITY));

        let mut s = Transform::IDENTITY;
        s.scale_simple(v(2.0, 3.0, 4.0), Some(v(1.0, 1.0, 1.0)));
        assert_eq!(s.to_disp(), v(-2.0, -3.0, -4.0));
        let mut z = Transform::IDENTITY;
        z.scale_simple(Vec3::ZERO, None);
        assert!(z.invert_simple().is_err());
        z.transform_simple(&Axis2Placement::IDENTITY, Some(v(1.0, 1.0, 1.0)));
    }

    #[test]
    fn predicates_and_stubs() {
        let t = Transform::IDENTITY;
        assert!(!t.is_bounded_simple());
        assert!(t.is_curve_in_param_domain_simple());
        assert!(!t.has_discontinuities_simple(None));
        let mut d = Discontinuities3d::new();
        d.set_unaligned();
        assert!(!t.has_discontinuities_simple(Some(&mut d)));
        assert_eq!(d, Discontinuities3d::new());
        assert!(t.is_kind_of(Transform::TYPE));
        assert!(t.is_kind_of(crate::surface::volume::p1::VOLUME_TYPE));
        assert!(!t.is_kind_of(1));
        assert_eq!(t.type_code(), 22008);
        assert_eq!(t.type_string(), "SmTransform_TYPE");
        assert_eq!(t.class_string(), "SmTransform");
        let d = t.natural_param_domain();
        assert_eq!(d.u_min(), -INFINITE_PARAMETER);
        assert_eq!(
            t.evaluate_bounding_box_simple(&d, None, None, None, Ok),
            Ok(())
        );
        let mut b = Extent3d::new();
        assert!(t
            .evaluate_bounding_box_simple(&d, None, Some(&mut b), None, Ok)
            .is_err());
        let pb = PseudoBox::new();
        let mut proj_pb = PseudoBox::new();
        assert_eq!(
            t.evaluate_bounding_box_simple(&d, Some(&pb), Some(&mut b), Some(&mut proj_pb), Ok),
            Ok(())
        );
        assert_eq!(b.u_min(), -INFINITE_PARAMETER);
        assert_eq!(
            proj_pb.interval(0).map(|i| i.max()),
            Some(INFINITE_PARAMETER)
        );
    }

    #[test]
    fn wr8_non_uniform_scale_and_translation() {
        let t = Transform::new(
            v(10.0, -5.0, 3.0),
            v(2.0, 0.0, 0.0),
            v(0.0, 3.0, 0.0),
            Some(v(0.0, 0.0, 4.0)),
        );
        let param_box = Extent3d::from_bounds(1.0, -1.0, 0.0, 2.0, 1.0, 0.5);
        let param_pb = PseudoBox::new();

        // Default (pinned C++ parity): scales by `1 / |axis|` and translates by `o · axis`.
        let mut def_box = Extent3d::new();
        let mut def_pb = PseudoBox::new();
        t.evaluate_bounding_box_simple(
            &param_box,
            Some(&param_pb),
            Some(&mut def_box),
            Some(&mut def_pb),
            |p| Ok(map_point(&t, p)),
        )
        .unwrap();
        assert!((def_box.min() - v(20.5, -15.0 - 1.0 / 3.0, 12.0)).length() < 1e-12);
        assert!((def_box.max() - v(21.0, -15.0 + 1.0 / 3.0, 12.125)).length() < 1e-12);
        assert!(def_pb
            .interval(0)
            .unwrap()
            .are_equal_bounds(20.5, 21.0, 1e-12));
        assert!(def_pb.interval(1).unwrap().are_equal_bounds(
            -15.0 - 1.0 / 3.0,
            -15.0 + 1.0 / 3.0,
            1e-12
        ));
        assert!(def_pb
            .interval(2)
            .unwrap()
            .are_equal_bounds(12.0, 12.125, 1e-12));

        let mut def_k_box = Extent3d::new();
        let mut def_k_pb = PseudoBox::new();
        t.evaluate_bounding_box_simple_with_kernel(
            &Kernel::default(),
            &param_box,
            Some(&param_pb),
            Some(&mut def_k_box),
            Some(&mut def_k_pb),
            |p| Ok(map_point(&t, p)),
        )
        .unwrap();
        assert_eq!(def_k_box, def_box);
        assert_eq!(def_k_pb, def_pb);

        // Opt-in: exact affine image `[12, 14] x [-8, -2] x [3, 5]`.
        let mut k_on = Kernel::default();
        k_on.fixes.transform_aligned_bbox_image = true;
        let mut opt_box = Extent3d::new();
        let mut opt_pb = PseudoBox::new();
        t.evaluate_bounding_box_simple_with_kernel(
            &k_on,
            &param_box,
            Some(&param_pb),
            Some(&mut opt_box),
            Some(&mut opt_pb),
            |p| Ok(map_point(&t, p)),
        )
        .unwrap();
        assert!((opt_box.min() - v(12.0, -8.0, 3.0)).length() < 1e-12);
        assert!((opt_box.max() - v(14.0, -2.0, 5.0)).length() < 1e-12);
        assert!(opt_pb
            .interval(0)
            .unwrap()
            .are_equal_bounds(12.0, 14.0, 1e-12));
        assert!(opt_pb
            .interval(1)
            .unwrap()
            .are_equal_bounds(-8.0, -2.0, 1e-12));
        assert!(opt_pb
            .interval(2)
            .unwrap()
            .are_equal_bounds(3.0, 5.0, 1e-12));
        let expected_corners = param_box.corners().map(|c| map_point(&t, c));
        assert_corner_sets_match(&opt_pb.corners(), &expected_corners, 1e-12);
    }

    #[test]
    fn wr8_three_cycle_param_basis_and_three_cycle_transform() {
        let mut k_on = Kernel::default();
        k_on.fixes.transform_aligned_bbox_image = true;

        // (a) 3-cycle and negated 3-cycle parameter PseudoBox bases under scale + translation.
        let t = Transform::new(
            v(4.0, -3.0, 2.0),
            v(2.0, 0.0, 0.0),
            v(0.0, 5.0, 0.0),
            Some(v(0.0, 0.0, 3.0)),
        );
        let param_box = Extent3d::from_bounds(1.0, 2.0, -1.0, 3.0, 4.0, 2.0);
        let expected_corners = param_box.corners().map(|c| map_point(&t, c));
        let expected_aabb = aabb_of_points(&expected_corners);
        assert_eq!(expected_aabb.min(), v(6.0, 7.0, -1.0));
        assert_eq!(expected_aabb.max(), v(10.0, 17.0, 8.0));

        // Even under Identity, default scrambles `[Y, Z, X]` intervals backwards (WR-8 bug 3):
        // `axis_map = [3, 1, 2]` puts `w` (`[-1, 2]`) onto basis 0 (`Y`) instead of `v` (`[2, 4]`).
        let mut pb_cycle = PseudoBox::new();
        pb_cycle.set_basis(Vec3::Y, Vec3::Z, Vec3::X).unwrap();
        let mut id_def_pb = PseudoBox::new();
        Transform::IDENTITY
            .evaluate_bounding_box_simple(
                &param_box,
                Some(&pb_cycle),
                None,
                Some(&mut id_def_pb),
                Ok,
            )
            .unwrap();
        assert!(id_def_pb
            .interval(0)
            .unwrap()
            .are_equal_bounds(-1.0, 2.0, 1e-12));

        for basis in [[Vec3::Y, Vec3::Z, Vec3::X], [-Vec3::Y, Vec3::Z, -Vec3::X]] {
            let mut param_pb = PseudoBox::new();
            param_pb.set_basis(basis[0], basis[1], basis[2]).unwrap();

            let mut def_box = Extent3d::new();
            let mut def_pb = PseudoBox::new();
            t.evaluate_bounding_box_simple(
                &param_box,
                Some(&param_pb),
                Some(&mut def_box),
                Some(&mut def_pb),
                |p| Ok(map_point(&t, p)),
            )
            .unwrap();
            assert!((def_box.min() - expected_aabb.min()).length() > 10.0);

            let mut opt_box = Extent3d::new();
            let mut opt_pb = PseudoBox::new();
            t.evaluate_bounding_box_simple_with_kernel(
                &k_on,
                &param_box,
                Some(&param_pb),
                Some(&mut opt_box),
                Some(&mut opt_pb),
                |p| Ok(map_point(&t, p)),
            )
            .unwrap();
            assert_corner_sets_match(&opt_pb.corners(), &expected_corners, 1e-12);
            assert!((opt_box.min() - expected_aabb.min()).length() < 1e-12);
            assert!((opt_box.max() - expected_aabb.max()).length() < 1e-12);
        }

        // (b) Canonical `param_pb` under a 3-cycle rotation + translation transform:
        // `opt_box` circumscribes `this_proj_pb` (`basis = [Y, Z, X]`) by basis without needing
        // `circumscribe_axis_map_by_basis` separately enabled on `k_on`.
        assert!(!k_on.fixes.circumscribe_axis_map_by_basis);
        let t_rot = Transform::new(
            v(10.0, -5.0, 7.0),
            Vec3::Y * 2.0,
            Vec3::Z * 3.0,
            Some(Vec3::X * 4.0),
        );
        let rot_corners = param_box.corners().map(|c| map_point(&t_rot, c));
        let rot_aabb = aabb_of_points(&rot_corners);
        let mut rot_box = Extent3d::new();
        let mut rot_pb = PseudoBox::new();
        t_rot
            .evaluate_bounding_box_simple_with_kernel(
                &k_on,
                &param_box,
                Some(&PseudoBox::new()),
                Some(&mut rot_box),
                Some(&mut rot_pb),
                |p| Ok(map_point(&t_rot, p)),
            )
            .unwrap();
        assert_corner_sets_match(&rot_pb.corners(), &rot_corners, 1e-12);
        assert!((rot_box.min() - rot_aabb.min()).length() < 1e-12);
        assert!((rot_box.max() - rot_aabb.max()).length() < 1e-12);
    }

    #[test]
    fn wr8_shear_with_translation() {
        // Non-orthogonal linear map: columns have non-zero pairwise dot products, so
        // `o · axis != o_param * |axis|^2`.
        let t = Transform::new(
            v(3.0, -2.0, 5.0),
            v(2.0, 1.0, 0.0),
            v(0.0, 3.0, 1.0),
            Some(v(1.0, 0.0, 4.0)),
        );
        let param_box = Extent3d::from_bounds(-1.0, 0.5, -2.0, 2.0, 2.5, 1.0);
        let expected_corners = param_box.corners().map(|c| map_point(&t, c));
        let expected_aabb = aabb_of_points(&expected_corners);

        let mut def_box = Extent3d::new();
        let mut def_pb = PseudoBox::new();
        t.evaluate_bounding_box_simple(
            &param_box,
            Some(&PseudoBox::new()),
            Some(&mut def_box),
            Some(&mut def_pb),
            |p| Ok(map_point(&t, p)),
        )
        .unwrap();
        assert!((def_box.min() - expected_aabb.min()).length() > 10.0);
        assert!((def_box.max() - expected_aabb.max()).length() > 10.0);

        let mut k_on = Kernel::default();
        k_on.fixes.transform_aligned_bbox_image = true;
        for basis in [[Vec3::X, Vec3::Y, Vec3::Z], [-Vec3::Z, Vec3::X, -Vec3::Y]] {
            let mut param_pb = PseudoBox::new();
            param_pb.set_basis(basis[0], basis[1], basis[2]).unwrap();

            let mut opt_box = Extent3d::new();
            let mut opt_pb = PseudoBox::new();
            t.evaluate_bounding_box_simple_with_kernel(
                &k_on,
                &param_box,
                Some(&param_pb),
                Some(&mut opt_box),
                Some(&mut opt_pb),
                |p| Ok(map_point(&t, p)),
            )
            .unwrap();
            assert!(!opt_pb.is_orthogonal_basis());
            assert_corner_sets_match(&opt_pb.corners(), &expected_corners, 1e-12);
            assert!((opt_box.min() - expected_aabb.min()).length() < 1e-12);
            assert!((opt_box.max() - expected_aabb.max()).length() < 1e-12);
        }
    }

    #[test]
    fn wr8_partially_unbounded_and_singular_and_volume_dispatch() {
        let mut k_on = Kernel::default();
        k_on.fixes.transform_aligned_bbox_image = true;

        // 1. Partially unbounded `param_box`.
        let t_scale = Transform::new(
            v(5.0, -1.0, 2.0),
            v(2.0, 0.0, 0.0),
            v(0.0, 3.0, 0.0),
            Some(v(0.0, 0.0, 4.0)),
        );
        let partial_box =
            Extent3d::from_bounds(-INFINITE_PARAMETER, 1.0, -1.0, 2.0, 3.0, INFINITE_PARAMETER);
        let mut p_box = Extent3d::new();
        let mut p_pb = PseudoBox::new();
        t_scale
            .evaluate_bounding_box_simple_with_kernel(
                &k_on,
                &partial_box,
                Some(&PseudoBox::new()),
                Some(&mut p_box),
                Some(&mut p_pb),
                |p| Ok(map_point(&t_scale, p)),
            )
            .unwrap();
        assert_eq!(p_box.u_min(), -INFINITE_PARAMETER);
        assert!((p_box.u_max() - 9.0).abs() < 1e-12);
        assert!((p_box.v_min() - 2.0).abs() < 1e-12);
        assert!((p_box.v_max() - 8.0).abs() < 1e-12);
        assert!((p_box.w_min() + 2.0).abs() < 1e-12);
        assert_eq!(p_box.w_max(), INFINITE_PARAMETER);
        assert_eq!(p_pb.interval(0).unwrap().min(), -INFINITE_PARAMETER);
        assert!((p_pb.interval(0).unwrap().max() - 9.0).abs() < 1e-12);
        assert!(p_pb.interval(1).unwrap().are_equal_bounds(2.0, 8.0, 1e-12));
        assert!((p_pb.interval(2).unwrap().min() + 2.0).abs() < 1e-12);
        assert_eq!(p_pb.interval(2).unwrap().max(), INFINITE_PARAMETER);

        // 2. Singular transform (`rank < 3`): falls back to corner sampling on bounded boxes
        // and to unbounded on unbounded boxes without `NaN` or division by zero.
        let t_sing = Transform::new(v(1.0, 2.0, 3.0), v(2.0, 0.0, 0.0), v(4.0, 0.0, 0.0), None);
        let bounded_box = Extent3d::from_bounds(-1.0, 0.5, -2.0, 3.0, 1.5, 4.0);
        let sing_vol = Volume::new(VolumeKind::Transform(t_sing));
        let mut sing_box = Extent3d::new();
        let mut sing_pb = PseudoBox::new();
        sing_vol
            .evaluate_bounding_box_simple(
                &k_on,
                &bounded_box,
                Some(&PseudoBox::new()),
                Some(&mut sing_box),
                Some(&mut sing_pb),
            )
            .unwrap();
        assert!((sing_box.min() - v(1.0, 2.0, 3.0)).length() < 1e-12);
        assert!((sing_box.max() - v(13.0, 2.0, 3.0)).length() < 1e-12);
        assert!(sing_pb
            .interval(0)
            .unwrap()
            .are_equal_bounds(1.0, 13.0, 1e-12));
        assert!(sing_pb
            .interval(1)
            .unwrap()
            .are_equal_bounds(2.0, 2.0, 1e-12));
        assert!(sing_pb
            .interval(2)
            .unwrap()
            .are_equal_bounds(3.0, 3.0, 1e-12));

        let mut sing_unb_box = Extent3d::new();
        let mut sing_unb_pb = PseudoBox::new();
        sing_vol
            .evaluate_bounding_box_simple(
                &k_on,
                &partial_box,
                Some(&PseudoBox::new()),
                Some(&mut sing_unb_box),
                Some(&mut sing_unb_pb),
            )
            .unwrap();
        assert_eq!(sing_unb_box.u_min(), -INFINITE_PARAMETER);
        assert_eq!(sing_unb_box.w_max(), INFINITE_PARAMETER);
        assert_eq!(sing_unb_pb.interval(1).unwrap().max(), INFINITE_PARAMETER);

        // 3. `Volume` dispatch + `UpstreamFixes` flag reflection (`flag_mut` / `flags`).
        let mut k_flag = Kernel::default();
        assert!(!k_flag
            .fixes
            .flags()
            .any(|f| f == ("transform_aligned_bbox_image", true)));
        *k_flag
            .fixes
            .flag_mut("transform_aligned_bbox_image")
            .unwrap() = true;
        assert!(k_flag.fixes.transform_aligned_bbox_image);
        assert!(k_flag
            .fixes
            .flags()
            .any(|f| f == ("transform_aligned_bbox_image", true)));

        let vol = Volume::new(VolumeKind::Transform(t_scale));
        let mut vol_def_box = Extent3d::new();
        let mut vol_opt_box = Extent3d::new();
        let mut vol_opt_pb = PseudoBox::new();
        vol.evaluate_bounding_box_simple(
            &Kernel::default(),
            &bounded_box,
            Some(&PseudoBox::new()),
            Some(&mut vol_def_box),
            None,
        )
        .unwrap();
        vol.evaluate_bounding_box_simple(
            &k_flag,
            &bounded_box,
            Some(&PseudoBox::new()),
            Some(&mut vol_opt_box),
            Some(&mut vol_opt_pb),
        )
        .unwrap();
        assert!((vol_opt_box.min() - v(3.0, 0.5, -6.0)).length() < 1e-12);
        assert!((vol_opt_box.max() - v(11.0, 3.5, 18.0)).length() < 1e-12);
        assert!((vol_def_box.min() - vol_opt_box.min()).length() > 10.0);
        let expected_vol_corners = bounded_box.corners().map(|c| map_point(&t_scale, c));
        assert_corner_sets_match(&vol_opt_pb.corners(), &expected_vol_corners, 1e-12);
    }
}
