//! Asset provenance gate. ATTRIBUTION.md is the single inventory of collected files.
use std::collections::BTreeSet;
use std::path::Path;

const ASSET_EXT: &[&str] = &[
    "png",
    "jpg",
    "jpeg",
    "svg",
    "ico",
    "icns",
    "car",
    "vectorcraft",
    "ai",
    "psd",
    "afdesign",
    "fig",
    "sketch",
    "ttf",
    "otf",
    "woff",
    "woff2",
    "gif",
    "bmp",
    "webp",
    "wav",
    "mp3",
    "mp4",
    "stl",
    "step",
    "stp",
    "obj",
    "fcstd",
    "3dm",
];

fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with("target") || matches!(name.as_str(), "plan" | "dist" | "node_modules" | "corpus") {
            continue;
        }
        let p = e.path();
        let kind = e.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            if p.starts_with(root.join("assets")) {
                return Err(format!("resource symlink: {}", p.display()));
            }
        } else if kind.is_dir() {
            walk(&p, root, out)?;
        } else if kind.is_file() {
            out.push(p.strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn needs_attribution(path: &str) -> bool {
    // Include code masters, native work, catalogs, metadata and licence texts in the collection.
    if path.starts_with("assets/locales/pending/") {
        return false;
    }
    if path.starts_with("assets/") {
        return !path.ends_with(".md");
    }
    Path::new(path).extension().is_some_and(|ext| ASSET_EXT.contains(&ext.to_string_lossy().to_ascii_lowercase().as_str()))
}

fn missing<'a>(files: &'a [String], attribution: &str) -> Vec<&'a str> {
    let rows: BTreeSet<_> =
        attribution.lines().filter(|line| line.starts_with('|')).filter_map(|line| line.split('|').nth(1)).map(str::trim).collect();
    files.iter().map(String::as_str).filter(|path| needs_attribution(path) && !rows.contains(path)).collect()
}

pub fn run(root: &Path) -> Result<(), String> {
    let mut attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    // Rows still pending (attribution/pending/*.md) count: they are folded on main.
    for (_, row) in crate::pending::attribution_rows(root)? {
        attribution.push('\n');
        attribution.push_str(&row);
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort();
    let missing = missing(&files, &attribution);
    if !missing.is_empty() {
        return Err(format!("assets missing attribution rows:\n{}", missing.iter().map(|path| format!("  {path}")).collect::<Vec<_>>().join("\n")));
    }
    crate::render_assets::run(root, &files, &attribution)?;
    println!("assets: {} files, all attributed", files.iter().filter(|path| needs_attribution(path)).count());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collected_code_native_work_and_metadata_require_rows() {
        for path in
            ["assets/icons/source/icon.rs", "assets/work/project.ai", "assets/fonts/OFL.txt", "assets/locales/en.json", "assets/macos/Assets.car"]
        {
            assert!(needs_attribution(path), "{path}");
        }
        assert!(needs_attribution("docs/screenshot.png"));
        assert!(needs_attribution("design/project.vectorcraft"));
        assert!(!needs_attribution("assets/README.md"));
        assert!(!needs_attribution("crates/ui/src/widget.rs"));
    }

    #[test]
    fn incidental_mentions_do_not_replace_attribution_rows() {
        let files = ["assets/source.svg".to_string(), "assets/README.md".to_string()];
        assert_eq!(missing(&files, "Mention assets/source.svg"), ["assets/source.svg"]);
        assert!(missing(&files, "| assets/source.svg | author | original | MIT |\n").is_empty());
    }
}
