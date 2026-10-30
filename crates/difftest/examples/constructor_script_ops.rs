//! Diagnostics for `difftest --step-corpus --constructor`: prints the calls of a body's
//! constructor script at the given indices (with the surface an `AdjustSTEPUVDomain`
//! call adjusts).
//!
//! `cargo run --profile difftest -p difftest --example constructor_script_ops -- <file.stp> <body> <op>...`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let body = args
        .first()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| {
            let index = args.get(1)?.parse::<usize>().ok()?;
            step_import::import(&b, &step_import::Limits::default())
                .ok()?
                .bodies
                .into_iter()
                .nth(index)
        });
    let Some(body) = body else {
        eprintln!("usage: constructor_script_ops <file.stp> <body> <op>...");
        std::process::exit(2);
    };
    let script =
        step_import::script::from_body(&body, step_import::script::FromBodyOptions::default());
    for i in args.iter().skip(2).filter_map(|x| x.parse::<usize>().ok()) {
        let Some(op) = script.ops.get(i) else {
            continue;
        };
        println!("op {i}: {op:?}");
        if let step_import::script::Op::AdjustStepUvDomain { surface, .. } = op {
            println!("    surface {:?}", script.surfaces.get(*surface));
        }
    }
}
