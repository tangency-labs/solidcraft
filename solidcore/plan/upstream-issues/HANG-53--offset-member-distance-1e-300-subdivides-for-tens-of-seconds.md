#### HANG-53. An offset by a distance far below the B-rep tolerance subdivides its offset surfaces for tens of seconds
- **Severity:** Hang
- **Area:** Offsets and shells
- **Location:** `source/SMLib/src/SmOffsetExecutive.cpp`, `SmOffsetExecutive::DoExtendedOffset` -> `BuildOffsetOfEdgeuses` -> `EdgeuseOffset`, in `SubdivideToTolerances` of the edge-use offset surfaces' caches.
- **What happens:** `DoSolidOffset` (extending convex edges) by a member distance of `1e-300` runs for about 21 s before it returns. Nothing refuses a distance the B-rep cannot resolve (below its tolerance); the surface caches are subdivided toward it instead.
- **Trigger:** `offset.offset_executive_p2` nightly fuzz `fuzz00175`: base `mate_two_curves_problemShell15_face8_outset` (`Shell/problemShell15.smb`, face 8, B-rep tolerance `5.64e-4`) with `/config/member_distance = 1e-300`.
- **How found:** generated difftest case (nightly fuzz, run 41438521478).
- **Confirmed:** yes, `fuzz00175` of `offset.offset_executive_p2` (C++ 20.9 s on x86-64, past the fuzz's 20 s case limit on some machines; `plan/findings/offset.offset_executive_p2--fuzz00175-member-distance-1e-300-slow-on-both-sides.md`).
- **solidcore:** the static, solid and extended offsets refuse a distance that is not a finite length of at least the input's B-rep tolerance (`OffsetFailure::DistanceUnresolved`, kind `InvalidInput`), and the difftest shim refuses the same inputs before the C++ runs them.
