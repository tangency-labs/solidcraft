// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_plane_binds_explicit_unmarked_and_identity_placements() {
    use crate::records::feature::scope::DesignParameterScope;
    use crate::test_support::work_plane::{placements, scope};
    for placement in placements()
        .into_iter()
        .chain(crate::test_support::placement_construction::modern_placements())
    {
        let mut scope = scope(&placement);
        let expected = placement.fields.matrix.as_ref().map_or(
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            |matrix| matrix.value,
        );
        let losses = super::bind(std::slice::from_mut(&mut scope), &[placement]);
        assert!(losses.is_empty());
        assert_eq!(scope.work_plane_transform(), Some(expected));
        let wire = serde_json::to_value(&scope).unwrap();
        let restored: DesignParameterScope = serde_json::from_value(wire).unwrap();
        assert_eq!(restored, scope);
    }
}

#[test]
fn work_plane_refuses_missing_foreign_ambiguous_and_overflowing_placements_with_loss() {
    use crate::test_support::work_plane::placements;
    let placement = placements().remove(0);
    let mut foreign = placement.clone();
    foreign.id = "f3d:Other/BulkStream.dat:placement#13".into();
    let mut overflow = placement.clone();
    let matrix = overflow.fields.matrix.as_mut().unwrap();
    let mut rows = matrix.value.rows();
    rows[0][3] = f64::MAX;
    matrix.value = rows.try_into().unwrap();
    let mut second = placement.clone();
    second.record_index = placement.record_index + 1;
    second.id = format!(
        "{}:design-placement-construction#{}",
        crate::ids::native_stream(&placement.id).unwrap(),
        second.record_index
    );
    let both = [placement.record_index, second.record_index];
    for (candidates, references) in [
        (vec![], &both[..1]),
        (vec![foreign], &both[..1]),
        (vec![placement.clone(), second], &both[..]),
        (vec![overflow], &both[..1]),
    ] {
        let mut scope = crate::test_support::work_plane::scope_referencing(references);
        let losses = super::bind(std::slice::from_mut(&mut scope), &candidates);
        assert!(scope.work_plane_frame().is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::WorkPlaneFrameUnbound
                .note("")
                .code
        );
    }
}

#[test]
fn work_plane_refuses_nonzero_axis_states_and_direction_controls() {
    use crate::test_support::work_plane::{placements, scope};
    let placement = placements().remove(0);
    for slot in 0..5 {
        let mut candidate = placement.clone();
        match slot {
            0 => candidate.fields.inferred_x_axis.value = 1,
            1 => candidate.fields.inferred_z_axis.value = 1,
            2 => candidate.fields.z_flip.value = 1,
            3 => candidate.fields.axis_flipped.as_mut().unwrap().value = 1,
            4 => candidate.fields.use_u_direction.as_mut().unwrap().value = 1,
            _ => unreachable!(),
        }
        let mut scope = scope(&placement);
        let losses = super::bind(std::slice::from_mut(&mut scope), &[candidate]);
        assert!(scope.work_plane_frame().is_none());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::WorkPlaneFrameUnbound
                .note("")
                .code
        );
    }
}

#[test]
fn work_plane_modern_frames_refuse_unchecked_periodic_and_direction_modes_with_loss() {
    use crate::test_support::placement_construction::modern_placements;
    for placement in modern_placements()
        .into_iter()
        .filter(|p| p.type_version == 16)
    {
        for slot in 0..6 {
            let mut candidate = placement.clone();
            match slot {
                0 => candidate.fields.periodic_face_mode.as_mut().unwrap().value = 1,
                1 => candidate.fields.use_u_direction.as_mut().unwrap().value = 2,
                2 => candidate.fields.inferred_x_axis.value = 1,
                3 => candidate.fields.inferred_z_axis.value = 1,
                4 => candidate.fields.z_flip.value = 1,
                5 => candidate.fields.axis_flipped.as_mut().unwrap().value = 1,
                _ => unreachable!(),
            }
            let mut scope = crate::test_support::work_plane::scope(&candidate);
            let losses = super::bind(std::slice::from_mut(&mut scope), &[candidate]);
            assert!(scope.work_plane_frame().is_none());
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::WorkPlaneFrameUnbound
                    .note("")
                    .code
            );
        }
    }
}

#[test]
fn work_plane_modern_join_uses_member_identity_independent_of_table_position_and_count() {
    for placement in crate::test_support::placement_construction::modern_placements() {
        for members in [
            [71, placement.record_index, 73].as_slice(),
            [71, 72, 73, placement.record_index].as_slice(),
        ] {
            let mut scope = crate::test_support::work_plane::scope_referencing(members);
            let losses = super::bind(
                std::slice::from_mut(&mut scope),
                std::slice::from_ref(&placement),
            );
            assert!(losses.is_empty());
            assert!(scope.work_plane_frame().is_some());
            assert!(scope.work_plane_construction().is_none());
            assert!(scope.work_plane_reference().is_none());
        }
    }
}

#[test]
fn work_plane_modern_frames_admit_both_checked_direction_values() {
    for placement in crate::test_support::placement_construction::modern_placements()
        .into_iter()
        .filter(|placement| placement.type_version == 16)
    {
        for direction in [0, 1] {
            let mut candidate = placement.clone();
            candidate.fields.use_u_direction.as_mut().unwrap().value = direction;
            let mut scope = crate::test_support::work_plane::scope(&candidate);
            assert!(super::bind(std::slice::from_mut(&mut scope), &[candidate]).is_empty());
            assert_eq!(
                scope.work_plane_transform(),
                Some(placement.fields.matrix.as_ref().map_or(
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    |matrix| matrix.value,
                ))
            );
        }
    }
}
