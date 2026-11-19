//! A Pattern of components keeps the component ids it places and the occurrence ids of the
//! copies it made, so a count or spacing expression updates those placements on rebuild
//! (#12116). Older designs have neither field: a Pattern without `components` is of features
//! or bodies as before, and one-shot component placements made before this change stay as
//! fixed occurrences with no Pattern feature. The last release asks for an update instead of
//! opening a design that has these fields.
