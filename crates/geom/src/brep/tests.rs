use super::*;
use crate::prelude::*;
use solidcore_ops::{label, pcurve, record_check as check};
use solidcore_record::brep::*;
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
        check::validate(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        solidcore_ops::record_check::agreement(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        let bytes = serde_json::to_vec(&body).unwrap();
        let restored: Brep = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body, restored, "{name}");
        assert!(!String::from_utf8(bytes).unwrap().contains("truck"));
    }
}

/// One defect each on a record with NURBS forms; every one is refused (the rest are the
/// record's own tests).
#[test]
fn one_defect_fixtures_on_forms_are_refused() {
    let cases: Vec<(&str, &str, Brep)> = vec![
        ("pcurve interval", "pcurve interval differs from its edge's", {
            let mut b = tube();
            if let Loop::Edges { edges } = &mut b.faces[0].loops[0] {
                edges[0].pcurve = segment(0., TAU - 1e-3);
            }
            b
        }),
        ("edge interval past its form", "parameter interval outside the natural domain", {
            let mut b = tube();
            b.edges[0].interval = [0., 4.1];
            b
        }),
        ("face domain outside the sphere", "parameter interval outside the natural domain", {
            let mut b = sheet();
            formed(&mut b, Surface::Sphere { frame: a_frame(), radius: 2. }, [[0., TAU], [-FRAC_PI_2, FRAC_PI_2]]);
            b.faces[0].domain[1][0] -= 0.1;
            b
        }),
        ("face domain past its form", "parameter interval outside the natural domain", {
            let mut b = tube();
            b.faces[0].domain[0] = [0., 4.1];
            b
        }),
        ("cone domain past its apex", "parameter interval outside the natural domain", {
            let mut b = sheet();
            formed(&mut b, Surface::Cone { frame: a_frame(), radius_at_origin: 1., half_angle: 0.25 * std::f64::consts::PI }, [[0., 1.], [-1., 1.]]);
            b.faces[0].domain[1][0] = -2.;
            b
        }),
    ];
    for (name, expected, body) in cases {
        let error = check::validate(&body).err().unwrap_or_else(|| panic!("{name} was accepted"));
        assert_eq!(error.0, expected, "{name}");
    }
}

/// Face 0 relabelled `surface` on its NURBS form over `domain` (its loops left as they were).
fn formed(b: &mut Brep, surface: Surface, domain: [[f64; 2]; 2]) {
    let (nurbs, domain) = form::surface(&surface, domain).expect("a form");
    let f = &mut b.faces[0];
    (f.surface, f.nurbs, f.domain) = (surface, Some(nurbs), domain);
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

/// A pcurve whose interval ends a rounding short of its edge's (as a kernel that snaps an edge's
/// end to its knot hands it back) is read at its end: the record validates and checks.
#[test]
fn a_pcurve_a_rounding_short_of_its_edge_is_read_at_its_end() {
    let mut b = sheet();
    let Curve2::Spline { knots, .. } = pcurve_of(&mut b, 0, 0) else { panic!("a spline pcurve") };
    let end = *knots.last().unwrap();
    let short = end - 1e-11 * end.abs().max(1.0);
    for k in knots.iter_mut().filter(|k| **k == end) {
        *k = short;
    }
    check::validate(&b).unwrap();
    solidcore_ops::record_check::agreement(&b).unwrap();
}

/// One geometric defect each, structurally valid: every one is refused, naming the record.
#[test]
fn geometric_defects_are_refused_by_record() {
    use solidcore_ops::record_check::{Problem, Record};
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
            let (a, z) = (pcurve::eval(pc, 0.).unwrap(), pcurve::eval(pc, 1.).unwrap());
            let middle = [0.5 * (a[0] + z[0]), 0.5 * (a[1] + z[1]) + 0.1];
            *pc = Curve2::Spline { knots: vec![0., 0., 0., 1., 1., 1.], points: vec![a, middle, z] };
            b
        }),
        (
            "seam pcurve on the wrong period",
            Record::Use { face: 0, r#loop: 0, position: 1 },
            Box::new(|p: &Problem| matches!(p, Problem::Period { .. })),
            {
                // One turn of the tube's NURBS form is four quarters.
                let mut b = tube();
                shift(pcurve_of(&mut b, 0, 1), [4., 0.]);
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
                let mid = |b: &mut Brep, k: usize| pcurve::eval(pcurve_of(b, 0, k), 0.).unwrap()[0];
                let by = mid(&mut b, 0) - mid(&mut b, 1);
                // On the sphere's NURBS form one turn is four quarters.
                assert!((by.abs() - 4.).abs() < 1e-9, "a period apart: {by}");
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
        check::validate(&body).unwrap_or_else(|e| panic!("{name}: structurally valid: {e}"));
        let error = solidcore_ops::record_check::agreement(&body).err().unwrap_or_else(|| panic!("{name} was accepted"));
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
    if let Some(Surface::RationalSpline { homogeneous, .. }) = &mut b.faces[0].nurbs {
        homogeneous[0][0][0] = f64::NAN;
    }
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
        let checked = std::panic::catch_unwind(|| solidcore_ops::record_check::agreement(b));
        assert!(matches!(checked, Ok(Err(_))), "case {i}: {checked:?}");
    }
}

/// The record's geometric records the kernel checks, which the record's structural validation
/// leaves alone: frames, planes, placements and a cone's apex.
#[test]
fn frames_planes_placements_and_apexes_are_the_kernels() {
    let f = a_frame();
    let mut skewed = f.clone();
    skewed.y_axis = skewed.x_axis;
    assert!(check::frame(&f).is_ok());
    assert_eq!(check::frame(&skewed).map_err(|e| e.0), Err("nonorthonormal frame"));
    let p = Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [2., 0., 0.] };
    assert_eq!(check::surface(&p).map_err(|e| e.0), Err("degenerate plane"));
    let line = Box::new(Curve::Line { start: [0.; 3], end: [1.; 3] });
    let identity = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
    let s = Surface::PlacedRevolution { profile: line.clone(), origin: [0.; 3], axis: [0., 0., 1.], matrix: identity, forward: false };
    check::surface(&s).unwrap();
    assert_eq!(label::surface_natural(&s), [Natural::Periodic, UNBOUNDED]);
    let mut flat = identity;
    flat[2] = [0.; 4];
    let bad = Surface::PlacedRevolution { profile: line, origin: [0.; 3], axis: [0., 0., 1.], matrix: flat, forward: true };
    assert_eq!(check::surface(&bad).map_err(|e| e.0), Err("singular revolution placement"));
    // A cone narrowing along z (a negative half angle, as SMLib makes them) lies below its apex.
    let apex = 2. / 0.2f64.tan();
    let narrowing = Surface::Cone { frame: f, radius_at_origin: 2., half_angle: -0.2 };
    assert_eq!(label::surface_natural(&narrowing)[1], Natural::Range(f64::NEG_INFINITY, apex));
    // A record with a skewed frame is structurally valid, and refused by the kernel's check.
    let mut b = tube();
    let Surface::Cylinder { frame, .. } = &mut b.faces[0].surface else { panic!("a cylinder") };
    frame.y_axis = frame.x_axis;
    b.validate().unwrap();
    assert_eq!(check::validate(&b).map_err(|e| e.0), Err("nonorthonormal frame"));
}

/// A cylinder face round its axis with a hole beside its seam: the seam at 350 degrees (so its
/// loop projects from 350 degrees on), the hole at 15..25 degrees (which on its own projects a
/// turn below). The face's box is the outer loop's turn with the hole in it; the hole's own
/// placement made a box from 15 degrees that cut the turn short and left the seam outside it
/// (#4598: a bored rod read back with its bore's pcurve a period off its face).
#[test]
fn a_hole_beside_the_seam_lies_in_its_face_turn() {
    let (r, h) = (20.0f64, 60.0);
    let cylinder = Surface::Cylinder { frame: Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] }, radius: r };
    let s = -10f64.to_radians();
    let at = |a: f64, z: f64| [r * a.cos(), r * a.sin(), z];
    let ring =
        |z: f64| Curve::Circle { frame: Frame { origin: [0., 0., z], x_axis: [s.cos(), s.sin(), 0.], y_axis: [-s.sin(), s.cos(), 0.] }, radius: r };
    let (bottom, top) = (ring(0.), ring(h));
    let seam = Curve::Line { start: at(s, 0.), end: at(s, h) };
    let (a0, a1) = (15f64.to_radians(), 25f64.to_radians());
    let hole =
        Curve::Spline { knots: vec![0., 0., 1., 2., 3., 4., 4.], points: vec![at(a0, 25.), at(a1, 25.), at(a1, 35.), at(a0, 35.), at(a0, 25.)] };
    let outer = vec![(&bottom, [0., TAU], true), (&seam, [0., 1.], true), (&top, [0., TAU], false), (&seam, [0., 1.], false)];
    let inner = vec![(&hole, [0., 4.], true)];
    let [u, v] = form::extent(&cylinder, &[outer, inner]).expect("an extent");
    assert!((u[1] - u[0] - TAU).abs() < 1e-9, "one turn: {u:?}");
    assert!((u[0].rem_euclid(TAU) - s.rem_euclid(TAU)).abs() < 1e-9, "from the seam: {u:?}");
    let hole_u = a0 + ((u[0] - a0) / TAU).ceil() * TAU;
    assert!(hole_u > u[0] && hole_u + (a1 - a0) < u[1], "the hole in the turn: {u:?}");
    assert!(v[0].abs() < 1e-9 && (v[1] - h).abs() < 1e-9, "{v:?}");
}

/// A hemisphere bounded by its rim and its seam, its loop starting a quarter turn along the rim
/// (the order a re-imported dimple's loop came in): along the rim to the seam, down the seam to
/// the pole, up it on the other side and round the rest of the rim. Through the pole the loop goes
/// on a turn away; continuing from the point before the pole put the seam's second use on the
/// first and the box a quarter turn off the seam (oracle 14: the re-imported face's form over
/// -270..90 degrees, its pcurves over 0..360, and its area a quarter short).
#[test]
fn a_loop_through_a_pole_closes_its_turn_from_the_seam() {
    let r = 15.0f64;
    let sphere = Surface::Sphere { frame: Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] }, radius: r };
    let rim = Curve::Circle { frame: Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] }, radius: r };
    let seam = Curve::Circle { frame: Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 0., -1.] }, radius: r };
    let outer =
        vec![(&rim, [0., FRAC_PI_2], false), (&seam, [0., FRAC_PI_2], true), (&seam, [0., FRAC_PI_2], false), (&rim, [FRAC_PI_2, TAU], false)];
    let [u, v] = form::extent(&sphere, &[outer]).expect("an extent");
    assert!((u[1] - u[0] - TAU).abs() < 1e-9, "one turn: {u:?}");
    let from = u[0].rem_euclid(TAU);
    assert!(from.min(TAU - from) < 1e-9, "from the seam: {u:?}");
    assert!((v[0] + FRAC_PI_2).abs() < 1e-9 && v[1].abs() < 1e-9, "{v:?}");
}

/// A face's box reaches its boundary's extremes, not its samples' nearest ones: an ellipse cut
/// from a cylinder of radius 20 by a plane at 45 degrees rises 20 above its centre, at an angle
/// between the points the box is sampled at. Short of it, the face's NURBS form stopped below the
/// edge (#4598: the bore of a re-imported rod ended 2.3e-4 short of where it meets the rod).
#[test]
fn a_face_box_reaches_its_boundary_extremes() {
    let r = 20.0f64;
    let cylinder = Surface::Cylinder { frame: Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] }, radius: r };
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let cut = Curve::Ellipse { frame: Frame { origin: [0., 0., 30.], x_axis: [c, 0., c], y_axis: [0., 1., 0.] }, radii: [r * 2f64.sqrt(), r] };
    let [_, v] = form::extent(&cylinder, &[vec![(&cut, [0.1, 0.1 + TAU], true)]]).expect("an extent");
    assert!((v[0] - 10.0).abs() < 1e-9 && (v[1] - 50.0).abs() < 1e-9, "{v:?}");
}

/// A point next to a sphere's pole is a pole to within the reach it is given: crossing half a
/// turn of longitude there moves it about π times its distance from the axis, and no further.
#[test]
fn a_point_beside_a_pole_is_one_within_its_reach() {
    let sphere = eval::SurfaceEval::new(&Surface::Sphere { frame: a_frame(), radius: 2. }).unwrap();
    let near = [0.3, FRAC_PI_2 - 1e-7];
    // 2e-7 mm from the axis: half a turn moves it 2π·1e-7.
    assert_eq!(sphere.pole(near), [false, false]);
    assert_eq!(sphere.pole_within(near, 1e-6), [true, false]);
    assert_eq!(sphere.pole_within(near, 1e-7), [false, false]);
    assert_eq!(sphere.pole_within([0.3, 0.2], 1e-3), [false, false]);
}

/// The analytic surfaces' partials up to the fourth order agree with their first and second
/// derivatives and are each the derivative of the order below (central differences).
#[test]
fn analytic_partials_are_the_derivatives_of_the_order_below() {
    use crate::tess::partials::MAX_PARTIAL_ORDER;
    let f = Frame { origin: [1., 2., 3.], x_axis: [0.6, 0.8, 0.], y_axis: [0., 0., 1.] };
    let surfaces = [
        Surface::Cylinder { frame: f.clone(), radius: 3. },
        Surface::Cone { frame: f.clone(), radius_at_origin: 2., half_angle: 0.3 },
        Surface::Sphere { frame: f.clone(), radius: 4. },
        Surface::Torus { frame: f.clone(), major_radius: 5., minor_radius: 1.5, tube_start: 0.0 },
    ];
    let (u, v, h) = (0.8, 0.4, 1e-4);
    let close = |a: solidcore_record::Vec3, b: solidcore_record::Vec3, tol: f64| a.dist(b) <= tol * (1.0 + a.len().max(b.len()));
    for s in &surfaces {
        let e = eval::SurfaceEval::new(s).unwrap();
        let d = e.partials(u, v).unwrap();
        let (p, su, sv) = e.derivs(u, v).unwrap();
        let (suu, suv, svv) = e.second(u, v).unwrap();
        for (a, b) in [(d[0][0], p), (d[1][0], su), (d[0][1], sv), (d[2][0], suu), (d[1][1], suv), (d[0][2], svv)] {
            assert!(close(a, b, 1e-12), "{s:?}: {a:?} vs {b:?}");
        }
        let (up, um, vp, vm) =
            (e.partials(u + h, v).unwrap(), e.partials(u - h, v).unwrap(), e.partials(u, v + h).unwrap(), e.partials(u, v - h).unwrap());
        for i in 0..MAX_PARTIAL_ORDER {
            for j in 0..MAX_PARTIAL_ORDER - i {
                assert!(close((up[i][j] - um[i][j]) * (0.5 / h), d[i + 1][j], 1e-6), "{s:?} d/du ({i},{j})");
                assert!(close((vp[i][j] - vm[i][j]) * (0.5 / h), d[i][j + 1], 1e-6), "{s:?} d/dv ({i},{j})");
            }
        }
    }
}

/// A curve's curvature vector, exactly, on every kind of record: a circle's is 1/R towards its
/// centre (as a conic and as SMLib's rational quadratic B-spline of it); an ellipse's at its
/// vertices is a/b² and b/a²; a line is straight; and the circle where a plane z = 3 cuts a
/// sphere of radius 5 about the origin, traced from a rough leader, has radius 4.
#[test]
fn curve_jets_are_exact_on_every_record() {
    use solidcore_record::Vec3;
    let near = |a: Vec3, b: Vec3| a.dist(b) <= 1e-9 * (1.0 + b.len());
    let jet = |c: &Curve, t: f64| eval::CurveEval::new(c).unwrap().jet(t).unwrap();
    let flat = Frame { origin: [1., 2., 3.], x_axis: [0.6, 0.8, 0.], y_axis: [-0.8, 0.6, 0.] };
    let o = Vec3::new(1., 2., 3.);
    for t in [0.0, 0.7, 2.5, 5.9] {
        let j = jet(&Curve::Circle { frame: flat.clone(), radius: 4. }, t);
        assert!(near(j.curvature, (o - j.point) * (1.0 / 16.0)), "{j:?}");
        assert!(j.tangent.dot(j.curvature).abs() < 1e-12);
    }
    // A quarter circle of radius 2 as a rational quadratic.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter =
        Curve::RationalSpline { knots: vec![0., 0., 0., 1., 1., 1.], homogeneous: vec![[2., 0., 0., 1.], [2. * w, 2. * w, 0., w], [0., 2., 0., 1.]] };
    for t in [0.0, 0.3, 0.5, 1.0] {
        let j = jet(&quarter, t);
        assert!(near(j.curvature, j.point * -0.25), "{t}: {j:?}");
    }
    let (a, b) = (10.0, 4.0);
    let ellipse = Curve::Ellipse { frame: flat.clone(), radii: [a, b] };
    assert!((jet(&ellipse, 0.0).curvature.len() - a / (b * b)).abs() < 1e-12);
    assert!((jet(&ellipse, FRAC_PI_2).curvature.len() - b / (a * a)).abs() < 1e-12);
    assert_eq!(jet(&Curve::Line { start: [0.; 3], end: [1., 2., 3.] }, 0.4).curvature, Vec3::ZERO);
    // The plane z = 3 and a sphere of radius 5: a circle of radius 4 about (0, 0, 3).
    let z = Frame { origin: [0.; 3], x_axis: [1., 0., 0.], y_axis: [0., 1., 0.] };
    let leader = Curve::Polyline { points: (0..=12).map(|k| (k as f64 * TAU / 12.0).sin_cos()).map(|(s, c)| [4.1 * c, 4.1 * s, 3.2]).collect() };
    let cut = Curve::SurfaceIntersection {
        surfaces: [
            Box::new(Surface::Plane { origin: [0., 0., 3.], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] }),
            Box::new(Surface::Sphere { frame: z, radius: 5. }),
        ],
        leader: Box::new(leader),
        tolerance: 1e-6,
    };
    for t in [0.0, 1.5, 4.25, 11.0] {
        let j = jet(&cut, t);
        let centre = Vec3::new(0., 0., 3.);
        assert!((j.point.dist(centre) - 4.0).abs() < 1e-9 && (j.point.z - 3.0).abs() < 1e-12, "{j:?}");
        assert!(near(j.curvature, (centre - j.point) * (1.0 / 16.0)), "{t}: {j:?}");
        assert!(j.tangent.dot(j.point - centre).abs() < 1e-9);
    }
}

#[test]
fn a_form_on_arcs_of_its_own_is_the_surface_and_does_not_close_around_an_open_face() {
    // A torus face round 280° of its tube: on the quarter grid it needs all four quarters, a form
    // that closes on itself; on arcs of its own it starts where the face does and stops past
    // where it ends.
    let torus = Surface::Torus { frame: a_frame(), major_radius: 5., minor_radius: 1.5, tube_start: 0. };
    let label = [[0.3, 1.2], [-2.9, 2.0]];
    let tube = |s: &Surface| match s {
        Surface::RationalSpline { v_knots, .. } => (v_knots.first().copied().unwrap(), v_knots.last().copied().unwrap()),
        _ => panic!("not a NURBS: {s:?}"),
    };
    let (grid, _) = form::surface(&torus, label).unwrap();
    assert_eq!(tube(&grid), (-2.0, 2.0), "the grid's form is the whole tube");
    let tol = 1e-3;
    let (own, domain) = form::surface_on_own_arcs(&torus, label, tol).unwrap();
    let (v0, v1) = tube(&own);
    assert_eq!((v0, v1), (0.0, 4.0));
    let (label_at, own_at) = (eval::SurfaceEval::new(&torus).unwrap(), eval::SurfaceEval::new(&own).unwrap());
    let u = domain[0][0];
    assert!(own_at.point([u, v0]).unwrap().dist(own_at.point([u, v1]).unwrap()) > 0.1, "the form does not close");
    // Every point of the form is on the torus: (√(x² + y²) − R)² + z² = r² in its frame.
    for i in 0..=8 {
        for j in 0..=12 {
            let uv = [domain[0][0] + (domain[0][1] - domain[0][0]) * i as f64 / 8., v0 + (v1 - v0) * j as f64 / 12.];
            let p = own_at.point(uv).unwrap() - solidcore_record::Vec3::new(1., 2., 3.);
            let off = ((p.x.hypot(p.y) - 5.).powi(2) + p.z.powi(2)).sqrt() - 1.5;
            assert!(off.abs() < 1e-12, "{uv:?}: {off}");
        }
    }
    // Its domain is the face's: the corners land where the label's do, the form past them by the
    // angle the tolerance makes.
    for (a, b) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let (p, q) = (label_at.point([label[0][a], label[1][b]]).unwrap(), own_at.point([domain[0][a], domain[1][b]]).unwrap());
        assert!(p.dist(q) < 1e-9, "corner {a}{b}: {p:?} vs {q:?}");
    }
    assert!(domain[1][0] > v0 && domain[1][1] < v1, "{domain:?}");
    // A whole turn stays on the grid (periodic), and a plane has no form.
    let (whole, _) = form::surface_on_own_arcs(&torus, [[0.3, 1.2], [-FRAC_PI_2, -FRAC_PI_2 + TAU]], tol).unwrap();
    assert_eq!(tube(&whole), (-1.0, 3.0));
    assert!(form::surface_on_own_arcs(&Surface::Plane { origin: [0.; 3], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] }, label, tol).is_none());
}

/// A whole turn whose seam lies a rounding short of a quarter line (as a file's seam on its
/// frame's x axis measures, 2π − 2e-12 or −2e-12) has its form from that quarter line: four
/// quarters that end where the seam's second use lies (#12760; from the quarter before, the
/// form ended a quarter short of it and the face's last quarter was off its form).
#[test]
fn a_whole_turn_a_rounding_short_of_a_quarter_line_has_its_form_from_it() {
    let cylinder = Surface::Cylinder { frame: a_frame(), radius: 21.2 };
    let u_knots = |s: &Surface| match s {
        Surface::RationalSpline { u_knots, .. } => (u_knots.first().copied().unwrap(), u_knots.last().copied().unwrap()),
        _ => panic!("not a NURBS: {s:?}"),
    };
    for (start, q) in [(TAU - 2e-12, 4.0), (-2.2e-12, 0.0), (TAU + 2e-12, 4.0)] {
        let (form, domain) = form::surface_within(&cylinder, [[start, start + TAU], [0., 10.]], 2e-5).unwrap();
        assert_eq!(u_knots(&form), (q, q + 4.0), "seam at {start}");
        assert_eq!(domain[0], [q, q + 4.0], "seam at {start}");
    }
}

/// A disc (a plane bounded by a circle) mapped by a scaling and a shear stays a plane bounded by
/// an ellipse: the record is valid and agrees with its topology, the circle's NURBS form mapped
/// with its parameter lies on the ellipse labelling it, and a reflection is left to the kernel.
#[test]
fn a_disc_scaled_and_sheared_is_a_plane_bounded_by_an_ellipse() {
    use solidcore_record::Vec3;
    let disc = one_face(
        Surface::Plane { origin: [1., 2., 3.], u_axis: [1., 0., 0.], v_axis: [0., 1., 0.] },
        [[-3., 3.], [-3., 3.]],
        vec![(Curve::Circle { frame: a_frame(), radius: 2. }, [0., TAU], [0, 0])],
        &[[3., 2., 3.]],
        &[(0, true)],
    );
    check::validate(&disc).unwrap();
    let m = [[2., 0., 0., 0.], [0.7, 3., 0., 0.], [0.3, 0., 0.5, 0.], [4., -1., 2., 1.]];
    let mapped = placement::mapped(&disc, &m).unwrap();
    check::validate(&mapped).unwrap();
    solidcore_ops::record_check::agreement(&mapped).unwrap();
    assert!(matches!(mapped.faces[0].surface, Surface::Plane { .. }));
    let edge = &mapped.edges[0];
    let Curve::Ellipse { frame, radii } = &edge.curve else { panic!("{:?}", edge.curve) };
    let (o, x, y) = (Vec3::from(frame.origin), Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
    let form = eval::CurveEval::new(edge.nurbs.as_ref().unwrap()).unwrap();
    for k in 0..12 {
        let t = edge.interval[0] + (edge.interval[1] - edge.interval[0]) * k as f64 / 12.0;
        let d = form.jet(t).unwrap().point - o;
        let on = (d.dot(x) / radii[0]).powi(2) + (d.dot(y) / radii[1]).powi(2);
        assert!((on - 1.0).abs() < 1e-9 && d.dot(x.cross(y)).abs() < 1e-9, "{t}: {on}");
    }
    let mirror = [[-1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
    assert!(placement::mapped(&disc, &mirror).is_none());
}

mod native_ray {
    use super::super::ray::{ClipDepth, Front, Scene};
    use solidcore_record::brep::*;

    type Wire = [[f64; 2]; 4];

    #[test]
    fn viewport_clip_encloses_both_planes_without_admitting_boundary_hits() {
        let orthographic = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let perspective = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 2.0, 1.0], [0.0, 0.0, -2.0, 0.0]];
        for (matrix, origin) in [(orthographic, [0.0, 0.0, -1.0]), (perspective, [0.0; 3])] {
            let clip = ClipDepth::from_projection(matrix, origin, [0.0, 0.0, 1.0]).unwrap();
            let [lo, hi] = clip.range();
            assert!(lo <= 1.0 && hi >= 2.0 && hi - lo < 1.00001);
            assert!(clip.contains([1.1, 1.9]));
            assert!(!clip.contains([1.0, 1.9]));
            assert!(!clip.contains([1.1, 2.0]));
        }
        assert!(ClipDepth::from_projection(orthographic, [1.0, 0.0, -1.0], [0.0, 0.0, 1.0]).is_none());
    }

    fn square(lo: f64, hi: f64) -> Wire {
        [[lo, lo], [hi, lo], [hi, hi], [lo, hi]]
    }

    fn sheets(spec: &[(f64, Wire, Option<Wire>)]) -> Brep {
        let mut record = Brep {
            version: VERSION,
            tolerance: 1e-7,
            regions: vec![Region { shells: (0..spec.len() as u32).collect(), void: false }],
            shells: Vec::new(),
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
        };
        for (native, &(z, outer, hole)) in spec.iter().enumerate() {
            let loops = std::iter::once(outer)
                .chain(hole.map(|mut points| {
                    points.reverse();
                    points
                }))
                .map(|wire| {
                    let base = record.vertices.len() as u32;
                    record.vertices.extend(wire.map(|[x, y]| Vertex { point: [x, y, z], tolerance: 1e-7 }));
                    let edges = (0..4)
                        .map(|i| {
                            let next = (i + 1) % 4;
                            let [[ax, ay], [bx, by]] = [wire[i], wire[next]];
                            let index = record.edges.len() as u32;
                            record.edges.push(Edge {
                                curve: Curve::Line { start: [ax, ay, z], end: [bx, by, z] },
                                nurbs: None,
                                interval: [0.0, 1.0],
                                vertices: [base + i as u32, base + next as u32],
                                tolerance: 1e-7,
                                uses: Vec::new(),
                            });
                            OrientedEdge {
                                edge: index,
                                forward: true,
                                pcurve: Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![wire[i], wire[next]] },
                            }
                        })
                        .collect();
                    Loop::Edges { edges }
                })
                .collect();
            record.faces.push(Face {
                surface: Surface::Plane { origin: [0.0, 0.0, z], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] },
                nurbs: None,
                domain: [[outer[0][0], outer[2][0]], [outer[0][1], outer[2][1]]],
                loops,
            });
            record.shells.push(Shell::Faces { faces: vec![OrientedFace { face: native as u32, forward: true }], wires: Vec::new() });
        }
        let canonical = record.canonical().unwrap();
        assert_eq!(canonical.faces, (0..spec.len() as u32).collect::<Vec<_>>());
        canonical.brep
    }

    #[test]
    fn trim_hole_reveals_a_rear_owner_and_context_keeps_its_own_trim() {
        let outer = square(-2.0, 2.0);
        let hole = Some(square(-0.5, 0.5));
        let record = sheets(&[(1.0, outer, hole), (2.0, outer, None)]);
        let scene = Scene::prepare(&record, &[Some(55), Some(7)], &[], &mut 2_000_000).unwrap();
        assert!(matches!(scene.front([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap(), Front::Hit { owner: 7, .. }));
        assert!(matches!(scene.front([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap(), Front::Hit { owner: 55, .. }));
        let rear = sheets(&[(2.0, outer, None)]);
        let context = sheets(&[(1.0, outer, hole)]);
        let scene = Scene::prepare(&rear, &[Some(7)], &[&context], &mut 2_000_000).unwrap();
        assert!(matches!(scene.front([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap(), Front::Hit { owner: 7, .. }));
        assert!(matches!(scene.front([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap(), Front::Occluded { .. }));
    }

    #[test]
    fn context_cylinder_only_blocks_rays_that_enter_its_bounds() {
        let rear = sheets(&[(3.0, square(-4.0, 4.0), None)]);
        let mut context = sheets(&[(1.0, square(-2.0, 2.0), None)]);
        context.faces[0].surface =
            Surface::Cylinder { frame: Frame { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: 1.0 };
        let scene = Scene::prepare(&rear, &[Some(7)], &[&context], &mut 2_000_000).unwrap();
        let hit = |x| scene.front([x, 0.3, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap();
        assert!(matches!(hit(2.5), Front::Hit { owner: 7, .. }));
        assert_eq!(hit(0.5), Front::Unresolved);
    }

    #[test]
    fn a_procedural_blend_net_cannot_clear_a_rear_owner() {
        use solidcore_record::brep::blend::{
            BlendDefinition, BlendDefinitionRecord, NurbsCurveRecord, NurbsSurfaceRecord, ParameterMapRecord, RadiusRecord, SupportKindRecord,
            SupportRecord,
        };
        let rear = sheets(&[(2.0, square(-2.0, 2.0), None)]);
        let mut context = sheets(&[(1.0, square(0.0, 1.0), None)]);
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        let support = |vertical| SupportRecord::Nurbs {
            kind: SupportKindRecord::Plane,
            form: NurbsSurfaceRecord {
                u_degree: 1,
                v_degree: 1,
                u_count: 2,
                v_count: 2,
                u_knots: knots.clone(),
                v_knots: knots.clone(),
                points: [0.0, 1.0].into_iter().flat_map(|x| [0.0, 1.0].map(|v| if vertical { [x, 0.0, 1.0 + v] } else { [x, v, 1.0] })).collect(),
                weights: None,
            },
        };
        let line = |points, dim| NurbsCurveRecord { dim, points_3d: dim == 3, degree: 1, knots: knots.clone(), points, weights: None };
        context.faces[0].surface = Surface::Blend {
            definition: Box::new(BlendDefinition::Procedural(Box::new(BlendDefinitionRecord {
                supports: [support(false), support(true)],
                sides: [1.0, -1.0],
                radius: RadiusRecord::Constant { offsets: [0.2, 0.2] },
                order: 1,
                thumbweight: 0.0,
                leader: line(vec![[0.0, 0.2, 1.2], [1.0, 0.2, 1.2]], 3),
                seeds: [line(vec![[0.0, 0.2, 0.0], [1.0, 0.2, 0.0]], 2), line(vec![[0.0, 0.2, 0.0], [1.0, 0.2, 0.0]], 2)],
                bound: 0.2,
                map: ParameterMapRecord { a: [0.0; 2], b: [1.0; 2], swapped: false },
                chamfer: None,
            }))),
        };
        context.faces[0].nurbs = Some(Surface::Spline {
            u_knots: knots.clone(),
            v_knots: knots,
            points: vec![vec![[0.0, 0.0, 1.0], [0.0, 1.0, 1.0]], vec![[1.0, 0.0, 1.0], [1.0, 1.0, 1.0]]],
        });
        let scene = Scene::prepare(&rear, &[Some(7)], &[&context], &mut 2_000_000).unwrap();
        assert_eq!(scene.front([-0.1, 0.4, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap(), Front::Unresolved);
    }

    #[test]
    fn planar_trim_joins_block_only_with_qualified_native_vertex_closure() {
        let rear = sheets(&[(2.0, square(-2.0, 2.0), None)]);
        let exact = sheets(&[(1.0, square(-1.0, 1.0), None)]);
        let shifted = |displacement: f64| {
            let mut context = exact.clone();
            let Loop::Edges { edges } = &mut context.faces[0].loops[0] else { return None };
            let Curve2::Spline { points, .. } = &mut edges[0].pcurve else { return None };
            let original = points[0];
            points[0][0] -= original[0].signum() * displacement;
            Some((context, original))
        };
        let query = |context: &Brep, [x, y]: [f64; 2]| {
            let scene = Scene::prepare(&rear, &[Some(7)], &[context], &mut 2_000_000).unwrap();
            scene.front([x, y, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap()
        };
        let (small, original) = shifted(1e-8).unwrap();
        let near_join = original.map(|x| x - x.signum() * 1e-7);
        assert!(matches!(query(&exact, near_join), Front::Occluded { .. }));
        assert!(matches!(query(&small, [0.3, 0.4]), Front::Occluded { depth } if depth[0] <= 1.0 && depth[1] >= 1.0));
        assert_eq!(query(&small, near_join), Front::Unresolved);
        assert_eq!(query(&shifted(1e-2).unwrap().0, [0.3, 0.4]), Front::Unresolved);
    }

    #[test]
    fn curved_side_join_proves_the_front_owner_but_keeps_the_native_tolerance_band() {
        let rear = sheets(&[(2.0, square(-2.0, 2.0), None)]);
        let mut exact = sheets(&[(1.0, square(0.0, 1.0), None)]);
        let lift = |x, y| [x, y, 1.0 + x * y * 0.25];
        exact.faces[0].surface = Surface::Spline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            points: [0.0, 1.0].map(|u| [0.0, 1.0].map(|v| lift(u, v)).to_vec()).to_vec(),
        };
        for vertex in &mut exact.vertices {
            vertex.point = lift(vertex.point[0], vertex.point[1]);
        }
        for edge in &mut exact.edges {
            let [start, end] = edge.vertices.map(|vertex| exact.vertices[vertex as usize].point);
            edge.curve = Curve::Line { start, end };
        }
        let shifted = |x, y| {
            let mut result = exact.clone();
            let Loop::Edges { edges } = &mut result.faces[0].loops[0] else { panic!("one edge loop") };
            let Curve2::Spline { points, .. } = &mut edges[0].pcurve else { panic!("a linear pcurve") };
            assert_eq!(points[0], [0.0, 0.0]);
            points[0] = [x, y];
            result
        };
        let query = |context: &Brep, [x, y]: [f64; 2]| {
            let scene = Scene::prepare(&rear, &[Some(7)], &[context], &mut 2_000_000).unwrap();
            scene.front([x, y, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap()
        };
        let rounded = shifted(2.0 * f64::EPSILON, 0.0);
        let depth = lift(0.3, 0.4)[2];
        assert!(matches!(query(&rounded, [0.3, 0.4]), Front::Occluded { depth: [lo, hi] } if lo <= depth && depth <= hi));
        assert!(matches!(query(&rounded, [1.3, 0.4]), Front::Hit { owner: 7, .. }));
        assert!(matches!(query(&exact, [1e-7, 1e-7]), Front::Occluded { .. }));
        assert_eq!(query(&rounded, [1e-7, 1e-7]), Front::Unresolved);
        let rounded_corner = shifted(2.0 * f64::EPSILON, f64::EPSILON);
        assert!(matches!(query(&rounded_corner, [0.3, 0.4]), Front::Occluded { depth: [lo, hi] } if lo <= depth && depth <= hi));
        assert_eq!(query(&rounded_corner, [1e-7, 1e-7]), Front::Unresolved);
        let mut false_corner = rounded_corner;
        false_corner.vertices[0].point[2] += 1e-2;
        assert_eq!(query(&false_corner, [0.3, 0.4]), Front::Unresolved);
        assert_eq!(query(&shifted(1e-2, 0.0), [0.3, 0.4]), Front::Unresolved);
        assert_eq!(query(&shifted(1e-10, 1e-10), [0.3, 0.4]), Front::Unresolved);
    }

    #[test]
    fn nearer_unresolved_face_blocks_a_different_owner() {
        let record = sheets(&[(1.0, square(0.0, 1.0), None), (2.0, square(-2.0, 2.0), None)]);
        let query = |owners| {
            let scene = Scene::prepare(&record, owners, &[], &mut 2_000_000).unwrap();
            scene.front([0.0, 0.25, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap()
        };
        assert_eq!(query(&[Some(1), Some(2)]), Front::Unresolved);
        assert!(matches!(query(&[Some(2), Some(2)]), Front::Hit { owner: 2, depth: [lo, hi] } if lo <= 1.0 && hi >= 2.0));
        assert!(matches!(query(&[None, Some(2)]), Front::Unresolved));
    }

    #[test]
    fn overlapping_depth_intervals_cannot_choose_a_different_owner() {
        let outer = square(-2.0, 2.0);
        let record = sheets(&[(2.0, outer, None), (2.0_f64.next_up(), outer, None)]);
        let query = |owners| {
            let scene = Scene::prepare(&record, owners, &[], &mut 2_000_000).unwrap();
            scene.front([0.0, 0.25, -1.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap()
        };
        assert_eq!(query(&[Some(1), Some(2)]), Front::Unresolved);
        assert!(matches!(query(&[Some(1), Some(1)]), Front::Hit { owner: 1, .. }));
    }

    #[test]
    fn reusable_scene_owns_its_exact_records_and_preserves_depth_order() {
        use std::sync::Arc;
        let edited = Arc::new(sheets(&[(2.0, square(-2.0, 2.0), None)]));
        let context = Arc::new(sheets(&[(1.0, square(-0.5, 0.5), None)]));
        let weak = Arc::downgrade(&context);
        let scene = Scene::prepare(edited, &[Some(7)], &[context], &mut 2_000_000).unwrap();
        assert!(weak.upgrade().is_some());
        let front = |x| scene.front([x, 0.25, 0.0], [0.0, 0.0, 1.0], [0.0, 10.0], &mut 2_000_000).unwrap();
        assert!(matches!(front(0.0), Front::Occluded { .. }));
        assert!(matches!(front(1.0), Front::Hit { owner: 7, .. }));
        drop(scene);
        assert!(weak.upgrade().is_none());
    }
}
