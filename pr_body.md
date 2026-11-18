## What changed

A sweep's path may now join the curves of **several sketches** (#16168): a curve picked in a
sketch other than the path's own is saved as `{"sketch": <sketch>, "curve": <id>}` in the
feature's `path`, and is taken where it is drawn — mapped from its own sketch's plane into the
feature's frame — instead of being looked for (and refused) in the path's sketch.

Changes a default: no

(The default path is still the single-sketch curve list; nothing a user could do before now
resolves differently.)

- **Format** (`crates/doc/src/document.rs`): `path` is now a list of `PathEntry` — a plain curve
  id (the path's own sketch, as before) or `{sketch, curve}`. Older designs read unchanged.
  `input_sketches()` reports every entry's sketch, so features that follow a sketch follow a
  cross-sketch path correctly.
- **Evaluation** (`crates/doc/src/eval.rs`): `path_curves` resolves each entry in its own sketch;
  the chain helpers (`path_points`, `path_segments_at`, `chain_order`) now work on world-space
  ends, so curves of different planes chain together, and the chain is put in order from the end
  nearest the profile (also when the picks are listed out of order, #16168's pick-order half).
  A closed path across sketches is followed as listed (the profile-plane split applies to a
  single plane).
- **Commands** (`crates/engine/src/cmd/features.rs`, `surface.rs`): `solid.sweep`/`solid.pipe`/
  `solid.coil`/pattern-on-path path entries accept `{"sketch": ..., "curve": ...}` and a bare
  string still works as one curve of the path's sketch; a `surface.sweep` path is still the
  curves of one sketch and refuses a cross-sketch entry.
- **UI** (`crates/ui-egui/src/dialogs.rs`): the Sweep dialog resolves each path pick's sketch one
  pick at a time instead of requiring all picks in one sketch, so a chain drawn across sketches
  can be picked; editing a feature restores each pick on its own sketch.

## Why

In Fusion 360 a continuous 3D path split across two sketches sweeps fine; here the sweep refused
with "path curve `…`" because the second curve was looked for in the path's sketch.

## Reviewer checks

- `sweep_path_across_sketches` in `crates/engine/src/cmd/sweep_options_tests.rs` sweeps a
  profile across an XZ-sketch line and its continuation on YZ, listed in the reverse pick
  order, and checks the volume (40 · 100π) and the same volume after a save and reopen.
- Note: I could not run the test suite in this sandbox — Windows Application Control blocked
  build scripts and proc-macro DLLs (os error 4551), and the engine test binary runs rustc-LLVM
  out of memory when linking. `cargo check --all-targets` passes for `doc`, `engine` and
  `ui-egui`; please let CI run the tests.

Closes #16168

_This pull request was opened by an AI agent (OpenHands)._
