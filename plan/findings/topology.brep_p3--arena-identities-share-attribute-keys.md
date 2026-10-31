# topology.brep_p3: api_brep_copy loses attributes under shared or arena namings (2026-10-30)

**Unit:** `topology.brep_p3` (`api_brep_copy`), `topology.brep_p2` (`seed_brep_attributes`,
`restore_keyed_attributes`). Port-only: upstream names objects by their addresses, so
there is no C++ counterpart and no upstream issue.

**Inputs:** a 10 mm `create_box`, with a `Copy` long attribute on each of its 6 faces and
12 edges. It is copied with `p3::api_brep_copy`; the copy is named (`env.key_of`) by a
fresh `Brep::new().owner_identity`. Reported by tdamsma on tangency-labs/solidcore#1823
(the since-removed `solidcore-smlib-api` crate's copy).

**Values:** expected 6 faces and 12 edges tagged on the copy; actual `(0, 4)`, with no error.

**Cause:**
- `brep_cache::arena_identity` keys an edge, a face and a vertex by their raw arena index;
  the kind is only in `type_code`. The B-rep itself gets key 0.
- `scheme_owner_identity`, and so `Brep::owner_identity`, uses those identities under
  every naming scheme but `Notify`. That includes `Unknown`, the scheme of `Brep::new()`.
- `AttributeStore` and the topology records key by `ObjectKey` alone. So face *i*, edge
  *i* and vertex *i* of the copy share one attribute list.
- `restore_keyed_attributes` named the copy's objects with
  `BTreeMap<ObjectKey, EntityRef>` over faces, then edges, then vertices, so the last
  object of a key won. On a box, keys 0..7 went to the vertices and keys 8..11 to edges
  8..11. No face kept a list, 4 edges did, and the merged lists of the others landed on
  the vertices.
- The same naming as a copy's `source_key` would pass a face's list to the copy of the
  edge (and vertex) of its index.

**Second defect, shared namings:**
- When the source and the copy are both named by `loopuse::notify_ref` (the default
  `ArenaNotifyState::env` naming plus the source's own `notify_ref` or `owner_identity`),
  the copy's tagged faces and edges come out as `(0, 8)` instead of `(6, 12)`. Measured
  in batch21.
- Cause: the copy is built with the source's arena ids, so its keys are the source's
  keys. Copy face *i*'s list is source face *i*'s lent list. After the copy,
  `api_brep_copy` dropped every handle that list gained after the lend
  (`skip(before)`), and that included the copy's own new handles. The records are
  shared the same way.
- Fix: `api_brep_copy` checks whether the source naming meets the copy's on the
  source's live objects (`private_source_shift`). If so, it names the source in a
  private domain for the copy: the first multiple of `1 << 44` that clears every copy
  key and stays below the reserved range. The caller's records of the source are
  copied there and dropped after.
- The lend is now taken back by its exact handles.
- C++ parity: the `topology.brep_p3` runner's `attributed_copy` cases gain
  `shared_naming` variants (`fixtures/topology.brep_p3/attributed_copy_shared_naming.json`,
  160 cases). The C++ copy keeps every attribute, as the disjoint-naming `#1462` cases
  already show.

**Not affected:** the B-rep's own store.
- `Brep::add_attribute` and `find_attribute` key by `loopuse::notify_ref`, which is
  disjoint across kinds (`1 + kind << 32 + index`).
- `ArenaNotifyState::env` and the merge's `source_ref` use `notify_ref` too.

**Resolution:**
- `p2::distinct_listed_keys` refuses, with `InvalidInput`, a naming that gives two live
  objects the key of an attribute list.
- `restore_keyed_attributes` checks the copy's naming. `api_brep_copy` checks the
  source's before anything changes. `seed_brep_attributes` refuses two source lists
  named to one key.
- `Brep::owner_identity` documents that it is a geometry-owner identity, never an
  attribute or notification key.
- Tests: `crates/solidcore/tests/copy_keeps_attributes_default_naming.rs`.
