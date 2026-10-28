# Pending drawing-format changes

One file per change to what a drawing saves (in its design's file) that no release has shipped
yet. Drawings (`"format": "solidcraft-drawing/N"`, #3890) are versioned by the same code as designs
(`crates/doc/src/format.rs`, `Family`), from their own first format, 1. Adding a file here is the
whole format bump: never edit the `DRAWING` family in `format.rs`, another change's file or a
`../v<N>/` folder.

- Name it after the change, lowercase with dashes: `sheet-views.rs`.
- Start it with a `//!` note: what is saved, what an older drawing gets without it, and what
  the last release does with a drawing that has it (it refuses the newer format number).
- If older drawings must be rewritten as they open, add the step to the same file:

  ```rust
  //! …the note…
  use serde_json::Value;

  pub fn step(v: &mut Value) -> crate::Result<()> {
      // rewrite the drawing's JSON here
      Ok(())
  }
  ```

  Steps run in file-name order, after the steps of every older format.

While this folder has a change in it, the build writes the next drawing format number (the last
folded one + 1, which no release reads), and lists the changes a drawing has in its
`format_pending` field, so a later build applies only the steps that drawing is missing. At
release time `scripts/fold-format-pending.sh` moves these files into `../v<N>/`, as it does the
design format's (`crates/doc/src/format/pending/README.md`).
