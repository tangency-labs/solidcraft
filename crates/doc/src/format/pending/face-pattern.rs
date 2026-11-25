//! A pattern feature can lay its copies on a face: `"pattern": {"type": "face", ...}` with the face
//! (a reference found again on each evaluation), the grid's two counts, and optionally `staggered`,
//! `random` (a count, a seed and a noise colour, in place of the grid), `angle`, `margin` (the edge
//! margin), `overhang` (copies may hang over the face's edges; absent, none does), `avoid` (points, faces and bodies, `geometry` and `bodies`, and its own `margin`), `flip`,
//! `anchor` (absent: the centre of the bottom of the copied bodies' bounding box) and `attract`:
//! the attractors (`geometry` and `bodies`, and optionally `far_geometry` and `far_bodies`) and the
//! scale, offset and rotation each copy takes from its distance to them. Older designs have no such
//! pattern, so nothing in them is rewritten. The last release does not know the pattern type: it
//! warns about the newer format number, opens what it can read and keeps the rest.
//! A face pattern can also scale, offset and rotate every copy (`scale`, `offset`, `rotate`);
//! with `attract`, a near or far value it leaves out is the pattern's own. Designs without them
//! are unchanged (1, 0 and 0, as before), so nothing in them is rewritten.
