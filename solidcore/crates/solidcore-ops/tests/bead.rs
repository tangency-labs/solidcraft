//! Beads along joints as bodies of their own (#15130), on a tee of two plates: the volumes
//! against their closed forms, the parts left as they were, the joints' order and faces, and
//! the refusals.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use solidcore::math::det;
use solidcore_ops::bead::{beads, Joint};
use solidcore_ops::boolean::union_all;
use solidcore_ops::{edge_faces_along, make_box, Body, Error, FitKind};

/// The legs of every case.
const LEG: f64 = 6.0;
/// The tee's length along y, the joints' length.
const LENGTH: f64 = 100.0;

/// A web 10 thick and 50 high standing on a base plate 100 wide and 10 thick, `dx` along x:
/// the inside corners run along y at x = dx + 45 and x = dx + 55, z = 10.
fn tee(dx: f64) -> Body {
    let base = make_box([dx, 0.0, 0.0], [100.0, LENGTH, 10.0]).unwrap();
    let web = make_box([dx + 45.0, 0.0, 10.0], [10.0, LENGTH, 50.0]).unwrap();
    union_all(vec![base, web]).unwrap()
}

/// The native index of the edge of `body` through `p`.
fn edge_at(body: &Body, p: [f64; 3]) -> usize {
    solidcore_ops::pick::edges(body, &[p], 1e-6)
        .unwrap()
        .unwrap()[0]
}

fn joint(body: usize, edge: usize, legs: [f64; 2]) -> Joint {
    Joint::Edge {
        body,
        edge,
        legs,
        along: None,
    }
}

fn close(got: f64, want: f64) -> bool {
    (got - want).abs() <= 1e-9 * want
}

/// The circular segment between a chord `c` and its arc at height `h` over its middle.
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn segment_area(c: f64, h: f64) -> f64 {
    let h = h.abs();
    let r = (c * c / 4.0 + h * h) / (2.0 * h);
    r * r * det::asin(c / (2.0 * r)) - (c / 2.0) * (r - h)
}

#[test]
fn a_flat_bead_is_its_triangle_along_the_edge_and_leaves_the_tee() {
    let t = tee(0.0);
    let before = t.to_smb(false).unwrap();
    let e = edge_at(&t, [45.0, 50.0, 10.0]);
    let got = beads(&[&t], &[joint(0, e, [LEG, LEG])], 0.0).unwrap();
    assert_eq!(got.len(), 1);
    let bead = &got[0];
    assert_eq!(bead.counts().unwrap(), [5, 9, 6]);
    bead.check_solid().unwrap();
    let v = bead.volume(1e-12).unwrap();
    assert!(close(v, LEG * LEG / 2.0 * LENGTH), "{v}");
    assert_eq!(t.to_smb(false).unwrap(), before, "the tee changed");
}

#[test]
#[allow(clippy::disallowed_methods)] // the chord's closed form
fn a_convex_bead_adds_its_segment_and_a_concave_one_takes_it() {
    let t = tee(0.0);
    let e = edge_at(&t, [45.0, 50.0, 10.0]);
    let triangle = LEG * LEG / 2.0;
    let segment = segment_area(LEG * 2.0_f64.sqrt(), 1.0);
    for (h, area) in [(1.0, triangle + segment), (-1.0, triangle - segment)] {
        let got = beads(&[&t], &[joint(0, e, [LEG, LEG])], h).unwrap();
        got[0].check_solid().unwrap();
        let v = got[0].volume(1e-12).unwrap();
        assert!(close(v, area * LENGTH), "sagitta {h}: {v}");
    }
}

#[test]
fn beads_come_back_in_the_order_of_their_joints() {
    let t = tee(0.0);
    let right = edge_at(&t, [55.0, 50.0, 10.0]);
    let left = edge_at(&t, [45.0, 50.0, 10.0]);
    let got = beads(
        &[&t],
        &[joint(0, right, [LEG, LEG]), joint(0, left, [LEG, LEG])],
        0.0,
    )
    .unwrap();
    let (r, l) = (got[0].bounds().unwrap(), got[1].bounds().unwrap());
    assert!(close(r.0[0], 55.0) && close(r.1[0], 55.0 + LEG), "{r:?}");
    assert!(close(l.0[0], 45.0 - LEG) && close(l.1[0], 45.0), "{l:?}");
}

#[test]
fn the_first_leg_runs_along_the_face_named() {
    let t = tee(0.0);
    let (e, base, web) = left_corner(&t);
    for (along, reach) in [
        (base, [45.0 - 4.0, 10.0 + 8.0]),
        (web, [45.0 - 8.0, 10.0 + 4.0]),
    ] {
        let j = Joint::Edge {
            body: 0,
            edge: e,
            legs: [4.0, 8.0],
            along: Some(along),
        };
        let (lo, hi) = beads(&[&t], &[j], 0.0).unwrap()[0].bounds().unwrap();
        assert!(
            close(lo[0], reach[0]) && close(hi[2], reach[1]),
            "{lo:?} {hi:?}"
        );
    }
}

#[test]
fn one_call_lays_beads_on_two_bodies_and_leaves_both() {
    let (a, b) = (tee(0.0), tee(200.0));
    let before = [a.to_smb(false).unwrap(), b.to_smb(false).unwrap()];
    let ea = edge_at(&a, [45.0, 50.0, 10.0]);
    let eb = edge_at(&b, [255.0, 50.0, 10.0]);
    let got = beads(
        &[&a, &b],
        &[joint(1, eb, [LEG, LEG]), joint(0, ea, [LEG, LEG])],
        0.0,
    )
    .unwrap();
    assert!(close(got[0].bounds().unwrap().0[0], 255.0));
    assert!(close(got[1].bounds().unwrap().1[0], 45.0));
    for bead in &got {
        assert!(close(bead.volume(1e-12).unwrap(), LEG * LEG / 2.0 * LENGTH));
    }
    assert_eq!([a.to_smb(false).unwrap(), b.to_smb(false).unwrap()], before);
}

/// The tee's left inside corner, with the faces on it that the base's top and the web's
/// side are there.
fn left_corner(t: &Body) -> (usize, usize, usize) {
    let e = edge_at(t, [45.0, 50.0, 10.0]);
    let faces = edge_faces_along(t, e, [0.0, 0.0, 1.0]).unwrap();
    if faces[0].1 > faces[1].1 {
        (e, faces[0].0, faces[1].0)
    } else {
        (e, faces[1].0, faces[0].0)
    }
}

#[test]
fn a_leg_past_its_face_is_refused_with_the_leg_that_fits_and_its_face() {
    let t = tee(0.0);
    let before = t.to_smb(false).unwrap();
    let (e, base, _) = left_corner(&t);
    // the base reaches 45 from the joint; the web's leg fits
    let along = |legs| Joint::Edge {
        body: 0,
        edge: e,
        legs,
        along: Some(base),
    };
    let got = beads(&[&t], &[along([60.0, LEG])], 0.0);
    let Err(Error::TooLarge {
        size,
        kind,
        at,
        face: Some(face),
        ..
    }) = got
    else {
        panic!("{got:?}")
    };
    assert_eq!(kind, FitKind::Leg);
    // the kernel halves the leg down until its bracket is 1e-7 (1 + hi) wide, hi its upper end
    // (just past 45, so under 4.7e-6), below the toe's limit, 45 plus the face's tolerance (a
    // toe holds up to it past the face's edge), then takes that tolerance off
    assert!((45.0 - 4.7e-6..=45.0).contains(&size), "{size}");
    assert!(at.is_some());
    // the toe at that leg, on the base's top: the leg along the base is the one to shrink
    assert!(
        close(face[2], 10.0) && (face[0] - (45.0 - size)).abs() < 1e-9,
        "{face:?}"
    );
    assert_eq!(t.to_smb(false).unwrap(), before, "the tee changed");
    beads(&[&t], &[along([size, LEG])], 0.0).unwrap();
}

/// A plate with a block standing on it whose side ends `w` short of the plate's end: the
/// joint's edge, and the plate's top, which runs only `w` past it.
fn narrow_top(w: f64) -> (Body, usize, usize) {
    let plate = make_box([0.0, 0.0, 0.0], [100.0, 100.0, 10.0]).unwrap();
    let block = make_box([40.0, 0.0, 10.0], [60.0 - w, 100.0, 10.0]).unwrap();
    let part = union_all(vec![plate, block]).unwrap();
    let e = edge_at(&part, [100.0 - w, 50.0, 10.0]);
    let faces = edge_faces_along(&part, e, [0.0, 0.0, 1.0]).unwrap();
    let top = if faces[0].1 > faces[1].1 {
        faces[0].0
    } else {
        faces[1].0
    };
    (part, e, top)
}

fn along_top(e: usize, top: usize, leg: f64) -> Joint {
    Joint::Edge {
        body: 0,
        edge: e,
        legs: [leg, 1.0],
        along: Some(top),
    }
}

/// A leg of 1e6 along a face that runs only 0.05 past the edge: the halving's bracket shrinks
/// with the leg it finds, not the leg asked, so the fit is the face's 0.05, not 0 (#21560).
#[test]
fn a_leg_far_past_a_narrow_face_is_refused_with_the_leg_that_fits() {
    let (part, e, top) = narrow_top(0.05);
    let got = beads(&[&part], &[along_top(e, top, 1e6)], 0.0);
    let Err(Error::TooLarge {
        size,
        kind: FitKind::Leg,
        ..
    }) = got
    else {
        panic!("{got:?}")
    };
    // the bracket ends 1e-7 (1 + hi) wide, hi just past 0.05 (so under 1.1e-7), below the
    // toe's limit, 0.05 plus the face's tolerance, which is then taken off
    assert!((0.05 - 1.1e-7..=0.05).contains(&size), "{size}");
    assert_eq!(
        beads(&[&part], &[along_top(e, top, size)], 0.0)
            .unwrap()
            .len(),
        1
    );
}

/// A face that runs only 5e-5 past the edge, under eight times the body's tolerance (1e-5):
/// the leg that keeps its toe on it is below the smallest leg the body builds, so no leg
/// fits, whatever the leg asked (#21560).
#[test]
fn a_face_narrower_than_the_body_s_resolution_leaves_no_room() {
    let (part, e, top) = narrow_top(5e-5);
    for leg in [1.0, 1e6] {
        let got = beads(&[&part], &[along_top(e, top, leg)], 0.0);
        assert!(matches!(got, Err(Error::NoRoom)), "{leg}: {got:?}");
    }
}

#[test]
#[allow(clippy::disallowed_methods)] // the bounds' closed forms
fn a_sagitta_past_its_bound_is_refused_with_the_sagitta_that_builds() {
    let t = tee(0.0);
    let (e, _, _) = left_corner(&t);
    let one = [joint(0, e, [LEG, LEG])];
    // the toes at 45 degrees: convex below (c / 2) tan(67.5), concave below (c / 2) tan(22.5)
    let half = 3.0 * 2.0_f64.sqrt();
    for (h, bound) in [(20.0, half + 6.0), (-5.0, 6.0 - half)] {
        let got = beads(&[&t], &one, h);
        let Err(Error::TooLarge { size, kind, at, .. }) = got else {
            panic!("{h}: {got:?}")
        };
        assert_eq!(kind, FitKind::Sagitta);
        assert!(close(size, bound), "{h}: {size} {bound}");
        assert!(at.is_some(), "{h}");
        let retry = if h < 0.0 { -size } else { size };
        beads(&[&t], &one, retry).unwrap();
    }
}

#[test]
fn each_joint_s_along_orders_its_own_legs_only() {
    // a block on a plate: the plate's top runs along both of the block's opposite foot edges
    let plate = make_box([0.0, 0.0, 0.0], [100.0, 100.0, 10.0]).unwrap();
    let block = make_box([40.0, 40.0, 10.0], [20.0, 20.0, 20.0]).unwrap();
    let part = union_all(vec![plate, block]).unwrap();
    let up = |e| {
        let f = edge_faces_along(&part, e, [0.0, 0.0, 1.0]).unwrap();
        if f[0].1 > f[1].1 {
            (f[0].0, f[1].0)
        } else {
            (f[1].0, f[0].0)
        }
    };
    let (front, back) = (
        edge_at(&part, [50.0, 40.0, 10.0]),
        edge_at(&part, [50.0, 60.0, 10.0]),
    );
    let (top, _) = up(front);
    let (_, side) = up(back);
    let j = |edge, along| Joint::Edge {
        body: 0,
        edge,
        legs: [4.0, 8.0],
        along: Some(along),
    };
    let got = beads(&[&part], &[j(front, top), j(back, side)], 0.0).unwrap();
    // front: 4 along the plate, 8 up the block; back: 4 up the block, 8 along the plate
    let (lo, hi) = got[0].bounds().unwrap();
    assert!(close(lo[1], 36.0) && close(hi[2], 18.0), "{lo:?} {hi:?}");
    let (_, hi) = got[1].bounds().unwrap();
    assert!(close(hi[1], 68.0) && close(hi[2], 14.0), "{hi:?}");
}

#[test]
fn the_kernel_s_refusals_of_its_input_come_through_with_its_reasons() {
    let t = tee(0.0);
    let (e, _, web) = left_corner(&t);
    let right = edge_at(&t, [55.0, 50.0, 10.0]);
    let top = edge_at(&t, [45.0, 50.0, 60.0]);
    // a face not on the joint's edge
    let off = Joint::Edge {
        body: 0,
        edge: right,
        legs: [LEG, LEG],
        along: Some(web),
    };
    for (joints, want) in [
        (
            vec![joint(0, e, [LEG, LEG]), joint(0, e, [LEG, LEG])],
            "each edge is listed once",
        ),
        (
            vec![joint(0, top, [LEG, LEG])],
            "a weld bead lies in an inside corner (a concave edge) only, not on an outside or a tangent edge",
        ),
        (
            vec![off],
            "a bead's first leg runs along one of its edge's two faces",
        ),
    ] {
        let got = beads(&[&t], &joints, 0.0);
        assert!(
            matches!(got, Err(Error::Refused { reason, .. }) if reason == want),
            "{want}: {got:?}"
        );
    }
    let got = beads(&[&t], &[joint(0, e, [1e-9, LEG])], 0.0);
    let Err(Error::BlendFailed {
        class,
        status,
        phrase,
    }) = &got
    else {
        panic!("{got:?}")
    };
    assert_eq!(*class, solidcore::error::ErrorKind::InvalidInput);
    assert_eq!(*status, 1007);
    assert!(
        phrase.contains("is below eight times the body's tolerance"),
        "{phrase}"
    );
}

/// Two joints whose sagitta bounds differ (legs 6 and 6, legs 4 and 8), on one body and on
/// two: the refusal names the smaller of the two fits, whichever joint or body comes first,
/// and a retry at it builds both beads.
#[test]
fn a_sagitta_too_large_for_several_joints_is_refused_with_the_smallest_fit() {
    let (a, b) = (tee(0.0), tee(200.0));
    let (left, _, _) = left_corner(&a);
    let right = edge_at(&a, [55.0, 50.0, 10.0]);
    let other = edge_at(&b, [245.0, 50.0, 10.0]);
    let fit_of = |bodies: &[&Body], joints: &[Joint]| match beads(bodies, joints, 20.0) {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Sagitta,
            ..
        }) => size,
        got => panic!("{got:?}"),
    };
    let wide = fit_of(&[&a], &[joint(0, left, [LEG, LEG])]);
    let narrow = fit_of(&[&a], &[joint(0, right, [4.0, 8.0])]);
    assert!(narrow < wide, "{narrow} {wide}");
    // one body, the wider joint first
    let one = [joint(0, left, [LEG, LEG]), joint(0, right, [4.0, 8.0])];
    assert_eq!(fit_of(&[&a], &one).to_bits(), narrow.to_bits());
    assert_eq!(beads(&[&a], &one, narrow).unwrap().len(), 2);
    // two bodies, the wider body first
    let two = [joint(0, left, [LEG, LEG]), joint(1, other, [4.0, 8.0])];
    assert_eq!(fit_of(&[&a, &b], &two).to_bits(), narrow.to_bits());
    assert_eq!(beads(&[&a, &b], &two, narrow).unwrap().len(), 2);
}

#[test]
fn beads_meeting_at_a_vertex_are_refused_and_the_part_left() {
    // a block on a plate: its four inside corners meet at its foot's corners
    let plate = make_box([0.0, 0.0, 0.0], [100.0, 100.0, 10.0]).unwrap();
    let block = make_box([40.0, 40.0, 10.0], [20.0, 20.0, 20.0]).unwrap();
    let part = union_all(vec![plate, block]).unwrap();
    let before = part.to_smb(false).unwrap();
    let a = edge_at(&part, [50.0, 40.0, 10.0]);
    let b = edge_at(&part, [40.0, 50.0, 10.0]);
    let got = beads(
        &[&part],
        &[joint(0, a, [LEG, LEG]), joint(0, b, [LEG, LEG])],
        0.0,
    );
    assert!(
        matches!(&got, Err(Error::BlendFailed { phrase, .. })
            if phrase.ends_with("weld beads that meet at a vertex are not built yet (their corner, #17916)")),
        "{got:?}"
    );
    assert_eq!(part.to_smb(false).unwrap(), before, "the part changed");
}

#[test]
fn bad_arguments_are_refused() {
    let t = tee(0.0);
    let e = edge_at(&t, [45.0, 50.0, 10.0]);
    let ok = joint(0, e, [LEG, LEG]);
    for (bodies, joints, h) in [
        (vec![&t], vec![], 0.0),
        (vec![&t], vec![ok], f64::NAN),
        (vec![&t, &t], vec![ok], 0.0),
        (vec![&t], vec![joint(1, e, [LEG, LEG])], 0.0),
        (vec![&t], vec![joint(0, e, [0.0, LEG])], 0.0),
        (vec![&t], vec![joint(0, e, [LEG, f64::INFINITY])], 0.0),
        (vec![&t], vec![joint(0, usize::MAX, [LEG, LEG])], 0.0),
        (
            vec![&t],
            vec![Joint::Edge {
                body: 0,
                edge: e,
                legs: [LEG, LEG],
                along: Some(usize::MAX),
            }],
            0.0,
        ),
    ] {
        let got = beads(&bodies, &joints, h);
        assert!(matches!(got, Err(Error::Argument)), "{got:?}");
    }
}
