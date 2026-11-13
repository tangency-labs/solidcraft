//! Saved GPU presentation and optional raw linear EXR retention.
//! Missing values keep exact neutral presentation, the legacy flat background
//! and PNG-only execution. No legacy document rewrite or transport change.
//! The optional screen-space backdrop is independent of illumination and cannot
//! be combined with transparent PNG output. The EXR omits display adjustments.
