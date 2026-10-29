//! Designs can contain editable subdivision Form features, with primitive parameters,
//! authoritative control cages, fit tolerance and an optional boolean operation. Earlier
//! designs gain no Form features. Releases without this change refuse the newer format
//! rather than discarding editable source data or misreading a Form as a mesh import.
//! Forms may also save stable local leaf domains, original coarse controls and sparse
//! details at nested subdivision levels. Forms without this data retain their base cage.
//! Deleted leaf domains and dissolved logical seams retain the original evaluator and
//! surviving control identities; missing seam records mean that every boundary is visible.
//! Whole-Form rigid motion may save a separate local placement. Designs without it
//! use the identity placement and keep their existing local control coordinates.
//! A Form may retain associative Match support by persistent body/face or edge names,
//! normalized parameters on individual support curves, bounded seam tolerances, a fixed
//! fitting basis and previous automatically changed sharpness. Missing Match data means
//! an independent Form. Derived evaluation rings and support preview geometry are not saved.
//! Match may choose the exact neighboring faces across the seam instead of the selected
//! support surface. Missing adjacency mode retains selected-surface behavior.
//! A Form Surface may retain one named source CAD face, a regular guide-grid
//! resolution, G0/G1 continuity and undeformed local samples. The CAD rim stays
//! fixed while interior offsets survive source-face recomputation. Missing data
//! retains the ordinary subdivision Form behavior.
//! An optional Surface face-grid flag asks for exact CAD cells on the fitted
//! surface, with the single-face fallback when a partition cannot be qualified.
//! Older Surface records keep their single CAD face when the flag is absent.
//! A Pipe may retain named sketch curves or CAD edges, section density, global
//! diameter and path sampling, and per-chain overrides. It follows the path
//! while parametric; after sculpting, its saved cage is retained until explicit
//! regeneration. Missing Pipe metadata preserves existing independent cages.
//! An imported Form may retain its Blender source file and object name as
//! read-only provenance. This never opens or executes the source file on load.
