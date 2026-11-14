# Mesher bake-off harness

Meshes one corpus of bodies with one or more SolidCraft commits and grades every mesh: bodies
meshed, failures, faces left without triangles, triangles, deviation from the surface against the
chord tolerance, open edges on closed bodies (true gaps and T-junctions), non-manifold edges,
sliver angles and time. The corpus is never committed: `fetch.sh` builds it at run time from where
each input lives (see its header).

The first run of it (2026-11-07) compared SolidCraft's mesher with solidcore's; it is repeated on
the combined mesher (#14003, #14029, #14033); the results are kept outside the repository.

## Steps

Each commit to measure is its own worktree with its own target folder (a shared one builds stale
code). Copy this folder into it first (an older commit has no `bench/tess/`).

1. `fetch.sh DEST`: the example designs and the 47 STEP files as kernel-case folders in `DEST/cases`.
2. Bodies: `git apply bench/tess/cli-dump.patch` on one commit (a local change to the CLI, never
   committed), build `solidcraft-cli`, then run it over `tests/kernel-cases/*/*/` and `DEST/cases/*/*/`
   with `MESHBAKE_DUMP=DUMPS`: every body each case ends with is written to `DUMPS/<area>__<case>__<n>.nb`.
   Every commit is fed the same `.nb` files. `mkprims` (in `src/bin`) writes the six primitives.
3. `build.sh`: builds `meshbake` and `mkprims` in release against the tree it sits in.
4. `run.sh LIST DUMPS OUT LABEL=BIN[:ONLY]...`: one process per body, setting and mesher, strictly one after
   another (body by body, the meshers in turn, so each meets the same load). `ONLY` is `solidcraft`
   (`Body::mesh_plan`, the viewport and export path) or `solidcore-tess` (`solidcore_tess::plan`).
   For the 2026-11-09 run: `sc-old=OLD/meshbake:solidcraft core-old=OLD/meshbake:solidcore-tess combined=NEW/meshbake:solidcraft`.
5. `agg.py RUNS LIST results.md`: the tables. `RUNS` holds `sc-old/`, `core-old/` and `combined/`.

Tolerances as the app asks for them: display is `MeshTolerance::display_of(size)` (chord
diagonal/1000 and a 20 degree facet angle), export is `export_of(size)` (diagonal/5000, chord only).

Deviation is sampled (up to 60 triangles per face, at the centroid, edge midpoints and interior
points, projected to the face's exact surface with `solidcore::eval::SurfaceEval::project`), so a
miss between samples is not seen. Vertices are welded at 1e-7 of the body's diagonal; a true gap is an
open edge whose midpoint is not within 1e-6 of the diagonal of another open edge.
