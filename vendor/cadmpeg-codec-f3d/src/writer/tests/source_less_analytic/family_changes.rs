// SPDX-License-Identifier: Apache-2.0

use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

#[test]
fn generated_f3d_rewrites_plane_frame() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
                &DecodeOptions::default(),
            )
            .expect("generated planar triangle decode"),
    );
    let mut edited = decoded.ir().clone();
    let expected = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            cadmpeg_ir::math::Point3::new(10.0, -20.0, 30.0),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    edited.model.surfaces[0].geometry = expected.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, decoded.source_fidelity(), &mut regenerated)
        .expect("plane frame regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated plane decode");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_f3d_rejects_analytic_surface_family_changes() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
                &DecodeOptions::default(),
            )
            .expect("generated planar triangle decode"),
    );
    let mut edited = decoded.ir().clone();
    edited.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            5.0,
        )
        .unwrap(),
    ));

    let error = crate::test_support::plan_inherited_write(
        &edited,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .expect_err("native plane record cannot silently retain a sphere edit");
    assert!(error
        .to_string()
        .contains("does not support edits to surface"));
}
