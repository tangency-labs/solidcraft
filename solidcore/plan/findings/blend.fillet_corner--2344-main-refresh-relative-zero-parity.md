# blend.fillet_corner: strict executive coordinate parity after refresh (2026-11-03)

PR: https://github.com/tangency-labs/solidcore/pull/2382
Merged library code: `403d8d30b71d5e9ad7d97396752c44b5f5f45de6`, including
main `ebe6722e0` and #2369 `ee6e1b349`. Native: allowed SMLib 20b1c01
setback overlay, built using the checked-in native executive driver.

Reproduction: follow `docs/setback-scale-search-validation.md`, then run
`compare_setback_executive.py` on the thirteen-case Rust/native dumps.
The full strict comparison fails; do not report its historical pass at
`274e0a4a6` as a pass for this merged code.

All seven built bodies' volumes and topology counts agree at 1e-9 relative;
statuses, refusal comments and named fits agree. Coordinate matching fails
under the comparator's relative-only rule (no absolute floor):

| Case (zero-based) | Input | Example Rust coordinate | Corresponding native coordinate |
| --- | --- | --- | --- |
| 9 | 20 mm box, r 2/3/4, requested setback 4.7175 | `(0,20,3)` | `(4.440892098500626e-16,20,3.0000000000000187)` |
| 10 | 20/20/5.5 box, r 2/3/4, room-limited | `(0,20,3)` | `(4.440892098500626e-16,20,3.0000000000000093)` |
| 12 | 20 mm box, r 2/2/3 at (20,0,20) | `(16.4195,0,18)` | `(16.419499999999985,1.3322676295501878e-15,18.00000000000009)` |

Cases 9 and 10 each have four unmatched vertices, case 12 has two; the
differences are in coordinates near zero. Example volumes (Rust/native):
7879.300574035238 / 7879.300574035242;
2128.974917615450 / 2128.974917615452;
7927.724282512233 / 7927.724282512234.

Suspected cause: changed exact-geometry/trimming evaluation or operation order
in the intervening main/dependency changes. The precise origin is unconfirmed;
this record does not assert a C++ defect. Rust's own Level2 BREP checks pass.
Stored and streamed native references agree with each other on all thirteen
cases, seven bodies and 167 vertices under the same strict comparator.

No tolerance, absolute floor, coordinate snapping, skipped case or parked
mismatch was introduced. Resolving or explaining the new coordinate differences
remains a blocker to claiming full executive parity or marking #2382 ready.
