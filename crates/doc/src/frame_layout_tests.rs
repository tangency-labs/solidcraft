use solidcore_record::Vec3;

use super::*;

fn seg(id: &str, a: (f64, f64), b: (f64, f64)) -> (String, Vec3, Vec3) {
    (id.to_string(), Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0))
}

/// Every member on the XY sketch plane, normal +Z, x +X.
fn xy(segments: &[(String, Vec3, Vec3)]) -> Vec<MemberPlane> {
    vec![Some((Vec3::Z, Vec3::X)); segments.len()]
}

/// Drawn in the order given.
fn in_order(segments: &[(String, Vec3, Vec3)]) -> Vec<usize> {
    (0..segments.len()).collect()
}

/// The ids in laying order, each with whether it is outer.
fn laid(segments: &[(String, Vec3, Vec3)]) -> Vec<(String, bool)> {
    ranks(segments, &xy(segments), &in_order(segments)).iter().map(|r| (segments[r.member].0.clone(), r.outer)).collect()
}

fn ids(v: &[(&str, bool)]) -> Vec<(String, bool)> {
    v.iter().map(|(s, o)| (s.to_string(), *o)).collect()
}

/// The 600×400 rectangle, drawn anticlockwise from the origin: bottom, right, top, left.
fn rectangle() -> Vec<(String, Vec3, Vec3)> {
    vec![
        seg("bottom", (0.0, 0.0), (600.0, 0.0)),
        seg("right", (600.0, 0.0), (600.0, 400.0)),
        seg("top", (600.0, 400.0), (0.0, 400.0)),
        seg("left", (0.0, 400.0), (0.0, 0.0)),
    ]
}

/// The rectangle with a cross-member at x = 300 and a diagonal from (0,0) to (300,400).
fn crossed() -> Vec<(String, Vec3, Vec3)> {
    let mut s = rectangle();
    s.push(seg("cross", (300.0, 0.0), (300.0, 400.0)));
    s.push(seg("diagonal", (0.0, 0.0), (300.0, 400.0)));
    s
}

#[test]
fn a_rectangles_long_sides_lay_first() {
    assert_eq!(laid(&rectangle()), ids(&[("bottom", true), ("top", true), ("right", true), ("left", true)]));
}

#[test]
fn a_squares_horizontal_sides_lay_first() {
    // Drawn verticals first, so only the tie on the sketch's X axis puts the horizontals first.
    let segs = vec![
        seg("right", (400.0, 0.0), (400.0, 400.0)),
        seg("left", (0.0, 400.0), (0.0, 0.0)),
        seg("bottom", (0.0, 0.0), (400.0, 0.0)),
        seg("top", (400.0, 400.0), (0.0, 400.0)),
    ];
    assert_eq!(laid(&segs), ids(&[("bottom", true), ("top", true), ("right", true), ("left", true)]));
}

#[test]
fn a_rectangle_with_a_cross_member_and_a_diagonal_lays_in_order() {
    let expected = [("bottom", true), ("top", true), ("right", true), ("left", true), ("cross", false), ("diagonal", false)];
    assert_eq!(laid(&crossed()), ids(&expected));
}

#[test]
fn an_l_shape_has_six_outer_tubes_and_its_inner_tube_last() {
    let segs = vec![
        seg("a", (0.0, 0.0), (600.0, 0.0)),
        seg("b", (600.0, 0.0), (600.0, 200.0)),
        seg("c", (600.0, 200.0), (200.0, 200.0)),
        seg("d", (200.0, 200.0), (200.0, 600.0)),
        seg("e", (200.0, 600.0), (0.0, 600.0)),
        seg("f", (0.0, 600.0), (0.0, 0.0)),
        seg("inner", (0.0, 0.0), (200.0, 200.0)),
    ];
    let out = laid(&segs);
    assert_eq!(out.len(), 7);
    assert!(out[..6].iter().all(|(_, outer)| *outer), "{out:?}");
    assert_eq!(out[6], ("inner".to_string(), false));
    // Longest first, ties on the X axis then drawn order: a (600, horizontal), f (600), c, d, e, b.
    let order: Vec<&str> = out.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(order, ["a", "f", "c", "d", "e", "b", "inner"]);
}

#[test]
fn a_tube_sticking_out_is_laid_last_and_is_not_outer() {
    let mut segs = rectangle();
    segs.push(seg("out", (600.0, 200.0), (800.0, 200.0)));
    assert_eq!(laid(&segs), ids(&[("bottom", true), ("top", true), ("right", true), ("left", true), ("out", false)]));
}

#[test]
fn a_u_and_an_h_lay_longest_first() {
    let u = vec![seg("left", (0.0, 400.0), (0.0, 0.0)), seg("bottom", (0.0, 0.0), (600.0, 0.0)), seg("right", (600.0, 0.0), (600.0, 300.0))];
    assert_eq!(laid(&u), ids(&[("bottom", false), ("left", false), ("right", false)]));
    // The H: the bar's ends on the middles of the two uprights.
    let h = vec![seg("bar", (0.0, 300.0), (400.0, 300.0)), seg("left", (0.0, 0.0), (0.0, 600.0)), seg("right", (400.0, 0.0), (400.0, 600.0))];
    assert_eq!(laid(&h), ids(&[("left", false), ("right", false), ("bar", false)]));
}

#[test]
fn two_groups_lay_by_their_longest_tube() {
    let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let segs = vec![
        // A 400×400 square in XY.
        ("s1".to_string(), p(0.0, 0.0, 0.0), p(400.0, 0.0, 0.0)),
        ("s2".to_string(), p(400.0, 0.0, 0.0), p(400.0, 400.0, 0.0)),
        ("s3".to_string(), p(400.0, 400.0, 0.0), p(0.0, 400.0, 0.0)),
        ("s4".to_string(), p(0.0, 400.0, 0.0), p(0.0, 0.0, 0.0)),
        // A 600×300 rectangle in XZ, sharing only the origin.
        ("r1".to_string(), p(0.0, 0.0, 0.0), p(-600.0, 0.0, 0.0)),
        ("r2".to_string(), p(-600.0, 0.0, 0.0), p(-600.0, 0.0, 300.0)),
        ("r3".to_string(), p(-600.0, 0.0, 300.0), p(0.0, 0.0, 300.0)),
        ("r4".to_string(), p(0.0, 0.0, 300.0), p(0.0, 0.0, 0.0)),
    ];
    let xz = Some((-Vec3::Y, Vec3::X));
    let planes = vec![Some((Vec3::Z, Vec3::X)), Some((Vec3::Z, Vec3::X)), Some((Vec3::Z, Vec3::X)), Some((Vec3::Z, Vec3::X)), xz, xz, xz, xz];
    let out = ranks(&segs, &planes, &in_order(&segs));
    let got: Vec<(&str, usize, bool)> = out.iter().map(|r| (segs[r.member].0.as_str(), r.group, r.outer)).collect();
    assert_eq!(
        got,
        [("r1", 0, true), ("r3", 0, true), ("r2", 0, true), ("r4", 0, true), ("s1", 1, true), ("s3", 1, true), ("s2", 1, true), ("s4", 1, true)]
    );
}

#[test]
fn members_in_no_plane_come_last_longest_first() {
    let mut segs = rectangle();
    segs.push(("short".to_string(), Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 100.0)));
    segs.push(("long".to_string(), Vec3::new(600.0, 0.0, 0.0), Vec3::new(600.0, 0.0, 300.0)));
    let mut planes = xy(&segs);
    planes[4] = None;
    planes[5] = None;
    let out = ranks(&segs, &planes, &in_order(&segs));
    let got: Vec<(&str, usize, bool)> = out.iter().map(|r| (segs[r.member].0.as_str(), r.group, r.outer)).collect();
    assert_eq!(got[4..], [("long", 1, false), ("short", 1, false)]);
}

#[test]
fn pick_order_never_changes_the_order() {
    let segs = crossed();
    let drawn = in_order(&segs);
    let order = |perm: &[usize]| {
        let s: Vec<_> = perm.iter().map(|&i| segs[i].clone()).collect();
        let d: Vec<_> = perm.iter().map(|&i| drawn[i]).collect();
        ranks(&s, &xy(&s), &d).iter().map(|r| (s[r.member].0.clone(), r.group, r.outer, r.reverse)).collect::<Vec<_>>()
    };
    let expected = order(&[0, 1, 2, 3, 4, 5]);
    assert_eq!(expected.len(), 6);
    for perm in [[5, 4, 3, 2, 1, 0], [2, 5, 0, 3, 1, 4], [4, 0, 5, 1, 3, 2]] {
        assert_eq!(order(&perm), expected, "{perm:?}");
    }
}

#[test]
fn outer_tubes_run_anticlockwise() {
    let mut segs = rectangle();
    segs[0] = seg("bottom", (600.0, 0.0), (0.0, 0.0));
    let out = ranks(&segs, &xy(&segs), &in_order(&segs));
    let reverse: Vec<(&str, bool)> = out.iter().map(|r| (segs[r.member].0.as_str(), r.reverse)).collect();
    assert_eq!(reverse, [("bottom", true), ("top", false), ("right", false), ("left", false)]);
    // Seen from behind (normal -Z), the same lines run the other way round.
    let behind = vec![Some((-Vec3::Z, Vec3::X)); segs.len()];
    let out = ranks(&segs, &behind, &in_order(&segs));
    assert!(out.iter().all(|r| r.reverse == (segs[r.member].0 != "bottom")), "{out:?}");
}

#[test]
fn hostile_members_never_panic_and_each_is_laid_once() {
    let nan = f64::NAN;
    let segs = vec![
        seg("a", (0.0, 0.0), (600.0, 0.0)),
        seg("nan", (nan, 0.0), (1.0, 1.0)),
        seg("point", (5.0, 5.0), (5.0, 5.0)),
        seg("huge", (-1e300, 0.0), (1e300, 0.0)),
        seg("b", (600.0, 0.0), (0.0, 0.0)),
    ];
    let mut planes = xy(&segs);
    planes[0] = Some((Vec3::new(nan, 0.0, 1.0), Vec3::X));
    let out = ranks(&segs, &planes, &[]);
    let mut members: Vec<usize> = out.iter().map(|r| r.member).collect();
    members.sort_unstable();
    assert_eq!(members, [0, 1, 2, 3, 4]);
    let p = plan(&segs, &planes, &[], &|_, _| true, &[]).unwrap();
    assert_eq!(p.ranks.len(), 5);
}

#[test]
fn a_bridge_between_two_closed_shapes_is_laid_last_and_is_not_outer() {
    let square = |n: &str, x0: f64| {
        vec![
            seg(&format!("{n}1"), (x0, 0.0), (x0 + 200.0, 0.0)),
            seg(&format!("{n}2"), (x0 + 200.0, 0.0), (x0 + 200.0, 200.0)),
            seg(&format!("{n}3"), (x0 + 200.0, 200.0), (x0, 200.0)),
            seg(&format!("{n}4"), (x0, 200.0), (x0, 0.0)),
        ]
    };
    let mut segs = vec![seg("bridge", (200.0, 100.0), (400.0, 100.0))];
    segs.extend(square("a", 0.0));
    segs.extend(square("b", 400.0));
    let out = laid(&segs);
    assert_eq!(out.len(), 9);
    assert!(out[..8].iter().all(|(_, outer)| *outer), "{out:?}");
    assert_eq!(out[8], ("bridge".to_string(), false));
}

#[test]
fn a_tube_crossing_the_outline_is_in_its_group() {
    let mut segs = rectangle();
    segs.push(seg("across", (-100.0, 200.0), (100.0, 200.0)));
    let out = ranks(&segs, &xy(&segs), &in_order(&segs));
    let last = out.last().map(|r| (segs[r.member].0.as_str(), r.group, r.outer));
    assert_eq!(out.len(), 5);
    assert_eq!(last, Some(("across", 0, false)));
}

// --- The plan (spec A4-A6) ---

fn planned(segments: &[(String, Vec3, Vec3)], overrides: &[Override]) -> Result<Plan> {
    plan(segments, &xy(segments), &in_order(segments), &|_, _| true, overrides)
}

/// The member index named id.
fn at(segments: &[(String, Vec3, Vec3)], id: &str) -> usize {
    segments.iter().position(|s| s.0 == id).unwrap()
}

fn auto(members: &[&str], flip: bool) -> Override {
    Override { members: members.iter().map(|s| s.to_string()).collect(), corner: CornerType::Auto, flip, gap: 0.0 }
}

#[test]
fn the_angle_table() {
    assert_eq!(table(20.0), Joint::AngledButt);
    assert_eq!(table(30.0), Joint::AngledButt);
    assert_eq!(table(45.0), Joint::Mitre);
    assert_eq!(table(60.0), Joint::Butt);
    assert_eq!(table(90.0), Joint::Butt);
    assert_eq!(table(120.0), Joint::Butt);
    assert_eq!(table(140.0), Joint::Mitre);
    assert_eq!(table(29.996), Joint::AngledButt); // rounds to 30.00
}

#[test]
fn a_rectangle_is_four_butts_with_the_long_sides_long() {
    let segs = rectangle();
    let p = planned(&segs, &[]).unwrap();
    assert_eq!(p.corners.len(), 4);
    assert!(p.fits.is_empty() && p.chained.is_empty());
    for c in &p.corners {
        assert_eq!((c.joint, c.kind, c.flip), (Joint::Butt, CornerType::Auto, false), "{c:?}");
        assert!((c.angle_deg - 90.0).abs() < 1e-9, "{c:?}");
        assert!(["bottom", "top"].contains(&segs[c.long.0].0.as_str()), "{c:?}");
        assert!(["right", "left"].contains(&segs[c.short.0].0.as_str()), "{c:?}");
    }
    // At the origin: the bottom's start and the left's end.
    let origin = p.corners.iter().find(|c| c.point.dist(Vec3::ZERO) < 1e-9).unwrap();
    assert_eq!((origin.long, origin.short), ((0, true), (3, false)));
}

#[test]
fn a_t_junction_is_a_fit_against_the_through_tube() {
    let segs = vec![seg("bar", (0.0, 0.0), (600.0, 0.0)), seg("stem", (300.0, 0.0), (300.0, 400.0))];
    let p = planned(&segs, &[]).unwrap();
    assert!(p.corners.is_empty());
    assert_eq!(p.fits, [Fit { member: 1, at_start: true, against: vec![0], gap: None }]);
}

/// Three lines ending at the origin: a along x (600), b along y (400), c at 45° (283).
fn three_ends() -> Vec<(String, Vec3, Vec3)> {
    vec![seg("c", (0.0, 0.0), (200.0, 200.0)), seg("a", (0.0, 0.0), (600.0, 0.0)), seg("b", (0.0, 400.0), (0.0, 0.0))]
}

#[test]
fn three_ends_make_one_corner_and_one_fit() {
    let segs = three_ends();
    let (a, b, c) = (at(&segs, "a"), at(&segs, "b"), at(&segs, "c"));
    let p = planned(&segs, &[]).unwrap();
    assert_eq!(p.corners.len(), 1);
    let corner = &p.corners[0];
    assert_eq!((corner.long, corner.short, corner.joint), ((a, true), (b, false), Joint::Butt));
    assert_eq!(p.fits, [Fit { member: c, at_start: true, against: vec![a, b], gap: None }]);
}

#[test]
fn mixed_sections_butt_instead_of_mitre() {
    let segs = vec![seg("a", (0.0, 0.0), (600.0, 0.0)), seg("b", (0.0, 0.0), (-300.0, 300.0))];
    let joint = |same: bool| {
        let p = plan(&segs, &xy(&segs), &in_order(&segs), &|_, _| same, &[]).unwrap();
        assert_eq!(p.corners.len(), 1);
        assert!((p.corners[0].angle_deg - 45.0).abs() < 1e-9, "{p:?}");
        p.corners[0].joint
    };
    assert_eq!(joint(true), Joint::Mitre);
    assert_eq!(joint(false), Joint::Butt);
}

#[test]
fn an_override_at_three_ends_flips_the_long_and_short() {
    let segs = three_ends();
    let (a, b) = (at(&segs, "a"), at(&segs, "b"));
    let p = planned(&segs, &[auto(&["a", "b", "c"], true)]).unwrap();
    let corner = &p.corners[0];
    assert_eq!((corner.long, corner.short, corner.kind, corner.flip), ((b, false), (a, true), CornerType::Auto, true));
    // Its first two make the corner (ruling 1), here a and c, not the two laid first; b is fitted against them.
    let c = at(&segs, "c");
    let p = planned(&segs, &[auto(&["c", "a", "b"], false)]).unwrap();
    assert_eq!((p.corners[0].long, p.corners[0].short), ((a, true), (c, true)));
    // It stands off by the override's gap.
    assert_eq!(p.fits, [Fit { member: b, at_start: false, against: vec![a, c], gap: Some(0.0) }]);
    // A Mitre override has nothing to flip.
    let mitre = Override { corner: CornerType::Mitre, ..auto(&["a", "b", "c"], true) };
    let corner = &planned(&segs, &[mitre]).unwrap().corners[0];
    assert_eq!((corner.long, corner.short, corner.kind), ((a, true), (b, false), CornerType::Mitre));
}

#[test]
fn an_override_whose_tubes_changed_fails() {
    let segs = vec![seg("l1", (0.0, 0.0), (600.0, 0.0)), seg("l2", (0.0, 400.0), (0.0, 0.0)), seg("l3", (600.0, 0.0), (600.0, 400.0))];
    let e = planned(&segs, &[auto(&["l1", "l2", "l3"], false)]).unwrap_err();
    assert_eq!(e.to_string(), DocError::Invalid("A corner override is not at a corner of the frame".into()).to_string());
    // Where one tube passes through, there is no corner to override either.
    let t = vec![seg("bar", (0.0, 0.0), (600.0, 0.0)), seg("stem", (300.0, 0.0), (300.0, 400.0))];
    assert!(planned(&t, &[auto(&["bar", "stem"], false)]).is_err());
    assert!(planned(&segs, &[auto(&["l1", "l2"], false)]).is_ok());
}

#[test]
fn crossing_tubes_make_no_corner_and_the_later_is_cut() {
    let segs = vec![seg("a", (0.0, 0.0), (400.0, 400.0)), seg("b", (0.0, 400.0), (400.0, 0.0))];
    let p = planned(&segs, &[]).unwrap();
    assert!(p.corners.is_empty() && p.chained.is_empty(), "{p:?}");
    assert_eq!(
        p.fits,
        [Fit { member: 1, at_start: false, against: vec![0], gap: None }, Fit { member: 2, at_start: true, against: vec![0], gap: None }]
    );
}

#[test]
fn tubes_in_no_plane_chain_and_mitre() {
    let p3 = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let mut segs = rectangle();
    // Up from the bottom tube's middle, then across: point pairs, in no sketch plane.
    segs.push(("pair1".to_string(), p3(300.0, 0.0, 0.0), p3(300.0, 0.0, 300.0)));
    segs.push(("pair2".to_string(), p3(300.0, 0.0, 300.0), p3(300.0, 300.0, 300.0)));
    let mut planes = xy(&segs);
    planes[4] = None;
    planes[5] = None;
    let p = plan(&segs, &planes, &in_order(&segs), &|_, _| true, &[]).unwrap();
    assert_eq!(p.corners.len(), 4, "the rectangle's corners only");
    assert_eq!(p.chained, [ChainCorner { a: (4, false), b: (5, true), kind: CornerType::Mitre, flip: false, overridden: false, gap: None }]);
    assert_eq!(p.fits, [Fit { member: 4, at_start: true, against: vec![0], gap: None }]);
    // An override at the chained corner is at a corner of the frame.
    let o = Override { members: vec!["pair1".into(), "pair2".into()], corner: CornerType::Butt, flip: false, gap: 0.0 };
    assert!(plan(&segs, &planes, &in_order(&segs), &|_, _| true, &[o]).is_ok());
}

#[test]
fn an_override_on_a_chained_corner_reaches_the_plan() {
    let p3 = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let segs =
        vec![("pair1".to_string(), p3(0.0, 0.0, 0.0), p3(0.0, 0.0, 300.0)), ("pair2".to_string(), p3(0.0, 0.0, 300.0), p3(300.0, 300.0, 300.0))];
    let planes = vec![None; segs.len()];
    let chained = |corner: CornerType, flip: bool| {
        let o = Override { members: vec!["pair1".into(), "pair2".into()], corner, flip, gap: 0.0 };
        plan(&segs, &planes, &in_order(&segs), &|_, _| true, &[o]).unwrap().chained
    };
    let butt = chained(CornerType::Butt, true);
    assert_eq!(butt.len(), 1);
    assert_eq!((butt[0].kind, butt[0].flip), (CornerType::Butt, true));
    // An Auto override is the chain's own joint, a mitre.
    let auto = chained(CornerType::Auto, false);
    assert_eq!((auto[0].kind, auto[0].flip), (CornerType::Mitre, false));
}

#[test]
fn groups_sharing_a_point_fit_the_later_group() {
    let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let segs = vec![
        ("s1".to_string(), p(0.0, 0.0, 0.0), p(400.0, 0.0, 0.0)),
        ("s2".to_string(), p(400.0, 0.0, 0.0), p(400.0, 400.0, 0.0)),
        ("s3".to_string(), p(400.0, 400.0, 0.0), p(0.0, 400.0, 0.0)),
        ("s4".to_string(), p(0.0, 400.0, 0.0), p(0.0, 0.0, 0.0)),
        ("r1".to_string(), p(0.0, 0.0, 0.0), p(-600.0, 0.0, 0.0)),
        ("r2".to_string(), p(-600.0, 0.0, 0.0), p(-600.0, 0.0, 300.0)),
        ("r3".to_string(), p(-600.0, 0.0, 300.0), p(0.0, 0.0, 300.0)),
        ("r4".to_string(), p(0.0, 0.0, 300.0), p(0.0, 0.0, 0.0)),
    ];
    let xz = Some((-Vec3::Y, Vec3::X));
    let mut planes = xy(&segs);
    planes[4..].fill(xz);
    let out = plan(&segs, &planes, &in_order(&segs), &|_, _| true, &[]).unwrap();
    let origin: Vec<&AutoCorner> = out.corners.iter().filter(|c| c.point.dist(Vec3::ZERO) < 1e-9).collect();
    assert_eq!(origin.len(), 1);
    assert_eq!((origin[0].long, origin[0].short), ((4, true), (7, false)), "the earlier group's r1 and r4");
    assert_eq!(out.corners.len(), 7);
    assert_eq!(
        out.fits,
        [Fit { member: 0, at_start: true, against: vec![4, 7], gap: None }, Fit { member: 3, at_start: false, against: vec![4, 7, 0], gap: None }]
    );
}

// --- Crossings (spec A4/A6, stage 3) and A17 ---

/// The 600×400 rectangle with both diagonals: d1 from the origin, d2 from (600, 0).
fn x_brace() -> Vec<(String, Vec3, Vec3)> {
    let mut s = rectangle();
    s.push(seg("d1", (0.0, 0.0), (600.0, 400.0)));
    s.push(seg("d2", (600.0, 0.0), (0.0, 400.0)));
    s
}

fn piece_ids(p: &Plan) -> Vec<&str> {
    p.pieces.iter().map(|q| q.id.as_str()).collect()
}

/// The piece index named id.
fn piece(p: &Plan, id: &str) -> usize {
    p.pieces.iter().position(|q| q.id == id).unwrap()
}

#[test]
fn an_x_brace_cuts_the_later_diagonal() {
    let segs = x_brace();
    let p = planned(&segs, &[]).unwrap();
    assert_eq!(piece_ids(&p), ["bottom", "right", "top", "left", "d1", "d2", "d2>d1"]);
    let (d1, d2, d2b) = (piece(&p, "d1"), piece(&p, "d2"), piece(&p, "d2>d1"));
    assert_eq!(p.pieces[d1], Piece { parent: 4, from: 0.0, to: 1.0, id: "d1".into() });
    assert_eq!((p.pieces[d2].parent, p.pieces[d2].from), (5, 0.0));
    assert!((p.pieces[d2].to - 0.5).abs() < 1e-9 && (p.pieces[d2b].from - 0.5).abs() < 1e-9, "{:?}", p.pieces);
    assert_eq!((p.pieces[d2b].parent, p.pieces[d2b].to), (5, 1.0));
    assert_eq!(p.crossings.len(), 1);
    assert!(p.crossings[0].dist(Vec3::new(300.0, 200.0, 0.0)) < 1e-9, "{:?}", p.crossings);
    // Each crossing end fits the whole diagonal, and only it: never its sibling.
    assert!(p.fits.contains(&Fit { member: d2, at_start: false, against: vec![d1], gap: None }), "{:?}", p.fits);
    assert!(p.fits.contains(&Fit { member: d2b, at_start: true, against: vec![d1], gap: None }), "{:?}", p.fits);
    assert!(!p.fits.iter().any(|f| (f.member == d2 && f.against.contains(&d2b)) || (f.member == d2b && f.against.contains(&d2))), "{:?}", p.fits);
    // The pieces take their line's place in the laying order, in order along it.
    let order: Vec<&str> = p.ranks.iter().map(|r| p.pieces[r.member].id.as_str()).collect();
    assert_eq!(order, ["bottom", "top", "right", "left", "d1", "d2", "d2>d1"]);
    assert!(p.corners.iter().all(|c| c.point.dist(Vec3::new(300.0, 200.0, 0.0)) > 1.0), "no corner at the crossing");
}

#[test]
fn a_crossing_override_with_flip_keeps_the_other_whole() {
    let segs = x_brace();
    let p = planned(&segs, &[auto(&["d1", "d2"], true)]).unwrap();
    assert_eq!(piece_ids(&p), ["bottom", "right", "top", "left", "d1", "d1>d2", "d2"]);
    let (d1, d1b, d2) = (piece(&p, "d1"), piece(&p, "d1>d2"), piece(&p, "d2"));
    assert!(p.fits.contains(&Fit { member: d1, at_start: false, against: vec![d2], gap: Some(0.0) }), "{:?}", p.fits);
    assert!(p.fits.contains(&Fit { member: d1b, at_start: true, against: vec![d2], gap: Some(0.0) }), "{:?}", p.fits);
    assert_eq!(p.crossing_overrides, [0]);
    // Without Flip it matches (a crossing's override) and changes nothing else: its pieces' ends there take its gap.
    let mut plain = planned(&segs, &[Override { gap: 3.0, ..auto(&["d2", "d1"], false) }]).unwrap();
    assert_eq!(plain.crossing_overrides, [0]);
    plain.crossing_overrides.clear();
    let diagonal: Vec<bool> = plain.pieces.iter().map(|pc| pc.id.starts_with('d')).collect();
    let at_crossing = |f: &Fit| diagonal[f.member] && f.against.iter().any(|&k| diagonal[k]);
    assert_eq!(plain.fits.iter().filter(|f| at_crossing(f)).count(), 2, "{:?}", plain.fits);
    for f in plain.fits.iter_mut().filter(|f| at_crossing(f)) {
        assert_eq!(f.gap, Some(3.0), "{f:?}");
        f.gap = None;
    }
    assert_eq!(plain, planned(&segs, &[]).unwrap());
}

#[test]
fn three_lines_through_one_point_and_a_fourth_ending_there() {
    let segs = vec![
        seg("d", (300.0, 300.0), (500.0, 100.0)),
        seg("c", (100.0, 100.0), (450.0, 450.0)),
        seg("b", (300.0, 0.0), (300.0, 500.0)),
        seg("a", (0.0, 300.0), (600.0, 300.0)),
    ];
    let p = planned(&segs, &[]).unwrap();
    assert_eq!(piece_ids(&p), ["d", "c", "c>a", "b", "b>a", "a"]);
    assert_eq!(p.crossings.len(), 1);
    let [d, c, c2, b, b2, a] = ["d", "c", "c>a", "b", "b>a", "a"].map(|id| piece(&p, id));
    let fits = [
        Fit { member: b, at_start: false, against: vec![a], gap: None },
        Fit { member: b2, at_start: true, against: vec![a], gap: None },
        Fit { member: c, at_start: false, against: vec![a, b, b2], gap: None },
        Fit { member: c2, at_start: true, against: vec![a, b, b2], gap: None },
        Fit { member: d, at_start: true, against: vec![a, b, b2, c, c2], gap: None },
    ];
    assert_eq!(p.fits, fits);
    assert!(p.corners.is_empty() && p.chained.is_empty(), "{p:?}");
}

/// A line L drawn first, crossed by two longer lines X1 and X2 (left out without x2).
fn crossed_twice(x2: bool) -> Vec<(String, Vec3, Vec3)> {
    let mut s = vec![seg("L", (50.0, 0.0), (350.0, 0.0)), seg("X1", (100.0, -200.0), (100.0, 200.0))];
    if x2 {
        s.push(seg("X2", (300.0, -200.0), (300.0, 200.0)));
    }
    s
}

#[test]
fn a_line_crossed_twice_has_three_pieces() {
    let p = planned(&crossed_twice(true), &[]).unwrap();
    assert_eq!(piece_ids(&p), ["L", "L>X1", "L>X2", "X1", "X2"]);
    let spans: Vec<(i64, i64)> = p.pieces[..3].iter().map(|q| ((q.from * 6.0).round() as i64, (q.to * 6.0).round() as i64)).collect();
    assert_eq!(spans, [(0, 1), (1, 5), (5, 6)]);
    assert_eq!(p.crossings.len(), 2);
}

#[test]
fn removing_a_crossing_keeps_the_other_piece_ids() {
    let with = planned(&crossed_twice(true), &[]).unwrap();
    let without = planned(&crossed_twice(false), &[]).unwrap();
    assert_eq!(piece_ids(&without), ["L", "L>X1", "X1"]);
    // L's first piece is unchanged; L>X1 grows over where L>X2 was.
    assert_eq!(with.pieces[0], without.pieces[0]);
    assert_eq!(with.pieces[1].from, without.pieces[1].from);
    assert_eq!(without.pieces[1].to, 1.0);
}

#[test]
fn a_far_end_override_survives_a_new_crossing() {
    // L and M meet at (300, 0); the brace B crosses L at x = 100 and is longer, so it stays whole.
    let mut segs = vec![seg("L", (0.0, 0.0), (300.0, 0.0)), seg("M", (300.0, 0.0), (300.0, 300.0))];
    let corner = |segs: &[(String, Vec3, Vec3)]| {
        let p = planned(segs, &[auto(&["M", "L"], true)]).unwrap();
        let c = p.corners.iter().find(|c| c.point.dist(Vec3::new(300.0, 0.0, 0.0)) < 1e-9).cloned().unwrap();
        (c.overridden, c.flip, p.pieces[c.long.0].id.clone(), p.pieces[c.short.0].id.clone())
    };
    assert_eq!(corner(&segs), (true, true, "M".to_string(), "L".to_string()));
    segs.push(seg("B", (100.0, -300.0), (100.0, 300.0)));
    assert_eq!(corner(&segs), (true, true, "M".to_string(), "L>B".to_string()));
}

#[test]
fn pair_ends_on_a_group_tube_are_fitted_not_chained() {
    let p3 = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let pairs = |at: Vec3| {
        let mut segs = rectangle();
        segs.push(("pair1".to_string(), at, at + p3(0.0, 0.0, 300.0)));
        segs.push(("pair2".to_string(), at, at + p3(0.0, -300.0, 300.0)));
        let mut planes = xy(&segs);
        planes[4] = None;
        planes[5] = None;
        plan(&segs, &planes, &in_order(&segs), &|_, _| true, &[]).unwrap()
    };
    // On the bottom tube's middle: pair2 (the longer) fits it, then pair1 fits it and pair2.
    let p = pairs(p3(300.0, 0.0, 0.0));
    assert!(p.chained.is_empty(), "{p:?}");
    assert_eq!(
        p.fits,
        [Fit { member: 5, at_start: true, against: vec![0], gap: None }, Fit { member: 4, at_start: true, against: vec![0, 5], gap: None }]
    );
    // At the rectangle's corner, where group ends meet.
    let p = pairs(p3(600.0, 0.0, 0.0));
    assert!(p.chained.is_empty(), "{p:?}");
    assert_eq!(
        p.fits,
        [Fit { member: 5, at_start: true, against: vec![0, 1], gap: None }, Fit { member: 4, at_start: true, against: vec![0, 1, 5], gap: None }]
    );
}

#[test]
fn pick_order_never_changes_the_pieces() {
    let segs = x_brace();
    let drawn = in_order(&segs);
    let shape = |perm: &[usize]| {
        let s: Vec<_> = perm.iter().map(|&i| segs[i].clone()).collect();
        let d: Vec<_> = perm.iter().map(|&i| drawn[i]).collect();
        let p = plan(&s, &xy(&s), &d, &|_, _| true, &[]).unwrap();
        let id = |k: usize| p.pieces[k].id.clone();
        let mut pieces: Vec<(String, String, i64, i64)> =
            p.pieces.iter().map(|q| (q.id.clone(), s[q.parent].0.clone(), (q.from * 1e6).round() as i64, (q.to * 1e6).round() as i64)).collect();
        pieces.sort();
        let fits: Vec<(String, bool, Vec<String>)> =
            p.fits.iter().map(|f| (id(f.member), f.at_start, f.against.iter().map(|&k| id(k)).collect())).collect();
        let order: Vec<String> = p.ranks.iter().map(|r| id(r.member)).collect();
        (pieces, fits, order, p.crossings.clone())
    };
    let expected = shape(&[0, 1, 2, 3, 4, 5]);
    assert_eq!(expected.0.len(), 7);
    for perm in [[5, 4, 3, 2, 1, 0], [2, 5, 0, 3, 1, 4], [4, 0, 5, 1, 3, 2]] {
        assert_eq!(shape(&perm), expected, "{perm:?}");
    }
}

#[test]
fn no_crossings_outside_groups() {
    let p3 = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let mut segs = rectangle();
    // A pair across the bottom tube, and two pairs crossing each other above the rectangle.
    segs.push(("across".to_string(), p3(300.0, -100.0, 0.0), p3(300.0, 100.0, 0.0)));
    segs.push(("p1".to_string(), p3(100.0, 100.0, 100.0), p3(300.0, 300.0, 100.0)));
    segs.push(("p2".to_string(), p3(100.0, 300.0, 100.0), p3(300.0, 100.0, 100.0)));
    let mut planes = xy(&segs);
    planes[4..].fill(None);
    let p = plan(&segs, &planes, &in_order(&segs), &|_, _| true, &[]).unwrap();
    assert_eq!(piece_ids(&p), segs.iter().map(|s| s.0.as_str()).collect::<Vec<_>>());
    assert!(p.crossings.is_empty());
}

#[test]
fn without_crossings_each_line_is_one_whole_piece() {
    let segs = crossed();
    let p = planned(&segs, &[]).unwrap();
    let whole: Vec<Piece> = segs.iter().enumerate().map(|(m, s)| Piece { parent: m, from: 0.0, to: 1.0, id: s.0.clone() }).collect();
    assert_eq!(p.pieces, whole);
    assert!(p.crossings.is_empty());
    let laid: Vec<usize> = ranks(&segs, &xy(&segs), &in_order(&segs)).iter().map(|r| r.member).collect();
    assert_eq!(p.ranks.iter().map(|r| r.member).collect::<Vec<_>>(), laid);
}
