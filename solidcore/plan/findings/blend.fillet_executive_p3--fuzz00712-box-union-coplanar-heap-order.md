# blend.fillet_executive_p3: fuzz00712, a box union with coplanar faces, varies with the C++ heap (2026-10-27)

**Unit:** `blend.fillet_executive_p3` (end-to-end `real_solid` cases: `api::fillet_edges` /
`FilletSession` on the Rust side, `SmApiFilletEdges` in a forked child on the C++ side).
The cause is in the boolean union that builds the source body, not in the fillet. It is
upstream nondeterminism, recorded as
`plan/upstream-issues/ND-258--addendum-2026-10-27-coincident-plane-survivor.md`.
**Not a port bug:** the Rust output is the same at every heap layout, and it is always one
of the C++ outcomes.

## Inputs

`--fuzz 1000` mutated `stubs3-union-inset-side-step-edges13-17-r1` into:

```json
{"real_solid": {"source": {"kind": "box_union",
  "a": {"origin": [-1, 0, 0], "size": [20, 10, 10]},
  "b": {"origin": [-1, 2, 5], "size": [10, 10, 10]}},
  "radius": 1.0, "xsect": 1, "continuity": 1, "thumbweight": 1, "edges": [13]}}
```

Both boxes start at x = -1, so their x = -1 faces are coplanar and overlap. The union has
one L-shaped x = -1 face (y 0..12, z 0..15). The base fixture (A at the origin, B at
(5,2,5)) has no coincident faces and is not affected.

## Values

`result_geometry[800..804]` is that face's record in `primitive_record::append`
(tolerance, then its UV domain min.u, min.v, max.u, max.v). Its three surface samples
agree, so only the parametrisation differs:

| | domain |
|---|---|
| Rust, and C++ with A's plane | (-0.75, -0.75) - (15.75, 12.75) |
| C++ with B's plane | (-5.75, -2.75) - (10.75, 10.75), i.e. shifted by B - A in (z, y) = (5, 2) |

## Heap-layout evidence

The C++ runs in a forked child (`solidcore_ffi::isolate::fresh_process`), which inherits
the Rust parent's heap, and C++ allocates from the same `malloc`. So the C++ result can
change with the cases run before it in a batch, with `--filter`, and with unrelated
changes to the binary. Run alone, `fuzz00712` failed twice at the PR #1298 build and
passed 8 times out of 8 after a debug print was added.

In a temporary (uncommitted) build, the child allocated `N` dummy blocks
(`vec![1u8; 24 + 40 * (i % 7)]`, freed after the call) before the C++ call:

| case | N giving Rust's result | N giving another C++ result |
|---|---|---|
| e13 (fuzz00712) | 0, 8 | 1, 2, 3, 5, 13, 21, 34, 55, 89, 144 |
| e00 | 4 | 0, 1, 2, 3, 5, 6, 7, 9, 10, 11, 12, 13, 16, 21, 34 (two distinct C++ results) |
| e02 | 0 | 1, 8 |
| e04 | 1, 8 | 0 |

The Rust output did not change with `N`.

## Scope: single-edge fillets of the same union

Each of the union's 24 edges filleted on its own (r = 1), in one batch at the default
layout (`--filter fuzz00712-e`): 12 passed, 12 failed. These are one heap layout's
results: other batches move cases between columns (e13 passed in the batch, e12, e15,
e16 and e18 failed in the `e1` batch).

| outcome | edges |
|---|---|
| pass | 2, 6, 8, 9, 10, 13, 14, 15, 16, 17, 18, 20 |
| geometry only (one face's domain, 20 mismatched values) | 0, 1, 3, 5, 11, 12, 19, 21, 22, 23 |
| **topology differs** (`result_topology[395..]`, 16 mismatches) | 4, 7 |

The two topology differences come after a different surviving plane: the fillet then
splits and trims the faces differently. How to recognise the pattern in other fuzz or
fixture cases: a `box_union` (or any boolean source) whose operands share a coplanar
face, a C++ value that flips between runs or batches, and face domains offset by an
operand-origin difference.

## Status and follow-ups

- Existing fixtures: 44 `real_solid` cases in `corner_3x2.json`, `overhang_union.json`,
  `piecewise_merge.json`, `solid_sources.json` and `stubs_round3.json` union boxes whose
  z = 10 faces are coplanar and overlap. The whole unit (592 cases) was run under the same
  heap-pad build at N = 0, 1, 2, 3, 5 and 8: every case gave the same outcome at every N
  (586 passed, 0 failed, the same 4 known mismatches). So those fixtures did not move
  under this perturbation. That is evidence, not proof, that they are stable: only this
  one allocation pattern was tried, and why their survivor choice is not exposed was not
  investigated.
- No fixture or known-mismatch entry was added. A `cpp_defect` known mismatch is for a
  C++ that is verified wrong, which this is not. A fixture like fuzz00712 would be flaky
  in CI, because which side matches depends on the heap.
- Proposed (separate PR): an opt-in difftest flag that reruns each C++ case in the child
  at two or three heap pads and flags a case whose C++ output changes as
  address-dependent, so fuzzing and fixture authors can separate this from a real
  divergence. Default runs unchanged.
- Found in PR #1298's gate; recorded in PR #1336.
