//! The record's structural checks and its serde form. Fixtures are built by hand: planar faces
//! with placeholder pcurves, which validation reads structurally (geometric checks, forms and
//! fitted pcurves are the kernel's, and tested there).
use super::*;
use std::f64::consts::TAU;

fn segment(a: f64, b: f64) -> Curve2 {
    Curve2::Spline {
        knots: vec![a, a, b, b],
        points: vec![[a, 0.], [b, 1.]],
    }
}
fn a_frame() -> Frame {
    Frame {
        origin: [1., 2., 3.],
        x_axis: [1., 0., 0.],
        y_axis: [0., 1., 0.],
    }
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
                    let at = EdgeUseRef {
                        face: fi as u32,
                        r#loop: li as u32,
                        position: p as u32,
                        upper: !u.forward,
                    };
                    b.edges[u.edge as usize].uses.push(at);
                }
            }
        }
    }
}

/// Planar faces, one per vertex cycle, grouped into shells; edges and vertices numbered in
/// canonical order as the cycles first meet them.
fn faceted(points: &[[f64; 3]], shells: &[&[&[usize]]], regions: Vec<Region>) -> Brep {
    let mut b = Brep {
        version: VERSION,
        tolerance: 1e-7,
        regions,
        shells: Vec::new(),
        faces: Vec::new(),
        edges: Vec::new(),
        vertices: Vec::new(),
    };
    let mut vertex_of = vec![None; points.len()];
    let mut vertex = |b: &mut Brep, p: usize| -> u32 {
        *vertex_of[p].get_or_insert_with(|| {
            b.vertices.push(Vertex {
                point: points[p],
                tolerance: 1e-7,
            });
            b.vertices.len() as u32 - 1
        })
    };
    let mut by_pair = std::collections::HashMap::new();
    for cycles in shells {
        let mut uses = Vec::new();
        for cycle in *cycles {
            let mut loop_ = Vec::new();
            for k in 0..cycle.len() {
                let (a, z) = (cycle[k], cycle[(k + 1) % cycle.len()]);
                let (edge, forward) = if let Some(&e) = by_pair.get(&(z, a)) {
                    (e, false)
                } else {
                    let vs = [vertex(&mut b, a), vertex(&mut b, z)];
                    b.edges.push(Edge {
                        nurbs: None,
                        curve: Curve::Line {
                            start: points[a],
                            end: points[z],
                        },
                        interval: [0., 1.],
                        vertices: vs,
                        tolerance: 1e-7,
                        uses: vec![],
                    });
                    let e = b.edges.len() as u32 - 1;
                    by_pair.insert((a, z), e);
                    (e, true)
                };
                loop_.push(OrientedEdge {
                    edge,
                    forward,
                    pcurve: segment(0., 1.),
                });
            }
            uses.push(OrientedFace {
                face: b.faces.len() as u32,
                forward: true,
            });
            // The plane through the cycle's first three points: its natural normal is the cycle's.
            let [o, p, q] = [0, 1, 2].map(|k| points[cycle[k]]);
            let axis = |a: [f64; 3]| [a[0] - o[0], a[1] - o[1], a[2] - o[2]];
            let surface = Surface::Plane {
                origin: o,
                u_axis: axis(p),
                v_axis: axis(q),
            };
            b.faces.push(Face {
                nurbs: None,
                surface,
                domain: [[-10., 10.], [-10., 10.]],
                loops: vec![Loop::Edges { edges: loop_ }],
            });
        }
        b.shells.push(Shell::Faces {
            faces: uses,
            wires: vec![],
        });
    }
    list_uses(&mut b);
    b
}

fn infinite(shells: Vec<u32>) -> Region {
    Region {
        shells,
        void: false,
    }
}
const SQUARE: [[f64; 3]; 4] = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
fn sheet() -> Brep {
    faceted(&SQUARE, &[&[&[0, 1, 2, 3]]], vec![infinite(vec![0])])
}
const TETRA: [[f64; 3]; 4] = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const TETRA_FACES: [&[usize]; 4] = [&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[0, 3, 2]];
fn tetra() -> Brep {
    faceted(
        &TETRA,
        &[&TETRA_FACES],
        vec![
            infinite(vec![0]),
            Region {
                shells: vec![0],
                void: false,
            },
        ],
    )
}
/// A tetrahedron with a tetrahedral void inside it.
fn hollow() -> Brep {
    let mut points = TETRA.to_vec();
    points.extend(TETRA.map(|p| p.map(|x| 0.2 + 0.1 * x)));
    let inner: Vec<Vec<usize>> = TETRA_FACES
        .iter()
        .map(|c| c.iter().rev().map(|i| i + 4).collect())
        .collect();
    let inner: Vec<&[usize]> = inner.iter().map(Vec::as_slice).collect();
    faceted(
        &points,
        &[&TETRA_FACES, &inner],
        vec![
            infinite(vec![0]),
            Region {
                shells: vec![0, 1],
                void: false,
            },
            Region {
                shells: vec![1],
                void: true,
            },
        ],
    )
}
/// The sheet with a vertex loop inside it, a wire shell and a vertex shell beside it.
fn sheet_with_lower_forms() -> Brep {
    let mut b = sheet();
    let n = b.vertices.len() as u32;
    // Vertex order: edge vertices first (the wire edge's), then the vertex loop's, then the shell's.
    b.vertices.extend(
        [[2., 0., 0.], [3., 0., 0.], [0.5, 0.5, 0.], [5., 5., 5.]].map(|point| Vertex {
            point,
            tolerance: 1e-7,
        }),
    );
    b.edges.push(Edge {
        nurbs: None,
        curve: Curve::Line {
            start: [2., 0., 0.],
            end: [3., 0., 0.],
        },
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
fn every_structure_validates_and_round_trips() {
    for (name, body) in [
        ("sheet", sheet()),
        ("closed", tetra()),
        ("void", hollow()),
        ("lower forms", sheet_with_lower_forms()),
    ] {
        body.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
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
        (
            "repeated use",
            "edge use listed twice or not a use of the edge",
            {
                let mut b = tetra();
                let first = b.edges[0].uses[0];
                b.edges[0].uses[1] = first;
                b
            },
        ),
        (
            "primary not first",
            "edge's primary use is not its first",
            {
                let mut b = tetra();
                b.edges[0].uses.reverse();
                b
            },
        ),
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
        (
            "edge interval outside a spline",
            "parameter interval outside the natural domain",
            {
                let mut b = sheet();
                b.edges[0].curve = Curve::Spline {
                    knots: vec![0., 0., 1., 1.],
                    points: vec![SQUARE[0], SQUARE[1]],
                };
                b.edges[0].interval = [0., 1.5];
                b
            },
        ),
        ("empty edge interval", "empty parameter interval", {
            let mut b = sheet();
            b.edges[0].interval = [1., 1.];
            b
        }),
        (
            "face domain outside a spline surface",
            "parameter interval outside the natural domain",
            {
                let mut b = sheet();
                b.faces[0].surface = Surface::Spline {
                    u_knots: vec![0., 0., 1., 1.],
                    v_knots: vec![0., 0., 1., 1.],
                    points: vec![
                        vec![[0.; 3], [0., 1., 0.]],
                        vec![[1., 0., 0.], [1., 1., 0.]],
                    ],
                };
                b
            },
        ),
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
        (
            "vertices out of order",
            "vertices out of canonical order",
            {
                let mut b = sheet();
                b.vertices.swap(0, 1);
                for e in &mut b.edges {
                    e.vertices = e.vertices.map(|v| [1, 0, 2, 3][v as usize]);
                }
                b
            },
        ),
        ("faces out of order", "faces out of canonical order", {
            let mut b = tetra();
            if let Shell::Faces { faces, .. } = &mut b.shells[0] {
                faces.swap(0, 1);
            }
            b
        }),
        ("orphan vertex", "vertices out of canonical order", {
            let mut b = sheet();
            b.vertices.push(Vertex {
                point: [9.; 3],
                tolerance: 1e-7,
            });
            b
        }),
        (
            "shell twice in a region",
            "shell listed twice in a region",
            {
                // The void lists its one shell twice: still within the shell count.
                let mut b = hollow();
                let void = b.regions.last_mut().unwrap();
                void.shells.push(void.shells[0]);
                b
            },
        ),
        ("void infinite region", "the infinite region is a void", {
            let mut b = sheet();
            b.regions[0].void = true;
            b
        }),
        (
            "open outer shell",
            "an open, wire or vertex shell lies in one region",
            {
                let mut b = sheet();
                b.regions.push(Region {
                    shells: vec![0],
                    void: false,
                });
                b
            },
        ),
        (
            "closed shell in one region",
            "a closed shell bounds two regions and encloses one",
            {
                let mut b = tetra();
                b.regions.pop();
                b
            },
        ),
        (
            "bounded region without shells",
            "bounded region without an outer shell",
            {
                let mut b = tetra();
                b.regions.push(Region {
                    shells: vec![],
                    void: true,
                });
                b
            },
        ),
        (
            "unlisted shell",
            "a closed shell bounds two regions and encloses one",
            {
                let mut b = hollow();
                b.regions[1].shells.pop();
                b.regions.pop();
                b
            },
        ),
        (
            "face used three times",
            "face used by neither one nor two oriented faces",
            {
                let mut b = tetra();
                if let Shell::Faces { faces, .. } = &mut b.shells[0] {
                    faces.push(OrientedFace {
                        face: 0,
                        forward: false,
                    });
                    faces.push(OrientedFace {
                        face: 0,
                        forward: true,
                    });
                }
                b
            },
        ),
        ("face twice in one shell", "a face's two sides", {
            let mut b = tetra();
            if let Shell::Faces { faces, .. } = &mut b.shells[0] {
                faces.push(OrientedFace {
                    face: 0,
                    forward: false,
                });
            }
            b
        }),
    ];
    for (name, expected, body) in cases {
        let error = body
            .validate()
            .err()
            .unwrap_or_else(|| panic!("{name} was accepted"));
        assert_eq!(error.0, expected, "{name}");
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
    let Loop::Edges { edges } = &mut b.faces[0].loops[0] else {
        panic!("an edge loop")
    };
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
    let vertices = seen
        .iter()
        .map(|&v| b.vertices[v as usize].clone())
        .collect();
    b.vertices = vertices;
    for e in &mut b.edges {
        e.vertices = e
            .vertices
            .map(|v| seen.iter().position(|&s| s == v).unwrap() as u32);
    }
    for e in &mut b.edges {
        e.uses.clear();
    }
    let Loop::Edges { edges } = &b.faces[0].loops[0] else {
        panic!("an edge loop")
    };
    for (p, u) in edges.iter().enumerate().rev() {
        b.edges[u.edge as usize].uses.push(EdgeUseRef {
            face: 0,
            r#loop: 0,
            position: p as u32,
            upper: !u.forward,
        });
    }
    b.validate().unwrap();
}

/// A valid record of more than [`MAX_ITEMS`] items: the sheet with its first edge a polyline of
/// that many points (#3992).
fn over_the_item_budget() -> Brep {
    let mut b = sheet();
    b.edges[0].curve = Curve::Polyline {
        points: (0..MAX_ITEMS).map(|i| [i as f64 * 1e-6, 0., 0.]).collect(),
    };
    b
}

#[test]
fn the_item_budget_bounds_input_records_and_not_the_kernels_own() {
    // A record from a file or the control channel is refused past the budget; the same record
    // built by the kernel in memory (a boolean's result) is its own size and is taken (#3992).
    let big = over_the_item_budget();
    assert_eq!(big.validate().map_err(|e| e.0), Err("item budget"));
    big.validate_built().unwrap();
    // Every other check still holds for a built record.
    let mut collapsed = big;
    if let Curve::Polyline { points } = &mut collapsed.edges[0].curve {
        points[1] = points[0];
    }
    assert_eq!(
        collapsed.validate_built().map_err(|e| e.0),
        Err("collapsed polyline segment")
    );
}

/// File-sized cap (#812): `file_bytes / ITEM_BYTES`, between [`MAX_ITEMS`] and
/// [`ITEM_LIMIT_CEILING`], the same on every machine.
#[test]
fn item_limit_is_the_floor_raised_by_the_file_up_to_the_ceiling() {
    assert_eq!(ITEM_BYTES, 16);
    assert_eq!(ITEM_LIMIT_CEILING, 16_000_000);
    assert_eq!(item_limit(0), MAX_ITEMS);
    assert_eq!(item_limit(1), MAX_ITEMS);
    let bytes = (MAX_ITEMS as u64) * ITEM_BYTES * 2;
    assert_eq!(item_limit(bytes), MAX_ITEMS * 2);
    assert_eq!(item_limit(u64::MAX), ITEM_LIMIT_CEILING);
    // The cap never drops under today's, whatever the file.
    for file_bytes in [0, 1, 1 << 20, 1 << 30, u64::MAX] {
        assert!(item_limit(file_bytes) >= MAX_ITEMS);
    }
}

#[test]
fn validate_file_names_the_count_when_the_record_passes_the_cap() {
    let Err(message) = over_the_item_budget().validate_file(1) else {
        panic!("a record past the floor was accepted")
    };
    assert!(
        message.starts_with("portable B-rep: the file is valid but too large: "),
        "{message}"
    );
    assert!(message.contains("cap 4000000"), "{message}");
    let cap = item_limit(1);
    assert_eq!(cap, MAX_ITEMS);
    let counted_text = message
        .split(" items")
        .next()
        .and_then(|head| head.rsplit(' ').next())
        .unwrap_or("");
    let counted: usize = counted_text.parse().unwrap_or(0);
    assert!(counted > MAX_ITEMS, "{message}");
    assert_eq!(
        message,
        format!("portable B-rep: the file is valid but too large: {counted} items, cap {cap}"),
        "{message}"
    );
}

#[test]
fn validate_file_accepts_the_record_when_the_file_pays_for_it() {
    // Valid apart from the fixed budget: `validate_built` already accepts this record.
    let bytes = (MAX_ITEMS as u64) * ITEM_BYTES * 2;
    let accepted = over_the_item_budget().validate_file(bytes);
    assert!(
        accepted.is_ok(),
        "a record inside the file-sized cap was refused: {accepted:?}"
    );
}

#[test]
fn validate_file_reports_a_collapsed_segment_ahead_of_the_size() {
    let mut collapsed = over_the_item_budget();
    if let Curve::Polyline { points } = &mut collapsed.edges[0].curve {
        points[1] = points[0];
    }
    let bytes = (MAX_ITEMS as u64) * ITEM_BYTES * 2;
    let Err(message) = collapsed.validate_file(bytes) else {
        panic!("a collapsed polyline was accepted")
    };
    assert!(message.contains("collapsed polyline segment"), "{message}");
    assert!(!message.contains("too large"), "{message}");
    assert_eq!(
        message, "portable B-rep: collapsed polyline segment",
        "{message}"
    );
}

#[test]
fn spline_domains_weights_and_nesting_are_bounded() {
    let mut budget = Budget::input();
    assert!(knots(&[0., 0., 1., 1.], 2, &mut budget).is_ok());
    for bad in [
        vec![0., 0., 0., 0.],
        vec![0., 1., 0., 1.],
        vec![0., 0., f64::INFINITY, 1.],
    ] {
        assert!(knots(&bad, 2, &mut Budget::input()).is_err());
    }
    let c = Curve::RationalSpline {
        knots: vec![0., 0., 1., 1.],
        homogeneous: vec![[0., 0., 0., 0.], [1., 0., 0., 1.]],
    };
    assert!(curve(&c, 0, &mut Budget::input()).is_err());
    assert!(
        curve(
            &Curve::Line {
                start: [0.; 3],
                end: [1.; 3]
            },
            MAX_DEPTH + 1,
            &mut Budget::input()
        )
        .is_err()
    );
    assert!(
        Budget {
            used: MAX_ITEMS,
            limit: MAX_ITEMS,
            overflowed: None,
        }
        .add(1)
        .is_err()
    );
    assert_eq!(
        curve_natural(&Curve::Spline {
            knots: vec![0., 0., 0., 1., 2., 2., 2.],
            points: vec![[0.; 3]; 4]
        }),
        Natural::Range(0., 2.)
    );
}

#[test]
fn exact_procedural_records_are_validated_and_round_trip() {
    let plane = |z: f64| {
        Box::new(Surface::Plane {
            origin: [0., 0., z],
            u_axis: [1., 0., 0.],
            v_axis: [0., 1., 0.],
        })
    };
    let leader = Box::new(Curve::Polyline {
        points: vec![[0.; 3], [0.; 3], [1., 0., 0.]],
    });
    let c = Curve::SurfaceIntersection {
        surfaces: [plane(0.), plane(1.)],
        leader: leader.clone(),
        tolerance: 1e-6,
    };
    curve(&c, 0, &mut Budget::input()).unwrap();
    assert_eq!(curve_natural(&c), Natural::Range(0., 2.));
    assert_eq!(
        serde_json::from_str::<Curve>(&serde_json::to_string(&c).unwrap()).unwrap(),
        c
    );
    let line = Box::new(Curve::Line {
        start: [0.; 3],
        end: [1.; 3],
    });
    for bad in [
        Curve::SurfaceIntersection {
            surfaces: [plane(0.), plane(1.)],
            leader: line.clone(),
            tolerance: 1e-6,
        },
        Curve::SurfaceIntersection {
            surfaces: [plane(0.), plane(1.)],
            leader: leader.clone(),
            tolerance: f64::NAN,
        },
        Curve::SurfaceIntersection {
            surfaces: [plane(f64::INFINITY), plane(1.)],
            leader,
            tolerance: 1e-6,
        },
    ] {
        assert!(curve(&bad, 0, &mut Budget::input()).is_err());
    }
    let identity = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let s = Surface::PlacedRevolution {
        profile: line.clone(),
        origin: [0.; 3],
        axis: [0., 0., 1.],
        matrix: identity,
        forward: false,
    };
    surface(&s, 0, &mut Budget::input()).unwrap();
    assert_eq!(
        serde_json::from_str::<Surface>(&serde_json::to_string(&s).unwrap()).unwrap(),
        s
    );
    let bad = Surface::PlacedRevolution {
        profile: line,
        origin: [0.; 3],
        axis: [0., 0., 1.],
        matrix: [[f64::NAN; 4]; 4],
        forward: true,
    };
    assert!(surface(&bad, 0, &mut Budget::input()).is_err());
}

#[test]
fn analytic_records_are_unbounded_with_independent_frames() {
    let f = a_frame();
    let surfaces = [
        Surface::Cylinder {
            frame: f.clone(),
            radius: 2.,
        },
        Surface::Cone {
            frame: f.clone(),
            radius_at_origin: 2.,
            half_angle: 0.2,
        },
        Surface::Cone {
            frame: f.clone(),
            radius_at_origin: 2.,
            half_angle: -0.2,
        },
        Surface::Sphere {
            frame: f.clone(),
            radius: 2.,
        },
        Surface::Torus {
            frame: f.clone(),
            major_radius: 4.,
            minor_radius: 1.,
            tube_start: 0.0,
        },
    ];
    for flat in [
        0.,
        1e-13,
        -1e-13,
        std::f64::consts::FRAC_PI_2,
        -std::f64::consts::FRAC_PI_2,
    ] {
        assert!(
            surface(
                &Surface::Cone {
                    frame: f.clone(),
                    radius_at_origin: 2.,
                    half_angle: flat
                },
                0,
                &mut Budget::input()
            )
            .is_err()
        );
    }
    for s in surfaces {
        surface(&s, 0, &mut Budget::input()).unwrap();
        let encoded = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Surface>(&encoded).unwrap(), s);
        assert!(
            !encoded.contains("\"angle\"")
                && !encoded.contains("height")
                && !encoded.contains("longitude")
        );
    }
    for c in [
        Curve::Circle {
            frame: f.clone(),
            radius: 2.,
        },
        Curve::Ellipse {
            frame: f.clone(),
            radii: [2., 3.],
        },
    ] {
        curve(&c, 0, &mut Budget::input()).unwrap();
        assert_eq!(
            serde_json::from_str::<Curve>(&serde_json::to_string(&c).unwrap()).unwrap(),
            c
        );
        assert_eq!(curve_natural(&c), Natural::Periodic);
        // A periodic edge may start anywhere and run up to one turn.
        interval(&[-1., TAU - 1.], Natural::Periodic).unwrap();
    }
    let mut bad = f;
    bad.y_axis[1] = f64::NAN;
    assert!(frame(&bad, &mut Budget::input()).is_err());
    assert!(interval(&[2., 1.], UNBOUNDED).is_err());
    assert!(radius(f64::NAN).is_err());
    assert!(
        surface(
            &Surface::Torus {
                frame: a_frame(),
                major_radius: 1.,
                minor_radius: 2.,
                tube_start: 0.0
            },
            0,
            &mut Budget::input()
        )
        .is_err()
    );
}

/// Owner ruling (item 8, option B): a face that carries its NURBS form may name it a cone of
/// half angle under 10⁻¹² or a torus whose tube reaches its axis (the form is the face's
/// geometry, the label a proved name). The same labels with no form, and as bare surfaces,
/// are refused.
#[test]
fn degenerate_cone_and_torus_labels_are_valid_only_with_their_form() {
    let form = Surface::Spline {
        u_knots: vec![-10., -10., 10., 10.],
        v_knots: vec![-10., -10., 10., 10.],
        points: vec![
            vec![[-10., -10., 0.], [-10., 10., 0.]],
            vec![[10., -10., 0.], [10., 10., 0.]],
        ],
    };
    let labels = [
        Surface::Cone {
            frame: a_frame(),
            radius_at_origin: 2.,
            half_angle: 1e-13,
        },
        Surface::Cone {
            frame: a_frame(),
            radius_at_origin: 2.,
            half_angle: 0.,
        },
        Surface::Torus {
            frame: a_frame(),
            major_radius: 1.,
            minor_radius: 2.,
            tube_start: 0.0,
        },
        Surface::Torus {
            frame: a_frame(),
            major_radius: 2.,
            minor_radius: 2.,
            tube_start: 0.0,
        },
    ];
    for label in labels {
        let mut b = sheet();
        b.faces[0].surface = label.clone();
        b.faces[0].nurbs = Some(form.clone());
        b.validate()
            .unwrap_or_else(|e| panic!("{label:?} with its form: {e:?}"));
        let mut bare = b.clone();
        bare.faces[0].nurbs = None;
        assert!(bare.validate().is_err(), "{label:?} without its form");
        assert!(
            surface(&label, 0, &mut Budget::input()).is_err(),
            "{label:?} as a bare surface"
        );
    }
}

/// A small, structurally valid procedural blend definition (its numbers mean nothing: the
/// record checks structure only).
fn a_blend(depth: u32) -> blend::BlendDefinitionRecord {
    use blend::*;
    let form = NurbsSurfaceRecord {
        u_degree: 1,
        v_degree: 1,
        u_count: 2,
        v_count: 2,
        u_knots: vec![0., 0., 1., 1.],
        v_knots: vec![0., 0., 1., 1.],
        points: vec![[0., 0., 0.], [0., 1., 0.], [1., 0., 0.], [1., 1., 0.]],
        weights: None,
    };
    let curve = |dim: u32| NurbsCurveRecord {
        dim,
        points_3d: dim == 3,
        degree: 1,
        knots: vec![0., 0., 1., 1.],
        points: vec![[0., 0., 0.], [1., 0., 0.]],
        weights: None,
    };
    let support = if depth > 1 {
        SupportRecord::Blend(Box::new(ProceduralBlendRecord {
            definition: a_blend(depth - 1),
            net: form.clone(),
        }))
    } else {
        SupportRecord::Nurbs {
            kind: SupportKindRecord::Plane,
            form: form.clone(),
            revolution: None,
        }
    };
    BlendDefinitionRecord {
        supports: [
            support,
            SupportRecord::Nurbs {
                kind: SupportKindRecord::Plane,
                form,
                revolution: None,
            },
        ],
        sides: [1., 1.],
        radius: RadiusRecord::Constant { offsets: [1., 1.] },
        order: 1,
        thumbweight: 0.,
        leader: curve(3),
        seeds: [curve(2), curve(2)],
        bound: 1e-6,
        map: ParameterMapRecord {
            a: [0., 0.],
            b: [1., 1.],
            swapped: false,
        },
        chamfer: None,
    }
}

fn blend_label(depth: u32) -> Surface {
    Surface::Blend {
        definition: Box::new(blend::BlendDefinition::Procedural(Box::new(a_blend(depth)))),
    }
}

/// A blend face is its definition with its net as its NURBS form: it validates and round-trips
/// number for number; without its net, or nested past three definitions, it is refused.
#[test]
fn a_blend_face_is_its_definition_on_its_net() {
    let form = Surface::Spline {
        u_knots: vec![-10., -10., 10., 10.],
        v_knots: vec![-10., -10., 10., 10.],
        points: vec![
            vec![[-10., -10., 0.], [-10., 10., 0.]],
            vec![[10., -10., 0.], [10., 10., 0.]],
        ],
    };
    let mut b = sheet();
    b.faces[0].surface = blend_label(3);
    b.faces[0].nurbs = Some(form);
    b.validate().unwrap_or_else(|e| panic!("{e}"));
    let text = serde_json::to_string(&b).unwrap();
    assert!(text.contains(r#""kind":"blend""#), "{text}");
    let back: Brep = serde_json::from_str(&text).unwrap();
    assert_eq!(back, b);
    let mut bare = b.clone();
    bare.faces[0].nurbs = None;
    assert!(bare.validate().is_err(), "a blend without its net");
    let mut deep = b.clone();
    deep.faces[0].surface = blend_label(4);
    assert!(deep.validate().is_err(), "nested past three definitions");
    let mut damaged = b;
    if let Surface::Blend { definition } = &mut damaged.faces[0].surface
        && let blend::BlendDefinition::Procedural(d) = definition.as_mut()
    {
        d.bound = f64::NAN;
    }
    assert!(
        damaged.validate().is_err(),
        "a definition's numbers are finite"
    );
}

/// A chamfer of setbacks held at every station: its kind is written only where it is not
/// offsets (so every older record reads and writes as before), it has the edge for its
/// radius, a wedge of 0, and no distance and angle yet.
#[test]
fn a_chamfer_of_setbacks_is_its_kind_and_the_edge() {
    use blend::{
        BlendDefinitionRecord, ChamferKindRecord, ChamferRecord, ChamferSizeRecord, RadiusRecord,
    };
    let offsets = ChamferRecord {
        size: ChamferSizeRecord::Distances([1., 2.]),
        wedge: 1.5,
        kind: ChamferKindRecord::Offsets,
    };
    let old = r#"{"size":{"Distances":[1.0,2.0]},"wedge":1.5}"#;
    assert_eq!(serde_json::to_string(&offsets).unwrap(), old);
    assert_eq!(serde_json::from_str::<ChamferRecord>(old).unwrap(), offsets);
    let mut d = a_blend(1);
    d.order = 0;
    d.radius = RadiusRecord::Edge;
    d.chamfer = Some(ChamferRecord {
        size: ChamferSizeRecord::Chord(1.5),
        wedge: 0.,
        kind: ChamferKindRecord::Setbacks,
    });
    d.validate().unwrap_or_else(|e| panic!("{e}"));
    let text = serde_json::to_string(&d).unwrap();
    assert!(text.contains(r#""radius":"Edge""#) && text.contains(r#""kind":"Setbacks""#));
    assert_eq!(
        serde_json::from_str::<BlendDefinitionRecord>(&text).unwrap(),
        d
    );
    let mut wedged = d.clone();
    if let Some(c) = wedged.chamfer.as_mut() {
        c.wedge = 1.5;
    }
    assert!(wedged.validate().is_err(), "a setbacks wedge");
    let mut angled = d.clone();
    if let Some(c) = angled.chamfer.as_mut() {
        c.size = ChamferSizeRecord::DistanceAngle {
            distance: 1.,
            angle: 0.5,
        };
    }
    assert!(
        angled.validate().is_err(),
        "setbacks by a distance and an angle"
    );
    let mut balled = d.clone();
    balled.radius = RadiusRecord::Constant { offsets: [1., 1.] };
    assert!(balled.validate().is_err(), "setbacks with a ball");
    let mut bare = a_blend(1);
    bare.radius = RadiusRecord::Edge;
    assert!(
        bare.validate().is_err(),
        "the edge without a chamfer of setbacks"
    );
}

/// A reader from before the blend label refuses a record that has one, by name, rather than
/// misreading it: its surface kinds are a closed set, and an unknown kind is an error.
#[test]
fn an_older_reader_refuses_a_blend_face_by_name() {
    #[derive(Debug, serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    #[allow(dead_code)]
    enum OlderSurface {
        Plane {
            origin: [f64; 3],
            u_axis: [f64; 3],
            v_axis: [f64; 3],
        },
        BallPatch {
            center: [f64; 3],
            radius: f64,
        },
    }
    let label = serde_json::to_value(blend_label(1)).unwrap();
    let error = serde_json::from_value::<OlderSurface>(label)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown variant `blend`"), "{error}");
}

#[test]
fn validate_file_reports_a_collapsed_segment_past_the_cap() {
    // The walk stops at the item cap before it reaches the collapsed segment; the record is
    // not valid, so the error must not say that it is.
    let mut collapsed = over_the_item_budget();
    if let Curve::Polyline { points } = &mut collapsed.edges[0].curve {
        points[1] = points[0];
    }
    let Err(message) = collapsed.validate_file(1) else {
        panic!("a collapsed polyline was accepted")
    };
    assert_eq!(message, "portable B-rep: collapsed polyline segment");
}

/// A screw-swept wall's definition: a straight profile across the axis, one turn in eight spans,
/// with a run-out law.
fn a_sweep() -> sweep::SweepDefinition {
    let quarter = TAU / 16.0;
    sweep::SweepDefinition {
        profile: Curve::Spline {
            knots: vec![0., 0., 1., 1.],
            points: vec![[5., 0., 0.], [6., 0., 0.5]],
        },
        motion: sweep::SweepMotion::Screw {
            origin: [0.; 3],
            axis: [0., 0., 1.],
            pitch: 1.5,
            law: Some(sweep::RadialLaw {
                direction: [1., 0., 0.],
                angles: vec![0., TAU],
                offsets: vec![0., -0.5],
            }),
        },
        stations: (0..=8).map(|i| f64::from(i) * 2.0 * quarter).collect(),
        half_angles: vec![quarter; 8],
        map: sweep::SweepMap::default(),
        bound: 1e-7,
    }
}

/// A sweep face is its definition with its net as its NURBS form: it validates and round-trips
/// number for number, writes as kind `sweep`, and is refused without its net and with each
/// structural fault.
#[test]
fn a_sweep_face_is_its_definition_on_its_net() {
    type Fault = fn(&mut sweep::SweepDefinition);
    let form = Surface::Spline {
        u_knots: vec![-10., -10., 10., 10.],
        v_knots: vec![-10., -10., 10., 10.],
        points: vec![
            vec![[-10., -10., 0.], [-10., 10., 0.]],
            vec![[10., -10., 0.], [10., 10., 0.]],
        ],
    };
    let mut b = sheet();
    b.faces[0].surface = Surface::Sweep {
        definition: Box::new(a_sweep()),
    };
    b.faces[0].nurbs = Some(form);
    b.validate().unwrap_or_else(|e| panic!("{e}"));
    let text = serde_json::to_string(&b).unwrap();
    assert!(text.contains(r#""kind":"sweep""#), "{text}");
    assert!(text.contains(r#""kind":"screw""#), "{text}");
    let back: Brep = serde_json::from_str(&text).unwrap();
    assert_eq!(back, b);
    let mut bare = b.clone();
    bare.faces[0].nurbs = None;
    assert!(bare.validate().is_err(), "a sweep without its net");
    let faults: [(&str, Fault); 10] = [
        ("a negative bound", |d| d.bound = -1.0),
        ("a line profile", |d| {
            d.profile = Curve::Line {
                start: [0.; 3],
                end: [1.; 3],
            }
        }),
        ("one station", |d| {
            d.stations.truncate(1);
            d.half_angles.clear();
        }),
        ("stations out of order", |d| d.stations.swap(1, 2)),
        ("a half angle missing", |d| {
            d.half_angles.pop();
        }),
        ("a span past an eighth of a turn", |d| {
            d.half_angles[0] = 1.0;
        }),
        ("a zero pitch", |d| {
            let sweep::SweepMotion::Screw { pitch, .. } = &mut d.motion;
            *pitch = 0.0;
        }),
        ("an axis not unit", |d| {
            let sweep::SweepMotion::Screw { axis, .. } = &mut d.motion;
            *axis = [0., 0., 2.];
        }),
        ("a law along the axis", |d| {
            let sweep::SweepMotion::Screw { law, .. } = &mut d.motion;
            if let Some(l) = law {
                l.direction = [0., 0., 1.];
            }
        }),
        ("a map of no scale", |d| d.map.b[1] = 0.0),
    ];
    for (what, fault) in faults {
        let mut bad = b.clone();
        if let Surface::Sweep { definition } = &mut bad.faces[0].surface {
            fault(definition);
        }
        assert!(bad.validate().is_err(), "{what}");
    }
    let mut nan = b;
    if let Surface::Sweep { definition } = &mut nan.faces[0].surface {
        definition.stations[3] = f64::NAN;
    }
    assert!(nan.validate().is_err(), "a station not finite");
}

/// A reader from before the sweep kind refuses a record that has one, by name: a design's cached
/// body that holds one is a cache miss there, and the design is evaluated again from its timeline.
#[test]
fn an_older_reader_refuses_a_sweep_face_by_name() {
    #[derive(Debug, serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    #[allow(dead_code)]
    enum OlderSurface {
        BallPatch { center: [f64; 3], radius: f64 },
    }
    let label = serde_json::to_value(Surface::Sweep {
        definition: Box::new(a_sweep()),
    })
    .unwrap();
    let error = serde_json::from_value::<OlderSurface>(label)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown variant `sweep`"), "{error}");
}
