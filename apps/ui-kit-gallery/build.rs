//! Embed the kit's English messages, including strings awaiting the merge-time locale fold.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Messages = BTreeMap<String, String>;

fn merge(messages: &mut Messages, added: Messages, source: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for (id, text) in added {
        if messages.insert(id.clone(), text).is_some() {
            return Err(format!("{}: `{id}` is already in an English catalogue or pending file", source.display()).into());
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join("../..");
    let mut messages = BTreeMap::<String, String>::new();
    for source in ["assets/locales/en.json", "crates/ui-text/locales/en.json"] {
        let path = root.join(source);
        println!("cargo:rerun-if-changed={}", path.display());
        merge(&mut messages, serde_json::from_slice(&std::fs::read(&path)?)?, &path)?;
    }
    let pending = root.join("assets/locales/pending");
    println!("cargo:rerun-if-changed={}", pending.display());
    let mut files = std::fs::read_dir(pending)?.map(|entry| entry.map(|entry| entry.path())).collect::<Result<Vec<_>, _>>()?;
    files.sort();
    for path in files.into_iter().filter(|path| path.extension().is_some_and(|extension| extension == "json")) {
        let mut locales: BTreeMap<String, BTreeMap<String, String>> = serde_json::from_slice(&std::fs::read(&path)?)?;
        merge(&mut messages, locales.remove("en").unwrap_or_default(), &path)?;
    }
    let output = PathBuf::from(std::env::var("OUT_DIR")?).join("gallery-en.json");
    std::fs::write(output, serde_json::to_vec(&messages)?)?;
    Ok(())
}
