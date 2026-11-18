//! A finger-joint feature stores two or more body references in selection order, its pattern
//! count, clearance, inversion, and edge offset. Older designs do not contain this feature.
//! Missing inversion and edge offset default to a non-inverted pattern with no edge offset.
//! The last release warns about the newer format, opens what it can read, and preserves the
//! unknown feature data when saving.
