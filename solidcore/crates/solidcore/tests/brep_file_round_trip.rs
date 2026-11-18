//! `SmApiWriteBrepToFile` then `SmApiReadBrepFromFile` on the port's own output: every
//! primitive and a boolean result, in binary and ASCII, read back with the same topology
//! counts, vertex points, face surfaces (kind and points over each original face's UV
//! domain), bounding box, and volume and area (to the precise properties' accuracy). The
//! file keeps geometry to its number format: binary exactly, ASCII `fixed` with 16
//! decimals as upstream writes it, so points agree to `1e-12` of the body's size. The
//! writer's caller identities (`key_of`) are
//! either the B-rep's own ([`Brep::owner_identity`]) or a caller's naming that has nothing
//! to do with how the B-rep records its geometry owners (as the C API glue names
//! entities): a face owns its surface whatever names the caller gives it, so both files
//! must hold every face's surface.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_with_options};
use solidcore::boolean::merge::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::heal::face_props::HealerOpType;
use solidcore::mass::integrator::properties::{
    brep_properties, MassPropertyBehavior, PropertyAttributes,
};
use solidcore::primitive::api_primitives as primitives;
use solidcore::query::brep::BrepQuery;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p1::heal_brep;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::topology_ref;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::p9::find_infinite_region;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId, RegionId};
use solidcore::{Brep, EntityRef, Kernel, Point2, Point3, SolidError, Vec3};

/// A caller's naming unrelated to the B-rep's owner identities (the C API glue's).
fn caller_named(entity: EntityRef) -> ObjectRef {
    let r = topology_ref(0, entity);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(r.type_code.unsigned_abs() * 1_000_000 + (r.key.0 & 0xffff_ffff)),
    }
}

#[derive(Clone, Copy, Debug)]
enum Naming {
    /// `key_of` is the B-rep's own owner identity.
    Own,
    /// `key_of` is [`caller_named`].
    Caller,
}

fn write(k: &Kernel, b: &Brep, naming: Naming, ascii: bool) -> Vec<u8> {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let own = |e: EntityRef| b.owner_identity(e);
    let key_of: &dyn Fn(EntityRef) -> ObjectRef = match naming {
        Naming::Own => &own,
        Naming::Caller => &caller_named,
    };
    let env = FromBrepEnv {
        attributes: &store,
        key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, ascii).unwrap()
}

fn read(k: &mut Kernel, bytes: Vec<u8>, ascii: bool) -> solidcore::Result<Brep> {
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &caller_named,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes), ascii, false, &mut env)?;
    b.refresh_cached_topology();
    Ok(b)
}

/// (faces, edges, vertices) reached by the topology walks.
fn counts(b: &Brep) -> [usize; 3] {
    [
        bl::faces(b, None).unwrap().len(),
        bl::edges(b, None).unwrap().len(),
        bl::vertices(b, None).unwrap().len(),
    ]
}

/// Bounding box min and max, then volume and area when the precise properties converge
/// (1.0.1's sphere properties can fail at tight accuracies, WR-472: the read body must
/// then fail the same way).
type Measures = ([f64; 3], [f64; 3], Option<(f64, f64)>);

fn measure(k: &Kernel, b: &mut Brep) -> Measures {
    let bbox = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default()).unwrap();
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let p = brep_properties(
        k,
        b,
        1e-6,
        bbox.mid(),
        MassPropertyBehavior::default(),
        &PropertyAttributes {
            store: &store,
            key_of: &key_of,
        },
    )
    .ok()
    .map(|p| (p.volume, p.area));
    let (lo, hi) = (bbox.min(), bbox.max());
    ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z], p)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// Volume and area agree to ten times the properties' accuracy (1e-6): the integration
/// adapts to the faces' representation, so equal geometry can give different roundoff
/// within it.
fn close_props(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-5 * a.abs().max(b.abs())
}

/// Vertex points and face surfaces of `r` against `b` (same walk order), within `tol`:
/// the surface kinds match and each read surface evaluates where the original does at a
/// 3x3 grid over the original face's UV domain.
fn geometry_mismatch(k: &Kernel, b: &Brep, r: &Brep, tol: f64) -> Option<String> {
    let near = |p: Point3, q: Point3| (p - q).length() <= tol;
    let (bv, rv) = (
        bl::vertices(b, None).unwrap(),
        bl::vertices(r, None).unwrap(),
    );
    for (i, (v, w)) in bv.iter().zip(&rv).enumerate() {
        let (p, q) = (b.vertex(*v).unwrap().point, r.vertex(*w).unwrap().point);
        if !near(p, q) {
            return Some(format!("vertex {i}: {q:?} != {p:?}"));
        }
    }
    let (bf, rf) = (bl::faces(b, None).unwrap(), bl::faces(r, None).unwrap());
    for (i, (f, g)) in bf.iter().zip(&rf).enumerate() {
        let (fr, gr) = (b.face(*f).unwrap(), r.face(*g).unwrap());
        let (s, t) = (fr.surface.as_ref().unwrap(), gr.surface.as_ref().unwrap());
        if std::mem::discriminant(s.kind()) != std::mem::discriminant(t.kind()) {
            return Some(format!("face {i}: surface kind changed"));
        }
        let (lo, hi) = (fr.uv_domain.min(), fr.uv_domain.max());
        for a in [0.0, 0.5, 1.0] {
            for c in [0.0, 0.5, 1.0] {
                let uv = Point2::new(lo.x + a * (hi.x - lo.x), lo.y + c * (hi.y - lo.y));
                let (p, q) = (s.evaluate_point(k, uv), t.evaluate_point(k, uv));
                match (p, q) {
                    (Ok(p), Ok(q)) if near(p, q) => {}
                    (Err(_), Err(_)) => {}
                    (p, q) => return Some(format!("face {i} at {uv:?}: {q:?} != {p:?}")),
                }
            }
        }
    }
    None
}

fn bodies(k: &mut Kernel) -> Vec<(&'static str, Brep)> {
    let o = Point3::new(0.0, 0.0, 0.0);
    let mut out = vec![
        ("box", primitives::create_box(k, o, 2.0, 3.0, 4.0).unwrap()),
        (
            "cylinder",
            primitives::create_cylinder(k, o, 1.5, 4.0).unwrap(),
        ),
        (
            "cone",
            primitives::create_cone(k, o, 2.0, 0.5, 3.0).unwrap(),
        ),
        ("sphere", primitives::create_sphere(k, o, 2.5).unwrap()),
        ("torus", primitives::create_torus(k, o, 5.0, 1.5).unwrap()),
    ];
    let block = primitives::create_box(k, o, 4.0, 4.0, 2.0).unwrap();
    let drill = primitives::create_cylinder(k, Point3::new(2.0, 2.0, -1.0), 1.0, 4.0).unwrap();
    let mut holed = boolean_difference(k, block, drill).unwrap();
    holed.refresh_cached_topology();
    out.push(("box less cylinder", holed));
    out.push(("crossing creases union", crossing_creases_union(k)));
    out
}

/// `boolean.api_brep`'s `crossing_creases_union_d1e-05`: box [0,10]^3 united (post-process
/// only) with a 10 x 10 x 20 box turned 45 degrees about z, then x, whose crease crosses the
/// first box's edge y = z = 10 at depth 1e-5. The Boolean drops a 1.4e-5 edge onto a face
/// point by point and finds only two points for its UV trim.
fn crossing_creases_union(k: &mut Kernel) -> Brep {
    let o = Point3::new(0.0, 0.0, 0.0);
    let a = primitives::create_box(k, o, 10.0, 10.0, 10.0).unwrap();
    let mut b = primitives::create_box(k, Point3::new(0.0, 0.0, -10.0), 10.0, 10.0, 20.0).unwrap();
    for axis in [Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)] {
        api_rotate(k, Some(TransformObject::Brep(&mut b)), o, axis, 45.0).unwrap();
    }
    let s = 10.0 - 1e-5 / 2.0_f64.sqrt();
    api_translate(k, Some(TransformObject::Brep(&mut b)), Vec3::new(5.0, s, s)).unwrap();
    let flags = MergeOptions::new(false, false, true, false, false);
    let mut u = boolean_with_options(k, a, b, BooleanOperation::Union, flags)
        .unwrap()
        .result;
    u.refresh_cached_topology();
    // the union that reaches the two-point drop: 3000 less a speck where the creases cross
    assert_eq!(counts(&u), [12, 26, 17]);
    let volume = measure(k, &mut u).2.map(|p| p.0);
    assert!(volume.is_some_and(|v| close_props(v, 3000.0)), "{volume:?}");
    u
}

fn round_trip(naming: Naming) {
    let mut k = Kernel::default();
    let mut failures = Vec::new();
    for (name, mut b) in bodies(&mut k) {
        let want_counts = counts(&b);
        let want = measure(&k, &mut b);
        for ascii in [false, true] {
            let label = format!(
                "{name} {naming:?} {}",
                if ascii { "ascii" } else { "binary" }
            );
            let bytes = write(&k, &b, naming, ascii);
            let mut r = match read(&mut k, bytes, ascii) {
                Ok(r) => r,
                Err(e) => {
                    failures.push(format!("{label}: read failed: {e:?}"));
                    continue;
                }
            };
            if counts(&r) != want_counts {
                failures.push(format!(
                    "{label}: counts {:?} != {want_counts:?}",
                    counts(&r)
                ));
                continue;
            }
            let size = (0..3).map(|i| want.1[i] - want.0[i]).fold(1.0, f64::max);
            if let Some(m) = geometry_mismatch(&k, &b, &r, 1e-12 * size) {
                failures.push(format!("{label}: {m}"));
                continue;
            }
            let got = measure(&k, &mut r);
            let same_props = match (got.2, want.2) {
                (Some(g), Some(w)) => close_props(g.0, w.0) && close_props(g.1, w.1),
                (None, None) => true,
                _ => false,
            };
            let same = (0..3).all(|i| close(got.0[i], want.0[i]) && close(got.1[i], want.1[i]))
                && same_props;
            if !same {
                failures.push(format!("{label}: measures {got:?} != {want:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn bodies_round_trip_with_their_own_identities() {
    round_trip(Naming::Own);
}

#[test]
fn bodies_round_trip_with_a_callers_identities() {
    round_trip(Naming::Caller);
}

/// `Solids/box.smb` at the pinned upstream commit, vendored verbatim
/// (`data/reader-regressions/README.md`). Its data predates the current healer, so a read
/// heals it, infinite-region ray search included.
const BOX: &[u8] = include_bytes!("data/reader-regressions/box.smb");

/// `SmBrep::ReadFromFile` of [`BOX`] into a new B-rep, the topology named by `key_of`.
fn read_box(
    k: &mut Kernel,
    key_of: &dyn Fn(EntityRef) -> ObjectRef,
) -> (solidcore::Result<()>, Brep) {
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let r = read_from_file(k, &mut b, BOX.to_vec(), FileType::Ascii, false, &mut env);
    (r, b)
}

/// The regions of `b` with their void flags, in arena order, and its infinite region.
fn regions(b: &Brep) -> (Vec<(RegionId, i32)>, Option<RegionId>) {
    (
        b.regions()
            .iter()
            .map(|(id, r)| (id, r.is_void()))
            .collect(),
        b.infinite_region(),
    )
}

/// What the box file declares: region 0 the void infinite region, region 1 the solid.
fn box_regions() -> (Vec<(RegionId, i32)>, Option<RegionId>) {
    (
        vec![(RegionId::from_index(0), 1), (RegionId::from_index(1), 0)],
        Some(RegionId::from_index(0)),
    )
}

/// Identities that do not name each object distinctly under its own type code are
/// refused before the read heals: a curve owned by a non-edge identity names no edge, so
/// the healer's infinite-region rays had no edge to solve on ("curve without an edge")
/// and the region step failed (silently, its status dropped, or as 1001 once passed on).
#[test]
fn a_read_refuses_identities_that_name_no_object() {
    let untyped = |_: EntityRef| ObjectRef {
        type_code: 0,
        key: ObjectKey(0),
    };
    let shared = |e: EntityRef| ObjectRef {
        key: ObjectKey(7),
        ..notify_ref(e)
    };
    let cases: [(&dyn Fn(EntityRef) -> ObjectRef, &str); 2] = [
        (
            &untyped,
            "an object identity does not carry its object's type code",
        ),
        (&shared, "two objects share one identity"),
    ];
    for (key_of, expected) in cases {
        let mut k = Kernel::default();
        let (r, _) = read_box(&mut k, key_of);
        match r {
            Err(SolidError::InvalidInput { operation, reason }) => {
                assert_eq!(operation, "topology::brep::p10::make_topology_from_data");
                assert_eq!(reason, expected);
            }
            r => panic!("expected {expected:?}, got {r:?}"),
        }
    }
}

/// A box read under distinct, typed identities heals with the healer's statuses passed
/// on: its infinite-region rays solve point-edge on the box's own edges and find the
/// region the file declares, directly, through the read and through the public heal.
#[test]
fn a_box_read_heals_and_its_rays_find_the_infinite_region() {
    let mut k = Kernel::default();
    let (r, mut b) = read_box(&mut k, &notify_ref);
    r.unwrap();
    assert_eq!(b.naming(), solidcore::topology::NamingScheme::Caller);
    assert_eq!(regions(&b), box_regions());

    // the search itself: a ray that fails its point-edge solves is an error here
    let query = BrepQuery::of_brep(&b);
    let found = find_infinite_region(&mut k, &mut b, &query, None, None).unwrap();
    assert_eq!(found, Some(RegionId::from_index(0)));

    // the public heal, statuses passed on, keeps the regions it finds
    heal_brep(&mut k, &mut b, HealerOpType::All, None, None, None, false).unwrap();
    assert_eq!(regions(&b), box_regions());
}
