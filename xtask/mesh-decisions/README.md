# Mesh decisions

Functions that decide geometry with a mesh today, one file each:
`<source path>/<function>.txt` holds what the function decides with the mesh, on one line. A method
`Type::name` is the file `Type.name.txt`, so `crates/doc/src/eval.rs:blend_edges_on` is
`crates/doc/src/eval.rs/blend_edges_on.txt`.

`cargo xtask mesh-decisions` (xtask/src/mesh_decisions.rs) fails on a function that uses a mesh
outside display, export, previews and mesh bodies and has no file here, and on a file whose
function no longer does: a fix deletes its file in the same pull request. Never add a file: decide
with exact geometry (`Body::exact_bounds`, `kernel::apart`, the B-rep's faces and edges). These are
truck-era uses SMLib answers exactly. One file per function means two fixes never touch the same
file, so they never conflict.

The files marked "drawings branch only" are on the `drawings` branch alone and must be cleared
before it lands on main.
