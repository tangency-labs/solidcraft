//! A design file as text, for `git diff` (#626). Git runs `solidcraft-cli textconv` on each side of
//! a diff and compares the text, so a `.sc` container, which is a ZIP, shows its changes too.
//!
//! - A `.json` design is laid out as a `.json` save lays it out (`json_layout`), whatever layout
//!   it was saved in. A file saved by a build with that layout prints as it is.
//! - A container prints `container.json` on one line, then `document.json` (or a drawing
//!   container's `drawing.json`) and each `drawings/<id>.json` in the same layout. Every other part
//!   gets one line with its name, its size and the SHA-256 of its bytes. A blob whose bytes do not
//!   match the SHA-256 in its name says so. A name holding a control character, or starting with a
//!   quote, is printed as a JSON string, so it cannot print as the lines of other parts.
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

use std::borrow::Cow;
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

/// A part's name as printed. A name holding a control character (a line feed could forge the
/// lines of other parts) or starting with a quote is printed as a JSON string, so two different
/// names never print alike. A name read from a container never holds a backslash, and every
/// quoted name starts with a quote.
fn shown(name: &str) -> Cow<'_, str> {
    if name.starts_with('"') || name.chars().any(char::is_control) {
        Cow::Owned(serde_json::to_string(name).unwrap_or_default())
    } else {
        Cow::Borrowed(name)
    }
}

/// An error's text on one line, so it cannot add lines of its own.
fn one_line(error: &dyn std::fmt::Display) -> String {
    error.to_string().replace(char::is_control, " ")
}

/// JSON text laid out as a `.json` save lays it out, with a final line feed, and the bytes the
/// layout wrote. Text nested too deeply, or that would lay out past `budget` bytes, is summed up in
/// one line instead. A layout that gives up has written the whole budget, so that is what it spent.
fn laid_out(text: &str, budget: usize) -> (String, usize) {
    if depth(text) > MAX_DEPTH {
        let line = summary(&format!("JSON nested more than {MAX_DEPTH} levels deep"), text.as_bytes());
        let spent = line.len();
        return (line, spent);
    }
    let Some(mut out) = pretty_within(&compact(text), budget) else {
        return (summary(&format!("JSON that would lay out to more than {budget} bytes"), text.as_bytes()), budget);
    };
    out.push('\n');
    let spent = out.len();
    (out, spent)
}

/// The text of a design file's bytes, for `git diff`. It never fails and never panics. Content it
/// cannot show is summed up in one line.
pub fn design_text(bytes: &[u8]) -> String {
    if is_design_container(bytes) {
        return container_text(bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) if serde_json::from_str::<IgnoredAny>(text).is_ok() => laid_out(text, budget(text.len())).0,
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
        Err(e) => return summary(&format!("a SolidCraft container that does not open ({})", one_line(&e)), bytes),
    };
    // Each part is read by its place, not looked up by name, so names stored without the UTF-8
    // flag do not make reading every part quadratic.
    let mut names = reader.indexed_part_names();
    names.sort();
    let mut out = String::new();
    let mut parts = String::new();
    let mut damaged = false;
    // One budget for every laid-out part together, so a container of many parts cannot add up to
    // more than [`FLOOR`] and [`GROWTH`] times the text it holds. It is charged for the bytes each
    // layout wrote, a layout that gave up included, so the work stays within it as well.
    let mut allowance = FLOOR;
    for (name, index) in names.iter().filter(|(name, _)| !is_cache(name)) {
        let part = match reader.limited_part_at(*index, part_limit(name)) {
            Ok(part) => part,
            Err(e) => {
                let _ = writeln!(parts, "{} damaged ({})", shown(name), one_line(&e));
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
                let _ = writeln!(out, "# {}", shown(name));
                allowance = allowance.saturating_add(text.len().saturating_mul(GROWTH));
                let (laid, spent) = laid_out(text, allowance);
                allowance = allowance.saturating_sub(spent);
                out.push_str(&laid);
            }
            _ => {
                let hash = digest(&part);
                // A blob is named for the SHA-256 of its bytes (`blobs/<sha256>.<extension>`).
                let named = name.strip_prefix("blobs/").and_then(|file| file.split('.').next());
                let mismatch = if named.is_some_and(|named| named != hash) { " damaged (its bytes do not match its name)" } else { "" };
                let _ = writeln!(parts, "{} {} {hash}{mismatch}", shown(name), part.len());
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
