# surface.offset_surface: two query-prefix named-slot failures (2026-11-08)

Continues #1914's evidence-backed dispositions. `gen0757` and `gen2243` each
return three independently wrong named `D(2,0)` components in the #2717 family.
Their old goldens are **removed, not replaced by wrong values**; both full cases
remain executing through new case-specific #2717 known-failure entries. This
does not implement the requested-grid change or choose refusal versus masking.

## Prefixes, state and independent expected values

Both prefixes start from a nonzero offset of the original direct B-spline.
Extension is disabled. The first operation is an independently decoded query
or returned copy; subsequent prefix operations are evaluations. The default
evidence decoder is not relaxed to accept aliases: only the new private proof
walks the aliased requests, using the inspected ordered write audit.

- `gen0757`: first two of five steps, 66 of 108 raw scalars. Step 0's
  `base_offset(d=1.0804625073576302, tol=0.001)` returns a non-null offset kind
  `15400`; it **does not replace** the receiver. The receiver distance remains
  `0.7822430607713939`. Step 1 asks `(hu,hv,upper)=(2,0,true)` at
  `[-5.856672193765806,2.801506594459069]`. With extension disabled,
  `OffsetSurface::select_surface` explicitly clamps u to `-2.73069809326579`;
  v remains `2.801506594459069`. The expected value is the named jet **at that
  clamped point**, not the derivative of a constant continuation outside it.
- `gen2243`: first three of six steps, 63 of 137 scalars. Step 0 is a bounding
  query, not an edit. The wrong `D(2,0)` occurs at step 2's `(2,0,true)` request,
  at in-domain UV `[-1.858474282224452,3.331215267820798]`, before step 3's swap.
  The receiver distance is `0.8689135148110283`. No post-swap value is graded.

For `(2,0,true)`, `D(2,0)` is stored at `2*(hv+1)=2`, then the later `D(1,1)`
branch writes `(hv+1)+1=2` into the same slot. Serialization still names it
`D(2,0)`. Independent 80/160-digit Cox-de Boor/Taylor algebra evaluates
`S + d (Su x Sv)/|Su x Sv|` from original homogeneous control points, weights,
knots and distance. No kernel derivatives or query/copy results supply a
mathematical expected value. Ambiguous interior knots, nonregular tangents,
normal substitution and unsupported orders are excluded by the same checks as
the earlier input-only oracle.

All six named components disagree by far more than the unchanged `1e-9`
relative rule, without an absolute floor. Precision drift is at most
`3.9304004881181306e-77`. The ordered-write defect and independent wrong named
value are both established; **the overwriting expression is not certified as
a correct `D(1,1)`**. The suffix is not graded and is not asserted correct.

| Case | Step / raw index | Axis | Observed | Independent expected (rounded display) | Observed binary64 bits | Relative error |
|---|---|---|---|---|---|---|
| gen0757 | 1 / 63 | x | -2.1616383101165546 | 0.2822558764347538 | `13835422032398768568` | 1.1305749787620736 |
| gen0757 | 1 / 64 | y | -9.08591818283331 | 0.12107242226112541 | `13844676572127350072` | 1.0133252820270687 |
| gen0757 | 1 / 65 | z | -4.69820136261125 | -0.2167761814816701 | `13840347759758655608` | 0.9538597508385238 |
| gen2243 | 2 / 60 | x | 0.22943334526922846 | -0.1462000843762228 | `4597434227716622333` | 1.6372224761168189 |
| gen2243 | 2 / 61 | y | 0.24036374908130256 | -0.14694417604158103 | `4597828037016902861` | 1.611340839054218 |
| gen2243 | 2 / 62 | z | 0.6333616623683682 | 0.05521866007976685 | `4603880034238542298` | 0.9128165417002281 |

## Removed encodings, controls and provenance

These are `[sum, abs_sum, weighted_sum]` checksum blocks of the old complete
goldens, not bounds or replacement values. Common removed-row fields are
`deferred=null`, `error=null`, `ints={}`, `topology=null`. Input hashes are of
full original inputs using sorted compact Python JSON, UTF-8.

| Case | Full original input SHA-256 | Prefix steps / scalars | Removed old golden block / full n |
|---|---|---|---|
| `gen0757` | `a7fff71f532ef0b76a0f9aace5c2d210fa393ff426cb68f470377a44d426a08b` | `2/5 steps, 66/108 scalars` | `[[45835.32261187165, 45951.766035862245, 580801.8107888201]]` / `108` |
| `gen2243` | `b5c1b0a415107215e7fdcd7fc301a26bf021c40add31c08ae54074e3f9e07f4d` | `3/6 steps, 63/137 scalars` | `[[30474.99107836607, 30522.408270029144, 3002299.3746962883]]` / `137` |

All **71 numerical/scope controls pass**. Five new controls bind the actual
prefixes without changing original input/bit records, restore each independently
expected named value and eliminate its wrong-value finding, reject extension,
zero-distance bypasses, bit tampering and an unmodelled edit, exclude failed
evaluations, require the returned offset's unambiguous kind, and show that a
non-aliasing request is not proof of a wrong output. A test initially expected
the wrong error string for the kind rejection; it was corrected to assert the
existing stronger direct-B-spline guard, without changing that guard.

The other three potential-alias candidates stay unapproved: `gen1864` and
`gen2081` are not supported canonical B-spline generators, and `gen2743`'s alias
is after an unmodelled transform. Request shape alone never transfers a case
into the failure registry. No other golden or known-failure entry changes.

The 49-member executable/raw/control archive was read back and hash-verified,
SHA-256 `9169e48e605bddc010862db3b610c420e54372dba1ce9ad46ce73c5b35fccfb7`.
It includes full original inputs/raw values/bits, exact old rows, both decoded
prefixes, full-precision expected values and bounds, the models and all controls.
Local path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-clamped-alias-prefix-20261108-0d6fda79b/`.

The collector remains attributed to measured head
`7a90407a0fe189ebceb551e5df28aaf4e8cc9893`, stream SHA-256
`735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
The pre-edit head is `0d6fda79b520fb6d2d8d6a2cde89f6f51e74852b`, based on
`3ef79991375bfb6f5cdde7c80584d76589808498`. Numerical source SHA-256 remains
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
No fresh collector execution is claimed.

Projected accounting is **164 supported replacements / 800 supported moved
components**, **3,094 golden rows + 88 known failures = all 3,182 cases**, with
**191 ordinary differences**, pending the real gate. These known failures are
not successful deliveries. No production case is cropped or skipped, no
tolerance/floor or geometry source changes, and no body-construction, topology,
validity, volume or global-regularity claim follows. The correctness, historical
attribution-review and canonical-CI holds remain.
