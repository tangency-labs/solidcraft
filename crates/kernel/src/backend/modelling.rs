//! solidcore's body-making operations ([`Modelling`]): each the call this crate made before the
//! interface existed.

use solidcore_record::{Loop2, Plane, Region2, Vec3};
use solidcraft_kernel_api::{Axis2, BoolOp, ChamferSide, ExtrudeSides, FilletTurn, Modelling, Primitive, RevolveSides, Taper, TrimKeep};
use solidcraft_kernel_api::{Helix, PathSpan, PointPath, RailFit, SweepOptions};

use crate::SplitTool;

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

    fn extrude(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64, sides: ExtrudeSides<'_>) -> Result<Vec<Body>> {
        match sides {
            ExtrudeSides::AsDrawn => crate::build::extrude(plane, regions, start, end),
            ExtrudeSides::Whole(one_curve) => crate::build::extrude_whole_sides(plane, regions, start, end, one_curve),
        }
    }

    fn extrude_material(&self, plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Body> {
        crate::prism::extrude_planar_regions(plane, regions, start, end)
    }

    fn extrude_tapered(&self, plane: &Plane, region: &Region2, t: &Taper) -> Result<Body> {
        crate::build::extrude_tapered_between(plane, region, t.from, t.to, t.foot, t.dir_sign, t.angle, t.corner)
    }

    fn revolve(&self, plane: &Plane, regions: &[Region2], axis: Axis2, angle: f64, whole: bool, sides: RevolveSides<'_>) -> Result<Vec<Body>> {
        let (o, d) = (axis.origin, axis.dir);
        match sides {
            RevolveSides::AsDrawn if whole => crate::build::revolve_whole(plane, regions, o, d, angle),
            RevolveSides::AsDrawn => crate::build::revolve(plane, regions, o, d, angle),
            RevolveSides::JoinArcs => crate::build::revolve_joining_arcs(plane, regions, o, d, angle),
            RevolveSides::JoinFreeForm(one_curve) => crate::build::revolve_whole_sides(plane, regions, o, d, angle, one_curve),
        }
    }

    fn boolean(&self, a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
        crate::ops::boolean(a, b, op)
    }

    fn fillet(&self, body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Body> {
        crate::blend::fillet_turning(body, edges, radius, turn)
    }

    fn fillet_removing(&self, body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<(Body, Vec<Vec3>)> {
        crate::blend::fillet_removing(body, edges, radius, turn)
    }

    fn fillet_each(&self, body: &Body, edges: &[Vec3], radii: &[f64], turn: FilletTurn) -> Result<Body> {
        crate::blend::fillet_each(body, edges, radii, turn)
    }

    fn fillet_setback(&self, body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)]) -> Result<Body> {
        crate::blend::fillet_setback(body, edges, radius, setbacks)
    }

    fn chamfer(&self, body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
        crate::blend::chamfer(body, edges, distance)
    }

    fn chamfer_carrying_face(&self, body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
        crate::blend::chamfer_carrying_face(body, edges, distance)
    }

    fn chamfer_sides_carrying_face(&self, body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
        crate::blend::chamfer_sides_carrying_face(body, edges, d1, side, flip)
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

    fn transform_face(&self, body: &Body, at: Vec3, translate: Vec3, origin: Vec3, axis: Vec3, angle: f64) -> Result<Body> {
        crate::offset_backend::transform_face(body, at, translate, origin, axis, angle)
    }

    fn draft(&self, body: &Body, faces: &[Vec3], neutral: &Plane, pull: Vec3, angle: f64) -> Result<Body> {
        crate::polyhedron::draft(body, faces, neutral, pull, angle)
    }

    fn split_faces(&self, body: &Body, faces: &[usize], tool: &SplitTool<'_>) -> Result<(Body, Vec<usize>)> {
        crate::splitface::split_faces_with_map(body, faces, tool)
    }

    fn split_body(&self, body: &Body, tool: &SplitTool<'_>) -> Result<Vec<Body>> {
        crate::splitface::split_body(body, tool)
    }

    fn split_body_by_faces(&self, body: &Body, sheet: &Body, face: usize, faces: &[usize]) -> Result<Vec<Body>> {
        crate::splitface::split_body_by_faces(body, sheet, face, faces)
    }

    fn split_body_by_sheet_where_it_cuts(&self, body: &Body, sheet: &Body, face: usize) -> Result<Option<Vec<Body>>> {
        crate::splitface::split_body_by_sheet_where_it_cuts(body, sheet, face)
    }

    fn trim_keep(&self, body: &Body, tool: &SplitTool<'_>, keep: TrimKeep<'_>) -> Result<Body> {
        crate::surface_trim::trim_keep(body, tool, keep)
    }

    fn sweep(&self, plane: &Plane, region: &Region2, path: &[PathSpan], options: &SweepOptions) -> Result<Body> {
        crate::build::sweep_with(plane, region, &crate::sweep_api::segs(path)?, &crate::sweep_api::control(options))
    }

    fn sweep_points(&self, plane: &Plane, region: &Region2, path: &PointPath, options: &SweepOptions) -> Result<Body> {
        crate::point_path::sweep_point_path_with(plane, region, &crate::sweep_api::path_of_points(path)?, &crate::sweep_api::control(options))
    }

    fn sweep_helix(&self, plane: &Plane, region: &Region2, h: &Helix<'_>) -> Result<Body> {
        match h.law {
            None => crate::helix::sweep_helix(plane, region, h.origin, h.axis, h.pitch, h.turns),
            Some(law) => crate::helix::sweep_helix_radial(plane, region, h.origin, h.axis, h.pitch, h.turns, law, h.continuous_flanks),
        }
    }

    fn loft(&self, sections: &[(Plane, Loop2)], rails: &[Vec<Vec3>], fit: RailFit) -> Result<Body> {
        crate::build::loft_with_rails(sections, rails, fit)
    }

    fn loft_regions(&self, sections: &[(Plane, Region2)]) -> Result<Body> {
        crate::build::loft_regions(sections)
    }

    fn loft_to_point(&self, plane: &Plane, section: &Loop2, apex: Vec3, rails: &[Vec<Vec3>], fit: RailFit) -> Result<Body> {
        crate::helix::loft_to_point_with_rails(plane, section, apex, rails, fit)
    }
}
