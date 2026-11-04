// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::math::Point2;

#[test]
fn certified_loop_containment_uses_existing_tube_vertices() {
    let vertices = [
        Point2::new(0.0, 0.0),
        Point2::new(2.0, 0.0),
        Point2::new(0.0, 2.0),
    ];
    let loop_ = crate::design::geometry::CertifiedProfileLoop::from_vertices(&vertices).unwrap();
    assert!(loop_.contains_point(Point2::new(0.25, 0.25)));
    assert!(!loop_.contains_point(Point2::new(1.5, 1.5)));
    assert!(!loop_.contains_point(Point2::new(0.0, 0.0)));
}
