// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

fn rectangle_profiles(offset: f64, transpose: bool) -> usize {
    let sketch = SketchId::mint("f3d:model:sketch#cell-limit").expect("fixture sketch ID");
    let side = if offset == 0.0 { 2.0 } else { 1.0e20 };
    let points = [
        Point2::new(offset, 0.0),
        Point2::new(offset + side, 0.0),
        Point2::new(offset + side, 2.0),
        Point2::new(offset, 2.0),
    ]
    .map(|point| {
        if transpose {
            Point2::new(point.v, point.u)
        } else {
            point
        }
    });
    let entities = (0..4)
        .map(|index| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#cell-edge-{index}"))
                    .expect("fixture edge ID"),
                sketch.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: points[index],
                    end: points[(index + 1) % 4],
                })
                .expect("finite nonzero fixture line"),
            )
        })
        .collect::<Vec<_>>();
    crate::test_support::with_decode_context(|decode_ctx| {
        super::closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0)
    })
    .expect("profile computation has no resource refusal")
    .len()
}

#[test]
fn closed_profile_u_cell_refuses_positive_i64_overflow() {
    assert_eq!(rectangle_profiles(0.0, false), 1);
    assert_eq!(rectangle_profiles(1.0e30, false), 0);
}

#[test]
fn closed_profile_u_cell_refuses_negative_i64_overflow() {
    assert_eq!(rectangle_profiles(0.0, false), 1);
    assert_eq!(rectangle_profiles(-1.0e30, false), 0);
}

#[test]
fn closed_profile_v_cell_refuses_positive_i64_overflow() {
    assert_eq!(rectangle_profiles(0.0, true), 1);
    assert_eq!(rectangle_profiles(1.0e30, true), 0);
}

#[test]
fn closed_profile_v_cell_refuses_negative_i64_overflow() {
    assert_eq!(rectangle_profiles(0.0, true), 1);
    assert_eq!(rectangle_profiles(-1.0e30, true), 0);
}
