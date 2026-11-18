//! An Import kept as B-rep records can place one part more than once, as an assembly that uses a
//! part twice does: a record body may be a copy of an earlier one (`copy_of`, that body's index),
//! with its own placement and no record of its own (`brep` and `face_origin` left out). The copies
//! are the same part to the design. Older designs have no copies and need no rewriting. The last
//! release warns about the newer format number, opens what it can read and keeps the rest; it
//! cannot build such an import's bodies, as a copy has no record it can read.
