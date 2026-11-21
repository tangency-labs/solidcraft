A B-rep point classification (`query::brep::ray`, `SmBrep::ClassifyPoint` and
its ray casts) while the body is being edited builds the face's trimmed-surface
tree with the face's current trims implanted, where upstream's
`SmTrimSrfCache::BuildTree` skips the trims while editing and answers against
the untrimmed surface, right only when it reuses a tree built before the edit
(a face trimmed mid-session is classified as its whole surface). Only a face
whose loops close is implanted (`query::brep::ray`'s `loops_close`, reading
the edges' ends through their uses; vertices a stitch's glue is still merging
meet within their tolerances). One whose trims then fail to implant, and one
whose loops are open mid-edit (#2624), is
[`crate::SolidError::FaceTrimsUnavailable`] naming the face, not an answer
against the untrimmed surface. Classifications outside editing are unchanged
(WR-732, #2503; solidcore-native, no fork commit).
