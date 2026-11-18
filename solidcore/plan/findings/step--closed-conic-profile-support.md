# STEP: regression coverage for closed swept conic profiles

Issue #17142 reports two circular extrusions and a circular revolution omitted from a bent
wire. The full file supplied with that report already reads as a closed five-face solid in
the current source, before any production change in this lane. Its native kernel volume is
1.9581955009590084 mm^3, matching the independent derivation below.

A profile whose seam falls inside a quarter needs five quarters of its frame's grid. Those
quarters, left whole, cover more than one turn and do not close. Commit 51404ea3e (#18200)
changed `solidcore/src/eval/form.rs::conic` to cut the first and last quarters to the requested
arc's ends. Each cut uses the rational quadratic arc's blossom in homogeneous coordinates:
the same curve at the same parameters, with no extra turn, moved seam, or tolerance change.
See Piegl and Tiller, *The NURBS Book*, second edition, rational conics and knot insertion,
and `docs/definitions-and-certificates.md`, section 3.2.

The regressions check the profile's evaluated points and periodic closure, rather than
requiring one particular knot layout. A circle or ellipse can be represented by four complete
quarters or by five quarters whose end pieces are cut; both must preserve the exact profile.

## Independent expected values

The synthetic capped extrusion has radius 0.3 mm, displacement (0.1, 0, 1.3) mm, and circular
sections in XY. Its volume is the base area times normal height:

    pi * 0.3^2 * 1.3 = 0.3675663404700058 mm^3.

It has three faces (two caps and the side), three edges (two rims and the twice-used seam),
and two vertices. The profile's period is four rational quarter parameters. These values
also apply when the seam is at either sign of 2e-12 rad, at zero, or at 0.2 rad.

For the report's wire, integrate each transported circular section's oriented area against
its centre's velocity. With radius r, unit section normal n, and displacement L*d, an
extrusion contributes pi*r^2*L*(n dot d). With rotation axis (o,k), profile centre c, and
rotation angle delta, the revolution contributes pi*r^2*delta*(n dot (k cross (c-o))).
Normalising the directions stated in the file gives these three normal heights:

    1.3063365221485899 + 1.0639557538445925 + 4.555407579960238 mm.

Multiplying by pi*0.3^2 gives 0.36935794989882453 + 0.3008264022020486 +
1.2880111488579307 = 1.958195500958804 mm^3, independently of the importer.

The regressions live in `solidcore-step/src/topology_tests.rs`: the whole-profile support
and period, and a capped oblique extrusion imported through STEP and rebuilt as a native body.
No golden or kernel-case expectation is changed.

## Validation boundary

The original report was evaluated in a temporary current-source `solidcore-step` test,
including native-body reconstruction and volume, and passed. The supplied third-party STEP
text was then removed; only independently constructed geometry remains in the regressions.
The first test command used the `bent_wire_sweeps_keep_their_closed_profiles` name filter:
it ran that temporary report test (pass) and an initial fixed-knot-layout assertion (fail,
exit 101 overall). The latter was an invalid test requirement, not a reproduction of this
issue: the existing five-quarter construction cut at its endpoints is valid. It was replaced
with the point-and-period assertions retained here; those revised assertions remain unrun.

The installed 0.44.1 CLI is a different build and refuses the file with a knot-multiplicity
error. It cannot establish whether the current application path has that separate refusal.
The queued current-source application test could not compile: the XPS worker returned
`[Errno 122] Disk quota exceeded` (exit 2). No production change is justified by that result.

The final targeted checks were submitted from the worktree root:

- `buildq run solidcraft . -- cargo test -p solidcore-step --lib topology::tests::`
- `buildq run solidcraft . -- cargo clippy -p solidcore-step --all-targets -- -D warnings`

Both returned exit 2 before compilation with that same worker disk-quota error. The two
retained regressions therefore remain unverified. Root formatting and the separate
`solidcore-step` manifest formatting check passed. No golden, format, command schema,
UI string, production code, or default changed.
