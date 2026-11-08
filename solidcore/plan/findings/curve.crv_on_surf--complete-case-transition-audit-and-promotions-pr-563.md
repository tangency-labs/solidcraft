# curve.crv_on_surf: complete case-transition audit and promotions (PR #563)

Code head `01890faa8`, integrated with authoritative main `b65597902`, passes all
seven checks in x86_64 CI run 39928207576: 260 native units, 173 green, 87 partial,
zero failed. Local fmt, workspace Clippy, kernel tests and wasm pass. Native
difftest harness tests also pass. No code, fixtures or guards changed after this
verification; the final follow-up records evidence and promotes certified units.

The complete ARM64 run produced all 260 reports. Against main's CI artifact
(run 39927450627), the case-name audit finds exactly:

- 545090 original passes still passing; 1436 original deferrals now passing.
- 6943 deferrals remain: 1344 change blocker, 5599 keep the same blocker.
- No original pass becomes deferred; no case is removed; the only two added
  cases are the passing healer sibling-cache regressions.
- All 239 rejected cases and five known mismatches keep their classifications.
- The remaining 738 original x86_64 passes fail only against the ARM64 native
  oracle in the four documented #448 hash-conversion units: geom.hash_table 435,
  heal.heal_data_p1 15, p2 260, p3 28. All 732 previously reachable ARM64 failures
  have identical mismatch details; the six newly reachable p2 cases were formerly
  blocked by main's edge-tolerance stub and do not call stage two. The x86_64 run
  passes these cases; no exceptions were added to the harness.

Genuine original deferral-to-pass gains (the separate 1344 moved deferrals are
not included): curve.crv_on_surf 307; heal.face_props 114; heal.gap 16;
heal.heal_data_p1 25; intersect.curve_class_p1 4; p4 153; p8 214; p9 18;
intersect.surface_drop_lib_p1 13; query.tess 11; surface.surface_cache_p2 1;
surface.surface_p9 40; topology.edgeuse_p2 390; topology.edgeuse_p3 80;
topology.face_p4 50. Total: 1436, plus the two new passing fixtures.

Promote the six downstream units with no deferred or failed cases, in addition
to CrvOnSurf: heal.gap 445/445, intersect.curve_class_p8 1127/1127,
query.tess 676/676, surface.surface_cache_p2 905/905,
topology.edgeuse_p2 4583/4583, topology.edgeuse_p3 4789/4789. Their compared values
are bit-identical on both native architectures. All previous passes in these
seven promoted units are retained. Other partial units stay partial.

Every one of the 1436 deferral-to-pass conversions compares actual floating-point
values (`values_compared > 0`), with maximum relative error exactly zero. None is
a status-only or vacuous pass.
