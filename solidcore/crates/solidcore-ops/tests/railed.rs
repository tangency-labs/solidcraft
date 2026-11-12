//! tangency-labs/solidcore#3140: the loft guided by rails and the blend through ordered
//! cross-sections, through ops.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

mod common;

use common::edge_at;
use solidcore_ops::make_cylinder;
use solidcore_ops::sweep::railed::{
    loft_blend, loft_rails, section_frames, BlendFrame, Continuity, RailFit, RailLoft, RailSection,
};
use solidcore_ops::sweep::Curve;
use solidcore_ops::Error;

fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
    Curve::Line { start, end }
}

/// A square of side `2 s` about the z axis at height `z`, as four lines.
fn square(s: f64, z: f64) -> Vec<Curve> {
    let c = [[s, -s, z], [s, s, z], [-s, s, z], [-s, -s, z]];
    (0..4).map(|i| line(c[i], c[(i + 1) % 4])).collect()
}

fn section(s: f64, z: f64) -> RailSection {
    RailSection {
        origin: [0.0, 0.0, z],
        normal: [0.0, 0.0, 1.0],
        curves: square(s, z),
    }
}

/// A square frustum: sections of side 2 at z = 0 and side 4 at z = 10, one straight rail
/// through a corner of each. Every station is the frustum's square there, so the solid is the
/// frustum, `h / 3 (A_0 + A_1 + sqrt(A_0 A_1)) = 10 / 3 (4 + 16 + 8)`.
#[test]
fn a_straight_rail_lofts_the_square_frustum() {
    let rail = line([1.0, -1.0, 0.0], [2.0, -2.0, 10.0]);
    let body = loft_rails(
        &[section(1.0, 0.0), section(2.0, 10.0)],
        &[rail],
        &RailLoft::new(1e-6),
    )
    .unwrap();
    let v = body.volume(1e-10).unwrap();
    let want = 10.0 / 3.0 * 28.0;
    assert!((v - want).abs() <= 1e-6 * want, "{v} against {want}");
}

/// Stretched instead of scaled, the loft along the same rail builds (the station stretched
/// across to the rail only).
#[test]
fn a_stretched_straight_rail_lofts_the_same_frustum() {
    let rail = line([1.0, -1.0, 0.0], [2.0, -2.0, 10.0]);
    let how = RailLoft {
        fit: RailFit::Stretch,
        ..RailLoft::new(1e-6)
    };
    let body = loft_rails(&[section(1.0, 0.0), section(2.0, 10.0)], &[rail], &how);
    assert!(body.is_ok(), "{body:?}");
}

/// The kernel's refusal past the wires reaches ops as its own error, naming the bound.
#[test]
fn a_rail_loft_past_its_wires_is_refused_naming_the_bound() {
    // a rail that winds between the sections: a cubic through the corners, which the skin
    // through three wires (the sections and one station) cannot follow to 1e-9
    let rail = Curve::Nurbs {
        degree: 3,
        points: vec![
            [1.0, -1.0, 0.0, 1.0],
            [4.0, -4.0, 3.0, 1.0],
            [0.0, 0.0, 7.0, 1.0],
            [2.0, -2.0, 10.0, 1.0],
        ],
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    };
    let how = RailLoft {
        max_wires: 3,
        ..RailLoft::new(1e-9)
    };
    match loft_rails(&[section(1.0, 0.0), section(2.0, 10.0)], &[rail], &how) {
        Err(Error::RailPastTolerance {
            asked,
            reached,
            wires,
        }) => {
            assert_eq!(asked, 1e-9);
            assert!(reached > asked, "{reached}");
            assert!(wires <= 3, "{wires}");
        }
        other => panic!("{other:?}"),
    }
}

/// Two straight sections held at G0 are the ruled sheet between them: a 4 by 3 rectangle.
#[test]
fn a_g0_blend_of_two_lines_is_the_rectangle() {
    let frame = |p: [f64; 3]| BlendFrame {
        p,
        ..BlendFrame::default()
    };
    let a = vec![frame([0.0, 0.0, 0.0]), frame([4.0, 0.0, 0.0])];
    let b = vec![frame([0.0, 3.0, 0.0]), frame([4.0, 3.0, 0.0])];
    let body = loft_blend(&[a, b], [Continuity::G0, Continuity::G0], [0.0, 0.0]).unwrap();
    assert_eq!(body.counts().unwrap()[0], 1);
    let area = body.area(1e-10).unwrap();
    assert!((area - 12.0).abs() <= 1e-9, "{area}");
}

/// The refusals reach ops: one section, no rail, three rails; a blend of one section.
#[test]
fn malformed_lofts_are_refused() {
    let rail = line([1.0, -1.0, 0.0], [2.0, -2.0, 10.0]);
    let how = RailLoft::new(1e-6);
    assert_eq!(
        loft_rails(&[section(1.0, 0.0)], std::slice::from_ref(&rail), &how).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        loft_rails(&[section(1.0, 0.0), section(2.0, 10.0)], &[], &how).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        loft_rails(
            &[section(1.0, 0.0), section(2.0, 10.0)],
            &[rail.clone(), rail.clone(), rail],
            &how
        )
        .err(),
        Some(Error::Argument)
    );
    let one = vec![vec![BlendFrame::default(); 2]];
    assert!(loft_blend(&one, [Continuity::G0; 2], [0.0; 2]).is_err());
}

/// The rims of two coaxial cylinders of radius 5 (one up to z = 10, the other from z = 20) read
/// as frames leaving along their walls (SolidCraft's 0.2 rule), up from the lower and down,
/// back into the blend, from the upper: blended at G1 the sheet leaves each rim along the axis,
/// and is the cylinder's wall between them to its stations' interpolation.
#[test]
fn two_rims_read_as_frames_blend_into_the_wall_between() {
    let lower = make_cylinder([0.0, 0.0, 0.0], 5.0, 10.0).unwrap();
    let upper = make_cylinder([0.0, 0.0, 20.0], 5.0, 10.0).unwrap();
    let a = section_frames(
        &lower,
        &[edge_at(&lower, [0.0, 5.0, 10.0])],
        [0.0, 0.0, 1.0],
        17,
        0.2,
    )
    .unwrap();
    let b = section_frames(
        &upper,
        &[edge_at(&upper, [0.0, 5.0, 20.0])],
        [0.0, 0.0, -1.0],
        17,
        0.2,
    )
    .unwrap();
    for f in a.iter().chain(&b) {
        assert!(f.bend.iter().all(|x| x.abs() <= 1e-9), "{f:?}");
    }
    let sheet = loft_blend(&[a, b], [Continuity::G1, Continuity::G1], [0.0, 0.0]).unwrap();
    assert_eq!(sheet.counts().unwrap()[0], 1);
    let area = sheet.area(1e-8).unwrap();
    let wall = 2.0 * std::f64::consts::PI * 5.0 * 10.0;
    assert!((area - wall).abs() <= 1e-3 * wall, "{area} vs {wall}");
}

/// A frame query on an edge the body does not have is refused.
#[test]
fn a_frame_query_off_the_body_is_refused() {
    let body = make_cylinder([0.0, 0.0, 0.0], 5.0, 10.0).unwrap();
    assert_eq!(
        section_frames(&body, &[99_999], [0.0, 0.0, 1.0], 5, 0.2).err(),
        Some(Error::Argument)
    );
}
