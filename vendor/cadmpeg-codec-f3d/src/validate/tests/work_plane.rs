// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_plane_validator_checks_solved_matrix_and_source_offset_for_each_form() {
    use crate::design::decode::placement_construction::work_planes::bind;
    use crate::test_support::work_plane::{placements, scope};
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for placement in placements()
            .into_iter()
            .chain(crate::test_support::placement_construction::modern_placements())
        {
            let mut scope = scope(&placement);
            let native = crate::native::F3dNative {
                design_placement_constructions: vec![placement],
                ..Default::default()
            };
            assert!(bind(
                std::slice::from_mut(&mut scope),
                &native.design_placement_constructions
            )
            .is_empty());
            let scope = serde_json::from_value::<
                crate::records::feature::scope::DesignParameterScope,
            >(serde_json::to_value(scope).unwrap())
            .unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            assert!(super::super::valid_work_plane_construction(
                &ctx,
                &scope,
                "f3d:Design/BulkStream.dat"
            ));
            let mut bad_offset = scope.clone();
            bad_offset
                .work_plane_frame_mut()
                .unwrap()
                .work_plane_transform_offset += 1;
            assert!(!super::super::valid_work_plane_construction(
                &ctx,
                &bad_offset,
                "f3d:Design/BulkStream.dat"
            ));
            let mut bad_matrix = scope;
            let frame = bad_matrix.work_plane_frame_mut().unwrap();
            let mut rows = frame.work_plane_transform.rows();
            rows[1][3] += 7.0;
            frame.work_plane_transform = rows.try_into().unwrap();
            assert!(!super::super::valid_work_plane_construction(
                &ctx,
                &bad_matrix,
                "f3d:Design/BulkStream.dat"
            ));
        }
    });
}

#[test]
fn work_plane_validator_refuses_a_frame_whose_typed_placement_does_not_bind() {
    use crate::design::decode::placement_construction::work_planes::bind;
    use crate::test_support::work_plane::{placements, scope};
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let placement = placements().remove(0);
        let mut scope = scope(&placement);
        assert!(bind(
            std::slice::from_mut(&mut scope),
            std::slice::from_ref(&placement)
        )
        .is_empty());
        assert!(scope.work_plane_frame().is_some());
        let mut unbound = placement;
        unbound.fields.z_flip.value = 1;
        let native = crate::native::F3dNative {
            design_placement_constructions: vec![unbound],
            ..Default::default()
        };
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        assert!(!super::super::valid_work_plane_construction(
            &ctx,
            &scope,
            "f3d:Design/BulkStream.dat"
        ));
    });
}

#[test]
fn work_plane_validator_rejects_modern_controls_that_do_not_bind_the_retained_frame() {
    use crate::design::decode::placement_construction::work_planes::bind;
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for placement in crate::test_support::placement_construction::modern_placements()
            .into_iter()
            .filter(|placement| placement.type_version == 16)
        {
            let mut scope = crate::test_support::work_plane::scope(&placement);
            assert!(bind(
                std::slice::from_mut(&mut scope),
                std::slice::from_ref(&placement)
            )
            .is_empty());
            for slot in 0..6 {
                let mut unbound = placement.clone();
                match slot {
                    0 => unbound.fields.periodic_face_mode.as_mut().unwrap().value = 1,
                    1 => unbound.fields.use_u_direction.as_mut().unwrap().value = 2,
                    2 => unbound.fields.inferred_x_axis.value = 1,
                    3 => unbound.fields.inferred_z_axis.value = 1,
                    4 => unbound.fields.z_flip.value = 1,
                    5 => unbound.fields.axis_flipped.as_mut().unwrap().value = 1,
                    _ => unreachable!(),
                }
                let native = crate::native::F3dNative {
                    design_placement_constructions: vec![unbound],
                    ..Default::default()
                };
                let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
                assert!(!super::super::valid_work_plane_construction(
                    &ctx,
                    &scope,
                    "f3d:Design/BulkStream.dat"
                ));
            }
        }
    });
}
