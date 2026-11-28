//! An at-angle construction plane may save `through_axis`: the base is first brought onto the
//! axis (moved along its normal, tilted when the axis is not parallel to it), so the plane goes
//! through the whole axis (#6180). An older design doesn't have it and keeps turning the base
//! about the axis where it is. The last release refuses a design that has this change (the newer
//! format number) instead of opening it without the option. A build that opens newer designs
//! (#12166) names `through_axis` in its notice as a field it doesn't read and keeps it on save.
