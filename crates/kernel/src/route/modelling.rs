//! The router's body-making operations ([`solidcraft_kernel_api::Modelling`]).

use solidcore_record::{Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::{Axis2, BoolOp, ChamferSide, DraftCorner, ExtrudeSides, FilletTurn, Modelling, OpKind, Primitive, RevolveSides, Taper};

use super::{Which, which};
use crate::Result;
use crate::backend::Solidcore;
use crate::body::Body;

/// The axis-aligned box between two opposite corners.
pub fn box_solid(a: Vec3, b: Vec3) -> Result<Body> {
    primitive(&Primitive::Box { a, b })
}

/// A cylinder standing on `base`, along `axis`, `height` long.
pub fn cylinder(base: Vec3, axis: Vec3, radius: f64, height: f64) -> Result<Body> {
    primitive(&Primitive::Cylinder { base, axis, radius, height })
}

/// A sphere.
pub fn sphere(center: Vec3, radius: f64) -> Result<Body> {
    primitive(&Primitive::Sphere { center, radius })
}

/// A ring torus about the z axis through `center`; needs `major > minor > 0`.
pub fn torus(center: Vec3, major: f64, minor: f64) -> Result<Body> {
    primitive(&Primitive::Torus { center, major, minor })
}

fn primitive(p: &Primitive) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::of_primitive(p), &[], |k, _| k.primitive(p).map(Some)) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the primitive is empty".into())));
    }
    match which(OpKind::of_primitive(p)) {
        Which::Solidcore => Solidcore.primitive(p),
    }
}

/// `regions` on `plane`, extruded from `start` to `end` along the plane's normal: one body per
/// region that stays apart.
pub fn extrude(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
    extruded(plane, regions, start, end, ExtrudeSides::AsDrawn)
}

/// [`extrude`] with each run of profile lines that continue one another in a straight line swept
/// into one side face, and each run of free-form segments that `one_curve` says are pieces of one
/// curve swept as that curve into one wall ([`ExtrudeSides::Whole`]). For the Extrude feature;
/// other builders keep a face per segment.
pub fn extrude_whole_sides(plane: &Plane, regions: &[Region2], start: f64, end: f64, one_curve: &dyn Fn(&Seg2, &Seg2) -> bool) -> Result<Vec<Body>> {
    extruded(plane, regions, start, end, ExtrudeSides::Whole(one_curve))
}

fn extruded(plane: &Plane, regions: &[Region2], start: f64, end: f64, sides: ExtrudeSides<'_>) -> Result<Vec<Body>> {
    match which(OpKind::Extrude) {
        Which::Solidcore => Solidcore.extrude(plane, regions, start, end, sides),
    }
}

/// The material of `regions` on `plane` (overlapping ones merged) extruded from `start` to `end`
/// along its normal, as one body.
pub fn extrude_planar_regions(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Body> {
    match which(OpKind::Extrude) {
        Which::Solidcore => Solidcore.extrude_material(plane, regions, start, end),
    }
}

/// `region` extruded `length` along `dir_sign` × the plane's normal with its sides sloping by
/// `taper` (radians), sharp corners closed as `corner` says.
pub fn extrude_tapered(plane: &Plane, region: &Region2, length: f64, dir_sign: f64, taper: f64, corner: DraftCorner) -> Result<Body> {
    extrude_tapered_between(plane, region, 0.0, length, 0.0, dir_sign, taper, corner)
}

/// [`extrude_tapered`] from `from` to `to`, the region being the section at 0, standing straight
/// for `foot` above `from` before the sides slope ([`Taper`]).
#[allow(clippy::too_many_arguments)]
pub fn extrude_tapered_between(
    plane: &Plane,
    region: &Region2,
    from: f64,
    to: f64,
    foot: f64,
    dir_sign: f64,
    taper: f64,
    corner: DraftCorner,
) -> Result<Body> {
    let taper = Taper { from, to, foot, dir_sign, angle: taper, corner };
    match which(OpKind::Extrude) {
        Which::Solidcore => Solidcore.extrude_tapered(plane, region, &taper),
    }
}

/// `regions` on `plane`, turned by `angle` (radians) about the axis through `axis_origin` along
/// `axis_dir` (plane coordinates).
pub fn revolve(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    revolved(plane, regions, axis_origin, axis_dir, angle, false, RevolveSides::AsDrawn)
}

/// [`revolve`], as the whole revolution's form where the angle is a full turn.
pub fn revolve_whole(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    revolved(plane, regions, axis_origin, axis_dir, angle, true, RevolveSides::AsDrawn)
}

/// [`revolve`] with contiguous pieces of the same circle joined before turning each profile.
pub fn revolve_joining_arcs(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    revolved(plane, regions, axis_origin, axis_dir, angle, false, RevolveSides::JoinArcs)
}

/// [`revolve`] with each run of free-form segments that `one_curve` says are pieces of one curve
/// turned as that curve into one face ([`RevolveSides::JoinFreeForm`]).
pub fn revolve_whole_sides(
    plane: &Plane,
    regions: &[Region2],
    axis_origin: Vec2,
    axis_dir: Vec2,
    angle: f64,
    one_curve: &dyn Fn(&Seg2, &Seg2) -> bool,
) -> Result<Vec<Body>> {
    revolved(plane, regions, axis_origin, axis_dir, angle, false, RevolveSides::JoinFreeForm(one_curve))
}

fn revolved(plane: &Plane, regions: &[Region2], origin: Vec2, dir: Vec2, angle: f64, whole: bool, sides: RevolveSides<'_>) -> Result<Vec<Body>> {
    match which(OpKind::Revolve) {
        Which::Solidcore => Solidcore.revolve(plane, regions, Axis2 { origin, dir }, angle, whole, sides),
    }
}

/// Boolean of two bodies. The result may be empty (`Ok(None)`) for a cut that removes
/// everything or an intersection of disjoint bodies. The result carries its exact history
/// ([`Body::history`]) over the operands `[a, b]` where the kernel reports it.
pub fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Boolean, &[a, b], |k, f| match f {
        [fa, fb] => k.boolean(fa, fb, op),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("a boolean takes two bodies".into())),
    }) {
        return made;
    }
    match which(OpKind::Boolean) {
        Which::Solidcore => Solidcore.boolean(a, b, op),
    }
}

/// Constant-radius fillet of the edges nearest to the given points. A radius too large for the
/// faces around the edges is refused with [`crate::KernelError::TooLarge`] ([`crate::fillet_max`]
/// finds the largest that works).
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    #[cfg(feature = "backend-occt")]
    if let Some(made) = super::foreign::run(&crate::backend::Occt, OpKind::Fillet, &[body], |k, f| match f {
        [fb] => k.fillet(fb, edges, radius, FilletTurn::Mitre).map(Some),
        _ => Err(solidcraft_kernel_api::BackendError::Invalid("a fillet takes one body".into())),
    }) {
        return made.and_then(|b| b.ok_or_else(|| crate::KernelError::Failed("the fillet is empty".into())));
    }
    match which(OpKind::Fillet) {
        Which::Solidcore => Solidcore.fillet(body, edges, radius, FilletTurn::Mitre),
    }
}

/// An equal chamfer of the edges nearest to the given points.
pub fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    match which(OpKind::Chamfer) {
        Which::Solidcore => Solidcore.chamfer(body, edges, distance),
    }
}

/// A chamfer with different setbacks on the two faces of each edge: `d1` along the first face
/// (the one facing up most, the other with `flip`), the second side by distance or angle.
pub fn chamfer_sides(body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
    match which(OpKind::Chamfer) {
        Which::Solidcore => Solidcore.chamfer_sides(body, edges, d1, side, flip),
    }
}

/// The body hollowed to `thickness`, open at the faces nearest `open`.
pub fn shell(b: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
    match which(OpKind::Shell) {
        Which::Solidcore => Solidcore.shell(b, open, thickness),
    }
}

/// A sheet thickened to `thickness`, to both sides when `symmetric`.
pub fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    match which(OpKind::Thicken) {
        Which::Solidcore => Solidcore.thicken(body, thickness, symmetric),
    }
}

/// The faces nearest `at` moved `distance` along their normals.
pub fn offset_faces(b: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
    match which(OpKind::OffsetFaces) {
        Which::Solidcore => Solidcore.offset_faces(b, at, distance),
    }
}
