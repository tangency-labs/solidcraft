//! Diagnostics for `difftest --step-corpus`: the independent area estimate of one
//! STEP body ([`step_import::script::step_face_area`] summed over its faces), or which
//! faces have no analytic estimate.
//!
//! `cargo run --profile difftest -p difftest --example step_body_area -- <file.stp> <body>`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(file), Some(index)) = (
        args.first(),
        args.get(1).and_then(|x| x.parse::<usize>().ok()),
    ) else {
        eprintln!("usage: step_body_area <file.stp> <body>");
        std::process::exit(2);
    };
    let body = std::fs::read(file)
        .ok()
        .and_then(|b| step_import::import(&b, &step_import::Limits::default()).ok())
        .and_then(|m| m.bodies.into_iter().nth(index));
    let Some(body) = body else {
        eprintln!("no body {index} in {file}");
        std::process::exit(2);
    };
    let mut total = 0.0;
    let mut missing = 0usize;
    for f in &body.faces {
        match step_import::script::step_face_area(&body, f, 2048) {
            Some(a) => total += a,
            None => missing += 1,
        }
    }
    println!(
        "{file} {index} faces={} estimate={total} faces_without_estimate={missing}",
        body.faces.len()
    );
}
