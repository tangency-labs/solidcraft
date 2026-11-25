use std::f64::consts::{FRAC_1_SQRT_2, PI};

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};

use super::*;
use crate::frame::{self, Anchor};

fn seg(id: &str, a: (f64, f64), b: (f64, f64)) -> (String, Vec3, Vec3) {
    (id.to_string(), Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0))
}

fn square(lo: f64, hi: f64) -> Vec<Region2> {
    let pts = [Vec2::new(lo, lo), Vec2::new(hi, lo), Vec2::new(hi, hi), Vec2::new(lo, hi)];
    vec![Region2 { outer: Loop2::polygon(&pts), holes: vec![] }]
}

/// Each member's section placed square to its line at `at`, as `eval_frame::evaluate` places it, with the
/// profile point `anchor` on the line.
fn placer(segments: &[(String, Vec3, Vec3)], anchor: Vec2) -> impl Fn(usize, Vec3) -> Plane + '_ {
    move |m, at| {
        let (_, a, b) = &segments[m];
        let (x, y) = frame::member_axes(*b - *a, 0.0, frame::UpAxis::Z).unwrap();
        Plane::new(at - x * anchor.x - y * anchor.y, x, y).unwrap()
    }
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-9
}

/// The L of the cut tests: (0,0)→(600,0) and (0,0)→(0,400), meeting at the origin.
fn ell() -> Vec<(String, Vec3, Vec3)> {
    vec![seg("a", (0.0, 0.0), (600.0, 0.0)), seg("b", (0.0, 0.0), (0.0, 400.0))]
}

#[test]
fn a_rectangle_has_four_corners() {
    let segs = vec![
        seg("a", (0.0, 0.0), (600.0, 0.0)),
        seg("b", (600.0, 0.0), (600.0, 400.0)),
        seg("c", (600.0, 400.0), (0.0, 400.0)),
        seg("d", (0.0, 400.0), (0.0, 0.0)),
    ];
    let corners = find_corners(&segs);
    // In member-pair order: (a, b), (a, d), (b, c), (c, d).
    let points: Vec<Vec3> = corners.iter().map(|c| c.point).collect();
    assert_eq!(points, vec![Vec3::new(600.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0), Vec3::new(600.0, 400.0, 0.0), Vec3::new(0.0, 400.0, 0.0)]);
    let ends: Vec<_> = corners.iter().map(|c| c.ends).collect();
    assert_eq!(ends, vec![[(0, false), (1, true)], [(0, true), (3, false)], [(1, false), (2, true)], [(2, false), (3, true)]]);
}

#[test]
fn three_ends_at_a_point_or_an_end_on_a_middle_is_no_corner() {
    let tee = vec![seg("a", (0.0, 0.0), (100.0, 0.0)), seg("b", (0.0, 0.0), (0.0, 100.0)), seg("c", (0.0, 0.0), (-100.0, 0.0))];
    assert!(find_corners(&tee).is_empty());
    // Three ends at a point: crowded, every end by member index.
    assert_eq!(find_crowded(&tee), vec![(Vec3::new(0.0, 0.0, 0.0), vec![(0, true), (1, true), (2, true)])]);
    let on_middle = vec![seg("a", (-100.0, 0.0), (100.0, 0.0)), seg("b", (0.0, 0.0), (0.0, 100.0))];
    assert!(find_corners(&on_middle).is_empty());
    // Two ends meet, but a third line passes through the point.
    let crossed = vec![seg("a", (0.0, 0.0), (100.0, 0.0)), seg("b", (0.0, 0.0), (0.0, 100.0)), seg("c", (-50.0, -50.0), (50.0, 50.0))];
    assert!(find_corners(&crossed).is_empty());
    // Three ends meet, and a fourth line passes through: neither.
    let mut crowded = tee.clone();
    crowded.push(seg("d", (-50.0, -50.0), (50.0, 50.0)));
    assert!(find_corners(&crowded).is_empty() && find_crowded(&crowded).is_empty());
}

#[test]
fn ends_within_a_thousandth_meet() {
    let near = vec![seg("a", (0.0, 0.0), (100.0, 0.0)), seg("b", (100.0, 0.0009), (100.0, 100.0))];
    assert_eq!(find_corners(&near).len(), 1);
    let apart = vec![seg("a", (0.0, 0.0), (100.0, 0.0)), seg("b", (100.0, 0.0011), (100.0, 100.0))];
    assert!(find_corners(&apart).is_empty());
}

#[test]
fn the_longer_member_runs_through() {
    let corner = |segs: &[(String, Vec3, Vec3)]| find_corners(segs).into_iter().next().unwrap();
    // The longer one, though the other lies along X.
    let longer = vec![seg("a", (0.0, 0.0), (400.0, 0.0)), seg("b", (0.0, 0.0), (0.0, 600.0))];
    assert_eq!(through(&longer, &corner(&longer), &segment_length(&longer), Some(Vec3::X), &[]), 1);
    // Within 1 mm: the one nearer X.
    let tie = vec![seg("a", (0.0, 0.0), (0.0, 400.5)), seg("b", (0.0, 0.0), (400.0, 0.0))];
    assert_eq!(through(&tie, &corner(&tie), &segment_length(&tie), Some(Vec3::X), &[]), 1);
    // Both at 45° to X and within 1 mm: the earlier.
    let diagonal = vec![seg("a", (0.0, 0.0), (300.0, 300.0)), seg("b", (0.0, 0.0), (-300.5, 300.5))];
    assert_eq!(through(&diagonal, &corner(&diagonal), &segment_length(&diagonal), Some(Vec3::X), &[]), 0);
    // No sketch plane: the earlier.
    assert_eq!(through(&tie, &corner(&tie), &segment_length(&tie), None, &[]), 0);
}

#[test]
fn mitre_cuts_on_the_bisector() {
    let segs = ell();
    let c = find_corners(&segs).into_iter().next().unwrap();
    let place = placer(&segs, Vec2::ZERO);
    let (cuts, cope) =
        corner_cuts(CornerType::Mitre, false, 0.0, &c, &segs, &segment_length(&segs), &place, &square(-20.0, 20.0), Some(Vec3::X), &[]).unwrap();
    assert_eq!(cope, None);
    assert_eq!(cuts.len(), 2);
    let d = Vec3::new(FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0);
    for (m, at_start, cut) in &cuts {
        assert!(at_start);
        assert!(close(cut.origin, Vec3::ZERO));
        assert!(close(cut.normal, if *m == 0 { d } else { -d }), "{cut:?}");
        // Each member keeps its own side: its far end is kept.
        let far = segs[*m].2;
        assert!((far - cut.origin).dot(cut.normal) > 0.0);
    }
}

#[test]
fn butt_through_member_is_flush_with_the_other_outer_face() {
    let segs = ell();
    let c = find_corners(&segs).into_iter().next().unwrap();
    let place = placer(&segs, Vec2::ZERO);
    let regions = square(-20.0, 20.0);
    let cut_of = |cuts: &[(usize, bool, Cut)], m: usize| cuts.iter().find(|(k, _, _)| *k == m).map(|(_, _, cut)| *cut).unwrap();

    let (cuts, cope) = corner_cuts(CornerType::Butt, false, 0.0, &c, &segs, &segment_length(&segs), &place, &regions, Some(Vec3::X), &[]).unwrap();
    assert_eq!((cuts.len(), cope), (2, None));
    // T is the 600 member: it keeps x ≥ −20, and U keeps y ≥ 20.
    let t = cut_of(&cuts, 0);
    assert!(close(t.normal, Vec3::X) && (t.origin.x + 20.0).abs() < 1e-9, "{t:?}");
    let u = cut_of(&cuts, 1);
    assert!(close(u.normal, Vec3::Y) && (u.origin.y - 20.0).abs() < 1e-9, "{u:?}");

    let (cuts, _) = corner_cuts(CornerType::Butt, true, 0.0, &c, &segs, &segment_length(&segs), &place, &regions, Some(Vec3::X), &[]).unwrap();
    // Flipped, T is the 400 member: it keeps y ≥ −20, and U keeps x ≥ 20.
    let t = cut_of(&cuts, 1);
    assert!(close(t.normal, Vec3::Y) && (t.origin.y + 20.0).abs() < 1e-9, "{t:?}");
    let u = cut_of(&cuts, 0);
    assert!(close(u.normal, Vec3::X) && (u.origin.x - 20.0).abs() < 1e-9, "{u:?}");

    // Coped Butt: T's cut only, and the pair (T, U) to cope.
    let (cuts, cope) =
        corner_cuts(CornerType::CopedButt, false, 0.0, &c, &segs, &segment_length(&segs), &place, &regions, Some(Vec3::X), &[]).unwrap();
    assert_eq!(cuts.len(), 1);
    assert_eq!(cuts[0].0, 0);
    assert_eq!(cope, Some((0, 1)));
}

#[test]
fn none_in_line_and_too_sharp() {
    let place_ell = ell();
    let c = find_corners(&place_ell).into_iter().next().unwrap();
    let place = placer(&place_ell, Vec2::ZERO);
    let regions = square(-20.0, 20.0);
    assert_eq!(
        corner_cuts(CornerType::None, false, 0.0, &c, &place_ell, &segment_length(&place_ell), &place, &regions, None, &[]).unwrap(),
        (vec![], None)
    );

    let straight = vec![seg("a", (0.0, 0.0), (100.0, 0.0)), seg("b", (100.0, 0.0), (200.0, 0.0))];
    let c = find_corners(&straight).into_iter().next().unwrap();
    assert!((interior(&straight, &c) - PI).abs() < 1e-12);
    let place = placer(&straight, Vec2::ZERO);
    assert_eq!(
        corner_cuts(CornerType::Butt, false, 0.0, &c, &straight, &segment_length(&straight), &place, &regions, None, &[]).unwrap(),
        (vec![], None)
    );

    let p = (12.5, -0.001);
    let (s, co) = (3f64.to_radians().sin(), 3f64.to_radians().cos());
    let sharp = vec![seg("a", p, (p.0 + 100.0, p.1)), seg("b", p, (p.0 + 100.0 * co, p.1 + 100.0 * s))];
    let c = find_corners(&sharp).into_iter().next().unwrap();
    let place = placer(&sharp, Vec2::ZERO);
    let err = corner_cuts(CornerType::Mitre, false, 0.0, &c, &sharp, &segment_length(&sharp), &place, &regions, None, &[]).unwrap_err();
    assert_eq!(err.to_string(), "Corner at (12.5, 0, 0) is too sharp to finish");
    assert!(corner_cuts(CornerType::None, false, 0.0, &c, &sharp, &segment_length(&sharp), &place, &regions, None, &[]).is_ok());
}

#[test]
fn reach_of_a_centred_square_and_a_bottom_left_one() {
    let regions = square(-20.0, 20.0);
    assert!((reach(&Plane::XY, &regions, Vec3::ZERO, Vec3::X) - 20.0).abs() < 1e-9);
    // Anchored at its bottom left: the plane's origin sits that far from the line.
    let at = frame::anchor_point(&regions, Anchor::BottomLeft);
    let plane = Plane::new(Vec3::ZERO - Vec3::X * at.x - Vec3::Y * at.y, Vec3::X, Vec3::Y).unwrap();
    assert!((reach(&plane, &regions, Vec3::ZERO, Vec3::X) - 40.0).abs() < 1e-9);
    assert!(reach(&plane, &regions, Vec3::ZERO, -Vec3::X).abs() < 1e-9);
    // An arc's extreme, not only its ends: a circle of radius 10 reaches 10 along a diagonal.
    let circle = vec![Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![] }];
    let diagonal = Vec3::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0);
    assert!((reach(&Plane::XY, &circle, Vec3::ZERO, diagonal) - 10.0).abs() < 1e-9);
}

#[test]
fn extension_reaches_every_point_of_each_cut() {
    let regions = square(-20.0, 20.0);
    // How far past the corner each member a corner cuts is built, in `corner_cuts` order.
    let built = |segs: &[(String, Vec3, Vec3)], kind| {
        let c = find_corners(segs).into_iter().next().unwrap();
        let place = placer(segs, Vec2::ZERO);
        let (cuts, _) = corner_cuts(kind, false, 0.0, &c, segs, &segment_length(segs), &place, &regions, None, &[]).unwrap();
        cuts.iter()
            .map(|(m, s, cut)| extension(&place(*m, c.point), &regions, c.point, away(segs, (*m, *s)).unwrap(), cut).unwrap())
            .collect::<Vec<_>>()
    };
    // A square mitre: the section's far corner is 20 back from the corner, and 1 mm more.
    let e = built(&ell(), CornerType::Mitre);
    assert!(e.iter().all(|e| (e - 21.0).abs() < 1e-9), "{e:?}");
    // A butt at 150°: T (along +x) runs back to U's outer face, 20 (1 + cos 30°) / sin 150° =
    // 74.641 at its far side. U is cut short of the corner and needs only the 1 mm.
    let (c, s) = (150f64.to_radians().cos(), 150f64.to_radians().sin());
    let wide = vec![seg("t", (0.0, 0.0), (400.0, 0.0)), seg("u", (0.0, 0.0), (300.0 * c, 300.0 * s))];
    let e = built(&wide, CornerType::Butt);
    let t = 20.0 * (1.0 + 30f64.to_radians().cos()) / s + 1.0;
    assert!(e.len() == 2 && (e[0] - t).abs() < 1e-9 && (e[1] - 1.0).abs() < 1e-9, "{e:?} vs {t}");
}

#[test]
fn a_member_not_at_a_corner_has_no_end_there() {
    let segs = ell();
    let c = find_corners(&segs).into_iter().next().unwrap();
    for e in c.ends {
        assert_eq!(end_at(&c, e.0).unwrap(), e);
    }
    let e = end_at(&c, 7).unwrap_err().to_string();
    assert!(e.contains("not at the corner"), "{e}");
}

#[test]
fn coped_cuts_a_butt_as_coped_butt_and_reads_an_old_coped_butt_as_butt() {
    // Under Coped (#17854) a Butt is cut as Coped Butt; every other type is cut as it is, and nothing without Coped.
    for kind in [CornerType::None, CornerType::Auto, CornerType::Mitre, CornerType::Butt] {
        let want = if kind == CornerType::Butt { CornerType::CopedButt } else { kind };
        assert_eq!((kind.coped(true), kind.coped(false)), (want, kind));
    }
    // An older build's `coped_butt`, the frame's or an override's, is Butt with Coped.
    let line = |l: &str| MemberRef { feature: None, line: l.into() };
    let o = |kind| CornerOverride { members: vec![line("a"), line("b")], kind, gap: None };
    for (frame, over, want) in [
        (CornerType::CopedButt, CornerType::Mitre, (CornerType::Butt, CornerType::Mitre, true)),
        (CornerType::Mitre, CornerType::CopedButt, (CornerType::Mitre, CornerType::Butt, true)),
        (CornerType::Butt, CornerType::Mitre, (CornerType::Butt, CornerType::Mitre, false)),
    ] {
        let (mut corner, mut corners, mut coped) = (frame, vec![o(over)], false);
        read_coped(&mut corner, &mut corners, &mut coped);
        assert_eq!((corner, corners[0].kind, coped), want);
    }
}

#[test]
fn old_corner_records_read_in_the_new_shape() {
    // A test build's record: bare string members, `corner`, `flip` (ignored: the members keep their written order).
    let old: CornerOverride = serde_json::from_str(r#"{"members":["l1","l2"],"corner":"butt","flip":true}"#).unwrap();
    let line = |l: &str| MemberRef { feature: None, line: l.into() };
    assert_eq!(old, CornerOverride { members: vec![line("l1"), line("l2")], kind: CornerType::Butt, gap: None });
    assert_eq!(old.lines().collect::<Vec<_>>(), ["l1", "l2"]);
    assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"members":[{"line":"l1"},{"line":"l2"}],"kind":"butt"}"#);
    // #4003's shape round-trips unchanged.
    let new = r#"{"members":[{"line":"l1"},{"feature":"Frame Member1","line":"l2"}],"kind":"auto"}"#;
    assert_eq!(serde_json::to_string(&serde_json::from_str::<CornerOverride>(new).unwrap()).unwrap(), new);
    // An older `coped_butt` still reads as Butt with Coped.
    let mut corners = vec![serde_json::from_str::<CornerOverride>(r#"{"members":["a","b"],"corner":"coped_butt"}"#).unwrap()];
    let (mut corner, mut coped) = (CornerType::Mitre, false);
    read_coped(&mut corner, &mut corners, &mut coped);
    assert_eq!((corners[0].kind, coped), (CornerType::Butt, true));
}
