# surface.offset_surface: two enabled-but-unused extension prefixes (2026-11-08)

Continues #1914 with two supported replacements / 12 moved components. Enabling
extension is not executing an extension. These prefixes never leave the original
direct B-spline domain, and contain no operation that can have created a stored
extension beforehand. This is not evidence for actual extension behavior. All
full original production cases remain executing, with no decoder relaxation,
input change, crop, skip, tolerance/floor change or new compatibility flag.

## State proof and mathematical values

`OffsetSurface::new` starts with `extended_surface=None` and an empty pending
extension (`surface/offset_surface.rs:430-436`). `select_surface` reads the
extension flag only after a point falls outside the natural domain
(`:1304-1348`). Thus an original direct B-spline, with only in-domain point
requests and the audited read-only queries/scalar setters, cannot reach either
the extension or clamping branch. `put_grid` samples only the natural domain;
`put_state` runs that grid on a clone, not a replacement receiver
(`crates/goldens/src/units/surface_offset_surface.rs:836-874`). Copies, type and
knot queries and scalar setters introduce no extended surface. The two accepted
prefixes actually use only `eval` and `state`.

The private proof audits **every** prefix operation and point request, not just
the moved evaluation. Other operations, earlier out-of-domain queries, nested
bases, possible named-slot aliases anywhere in the original script, ambiguous
interior knots, nonregular tangents and normal-substitution/step-off thresholds
remain excluded. It validates full original input identity, output shape and
every value/bit pair against the retained counterfactual, decodes the prefix at
an operation boundary, grades every moved bit, and proves the entire suffix
bit-identical. No suffix-correctness or post-edit claim follows.

Original binary64 inputs are converted exactly. Independent 80/160-digit
Cox-de Boor/Taylor jets differentiate `S + d (Su x Sv)/|Su x Sv|`, without using
kernel jets or a kernel-generated net. All 12 moved components satisfy strict
relative `1e-9` without an absolute floor. Maximum relative discrepancy is
`8.805332234651928e-14`; maximum relative precision drift is
`4.472617148231448e-77`.

- `gen2007`: first **1/3 steps, 32/36 scalars**. The changed equal-order
  upper-half evaluation is at `[0.9822670381287524,0.6137966653036512]` in original
  domain `[0,0]–[1,1]`, distance `0.8570806631900703`. The four-scalar suffix
  contains swap and equality; no post-swap/equality correctness is claimed.
  Suffix SHA-256: `19bc6bb623ffec75dcd46d274dff9c601467c2db29de1ce040b5dae0eca63c9c`.
- `gen2248`: first **2/7 steps, 74/308 scalars**. After the original state query,
  the changed evaluation is at `[-1.3714566043894583,-0.6961094625742988]` in
  original domain `[-1.9135790103933323,-0.8312686221256724]–[1.3829268584332177,-0.2906319839201781]`,
  distance `-27.14016406633157`. Split, trim, copy, reverse and state form the
  **234-scalar ungraded suffix**, all unchanged bits. Suffix SHA-256:
  `16665a339beb0204f724908b09b01bbd38d7dde4273d04406f89552667d69ee8`.

Suffix digests concatenate big-endian u64 binary64 bit patterns in output order.

## Complete original encodings and observations

New rows equal the complete original collector encoding, not a cropped record.
Common fields stay `deferred=null`, `error=null`, `ints={}`, `topology=null`;
`out.n` stays **36 / 308**. For `gen2248`, the complete second checksum block is
unchanged too. Blocks are `[sum, abs_sum, weighted_sum]`, not error bounds.
Input hashes use sorted compact Python JSON, UTF-8, of the full original input.

| Case | Components / max relative error | Original input SHA-256 | Old → new checksum blocks |
|---|---|---|---|
| `gen2007` | 9 / `1.08547e-14` | `40979f91a5c8143e5b0b2e560902302e5bc30d21b034c5bc5e5bee592e922ac6` | `[[8385.129761675184, 10208.861909515392, 240531.36113004576]]` → `[[7433.183654517214, 8963.545179952707, 224246.08082010786]]` |
| `gen2248` | 3 / `8.80533e-14` | `a8b8b0afe101dabe69c50ee63ee7897ba829c8c8873bd9089fde1faf5ce3d839` | `[[156372.75134309038, 252429.00380531867, 19263689.92973941], [40312.70837088957, 63077.20645060704, 274696.5340730896]]` → `[[155970.61055021366, 252143.7086417839, 19234501.757323023], [40312.70837088957, 63077.20645060704, 274696.5340730896]]` |

All moved observations (display rounded; decisions use archived full precision):

| Case / raw index | Partial / axis | Observed | Independent expected | Relative error |
|---|---|---|---|---|
| gen2007 / 11 | (0,3) / x | 5.840236929270969 | 5.8402369292709695 | 2.153e-16 |
| gen2007 / 12 | (0,3) / y | -5.829472830029468 | -5.829472830029531 | 1.085e-14 |
| gen2007 / 13 | (0,3) / z | 4.353237972167417 | 4.3532379721674035 | 2.990e-15 |
| gen2007 / 20 | (1,2) / x | 59.72664999179381 | 59.72664999179384 | 4.863e-16 |
| gen2007 / 21 | (1,2) / y | -30.088202301598827 | -30.088202301598802 | 8.212e-16 |
| gen2007 / 22 | (1,2) / z | 36.106361567067125 | 36.10636156706718 | 1.572e-15 |
| gen2007 / 26 | (2,1) / x | 366.6287166271855 | 366.6287166271859 | 1.137e-15 |
| gen2007 / 27 | (2,1) / y | -58.98860905290834 | -58.988609052908714 | 6.378e-15 |
| gen2007 / 28 | (2,1) / z | 261.43275620586513 | 261.43275620586473 | 1.606e-15 |
| gen2248 / 71 | (1,2) / x | -38.71656614072562 | -38.71656614072553 | 2.136e-15 |
| gen2248 / 72 | (1,2) / y | -3.1621556721435087 | -3.1621556721432302 | 8.805e-14 |
| gen2248 / 73 | (1,2) / z | -17.167331184126198 | -17.167331184125945 | 1.462e-14 |

## Controls, preserved evidence and limits

All **78 numerical/scope controls pass**. Seven new controls establish the
enabled-but-unused flag on an independent affine-plane closed form; reject
out-of-domain evaluations and earlier out-of-domain normal queries; allow only
bit-identical suffixes after every moved field; reject a wrong jet despite an
unused flag; keep the previous stricter model unchanged; reject aliases,
tampered bits and changed full inputs; and bind both actual retained prefixes
before deliberately moving a suffix field. `gen2690` independently disagrees
and remains unapproved. Actual extensions and other unsupported scripts stay red.

The **56-member archive** was read back and hash-verified, SHA-256
`b2b23e145751baaa363d55a718475028b67f24f513de9dce23d9884d297e1064`.
It retains executable models/controls, complete original inputs/raw values/bits,
full-precision observations/disagreements, old/new encodings, prefix/suffix
identities and the inspected production source snapshots. Local path:
`~/.local/share/opencode/solidcore-cleanup-evidence/pr1914-unused-extension-20261108-beb1eb184/`.

Pre-edit head: `beb1eb1843a2e988eb42ab169467494395fc6047`, based on
`3ef79991375bfb6f5cdde7c80584d76589808498`. The collector remains attributed to
its actual measured `7a90407a0fe189ebceb551e5df28aaf4e8cc9893` head, raw SHA-256
`735d018cce5a61c0ffd3479cf6b2f780182779dcfba91d2f8bb89e20583bb958`.
Numerical source SHA-256 stays
`86565c6a5d30bf5e8efb6e3dcbe8b34b4ecb4683d60982a4e214adb525889bef`.
No new collector execution is claimed.

Only these two complete expected rows change; every other row and all 88 known
failures remain unchanged. Projected accounting is **166 supported cases / 812
moved components**, all **3,182 cases = 3,094 golden rows + 88 known failures**,
and **189 ordinary differences**, pending the actual gate. This proves no actual
extension, suffix correctness, B-rep, topology, validity, volume or global
regularity result. Correctness/review holds remain. Parent canonical run
42475855876 stopped at `action_required`, zero jobs, not success; no approval,
rerun, review resolution or readiness is implied.
