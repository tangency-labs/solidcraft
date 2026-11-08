//! Prints one body of a STEP file: its debug form, or with `--flat` its flat encoding
//! (one number per line), for debugging the importer and the C++ shim.
//!
//! `cargo run -p step-import --example dump_body -- <file.stp> <body index> [--flat]`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(path), Some(index)) = (args.first(), args.get(1)) else {
        eprintln!("usage: dump_body <file.stp> <body index> [--flat]");
        std::process::exit(2);
    };
    let bytes = std::fs::read(path).unwrap_or_default();
    let model = match step_import::import(&bytes, &step_import::Limits::default()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("parse error: {e}");
            std::process::exit(1);
        }
    };
    let Some(body) = index
        .parse::<usize>()
        .ok()
        .and_then(|i| model.bodies.get(i))
    else {
        eprintln!("no body {index}");
        std::process::exit(1);
    };
    if args.iter().any(|a| a == "--flat") {
        for x in step_import::flat::encode(body) {
            println!("{x:?}");
        }
    } else {
        println!("{body:#?}");
    }
}
