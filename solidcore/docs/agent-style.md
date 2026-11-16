# Machine budget and code style

Moved from AGENTS.md (§2, §3) unchanged; AGENTS.md points here.

## Machine budget

8 cores / 16 threads shared by every agent.

```sh
export CARGO_BUILD_JOBS=2 RUSTC_WRAPPER=sccache      # sccache is installed in ~/.cargo/bin
nice -n 10 cargo test -p solidcore                     # every cargo command under nice
```

- Before claiming a unit, check load: `cut -d' ' -f1 /proc/loadavg`. If > 24, do not
  claim; finish or wait.
- After merging, delete your worktree's `bin/` (each worktree has its own cargo target).
- Unset `RUSTC_WRAPPER` if sccache is missing; the repo never requires it.

## Code style

**Numerics: ported code keeps upstream's, new code chooses for correctness.** Code that is
ported and unchanged keeps upstream numerics: same formulas, same operation order, same
iteration order, same branch structure for tolerance tests and degenerate cases. Do not
"simplify" `a*b + c` into `mul_add`, reorder sums, swap in `hypot`/`midpoint`, or replace
loops with library calls that change rounding there: the goldens record those bits, and
upstream's results came from matching order with no contraction. Code that fixes or replaces a
construction uses the numerics its correctness needs (exact predicates, interval enclosures,
certified rounding bounds) and says so in its doc comment. Everything else is Rust-first:

- **Data model.** Topology lives in `topology::Brep` arenas addressed by typed ids
  (`FaceId`, `EdgeId`, `VertexId`, `LoopId`, `ShellId`, `BrepId`); no pointers, `Rc`,
  or `RefCell` graphs. Geometry is concrete `enum`s (`Curve`, `Surface`) over structs,
  not trait objects. Points/vectors are `math::Vec3` (`Point3` is an alias, as upstream).
- **Kernel.** Tolerances, RNG and caches are fields of `kernel::Kernel` (upstream
  `SmContext`). Take `&Kernel` (or `&mut Kernel` when using rng/caches); it must stay
  `Send`.
- **APIs.** Return values, not out-params (tuples or small structs for multiple
  outputs). Fallible fns return `solidcore::Result<T>`; build `SolidError` with the
  `operation` path (e.g. `"nurbs::knots::insert"`) and entity id. Many optional
  arguments become an options struct with `Default`. Upstream `SmStatus` codes with no
  matching variant map to `SolidError::Upstream { code, .. }`.
- **Names.** Drop `NL_`/`Sm`/`SM_`/`smgu_`/Hungarian prefixes; snake_case fns,
  CamelCase types (`SmBSplineCurve` -> `BSplineCurve`, `NL_KnotInsert` -> `knot_insert`).
- **Organisation.** A unit's `rust_module` (`nurbs::knots`) lives at
  `crates/solidcore/src/nurbs/knots.rs`, declared with `pub mod knots;` in `nurbs.rs`
  (no `mod.rs`). Keep `mod` lines sorted, one per line, so parallel merges conflict
  cleanly. Unit tests go in the same file (`#[cfg(test)] mod tests`).
- **Docs.** Every public item has a doc comment (`missing_docs` warns; CI uses
  `-D warnings`). Lints are in root `Cargo.toml` `[workspace.lints]`; do not add
  `#[allow]` for denied lints in library code.

### File header

Every ported file starts with:

```rust
//! One-line summary of what the module does.
//!
//! Upstream: `source/SMLib/src/NL_KnotVec.cpp`, `source/SMLib/inc/NL_KnotVec.h`.
//!
//! | Rust                    | Upstream                         |
//! |-------------------------|----------------------------------|
//! | [`KnotVector`]          | `NL_KNOTVECTOR`                  |
//! | [`KnotVector::insert`]  | `NL_KnotInsert`                  |
//! | `find_span` (private)   | `nl_FindSpan`                    |
//!
//! Deviations: <anything not 1:1, e.g. out-param -> return value, rand() -> kernel.rng>.
```

## Figures

Schematics are drawn in VectorCraft, never by hand-writing SVG. Each figure is exported to its
doc's folder as SVG with live text (`outlineText: false`), so its labels stay readable in the
file, its `.vectorcraft` source kept next to it, and embedded with a Markdown image link and a
one-line caption that says what to look at. A figure's alt text says what it shows; when it has
arrows, the alt text states the flow step by step, since the file records no connection between
an arrow and its boxes. Captions name
things, never colours. When several agents share one VectorCraft instance, draw each figure in
one `command_batch` (new document, drawing, save, export), so nothing interleaves.

The palette is the one `docs/generic-blend.md`'s figures use:

| What | Colour |
|---|---|
| the subject (the definition S, the true curve or surface) | accent blue `#1f6fb2`, 3 pt |
| nets, control points, marched points, text | black `#000000` |
| tolerance bands, enclosures | pale blue `#dce9f5` |
| construction (frames, normals, radii, axes) | dashed grey `#999999`, 1.5 pt |
| secondary lines, a proved or start point | dark grey `#555555` |
| surfaces, faces | pale grey `#f0f0f0` |

A white background over the whole artboard; one font, Helvetica (titles 20 pt bold, labels 14
pt, notes 12 pt); every label beside what it names, with a thin leader line when it's farther
away, and no text over a line or other text. Check each figure with a render before exporting.

