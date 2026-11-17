//! Spot welds (`weld::spot`): every look's bodies against their closed-form volumes, the
//! placement at any spot and normal, and the refusals.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::f64::consts::PI;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::topology::Brep;
use solidcore::weld::spot::{
    proposed_spot_diameter, spot_weld, SpotLook, SpotPiece, SpotSide, SpotWeld, SpotZone, CORE,
    DENT_MIDDLE, DENT_RIM, EDGE, MARK_THICKNESS, PIN_DISH, SMALL_BALL,
};
use solidcore::SolidError;

const T_U: f64 = 1.0;
const T_L: f64 = 1.5;
const D: f64 = 5.0;

fn kernel() -> Kernel {
    Kernel::new(&KernelOptions::default())
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

fn spot(look: SpotLook) -> SpotWeld {
    SpotWeld {
        at: Point3::new(0.0, 0.0, 0.0),
        normal: Vec3::new(0.0, 0.0, 1.0),
        upper_thickness: T_U,
        lower_thickness: T_L,
        diameter: D,
        look,
    }
}

fn close(got: f64, want: f64) {
    assert!(
        (got - want).abs() <= 1e-6 * want.abs().max(1.0),
        "volume {got} != {want}"
    );
}

/// A disc or ring of diameters `a d`..`b d` and `height`.
fn ring_volume(a: f64, b: f64, height: f64) -> f64 {
    PI / 4.0 * (b * b - a * a) * D * D * height
}

/// The dent zone between radii `a` and `b`: the volume under the dished top.
fn dent_volume(a: f64, b: f64) -> f64 {
    let (rim, middle) = (DENT_RIM * D, DENT_MIDDLE * D);
    let ro = EDGE * D / 2.0;
    let c = rim - middle;
    let rs = (ro * ro + c * c) / (2.0 * c);
    PI * (middle + rs) * (b * b - a * a)
        + 2.0 * PI / 3.0 * ((rs * rs - b * b).powf(1.5) - (rs * rs - a * a).powf(1.5))
}

fn marks_volume() -> f64 {
    ring_volume(0.0, EDGE, MARK_THICKNESS * D)
}

fn zones(pieces: &[SpotPiece], side: SpotSide) -> Vec<SpotZone> {
    pieces
        .iter()
        .filter(|p| p.side == side)
        .map(|p| p.zone)
        .collect()
}

#[test]
fn the_marks_lie_on_both_outer_faces_as_core_band_and_edge() {
    let mut k = kernel();
    let mut pieces = spot_weld(&mut k, &spot(SpotLook::Marks)).unwrap();
    let three = vec![SpotZone::Core, SpotZone::Band, SpotZone::Edge];
    assert_eq!(zones(&pieces, SpotSide::Upper), three);
    assert_eq!(zones(&pieces, SpotSide::Lower), three);
    assert_eq!(pieces.len(), 6);
    let h = MARK_THICKNESS * D;
    let want = [
        ring_volume(0.0, CORE, h),
        ring_volume(CORE, 1.0, h),
        ring_volume(1.0, EDGE, h),
    ];
    for (i, piece) in pieces.iter_mut().enumerate() {
        close(volume(&k, &mut piece.body), want[i % 3]);
    }
}

#[test]
fn every_look_has_its_closed_form_volume() {
    let r = D / 2.0;
    let pin_dish = |c: f64| PI * c * (3.0 * r * r + c * c) / 6.0;
    let cases: [(SpotLook, f64); 6] = [
        (SpotLook::Marks, 2.0 * marks_volume()),
        (SpotLook::Ball, 4.0 / 3.0 * PI * r.powi(3)),
        (
            SpotLook::MarksSmallBall,
            2.0 * marks_volume() + 4.0 / 3.0 * PI * (SMALL_BALL * r).powi(3),
        ),
        (
            SpotLook::MarksThinPin,
            2.0 * marks_volume() + PI * (CORE * r).powi(2) * (T_U + T_L),
        ),
        (
            SpotLook::MarksPinConcaveEnds,
            2.0 * marks_volume() + PI * r * r * (T_U + T_L)
                - 2.0 * pin_dish(PIN_DISH * T_U.min(T_L)),
        ),
        (SpotLook::Dent, 2.0 * dent_volume(0.0, EDGE * r)),
    ];
    for (look, want) in cases {
        let mut k = kernel();
        let pieces = spot_weld(&mut k, &spot(look)).unwrap();
        let got: f64 = pieces
            .into_iter()
            .map(|mut p| volume(&k, &mut p.body))
            .sum();
        close(got, want);
    }
}

#[test]
fn the_dent_is_thick_at_the_rim_and_thin_in_the_middle() {
    let mut k = kernel();
    let mut pieces = spot_weld(&mut k, &spot(SpotLook::Dent)).unwrap();
    assert_eq!(
        zones(&pieces, SpotSide::Upper),
        vec![SpotZone::Core, SpotZone::Band, SpotZone::Edge]
    );
    let r = D / 2.0;
    let radii = [(0.0, CORE * r), (CORE * r, r), (r, EDGE * r)];
    for (i, piece) in pieces.iter_mut().enumerate() {
        let (a, b) = radii[i % 3];
        close(volume(&k, &mut piece.body), dent_volume(a, b));
    }
    // Every vertex of the upper dent lies between the face and the rim height.
    for piece in pieces.iter().filter(|p| p.side == SpotSide::Upper) {
        for &v in piece.body.vertex_list() {
            let z = piece.body.vertex(v).unwrap().point.z;
            assert!(
                (T_U - 1e-9..=T_U + DENT_RIM * D + 1e-9).contains(&z),
                "vertex at z {z}"
            );
        }
    }
}

#[test]
fn a_spot_is_placed_at_its_point_along_its_normal() {
    let at = Point3::new(10.0, -3.0, 7.0);
    let n = Vec3::new(1.0, 2.0, -2.0);
    let unit = n * (1.0 / 3.0);
    let mut k = kernel();
    let s = SpotWeld {
        at,
        normal: n,
        ..spot(SpotLook::MarksThinPin)
    };
    let pieces = spot_weld(&mut k, &s).unwrap();
    let h = MARK_THICKNESS * D;
    for piece in &pieces {
        // the signed height of every vertex above the faying face, along the normal
        let heights: Vec<f64> = piece
            .body
            .vertex_list()
            .iter()
            .map(|&v| (piece.body.vertex(v).unwrap().point - at).dot(unit))
            .collect();
        let (lo, hi) = heights
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &z| (a.min(z), b.max(z)));
        let (want_lo, want_hi) = match piece.side {
            SpotSide::Upper => (T_U, T_U + h),
            SpotSide::Lower => (-T_L - h, -T_L),
            SpotSide::Through => (-T_L, T_U),
        };
        assert!(
            (lo - want_lo).abs() < 1e-9 && (hi - want_hi).abs() < 1e-9,
            "{:?}: {lo}..{hi}",
            piece.side
        );
        // every vertex lies within the edge's radius of the axis
        for &v in piece.body.vertex_list() {
            let p = piece.body.vertex(v).unwrap().point - at;
            let radial = (p - unit * p.dot(unit)).length();
            assert!(
                radial <= EDGE * D / 2.0 + 1e-9,
                "vertex {radial} off the axis"
            );
        }
    }
}

#[test]
fn bad_sizes_and_normals_are_refused_as_invalid_input() {
    let bad = [
        SpotWeld {
            upper_thickness: 0.0,
            ..spot(SpotLook::Marks)
        },
        SpotWeld {
            lower_thickness: -1.0,
            ..spot(SpotLook::Marks)
        },
        SpotWeld {
            diameter: f64::NAN,
            ..spot(SpotLook::Marks)
        },
        SpotWeld {
            normal: Vec3::new(0.0, 0.0, 0.0),
            ..spot(SpotLook::Marks)
        },
        SpotWeld {
            at: Point3::new(f64::INFINITY, 0.0, 0.0),
            ..spot(SpotLook::Marks)
        },
    ];
    for s in bad {
        let mut k = kernel();
        let err = spot_weld(&mut k, &s).unwrap_err();
        assert!(matches!(err, SolidError::InvalidInput { .. }), "{err:?}");
    }
}

#[test]
fn the_proposed_diameter_is_five_root_t_of_the_thinner_sheet() {
    assert!((proposed_spot_diameter(1.0, 1.5) - 5.0).abs() < 1e-15);
    assert!((proposed_spot_diameter(4.0, 2.25) - 7.5).abs() < 1e-15);
}
