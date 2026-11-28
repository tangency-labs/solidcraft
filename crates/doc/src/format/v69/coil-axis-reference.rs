//! Coils save an optional axis reference and a separate reversed propagation flag. A picked edge
//! stores the definition body, occurrence path, persistent name and a point in the body's frame.
//! When a placed component was active, the reference also saves its authoring occurrence.
//! Older designs
//! keep their numeric axis and forward propagation. The last release warns about the newer
//! format, opens what it can read, and preserves unknown fields when saving.
