use super::*;
use crate::brep;

// The fixtures below write a body as data: shells each with their own vertices, edges and faces,
// curves and surfaces carrying their bounds, edges numbered as the fixture likes. `Brep::record`
// makes the portable record of it, which `prepare` here hands to the real one.

#[derive(Clone)]
struct Brep {
    version: u32,
    shells: Vec<Shell>,
}

#[derive(Clone)]
struct Shell {
    vertices: Vec<[f64; 3]>,
    edges: Vec<Edge>,
    faces: Vec<Face>,
}

#[derive(Clone)]
struct Edge {
    vertices: [u32; 2],
    curve: Curve,
}

#[derive(Clone)]
struct Face {
    loops: Vec<Vec<EdgeUse>>,
    surface: Surface,
    forward: bool,
}

#[derive(Clone, Copy)]
struct EdgeUse {
    edge: u32,
    forward: bool,
}

#[derive(Clone)]
enum Curve {
    Line { start: [f64; 3], end: [f64; 3] },
    CircleArc { frame: brep::Frame, radius: f64, angle: [f64; 2] },
    Polyline { points: Vec<[f64; 3]> },
    Spline { knots: Vec<f64>, points: Vec<[f64; 3]> },
    RationalSpline { knots: Vec<f64>, homogeneous: Vec<[f64; 4]> },
    SurfaceIntersection { surfaces: [Box<Surface>; 2], leader: Box<Curve>, tolerance: f64 },
}

#[derive(Clone)]
enum Surface {
    Plane {
        origin: [f64; 3],
        u_axis: [f64; 3],
        v_axis: [f64; 3],
    },
    PlaneThrough {
        origin: [f64; 3],
        u_point: [f64; 3],
        v_point: [f64; 3],
    },
    Cylinder {
        frame: brep::Frame,
        radius: f64,
        angle: [f64; 2],
        height: [f64; 2],
    },
    Cone {
        frame: brep::Frame,
        radius_at_origin: f64,
        half_angle: f64,
        angle: [f64; 2],
        height: [f64; 2],
    },
    Sphere {
        frame: brep::Frame,
        radius: f64,
        longitude: [f64; 2],
        latitude: [f64; 2],
    },
    Torus {
        frame: brep::Frame,
        major_radius: f64,
        minor_radius: f64,
        longitude: [f64; 2],
        meridian: [f64; 2],
    },
    Spline {
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        points: Vec<Vec<[f64; 3]>>,
    },
    RationalSpline {
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        homogeneous: Vec<Vec<[f64; 4]>>,
    },
    /// A revolution as far as `angle`, its profile where it stands.
    Revolution {
        profile: Box<Curve>,
        origin: [f64; 3],
        axis: [f64; 3],
        angle: [f64; 2],
    },
    /// A whole revolution.
    PlacedRevolution {
        profile: Box<Curve>,
        origin: [f64; 3],
        axis: [f64; 3],
        matrix: [[f64; 4]; 4],
        forward: bool,
    },
}

const IDENTITY: [[f64; 4]; 4] = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];

fn range(natural: brep::Natural) -> [f64; 2] {
    match natural {
        brep::Natural::Range(a, b) => [a, b],
        brep::Natural::Periodic => [0., std::f64::consts::TAU],
    }
}

impl Curve {
    /// What the curve is, unbounded.
    fn label(&self) -> brep::Curve {
        match self {
            Curve::Line { start, end } => brep::Curve::Line { start: *start, end: *end },
            Curve::CircleArc { frame, radius, .. } => brep::Curve::Circle { frame: frame.clone(), radius: *radius },
            Curve::Polyline { points } => brep::Curve::Polyline { points: points.clone() },
            Curve::Spline { knots, points } => brep::Curve::Spline { knots: knots.clone(), points: points.clone() },
            Curve::RationalSpline { knots, homogeneous } => brep::Curve::RationalSpline { knots: knots.clone(), homogeneous: homogeneous.clone() },
            Curve::SurfaceIntersection { surfaces, leader, tolerance } => brep::Curve::SurfaceIntersection {
                surfaces: [Box::new(surfaces[0].label()), Box::new(surfaces[1].label())],
                leader: Box::new(leader.label()),
                tolerance: *tolerance,
            },
        }
    }

    /// The curve's parameter range, the whole of it for a profile (a line from `start` to `end`).
    fn extent(&self) -> [f64; 2] {
        match self {
            Curve::Line { .. } => [0., 1.],
            Curve::CircleArc { angle, .. } => *angle,
            _ => range(self.label().natural()),
        }
    }

    /// The label, its NURBS form and the interval the edge's parameter runs over.
    fn parts(&self) -> (brep::Curve, Option<brep::Curve>, [f64; 2]) {
        let label = self.label();
        match self {
            Curve::CircleArc { angle, .. } => {
                let (form, interval) = brep::form::curve(&label, *angle).expect("a circle's form");
                (label, Some(form), interval)
            }
            _ => (label, None, self.extent()),
        }
    }
}

impl Surface {
    /// What the surface is, unbounded.
    fn label(&self) -> brep::Surface {
        match self {
            Surface::Plane { origin, u_axis, v_axis } => brep::Surface::Plane { origin: *origin, u_axis: *u_axis, v_axis: *v_axis },
            Surface::PlaneThrough { origin, u_point, v_point } => {
                brep::Surface::PlaneThrough { origin: *origin, u_point: *u_point, v_point: *v_point }
            }
            Surface::Cylinder { frame, radius, .. } => brep::Surface::Cylinder { frame: frame.clone(), radius: *radius },
            Surface::Cone { frame, radius_at_origin, half_angle, .. } => {
                brep::Surface::Cone { frame: frame.clone(), radius_at_origin: *radius_at_origin, half_angle: *half_angle }
            }
            Surface::Sphere { frame, radius, .. } => brep::Surface::Sphere { frame: frame.clone(), radius: *radius },
            Surface::Torus { frame, major_radius, minor_radius, .. } => {
                brep::Surface::Torus { frame: frame.clone(), major_radius: *major_radius, minor_radius: *minor_radius, tube_start: 0. }
            }
            Surface::Spline { u_knots, v_knots, points } => {
                brep::Surface::Spline { u_knots: u_knots.clone(), v_knots: v_knots.clone(), points: points.clone() }
            }
            Surface::RationalSpline { u_knots, v_knots, homogeneous } => {
                brep::Surface::RationalSpline { u_knots: u_knots.clone(), v_knots: v_knots.clone(), homogeneous: homogeneous.clone() }
            }
            // A revolution from its profile, u the angle and v along the profile.
            Surface::Revolution { profile, origin, axis, .. } => {
                brep::Surface::PlacedRevolution { profile: Box::new(profile.label()), origin: *origin, axis: *axis, matrix: IDENTITY, forward: false }
            }
            Surface::PlacedRevolution { profile, origin, axis, matrix, forward } => brep::Surface::PlacedRevolution {
                profile: Box::new(profile.label()),
                origin: *origin,
                axis: *axis,
                matrix: *matrix,
                forward: *forward,
            },
        }
    }

    /// The bounds it carries, on its label.
    fn bounds(&self) -> [[f64; 2]; 2] {
        let turn = [0., std::f64::consts::TAU];
        match self {
            Surface::Cylinder { angle, height, .. } | Surface::Cone { angle, height, .. } => [*angle, *height],
            Surface::Sphere { longitude, latitude, .. } => [*longitude, *latitude],
            Surface::Torus { longitude, meridian, .. } => [*longitude, *meridian],
            Surface::Revolution { profile, angle, .. } => [*angle, profile.extent()],
            Surface::PlacedRevolution { profile, forward: true, .. } => [profile.extent(), turn],
            Surface::PlacedRevolution { profile, forward: false, .. } => [turn, profile.extent()],
            Surface::Spline { .. } | Surface::RationalSpline { .. } => self.label().natural().map(range),
            // Nothing reads a plane's domain; this is around anything a fixture draws on one.
            Surface::Plane { .. } | Surface::PlaneThrough { .. } => [[-1e12, 1e12]; 2],
        }
    }

    /// The label, its NURBS form and the face's domain.
    fn parts(&self) -> (brep::Surface, Option<brep::Surface>, [[f64; 2]; 2]) {
        let (label, bounds) = (self.label(), self.bounds());
        match self {
            Surface::Plane { .. } | Surface::PlaneThrough { .. } | Surface::Spline { .. } | Surface::RationalSpline { .. } => (label, None, bounds),
            _ => {
                let (form, domain) = brep::form::surface(&label, bounds).expect("a surface's form");
                (label, Some(form), domain)
            }
        }
    }
}

impl Brep {
    /// The portable record of the fixture, and the fixture's number of each of its edges. Shells,
    /// and faces within them, keep their order, so a face keeps its number; edges and vertices come
    /// in canonical order, first appearance walking faces, loops and uses. Pcurves carry the edge's
    /// interval, which `Brep::validate` checks; neither it nor hidden lines read their points, as
    /// hidden lines trace trims from the edges in 3D, so these are not on the surface.
    fn record(&self) -> (brep::Brep, Vec<usize>) {
        let (mut points, mut edges, mut faces, mut spans) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for s in &self.shells {
            let (base_edge, base_vertex) = (edges.len() as u32, points.len() as u32);
            spans.push((faces.len()..faces.len() + s.faces.len(), edges.len()..edges.len() + s.edges.len()));
            points.extend_from_slice(&s.vertices);
            edges.extend(s.edges.iter().map(|e| Edge { vertices: e.vertices.map(|v| v + base_vertex), curve: e.curve.clone() }));
            faces.extend(s.faces.iter().cloned().map(|mut f| {
                f.loops.iter_mut().flatten().for_each(|u| u.edge += base_edge);
                f
            }));
        }
        let mut order: Vec<Option<u32>> = vec![None; edges.len()];
        let mut numbers: Vec<usize> = Vec::new();
        let mut walked: Vec<Vec<brep::EdgeUseRef>> = vec![Vec::new(); edges.len()];
        let mut visit = |e: usize, order: &mut Vec<Option<u32>>| {
            if order[e].is_none() {
                order[e] = Some(numbers.len() as u32);
                numbers.push(e);
            }
        };
        for (fi, f) in faces.iter().enumerate() {
            for (li, l) in f.loops.iter().enumerate() {
                for k in 0..l.len() {
                    let position = if f.forward { k } else { l.len() - 1 - k };
                    let u = l[position];
                    visit(u.edge as usize, &mut order);
                    let at = brep::EdgeUseRef { face: fi as u32, r#loop: li as u32, position: position as u32, upper: !u.forward };
                    walked[u.edge as usize].push(at);
                }
            }
        }
        // Edges no face uses, shell by shell: its wires.
        let wires: Vec<Vec<u32>> = spans
            .iter()
            .map(|(_, shell_edges)| {
                shell_edges
                    .clone()
                    .filter(|e| walked[*e].is_empty())
                    .map(|e| {
                        visit(e, &mut order);
                        order[e].expect("just numbered")
                    })
                    .collect()
            })
            .collect();
        let number = |e: u32| order[e as usize].expect("every edge is numbered");
        let mut vertex_order: Vec<Option<u32>> = vec![None; points.len()];
        let mut vertices = Vec::new();
        let mut edge_records = Vec::new();
        for &e in &numbers {
            let (label, nurbs, interval) = edges[e].curve.parts();
            let ends = edges[e].vertices.map(|v| {
                *vertex_order[v as usize].get_or_insert_with(|| {
                    vertices.push(brep::Vertex { point: points[v as usize], tolerance: 1e-6 });
                    vertices.len() as u32 - 1
                })
            });
            edge_records.push(brep::Edge { curve: label, nurbs, interval, vertices: ends, tolerance: 1e-6, uses: walked[e].clone() });
        }
        let pcurve = |edge: u32| {
            let [t0, t1] = edge_records[number(edge) as usize].interval;
            brep::Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![[0., 0.], [1., 0.]] }
        };
        let face_records = faces
            .iter()
            .map(|f| {
                let (surface, nurbs, domain) = f.surface.parts();
                let loops = f
                    .loops
                    .iter()
                    .map(|l| brep::Loop::Edges {
                        edges: l.iter().map(|u| brep::OrientedEdge { edge: number(u.edge), forward: u.forward, pcurve: pcurve(u.edge) }).collect(),
                    })
                    .collect();
                brep::Face { surface, nurbs, domain, loops }
            })
            .collect();
        let mut regions = vec![brep::Region { shells: (0..self.shells.len() as u32).collect(), void: false }];
        let mut shells = Vec::new();
        for (s, (shell_faces, _)) in spans.iter().enumerate() {
            // A closed shell (each edge on its faces used twice among them) also bounds a region.
            let used = |e: u32| faces[shell_faces.clone()].iter().flat_map(|f| f.loops.iter().flatten()).filter(|u| u.edge == e).count();
            if faces[shell_faces.clone()].iter().flat_map(|f| f.loops.iter().flatten()).all(|u| used(u.edge) >= 2) {
                regions.push(brep::Region { shells: vec![s as u32], void: false });
            }
            let oriented = shell_faces.clone().map(|f| brep::OrientedFace { face: f as u32, forward: faces[f].forward }).collect();
            shells.push(brep::Shell::Faces { faces: oriented, wires: wires[s].clone() });
        }
        let record = brep::Brep { version: self.version, tolerance: 1e-6, regions, shells, faces: face_records, edges: edge_records, vertices };
        (record, numbers)
    }
}

/// The real `prepare_cancellable` on the fixture's record, its edges then back in the fixture's
/// numbering and its arcs on the angles the fixture counted them from (the record's are within a
/// turn of zero: `Edge::curve_interval`), which is what the assertions below speak of.
fn prepare_cancellable(b: &Brep, opts: &Options, cancel: &dyn Fn() -> bool) -> Result<Prepared, HlrError> {
    let (record, numbers) = b.record();
    let mut def = super::prepare_cancellable(&record, opts, cancel)?;
    let curves: Vec<&Curve> = b.shells.iter().flat_map(|s| s.edges.iter().map(|e| &e.curve)).collect();
    let mut ordered: Vec<(usize, EdgeData)> = numbers.into_iter().zip(std::mem::take(&mut def.edges)).collect();
    ordered.sort_by_key(|(n, _)| *n);
    for (n, e) in &mut ordered {
        if let (Curve::CircleArc { angle, .. }, C3::Arc { range, .. }) = (curves[*n], &mut e.curve) {
            let turns = ((angle[0] - range[0]) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
            *range = [range[0] + turns, range[1] + turns];
        }
    }
    def.edges = ordered.into_iter().map(|(_, e)| e).collect();
    Ok(def)
}

fn prepare(b: &Brep, opts: &Options) -> Result<Prepared, HlrError> {
    prepare_cancellable(b, opts, &|| false)
}

/// A surface's chart over the bounds it carries.
fn chart(s: &Surface) -> S3 {
    S3::bounded(&s.label(), s.bounds()).unwrap()
}

/// An axis-aligned box with line edges and plane faces.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Brep {
    let corner = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let vertices: Vec<[f64; 3]> = (0..8).map(corner).collect();
    let pairs = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    let edges: Vec<Edge> =
        pairs.iter().map(|&(a, b)| Edge { vertices: [a as u32, b as u32], curve: Curve::Line { start: corner(a), end: corner(b) } }).collect();
    let cycle = |vs: [usize; 4]| -> Vec<EdgeUse> {
        (0..4)
            .map(|k| {
                let (a, b) = (vs[k], vs[(k + 1) % 4]);
                let (i, fwd) = pairs
                    .iter()
                    .enumerate()
                    .find_map(|(i, &(x, y))| (x == a && y == b).then_some((i, true)).or((x == b && y == a).then_some((i, false))))
                    .unwrap();
                EdgeUse { edge: i as u32, forward: fwd }
            })
            .collect()
    };
    let plane = |o: usize, u: usize, v: usize| {
        let (o, u, v) = (corner(o), corner(u), corner(v));
        Surface::Plane { origin: o, u_axis: [u[0] - o[0], u[1] - o[1], u[2] - o[2]], v_axis: [v[0] - o[0], v[1] - o[1], v[2] - o[2]] }
    };
    let faces = vec![
        Face { loops: vec![cycle([0, 2, 3, 1])], surface: plane(0, 2, 1), forward: true },
        Face { loops: vec![cycle([4, 5, 7, 6])], surface: plane(4, 5, 6), forward: true },
        Face { loops: vec![cycle([0, 1, 5, 4])], surface: plane(0, 1, 4), forward: true },
        Face { loops: vec![cycle([2, 6, 7, 3])], surface: plane(2, 6, 3), forward: true },
        Face { loops: vec![cycle([0, 4, 6, 2])], surface: plane(0, 4, 2), forward: true },
        Face { loops: vec![cycle([1, 3, 7, 5])], surface: plane(1, 3, 5), forward: true },
    ];
    Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices, edges, faces }] }
}

fn identity(body: usize, t: [f64; 3]) -> Placement {
    Placement { body, rotation: [Vec3::new(1., 0., 0.), Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)], translation: Vec3::new(t[0], t[1], t[2]) }
}

fn never() -> bool {
    false
}

#[test]
fn iso_cube_hides_the_three_back_edges() {
    let def = prepare(&cuboid([0.; 3], [1.; 3]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -1., -1.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let mut hidden = Vec::new();
    for l in &d.lines {
        let Source::Edge { edge, .. } = l.source else { panic!("a box has no silhouettes") };
        let states: Vec<_> = l.pieces.iter().map(|p| p.visibility).collect();
        assert!(states.windows(2).all(|w| w[0] == w[1]), "edge {edge} is split: {states:?}");
        if states[0] == Visibility::Hidden {
            hidden.push(edge);
        }
    }
    hidden.sort_unstable();
    // The edges at the corner nearest the origin: (0,1), (0,2), (0,4).
    assert_eq!(hidden, vec![0, 4, 8], "{d:#?}");
}

#[test]
fn a_box_in_front_hides_the_middle_of_an_edge() {
    let back = prepare(&cuboid([0.; 3], [1.; 3]), &Options::default()).unwrap();
    let front = prepare(&cuboid([0.5, -2., 0.25], [1.5, -1., 0.75]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[back, front], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    // Edge 9 joins corners 1 (1,0,0) and 5 (1,0,1): t is z.
    let line = d.lines.iter().find(|l| l.placement == 0 && l.source == Source::Edge { edge: 9, tangent: false }).unwrap();
    let got: Vec<([f64; 2], Visibility)> = line.pieces.iter().map(|p| (p.t, p.visibility)).collect();
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(got[1].1, Visibility::Hidden);
    assert!((got[1].0[0] - 0.25).abs() < 1e-12 && (got[1].0[1] - 0.75).abs() < 1e-12, "{got:?}");
    assert_eq!((got[0].1, got[2].1), (Visibility::Visible, Visibility::Visible));
}

#[test]
fn quadratic_roots_with_no_linear_term() {
    // A ray from inside a sphere, square to the radius at its start (b = 0): both roots.
    let mut roots = surface::quadratic(1., 0., -100.);
    roots.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(roots.len(), 2, "{roots:?}");
    assert!((roots[0].0 + 10.).abs() < 1e-12 && (roots[1].0 - 10.).abs() < 1e-12, "{roots:?}");
    let mut roots = surface::quadratic(1., -0., -100.);
    roots.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!((roots[0].0 + 10.).abs() < 1e-12 && (roots[1].0 - 10.).abs() < 1e-12, "{roots:?}");
}

#[test]
fn a_torus_seen_along_its_axis_has_both_equator_silhouettes() {
    // A whole-meridian torus record: the outer equator sits where the closed profile's ends meet.
    let frame = crate::brep::Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let tau = std::f64::consts::TAU;
    let s = chart(&Surface::Torus { frame, major_radius: 20., minor_radius: 5., longitude: [0., tau], meridian: [0., tau] });
    let mut radii: Vec<f64> = s
        .rev_silhouette(Vec3::new(0., 0., -1.))
        .iter()
        .map(|(c, _)| match c {
            curve::C3::Arc { r, .. } => r[0],
            _ => panic!("circles expected"),
        })
        .collect();
    radii.sort_by(f64::total_cmp);
    assert_eq!(radii.len(), 2, "{radii:?}");
    assert!((radii[0] - 15.).abs() < 1e-9 && (radii[1] - 25.).abs() < 1e-9, "{radii:?}");
}

#[test]
fn far_apart_bodies_do_not_merge_nearby_visible_edges() {
    // A 5 mm cube and a box 100 m away: coincidence is judged at the chords' own accuracy,
    // not at a fraction of the scene's size, so none of the cube's edges is taken as a duplicate.
    let near = prepare(&cuboid([0.; 3], [5.; 3]), &Options::default()).unwrap();
    let far = prepare(&cuboid([100_000.; 3], [100_001.; 3]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -0.7, -0.4), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[near, far], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let covered_visible = d
        .lines
        .iter()
        .filter(|l| l.placement == 0)
        .flat_map(|l| l.pieces.iter())
        .filter(|p| p.visibility == Visibility::Visible && p.covered)
        .count();
    assert_eq!(covered_visible, 0);
}

#[test]
fn a_thin_torus_tube_stops_a_ray() {
    // R 100, r 0.1: a ray across the tube meets it twice within 0.2 mm.
    let roots = surface::torus_roots(Vec3::new(100., -50., 0.), Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.), 100., 0.1, [0., 200.]);
    assert!(roots.iter().filter(|(_, grazing)| !grazing).count() >= 2, "{roots:?}");
}

#[test]
fn a_thin_tube_on_a_huge_torus_stops_a_ray() {
    // R 100 m, r 1 mm: inside the tube the implicit function stays far below a size^4 floor.
    // A radial ray from 5 mm inside the centre circle crosses the tube at 4 and 6 mm.
    let roots = surface::torus_roots(Vec3::new(100_000. - 5., 0., 0.), Vec3::new(1., 0., 0.), Vec3::new(0., 0., 1.), 100_000., 1., [0., 10.]);
    let hits: Vec<f64> = roots.iter().filter(|(_, grazing)| !grazing).map(|(l, _)| *l).collect();
    // The implicit form cancels terms of 4e20 here: the roots are good to about 1e-6 mm.
    assert!(hits.len() == 2 && (hits[0] - 4.).abs() < 1e-5 && (hits[1] - 6.).abs() < 1e-5, "{roots:?}");
}

#[test]
fn a_hair_thin_tube_on_a_kilometre_torus_stops_a_ray() {
    // R 1 km, r 0.1 mm (R/r = 1e7): roots at 0.9 and 1.1 mm, good to about 1e-4 mm here.
    let roots = surface::torus_roots(Vec3::new(1e6 - 1., 0., 0.), Vec3::new(1., 0., 0.), Vec3::new(0., 0., 1.), 1e6, 0.1, [0., 2.]);
    let hits: Vec<f64> = roots.iter().filter(|(_, grazing)| !grazing).map(|(l, _)| *l).collect();
    assert!(hits.len() == 2 && (hits[0] - 0.9).abs() < 1e-3 && (hits[1] - 1.1).abs() < 1e-3, "{roots:?}");
}

#[test]
fn a_ray_from_the_hole_crosses_a_thin_large_torus() {
    // R 4 m, r 0.01 mm, from the centre outward over 8 m: only the shell around the tube is sampled.
    let roots = surface::torus_roots(Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.), Vec3::new(0., 0., 1.), 4000., 0.01, [0., 8000.]);
    let hits: Vec<f64> = roots.iter().filter(|(_, grazing)| !grazing).map(|(l, _)| *l).collect();
    assert!(hits.len() == 2 && (hits[0] - 3999.99).abs() < 1e-6 && (hits[1] - 4000.01).abs() < 1e-6, "{roots:?}");
}

#[test]
fn a_torus_seen_nearly_along_its_axis_keeps_its_silhouettes() {
    // Meridian starting off the equator and a view 1e-4 rad from the axis: the silhouette lives
    // in narrow profile intervals around the equators, which must not fall between samples.
    let frame = crate::brep::Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let tau = std::f64::consts::TAU;
    let shift = std::f64::consts::PI / 720.;
    let s = chart(&Surface::Torus { frame, major_radius: 1000., minor_radius: 500., longitude: [0., tau], meridian: [shift, shift + tau] });
    let d = Vec3::new(1e-4, 0., -1.).normalized().unwrap();
    let curves = s.rev_silhouette(d);
    // Both the outer (radius about 1500) and the inner (about 500) silhouettes are there.
    let radii: Vec<f64> = curves
        .iter()
        .flat_map(|(c, _)| {
            let [t0, t1] = c.domain();
            (0..=8).map(move |i| c.at(t0 + (t1 - t0) * i as f64 / 8.))
        })
        .map(|p| p.x.hypot(p.y))
        .collect();
    assert!(radii.iter().any(|r| *r > 1400.) && radii.iter().any(|r| *r < 600.), "{radii:?}");
}

#[test]
fn a_shallow_cut_through_a_torus_tube_is_found() {
    // R 20, r 5, a ray along the axis 0.01 mm inside the outer equator: a 0.63 mm chord.
    let roots = surface::torus_roots(Vec3::new(24.99, 0., -10.), Vec3::new(0., 0., 1.), Vec3::new(0., 0., 1.), 20., 5., [0., 20.]);
    let hits: Vec<f64> = roots.iter().filter(|(_, grazing)| !grazing).map(|(l, _)| *l).collect();
    let half = (25f64 - 4.99f64.powi(2)).sqrt();
    assert!(hits.len() == 2 && (hits[0] - (10. - half)).abs() < 1e-6 && (hits[1] - (10. + half)).abs() < 1e-6, "{roots:?}");
}

/// A frame at `o` with x along +X and y along +Y (z along +Z).
fn frame_at(o: [f64; 3]) -> crate::brep::Frame {
    crate::brep::Frame { origin: o, x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] }
}

/// A full circle about Z through vertex `v`.
fn circle(o: [f64; 3], r: f64, v: u32) -> Edge {
    Edge { vertices: [v, v], curve: Curve::CircleArc { frame: frame_at(o), radius: r, angle: [0., std::f64::consts::TAU] } }
}

fn uses(edge: u32, forward: bool) -> EdgeUse {
    EdgeUse { edge, forward }
}

/// A cylinder of radius `r` and height `h` standing on the XY plane, as analytic records.
fn cylinder(r: f64, h: f64) -> Brep {
    turned(r, r, h)
}

/// A cylinder (`r0 == r1`) or a cone frustum from radius `r0` at z = 0 to `r1` at z = `h`,
/// its seam at angle 0.
fn turned(r0: f64, r1: f64, h: f64) -> Brep {
    let tau = std::f64::consts::TAU;
    let seam = Edge { vertices: [0, 1], curve: Curve::Line { start: [r0, 0., 0.], end: [r1, 0., h] } };
    let edges = vec![circle([0.; 3], r0, 0), circle([0., 0., h], r1, 1), seam];
    let side = if r0 == r1 {
        Surface::Cylinder { frame: frame_at([0.; 3]), radius: r0, angle: [0., tau], height: [0., h] }
    } else {
        Surface::Cone { frame: frame_at([0.; 3]), radius_at_origin: r0, half_angle: ((r1 - r0) / h).atan(), angle: [0., tau], height: [0., h] }
    };
    let faces = vec![
        Face { loops: vec![vec![uses(0, true), uses(2, true), uses(1, false), uses(2, false)]], surface: side, forward: true },
        Face {
            loops: vec![vec![uses(0, false)]],
            surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., -1., 0.] },
            forward: true,
        },
        Face {
            loops: vec![vec![uses(1, true)]],
            surface: Surface::Plane { origin: [0., 0., h], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
            forward: true,
        },
    ];
    Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: vec![[r0, 0., 0.], [r1, 0., h]], edges, faces }] }
}

/// A sphere of radius `r` at the origin: two hemispheres meeting at the equator.
fn sphere(r: f64) -> Brep {
    let (tau, half) = (std::f64::consts::TAU, std::f64::consts::FRAC_PI_2);
    let hemisphere = |latitude: [f64; 2], forward| Face {
        loops: vec![vec![uses(0, forward)]],
        surface: Surface::Sphere { frame: frame_at([0.; 3]), radius: r, longitude: [0., tau], latitude },
        forward: true,
    };
    let faces = vec![hemisphere([0., half], true), hemisphere([-half, 0.], false)];
    Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: vec![[r, 0., 0.]], edges: vec![circle([0.; 3], r, 0)], faces }] }
}

/// A torus about Z (major radius `big`, minor `small`): top and bottom halves meeting at the
/// outer and inner equators.
fn torus(big: f64, small: f64) -> Brep {
    let (tau, pi) = (std::f64::consts::TAU, std::f64::consts::PI);
    let half = |meridian: [f64; 2], forward: bool| Face {
        loops: vec![vec![uses(0, forward)], vec![uses(1, !forward)]],
        surface: Surface::Torus { frame: frame_at([0.; 3]), major_radius: big, minor_radius: small, longitude: [0., tau], meridian },
        forward: true,
    };
    let edges = vec![circle([0.; 3], big + small, 0), circle([0.; 3], big - small, 1)];
    let faces = vec![half([0., pi], true), half([pi, tau], false)];
    Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: vec![[big + small, 0., 0.], [big - small, 0., 0.]], edges, faces }] }
}

/// A bilinear (twisted) spline patch over x, y in [-5, 5] with z between 0 and 1.
fn twisted_patch() -> Brep {
    let p = [[-5., -5., 0.], [5., -5., 1.], [5., 5., 0.], [-5., 5., 1.]];
    let edges =
        (0..4).map(|i| Edge { vertices: [i as u32, ((i + 1) % 4) as u32], curve: Curve::Line { start: p[i], end: p[(i + 1) % 4] } }).collect();
    let face = Face {
        loops: vec![(0..4).map(|e| uses(e, true)).collect()],
        surface: Surface::Spline { u_knots: vec![0., 0., 1., 1.], v_knots: vec![0., 0., 1., 1.], points: vec![vec![p[0], p[3]], vec![p[1], p[2]]] },
        forward: true,
    };
    Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: p.to_vec(), edges, faces: vec![face] }] }
}

fn pieces(d: &Drawing) -> impl Iterator<Item = &Piece> {
    d.lines.iter().flat_map(|l| l.pieces.iter())
}

/// The turn of a circle edge's pieces drawn as exact conic arcs, or `None` when they are all
/// drawn as chords (weights too far apart to certify the circle): never a wrong exact arc.
fn turn_or_chords<'a>(pieces: impl Iterator<Item = &'a Piece>) -> Option<f64> {
    let pieces: Vec<&Piece> = pieces.collect();
    if pieces.iter().all(|p| matches!(p.accuracy, CurveAccuracy::Sampled { .. })) {
        return None;
    }
    let mut sweep = 0.;
    for p in pieces {
        assert_eq!(p.accuracy, CurveAccuracy::Analytic, "{p:?}");
        for g in &p.geometry {
            let Seg2::Conic { w, .. } = *g else { panic!("a circle is drawn as conic arcs: {g:?}") };
            sweep += 2. * w.acos();
        }
    }
    Some(sweep)
}

fn silhouette_pieces(d: &Drawing) -> Vec<&Piece> {
    d.lines.iter().filter(|l| matches!(l.source, Source::Silhouette { .. })).flat_map(|l| l.pieces.iter()).collect()
}

fn edge_line(d: &Drawing, placement: usize, edge: usize) -> &Line {
    let found = d.lines.iter().find(|l| l.placement == placement && matches!(l.source, Source::Edge { edge: e, .. } if e == edge));
    found.unwrap_or_else(|| panic!("no line for edge {edge} of placement {placement}"))
}

#[test]
fn a_cylinder_seen_from_the_side_has_exact_silhouette_lines() {
    let def = prepare(&cylinder(10., 30.), &Options::default()).unwrap();
    // Slightly from above, along +X (rulings at +-90 degrees) and along +Y (rulings at 0 and
    // 180 degrees: one runs along the seam, which is not drawn as an edge).
    for dir in [Vec3::new(1., 0., -0.3), Vec3::new(0., 1., -0.3)] {
        let view = View::new(dir, Vec3::new(0., 0., 1.)).unwrap();
        let d = hidden_lines(std::slice::from_ref(&def), &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
        check_cylinder_sides(&d, &view);
    }
}

fn check_cylinder_sides(d: &Drawing, view: &View) {
    let sil = silhouette_pieces(d);
    assert!(!sil.is_empty());
    let rise = 30. * view.y.z;
    let mut sides = Vec::new();
    for p in sil {
        assert_eq!(p.accuracy, CurveAccuracy::Analytic);
        assert_eq!(p.visibility, Visibility::Visible);
        for g in &p.geometry {
            let Seg2::Line { a, b } = *g else { panic!("a silhouette ruling is a line: {g:?}") };
            assert!((a.x.abs() - 10.).abs() < 1e-12 && (a.x - b.x).abs() < 1e-12, "{g:?}");
            sides.push((a.x.signum(), (b.y - a.y).abs()));
        }
    }
    for side in [-1., 1.] {
        let len: f64 = sides.iter().filter(|s| s.0 == side).map(|s| s.1).sum();
        assert!((len - rise).abs() < 1e-9, "side {side}: {len} vs {rise}");
    }
    assert_eq!(d.stats.uncertain_uncovered, 0, "{:?}", d.stats);
    assert!(d.unsupported.is_empty());
}

#[test]
fn a_sphere_outline_is_exact_conic_arcs_in_any_view() {
    let def = prepare(&sphere(10.), &Options::default()).unwrap();
    let views = [
        (Vec3::new(1., 2., 3.), Vec3::new(0., 0., 1.)),
        (Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)),
        (Vec3::new(-0.3, 1., 0.1), Vec3::new(0., 0., 1.)),
    ];
    for (dir, up) in views {
        let view = View::new(dir, up).unwrap();
        let d = hidden_lines(std::slice::from_ref(&def), &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
        // The outline: every uncovered visible piece lies on the circle of radius 10.
        let mut drawn = 0.;
        for p in
            pieces(&d).filter(|p| p.visibility == Visibility::Visible && !p.covered && p.geometry.iter().all(|g| matches!(g, Seg2::Conic { .. })))
        {
            assert_eq!(p.accuracy, CurveAccuracy::Analytic);
            for g in &p.geometry {
                let ends = (g.point_at(0.), g.point_at(1.));
                if (ends.0.len() - 10.).abs() > 1e-9 {
                    continue;
                }
                for k in 0..=16 {
                    let q = g.point_at(k as f64 / 16.);
                    assert!((q.len() - 10.).abs() < 1e-9, "{dir:?}: {q:?} off the outline");
                }
                drawn += ends.0.dist(ends.1);
            }
        }
        assert!(drawn > 40., "{dir:?}: the outline is drawn ({drawn} mm of chords)");
        for p in silhouette_pieces(&d) {
            assert!(p.geometry.iter().all(|g| matches!(g, Seg2::Conic { .. })), "{dir:?}: {p:?}");
        }
        assert_eq!(d.stats.uncertain_uncovered, 0, "{dir:?}: {:?}", d.stats);
    }
}

#[test]
fn a_torus_seen_face_on_is_drawn_at_both_equators() {
    let def = prepare(&torus(20., 5.), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let (mut outer, mut inner) = (false, false);
    for p in pieces(&d).filter(|p| p.visibility == Visibility::Visible && !p.covered) {
        for g in &p.geometry {
            for k in 0..=8 {
                let r = g.point_at(k as f64 / 8.).len();
                assert!((r - 25.).abs() < 1e-9 || (r - 15.).abs() < 1e-9, "a point at radius {r}");
                outer |= (r - 25.).abs() < 1e-9;
                inner |= (r - 15.).abs() < 1e-9;
            }
        }
    }
    assert!(outer && inner);
    // Both equators are silhouettes along the seams between the halves: one curve each.
    assert_eq!(d.stats.silhouettes, 2, "{:?}", d.stats);
    assert_eq!(d.stats.uncertain_uncovered, 0, "{:?}", d.stats);
}

#[test]
fn an_edge_under_a_torus_is_hidden_where_the_tube_covers_it() {
    // Looking down on a torus (R 20, r 5) over a long bar: the bar's top edge along y = 1 is
    // hidden where sqrt(x^2 + 1) is between 15 and 25.
    let t = prepare(&torus(20., 5.), &Options::default()).unwrap();
    let bar = prepare(&cuboid([-40., -1., -12.], [40., 1., -10.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[t, bar], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    // Edge 3 joins corners 6 (-40, 1, -10) and 7 (40, 1, -10): x = 80 t - 40.
    let hidden: Vec<[f64; 2]> =
        edge_line(&d, 1, 3).pieces.iter().filter(|p| p.visibility == Visibility::Hidden).map(|p| [p.t[0] * 80. - 40., p.t[1] * 80. - 40.]).collect();
    let (a, b) = (224f64.sqrt(), 624f64.sqrt());
    assert_eq!(hidden.len(), 2, "{hidden:?}");
    for (h, want) in hidden.iter().zip([[-b, -a], [a, b]]) {
        assert!((h[0] - want[0]).abs() < 1e-9 && (h[1] - want[1]).abs() < 1e-9, "{hidden:?}");
    }
    assert_eq!(d.stats.uncertain_uncovered, 0, "{:?}", d.stats);
}

#[test]
fn a_part_far_from_the_origin_draws_the_same_as_at_the_origin() {
    let def = prepare(&cylinder(10., 30.), &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -0.7, -0.4), Vec3::new(0., 0., 1.)).unwrap();
    let defs = [def];
    let states = |d: &Drawing| -> Vec<(Visibility, bool, usize)> { pieces(d).map(|p| (p.visibility, p.covered, p.geometry.len())).collect() };
    let base = hidden_lines(&defs, &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    for t in [[1e5, 2e5, -3e5], [1e7, 2e7, -3e7]] {
        let d = hidden_lines(&defs, &[identity(0, t)], &view, &Options::default(), &never).unwrap();
        assert_eq!(states(&base), states(&d), "classification moved at {t:?}");
        let shift = view.screen(Vec3::new(t[0], t[1], t[2]));
        // Only the final shift back to the caller's frame rounds: about 1e-16 of |t|.
        let tol = 1e-15 * (t[0].abs() + t[1].abs() + t[2].abs()) + 1e-9;
        for (a, b) in pieces(&base).zip(pieces(&d)) {
            for (g, h) in a.geometry.iter().zip(&b.geometry) {
                for s in [0., 0.5, 1.] {
                    let (p, q) = (g.point_at(s), h.point_at(s));
                    assert!((q.x - shift[0] - p.x).abs() < tol && (q.y - shift[1] - p.y).abs() < tol, "{p:?} vs {q:?} at {t:?}");
                }
            }
        }
    }
}

#[test]
fn freeform_faces_are_reported_and_never_hide_for_certain() {
    let patch = prepare(&twisted_patch(), &Options::default()).unwrap();
    assert_eq!(patch.report.freeform_faces, vec![0]);
    // A bar under the patch, seen from above: its top edges (2, 3, 6, 7) are behind the patch only.
    let bar = prepare(&cuboid([-2., -1., -5.], [2., 1., -3.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0.1, 0.05, -1.), Vec3::new(0., 1., 0.)).unwrap();
    let defs = [patch, bar];
    let scene = [identity(0, [0.; 3]), identity(1, [0.; 3])];
    let top = |d: &Drawing| -> Vec<Visibility> {
        [2, 3, 6, 7].iter().flat_map(|&e| edge_line(d, 1, e).pieces.iter().map(|p| p.visibility)).collect::<Vec<_>>()
    };
    let d = hidden_lines(&defs, &scene, &view, &Options::default(), &never).unwrap();
    assert_eq!(d.unsupported, vec![Unsupported { placement: 0, face: 0, reason: "freeform face" }]);
    assert!(top(&d).iter().all(|v| *v == Visibility::Uncertain), "{:?}", top(&d));
    // With freeform faces handled the patch hides them.
    let d = hidden_lines(&defs, &scene, &view, &Options { freeform: true, ..Options::default() }, &never).unwrap();
    assert!(d.unsupported.is_empty());
    assert!(top(&d).iter().all(|v| *v == Visibility::Hidden), "{:?}", top(&d));
}

#[test]
fn a_flat_spline_patch_is_a_plane_that_hides_for_certain() {
    // A bilinear patch with all four corners at z = 0 over a bar whose top is at z = -1.
    let p = [[-5., -5., 0.], [5., -5., 0.], [5., 5., 0.], [-5., 5., 0.]];
    let edges =
        (0..4).map(|i| Edge { vertices: [i as u32, ((i + 1) % 4) as u32], curve: Curve::Line { start: p[i], end: p[(i + 1) % 4] } }).collect();
    let face = Face {
        loops: vec![(0..4).map(|e| uses(e, true)).collect()],
        surface: Surface::Spline { u_knots: vec![0., 0., 1., 1.], v_knots: vec![0., 0., 1., 1.], points: vec![vec![p[0], p[3]], vec![p[1], p[2]]] },
        forward: true,
    };
    let patch =
        prepare(&Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: p.to_vec(), edges, faces: vec![face] }] }, &Options::default())
            .unwrap();
    assert!(patch.report.freeform_faces.is_empty());
    let bar = prepare(&cuboid([-2., -1., -3.], [2., 1., -1.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0.1, 0.05, -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[patch, bar], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert!(d.unsupported.is_empty());
    let top: Vec<Visibility> = [2, 3, 6, 7].iter().flat_map(|&e| edge_line(&d, 1, e).pieces.iter().map(|p| p.visibility)).collect();
    assert!(top.iter().all(|v| *v == Visibility::Hidden), "{top:?}");
}

/// The Bernstein coefficients of the product of two polynomials given by theirs.
fn bernstein_product(a: &[f64], b: &[f64]) -> Vec<f64> {
    let binom = |n: usize, k: usize| (0..k).fold(1f64, |acc, i| acc * (n - i) as f64 / (i + 1) as f64);
    let (m, n) = (a.len() - 1, b.len() - 1);
    (0..=m + n)
        .map(|k| {
            let terms = (k.saturating_sub(n)..=k.min(m)).map(|i| binom(m, i) * binom(n, k - i) * a[i] * b[k - i]);
            terms.sum::<f64>() / binom(m + n, k)
        })
        .collect()
}

#[test]
fn a_circle_racing_through_its_first_turn_is_drawn_whole_edge_on() {
    // A closed rational sextic on the circle of radius 10: 10 (A + iB)^6 / |A + iB|^6 with
    // A = 0.05 + 0.95 t, B = sqrt 3 t. It turns 3.6 rad in its first screen sample step, and
    // seen along its plane that step looks straight; the arcs must still turn 360 degrees.
    // A + iB = 0.05 (1 - t) + (1 + i sqrt 3) t, so (A + iB)^6 has the Bernstein coefficients
    // 0.05^(6-k) (1 + i sqrt 3)^k, and A^2 + B^2 the coefficients 0.0025, 0.05, 4.
    let c1 = (1f64, 3f64.sqrt());
    let mut num = Vec::new();
    let mut pw = (1f64, 0f64);
    for k in 0..=6 {
        num.push((0.05f64.powi(6 - k) * pw.0, 0.05f64.powi(6 - k) * pw.1));
        pw = (pw.0 * c1.0 - pw.1 * c1.1, pw.0 * c1.1 + pw.1 * c1.0);
    }
    let q = [0.0025, 0.05, 4.];
    let w = bernstein_product(&bernstein_product(&q, &q), &q);
    let homogeneous: Vec<[f64; 4]> = num.iter().zip(&w).map(|((re, im), w)| [10. * re, 10. * im, 0., *w]).collect();
    let start = [10., 0., 0.];
    let disc = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![start],
            edges: vec![Edge { vertices: [0, 0], curve: Curve::RationalSpline { knots: [[0.; 7], [1.; 7]].concat(), homogeneous } }],
            faces: vec![Face {
                loops: vec![vec![uses(0, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let def = prepare(&disc, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    if let Some(sweep) = turn_or_chords(pieces(&d)) {
        assert!((sweep - std::f64::consts::TAU).abs() < 1e-9, "the arcs turn {sweep} rad");
    }
}

#[test]
fn an_arc_racing_through_almost_a_full_turn_is_drawn_whole() {
    // 10 z^4 / |z|^4 with z = (1 - t) + 400 e^(i (pi/2 - 0.01)) t: an exact arc of radius 10 that
    // turns 2 pi - 0.04 almost all within t < 0.01, closed by a short line. Seen along its plane
    // at -0.45 rad, the first samples look straight and turn little between them; the arcs
    // must still turn the whole 2 pi - 0.04.
    let phi = std::f64::consts::FRAC_PI_2 - 0.01;
    let c1 = (400. * phi.cos(), 400. * phi.sin());
    let mut num = Vec::new();
    let mut pw = (1f64, 0f64);
    for _ in 0..=4 {
        num.push(pw);
        pw = (pw.0 * c1.0 - pw.1 * c1.1, pw.0 * c1.1 + pw.1 * c1.0);
    }
    let q = [1., c1.0, 160_000.];
    let w = bernstein_product(&q, &q);
    let homogeneous: Vec<[f64; 4]> = num.iter().zip(&w).map(|((re, im), w)| [10. * re, 10. * im, 0., *w]).collect();
    let sweep_want = 4. * phi;
    let end = [10. * sweep_want.cos(), 10. * sweep_want.sin(), 0.];
    let shape = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[10., 0., 0.], end],
            edges: vec![
                Edge { vertices: [0, 1], curve: Curve::RationalSpline { knots: [[0.; 5], [1.; 5]].concat(), homogeneous } },
                Edge { vertices: [1, 0], curve: Curve::Line { start: end, end: [10., 0., 0.] } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let def = prepare(&shape, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0.45f64.sin(), 0.45f64.cos(), 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    if let Some(sweep) = turn_or_chords(edge_line(&d, 0, 0).pieces.iter()) {
        assert!((sweep - sweep_want).abs() < 1e-9, "the arcs turn {sweep} rad, want {sweep_want}");
    }
}

#[test]
fn a_short_piece_of_a_fast_arc_turns_its_own_way() {
    // An exact arc of radius 10 as a quartic rational spline of two spans: 0 to 5 rad over the
    // parameters 0 to 1e-6, then on to 5.8 rad; a chord closes the loop. A box in front, seen
    // along -Z, cuts the arc into pieces a fraction of 1e-6 long that turn several radians.
    let span = |c0: (f64, f64), c1: (f64, f64)| -> Vec<[f64; 4]> {
        let mul = |a: (f64, f64), b: (f64, f64)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
        let pw = |c: (f64, f64), k: usize| (0..k).fold((1., 0.), |acc, _| mul(acc, c));
        let q = [c0.0 * c0.0 + c0.1 * c0.1, c0.0 * c1.0 + c0.1 * c1.1, c1.0 * c1.0 + c1.1 * c1.1];
        let w = bernstein_product(&q, &q);
        (0..=4)
            .map(|k| {
                let z = mul(pw(c0, 4 - k), pw(c1, k));
                [10. * z.0, 10. * z.1, 0., w[k]]
            })
            .collect()
    };
    let unit = |a: f64| (a.cos(), a.sin());
    // Weights far apart in the first span (|z| from 1 to 40) crowd its turn near its start.
    let fast = (40. * 1.25f64.cos(), 40. * 1.25f64.sin());
    let first = span(unit(0.), fast);
    let second = span(fast, unit(1.45));
    let homogeneous: Vec<[f64; 4]> = first.iter().chain(second.iter().skip(1)).copied().collect();
    let end = [10. * 5.8f64.cos(), 10. * 5.8f64.sin(), 0.];
    let shape = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[10., 0., 0.], end],
            edges: vec![
                Edge { vertices: [0, 1], curve: Curve::RationalSpline { knots: [vec![0.; 5], vec![1e-6; 4], vec![1.; 5]].concat(), homogeneous } },
                Edge { vertices: [1, 0], curve: Curve::Line { start: end, end: [10., 0., 0.] } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let arc = prepare(&shape, &Options::default()).unwrap();
    let block = prepare(&cuboid([-8.2, -11., 1.], [11., -0.001, 2.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[arc, block], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let line = edge_line(&d, 0, 0);
    assert!(line.pieces.len() > 1, "the block splits the arc");
    if let Some(sweep) = turn_or_chords(line.pieces.iter()) {
        assert!((sweep - 5.8).abs() < 1e-9, "the pieces turn {sweep} rad in all, want 5.8");
    }
}

/// The homogeneous control points of an exact quartic arc of radius 10 about the origin from
/// angle `a0` to `a1` (z^4 / |z|^4 with z running straight between the quarter angles).
fn quartic_arc(a0: f64, a1: f64) -> Vec<[f64; 4]> {
    let (c0, c1) = ((a0 / 4.).cos(), (a0 / 4.).sin());
    let (d0, d1) = ((a1 / 4.).cos(), (a1 / 4.).sin());
    let mul = |a: (f64, f64), b: (f64, f64)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
    let pw = |c: (f64, f64), k: usize| (0..k).fold((1., 0.), |acc, _| mul(acc, c));
    let q = [1., c0 * d0 + c1 * d1, 1.];
    let w = bernstein_product(&q, &q);
    (0..=4)
        .map(|k| {
            let z = mul(pw((c0, c1), 4 - k), pw((d0, d1), k));
            [10. * z.0, 10. * z.1, 0., w[k]]
        })
        .collect()
}

#[test]
fn an_arc_whose_whole_domain_is_8_roundings_wide_is_not_guessed() {
    // An exact arc of radius 10 turning +3.75 rad over the parameters 0.25 to 0.25 + 2^-51
    // (8 roundings), closed by a chord: drawn as the right arc or as chords, never as the short
    // arc the other way round, through (0, -10).
    let k = 0.25 + 2f64.powi(-51);
    let end = [10. * 3.75f64.cos(), 10. * 3.75f64.sin(), 0.];
    let shape = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[10., 0., 0.], end],
            edges: vec![
                Edge {
                    vertices: [0, 1],
                    curve: Curve::RationalSpline { knots: [vec![0.25; 5], vec![k; 5]].concat(), homogeneous: quartic_arc(0., 3.75) },
                },
                Edge { vertices: [1, 0], curve: Curve::Line { start: end, end: [10., 0., 0.] } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let def = prepare(&shape, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    if let Some(sweep) = turn_or_chords(edge_line(&d, 0, 0).pieces.iter()) {
        assert!((sweep - 3.75).abs() < 1e-9, "the arcs turn {sweep} rad");
    }
    for p in &edge_line(&d, 0, 0).pieces {
        for g in &p.geometry {
            for s in [0., 0.25, 0.5, 0.75, 1.] {
                assert!(g.point_at(s).dist(Vec2::new(0., -10.)) > 1., "{g:?} passes the wrong side");
            }
        }
    }
}

#[test]
fn a_piece_in_a_knot_span_of_1e_11_turns_its_own_way() {
    // An exact arc of radius 10 in three quartic spans: 0 to 0.1 rad, then 0.1 to 3.85 rad over
    // the parameters 0.25 to 0.25 + 1e-11, then on to 4.5 rad. Two blocks in front hide the
    // outer spans, so the middle one is a visible piece of its own turning 3.75 rad.
    let span = quartic_arc;
    let (s1, s2, s3) = (span(0., 0.1), span(0.1, 3.85), span(3.85, 4.5));
    let homogeneous: Vec<[f64; 4]> = s1.iter().chain(s2.iter().skip(1)).chain(s3.iter().skip(1)).copied().collect();
    let k = 0.25 + 1e-11;
    let end = [10. * 4.5f64.cos(), 10. * 4.5f64.sin(), 0.];
    let shape = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[10., 0., 0.], end],
            edges: vec![
                Edge {
                    vertices: [0, 1],
                    curve: Curve::RationalSpline { knots: [vec![0.; 5], vec![0.25; 4], vec![k; 4], vec![1.; 5]].concat(), homogeneous },
                },
                Edge { vertices: [1, 0], curve: Curve::Line { start: end, end: [10., 0., 0.] } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let arc = prepare(&shape, &Options::default()).unwrap();
    let low = prepare(&cuboid([9., -1., 1.], [11., 10. * 0.1f64.sin(), 2.]), &Options::default()).unwrap();
    let high = prepare(&cuboid([-11., -11., 1.], [0., 10. * 3.85f64.sin(), 2.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let scene = [identity(0, [0.; 3]), identity(1, [0.; 3]), identity(2, [0.; 3])];
    let d = hidden_lines(&[arc, low, high], &scene, &view, &Options::default(), &never).unwrap();
    let line = edge_line(&d, 0, 0);
    let mut sweep = 0.;
    for p in &line.pieces {
        for g in &p.geometry {
            match *g {
                Seg2::Conic { w, .. } => sweep += 2. * w.acos(),
                _ => panic!("an arc is drawn as conic arcs: {g:?}"),
            }
        }
    }
    assert!(line.pieces.len() >= 3, "{:?}", line.pieces.iter().map(|p| p.t).collect::<Vec<_>>());
    assert!((sweep - 4.5).abs() < 1e-9, "the pieces turn {sweep} rad in all, want 4.5");
}

#[test]
fn a_curve_that_swerves_where_its_weight_is_near_zero_is_not_taken_for_a_circle() {
    // 1e-12 (1 - t)^3 (0, -10) + t Q(t), Q the exact quarter circle of radius 10 from (10, 0)
    // to (0, 10): on the circle at every check point to 1.3e-9 mm, yet near t = 1e-12, where
    // its weight is tiny, it passes (5, -5), 2.9 mm inside. Drawn as chords, not an arc.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let q = [[10., 0., 0., 1.], [10. * s, 10. * s, 0., s], [0., 10., 0., 1.]];
    let homogeneous: Vec<[f64; 4]> = (0..4)
        .map(|k| {
            let tq: Vec<f64> = (0..4).map(|c| bernstein_product(&[0., 1.], &q.map(|h| h[c]))[k]).collect();
            let a = if k == 0 { [0., -10e-12, 0., 1e-12] } else { [0.; 4] };
            [tq[0] + a[0], tq[1] + a[1], tq[2] + a[2], tq[3] + a[3]]
        })
        .collect();
    let shape = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[0., -10., 0.], [0., 10., 0.]],
            edges: vec![
                Edge { vertices: [0, 1], curve: Curve::RationalSpline { knots: [[0.; 4], [1.; 4]].concat(), homogeneous } },
                Edge { vertices: [1, 0], curve: Curve::Line { start: [0., 10., 0.], end: [0., -10., 0.] } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let def = prepare(&shape, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert!(edge_line(&d, 0, 0).pieces.iter().all(|p| matches!(p.accuracy, CurveAccuracy::Sampled { .. })), "{d:?}");
}

#[test]
fn an_edge_leaving_a_left_out_faces_box_in_front_stays_visible() {
    // Over the twisted patch (x, y in [-5, 5], z in [0, 1]), seen from above, an edge from
    // (-10, 0, -1) up to (0, 0, 5): below the patch's level only outside its box, inside the
    // box above the patch. Nothing can hide it.
    let patch = prepare(&twisted_patch(), &Options::default()).unwrap();
    let c = [[-10., 0., -1.], [0., 0., 5.], [0., 0.2, 5.], [-10., 0.2, -1.]];
    let edges =
        (0..4).map(|i| Edge { vertices: [i as u32, ((i + 1) % 4) as u32], curve: Curve::Line { start: c[i], end: c[(i + 1) % 4] } }).collect();
    let face = Face {
        loops: vec![(0..4).map(|e| uses(e, true)).collect()],
        surface: Surface::Plane { origin: c[0], u_axis: [10., 0., 6.], v_axis: [0., 0.2, 0.] },
        forward: true,
    };
    let strip =
        prepare(&Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: c.to_vec(), edges, faces: vec![face] }] }, &Options::default())
            .unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[patch, strip], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let states: Vec<([f64; 2], Visibility)> = edge_line(&d, 1, 0).pieces.iter().map(|p| (p.t, p.visibility)).collect();
    assert!(states.iter().all(|(_, v)| *v == Visibility::Visible), "{states:?}");
}

#[test]
fn a_nearly_straight_cubic_is_not_taken_for_a_huge_circle() {
    // x = 1000 t, y = 0.071 t - 0.113 t^2 + 0.042 t^3: within 0.002 mm of a circle of radius
    // about 1e7, which is 2e-10 of that radius but 2e-6 of the curve. Drawn as chords.
    let pts = [[0., 0., 0.], [1000. / 3., 0.071 / 3., 0.], [2000. / 3., 0.071 * 2. / 3. - 0.113 / 3., 0.], [1000., 0., 0.]];
    let shape = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[0.; 3], [1000., 0., 0.]],
            edges: vec![
                Edge { vertices: [0, 1], curve: Curve::Spline { knots: [[0.; 4], [1.; 4]].concat(), points: pts.to_vec() } },
                Edge { vertices: [1, 0], curve: Curve::Line { start: [1000., 0., 0.], end: [0.; 3] } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let def = prepare(&shape, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert!(edge_line(&d, 0, 0).pieces.iter().all(|p| matches!(p.accuracy, CurveAccuracy::Sampled { .. })), "{d:?}");
}

#[test]
fn an_edge_dipping_behind_a_left_out_face_between_samples_is_uncertain() {
    // Under a freeform patch over x in [0, 10], y in [0, 1] (z within 1e-4 of zero, zero along
    // y = 0.5), a quadratic edge x = 10 t, y = 0.5, z = 20 (t - 0.0075)(t - 0.02375) dips
    // below the patch for t from 0.0075 to 0.02375, between its samples; its projection is
    // straight, so the screen never asks for more.
    let e = 1e-4;
    let p = [[0., 0., e], [10., 0., -e], [10., 1., e], [0., 1., -e]];
    let edges =
        (0..4).map(|i| Edge { vertices: [i as u32, ((i + 1) % 4) as u32], curve: Curve::Line { start: p[i], end: p[(i + 1) % 4] } }).collect();
    let face = Face {
        loops: vec![(0..4).map(|e| uses(e, true)).collect()],
        surface: Surface::Spline { u_knots: vec![0., 0., 1., 1.], v_knots: vec![0., 0., 1., 1.], points: vec![vec![p[0], p[3]], vec![p[1], p[2]]] },
        forward: true,
    };
    let patch =
        prepare(&Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: p.to_vec(), edges, faces: vec![face] }] }, &Options::default())
            .unwrap();
    assert_eq!(patch.report.freeform_faces, vec![0]);
    // z = 20 t^2 - 0.625 t + 0.0035625 as a quadratic Bezier.
    let z = [0.0035625, 0.0035625 - 0.3125, 20. - 0.625 + 0.0035625];
    let c = [[0., 0.5, z[0]], [5., 0.5, z[1]], [10., 0.5, z[2]]];
    let shape = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![c[0], c[2]],
            edges: vec![
                Edge { vertices: [0, 1], curve: Curve::Spline { knots: vec![0., 0., 0., 1., 1., 1.], points: c.to_vec() } },
                Edge { vertices: [1, 0], curve: Curve::Line { start: c[2], end: c[0] } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: c[0], u_axis: [1., 0., 0.], v_axis: [0., 0., 1.] },
                forward: true,
            }],
        }],
    };
    let curve = prepare(&shape, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[patch, curve], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let dip: Vec<&Piece> = edge_line(&d, 1, 0).pieces.iter().filter(|p| p.t[0] < 0.02 && p.t[1] > 0.01).collect();
    assert!(!dip.is_empty());
    assert!(dip.iter().all(|p| p.visibility != Visibility::Visible), "{dip:?}");
}

#[test]
fn an_arc_that_turns_back_is_drawn_as_chords() {
    // (10 (1 - u^2), 20 u) / (1 + u^2) with u = 10 (t^3 / 3 - t^2 / 4 + 0.04 t): on the circle
    // of radius 10, but u falls between t = 0.1 and 0.4, so the edge goes back down to
    // y = -0.533 before it turns on to 1.78 rad. No single arc draws that: chords, which reach
    // down there.
    let u3 = [0., 0.4 / 3., 0.8 / 3. - 2.5 / 3., 0.4 - 2.5 + 10. / 3.];
    let u = bernstein_product(&u3, &[1.; 4]);
    let uu = bernstein_product(&u3, &u3);
    let homogeneous: Vec<[f64; 4]> = (0..7).map(|k| [10. * (1. - uu[k]), 20. * u[k], 0., 1. + uu[k]]).collect();
    let th = 2. * u3[3].atan();
    let end = [10. * th.cos(), 10. * th.sin(), 0.];
    let shape = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[10., 0., 0.], end],
            edges: vec![
                Edge { vertices: [0, 1], curve: Curve::RationalSpline { knots: [[0.; 7], [1.; 7]].concat(), homogeneous } },
                Edge { vertices: [1, 0], curve: Curve::Line { start: end, end: [10., 0., 0.] } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let def = prepare(&shape, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let line = edge_line(&d, 0, 0);
    assert!(line.pieces.iter().all(|p| matches!(p.accuracy, CurveAccuracy::Sampled { .. })), "{:?}", line.pieces);
    let low = line.pieces.iter().flat_map(|p| p.geometry.iter()).map(|g| g.point_at(0.).y.min(g.point_at(1.).y)).fold(f64::MAX, f64::min);
    assert!(low < -0.5, "the chords reach down to y = {low}");
}

#[test]
fn a_cone_tip_trimmed_near_the_apex_is_only_its_tip() {
    // A cone record of slope 0.25 from its apex up to 10 m, trimmed by a circle at z = 1 to the
    // tip below it: a box edge at x = 40, z = 50 to 150, seen along +X, is in front of nothing.
    let tau = std::f64::consts::TAU;
    let tip = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[0.25, 0., 1.]],
            edges: vec![circle([0., 0., 1.], 0.25, 0)],
            faces: vec![Face {
                loops: vec![vec![uses(0, false)]],
                surface: Surface::Cone {
                    frame: frame_at([0.; 3]),
                    radius_at_origin: 0.,
                    half_angle: 0.25f64.atan(),
                    angle: [0., tau],
                    height: [0., 10_000.],
                },
                forward: true,
            }],
        }],
    };
    let tip = prepare(&tip, &Options::default()).unwrap();
    let bar = prepare(&cuboid([40., -1., 50.], [41., 1., 150.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(1., 0., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[tip, bar], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    // Edges 8 and 10 run along z at x = 40, the side facing the viewer.
    for e in [8, 10] {
        let states: Vec<Visibility> = edge_line(&d, 1, e).pieces.iter().map(|p| p.visibility).collect();
        assert!(states.iter().all(|v| *v == Visibility::Visible), "edge {e}: {states:?}");
    }
}

#[test]
fn an_edge_behind_a_left_out_freeform_face_is_uncertain_there() {
    // A freeform tube about the vertical line x = 10 (radius 2.5 at its ends, 2.75 halfway up,
    // a quadratic profile) in front of a long box edge at z = 5. Its silhouettes are left out,
    // so nothing cuts the edge where the tube covers it (x from 7.25 to 12.75); that part may
    // be hidden and must not come out visible.
    let tau = std::f64::consts::TAU;
    let profile = Curve::Spline { knots: vec![0., 0., 0., 1., 1., 1.], points: vec![[12.5, 0., 0.], [13., 0., 5.], [12.5, 0., 10.]] };
    let tube = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[12.5, 0., 0.], [12.5, 0., 10.]],
            edges: vec![circle([10., 0., 0.], 2.5, 0), circle([10., 0., 10.], 2.5, 1), Edge { vertices: [0, 1], curve: profile.clone() }],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(2, true), uses(1, false), uses(2, false)]],
                surface: Surface::Revolution { profile: Box::new(profile), origin: [10., 0., 0.], axis: [0., 0., 1.], angle: [0., tau] },
                forward: true,
            }],
        }],
    };
    let tube = prepare(&tube, &Options::default()).unwrap();
    assert_eq!(tube.report.freeform_faces, vec![0]);
    let bar = prepare(&cuboid([-50., 4., 4.], [50., 6., 5.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[tube, bar], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    // Edge 2 joins corners 4 (-50, 4, 5) and 5 (50, 4, 5): x = 100 t - 50.
    for p in &edge_line(&d, 1, 2).pieces {
        let (x0, x1) = (100. * p.t[0] - 50., 100. * p.t[1] - 50.);
        if x1 > 7.25 && x0 < 12.75 {
            assert_ne!(p.visibility, Visibility::Visible, "x {x0}..{x1}");
        }
        if x1 < 6. || x0 > 14. {
            assert_eq!(p.visibility, Visibility::Visible, "x {x0}..{x1}");
        }
    }
}

#[test]
fn a_sloping_edge_that_runs_behind_a_left_out_face_is_uncertain() {
    // Over the twisted patch (z = 0.5 along y = 0), seen from above, a sloping strip's edge runs
    // from (-4, 0, 2) down to (4, 0, 0.4): in front of the patch at its middle, behind it past
    // x = 3.5. All of it lies in the patch's screen box, so nothing cuts it there.
    let patch = prepare(&twisted_patch(), &Options::default()).unwrap();
    let c = [[-4., 0., 2.], [4., 0., 0.4], [4., 0.2, 0.4], [-4., 0.2, 2.]];
    let edges =
        (0..4).map(|i| Edge { vertices: [i as u32, ((i + 1) % 4) as u32], curve: Curve::Line { start: c[i], end: c[(i + 1) % 4] } }).collect();
    let face = Face {
        loops: vec![(0..4).map(|e| uses(e, true)).collect()],
        surface: Surface::Plane { origin: c[0], u_axis: [8., 0., -1.6], v_axis: [0., 0.2, 0.] },
        forward: true,
    };
    let strip =
        prepare(&Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: c.to_vec(), edges, faces: vec![face] }] }, &Options::default())
            .unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[patch, strip], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    for p in &edge_line(&d, 1, 0).pieces {
        if 8. * p.t[1] - 4. > 3.6 {
            assert_ne!(p.visibility, Visibility::Visible, "{p:?}");
        }
    }
}

#[test]
fn a_torus_stored_as_a_rational_spline_patch_is_recognised() {
    // The patch of a torus R 20, r 5 with longitude and meridian both from -pi/4 to pi/4, as a
    // degree (2, 2) rational spline, seen along the axis: a torus, not a freeform face, and the
    // outer equator inside it is an exact silhouette arc of radius 25.
    let (big, small) = (20., 5.);
    let q = std::f64::consts::FRAC_PI_4;
    let s2 = std::f64::consts::SQRT_2;
    // The unit arc from -45 to 45 degrees: its control points and weights.
    let arc = [([q.cos(), -q.sin()], 1.), ([s2, 0.], q.cos()), ([q.cos(), q.sin()], 1.)];
    let homogeneous: Vec<Vec<[f64; 4]>> = arc
        .iter()
        .map(|(ci, wi)| {
            arc.iter()
                .map(|(mj, wj)| {
                    let (rho, h, w) = (big + small * mj[0], small * mj[1], wi * wj);
                    [rho * ci[0] * w, rho * ci[1] * w, h * w, w]
                })
                .collect()
        })
        .collect();
    let at = |t: f64, f: f64| [(big + small * f.cos()) * t.cos(), (big + small * f.cos()) * t.sin(), small * f.sin()];
    let parallel = |f: f64| crate::brep::Frame { origin: [0., 0., small * f.sin()], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let meridian = |t: f64| crate::brep::Frame { origin: [big * t.cos(), big * t.sin(), 0.], x_axis: [t.cos(), t.sin(), 0.], y_axis: [0., 0., 1.] };
    let edges = vec![
        Edge { vertices: [0, 1], curve: Curve::CircleArc { frame: parallel(-q), radius: big + small * q.cos(), angle: [-q, q] } },
        Edge { vertices: [1, 2], curve: Curve::CircleArc { frame: meridian(q), radius: small, angle: [-q, q] } },
        Edge { vertices: [3, 2], curve: Curve::CircleArc { frame: parallel(q), radius: big + small * q.cos(), angle: [-q, q] } },
        Edge { vertices: [0, 3], curve: Curve::CircleArc { frame: meridian(-q), radius: small, angle: [-q, q] } },
    ];
    let face = Face {
        loops: vec![vec![uses(0, true), uses(1, true), uses(2, false), uses(3, false)]],
        surface: Surface::RationalSpline { u_knots: vec![0., 0., 0., 1., 1., 1.], v_knots: vec![0., 0., 0., 1., 1., 1.], homogeneous },
        forward: true,
    };
    let patch = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell { vertices: vec![at(-q, -q), at(q, -q), at(q, q), at(-q, q)], edges, faces: vec![face] }],
    };
    let def = prepare(&patch, &Options::default()).unwrap();
    assert!(def.report.freeform_faces.is_empty(), "{:?}", def.report);
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert!(d.unsupported.is_empty());
    let mut length = 0.;
    for p in silhouette_pieces(&d) {
        assert_eq!(p.accuracy, CurveAccuracy::Analytic);
        for g in &p.geometry {
            for k in 0..=8 {
                assert!((g.point_at(k as f64 / 8.).len() - 25.).abs() < 1e-6, "{g:?}");
            }
            length += (0..16).map(|k| g.point_at(k as f64 / 16.).dist(g.point_at((k + 1) as f64 / 16.))).sum::<f64>();
        }
    }
    assert!((length - 25. * std::f64::consts::FRAC_PI_2).abs() < 0.01, "{length} mm of outline");
}

#[test]
fn a_torus_patch_with_a_bump_in_a_narrow_knot_span_is_not_taken_for_a_torus() {
    // The torus patch R 20, r 5 again, its longitude in three spans, the middle one 0.002 of
    // the parameters wide; one control point there pushed out by 0.2 %. Between the sample
    // grid's points, but not between the checks on every knot span.
    let (big, small) = (20., 5.);
    let q = std::f64::consts::FRAC_PI_4;
    let s2 = std::f64::consts::SQRT_2;
    let ts = [-q, -q + 0.3, -q + 0.5, q];
    let mut lon: Vec<([f64; 2], f64)> = vec![([ts[0].cos(), ts[0].sin()], 1.)];
    for w in ts.windows(2) {
        let (m, h) = (0.5 * (w[0] + w[1]), 0.5 * (w[1] - w[0]));
        lon.push(([m.cos() / h.cos(), m.sin() / h.cos()], h.cos()));
        lon.push(([w[1].cos(), w[1].sin()], 1.));
    }
    let mer = [([q.cos(), -q.sin()], 1.), ([s2, 0.], q.cos()), ([q.cos(), q.sin()], 1.)];
    let homogeneous: Vec<Vec<[f64; 4]>> = lon
        .iter()
        .enumerate()
        .map(|(i, (ci, wi))| {
            mer.iter()
                .enumerate()
                .map(|(j, (mj, wj))| {
                    let bump = if (i, j) == (3, 1) { 1.002 } else { 1. };
                    let (rho, h, w) = ((big + small * mj[0]) * bump, small * mj[1], wi * wj);
                    [rho * ci[0] * w, rho * ci[1] * w, h * w, w]
                })
                .collect()
        })
        .collect();
    let at = |t: f64, f: f64| [(big + small * f.cos()) * t.cos(), (big + small * f.cos()) * t.sin(), small * f.sin()];
    let parallel = |f: f64| crate::brep::Frame { origin: [0., 0., small * f.sin()], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let meridian = |t: f64| crate::brep::Frame { origin: [big * t.cos(), big * t.sin(), 0.], x_axis: [t.cos(), t.sin(), 0.], y_axis: [0., 0., 1.] };
    let edges = vec![
        Edge { vertices: [0, 1], curve: Curve::CircleArc { frame: parallel(-q), radius: big + small * q.cos(), angle: [-q, q] } },
        Edge { vertices: [1, 2], curve: Curve::CircleArc { frame: meridian(q), radius: small, angle: [-q, q] } },
        Edge { vertices: [3, 2], curve: Curve::CircleArc { frame: parallel(q), radius: big + small * q.cos(), angle: [-q, q] } },
        Edge { vertices: [0, 3], curve: Curve::CircleArc { frame: meridian(-q), radius: small, angle: [-q, q] } },
    ];
    let face = Face {
        loops: vec![vec![uses(0, true), uses(1, true), uses(2, false), uses(3, false)]],
        surface: Surface::RationalSpline {
            u_knots: vec![0., 0., 0., 0.1, 0.1, 0.102, 0.102, 1., 1., 1.],
            v_knots: vec![0., 0., 0., 1., 1., 1.],
            homogeneous,
        },
        forward: true,
    };
    let patch = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell { vertices: vec![at(-q, -q), at(q, -q), at(q, q), at(-q, q)], edges, faces: vec![face] }],
    };
    let def = prepare(&patch, &Options::default()).unwrap();
    assert_eq!(def.report.freeform_faces, vec![0], "{:?}", def.report);
}

#[test]
fn a_big_circle_passing_behind_a_thin_left_out_face_is_uncertain_there() {
    // A disc of radius 5000 whose edge passes under a freeform tube (radius 0.01 to about
    // 0.02, a cubic profile) lying along Y over (5000, 0), with a cube 9 km away making the scene coarse. The edge is turned so
    // that its samples on either side of the tube are 30 mm away at x = 4999.905: the chord
    // between them misses the tube's box by less than its sag, crossing none of its sides.
    let tau = std::f64::consts::TAU;
    let turn = 8.5 * std::f64::consts::PI / 256.;
    let (c, sn) = (turn.cos(), turn.sin());
    let frame = crate::brep::Frame { origin: [0.; 3], x_axis: [-c, -sn, 0.], y_axis: [sn, -c, 0.] };
    let disc = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[-5000. * c, -5000. * sn, 0.]],
            edges: vec![Edge { vertices: [0, 0], curve: Curve::CircleArc { frame, radius: 5000., angle: [0., tau] } }],
            faces: vec![Face {
                loops: vec![vec![uses(0, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let profile = Curve::Spline {
        knots: vec![0., 0., 0., 0., 1., 1., 1., 1.],
        points: vec![[5000.01, -100., 1.], [5000.03, -33., 1.], [5000.01, 33., 1.], [5000.01, 100., 1.]],
    };
    let end = |y: f64, v: u32| Edge {
        vertices: [v, v],
        curve: Curve::CircleArc {
            frame: crate::brep::Frame { origin: [5000., y, 1.], x_axis: [1., 0., 0.], y_axis: [0., 0., 1.] },
            radius: 0.01,
            angle: [0., tau],
        },
    };
    let tube = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[5000.01, -100., 1.], [5000.01, 100., 1.]],
            edges: vec![end(-100., 0), end(100., 1), Edge { vertices: [0, 1], curve: profile.clone() }],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(2, true), uses(1, false), uses(2, false)]],
                surface: Surface::Revolution { profile: Box::new(profile), origin: [5000., 0., 1.], axis: [0., 1., 0.], angle: [0., tau] },
                forward: true,
            }],
        }],
    };
    let defs = [
        prepare(&tube, &Options::default()).unwrap(),
        prepare(&disc, &Options::default()).unwrap(),
        prepare(&cuboid([9e6, 0., 0.], [9e6 + 1., 1., 1.]), &Options::default()).unwrap(),
    ];
    assert_eq!(defs[0].report.freeform_faces, vec![0]);
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&defs, &[identity(0, [0.; 3]), identity(1, [0.; 3]), identity(2, [0.; 3])], &view, &Options::default(), &never).unwrap();
    // The disc's edge reaches (5000, 0) at the parameter pi - turn.
    let at = std::f64::consts::PI - turn;
    let under: Vec<&Piece> = edge_line(&d, 1, 0).pieces.iter().filter(|p| p.t[0] <= at && p.t[1] >= at).collect();
    assert!(!under.is_empty());
    assert!(under.iter().all(|p| p.visibility != Visibility::Visible), "{under:?}");
}

#[test]
fn a_torus_in_two_halves_keeps_whole_outlines_with_fine_trims() {
    // Seen along (0, 1, -1) the traced outline's chords stand up to their deviation off the
    // torus; with a finer trim (768 samples a span) the boundary tolerance is smaller than
    // that, and the outline must still not break into gaps.
    let def = |n: usize| prepare(&torus(20., 5.), &Options { trim_samples: n, ..Options::default() }).unwrap();
    let view = View::new(Vec3::new(0., 1., -1.), Vec3::new(0., 0., 1.)).unwrap();
    let count = |n: usize| {
        hidden_lines(&[def(n)], &[identity(0, [0.; 3])], &view, &Options { trim_samples: n, ..Options::default() }, &never).unwrap().stats.silhouettes
    };
    assert_eq!(count(768), count(256));
}

#[test]
fn an_arc_folding_back_under_a_left_out_face_seen_edge_on_is_uncertain_there() {
    // An arc of radius 10000 in the XZ plane, seen from above: its projection lies on the X axis
    // and turns back at x = 10000 a quarter of the way into its first sample step, so the chord
    // between its first samples (x 9999.953 and 9999.577) has no sag and stops short of the
    // turn. A tiny freeform patch above x = 10000 covers the turn. The arc's angles are counted
    // from 0 and from 8 pi.
    for turns in [0., 4.] {
        folding_arc_under_a_patch(turns * std::f64::consts::TAU);
    }
}

fn folding_arc_under_a_patch(offset: f64) {
    let step = std::f64::consts::PI / 256.;
    let ta = -step / 4. + offset;
    let frame = crate::brep::Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 0., 1.] };
    let at = |t: f64| [10000. * t.cos(), 0., 10000. * t.sin()];
    let (start, end) = (at(ta), at(ta + std::f64::consts::PI / 8.));
    let arc = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![start, end],
            edges: vec![
                Edge { vertices: [0, 1], curve: Curve::CircleArc { frame, radius: 10000., angle: [ta, ta + std::f64::consts::PI / 8.] } },
                Edge { vertices: [1, 0], curve: Curve::Line { start: end, end: start } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 0., 1.] },
                forward: true,
            }],
        }],
    };
    let (x0, x1, y0, y1) = (9999.98, 10000.02, -0.02, 0.02);
    let p = [[x0, y0, 1.], [x1, y0, 1.0004], [x1, y1, 1.], [x0, y1, 1.0004]];
    let edges =
        (0..4).map(|i| Edge { vertices: [i as u32, ((i + 1) % 4) as u32], curve: Curve::Line { start: p[i], end: p[(i + 1) % 4] } }).collect();
    let face = Face {
        loops: vec![(0..4).map(|e| uses(e, true)).collect()],
        surface: Surface::Spline { u_knots: vec![0., 0., 1., 1.], v_knots: vec![0., 0., 1., 1.], points: vec![vec![p[0], p[3]], vec![p[1], p[2]]] },
        forward: true,
    };
    let patch = Brep { version: crate::brep::VERSION, shells: vec![Shell { vertices: p.to_vec(), edges, faces: vec![face] }] };
    let defs = [prepare(&patch, &Options::default()).unwrap(), prepare(&arc, &Options::default()).unwrap()];
    assert_eq!(defs[0].report.freeform_faces, vec![0]);
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&defs, &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let turn: Vec<&Piece> = edge_line(&d, 1, 0).pieces.iter().filter(|p| p.t[0] <= offset && p.t[1] >= offset).collect();
    assert!(!turn.is_empty());
    assert!(turn.iter().all(|p| p.visibility != Visibility::Visible), "{turn:?}");
}

#[test]
fn a_left_out_face_curling_over_its_own_edge_makes_it_uncertain() {
    // A freeform patch X = 1000 u (u - 0.5), Y = 8000 v, Z = 0.01 u seen from above: at u = 0.5
    // it passes back over its own edge u = 0, 0.005 mm above it, closer than the own-face
    // margin of the ray test. The edge may be hidden.
    let (a, b, c) = ([0., 0.], [-250., 0.005], [500., 0.01]);
    let at = |q: [f64; 2], y: f64| [q[0], y, q[1]];
    let edges = vec![
        Edge { vertices: [0, 1], curve: Curve::Spline { knots: vec![0., 0., 0., 1., 1., 1.], points: vec![at(a, 0.), at(b, 0.), at(c, 0.)] } },
        Edge { vertices: [1, 2], curve: Curve::Line { start: at(c, 0.), end: at(c, 8000.) } },
        Edge {
            vertices: [3, 2],
            curve: Curve::Spline { knots: vec![0., 0., 0., 1., 1., 1.], points: vec![at(a, 8000.), at(b, 8000.), at(c, 8000.)] },
        },
        Edge { vertices: [0, 3], curve: Curve::Line { start: at(a, 0.), end: at(a, 8000.) } },
    ];
    let face = Face {
        loops: vec![vec![uses(0, true), uses(1, true), uses(2, false), uses(3, false)]],
        surface: Surface::Spline {
            u_knots: vec![0., 0., 0., 1., 1., 1.],
            v_knots: vec![0., 0., 1., 1.],
            points: vec![vec![at(a, 0.), at(a, 8000.)], vec![at(b, 0.), at(b, 8000.)], vec![at(c, 0.), at(c, 8000.)]],
        },
        forward: true,
    };
    let sheet = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell { vertices: vec![at(a, 0.), at(c, 0.), at(c, 8000.), at(a, 8000.)], edges, faces: vec![face] }],
    };
    let def = prepare(&sheet, &Options::default()).unwrap();
    assert_eq!(def.report.freeform_faces, vec![0]);
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let states: Vec<Visibility> = edge_line(&d, 0, 3).pieces.iter().map(|p| p.visibility).collect();
    assert!(states.iter().all(|v| *v != Visibility::Visible), "{states:?}");
}

#[test]
fn a_plate_just_above_a_box_in_a_large_scene_is_not_looked_through() {
    // A box with its top at z = 0, a larger plate 0.005 mm above it and a cube 1e7 mm away:
    // the box's top edges are under the plate and must not come out visible.
    let defs = [
        prepare(&cuboid([0., 0., -10.], [10., 10., 0.]), &Options::default()).unwrap(),
        prepare(&cuboid([-5., -5., 0.005], [15., 15., 0.007]), &Options::default()).unwrap(),
        prepare(&cuboid([1e7, 0., 0.], [1e7 + 1., 1., 1.]), &Options::default()).unwrap(),
    ];
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&defs, &[identity(0, [0.; 3]), identity(1, [0.; 3]), identity(2, [0.; 3])], &view, &Options::default(), &never).unwrap();
    // Edges 2, 3, 6 and 7 of the box run along its top face.
    for e in [2, 3, 6, 7] {
        let states: Vec<Visibility> = edge_line(&d, 0, e).pieces.iter().map(|p| p.visibility).collect();
        assert!(states.iter().all(|v| *v != Visibility::Visible), "edge {e}: {states:?}");
    }
}

#[test]
fn a_sphere_with_a_spline_band_between_record_caps_is_one_sphere() {
    // A sphere of radius 10: a rational spline band all the way round between the latitudes
    // -45 and 45 degrees, and two sphere records as caps, meeting at circles of radius 7.07.
    // The band is recognised as the same sphere, so those circles are seams: seen along the
    // axis only the outline of radius 10 is drawn.
    let (r, q, s) = (10., std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_1_SQRT_2);
    let ring = [
        ([1., 0.], 1.),
        ([1., 1.], s),
        ([0., 1.], 1.),
        ([-1., 1.], s),
        ([-1., 0.], 1.),
        ([-1., -1.], s),
        ([0., -1.], 1.),
        ([1., -1.], s),
        ([1., 0.], 1.),
    ];
    let lat = [([r * q.cos(), -r * q.sin()], 1.), ([r / q.cos(), 0.], q.cos()), ([r * q.cos(), r * q.sin()], 1.)];
    let homogeneous: Vec<Vec<[f64; 4]>> = ring
        .iter()
        .map(|(ci, wi)| lat.iter().map(|(pj, wj)| [pj[0] * ci[0] * wi * wj, pj[0] * ci[1] * wi * wj, pj[1] * wi * wj, wi * wj]).collect())
        .collect();
    let z = r * q.sin();
    let rho = r * q.cos();
    let tau = std::f64::consts::TAU;
    let half = std::f64::consts::FRAC_PI_2;
    let cap = |latitude: [f64; 2], e: u32, forward: bool| Face {
        loops: vec![vec![uses(e, forward)]],
        surface: Surface::Sphere { frame: frame_at([0.; 3]), radius: r, longitude: [0., tau], latitude },
        forward: true,
    };
    let band = Face {
        loops: vec![vec![uses(0, true)], vec![uses(1, false)]],
        surface: Surface::RationalSpline {
            u_knots: vec![0., 0., 0., 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1., 1., 1.],
            v_knots: vec![0., 0., 0., 1., 1., 1.],
            homogeneous,
        },
        forward: true,
    };
    let body = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[rho, 0., -z], [rho, 0., z]],
            edges: vec![circle([0., 0., -z], rho, 0), circle([0., 0., z], rho, 1)],
            faces: vec![band, cap([q, half], 1, true), cap([-half, -q], 0, false)],
        }],
    };
    let def = prepare(&body, &Options::default()).unwrap();
    assert_eq!(def.report.surfaces.get("sphere"), Some(&3), "{:?}", def.report);
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    for p in pieces(&d).filter(|p| p.visibility == Visibility::Visible && !p.covered) {
        for g in &p.geometry {
            let radius = g.point_at(0.5).len();
            assert!((radius - 10.).abs() < 1e-6, "a line at radius {radius}");
        }
    }
}

#[test]
fn chords_of_a_racing_circle_stay_within_their_stated_deviation() {
    // z^6 / |z|^6 with z = 0.01 (1 - t) + (1 + i sqrt 3) t: the unit circle, its weights
    // 1e-12 to 64 apart, so drawn as chords; a cube 9 km away makes the scene coarse. Every
    // point of the circle must lie within the chords' stated deviation of them.
    let c1 = (1f64, 3f64.sqrt());
    let mut num = Vec::new();
    let mut pw = (1f64, 0f64);
    for k in 0..=6 {
        num.push((0.01f64.powi(6 - k) * pw.0, 0.01f64.powi(6 - k) * pw.1));
        pw = (pw.0 * c1.0 - pw.1 * c1.1, pw.0 * c1.1 + pw.1 * c1.0);
    }
    let q = [1e-4, 0.01, 4.];
    let w = bernstein_product(&bernstein_product(&q, &q), &q);
    let homogeneous: Vec<[f64; 4]> = num.iter().zip(&w).map(|((re, im), w)| [*re, *im, 0., *w]).collect();
    let disc = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[1., 0., 0.]],
            edges: vec![Edge { vertices: [0, 0], curve: Curve::RationalSpline { knots: [[0.; 7], [1.; 7]].concat(), homogeneous } }],
            faces: vec![Face {
                loops: vec![vec![uses(0, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let defs = [prepare(&disc, &Options::default()).unwrap(), prepare(&cuboid([9e6, 0., 0.], [9e6 + 1., 1., 1.]), &Options::default()).unwrap()];
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&defs, &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let line = edge_line(&d, 0, 0);
    if turn_or_chords(line.pieces.iter()).is_some() {
        return;
    }
    let dev = line
        .pieces
        .iter()
        .filter_map(|p| if let CurveAccuracy::Sampled { max_sample_deviation_mm: v } = p.accuracy { Some(v) } else { None })
        .fold(0., f64::max);
    let chords: Vec<(Vec2, Vec2)> =
        line.pieces.iter().flat_map(|p| p.geometry.iter()).filter_map(|g| if let Seg2::Line { a, b } = *g { Some((a, b)) } else { None }).collect();
    for k in 0..720 {
        let t = std::f64::consts::TAU * k as f64 / 720.;
        let q = Vec2::new(t.cos(), t.sin());
        let gap = chords.iter().map(|(a, b)| point_segment([q.x, q.y], [a.x, a.y], [b.x, b.y])).fold(f64::MAX, f64::min);
        assert!(gap <= dev + 1e-6, "({}, {}) is {gap} mm from the chords, stated {dev}", q.x, q.y);
    }
}

#[test]
fn a_big_arc_is_cut_where_it_meets_a_left_out_faces_box() {
    // An arc of radius 10000 (angles -pi/8 to 0) under a freeform tube lying along Y at
    // x = 9990 (radius about 2.5, a cubic profile), a cube 9 km away making the scene coarse:
    // no visible piece of the arc may reach under the tube.
    let tau = std::f64::consts::TAU;
    let a0 = -std::f64::consts::PI / 8.;
    let (start, end) = ([10000. * a0.cos(), 10000. * a0.sin(), 0.], [10000., 0., 0.]);
    let arc = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![start, end],
            edges: vec![
                Edge { vertices: [0, 1], curve: Curve::CircleArc { frame: frame_at([0.; 3]), radius: 10000., angle: [a0, 0.] } },
                Edge { vertices: [1, 0], curve: Curve::Line { start: end, end: start } },
            ],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(1, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let x = 9990.;
    let profile = Curve::Spline {
        knots: vec![0., 0., 0., 0., 1., 1., 1., 1.],
        points: vec![[x + 2.5, -4000., 1.], [x + 2.504, -4000. / 3., 1.], [x + 2.5, 2000. / 3., 1.], [x + 2.5, 3000., 1.]],
    };
    let end_circle = |y: f64, v: u32| Edge {
        vertices: [v, v],
        curve: Curve::CircleArc {
            frame: crate::brep::Frame { origin: [x, y, 1.], x_axis: [1., 0., 0.], y_axis: [0., 0., 1.] },
            radius: 2.5,
            angle: [0., tau],
        },
    };
    let tube = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[x + 2.5, -4000., 1.], [x + 2.5, 3000., 1.]],
            edges: vec![end_circle(-4000., 0), end_circle(3000., 1), Edge { vertices: [0, 1], curve: profile.clone() }],
            faces: vec![Face {
                loops: vec![vec![uses(0, true), uses(2, true), uses(1, false), uses(2, false)]],
                surface: Surface::Revolution { profile: Box::new(profile), origin: [x, 0., 1.], axis: [0., 1., 0.], angle: [0., tau] },
                forward: true,
            }],
        }],
    };
    let defs = [
        prepare(&tube, &Options::default()).unwrap(),
        prepare(&arc, &Options::default()).unwrap(),
        prepare(&cuboid([9e6, 0., 0.], [9e6 + 1., 1., 1.]), &Options::default()).unwrap(),
    ];
    assert_eq!(defs[0].report.freeform_faces, vec![0]);
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&defs, &[identity(0, [0.; 3]), identity(1, [0.; 3]), identity(2, [0.; 3])], &view, &Options::default(), &never).unwrap();
    for p in edge_line(&d, 1, 0).pieces.iter().filter(|p| p.visibility == Visibility::Visible) {
        for k in 0..=400 {
            let t = p.t[0] + (p.t[1] - p.t[0]) * k as f64 / 400.;
            let (px, py) = (10000. * t.cos(), 10000. * t.sin());
            let under = px > x - 2.5 && px < x + 2.5 && py > -4000. && py < 3000.;
            assert!(!under, "visible at ({px}, {py}), t {t}, piece {:?}", p.t);
        }
    }
}

#[test]
fn narrow_torus_strips_keep_their_outline() {
    // Two strips of a torus R 20, r 10 between the meridian angles 0.3002, 0.3005 and 0.3008,
    // seen from the side: each side of the outline is a 0.006 mm piece of the circle traced on
    // the whole torus, whose chords are far longer than the strips are wide.
    let tau = std::f64::consts::TAU;
    let ring = |phi: f64, v: u32| circle([0., 0., 10. * phi.sin()], 20. + 10. * phi.cos(), v);
    let strip = |m: [f64; 2], e: [u32; 2]| Face {
        loops: vec![vec![uses(e[0], true)], vec![uses(e[1], false)]],
        surface: Surface::Torus { frame: frame_at([0.; 3]), major_radius: 20., minor_radius: 10., longitude: [0., tau], meridian: m },
        forward: true,
    };
    let phis: [f64; 3] = [0.3002, 0.3005, 0.3008];
    let body = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: phis.iter().map(|p| [20. + 10. * p.cos(), 0., 10. * p.sin()]).collect(),
            edges: vec![ring(phis[0], 0), ring(phis[1], 1), ring(phis[2], 2)],
            faces: vec![strip([phis[0], phis[1]], [0, 1]), strip([phis[1], phis[2]], [1, 2])],
        }],
    };
    let def = prepare(&body, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    for side in [-1., 1.] {
        let drawn: f64 = silhouette_pieces(&d)
            .iter()
            .filter(|p| p.visibility == Visibility::Visible && !p.covered)
            .flat_map(|p| p.geometry.iter())
            .filter(|g| g.point_at(0.5).x * side > 0.)
            .map(|g| g.point_at(0.).dist(g.point_at(1.)))
            .sum();
        assert!((drawn - 0.006).abs() < 1e-4, "side {side}: {drawn} mm of outline");
    }
}

#[test]
fn mirrored_placements_bad_indices_and_bad_views_are_refused() {
    let def = prepare(&cuboid([0.; 3], [1.; 3]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -1., -1.), Vec3::new(0., 0., 1.)).unwrap();
    let mirror =
        Placement { body: 0, rotation: [Vec3::new(-1., 0., 0.), Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)], translation: Vec3::new(0., 0., 0.) };
    let defs = [def];
    assert!(matches!(hidden_lines(&defs, &[mirror], &view, &Options::default(), &never), Err(HlrError::Invalid(_))));
    assert!(matches!(hidden_lines(&defs, &[identity(3, [0.; 3])], &view, &Options::default(), &never), Err(HlrError::Invalid(_))));
    assert!(matches!(hidden_lines(&defs, &[identity(0, [f64::NAN, 0., 0.])], &view, &Options::default(), &never), Err(HlrError::Invalid(_))));
    assert!(View::new(Vec3::new(0., 0., 0.), Vec3::new(0., 0., 1.)).is_err());
    assert!(View::new(Vec3::new(0., 0., 1.), Vec3::new(0., 0., 2.)).is_err());
}

#[test]
fn cancellation_and_budgets_return_errors() {
    let def = prepare(&cylinder(10., 30.), &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -0.7, -0.4), Vec3::new(0., 0., 1.)).unwrap();
    let defs = [def];
    let scene = [identity(0, [0.; 3]), identity(0, [5., 0., 0.])];
    assert!(matches!(hidden_lines(&defs, &scene, &view, &Options::default(), &|| true), Err(HlrError::Cancelled)));
    assert!(matches!(hidden_lines(&defs, &scene, &view, &Options { max_rays: 3, ..Options::default() }, &never), Err(HlrError::Budget(_))));
    assert!(matches!(hidden_lines(&defs, &scene, &view, &Options { max_tracks: 1, ..Options::default() }, &never), Err(HlrError::Budget(_))));
}

#[test]
fn two_touching_boxes_draw_each_shared_edge_once() {
    let a = prepare(&cuboid([0.; 3], [10.; 3]), &Options::default()).unwrap();
    let b = prepare(&cuboid([10., 0., 0.], [20., 10., 10.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -0.7, -0.5), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[a, b], &[identity(0, [0.; 3]), identity(1, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert_eq!(d.stats.uncertain_uncovered, 0, "{:?}", d.stats);
    // The edges at x = 10: 5, 7, 9, 11 of the first box are 4, 6, 8, 10 of the second.
    let uncovered = |l: &Line| -> f64 { l.pieces.iter().filter(|p| !p.covered).map(|p| p.t[1] - p.t[0]).sum() };
    for (ea, eb) in [(5, 4), (7, 6), (9, 8), (11, 10)] {
        let once = uncovered(edge_line(&d, 0, ea)) + uncovered(edge_line(&d, 1, eb));
        assert!((once - 1.).abs() < 1e-9, "edges {ea}/{eb} drawn {once} times");
    }
}

#[test]
fn a_cone_in_a_front_view_has_exact_outline_rulings_and_edge_on_circles() {
    // A frustum from radius 5 at z = 0 to 10 at z = 20, seen exactly from the front: the
    // rulings run from (+-5, 0) to (+-10, 20) (one along the seam) and each circle is drawn
    // once as the segment it is seen edge-on as.
    let def = prepare(&turned(5., 10., 20.), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert_eq!(d.stats.uncertain_uncovered, 0, "{:?}", d.stats);
    let mut rulings: Vec<[f64; 4]> = Vec::new();
    for p in silhouette_pieces(&d) {
        assert_eq!((p.visibility, p.accuracy), (Visibility::Visible, CurveAccuracy::Analytic));
        for g in &p.geometry {
            let Seg2::Line { a, b } = *g else { panic!("a ruling is a line: {g:?}") };
            let (a, b) = if a.y <= b.y { (a, b) } else { (b, a) };
            rulings.push([a.x, a.y, b.x, b.y]);
        }
    }
    rulings.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(rulings.len(), 2, "{rulings:?}");
    for (r, want) in rulings.iter().zip([[-5., 0., -10., 20.], [5., 0., 10., 20.]]) {
        assert!(r.iter().zip(want).all(|(x, y)| (x - y).abs() < 1e-9), "{rulings:?}");
    }
    for (edge, (y, half)) in [(0, (0., 5.)), (1, (20., 10.))] {
        let drawn: f64 = edge_line(&d, 0, edge)
            .pieces
            .iter()
            .filter(|p| p.visibility == Visibility::Visible && !p.covered)
            .flat_map(|p| p.geometry.iter())
            .map(|g| {
                let (a, b) = (g.point_at(0.), g.point_at(1.));
                assert!((a.y - y).abs() < 1e-9 && (b.y - y).abs() < 1e-9 && a.x.abs() <= half + 1e-9, "{g:?}");
                (b.x - a.x).abs()
            })
            .sum();
        assert!((drawn - 2. * half).abs() < 1e-6, "circle {edge}: {drawn} mm drawn");
    }
}

#[test]
fn a_torus_seen_obliquely_spans_its_outer_diameter() {
    // Across the view (screen x is square to the axis here) the outline of a torus reaches
    // R + r on both sides, whatever the tilt; the traced silhouette reports its deviation.
    let def = prepare(&torus(20., 5.), &Options::default()).unwrap();
    for tilt in [0.3, 1., 3.] {
        let view = View::new(Vec3::new(0., 1., -tilt), Vec3::new(0., 0., 1.)).unwrap();
        let d = hidden_lines(std::slice::from_ref(&def), &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
        assert_eq!(d.stats.uncertain_uncovered, 0, "tilt {tilt}: {:?}", d.stats);
        let mut dev = 0f64;
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for p in pieces(&d).filter(|p| p.visibility == Visibility::Visible && !p.covered) {
            if let CurveAccuracy::Sampled { max_sample_deviation_mm } = p.accuracy {
                dev = dev.max(max_sample_deviation_mm);
            }
            for g in &p.geometry {
                for s in [0., 0.5, 1.] {
                    let q = g.point_at(s);
                    (lo, hi) = (lo.min(q.x), hi.max(q.x));
                }
            }
        }
        assert!(dev < 1e-3, "tilt {tilt}: deviation {dev}");
        assert!((hi - 25.).abs() < 1e-3 && (lo + 25.).abs() < 1e-3, "tilt {tilt}: x from {lo} to {hi}");
    }
}

/// A disc bounded by a rational quadratic circle of radius `r` (control points `pts` with
/// weights 1 and 1/sqrt 2 alternating) whose first three quarters are squeezed into the
/// parameters 0 to 0.009.
fn squeezed_disc(r: f64, pts: [[f64; 2]; 9]) -> Brep {
    disc(r, pts, vec![0., 0., 0., 0.003, 0.003, 0.006, 0.006, 0.009, 0.009, 1., 1., 1.])
}

/// A disc bounded by a rational quadratic spline with control points `pts` (weights 1 and
/// 1/sqrt 2 alternating) and `knots`, starting at (`r`, 0).
fn disc(r: f64, pts: [[f64; 2]; 9], knots: Vec<f64>) -> Brep {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let homogeneous = pts.iter().enumerate().map(|(i, p)| if i % 2 == 1 { [p[0] * s, p[1] * s, 0., s] } else { [p[0], p[1], 0., 1.] }).collect();
    Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![[r, 0., 0.]],
            edges: vec![Edge { vertices: [0, 0], curve: Curve::RationalSpline { knots, homogeneous } }],
            faces: vec![Face {
                loops: vec![vec![uses(0, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    }
}

fn circle_points(r: f64) -> [[f64; 2]; 9] {
    [[r, 0.], [r, r], [0., r], [-r, r], [-r, 0.], [-r, -r], [0., -r], [r, -r], [r, 0.]]
}

#[test]
fn a_bent_quarter_in_a_knot_span_narrower_than_rounding_is_drawn() {
    // The second quarter lives in the knot span 0.25 to 0.25 + 2^-50 and bulges to a radius of
    // about 10.3: the edge is not taken for a circle and the bulge is drawn.
    let mut pts = circle_points(10.);
    if let Some(p) = pts.get_mut(3) {
        *p = [-11., 10.];
    }
    let k = 0.25 + 2f64.powi(-50);
    let def = prepare(&disc(10., pts, vec![0., 0., 0., 0.25, 0.25, k, k, 0.5, 0.5, 1., 1., 1.]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert!(pieces(&d).all(|p| matches!(p.accuracy, CurveAccuracy::Sampled { .. })), "{d:?}");
    let far = pieces(&d).flat_map(|p| p.geometry.iter()).flat_map(|g| [g.point_at(0.), g.point_at(1.)]).map(|q| q.len()).fold(0., f64::max);
    assert!(far > 10.25, "the bulge reaches {far}");
}

#[test]
fn an_unevenly_parameterised_circle_is_drawn_whole() {
    // The conic arcs must still turn the full 360 degrees.
    let def = prepare(&squeezed_disc(10., circle_points(10.)), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0.2, 0.1, -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let mut sweep = 0.;
    for p in pieces(&d) {
        assert_eq!((p.visibility, p.accuracy), (Visibility::Visible, CurveAccuracy::Analytic));
        for g in &p.geometry {
            let Seg2::Conic { w, .. } = *g else { panic!("a circle is drawn as conic arcs: {g:?}") };
            sweep += 2. * w.acos();
        }
    }
    assert!((sweep - std::f64::consts::TAU).abs() < 1e-9, "the arcs turn {sweep} rad");
}

#[test]
fn a_spline_off_its_circle_in_a_narrow_knot_span_is_not_taken_for_one() {
    // Moving one control point of the squeezed circle bends the span 0.003 to 0.006 off the
    // circle by about 0.3 mm: the edge is drawn as sampled chords, not exact arcs.
    let mut pts = circle_points(10.);
    if let Some(p) = pts.get_mut(3) {
        *p = [-11., 10.];
    }
    let def = prepare(&squeezed_disc(10., pts), &Options::default()).unwrap();
    let view = View::new(Vec3::new(0.2, 0.1, -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert!(pieces(&d).all(|p| matches!(p.accuracy, CurveAccuracy::Sampled { .. })), "{d:?}");
}

/// A cone frustum about a vertical axis through `o`: radius `r0` at the frame's origin and
/// slope `k`, between the heights `h`.
fn frustum(o: [f64; 3], r0: f64, k: f64, h: [f64; 2]) -> Shell {
    let tau = std::f64::consts::TAU;
    let at = |z: f64| [o[0] + r0 + k * z, o[1], o[2] + z];
    let ring = |z: f64, v: u32| circle([o[0], o[1], o[2] + z], r0 + k * z, v);
    let edges = vec![ring(h[0], 0), ring(h[1], 1), Edge { vertices: [0, 1], curve: Curve::Line { start: at(h[0]), end: at(h[1]) } }];
    let plane = |z: f64, flip: f64| Surface::Plane { origin: [o[0], o[1], o[2] + z], u_axis: [1., 0., 0.], v_axis: [0., flip, 0.] };
    let faces = vec![
        Face {
            loops: vec![vec![uses(0, true), uses(2, true), uses(1, false), uses(2, false)]],
            surface: Surface::Cone { frame: frame_at(o), radius_at_origin: r0, half_angle: k.atan(), angle: [0., tau], height: h },
            forward: true,
        },
        Face { loops: vec![vec![uses(0, false)]], surface: plane(h[0], -1.), forward: true },
        Face { loops: vec![vec![uses(1, true)]], surface: plane(h[1], 1.), forward: true },
    ];
    Shell { vertices: vec![at(h[0]), at(h[1])], edges, faces }
}

#[test]
fn nearly_coaxial_cones_far_out_keep_their_own_silhouettes() {
    // Two frusta of slope 0.25 whose axes are slightly apart, far out: recognition may take
    // them for one cone, but each is drawn with its own outline. Small ones 0.005 mm apart
    // 1e7 mm out, and 1 km ones 0.4 mm apart 1e9 mm out.
    for (x, r, gap) in [(1e7, 5., 0.005), (1e9, 1e6, 0.4)] {
        let shells = vec![frustum([x, 0., 0.], r, 0.25, [0., 10.]), frustum([x + gap, 0., 0.], r, 0.25, [20., 30.])];
        check_two_cone_outlines(&Brep { version: crate::brep::VERSION, shells });
    }
}

/// A sector `a` (radians) of a cone about a vertical axis through `o`, radius `r0` at the
/// frame's origin and slope `k`, between the heights `h`: one face, no caps.
fn cone_sector(o: [f64; 3], r0: f64, k: f64, h: [f64; 2], a: [f64; 2]) -> Shell {
    let at = |z: f64, t: f64| [o[0] + (r0 + k * z) * t.cos(), o[1] + (r0 + k * z) * t.sin(), o[2] + z];
    let arc = |z: f64, v: u32, w: u32| Edge {
        vertices: [v, w],
        curve: Curve::CircleArc { frame: frame_at([o[0], o[1], o[2] + z]), radius: r0 + k * z, angle: a },
    };
    let ruling = |t: f64, v: u32, w: u32| Edge { vertices: [v, w], curve: Curve::Line { start: at(h[0], t), end: at(h[1], t) } };
    let edges = vec![arc(h[0], 0, 1), ruling(a[1], 1, 2), arc(h[1], 3, 2), ruling(a[0], 0, 3)];
    let face = Face {
        loops: vec![vec![uses(0, true), uses(1, true), uses(2, false), uses(3, false)]],
        surface: Surface::Cone { frame: frame_at(o), radius_at_origin: r0, half_angle: k.atan(), angle: a, height: h },
        forward: true,
    };
    Shell { vertices: vec![at(h[0], a[0]), at(h[0], a[1]), at(h[1], a[1]), at(h[1], a[0])], edges, faces: vec![face] }
}

#[test]
fn nearly_coincident_cone_sectors_both_draw_their_outline() {
    // Two sectors (0 to 0.02 rad) of 1 m cones of slope 0.25, 1e9 mm out, axes 3e-6 mm apart,
    // seen along the tangent at 0.01 rad: recognition takes them for one cone, but a ray along
    // one's ruling would cross the other, so each keeps its own outline, visible over its
    // whole height.
    let x = 1e9;
    let shells = vec![
        cone_sector([x, 0., 0.], 1000., 0.25, [0., 1000.], [0., 0.02]),
        cone_sector([x + 3e-6, 0., 0.], 1000., 0.25, [2000., 3000.], [0., 0.02]),
    ];
    let def = prepare(&Brep { version: crate::brep::VERSION, shells }, &Options::default()).unwrap();
    let view = View::new(Vec3::new(-(0.01f64.sin()), 0.01f64.cos(), 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    for (z0, z1) in [(0., 1000.), (2000., 3000.)] {
        let drawn: f64 = silhouette_pieces(&d)
            .iter()
            .filter(|p| p.visibility == Visibility::Visible && !p.covered)
            .flat_map(|p| p.geometry.iter())
            .map(|g| {
                let (a, b) = (g.point_at(0.).y.clamp(z0, z1), g.point_at(1.).y.clamp(z0, z1));
                (b - a).abs()
            })
            .sum();
        assert!((drawn - (z1 - z0)).abs() < 1e-3, "z {z0}..{z1}: {drawn} mm of ruling drawn");
    }
}

/// A spherical cap of radius `r` about `c`: the latitudes from `lat` up to the pole, bounded by
/// one circle through the vertex `v` at longitude 0.
fn cap(c: [f64; 3], r: f64, lat: f64, edge: Curve, v: [f64; 3]) -> Shell {
    let (tau, half) = (std::f64::consts::TAU, std::f64::consts::FRAC_PI_2);
    let face = Face {
        loops: vec![vec![uses(0, true)]],
        surface: Surface::Sphere { frame: frame_at(c), radius: r, longitude: [0., tau], latitude: [lat, half] },
        forward: true,
    };
    Shell { vertices: vec![v], edges: vec![Edge { vertices: [0, 0], curve: edge }], faces: vec![face] }
}

#[test]
fn spherical_caps_through_one_circle_keep_their_own_outlines() {
    // Two caps of radius 1000 whose centres are 5e-7 mm apart along the axis, both bounded by
    // the same circle: recognition takes them for one sphere, the boundary lies on both, but
    // inside they differ, so each draws its own outline.
    let (r, z): (f64, f64) = (1000., 600.);
    let ring = (r * r - z * z).sqrt();
    let edge = || Curve::CircleArc { frame: frame_at([0., 0., z]), radius: ring, angle: [0., std::f64::consts::TAU] };
    let dz = 5e-7;
    let lat = |zc: f64| ((z - zc) / r).asin();
    let shells = vec![
        cap([0.; 3], r, lat(0.), edge(), [ring, 0., z]),
        cap([0., 0., dz], (ring * ring + (z - dz).powi(2)).sqrt(), lat(dz), edge(), [ring, 0., z]),
    ];
    let def = prepare(&Brep { version: crate::brep::VERSION, shells }, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    // Above the ring the outline is a half circle of about pi * 1000 * 0.41 mm on each side.
    let drawn: f64 = silhouette_pieces(&d)
        .iter()
        .filter(|p| p.visibility == Visibility::Visible && !p.covered)
        .flat_map(|p| p.geometry.iter())
        .map(|g| (0..32).map(|k| g.point_at(k as f64 / 32.).dist(g.point_at((k + 1) as f64 / 32.))).sum::<f64>())
        .sum();
    let arc = 2. * r * (std::f64::consts::FRAC_PI_2 - (z / r).asin());
    assert!(drawn > 0.99 * arc, "{drawn} mm of outline drawn of about {arc}");
    assert_eq!(d.stats.uncertain_uncovered, 0, "{:?}", d.stats);
}

/// The lateral face of a cone with its apex at the origin, slope 0.25, between the heights
/// `h`, its axis turned by `tilt` about Y from +Z.
fn tilted_cone(tilt: f64, h: [f64; 2]) -> Shell {
    let (x, y, z) = (Vec3::new(tilt.cos(), 0., -tilt.sin()), Vec3::new(0., 1., 0.), Vec3::new(tilt.sin(), 0., tilt.cos()));
    let arr = |v: Vec3| [v.x, v.y, v.z];
    let frame = |o: Vec3| crate::brep::Frame { origin: arr(o), x_axis: arr(x), y_axis: arr(y) };
    let at = |hh: f64| x * (0.25 * hh) + z * hh;
    let ring = |hh: f64, v: u32| Edge {
        vertices: [v, v],
        curve: Curve::CircleArc { frame: frame(z * hh), radius: 0.25 * hh, angle: [0., std::f64::consts::TAU] },
    };
    let seam = Edge { vertices: [0, 1], curve: Curve::Line { start: arr(at(h[0])), end: arr(at(h[1])) } };
    let face = Face {
        loops: vec![vec![uses(0, true), uses(2, true), uses(1, false), uses(2, false)]],
        surface: Surface::Cone {
            frame: frame(Vec3::new(0., 0., 0.)),
            radius_at_origin: 0.,
            half_angle: 0.25f64.atan(),
            angle: [0., std::f64::consts::TAU],
            height: h,
        },
        forward: true,
    };
    Shell { vertices: vec![arr(at(h[0])), arr(at(h[1]))], edges: vec![ring(h[0], 0), ring(h[1], 1), seam], faces: vec![face] }
}

#[test]
fn cones_with_slightly_turned_axes_keep_their_own_outlines() {
    // One apex, axes 3e-7 rad apart, heights 1000 to 3000 and 6000 to 8000: each frustum draws
    // both rulings of its own outline.
    let shells = vec![tilted_cone(0., [1000., 3000.]), tilted_cone(3e-7, [6000., 8000.])];
    let def = prepare(&Brep { version: crate::brep::VERSION, shells }, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    for (z0, z1) in [(1000., 3000.), (6000., 8000.)] {
        let drawn: f64 = silhouette_pieces(&d)
            .iter()
            .filter(|p| p.visibility == Visibility::Visible && !p.covered)
            .flat_map(|p| p.geometry.iter())
            .filter(|g| g.point_at(0.5).y > z0 && g.point_at(0.5).y < z1)
            .map(|g| g.point_at(0.).dist(g.point_at(1.)))
            .sum();
        let ruling = (z1 - z0) * (1f64 + 0.0625).sqrt();
        assert!((drawn - 2. * ruling).abs() < 1e-3 * ruling, "z {z0}..{z1}: {drawn} mm of rulings drawn, want {}", 2. * ruling);
    }
}

#[test]
fn a_high_degree_spline_through_points_of_a_circle_is_not_taken_for_one() {
    // A closed degree-16 Bezier through 17 points of a circle of radius 10 at t = j / 16, at
    // unevenly spaced angles: on the circle at every t = j / 16 (where a check with evenly
    // spaced samples would look), off it between them, so it is drawn as chords.
    let n = 16usize;
    let ts: Vec<f64> = (0..=n).map(|j| j as f64 / n as f64).collect();
    let binom = |k: usize| (0..k).fold(1f64, |acc, i| acc * (n - i) as f64 / (i + 1) as f64);
    // Solve the Bernstein collocation system for x and y by Gaussian elimination.
    let mut rows: Vec<Vec<f64>> = ts
        .iter()
        .enumerate()
        .map(|(j, t)| {
            let a = std::f64::consts::TAU * j as f64 / n as f64 + 0.2 * (std::f64::consts::TAU * 3. * j as f64 / n as f64).sin();
            let mut row: Vec<f64> = (0..=n).map(|i| binom(i) * t.powi(i as i32) * (1. - t).powi((n - i) as i32)).collect();
            row.extend([10. * a.cos(), 10. * a.sin()]);
            row
        })
        .collect();
    for col in 0..=n {
        let pivot = (col..=n).max_by(|a, b| rows[*a][col].abs().total_cmp(&rows[*b][col].abs())).unwrap();
        rows.swap(col, pivot);
        for r in 0..=n {
            if r != col {
                let f = rows[r][col] / rows[col][col];
                let src = rows[col].clone();
                for (x, s) in rows[r].iter_mut().zip(src) {
                    *x -= f * s;
                }
            }
        }
    }
    let points: Vec<[f64; 3]> = (0..=n).map(|i| [rows[i][n + 1] / rows[i][i], rows[i][n + 2] / rows[i][i], 0.]).collect();
    let knots: Vec<f64> = std::iter::repeat_n(0., n + 1).chain(std::iter::repeat_n(1., n + 1)).collect();
    let start = points[0];
    let disc = Brep {
        version: crate::brep::VERSION,
        shells: vec![Shell {
            vertices: vec![start],
            edges: vec![Edge { vertices: [0, 0], curve: Curve::Spline { knots, points } }],
            faces: vec![Face {
                loops: vec![vec![uses(0, true)]],
                surface: Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
                forward: true,
            }],
        }],
    };
    let def = prepare(&disc, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0.2, 0.1, -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    assert!(pieces(&d).all(|p| matches!(p.accuracy, CurveAccuracy::Sampled { .. })), "{:?}", pieces(&d).map(|p| p.accuracy).collect::<Vec<_>>());
}

fn check_two_cone_outlines(body: &Brep) {
    let def = prepare(body, &Options::default()).unwrap();
    let view = View::new(Vec3::new(0., 1., 0.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    for (z0, z1) in [(0., 10.), (20., 30.)] {
        let rulings = silhouette_pieces(&d)
            .iter()
            .filter(|p| p.visibility == Visibility::Visible && !p.covered)
            .flat_map(|p| p.geometry.iter())
            .filter(|g| {
                let (a, b) = (g.point_at(0.), g.point_at(1.));
                (a.y.min(b.y) - z0).abs() < 1e-6 && (a.y.max(b.y) - z1).abs() < 1e-6
            })
            .count();
        assert_eq!(rulings, 2, "z {z0}..{z1}: {:?}", silhouette_pieces(&d));
    }
}

#[test]
fn the_shadow_index_finds_each_nearby_box_once() {
    // 40 000 boxes 1 mm wide every 20 mm: a query near one finds it and no far one.
    let mut list = Vec::new();
    for i in 0..200 {
        for j in 0..200 {
            let (x, y) = (20. * i as f64, 20. * j as f64);
            list.push(Shadow { lo: [x, y], hi: [x + 1., y + 1.], near: 0. });
        }
    }
    let index = ShadowIndex::new(list).unwrap();
    let mut near = Vec::new();
    index.query([400.5, 600.5], [400.6, 600.6], &mut near);
    let faces: Vec<usize> = near
        .iter()
        .copied()
        .filter(|k| index.list.get(*k).is_some_and(|sh| sh.lo[0] <= 400.6 && sh.hi[0] >= 400.5 && sh.lo[1] <= 600.6 && sh.hi[1] >= 600.5))
        .collect();
    assert_eq!(faces, vec![20 * 200 + 30]);
    assert!(near.len() <= 9, "{} candidates", near.len());
    index.query([-50., -50.], [-40., -40.], &mut near);
    assert!(near.is_empty());
}

#[test]
fn a_plane_through_three_points_draws_as_a_plane() {
    // The box with every face stored as the plane through three of its points draws the same
    // bits as the box with axis-stored planes.
    let o = Options::default();
    let mut through = cuboid([0.; 3], [1.; 3]);
    for f in &mut through.shells[0].faces {
        let Surface::Plane { origin, u_axis, v_axis } = f.surface else { panic!("a box face is a plane") };
        let at = |a: [f64; 3]| [origin[0] + a[0], origin[1] + a[1], origin[2] + a[2]];
        f.surface = Surface::PlaneThrough { origin, u_point: at(u_axis), v_point: at(v_axis) };
    }
    let view = View::new(Vec3::new(-1., -0.8, -0.6), Vec3::new(0., 0., 1.)).unwrap();
    let draw = |b: &Brep| {
        let mut d = hidden_lines(&[prepare(b, &o).unwrap()], &[identity(0, [0.; 3])], &view, &o, &never).unwrap();
        d.stats.phase_ms = [0.; 4];
        format!("{d:?}")
    };
    assert!(draw(&through) == draw(&cuboid([0.; 3], [1.; 3])));
}

#[test]
fn procedural_and_placed_records_are_not_supported_yet() {
    let o = Options::default();
    // A cylinder side stored as a placed revolution of its seam line.
    let mut placed = cylinder(4., 10.);
    let profile = Box::new(Curve::Line { start: [4., 0., 0.], end: [4., 0., 10.] });
    let matrix = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
    placed.shells[0].faces[0].surface = Surface::PlacedRevolution { profile, origin: [0.; 3], axis: [0., 0., 1.], matrix, forward: true };
    assert!(matches!(prepare(&placed, &o), Err(HlrError::Unsupported("placed revolution surface"))));
    // A box edge stored as the meeting of the planes y = 0 and z = 0, traced from a leader.
    let mut traced = cuboid([0.; 3], [1.; 3]);
    let y0 = Box::new(Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 0., 1.] });
    let z0 = Box::new(Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] });
    let leader = Box::new(Curve::Polyline { points: vec![[0.; 3], [1., 0., 0.]] });
    traced.shells[0].edges[0].curve = Curve::SurfaceIntersection { surfaces: [y0, z0], leader, tolerance: 1e-6 };
    assert!(matches!(prepare(&traced, &o), Err(HlrError::Unsupported("surface intersection curve"))));
}

/// The edges whose every piece is hidden, in a box drawn from the isometric view.
fn hidden_box_edges(b: &Brep) -> Vec<usize> {
    let def = prepare(b, &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -1., -1.), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let mut hidden = Vec::new();
    for l in &d.lines {
        let Source::Edge { edge, .. } = l.source else { panic!("a box has no silhouettes") };
        let states: Vec<_> = l.pieces.iter().map(|p| p.visibility).collect();
        assert!(states.windows(2).all(|w| w[0] == w[1]), "edge {edge} is split: {states:?}");
        if states[0] == Visibility::Hidden {
            hidden.push(edge);
        }
    }
    hidden.sort_unstable();
    hidden
}

#[test]
fn a_plane_record_off_its_edges_by_rounding_still_hides() {
    // The top face's plane stands one rounding (a 1 mm cube) or 1e-11 (a 100 mm cube) above
    // its edges. Its box must still let the rays from the back edges reach it.
    for (size, off) in [(1., 2.2e-16), (100., 1e-11), (1., -2.2e-16)] {
        let mut b = cuboid([0.; 3], [size; 3]);
        let Surface::Plane { origin, .. } = &mut b.shells[0].faces[1].surface else { panic!("the top face is a plane") };
        assert_eq!(origin[2], size);
        origin[2] += off;
        assert_ne!(origin[2], size, "the offset must change the record");
        assert_eq!(hidden_box_edges(&b), vec![0, 4, 8], "size {size}, offset {off}");
    }
    // A 0.001 mm cube modelled 1e7 mm out, its top plane one step (1.9e-9 mm) above its edges:
    // more than 1e-6 of the face, so the padding must also grow with the coordinates.
    let (c, size) = (1e7, 0.001);
    let mut b = cuboid([c; 3], [c + size; 3]);
    let Surface::Plane { origin, .. } = &mut b.shells[0].faces[1].surface else { panic!("the top face is a plane") };
    origin[2] = f64::from_bits(origin[2].to_bits() + 1);
    assert!(origin[2] - (c + size) > 1e-9);
    assert_eq!(hidden_box_edges(&b), vec![0, 4, 8], "a tiny cube far out");
}

/// A plate of `n` by `n` holes of radius `r` at `pitch`, `h` thick: the faces of `cuboid`, a
/// circle per hole in the top and bottom faces, and a cylinder face (seam at angle 0) per hole.
fn holed_plate(n: usize, pitch: f64, r: f64, h: f64) -> Brep {
    let tau = std::f64::consts::TAU;
    let size = n as f64 * pitch;
    let mut b = cuboid([0.; 3], [size, size, h]);
    let shell = &mut b.shells[0];
    for i in 0..n {
        for j in 0..n {
            let (cx, cy) = ((i as f64 + 0.5) * pitch, (j as f64 + 0.5) * pitch);
            let v = shell.vertices.len() as u32;
            shell.vertices.push([cx + r, cy, 0.]);
            shell.vertices.push([cx + r, cy, h]);
            let e = shell.edges.len() as u32;
            shell.edges.push(circle([cx, cy, 0.], r, v));
            shell.edges.push(circle([cx, cy, h], r, v + 1));
            shell.edges.push(Edge { vertices: [v, v + 1], curve: Curve::Line { start: [cx + r, cy, 0.], end: [cx + r, cy, h] } });
            // Holes run clockwise seen from outside: the bottom face looks down, the top up.
            shell.faces[0].loops.push(vec![uses(e, true)]);
            shell.faces[1].loops.push(vec![uses(e + 1, false)]);
            shell.faces.push(Face {
                loops: vec![vec![uses(e, true), uses(e + 2, true), uses(e + 1, false), uses(e + 2, false)]],
                surface: Surface::Cylinder { frame: frame_at([cx, cy, 0.]), radius: r, angle: [0., tau], height: [0., h] },
                forward: false,
            });
        }
    }
    b
}

#[test]
fn trim_samples_and_scene_extents_beyond_their_limits_are_refused() {
    for trim_samples in [0, MAX_TRIM_SAMPLES + 1] {
        assert!(matches!(prepare(&cuboid([0.; 3], [1.; 3]), &Options { trim_samples, ..Options::default() }), Err(HlrError::Invalid(_))));
    }
    // Placements far apart once made the scene's size infinite and every piece visible.
    let def = prepare(&cuboid([0.; 3], [1.; 3]), &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -1., -1.), Vec3::new(0., 0., 1.)).unwrap();
    let defs = [def];
    for far in [1e200, 2e12, -2e12] {
        let scene = [identity(0, [0.; 3]), identity(0, [far, 0., 0.])];
        assert!(matches!(hidden_lines(&defs, &scene, &view, &Options::default(), &never), Err(HlrError::Invalid("scene extent"))), "{far}");
    }
    let scene = [identity(0, [0.; 3]), identity(0, [1e11, 0., 0.])];
    assert!(hidden_lines(&defs, &scene, &view, &Options::default(), &never).is_ok());
}

#[test]
fn surface_groups_match_comparing_every_pair_and_preparing_can_be_cancelled() {
    // Many holes of one radius, one a hair larger, a torus and a sphere in halves.
    let mut b = holed_plate(4, 10., 3., 2.);
    let wider = holed_plate(1, 10., 3. * (1. + 5e-10), 2.);
    for s in [wider.shells[0].clone(), torus(5., 1.5).shells[0].clone(), sphere(4.).shells[0].clone()] {
        b.shells.push(s);
    }
    let def = prepare(&b, &Options::default()).unwrap();
    for (i, f) in def.faces.iter().enumerate() {
        let every: Vec<usize> =
            def.faces.iter().enumerate().filter(|(j, g)| *j == i || same_analytic(&f.analytic, &g.analytic, 1e-9)).map(|(j, _)| j).collect();
        assert_eq!(f.group, every, "face {i}");
    }
    // The wider hole joins the first hole (same axis); the torus and sphere halves pair up.
    assert_eq!(def.faces.iter().filter(|f| f.group.len() == 2).count(), 6);
    assert!(matches!(prepare_cancellable(&b, &Options::default(), &|| true), Err(HlrError::Cancelled)));
}

#[test]
fn the_chord_grid_finds_what_a_scan_of_every_chord_finds() {
    // The plate's top face: 16 circles and a square, thousands of chords.
    let def = prepare(&holed_plate(4, 10., 3., 2.), &Options::default()).unwrap();
    let trim = &def.faces[1].trim;
    assert!(trim.chords.len() > 1000);
    let (lo, hi) = (trim.lo, trim.hi);
    let mut checked = [0usize; 2];
    for i in 0..=60 {
        for j in 0..=60 {
            let p = Vec3::new(lo.x + (hi.x - lo.x) * i as f64 / 60., lo.y + (hi.y - lo.y) * j as f64 / 60., lo.z + 1e-9 * (i % 3) as f64);
            for tol in [1e-12, 1e-6, 0.01, 0.3, 2., 50.] {
                let scan = trim.chords.iter().any(|(a, b)| p.dist_to_segment(*a, *b) <= tol);
                assert_eq!(trim.near(p, tol), scan, "point {p:?}, tol {tol}");
                checked[usize::from(scan)] += 1;
            }
        }
    }
    assert!(checked[0] > 1000 && checked[1] > 1000, "{checked:?}");
}

#[test]
fn every_hole_of_a_plate_hides_the_same_part_of_its_bottom_circle() {
    let def = prepare(&holed_plate(4, 10., 3., 2.), &Options::default()).unwrap();
    let view = View::new(Vec3::new(-1., -0.8, -0.6), Vec3::new(0., 0., 1.)).unwrap();
    let d = hidden_lines(&[def], &[identity(0, [0.; 3])], &view, &Options::default(), &never).unwrap();
    let hidden =
        |e: usize| -> f64 { edge_line(&d, 0, e).pieces.iter().filter(|p| p.visibility == Visibility::Hidden).map(|p| p.t[1] - p.t[0]).sum() };
    // Edges 12, 15, 18, ...: each hole's bottom circle (12 box edges first).
    let first = hidden(12);
    assert!(first > 0.1 && first < std::f64::consts::TAU - 0.1, "part of a bottom circle is hidden by the wall: {first}");
    for k in 1..16 {
        let e = 12 + 3 * k;
        assert!((hidden(e) - first).abs() < 1e-6, "hole {k}: {} against {first}", hidden(e));
        let top = edge_line(&d, 0, e + 1);
        assert!(top.pieces.iter().all(|p| p.visibility == Visibility::Visible), "hole {k}'s top circle is in front: {:?}", top.pieces);
    }
}

#[test]
fn coaxial_cylinders_seen_along_their_axis_draw_one_circle() {
    // Eight cylinders stacked along Z, seen from above: all sixteen circles fall on one; only
    // the top one shows, the others are hidden or lie under it.
    let def = prepare(&cylinder(4., 10.), &Options::default()).unwrap();
    let scene: Vec<Placement> = (0..8).map(|k| identity(0, [0., 0., 15. * k as f64])).collect();
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &scene, &view, &Options::default(), &never).unwrap();
    assert_eq!(d.stats.uncertain_uncovered, 0, "{:?}", d.stats);
    // The circles meet at every sample point they share; none of that is a crossing (only the
    // seam's ends, where each curve stops, may count): 120 pairs once gave over 100 000.
    assert!(d.stats.crossings <= 4 * 120, "{:?}", d.stats);
    for l in &d.lines {
        let top = l.placement == 7 && l.source == Source::Edge { edge: 1, tangent: false };
        let shown: f64 = l.pieces.iter().filter(|p| p.visibility == Visibility::Visible && !p.covered).map(|p| p.t[1] - p.t[0]).sum();
        if top {
            assert!((shown - std::f64::consts::TAU).abs() < 1e-9, "the top circle shows whole: {:?}", l.pieces);
        } else {
            assert_eq!(shown, 0., "placement {} {:?}: {:?}", l.placement, l.source, l.pieces);
        }
    }
}

#[test]
fn a_few_hundred_placements_draw_as_one() {
    // 300 boxes apart on screen: each is drawn exactly as the first one is.
    let def = prepare(&cuboid([0.; 3], [1.; 3]), &Options::default()).unwrap();
    let scene: Vec<Placement> = (0..300).map(|k| identity(0, [4. * (k % 20) as f64, 4. * (k / 20) as f64, 0.])).collect();
    let view = View::new(Vec3::new(0.3, 0.2, -1.), Vec3::new(0., 1., 0.)).unwrap();
    let d = hidden_lines(&[def], &scene, &view, &Options::default(), &never).unwrap();
    assert_eq!(d.lines.len(), 300 * 12);
    let first: Vec<&Line> = d.lines.iter().filter(|l| l.placement == 0).collect();
    assert!(first.iter().any(|l| l.pieces.iter().any(|p| p.visibility == Visibility::Hidden)), "a box hides some of its own edges");
    for (k, chunk) in d.lines.chunks(12).enumerate() {
        for (l, f) in chunk.iter().zip(&first) {
            assert_eq!((l.placement, l.source), (k, f.source));
            assert_eq!(l.pieces.len(), f.pieces.len(), "placement {k} {:?}", l.source);
            for (p, q) in l.pieces.iter().zip(&f.pieces) {
                assert_eq!((p.visibility, p.covered), (q.visibility, q.covered), "placement {k} {:?}", l.source);
                assert!((p.t[0] - q.t[0]).abs() < 1e-9 && (p.t[1] - q.t[1]).abs() < 1e-9, "placement {k} {:?}", l.source);
            }
        }
    }
}

#[test]
fn chords_only_near_each_other_still_meet_their_crossings() {
    // An arc of radius 1000 sampled as one chord (sag about 5 mm) and a line 1 mm off that chord
    // toward the arc's bulge: all four ends lie within the sag of the other chord, yet the arc
    // crosses the line twice. The crossings must be found, not skipped as a coincidence.
    let def = prepare(&cuboid([0.; 3], [1.; 3]), &Options::default()).unwrap();
    let scene = [identity(0, [0.; 3])];
    let view = View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap();
    let defs = [def];
    let ctx = context(&defs, &scene, &view, &Options::default(), &never).unwrap();
    let arc = C3::Arc { o: Vec3::ZERO, x: Vec3::new(1., 0., 0.), y: Vec3::new(0., 1., 0.), r: [1000., 1000.], range: [0., 0.2] };
    let (a, b) = (arc.at(0.), arc.at(0.2));
    let sag = 1000. * (1. - 0.1f64.cos());
    let out = Vec3::new(0.1f64.cos(), 0.1f64.sin(), 0.);
    let line = C3::Line { a: a + out, b: b + out };
    let track = |curve: &C3, range: [f64; 2], sag: f64| {
        let screen = |t: f64| view.screen(curve.at(t));
        Track {
            placement: 0,
            curve: TrackCurve::Owned(curve.clone()),
            range,
            source: Source::Edge { edge: 0, tangent: false },
            acc: CurveAccuracy::Analytic,
            own: Vec::new(),
            own_eps: 1e-6,
            samples: vec![(range[0], screen(range[0])), (range[1], screen(range[1]))],
            sags: vec![sag],
            splits: Vec::new(),
        }
    };
    let mut tracks = vec![track(&arc, [0., 0.2], sag), track(&line, [0., 1.], 0.)];
    assert!(sag > 4.9 && ((a + out).dist(a) - 1.).abs() < 1e-12);
    assert_eq!(crossings(&mut tracks, &ctx).unwrap(), 2);
    assert!(tracks.iter().all(|t| t.splits.len() == 2), "{:?}", tracks.iter().map(|t| &t.splits).collect::<Vec<_>>());
}
