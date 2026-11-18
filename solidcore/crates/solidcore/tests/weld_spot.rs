//! Spot welds (`weld::spot`): every style's bodies against their closed-form volumes at
//! several sizes, the kernel's B-rep check on every body, the ball that does not reach a
//! face, the pin's dishes, the placement at any spot and normal, and the refusals.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::f64::consts::PI;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::query::api_queries::brep_bounding_box;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use solidcore::weld::spot::{
    proposed_spot_diameter, spot_weld, MarkFaces, MarkTop, SpotBall, SpotMarks, SpotPiece, SpotPin,
    SpotSide, SpotStyle, SpotWeld, SpotZone,
};
use solidcore::SolidError;

const T_U: f64 = 1.0;
const T_L: f64 = 1.5;
const D: f64 = 5.0;

/// Flat marks: core 0.55 d, band to d, edge to 1.05 d, 0.006 d thick.
const MARKS: SpotMarks = SpotMarks {
    core: 0.55,
    band: 1.0,
    edge: 1.05,
    top: MarkTop::Flat { thickness: 0.006 },
    faces: MarkFaces::Both,
};

/// Six styles: marks; a ball of d with marks where it doesn't reach a face; marks and a
/// small ball; marks and a thin pin; marks and a pin with dished ends; dished marks.
const STYLES: [SpotStyle; 6] = [
    SpotStyle {
        marks: Some(MARKS),
        ball: None,
        pin: None,
    },
    SpotStyle {
        marks: Some(SpotMarks {
            faces: MarkFaces::WhereBallHidden,
            ..MARKS
        }),
        ball: Some(SpotBall { diameter: 1.0 }),
        pin: None,
    },
    SpotStyle {
        marks: Some(MARKS),
        ball: Some(SpotBall { diameter: 0.6 }),
        pin: None,
    },
    SpotStyle {
        marks: Some(MARKS),
        ball: None,
        pin: Some(SpotPin {
            diameter: 0.55,
            dish: None,
        }),
    },
    SpotStyle {
        marks: Some(MARKS),
        ball: None,
        pin: Some(SpotPin {
            diameter: 1.0,
            dish: Some(0.3),
        }),
    },
    SpotStyle {
        marks: Some(SpotMarks {
            top: MarkTop::Dish {
                rim: 0.04,
                middle: 0.004,
            },
            ..MARKS
        }),
        ball: None,
        pin: None,
    },
];

/// Sheets and diameters (upper, lower, d): the usual spot, equal sheets, a large spot, a
/// spot under `0.6` of the sheet (the pin's dish held to a half sphere), a tiny one, and two
/// thick-plate spots at the proposed `5 √t` (the ball hidden in both sheets, in one).
const SIZES: [(f64, f64, f64); 7] = [
    (T_U, T_L, D),
    (2.0, 2.0, 7.0),
    (1.0, 1.5, 40.0),
    (3.0, 3.0, 1.0),
    (1.0, 1.0, 0.05),
    (8.0, 8.0, 14.142_135_623_730_951),
    (1.0, 8.0, 14.142_135_623_730_951),
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

fn spot(style: SpotStyle) -> SpotWeld {
    SpotWeld {
        at: Point3::new(0.0, 0.0, 0.0),
        normal: Vec3::new(0.0, 0.0, 1.0),
        upper_thickness: T_U,
        lower_thickness: T_L,
        diameter: D,
        style,
    }
}

fn sized(style: SpotStyle, (t_u, t_l, d): (f64, f64, f64)) -> SpotWeld {
    SpotWeld {
        upper_thickness: t_u,
        lower_thickness: t_l,
        diameter: d,
        ..spot(style)
    }
}

fn close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= 1e-6 * want.abs(),
        "{what}: volume {got} != {want}"
    );
}

/// A flat ring between radii `a` and `b`, `h` high.
fn ring(a: f64, b: f64, h: f64) -> f64 {
    PI * (b * b - a * a) * h
}

/// A dished zone between radii `a` and `b`: the volume between the face and a sphere's cap
/// that stands `rim` off the face at radius `ro` and `middle` on the axis.
fn dish_zone(a: f64, b: f64, rim: f64, middle: f64, ro: f64) -> f64 {
    let c = rim - middle;
    let rs = (ro * ro + c * c) / (2.0 * c);
    let zc = middle + rs;
    PI * zc * (b * b - a * a)
        + 2.0 * PI / 3.0 * ((rs * rs - b * b).powf(1.5) - (rs * rs - a * a).powf(1.5))
}

/// The marks' three zone volumes (core, band, edge) on one face.
fn zone_volumes(m: &SpotMarks, d: f64) -> [f64; 3] {
    let r = [0.0, m.core * d / 2.0, m.band * d / 2.0, m.edge * d / 2.0];
    let zone = |i: usize| match m.top {
        MarkTop::Flat { thickness } => ring(r[i], r[i + 1], thickness * d),
        MarkTop::Dish { rim, middle } => dish_zone(r[i], r[i + 1], rim * d, middle * d, r[3]),
    };
    [zone(0), zone(1), zone(2)]
}

/// The closed-form volume of all of a style's bodies on sheets `t_u`, `t_l` at diameter
/// `d`: marks on the faces they are drawn on, a ball, a pin less each end's spherical cap.
fn style_volume(s: &SpotStyle, (t_u, t_l, d): (f64, f64, f64)) -> f64 {
    let mut v = 0.0;
    let ball = s.ball.map(|b| b.diameter * d / 2.0);
    if let Some(r) = ball {
        v += 4.0 / 3.0 * PI * r.powi(3);
    }
    if let Some(p) = s.pin {
        let r = p.diameter * d / 2.0;
        let cap = |t: f64| {
            p.dish.map_or(0.0, |f| {
                let c = (f * t).min(r);
                PI * c * (3.0 * r * r + c * c) / 6.0
            })
        };
        v += PI * r * r * (t_u + t_l) - cap(t_u) - cap(t_l);
    }
    if let Some(m) = s.marks {
        let one: f64 = zone_volumes(&m, d).iter().sum();
        for t in [t_u, t_l] {
            let hidden = ball.is_none_or(|r| r <= t);
            if m.faces == MarkFaces::Both || hidden {
                v += one;
            }
        }
    }
    v
}

fn pieces(k: &mut Kernel, s: &SpotWeld) -> Vec<SpotPiece> {
    spot_weld(k, s).unwrap()
}

/// The flat marks lie on both outer faces as three zones, each its closed-form ring.
#[test]
fn the_marks_lie_on_both_outer_faces_as_core_band_and_edge() {
    let mut k = kernel();
    let mut got = pieces(&mut k, &spot(STYLES[0]));
    assert_eq!(got.len(), 6);
    let want = zone_volumes(&MARKS, D);
    for (i, piece) in got.iter_mut().enumerate() {
        let side = if i < 3 {
            SpotSide::Upper
        } else {
            SpotSide::Lower
        };
        let zone = [SpotZone::Core, SpotZone::Band, SpotZone::Edge][i % 3];
        assert_eq!((piece.side, piece.zone), (side, zone));
        close(volume(&k, &mut piece.body), want[i % 3], "zone");
    }
}

/// Every style at every size in `SIZES` against its closed form: balls, pins, the dished
/// pin ends as spherical caps, the dished marks, and marks only where the ball is hidden.
#[test]
fn every_style_has_its_closed_form_volume() {
    for size in SIZES {
        for (i, style) in STYLES.iter().enumerate() {
            let mut k = kernel();
            let got: f64 = pieces(&mut k, &sized(*style, size))
                .into_iter()
                .map(|mut p| volume(&k, &mut p.body))
                .sum();
            close(
                got,
                style_volume(style, size),
                &format!("style {i} at {size:?}"),
            );
        }
    }
}

/// Every body of every style passes the kernel's B-rep check (`check_brep`, level 2), at
/// every size in `SIZES`, on the axis and moved to a spot with a tilted normal, with the
/// kernel's default fixes and with all of them.
#[test]
fn every_body_of_every_style_passes_the_brep_check() {
    let places = [
        (Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
        (Point3::new(10.0, -3.0, 7.0), Vec3::new(1.0, 2.0, -2.0)),
    ];
    for all in [false, true] {
        for (at, normal) in places {
            for size in SIZES {
                for (i, style) in STYLES.iter().enumerate() {
                    let mut k = kernel();
                    if all {
                        k.fixes = UpstreamFixes::all();
                    }
                    let s = SpotWeld {
                        at,
                        normal,
                        ..sized(*style, size)
                    };
                    for mut piece in pieces(&mut k, &s) {
                        let lines = check_brep(&mut k, &mut piece.body, CheckLevel::Level2)
                            .unwrap()
                            .lines();
                        assert!(
                            lines.is_empty(),
                            "style {i} at {size:?} {at:?} all fixes {all} {:?}/{:?}: {lines:?}",
                            piece.zone,
                            piece.side
                        );
                    }
                }
            }
        }
    }
}

/// The lowest and highest local height (along +Z, spot on the axis) of a body: its tight
/// bounding box.
fn extent(k: &Kernel, b: &Brep) -> (f64, f64) {
    let e = brep_bounding_box(k, b, true).unwrap();
    (e.min().z, e.max().z)
}

/// A ball of the spot diameter shows on a face only where its radius passes that sheet:
/// on two 8 mm plates at the proposed `5 √8` (radius 7.07) it is hidden in both, so the
/// marks stand in on both faces; on 1 mm over 8 mm it stands proud of the upper face only,
/// and the marks stand in on the lower. Its extent is checked against both faces.
#[test]
fn a_ball_that_does_not_reach_a_face_gets_marks_there() {
    let d = proposed_spot_diameter(8.0, 8.0).unwrap();
    for ((t_u, t_l), want) in [
        ((8.0, 8.0), vec![SpotSide::Upper, SpotSide::Lower]),
        ((1.0, 8.0), vec![SpotSide::Lower]),
        ((1.0, 1.5), vec![]),
    ] {
        let mut k = kernel();
        let got = pieces(&mut k, &sized(STYLES[1], (t_u, t_l, d)));
        let ball = got.iter().find(|p| p.zone == SpotZone::Inside).unwrap();
        let (lo, hi) = extent(&k, &ball.body);
        let r = d / 2.0;
        assert!((hi - r).abs() < 1e-9 && (lo + r).abs() < 1e-9);
        assert_eq!(hi > t_u, !want.contains(&SpotSide::Upper));
        assert_eq!(-lo > t_l, !want.contains(&SpotSide::Lower));
        let mut sides: Vec<SpotSide> = got
            .iter()
            .filter(|p| p.zone == SpotZone::Core)
            .map(|p| p.side)
            .collect();
        sides.dedup();
        assert_eq!(sides, want, "{t_u} on {t_l}");
    }
}

/// The dished marks are thick at the rim and thin in the middle: each zone's volume, and
/// every vertex between the face and the rim height.
#[test]
fn the_dish_is_thick_at_the_rim_and_thin_in_the_middle() {
    let mut k = kernel();
    let mut got = pieces(&mut k, &spot(STYLES[5]));
    assert_eq!(got.len(), 6);
    let want = zone_volumes(&STYLES[5].marks.unwrap(), D);
    let rim = 0.04 * D;
    for (i, piece) in got.iter_mut().enumerate() {
        close(volume(&k, &mut piece.body), want[i % 3], "dish zone");
        let (lo, hi) = extent(&k, &piece.body);
        if piece.side == SpotSide::Upper {
            assert!(lo >= T_U - 1e-9 && hi <= T_U + rim + 1e-9, "{lo}..{hi}");
        } else {
            assert!(lo >= -T_L - rim - 1e-9 && hi <= -T_L + 1e-9, "{lo}..{hi}");
        }
    }
}

/// Each end of the dished pin is dished from the sheet it lies in, `0.3` of it, and held
/// to a half sphere of the pin's radius below `0.6` of the sheet: the pin runs from face to
/// face either way, never cut short (a dish of `0.3` of 3 mm on a 1 mm spot would need a
/// sphere wider than the pin, which cut each end 0.62 mm below its face).
#[test]
fn each_pin_end_is_dished_from_its_own_sheet_at_most_a_half_sphere() {
    for size in [(1.0, 3.0, 5.0), (3.0, 3.0, 1.0)] {
        let mut k = kernel();
        let got = pieces(&mut k, &sized(STYLES[4], size));
        let mut pin = got
            .into_iter()
            .find(|p| p.zone == SpotZone::Inside)
            .unwrap();
        let (lo, hi) = extent(&k, &pin.body);
        assert!(
            (lo + size.1).abs() < 1e-9 && (hi - size.0).abs() < 1e-9,
            "pin from {lo} to {hi}"
        );
        let r = size.2 / 2.0;
        let cap = |t: f64| {
            let c = (0.3 * t).min(r);
            PI * c * (3.0 * r * r + c * c) / 6.0
        };
        let want = PI * r * r * (size.0 + size.1) - cap(size.0) - cap(size.1);
        close(volume(&k, &mut pin.body), want, "pin");
    }
}

/// A spot at (10, −3, 7) with normal (1, 2, −2): every body's vertices lie at the right
/// heights along the normal and within the edge's radius of the spot's axis.
#[test]
fn a_spot_is_placed_at_its_point_along_its_normal() {
    let at = Point3::new(10.0, -3.0, 7.0);
    let n = Vec3::new(1.0, 2.0, -2.0);
    let unit = n * (1.0 / 3.0);
    for style in STYLES {
        let mut k = kernel();
        let s = SpotWeld {
            at,
            normal: n,
            ..spot(style)
        };
        for piece in pieces(&mut k, &s) {
            for &v in piece.body.vertex_list() {
                let p = piece.body.vertex(v).unwrap().point;
                let along = (p - at).dot(unit);
                let off = ((p - at) - unit * along).length();
                assert!(off <= 1.05 * D / 2.0 + 1e-9, "off the axis by {off}");
                let (lo, hi) = match piece.side {
                    SpotSide::Upper => (T_U, T_U + 0.04 * D),
                    SpotSide::Lower => (-T_L - 0.04 * D, -T_L),
                    SpotSide::Through => (-T_L.max(D / 2.0), T_U.max(D / 2.0)),
                };
                assert!(
                    along >= lo - 1e-9 && along <= hi + 1e-9,
                    "{along} not in {lo}..{hi}"
                );
            }
        }
    }
}

fn refused(s: &SpotWeld) {
    let mut k = kernel();
    match spot_weld(&mut k, s) {
        Err(SolidError::InvalidInput { operation, .. }) => assert_eq!(operation, "weld::spot"),
        other => panic!("not refused as the spot's invalid input: {other:?}"),
    }
}

/// Bad sizes, points, normals and styles are refused as the spot's own invalid input.
#[test]
fn bad_sizes_normals_and_styles_are_refused_as_invalid_input() {
    let s = spot(STYLES[0]);
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        refused(&SpotWeld {
            upper_thickness: t,
            ..s
        });
        refused(&SpotWeld {
            lower_thickness: t,
            ..s
        });
        refused(&SpotWeld { diameter: t, ..s });
    }
    refused(&SpotWeld {
        normal: Vec3::new(0.0, 0.0, 0.0),
        ..s
    });
    refused(&SpotWeld {
        at: Point3::new(f64::NAN, 0.0, 0.0),
        ..s
    });
    let none = SpotStyle {
        marks: None,
        ball: None,
        pin: None,
    };
    let bad_marks = [
        SpotMarks { core: 0.0, ..MARKS },
        SpotMarks { band: 0.5, ..MARKS },
        SpotMarks { edge: 1.0, ..MARKS },
        SpotMarks {
            top: MarkTop::Flat { thickness: 0.0 },
            ..MARKS
        },
        SpotMarks {
            top: MarkTop::Dish {
                rim: 0.04,
                middle: 0.04,
            },
            ..MARKS
        },
    ];
    let mut styles = vec![
        none,
        SpotStyle {
            ball: Some(SpotBall { diameter: -1.0 }),
            ..none
        },
        SpotStyle {
            pin: Some(SpotPin {
                diameter: 1.0,
                dish: Some(0.0),
            }),
            ..none
        },
    ];
    styles.extend(bad_marks.map(|m| SpotStyle {
        marks: Some(m),
        ..none
    }));
    for style in styles {
        refused(&SpotWeld { style, ..s });
    }
}

/// Finite sizes the model cannot hold are refused as the spot's own invalid input before a
/// primitive sees them: a diameter or sheet so large the spot passes the model's size limit
/// (a dish's sphere at `d = 1e200` overflows), or so small a piece is thinner than the zone
/// tolerance (a mark of `0.006 d` at `d = 1e-10`, a pin dish on a `1e-9` sheet).
#[test]
fn finite_sizes_the_model_cannot_hold_are_refused() {
    for style in STYLES {
        for (t_u, t_l, d) in [
            (T_U, T_L, 1e200),
            (1e200, T_L, D),
            (T_U, T_L, 1e-10),
            (1e-9, T_L, D),
        ] {
            refused(&sized(style, (t_u, t_l, d)));
        }
    }
    // far from the origin: the spot passes the limit though each piece is small
    refused(&SpotWeld {
        at: Point3::new(1e151, 0.0, 0.0),
        ..spot(STYLES[0])
    });
}

/// The proposed diameter is `5 √t` of the thinner sheet; a thickness that is not finite
/// and positive is refused instead of giving NaN.
#[test]
fn the_proposed_diameter_is_five_root_t_of_the_thinner_sheet() {
    assert!((proposed_spot_diameter(1.0, 1.5).unwrap() - 5.0).abs() < 1e-12);
    assert!((proposed_spot_diameter(4.0, 2.25).unwrap() - 7.5).abs() < 1e-12);
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            proposed_spot_diameter(t, 1.0),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            proposed_spot_diameter(1.0, t),
            Err(SolidError::InvalidInput { .. })
        ));
    }
}
