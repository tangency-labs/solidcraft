# Plane/extrusion interval-union derivations

Inputs and original PR raw outputs: [initial-35.json](initial-35.json). Arithmetic, exact source values, 3D endpoints and intermediate segment comparisons are retained in [interval-unions.json](interval-unions.json). “Intermediate” means restored planar classification before bounded-fiber verification; it is included to explain why merely restoring the old segment counts would be wrong.

For a generator root s_i, directrix endpoints are C(s_i)+v_min E and C(s_i)+v_max E. Several roots on the same sweep fiber contribute the **union** of their finite intervals. Re-solving the directrix origin can choose the first root again and collapse distinct intervals. Restored planar classification must retain only intervals inside/on the sheet. Its exterior `Nothing` marker is erased to `Unknown` when no object is supplied, and normal projection can then mark an outside interval as `Surface`. Bounded-fiber verification rejects those intervals geometrically. Counts alone are insufficient evidence.

Arithmetic uses 85-digit Decimal evaluation of stored binary64 control nets and scalar plane equations. Polynomial roots are isolated on every knot span using derivative partitions and bisection. Circle roots use the analytic sine equation. Only gen0786 has nonzero stored n·E among these five (−5.717278610088272e−20). It is piecewise linear: a root branch is t(v)=−(f0+v(n·E))/f1, hence its exact swept position is affine in v with derivative E−(n·E)(P1−P0)/f1; the Y derivative retains E_y’s negative sign, so its extrema occur at the two analytic V endpoints. It was solved at both endpoints; the listed extents include that input-rounding drift. The other four have exactly n·E=0, so their root branches are fixed and the finite directrices are exact apart from displayed arithmetic rounding. Values are printed to 16 significant digits; supporting JSON retains the higher precision. Normal-plane marker boxes do not clip any true intervals: their endpoint distances from the plane origin are below 1234567, bounding each unit-axis UV coordinate. Both explicitly bounded planes are clipped using their affine UV inequalities.

| Case | Coordinate | True union | Original PR | Intermediate planar classification |
| --- | --- | --- | --- | --- |
| gen0193 | y | [-2.449315091161047, 20.4676765312372] | 2 duplicate first-root intervals | 1 segments; 0 exterior tail(s) |
| gen0786 | y | [-745.0322444929938, 738.5299357128995] | 2 duplicate first-root intervals; misses other-root tail | 5 segments; 2 exterior tail(s) |
| gen0906 | y | [-1.403626447781622, 3.445224651418103] | 2 duplicate first-root intervals; misses other-root tail | 4 segments; 1 exterior tail(s) |
| gen0944 | y | [-15.23796365140969, -0.0412184393736986] | 2 duplicate first-root intervals; misses other-root tail | 5 segments; 2 exterior tail(s) |
| gen1099 | x | [-777803.1535292441, 777807.4182424814] | 2 duplicate first-root intervals; misses other-root tail | 3 segments; 0 exterior tail(s) |

## gen0193

The generator is C(θ)=(-3.383847874523682, 5.666749032502601+r cosθ, .5643857631864648+r sinθ), r=1.0168320677756253, θ∈[-50.38195298825951°,309.6180470117405°]. Since n_y=0, substitute constant x in n·(C−O)=0: 1.0129305701307193 sinθ + .7759802435873979 = 0. The two attained angles below are the only roots over this complete turn. Both full sweep intervals cover the plane box; intersecting the two affine plane UV slab constraints gives the same clipped Y interval for each. Thus the original two copies duplicate exactly the same answer; restored one segment is correct.

Sweep vector E=(0, 0.23960635348037, 0); analytic V=[-1234567, 1234567]; exact-real stored n·E=0.

| Root parameter | Generator point C | Root interval in Y after target clipping |
| ---: | --- | --- |
| -50.00267602848287 | (-3.383847874523682, 6.320319705366997, -0.214583318118072) | [-2.449315091161047, 20.4676765312372] |
| 230.0026760284829 | (-3.383847874523682, 5.013178359638204, -0.2145833181180717) | [-2.449315091161047, 20.4676765312372] |

True union: **[-2.449315091161047, 20.4676765312372]**.

Original two raw intervals (both the same clipped region within rounding):

- Original segment 1: Y [-2.449315091100289, 20.46767653126153].
- Original segment 2: Y [-2.449315091158496, 20.46767653126153].

| Intermediate segment | Coordinate interval | Verdict |
| ---: | --- | --- |
| 1 | [-2.44931509116104, 20.46767653123721] | valid partition of true union |

Independent scalar equations (local t∈[0,1] on the listed native knot span):

- 0 cosθ + (1.012930570130719) sinθ + (0.7759802435873979) = 0.

## gen0786

The degree-one B-spline is the three line segments joining the four given controls. On each span let t=(s−s_lo)/(s_hi−s_lo). Signed plane residual is affine, with the three (constant,linear) coefficient pairs listed below. First span has no sign change; second and third each have exactly one simple root. At each root use C(s)+vE, with the actual marker-limited analytic V=[−1234567,1234567]. The roots differ along the sweep fiber, so their intervals overlap but their endpoints differ. Their union must retain the second-root lower-Y tail. The original directrix solve realigns the second root to the first and emits the first interval twice. The intermediate classification restores the missing valid tail but also keeps two known-exterior marker tails.

Sweep vector E=(-0.0005764323441651645, -0.0006007187349253057, -0.0005539519420581482); analytic V=[-1234567, 1234567]; exact-real stored n·E=-5.717278610088272e-20.

| Root parameter | Generator point C | Root interval in Y after target clipping |
| ---: | --- | --- |
| 1.872943285802458 | (-0.3634305392906191, -3.097590707630502, -4.39026524419912) | [-744.7251171281605, 738.5299357128995] |
| 2.876958194619061 | (-0.6581410861327649, -3.404718072463854, -4.673482314547011) | [-745.0322444929938, 738.2228083480661] |

True union: **[-745.0322444929938, 738.5299357128995]**.

Original missing coverage: [-745.0322444929938, -744.7251171281605]. Original two raw intervals:

- Original segment 1: Y [-744.7251171281604, 738.5299357128994].
- Original segment 2: Y [-744.7251171281604, 738.5299357128994].

| Intermediate segment | Coordinate interval | Verdict |
| ---: | --- | --- |
| 1 | [-741630.7242217293, -745.0322444929938] | outside sheet: [-741630.7242217293, -745.0322444929938] |
| 2 | [-745.0322444929938, -744.7251171281604] | valid partition of true union |
| 3 | [-744.7251171281604, 738.2228083480659] | valid partition of true union |
| 4 | [738.2228083480659, 738.5299357128994] | valid partition of true union |
| 5 | [738.5299357128994, 741624.3286193303] | outside sheet: [738.5299357128995, 741624.3286193303] |

Independent scalar equations (local t∈[0,1] on the listed native knot span):

- Span [0.3527940115372181, 1.463994604131021]: (-1.53831235127074)t^0 + (0.7372717158413337)t^1 = 0.
- Span [1.463994604131021, 2.479049215290715]: (-0.8010406354294062)t^0 + (1.988269010663255)t^1 = 0.
- Span [2.479049215290715, 3.340637755675829]: (1.187228375233849)t^0 + (-2.570694344842731)t^1 = 0.

## gen0906

The degree-two B-spline has one interior knot k=.5052645141995069. Plane normal has n_y=0 and all controls have x=0, so its intersection condition reduces exactly to z(s)=.6777608093418188. On each knot span, substitute the quadratic de Boor expression and use local t∈[0,1]; the residual polynomial coefficients are below. The derivative-partitioned quadratic roots give one root in each span. Sweeping each root by E=Y, v∈[−1,2.5], gives two overlapping intervals; their union includes the second-root upper tail. The original outputs duplicate only the first interval. The intermediate retains the correct three internal partitions, plus a fourth exterior interval below the true minimum.

Sweep vector E=(0, 1, 0); analytic V=[-1, 2.5]; exact-real stored n·E=0.

| Root parameter | Generator point C | Root interval in Y after target clipping |
| ---: | --- | --- |
| 0.1216584518464938 | (0, -0.4036264477816225, 0.6777608093418188) | [-1.403626447781622, 2.096373552218378] |
| 0.996942697668908 | (0, 0.9452246514181026, 0.6777608093418188) | [-0.0547753485818974, 3.445224651418103] |

True union: **[-1.403626447781622, 3.445224651418103]**.

Original missing coverage: [2.096373552218378, 3.445224651418103]. Original two raw intervals:

- Original segment 1: Y [-1.403626447781622, 2.096373552218378].
- Original segment 2: Y [-1.403626447781622, 2.096373552218378].

| Intermediate segment | Coordinate interval | Verdict |
| ---: | --- | --- |
| 1 | [2.096373552218374, 3.445224651418101] | valid partition of true union |
| 2 | [-0.0547753485818987, 2.096373552218374] | valid partition of true union |
| 3 | [-1.403626447781619, -0.0547753485818987] | valid partition of true union |
| 4 | [-22.32411219073524, -1.403626447781619] | outside sheet: [-22.32411219073524, -1.403626447781622] |

Independent scalar equations (local t∈[0,1] on the listed native knot span):

- Span [0, 0.5052645141995069]: (-0.2863028500830709)t^0 + (1.389857999243062)t^1 + (-0.8339601241662116)t^2 = 0.
- Span [0.5052645141995069, 1]: (0.2695950249937795)t^0 + (-0.2722678082864206)t^1 + (0.001002611251839123)t^2 = 0.

## gen0944

The generator is two line segments (degree one, knot .5), at constant x=2.844316555831364. Substituting into the plane equation gives z*=−1.1515357404828535. Each line segment crosses this z exactly once. Linear interpolation supplies the two native parameters and generator Y values below. Add 4.076940246672004×v with v∈[−4.638756436755019,−1.160726592388202] to each root Y. Both root intervals overlap; union retains the second-root upper-Y tail. Original outputs duplicate the first interval and omit that tail. Intermediate restores the three valid partitions but adds exterior segments running toward both marker endpoints.

Sweep vector E=(0, 4.076940246672004, 0); analytic V=[-4.638756436755019, -1.160726592388202]; exact-real stored n·E=0.

| Root parameter | Generator point C | Root interval in Y after target clipping |
| ---: | --- | --- |
| 0.1796040840295541 | (2.844316555831364, 3.673969160105659, -1.151535740482853) | [-15.23796365140969, -1.058243799784251] |
| 0.8673290564548682 | (2.844316555831364, 4.690994520516212, -1.151535740482853) | [-14.22093829099914, -0.0412184393736986] |

True union: **[-15.23796365140969, -0.0412184393736986]**.

Original missing coverage: [-1.058243799784251, -0.0412184393736986]. Original two raw intervals:

- Original segment 1: Y [-15.23796365140969, -1.05824379978425].
- Original segment 2: Y [-15.23796365140969, -1.05824379978425].

| Intermediate segment | Coordinate interval | Verdict |
| ---: | --- | --- |
| 1 | [-0.04121843937369807, 1234564.095198731] | outside sheet: [-0.04121843937369807, 1234564.095198731] |
| 2 | [-1.058243799784252, -0.04121843937369807] | valid partition of true union |
| 3 | [-14.22093829099914, -1.058243799784252] | valid partition of true union |
| 4 | [-15.23796365140969, -14.22093829099914] | valid partition of true union |
| 5 | [-1234569.904801269, -15.23796365140969] | outside sheet: [-1234569.904801269, -15.23796365140969] |

Independent scalar equations (local t∈[0,1] on the listed native knot span):

- Span [0, 0.5]: (-0.1590012988568688)t^0 + (0.4426438845085084)t^1 = 0.
- Span [0.5, 1]: (0.2836425856516396)t^0 + (-0.3860878695373357)t^1 = 0.

## gen1099

The generator is one positive-weight rational cubic Bezier span. Normalize t=(s−2.6732068221646728)/(6.494791779629098−2.6732068221646728). Since n_x=0 and generator z=0, the scalar equation is y(s)=−.8692503877221749. Multiply by the positive weight denominator: the cubic numerator coefficients below result from the homogeneous controls (Bernstein-to-power conversion). Its derivative partitions isolate exactly two roots in t∈[0,1]; the third real root lies outside. Sweep each by .6300223112498824×v in X for |v|≤1234567. Their union includes the second-root high-X tail. Original outputs duplicate the first-root interval and miss that tail; all three restored partitions are valid.

Sweep vector E=(0.6300223112498824, 0, 0); analytic V=[-1234567, 1234567]; exact-real stored n·E=0.

| Root parameter | Generator point C | Root interval in X after target clipping |
| ---: | --- | --- |
| 3.830254303575745 | (1.601203589467039, -0.8692503877221749, 0) | [-777803.1535292441, 777806.3559364231] |
| 5.847014595363178 | (2.663509647870795, -0.8692503877221749, 0) | [-777802.0912231856, 777807.4182424814] |

True union: **[-777803.1535292441, 777807.4182424814]**.

Original missing coverage: [777806.3559364231, 777807.4182424814]. Original two raw intervals:

- Original segment 1: X [-777803.1535292441, 777806.3559364231].
- Original segment 2: X [-777803.153529244, 777806.3559364229].

| Intermediate segment | Coordinate interval | Verdict |
| ---: | --- | --- |
| 1 | [-777803.1535292442, -777802.0912231859] | valid partition of true union |
| 2 | [-777802.0912231859, 777806.3559364232] | valid partition of true union |
| 3 | [777806.3559364232, 777807.4182424814] | valid partition of true union |

Independent scalar equations (local t∈[0,1] on the listed native knot span):

- Span [2.673206822164673, 6.494791779629098]: (-0.1086076050479857)t^0 + (0.5186099591446)t^1 + (-0.5631659482486283)t^2 + (0.1158019138293348)t^3 = 0.

Verdicts: `gen0193` and `gen1099` intermediate coverage is correct; `gen0786`, `gen0906`, and `gen0944` still contain exterior intervals. Every emitted interval must pass the bounded-fiber geometric membership check; the classifier can erase `Nothing` before returning its normalized intervals. Every original row duplicates its first-root interval; four also omit part of the required second-root coverage. These reference intervals follow from the fixture inputs and scalar equations above.
