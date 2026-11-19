//! An Import kept as B-rep records can hold a part the file gives as triangles alone (an AP242
//! tessellated part read through cadmpeg): its record body has a `mesh` (vertex positions and
//! triangles) in place of a B-rep record, and its body is a mesh body, as an STL or 3MF import's
//! is. Older designs have no such bodies and need no rewriting. The last release warns about the
//! newer format number, opens what it can read and keeps the rest; it cannot build such an
//! import's bodies, as a mesh record is newer than it reads.
