# Bounded linear film accumulation

`solidcraft_render::film` is the independent accumulator for #1696. It is extracted from the original renderer donor at `3b892eab`; it does not install a CPU integrator, PNG resolver, camera coverage buffer or progressive UI.

Samples are nonnegative finite linear RGB. Per-pixel running means and unbiased sample variance remain linear: no exposure, denoising, tone mapping or display conversion occurs here. A later consumer applies its output transform exactly once after resolving a complete pass. Sample order is significant; splitting the same ordered samples into bounded contiguous calls preserves identical results, while arbitrary floating-point reordering is not promised.

The caller reserves `required_bytes` in the existing image-job lease and retains it throughout the film lifetime. Allocation is fallible and actual capacity is checked. Dimensions, pixel count, sample count, storage and update sizes are bounded. This module creates no second pool, thread, cache or cancellation token. Initialization and updates debit the same caller-provided cumulative work budget.

`add_range` stages at most 256 pixel updates before committing. Invalid input, range/count overflow, exhausted work or cancellation before the bounded commit leaves prior pixels unchanged; consumed work is not refunded. Cancellation after commit must still be checked by the consumer before publication.

`complete_pass` refuses empty or uneven sample counts and returns an immutable borrowed view, not an owned or publishable image. The caller must separately admit any output snapshot and enforce its existing generation/cancellation publication lock. Camera background coverage and actual progressive image delivery remain separate integration work.

No design data is newly saved, so no format descriptor is introduced. No extra dependency, asset, geometry operation or renderer capability is advertised.
