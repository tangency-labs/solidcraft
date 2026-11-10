//! A face's boundary crossing itself: a box's top face (none), and the same face with its
//! front edge swung across its back edge.

use super::*;
use crate::curve::curve::Curve;
use crate::primitive::api_curves::create_line_segment;
use crate::primitive::api_primitives::create_box;
use crate::query::api_queries::edge_closest_point;

fn edge_at(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| edge_closest_point(k, b, e, p).unwrap().closest.distance < 1e-9)
        .unwrap()
}

/// The face of `b` at `p` (the face of the edge through `p` facing up).
fn top(b: &Brep, e: EdgeId) -> FaceId {
    edge::faces(b, e, None)
        .unwrap()
        .into_iter()
        .find(|&f| matches!(b.face(f).unwrap().surface.as_ref().map(Surface::kind), Some(SurfaceKind::Plane(p)) if p.is_planar().1.z.abs() > 0.5))
        .unwrap()
}

/// A 30 x 20 x 10 box's top face crosses itself nowhere; with its front edge's curve made the
/// segment from (0, 0, 10) to (30, 25, 10) it crosses the back edge y = 20 at x = 24 (the
/// segment's joint with the left edge, at the vertex they share, is no crossing), and the
/// rounds that made such a face are refused naming the point.
#[test]
fn a_boundary_crossing_itself_is_found_and_refused() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let front = edge_at(&k, &b, Point3::new(15.0, 0.0, 10.0));
    let f = top(&b, front);
    assert!(boundary_crossings(&k, &b, f).unwrap().is_empty());
    rounds_cross(&k, &b, &[f]).unwrap();

    let start = edge::start_end_vertices(&b, front).unwrap().0.unwrap();
    let from_origin = b.vertex(start).unwrap().point.x < 15.0;
    let (a, c) = (Point3::new(0.0, 0.0, 10.0), Point3::new(30.0, 25.0, 10.0));
    let line = Curve::from(if from_origin {
        create_line_segment(a, c).unwrap()
    } else {
        create_line_segment(c, a).unwrap()
    });
    let interval = line.natural_interval();
    let edge = b.edge_mut(front).unwrap();
    edge.curve = Some(line);
    edge.interval = interval;
    let at = boundary_crossings(&k, &b, f).unwrap();
    assert_eq!(at.len(), 1, "{at:?}");
    assert!(
        (at[0] - Point3::new(24.0, 20.0, 10.0)).length() < 1e-9,
        "{at:?}"
    );
    let err = rounds_cross(&k, &b, &[f]).unwrap_err();
    let SolidError::RoundRefused {
        refusal: RoundRefusal::RoundsCross { at, fit: None },
        ..
    } = err
    else {
        panic!("{err:?}")
    };
    assert!(
        !at.is_empty()
            && at
                .iter()
                .all(|p| (*p - Point3::new(24.0, 20.0, 10.0)).length() < 1e-9)
    );
}
