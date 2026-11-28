use super::*;

fn line(a: (f64, f64), b: (f64, f64)) -> Seg2 {
    Seg2::Line {
        a: Vec2::new(a.0, a.1),
        b: Vec2::new(b.0, b.1),
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Seg2> {
    vec![
        line((x0, y0), (x1, y0)),
        line((x1, y0), (x1, y1)),
        line((x1, y1), (x0, y1)),
        line((x0, y1), (x0, y0)),
    ]
}

fn area(r: &[Vec2]) -> f64 {
    (0..r.len())
        .map(|k| cross(Vec2::ZERO, r[k], r[(k + 1) % r.len()]))
        .sum::<f64>()
        / 2.0
}

#[test]
fn a_drawing_with_inner_lines_gives_its_outer_boundary() {
    // A plate seen from above: its outline, the top face's edges drawn again on it, a hole and a
    // line across the middle. Only the outer boundary is the footprint.
    let mut c = rect(0.0, 0.0, 40.0, 20.0);
    c.extend(rect(0.0, 0.0, 40.0, 20.0));
    c.extend(rect(10.0, 5.0, 15.0, 10.0));
    c.push(line((0.0, 10.0), (40.0, 10.0)));
    let f = outline_of(&c).unwrap();
    assert!(!f.hull);
    assert!(f.shape.holes.is_empty());
    assert_eq!(f.shape.outer.len(), 4, "{:?}", f.shape.outer);
    assert!((area(&f.shape.outer) - 800.0).abs() < 1e-9);
}

#[test]
fn overlapping_outlines_give_their_union() {
    // An L seen as two boxes drawn over each other (a base and an upright), with a T where the
    // upright's side ends on the base's top edge.
    let mut c = rect(0.0, 0.0, 30.0, 10.0);
    c.extend(rect(0.0, 5.0, 10.0, 30.0));
    let f = outline_of(&c).unwrap();
    assert!(!f.hull);
    assert!(
        (area(&f.shape.outer) - (300.0 + 200.0)).abs() < 1e-9,
        "{:?}",
        f.shape.outer
    );
    assert_eq!(f.shape.outer.len(), 6);
}

#[test]
fn arcs_are_flattened_within_the_reported_error() {
    // A slot: two half circles joined by lines.
    let c = vec![
        line((0.0, 0.0), (50.0, 0.0)),
        Seg2::Arc {
            center: Vec2::new(50.0, 10.0),
            radius: 10.0,
            start: -std::f64::consts::FRAC_PI_2,
            sweep: std::f64::consts::PI,
        },
        line((50.0, 20.0), (0.0, 20.0)),
        Seg2::Arc {
            center: Vec2::new(0.0, 10.0),
            radius: 10.0,
            start: std::f64::consts::FRAC_PI_2,
            sweep: std::f64::consts::PI,
        },
    ];
    let f = outline_of(&c).unwrap();
    let want = 50.0 * 20.0 + std::f64::consts::PI * 100.0;
    let got = area(&f.shape.outer);
    assert!(
        got <= want + 1e-9 && got > want - f.error * 2.0 * (50.0 + 20.0 * std::f64::consts::PI),
        "{got} {want} {}",
        f.error
    );
    // Every corner of the outline is on the true curve, within the error.
    for p in &f.shape.outer {
        let d = if p.x < 0.0 {
            ((p.x).hypot(p.y - 10.0) - 10.0).abs()
        } else if p.x > 50.0 {
            ((p.x - 50.0).hypot(p.y - 10.0) - 10.0).abs()
        } else {
            p.y.min(20.0 - p.y).abs()
        };
        assert!(d <= f.error, "{p:?} is {d} off");
    }
}

#[test]
fn a_curve_drawn_as_many_short_lines_is_flattened_within_the_reported_error() {
    // A disc whose rim comes already chorded (as a spline side does from the hidden-line work),
    // in more lines than an outline may have corners, each bowed out past the tidying of corners
    // a hair off their neighbours' line.
    let n = 4_500;
    let at = |k: usize| {
        let a = std::f64::consts::TAU * k as f64 / n as f64;
        (40.0 * a.cos(), 40.0 * a.sin())
    };
    let c: Vec<Seg2> = (0..n).map(|k| line(at(k), at(k + 1))).collect();
    let f = outline_of(&c).unwrap();
    assert!(!f.hull, "{}", f.shape.outer.len());
    assert!(
        f.shape.outer.len() <= MAX_CORNERS,
        "{}",
        f.shape.outer.len()
    );
    assert!(f.error > 0.0 && f.error < 0.1, "{}", f.error);
    // Every corner of the outline is on the rim, and every rim corner is within the error of
    // the outline's edges.
    for p in &f.shape.outer {
        assert!((p.x.hypot(p.y) - 40.0).abs() < 1e-9, "{p:?}");
    }
    let r = &f.shape.outer;
    for k in 0..n {
        let (x, y) = at(k);
        let p = Vec2::new(x, y);
        let d = (0..r.len())
            .map(|i| {
                let (a, b) = (r[i], r[(i + 1) % r.len()]);
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
                (a + ab * t - p).length()
            })
            .fold(f64::INFINITY, f64::min);
        assert!(d <= f.error, "{p:?} is {d} off");
    }
}

#[test]
fn a_shadow_in_pieces_or_pinched_gets_a_hull_that_covers_it() {
    let mut apart = rect(0.0, 0.0, 10.0, 10.0);
    apart.extend(rect(20.0, 0.0, 30.0, 10.0));
    let f = outline_of(&apart).unwrap();
    assert!(f.hull);
    assert!((area(&f.shape.outer) - 300.0).abs() < 1e-9);
    // Two squares meeting at one corner.
    let mut pinched = rect(0.0, 0.0, 10.0, 10.0);
    pinched.extend(rect(10.0, 10.0, 20.0, 20.0));
    let f = outline_of(&pinched).unwrap();
    assert!(f.hull);
    assert!(area(&f.shape.outer) >= 200.0);
}

#[test]
fn crossing_lines_split_and_stray_lines_are_dropped() {
    // A star of two crossing triangles, and a stray tail sticking out of a square.
    let tri =
        |a: (f64, f64), b: (f64, f64), c: (f64, f64)| vec![line(a, b), line(b, c), line(c, a)];
    let mut c = tri((0.0, 0.0), (20.0, 0.0), (10.0, 17.0));
    c.extend(tri((0.0, 11.0), (20.0, 11.0), (10.0, -6.0)));
    let f = outline_of(&c).unwrap();
    assert!(!f.hull);
    assert_eq!(f.shape.outer.len(), 12, "{:?}", f.shape.outer);
    let mut tail = rect(0.0, 0.0, 10.0, 10.0);
    tail.push(line((5.0, 5.0), (8.0, 8.0)));
    let f = outline_of(&tail).unwrap();
    assert!(!f.hull);
    assert_eq!(f.shape.outer.len(), 4);
}

#[test]
fn a_spike_tip_is_never_tidied_away() {
    // The tip (10, 0) is nearly on the line through its neighbours but far from the segment
    // between them: dropping it would lose the spike.
    let ring = [
        Vec2::new(0.0, 0.0),
        Vec2::new(10.0, 0.0),
        Vec2::new(1.0, 5e-8),
        Vec2::new(0.0, 1.0),
    ];
    assert_eq!(tidy(&ring, 1e-6).len(), 4);
    assert_eq!(thin(&ring, 1e-6).len(), 4);
    // A corner on a straight edge still goes.
    let straight = [
        Vec2::new(0.0, 0.0),
        Vec2::new(5.0, 1e-9),
        Vec2::new(10.0, 0.0),
        Vec2::new(10.0, 10.0),
        Vec2::new(0.0, 10.0),
    ];
    assert_eq!(tidy(&straight, 1e-6).len(), 4);
}

#[test]
fn a_drawing_crossing_itself_everywhere_gets_its_hull() {
    // 400 lines across 400 others: more crossings than an outline is worked out from.
    let mut grid = Vec::new();
    for k in 0..400 {
        let v = f64::from(k) * 0.25;
        grid.push(line((v, -1.0), (v, 100.0)));
        grid.push(line((-1.0, v), (100.0, v)));
    }
    let f = outline_of(&grid).unwrap();
    assert!(f.hull);
    // It covers every line: at least the square they cross in.
    assert!(area(&f.shape.outer) >= 99.75 * 99.75);
}

#[test]
fn hostile_drawings_are_errors_never_panics() {
    assert!(outline_of(&[]).is_err());
    assert!(outline_of(&[line((0.0, 0.0), (0.0, 0.0))]).is_err());
    assert!(outline_of(&[line((f64::NAN, 0.0), (1.0, 1.0))]).is_err());
    // A single line has no area: its hull is two points.
    assert!(outline_of(&[line((0.0, 0.0), (1.0, 1.0))]).is_err());
    let _ = outline_of(&[
        line((-1e12, -1e12), (1e12, 1e12)),
        line((1e12, -1e12), (-1e12, 1e12)),
    ]);
}
