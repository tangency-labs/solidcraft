//! A design file as text, for `git diff` (#626). Git runs `solidcraft-cli textconv` on each side of
//! a diff and compares the text, so a `.sc` container, which is a ZIP, shows its changes too.
//!
//! - A `.json` design is laid out as a `.json` save lays it out (`json_layout`), whatever layout
//!   it was saved in. A file saved by a build with that layout prints as it is.
//! - A container prints `container.json` on one line, then `document.json` (or a drawing
//!   container's `drawing.json`) and each `drawings/<id>.json` in the same layout. Every other part
//!   gets one line with its name, its size and the SHA-256 of its bytes. A blob whose bytes do not
//!   match the SHA-256 in its name says so.
//! - Parts that change on every save without the design changing are left out. Those are the model
//!   cache and preview under `cache/`, the signature log under `provenance/` and a plugin's own
//!   `cache/` folder.
//!
//! The file is printed as it is stored. Nothing is upgraded, evaluated or followed (links), so a
//! design saved by a newer build or with a damaged part still prints. A file that is neither a
//! container nor JSON, or a container that does not open, prints one line saying so. The text never
//! names the file, because git hands textconv a temporary copy.

use std::fmt::Write as _;
use std::io::Cursor;

use serde::de::IgnoredAny;
use solidcraft_doc::json_layout::{compact, pretty};
use solidcraft_format::container::digest;

use super::{PackageReader, is_design_container, part_limit};

/// The parts printed as laid-out JSON: the design or drawing itself and a design's drawings.
fn is_text_part(name: &str) -> bool {
    name == "document.json" || name == "drawing.json" || (name.starts_with("drawings/") && name.ends_with(".json"))
}

/// The parts left out: derived data that a save writes afresh each time.
fn is_derived(name: &str) -> bool {
    let mut path = name.split('/');
    let (first, _, third) = (path.next(), path.next(), path.next());
    first == Some("cache") || first == Some("provenance") || (first == Some("plugins") && third == Some("cache"))
}

/// JSON text laid out as a `.json` save lays it out, with a final line feed.
fn laid_out(text: &str) -> String {
    let mut out = pretty(&compact(text));
    out.push('\n');
    out
}

/// One line for bytes that are not shown: what they are, their size and their SHA-256.
fn summary(what: &str, bytes: &[u8]) -> String {
    format!("# {what} ({} bytes, SHA-256 {})\n", bytes.len(), digest(bytes))
}

/// The text of a design file's bytes, for `git diff`. Never fails and never panics: content it
/// cannot show is summed up in one line.
pub fn design_text(bytes: &[u8]) -> String {
    if is_design_container(bytes) {
        return container_text(bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) if serde_json::from_str::<IgnoredAny>(text).is_ok() => laid_out(text),
        Ok(_) => summary("not JSON and not a SolidCraft container", bytes),
        Err(_) => summary("not text and not a SolidCraft container", bytes),
    }
}

fn container_text(bytes: &[u8]) -> String {
    let mut reader = match PackageReader::new(Cursor::new(bytes)) {
        Ok(reader) => reader,
        Err(e) => return summary(&format!("a SolidCraft container that does not open: {e}"), bytes),
    };
    let mut names = reader.part_names();
    names.sort();
    let mut out = String::new();
    let mut parts = String::new();
    for name in names.iter().filter(|name| !is_derived(name)) {
        let part = match reader.limited_part(name, part_limit(name)) {
            Ok(part) => part,
            Err(e) => {
                let _ = writeln!(parts, "{name} damaged ({e})");
                continue;
            }
        };
        let json = std::str::from_utf8(&part).ok().filter(|text| serde_json::from_str::<IgnoredAny>(text).is_ok());
        match json {
            Some(text) if name == "container.json" => {
                let _ = writeln!(out, "# SolidCraft container {}", compact(text));
            }
            Some(text) if is_text_part(name) => {
                let _ = writeln!(out, "# {name}");
                out.push_str(&laid_out(text));
            }
            _ => {
                let hash = digest(&part);
                // A blob is named for the SHA-256 of its bytes: `blobs/<sha256>.<extension>`.
                let named = name.strip_prefix("blobs/").and_then(|file| file.split('.').next());
                let damaged = if named.is_some_and(|named| named != hash) { " damaged (its bytes do not match its name)" } else { "" };
                let _ = writeln!(parts, "{name} {} {hash}{damaged}", part.len());
            }
        }
    }
    if !parts.is_empty() {
        out.push_str("# parts (name, bytes, SHA-256)\n");
        out.push_str(&parts);
    }
    out
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
