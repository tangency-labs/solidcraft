# surface.offset_surface: one outward trim that edits no net (2026-11-08)

Continues #1914 with one supported complete row / six moved components,
`gen1851`. Its outward trim is not an actual net trim: with extension disabled,
the inspected branch leaves both receiver geometry and parameter domain alone.
No production construction, flag, input, tolerance, floor, decoder, crop or
skip changes. All six original steps and all 147 raw scalars still execute.

## State proof before the changed evaluation

The receiver owns its original direct B-spline, has domain `[0,0]–[1,1]`,
degrees p=1/q=3, distance `0.37661542454724795`, explicit mask 9 and extension
disabled throughout. Steps 0–2 are state, a derivative request and a point
query, not base edits. No earlier operation can create an extended surface.

Step 3 asks to trim to
`[-0.10841096368374764,0,1.4533635181537692,1.141637526818134]`, whose minimum
and maximum corners extend beyond the original domain. In
`OffsetSurface::trim_with_domain` (`surface/offset_surface.rs:1969-1990`),
either outside corner sets **gets_bigger=true** after evaluating that corner.
With `allow=false`, `select_surface` clamps those evaluations and creates no
extension (`:1304-1348`). Consequently the `if let Some(extended)` trim has
nothing to edit, and `owns_surface && !gets_bigger` is false: **the original
base is not trimmed**. The successful status and returned four domain scalars
are retained; their binary64 bits equal the original request exactly.

Thus step 4's changed `eval_simple(hu=2,hv=3,upper=true)` at
`[0.5,0.45456415197103317]` still uses the **original net and domain**. It is
strictly inside the original u span `(0.13597465269160103,1)` and v span
`(0,0.45930959990169207)`, away from a knot-side ambiguity. Step 5 is state.
No post-trim reconstructed net is used as an independent expected input.

The evidence model validates the complete original input identity, output
shape and every value/bit pair against the retained four-term counterfactual.
It audits every operation, excludes enabled extension, nonowning or nested
receivers, actual inward trims, failed trims and other base edits. Only after
that state proof does its **private decoder** view trim's known status/four-
scalar payload as the existing `max_anal` query shape. This neither changes
the production script nor relaxes the default evidence decoder, which still
refuses `trim`. Every moved field must be an independently graded jet, not a
status, returned domain or other query result. The complete script is decoded;
unchanged fields are not asserted independently correct.

## Independent values and exact complete row

Original binary64 control points, weights, knots and distance are converted
exactly. Independent 80/160-digit Cox-de Boor/Taylor algebra differentiates
`S + d (Su x Sv)/|Su x Sv|` from that net. All six moved components pass
unchanged relative `1e-9`, without a floor. Maximum relative discrepancy is
**9.048670857263963e-13**; precision drift **1.3065385952206204e-76**. The
regularity/normal-substitution guards are unchanged. This is a local numerical
and state proof, not a whole-domain interval error/regularity certificate.

The replacement equals the complete retained collector encoding. Common
fields stay `deferred=null`, `error=null`, `ints={}`, `topology=null`; `out.n`
stays **147**. Blocks are `[sum, abs_sum, weighted_sum]`, not error bounds.
Input hashes use sorted compact Python JSON, UTF-8, of the full original input.

| Case | Components / max relative error | Full original input SHA-256 | Old → new checksum blocks / unchanged n |
|---|---|---|---|
| `gen1851` | 6 / `9.04867e-13` | `71955d6e58899aa90a8f3e07a5fd12adce0d1f9b008a5af7cf74c613054d7532` | `[[61829.67557510773, 61888.01368940549, 3859448.1427769545]]` → `[[61831.18435404368, 61889.99122955711, 3859566.2962446106]]` / `147` |

All moved observations, at step 4 (rounded display; full precision archived):

| Raw index / named partial / axis | Observed | Independent expected | Relative error |
|---|---|---|---|
| 75 / D(0,3) / x | -0.2008756440505442 | -0.20087564405050865 | 1.770e-13 |
| 76 / D(0,3) / y | -0.03506846111704871 | -0.03506846111704853 | 5.293e-15 |
| 77 / D(0,3) / z | 1.742602619174874 | 1.7426026191748802 | 3.547e-15 |
| 84 / D(1,2) / x | -0.0007919694071997914 | -0.000791969407200508 | 9.049e-13 |
| 85 / D(1,2) / y | 0.004952324689906306 | 0.004952324689906027 | 5.621e-14 |
| 86 / D(1,2) / z | 0.0016653571816100078 | 0.0016653571816091944 | 4.884e-13 |

## Controls, exclusions and provenance

All **98 numerical/scope controls pass**. Six new controls check an independent
affine-plane closed form through the no-op; reject inward trims, nonowning
receivers, extension, other edits, failed trims and changed returned rectangles;
reject wrong jets and tampered bits; and bind the actual complete record while
showing the previous default decoder still refuses trim. `gen2214` independently
disagrees and is **not blessed**. Actual net trimming and other exclusions stay
visible; no unsupported operation is inferred from its request shape alone.

The **71-member executable/raw/source/control archive** was read back and
hash-verified, SHA-256
`677d75b737faa780761f80f302738623bc479d7347f01aa746219927c6128113`.
It retains the complete original inputs/raw values/bits, old/new rows,
full-precision observations/disagreements, decoder/state proof and source
snapshots. Local path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-outward-trim-noop-20261108-5846b6075/`.

Pre-edit head: `5846b6075166185d668d64a63471a17575afe272`, refreshed onto main
`e78687eb5ef9f7751f591c1964cfc0d4005334b5`. Both published `ceadc86fe` and
pre-refresh local `63ab11e08` were preserved locally and on the fork at
`refs/archive/20261108-pr1914-before-e78687eb5/{published,local}`. All 23 PR-owned
file bytes were preserved by that refresh. At the pre-edit head the actual
unit gate measured 187 ordinary differences, all 3,182 cases retained, and
fmt, workspace/all-target clippy, wasm, panic inventory and 23 offset tests passed.

The collector remains attributed to actually measured head
`7a90407a0fe189ebceb551e5df28aaf4e8cc9893`, raw SHA-256
`735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
Numerical source SHA-256 remains
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
New main includes shared surface/revolution changes: the fresh gate is a new
execution, not a relabelling of that collector or a claim of fresh raw-stream
bit identity across main's changes.

Every other golden and all 88 known failures remain unchanged. Projected
accounting is **169 supported cases / 830 components**, **186 ordinary
differences**, all **3,182 cases = 3,094 goldens + 88 known failures**, pending
the actual gate. No B-rep, topology, validity, volume or actual trimming result
is certified here. Correctness, historical attribution-review and canonical-CI
holds remain; no approval request, rerun or readiness is implied.
