//! A body Pattern keeps the placement each body was picked at (`placements`: one occurrence path
//! per body, as a Mirror's), and copies the body where that placement shows it, so the bodies of
//! a part placed several times in an assembly can be patterned as they are seen. A body the
//! pattern cannot copy is left out with a warning instead of failing the pattern; the saved model
//! records which (`pattern_left_out`, absent in older caches), so the names stored for its copies
//! stay in their places. Older designs
//! need no transformation: a Pattern without `placements` copies the bodies named as before; the
//! last release asks for an update instead of copying placed bodies there.
