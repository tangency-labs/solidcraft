//! Windows File Explorer thumbnails for SolidCraft designs: the picture a save keeps in the design.
//!
//! `solidcraft_explorer.dll` is a COM in-process server, a thumbnail handler (`IThumbnailProvider`
//! with `IInitializeWithStream`), that the installer registers for `.sc` and `.solidcraft`. Windows
//! runs it in its isolated thumbnail process and hands it the file as a stream. It reads
//! `cache/thumbnail.png` with `solidcraft-design-preview`, only when the picture's key matches the
//! design, and never opens the model; a design without a usable picture keeps its icon. It is the
//! Windows counterpart of apps/solidcraft-quicklook (docs/design-container.md, "Portable thumbnail").
//!
//! Everything but the COM server is platform-neutral, so its tests run anywhere.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{Read, Seek};

mod bitmap;
#[cfg(windows)]
mod com;

pub use bitmap::{Bitmap, fit, thumbnail_from_png};

/// The handler's class id: the same for every architecture and both install scopes
/// (assets/platforms/windows/solidcraft-registry.wxi).
pub const CLSID: &str = "{34025B8F-2354-4B5F-8967-89BA0B71FDCF}";
/// [`CLSID`] as a number, for `windows_core::GUID::from_u128`.
pub const CLSID_U128: u128 = 0x34025B8F_2354_4B5F_8967_89BA0B71FDCF;

/// The thumbnail of the design `source` holds, at most `cx` pixels on its longer side: its saved
/// picture when the picture's key matches the design (`solidcraft_design_preview::keyed_picture`),
/// decoded and fitted. `None` for anything else.
pub fn thumbnail<R: Read + Seek>(source: R, cx: u32) -> Option<Bitmap> {
    let picture = solidcraft_design_preview::keyed_picture(source)?;
    thumbnail_from_png(&picture.png, picture.width, picture.height, cx)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
