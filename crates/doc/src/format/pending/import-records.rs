//! An Import can keep its bodies as B-rep records instead of STEP text: a FreeCAD `.FCStd` or
//! Rhino `.3dm` file read through cadmpeg's neutral model is saved as the records it was read
//! into (`records`, held in the design container as the part `records_blob`, compressed as
//! STEP is), and its `step` is then empty. Older designs have no records: every Import is its STEP
//! text, as before, and needs no rewriting. The last release warns about the newer format
//! number, opens what it can read and keeps the rest; it cannot build such an import's bodies, as
//! its STEP text is empty.
