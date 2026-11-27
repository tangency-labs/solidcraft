//! Shaft splines ([`ShaftSplines`]): a spline on a shaft and in its hub planned from plain numbers,
//! what a caller needs of an outline to cut it, where a seat lies on a round face, and how the
//! stretch past a round face's end is carried. The values and refusals are the record's
//! (`solidcore_record::shaft_spline`, `solidcore_record::round_face`).

use solidcore_record::round_face::End;
use solidcore_record::shaft_spline::{
    AxisGap, ChamferRefusal, ChamferSection, CutterSection, EndChamfer, OutlineLoop, RoundFace, SeatFrame, SeatRefusal, SpaceRegion, SpaceSection,
    SplineNumbers, SplinePlan, SplineRefusal, ToothOutline,
};
use solidcore_record::{Plane, Vec3};

use crate::Backend;

/// The shaft-spline planning of one kernel ([`crate::OpKind::ShaftSpline`]).
pub trait ShaftSplines: Backend {
    /// What the numbers make: the shaft's outline and the hub's, the numbers they come to, the master
    /// spline as placed, and the angles a caller turns its tools by (the seams, the spaced teeth).
    ///
    /// # Errors
    ///
    /// `SplineRefusal::Refused`: a number is refused (not finite, out of range, missing for this
    /// kind of teeth), or the numbers together make no teeth; the refusal names the number to change.
    fn plan(&self, numbers: &SplineNumbers) -> Result<SplinePlan, SplineRefusal>;
    /// The numbers of a spline cut on a shaft that is there, whose radius is `radius`: the size left
    /// out is the shaft's own, and a size given that is the shaft's own but for its last places is
    /// the shaft's own too. Any other size given stays. The size is the module of involute teeth and
    /// the major diameter of straight-sided teeth and serrations; no other number changes. The
    /// numbers given are left as they are; the fitted ones are returned.
    fn fit_to_shaft_within(&self, numbers: &SplineNumbers, radius: f64) -> SplineNumbers;
    /// The module that puts the tips of involute teeth on the circle of radius `radius` (mm), with
    /// the addendum and the profile shift in modules. None where no positive finite module does.
    fn module_for_major_radius(&self, teeth: f64, addendum: f64, profile_shift: f64, radius: f64) -> Option<f64>;
    /// How far a radius of the spline may be from a picked face's own and still be taken as the same
    /// (mm).
    fn same_size(&self, radius: f64) -> f64;
    /// How far the tools that cut a spline reach past the faces they would otherwise only touch, for
    /// teeth `depth` deep (mm).
    fn tool_margin(&self, depth: f64) -> f64;
    /// The radius of the cutter of diameter `diameter` that ends the tooth spaces of a shaft, whose
    /// section runs from the radius `root` out to `out_to` with `margin` to spare.
    ///
    /// # Errors
    ///
    /// `SplineRefusal::Refused` naming the cutter's diameter: it is not a number the kernel takes,
    /// or the cutter does not reach past the depth it cuts.
    fn cutter_radius(&self, diameter: f64, root: f64, out_to: f64, margin: f64) -> Result<f64, SplineRefusal>;
    /// The section of the tooth space of the shaft `outline` after the tooth whose top lies nearest
    /// the angle `tooth_at` (radians), closed above by the circle of radius `out_to` about the axis,
    /// with the outline's tolerance.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: the angle is not a number, the radius is not above the outline's largest,
    /// or no top of a tooth lies near the angle.
    fn space_section(&self, outline: &ToothOutline, tooth_at: f64, out_to: f64) -> Result<SpaceSection, SplineRefusal>;
    /// The whole `outline` as a closed chain of points, counter-clockwise, without repeating the
    /// first point, with chords within the outline's tolerance of each curve, turned about the axis
    /// by `turn` (radians).
    ///
    /// # Errors
    ///
    /// The kernel's refusal: the outline or the turn is not finite, or the outline would need more
    /// points than the kernel makes.
    fn outline_points(&self, outline: &ToothOutline, turn: f64) -> Result<Vec<[f64; 2]>, SplineRefusal>;
    /// A B-spline as its polynomial pieces between knots, in order, each as `degree + 1` control
    /// points of a Bézier segment. A piece starts at the very point the piece before it ends at.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: the knots and control points do not describe a spline.
    fn bezier_pieces(&self, degree: usize, knots: &[f64], control: &[[f64; 2]]) -> Result<Vec<Vec<[f64; 2]>>, SplineRefusal>;
    /// How far `p` is from the cylinder of the round face: the distance of `p` from the axis line,
    /// less the radius, as an absolute value (not the distance past the face's ends).
    fn off_cylinder(&self, face: &RoundFace, p: Vec3) -> f64;
    /// The seat of a spline on `face`: the free end is the end nearest along the axis to `start`, or
    /// with none the end nearest `pick`; the plane square to the axis there, angle 0 from the world
    /// axes; and the pick brought onto the face's cylinder.
    ///
    /// # Errors
    ///
    /// `SeatRefusal`: the axis has no direction, no plane square to it can be made, or the face has
    /// no length or no radius.
    fn seat_frame(&self, face: &RoundFace, pick: Vec3, start: Option<Vec3>) -> Result<SeatFrame, SeatRefusal>;
    /// The points a spline's start is looked for at: in the plane of the end of the face nearest
    /// `pick`, along angle 0, half, a quarter and a tenth of the radius off the rim (inside the rim of
    /// a shaft, outside the rim of a `bore`), then the point on the axis. None where the axis has no
    /// angle 0.
    fn start_candidates(&self, face: &RoundFace, pick: Vec3, bore: bool) -> Option<[Vec3; 4]>;
    /// `at` moved `by` straight out from the axis (the line through the plane's origin along its
    /// normal); `at` itself where it lies on the axis.
    fn moved_out(&self, plane: &Plane, at: Vec3, by: f64) -> Vec3;
    /// How the line through `point` along `dir` lies to the axis through `axis_point` along `axis`
    /// (unit vectors).
    fn axis_gap(&self, axis_point: Vec3, axis: Vec3, point: Vec3, dir: Vec3) -> AxisGap;
    /// The angle a tool made about the middle line at the angle `from` is turned by about the axis to
    /// lie about the middle line at `to` (radians), the other way where `back`.
    fn copy_turn(&self, from: f64, to: f64, back: bool) -> f64;
    /// Whether a copy turned from the middle line at `from` to the one at `to` tells the two senses of
    /// turning apart by where it lies.
    fn turn_tells_sense(&self, from: f64, to: f64) -> bool;
    /// Whether a copy turned by `copy_turn` from `from` to `to` (not back), whose box is
    /// `min`..`max`, lies about the mirror of `to` instead, so that the turns go the other way.
    fn lies_turned_back(&self, plane: &Plane, min: Vec3, max: Vec3, from: f64, to: f64) -> bool;
    /// The quarter turn that takes a section on the plane into the shaft (radians), the other way
    /// where `back`.
    fn quarter_turn_in(&self, inward: f64, back: bool) -> f64;
    /// Whether the box `min`..`max` of a turned tool lies past the plane into the shaft, along the
    /// plane's normal times `inward`.
    fn lies_inward(&self, plane: &Plane, inward: f64, min: Vec3, max: Vec3) -> bool;
    /// The distance between two points of an outline.
    fn points_apart(&self, a: [f64; 2], b: [f64; 2]) -> f64;
    /// The whole `outline` as one closed loop of profile segments, every tooth turned to its place
    /// and the whole turned by `turn` (radians), with where two segments of one curve meet and each
    /// piece as it lies in the loop. A spline of a degree higher than a profile segment holds becomes
    /// pieces within the outline's tolerance.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: `solidcore_record::shaft_spline::PlanRefusal::NotAProfile` for a curve
    /// that is not fitted or not a spline.
    fn outline_loop(&self, outline: &ToothOutline, turn: f64) -> Result<OutlineLoop, SplineRefusal>;
    /// The number of the tooth whose top is nearest the angle `at` (radians), for the names of an
    /// outline loop's pieces (`outline_loop`): `teeth` teeth spaced evenly round the axis, tooth
    /// 0's top at the angle `zero`. Tooth 0 where there are no teeth or the angles are not numbers.
    fn tooth_at(&self, teeth: u32, zero: f64, at: f64) -> u32;
    /// The section a cutter of radius `cutter_radius` takes out of the tooth space `space` where it
    /// leaves the shaft, out to `out_to` from the axis; with the corners as they lie with the space's
    /// middle line on angle 0 and the cutter's axis at the root radius `root` plus the cutter's
    /// radius. None where one cutter's section does not reach round a space this wide: that space
    /// ends square (`space_region`).
    ///
    /// # Errors
    ///
    /// The kernel's refusal: the space's corners give no width, or its floor cannot be made into a
    /// profile.
    fn cutter_section(&self, space: &SpaceSection, root: f64, cutter_radius: f64, out_to: f64) -> Result<Option<CutterSection>, SplineRefusal>;
    /// The whole cross-section of the tooth space `space` out to the circle of radius `out_to` about
    /// the axis, for a space of any width: its floor, the lines out from its corners and the circle's
    /// arc between them, counter-clockwise.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: the space's corners give no width, the circle does not lie outside both
    /// corners, or the floor cannot be made into a profile.
    fn space_region(&self, space: &SpaceSection, out_to: f64) -> Result<SpaceRegion, SplineRefusal>;
    /// The section of a chamfer's tool in a plane through the axis, counter-clockwise, and which of
    /// its sides is the slope; it reaches `margin` past the end and the face.
    ///
    /// # Errors
    ///
    /// `ChamferRefusal`: which number rules a section out (not finite, no size, no corner at a
    /// step, a section that would reach the axis).
    fn chamfer_section(&self, chamfer: &EndChamfer, margin: f64, outside: bool) -> Result<ChamferSection, ChamferRefusal>;
    /// The plane through the axis a chamfer's section is drawn in: the origin of `plane`, x across
    /// the shaft at the angle `seam`, y along the axis into the face (the plane's normal times
    /// `inward`).
    fn chamfer_plane(&self, plane: &Plane, inward: f64, seam: f64) -> Plane;
    /// The seat seen from the other end of its round face: the far end as the free end, with angle 0
    /// where it was.
    fn far_end(&self, seat: &SeatFrame) -> SeatFrame;
    /// An end nothing is known to be past, or that something stands in the way of, on a face of
    /// radius `radius`: the cuts stop at it.
    fn shut_end(&self, radius: f64) -> End;
    /// How far past the end `end` the cuts run to come out of the body (zero where they do not come
    /// out there).
    fn end_run_out(&self, end: &End) -> f64;
    /// How far a tool that takes the whole round face runs on past the end `end`, with `margin` to
    /// spare: through what the body has on its end and the margin, the margin past an end the body
    /// ends at, and not at all at a shoulder or a neck.
    fn end_run_past(&self, end: &End, margin: f64) -> f64;
    /// How far into the round face, past the end's plane, a chamfer on the end `end` reaches once the
    /// face has been moved from the radius `from` to the radius `to`: the chamfer goes with the face
    /// along its own cone, which meets the new surface further in. Zero where the end has no chamfer
    /// of its own.
    fn end_carried(&self, end: &End, from: f64, to: f64) -> f64;
}
