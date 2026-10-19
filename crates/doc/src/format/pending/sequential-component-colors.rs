//! Components save an optional `identification_color_index` and the document saves
//! `next_component_color_index`. Automatic colours follow creation order and wrap through the
//! palette; deletions do not rewind the sequence. Older records receive indices in stored order
//! during normalisation, preserving explicit RGB overrides. The last release refuses the newer
//! format rather than dropping the saved sequence.
