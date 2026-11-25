# Offset derivatives after returned base offsets and inversion queries (2026-11-08)

Continues #1914 with five independently supported golden replacements, not a
new numerical implementation or #2717's grid contract. All 26 moved components
are receiver derivatives; no returned-surface or inversion-query value is blessed.
`gen2558` independently disagrees and stays unresolved.

## Receiver state and independent calculation

The runner in `crates/goldens/src/units/surface_offset_surface.rs` serializes
`base_surface().create_offset_surface(...)` without assigning the result to its
receiver. For a direct B-spline, `BSplineSurface::create_offset_surface` in
`nurbs/b_spline_surface/p1.rs` copies the input into an owning `OffsetSurface`,
including for zero distance. The evidence decoder accepts only that non-null
type `15400`, decodes its entire payload and retains the receiver's original
distance. Other kinds and nullable providers are not inferred.

`step_inv` serializes a parameter pair and location on success, or just its
status on failure. None of those fields moves in these five cases. These are
direct B-spline receivers with extension disabled throughout: the inversion's
base is not a nested offset, and `select_surface` cannot make an extension.
The query therefore leaves the input geometry/distance used by subsequent
derivatives intact. This is not a claim that inversion is state-free in general;
extension-enabled and nested receivers remain outside this proof.

From the original rational control net, weights and knots alone, the existing
independent Taylor algebra evaluates
`O = S + d (Su x Sv) / |Su x Sv|` through order three at 80/160 decimal digits,
converting each binary64 input exactly. The four-term counterfactual selects
the moved indices only; neither its derivatives nor its golden supplies an
expected value. Every moved index must decode as a named receiver derivative,
with no final-writer collision, and all other output fields must be unchanged.
No new tolerance or absolute floor is used.

All 26 components pass strict relative `1e-9`, maximum error
`1.176059847726366e-12`; maximum relative precision drift is
`1.692416792727516e-76`. For the three boundary cases the original knots determine
the clamped point before differentiating, not derivatives of a constant extension:

| Case / step | Requested UV | Evaluated UV |
|---|---|---|
| `gen1623` / 0 | `[2.4772525243355457,0.27913697416804306]` | `[1.0,0.27913697416804306]` |
| `gen1782` / 1 | `[-0.3652375559371569,0.6945687218522235]` | `[0.0,0.6945687218522235]` |
| `gen2425` / 0 | `[0.0823062150585343,-2.08422407711569]` | `[0.0823062150585343,-1.5916340197964498]` |

## Exact row replacements and retained input identities

Each new row is the complete retained actual collector encoding. The common
fields remain `deferred=null`, `error=null`, `ints={}`, `topology=null`; `out.n`
stays 134, 191, 45, 133 and 37 respectively. Only the blocks below change.
The blocks are `[sum, abs_sum, weighted_sum]`, not error bounds; the errors grade
the raw moved components, not their checksums. Input hashes use sorted compact
Python JSON, UTF-8.

| Case | Components / max relative error | Input SHA-256 | Old → new checksum blocks |
|---|---|---|---|
| `gen0136` | 3 / `5.79439e-15` | `4515a282fe90fd6359b6858d6d7c8301dc3f80f89eb2f6e707b2f2a1d8695052` | `[[91876.89527025135, 92224.64165636596, 3709608.8064747667]]` → `[[91973.71856665419, 92226.07023677968, 3722151.3675662098]]` |
| `gen0287` | 3 / `1.17606e-12` | `e51e3bd2ec6e6c620c88445e04bb4153986ad684128ca811a218da31284c462f` | `[[106623.85073001102, 106758.36789985334, 8192383.812744899]]` → `[[106623.09246265274, 106758.03749782688, 8192286.540092921]]` |
| `gen1623` | 8 / `1.22953e-14` | `857b21820fa862beba1580ea3dd53eb5debdd8553e0b86d9c5790abee8e31467` | `[[937.572243299023, 1479.9961428738736, 24633.179569469605]]` → `[[898.8254877713206, 1424.9679762169394, 23984.775109513852]]` |
| `gen1782` | 3 / `1.9561e-14` | `015844bf6a82125fa52f3b3cbbd7dcf3753397ed5a8a9e725be22de887cb8fff` | `[[77236.60030788192, 87804.80267997787, 4353949.4496201]]` → `[[76718.60999413418, 86553.13132175046, 4319698.866069493]]` |
| `gen2425` | 9 / `3.87624e-14` | `3e08eda805173750cd4cb0f3a6bcf45b221e3853587034ab96ed05a4060e286e` | `[[994.256720147682, 1013.7863198590826, 32902.28326575608]]` → `[[995.7214865508853, 1015.1216311978034, 32929.3849376887]]` |

All five original inputs and all raw binary64 bits were compared with the
retained streams before editing. Every unrelated row and all 86 known-failure
entries stay byte-identical; all 3,182 cases remain selected as 3,096 golden rows
plus 86 known failures.

## Controls, exclusions and provenance

All 52 numerical/scope controls pass. The four new controls distinguish returned
base-offset distance from receiver distance; decode an unchanged inversion result
but reject a moved query field; reject a null/wrong returned kind; and still
reject extension-enabled receivers or wrong derivative values. Existing controls
cover independent closed forms, weights, parameter orientation, clamping,
named-slot aliases and precision stability. Exact-zero closed-form controls may
compare 95 digits of a 100-digit Decimal calculation; captured kernel values
still use strict relative `1e-9`, never a new floor.

`gen2558` remains unapproved: its three `D(1,2)` components have relative errors
about `0.343`, `0.929` and `1.870`. Their small magnitude (about `1e-15`) is not
an excuse to add an absolute floor. No new failure attribution is made. Unsupported
edits, extension, ambiguous knot sides, singular substitutions and actual aliases
remain excluded. No B-rep topology, validity, volume or global fold certificate is
claimed by these local derivative observations.

The raw collector was actually run at
`7a90407a0fe189ebceb551e5df28aaf4e8cc9893`, raw-stream SHA-256
`735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
At pre-edit head `b982cc76604d6c910dfe313064aa4417250f0674`, numerical source
SHA-256 remains
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
These identities are preserved, not relabelled as newer executions.

The 39-member executable/data/control archive was read back and hash-verified,
SHA-256 `94b10bb4a4e6f0fe58e9d55aa98416b09ede4ec237b0d99bd5a3e95ee7efa65c`.
It includes complete original inputs/raw values/bits, all full-precision
observations and exclusions, the disagreement, full old/new encodings, controls,
the prior helper versions and the pre-edit identity receipt. Local retained path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-returned-base-input-oracle-20261108-b982cc766/`.

Projected accounting is 152 supported replacements / 747 moved components and
205 remaining ordinary differences, pending the actual gate. Parent canonical
run 42465643185 ended `action_required` with zero jobs, not success. The PR stays
draft with its correctness and attribution-review holds; no workflow approval,
rerun, review resolution or #2717 implementation is implied.
