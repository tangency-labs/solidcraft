//! tangency-labs/solidcore#1058 (WR-306): SolidCraft rig booleans/4623 joins two crossing
//! pleated loft skins, and SMLib's check refuses the result: edges and vertices with a
//! tolerance of 1.73 mm against 0.01. Where two pleat creases pass about 1e-5 apart, both
//! ends of a short intersection curve fall in one proto vertex; `SmProtoEdge::Resolve`
//! keeps it, a speck face is split off holding a 15 mm intersection edge, and
//! `RadialSectorClassify` drops that edge's trim 0.86 mm off the face. The join also loses
//! volume. The opt-in `Kernel::fixes.degenerate_proto_edge_in_one_vertex` makes such a
//! curve the vertex (the fork's "an intersection curve inside one vertex is that vertex,
//! not a closed edge on it").
//!
//! The operands are the rig's recipe, as in `issue_1480_pleated_loft_union_edge_ends.rs`:
//! each skin is a capped ruled loft minus a slightly larger one, copied into its own key
//! domain before each boolean.
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
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
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

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The union's worst vertex and edge tolerances, and how far union + intersection falls
/// short of the two skins' volumes (relative to their sum).
fn join(fix: bool) -> (f64, f64, f64) {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes.degenerate_proto_edge_in_one_vertex = fix;
    let k = &mut k;
    let outer = loft(k, &CONELO, &CONEHI);
    let inner = loft(k, &CONEINLO, &CONEINHI);
    let mut cone = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let outer = loft(k, &LIPLO, &LIPHI);
    let inner = loft(k, &LIPINLO, &LIPINHI);
    let mut lip = boolean(k, &outer, &inner, BooleanOperation::Difference);
    let mut union = boolean(k, &cone, &lip, BooleanOperation::Union);
    let mut common = boolean(k, &cone, &lip, BooleanOperation::Intersection);
    assert!(p2::is_manifold_solid(&union).unwrap());
    let worst_vertex = union
        .vertex_list()
        .iter()
        .map(|&v| union.vertex(v).unwrap().tolerance)
        .fold(0.0, f64::max);
    let worst_edge = union
        .edge_list()
        .iter()
        .map(|&e| union.edge(e).unwrap().tolerance)
        .fold(0.0, f64::max);
    let skins = volume(k, &mut cone) + volume(k, &mut lip);
    let lost = (skins - volume(k, &mut union) - volume(k, &mut common)) / skins;
    (worst_vertex, worst_edge, lost)
}

#[test]
fn issue_1058_flag_is_off_by_default() {
    assert!(!Kernel::default().fixes.degenerate_proto_edge_in_one_vertex);
}

/// Upstream: tolerances of 1.73 mm, and the join and the intersection lose 18.8 mm^3.
#[test]
fn issue_1058_upstream_join_has_millimetre_tolerances() {
    let (vertex, edge, lost) = join(false);
    assert!(vertex > 1.0 && edge > 1.0, "{vertex:e} {edge:e}");
    assert!(lost > 1e-4, "{lost:e}");
}

/// With the fix every tolerance stays under 1e-4 (the creases' near miss is about 1.6e-5),
/// and union + intersection is the two skins' volumes.
#[test]
fn issue_1058_join_keeps_its_tolerances_with_the_fix() {
    let (vertex, edge, lost) = join(true);
    assert!(vertex < 1e-4 && edge < 1e-4, "{vertex:e} {edge:e}");
    assert!(lost.abs() < 1e-6, "{lost:e}");
}
