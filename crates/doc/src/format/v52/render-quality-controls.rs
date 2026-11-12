//! Saves CPU worker selection, linear 2x supersampling and denoising controls in render setups.
//! Older designs retain native-size output, three full-strength denoising passes and no firefly suppression.
//! DenoiseSettings.optical_filter defaults to true for supported mirror/glass filtering; an explicit false
//! restores legacy opaque-only denoising and is retained in CPU and GPU setups. No rewrite is needed.
//! Omitted CPU worker selection uses the bounded automatic pool, with one worker available for serial work.
//! The last release warns about this newer design format and preserves fields it cannot read.
