//! A profile picked by the curves around it (`"type": "curves"`) also saves, in `sides`, the side
//! of each of those curves each profile lay on when it was picked (#23291). When a change makes
//! the curves bound several profiles, the one lying the same way is taken instead of failing.
//! `sides` has one list per loop; a loop of offset curves, or one whose sides can't be told,
//! has an empty list and is read by its curves alone.
//! An older design has no `sides` and keeps failing on such a change, as before. The last
//! release ignores `sides` and reads the pick by its curves alone.
