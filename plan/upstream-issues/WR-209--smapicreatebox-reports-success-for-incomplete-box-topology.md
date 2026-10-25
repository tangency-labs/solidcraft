#### WR-209. `SmApiCreateBox` reports success for incomplete box topology
- **Severity:** Wrong result
- **Area:** Primitive construction
- **Location:** `source/SMLib/src/SmPrimitiveCreation.cpp:294-344` (`CreateBox`) and `source/SM_API/src/SmApiPrimitives.cpp:306-324` (`SmApiCreateBox`).
- **What happens:** `CreateBox` ignores all six `CreateFaceFromSurface` statuses. The API can return `SM_SUCCESS` with a non-null B-rep that lacks the expected six faces, twelve edges and eight vertices; callers cannot rely on that success status to select box faces.
- **Trigger:** Positive box dimensions `[1e-6, 1e-6, 1e-6]` at `[0, 0, 0]`, or `[1e-3, 1e-3, 1e-3]` at `[1e6, 1e6, 1e6]`, under the reference context's construction tolerances. Applying B-rep tolerance `1e-9` afterwards does not repair the failed construction.
- **How found:** adversarial review of `offset.offset_executive_p2` (PR #201), then fixed native fixtures `review_box_tiny_*` and `review_box_translated_*` in `fixtures/offset.offset_executive_p2/review_box_construction.json`.
- **Confirmed:** yes, Linux ARM64 C++ reference build at `c9c5979`; eight source/foreign and null/indexed-selector fixtures reject incomplete construction, while two ordinary-size controls construct the expected topology and reach the real API selection error.
- **Severity:** wrong result / hidden construction failure.
- **Rust port:** No box-construction parity is claimed by the input scaffold. The native fixture adapter checks actual topology counts immediately after construction and records an explicit constructor rejection, before selector resolution or offset/shell execution. It does not substitute a successful Rust shape or impose a size cutoff.
