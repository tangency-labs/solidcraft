//! A Single hole can be placed by distances from up to two straight edges of its face
//! (`references`: each `{"edge": [x, y, z], "distance": "…"}`, the edges' persistent names in the
//! feature's `edge_names`, #37): it is drilled where those distances hold, measured in the face's
//! plane, and follows the edges when the part changes. Older designs need no transformation: a
//! hole without `references` stays at its position, as before; 0.21.x asks for an update instead
//! of drilling a referenced hole where it was first placed.
