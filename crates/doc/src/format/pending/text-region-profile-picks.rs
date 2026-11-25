//! A profile pick that takes every letter of one or more sketch texts saves those texts' links
//! (`{"type": "text_regions", "links": [...]}`) instead of the letters' curve ids, so the pick
//! follows the text when its words change (#727). Older designs keep the picks they saved. The
//! last release warns about the newer format number, opens the design and keeps a feature with
//! such a pick as a feature it can't read.
