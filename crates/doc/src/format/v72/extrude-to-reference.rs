//! An extrude's To / From Object can be a construction plane or point, an origin plane or the
//! origin (#8527): `extent.to_ref` and `extent.from_ref`, a reference as the construction
//! features save one, found again on each evaluation so the extrude follows it. `to` / `from`
//! still hold a point on it, so the extrude still reads as To / From Object. Older designs need
//! no transformation: without the fields `to` / `from` are the points they were saved with. The
//! last release asks for an update (the version stamp) instead of ending the extrude at the
//! saved point, where the reference no longer is once it moves.
