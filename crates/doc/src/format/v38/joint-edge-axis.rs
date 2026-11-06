//! An edge joint origin (`Snap::Edge`) saves an optional `axis`: `{"along": [x, y, z]}` (z runs
//! along the edge, the way the vector points) or `{"fixed": [x, y, z]}` (z is that direction), for
//! an edge or a corner picked in the Joint dialog, which used to be saved as a point. Older
//! designs have no `axis` and resolve as before (z out of the face). The last release refuses a
//! design with this change by its newer format number.
