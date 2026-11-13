use super::*;

/// A sphere of radius `r` as a scan reads it: `n` × 2`n` quads, each triangle's corners its own
/// points (the same position repeated where triangles meet).
fn sphere(r: f64, n: u32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let at = |i: u32, j: u32| {
        let (t, p) = (std::f64::consts::PI * f64::from(i) / f64::from(n), std::f64::consts::TAU * f64::from(j) / f64::from(2 * n));
        let (st, ct, sp, cp) = (t.sin(), t.cos(), p.sin(), p.cos());
        // The poles exactly, and the seam the same as the start, so corners weld.
        let (st, sp, cp) = (if i == 0 || i == n { 0.0 } else { st }, if j == 2 * n { 0.0 } else { sp }, if j == 2 * n { 1.0 } else { cp });
        Vec3::new(r * st * cp, r * st * sp, r * ct)
    };
    let (mut pos, mut tris) = (Vec::new(), Vec::new());
    let mut tri = |a: Vec3, b: Vec3, c: Vec3| {
        let k = pos.len() as u32;
        pos.extend([a, b, c]);
        tris.push([k, k + 1, k + 2]);
    };
    for i in 0..n {
        for j in 0..2 * n {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            if i > 0 {
                tri(a, c, d);
            }
            if i + 1 < n {
                tri(a, b, c);
            }
        }
    }
    (pos, tris)
}

fn volume(l: &Level) -> f64 {
    l.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| l.positions[i as usize]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

/// Each side of a closed surface is shared by exactly two triangles, once each way.
fn closed(l: &Level) -> bool {
    let mut sides: Vec<(u32, u32)> = l.triangles.iter().flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]).collect();
    sides.sort_unstable();
    let n = sides.len();
    sides.dedup();
    sides.len() == n && sides.iter().all(|(a, b)| sides.binary_search(&(*b, *a)).is_ok())
}

/// A finely meshed sphere comes down to each target, closed, with its volume and shape kept
/// within the error each level reports, and the errors grow as the levels coarsen.
#[test]
fn a_sphere_comes_down_to_each_target_within_its_error() {
    let (pos, tris) = sphere(50.0, 100);
    let full = tris.len();
    let made = levels(&pos, &tris, &[full / 4, full / 16, full / 64]);
    assert_eq!(made.len(), 3);
    let exact = 4.0 / 3.0 * std::f64::consts::PI * 50.0f64.powi(3);
    let mut last = 0.0;
    for (l, target) in made.iter().zip([full / 4, full / 16, full / 64]) {
        assert!(l.triangles.len() <= target, "{} > {target}", l.triangles.len());
        assert!(closed(l), "closed and consistently wound");
        assert!(l.error >= last, "the error grows: {} after {last}", l.error);
        last = l.error;
        // Each point within the error of the sphere (its faces' planes lie within their sag of it).
        let sag = 50.0 * (1.0 - (std::f64::consts::PI / 100.0).cos());
        for p in &l.positions {
            assert!((p.len() - 50.0).abs() <= l.error + sag + 1e-9, "{} from the centre, error {}", p.len(), l.error);
        }
        assert!((volume(l) - exact).abs() < exact * 0.02, "volume {} vs {exact}", volume(l));
        // A 600-triangle sphere of radius 50 bows in by about 0.4 at its triangles' centres at
        // best; finer levels a fraction of that.
        assert!(l.error < 1.0, "error {}", l.error);
    }
    // The same mesh simplifies the same way.
    let again = levels(&pos, &tris, &[full / 4, full / 16, full / 64]);
    assert!(made.iter().zip(&again).all(|(a, b)| a.triangles == b.triangles && a.positions == b.positions));
}

/// A flat sheet comes down to a few triangles with no error, and its open outline stays put.
#[test]
fn a_flat_sheet_keeps_its_outline() {
    let n = 40u32;
    let at = |i: u32, j: u32| Vec3::new(f64::from(i), f64::from(j) * 0.5, 0.0);
    let (mut pos, mut tris) = (Vec::new(), Vec::new());
    for i in 0..n {
        for j in 0..n {
            for t in [[at(i, j), at(i + 1, j), at(i + 1, j + 1)], [at(i, j), at(i + 1, j + 1), at(i, j + 1)]] {
                let k = pos.len() as u32;
                pos.extend(t);
                tris.push([k, k + 1, k + 2]);
            }
        }
    }
    let made = levels(&pos, &tris, &[tris.len() / 16]);
    let l = made.first().unwrap();
    assert!(l.triangles.len() <= tris.len() / 16);
    assert!(l.error < 1e-6, "flat: {}", l.error);
    assert!(l.positions.iter().all(|p| p.z.abs() < 1e-9));
    let b = l.positions.iter().fold(crate::Aabb3::EMPTY, |mut b, p| {
        b.add(*p);
        b
    });
    assert!(b.min.dist(Vec3::ZERO) < 1e-9 && b.max.dist(Vec3::new(40.0, 20.0, 0.0)) < 1e-9, "{b:?}");
    let area: f64 = l
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| l.positions[i as usize]);
            (b - a).cross(c - a).len() / 2.0
        })
        .sum();
    assert!((area - 800.0).abs() < 1e-6, "the sheet is still whole: {area}");
}

/// Input from a file is hostile: points that are not finite, indices out of range, triangles
/// with a repeated corner and empty meshes are left out, never a panic.
#[test]
fn hostile_meshes_never_panic() {
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    assert!(levels(&[], &[], &[1]).is_empty());
    assert!(levels(&[p(0.0, 0.0)], &[[0, 0, 0]], &[1]).is_empty());
    assert!(levels(&[p(f64::NAN, 0.0), p(1.0, 0.0), p(0.0, 1.0)], &[[0, 1, 2]], &[0]).is_empty());
    assert!(levels(&[p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0)], &[[0, 1, 99]], &[0]).is_empty());
    let (pos, mut tris) = sphere(5.0, 12);
    tris.push([0, 1, u32::MAX]);
    tris.push([2, 2, 3]);
    let made = levels(&pos, &tris, &[usize::MAX, 100, 0]);
    assert!(made.iter().all(|l| l.triangles.iter().flatten().all(|i| (*i as usize) < l.positions.len())));
}

/// Timing, not a check (`SOLIDCRAFT_BENCH_STL=part.stl cargo test --release -p solidcraft-geom
/// --lib simplify_timing -- --ignored --nocapture`): the levels of a binary STL.
#[test]
#[ignore]
fn simplify_timing() {
    let Some(path) = std::env::var_os("SOLIDCRAFT_BENCH_STL") else { return };
    let bytes = std::fs::read(path).unwrap();
    let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    let f = |o: usize| f64::from(f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()));
    let (mut pos, mut tris) = (Vec::with_capacity(n * 3), Vec::with_capacity(n));
    for t in 0..n {
        let o = 84 + t * 50 + 12;
        let k = pos.len() as u32;
        for v in 0..3 {
            pos.push(Vec3::new(f(o + v * 12), f(o + v * 12 + 4), f(o + v * 12 + 8)));
        }
        tris.push([k, k + 1, k + 2]);
    }
    let started = std::time::Instant::now();
    let made = levels(&pos, &tris, &[n / 4, n / 16, n / 64]);
    eprintln!("{n} triangles: levels in {:.2} s", started.elapsed().as_secs_f64());
    for l in &made {
        eprintln!("  {} triangles, error {:.3} mm", l.triangles.len(), l.error);
    }
}
