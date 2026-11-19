//! Designs can contain editable subdivision Form features, with primitive parameters,
//! authoritative control cages, fit tolerance and an optional boolean operation. Earlier
//! designs gain no Form features. An older reader warns about this change and keeps any
//! Form source it cannot read when saving.
//! Forms may also save stable local leaf domains, original coarse controls and sparse
//! details at nested subdivision levels. Forms without this data retain their base cage.
//! A refined Form may save face and free-edge extrusions at their authoring levels. The original
//! hierarchy, logical cap IDs, displacement and subsequent details stay editable.
//! Mirrored face or edge regions can save different displacements, which remain when
//! symmetry is turned off. Missing extrusion records retain the original evaluator topology.
//! A trimmed edge can retain a derived wall anchored to an active chart's oriented
//! parameter segment. The stored displacement edits its rim while the hidden subdivision
//! stencil remains intact. Missing wall data retains the existing trimmed surface.
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
//! Match may retain ordered named support edges and endpoint orientation independently
//! of a face's native UV loop seam. Older positional coordinates are recovered only when
//! exactly one bounded loop ordering satisfies all retained physical limit constraints;
//! an already changed or ambiguous legacy support is refused without changing its source.
//! Closed support edges additionally retain their complete bounded rational
//! parameterization in the source definition frame. Positive uniform size changes
//! and tracked rigid placement preserve its intrinsic seam; an uncertifiable seam,
//! curve-type or nonuniform intrinsic change is refused instead of rebinding by proximity.
//! A Form Surface may retain one named source CAD face, a regular guide-grid
//! resolution, G0/G1 continuity and undeformed local samples. The CAD rim stays
//! fixed while interior offsets survive source-face recomputation. Missing data
//! retains the ordinary subdivision Form behavior.
//! A Surface may retain the same named boundary binding and sixteen exact rim
//! stations in its logical normalized UV chart. Without that chart, only one
//! whole-rest-grid correspondence may upgrade a legacy record. Ambiguous or already
//! changed legacy source geometry is refused. Bound recomputation transports interior
//! sculpt offsets in logical orthonormal grid frames, retaining their lengths without
//! stretch amplification. Missing chart data never licenses a guessed rotation.
//! An optional Surface face-grid flag asks for exact CAD cells on the fitted
//! surface, with the single-face fallback when a partition cannot be qualified.
//! Older Surface records keep their single CAD face when the flag is absent.
//! A refined Match may store terminal-level crease overrides alongside sparse
//! control details. Hierarchies without overrides inherit sharpness as before.
//! Match can retain inherited crease history when a bound opening is refined
//! further. Missing ancestor records mean all generated creases are at the
//! current evaluator level, as in earlier editable Forms.
//! A Pipe may retain named sketch curves or CAD edges, section density, global
//! diameter and path sampling, and per-chain overrides. It follows the path
//! while parametric; after sculpting, its saved cage is retained until explicit
//! regeneration. Missing Pipe metadata preserves existing independent cages.
//! An imported Form may retain its Blender source file and object name as
//! read-only provenance. This never opens or executes the source file on load.
//! Unfinished authoring autosaves may retain a bounded support-repair error beside
//! the unchanged accepted Match or Surface source, including undo/redo history.
//! Missing repair data means ordinary editing. Staged replacement previews and
//! derived qualification certificates are never persisted as accepted repairs.
//!
//! A refined Form can save one exact triangular trim of a planar boundary leaf.
//! The retained rectangular evaluator still carries sculpt details and stable
//! identities. Earlier designs have no collapse trim. An older reader warns and
//! preserves the trim it cannot read.
//!
//! A refined Form can save additional disjoint quad cap records. Each record
//! retains its face slot and directed rim. Older designs still open with their
//! original single cap or no cap. An older reader warns and preserves the cap
//! records it cannot read.
//!
//! A Form support edge with two distinct vertices may name both vertices when
//! their incident edge names coincide. Older bindings retain their intrinsic
//! parameter checks. An older reader warns and preserves the additional names.
//!
//! Thicken stores which Form sources use an exact wall swept from planar spline
//! boundaries. Older designs have no saved planar construction and keep their
//! existing evaluation. An older reader warns and preserves the construction
//! choice it cannot read.
//!
//! A refined Form may save a derived quad cap with its reserved logical face
//! slot and four directed rim sides and retained owners. Earlier designs
//! without a cap record keep their original editable skin. An older reader warns
//! and preserves the cap record it cannot read.
//!
//! Refined Unweld can save detached logical leaf IDs while retaining the original
//! evaluator. Detached slots stay empty and retain shared boundary stations.
//! Earlier designs have no detached leaves. An older reader warns and preserves
//! the detached leaf records it cannot read.
//!
//! A Stitch can save one qualified seam between two independently editable refined Forms.
//! The record stores typed source feature IDs and directed logical rim owners.
//! Older designs contain ordinary Stitch features with no seam. An older reader
//! warns and preserves the seam record it cannot read.
//!
//! Match can retain the earlier tensor interpolation basis, evaluator depth,
//! and continuation order when aligned subdivision changes only topology.
//! Older Match definitions omit the optional continued_surface record and
//! retain their ordinary current-evaluator fit. An older reader warns and
//! preserves the continuation record it cannot read.
//!
//! Refined Forms can save retired quad cap face slots after removing a cap.
//! The saved IDs keep those slots empty when the rim is capped again or the
//! skin is refined. Earlier designs have no retired cap slots. An older reader
//! warns and preserves the retired slots it cannot read.
//!
//! Symmetric Thicken on a saved refined Form retains the chosen construction.
//! Older files have no recorded construction and keep their existing evaluation.
//! An older reader warns and preserves the construction choice it cannot read.
