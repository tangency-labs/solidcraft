//! Nothing new is saved for holes, but a hole placed on a face that names the bodies it drills
//! (`participants`: its `targets`, or the body of another component it was picked on) now finds
//! the face it starts on among those bodies' faces, where one of them passes through its position
//! (#12066). Where another body touched that face at the hole, older designs could save a
//! `direction` taken from the other body's face, pointing out of the drilled body; such a hole cut
//! only a shallow pocket. It is now mirrored into the drilled body's face when the design is
//! evaluated, as format 18 does for a direction taken from the far side of a thin plate, and
//! drills through. Holes whose direction enters one of their own bodies' faces there, holes whose
//! own bodies have no face at their position (placed on another body's face to drill through it),
//! holes that name no bodies, and holes at several sketch points (already limited to their bodies,
//! #2882) build as before. No rewrite is needed. The last release refuses a design with this
//! change (the newer format number) instead of building the pocket again.
