// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_core::convert::f64_from_index;

use cadmpeg_test_support::edit;

use std::io::{Cursor, Write};

use cadmpeg_asm::asm_header;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};
use zip::CompressionMethod;

use crate::loss::F3dLossCode;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::native_test::TestEncode;
use crate::test_support::procedural_test::{
    generated_form_two_par_int_cur, push_optional_value_quartet, push_revision_cl_scale,
};
use crate::test_support::smbh_blends_test::{
    append_generated_variable_blend_side, synthetic_rational_cyl_spl_sur_smbh,
    synthetic_ref_cyl_spl_sur_smbh, synthetic_revision_ref_directrix_cyl_spl_sur_smbh,
    synthetic_variable_blend_smbh_with_selector, synthetic_vertex_blend_smbh,
};
use crate::test_support::smbh_blocks_test::{
    generated_curve_block, generated_pcurve_block, generated_surface_block,
};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::smbh_revision_test::{
    assert_parameterized_tail, push_parameterized_revision_surface_tail,
    push_revision_surface_tail, synthetic_revision_surface_smbh,
};
use crate::test_support::smbh_surfaces_test::{
    synthetic_cyl_spl_sur_smbh, synthetic_versioned_cyl_spl_sur_smbh,
    synthetic_versioned_cyl_spl_sur_with_trailing_token_smbh,
};
use crate::test_support::tokens_test::{
    push_tagged_i64, t_dbl, t_ident, t_long, t_pos, t_u16_string, t_vec,
};
use crate::test_support::zip_test::{assert_revision_surface_round_trip, f3d_with_smbh};
use crate::F3dCodec;

#[test]
fn generated_revision_exact_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
    });
    assert_revision_surface_round_trip(smbh, "exact");
}

#[test]
fn generated_revision_exact_surface_carries_two_unextended_intervals() {
    use cadmpeg_ir::geometry::{ExactSpline, ProceduralSurfaceDefinition};

    // Two distinct non-[0,1] unextended parameter intervals: U then V.
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        for value in [0.0, std::f64::consts::FRAC_PI_2, 0.5, 2.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        push_tagged_i64(surface, 0x15, 0);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision exact decode");
    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Exact(definition_payload) = procedural.definition() else {
        panic!("expected exact definition");
    };
    let spline = definition_payload.spline().to_raw();

    let ExactSpline::Revision { intervals, .. } = &spline else {
        panic!("expected revision exact-spline layout")
    };
    assert_eq!(
        *intervals,
        [
            [Some(0.0), Some(std::f64::consts::FRAC_PI_2)],
            [Some(0.5), Some(2.0)],
        ]
    );
    assert_revision_surface_round_trip(smbh, "exact");
}

#[test]
fn generated_revision_loft_surface_carries_one_nonempty_wrap_interval() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SplineSurfaceParameters};

    // First wrap interval non-empty [0,1]; second reversed [1,0] = empty.
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        t_long(surface, 1);
        t_dbl(surface, 0.0);
        t_long(surface, 1);
        t_long(surface, 1);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        t_ident(surface, "null_surface");
        t_ident(surface, "nullbs");
        surface.push(0x0b);
        t_long(surface, -1);
        t_long(surface, 213);
        t_long(surface, 1);
        t_long(surface, 1);
        for value in [0.0, 1.0, 0.25, 0.75, 0.5, 1.5] {
            t_dbl(surface, value);
        }
        surface.push(0x0b);
        t_ident(surface, "null_curve");
        t_long(surface, 0);
        t_long(surface, -1);
        t_long(surface, 0);
        for value in [0.0, 1.0, 1.0, 0.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 0);
        t_long(surface, 0);
        push_revision_surface_tail(surface);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision loft decode");
    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Loft(definition_payload) = procedural.definition() else {
        panic!("expected loft definition");
    };
    let parameters = definition_payload.parameters().to_raw();

    assert_eq!(
        &parameters,
        &SplineSurfaceParameters::RevisionRanges {
            intervals: [[Some(0.0), Some(1.0)], [Some(1.0), Some(0.0)]],
        }
    );
    assert_revision_surface_round_trip(smbh, "loft");
}

#[test]
fn generated_revision_sum_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("sum_spl_sur", |surface| {
        for (lower, upper) in [(0.0, 1.0), (-2.0, 2.0)] {
            surface.extend_from_slice(&generated_curve_block());
            surface.push(0x0a);
            t_dbl(surface, lower);
            surface.push(0x0a);
            t_dbl(surface, upper);
        }
        t_pos(surface, [1.0, 2.0, 3.0]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "sum");
}

#[test]
fn generated_revision_rot_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("rot_spl_sur", |surface| {
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        t_pos(surface, [0.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "revolution");
}

#[test]
fn generated_revision_t_spline_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("t_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        for value in [0.0, 1.0, 0.0, 1.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        push_tagged_i64(surface, 0x15, 0);
        surface.push(0x0f);
        t_ident(surface, "t_spl_subtrans_object");
        t_u16_string(
            surface,
            "degree 3\nunits mm\nv 1 0 0 0\nv 2 1 0 0\ne 1 1 2\n",
        );
        surface.push(0x0b);
        t_u16_string(surface, "100verts 1 2\n");
        surface.push(0x10);
        t_long(surface, 2);
    });
    assert_revision_surface_round_trip(smbh, "t_spline");
}

#[test]
fn generated_revision_g2_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("g2_blend_spl_sur", |surface| {
        t_dbl(surface, 1.0);
        t_dbl(surface, 1.0);
        append_generated_variable_blend_side(surface, "left", 1.0);
        append_generated_variable_blend_side(surface, "right", 4.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.5);
        surface.push(0x0a);
        t_dbl(surface, 2.5);
        t_dbl(surface, 0.125);
        t_dbl(surface, 0.125);
        push_tagged_i64(surface, 0x15, -1);
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 1);
        t_dbl(surface, 0.001);
        t_dbl(surface, 0.0001);
        t_long(surface, 1);
        push_revision_surface_tail(surface);
        for value in [0, 0, 0] {
            t_long(surface, value);
        }
    });
    assert_revision_surface_round_trip(smbh, "revision_g2_blend");
}

#[test]
fn generated_parameterized_revision_g2_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("g2_blend_spl_sur", |surface| {
        t_dbl(surface, 1.0);
        t_dbl(surface, 1.0);
        append_generated_variable_blend_side(surface, "left", 1.0);
        append_generated_variable_blend_side(surface, "right", 4.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.5);
        surface.push(0x0a);
        t_dbl(surface, 2.5);
        t_dbl(surface, 0.125);
        t_dbl(surface, 0.125);
        push_tagged_i64(surface, 0x15, -1);
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 1);
        t_dbl(surface, 0.001);
        t_dbl(surface, 0.0001);
        t_long(surface, 1);
        push_parameterized_revision_surface_tail(surface);
        for value in [0, 0, 0] {
            t_long(surface, value);
        }
    });
    assert_revision_surface_round_trip(smbh.clone(), "revision_g2_blend");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision g2 blend decode");
    let procedural = &result.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RevisionG2Blend { construction } =
        procedural.definition()
    else {
        panic!("expected a revision g2 blend construction")
    };
    assert_parameterized_tail(&construction.cache().to_raw());
}

#[test]
fn generated_revision_vertex_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("VBL_SURF", |surface| {
        t_long(surface, 2);

        t_ident(surface, "circle");
        surface.push(0x0a);
        t_vec(surface, [0.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.1);
        surface.push(0x0a);
        t_dbl(surface, 0.9);
        push_tagged_i64(surface, 0x15, 3);
        t_vec(surface, [0.0, 0.0, 0.5]);
        t_vec(surface, [0.5, 0.0, 0.0]);
        t_dbl(surface, 0.1);
        t_dbl(surface, 0.9);
        surface.push(0x0b);

        t_ident(surface, "pcurve");
        surface.push(0x0b);
        t_vec(surface, [0.0, 0.0, 0.0]);
        surface.push(0x0a);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        t_ident(surface, "plane");
        t_pos(surface, [0.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        t_vec(surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_pcurve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.002);

        t_long(surface, 9);
        t_dbl(surface, 0.003);
    });
    assert_revision_surface_round_trip(smbh, "vertex_blend");
}

#[test]
fn generated_revision_offset_with_inline_untyped_support_decodes() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.push(0x0b);
        surface.push(0x0f);
        t_ident(surface, "mystery_spl_sur");
        t_long(surface, 23100);
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x10);
        surface.extend_from_slice(&[0x0b; 4]);
        t_dbl(surface, 0.3);
        surface.extend_from_slice(&[0x0b; 4]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "offset");
}

#[test]
fn generated_single_radius_variable_blend_decodes_explicit_circular_cross_section() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                false,
                Some(0),
                [None, None],
            ))),
            &DecodeOptions::default(),
        )
        .expect("single-radius selector-zero decode");
    let ProceduralSurfaceDefinition::VariableBlend(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected variable blend")
    };
    let construction = &definition_payload.construction().to_raw();

    assert!(matches!(
        &construction.cross_section,
        Some(cadmpeg_ir::geometry::VariableBlendCrossSection::Circular {})
    ));
    let expected = construction.clone();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("selector-zero source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("selector-zero round trip");
    assert!(matches!(
        &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::VariableBlend(definition_payload) if matches!((definition_payload.construction(),), (construction,) if construction.to_raw() == expected)));
}

#[test]
fn generated_variable_blend_round_trips_parameterized_cross_sections() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendCrossSection};

    for (selector, expected_cross_section) in [
        (
            1,
            VariableBlendCrossSection::Thumbweights {
                parameters: [2.0, 2.0],
            },
        ),
        (
            7,
            VariableBlendCrossSection::G2Round {
                parameters: [2.0, 2.0],
            },
        ),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                    "srf_srf_v_bl_spl_sur",
                    false,
                    Some(selector),
                    [None, None],
                ))),
                &DecodeOptions::default(),
            )
            .expect("parameterized cross-section decode");
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) =
            &decoded.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected variable blend")
        };
        let construction = &definition_payload.construction().to_raw();

        assert_eq!(
            construction.cross_section.as_ref(),
            Some(&expected_cross_section)
        );

        let expected = construction.clone();
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("parameterized cross-section source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("parameterized cross-section round trip");
        assert!(matches!(
            &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::VariableBlend(definition_payload) if matches!((definition_payload.construction(),), (construction,) if construction.to_raw() == expected)));
    }
}

#[test]
fn generated_variable_blend_round_trips_unclassified_bare_cross_sections() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendBareCrossSection, VariableBlendCrossSection,
    };

    for (selector, expected) in [
        (2, VariableBlendBareCrossSection::Selector2),
        (4, VariableBlendBareCrossSection::Selector4),
        (5, VariableBlendBareCrossSection::Selector5),
        (6, VariableBlendBareCrossSection::Selector6),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                    "srf_srf_v_bl_spl_sur",
                    false,
                    Some(selector),
                    [None, None],
                ))),
                &DecodeOptions::default(),
            )
            .expect("bare cross-section decode");
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) =
            &decoded.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected variable blend")
        };
        let construction = &definition_payload.construction().to_raw();

        assert_eq!(
            construction.cross_section,
            Some(VariableBlendCrossSection::UnclassifiedBare { selector: expected })
        );

        let expected_construction = construction.clone();
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("bare cross-section source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("bare cross-section round trip");
        assert!(matches!(
            &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::VariableBlend(definition_payload) if matches!((definition_payload.construction(),), (construction,) if construction.to_raw() == expected_construction)));
    }
}

#[test]
fn generated_revision_compound_loft_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, true);
        t_long(surface, 2);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 0.0);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0b);
        surface.push(0x0b);
    });
    assert_revision_surface_round_trip(smbh, "revision_compound_loft");
}

#[test]
fn generated_parameterized_revision_compound_loft_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_parameterized_revision_surface_tail(surface);
        push_revision_cl_scale(surface, true);
        t_long(surface, 2);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 0.0);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0b);
        surface.push(0x0b);
    });
    assert_revision_surface_round_trip(smbh.clone(), "revision_compound_loft");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision compound loft decode");
    let procedural = &result.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } =
        procedural.definition()
    else {
        panic!("expected a revision compound loft construction")
    };
    assert_parameterized_tail(&construction.cache().to_raw());
}

#[test]
fn generated_revision_compound_loft_trailing_curve_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, false);
        t_long(surface, 1);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.extend_from_slice(&generated_curve_block());
    });
    assert_revision_surface_round_trip(smbh, "revision_compound_loft");
}

#[test]
fn generated_revision_compound_loft_rejects_present_parameters_without_a_curve() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    // The trailing curve is present exactly when both parameter values are
    // present, so a payload carrying two present values and closing straight
    // away is not a legal record. The decoder reads the curve on the parameter
    // pair alone; it does not look ahead for the subtype-close byte.
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, false);
        t_long(surface, 1);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("decode retains the record as a native unknown");
    assert!(!decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition(),
            ProceduralSurfaceDefinition::RevisionCompoundLoft { .. }
        )));

    let legal = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_revision_surface_smbh(
                "cl_loft_spl_sur",
                |surface| {
                    push_revision_surface_tail(surface);
                    push_revision_cl_scale(surface, false);
                    t_long(surface, 1);
                    push_revision_cl_scale(surface, false);
                    t_dbl(surface, 1.0);
                    surface.push(0x0b);
                    surface.push(0x0b);
                    t_long(surface, 0);
                    surface.push(0x0b);
                    surface.push(0x0b);
                    t_long(surface, 0);
                    t_vec(surface, [0.0, 0.0, 1.0]);
                    surface.push(0x0a);
                    t_dbl(surface, 1.0);
                    surface.push(0x0a);
                    t_dbl(surface, 0.0);
                    surface.extend_from_slice(&generated_curve_block());
                },
            ))),
            &DecodeOptions::default(),
        )
        .expect("legal revision compound loft decode")
        .into_parts()
        .0;
    let mut wire = serde_json::to_value(&legal.model.procedural_surfaces[0]).unwrap();
    assert_eq!(wire["definition"]["construction"]["tail"]["kind"], "curve");
    wire["definition"]["construction"]["tail"]
        .as_object_mut()
        .unwrap()
        .remove("curve");
    let error =
        serde_json::from_value::<cadmpeg_ir::geometry::ProceduralSurface>(wire).unwrap_err();
    assert!(error.to_string().contains("curve"), "{error}");
}

#[test]
fn decode_carries_the_document_modeling_length_unit_into_source_metadata() {
    // The `Custom` system's `modelingLengthName` is the document's display
    // length unit. It reaches `SourceMeta`, not `CadIr::units`: no stored
    // quantity depends on it, and model-space coordinates stay centimetres
    // under every value.
    let design = crate::design::decode::units::tests::stream([
        "centimeter",
        "millimeter",
        "meter",
        "inch",
        "foot",
        "inch",
    ]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(&synthetic_geometry_smbh()).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&design).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("decode with a unit-systems design stream");
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("modeling_length_unit"))
            .map(String::as_str),
        Some("inch")
    );
}

#[test]
fn record_level_surface_bounds_round_trip() {
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("exact revision decode");
    let (mut source_less, _, _) = decoded.into_parts();
    assert_eq!(
        source_less.model.procedural_surfaces[0]
            .record_bounds()
            .map(cadmpeg_ir::geometry::RecordBounds::get),
        None
    );
    {
        let replacement = Some([Some(0.1), None, Some(0.2), None]);
        edit::replace(&mut source_less.model.procedural_surfaces[0], |previous| {
            replacement
                .map(cadmpeg_ir::geometry::RecordBounds::try_from)
                .transpose()
                .map(|bounds| {
                    cadmpeg_ir::geometry::ProceduralSurface::new(
                        previous.id.clone(),
                        previous.definition().clone(),
                        bounds,
                    )
                })
        })
    }
    .expect("finite record bounds");
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("record-bounds encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("record-bounds round trip");
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0]
            .record_bounds()
            .map(cadmpeg_ir::geometry::RecordBounds::get),
        Some([Some(0.1), None, Some(0.2), None])
    );
}

#[test]
fn generated_vertex_blends_decode_all_boundary_variants() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, SurfaceGeometry, VertexBlendBoundaryGeometry,
    };

    for name in ["VBL_SURF", "vertexblendsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_vertex_blend_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("vertex-blend decode");
        let ProceduralSurfaceDefinition::VertexBlend(definition_payload) =
            &result.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected vertex blend")
        };
        let construction = &definition_payload.construction().to_raw();

        let owner = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| {
                result
                    .ir()
                    .model
                    .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
                    == Some(&surface.id)
            })
            .expect("vertex-blend owner");
        assert!(
            matches!(
                owner.geometry,
                SurfaceGeometry::Procedural { ref construction, .. }
                    if *construction == result.ir().model.procedural_surfaces[0].id
            ),
            "unexpected vertex-blend carrier: {:?}",
            owner.geometry
        );
        assert_eq!(construction.boundaries.len(), 4);
        assert_eq!(construction.grid_size, 17);
        assert_eq!(construction.fit_tolerance.get(), 0.03);
        let VertexBlendBoundaryGeometry::Circle {
            twists,
            parameters,
            sense,
            ..
        } = &construction.boundaries[0].geometry
        else {
            panic!("expected circle boundary")
        };
        assert_eq!(twists.form(), 1);
        assert_eq!(
            twists.entries(),
            &[cadmpeg_ir::math::Point3::new(20.0, 30.0, 40.0)]
        );
        assert_eq!(*parameters, [0.1, 0.9]);
        assert!(!*sense);
        assert!(matches!(
            construction.boundaries[1].geometry,
            VertexBlendBoundaryGeometry::Degenerate { .. }
        ));
        assert!(matches!(
            construction.boundaries[2].geometry,
            VertexBlendBoundaryGeometry::Pcurve {
                pcurve: Some(_),
                ..
            }
        ));
        assert!(matches!(
            construction.boundaries[3].geometry,
            VertexBlendBoundaryGeometry::Plane { .. }
        ));
        let bounded_curves =
            [0usize, 3].map(|ordinal| match &construction.boundaries[ordinal].geometry {
                VertexBlendBoundaryGeometry::Circle {
                    curve, parameters, ..
                }
                | VertexBlendBoundaryGeometry::Plane {
                    curve, parameters, ..
                } => (curve.clone(), *parameters),
                _ => unreachable!(),
            });

        let expected = construction.clone();
        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        for (ordinal, (curve, _)) in bounded_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|candidate| candidate.id == *curve)
                .expect("vertex-blend boundary curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(
                        f64_from_index(ordinal).expect("fixture index is exact in f64"),
                        2.0,
                        -3.0,
                    ),
                    cadmpeg_ir::math::Vector3::new(2.0, -1.0, 4.0)
                        .unit()
                        .unwrap(),
                )
                .unwrap(),
            ));
        }
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less vertex-blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less vertex-blend round trip");
        let ProceduralSurfaceDefinition::VertexBlend(definition_payload) =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip vertex blend")
        };
        let actual = &definition_payload.construction().to_raw();

        assert_eq!(actual, &expected);
        for (curve, range) in bounded_curves {
            assert!(matches!(
                round_trip
                    .ir()
                    .model
                    .curves
                    .iter()
                    .find(|candidate| candidate.id == curve)
                    .map(|curve| &curve.geometry),
                Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
                    if curve.degree() == 1
                        && curve.knots().as_slice() == [range[0], range[0], range[1], range[1]]
            ));
        }
    }
}

#[test]
fn decode_retains_generated_translational_extrusion_and_fit_contract() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let f3d = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    assert_eq!(
        procedural
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.02)
    );
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) = procedural.definition() else {
        panic!("expected extrusion")
    };
    let direction = definition_payload.direction();
    let directrix = definition_payload.directrix();
    let parameter_interval = definition_payload.parameter_interval();
    let native_position = definition_payload.native_position();
    let None = definition_payload.revision_form() else {
        panic!("expected extrusion")
    };
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        parameter_interval.map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.25, 0.75])
    );
    assert_eq!(
        native_position.map(cadmpeg_ir::features::FinitePoint3::get),
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
    let directrix = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .expect("extrusion directrix carrier");
    let Some(SolvedCurveGeometry::Nurbs(directrix)) = directrix.geometry.solved() else {
        panic!("expected NURBS directrix")
    };
    assert_eq!(directrix.control_points().len(), 3);
}

#[test]
fn decode_retains_versioned_nested_translational_extrusion() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_versioned_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("versioned extrusion decode");
    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    assert_eq!(
        procedural
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.02)
    );
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) = procedural.definition() else {
        panic!("expected versioned extrusion")
    };
    let direction = definition_payload.direction();
    let parameter_interval = definition_payload.parameter_interval();
    let native_position = definition_payload.native_position();
    assert_eq!(
        parameter_interval.map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.25, 0.75])
    );
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        native_position.map(cadmpeg_ir::features::FinitePoint3::get),
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
}

#[test]
fn revision_cylinder_rejects_tokens_after_its_terminal_surface_tail() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_versioned_cyl_spl_sur_with_trailing_token_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("opaque revision-cylinder decode");

    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition(),
            ProceduralSurfaceDefinition::Unknown { .. }
        )));
}

#[test]
fn generated_f3d_rewrites_translational_extrusion_header() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Extrusion(definition_payload) = definition else {
            panic!("expected extrusion")
        };
        let mut parameter_interval_value = definition_payload
            .parameter_interval()
            .map(cadmpeg_ir::units::FiniteVector::get);
        let parameter_interval = &mut parameter_interval_value;
        let mut direction_value = definition_payload.direction().get();
        let direction = &mut direction_value;
        let mut native_position_value = definition_payload
            .native_position()
            .map(cadmpeg_ir::features::FinitePoint3::get);
        let native_position = &mut native_position_value;
        {
            *parameter_interval = Some([-0.5, 1.25]);
            *direction = cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0);
            *native_position = Some(cadmpeg_ir::math::Point3::new(-20.0, 70.0, 15.0));
        };
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload =
            cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                definition_payload.directrix().clone(),
                parameter_interval_value,
                direction_value,
                native_position_value,
                cadmpeg_ir::geometry::CacheContract::from_form(
                    definition_payload
                        .revision_form()
                        .map(cadmpeg_ir::geometry::RevisionSurfaceForm::to_raw),
                ),
            )
            .unwrap();
        definition
            .set_legacy_cache(restored_cache)
            .expect("the rebuilt construction states the same legacy cache slot");
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("extrusion-direction regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip extrusion")
    };
    let parameter_interval = definition_payload.parameter_interval();
    let direction = definition_payload.direction();
    let native_position = definition_payload.native_position();
    assert_eq!(
        parameter_interval.map(cadmpeg_ir::units::FiniteVector::get),
        Some([-0.5, 1.25])
    );
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0));
    assert_eq!(
        native_position.map(cadmpeg_ir::features::FinitePoint3::get),
        Some(cadmpeg_ir::math::Point3::new(-20.0, 70.0, 15.0))
    );
}

#[test]
fn generated_f3d_rewrites_procedural_surface_fit_tolerance() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated procedural-surface decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.procedural_surfaces[0]
        .set_cache_fit_tolerance(Some(0.075))
        .unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("procedural-surface fit regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated procedural-surface decode");
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.075)
    );
}

#[test]
fn generated_f3d_rewrites_nurbs_surface_control_grid() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated NURBS surface decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                surface.geometry.solved_cache(),
                Some(SolvedSurfaceGeometry::Nurbs(_))
            )
        })
        .expect("generated NURBS surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Procedural {
        cache: Some(cache), ..
    } = &mut surface.geometry
    else {
        panic!("procedural carrier with a solved cache")
    };
    let SolvedSurfaceGeometry::Nurbs(mut nurbs) = cache.clone() else {
        unreachable!()
    };
    let target = nurbs.v_count();
    nurbs
        .try_map_control_points(|index, pole| {
            let mut pole = pole.get();
            if index == target {
                pole.x = 17.5;
                pole.z = -3.25;
            }
            cadmpeg_ir::features::FinitePoint3::new(pole).ok_or_else(|| {
                cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".into(),
                )
            })
        })
        .expect("pole edit admission");
    edit::replace(&mut nurbs, |previous| {
        let mut knots = previous.u_knots().to_vec();
        {
            let knots: &mut [f64] = &mut knots;
            knots.copy_from_slice(&[-1.0, -1.0, 2.0, 2.0]);
        };
        cadmpeg_ir::geometry::nurbs::NurbsSurface::new(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.u_degree(),
                knots,
                previous.u_periodic(),
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.v_degree(),
                previous.v_knots().to_vec(),
                previous.v_periodic(),
            ),
            previous.pole_grid().clone(),
            previous.normal_reversed(),
        )
    })
    .unwrap();
    edit::replace(&mut nurbs, |previous| {
        let mut knots = previous.v_knots().to_vec();
        {
            let knots: &mut [f64] = &mut knots;
            knots.copy_from_slice(&[-0.5, -0.5, 1.5, 1.5]);
        };
        cadmpeg_ir::geometry::nurbs::NurbsSurface::new(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.u_degree(),
                previous.u_knots().to_vec(),
                previous.u_periodic(),
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.v_degree(),
                knots,
                previous.v_periodic(),
            ),
            previous.pole_grid().clone(),
            previous.normal_reversed(),
        )
    })
    .unwrap();
    {
        let replacement = true;
        edit::replace(&mut nurbs, |previous| {
            cadmpeg_ir::geometry::nurbs::NurbsSurface::new(
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    previous.u_degree(),
                    previous.u_knots().to_vec(),
                    replacement,
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    previous.v_degree(),
                    previous.v_knots().to_vec(),
                    previous.v_periodic(),
                ),
                previous.pole_grid().clone(),
                previous.normal_reversed(),
            )
        })
        .unwrap()
    };
    *cache = SolvedSurfaceGeometry::Nurbs(nurbs.clone());
    let expected = nurbs.clone();
    let surface_id = surface.id.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("NURBS surface regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated NURBS surface decode");
    let surface = round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .expect("round-trip NURBS surface");
    assert_eq!(
        *surface.geometry.solved_cache().expect("solved NURBS cache"),
        SolvedSurfaceGeometry::Nurbs(expected)
    );
}

#[test]
fn generated_f3d_rewrites_rational_nurbs_surface_weights() {
    let source = f3d_with_smbh(&synthetic_rational_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rational surface decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                surface.geometry.solved_cache(),
                Some(SolvedSurfaceGeometry::Nurbs(nurbs))
                    if nurbs.weights().is_some()
            )
        })
        .expect("generated rational surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Procedural {
        cache: Some(cache), ..
    } = &mut surface.geometry
    else {
        panic!("procedural carrier with a solved cache")
    };
    let SolvedSurfaceGeometry::Nurbs(mut nurbs) = cache.clone() else {
        unreachable!()
    };
    let mut weight_rows = nurbs.pole_grid().weights();
    if let Some(rows) = &mut weight_rows {
        rows[0][1] = 0.65;
    }
    let poles = cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::from_lanes(
        nurbs.pole_grid().raw_points(),
        weight_rows,
    );
    {
        let replacement = poles.unwrap();
        edit::replace(&mut nurbs, |previous| {
            cadmpeg_ir::geometry::nurbs::NurbsSurface::new(
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    previous.u_degree(),
                    previous.u_knots().to_vec(),
                    previous.u_periodic(),
                ),
                cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                    previous.v_degree(),
                    previous.v_knots().to_vec(),
                    previous.v_periodic(),
                ),
                replacement,
                previous.normal_reversed(),
            )
        })
    }
    .unwrap();
    *cache = SolvedSurfaceGeometry::Nurbs(nurbs.clone());
    let expected = nurbs.clone();
    let surface_id = surface.id.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("rational-weight regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rational surface decode");
    let surface = round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .expect("round-trip rational surface");
    assert_eq!(
        *surface.geometry.solved_cache().expect("solved NURBS cache"),
        SolvedSurfaceGeometry::Nurbs(expected)
    );
}

#[test]
fn generated_f3d_rewrites_extrusion_directrix_control_points() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) =
        edited.model.procedural_surfaces[0].definition()
    else {
        panic!("expected extrusion")
    };
    let directrix = definition_payload.directrix();
    let directrix_id = directrix.clone();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == directrix_id)
        .expect("extrusion directrix");
    let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) =
        &mut curve.geometry
    else {
        panic!("expected NURBS directrix")
    };
    let mut control_points = nurbs.pole_rows().raw_points();
    control_points[1].y = 12.5;
    control_points[1].z = -2.0;
    *nurbs = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![-2.0, -2.0, 3.0, 3.0, 3.0],
        control_points,
        nurbs.pole_rows().weights(),
        true,
    )
    .unwrap();
    let expected = nurbs.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("extrusion-directrix regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let curve = round_trip
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == directrix_id)
        .expect("round-trip directrix");
    assert_eq!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(expected))
    );
}

#[test]
fn decode_resolves_generated_ref_translational_extrusion() {
    let f3d = f3d_with_smbh(&synthetic_ref_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir().model.procedural_surfaces[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.02)
    );
}

#[test]
fn decode_resolves_revision_extrusion_implicit_directrix_reference() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let f3d = f3d_with_smbh(&synthetic_revision_ref_directrix_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert!(matches!(
        result.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::Extrusion(_)
    ));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == F3dLossCode::SurfaceShapeNotDecoded.kind() }));
}

#[test]
fn subtype_reference_resolves_surface_cache() {
    let mut target = Vec::new();
    target.extend_from_slice(b"\x0f\x0d\x07surface");
    // A payload byte equal to SUBTYPE_CLOSE must not terminate the span.
    target.push(0x06);
    target.extend_from_slice(&[0x10, 0, 0, 0, 0, 0, 0, 0]);
    target.extend_from_slice(&generated_surface_block());
    target.push(0x10);

    let mut source = Vec::new();
    source.extend_from_slice(b"\x0f\x0d\x03ref\x04");
    source.extend_from_slice(&0i64.to_le_bytes());
    source.push(0x10);

    let mut active = target;
    active.extend_from_slice(&source);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    let decoded = cadmpeg_asm::nurbs::core::surface_cache_resolving_refs(
        &ctx,
        &cadmpeg_asm::nurbs::toks::lex_test_span(
            &source,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .expect("valid single-record byte fixture"),
        &cadmpeg_asm::nurbs::toks::test_table(&active, cadmpeg_asm::kernel_header::RefWidth::Eight)
            .expect("valid single-record byte fixture"),
    )
    .transpose()
    .expect("resource allocation")
    .expect("subtype-table reference resolves to its surface cache");
    assert_eq!((decoded.u_count(), decoded.v_count()), (2, 2));
}

#[test]
fn a_form_two_par_int_cur_decodes_as_its_support_isoline() {
    use cadmpeg_asm::nurbs::proc_curve::decode_par_int_cur_isoline;
    use cadmpeg_ir::math::Point3;

    // The support is the unit bilinear patch scaled to millimetres, so the
    // isoline at u = 1 is the patch's far edge.
    let scope = generated_form_two_par_int_cur([1.0, 0.0], [1.0, 1.0]);
    let curve = decode_par_int_cur_isoline(&scope, cadmpeg_asm::kernel_header::RefWidth::Eight)
        .transpose()
        .expect("decode the support isoline")
        .expect("form-2 isoline");
    assert_eq!(curve.degree(), 1);
    assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        curve.control_points(),
        [Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)]
    );

    // A pcurve that crosses the support holds neither parameter fixed, so no
    // NURBS curve reproduces it and the form is refused.
    let diagonal = generated_form_two_par_int_cur([0.0, 0.0], [1.0, 1.0]);
    assert!(
        decode_par_int_cur_isoline(&diagonal, cadmpeg_asm::kernel_header::RefWidth::Eight)
            .is_none()
    );

    // A pcurve running only part of the support's domain would need a trim.
    let partial = generated_form_two_par_int_cur([1.0, 0.0], [1.0, 0.5]);
    assert!(
        decode_par_int_cur_isoline(&partial, cadmpeg_asm::kernel_header::RefWidth::Eight).is_none()
    );
}

#[test]
fn a_nested_construction_cache_is_not_the_enclosing_scope_cache() {
    use cadmpeg_asm::nurbs::core::{decode_curve_cache, decode_owned_curve_cache_at};

    // A `par_int_cur` whose cache slot is `nullbs` and whose support is an
    // intcurve construction carrying a curve block of its own.
    let mut scope = vec![0x0f];
    t_ident(&mut scope, "par_int_cur");
    scope.push(0x0f);
    t_ident(&mut scope, "exact_int_cur");
    scope.extend_from_slice(&generated_curve_block());
    scope.push(0x10);
    t_ident(&mut scope, "nullbs");
    scope.push(0x10);

    let width = cadmpeg_asm::kernel_header::RefWidth::Eight;
    let span = cadmpeg_asm::nurbs::subtypes::subtype_span(&scope, 0, width)
        .expect("the fixture is a balanced subtype scope");
    assert!(decode_curve_cache(&scope).is_some());
    assert!(decode_owned_curve_cache_at(span, width).is_none());
}

#[test]
fn a_nested_construction_does_not_claim_its_enclosing_record() {
    use cadmpeg_asm::nurbs::proc_surface::{
        procedural_surface_resolving_refs, DecodedProceduralSurfaceDefinition,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");

    let bytes = synthetic_cyl_spl_sur_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let record = &records[9];
    let owned = bytes[record.offset..record.offset + record.len].to_vec();
    let decoded = procedural_surface_resolving_refs(
        &ctx,
        &record.tokens,
        &cadmpeg_asm::nurbs::toks::SubtypeTable::from_records(std::slice::from_ref(record)),
    )
    .transpose()
    .expect("resolve the procedural surface references")
    .expect("the record owns its extrusion");
    assert!(matches!(
        decoded.definition(),
        DecodedProceduralSurfaceDefinition::Extrusion { .. }
    ));

    // The same extrusion nested inside a variable-blend scope is that blend's
    // support surface, not the record's own surface.
    let marker = b"\x0f\x0d\x0bcyl_spl_sur";
    let at = owned
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    let mut nested = owned.clone();
    nested.splice(at..at, *b"\x0f\x0d\x14srf_srf_v_bl_spl_sur");
    let terminator = nested.len() - 1;
    nested.insert(terminator, 0x10);
    let nested_records = cadmpeg_asm::test_support::sab::frame(
        &nested,
        0,
        nested.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    assert!(procedural_surface_resolving_refs(
        &ctx,
        &nested_records[0].tokens,
        &cadmpeg_asm::nurbs::toks::SubtypeTable::from_records(&nested_records),
    )
    .transpose()
    .expect("resolve the procedural surface references")
    .is_none());
}

mod rolling_ball;
