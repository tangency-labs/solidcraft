//! The router's body-making operations ([`solidcraft_kernel_api::Modelling`]).

use solidcore_record::{Plane, Region2, Vec2, Vec3};
use solidcraft_kernel_api::{Axis2, BoolOp, ChamferSide, FilletTurn, Modelling, OpKind, Primitive};

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
    match which(OpKind::of_primitive(p)) {
        Which::Solidcore => Solidcore.primitive(p),
    }
}

/// `regions` on `plane`, extruded from `start` to `end` along the plane's normal: one body per
/// region that stays apart.
pub fn extrude(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
    match which(OpKind::Extrude) {
        Which::Solidcore => Solidcore.extrude(plane, regions, start, end),
    }
}

/// `regions` on `plane`, turned by `angle` (radians) about the axis through `axis_origin` along
/// `axis_dir` (plane coordinates).
pub fn revolve(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    match which(OpKind::Revolve) {
        Which::Solidcore => Solidcore.revolve(plane, regions, Axis2 { origin: axis_origin, dir: axis_dir }, angle, false),
    }
}

/// [`revolve`], as the whole revolution's form where the angle is a full turn.
pub fn revolve_whole(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    match which(OpKind::Revolve) {
        Which::Solidcore => Solidcore.revolve(plane, regions, Axis2 { origin: axis_origin, dir: axis_dir }, angle, true),
    }
}

/// Boolean of two bodies. The result may be empty (`Ok(None)`) for a cut that removes
/// everything or an intersection of disjoint bodies. The result carries its exact history
/// ([`Body::history`]) over the operands `[a, b]` where the kernel reports it.
pub fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    match which(OpKind::Boolean) {
        Which::Solidcore => Solidcore.boolean(a, b, op),
    }
}

/// Constant-radius fillet of the edges nearest to the given points. A radius too large for the
/// faces around the edges is refused with [`crate::KernelError::TooLarge`] ([`crate::fillet_max`]
/// finds the largest that works).
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
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
