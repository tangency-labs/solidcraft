//! A canvas saves an optional explicit `height` (mm) when stretched non-uniformly
//! (`Canvas::height: Option<f64>`, omitted when `None`). Older designs need no transformation:
//! without `height`, `Canvas::height()` derives the height from `width` and the image's pixel
//! aspect ratio, as before. The last release warns about the newer format number, drops `height`,
//! and opens the canvas at its original aspect ratio from `width`.
