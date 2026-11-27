//! A surface extrude can lean its walls by a taper: its `extent.taper` (the angle) and
//! `extent.corner` (how the taper closes a sharp corner) are now saved and built for closed
//! profiles and closed loops of curves, as a solid extrude's are. Older designs carry neither, so
//! they rebuild as before. The last release refused a surface extrude with a taper: it warns that
//! the design is newer, opens it, keeps the feature as saved, and shows that feature's error
//! instead of its surface.
