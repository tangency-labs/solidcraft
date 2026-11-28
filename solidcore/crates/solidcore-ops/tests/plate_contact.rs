//! Plate contact through ops on fixture F of the Tab and Slot design: `Bottom`
//! (0,0,0)–(160,100,6) and `Wall` (20,30,6)–(140,36,66), P Bottom's top (z = 6, n = +z).
#![allow(clippy::unwrap_used)]

use solidcore::math::Vec3;
use solidcore::plate_contact::{Contact, PairSkip, Plane};
use solidcore_ops::make_box;
use solidcore_ops::session;

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

const TOL: f64 = 1.0e-3;

fn plane(at: [f64; 3], n: Vec3) -> Plane {
    Plane::through(solidcore::Point3::new(at[0], at[1], at[2]), n).unwrap()
}

/// Wall on Bottom: a touch, Wall's y pair, 6 thick; neither body changes.
#[test]
fn wall_on_bottom_is_a_joint() {
    all_fixes();
    let bottom = make_box([0.0, 0.0, 0.0], [160.0, 100.0, 6.0]).unwrap();
    let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
    let p = plane([0.0, 0.0, 6.0], Vec3::Z);
    let got = wall.plate_pair(&bottom, p, TOL).unwrap().unwrap();
    assert_eq!(got.contact, Contact::Touch { delta: 0.0 });
    assert!((got.faces.thickness - 6.0).abs() < 1e-9, "{got:?}");
    assert_eq!(wall.counts().unwrap(), [6, 12, 8]);
    assert_eq!(bottom.counts().unwrap(), [6, 12, 8]);
    assert!(wall.plate_pair(&bottom, p, 0.0).is_err());
}

/// Bottom as the tab plate on Wall's foot (z = 6, n = −z): Bottom lies face to face on the
/// plane's side, no pair; the reverse (Wall on Bottom's top) is a joint: swapped.
#[test]
fn bottom_under_wall_is_swapped() {
    all_fixes();
    let bottom = make_box([0.0, 0.0, 0.0], [160.0, 100.0, 6.0]).unwrap();
    let wall = make_box([20.0, 30.0, 6.0], [120.0, 6.0, 60.0]).unwrap();
    let foot = plane([0.0, 0.0, 6.0], -Vec3::Z);
    let top = plane([0.0, 0.0, 6.0], Vec3::Z);
    let forward = bottom.plate_pair(&wall, foot, TOL).unwrap();
    assert!(forward.is_err(), "{forward:?}");
    assert_eq!(
        bottom.plate_pair_swapped(&wall, foot, top, TOL).unwrap(),
        Err(PairSkip::Swapped)
    );
}
