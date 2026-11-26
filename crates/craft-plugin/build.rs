#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=wit/plugin.wit");
    let source = fs::read_to_string("wit/plugin.wit")?;
    let package = source
        .lines()
        .find_map(|line| line.strip_prefix("package ").and_then(|line| line.strip_suffix(';')))
        .ok_or("WIT package declaration is missing")?;
    let output = PathBuf::from(env::var("OUT_DIR")?).join("api.rs");
    fs::write(output, format!("pub const API: &str = {package:?};\n"))?;
    Ok(())
}
