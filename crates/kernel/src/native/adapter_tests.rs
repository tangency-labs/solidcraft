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
    // A spline edge on part of its curve would need truck to cut it: refused, not refitted.
    let spline = b::Curve::Spline { knots: vec![0., 0., 1., 1.], points: vec![[0.; 3], [1.; 3]] };
    assert!(load_edge_curve(spline.clone(), [0., 0.5]).is_err());
    assert!(load_edge_curve(spline, [0., 1.]).is_ok());
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
        let surface = load_surface(record, [[0., TAU], height]).unwrap();
        assert!(surface.subs(0., parameter).distance(p(expected)) < 1e-10);
        assert!(surface.normal(0., parameter).normalize().dot(v(normal).normalize()) > 1. - 1e-10);
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
        let record = encode(&body, super::super::pcurve::pcurves).unwrap_or_else(|e| panic!("{name}: {e}"));
        same_indices(&body, &record);
        let decoded = decode(record.clone()).unwrap();
        let again = Body::new(decoded).unwrap();
        let record2 = encode(&again, super::super::pcurve::pcurves).unwrap();
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
    let record = encode(&cube, super::super::pcurve::pcurves).unwrap();
    let mut wire = record.clone();
    wire.shells.push(b::Shell::Vertex { vertex: 0 });
    assert!(decode(wire).is_err());
    let mut vertex_loop = record;
    vertex_loop.faces[0].loops.push(b::Loop::Vertex { vertex: 0 });
    assert!(decode(vertex_loop).is_err());
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
        let record = encode(&body, super::super::pcurve::pcurves).unwrap();
        for (f, face) in record.faces.iter().zip(body.solid.face_iter()) {
            let surface = face.surface();
            for l in &f.loops {
                let b::Loop::Edges { edges } = l else { continue };
                for u in edges {
                    let e = &record.edges[u.edge as usize];
                    let curve = load_edge_curve(e.curve.clone(), e.interval).unwrap();
                    for s in [0., 0.5, 1.] {
                        let t = e.interval[0] + s * (e.interval[1] - e.interval[0]);
                        let [x, y] = super::super::pcurve::eval(&u.pcurve, t).unwrap();
                        let d = surface.subs(x, y).distance(curve.subs(t));
                        assert!(d <= e.tolerance, "{name}: edge {} at {s}: {d} mm off (forward face {})", u.edge, face.orientation());
                    }
                }
            }
        }
    }
}
