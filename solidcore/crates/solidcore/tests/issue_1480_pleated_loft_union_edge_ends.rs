//! tangency-labs/solidcore#1480 (#470, WR-498): the union of two crossing pleated loft
//! skins. Where pleat ridges cross, two vertex definitions 3e-5 to 8e-5 apart fall in one
//! proto vertex, which `Resolve` makes two vertices of; the proto vertex's position is
//! their mean. With the end extension (WR-453) and the end trim (WR-452) an
//! intersection curve that already ended on its own vertex was continued to that mean,
//! leaving edge ends up to 8.06e-5 off their vertices and dropping 104 short edges
//! (992 edges, 608 vertices). The fork's ffaf4e8, folded into both end fits, sends such an
//! end to its own vertex: the C++ fork's union has 1096 edges and 712 vertices, with
//! every edge end within 1.206e-5 of its vertex.
//!
//! The operands are SolidCraft's rig recipe (booleans/4623) on solidcore alone: each is a
//! capped ruled loft minus a slightly larger one, and both are copied into their own key
//! domain before each boolean, as SolidCraft's backend does.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/volume_leak.rs"]
mod volume_leak;
use solidcore::boolean::api_brep as sm;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{p2, p3};
use solidcore::topology::Brep;
use solidcore::EntityRef;

include!("data/issue_1480_pleated_outlines.rs");

type Outline = (f64, [[f64; 2]; 48]);

fn named(entity: EntityRef, domain: u64) -> ObjectRef {
    let r = p3::topology_ref(0, entity);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
    }
}

/// `api_brep_copy` into key domain `n`, as SolidCraft copies boolean operands.
fn copy_of(k: &mut Kernel, b: &Brep, n: u64) -> Brep {
    let target = move |e| named(e, n * 100_000_000);
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target;
    p3::api_brep_copy(k, &mut env, Some(b), &|e| named(e, 0)).unwrap()
}

/// The closed outline's line segments at its height.
fn section(o: &Outline) -> Vec<Curve> {
    let (z, pts) = o;
    (0..pts.len())
        .map(|i| {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            let l = Line::create_line_segment(
                3,
                Point3::new(a[0], a[1], *z),
                Point3::new(b[0], b[1], *z),
                None,
            )
            .unwrap();
            Curve::from(l)
        })
        .collect()
}

/// A capped degree-1 skin between two outlines.
fn loft(k: &mut Kernel, lo: &Outline, hi: &Outline) -> Brep {
    let mut curves = section(lo);
    curves.extend(section(hi));
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    solidcore::sweep::api_surfaces::create_skin_primitive(k, &[48, 48], &refs, 1, true)
        .unwrap()
        .brep
}

fn boolean(k: &mut Kernel, a: &Brep, b: &Brep, op: BooleanOperation) -> Brep {
    let ca = copy_of(k, a, 1);
    let cb = copy_of(k, b, 2);
    sm::boolean(k, ca, cb, op).unwrap().result
}

/// The body's worst edge-curve-end to vertex distance, and its worst ratio of that
/// distance to the vertex's tolerance.
fn end_gaps(k: &Kernel, r: &Brep) -> (f64, f64) {
    let (mut worst, mut ratio): (f64, f64) = (0.0, 0.0);
    for &e in r.edge_list() {
        let er = r.edge(e).unwrap();
        let c = er.curve.as_ref().unwrap();
        for (param, v) in [er.interval.min(), er.interval.max()]
            .into_iter()
            .zip(er.vertices)
        {
            let p = c.evaluate_point(k, param).unwrap();
            let v = r.vertex(v).unwrap();
            let gap = p.distance(v.point);
            worst = worst.max(gap);
            ratio = ratio.max(gap / v.tolerance);
        }
    }
    (worst, ratio)
}

/// The union's (edges, vertices, worst edge-curve-end to vertex distance, worst ratio of
/// that distance to the vertex's tolerance).
fn union_edge_ends(k: &mut Kernel) -> (usize, usize, f64, f64) {
    let outer = loft(k, &CONELO, &CONEHI);
    let inner = loft(k, &CONEINLO, &CONEINHI);
    let cone = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let outer = loft(k, &LIPLO, &LIPHI);
    let inner = loft(k, &LIPINLO, &LIPINHI);
    let lip = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let r = boolean(k, &cone, &lip, BooleanOperation::Union);
    assert_eq!(r.face_list().len(), 388);
    assert!(p2::is_manifold_solid(&r).unwrap());
    let (worst, ratio) = end_gaps(k, &r);
    (r.edge_list().len(), r.vertex_list().len(), worst, ratio)
}

/// The end extension (always on), with WR-469's vertex step (always on since #17985; its
/// wider vertex tolerances gave the extension its reach in #1480's bisect). Without ffaf4e8 the
/// extended union had 992 edges, 608 vertices and an edge end 8.061e-5 off its vertex. With
/// every `UpstreamFixes` flag on (SolidCraft's set) the union is the C++ fork's: 1096 edges,
/// 712 vertices, worst edge end 1.206e-5.
fn kernel() -> Kernel {
    Kernel::new(&KernelOptions::default())
}

#[test]
fn issue_1480_union_edges_end_at_their_own_vertices() {
    let mut k = kernel();
    let (edges, vertices, worst, ratio) = union_edge_ends(&mut k);
    assert_eq!((edges, vertices), (1096, 712));
    assert!(worst <= 2e-5, "worst edge end {worst:e} off its vertex");
    assert!(
        ratio <= 0.5,
        "an edge end {ratio} of its vertex's tolerance away"
    );
}

/// The union (every `UpstreamFixes` field at its default). `tolerances`, `worst_end_gap` and `volume` are
/// captured before the validity check; `faces`, `edges`, `vertices` and `manifold`
/// assert on the returned body after it. `check` is the level-2 report (empty since
/// #17888; before it `ValidatePointers` refused and `AssertValid` was skipped).
struct Union {
    faces: usize,
    edges: usize,
    vertices: usize,
    manifold: bool,
    worst_end_gap: f64,
    /// The worst ratio of an edge end's distance to its vertex's tolerance.
    worst_ratio: f64,
    volume: f64,
    tolerances: Vec<(Point3, f64)>,
    check: Vec<String>,
    /// The volume's move from 44443.49387571212 (before `brep_volume` closed tolerant edges'
    /// gaps, #16735) checked on the body: `volume_leak::moved_by_the_old_leak`'s figures.
    moved: Result<String, String>,
}

fn union_with_default_fixes() -> Union {
    let k = &mut Kernel::new(&KernelOptions::default());
    let outer = loft(k, &CONELO, &CONEHI);
    let inner = loft(k, &CONEINLO, &CONEINHI);
    let cone = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let outer = loft(k, &LIPLO, &LIPHI);
    let inner = loft(k, &LIPINLO, &LIPINHI);
    let lip = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let mut r = boolean(k, &cone, &lip, BooleanOperation::Union);
    let (worst_end_gap, worst_ratio) = end_gaps(k, &r);
    let tolerances = r
        .vertex_list()
        .iter()
        .map(|&v| {
            let v = r.vertex(v).unwrap();
            (v.point, v.tolerance)
        })
        .collect();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(k, &mut r, 1e-8, &attrs).unwrap();
    let moved = volume_leak::moved_by_the_old_leak(k, &mut r, 44443.49387571212, volume, 1e-8);
    let check = check_brep(k, &mut r, CheckLevel::Level2).unwrap();
    Union {
        faces: r.face_list().len(),
        edges: r.edge_list().len(),
        vertices: r.vertex_list().len(),
        manifold: p2::is_manifold_solid(&r).unwrap(),
        worst_end_gap,
        worst_ratio,
        volume,
        tolerances,
        moved,
        check: check.lines(),
    }
}

/// The eight vertices whose tolerance WR-469's step (then the opt-in
/// `new_edge_vertex_tolerance_covers_gap`, retired by #17985) alone changed on the #1480
/// union until #17888, as `(x, y, z, tolerance with the flag off, with it on)`. The "on" values are the branch's `2 * gap`-sized stored values: the
/// store-time gap is not observable from the final body (the p9 routine test bounds the
/// growth at `>= 2 * gap`), and later booleans move the edge ends, so the end gaps were
/// smaller (the fix's value only grows a tolerance).
const WR469_VERTICES: [(f64, f64, f64, f64, f64); 8] = [
    (
        99.5929150054111,
        57.49996907908349,
        -0.30001241426386144,
        1.70348595231800470e-5,
        2.85897936527436294e-5,
    ),
    (
        57.499969079083485,
        99.5929150054111,
        -0.3000124142638605,
        1.70348595203878130e-5,
        2.85897936668835265e-5,
    ),
    (
        -57.49996907908349,
        99.5929150054111,
        -0.3000124142638637,
        1.70348595232676946e-5,
        2.85897936177208906e-5,
    ),
    (
        -99.5929150054111,
        57.49996907908349,
        -0.30001241426386155,
        1.70348595231860914e-5,
        2.85897936786007297e-5,
    ),
    (
        -99.5929150054111,
        -57.49996907908349,
        -0.3000124142638637,
        1.70348595232676946e-5,
        2.85897936268689989e-5,
    ),
    (
        -57.49996907908349,
        -99.5929150054111,
        -0.30001241426386155,
        1.70348595231860914e-5,
        2.85897936891094169e-5,
    ),
    (
        57.49996907908349,
        -99.5929150054111,
        -0.3000124142638637,
        1.70348595232676946e-5,
        2.85897936805655650e-5,
    ),
    (
        99.5929150054111,
        -57.49996907908349,
        -0.30001241426386155,
        1.70348595231860914e-5,
        2.85897936891094169e-5,
    ),
];

/// The same eight vertices since #17888, in the same order, as `(x, y, z, tolerance)` in
/// both runs: WR-306's absorb fits each to its edges, moving it 2.0e-6 or 5.4e-6 and growing its
/// tolerance past the flag's "on" value above, so the flag (which only grows a tolerance)
/// leaves it as it is. With the end extension (WR-453, always on since #14246) the fitted
/// tolerances are 2.9944e-5 / 3.3237e-5 (MEASURED; 3.8427e-5 / 3.8508e-5 without it), the
/// positions the same.
const WR469_VERTICES_NOW: [(f64, f64, f64, f64); 8] = [
    (
        99.59291687949045,
        57.49996447286864,
        -0.30001043114010567,
        2.9944469066305617e-5,
    ),
    (
        57.49996743305864,
        99.59291618106259,
        -0.3000121926808409,
        3.323710466021156e-5,
    ),
    (
        -57.49996447286864,
        99.59291687949046,
        -0.3000104311401014,
        2.9944469011196064e-5,
    ),
    (
        -99.59291618106259,
        57.49996743305864,
        -0.3000121926808419,
        3.323710469548465e-5,
    ),
    (
        -99.59291687949046,
        -57.49996447286864,
        -0.3000104311401034,
        2.9944469025042095e-5,
    ),
    (
        -57.49996743305864,
        -99.59291618106259,
        -0.3000121926808419,
        3.3237104694946994e-5,
    ),
    (
        57.49996447286864,
        -99.59291687949045,
        -0.30001043114010123,
        2.9944469020331108e-5,
    ),
    (
        99.59291618106259,
        -57.49996743305864,
        -0.3000121926808419,
        3.3237104694946994e-5,
    ),
];

/// The vertices within `radius` of `point`, as `(position, tolerance)`.
fn vertices_near(u: &Union, point: Point3, radius: f64) -> Vec<(Point3, f64)> {
    u.tolerances
        .iter()
        .filter(|(p, _)| p.distance(point) <= radius)
        .copied()
        .collect()
}

fn close(got: f64, want: f64) -> bool {
    (got - want).abs() <= 1e-12 * want.abs()
}

/// WR-469, isolated: until #17985 retired `new_edge_vertex_tolerance_covers_gap` (the
/// step is now always on) this ran the union with the flag cleared, then set. Until
/// WR-306's absorb became the only path (#17888) the flag grew the eight
/// vertex tolerances of `WR469_VERTICES` at the ridge crossings from 1.70e-5 to its
/// `2 * gap`-sized 2.86e-5, and the union failed level 2 with 1.73 mm edge and vertex
/// tolerances (upstream's millimetre tolerances, #1058). Now the eight 1.43e-5 closed
/// proto edges there lie inside their vertex's tolerance, so they are absorbed and
/// `ImprintIntersections` fits each vertex pair to its edges: the same eight vertices
/// (`WR469_VERTICES_NOW`) move under 1e-5 and their tolerances grow to 3.84e-5, past the
/// flag's value, which only grows a tolerance. The flag then changes no vertex on this
/// rig: both runs are the same body, every edge end within half its vertex's tolerance,
/// and level 2 passes (MEASURED). That made the retirement a no-op here: the one run
/// left is both earlier runs' body. The step's own growth stays covered by the p9 routine
/// test (`>= 2 * gap`). The name is kept because the WR-469 records cite it.
#[test]
fn wr469_union_flag_alone_grows_eight_vertex_tolerances() {
    let u = union_with_default_fixes();
    let name = "now";
    assert_eq!((u.faces, u.edges, u.vertices), (388, 1096, 712), "{name}");
    assert!(u.manifold, "{name}");
    // 7.4750471556652e-6 until the end extension (WR-453) became the only path
    // (#14246): it continues an intersection curve that stopped short of its proto
    // vertex to it, and the worst edge end is now nearer its vertex; topology, check,
    // vertex tolerances and volume (4.5e-15 relative) are unchanged (MEASURED). The
    // gap is the curve's point at each interval end against its vertex, exactly as
    // `union_with_default_fixes` evaluates it. That gave 7.034859535377774e-6
    // until WR-306's absorb became the only path (#17888): the eight 1.43e-5 closed
    // proto edges at the ridge crossings lie inside their vertex's tolerance, so they
    // are absorbed and the vertex pairs fitted to their edges (MEASURED; 1.2200e-5
    // without the extension). Every end stays within half its vertex's tolerance,
    // asserted in `issue_1480_union_edges_end_at_their_own_vertices`.
    assert!(
        close(u.worst_end_gap, 1.2009422366085169e-5),
        "{name} worst end gap {:e}",
        u.worst_end_gap
    );
    assert!(
        u.worst_ratio <= 0.5,
        "{name}: an edge end {} of its vertex's tolerance away",
        u.worst_ratio
    );
    // 44443.49369572108 until WR-751 (#2621): `MergeIntoObject` now splits the eight
    // crossing ridges' edges where they pass nearest their points (2.2476e-6 ->
    // 2.2417e-6 and 2.2381e-6 -> 2.2298e-6 off), not at the classifications'
    // parameters, 1.8e-8 along; topology, worst end gap and check are unchanged and
    // the volume moves 4.0e-9 relative. 44443.49387571212 until `brep_volume` closed
    // tolerant edges' gaps (#16735): -3.0e-8 relative with topology, end gap and check
    // unchanged (MEASURED), the move within the old integrator's leak bound on the body
    // (`moved`). #17888 moves it a further -3.2e-14 relative (44443.49253850026).
    assert!(
        close(u.volume, 44443.49253850169),
        "{name} volume {:?}",
        u.volume
    );
    assert!(u.moved.is_ok(), "{name}: {:?}", u.moved);
    // Until #17888 the report held four "Edge tolerance relative to brep tolerance"
    // and eight "Vertex tolerance is too wide" failures (1.7255363), and
    // `AssertValid` was skipped.
    assert!(u.check.is_empty(), "{name}: {:?}", u.check);
    for (was, now) in WR469_VERTICES.iter().zip(WR469_VERTICES_NOW) {
        let (x, y, z, off_tolerance, on_tolerance) = *was;
        let point = Point3::new(x, y, z);
        let at = Point3::new(now.0, now.1, now.2);
        assert!(off_tolerance < on_tolerance, "at {point:?}");
        // The vertex that was at `point` is the one there now: the only vertex within
        // 1e-5 (the crossings' vertex pairs are 3e-5 to 8e-5 apart).
        let near = vertices_near(&u, point, 1e-5);
        assert_eq!(near.len(), 1, "{name} at {point:?}: {near:?}");
        let (p, tolerance) = near[0];
        assert!(p.distance(at) <= 1e-9, "{name}: {p:?} is not {at:?}");
        assert!(close(tolerance, now.3), "{name} {tolerance:.17e} at {at:?}");
        // past the flag's value, so the flag leaves it as it is
        assert!(tolerance > on_tolerance, "{name} at {at:?}");
    }
}
