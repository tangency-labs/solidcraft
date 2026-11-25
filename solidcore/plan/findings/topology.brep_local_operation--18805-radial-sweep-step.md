# Exact radial removal for a complete cylindrical draft

Issue: SolidCraft #18805, first construction step. This is an addendum to
`offset.offset_executive--18805-draft-crossing-walls.md`.

## Construction and scope

For a complete outward cylindrical wall of radius R over an axial slab
[z0, z1], replacing its support by a coaxial cone with positive affine
radius r(z) <= R removes exactly

    W = { (rho, theta, z): z0 <= z <= z1, r(z) <= rho <= R }.

The result is the regularized difference B minus W. This follows boundary
evaluation of regularized set operations (Requicha and Voelcker, as cited
in `docs/booleans.md`) and the set-operation offset construction of
Rossignac and Requicha (1986), cited in `docs/offset-and-shell.md`.
It does not reorder the user's features or reconstruct their holes.
The Boolean imprints W's conical boundary through every intervening face.

`contract_complete_cylinder` selects this event before the existing local
operation executes. It requires one selected side, one material region,
an outward bare cylindrical support, and a draft definition derived from
that support. Both the body surface and the draft's saved source must be
bare; an approximant's analytic kind alone does not prove its definition.
The existing exact `net_sense` polynomial proof combines with
the face-use orientation to prove physical radial outwardness; a raw
`Same` use alone does not establish it. An exactly zero semi-angle proves the cylindrical geometry;
the object need not carry the cylinder type flag. Boolean seam relocation
can re-recognize its joined net as a zero-angle cone
(`face/p1.rs`, `MoveSeam`, through `copy_and_add_analytics`). The tolerant
`is_cylinder` query does not establish this predicate.
The current step requires exactly equal source and target
frames, natural UV boxes, and axial intervals. It proves the actual UV
boundary is the four box sides using positive-weight control hulls and
exact endpoint equalities, and checks the face across each circular rim
is a bare plane (see "The body's rim planes" below). No sampled rectangle
or bounding box establishes this event. Expansion arithmetic proves the
affine radius positive and its displacement nonpositive at both axial
ends, hence throughout the slab.

`radial_sweep` copies both supplied surfaces, preserving the target's
DraftDefinition. At an end where the supports coincide, their common
circle closes the tool; otherwise two defining circles bound an annular
cap. There is one stitch at the input body's tolerance, with the stitcher's
full edge comparison (the fast sampled comparison is not enabled). The closed tool
must pass the outward-solid check before subtraction. The target wall
carries the selected source face's attributes, including the caller's
moved-face mark. The operation preserves rollback, body naming, and the
existing final check/manifold/positive-volume gate. It does not retry,
change a tolerance to gain acceptance, or switch constructions on error.

The strict endpoint radius proof supplies each annulus's outer circle and
its one inner circle directly to `make_face_with_curves`. Each rim is
defined from its own cone's endpoint height, stored placement and
`Cone::radius(height)`. The cone iso extractor averages sampled radii;
promoting that value into a fresh circle would not establish its identity
with the support. The direct circles must have exactly equal stored
centers and axes, positive circular radii in strict outer/inner order,
and inside-out flags false. The construction does not use
`find_outer_loops`, whose sampled containment cannot establish this
topology. Each direct circle makes its form from its
analytic ellipse definition, giving counterclockwise traversal in that
definition's frame. The cap keeps their exactly proved common plane and
certified curve-hull bounds; `fixed_rim_plane` also proves that plane is
the exact axial endpoint. A differently rounded circle plane is a typed
error, never accepted as an approximate fixed rim.

The two-cap control's shifted neutral frame is significant here: its
target origin is z = -10 and its local endpoint heights are 10 and 30.
Those give target radii 9 and 7 at world z = 0 and 20. The old cylinder's
local heights 0 and 20 must not replace the target's heights.

This first step is wired into the static local-operation entry point,
which owns its notification stores. The caller-notified executive still
needs a construction carrying its external identities through arrangement
and cell selection. Partial cylindrical faces, other supports, different
source/neutral frames, outward or mixed-sign edits, and multiple material
regions continue to use the existing construction.

## The body's rim planes

The selector does not compare the body's end planes with the cylinder.
The exact UV box already proves each rim is the cylinder's own iso circle
at its stored end height, and the tool's caps are built on exactly those
circles (`plane_on_ellipses` plus `fixed_rim_plane`, both local to this
construction). The face across a rim shares that edge in a body that the
standing validation accepts, so a bare plane there is the slab's end
plane up to the body's own validity; it stays the result's support and
the Boolean trims it by the cone. No tolerance is introduced: the
Boolean's coincident-face handling merges the tool cap with that face.

An earlier version demanded the body's end planes be bit-exact. On main
they are not: a primitive cylinder's top cap is fitted through
z = 20.000000000000004 with a y-axis z component of -2.28e-16, its iso
rims average projected sample heights, and the Boolean's temporary
translation refits planes from their rounded nets. Making those exact
meant changing `cone_iso_curve`, `Cone::transform`,
`Surface::create_plane_with_3d_curves` and `Plane::transform` for every
caller, which moved 270 golden rows in 17 units (primitives, offsets,
fillets, cones, planes, `topology.api_files`) and a sweep rig case. Those
shared routines are now exactly as on main; the exact plane construction
from ellipses is `plane_on_ellipses`, called only by `radial_sweep`.

## Independent geometric checks

The tool test uses R = 10, H = 20 and slope -0.1. A neutral plane at z = 0
has end radii (10, 8), so W has volume

    pi*20*(3*100 - 100 - 80 - 64)/3 = 1120*pi/3.

A neutral plane at z = -10 has end radii (9, 7), so both ends need caps:

    pi*20*(3*100 - 81 - 63 - 49)/3 = 2140*pi/3.

These are cylinder volume minus the closed-form frustum volume, independent
of the stitched tool's measured result. Both tools must pass the full
geometry check and remain manifold.

For S8075 the cone is r(z) = 600 - z*tan(3 degrees). At z = 1900 its
radius exceeds 500 and the blind cuts have not begun: (-480, 0, 1900)
is material. At z = 2000 its radius lies between 495 and 496: (500, 0,
2000) is outside. At z = 2300 its radius lies between 479 and 480.
(-410, 0, 2300) is material, 70 from the central hole's axis, while
(-430, y, 2300), y = -180, 0, 180, lie inside the cone and inside their
respective radius-60 holes (distance 50 from their axes), so are void.
All three holes must keep their inward wall portions, and open through
the conical wall. The prior finding derives the six spatial branches.

The finite-slab guard uses R = 1.5, H = 4, a boss of size
0.36 by 0.72 by 0.36 entirely above z = 4, and a radius-0.05 blind hole
at radial distance 1.4 over [3.8, 4]. Its nearest radius is 1.35, while
r(3.8) < 1.301, so the hole vanishes entirely. With s = 1.5 - 4*tan(3
degrees), the result's volume is therefore

    pi*4*(1.5*1.5 + 1.5*s + s*s)/3 + 0.36*0.72*0.36.

Its nine faces are the cone, the bottom, the exposed top annulus, the
boss's exposed underside, its four side walls, and its top. This checks
both removal of a floor outside the cone and preservation of the overhang.

## Remaining reproduced case

Before the cap corrections, S1767 failed before drafting. A source diagnostic on the committed
reproduction returns `NotSolidBecause(Invalid)` and three instances of
`SmLoop::Test #2 Bad Loop Orient: A UpperLoopuse->Edgeuse's 'Inside' is
Outside Surface Natural Boundary`. The clean rerun after the exact-cap and iso-height changes still returns
`Argument` at the draft input-solid precondition. It remains ignored;
repairing the joined input is separate from the complete-wall removal
construction, and no validity gate is relaxed.

## Verification

All Cargo validation uses `buildq run solidcraft . -- cargo` from the
worktree root. Inline Cargo configuration sets and forces
`TMPDIR=/home/ray/builds/lane-18805/tmp`; absolute manifest paths keep these
runs on this machine. No golden or kernel-case expectation has been changed.

The preceding reproduction probe was:

```sh
TMPDIR="$HOME/builds/lane-18805/tmp" buildq run solidcraft . -- cargo \
  --config 'env.TMPDIR.value="/home/ray/builds/lane-18805/tmp"' \
  --config env.TMPDIR.force=true test \
  --manifest-path /home/ray/builds/worktrees/lane-18805/solidcore/Cargo.toml \
  -p solidcore -p solidcore-ops --lib -- \
  surface::cone::p1::tests:: \
  offset::tests::drafts::a_cylinder_with_three_blind_holes_drafts_through_the_hole_walls \
  offset::tests::drafts::a_cylinder_with_touching_stepped_bosses_drafts_under_both \
  --include-ignored --nocapture
```

- Initial S8075 implementation run (job `1795002084.149875-6c3adef6`):
  failed with the unchanged `LoopsCross(Unmerged)` refusal, showing the
  new event had not selected the case.
- Initial local-operation area run (job `1795002099.765181-7ef45e02`):
  41 existing tests passed; the new radial-sweep test failed because its
  complete-boundary predicate declined the default-kernel input. The
  source and target frames, heights, and natural boxes were identical.
  The control now retains exact pcurves, as the ops reproduction does.
- Root and solidcore `cargo fmt --all` and format checks passed. Final
  checks after diagnostic removal remain required.
- The exact-cap implementation and interval canvas bounds passed solidcore
  `cargo fmt --all` (client session `44045`) and `git diff --check`.
- Diagnostic S8075 run (`1795003614.167027-c1bcd4d8`): production kernel compiled; test compilation failed because its temporary diagnostic lacked a `Kernel` import. Corrected to the qualified kernel type; rerun required.
- Local-operation and draft area run (`1795007682.217617-76c8aea0`):
  57 passed, 1 failed, exit 101; the new construction test declined the
  fitted top cap. This result motivated the exact-cap foundation above.
- Corrected S8075 diagnostic (`1795014338.505242-7e328bba`): production
  and tests compiled; 0 passed, 1 failed, exit 101 in 254s, with
  `LoopsCross(Unmerged)` and a top support at z = 2399.9999999999995.
  The unnecessary Vec2 qualification warning was corrected. This result
  motivated the analytic iso-height map above.
- Root and solidcore `cargo fmt --all` passed again after the height and
  ordered-cap changes (client sessions `42266` and `17356`).
  `git diff --check` passed.
- Kernel area run `1795024721.205695-74ef3330` (local operation, draft,
  offset executive, and surface part 2): exit 101 in 74s, before tests.
  The two new plane tests called the nonexistent `Extent1d::from_bounds`;
  both calls now use the type's `from_min_max` constructor.
- Cone part-1 area plus both reported cases queued:
  `1795034967.312994-f804a214`. The two reproductions are explicitly
  included while still ignored; no other ignored area is enabled.
- Root and solidcore formatting passed after the direct defining-circle
  construction and both test-constructor corrections (client sessions
  `11804` and `68540`).
- Combined affected kernel areas and active ops offset-file tests
  `1795045534.614663-0148125e` were canceled while queued (exit 130,
  `client gone`) under the owner's new instruction to stop after the
  fix compiles and its regression passes, and leave wider checks to CI.
  The original-case probe remains queued because no reported case has
  passed yet. Temporary predicate diagnostics and file dumps were
  removed before that probe starts.
- Wider area reruns and changed-crate clippy are skipped under that new
  instruction. This record does not yet claim the reported case builds.
- After diagnostic removal, root formatting (`39388`) and solidcore
  formatting (`71101`) passed, as did `git diff --check`.
- Clean probe `1795034967.312994-f804a214`: compilation passed in
  2m 25s; all 19 cone part-1 tests passed, including
  `iso_circles_retain_the_cones_exact_axial_heights`. Both reported ops
  cases still failed (0 passed, 2 failed, 110.90s): S8075 again returned
  `LoopsCross(Unmerged)` and S1767 returned `Argument`. Overall exit 101
  in 258s. Thus the exact-height foundation is verified, but the radial
  event still declines S8075; no original case is fixed yet.
- Focused predicate diagnostics were reintroduced in the two existing
  regression tests to identify that remaining selection failure. The
  next run selects only the new radial-sweep control and S8075, with no
  wider area or clippy job queued.

- Removed the temporary diagnostics again before the next focused job,
  `1795054014.795559-9c483dd2`, starts. It selects only
  `radial_sweeps_close_at_the_hinge_or_on_two_annular_caps` and S8075.
  The new pure-translation definition-preservation path is included.

- Solidcore formatting passed after the translation branch and its stored-frame
  regression were added (client sessions `56114` and `37291`).
  `git diff --check` passed, and all temporary predicate prints are absent.

## Final focused result

The first self-contained construction step now builds S8075. The original
`a_cylinder_with_three_blind_holes_drafts_through_the_hole_walls` regression
is active again; its successful run also checks the hand-derived point
classes, manifold solid status and zero `CheckLevel::All` failures. The
new `radial_sweeps_close_at_the_hinge_or_on_two_annular_caps` regression
passes both cap arrangements, closed-form volumes and attribute/definition
preservation.

Final command (job `1795054014.795559-9c483dd2`):

```sh
TMPDIR="$HOME/builds/lane-18805/tmp" buildq run solidcraft . -- cargo \
  --config 'env.TMPDIR.value="/home/ray/builds/lane-18805/tmp"' \
  --config env.TMPDIR.force=true test \
  --manifest-path /home/ray/builds/worktrees/lane-18805/solidcore/Cargo.toml \
  -p solidcore -p solidcore-ops --lib -- \
  topology::brep::local_operation::tests::radial_sweeps_close_at_the_hinge_or_on_two_annular_caps \
  offset::tests::drafts::a_cylinder_with_three_blind_holes_drafts_through_the_hole_walls \
  --include-ignored --nocapture
```

Compilation passed in 2m 57s, without reported warnings. Kernel: 1 passed,
0 failed, 3.60s. Ops: 1 passed, 0 failed, 9.43s. Overall exit 0 in 191s.
The only subsequent Rust edit removes S8075's ignore attribute; no further
Cargo checks are queued, following the owner's instruction to stop once
the fix compiles and its own regression passes.

S1767 remains ignored: the preceding clean probe still refused its joined
input at the solid precondition (`Argument`). General caller-notified
local operations, partial cylindrical walls, other support classes and
mixed edits remain outside this construction step. Wider area tests,
changed-crate clippy, wasm and goldens remain for CI. The newly added
plane and ellipse-support unit tests were compiled but not selected by
the final focused run. No golden or kernel-case expectations changed.
All source changes and COMMIT_MSG.txt remain unstaged.

## Narrowed to the draft path

CI's goldens showed the shared changes above moving 270 rows in 17 units.
`cone_iso_curve`, `Cone::transform`, `Surface::create_plane_with_3d_curves`
and `Plane::transform` are back to main; the selector takes the body's
rim faces as bare planes (see "The body's rim planes"). All through
buildq, from the worktree:

- Focused tests (`radial_sweeps_close_at_the_hinge_or_on_two_annular_caps`,
  S8075 `a_cylinder_with_three_blind_holes_drafts_through_the_hole_walls`,
  `a_drafted_cylinder_keeps_its_boss_and_loses_a_hole_beyond_the_new_wall`,
  `circular_boundaries_keep_their_exact_common_plane`,
  `nearby_ellipse_planes_are_not_one_exact_plane`): 5 passed, 0 failed.
- Goldens, 0 changed, 0 broken each: surface.cone_p1 (20736 cases),
  primitive.api_primitives (2245), offset.offset_executive_p2 (594),
  blend.fillet_executive_p3 (798), topology.api_files (339).
- `error::phrase_sites`, `error::blend`, cone part-1 and plane part-2
  tests: 46 passed. Clippy `-p solidcore -p solidcore-ops --all-targets
  -D warnings`: clean. Solidcore `cargo fmt --all -- --check`: clean.
