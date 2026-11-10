# Offset derivatives: receiver copies and scalar setters (2026-11-08)

This preserves the twenty-case evidence published with #1914 at
`eafd9f983a896d404708f560250e61d98af1148d`, without changing its conclusions.
The table moves here only to keep the growing PR description within GitHub's
size limit. This is evidence, not a new implementation or a blanket blessing.

## Contract and independent calculation

`OffsetSurface::create_offset_surface` and `split_at` in
`crates/solidcore/src/surface/offset_surface.rs` take `&self` and return separate
surfaces. The golden runner in
`crates/goldens/src/units/surface_offset_surface.rs` serializes their results but
does not assign them to its evaluated receiver. `set_offset_distance` changes
only the scalar distance; `set_allow_extension` changes only that flag. The
independent script decodes the complete output, distinguishing returned copies
from those explicit scalar-state changes.

All 89 moved receiver-derivative components are computed from the original input
B-spline net and its active distance, not from kernel derivatives or the old
golden. The independent Taylor-series calculation forms
`O = S + d (Su x Sv) / |Su x Sv|` through order three at 80 and 160 decimal digits,
with exact conversion of input binary64 numbers. Every moved component satisfies
the unchanged `1e-9` relative rule; the maximum discrepancy is
`2.2550171238068774e-12`, and maximum relative precision drift is
`2.467611411137571e-76`. No absolute floor is used.

All 32 numerical/scope controls pass. They distinguish assignment from a returned
offset and two returned split copies; detect a wrong value, a changed returned
metadata field or leftover output; reject a real `(3,1)` named-slot collision;
and accept a fully decoded non-aliasing `(2,3)` request. An initial negative
control used a non-aliasing grid and failed closed on leftover data; correcting
its input did not relax the oracle. Actual aliases remain #2717's separate
implementation and refusal-versus-mask contract.

Other edits, clamping/extension, singular evaluations and unsupported orders are
not accepted by this particular oracle. Changed returned-copy metadata and
point-grid fields are not graded as derivatives and fail closed. No global
fold, B-rep validity or volume claim is made for these scalar observations.

## Exact twenty-row replacements

The numbers below are the golden encoding's `[sum, abs_sum, weighted_sum]`, not
an error bound. The error column grades every raw moved derivative component.
Each complete replacement equals the profile-matched collector actually run at
`7a90407a0fe189ebceb551e5df28aaf4e8cc9893`; raw stream SHA-256 is
`735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
All case inputs, other rows, topology/status fields and 85 known failures remain
unchanged. These twenty replacements were committed before this record.

| Case | Components / maximum relative error | Old → new checksum blocks |
|---|---|---|
| `gen0032` | 3 / `4.4236e-16` | `[[92670.95430254005, 92715.26944807055, 5394495.4598471755]]` → `[[92693.03707327033, 92694.87458551113, 5394777.774092088]]` |
| `gen0162` | 3 / `1.0244e-15` | `[[46871.64465480175, 46912.45744508984, 641284.698235866]]` → `[[46871.953361038046, 46912.76615132614, 641305.966600447]]` |
| `gen0475` | 3 / `1.5724e-14` | `[[27151.82918504171, 131932.52549897382, -939979.6007762165]]` → `[[31185.4680385726, 127549.24613271712, -558222.3126147436]]` |
| `gen0572` | 6 / `1.7067e-15` | `[[167849.9025395879, 167890.1078119872, 21300050.663649388]]` → `[[167850.06785832535, 167890.11503477694, 21300062.927538052]]` |
| `gen0880` | 3 / `5.5494e-14` | `[[60920.154703662876, 61176.964056868645, 3906415.661581074]]` → `[[60920.704177028034, 61177.49230659628, 3906458.4145365255]]` |
| `gen0994` | 3 / `1.1467e-14` | `[[106650.05878460688, 106650.18920233288, 8439346.19902911]]` → `[[106650.05725603321, 106650.19177550825, 8439345.982613547]]` |
| `gen1224` | 3 / `1.8937e-13` | `[[30411.369709868013, 30459.526237990547, 1232554.694631206]]` → `[[30411.448242488477, 30459.51607033539, 1232556.181057504]]` |
| `gen1315` | 8 / `4.5609e-13` | `[[30383.545788392545, 30601.565436381683, 500105.67712378607]]` → `[[30810.583620881953, 31221.276639732067, 528537.8746195738]]` |
| `gen1409` | 3 / `6.742e-15` | `[[152531.7026919565, 152595.2574882943, 15973296.61425218]]` → `[[152487.8116847785, 152551.36648111625, 15964854.977971999]]` |
| `gen1736` | 5 / `5.803e-13` | `[[30381.85489376341, 30463.342937108937, 501217.8127281958]]` → `[[30381.84341680052, 30463.32683485667, 501216.735673139]]` |
| `gen1832` | 8 / `3.0573e-14` | `[[76195.08638962446, 76280.17415270921, 4698801.86238598]]` → `[[76194.26386539408, 76280.91280592125, 4698729.4083463205]]` |
| `gen1868` | 3 / `3.4819e-14` | `[[106585.78353762635, 106753.77622991134, 10570997.703791479]]` → `[[106603.2684371318, 106777.40382001479, 10572110.759543618]]` |
| `gen2179` | 3 / `4.8025e-13` | `[[106611.43744989557, 106748.63148533777, 10203459.959847251]]` → `[[106611.42625550646, 106748.64315685986, 10203459.145313958]]` |
| `gen2517` | 3 / `2.7329e-14` | `[[45834.828639702326, 45858.81352402091, 1450934.8174728234]]` → `[[45834.921610059566, 45858.92964139639, 1450936.507029497]]` |
| `gen2687` | 8 / `1.2649e-13` | `[[91548.27224064688, 91893.13238284142, 6649038.304278976]]` → `[[91548.21958135064, 91893.04473901891, 6649037.327577968]]` |
| `gen2688` | 3 / `2.255e-12` | `[[152371.15474998739, 152542.02290568707, 16971914.402893644]]` → `[[152371.10683083418, 152541.96646537865, 16971911.22996498]]` |
| `gen2732` | 6 / `2.4426e-15` | `[[107607.11219702837, 107831.01576927245, 8908709.776030885]]` → `[[107623.6670791933, 107824.74647024467, 8910818.62771597]]` |
| `gen2740` | 3 / `1.8201e-13` | `[[61009.98164865395, 61027.88924260481, 4365938.865820019]]` → `[[61009.990955444446, 61027.92015518564, 4365939.5726180915]]` |
| `gen2785` | 9 / `2.399e-14` | `[[91152.17943308967, 92306.83440740129, 7652415.540723781]]` → `[[91076.78844778116, 92375.2087779946, 7650622.669702106]]` |
| `gen2836` | 3 / `4.0678e-14` | `[[91224.74312733122, 91314.04492096876, 5636908.971653164]]` → `[[91224.72905059176, 91314.07339008275, 5636907.712885878]]` |

## Retained executable evidence and limitations

The original thirty-member executable/raw-data/control/check archive was read
back and hash-verified, SHA-256
`96f1bf236a6bf5fe9096644c76151be41b9c31581f36199d04a2da64a5218864`.
The numerical source SHA-256 remains
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
At the named publication head, fmt, workspace/all-target clippy, wasm32 and panic
inventory pass, while the gate is still
`FAIL surface.offset_surface cases=3182 changed=242 added=0 removed=0 broken=0 known_failing=85`.
Canonical run 42462015631 ended `action_required`, not success. None of this
certifies the unresolved cases or supersedes the draft's correctness/review hold.
