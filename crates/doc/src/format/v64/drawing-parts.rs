//! A design whose drawings have images names the container parts those images are
//! (`drawing_parts`: `["blobs/<sha256>.png", ...]`) at the top of `document.json`. The drawings'
//! records name them too (`crates/doc/src/drawing/format/pending/image-parts.rs`); this list is for
//! a release that can't read those records (0.45.0): it keeps a part only when the design's own
//! metadata names it, so with this list its save keeps the images' bytes. This build writes the list
//! on every save and ignores what a file says. An older design has none and needs none. The last
//! release warns about the newer format, opens the design and keeps the field and the parts.
