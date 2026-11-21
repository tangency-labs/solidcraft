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

thread_local! {
    static READING: std::cell::RefCell<std::collections::BTreeMap<String, Blob>> = std::cell::RefCell::default();
    static WRITING: std::cell::RefCell<Option<Vec<(String, Blob)>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `read` (a record of a container being read) with the container's parts it names, by name:
/// a record that names a part takes its bytes from here, as they are.
pub fn with_parts<T>(parts: std::collections::BTreeMap<String, Blob>, read: impl FnOnce() -> T) -> T {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            READING.with(|held| held.borrow_mut().clear());
        }
    }
    READING.with(|held| *held.borrow_mut() = parts);
    let _clear = Clear;
    read()
}

/// The bytes of the part `name`, while a container's record is read ([`with_parts`]).
pub(crate) fn part(name: &str) -> Option<Blob> {
    READING.with(|held| held.borrow().get(name).cloned())
}

/// Runs `write` (a record being written into a container) and answers what it wrote with the parts
/// it names: their names, `blobs/<sha256>.<extension>`, and bytes, for the container to write.
pub fn collecting_parts<T>(write: impl FnOnce() -> T) -> (T, Vec<(String, Blob)>) {
    struct Stop;
    impl Drop for Stop {
        fn drop(&mut self) {
            WRITING.with(|held| *held.borrow_mut() = None);
        }
    }
    WRITING.with(|held| *held.borrow_mut() = Some(Vec::new()));
    let stop = Stop;
    let written = write();
    let parts = WRITING.with(|held| held.borrow_mut().take()).unwrap_or_default();
    drop(stop);
    (written, parts)
}

/// The name of the container part that holds `blob`: `blobs/<sha256>.<extension>`.
pub fn part_name(blob: &Blob, extension: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(blob.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("blobs/{hex}.{extension}")
}

/// The part name for `blob`, `blobs/<sha256>.<extension>`, recorded for the container while one is
/// written ([`collecting_parts`]). A record always names its part, so one read back without it is
/// refused by name rather than read as an empty file.
pub(crate) fn add_part(blob: &Blob, extension: &str) -> String {
    let name = part_name(blob, extension);
    WRITING.with(|held| {
        if let Some(parts) = held.borrow_mut().as_mut()
            && !parts.iter().any(|(known, _)| *known == name)
        {
            parts.push((name.clone(), blob.clone()));
        }
    });
    name
}

/// An older design's base64 text, as [`Blob`]'s own reading, except that text which is not base64
/// reads as no bytes: a damaged image never stops the design opening.
pub fn lenient<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Blob, D::Error> {
    let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
    Ok(from_legacy_text(&text).unwrap_or_default())
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
