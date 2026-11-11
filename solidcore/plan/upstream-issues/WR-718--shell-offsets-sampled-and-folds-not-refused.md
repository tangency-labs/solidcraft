#### WR-718. A shell's B-spline offsets are sampled fits, not certified, and an offset that folds is built or crashes instead of being refused
- **Severity:** Wrong result (a folded body; a crash on a variant)
- **Area:** Offset and shell (offset executive)
- **Location:** `source/SMLib/src/SmOffsetExecutive.cpp:2015`, `SmOffsetExecutive::ReplaceImplicitOffsets` (the `ApproximateOffsetSurface` call at `:2052`, NLib `N_SrfOffset`); `source/SMLib/src/SmOffsetGeometryCreation.cpp:118`, `SmOffsetGeometryCreation::FaceuseOffset` (no regularity test on the paths the shell takes: `DoExtendedOffset` asks it to skip the self-intersection check).
- **What happens:**
  - Every implicit offset of a B-spline face is replaced, after the trims were computed on it, by `N_SrfOffset`'s fit (Piegl & Tiller 1999): exact points sampled, interpolated and knot-removed, its error checked at samples only. Nothing bounds the fit between them (Elber, Lee & Kim 1997). On the issue's parabolic sheet thickened 2 towards its centre the solid comes out 2.1e-8 off the tube formula's closed form.
  - An offset that folds (the wall thicker than a radius of curvature on its side, `(1 - k1 d)(1 - k2 d) < 0` somewhere on the face) is not detected. The same sheet thickened 11 towards its centre (the apex's radius is 10):
    - one span: the pinned C++ returns `SM_SUCCESS` with a folded body (6 faces, 14 edges, 10 vertices where the sound thickens have 12 and 8);
    - two spans (a knot inserted at s = 1/2, the same curve): the pinned C++ crashes (the difftest FFI's child process dies, `-1100`);
    - the Rust port fails on both with `proto_topology::p3: invalid input: proto edge is deleted` (a separate port divergence for the one-span case, tracked as its own follow-up).
- **Trigger:** `SmApiShellBrepFull` / `ShellBrep` (shell, or a sheet thickened with `bCreateOffsetSolid`) of a face on a B-spline surface: any such face for the sampled fit; a wall past the face's smallest radius of curvature on its side for the fold.
- **How found:** tangency-labs/solidcore#2350 §3.1's `cov-thicken-parabolic-sheet` rows, run on solidcore main and on the pinned C++ through the `offset.offset_executive_p2` FFI (`StaticShell`, extend and merge on, `bCreateOffsetSolid`, the sheet written as an ASCII `.smb`: the quadratic Bezier (-10, 0), (0, 10), (10, 0) in xy extruded 20 along z, u along the curve, v along z, non-rational, distances 2, -2, 5, 11, -11).
- **Confirmed:** yes, the pinned C++ (3fd5b95) as above.
- **Rust fix (opt-in):** `UpstreamFixes::shell_one_construction`, the first stage (S1) of #2350's one construction for shell, offset and thicken. Solidcore-native: the design was written for SMLib and moved to solidcore; the fork has no code of it (checked at replay/1.0.1 18e96f1).
  - `ReplaceImplicitOffsets` takes a bicubic B-spline certified within the shell's tolerance of the exact offset `S + d n` at every parameter, in the base's parametrisation (`crate::offset::construction::sheets::certified_offset`). The bound is Elber & Cohen's (1991): `|e|`, `|tau|` and the side of `G - S` bounded by the coefficients of exact Bernstein products on every patch, with `|G - O| <= |tau| + (|e| + |tau|^2) / |d|`; knots are inserted where it is not met.
  - A B-spline face whose offset is not certified regular on its parameter box is refused with `SolidError::ShellRefused` (`ShellRefusal::Fold`, status `SM_ERR`), naming the largest wall that works: the smallest radius of curvature on the displaced side (D5).
  - Offset faces and draft (`LocalOperation`) are not affected. Off: SMLib's behaviour.

  Tests: `crates/solidcore/tests/issue_2350_shell_offset_construction.rs`:
  - the parabola thickened 2 both ways and 5 towards the centre, one and two spans: six faces, Level-2 check clean, the offset face within the shell's tolerance (1e-5) of distance t from the sheet at every sampled parameter (2.7e-6 at t2), the volume the tube formula's within the tolerance times the boundary area. Measured at t2: the offset face's area 6.7e-9 off the closed form (the flag off: 1.4e-8), the volume 2.4e-8, all from the end caps, which are ruled to the offset edges the executive fits (their exact step walls are S2's);
  - t11 towards the centre refused naming 10, one and two spans; the flag off: not refused (the port's error);
  - the cup, closed sphere and closed torus t1 of §3.1 keep their closed forms within 1e-8 with every opt-in fix on.

  And the unit tests of `crates/solidcore/src/offset/construction/sheets.rs`: certificates checked against 10^4 fresh points, a rational quarter cylinder (the exact offset is the cylinder of radius 10 + d), fold radii (the parabola's 10, the cylinder's 10).
