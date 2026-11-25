//! The frame-member trim workflow's decisions (`frame_trim`): which end each target moves, two targets on one
//! end refused, a member no target meets refused, and the kernel's refusals passed on with the
//! target they are about. The geometry itself is the kernel's (`solidcore::prism::member`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]

use solidcore_ops::frame_trim::{
    axis, extent, trim, Cut, End, Member, Refusal, Side, Target, TrimRefusal,
};
use solidcore_record::{Plane, Vec2, Vec3};

/// A 45 × 45 bar centred on the line from `a` to `b` along +x, its section in y and z.
fn rail(a: f64, b: f64) -> Member {
    let section = Plane {
        origin: Vec3::new(a, 0.0, 0.0),
        x: Vec3::new(0.0, 1.0, 0.0),
        y: Vec3::new(0.0, 0.0, 1.0),
    };
    Member::along(
        &section,
        Vec3::new(a, 0.0, 0.0),
        Vec3::new(b, 0.0, 0.0),
        Vec2::new(-22.5, -22.5),
        Vec2::new(22.5, 22.5),
    )
    .unwrap()
}

/// A 45 × 45 post along +y at x = 0 from y = −100 to 100 (its section's x is world −x, y up).
fn post() -> Member {
    let section = Plane {
        origin: Vec3::new(0.0, -100.0, 0.0),
        x: Vec3::new(-1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 0.0, 1.0),
    };
    Member::along(
        &section,
        Vec3::new(0.0, -100.0, 0.0),
        Vec3::new(0.0, 100.0, 0.0),
        Vec2::new(-22.5, -22.5),
        Vec2::new(22.5, 22.5),
    )
    .unwrap()
}

/// The plane x = `x`, its normal along +x.
fn across_x(x: f64) -> Plane {
    Plane {
        origin: Vec3::new(x, 0.0, 0.0),
        x: Vec3::new(0.0, 1.0, 0.0),
        y: Vec3::new(0.0, 0.0, 1.0),
    }
}

/// The plane of a cut, its x along world y (the cuts here are square to x).
fn plane(c: &Cut) -> Plane {
    // n × (world y), so that x × y = n.
    let y = Vec3::new(-c.normal.z, 0.0, c.normal.x);
    Plane {
        origin: c.origin,
        x: Vec3::new(0.0, 1.0, 0.0),
        y,
    }
}

#[test]
fn a_rail_butted_to_a_post_and_cut_to_a_plane_moves_each_end_once() {
    let (rail, post) = (rail(100.0, 300.0), post());
    let none = [None, None];
    let far = across_x(250.0);
    let targets = [
        Target::Member {
            member: &post,
            ends: &none,
            side: None,
        },
        Target::Plane(&far),
    ];
    let cuts = trim(&rail, &none, End::Nearer, &targets, 0.0).unwrap();
    let [start, end] = cuts.map(Option::unwrap);
    // The start against the post's side at x = 22.5, facing back along the rail; the end at 250.
    assert!(
        (start.origin.x - 22.5).abs() < 1e-9 && (start.normal.x + 1.0).abs() < 1e-9,
        "{start:?}"
    );
    assert!(
        (end.origin.x - 250.0).abs() < 1e-9 && (end.normal.x - 1.0).abs() < 1e-9,
        "{end:?}"
    );
    let ends = [Some(plane(&start)), Some(plane(&end))];
    let a = axis(&rail, &ends).unwrap();
    assert!((a.length - 227.5).abs() < 1e-9, "{a:?}");
    let e = extent(&rail, &ends).unwrap();
    assert!(
        e.cut == [false, false]
            && (e.from - (22.5 - 100.0)).abs() < 1e-9
            && (e.to - 150.0).abs() < 1e-9,
        "{e:?}"
    );
}

#[test]
fn two_targets_on_one_end_or_none_meeting_the_member_are_refused() {
    let (rail, post) = (rail(100.0, 300.0), post());
    let none = [None, None];
    let near = across_x(0.0);
    let targets = [
        Target::Member {
            member: &post,
            ends: &none,
            side: None,
        },
        Target::Plane(&near),
    ];
    assert_eq!(
        trim(&rail, &none, End::Nearer, &targets, 0.0).unwrap_err(),
        TrimRefusal::SameEnd {
            first: 0,
            second: 1,
            end: 0
        }
    );
    let beside = Plane {
        origin: Vec3::new(0.0, 50.0, 0.0),
        x: Vec3::new(1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 0.0, 1.0),
    };
    assert_eq!(
        trim(&rail, &none, End::Nearer, &[Target::Plane(&beside)], 0.0).unwrap_err(),
        TrimRefusal::MeetsNone
    );
}

#[test]
fn the_kernels_refusal_names_the_target_it_is_about() {
    let (rail, post) = (rail(100.0, 300.0), post());
    let none = [None, None];
    let away = [Target::Member {
        member: &post,
        ends: &none,
        side: Some(Side::Right),
    }];
    assert_eq!(
        trim(&rail, &none, End::Nearer, &away, 0.0).unwrap_err(),
        TrimRefusal::Member {
            target: Some(0),
            why: Refusal::FacesAway
        }
    );
    let along = Plane {
        origin: Vec3::new(0.0, 0.0, 0.0),
        x: Vec3::new(1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 0.0, 1.0),
    };
    let bad = [None, Some(along)];
    assert_eq!(
        trim(
            &rail,
            &bad,
            End::Nearer,
            &[Target::Plane(&across_x(0.0))],
            0.0
        )
        .unwrap_err(),
        TrimRefusal::Member {
            target: None,
            why: Refusal::RunsAlong
        }
    );
}

#[test]
fn a_members_data_makes_it_again_and_other_data_is_refused() {
    use solidcore_ops::frame_trim::MemberData;
    for m in [rail(100.0, 300.0), post()] {
        let data = m.data();
        let again = Member::from_data(&data).unwrap();
        assert_eq!(again, m);
        assert_eq!(again.data(), data);
    }
    let data = post().data();
    let long = MemberData {
        dir: Vec3::new(0.0, 2.0, 0.0),
        ..data
    };
    assert_eq!(Member::from_data(&long), Err(Refusal::Degenerate));
    let flat = MemberData {
        hi: Vec2::new(-30.0, 22.5),
        ..data
    };
    assert_eq!(Member::from_data(&flat), Err(Refusal::Degenerate));
}
