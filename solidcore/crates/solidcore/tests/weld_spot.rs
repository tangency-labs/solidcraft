//! Spot welds (`weld::spot`): every look's bodies against their closed-form volumes at
//! several sizes, the kernel's B-rep check on every body, the placement at any spot and
//! normal, and the refusals.
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
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use solidcore::weld::spot::{
    pin_dish_depth, proposed_spot_diameter, spot_weld, SpotLook, SpotPiece, SpotSide, SpotWeld,
    SpotZone, CORE, DENT_MIDDLE, DENT_RIM, EDGE, MARK_THICKNESS, PIN_DISH, SMALL_BALL,
};
use solidcore::SolidError;

const T_U: f64 = 1.0;
const T_L: f64 = 1.5;
const D: f64 = 5.0;

const LOOKS: [SpotLook; 6] = [
    SpotLook::Marks,
    SpotLook::Ball,
    SpotLook::MarksSmallBall,
    SpotLook::MarksThinPin,
    SpotLook::MarksPinConcaveEnds,
    SpotLook::Dent,
];

/// Sheets and diameters (upper, lower, d): the usual spot, equal sheets, a large spot, a
/// spot under `0.6` of the thinner sheet (the pin's dish held to a half sphere) and a tiny one.
const SIZES: [(f64, f64, f64); 5] = [
    (T_U, T_L, D),
    (2.0, 2.0, 7.0),
    (1.0, 1.5, 40.0),
    (3.0, 3.0, 1.0),
    (1.0, 1.0, 0.05),
];

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
        (got - want).abs() <= 1e-6 * want.abs(),
        "volume {got} != {want}"
    );
}

/// A disc or ring of diameters `a d`..`b d` and `height`.
fn ring_volume(d: f64, a: f64, b: f64, height: f64) -> f64 {
    PI / 4.0 * (b * b - a * a) * d * d * height
}

/// The dent zone between radii `a` and `b` of a spot of diameter `d`: the volume under the
/// dished top.
fn dent_volume(d: f64, a: f64, b: f64) -> f64 {
    let (rim, middle) = (DENT_RIM * d, DENT_MIDDLE * d);
    let ro = EDGE * d / 2.0;
    let c = rim - middle;
    let rs = (ro * ro + c * c) / (2.0 * c);
    PI * (middle + rs) * (b * b - a * a)
        + 2.0 * PI / 3.0 * ((rs * rs - b * b).powf(1.5) - (rs * rs - a * a).powf(1.5))
}

/// The marks on one outer face.
fn marks_volume(d: f64) -> f64 {
    ring_volume(d, 0.0, EDGE, MARK_THICKNESS * d)
}

/// The closed-form volume of all of a look's bodies on sheets `t_u`, `t_l` at diameter `d`.
fn look_volume(look: SpotLook, t_u: f64, t_l: f64, d: f64) -> f64 {
    let r = d / 2.0;
    // a spherical cap of depth `c` over a circle of radius `r`
    let cap = |c: f64| PI * c * (3.0 * r * r + c * c) / 6.0;
    match look {
        SpotLook::Marks => 2.0 * marks_volume(d),
        SpotLook::Ball => 4.0 / 3.0 * PI * r.powi(3),
        SpotLook::MarksSmallBall => {
            2.0 * marks_volume(d) + 4.0 / 3.0 * PI * (SMALL_BALL * r).powi(3)
        }
        SpotLook::MarksThinPin => 2.0 * marks_volume(d) + PI * (CORE * r).powi(2) * (t_u + t_l),
        SpotLook::MarksPinConcaveEnds => {
            2.0 * marks_volume(d) + PI * r * r * (t_u + t_l)
                - 2.0 * cap(pin_dish_depth(d, t_u, t_l))
        }
        SpotLook::Dent => 2.0 * dent_volume(d, 0.0, EDGE * r),
    }
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
        ring_volume(D, 0.0, CORE, h),
        ring_volume(D, CORE, 1.0, h),
        ring_volume(D, 1.0, EDGE, h),
    ];
    for (i, piece) in pieces.iter_mut().enumerate() {
        close(volume(&k, &mut piece.body), want[i % 3]);
    }
}

/// Every look at every size in `SIZES`, equal sheets and a large spot among them, against
/// its closed form: balls, pins, the dished pin ends as spherical caps, the dent's dish.
#[test]
fn every_look_has_its_closed_form_volume() {
    for (t_u, t_l, d) in SIZES {
        for look in LOOKS {
            let mut k = kernel();
            let s = SpotWeld {
                upper_thickness: t_u,
                lower_thickness: t_l,
                diameter: d,
                ..spot(look)
            };
            let pieces = spot_weld(&mut k, &s).unwrap();
            let got: f64 = pieces
                .into_iter()
                .map(|mut p| volume(&k, &mut p.body))
                .sum();
            let want = look_volume(look, t_u, t_l, d);
            assert!(
                (got - want).abs() <= 1e-6 * want,
                "{look:?} t {t_u}/{t_l} d {d}: volume {got} != {want}"
            );
        }
    }
}

/// Every body of every look passes the kernel's B-rep check (`check_brep`, level 2), at every
/// size in `SIZES`, on the axis and moved to a spot with a tilted normal.
#[test]
fn every_body_of_every_look_passes_the_brep_check() {
    let places = [
        (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
        (Point3::new(10.0, -3.0, 7.0), Vec3::new(1.0, 2.0, -2.0)),
    ];
    for (at, normal) in places {
        for (t_u, t_l, d) in SIZES {
            for look in LOOKS {
                let mut k = kernel();
                let s = SpotWeld {
                    at,
                    normal,
                    upper_thickness: t_u,
                    lower_thickness: t_l,
                    diameter: d,
                    look,
                };
                for mut piece in spot_weld(&mut k, &s).unwrap() {
                    let lines = check_brep(&mut k, &mut piece.body, CheckLevel::Level2)
                        .unwrap()
                        .lines();
                    assert!(
                        lines.is_empty(),
                        "{look:?} t {t_u}/{t_l} d {d} at {at:?} {:?}/{:?}: {lines:?}",
                        piece.zone,
                        piece.side
                    );
                }
            }
        }
    }
}

/// A pin under `0.6` of the thinner sheet keeps its rim on both outer faces: its dish is
/// held to a half sphere of the pin's radius. A dish of `0.3` of the sheet there needed a
/// sphere wider than the pin, which cut the pin short (at 1 mm on 3 mm sheets each end
/// sank 0.62 mm below its face).
#[test]
fn a_thin_pin_s_dish_is_held_to_a_half_sphere() {
    let (t, d) = (3.0, 1.0);
    assert!((pin_dish_depth(d, t, t) - d / 2.0).abs() < 1e-15);
    assert!((pin_dish_depth(D, T_U, T_L) - PIN_DISH * T_U).abs() < 1e-15);
    let mut k = kernel();
    let s = SpotWeld {
        upper_thickness: t,
        lower_thickness: t,
        diameter: d,
        ..spot(SpotLook::MarksPinConcaveEnds)
    };
    let pieces = spot_weld(&mut k, &s).unwrap();
    let pin = pieces.iter().find(|p| p.zone == SpotZone::Inside).unwrap();
    let (lo, hi) = pin
        .body
        .vertex_list()
        .iter()
        .map(|&v| pin.body.vertex(v).unwrap().point.z)
        .fold((f64::MAX, f64::MIN), |(a, b), z| (a.min(z), b.max(z)));
    assert!(
        (lo + t).abs() < 1e-9 && (hi - t).abs() < 1e-9,
        "pin from {lo} to {hi}, not -{t} to {t}"
    );
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
        close(volume(&k, &mut piece.body), dent_volume(D, a, b));
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
