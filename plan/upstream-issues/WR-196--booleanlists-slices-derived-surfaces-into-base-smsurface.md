#### WR-196. `BooleanLists` slices derived surfaces into base `SmSurface` objects
- **Severity:** Wrong result
- **Area:**

- **Unit:** `boolean.merge_p3` (PR #245).
- **Location:** `source/SMLib/src/SmMerge.cpp`, `SmMerge::BooleanLists`, both input surface-copy loops and the final output surface-copy loop; `source/SMLib/src/SmSurface.cpp:731` (base copy constructor), `source/SMLib/inc/SmSurface.h:1096` (base natural domain).
- **Trigger:** a nonempty surface list without a preceding B-rep list. The function uses `new (*pContext) SmSurface(*pOrig)` rather than virtual `Copy`. This creates a concrete base object, discards the derived geometry, returns the base's zero natural domain, and makes the subsequent `IsKindOf(SmPlane_TYPE)` false. The intended planar dispatch therefore cannot be selected by these copied derived inputs. The output loop also constructs base-only copies.
- **How found:** source audit against the base constructor, base `GetNaturalUVDomain`, and the `SM_COMMON` virtual type methods while porting the list branches.
- **Confirmed: no.** No C++ reproduction was run; this entry records the source-level slicing behavior and the mismatch with the function's stated planar intent.
- **Rust handling:** the current `SurfaceKind` has no base-only variant. `boolean::merge::p3::copy_base_surface` defers that missing constructor branch specifically to the partial `surface.surface_p1` owner. It does not silently preserve derived geometry by invoking virtual `Surface::copy`; all subsequent list logic remains ported.
