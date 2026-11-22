//! Actual-object lifetime controls for the production fillet session.

use solidcore::blend::fillet_corner::p3::FilletCornerKind;
use solidcore::blend::fillet_executive::p3::VertexEntry;
use solidcore::blend::fillet_executive::session::{FilletBody, FilletObject, FilletSession};
use solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator;
use solidcore::geom::attribute::ids;
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::edge;
use solidcore::{EntityRef, Kernel};

#[test]
fn actual_box_solver_stays_authoritative_through_corner_creation() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let edgeuse = edge::blend_edgeuse(&k, &mut target, edge).unwrap().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    let generator = session
        .add_surface_generator(FilletSurfaceGenerator::circular(false, 0.05, false, false))
        .unwrap();
    let solver = session
        .new_constant_radius_solver(&k, edgeuse, 0.5, 1e-4, 20.0 * PI / 180.0, 10.0 * PI / 180.0)
        .unwrap();
    session.set_solver_generator(solver, generator).unwrap();
    let actual_base = std::ptr::from_ref(session.solver(solver).unwrap().base());
    assert!(session
        .solver(solver)
        .unwrap()
        .base()
        .surface(0)
        .unwrap()
        .is_some());
    assert!(session
        .solver(solver)
        .unwrap()
        .base()
        .surface(1)
        .unwrap()
        .is_some());
    session.load_solver(&k, solver).unwrap();
    assert_eq!(
        session.executive().solvers,
        vec![FilletObject::Solver(solver)]
    );
    assert!(session
        .solver(solver)
        .unwrap()
        .base()
        .fillet_executive()
        .is_some());
    assert_eq!(session.executive().filleted_vertices.len(), 2);
    session.create_corners(&k, None).unwrap();
    assert_eq!(session.executive().corners.len(), 2);
    assert!(std::ptr::eq(
        actual_base,
        session.solver(solver).unwrap().base()
    ));
    for vertex in &session.executive().filleted_vertices {
        let Some(FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(expected))) = vertex
        else {
            panic!("actual target vertex required");
        };
        let Some(VertexEntry::Corner(FilletObject::Corner(id))) =
            session.executive().vertex_corner_map.get(vertex)
        else {
            panic!("actual retained corner required");
        };
        assert_eq!(session.corner(*id).unwrap().vertex, Some(*expected));
        assert_eq!(session.corner(*id).unwrap().kind, FilletCornerKind::NByOne);
        session.target().vertex(*expected).unwrap();
    }
    let key = session.target_identity(EntityRef::Edge(edge));
    assert!(session
        .attributes()
        .find_attribute(key.key, ids::MASS_PROPERTIES)
        .is_none());
    assert_ne!(
        FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)),
        FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(edge))
    );
}

#[test]
fn native_api_setup_uses_owned_parameter_algorithm_and_unknown_xsect_default() {
    use solidcore::blend::fillet_executive::session::SolverObject;
    use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;

    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let tolerance = edge::tolerance(&target, edge).unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 99, 1, 1.0)
        .unwrap();
    assert!(session.executive().initialized);
    // The native SetFilletParameters reset follows the API's earlier TRUE write.
    assert!(!session.executive().options.global_merge);
    assert_eq!(session.executive().surface_generators.len(), 1);
    let [FilletObject::Solver(id)] = session.executive().solvers.as_slice() else {
        panic!("one actual solver required");
    };
    let SolverObject::ConstantRadius(solver) = session.solver(*id).unwrap() else {
        panic!("actual constant-radius constructor required");
    };
    assert_eq!(solver.fillet_radii, [0.5, 0.5]);
    assert_eq!(solver.solver.approx_tol_3d, 4.0 * tolerance);
    assert_eq!(
        solver.solver.fsg.unwrap().fillet_surface_generator_type(),
        FilletSurfaceGeneratorType::Circular
    );
    session.create_corners(&k, None).unwrap();
    assert_eq!(session.executive().corners.len(), 2);
}

#[test]
fn real_box_nx1_topology_uses_live_pseudo_objects() {
    use solidcore::blend::fillet_geom::p1::{FilletEdgeType, FilletVertexType};
    use solidcore::intersect::point_class::PointClassType;

    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    let FilletObject::Corner(id) = session.executive().corners[0] else {
        panic!("actual corner required");
    };
    session.make_nx1_corner_topology(&k, id).unwrap();
    let corner = session.corner(id).unwrap();
    assert_eq!(corner.vertices.len(), 2);
    assert_eq!(corner.edges.len(), 1);
    for &vertex in &corner.vertices {
        let vertex = session.pseudo_brep().fillet_vertex(vertex).unwrap();
        assert_eq!(vertex.fillet_corner(), Some(id));
        assert_eq!(vertex.fillet_vertex_type(), FilletVertexType::RailXEdgeuse);
        assert_eq!(vertex.point_class(), PointClassType::Edgeuse);
        let Some(EntityRef::Edgeuse(original)) = vertex.point_class_object() else {
            panic!("actual classified target edge use required");
        };
        session.target().edgeuse(original).unwrap();
    }
    let rib = session.pseudo_brep().fillet_edge(corner.edges[0]).unwrap();
    assert_eq!(rib.fillet_edge_type(), FilletEdgeType::FilletXSideFace);
    session.target().face(rib.original_face().unwrap()).unwrap();
}

#[test]
fn real_box_corner_vertex_stage_retains_native_geometry_and_rail_start() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    session.create_corner_vertices(&mut k).unwrap();
    let mut points = Vec::new();
    for &object in &session.executive().corners {
        let FilletObject::Corner(id) = object else {
            panic!("actual corner required");
        };
        let corner = session.corner(id).unwrap();
        assert!(!corner.is_calc_as_const_rad());
        for &vertex in &corner.vertices {
            assert!(session
                .pseudo_brep()
                .fillet_vertex(vertex)
                .unwrap()
                .is_processed());
            points.push(session.pseudo_brep().vertex_point(vertex).unwrap());
        }
    }
    // Direct native box-edge component trace, in actual corner/FV order.
    assert_eq!(
        points,
        [
            Point3::new(9.5, 0.0, 30.0),
            Point3::new(10.0, 0.0, 29.5),
            Point3::new(10.0, 20.0, 29.5),
            Point3::new(9.5, 20.0, 30.0),
        ]
    );
    let rail = session.pseudo_brep().edge_list()[0];
    assert_eq!(
        session
            .pseudo_brep()
            .fillet_edge(rail)
            .unwrap()
            .fillet_edge_type(),
        solidcore::blend::fillet_geom::p1::FilletEdgeType::Rail
    );
    let first_use = session.pseudo_brep().brep().edge(rail).unwrap().edgeuses[0];
    let FilletObject::Corner(first_corner) = session.executive().corners[0] else {
        panic!("actual first corner required");
    };
    let first_vertex = session.corner(first_corner).unwrap().vertices[0];
    let vertexuse = session
        .pseudo_brep()
        .brep()
        .edgeuse(first_use)
        .unwrap()
        .vertexuse
        .unwrap();
    assert_eq!(
        session
            .pseudo_brep()
            .brep()
            .vertexuse(vertexuse)
            .unwrap()
            .vertex,
        Some(first_vertex)
    );
}

#[test]
fn real_box_post_vertex_adjustment_preserves_retained_corner_topology() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    session.create_corner_vertices(&mut k).unwrap();
    let corners = session.executive().corners.clone();
    let edges = session.pseudo_brep().edge_list().to_vec();
    let mut points = Vec::new();
    for &object in &corners {
        let FilletObject::Corner(id) = object else {
            panic!("actual corner required");
        };
        for &vertex in &session.corner(id).unwrap().vertices {
            points.push((vertex, session.pseudo_brep().vertex_point(vertex).unwrap()));
        }
    }
    session.adjust_corner_topology(&mut k).unwrap();
    assert_eq!(session.executive().corners, corners);
    assert_eq!(session.pseudo_brep().edge_list(), edges);
    for (vertex, point) in points {
        assert_eq!(session.pseudo_brep().vertex_point(vertex).unwrap(), point);
        assert!(session
            .pseudo_brep()
            .fillet_vertex(vertex)
            .unwrap()
            .is_processed());
    }
}

#[test]
fn actual_box_solver_builds_surface_and_rails_in_retained_geometry() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    session.create_corner_vertices(&mut k).unwrap();
    session.adjust_corner_topology(&mut k).unwrap();
    let FilletObject::Solver(id) = session.executive().solvers[0] else {
        panic!("actual solver");
    };
    let actual_base = std::ptr::from_ref(session.solver(id).unwrap().base());
    let geometries = session.solver(id).unwrap().base().fillet_geoms.clone();
    assert!(geometries.iter().all(|&geometry| session
        .pseudo_brep()
        .geom(geometry)
        .unwrap()
        .fillet_surface
        .is_none()));
    session.calc_solver_geometry(&k, id).unwrap();
    let solver = session.solver(id).unwrap().base();
    assert_eq!(std::ptr::from_ref(solver), actual_base);
    assert_eq!(solver.fillet_geoms, geometries);
    assert!(solver.extend_before && solver.extend_after);
    for geometry in geometries {
        let geometry = session.pseudo_brep().geom(geometry).unwrap();
        assert_eq!(geometry.solver, Some(id));
        assert!(geometry.fillet_surface.is_some());
        let center = geometry.center_line_geometry().unwrap();
        let solidcore::curve::curve::CurveKind::Line(line) = center.kind() else {
            panic!("actual native analytic centerline must remain a line");
        };
        assert!(std::ptr::eq(
            geometry.center_line_curve().unwrap(),
            line.base()
        ));
        for rail in geometry.rails {
            let rail = rail.unwrap();
            assert!(edge::curve(session.pseudo_brep().brep(), rail)
                .unwrap()
                .is_some());
        }
    }
}

#[test]
fn actual_box_all_solver_stage_builds_retained_fillet_surface() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    session.create_corner_vertices(&mut k).unwrap();
    session.adjust_corner_topology(&mut k).unwrap();
    session.calculate_solver_geometry(&mut k).unwrap();
    assert!(!session.executive().solvers.is_empty());
    for &object in &session.executive().solvers {
        let FilletObject::Solver(id) = object else {
            panic!("actual solver");
        };
        let solver = session.solver(id).unwrap().base();
        assert!(!solver.fillet_geoms.is_empty());
        for &geometry in &solver.fillet_geoms {
            let geometry = session.pseudo_brep().geom(geometry).unwrap();
            assert_eq!(geometry.solver, Some(id));
            assert!(geometry.fillet_surface.is_some());
        }
    }
}

#[test]
fn smooth_prelude_requires_live_tangent_context_for_multiple_solvers() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edges: Vec<_> = target.edges().ids().take(2).collect();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &edges, 0.5, 1, 1, 1.0)
        .unwrap();
    assert_eq!(session.executive().solvers.len(), 2);
    let solvers = session.executive().solvers.clone();
    let error = session.process_smooth_edge_sequences(&mut k).unwrap_err();
    assert!(matches!(error, solidcore::SolidError::InvalidInput { .. }));
    assert_eq!(session.executive().solvers, solvers);
    session.create_corners(&k, None).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    assert_eq!(session.executive().solvers, solvers);
}

#[test]
fn actual_target_extension_map_retains_native_containing_surface() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let face = target.faces().ids().next().unwrap();
    let surface = target.face(face).unwrap().surface.as_ref().unwrap().clone();
    let domain = surface.natural_uv_domain_if_built();
    let duplicate = surface.clone();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    assert!(session.extended_surface(face).is_none());
    session
        .add_extended_original_surface(face, surface)
        .unwrap();
    let retained = std::ptr::from_ref(session.extended_surface(face).unwrap());
    session
        .add_extended_original_surface(face, duplicate)
        .unwrap();
    let actual = session.extended_surface(face).unwrap();
    assert_eq!(std::ptr::from_ref(actual), retained);
    assert_eq!(actual.natural_uv_domain_if_built(), domain);
}

#[test]
fn actual_box_corner_edges_are_processed_in_retained_corner_order() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    session.create_corner_vertices(&mut k).unwrap();
    session.adjust_corner_topology(&mut k).unwrap();
    session.calculate_solver_geometry(&mut k).unwrap();
    session.calculate_corner_edges(&mut k).unwrap();
    assert!(!session.executive().corners.is_empty());
    for &object in &session.executive().corners {
        let FilletObject::Corner(id) = object else {
            panic!("actual corner");
        };
        let corner = session.corner(id).unwrap();
        assert!(!corner.edges.is_empty());
        for &edge_id in &corner.edges {
            let edge_record = session.pseudo_brep().brep().edge(edge_id).unwrap();
            let curve = edge_record.curve.as_ref().unwrap();
            assert_eq!(curve.type_code(), solidcore::curve::curve::p2::CIRCLE_TYPE);
            assert_eq!(edge_record.interval, curve.natural_interval());
            let fillet_edge = session.pseudo_brep().fillet_edge(edge_id).unwrap();
            assert_eq!(
                fillet_edge.status,
                solidcore::blend::fillet_geom::p1::FilStatus::Processed
            );
            assert_eq!(fillet_edge.fillet_corner(), Some(id));
        }
    }
}

#[test]
fn actual_box_rail_trim_preserves_live_rails_and_classification_option() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    session.create_corner_vertices(&mut k).unwrap();
    session.adjust_corner_topology(&mut k).unwrap();
    session.calculate_solver_geometry(&mut k).unwrap();
    session.calculate_corner_edges(&mut k).unwrap();
    assert!(session.executive().options.classification);
    let rails: Vec<_> = session
        .pseudo_brep()
        .geoms()
        .iter()
        .flat_map(|geometry| geometry.rails.into_iter().flatten())
        .collect();
    assert!(!rails.is_empty());
    session.trim_solver_rails(&mut k).unwrap();
    assert!(session.executive().options.classification);
    for rail in rails {
        let edge = session.pseudo_brep().brep().edge(rail).unwrap();
        let curve = edge.curve.as_ref().unwrap();
        let interval = curve.natural_interval();
        assert!(interval.min() <= interval.max());
        assert!(matches!(
            curve.kind(),
            solidcore::curve::curve::CurveKind::Line(_)
        ));
    }
}

#[test]
fn actual_box_rollover_keeps_scoped_attached_rail_classifications() {
    use solidcore::blend::fillet_classification::FilletClassObject;
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    session.create_corner_vertices(&mut k).unwrap();
    session.adjust_corner_topology(&mut k).unwrap();
    session.calculate_solver_geometry(&mut k).unwrap();
    session.calculate_corner_edges(&mut k).unwrap();
    session.trim_solver_rails(&mut k).unwrap();
    assert!(session.executive().options.classification);
    let adjusted = session.classify_solver_rails_with_adjusted(&mut k).unwrap();
    assert!(adjusted.is_empty());
    let rails: Vec<_> = session
        .pseudo_brep()
        .geoms()
        .iter()
        .flat_map(|geom| geom.rails.into_iter().flatten())
        .collect();
    assert!(!rails.is_empty());
    for rail in rails {
        let record = session.pseudo_brep().fillet_edge(rail).unwrap();
        let cc = record.curve_class().unwrap();
        assert_eq!(
            cc.curve(),
            Some(FilletClassObject::Curve(FilletBody::Pseudo, rail))
        );
        assert_eq!(
            cc.face(),
            record
                .original_face()
                .map(|face| FilletClassObject::Topology(FilletBody::Target, EntityRef::Face(face)))
        );
    }
}

#[test]
fn actual_box_face_stage_constructs_a_distinct_intermediate_body() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    assert!(session.intermediate_brep().is_none());
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    session.process_smooth_edge_sequences(&mut k).unwrap();
    session.create_corner_vertices(&mut k).unwrap();
    session.adjust_corner_topology(&mut k).unwrap();
    session.calculate_solver_geometry(&mut k).unwrap();
    session.calculate_corner_edges(&mut k).unwrap();
    session.trim_solver_rails(&mut k).unwrap();
    session.classify_solver_rails(&mut k).unwrap();
    session.build_fillet_faces(&mut k).unwrap();
    let body = session.intermediate_brep().unwrap();
    assert!(body.editing_enabled());
    assert_eq!(
        body.key(),
        solidcore::topology::brep::p3::topology_ref(
            body.notify_serial().unwrap(),
            EntityRef::Brep(solidcore::topology::BrepId::from_index(0)),
        )
        .key
    );
    assert_ne!(body.key(), solidcore::geom::attribute::ObjectKey(0));
    assert!(!body.faces().is_empty());
    assert_ne!(body.notify_serial(), session.target().notify_serial());
    assert_eq!(
        session.executive().fillet,
        Some(FilletObject::IntermediateBrep)
    );
    for (index, geometry) in session.pseudo_brep().geoms().iter().enumerate() {
        let id = solidcore::blend::fillet_geom::p1::FilletGeomId(u32::try_from(index).unwrap());
        let source = session.fillet_surface(id).unwrap().unwrap();
        let face = body
            .faces()
            .ids()
            .find(|&face| {
                body.face(face)
                    .unwrap()
                    .surface
                    .as_ref()
                    .is_some_and(|managed| core::ptr::eq(source, managed))
            })
            .expect("native source surface must be the actual managed face surface");
        assert_eq!(
            source.owner(),
            Some(
                session
                    .intermediate_identity(EntityRef::Face(face))
                    .unwrap()
            )
        );
        // `GetFilletSurface()` after `MakeFaceBrep`: the face's surface in the owning
        // intermediate body, refused without it or with another body.
        let owner = solidcore::blend::fillet_geom::p1::FilletSurfaceOwner {
            identity: session
                .intermediate_identity(EntityRef::Brep(solidcore::topology::BrepId::from_index(0)))
                .unwrap(),
            body,
        };
        let resolved = geometry.fillet_surface(Some(owner)).unwrap().unwrap();
        assert!(core::ptr::eq(source, resolved));
        assert!(core::ptr::eq(
            resolved,
            body.face(face).unwrap().surface.as_ref().unwrap()
        ));
        assert!(matches!(
            geometry.fillet_surface(None).unwrap_err(),
            solidcore::SolidError::InvalidInput { .. }
        ));
        let foreign = solidcore::blend::fillet_geom::p1::FilletSurfaceOwner {
            identity: session
                .intermediate_identity(EntityRef::Face(face))
                .unwrap(),
            body,
        };
        assert!(matches!(
            geometry.fillet_surface(Some(foreign)).unwrap_err(),
            solidcore::SolidError::InvalidInput { .. }
        ));
    }
    let edge_keys: Vec<_> = solidcore::topology::brep::p1::edges(body, None)
        .unwrap()
        .into_iter()
        .map(|edge| {
            session
                .intermediate_identity(EntityRef::Edge(edge))
                .unwrap()
                .key
        })
        .collect();
    let Some(FilletObject::Attribute(marker)) = session.executive().edge_attribute else {
        panic!("actual executive edge marker must be retained");
    };
    assert!(session.attributes().users(marker).unwrap().is_empty());
    let outcome = session.stitch_fillet_faces(&mut k).unwrap();
    assert!(outcome.status.is_none());
    // Independent native box-edge baseline: no glues, four lamina edges.
    assert_eq!((outcome.stitched, outcome.lamina), (0, 4));
    assert_eq!((outcome.max_vertex_gap, outcome.max_edge_gap), (0.0, 0.0));
    assert_eq!(session.attributes().users(marker).unwrap(), edge_keys);
    let geom = solidcore::blend::fillet_geom::p1::FilletGeomId(0);
    let before = session
        .fillet_surface(geom)
        .unwrap()
        .unwrap()
        .natural_uv_domain_if_built();
    assert_eq!((before.min().x, before.max().x), (0.0, 1.0));
    let context = k.context.doing_boolean();
    session.shrink_fillet_faces(&mut k).unwrap();
    assert_eq!(k.context.doing_boolean(), context);
    let after = session
        .fillet_surface(geom)
        .unwrap()
        .unwrap()
        .natural_uv_domain_if_built();
    // Independent native direct-component baseline; default trim options.
    assert_eq!((after.min().x, after.max().x), (0.25, 0.75));
    assert_eq!((after.min().y, after.max().y), (0.0, 1.0));
}

#[test]
fn actual_box_shared_constructor_builds_and_stitches_native_intermediate_body() {
    let mut k = Kernel::default();
    let mut target = create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0).unwrap();
    let edge = target.edges().ids().next().unwrap();
    let mut session = FilletSession::new(&mut k, &mut target).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[edge], 0.5, 1, 1, 1.0)
        .unwrap();
    session.create_corners(&k, None).unwrap();
    let mut error_info = solidcore::blend::fillet_executive::p3::ErrorInfo {
        code: 1,
        ..Default::default()
    };
    session
        .create_fillet_brep(&mut k, Some(&mut error_info))
        .unwrap();
    let body = session.intermediate_brep().unwrap();
    // Independently measured native intermediate-body construction endpoint.
    assert_eq!(
        (
            body.vertices().len(),
            body.edges().len(),
            body.faces().len()
        ),
        (4, 4, 1)
    );
    let geometry = solidcore::blend::fillet_geom::p1::FilletGeomId(0);
    let surface = session.fillet_surface(geometry).unwrap().unwrap();
    let face = body.faces().ids().next().unwrap();
    assert!(core::ptr::eq(
        surface,
        body.face(face).unwrap().surface.as_ref().unwrap()
    ));
    assert_eq!(
        surface.owner(),
        Some(
            session
                .intermediate_identity(EntityRef::Face(face))
                .unwrap()
        )
    );
    let domain = surface.natural_uv_domain_if_built();
    assert_eq!((domain.min().x, domain.max().x), (0.0, 1.0));
    let Some(FilletObject::Attribute(marker)) = session.executive().edge_attribute else {
        panic!("constructor must retain actual executive edge attribute");
    };
    assert_eq!(session.attributes().users(marker).unwrap().len(), 4);
    assert_eq!(error_info.code, 0);
    assert!(!k.context.doing_boolean());
}
