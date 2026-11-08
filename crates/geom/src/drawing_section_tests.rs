use super::*;
use crate::bspline::tests::Lcg;
use crate::drawing_view::NamedView;
use solidcore_record::Vec3;

fn line(kind: SectionKind, points: &[(f64, f64)], arrows: ArrowSide) -> Result<CuttingLine> {
    CuttingLine::new(kind, points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(), arrows)
}

fn full(a: (f64, f64), b: (f64, f64), arrows: ArrowSide) -> CuttingLine {
    line(SectionKind::Full, &[a, b], arrows).unwrap()
}

fn axes(plane: Plane) -> (Vec3, Vec3) {
    (plane.x, plane.y)
}

/// A cutting line along one of a view's paper axes gives the view turned a quarter turn about
/// it, exactly: on a front view the top, bottom, left and right views, however the line was drawn.
#[test]
fn a_line_along_a_paper_axis_gives_the_parent_turned_about_it() {
    use ArrowSide::{Left, Right};
    let front = NamedView::Front.basis();
    let cases = [
        // Horizontal, arrows down (to the right of a line drawn left to right), then up.
        (((-10.0, 5.0), (50.0, 5.0)), Right, NamedView::Top),
        (((-10.0, 5.0), (50.0, 5.0)), Left, NamedView::Bottom),
        // The same line drawn right to left: the arrows' side swaps, the view doesn't.
        (((50.0, 5.0), (-10.0, 5.0)), Left, NamedView::Top),
        // Vertical, arrows right, then left.
        (((5.0, -10.0), (5.0, 50.0)), Right, NamedView::Left),
        (((5.0, -10.0), (5.0, 50.0)), Left, NamedView::Right),
    ];
    for ((a, b), arrows, view) in cases {
        let basis = full(a, b, arrows).basis(&front).unwrap();
        assert_eq!(axes(basis), axes(view.basis()), "{arrows:?} → {view:?}");
    }
    // A half section looks across its first segment, not its last (which here runs down).
    let half = line(SectionKind::Half, &[(-10.0, 5.0), (20.0, 5.0), (20.0, -10.0)], Right).unwrap();
    assert_eq!(axes(half.basis(&front).unwrap()), axes(NamedView::Top.basis()));
    // On a top view, a horizontal line with its arrows up gives the front view: as projected below it.
    let top = NamedView::Top.basis();
    assert_eq!(axes(full((0.0, 5.0), (40.0, 5.0), Left).basis(&top).unwrap()), axes(NamedView::Front.basis()));
    let projected = crate::drawing_view::projected_basis(&top, crate::drawing_view::ProjectedSide::Below, crate::cell::Projection::Third).unwrap();
    assert_eq!(axes(full((0.0, 5.0), (40.0, 5.0), Left).basis(&top).unwrap()), axes(projected));
}

/// A slanted line's view looks the arrows' way: its normal is against the arrows, the line lies in
/// its plane, its axes are orthonormal, and it goes through the parent's origin.
#[test]
fn a_slanted_line_looks_the_arrows_way() {
    let parent = Plane { origin: Vec3::new(3.0, -2.0, 7.0), ..NamedView::IsoFrontRight.basis() };
    let (a, b) = (Vec2::new(1.0, 2.0), Vec2::new(1.0 + 30f64.to_radians().cos() * 20.0, 2.0 + 30f64.to_radians().sin() * 20.0));
    for arrows in [ArrowSide::Left, ArrowSide::Right] {
        let cut = CuttingLine::new(SectionKind::Full, vec![a, b], arrows).unwrap();
        let basis = cut.basis(&parent).unwrap();
        validate_plane(&basis).unwrap();
        let world = |v: Vec2| parent.x * v.x + parent.y * v.y;
        let direction = world((b - a).normalized().unwrap());
        let to_arrows = world(match arrows {
            ArrowSide::Left => (b - a).perp(),
            ArrowSide::Right => -(b - a).perp(),
        })
        .normalized()
        .unwrap();
        assert!((basis.x.cross(basis.y) + to_arrows).len() < 1e-12, "{arrows:?}: the viewer is behind the arrows");
        assert!(basis.x.cross(basis.y).dot(direction).abs() < 1e-12, "the line lies in the view's plane");
        assert_eq!(basis.origin, parent.origin);
    }
}

/// Each segment's plane holds the segment and the parent's normal, its normal toward the arrows.
#[test]
fn each_segment_cuts_along_a_plane_through_it_and_the_parents_normal() {
    let parent = NamedView::Front.basis();
    let normal = parent.x.cross(parent.y);
    for arrows in [ArrowSide::Left, ArrowSide::Right] {
        let cut = line(SectionKind::Offset, &[(0.0, 10.0), (15.0, 10.0), (15.0, 25.0), (40.0, 25.0)], arrows).unwrap();
        let planes = cut.planes(&parent).unwrap();
        assert_eq!(planes.len(), 3);
        for (plane, w) in planes.iter().zip(cut.points().windows(2)) {
            let world = |p: Vec2| parent.origin + parent.x * p.x + parent.y * p.y;
            let n = plane.x.cross(plane.y);
            for p in [world(w[0]), world(w[1]), world(w[0]) + normal * 9.0] {
                assert!((p - plane.origin).dot(n).abs() < 1e-12, "the plane holds the segment and the normal");
            }
            let side = match arrows {
                ArrowSide::Left => (w[1] - w[0]).perp(),
                ArrowSide::Right => -(w[1] - w[0]).perp(),
            };
            assert!(n.dot(parent.x * side.x + parent.y * side.y) > 0.0, "{arrows:?}: the normal points to the arrows");
        }
    }
}

/// How much of a 40 × 40 box, sampled on a 1 mm grid of cell centres, a region holds.
fn kept_of_box(region: &Region2) -> usize {
    (0..40).flat_map(|i| (0..40).map(move |j| Vec2::new(f64::from(i) + 0.5, f64::from(j) + 0.5))).filter(|p| region.outer.contains(*p)).count()
}

const BOX: Aabb2 = Aabb2 { min: Vec2::ZERO, max: Vec2::new(40.0, 40.0) };

/// The kept region holds the arrows' side of the box: half for a full cut, three quarters for a
/// half section, the stepped side for an offset one, the V's side for an aligned one. Each is one
/// anticlockwise loop with no holes, whichever side the arrows are.
#[test]
fn the_kept_region_holds_the_arrows_side() {
    use ArrowSide::{Left, Right};
    use SectionKind::{Aligned, Full, Half, Offset};
    let cases: [(SectionKind, &[(f64, f64)], ArrowSide, usize); 9] = [
        (Full, &[(-5.0, 20.0), (45.0, 20.0)], Right, 800),
        (Full, &[(-5.0, 20.0), (45.0, 20.0)], Left, 800),
        // Inside the box: carried on past it.
        (Full, &[(10.0, 10.0), (12.0, 10.0)], Right, 400),
        // Through the box's corner, slanted (no cell's centre on it): the triangle below, 40 × 20 / 2.
        (Full, &[(0.0, 0.0), (40.0, 20.0)], Right, 400),
        // From the left to the middle, then down: the quarter it walks round on its right.
        (Half, &[(-5.0, 20.0), (20.0, 20.0), (20.0, -5.0)], Right, 400),
        (Half, &[(-5.0, 20.0), (20.0, 20.0), (20.0, -5.0)], Left, 1200),
        // Steps up at x = 15: below y = 10 left of it, below y = 30 right of it.
        (Offset, &[(-5.0, 10.0), (15.0, 10.0), (15.0, 30.0), (45.0, 30.0)], Right, 150 + 750),
        // A right-angled V about (20, 20): the quarter above right of it on its left.
        (Aligned, &[(20.0, 45.0), (20.0, 20.0), (45.0, 20.0)], Left, 400),
        // A line that misses the box: all of it or none.
        (Full, &[(-5.0, 60.0), (45.0, 60.0)], Right, 1600),
    ];
    for (kind, points, arrows, cells) in cases {
        let region = line(kind, points, arrows).unwrap().kept(BOX).unwrap();
        assert!(region.holes.is_empty() && region.outer.is_ccw(), "{kind:?} {arrows:?}");
        assert_eq!(kept_of_box(&region), cells, "{kind:?} {points:?} {arrows:?}");
    }
    assert_eq!(kept_of_box(&full((-5.0, 60.0), (45.0, 60.0), Left).kept(BOX).unwrap()), 0);
}

/// An aligned section's second arm turns onto the first carried on past the centre.
#[test]
fn an_aligned_sections_second_arm_turns_onto_the_first() {
    let quarter = line(SectionKind::Aligned, &[(-10.0, 0.0), (0.0, 0.0), (0.0, 10.0)], ArrowSide::Left).unwrap();
    assert!((quarter.turn().unwrap() + std::f64::consts::FRAC_PI_2).abs() < 1e-15);
    let (s, c) = 120f64.to_radians().sin_cos();
    let wide = line(SectionKind::Aligned, &[(-10.0, 0.0), (0.0, 0.0), (10.0 * c, 10.0 * s)], ArrowSide::Left).unwrap();
    let turn = wide.turn().unwrap();
    assert!((turn + 120f64.to_radians()).abs() < 1e-12, "{turn}");
    // Turned, the arm lies along the first arm's way.
    let arm = Vec2::new(10.0 * c, 10.0 * s);
    let laid = Vec2::new(arm.x * turn.cos() - arm.y * turn.sin(), arm.x * turn.sin() + arm.y * turn.cos());
    assert!((laid - Vec2::new(10.0, 0.0)).len() < 1e-12);
    assert_eq!(full((0.0, 0.0), (1.0, 0.0), ArrowSide::Left).turn(), None);
}

/// Every rule refuses with its own message, at and past its bounds; at a tolerance's exact value
/// the line is kept ([`PARALLEL`] is 1e-9; the second vector of each is of length 1 exactly).
#[test]
fn hostile_lines_are_refused() {
    use ArrowSide::Left;
    use SectionKind::{Aligned, Full, Half, Offset};
    let refused = |kind, points: &[(f64, f64)]| line(kind, points, Left).unwrap_err().0;
    assert_eq!(refused(Full, &[]), "a cutting line has 2 to 32 points");
    assert_eq!(refused(Full, &[(0.0, 0.0)]), "a cutting line has 2 to 32 points");
    let many: Vec<(f64, f64)> = (0..33).map(|i| (f64::from(i), f64::from(i % 2))).collect();
    assert_eq!(refused(Offset, &many), "a cutting line has 2 to 32 points");
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, 1.0000001e12] {
        assert_eq!(refused(Full, &[(0.0, 0.0), (bad, 1.0)]), "a cutting line's points are finite and within 1e12 mm");
    }
    assert!(line(Full, &[(0.0, 0.0), (1e12, -1e12)], Left).is_ok());
    assert_eq!(refused(Full, &[(1.0, 1.0), (1.0, 1.0)]), "a cutting line's points are at least 0.000001 mm apart");
    assert_eq!(refused(Full, &[(0.0, 0.0), (9e-7, 0.0)]), "a cutting line's points are at least 0.000001 mm apart");
    assert!(line(Full, &[(0.0, 0.0), (1e-6, 0.0)], Left).is_ok());
    assert_eq!(refused(Full, &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]), "a full section's cutting line has 2 points");
    assert_eq!(refused(Half, &[(0.0, 0.0), (1.0, 0.0)]), "a half section's cutting line has 3 points");
    // A square turn: cosine 1e-9 is kept, a little more refused; a turn of 89°, a turn back, refused.
    assert!(line(Half, &[(-1.0, 0.0), (0.0, 0.0), (1e-9, 1.0)], Left).is_ok());
    let square = "a half section's cutting line turns a square corner";
    assert_eq!(refused(Half, &[(-1.0, 0.0), (0.0, 0.0), (1.1e-9, 1.0)]), square);
    let (s, c) = 89f64.to_radians().sin_cos();
    assert_eq!(refused(Half, &[(0.0, 0.0), (1.0, 0.0), (1.0 + c, s)]), square);
    assert_eq!(refused(Half, &[(0.0, 0.0), (1.0, 0.0), (0.5, 0.0)]), square);
    assert_eq!(refused(Offset, &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]), "an offset section's cutting line has an even number of points, 4 or more");
    assert_eq!(refused(Offset, &[(0.0, 0.0), (1.0, 0.0)]), "an offset section's cutting line has an even number of points, 4 or more");
    assert_eq!(refused(Offset, &[(0.0, 0.0), (1.0, 0.0), (1.1, 1.0), (2.0, 1.0)]), "an offset section's cutting line turns only square corners");
    // A leg back the other way, or one off parallel by more than the tolerance (its step square).
    let along = "an offset section's legs all run the same way as its first";
    assert_eq!(refused(Offset, &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]), along);
    assert!(line(Offset, &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (2.0, 1.0)], Left).is_ok());
    let aligned = "an aligned section's arms meet at an angle, not straight on or folded back";
    assert_eq!(refused(Aligned, &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]), aligned);
    assert_eq!(refused(Aligned, &[(0.0, 0.0), (1.0, 0.0), (0.0, 0.0)]), aligned);
    assert_eq!(refused(Aligned, &[(0.0, 0.0), (1.0, 0.0), (1.0, 0.0)]), "a cutting line's points are at least 0.000001 mm apart");
    assert_eq!(refused(Aligned, &[(0.0, 0.0), (1.0, 0.0), (0.5, 0.0)]), aligned);
    // Sine 1e-9 exactly is kept, a little less refused.
    assert!(line(Aligned, &[(0.0, 0.0), (1.0, 0.0), (2.0, 1e-9)], Left).is_ok());
    assert_eq!(refused(Aligned, &[(0.0, 0.0), (1.0, 0.0), (2.0, 0.9e-9)]), aligned);
    assert_eq!(refused(Aligned, &[(0.0, 0.0), (1.0, 0.0)]), "an aligned section's cutting line has 3 points, the centre in the middle");
    // A parent that isn't a basis, a box that is inside out or not finite.
    let cut = full((0.0, 0.0), (1.0, 0.0), Left);
    let skewed = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::X };
    assert!(cut.basis(&skewed).is_err() && cut.planes(&skewed).is_err());
    for around in [Aabb2::EMPTY, Aabb2 { min: Vec2::ZERO, max: Vec2::new(f64::NAN, 1.0) }, Aabb2 { min: Vec2::ZERO, max: Vec2::new(1e13, 1.0) }] {
        assert!(cut.kept(around).is_err());
    }
}

/// Seeded lines of every kind, in boxes of every size: nothing panics; every kept region is one
/// anticlockwise loop, with points just off each segment inside on the arrows' side and outside on
/// the other.
#[test]
fn seeded_lines_keep_their_arrows_side() {
    let mut rng = Lcg(0x5ec7);
    let mut made = [0usize; 4];
    for _ in 0..4000 {
        let scale = 10f64.powf(rng.unit() * 8.0 - 3.0);
        let mut at = Vec2::new(rng.unit() - 0.5, rng.unit() - 0.5) * scale;
        let first = Vec2::from_angle(rng.unit() * std::f64::consts::TAU);
        let kinds = [SectionKind::Full, SectionKind::Half, SectionKind::Offset, SectionKind::Aligned];
        let pick = (rng.unit() * 4.0) as usize % 4;
        let kind = kinds[pick];
        let length = |rng: &mut Lcg| scale * (0.05 + rng.unit());
        let mut points = vec![at];
        let mut step = |way: Vec2, rng: &mut Lcg| {
            at += way * length(rng);
            points.push(at);
        };
        match kind {
            SectionKind::Full => step(first, &mut rng),
            SectionKind::Half => {
                step(first, &mut rng);
                let side = if rng.unit() < 0.5 { first.perp() } else { -first.perp() };
                step(side, &mut rng);
            }
            SectionKind::Offset => {
                for _ in 0..1 + (rng.unit() * 4.0) as usize {
                    step(first, &mut rng);
                    let side = if rng.unit() < 0.5 { first.perp() } else { -first.perp() };
                    step(side, &mut rng);
                }
                step(first, &mut rng);
            }
            SectionKind::Aligned => {
                step(first, &mut rng);
                step(Vec2::from_angle(first.angle() + 0.2 + rng.unit() * 2.7), &mut rng);
            }
        }
        let arrows = if rng.unit() < 0.5 { ArrowSide::Left } else { ArrowSide::Right };
        let Ok(cut) = CuttingLine::new(kind, points.clone(), arrows) else {
            continue;
        };
        made[pick] += 1;
        let around = Aabb2 { min: Vec2::new(-scale, -scale) * rng.unit(), max: Vec2::new(scale, scale) * rng.unit() + Vec2::new(1e-3, 1e-3) * scale };
        let region = cut.kept(around).unwrap();
        assert!(region.holes.is_empty() && region.outer.is_ccw(), "{kind:?} {points:?}");
        for w in points.windows(2) {
            let (mid, away) = ((w[0] + w[1]) * 0.5, (w[1] - w[0]).perp().normalized().unwrap() * scale * 1e-4);
            let left = region.outer.contains(mid + away);
            let right = region.outer.contains(mid - away);
            assert_eq!((left, right), (arrows == ArrowSide::Left, arrows == ArrowSide::Right), "{kind:?} {points:?} {arrows:?}");
        }
    }
    assert!(made.iter().all(|n| *n > 500), "{made:?}");
}

/// A line reads back as it was written, and a saved line that breaks a rule is refused on reading.
#[test]
fn a_line_round_trips_and_a_bad_one_is_refused_on_reading() {
    let cut = line(SectionKind::Half, &[(-5.0, 20.0), (20.0, 20.0), (20.0, -5.0)], ArrowSide::Right).unwrap();
    let text = serde_json::to_string(&cut).unwrap();
    assert!(text.starts_with(r#"{"kind":"half","points":"#) && text.ends_with(r#","arrows":"right"}"#), "{text}");
    assert_eq!(serde_json::from_str::<CuttingLine>(&text).unwrap(), cut);
    let bent = Stored { kind: SectionKind::Half, points: vec![Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.5)], arrows: ArrowSide::Right };
    let bent = serde_json::to_string(&bent).unwrap();
    assert!(serde_json::from_str::<CuttingLine>(&bent).unwrap_err().to_string().contains("turns a square corner"));
    let mut extra = serde_json::to_value(&cut).unwrap();
    extra["label"] = serde_json::json!("A");
    assert!(serde_json::from_value::<CuttingLine>(extra).is_err());
}
