//! A Rectangular Pattern says, for each direction, what sets its copies (#8523): `distance1` and
//! `distance2` are `count_spacing` (the count and the spacing, as before), `count_extent` (the
//! count and `extent1` / `extent2`, the distance from the first copy to the last) or
//! `spacing_extent` (the spacing and the extent; as many copies as fit, with `copy_at_end1` /
//! `copy_at_end2` adding one exactly at the extent's end). Older designs need no rewriting:
//! without the fields every direction is count and spacing. The last release warns about the
//! newer format number, opens such a design and keeps the fields it can't read, but builds each
//! direction from its count and spacing.
