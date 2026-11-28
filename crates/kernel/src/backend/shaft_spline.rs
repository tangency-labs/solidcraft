//! solidcore's shaft splines ([`ShaftSplines`]): each the solidcore-ops call this crate's callers
//! made before the interface existed; its values and refusals are the record's already.

use solidcore_ops::{round_face, shaft_spline};
use solidcore_record::round_face::End;
use solidcore_record::shaft_spline::{
    AxisGap, ChamferRefusal, ChamferSection, CutterSection, EndChamfer, OutlineLoop, RoundFace, SeatFrame, SeatRefusal, SpaceRegion, SpaceSection,
    SplineNumbers, SplinePlan, SplineRefusal, ToothOutline,
};
use solidcore_record::{Plane, Vec3};
use solidcraft_kernel_api::shaft_spline::ShaftSplines;

use super::Solidcore;

impl ShaftSplines for Solidcore {
    fn plan(&self, numbers: &SplineNumbers) -> Result<SplinePlan, SplineRefusal> {
        shaft_spline::plan(numbers)
    }

    fn fit_to_shaft_within(&self, numbers: &SplineNumbers, radius: f64) -> SplineNumbers {
        shaft_spline::fit_to_shaft_within(numbers, radius)
    }

    fn module_for_major_radius(&self, teeth: f64, addendum: f64, profile_shift: f64, radius: f64) -> Option<f64> {
        shaft_spline::module_for_major_radius(teeth, addendum, profile_shift, radius)
    }

    fn same_size(&self, radius: f64) -> f64 {
        shaft_spline::same_size(radius)
    }

    fn tool_margin(&self, depth: f64) -> f64 {
        shaft_spline::tool_margin(depth)
    }

    fn cutter_radius(&self, diameter: f64, root: f64, out_to: f64, margin: f64) -> Result<f64, SplineRefusal> {
        shaft_spline::cutter_radius(diameter, root, out_to, margin)
    }

    fn space_section(&self, outline: &ToothOutline, tooth_at: f64, out_to: f64) -> Result<SpaceSection, SplineRefusal> {
        shaft_spline::space_section(outline, tooth_at, out_to)
    }

    fn outline_points(&self, outline: &ToothOutline, turn: f64) -> Result<Vec<[f64; 2]>, SplineRefusal> {
        shaft_spline::outline_points(outline, turn)
    }

    fn bezier_pieces(&self, degree: usize, knots: &[f64], control: &[[f64; 2]]) -> Result<Vec<Vec<[f64; 2]>>, SplineRefusal> {
        shaft_spline::bezier_pieces(degree, knots, control)
    }

    fn off_cylinder(&self, face: &RoundFace, p: Vec3) -> f64 {
        shaft_spline::off_cylinder(face, p)
    }

    fn seat_frame(&self, face: &RoundFace, pick: Vec3, start: Option<Vec3>) -> Result<SeatFrame, SeatRefusal> {
        shaft_spline::seat_frame(face, pick, start)
    }

    fn start_candidates(&self, face: &RoundFace, pick: Vec3, bore: bool) -> Option<[Vec3; 4]> {
        shaft_spline::start_candidates(face, pick, bore)
    }

    fn moved_out(&self, plane: &Plane, at: Vec3, by: f64) -> Vec3 {
        shaft_spline::moved_out(plane, at, by)
    }

    fn axis_gap(&self, axis_point: Vec3, axis: Vec3, point: Vec3, dir: Vec3) -> AxisGap {
        shaft_spline::axis_gap(axis_point, axis, point, dir)
    }

    fn copy_turn(&self, from: f64, to: f64, back: bool) -> f64 {
        shaft_spline::copy_turn(from, to, back)
    }

    fn turn_tells_sense(&self, from: f64, to: f64) -> bool {
        shaft_spline::turn_tells_sense(from, to)
    }

    fn lies_turned_back(&self, plane: &Plane, min: Vec3, max: Vec3, from: f64, to: f64) -> bool {
        shaft_spline::lies_turned_back(plane, min, max, from, to)
    }

    fn quarter_turn_in(&self, inward: f64, back: bool) -> f64 {
        shaft_spline::quarter_turn_in(inward, back)
    }

    fn lies_inward(&self, plane: &Plane, inward: f64, min: Vec3, max: Vec3) -> bool {
        shaft_spline::lies_inward(plane, inward, min, max)
    }

    fn points_apart(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        shaft_spline::points_apart(a, b)
    }

    fn outline_loop(&self, outline: &ToothOutline, turn: f64) -> Result<OutlineLoop, SplineRefusal> {
        shaft_spline::outline_loop(outline, turn)
    }

    fn tooth_at(&self, teeth: u32, zero: f64, at: f64) -> u32 {
        shaft_spline::tooth_at(teeth, zero, at)
    }

    fn cutter_section(&self, space: &SpaceSection, root: f64, cutter_radius: f64, out_to: f64) -> Result<Option<CutterSection>, SplineRefusal> {
        shaft_spline::cutter_section(space, root, cutter_radius, out_to)
    }

    fn space_region(&self, space: &SpaceSection, out_to: f64) -> Result<SpaceRegion, SplineRefusal> {
        shaft_spline::space_region(space, out_to)
    }

    fn chamfer_section(&self, chamfer: &EndChamfer, margin: f64, outside: bool) -> Result<ChamferSection, ChamferRefusal> {
        shaft_spline::chamfer_section(chamfer, margin, outside)
    }

    fn chamfer_plane(&self, plane: &Plane, inward: f64, seam: f64) -> Plane {
        shaft_spline::chamfer_plane(plane, inward, seam)
    }

    fn far_end(&self, seat: &SeatFrame) -> SeatFrame {
        round_face::far_end(seat)
    }

    fn shut_end(&self, radius: f64) -> End {
        round_face::shut_end(radius)
    }

    fn end_run_out(&self, end: &End) -> f64 {
        round_face::end_run_out(end)
    }

    fn end_run_past(&self, end: &End, margin: f64) -> f64 {
        round_face::end_run_past(end, margin)
    }

    fn end_carried(&self, end: &End, from: f64, to: f64) -> f64 {
        round_face::end_carried(end, from, to)
    }
}
