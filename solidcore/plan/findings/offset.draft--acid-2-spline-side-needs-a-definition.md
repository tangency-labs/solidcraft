# Acid-2 stage 4 needs a spline-side draft construction

Issue: #19183. Investigated 2026-11-18. Status: blocked on a new kernel feature;
no production code or expectations changed.

## Reproduction

The installed `solidcraft-cli --version` reports `solidcraft-cli 0.44.1`.
Running the issue's complete ten-command script with
`timeout 120 solidcraft-cli run /tmp/lane-19183-acid2.json --results` exits 1:

```text
error: step 10 (solid.draft): not supported yet: SMLib draft for this geometry
```

Removing the fillet and all four bosses preserves the failure. This complete
minimal script exits 1 at step 5 with the same error:

```json
[
  {"command":"sketch.create","params":{"plane":"XY","name":"Outline"}},
  {"command":"sketch.spline.fit_point","params":{"points":[[60,0],[42,30],[0,40],[-42,30],[-60,0],[-42,-28],[0,-35],[42,-28]],"periodic":true}},
  {"command":"sketch.finish","params":{}},
  {"command":"solid.extrude","params":{"sketch":"Outline","distance":25,"operation":"new","body_name":"Part"}},
  {"command":"solid.draft","params":{"faces":[[60,0,10]],"angle":-3,"neutral":"XY","body":"Part"}}
]
```

The control, a radius-10, height-20 cylinder followed by a 5-degree draft of
`[10,0,10]` about XY, exits 0. These executions use the installed release, not
a binary compiled from this worktree.

## Source evidence

- `crates/smlib/src/core/offset.rs`, `draft`: a selected cone is handled first;
  every remaining selected surface must be a plane (lines 612–613). The spline
  extrusion side is neither. This returns `Error::Unsupported` before the local
  operation receives any replacement surfaces.
- `solidcore/crates/solidcore/src/offset/draft.rs`, `draft_surface`: its only
  construction arms are `SurfaceKind::Cone` and `SurfaceKind::Plane`; the other
  kinds return `DraftRefusal::NotPlaneOrCone` (line 863). Switching the app to
  this API alone cannot add support.
- The same file's `DraftDefinition::derived` derives only an implicit plane or
  cone and refuses every other source kind (lines 739–742). `bound` is always
  zero. Its certification contract cannot represent the drafted spline side.
- `solidcore/docs/offset-and-shell.md`, section 6, documents those same two
  supported draft constructions. The existing spline-radius-round draft tests
  in `solidcore-ops/tests/issue_2878_spline_round_draft.rs` select the box's
  planar bottom and ends; they do not draft a curved spline support.

## Why this needs a feature

For a regular planar outline C(u), outward planar unit normal n(u), and upward
pull p, the outward 3-degree drafted side about the outline's plane is

```text
D(u,h) = C(u) + h p + h tan(3 degrees) n(u).
```

The neutral trace stays C, and each generator leans outward 3 degrees. For a
general spline, normalizing the tangent introduces its varying speed's square
root; this surface is generally neither a plane nor a cone and need not have
an exact rational B-spline representation. Moving a spline's control points
radially or scaling its sections does not implement this normal-offset law.

Correct support therefore needs a definition and certified representation for
the drafted spline surface, with evaluation, continuation, records and
recertification, then integration with the local operation's adjoining faces.
The rounded top and joined bosses must survive that operation. Replacing the
body with a tapered extrusion drops those features and does not solve the
reported operation. No such substitute, mesh decision, tolerance increase or
fallback was added. The independent expected volume and probes have not been
obtained or checked; none were invented or blessed.

## Validation limits and handoff

A temporary test named
`tests::acid_2_rounded_spline_side_with_bosses_drafts_outward` was placed beside
the cylinder draft test in `crates/engine/src/tests.rs`. Its buildq job never
started (over 50 jobs were waiting). It was cancelled and removed; no failing
test is left in the tree. The temporary source is retained locally at
`/tmp/lane-19183-regression-probe.rs`. A first submission omitted the `tests::`
prefix while using `--exact`; it too was cancelled before execution. Neither
submission constitutes a Rust test result.

No Rust crate remains changed, so crate tests, clippy and the kernel error
ratchets have no changed code to validate. The full suite remains for CI when
the missing construction is implemented. GitHub PR and issue searches failed
with `error connecting to api.github.com`; related active work could not be
checked. No claim, commit or push was made.

Checks of the retained changes:

- `buildq run solidcraft . -- cargo fmt --all --check`: exit 0.
- `buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all --check`:
  exit 0.
- `git diff --check`: exit 0.
- `git diff --exit-code -- crates/engine/src/tests.rs`: exit 0; the temporary
  test was removed completely.
