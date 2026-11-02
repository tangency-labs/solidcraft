//! An Extrude's To / From Object can be a reference that is not a body face (`Extent::to_ref`,
//! `from_ref`): a sketch point or curve, an origin or construction plane, axis or point, or a
//! vertex, found again on each evaluation so the extent follows it when it moves. `to` / `from`
//! still hold where it was picked. Older designs need no transformation: their To / From stay
//! the points they were saved with. The last release refuses a design saved with this (the newer
//! format number).
