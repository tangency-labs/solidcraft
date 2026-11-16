//! Diagnostics for `goldens --step-corpus --constructor`: prints the calls of a body's
//! constructor script at the given indices (with the surface an `AdjustSTEPUVDomain`
//! call adjusts).
//!
//! `cargo run --profile goldens -p goldens --example constructor_script_ops -- <file.stp> <body> <op>...`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let body = args
        .first()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| {
            let index = args.get(1)?.parse::<usize>().ok()?;
            step_export::import(&b, &step_export::Limits::default())
                .ok()?
                .bodies
                .into_iter()
                .nth(index)
        });
    let Some(body) = body else {
        eprintln!("usage: constructor_script_ops <file.stp> <body> <op>...");
        std::process::exit(2);
    };
    let script = goldens::script::from_body(&body, goldens::script::FromBodyOptions::default());
    for i in args.iter().skip(2).filter_map(|x| x.parse::<usize>().ok()) {
        let Some(op) = script.ops.get(i) else {
            continue;
        };
        println!("op {i}: {op:?}");
        if let goldens::script::Op::AdjustStepUvDomain { surface, .. } = op {
            println!("    surface {:?}", script.surfaces.get(*surface));
        }
    }
}
