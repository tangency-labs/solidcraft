//! A tool copied (`SmApiBrepCopy`), moved (`SmApiTranslate`) and cut again, as a
//! patterned cut does (SolidCraft #1717): the copy's surfaces carry their faces'
//! tolerances, and the second cut's curve classification makes the moved face's
//! trimmed surface cache on the lent B-rep instead of deferring.
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::topology::brep::{p1, p2, p3};
use solidcore::topology::proto_topology::p1::api_transform::{api_translate, TransformObject};
use solidcore::{Brep, Kernel};

fn copy(k: &mut Kernel, b: &Brep, n: u64) -> solidcore::Result<Brep> {
    let named = |entity, domain: u64| {
        let r = p3::topology_ref(0, entity);
        ObjectRef {
            type_code: r.type_code,
            key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
        }
    };
    let target_key = |entity| named(entity, n * 100_000_000);
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target_key;
    p3::api_brep_copy(k, &mut env, Some(b), &|entity| named(entity, 0))
}

fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs)
}

#[test]
fn copied_faces_keep_their_tolerance_on_their_surfaces() {
    let mut k = Kernel::default();
    let ball = create_sphere(&mut k, Point3::new(12.0, 20.0, 0.0), 10.0).unwrap();
    let copied = copy(&mut k, &ball, 1).unwrap();
    for f in p1::faces(&copied, None).unwrap() {
        let face = copied.face(f).unwrap();
        let surface = face.surface.as_ref().unwrap();
        assert_eq!(surface.owner_zone_tol_3d(), Some(face.tolerance));
    }
}

#[test]
fn patterned_sphere_cut_matches_two_separate_cuts() {
    let mut k = Kernel::default();
    let mut patterned = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 5.0).unwrap();
    let ball = create_sphere(&mut k, Point3::new(12.0, 20.0, 0.0), 10.0).unwrap();
    for i in 0..2_u32 {
        let mut tool = copy(&mut k, &ball, u64::from(i) + 1).unwrap();
        let shift = Vec3::new(5.0 * f64::from(i), 0.0, 0.0);
        api_translate(&mut k, Some(TransformObject::Brep(&mut tool)), shift).unwrap();
        patterned = boolean_difference(&mut k, patterned, tool).unwrap();
    }

    let mut separate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 5.0).unwrap();
    for x in [12.0, 17.0] {
        let tool = create_sphere(&mut k, Point3::new(x, 20.0, 0.0), 10.0).unwrap();
        separate = boolean_difference(&mut k, separate, tool).unwrap();
    }

    assert_eq!(
        p1::num_faces(&patterned),
        p1::num_faces(&separate),
        "same faces"
    );
    let a = volume(&k, &mut patterned).unwrap();
    let b = volume(&k, &mut separate).unwrap();
    assert!((a - b).abs() <= 1e-9 * a.abs().max(b.abs()), "{a} vs {b}");
}
