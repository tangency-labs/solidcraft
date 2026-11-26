# Addendum: the small elbow's invalid body, traced to the mitre's reference points and fixed (2026-11-12)

Addendum to `primitive.primitive_creation_p2--exact-net-elbow-unites-into-an-invalid-body-below-radius-0-1.md`.

**The cause.** The record names the fixed 0.1 reference offsets of the mitre trim and sets them
aside because a leg of 1 fails like a leg of 1e-2. The length of the leg does not enter. What
enters is the length of the cutting line, which is the profile's largest dimension times 2 sqrt 2
each way. A reference point 0.1 along a leg has its foot on the cutting line 0.0707 from a
right-angle corner. For a circle of radius below 0.0125 that is past the end of the line, the
side test of the reference point fails, the sweep ignores the status, and the piece stays whole.
Between radius 0.0125 and 0.025 the trim runs but is wrong or untidy: 7 faces and not valid, or
valid with 12 faces where 4 are right. `WR-780` has the C++ lines and the measurements.

**The change.** `planar_path.rs` steps the smaller of 0.1 and half the reach of the cutting line.
tangency-labs/solidcraft#13687.

**The cases.** `gen0092` and `gen0189` now return a valid closed body (`check_brep` level 2, no
failed check) of 4 faces and of volume 6.2831722e-8 against the closed form 6.2831853e-8. Their entries are
removed from `crates/goldens/expected/known_failing.json` and their lines are blessed.
