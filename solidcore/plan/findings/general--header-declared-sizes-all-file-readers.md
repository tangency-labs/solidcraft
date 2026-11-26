# Header-declared sizes (all file readers)

Upstream readers (`SmBSplineSurface::ReadFromDB` and friends) `malloc` whatever counts
a file header declares and fail later at end of data; ASCII readers even keep going
with stale values after a missing field. In Rust a huge allocation aborts the process
(and `n + p + 1` can overflow), so readers bound counts against the bytes left via
`DatabaseReader::ensure_available` (binary: 8 bytes per double; ASCII: 1 byte per
item) after the same validation checks upstream makes first (e.g. the rational flag).
Deviation: an ASCII file whose header declares more items than it has bytes left is
an error in Rust, while upstream reads it with garbage values. No fixture exercises
this; found by the commit security review of `nurbs.b_spline_surface_p1`.
