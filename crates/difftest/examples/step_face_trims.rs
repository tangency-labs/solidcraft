//! Diagnostics: the UV trim curves of one STEP face against its edges, for the face
//! built alone by the stitch recipe and in the `SmBrepConstructor` B-rep (Rust port).
//! Each edge use's trim (made by the area integration when missing) is evaluated at
//! the ends of its natural interval, lifted to 3D on the face surface, and compared
//! with the edge curve at the edge use's start and end (the interval's min and max for
//! a use with the curve, max and min against it).
//!
//! `cargo run --profile difftest -p difftest --example step_face_trims -- <file.stp> <body> <face>`
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::face_area;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::{Brep, FaceId, LoopuseStart};

fn report(tag: &str, k: &Kernel, b: &mut Brep, f: FaceId) {
    let area = face_area(k, b, f, 1e-6);
    println!("{tag}: face area {area:?}");
    let Ok(face) = b.face(f) else { return };
    let Some(srf) = face.surface.clone() else {
        return;
    };
    let Some(fu) = face.faceuse else { return };
    let loopuses = b
        .faceuse(fu)
        .map(|r| r.loopuses.clone())
        .unwrap_or_default();
    for lu in loopuses {
        let Ok(LoopuseStart::Edgeuse(Some(start))) = b.loopuse(lu).map(|r| r.start) else {
            continue;
        };
        let mut x = start;
        for _ in 0..10_000 {
            let Ok(eu) = b.edgeuse(x) else { break };
            let mate = solidcore::topology::edgeuse::p1::mate(b, x).ok().flatten();
            // the trim of the pair (kept on the use that runs with the curve)
            let trim = eu
                .uv_trim_curve
                .clone()
                .or_else(|| mate.and_then(|m| b.edgeuse(m).ok()?.uv_trim_curve.clone()));
            if let (Some(e), Some(trim)) = (eu.edge.and_then(|e| b.edge(e).ok()), trim) {
                if let Some(c) = &e.curve {
                    let ivl = e.interval;
                    let (s, t) = if eu.orientation == OrientType::Opposite {
                        (ivl.max(), ivl.min())
                    } else {
                        (ivl.min(), ivl.max())
                    };
                    let n = trim.natural_interval();
                    let lift = |w: f64| -> Option<solidcore::math::Point3> {
                        let p = trim.evaluate_point(k, w).ok()?;
                        srf.evaluate_point(k, Point2::new(p.x, p.y)).ok()
                    };
                    let (e0, e1) = (
                        c.evaluate_point(k, ivl.min()).ok(),
                        c.evaluate_point(k, ivl.max()).ok(),
                    );
                    let (t0, t1) = (lift(n.min()), lift(n.max()));
                    let d = |a: Option<solidcore::math::Point3>,
                             b: Option<solidcore::math::Point3>| {
                        a.zip(b).map(|(a, b)| a.distance(b))
                    };
                    println!(
                        "  eu {:?} {:?} edge ivl [{:.6}, {:.6}] trim ivl [{:.6}, {:.6}] |trim(min)-edge(min)|={:?} |trim(max)-edge(max)|={:?} |trim(min)-edge(max)|={:?} (use runs {s:.6}->{t:.6})",
                        x, eu.orientation, ivl.min(), ivl.max(), n.min(), n.max(), d(t0, e0), d(t1, e1), d(t0, e1)
                    );
                }
            }
            match eu.ccw {
                Some(next) if next != start => x = next,
                _ => break,
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parse = |i: usize| args.get(i).and_then(|x| x.parse::<usize>().ok());
    let (Some(file), Some(bi), Some(fi)) = (args.first(), parse(1), parse(2)) else {
        eprintln!("usage: step_face_trims <file.stp> <body> <face>");
        std::process::exit(2);
    };
    let Some(body) = std::fs::read(file)
        .ok()
        .and_then(|b| step_import::import(&b, &step_import::Limits::default()).ok())
        .and_then(|m| m.bodies.into_iter().nth(bi))
    else {
        eprintln!("no body {bi}");
        std::process::exit(2);
    };
    if let Some((k, mut b, faces)) = difftest::step::build_face_alone(&body, fi, false) {
        for f in faces {
            report("stitch recipe (alone)", &k, &mut b, f);
        }
    }
    let script =
        step_import::script::from_body(&body, step_import::script::FromBodyOptions::default());
    let (k, b, _) = difftest::constructor::build_brep(&script);
    if let Some(mut b) = b {
        if let Some(&f) = solidcore::topology::brep::p1::faces(&b, None)
            .ok()
            .and_then(|v| v.get(fi).copied())
            .as_ref()
        {
            report("constructor", &k, &mut b, f);
        }
    }
}
