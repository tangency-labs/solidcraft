//! How long a body takes to cross a kernel boundary: through STEP text (solidcore's writer, then
//! its reader, as an adapter without the exact exchange takes a body across) against in memory as
//! an exact body (`solidcraft_kernel::exact`). Each case is a threaded shaft as the thread
//! feature makes one, or a plain body; each crossing runs `RUNS` times and the best is printed.
//!
//! `cargo run --release -p solidcraft-kernel --example exact_exchange [major pitch turns]...`
//! (default: M20 x 2.5 over 40 mm, M30 x 1.5 over 100 mm).

use std::time::Instant;

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_kernel::{Body, BoolOp, exact, measure_mass, step_export, step_import};

const RUNS: usize = 3;

fn thread(major: f64, pitch: f64, turns: f64) -> Body {
    let h = pitch * 3f64.sqrt() / 2.0;
    let (rc, rm) = (major / 2.0 - 5.0 * h / 8.0, major / 2.0);
    let (foot, crest) = ((pitch - pitch / 4.0) / 2.0, pitch / 16.0);
    let z0 = pitch;
    let len = z0 + pitch * turns + pitch;
    let half = |r: f64| foot + (crest - foot) * (r - rc) / (rm - rc);
    let r0 = rc - 0.1 * pitch;
    let section = Loop2::polygon(&[Vec2::new(r0, -half(r0)), Vec2::new(rm, -crest), Vec2::new(rm, crest), Vec2::new(r0, half(r0))]);
    let plane = Plane { origin: Vec3::new(0.0, 0.0, z0), x: Vec3::X, y: Vec3::Z };
    let ridge =
        solidcraft_kernel::sweep_helix(&plane, &Region2 { outer: section, holes: Vec::new() }, Vec3::ZERO, Vec3::Z, pitch, turns).expect("the ridge");
    let core = Region2 { outer: Loop2::circle(Vec2::ZERO, rc), holes: Vec::new() };
    let core = solidcraft_kernel::extrude(&Plane::XY, &[core], 0.0, len).expect("the core").pop().expect("one core");
    solidcraft_kernel::boolean(&core, &ridge, BoolOp::Union).expect("the join").expect("a shaft")
}

/// The best of `RUNS` runs of `f`, in seconds, and its last answer.
fn best<T>(mut f: impl FnMut() -> T) -> (f64, T) {
    let mut out = None;
    let mut t = f64::INFINITY;
    for _ in 0..RUNS {
        let start = Instant::now();
        out = Some(f());
        t = t.min(start.elapsed().as_secs_f64());
    }
    (t, out.expect("a run"))
}

fn cross(what: &str, body: &Body) {
    let volume = measure_mass(body).map(|m| m.volume).unwrap_or(f64::NAN);
    let (write, text) = best(|| step_export(&[body], "exchange timing"));
    let step = match text {
        Ok(text) => {
            let (read, back) = best(|| step_import(&text));
            let back = back.map_err(|e| e.to_string()).and_then(|i| i.bodies.into_iter().next().map(|b| b.body).ok_or_else(|| "no body".to_string()));
            let v = back.as_ref().ok().and_then(|b| measure_mass(b).ok()).map_or(f64::NAN, |m| m.volume);
            Some((write, read, back.err(), v))
        }
        Err(e) => {
            println!("{what}: STEP export refused: {e}");
            None
        }
    };
    let (out, ex) = best(|| exact::exact_exported(body));
    // No operands here to check a history against.
    let ex = solidcraft_kernel_api::ExactExported { history: None, ..ex.expect("the exact body") };
    let (assembly, _) = best(|| exact::assemble(&ex.body));
    // The same body from a producer that holds no pcurves: every one made.
    let mut bare = ex.body.clone();
    for f in &mut bare.faces {
        for b in &mut f.bounds {
            if let solidcraft_kernel_api::ExactBound::Edges { uses } = b {
                for u in uses {
                    u.pcurve = None;
                }
            }
        }
    }
    let (bare_assembly, bare_back) = best(|| exact::assemble(&bare));
    let (inn, back) = best(|| exact::body_from_exact(ex.clone(), &[]));
    let v = back.as_ref().ok().and_then(|b| measure_mass(b).ok()).map_or(f64::NAN, |m| m.volume);
    let faces = body.face_count();
    println!("{what}: {faces} faces, volume {volume:.6}");
    if let Some((w, r, err, sv)) = step {
        println!(
            "  STEP:  write {w:.3} s + read {r:.3} s = {:.3} s, volume {sv:.6}{}",
            w + r,
            err.map(|e| format!(" (read refused: {e})")).unwrap_or_default()
        );
    }
    println!(
        "  exact: out {out:.3} s + in {inn:.3} s (assembly {assembly:.3} s) = {:.3} s, volume {v:.6}{}",
        out + inn,
        back.err().map(|e| format!(" (refused: {e})")).unwrap_or_default()
    );
    println!("  exact, no pcurves given: assembly {bare_assembly:.3} s{}", bare_back.err().map(|e| format!(" (refused: {e})")).unwrap_or_default());
}

fn main() {
    let args: Vec<f64> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let cases: Vec<(f64, f64, f64)> = if args.len() >= 3 {
        args.as_chunks::<3>().0.iter().map(|&[a, b, c]| (a, b, c)).collect()
    } else {
        // M20 x 2.5 over 40 mm and M30 x 1.5 over 100 mm, less a pitch at each end.
        vec![(20.0, 2.5, 14.0), (30.0, 1.5, 64.0)]
    };
    let block = solidcraft_kernel::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 4.0)).expect("a box");
    let hole = solidcraft_kernel::cylinder(Vec3::new(5.0, 5.0, -1.0), Vec3::Z, 2.0, 6.0).expect("a rod");
    let drilled = solidcraft_kernel::boolean(&block, &hole, BoolOp::Cut).expect("the cut").expect("a body");
    cross("a box with a hole", &drilled);
    for (major, pitch, turns) in cases {
        let start = Instant::now();
        let body = thread(major, pitch, turns);
        println!("(M{major} x {pitch}, {turns} turns built in {:.1} s)", start.elapsed().as_secs_f64());
        cross(&format!("M{major} x {pitch}, {turns} turns"), &body);
    }
}
