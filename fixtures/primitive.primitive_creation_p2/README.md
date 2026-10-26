# Primitive creation p2 comparisons

A case is `{ "op": N, "data": [...] }`, checked by the FFI wrapper before either
engine runs (layouts in `crates/solidcore-ffi/src/units/primitive_primitive_creation_p2.rs`).
Curve records are p1's `[kind, x, y, z, a, b, c]`: a B-spline line, or a quadratic
circular arc (the same construction as `SmApiCreateArc`). Each op calls the public
`SmPrimitiveCreation` entry point on a fresh native B-rep with only its infinite
region; the static planar-path sweep makes its own B-reps.

Every record compares, on both sides:

| Value | Meaning |
| --- | --- |
| status | `0`, or the upstream status of the failure (`SM_ERR`, `SM_ERR_INVALID_INPUT`, ...) |
| edit | the B-rep's editing flag afterwards (each case sets the initial flag) |
| doing boolean | the context's doing-boolean flag afterwards |
| maps | the counts of the four entity maps |
| faces | the start, side and end face list sizes |
| extra | skin: one ownership flag per curve; taper: the curve normal afterwards; planar path: whether a B-rep came back |

## What is compared

| Path | Cases |
| --- | --- |
| Partial cone: height below `SM_EFF_ZERO`, a turn of almost zero degrees, a zero-length side or axis (after edit mode is switched on, so the flag left on is compared) | upstream `TestSmCreateSphere` partial cone, adversarial fixtures, generated |
| Partial sphere and torus: zero or negative radii (line or circle construction fails after edit mode is on) | upstream `TestSmCreateSphere` partial sphere and torus, generated |
| Full turns (`|turn - 360| < 360 * SM_EFF_ZERO`): p1's analytic constructors and their degenerate-radius failures | adversarial fixtures, generated |
| Skin with fewer than two profiles: the early `SM_ERR`, with the output lists cleared and the ownership report sized and all false | adversarial fixtures, generated |
| Curve sweeps from no edges and no faces: p3's empty-profile exit | adversarial fixtures, generated |

## What is deferred (not certified)

The Rust side runs every entry point through the production adapter
(`primitive::primitive_creation::model::PrimitiveModel`), which makes the scratch and
piece B-reps (`new SmBrep()`), copies faces between them and keys faces as the B-rep
caches resolve them. Cases that build topology go on until a call that is not ported
yet and defer there (no value is compared):

| Entry point | First unported call | Blocking unit |
| --- | --- | --- |
| skin, pipe sweep, planar path, some partial solids and tapers | the shell ray fire of `StitchAndOrient`'s region split reaches a face's trimmed surface cache | `surface.trim_srf_cache` |
| taper extrude | the merge of its pieces (`SmMerge::SmMerge`; the model keeps this deferral, `SweepStore` owns the merge services) | `boolean.merge_p2` |
| partial cone, sphere and torus | `SmFace::FindVertexSector`, a face handle of a surface's face, `SmFace::EdgeCurveOnClassify` | `topology.face_p4`, `topology.face_p2`, `query.face_p1` |
| a few of each | an opaque curve handle's `SmCurve::GetNaturalInterval` | `curve.curve_p2` |

Before the production adapter, a one-B-rep fixture keyed faces in its own namespace,
which the B-rep caches cannot resolve to a face: the ray fire then treated the faces'
surfaces as untrimmed and 12 pipe, skin and planar-path cases matched C++'s status on
that path. With the production keys they reach the trimmed cache like C++ and defer
(see `plan/findings.md`).

The upstream skin (`TestSmCreateSkinPrimitive`), draft sweep (`TestSmCreateDraftSweep`),
taper (`TestSmTaperExtrude`) and pipe (`TestSmPipeSweep`) cases run the real C++ and
defer in Rust. The pipe test's cubic path is re-expressed as a half circle (the record
format has lines and arcs only). The face and edge sweeps of
`TestSmCurveSweepFromFaces` and `TestSmCurveSweepFailureCleanup`, and the face/edge
blend, need real faces and edges and are not fixtures yet.

## Rejected inputs

The wrapper rejects what upstream cannot run safely: a taper of fewer than two curves
(UB-111 for none; with one, the sliver test can read past the split curve array) or with
a curve shorter than `2e-5` (UB-112); a planar path without path curves; skin profile
counts past the curves. A curve record that cannot be built natively is rejected, not
compared.

Four guards come from isolated native probes (one process per input, 20-25 s limit):

| Guard | Upstream issue | Rejected |
| --- | --- | --- |
| A taper whose chaining deletes a curve | UB-50: `FixChainCurvesList` (`2 * SM_ZONE_TOL_3D`, deleting duplicates) deletes a curve whose ends and mid point match the chain built so far, then reads it again (segfault) | the wrapper repeats the chaining on the curves' end and mid points: two identical lines or arcs, a line and its reverse, a line from a chain's end back to its start through its last link's mid point (a repeat of no curve). With a supplied normal upstream reverses the curves and chains them again when their computed normal (`ComputeCrvsNormal`) is antiparallel to it; the wrapper repeats that pass unless its own sum of the same sampled cross products surely points along the normal (a half circle and both its chords segfault under -Z and run under +Z). The end cap at the offset end (caps 2 and 3) chains the trimmed offset curves through a copy of their list, so one deleted there is freed again at cleanup (`double free or corruption`, or a segfault); where upstream may reverse the curves, the wrapper refuses a closed chain whose short curve meets one over 100 times longer at a corner sharper than `3e-5` radians. Natively (7,236 probes over widths, lengths, sizes 1 to 1,000, heights 1 to 100, drafts -45 to 45 degrees, every normal and cap): such slivers crash up to a 1e-6 corner and run from 3e-6, links up to 5e-4 of the long side crash and 1e-3 run, and none crashed where upstream does not reverse |
| A taper with the start cap (caps 1 and 3) whose curves close a loop while an end meets no other end or more than one | UB-247: `CreatePlanarFaceWith3DCurves` needs each curve end to meet exactly one other; when it fails, the taper deletes input curves that edges of the result still use (a segfault in `SmEdgeuse::~SmEdgeuse` as the B-rep is deleted, or inside `CreateTaperExtrude`) | the wrapper joins the curve ends within `2 * SM_ZONE_TOL_3D` (a repeat of another curve, the same ends either way and the same mid point, left out) and refuses the curves when an end meets no other end or more than one and a curve closes a loop: a line into a unit square ending at its corner (`difftest --fuzz` case `fuzz00880`), the same line stopping `1e-3` short, two squares at a corner, a line from a corner into the square, a half circle across a side (the side's ends, another mid point: no repeat). Natively (about 400 probes over tails, squares, spikes and arcs, every cap and normal) each crashes under one normal or another with the start cap and runs without it; curves that close no loop fail first and harmlessly, and clean loops ran in every probe |
| A swept profile edge shorter than the zone tolerance (`1e-5`) | UB-215: `SweepEdgeHigh` uses the edge's curve after `MakeFaceWithCurves` freed it (segfault) | partial cones (turn below `359.99999`) that build a side, base or top line from `0.9999e-6` up to `1e-5`, mirroring the cone's own early failures (a shorter line's face fails `IsDegenerateCurve` first, `SM_ERR_INVALID_INPUT`; the axis line sweeps to no face); partial tori whose minor circle is below `1e-5` |
| A closed shell from half the zone tolerance to `1.5e-4` across | HANG-35: the ray fire that places the shell in its region takes minutes | spheres, tori and cones of that size at any turn (analytic faces from `359.99999`, stitched sweeps below) |

Early failures stay compared: a cone without height or turn, a zero side, a radius
within the scaled zero (not built), zero radii, and the duplicate-looking tapers the
chaining keeps (`review_duplicate_curves.json`, each run natively without a crash): a
square with a repeated or reversed side under each normal and both caps, two halves
of a circle, a half circle and its chord, a line and its reverse chained apart (an open
chain), and, with the start cap only, the line back `2.5e-5` off the last mid point and the
narrow closed chain of the second pass; and, from the review of 2026-10-21, a half circle
with both its chords and the line and its reverse apart under +Z or a computed normal, a
triangle `5e-5` tall on a side of 1 and a needle under -Z with the end cap, and the
sliver under +Z; with the start cap, from the fuzz finding of 2026-10-21, a self-crossing
bow-tie, a line into an open square (no loop), two squares apart and a square `4e-5` off
another's corner. The wrapper still refuses some inputs that run: the start-cap rule refuses
any loop with an end met by no other end or by more than one (a line ending inside the square,
a square with a diagonal and two squares at a corner listed in another order run natively); the second pass and
the corner rule count a chain without area as reversible under either normal (a line
back from `1` along the same span runs under +Z), the corner rule keeps a tenfold
margin, and the offsets' chaining before the caps (`SmPrimitiveCreation.cpp:3884`) is not
checked. The cone guard follows `SweepEdgeHigh`: a line of length `L` sweeps to a face with
`|Su| = L` at every sample, and `IsDegenerateCurve` (tolerance `1e-6`, or the scaled zero
when larger) refuses it before the face build up to `L = 1e-6`. Native probes (one
process each): `1007` up to `1e-6` and a segfault just above it at unit size, `1007` up
to `1.0005e-6` and a segfault at `1.002e-6` a distance of 1,000 away, and a segfault at
exactly `1e-6` in the frame of `ub215_partial_cone_apex_top_line_1e-6` (rounding decides
there, so the guard starts at `0.9999e-6`); a short axis line alone returns `1025`. 300
random cones the narrowed guard accepts and the earlier one refused (lines from `1e-9`,
origins up to `1e4`, turns from `1e-9` to 359 degrees) ran without a crash
(`ub215_degenerate_faces.json` keeps 16 of them). The cone guard still refuses some
inputs that run: lines in the band at larger distances (the scaled zero passes `1e-6`
from about 1,000 away; at `1e5` a `2e-6` line returns `1007`) and at small turns where
the far end of the line moves at most about `1e-6` (`2 L sin(turn / 2)`, `1007` from
`0.01` degrees, while a turn of `1e-9` degrees segfaults). The torus guard is unchanged:
its native boundary depends on the major radius and the position (a minor radius of
`1.5e-7` returns `1007` and `2e-7` segfaults at a major radius of `0.01`, while at major
radius `0` up to `5e-7` returns `1007`, and `1.59e-6` returns `1001`), so it refuses
more than crashes. The generator keeps the earlier cone guard for its redraws
(`generator_refuses`), so the generated cases are the same draws. Partial spheres are not guarded: their
axis line sweeps to no face and their arc's face fails the degenerate-surface test
first, so no probe crashed. Every rejected case that reaches the native sweep defers in
Rust today (`topology.brep_p8`), so the guards remove no compared value. The generator
draws again when the wrapper refuses an input (as `curve.curve_p1`); its tapers are
a square, a square with a side repeated either way, a line with its repeat alone and
the line back past a short first link, under a computed, up or down normal. Tapers (UB-112),
skins and planar paths (seconds to minutes natively below that, HANG-35) stay at
scale `1e-3` or more. `guarded.json` keeps the crashing and hanging generated cases as
expected rejections.

The native context is shared by every case: the shim clears its doing-boolean flag
before each one, because a failed planar-path boolean leaves it set (WR-191), while the
Rust side starts from a fresh context.
