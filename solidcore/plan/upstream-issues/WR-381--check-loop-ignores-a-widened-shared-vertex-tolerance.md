#### WR-381. CheckLoop ignores a widened shared vertex's tolerance
- **Severity:** Wrong result
- **Area:** Topology
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:3150`, `SmTrimmingTools::CheckLoop`.
- **What happens / Trigger / Confirmed:** see below (issue #476, PR #691).


The 3D endpoint-gap test and the later non-seam UV-gap certificate use only
`GetXSectTol3d(edge1, edge2)`. The source's own comment acknowledges that this
can reject two edge ends attached to a consistent widened vertex.

Confirmed against the pinned C++ library: on public `plane_square.smb`, change
one UV trim's starting U coordinate by `1.3e-5`, set both edge zones to `5.8e-6`
and vertex zones to `2.6e-5`. For loop edge uses 0, 1 and 3 the measured gap is
`1.3000000000040757e-5`, but native CheckLoop returns success with closure 3 and
one failed gap report at `1.16e-5`. The Rust fix uses the greater of the existing
edge intersection limit and the **shared** vertex zone, accepting this gap.
An outside gap `2.6100000000361945e-5` remains closure 3, with a corrected report
limit `2.6e-5`. Twelve native comparison differences (three accepted-gap cases and nine
diagnostic-limit differences, including six existing mirrored Mouse cases) remain explicit known mismatches; the unit is partial.
No C++ CheckLoop change, comparison-tolerance change or case removal is used.
WR-255 is reserved by the earlier independent PR #684.
