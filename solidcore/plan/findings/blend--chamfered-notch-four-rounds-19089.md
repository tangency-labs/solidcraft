# blend: chamfered-notch rounds need a different corner construction (2026-11-18)

Issue: tangency-labs/solidcraft#19089. Units: `blend/fillet_corner`,
`blend/fillet_executive`, `blend/fillet_geom`. No fix or golden change is claimed.

## Synthetic reproduction

The reporter's design is unnecessary. Save this as `notch.json` and run
`solidcraft-cli run notch.json --out notch.sc`:

```json
{"commands":[
  {"command":"solid.box","params":{"corner":[-10,-10,0],"length":20,"width":20,"height":20}},
  {"command":"solid.box","params":{"corner":[0,0,0],"length":10,"width":10,"height":10,"operation":"cut"}},
  {"command":"solid.chamfer","params":{"body":"Body1","edges":[[-5,10,0]],"distance":10}},
  {"command":"solid.fillet","params":{"body":"Body1","edges":[[0,0,5]],"radius":6,"type":"chord"}}
]}
```

Then save this as `rounds.json` and run
`solidcraft-cli run rounds.json --in notch.sc`:

```json
{"commands":[
  {"command":"solid.fillet","params":{"body":"Body1","edges":[[0,7.121320343559643,7.121320343559642],[1.0238843964525015,1.5119364195653076,1.5119364195653067],[7.121320343559643,0,0],[-2.878679656440358,0,0]],"radius":"2 mm"}}
]}
```

The installed 0.44.1 CLI builds the first script and rejects the second with
status 1001, `the fillet body is not built: Fillet Corner Error: Unable to Make
Topology`, as in the report. This checks all four edges together, not each alone.
A snapshot of the synthetic input was captured and inspected. A failed operation
has no after-body; displaying the unchanged input is not evidence of success.

## First observed source failure

A temporary test in the existing `crates/solidcore-ops/tests/blend.rs`,
`blend::rounds_meet_a_chord_round_at_a_chamfered_notch`, built these same boxes
with `create_box` and `boolean_difference`, then applied `Blend::Chamfer` (10)
and `Blend::Chord` (6). A `FilletSession` diagnostic attempted each corner's
topology before the final four-edge round. The first corner, `NByOne` at
approximately `(0,10,10)`, failed with:

```text
operation: blend::fillet_geom::p5::FilletBrep::calc_edge_on_extended_edge
reason: an end vertex of the edge is not processed
origin: crates/solidcore/src/blend/fillet_geom/p5.rs:1775
```

`fillet_executive/session.rs:6603` calls `nx1_end_gap_checks` during topology
creation. That helper attempts cap side-edge geometry. However,
`fillet_geom/p4.rs:1960-1963` deliberately leaves the rail/end-surface vertex
unprocessed until the rail and fillet surface have been traced. The side-edge
calculation requires processed endpoints. The outer executive reports the generic
corner-topology error (`fillet_executive/p3.rs:2990`).

Moving that check later was investigated, then removed: it cannot complete the
requested result because of the independent geometric obstruction below. The
temporary failing test was also removed rather than leaving a red test or
asserting that refusal is the desired result.

## The required legacy rail intersection does not exist

The chord-width-6 round between perpendicular walls has radius
`R = 6 / sqrt(2) = 3 sqrt(2)`. Its vertical cylinder has equation
`(x-R)^2 + (y-R)^2 = R^2`. At `B = (R,0,0)`, three selected edges meet an
unselected vertical tangent seam. The relevant supports are the bottom `z=0`,
the chamfer `y=z`, and the notch wall `y=0`, with material on the sides
`z >= 0`, `y <= z`, and `y <= 0` respectively.

For any positive final round radius `r`, a ball touching the bottom has centre
height `z_c = r`. Touching the chamfer on its material side requires

```text
(z_c - y_c) / sqrt(2) = r
y_c = -(sqrt(2) - 1) r
```

Touching the wall instead requires `y_c = -r`. Consequently the two bottom-face
rails are straight lines parallel to x, with separation `(2-sqrt(2)) r`.
At the reported `r=2`, this is `4-2 sqrt(2)` mm, strictly positive. They cannot
share a rail-intersection vertex. This is an exact obstruction for this
construction, not a convergence tolerance or a radius-specific near miss.

`fillet_corner/p3.rs:1332-1333` selects `FourByThree` for three selected edges
in a four-edge ring. `fillet_corner/topology/end_corners.rs:179-188` makes
the shared face vertex a `RailXRail` using the two adjacent solvers' rails.
`fillet_geom/p2.rs:2421-2441` requires their actual intersection and rejects
absent intersections with `the two rails do not meet`.

Existing machinery does not provide a route around this vertex:

- `fillet_executive/p3.rs:4311` creates corner vertices before solver geometry
  and rail classification; a failed vertex exits before later tangent recovery.
- `trim_at_creases` needs a completed intermediate sheet
  (`fillet_executive/session/finish.rs:1141`).
- The Roll-over mixed-ring classifier requires exactly two selected edges
  (`blend_corner/mixed.rs:219`); this corner has three.
- Ball and Setback dispatch excludes `FourByThree`
  (`fillet_executive/session.rs:2251,2311`).
- Ops overflow reordering requires the classified `OverflowFirst` event
  (`solidcore-ops/src/blend.rs:929`), not a failed corner vertex.
- The existing concave-chain rollover route needs a cylinder end arc in a plane
  normal to the cylinder axis (`blend_corner/rollover.rs:417-429`).
  The chamfer's normal `(0,1,-1)` is not parallel to the vertical cylinder axis.

A correct implementation needs a general corner/support-change construction
and its compatible trimming and topology. A bottom/cylinder toroidal piece is a
possible component, not a proved complete solution. Neither postponing the first
check, reducing the radius, retries, nor relaxing tolerances supplies that
construction. Maintainer design work is needed before claiming a fix.

## Related reports and validation

The shared message alone does not establish a common cause for #17600, #18424,
#16948, or #17379. GitHub API access failed during this investigation, so their
inputs could not be compared. Locally, the design from #16948 is associated with
the distinct short-revolution readback defect #17004; see
`surface.implicit--short-sweep-revolution-centre.md`. Do not close those reports
on this evidence.

The temporary diagnostic test was run before the experimental scheduling change:

```text
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --test ops blend::rounds_meet_a_chord_round_at_a_chamfered_notch -- --exact --nocapture
FAILED: 0 passed, 1 failed, 457 filtered out; exit 101.
```

Buildq log: `1795012757.966590-c5f51f2f`. Follow-up targeted test and clippy
attempts failed before compiling on the XPS worker with `Disk quota exceeded`
(exit 2; jobs `1795013408.176095-437e1e8f` and
`1795012998.661402-2df9f1eb`). Local-only retries were cancelled while still
queued after all experimental Rust changes were removed; they supply no
validation result. Root and solidcore `cargo fmt --all` passed through buildq.
Only this findings record and the owner's commit-message file remain changed;
there is no changed Rust crate, retained regression test, golden or design format.

Other validation commands (all run from the worktree root):

```text
buildq run solidcraft . -- cargo clippy --manifest-path solidcore/Cargo.toml -p solidcore-ops --test ops -- -D warnings
exit 2: XPS disk quota, before compilation.

buildq run solidcraft . -- cargo fmt --all
exit 0.

buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all
exit 0.

buildq run solidcraft . -- cargo clippy --manifest-path /home/ray/builds/worktrees/lane-19089/solidcore/Cargo.toml -p solidcore -p solidcore-ops --all-targets -- -D warnings
exit 130: cancelled while queued after removing experimental Rust changes.

buildq run solidcraft . -- cargo test --manifest-path /home/ray/builds/worktrees/lane-19089/solidcore/Cargo.toml -p solidcore-ops --test ops blend:: -- --nocapture
exit 130: cancelled while queued after removing experimental Rust changes.

git diff --check
exit 0.

git diff --exit-code -- solidcore/crates
exit 0: no retained Rust changes.
```
