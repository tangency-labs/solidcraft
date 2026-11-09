//! Regression test: `Surface::assert_valid` runs `SmOffsetSurface::AssertValid` and
//! `SmSrfInVolume::AssertValid` (with the volume checks it nests into) instead of
//! returning `Unported`.

#![allow(clippy::unwrap_used)] // test helpers

use solidcore::geom::assert_array::labels::{
    BEND_VOLUME_TYPE, OFFSET_SURFACE_TYPE, SRF_IN_VOLUME_TYPE, VOLUME_TYPE,
};
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::surface::bend_volume::BendVolume;
use solidcore::surface::offset_surface::OffsetSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::srf_in_volume::SrfInVolume;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::{Volume, VolumeKind};

const LEVEL_GIVEN: u8 = 4;

fn plane() -> Surface {
    Surface::from(Plane::from_normal(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ))
}

/// `(reporting type, test index)` of every report.
fn run(k: &Kernel, s: &Surface, level: u8) -> (bool, Vec<(i64, u64)>) {
    let mut list = AssertArray::new();
    let ok = s
        .assert_valid(k, Some(&mut list), level, ObjectKey(1))
        .unwrap();
    let ids = list
        .reports()
        .iter()
        .map(|r| (r.reporting_type, r.test_index))
        .collect();
    (ok, ids)
}

#[test]
fn offset_surface_owner_and_self_intersection_tests() {
    let k = Kernel::default();
    // an owning offset of a plane is valid (no self-intersection, base owned)
    let owning = Surface::from(OffsetSurface::new(&k, 0.5, plane(), true, 0).unwrap());
    assert_eq!(run(&k, &owning, LEVEL_GIVEN), (true, vec![]));

    // a non-owning offset fails test 1 (its base's owner is not the offset)
    let shared = Surface::from(OffsetSurface::new(&k, 0.5, plane(), false, 0).unwrap());
    assert_eq!(
        run(&k, &shared, LEVEL_GIVEN),
        (false, vec![(OFFSET_SURFACE_TYPE, 1)])
    );
}

#[test]
fn srf_in_volume_owner_flags_and_volume_checks() {
    let k = Kernel::default();
    // plain SmVolume, owning both parts: valid
    let owned = SrfInVolume::new(&k, plane(), false, Volume::new(VolumeKind::Base), 0, 3).unwrap();
    assert_eq!(run(&k, &Surface::from(owned), LEVEL_GIVEN), (true, vec![]));

    // owning nothing: tests 2..5 fail (owner and context are only checked when owned)
    let shared = SrfInVolume::new(&k, plane(), false, Volume::new(VolumeKind::Base), 0, 0).unwrap();
    assert_eq!(
        run(&k, &Surface::from(shared), LEVEL_GIVEN),
        (
            false,
            vec![
                (SRF_IN_VOLUME_TYPE, 2),
                (SRF_IN_VOLUME_TYPE, 3),
                (SRF_IN_VOLUME_TYPE, 4),
                (SRF_IN_VOLUME_TYPE, 5),
            ]
        )
    );

    // a default bend (neutral distance 0 below its singularity tolerance) fails the
    // bend's test 5, so the volume (test 1) fails; a valid next map changes nothing
    let mut bend = Volume::new(VolumeKind::Bend(BendVolume::new()));
    let (ok, ids) = {
        let s = SrfInVolume::new(&k, plane(), false, bend.clone(), 0, 3).unwrap();
        run(&k, &Surface::from(s), LEVEL_GIVEN)
    };
    assert!(!ok);
    assert!(ids.contains(&(BEND_VOLUME_TYPE, 5)), "{ids:?}");
    assert!(ids.contains(&(SRF_IN_VOLUME_TYPE, 1)), "{ids:?}");
    assert!(!ids.contains(&(VOLUME_TYPE, 0)), "{ids:?}");

    // the bend as the next map of a plain volume: SmVolume test 0 (contained next map)
    let mut head = Volume::new(VolumeKind::Base);
    head.add_next_map(
        std::mem::replace(&mut bend, Volume::new(VolumeKind::Base)),
        2,
    )
    .unwrap();
    let s = SrfInVolume::new(&k, plane(), false, head, 0, 3).unwrap();
    let (ok, ids) = run(&k, &Surface::from(s), LEVEL_GIVEN);
    assert!(!ok);
    assert!(ids.contains(&(BEND_VOLUME_TYPE, 5)), "{ids:?}");
    assert!(ids.contains(&(VOLUME_TYPE, 0)), "{ids:?}");
    assert!(ids.contains(&(SRF_IN_VOLUME_TYPE, 1)), "{ids:?}");
}
