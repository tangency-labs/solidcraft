//! Timings of the window's math: `plan()` and `check_mesh()` for each kind's defaults and for
//! big tooth counts. `cargo run --release -p solidcraft-gear --example gear_bench`.

use std::hint::black_box;
use std::time::Instant;

use solidcraft_gear::{Kind, Spec, check_mesh, plan};

const KINDS: &[Kind] = &[
    Kind::Spur,
    Kind::Helical,
    Kind::Herringbone,
    Kind::Internal,
    Kind::Rack,
    Kind::Pair,
    Kind::Planetary,
    Kind::Bevel,
    Kind::SpiralBevel,
    Kind::Hypoid,
    Kind::CrossedHelical,
    Kind::Worm,
    Kind::Cycloidal,
    Kind::CycloidalDrive,
    Kind::Sprocket,
    Kind::Pulley,
    Kind::Spline,
    Kind::FaceGear,
    Kind::Elliptical,
];

/// Fastest and median of `n` runs, in ms (the first run is also reported: a cold cache).
fn time<T>(n: usize, mut f: impl FnMut() -> T) -> (f64, f64, f64) {
    let mut v: Vec<f64> = (0..n)
        .map(|_| {
            let t = Instant::now();
            black_box(f());
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    let first = v[0];
    v.sort_by(f64::total_cmp);
    (v[0], v[n / 2], first)
}

fn main() {
    let only: Option<String> = std::env::args().nth(1);
    println!(
        "{:<16} {:>9} {:>9} {:>9} | {:>9} {:>9} {:>9}  verdicts",
        "kind", "plan min", "plan med", "plan 1st", "mesh min", "mesh med", "mesh 1st"
    );
    for kind in KINDS.iter().copied() {
        let name = format!("{kind:?}");
        if only.as_deref().is_some_and(|o| !name.eq_ignore_ascii_case(o)) {
            continue;
        }
        let s = Spec::defaults_for(kind);
        let p = match plan(&s) {
            Ok(p) => p,
            Err(e) => {
                println!("{name:<16} refused: {e}");
                continue;
            }
        };
        let (pmin, pmed, p1) = time(9, || plan(&s));
        let (mmin, mmed, m1) = time(5, || check_mesh(&p));
        let verdicts: Vec<String> =
            check_mesh(&p).iter().map(|r| format!("{:?} {:.6}/{:.6}/{:.6}", r.verdict, r.penetration_mm, r.gap_min_mm, r.gap_max_mm)).collect();
        println!("{name:<16} {pmin:9.2} {pmed:9.2} {p1:9.2} | {mmin:9.2} {mmed:9.2} {m1:9.2}  {}", verdicts.join(","));
    }
    // Where a 3D check's time goes: the solids, the two surfaces, the steps.
    if std::env::var("BREAKDOWN").is_ok() {
        use solidcraft_gear::mesh2d::{Member, Motion, probe};
        for (label, mut s) in [("planetary", Spec::defaults_for(Kind::Planetary)), ("planetary z400", Spec::defaults_for(Kind::Planetary))] {
            if label.ends_with("z400") {
                (s.teeth, s.teeth2, s.teeth3) = (100, 150, 400);
            }
            let p = plan(&s).unwrap();
            let t = Instant::now();
            let members: Vec<Member> = p
                .preview
                .iter()
                .map(|g| {
                    let surrounds = p.meshes.iter().any(|q| q.internal && q.b == p.preview.iter().position(|x| std::ptr::eq(x, g)).unwrap());
                    let region =
                        if surrounds { g.region.clone() } else { solidcraft_gear::Region { outer: g.region.outer.clone(), holes: Vec::new() } };
                    Member::build(&region, Motion::of(g), 0.25 * s.module, 1e-4 * s.module, 8)
                })
                .collect();
            let t_build = t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            for m in &p.meshes {
                for i in 0..12 {
                    black_box(probe(&members[m.a], &members[m.b], m.period * (i as f64 / 12.0 - 0.5)));
                }
            }
            println!("  2D {label}: members {t_build:.1} ms, probes {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        }
        use solidcraft_gear::mesh3d::{Solid, Surface, mesh_stat};
        for kind in [Kind::Bevel, Kind::SpiralBevel, Kind::Hypoid, Kind::CrossedHelical, Kind::Worm] {
            let s = Spec::defaults_for(kind);
            let p = plan(&s).unwrap();
            let m = &p.meshes3d[0];
            let (ba, bb) = (&p.bodies[0], &p.bodies[1]);
            let (h, hz) = (m.scale / 10.0, m.scale / 3.0);
            let t = Instant::now();
            let (mut a, mut b) = (Solid::new(ba, h), Solid::new(bb, h));
            let t_solid = t.elapsed().as_secs_f64() * 1e3;
            let (t0, t1) = (-0.5 * m.period, 0.5 * m.period);
            let (b0, b1) = (m.ratio * t0, m.ratio * t1);
            let t = Instant::now();
            let sa = Surface::of(&a, &b, hz, Some((t0.min(t1), t0.max(t1))));
            let t_sa = t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            let sb = Surface::of(&b, &a, hz, Some((b0.min(b1), b0.max(b1))));
            let t_sb = t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            for i in 0..solidcraft_gear::mesh3d::CHECK_STEPS {
                let tt = t0 + (t1 - t0) * i as f64 / solidcraft_gear::mesh3d::CHECK_STEPS as f64;
                a.spin = tt;
                b.spin = m.ratio * tt;
                black_box(mesh_stat(&a, &sa, &b, &sb));
            }
            let t_run = t.elapsed().as_secs_f64() * 1e3;
            println!(
                "  3D {kind:?}: solids {t_solid:.1} ms, surface a {t_sa:.1} ms ({} samples), surface b {t_sb:.1} ms ({} samples), {} steps {t_run:.1} ms",
                sa.samples.len(),
                sb.samples.len(),
                solidcraft_gear::mesh3d::CHECK_STEPS
            );
        }
    }
    // Big counts: a z400 gear of each planar kind.
    for kind in [Kind::Spur, Kind::Helical, Kind::Herringbone, Kind::Internal, Kind::Pair, Kind::Planetary] {
        let name = format!("{kind:?} z400");
        if only.as_deref().is_some_and(|o| !format!("{kind:?}").eq_ignore_ascii_case(o)) {
            continue;
        }
        let mut s = Spec::defaults_for(kind);
        s.teeth = 400;
        if kind == Kind::Internal {
            s.teeth2 = 380;
        }
        if kind == Kind::Planetary {
            (s.teeth, s.teeth2, s.teeth3) = (100, 150, 400);
        }
        let Ok(p) = plan(&s) else {
            println!("{name:<16} refused");
            continue;
        };
        let (pmin, pmed, p1) = time(5, || plan(&s));
        let (mmin, mmed, m1) = time(3, || check_mesh(&p));
        let verdicts: Vec<String> =
            check_mesh(&p).iter().map(|r| format!("{:?} {:.6}/{:.6}/{:.6}", r.verdict, r.penetration_mm, r.gap_min_mm, r.gap_max_mm)).collect();
        println!("{name:<16} {pmin:9.2} {pmed:9.2} {p1:9.2} | {mmin:9.2} {mmed:9.2} {m1:9.2}  {}", verdicts.join(","));
    }
}
