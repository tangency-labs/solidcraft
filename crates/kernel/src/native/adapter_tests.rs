use super::*;
use mt::{BoundedCurve, InnerSpace, Transformed};
use solidcraft_geom::Vec3;

#[test]
fn refuses_domain_or_axis_changes_instead_of_approximating() {
    let curve = b::Curve::Spline { knots: vec![0., 1., 2., 3.], points: vec![[0.; 3], [1.; 3]] };
    assert!(compatible_curve(&curve).is_err());
    let plane = b::Surface::Plane { origin: [1e12; 3], u_axis: [1e-6, 0., 0.], v_axis: [0., 1., 0.] };
    assert!(compatible_surface(&plane).is_err());
    assert!(clamped(&[0., 0., 1., 1.], 2).is_ok());
    let spline = b::Curve::Spline { knots: vec![0., 0., 1., 1.], points: vec![[0.; 3], [1.; 3]] };
    assert!(load_edge_curve(spline, [0., 1.]).is_ok());
}

/// A spline edge on part of its curve (SMLib bounds an intersection edge on a piece of its
/// spline) loads as the piece cut out by knot insertion, not a refit: the same points at the
/// same parameters, over exactly the interval.
#[test]
fn spline_edges_on_part_of_their_curve_load_as_that_piece() {
    use mt::ParametricCurve;
    let knots = vec![0., 0., 0., 0., 0.4, 0.4, 0.7, 1., 1., 1., 1.];
    let points: Vec<[f64; 3]> = (0..7).map(|i| [f64::from(i), (f64::from(i) * 1.3).sin() * 4., f64::from(i * i) * 0.2]).collect();
    let weights = [1., 0.8, 1.3, 0.9, 1.1, 0.7, 1.];
    let rational = b::Curve::RationalSpline {
        knots: knots.clone(),
        homogeneous: points.iter().zip(weights).map(|(q, w)| [q[0] * w, q[1] * w, q[2] * w, w]).collect(),
    };
    for record in [b::Curve::Spline { knots, points }, rational] {
        let exact = solidcraft_geom::brep::eval::CurveEval::new(&record).unwrap();
        for interval in [[0.1, 0.35], [0.25, 0.9], [0., 0.4], [0.4, 1.]] {
            let piece = load_edge_curve(record.clone(), interval).unwrap();
            assert_eq!(piece.range_tuple(), (interval[0], interval[1]), "{interval:?}");
            for k in 0..=20 {
                let t = interval[0] + (interval[1] - interval[0]) * f64::from(k) / 20.;
                let (q, r) = (piece.subs(t), exact.point(t).unwrap());
                assert!((q - p([r.x, r.y, r.z])).magnitude() < 1e-12, "{interval:?} at {t}");
            }
        }
        assert!(load_edge_curve(record, [0.5, 1.2]).is_err(), "beyond the curve's domain");
    }
}

#[test]
fn analytic_records_decode_as_their_mathematical_loci_and_normals() {
    use mt::{MetricSpace, ParametricSurface, ParametricSurface3D};
    use std::f64::consts::{FRAC_PI_2, TAU};
    let frame = b::Frame { origin: [1., 2., 3.], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let circle = load_edge_curve(b::Curve::Circle { frame: frame.clone(), radius: 2. }, [0., FRAC_PI_2]).unwrap();
    assert!(circle.subs(0.).distance(p([3., 2., 3.])) < 1e-12);
    assert!(circle.subs(1.).distance(p([1., 4., 3.])) < 1e-12);
    let ellipse = load_edge_curve(b::Curve::Ellipse { frame: frame.clone(), radii: [2., 3.] }, [0., FRAC_PI_2]).unwrap();
    assert!(ellipse.subs(1.).distance(p([1., 5., 3.])) < 1e-12);
    // Past the seam: an arc from -π/2 to π/2.
    let across = load_edge_curve(b::Curve::Circle { frame: frame.clone(), radius: 2. }, [-FRAC_PI_2, FRAC_PI_2]).unwrap();
    assert!(across.subs(0.).distance(p([1., 0., 3.])) < 1e-12);
    // A line edge on part of its line.
    let line = load_edge_curve(b::Curve::Line { start: [0.; 3], end: [1., 0., 0.] }, [-1., 2.]).unwrap();
    assert!(line.subs(0.).distance(p([-1., 0., 0.])) < 1e-12 && line.subs(1.).distance(p([2., 0., 0.])) < 1e-12);
    let cases = [
        (b::Surface::Cylinder { frame: frame.clone(), radius: 2. }, [-1., 3.], 0.5, [3., 2., 4.], [1., 0., 0.]),
        (
            b::Surface::Cone { frame: frame.clone(), radius_at_origin: 2., half_angle: 0.2 },
            [0., 3.],
            0.5,
            [3. + 1.5 * 0.2f64.tan(), 2., 4.5],
            [1., 0., -0.2f64.tan()],
        ),
        (b::Surface::Sphere { frame: frame.clone(), radius: 2. }, [-FRAC_PI_2, FRAC_PI_2], 0.5, [3., 2., 3.], [1., 0., 0.]),
        (b::Surface::Torus { frame, major_radius: 4., minor_radius: 1. }, [0., TAU], 0., [6., 2., 3.], [1., 0., 0.]),
    ];
    for (record, height, parameter, expected, normal) in cases {
        // Truck's revolution as truck makes its own: (u, v) = (the profile's parameter, the angle).
        let surface = load_surface(record, [[0., TAU], height]).unwrap();
        assert!(surface.subs(parameter, 0.).distance(p(expected)) < 1e-10);
        assert!(surface.normal(parameter, 0.).normalize().dot(v(normal).normalize()) > 1. - 1e-10);
    }
}

/// Every face, edge and vertex index of the record is the kernel's own: faces in `face_iter`
/// order, edges in `Body::edges` order (first appearance through `edge_iter`, which walks a
/// reversed face backwards).
fn same_indices(body: &Body, record: &b::Brep) {
    assert_eq!(record.faces.len(), body.face_count());
    let edges = Body::unique_edges(&body.solid);
    assert_eq!(record.edges.len(), edges.len());
    for (r, e) in record.edges.iter().zip(&edges) {
        let ends = [e.absolute_front().point(), e.absolute_back().point()];
        let recorded = r.vertices.map(|i| p(record.vertices[i as usize].point));
        assert_eq!(recorded, ends);
    }
    for (r, f) in record.faces.iter().zip(body.solid.face_iter()) {
        let b::Loop::Edges { edges } = &r.loops[0] else { panic!("an edge loop") };
        assert_eq!(edges.len(), f.absolute_boundaries()[0].len());
    }
    let history = crate::history::edge_faces(&body.solid);
    for (i, faces) in history.iter().enumerate() {
        let mut recorded: Vec<usize> = record.edges[i].uses.iter().map(|u| u.face as usize).collect();
        recorded.dedup();
        assert_eq!(&recorded, faces, "edge {i}");
    }
}

#[test]
fn planar_bodies_encode_in_the_kernels_order_and_decode_exactly() {
    let cube = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
    let slot = crate::box_solid(Vec3::new(10., -1., 4.), Vec3::new(20., 31., 11.)).unwrap();
    let notched = crate::boolean(&cube, &slot, crate::BoolOp::Cut).unwrap().unwrap();
    let sheet = crate::copy_faces(&cube, &[Vec3::new(20., 15., 10.)]).unwrap();
    let inner = crate::box_solid(Vec3::new(5., 5., 2.), Vec3::new(10., 10., 6.)).unwrap();
    let hollow = crate::boolean(&cube, &inner, crate::BoolOp::Cut).unwrap().unwrap();
    let far = crate::box_solid(Vec3::new(60., 0., 0.), Vec3::new(70., 10., 10.)).unwrap();
    let pair = crate::boolean(&cube, &far, crate::BoolOp::Union).unwrap().unwrap();
    let bodies = [("cube", cube), ("notched", notched), ("sheet", sheet), ("hollow", hollow), ("pair", pair)];
    assert!(bodies.iter().any(|(_, b)| b.solid.face_iter().any(|f| !f.orientation())), "reversed faces are walked");
    for (name, body) in bodies {
        let record = encode(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        same_indices(&body, &record);
        let decoded = decode(record.clone()).unwrap();
        let again = Body::new(decoded).unwrap();
        let record2 = encode(&again).unwrap();
        assert_eq!(record, record2, "{name}: decoded body re-encodes to the same record");
        match name {
            "sheet" => assert_eq!(record.regions, vec![b::Region { shells: vec![0], void: false }]),
            "hollow" => {
                assert_eq!(record.shells.len(), 2);
                assert_eq!(
                    record.regions,
                    vec![
                        b::Region { shells: vec![0], void: false },
                        b::Region { shells: vec![0, 1], void: false },
                        b::Region { shells: vec![1], void: true },
                    ]
                );
            }
            "pair" => assert_eq!(
                record.regions,
                vec![
                    b::Region { shells: vec![0, 1], void: false },
                    b::Region { shells: vec![0], void: false },
                    b::Region { shells: vec![1], void: false },
                ]
            ),
            _ => assert_eq!(record.regions, vec![b::Region { shells: vec![0], void: false }, b::Region { shells: vec![0], void: false }]),
        }
    }
}

#[test]
fn records_truck_cannot_hold_are_refused() {
    let cube = crate::box_solid(Vec3::ZERO, Vec3::new(4., 3., 1.)).unwrap();
    let record = encode(&cube).unwrap();
    let mut wire = record.clone();
    wire.shells.push(b::Shell::Vertex { vertex: 0 });
    assert!(decode(wire).is_err());
    let mut vertex_loop = record;
    vertex_loop.faces[0].loops.push(b::Loop::Vertex { vertex: 0 });
    assert!(decode(vertex_loop).is_err());
}

/// Truck builds edges and faces unchecked: a record whose geometry misses its topology is
/// refused before it gets there.
#[test]
fn records_whose_geometry_misses_their_topology_are_refused() {
    let cube = crate::box_solid(Vec3::ZERO, Vec3::new(4., 3., 1.)).unwrap();
    let mut moved = encode(&cube).unwrap();
    moved.vertices[0].point[0] += 0.1;
    moved.validate().unwrap();
    let error = decode(moved).err().unwrap().to_string();
    assert!(error.contains("misses its"), "{error}");
}

#[test]
fn intersections_and_revolutions_reload_exactly() {
    let cyl = crate::cylinder(Vec3::ZERO, Vec3::Z, 5., 40.).unwrap();
    let block = crate::box_solid(Vec3::new(-20., -20., 20.), Vec3::new(20., 20., 60.)).unwrap();
    let block = crate::transform(&block, Vec3::ZERO, Vec3::new(0., 0., 20.), Vec3::X, 0.4).unwrap();
    let cut = crate::boolean(&cyl, &block, crate::BoolOp::Cut).unwrap().unwrap();
    let curve = cut.solid.edge_iter().map(|e| e.curve()).find(|c| matches!(c, mt::Curve::IntersectionCurve(_))).unwrap();
    let record = save_curve(&curve, 0).unwrap();
    assert!(matches!(record, b::Curve::SurfaceIntersection { .. }), "the procedural curve itself, not a fit");
    let (a, z) = curve.range_tuple();
    let restored = load_edge_curve(record.clone(), [a, z]).unwrap();
    for i in 0..=64 {
        let t = a + (z - a) * i as f64 / 64.;
        assert_eq!(curve.subs(t), restored.subs(t), "sample {i}");
    }
    assert_eq!(save_curve(&restored, 0).unwrap(), record);
    // A revolution keeps its own placement, reversed normal included.
    let line = mt::Curve::Line(mt::Line(mt::Point3::new(2., 0., 0.), mt::Point3::new(2.5, 0., 3.)));
    let mt::Surface::RevolutedCurve(mut placed) = revolved(line, mt::Point3::new(0., 1., 0.), mt::Vector3::new(0., 0., 1.), true) else {
        panic!("a revolution")
    };
    placed.transform_by(mt::Matrix4::from_translation(mt::Vector3::new(1., 2., 3.)) * mt::Matrix4::from_angle_x(mt::Rad(0.3)));
    let placed = mt::Surface::RevolutedCurve(placed);
    let record = save_surface(&placed, 0).unwrap();
    assert!(matches!(record, b::Surface::PlacedRevolution { forward: false, .. }));
    let restored = load_surface(record.clone(), UNIT).unwrap();
    assert_eq!(save_surface(&restored, 0).unwrap(), record);
    use mt::{ParametricSurface, ParametricSurface3D};
    for (u, v) in [(0., 0.), (1.3, 0.4), (4., 1.)] {
        assert_eq!(placed.subs(u, v), restored.subs(u, v));
        assert_eq!(placed.normal(u, v), restored.normal(u, v));
    }
    // The whole body: the same records again, and the same volume to the last bit.
    let bytes = crate::encode_native_body(&cut).unwrap();
    let decoded = crate::decode_native_body(&bytes).unwrap();
    assert_eq!(crate::encode_native_body(&decoded).unwrap(), bytes);
    assert_eq!(crate::measure(&decoded).unwrap().volume, crate::measure(&cut).unwrap().volume);
}

/// Each oriented edge's pcurve, lifted onto its face's surface, is its edge: checked at the
/// ends and the middle of the edge's interval, on faces of either orientation.
#[test]
fn every_pcurve_lifts_onto_its_own_edge() {
    use mt::{MetricSpace, ParametricCurve, ParametricSurface};
    let cube = crate::box_solid(Vec3::ZERO, Vec3::new(40., 30., 10.)).unwrap();
    let rounded = crate::fillet(&cube, &[Vec3::new(20., 0., 10.)], 2.).unwrap();
    let bore = crate::cylinder(Vec3::new(10., 10., -1.), Vec3::Z, 3., 12.).unwrap();
    let holed = crate::boolean(&rounded, &bore, crate::BoolOp::Cut).unwrap().unwrap();
    let sphere = crate::sphere(Vec3::ZERO, 12.).unwrap();
    for (name, body) in [("cube", cube), ("holed", holed), ("sphere", sphere)] {
        let record = encode(&body).unwrap();
        for (f, face) in record.faces.iter().zip(body.solid.face_iter()) {
            let surface = face.surface();
            for l in &f.loops {
                let b::Loop::Edges { edges } = l else { continue };
                for u in edges {
                    let e = &record.edges[u.edge as usize];
                    let curve = load_edge_curve(e.curve.clone(), e.interval).unwrap();
                    for s in [0., 0.5, 1.] {
                        let t = e.interval[0] + s * (e.interval[1] - e.interval[0]);
                        let [x, y] = u.pcurve.eval(t).unwrap();
                        let d = surface.subs(x, y).distance(curve.subs(t));
                        assert!(d <= e.tolerance, "{name}: edge {} at {s}: {d} mm off (forward face {})", u.edge, face.orientation());
                    }
                }
            }
        }
    }
}

/// SMLib's primitives load through their schema 4 records as bodies of SMLib's volume, oriented
/// as the records say (outward without `Body::new` turning them).
#[cfg(feature = "smlib")]
#[test]
fn smlib_primitives_decode_with_their_volume_and_orientation() {
    use solidcraft_smlib::Brep as S;
    let cases = [
        ("box", S::make_box([1., 2., 3.], [10., 20., 30.]).unwrap()),
        ("cylinder", S::make_cylinder([1., 2., 3.], 5., 10.).unwrap()),
        ("sphere", S::make_sphere([1., 2., 3.], 5.).unwrap()),
        ("frustum", S::make_cone([0.; 3], 5., 2., 10.).unwrap()),
        ("widening frustum", S::make_cone([0.; 3], 2., 5., 10.).unwrap()),
        ("cone", S::make_cone([0.; 3], 5., 0., 10.).unwrap()),
        ("torus", S::make_torus([0.; 3], 10., 3.).unwrap()),
    ];
    for (name, s) in cases {
        let record = s.to_native().unwrap();
        let body = crate::native::body_from_smlib(&s).unwrap_or_else(|e| panic!("{name}: {e}"));
        let forward: Vec<bool> = record
            .shells
            .iter()
            .flat_map(|sh| match sh {
                b::Shell::Faces { faces, .. } => faces.iter().map(|f| f.forward).collect(),
                _ => Vec::new(),
            })
            .collect();
        let loaded: Vec<bool> = body.solid.boundaries().iter().flat_map(|sh| sh.face_iter().map(|f| f.orientation()).collect::<Vec<_>>()).collect();
        assert_eq!(loaded, forward, "{name}: the body was turned inside out");
        let smlib = s.mass_properties(solidcraft_smlib::MEASURE_ACCURACY).unwrap().volume;
        let ours = crate::measure(&body).unwrap().volume;
        assert!((ours - smlib).abs() < 2e-3 * smlib, "{name}: decoded volume {ours}, SMLib's {smlib}");
        assert_eq!(
            (body.face_count(), body.edge_count(), body.vertex_count()),
            (record.faces.len(), record.edges.len(), record.vertices.len()),
            "{name}"
        );
    }
}

/// SMLib's fillet round a hole's rim is a rational spline closed on itself in u, bounded by its
/// seam used twice: the seam's pcurves lie a period apart, so the face meshes and the decoded
/// body has SMLib's volume (both seam uses on u = 0 left the face empty, 149 mm³ short).
#[cfg(feature = "smlib")]
#[test]
fn smlib_hole_rim_fillet_decodes_with_its_volume() {
    use solidcraft_smlib::{BooleanOp, Brep as S};
    let block = S::make_box([0.; 3], [20.; 3]).unwrap();
    let pin = S::make_cylinder([10., 10., -1.], 4., 22.).unwrap();
    let holed = block.boolean(&pin, BooleanOp::Subtract).unwrap().unwrap();
    let rim = holed.fillet(&[[10., 14., 20.]], 0.1, 1.).unwrap();
    let record = rim.to_native().unwrap();
    assert!(
        record
            .faces
            .iter()
            .any(|f| matches!(&f.surface, b::Surface::RationalSpline { homogeneous, .. } if homogeneous.first() == homogeneous.last())),
        "the fillet is a spline closed in u"
    );
    record.check_geometry().unwrap();
    let body = crate::native::body_from_smlib(&rim).unwrap();
    let mesh = body.tessellate(0.02).unwrap();
    for f in 0..body.face_count() {
        assert!(mesh.tri_face.iter().any(|&t| t as usize == f), "face {f} meshes");
    }
    let smlib = rim.mass_properties(solidcraft_smlib::MEASURE_ACCURACY).unwrap().volume;
    let ours = crate::measure(&body).unwrap().volume;
    assert!((ours - smlib).abs() < 1e-3 * smlib, "decoded volume {ours}, SMLib's {smlib}");
}

/// SMLib's sphere cut by a box keeps its meridian seam from pole to pole: whole beside a cap
/// cut away from it, split where the cap crosses it. Decoded, each body is oriented as its
/// record says (never by truck's coarse mesh, which leaves the seam-bounded sphere out and
/// turned one inside out), the sphere keeps its wrap on its seam and every face meshes: the
/// volume is the sphere's less the cap, within 1e-3 (it came out −V, or short by the sphere).
#[cfg(feature = "smlib")]
#[test]
fn smlib_spheres_cut_beside_or_across_their_seam_decode_with_their_volume() {
    use solidcraft_smlib::{BooleanOp, Brep as S};
    use std::f64::consts::PI;
    let (r, h) = (5., 2.);
    let exact = 4. / 3. * PI * r * r * r - PI * h * h * (3. * r - h) / 3.;
    // The seam is the meridian on +x: a cap on −x or +y leaves it whole, one on +x splits it.
    let caps =
        [("−x", [-10., -10., -10.], [h - r, 10., 10.]), ("+x", [r - h, -10., -10.], [10., 10., 10.]), ("+y", [-10., r - h, -10.], [10., 10., 10.])];
    for (name, lo, hi) in caps {
        let ball = S::make_sphere([0.; 3], r).unwrap();
        let tool = S::make_box(lo, [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]).unwrap();
        let cut = ball.boolean(&tool, BooleanOp::Subtract).unwrap().unwrap();
        let record = cut.to_native().unwrap();
        record.check_geometry().unwrap();
        crate::body::ORIENTED.with(|n| n.set(0));
        let body = crate::native::body_from_smlib(&cut).unwrap();
        assert_eq!(crate::body::ORIENTED.with(|n| n.get()), 0, "{name}: the decode meshed the body to orient it");
        let forward: Vec<bool> = record
            .shells
            .iter()
            .flat_map(|sh| match sh {
                b::Shell::Faces { faces, .. } => faces.iter().map(|f| f.forward).collect(),
                _ => Vec::new(),
            })
            .collect();
        let loaded: Vec<bool> = body.solid.boundaries().iter().flat_map(|sh| sh.face_iter().map(|f| f.orientation()).collect::<Vec<_>>()).collect();
        assert_eq!(loaded, forward, "{name}: the body was turned inside out");
        let mesh = body.tessellate(0.02).unwrap();
        for f in 0..body.face_count() {
            assert!(mesh.tri_face.iter().any(|&t| t as usize == f), "{name}: face {f} meshes");
        }
        let ours = crate::measure(&body).unwrap().volume;
        assert!((ours - exact).abs() < 1e-3 * exact, "{name}: decoded volume {ours}, exact {exact}");
    }
    // A bar bored into the sphere's side away from its seam: truck's turning of revolved faces
    // away from their wrap ran the wrap through the hole, and the face meshed as nothing. The
    // volume the bar takes, by midpoint rule over its disc of the chord inside the ball.
    let (c, big, y0, z0, x0, x1, rb) = ([25., 17.5, 2.5], 10., 22.5, 10., 12.5, 30., 2.5);
    let n = 400;
    let mut taken = 0.;
    for i in 0..n {
        let rho = rb * (f64::from(i) + 0.5) / f64::from(n);
        for j in 0..n {
            let t = 2. * PI * (f64::from(j) + 0.5) / f64::from(n);
            let (y, z) = (y0 + rho * t.cos(), z0 + rho * t.sin());
            let d = big * big - (y - c[1]).powi(2) - (z - c[2]).powi(2);
            if d > 0. {
                taken += ((c[0] + d.sqrt()).min(x1) - (c[0] - d.sqrt()).max(x0)).max(0.) * rho;
            }
        }
    }
    let exact = 4. / 3. * PI * big.powi(3) - taken * rb / f64::from(n) * 2. * PI / f64::from(n);
    let ball = S::make_sphere(c, big).unwrap();
    let bar = crate::cylinder(Vec3::new(x0, y0, z0), Vec3::X, rb, x1 - x0).unwrap();
    let cut = ball.boolean(&crate::native::smlib_body(&bar).unwrap(), BooleanOp::Subtract).unwrap().unwrap();
    let ours = crate::measure(&crate::native::body_from_smlib(&cut).unwrap()).unwrap().volume;
    assert!((ours - exact).abs() < 1e-3 * exact, "bored: decoded volume {ours}, exact {exact}");
}

/// SMLib's torus is one face bounded by its two seam circles, each used both ways: the display
/// plan treats the closed profile as periodic, so the torus meshes and, beside a sphere bounded
/// by its seam alone (which only the plan meshes), the body keeps both (the sphere was lost).
/// The decode never meshes the body (it took half a second, meshing a torus to orient it).
#[cfg(feature = "smlib")]
#[test]
fn smlib_torus_decodes_without_meshing_and_plans_with_a_sphere_beside_it() {
    use solidcraft_smlib::{BooleanOp, Brep as S};
    use std::f64::consts::PI;
    let ring = S::make_torus([0., 30., 15.], 7.5, 2.5).unwrap();
    let ball = S::make_sphere([2.5, 15., 7.5], 2.5).unwrap();
    let (torus, sphere) = (2. * PI * PI * 7.5 * 2.5 * 2.5, 4. / 3. * PI * 2.5 * 2.5 * 2.5);
    let both = ring.boolean(&ball, BooleanOp::Union).unwrap().unwrap();
    for (name, s, exact) in [("torus", ring, torus), ("torus and sphere", both, torus + sphere)] {
        crate::body::ORIENTED.with(|n| n.set(0));
        let body = crate::native::body_from_smlib(&s).unwrap();
        assert_eq!(crate::body::ORIENTED.with(|n| n.get()), 0, "{name}: the decode meshed the body to orient it");
        let plan = body.display_plan(0.05).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(plan.faces.len(), body.face_count(), "{name}");
        let ours = crate::measure(&body).unwrap().volume;
        assert!((ours - exact).abs() < 1e-3 * exact, "{name}: decoded volume {ours}, exact {exact}");
    }
}

/// SMLib's intersection of a block and a cylinder, both sent as truck's splines, decodes with
/// the cylinder's volume inside the block, π r² h. (SMLib used to bound the top disc with spline
/// edges on part of their curves, which the decode refused; since the fork keeps a split face's
/// surface parameterization it no longer does, and
/// `spline_edges_on_part_of_their_curve_load_as_that_piece` covers that decode directly.)
#[cfg(feature = "smlib")]
#[test]
fn smlib_intersection_of_truck_splines_decodes_with_its_volume() {
    use solidcraft_smlib::BooleanOp;
    let block = crate::box_solid(Vec3::new(7.5, 15., 0.), Vec3::new(25., 32.5, 27.5)).unwrap();
    let post = crate::cylinder(Vec3::new(12.5, 20., 10.), Vec3::Z, 5., 30.).unwrap();
    // Both as truck holds them, the post a rational spline and the block's sides bilinear
    // patches (the import otherwise sends SMLib's cylinder and planes, whose cut leaves circles).
    let keeping = |body: &Body| solidcraft_smlib::Brep::from_record_keeping_splines(&crate::brep_record(body).unwrap(), true).unwrap();
    let common = keeping(&block).boolean(&keeping(&post), BooleanOp::Intersect).unwrap().unwrap();
    let body = crate::native::body_from_smlib(&common).unwrap();
    let exact = std::f64::consts::PI * 25. * 17.5;
    let ours = crate::measure(&body).unwrap().volume;
    assert!((ours - exact).abs() < 1e-3 * exact, "decoded volume {ours}, exact {exact}");
}
