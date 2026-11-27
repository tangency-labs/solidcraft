//! solidcore's body-making operations ([`Modelling`]): each the call this crate made before the
//! interface existed.

use solidcore_record::{Plane, Region2, Vec3};
use solidcraft_kernel_api::{Axis2, BoolOp, ChamferSide, FilletTurn, Modelling, Primitive};

use super::Solidcore;
use crate::Result;
use crate::body::Body;

impl Modelling for Solidcore {
    fn primitive(&self, p: &Primitive) -> Result<Body> {
        match *p {
            Primitive::Box { a, b } => crate::build::box_solid(a, b),
            Primitive::Cylinder { base, axis, radius, height } => crate::build::cylinder(base, axis, radius, height),
            Primitive::Sphere { center, radius } => crate::build::sphere(center, radius),
            Primitive::Torus { center, major, minor } => crate::build::torus(center, major, minor),
        }
    }

    fn extrude(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
        crate::build::extrude(plane, regions, start, end)
    }

    fn revolve(&self, plane: &Plane, regions: &[Region2], axis: Axis2, angle: f64, whole: bool) -> Result<Vec<Body>> {
        if whole {
            crate::build::revolve_whole(plane, regions, axis.origin, axis.dir, angle)
        } else {
            crate::build::revolve(plane, regions, axis.origin, axis.dir, angle)
        }
    }

    fn boolean(&self, a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
        crate::ops::boolean(a, b, op)
    }

    fn fillet(&self, body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Body> {
        crate::blend::fillet_turning(body, edges, radius, turn)
    }

    fn chamfer(&self, body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
        crate::blend::chamfer(body, edges, distance)
    }

    fn chamfer_sides(&self, body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
        crate::blend::chamfer_sides(body, edges, d1, side, flip)
    }

    fn shell(&self, body: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
        crate::polyhedron::shell(body, open, thickness)
    }

    fn thicken(&self, sheet: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
        crate::surfaces::thicken(sheet, thickness, symmetric)
    }

    fn offset_faces(&self, body: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
        crate::polyhedron::offset_faces(body, at, distance)
    }
}
