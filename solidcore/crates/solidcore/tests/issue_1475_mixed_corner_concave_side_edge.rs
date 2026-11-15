//! tangency-labs/solidcore#1475 (WR-524): a bar 100 x 30 x 10 (x 0..100, y -15..15)
//! joined with a boss r 20, height 20, on the origin (SolidCraft rig case fillets/1207).
//! The bar's top front edge (y = -15, z = 10) and the boss's concave base arc over the
//! bar (both halves, split at the cylinder's seam) are rounded r 2 together. They meet
//! at (13.229, -15, 10), where the third edge, the boss's line on the bar's front face,
//! is concave and runs down from the vertex: a 3x2 mixed-convexity corner whose
//! unfilleted edge is concave. `SmFillet3x2MixedConvexityCorner::MakeCornerTopology`
//! gives the fillets the roles of an L-shaped box (convex unfilleted edge) and fails
//! with status 1001. solidcore lets the roles follow the unfilleted edge's convexity, as
//! tangency-labs/smlib@1255993 (once the opt-in `fillet_3x2_mixed_concave_side_edge`,
//! now the only path).
//!
//! The operand is the C++ union's body as SolidCraft hands it over (its flat arrays,
//! written with `BrepData::write_to_file`), read back with SolidCraft's `BrepFromFlat`
//! options; solidcore's own boolean builds a different body (see the issue).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::{
    make_topology_from_data, MakeTopologyOptions, TopologyEnv, HEALER_VERSION,
};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId};
use solidcore::{EntityRef, Kernel};

const BODY: &[u8] = include_bytes!("data/issue_1475_bar_and_boss.smd");

/// SolidCraft's topology identities (type code, entity index).
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(((kind + 1) << 32) | u64::from(i)),
    }
}

/// The body as SolidCraft's `BrepFromFlat` builds it: no UV trims, no analytics, same
/// tolerances, healer on.
fn load(k: &mut Kernel) -> Brep {
    let mut data = BrepData::read_from_file(k, BODY.to_vec(), FileType::Ascii).unwrap();
    data.set_healer_version(HEALER_VERSION);
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut store = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let next_key = (data.uv_curves.len() + data.curves_3d.len() + data.surfaces.len()) as u64 + 1;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key,
    };
    let options = MakeTopologyOptions {
        make_uv_trim_curves: false,
        add_analytics: false,
        force_same_tol: true,
        check_uv_trim_curves: false,
        always_heal: false,
        healer_enabled: true,
    };
    make_topology_from_data(k, &mut b, &mut data, &mut env, options).unwrap();
    b
}

/// The edge nearest `p` (SolidCraft's pick: its points are display-mesh points, off the arc).
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let mut best = (f64::MAX, None);
    for e in bp1::edges(b, None).unwrap() {
        if let Ok(c) = edge_closest_point(k, b, e, Vec3::new(p[0], p[1], p[2])) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    assert!(best.0 < 0.5, "no edge at {p:?}");
    best.1.unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// Rounds the bar's top front edge and both halves of the boss's base arc as
/// `SmApiFilletEdgesPerEdge` does; `Err` carries the fillet error info code.
fn round(radius: f64) -> (Kernel, Brep, Result<(), (solidcore::SolidError, i32)>) {
    let mut k = Kernel::default();
    let mut b = load(&mut k);
    let edges: Vec<EdgeId> = [
        [56.614_378_277_661_47, -15.0, 10.0],
        [14.793_060_209_241_245, 13.254_056_122_601_737, 10.0],
        [19.849_045_376_133_382, -2.343_832_202_869_714, 10.0],
    ]
    .iter()
    .map(|&p| pick(&k, &b, p))
    .collect();
    let r = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        let step = s
            .prepare_per_edge_constant_radius(
                &mut k,
                &edges,
                &[radius; 3],
                &[FilletSurfaceGeneratorType::Circular],
                1,
                1.0,
            )
            .and_then(|()| s.create_corners(&k, None))
            .and_then(|()| s.do_filleting(&mut k, None));
        let code = s.error_info().code;
        step.map_err(|e| (e, code))
    };
    b.refresh_cached_topology();
    if r.is_ok() {
        stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
        b.refresh_cached_topology();
    }
    (k, b, r)
}

/// The unrounded body: the bar, the boss, less the half-disk of the boss over the bar
/// (|y| <= 15, x >= 0) through the bar's height.
fn body_volume() -> f64 {
    let pi = core::f64::consts::PI;
    let overlap = 10.0 * (15.0 * 175f64.sqrt() + 400.0 * 0.75f64.asin());
    100.0 * 30.0 * 10.0 + pi * 20.0 * 20.0 * 20.0 - overlap
}

#[test]
fn operand_is_the_bar_and_boss() {
    let mut k = Kernel::default();
    let mut b = load(&mut k);
    let v = volume(&k, &mut b);
    let expected = body_volume();
    assert!((v - expected).abs() < 1e-6 * expected, "{v} vs {expected}");
}

#[test]
fn corner_builds_with_the_fix() {
    for radius in [1.0, 2.0] {
        let (k, mut b, r) = round(radius);
        r.unwrap();
        assert!(is_manifold_solid(&b).unwrap());
        // One closed shell: V - E + F = 16 - 25 + 11 = 2.
        assert_eq!(b.face_list().len(), 11);
        assert_eq!(b.edge_list().len(), 25);
        assert_eq!(b.vertex_list().len(), 16);
        if radius == 2.0 {
            // The issue's estimate: the arc round's spandrel strip ending on y = +-15 adds
            // 28.9190 (the arc round alone builds to exactly that), the front round's
            // spandrel out to the boss wall removes 74.0736; the corner patch lies inside
            // the (2r)^3 cube about the vertex.
            let v = volume(&k, &mut b);
            let estimate = body_volume() + 28.9190 - 74.0736;
            assert!(
                (v - estimate).abs() < (2.0 * radius).powi(3),
                "{v} vs {estimate}"
            );
        }
    }
}

/// A box `[origin, origin + size]`.
type BoxSpec = ([f64; 3], [f64; 3]);

/// The lower box of every two-box case: x 0..20, y 0..10, z 0..10.
const LOWER: BoxSpec = ([0.0, 0.0, 0.0], [20.0, 10.0, 10.0]);
/// A box standing on the lower one's top and overhanging its front face by 2.
const OVERHANG: BoxSpec = ([5.0, -2.0, 10.0], [10.0, 6.0, 10.0]);
/// A box sunk 5 into the lower one's top and standing out of its back face by 2.
const INSET: BoxSpec = ([5.0, 2.0, 5.0], [10.0, 10.0, 10.0]);

/// The `blend.fillet_executive_p3` goldens cases whose two rounded edges meet at a 3x2
/// mixed-convexity corner with the unfilleted edge taking the swapped roles (WR-524): the
/// upper box and the two edges, by index into `GetEdges` of the union, as the goldens
/// runner picks them.
const SWAPPED_ROLE_CASES: [(BoxSpec, [usize; 2]); 12] = [
    (OVERHANG, [15, 22]),
    (OVERHANG, [15, 24]),
    (INSET, [1, 14]),
    (INSET, [1, 18]),
    (INSET, [12, 13]),
    (INSET, [12, 17]),
    (INSET, [13, 19]),
    (INSET, [14, 27]),
    (INSET, [15, 19]),
    (INSET, [15, 27]),
    (INSET, [16, 22]),
    (INSET, [17, 22]),
];

fn inside_box(p: [f64; 3], (o, s): BoxSpec) -> bool {
    (0..3).all(|i| p[i] > o[i] && p[i] < o[i] + s[i])
}

/// The volume of the two boxes' union.
fn union_volume((ao, as_): BoxSpec, (bo, bs): BoxSpec) -> f64 {
    let overlap: f64 = (0..3)
        .map(|i| ((ao[i] + as_[i]).min(bo[i] + bs[i]) - ao[i].max(bo[i])).max(0.0))
        .product();
    as_.iter().product::<f64>() + bs.iter().product::<f64>() - overlap
}

/// A straight, axis-parallel edge of the union read from the boxes alone, not from the
/// kernel: whether it is concave, and the axis its round of radius `r` must have.
struct EdgeRound {
    concave: bool,
    /// The edge's unit direction.
    along: Vec3,
    /// A point the round's axis passes through: off the edge's midpoint by `r` in each of
    /// the two directions square to it, into the one quadrant round the edge that differs
    /// from the other three (the material of a convex edge, the empty one of a concave
    /// edge), so the axis lies `r` from both of the edge's faces.
    centre: Vec3,
}

fn unit(i: usize) -> Vec3 {
    let mut a = [0.0; 3];
    a[i] = 1.0;
    Vec3::from(a)
}

fn edge_round(a: Vec3, b: Vec3, upper: BoxSpec, r: f64) -> EdgeRound {
    let m = (a + b) * 0.5;
    let mm = [m.x, m.y, m.z];
    let d = b - a;
    let d = [d.x, d.y, d.z];
    let along = (0..3)
        .max_by(|&i, &j| d[i].abs().total_cmp(&d[j].abs()))
        .unwrap();
    assert!((0..3).all(|i| i == along || d[i] == 0.0), "{a:?} {b:?}");
    let (u, w) = ((along + 1) % 3, (along + 2) % 3);
    let mut quadrants = Vec::new();
    for su in [-1.0, 1.0] {
        for sw in [-1.0, 1.0] {
            let mut p = mm;
            p[u] += su * 1e-3;
            p[w] += sw * 1e-3;
            quadrants.push((su, sw, inside_box(p, LOWER) || inside_box(p, upper)));
        }
    }
    let inside = quadrants.iter().filter(|q| q.2).count();
    assert!(
        inside == 1 || inside == 3,
        "{a:?} {b:?}: {inside} quadrants"
    );
    let concave = inside == 3;
    // the odd quadrant: the one inside for a convex edge, outside for a concave one
    let &(su, sw, _) = quadrants.iter().find(|q| q.2 != concave).unwrap();
    EdgeRound {
        concave,
        along: unit(along),
        centre: m + unit(u) * (su * r) + unit(w) * (sw * r),
    }
}

#[test]
fn box_union_corners_with_swapped_roles_build() {
    use solidcore::blend::fillet_executive::api::fillet_edges;
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::surface::surface::p2::SurfaceKind;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use solidcore::topology::{edge, face};

    let r = 1.0_f64;
    let section = (1.0 - core::f64::consts::PI / 4.0) * r * r;
    for (upper, picks) in SWAPPED_ROLE_CASES {
        let mut k = Kernel::default();
        let boxed = |k: &mut Kernel, (o, s): BoxSpec| {
            create_box(k, Vec3::from(o), s[0], s[1], s[2]).unwrap()
        };
        let lower = boxed(&mut k, LOWER);
        let top = boxed(&mut k, upper);
        let mut b = boolean_union(&mut k, lower, top).unwrap();
        let v0 = union_volume(LOWER, upper);
        let before = volume(&k, &mut b);
        assert!((before - v0).abs() <= 1e-9 * v0, "{before} vs {v0}");
        assert_eq!(bp1::faces(&b, None).unwrap().len(), 12);
        let edges = bp1::edges(&b, None).unwrap();
        let mut selected = Vec::new();
        let mut rounds = Vec::new();
        // the straight rounds' volume: + (1 - pi/4) r^2 L for a concave edge (material
        // added), - for a convex one
        let mut prisms = 0.0;
        let mut shortest = f64::MAX;
        let mut ends = Vec::new();
        for i in picks {
            let e = edges[i];
            selected.push(e);
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            let (a, c) = (
                b.vertex(s.unwrap()).unwrap().point,
                b.vertex(t.unwrap()).unwrap().point,
            );
            let round = edge_round(a, c, upper, r);
            let sign = if round.concave { 1.0 } else { -1.0 };
            prisms += sign * section * a.distance(c);
            shortest = shortest.min(a.distance(c));
            rounds.push(round);
            ends.extend([a, c]);
        }
        // one concave and one convex edge, sharing the corner vertex
        assert!(
            rounds[0].concave != rounds[1].concave,
            "{upper:?} {picks:?}"
        );
        let shared: Vec<Vec3> = ends
            .iter()
            .copied()
            .filter(|p| ends.iter().filter(|q| q.distance(*p) < 1e-9).count() == 2)
            .collect();
        assert_eq!(shared.len(), 2, "{ends:?}");
        fillet_edges(&mut k, &mut b, &selected, r, 1, 1, 1.0).unwrap();
        assert!(is_manifold_solid(&b).unwrap());
        let report = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(report.is_valid(), "{upper:?} {picks:?}: {report:?}");
        // The topology the goldens record: 12 faces become 15 (two rounds and the corner
        // patch); the overhang's union has two vertices and two edges fewer.
        let (vertices, edges_after) = if upper == OVERHANG {
            (23, 36)
        } else {
            (25, 38)
        };
        let faces = bp1::faces(&b, None).unwrap();
        assert_eq!(
            (
                bp1::vertices(&b, None).unwrap().len(),
                bp1::edges(&b, None).unwrap().len(),
                faces.len()
            ),
            (vertices, edges_after, 15),
            "{upper:?} {picks:?}"
        );
        // Each round is a face on a cylinder of radius r whose axis runs along its edge,
        // r from both of the edge's faces, on the side the edge's convexity gives; the
        // body's other faces are none of these (the unions' faces are planes).
        let cylinders: Vec<(f64, Vec3, Vec3)> = faces
            .iter()
            .filter_map(|&f| match face::p2::surface(&b, f).unwrap()?.kind() {
                SurfaceKind::Cone(c) if c.is_cylinder() => {
                    let placement = c.canonical().origin;
                    Some((c.bot_radius(), placement.origin(), placement.z_axis()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(cylinders.len(), 2, "{upper:?} {picks:?}: {cylinders:?}");
        for round in &rounds {
            let matches = cylinders
                .iter()
                .filter(|&&(radius, origin, axis)| {
                    let axis = axis * (1.0 / axis.length());
                    let off = round.centre - origin;
                    (radius - r).abs() <= 1e-9 * r
                        && axis.cross(round.along).length() <= 1e-12
                        && (off - axis * off.dot(axis)).length() <= 1e-9
                })
                .count();
            assert_eq!(
                matches, 1,
                "{upper:?} {picks:?}: no round about {:?} along {:?} in {cylinders:?}",
                round.centre, round.along
            );
        }
        // The volume against the straight rounds' closed form. The corner patch's share is
        // not bounded here: the allowance only lies below the smallest single round's
        // (1 - pi/4) r^2 L, so a body missing either round's material by a whole round
        // fails where the two do not cancel. The residue is -0.0017 in every case.
        let v = volume(&k, &mut b);
        assert!(
            (v - v0 - prisms).abs() < 0.25 * section * shortest,
            "{upper:?} {picks:?}: {v} vs {}",
            v0 + prisms
        );
    }
}
