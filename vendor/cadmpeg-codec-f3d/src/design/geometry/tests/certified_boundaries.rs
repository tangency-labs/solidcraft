// SPDX-License-Identifier: Apache-2.0

use super::*;

fn triangle() -> [Point2; 3] {
    [
        Point2::new(0.0, 0.0),
        Point2::new(3.0, 0.0),
        Point2::new(0.0, 3.0),
    ]
}

#[test]
fn certified_boundary_borrows_its_loop() {
    let loop_ = super::super::CertifiedProfileLoop::from_vertices(&triangle()).unwrap();
    let boundary = super::super::ProfileBoundary::CertifiedLoop(loop_);
    assert!(matches!(
        boundary.certified_loop(),
        Some(std::borrow::Cow::Borrowed(_))
    ));
}
