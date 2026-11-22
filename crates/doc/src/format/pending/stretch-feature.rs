//! Stretch (#20512): a new feature type, `stretch`, saves the body it stretches (`body`), the way
//! its moving side goes along one of its component's axes (`direction`: `plus_x` … `minus_z`),
//! the cut plane's distance from the side that stays put (`at`, a length expression) and how far
//! the moving side goes (`distance`, a length expression; negative shrinks). Older designs have
//! no such feature and open as before. The last release warns about the newer format number and
//! keeps a Stretch it cannot read as it was saved, without applying it.
