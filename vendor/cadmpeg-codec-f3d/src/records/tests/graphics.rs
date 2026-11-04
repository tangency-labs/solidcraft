// SPDX-License-Identifier: Apache-2.0

#[test]
fn affine_placement_preserves_shear_and_rejects_invalid_wire() {
    use crate::records::identity::DesignAffineTransform;
    let mut rows = cadmpeg_ir::transform::Transform::identity().rows();
    rows[0][1] = 2.0;
    rows[2][2] = 0.0;
    let placement = DesignAffineTransform::try_from(rows).unwrap();
    let wire = serde_json::to_value(rows).unwrap();
    assert_eq!(serde_json::to_value(placement).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<DesignAffineTransform>(wire).unwrap(),
        placement
    );
    rows[3][0] = 1.0;
    assert!(DesignAffineTransform::try_from(rows).is_err());
    assert!(
        serde_json::from_value::<DesignAffineTransform>(serde_json::to_value(rows).unwrap())
            .unwrap_err()
            .to_string()
            .contains("transform")
    );
    rows[3][0] = 0.0;
    rows[0][0] = f64::INFINITY;
    assert!(DesignAffineTransform::try_from(rows).is_err());
}

#[test]
fn xref_placement_requires_a_proper_rigid_transform() {
    use crate::records::xref::XrefPlacementTransform;

    let identity = cadmpeg_ir::transform::Transform::identity().rows();
    assert!(XrefPlacementTransform::try_from(identity).is_ok());

    let mut reflected = identity;
    reflected[0][0] = -1.0;
    assert!(XrefPlacementTransform::try_from(reflected).is_err());

    let mut scaled = identity;
    scaled[0][0] = 2.0;
    assert!(XrefPlacementTransform::try_from(scaled).is_err());

    let mut non_affine = identity;
    non_affine[3][0] = 1.0;
    assert!(XrefPlacementTransform::try_from(non_affine).is_err());
}
