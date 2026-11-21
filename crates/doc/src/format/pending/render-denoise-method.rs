//! Saved CPU and GPU render intent: `denoise_settings` gains `method` (`spatial` or
//! `oidn`) and `oidn_quality` (`fast`, `balanced` or `high`). Older designs keep the
//! built-in spatial filter and default the unused OIDN quality to `high`; no rewrite is needed.
//! Builds without Open Image Denoise, including the web build, keep the saved choice when
//! opening and saving; rendering with that choice requires support or an explicit change.
//! The last release warns about the newer format number, opens what it can read and keeps
//! the fields it cannot read when saving again.
