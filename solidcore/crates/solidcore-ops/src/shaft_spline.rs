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
use solidcore_record::shaft_spline::{
    SpaceSection, SplineNumbers, SplinePlan, SplineRefusal, ToothOutline,
};

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
        .map_err(|refusal| convert::record_refusal(&refusal))
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
    planner::cutter_radius(diameter, root, out_to, margin)
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
    .map_err(|e| e.to_string())
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
    .map_err(|e| e.to_string())
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
    kernel::fit::bezier_pieces(degree, knots, control).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::shaft_spline::{OutlinePiece, SplineForm, SplineMake, ToothRole};

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
