use super::*;
use solidcraft_geom::brep::{
    Brep as Record, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace, Region, Shell, Surface, Vertex,
};

/// A closed polyhedron record from its corner points and its faces, each a cycle of corner
/// indices counter-clockwise seen from outside: planar faces, straight edges, canonical order.
fn polyhedron(corners: &[[f64; 3]], cycles: &[&[usize]]) -> Record {
    let unit = |a: [f64; 3]| {
        let n = dot(a, a).sqrt();
        scale(a, 1.0 / n)
    };
    let mut edge_of: HashMap<(usize, usize), u32> = HashMap::new();
    let mut ends: Vec<(usize, usize)> = Vec::new();
    let mut uses: Vec<Vec<EdgeUseRef>> = Vec::new();
    let mut faces = Vec::new();
    for (fi, cycle) in cycles.iter().enumerate() {
        let o = corners[cycle[0]];
        let x = unit(sub(corners[cycle[1]], o));
        let z = unit(cross(x, sub(corners[cycle[cycle.len() - 1]], o)));
        let y = cross(z, x);
        let uv = |p: [f64; 3]| [dot(sub(p, o), x), dot(sub(p, o), y)];
        let mut edges = Vec::new();
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for k in 0..cycle.len() {
            let (a, b) = (cycle[k], cycle[(k + 1) % cycle.len()]);
            let (e, forward) = match edge_of.get(&(b, a)) {
                Some(&e) => (e, false),
                None => {
                    let e = ends.len() as u32;
                    edge_of.insert((a, b), e);
                    ends.push((a, b));
                    uses.push(Vec::new());
                    (e, true)
                }
            };
            uses[e as usize].push(EdgeUseRef { face: fi as u32, r#loop: 0, position: k as u32, upper: !forward });
            let (s, t) = ends[e as usize];
            for p in [uv(corners[s]), uv(corners[t])] {
                for i in 0..2 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
            edges.push(OrientedEdge {
                edge: e,
                forward,
                pcurve: Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![uv(corners[s]), uv(corners[t])] },
            });
        }
        faces.push(Face {
            nurbs: None,
            surface: Surface::Plane { origin: o, u_axis: x, v_axis: y },
            domain: [[lo[0], hi[0]], [lo[1], hi[1]]],
            loops: vec![Loop::Edges { edges }],
        });
    }
    // Vertices in first appearance along the edges.
    let mut vertex_of: HashMap<usize, u32> = HashMap::new();
    let mut vertices = Vec::new();
    let mut number = |c: usize, vertices: &mut Vec<Vertex>| {
        *vertex_of.entry(c).or_insert_with(|| {
            vertices.push(Vertex { point: corners[c], tolerance: 1e-6 });
            vertices.len() as u32 - 1
        })
    };
    let edges = ends
        .iter()
        .zip(uses)
        .map(|(&(s, t), uses)| {
            let vs = [number(s, &mut vertices), number(t, &mut vertices)];
            Edge { nurbs: None, curve: Curve::Line { start: corners[s], end: corners[t] }, interval: [0.0, 1.0], vertices: vs, tolerance: 1e-6, uses }
        })
        .collect();
    Record {
        version: solidcraft_geom::brep::VERSION,
        tolerance: 1e-6,
        regions: vec![Region { shells: vec![0], void: false }, Region { shells: vec![0], void: false }],
        shells: vec![Shell::Faces { faces: (0..cycles.len() as u32).map(|face| OrientedFace { face, forward: true }).collect(), wires: vec![] }],
        faces,
        edges,
        vertices,
    }
}

fn cube(s: f64) -> Record {
    let c = [[0., 0., 0.], [s, 0., 0.], [s, s, 0.], [0., s, 0.], [0., 0., s], [s, 0., s], [s, s, s], [0., s, s]];
    polyhedron(&c, &[&[0, 3, 2, 1], &[4, 5, 6, 7], &[0, 1, 5, 4], &[1, 2, 6, 5], &[2, 3, 7, 6], &[3, 0, 4, 7]])
}

#[test]
fn a_cube_record_is_valid_and_maps_to_two_shells() {
    let r = cube(10.0);
    r.validate().unwrap();
    let a = arrays(&r, true).unwrap();
    // The infinite region (a void in SMLib) and the solid, one shell each, every face once per side.
    assert_eq!(a.ints[&flat::REGION_SHELL_COUNT], vec![1, 1]);
    assert_eq!(a.ints[&flat::REGION_TYPE], vec![1, 0]);
    assert_eq!(a.ints[&flat::SHELL_FACEUSE_COUNT], vec![6, 6]);
    assert_eq!(a.ints[&flat::FACEUSE_ORIENTATION], [vec![1; 6], vec![0; 6]].concat());
    assert_eq!(a.ints[&flat::EDGE_CURVE_TYPE], vec![i64::from(flat::CURVE_LINE); 12]);
    assert_eq!(a.ints[&flat::FACE_SURFACE_TYPE], vec![i64::from(flat::SURFACE_PLANE); 6]);
    // Each edge's two uses cycle into each other; no trims on analytic faces.
    let next = &a.ints[&flat::EDGEUSE_NEXT_RADIAL];
    assert_eq!(next.len(), 24);
    assert!(next.iter().enumerate().all(|(i, &n)| n != i as i64 && next[n as usize] == i as i64));
    assert!(!a.ints.contains_key(&flat::EDGEUSE_NURBS_VERTEX_COUNT));
}

#[test]
fn records_smlib_cannot_take_are_errors() {
    let mut r = cube(1.0);
    r.version = 3;
    assert!(matches!(arrays(&r, true), Err(Unsupported::Invalid(_))));
    let mut r = cube(1.0);
    r.shells = vec![Shell::Vertex { vertex: 0 }];
    assert!(arrays(&r, true).is_err());
}

#[cfg(feature = "native")]
#[test]
fn a_cube_record_imports_into_smlib() {
    let b = crate::Brep::from_record(&cube(10.0), true).unwrap();
    assert!((b.volume(1e-7).unwrap() - 1000.0).abs() < 1e-6);
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    assert!(b.is_manifold_solid().unwrap());
}

#[cfg(feature = "native")]
#[test]
fn a_plane_on_other_axes_goes_as_smlibs_plane() {
    // A tetrahedron: its faces' natural frames are unit and orthogonal, so make one plane's axes
    // non-unit and check it still goes as SMLib's plane, the volume unchanged.
    let c = [[0., 0., 0.], [6., 0., 0.], [0., 6., 0.], [0., 0., 6.]];
    let mut r = polyhedron(&c, &[&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[2, 0, 3]]);
    if let Some(Face { nurbs: None, surface: Surface::Plane { u_axis, v_axis, .. }, domain, loops }) = r.faces.get_mut(0) {
        *u_axis = scale(*u_axis, 2.0);
        *v_axis = scale(*v_axis, 2.0);
        *domain = domain.map(|d| d.map(|x| x / 2.0));
        if let Some(Loop::Edges { edges }) = loops.get_mut(0) {
            for e in edges {
                if let Curve2::Spline { points, .. } = &mut e.pcurve {
                    points.iter_mut().for_each(|p| *p = p.map(|x| x / 2.0));
                }
            }
        }
    }
    r.validate().unwrap();
    let a = arrays(&r, true).unwrap();
    assert_eq!(a.ints[&flat::FACE_SURFACE_TYPE][0], i64::from(flat::SURFACE_PLANE));
    let b = crate::Brep::from_record(&r, true).unwrap();
    assert!((b.volume(1e-7).unwrap() - 36.0).abs() < 1e-6, "{:?}", b.volume(1e-7));
}

/// The surface `arrays` wrote for the first face, back as a schema 4 rational spline, with its range.
fn first_nurbs(a: &Arrays) -> (Surface, [f64; 4]) {
    let (nu, nv) = (a.ints[&flat::FACE_NURBS_U_VERTEX_COUNT][0] as usize, a.ints[&flat::FACE_NURBS_V_VERTEX_COUNT][0] as usize);
    let cvs = &a.reals[&flat::FACE_NURBS_CVS];
    let w = &a.reals[&flat::FACE_NURBS_WEIGHTS];
    let homogeneous = (0..nu)
        .map(|i| {
            (0..nv)
                .map(|j| {
                    let k = i * nv + j;
                    [cvs[3 * k] * w[k], cvs[3 * k + 1] * w[k], cvs[3 * k + 2] * w[k], w[k]]
                })
                .collect()
        })
        .collect();
    let r = &a.reals[&flat::FACE_RANGE];
    (
        Surface::RationalSpline {
            u_knots: a.reals[&flat::FACE_NURBS_U_KNOTS].clone(),
            v_knots: a.reals[&flat::FACE_NURBS_V_KNOTS].clone(),
            homogeneous,
        },
        [r[0], r[1], r[2], r[3]],
    )
}

#[test]
fn revolutions_become_the_same_surface_as_a_rational_spline() {
    use solidcraft_geom::brep::eval::SurfaceEval;
    // A quarter-circle profile (rational) away from the axis, so the surface is a torus piece.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let profile = Curve::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        homogeneous: vec![[3.0, 0.0, 0.0, 1.0], [3.0 * s, 0.0, 1.0 * s, s], [2.0, 0.0, 1.0, 1.0]],
    };
    let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let mirror = [[0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [1.0, -2.0, 3.0, 1.0]];
    for matrix in [identity, mirror] {
        for forward in [true, false] {
            let rev =
                Surface::PlacedRevolution { profile: Box::new(profile.clone()), origin: [0.0, 0.0, 0.0], axis: [0.0, 0.0, 1.0], matrix, forward };
            let (angles, prof) = ([0.4, 0.4 + 1.9 * std::f64::consts::PI], [0.0, 1.0]);
            let domain = if forward { [prof, angles] } else { [angles, prof] };
            let mut a = Arrays::default();
            surface(&mut a, &rev, domain, true).unwrap();
            let (nurbs, range) = first_nurbs(&a);
            let (re, ne) = (SurfaceEval::new(&rev).unwrap(), SurfaceEval::new(&nurbs).unwrap());
            let det = {
                let col = |c: usize| [matrix[c][0], matrix[c][1], matrix[c][2]];
                dot(cross(col(0), col(1)), col(2))
            };
            for i in 0..=6 {
                for j in 0..=6 {
                    let (sp, ang) = (i as f64 / 6.0, angles[0] + (angles[1] - angles[0]) * j as f64 / 6.0);
                    let uv = if forward { [sp, ang] } else { [ang, sp] };
                    let (p, du, dv) = re.derivs(uv[0], uv[1]).unwrap();
                    let n = du.cross(dv) * det.signum();
                    let t = circle_param(ang - angles[0]);
                    let profile_first = forward == (det > 0.0);
                    let q = if profile_first { [sp, t] } else { [t, sp] };
                    assert!(q[0] >= range[0] - 1e-12 && q[0] <= range[2] + 1e-12 && q[1] >= range[1] - 1e-12 && q[1] <= range[3] + 1e-12);
                    let (pn, nu_, nv_) = ne.derivs(q[0], q[1]).unwrap();
                    assert!((pn - p).len() < 1e-12, "forward {forward} det {det}: {p:?} vs {pn:?}");
                    assert!(nu_.cross(nv_).dot(n) > 0.0, "normal flipped: forward {forward} det {det}");
                }
            }
        }
    }
}

/// A circle or ellipse edge past a full turn (schema 4 allows up to one turn past the seam)
/// goes to SMLib a whole turn back, within [-2π, 2π]; other curves keep their interval.
#[test]
fn arcs_past_a_turn_come_a_turn_back() {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let frame = solidcraft_geom::brep::Frame { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] };
    let circle = Curve::Circle { frame, radius: 2.0 };
    assert_eq!(turned(&circle, [TAU, TAU + FRAC_PI_2]), ([0.0, FRAC_PI_2], -TAU));
    assert_eq!(turned(&circle, [-TAU - FRAC_PI_2, -TAU]), ([-FRAC_PI_2, 0.0], TAU));
    let (i, s) = turned(&circle, [5.0, 5.0 + TAU]);
    assert!(s == -TAU && i[0] >= -TAU && i[1] <= TAU, "{i:?}");
    assert_eq!(turned(&circle, [0.5, 2.0]), ([0.5, 2.0], 0.0));
    let line = Curve::Line { start: [0.0; 3], end: [1.0, 0.0, 0.0] };
    assert_eq!(turned(&line, [7.0, 9.0]), ([7.0, 9.0], 0.0));
}

/// A revolved circle is SMLib's ring torus only where its tube stays off the axis: a tube that
/// reaches it to rounding (a horn torus, its centre as far from the axis as its radius, give or
/// take an ulp) and one across it (a spindle) go as their rational B-splines.
#[test]
fn only_a_tube_clear_of_its_axis_is_a_ring_torus() {
    let ring = |major: f64| {
        let frame = solidcraft_geom::brep::Frame { origin: [major, 0.0, 0.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 0.0, 1.0] };
        let (profile, _) = solidcraft_geom::brep::form::curve(&Curve::Circle { frame, radius: 1.0 }, [-1.0, 1.0]).unwrap();
        let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let surface =
            Surface::PlacedRevolution { profile: Box::new(profile), origin: [0.0; 3], axis: [0.0, 0.0, 1.0], matrix: identity, forward: false };
        let face = Face { surface, nurbs: None, domain: [[0.0, 1.0], [-0.5, 0.5]], loops: Vec::new() };
        let record = Record {
            version: b::VERSION,
            tolerance: 1e-6,
            regions: Vec::new(),
            shells: Vec::new(),
            faces: vec![face],
            edges: Vec::new(),
            vertices: Vec::new(),
        };
        let made = analytic::analytic(&record).unwrap();
        made.faces[0].surface.clone()
    };
    assert!(matches!(ring(1.5), Surface::Torus { .. }));
    for major in [1.0, 1.0 + 4.0 * f64::EPSILON, 0.5] {
        assert!(matches!(ring(major), Surface::PlacedRevolution { .. }), "major {major}");
    }
}

/// A 6 mm wide patch of an extruded circle of radius 3000 whose trims straddle the knot where
/// the profile's seam lies (u = 4 of a half circle in two quarter spans): the cylinder takes the
/// patch's own range, 0.002 rad round and 7 high, on the circle exactly. Fitted on the patch's
/// range (three points 2 mm apart) the centre came off the circle's plane by 2.5e-10 mm and the
/// radius off by 8e-9 mm, a cylinder SMLib did not take back from its own NURBS of it.
#[test]
fn a_patch_across_an_extruded_circles_seam_is_its_exact_cylinder() {
    let (r, h, half) = (3000.0, 7.0, 0.001);
    let c = [10.0 - r, 0.0, 0.0];
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let profile = [([c[0], -r], 1.0), ([c[0] + r, -r], w), ([c[0] + r, 0.0], 1.0), ([c[0] + r, r], w), ([c[0], r], 1.0)];
    let homogeneous = profile.iter().map(|&([x, y], wt)| vec![[x * wt, y * wt, 0.0, wt], [x * wt, y * wt, h * wt, wt]]).collect();
    let surface = Surface::RationalSpline { u_knots: vec![3.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0, 5.0], v_knots: vec![0.0, 0.0, h, h], homogeneous };
    // The quarter span after the seam reaches `half` at quarter_param(half); the one before it
    // ends there, so the trims run symmetrically round the knot.
    let (u0, u1) = (4.0 - quarter_param(half), 4.0 + quarter_param(half));
    let corners = [[u0, 0.0], [u1, 0.0], [u1, h], [u0, h]];
    let edges: Vec<OrientedEdge> = (0..4)
        .map(|k| OrientedEdge {
            edge: k as u32,
            forward: true,
            pcurve: Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![corners[k], corners[(k + 1) % 4]] },
        })
        .collect();
    let line = Curve::Line { start: [0.0; 3], end: [1.0, 0.0, 0.0] };
    let record = Record {
        version: b::VERSION,
        tolerance: 1e-6,
        regions: Vec::new(),
        shells: Vec::new(),
        faces: vec![Face { surface, nurbs: None, domain: [[3.0, 5.0], [0.0, h]], loops: vec![Loop::Edges { edges }] }],
        edges: (0..4)
            .map(|_| Edge { nurbs: None, curve: line.clone(), interval: [0.0, 1.0], vertices: [0, 0], tolerance: 1e-6, uses: Vec::new() })
            .collect(),
        vertices: Vec::new(),
    };
    let made = analytic::analytic(&record).unwrap();
    let f = &made.faces[0];
    let Surface::Cylinder { frame, radius } = &f.surface else { panic!("{:?}", f.surface) };
    assert!((radius - r).abs() <= 1e-10, "radius {radius}");
    let z = cross(frame.x_axis, frame.y_axis);
    assert!(z[0].abs() <= 1e-15 && z[1].abs() <= 1e-15 && frame.x_axis[2].abs() <= 1e-15, "axis {z:?}, x {:?}", frame.x_axis);
    assert!(frame.origin[2].abs() <= 1e-12 && (frame.origin[0] - c[0]).abs() <= 1e-9 && frame.origin[1].abs() <= 1e-9, "centre {:?}", frame.origin);
    // u from the patch's first edge, 2 * half round; v its height.
    let [[a0, a1], [v0, v1]] = f.domain;
    assert!(a0 == 0.0 && (a1 - 2.0 * half).abs() <= 1e-12, "{:?}", f.domain);
    assert!(v0.abs() <= 1e-12 && (v1 - h).abs() <= 1e-12, "{:?}", f.domain);
    let start = (-half).sin_cos();
    assert!((frame.x_axis[0] - start.1).abs() <= 1e-12 && (frame.x_axis[1] - start.0).abs() <= 1e-12, "x {:?}", frame.x_axis);
}

/// A one-face record of `surface` trimmed by the rectangle `domain` (its four sides as pcurves,
/// on placeholder edges): what the import of a spline face reads its range from.
fn rectangle(surface: Surface, domain: [[f64; 2]; 2]) -> Record {
    let [[u0, u1], [v0, v1]] = domain;
    let corners = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let edges: Vec<OrientedEdge> = (0..4)
        .map(|k| OrientedEdge {
            edge: k as u32,
            forward: true,
            pcurve: Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![corners[k], corners[(k + 1) % 4]] },
        })
        .collect();
    let line = Curve::Line { start: [0.0; 3], end: [1.0, 0.0, 0.0] };
    Record {
        version: b::VERSION,
        tolerance: 1e-6,
        regions: Vec::new(),
        shells: Vec::new(),
        faces: vec![Face { surface, nurbs: None, domain, loops: vec![Loop::Edges { edges }] }],
        edges: (0..4)
            .map(|_| Edge { nurbs: None, curve: line.clone(), interval: [0.0, 1.0], vertices: [0, 0], tolerance: 1e-6, uses: Vec::new() })
            .collect(),
        vertices: Vec::new(),
    }
}

/// Every revolution truck makes for an analytic surface comes to SMLib as that surface, placed
/// off the world's axes and with the profile along either parameter, and a rational spline that
/// is an extruded circle as a cylinder: points of the record's surface over its domain lie on
/// the analytic surface, and the analytic surface over its range (but a plane's, the box round
/// the face) on the record's.
#[test]
fn revolutions_and_extruded_circles_become_their_analytic_surfaces() {
    use solidcraft_geom::brep::{Frame, eval::SurfaceEval, form};
    use std::f64::consts::{FRAC_PI_2, TAU};
    let line = |a: [f64; 3], z: [f64; 3]| (Curve::Line { start: a, end: z }, [0.0, 1.0]);
    let xz = |o: [f64; 3]| Frame { origin: o, x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 0.0, 1.0] };
    let arc = |o: [f64; 3], r: f64, t: [f64; 2]| form::curve(&Curve::Circle { frame: xz(o), radius: r }, t).unwrap();
    let profiles: Vec<(&str, (Curve, [f64; 2]))> = vec![
        ("plane", line([1.0, 0.0, 0.0], [3.0, 0.0, 0.0])),
        ("cylinder", line([2.0, 0.0, 0.0], [2.0, 0.0, 4.0])),
        ("cone", line([1.0, 0.0, 0.0], [3.0, 0.0, 4.0])),
        ("sphere", arc([0.0; 3], 2.0, [-FRAC_PI_2 + 0.2, FRAC_PI_2 - 0.2])),
        ("torus", arc([5.0, 0.0, 0.0], 1.0, [-1.0, 2.0])),
    ];
    // As they are; turned about (1, 1, 1) by 120° (x → y → z) and moved; mirrored.
    let placements = [
        [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        [[0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 0.0], [3.0, -1.0, 2.0, 1.0]],
        // Mirrored in x = 0: the revolution turns the other way about its placed axis.
        [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
    ];
    let single = |surface: Surface, domain: [[f64; 2]; 2]| Record {
        version: b::VERSION,
        tolerance: 1e-6,
        regions: Vec::new(),
        shells: Vec::new(),
        faces: vec![Face { surface, nurbs: None, domain, loops: Vec::new() }],
        edges: Vec::new(),
        vertices: Vec::new(),
    };
    let mut cases: Vec<(String, Surface, [[f64; 2]; 2])> = Vec::new();
    for (name, (profile, t)) in &profiles {
        for matrix in placements {
            for forward in [true, false] {
                let surface =
                    Surface::PlacedRevolution { profile: Box::new(profile.clone()), origin: [0.0; 3], axis: [0.0, 0.0, 1.0], matrix, forward };
                let angles = [0.3, 0.3 + 0.75 * TAU];
                let domain = if forward { [*t, angles] } else { [angles, *t] };
                cases.push((format!("{name}, forward {forward}, placed {:?}", matrix[3]), surface, domain));
            }
        }
    }
    let frame = Frame { origin: [1.0, 2.0, 3.0], x_axis: [0.0, 1.0, 0.0], y_axis: [0.0, 0.0, 1.0] };
    let (extruded, range) = form::surface(&Surface::Cylinder { frame, radius: 2.5 }, [[0.2, 2.0], [-1.0, 4.0]]).unwrap();
    assert!(matches!(extruded, Surface::RationalSpline { .. }));
    cases.push(("extruded circle".into(), extruded, range));
    for (name, surface, domain) in &cases {
        // A spline face's range comes from its trims; a revolution's from its domain.
        let record =
            if matches!(surface, Surface::RationalSpline { .. }) { rectangle(surface.clone(), *domain) } else { single(surface.clone(), *domain) };
        let made = analytic::analytic(&record).unwrap_or_else(|e| panic!("{name}: {e}"));
        let face = &made.faces[0];
        let kind = name.split(',').next().unwrap();
        let want = match kind {
            "plane" => matches!(face.surface, Surface::Plane { .. }),
            "cylinder" | "extruded circle" => matches!(face.surface, Surface::Cylinder { .. }),
            "cone" => matches!(face.surface, Surface::Cone { .. }),
            "sphere" => matches!(face.surface, Surface::Sphere { .. }),
            "torus" => matches!(face.surface, Surface::Torus { .. }),
            _ => false,
        };
        assert!(want, "{name}: {:?}", face.surface);
        if kind == "plane" {
            // The box round the annulus' sector from 0.3 rad through three quarter turns, radii 1
            // to 3: its corners where the sector reaches x = -3, y = -3 and y = 3.
            let want = [[-3.0, 3.0 * 0.3f64.cos()], [-3.0, 3.0]];
            let close = face.domain.iter().flatten().zip(want.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-9);
            assert!(close, "{name}: range {:?}, the sector's box {want:?}", face.domain);
        }
        let (old, new) = (SurfaceEval::new(surface).unwrap(), SurfaceEval::new(&face.surface).unwrap());
        // A plane's range is the box round the face, not all of it on the revolution.
        let both = if kind == "plane" { 1 } else { 2 };
        for ((from, d), onto) in [((&old, *domain), &new), ((&new, face.domain), &old)].into_iter().take(both) {
            let [[u0, u1], [v0, v1]] = d;
            for j in 0..=6 {
                for k in 0..=6 {
                    let p = from.point([u0 + (u1 - u0) * f64::from(j) / 6.0, v0 + (v1 - v0) * f64::from(k) / 6.0]).unwrap();
                    let gap = onto.project(p, None, 1e-3).and_then(|uv| onto.point(uv)).map_or(f64::INFINITY, |q| q.dist(p));
                    assert!(gap < 1e-8, "{name}: {p:?} is {gap} off the other surface ({:?} on {:?})", face.surface, face.domain);
                }
            }
        }
    }
}

/// A revolution SMLib has no analytic surface for stays a revolution (the import sends its
/// rational B-spline): a profile line off the plane through the axis, a cone across its apex, a
/// line on the axis, a circle out of that plane, a sphere's arc past a pole, a spline profile,
/// and a placement that is not a rigid motion. A rational spline that is no extruded circle stays
/// a spline.
#[test]
fn revolutions_with_no_analytic_surface_stay_revolutions() {
    use solidcraft_geom::brep::{Frame, form};
    use std::f64::consts::{FRAC_PI_2, TAU};
    let line = |a: [f64; 3], z: [f64; 3]| (Curve::Line { start: a, end: z }, [0.0, 1.0]);
    let arc = |frame: Frame, r: f64, t: [f64; 2]| form::curve(&Curve::Circle { frame, radius: r }, t).unwrap();
    let xz = |o: [f64; 3]| Frame { origin: o, x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 0.0, 1.0] };
    let tilted = Frame { origin: [5.0, 0.0, 0.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 0.6, 0.8] };
    let wave =
        (Curve::Spline { knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], points: vec![[2.0, 0.0, 0.0], [4.0, 0.0, 1.0], [2.0, 0.0, 2.0]] }, [0.0, 1.0]);
    let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let doubled = [[2.0, 0.0, 0.0, 0.0], [0.0, 2.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let cases = [
        ("skew line", line([2.0, 0.0, 0.0], [2.0, 1.0, 3.0]), identity),
        ("cone across its apex", line([-1.0, 0.0, 0.0], [3.0, 0.0, 4.0]), identity),
        ("line on the axis", line([0.0, 0.0, 0.0], [0.0, 0.0, 4.0]), identity),
        ("tilted circle", arc(tilted, 1.0, [-1.0, 2.0]), identity),
        ("arc past a pole", arc(xz([0.0; 3]), 2.0, [0.0, FRAC_PI_2 + 0.5]), identity),
        ("spline", wave, identity),
        ("scaled placement", line([2.0, 0.0, 0.0], [2.0, 0.0, 4.0]), doubled),
    ];
    for (name, (profile, t), matrix) in cases {
        let surface = Surface::PlacedRevolution { profile: Box::new(profile), origin: [0.0; 3], axis: [0.0, 0.0, 1.0], matrix, forward: true };
        let face = Face { surface, nurbs: None, domain: [t, [0.0, 0.75 * TAU]], loops: Vec::new() };
        let record = Record {
            version: b::VERSION,
            tolerance: 1e-6,
            regions: Vec::new(),
            shells: Vec::new(),
            faces: vec![face],
            edges: Vec::new(),
            vertices: Vec::new(),
        };
        let made = analytic::analytic(&record).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(matches!(made.faces[0].surface, Surface::PlacedRevolution { .. }), "{name}: {:?}", made.faces[0].surface);
    }
    // A rational spline swept along a line that is not square to its circle, and one whose
    // section is no circle (its middle weight off), stay splines.
    let frame = Frame { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] };
    let (round, range) = form::surface(&Surface::Cylinder { frame, radius: 2.0 }, [[0.0, FRAC_PI_2], [0.0, 3.0]]).unwrap();
    let Surface::RationalSpline { u_knots, v_knots, homogeneous } = round else { panic!("rational") };
    let sheared: Vec<Vec<[f64; 4]>> = homogeneous.iter().map(|row| row.iter().map(|h| [h[0] + 0.5 * h[2], h[1], h[2], h[3]]).collect()).collect();
    let mut bent = homogeneous.clone();
    for p in bent.iter_mut().flat_map(|row| row.iter_mut()).filter(|h| h[3] < 1.0) {
        *p = p.map(|x| x * 1.2);
    }
    for (name, net) in [("sheared", sheared), ("bent", bent)] {
        let surface = Surface::RationalSpline { u_knots: u_knots.clone(), v_knots: v_knots.clone(), homogeneous: net };
        let record = rectangle(surface, range);
        let made = analytic::analytic(&record).unwrap();
        assert!(matches!(made.faces[0].surface, Surface::RationalSpline { .. }), "{name}: {:?}", made.faces[0].surface);
    }
}
