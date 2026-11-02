//! Paths and conditional writes for the optional same-origin server folder.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub const PREFIX: &str = "/server/";
pub const ENDPOINT: &str = "/designs/";
pub const MAX_LISTING: usize = 10_000;
pub const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;

/// Relative names only; reject ambiguous URL encodings and platform path separators.
pub fn valid(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 2048
        && path.split('/').count() <= 32
        && path.split('/').all(|s| !s.is_empty() && s != "." && s != ".." && s.len() <= 255 && !s.starts_with('.'))
        && !path.chars().any(|c| c.is_control() || matches!(c, '\\' | '%' | '?' | '#' | ':'))
}

pub fn parse(path: &str) -> Option<&str> {
    path.strip_prefix(PREFIX).filter(|p| valid(p))
}

pub fn engine_path(path: &str) -> Option<String> {
    valid(path).then(|| format!("{PREFIX}{path}"))
}

pub fn endpoint(path: &str) -> Option<String> {
    if !path.is_empty() && !valid(path) {
        return None;
    }
    let mut out = ENDPOINT.to_string();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Some(out)
}

/// Decode once, then validate before URL normalization can hide traversal segments.
pub fn from_endpoint(value: &str) -> Option<String> {
    let encoded = value.strip_prefix(ENDPOINT)?;
    if encoded.len() > 6144 {
        return None;
    }
    let mut out = Vec::new();
    let mut chars = encoded.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let a = char::from(chars.next()?).to_digit(16)?;
            let b = char::from(chars.next()?).to_digit(16)?;
            let byte = (a * 16 + b) as u8;
            if byte == b'/' || byte == b'\\' {
                return None;
            }
            out.push(byte);
        } else {
            out.push(b);
        }
    }
    let path = String::from_utf8(out).ok()?;
    valid(&path).then_some(path)
}

/// A failed precondition is the only response that offers an explicit overwrite.
pub fn save_conflict(status: u16, interactive: bool) -> bool {
    interactive && status == 412
}

#[cfg(target_arch = "wasm32")]
#[path = "server_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;

/// New paths must not replace an existing design; only an explicit overwrite skips validation.
pub fn save_condition(tag: Option<&str>, overwrite: bool) -> Option<(&'static str, &str)> {
    if overwrite { None } else { Some(tag.map_or(("If-None-Match", "*"), |tag| ("If-Match", tag))) }
}

/// Only a root-relative folder URL or a URL on this exact origin is writable in place.
pub fn link_path(value: &str, origin: &str) -> Option<String> {
    from_endpoint(value).or_else(|| value.strip_prefix(origin).and_then(from_endpoint))
}
