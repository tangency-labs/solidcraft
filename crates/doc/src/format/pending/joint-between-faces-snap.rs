//! Midplane joint origin reference (#22615): `Snap::BetweenFaces` (`between_faces`) can save an
//! optional `snap` (a non-between-faces snap on the same component) whose resolved point is
//! projected onto the midplane between `a` and `b` on every solve. Older designs omit `snap` and
//! load identically (`snap: None`), projecting the midpoint of `a.pick` and `b.pick` as before.
//! The last release warns about the newer format number, opens what it can read and keeps the
//! unreadable assembly section intact when saving. No step: nothing older is rewritten.
