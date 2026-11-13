//! tangency-labs/solidcore#1480 (#470, WR-498): the union of two crossing pleated loft
//! skins. Where pleat ridges cross, two vertex definitions 3e-5 to 8e-5 apart fall in one
//! proto vertex, which `Resolve` makes two vertices of; the proto vertex's position is
//! their mean. With `imprint_extends_edge_to_proto_vertex` (and the end trim, WR-452) an
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

/// The union's (edges, vertices, worst edge-curve-end to vertex distance).
fn union_edge_ends(k: &mut Kernel) -> (usize, usize, f64) {
    let outer = loft(k, &CONELO, &CONEHI);
    let inner = loft(k, &CONEINLO, &CONEINHI);
    let cone = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let outer = loft(k, &LIPLO, &LIPHI);
    let inner = loft(k, &LIPINLO, &LIPINHI);
    let lip = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let r = boolean(k, &cone, &lip, BooleanOperation::Union);
    assert_eq!(r.face_list().len(), 388);
    assert!(p2::is_manifold_solid(&r).unwrap());
    let mut worst: f64 = 0.0;
    for &e in r.edge_list() {
        let er = r.edge(e).unwrap();
        let c = er.curve.as_ref().unwrap();
        for (param, v) in [er.interval.min(), er.interval.max()]
            .into_iter()
            .zip(er.vertices)
        {
            let p = c.evaluate_point(k, param).unwrap();
            worst = worst.max(p.distance(r.vertex(v).unwrap().point));
        }
    }
    (r.edge_list().len(), r.vertex_list().len(), worst)
}

/// The extension flag, with `new_edge_vertex_tolerance_covers_gap` (whose wider vertex
/// tolerances gave the extension its reach in #1480's bisect). Without ffaf4e8 the
/// extended union had 992 edges, 608 vertices and an edge end 8.061e-5 off its vertex. With
/// every `UpstreamFixes` flag on (SolidCraft's set) the union is the C++ fork's: 1096 edges,
/// 712 vertices, worst edge end 1.206e-5.
fn kernel(extend: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes.new_edge_vertex_tolerance_covers_gap = true;
    k.fixes.imprint_extends_edge_to_proto_vertex = extend;
    k
}

#[test]
fn issue_1480_union_edges_end_at_their_own_vertices() {
    let mut k = kernel(true);
    let (edges, vertices, worst) = union_edge_ends(&mut k);
    assert_eq!((edges, vertices), (1096, 712));
    assert!(worst <= 2e-5, "worst edge end {worst:e} off its vertex");
}

#[test]
fn issue_1480_union_without_the_extension_keeps_upstream_s_ends() {
    let mut k = kernel(false);
    let (edges, vertices, worst) = union_edge_ends(&mut k);
    // The end trim alone moves no curve end here: upstream's topology, every end within
    // 7.5e-6.
    assert_eq!((edges, vertices), (1096, 712));
    assert!(worst < 7.5e-6, "worst edge end {worst:e} off its vertex");
}

/// The union, with `new_edge_vertex_tolerance_covers_gap` cleared, then set (every
/// other `UpstreamFixes` flag default). `tolerances`, `worst_end_gap` and `volume` are
/// captured before the validity check; `faces`, `edges`, `vertices` and `manifold`
/// assert on the returned body after it. On this rig the check skips `AssertValid`
/// (`ValidatePointers` refused first), so the report the test compares is the
/// segmented-pointer report alone.
struct Union {
    faces: usize,
    edges: usize,
    vertices: usize,
    manifold: bool,
    worst_end_gap: f64,
    volume: f64,
    tolerances: Vec<(Point3, f64)>,
    check: Vec<String>,
}

fn union_with_only_the_new_edge_flag(fix: bool) -> Union {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes.new_edge_vertex_tolerance_covers_gap = fix;
    let k = &mut k;
    let outer = loft(k, &CONELO, &CONEHI);
    let inner = loft(k, &CONEINLO, &CONEINHI);
    let cone = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let outer = loft(k, &LIPLO, &LIPHI);
    let inner = loft(k, &LIPINLO, &LIPINHI);
    let lip = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let mut r = boolean(k, &cone, &lip, BooleanOperation::Union);
    let mut worst_end_gap: f64 = 0.0;
    for &e in r.edge_list() {
        let er = r.edge(e).unwrap();
        let c = er.curve.as_ref().unwrap();
        for (param, v) in [er.interval.min(), er.interval.max()]
            .into_iter()
            .zip(er.vertices)
        {
            let p = c.evaluate_point(k, param).unwrap();
            worst_end_gap = worst_end_gap.max(p.distance(r.vertex(v).unwrap().point));
        }
    }
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
    let check = check_brep(k, &mut r, CheckLevel::Level2).unwrap();
    Union {
        faces: r.face_list().len(),
        edges: r.edge_list().len(),
        vertices: r.vertex_list().len(),
        manifold: p2::is_manifold_solid(&r).unwrap(),
        worst_end_gap,
        volume,
        tolerances,
        check: check.lines(),
    }
}

/// The eight vertices whose tolerance `new_edge_vertex_tolerance_covers_gap` alone
/// changes on the #1480 union, as `(x, y, z, tolerance with the flag off, with it on)`.
/// The "on" values are the branch's `2 * gap`-sized stored values: the store-time gap
/// is not observable from the final body (the p9 routine test bounds the growth at
/// `>= 2 * gap`), and later booleans move the edge ends, so the current end gaps are
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

/// The tolerance of the vertex at `point` (within 1e-9, far closer than the rig's
/// distinct vertices are to each other).
fn tolerance_at(u: &Union, point: Point3) -> f64 {
    let mut found = None;
    for (p, t) in &u.tolerances {
        if p.distance(point) <= 1e-9 {
            assert!(found.is_none(), "two vertices at {point:?}");
            found = Some(*t);
        }
    }
    found.unwrap_or_else(|| panic!("no vertex at {point:?}"))
}

fn close(got: f64, want: f64) -> bool {
    (got - want).abs() <= 1e-12 * want.abs()
}

/// WR-469, isolated: on the #1480 union the flag changes no topology, volume or
/// reported check, and grows exactly these eight vertex tolerances (to the branch's
/// `2 * gap`-sized stored values). On this rig the report is the segmented-pointer
/// check (`AssertValid` skipped); the default keeps them, as the pinned C++ does.
#[test]
fn wr469_union_flag_alone_grows_eight_vertex_tolerances() {
    assert!(!Kernel::default().fixes.new_edge_vertex_tolerance_covers_gap);
    let off = union_with_only_the_new_edge_flag(false);
    let on = union_with_only_the_new_edge_flag(true);
    for (name, u) in [("off", &off), ("on", &on)] {
        assert_eq!((u.faces, u.edges, u.vertices), (388, 1096, 712), "{name}");
        assert!(u.manifold, "{name}");
        assert!(
            close(u.worst_end_gap, 7.4750471556652e-6),
            "{name} worst end gap {:e}",
            u.worst_end_gap
        );
        // 44443.49369572108 until WR-751 (#2621): `MergeIntoObject` now splits the eight
        // crossing ridges' edges where they pass nearest their points (2.2476e-6 ->
        // 2.2417e-6 and 2.2381e-6 -> 2.2298e-6 off), not at the classifications'
        // parameters, 1.8e-8 along; topology, worst end gap and check are unchanged and
        // the volume moves 4.0e-9 relative.
        assert!(
            close(u.volume, 44443.49387571212),
            "{name} volume {:?}",
            u.volume
        );
    }
    // The two runs are deterministic: same vertices in the same order, same outcome.
    assert_eq!(off.tolerances.len(), on.tolerances.len());
    assert_eq!(off.check, on.check);
    let mut changed = Vec::new();
    for ((p_off, t_off), (p_on, t_on)) in off.tolerances.iter().zip(&on.tolerances) {
        assert_eq!(p_off, p_on);
        if t_off != t_on {
            changed.push(*p_on);
        }
    }
    assert_eq!(changed.len(), WR469_VERTICES.len(), "{changed:?}");
    for (x, y, z, off_tolerance, on_tolerance) in WR469_VERTICES {
        let point = Point3::new(x, y, z);
        assert!(
            changed.iter().any(|p| p.distance(point) <= 1e-9),
            "vertex {point:?} did not change"
        );
        let off_tolerance_at = tolerance_at(&off, point);
        let on_tolerance_at = tolerance_at(&on, point);
        assert!(
            close(off_tolerance_at, off_tolerance),
            "off {off_tolerance_at:.17e} at {point:?}"
        );
        assert!(
            close(on_tolerance_at, on_tolerance),
            "on {on_tolerance_at:.17e} at {point:?}"
        );
        assert!(on_tolerance_at > off_tolerance_at, "at {point:?}");
    }
}
