use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let wit = "../craft-plugin/wit/plugin.wit";
    println!("cargo:rerun-if-changed={wit}");
    let source = fs::read_to_string(wit)?;
    let package =
        source.lines().find_map(|line| line.strip_prefix("package ").and_then(|line| line.strip_suffix(';'))).ok_or("missing WIT package")?;
    let (namespace, rest) = package.split_once(':').ok_or("missing WIT namespace")?;
    let (name, _) = rest.split_once('@').ok_or("missing WIT version")?;
    let namespace = namespace.replace('-', "_");
    let name = name.replace('-', "_");
    let output = format!(
        "use bindings::{namespace}::{name}::{{clock, commands, control, files, log, types::{{self, PluginError}}}};\n\
         macro_rules! call_entry {{ ($guest:expr, $store:expr, $entry:expr, $params:expr) => {{ $guest.{namespace}_{name}_entry().call_execute($store, $entry, $params) }}; }}\n"
    );
    fs::write(PathBuf::from(env::var("OUT_DIR")?).join("aliases.rs"), output)?;
    Ok(())
}
