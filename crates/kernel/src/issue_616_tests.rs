//! Issue #616: small boxes joined flush to the walls and floor of a part never change it away
//! from the box, and a retry that would is refused.

use solidcraft_geom::{Aabb3, Mesh, Vec3};

use crate::*;

fn bx(c: [f64; 3], s: [f64; 3]) -> Body {
    box_solid(Vec3::new(c[0], c[1], c[2]), Vec3::new(c[0] + s[0], c[1] + s[1], c[2] + s[2])).unwrap()
}

fn volume(b: &Body) -> f64 {
    measure(b).unwrap().volume
}

/// The lid of the issue (public dimensions): a plate with rounded corners, a skirt ring on
/// it, a window and a slot cut through, and a notch cut in the skirt.
fn lid() -> Body {
    let plate = bx([100., 0., 0.], [76.8, 89.3, 2.4]);
    let corners: Vec<Vec3> = [[100., 0.], [176.8, 0.], [100., 89.3], [176.8, 89.3]].iter().map(|p| Vec3::new(p[0], p[1], 1.2)).collect();
    let plate = fillet(&plate, &corners, 3.0).unwrap();
    let ring = boolean(&bx([102.65, 2.65, 2.4], [71.5, 84., 3.]), &bx([104.15, 4.15, 2.0], [68.5, 81., 4.]), BoolOp::Cut).unwrap().unwrap();
    let mut b = boolean(&plate, &ring, BoolOp::Union).unwrap().unwrap();
    for (c, s) in [([104.4, 5.4, -1.], [68., 61., 8.]), ([116.4, 72., -1.], [44., 10., 8.]), ([127.4, 84., 2.4], [22., 6., 4.])] {
        b = boolean(&b, &bx(c, s), BoolOp::Cut).unwrap().unwrap();
    }
    b
}

/// Distance from `p` to the triangle `abc` (brute force, independent of the guard's grid).
fn tri_dist(p: Vec3, [a, b, c]: [Vec3; 3]) -> f64 {
    let n = (b - a).cross(c - a);
    let Some(u) = n.normalized() else { return f64::INFINITY };
    let h = (p - a).dot(u);
    let q = p - u * h;
    if [(a, b), (b, c), (c, a)].iter().all(|(x, y)| (*y - *x).cross(q - *x).dot(n) >= 0.0) {
        h.abs()
    } else {
        [(a, b), (b, c), (c, a)].iter().map(|(x, y)| p.dist_to_segment(*x, *y)).fold(f64::INFINITY, f64::min)
    }
}

/// A point of `from`'s surface outside `region` that is not on `onto`'s surface.
fn moved_outside(from: &Mesh, onto: &Mesh, region: &Aabb3, tol: f64) -> Option<Vec3> {
    let tris: Vec<[Vec3; 3]> = onto.triangles.iter().filter_map(|t| onto.tri(t)).collect();
    from.triangles.iter().filter_map(|t| from.tri(t)).find_map(|[a, b, c]| {
        let p = (a + b + c) / 3.0;
        let inside = p.x > region.min.x && p.y > region.min.y && p.z > region.min.z && p.x < region.max.x && p.y < region.max.y && p.z < region.max.z;
        (!inside && !tris.iter().any(|t| tri_dist(p, *t) < tol)).then_some(p)
    })
}

/// The join of `tool` onto `target` left `target` as it was away from the tool: no new surface
/// (a slit, a moved face) and none lost, outside the tool's box.
fn assert_local(target: &Body, tool: &Body, joined: &Body, what: &str) {
    let tol = 1e-3;
    let (mt, mtool, mj) = (
        target.mesh(&crate::MeshTolerance::chord(0.01)).unwrap(),
        tool.mesh(&crate::MeshTolerance::chord(0.01)).unwrap(),
        joined.mesh(&crate::MeshTolerance::chord(0.01)).unwrap(),
    );
    let mut region = mtool.bounds();
    region.min -= Vec3::new(0.01, 0.01, 0.01);
    region.max += Vec3::new(0.01, 0.01, 0.01);
    assert_eq!(moved_outside(&mj, &mt, &region, tol), None, "{what}: new surface away from the tool");
    assert_eq!(moved_outside(&mt, &mj, &region, tol), None, "{what}: surface lost away from the tool");
}

/// The four ribs of the issue, 1.6 × 6 × 11.7, sit on the plate (z = 2.4), over the skirt, flush
/// with the window's sides. Each join adds exactly what lies outside the skirt, changes nothing
/// else (it used to leave a slit through the far wall).
#[test]
fn ribs_joined_flush_to_the_lid_change_nothing_else() {
    let mut b = lid();
    assert!((volume(&b) - 6703.5222).abs() < 1e-3, "{}", volume(&b));
    for c in [[102.8, 14., 2.4], [102.8, 48., 2.4], [172.4, 14., 2.4], [172.4, 48., 2.4]] {
        let tool = bx(c, [1.6, 6., 11.7]);
        let r = boolean(&b, &tool, BoolOp::Union).unwrap().unwrap();
        // 1.6 × 6 × 11.7 less the 1.35 × 6 × 3 inside the skirt.
        assert!((volume(&r) - volume(&b) - 88.02).abs() < 1e-6, "{c:?}: {}", volume(&r) - volume(&b));
        assert_eq!(r.shell_count(), 1);
        assert_local(&b, &tool, &r, &format!("{c:?}"));
        b = r;
    }
    let bounds = b.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().bounds();
    assert!((bounds.max.z - 14.1).abs() < 1e-9, "the ribs keep their height: {}", bounds.max.z);
}

/// Random small boxes joined flush to the walls and floor of a shelled box with rounded corners
/// (sitting on the floor, against a wall, flush with a wall's top or into its corner), and of
/// the same box with a window through its floor (on the ledge beside the window, flush with its
/// side, as the ribs of #616), never lose volume and never change the part away from the box. A
/// join that fails is an error, never a wrong result.
#[test]
fn boxes_joined_flush_to_a_shelled_box_stay_local() {
    let (w, d, h, t) = (60.0, 40.0, 20.0, 2.0);
    let outer = bx([0., 0., 0.], [w, d, h]);
    let corners: Vec<Vec3> = [[0., 0.], [w, 0.], [0., d], [w, d]].iter().map(|p| Vec3::new(p[0], p[1], h / 2.0)).collect();
    let outer = fillet(&outer, &corners, 3.0).unwrap();
    let shelled = boolean(&outer, &bx([t, t, t], [w - 2. * t, d - 2. * t, h]), BoolOp::Cut).unwrap().unwrap();
    // The window leaves a 1.5 ledge inside the walls.
    let (ledge, gap) = (t + 1.5, 6.0);
    let framed = boolean(&shelled, &bx([ledge, gap, -1.], [w - 2. * ledge, d - 2. * gap, t + 2.]), BoolOp::Cut).unwrap().unwrap();
    let mut seed = 0x616_u64;
    let mut next = |lo: f64, hi: f64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        lo + (hi - lo) * ((seed >> 11) as f64 / (1u64 << 53) as f64)
    };
    // Sizes and places on a 0.1 grid, like typed values.
    let q = |x: f64| (x * 10.0).round() / 10.0;
    let (mut joined, mut refused) = (0, 0);
    for (name, target) in [("shelled", &shelled), ("framed", &framed)] {
        let base = volume(target);
        for case in 0..20 {
            let s = [q(next(1.0, 6.0)), q(next(1.0, 6.0)), q(next(1.0, 12.0))];
            let (x, y) = (q(next(t + 1.0, w - t - 8.0)), q(next(gap, d - gap - 8.0)));
            let c = match (name, case % 4) {
                // On the floor.
                ("shelled", 0) => [x, y, t],
                // On the floor against the -x wall.
                ("shelled", 1) => [t, y, t],
                // Into the +y wall, flush with its top, overlapping it.
                ("shelled", 2) => [x, d - t - s[1] / 2.0, h - s[2]],
                // On the floor in the corner of the -x and -y walls, sunk into both.
                ("shelled", _) => [t - 0.5, t - 0.5, t],
                // On the ledge over the wall, flush with the window's -x or +x side.
                (_, 0) => [ledge - 1.6, y, t],
                (_, 1) => [w - ledge, y, t],
                // On the ledge against the -x wall, short of the window.
                (_, 2) => [t, y, t],
                // Into the +x wall from the ledge, flush with the window's side.
                _ => [w - ledge, y, t],
            };
            let s = if name == "framed" && case % 4 != 2 { [1.6, s[1], s[2]] } else { s };
            let tool = bx(c, s);
            let what = format!("{name} case {case}: corner {c:?} size {s:?}");
            match boolean(target, &tool, BoolOp::Union) {
                Ok(Some(r)) => {
                    assert!(volume(&r) >= base - 1e-6, "{what}: the join lost volume ({} < {base})", volume(&r));
                    assert!(volume(&r) <= base + volume(&tool) + 1e-6, "{what}");
                    assert_local(target, &tool, &r, &what);
                    joined += 1;
                }
                Ok(None) => panic!("{what}: a join came out empty"),
                Err(e) => {
                    eprintln!("{what}: refused: {e}");
                    refused += 1;
                }
            }
        }
    }
    assert_eq!((joined, refused), (40, 0), "every one of these joins works");
}
