# Offset derivatives: input-only canonical revolution evidence (2026-11-08)

Continues #1914's golden dispositions, not #2717's implementation. The four
numerical corrections are unchanged. The thirteen replacements below have
independent value evidence; `gen1603` is instead a demonstrated wrong result,
recorded against #2717 and never blessed. The other ordinary differences and the
unresolved attribution review remain a draft hold.

## Provenance and independent construction

The complete 3,182-case collector was actually measured at
`7a90407a0fe189ebceb551e5df28aaf4e8cc9893`, with raw-stream SHA-256
`735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
The subsequent data-only commits through `2ef53c83e` leave numerical source
SHA-256 `86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`
unchanged. Those measured revisions are not relabelled as newer executions.

The oracle reads only the original canonical B-spline generator, its weights and
knots, and the input axis and origin. It never reads a kernel-generated control
net or kernel derivatives. For unit axis z, origin o, and generator point p,
write c = o + ((p-o) dot z) z, r = p-c, t = z cross r. The exact full-turn
rational circle has Euclidean control points
`(1,0),(1,1),(0,1),(-1,1),(-1,0),(-1,-1),(0,-1),(1,-1),(1,0)`, alternating
weights 1 and `sqrt(1/2)`, degree 2 and doubled quarter knots. Its tensor-product
homogeneous control points are `(w(c+a r+b t),w)`, multiplying each circle weight
by its original generator weight. Linear affine rotation commutes with the
rational generator sum, proving that this net is the original generator revolved
in that parametrisation. Parameter swapping transposes it and reverses the
normal. This is the standard rational-quarter-circle identity, not a fitted net
(see `docs/definitions-and-certificates.md`, exact nets, and Piegl and Tiller,
*The NURBS Book*, rational circles and surfaces of revolution).

At separately constructed 80- and 160-digit nets, the independent Taylor algebra
differentiates `O = S + d (Su x Sv)/|Su x Sv|` through order three. Every input
binary64 is converted exactly. The retained old-term counterfactual selects the
moved indices only; it provides no expected value. All moved fields must be
decoded derivative components, with the correct final named-slot writer.
Only explicit scalar setters and nonassigning returned copies are modelled.
For `gen0078` and `gen2174`, extension is disabled throughout and the original
knots determine the API's clamped evaluation point before differentiation.

Every one of the 62 moved components passes the unchanged `1e-9` relative rule,
with maximum discrepancy `3.688949523297068e-12`. Maximum relative precision drift
is `2.0466689257358722e-76` within the jet calculation and
`2.1099858277606303e-76` across separately constructed nets. No absolute floor is
added. All thirteen retained input hashes and every raw float's binary64 bits
were checked before editing an expectation.

## Thirteen exact row replacements

Each new row is the original collector's complete encoding. The checksums below
are `[sum, abs_sum, weighted_sum]`, not bounds. The error column grades every raw
moved derivative component, not merely these checksums.

| Case | Components / max relative error | Old → new checksum blocks |
|---|---|---|
| `gen0078` | 9 / `1.56099e-14` | `[[60992.32232773774, 62007.82172754136, 3573365.621681268]]` → `[[60955.3429329313, 61630.4439708014, 3570310.42044418]]` |
| `gen0171` | 3 / `2.40474e-14` | `[[30460.067354949344, 30789.234141913264, 1571002.9046867639]]` → `[[30170.97766234359, 31175.217379802565, 1563841.1838079793]]` |
| `gen0510` | 2 / `3.49776e-15` | `[[136783.91504548964, 136940.61322075027, 12115341.037492398]]` → `[[136755.1149473944, 136912.07291654628, 12111572.593217228]]` |
| `gen0688` | 3 / `1.9788e-15` | `[[31346.461684132, 32403.592700681234, 1772295.8567843311]]` → `[[30781.309826793167, 32510.58038801976, 1753301.5773981945]]` |
| `gen0913` | 3 / `1.51772e-13` | `[[61750.73067260197, 61840.21030969888, 3698631.549175109]]` → `[[61633.6609800661, 61723.14061716303, 3688650.729457595]]` |
| `gen0965` | 6 / `3.92942e-16` | `[[31437.46049255547, 31844.361634149758, 3185201.893145722]]` → `[[31020.49497003035, 33310.32052049644, 3179826.550622901]]` |
| `gen1021` | 3 / `1.32677e-14` | `[[32022.521603079706, 32221.489579301622, 622051.0166188008]]` → `[[32040.91340095389, 32239.881377175807, 623490.583575049]]` |
| `gen1336` | 3 / `1.80575e-15` | `[[30508.18586005638, 30627.932843864983, 508130.084364765]]` → `[[30493.239429126246, 30634.94737160066, 507062.21452469326]]` |
| `gen1523` | 3 / `3.02139e-15` | `[[106762.79310347415, 107249.71947489904, 8025630.370844599]]` → `[[108515.03086142626, 109001.95723285116, 8245925.889540591]]` |
| `gen2083` | 3 / `9.18182e-14` | `[[76180.35956883941, 76708.58602231681, 3861956.0360428616]]` → `[[76207.07582306274, 76693.32645472689, 3865576.97943913]]` |
| `gen2174` | 9 / `3.68895e-12` | `[[30567.472013004335, 30885.59102480571, 513955.18919101433]]` → `[[30404.688016921584, 30612.68300826498, 501435.5076105441]]` |
| `gen2176` | 6 / `9.06304e-14` | `[[30807.70523767886, 31289.921984959317, 534969.6617323706]]` → `[[30716.53174207574, 31302.716083319912, 528245.5167390216]]` |
| `gen2792` | 9 / `5.18444e-15` | `[[61142.0736249404, 61506.22354610884, 3704443.051684439]]` → `[[60914.84419372687, 61285.2585890741, 3688211.2636018256]]` |

## One demonstrated wrong named result: gen1603

The unchanged first three script steps decode to exactly the first 35 raw
scalars. Step 2 evaluates `(hu,hv,upper)=(3,1,true)` at
`[0.0,0.5056932298525347]`. The input SHA-256 is
`731f3240c0a95f5d4885d50f3f19971f9c695729a9cc2d7bf85a268b03863cd7`
(sorted compact Python JSON UTF-8). The original input, exact prefix values/bits
and both independently constructed nets are retained in the archive below.

`plain_or_simple` writes `D(2,0)` at `2*(hv+1)=4`, then overwrites that slot at
`(hv+1)+2=4` in the `D(1,2)` branch. Its `cvv/nvv` dependencies were not assigned
because `hv<2`. The serializer still names the slot `D(2,0)`. Both the ordered
source writes and the independent named derivative establish the defect:

| Raw index | Named component | Observed | Independent expected | Observed binary64 bits |
|---|---|---|---|---|
| 27 | `D(2,0).y` | `-682.320875639607038465328514575958251953125` | `98.17705633871186130273814537770736337787356829969092636818019892066445767973323701654830201015106781171705963251994176157046773168970297790093474259243937981360` | `13872585010566877584` |
| 28 | `D(2,0).z` | `-274.23736224240929004736244678497314453125` | `-62.64369053230619373421032002083453895521648140114104432268252594384290426475072163262198550190050666582318223507349155873132572855942707179898455707666512398502` | `13866904087743285056` |

Relative discrepancies are `1.143886930392802` and `0.7715712767214668`, versus
the unchanged `1e-9` rule. Separate-net precision drift is below `3.169e-79`.
This proves a wrong returned named derivative; it does not grade the suffix or
claim that the overwriting expression is a correct `D(1,2)`. Its old golden has
length 46 and block `[[3650.1057913563527,6286.665825084098,118136.81376043486]]`.
That line is removed, not replaced by another wrong result. The complete case
still executes through its new issue-backed `known_failing.json` entry.

## Controls, exclusions and retained executable evidence

All 48 controls pass: the prior numerical/scope controls; cylinder point,
tangent and circle identities; normal orientation on parameter swap; translated,
non-Z and scaled axes; a rational generator's fractional-linear value and first
three derivatives; explicit clamp versus allowed extension; wrong-value rejection;
and the actual `gen1603` prefix's independently derived nets and input/bit identity.
Restoring the two named values to their independent answers eliminates those
wrong-value findings. Tampered bits and an unmodelled prefix edit are refused.
The exact-zero closed-form controls compare 95 digits of a 100-digit Decimal
calculation to accommodate its last rounding digits; captured values still use
strict relative `1e-9` with no added floor.

The nine independently disagreeing revolution cases remain unresolved:
`gen0729`, `gen0935`, `gen0993`, `gen1057`, `gen1488`, `gen2359`, `gen2588`,
`gen2653`, `gen2794`. Constructor frames, analytic/arc-length generators,
near-axis control points, unsupported edits/extensions, ambiguous knot sides
and actual aliases are outside the replacement oracle. No B-rep topology,
volume, global regularity or fold certificate is claimed for these jets.

The 35-member archive was read back and hash-verified before the edits, SHA-256
`9266f4152921f181300f3811f269622338fec6b672311413a86764fece3d7a0c`.
It contains executable Python models/tests, full-precision observations and
exclusions, original raw/counterfactual streams, both alias proof nets, the old/new
table and pre-edit identity receipt. Local retained path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-canonical-revolution-20261108-2ef53c83e/`.
After these dispositions the expected accounting is 147 supported replacements /
721 moved components, 3,096 ordinary rows plus 86 known failures, all 3,182 cases
retained, and 210 ordinary differences. Those are projected counts until the
actual gate runs. Parent canonical run 42463902198 ended `action_required`, not
success. No workflow approval, rerun, review resolution or readiness is implied.
