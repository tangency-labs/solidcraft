//! #871: a fillet that opened holes in the model must be refused, not accepted.

use solidcraft_geom::{Length, Vec3};

use crate::{Body, BoolOp, build::box_solid, mm, ops::boolean};

/// The part from #871: a plate with a narrower block on top and a window through both, filleted
/// at radius 1 along every edge of both end faces.
fn part() -> Body {
    let (a, b) = (12.74437365538526, 22.69275963366936);
    let base = box_solid(Vec3::new(9.5, 8.0, 0.0), Vec3::new(23.5, 29.5, 10.0)).unwrap();
    let top = box_solid(Vec3::new(9.5, a, 10.0), Vec3::new(23.5, b, 20.0)).unwrap();
    let joined = boolean(&base, &top, BoolOp::Union).unwrap().unwrap();
    let window = box_solid(Vec3::new(9.5, 14.0, 7.0), Vec3::new(23.5, 19.2, 14.1)).unwrap();
    boolean(&joined, &window, BoolOp::Cut).unwrap().unwrap()
}

fn edges() -> Vec<Vec3> {
    let yz = [
        (12.74437365538526, 15.0),
        (10.37218682769263, 10.0),
        (8.0, 5.0),
        (18.75, 0.0),
        (29.5, 5.0),
        (26.09637981683468, 10.0),
        (22.69275963366936, 15.0),
        (17.71856664452731, 20.0),
        (16.6, 7.0),
        (14.0, 10.55),
        (16.6, 14.100000000000001),
        (19.200000000000003, 10.55),
    ];
    [23.5, 9.5].iter().flat_map(|x| yz.iter().map(move |(y, z)| Vec3::new(*x, *y, *z))).collect()
}

/// Radius 1 on both the window's edges and the block's sides: the wall between them is 1.26
/// mm thick, so the rounds overlap and the end faces' boundaries cross. That result has
/// openings and is refused.
#[test]
fn overlapping_rounds_are_refused() {
    let p = part();
    assert_eq!(p.crossed_face_loops().unwrap(), 0);
    assert!(crate::fillet(&p, &edges(), mm(1.0)).is_err(), "a fillet with openings in its surface was accepted");
    // The corner build makes that body (what #871 got): closed as topology, every face meshes,
    // but the loops of its end faces cross.
    let sel: Vec<(Vec3, Length)> = edges().into_iter().map(|e| (e, mm(1.0))).collect();
    if let Ok(b) = crate::blend_corner::fillet_radii(&p, &sel) {
        assert!(b.validity().is_empty() && b.crossed_face_loops().unwrap() > 0);
    }
}

/// Below half the wall the rounds fit: a closed solid with a round on every edge.
#[test]
fn rounds_that_fit_are_closed() {
    let p = part();
    let b = crate::fillet(&p, &edges(), mm(0.5)).unwrap();
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    assert_eq!(b.crossed_face_loops().unwrap(), 0);
    assert_eq!(b.face_count(), p.face_count() + edges().len());
}
