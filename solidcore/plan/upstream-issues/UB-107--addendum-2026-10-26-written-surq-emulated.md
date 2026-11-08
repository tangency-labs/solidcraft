Updates: UB-107

#### UB-107 (update, 2026-10-26). The fully written in-place state is now emulated

The Rust port (pin/v1.0.1) now distinguishes the in-place states `CreateExtendedSurface`'s
retry sees after a failed `N_SrfExtendByDist(&esur, .., &esur)` (`ST_surxsdr`):
- When the closing `N_SrfRemoveKnotConditional` fails after finishing its removal (its
  last `N_BasisFindGlobalMax` failed), `surQ` is complete; the retry extends that
  surface, and `srf_adv::p2::extend_by_dist_in_place` now does the same. This is not
  undefined behaviour and is now compared (`surface.surf_of_extrusion_p2`
  `lazy_big_extend_*`, exposed by 1.0.1's `N_CrvRemoveKnot` change; see
  `plan/findings/nlib.srf_adv_p2--in-place-extension-state-on-error.md`).
- The half-written case this record describes (an error such as `NL_WEI_ERR` after
  `N_AllocSrfArrays` and before `surQ` is complete) is unchanged: upstream reads
  uninitialised entries, and the port keeps the input surface there.
