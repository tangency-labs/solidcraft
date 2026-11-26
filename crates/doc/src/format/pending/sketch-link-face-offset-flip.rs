//! An offset-curve sketch link (`{"type": "face_offset", ...}`, #22414) can save `"flip": true`:
//! the curve goes to the other side of the sketch curve it is offset from than the link's face
//! point `at` (an edge's offset always goes into the face, so an edge's link never has it). It is
//! left out when false, so a link made without the dialog's Flip saves as before, and a design
//! without it means what it meant. The Offset Curve dialog's Flip sets it. A release that knows the
//! link but not `flip` would read a flipped link as unflipped and put the curve on the other side.
