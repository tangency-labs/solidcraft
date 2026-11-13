//! tangency-labs/solidcore#1670: an offset or shell (thicken) of a caller-keyed copy
//! (`api_brep_copy`, as SolidCraft copies a body before an operation) came back with
//! its edges' curves owned under the caller's result identities (`result_key_of`) or,
//! after the self-intersection merge, the merge bodies' identities, while the result
//! was labelled `NamingScheme::Notify` (or `Unknown`) without those keys recorded, so
//! `check_brep` failed `SmBrep` test #61 on every edge. The geometry is right; upstream's
//! owner is the edge itself and its check passes.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, dead_code)]

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::offset::offset_executive::p2::{
    api_offset_brep_full, api_shell_brep_full, OffsetNotifications, OffsetOptions,
    ShellCopyIdentities, ShellOptions,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::BrepRef;
use solidcore::sweep::topology_sweep::store::SweepIdentities;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{p2 as brep_p2, p3 as brep_p3};
use solidcore::topology::topology::{NotifyRecords, Topology};
use solidcore::topology::{Brep, BrepId, EdgeId, RegionId};
use solidcore::{EntityRef, Point3};

const ONE: BrepId = BrepId::from_index(0);

/// An object name in key domain `domain`: each B-rep the offset executive touches (the source,
/// its working copy, the result, the merge's bodies) names its objects in its own domain, as
/// solidcore's offset fixture does, so their notification records never collide.
fn named(entity: EntityRef, domain: u64) -> ObjectRef {
    let r = brep_p3::topology_ref(0, entity);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
    }
}

const SOURCE: u64 = 100_000_000_000;
const RESULT: u64 = 200_000_000_000;
const WORKING: u64 = 600_000_000_000;

fn source_key(e: EntityRef) -> ObjectRef {
    named(e, SOURCE)
}

fn result_key(e: EntityRef) -> ObjectRef {
    named(e, RESULT)
}

fn working_key(e: EntityRef) -> ObjectRef {
    named(e, WORKING)
}

fn curve_in(domain: u64, edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(domain + u64::from(edge.index())),
        type_code,
    }
}

fn result_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
    curve_in(300_000_000_000, edge, type_code)
}

fn source_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
    curve_in(400_000_000_000, edge, type_code)
}

fn working_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
    curve_in(650_000_000_000, edge, type_code)
}

fn lists_in(domain: u64, region: RegionId) -> [ObjectRef; 2] {
    [0, 1].map(|list| ObjectRef {
        key: ObjectKey(domain + u64::from(region.index()) * 2 + list),
        type_code: 0,
    })
}

fn source_lists(result: bool, region: RegionId) -> [ObjectRef; 2] {
    lists_in(
        if result {
            510_000_000_000
        } else {
            500_000_000_000
        },
        region,
    )
}

fn working_lists(region: RegionId) -> [ObjectRef; 2] {
    lists_in(900_000_000_000, region)
}

fn merge_curve(brep: BrepId, edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(
            700_000_000_000 + u64::from(brep.index()) * 1_000_000_000 + u64::from(edge.index()),
        ),
        type_code,
    }
}

/// Identities of the B-reps the merge makes, disjoint from the keys above.
struct MergeBodies;

impl SweepIdentities for MergeBodies {
    fn topology(&self, brep: BrepId, entity: EntityRef) -> ObjectRef {
        named(
            entity,
            1_000_000_000_000 + u64::from(brep.index()) * 100_000_000_000,
        )
    }
    fn owned_auxiliary_objects(&self, brep: BrepId, model: &Brep) -> Vec<ObjectKey> {
        model
            .edges()
            .iter()
            .filter(|(_, e)| e.curve.is_some())
            .map(|(edge, _)| merge_curve(brep, edge, 0).key)
            .collect()
    }
    fn curve(&self, brep: BrepId, edge: EdgeId, curve: &Curve) -> ObjectRef {
        merge_curve(brep, edge, curve.type_code())
    }
    fn new_curve(
        &self,
        brep: BrepId,
        edge: EdgeId,
        type_code: i64,
    ) -> solidcore::Result<ObjectRef> {
        Ok(merge_curve(brep, edge, type_code))
    }
    fn source_lists(&self, brep: BrepId, region: RegionId) -> solidcore::Result<[ObjectRef; 2]> {
        Ok(lists_in(
            800_000_000_000 + u64::from(brep.index()) * 1_000_000_000,
            region,
        ))
    }
}

/// The notification stores of one offset call on `source` (`SM_API`'s context keeps them for the
/// process; each call here has its own).
struct Notifications {
    records: NotifyRecords,
    attributes: AttributeStore,
}

impl Notifications {
    /// Registers `source` (B-rep 0 of the source domain), its objects and the reserved result.
    fn new(source: &mut Brep) -> Notifications {
        source.set_key(source_key(EntityRef::Brep(ONE)).key);
        let mut records = NotifyRecords::new();
        records.insert(source.key(), Topology::new());
        records.insert(result_key(EntityRef::Brep(ONE)).key, Topology::new());
        let entities = source
            .shells()
            .iter()
            .map(|(id, _)| EntityRef::Shell(id))
            .chain(source.faces().iter().map(|(id, _)| EntityRef::Face(id)))
            .chain(
                source
                    .faceuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Faceuse(id)),
            )
            .chain(source.edges().iter().map(|(id, _)| EntityRef::Edge(id)))
            .chain(
                source
                    .edgeuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Edgeuse(id)),
            )
            .chain(
                source
                    .vertices()
                    .iter()
                    .map(|(id, _)| EntityRef::Vertex(id)),
            )
            .chain(
                source
                    .vertexuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Vertexuse(id)),
            )
            .chain(
                source
                    .loopuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Loopuse(id)),
            )
            .chain(source.regions().iter().map(|(id, _)| EntityRef::Region(id)));
        for e in entities {
            records.insert(source_key(e).key, Topology::new());
        }
        Notifications {
            records,
            attributes: AttributeStore::default(),
        }
    }

    fn context(&mut self) -> OffsetNotifications<'_> {
        OffsetNotifications {
            records: &mut self.records,
            attributes: &mut self.attributes,
            original_key_of: &source_key,
            result_key_of: &result_key,
            original_curve_ref: &source_curve,
            result_curve_ref: &result_curve,
            boolean_source_lists: &source_lists,
            result_brep: result_key(EntityRef::Brep(ONE)),
            merge_identities: Some(&MergeBodies),
        }
    }
}

/// `SmApiOffsetBrepFull(b, distance, TRUE, TRUE, r)`.
fn offset_brep_full(
    k: &mut Kernel,
    b: &mut Brep,
    distance: f64,
    merge: bool,
) -> solidcore::Result<Brep> {
    let mut n = Notifications::new(b);
    let options = OffsetOptions {
        distance,
        extend_convex_edges: true,
        merge_results: merge,
    };
    api_offset_brep_full(k, Some(b), options, &mut n.context())
}

/// `SmApiBrepCopy` of `b` named in domain `n` (a `NamingScheme::Caller` body with its keys
/// recorded, solidcore#1585), as SolidCraft copies a body before an operation.
fn copy_of(k: &mut Kernel, b: &Brep, n: u64) -> Brep {
    let target = move |e| named(e, n.saturating_mul(100_000_000));
    let mut state = brep_p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target;
    brep_p3::api_brep_copy(k, &mut env, Some(b), &|e| named(e, 0)).unwrap()
}

fn failed_tests(k: &mut Kernel, r: &mut Brep) -> Vec<u64> {
    let check = check_brep(k, r, CheckLevel::Level2).unwrap();
    check
        .pointer_reports
        .reports()
        .iter()
        .chain(check.reports.reports())
        .filter(|x| !x.ok)
        .map(|x| x.test_index)
        .collect()
}

fn volume(k: &Kernel, r: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    solidcore::mass::integrator::properties::brep_volume(k, r, 1e-9, &attrs).unwrap()
}

/// The 20 x 10 x 10 box offset by 5 with convex edges extended is the 30 x 20 x 20
/// box (V = 12000), with and without the self-intersection merge, and checks valid.
#[test]
fn offset_of_a_caller_keyed_box_checks_valid() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0, 10.0).unwrap();
    for merge in [true, false] {
        let mut c = copy_of(&mut k, &a, 1);
        let mut r = offset_brep_full(&mut k, &mut c, 5.0, merge).unwrap();
        let v = volume(&k, &mut r);
        assert!(
            (v - 12000.0).abs() <= 1e-9 * 12000.0,
            "merge {merge}: volume {v}"
        );
        assert_eq!(r.edges().ids().count(), 12, "merge {merge}");
        assert_eq!(
            failed_tests(&mut k, &mut r),
            Vec::<u64>::new(),
            "merge {merge}"
        );
    }
}

/// `SmApiShellBrepFull(sheet, 2, TRUE, TRUE, TRUE)` of a caller-keyed copy of the
/// 10 x 10 sheet: the 10 x 10 x 2 slab (V = 200), checking valid.
#[test]
fn thicken_of_a_caller_keyed_sheet_checks_valid() {
    let mut k = Kernel::default();
    let sheet = solidcore::primitive::api_primitives::create_plane(
        &mut k,
        Point3::new(0.0, 0.0, 0.0),
        10.0,
        10.0,
    )
    .unwrap();
    let mut c = copy_of(&mut k, &sheet, 1);
    let mut n = Notifications::new(&mut c);
    let copy = ShellCopyIdentities {
        key_of: &working_key,
        curve_ref: &working_curve,
        boolean_source_lists: &working_lists,
    };
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: 2.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: false,
    };
    let mut r = api_shell_brep_full(
        &mut k,
        Some(BrepRef {
            brep: &c,
            handle: ONE,
        }),
        options,
        &[],
        &copy,
        &mut n.context(),
    )
    .unwrap();
    let v = volume(&k, &mut r);
    assert!((v - 200.0).abs() <= 1e-9 * 200.0, "volume {v}");
    assert_eq!(failed_tests(&mut k, &mut r), Vec::<u64>::new());
}
