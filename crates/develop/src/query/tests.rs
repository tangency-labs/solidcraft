use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_ops::pcurve;
use solidcore_record::brep::*;

use super::*;

fn segment(a: f64, b: f64) -> Curve2 {
    Curve2::Spline { knots: vec![a, a, b, b], points: vec![[a, 0.], [b, 1.]] }
}
fn a_frame() -> Frame {
    Frame { origin: [1., 2., 3.], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] }
}

/// Every edge's uses as the canonical walk meets them (faces are all forward here).
fn list_uses(b: &mut Brep) {
    for e in &mut b.edges {
        e.uses.clear();
    }
    for (fi, f) in b.faces.iter().enumerate() {
        for (li, l) in f.loops.iter().enumerate() {
            if let Loop::Edges { edges } = l {
                for (p, u) in edges.iter().enumerate() {
                    let at = EdgeUseRef { face: fi as u32, r#loop: li as u32, position: p as u32, upper: !u.forward };
                    b.edges[u.edge as usize].uses.push(at);
                }
            }
        }
    }
}

/// Every analytic face and conic edge on its NURBS form, as producers make them: a face's form
/// over its stated domain, a circle about a face's axis labelled on its frame (`form::edge`).
fn on_forms(b: &mut Brep) {
    let frames: Vec<Frame> = b.faces.iter().filter(|f| f.nurbs.is_none()).filter_map(|f| form::frame_of(&f.surface)).collect();
    for f in &mut b.faces {
        if f.nurbs.is_none()
            && let Some((n, d)) = form::surface(&f.surface, f.domain)
        {
            (f.nurbs, f.domain) = (Some(n), d);
        }
    }
    for e in &mut b.edges {
        if e.nurbs.is_none()
            && let Some((label, _, Some((n, iv)))) = form::edge(&e.curve, e.interval, &frames, b.tolerance)
        {
            (e.curve, e.nurbs, e.interval) = (label, Some(n), iv);
        }
    }
}

/// Every oriented edge's pcurve from its face's surface and its edge (`pcurve::pcurves`).
fn fit_pcurves(b: &mut Brep) {
    for f in &mut b.faces {
        let loops: Vec<Vec<pcurve::EdgeUse<'_>>> = f
            .loops
            .iter()
            .map(|l| match l {
                Loop::Edges { edges } => edges
                    .iter()
                    .map(|u| {
                        let e = &b.edges[u.edge as usize];
                        pcurve::EdgeUse {
                            edge: u.edge,
                            curve: e.parameters(),
                            interval: e.interval,
                            forward: u.forward,
                            tolerance: e.tolerance,
                            given: None,
                        }
                    })
                    .collect(),
                Loop::Vertex { .. } => vec![],
            })
            .collect();
        let fitted = pcurve::pcurves_within(f.parameters(), &loops, 1e-7, Some(f.domain)).unwrap();
        for (l, pcs) in f.loops.iter_mut().zip(fitted) {
            if let Loop::Edges { edges } = l {
                for (u, pc) in edges.iter_mut().zip(pcs) {
                    u.pcurve = pc;
                }
            }
        }
    }
}

/// Planar faces, one per vertex cycle, grouped into shells; edges and vertices numbered in
/// canonical order as the cycles first meet them.
fn faceted(points: &[[f64; 3]], shells: &[&[&[usize]]], regions: Vec<Region>) -> Brep {
    let mut b = Brep { version: VERSION, tolerance: 1e-7, regions, shells: Vec::new(), faces: Vec::new(), edges: Vec::new(), vertices: Vec::new() };
    let mut vertex_of = vec![None; points.len()];
    let mut vertex = |b: &mut Brep, p: usize| -> u32 {
        *vertex_of[p].get_or_insert_with(|| {
            b.vertices.push(Vertex { point: points[p], tolerance: 1e-7 });
            b.vertices.len() as u32 - 1
        })
    };
    let mut by_pair = std::collections::HashMap::new();
    for cycles in shells {
        let mut uses = Vec::new();
        for cycle in cycles.iter() {
            let mut loop_ = Vec::new();
            for k in 0..cycle.len() {
                let (a, z) = (cycle[k], cycle[(k + 1) % cycle.len()]);
                let (edge, forward) = match by_pair.get(&(z, a)) {
                    Some(&e) => (e, false),
                    None => {
                        let vs = [vertex(&mut b, a), vertex(&mut b, z)];
                        b.edges.push(Edge {
                            nurbs: None,
                            curve: Curve::Line { start: points[a], end: points[z] },
                            interval: [0., 1.],
                            vertices: vs,
                            tolerance: 1e-7,
                            uses: vec![],
                        });
                        let e = b.edges.len() as u32 - 1;
                        by_pair.insert((a, z), e);
                        (e, true)
                    }
                };
                loop_.push(OrientedEdge { edge, forward, pcurve: segment(0., 1.) });
            }
            uses.push(OrientedFace { face: b.faces.len() as u32, forward: true });
            // The plane through the cycle's first three points: its natural normal is the cycle's.
            let [o, p, q] = [0, 1, 2].map(|k| points[cycle[k]]);
            let axis = |a: [f64; 3]| [a[0] - o[0], a[1] - o[1], a[2] - o[2]];
            let surface = Surface::Plane { origin: o, u_axis: axis(p), v_axis: axis(q) };
            b.faces.push(Face { nurbs: None, surface, domain: [[-10., 10.], [-10., 10.]], loops: vec![Loop::Edges { edges: loop_ }] });
        }
        b.shells.push(Shell::Faces { faces: uses, wires: vec![] });
    }
    list_uses(&mut b);
    fit_pcurves(&mut b);
    b
}

fn infinite(shells: Vec<u32>) -> Region {
    Region { shells, void: false }
}
const TETRA: [[f64; 3]; 4] = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const TETRA_FACES: [&[usize]; 4] = [&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[0, 3, 2]];
fn tetra() -> Brep {
    faceted(&TETRA, &[&TETRA_FACES], vec![infinite(vec![0]), Region { shells: vec![0], void: false }])
}
/// The side of a cylinder: one face bounded by two circles and a seam edge used twice.
fn tube() -> Brep {
    let f = a_frame();
    let circle = |z: f64| Curve::Circle { frame: Frame { origin: [1., 2., 3. + z], ..f.clone() }, radius: 2. };
    let edge = |curve, interval, vertices| Edge { nurbs: None, curve, interval, vertices, tolerance: 1e-7, uses: vec![] };
    let mut b = Brep {
        version: VERSION,
        tolerance: 1e-7,
        regions: vec![infinite(vec![0])],
        shells: vec![Shell::Faces { faces: vec![OrientedFace { face: 0, forward: true }], wires: vec![] }],
        faces: vec![Face {
            nurbs: None,
            surface: Surface::Cylinder { frame: f.clone(), radius: 2. },
            domain: [[0., TAU], [0., 5.]],
            loops: vec![Loop::Edges {
                edges: vec![
                    OrientedEdge { edge: 0, forward: true, pcurve: segment(0., TAU) },
                    OrientedEdge { edge: 1, forward: true, pcurve: segment(0., 1.) },
                    OrientedEdge { edge: 2, forward: false, pcurve: segment(0., TAU) },
                    OrientedEdge { edge: 1, forward: false, pcurve: segment(0., 1.) },
                ],
            }],
        }],
        edges: vec![
            edge(circle(0.), [0., TAU], [0, 0]),
            edge(Curve::Line { start: [3., 2., 3.], end: [3., 2., 8.] }, [0., 1.], [0, 1]),
            edge(circle(5.), [0., TAU], [1, 1]),
        ],
        vertices: vec![Vertex { point: [3., 2., 3.], tolerance: 1e-7 }, Vertex { point: [3., 2., 8.], tolerance: 1e-7 }],
    };
    list_uses(&mut b);
    on_forms(&mut b);
    fit_pcurves(&mut b);
    b
}
/// One face on `surface` bounded by one loop of `uses` (edge index, forward), each edge's
/// pcurves fitted, as an open sheet.
fn one_face(surface: Surface, domain: [[f64; 2]; 2], edges: Vec<(Curve, [f64; 2], [u32; 2])>, vertices: &[[f64; 3]], uses: &[(u32, bool)]) -> Brep {
    let mut b = Brep {
        version: VERSION,
        tolerance: 1e-7,
        regions: vec![infinite(vec![0])],
        shells: vec![Shell::Faces { faces: vec![OrientedFace { face: 0, forward: true }], wires: vec![] }],
        faces: vec![Face {
            nurbs: None,
            surface,
            domain,
            loops: vec![Loop::Edges { edges: uses.iter().map(|&(edge, forward)| OrientedEdge { edge, forward, pcurve: segment(0., 1.) }).collect() }],
        }],
        edges: edges
            .into_iter()
            .map(|(curve, interval, vertices)| Edge { nurbs: None, curve, interval, vertices, tolerance: 1e-7, uses: vec![] })
            .collect(),
        vertices: vertices.iter().map(|&point| Vertex { point, tolerance: 1e-7 }).collect(),
    };
    list_uses(&mut b);
    on_forms(&mut b);
    fit_pcurves(&mut b);
    b
}
/// A ring of a torus on its NURBS form, closed in u (first and last control rows the same), as
/// SMLib holds the fillet round a hole's rim: bounded by its two circles and the tube's quarter
/// at u = 0, a seam used twice, each on its form.
fn closed_band() -> Brep {
    let (big, small) = (5., 1.);
    let torus = Surface::Torus {
        frame: Frame { origin: [0., 0., 0.], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] },
        major_radius: big,
        minor_radius: small,
        tube_start: 0.0,
    };
    let about = |z: f64| Frame { origin: [0., 0., z], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let quarter = Curve::Circle { frame: Frame { origin: [big, 0., 0.], x_axis: [1., 0., 0.], y_axis: [0., 0., 1.] }, radius: small };
    one_face(
        torus,
        [[0., TAU], [0., FRAC_PI_2]],
        vec![
            (Curve::Circle { frame: about(0.), radius: big + small }, [0., TAU], [0, 0]),
            (quarter, [0., FRAC_PI_2], [0, 1]),
            (Curve::Circle { frame: about(small), radius: big }, [0., TAU], [1, 1]),
        ],
        &[[big + small, 0., 0.], [big, 0., small]],
        &[(0, true), (1, true), (2, false), (1, false)],
    )
}

/// Exact queries on a record: a tetrahedron's faces, one point on each, its outward normals, and
/// rays from inside it meeting the faces they should at the distances they should.
#[test]
fn queries_find_a_tetrahedrons_faces_from_its_record() {
    let b = tetra();
    let q = Query::new(&b);
    let third = 1.0 / 3.0;
    let middles = [[third, third, 0.], [third, 0., third], [third, third, third], [0., third, third]];
    for (f, m) in middles.iter().enumerate() {
        let p = Vec3::new(m[0], m[1], m[2]);
        assert!(q.face_contains(f, p, 1e-9), "face {f} at {p:?}");
        assert_eq!(q.faces_at(p, 1e-9).first().map(|x| x.1), Some(f));
    }
    // Off a face's outline, in its plane: not on it.
    assert!(!q.face_contains(0, Vec3::new(0.9, 0.9, 0.), 1e-9));
    // The floor's normal points down, out of the solid.
    let uv = q.face_uv(0, Vec3::new(0.2, 0.2, 0.), 1e-9).unwrap_or_default();
    let n = q.face_normal(0, uv).unwrap_or_default();
    assert!((n - Vec3::new(0., 0., -1.)).len() < 1e-12, "{n:?}");
    // From inside: down to the floor 0.1 away, and along x to the slanted face x + y + z = 1.
    let o = Vec3::new(0.1, 0.1, 0.1);
    let down = q.ray(o, Vec3::new(0., 0., -1.), 1e-9).unwrap();
    assert!(down.first().is_some_and(|h| h.1 == 0 && (h.0 - 0.1).abs() < 1e-12), "{down:?}");
    let along = q.ray(o, Vec3::X, 1e-9).unwrap();
    assert!(along.first().is_some_and(|h| h.1 == 2 && (h.0 - 0.7).abs() < 1e-12), "{along:?}");
}

/// A tube's side on its NURBS form, as SMLib holds it: its circles drawn within the sag asked
/// for, a line drawn as its ends, the distance from the axis to its seam, points on it and off
/// its ends, and a ray from the axis meeting it at its radius.
#[test]
fn queries_read_a_tube_side_on_its_form() {
    let b = tube();
    let q = Query::new(&b);
    let axis = Vec3::new(1., 2., 0.);
    let off_axis = |p: Vec3| Vec3::new(p.x - axis.x, p.y - axis.y, 0.).len();
    let circle = q.edge_points(0, 1e-4).unwrap_or_default();
    assert!(circle.len() > 20 && circle.iter().all(|(_, p)| (off_axis(*p) - 2.0).abs() < 1e-9), "{}", circle.len());
    assert_eq!(q.edge_points(1, 1e-4).map(|v| v.len()), Some(2));
    let d = q.edge_distance(1, Vec3::new(1., 2., 5.5), 1e-4).unwrap_or(f64::NAN);
    assert!((d - 2.0).abs() < 1e-12, "{d}");
    assert!(q.face_contains(0, Vec3::new(1., 4., 5.5), 1e-9));
    assert!(q.face_contains(0, Vec3::new(3., 2., 5.5), 1e-9), "on the seam");
    assert!(!q.face_contains(0, Vec3::new(1., 4., 9.), 1e-9), "past the top circle");
    let hit = q.ray(Vec3::new(1., 2., 5.5), Vec3::Y, 1e-9).unwrap();
    assert!(hit.first().is_some_and(|h| h.1 == 0 && (h.0 - 2.0).abs() < 1e-12), "{hit:?}");
}

/// A face's trims are read on their pcurves, not on polygons drawn through them. A disc of radius
/// 100 on the plane z = 0 is bounded by one circle: every point 10⁻⁶ inside it, all the way round,
/// is on the face, and every point 10⁻⁶ outside it is not at an acceptance of 10⁻⁹, but is at
/// 10⁻⁵, which reaches the circle. A 64-gon drawn through the circle lies up to
/// 100 (1 − cos(π/64)) = 0.1205 inside it between its corners, so it put nearly all of the
/// points inside off the face.
#[test]
fn a_point_a_hair_inside_a_round_trim_is_on_its_face() {
    let frame = Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let b = one_face(
        Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
        [[-200., 200.], [-200., 200.]],
        vec![(Curve::Circle { frame, radius: 100. }, [0., TAU], [0, 0])],
        &[[100., 0., 0.]],
        &[(0, true)],
    );
    let q = Query::new(&b);
    assert!(q.face_contains(0, Vec3::ZERO, 1e-9), "the center");
    for k in 0..360 {
        let (s, c) = f64::from(k).to_radians().sin_cos();
        let at = |r: f64| Vec3::new(r * c, r * s, 0.);
        assert!(q.face_contains(0, at(100. - 1e-6), 1e-9), "{k}°: a hair inside");
        assert!(!q.face_contains(0, at(100. + 1e-6), 1e-9), "{k}°: a hair outside");
        assert!(q.face_contains(0, at(100. + 1e-6), 1e-5), "{k}°: outside within the acceptance");
    }
}

/// A face's boundary is measured in space, not in its parameters. The plane whose u axis is 1000
/// mm long and v axis 1 mm, bounded by the square 0..1 of both, is a 1000 × 1 mm strip. A point
/// 0.5 mm past its far end is 0.0005 past it in u: off the face at an acceptance of 0.001 mm,
/// where the parameters read through the slower axis (1 mm a unit) put it within reach. Points
/// 0.0005 mm past the far end or past a long edge are on it at 0.001 and off it at 0.0001.
#[test]
fn a_faces_boundary_is_measured_in_space() {
    let corners = [[0., 0., 0.], [1000., 0., 0.], [1000., 1., 0.], [0., 1., 0.]];
    let side = |i: usize| (Curve::Line { start: corners[i], end: corners[(i + 1) % 4] }, [0., 1.], [i as u32, ((i + 1) % 4) as u32]);
    let b = one_face(
        Surface::Plane { origin: [0.; 3], u_axis: [1000., 0., 0.], v_axis: [0., 1., 0.] },
        [[-1., 2.], [-1., 2.]],
        (0..4).map(side).collect(),
        &corners,
        &[(0, true), (1, true), (2, true), (3, true)],
    );
    let q = Query::new(&b);
    assert!(q.face_contains(0, Vec3::new(500., 0.5, 0.), 1e-9), "the middle");
    assert!(!q.face_contains(0, Vec3::new(1000.5, 0.5, 0.), 1e-3), "0.5 mm past the far end");
    assert!(q.face_contains(0, Vec3::new(1000.0005, 0.5, 0.), 1e-3), "0.0005 mm past the far end");
    assert!(!q.face_contains(0, Vec3::new(1000.0005, 0.5, 0.), 1e-4), "0.0005 mm past the far end");
    assert!(q.face_contains(0, Vec3::new(500., 1.0005, 0.), 1e-3), "0.0005 mm past a long edge");
    assert!(!q.face_contains(0, Vec3::new(500., 1.0005, 0.), 1e-4), "0.0005 mm past a long edge");
}

/// A ray is not met with a torus face exactly yet, so it names the face instead of an answer that
/// would leave the face out.
#[test]
fn a_ray_names_a_face_it_cannot_meet_exactly() {
    let b = closed_band();
    assert_eq!(Query::new(&b).ray(Vec3::new(0., 0., 0.5), Vec3::X, 1e-9), Err(0));
}

/// A ball patch lies on its sphere, so a ray meets it where it meets the sphere: from
/// (−5, 2, 3) along x, the sphere of radius 2 about (1, 2, 3) is met 6 ∓ 2 along, at 4 and 8
/// (s² − 12 s + 32 = 0). A torus is not met exactly yet: no answer, rather than none met.
#[test]
fn a_ray_meets_a_ball_patch_on_its_sphere_and_refuses_a_torus() {
    let ball = Surface::BallPatch { center: [1., 2., 3.], radius: 2. };
    let mut met = ray_surface(&ball, Vec3::new(-5., 2., 3.), Vec3::X).unwrap_or_default();
    met.sort_by(f64::total_cmp);
    assert_eq!(met, vec![4., 8.]);
    let frame = Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let torus = Surface::Torus { frame, major_radius: 5., minor_radius: 1., tube_start: 0. };
    assert_eq!(ray_surface(&torus, Vec3::new(-10., 0., 0.), Vec3::X), None);
}

/// A trim piece of a degree no profile segment holds (a line elevated to degree 14, as a fold's
/// pcurves come) is read on the piece itself: the line from (0, 0) to (2, 2) crosses y = 1 at
/// x = 1, so the ray along +x from (0.5, 1) crosses it once and the one from (1.5, 1) not at all;
/// from (1, 1), on the piece, the halvings never settle. The same line with weights 1, 2, 1 ... (a
/// rational piece) is the same line, read the same way.
#[test]
fn a_trim_piece_past_degree_nine_is_read_on_itself() {
    let line = |w: &dyn Fn(usize) -> f64| -> Vec<HPoint> {
        (0..=14)
            .map(|i| {
                let s = i as f64 / 14.0;
                HPoint::weighted(Vec3::new(2.0 * s, 2.0 * s, 0.0), w(i))
            })
            .collect()
    };
    for ctrl in [line(&|_| 1.0), line(&|i| if i % 2 == 0 { 1.0 } else { 2.0 })] {
        assert_eq!(ray_crossings(&ctrl, Vec2::new(0.5, 1.0), MAX_HALVINGS), Some(1));
        assert_eq!(ray_crossings(&ctrl, Vec2::new(1.5, 1.0), MAX_HALVINGS), Some(0));
        assert_eq!(ray_crossings(&ctrl, Vec2::new(0.5, 3.0), MAX_HALVINGS), Some(0));
        assert_eq!(ray_crossings(&ctrl, Vec2::new(1.0, 1.0), MAX_HALVINGS), None);
    }
}
