# query.face_p4: two scale-1e-3 cylinder cases move with the importer's curve approximation (2026-10-24)

The runner imports the C++ B-rep. The edge curves `MakeFaceWithCurves` and
`CreateFaceInRegionFromSurface` give analytic faces are only approximated by the importer
(unit `topology.face_p2`'s `Exact`). Almost every case the loop bisector now reaches has
such an edge. The classification is integer-valued and agrees on all of them except two,
so they stay compared, and these two are known mismatches:

- `gen0300`: a rectangle and two bands on an `SmCylinder` at scale `1e-3`, loops
  reversed. The bisector's UV trim line starts at
  `(0.010376246333575003, -0.010016146303418206)` in C++ and at
  `(0.010376246333575003, -0.010016146463519855)` in the port (relative 1.6e-8, from the
  edge midpoint dropped to the surface). C++'s classification of that line has 3
  intervals (out / in / out, touches at 0.2097 and 0.2229), the port's has 2 (the first
  interval unknown). The rectangle then becomes a no-area loop in the port
  (`classify_loop_ios[2]` cpp 2 rust 1, `[4]` cpp 1 rust 0, `[5]` cpp 1 rust 5,
  `ordered_no_area.len` cpp 0 rust 1, `scratch_size` cpp 3 rust 2).
- `gen0157`: the natural face of an `SmCylinder` at scale `1e-3`, swapped:
  `classify_loop_ios[4]` cpp 1 rust 0, `[5]` cpp 1 rust 5, `ordered_no_area.len` cpp 0
  rust 1. Same pattern.

I found no input-only exactness rule that separates these two: excluding every case where
the bisector reaches an inexact edge also drops about 85 cases that agree. The fix
belongs in the importer (exact analytic edge curves).
