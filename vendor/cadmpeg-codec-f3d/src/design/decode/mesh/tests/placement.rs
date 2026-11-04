// SPDX-License-Identifier: Apache-2.0
use super::matrix;
use crate::design::decode::mesh::{mesh_body_transform, MeshBody};
use crate::layout::paramesh_mesh_body_join_prefix;
use crate::paramesh::MeshContainer;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::units::UnitVector3;

const EPS_MESH_NORMAL_ALIGNMENT: f64 = 1.0e-12;

fn mesh_body_payload(cells: [f64; 16]) -> Vec<u8> {
    let mut payload = vec![0; paramesh_mesh_body_join_prefix::FIRST_TRANSFORM];
    payload.extend_from_slice(&matrix(cells));
    payload.push(0);
    payload.extend_from_slice(&matrix(cells));
    payload
}

#[test]
fn mesh_body_transform_applies_nonuniform_scale_and_translation() {
    let cells = [
        0.175, 0.0, 0.0, 0.4, 0.0, 0.06, 0.0, 0.7, 0.0, 0.0, 0.125, 0.3, 0.0, 0.0, 0.0, 1.0,
    ];
    let transform = mesh_body_transform(&mesh_body_payload(cells)).expect("affine map");
    assert_eq!(
        transform
            .transform_point(
                FinitePoint3::new(cadmpeg_ir::math::Point3::new(2.0, 5.0, 8.0)).unwrap(),
            )
            .expect("transformed point")
            .get(),
        cadmpeg_ir::math::Point3::new(7.5, 10.0, 13.0)
    );
}

#[test]
fn reflected_mesh_placement_preserves_triangle_and_corner_order() {
    let cells = [
        -0.5, 0.0, 0.0, 1.0, 0.0, 0.25, 0.0, -2.0, 0.0, 0.0, 2.0, 0.5, 0.0, 0.0, 0.0, 1.0,
    ];
    let transform = mesh_body_transform(&mesh_body_payload(cells)).expect("reflected map");
    let container = MeshContainer {
        fusion_uuid: "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".into(),
        mesh_uuid: crate::records::mesh::DesignMeshUuid::try_from(
            "11111111-2222-4333-8444-555555555555".to_owned(),
        )
        .unwrap(),
        vertices: vec![
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(2.0, 8.0, 3.0)).unwrap(),
            FinitePoint3::ZERO,
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(4.0, 4.0, -1.0)).unwrap(),
        ],
        triangles: vec![[2, 0, 1]],
        feature_edges: vec![[0, 2]],
        corner_normals: None,
        triangle_groups: Vec::new(),
        texture_ids: None,
        attributes: vec![crate::paramesh::MeshAttribute {
            role: 4,
            resource_guid: None,
            authored_name: None,
            groups: crate::paramesh::UniqueFaceGroups::default(),
            elements: crate::paramesh::MeshElements::Float {
                width: crate::paramesh::FloatWidth::Quad,
                values: (0..80).collect(),
            },
            addressing: crate::paramesh::MeshAttributeAddressing::Corner(vec![0, 2]),
        }],
    };
    let body = MeshBody::from_container("mesh.paramesh", 100, transform, container)
        .expect("projected mesh");

    assert_eq!(
        body.vertices
            .iter()
            .map(|point| point.get())
            .collect::<Vec<_>>(),
        [
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 65.0),
            cadmpeg_ir::math::Point3::new(10.0, -20.0, 5.0),
            cadmpeg_ir::math::Point3::new(-10.0, -10.0, -15.0),
        ]
    );
    assert_eq!(body.triangles, [[2, 0, 1]]);
    assert_eq!(body.feature_edges, [[0, 2]]);
    assert!(matches!(
        &body.attributes[0].addressing,
        crate::paramesh::MeshAttributeAddressing::Corner(positions) if positions == &[0, 2]
    ));
}

#[test]
fn mesh_placement_transforms_corner_normals_with_oriented_cofactors() {
    let cells = [
        -2.0, 0.5, 0.2, 1.0, 0.1, 3.0, 0.25, -2.0, 0.3, -0.2, 4.0, 0.5, 0.0, 0.0, 0.0, 1.0,
    ];
    let transform = mesh_body_transform(&mesh_body_payload(cells)).expect("affine map");
    let container = MeshContainer {
        fusion_uuid: "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".into(),
        mesh_uuid: crate::records::mesh::DesignMeshUuid::try_from(
            "11111111-2222-4333-8444-555555555555".to_owned(),
        )
        .unwrap(),
        vertices: vec![
            FinitePoint3::ZERO,
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)).unwrap(),
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0)).unwrap(),
        ],
        triangles: vec![[0, 1, 2]],
        feature_edges: vec![[0, 1]],
        corner_normals: Some(vec![UnitVector3::Z_AXIS; 3]),
        triangle_groups: Vec::new(),
        texture_ids: None,
        attributes: Vec::new(),
    };
    let body = MeshBody::from_container("mesh.paramesh", 100, transform, container)
        .expect("projected mesh");
    let geometric_normal = body.vertices[1]
        .get()
        .vector_from(body.vertices[0].get())
        .cross(body.vertices[2].get().vector_from(body.vertices[0].get()))
        .unit()
        .expect("triangle normal");
    let corner_normals = body
        .corner_normals
        .expect("a stated corner-normal channel reaches the body");
    assert_eq!(corner_normals.len(), 3);
    for normal in corner_normals {
        assert!((normal.as_raw().dot(geometric_normal) - 1.0).abs() < EPS_MESH_NORMAL_ALIGNMENT);
    }
}

#[test]
fn mesh_body_transform_refuses_mismatched_projective_and_singular_pairs() {
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let mut mismatched = mesh_body_payload(identity);
    mismatched[paramesh_mesh_body_join_prefix::SECOND_TRANSFORM
        ..paramesh_mesh_body_join_prefix::SECOND_TRANSFORM + 8]
        .copy_from_slice(&2.0f64.to_le_bytes());
    assert!(mesh_body_transform(&mismatched).is_none());

    let mut projective = identity;
    projective[12] = 1.0;
    assert!(mesh_body_transform(&mesh_body_payload(projective)).is_none());

    let mut singular = identity;
    singular[0] = 0.0;
    assert!(mesh_body_transform(&mesh_body_payload(singular)).is_none());
}
