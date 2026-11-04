// SPDX-License-Identifier: Apache-2.0

use super::{patch_framed_geometry, retained_plane_pcurve, GeometryEdits};
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_ir::transform::Transform;
use std::collections::BTreeMap;

#[test]
fn body_transform_reference_refuses_32_bit_wrap() {
    let records = [
        Record {
            index: 0,
            name: "transform".to_owned(),
            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        },
        Record {
            index: 1,
            name: "body".to_owned(),
            tokens: vec![
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(4_294_967_296),
            ]
            .into(),
            offset: 0,
            len: 0,
        },
    ];
    let transforms = BTreeMap::from([("f3d:brep:entity#1".to_owned(), Transform::identity())]);
    let mut bytes = Vec::new();
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &BTreeMap::new(),
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &transforms,
            entity_colors: &BTreeMap::new(),
            edge_ranges: &BTreeMap::new(),
            face_senses: &BTreeMap::new(),
            coedge_senses: &BTreeMap::new(),
            procedural_surface_edits: &BTreeMap::new(),
            nurbs_surfaces: &BTreeMap::new(),
            nurbs_curves: &BTreeMap::new(),
            pcurves: &BTreeMap::new(),
            procedural_curve_edits: &BTreeMap::new(),
            procedural_surface_fits: &BTreeMap::new(),
            creation_timestamps: &BTreeMap::new(),
            edge_continuities: &BTreeMap::new(),
            vertex_ownerships: &BTreeMap::new(),
            face_sidedness: &BTreeMap::new(),
            tolerant_edges: &BTreeMap::new(),
            tolerant_vertices: &BTreeMap::new(),
        },
        1.0,
    )
    .expect("an unaddressable reference names no transform record");
    assert!(bytes.is_empty());
}

#[test]
fn an_edited_pcurve_moves_into_the_chart_of_its_retained_plane() {
    use cadmpeg_asm::nurbs::support_chart::NativeChart;
    use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
    use cadmpeg_ir::math::Point2;
    // Poles in the writer's unit chart: plane coordinates in internal
    // centimetres.
    let unit = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(1.0, 2.0), Point2::new(-3.0, 0.5)],
        None,
        false,
    )
    .expect("unit-chart pcurve");
    let chart = NativeChart::Plane {
        reverse_v: true,
        scale: 0.5,
    };
    let retained = retained_plane_pcurve(&unit, chart).expect("retained chart");
    let points = retained
        .control_points()
        .into_iter()
        .map(cadmpeg_ir::units::FinitePoint2::get)
        .collect::<Vec<_>>();
    // Coordinates in units of the half-length reference vector, v negated.
    assert_eq!(points, [Point2::new(2.0, -4.0), Point2::new(-6.0, -1.0)]);
    // Both charts map the retained poles to the same plane points.
    for (unit_point, retained_point) in unit.control_points().into_iter().zip(&points) {
        let unit_chart = NativeChart::Plane {
            reverse_v: false,
            scale: 1.0,
        };
        assert_eq!(
            unit_chart.frame_point(unit_point.get()),
            chart.frame_point(*retained_point)
        );
    }
}
