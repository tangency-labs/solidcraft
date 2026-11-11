//! A canvas can be locked (#12310): `locked: true` on the canvas keeps its place, size, angle and
//! flip, and keeps it from being deleted, until it is unlocked. Older designs need no
//! transformation: without the field a canvas is unlocked, as before. The last release warns about
//! the newer format number and opens the design with its canvases unlocked.
