//! A design file as text, for `git diff` (#626). Git runs `solidcraft-cli textconv` on each side of
//! a diff and compares the text, so a `.sc` container, which is a ZIP, shows its changes too.
//!
//! - A `.json` design is laid out as a `.json` save lays it out (`json_layout`), whatever layout
//!   it was saved in. A file saved by a build with that layout prints as it is.
//! - A container prints `container.json` on one line, then `document.json` (or a drawing
//!   container's `drawing.json`) and each `drawings/<id>.json` in the same layout. Every other part
//!   gets one line with its name, its size and the SHA-256 of its bytes. A blob whose bytes do not
//!   match the SHA-256 in its name says so.
//! - The model cache, the preview and the thumbnail under `cache/` are left out, because a save
//!   writes them afresh, so they would show as a change on every save. So is a plugin's own
//!   `cache/` folder, which the plugin rebuilds. Everything else a container holds is shown,
//!   the signature log included, because a change there is a real change to the file.
//!
//! The file is printed as it is stored. Nothing is upgraded, evaluated or followed (links), so a
//! design saved by a newer build or with a damaged part still prints. A file that is neither a
//! container nor JSON, or a container that does not open, prints one line saying so, with its size
//! and SHA-256. So does JSON nested deeper than [`MAX_DEPTH`], or JSON that would lay out to more
//! than [`budget`] bytes, because the layout indents every line two spaces a level and would take
//! memory that grows with the size times the depth. When a part does not read, the text ends with
//! the SHA-256 of the whole file, so a change inside that part still shows.

use std::fmt::Write as _;
use std::io::{Cursor, Read};

use serde::de::IgnoredAny;
use sha2::{Digest, Sha256};
use solidcraft_doc::json_layout::{compact, depth, pretty_within};
use solidcraft_format::container::digest;

use super::{PackageReader, is_design_container, part_limit};

/// The deepest nesting laid out, the 127 levels serde_json reads in a design.
const MAX_DEPTH: usize = 127;

/// How many times its size JSON text may grow when laid out. A real design lays out to about twice
/// its size.
const GROWTH: usize = 4;

/// The laid-out bytes always allowed, so small text with long lines still prints.
const FLOOR: usize = 64 << 20;

/// The most bytes JSON text of `length` bytes may lay out to.
fn budget(length: usize) -> usize {
    length.saturating_mul(GROWTH).max(FLOOR)
}

/// Whether a part is printed as laid-out JSON. These are the design or drawing itself and a
/// design's drawings.
fn is_text_part(name: &str) -> bool {
    name == "document.json" || name == "drawing.json" || (name.starts_with("drawings/") && name.ends_with(".json"))
}

/// Whether a part is left out, as a cache that a save or a plugin writes afresh.
fn is_cache(name: &str) -> bool {
    let mut path = name.split('/');
    let (first, _, third) = (path.next(), path.next(), path.next());
    first == Some("cache") || (first == Some("plugins") && third == Some("cache"))
}

/// One line for bytes that are not shown, with what they are, their size and their SHA-256.
fn summary(what: &str, bytes: &[u8]) -> String {
    format!("# {what} ({} bytes, SHA-256 {})\n", bytes.len(), digest(bytes))
}

/// JSON text laid out as a `.json` save lays it out, with a final line feed. Text nested too deeply,
/// or that would lay out past `budget` bytes, is summed up in one line instead.
fn laid_out(text: &str, budget: usize) -> String {
    if depth(text) > MAX_DEPTH {
        return summary(&format!("JSON nested more than {MAX_DEPTH} levels deep"), text.as_bytes());
    }
    let Some(mut out) = pretty_within(&compact(text), budget) else {
        return summary(&format!("JSON that would lay out to more than {budget} bytes"), text.as_bytes());
    };
    out.push('\n');
    out
}

/// The text of a design file's bytes, for `git diff`. It never fails and never panics. Content it
/// cannot show is summed up in one line.
pub fn design_text(bytes: &[u8]) -> String {
    if is_design_container(bytes) {
        return container_text(bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) if serde_json::from_str::<IgnoredAny>(text).is_ok() => laid_out(text, budget(text.len())),
        Ok(_) => summary("not JSON and not a SolidCraft container", bytes),
        Err(_) => summary("not text and not a SolidCraft container", bytes),
    }
}

/// One line for a file too large to read whole, from its bytes read a piece at a time. It says
/// `what` the file is, its size and its SHA-256, so a change that keeps the size still shows.
pub fn large_file_text(what: &str, mut source: impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut length = 0u64;
    loop {
        let read = match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(buffer.get(..read).unwrap_or_default());
        length = length.saturating_add(read as u64);
    }
    Ok(format!("# {what} ({length} bytes, SHA-256 {:x})\n", hasher.finalize()))
}

fn container_text(bytes: &[u8]) -> String {
    let mut reader = match PackageReader::new(Cursor::new(bytes)) {
        Ok(reader) => reader,
        Err(e) => return summary(&format!("a SolidCraft container that does not open ({e})"), bytes),
    };
    let mut names = reader.part_names();
    names.sort();
    let mut out = String::new();
    let mut parts = String::new();
    let mut damaged = false;
    // One budget for every laid-out part together, so a container of many parts cannot add up to
    // more than [`FLOOR`] and [`GROWTH`] times the text it holds.
    let mut allowance = FLOOR;
    for name in names.iter().filter(|name| !is_cache(name)) {
        let part = match reader.limited_part(name, part_limit(name)) {
            Ok(part) => part,
            Err(e) => {
                let _ = writeln!(parts, "{name} damaged ({e})");
                damaged = true;
                continue;
            }
        };
        let json = std::str::from_utf8(&part).ok().filter(|text| serde_json::from_str::<IgnoredAny>(text).is_ok());
        match json {
            Some(text) if name == "container.json" && depth(text) <= MAX_DEPTH => {
                let _ = writeln!(out, "# SolidCraft container {}", compact(text));
            }
            Some(text) if is_text_part(name) => {
                let _ = writeln!(out, "# {name}");
                allowance = allowance.saturating_add(text.len().saturating_mul(GROWTH));
                let laid = laid_out(text, allowance);
                allowance = allowance.saturating_sub(laid.len());
                out.push_str(&laid);
            }
            _ => {
                let hash = digest(&part);
                // A blob is named for the SHA-256 of its bytes (`blobs/<sha256>.<extension>`).
                let named = name.strip_prefix("blobs/").and_then(|file| file.split('.').next());
                let mismatch = if named.is_some_and(|named| named != hash) { " damaged (its bytes do not match its name)" } else { "" };
                let _ = writeln!(parts, "{name} {} {hash}{mismatch}", part.len());
            }
        }
    }
    if !parts.is_empty() {
        out.push_str("# parts (name, bytes, SHA-256)\n");
        out.push_str(&parts);
    }
    if damaged {
        out.push_str(&summary("the whole file, because a part above does not read", bytes));
    }
    out
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
