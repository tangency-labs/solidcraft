//! Vector images a plugin package ships: an entry's icon and pictures for a choice's options.
//!
//! An entry's `input_schema` names them with [`KEY`]: at its top, the command or importer's icon (its ribbon
//! button, its menu rows and its dialog's header); on a `oneOf` option, that option's picture in
//! the dropdown. Each names an `.svg` file inside the package by a relative path.
//!
//! The file is read once, when the plugins are listed, from the package folder only: no network,
//! no path outside the folder, no link, at most 64 KiB. It must be an icon in the app's own SVG
//! subset (`solidcraft_ui_kit::icons`), the check the built-in icons pass: a 64 by 64 view box,
//! absolute paths, circles, groups, masks and clips, and colours that name the icon palette's
//! roles (or `currentColor`), so it is drawn in the theme's colours like the app's icons. Text,
//! images, scripts, styles, links and references to other files are not in the subset, so such a
//! file is refused. A refused or missing image is left out (an entry keeps the generic plugin
//! icon) and logged; it never stops the plugin from working.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

use serde_json::Value;
use solidcraft_engine::plugin::images;

pub(crate) use images::{KEY, referenced, valid_path};

/// The icon every plugin entry has when it names none of its own.
pub(crate) const GENERIC: &str = "prefs_plugins";

/// The largest image file read.
const MAX_BYTES: u64 = images::MAX_BYTES as u64;

/// The most images one package may name.
const MAX_IMAGES: usize = images::MAX_IMAGES;

/// Every image the entries' schemas name, read from the package at `dir`: each relative path
/// with the icon name it is drawn by. Paths that are not usable are left out.
pub(crate) fn load(dir: &Path, plugin_id: &str, entries: &[solidcraft_engine::plugin::Entry]) -> BTreeMap<String, String> {
    let mut named = Vec::new();
    for entry in entries {
        named.extend(referenced(&entry.input_schema));
    }
    named.sort();
    named.dedup();
    let mut images = BTreeMap::new();
    for rel in named.into_iter().take(MAX_IMAGES) {
        match read(dir, plugin_id, &rel) {
            Ok(name) => {
                images.insert(rel, name);
            }
            Err(why) => log::warn!("plugin {plugin_id}: image {rel}: {why}"),
        }
    }
    images
}

/// Shared across the inventory and open dialogs. Files and decoding stay off the window thread.
pub(crate) type Swatches = std::sync::Arc<std::sync::OnceLock<Artwork>>;

/// The decoded package rasters, kept apart by the annotation that named them: a file accepted as
/// a larger `x-solidcraft-image` picture may still have failed the swatch limits.
#[derive(Debug, Default)]
pub(crate) struct Artwork {
    pub(crate) swatches: BTreeMap<String, Swatch>,
    pub(crate) pictures: BTreeMap<String, Swatch>,
}

#[derive(Debug)]
pub(crate) struct Swatch {
    image: egui::ColorImage,
    key: egui::Id,
}

impl Swatch {
    /// Upload once in each egui context. A content key prevents stale textures after a reload.
    pub(crate) fn texture(&self, ctx: &egui::Context) -> egui::load::SizedTexture {
        let cached = ctx.data(|data| data.get_temp::<egui::TextureHandle>(self.key));
        let texture = match cached {
            Some(texture) => texture,
            None => {
                let texture = ctx.load_texture("plugin swatch", self.image.clone(), egui::TextureOptions::LINEAR);
                ctx.data_mut(|data| data.insert_temp(self.key, texture.clone()));
                texture
            }
        };
        egui::load::SizedTexture::new(texture.id(), texture.size_vec2())
    }
}

pub(crate) fn read_swatches(dir: &Path, entries: &[solidcraft_engine::plugin::Entry]) -> Artwork {
    let report = solidcraft_engine::plugin::swatches::check(entries, |rel| solidcraft_engine::plugin::swatches::read_artwork(dir, rel));
    for warning in report.warnings {
        log::warn!("plugin {}: {warning}", dir.display());
    }
    let texture = |(path, image): (String, solidcraft_engine::plugin::swatches::Swatch)| {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        image.size.hash(&mut hash);
        image.rgba.hash(&mut hash);
        let key = egui::Id::unique(("plugin_swatch", hash.finish()));
        (path, Swatch { image: egui::ColorImage::from_rgba_unmultiplied(image.size, &image.rgba), key })
    };
    Artwork { swatches: report.images.into_iter().map(texture).collect(), pictures: report.pictures.into_iter().map(texture).collect() }
}

pub(crate) fn load_swatches(dir: &Path, entries: &[solidcraft_engine::plugin::Entry]) -> Swatches {
    let swatches = Swatches::default();
    let referenced = entries.iter().any(|entry| {
        entry.input_schema.get("properties").and_then(Value::as_object).into_iter().flat_map(|p| p.values()).any(|property| {
            property.get("oneOf").and_then(Value::as_array).into_iter().flatten().any(|option| {
                option.get(solidcraft_engine::plugin::swatches::KEY).is_some() || option.get(solidcraft_engine::plugin::swatches::IMAGE_KEY).is_some()
            })
        })
    });
    if !referenced {
        let _ = swatches.set(Artwork::default());
        return swatches;
    }
    let (dir, entries, result) = (dir.to_path_buf(), entries.to_vec(), swatches.clone());
    if let Err(error) = std::thread::Builder::new().name("plugin-swatches".into()).spawn(move || {
        let _ = result.set(read_swatches(&dir, &entries));
    }) {
        log::warn!("could not load plugin swatches: {error}");
        let _ = swatches.set(Artwork::default());
    }
    swatches
}

/// Read, check and add the image `rel` of the package at `dir`: the icon name it is drawn by.
fn read(dir: &Path, plugin_id: &str, rel: &str) -> Result<String, String> {
    if !valid_path(rel) {
        return Err("not a relative .svg path inside the package".into());
    }
    let path = dir.join(rel);
    // Every component a plain folder or file: no link leads out of the package.
    let mut at = dir.to_path_buf();
    for part in rel.split('/') {
        at.push(part);
        let meta = std::fs::symlink_metadata(&at).map_err(|_| "not found in the package".to_string())?;
        if meta.file_type().is_symlink() {
            return Err("a link".into());
        }
    }
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(format!("not a file of at most {} KiB", MAX_BYTES / 1024));
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let src = images::source(&bytes)?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut hash);
    let name = format!("plugin:{plugin_id}:{rel}:{:016x}", hash.finish());
    solidcraft_ui_kit::icons::add(&name, src)?;
    Ok(name)
}

#[cfg(test)]
#[path = "plugin_images_tests.rs"]
mod tests;
