# topology.edgeuse_p1: arena-named B-reps owned new UV trim curves under a second identity (2026-10-30)

**Unit:** `topology.edgeuse_p1` (`edgeuse::p1::owner_ref`, reached through `owner_ref_in`
from `set_uv_trim_curve`).

**Upstream:** `SmEdgeuse::SetUVTrimCurve` (`source/SMLib/src/SmEdgeuse.cpp`, v1.0.1 line
4552) gives every attached curve one owner, the edge use holding it:
`pUVTrimCurve->SetOwner((SmEdgeuse *)this)`. `SmEdgeuse::AssertValid` test #6 (line
6126, "UVTrimCurve->Owner pointer must equal this Edgeuse pointer") checks exactly that
pointer.

**What was wrong.** The port names an owner by an `ObjectRef` under the B-rep's
`NamingScheme`. Under `NamingScheme::Arena`, `owner_ref` named an edge use by its raw
arena index, `(SmEdgeuse_TYPE, index)`. Everything else names an arena B-rep's edge uses
by their notification identity, `loopuse::notify_ref` (`(SmEdgeuse_TYPE, 1 + (7 << 32) +
index)`):
- a B-rep built under the notify scheme and then arena-named keeps its trims under
  `notify_ref` (`Brep::use_arena_naming` relabelled only faces and edges);
- `check_brep` under `Arena` checks edge-use owners against `notify_ref` (`arena()` in
  `assert_valid` falls back to it for edge uses);
- the merge's `adopt_namespace_owners` re-owns merged trims under `notify_ref` for both
  schemes.

So on an arena-named B-rep, every trim an operation attached afterwards
(`CreateUVTrimCurves`, `FixBackwards`, `FixTiny`, `SplitAtSeams`, ...) got a second
identity:
- `check_brep` reported it as SmEdgeuse test #6 ("no edge-use owner");
- `owner_ref_in` comparisons (`set_uv_trim_curve`'s holder test, fillet
  `topology_objects.rs` linking) saw the reader's own trims as not owned by their edge
  use.

A body the Rust reader reads (`make_topology_from_data`, `NamingScheme::Caller`) and then
arena-names was worse off: its trims kept the reader's caller keys, which are neither
identity, so by the code `check_brep` reports test #6 on every one of them (the
regression test's first check).

**Fix.**
- `owner_ref(NamingScheme::Arena, eu)` returns `notify_ref(Edgeuse(eu))`, the same as
  `Notify`.
- `Brep::use_arena_naming` also rebinds each edge use's own UV trim curve to that
  identity when the curve's owner is the edge use's notification identity or its
  recorded caller key (`geometry_owner_key`), as it already does for faces and edges.
  Shared or foreign owners are kept.

The same PR gives `NamingScheme::Unknown` that identity too
(`plan/findings/topology.edgeuse_p1--uv-trim-owner-under-unknown-naming.md`): one rule,
one key, for every scheme but `Caller`.

**Why it was invisible.**
- No difftest output carried an edge use's UV trim owner. The runners exported the C++
  owner, but each Rust side printed the holder's own index (or `-1`) without reading the
  curve's owner. The owner-export work replaced that with the real owner
  (`own_builder::uv_owner_index`).
- `check_brep` runs on few arena bodies after an operation that attaches new trims.
- The analysis of 2026-10-29 that called this path unreachable checked only library
  callers of `check_brep`. It missed that the Rust reader (`own_reader`) and the merge
  runners arena-name the bodies that later operations edit.

**Regression test:** `crates/solidcore/tests/arena_uv_trim_owner.rs`:
1. Reads `Solids/box.smb` with the Rust reader and arena-names it, and asserts the
   level-2 `check_brep` reports no SmEdgeuse test #6 (the reader's trims rebound).
2. Replaces every UV trim through `set_uv_trim_curve` with an unowned copy
   (`Curve::clone`, the copy constructor, drops the owner). `SetUVTrimCurve` stores it
   on the `SAME` member of the mated pair, possibly the mate of the slot the reader
   filled (as upstream).
3. Asserts every new trim has an owner and that the level-2 `check_brep` again reports
   no SmEdgeuse test #6, which compares each owner with its holder's identity.
