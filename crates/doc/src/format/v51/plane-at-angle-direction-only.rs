//! An at-angle construction plane may save `direction_only` and an optional `pivot` point: the
//! plane turns about the line through the pivot (or the base origin) using only the axis
//! direction projected onto the base. An older design has neither and keeps turning about the
//! axis as a full line. The last release refuses a design that has this change (the newer
//! format number) instead of opening it without the option.
