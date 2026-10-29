---
name: solidcraft-file-formats
description: "Use when a SolidCraft change saves something new in a design (a field, a feature type, a kind of name) or changes what older designs mean, and when touching STEP, IGES, 3MF, STL, OBJ, DXF or URDF import and export."
---

# Design file format, import and export

## Design file format changes
A design says which format it is in (`"format": "solidcraft/N"`). A change that saves something new, or makes older designs mean something else, adds ONE file and edits nothing shared:

1. Create `crates/doc/src/format/pending/<what-it-is>.rs` (lowercase, dashes). Start it with a `//!` note: what is saved, what older designs get without it, and that the last release asks for an update instead of meeting it.
2. If older designs must be rewritten as they open, put `pub fn step(v: &mut Value) -> crate::Result<()>` in the same file. Pending steps run in file-name order, after the older formats' steps.
3. Never touch `FORMAT`, `JSON_STEPS`, the history in `crates/doc/src/format.rs`, another change's file, or any `v<N>/` folder. The build counts the pending files and writes the next number, so a release refuses a design that carries your change instead of losing it.

Two PRs that each add a change merge without a conflict. `crates/doc/src/format/pending/README.md` has an example. At release time the maintainer runs `scripts/fold-format-pending.sh`, which moves pending files into `v<N>/` for good; `scripts/test-format-pending.sh` runs in CI. Never add a file to a `v<N>/` folder afterwards. See `docs/releasing.md`.

Drawings follow the same rules with their own numbers, from 1: a drawing-format change is ONE new file in `crates/doc/src/drawing/format/pending/` (see its `README.md`), and the same fold script moves them into `crates/doc/src/drawing/format/v<N>/`.

## Import
- Every import (STEP, IGES, 3MF, STL, OBJ; inserted, dropped, opened, or run as `file.insert_step`, `file.insert_mesh` or `doc.open`) becomes a new component named after the file, with the Import feature inside it. Bodies never land loose.
- Imported files are untrusted: sizes, entity counts and nesting are capped, and every kernel call is guarded (`skills/solidcraft-crash-safety/SKILL.md`). What cannot be read becomes a warning on the feature.
- STEP is read by our own reader in `crates/kernel/src/step_in`, written from the public standards, with no third-party STEP code. The reader and what it supports: `docs/step-import.md`.
- Imported assemblies are rigid by default; Unlock Contents releases them.
- Kernel work (booleans, STEP/IGES import and export, tessellation) is paused except crashes and data loss: CONTRIBUTING.md, "Kernel work is paused".

## Export
- STEP, 3MF, STL, OBJ and URDF go through `file.export` and `file.save_mesh`; the MCP `export` tool calls them. URDF: `docs/urdf.md`. 3MF face tags: `docs/formats/3mf-face-tags.md`.
- Mesh and design readers and writers live in `crates/io/src/`. Write files through `crates/io/src/safe_write.rs`.
- Reference tests for exports never take their expected values from SolidCraft's own output (`skills/solidcraft-tests/SKILL.md`).
