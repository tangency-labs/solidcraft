use super::*;

fn line(a: (f64, f64), b: (f64, f64)) -> Seg2 {
    Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Seg2> {
    vec![line((x0, y0), (x1, y0)), line((x1, y0), (x1, y1)), line((x1, y1), (x0, y1)), line((x0, y1), (x0, y0))]
}

fn area(r: &[Vec2]) -> f64 {
    (0..r.len()).map(|k| cross(Vec2::ZERO, r[k], r[(k + 1) % r.len()])).sum::<f64>() / 2.0
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
    assert!((area(&f.shape.outer) - (300.0 + 200.0)).abs() < 1e-9, "{:?}", f.shape.outer);
    assert_eq!(f.shape.outer.len(), 6);
}

#[test]
fn arcs_are_flattened_within_the_reported_error() {
    // A slot: two half circles joined by lines.
    let c = vec![
        line((0.0, 0.0), (50.0, 0.0)),
        Seg2::Arc { center: Vec2::new(50.0, 10.0), radius: 10.0, start: -std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
        line((50.0, 20.0), (0.0, 20.0)),
        Seg2::Arc { center: Vec2::new(0.0, 10.0), radius: 10.0, start: std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
    ];
    let f = outline_of(&c).unwrap();
    let want = 50.0 * 20.0 + std::f64::consts::PI * 100.0;
    let got = area(&f.shape.outer);
    assert!(got <= want + 1e-9 && got > want - f.error * 2.0 * (50.0 + 20.0 * std::f64::consts::PI), "{got} {want} {}", f.error);
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
    let tri = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| vec![line(a, b), line(b, c), line(c, a)];
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
fn hostile_drawings_are_errors_never_panics() {
    assert!(outline_of(&[]).is_err());
    assert!(outline_of(&[line((0.0, 0.0), (0.0, 0.0))]).is_err());
    assert!(outline_of(&[line((f64::NAN, 0.0), (1.0, 1.0))]).is_err());
    // A single line has no area: its hull is two points.
    assert!(outline_of(&[line((0.0, 0.0), (1.0, 1.0))]).is_err());
    let _ = outline_of(&[line((-1e12, -1e12), (1e12, 1e12)), line((1e12, -1e12), (-1e12, 1e12))]);
}
