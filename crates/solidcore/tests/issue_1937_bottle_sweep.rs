//! Default parity and opt-in regularization of the bottle sweep (#1937).
#![allow(clippy::unwrap_used)] // integration-test helpers
use solidcore::geom::attribute::ObjectKey;
use solidcore::sweep::topology_sweep::store::SweepIdentities;
use solidcore::topology::{BrepId, EdgeId, RegionId};
use solidcore::{Brep, Result};

use solidcore::geom::attribute::AttributeStore;
use solidcore::sweep::topology_sweep::{
    access::SweepAccess, engine::SweepTargets, geometry::SweepGeometry, store::SweepStore,
    TopologySweep,
};
use solidcore::sweep::trans_sweep_geometry::TranslationalSweepGeometry;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Kernel, Vec3};
#[test]
#[ignore = "requires the pinned upstream bottle.smb fixture"]
fn bottle_sweep_reproduces_native_failures() {
    check_bottle(false);
}

#[test]
#[ignore = "requires the pinned upstream bottle.smb fixture"]
fn bottle_sweep_regularizes_degenerate_output() {
    check_bottle(true);
}

fn check_bottle(regularize: bool) {
    let mut k = Kernel::default();
    k.fixes.translation_sweep_regularize = regularize;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/bottle.smb");
    let bytes = std::fs::read(path).unwrap();
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let key_of = |e| WholeIdentities.topology(BrepId::from_index(0), e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(&mut k, Some(bytes), true, false, &mut env).unwrap();
    b.refresh_cached_topology();
    let before = check_brep(&mut k, &mut b, CheckLevel::Level0).unwrap();
    eprintln!(
        "BEFORE valid={} lines={:?}",
        before.is_valid(),
        before.lines()
    );
    b.set_key(key_of(solidcore::error::EntityRef::Brep(BrepId::from_index(0))).key);
    let identities = WholeIdentities;
    let mut next = 0;
    let mut store = SweepStore::new(&identities, &mut records, &mut attributes, &mut next);
    let context = store.register_kernel(&mut k).unwrap();
    let id = store.register_brep(context, &mut b).unwrap();
    let region = store.infinite_region(id).unwrap();
    let g = SweepGeometry::Translation(TranslationalSweepGeometry::new(Vec3::new(0., 0., 4.)));
    let mut sweep = TopologySweep {
        do_stitching: false,
        do_merge: false,
        repetitions: 1,
        ..TopologySweep::default()
    };
    let result = sweep.do_sweep(
        &mut store,
        &g,
        Some(id),
        Some((id, region)),
        &mut SweepTargets::default(),
    );
    eprintln!("SWEEP {result:?}");
    result.unwrap();
    drop(store);
    let after = check_brep(&mut k, &mut b, CheckLevel::Level0).unwrap();
    eprintln!("AFTER valid={} lines={:?}", after.is_valid(), after.lines());
    assert_eq!(before.pointers_status, 1000);
    assert_eq!(before.assert_valid, Some(true));
    assert_eq!(before.reports.reports().len(), 8);
    if regularize {
        assert_eq!(after.pointers_status, 1000);
        assert!(after.pointer_reports.reports().is_empty());
        // These three reports are inherited input gaps, not sweep defects.
        let inherited: Vec<_> = before
            .reports
            .reports()
            .iter()
            .filter(|r| r.test_index == 4 && r.value > 1.9e-5 && r.value < 2.1e-5)
            .map(|r| (r.reporting_type, r.test_index, r.value, r.tol3d))
            .collect();
        assert_eq!(
            after
                .reports
                .reports()
                .iter()
                .map(|r| (r.reporting_type, r.test_index, r.value, r.tol3d))
                .collect::<Vec<_>>(),
            inherited
        );
        assert_eq!(after.assert_valid, Some(true));
        assert_eq!(solidcore::topology::brep::p1::num_faces(&b), 84);
        assert_eq!(
            solidcore::topology::brep::p1::edges(&b, None)
                .unwrap()
                .len(),
            317
        );
        assert_eq!(
            solidcore::topology::brep::p1::regions(&b, None)
                .unwrap()
                .len(),
            2
        );
        assert!(check_brep(&mut k, &mut b, CheckLevel::Level0)
            .unwrap()
            .is_valid());
        for entity in sweep
            .to_same
            .values()
            .chain(sweep.to_higher.values())
            .chain(sweep.from_same.keys())
            .chain(sweep.from_lower.keys())
            .flatten()
        {
            use solidcore::sweep::topology_sweep::SweepEntity;
            match *entity {
                SweepEntity::Face(owner, id) if owner == id_body() => {
                    b.face(id).unwrap();
                }
                SweepEntity::Edge(owner, id) if owner == id_body() => {
                    b.edge(id).unwrap();
                }
                SweepEntity::Vertex(owner, id) if owner == id_body() => {
                    b.vertex(id).unwrap();
                }
                SweepEntity::Region(owner, id) if owner == id_body() => {
                    b.region(id).unwrap();
                }
                _ => {}
            }
        }
        let full = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert_eq!(full.assert_valid, Some(false));
        assert_eq!(full.reports.reports().len(), 1);
        let gap = full.reports.reports().first().unwrap();
        assert_eq!((gap.reporting_type, gap.test_index), (16021, 1));
        assert!((gap.value - 4.7135458964378874e-5).abs() < 1e-12);
        assert_eq!(gap.tol3d, 2e-5);
        return;
    }
    assert_eq!(after.pointers_status, 1000);
    assert_eq!(after.assert_valid, Some(false));
    assert_eq!(after.reports.reports().len(), 6);
    let introduced: Vec<_> = after
        .reports
        .reports()
        .iter()
        .filter(|r| r.reporting_type != 16011)
        .map(|r| (r.reporting_type, r.test_index))
        .collect();
    assert_eq!(introduced, [(15001, 1), (15001, 6), (12225, 2)]);
    // Validation populates derived UV curves. A second check of the unchanged body
    // exposes an additional loop-orientation failure in pointer validation.
    let repeated = check_brep(&mut k, &mut b, CheckLevel::Level0).unwrap();
    eprintln!("REPEATED {:?}", repeated.lines());
    assert_eq!(repeated.pointers_status, 1000);
    assert_eq!(repeated.assert_valid, None);
    assert_eq!(
        repeated
            .pointer_reports
            .reports()
            .iter()
            .map(|r| (r.reporting_type, r.test_index))
            .collect::<Vec<_>>(),
        [(12225, 2), (12225, 3), (12225, 4)]
    );
}

fn id_body() -> BrepId {
    BrepId::from_index(0)
}

type BoxSweep = (usize, usize, usize, Vec<[f64; 3]>, Vec<String>);

fn sweep_box(regularize: bool, vector: Vec3, height: f64, tilt: f64) -> BoxSweep {
    let mut k = Kernel::default();
    k.fixes.translation_sweep_regularize = regularize;
    let mut b =
        solidcore::primitive::api_primitives::create_box(&mut k, Vec3::ZERO, 1.0, 1.0, height)
            .unwrap();
    if tilt != 0.0 {
        let mut frame = solidcore::geom::axis2_placement::Axis2Placement::IDENTITY;
        frame.rotate_about_axis(tilt, Vec3::Y);
        solidcore::topology::brep::p5::transform(&mut k, &mut b, &frame, None, false).unwrap();
    }
    b.set_key(
        WholeIdentities
            .topology(id_body(), solidcore::EntityRef::Brep(id_body()))
            .key,
    );
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    // Read with the same owner identities used by the borrowed sweep adapter.
    let bytes = write_body(&k, &b);
    let key_of = |e| WholeIdentities.topology(id_body(), e);
    b = read_brep_from_file(
        &mut k,
        Some(bytes),
        true,
        false,
        &mut TopologyEnv {
            all_attributes: &[],
            attributes: &mut attributes,
            key_of: &key_of,
            records: &mut records,
            next_key: 1 << 50,
        },
    )
    .unwrap();
    b.refresh_cached_topology();
    b.set_key(key_of(solidcore::EntityRef::Brep(id_body())).key);
    let baseline = check_brep(&mut k, &mut b, CheckLevel::Level0).unwrap();
    assert!(baseline.is_valid(), "{:?}", baseline.lines());
    let mut next = 0;
    let mut store = SweepStore::new(&WholeIdentities, &mut records, &mut attributes, &mut next);
    let context = store.register_kernel(&mut k).unwrap();
    let id = store.register_brep(context, &mut b).unwrap();
    let region = store.infinite_region(id).unwrap();
    let mut sweep = TopologySweep {
        do_stitching: false,
        do_merge: false,
        ..TopologySweep::default()
    };
    sweep
        .do_sweep(
            &mut store,
            &SweepGeometry::Translation(TranslationalSweepGeometry::new(vector)),
            Some(id),
            Some((id, region)),
            &mut SweepTargets::default(),
        )
        .unwrap();
    drop(store);
    let check = check_brep(&mut k, &mut b, CheckLevel::Level0).unwrap();
    let mut points: Vec<_> = solidcore::topology::brep::p1::vertices(&b, None)
        .unwrap()
        .into_iter()
        .map(|id| {
            let p = b.vertex(id).unwrap().point;
            [p.x, p.y, p.z]
        })
        .collect();
    points.sort_by(|a, b| {
        a.iter()
            .zip(b)
            .map(|(x, y)| x.total_cmp(y))
            .find(|order| !order.is_eq())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    (
        solidcore::topology::brep::p1::num_faces(&b),
        solidcore::topology::brep::p1::edges(&b, None)
            .unwrap()
            .len(),
        solidcore::topology::brep::p1::regions(&b, None)
            .unwrap()
            .len(),
        points,
        check.lines(),
    )
}

#[test]
fn regularization_preserves_ordinary_box_sweeps() {
    for vector in [Vec3::new(0.0, 0.0, 4.0), Vec3::new(1.0, 2.0, 3.0)] {
        let native = sweep_box(false, vector, 1.0, 0.0);
        let fixed = sweep_box(true, vector, 1.0, 0.0);
        assert_eq!(fixed, native);
        assert!(fixed.4.is_empty(), "{:?}", fixed.4);
        assert!(fixed.0 >= 6);
    }
}

#[test]
fn regularization_handles_near_parallel_box_sweep() {
    let native = sweep_box(false, Vec3::new(0.0, 0.0, 4.0), 0.07, 0.0000268);
    let fixed = sweep_box(true, Vec3::new(0.0, 0.0, 4.0), 0.07, 0.0000268);
    assert!(native.0 >= 6);
    assert!(fixed.4.is_empty(), "{:?}", fixed.4);
    assert!(fixed.0 >= 6);
}

#[test]
fn near_parallel_generator_returns_invalid_surface() {
    use solidcore::curve::{curve::Curve, line::Line};
    use solidcore::geom::{assert_array::AssertArray, extent1d::Extent1d};
    let mut k = Kernel::default();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let c = Curve::from(
        Line::create_line_segment(
            3,
            Vec3::new(-0.30118110236220502, 0., 4.3331526241915306),
            Vec3::new(-0.30118297798525001, 0., 4.2633059315015203),
            None,
        )
        .unwrap(),
    );
    let edge = solidcore::topology::brep::p5::create_wire_edge_from_curve(
        &mut k,
        &mut b,
        c,
        Extent1d::from_min_max(0., 1.),
    )
    .unwrap();
    let mut g = TranslationalSweepGeometry::new(Vec3::new(0., 0., 4.));
    g.base.make_analytics = false;
    let result = g.edge_sweep_higher(&k, &b, edge);
    eprintln!("REDUCED sweep={:?}", result.as_ref().map(|_| ()));
    let swept = result.unwrap();
    {
        let mut reports = AssertArray::new();
        let valid = swept
            .surface
            .assert_valid(
                &k,
                Some(&mut reports),
                0,
                solidcore::geom::assert_array::ObjectKey(1),
            )
            .unwrap();
        eprintln!("REDUCED valid={valid} reports={reports:?}");
        assert!(!valid);
        assert_eq!(
            reports
                .reports()
                .iter()
                .map(|r| (r.reporting_type, r.test_index))
                .collect::<Vec<_>>(),
            [(15001, 1), (15001, 6)]
        );
    }
}

struct WholeIdentities;
impl SweepIdentities for WholeIdentities {
    fn topology(
        &self,
        b: BrepId,
        e: solidcore::error::EntityRef,
    ) -> solidcore::geom::object::ObjectRef {
        use solidcore::error::EntityRef as R;
        // (kind, index, native type code)
        let (kind, index, type_code) = match e {
            R::Brep(i) => (0_u64, i.index(), 16011),
            R::Shell(i) => (1, i.index(), 16013),
            R::Face(i) => (2, i.index(), 16002),
            R::Loop(i) => (3, i.index(), 16003),
            R::Edge(i) => (4, i.index(), 16016),
            R::Vertex(i) => (5, i.index(), 16021),
            R::Region(i) => (6, i.index(), 16012),
            R::Faceuse(i) => (7, i.index(), 16022),
            R::Loopuse(i) => (8, i.index(), 16004),
            R::Edgeuse(i) => (9, i.index(), 16005),
            R::Vertexuse(i) => (10, i.index(), 16006),
        };
        solidcore::geom::object::ObjectRef {
            type_code,
            key: ObjectKey((u64::from(b.index()) << 40) | (kind << 32) | u64::from(index)),
        }
    }
    fn curve(
        &self,
        b: BrepId,
        e: EdgeId,
        _: &solidcore::curve::curve::Curve,
    ) -> solidcore::geom::object::ObjectRef {
        solidcore::geom::object::ObjectRef {
            type_code: 11001,
            key: ObjectKey((u64::from(b.index()) << 40) | (20 << 32) | u64::from(e.index())),
        }
    }
    /// A curve the stitch allocates when it splits an edge (`MakeVertexSplitEdge`'s
    /// curve copy) is named, as every edge curve here, by its edge: the native copy
    /// is a new object of its own, distinct from every other curve.
    fn new_curve(
        &self,
        b: BrepId,
        e: EdgeId,
        type_code: i64,
    ) -> Result<solidcore::geom::object::ObjectRef> {
        Ok(solidcore::geom::object::ObjectRef {
            type_code,
            key: ObjectKey((u64::from(b.index()) << 40) | (20 << 32) | u64::from(e.index())),
        })
    }
    fn owned_auxiliary_objects(&self, _: BrepId, _: &Brep) -> Vec<ObjectKey> {
        Vec::new()
    }
    /// The merge's two source lists of a primary region: handles outside every
    /// topology and curve kind above.
    fn source_lists(
        &self,
        b: BrepId,
        r: RegionId,
    ) -> Result<[solidcore::geom::object::ObjectRef; 2]> {
        Ok([21_u64, 22].map(|kind| solidcore::geom::object::ObjectRef {
            type_code: 0,
            key: ObjectKey((u64::from(b.index()) << 40) | (kind << 32) | u64::from(r.index())),
        }))
    }
}

fn write_body(k: &Kernel, b: &Brep) -> Vec<u8> {
    use solidcore::topology::brep_data_write::FromBrepEnv;
    let attributes = AttributeStore::new();
    let records = NotifyRecords::new();
    let own = |e| b.owner_identity(e);
    let env = FromBrepEnv {
        attributes: &attributes,
        key_of: &own,
        records: &records,
        curve_key: &|_| ObjectKey(0),
        uv_curve_key: &|_| ObjectKey(0),
        surface_key: &|_| ObjectKey(0),
    };
    solidcore::topology::api_files::write_brep_to_file(k, Some(b), &env, true).unwrap()
}
