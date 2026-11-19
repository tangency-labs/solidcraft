# Initial-handoff derivations

Source: [initial-35.json](initial-35.json), captured by the queued audit of exactly 35 original changed rows. Membership is initial output `raw_after.out[1] == 1`, yielding 25 rows. Corrected outputs were not used to select rows. Exact arithmetic and vectors: [handoff-bounds.json](handoff-bounds.json).

For each generator `C(s)` and stored sweep vector `E`, `S(s,v)=C(s)+vE`. For the target plane with stored normal `n` and origin `O`, its signed residual is `n·(C(s)−O)+v(n·E)`. Therefore relative to the generator point, the maximum normal displacement over the full sweep interval is `D=M|n·E|/|n|`, where `M=max|v|` also includes caller endpoints when supplied. The certificate is `B=(|fl(n·E)|+gamma(5)Σ|fl(nᵢEᵢ)|) M (1+gamma(2))`, `gamma(k)=ku/(1−ku)`, `u=2^-53`.

Products and sums of the actual stored binary64 components were evaluated with 2500-digit Decimal arithmetic and independently checked against exact rational arithmetic (`Fraction`). `B` reproduces the explicit binary64 operation order. Printed values below are rounded to 14 significant digits; supporting JSON retains the full Decimal numbers, exact dot fractions, vectors and outputs.

All 25 rows have **physical stored-input drift above tolerance**, with the certificate also above tolerance. None is solely a rounding-certificate refusal, and none lacks a drift explanation. In particular, values at zero tolerance remain strictly nonzero even when ordinary binary64 dot accumulation cancels to zero. These are results for the stored plane-normal contract, not a claim about an ideal intended perpendicular vector before input rounding.

This proves that treating an entire directrix as in-plane under the requested tolerance is unjustified. It does **not** prove that every old returned directrix missed tolerance after bounded-plane clipping, nor that an old empty intersection was wrong; that would require solving/clipping the exact intersections. A needs-more handoff correctly declines this special construction and delegates to the general solver.

| Case | Exact-real n·E | M | Effective tol | Physical D | Certificate B | Initial before → after |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| adv_tilt_1e-09 | 2.0000000000000e-9 | 1234567 | 0.0000010000000000000 | 0.0024691340000000 | 0.0024691340000000 | out n=55 → needs-more, no curves |
| gen0005 | -2.5528540120268e-17 | 1234567 | 1.0000000000000e-12 | 3.1516693190659e-11 | 1.9292414240214e-9 | empty → needs-more, no curves |
| gen0008 | -6.2572584719801e-17 | 1234567 | 1.0000000000000e-12 | 7.7250048199771e-11 | 2.2846246795683e-9 | empty → needs-more, no curves |
| gen0029 | -2.1690776339145e-18 | 1234567 | 0 | 2.6778716672690e-12 | 5.4388394058204e-11 | empty → needs-more, no curves |
| gen0049 | 5.7530773557136e-18 | 1234567 | 0 | 7.1025594518112e-12 | 2.8995700026968e-10 | out n=31 → needs-more, no curves |
| gen0176 | -2.7284802526576e-18 | 1234567 | 1.0000000000000e-12 | 3.3684916800828e-12 | 3.4284940755117e-10 | out n=55 → needs-more, no curves |
| gen0179 | 7.5148585654760e-18 | 1 | 0 | 7.5148585654760e-18 | 7.5148585657699e-18 | out n=31 → needs-more, no curves |
| gen0344 | -4.7944565781051e-18 | 1234567 | 1.0000000000000e-12 | 5.9190778742614e-12 | 2.4268910583279e-10 | out n=31 → needs-more, no curves |
| gen0350 | -6.2053312504907e-17 | 2.5 | 0 | 1.5513328126227e-16 | 3.5096863613157e-15 | empty → needs-more, no curves |
| gen0377 | 1.5632712267159e-17 | 2.5 | 0 | 3.9081780667898e-17 | 3.9081780667898e-17 | empty → needs-more, no curves |
| gen0429 | -1.5383667915373e-25 | 2.5 | 0 | 3.8459169788433e-25 | 2.7648995256392e-24 | empty → needs-more, no curves |
| gen0484 | -3.1074262638784e-16 | 1234567 | 0 | 3.8363259203175e-10 | 1.5387961993738e-9 | out n=79 → needs-more, no curves |
| gen0554 | 3.8843367941387e-16 | 1234567 | 0 | 4.7954740229295e-10 | 2.1528742685528e-9 | out n=31 → needs-more, no curves |
| gen0594 | 3.4025681435016e-16 | 1234567 | 0 | 4.2006983452184e-10 | 3.0686832097724e-9 | out n=31 → needs-more, no curves |
| gen0614 | -1.5660532382085e-17 | 1234567 | 1.0000000000000e-12 | 1.9333976481354e-11 | 1.5414094766746e-10 | empty → needs-more, no curves |
| gen0617 | 3.4277723889230e-17 | 1.3520127452482 | 0 | 4.6343919576339e-17 | 2.1320054782401e-15 | out n=55 → needs-more, no curves |
| gen0622 | 2.8521575943456e-17 | 2.5 | 0 | 7.1303939858640e-17 | 7.6119485301270e-16 | out n=31 → needs-more, no curves |
| gen0709 | -6.3152092180894e-17 | 1234567 | 1.0000000000000e-12 | 7.7965488987489e-11 | 5.8168016617185e-10 | empty → needs-more, no curves |
| gen0789 | 5.5104270874890e-14 | 1234567 | 1.0000000000000e-12 | 6.8029914381200e-8 | 5.4558998155058e-7 | empty → needs-more, no curves |
| gen0822 | 3.0652960633803e-17 | 1234567 | 1.0000000000000e-12 | 3.7843133650792e-11 | 3.7843133650792e-11 | out n=103 → needs-more, no curves |
| gen0869 | 4.0594887742425e-28 | 1234567 | 0 | 5.0117108775502e-22 | 3.3416993632812e-21 | empty → needs-more, no curves |
| gen0924 | -7.0055572353446e-18 | 1234567 | 1.0000000000000e-12 | 8.6488297793676e-12 | 1.2975190874624e-10 | out n=55 → needs-more, no curves |
| gen1071 | -4.2431807415683e-17 | 1234567 | 0 | 5.2384909185757e-11 | 1.0099587633433e-9 | empty → needs-more, no curves |
| gen1117 | -1.3042532572332e-17 | 1 | 0 | 1.3042532572332e-17 | 1.3042532576112e-17 | out n=55 → needs-more, no curves |
| gen1161 | 2.9637721623672e-19 | 1234567 | 0 | 3.6589753071771e-13 | 3.6589753075674e-13 | out n=31 → needs-more, no curves |

## Per-row geometric derivations

### adv_tilt_1e-09

The circle generator is swept by `E=[0.0, 0.0, 2.0]`; target stored normal `n=[1.0, 0.0, 1e-09]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=2.0000000000000e-9`; therefore `D=1234567 × |2.0000000000000e-9| / 1.0000000000000 = 0.0024691340000000`. The implemented conservative bound is `B=0.0024691340000000`. Both exceed effective tolerance `0.0000010000000000000`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

Here the full exact intersection is explicit: with ε=1e-9, `x=0.3−2εv`, `y=±sqrt(1−x²)`, `z=2v`. Over `|v|≤1234567`, old fixed-`x=0.3` directrices miss the plane by up to `0.002469134`; the target is unbounded, so clipping does not remove that error. Their previous zero deviation was demonstrably false.

### gen0005

The b_spline generator is swept by `E=[-1.0522055751702672, -1.4963086667018128, 3.130525389784795]`; target stored normal `n=[-0.39365004085966326, 0.8738457992347527, 0.2853649671753033]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[-1234567.0, 1234567.0]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-2.5528540120268e-17`; therefore `D=1234567 × |-2.5528540120268e-17| / 1.0000000000000 = 3.1516693190659e-11`. The implemented conservative bound is `B=1.9292414240214e-9`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0008

The b_spline generator is swept by `E=[-1.7773058534620032, -4.380337069006313, 0.36197354046379043]`; target stored normal `n=[0.8469949839449336, -0.37481701465485373, -0.3769770585822764]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-6.2572584719801e-17`; therefore `D=1234567 × |-6.2572584719801e-17| / 1.0000000000000 = 7.7250048199771e-11`. The implemented conservative bound is `B=2.2846246795683e-9`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0029

The b_spline generator is swept by `E=[0.035400047288508624, 0.4106648171955617, 0.3322111696651475]`; target stored normal `n=[-0.9952306369500918, 0.09662596502584292, -0.01339410910584904]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-2.1690776339145e-18`; therefore `D=1234567 × |-2.1690776339145e-18| / 1.0000000000000 = 2.6778716672690e-12`. The implemented conservative bound is `B=5.4388394058204e-11`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0049

The circle generator is swept by `E=[-0.06308907833410918, -0.21606941751311756, -0.9743376083321914]`; target stored normal `n=[0.51662116237014, 0.8282279932229879, -0.21711970392653906]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=5.7530773557136e-18`; therefore `D=1234567 × |5.7530773557136e-18| / 1.0000000000000 = 7.1025594518112e-12`. The implemented conservative bound is `B=2.8995700026968e-10`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0176

The circle generator is swept by `E=[0.2818090991921963, -0.03995071150950154, -0.6264980352132926]`; target stored normal `n=[0.833455067179157, -0.38202343606673195, 0.3992627521935331]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-2.7284802526576e-18`; therefore `D=1234567 × |-2.7284802526576e-18| / 1.0000000000000 = 3.3684916800828e-12`. The implemented conservative bound is `B=3.4284940755117e-10`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0179

The circle generator is swept by `E=[0.95007626533038, 9.5007626533038e-13, 0.0]`; target stored normal `n=[-2.9354296771089144e-13, 0.29355087745334024, 0.9559434514375702]`. The full sweep interval is `[0.0, 1.0]`, caller sweep endpoints `[]`, swapped UV `True`. Thus `M=1`.

Exact-real `n·E=7.5148585654760e-18`; therefore `D=1 × |7.5148585654760e-18| / 1.0000000000000 = 7.5148585654760e-18`. The implemented conservative bound is `B=7.5148585657699e-18`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0344

The b_spline generator is swept by `E=[-0.27383779184260404, 0.12641194856334412, -0.2309211609940722]`; target stored normal `n=[-0.6465944961515798, -0.638682522072985, 0.4171333042865022]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-4.7944565781051e-18`; therefore `D=1234567 × |-4.7944565781051e-18| / 1.0000000000000 = 5.9190778742614e-12`. The implemented conservative bound is `B=2.4268910583279e-10`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0350

The b_spline generator is swept by `E=[1.941639193874112, 1.1371130771434612, 1.550721574155378]`; target stored normal `n=[0.6255012666371976, -0.7428995850443316, -0.23842896631118737]`. The full sweep interval is `[-1.0, 2.5]`, caller sweep endpoints `[]`, swapped UV `True`. Thus `M=2.5`.

Exact-real `n·E=-6.2053312504907e-17`; therefore `D=2.5 × |-6.2053312504907e-17| / 1.0000000000000 = 1.5513328126227e-16`. The implemented conservative bound is `B=3.5096863613157e-15`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0377

The circle generator is swept by `E=[0.0, 0.0, 1.1264556342587]`; target stored normal `n=[0.5364351602489351, -0.8439415375775144, 1.3877787807814457e-17]`. The full sweep interval is `[-1.0, 2.5]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=2.5`.

Exact-real `n·E=1.5632712267159e-17`; therefore `D=2.5 × |1.5632712267159e-17| / 1.0000000000000 = 3.9081780667898e-17`. The implemented conservative bound is `B=3.9081780667898e-17`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0429

The b_spline generator is swept by `E=[1000.0, 1e-09, 0.0]`; target stored normal `n=[-9.961600378708533e-13, 0.996160037870853, 0.08755100769917365]`. The full sweep interval is `[-1.0, 2.5]`, caller sweep endpoints `[]`, swapped UV `True`. Thus `M=2.5`.

Exact-real `n·E=-1.5383667915373e-25`; therefore `D=2.5 × |-1.5383667915373e-25| / 1.0000000000000 = 3.8459169788433e-25`. The implemented conservative bound is `B=2.7648995256392e-24`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0484

The b_spline generator is swept by `E=[-0.843163214535329, 1.4822038515595077, 1.4205892713889068]`; target stored normal `n=[-0.8122750819518326, -0.5719055317937307, 0.11460040987655182]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-3.1074262638784e-16`; therefore `D=1234567 × |-3.1074262638784e-16| / 1.0000000000000 = 3.8363259203175e-10`. The implemented conservative bound is `B=1.5387961993738e-9`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0554

The b_spline generator is swept by `E=[-1.2445606219835765, 1.9388434380635813, -1.4027557037216636]`; target stored normal `n=[0.7492580619928624, 0.6296031390340762, 0.20545618476244015]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=3.8843367941387e-16`; therefore `D=1234567 × |3.8843367941387e-16| / 1.0000000000000 = 4.7954740229295e-10`. The implemented conservative bound is `B=2.1528742685528e-9`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0594

The parabola generator is swept by `E=[2.885388504455174, 0.2764082206683943, 2.5164073507844402]`; target stored normal `n=[-0.6074671329162303, -0.311432426783914, 0.7307486065494871]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=3.4025681435016e-16`; therefore `D=1234567 × |3.4025681435016e-16| / 1.0000000000000 = 4.2006983452184e-10`. The implemented conservative bound is `B=3.0686832097724e-9`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0614

The b_spline generator is swept by `E=[-0.1533155795295638, 0.6569127105215488, -0.12955775499012162]`; target stored normal `n=[-0.6361726597046304, 0.003689285650832013, 0.7715379032916807]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-1.5660532382085e-17`; therefore `D=1234567 × |-1.5660532382085e-17| / 1.0000000000000 = 1.9333976481354e-11`. The implemented conservative bound is `B=1.5414094766746e-10`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0617

The b_spline generator is swept by `E=[1.9256226692773428, -0.29638835305305367, -4.408884765967529]`; target stored normal `n=[-0.737608836362482, -0.6140359817898706, -0.28087900880509953]`. The full sweep interval is `[-1.1161758937844475, 1.352012745248239]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1.3520127452482`.

Exact-real `n·E=3.4277723889230e-17`; therefore `D=1.3520127452482 × |3.4277723889230e-17| / 1.0000000000000 = 4.6343919576339e-17`. The implemented conservative bound is `B=2.1320054782401e-15`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0622

The b_spline generator is swept by `E=[0.15497366255566142, 0.21948426411842153, 0.9632288522041265]`; target stored normal `n=[0.31517450670462593, 0.9130750291712407, -0.258764412984927]`. The full sweep interval is `[-1.0, 2.5]`, caller sweep endpoints `[-1.0, 2.5]`, swapped UV `False`. Thus `M=2.5`.

Exact-real `n·E=2.8521575943456e-17`; therefore `D=2.5 × |2.8521575943456e-17| / 1.0000000000000 = 7.1303939858640e-17`. The implemented conservative bound is `B=7.6119485301270e-16`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0709

The b_spline generator is swept by `E=[-0.6863306812070288, 0.705515366359793, 0.1766303027911777]`; target stored normal `n=[0.34640517647560237, 0.5306537171979161, -0.7735697034753499]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-6.3152092180894e-17`; therefore `D=1234567 × |-6.3152092180894e-17| / 1.0000000000000 = 7.7965488987489e-11`. The implemented conservative bound is `B=5.8168016617185e-10`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0789

The ellipse generator is swept by `E=[490.0304491455558, 871.4253986499793, 22.08921677944796]`; target stored normal `n=[0.6418237064630239, -0.3778333094051492, 0.6673112618006826]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=5.5104270874890e-14`; therefore `D=1234567 × |5.5104270874890e-14| / 1.0000000000000 = 6.8029914381200e-8`. The implemented conservative bound is `B=5.4558998155058e-7`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0822

The b_spline generator is swept by `E=[1.1043892967055657, 0.0, 0.0]`; target stored normal `n=[2.7755575615628914e-17, 0.6919194905173872, 0.7219746662052342]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[-1234567.0, 1234567.0]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=3.0652960633803e-17`; therefore `D=1234567 × |3.0652960633803e-17| / 1.0000000000000 = 3.7843133650792e-11`. The implemented conservative bound is `B=3.7843133650792e-11`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen0869

The circle generator is swept by `E=[2.202932784801487, 2.202932784801487e-12, 0.0]`; target stored normal `n=[9.415863373377623e-13, -0.9415863373377621, -0.3367716872583831]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=4.0594887742425e-28`; therefore `D=1234567 × |4.0594887742425e-28| / 1.0000000000000 = 5.0117108775502e-22`. The implemented conservative bound is `B=3.3416993632812e-21`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen0924

The circle generator is swept by `E=[0.5945968803202525, 0.0075661524544968235, 2.3905994133616963]`; target stored normal `n=[0.14663128030509365, 0.9883983015340907, -0.03959880251490009]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `True`. Thus `M=1234567`.

Exact-real `n·E=-7.0055572353446e-18`; therefore `D=1234567 × |-7.0055572353446e-18| / 1.0000000000000 = 8.6488297793676e-12`. The implemented conservative bound is `B=1.2975190874624e-10`. Both exceed effective tolerance `1.0000000000000e-12`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen1071

The ellipse generator is swept by `E=[0.507780020527803, -3.8300848916087977, -0.770046074855485]`; target stored normal `n=[-0.5544540863576382, 0.09276784355016196, -0.8270276859479189]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=-4.2431807415683e-17`; therefore `D=1234567 × |-4.2431807415683e-17| / 1.0000000000000 = 5.2384909185757e-11`. The implemented conservative bound is `B=1.0099587633433e-9`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output was empty. This drift derivation does not establish a nonempty exact intersection; it establishes that sweep parallelism is insufficient to decide the empty answer. Do not label that old empty result geometrically wrong on this evidence.

### gen1117

The b_spline generator is swept by `E=[4.51765270481319, 4.51765270481319e-12, 0.0]`; target stored normal `n=[7.493450304707492e-13, -0.7493479174860699, 0.6621764859607221]`. The full sweep interval is `[0.0, 1.0]`, caller sweep endpoints `[]`, swapped UV `True`. Thus `M=1`.

Exact-real `n·E=-1.3042532572332e-17`; therefore `D=1 × |-1.3042532572332e-17| / 1.0000000000000 = 1.3042532572332e-17`. The implemented conservative bound is `B=1.3042532576112e-17`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

### gen1161

The hyperbola generator is swept by `E=[0.24172853442876363, 2.4172853442876364e-13, 0.0]`; target stored normal `n=[1.190991749666637e-13, -0.11909794889206336, 0.9928825099525641]`. The full sweep interval is `[-1234567.0, 1234567.0]`, caller sweep endpoints `[]`, swapped UV `False`. Thus `M=1234567`.

Exact-real `n·E=2.9637721623672e-19`; therefore `D=1234567 × |2.9637721623672e-19| / 1.0000000000000 = 3.6589753071771e-13`. The implemented conservative bound is `B=3.6589753075674e-13`. Both exceed effective tolerance `0`. Verdict: physical drift justifies declining the parallel-sweep special case.

The previous output contained curves. This derivation proves the full-sweep approximation is outside the allowed displacement; bounded clipping and exact generator-root accuracy could shorten or alter an individual old curve, so it does not independently certify every old output endpoint.

## Arithmetic checks

Each dot product is the sum of the three exact products of the stored binary64
components, treated as rational numbers. `handoff-bounds.json` retains their
numerators and denominators and the full high-precision substitutions into the
formula above. The physical drift calculation does not use a rounded binary64
dot product.

Exact dot cross-check: 25/25 agree with rational arithmetic. Physical drift
`D>tol`: 25/25. Certificate `B>=D`: 25/25. Classification: physical drift 25;
certificate-only refusal 0; unexplained handoff 0.
