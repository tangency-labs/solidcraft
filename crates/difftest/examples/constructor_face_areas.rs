//! Diagnostics for `difftest --step-corpus --constructor`: per face of one STEP body,
//! its area built alone by `CreateFacesWithCurves` and in the `SmBrepConstructor`
//! B-rep (Rust port), with the face's surface kind and orientation flags.
//!
//! `cargo run --profile difftest -p difftest --example constructor_face_areas -- <file.stp> <body>`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(file), Some(index)) = (
        args.first(),
        args.get(1).and_then(|x| x.parse::<usize>().ok()),
    ) else {
        eprintln!("usage: constructor_face_areas <file.stp> <body>");
        std::process::exit(2);
    };
    let Ok(bytes) = std::fs::read(file) else {
        eprintln!("cannot read {file}");
        std::process::exit(2);
    };
    let Some(body) = step_import::import(&bytes, &step_import::Limits::default())
        .ok()
        .and_then(|m| m.bodies.into_iter().nth(index))
    else {
        eprintln!("no body {index}");
        std::process::exit(2);
    };
    let areas = difftest::step::face_areas(
        &body,
        difftest::step::ACCURACY,
        std::env::var_os("ADJUST_ALONE").is_some(),
    );
    let shell_of = |fi: usize| body.shells.iter().position(|s| s.faces.contains(&fi));
    for (fi, (alone, ctor)) in areas.iter().enumerate() {
        let Some(face) = body.faces.get(fi) else {
            continue;
        };
        let kind = format!("{:?}", face.surface);
        let kind = kind
            .split([' ', '{', '('])
            .next()
            .unwrap_or("?")
            .to_string();
        let flag = match (alone, ctor) {
            (Some(a), Some(c)) if (a - c).abs() <= 1e-6 * a.abs().max(c.abs()).max(1e-300) => "",
            _ => "  <<",
        };
        let exact = step_import::script::step_face_area(&body, face, 512);
        let close = |x: &Option<f64>| match (x, exact) {
            (Some(a), Some(e)) => (a - e).abs() <= 1e-4 * e.abs().max(1e-300),
            _ => false,
        };
        println!(
            "face {fi:4} {kind:12} same_sense={} shell={:?} loops={} alone={alone:?} ctor={ctor:?} green={exact:?} alone_ok={} ctor_ok={}{flag}",
            face.same_sense,
            shell_of(fi),
            face.loops.len(),
            close(alone),
            close(ctor)
        );
    }
}
