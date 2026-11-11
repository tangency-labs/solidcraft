//! Designs save the Trapezoidal 90° thread for 3D printing: a thread feature's `form` can be
//! `print` (designations `PT30x3`, `PT30x6(P3)`), and such a thread can save `print_fit`, its
//! flank, crest and root clearances in mm, when they differ from the normal fit. Older designs
//! have no printed threads and need no migration; a printed thread without `print_fit` has the
//! normal fit. The last release warns about the newer format number and cannot rebuild a printed
//! thread, which it does not know.
