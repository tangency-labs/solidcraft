//! Trim along curves on a face against closed forms: the square sheet [0, 20]^2 at z = 0 cut by
//! curves drawn above it and projected down onto it (`sweep::project_curves`), its regions'
//! areas and the points inside them, the regions kept and discarded, and the refusals; and
//! bodies cut along curves dropped square onto their faces (`trim_by_dropped_curves`), keeping
//! the regions points pick.
// The tests index and unwrap as tests do.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use std::f64::consts::PI;

use solidcore_ops::sweep::{extrude, planar_faces, project_curves, Curve, DraftCorner};
use solidcore_ops::trim::{
    remove_faces, split_face_by_curves, trim_by_dropped_curves, Region, MAX_PICKS,
};
use solidcore_ops::{Body, Error, Order};

const DOWN: [f64; 3] = [0.0, 0.0, -1.0];
const TOL: f64 = 1e-6;

fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::Line { start: a, end: b }
}

/// The closed polygon through `pts`.
fn polygon(pts: &[[f64; 3]]) -> Vec<Curve> {
    (0..pts.len())
        .map(|i| line(pts[i], pts[(i + 1) % pts.len()]))
        .collect()
}

/// The square from `lo` to `hi` along x and y at height `z`, as a closed polygon.
fn square(lo: f64, hi: f64, z: f64) -> Vec<Curve> {
    polygon(&[[lo, lo, z], [hi, lo, z], [hi, hi, z], [lo, hi, z]])
}

/// The square [0, 20]^2 at z = 0 as a sheet of one planar face, and its record order.
fn sheet() -> (Body, Order) {
    let b = planar_faces(&square(0.0, 20.0, 0.0)).unwrap();
    let (_, order) = b.to_record().unwrap();
    (b, order)
}

/// `curves` (drawn at z = 5) projected down onto the sheet's face.
fn projected(b: &Body, o: &Order, curves: &[Curve]) -> Vec<Curve> {
    project_curves(b, o, 0, curves, DOWN, TOL)
        .unwrap()
        .into_iter()
        .map(|(c, _)| c)
        .collect()
}

fn rel(a: f64, want: f64) -> f64 {
    (a - want).abs() / want.abs()
}

fn area(b: &Body) -> f64 {
    b.area(1e-8).unwrap()
}

/// The two regions of a cut, the smaller first, with their areas.
fn two(cut: &Body, regions: &[Region]) -> [(Region, f64); 2] {
    assert_eq!(regions.len(), 2, "{regions:?}");
    let (_, o) = cut.to_record().unwrap();
    assert_eq!(o.faces.len(), 2, "two faces");
    let mut out = [0, 1].map(|i| {
        let r = regions[i];
        (r, cut.face_area(&o, r.face, 1e-8).unwrap())
    });
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

/// Whether `p` is on the sheet, inside its boundary.
fn on_sheet(p: [f64; 3]) -> bool {
    p[2].abs() < 1e-9 && (0.0..=20.0).contains(&p[0]) && (0.0..=20.0).contains(&p[1])
}

/// Whether `p` is strictly inside the square [5, 15]^2.
fn in_middle(p: [f64; 3]) -> bool {
    p[0] > 5.0 && p[0] < 15.0 && p[1] > 5.0 && p[1] < 15.0
}

/// The square [5, 15]^2 projected onto the sheet cuts it into the square, area 100, and the
/// sheet round it, area 300, each with a point inside it. Keeping the inside discards the
/// sheet round it, keeping the outside discards the square.
#[test]
fn a_closed_curve_cuts_a_sheet_in_two() {
    let (b, o) = sheet();
    let curves = projected(&b, &o, &square(5.0, 15.0, 5.0));
    let (cut, regions) = split_face_by_curves(&b, &o, 0, &curves, TOL).unwrap();
    let [(inner, inner_area), (outer, outer_area)] = two(&cut, &regions);
    assert!(rel(inner_area, 100.0) < 1e-6, "inside {inner_area}");
    assert!(rel(outer_area, 300.0) < 1e-6, "outside {outer_area}");
    assert!(
        on_sheet(inner.inside) && in_middle(inner.inside),
        "{inner:?}"
    );
    assert!(
        on_sheet(outer.inside) && !in_middle(outer.inside),
        "{outer:?}"
    );
    let (_, co) = cut.to_record().unwrap();
    let inside = remove_faces(&cut, &co, &[outer.face]).unwrap();
    assert_eq!(inside.counts().unwrap()[0], 1, "keep inside: one face");
    assert!(rel(area(&inside), 100.0) < 1e-6, "keep inside");
    let outside = remove_faces(&cut, &co, &[inner.face]).unwrap();
    assert_eq!(outside.counts().unwrap()[0], 1, "keep outside: one face");
    assert!(rel(area(&outside), 300.0) < 1e-6, "keep outside");
    assert_eq!(cut.counts().unwrap()[0], 2, "the cut body stays as it was");
}

/// A line across the whole sheet at y = 10 cuts it into two halves of area 200, one point
/// below the line and one above.
#[test]
fn an_open_curve_from_edge_to_edge_cuts_a_sheet_in_two() {
    let (b, o) = sheet();
    let curves = projected(&b, &o, &[line([-5.0, 10.0, 5.0], [25.0, 10.0, 5.0])]);
    let (cut, regions) = split_face_by_curves(&b, &o, 0, &curves, TOL).unwrap();
    let [(a, a_area), (c, c_area)] = two(&cut, &regions);
    assert!(rel(a_area, 200.0) < 1e-6 && rel(c_area, 200.0) < 1e-6);
    assert!(on_sheet(a.inside) && on_sheet(c.inside), "{regions:?}");
    let below = |p: [f64; 3]| p[1] < 10.0;
    assert!(below(a.inside) != below(c.inside), "{regions:?}");
}

/// A line that stops short of the sheet's boundary at both ends leaves a slit, no region:
/// refused, and so is a curve off the face.
#[test]
fn an_open_curve_inside_the_face_is_refused() {
    let (b, o) = sheet();
    let curves = projected(&b, &o, &[line([5.0, 10.0, 5.0], [15.0, 10.0, 5.0])]);
    assert_eq!(
        split_face_by_curves(&b, &o, 0, &curves, TOL).err(),
        Some(Error::CurveDoesNotCut)
    );
    let off = [line([30.0, 30.0, 0.0], [40.0, 30.0, 0.0])];
    assert_eq!(
        split_face_by_curves(&b, &o, 0, &off, TOL).err(),
        Some(Error::CurveDoesNotCut)
    );
    assert_eq!(b.counts().unwrap()[0], 1, "the sheet stays as it was");
}

#[test]
fn bad_asks_are_refused() {
    let (b, o) = sheet();
    let curves = projected(&b, &o, &square(5.0, 15.0, 5.0));
    let split = |f, c: &[Curve], t| split_face_by_curves(&b, &o, f, c, t).err();
    assert_eq!(split(1, &curves, TOL), Some(Error::Argument), "no face 1");
    assert_eq!(split(0, &[], TOL), Some(Error::Argument), "no curve");
    assert_eq!(
        split(0, &curves, f64::NAN),
        Some(Error::Argument),
        "tolerance"
    );
    assert_eq!(split(0, &curves, 0.0), Some(Error::Argument), "tolerance");
    let remove = |faces: &[usize]| remove_faces(&b, &o, faces).err();
    assert_eq!(remove(&[]), Some(Error::Argument), "no face");
    assert_eq!(remove(&[0, 0]), Some(Error::Argument), "a face twice");
    assert_eq!(remove(&[1]), Some(Error::Argument), "no face 1");
    assert_eq!(remove(&[usize::MAX]), Some(Error::Argument));
    assert_eq!(remove(&[0]), Some(Error::Empty), "nothing left");
    assert_eq!(b.counts().unwrap()[0], 1, "the sheet stays as it was");
}

/// The half cylinder of radius 10 about Z over y >= 0, from z = 0 to 40: a sheet of one face,
/// area 10 pi * 40.
fn half_cylinder() -> Body {
    let arc = Curve::Arc {
        center: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: 10.0,
        start: 0.0,
        end: PI,
    };
    let b = extrude(
        &[arc],
        [0.0, 0.0, 1.0],
        40.0,
        0.0,
        DraftCorner::Round,
        false,
    )
    .unwrap();
    assert_eq!(b.counts().unwrap()[0], 1, "one face");
    b
}

/// The slot of half width `a` drawn in the plane y = `d`, about the line x = 0: straight sides
/// from z = 12 to 28 and round ends about (0, d, 12) and (0, d, 28). One closed loop.
fn slot(a: f64, d: f64) -> [Curve; 4] {
    // counter-clockwise seen from +y towards the axis: angle 0 along +x, a quarter turn at +z
    let end = |z: f64, start: f64| Curve::Arc {
        center: [0.0, d, z],
        axis: [0.0, -1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: a,
        start,
        end: start + PI,
    };
    [
        line([a, d, 12.0], [a, d, 28.0]),
        end(28.0, 0.0),
        line([-a, d, 28.0], [-a, d, 12.0]),
        end(12.0, PI),
    ]
}

/// The area the slot of [`slot`] (half width `a`, drawn at the distance `d` from the axis)
/// covers on a cylinder of radius `r` about Z once dropped square onto it, in closed form. A
/// point (x, d, z) of the slot's plane has its foot on the cylinder at the angle atan2(d, x), at
/// the same height, so a width 2 w of the slot at one height lands on the arc 2 r atan(w / d):
/// - the straight run (w = a over a height of 16): 16 * 2 r atan(a / d);
/// - the two round ends (w = sqrt(a^2 - t^2) for t from -a to a, once each): the integral of
///   atan(sqrt(a^2 - t^2) / d) over t from 0 to a is (pi / 2) (sqrt(a^2 + d^2) - d), so the
///   ends cover 2 pi r (sqrt(a^2 + d^2) - d) together.
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn slot_area_on_cylinder(r: f64, a: f64, d: f64) -> f64 {
    32.0 * r * (a / d).atan() + 2.0 * PI * r * ((a * a + d * d).sqrt() - d)
}

/// A slot of half width a = 3 drawn in the plane y = d = 20 and dropped square onto the half
/// cylinder of radius 10 about Z: the piece inside it is [`slot_area_on_cylinder`], 61.7033...;
/// the piece outside is the rest of 10 pi * 40. The straight sides drop onto rulings, exactly;
/// the round ends are within TOL of their true drop, and no longer than the arcs they come from
/// (the drop scales widths by at most r / d < 1 and keeps heights), 2 pi a together: the area is
/// within TOL * 2 pi a of the closed form, 1.9e-5.
#[test]
fn a_slot_dropped_onto_a_half_cylinder_cuts_out_its_closed_form_area() {
    let (r, a, d) = (10.0_f64, 3.0_f64, 20.0_f64);
    let b = half_cylinder();
    let inside = slot_area_on_cylinder(r, a, d);
    let all = PI * r * 40.0;
    let bound = TOL * 2.0 * PI * a;
    // the foot of the slot's middle, and a point of the wall well outside the slot
    let (middle, beside) = ([0.0, r, 20.0], [r, 0.0, 20.0]);

    let slot = [slot(a, d).to_vec()];
    let kept = trim_by_dropped_curves(&b, &slot, &[middle], TOL).unwrap();
    assert_eq!(kept.counts().unwrap()[0], 1, "inside: one face");
    assert!(
        (area(&kept) - inside).abs() < bound,
        "inside {} against {inside}",
        area(&kept)
    );

    let rest = trim_by_dropped_curves(&b, &slot, &[beside], TOL).unwrap();
    assert_eq!(rest.counts().unwrap()[0], 1, "outside: one face");
    assert!(
        (area(&rest) - (all - inside)).abs() < bound,
        "outside {} against {}",
        area(&rest),
        all - inside
    );

    // both regions picked: the whole wall, cut in two faces
    let both = trim_by_dropped_curves(&b, &slot, &[middle, beside], TOL).unwrap();
    assert_eq!(both.counts().unwrap()[0], 2, "both: two faces");
    assert!(rel(area(&both), all) < 1e-9, "both {}", area(&both));
    assert_eq!(
        b.counts().unwrap()[0],
        1,
        "the half cylinder stays as it was"
    );
}

/// The drop is square to the surface, not along the sketch's normal: the same slot drawn twice
/// as far from the axis lands on a narrower arc (atan(a / 2d) for atan(a / d)), where a
/// projection along y would land on the same one. With d = 40 the inside is
/// [`slot_area_on_cylinder`]'s 31.0138...
#[test]
fn a_slot_drawn_farther_away_drops_onto_a_narrower_piece() {
    let (r, a, d) = (10.0_f64, 3.0_f64, 40.0_f64);
    let inside = slot_area_on_cylinder(r, a, d);
    let slot = [slot(a, d).to_vec()];
    let kept = trim_by_dropped_curves(&half_cylinder(), &slot, &[[0.0, r, 20.0]], TOL).unwrap();
    assert!(
        (area(&kept) - inside).abs() < TOL * 2.0 * PI * a,
        "inside {} against {inside}",
        area(&kept)
    );
}

/// Two walls in one body, joined at a right angle along the y axis: the floor z = 0 over
/// x in [0, 10] and the wall x = 0 over z in [0, 10], both over y in [0, 10]. The square
/// [3, 7]^2 drawn 5 above the floor drops onto the floor as itself, area 16; square to the wall
/// it drops onto the one segment y in [3, 7] at z = 5, which bounds nothing, so the wall stays
/// whole. Keeping the square's inside leaves 16; keeping the floor round it leaves the floor's
/// 84 and the whole wall's 100, one region across the join.
#[test]
fn a_face_the_dropped_curves_bound_nothing_on_stays_whole() {
    let chain = [
        line([0.0, 0.0, 10.0], [0.0, 0.0, 0.0]),
        line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
    ];
    let b = extrude(
        &chain,
        [0.0, 1.0, 0.0],
        10.0,
        0.0,
        DraftCorner::Round,
        false,
    )
    .unwrap();
    assert_eq!(b.counts().unwrap()[0], 2, "two faces");
    let above = [polygon(&[
        [3.0, 3.0, 5.0],
        [7.0, 3.0, 5.0],
        [7.0, 7.0, 5.0],
        [3.0, 7.0, 5.0],
    ])];
    let inside = trim_by_dropped_curves(&b, &above, &[[5.0, 5.0, 0.0]], TOL).unwrap();
    assert_eq!(inside.counts().unwrap()[0], 1);
    assert!(rel(area(&inside), 16.0) < 1e-9, "inside {}", area(&inside));
    // a point on the wall picks the wall's region, which runs on round the square
    let round = trim_by_dropped_curves(&b, &above, &[[0.0, 5.0, 8.0]], TOL).unwrap();
    assert_eq!(round.counts().unwrap()[0], 2);
    assert!(rel(area(&round), 184.0) < 1e-9, "round {}", area(&round));
}

/// What a dropped-curve trim refuses: a curve that bounds nothing (an open line that stops
/// inside the sheet), a curve that lands nowhere on the body, a point on the cut line itself,
/// and hostile numbers. The body stays as it was.
#[test]
fn a_dropped_trim_refuses_what_does_not_cut_or_cannot_be_picked() {
    let (b, _) = sheet();
    // one chain
    let trim =
        |c: &[Curve], p: &[[f64; 3]], t| trim_by_dropped_curves(&b, &[c.to_vec()], p, t).err();
    let inner = square(5.0, 15.0, 5.0);
    let on = [10.0, 10.0, 0.0];
    let slit = [line([5.0, 10.0, 5.0], [15.0, 10.0, 5.0])];
    assert_eq!(trim(&slit, &[on], TOL), Some(Error::CurveDoesNotCut));
    // a curve beside the sheet lands on no face, alone or with a square that does cut
    let off = [line([30.0, 30.0, 5.0], [40.0, 30.0, 5.0])];
    assert!(matches!(
        trim(&off, &[on], TOL),
        Some(Error::Unsupported(_))
    ));
    assert!(matches!(
        trim_by_dropped_curves(&b, &[inner.clone(), off.to_vec()], &[on], TOL),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(
        trim_by_dropped_curves(&b, &[], &[on], TOL).err(),
        Some(Error::Argument),
        "no chain"
    );
    // on the cut line x = 5: as near the square as the sheet round it
    assert!(matches!(
        trim(&inner, &[[5.0, 10.0, 0.0]], TOL),
        Some(Error::Unsupported(_))
    ));
    assert_eq!(
        trim(&[], &[on], TOL),
        Some(Error::Argument),
        "an empty chain"
    );
    assert_eq!(trim(&inner, &[], TOL), Some(Error::Argument), "no point");
    assert_eq!(
        trim(&inner, &[[f64::NAN, 0.0, 0.0]], TOL),
        Some(Error::Argument)
    );
    assert_eq!(
        trim(&inner, &[[f64::INFINITY, 0.0, 0.0]], TOL),
        Some(Error::Argument)
    );
    assert_eq!(trim(&inner, &[on], f64::NAN), Some(Error::Argument));
    assert_eq!(trim(&inner, &[on], 0.0), Some(Error::Argument));
    let many = vec![on; MAX_PICKS + 1];
    assert_eq!(trim(&inner, &many, TOL), Some(Error::Argument));
    assert_eq!(b.counts().unwrap()[0], 1, "the sheet stays as it was");
    // and the square does cut it: inside 100, outside 300
    let kept = trim_by_dropped_curves(&b, std::slice::from_ref(&inner), &[on], TOL).unwrap();
    assert!(rel(area(&kept), 100.0) < 1e-9, "{}", area(&kept));
    let round = trim_by_dropped_curves(&b, &[inner], &[[1.0, 1.0, 0.0]], TOL).unwrap();
    assert!(rel(area(&round), 300.0) < 1e-9, "{}", area(&round));
}

/// The dropped curves are cut where the face ends, not where its surface's parameter box does:
/// the triangle (0, 0), (20, 0), (0, 20) at z = 0 is half of its box, and the line y = 5 drawn
/// above it from x = -5 to 25 drops onto the box's whole width. It divides the triangle into
/// the strip below it, 20 * 5 - 5^2 / 2 = 87.5, and the triangle above it, 15^2 / 2 = 112.5.
#[test]
fn a_dropped_curve_past_a_faces_boundary_is_cut_at_the_boundary() {
    let b = planar_faces(&polygon(&[
        [0.0, 0.0, 0.0],
        [20.0, 0.0, 0.0],
        [0.0, 20.0, 0.0],
    ]))
    .unwrap();
    let across = [vec![line([-5.0, 5.0, 5.0], [25.0, 5.0, 5.0])]];
    let below = trim_by_dropped_curves(&b, &across, &[[2.0, 1.0, 0.0]], TOL).unwrap();
    assert!(rel(area(&below), 87.5) < 1e-9, "below {}", area(&below));
    let above = trim_by_dropped_curves(&b, &across, &[[2.0, 9.0, 0.0]], TOL).unwrap();
    assert!(rel(area(&above), 112.5) < 1e-9, "above {}", area(&above));
}

/// On a whole cylinder (radius 10 about Z, 40 high, with its two flat ends) the wall is one
/// face closed on its seam, the ruling at y = 0, x = 10. A slot drawn facing the wall from
/// +y, -x or -y lies clear of the seam and cuts out [`slot_area_on_cylinder`]'s piece, within
/// the bound of [`a_slot_dropped_onto_a_half_cylinder_cuts_out_its_closed_form_area`]; the flat
/// ends, on which the dropped slot bounds nothing, stay whole. Facing the wall from +x the
/// slot's round ends cross the seam, and the kernel's drop does not follow a curve round a
/// seam: it gives them no curve. That is refused with its reason, never cut as half a slot.
#[test]
fn a_slot_across_a_closed_faces_seam_is_refused_and_one_clear_of_it_cuts() {
    let (r, a, d) = (10.0_f64, 3.0_f64, 20.0_f64);
    let can = solidcore_ops::make_cylinder([0.0; 3], r, 40.0).unwrap();
    assert_eq!(can.counts().unwrap()[0], 3, "a wall and two ends");
    let inside = slot_area_on_cylinder(r, a, d);
    // the slot of [`slot`] turned about Z to face the wall along the unit n
    let facing = |n: [f64; 2]| {
        let at = |s: f64, z: f64| [n[0] * d + n[1] * s, n[1] * d - n[0] * s, z];
        let end = |z: f64, start: f64| Curve::Arc {
            center: at(0.0, z),
            axis: [-n[0], -n[1], 0.0],
            reference: [n[1], -n[0], 0.0],
            radius: a,
            start,
            end: start + PI,
        };
        [
            line(at(a, 12.0), at(a, 28.0)),
            end(28.0, 0.0),
            line(at(-a, 28.0), at(-a, 12.0)),
            end(12.0, PI),
        ]
    };
    for n in [[0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]] {
        let foot = [n[0] * r, n[1] * r, 20.0];
        let kept = trim_by_dropped_curves(&can, &[facing(n).to_vec()], &[foot], TOL).unwrap();
        assert_eq!(kept.counts().unwrap()[0], 1, "{n:?}: the piece alone");
        assert!(
            (area(&kept) - inside).abs() < TOL * 2.0 * PI * a,
            "{n:?}: inside {} against {inside}",
            area(&kept)
        );
    }
    assert!(matches!(
        trim_by_dropped_curves(&can, &[facing([1.0, 0.0]).to_vec()], &[[r, 0.0, 20.0]], TOL),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(can.counts().unwrap()[0], 3, "the cylinder stays as it was");
}

/// Two lines that cross (x = 10 and y = 10, each a chain) cut the sheet [0, 20]^2 into four
/// squares of 100. The second line splits the first line's edge where they cross, and both
/// pieces of it stay cut lines: a point picks its own square alone, whichever it is, and two
/// points pick two.
#[test]
fn crossing_cut_lines_part_all_the_regions_they_bound() {
    let (b, _) = sheet();
    let cross = [
        vec![line([10.0, -5.0, 5.0], [10.0, 25.0, 5.0])],
        vec![line([-5.0, 10.0, 5.0], [25.0, 10.0, 5.0])],
    ];
    for p in [
        [5.0, 5.0, 0.0],
        [15.0, 5.0, 0.0],
        [15.0, 15.0, 0.0],
        [5.0, 15.0, 0.0],
    ] {
        let kept = trim_by_dropped_curves(&b, &cross, &[p], TOL).unwrap();
        assert_eq!(kept.counts().unwrap()[0], 1, "{p:?}");
        assert!(rel(area(&kept), 100.0) < 1e-9, "{p:?}: {}", area(&kept));
    }
    let opposite = [[5.0, 5.0, 0.0], [15.0, 15.0, 0.0]];
    let two = trim_by_dropped_curves(&b, &cross, &opposite, TOL).unwrap();
    assert_eq!(two.counts().unwrap()[0], 2);
    assert!(rel(area(&two), 200.0) < 1e-9, "{}", area(&two));
}

/// A chain that ends on another chain's line cuts whichever is listed first. The line y = 10
/// halves the sheet [0, 20]^2; the line x = 10 from y = 10 up past the sheet's edge halves the
/// upper half. Alone the second leaves a slit, so listed first it waits for the first. The
/// lower half is 200 and the two upper pieces are 100 each.
#[test]
fn a_chain_that_ends_on_another_chains_line_cuts_in_either_order() {
    let (b, _) = sheet();
    let across = vec![line([-5.0, 10.0, 5.0], [25.0, 10.0, 5.0])];
    let up = vec![line([10.0, 10.0, 5.0], [10.0, 25.0, 5.0])];
    for chains in [[across.clone(), up.clone()], [up, across]] {
        for (p, want) in [
            ([5.0, 5.0, 0.0], 200.0),
            ([5.0, 15.0, 0.0], 100.0),
            ([15.0, 15.0, 0.0], 100.0),
        ] {
            let kept = trim_by_dropped_curves(&b, &chains, &[p], TOL).unwrap();
            assert_eq!(kept.counts().unwrap()[0], 1, "{p:?}");
            assert!(rel(area(&kept), want) < 1e-9, "{p:?}: {}", area(&kept));
        }
    }
}

/// A chain that bounds nothing is refused, however well the others cut; it never goes missing
/// from a cut that succeeds. On the triangle (0, 0), (20, 0), (0, 20) the line y = 5 cuts, as
/// in [`a_dropped_curve_past_a_faces_boundary_is_cut_at_the_boundary`]. With it:
/// - a segment from (12, 12) to (15, 12) drops inside the triangle's parameter box but off the
///   triangle itself;
/// - a segment from (2, 8) to (4, 8) drops inside the triangle and ends there, a slit.
///
/// Each is refused in either order, and the triangle stays as it was.
#[test]
fn a_chain_that_bounds_nothing_is_refused_beside_one_that_cuts() {
    let b = planar_faces(&polygon(&[
        [0.0, 0.0, 0.0],
        [20.0, 0.0, 0.0],
        [0.0, 20.0, 0.0],
    ]))
    .unwrap();
    let across = vec![line([-5.0, 5.0, 5.0], [25.0, 5.0, 5.0])];
    let off_the_face = vec![line([12.0, 12.0, 5.0], [15.0, 12.0, 5.0])];
    let slit = vec![line([2.0, 8.0, 5.0], [4.0, 8.0, 5.0])];
    let keep = [[2.0, 1.0, 0.0]];
    for stray in [off_the_face, slit] {
        for chains in [[across.clone(), stray.clone()], [stray, across.clone()]] {
            assert_eq!(
                trim_by_dropped_curves(&b, &chains, &keep, TOL).err(),
                Some(Error::CurveDoesNotCut)
            );
        }
    }
    assert_eq!(b.counts().unwrap()[0], 1, "the triangle stays as it was");
}

/// A cut line that ends on an edge two faces share splits that edge, and both pieces still
/// join the faces. The floor and wall of
/// [`a_face_the_dropped_curves_bound_nothing_on_stays_whole`] with the line y = 5 drawn above
/// the floor from the fold to the floor's far side: the floor is cut in two halves of 50, each
/// still joined to the wall along its piece of the fold, so the whole body is one region. Any
/// point keeps all of it: three faces, 200.
#[test]
fn a_cut_that_splits_a_shared_edge_leaves_its_faces_joined() {
    let chain = [
        line([0.0, 0.0, 10.0], [0.0, 0.0, 0.0]),
        line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
    ];
    let b = extrude(
        &chain,
        [0.0, 1.0, 0.0],
        10.0,
        0.0,
        DraftCorner::Round,
        false,
    )
    .unwrap();
    let across = [vec![line([-5.0, 5.0, 5.0], [15.0, 5.0, 5.0])]];
    for p in [[5.0, 2.0, 0.0], [5.0, 8.0, 0.0], [0.0, 5.0, 8.0]] {
        let kept = trim_by_dropped_curves(&b, &across, &[p], TOL).unwrap();
        assert_eq!(kept.counts().unwrap()[0], 3, "{p:?}");
        assert!(rel(area(&kept), 200.0) < 1e-9, "{p:?}: {}", area(&kept));
    }
}
