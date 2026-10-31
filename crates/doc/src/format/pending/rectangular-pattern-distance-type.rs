//! A Rectangular Pattern chooses, for each direction, what fixes its copies' distance (#8523):
//! Spacing (between neighbours, as before), Extent (`extent1`/`extent2`: `spacing1`/`spacing2`
//! is the distance from the first copy to the last) or To Object (`to1`/`to2`: the face or point
//! the last copy's centre reaches, followed by its persistent name, past it by `to_offset1` /
//! `to_offset2`; `to1_ref` / `to2_ref` hold a reference that is not a body face, such as a
//! sketch point or a construction plane, found again on each evaluation as an extrude's
//! `to_ref`). Older designs need no transformation: without the fields every direction is
//! Spacing. The last release refuses a design saved with this (the newer format number).
