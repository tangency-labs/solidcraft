//! Lists the format changes of each family of files versioned by `src/format.rs`: designs in
//! `src/format/`, drawings in `src/drawing/format/` (#3890). One file per change, in `v<N>/` once a
//! release fixed its number, else in `pending/` (see `src/format/pending/README.md`). The lists are
//! generated (`solidcraft_format::build`) so that a change adds a file and never edits a line
//! another change also edits.

use std::path::PathBuf;

use solidcraft_format::build::Family;

/// Every family, each read by `scripts/fold-format-pending.sh` too. A design's upgrade steps are
/// written out in `format.rs` up to 22 (`JSON_STEPS`); a drawing's first format is 1.
const FAMILIES: [Family; 2] =
    [Family { folder: "src/format", base: 22, list_prefix: "" }, Family { folder: "src/drawing/format", base: 1, list_prefix: "DRAWING_" }];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let lists = solidcraft_format::build::lists(&manifest, &FAMILIES).unwrap_or_else(|e| panic!("{e}"));
    for folder in &lists.watch {
        println!("cargo:rerun-if-changed={}", folder.display());
    }
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("format_parts.rs");
    std::fs::write(dest, lists.code).unwrap();
}
