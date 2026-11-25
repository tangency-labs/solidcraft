//! An Import kept as B-rep records keeps how the file shows each body beyond its colour, as a STEP
//! import does: `paint` on a record body holds its opacity, its colour's name in the file, and the
//! faces with colours of their own (each by its face in the file). Older designs have no paint:
//! their bodies show in their colour, as before, and need no rewriting. The last release warns
//! about the newer format number, opens what it can read and keeps the rest; it cannot build such
//! an import's bodies, as their records are newer than it reads.
