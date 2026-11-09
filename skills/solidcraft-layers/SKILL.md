---
name: solidcraft-layers
description: "Use when deciding whether code goes in SolidCraft or in solidcore (the vendored kernel in solidcore/), and in which solidcore crate: geometry, a modelling workflow, kernel work, or a review comment about the layer split."
---

# Where code goes: SolidCraft or solidcore

The authority is `solidcore/docs/layers.md`; this is its short form. Every job lives in exactly one layer, and two implementations of one job are a bug.

## Decide
1. **Does it compute geometry?** Anything computed from coordinates: a point, vector, length, angle, curvature, curve or surface, a solve, an intersection, a fit, a mesh, a STEP parse. Then it is a kernel primitive in `solidcore/crates/solidcore`, with its own tests. Dot and cross products, normalising, square roots or trigonometry on coordinates count, however small.
2. **Is it a workflow recipe of kernel calls?** It picks entities, parameters and defaults, sequences kernel calls, compares what they return with tolerances and reports. Then it is `solidcore/crates/solidcore-ops`. Ops computes no geometric quantity of its own: when it needs one the kernel lacks, add a kernel primitive and call it.
3. **Is it state, selection, commands, settings, history or UI?** Then it is SolidCraft (`crates/`, `apps/`), which computes no geometry.

## What SolidCraft may use
- `solidcore-ops` (modelling), `solidcore-tess` (display meshes, polylines) and `solidcore-record` (geometry as data). CI enforces this seam through `cargo xtask layers` (#13498): solidcore's public API is listed in xtask/solidcore-public.txt, and the remaining direct uses in xtask/solidcore-seam.txt, a list that only shrinks.
- Before writing STEP, meshing, surface intersection, tangent chains or frames, look for solidcore's: a second implementation is flagged.

## Lists that only shrink
- `xtask/mesh-decisions.txt`: model decisions made from a mesh (`cargo xtask mesh-decisions`).
- `solidcore/plan/panic-sites.tsv`: panic-prone calls in solidcore (SolidCore checks in CI).

## Working in solidcore/
- Follow `solidcore/AGENTS.md` and the reference doc for the area (`solidcore/docs/`). A change there needs a code owner's review (`.github/CODEOWNERS`) and runs SolidCore checks (its tests, goldens, kernel rig, clippy, fmt, panic sites) in `.github/workflows/ci.yml`.
- New kernel cases go in `solidcore/crates/solidcore/tests/kernel_cases/`, never in SolidCraft (`cargo xtask lists`).
- Kernel work goes here: CONTRIBUTING.md, "Kernel work goes in solidcore/". Kernel-only PRs target the `kernel` branch.
- Issues and claims for solidcore are in this repository, like any other change (`skills/solidcraft-contributing/SKILL.md`): the vendored tree is mirrored to tangency-labs/solidcore by `.github/workflows/solidcore-mirror.yml`, so pull requests are not opened there.
