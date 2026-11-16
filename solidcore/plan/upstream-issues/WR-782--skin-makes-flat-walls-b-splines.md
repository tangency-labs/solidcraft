#### WR-782. The skin makes its flat walls B-splines
- **Severity:** Missing feature (the shape is right; its walls are not recognised as planes)
- **Area:** Sweep
- **Location:** `SmApiCreateSkinPrimitive` (`SmPrimitiveCreation::CreateSkinPrimitive`); solidcore `sweep/api_surfaces.rs` `skin_primitive`.
- **What happens:** every side wall of a skin is a B-spline surface, also where it is flat. A loft
  through three or more sections whose corners lie on straight lines, or one whose sections mix
  lines with arcs, has walls that are planar trapezoids. They come out as quadratic, cubic or
  bilinear B-splines whose control points all lie in one plane. Anything that asks a face for its
  plane (`IsNurbSurfacePlane` needs perpendicular corner derivatives, which a trapezoid lacks)
  finds none: sheet-metal conversion refuses them, offsets and drafts treat them as free-form.
- **Trigger:** three squares of half side 55, 77.5 and 100 at z 0, 60 and 120, skinned at
  degree 2: four B-spline walls, each flat.
- **How found:** solidcraft#13331 (Convert to Sheet Metal refuses a lofted duct).
- **Fix (always on, no flag):** after the skin, each side wall
  whose surface `IsPlanar` at `SM_EFF_ZERO` (`plane_of`) takes that plane through
  `ReplaceSurface` with `bCreateUVTrimCurves`. The plane's axes put its natural normal on the
  B-spline's side at the middle of its domain, so the faceuses keep their orientation. A wall
  with no clear side, or whose replacement fails, keeps its B-spline (the replacement is rolled
  back). `sweep/skin_flat_walls.rs`.
- **Confirmed:** no C++ run; solidcore-native, no fork commit.
