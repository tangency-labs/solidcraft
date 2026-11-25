//! Review markup pictures (`markup.image`): each picture's bytes in the design, a part of the
//! container of its own under `markups/`, the way a canvas keeps its image. The markup set beside
//! the design (`solidcraft_io::markups`) keeps the threads and anchors and names each picture by
//! its entry here; the design holds the bytes, never text.
use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

use crate::blob::Blob;

/// The container folder the pictures live in.
pub const PREFIX: &str = "markups/";
/// The largest picture, in bytes.
pub const MAX_BYTES: usize = 16 << 20;
/// At most this many pictures in one design; past it the rest is left out as it opens.
pub const MAX_FILES: usize = 4096;
/// The most the pictures of one design take together, in bytes.
pub const MAX_TOTAL_BYTES: u64 = 1 << 30;

/// Whether `name` is a picture's entry name: 32 hexadecimal digits and `.png` or `.jpg`, so it can
/// never name a part outside its folder.
pub fn valid_name(name: &str) -> bool {
    name.strip_suffix(".png").or_else(|| name.strip_suffix(".jpg")).is_some_and(|g| g.len() == 32 && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The kind of picture `bytes` hold by their first bytes: `png` or `jpg` (`None`: neither).
pub fn kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else {
        None
    }
}

/// A design's JSON never holds the pictures: whatever a hand-edited file has there is left out.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, Blob>, D::Error> {
    serde::de::IgnoredAny::deserialize(deserializer)?;
    Ok(BTreeMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_kinds() {
        let guid = "0123456789abcdef0123456789ABCDEF";
        assert!(valid_name(&format!("{guid}.png")));
        assert!(valid_name(&format!("{guid}.jpg")));
        for bad in ["x.png", "../x.png", &format!("{guid}.gif"), &format!("a/{guid}.png"), ""] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert_eq!(kind(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]), Some("png"));
        assert_eq!(kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(kind(b"GIF89a"), None);
    }
}
