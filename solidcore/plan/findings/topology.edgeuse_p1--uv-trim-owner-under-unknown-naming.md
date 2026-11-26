# topology.edgeuse_p1: SetUVTrimCurve leaves the new curve without owner under the default naming (2026-10-30)

**Unit:** `topology.edgeuse_p1` (`edgeuse::p1::set_uv_trim_curve`, `owner_ref`,
`owner_ref_in`). Seen through `topology.face_p3` and `topology.trimming_tools` once their
shims exported each UV trim curve's owner.

**Upstream.** `SmEdgeuse::SetUVTrimCurve` always calls `pUVTrimCurve->SetOwner(eu)` on
the holder (the `SAME` member of the mated pair, or the edge use itself without a mate).
No caller decides whether the curve has an owner.

**Rust before the fix.** `set_uv_trim_curve` names the owner through
`owner_ref_in(b, holder)`, which depends on the B-rep's `NamingScheme`.
`NamingScheme::Unknown` (the `Default`) returned `None`, so the curve stayed
without owner. Most public B-rep constructors record a scheme:

| Constructor | Naming |
|---|---|
| `brep::p3::new_brep` (`new SmBrep()` with tolerances) and so the primitives | `Notify` |
| `api_files::read_brep_from_file`, `make_topology_from_data` | `Caller` |
| booleans (`boolean::merge::p3`) | `Notify` |
| `brep::p3::copy_brep` | `Caller` |
| `Brep::new`, `Brep::new_with_infinite_region` (documented as `SmBrep()`), `Brep::default` | **`Unknown`** |

A caller that builds on `Brep::new_with_infinite_region()` therefore gets UV trim
curves with no owner from every later `SetUVTrimCurve` call: `CreateUVTrimCurves`, the
trimming fixes, `SplitAtSeams`, and so on. Upstream never produces such a curve. The
difftest harness hit this because `own_builder::renumber_with` builds its B-rep with
`Brep::new()`:

- `topology.face_p3` (652 values) and `topology.trimming_tools` (702 values):
  `out[...]: cpp=0 rust=-1`, where the C++ value is the holder's edge use index and the
  Rust curve had no owner.

**Fix.** `owner_ref(NamingScheme::Unknown, eu)` returns what `NamingScheme::Arena`
returns for an edge use: its notification identity `(SmEdgeuse_TYPE, 1 + (7 << 32) +
index)` (`loopuse::notify_ref`), the identity the arena scheme uses for edge uses in the
same library PR (`use_arena_naming`, `check_brep`, `adopt_namespace_owners`). Faces and
edges already fall back to arena identities under `Unknown` (`brep::p1::default_face_owner` /
`default_edge_owner`), so `Unknown` now behaves like `Arena`. Other geometry under `Unknown` is
unchanged. Test: `crates/solidcore/tests/uv_trim_owner_default_naming.rs` builds a
plane face on `Brep::new_with_infinite_region()`, deletes and re-creates its UV trim
curves with `create_uv_trim_curves`, and checks that each curve's owner is its holder's
notification identity.

**Effect beyond these runners.** Only B-reps under `Unknown` change. Their new UV
trim curves now have owners where they had none before. Code that looks UV trim owners up
across B-reps (`boolean::merge::p4::arena_pair::owner_of`,
`offset::offset_executive::edgeuse`, `topology::face::p3`) returns `None` when two
B-reps' curves have equal owners. Two `Unknown` B-reps can now carry equal
owners, as two `Arena` or `Notify` B-reps already could: a notification key names an
edge use by its kind and arena index only and carries no B-rep identity, so the exposure
is the one the `Arena` and `Notify` schemes have today, not a new one. A full
`--green` run is required before merging.

**Open: `Caller` without a recorded key.** Under `NamingScheme::Caller`,
`owner_ref_in` returns the key recorded for the edge use. Only
`brep::p10` records edge use keys: at the build, and in `bind_uv_trim_owners` after
lazy face queries and edge splits. An edge use that another operation creates later in a
read B-rep has no key, so `SetUVTrimCurve` on it still leaves the curve without owner.
That case is left unchanged on purpose. An arena fallback there would be set before `bind_uv_trim_owners`
runs, which keeps existing owners, so it would no longer bind the caller's key and
the p10 runners that compare caller keys would change. It needs its own fix: record a key
for each edge use an Euler operator creates under `Caller`, or fall back and let
`bind_uv_trim_owners` replace the fallback owner.
