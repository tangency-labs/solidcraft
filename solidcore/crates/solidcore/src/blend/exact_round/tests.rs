//! The closed forms of a round between planes, a corner's ball and their curves.
use super::*;

fn plane(origin: V3, u: V3, v: V3) -> Surface {
    Surface::Plane {
        origin,
        u_axis: u,
        v_axis: v,
    }
}

/// The front (y = 0, normal -y) and the top (z = 1, normal +z) of a block meet along x: a
/// convex edge, the round's axis r in from both and its contacts r back along each face.
#[test]
fn a_square_convex_edge_takes_its_axis_r_in_from_both_faces() {
    let front = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let top = plane([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let (a, b) = ([0.0, 0.0, 1.0], [10.0, 0.0, 1.0]);
    // the front's loop runs -x along its top edge, the top's +x along its front edge
    let sides = [
        Side {
            plane: &front,
            outward: true,
            forward: false,
        },
        Side {
            plane: &top,
            outward: true,
            forward: true,
        },
    ];
    let r = Round::new(a, b, sides, 2.0).unwrap();
    assert!(r.convex());
    let end = plane([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    let e = r.open_end(a, &end, true).unwrap();
    assert_eq!(e.centre, [0.0, 2.0, -1.0]);
    assert_eq!(e.contacts, [[0.0, 0.0, -1.0], [0.0, 2.0, 1.0]]);
    // a plane not square to the edge is no open end
    let tilted = plane([0.0; 3], [0.0, 0.0, 1.0], [0.1, 1.0, 0.0]);
    assert!(r.open_end(a, &tilted, true).is_none());
}

/// A quarter arc's rational quadratic has the corner of its tangents as its middle control
/// point and weight cos 45 degrees, and lies on the circle.
#[test]
fn a_quarter_arc_is_its_rational_quadratic() {
    let (_, form) = arc([0.0; 3], 2.0, [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]).unwrap();
    let Curve::RationalSpline { homogeneous, .. } = &form else {
        panic!("{form:?}")
    };
    let w = std::f64::consts::FRAC_1_SQRT_2;
    assert_eq!(homogeneous[1], [2.0 * w, 2.0 * w, 0.0, w]);
    for t in [0.1, 0.5, 0.9] {
        let p = bezier(homogeneous, t).unwrap();
        assert!((norm(p) - 2.0).abs() < 1e-14, "{p:?}");
    }
}

/// The ball at a box's corner touches the three faces at r from each.
#[test]
fn a_box_corner_s_ball_is_r_in_from_each_face() {
    let x0 = plane([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    let y0 = plane([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    let z0 = plane([0.0; 3], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
    let c = ball_centre([0.0; 3], [(&x0, true), (&y0, true), (&z0, true)], true, 3.0).unwrap();
    assert_eq!(c, [3.0, 3.0, 3.0]);
    let c = ball_centre(
        [0.0; 3],
        [(&x0, true), (&y0, true), (&z0, true)],
        false,
        3.0,
    )
    .unwrap();
    assert_eq!(c, [-3.0, -3.0, -3.0]);
}
