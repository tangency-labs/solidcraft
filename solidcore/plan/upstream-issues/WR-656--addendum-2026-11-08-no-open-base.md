Updates: WR-656

#### WR-656 (addendum 2026-11-08): no base comes back open; its flag is retired (#3093)

- **Rust port:** `UpstreamFixes::defeature_refuses_open_base` is gone. On `Kernel::default()`, `FeatureExecutive::do_defeature` never takes off a feature whose base patch could not be built. It puts that feature back (upstream's `RestoreFeature`, which upstream leaves to its caller's `RestoreFeatures`) and returns it as a `KeptFeature` naming the loop edge of its hole. The features that rebuilt come off. When none comes off, the call is refused with `SM_ERR` at that loop edge and the base is put back untouched.
- **Reached by:** 73 `blend.feature_executive` goldens, every one an open base before (measured, refusal off: 72 not manifold; the 73rd, `multi_rebuild_2_extrusion`, closed by the case's own `RestoreFeatures` afterwards). Now 70 are refusals with the original body untouched and 3 are partial successes with their unrebuilt features put back: `multi_rebuild_1_extrusion`, `multi_rebuild_2_extrusion` (757.4995, as upstream's restore makes it) and `fillet_each_cyl_on_box_w_fillet`.
