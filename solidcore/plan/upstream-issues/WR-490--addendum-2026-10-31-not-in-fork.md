Updates: WR-490

#### WR-490 (update, 2026-10-31). The fix is solidcore-only; the fork at 26cc451 does not have it

The record's fix was confirmed on a local patch over tangency-labs/smlib@5af7070; it was never committed to the fork. At tangency-labs/smlib@26cc451 `sm_GetEllipseTrimIntervals` (`source/SMLib/src/SmCone.cpp`, about lines 5455-5472) still takes the seam-line corners from `sAnalUVDomain.Evaluate(.., 0.0/1.0)`, the given domain's v range, with the end circles at that domain's v limits (as v1.0.1). `Kernel::fixes.ellipse_trim_seam_lines_span_cone_height` is therefore a solidcore-only fix; its doc says so. No behaviour changes.
