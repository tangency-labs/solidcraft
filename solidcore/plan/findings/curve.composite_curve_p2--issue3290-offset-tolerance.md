# Issue 3290: changed generated offset cases

Base: SolidCore `4c862e55828970921b56fca3d42f032de6a26f89`, macOS Apple Silicon. Candidate and unmodified core were built in separate target directories.

All 43 changed cases contain a degree-one B-spline with at least one internal corner for which the endpoint offset gap `2 |d| sin(angle/2)` exceeds the case intersection tolerance. Subdivision exposes these corner events to the existing corner and intersection code. The output is a compressed flat geometric record; the old and new summaries below are exact stored values, not independent certificates.

Sampling: five parameter values per returned piece, checked against all original segments. The tolerance was `intersection tolerance + approximation tolerance`. This is a diagnostic, not a continuous clearance certificate. Untrimmed results were not subject to a clearance assertion. Seven cases retained pre-existing violations; all 19 sampled violating points across those seven cases lay on unmodified result curves, with maximum measured distance 6.3e-16.

The temporary sampler did not assess `gen0255` or `gen0320`: it mapped their unknown direction to an invalid direction rather than invoking the original wrapper. Their diagnostic error is not a kernel regression claim.

On this machine `gen0288` already differs from the checked-in upstream record without the patch. Its observed baseline and stored baseline are listed separately.

## gen0020

- Corner gap / tolerance: up to 392.605; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":581}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":416}`
- Baseline samples: `pieces=6 comparisons=390 unknown=0 violations=[(0.6800471998603738, 0.9149173082006619)]`
- Candidate samples: `pieces=6 comparisons=390 unknown=0 violations=[]`

## gen0030

- Corner gap / tolerance: up to 27.4226; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],[30.26709185796282,30.26709185796282,215.48878133127099]],"n":533}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],[28287.930188280247,28407.78943016124,2242534.7440120173]],"n":427}`
- Baseline samples: `pieces=6 comparisons=90 unknown=0 violations=[]`
- Candidate samples: `pieces=7 comparisons=105 unknown=0 violations=[]`

## gen0042

- Corner gap / tolerance: up to 7037.46; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":909}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":851}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0059

- Corner gap / tolerance: up to 12.6038; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],[13.442465900980224,27.812183458985317,219.9813047195761]],"n":535}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],[16.001576532967377,16.001576532967377,80.01782173686985]],"n":527}`
- Baseline samples: `pieces=5 comparisons=75 unknown=0 violations=[]`
- Candidate samples: `pieces=7 comparisons=105 unknown=0 violations=[]`

## gen0074

- Corner gap / tolerance: up to 35.3197; 7 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":2286}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[56094.9285573118,56412.1036264552,4599285.146303986]],"n":1778}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0102

- Corner gap / tolerance: up to 16.9125; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":298}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":350}`
- Baseline samples: `pieces=4 comparisons=80 unknown=0 violations=[]`
- Candidate samples: `pieces=6 comparisons=120 unknown=0 violations=[]`

## gen0113

- Corner gap / tolerance: up to 125.619; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":403}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":391}`
- Baseline samples: `pieces=4 comparisons=60 unknown=0 violations=[]`
- Candidate samples: `pieces=7 comparisons=105 unknown=0 violations=[]`

## gen0119

- Corner gap / tolerance: up to 1827.26; 3 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":946}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1109}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0137

- Corner gap / tolerance: up to 96.162; 5 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":2176}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":2152}`
- Baseline samples: `pieces=34 comparisons=2720 unknown=0 violations=[(0.2385526116060022, 0.3968282803506669), (0.382285894940826, 0.3968282803506669), (0.3547054870434751, 0.3968282803506669), (0.35552774819842536, 0.3968282803506669), (0.30957223703035325, 0.3968282803506669), (0.30957223703035325, 0.3968282803506669)]`
- Candidate samples: `pieces=40 comparisons=3200 unknown=0 violations=[(0.2385526116060022, 0.3968282803506669), (0.382285894940826, 0.3968282803506669), (0.3547054870434751, 0.3968282803506669), (0.35552774819842536, 0.3968282803506669), (0.30957223703035325, 0.3968282803506669), (0.30957223703035325, 0.3968282803506669)]`

## gen0187

- Corner gap / tolerance: up to 288.278; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[14187.012455418437,14189.012455418437,878537.4306479975]],"n":1141}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[56355.46037500249,56392.4654187917,6824652.979943588],[14111.536924425453,14113.536924425453,72372.6915999847]],"n":1087}`
- Baseline samples: `pieces=11 comparisons=330 unknown=0 violations=[]`
- Candidate samples: `pieces=14 comparisons=420 unknown=0 violations=[]`

## gen0202

- Corner gap / tolerance: up to 406.595; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1271}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1168}`
- Baseline samples: `pieces=18 comparisons=720 unknown=0 violations=[]`
- Candidate samples: `pieces=22 comparisons=880 unknown=0 violations=[]`

## gen0209

- Corner gap / tolerance: up to 52.7112; 3 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[0.0039920797209637955,0.0039920797209637955,0.0080314183700023]],"n":1037}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1132}`
- Baseline samples: `pieces=15 comparisons=450 unknown=0 violations=[]`
- Candidate samples: `pieces=22 comparisons=660 unknown=0 violations=[]`

## gen0217

- Corner gap / tolerance: up to 125.725; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1320}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1232}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0236

- Corner gap / tolerance: up to 391.232; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":990}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1004}`
- Baseline samples: `pieces=17 comparisons=595 unknown=0 violations=[(1.1463756957690734, 1.1494724016830726), (1.1463756957690734, 1.1494724016830726), (1.1209437045855548, 1.1494724016830726), (1.1209437045855548, 1.1494724016830726)]`
- Candidate samples: `pieces=18 comparisons=630 unknown=0 violations=[(1.1463756957690734, 1.1494724016830726), (1.1463756957690734, 1.1494724016830726), (1.1209437045855548, 1.1494724016830726), (1.1209437045855548, 1.1494724016830726)]`

## gen0245

- Corner gap / tolerance: up to 26.8684; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":558}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":485}`
- Baseline samples: `pieces=5 comparisons=75 unknown=0 violations=[(0.3630377301252828, 0.4149371715820774)]`
- Candidate samples: `pieces=9 comparisons=135 unknown=0 violations=[]`

## gen0255

- Corner gap / tolerance: up to 412.619; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[70623.35854080984,70705.22157647122,8220683.131782986],[33.14648132603949,33.14648132603949,178.3686848830051]],"n":1551}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[70547.84641193312,70640.5077637588,8980681.113128942],[14259.994008713658,14261.994008713658,527915.6207086546]],"n":1616}`
- Baseline samples: `offset-error: Failed(Failure { operation: "curve::composite_curve::p2::CompositeCurve::create_offsets_of_many_curves", reason: Curve(Failed("the offset direction is not 1, 2 or 3")), site: Site { edge: None, face: None, vertex: None, at: None }, fit: None, cause: None, entity: None, status: 1001, origin: crates/solidcore/src/error/curve.rs:273 })`
- Candidate samples: `offset-error: Failed(Failure { operation: "curve::composite_curve::p2::CompositeCurve::create_offsets_of_many_curves", reason: Curve(Failed("the offset direction is not 1, 2 or 3")), site: Site { edge: None, face: None, vertex: None, at: None }, fit: None, cause: None, entity: None, status: 1001, origin: crates/solidcore/src/error/curve.rs:273 })`

## gen0257

- Corner gap / tolerance: up to 41.8734; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":895}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1003}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0288

- Corner gap / tolerance: up to 2568.69; 4 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[14136.723824178474,14187.881377164651,750295.5996490137]],"n":1135}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[28190.899137217053,28299.174494779483,2061585.890756583]],"n":932}`
- Baseline samples: `pieces=15 comparisons=675 unknown=0 violations=[(0.602445389174896, 0.6774379998029175), (0.6063384794163341, 0.6774379998029175)]`
- Candidate samples: `pieces=16 comparisons=720 unknown=0 violations=[(0.602445389174896, 0.6774379998029175), (0.6063384794163341, 0.6774379998029175)]`

- Baseline observed locally: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[18.046480845348977,18.046480845348977,84.33723216294891]],"n":1044}`

## gen0296

- Corner gap / tolerance: up to 287.449; 3 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[56283.27691485414,56444.847792502376,7159680.317048395],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[42212.578026543284,42268.66034608852,2649607.749827582]],"n":2238}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[70446.17154041838,70502.6294836801,7834103.039333258],["-inf","inf","-inf"],["-inf","inf","-inf"],[42268.03308855988,42324.115408105135,4551318.145829505]],"n":2032}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0300

- Corner gap / tolerance: up to 8999.7; 3 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":417}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":700}`
- Baseline samples: `pieces=5 comparisons=150 unknown=0 violations=[]`
- Candidate samples: `pieces=10 comparisons=300 unknown=0 violations=[]`

## gen0305

- Corner gap / tolerance: up to 1.71873; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[14045.641034513636,14172.57710756419,603597.0089736654]],"n":862}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[-2.923854591806914,77.21758849042719,340.13239657589804]],"n":809}`
- Baseline samples: `pieces=7 comparisons=140 unknown=0 violations=[]`
- Candidate samples: `pieces=9 comparisons=180 unknown=0 violations=[]`

## gen0314

- Corner gap / tolerance: up to 1013.04; 3 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1887}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1792}`
- Baseline samples: `pieces=26 comparisons=780 unknown=0 violations=[(0.6014259459981554, 0.8007262642065863), (0.3494126353812492, 0.8007262642065863), (0.11292629985719423, 0.8007262642065863), (0.3352066640874945, 0.8007262642065863), (0.6821841977872537, 0.8007262642065863)]`
- Candidate samples: `pieces=34 comparisons=1020 unknown=0 violations=[(0.6821841977872537, 0.8007262642065863)]`

## gen0320

- Corner gap / tolerance: up to 455.004; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[69906.31312938509,70653.36807194188,8392076.86816882],[-4.585243417904177,38.185861327111766,79.3471603302628]],"n":1307}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[55845.10798611939,56645.341496881534,7104070.015802763],[4.0,4.0,21.5]],"n":1287}`
- Baseline samples: `offset-error: Failed(Failure { operation: "curve::composite_curve::p2::CompositeCurve::create_offsets_of_many_curves", reason: Curve(Failed("the offset direction is not 1, 2 or 3")), site: Site { edge: None, face: None, vertex: None, at: None }, fit: None, cause: None, entity: None, status: 1001, origin: crates/solidcore/src/error/curve.rs:273 })`
- Candidate samples: `offset-error: Failed(Failure { operation: "curve::composite_curve::p2::CompositeCurve::create_offsets_of_many_curves", reason: Curve(Failed("the offset direction is not 1, 2 or 3")), site: Site { edge: None, face: None, vertex: None, at: None }, fit: None, cause: None, entity: None, status: 1001, origin: crates/solidcore/src/error/curve.rs:273 })`

## gen0324

- Corner gap / tolerance: up to 105.988; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":284}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":403}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0343

- Corner gap / tolerance: up to 1585.45; 3 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[14144.284095757057,14144.284095757057,300792.47332525067]],"n":869}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[9.322568017354379e-05,9.322568017354379e-05,9.322568017354379e-05]],"n":769}`
- Baseline samples: `pieces=6 comparisons=150 unknown=0 violations=[]`
- Candidate samples: `pieces=13 comparisons=325 unknown=0 violations=[]`

## gen0372

- Corner gap / tolerance: up to 508.53; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1963}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1675}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0374

- Corner gap / tolerance: up to 343.809; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1515}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1655}`
- Baseline samples: `pieces=23 comparisons=920 unknown=0 violations=[(0.3450891867384873, 0.4184989151552266)]`
- Candidate samples: `pieces=30 comparisons=1200 unknown=0 violations=[(0.3922703539944784, 0.4184989151552266), (0.3450891867384873, 0.4184989151552266)]`

## gen0381

- Corner gap / tolerance: up to 576.01; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[14147.871024928358,14155.846928802293,468260.1351543502]],"n":1884}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[42347.280916717405,42406.35467968545,4295359.482172574]],"n":1761}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0408

- Corner gap / tolerance: up to 2167.06; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":781}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":809}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0413

- Corner gap / tolerance: up to 2.78802; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":274}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":273}`
- Baseline samples: `pieces=4 comparisons=40 unknown=0 violations=[]`
- Candidate samples: `pieces=5 comparisons=50 unknown=0 violations=[]`

## gen0427

- Corner gap / tolerance: up to 49.3675; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[56254.73642038929,56318.802523685314,4928151.748214143],[0.0011757386623617368,0.0011757386623617368,0.0015156787552388531]],"n":2562}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[56251.40537973089,56402.16041551191,7321880.915665733],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[14097.003404117728,14129.610784831606,424770.69518196705]],"n":2673}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0436

- Corner gap / tolerance: up to 514.595; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1204}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1180}`
- Baseline samples: `pieces=16 comparisons=320 unknown=0 violations=[]`
- Candidate samples: `pieces=22 comparisons=440 unknown=0 violations=[]`

## gen0441

- Corner gap / tolerance: up to 216.704; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[0.0005181313675268385,0.0005181313675268385,0.004045387753253796]],"n":3367}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":3294}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0452

- Corner gap / tolerance: up to 6.77462; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":990}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1098}`
- Baseline samples: `pieces=17 comparisons=510 unknown=0 violations=[]`
- Candidate samples: `pieces=20 comparisons=600 unknown=0 violations=[]`

## gen0467

- Corner gap / tolerance: up to 2183.35; 6 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[-1.7976931348623157e+308,1.7976931348623157e+308,"-inf"],[28358.91394730109,28362.91394730109,884937.0794870213]],"n":1138}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[70597.19157735571,70613.13354450006,10724731.527365606],[42663.90158347186,42669.90158347186,4434301.9731746875]],"n":1246}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0488

- Corner gap / tolerance: up to 117.529; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1227}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1215}`
- Baseline samples: `pieces=20 comparisons=1000 unknown=0 violations=[]`
- Candidate samples: `pieces=23 comparisons=1150 unknown=0 violations=[]`

## gen0507

- Corner gap / tolerance: up to 225.224; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1345}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1168}`
- Baseline samples: `pieces=16 comparisons=320 unknown=0 violations=[(0.6300248625203417, 0.6548905685528392), (0.6442934707799023, 0.6548905685528392), (0.6442934707799023, 0.6548905685528392)]`
- Candidate samples: `pieces=19 comparisons=380 unknown=0 violations=[]`

## gen0534

- Corner gap / tolerance: up to 8458.59; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1575}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[29.000071662314355,29.000071662314355,29.000861209251035]],"n":1566}`
- Baseline samples: `pieces=23 comparisons=690 unknown=0 violations=[(2.125444929733802, 2.1475124126123353)]`
- Candidate samples: `pieces=29 comparisons=870 unknown=0 violations=[(2.125444929733802, 2.1475124126123353)]`

## gen0539

- Corner gap / tolerance: up to 1912.45; 5 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[70824.2261821255,70865.31125932992,9356455.54352876],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[42372.42721861097,42378.42721861097,2888992.1275568916]],"n":2507}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[70824.2261821255,70865.31125932992,9356455.54352876],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[70518.41725815498,70528.41725815498,8187420.948018191],[52.00772880980775,52.00772880980775,368.22449448323226]],"n":2611}`
- Baseline samples: `pieces=39 comparisons=3510 unknown=0 violations=[(0.5525682465290455, 0.5729706164745375), (0.5654772552228539, 0.5729706164745375), (0.5282925227324692, 0.5729706164745375)]`
- Candidate samples: `pieces=43 comparisons=3870 unknown=0 violations=[(0.5525682465290455, 0.5729706164745375), (0.5654772552228539, 0.5729706164745375), (0.5282925227324692, 0.5729706164745375)]`

## gen0543

- Corner gap / tolerance: up to 30.9928; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[28180.680393520135,28294.73395464132,1327600.4945884764]],"n":1165}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],[14129.443174722384,14236.773054116404,566525.3234014102]],"n":877}`
- Baseline samples: `pieces=9 comparisons=135 unknown=0 violations=[(0.586309145755306, 0.6410904026527351)]`
- Candidate samples: `pieces=15 comparisons=225 unknown=0 violations=[]`

## gen0549

- Corner gap / tolerance: up to 489.987; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1821}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"],["-inf","inf","-inf"]],"n":1862}`
- Baseline samples: `untrimmed, no clearance assertion`
- Candidate samples: `untrimmed, no clearance assertion`

## gen0558

- Corner gap / tolerance: up to 1.75854; 1 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],[42298.02617453863,42327.416668244725,3258194.4371347902]],"n":684}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],[70501.63012226544,70535.02061597155,8459270.90562781],[14.103676872529505,14.103676872529505,70.74890436377224]],"n":779}`
- Baseline samples: `pieces=11 comparisons=275 unknown=0 violations=[]`
- Candidate samples: `pieces=13 comparisons=325 unknown=0 violations=[]`

## gen0579

- Corner gap / tolerance: up to 51.8884; 2 internal corners exceed tolerance.
- Stored upstream `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],[28162.42820441039,28176.705162006445,1607117.06053579]],"n":659}`
- Candidate `values.out`: `{"blocks":[["-inf","inf","-inf"],["-inf","inf","-inf"],[42227.721012159695,42252.955917534106,3423200.572174986]],"n":711}`
- Baseline samples: `pieces=10 comparisons=350 unknown=0 violations=[(0.14785677660287064, 0.16849968664890264), (0.15503361506838456, 0.16849968664890264)]`
- Candidate samples: `pieces=12 comparisons=420 unknown=0 violations=[]`
