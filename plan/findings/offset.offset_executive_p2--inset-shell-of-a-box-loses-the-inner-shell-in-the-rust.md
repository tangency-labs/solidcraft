# offset.offset_executive_p2: inset shell of a box loses the inner shell in the Rust Difference

Unit `offset.offset_executive_p2`, cases `gen0000` and `gen0003` (static `ShellBrep`
of an `SmApiCreateBox` box, `extend_convex_edges`, negative distance, no shell faces;
`gen0003`: box at (122.38, -217.37, ...) with distance -5). Both sides build the same
inset shell (Rust: 6 outward planes, inset by |distance|, one void and one solid
region, manifold) and pass it to `SmMerge::ManifoldBoolean(SM_BO_DIFFERENCE)` with
the original as primary operand, whose result is the original itself on both sides.
C++ returns the hollow box: regions/shells/faces/loops/edges/edgeuses/vertices
3/4/12/12/24/96/16. Rust returns the original unchanged: 2/2/6/6/12/48/8. The
operands do not intersect (the inner box lies strictly inside), so the suspected
cause is the Rust Difference's handling of a fully contained, non-intersecting
other operand in the merge editing stages (`boolean.merge_p4` /
`boolean.merge_p2` classification): the contained shell's faces are dropped instead
of being kept reversed as a void. Listed in `known_mismatches.toml`; the offset
port itself is not involved after the shell is built.
