use super::*;
use std::f64::consts::{FRAC_PI_2, TAU};

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
const SQUARE: [[f64; 3]; 4] = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
fn sheet() -> Brep {
    faceted(&SQUARE, &[&[&[0, 1, 2, 3]]], vec![infinite(vec![0])])
}
const TETRA: [[f64; 3]; 4] = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const TETRA_FACES: [&[usize]; 4] = [&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[0, 3, 2]];
fn tetra() -> Brep {
    faceted(&TETRA, &[&TETRA_FACES], vec![infinite(vec![0]), Region { shells: vec![0], void: false }])
}
/// A tetrahedron with a tetrahedral void inside it.
fn hollow() -> Brep {
    let mut points = TETRA.to_vec();
    points.extend(TETRA.map(|p| p.map(|x| 0.2 + 0.1 * x)));
    let inner: Vec<Vec<usize>> = TETRA_FACES.iter().map(|c| c.iter().rev().map(|i| i + 4).collect()).collect();
    let inner: Vec<&[usize]> = inner.iter().map(Vec::as_slice).collect();
    faceted(
        &points,
        &[&TETRA_FACES, &inner],
        vec![infinite(vec![0]), Region { shells: vec![0, 1], void: false }, Region { shells: vec![1], void: true }],
    )
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
    fit_pcurves(&mut b);
    b
}
/// A ring of a torus on its NURBS form, closed in u (first and last control rows the same), as
/// SMLib holds the fillet round a hole's rim: bounded by its two circles and the tube's quarter
/// at u = 0, a seam used twice, each on its form.
fn closed_band() -> Brep {
    let (big, small) = (5., 1.);
    let torus =
        Surface::Torus { frame: Frame { origin: [0., 0., 0.], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] }, major_radius: big, minor_radius: small };
    let (form, domain) = form::surface(&torus, [[0., TAU], [0., FRAC_PI_2]]).unwrap();
    let about = |z: f64| Frame { origin: [0., 0., z], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let quarter = Curve::Circle { frame: Frame { origin: [big, 0., 0.], x_axis: [1., 0., 0.], y_axis: [0., 0., 1.] }, radius: small };
    let mut b = one_face(
        torus,
        domain,
        vec![
            (Curve::Circle { frame: about(0.), radius: big + small }, [0., TAU], [0, 0]),
            (quarter, [0., FRAC_PI_2], [0, 1]),
            (Curve::Circle { frame: about(small), radius: big }, [0., TAU], [1, 1]),
        ],
        &[[big + small, 0., 0.], [big, 0., small]],
        &[(0, true), (1, true), (2, false), (1, false)],
    );
    b.faces[0].nurbs = Some(form);
    for e in &mut b.edges {
        if let Some((nurbs, interval)) = form::curve(&e.curve, e.interval) {
            (e.nurbs, e.interval) = (Some(nurbs), interval);
        }
    }
    fit_pcurves(&mut b);
    b
}
/// A whole sphere bounded by one meridian from pole to pole, used twice: a closed shell.
fn ball() -> Brep {
    let meridian = Curve::Circle { frame: Frame { origin: [1., 2., 3.], x_axis: [1., 0., 0.], y_axis: [0., 0., 1.] }, radius: 2. };
    let mut b = one_face(
        Surface::Sphere { frame: a_frame(), radius: 2. },
        [[0., TAU], [-FRAC_PI_2, FRAC_PI_2]],
        vec![(meridian, [-FRAC_PI_2, FRAC_PI_2], [0, 1])],
        &[[1., 2., 1.], [1., 2., 5.]],
        &[(0, true), (0, false)],
    );
    b.regions.push(Region { shells: vec![0], void: false });
    b
}
/// The sheet with a vertex loop inside it, a wire shell and a vertex shell beside it.
fn sheet_with_lower_forms() -> Brep {
    let mut b = sheet();
    let n = b.vertices.len() as u32;
    // Vertex order: edge vertices first (the wire edge's), then the vertex loop's, then the shell's.
    b.vertices.extend([[2., 0., 0.], [3., 0., 0.], [0.5, 0.5, 0.], [5., 5., 5.]].map(|point| Vertex { point, tolerance: 1e-7 }));
    b.edges.push(Edge {
        nurbs: None,
        curve: Curve::Line { start: [2., 0., 0.], end: [3., 0., 0.] },
        interval: [0., 1.],
        vertices: [n, n + 1],
        tolerance: 1e-7,
        uses: vec![],
    });
    b.faces[0].loops.push(Loop::Vertex { vertex: n + 2 });
    b.shells.push(Shell::Wire { edges: vec![4] });
    b.shells.push(Shell::Vertex { vertex: n + 3 });
    b.regions[0].shells.extend([1, 2]);
    b
}

#[test]
fn every_form_validates_and_round_trips_without_a_kernel() {
    for (name, body) in [
        ("sheet", sheet()),
        ("closed", tetra()),
        ("void", hollow()),
        ("seam", tube()),
        ("seam on a closed spline", closed_band()),
        ("seam from pole to pole", ball()),
        ("lower forms", sheet_with_lower_forms()),
    ] {
        body.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
        body.check_geometry().unwrap_or_else(|e| panic!("{name}: {e}"));
        let bytes = serde_json::to_vec(&body).unwrap();
        let restored: Brep = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, restored, "{name}");
        assert!(!String::from_utf8(bytes).unwrap().contains("truck"));
    }
}

/// One defect each; every one is refused.
#[test]
fn one_defect_fixtures_are_refused() {
    let cases: Vec<(&str, &str, Brep)> = vec![
        ("version", "unsupported version", {
            let mut b = sheet();
            b.version = 3;
            b
        }),
        ("tolerance", "tolerance", {
            let mut b = sheet();
            b.tolerance = f64::NAN;
            b
        }),
        ("vertex tolerance", "tolerance", {
            let mut b = sheet();
            b.vertices[0].tolerance = 0.;
            b
        }),
        ("coordinate", "non-finite or excessive coordinate", {
            let mut b = sheet();
            b.vertices[0].point[0] = f64::INFINITY;
            b
        }),
        ("edge reference", "edge reference", {
            let mut b = sheet();
            if let Loop::Edges { edges } = &mut b.faces[0].loops[0] {
                edges[0].edge = u32::MAX;
            }
            b
        }),
        ("edge vertices", "edge vertices", {
            let mut b = sheet();
            b.edges[0].vertices[0] = u32::MAX;
            b
        }),
        ("disconnected loop", "disconnected loop", {
            let mut b = sheet();
            if let Loop::Edges { edges } = &mut b.faces[0].loops[0] {
                edges.swap(1, 2);
            }
            list_uses(&mut b);
            b
        }),
        ("reversed use", "disconnected loop", {
            let mut b = sheet();
            if let Loop::Edges { edges } = &mut b.faces[0].loops[0] {
                edges[1].forward = false;
            }
            list_uses(&mut b);
            b
        }),
        ("outer vertex loop", "outer loop is not an edge loop", {
            let mut b = sheet_with_lower_forms();
            b.faces[0].loops.swap(0, 1);
            b
        }),
        ("missing use", "edge uses do not list the edge's uses", {
            let mut b = tetra();
            b.edges[0].uses.pop();
            b
        }),
        ("repeated use", "edge use listed twice or not a use of the edge", {
            let mut b = tetra();
            let first = b.edges[0].uses[0];
            b.edges[0].uses[1] = first;
            b
        }),
        ("primary not first", "edge's primary use is not its first", {
            let mut b = tetra();
            b.edges[0].uses.reverse();
            b
        }),
        ("upper", "edge use listed twice or not a use of the edge", {
            let mut b = tetra();
            b.edges[0].uses[1].upper = !b.edges[0].uses[1].upper;
            b
        }),
        ("wire edge with uses", "wire edge with face uses", {
            let mut b = sheet_with_lower_forms();
            let u = b.edges[0].uses[0];
            b.edges[4].uses.push(u);
            b
        }),
        ("pcurve interval", "pcurve interval differs from its edge's", {
            let mut b = tube();
            if let Loop::Edges { edges } = &mut b.faces[0].loops[0] {
                edges[0].pcurve = segment(0., TAU - 1e-3);
            }
            b
        }),
        ("edge interval past one turn", "parameter interval exceeds one turn", {
            let mut b = tube();
            b.edges[0].interval = [0., TAU + 0.1];
            b
        }),
        ("edge interval outside a spline", "parameter interval outside the natural domain", {
            let mut b = sheet();
            b.edges[0].curve = Curve::Spline { knots: vec![0., 0., 1., 1.], points: vec![SQUARE[0], SQUARE[1]] };
            b.edges[0].interval = [0., 1.5];
            b
        }),
        ("empty edge interval", "empty parameter interval", {
            let mut b = sheet();
            b.edges[0].interval = [1., 1.];
            b
        }),
        ("face domain outside the sphere", "parameter interval outside the natural domain", {
            let mut b = sheet();
            b.faces[0].surface = Surface::Sphere { frame: a_frame(), radius: 2. };
            b.faces[0].domain = [[0., TAU], [-FRAC_PI_2 - 0.1, FRAC_PI_2]];
            b
        }),
        ("face domain past one turn", "parameter interval exceeds one turn", {
            let mut b = tube();
            b.faces[0].domain[0] = [0., TAU + 0.1];
            b
        }),
        ("face domain outside a spline surface", "parameter interval outside the natural domain", {
            let mut b = sheet();
            b.faces[0].surface = Surface::Spline {
                u_knots: vec![0., 0., 1., 1.],
                v_knots: vec![0., 0., 1., 1.],
                points: vec![vec![[0.; 3], [0., 1., 0.]], vec![[1., 0., 0.], [1., 1., 0.]]],
            };
            b
        }),
        ("edges out of order", "edges out of canonical order", {
            let mut b = sheet();
            b.edges.swap(0, 1);
            for f in &mut b.faces {
                for l in &mut f.loops {
                    if let Loop::Edges { edges } = l {
                        for u in edges {
                            u.edge = [1, 0, 2, 3][u.edge as usize];
                        }
                    }
                }
            }
            list_uses(&mut b);
            b
        }),
        ("vertices out of order", "vertices out of canonical order", {
            let mut b = sheet();
            b.vertices.swap(0, 1);
            for e in &mut b.edges {
                e.vertices = e.vertices.map(|v| [1, 0, 2, 3][v as usize]);
            }
            b
        }),
        ("faces out of order", "faces out of canonical order", {
            let mut b = tetra();
            if let Shell::Faces { faces, .. } = &mut b.shells[0] {
                faces.swap(0, 1);
            }
            b
        }),
        ("orphan vertex", "vertices out of canonical order", {
            let mut b = sheet();
            b.vertices.push(Vertex { point: [9.; 3], tolerance: 1e-7 });
            b
        }),
        ("shell twice in a region", "shell listed twice in a region", {
            // The void lists its one shell twice: still within the shell count.
            let mut b = hollow();
            let void = b.regions.last_mut().unwrap();
            void.shells.push(void.shells[0]);
            b
        }),
        ("void infinite region", "the infinite region is a void", {
            let mut b = sheet();
            b.regions[0].void = true;
            b
        }),
        ("open outer shell", "an open, wire or vertex shell lies in one region", {
            let mut b = sheet();
            b.regions.push(Region { shells: vec![0], void: false });
            b
        }),
        ("closed shell in one region", "a closed shell bounds two regions and encloses one", {
            let mut b = tetra();
            b.regions.pop();
            b
        }),
        ("bounded region without shells", "bounded region without an outer shell", {
            let mut b = tetra();
            b.regions.push(Region { shells: vec![], void: true });
            b
        }),
        ("unlisted shell", "a closed shell bounds two regions and encloses one", {
            let mut b = hollow();
            b.regions[1].shells.pop();
            b.regions.pop();
            b
        }),
        ("face used three times", "face used by neither one nor two oriented faces", {
            let mut b = tetra();
            if let Shell::Faces { faces, .. } = &mut b.shells[0] {
                faces.push(OrientedFace { face: 0, forward: false });
                faces.push(OrientedFace { face: 0, forward: true });
            }
            b
        }),
        ("face twice in one shell", "a face's two sides", {
            let mut b = tetra();
            if let Shell::Faces { faces, .. } = &mut b.shells[0] {
                faces.push(OrientedFace { face: 0, forward: false });
            }
            b
        }),
        ("cone domain past its apex", "parameter interval outside the natural domain", {
            let mut b = sheet();
            b.faces[0].surface = Surface::Cone { frame: a_frame(), radius_at_origin: 1., half_angle: 0.25 * std::f64::consts::PI };
            b.faces[0].domain = [[0., 1.], [-2., 1.]];
            b
        }),
    ];
    for (name, expected, body) in cases {
        let error = body.validate().err().unwrap_or_else(|| panic!("{name} was accepted"));
        assert_eq!(error.0, expected, "{name}");
    }
}

fn pcurve_of(b: &mut Brep, face: usize, position: usize) -> &mut Curve2 {
    let Loop::Edges { edges } = &mut b.faces[face].loops[0] else { panic!("an edge loop") };
    &mut edges[position].pcurve
}
fn shift(c: &mut Curve2, by: [f64; 2]) {
    match c {
        Curve2::Spline { points, .. } => points.iter_mut().for_each(|p| *p = [p[0] + by[0], p[1] + by[1]]),
        Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter_mut().for_each(|h| *h = [h[0] + by[0] * h[2], h[1] + by[1] * h[2], h[2]]),
    }
}

/// One geometric defect each, structurally valid: every one is refused, naming the record.
#[test]
fn geometric_defects_are_refused_by_record() {
    use geometry::{Problem, Record};
    let off = |what| move |p: &Problem| matches!(p, Problem::Off { what: w, .. } if *w == what);
    let cases: Vec<(&str, Record, Box<dyn Fn(&Problem) -> bool>, Brep)> = vec![
        ("vertex off its curve's start", Record::Edge(0), Box::new(off("curve start misses its start vertex")), {
            let mut b = sheet();
            b.vertices[0].point = [0., 0., 0.01];
            b
        }),
        ("vertex off its curve's end", Record::Edge(0), Box::new(off("curve end misses its end vertex")), {
            let mut b = sheet();
            b.edges[0].curve = Curve::Line { start: SQUARE[0], end: [1., 0., 0.01] };
            b
        }),
        ("pcurve shifted off its edge", Record::Use { face: 0, r#loop: 0, position: 1 }, Box::new(off("pcurve start lifts off its edge's curve")), {
            let mut b = sheet();
            shift(pcurve_of(&mut b, 0, 1), [0., 0.1]);
            b
        }),
        ("pcurve bowed off its edge", Record::Use { face: 0, r#loop: 0, position: 0 }, Box::new(off("pcurve middle lifts off its edge's curve")), {
            let mut b = sheet();
            let pc = pcurve_of(&mut b, 0, 0);
            let (a, z) = (pc.eval(0.).unwrap(), pc.eval(1.).unwrap());
            let middle = [0.5 * (a[0] + z[0]), 0.5 * (a[1] + z[1]) + 0.1];
            *pc = Curve2::Spline { knots: vec![0., 0., 0., 1., 1., 1.], points: vec![a, middle, z] };
            b
        }),
        (
            "seam pcurve on the wrong period",
            Record::Use { face: 0, r#loop: 0, position: 1 },
            Box::new(|p: &Problem| matches!(p, Problem::Period { .. })),
            {
                let mut b = tube();
                shift(pcurve_of(&mut b, 0, 1), [TAU, 0.]);
                b
            },
        ),
        ("vertex loop off its surface", Record::VertexLoop { face: 0, r#loop: 1 }, Box::new(off("vertex lies off its face's surface")), {
            let mut b = sheet_with_lower_forms();
            b.vertices[6].point[2] = 0.01;
            b
        }),
        (
            "seam uses of a closed spline on one line",
            Record::Use { face: 0, r#loop: 0, position: 1 },
            // On the ring's NURBS form one turn is four quarters.
            Box::new(|p: &Problem| *p == Problem::Period { gap: 4., period: 4. }),
            {
                let mut b = closed_band();
                shift(pcurve_of(&mut b, 0, 1), [-4., 0.]);
                b
            },
        ),
        (
            "seam from pole to pole on one line",
            Record::Loop { face: 0, r#loop: 0 },
            Box::new(|p: &Problem| {
                matches!(p, Problem::Empty { area } if area.abs() < 1e-9) && p.to_string().starts_with("loop encloses no (u, v) area (signed area ")
            }),
            {
                let mut b = ball();
                let mid = |b: &mut Brep, k: usize| pcurve_of(b, 0, k).eval(0.).unwrap()[0];
                let by = mid(&mut b, 0) - mid(&mut b, 1);
                assert!((by.abs() - TAU).abs() < 1e-9, "a period apart: {by}");
                shift(pcurve_of(&mut b, 0, 1), [by, 0.]);
                b
            },
        ),
        (
            "outer loop clockwise",
            Record::Loop { face: 0, r#loop: 0 },
            Box::new(|p: &Problem| {
                matches!(p, Problem::Turn { outer: true, area } if (*area + 1.).abs() < 1e-12)
                    && p.to_string() == "outer loop runs clockwise in (u, v) (signed area -1)"
            }),
            {
                // The plane's axes swapped turn its natural normal over: the loop, unchanged in
                // space, runs clockwise about it.
                let mut b = sheet();
                if let Surface::Plane { u_axis, v_axis, .. } = &mut b.faces[0].surface {
                    std::mem::swap(u_axis, v_axis);
                }
                fit_pcurves(&mut b);
                b
            },
        ),
    ];
    for (name, record, problem, body) in cases {
        body.validate().unwrap_or_else(|e| panic!("{name}: structurally valid: {e}"));
        let error = body.check_geometry().err().unwrap_or_else(|| panic!("{name} was accepted"));
        assert_eq!(error.record, record, "{name}: {error}");
        assert!(problem(&error.problem), "{name}: {error}");
    }
}

/// Records `validate` would refuse, checked anyway: errors, never panics.
#[test]
fn hostile_records_never_panic_the_geometry_check() {
    let mut cases: Vec<Brep> = Vec::new();
    let mut b = sheet();
    b.vertices[0].point = [f64::NAN; 3];
    cases.push(b);
    let mut b = sheet();
    b.edges[0].vertices = [u32::MAX; 2];
    cases.push(b);
    let mut b = sheet();
    if let Loop::Edges { edges } = &mut b.faces[0].loops[0] {
        edges[2].edge = u32::MAX;
    }
    cases.push(b);
    let mut b = sheet();
    shift(pcurve_of(&mut b, 0, 0), [f64::NAN, 0.]);
    cases.push(b);
    let mut b = sheet();
    *pcurve_of(&mut b, 0, 3) = Curve2::Spline { knots: vec![], points: vec![] };
    cases.push(b);
    let mut b = sheet();
    b.edges[1].interval = [f64::NEG_INFINITY, f64::INFINITY];
    cases.push(b);
    let mut b = sheet_with_lower_forms();
    b.faces[0].loops[1] = Loop::Vertex { vertex: u32::MAX };
    cases.push(b);
    let mut b = tube();
    b.faces[0].surface = Surface::Cylinder { frame: a_frame(), radius: f64::NAN };
    cases.push(b);
    let mut b = tube();
    b.edges[0].tolerance = f64::NAN;
    cases.push(b);
    let mut b = sheet();
    let circle = Box::new(Curve::Circle { frame: a_frame(), radius: 1. });
    let identity = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
    b.faces[0].surface = Surface::PlacedRevolution { profile: circle, origin: [0.; 3], axis: [0., 0., 1.], matrix: identity, forward: true };
    cases.push(b);
    for (i, b) in cases.iter().enumerate() {
        let checked = std::panic::catch_unwind(|| b.check_geometry());
        assert!(matches!(checked, Ok(Err(_))), "case {i}: {checked:?}");
    }
}

#[test]
fn a_reversed_face_is_walked_backwards() {
    // The sheet seen from below: its first (only) side is reversed, so the canonical walk
    // meets its loop's edges last to first.
    let mut b = sheet();
    if let Shell::Faces { faces, .. } = &mut b.shells[0] {
        faces[0].forward = false;
    }
    assert!(b.validate().is_err());
    // Renumber the edges (and vertices) in the backward walk's order.
    let Loop::Edges { edges } = &mut b.faces[0].loops[0] else { panic!("an edge loop") };
    let order: Vec<u32> = edges.iter().rev().map(|u| u.edge).collect();
    let mut new = vec![0; order.len()];
    for (i, e) in order.iter().enumerate() {
        new[*e as usize] = i as u32;
    }
    for u in edges.iter_mut() {
        u.edge = new[u.edge as usize];
    }
    let mut edges_ = b.edges.clone();
    for (old, e) in b.edges.iter().enumerate() {
        edges_[new[old] as usize] = e.clone();
    }
    b.edges = edges_;
    let mut seen = Vec::new();
    for e in &b.edges {
        for v in e.vertices {
            if !seen.contains(&v) {
                seen.push(v);
            }
        }
    }
    let vertices = seen.iter().map(|&v| b.vertices[v as usize].clone()).collect();
    b.vertices = vertices;
    for e in &mut b.edges {
        e.vertices = e.vertices.map(|v| seen.iter().position(|&s| s == v).unwrap() as u32);
    }
    for e in &mut b.edges {
        e.uses.clear();
    }
    let Loop::Edges { edges } = &b.faces[0].loops[0] else { panic!("an edge loop") };
    for (p, u) in edges.iter().enumerate().rev() {
        b.edges[u.edge as usize].uses.push(EdgeUseRef { face: 0, r#loop: 0, position: p as u32, upper: !u.forward });
    }
    b.validate().unwrap();
}

#[test]
fn spline_domains_weights_and_nesting_are_bounded() {
    let mut budget = Budget(0);
    assert!(knots(&[0., 0., 1., 1.], 2, &mut budget).is_ok());
    for bad in [vec![0., 0., 0., 0.], vec![0., 1., 0., 1.], vec![0., 0., f64::INFINITY, 1.]] {
        assert!(knots(&bad, 2, &mut Budget(0)).is_err());
    }
    let c = Curve::RationalSpline { knots: vec![0., 0., 1., 1.], homogeneous: vec![[0., 0., 0., 0.], [1., 0., 0., 1.]] };
    assert!(curve(&c, 0, &mut Budget(0)).is_err());
    let p = Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [2., 0., 0.] };
    assert!(surface(&p, 0, &mut Budget(0)).is_err());
    assert!(curve(&Curve::Line { start: [0.; 3], end: [1.; 3] }, MAX_DEPTH + 1, &mut Budget(0)).is_err());
    assert!(Budget(MAX_ITEMS).add(1).is_err());
    assert_eq!(curve_natural(&Curve::Spline { knots: vec![0., 0., 0., 1., 2., 2., 2.], points: vec![[0.; 3]; 4] }), Natural::Range(0., 2.));
}

#[test]
fn exact_procedural_records_are_validated_and_round_trip() {
    let plane = |z: f64| Box::new(Surface::Plane { origin: [0., 0., z], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] });
    let leader = Box::new(Curve::Polyline { points: vec![[0.; 3], [0.; 3], [1., 0., 0.]] });
    let c = Curve::SurfaceIntersection { surfaces: [plane(0.), plane(1.)], leader: leader.clone(), tolerance: 1e-6 };
    curve(&c, 0, &mut Budget(0)).unwrap();
    assert_eq!(curve_natural(&c), Natural::Range(0., 2.));
    assert_eq!(serde_json::from_str::<Curve>(&serde_json::to_string(&c).unwrap()).unwrap(), c);
    let line = Box::new(Curve::Line { start: [0.; 3], end: [1.; 3] });
    for bad in [
        Curve::SurfaceIntersection { surfaces: [plane(0.), plane(1.)], leader: line.clone(), tolerance: 1e-6 },
        Curve::SurfaceIntersection { surfaces: [plane(0.), plane(1.)], leader: leader.clone(), tolerance: f64::NAN },
        Curve::SurfaceIntersection { surfaces: [plane(f64::INFINITY), plane(1.)], leader, tolerance: 1e-6 },
    ] {
        assert!(curve(&bad, 0, &mut Budget(0)).is_err());
    }
    let identity = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
    let s = Surface::PlacedRevolution { profile: line.clone(), origin: [0.; 3], axis: [0., 0., 1.], matrix: identity, forward: false };
    surface(&s, 0, &mut Budget(0)).unwrap();
    assert_eq!(surface_natural(&s), [Natural::Periodic, UNBOUNDED]);
    assert_eq!(serde_json::from_str::<Surface>(&serde_json::to_string(&s).unwrap()).unwrap(), s);
    let mut flat = identity;
    flat[2] = [0.; 4];
    let bad = Surface::PlacedRevolution { profile: line, origin: [0.; 3], axis: [0., 0., 1.], matrix: flat, forward: true };
    assert!(surface(&bad, 0, &mut Budget(0)).is_err());
}

#[test]
fn analytic_records_are_unbounded_with_independent_frames() {
    let f = a_frame();
    let surfaces = [
        Surface::Cylinder { frame: f.clone(), radius: 2. },
        Surface::Cone { frame: f.clone(), radius_at_origin: 2., half_angle: 0.2 },
        Surface::Cone { frame: f.clone(), radius_at_origin: 2., half_angle: -0.2 },
        Surface::Sphere { frame: f.clone(), radius: 2. },
        Surface::Torus { frame: f.clone(), major_radius: 4., minor_radius: 1. },
    ];
    // A cone narrowing along z (a negative half angle, as SMLib makes them) lies below its apex.
    let apex = 2. / 0.2f64.tan();
    let narrowing = Surface::Cone { frame: f.clone(), radius_at_origin: 2., half_angle: -0.2 };
    assert_eq!(surface_natural(&narrowing)[1], Natural::Range(f64::NEG_INFINITY, apex));
    for flat in [0., 1e-13, -1e-13, std::f64::consts::FRAC_PI_2, -std::f64::consts::FRAC_PI_2] {
        assert!(surface(&Surface::Cone { frame: f.clone(), radius_at_origin: 2., half_angle: flat }, 0, &mut Budget(0)).is_err());
    }
    for s in surfaces {
        surface(&s, 0, &mut Budget(0)).unwrap();
        let encoded = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Surface>(&encoded).unwrap(), s);
        assert!(!encoded.contains("\"angle\"") && !encoded.contains("height") && !encoded.contains("longitude"));
    }
    for c in [Curve::Circle { frame: f.clone(), radius: 2. }, Curve::Ellipse { frame: f.clone(), radii: [2., 3.] }] {
        curve(&c, 0, &mut Budget(0)).unwrap();
        assert_eq!(serde_json::from_str::<Curve>(&serde_json::to_string(&c).unwrap()).unwrap(), c);
        assert_eq!(curve_natural(&c), Natural::Periodic);
        // A periodic edge may start anywhere and run up to one turn.
        interval(&[-1., TAU - 1.], Natural::Periodic).unwrap();
    }
    let mut bad = f;
    bad.y_axis = bad.x_axis;
    assert!(frame(&bad, &mut Budget(0)).is_err());
    assert!(interval(&[2., 1.], UNBOUNDED).is_err());
    assert!(radius(f64::NAN).is_err());
    assert!(surface(&Surface::Torus { frame: a_frame(), major_radius: 1., minor_radius: 2. }, 0, &mut Budget(0)).is_err());
}
