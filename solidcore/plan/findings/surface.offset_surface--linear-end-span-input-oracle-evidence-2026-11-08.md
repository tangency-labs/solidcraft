# surface.offset_surface: one polynomial linear-end-span continuation (2026-11-08)

Continues #1914 with one supported complete row / nine moved components,
`gen1320`. No production construction, flag, input, tolerance, absolute floor,
decoder or case selection changes. In particular this does not implement
#2738's extension work or #2717's named-grid change. All three original steps
and all 56 original raw scalars remain selected; no suffix is cropped or skipped.

## State and construction proof

The original direct B-spline has degrees **p=3, q=1**, unit/NOW weights, distance
`0.0002950299289045415`, `allow=true`, and explicit singular-side mask **0**.
Step 0 is `max_anal`, not an edit. Step 1 evaluates `(hu,hv,upper)=(2,3,true)`
at **[4.811712701187294,0.4857657254451476]**. Its u is strictly inside the
original first cubic span `[2.2788605465371647,5.471921079381099]`; v is above
the original domain `[-1.539680032514508,0.0808781086230479]`. The terminal
linear v span starts at `-1.1286307215898375`. Step 2 is a bounding query over
the original domain, not an edit. The complete original script is decoded;
only moved jets, not the correctness of unchanged query outputs, are graded.

Both opposite boundary control hulls are separated by an exact-rational plane
derived from the original binary64 control points. The squared-distance lower
bounds are **3.8310369064621734e-6 / 1.9159016944257274e-6**, both above the
conservative source-threshold upper bound **1.003453397165149e-12**. The archive
retains the exact fractions, plane normals, projection gaps and norm squares.
Clamped polynomial boundaries lie inside their control hulls, so neither
direction can pass a positional closure test. This covers both the receiver's
`Surface::is_periodic` and NURB `is_closed_smooth` G0 checks, before tangent
checks could wrap parameters. Original-net 160-digit first-sample position
checks separately have separation/threshold ratios **4.210233608e6 / 2.872639340e6**,
with regular first samples. No periodic-result flag is used as an expected value.
Sources: `surface/surface/p1.rs:2937-2995`, `nurbs/srf_nurb.rs:1644-1743`.

`select_surface` extends the original receiver for this outside request.
The mask has no clamp bit. `extend_surface` calls the unbounded evaluator with
**G1R**; its extension chunks are at most 0.9 of the current domain length
(`surface/offset_surface.rs:1294-1350,3067-3162`,
`nurbs/srf_adv/p1.rs:240-281,333-375`). This is an actual extension, unlike the
earlier enabled-but-unused or explicit-side-clamp dispositions.

For fixed u, write the original terminal span `[a,b]` as

```text
S(u,v) = A(u) + (v-b) B(u)
B(u)   = (P_last(u)-P_prev(u))/(b-a).
```

The inspected G1R formula, specialized to extension-direction degree one,
creates `Q_last=P_last+(c-b)*(P_last-P_prev)/(b-a)`. There are no intermediate
degree-elevation rows and every weight stays one. Thus the new segment is
**the same affine polynomial**, with the same parameter derivative B(u), not
merely an unspecified G1 continuation. Repeating the construction at any farther
endpoint preserves that polynomial. Its join is exactly C1 in real arithmetic,
so removal of the old boundary knot preserves the same function; retaining the
knot does too. Sources: `nurbs/srf_adv/p3.rs:533-607`,
`nurbs/srf_tool/p2.rs:96-215`. This proof does not generalize to rational nets
or a curved end span in an extended direction.

The oracle continues the **original terminal-span polynomial**, not a
kernel-generated extended net and not a replay of source-order floating-point
arithmetic. Original inputs are converted exactly to Decimal; independent
80/160-digit Cox-de Boor/Taylor algebra differentiates
`S + d (Su x Sv)/|Su x Sv|`. All nine moved components pass unchanged relative
`1e-9`, without a floor; maximum discrepancy **9.324089962188992e-14**, precision
drift **6.182732331065606e-78**. Regularity/normal-substitution guards remain.
No whole-domain regularity or floating-point interval error certificate follows.

## Complete old/new row and observations

The new row equals the complete retained collector encoding. Common fields
stay `deferred=null`, `error=null`, `ints={}`, `topology=null`; `out.n` stays
**56**. Blocks are `[sum, abs_sum, weighted_sum]` checksums, not error bounds.
The hash covers the complete original input, sorted compact Python JSON, UTF-8.

| Case | Components / max relative error | Full original input SHA-256 | Old → new checksum blocks / unchanged n |
|---|---|---|---|
| `gen1320` | 9 / `9.32409e-14` | `01537720249ce9b50614cc1147ba7646e678602cb601657b2468bd837ce52399` | `[[8.083516148205456, 338.29768636282773, 410.2475809158631]]` → `[[8.083506601475959, 338.297647168599, 410.2473283613418]]` / `56` |

All moved observations (display rounded; decisions use archived full precision):

| Raw index / named partial / axis | Observed | Independent expected | Relative error |
|---|---|---|---|
| 16 / D(0,3) / x | -4.0165406990270497e-7 | -4.0165406990270577e-7 | 1.958e-15 |
| 17 / D(0,3) / y | -4.185013264215656e-6 | -4.1850132642156576e-6 | 4.667e-16 |
| 18 / D(0,3) / z | 2.811709755875541e-6 | 2.811709755875539e-6 | 7.824e-16 |
| 25 / D(1,2) / x | -1.1580306332549033e-7 | -1.1580306332549017e-7 | 1.412e-15 |
| 26 / D(1,2) / y | -4.9149666324015915e-6 | -4.914966632401594e-6 | 4.588e-16 |
| 27 / D(1,2) / z | -4.336982137589329e-6 | -4.336982137589327e-6 | 5.264e-16 |
| 31 / D(2,1) / x | -1.8565629605747271e-6 | -1.8565629605749001e-6 | 9.324e-14 |
| 32 / D(2,1) / y | -1.0011266070069925e-5 | -1.0011266070069917e-5 | 7.733e-16 |
| 33 / D(2,1) / z | 1.29596701263065e-5 | 1.2959670126306509e-5 | 6.323e-16 |

## Controls, exclusions and provenance

All **92 numerical/scope controls pass**, including rerunning all earlier controls
after adding explicit opt-in basis/jet evaluator keywords to the temporary
shared helper. Its defaults still reject outside-domain inputs. Seven new
controls cover affine-plane continuation in every direction; independent
nonzero saddle third partials in u/v; rejection of curved/rational extensions,
overlapping/closed hulls and base edits; complete actual-record input/bit
preservation; rejection of wrong jets, corrupted bits and moved query fields.
Only `gen1320` is accepted by the bounded scan; all other exclusions remain
recorded. No unrelated numerical disagreement or extension case is blessed.

The saddle controls use `S=(u,v,uv)`, `d=9/8`, and direct differentiation of
`(-v,-u,1)/sqrt(1+u²+v²)`. At `(1/4,2)` its offset point is `(-3/4,15/8,1)`;
`D(0,3)=(-877608,133056,-532224)/4782969`. The swapped control exchanges x/y
and the differentiation direction. Exact Fraction arithmetic independently
verified these identities. Three initial controls failed on exact equality of
Decimal point components rounded at the 160th digit; they now use the existing
120-digit closed-form comparison. Their exact expected expressions, third
partials, captured-value `1e-9` rule and all scope guards are unchanged. The
initial failure log and separate derivation are preserved, not concealed.

The **76-member executable/raw/source/control archive** was read back and
hash-verified, SHA-256
`1a6491aeb5fc3d00ea479f11d14870038ba28ae31c7537c59d8f8ab8cad00bd2`.
It retains all full original inputs/raw values/bits, exact-rational hull bounds,
old/new rows, full-precision observations, controls, previous strict helper,
derivation, exclusions and source snapshots. Local path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-linear-span-extension-20261108-ceadc86fe/`.

Pre-edit head: `ceadc86fedb8d337812b4882175f4fbb0fd0b79c`, on main
`3ef79991375bfb6f5cdde7c80584d76589808498`. The collector retains its actually
measured `7a90407a0fe189ebceb551e5df28aaf4e8cc9893` revision and raw SHA-256
`735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
Numerical source SHA-256 remains
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
No fresh collector execution is claimed.

Every other golden and all 88 known failures remain unchanged. Projected
accounting is **168 supported cases / 824 components**, **187 ordinary
differences**, all **3,182 cases = 3,094 goldens + 88 known failures**, pending
the actual gate. This derivative-only disposition makes no new B-rep, topology,
validity, volume or body-construction claim. Parent canonical run 42478935902
ended `action_required`, zero jobs, not success. Correctness, review and CI
holds remain; no approval request, rerun or readiness is implied.
