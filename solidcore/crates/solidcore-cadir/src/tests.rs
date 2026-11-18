//! Models read into records: cadmpeg's unit cube as it is, and changed the ways a producer may
//! write the same solid (an edge's vertices the other way round, a face on a plane facing the
//! other way), and broken the ways hostile input may be. The cube is 10 mm across: 1000 mm³.
//! A prism over an ellipse's segment, built here, has its curved side on a procedural surface
//! with no solved form.

use super::*;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

fn cube() -> CadIr {
    cadmpeg_ir::examples::unit_cube().unwrap()
}

fn one(ir: &CadIr) -> Solid {
    let bodies = read_bodies(ir, None);
    assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
    assert_eq!(bodies.read.len(), 1);
    bodies.read.into_iter().next().unwrap().solid
}

/// A solved surface read as a record surface, and whether it is reversed against CADIR's.
fn record_surface(g: &SurfaceGeometry) -> (solidcore_record::brep::Surface, bool) {
    match geometry::surface(g).unwrap() {
        Some((geometry::FaceSurface::Record(s), reversed)) => (s, reversed),
        other => panic!("{other:?}"),
    }
}

/// The record's volume: positive when its faces face out of the material.
fn volume(s: &Solid) -> f64 {
    let (body, _) = solidcore_ops::Body::from_record(&s.brep, false).unwrap();
    body.volume(1e-9).unwrap()
}

#[test]
fn the_example_cube_is_a_closed_solid_record_of_its_volume() {
    let s = one(&cube());
    assert_eq!(
        (
            s.brep.faces.len(),
            s.brep.edges.len(),
            s.brep.vertices.len()
        ),
        (6, 12, 8)
    );
    assert!(!s.sheet);
    assert!(s.turned.is_empty(), "{:?}", s.turned);
    for e in &s.brep.edges {
        assert_eq!(e.uses.len(), 2);
    }
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
    // Each face's origin is its index in the model's faces.
    let mut origins = s.face_origin.clone();
    origins.sort_unstable();
    assert_eq!(origins, vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn an_edge_whose_vertices_are_written_the_other_way_round_is_turned_and_noted() {
    let mut ir = cube();
    let edge = ir.model.edges.first_mut().unwrap();
    std::mem::swap(&mut edge.start, &mut edge.end);
    let s = one(&ir);
    assert!(
        s.notes
            .iter()
            .any(|n| n.contains("1 edges whose vertices lie at the other ends")),
        "{:?}",
        s.notes
    );
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn a_face_on_a_plane_facing_the_other_way_keeps_its_side() {
    let mut ir = cube();
    let face = ir.model.faces.first().unwrap().clone();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|s| s.id == face.surface)
        .unwrap();
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
        panic!("{:?}", surface.geometry)
    };
    let o = plane.origin().get();
    let (n, u) = (
        plane.frame().axis().as_raw(),
        plane.frame().reference().as_raw(),
    );
    let flipped = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
        Point3::new(o.x, o.y, o.z),
        Vector3::new(-n.x, -n.y, -n.z),
        Vector3::new(u.x, u.y, u.z),
    )
    .unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(flipped));
    let f = ir.model.faces.first_mut().unwrap();
    f.sense = if f.sense == Sense::Forward {
        Sense::Reversed
    } else {
        Sense::Forward
    };
    let s = one(&ir);
    assert!(s.turned.is_empty(), "{:?}", s.turned);
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn a_missing_coedge_refuses_its_region_without_a_panic() {
    let mut ir = cube();
    ir.model.coedges.remove(0);
    let bodies = read_bodies(&ir, None);
    assert!(bodies.read.is_empty());
    assert_eq!(bodies.failures.len(), 1);
    assert!(
        bodies.failures[0].message.contains("missing coedge"),
        "{:?}",
        bodies.failures
    );
}

#[test]
fn a_vertex_far_from_its_edges_refuses_its_region() {
    let mut ir = cube();
    let v = ir.model.vertices.first().unwrap().point.clone();
    let p = ir.model.points.iter_mut().find(|p| p.id == v).unwrap();
    // The point moved 5 mm along x, through its wire form.
    let mut wire = serde_json::to_value(&*p).unwrap();
    let x = wire["position"]["x"].as_f64().unwrap();
    wire["position"]["x"] = serde_json::json!(x + 5.0);
    *p = serde_json::from_value(wire).unwrap();
    let bodies = read_bodies(&ir, None);
    assert!(bodies.read.is_empty());
    assert!(
        bodies.failures[0].message.contains("off its curve"),
        "{:?}",
        bodies.failures
    );
}

/// The side a record surface keeps against CADIR's, checked by the normals at one point: CADIR's
/// sphere and torus (`c + r(cos v cos u x + cos v sin u y + sin v z)` and
/// `c + (R + r cos v)(cos u x + sin u y) + r sin v z`, cadmpeg's evaluator), and the record's with
/// the radius's magnitude, each normal `∂u × ∂v`. A sphere of negative radius has CADIR's normal
/// pointing in where the record's points out, so it is reversed; a torus of negative tube radius
/// is the same surface with v moved by π, its normal away from the tube's centre either way, so it
/// keeps its side. Both signs are checked, so a reversal applied to both could not pass.
#[test]
fn a_negative_radius_turns_the_surface_side_as_the_normals_say() {
    use cadmpeg_ir::geometry::analytic::{SphereSurface, TorusSurface};
    use solidcore_record::brep::Surface as BSurface;
    type F = dyn Fn(f64, f64) -> [f64; 3];
    let (c, z, x) = (
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    let (cx, cy, cz) = (c.x, c.y, c.z);
    let at = move |a: f64, b: f64, h: f64| [cx + a, cy + b, cz + h];
    let sphere = |r: f64| {
        move |u: f64, v: f64| at(r * v.cos() * u.cos(), r * v.cos() * u.sin(), r * v.sin())
    };
    let torus = |r: f64| {
        move |u: f64, v: f64| {
            let ring = 20.0 + r * v.cos();
            at(ring * u.cos(), ring * u.sin(), r * v.sin())
        }
    };
    // ∂u × ∂v by central differences.
    let normal = |f: &F, u: f64, v: f64| {
        let h = 1e-6;
        let d = |p: [f64; 3], q: [f64; 3]| [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
        let du = d(f(u - h, v), f(u + h, v));
        let dv = d(f(u, v - h), f(u, v + h));
        [
            du[1] * dv[2] - du[2] * dv[1],
            du[2] * dv[0] - du[0] * dv[2],
            du[0] * dv[1] - du[1] * dv[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9);
    let (u, v) = (0.3, 0.4);
    let pi = std::f64::consts::PI;
    for r in [5.0, -5.0] {
        let g = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(c, z, x, r).unwrap(),
        ));
        let (s, reversed) = record_surface(&g);
        assert!(
            matches!(s, BSurface::Sphere { radius, .. } if radius == 5.0),
            "{s:?}"
        );
        // The record's point there: (u + π, −v) on the sphere of the radius's magnitude.
        let (theirs, ours) = (sphere(r), sphere(5.0));
        let (ou, ov) = if r < 0.0 { (u + pi, -v) } else { (u, v) };
        assert!(near(theirs(u, v), ours(ou, ov)), "sphere radius {r}");
        let agree = dot(normal(&theirs, u, v), normal(&ours, ou, ov)) > 0.0;
        assert_eq!(reversed, !agree, "sphere radius {r}");
        assert_eq!(reversed, r < 0.0, "sphere radius {r}");

        let g = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(c, z, x, 20.0, r).unwrap(),
        ));
        let (s, reversed) = record_surface(&g);
        assert!(
            matches!(s, BSurface::Torus { minor_radius, major_radius, .. } if minor_radius == 5.0 && major_radius == 20.0),
            "{s:?}"
        );
        // The record's point there: (u, v + π) on the tube of the radius's magnitude.
        let (theirs, ours) = (torus(r), torus(5.0));
        let ov = if r < 0.0 { v + pi } else { v };
        assert!(near(theirs(u, v), ours(u, ov)), "torus tube radius {r}");
        let agree = dot(normal(&theirs, u, v), normal(&ours, u, ov)) > 0.0;
        assert_eq!(reversed, !agree, "torus tube radius {r}");
        assert!(!reversed, "torus tube radius {r}");
    }
}

/// A NURBS surface CADIR marks `normal_reversed` is reversed against its record, whose natural
/// normal is `Pu × Pv`; one not marked is not.
#[test]
fn a_nurbs_surface_marked_normal_reversed_turns_the_side() {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    for flag in [false, true] {
        let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
        let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
        let poles = NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![p(0.0, 0.0), p(0.0, 1.0)],
                vec![p(1.0, 0.0), p(1.0, 1.0)],
            ],
        };
        let g = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::new(axis(), axis(), poles, flag).unwrap(),
        ));
        let (_, reversed) = record_surface(&g);
        assert_eq!(reversed, flag);
    }
}

/// A polyline whose parameters decrease along its samples is the same map from parameter to
/// point: the point at each parameter is the sample given with it.
#[test]
fn a_polyline_with_decreasing_parameters_keeps_its_parameters() {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let at = |t: f64, x: f64| PolylineVertex {
        parameter: t,
        point: Point3::new(x, 0.0, 0.0),
    };
    let samples = PolylineSamples::Parameterized {
        vertices: vec![at(2.0, 0.0), at(1.0, 4.0), at(0.0, 5.0)]
            .try_into()
            .unwrap(),
    };
    let g = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
        PolylineCurve::new(samples, 0.0).unwrap(),
    ));
    let Some(geometry::Carrier::Curve {
        curve: solidcore_record::brep::Curve::Spline { knots, points },
        periodic: false,
    }) = geometry::curve(&g).unwrap()
    else {
        panic!("not a spline")
    };
    assert_eq!(knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(
        points,
        vec![[5.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 0.0, 0.0]]
    );
}

/// The cube's tolerances: how many edges and vertices hold more than the model's tolerance.
fn widened(s: &Solid) -> (usize, usize) {
    let tol = s.brep.tolerance;
    (
        s.brep.edges.iter().filter(|e| e.tolerance > tol).count(),
        s.brep.vertices.iter().filter(|v| v.tolerance > tol).count(),
    )
}

/// A polyline's chords stand within the chordal deflection its producer states of the curve
/// they sample: the edge on it holds that allowance (and its two vertices with it), so the record
/// does not take the chords for the curve; one past what an edge's tolerance holds refuses its
/// region with why, and a polyline whose chords are the curve (deflection 0) changes nothing.
#[test]
fn a_polylines_chordal_deflection_is_its_edges_tolerance() {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let with = |deflection: f64| {
        let mut ir = cube();
        let edge = ir.model.edges[0].clone();
        let at = |v: &cadmpeg_ir::ids::VertexId| {
            let vertex = ir.model.vertices.iter().find(|x| x.id == *v).unwrap();
            let point = ir
                .model
                .points
                .iter()
                .find(|p| p.id == vertex.point)
                .unwrap();
            point.position().get()
        };
        let [t0, t1] = edge.param_range().unwrap().get();
        let samples = PolylineSamples::Parameterized {
            vertices: vec![
                PolylineVertex {
                    parameter: t0,
                    point: at(&edge.start),
                },
                PolylineVertex {
                    parameter: t1,
                    point: at(&edge.end),
                },
            ]
            .try_into()
            .unwrap(),
        };
        let curve = edge.carrier.curve().unwrap().clone();
        let c = ir.model.curves.iter_mut().find(|c| c.id == curve).unwrap();
        c.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
            PolylineCurve::new(samples, deflection).unwrap(),
        ));
        ir.finalize();
        ir
    };
    let s = one(&with(0.05));
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
    assert_eq!(widened(&s), (1, 2));
    assert!(s.brep.edges.iter().any(|e| e.tolerance >= 0.05));
    assert!(
        s.brep
            .vertices
            .iter()
            .filter(|v| v.tolerance >= 0.05)
            .count()
            == 2
    );
    assert_eq!(widened(&one(&with(0.0))), (0, 0));
    let bodies = read_bodies(&with(2.0), None);
    assert!(bodies.read.is_empty());
    assert!(
        bodies.failures[0]
            .message
            .contains("more than an edge's tolerance holds"),
        "{:?}",
        bodies.failures
    );
}

/// A procedural surface's solved cache fitted within a tolerance its construction states is not
/// the exact surface: read as the face's surface, the face's edges (and their vertices) hold that
/// tolerance; a cache whose fit no edge's tolerance holds refuses the region, naming the face;
/// a cache stated with no fit is read as before.
#[test]
fn a_fitted_procedural_cache_is_held_to_its_fit_not_taken_as_exact() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition as D;
    use cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction;
    use cadmpeg_ir::geometry::{CacheContract, LegacyCache, ProceduralSurface};
    let with = |fit: Option<f64>| {
        let mut ir = cube();
        let construction: cadmpeg_ir::ids::ProceduralSurfaceId = tid("procedural", 0);
        let cache = match fit {
            Some(t) => CacheContract::Legacy {
                cache: Some(LegacyCache::try_new(t).unwrap()),
            },
            None => CacheContract::legacy(),
        };
        let directrix = ir.model.curves[0].id.clone();
        let definition = D::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                directrix,
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                cache,
            )
            .unwrap(),
        );
        assert_eq!(
            definition.cache_fit_tolerance().map(|t| t.get()),
            fit,
            "the construction states the fit"
        );
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            construction.clone(),
            definition,
            None,
        ));
        let surface = &mut ir.model.surfaces[0];
        let SurfaceGeometry::Solved(plane) = surface.geometry.clone() else {
            panic!("the cube's faces are on planes")
        };
        surface.geometry = SurfaceGeometry::Procedural {
            construction,
            cache: Some(plane),
        };
        ir.finalize();
        ir
    };
    let s = one(&with(Some(0.05)));
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
    // The face's four edges, and their four corners.
    assert_eq!(widened(&s), (4, 4));
    assert!(s.brep.edges.iter().filter(|e| e.tolerance >= 0.05).count() == 4);
    assert_eq!(widened(&one(&with(None))), (0, 0));
    let bodies = read_bodies(&with(Some(2.0)), None);
    assert!(bodies.read.is_empty());
    let why = &bodies.failures[0].message;
    assert!(
        why.contains("face") && why.contains("its solved cache"),
        "{why}"
    );
}

/// A placement that scales unevenly, by however little more than rounding, is refused; a
/// rotation is taken.
#[test]
fn a_placement_scaling_unevenly_is_refused() {
    let uneven = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0 + 5e-10, 0.0],
    ];
    assert!(geometry::Similarity::of(uneven).is_err());
    let (c, s) = (0.6, 0.8);
    let turned = [[c, -s, 0.0, 3.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
    assert!(geometry::Similarity::of(turned).is_ok());
}

/// A shell's wire edges and free vertices, which a record of faces cannot hold yet, are noted on
/// the region, and its faces still come in: the cube's six faces, and one free vertex noted.
#[test]
fn a_shells_free_vertex_is_noted_and_its_faces_kept() {
    let mut ir = cube();
    let shell = ir.model.shells.first().unwrap().clone();
    let vertex = ir.model.vertices.first().unwrap().id.clone();
    let with_vertex = cadmpeg_ir::topology::Shell::new(
        shell.id.clone(),
        shell.region.clone(),
        shell.faces().to_vec(),
        Vec::new(),
        vec![vertex],
    )
    .unwrap();
    *ir.model.shells.first_mut().unwrap() = with_vertex;
    let s = one(&ir);
    assert_eq!(s.brep.faces.len(), 6);
    assert!((volume(&s) - 1000.0).abs() < 1e-6);
    assert!(
        s.notes
            .iter()
            .any(|n| n == "1 wire edges and free vertices of its shells left out"),
        "{:?}",
        s.notes
    );
}

/// The cube with its top face (z = 10) on a cone whose half angle is 7e-12 short of a right
/// angle, radius 1 at the face's middle, as an exporter writes a plane: the plane it is, so the
/// record has a plane there and the cube's volume. The cone's normal points down its axis, into
/// the cube, so the face's sense is reversed against it.
#[test]
fn a_cone_of_nearly_a_right_angle_is_read_as_its_plane() {
    use cadmpeg_ir::geometry::analytic::ConeSurface;
    use solidcore_record::brep::Surface as BSurface;
    let mut ir = cube();
    let top = ir
        .model
        .faces
        .iter()
        .position(|f| {
            let s = ir
                .model
                .surfaces
                .iter()
                .find(|s| s.id == f.surface)
                .unwrap();
            matches!(&s.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(p))
                if p.origin().get().z == 10.0 && p.frame().axis().as_raw().z == 1.0)
        })
        .unwrap();
    let id = ir.model.faces[top].surface.clone();
    let cone = ConeSurface::try_new(
        Point3::new(5.0, 5.0, 10.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
        1.0,
        std::f64::consts::FRAC_PI_2 - 7e-12,
    )
    .unwrap();
    let surface = ir.model.surfaces.iter_mut().find(|s| s.id == id).unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone));
    ir.model.faces[top].sense = Sense::Reversed;
    let s = one(&ir);
    let face = s.face_origin.iter().position(|o| *o == top as u64).unwrap();
    assert!(
        matches!(s.brep.faces[face].surface, BSurface::Plane { .. }),
        "{:?}",
        s.brep.faces[face].surface
    );
    assert!(s.turned.is_empty(), "{:?}", s.turned);
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
}

/// A face with no loops on a sphere is the whole sphere (CADIR's form for a face of a closed
/// surface, which cadmpeg's STEP and FreeCAD readers write): the cube's shell given one such face
/// is a ball of radius 5, its volume (4/3)π·125.
#[test]
fn a_sphere_face_with_no_boundary_is_the_whole_ball() {
    use cadmpeg_ir::geometry::analytic::SphereSurface;
    let mut ir = cube();
    let shell = ir.model.shells.first().unwrap().clone();
    let face = ir.model.faces.first().unwrap().clone();
    *ir.model.shells.first_mut().unwrap() = cadmpeg_ir::topology::Shell::new(
        shell.id.clone(),
        shell.region.clone(),
        vec![face.id.clone()],
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    let ball = SphereSurface::try_new(
        Point3::new(5.0, 5.0, 5.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        5.0,
    )
    .unwrap();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|s| s.id == face.surface)
        .unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(ball));
    let f = ir.model.faces.first_mut().unwrap();
    f.loops = FaceLoops::unspecified(Vec::new());
    f.sense = Sense::Forward;
    let s = one(&ir);
    assert_eq!(s.brep.faces.len(), 1);
    let ball = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
    assert!((volume(&s) - ball).abs() < 1e-6 * ball, "{}", volume(&s));
}

/// A curve-less edge between two vertices at one point collapses, and the loops through it run on
/// from the one vertex the two are: the cube with one corner split into two vertices at its
/// point, joined by such an edge in both loops that pass there, is still the cube.
#[test]
fn an_edge_collapsed_between_two_vertices_at_one_point_keeps_its_loops_closed() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, VertexId};
    use cadmpeg_ir::topology::{EdgeCarrier, LoopRing, Vertex};
    let mut ir = cube();
    let first = ir.model.edges.first().unwrap().clone();
    let a = first.start.clone();
    let point = ir
        .model
        .vertices
        .iter()
        .find(|v| v.id == a)
        .unwrap()
        .point
        .clone();
    let split: VertexId = tid("vertex", 0);
    ir.model.vertices.push(Vertex {
        id: split.clone(),
        point,
        tolerance: None,
    });
    ir.model.edges.first_mut().unwrap().start = split.clone();
    let collapsed: EdgeId = tid("edge", 0);
    ir.model.edges.push(cadmpeg_ir::topology::Edge {
        id: collapsed.clone(),
        carrier: EdgeCarrier::new(None, None).unwrap(),
        start: a,
        end: split,
        tolerance: None,
    });
    // In each loop through the first edge, a use of the collapsed edge where the loop passes
    // the corner: before the edge's use when that runs from the corner, else after it.
    let uses: Vec<Coedge> = ir
        .model
        .coedges
        .iter()
        .filter(|c| c.edge == first.id)
        .cloned()
        .collect();
    assert_eq!(uses.len(), 2);
    let ids: [CoedgeId; 2] = [tid("coedge", 0), tid("coedge", 1)];
    for (k, c) in uses.iter().enumerate() {
        ir.model.coedges.push(Coedge {
            id: ids[k].clone(),
            owner_loop: c.owner_loop.clone(),
            edge: collapsed.clone(),
            radial_next: ids[1 - k].clone(),
            sense: c.sense,
            pcurves: Vec::new(),
            use_curve: None,
        });
        let lp = ir
            .model
            .loops
            .iter_mut()
            .find(|l| l.id == c.owner_loop)
            .unwrap();
        let LoopBoundary::Ring(ring) = &lp.boundary else {
            panic!("{:?}", lp.boundary)
        };
        let mut ring: Vec<CoedgeId> = ring.coedges().to_vec();
        let at = ring.iter().position(|x| *x == c.id).unwrap();
        let at = if c.sense == Sense::Forward {
            at
        } else {
            at + 1
        };
        ring.insert(at, ids[k].clone());
        lp.boundary = LoopBoundary::Ring(LoopRing::new(ring, Vec::new()).unwrap());
    }
    let s = one(&ir);
    assert_eq!(
        (
            s.brep.faces.len(),
            s.brep.edges.len(),
            s.brep.vertices.len()
        ),
        (6, 12, 8)
    );
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
}

/// A coedge with a 3D curve of its own in place of its edge's refuses its region with the reason,
/// rather than being built on the edge's curve.
#[test]
fn a_coedge_on_a_curve_of_its_own_refuses_its_region() {
    let mut ir = cube();
    let curve = ir.model.curves.first().unwrap().id.clone();
    let c = ir.model.coedges.first_mut().unwrap();
    c.use_curve = Some(cadmpeg_ir::topology::CoedgeUseCurve {
        curve,
        parameter_range: cadmpeg_ir::topology::ParameterInterval::new([0.0, 1.0]).unwrap(),
    });
    let bodies = read_bodies(&ir, None);
    assert!(bodies.read.is_empty());
    assert!(
        bodies.failures[0].message.contains("a 3D curve of its own"),
        "{:?}",
        bodies.failures
    );
}

/// An id of kind `kind` with key `key`, in a namespace of these tests.
fn tid<T: TryFrom<String>>(kind: &str, key: usize) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(format!("test:segment:{kind}#{key}")).unwrap()
}

/// A prism over the segment of the ellipse `(10 cos t, 6 sin t)` cut off by the chord x = 5
/// (the ellipse from −60° round its parameter's origin to +60°), 5 mm tall: its curved side on
/// `side`, a procedural surface with no solved form, its face's sense `sense`.
fn segment_prism(
    side: impl FnOnce(cadmpeg_ir::ids::CurveId) -> cadmpeg_ir::geometry::ProceduralSurfaceDefinition,
    sense: Sense,
) -> CadIr {
    use cadmpeg_ir::geometry::analytic::{EllipseCurve, LineCurve, PlaneSurface};
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, ProceduralSurface, SolvedCurveGeometry, Surface,
    };
    use cadmpeg_ir::topology::{
        Body, Coedge, EdgeCarrier, FaceLoops, LoopRing, Point, Region, Vertex,
    };
    let (b, h) = (6.0, 5.0);
    let s = std::f64::consts::FRAC_PI_3.sin();
    let corners = [
        [5.0, -b * s, 0.0],
        [5.0, b * s, 0.0],
        [5.0, b * s, h],
        [5.0, -b * s, h],
    ];
    let mut ir = CadIr::empty();
    for (i, c) in corners.iter().enumerate() {
        let at = cadmpeg_ir::features::FinitePoint3::new(Point3::new(c[0], c[1], c[2])).unwrap();
        ir.model.points.push(Point::new(tid("point", i), at, None));
        ir.model.vertices.push(Vertex {
            id: tid("vertex", i),
            point: tid("point", i),
            tolerance: None,
        });
    }
    // Curves 0 and 1: the ellipse at the bottom and the top; then a line per straight edge.
    let z = Vector3::new(0.0, 0.0, 1.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    let mut curves = Vec::new();
    for c in [0.0, h] {
        curves.push(SolvedCurveGeometry::Ellipse(
            EllipseCurve::try_new(Point3::new(0.0, 0.0, c), z, x, 10.0, b).unwrap(),
        ));
    }
    // Edges: (curve, interval, start, end): the two arcs, the chords low and high, the rises
    // at +y and −y.
    let third = std::f64::consts::FRAC_PI_3;
    let mut edges = vec![(0, [-third, third], 0, 1), (1, [-third, third], 3, 2)];
    for (a, z) in [(1, 0), (2, 3), (1, 2), (0, 3)] {
        let (p, q) = (corners[a], corners[z]);
        let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        curves.push(SolvedCurveGeometry::Line(
            LineCurve::try_new(
                Point3::new(p[0], p[1], p[2]),
                Vector3::new(d[0] / len, d[1] / len, d[2] / len),
            )
            .unwrap(),
        ));
        edges.push((curves.len() - 1, [0.0, len], a, z));
    }
    for (i, g) in curves.into_iter().enumerate() {
        ir.model.curves.push(Curve {
            id: tid("curve", i),
            geometry: CurveGeometry::Solved(g),
            parameter_range: None,
            source_object: None,
        });
    }
    for (i, (c, interval, a, z)) in edges.into_iter().enumerate() {
        ir.model.edges.push(cadmpeg_ir::topology::Edge {
            id: tid("edge", i),
            carrier: EdgeCarrier::new(Some(tid("curve", c)), Some(interval)).unwrap(),
            start: tid("vertex", a),
            end: tid("vertex", z),
            tolerance: None,
        });
    }
    // Faces: bottom, top, the flat side on the chord, the curved side; each ring
    // counter-clockwise about the face's outward normal.
    let plane = |o: [f64; 3], n: [f64; 3], u: [f64; 3]| {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(o[0], o[1], o[2]),
                Vector3::new(n[0], n[1], n[2]),
                Vector3::new(u[0], u[1], u[2]),
            )
            .unwrap(),
        ))
    };
    let procedural: cadmpeg_ir::ids::ProceduralSurfaceId = tid("procedural", 0);
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        procedural.clone(),
        side(tid("curve", 0)),
        None,
    ));
    let faces = [
        (
            plane([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
            Sense::Forward,
            vec![(0, false), (2, false)],
        ),
        (
            plane([0.0, 0.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            Sense::Forward,
            vec![(1, true), (3, true)],
        ),
        (
            plane([5.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            Sense::Forward,
            vec![(2, true), (5, true), (3, false), (4, false)],
        ),
        (
            SurfaceGeometry::Procedural {
                construction: procedural,
                cache: None,
            },
            sense,
            vec![(0, true), (4, true), (1, false), (5, false)],
        ),
    ];
    let mut uses: HashMap<String, Vec<String>> = HashMap::new();
    for (f, (geometry, sense, ring)) in faces.into_iter().enumerate() {
        ir.model.surfaces.push(Surface {
            id: tid("surface", f),
            geometry,
            source_object: None,
        });
        let coedges: Vec<cadmpeg_ir::ids::CoedgeId> =
            (0..ring.len()).map(|k| tid("coedge", 10 * f + k)).collect();
        for (k, (e, forward)) in ring.into_iter().enumerate() {
            uses.entry(tid::<cadmpeg_ir::ids::EdgeId>("edge", e).to_string())
                .or_default()
                .push(coedges[k].to_string());
            ir.model.coedges.push(Coedge {
                id: coedges[k].clone(),
                owner_loop: tid("loop", f),
                edge: tid("edge", e),
                radial_next: coedges[k].clone(),
                sense: if forward {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: Vec::new(),
                use_curve: None,
            });
        }
        ir.model.loops.push(cadmpeg_ir::topology::Loop {
            id: tid("loop", f),
            face: tid("face", f),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                LoopRing::new(coedges, Vec::new()).unwrap(),
            ),
        });
        ir.model.faces.push(Face {
            id: tid("face", f),
            shell: tid("shell", 0),
            surface: tid("surface", f),
            sense,
            loops: FaceLoops::unspecified(vec![tid("loop", f)]),
            name: None,
            color: None,
            tolerance: None,
        });
    }
    // Each edge's two coedges partner each other.
    for c in &mut ir.model.coedges {
        let pair = &uses[c.edge.as_str()];
        if let Some(other) = pair.iter().find(|o| **o != c.id.to_string()) {
            c.radial_next = cadmpeg_ir::ids::CoedgeId::try_from(other.clone()).unwrap();
        }
    }
    ir.model.shells.push(
        Shell::new(
            tid("shell", 0),
            tid("region", 0),
            (0..4).map(|f| tid("face", f)).collect(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
    );
    ir.model.regions.push(Region {
        id: tid("region", 0),
        body: tid("body", 0),
        shells: vec![tid("shell", 0)],
    });
    ir.model.bodies.push(Body {
        id: tid("body", 0),
        kind: BodyKind::Solid,
        regions: vec![tid("region", 0)],
        transform: None,
        name: Some("segment".into()),
        color: None,
        visible: None,
    });
    ir.finalize();
    ir
}

/// The segment prism's curved side as the ellipse swept up (a linear sweep, its normal
/// ∂ellipse × up out of the prism) or as an extrusion down (its normal into the prism, so the
/// face's sense is reversed), neither with a solved form: each is read by its definition, its
/// profile bounded to the narrow segment it covers (not wrapped round the ellipse from the
/// parameter's origin), and the prism has the segment's volume: the circle's segment (r = 10,
/// 120°) scaled by 6/10 across the chord.
#[test]
fn an_extruded_ellipse_without_a_solved_form_is_read_by_its_definition() {
    use cadmpeg_ir::geometry::CacheContract;
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition as D;
    use cadmpeg_ir::geometry::surface_payloads::{
        ExtrusionSurfaceConstruction, LinearSweepSurfaceConstruction,
    };
    let up = |c| {
        D::LinearSweep(
            LinearSweepSurfaceConstruction::try_new(c, Vector3::new(0.0, 0.0, 5.0)).unwrap(),
        )
    };
    let down = |c| {
        D::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                c,
                None,
                Vector3::new(0.0, 0.0, -1.0),
                None,
                CacheContract::from_form(None),
            )
            .unwrap(),
        )
    };
    let theta = 2.0 * std::f64::consts::FRAC_PI_3;
    let expected = 0.6 * 50.0 * (theta - theta.sin()) * 5.0;
    for (name, ir) in [
        ("up", segment_prism(up, Sense::Forward)),
        ("down", segment_prism(down, Sense::Reversed)),
    ] {
        let s = one(&ir);
        assert_eq!(
            (
                s.brep.faces.len(),
                s.brep.edges.len(),
                s.brep.vertices.len()
            ),
            (4, 6, 4),
            "{name}"
        );
        assert!(s.turned.is_empty(), "{name}: {:?}", s.turned);
        let v = volume(&s);
        assert!(
            (v - expected).abs() < 1e-6 * expected,
            "{name}: {v} vs {expected}"
        );
    }
}

/// A procedural revolution's normal against the definition's own (∂angle × ∂directrix): a
/// revolution about an axis and a native one `transposed` run u round the axis, as the
/// definition does; a native one not transposed runs u along the directrix, the other way.
#[test]
fn a_procedural_revolution_states_its_normal_against_the_definitions() {
    use cadmpeg_ir::geometry::CacheContract;
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition as D;
    use cadmpeg_ir::geometry::surface_payloads::{
        AxisRevolutionSurfaceConstruction, RevolutionSurfaceConstruction,
    };
    use solidcore_record::brep::Curve as BCurve;
    let profile = || geometry::Carrier::Curve {
        curve: BCurve::Line {
            start: [10.0, 0.0, 0.0],
            end: [10.0, 0.0, 1.0],
        },
        periodic: false,
    };
    let origin = cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
    let axis = cadmpeg_ir::units::UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap();
    let about = D::AxisRevolution(AxisRevolutionSurfaceConstruction::from_parts(
        tid("curve", 0),
        origin,
        axis,
    ));
    let native = |transposed| {
        D::Revolution(
            RevolutionSurfaceConstruction::try_new(
                tid("curve", 0),
                (origin, axis),
                [0.0, std::f64::consts::TAU],
                None,
                None,
                transposed,
                CacheContract::from_form(None),
            )
            .unwrap(),
        )
    };
    for (d, reversed) in [(about, false), (native(true), false), (native(false), true)] {
        let (definition, got) = geometry::swept(&d, profile()).unwrap();
        assert!(
            matches!(
                definition,
                solidcore_step::assembly::SweptSurface::Revolution {
                    axis: [0.0, 0.0, 1.0],
                    ..
                }
            ),
            "{definition:?}"
        );
        assert_eq!(got, reversed, "{d:?}");
    }
}

/// A body placed by a scale, or by a mirror, is refused with the reason rather than read where
/// it is: its geometry would otherwise be at the wrong size or place. A rigid placement moves it.
#[test]
fn a_body_placed_by_a_scale_or_mirror_is_refused_and_a_rigid_one_moves_it() {
    let placed = |rows: [[f64; 4]; 3]| {
        let mut ir = cube();
        ir.model.bodies.first_mut().unwrap().transform =
            Some(cadmpeg_ir::transform::Transform::affine(rows).unwrap());
        read_bodies(&ir, None)
    };
    for rows in [
        [
            [2.0, 0.0, 0.0, 100.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
        ],
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    ] {
        let b = placed(rows);
        assert!(b.read.is_empty(), "{rows:?}");
        assert!(
            b.failures[0].message.starts_with("its placement:"),
            "{:?}",
            b.failures
        );
    }
    let b = placed([
        [1.0, 0.0, 0.0, 100.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    assert_eq!(b.read.len(), 1);
    assert_eq!(
        b.read[0].placement.map(|m| m[3]),
        Some([100.0, 0.0, 0.0, 1.0])
    );
}
