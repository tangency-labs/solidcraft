//! A pattern feature can lay its copies on a face: `"pattern": {"type": "face", ...}` with the face
//! (a reference found again on each evaluation), the grid's two counts, and optionally `staggered`,
//! `angle`, `margin`, `flip`, `anchor` and `attract`: the attractors (`geometry` and `bodies`, and
//! optionally `far_geometry` and `far_bodies`) and the scale, offset and turn each copy takes from
//! its distance to them. Older designs have no such pattern, so nothing in them is rewritten. The
//! last release does not know the pattern type: it warns about the newer format number, opens what
//! it can read and keeps the rest.
