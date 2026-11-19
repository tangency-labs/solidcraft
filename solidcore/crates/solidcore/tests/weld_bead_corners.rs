//! Weld beads meeting at a vertex (#17916, the welds epic #15132): walls on a floor, a bead on
//! each wall's foot. At an inside corner the beads are joined as they are; below 90 degrees each
//! is cut at the other's wall first. At an outside corner the first bead's end section is
//! turned about the floor's normal through the corner to the second's (a wrap-around). Three
//! beads at an inside corner (two on the floor, one up the walls' edge) are joined as they are.
//! The part is never changed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::weld_bead::{bead_body, BeadEnd};
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::surface::procedural_blend::BlendRef;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use std::f64::consts::PI;

/// The legs of every bead (mm).
const L: f64 = 5.0;
/// The walls' length along the floor (mm).
const WALL: f64 = 18.0;
/// The walls' height (mm).
const HIGH: f64 = 12.0;
/// The walls' thickness (mm).
const THICK: f64 = 4.0;
const SQUARE: [BeadEnd; 2] = [BeadEnd::Square; 2];

/// Every fix on but `geometric_g2_continuity`, as ops runs.
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

fn smb(k: &Kernel, b: &Brep) -> Vec<u8> {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &|e| b.owner_identity(e),
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, true).unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn pick(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, p) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

fn dir(deg: f64) -> Vec3 {
    let a = deg.to_radians();
    Vec3::new(a.cos(), a.sin(), 0.0)
}

/// Walls `HIGH` high on a 50 x 50 x 4 floor, their plan the polygon `pts`.
fn walls(k: &mut Kernel, pts: &[(f64, f64)]) -> Brep {
    let curves: Vec<Curve> = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            Curve::from(
                create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let w = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), HIGH, true).unwrap();
    let f = create_box(k, Point3::new(-25.0, -25.0, -4.0), 50.0, 50.0, 4.0).unwrap();
    boolean_union(k, f, w).unwrap()
}

/// Two walls whose inner faces leave the origin along 0 and `theta` degrees: the inside
/// corner between them, where the beads go.
fn inside(k: &mut Kernel, theta: f64) -> Brep {
    let t = theta.to_radians();
    let d = (t.cos(), t.sin());
    let x = -(THICK + THICK * t.cos()) / t.sin();
    let mut p = vec![
        (0.0, 0.0),
        (WALL * d.0, WALL * d.1),
        (WALL * d.0 - THICK * d.1, WALL * d.1 + THICK * d.0),
        (x, -THICK),
        (WALL, -THICK),
        (WALL, 0.0),
    ];
    p.reverse();
    walls(k, &p)
}

/// Two walls whose outer faces leave the origin along 0 and `theta` degrees, the material
/// between them: the outside corner, where the beads go.
fn outside(k: &mut Kernel, theta: f64) -> Brep {
    let t = theta.to_radians();
    let d = (t.cos(), t.sin());
    let x = (THICK + THICK * t.cos()) / t.sin();
    let p = vec![
        (0.0, 0.0),
        (WALL, 0.0),
        (WALL, THICK),
        (x, THICK),
        (WALL * d.0 + THICK * d.1, WALL * d.1 - THICK * d.0),
        (WALL * d.0, WALL * d.1),
    ];
    walls(k, &p)
}

/// The floor edges at the walls' feet, and with `up` the walls' edge at the origin.
fn joints(k: &Kernel, part: &Brep, theta: f64, up: bool) -> Vec<EdgeId> {
    let mut e = vec![
        pick(k, part, Point3::new(WALL / 2.0, 0.0, 0.0)),
        pick(
            k,
            part,
            Point3::new(0.0, 0.0, 0.0) + dir(theta) * (WALL / 2.0),
        ),
    ];
    if up {
        e.push(pick(k, part, Point3::new(0.0, 0.0, HIGH / 2.0)));
    }
    e
}

/// The bead body on `edges` of `part` at section `h`: sound, the part's file unchanged, and
/// each bead's section face still carrying the foot blend's definition after the joins.
fn beads(k: &mut Kernel, part: &mut Brep, edges: &[EdgeId], h: f64) -> Brep {
    let before = smb(k, part);
    let size = ChamferSize::Distances([L; 2]);
    let sizes = vec![size; edges.len()];
    let mut body = bead_body(k, part, edges, &sizes, h, &[], &[], SQUARE).unwrap();
    assert!(smb(k, part) == before, "the part changed");
    let c = check_brep(k, &mut body, CheckLevel::Level2).unwrap();
    assert_eq!(c.failures().count(), 0, "{c:?}");
    let defined = body
        .face_list()
        .iter()
        .filter(|&&f| {
            body.face(f)
                .ok()
                .and_then(|f| f.surface.clone())
                .is_some_and(|s| matches!(s.procedural_blend(), Some(BlendRef::FootBlend { .. })))
        })
        .count();
    assert_eq!(defined, edges.len(), "section faces with their definition");
    body
}

/// A flat bead's section, the triangle of its legs.
const FLAT: f64 = L * L / 2.0;

/// Two flat beads in a square inside corner: their sections along the walls, the overlap at
/// the corner (a square pyramid, L^3 / 3) counted once.
#[test]
fn flat_beads_in_a_square_inside_corner_join() {
    let mut k = kernel();
    let mut part = inside(&mut k, 90.0);
    let edges = joints(&k, &part, 90.0, false);
    let mut body = beads(&mut k, &mut part, &edges, 0.0);
    let want = 2.0 * FLAT * WALL - L.powi(3) / 3.0;
    let got = volume(&k, &mut body);
    assert!((got - want).abs() < 1e-6 * want, "{got} {want}");
}

/// Three flat beads at a square inside corner, one up the walls' edge: inclusion and
/// exclusion over the beads, each pair's overlap L^3 / 3, all three's L^3 / 4
/// (`x, y, z >= 0`, each two of them summing to at most L).
#[test]
fn three_flat_beads_at_a_square_inside_corner_join() {
    let mut k = kernel();
    let mut part = inside(&mut k, 90.0);
    let edges = joints(&k, &part, 90.0, true);
    let mut body = beads(&mut k, &mut part, &edges, 0.0);
    let l3 = L.powi(3);
    let want = FLAT * (2.0 * WALL + HIGH) - 3.0 * l3 / 3.0 + l3 / 4.0;
    let got = volume(&k, &mut body);
    assert!((got - want).abs() < 1e-6 * want, "{got} {want}");
}

/// Two flat beads in a 60 degree inside corner: each runs into the other's wall and is cut
/// there, keeping its section along the wall less the wedge behind the other wall,
/// `cot(theta) L^3 / 6`. They overlap where both cover the floor: in the distances `y`, `s` to
/// the two walls (area `dy ds / sin(theta)`), height `L - max(y, s)`, `L^3 / (3 sin(theta))`.
#[test]
fn flat_beads_in_a_sixty_degree_inside_corner_are_cut_at_the_walls() {
    let mut k = kernel();
    let theta: f64 = 60.0;
    let mut part = inside(&mut k, theta);
    let edges = joints(&k, &part, theta, false);
    let mut body = beads(&mut k, &mut part, &edges, 0.0);
    let t = theta.to_radians();
    let each = FLAT * WALL - L.powi(3) / (6.0 * t.tan());
    let want = 2.0 * each - L.powi(3) / (3.0 * t.sin());
    let got = volume(&k, &mut body);
    assert!((got - want).abs() < 1e-6 * want, "{got} {want}");
}

/// Two flat beads round a square outside corner: their sections along the walls and the
/// wrap-around, the end section turned a quarter turn about the corner's upright (Pappus: the
/// triangle's area times its centroid's path, L / 3 from the axis).
#[test]
fn flat_beads_round_a_square_outside_corner_wrap() {
    let mut k = kernel();
    let mut part = outside(&mut k, 90.0);
    let edges = joints(&k, &part, 90.0, false);
    let mut body = beads(&mut k, &mut part, &edges, 0.0);
    let want = 2.0 * FLAT * WALL + FLAT * (L / 3.0) * (PI / 2.0);
    let got = volume(&k, &mut body);
    assert!((got - want).abs() < 1e-6 * want, "{got} {want}");
}

/// Round a 60 and a 120 degree outside corner the wrap-around turns 180 degrees less the
/// walls' angle; convex and concave beads wrap too, and every body is sound.
#[test]
fn beads_wrap_round_outside_corners_at_any_angle() {
    for theta in [60.0_f64, 120.0] {
        for h in [0.0, 1.0, -0.5] {
            let mut k = kernel();
            let mut part = outside(&mut k, theta);
            let edges = joints(&k, &part, theta, false);
            let mut body = beads(&mut k, &mut part, &edges, h);
            if h == 0.0 {
                let turn = (180.0 - theta).to_radians();
                let want = 2.0 * FLAT * WALL + FLAT * (L / 3.0) * turn;
                let got = volume(&k, &mut body);
                assert!((got - want).abs() < 1e-6 * want, "{theta} {got} {want}");
            }
        }
    }
}

/// Convex and concave beads join in inside corners of 60, 90 and 120 degrees, two on the floor
/// or three with one up the walls' edge (three at 90 and 120 only), and every body is sound.
#[test]
fn convex_and_concave_beads_join_in_inside_corners() {
    for theta in [60.0_f64, 90.0, 120.0] {
        for h in [1.0, -0.5] {
            for up in [false, true] {
                if up && theta < 90.0 {
                    continue;
                }
                let mut k = kernel();
                let mut part = inside(&mut k, theta);
                let edges = joints(&k, &part, theta, up);
                let mut body = beads(&mut k, &mut part, &edges, h);
                assert!(volume(&k, &mut body) > 0.0);
            }
        }
    }
}

/// Beads of different legs round an outside corner are refused naming the corner's vertex
/// (the wrap-around turns one section into the other), and the part is unchanged.
#[test]
fn different_beads_round_an_outside_corner_are_refused() {
    let mut k = kernel();
    let mut part = outside(&mut k, 90.0);
    let edges = joints(&k, &part, 90.0, false);
    let before = smb(&k, &part);
    let sizes = [
        ChamferSize::Distances([L; 2]),
        ChamferSize::Distances([L, 4.0]),
    ];
    let err = bead_body(&mut k, &mut part, &edges, &sizes, 0.0, &[], &[], SQUARE)
        .err()
        .unwrap();
    assert!(err.site().and_then(|s| s.vertex).is_some(), "{err:?}");
    assert!(smb(&k, &part) == before, "the part changed");
}
