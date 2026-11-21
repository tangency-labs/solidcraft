#### WR-799. Gap sampling of a B-spline surface asserts when its sample domain has no length in U or V

- **Severity:** Wrong result (a gap function refused with `SM_ERR_ASSERT_FAILURE`)
- **Area:** Healing (gap functions)
- **Location:** `source/SMLib/src/SmGap.cpp:1764-1767`, `SmGapFunction::BuildSampleSet`; solidcore `crates/solidcore/src/heal/gap.rs`, `GapFunction::sample_normalized_values`.
- **What happens:** for B-spline geometry the samples go per knot span of the sample interval. A zero-length interval in U or V (the samples of one iso line) indexes one knot at both ends, so it spans no knot interval, and `AERN_MSG` returns `SM_ERR_ASSERT_FAILURE` ("BuildSampleSet for Splines found an unhandled degenerate case - review case and extend method"; the comment above it: "a case that isn't expected to be supported"). The same domain on non-B-spline geometry is sampled by even stride.
- **Trigger:** a gap function to or from a B-spline surface whose sample domain is one iso line, for example `heal.gap` `gen0061` (u in [0.5, 0.5], v in [0, 1]).
- **How found:** making a curve-bounded surface sample as its basis B-spline (WR-20163, the curve-bounded record) turned `gen0061`, which main sampled by even stride, into this assertion; plain B-spline cases of the unit (`gen0086`, `gen0195`, `gen0197`, `gen0202`, `gen0241`, `gen0274`, `gen0384`, and with no samples set `gen0027`, `gen0112`, `gen0269`, `gen0370`) had always met it.
- **Confirmed:** yes, in the port, which kept the assertion: `heal::gap::tests::a_b_spline_surface_samples_one_iso_line_by_even_stride` fails with status 1005 without the change. The pinned C++ was not run.
- **Rust port:** a surface's sample domain that spans no knot interval in U or V is sampled by even stride over the counts as given, as non-B-spline geometry is. A curve's zero-length interval keeps the refusal (`wr89_unsupported_inputs_are_errors`).
