// SPDX-License-Identifier: Apache-2.0
//! Writer unit tests.

use std::collections::BTreeMap;

use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;

use crate::writer::patch::edits::encode_sketch_relation_state;
use crate::writer::patch::geometry::{patch_framed_geometry, GeometryEdits};
use crate::writer::primitives::normalized_face_sense_to_native;
use cadmpeg_asm::edit::AsmEditSet;
use cadmpeg_asm::test_support::sab;

fn decode_surface(record: &cadmpeg_asm::sab::Record) -> Option<(SolvedSurfaceGeometry, bool)> {
    cadmpeg_asm::brep::geometry::decode_surface(record)
}

fn decode_curve(record: &cadmpeg_asm::sab::Record) -> Option<CurveGeometry> {
    cadmpeg_asm::brep::geometry::decode_curve(record)
}

#[test]
fn sketch_relation_state_encoding_requires_the_stored_width_discriminator() {
    let record = |discriminator| {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"298");
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        bytes.push(discriminator);
        bytes
    };

    assert_eq!(
        encode_sketch_relation_state("relation-u32", &record(0), 0x8000_0000).unwrap(),
        0x8000_0000u32.to_le_bytes()
    );
    assert_eq!(
        encode_sketch_relation_state("relation-u64", &record(1), 0x0020_0000_0000).unwrap(),
        0x0020_0000_0000_u64.to_le_bytes()
    );
    assert!(
        encode_sketch_relation_state("relation-unknown", &record(2), 1)
            .unwrap_err()
            .to_string()
            .contains("mask-width discriminator")
    );
    assert!(
        encode_sketch_relation_state("relation-u32", &record(0), 0x1_0000_0000)
            .unwrap_err()
            .to_string()
            .contains("cannot carry")
    );
}

#[test]
fn generated_face_sense_edit_preserves_native_normalization_relation() {
    assert_eq!(
        normalized_face_sense_to_native(Sense::Reversed, false),
        Sense::Reversed
    );
    assert_eq!(
        normalized_face_sense_to_native(Sense::Reversed, true),
        Sense::Forward
    );
}

#[test]
fn generated_straight_record_patches_by_token_boundaries() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x08straight");
    bytes.push(0x13);
    for value in [1.0f64, 2.0, 3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x14);
    for value in [1.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    let records = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated straight record");
    let lines = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (Point3::new(40.0, 50.0, 60.0), Vector3::new(0.0, 1.0, 0.0)),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &lines,
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &BTreeMap::new(),
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
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
    .expect("generated line edit");
    let decoded = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("patched generated straight record");
    assert!(
        matches!(decode_curve(&decoded[0]), Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)))
        if {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            origin == Point3::new(40.0, 50.0, 60.0)
                && direction == Vector3::new(0.0, 1.0, 0.0)
        })
    );
}

#[test]
fn generated_signed_sphere_patches_exact_frame_and_radius() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x06sphere");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x06);
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    for vector in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        bytes.push(0x14);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.push(0x11);
    let records = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated sphere record");
    let spheres = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            -25.0,
        ),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &spheres,
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
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
    .expect("generated sphere edit");
    let decoded = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("patched sphere record");
    assert!(
        matches!(decode_surface(&decoded[0]), Some((SolvedSurfaceGeometry::Sphere(sphere_surface), false))
                if {
                    let center = sphere_surface.center();
        let axis = sphere_surface.frame().axis().as_raw();
        let ref_direction = sphere_surface.frame().reference().as_raw();
        let radius = sphere_surface.radius().get();
                    *center == Point3::new(10.0, 20.0, 30.0)
                        && *axis == Vector3::new(0.0, 1.0, 0.0)
                        && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                        && radius == -25.0
                })
    );
}

#[test]
fn generated_plane_patch_keeps_its_reference_vector_length_and_sense() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x05plane");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    // The normal, then a reference vector of length 0.5.
    for vector in [[0.0f64, 0.0, 1.0], [0.5, 0.0, 0.0]] {
        bytes.push(0x14);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    // Reversed v-sense.
    bytes.push(0x0a);
    bytes.push(0x11);
    let records = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated plane record");
    let planes = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &BTreeMap::new(),
            degenerate_curves: &BTreeMap::new(),
            planes: &planes,
            spheres: &BTreeMap::new(),
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
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
    .expect("generated plane edit");
    let decoded = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("patched plane record");
    // The new reference direction keeps the stored length, so the plane's
    // pcurve chart is unchanged.
    assert_eq!(
        cadmpeg_asm::nurbs::support_chart::plane_record_chart(
            decoded[0].head(),
            &decoded[0].tokens
        ),
        Some(cadmpeg_asm::nurbs::support_chart::NativeChart::Plane {
            reverse_v: true,
            scale: 0.5,
        })
    );
    assert!(decoded[0]
        .tokens
        .contains(&cadmpeg_asm::sab::Token::Vector3([0.0, 0.0, 0.5])));
}

#[test]
fn generated_torus_preserves_signed_self_intersecting_radii() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x05torus");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x14);
    for value in [0.0f64, 0.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [1.0f64, 0.25] {
        bytes.push(0x06);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x14);
    for value in [1.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    let records = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated torus record");
    let tori = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            20.0,
            -35.0,
        ),
    )]);
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
            tori: &tori,
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
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
    .expect("generated torus edit");
    let decoded = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("patched torus record");
    assert!(
        matches!(decode_surface(&decoded[0]), Some((SolvedSurfaceGeometry::Torus(torus_surface), true))
                if {
                    let center = torus_surface.center();
        let axis = torus_surface.frame().axis().as_raw();
        let ref_direction = torus_surface.frame().reference().as_raw();
        let major_radius = torus_surface.major_radius().get();
        let minor_radius = torus_surface.minor_radius().get();
                    *center == Point3::new(10.0, 20.0, 30.0)
                        && *axis == Vector3::new(0.0, 1.0, 0.0)
                        && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                        && major_radius == 20.0
                        && minor_radius == -35.0
                })
    );
}

#[test]
fn generated_cylinder_preserves_native_angle_branch() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x04cone");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
        bytes.push(0x14);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for value in [1.0f64, 0.0, -1.0, 1.0] {
        bytes.push(0x06);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    let records = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated cylinder record");
    let cones = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            40.0,
            1.0,
            0.0,
        ),
    )]);
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
            cones: &cones,
            body_transforms: &BTreeMap::new(),
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
    .expect("generated cylinder edit");
    let decoded = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("patched cylinder record");
    // The patch preserves the record's native negative-cosine angle
    // branch, so decode reports the inward-normal flag.
    assert!(
        matches!(decode_surface(&decoded[0]), Some((SolvedSurfaceGeometry::Cylinder(cylinder_surface), true))
                if {
                    let origin = cylinder_surface.origin();
        let axis = cylinder_surface.frame().axis().as_raw();
        let ref_direction = cylinder_surface.frame().reference().as_raw();
        let radius = cylinder_surface.radius().get();
                    *origin == Point3::new(10.0, 20.0, 30.0)
                        && *axis == Vector3::new(0.0, 1.0, 0.0)
                        && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
                        && radius == 40.0
                })
    );
}

#[test]
fn generated_ellipse_preserves_negative_ratio_phase() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x0d\x07ellipse");
    bytes.push(0x13);
    for value in [0.0f64, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
        bytes.push(0x14);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.push(0x06);
    bytes.extend_from_slice(&(-0.5f64).to_le_bytes());
    bytes.push(0x11);
    let records = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated ellipse record");
    let conics = BTreeMap::from([(
        "f3d:brep:entity#0".to_string(),
        (
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            40.0,
            10.0,
        ),
    )]);
    patch_framed_geometry(
        &mut bytes,
        &records,
        &GeometryEdits {
            positions: &BTreeMap::new(),
            lines: &BTreeMap::new(),
            conics: &conics,
            degenerate_curves: &BTreeMap::new(),
            planes: &BTreeMap::new(),
            spheres: &BTreeMap::new(),
            tori: &BTreeMap::new(),
            cones: &BTreeMap::new(),
            body_transforms: &BTreeMap::new(),
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
    .expect("generated ellipse edit");
    let ratio_offset = cadmpeg_asm::test_support::sab::payload_token_offsets(
        &bytes,
        &records[0],
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        0x06,
    )
    .expect("ellipse tokens")[0];
    assert_eq!(
        f64::from_le_bytes(
            bytes[ratio_offset + 1..ratio_offset + 9]
                .try_into()
                .expect("ratio payload"),
        ),
        -0.25
    );
    let decoded = sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("patched ellipse record");
    assert!(
        matches!(decode_curve(&decoded[0]), Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)))
                if {
                    let center = ellipse_curve.center().get();
        let axis = ellipse_curve.frame().axis().as_raw();
        let major_direction = ellipse_curve.frame().reference().as_raw();
        let major_radius = ellipse_curve.major_radius().get();
        let minor_radius = ellipse_curve.minor_radius().get();
                    center == Point3::new(10.0, 20.0, 30.0)
                        && *axis == Vector3::new(0.0, 1.0, 0.0)
                        && *major_direction == Vector3::new(1.0, 0.0, 0.0)
                        && major_radius == 40.0
                        && minor_radius == 10.0
                })
    );
}

#[test]
fn generated_binaryfile4_integer_patch_preserves_following_token() {
    let mut bytes = vec![0x15];
    bytes.extend_from_slice(&(-3i32).to_le_bytes());
    bytes.extend_from_slice(&[0x0d, 0x03, b'n', b'e', b'x']);

    AsmEditSet::patch_tagged_integer_at(
        &mut bytes,
        0,
        cadmpeg_asm::kernel_header::RefWidth::Four,
        7,
    )
    .expect("width-4 enum patch");

    assert_eq!(&bytes[1..5], &7i32.to_le_bytes());
    assert_eq!(&bytes[5..], &[0x0d, 0x03, b'n', b'e', b'x']);
}
