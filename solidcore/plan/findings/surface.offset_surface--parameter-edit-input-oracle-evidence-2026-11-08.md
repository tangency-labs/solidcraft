# Offset derivatives in five parameter-edit scripts (2026-11-08)

Continues #1914 with five independently supported replacements / 24 moved
components. Production formulas, input scripts, tolerances, floors, the failure
registry and #2717's named-grid contract are unchanged. `gen0304` and `gen0672`
independently disagree and are not blessed.

## Independent state and value proof

For a clamped direct B-spline, reversing one parameter reflects its knots by
`lo + hi - t` and reverses that direction's control rows. Swapping u/v transposes
the net, knots and degrees. Both reverse the base normal. The inspected receiver
API (`surface/offset_surface.rs`) leaves the offset distance unchanged on reverse,
but negates it on swap. This is explicitly modelled from the original input net,
not from an edited kernel net. Extension is disabled throughout; periodic or
unclamped knots, other edits, failed parameter edits, ambiguous interior-knot
sides, actual aliases and unsupported derivative orders remain excluded.

At each changed evaluation, the current mathematical input net and scalar
distance are constructed separately at 80/160 decimal digits. The independent
Taylor algebra differentiates `S + d (Su x Sv) / |Su x Sv|` through total order
three, applying explicit clamping from the current knots first. Every moved raw
index must decode as a receiver derivative with its correct final named writer;
unchanged statuses and returned/query payloads are decoded, never blessed as
derivatives. Original binary64 inputs are converted exactly. The four-term
counterfactual selects changed indices only and supplies no expected value.

All 24 components satisfy the unchanged `1e-9` relative rule; maximum error
`1.999548313015847e-12`, maximum relative precision drift
`1.1300118342727895e-76`. No new absolute floor is used. The scope is explicit:

- `gen0686`, `gen0803`: changed jets occur **before** their later swap. Decoding
  the unchanged suffix permits the complete row, but does not pretend these
  changed jets exercise post-swap derivatives.
- `gen2531`: the changed jet precedes the final reverse, with the same limitation.
- `gen0909`: the changed jet follows swap; transposition and the negated distance
  are both required.
- `gen2960`: the changed jet follows reverse in v; the reflected knots/net and
  unchanged distance are both required.

## Exact replacement encodings

Each new row equals the complete retained actual collector encoding. Common
fields remain `deferred=null`, `error=null`, `ints={}`, `topology=null`; `out.n`
stays 204, 108, 147, 87 and 86 respectively. Only the blocks below change.
They are `[sum, abs_sum, weighted_sum]`, not error bounds; errors grade all raw
moved components. Input hashes use sorted compact Python JSON, UTF-8.

| Case | Components / max relative error | Input SHA-256 | Old → new checksum blocks |
|---|---|---|---|
| `gen0686` | 6 / `1.99955e-12` | `9d08376d9c36e5392c5d09ca19e7a42d951006834726e090c52d2cd7fdb10dd9` | `[[121930.35080477943, 122286.7144167371, 8701121.039546246]]` → `[[121962.3389298341, 122224.82699030975, 8705193.75695359]]` |
| `gen0803` | 9 / `2.23887e-15` | `1c9b9b7c8465da58ee4de50b4a92ba6086af384f7d7967df546742c35243d4a8` | `[[30433.220442247508, 30463.821946298885, 503538.46555445157]]` → `[[30433.06458783185, 30463.6612749778, 503524.44087460113]]` |
| `gen0909` | 3 / `5.29685e-13` | `4bb845ec023ff9aa5ed7af2f2175f1dc52125b80adbb1a3f9a708a8c9fb7c57a` | `[[76256.28041470288, 76301.39173228155, 4336610.869009565]]` → `[[76255.8695864608, 76301.01981243513, 4336553.392578878]]` |
| `gen2531` | 3 / `1.30999e-14` | `be2221978e98e31d856b00abc7753267057b708f92496c105d0271c8ae329689` | `[[45825.10682569999, 45897.16292421347, 1451228.6467945287]]` → `[[45826.31522262935, 45897.31002906505, 1451250.566334015]]` |
| `gen2960` | 3 / `4.3237e-14` | `0cad4c75b7b482c04b0a69061ef20d3065743f94ab5eef5696ea3b4903b70865` | `[[30413.486952040126, 30468.854227191732, 1445253.6014927593]]` → `[[30413.479977829935, 30468.838392777052, 1445253.4163972281]]` |

All five original inputs and every raw binary64 output bit were checked against
the retained streams before editing. Every other row and all 86 known failures
stay byte-identical. All 3,182 cases remain selected as 3,096 golden rows plus
86 known failures.

## Controls and retained evidence

All 59 numerical/scope controls pass. The seven new controls cover an asymmetric
plane's closed-form values and first partials through swap/double swap, each
reverse and compositions; nonuniform interior-knot reflection on a piecewise
linear plane; wrong-value/changed-status rejection; failed edits and allowed
extension rejection; the previous copy decoder staying strict by default; and
the actual retained `gen0686` with unchanged input followed by a deliberately
wrong component. The affine closed-form controls compute the parameter map and
constant cross product, not a transformed control net or Taylor jet.

The earlier decoder gains an explicitly opt-in parameter-edit trace only; its
default still rejects swap/reverse, so previously archived proofs do not silently
acquire unsupported geometry mutations. No production affordance is introduced.
No B-rep, validity, volume or global regularity claim follows from these jets.

The raw collector was actually measured at
`7a90407a0fe189ebceb551e5df28aaf4e8cc9893`, raw-stream SHA-256
`735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
At pre-edit head `ece73364cc38f4aa9230978e62af78132b0c846a`, numerical source
SHA-256 remains
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
These measurements are not relabelled as a newer collector run.

The 40-member executable/raw/control archive was read back and hash-verified,
SHA-256 `9887e283e19139ec82596e30a378753dc2c6b55fb6930dce9157d2243b5bab2d`.
It retains complete original inputs/raw values/bits, full-precision observations
and disagreements, all controls, exact old/new encodings and the previous
strict helper source. Local path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-parameter-edits-input-oracle-20261108-ece73364c/`.

Projected accounting is 157 supported replacements / 771 moved components and
200 remaining ordinary differences, pending the actual gate. Parent canonical
run 42470822199 stopped at `action_required` with zero jobs, not success. The
correctness and attribution-review holds remain; no workflow approval, rerun,
review resolution or readiness is implied.
