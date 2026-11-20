//! Binary data a design keeps (plugin files, and later images, point clouds and render
//! resources): the bytes themselves, shared, never text. A container writes them to a part of
//! their own and reads them back as they are.
//!
//! Designs from before this kept such data as base64 text; reading still takes that text and
//! decodes it once, as the design opens. Nothing writes it again.
use std::sync::Arc;

use serde::{Deserialize, Deserializer};

/// Bytes shared between the copies of a document (every edit clones it for undo), so a large
/// file is held once however many undo steps keep it.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct Blob(Arc<[u8]>);

impl Blob {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::ops::Deref for Blob {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Blob {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for Blob {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes.into())
    }
}

impl From<&[u8]> for Blob {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.into())
    }
}

impl<const N: usize> From<&[u8; N]> for Blob {
    fn from(bytes: &[u8; N]) -> Self {
        Self(bytes.as_slice().into())
    }
}

impl std::fmt::Debug for Blob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Blob({} bytes)", self.0.len())
    }
}

/// Reads the base64 text an older design kept; `None` when it is not base64.
pub fn from_legacy_text(text: &str) -> Option<Blob> {
    crate::canvas::base64_decode(text).map(Blob::from)
}

/// An older design's base64 text, decoded as it is read. Text that is not base64 is an error.
impl<'de> Deserialize<'de> for Blob {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        from_legacy_text(&text).ok_or_else(|| serde::de::Error::custom("binary data is not base64"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_text_decodes_once_and_copies_share_the_bytes() {
        let blob: Blob = serde_json::from_str("\"c3RhdGU=\"").unwrap();
        assert_eq!(blob.as_bytes(), b"state");
        let copy = blob.clone();
        assert!(std::ptr::eq(blob.as_bytes(), copy.as_bytes()), "a copy shares the bytes");
        assert!(serde_json::from_str::<Blob>("\"not base64!\"").is_err());
        assert_eq!(format!("{blob:?}"), "Blob(5 bytes)");
    }
}
