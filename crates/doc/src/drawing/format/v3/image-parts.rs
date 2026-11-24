//! A drawing's image files are saved as parts of its container of their own: each image record
//! names its part (`part`, `blobs/<sha256>.<png|jpeg>`) and keeps an empty `data`, where the bytes
//! were base64 before. A drawing that has images keeps their records, their placements and its
//! logos in top-level fields of their own (`image_files`, `placed_images`, `logo_cells`), and its
//! logo cells keep only their text. An older drawing has them in `images` and `sheet_images`, the
//! bytes in `data`, and reads as before; nothing is rewritten as it opens. The last release (0.45.0)
//! warns about the newer drawing format, sets those fields aside and opens the drawing without its
//! images (a logo cell shows its text), and writes them back as they came when it saves; the
//! design names the parts (`drawing_parts`, `crates/doc/src/format/pending/drawing-parts.rs`), so its
//! save keeps them.
