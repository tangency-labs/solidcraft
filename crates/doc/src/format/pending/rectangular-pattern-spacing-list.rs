//! A Rectangular Pattern direction can be set by its count and a list of spacings:
//! `distance1` / `distance2` is `count_list` and `spacing_list1` / `spacing_list2` holds the
//! gaps between neighbours, used in turn and from the start again. `spacing1` / `spacing2` keep
//! the list's first gap. Older designs need no rewriting: they have no `count_list` direction
//! and no list. A release that reads the other distance types but not `count_list` warns about
//! the newer format number and keeps the pattern on the timeline as a feature it can't read,
//! saved back as it came; a release without distance types keeps the fields it can't read and
//! builds the direction from its count and the kept spacing, so every gap is the list's first.
