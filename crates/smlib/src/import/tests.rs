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
            Edge { curve: Curve::Line { start: corners[s], end: corners[t] }, interval: [0.0, 1.0], vertices: vs, tolerance: 1e-6, uses }
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
fn a_skewed_plane_goes_as_an_exact_bilinear_patch() {
    // A tetrahedron: its faces' natural frames are unit and orthogonal, so make one plane's axes
    // non-unit to force the NURBS form and check the volume is unchanged.
    let c = [[0., 0., 0.], [6., 0., 0.], [0., 6., 0.], [0., 0., 6.]];
    let mut r = polyhedron(&c, &[&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[2, 0, 3]]);
    if let Some(Face { surface: Surface::Plane { u_axis, v_axis, .. }, domain, loops }) = r.faces.get_mut(0) {
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
    assert_eq!(a.ints[&flat::FACE_SURFACE_TYPE][0], i64::from(flat::SURFACE_NURBS));
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
            surface(&mut a, &rev, domain).unwrap();
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
