//! Where a spot weld sits (`weld::spot::spot_site`): stacks of boxes picked on an outer face
//! give the frame and every sheet's faces and thickness, which build the spot; every typed
//! refusal of a stack that does not take the spot, and the bad inputs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::boolean::api_brep::boolean_union;
use solidcore::error::weld::WeldFailure;
use solidcore::error::Reason;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::query::api_queries::brep_bounding_box;
use solidcore::topology::brep::p5::transform;
use solidcore::topology::{Brep, FaceId};
use solidcore::weld::spot::{spot_site, spot_weld, SpotSide, SpotSite, SpotStyle, SpotWeld};
use solidcore::SolidError;

const D: f64 = 5.0;

fn kernel() -> Kernel {
    Kernel::new(&KernelOptions::default())
}

/// A 40 × 40 sheet from `z` up `t`, centred on the z axis.
fn sheet(k: &mut Kernel, z: f64, t: f64) -> Brep {
    create_box(k, Point3::new(-20.0, -20.0, z), 40.0, 40.0, t).unwrap()
}

/// The face of `b` whose plane holds `p` (a point inside a box's top or bottom).
fn face_at(k: &Kernel, b: &Brep, p: Point3) -> FaceId {
    b.face_list()
        .iter()
        .copied()
        .find(|&f| {
            let s = solidcore::topology::face::p2::surface(b, f)
                .unwrap()
                .unwrap();
            let planar = s.is_planar(k, 1e-9).unwrap();
            let n = planar.normal.unitize().unwrap();
            (p - planar.point).dot(n).abs() < 1e-9
        })
        .unwrap()
}

fn site(k: &Kernel, sheets: &[&Brep], picked: usize, p: Point3) -> solidcore::Result<SpotSite> {
    let face = face_at(k, sheets[picked], p);
    spot_site(k, sheets, picked, face, p, D)
}

/// The typed refusal, and its fit.
fn refused(r: solidcore::Result<SpotSite>) -> (WeldFailure, Option<f64>) {
    let err = r.unwrap_err();
    let SolidError::Failed(failure) = &err else {
        panic!("not a typed refusal: {err:?}");
    };
    let Reason::Weld(why) = failure.reason else {
        panic!("not a weld refusal: {err:?}");
    };
    (why, err.fit().map(|f| f.size))
}

/// Two sheets (1.5 mm below, 1.0 mm above), picked on the upper sheet's top: the line runs
/// down, the upper sheet first, the spot's point on the faying face; the site builds the spot
/// with its marks on both outer faces.
#[test]
fn two_sheets_picked_on_top_give_the_frame_and_each_sheet() {
    let mut k = kernel();
    let lower = sheet(&mut k, 0.0, 1.5);
    let upper = sheet(&mut k, 1.5, 1.0);
    let s = site(&k, &[&lower, &upper], 1, Point3::new(2.0, 3.0, 2.5)).unwrap();
    assert!(
        (s.normal - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-12,
        "{s:?}"
    );
    assert!(s.at.distance(Point3::new(2.0, 3.0, 1.5)) < 1e-9, "{s:?}");
    let got: Vec<(usize, f64)> = s.sheets.iter().map(|x| (x.sheet, x.thickness)).collect();
    assert_eq!(got.len(), 2);
    assert_eq!((got[0].0, got[1].0), (1, 0));
    assert!(
        (got[0].1 - 1.0).abs() < 1e-9 && (got[1].1 - 1.5).abs() < 1e-9,
        "{got:?}"
    );
    assert_eq!(
        s.sheets[0].near,
        face_at(&k, &upper, Point3::new(0.0, 0.0, 2.5))
    );
    assert_eq!(
        s.sheets[1].far,
        face_at(&k, &lower, Point3::new(0.0, 0.0, 0.0))
    );
    // the site builds the spot: marks on the picked face (z = 2.5) and on the far one (z = 0)
    let spot = SpotWeld {
        at: s.at,
        normal: s.normal,
        thicknesses: s.sheets.iter().map(|x| x.thickness).collect(),
        diameter: D,
        style: SpotStyle {
            marks: Some(solidcore::weld::spot::SpotMarks {
                core: 0.55,
                band: 1.0,
                edge: 1.05,
                top: solidcore::weld::spot::MarkTop::Flat { thickness: 0.006 },
                faces: solidcore::weld::spot::MarkFaces::Both,
            }),
            ball: None,
            pin: None,
        },
    };
    for piece in spot_weld(&mut k, &spot).unwrap() {
        let e = brep_bounding_box(&k, &piece.body, true).unwrap();
        match piece.side {
            // `Lower` is the first sheet's outer face: the picked one
            SpotSide::Lower => assert!(e.min().z >= 2.5 - 1e-9, "{:?}", e.min()),
            SpotSide::Upper => assert!(e.max().z <= 1e-9, "{:?}", e.max()),
            SpotSide::Through => panic!("no pin in this style"),
        }
    }
}

/// Three sheets listed in any order, picked on the bottom: the line runs up, the sheets
/// come in stack order with their thicknesses.
#[test]
fn three_sheets_come_in_stack_order_from_the_picked_face() {
    let mut k = kernel();
    let a = sheet(&mut k, 0.0, 1.0);
    let b = sheet(&mut k, 1.0, 2.0);
    let c = sheet(&mut k, 3.0, 0.8);
    let s = site(&k, &[&c, &a, &b], 1, Point3::new(0.0, 0.0, 0.0)).unwrap();
    assert!((s.normal - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-12);
    let got: Vec<usize> = s.sheets.iter().map(|x| x.sheet).collect();
    assert_eq!(got, [1, 2, 0]);
    for (x, t) in s.sheets.iter().zip([1.0, 2.0, 0.8]) {
        assert!((x.thickness - t).abs() < 1e-9, "{x:?}");
    }
    assert!(s.at.distance(Point3::new(0.0, 0.0, 1.0)) < 1e-9);
}

/// A point beside the picked face (past its edge, in its plane) is off the face.
#[test]
fn a_point_off_the_picked_face_is_refused() {
    let mut k = kernel();
    let lower = sheet(&mut k, 0.0, 1.5);
    let upper = sheet(&mut k, 1.5, 1.0);
    let face = face_at(&k, &upper, Point3::new(0.0, 0.0, 2.5));
    let r = spot_site(
        &k,
        &[&lower, &upper],
        1,
        face,
        Point3::new(30.0, 0.0, 2.5),
        D,
    );
    assert_eq!(refused(r).0, WeldFailure::SpotPointOffFace);
}

/// Sheets 0.5 apart are refused with the gap; sheets that overlap by 0.5 are refused.
#[test]
fn sheets_apart_or_overlapping_are_refused() {
    let mut k = kernel();
    let lower = sheet(&mut k, 0.0, 1.5);
    let apart = sheet(&mut k, 2.0, 1.0);
    let (why, _) = refused(site(&k, &[&lower, &apart], 1, Point3::new(0.0, 0.0, 3.0)));
    let WeldFailure::SpotSheetsApart { sheet: which, gap } = why else {
        panic!("{why:?}");
    };
    assert_eq!(which, 0);
    assert!((gap - 0.5).abs() < 1e-9, "gap {gap}");
    let sunk = sheet(&mut k, 1.0, 1.0);
    let (why, _) = refused(site(&k, &[&lower, &sunk], 1, Point3::new(0.0, 0.0, 2.0)));
    assert_eq!(why, WeldFailure::SpotSheetsOverlap { sheet: 0 });
}

/// A listed sheet the line does not reach is missed; one on the outer side of the picked face
/// means the face is not an outer face of the stack.
#[test]
fn a_sheet_missed_or_beyond_the_picked_face_is_refused() {
    let mut k = kernel();
    let lower = sheet(&mut k, 0.0, 1.5);
    let upper = sheet(&mut k, 1.5, 1.0);
    let aside = create_box(&mut k, Point3::new(50.0, 0.0, 0.0), 5.0, 5.0, 1.0).unwrap();
    let (why, _) = refused(site(
        &k,
        &[&lower, &upper, &aside],
        0,
        Point3::new(0.0, 0.0, 0.0),
    ));
    assert_eq!(why, WeldFailure::SpotSheetMissed { sheet: 2 });
    let below = sheet(&mut k, -5.0, 1.0);
    let (why, _) = refused(site(
        &k,
        &[&lower, &upper, &below],
        0,
        Point3::new(0.0, 0.0, 0.0),
    ));
    assert_eq!(why, WeldFailure::SpotFaceNotOuter { sheet: 2 });
}

/// A sheet the line crosses twice (two lumps, one above the other) is refused.
#[test]
fn a_sheet_crossed_twice_is_refused() {
    let mut k = kernel();
    let lower = sheet(&mut k, 0.0, 1.5);
    let a = sheet(&mut k, 1.5, 1.0);
    let b = sheet(&mut k, 4.0, 1.0);
    let twice = boolean_union(&mut k, a, b).unwrap();
    let (why, _) = refused(site(&k, &[&lower, &twice], 0, Point3::new(0.0, 0.0, 0.0)));
    assert_eq!(why, WeldFailure::SpotSheetCrossedAgain { sheet: 1 });
}

/// A curved picked face is not flat; a sheet tilted 5° under the line is not square to it.
#[test]
fn a_face_not_flat_or_not_square_is_refused() {
    let mut k = kernel();
    let ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 10.0), 5.0).unwrap();
    let lower = sheet(&mut k, 0.0, 1.5);
    let curved = ball.face_list()[0];
    let r = spot_site(
        &k,
        &[&ball, &lower],
        0,
        curved,
        Point3::new(0.0, 0.0, 15.0),
        D,
    );
    assert_eq!(
        refused(r).0,
        WeldFailure::SpotFaceNotFlat {
            sheet: 0,
            face: curved
        }
    );
    // a sheet whose lower face passes through (0, 0, 1.5), tilted 5° about the y axis
    let mut tilted = create_box(&mut k, Point3::new(-20.0, -20.0, 0.0), 40.0, 40.0, 1.0).unwrap();
    let a = 5f64.to_radians();
    let place = Axis2Placement::new(
        Point3::new(0.0, 0.0, 1.5),
        Vec3::new(a.cos(), 0.0, -a.sin()),
        Vec3::new(0.0, 1.0, 0.0),
    );
    transform(&mut k, &mut tilted, &place, None, false).unwrap();
    let (why, _) = refused(site(&k, &[&lower, &tilted], 0, Point3::new(0.0, 0.0, 0.0)));
    assert!(
        matches!(why, WeldFailure::SpotFaceNotSquare { sheet: 1, .. }),
        "{why:?}"
    );
}

/// A spot of 5 on a 3 × 3 sheet passes its edges; its fit is the largest diameter that fits,
/// 3 (the distance to the nearest edge, twice).
#[test]
fn a_footprint_past_a_face_edge_is_refused_with_the_diameter_that_fits() {
    let mut k = kernel();
    let lower = sheet(&mut k, 0.0, 1.5);
    let small = create_box(&mut k, Point3::new(-1.5, -1.5, 1.5), 3.0, 3.0, 1.0).unwrap();
    let (why, fit) = refused(site(&k, &[&lower, &small], 0, Point3::new(0.0, 0.0, 0.0)));
    assert!(
        matches!(why, WeldFailure::SpotPastFaceEdge { sheet: 1, diameter, .. } if diameter == D),
        "{why:?}"
    );
    assert!((fit.unwrap() - 3.0).abs() < 1e-9, "fit {fit:?}");
    // just under the diameter that fits it builds
    let face = face_at(&k, &lower, Point3::new(0.0, 0.0, 0.0));
    assert!(spot_site(
        &k,
        &[&lower, &small],
        0,
        face,
        Point3::new(0.0, 0.0, 0.0),
        2.99
    )
    .is_ok());
}

/// Fewer than two sheets, a picked sheet not listed, a point that is not finite or a
/// diameter that is not finite and positive are bad input.
#[test]
fn bad_input_is_refused_as_invalid_input() {
    let mut k = kernel();
    let lower = sheet(&mut k, 0.0, 1.5);
    let upper = sheet(&mut k, 1.5, 1.0);
    let face = face_at(&k, &lower, Point3::new(0.0, 0.0, 0.0));
    let p = Point3::new(0.0, 0.0, 0.0);
    let bad = |r: solidcore::Result<SpotSite>| matches!(r, Err(SolidError::InvalidInput { .. }));
    assert!(bad(spot_site(&k, &[&lower], 0, face, p, D)));
    assert!(bad(spot_site(&k, &[&lower, &upper], 2, face, p, D)));
    assert!(bad(spot_site(
        &k,
        &[&lower, &upper],
        0,
        face,
        Point3::new(f64::NAN, 0.0, 0.0),
        D
    )));
    for d in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(bad(spot_site(&k, &[&lower, &upper], 0, face, p, d)));
    }
}
