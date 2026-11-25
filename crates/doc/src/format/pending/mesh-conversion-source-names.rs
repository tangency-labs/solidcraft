//! Mesh conversions replay through the one SolidCore converter, using analytic faces where
//! certified and the original closed mesh's planar facets otherwise. Every conversion uses
//! source-bound v6 face names. Earlier designs and unreleased reconstruction/source_bound_names
//! fields also replay this path; obsolete fields are ignored. References that no longer resolve
//! report the normal broken-reference error and may need reattachment. The previous release
//! opens a newer-format design with a warning and preserves fields it cannot read.
