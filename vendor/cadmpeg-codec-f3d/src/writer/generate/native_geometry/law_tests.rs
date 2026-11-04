// SPDX-License-Identifier: Apache-2.0
//! Law-slot grammar at the source-less writer boundary.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::surface_payloads::SweepSurfacePayload;
use cadmpeg_ir::geometry::{
    LawExpression, LawFormula, LoftPathCurve, ProceduralSurfaceDefinition, SweepSurfaceLayout,
};

use crate::test_support::smbh_surfaces_test::{
    synthetic_law_driven_sweep_smbh, synthetic_revision_text_law_sweep_smbh,
};
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::{Point3, Vector3};

fn document(revision: bool) -> CadIr {
    let smbh = if revision {
        synthetic_revision_text_law_sweep_smbh()
    } else {
        synthetic_law_driven_sweep_smbh()
    };
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    ir.source = None;
    ir.set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    ir
}

fn layout(ir: &CadIr) -> SweepSurfaceLayout {
    let ProceduralSurfaceDefinition::Sweep(payload) = ir.model.procedural_surfaces[0].definition()
    else {
        panic!("sweep construction")
    };
    payload.native().as_ref().unwrap().layout.to_raw()
}

fn edit_layout(ir: &mut CadIr, edit: impl FnOnce(&mut SweepSurfaceLayout)) {
    let procedural = &mut ir.model.procedural_surfaces[0];
    let ProceduralSurfaceDefinition::Sweep(payload) = procedural.definition() else {
        panic!("sweep construction")
    };
    let mut native = payload
        .native()
        .as_deref()
        .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw)
        .unwrap();
    edit(&mut native.layout);
    let replacement = SweepSurfacePayload::try_new(
        payload.profile().clone(),
        payload.spine().clone(),
        Some(Box::new(native)),
    )
    .expect("the neutral construction admits the law values");
    procedural.edit_definition(|definition| {
        *definition = ProceduralSurfaceDefinition::Sweep(replacement);
    });
    let json = serde_json::to_value(&*ir).unwrap();
    let decoded: CadIr = serde_json::from_value(json).expect("document serde admission");
    assert_eq!(layout(&decoded), layout(ir));
}

fn text(value: &str) -> LawExpression {
    LawExpression::Text {
        value: cadmpeg_core::text::NonBlankString::new(value).unwrap(),
    }
}

fn assert_round_trip(ir: &CadIr) {
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("representable law construction writes");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("generated law construction decodes");
    assert_eq!(layout(decoded.ir()), layout(ir));
}

fn assert_refused(ir: &CadIr) {
    let sentinel = b"existing output".to_vec();
    let mut output = sentinel.clone();
    let result = F3dCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut output));
    assert!(
        result.is_err(),
        "unrepresentable law was written: {:?}",
        layout(ir)
    );
    assert_eq!(output, sentinel, "refusal must precede output mutation");
}

#[test]
fn sweep_string_laws_preserve_reserved_operator_text() {
    for revision in [false, true] {
        for value in [
            "null_law",
            "TRANS",
            "EDGE",
            "SPLINE_LAW",
            "COS",
            "arbitrary text",
        ] {
            let mut ir = document(revision);
            edit_layout(&mut ir, |layout| {
                let SweepSurfaceLayout::LawDriven {
                    first_law,
                    second_law,
                    ..
                } = layout
                else {
                    panic!("law-driven layout")
                };
                **first_law = text(value);
                **second_law = text(value);
            });
            assert_round_trip(&ir);
        }
    }
}

#[test]
fn formula_variables_refuse_text_including_nested_text() {
    for revision in [false, true] {
        for value in ["arbitrary text", "COS", "TRANS", "null_law"] {
            for nested in [false, true] {
                let mut ir = document(revision);
                let expression = if nested {
                    LawExpression::Algebraic {
                        operator: "COS".into(),
                        operands: vec![text(value)],
                    }
                } else {
                    text(value)
                };
                edit_layout(&mut ir, |layout| {
                    let SweepSurfaceLayout::LawDriven { formula, .. } = layout else {
                        panic!("law-driven layout")
                    };
                    *formula = LawFormula::Named {
                        name: cadmpeg_core::nonblank_literal!("variable"),
                        variables: vec![expression],
                    };
                });
                assert_refused(&ir);
            }
        }
    }
}

fn recursive_expressions(ir: &CadIr) -> Vec<LawExpression> {
    let ProceduralSurfaceDefinition::Sweep(payload) = ir.model.procedural_surfaces[0].definition()
    else {
        panic!("sweep construction")
    };
    vec![
        LawExpression::Null {},
        LawExpression::Transform {
            scalars: [1.0; 13],
            enums: [0; 3],
        },
        LawExpression::TransformVec {
            vectors: [Vector3::new(1.0, 0.0, 0.0); 4],
            scale: 1.0,
            flags: [false; 3],
        },
        LawExpression::Edge {
            curve: LoftPathCurve {
                id: payload.profile().clone(),
                endpoints: None,
            },
            parameters: [0.0, 1.0],
        },
        LawExpression::Spline {
            native_id: 3,
            knots: vec![0.0, 1.0],
            controls: vec![2.0, 3.0],
            point: Point3::new(0.0, 0.0, 0.0),
        },
        LawExpression::Algebraic {
            operator: "COS".into(),
            operands: vec![LawExpression::Double { value: 2.0 }],
        },
    ]
}

#[test]
fn sweep_slots_refuse_recursive_string_operators() {
    for revision in [false, true] {
        let source = document(revision);
        for expression in recursive_expressions(&source) {
            for first in [false, true] {
                let mut ir = source.clone();
                edit_layout(&mut ir, |layout| {
                    let SweepSurfaceLayout::LawDriven {
                        first_law,
                        second_law,
                        ..
                    } = layout
                    else {
                        panic!("law-driven layout")
                    };
                    if first {
                        **first_law = expression.clone();
                    } else {
                        **second_law = expression.clone();
                    }
                });
                assert_refused(&ir);
            }
        }
    }
}

#[test]
fn sweep_primitive_laws_follow_layout_discriminators() {
    for revision in [false, true] {
        for expression in [
            LawExpression::Integer { value: 2 },
            LawExpression::Double { value: 2.0 },
            LawExpression::Point {
                value: Point3::new(10.0, 20.0, 30.0),
            },
            LawExpression::Vector {
                value: Vector3::new(1.0, 2.0, 3.0),
            },
        ] {
            for first in [false, true] {
                let mut ir = document(revision);
                edit_layout(&mut ir, |layout| {
                    let SweepSurfaceLayout::LawDriven {
                        first_law,
                        second_law,
                        ..
                    } = layout
                    else {
                        panic!("law-driven layout")
                    };
                    if first {
                        **first_law = expression.clone();
                    } else {
                        **second_law = expression.clone();
                    }
                });
                if first && (revision || matches!(expression, LawExpression::Integer { .. })) {
                    assert_refused(&ir);
                } else {
                    assert_round_trip(&ir);
                }
            }
        }
    }
}
