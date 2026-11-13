//! The record's B-rep and the kernel's, both ways ([`solidcore_convert::brep`]). These are the
//! tests of the conversion that SolidCraft's flat-array hop had (crates/smlib `import/tests.rs`,
//! `export/tests.rs`, `core/convert_tests.rs`), on the direct conversion.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::type_complexity,
    clippy::single_match_else,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_closure,
    clippy::panic
)] // a test's fixtures
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_primitives as primitives;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Vec3};
use solidcore_convert::brep::{to_kernel, to_record, Order, Unsupported};
use solidcore_record::brep::{
    Brep as Record, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace,
    Region, Shell, Surface, Vertex, VERSION,
};

fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

fn valid(k: &mut Kernel, b: &mut Brep) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert_eq!(c.pointers_status, 1000, "pointers");
    assert_eq!(
        c.assert_valid,
        Some(true),
        "{:?}",
        c.reports
            .reports()
            .iter()
            .filter(|r| !r.ok)
            .map(|r| r.format_log_message())
            .collect::<Vec<_>>()
    );
}

/// The cube `[0, s]^3`, written as the record says a body is (canonical order, every face a plane
/// on unit axes, loops counter-clockwise seen from the plane's natural normal, which points out).
fn cube(s: f64) -> Record {
    let c = [
        [0., 0., 0.],
        [s, 0., 0.],
        [s, s, 0.],
        [0., s, 0.],
        [0., 0., s],
        [s, 0., s],
        [s, s, s],
        [0., s, s],
    ];
    // Each face: its corner cycle (counter-clockwise from outside), its plane's origin and axes.
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    let neg = |a: [f64; 3]| [-a[0], -a[1], -a[2]];
    let faces: [([usize; 4], [f64; 3], [f64; 3], [f64; 3]); 6] = [
        ([0, 3, 2, 1], c[0], y, x),      // bottom, normal -z
        ([4, 5, 6, 7], c[4], x, y),      // top, +z
        ([0, 1, 5, 4], c[0], x, z),      // front, -y
        ([1, 2, 6, 5], c[1], y, z),      // right, +x
        ([2, 3, 7, 6], c[2], neg(x), z), // back, +y
        ([3, 0, 4, 7], c[3], neg(y), z), // left, -x
    ];
    let mut ends: Vec<(usize, usize)> = Vec::new();
    let mut uses: Vec<Vec<EdgeUseRef>> = Vec::new();
    let mut out = Vec::new();
    for (fi, (cycle, o, u, v)) in faces.iter().enumerate() {
        let uv = |p: [f64; 3]| {
            let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
            [
                d[0] * u[0] + d[1] * u[1] + d[2] * u[2],
                d[0] * v[0] + d[1] * v[1] + d[2] * v[2],
            ]
        };
        let mut edges = Vec::new();
        for k in 0..4 {
            let (a, b) = (cycle[k], cycle[(k + 1) % 4]);
            let (e, forward) = match ends.iter().position(|&(s, t)| (s, t) == (b, a)) {
                Some(e) => (e, false),
                None => {
                    ends.push((a, b));
                    uses.push(Vec::new());
                    (ends.len() - 1, true)
                }
            };
            uses[e].push(EdgeUseRef {
                face: fi as u32,
                r#loop: 0,
                position: k as u32,
                upper: !forward,
            });
            let (s0, t0) = ends[e];
            edges.push(OrientedEdge {
                edge: e as u32,
                forward,
                pcurve: Curve2::Spline {
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    points: vec![uv(c[s0]), uv(c[t0])],
                },
            });
        }
        out.push(Face {
            nurbs: None,
            surface: Surface::Plane {
                origin: *o,
                u_axis: *u,
                v_axis: *v,
            },
            domain: [[0.0, s], [0.0, s]],
            loops: vec![Loop::Edges { edges }],
        });
    }
    let mut vertex_of: Vec<Option<u32>> = vec![None; 8];
    let mut vertices = Vec::new();
    let mut number = |i: usize, vertices: &mut Vec<Vertex>| {
        *vertex_of[i].get_or_insert_with(|| {
            vertices.push(Vertex {
                point: c[i],
                tolerance: 1e-6,
            });
            vertices.len() as u32 - 1
        })
    };
    let edges = ends
        .iter()
        .zip(uses)
        .map(|(&(s0, t0), uses)| {
            let vs = [number(s0, &mut vertices), number(t0, &mut vertices)];
            Edge {
                nurbs: None,
                curve: Curve::Line {
                    start: c[s0],
                    end: c[t0],
                },
                interval: [0.0, 1.0],
                vertices: vs,
                tolerance: 1e-6,
                uses,
            }
        })
        .collect();
    Record {
        version: VERSION,
        screw_owners: Vec::new(),
        tolerance: 1e-6,
        regions: vec![
            Region {
                shells: vec![0],
                void: false,
            },
            Region {
                shells: vec![0],
                void: false,
            },
        ],
        shells: vec![Shell::Faces {
            faces: (0..6)
                .map(|face| OrientedFace {
                    face,
                    forward: true,
                })
                .collect(),
            wires: vec![],
        }],
        faces: out,
        edges,
        vertices,
    }
}

/// Every record index names a kernel entity, and each kernel face and vertex once.
fn order_is_total(o: &Order, r: &Record, b: &Brep) {
    assert_eq!(o.faces.len(), r.faces.len());
    assert_eq!(o.edges.len(), r.edges.len());
    assert_eq!(o.vertices.len(), r.vertices.len());
    let mut faces = o.faces.clone();
    faces.sort_unstable_by_key(|f| f.index());
    faces.dedup();
    assert_eq!(faces.len(), b.face_list().len());
    for e in &o.edges {
        assert!(b.edge_list().contains(e));
    }
}

#[test]
fn a_cube_record_builds_the_kernels_cube() {
    let mut k = kernel();
    let r = cube(10.0);
    r.validate().unwrap();
    let (mut b, order) = to_kernel(&mut k, &r, true).unwrap();
    assert_eq!(
        [
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ],
        [6, 12, 8]
    );
    assert!(is_manifold_solid(&b).unwrap());
    assert!((volume(&k, &mut b) - 1000.0).abs() < 1e-6);
    valid(&mut k, &mut b);
    order_is_total(&order, &r, &b);
    // Record vertex i is the kernel vertex at its point.
    for (i, v) in order.vertices.iter().enumerate() {
        let p = b.vertex(*v).unwrap().point;
        assert_eq!([p.x, p.y, p.z], r.vertices[i].point, "vertex {i}");
    }
}

#[test]
fn a_pcurve_support_may_extend_past_its_edge_interval() {
    let mut record = cube(1.0);
    let Curve::Line { start, end } = &record.edges[0].curve else {
        panic!("cube line")
    };
    let (start, end) = (*start, *end);
    record.edges[0].curve = Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![start, end],
    };
    record.edges[0].interval = [0.0, f64::from_bits(1.0_f64.to_bits() - 2)];
    record.validate().unwrap();
    let original = record.faces.clone();
    let mut k = kernel();
    let (mut body, _) = to_kernel(&mut k, &record, true).unwrap();
    let (saved, _) = to_record(&mut k, &mut body).unwrap();
    assert_eq!(
        saved.faces.iter().map(|f| &f.loops).collect::<Vec<_>>(),
        original.iter().map(|f| &f.loops).collect::<Vec<_>>()
    );
}

#[test]
fn records_the_conversion_cannot_carry_are_refused() {
    let mut k = kernel();
    let mut r = cube(1.0);
    r.version = 3;
    assert!(matches!(
        to_kernel(&mut k, &r, true),
        Err(Unsupported::Invalid(_))
    ));
    let mut r = cube(1.0);
    r.shells = vec![Shell::Vertex { vertex: 0 }];
    assert!(to_kernel(&mut k, &r, true).is_err());
    let mut r = cube(1.0);
    r.edges[0].tolerance = 2.0;
    assert!(matches!(
        to_kernel(&mut k, &r, true),
        Err(Unsupported::NotYet(_))
    ));
}

#[test]
fn exact_curve_owner_records_cannot_bypass_the_input_budget() {
    use solidcore_record::brep::sweep::{
        NurbsCurveRecord, NurbsSurfaceRecord, ProceduralSweepRecord, ScrewMotionRecord,
        SweepDefinitionRecord,
    };
    fn knots(n: usize) -> Vec<f64> {
        let mut k = Vec::with_capacity(n + 2);
        k.push(0.0);
        k.extend((0..n).map(|i| i as f64));
        k.push((n - 1) as f64);
        k
    }
    let (u, v) = (2000_usize, 2001_usize);
    let profile = NurbsCurveRecord {
        dim: 3,
        points_3d: true,
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[2.0, 0.0, 0.0], [3.0, 0.0, 1.0]],
        weights: None,
    };
    let owner = ProceduralSweepRecord {
        definition: SweepDefinitionRecord::Screw {
            profile,
            motion: ScrewMotionRecord {
                origin: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                pitch: 1.0,
                law: None,
            },
            offset: None,
        },
        net: NurbsSurfaceRecord {
            u_degree: 1,
            v_degree: 1,
            u_count: u as u32,
            v_count: v as u32,
            u_knots: knots(u),
            v_knots: knots(v),
            points: vec![[2.0, 0.0, 0.0]; u * v],
            weights: None,
        },
        stations: vec![0.0, 1.0],
        half: 0.5,
        bound: 0.1,
        map_a: [0.0, 0.0],
        map_b: [1.0, 1.0],
        swapped: false,
    };
    let mut record = cube(1.0);
    record.edges[0].curve = Curve::OnSurface {
        surface: Box::new(Surface::ProceduralSweep(Box::new(owner))),
        uv_curve: Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        },
        domain: [[0.0, 1.0], [0.0, 1.0]],
    };
    let mut k = kernel();
    assert!(matches!(
        to_kernel(&mut k, &record, true),
        Err(Unsupported::Invalid("item budget"))
    ));
    let Curve::OnSurface {
        surface,
        uv_curve,
        domain,
    } = std::mem::replace(
        &mut record.edges[0].curve,
        Curve::Line {
            start: [0.0; 3],
            end: [1.0, 0.0, 0.0],
        },
    )
    else {
        panic!("inline owner")
    };
    let Surface::ProceduralSweep(owner) = *surface else {
        panic!("screw owner")
    };
    record.screw_owners.push(*owner);
    record.edges[0].curve = Curve::OnSurfaceRef {
        owner: 0,
        uv_curve,
        domain,
    };
    assert!(matches!(
        to_kernel(&mut k, &record, true),
        Err(Unsupported::Invalid("item budget"))
    ));
}

/// Each primitive goes to the record with its analytic kinds, validates, and comes back to the
/// same body: the same counts and volume, valid, and the same record again.
#[test]
fn every_primitive_goes_round_unchanged() {
    let o = Vec3::new(1.0, 2.0, 3.0);
    let makers: [(&str, fn(&mut Kernel, Vec3) -> Brep, &[&str]); 5] = [
        (
            "box",
            |k, o| primitives::create_box(k, o, 2.0, 3.0, 4.0).unwrap(),
            &["plane"],
        ),
        (
            "cylinder",
            |k, o| primitives::create_cylinder(k, o, 15.0, 50.0).unwrap(),
            &["plane", "cylinder"],
        ),
        (
            "sphere",
            |k, o| primitives::create_sphere(k, o, 12.0).unwrap(),
            &["sphere"],
        ),
        (
            "frustum",
            |k, o| primitives::create_cone(k, o, 10.0, 4.0, 20.0).unwrap(),
            &["plane", "cone"],
        ),
        (
            "torus",
            |k, o| primitives::create_torus(k, o, 22.0, 4.0).unwrap(),
            &["torus"],
        ),
    ];
    for (name, make, kinds) in makers {
        let mut k = kernel();
        let mut b = make(&mut k, o);
        let v0 = volume(&k, &mut b);
        let (r, order) = to_record(&mut k, &mut b).unwrap();
        r.validate_built().unwrap_or_else(|e| panic!("{name}: {e}"));
        order_is_total(&order, &r, &b);
        for f in &r.faces {
            let kind = match f.surface {
                Surface::Plane { .. } => "plane",
                Surface::Cylinder { .. } => "cylinder",
                Surface::Cone { .. } => "cone",
                Surface::Sphere { .. } => "sphere",
                Surface::Torus { .. } => "torus",
                _ => "other",
            };
            assert!(kinds.contains(&kind), "{name}: a {kind} face");
        }
        let (mut back, _) = to_kernel(&mut k, &r, true).unwrap();
        assert_eq!(back.face_list().len(), b.face_list().len(), "{name}");
        assert_eq!(back.edge_list().len(), b.edge_list().len(), "{name}");
        let v1 = volume(&k, &mut back);
        assert!((v1 - v0).abs() <= 1e-9 * v0, "{name}: {v0} -> {v1}");
        valid(&mut k, &mut back);
        let (again, _) = to_record(&mut k, &mut back).unwrap();
        assert_eq!(again, r, "{name}: the record changed on a round trip");
    }
}

/// A revolution of a line parallel to its axis is that cylinder exactly, and goes as one. A
/// spline profile a hair off a line (solidcore#2540's sliver) is no cylinder: it goes as the
/// revolution's NURBS, never as a cylinder the check then refuses.
#[test]
fn only_a_revolution_proved_on_its_cylinder_goes_as_one() {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let line = Curve::Line {
        start: [5.0, 0.0, 0.0],
        end: [5.0, 0.0, 4.0],
    };
    let exact = Surface::PlacedRevolution {
        profile: Box::new(line),
        origin: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        matrix: identity,
        forward: true,
    };
    let bent = Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![
            [5.0, 0.0, 0.0],
            [5.0 + 7e-7, 0.0, 4.0 / 3.0],
            [5.0 + 4e-7, 0.0, 8.0 / 3.0],
            [5.0, 0.0, 4.0],
        ],
    };
    let sliver = Surface::PlacedRevolution {
        profile: Box::new(bent),
        origin: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        matrix: identity,
        forward: true,
    };
    let mut k = kernel();
    let found = solidcore_convert::brep::recognised(
        &mut k,
        &exact,
        [[0.0, 1.0], [0.0, std::f64::consts::PI]],
    )
    .unwrap();
    assert_eq!(found, Some("cylinder"));
    let found = solidcore_convert::brep::recognised(
        &mut k,
        &sliver,
        [[0.0, 1.0], [0.0, std::f64::consts::PI]],
    )
    .unwrap();
    assert!(found.is_none(), "{found:?}");
}

/// The square `[0, s]^2` at z = 0 as a sheet: one plane face in an open shell of the infinite
/// region, its loop counter-clockwise seen from +z.
fn square(s: f64) -> Record {
    let c = [[0., 0., 0.], [s, 0., 0.], [s, s, 0.], [0., s, 0.]];
    let uv = |p: [f64; 3]| [p[0], p[1]];
    let edges: Vec<Edge> = (0..4)
        .map(|i| Edge {
            nurbs: None,
            curve: Curve::Line {
                start: c[i],
                end: c[(i + 1) % 4],
            },
            interval: [0.0, 1.0],
            vertices: [i as u32, ((i + 1) % 4) as u32],
            tolerance: 1e-6,
            uses: vec![EdgeUseRef {
                face: 0,
                r#loop: 0,
                position: i as u32,
                upper: false,
            }],
        })
        .collect();
    let loop_edges = (0..4)
        .map(|i| OrientedEdge {
            edge: i as u32,
            forward: true,
            pcurve: Curve2::Spline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: vec![uv(c[i]), uv(c[(i + 1) % 4])],
            },
        })
        .collect();
    Record {
        version: VERSION,
        screw_owners: Vec::new(),
        tolerance: 1e-6,
        regions: vec![Region {
            shells: vec![0],
            void: false,
        }],
        shells: vec![Shell::Faces {
            faces: vec![OrientedFace {
                face: 0,
                forward: true,
            }],
            wires: vec![],
        }],
        faces: vec![Face {
            nurbs: None,
            surface: Surface::Plane {
                origin: [0.0; 3],
                u_axis: [1.0, 0.0, 0.0],
                v_axis: [0.0, 1.0, 0.0],
            },
            domain: [[0.0, s], [0.0, s]],
            loops: vec![Loop::Edges { edges: loop_edges }],
        }],
        edges,
        vertices: c
            .iter()
            .map(|&point| Vertex {
                point,
                tolerance: 1e-6,
            })
            .collect(),
    }
}

/// A sheet's open shell holds both sides of its face, in the infinite region.
#[test]
fn a_square_record_builds_a_sheet_holding_both_sides() {
    let mut k = kernel();
    let r = square(3.0);
    r.validate().unwrap();
    let (mut b, order) = to_kernel(&mut k, &r, true).unwrap();
    order_is_total(&order, &r, &b);
    assert_eq!(
        [
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ],
        [1, 4, 4]
    );
    assert!(!is_manifold_solid(&b).unwrap());
    let infinite = b.infinite_region().unwrap();
    let shells = b.region(infinite).unwrap().shells.clone();
    assert_eq!(shells.len(), 1);
    assert_eq!(b.shell(shells[0]).unwrap().get_faceuses().unwrap().len(), 2);
    valid(&mut k, &mut b);
}

/// A record not marked trusted is healed as it is built, and the healed cube is the cube.
#[test]
fn an_untrusted_record_is_healed_into_the_same_body() {
    let mut k = kernel();
    let r = cube(2.0);
    let (mut b, order) = to_kernel(&mut k, &r, false).unwrap();
    order_is_total(&order, &r, &b);
    assert!(is_manifold_solid(&b).unwrap());
    assert!((volume(&k, &mut b) - 8.0).abs() < 1e-9);
    valid(&mut k, &mut b);
}

/// A body goes to the record, validates, comes back to the kernel as the same body (counts,
/// volume, valid) and gives the same record again. Returns the record and the body back.
fn round_trip(k: &mut Kernel, name: &str, b: &mut Brep) -> (Record, Brep) {
    let v0 = volume(k, b);
    let (r, order) = to_record(k, b).unwrap_or_else(|e| panic!("{name}: {e}"));
    r.validate_built().unwrap_or_else(|e| panic!("{name}: {e}"));
    order_is_total(&order, &r, b);
    let (mut back, back_order) = to_kernel(k, &r, true).unwrap_or_else(|e| panic!("{name}: {e}"));
    order_is_total(&back_order, &r, &back);
    let v1 = volume(k, &mut back);
    assert!((v1 - v0).abs() <= 1e-9 * v0.abs(), "{name}: {v0} -> {v1}");
    valid(k, &mut back);
    let (again, _) = to_record(k, &mut back).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(again, r, "{name}: the record changed on a round trip");
    (r, back)
}

/// A capped quarter-torus pipe has a Path definition before export. The established
/// record representation stores its certified net as a spline, and preserves the
/// closed body and its geometry when rebuilt.
#[test]
#[allow(clippy::disallowed_methods)] // Independent test-only geometry oracle; conversion copies fields.
fn a_path_pipe_goes_round_as_its_certified_spline() {
    use solidcore::curve::curve::Curve as KernelCurve;
    use solidcore::math::Point2;
    use solidcore::primitive::api_curves::create_arc;
    use solidcore::sweep::api_brep::create_pipe_sweep;
    use std::f64::consts::PI;

    let mut k = kernel();
    let path = KernelCurve::from(create_arc(Vec3::ZERO, 10.0, 0.0, 90.0).unwrap());
    let mut pipe = create_pipe_sweep(&mut k, 1.0, Some(&path), true).unwrap();
    assert!(is_manifold_solid(&pipe).unwrap());
    valid(&mut k, &mut pipe);
    let expected_volume = 5.0 * PI * PI;
    assert!((volume(&k, &mut pipe) - expected_volume).abs() < 1e-9 * expected_volume);

    let mut samples = Vec::new();
    for &id in pipe.face_list() {
        let surface = pipe.face(id).unwrap().surface.as_ref().unwrap();
        let Some(sweep) = surface.procedural_sweep() else {
            continue;
        };
        assert!(
            sweep.definition().path().is_some(),
            "pipe wall must use Path motion"
        );
        let domain = sweep.net().natural_uv_domain();
        let uv = domain.evaluate(0.37, 0.61);
        let point = sweep.definition().evaluate_definition_point(uv).unwrap();
        let radius = (point.x.hypot(point.y) - 10.0).hypot(point.z);
        assert!((radius - 1.0).abs() < 1e-10);
        samples.push((id, Point2::new(uv.x, uv.y), point, sweep.bound()));
    }
    assert!(
        !samples.is_empty(),
        "the pipe must retain a Path wall before export"
    );

    let counts = (
        pipe.face_list().len(),
        pipe.edge_list().len(),
        pipe.vertex_list().len(),
    );
    let (record, order) = to_record(&mut k, &mut pipe).unwrap();
    record.validate_built().unwrap();
    order_is_total(&order, &record, &pipe);
    for (id, _, _, _) in &samples {
        let index = order.faces.iter().position(|face| face == id).unwrap();
        assert!(matches!(
            record.faces[index].surface,
            Surface::Spline { .. } | Surface::RationalSpline { .. }
        ));
        assert!(record.faces[index].nurbs.is_none());
    }
    let (mut restored, back_order) = to_kernel(&mut k, &record, true).unwrap();
    order_is_total(&back_order, &record, &restored);
    assert_eq!(
        counts,
        (
            restored.face_list().len(),
            restored.edge_list().len(),
            restored.vertex_list().len()
        )
    );
    assert!(is_manifold_solid(&restored).unwrap());
    valid(&mut k, &mut restored);
    assert!((volume(&k, &mut restored) - expected_volume).abs() < 1e-9 * expected_volume);
    for (id, uv, expected, bound) in samples {
        let index = order.faces.iter().position(|face| *face == id).unwrap();
        let wall = restored
            .face(back_order.faces[index])
            .unwrap()
            .surface
            .as_ref()
            .unwrap();
        let actual = wall.evaluate_point(&k, uv).unwrap();
        assert!(
            (actual - expected).length() <= bound + 1e-12,
            "restored point differs from the Path definition"
        );
    }
}

/// A moved M20 flank remains an exact screw definition after the portable
/// full-body record round trip used by the application.
#[test]
#[allow(clippy::disallowed_methods)] // Independent test-only geometry oracle; conversion copies fields.
fn an_offset_m20_body_round_trips_its_screw_definitions() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::curve::curve::Curve as KernelCurve;
    use solidcore::eval::geometry::sweep::SweepDefinitionRecord as KernelSweep;
    use solidcore::geom::tol::{brep_approx_tol_3d, SmallTopoType};
    use solidcore::math::{Point2, Point3};
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::offset::offset_faces::OffsetFacesExecutive;
    use solidcore::primitive::api_curves::create_line_segment;
    use solidcore::sweep::screw::screw_sweep;
    use solidcore_record::brep::sweep::SweepDefinitionRecord as RecordSweep;

    let mut k = kernel();
    let base = Point3::new(-1.0, -64.0, 0.0);
    let pitch = 2.5;
    let root = 10.0 - 5.0 * pitch * 3.0_f64.sqrt() / 16.0;
    let outer = 10.0 + 0.05 * pitch;
    let half_root = pitch / 8.0;
    let half_outer = half_root + (outer - root) / 3.0_f64.sqrt();
    let points = [
        Point3::new(base.x + root, base.y, -pitch - half_root),
        Point3::new(base.x + outer, base.y, -pitch - half_outer),
        Point3::new(base.x + outer, base.y, -pitch + half_outer),
        Point3::new(base.x + root, base.y, -pitch + half_root),
    ];
    let profile: Vec<KernelCurve> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &b)| KernelCurve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let stock = primitives::create_cylinder(&mut k, base, 10.0, 20.0).unwrap();
    let tolerance = brep_approx_tol_3d(&k, &stock, SmallTopoType::NormalSize);
    let groove = screw_sweep(&mut k, &profile, base, Vec3::Z, pitch, 10.0, tolerance).unwrap();
    let mut body = boolean_difference(&mut k, stock, groove).unwrap();
    let strict = |k: &mut Kernel, b: &mut Brep| {
        assert!(is_manifold_solid(b).unwrap());
        let check = check_brep(k, b, CheckLevel::Level2).unwrap();
        assert_eq!(check.assert_valid, Some(true), "{:?}", check.reports);
        assert_eq!(check.failures().count(), 0, "{:?}", check.reports);
    };
    strict(&mut k, &mut body);
    let picked: Vec<_> = body
        .faces()
        .ids()
        .filter(|&id| {
            body.face(id)
                .unwrap()
                .surface
                .as_ref()
                .unwrap()
                .procedural_sweep()
                .is_some()
        })
        .collect();
    assert_eq!(picked.len(), 2);
    let originals: Vec<_> = picked
        .iter()
        .map(|&id| {
            let face = body.face(id).unwrap();
            let surface = face.surface.as_ref().unwrap().copy(&k).unwrap();
            let faceuse = solidcore::topology::face::p2::outer_faceuse(&body, id)
                .unwrap()
                .unwrap();
            let outward = if body.faceuse(faceuse).unwrap().orientation == OrientType::Same {
                1.0
            } else {
                -1.0
            };
            (surface, outward)
        })
        .collect();
    let distance = -0.1;
    let offset_result =
        OffsetFacesExecutive::new().offset_faces(&mut k, &mut body, &picked, distance);
    offset_result.unwrap();
    strict(&mut k, &mut body);

    let mut samples = Vec::new();
    for (id, face) in body.faces().iter() {
        let Some(output) = face.surface.as_ref().unwrap().procedural_sweep() else {
            continue;
        };
        if !output.definition().is_normal_offset() {
            continue;
        }
        let saved = output.definition().to_record(output.net()).unwrap();
        let KernelSweep::Screw {
            offset: Some(offset),
            ..
        } = &saved.definition
        else {
            panic!("offset wall must retain its exact source");
        };
        assert!((offset.signed_distance.abs() - distance.abs()).abs() < 1e-14);
        let (original, outward) = originals.iter().find(|(surface, _)| {
            let sweep = surface.procedural_sweep().unwrap();
            let record = sweep.definition().to_record(sweep.net()).unwrap();
            matches!(record.definition, KernelSweep::Screw { profile, .. } if profile == offset.source)
        }).unwrap();
        let input = original.procedural_sweep().unwrap();
        let input = input.definition();
        let result = output.definition();
        let profile = input.profile().natural_interval();
        let first = input.stations()[0].max(result.stations()[0]);
        let last = input
            .stations()
            .last()
            .unwrap()
            .min(*result.stations().last().unwrap());
        assert!(last > first);
        for fraction in [0.211_324_865_405_187, 0.618_033_988_749_895] {
            let angle = first + fraction * (last - first);
            let w = profile.min() + fraction * (profile.max() - profile.min());
            let before_uv = input
                .face_parameters(Point2::new(w, input.parameter_at_angle(angle).unwrap()))
                .unwrap();
            let after_uv = result
                .face_parameters(Point2::new(w, result.parameter_at_angle(angle).unwrap()))
                .unwrap();
            let jet = input.evaluate_definition(before_uv, 1, 1).unwrap();
            let normal = jet[1][0].cross(jet[0][1]);
            let normal = normal * (*outward / normal.length());
            let point = result.evaluate_definition_point(after_uv).unwrap();
            let rounding = 2e-12 * (1.0 + point.length());
            assert!((point - jet[0][0] - normal * distance).length() <= rounding);
            assert!(((point - jet[0][0]).dot(normal) - distance).abs() <= rounding);
            assert!(
                (output.net().evaluate_point(after_uv).unwrap() - point).length() <= output.bound()
            );
            samples.push((id, after_uv, point, rounding));
        }
    }
    assert_eq!(samples.len(), 4, "both moved flanks need two witnesses");
    let (mut record, order) = to_record(&mut k, &mut body).unwrap();
    record.validate_built().unwrap();
    record.validate().unwrap();
    assert_eq!(record.screw_owners.len(), 2);
    assert_eq!(
        record
            .edges
            .iter()
            .filter(|e| matches!(e.curve, Curve::OnSurfaceRef { .. }))
            .count(),
        52
    );
    order_is_total(&order, &record, &body);
    let original_record = record.clone();
    for (id, _, _, _) in &samples {
        let index = order.faces.iter().position(|face| face == id).unwrap();
        let Surface::ProceduralSweep(sweep) = &mut record.faces[index].surface else {
            panic!("portable record lost screw definition")
        };
        assert!(matches!(
            sweep.definition,
            RecordSweep::Screw {
                offset: Some(_),
                ..
            }
        ));
        // A forged finite bound must never authorize a saved net.
        sweep.bound = 1e-300;
    }
    record.validate_built().unwrap();
    let bytes = serde_json::to_vec(&record).unwrap();
    let decoded: Record = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        serde_json::to_vec(&decoded).unwrap(),
        bytes,
        "portable bytes must preserve exact owner and UV fields"
    );
    decoded.validate().unwrap();
    record = decoded;
    let (mut reopened, reopened_order) = to_kernel(&mut k, &record, true).unwrap();
    order_is_total(&reopened_order, &record, &reopened);
    strict(&mut k, &mut reopened);
    let mut owner_payloads = std::collections::BTreeMap::new();
    for (index, edge) in record.edges.iter().enumerate() {
        let Curve::OnSurfaceRef { owner, .. } = &edge.curve else {
            continue;
        };
        let original = solidcore::topology::edge::curve(&body, order.edges[index])
            .unwrap()
            .unwrap();
        let after = solidcore::topology::edge::curve(&reopened, reopened_order.edges[index])
            .unwrap()
            .unwrap();
        let solidcore::curve::curve::CurveKind::CrvOnSurf(curve) = after.kind() else {
            panic!("lost exact surface curve")
        };
        let net = curve
            .surface()
            .procedural_sweep()
            .unwrap()
            .net()
            .gw_nurb()
            .unwrap();
        let payload = net.row(0).unwrap().as_ptr() as usize;
        if let Some(first) = owner_payloads.insert(*owner, payload) {
            assert_eq!(first, payload);
        }
        for fraction in [0.211_324_865_405_187, 0.618_033_988_749_895] {
            let t = edge.interval[0] + fraction * (edge.interval[1] - edge.interval[0]);
            let before = original.evaluate(&k, t, 1, false, false).unwrap();
            let restored = after.evaluate(&k, t, 1, false, false).unwrap();
            for (a, b) in before.iter().zip(&restored) {
                assert!((*a - *b).length() <= 1e-11 * (1.0 + a.length()));
            }
        }
    }
    assert_eq!(owner_payloads.len(), 2);
    for (id, uv, point, rounding) in samples {
        let index = order.faces.iter().position(|face| *face == id).unwrap();
        let surface = reopened
            .face(reopened_order.faces[index])
            .unwrap()
            .surface
            .as_ref()
            .unwrap();
        let sweep = surface
            .procedural_sweep()
            .expect("portable restore lost screw definition");
        assert!(sweep.definition().is_normal_offset());
        assert!(sweep.bound() > 1e-300 && sweep.bound().is_finite());
        assert!(sweep.bound() <= reopened.tolerance());
        assert!(
            (sweep.definition().evaluate_definition_point(uv).unwrap() - point).length()
                <= rounding
        );
        assert!((sweep.net().evaluate_point(uv).unwrap() - point).length() <= sweep.bound());
    }
    let (again, _) = to_record(&mut k, &mut reopened).unwrap();
    assert_eq!(again.screw_owners.len(), original_record.screw_owners.len());
    for (a, b) in again.screw_owners.iter().zip(&original_record.screw_owners) {
        assert_eq!(a.net, b.net);
        assert_eq!(
            (&a.stations, a.half, a.map_a, a.map_b, a.swapped),
            (&b.stations, b.half, b.map_a, b.map_b, b.swapped)
        );
        let (
            RecordSweep::Screw {
                profile: ap,
                motion: am,
                offset: ao,
            },
            RecordSweep::Screw {
                profile: bp,
                motion: bm,
                offset: bo,
            },
        ) = (&a.definition, &b.definition);
        assert_eq!((ap, am), (bp, bm));
        assert_eq!(
            ao.as_ref().map(|o| (&o.source, o.signed_distance)),
            bo.as_ref().map(|o| (&o.source, o.signed_distance))
        );
    }
    assert_eq!(again.regions, original_record.regions);
    assert_eq!(again.shells, original_record.shells);
    assert_eq!(again.edges, original_record.edges);
    assert_eq!(again.vertices, original_record.vertices);
    assert_eq!(again.faces.len(), original_record.faces.len());
    for (a, b) in again.faces.iter().zip(&original_record.faces) {
        assert_eq!(a.loops, b.loops);
        if let (Surface::ProceduralSweep(a), Surface::ProceduralSweep(b)) = (&a.surface, &b.surface)
        {
            assert_eq!(a.stations, b.stations);
            assert_eq!(
                (a.half, a.map_a, a.map_b, a.swapped),
                (b.half, b.map_a, b.map_b, b.swapped)
            );
            assert_eq!(a.net, b.net);
            let (
                RecordSweep::Screw {
                    profile: ap,
                    motion: am,
                    offset: Some(ao),
                },
                RecordSweep::Screw {
                    profile: bp,
                    motion: bm,
                    offset: Some(bo),
                },
            ) = (&a.definition, &b.definition)
            else {
                panic!("missing offset screw after restore")
            };
            assert_eq!(
                (ap, am, &ao.source, ao.signed_distance),
                (bp, bm, &bo.source, bo.signed_distance)
            );
        } else {
            assert!(
                !matches!(a.surface, Surface::ProceduralSweep(_))
                    && !matches!(b.surface, Surface::ProceduralSweep(_))
            );
        }
    }
}

/// Reconstructed acceptance classes, not the unavailable historical #2613 driver.
/// The external dimensions match the independently verified M20 fixture above.
/// The internal bore/groove follows the existing `issue_410` construction, scaled
/// to M20x2.5. Segmentation uses the existing radial sweep builder: a zero radial
/// law is identically zero, so its breakpoints change face partitioning while
/// leaving the ideal screw geometry unchanged.
#[derive(Clone, Copy, Debug)]
enum MatrixShape {
    SegmentedExternal,
    ContinuousExternal,
    ContinuousInternal,
}

#[allow(clippy::disallowed_methods)] // Independent test-only geometry oracle; conversion copies fields.
fn matrix_m20_body(k: &mut Kernel, shape: MatrixShape) -> Brep {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::curve::curve::Curve as KernelCurve;
    use solidcore::geom::tol::{brep_approx_tol_3d, SmallTopoType};
    use solidcore::math::Point3;
    use solidcore::primitive::api_curves::create_line_segment;
    use solidcore::sweep::screw::{screw_sweep, screw_sweep_radial};

    let base = Point3::new(-1.0, -64.0, 0.0);
    let pitch = 2.5;
    let minor = 10.0 - 5.0 * pitch * 3.0_f64.sqrt() / 16.0;
    let internal = matches!(shape, MatrixShape::ContinuousInternal);
    let (low, high, half_low, half_high) = if internal {
        let low = minor - 0.05;
        let half_high = pitch / 16.0;
        (
            low,
            10.0,
            half_high + (10.0 - low) / 3.0_f64.sqrt(),
            half_high,
        )
    } else {
        let high = 10.0 + 0.05 * pitch;
        let half_low = pitch / 8.0;
        (
            minor,
            high,
            half_low,
            half_low + (high - minor) / 3.0_f64.sqrt(),
        )
    };
    let points = [
        Point3::new(base.x + low, base.y, -pitch - half_low),
        Point3::new(base.x + high, base.y, -pitch - half_high),
        Point3::new(base.x + high, base.y, -pitch + half_high),
        Point3::new(base.x + low, base.y, -pitch + half_low),
    ];
    let profile: Vec<KernelCurve> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &b)| KernelCurve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let stock =
        primitives::create_cylinder(k, base, if internal { 15.0 } else { 10.0 }, 20.0).unwrap();
    let tolerance = brep_approx_tol_3d(k, &stock, SmallTopoType::NormalSize);
    let stock = if internal {
        let bore = primitives::create_cylinder(
            k,
            Point3::new(base.x, base.y, -pitch),
            minor,
            20.0 + 2.0 * pitch,
        )
        .unwrap();
        boolean_difference(k, stock, bore).unwrap()
    } else {
        stock
    };
    let groove = if matches!(shape, MatrixShape::SegmentedExternal) {
        let law: Vec<_> = (0..=5).map(|i| (2.0 * f64::from(i), 0.0)).collect();
        screw_sweep_radial(k, &profile, base, Vec3::Z, pitch, 10.0, &law, tolerance).unwrap()
    } else {
        screw_sweep(k, &profile, base, Vec3::Z, pitch, 10.0, tolerance).unwrap()
    };
    boolean_difference(k, stock, groove).unwrap()
}

fn matrix_m20_selections(k: &Kernel, b: &Brep) -> (Vec<solidcore::FaceId>, Vec<solidcore::FaceId>) {
    use solidcore::surface::surface::SurfaceKind;
    let mut flanks = Vec::new();
    let mut lateral = Vec::new();
    for (id, face) in b.faces().iter() {
        let surface = face.surface.as_ref().unwrap();
        if surface.procedural_sweep().is_some() {
            flanks.push(id);
            lateral.push(id);
        } else if matches!(surface.kind_with_nurb(k).unwrap(), SurfaceKind::Cone(c) if c.is_cylinder_class())
        {
            lateral.push(id);
        }
    }
    (flanks, lateral)
}

fn check_matrix_fixture(shape: MatrixShape) {
    let mut k = kernel();
    let mut body = matrix_m20_body(&mut k, shape);
    assert!(is_manifold_solid(&body).unwrap(), "{shape:?}");
    let check = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap();
    assert_eq!(
        check.assert_valid,
        Some(true),
        "{shape:?}: {:?}",
        check.reports
    );
    assert_eq!(
        check.failures().count(),
        0,
        "{shape:?}: {:?}",
        check.reports
    );
    let (flanks, lateral) = matrix_m20_selections(&k, &body);
    if matches!(shape, MatrixShape::SegmentedExternal) {
        assert!(
            flanks.len() > 2,
            "law breaks must create segmented screw walls"
        );
    } else {
        assert_eq!(flanks.len(), 2, "one continuous face per flank");
    }
    assert!(
        lateral.len() > flanks.len(),
        "all-lateral selection must include cylinders"
    );
}

#[test]
fn reconstructed_m20_segmented_external_fixture() {
    check_matrix_fixture(MatrixShape::SegmentedExternal);
}
#[test]
fn reconstructed_m20_continuous_external_fixture() {
    check_matrix_fixture(MatrixShape::ContinuousExternal);
}
#[test]
fn reconstructed_m20_continuous_internal_fixture() {
    check_matrix_fixture(MatrixShape::ContinuousInternal);
}

fn exercise_m20_matrix(shape: MatrixShape, all_lateral: bool, distance: f64) {
    exercise_m20(shape, all_lateral, false, distance);
}

/// [`exercise_m20_matrix`] with one flank of the M20 body moved and the other kept, after the body
/// has been through its record and back as an application holds it: its end caps' supports are then their faces' boxes, short of the
/// thread's radial disk, and the kept flank's section starts on the root land's radius, a root
/// no move of its own pads. Both were refused (#11582).
fn exercise_m20_one_flank(shape: MatrixShape, distance: f64) {
    exercise_m20(shape, false, true, distance);
}

#[allow(clippy::disallowed_methods)] // Independent test-only geometry oracle; conversion copies fields.
#[allow(clippy::too_many_lines)] // one witnessed matrix case
fn exercise_m20(shape: MatrixShape, all_lateral: bool, one_flank: bool, distance: f64) {
    use solidcore::eval::geometry::sweep::SweepDefinitionRecord;
    use solidcore::math::Point2;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::{EvaluateFlags, OrientType};
    use solidcore::offset::offset_faces::OffsetFacesExecutive;
    use solidcore::surface::surface::SurfaceKind;

    let mut k = kernel();
    let mut body = matrix_m20_body(&mut k, shape);
    let strict = |k: &mut Kernel, body: &mut Brep| {
        assert!(is_manifold_solid(body).unwrap());
        let check = check_brep(k, body, CheckLevel::Level2).unwrap();
        assert_eq!(check.assert_valid, Some(true), "{:?}", check.reports);
        assert_eq!(check.failures().count(), 0, "{:?}", check.reports);
    };
    strict(&mut k, &mut body);
    if one_flank {
        let (record, _) = to_record(&mut k, &mut body).unwrap();
        body = to_kernel(&mut k, &record, true).unwrap().0;
        strict(&mut k, &mut body);
    }
    let (flanks, lateral) = matrix_m20_selections(&k, &body);
    assert!(!flanks.is_empty() && (!all_lateral || lateral.len() > flanks.len()));
    let profile_of = |id: solidcore::FaceId| {
        let sweep = body
            .face(id)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .procedural_sweep()
            .unwrap();
        let SweepDefinitionRecord::Screw { profile, .. } = sweep
            .definition()
            .to_record(sweep.net())
            .unwrap()
            .definition;
        profile
    };
    let one: Vec<_> = flanks
        .iter()
        .copied()
        .filter(|&id| profile_of(id) == profile_of(flanks[0]))
        .collect();
    assert!(one.len() < flanks.len(), "the other flank stays");
    let selected = if all_lateral {
        &lateral
    } else if one_flank {
        &one
    } else {
        &flanks
    };
    let moved_flanks = if one_flank { &one } else { &flanks };

    let originals: Vec<_> = moved_flanks
        .iter()
        .map(|&id| {
            let face = body.face(id).unwrap();
            let surface = face.surface.as_ref().unwrap().copy(&k).unwrap();
            let faceuse = solidcore::topology::face::p2::outer_faceuse(&body, id)
                .unwrap()
                .unwrap();
            let outward = if body.faceuse(faceuse).unwrap().orientation == OrientType::Same {
                1.0
            } else {
                -1.0
            };
            (surface, outward)
        })
        .collect();

    // Independently witness each selected cylindrical support's radial motion.
    let cylinders: Vec<_> = if all_lateral {
        selected
            .iter()
            .filter_map(|&id| {
                let face = body.face(id).unwrap();
                let surface = face.surface.as_ref().unwrap();
                let SurfaceKind::Cone(cone) = surface.kind_with_nurb(&k).unwrap() else {
                    return None;
                };
                if !cone.is_cylinder_class() {
                    return None;
                }
                let uv = Point2::new(
                    0.5 * (face.uv_domain.u_min() + face.uv_domain.u_max()),
                    0.5 * (face.uv_domain.v_min() + face.uv_domain.v_max()),
                );
                let jet = surface
                    .evaluate(&k, uv, 1, 1, EvaluateFlags::default())
                    .unwrap();
                let faceuse = solidcore::topology::face::p2::outer_faceuse(&body, id)
                    .unwrap()
                    .unwrap();
                let outward = if body.faceuse(faceuse).unwrap().orientation == OrientType::Same {
                    1.0
                } else {
                    -1.0
                };
                let point = jet[0][0];
                let radial = Vec3::new(point.x + 1.0, point.y + 64.0, 0.0);
                let orientation = (jet[1][0].cross(jet[0][1]) * outward).dot(radial);
                assert!(orientation.is_finite() && orientation != 0.0);
                Some((cone.canonical().radius, orientation.signum()))
            })
            .collect()
    } else {
        Vec::new()
    };

    let (before_record, _) = to_record(&mut k, &mut body).unwrap();
    before_record.validate().unwrap();
    let mut executive = OffsetFacesExecutive::new();
    let result = executive.offset_faces(&mut k, &mut body, selected, distance);
    assert!(
        result.is_ok(),
        "offset failed: {result:?}; refusal={:?}; reason={}",
        executive.refusal(),
        executive.reason()
    );
    strict(&mut k, &mut body);
    let mut witnessed = vec![false; originals.len()];
    let mut output_flanks = 0_usize;
    for (_, face) in body.faces().iter() {
        let Some(output) = face.surface.as_ref().unwrap().procedural_sweep() else {
            continue;
        };
        let saved = output.definition().to_record(output.net()).unwrap();
        let SweepDefinitionRecord::Screw {
            offset: Some(offset),
            ..
        } = &saved.definition
        else {
            assert!(one_flank, "a moved flank lost its exact offset source");
            continue;
        };
        assert!((offset.signed_distance.abs() - distance.abs()).abs() < 1e-14);
        output_flanks += 1;
        for (index, (original, outward)) in originals.iter().enumerate() {
            let input = original.procedural_sweep().unwrap();
            let input_record = input.definition().to_record(input.net()).unwrap();
            let SweepDefinitionRecord::Screw { profile, .. } = input_record.definition;
            if profile != offset.source {
                continue;
            }
            let a = input.definition().stations()[0].max(output.definition().stations()[0]);
            let b = input
                .definition()
                .stations()
                .last()
                .unwrap()
                .min(*output.definition().stations().last().unwrap());
            if b <= a {
                continue;
            }
            let profile = input.definition().profile().natural_interval();
            for fraction in [0.211_324_865_405_187, 0.618_033_988_749_895] {
                let angle = a + fraction * (b - a);
                let w = profile.min() + fraction * (profile.max() - profile.min());
                let before_uv = input
                    .definition()
                    .face_parameters(Point2::new(
                        w,
                        input.definition().parameter_at_angle(angle).unwrap(),
                    ))
                    .unwrap();
                let after_uv = output
                    .definition()
                    .face_parameters(Point2::new(
                        w,
                        output.definition().parameter_at_angle(angle).unwrap(),
                    ))
                    .unwrap();
                let before = input
                    .definition()
                    .evaluate_definition(before_uv, 1, 1)
                    .unwrap();
                let normal = before[1][0].cross(before[0][1]);
                let normal = normal * (*outward / normal.length());
                let after = output
                    .definition()
                    .evaluate_definition_point(after_uv)
                    .unwrap();
                let roundoff = 2e-12 * (1.0 + after.length());
                assert!((after - before[0][0] - normal * distance).length() <= roundoff);
                assert!(
                    (output.net().evaluate_point(after_uv).unwrap() - after).length()
                        <= output.bound()
                );
            }
            witnessed[index] = true;
        }
    }
    assert!(
        output_flanks > 0 && witnessed.iter().all(|w| *w),
        "every selected flank needs an exact movement witness"
    );
    for (radius, orientation) in cylinders {
        let expected = radius + distance * orientation;
        let found = body.faces().iter().any(|(_, face)| {
            let Some(surface) = face.surface.as_ref() else { return false };
            matches!(surface.kind_with_nurb(&k), Ok(SurfaceKind::Cone(cone)) if cone.is_cylinder_class() && (cone.canonical().radius - expected).abs() <= 2e-11 * (1.0 + expected.abs()))
        });
        assert!(
            found,
            "selected cylinder radius {radius} did not move to {expected}"
        );
    }
}

macro_rules! m20_case {
    ($name:ident, $shape:expr, $lateral:expr, $distance:expr) => {
        #[test]
        fn $name() {
            exercise_m20_matrix($shape, $lateral, $distance);
        }
    };
}
m20_case!(
    m20_segmented_external_flanks_in,
    MatrixShape::SegmentedExternal,
    false,
    -0.1
);
m20_case!(
    m20_segmented_external_flanks_out,
    MatrixShape::SegmentedExternal,
    false,
    0.1
);
m20_case!(
    m20_segmented_external_lateral_in,
    MatrixShape::SegmentedExternal,
    true,
    -0.1
);
m20_case!(
    m20_segmented_external_lateral_out,
    MatrixShape::SegmentedExternal,
    true,
    0.1
);
m20_case!(
    m20_continuous_external_flanks_in,
    MatrixShape::ContinuousExternal,
    false,
    -0.1
);
m20_case!(
    m20_continuous_external_flanks_out,
    MatrixShape::ContinuousExternal,
    false,
    0.1
);
m20_case!(
    m20_continuous_external_lateral_in,
    MatrixShape::ContinuousExternal,
    true,
    -0.1
);
m20_case!(
    m20_continuous_external_lateral_out,
    MatrixShape::ContinuousExternal,
    true,
    0.1
);
m20_case!(
    m20_continuous_internal_flanks_in,
    MatrixShape::ContinuousInternal,
    false,
    -0.1
);
m20_case!(
    m20_continuous_internal_flanks_out,
    MatrixShape::ContinuousInternal,
    false,
    0.1
);
m20_case!(
    m20_continuous_internal_lateral_in,
    MatrixShape::ContinuousInternal,
    true,
    -0.1
);
m20_case!(
    m20_continuous_internal_lateral_out,
    MatrixShape::ContinuousInternal,
    true,
    0.1
);

macro_rules! m20_one_flank_case {
    ($name:ident, $shape:expr, $distance:expr) => {
        #[test]
        fn $name() {
            exercise_m20_one_flank($shape, $distance);
        }
    };
}
m20_one_flank_case!(
    m20_continuous_external_one_flank_in,
    MatrixShape::ContinuousExternal,
    -0.1
);
m20_one_flank_case!(
    m20_continuous_external_one_flank_out,
    MatrixShape::ContinuousExternal,
    0.1
);
m20_one_flank_case!(
    m20_continuous_internal_one_flank_in,
    MatrixShape::ContinuousInternal,
    -0.1
);

/// SolidCraft #11582: an M10x1.5 thread on a 6 mm shaft (a cylinder of radius 5 and height 6),
/// its flanks one continuous face each, as SolidCraft's record holds it. Either flank moved in
/// 0.05 builds a strict closed solid. The moved flank's section by an end cap crosses the root
/// cylinder 3° off its tangent, so that section's own crossing ended 1.7e-5 along the circle
/// from the corner where the cap, the root cylinder and the flank meet, past the half vertex
/// tolerance the check allows an edge's end: the second flank was refused as unsound. Every
/// edge there now ends where its curve passes nearest that corner.
#[test]
fn either_flank_of_a_continuous_m10_thread_moves_in() {
    use solidcore::eval::geometry::sweep::SweepDefinitionRecord;
    use solidcore::mass::integrator::properties::face_area;
    use solidcore::offset::offset_faces::OffsetFacesExecutive;

    let gz = format!(
        "{}/tests/data/m10x1.5_continuous_thread_on_6mm_shaft.record.json.gz",
        env!("CARGO_MANIFEST_DIR")
    );
    let record: Record = serde_json::from_reader(flate2::read::GzDecoder::new(
        std::fs::File::open(&gz).unwrap(),
    ))
    .unwrap();
    let strict = |k: &mut Kernel, body: &mut Brep| {
        assert!(is_manifold_solid(body).unwrap());
        let check = check_brep(k, body, CheckLevel::Level2).unwrap();
        assert_eq!(check.assert_valid, Some(true), "{:?}", check.reports);
        assert_eq!(check.failures().count(), 0, "{:?}", check.reports);
    };
    let mut k = kernel();
    let (mut shaft, _) = to_kernel(&mut k, &record, true).unwrap();
    strict(&mut k, &mut shaft);
    let before = volume(&k, &mut shaft);
    let flanks: Vec<_> = shaft
        .faces()
        .ids()
        .filter(|&id| {
            let surface = shaft.face(id).unwrap().surface.as_ref().unwrap();
            surface.procedural_sweep().is_some()
        })
        .collect();
    assert_eq!(flanks.len(), 2, "one continuous face per flank");
    let distance = -0.05;
    for flank in flanks {
        let mut body = solidcore::topology::brep::p3::brep_copy(&mut k, &shaft).unwrap();
        OffsetFacesExecutive::new()
            .offset_faces(&mut k, &mut body, &[flank], distance)
            .unwrap_or_else(|e| panic!("flank {flank:?}: {e:?}"));
        strict(&mut k, &mut body);
        // The material behind the flank moved |d| along its normal: its area times |d|, to
        // the first order (the flank's curvature and its ends change it by far less than 5%).
        let area = face_area(&k, &mut shaft, flank, 1e-9).unwrap();
        let lost = before - volume(&k, &mut body);
        let swept = distance.abs() * area;
        assert!(
            (lost - swept).abs() < 0.05 * swept,
            "flank {flank:?}: {lost} mm³ lost, {swept} mm³ swept"
        );
        let moved: Vec<_> = body
            .faces()
            .iter()
            .filter_map(|(_, face)| face.surface.as_ref()?.procedural_sweep())
            .filter(|sweep| sweep.definition().is_normal_offset())
            .map(|sweep| {
                sweep
                    .definition()
                    .to_record(sweep.net())
                    .unwrap()
                    .definition
            })
            .collect();
        assert_eq!(moved.len(), 1, "flank {flank:?}: one moved wall");
        let SweepDefinitionRecord::Screw {
            offset: Some(offset),
            ..
        } = &moved[0]
        else {
            panic!("the moved wall keeps its exact source");
        };
        assert!((offset.signed_distance.abs() - distance.abs()).abs() < 1e-14);
    }
}

/// SolidCraft #11582: the M10x1.5 thread of the test above as a design saved before its
/// flanks were joined holds it, each flank cut into a face for every half turn, and
/// either whole flank (all its pieces) moved in 0.05. Two seams were refused. At a seam's
/// root end the half-turn root cylinders read back from the record have a STEP height a
/// rounding off their native one (`[0, 1.6894999999999998]` against `[0, 1.6895]`), and
/// their continuation kept their parameters only for the identity height map: it gave a
/// bare B-spline, no cylinder to certify the seam's corner on. At the crest end the seam
/// stops inside the stock cylinder, whose own seam is 35° away: no cylinder edge meets
/// it there. The continuation now keeps a height map off the identity, and a seam ending
/// inside one coaxial cylinder face takes its corner from that face.
#[test]
fn either_whole_flank_of_a_half_turn_m10_thread_moves_in() {
    use solidcore::eval::geometry::sweep::SweepDefinitionRecord;
    use solidcore::mass::integrator::properties::face_area;
    use solidcore::offset::offset_faces::OffsetFacesExecutive;

    let gz = format!(
        "{}/tests/data/m10x1.5_half_turn_thread_on_6mm_shaft.record.json.gz",
        env!("CARGO_MANIFEST_DIR")
    );
    let record: Record = serde_json::from_reader(flate2::read::GzDecoder::new(
        std::fs::File::open(&gz).unwrap(),
    ))
    .unwrap();
    let strict = |k: &mut Kernel, body: &mut Brep| {
        assert!(is_manifold_solid(body).unwrap());
        let check = check_brep(k, body, CheckLevel::Level2).unwrap();
        assert_eq!(check.assert_valid, Some(true), "{:?}", check.reports);
        assert_eq!(check.failures().count(), 0, "{:?}", check.reports);
    };
    let mut k = kernel();
    let (mut shaft, _) = to_kernel(&mut k, &record, true).unwrap();
    strict(&mut k, &mut shaft);
    let before = volume(&k, &mut shaft);
    let profile = |body: &Brep, id| {
        let sweep = body
            .face(id)
            .unwrap()
            .surface
            .as_ref()?
            .procedural_sweep()?;
        let SweepDefinitionRecord::Screw { profile, .. } = sweep
            .definition()
            .to_record(sweep.net())
            .unwrap()
            .definition;
        Some(profile)
    };
    // the flanks: the screw faces grouped by their profile
    let mut flanks: Vec<Vec<solidcore::FaceId>> = Vec::new();
    for id in shaft.faces().ids() {
        let Some(own) = profile(&shaft, id) else {
            continue;
        };
        match flanks
            .iter_mut()
            .find(|f| profile(&shaft, f[0]).as_ref() == Some(&own))
        {
            Some(flank) => flank.push(id),
            None => flanks.push(vec![id]),
        }
    }
    assert_eq!(flanks.len(), 2);
    assert!(flanks.iter().all(|f| f.len() > 1), "a face per half turn");
    let distance = -0.05;
    for flank in flanks {
        let mut body = solidcore::topology::brep::p3::brep_copy(&mut k, &shaft).unwrap();
        OffsetFacesExecutive::new()
            .offset_faces(&mut k, &mut body, &flank, distance)
            .unwrap_or_else(|e| panic!("flank {flank:?}: {e:?}"));
        strict(&mut k, &mut body);
        // as for the continuous flank: its area times |d|, to well within 5%
        let area: f64 = flank
            .iter()
            .map(|&id| face_area(&k, &mut shaft, id, 1e-9).unwrap())
            .sum();
        let lost = before - volume(&k, &mut body);
        let swept = distance.abs() * area;
        assert!(
            (lost - swept).abs() < 0.05 * swept,
            "flank {flank:?}: {lost} mm³ lost, {swept} mm³ swept"
        );
        let moved: Vec<f64> = body
            .faces()
            .iter()
            .filter_map(|(_, face)| face.surface.as_ref()?.procedural_sweep())
            .filter(|sweep| sweep.definition().is_normal_offset())
            .map(|sweep| {
                let SweepDefinitionRecord::Screw {
                    offset: Some(offset),
                    ..
                } = sweep
                    .definition()
                    .to_record(sweep.net())
                    .unwrap()
                    .definition
                else {
                    panic!("a moved wall keeps its exact source");
                };
                offset.signed_distance
            })
            .collect();
        assert_eq!(
            moved.len(),
            flank.len(),
            "flank {flank:?}: every piece moved"
        );
        assert!(moved
            .iter()
            .all(|d| (d.abs() - distance.abs()).abs() < 1e-14));
    }
}

fn boxed(k: &mut Kernel, o: [f64; 3], s: f64) -> Brep {
    primitives::create_box(k, Vec3::new(o[0], o[1], o[2]), s, s, s).unwrap()
}

/// A box with a cubic void: the void is a bounded region of its own, its shell listed by the
/// material round it as an inner shell, and the enclosing shell goes first again however the
/// kernel's list is turned.
#[test]
fn a_void_goes_round_as_its_own_region_and_shell() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::topology::brep::enclosing::put_enclosing_shells_first;
    let mut k = kernel();
    let outer = boxed(&mut k, [0.0; 3], 10.0);
    let inner = boxed(&mut k, [3.0; 3], 4.0);
    let mut b = boolean_difference(&mut k, outer, inner).unwrap();
    assert!((volume(&k, &mut b) - 936.0).abs() < 1e-9);
    let (r, mut back) = round_trip(&mut k, "void", &mut b);
    assert_eq!(r.regions.len(), 3);
    assert_eq!(r.shells.len(), 2);
    let material: Vec<_> = r.regions.iter().filter(|g| g.shells.len() == 2).collect();
    assert_eq!(material.len(), 1, "{:?}", r.regions);
    assert!(!material[0].void);
    let void = r.regions.iter().find(|g| g.void).expect("a void region");
    assert_eq!(void.shells, vec![material[0].shells[1]]);
    // The kernel's list turned round: the enclosing shell is put first again.
    let region = b_region_with_two_shells(&back);
    let before = back.region(region).unwrap().shells.clone();
    back.region_mut(region).unwrap().shells = before.iter().rev().copied().collect();
    put_enclosing_shells_first(&mut k, &mut back).unwrap();
    assert_eq!(back.region(region).unwrap().shells, before);
}

/// The cup and lid on the default kernel: a 100-cube hollowed to an open cup (walls and floor
/// 10) joined with a lid over its top 10 seals an 80-cube cavity, 1000000 − 512000. The union
/// leaves the cavity's shell first in the solid region's list; the record still lists the
/// solid's outer shell first (the one facing the infinite region), so it validates and goes
/// round. The record is the same whichever way round the kernel lists the solid's shells.
#[test]
fn a_void_the_kernel_lists_first_goes_round_with_the_outer_shell_first() {
    use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
    for lid_first in [false, true] {
        let mut k = Kernel::new(&KernelOptions::default());
        let cube = boxed(&mut k, [3.0, 4.0, 5.0], 100.0);
        let pocket =
            primitives::create_box(&mut k, Vec3::new(13.0, 14.0, 15.0), 80.0, 80.0, 100.0).unwrap();
        let lid =
            primitives::create_box(&mut k, Vec3::new(3.0, 4.0, 95.0), 100.0, 100.0, 10.0).unwrap();
        let cup = boolean_difference(&mut k, cube, pocket).unwrap();
        let (a, c) = if lid_first { (lid, cup) } else { (cup, lid) };
        let mut b = boolean_union(&mut k, a, c).unwrap();
        let name = format!("cup and lid (lid first: {lid_first})");
        assert!(
            (volume(&k, &mut b) - 488_000.0).abs() <= 1e-9 * 488_000.0,
            "{name}"
        );
        let region = b_region_with_two_shells(&b);
        let listed = b.region(region).unwrap().shells.clone();
        let (r, _) = round_trip(&mut k, &name, &mut b);
        assert_eq!(r.regions.len(), 3, "{name}");
        let material: Vec<_> = r.regions.iter().filter(|g| g.shells.len() == 2).collect();
        assert_eq!(material.len(), 1, "{name}: {:?}", r.regions);
        let void = r.regions.iter().find(|g| g.void).expect("a void region");
        assert_eq!(void.shells, vec![material[0].shells[1]], "{name}");
        // the kernel's list turned round: the same record
        b.region_mut(region).unwrap().shells = listed.iter().rev().copied().collect();
        let (turned, _) = to_record(&mut k, &mut b).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            turned, r,
            "{name}: the record follows the kernel's list order"
        );
    }
}

fn b_region_with_two_shells(b: &Brep) -> solidcore::topology::RegionId {
    solidcore::topology::brep::p1::regions(b, None)
        .unwrap()
        .into_iter()
        .find(|&g| Some(g) != b.infinite_region() && b.region(g).unwrap().shells.len() == 2)
        .unwrap()
}

/// Two boxes apart are two bounded regions and two shells; two boxes touching along an edge
/// are split there, each solid with its own copy of the shared edge and vertices, every copy
/// naming the one kernel edge.
#[test]
fn lumps_and_solids_touching_along_an_edge_go_round() {
    use solidcore::boolean::api_brep::boolean_union;
    let mut k = kernel();
    let a = boxed(&mut k, [0.0; 3], 1.0);
    let c = boxed(&mut k, [3.0, 0.0, 0.0], 1.0);
    let mut apart = boolean_union(&mut k, a, c).unwrap();
    let (r, _) = round_trip(&mut k, "apart", &mut apart);
    assert_eq!((r.regions.len(), r.shells.len(), r.faces.len()), (3, 2, 12));

    let a = boxed(&mut k, [0.0; 3], 1.0);
    let c = boxed(&mut k, [1.0, 1.0, 0.0], 1.0);
    let mut touching = boolean_union(&mut k, a, c).unwrap();
    let kernel_edges = touching.edge_list().len();
    let (r, order) = to_record(&mut k, &mut touching).unwrap();
    r.validate_built().unwrap();
    assert_eq!(r.shells.len(), 2);
    assert_eq!(
        r.edges.len(),
        kernel_edges + 1,
        "the shared edge is copied once"
    );
    let mut named = order.edges.clone();
    named.sort_unstable_by_key(|e| e.index());
    named.dedup();
    assert_eq!(named.len(), kernel_edges, "every kernel edge is named");
    round_trip(&mut k, "touching", &mut touching);
}

/// An analytic body's record: its curved faces are labels on their NURBS forms, its circles
/// labels on theirs, its lines as the kernel holds them (its point and unit vector, over the
/// height); each pcurve on its edge's interval.
#[test]
fn analytic_records_carry_labels_on_their_forms() {
    let mut k = kernel();
    let mut b = primitives::create_cylinder(&mut k, Vec3::new(0.0, 0.0, 0.0), 2.0, 5.0).unwrap();
    let (r, _) = round_trip(&mut k, "cylinder", &mut b);
    for f in &r.faces {
        match f.surface {
            Surface::Cylinder { radius, .. } => {
                assert_eq!(radius, 2.0);
                assert!(f.nurbs.is_some());
            }
            Surface::Plane { .. } => assert!(f.nurbs.is_none()),
            ref other => panic!("a {other:?} face"),
        }
    }
    for e in &r.edges {
        match &e.curve {
            Curve::Line { start, end } => {
                // as the kernel holds the seam: its point and unit vector, over its height
                assert_eq!(e.interval, [0.0, 5.0]);
                assert_eq!((start[2], end[2]), (0.0, 1.0));
                assert!(e.nurbs.is_none());
            }
            Curve::Circle { radius, .. } => {
                assert_eq!(*radius, 2.0);
                assert!(e.nurbs.is_some());
            }
            other => panic!("a {other:?} edge"),
        }
    }
}

/// Uses that go to the kernel bare (without their record pcurves) come back with the kernel's
/// own trims: on the square sheet, the very lines its record had.
#[test]
fn bare_uses_come_back_with_the_kernels_trims() {
    use solidcore_convert::brep::to_kernel_with_bare_uses;
    let mut k = kernel();
    let r = square(3.0);
    let mut placeholder = r.clone();
    let Loop::Edges { edges } = &mut placeholder.faces[0].loops[0] else {
        unreachable!()
    };
    for u in edges.iter_mut() {
        u.pcurve = Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0; 2]; 2],
        };
    }
    let bare: Vec<(u32, u32, u32)> = (0..4).map(|p| (0, 0, p)).collect();
    let (mut b, _) = to_kernel_with_bare_uses(&mut k, &placeholder, true, &bare).unwrap();
    let (back, _) = to_record(&mut k, &mut b).unwrap();
    assert_eq!(back.faces[0].loops, r.faces[0].loops);
    let mut unsorted = bare.clone();
    unsorted.reverse();
    assert!(matches!(
        to_kernel_with_bare_uses(&mut k, &placeholder, true, &unsorted),
        Err(Unsupported::Invalid(_))
    ));
}

/// Every face the conversion builds owns its surface (`SmFace::SetSurface`: its `GetFace()` is
/// the face, under the body's owner identity), the surface reads the face's tolerance through
/// it, and every use on a face whose record pcurves are its parameters keeps its record trim
/// (solidcore#2550: an unowned surface fails any blend whose rail merges onto it with 1006,
/// and dropped trims are re-dropped outside the blend's net scope).
#[test]
fn every_face_owns_its_surface_and_keeps_its_trims() {
    use solidcore::topology::edgeuse::p1::{faceuse, mate, uv_trim_curve_pointer};
    let mut k = kernel();
    let mut made = primitives::create_cylinder(&mut k, Vec3::new(1.0, 2.0, 3.0), 4.0, 5.0).unwrap();
    let (r, _) = to_record(&mut k, &mut made).unwrap();
    let (b, _) = to_kernel(&mut k, &r, true).unwrap();
    for &f in b.face_list() {
        let face = b.face(f).unwrap();
        let s = face.surface.as_ref().unwrap();
        assert_eq!(
            s.owner(),
            Some(b.owner_identity(solidcore::EntityRef::Face(f))),
            "face {f:?} owns its surface"
        );
        assert_eq!(
            s.owner_zone_tol_3d(),
            Some(face.tolerance),
            "face {f:?}'s tolerance"
        );
    }
    // the kernel keeps a mated pair's trim on one of the two
    for eu in solidcore::topology::brep::p2::edgeuses(&b, None).unwrap() {
        if faceuse(&b, eu).unwrap().is_some() {
            let held = |u| uv_trim_curve_pointer(&b, u).unwrap().is_some();
            assert!(
                held(eu) || mate(&b, eu).unwrap().is_some_and(held),
                "a use without its record trim"
            );
        }
    }
}

/// The unit cube with a square fin sheet on its bottom edge at x = 1 (from (1, 1, 0) to
/// (1, 0, 0)), in the plane z = 0 out to x = 2: a non-manifold body, the edge used by the
/// bottom, the fin and the right face in that radial order about its direction (-y).
fn cube_with_fin() -> Record {
    let mut r = cube(1.0);
    let vertex =
        |r: &Record, p: [f64; 3]| r.vertices.iter().position(|v| v.point == p).unwrap() as u32;
    let (c1, c2) = (vertex(&r, [1., 0., 0.]), vertex(&r, [1., 1., 0.]));
    let shared = r.edges.iter().position(|e| e.vertices == [c2, c1]).unwrap();
    let (v8, v9) = (r.vertices.len() as u32, r.vertices.len() as u32 + 1);
    for point in [[2., 0., 0.], [2., 1., 0.]] {
        r.vertices.push(Vertex {
            point,
            tolerance: 1e-6,
        });
    }
    let fin = r.faces.len() as u32;
    let corners = [[1., 0., 0.], [2., 0., 0.], [2., 1., 0.], [1., 1., 0.]];
    let ids = [c1, v8, v9, c2];
    let uv = |p: [f64; 3]| [p[0] - 1.0, p[1]];
    let mut loop_edges = Vec::new();
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let edge = if i == 3 {
            r.edges[shared].uses.insert(
                1,
                EdgeUseRef {
                    face: fin,
                    r#loop: 0,
                    position: 3,
                    upper: false,
                },
            );
            shared as u32
        } else {
            r.edges.push(Edge {
                nurbs: None,
                curve: Curve::Line { start: a, end: b },
                interval: [0.0, 1.0],
                vertices: [ids[i], ids[(i + 1) % 4]],
                tolerance: 1e-6,
                uses: vec![EdgeUseRef {
                    face: fin,
                    r#loop: 0,
                    position: i as u32,
                    upper: false,
                }],
            });
            r.edges.len() as u32 - 1
        };
        loop_edges.push(OrientedEdge {
            edge,
            forward: true,
            pcurve: Curve2::Spline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: vec![uv(a), uv(b)],
            },
        });
    }
    r.faces.push(Face {
        nurbs: None,
        surface: Surface::Plane {
            origin: [1.0, 0.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
        },
        domain: [[0.0, 1.0], [0.0, 1.0]],
        loops: vec![Loop::Edges { edges: loop_edges }],
    });
    r.shells.push(Shell::Faces {
        faces: vec![OrientedFace {
            face: fin,
            forward: true,
        }],
        wires: vec![],
    });
    r.regions[0].shells.push(1);
    r
}

/// B3: a sheet attached to a solid along an edge is refused going out (`NotYet`), not
/// written as a sheet beside the solid on its own copy of the edge, which would detach it.
#[test]
fn a_sheet_on_a_solids_edge_is_refused_not_detached() {
    let mut k = kernel();
    let r = cube_with_fin();
    r.validate_built().unwrap();
    let (mut b, order) = to_kernel(&mut k, &r, true).unwrap();
    order_is_total(&order, &r, &b);
    assert_eq!(b.face_list().len(), 7);
    match to_record(&mut k, &mut b) {
        Err(Unsupported::NotYet(w)) => assert!(w.contains("sheet"), "{w}"),
        other => panic!("not refused: {:?}", other.map(|(r, _)| r.edges.len())),
    }
}

/// `r`'s edge `e` split at `at` (a point on it) into two edges, the second from `at` to the
/// old end, every use of `e` followed (or, reversed, preceded) by one of the new edge in its
/// loop; every use's position renumbered from the loops.
fn split_edge(r: &mut Record, e: usize, at: [f64; 3]) {
    let m = r.vertices.len() as u32;
    r.vertices.push(Vertex {
        point: at,
        tolerance: 1e-6,
    });
    let Curve::Line { start, end } = r.edges[e].curve.clone() else {
        panic!("a line")
    };
    let second = r.edges.len() as u32;
    let mut tail = r.edges[e].clone();
    tail.curve = Curve::Line { start: at, end };
    tail.vertices = [m, r.edges[e].vertices[1]];
    r.edges[e].curve = Curve::Line { start, end: at };
    r.edges[e].vertices[1] = m;
    r.edges.push(tail);
    for face in &mut r.faces {
        for l in &mut face.loops {
            let Loop::Edges { edges } = l else { continue };
            let Some(i) = edges.iter().position(|u| u.edge == e as u32) else {
                continue;
            };
            let u = edges[i].clone();
            let Curve2::Spline { points, .. } = &u.pcurve else {
                panic!("a line pcurve")
            };
            let (p0, p1) = (points[0], points[1]);
            // the pcurve's point at `at`, by the fraction along the edge (along its longest
            // coordinate: a cube's edge runs along one axis)
            let axis = (0..3)
                .max_by(|&a, &b| {
                    (end[a] - start[a])
                        .abs()
                        .total_cmp(&(end[b] - start[b]).abs())
                })
                .unwrap();
            let f = (at[axis] - start[axis]) / (end[axis] - start[axis]);
            let f = if u.forward { f } else { 1.0 - f };
            let pm = [p0[0] + f * (p1[0] - p0[0]), p0[1] + f * (p1[1] - p0[1])];
            let line = |a: [f64; 2], b: [f64; 2]| Curve2::Spline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: vec![a, b],
            };
            let (first, last) = (line(p0, pm), line(pm, p1));
            if u.forward {
                edges[i].pcurve = first;
                edges.insert(
                    i + 1,
                    OrientedEdge {
                        edge: second,
                        forward: true,
                        pcurve: last,
                    },
                );
            } else {
                edges[i].pcurve = last;
                edges.insert(
                    i,
                    OrientedEdge {
                        edge: second,
                        forward: false,
                        pcurve: first,
                    },
                );
            }
        }
    }
    // canonical order: edges by first appearance through the faces' loops, vertices through
    // the edges
    let mut edge_new: Vec<Option<u32>> = vec![None; r.edges.len()];
    let mut next = 0u32;
    for face in &r.faces {
        for l in &face.loops {
            let Loop::Edges { edges } = l else { continue };
            for u in edges {
                if edge_new[u.edge as usize].is_none() {
                    edge_new[u.edge as usize] = Some(next);
                    next += 1;
                }
            }
        }
    }
    let mut old_edges: Vec<Option<Edge>> = r.edges.drain(..).map(Some).collect();
    let mut by_new: Vec<(u32, usize)> = edge_new
        .iter()
        .enumerate()
        .map(|(o, n)| (n.unwrap(), o))
        .collect();
    by_new.sort_unstable();
    r.edges = by_new
        .iter()
        .map(|&(_, o)| old_edges[o].take().unwrap())
        .collect();
    for face in &mut r.faces {
        for l in &mut face.loops {
            let Loop::Edges { edges } = l else { continue };
            for u in edges {
                u.edge = edge_new[u.edge as usize].unwrap();
            }
        }
    }
    let mut vertex_new: Vec<Option<u32>> = vec![None; r.vertices.len()];
    let mut next = 0u32;
    for edge in &r.edges {
        for v in edge.vertices {
            if vertex_new[v as usize].is_none() {
                vertex_new[v as usize] = Some(next);
                next += 1;
            }
        }
    }
    let mut old_vertices: Vec<Option<Vertex>> = r.vertices.drain(..).map(Some).collect();
    let mut by_new: Vec<(u32, usize)> = vertex_new
        .iter()
        .enumerate()
        .map(|(o, n)| (n.unwrap(), o))
        .collect();
    by_new.sort_unstable();
    r.vertices = by_new
        .iter()
        .map(|&(_, o)| old_vertices[o].take().unwrap())
        .collect();
    for edge in &mut r.edges {
        for v in &mut edge.vertices {
            *v = vertex_new[*v as usize].unwrap();
        }
    }
    let mut at_of = std::collections::HashMap::new();
    for (fi, face) in r.faces.iter().enumerate() {
        for (li, l) in face.loops.iter().enumerate() {
            let Loop::Edges { edges } = l else { continue };
            for (pi, u) in edges.iter().enumerate() {
                at_of.insert((u.edge, fi as u32), (li as u32, pi as u32));
            }
        }
    }
    for (ei, edge) in r.edges.iter_mut().enumerate() {
        for u in &mut edge.uses {
            let (l, p) = at_of[&(ei as u32, u.face)];
            u.r#loop = l;
            u.position = p;
        }
    }
}

/// B2: the order of an untrusted record whose topology the healer changes is not known: the
/// cube with one edge split 1e-8 from its end (an edge shorter than its tolerance, which the
/// healer removes) is refused with `NotYet` where an order is asked for, and built as its
/// healed body where it is not. Trusted, the same record keeps its order (no healer).
#[test]
fn a_healed_record_whose_topology_the_healer_changed_has_no_order() {
    use solidcore_convert::brep::to_kernel_body;
    let mut k = kernel();
    let mut r = cube(2.0);
    let e = r
        .edges
        .iter()
        .position(|e| matches!(e.curve, Curve::Line { start: [0., 0., 0.], end } if end == [2., 0., 0.] || end == [0., 2., 0.] || end == [0., 0., 2.]))
        .unwrap();
    let Curve::Line { end, .. } = r.edges[e].curve.clone() else {
        unreachable!()
    };
    let at = end.map(|x| if x == 2.0 { 2.0 - 1e-8 } else { x });
    split_edge(&mut r, e, at);
    r.validate_built().unwrap();
    let (b, order) = to_kernel(&mut k, &r, true).unwrap();
    order_is_total(&order, &r, &b);
    match to_kernel(&mut k, &r, false) {
        Err(Unsupported::NotYet(w)) => assert!(w.contains("healer"), "{w}"),
        Err(e) => panic!("refused: {e}"),
        Ok((b, _)) => panic!(
            "an order after healing: {} edges for the record's {}",
            b.edge_list().len(),
            r.edges.len()
        ),
    }
    let healed = to_kernel_body(&mut k, &r, false).unwrap();
    assert_eq!(
        healed.edge_list().len(),
        12,
        "the healer removed the short edge"
    );
}

// ---- #2357: a ball patch face keeps its sphere only once its net and loop are certified on it ----

/// A 40 x 30 x 60 box at `o`, every edge rounded r 1.5: eight Ball corners.
fn rounded_box(k: &mut Kernel, o: [f64; 3]) -> Brep {
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
    use solidcore::query::api_queries::get_edges;
    use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
    let mut b = primitives::create_box(k, Vec3::new(o[0], o[1], o[2]), 40.0, 30.0, 60.0).unwrap();
    let edges = get_edges(&b).unwrap();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &[1.5; 12], &xs, 1, 1.0)
            .unwrap();
        s.do_fillet(k).unwrap();
    }
    stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b
}

/// The record's ball patch faces, by index.
fn ball_faces(r: &Record) -> Vec<usize> {
    (0..r.faces.len())
        .filter(|&i| matches!(r.faces[i].surface, Surface::BallPatch { .. }))
        .collect()
}

/// A record face's net, its homogeneous control points as bits.
fn record_net_bits(f: &Face) -> Vec<[u64; 4]> {
    let Some(Surface::RationalSpline { homogeneous, .. }) = &f.nurbs else {
        panic!("a ball patch face on a rational net");
    };
    homogeneous
        .iter()
        .flatten()
        .map(|h| h.map(f64::to_bits))
        .collect()
}

/// A kernel net's homogeneous control points as bits, as the record stores them.
fn kernel_net_bits(net: &solidcore::nurbs::b_spline_surface::BSplineSurface) -> Vec<[u64; 4]> {
    use solidcore::nurbs::math::NOW;
    let n = net.gw_nurb().unwrap();
    n.rows()
        .flatten()
        .map(|c| [c.x, c.y, c.z, if c.w == NOW { 1.0 } else { c.w }].map(f64::to_bits))
        .collect()
}

/// What record face `i` of `r` reads as: `Some` (centre, radius, net bits) for a ball patch,
/// `None` with the net's bits checked equal to the record's for a face read as its NURBS.
fn read_ball(k: &mut Kernel, r: &Record, i: usize) -> Option<([f64; 3], f64, Vec<[u64; 4]>)> {
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::face::p2 as face2;
    let (back, order) = to_kernel(k, r, true).unwrap();
    let s = face2::surface(&back, order.faces[i]).unwrap().unwrap();
    match s.kind() {
        SurfaceKind::BallPatch(bp) => {
            let c = bp.centre();
            Some(([c.x, c.y, c.z], bp.radius(), kernel_net_bits(bp.net())))
        }
        SurfaceKind::BSpline(net) => {
            assert_eq!(
                kernel_net_bits(net),
                record_net_bits(&r.faces[i]),
                "face {i}"
            );
            None
        }
        other => panic!("face {i} read as {other:?}"),
    }
}

/// Moves the homogeneous points of a NURBS record curve by `d` (times each weight); false for a
/// curve that is not one.
fn move_curve(c: &mut Curve, d: f64) -> bool {
    match c {
        Curve::RationalSpline { homogeneous, .. } => {
            for h in homogeneous.iter_mut() {
                let w = h[3];
                for x in &mut h[..3] {
                    *x += d * w;
                }
            }
            true
        }
        Curve::Circle { frame, .. } | Curve::Ellipse { frame, .. } => {
            for x in &mut frame.origin {
                *x += d;
            }
            true
        }
        Curve::Spline { points, .. } => {
            for p in points.iter_mut() {
                for x in p.iter_mut() {
                    *x += d;
                }
            }
            true
        }
        _ => false,
    }
}

/// The eight corners read back as ball patches on their record's sphere and net, bit for bit;
/// each centre r in from its three faces, R = 1.5.
#[test]
fn a_certified_ball_patch_reads_as_one() {
    let mut k = kernel();
    let mut b = rounded_box(&mut k, [0.0; 3]);
    let (r, _) = to_record(&mut k, &mut b).unwrap();
    let balls = ball_faces(&r);
    assert_eq!(balls.len(), 8);
    let (back, order) = to_kernel(&mut k, &r, true).unwrap();
    for i in balls {
        let Surface::BallPatch { center, radius } = r.faces[i].surface else {
            unreachable!()
        };
        let s = solidcore::topology::face::p2::surface(&back, order.faces[i])
            .unwrap()
            .unwrap();
        let solidcore::surface::surface::SurfaceKind::BallPatch(bp) = s.kind() else {
            panic!("face {i} read as {:?}", s.kind());
        };
        let c = bp.centre();
        assert_eq!([c.x, c.y, c.z].map(f64::to_bits), center.map(f64::to_bits));
        assert_eq!(bp.radius().to_bits(), radius.to_bits());
        assert_eq!(radius, 1.5);
        assert_eq!(kernel_net_bits(bp.net()), record_net_bits(&r.faces[i]));
        for (x, side) in [(c.x, 40.0), (c.y, 30.0), (c.z, 60.0)] {
            let near = (x - 1.5).abs().min((x - (side - 1.5)).abs());
            assert!(near <= 1e-12, "face {i}: {c:?}");
        }
    }
}

/// A sphere 1e-9 larger than the net, a net point moved 1e-7, a loop curve moved 1e-7: each
/// corner reads as its exact NURBS with no ball patch label, and the body is still read.
#[test]
fn a_ball_patch_off_its_sphere_reads_as_its_nurbs() {
    let mut k = kernel();
    let mut b = rounded_box(&mut k, [0.0; 3]);
    let (r, _) = to_record(&mut k, &mut b).unwrap();
    let balls = ball_faces(&r);
    assert_eq!(balls.len(), 8);
    for &i in &balls {
        let mut off = r.clone();
        if let Surface::BallPatch { radius, .. } = &mut off.faces[i].surface {
            *radius += 1e-9;
        }
        assert!(
            read_ball(&mut k, &off, i).is_none(),
            "face {i}: larger sphere"
        );

        let mut off = r.clone();
        if let Some(Surface::RationalSpline { homogeneous, .. }) = &mut off.faces[i].nurbs {
            let (u, v) = (homogeneous.len() / 2, homogeneous[0].len() / 2);
            let h = &mut homogeneous[u][v];
            h[0] += 1e-7 * h[3];
        }
        assert!(read_ball(&mut k, &off, i).is_none(), "face {i}: net moved");

        let mut off = r.clone();
        let Loop::Edges { edges } = &off.faces[i].loops[0] else {
            panic!("face {i}: an edge loop");
        };
        let e = &mut off.edges[edges[0].edge as usize];
        // the curve and its NURBS form move together, so the record stays consistent and the
        // loop curve lies 1e-7 off the sphere
        let moved = match &mut e.nurbs {
            Some(n) => move_curve(n, 1e-7) && move_curve(&mut e.curve, 1e-7),
            None => move_curve(&mut e.curve, 1e-7),
        };
        assert!(moved, "face {i}: a loop curve that can be moved");
        assert!(read_ball(&mut k, &off, i).is_none(), "face {i}: loop moved");
    }
    // unchanged, every corner still reads as its ball patch
    for &i in &balls {
        assert!(read_ball(&mut k, &r, i).is_some(), "face {i}");
    }
}

/// Far from the origin the certificate is as tight: the corners of a box at
/// (1e5, -7e4, 3e4) read as ball patches, and a sphere 1e-9 larger there reads as the NURBS.
#[test]
fn a_ball_patch_far_from_the_origin_reads_as_one() {
    let mut k = kernel();
    let mut b = rounded_box(&mut k, [1e5, -7e4, 3e4]);
    let (r, _) = to_record(&mut k, &mut b).unwrap();
    let balls = ball_faces(&r);
    assert_eq!(balls.len(), 8);
    for &i in &balls {
        let (c, radius, _) = read_ball(&mut k, &r, i).unwrap_or_else(|| panic!("face {i}"));
        assert_eq!(radius, 1.5);
        assert!(
            c[0] > 9.9e4 && c[1] < -6.9e4 && c[2] > 2.9e4,
            "face {i}: {c:?}"
        );
        let mut off = r.clone();
        if let Surface::BallPatch { radius, .. } = &mut off.faces[i].surface {
            *radius += 1e-9;
        }
        assert!(read_ball(&mut k, &off, i).is_none(), "face {i}");
    }
}

/// A record whose body tolerance is above its edges' and vertices' builds with the body's
/// lowered to the least of them (the flat import's `own_tolerances`): valid at Level 2, and
/// every edge and vertex at least the body's (invariant 5 of `docs/validity-and-tolerances.md`
/// §10), each at its own (the zone, half the record's), none raised.
#[test]
fn a_body_tolerance_above_its_entities_comes_down_to_theirs() {
    let mut k = kernel();
    let mut r = cube(10.0);
    r.tolerance = 1e-4;
    r.edges[3].tolerance = 4e-6;
    r.vertices[5].tolerance = 2e-6;
    r.validate().unwrap();
    let (mut b, _) = to_kernel(&mut k, &r, true).unwrap();
    valid(&mut k, &mut b);
    // the least is the other edges' 1e-6, as a zone: half of it
    assert_eq!(b.tolerance(), 5e-7);
    for &e in b.edge_list() {
        assert!(b.edge(e).unwrap().tolerance >= b.tolerance(), "edge {e:?}");
    }
    for &v in b.vertex_list() {
        assert!(
            b.vertex(v).unwrap().tolerance >= b.tolerance(),
            "vertex {v:?}"
        );
    }
    let mut own: Vec<f64> = b
        .edge_list()
        .iter()
        .map(|&e| b.edge(e).unwrap().tolerance)
        .collect();
    own.sort_by(f64::total_cmp);
    own.dedup();
    assert_eq!(own, vec![5e-7, 2e-6], "edges keep their own zones");
}

/// A tolerance so small that its zone (half of it) underflows to zero passes the record's
/// validation and is refused by the conversion, not dropped to the body's.
#[test]
fn a_tolerance_whose_zone_underflows_is_refused() {
    let mut k = kernel();
    let mut r = cube(1.0);
    r.vertices[0].tolerance = f64::from_bits(1);
    assert!(matches!(
        to_kernel(&mut k, &r, true),
        Err(Unsupported::Invalid(_))
    ));
}

/// Timings of an M20×2.5 external thread, 4 and 10 turns: the groove's build, the cut, the
/// record both ways (opening a saved body is `to_kernel`, the median of eleven), the record's
/// evaluators (every face's and edge's, as display builds them; the median of eleven) and a
/// closest point over every edge of the body just opened. A bench: it prints, and asserts
/// only that each step succeeds and the record's volume is the body's. `cargo test --profile
/// goldens -p solidcore-convert --test brep -- --ignored --nocapture m20_thread_timings`.
#[test]
#[ignore = "a bench: prints timings"]
#[allow(clippy::disallowed_methods)] // Test-only fixture.
fn m20_thread_timings() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::curve::curve::Curve as KernelCurve;
    use solidcore::geom::tol::{brep_approx_tol_3d, SmallTopoType};
    use solidcore::math::Point3;
    use solidcore::primitive::api_curves::create_line_segment;
    use solidcore::query::api_queries::edge_closest_point;
    use solidcore::sweep::screw::screw_sweep;
    use std::time::Instant;

    for turns in [4.0, 10.0] {
        let mut k = kernel();
        let pitch = 2.5_f64;
        let minor = 10.0 - 5.0 * pitch * 3.0_f64.sqrt() / 16.0;
        let high = 10.0 + 0.05 * pitch;
        let half_low = pitch / 8.0;
        let half_high = half_low + (high - minor) / 3.0_f64.sqrt();
        let points = [
            Point3::new(minor, 0.0, -half_low),
            Point3::new(high, 0.0, -half_high),
            Point3::new(high, 0.0, half_high),
            Point3::new(minor, 0.0, half_low),
        ];
        let profile: Vec<KernelCurve> = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(&a, &b)| KernelCurve::from(create_line_segment(a, b).unwrap()))
            .collect();
        let length = pitch * (turns - 1.0);
        let stock =
            primitives::create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, length).unwrap();
        let tolerance = brep_approx_tol_3d(&k, &stock, SmallTopoType::NormalSize);
        let t = Instant::now();
        let mut groove = screw_sweep(
            &mut k,
            &profile,
            Point3::new(0.0, 0.0, 0.0),
            Vec3::Z,
            pitch,
            turns,
            tolerance,
        )
        .unwrap();
        let build = t.elapsed();
        let groove_volume = volume(&k, &mut groove);
        let t = Instant::now();
        let mut body = boolean_difference(&mut k, stock, groove).unwrap();
        let cut = t.elapsed();
        let body_volume = volume(&k, &mut body);
        let t = Instant::now();
        let (record, _) = to_record(&mut k, &mut body).unwrap();
        let write = t.elapsed();
        // the median of eleven opens
        let mut opens = Vec::new();
        for _ in 0..11 {
            let t = Instant::now();
            let _ = to_kernel(&mut k, &record, true).unwrap();
            opens.push(t.elapsed());
        }
        opens.sort();
        let open = opens[5];
        let (mut back, _) = to_kernel(&mut k, &record, true).unwrap();
        let back_volume = volume(&k, &mut back);
        let mut evals = Vec::new();
        for _ in 0..11 {
            let t = Instant::now();
            for f in &record.faces {
                solidcore::eval::SurfaceEval::new(&solidcore_convert::surface(&f.surface).unwrap())
                    .unwrap();
            }
            let context = solidcore_convert::curve_context::BodyCurveContext::new(&record).unwrap();
            for i in 0..record.edges.len() {
                context.edge(i).unwrap();
            }
            evals.push(t.elapsed());
        }
        evals.sort();
        let evaluators = evals[5];
        assert!((back_volume - body_volume).abs() <= 1e-9 * body_volume);
        let q = Vec3::new(-9.0, 0.5, 0.4 * length);
        let t = Instant::now();
        let mut nearest = f64::INFINITY;
        for &e in back.edge_list() {
            let d = edge_closest_point(&k, &back, e, q)
                .unwrap()
                .closest
                .distance;
            nearest = nearest.min(d);
        }
        let closest = t.elapsed();
        eprintln!(
            "M20 turns {turns}: groove {groove_volume:.6} body {body_volume:.6} faces {} edges {} | \
             build {build:?} cut {cut:?} to_record {write:?} to_kernel {open:?} evaluators {evaluators:?} \
             closest-over-{}-edges {closest:?} (nearest {nearest:.9})",
            back.face_list().len(),
            back.edge_list().len(),
            back.edge_list().len(),
        );
    }
}
