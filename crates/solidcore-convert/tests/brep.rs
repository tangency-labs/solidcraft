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
/// labels on theirs, its lines their two end points over [0, 1]; each pcurve on its edge's
/// interval.
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
                assert_eq!(e.interval, [0.0, 1.0]);
                assert_eq!((start[2], end[2]), (0.0, 5.0));
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
        let moved = match &mut e.nurbs {
            Some(n) => move_curve(n, 1e-7),
            None => move_curve(&mut e.curve, 1e-7),
        };
        assert!(moved, "face {i}: a loop curve with a NURBS form");
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
