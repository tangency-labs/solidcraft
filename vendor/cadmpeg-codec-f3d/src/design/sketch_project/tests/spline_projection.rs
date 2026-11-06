// SPDX-License-Identifier: Apache-2.0
use crate::records::sketch_relations::SketchRelationReturnMember;
use cadmpeg_ir::math::Point3;

#[test]
fn spline_segment_index_keeps_agreeing_points() {
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let mut segments = std::collections::HashMap::new();
    crate::design::sketch_project::record_spline_segment(&mut segments, "Design", 10, points);
    crate::design::sketch_project::record_spline_segment(&mut segments, "Design", 10, points);
    assert_eq!(segments.get(&("Design", 10)), Some(&Some(points)));
    crate::design::sketch_project::record_spline_segment(
        &mut segments,
        "Design",
        10,
        [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
    );
    assert_eq!(segments.get(&("Design", 10)), Some(&None));
}

#[test]
fn spatial_spline_member_indices_reject_duplicates() {
    let members = [
        SketchRelationReturnMember::from_index(10),
        SketchRelationReturnMember::from_index(11),
    ];
    assert!(crate::design::sketch_project::distinct_return_member_indices(&members));
    let duplicates = [
        SketchRelationReturnMember::from_index(10),
        SketchRelationReturnMember::from_index(10),
    ];
    assert!(!crate::design::sketch_project::distinct_return_member_indices(&duplicates));
}

#[test]
fn sketch_nurbs_refuses_nonpositive_weight_before_projection() {
    let error = crate::records::sketch_geometry::SketchNurbsPoles::from_wire(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        vec![1.0, 0.0],
    )
    .expect_err("nonpositive weight must be refused at admission");
    assert!(
        error.contains("weight is not positive and finite"),
        "{error}"
    );
}
