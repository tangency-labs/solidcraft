# surface.offset_surface: published migration evidence before the 78d1eb12d refresh (2026-11-08)

This record preserves **verbatim** the complete body published for #1914 at
`ceadc86fedb8d337812b4882175f4fbb0fd0b79c`, including its per-case tables,
old/new values, input identities, proof boundaries, archive hashes and measured
revisions. SHA-256 of the exact body below (UTF-8, before this wrapper):
`5c776c9f037551acaab9abc3df3c033e0f57046dd41366bd330aa130362f1e41`.

The published body was read back and checked before this archival copy was made.
Its historical heads, counts and pending-state statements are retained, not
updated to describe this record's commit or a later execution. This is an evidence
archive, **not a current readiness or CI claim**. The current PR description names
the latest measured head and links the subsequent complete `gen1320`/`gen1851`
proofs separately. No numerical code, expectation, input or case selection is
changed by copying the text.

## Verbatim published body
Units: `surface.offset_surface`, WR-160/WR-161. **Draft: 188 ordinary golden differences remain unresolved.**

This continues [the goldens migration instruction](https://github.com/tangency-labs/solidcore/pull/1914#issuecomment-6684465108), not the retired C++ gate. Head `ceadc86fedb8d337812b4882175f4fbb0fd0b79c`, linearly based on main `3ef79991375bfb6f5cdde7c80584d76589808498`. Both published and local `8e07893cff99db33d90e3ed10d6551a87587f032` are preserved locally and on the fork at `refs/archive/20261108-pr1914-before-3ef799913/{published,local}`; every one of the 17 PR-owned file bytes was preserved during refresh. Incoming ops/profile edits are disjoint. Earlier preservation refs and measured evidence remain intact. Earlier data-only commits replaced seven supported rows and move two proven wrong rows to #2717 failures, without implementation changes.

## Approved numerical scope, unchanged


The production correction still changes exactly four terms, unconditionally:

1. `Cuuv`: coefficient `+1`, not `3`, for `Buuv × Buv`.
2. `Cuvv`: coefficient `-1`, not `3`, for `Buvv × Buv`.
3. `Duvv` adds `Buvv`, not `Buuv`.
4. `Dvvv` adds `Bvvv`, not `Buuv`.

The default-kernel closed-form regression is retained, as are the unchanged-partial controls, WR-164 and extension tests. Numerical file SHA-256 is `86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`, unchanged throughout the migration. No new compatibility flag, tolerance, floor, fixture, generator, input guard or harness rule is introduced. Retired `known_mismatches` entries are not restored.

The #2717 requested-grid implementation remains separate. Its closed handoff #2970 and the later refusal-versus-mask design question are not silently absorbed here. Recording a demonstrated named-slot failure does not choose that design.


## Current bounded dispositions

- **167 supported golden replacements / 815 moved components**: the original 45 (199 components), 13 pass-through-wrapper cases (54), 26 non-aliasing-grid scripts (162), four nested-zero-identity cases (17), eight locally regular nested cases (36), twenty copy/setter scripts (89), eighteen clamped-boundary scripts (102), thirteen canonical-revolution scripts (62), five returned-base/inversion-query scripts (26), five parameter-edit scripts (24), seven complete/prefix-supported scripts (29), two enabled-but-unused extension prefixes (12), and one explicit-side-clamp script (3), independently checked from original inputs.
- **88 demonstrated wrong cases** are individually listed against #2717, without wrong golden lines: the original 79 exact-input/exact-bit attributions plus six additional direct-input proofs below, the canonical-revolution prefix `gen1603`, and the two query-prefix proofs `gen0757`/`gen2243`. These cases continue to execute; harness failures and panics are not waived.
- **All 3,182 cases remain selected**: 3,094 golden rows and 88 known failures, with disjoint identities. Every unrelated expectation and every previously listed failure is unchanged.
- **188 ordinary changed cases remain red**. This update replaces only the independently supported complete `gen1332` row; every other expected row and all 88 known-failure entries are unchanged. No numerical source, fixture, runner, tolerance, floor or requested-grid contract changes.

## Why the original 45 moved results are supported


The independent probe starts from the input homogeneous B-spline net, not derivatives supplied by the kernel. It evaluates Cox–de Boor basis polynomials as Taylor series, divides the homogeneous series, differentiates the base series, and forms `O = B + d (Bu × Bv) / |Bu × Bv|` through order three. The inverse square root is expanded by its binomial series. Input binary64 numbers are converted exactly with `Decimal.from_float`.

The calculation is repeated at 80 and 160 decimal digits. Each of the 199 moved components agrees with the independent result under the unchanged `1e-9` relative rule, without an absolute floor; precision agreement at `1e-50` is an additional numerical stability check, not a production tolerance. Accepted evaluations use original-domain parameters, regular tangents and the original base, not clamping, extension, pole extrapolation, substituted normals or aliased slots. A separate audit found no successful unchanged aliased request in these 45 scripts.

Five controls pass: basis partition/derivative sums, the API's interior-knot side convention, a parabolic graph's closed-form offset derivatives, a rational plane's fractional-linear parametrization, and rejection of a degenerate normal. These are numerical checks at the requested parameters, not a whole-domain approximation certificate or proof of unrelated unchanged operations. The moved outputs are third derivatives, not newly built B-reps; there is no changed topology or volume result being accepted here.

The definition follows [Shape Interrogation §11.3.1](https://web.mit.edu/hyperbook/Patrikalakis-Maekawa-Cho/node224.html) (`B + dN`) and [§3.1](https://web.mit.edu/hyperbook/Patrikalakis-Maekawa-Cho/node27.html) (the normal from parametric tangents). The probe uses independent Taylor-series arithmetic rather than the production third-order recurrence.

<details>
<summary>Each deliberately changed golden: old/new stored values and independent relative error</summary>

`blocks` are the existing golden encoding's `[sum, abs_sum, weighted_sum]`; `n` is unchanged. The error column grades the raw moved components, not those checksum blocks.

| Case | Old stored block(s) | New stored block(s) | Largest independent relative error |
|---|---|---|---|
| `api_offset_m12_state_evals` | `{"blocks":[[18555.43255227718,93499.06290384065,-843552.9123418084]],"n":247}` | `{"blocks":[[19461.556148815078,93729.6399627025,-657550.8157014829]],"n":247}` | `7.6827e-13` |
| `gen0000` | `{"blocks":[[15.578470146825442,66.19398803014818,149.0970835084266]],"n":31}` | `{"blocks":[[15.934947456660122,65.44373059469201,157.64058882512336]],"n":31}` | `2.4657e-15` |
| `gen0007` | `{"blocks":[[-66837181420.41156,135130570084.05176,-5960704060979.212]],"n":152}` | `{"blocks":[[-66859213497.91429,135177776516.49692,-5962348872759.839]],"n":152}` | `5.9725e-14` |
| `gen0068` | `{"blocks":[[15.319010631468199,23.388114014721587,351.8112104461945]],"n":38}` | `{"blocks":[[15.355483099501537,23.32098921480353,352.8991097478107]],"n":38}` | `9.1962e-15` |
| `gen0196` | `{"blocks":[[12.565184277392516,20.946060283902344,154.70633404669488]],"n":25}` | `{"blocks":[[12.576610099938545,20.823068612513566,154.88348912442416]],"n":25}` | `5.6728e-15` |
| `gen0203` | `{"blocks":[[61738.3549758855,61958.941867929476,4525927.325586328]],"n":168}` | `{"blocks":[[61723.3828120861,61945.84157921354,4524769.84162097]],"n":168}` | `1.4921e-13` |
| `gen0241` | `{"blocks":[[30402.471835876284,30530.224793782254,502734.7743859657]],"n":127}` | `{"blocks":[[30402.18860733752,30530.466737296207,502701.62180807826]],"n":127}` | `4.0998e-13` |
| `gen0255` | `{"blocks":[[30387.87660331008,30487.36593020654,501929.7715273392]],"n":93}` | `{"blocks":[[30387.892616568915,30487.379012093024,501931.26253273303]],"n":93}` | `2.1702e-14` |
| `gen0430` | `{"blocks":[[-27436.54375901606,167436.2705040306,-3197648.2647903794]],"n":128}` | `{"blocks":[[-27218.29986474127,167654.5143983054,-3181901.7985980776]],"n":128}` | `2.5459e-15` |
| `gen0453` | `{"blocks":[[30412.39559535699,30449.645002398775,1080775.5492491084]],"n":74}` | `{"blocks":[[30412.369114628094,30449.685228169474,1080775.0746664007]],"n":74}` | `6.8215e-14` |
| `gen0458` | `{"blocks":[[-224.78955409790183,1563.1038922851922,-2999.309792598596]],"n":32}` | `{"blocks":[[-138.86128799414814,1604.836296732452,-1000.5016409527848]],"n":32}` | `9.2360e-15` |
| `gen0506` | `{"blocks":[[31427.5453740134,31486.672659007047,562961.3295158537]],"n":77}` | `{"blocks":[[31427.527719028058,31486.6359684364,562959.9732961301]],"n":77}` | `1.2421e-12` |
| `gen0587` | `{"blocks":[[-19521938014.22505,98857485394.04991,870182291661.0085]],"n":72}` | `{"blocks":[[-7555521493.592014,86891068873.41685,1031988872414.2708]],"n":72}` | `4.4463e-15` |
| `gen0659` | `{"blocks":[[62883.466662419545,63105.28156632557,3921651.0161867053]],"n":147}` | `{"blocks":[[62871.85752263127,63101.502957452874,3920579.0618740157]],"n":147}` | `3.2199e-15` |
| `gen0660` | `{"blocks":[[35271.972121450366,36061.61966549416,827725.0473255506]],"n":68}` | `{"blocks":[[40718.075740712775,41507.72328475657,1201562.0899414327]],"n":68}` | `2.4452e-13` |
| `gen0716` | `{"blocks":[[990.8128971350529,1017.7459271366557,52788.08308444568]],"n":53}` | `{"blocks":[[990.8113538623086,1017.4578477405812,52788.04512916689]],"n":53}` | `2.3141e-13` |
| `gen0741` | `{"blocks":[[-816281.9636894614,4894071.395232069,-73501434.61234763]],"n":96}` | `{"blocks":[[-1004897.0756162175,5095644.327690265,-87266893.29733452]],"n":96}` | `1.7766e-15` |
| `gen0745` | `{"blocks":[[76913.34859873245,77048.86242362625,6728234.54079606]],"n":174}` | `{"blocks":[[76912.39803074404,77048.71396391184,6728127.085842954]],"n":174}` | `3.5550e-15` |
| `gen0794` | `{"blocks":[[61913.290765653765,61952.418624409256,3731820.215717172]],"n":142}` | `{"blocks":[[61913.507091023246,61951.88874275767,3731835.342500676]],"n":142}` | `4.3821e-15` |
| `gen0951` | `{"blocks":[[30390.801227006006,30473.045662677512,501425.9333438466]],"n":72}` | `{"blocks":[[30389.696414887367,30471.823367723846,501352.73130386946]],"n":72}` | `2.9624e-14` |
| `gen0959` | `{"blocks":[[30387.499252799174,30491.494320710906,501817.54335713683]],"n":94}` | `{"blocks":[[30387.61201196988,30491.694589446834,501825.6425615884]],"n":94}` | `8.9063e-16` |
| `gen0974` | `{"blocks":[[30407.00600431937,30461.501816059466,2205598.372543032]],"n":111}` | `{"blocks":[[30409.68831855572,30455.699700325687,2205638.436354326]],"n":111}` | `6.9307e-15` |
| `gen0985` | `{"blocks":[[-42.88321000418377,182.95455225825478,-1588.8293711760505]],"n":57}` | `{"blocks":[[-42.936953620743395,181.87538135998145,-1702.8033865412876]],"n":57}` | `8.6160e-15` |
| `gen1080` | `{"blocks":[[-18.454362512987302,5184.662452726535,687.3743878188707]],"n":27}` | `{"blocks":[[-18.45932901752286,5184.658938565483,687.3046015064483]],"n":27}` | `8.3005e-15` |
| `gen1188` | `{"blocks":[[881.5993752030242,1166.245851897853,-437.41215602270063]],"n":21}` | `{"blocks":[[877.2882770184451,1170.556950082432,-517.8514701984736]],"n":21}` | `5.9405e-15` |
| `gen1312` | `{"blocks":[[10.551665165939022,20.707273125398622,281.8292866098144]],"n":57}` | `{"blocks":[[11.372277661270065,20.54382823730145,302.4067750275329]],"n":57}` | `5.3235e-14` |
| `gen1416` | `{"blocks":[[78186.23114002944,78277.09957161486,4666439.935435823]],"n":131}` | `{"blocks":[[78186.02901103486,78278.70326175417,4666428.1500635985]],"n":131}` | `6.6129e-15` |
| `gen1423` | `{"blocks":[[106602.2990378796,106761.4768768358,8817730.82046103]],"n":183}` | `{"blocks":[[106602.78161245614,106761.7782564493,8817766.083273219]],"n":183}` | `1.2042e-14` |
| `gen1425` | `{"blocks":[[45390.45442824365,45460.2762228435,1968263.0034797394]],"n":83}` | `{"blocks":[[45390.903056125324,45459.827594961826,1968273.4955305809]],"n":83}` | `4.1320e-15` |
| `gen1540` | `{"blocks":[[30409.747976543957,30457.8435500296,502957.1232589204]],"n":84}` | `{"blocks":[[30407.19873457563,30454.111781707572,502766.6501428983]],"n":84}` | `3.0296e-14` |
| `gen1846` | `{"blocks":[[37874.79106367064,207276.11661343987,2316642.6551924506]],"n":141}` | `{"blocks":[[44235.750161099335,198222.83573974654,2431494.133546644]],"n":141}` | `9.2354e-14` |
| `gen1977` | `{"blocks":[[-2700462544.6541142,7650063175.587434,-278518087968.6768]],"n":106}` | `{"blocks":[[-2700462537.8681984,7650063125.205684,-278518087801.72485]],"n":106}` | `4.6555e-15` |
| `gen1979` | `{"blocks":[[45325.117151139006,45552.87716336386,2056746.2924211316]],"n":85}` | `{"blocks":[[45325.68512792134,45552.36068518827,2056753.967175954]],"n":85}` | `3.8490e-14` |
| `gen1998` | `{"blocks":[[10.401723128094405,26.01682301344319,10.440498386317746]],"n":52}` | `{"blocks":[[10.402186832562927,26.017778473254932,10.45411085670047]],"n":52}` | `1.7111e-13` |
| `gen2053` | `{"blocks":[[30466.356560189328,30474.530022372986,1265413.215443816]],"n":80}` | `{"blocks":[[30466.758255769062,30476.0661078709,1265420.614011899]],"n":80}` | `2.2881e-14` |
| `gen2324` | `{"blocks":[[60941.0770320113,61032.02632995147,3628110.931002712]],"n":141}` | `{"blocks":[[60941.08232995051,61032.04490793993,3628111.295936332]],"n":141}` | `9.3548e-14` |
| `gen2456` | `{"blocks":[[-43089878.01306949,272907235.8543918,-2929882439.745673]],"n":83}` | `{"blocks":[[-43044050.63786446,272735952.65986407,-2926549280.2746844]],"n":83}` | `2.2169e-14` |
| `gen2559` | `{"blocks":[[30419.616265478202,30468.86518848086,503458.000436155]],"n":72}` | `{"blocks":[[30419.17866536324,30468.068270721495,503427.94034354197]],"n":72}` | `6.4831e-15` |
| `gen2601` | `{"blocks":[[-16.876927756008865,22.344934551546142,-315.8855434005137]],"n":27}` | `{"blocks":[[-150.71510034840648,156.18190139886246,-3821.2927119357746]],"n":27}` | `6.5487e-15` |
| `gen2689` | `{"blocks":[[30452.303990406777,30478.42259376221,504989.6296579127]],"n":81}` | `{"blocks":[[30451.45941941629,30479.065646945714,504921.7154439036]],"n":81}` | `1.7721e-14` |
| `gen2761` | `{"blocks":[[34813.03608906995,62397.60277708933,-154463.55221105885]],"n":89}` | `{"blocks":[[40771.3320560674,68355.89874408676,303580.79629800393]],"n":89}` | `2.9374e-15` |
| `gen2839` | `{"blocks":[[30426.957276829016,30485.566248671214,1659541.598744629]],"n":93}` | `{"blocks":[[30426.898283505136,30484.909052469564,1659539.8893148112]],"n":93}` | `3.8787e-11` |
| `gen2864` | `{"blocks":[[88822500892.20778,107434832376.38777,10139325312740.799]],"n":211}` | `{"blocks":[[90106326379.56178,110220510077.56047,10253259242225.396]],"n":211}` | `5.6758e-14` |
| `gen2882` | `{"blocks":[[30414.18457633274,30474.510353873786,1080548.0221309138]],"n":74}` | `{"blocks":[[30414.174598576585,30474.548960054533,1080547.793490986]],"n":74}` | `1.6741e-14` |
| `third_derivative_outputs` | `{"blocks":[[57.238638693897954,112.81064583069951,4249.98436408646]],"n":146}` | `{"blocks":[[72.7128982159763,128.5625491631388,5686.325692315696]],"n":146}` | `2.6218e-12` |

</details>

<details>
<summary>The original 79 wrong cases: input identity and measured wrong named slot</summary>

The same inputs and exact binary64 values occur in the retained per-case attribution record. That record also gives the full steps and write histories. No claim is made that unrelated components in these scripts are all wrong or all right.

| Case | Input SHA-256 | Step / named → actual partial / output index | Measured value (bits) |
|---|---|---|---|
| `gen0086` | `4df48003f9f2bfde020811f4f9335a2658eb71f298930384a4cb5159c0e6d8ea` | `1: D(3,0) → D(1,2); out[30]` | `-11.550457408491006` (`c02719d58dae9e9c`) |
| `gen0111` | `4a60121f505fa13cef71003ef92dd5fab0c703839db47e367d6e3165c4fe3e8a` | `2: D(2,0) → D(1,2); out[178]` | `0.0012087372430256635` (`3f53cdcfbb687ce0`) |
| `gen0112` | `84cd0e109cba4a1e64787903c2a34be6933bb45ba1216427ccdd8c990790ce1e` | `0: D(2,0) → D(1,2); out[14]` | `124.91549873680505` (`405f3a97880386d9`) |
| `gen0124` | `d96b72dd177493230f3918d1748a97309951e5f6635cedfde8981cd06340ea7e` | `1: D(3,0) → D(1,2); out[18]` | `0.3176445149410608` (`3fd45449a8db3dad`) |
| `gen0166` | `4de065ccdf78222f7ffd35943b12fb4af04810684dcf834b3b4dfb78acaee4c7` | `2: D(3,0) → D(1,2); out[16]` | `-2636845626.9779134` (`c1e3a562475f4b11`) |
| `gen0201` | `983d07f41935094158ebfc8caca0b26bad95163d2da2d31ab1f19ebbfcb1b2c4` | `0: D(2,0) → D(1,2); out[14]` | `12576.920008190044` (`40c89075c2d41025`) |
| `gen0256` | `bf864366a1506c158adafc754548fa2a74beef0ebbcdf60fbd6ae603477a5b5a` | `1: D(3,0) → D(1,2); out[65]` | `13.795431244846744` (`402b9742c39daaab`) |
| `gen0411` | `31e9dfd22bcd2b77017977ec3b87d1fa59f6ba955920f59af994547b2cb45af1` | `3: D(3,0) → D(1,2); out[86]` | `-0.007866018108114324` (`bf801c0f1431589f`) |
| `gen0414` | `2dd0449248615a45f4791f2fc3c0c166a86ab58d616e601993c19ce687cb9502` | `4: D(2,0) → D(1,2); out[133]` | `-9.100532018925273` (`c0223378eecaf845`) |
| `gen0460` | `8fcb022acbe0aac5fe6ffbb3dde41ef49fb68938a5067d973954e8d64b96d9c9` | `2: D(3,0) → D(1,2); out[66]` | `0.048548910288894395` (`3fa8db671be4fc8e`) |
| `gen0495` | `f088ad03f45397c07eb445b4f56918b3a9cabbb1e72fd2c07a4ea338bbba5c2b` | `3: D(3,0) → D(1,2); out[127]` | `4.180980285512458` (`4010b952e55dfcac`) |
| `gen0500` | `e345d54c1e6e72d986b32da83a6d5f8b2695d2022d09c1daae0810900abc5f02` | `1: D(2,0) → D(1,2); out[68]` | `-2.218763810011797` (`c001c0073d8c6467`) |
| `gen0550` | `b7ac007f31e43c76b6486748de3735558a3fcd6110391dc27312d53deb4227c5` | `5: D(2,0) → D(1,2); out[96]` | `-4.772093172003938` (`c013169f97ace324`) |
| `gen0551` | `45f2b1c88c7a9dcac65d0f2b49d40f552723e10e4b2507da859ceb7355293e66` | `0: D(3,0) → D(1,2); out[11]` | `-0.0001900259283527403` (`bf28e8364b9b0448`) |
| `gen0554` | `312160ad42b1b5d983397a487db4b97ffadadf5a345a69bd2e685da4c7e5a0ba` | `1: D(3,0) → D(1,2); out[24]` | `-1.3575472366319557e-62` (`b31656a80b3da5a9`) |
| `gen0565` | `48acced4a5bc728e3559b502c6eaca17c65a6ed47d02cff39422e40ccc692e48` | `0: D(2,0) → D(1,2); out[14]` | `-0.014705512576740604` (`bf8e1dec7cb44290`) |
| `gen0615` | `eca9bbf3d85f8e5d2cc018e2452738e1b8511ffb9032fc1550adbfbe43d78b8f` | `2: D(3,0) → D(1,2); out[43]` | `-0.0017960939312878011` (`bf5d6d5d2c81d89a`) |
| `gen0640` | `e1442331b4ba3ca9ef116df39932839f80cbebfe8ff7e964f1a640ec14581d99` | `2: D(2,0) → D(1,2); out[123]` | `9.988881846404788e-31` (`39b4428623c24221`) |
| `gen0641` | `a5d1eca059347dbc383305efbcc9c3674d24200ff8a467e8cf41037a216d41c0` | `2: D(2,0) → D(1,2); out[123]` | `-16.703056407748573` (`c030b3fb813685fb`) |
| `gen0761` | `e012c720f0c16da34056943e47e6f044dfb49bfaeb7dd806119190b839738fe4` | `2: D(3,0) → D(1,2); out[46]` | `-16.42245763261416` (`c0306c262ef405fe`) |
| `gen0785` | `38f12468d5809e36112f37d6768a2e25703b84672b49683ac3097fdd1cff895f` | `1: D(3,0) → D(1,2); out[65]` | `-0.007167672776218397` (`bf7d5bd98298bad2`) |
| `gen0837` | `10a5333437bdbf27f94340d09ad9824813cac9c1fa760e8d0fbd36d7a497fa44` | `2: D(3,0) → D(1,2); out[25]` | `-187.0087335080898` (`c06760478b7e7400`) |
| `gen0863` | `fa41005408047a3e0681b0ea0f071e1cf700ed1f39b944a866bd57e5a209ce17` | `0: D(3,0) → D(1,2); out[11]` | `3.706065473084212e-61` (`33630ead97ff7122`) |
| `gen0869` | `796f377b44849d99840cbf312ccce4acb3749b64d211d657f1b8dd884c794f93` | `2: D(3,0) → D(1,2); out[25]` | `61.32131957593377` (`404ea920fff7199e`) |
| `gen0896` | `0fc6f501d9afb3a9851f62a3cdc4b95ff033ccc9c4584f0d9035722bc348568c` | `0: D(3,0) → D(1,2); out[11]` | `-0.06023639161848329` (`bfaed74de80f9514`) |
| `gen0942` | `36d9cef24efa4f0deab6774a647668379cbea77c09e136b80810097207b202d5` | `2: D(2,0) → D(1,2); out[75]` | `-5.527640947576007` (`c0161c4de8977a44`) |
| `gen0978` | `16da8e112063fa0637834b48ae14299553b5128f98d2565f972de3d42db39ec0` | `1: D(3,0) → D(1,2); out[65]` | `-5.043807100029481` (`c0142cdbc4b7d63e`) |
| `gen1010` | `ce0f6b44df5d55637efd2d4048caa97891e58a943a2b83e5b74fa9b1a5bfa399` | `3: D(2,0) → D(1,2); out[91]` | `-1.0154000817065179` (`bff03f1427f49045`) |
| `gen1033` | `4722dc0af92f48eb601d4065533ef921a4d367f880829db06ad0facaf199df82` | `3: D(2,0) → D(1,2); out[77]` | `1.4370933558807315` (`3ff6fe559a4ce7f6`) |
| `gen1058` | `d38499e54878f56a7a75253482399df0b938527e96bb825a87ce1db0165321ea` | `4: D(3,0) → D(1,2); out[88]` | `-3.3635704624734384` (`c00ae897a170eb8c`) |
| `gen1114` | `b3e5c8c3885ce9b3414b4f37d49911df82a862171a91162a023d5c95f9afab08` | `1: D(3,0) → D(1,2); out[30]` | `1.385161992635685` (`3ff6299f9f2084f5`) |
| `gen1119` | `617d1dec1c4266f77ddec40b5212915998fa4fcb9fcb3024e28e6344bdf00db1` | `2: D(3,0) → D(1,2); out[78]` | `-7.8701147674852e-62` (`b34030195f3a004f`) |
| `gen1121` | `06d247e3af554e7c8bf47b0ecf206a34c2ba491e6316dad25fc5231059ba9278` | `2: D(2,0) → D(1,2); out[72]` | `0.018811935995894622` (`3f93436fa781db29`) |
| `gen1167` | `0e3509e71a8f6a2e4d6791dee5fb13c971f9891eadeeac80c59a1b343bd94cd0` | `5: D(3,0) → D(1,2); out[110]` | `2.8324817610385094e-49` (`35da7e7244710fe4`) |
| `gen1174` | `3b9182dcc382644559160a3038e9d817c5d11e87ab00fc96dfe547af2ff31752` | `1: D(3,0) → D(1,2); out[65]` | `-363.06551171903754` (`c076b10c56042c56`) |
| `gen1265` | `2b7af8a5366ba896f1c6273ccc1ae800bd684fc77246b686ea482894b44fb529` | `1: D(3,0) → D(1,2); out[65]` | `-12.029373775659867` (`c0280f0a145ba36a`) |
| `gen1274` | `d6c7a046a1c0c2ad632bcdc22fc7ce65ebe08cac99c4f6488f65eba58a8ceeca` | `0: D(3,0) → D(1,2); out[11]` | `4.7236676238936495e-15` (`3cf546049a9cd7ef`) |
| `gen1333` | `0d18b6c9e131ebf8c60fc82f80a89454b6bad5ffd34c55497ffa96b41cd7eb44` | `1: D(3,0) → D(1,2); out[66]` | `-1.2669447513979307e-61` (`b34a0f3e257a30d1`) |
| `gen1360` | `04024341fe454e346c6ccf712b3f87ff443bf11522f4fbbef5a8cf72e1eb58f4` | `2: D(2,0) → D(1,2); out[28]` | `0.28564080963923255` (`3fd247f063f36bfa`) |
| `gen1363` | `59bdccb9b96cfaf526f3814c7e5d423437311a542123a60300aba7bd580f5cfc` | `3: D(2,0) → D(1,2); out[84]` | `1.6861537843785388` (`3ffafa7c63feeb5b`) |
| `gen1385` | `51ad69032231b109b9a636976ce1b0aa317c7a4a5d088231b7204bd4aef6172c` | `2: D(2,0) → D(1,2); out[178]` | `2.723573936592168e-16` (`3cb3a01b6d5b89e5`) |
| `gen1397` | `5e57004c087e0284e4252408eacd4c0e09785032b7f2cde9d64034ee13c5a5de` | `4: D(3,0) → D(1,2); out[53]` | `-6624.1238307257145` (`c0b9e01fb35ed52f`) |
| `gen1403` | `4cdd724c1a07e992b7fc3724b27f4648ec1c61ff7368c05943e990af293edb32` | `0: D(2,0) → D(1,2); out[14]` | `10.963845484493934` (`4025ed7d27c498f2`) |
| `gen1410` | `0acc03a618636cf0999f7eb0ac2fdc321dab2a07b89e2d66771cbbe4a338147d` | `2: D(3,0) → D(1,2); out[69]` | `-112.65878708190243` (`c05c2a29914af316`) |
| `gen1444` | `16438a58bf75fc376839ef8127fe695aee460bf162ec8c3055a27ee18558aadd` | `1: D(3,0) → D(1,2); out[65]` | `-1.6600208929734075` (`bffa8f72115ff5c4`) |
| `gen1468` | `3e7bdb6166f8886ec12659de5aca88e076b3d0347cc311e191908d3efaeca052` | `1: D(3,0) → D(1,2); out[15]` | `-3.6321747802892665` (`c00d0eb1a6b5964b`) |
| `gen1562` | `08be9103f5de9cfe44d1a8814eb462af6c109c8f1e25e0636c1ca3ac323ad5f8` | `1: D(3,0) → D(1,2); out[65]` | `0.170640939665158` (`3fc5d78ff39c3b67`) |
| `gen1584` | `f0555bd1ca9c40e83db2b164de028058aee1fc21b85306a9abbd258822e72b5e` | `4: D(2,0) → D(1,2); out[30]` | `-0.00017810217346443734` (`bf27581e05500a18`) |
| `gen1635` | `fd4d0c455a4adb4b5d65b10ec5508ddbbd11c074120ea70a28c15f44535b2c5b` | `2: D(2,0) → D(1,2); out[87]` | `0.1420179483388816` (`3fc22da4e5c7beec`) |
| `gen1694` | `de56df3e5bac341fe3b9838a032f761ef376193b9d20be9eed03ffc44a8014d4` | `2: D(3,0) → D(1,2); out[72]` | `4.6878380047132` (`4012c0589b1cc1a0`) |
| `gen1726` | `b07ade2fd5772ad144f48bb473bcfa0532b4f8354f627226805ee70eece3b50f` | `4: D(2,0) → D(1,2); out[92]` | `25.13261791980742` (`403921f33f7c6fb9`) |
| `gen1733` | `cf69421a4f14344373f0b1a9eef4d71ed3f9a555dcc692bd2e53d9682f09f16d` | `0: D(3,0) → D(1,2); out[11]` | `-1517.7895841236127` (`c097b72888bd916e`) |
| `gen1817` | `421a285c12439f54205809851c12be6ea1c72c87d0f939e76825aa0be161be55` | `2: D(3,0) → D(1,2); out[28]` | `-3.964099328069551` (`c00fb679b5613c00`) |
| `gen1818` | `87b2472e08b22d1e3cd1f6f04be26dd973f3c90e64d38fb81398db4bb4acd780` | `5: D(3,0) → D(1,2); out[161]` | `3.8359748011239256` (`400eb0138e78dbe2`) |
| `gen1890` | `65940b8bb3ac18059642ac1b0a4e74e38b995755569c1ead500b0df5c30fa04b` | `2: D(2,0) → D(1,2); out[22]` | `-0.020092287442253165` (`bf94931295dd8010`) |
| `gen1955` | `b3753c19abdcea573690a0c0afb9a80fdd426efb53d2f2226517069f5c65b892` | `0: D(2,0) → D(1,2); out[14]` | `-0.0008010517976805854` (`bf4a3fb5a3001601`) |
| `gen2010` | `57485178d00a8aba7d86b94a241293f605b310f124721af6f835746bb7458b6f` | `2: D(3,0) → D(1,2); out[16]` | `0.0829277684224362` (`3fb53ac1154dfbe7`) |
| `gen2015` | `c0341011ca827b61238beee0970d9e29a65df70fad3bcfbf0ab187d3dfc364b2` | `3: D(3,0) → D(1,2); out[89]` | `-7077.814319493817` (`c0bba5d0773e0a70`) |
| `gen2072` | `f875d773bc6b17f8cfbe81b83ed65c45d852976da357b7910918ad6dbe254c78` | `0: D(2,0) → D(1,2); out[14]` | `-2.221733576587689` (`c001c61c40def432`) |
| `gen2140` | `232ef35dbcf0b86eda524a2cdd55fa085fac4753eeb0c2a10a1c180b7a7f3173` | `1: D(3,0) → D(1,2); out[65]` | `88.8629248266755` (`4056373a290d5e93`) |
| `gen2168` | `afb2eec5a8dd7532f6991b3c0cfcf9a6f951632e86421b4e99cdc2df562e15f4` | `3: D(3,0) → D(1,2); out[103]` | `-2.5242259216476555` (`c004319d5c298980`) |
| `gen2170` | `36a3a3696a26fd0de7dab2cc4ad37a2b54103ef55b38561c40928af51505b419` | `1: D(3,0) → D(1,2); out[65]` | `0.05352221334192478` (`3fab674377d38214`) |
| `gen2213` | `24a3e82b7a1765408660b43f8f6a214d4874a1b08aefbff8d9a55fa4ae3e2a74` | `4: D(3,0) → D(1,2); out[87]` | `1.2357925070404463` (`3ff3c5ce5d2616cd`) |
| `gen2240` | `96f22e7292b010a633ba86af052349791b952b6e8dd93f803ad8bd5afdc06f1e` | `1: D(3,0) → D(1,2); out[66]` | `8.273897014998465` (`40208c3c3ac3c7e6`) |
| `gen2257` | `56ed11060d9c321ce1960d526bf0977ff3513ddc8565b69c1cbcdec0fd7fa26d` | `3: D(3,0) → D(1,2); out[70]` | `338.8860886295512` (`40752e2d6b45547a`) |
| `gen2381` | `55808d3df06a378a122913c6cc44e393ca31b2313f307afc21512cb62c8b4558` | `1: D(2,0) → D(1,2); out[68]` | `1.0332500264348623` (`3ff088312e020fd9`) |
| `gen2485` | `78e8538a0f6a94e60e596bda269074b20f1a7f79ac4af1197f378297fabf1eee` | `2: D(2,0) → D(1,2); out[32]` | `-6.103627421827888` (`c0186a1d4e8ee0d8`) |
| `gen2495` | `c6b0babbc92576c1fd6505a82bcd8f0c00415e31bc8dd8fe51edc50bd739e233` | `5: D(3,0) → D(1,2); out[108]` | `1.330571572345724` (`3ff54a056ac3651a`) |
| `gen2543` | `0527ab43d71daf6f485ac50d21049d050219b5ba6c4ffe955906035be9566559` | `1: D(3,0) → D(1,2); out[65]` | `0.0004971143592971405` (`3f404a18f530d900`) |
| `gen2598` | `fbb78af412bd1dd6674cb0d16ba674e130d0b50325bf9a7014d45b9ce826148c` | `2: D(3,0) → D(1,2); out[68]` | `0.007050008578397991` (`3f7ce078447de7fe`) |
| `gen2607` | `c78c8961c979fa6ab66e109dd2085f22c5478406b74a3f61bb850e6cbed2e702` | `3: D(2,0) → D(1,2); out[26]` | `0.012756483904214931` (`3f8a20124971c60c`) |
| `gen2685` | `726ac1e2ac3e47db02bafc970c8154fe7b18a7c4f3573a20f44b65bfba92b7f8` | `2: D(3,0) → D(1,2); out[22]` | `0.07517924159771541` (`3fb33ef2600011ba`) |
| `gen2754` | `d2692fad797e2e9332fdeb67dfe1e49cc6cfcecfff5880e96450d23ccad51200` | `2: D(2,0) → D(1,2); out[33]` | `7.833067750735332e-05` (`3f1488aecaec72e0`) |
| `gen2756` | `10543f07170d25b24c36b0013631177f9e7b38ba6b10e87f293716a594fd1074` | `1: D(3,0) → D(1,2); out[65]` | `-0.003874528919058832` (`bf6fbd79dfd51b22`) |
| `gen2824` | `985ce581b423c40787c3931d30ca4970e92a163bcb1b3eec6df2dca9bc0403d8` | `0: D(3,0) → D(1,2); out[11]` | `-117.89829346856504` (`c05d797da3e36ca0`) |
| `gen2945` | `4e15a87b83c2b0d9932001045f2157ffb217b707a3add7a9293d807caa2fcda5` | `0: D(2,0) → D(1,2); out[14]` | `0.7670261696640784` (`3fe88b7a773c4485`) |
| `gen2949` | `e12bea15388200613bdbfa01f4dc64a76acc7c132ee0cb77fa863452f0068019` | `4: D(2,0) → D(1,2); out[92]` | `0.0450025903849231` (`3fa70a945be086d8`) |
| `gen2966` | `278533c3d93d1a3df54e257f7e69ba48f400b848c3c4c17901f76344334f6f5e` | `0: D(3,0) → D(1,2); out[11]` | `-31.153421623031385` (`c03f2746a3b56b46`) |
| `upper_half_slot_overwrite` | `47852c7ac57e21e5b803752ca3d7aebe95b956cbed17da6678bbdcfdf785f6de` | `1: D(2,0) → D(1,2); out[26]` | `-0.055925937843527895` (`bfaca25314127938`) |

</details>


## Thirteen additional input-only supported wrapper cases

This is a bounded extension of the same independent evaluator, not a kernel change. The harness constructs `CurveBoundedSurface` with empty boundary lists and `StepSurface` with a B-spline base. Source inspection confirms the former forwards its evaluations to the basis (`surface/curve_bounded_surface.rs:313–381`); the latter uses `evaluate_step`, which forwards B-splines with the same parameters (`surface/surface/p1.rs:500–524,1458–1519`). Thus the mathematical surface is the input net in both cases. The independent oracle unwraps only those two forms, then evaluates the input-control-net offset O=S+d(Su×Sv)/|Su×Sv| at 80 and 160 decimal digits. No kernel derivative is an oracle input.

All **54 moved scalar components** meet the unchanged `1e-9` relative rule with precision stability below `1e-50`; the largest measured relative discrepancy is `1.8782e-11`. Each replaced row is exactly the actual profile-matched collector's encoding, also checked against the saved CLI encoding. A whole-script unequal-upper-half audit found no wrong named-slot aliases in these 13 scripts. Nested offsets, mutations, extended/clamped parameters, singular/step-off tangents and unsupported derivative orders remain excluded; `gen1840`'s independent disagreement is left unresolved, not blessed or automatically attributed to #2717.

| Case | Input SHA-256 | Wrapper | Old → new output checksum block (sum, abs sum, weighted sum) |
|---|---|---|---|
| `gen0110` | `7cc4e9ed2024dec85d60580fbb8e24dc50230ae88f79b7bb33e00b931a686aeb` | `step` | `[[-1793.265256031545, 4539.760888149531, -92063.26276291256]]` → `[[-2621.2086646228818, 5367.704296740867, -122100.0187950678]]` |
| `gen0756` | `5ada14a2cb68e27842d66a758c9a4a5bc9d9ed08bdafffc83e72bfac734fa178` | `step` | `[[1.3554805984540093, 7.391439631031779, 39.10197191819011]]` → `[[1.2122093199339468, 7.355706496090119, 36.380232245569125]]` |
| `gen0799` | `95398f876a0aa149584f1e89a0a1d5dd1366b4a347cfd38c36a6416507fba65b` | `bounded` | `[[15685.128669914944, 99233.2195831997, 941088.084522405]]` → `[[15676.719215315688, 99199.2694150199, 940771.9940762286]]` |
| `gen0824` | `39f05d9ab5a4472610d60a753220b5490461bb412fc98f85abd0fac223ee3613` | `bounded` | `[[33464.12457052777, 33497.84604874101, 744137.1302373111]]` → `[[33463.98433500545, 33497.953435537, 744126.2616127371]]` |
| `gen1240` | `535dc18cc183873da4b7b6f9e4cedcb69e27917afe71995e3f9720dbec211011` | `bounded` | `[[2015.222120878384, 2039.5927729894993, 39281.460277584854]]` → `[[2015.955618797746, 2040.007408455776, 39290.117769365075]]` |
| `gen1275` | `a8d836477bfc91cfe9322edee1345e09d18bd60e94014353d46c3cf92a69fce4` | `step` | `[[1.6588839137886584, 29.229129355613253, 284.7963141870564]]` → `[[1.6556057449840669, 29.222579118304704, 284.75313713692185]]` |
| `gen1469` | `6228cef53bdc34bf656f94aad667b2c1106e0976650273f710909f0ad845e6ca` | `bounded` | `[[148487834.10462496, 46013379173.96301, 32688985476.503548]]` → `[[307780230.4308045, 45491081875.461876, 43379397073.4833]]` |
| `gen1748` | `00749bcacd43840715bcae1508d4a755cf504f200cb0e984095270d6b30f4a11` | `step` | `[[13.606676736070348, 13.631349729313825, 75.07088936718932]]` → `[[13.606866826190364, 13.630757323655496, 75.07918855062239]]` |
| `gen1756` | `06ba7f6d39f1f3c27ad7a126317cad14dc8a359e01073f5c59be10a17abca867` | `bounded` | `[[977.2996042465044, 1155.959435686678, 44676.875424165686]]` → `[[963.9718474036881, 1173.7334467816693, 44287.30973723946]]` |
| `gen2231` | `0f56676c5b0bd0b16dbb67439f43f69a5e170ef7843a3b9f375f655ac2e6c912` | `bounded` | `[[1008.0970878598832, 1014.3304606380989, 19140.39888188975]]` → `[[1008.1011108250719, 1014.3260238388825, 19140.46842920942]]` |
| `gen2423` | `03bfb2de669ecf0e8900265f00e674bb769afb85c7c75524f2555cb78677cdd6` | `bounded` | `[[-129863870541.9563, 244523105179.42017, -12864006992933.385]]` → `[[-108399690827.79427, 225474076435.15067, -10885792900484.361]]` |
| `gen2662` | `f99a74ccc2923396bc7777667e7a4ed5477768f0c4ec79c26ce5860eb48d165d` | `step` | `[[1996.2288030841157, 2013.128826422467, 25924.04409976793]]` → `[[1996.2577180518608, 2013.0943424880004, 25924.731084359166]]` |
| `gen2762` | `ae5b6a4e02c506a42a9ad0713d2361990b7b3efaa2383f00098b5c67b8d21a50` | `bounded` | `[[5.498787870148015, 13.119847544973723, 117.5105632099215]]` → `[[5.438370415627446, 14.546961116731453, 117.24257744500959]]` |

<details>
<summary>All 54 component observations (display rounded; decisions use full precision)</summary>

| Case / index | Partial / axis | Observed | Input-only expected (17 digits) | Relative error |
|---|---|---|---|---|
| `gen0110 / 35` | `D(1,2).x` | `-2850.9464164505539` | `-2850.9464164505562` | `7.334e-16` |
| `gen0110 / 36` | `D(1,2).y` | `-714.84554194667953` | `-714.84554194667373` | `8.086e-15` |
| `gen0110 / 37` | `D(1,2).z` | `-382.56768812871474` | `-382.56768812871439` | `8.838e-16` |
| `gen0756 / 17` | `D(1,2).x` | `-0.0017965965393912132` | `-0.0017965965393913672` | `8.57e-14` |
| `gen0756 / 18` | `D(1,2).y` | `-0.050786103949312995` | `-0.050786103949313044` | `1.012e-15` |
| `gen0756 / 19` | `D(1,2).z` | `-0.0011863713004979324` | `-0.0011863713004979725` | `3.378e-14` |
| `gen0799 / 28` | `D(0,3).x` | `-0.015326054691295108` | `-0.015326054691295225` | `7.609e-15` |
| `gen0799 / 29` | `D(0,3).y` | `-0.0041048801719275546` | `-0.0041048801719273838` | `4.172e-14` |
| `gen0799 / 30` | `D(0,3).z` | `0.035464771065865722` | `0.035464771065865694` | `8.178e-16` |
| `gen0799 / 37` | `D(1,2).x` | `0.013659273014666726` | `0.013659273014662875` | `2.819e-13` |
| `gen0799 / 38` | `D(1,2).y` | `-0.16930255100861552` | `-0.16930255100860836` | `4.231e-14` |
| `gen0799 / 39` | `D(1,2).z` | `0.030855762103872225` | `0.030855762103869741` | `8.05e-14` |
| `gen0799 / 43` | `D(2,1).x` | `-7.0880793555857` | `-7.0880793555852684` | `6.088e-14` |
| `gen0799 / 44` | `D(2,1).y` | `3.5227057852188741` | `3.5227057852189336` | `1.685e-14` |
| `gen0799 / 45` | `D(2,1).z` | `7.0189194654588407` | `7.018919465458441` | `5.698e-14` |
| `gen0824 / 75` | `D(1,2).x` | `-0.0061179323062561641` | `-0.0061179323062551961` | `1.583e-13` |
| `gen0824 / 76` | `D(1,2).y` | `-0.070478807001997268` | `-0.07047880700199978` | `3.56e-14` |
| `gen0824 / 77` | `D(1,2).z` | `-0.056446254354832123` | `-0.056446254354830791` | `2.359e-14` |
| `gen1240 / 15` | `D(0,3).x` | `-4.2997202511322685` | `-4.299720251132344` | `1.759e-14` |
| `gen1240 / 16` | `D(0,3).y` | `6.5318781400089181` | `6.5318781400088968` | `3.313e-15` |
| `gen1240 / 17` | `D(0,3).z` | `12.031681444225347` | `12.031681444225173` | `1.447e-14` |
| `gen1275 / 11` | `D(0,3).x` | `-2.7928690761753669e-05` | `-2.7928690761753699e-05` | `1.089e-15` |
| `gen1275 / 12` | `D(0,3).y` | `0.00029301441667883978` | `0.00029301441667884732` | `2.572e-14` |
| `gen1275 / 13` | `D(0,3).z` | `-0.00010453193300659626` | `-0.00010453193300659929` | `2.887e-14` |
| `gen1469 / 66` | `D(0,3).x` | `-5684271.7373430096` | `-5684271.7373429872` | `3.996e-15` |
| `gen1469 / 67` | `D(0,3).y` | `3007006.1287919609` | `3007006.1287919539` | `2.379e-15` |
| `gen1469 / 68` | `D(0,3).z` | `-4395754.2124791071` | `-4395754.2124790819` | `5.732e-15` |
| `gen1748 / 40` | `D(1,2).x` | `7.395305148618172e-06` | `7.3953051487570702e-06` | `1.878e-11` |
| `gen1748 / 41` | `D(1,2).y` | `3.6422524869716576e-05` | `3.6422524869412721e-05` | `8.342e-12` |
| `gen1748 / 42` | `D(1,2).z` | `3.5587225574578896e-05` | `3.5587225574182336e-05` | `1.114e-11` |
| `gen1756 / 27` | `D(1,2).x` | `7.2847785298725256` | `7.2847785298724883` | `5.081e-15` |
| `gen1756 / 28` | `D(1,2).y` | `-63.214943386328045` | `-63.214943386328109` | `1.015e-15` |
| `gen1756 / 29` | `D(1,2).z` | `-3.1050350159108495` | `-3.1050350159107811` | `2.209e-14` |
| `gen2231 / 15` | `D(0,3).x` | `6.1515609799659057e-09` | `6.1515609799672077e-09` | `2.117e-13` |
| `gen2231 / 16` | `D(0,3).y` | `-1.7031922064566014e-09` | `-1.7031922064564419e-09` | `9.357e-14` |
| `gen2231 / 17` | `D(0,3).z` | `4.4956424221657102e-09` | `4.4956424221668369e-09` | `2.506e-13` |
| `gen2423 / 71` | `D(2,1).x` | `-22940207355.336498` | `-22940207355.336506` | `3.623e-16` |
| `gen2423 / 72` | `D(2,1).y` | `2434448490.510294` | `2434448490.5103102` | `6.708e-15` |
| `gen2423 / 73` | `D(2,1).z` | `-2398325735.1938248` | `-2398325735.1938133` | `4.779e-15` |
| `gen2423 / 90` | `D(2,1).x` | `-2387510684.0437241` | `-2387510684.043777` | `2.208e-14` |
| `gen2423 / 91` | `D(2,1).y` | `-21746697320.231216` | `-21746697320.231243` | `1.249e-15` |
| `gen2423 / 92` | `D(2,1).z` | `-64074153297.938492` | `-64074153297.938408` | `1.369e-15` |
| `gen2662 / 18` | `D(1,2).x` | `0.00034038376084129056` | `0.00034038376084129257` | `5.872e-15` |
| `gen2662 / 19` | `D(1,2).y` | `0.00010951663774112274` | `0.00010951663774112518` | `2.224e-14` |
| `gen2662 / 20` | `D(1,2).z` | `-0.00010690599849317438` | `-0.00010690599849317812` | `3.499e-14` |
| `gen2662 / 34` | `D(0,3).x` | `-3.5645030615455682e-07` | `-3.5645030615454867e-07` | `2.29e-14` |
| `gen2662 / 35` | `D(0,3).y` | `-2.7392384406755991e-06` | `-2.7392384406754944e-06` | `3.825e-14` |
| `gen2662 / 36` | `D(0,3).z` | `4.6564817003619468e-06` | `4.6564817003617673e-06` | `3.864e-14` |
| `gen2762 / 24` | `D(0,3).x` | `-0.47719973655667419` | `-0.47719973655667358` | `1.271e-15` |
| `gen2762 / 25` | `D(0,3).y` | `0.21488770295869314` | `0.21488770295869242` | `3.413e-15` |
| `gen2762 / 26` | `D(0,3).z` | `0.3135850468117849` | `0.31358504681178329` | `5.065e-15` |
| `gen2762 / 44` | `D(0,3).x` | `-0.61350814367711859` | `-0.61350814367711581` | `4.578e-15` |
| `gen2762 / 45` | `D(0,3).y` | `0.073007941552904604` | `0.073007941552903535` | `1.46e-14` |
| `gen2762 / 46` | `D(0,3).z` | `0.30682319204641978` | `0.3068231920464225` | `8.889e-15` |

</details>

The four additional scope controls accept bounded/STEP/composed pass-through evaluation and reject a wrong returned component, nested offset and edited script. Together with the previous nine controls, all 13 pass. No result-body construction, topology, validity check, volume or retained-input contract is changed by these scalar golden replacements.

Full precision results, code, controls and the actual gate are retained in the readback-verified 15-member archive, SHA-256 `0f688a32ee293eea988f8db5f1f5dda797bdaad2f26045d84dffdc0ac28c9072`.

## Twenty-six additional scripts with non-aliasing named grids

Unequal request shape alone is not a wrong result: e.g. a `(hu,hv)=(1,3)` upper-half request can keep every serialized named slot correct. This bounded oracle checks the inspected write order for **every successful upper-half evaluation in each whole script**. Every serialized slot must have its requested partial as its final writer; an actual alias is excluded even if its returned value happens to match. It then independently evaluates **every moved component**, not just the unequal request, from the original control net at 80 and 160 decimal digits. Every one of the 162 components is within `1e-9` relative and has precision stability below `1e-50`.

All scripts stay read-only. The same direct/pass-through B-spline scope and exclusions apply; edits, clamping, extensions, unsupported orders, interior-knot sides and singular/step-off evaluations are not blessed. The new negative controls reject a true named-slot alias, a wrong value despite correct writers, and a changed output index not covered by decoded evaluations. #2717's implementation and refusal-versus-mask contract remain untouched. Three independent disagreements (`gen0865`, `gen2145`, `gen2860`) remain unresolved, not blessed or automatically attributed to that issue.

Each replacement below is the complete actual collector encoding; integers, statuses, shape and all unrelated rows are preserved. Full precision component observations and indices are in the verified 16-member archive, SHA-256 `19f4705e170673119cbf58b2437b6e65c3273b067f500b548f81b76868bad671`.

| Case | Input SHA-256 | Independently checked moved components / largest relative error | Old → new checksum blocks |
|---|---|---|---|
| `gen0080` | `9e436658a1572dca1558bf2442e6797b403917c305406b3b57c13ab647915df0` | `6 / 3.6774e-15` | `[[30379.00257735898, 30447.956764713053, 1686880.8415455215]]` → `[[30381.206162080358, 30448.991614664017, 1686945.450146894]]` |
| `gen0236` | `f1f18708aae2c87ea13310b1234e711141b070b2b0271b2118503dafb3e6b302` | `6 / 1.1183e-12` | `[[30421.234672403345, 30456.973904279326, 2662674.4826113833]]` → `[[30421.06883505922, 30456.808066935217, 2662669.7392165186]]` |
| `gen0252` | `e2623e88b9a9effb2d532f01613790991180c175f7c431c6e07b0d25cac87f71` | `3 / 3.3726e-13` | `[[30406.384453877406, 30478.85928586466, 1566099.3574601205]]` → `[[30406.354976187125, 30478.87649281059, 1566098.8504459893]]` |
| `gen0379` | `6e75fe9c11aef56989e7f6eb40a94d541b14497c43e7f4ecc958b985df98e378` | `3 / 5.4169e-14` | `[[31699.330865783555, 32275.434630051248, 608329.1022738847]]` → `[[31700.498837974286, 32276.578274146483, 608426.1846821741]]` |
| `gen0420` | `00690c96b2204acc746577753bb9780882ed1c9b7b32be9b3cb45d4fe42d4da5` | `9 / 3.2509e-15` | `[[31250.937855246448, 31793.443414857327, 2461983.8602397055]]` → `[[31335.61860395548, 31684.50755778467, 2465314.897156685]]` |
| `gen0466` | `979b5a76b44c21cd69755575d9307ac6936dcc41637436f796aa65c6b81279e3` | `4 / 4.7499e-13` | `[[60868.818536099345, 60918.42222606814, 3836571.0485280785]]` → `[[60868.894608623326, 60918.2921872957, 3836577.3904423118]]` |
| `gen0700` | `36e56a9d092e5a925db2069181a156218103aa42ee3f1d62d9457f8635b2288e` | `9 / 8.1622e-15` | `[[30392.901880758895, 30459.305723063335, 502349.9584627636]]` → `[[30391.922884114574, 30461.59455671349, 502273.1675116542]]` |
| `gen0811` | `412b0d592f4de2a157bf6f182e536f9d826d0306f9f2f3f98c548c8931f65386` | `9 / 1.1564e-13` | `[[31538.727703292338, 31580.115359256026, 1860370.1995832226]]` → `[[31539.44686175704, 31579.76019984597, 1860392.0706096264]]` |
| `gen0835` | `9b762b97777013b43903b7ea92ac48093bf0b6e86ddb37596c5bfd61b94ad7d9` | `6 / 1.414e-13` | `[[30414.330112128788, 30497.647075211837, 1658793.2847836807]]` → `[[30413.157940337896, 30497.146754871326, 1658759.1715066873]]` |
| `gen1052` | `1bdaf6aec3c27961414ea68fcd4672443acc0df63793cd3d5f9e27cfbf3a5be2` | `6 / 6.3184e-13` | `[[61918.03497148949, 62025.00353079037, 3591838.874331083]]` → `[[61918.99313783185, 62022.14715561212, 3591912.498154636]]` |
| `gen1129` | `7e9ca32cdd9a71b3850c5745130602b5cb7ba00d972873feb9a84c5af6bf33e7` | `4 / 5.3587e-12` | `[[1186497979.152731, 1218626548.3444576, 32735246131.13009]]` → `[[1186537755.2086275, 1218558795.090664, 32736071395.094486]]` |
| `gen1535` | `ee6a964a87629b2eaa449180cc195524c26f2c48bebf4b2560446e4e791e5627` | `8 / 1.1332e-14` | `[[49497.583260137486, 155368.6558296504, 4484276.854383658]]` → `[[76825.05070814188, 177478.76016904792, 5748603.666975015]]` |
| `gen1676` | `2d27da7af77ef77f6f607838e843e059d8e38e73f9c9eb15e4694aab33bcdb0c` | `8 / 5.5347e-15` | `[[-92.3124121670792, 183.10863961763488, -2742.3680877607035]]` → `[[-90.53952096325878, 182.6202420786579, -2658.9683361616776]]` |
| `gen1691` | `f3cb837213375c6cb1a548cf8e9e9eac553dfef1d02f9fccecbbd386a2555986` | `6 / 1.0436e-13` | `[[62964.96195446904, 63030.27212845825, 3709590.7899841545]]` → `[[62964.88025255849, 63030.35212720648, 3709584.498368973]]` |
| `gen1743` | `258ff3eba9d71e6d896c6422f449821f9a679e8467b7f834c09fc8a87b3fa6a1` | `12 / 2.738e-14` | `[[27712.10813313832, 38745.97861079435, 215263.02667339597]]` → `[[27806.873231672107, 38436.394755773166, 225937.0909352196]]` |
| `gen1804` | `0fd77d6d729189c4d974960517ab127228f18f10faf9e7b49d733259b8c2b7dc` | `5 / 1.8609e-12` | `[[30473.35618335161, 30513.728819801316, 1965374.501426186]]` → `[[30473.356490371585, 30513.728229923236, 1965374.5090134607]]` |
| `gen1905` | `c923bd8e86e3a3c1d0dfeaf8d3f6de6750aa0d8d43bd185aa0b4183464e6e39e` | `6 / 1.9336e-13` | `[[31837.222528412156, 31903.469290274574, 1432598.639920631]]` → `[[31837.242682660522, 31903.45270763585, 1432599.028721083]]` |
| `gen2058` | `cc3902590fd5eec4bdf13186fb366df954f63716e9e113a1e54530aad96a904a` | `3 / 1.4144e-15` | `[[60877.968770035375, 60915.38270499651, 4385454.476949993]]` → `[[60876.88228251488, 60922.86583329465, 4385379.8824029]]` |
| `gen2101` | `320e05376912fbd7ba1870366ae9e483028e83c78c223c2fad49883411c49005` | `6 / 1.4334e-15` | `[[6.120303143823539, 29.807555644234586, 44.089840393630936]]` → `[[12.736870292892549, 36.59454372347717, 171.08369003361463]]` |
| `gen2225` | `369ac89a0e0b8fb3c3a89fc78e530552b608901f0e411758c8f4e578800ccc74` | `7 / 2.5883e-12` | `[[-286910412392.90857, 4398900748443.411, -3427730726124.274]]` → `[[304464144073.1033, 5873260061992.563, 9249330930772.734]]` |
| `gen2242` | `bd573307972059bf9606fc3420c9fb336cd3818d3f7d2fdf8fe5258e53bcd21e` | `6 / 2.0845e-13` | `[[61857.73232266659, 61947.336223590755, 4056530.591775608]]` → `[[61852.770849920766, 61939.45936701227, 4056097.0808932446]]` |
| `gen2402` | `bcd174c210ea7ec7ba6cc338a9bef1448bcad8a7503c0b65245d67c0f5684a34` | `6 / 1.1291e-14` | `[[32193.652313328614, 33075.20055926031, 1440327.8072871233]]` → `[[32127.0172981427, 32674.650403130265, 1438086.9123989216]]` |
| `gen2586` | `27c106e0d41d051e77ee878a52bd357e6faaa1874646a3069c36b977b361d920` | `8 / 1.3559e-13` | `[[30402.4513996562, 30492.49403155344, 2691381.7170912246]]` → `[[30403.280350555364, 30490.853927247652, 2691397.098786934]]` |
| `gen2721` | `f2ac0052ab83ae2f3be342e7ffc4415c8fe1ff28d52d60769e766b4550e77fa7` | `6 / 5.9118e-14` | `[[119636313604.18224, 573461094890.6915, 7083305718612.147]]` → `[[-6770903206.227723, 461629696222.1089, -2181812207294.0457]]` |
| `gen2888` | `90c613d2379d34a3367cb6fa9e6a8571cd71e94f84448b4e09f0f103c08785a5` | `4 / 1.0343e-11` | `[[-10102.721720538217, 24393.523038543684, -142894.98664173268]]` → `[[-10142.640060926964, 24529.309695200158, -143905.46653528017]]` |
| `gen2913` | `e96427c91d40ce3079de04c33ac6275cdb615aaceebd5d59aa2de13cbd732553` | `6 / 3.732e-14` | `[[30417.766309459083, 30477.93182223716, 502681.3094662713]]` → `[[30417.680348608184, 30477.79120381156, 502675.1315456713]]` |

## Four additional nested-zero identities

In each of these unmutated scripts exactly one nested distance is zero. The zero offset is the identity, so the mathematical derivative is that of the remaining single offset of the original input net. This argument makes no assumption about the orientation or fold behaviour of **two nonzero** offsets: those remain outside this oracle. The original domain, regular-tangent, total-order and whole-script named-writer restrictions still apply. A clamped request and a mutated script remain excluded, not blessed.

All **17 moved components** independently agree at 80/160 decimal digits, with precision stability below `1e-50` and maximum relative discrepancy `1.0118e-12`, below the unchanged `1e-9`. The new controls accept both zero identities, reject two nonzero offsets, and detect a wrong returned value. Together all 20 controls pass. Only these four encoded rows change, each identical to the preserved profile-matched collector; all other expectations and failure entries stay unchanged.

| Case / input SHA-256 | Identity | Old → new checksum blocks |
|---|---|---|
| `gen0302` / `30a04c108e5d8e4b4765f2153918410ab70bd8ed1ae36ceb4c4ea091960fae0c` | outer zero | `[[30827.64211473529, 30862.21966957595, 909319.6839455989]]` → `[[30827.640352428367, 30862.236462334913, 909319.6443810975]]` |
| `gen0957` / `0903c49446b3f0fdeca1630789ef4c93778b05849ac678f8f5455f99f0298d7c` | inner zero | `[[56990.064452212995, 60611.50358534768, 2558024.7556519024]]` → `[[56969.283059406494, 60647.37636115499, 2557313.2601297847]]` |
| `gen2311` / `335a922cf7b661a8974ebededb45a49eaf26ae092355c6397017cb09989f223e` | outer zero | `[[61557.13565702396, 61724.96166855296, 3537970.152684235]]` → `[[61547.02359600594, 61741.13247766892, 3537325.0349517446]]` |
| `gen2766` / `adeba1dbd034c1039a4994c4ab29fa57ec2a04f662aa7c04c5cbb6a090f18415` | outer zero | `[[958900887471.6716, 1414026493265.7517, 71062889191835.97]]` → `[[942303322374.7341, 1397428928168.8142, 69858556187898.766]]` |

All 17 full-precision component observations remain in the nested-zero evidence archive named below.

Code, raw data, full-precision observations, controls and the actual gate are in the readback-verified 18-member archive, SHA-256 `e9444ddcad2126de4888713d5878e1323aab1237b9ab41f8b90e33ef93cb025e`. No geometry-body construction, topology, validity, volume or retained-input contract is changed by these scalar expectation updates.

## Eight additional locally regular nested offsets

This is an independent **local derivative proof**, not permission to flatten arbitrary offsets. Put `T=S+d*n`, where `n` is S's unit normal. Differentiating `n·n=1` shows `Tu` and `Tv` lie in S's tangent plane; hence `Tu×Tv=J(Su×Sv)`. Where `J>0` and both surfaces are regular, continuity preserves that sign in a neighborhood, so T's unit normal is n and its further offset is `S+(d+e)n`, including the derivatives at that point. This is the normal/orientation relation in [Patrikalakis–Maekawa–Cho §11.3.1, equations 11.17–11.23](https://web.mit.edu/hyperbook/Patrikalakis-Maekawa-Cho/node224.html).

For **every moved evaluation** below, jets computed only from the original net independently establish the positive J and normal collinearity at 80/160 digits. J ranges from 0.7951 to 2.5695; the relative squared collinearity residual is below `1e-100`. All **36 moved scalar components** satisfy the unchanged `1e-9` relative comparison and `1e-50` precision-stability bound (largest discrepancy `2.2847e-11`). The distance sum is formed independently in decimal from the original scalar inputs, not from kernel state. Whole-script named-slot checks still exclude actual aliases. Mutation, clamping, extensions, interior-knot sides and step-off thresholds remain excluded.

The added paraboloid controls independently check the point, first tangents and Jacobian factor in closed form; a negative-orientation offset, a focal offset, and an incorrect returned value are rejected. All 24 controls pass. `gen1323` and `gen2190` fail the positive-normal test and remain **unresolved**, not blessed with an unjustified distance sum. This proves no global fold absence, surface topology or result-body validity; those claims are not made.

| Case / input SHA-256 | Components / max relative error | Positive local normal factors | Old → new checksum blocks |
|---|---|---|---|
| `gen0231` / `ce024ef3518668831c4a262f61cd450895b0724b4b02129bbf23fe55c112fa4e` | `12 / 1.9827e-13` | `[0.8988037434837284, 0.8964042836013618]` | `[[45580896831.72423, 95389791944.91127, 4002174278995.3906]]` → `[[45581293347.95954, 95383078871.46913, 4002191020838.163]]` |
| `gen0693` / `09f41f3cf32a195ae9340c98529931e9695223d011e272821c2ff36b6ab0ea12` | `3 / 4.874e-15` | `[1.2836520633183264]` | `[[30768.87584330439, 30937.918892050533, 505211.64055525354]]` → `[[30768.25063146633, 30938.188245733425, 505166.13185559894]]` |
| `gen1737` / `6b5e541dea9890626cb0d8abaf0c5012ffd1bf023dd32f22f958c6d184d390a6` | `6 / 3.6763e-13` | `[2.5694737131193226]` | `[[29848.100318849418, 31834.48077184831, 1312427.284835471]]` → `[[29847.345690061786, 31831.810669040908, 1312406.4322319615]]` |
| `gen1831` / `7bfd0b144c304adde1c533f669c21d44527fc91109ca5d83b04889a3a91ad0b9` | `3 / 4.237e-12` | `[0.9889378944889109]` | `[[-12625472801.0987, 30180114690.678963, -660746037972.9828]]` → `[[-12621900570.213278, 30175545644.943497, -660510738258.7076]]` |
| `gen1961` / `95e25ea1d730f8a2056799fbfd082092cc1301cd053e6e343388019a6be57386` | `3 / 1.1342e-14` | `[0.955167466420743]` | `[[31828.353267824037, 31906.008061543303, 630009.7380669664]]` → `[[31827.99141782863, 31905.195112392754, 629983.9711326736]]` |
| `gen2461` / `a230545cc2d3c29214688e00f4ae3836c571d2b7e16fef59e113c3dc2ee409ca` | `3 / 2.2846e-11` | `[1.0200279685187483]` | `[[-0.007559155208330911, 0.013805289471912924, -0.08892357320304935]]` → `[[-0.007559155134554697, 0.013805289432922412, -0.08892357109854443]]` |
| `gen2680` / `a34ce59d5704014135d17d97489e8d83bb665b990e1fcfd81c6035db91c28bda` | `3 / 3.0828e-14` | `[0.7951450461677018]` | `[[-4.172894886415667, 27.14634803960104, -59.00430477408537]]` → `[[-3.411351874036506, 51.416200568900344, -47.69090686109031]]` |
| `gen2938` / `6d66abfa1a66636f0e086d0e9fc316260ef722e71089e94fd7823f622ee9ce18` | `3 / 1.9294e-13` | `[0.9712510356712598]` | `[[61656.09857959446, 61701.59111604824, 3392138.137636158]]` → `[[61659.71526991051, 61696.25679886635, 3392412.5706676855]]` |

Full-precision observations, original inputs, executable proof and all controls/gates are retained in the readback-verified 20-member archive, SHA-256 `ffa9677bcc4a9c2d96047e740e6c1d515f1df2615ba7fc0482146b176442fbfb`. Only these eight complete encoded rows change; all case identities, unrelated expectations and known failures remain unchanged.

## Twenty receiver-copy/scalar-setter scripts, evidence preserved

The preceding twenty-row old/new table, receiver-state proof and all 89 component bounds are preserved unchanged in [this committed finding](https://github.com/altvnk/solidcore/blob/2ef53c83e1e4f0f37146f27e79f4a2cb5ff3b445/plan/findings/surface.offset_surface--copy-setter-input-oracle-evidence-2026-11-08.md), including the original thirty-member archive hash. Those rows are not reblessed here.

## Clamped-point evidence retained verbatim

The preceding eighteen-row old/new table, complete published clamp proof, exclusions and archive hash are preserved verbatim in [this committed finding](https://github.com/altvnk/solidcore/blob/b982cc76604d6c910dfe313064aa4417250f0674/plan/findings/surface.offset_surface--clamped-input-oracle-evidence-2026-11-08.md), explicitly attributed to the historical `2ef53c83e` publication. Those eighteen rows are not reblessed here.

## Canonical-revolution evidence, preserved

The thirteen-case / 62-component input-only proof, all old/new blocks, construction/precision controls and the independently wrong `gen1603` named slot remain in [the committed finding](https://github.com/altvnk/solidcore/blob/beb1eb1843a2e988eb42ab169467494395fc6047/plan/findings/surface.offset_surface--canonical-revolution-input-oracle-evidence-2026-11-08.md). Its verified 35-member archive is `9266f4152921f181300f3811f269622338fec6b672311413a86764fece3d7a0c`. No prior row is reblessed here; no suffix-correctness or #2717 implementation claim follows.

## Five returned-base/inversion-query scripts, evidence preserved

The five-case / 26-component proof, all old/new blocks, original input hashes, clamped points, 52 controls, `gen2558` disagreement and executable archive are preserved in [the committed finding](https://github.com/altvnk/solidcore/blob/8e07893cff99db33d90e3ed10d6551a87587f032/plan/findings/surface.offset_surface--returned-base-input-oracle-evidence-2026-11-08.md). Those rows are not reblessed by this update.

## Parameter-edit evidence, preserved

The five-case / 24-component proof, every old/new block, original input/bit identities, 59 controls and pre-edit versus post-edit distinctions remain in [the committed finding](https://github.com/altvnk/solidcore/blob/beb1eb1843a2e988eb42ab169467494395fc6047/plan/findings/surface.offset_surface--parameter-edit-input-oracle-evidence-2026-11-08.md). Only `gen0909`/`gen2960` claimed changed post-swap/post-reverse jets. `gen0304`/`gen0672` still disagree. No previous row is reblessed; the verified archive remains `9887e283e19139ec82596e30a378753dc2c6b55fb6930dce9157d2243b5bab2d`.

## Seven further independently supported rows

`gen0046`, `gen0294`, `gen0715`, `gen1107`, `gen1871`, `gen1956`, `gen2606`: **29 moved components**, input-only 80/160-digit jets, maximum relative error `7.9414e-14` under unchanged `1e-9`; **66 controls pass**. `gen0294`/`gen1956` use complete scripts; the other five prove every moved bit before an entirely bit-identical suffix. No suffix-correctness claim; all complete production cases still run.

The [complete old/new encodings, all observations, original inputs, boundaries, suffix digests and exclusions](https://github.com/altvnk/solidcore/blob/beb1eb1843a2e988eb42ab169467494395fc6047/plan/findings/surface.offset_surface--unchanged-suffix-input-oracle-evidence-2026-11-08.md) are committed. Verified 44-member archive: `ffbef76485eb2d56b266d1e1dad0a8c785d27f05dfe03023ed629b1439026afe`. Numerical disagreements remain unapproved; no decoder relaxation, production crop or skip.

## One explicit-side-clamp script with extension enabled

`gen1332`: all **three moved D(0,3) components** agree with input-only 80/160-digit jets at the explicitly clamped VMax endpoint, maximum relative error `8.06353e-14` under unchanged `1e-9`; **85 controls pass**. The original enabled flag and side mask **11** stay unchanged; VMax bit 8 applies before extension. Independent regular first samples separate the periodicity positions from the source threshold by more than `6.632e12`. The full three-step/72-scalar script is decoded; no crop or actual-extension claim.

[Full old/new encoding, original input hash, all observed/independent values, state proof and exclusions](https://github.com/altvnk/solidcore/blob/ceadc86fedb8d337812b4882175f4fbb0fd0b79c/plan/findings/surface.offset_surface--explicit-side-clamp-input-oracle-evidence-2026-11-08.md). Verified 64-member archive: `c0277e8e83feb5100d1c40e5f947f3aa476e0073555c19acd9fcdf403086ec81`. Numeric-only `gen0590` remains excluded by its first-sample normal fallback; `gen0962` remains excluded because swap does not remap its original side mask. Neither is blessed.

## Two enabled-but-unused extension prefixes

`gen2007` and `gen2248`: **12 moved components**, input-only 80/160-digit jets, maximum relative error `8.80533e-14` under unchanged `1e-9`; **78 controls pass**. Every prefix point stays in the original direct B-spline domain, so the enabled flag never reaches extension or clamping. All moved bits precede unchanged suffixes of **4 / 234 scalars**. No actual-extension, post-edit or suffix-correctness claim; full original cases still run. `gen2690` disagrees and stays unapproved.

[Full old/new encodings, all observations, original input identities, state proof and boundaries](https://github.com/altvnk/solidcore/blob/670557efa12f074f96d7fc559a1d854f7f722aca/plan/findings/surface.offset_surface--unused-extension-input-oracle-evidence-2026-11-08.md). Verified 56-member archive: `b2b23e145751baaa363d55a718475028b67f24f513de9dce23d9884d297e1064`.

## Two further wrong named-slot results, not blessed

`gen0757` step 1, `out[63..65]`, and `gen2243` step 2, `out[60..62]`: `D(1,1)` overwrites named `D(2,0)`. Input-only 80/160-digit jets prove six wrong components, relative discrepancies **0.9128–1.6373**; **71 controls pass**. The first proof uses the explicit clamped endpoint after a returned copy; the second precedes swap.

Both wrong goldens are removed; full cases keep executing under case-specific #2717 failures. [Old encodings, observed/independent values and bits, inputs and limits](https://github.com/altvnk/solidcore/blob/beb1eb1843a2e988eb42ab169467494395fc6047/plan/findings/surface.offset_surface--two-query-prefix-named-slot-failures-2026-11-08.md) are committed. Verified 49-member archive: `9169e48e605bddc010862db3b610c420e54372dba1ce9ad46ce73c5b35fccfb7`. No suffix/writer-correctness or refusal-versus-mask claim.

## Six earlier named-slot proofs, retained verbatim

The full six-case / 21-component published section, every input hash and observation, original collector attribution, controls and archive hash are now [preserved verbatim in a committed finding](https://github.com/altvnk/solidcore/blob/670557efa12f074f96d7fc559a1d854f7f722aca/plan/findings/surface.offset_surface--six-named-slot-evidence-published-2026-11-08.md). Exact text equality was verified before replacing the inline duplicate. No prior result is reclassified or reblessed.

## Verification and remaining limits

At exact head `ceadc86fedb8d337812b4882175f4fbb0fd0b79c`, workspace fmt, workspace/all-target clippy (`-D warnings`), wasm32, panic inventory and all **23 offset-module tests pass**. Rust 1.98.0, `CARGO_BUILD_JOBS=2`, commands under `nice -n 10`; offset tests use `CARGO_INCREMENTAL=0`. The actual full-case unit gate remains **FAIL**:

```text
FAIL surface.offset_surface cases=3182 changed=188 added=0 removed=0 broken=0 known_failing=88 rust=29.309s
```

All **3,182 cases / 3,094 golden rows / 88 known failures** remain selected. The latest commit changes only the complete `gen1332` golden plus its evidence record; every other row and known failure is unchanged. **167 supported cases / 815 moved components** have independent evidence; **188 ordinary differences** remain. Numerical source SHA-256 stays `86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`. The collector remains attributed to measured `7a90407a0`, not a new execution. The full `goldens` package tests passed **732 / zero failed / 31 ignored** at `beb1eb1843a2e988eb42ab169467494395fc6047`, not relabelled here. No full kernel-suite or all-unit-golden success is claimed. Exact-head canonical [run 42478935902](https://github.com/tangency-labs/solidcore/actions/runs/42478935902) at `ceadc86fedb8d337812b4882175f4fbb0fd0b79c` completed **`action_required` with zero jobs**, not success. No approval request, workflow rerun or review bypass was made.

Historical test/collector executions retain their measured revisions and archived evidence. The upstream-commented integration test passed at `ece73364c`, not a new execution here. No downstream or canonical-CI success is claimed; the historical attribution-review thread remains unresolved.

In scope: the four terms, their retained regression and evidence-backed golden dispositions.
Expected edits: the existing numerical file and records, this unit's golden and its known-failure entries.
Related work / available follow-ups: #2717's implementation and refusal-versus-mask design remain separate. No new PR or unrelated work lane is claimed.
Status / next step: keep draft; explain or fix the remaining 188 ordinary changes before readiness. No maintainer ping, workflow approval request, rerun or bypass is made.


Original executable oracle and control sources remain retained in the verified evidence archives; their inline duplicate is omitted here to keep the complete per-case migration evidence within GitHub's body limit.
