//! Shared fixture of the shell / offset issue probes (`#[path]`-included by each
//! `issue_<n>_*.rs`): the caller-owned notification identities `OffsetExecutive`
//! needs, shell and whole-offset wrappers, and the measurements the probes report.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    dead_code
)] // test helpers; each probe uses a subset

use solidcore::curve::curve::Curve;
use solidcore::geom::a_object::BREP_TYPE;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{
    api_offset_brep_full, OffsetNotifications, OffsetOptions, ShellOptions,
};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::sweep::topology_sweep::store::SweepIdentities;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::topology::Topology;
use solidcore::topology::{Region, RegionId, Shell};
use solidcore::{Brep, BrepId, EdgeId, EntityRef, FaceId, Kernel};

fn fixture_key(entity: EntityRef, namespace: u64) -> ObjectRef {
    let (index, type_code) = match entity {
        EntityRef::Brep(id) => (id.index(), BREP_TYPE),
        EntityRef::Shell(id) => (id.index(), Shell::type_code()),
        EntityRef::Face(id) => (id.index(), solidcore::topology::face::p2::TYPE),
        EntityRef::Faceuse(id) => (id.index(), 16022),
        EntityRef::Edge(id) => (id.index(), solidcore::topology::edge::TYPE),
        EntityRef::Edgeuse(id) => (id.index(), solidcore::topology::edgeuse::p1::TYPE),
        EntityRef::Vertex(id) => (id.index(), solidcore::topology::vertex::TYPE),
        EntityRef::Vertexuse(id) => (id.index(), solidcore::topology::vertexuse::TYPE),
        EntityRef::Loopuse(id) => (id.index(), solidcore::topology::loopuse::TYPE),
        EntityRef::Region(id) => (id.index(), Region::type_code()),
        EntityRef::Loop(id) => (id.index(), solidcore::topology::topology::TYPE),
    };
    ObjectRef {
        key: ObjectKey(
            namespace + u64::try_from(type_code).unwrap() * 1_000_000 + u64::from(index),
        ),
        type_code,
    }
}

fn original_key(entity: EntityRef) -> ObjectRef {
    fixture_key(entity, 100_000_000_000)
}

fn result_key(entity: EntityRef) -> ObjectRef {
    fixture_key(entity, 200_000_000_000)
}

fn result_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(300_000_000_000 + u64::from(edge.index())),
        type_code,
    }
}

fn original_curve(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(400_000_000_000 + u64::from(edge.index())),
        type_code,
    }
}

fn boolean_source_lists(result: bool, region: RegionId) -> [ObjectRef; 2] {
    [0, 1].map(|list| ObjectRef {
        key: ObjectKey(
            500_000_000_000
                + u64::from(result) * 10_000_000_000
                + u64::from(region.index()) * 2
                + list,
        ),
        type_code: 0,
    })
}

fn merge_curve(brep: BrepId, edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(
            700_000_000_000 + u64::from(brep.index()) * 1_000_000_000 + u64::from(edge.index()),
        ),
        type_code,
    }
}

/// Identities of the merge's new B-reps, disjoint from the keys above.
struct MergeBodies;

impl SweepIdentities for MergeBodies {
    fn topology(&self, brep: BrepId, entity: EntityRef) -> ObjectRef {
        fixture_key(
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
        Ok([0, 1].map(|list| ObjectRef {
            key: ObjectKey(
                800_000_000_000
                    + u64::from(brep.index()) * 1_000_000_000
                    + u64::from(region.index()) * 2
                    + list,
            ),
            type_code: 0,
        }))
    }
}

/// The notification stores of one offset run on `original`.
pub struct Notifications {
    records: NotifyRecords,
    attributes: AttributeStore,
}

impl Notifications {
    /// Registers `original` (keyed as B-rep 0 of the original namespace) and the
    /// reserved result B-rep.
    pub fn new(original: &mut Brep) -> Self {
        original.set_key(original_key(EntityRef::Brep(BrepId::from_index(0))).key);
        let mut records = NotifyRecords::new();
        records.insert(original.key(), Topology::new());
        records.insert(
            result_key(EntityRef::Brep(BrepId::from_index(0))).key,
            Topology::new(),
        );
        let entities = original
            .shells()
            .iter()
            .map(|(id, _)| EntityRef::Shell(id))
            .chain(original.faces().iter().map(|(id, _)| EntityRef::Face(id)))
            .chain(
                original
                    .faceuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Faceuse(id)),
            )
            .chain(original.edges().iter().map(|(id, _)| EntityRef::Edge(id)))
            .chain(
                original
                    .edgeuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Edgeuse(id)),
            )
            .chain(
                original
                    .vertices()
                    .iter()
                    .map(|(id, _)| EntityRef::Vertex(id)),
            )
            .chain(
                original
                    .vertexuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Vertexuse(id)),
            )
            .chain(
                original
                    .loopuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Loopuse(id)),
            )
            .chain(
                original
                    .regions()
                    .iter()
                    .map(|(id, _)| EntityRef::Region(id)),
            );
        for entity in entities {
            records.insert(original_key(entity).key, Topology::new());
        }
        Self {
            records,
            attributes: AttributeStore::default(),
        }
    }

    /// The executive's view of the stores.
    pub fn context(&mut self) -> OffsetNotifications<'_> {
        OffsetNotifications {
            records: &mut self.records,
            attributes: &mut self.attributes,
            original_key_of: &original_key,
            result_key_of: &result_key,
            original_curve_ref: &original_curve,
            result_curve_ref: &result_curve,
            boolean_source_lists: &boolean_source_lists,
            result_brep: result_key(EntityRef::Brep(BrepId::from_index(0))),
            merge_identities: Some(&MergeBodies),
        }
    }
}

/// `SmApiShellBrepFull` on `part` (extended convex edges, merged results), `open`
/// faces removed (empty: a closed shell).
pub fn shell(
    k: &mut Kernel,
    mut part: Brep,
    distance: f64,
    open: &[FaceId],
) -> solidcore::Result<Brep> {
    let mut notifications = Notifications::new(&mut part);
    OffsetExecutive::shell_brep(
        k,
        part,
        ShellOptions {
            offset: OffsetOptions {
                distance,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: false,
            merge_sheet_side_walls: false,
        },
        open,
        &mut notifications.context(),
    )
    .result
    .map(|mut b| {
        // as `api_shell_brep_full` does for its caller: the port's cached topology fields
        b.refresh_cached_topology();
        b
    })
}

/// `SmApiOffsetBrepFull` on `part` (extended convex edges, merged results).
pub fn offset_full(k: &mut Kernel, part: &mut Brep, distance: f64) -> solidcore::Result<Brep> {
    let mut notifications = Notifications::new(part);
    api_offset_brep_full(
        k,
        Some(part),
        OffsetOptions {
            distance,
            extend_convex_edges: true,
            merge_results: true,
        },
        &mut notifications.context(),
    )
}

/// The body's volume (`SmApiBrepComputeVolume`, accuracy 1e-6).
pub fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs)
}

/// The plane faces of `b` whose middle lies at `value` along axis `axis` (0 x, 1 y, 2 z).
pub fn planar_faces_at(k: &Kernel, b: &Brep, axis: usize, value: f64) -> Vec<FaceId> {
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let Some(s) = face.surface.as_ref() else {
                return false;
            };
            if !s.is_kind_of(Plane::TYPE).unwrap() {
                return false;
            }
            let p = s.evaluate_point(k, face.uv_domain.mid()).unwrap();
            ([p.x, p.y, p.z][axis] - value).abs() < 1e-6
        })
        .collect()
}

/// What a probe reports about a result: status, topology integrity, manifold solid,
/// face count, volume against the closed form (when there is one). Printed for the hand-off; returns
/// whether the result matches the oracle (an intact manifold solid of the expected
/// volume to `rel` relative).
pub fn report(
    k: &Kernel,
    label: &str,
    result: solidcore::Result<Brep>,
    want: Option<f64>,
    rel: f64,
) -> bool {
    match result {
        Err(e) => {
            eprintln!("PROBE {label}: Err({e:?})");
            false
        }
        Ok(mut b) => {
            let integrity = solidcore::topology::integrity::check(&b);
            let manifold = brep_is_manifold_solid(&b);
            let faces = b.face_list().len();
            let got = volume(k, &mut b);
            eprintln!(
                "PROBE {label}: Ok; integrity={integrity:?} manifold={manifold:?} faces={faces} edges={} vertices={} volume={got:?} want={want:?}",
                b.edge_list().len(),
                b.vertex_list().len()
            );
            let volume_ok = match (want, &got) {
                (Some(w), Ok(g)) => (g - w).abs() <= rel * w.abs(),
                (None, Ok(_)) => true,
                _ => false,
            };
            integrity.is_ok() && matches!(manifold, Ok(true)) && volume_ok
        }
    }
}
