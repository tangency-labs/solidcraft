// SPDX-License-Identifier: Apache-2.0

use super::admit;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve, Curve, CurveGeometry, LegacyCache, ProceduralCurve,
    ProceduralCurveDefinition, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Edge, EdgeCarrier, Point, Vertex};
use cadmpeg_ir::CadIr;

fn exact_curve(offset: f64) -> CadIr {
    let mut ir = CadIr::empty();
    for (slot, x) in [(0, -100.0), (1, 0.0), (2, 100.0)] {
        let point = PointId::mint(format!("test:brep:point#{slot}")).unwrap();
        ir.model.points.push(Point::new(
            point.clone(),
            FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap(),
            None,
        ));
        ir.model.vertices.push(Vertex {
            id: VertexId::mint(format!("test:brep:vertex#{slot}")).unwrap(),
            point,
            tolerance: None,
        });
    }
    for (slot, start, end) in [
        (
            0,
            Point3::new(-100.0, 0.0, 0.0),
            Point3::new(0.0, offset, 0.0),
        ),
        (
            1,
            Point3::new(0.0, -offset, 0.0),
            Point3::new(100.0, 0.0, 0.0),
        ),
    ] {
        let curve = CurveId::mint(format!("test:brep:curve#{slot}")).unwrap();
        let construction =
            ProceduralCurveId::mint(format!("test:brep:construction#{slot}")).unwrap();
        let poles = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0].map(|t| {
            Point3::new(
                start.x + t * (end.x - start.x),
                start.y + t * (end.y - start.y),
                0.0,
            )
        });
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    3,
                    vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    poles.to_vec(),
                    None,
                    false,
                )
                .unwrap(),
            )),
            parameter_range: None,
            source_object: None,
        });
        ir.model
            .add_procedural_curves(vec![(
                curve.clone(),
                ProceduralCurve::new(
                    construction,
                    ProceduralCurveDefinition::Exact {
                        cache: Some(LegacyCache::try_new(0.0).unwrap()),
                    },
                ),
            )])
            .unwrap()
            .unwrap();
        ir.model.edges.push(Edge {
            id: EdgeId::mint(format!("test:brep:edge#{slot}")).unwrap(),
            carrier: EdgeCarrier::new(Some(curve), Some([0.0, 1.0])).unwrap(),
            start: ir.model.vertices[slot].id.clone(),
            end: ir.model.vertices[slot + 1].id.clone(),
            tolerance: None,
        });
    }
    let body = cadmpeg_ir::ids::BodyId::mint("test:brep:body#0").unwrap();
    let region = cadmpeg_ir::ids::RegionId::mint("test:brep:region#0").unwrap();
    let shell = cadmpeg_ir::ids::ShellId::mint("test:brep:shell#0").unwrap();
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        id: body.clone(),
        kind: cadmpeg_ir::topology::BodyKind::Wire,
        regions: vec![region.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(cadmpeg_ir::topology::Region {
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    ir.model.shells.push(
        cadmpeg_ir::topology::Shell::new(
            shell,
            region,
            Vec::new(),
            ir.model.edges.iter().map(|edge| edge.id.clone()).collect(),
            Vec::new(),
        )
        .unwrap(),
    );
    ir
}

fn geometry_is_valid(ctx: &cadmpeg_core::decode::DecodeContext<'_>, ir: &CadIr) -> bool {
    cadmpeg_ir::validate::admit::admit(
        ctx,
        ir,
        &[cadmpeg_ir::report::check::Check::GeometricConsistency],
        Vec::new(),
    )
    .unwrap()
    .is_ok()
}

#[test]
fn straddling_exact_cubic_endpoints_widen_the_shared_vertex_tolerance() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = exact_curve(0.5);
        let mut expected = ir.clone();
        expected.model.vertices[1].tolerance = cadmpeg_ir::scalar::PositiveReal::new(0.5);
        let mut losses = Vec::new();
        assert!(!geometry_is_valid(ctx, &ir));
        let mut annotations = vec![cadmpeg_asm::brep::annotations::AnnotationRecord {
            id: ir.model.vertices[1].id.as_str().to_owned(),
            stream: "synthetic".into(),
            offset: 0,
            tag: cadmpeg_asm::brep::annotations::AnnotationTag::Record("vertex".into()),
            derived_fields: Vec::new(),
        }];
        admit(ctx, &mut ir, &mut losses, &mut annotations).unwrap();
        assert_eq!(annotations[0].derived_fields, ["tolerance"]);
        assert_eq!(ir, expected);
        assert!(geometry_is_valid(ctx, &ir));
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "geometry.vertex-tolerance-widened"
        );
        assert!(losses[0].message.contains("1 vertex tolerance(s)"));
        assert!(losses[0].message.contains("0.50000000000000000 mm"));
        let admitted = ir.clone();
        let mut repeated_losses = Vec::new();
        admit(ctx, &mut ir, &mut repeated_losses, &mut []).unwrap();
        assert_eq!(ir, admitted);
        assert!(repeated_losses.is_empty());
    });
}

#[test]
fn endpoint_miss_above_body_fraction_still_fails_validation() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = exact_curve(2.01);
        let expected = ir.clone();
        let mut losses = Vec::new();
        admit(ctx, &mut ir, &mut losses, &mut []).unwrap();
        assert_eq!(ir, expected);
        assert!(losses.is_empty());
        assert!(!geometry_is_valid(ctx, &ir));
    });
}

#[test]
fn matching_cache_and_explicit_endpoint_tolerance_are_preserved() {
    crate::test_support::with_decode_context(|ctx| {
        for offset in [0.0, 0.5] {
            let mut ir = exact_curve(offset);
            if offset != 0.0 {
                ir.model.vertices[1].tolerance = cadmpeg_ir::scalar::PositiveReal::new(0.75);
            }
            let expected = ir.clone();
            let mut losses = Vec::new();
            admit(ctx, &mut ir, &mut losses, &mut []).unwrap();
            assert_eq!(ir, expected);
            assert!(losses.is_empty());
            assert!(geometry_is_valid(ctx, &ir));
        }
    });
}

#[test]
fn an_unevaluable_incident_endpoint_blocks_widening() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = exact_curve(0.5);
        ir.model.edges[1].carrier = EdgeCarrier::new(None, None).unwrap();
        let expected = ir.clone();
        let mut losses = Vec::new();
        admit(ctx, &mut ir, &mut losses, &mut []).unwrap();
        assert_eq!(ir, expected);
        assert!(losses.is_empty());
        assert!(!geometry_is_valid(ctx, &ir));
    });
}

#[test]
fn unresolved_spring_support_keeps_the_solved_cache_and_support_surface() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = exact_curve(0.0);
        ir.model.surfaces.push(
            serde_json::from_value(serde_json::json!({
                "id": "test:brep:surface#1",
                "geometry": {"kind": "plane", "origin": {"x":0.0,"y":0.0,"z":0.0},
                    "normal": {"x":0.0,"y":0.0,"z":1.0}, "u_axis":{"x":1.0,"y":0.0,"z":0.0}}
            }))
            .unwrap(),
        );
        let definition = serde_json::from_value(serde_json::json!({
            "kind":"spring", "direction":0,
            "layout":{"kind":"cache_first",
                "context":{"sides":[{"surface":"test:brep:surface#1",
                    "pcurve":{"geometry":{"kind":"nurbs", "degree":1,
                        "knots":[0.0,0.0,1.0,1.0], "periodic":false,
                        "poles":{"form":"polynomial","points":[{"u":4.0,"v":4.0},{"u":6.0,"v":4.0}]}}}},{}],
                    "parameter_range":[0.0,1.0], "discontinuities":[[],[],[]]},
                "form":{"revision":23000,"cache":{"kind":"solved_cache","fit_tolerance":0.0},
                    "support_bounds":[[null,null,null,null],[null,null,null,null]],
                    "solved_range":[null,null],"extension":0}}
        })).unwrap();
        ir.model.procedural_curves[0].replace_definition(definition);
        let curves = ir.model.curves.clone();
        let surfaces = ir.model.surfaces.clone();
        let mut losses = Vec::new();
        admit(ctx, &mut ir, &mut losses, &mut []).unwrap();
        assert_eq!(ir.model.curves, curves);
        assert_eq!(ir.model.surfaces, surfaces);
        let ProceduralCurveDefinition::Spring(spring) = ir.model.procedural_curves[0].definition()
        else {
            panic!("spring construction");
        };
        assert!(spring.support_context().sides()[0].pcurve.is_none());
        assert!(spring.support_context().sides()[0].surface.is_some());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "geometry.spring-support-pcurve-unresolved"
        );
    });
}

#[test]
fn an_unowned_vertex_has_no_body_bound() {
    crate::test_support::with_decode_context(|ctx| {
        let mut ir = exact_curve(0.5);
        ir.model.bodies.clear();
        ir.model.regions.clear();
        ir.model.shells.clear();
        let expected = ir.clone();
        let mut losses = Vec::new();
        admit(ctx, &mut ir, &mut losses, &mut []).unwrap();
        assert_eq!(ir, expected);
        assert!(losses.is_empty());
        assert!(!geometry_is_valid(ctx, &ir));
    });
}

#[test]
fn conic_extrusion_unbound_support_has_one_loss_per_native_pcurve() {
    use cadmpeg_ir::geometry::{
        pcurve::{PcurveGeometry, PcurveNurbs},
        IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, ProceduralCurveDefinition,
        SupportPcurve,
    };
    use cadmpeg_ir::ids::{ProceduralCurveId, SurfaceId};
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::CadIr;

    let pcurve = SupportPcurve::from(PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
        .unwrap(),
    });
    for surface_present in [false, true] {
        for pcurve_present in [false, true] {
            let context = IntcurveSupportContext::try_new(
                [
                    IntcurveSupportSide {
                        surface: surface_present
                            .then(|| SurfaceId::mint("test:model:surface#0").unwrap()),
                        pcurve: pcurve_present.then(|| pcurve.clone()),
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                    },
                ],
                [0.0, 1.0],
                std::array::from_fn(|_| Vec::new()),
            )
            .unwrap();
            let mut ir = CadIr::empty();
            ir.model.procedural_curves.push(ProceduralCurve::new(
                ProceduralCurveId::mint("test:model:construction#0").unwrap(),
                ProceduralCurveDefinition::Intersection {
                    context,
                    discontinuity_flag: false,
                    cache: None,
                },
            ));
            let mut losses = Vec::new();
            let ctx = cadmpeg_test_support::service_decode_context();
            admit(&ctx, &mut ir, &mut losses, &mut []).unwrap();
            assert_eq!(
                losses.len(),
                usize::from(pcurve_present && !surface_present)
            );
            if let Some(loss) = losses.first() {
                assert_eq!(
                    loss.code,
                    crate::loss::F3dLossCode::ProceduralSupportChartUnresolved.kind()
                );
                assert!(loss.message.contains("support side 0"));
            }
        }
    }
}
