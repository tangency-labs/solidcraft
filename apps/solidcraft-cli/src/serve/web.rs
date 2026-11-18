//! The browser app's files (`--web DIR`), served with the caching rules of
//! `packaging/docker/nginx.conf`: `index.html`, `sw.js`, `build.json` and the rest are asked for
//! again every time (`no-cache`), Trunk's hashed files (`name-<hash>.js`, `_bg.wasm`, `.css`) are
//! kept for a year. Only files inside the folder, after resolving links, are served.

use std::path::{Path, PathBuf};

use super::http::{Body, HttpError, Response};

pub struct Site {
    root: PathBuf,
}

impl Site {
    pub fn new(root: &Path) -> Result<Site, String> {
        let root = std::fs::canonicalize(root).map_err(|e| format!("--web {}: {e}", root.display()))?;
        if !root.join("index.html").is_file() {
            return Err(format!("--web {}: no index.html there (give the folder of the web build, dist/web)", root.display()));
        }
        Ok(Site { root })
    }

    /// The file for the URL path `path`.
    pub fn get(&self, path: &str) -> Result<Response, HttpError> {
        let not_found = || HttpError::new(404, "no such file");
        let rel = path.trim_start_matches('/');
        let mut file = self.root.clone();
        for s in rel.split('/').filter(|s| !s.is_empty()) {
            if s == "." || s == ".." || s.starts_with('.') || s.contains(['\\', ':']) {
                return Err(not_found());
            }
            file.push(s);
        }
        if file.is_dir() {
            file.push("index.html");
        }
        let file = std::fs::canonicalize(&file).map_err(|_| not_found())?;
        if !file.starts_with(&self.root) {
            return Err(not_found());
        }
        let f = std::fs::File::open(&file).map_err(|_| not_found())?;
        let meta = f.metadata().map_err(|_| not_found())?;
        if !meta.is_file() {
            return Err(not_found());
        }
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Response { status: 200, headers: Vec::new(), body: Body::File(f, meta.len()) }
            .with("Content-Type", content_type(&name))
            .with("Cache-Control", if hashed(&name) { "public, max-age=31536000, immutable" } else { "no-cache" }))
    }
}

fn content_type(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "ttf" => "font/ttf",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Trunk's content-hashed names (`-[0-9a-f]{8,16}(_bg)?.(js|wasm|css)`), which never change.
fn hashed(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    if !matches!(ext, "js" | "wasm" | "css") {
        return false;
    }
    let stem = stem.strip_suffix("_bg").unwrap_or(stem);
    let Some((_, hash)) = stem.rsplit_once('-') else { return false };
    (8..=16).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_names_are_kept_and_the_rest_asked_for_again() {
        assert!(hashed("solidcraft-web-0123456789abcdef_bg.wasm"));
        assert!(hashed("solidcraft-web-01234567.js"));
        assert!(!hashed("sw.js"));
        assert!(!hashed("index.html"));
        assert!(!hashed("solidcraft-web-0123.js"));
        assert_eq!(content_type("x_bg.wasm"), "application/wasm");
    }
}
