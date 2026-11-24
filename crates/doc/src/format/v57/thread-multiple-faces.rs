//! A Thread feature may save additional_faces beside its original face, so one feature
//! threads several cylindrical faces with shared options. New plural selections also save
//! target_bodies, one definition-body identity for each face in selection order. Older designs
//! omit these fields and keep their previous selection behavior. The last release warns about
//! the newer format, opens the original face where it can, and preserves fields it cannot read.
