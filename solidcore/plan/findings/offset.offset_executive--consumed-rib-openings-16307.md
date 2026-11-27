# Consumed rib openings and shared cylindrical caps (#16307)

## Candidate extension, not implemented

A surviving planar shoulder can remain selected while its neighboring rib loses
its cavity. The opposing rib walls still move. The exact shoulder cut and the
persistent-wall containment check still apply, and the surviving opening is
remapped to its unique oriented plane with zero displacement.

A persistent wall must have an actual boundary curve below the shoulder. An
entire nonempty positive rational control hull strictly below the shoulder proves
that such a curve exists. A hull minimum alone does not prove this. The bound
includes arithmetic rounding and the edge and face zone tolerances. Opposite
sides of a square boss end at the shoulder and do not qualify. The existing
half-width boss refusal remains the required behavior. No production change is
included: this conservative extension does not complete the shared-cap case below.

## Planar shoulder reproduction

The existing chamfered-rib fixture sweeps the xz outline
`(0,0), (20,0), (20,14), (1,14), (1,20), (0,19)` through 20 mm along y.
Its volume is `20*(280+5+1/2) = 5710 mm3`. Open the shoulder at z14.
For `t >= 0.5`, the cavity is `(20-2t)^2*(14-t)`: its top remains z14,
its bottom moves to z=t, and both opposing side pairs move by t.
Expected shell volumes are 836.5, 1498 and 2638 mm3 at walls 0.5, 1 and 2 mm.

The diagnostic `shell_opens_the_shoulder_beside_a_consumed_rib` regression failed
at t=0.5 with `WallsMeetAcross { distance: 0.5, largest: 0.5 }`.
The failed run was buildq job `1795020471.106052-494ab8c2`, exit 101.
An independent exact box-cut reference on the installed 0.44.1 CLI measures
836.4999999999995 mm3, closed, with no validity notes. This reference establishes
the expected result; it does not validate shell in the worktree.

## Remaining shared-cap construction

A small synthetic isolates the partial-face event:

1. Make a 20 x 20 x 20 box at the origin.
2. Round the x0/z20 edge along y with radius 2.
3. Cut x>=1, y<=10, z>=14 (tool corner `(1,-1,14)`, size `20,11,8`).
4. Cut x>=2, y>=10, z>=14 (tool corner `(2,10,14)`, size `20,11,8`).
5. Shell 0.5 inward, opening z0.

One cylindrical face caps the adjacent 1 mm and 2 mm ribs. The first rib loses its
cavity at t=0.5 while the second retains one. Whole-face removal cannot represent
this event. The installed 0.44.1 CLI refuses with an uncertified loop gap.

The base contributes 5600 mm3. Above z14, the narrow half contributes
`10*(4 + 2*pi/3 - sqrt(3)/2)` and the wide half contributes `10*(8+pi)`.
Thus the input volume is `5720 + 10*(5*pi/3 - sqrt(3)/2)` =
5763.699623521986 mm3, matching independent CLI measurement.

The base cavity contributes `19*19*13.5`. Only y10.5..19.5 of the wider rib
retains cavity. Its cap has radius 1.5 about `(x,z)=(2,18)`; above z13.5 its
cross section is
`4.5 + integral(x=0.5..1.5, sqrt(2.25-(x-2)^2))`.
The integral is `9*pi/16 - sqrt(2)/4 - 9*asin(1/3)/8`.
Subtracting this cavity gives 840.418139936750 mm3.

A slab-bounded cut alone is insufficient: cutting the same slab through the
wider neighboring rib exposes an artificial wall and shrinks that rib's cavity.
A correct extension needs an exact partial-patch/cell certificate proving which
parts cancel and that every newly exposed constraint preserves the retained
cavity. The current whole-face hull and shoulder-cut certificates do not prove
that. The general arrangement and cavity rule remain open in
`docs/offset-and-shell.md`, section 5.1.

The reporter's original design/replay is absent from the supplied issue text and
was not found locally. GitHub API access fails in this environment. Therefore the
reporter's 0.5 mm and rib-touching openings remain unverified, and the draft's
remaining limitation cannot be removed on this evidence.

## Validation and artifacts

The Rust regression was inserted temporarily in the existing shell construction
test file and run through buildq. The command below is a historical transcript of
that run: the diagnostic test (`shell_opens_the_shoulder_beside_a_consumed_rib`) is
not in this revision, so the filter matches no test here. To reproduce, add a test
that shells a solid with a rib beside a consumed rib at inset 0.5 with the shoulder
face open, and expect `WallsMeetAcross` from the offset:

```text
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore --profile goldens --test issue_2350_shell_offset_construction shell_opens_the_shoulder_beside_a_consumed_rib -- --nocapture
```

Result: exit 101; 0 passed, 1 failed, 8 filtered out. The reason is
`WallsMeetAcross { face: FaceId(3), distance: 0.5, largest: 0.5 }` from
`offset_executive/p2.rs:429`. The surviving selected shoulder is excluded by
`consumed.rs:406-410`, so source regularization is declined. The curved-neighbor
restriction at the same site and `clip.rs:203-204` cannot be removed without
replacing their geometric certificates.

The original Rust files were restored after withdrawing the incomplete candidate
implementation. The subsequent queued area-test and clippy clients were cancelled
before execution (exit 130); no Rust changes remain. No expectations were blessed.
No full suite or CI was run locally.

The diagnostic files of that run (the two diagnostic tests as a patch, the unverified
conservative extension as a patch, the planar and shared-cap reproduction scripts
and their reference results, and three synthetic views) lived only under a
temporary directory and were not kept; no observation in this record depends on them.

CLI reference evaluation and all three snapshots succeeded. The shared-cap shell
script failed at step 5 with an uncertified loop gap. The installed CLI is 0.44.1;
its results are separate from the worktree regression above. The 1 and 2 mm planar
shoulder scripts succeed on that installed version.

Final formatting checks both passed (exit 0), through buildq:

```text
buildq run solidcraft . -- cargo fmt --all --check
buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all --check
```

`git diff --exit-code` for both temporarily edited Rust files passed; both are
identical to HEAD. The new text files pass whitespace and commit-message checks.
Only this findings record and `COMMIT_MSG.txt` remain unstaged.
