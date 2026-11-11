# surface.offset_surface: one enabled-extension explicit-side clamp (2026-11-08)

Continues #1914 with one independently supported complete row / three moved
components, `gen1332`. The original script keeps extension enabled, but its
explicit `SS_VMAX` flag clamps the only outside request before extension runs.
This is not evidence for an actual extended surface, a changed default flag,
or a constant continuation's derivatives. All original inputs and all three
script steps remain executing.

## Independent state and value evidence

The original direct B-spline has domain `[0,0]–[1,1]`, distance
`8.716126750113276`, `allow=true` and explicit side mask **11 = 1|2|8**. At step
0, `(hu,hv,upper)=(0,3,false)`, the requested UV
`[0.004833398684368828,1.9535512732399394]` is clamped to
`[0.004833398684368828,1.0]` by its original `SS_VMAX=8` flag. The later normal
query is in the original domain; the final state grid uses that domain too.
No edit occurs anywhere in the full script. `OffsetSurface::new` starts without
an extension; `extend_surface` wraps periodic directions, applies the explicit
side flags, then returns `None` when the resulting UV is in domain
(`surface/offset_surface.rs:430-436,3088-3113`).

The original input net independently establishes nonperiodicity, rather than
assuming it from a result flag. The first opposite-boundary position test of
`Surface::is_periodic` is evaluated at 80/160 digits, from original knots/weights
and control points, with the same binary64 `1/7` sample fraction. Its squared
separation exceeds the source's position threshold
`1e-12*(1+max(abs(Pmax)))` by **6.632468755324e12** in u and
**7.813412246639e12** in v. Relative precision drift of those separations is
`1.292e-79` / `1.421e-81`; both endpoint samples pass the independent regularity
checks. The private proof admits only ratios above `1e6`, a conservative scope
filter, not a production tolerance or new absolute floor. These are explicit
local numerical/state diagnostics, not a whole-domain interval certificate.
Source: `surface/surface/p1.rs:2937-2995`, `geom/tol.rs:869-873`, `math.rs:44`.

The independent Cox-de Boor/Taylor algebra differentiates
`S + d (Su x Sv)/|Su x Sv|` **at the clamped point**, converting original
binary64 inputs exactly and constructing no net from kernel derivatives. All
three moved components pass unchanged relative `1e-9`, without a floor; maximum
discrepancy `8.063531413806036e-14`, precision drift `1.8686238951724364e-77`.
The entire original input and every output bit/shape were checked against the
retained counterfactual. It selects moved indices only, never expected values.

| Raw index / component | Observed | Independent expected (rounded display) | Relative error |
|---|---|---|---|
| 11 / D(0,3).x | 67.24942617590006 | 67.24942617589463 | 8.064e-14 |
| 12 / D(0,3).y | -213.5282907546842 | -213.52829075468333 | 4.083e-15 |
| 13 / D(0,3).z | -200.89857095976015 | -200.89857095975854 | 8.035e-15 |

## Complete old/new encoding

The replacement is the full retained collector encoding. Common fields stay
`deferred=null`, `error=null`, `ints={}`, `topology=null`; `out.n` stays **72**.
Blocks are `[sum, abs_sum, weighted_sum]`, not error bounds. The full original
input hash uses sorted compact Python JSON, UTF-8.

| Case | Components / max relative error | Full original input SHA-256 | Old → new checksum blocks / unchanged n |
|---|---|---|---|
| `gen1332` | 3 / `8.06353e-14` | `cb46fb52544fb4c84e3a64deca2346d55fc1ad7f94adec553f4e41652db65660` | `[[68487.9127286635, 73586.6520814705, 1550175.2367883257]]` → `[[30395.004642320324, 31489.622915038668, 1034470.1990414166]]` / `72` |

## Controls, exclusions and provenance

All **85 numerical/scope controls pass**. Seven new controls exercise each side
and combined masks on an independent affine-plane closed form; reject missing
flags and unknown masks; check that plane's opposite-boundary squared distances
are exactly 5 and 10; reject a closed/unseparated or nonregular first sample;
reject a wrong jet and preserve the previous model's extension-disabled guard;
exclude a parameter edit; and bind the actual complete input/output before
deliberately corrupting a bit.

The numeric-only candidates `gen0590` and `gen0962` are **not approved**.
`gen0590` reaches a normal fallback in its first periodicity sample, outside this
state proof. `gen0962` swaps parameters, but `swap_uv_notifying` does not remap
the stored side mask (`offset_surface.rs:1886-1904`); a clamp-only model silently
remapping that mask is not justified even though three components agree. No
actual extension or unmodelled branch is excused by numeric agreement alone.

The **64-member archive** was read back and hash-verified, SHA-256
`c0277e8e83feb5100d1c40e5f947f3aa476e0073555c19acd9fcdf403086ec81`.
It contains executable models/controls, source snapshots, complete original
inputs/raw values/bits, old/new rows, full-precision values and nonperiodicity
diagnostics. Local path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-singular-side-clamp-20261108-670557efa/`.

Pre-edit head: `670557efa12f074f96d7fc559a1d854f7f722aca`. The collector remains
at its actually measured `7a90407a0fe189ebceb551e5df28aaf4e8cc9893` revision,
raw SHA-256 `735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
Numerical source SHA-256 stays
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
No new collector or full-suite execution is claimed.

Every other golden and all 88 known failures remain unchanged. Projected
accounting is **167 supported cases / 815 components**, **188 ordinary
differences**, all **3,182 cases = 3,094 goldens + 88 known failures**, pending
the actual gate. There is no new body/topology, validity, volume, global
regularity or actual-extension claim. Parent canonical run 42477082489 ended
`action_required`, zero jobs, not success; correctness/review holds remain.
