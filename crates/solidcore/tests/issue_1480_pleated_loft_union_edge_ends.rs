//! tangency-labs/solidcore#1480 (#470, WR-498): the union of two crossing pleated loft
//! skins. Where pleat ridges cross, two vertex definitions 3e-5 to 8e-5 apart fall in one
//! proto vertex, which `Resolve` makes two vertices of; the proto vertex's position is
//! their mean. With `imprint_extends_edge_to_proto_vertex` (and the trim flag) an
//! intersection curve that already ended on its own vertex was continued to that mean,
//! leaving edge ends up to 8.06e-5 off their vertices and dropping 104 short edges
//! (992 edges, 608 vertices). The fork's ffaf4e8, folded into both flags, sends such an
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
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::math::Point3;
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

/// The two imprint flags, with `new_edge_vertex_tolerance_covers_gap` (whose wider vertex
/// tolerances gave the extension its reach in #1480's bisect). Without ffaf4e8 the flags-on
/// union had 992 edges, 608 vertices and an edge end 8.061e-5 off its vertex. With every
/// `UpstreamFixes` flag on (SolidCraft's set) the union is the C++ fork's: 1096 edges, 712
/// vertices, worst edge end 1.206e-5.
fn kernel(trim: bool, extend: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes.new_edge_vertex_tolerance_covers_gap = true;
    k.fixes.imprint_trims_edge_to_proto_vertex = trim;
    k.fixes.imprint_extends_edge_to_proto_vertex = extend;
    k
}

#[test]
fn issue_1480_union_edges_end_at_their_own_vertices() {
    let mut k = kernel(true, true);
    let (edges, vertices, worst) = union_edge_ends(&mut k);
    assert_eq!((edges, vertices), (1096, 712));
    assert!(worst <= 2e-5, "worst edge end {worst:e} off its vertex");
}

#[test]
fn issue_1480_union_without_the_imprint_fixes_is_upstream() {
    let mut k = kernel(false, false);
    let (edges, vertices, worst) = union_edge_ends(&mut k);
    // Upstream moves no curve end: the same topology, every end within 7.5e-6.
    assert_eq!((edges, vertices), (1096, 712));
    assert!(worst < 7.5e-6, "worst edge end {worst:e} off its vertex");
}
