# topology: `Brep::clone` drops every geometry owner, and owner-based kernel calls then skip faces and edges silently (2026-10-30)

- **Unit:** `topology` (`impl Clone for Brep`, `crates/solidcore/src/topology.rs`). The consumers are the topology solver, `brep::p1::surfaces` / `curves`, `brep_data_write` (`FromBrep`), the B-rep and ray-fire caches, the fillet merge and geometry transforms.
- **What `Clone` does:**
  - It copies the arenas. `Face` and `Edge` derive `Clone`, and `impl Clone for Surface` / `Curve` is the geometry's copy constructor, which sets the owner to `None`.
  - So no curve or surface of a clone names its edge or face. The B-rep's naming and `geometry_owner_keys` are copied, so `Brep::owner_identity` still answers. But the geometry no longer carries that owner.
  - This is documented on the impl and is deliberate (it is the copy constructor's semantics).
- **What goes wrong, silently (no error):**
  - **Topology solves** over a clone, for example `face_closest_point_in_shape` over a bare `query::brep::LiveBrepObjects`. `solve_point_face` resolves the surface's face through its owner (`TopologyObjects::surface_face`). That gives `None`, so `in_shape(None)` returns early (`intersect/topology_solver/p1.rs`, `solve_point_face`). `solve_point_edge` goes the same way through `curve_edge`. Only the shape's vertex candidates are left. On a 10 × 20 × 30 `create_box`, the centre of every face was answered by a box corner (11.18 or 32.02 off, not 0). This is how the SolidCraft API crate's pick helper (#1823, fixer-7's `diag_face_solve`) ran into it.
  - **`GetSurfaces` / `GetCurves`** (`brep::p1::surfaces` / `curves`) and the `.smb` writer (`brep_data_write`'s `FromBrep`, which writes the surfaces `surface_owners` lists) leave out an ownerless surface or curve. A clone is written with faces that have no surface. The crate's F1b export (fixer-7) hit this on a `.clone()`.
  - **B-rep caches and ray-fire caches** built from a clone have no curve or surface in their trees. `fillet_edges` of a clone fails in its merge (#1144). Transforms of a body's geometry that follow the owners miss it too.
- **Evidence:**
  - `crates/solidcore/tests/face_closest_point_live_box.rs` (#1841), `a_clone_answers_no_face_and_an_api_copy_answers_every_centre`. The original answers all 6 face centres at 0; `b.clone()` answers 0 of 6; an `api_brep_copy` of the box answers all 6.
  - The kernel solve itself agrees with pinned C++ on the original (`query.api_queries` box face-centre cases, adapter and bare live view, #1841).
  - C++ behaves the same for an ownerless surface (`GetFace()` NULL: the face is not in the shape). So this is a Rust API trap, not a port divergence: `SmBrep`'s copy constructor (`SmApiBrepCopy`) is `api_brep_copy`, not `Clone`.
- **Safe alternatives:**
  - `brep::p3::api_brep_copy` (`SmApiBrepCopy`) gives an independent body that is queried or edited later. The copy owns its geometry.
  - `Brep::clone_standing_for_self` gives a private read-only stand-in for the same B-rep. Its curves and surfaces keep the original's owners. It is not an independent body.
- **Status:** the rustdoc on `impl Clone for Brep` now opens with a `# Warning` section that names these calls and the alternatives. `Clone`'s behaviour is unchanged. Whether a clone should keep its owners is an API decision taken to the owner (team-lead).
