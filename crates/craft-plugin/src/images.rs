//! Package image annotations shared by the plugin check and the UI.

use serde_json::Value;

/// The schema keyword that names a package image.
pub const KEY: &str = "x-solidcraft-icon";
/// The largest package image file read.
pub const MAX_BYTES: usize = 64 * 1024;
/// The most distinct images one package may name.
pub const MAX_IMAGES: usize = 64;

/// Image annotations with their paths relative to the entry's input schema. Non-string
/// annotations are included so the package check can explain why they cannot be drawn.
pub fn annotations(schema: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    if let Some(image) = schema.get(KEY) {
        out.push((KEY.into(), image));
    }
    for (name, property) in schema.get("properties").and_then(Value::as_object).into_iter().flat_map(|p| p.iter()) {
        // A list of choices (`items` a `oneOf`) names its options' pictures the same way.
        for (at, choice) in [("", Some(property)), ("items.", property.get("items"))] {
            for (index, option) in choice.and_then(|c| c.get("oneOf")).and_then(Value::as_array).into_iter().flatten().enumerate() {
                if let Some(image) = option.get(KEY) {
                    out.push((format!("properties.{name}.{at}oneOf.{index}.{KEY}"), image));
                }
            }
        }
    }
    out
}

/// The image paths a schema names: at its top, and on its properties' `oneOf` options.
pub fn referenced(schema: &Value) -> Vec<String> {
    annotations(schema).into_iter().filter_map(|(_, image)| image.as_str().map(str::to_owned)).collect()
}

/// Forward slashes, plain names, no `.` or `..`, ending in `.svg`, at most 128 characters.
pub fn valid_path(rel: &str) -> bool {
    (5..=128).contains(&rel.len())
        && rel.ends_with(".svg")
        && rel.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && part.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        })
}

/// A package image's bounded UTF-8 source with the view box used by the app's icons.
/// The caller then parses it in the icon set's SVG subset and colour roles.
pub fn source(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("not a file of at most {} KiB", MAX_BYTES / 1024));
    }
    let src = std::str::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_string())?;
    let square = src
        .find("<svg")
        .and_then(|start| src[start..].split('>').next())
        .is_some_and(|tag| ["viewBox=\"0 0 64 64\"", "viewBox='0 0 64 64'"].iter().any(|view_box| tag.contains(view_box)));
    if !square {
        return Err("its <svg> needs viewBox=\"0 0 64 64\", as the app's icons have".into());
    }
    Ok(src)
}
