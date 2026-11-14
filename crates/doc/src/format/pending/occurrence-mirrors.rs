//! Mirrored occurrences: `assembly.mirrors` lists the occurrences placed as the mirror image of
//! another occurrence across a plane (the occurrence, its source, the plane and the frame it is
//! in, and the part's own reflection that keeps the placement rigid), so they follow the source
//! when it moves. Older designs have no such list and nothing follows. The last release refuses
//! a design with this newer format number instead of opening it with the mirrors placed once
//! and no longer following.
