# nlib.srf_adv_p1 (NL_SrfAdv.cpp part 1: extension by distance, offset bounds, unbounded evaluation, spine skinning, vector-field strips)

No Rust-vs-C++ disagreement: 2886 of 2886 default difftest cases bit-identical (386
fixtures on the three `pt_TestFiles` `.sms` surfaces, the `TestSmCreateRectangle` patch, a
closed rational cylinder, the `.smc` curves and the `TestSmCreateRegularPolygon` hexagon;
2500 generated), and a 20,386-case run (`--seeds 20000`) also clean; `--perturb` detects
every case. No `UNPORTED(nlib.srf_adv_p1)` stubs existed to wire. Upstream behaviour worth
knowing:

- **Degree-1 extensions read past `Rw` (inherited from `ST_surxsdr`).** `N_SrfExtendByDist`
  is a wrapper of `ST_surxsdr` (`nlib.srf_adv_p2`): with G1/G2 in a degree-1 direction the
  reflection-plane fallback can read outside the scratch row, where upstream "succeeds" on
  whatever memory it reads and the port returns `InvalidInput`. The generator keeps
  degree-1 extension directions to `NL_CMAX`. Past the recursion limit the stale error
  number is compared as the bare flag, as in the p2 runner.
- **`N_SrfEvalPtDerivsUnbounded`** wraps parameters of a G1-closed direction with `while`
  loops that never end when `ue - us` no longer changes the parameter (or take ~1e12 steps
  for far parameters); the port stops with `NotConverged` (after `MAX_PERIOD_SHIFTS`) and
  the FFI only passes parameters within 16 domain lengths (or beyond `1e12`, the immediate
  bare-flag exit). It copies the surface into `esur` before `N_SrfIsClosedSmooth`; if that
  test fails `esur` stays set with the caller's `gflg`, while the port leaves its
  extension `None` (the difftest stops a call sequence at the first failure).
- **`N_SrfOffsetGetMaxDeriv`** allocates `u`/`v` with `N_AllocReal1dArray(p)` (p + 1 entries,
  enough) and clears every `N_SrfEvalPtNormalDeriv` error (skips the sample).
- **`N_CreateSkinSpine`** modifies the profiles in place (compatibility, alignment, and the
  `N_CrvsAreCoincident` closedness test on profiles 0 and k), reads
  `pSpineParams[iNumProfiles - 1]` (index -1 for a single profile with end derivatives;
  the port returns `InvalidInput`), and with `bMakeCompatible == NL_NO` copies profile 0's
  control point count from every profile (the FFI requires equal shapes then). Rational
  profiles often produce weights outside `[NL_WMIN, NL_WMAX]` (`NL_WEI_ERR`, surface built
  but not returned by the port).
- **`N_CrossBoundaryDerivsVectorField`** compares knots exactly; two valid clamped curves
  with equal knot vectors have equal degree and size, so its unchecked `Qw[ii]` reads stay
  in bounds.
