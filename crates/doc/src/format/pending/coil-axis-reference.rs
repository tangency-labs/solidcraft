//! Coils save an optional axis reference and a separate reversed propagation flag. Older designs
//! keep their numeric axis and forward propagation. The last release warns about the newer
//! format, opens what it can read, and preserves unknown fields when saving.
