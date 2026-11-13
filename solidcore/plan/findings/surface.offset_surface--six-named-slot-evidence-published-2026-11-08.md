# surface.offset_surface: six published named-slot proofs retained (2026-11-08)

Historical excerpt from #1914's published description, read back at
`beb1eb1843a2e988eb42ab169467494395fc6047`. The section below is retained
verbatim so the PR can link it without dropping any published per-case detail.
Its collector revisions, nine controls, counts and archive hash remain historical
measurements, not new executions. This preservation changes no production file,
golden, failure entry or disposition, and resolves no review or CI hold.

## Six additional wrong named-slot results

These are not transferred by case name from an old trace. The current profile-matched collector at `2a187b487004d6d31a40e63e8bf16c97517dee53` reproduces all 3,182 complete input/raw-output records byte for byte (SHA-256 `735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`). The disposition commit changes only the two expected-data files.

For each additional case, a successful unequal upper-half request returns a named partial overwritten by another partial in the inspected flat write order. Independent 80/160-digit Cox-de Boor/Taylor evaluation from its input net confirms **21 wrong scalar components** under the unchanged relative rule. Request shape alone is not proof: the returned named value must independently disagree. Zero-distance bypasses, failed steps, clamped/extended parameters, nonregular tangents, interior knots and edited/unsupported script forms are excluded.

| Case | Input SHA-256 | Step / output index / wrong named slot |
|---|---|---|
| `gen0033` | `70780a5c41ad46c90f83d5f576df88d61a7714aacf419d3909471911b1361dea` | `step 3, out[67]: D(2,0) overwritten by D(1,1)` |
| `gen0351` | `4b95c449950c93ce3cd46293ea0557556f3b4801767e67d3a3cc326722446cc9` | `step 3, out[73]: D(2,0) overwritten by D(1,1)` |
| `gen0690` | `007587d1c0dd72171f7817ca8b70124a16071d41dbbd6b873a4a253aa5b96ef6` | `step 1, out[62]: D(2,0) overwritten by D(1,1)` |
| `gen1110` | `d7b12a3aa2f98cf9d6204c234379b7a897e18ae42e9d6c67ecddc5cf557d4f67` | `step 1, out[9]: D(2,0) overwritten by D(1,1)` |
| `gen1310` | `e9c4f52efed32ca8c5470cc6cecb59828ab3872d2e25614d5d0ff90aee05d13b` | `step 0, out[8]: D(2,0) overwritten by D(1,1)` |
| `gen1643` | `18c66e2311bd6262fe0f848206b7065ed0b349560a682daab876fffa6c173a41` | `step 1, out[69]: D(2,0) overwritten by D(1,2)` |

The second-order D(2,0)←D(1,1) alias is the same requested-rectangle defect as the third-order aliases, not another numerical correction. All nine oracle controls pass: the five original controls plus a wrong-slot control, a correct-slot negative control, zero-offset exclusion and failed-step exclusion. Full precision observations and code are retained in the verified 11-member evidence archive, SHA-256 `3e8e88ffcba10c6985f9548f13113530a0617c08baaaf6c0bc8f468599cb3aa9`.

<details>
<summary>All 21 new component observations (display rounded; proof uses full precision)</summary>

| Case | Step / index | Named partial / axis | Last writer | Observed | Independent expected (17 significant digits) | Relative error |
|---|---|---|---|---|---|---|
| `gen0033` | `3 / 67` | `D(2,0) / x` | `D(1,1)` | `0.018724977469827681` | `0.94947706571689405` | `0.9802786` |
| `gen0033` | `3 / 68` | `D(2,0) / y` | `D(1,1)` | `-0.00013649133486782865` | `-0.46554174393040504` | `0.9997068` |
| `gen0033` | `3 / 69` | `D(2,0) / z` | `D(1,1)` | `0.03180708635890208` | `2.8042190423216997` | `0.9886574` |
| `gen0351` | `3 / 73` | `D(2,0) / x` | `D(1,1)` | `0.45960729386454163` | `-2.5116317991687791` | `1.182992` |
| `gen0351` | `3 / 74` | `D(2,0) / y` | `D(1,1)` | `0.093772116550005127` | `-0.50950947374403088` | `1.184044` |
| `gen0351` | `3 / 75` | `D(2,0) / z` | `D(1,1)` | `-0.20616503773553352` | `0.8025178110146608` | `1.256898` |
| `gen0351` | `3 / 76` | `D(3,0) / x` | `D(1,2)` | `-0.074509313044287567` | `-6.3156316792075415` | `0.9882024` |
| `gen0351` | `3 / 77` | `D(3,0) / y` | `D(1,2)` | `-0.015739847674017218` | `-1.122898963269378` | `0.9859828` |
| `gen0351` | `3 / 78` | `D(3,0) / z` | `D(1,2)` | `0.032706421316059255` | `2.7671039657496665` | `0.9881803` |
| `gen0690` | `1 / 62` | `D(2,0) / x` | `D(1,1)` | `-0.019052016500627499` | `-0.7775375111457119` | `0.975497` |
| `gen0690` | `1 / 63` | `D(2,0) / y` | `D(1,1)` | `-0.0161844375287322` | `0.44902605842363585` | `1.036043` |
| `gen0690` | `1 / 64` | `D(2,0) / z` | `D(1,1)` | `-0.031707902133859094` | `-2.0671257497366193` | `0.9846609` |
| `gen1110` | `1 / 9` | `D(2,0) / x` | `D(1,1)` | `0.14139133719370992` | `0.010038776196591289` | `0.9290001` |
| `gen1110` | `1 / 10` | `D(2,0) / y` | `D(1,1)` | `0.049876113749574992` | `-0.24302254799445155` | `1.205232` |
| `gen1110` | `1 / 11` | `D(2,0) / z` | `D(1,1)` | `0.088310451664655662` | `0.067595805569719364` | `0.2345662` |
| `gen1310` | `0 / 8` | `D(2,0) / x` | `D(1,1)` | `0.053073370992377245` | `0.092257247937169787` | `0.4247241` |
| `gen1310` | `0 / 9` | `D(2,0) / y` | `D(1,1)` | `-0.10517003905227755` | `-0.035204568600612281` | `0.6652605` |
| `gen1310` | `0 / 10` | `D(2,0) / z` | `D(1,1)` | `-0.087096203244838494` | `0.11288825209382408` | `1.771526` |
| `gen1643` | `1 / 69` | `D(2,0) / x` | `D(1,2)` | `-1.2098301821953668` | `21.441414232216133` | `1.056425` |
| `gen1643` | `1 / 70` | `D(2,0) / y` | `D(1,2)` | `-2.0784483827338414` | `35.121179153073513` | `1.059179` |
| `gen1643` | `1 / 71` | `D(2,0) / z` | `D(1,2)` | `1.5222138413884911` | `-23.137063892159638` | `1.065791` |

</details>
