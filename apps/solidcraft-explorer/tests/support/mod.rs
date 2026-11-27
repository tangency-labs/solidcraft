//! Design containers with real pictures, built in memory as a save writes them
//! (docs/design-container.md, "Portable thumbnail").
#![allow(dead_code)]

use std::io::{Cursor, Write};

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

pub const DOCUMENT: &[u8] = br#"{"format":"solidcraft/73","name":"Thumbnail","units":"mm","features":[]}"#;
pub const CONTAINER: &[u8] = br#"{"format":"solidcraft-container/1"}"#;
pub const V1: &str = "solidcraft-thumbnail/1";
pub const V2: &str = "solidcraft-thumbnail/2";

/// Lowercase hexadecimal SHA-256, as a save keys its picture.
pub fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// An RGBA picture `width` x `height`: the left half opaque, its top quarter green
/// `[0, 255, 0, 255]` and the rest red `[255, 0, 0, 255]`; the right half transparent blue
/// `[0, 0, 255, 0]`. It carries a private `scSh` chunk, as SolidCraft's pictures do.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(if x >= width / 2 {
                &[0, 0, 255, 0]
            } else if y < height / 4 {
                &[0, 255, 0, 255]
            } else {
                &[255, 0, 0, 255]
            });
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_chunk(png::chunk::ChunkType(*b"scSh"), &[0; 16]).unwrap();
        writer.write_image_data(&data).unwrap();
    }
    out
}

/// A ZIP of `parts`, each Deflate-compressed.
pub fn container(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A design saved with a `width` x `height` picture of format `tag`, keyed to `keyed_document`
/// (pass `DOCUMENT` for a picture that matches the design).
pub fn keyed_to(tag: &str, width: u32, height: u32, keyed_document: &[u8]) -> Vec<u8> {
    let picture = png(width, height);
    let key = format!(r#"{{"format":"{tag}","app":"0.51.1","document":"{}","payload":"{}"}}"#, hex(keyed_document), hex(&picture));
    container(&[
        ("container.json", CONTAINER),
        ("document.json", DOCUMENT),
        ("cache/thumbnail.json", key.as_bytes()),
        ("cache/thumbnail.png", &picture),
    ])
}

/// A design saved with a matching picture of format `tag`.
pub fn design(tag: &str, width: u32, height: u32) -> Vec<u8> {
    keyed_to(tag, width, height, DOCUMENT)
}

/// A format 2 design whose `cache/thumbnail.png` entry expands to 64 MiB (a decompression bomb).
pub fn bomb() -> Vec<u8> {
    let key = format!(r#"{{"format":"{V2}","document":"{}","payload":"{}"}}"#, hex(DOCUMENT), "0".repeat(64));
    let zeros = vec![0_u8; 64 << 20];
    container(&[
        ("container.json", CONTAINER),
        ("document.json", DOCUMENT),
        ("cache/thumbnail.json", key.as_bytes()),
        ("cache/thumbnail.png", &zeros),
    ])
}

/// `bytes` with its end-of-central-directory record claiming 65,535 entries.
pub fn many_entries(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let at = out.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    out[at + 8..at + 12].copy_from_slice(&[0xFF; 4]);
    out
}
