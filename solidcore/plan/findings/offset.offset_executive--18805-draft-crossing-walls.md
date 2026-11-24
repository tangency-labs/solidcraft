# Drafting a cylinder beside pads or blind holes

Issue: SolidCraft #18805. Inputs are S1767 and S8075 of the public,
Apache-2.0 `dutchman-cad/stress-corpus` (`items.jsonl`). The local corpus
copy's `results/batch-81.jsonl` records both failures on main 374359bed.
Only the cylinder's outer side is drafted in those runs. The recipes'
final rigid rotations/translation happen after the failing operation.

## S1767: the reported missing underside and a source-input blocker

Input: cylinder radius 1.5, height 4, base at the origin; union boxes
with corners (1.02, -0.18, 4), size (0.36, 0.36, 0.36), and
(1.02, 0.18, 4), size (0.36, 0.36, 0.36002). Draft the cylinder wall
inward by 3 degrees about z = 0.

Put t = tan(3 degrees). The correct cylinder wall is r(z) = 1.5 - z t.
Its top radius is 1.2903688828678352. Its top meets the two outer pad
sides at (sqrt(r(4)^2 - y^2), y, 4), y = -0.18 or 0.54: x is
1.2777526575490208 or 1.1719436223102138, respectively. Both pads keep
their full volumes above z = 4; the footprint corners lie inside the
original cylinder. The newly exposed underside closes the overhang.
The volume is therefore

    pi * 4 * (1.5^2 + 1.5*r(4) + r(4)^2) / 3
      + 0.36^2 * (0.36 + 0.36002)
    = 24.600272254413394.

A candidate cause of the installed binary's refusal is underside selection;
the current source reproduction does not reach that construction (see
the source results below).

`offset_executive/crossing_loops.rs::floor_is_cavity` selects a witness
by averaging boundary stretch midpoints, then moves it by 0.001 times
the executive's offset distance. For a local operation that distance is
half the bounding-sphere radius, not a geometric wall thickness
(`topology/brep/local_operation.rs`). It tests the witness against every
other offset surface whose perpendicular foot lies inside its trims.

This is not a certificate for an exposed underside. Write the witness
as q = (r, 4 + s) in a radial section of the cone and let R = r(4).
Its perpendicular foot on the cone has height

    z_foot = 4 + (s - t*(r - R)) / (1 + t^2).

When r - R > s/t the foot lies below the top, inside the cone's trims.
The witness is outside the cone, so the cavity rule rejects it, although
that is precisely the material side of the required overhang underside.
The result can lose its closing face and fail the final manifold gate.
Changing the witness distance or nudging its position would not fix the
construction.

A blanket rule retaining every reversed floor for a local operation is
also insufficient. Add an otherwise disjoint blind hole at (-1.4, 0),
radius 0.05, z = 3.8 to 4. Its nearest radius is 1.35, beyond the drafted
wall throughout that interval, so the hole should disappear. Its whole
loop outside the new rim is also classified as a floor, but none of its
edges participates in an opposite-side crossing. A general selection
must distinguish that detached cavity from the pads' material and
maintain the corresponding wall topology. The installed CLI control with
one centered 0.36 by 0.72 pad, with and without that shallow hole, returns
the same closed result: nine faces and volume 24.6002696624134. The
closed-form frustum plus that pad is 24.600269662413396. Thus indiscriminately
keeping every negative loop would jeopardize a currently valid case.

## S8075: walls that must intersect through their interiors

Input: cylinder radius 600, height 2400; subtract three blind cylinders
of radius 60 from z = 1920 to 2400 at x = -480 and y = -180, 0, 180.
Draft only the outer cylinder wall inward by 3 degrees about z = 0.

The desired wall is r(z) = 600 - z t. Its radii are
499.37706377656093 at the hole floors and 474.22132972070114 at the top.
Hole-center distances from the axis are d = 480 and sqrt(262800).
For every z in the hole depth, d - 60 < r(z) < d + 60 for all three
holes. Thus each hole intersects the cone throughout that depth, not
only along the top rim. Each section's intersections obey

    cos(theta - phi) = (r(z)^2 + d^2 - 60^2) / (2*r(z)*d).

Six spatial cone/cylinder intersection branches, and trimmed hole-floor
disks, are required. Both walls leave the top plane downward. The
opposite-side test in `walls_part_at_crossings` deliberately rejects them;
`split_walls_at_crossings` only splits existing boundary edges and
cannot create those branches.

The local operation is unmerged. Removing the `Unmerged` refusal cannot
make these faces a solid. The prior record
`plan/upstream-issues/WR-345--addendum-2026-11-16-flag-removed-offset-resolve-crossing-loops.md`
documents the resulting crossing face loops or a stitch enclosing no
region. Merely enabling the merge would also enable excess-face
selection by the dummy offset distance. General local-operation
intersection trimming and region selection need a design; the
arrangement/cell-selection/extraction stages remain open in
`docs/offset-and-shell.md` section 5.1.

## Historical evidence and initial scope

The installed SolidCraft CLI 0.44.1 reproduces both reported refusal
messages. It also builds equivalent expected geometry by drafting the
plain cylinder first, then joining the pads or subtracting the holes.
Those reordered recipes are evidence of the desired geometry, not an
implementation proposal or a fallback. Before/expected screenshots were
captured and inspected in `/tmp/18805-S1767-{before,expected}.png` and
`/tmp/18805-S8075-{before,expected}.png`; review media is not committed.
The expected S1767 reference measures volume 24.60027225441339, consistent
with the independent closed form above. These reordered recipes establish
an observable target, not an expected value derived from the buggy call.
The installed binary does not establish the current source's results.

At this initial investigation stage, the existing ops draft test file
contained both minimal reproductions. No tolerance, validity gate,
production construction, golden or kernel-case expectation had changed.
The initial conclusion was that completing the issue required oriented
boundary selection and same-side wall-intersection construction, rather
than a recipe-specific Boolean sequence. The later exact construction
below supersedes this no-production-change scope. The literature and
construction references recorded in `docs/offset-and-shell.md` sections
5 and 11 were the starting point; external literature could not be
fetched because this environment could not resolve public hosts.

## Final construction and case status

The implemented first construction step selects a complete outward
cylindrical wall with fixed perpendicular end planes and a coaxial inward
draft whose affine radius satisfies 0 < r(z) <= R. For its axial slab
[z0, z1], it constructs the exact radial removal volume

    W = { (rho, theta, z): z0 <= z <= z1, r(z) <= rho <= R }

and takes the regularized difference B minus W. The Boolean creates the
required cone/hole-wall intersections through their interiors. This
construction preserves the draft definition, source attributes and final
validity gates. It uses no fallback or widened tolerance. The supporting
cap and translation corrections, construction limits and verification
are recorded in [Exact radial removal for a complete cylindrical draft](topology.brep_local_operation--18805-radial-sweep-step.md).

S8075 now passes its original regression,
`a_cylinder_with_three_blind_holes_drafts_through_the_hole_walls`, and its
ignore attribute has been removed. S1767 remains ignored: its joined
Boolean input fails the draft input-solid precondition with `Argument`.
The prior input diagnostic reported three bad loop orientations. The
underside-selection explanation above remains a hypothesis for the
installed binary; the current source input fails before drafting.

## Historical source verification

These commands and the two-ignored-tests status describe the initial
investigation, before the production construction and final focused
result linked above.

On worktree head b81b7feed, the targeted source run

    buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --lib offset::tests::drafts::a_cylinder_with_ -- --nocapture

exited 101: one passed, two failed. The existing single-boss control
`a_cylinder_with_a_boss_near_its_rim_drafts_under_the_boss` passed.
`a_cylinder_with_three_blind_holes_drafts_through_the_hole_walls` failed
with `OffsetRefused { face: Some(1), why: LoopsCross { crossing: Unmerged } }`,
reproducing S8075.

`a_cylinder_with_touching_stepped_bosses_drafts_under_both` failed with
`Argument` from `draft`. The parameters and selected face are valid;
`offset.rs::draft` tests `Expect::of(...).solid()` before constructing the
draft. The Boolean-built input does not pass that solid precondition.
This is an earlier blocker than the installed CLI's non-manifold result;
the suspected underside-selection cause is not confirmed on this source
input. The input construction has not been nudged or repaired to bypass
that precondition. At that stage, both unresolved recipe tests were
explicitly ignored with their issue and reason, following the existing
test file's pattern. S8075's later passing, active status supersedes that
two-ignored-tests conclusion; S1767's input blocker remains.

Crate clippy (`-p solidcore-ops --all-targets -- -D warnings`) passed, as
did formatting and format checks for both workspaces. All cargo commands
went through `buildq run solidcraft . --`; no full local suite ran.

The remainder of the affected test file passed (exit 0): 42 passed,
0 failed, 1 existing ignored test (`#17608`), 255 filtered out. Command:

    buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --lib offset::tests:: -- --skip a_cylinder_with_touching_stepped_bosses_drafts_under_both --skip a_cylinder_with_three_blind_holes_drafts_through_the_hole_walls

The two new reproductions were excluded from this run after their
failures had been captured separately above. Verification commands that
passed (each exit 0):

    buildq run solidcraft . -- cargo clippy --manifest-path solidcore/Cargo.toml -p solidcore-ops --all-targets -- -D warnings
    buildq run solidcraft . -- cargo fmt --all
    buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all
    buildq run solidcraft . -- cargo fmt --all --check
    buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all --check
    buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml -p solidcore-ops --check
    git diff --check

At that stage, the full suite, wasm, repository guards, and full goldens
remained for CI when the owner pushed. No commit, staging, push, or other
git metadata write had been performed during that investigation.
