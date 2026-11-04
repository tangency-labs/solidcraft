// SPDX-License-Identifier: Apache-2.0
//! Linear historical radius replacement and ordered coedge incidence.
use crate::history::historical_topology;
use cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CacheContract, ProceduralSurface,
    ProceduralSurfaceDefinition,
};
use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::topology::{Coedge, Loop, LoopBoundary, LoopRing, Sense};

#[test]
fn historical_radii_replace_each_carrier_once_and_keep_last_blend() {
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    let blend = |slot, radius| {
        let owner = SurfaceId::mint(format!("test:brep:surface#{slot}")).unwrap();
        let payload = BlendSurfacePayload::try_new(
            [None, None],
            None,
            BlendRadiusLaw::constant(radius).unwrap(),
            BlendCrossSection::Circular,
            CacheContract::legacy(),
        )
        .unwrap();
        (
            owner,
            ProceduralSurface::new(
                ProceduralSurfaceId::mint(format!("test:brep:blend#{slot}")).unwrap(),
                ProceduralSurfaceDefinition::Blend(payload),
                None,
            ),
        )
    };
    for slot in 0..1_000 {
        brep.procedural_surfaces.push(blend(slot, -2.0));
    }
    brep.procedural_surfaces.push(blend(0, 3.0));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let topology = historical_topology(&brep).unwrap().unwrap();
    assert_eq!(topology.surface_radii.len(), 1_000);
    assert_eq!(topology.surface_radii[0].surface, 0);
    assert_eq!(topology.surface_radii[0].radius, 3.0);
    assert!(topology.surface_radii[1..]
        .iter()
        .all(|radius| radius.radius == 2.0));
    ctx.finish_session().unwrap();
}

#[test]
fn historical_coedge_neighbors_follow_ring_order() {
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    let owner = LoopId::mint("test:brep:loop#1").unwrap();
    let ids: Vec<_> = (10..110)
        .map(|slot| CoedgeId::mint(format!("test:brep:coedge#{slot}")).unwrap())
        .collect();
    brep.loops.push(Loop {
        id: owner.clone(),
        face: FaceId::mint("test:brep:face#2").unwrap(),
        boundary: LoopBoundary::Ring(LoopRing::new(ids.clone(), Vec::new()).unwrap()),
    });
    for id in &ids {
        brep.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: owner.clone(),
            edge: EdgeId::mint("test:brep:edge#3").unwrap(),
            radial_next: id.clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    let topology = historical_topology(&brep).unwrap().unwrap();
    assert_eq!(topology.coedge_topology.len(), 100);
    for (ordinal, coedge) in topology.coedge_topology.iter().enumerate() {
        let ordinal = i64::try_from(ordinal).unwrap();
        assert_eq!(coedge.next, 10 + (ordinal + 1) % 100);
        assert_eq!(coedge.previous, 10 + (ordinal + 99) % 100);
    }
}

#[test]
fn appended_tolerant_coedge_curves_join_the_curve_family_in_key_order() {
    use cadmpeg_ir::geometry::analytic::LineCurve;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    for (id, x) in [
        ("test:brep:entity#9", 9.0),
        ("test:brep:entity#20", 20.0),
        ("test:brep:tolerant-coedge-curve#5", 5.0),
    ] {
        brep.curves.push(Curve {
            parameter_range: None,
            id: CurveId::mint(id).unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(x, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            )),
            source_object: None,
        });
    }
    let topology = historical_topology(&brep).unwrap().unwrap();
    assert_eq!(topology.curves, [5, 9, 20]);
    assert_eq!(
        topology
            .curve_axes
            .iter()
            .map(|axis| (axis.curve, axis.origin.x))
            .collect::<Vec<_>>(),
        [(5, 5.0), (9, 9.0), (20, 20.0)]
    );
}
