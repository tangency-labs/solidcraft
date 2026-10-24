//! Components save an optional `identification_color_index` and the document saves
//! `next_component_color_index`. Automatic colours follow creation order and wrap through the
//! palette; deletions do not rewind the sequence. Older records receive indices in stored order
//! during normalization (not the order they were originally created), preserving explicit RGB overrides.
