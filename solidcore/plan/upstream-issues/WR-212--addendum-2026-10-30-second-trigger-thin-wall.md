Updates: WR-212

#### Updates: WR-212 (2026-10-30). A second trigger: a round through a thin wall

- **Trigger:** `SmApiFilletEdges`, constant radius 18, on the two outside top edges of an
  upside-down U: a 60x20x50 block minus a 54x22x48 cavity that starts at z=-1, so the top
  wall is 3 mm thick (z=47..50). The edges are at (0,10,50) and (60,10,50). This is
  SolidCraft #2852, fixture 2852 (tangency-labs/solidcore#683), taken one step earlier
  than the fixture: without the R15 inside round first. R18 is larger than the 3 mm wall
  plus the free corner.
- **Observed with the Rust port, default kernel:** success; 1 lump; volume 9240, equal to
  the unfilleted U; F/E/V 12/40/32 (Euler 4, a non-manifold shell). `OrientTrimmedSurfaces`
  sets `bMaybeNotSolid` to true on the final pass.
- **Cause:** the same as above. `SmFilletExecutive.cpp:3678-3679` ignores
  `bMaybeNotSolid`, and `:3698-3711` only does `SM_DBG_WARN` for edges that are not
  manifold. `SmApiFilletEdges` (`SM_API/src/SmApiBrep.cpp:2049-2087`) has no check of the
  radius against the wall thickness. The solver's "radius too large" refusal
  (`SmFilletSolver.cpp:2096-2103`) does not fire, because the offset surfaces still
  intersect.
- **Rust port:** it matches the C++ by default. With `fixes.fillet_refuse_non_manifold_result`
  on, `fillet_edges` refuses it with status 1001 (verified).
- **Confirmed:** no. The behaviour of the C++ reference is read from the source above, not
  run.
