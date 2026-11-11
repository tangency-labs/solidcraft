Updates: WR-490

#### WR-490 (update, 2026-11-02). The record's boolean triggers are right in v1.0.1

The 2026-10-27 addendum left the record's boolean triggers unchecked against the C++ 1.0.1. They are now checked, on the pinned build (x86-64 Linux, through the `boolean.api_brep` runner):

- **Equal rods:** `(C u A) u B`, `(C u B) u A` and `C u (A u B)` of #868's rods give the whole body at seams 0, 35, 60 and 90 deg. That is the volume of #868's closed form (3137849.37), a manifold solid, and `AssertValid` passes. At the old pin, seams 0, 35 and 60 gave `C u A`.
- **Tubes:** `(chord + A) + B` of the steel-026 tubes, built from the same cylinders, gives the closed form's volume (779093.25) and a manifold solid at every seam. At the old pin it was refused (-1100), or 4592 mm^3 too large at seam 90.

The Rust default agrees with the C++ within 1e-9 in every case. `Kernel::fixes.ellipse_trim_seam_lines_span_cone_height` gives the default's body, within 1.5e-11. The table and the cases are in `plan/findings/general--flag-pin-wr490-ellipse-trim-same-since-v1-0-1--addendum-2026-11-02-boolean-triggers.md`.

Two failures remain on both engines:
- The tubes' check still reports `SmBrep` test 12 (an inner shell not inside its outer shell).
- Joining the braces coped to the chord's solid fails at seams 35 and 90.

Neither changes with the record's fix. Neither is traced.
