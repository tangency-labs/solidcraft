use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::*;

/// Robustness seed 57's part: a block with an arc side, a ball cut from the edge of its back
/// face, just touching its bottom face.
fn block_with_ball_cut() -> Body {
    let p = Vec2::new;
    let line = |a: Vec2, b: Vec2| Seg2::Line { a, b };
    let (c, r) = (p(24.0625, 11.25), 26.5625);
    let start = (p(0.0, 22.5) - c).angle();
    let sweep = ((p(0.0, 0.0) - c).angle() - start).rem_euclid(std::f64::consts::TAU);
    let segs = vec![
        line(p(0.0, 0.0), p(47.5, 0.0)),
        line(p(47.5, 0.0), p(47.5, 22.5)),
        line(p(47.5, 22.5), p(27.5, 22.5)),
        line(p(27.5, 22.5), p(27.5, 12.5)),
        line(p(27.5, 12.5), p(15.0, 12.5)),
        line(p(15.0, 12.5), p(15.0, 22.5)),
        line(p(15.0, 22.5), p(0.0, 22.5)),
        Seg2::Arc { center: c, radius: r, start, sweep },
    ];
    let block = crate::extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 0.0, 17.5).unwrap().remove(0);
    let ball = crate::sphere(Vec3::new(2.5, 22.5, 5.0), 5.0).unwrap();
    boolean(&block, &ball, BoolOp::Cut).unwrap().unwrap()
}

/// The ball's rim on the back face dips to 0.04 mm above the bottom: a 0.5 mm chamfer of it
/// would run off the back face, and what was built grew the part (by 0.65 mm³, more with other
/// edge orders). On a convex edge a chamfer takes material away, so it is refused.
#[test]
fn a_chamfer_that_would_grow_the_part_on_a_convex_edge_is_refused() {
    let body = block_with_ball_cut();
    let rim = Vec3::new(3.1526309873924174, 22.5, 0.04277569594852188);
    let e = chamfer(&body, &[rim], 0.5).expect_err("refused");
    assert!(matches!(e, KernelError::TooLarge(_)) && e.to_string().contains("convex"), "{e}");
}

/// Curved convex edges that a chamfer does take material from still build: a hole's rim in a
/// plate.
#[test]
fn a_chamfer_of_a_hole_rim_still_builds() {
    let plate = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let hole = crate::cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
    let body = boolean(&plate, &hole, BoolOp::Cut).unwrap().unwrap();
    let v0 = crate::measure(&body).unwrap().volume;
    let out = chamfer(&body, &[Vec3::new(25.0, 15.0, 10.0)], 1.0).unwrap();
    assert!(out.validity().is_empty(), "{:?}", out.validity());
    // A ring of a 1 mm chamfer round a 5 mm hole: (1/2)·2π·(5 + 1/3).
    let taken = v0 - crate::measure(&out).unwrap().volume;
    let want = std::f64::consts::PI * (5.0 + 1.0 / 3.0);
    assert!((taken - want).abs() < 0.02 * want, "{taken} vs {want}");
}

/// Chamfers of groups that lie apart (a pattern's copies), all at once: the same body as
/// chamfering the groups one at a time, by cutters, whole loops or a ball rolled along a chain.
mod apart {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

    use crate::blend::{LOCALS, chamfer, chamfer_apart};
    use crate::{Body, BoolOp, boolean, box_solid, cylinder, measure};

    /// The groups chamfered one at a time.
    fn one_at_a_time(body: &Body, groups: &[Vec<Vec3>], d: f64) -> Body {
        groups.iter().fold(body.clone(), |b, g| chamfer(&b, g, d).unwrap())
    }

    fn assert_same(a: &Body, b: &Body) {
        let (ma, mb) = (measure(a).unwrap(), measure(b).unwrap());
        assert!((ma.volume - mb.volume).abs() <= 1e-9 * ma.volume, "volume {} vs {}", ma.volume, mb.volume);
        assert!((ma.area - mb.area).abs() <= 1e-9 * ma.area, "area {} vs {}", ma.area, mb.area);
        assert_eq!((a.face_count(), a.edge_count()), (b.face_count(), b.edge_count()));
        assert!(a.validity().is_empty(), "{:?}", a.validity());
    }

    /// A rectangle from (x0, y0) to (x1, y1), its corners at x1 rounded to radius r.
    fn rounded(x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> Loop2 {
        use std::f64::consts::FRAC_PI_2;
        let v = Vec2::new;
        let arc = |cx: f64, cy: f64, start: f64| Seg2::Arc { center: v(cx, cy), radius: r, start, sweep: FRAC_PI_2 };
        Loop2 {
            segs: vec![
                Seg2::Line { a: v(x0, y0), b: v(x1 - r, y0) },
                arc(x1 - r, y0 + r, -FRAC_PI_2),
                Seg2::Line { a: v(x1, y0 + r), b: v(x1, y1 - r) },
                arc(x1 - r, y1 - r, 0.0),
                Seg2::Line { a: v(x1 - r, y1), b: v(x0, y1) },
                Seg2::Line { a: v(x0, y1), b: v(x0, y0) },
            ],
        }
    }

    const XS: [f64; 3] = [7.0, 27.0, 47.0];

    /// A 60 × 20 × 2 plate and three 6 × 6 × 3 bosses on it; each group the top edges of a boss.
    #[test]
    fn copies_chamfered_by_cutters_at_once_are_chamfered_as_one_at_a_time() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(60.0, 20.0, 2.0)).unwrap();
        for x in XS {
            b = boolean(&b, &box_solid(Vec3::new(x, 7.0, 2.0), Vec3::new(x + 6.0, 13.0, 5.0)).unwrap(), BoolOp::Union).unwrap().unwrap();
        }
        let groups: Vec<Vec<Vec3>> = XS
            .iter()
            .map(|x| vec![Vec3::new(x + 3.0, 7.0, 5.0), Vec3::new(x + 6.0, 10.0, 5.0), Vec3::new(x + 3.0, 13.0, 5.0), Vec3::new(*x, 10.0, 5.0)])
            .collect();
        assert_same(&chamfer_apart(&b, &groups, 0.5).unwrap(), &one_at_a_time(&b, &groups, 0.5));
    }

    /// Three round bosses: each group the top circle, a whole loop of the boss's top face.
    #[test]
    fn copies_chamfered_as_whole_loops_at_once_are_chamfered_as_one_at_a_time() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(60.0, 20.0, 2.0)).unwrap();
        for x in XS {
            b = boolean(&b, &cylinder(Vec3::new(x + 3.0, 10.0, 2.0), Vec3::Z, 3.0, 3.0).unwrap(), BoolOp::Union).unwrap().unwrap();
        }
        let groups: Vec<Vec<Vec3>> = XS.iter().map(|x| vec![Vec3::new(x + 6.0, 10.0, 5.0)]).collect();
        LOCALS.with(|n| n.set((0, 0)));
        let at_once = chamfer_apart(&b, &groups, 0.5).unwrap();
        assert_eq!(LOCALS.with(|n| n.get()), (3, 0), "three loops");
        assert_same(&at_once, &one_at_a_time(&b, &groups, 0.5));
    }

    /// Three pockets with two rounded corners; each group one straight edge of a pocket's floor,
    /// chamfered along its smooth chain round the rounded corners (not a whole loop).
    #[test]
    fn copies_chamfered_along_their_chains_at_once_are_chamfered_as_one_at_a_time() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(100.0, 40.0, 10.0)).unwrap();
        let floor = Plane::XY;
        for x in [5.0, 37.0, 69.0] {
            let region = Region2 { outer: rounded(x, 5.0, x + 26.0, 35.0, 4.0), holes: vec![] };
            let pocket = crate::build::extrude(&floor, &[region], 5.0, 11.0).unwrap().pop().unwrap();
            b = boolean(&b, &pocket, BoolOp::Cut).unwrap().unwrap();
        }
        let groups: Vec<Vec<Vec3>> = [5.0, 37.0, 69.0].iter().map(|x| vec![Vec3::new(x + 13.0, 5.0, 5.0)]).collect();
        LOCALS.with(|n| n.set((0, 0)));
        let at_once = chamfer_apart(&b, &groups, 1.0).unwrap();
        assert_eq!(LOCALS.with(|n| n.get()), (0, 3), "three chains");
        assert_same(&at_once, &one_at_a_time(&b, &groups, 1.0));
    }

    /// Groups whose chamfers meet are not chamfered at once.
    #[test]
    fn copies_whose_chamfers_meet_are_refused() {
        let b = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let edge = vec![Vec3::new(10.0, 0.0, 20.0)];
        assert!(chamfer_apart(&b, &[edge.clone(), edge], 1.0).is_err());
        assert!(chamfer_apart(&b, &[vec![Vec3::new(10.0, 0.0, 20.0)], vec![Vec3::new(20.0, 10.0, 20.0)]], 1.0).is_err(), "edges meeting at a corner");
    }
}
