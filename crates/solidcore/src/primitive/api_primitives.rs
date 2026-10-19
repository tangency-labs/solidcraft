//! High-level primitive creation (`SM_API` primitives): bounded plane, cylinder, cone
//! and torus surfaces in the canonical XY frame at a point, and the B-rep primitives
//! (plane and disc sheets, box, sphere, cylinder, cone, torus, pyramid, cylindrical
//! box, their partial variants, a pole-free sphere, an elliptic-ended cone, blend
//! faces and swung solids) with their input validation.
//!
//! Upstream: `source/SM_API/src/SmApiPrimitives.cpp`, `source/SM_API/inc/SmApiPrimitives.h`.
//!
//! | Rust                               | Upstream                                              |
//! |------------------------------------|-------------------------------------------------------|
//! | `is_positive_dim` (private)        | `SmApiIsPositiveDim` (static)                         |
//! | `is_non_zero_radius` (private)     | `SmApiIsNonZeroRadius` (static)                       |
//! | `cone_dims_valid` (private)        | `SmApiConeDimsValid` (static)                         |
//! | [`create_plane`]                   | `SmApiCreatePlane` (`SmBrep*&` overload)              |
//! | [`create_plane_surface`]           | `SmApiCreatePlane` (`SmPlane*&` overload)             |
//! | [`create_planar_circle`]           | `SmApiCreatePlanarCircle`                             |
//! | [`create_box`]                     | `SmApiCreateBox`                                      |
//! | [`create_sphere`]                  | `SmApiCreateSphere`                                   |
//! | [`create_cylinder`]                | `SmApiCreateCylinder` (`SmBrep*&` overload)           |
//! | [`create_cylinder_surface`]        | `SmApiCreateCylinder` (`SmSurface*&` overload)        |
//! | [`create_cone`]                    | `SmApiCreateCone` (`SmBrep*&` overload)               |
//! | [`create_cone_surface`]            | `SmApiCreateCone` (`SmSurface*&` overload)            |
//! | [`create_torus`]                   | `SmApiCreateTorus` (`SmBrep*&` overload)              |
//! | [`create_torus_surface`]           | `SmApiCreateTorus` (`SmSurface*&` overload)           |
//! | [`create_pyramid`]                 | `SmApiCreatePyramid`                                  |
//! | [`create_cylindrical_box`]         | `SmApiCreateCylindricalBox`                           |
//! | [`create_partial_sphere`]          | `SmApiCreatePartialSphere`                            |
//! | [`create_sphere_no_pole`]          | `SmApiCreateSphereNoPole`                             |
//! | [`create_partial_cone`]            | `SmApiCreatePartialCone`                              |
//! | [`create_cone_elliptic_ends`], [`ConeEllipticEnds`] | `SmApiCreateConeEllipticEnds`        |
//! | [`create_partial_cylinder`]        | `SmApiCreatePartialCylinder`                          |
//! | [`create_partial_torus`]           | `SmApiCreatePartialTorus`                             |
//! | [`create_blend_primitive`]         | `SmApiCreateBlendPrimitive`                           |
//! | [`create_swung_primitive`]         | `SmApiCreateSwungPrimitive`                           |
//!
//! Deviations:
//! - **Out-params.** The created surface or B-rep is the return value; on failure there
//!   is none (upstream clears `rpResult`). [`create_blend_primitive`] returns the new
//!   face's id.
//! - **Context.** `SmApiGetOrCreateContext()` is the caller's [`Kernel`].
//! - **Types.** The `SmSurface*&` overloads return the concrete surface: a [`Cone`] for
//!   the cylinder (an `SmCylinder`, [`Cone::is_cylinder_class`]) and the cone, a
//!   [`Torus`] for the torus. `SmBoolean` flags are `bool`. The curve lists of
//!   [`create_swung_primitive`] hold no `NULL` entries (upstream skips them); the B-rep,
//!   faces and edges of [`create_blend_primitive`] are a [`Brep`] and ids into it, so
//!   upstream's `NULL` checks (`SM_ERR_INVALID_INPUT`) hold by construction.
//! - **Statuses.** `SM_ERR_INVALID_INPUT` is [`SolidError::InvalidInput`]; errors of the
//!   called constructors pass through unchanged.
//! - Every B-rep entry point validates its inputs as upstream (and the pyramid makes its
//!   base edges), then returns [`SolidError::Unported`]: `SmPrimitiveCreation` (units
//!   `primitive.primitive_creation_p1..p3`) and the `SmBrep` construction it fills
//!   (`topology.brep_p3`, `topology.brep_p5`, `topology.brep_p8`) are not ported yet.
//!   The error names the call that builds the solid. `SmBrep::GetInfiniteRegion` is
//!   ported (`Brep::infinite_region`) but is only reached once the B-rep exists.
//! - `SmApiCreateOffsetProfile`, `SmApiCreateSkinPrimitive` and
//!   `SmApiCreateSkinFromFaces`, also in this file, belong to other units.
//! - No recursion.

use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector3d::{is_valid_double, Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::surface::cone::Cone;
use crate::surface::plane::Plane;
use crate::surface::torus::{Torus, TorusOptions};
use crate::topology::{Brep, EdgeId, FaceId};

const OP_BOX: &str = "primitive::api_primitives::create_box";
const OP_SPHERE: &str = "primitive::api_primitives::create_sphere";
const OP_CYLINDER: &str = "primitive::api_primitives::create_cylinder_surface";
const OP_CONE: &str = "primitive::api_primitives::create_cone";
const OP_CONE_SURFACE: &str = "primitive::api_primitives::create_cone_surface";
const OP_PYRAMID: &str = "primitive::api_primitives::create_pyramid";
const OP_CYLINDRICAL_BOX: &str = "primitive::api_primitives::create_cylindrical_box";
const OP_SPHERE_NO_POLE: &str = "primitive::api_primitives::create_sphere_no_pole";

const UNIT_P1: &str = "primitive.primitive_creation_p1";
const UNIT_P2: &str = "primitive.primitive_creation_p2";
const UNIT_P3: &str = "primitive.primitive_creation_p3";
const UNIT_BREP_P8: &str = "topology.brep_p8";

/// `SmApiIsPositiveDim`: a finite dimension (radius, length, height) above
/// `SM_EFF_ZERO`.
fn is_positive_dim(value: f64) -> bool {
    is_valid_double(value) && value > EFF_ZERO
}

/// `SmApiIsNonZeroRadius`: a finite, non-zero sphere radius (a negative radius is a
/// valid inside-out sphere).
fn is_non_zero_radius(radius: f64) -> bool {
    is_valid_double(radius) && radius.abs() > EFF_ZERO
}

/// `SmApiConeDimsValid`: finite radii that are not negative and not both zero, and a
/// positive height.
fn cone_dims_valid(radius_base: f64, radius_top: f64, height: f64) -> bool {
    if !is_valid_double(radius_base) || !is_valid_double(radius_top) {
        return false;
    }
    if radius_base < 0.0 || radius_top < 0.0 {
        return false;
    }
    if radius_base <= EFF_ZERO && radius_top <= EFF_ZERO {
        return false;
    }
    is_positive_dim(height)
}

/// The canonical XY frame at `origin` (`SmAxis2Placement` default constructor, then
/// `SetCanonical(origin, (1,0,0), (0,1,0))`, status ignored as upstream).
fn xy_frame(origin: Point3) -> Axis2Placement {
    let mut frame = Axis2Placement::default();
    let _ = frame.set_canonical(origin, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    frame
}

/// The extent `[0, max_x] x [0, max_y]` (`SmExtent2d()` then `SetMinMax(0, 0, max_x,
/// max_y)`, status ignored as upstream: a negative bound leaves the empty default
/// extent).
fn extent_from_zero(max_x: f64, max_y: f64) -> Extent2d {
    let mut extent = Extent2d::default();
    let _ = extent.set_min_max_coords(0.0, 0.0, max_x, max_y);
    extent
}

/// The rectangular sheet B-rep of `x` by `y` in the XY plane with its corner at
/// `origin` (`SmApiCreatePlane`, `SmBrep*&` overload; tolerance `1.0e-5`).
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateRectangle`); upstream fails
/// with `SM_ERR` when no face is made.
pub fn create_plane(origin: Point3, x: f64, y: f64) -> Result<Brep> {
    let frame = xy_frame(origin);
    let tol = 1.0e-5;
    let _ = (frame, tol, x, y);
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::CreateRectangle
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateRectangle",
        UNIT_P1,
    ))
}

/// The plane bounded to `[0, length] x [0, width]` in the XY plane with its minimum
/// corner at `origin` (`SmApiCreatePlane`, `SmPlane*&` overload).
///
/// # Errors
///
/// Those of [`Plane::create_canonical`] and [`Plane::adjust_step_uv_domain`] (e.g. for
/// a zero length or width).
pub fn create_plane_surface(origin: Point3, length: f64, width: f64) -> Result<Plane> {
    let frame = xy_frame(origin);
    let mut plane = Plane::create_canonical(&frame)?;
    plane.adjust_step_uv_domain(&extent_from_zero(length, width))?;
    Ok(plane)
}

/// The disc sheet B-rep of `radius` in the XY plane centred at `origin`
/// (`SmApiCreatePlanarCircle`; tolerance `1.0e-5`).
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateCircle`); upstream fails with
/// `SM_ERR` when no face is made.
pub fn create_planar_circle(origin: Point3, radius: f64) -> Result<Brep> {
    let frame = xy_frame(origin);
    let tol = 1.0e-5;
    let _ = (frame, tol, radius);
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::CreateCircle
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateCircle",
        UNIT_P1,
    ))
}

/// The solid box of `length` (X) by `width` (Y) by `height` (Z) with its lower left
/// corner at `origin` (`SmApiCreateBox`).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] unless every size is finite and above
/// `SM_EFF_ZERO`; then [`SolidError::Unported`] (`SmPrimitiveCreation::CreateBox`).
pub fn create_box(origin: Point3, length: f64, width: f64, height: f64) -> Result<Brep> {
    if !is_positive_dim(length) || !is_positive_dim(width) || !is_positive_dim(height) {
        return Err(SolidError::invalid_input(OP_BOX, "non-positive size"));
    }
    let frame = xy_frame(origin);
    let _ = frame;
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::CreateBox
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateBox",
        UNIT_P1,
    ))
}

/// The solid sphere of `radius` (negative: inside out) centred at `origin`, with poles
/// (`SmApiCreateSphere`: `CreateSphere(radius, 0, 360, frame)`).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] unless the radius is finite and its magnitude above
/// `SM_EFF_ZERO`; then [`SolidError::Unported`] (`SmPrimitiveCreation::CreateSphere`).
pub fn create_sphere(origin: Point3, radius: f64) -> Result<Brep> {
    if !is_non_zero_radius(radius) {
        return Err(SolidError::invalid_input(OP_SPHERE, "zero radius"));
    }
    partial_sphere(origin, radius, 0.0, 360.0)
}

/// The solid cylinder of `radius` from the base circle centred at `origin` to
/// `height` along Z (`SmApiCreateCylinder`, `SmBrep*&` overload: [`create_cone`] with
/// equal radii).
///
/// # Errors
///
/// Those of [`create_cone`].
pub fn create_cylinder(origin: Point3, radius: f64, height: f64) -> Result<Brep> {
    create_cone(origin, radius, radius, height)
}

/// The cylinder surface (no caps) of `radius` about the Z axis through `origin`,
/// bounded to `[0, 360] x [0, height]` (`SmApiCreateCylinder`, `SmSurface*&`
/// overload).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] unless the radius and height are finite and above
/// `SM_EFF_ZERO`; those of [`Cone::create_canonical_cylinder`] and
/// [`Cone::adjust_step_uv_domain`].
pub fn create_cylinder_surface(
    k: &Kernel,
    origin: Point3,
    radius: f64,
    height: f64,
) -> Result<Cone> {
    if !is_positive_dim(radius) || !is_positive_dim(height) {
        return Err(SolidError::invalid_input(
            OP_CYLINDER,
            "non-positive radius or height",
        ));
    }
    let frame = xy_frame(origin);
    let mut cylinder = Cone::create_canonical_cylinder(k, &frame, radius)?;
    cylinder.adjust_step_uv_domain(k, &extent_from_zero(360.0, height))?;
    Ok(cylinder)
}

/// The solid cone from the base circle of `radius_base` centred at `origin` to the top
/// circle of `radius_top` at `height` along Z (`SmApiCreateCone`, `SmBrep*&`
/// overload: `CreateCone(height, radius_base, radius_top, 0, 360, frame)`).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite or negative radius, both radii not
/// above `SM_EFF_ZERO`, or a height not finite and above `SM_EFF_ZERO`; then
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateCone`).
pub fn create_cone(origin: Point3, radius_base: f64, radius_top: f64, height: f64) -> Result<Brep> {
    if !cone_dims_valid(radius_base, radius_top, height) {
        return Err(SolidError::invalid_input(
            OP_CONE,
            "invalid cone dimensions",
        ));
    }
    partial_cone(origin, radius_base, radius_top, height, 0.0, 360.0)
}

/// The cone surface from the base circle of `radius_base` centred at `origin` to the
/// top circle of `radius_top` at `height` along Z, bounded to `[0, 360] x [0, height]`
/// (`SmApiCreateCone`, `SmSurface*&` overload).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] as [`create_cone`]; those of [`Cone::create_canonical`]
/// and [`Cone::adjust_step_uv_domain`].
pub fn create_cone_surface(
    k: &Kernel,
    origin: Point3,
    radius_base: f64,
    radius_top: f64,
    height: f64,
) -> Result<Cone> {
    if !cone_dims_valid(radius_base, radius_top, height) {
        return Err(SolidError::invalid_input(
            OP_CONE_SURFACE,
            "invalid cone dimensions",
        ));
    }
    let frame = xy_frame(origin);
    let mut cone =
        Cone::create_canonical(k, &frame, radius_base, radius_top, height, false, false)?;
    cone.adjust_step_uv_domain(k, &extent_from_zero(360.0, height))?;
    Ok(cone)
}

/// The solid torus of `radius_major` and `radius_minor` centred at `origin` about Z
/// (`SmApiCreateTorus`, `SmBrep*&` overload: `CreateTorus(major, minor, 0, 360,
/// frame)`).
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateTorus`).
pub fn create_torus(origin: Point3, radius_major: f64, radius_minor: f64) -> Result<Brep> {
    partial_torus(origin, radius_major, radius_minor, 0.0, 360.0)
}

/// The torus surface of `radius_major` and `radius_minor` centred at `origin` about Z
/// (`SmApiCreateTorus`, `SmSurface*&` overload: `SmTorus::CreateCanonical` with the
/// default options and domain).
///
/// # Errors
///
/// Those of [`Torus::create_canonical`] (`SM_ERR` for a radius not above
/// `SM_EFF_ZERO`).
pub fn create_torus_surface(
    k: &Kernel,
    origin: Point3,
    radius_major: f64,
    radius_minor: f64,
) -> Result<Torus> {
    let frame = xy_frame(origin);
    Torus::create_canonical(
        k,
        &frame,
        radius_major,
        radius_minor,
        TorusOptions::default(),
        None,
    )
}

/// The solid pyramid on the square base of side `length` centred at `base_center` in
/// the XY plane, with its apex `height` above the centre along Z
/// (`SmApiCreatePyramid`).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a length or height not above `0` (NaN passes, as
/// upstream); those of [`Line::create_line_segment`] for the base edges (corners that
/// coincide); then [`SolidError::Unported`] (`SmBrep::CreatePlanarFaceWith3DCurves`).
pub fn create_pyramid(base_center: Point3, length: f64, height: f64) -> Result<Brep> {
    if length <= 0.0 || height <= 0.0 {
        return Err(SolidError::invalid_input(
            OP_PYRAMID,
            "non-positive length or height",
        ));
    }

    let half = length / 2.0;

    // Square base corners (CCW in the XY plane) and the apex above the center.
    let base = [
        base_center + Vec3::new(-half, -half, 0.0),
        base_center + Vec3::new(half, -half, 0.0),
        base_center + Vec3::new(half, half, 0.0),
        base_center + Vec3::new(-half, half, 0.0),
    ];
    let apex = base_center + Vec3::new(0.0, 0.0, height);
    let tol = 1.0e-5;

    // UNPORTED(topology.brep_p3): SmBrep::SmBrep

    // Square base face: c0 -> c1 -> c2 -> c3.
    let mut base_loop: Vec<Curve> = Vec::new();
    for (ii, start) in base.iter().enumerate() {
        let end = base.get((ii + 1) % 4).copied().unwrap_or(*start);
        base_loop.push(Curve::from(Line::create_line_segment(
            3, *start, end, None,
        )?));
    }
    let _ = (base_loop, apex, tol);
    // UNPORTED(topology.brep_p8): SmBrep::CreatePlanarFaceWith3DCurves
    // UNPORTED(topology.brep_p5): SmBrep::StitchAndOrient
    Err(SolidError::unported(
        "SmBrep::CreatePlanarFaceWith3DCurves",
        UNIT_BREP_P8,
    ))
}

/// The solid sector of the hollow cylinder of `inside_radius` and `outside_radius`
/// about the Z axis through `origin`, `length` along Z, from `start_angle_deg` to
/// `end_angle_deg` about Z from X (`SmApiCreateCylindricalBox`).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] unless the length and inside radius are finite and
/// above `SM_EFF_ZERO`, the outside radius is finite and above the inside radius plus
/// `SM_EFF_ZERO`, the start angle is finite in `[-360, 360]`, and the end angle is
/// finite, above the start plus `SM_EFF_ZERO` and at most 360 degrees past it; then
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateCylindricalBox`).
pub fn create_cylindrical_box(
    origin: Point3,
    length: f64,
    inside_radius: f64,
    outside_radius: f64,
    start_angle_deg: f64,
    end_angle_deg: f64,
) -> Result<Brep> {
    if !is_positive_dim(length)
        || !is_positive_dim(inside_radius)
        || !is_valid_double(outside_radius)
        || outside_radius <= inside_radius + EFF_ZERO
        || !is_valid_double(start_angle_deg)
        || !is_valid_double(end_angle_deg)
        || start_angle_deg < -360.0
        || start_angle_deg > 360.0
        || end_angle_deg <= start_angle_deg + EFF_ZERO
        || end_angle_deg - start_angle_deg > 360.0
    {
        return Err(SolidError::invalid_input(
            OP_CYLINDRICAL_BOX,
            "invalid cylindrical box dimensions",
        ));
    }
    let frame = xy_frame(origin);
    let _ = frame;
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::CreateCylindricalBox
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateCylindricalBox",
        UNIT_P1,
    ))
}

/// `SmPrimitiveCreation::CreateSphere(radius, start, end, frame)` into a new B-rep at
/// the XY frame of `origin` (the body shared by [`create_sphere`] and
/// [`create_partial_sphere`]).
fn partial_sphere(
    origin: Point3,
    radius: f64,
    start_angle_deg: f64,
    end_angle_deg: f64,
) -> Result<Brep> {
    let frame = xy_frame(origin);
    let _ = (frame, radius, start_angle_deg, end_angle_deg);
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateSphere
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateSphere",
        UNIT_P2,
    ))
}

/// The solid sphere sector of `radius` centred at `origin` from `start_angle_deg` to
/// `end_angle_deg` about Z from X (`SmApiCreatePartialSphere`; no input validation).
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateSphere`).
pub fn create_partial_sphere(
    origin: Point3,
    radius: f64,
    start_angle_deg: f64,
    end_angle_deg: f64,
) -> Result<Brep> {
    partial_sphere(origin, radius, start_angle_deg, end_angle_deg)
}

/// The solid sphere of `radius` (negative: inside out) centred at `center`, made of
/// patches without degenerate pole vertices (`SmApiCreateSphereNoPole`, tolerance
/// `SM_ZONE_TOL_3D`).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] unless the radius is finite and its magnitude above
/// `SM_EFF_ZERO`; then [`SolidError::Unported`]
/// (`SmPrimitiveCreation::CreateSphereNoPole`).
pub fn create_sphere_no_pole(center: Point3, radius: f64) -> Result<Brep> {
    if !is_non_zero_radius(radius) {
        return Err(SolidError::invalid_input(OP_SPHERE_NO_POLE, "zero radius"));
    }
    let _ = center;
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::CreateSphereNoPole
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateSphereNoPole",
        UNIT_P1,
    ))
}

/// `SmPrimitiveCreation::CreateCone(height, radius_base, radius_top, start, end,
/// frame)` into a new B-rep at the XY frame of `origin` (the body shared by
/// [`create_cone`] and [`create_partial_cone`]).
fn partial_cone(
    origin: Point3,
    radius_base: f64,
    radius_top: f64,
    height: f64,
    start_angle_deg: f64,
    end_angle_deg: f64,
) -> Result<Brep> {
    let frame = xy_frame(origin);
    let _ = (
        frame,
        height,
        radius_base,
        radius_top,
        start_angle_deg,
        end_angle_deg,
    );
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateCone
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateCone",
        UNIT_P2,
    ))
}

/// The solid cone sector from the base circle of `radius_base` centred at `origin` to
/// the top circle of `radius_top` at `height` along Z, from `start_angle_deg` to
/// `end_angle_deg` about Z from X (`SmApiCreatePartialCone`; no input validation).
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateCone`).
pub fn create_partial_cone(
    origin: Point3,
    radius_base: f64,
    radius_top: f64,
    height: f64,
    start_angle_deg: f64,
    end_angle_deg: f64,
) -> Result<Brep> {
    partial_cone(
        origin,
        radius_base,
        radius_top,
        height,
        start_angle_deg,
        end_angle_deg,
    )
}

/// The arguments of [`create_cone_elliptic_ends`] (`SmApiCreateConeEllipticEnds`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ConeEllipticEnds {
    /// `dFrontRadius`: the radius at the front end.
    pub front_radius: f64,
    /// `dRearRadius`: the radius at the rear end.
    pub rear_radius: f64,
    /// `dHeight`: the distance between the end-ellipse centres.
    pub height: f64,
    /// `dFrontYTiltDeg`: the front end's tilt about the X axis (degrees).
    pub front_y_tilt_deg: f64,
    /// `dRearYTiltDeg`: the rear end's tilt about the X axis (degrees).
    pub rear_y_tilt_deg: f64,
    /// `dFrontXTiltDeg`: the front end's tilt about the Y axis (degrees).
    pub front_x_tilt_deg: f64,
    /// `dRearXTiltDeg`: the rear end's tilt about the Y axis (degrees).
    pub rear_x_tilt_deg: f64,
    /// `bEndCaps`: close off the end caps.
    pub end_caps: bool,
}

/// The cone with elliptic ends and independent tilts (`SmApiCreateConeEllipticEnds`;
/// no input validation).
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateConeEllipticEnds`).
pub fn create_cone_elliptic_ends(params: &ConeEllipticEnds) -> Result<Brep> {
    let _ = params;
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::CreateConeEllipticEnds
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateConeEllipticEnds",
        UNIT_P1,
    ))
}

/// The solid cylinder sector of `radius` from the base circle centred at `origin` to
/// `height` along Z, from `start_angle_deg` to `end_angle_deg` about Z from X
/// (`SmApiCreatePartialCylinder`: [`create_partial_cone`] with equal radii).
///
/// # Errors
///
/// Those of [`create_partial_cone`].
pub fn create_partial_cylinder(
    origin: Point3,
    radius: f64,
    height: f64,
    start_angle_deg: f64,
    end_angle_deg: f64,
) -> Result<Brep> {
    create_partial_cone(
        origin,
        radius,
        radius,
        height,
        start_angle_deg,
        end_angle_deg,
    )
}

/// `SmPrimitiveCreation::CreateTorus(major, minor, start, end, frame)` into a new B-rep
/// at the XY frame of `origin` (the body shared by [`create_torus`] and
/// [`create_partial_torus`]).
fn partial_torus(
    origin: Point3,
    radius_major: f64,
    radius_minor: f64,
    start_angle_deg: f64,
    end_angle_deg: f64,
) -> Result<Brep> {
    let frame = xy_frame(origin);
    let _ = (
        frame,
        radius_major,
        radius_minor,
        start_angle_deg,
        end_angle_deg,
    );
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateTorus
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateTorus",
        UNIT_P2,
    ))
}

/// The solid torus sector of `radius_major` and `radius_minor` centred at `origin`
/// about Z, from `start_angle_deg` to `end_angle_deg` about Z from X
/// (`SmApiCreatePartialTorus`; no input validation).
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateTorus`).
pub fn create_partial_torus(
    origin: Point3,
    radius_major: f64,
    radius_minor: f64,
    start_angle_deg: f64,
    end_angle_deg: f64,
) -> Result<Brep> {
    partial_torus(
        origin,
        radius_major,
        radius_minor,
        start_angle_deg,
        end_angle_deg,
    )
}

/// The blend face between the boundary edges `edge1` (of `face1`) and `edge2` (of
/// `face2`) of `brep`, added to `brep` (`SmApiCreateBlendPrimitive`): from the start
/// of `edge1`'s curve to the start (`same_dir_curves`) or end of `edge2`'s curve.
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateBlendPrimitive`).
pub fn create_blend_primitive(
    brep: &mut Brep,
    edge1: EdgeId,
    face1: FaceId,
    edge2: EdgeId,
    face2: FaceId,
    same_dir_curves: bool,
) -> Result<FaceId> {
    let _ = (brep, edge1, face1, edge2, face2, same_dir_curves);
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreateBlendPrimitive
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateBlendPrimitive",
        UNIT_P2,
    ))
}

/// The swung solid of the XY path curves and XZ profile curves scaled by `scale`
/// (`SmApiCreateSwungPrimitive`, tolerance `SM_ZONE_TOL_3D`). The curves are not
/// consumed.
///
/// # Errors
///
/// [`SolidError::Unported`] (`SmPrimitiveCreation::CreateSwungPrimitive`).
pub fn create_swung_primitive(
    xy_curves: &[Curve],
    xz_curves: &[Curve],
    scale: f64,
) -> Result<Brep> {
    let _ = (xy_curves, xz_curves, scale);
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::SmPrimitiveCreation
    // UNPORTED(primitive.primitive_creation_p3): SmPrimitiveCreation::CreateSwungPrimitive
    Err(SolidError::unported(
        "SmPrimitiveCreation::CreateSwungPrimitive",
        UNIT_P3,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o() -> Point3 {
        Point3::new(0.0, 0.0, 0.0)
    }

    #[test]
    fn validators_match_upstream() {
        assert!(is_positive_dim(1.0));
        assert!(!is_positive_dim(EFF_ZERO));
        assert!(!is_positive_dim(f64::NAN));
        assert!(!is_positive_dim(f64::INFINITY));
        assert!(is_non_zero_radius(-5.0));
        assert!(!is_non_zero_radius(0.0));
        assert!(cone_dims_valid(0.0, 1.0, 1.0));
        assert!(!cone_dims_valid(0.0, 0.0, 1.0));
        assert!(!cone_dims_valid(-1.0, 10.0, 20.0));
        assert!(!cone_dims_valid(5.0, 10.0, -20.0));
    }

    #[test]
    fn invalid_inputs_are_rejected_before_the_kernel() {
        let k = Kernel::default();
        assert!(matches!(
            create_box(o(), 0.0, 10.0, 20.0),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            create_sphere(o(), 0.0),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            create_cylinder(o(), -1.0, 20.0),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            create_cylinder_surface(&k, o(), 5.0, -20.0),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            create_cone_surface(&k, o(), 5.0, 10.0, -20.0),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            create_pyramid(o(), 0.0, 1.0),
            Err(SolidError::InvalidInput { .. })
        ));
        for (a0, a1) in [(400.0, 450.0), (90.0, 10.0), (45.0, 45.0), (0.0, 400.0)] {
            assert!(matches!(
                create_cylindrical_box(o(), 10.0, 2.0, 5.0, a0, a1),
                Err(SolidError::InvalidInput { .. })
            ));
        }
        assert!(matches!(
            create_sphere_no_pole(o(), 0.0),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    #[test]
    fn valid_brep_inputs_reach_the_unported_kernel() {
        assert!(create_box(o(), 5.0, 10.0, 20.0).is_err_and(|e| e.is_unported()));
        assert!(create_sphere_no_pole(o(), -5.0).is_err_and(|e| e.is_unported()));
        assert!(create_pyramid(o(), 2.0, 3.0).is_err_and(|e| e.is_unported()));
    }

    #[test]
    fn surfaces_are_bounded_as_upstream() {
        let k = Kernel::default();
        let plane = create_plane_surface(o(), 10.0, 15.0).unwrap();
        let d = plane.step_uv_domain();
        assert_eq!((d.max().x, d.max().y), (10.0, 15.0));
        assert!(create_plane_surface(o(), 0.0, 0.0).is_err());

        let cyl = create_cylinder_surface(&k, o(), 5.0, 20.0).unwrap();
        assert!(cyl.is_cylinder_class());
        let d = cyl.rev().step_uv_domain();
        assert_eq!((d.max().x, d.max().y), (360.0, 20.0));

        let cone = create_cone_surface(&k, o(), 5.0, 10.0, 20.0).unwrap();
        assert!((cone.top_radius() - 10.0).abs() < 1e-12);

        let torus = create_torus_surface(&k, o(), 10.0, 2.0).unwrap();
        assert_eq!(torus.major_radius(), 10.0);
        assert!(create_torus_surface(&k, o(), 5.0, 0.0).is_err());
    }

    #[test]
    fn extreme_inputs_do_not_panic() {
        // Inputs the difftest fences off on the C++ side (upstream crashes there).
        let k = Kernel::default();
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        let _ = create_plane_surface(o(), nan, 1.0);
        let _ = create_plane_surface(o(), 1.0, nan);
        let _ = create_torus_surface(&k, o(), inf, 1.0);
        let _ = create_torus_surface(&k, o(), 1.0, inf);
        let _ = create_torus_surface(&k, o(), 5.0, f64::MAX);
        let _ = create_torus_surface(&k, o(), f64::MAX, f64::MAX);
        let _ = create_torus_surface(&k, Point3::new(nan, 0.0, inf), 3.0, 1.0);
        let _ = create_cylinder_surface(&k, Point3::new(nan, inf, 0.0), 1.0, 1.0e17);
        let _ = create_cone_surface(&k, Point3::new(1.0e300, 0.0, 0.0), 1.0e17, 0.0, 1.0e-11);
        let _ = create_pyramid(Point3::new(nan, 0.0, 0.0), nan, 1.0);
        let _ = create_pyramid(Point3::new(1.0e17, 0.0, 0.0), 1.0, 1.0);
        let _ = create_cylindrical_box(o(), 1.0, 1.0, 2.0, 0.0, 360.0);
        let _ = create_sphere_no_pole(o(), 1.0e18);
    }
}
