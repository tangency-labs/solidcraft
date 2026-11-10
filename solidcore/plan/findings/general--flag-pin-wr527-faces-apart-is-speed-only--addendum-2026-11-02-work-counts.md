# General: WR-527 addendum: the fork's plate test counts its work, and the port counts it too (2026-11-02)

Updates: plan/findings/general--flag-pin-wr527-faces-apart-is-speed-only.md

The fork no longer times the perforated plate (tangency-labs/smlib#122, edf7414): it counts the two kinds of work that grew with the square of the holes and prints the time without checking it. The port now has the same counts, `Kernel::work_counters` (the fork's `SmWorkCounters`, per kernel instead of process-wide):

- `face_pairs`: each face pair the face loop of `TopologyIntersector::intersect_topology` visits (the fork's `eFacePairs`, 2e9e6b2);
- `loops_classified`: each inner loop `make_edge_in_face_with_vu` classifies after the box test that leaves loops outside the new face's box in place (the fork's `eLoopsClassified`, edf7414).

The fork's test is ported as `crates/solidcore/tests/fork_edf7414_perforated_plate_work.rs`: k x k r 2 holes on a 20 mm pitch, k = 8 and 16, united in balanced rounds and cut from the plate with `boolean_2d`. The counts, on the port (difftest profile, aarch64 macOS):

| Run | Face pairs (64, 256 holes) | Loops classified (64, 256 holes) | Time (64, 256 holes) |
|---|---|---|---|
| `boolean_skips_faces_apart` on (the test) | 64, 256 | 0, 0 | 0.06 s, 0.34 s |
| on, the loop box test bypassed (local probe, not committed) | 64, 256 | 4032, 65280 | 0.10 s, 0.95 s |
| off (upstream) | 2080, 32896 | 4032, 65280 | 0.35 s, 5.06 s |

The counts in the first two rows are the fork's. The area is the same in every run, within 1e-10 relative of the exact area. So the flag is now pinned: with it forced off the test fails on the face pair count.
