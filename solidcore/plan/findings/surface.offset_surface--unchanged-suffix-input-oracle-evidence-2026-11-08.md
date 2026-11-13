# surface.offset_surface: seven input-only dispositions, unchanged suffixes (2026-11-08)

Continues #1914 with seven supported replacements / 29 moved components. Two
scripts have complete-script proofs; five use decoded prefixes covering **every
moved bit**, with the entire remaining output bit-identical to the four-term
counterfactual. This proves no correctness of the unchanged suffix. No original
input, production execution, case selection, tolerance, absolute floor or known
failure changes. #2717's implementation and refusal-versus-mask contract remain
separate.

## Independent values and scope

The existing independent 80/160-digit Cox-de Boor/Taylor evaluator constructs
`O = S + d (Su x Sv) / |Su x Sv|` from the original control points, weights,
knots and distance, with binary64 inputs converted exactly. It covers all 29
moved scalar components with the unchanged relative `1e-9` rule and no absolute
floor. Maximum relative discrepancy is `7.941402674945828e-14`, maximum relative
precision drift `9.382196956836735e-78`. The counterfactual supplies changed
indices only, never mathematical expected values.

The proof first tries the **complete script**. If that model produces a numerical
disagreement, it returns that disagreement rather than trying a shorter proof.
Only a script outside the complete decoder's scope may use a prefix. A private
copy of the evidence record ends at an explicitly decoded operation boundary;
the complete production case still runs. Full original input identity, raw
output length and value/bit agreement are checked before any prefix is made.
All moved indices must decode as independently graded receiver jets before the
boundary. Every suffix bit, including statuses and payloads, must equal the
counterfactual. Extension-enabled scripts and potential named-slot aliases
**anywhere in the full original script** are refused. The prefix returned-copy
decoder accepts only unambiguous offset kind `15400`. Default decoder callers
still reject incomplete records; no production decoder is changed.

Case boundaries (steps and scalar ranges are zero-based, half-open):

- `gen0294`: **complete**, 7 steps / 139 scalars. The changed `D(0,3)` is at step
  2, before the unchanged inversion query. The original revolution generator's
  rational full-turn net is independently constructed, with explicit parameter
  clamping. No kernel-generated net or inversion result supplies an expected jet.
- `gen1956`: **complete**, 2 steps / 68 scalars. The changed `D(0,3)` is at step 1.
- `gen0046`: steps `[0,4)`, scalars `[0,131)`; moved step 1 precedes trim and a
  returned offset in the ungraded 56-scalar suffix.
- `gen0715`: steps `[0,4)`, scalars `[0,140)`; moved step 2 follows a **returned
  copy**, not a receiver replacement. Trim/inversion form the ungraded six-scalar
  suffix. Eight rather than nine fields move; every actually moved bit is graded.
- `gen1107`: steps `[0,4)`, scalars `[0,101)`; moved step 3 follows copy and
  precedes trim in the ungraded five-scalar suffix.
- `gen1871`: steps `[0,4)`, scalars `[0,98)`; moved step 1 precedes trim in the
  ungraded five-scalar suffix.
- `gen2606`: steps `[0,2)`, scalars `[0,22)`; moved step 0 precedes transform and
  a later evaluation in the ungraded 19-scalar suffix. **No post-transform
  derivative or transform-correctness claim** is made.

## Exact old/new encodings and original input identities

Each new row equals the **complete** retained collector encoding, not an encoding
of a cropped case. Common fields stay `deferred=null`, `error=null`, `ints={}`,
`topology=null`. `out.n` stays 187, 139, 146, 106, 103, 68 and 41 respectively.
Only the following `[sum, abs_sum, weighted_sum]` blocks change. These blocks are
checksums, not error bounds; every moved raw scalar is graded separately below.
Input hashes use sorted compact Python JSON, UTF-8, of the full original input.

| Case | Proof steps / raw scalars; unchanged suffix | Components / max relative error | Input SHA-256 | Old → new checksum blocks |
|---|---|---|---|---|
| `gen0046` | `4/6 steps, 131/187 scalars; 56` | 3 / `1.37231e-15` | `dbd153151a9ece86d7374df7e75c3b8fd643697463040a2c9b0aca8d7078246c` | `[[77121.34624082278, 78371.89588151565, 7204413.02628398]]` → `[[77127.56162394208, 78413.58757987301, 7204937.948856801]]` |
| `gen0294` | `7/7 steps, 139/139 scalars; 0` | 3 / `6.07478e-15` | `8e631a42bc095bbf10e999e257af4b43316e25bff9aa5b110989c4598f8075d1` | `[[131571.70347178396, 142646.29819385172, 8950770.303338341]]` → `[[131585.2632002976, 142668.59266738236, 8951797.094917087]]` |
| `gen0715` | `4/6 steps, 140/146 scalars; 6` | 8 / `7.9414e-14` | `3095b349ba413411105bf66000545935862075cf6b359fe85ebb31e65a068ad5` | `[[77220.71046083543, 77311.38053866001, 3702379.153818082]]` → `[[77227.07637822005, 77297.60213367597, 3703184.8790371185]]` |
| `gen1107` | `4/5 steps, 101/106 scalars; 5` | 6 / `6.85615e-15` | `9ea3bc28cade8ee485e49065c45b30dbad3f665b0e575a3b437e41e607b4185f` | `[[46154.69065168403, 46690.47230266395, 1392869.5927645955]]` → `[[46145.861751782475, 46681.36711117941, 1391635.70220002]]` |
| `gen1871` | `4/5 steps, 98/103 scalars; 5` | 3 / `3.18996e-15` | `b1f76deb657f9c21511623f227d889bc3682961414b54382f3dc3db102abd84d` | `[[30452.32264385231, 31097.267325821787, 505666.83287312003]]` → `[[30457.86551369243, 31094.125835654002, 506083.6793920865]]` |
| `gen1956` | `2/2 steps, 68/68 scalars; 0` | 3 / `9.69707e-15` | `025de42ef6f9465e60ab68ca104811b32a4bc5d9bd59e3289a451edb3d5fd2ff` | `[[31752.321348733272, 33425.56012012197, 595083.5858024094]]` → `[[31972.454180961, 33392.31834752314, 609529.5325767717]]` |
| `gen2606` | `2/5 steps, 22/41 scalars; 19` | 3 / `2.37756e-15` | `90597c56d3e9efbe5c336f44a7b90af5dcbe4059a4667c1a442afa29d7ea3c79` | `[[3.6721180340205315, 9.99814166857267, 125.20427053028689]]` → `[[3.8326148961392716, 10.356535523557282, 128.5298394660693]]` |

All 29 observations (display rounded to binary64 precision; decisions and archived
expected values use full decimal precision). Axis 0/1/2 means x/y/z:

| Case | Step / raw index | Partial / axis | Observed | Independent expected | Relative error |
|---|---|---|---|---|---|
| gen0046 | 1 / 71 | (1,2) / 0 | -28.380060620125426 | -28.380060620125423 | 9.451e-17 |
| gen0046 | 1 / 72 | (1,2) / 1 | 6.367558855092271 | 6.367558855092262 | 1.372e-15 |
| gen0046 | 1 / 73 | (1,2) / 2 | 31.966781891409557 | 31.966781891409568 | 3.876e-16 |
| gen0294 | 2 / 74 | (0,3) / 0 | 34458.23563946715 | 34458.2356394673 | 4.177e-15 |
| gen0294 | 2 / 75 | (0,3) / 1 | -3052.6896165881717 | -3052.6896165881813 | 3.111e-15 |
| gen0294 | 2 / 76 | (0,3) / 2 | 38200.35328405772 | 38200.353284057484 | 6.075e-15 |
| gen0715 | 2 / 120 | (0,3) / 0 | -0.0009522569330325635 | -0.000952256933032534 | 3.094e-14 |
| gen0715 | 2 / 121 | (0,3) / 1 | -0.008402533916048624 | -0.008402533916048654 | 3.551e-15 |
| gen0715 | 2 / 122 | (0,3) / 2 | -0.002305644043414734 | -0.0023056440434147387 | 2.071e-15 |
| gen0715 | 2 / 129 | (1,2) / 0 | -0.10535987629543768 | -0.1053598762954376 | 7.396e-16 |
| gen0715 | 2 / 130 | (1,2) / 1 | 0.006463147274761187 | 0.006463147274760673 | 7.941e-14 |
| gen0715 | 2 / 131 | (1,2) / 2 | -0.032374004940173076 | -0.032374004940173215 | 4.273e-15 |
| gen0715 | 2 / 136 | (2,1) / 1 | 2.3190759575134994 | 2.319075957513507 | 3.178e-15 |
| gen0715 | 2 / 137 | (2,1) / 2 | -2.6345573074983024 | -2.634557307498287 | 5.822e-15 |
| gen1107 | 3 / 89 | (1,2) / 0 | 36.912671780944954 | 36.91267178094503 | 2.167e-15 |
| gen1107 | 3 / 90 | (1,2) / 1 | -38.609716645925296 | -38.609716645925225 | 1.802e-15 |
| gen1107 | 3 / 91 | (1,2) / 2 | -39.64200472819031 | -39.642004728190365 | 1.385e-15 |
| gen1107 | 3 / 95 | (2,1) / 0 | 25.86625615297529 | 25.866256152975268 | 8.698e-16 |
| gen1107 | 3 / 96 | (2,1) / 1 | 26.864676213724515 | 26.8646762137247 | 6.856e-15 |
| gen1107 | 3 / 97 | (2,1) / 2 | -16.43263921285802 | -16.43263921285799 | 2.054e-15 |
| gen1871 | 1 / 74 | (1,2) / 0 | 0.8535555622602854 | 0.8535555622602853 | 9.382e-17 |
| gen1871 | 1 / 75 | (1,2) / 1 | -0.6555910077224903 | -0.6555910077224882 | 3.190e-15 |
| gen1871 | 1 / 76 | (1,2) / 2 | 0.8089661538410308 | 0.8089661538410307 | 1.672e-16 |
| gen1956 | 1 / 65 | (0,3) / 0 | -632.537752764933 | -632.5377527649268 | 9.697e-15 |
| gen1956 | 1 / 66 | (0,3) / 1 | 1400.051215352398 | 1400.051215352393 | 3.556e-15 |
| gen1956 | 1 / 67 | (0,3) / 2 | 853.8463232765255 | 853.8463232765242 | 1.402e-15 |
| gen2606 | 0 / 17 | (1,2) / 0 | -0.01668319697464882 | -0.01668319697464886 | 2.378e-15 |
| gen2606 | 0 / 18 | (1,2) / 1 | -0.08226529945828799 | -0.082265299458288 | 2.328e-16 |
| gen2606 | 0 / 19 | (1,2) / 2 | 0.25944535855167694 | 0.25944535855167716 | 8.984e-16 |

The complete suffix digests are SHA-256 of each raw u64 bit pattern encoded
big-endian, concatenated in output order:

| Case | Suffix scalars | Suffix bits SHA-256 |
|---|---|---|
| gen0046 | 56 | `f25d4910341087acdf5c2f761cf5f654286070e583fbe8a04fefea9aeb6b875c` |
| gen0715 | 6 | `7d995755fcb29d284d08fbf128459ab6bd6db711d77d2f29ebb960be9edecafb` |
| gen1107 | 5 | `d0c794b747c8caf28182702f2a59282c854fa40f4a50c1446f8df2f83359e15f` |
| gen1871 | 5 | `e4d7dca9a8dd98ef3dba9038fde2c7b9d591cedc2458b6654a85f29e2a92fc29` |
| gen2606 | 19 | `bb3ae3fb0a4f5fde38e896a01c336fc4c21d8d873090bd768a6b22921de55a1d` |

## Controls, provenance and limits

All **66 numerical/scope controls pass**. The seven added controls prefer the
complete-script proof, preserve the complete original record during prefixing,
reject a moved suffix, changed input/shape or tampered value/bit pair, reject a
wrong prefix value despite an unchanged tail and a potential suffix alias, keep
the default decoder strict, and bind the actual retained `gen0046` before
deliberately moving a suffix field. The 23 numerical disagreements in this scan
remain visible and unapproved; an unchanged suffix is never a correctness waiver.

The 44-member executable/raw/control archive was read back and hash-verified:
`ffbef76485eb2d56b266d1e1dad0a8c785d27f05dfe03023ed629b1439026afe`.
It retains all original inputs/raw values/bits, full-precision observations and
disagreements, exact old/new rows, controls, prefix boundaries/suffix digests and
the previous strict helper sources. Local path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-unchanged-suffix-input-oracle-20261108-8e07893cf/`.

The raw collector was measured at `7a90407a0fe189ebceb551e5df28aaf4e8cc9893`,
stream SHA-256 `735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`;
counterfactual stream SHA-256
`56cd28f6ad7ff18c4890469a148c0cfbaf49b70ff241ff9136cfbfee01d84dd1`.
The archive's pre-edit head is `8e07893cff99db33d90e3ed10d6551a87587f032`.
Before applying these seven rows the branch was refreshed onto main
`3ef79991375bfb6f5cdde7c80584d76589808498`, preserving every PR-owned file byte,
at `c8262aea0a1d8c77f4a718e037ef698ee56385eb`. Numerical source SHA-256 stays
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
These are not relabelled as new raw-collector measurements.

All other expected rows and all 86 known failures remain unchanged. All 3,182
cases remain selected as 3,096 golden rows plus 86 known failures. Projected
accounting is **164 supported cases / 800 moved components / 193 ordinary
differences**, pending the real full-case gate. This data-only disposition makes
no body-construction, topology, validity, volume, post-edit or global-regularity
claim. No full-suite or canonical-CI success, review resolution or readiness is
implied; the correctness and attribution-review holds remain.
