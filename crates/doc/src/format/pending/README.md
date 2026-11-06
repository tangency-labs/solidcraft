# Pending design-format changes

One file per change to what a design file saves that no release has shipped yet. Adding a file
here is the whole format bump: never edit `FORMAT`, `JSON_STEPS` or another change's file.

- Name it after the change, lowercase with dashes: `placed-plane-origins.rs`.
- Start it with a `//!` note: what is saved, what an older design gets without it, and what
  the last release does with a design that has it (it warns about the newer format number, opens what it can read and keeps the rest).
- If older designs must be rewritten as they open, add the step to the same file:

  ```rust
  //! …the note…
  use serde_json::Value;

  pub fn step(v: &mut Value) -> crate::Result<()> {
      // rewrite the design's JSON here
      Ok(())
  }
  ```

  Steps run in file-name order, after the steps of every older format.

While this folder has a change in it, the build writes the next format number (the last
folded one + 1, which no release reads), and lists the changes a file has in its
`format_pending` field, so a later build applies only the steps that file is missing. At release
time `scripts/fold-format-pending.sh` moves these files into `../v<N>/`, which fixes that number
for good. See `crates/doc/src/format.rs` and CONTRIBUTING.md.
