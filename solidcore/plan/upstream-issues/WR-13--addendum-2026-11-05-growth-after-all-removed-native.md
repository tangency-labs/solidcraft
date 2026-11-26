Updates: WR-13

#### WR-13 (addendum 2026-11-05) — Measured `growth_after_all_removed` case after default preservation

- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `source/SMLib/inc/SmHashTable.h:804-879`, `SmHashTable<TYPE>::IncMaxItemCount`.
- **Confirmed:** yes, case `growth_after_all_removed` of `geom.hash_table`, locally reproduced against pinned C++ v1.0.1 / `3fd5b95` with reference key `v1-3fd5b9548d19-726643fb17a52b7d` on Linux x86-64.

The existing fixture (`fixtures/geom.hash_table/cases.json#14`) adds items 0
through 57 into one voxel, removes all 58, dumps the model, then adds items 100
and 101 before further dumps and removals. At the first new addition, 58
historical point slots plus the seven-slot allowance exceed capacity 64.
Growth takes the zero-used-item branch and loses the removed bucket records,
as the original WR-13 record describes.

The first mismatch, `ints.ints[490]`, is the first historical bucket record's
bucket index in the second dump: C++ returns 0; Rust returns 123456789, the
runner's encoding of `SM_UNDEF_ULONG` / a removed record. Its item reference at
`[491]` is C++ -1 (null) versus Rust 0. Both dump implementations emit each
record as next-used link, next-bucket link, bucket index, point-item reference,
and item ID.

A separate-process state check runs each implementation from the same fixture,
without passing C++ output or process state to Rust. Both report 60 total
records and 58 removed records. C++ nevertheless marks all 60 records active;
Rust marks two, consistent with the counts. The two 1238-element integer outputs
differ at 348 coordinates. All 712 floating outputs are finite and equal.
This establishes the reference's invalid record state, beyond a disagreement
with Rust. The preservation fix itself landed in #2763 and is unchanged here.

Main's exact-base CI run [42014925714](https://github.com/tangency-labs/solidcore/actions/runs/42014925714)
on `b5569dae55a545b94d7ef3ca010da0e3a12836aa` published the same case in
`difftest-sweep-reports` (artifact 13409122641), with inputs identical to the
current fixture. The local unfiltered unit run reproduces its counts: 621
cases, 620 passing, this one failure, no deferrals, maximum floating relative
error zero. The named case has 348 mismatches.

The case is listed with `cpp_defect = "WR-13"`. It remains in the case inventory
and is parked rather than counted as a pass. `--run-parked` runs the comparison
again. No fixture, generator, input guard, tolerance, absolute floor, library
behavior, or unit status changes accompany the marker.
