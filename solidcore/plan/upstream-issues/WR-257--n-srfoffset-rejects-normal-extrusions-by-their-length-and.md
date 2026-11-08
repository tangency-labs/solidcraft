#### WR-257. `N_SrfOffset` rejects normal extrusions by their length and destabilizes fits on sliver spans (#471)
- **Severity:** Wrong result
- **Area:**

- **Location:** `source/SMLib/src/NL_SrfOffset.cpp:854-892, 956-1108`.
- **Trigger:** a centered 120x80 rectangle, taper-extruded 40 mm with 5-degree draft,
  four sloping edges filleted r4, then shelled inward by 2 mm with the top removed.
  `fixtures/nlib.srf_offset/issue471-repro.cpp` records the actual offset inputs via
  a linker wrapper around the unmodified `N_SrfOffset`; `issue471.json` stores the
  four round surfaces.
- **What happens:** the last round has u knots 20.152295485472926 and
  20.152507010543033. `N_SrfOffset` succeeds but its net reaches approximately
  4125.906 mm from the origin; the full shell returns 1001 on this build. The
  extrusion test uses the length-bearing `AA`: `|AA x NN| = 2.1064e-6`, while
  normalizing AA gives 5.4909e-8, below `NL_MTOL = 1e-7`. The fallback assigns
  shared u/v patch counts, and its grid cap raises even previously skipped sliver
  counts to the input degree, clustering samples around nearly coincident knots.
- **Confirmed:** yes, pinned `c9c5979`, x86-64 Linux, unmodified C++ reference.
  The Rust port reproduces the defective control net. See the #471 findings entry
  for the opt-in correction and its validation limits.
