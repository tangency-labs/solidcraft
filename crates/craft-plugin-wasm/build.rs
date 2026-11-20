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
    // The importer world shares its common interfaces with the command world, so one host state
    // serves both. `with` needs the package's literal name, which only this script knows.
    let package_name = package.split_once('@').map(|(name, _)| name).ok_or("missing WIT version")?;
    let importer = format!(
        "mod importer_bindings {{\n\
             wasmtime::component::bindgen!({{\n\
                 path: \"../craft-plugin/wit\",\n\
                 world: \"importer-plugin\",\n\
                 with: {{\n\
                     \"{package_name}/types\": super::bindings::{namespace}::{name}::types,\n\
                     \"{package_name}/log\": super::bindings::{namespace}::{name}::log,\n\
                     \"{package_name}/control\": super::bindings::{namespace}::{name}::control,\n\
                     \"{package_name}/clock\": super::bindings::{namespace}::{name}::clock,\n\
                 }},\n\
             }});\n\
         }}\n\
         use importer_bindings::{namespace}::{name}::{{design, evaluate, source}};\n\
         macro_rules! call_import {{ ($guest:expr, $store:expr, $entry:expr, $file:expr, $options:expr) => {{ $guest.{namespace}_{name}_importer().call_import_file($store, $entry, $file, $options) }}; }}\n"
    );
    fs::write(PathBuf::from(env::var("OUT_DIR")?).join("importer.rs"), importer)?;
    Ok(())
}
