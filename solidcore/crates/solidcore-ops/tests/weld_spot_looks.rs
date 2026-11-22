//! The named spot-weld looks (`solidcore_ops::weld::spot`): each builds through the kernel's
//! `spot_weld`, every body passes the B-rep check, the ball looks are refused on three
//! sheets, and the default is the marks alone.
#![allow(clippy::unwrap_used)]

use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::math::{Point3, Vec3};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::weld::spot::{proposed_spot_diameter, spot_weld, SpotZone};
use solidcore::SolidError;
use solidcore_ops::weld::spot::{SpotLook, SpotWeld, MARKS};

fn spot(look: SpotLook, thicknesses: &[f64]) -> SpotWeld {
    SpotWeld {
        at: Point3::new(0.0, 0.0, 0.0),
        normal: Vec3::new(0.0, 0.0, 1.0),
        thicknesses: thicknesses.to_vec(),
        diameter: proposed_spot_diameter(thicknesses).unwrap(),
        style: look.style(),
    }
}

#[test]
fn every_look_builds_and_its_bodies_pass_the_check() {
    for look in SpotLook::ALL {
        for (t_u, t_l) in [(1.0, 1.5), (8.0, 8.0)] {
            let mut k = Kernel::new(&KernelOptions::default());
            let pieces = spot_weld(&mut k, &spot(look, &[t_l, t_u])).unwrap();
            assert!(!pieces.is_empty(), "{look:?}");
            // every look shows on both outer faces, the ball's included
            let marks = pieces.iter().filter(|p| p.zone == SpotZone::Core).count();
            let ball_shows = look == SpotLook::Ball && t_u < 2.0;
            assert_eq!(
                marks,
                if ball_shows { 0 } else { 2 },
                "{look:?} on {t_u}/{t_l}"
            );
            for mut piece in pieces {
                let lines = check_brep(&mut k, &mut piece.body, CheckLevel::Level2)
                    .unwrap()
                    .lines();
                assert!(lines.is_empty(), "{look:?}: {lines:?}");
            }
        }
    }
}

/// On three sheets the looks without a ball build, marks on both outer faces; the ball looks
/// are refused as invalid input.
#[test]
fn on_three_sheets_only_the_looks_without_a_ball_build() {
    for look in SpotLook::ALL {
        let mut k = Kernel::new(&KernelOptions::default());
        let got = spot_weld(&mut k, &spot(look, &[1.0, 1.5, 1.0]));
        if look.style().ball.is_some() {
            assert!(
                matches!(got, Err(SolidError::InvalidInput { .. })),
                "{look:?}"
            );
        } else {
            let marks = got
                .unwrap()
                .iter()
                .filter(|p| p.zone == SpotZone::Core)
                .count();
            assert_eq!(marks, 2, "{look:?}");
        }
    }
}

#[test]
fn the_default_look_is_the_marks_alone() {
    let style = SpotLook::DEFAULT.style();
    assert_eq!(SpotLook::DEFAULT, SpotLook::Marks);
    assert_eq!(style.marks, Some(MARKS));
    assert!(style.ball.is_none() && style.pin.is_none());
}
