//! Planning a spline on a shaft and in its hub: the outlines of the shaft's teeth and of the hub's
//! opening from plain numbers, the numbers they come to, and what a caller needs of an outline
//! to cut it (the section of one tooth space, the outline as points, a spline as Bézier pieces).
//!
//! A recipe of kernel calls (`solidcore::teeth`, which holds the tooth geometry and the checks of
//! its numbers): the record's numbers are converted by [`solidcore_convert::shaft_spline`], the
//! kernel plans, and the plan comes back as the record's. It computes no geometric quantity.

use solidcore::teeth as kernel;
use solidcore::teeth::shaft_spline as planner;
use solidcore_convert::shaft_spline as convert;
use solidcore_convert::{plane as to_frame, record_plane, record_vec3, vec3};
use solidcore_record::shaft_spline::{
    AxisGap, ChamferSection, ChordGap, CutterSection, EndChamfer, OutlineLoop, RoundFace,
    SeatFrame, SeatRefusal, SpaceSection, SplineNumbers, SplinePlan, SplineRefusal, ToothOutline,
};
use solidcore_record::{Plane, Vec3};

pub use solidcore::teeth::shaft_spline::LEAST_LENGTH;
pub use solidcore::teeth::MAX_LENGTH;

/// What the numbers make: the shaft's outline and the hub's, the numbers they come to and the
/// master spline as placed.
///
/// # Errors
///
/// [`SplineRefusal`]: a number is refused (not finite, out of range, missing for this kind of
/// teeth), or the numbers together make no teeth. The words name the number to change.
pub fn plan(numbers: &SplineNumbers) -> Result<SplinePlan, SplineRefusal> {
    planner::plan(&convert::numbers(numbers))
        .map(|made| convert::record_plan(&made))
        .map_err(|e| convert::record_refusal(&e))
}

/// The numbers of a spline cut on a shaft that is there, whose radius is `radius`: the size left
/// out is the shaft's own, and a size given that is the shaft's own but for its last places is
/// the shaft's own too. Any other size given stays. The size is the module of involute teeth and
/// the major diameter of straight-sided teeth and serrations; no other number changes.
pub fn fit_to_shaft_within(numbers: &mut SplineNumbers, radius: f64) {
    let mut fitted = convert::numbers(numbers);
    planner::fit_to_shaft_within(&mut fitted, radius);
    numbers.module = fitted.module;
    numbers.major_diameter = fitted.major_diameter;
}

/// The module that puts the tips of involute teeth on the circle of radius `radius` (mm), with
/// the addendum and the profile shift in modules. None where no positive finite module does.
#[must_use]
pub fn module_for_major_radius(
    teeth: f64,
    addendum: f64,
    profile_shift: f64,
    radius: f64,
) -> Option<f64> {
    planner::module_for_major_radius(teeth, addendum, profile_shift, radius)
}

/// How far a radius of the spline may be from a picked face's own and still be taken as the same
/// (mm).
#[must_use]
pub fn same_size(radius: f64) -> f64 {
    planner::same_size(radius)
}

/// How far the tools that cut a spline reach past the faces they would otherwise only touch, for
/// teeth `depth` deep (mm).
#[must_use]
pub fn tool_margin(depth: f64) -> f64 {
    planner::tool_margin(depth)
}

/// The radius of the cutter of diameter `diameter` that ends the tooth spaces of a shaft, whose
/// section runs from the radius `root` out to `out_to` with `margin` to spare.
///
/// # Errors
///
/// The words of the refusal: the diameter is not a number the kernel takes, or the cutter does
/// not reach past the depth it cuts.
pub fn cutter_radius(diameter: f64, root: f64, out_to: f64, margin: f64) -> Result<f64, String> {
    planner::cutter_radius(diameter, root, out_to, margin).map_err(|e| convert::refusal_words(&e))
}

/// The section of the tooth space of the shaft `outline` after the tooth whose top lies nearest
/// the angle `tooth_at` (radians), closed above by the circle of radius `out_to` about the axis.
///
/// # Errors
///
/// The kernel's words: the angle is not a number, the radius is not above the outline's largest,
/// or no top of a tooth lies near the angle.
pub fn space_section(
    outline: &ToothOutline,
    tooth_at: f64,
    out_to: f64,
) -> Result<SpaceSection, String> {
    kernel::outline::space_section_of(
        outline.teeth,
        &convert::tooth(&outline.tooth),
        outline.most_radius,
        tooth_at,
        out_to,
    )
    .map(|section| convert::record_section(&section))
    .map_err(|e| convert::refusal_words(&e))
}

/// The whole `outline` as a closed chain of points, counter-clockwise, without repeating the
/// first point, with chords within the outline's tolerance of each curve, turned about the axis
/// by `turn` (radians).
///
/// # Errors
///
/// The kernel's words: the outline or the turn is not finite, or the outline would need more
/// points than the kernel makes.
pub fn outline_points(outline: &ToothOutline, turn: f64) -> Result<Vec<[f64; 2]>, String> {
    kernel::outline::points_of(
        outline.teeth,
        &convert::tooth(&outline.tooth),
        outline.tolerance,
        turn,
    )
    .map_err(|e| convert::refusal_words(&e))
}

/// A B-spline as its polynomial pieces between knots, in order, each as `degree + 1` control
/// points of a Bézier segment. A piece starts at the very point the piece before it ends at.
///
/// # Errors
///
/// The kernel's words: the knots and control points do not describe a spline.
pub fn bezier_pieces(
    degree: usize,
    knots: &[f64],
    control: &[[f64; 2]],
) -> Result<Vec<Vec<[f64; 2]>>, String> {
    kernel::fit::bezier_pieces(degree, knots, control).map_err(|e| convert::refusal_words(&e))
}

/// How far `p` is from the cylinder of the round face: the distance of `p` from the axis line,
/// less the radius, as an absolute value (not the distance past the face's ends).
#[must_use]
pub fn off_cylinder(face: &RoundFace, p: Vec3) -> f64 {
    kernel::seat::off_cylinder(&convert::round_face(face), vec3(p))
}

/// Whether `p` lies nearer along the axis to the face's `start` end than to its `end` (a tie is
/// the start's).
#[must_use]
pub fn nearer_start(face: &RoundFace, p: Vec3) -> bool {
    kernel::seat::nearer_start(&convert::round_face(face), vec3(p))
}

/// The seat of a spline on `face`: the free end is the end nearest along the axis to `start`, or
/// with none the end nearest `pick`; the plane square to the axis there, angle 0 from the world
/// axes; and the pick brought onto the face's cylinder.
///
/// # Errors
///
/// [`SeatRefusal`]: the axis has no direction, no plane square to it can be made, or the face has
/// no length or no radius.
pub fn seat_frame(
    face: &RoundFace,
    pick: Vec3,
    start: Option<Vec3>,
) -> Result<SeatFrame, SeatRefusal> {
    kernel::seat::seat_frame(&convert::round_face(face), vec3(pick), start.map(vec3))
        .map(|seat| convert::record_seat_frame(&seat))
        .map_err(convert::record_seat_refusal)
}

/// The points a spline's start is looked for at: in the plane of the end of the face nearest
/// `pick`, along angle 0, half, a quarter and a tenth of the radius off the rim (inside the rim of
/// a shaft, outside the rim of a `bore`), then the point on the axis. None where the axis has no
/// angle 0.
#[must_use]
pub fn start_candidates(face: &RoundFace, pick: Vec3, bore: bool) -> Option<[Vec3; 4]> {
    kernel::seat::start_candidates(&convert::round_face(face), vec3(pick), bore)
        .map(|points| points.map(record_vec3))
}

/// `at` moved `by` straight out from the axis (the line through the plane's origin along its
/// normal); `at` itself where it lies on the axis.
#[must_use]
pub fn moved_out(plane: &Plane, at: Vec3, by: f64) -> Vec3 {
    record_vec3(kernel::seat::moved_out(&to_frame(plane), vec3(at), by))
}

/// The matrix that takes world coordinates into the plane's frame (x, y, normal; the origin to
/// zero), column by column.
#[must_use]
pub fn into_frame(plane: &Plane) -> [[f64; 4]; 4] {
    kernel::seat::into_frame(&to_frame(plane))
}

/// How the line through `point` along `dir` lies to the axis through `axis_point` along `axis`
/// (unit vectors).
#[must_use]
pub fn axis_gap(axis_point: Vec3, axis: Vec3, point: Vec3, dir: Vec3) -> AxisGap {
    convert::record_axis_gap(kernel::seat::axis_gap(
        vec3(axis_point),
        vec3(axis),
        vec3(point),
        vec3(dir),
    ))
}

/// The chord from `a` to `b` against the direction `axis` (a unit vector).
#[must_use]
pub fn chord_gap(axis: Vec3, a: Vec3, b: Vec3) -> ChordGap {
    convert::record_chord_gap(kernel::seat::chord_gap(vec3(axis), vec3(a), vec3(b)))
}

/// The angle about the plane's normal, from its x towards its y, of the middle of the box
/// `min`..`max`.
#[must_use]
pub fn centre_angle(plane: &Plane, min: Vec3, max: Vec3) -> f64 {
    kernel::seat::centre_angle(&to_frame(plane), vec3(min), vec3(max))
}

/// How far the angle `a` is from the angle `b`, the short way round (0 to pi).
#[must_use]
pub fn angles_apart(a: f64, b: f64) -> f64 {
    kernel::seat::angles_apart(a, b)
}

/// The least and the most the box `min`..`max` reaches along `dir` from `origin`, over its eight
/// corners.
#[must_use]
pub fn reach_along(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> (f64, f64) {
    kernel::seat::reach_along(vec3(origin), vec3(dir), vec3(min), vec3(max))
}

/// The distance between two points of an outline.
#[must_use]
pub fn points_apart(a: [f64; 2], b: [f64; 2]) -> f64 {
    kernel::seat::points_apart(a, b)
}

/// The whole `outline` as one closed loop of profile segments, every tooth turned to its place
/// and the whole turned by `turn` (radians), with where two segments of one curve meet and each
/// piece as it lies in the loop.
///
/// # Errors
///
/// The kernel's words: "the spline's outline cannot be made into a profile: ..." for a curve
/// that is not fitted, not a spline or of too high a degree.
pub fn outline_loop(outline: &ToothOutline, turn: f64) -> Result<OutlineLoop, String> {
    kernel::section::outline_loop(
        outline.teeth,
        outline.pitch_angle,
        &convert::tooth(&outline.tooth),
        outline.most_radius,
        turn,
    )
    .map(|made| convert::record_outline_loop(&made))
    .map_err(|e| convert::refusal_words(&e))
}

/// The section a cutter of radius `cutter_radius` takes out of the tooth space `space` where it
/// leaves the shaft, out to `out_to` from the axis, on a plane whose origin is `plane_origin`;
/// with the corners as they lie with the space's middle line on angle 0 and the cutter's axis at
/// the root radius `root` plus the cutter's radius.
///
/// # Errors
///
/// The kernel's words: the space is too wide to end in the cutter's curve (naming
/// `cutter_diameter`), or its floor cannot be made into a profile.
pub fn cutter_section(
    space: &SpaceSection,
    root: f64,
    cutter_radius: f64,
    out_to: f64,
    plane_origin: Vec3,
) -> Result<CutterSection, String> {
    kernel::section::cutter_section(
        &convert::tooth(&space.floor),
        space.start,
        space.end,
        root,
        cutter_radius,
        out_to,
        vec3(plane_origin),
    )
    .map(|made| convert::record_cutter_section(&made))
    .map_err(|e| convert::refusal_words(&e))
}

/// The section of a chamfer's tool in a plane through the axis, counter-clockwise, and which of
/// its sides is the slope; it reaches `margin` past the end and the face. None where no such
/// section lies clear of the axis.
#[must_use]
pub fn chamfer_section(chamfer: &EndChamfer, margin: f64, outside: bool) -> Option<ChamferSection> {
    kernel::section::chamfer_section(&convert::end_chamfer(chamfer), margin, outside)
        .map(|section| convert::record_chamfer_section(&section))
}

/// The plane through the axis a chamfer's section is drawn in: the origin of `plane`, x across
/// the shaft at the angle `seam`, y along the axis into the face (the plane's normal times
/// `inward`).
#[must_use]
pub fn chamfer_plane(plane: &Plane, inward: f64, seam: f64) -> Plane {
    record_plane(&kernel::section::chamfer_plane(
        &to_frame(plane),
        inward,
        seam,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::shaft_spline::{
        OutlinePiece, SplineForm, SplineMake, SplineMaster, ToothRole,
    };

    /// A straight-sided spline of six teeth, 28 over 23 mm, 6 mm wide: a shaft and its hub.
    fn straight() -> SplineNumbers {
        SplineNumbers {
            form: SplineForm::Straight,
            make: SplineMake::Both,
            teeth: 6,
            length: 20.0,
            major_diameter: Some(28.0),
            minor_diameter: Some(23.0),
            tooth_width: Some(6.0),
            side_clearance: Some(0.05),
            major_clearance: Some(0.3),
            minor_clearance: Some(0.2),
            hub_diameter: Some(40.0),
            with_hub: true,
            with_shaft: true,
            ..SplineNumbers::default()
        }
    }

    #[test]
    fn a_straight_sided_spline_is_planned_in_record_types() {
        let made = plan(&straight()).unwrap();
        assert_eq!((made.shaft.teeth, made.hub.teeth), (6, 6));
        // The shaft's outline lies between its two circles, and the hub's opening clears it by
        // the clearances asked for.
        assert_eq!(
            (made.shaft.least_radius, made.shaft.most_radius),
            (11.5, 14.0)
        );
        assert!((made.made.hub_major_diameter - 28.6).abs() < 1e-12);
        assert!((made.made.hub_minor_diameter - 23.4).abs() < 1e-12);
        assert_eq!(made.made.positions, 6);
        assert!(made.master.is_none());
        assert!(made
            .shaft
            .tooth
            .iter()
            .any(|piece| piece.role() == ToothRole::Flank));
    }

    #[test]
    fn a_refused_number_names_itself_in_words() {
        let refused = plan(&SplineNumbers {
            tooth_width: Some(f64::NAN),
            ..straight()
        })
        .unwrap_err();
        assert_eq!(refused.key.as_deref(), Some("tooth_width"));
        assert!(refused.words.starts_with("`tooth_width`"), "{refused}");
    }

    /// The words the four outline functions give are the kernel's refusal itself, as they were
    /// before the kernel's errors named their operation: never prefixed by the operation's path.
    #[test]
    fn the_kernel_words_come_without_the_operation() {
        let made = plan(&straight()).unwrap();
        let refusals = [
            (
                outline_points(&made.shaft, f64::NAN).map(|_| ()),
                "the outline would not be finite",
            ),
            (
                space_section(&made.shaft, f64::NAN, 14.5).map(|_| ()),
                "the angle of the tooth is not a number",
            ),
            (
                space_section(&made.shaft, 0.0, 13.0).map(|_| ()),
                "radius is 13; it must be above 14 and at most 100000",
            ),
            (
                bezier_pieces(3, &[], &[]).map(|_| ()),
                "the knots and control points do not describe a spline",
            ),
            (
                cutter_radius(f64::NAN, 11.5, 14.0, 0.1).map(|_| ()),
                "`cutter_diameter` must be a number of at most 100000 mm",
            ),
            (
                cutter_radius(1.0, 11.5, 14.0, 0.1).map(|_| ()),
                "`cutter_diameter` must be more than 5.2 mm: the cutter's radius must reach past \
                 the depth of the teeth",
            ),
        ];
        for (refused, words) in refusals {
            let refused = refused.unwrap_err();
            assert_eq!(refused, words);
            assert!(!refused.starts_with("teeth::"), "{refused}");
        }
    }

    #[test]
    fn an_outline_gives_its_points_and_the_section_of_a_space() {
        let made = plan(&straight()).unwrap();
        let points = outline_points(&made.shaft, 0.0).unwrap();
        assert!(outline_points(&made.shaft, f64::NAN).is_err());
        // Every point lies between the outline's two circles, of radius 11.5 and 14: the squares
        // of its distances from the axis lie between theirs.
        assert!(points.len() >= 24);
        assert!(points
            .iter()
            .all(|p| (132.25 - 1e-6..=196.0 + 1e-6).contains(&(p[0] * p[0] + p[1] * p[1]))));
        let section = space_section(&made.shaft, 0.0, 14.5).unwrap();
        assert!(section.apart);
        assert!(section
            .floor
            .iter()
            .any(|piece| matches!(piece, OutlinePiece::Arc { .. } | OutlinePiece::Line { .. })));
        assert!(space_section(&made.shaft, f64::NAN, 14.5).is_err());
        assert!(space_section(&made.shaft, 0.0, 13.0).is_err());
    }

    #[test]
    fn the_plan_carries_the_pitch_of_the_teeth_and_the_tooth_a_close_up_shows() {
        let tau = std::f64::consts::TAU;
        // Without a master spline: tooth 0, on angle 0.
        let plain = plan(&straight()).unwrap();
        assert_eq!(plain.tooth_pitch.to_bits(), (tau / 6.0).to_bits());
        assert_eq!(plain.tooth_at.to_bits(), 0.0_f64.to_bits());
        for (form, angle) in [
            (SplineMaster::ToothOut, 0.0),
            (SplineMaster::ToothOut, 47.0),
            (SplineMaster::WideTooth, -100.0),
        ] {
            let made = plan(&SplineNumbers {
                master: form,
                master_angle: Some(angle),
                ..straight()
            })
            .unwrap();
            // The outline is one period of the whole ring, a full turn; the teeth as counted are
            // a sixth of one.
            assert_eq!(made.shaft.pitch_angle.to_bits(), tau.to_bits());
            assert_eq!(made.tooth_pitch.to_bits(), (tau / 6.0).to_bits());
            let m = made.master.unwrap();
            // The tooth opposite the master spline.
            let opposite = (m.tooth + 3) % 6;
            let at = m.turn + made.tooth_pitch * f64::from(opposite);
            assert_eq!(made.tooth_at.to_bits(), at.to_bits(), "{form:?} {angle}");
            // The hub's arc laid over the shaft: turned by the hub's start.
            assert_eq!(
                m.hub_arc_on_shaft.radius.to_bits(),
                m.hub_arc.radius.to_bits()
            );
            let start = m.hub_arc.start + made.hub_start;
            let end = m.hub_arc.end + made.hub_start;
            assert_eq!(m.hub_arc_on_shaft.start.to_bits(), start.to_bits());
            assert_eq!(m.hub_arc_on_shaft.end.to_bits(), end.to_bits());
        }
    }

    #[test]
    fn the_module_for_a_radius_puts_the_tips_on_it() {
        // 20 teeth, addendum 0.5: the tips lie at the module times 10.5.
        let module = module_for_major_radius(20.0, 0.5, 0.0, 15.0).unwrap();
        assert_eq!(module.to_bits(), (2.0 * 15.0 / 21.0_f64).to_bits());
        // With a profile shift of 0.25 the tips lie at the module times 10.75.
        let shifted = module_for_major_radius(20.0, 0.5, 0.25, 15.0).unwrap();
        assert_eq!(shifted.to_bits(), (2.0 * 15.0 / 21.5_f64).to_bits());
        for (teeth, addendum, shift, radius) in [
            (20.0, 0.5, 0.0, 0.0),
            (20.0, 0.5, 0.0, -15.0),
            (20.0, 0.5, 0.0, f64::NAN),
            (20.0, 0.5, 0.0, f64::INFINITY),
            (20.0, -10.5, 0.0, 15.0),
            (20.0, -20.0, 0.0, 15.0),
            (f64::NAN, 0.5, 0.0, 15.0),
            (20.0, 0.5, f64::INFINITY, 15.0),
            (1e-300, 0.0, 0.0, f64::MAX),
        ] {
            assert_eq!(
                module_for_major_radius(teeth, addendum, shift, radius),
                None,
                "{teeth} {addendum} {shift} {radius}"
            );
        }
        // A spline fitted to a shaft takes the same module.
        let mut numbers = SplineNumbers {
            form: SplineForm::Involute,
            teeth: 20,
            module: None,
            ..straight()
        };
        fit_to_shaft_within(&mut numbers, 15.0);
        assert_eq!(numbers.module.map(f64::to_bits), Some(module.to_bits()));
    }

    /// A round face along +Z: radius 10, from z = 0 to z = 30.
    fn face_along_z() -> RoundFace {
        RoundFace {
            axis_point: Vec3::new(0.0, 0.0, 0.0),
            axis: Vec3::new(0.0, 0.0, 1.0),
            radius: 10.0,
            start: 0.0,
            end: 30.0,
        }
    }

    #[test]
    fn a_round_face_gives_its_seat_and_its_measures() {
        let face = face_along_z();
        assert_eq!(off_cylinder(&face, Vec3::new(12.0, 0.0, 5.0)), 2.0);
        assert!(nearer_start(&face, Vec3::new(10.0, 0.0, 3.0)));
        assert!(!nearer_start(&face, Vec3::new(10.0, 0.0, 27.0)));
        // Picked near the top: the free end is the top, the face runs on against +Z.
        let seat = seat_frame(&face, Vec3::new(0.0, 11.0, 28.0), None).unwrap();
        assert_eq!(seat.plane.origin, Vec3::new(0.0, 0.0, 30.0));
        assert_eq!(seat.plane.x, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(seat.inward, -1.0);
        assert_eq!(seat.room, 30.0);
        let close = |a: Vec3, b: Vec3| {
            (a.x - b.x).abs() < 1e-12 && (a.y - b.y).abs() < 1e-12 && (a.z - b.z).abs() < 1e-12
        };
        assert!(close(seat.at, Vec3::new(0.0, 10.0, 28.0)), "{:?}", seat.at);
        // A start given decides the free end.
        let from_start = seat_frame(&face, Vec3::new(0.0, 11.0, 28.0), Some(Vec3::ZERO)).unwrap();
        assert_eq!(from_start.inward, 1.0);
        let flat = RoundFace { end: 0.0, ..face };
        assert_eq!(
            seat_frame(&flat, Vec3::ZERO, None),
            Err(SeatRefusal::NoSize)
        );
        let lost = RoundFace {
            axis: Vec3::new(f64::NAN, 0.0, 1.0),
            ..face
        };
        assert_eq!(
            seat_frame(&lost, Vec3::ZERO, None),
            Err(SeatRefusal::NoDirection)
        );
        let none = RoundFace {
            axis: Vec3::ZERO,
            ..face
        };
        assert_eq!(
            seat_frame(&none, Vec3::ZERO, None),
            Err(SeatRefusal::NoPlane)
        );
        let starts = start_candidates(&face, Vec3::new(10.0, 0.0, 2.0), false).unwrap();
        assert_eq!(starts[0], Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(starts[3], Vec3::ZERO);
        let bore = start_candidates(&face, Vec3::new(10.0, 0.0, 2.0), true).unwrap();
        assert_eq!(bore[0], Vec3::new(15.0, 0.0, 0.0));
        assert!(close(
            moved_out(&seat.plane, Vec3::new(0.0, 10.0, 28.0), 2.0),
            Vec3::new(0.0, 12.0, 28.0)
        ));
        let m = into_frame(&seat.plane);
        assert_eq!(m[3], [0.0, 0.0, -30.0, 1.0]);
    }

    #[test]
    fn lines_chords_boxes_and_angles_are_measured() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let gap = axis_gap(Vec3::ZERO, z, Vec3::new(3.0, 4.0, 7.0), z);
        assert_eq!((gap.lean, gap.off), (0.0, 5.0));
        let chord = chord_gap(z, Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0));
        assert_eq!((chord.length, chord.lean), (2.0, 0.0));
        let angle = centre_angle(
            &Plane::XY,
            Vec3::new(-1.0, 1.0, 0.0),
            Vec3::new(1.0, 3.0, 0.0),
        );
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-15);
        assert!((angles_apart(0.1, std::f64::consts::TAU - 0.1) - 0.2).abs() < 1e-12);
        assert_eq!(
            reach_along(
                Vec3::ZERO,
                z,
                Vec3::new(-1.0, -1.0, -2.0),
                Vec3::new(1.0, 1.0, 5.0)
            ),
            (-2.0, 5.0)
        );
        assert_eq!(points_apart([0.0, 0.0], [3.0, 4.0]), 5.0);
    }

    #[test]
    fn an_outline_is_one_loop_and_a_space_a_cutter_section() {
        let made = plan(&straight()).unwrap();
        let whole = outline_loop(&made.shaft, 0.0).unwrap();
        // Six teeth, each its pieces; the pieces cover the loop's segments in order.
        assert_eq!(whole.pieces.len() % 6, 0);
        let covered: usize = whole.pieces.iter().map(|p| p.segs).sum();
        assert_eq!(covered, whole.segs.len());
        let tops = whole.pieces.iter().filter(|p| p.top).count();
        assert!(tops >= 6 && tops % 6 == 0, "{tops}");
        let space = space_section(&made.shaft, 0.0, 14.5).unwrap();
        let cut = cutter_section(&space, 11.5, 5.0, 14.5, Vec3::ZERO).unwrap();
        assert_eq!(cut.spans.len(), space.floor.len());
        let floor: usize = cut.spans.iter().sum();
        assert_eq!(floor + 3, cut.segs.len());
        // The cutter's axis lies on the space's middle line, 16.5 from the shaft's axis.
        let squared = cut.axis_at.x * cut.axis_at.x + cut.axis_at.y * cut.axis_at.y;
        assert!((squared - 272.25).abs() < 1e-9, "{squared}");
    }

    #[test]
    fn a_chamfer_section_and_its_plane() {
        let chamfer = EndChamfer {
            radius: 10.0,
            radial: 1.0,
            axial: 1.0,
            past: 0.0,
            flush: None,
        };
        let section = chamfer_section(&chamfer, 0.1, true).unwrap();
        assert_eq!((section.points.len(), section.slope), (3, 2));
        let none = EndChamfer {
            radial: f64::NAN,
            ..chamfer
        };
        assert!(chamfer_section(&none, 0.1, true).is_none());
        let through = chamfer_plane(&Plane::XY, -1.0, 0.0);
        assert_eq!(through.x, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(through.y, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn the_plan_carries_the_chamfers_as_made() {
        let plain = plan(&SplineNumbers {
            end_chamfer: Some(0.5),
            ..straight()
        })
        .unwrap();
        let end = plain.end_chamfer.unwrap();
        assert_eq!((end.axial, end.radial), (0.5, 0.5));
        assert!(plain.entry_chamfer.is_none());
        let two = plan(&SplineNumbers {
            entry_chamfer: Some(0.5),
            entry_chamfer_distance2: Some(0.25),
            ..straight()
        })
        .unwrap();
        let entry = two.entry_chamfer.unwrap();
        assert_eq!((entry.axial, entry.radial), (0.5, 0.25));
        let refused = plan(&SplineNumbers {
            end_chamfer_angle: Some(30.0),
            ..straight()
        })
        .unwrap_err();
        assert_eq!(refused.key.as_deref(), Some("end_chamfer_angle"));
    }

    #[test]
    fn a_size_left_out_is_the_shaft_own() {
        let mut numbers = SplineNumbers {
            major_diameter: None,
            ..straight()
        };
        fit_to_shaft_within(&mut numbers, 15.0);
        assert_eq!(numbers.major_diameter, Some(30.0));
        // A size given within a thousandth of the shaft's own is the shaft's own.
        let mut close = SplineNumbers {
            major_diameter: Some(30.01),
            ..straight()
        };
        fit_to_shaft_within(&mut close, 15.0);
        assert_eq!(close.major_diameter, Some(30.0));
        assert!((same_size(15.0) - 0.015).abs() < 1e-15);
        // Nothing but the size changes: which parts take the spline is the caller's own.
        assert_eq!(
            close,
            SplineNumbers {
                major_diameter: Some(30.0),
                ..straight()
            }
        );
    }
}
