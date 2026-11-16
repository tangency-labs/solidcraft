Updates: UB-169

Two `SM_API` wrappers reach uninitialised vectors through a normal whose squared length overflows. `SmVector3d::Unitize` succeeds on such a normal with a zero vector (`dLenSq` is infinite, so it divides by an infinite length).

- **`SmApiCreateSilhouetteCurves`** (`source/SM_API/src/SmApiBrep.cpp:1008`) passes the zero normal to `SmHCR(.., crViewVector, ..)`. This is the original UB-169: `MakeUnitOrthoVectors` fails, and the view transform is built from uninitialised `sPerp1` / `sPerp2`.
- **`SmApiProjectBrepOntoPlane`** (`:797`, a new location) calls `sPlaneNormal.MakeUnitOrthoVectors(0, x, y, z)` and ignores its status. For the zero normal, the `Unitize` of its X axis fails and it returns before writing `y` and `z`. These are `SmVector3d` locals left uninitialised by the defaulted constructor. `plane.SetCanonical(planeOrigin, y, z)` then reads them, and the plane feeds the `SmHCR`.
- **How found:** the nightly fuzz run 41190675541 (2026-10-31), unit `topology.api_projection`, six `differs` notes. In each case one normal component is `1e300` or `-1e300`; the other components are as in the base fixture:
  - `fuzz00010`: silhouette of `torus.smb`, normal `(1, 1e300, 0.1)`. The C++ returned 2 curves; the port returned `InvalidInput`.
  - `fuzz00083`: silhouette of `Solids/cylinder.smb`, normal `(1, 1e-300, 1e300)`.
  - `fuzz00271`: silhouette of `Solids/nurb_fillet.smb`, normal `(0.028, 0.862, -1e300)`.
  - `fuzz00023` and `fuzz00293`: silhouettes of `Solids/sphere.smb`. The C++ succeeded with no curves.
  - `fuzz00002`: `SmApiProjectBrepOntoPlane` of a box, normal `(-1e300, 0, 1)`. The C++ returned `SM_ERR`, the port `SM_ERR_INVALID_INPUT`.
- **Confirmed:** yes, by reading: the zero normal reaches both reads above. The C++ results depend on stack contents.
- **Harness:** the `topology.api_projection` FFI refuses a normal whose squared length is infinite for ops 1 and 2 (`normal_overflows`), and the runner's Rust side refuses it too. `blend.hcr`'s runner already refuses such view vectors.
