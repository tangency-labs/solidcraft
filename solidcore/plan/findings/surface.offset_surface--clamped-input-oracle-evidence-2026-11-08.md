# Offset derivatives: preserved clamped-point evidence (2026-11-08)

The section below is preserved verbatim from #1914's published description at
`2ef53c83e1e4f0f37146f27e79f4a2cb5ff3b445`, to keep later descriptions below
GitHub's size limit. "This exact head", "only these eighteen rows" and the
85-failure count refer to that historical publication, not to a later commit.
Nothing is reblessed or remeasured by moving this evidence into a record.

## Eighteen additional clamped-point jets

With extension disabled throughout, `select_surface` clamps to the original direct-B-spline domain **before** `plain_or_simple` differentiates it. The returned jet is at that point, not a constant extension's derivatives. The input-only oracle computes those coordinates from the knots and evaluates at 80/160 digits, tracking scalar setters without changing inputs. Wrappers, extension-enabled scripts, other edits, unsupported orders, singular substitutions and actual named-slot aliases remain excluded.

All **102 moved components** satisfy the unchanged `1e-9` relative rule (maximum `4.3632e-13`) with maximum precision drift `3.453e-77`. All **37 numerical/scope controls** pass, and the existing kernel clamp/derivative-limit regression passes at this exact head. `gen1146`, `gen2298` and `gen2832` independently disagree and remain **unresolved**, with no automatic #2717 attribution, added floor or tolerance relaxation.

| Case | Components / maximum relative error | Old → new checksum blocks |
|---|---|---|
| `gen0096` | 3 / `5.2088e-14` | `[[-6773770209.875433, 64424015402.72274, -378749722047.2987]]` → `[[-6870563819.263274, 64495408259.89502, -386553674290.37585]]` |
| `gen0106` | 9 / `7.5415e-14` | `[[31626.719296583855, 33363.51465493481, 2099118.839468814]]` → `[[31510.658539520075, 33182.16476097925, 2093273.1365587397]]` |
| `gen0117` | 3 / `3.7514e-14` | `[[30393.92240119993, 30444.3317490507, 501900.592222631]]` → `[[30393.952077675916, 30444.158329310143, 501902.39235053613]]` |
| `gen0134` | 6 / `4.3632e-13` | `[[37.832510687021795, 104.14284964357064, 1054.742957704554]]` → `[[39.06487390981253, 102.07201454768582, 1103.5111544935114]]` |
| `gen0439` | 9 / `1.588e-14` | `[[-13493691.441181729, 13717537.718196781, -1869604804.9283178]]` → `[[-5556688.228706748, 5780534.505721803, -802954287.5887265]]` |
| `gen0470` | 6 / `3.6124e-14` | `[[30411.56311392412, 30538.255424358227, 2722225.4981703223]]` → `[[30411.02807801994, 30538.749829700955, 2722215.508914114]]` |
| `gen0520` | 6 / `2.0478e-14` | `[[2603.181323610652, 101149.6432820234, -150409.28755995663]]` → `[[4478.212077142718, 91793.59915509017, -110231.78941596425]]` |
| `gen0975` | 3 / `2.4293e-14` | `[[-0.07499960999706787, 0.28311173511413507, -1.263414230268762]]` → `[[0.0019536212055459258, 0.26423357801083436, -0.039611592564807424]]` |
| `gen1005` | 9 / `9.107e-14` | `[[28217.743974127192, 38093.608383810904, 1863000.1361175987]]` → `[[28256.151890684785, 38207.22373499281, 1865588.3408999464]]` |
| `gen1026` | 3 / `9.0952e-15` | `[[45771.34220991706, 45864.49375112448, 577600.5831079823]]` → `[[45772.14833280894, 45865.584040146976, 577658.7064678086]]` |
| `gen1047` | 3 / `8.4307e-14` | `[[31208.26635863553, 32223.47216717072, 562497.5647403828]]` → `[[31221.666358372844, 32308.728980748914, 563452.3343034461]]` |
| `gen1501` | 6 / `4.6519e-15` | `[[76938.6903388469, 77235.62454754181, 6127941.733940306]]` → `[[76034.63657112679, 77163.63088496617, 6061737.160377767]]` |
| `gen1815` | 3 / `7.9991e-14` | `[[30455.168428658406, 30493.41594569247, 1691412.366477158]]` → `[[30459.764928504002, 30490.46183200792, 1691573.0880891723]]` |
| `gen2309` | 3 / `2.8838e-13` | `[[-2130345613.6145353, 50165393450.44275, -376803058672.42773]]` → `[[-2129529154.1441603, 50151508983.73155, -376729189548.8684]]` |
| `gen2448` | 9 / `1.1976e-14` | `[[35233.48643230344, 35764.10016249043, 1427262.1225083272]]` → `[[19456.819917104032, 54845.74218580647, 1214690.186811323]]` |
| `gen2507` | 6 / `1.6393e-14` | `[[60864.74652840865, 60955.2526814626, 3623994.1771435984]]` → `[[60872.637104188514, 60959.125270976685, 3624559.095594456]]` |
| `gen2629` | 3 / `9.9247e-15` | `[[-0.7952289704799329, 21.961465706434275, -1.3129666919095415]]` → `[[-0.3803237369067746, 22.131710411144315, 10.81145447121247]]` |
| `gen2869` | 12 / `5.2903e-15` | `[[43578.856643134844, 50713.272901409575, 2669836.6176526896]]` → `[[45556.57516403698, 48850.43526261595, 2728328.359954016]]` |


The replacement rows equal the collector actually run at `7a90407a0fe189ebceb551e5df28aaf4e8cc9893`, raw SHA-256 `735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`; only these eighteen rows change. All **3,182 cases**, source inputs, unrelated expectations, topology/status fields and **85 known failures** are retained. Code, exact requested/clamped points, all full-precision observations (including disagreements), inputs, controls and actual checks are retained in the readback-verified 37-member archive SHA-256 `40de6d3665e90d04529ac1c6c36cc760ee9c4cccb093dc65aae69cd595f09bf7`. No new B-rep, global fold or volume result is accepted here.
