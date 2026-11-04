// SPDX-License-Identifier: Apache-2.0

use super::decode;
use crate::test_support::procedural_support::revolution_intersection;
use crate::test_support::zip_test::f3d_with_smbh;
use cadmpeg_ir::geometry::ProceduralCurveDefinition;

#[test]
fn revolution_support_keeps_native_chart_without_false_endpoint_errors() {
    for name in ["rot_spl_sur", "rotsur"] {
        for referenced in [false, true] {
            for side in 0..2 {
                let result = decode(f3d_with_smbh(&revolution_intersection(
                    name, referenced, side, true,
                )));
                let construction = result
                    .ir()
                    .model
                    .procedural_curves
                    .iter()
                    .find(|curve| {
                        matches!(
                            curve.definition(),
                            ProceduralCurveDefinition::Intersection { .. }
                        )
                    })
                    .expect("intersection construction");
                let ProceduralCurveDefinition::Intersection { context, .. } =
                    construction.definition()
                else {
                    panic!("intersection construction");
                };
                assert!(context.sides()[side].surface.is_none());
                assert!(context.sides()[side].pcurve.is_some());
                assert!(result.report().losses.iter().any(|loss| {
                    loss.code.local_code() == "geometry.procedural-support-chart-unresolved"
                }));
                let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
                assert!(
                    !validation
                        .findings
                        .iter()
                        .any(|finding| { finding.message.contains("endpoint distance contract") }),
                    "{:#?}",
                    validation.findings
                );
            }
        }
    }
}

#[test]
fn revolution_support_keeps_native_chart_without_pcurve() {
    let result = decode(f3d_with_smbh(&revolution_intersection(
        "rot_spl_sur",
        false,
        0,
        false,
    )));
    let construction = result
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| {
            matches!(
                curve.definition(),
                ProceduralCurveDefinition::Intersection { .. }
            )
        })
        .unwrap();
    let ProceduralCurveDefinition::Intersection { context, .. } = construction.definition() else {
        panic!("intersection construction");
    };
    assert!(context.sides()[0].surface.is_some());
    assert!(context.sides()[0].pcurve.is_none());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code.local_code() == "geometry.procedural-support-chart-unresolved" }));
}
