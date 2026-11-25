# boolean.merge_p5

- The wire/shell-vertex stage is tested directly through the real protected
  `SmMerge::ProcessWiresAndShellVertices`, without running the surrounding boolean
  engine. Raw fixture topology is exported before the stage; ids follow native
  B-rep edge/vertex owning-list order. The intersector's `Relate` call is fixture
  setup, including its tolerance propagation, on both sides.
- `SmallBox.smb` is a part container, not a B-rep stream. Feeding it to the shared
  `ReadFromFile` builder reconfirmed UB-1 (`SmTopologyData::ReadFromDB`, GDB).
  The new FFI wrapper rejects that header before C++; the solid operands use
  `box_with_large_fillet.smb`. This is a fixture-format exclusion, not a parity
  pass or a deferred kernel operation.
- Classification-dependent paths stop at `query.brep::SmBrep::Point3DClassify`.
  Their downstream topology deletion and copy paths remain uncertified, with
  explicit `topology.brep_p4` and `boolean.merge_p1` / `_p2` adapters. Arena B-reps
  do not yet carry an attached B-rep cache; optional cache-resizing integration
  remains for the merge engine's eventual cache binding.
- **PR #161 review:** a marked other wire without a mate is valid for Unknown,
  Difference, ExclusiveOr, Imprint, ImprintClassify, ExtractSeparate and Slice.
  The release C++ assertion does not reject it, and those operations never use
  the mate. Rust now looks up/validates mates only for Union, Intersection,
  Merge and PartialMerge. The shim excludes null keep-list entries only for
  those four operations because `Vec<EdgeId>` cannot represent them; appending
  null is defined in this upstream stage, rather than an immediate C++ crash.
  Seven differential regressions exercise the previously over-rejected inputs,
  and eleven more check nonidentity network mate order for every operation.
- Both runners validate one-to-one mate setup before tolerance propagation:
  duplicate primary targets (e.g. `[0,0,1,2,3]`) conflict in upstream `Relate`
  and are invalid fixture input. Generated cases now choose operands and mark
  masks independently, permute targets and leave edges unrelated where valid.
- The current borrowed relation maps and immutable other operand cannot express
  `MergeEdge` / `MergeVertex`'s `Relate` updates. When those adapters are wired,
  relation state must become mutable and preserve updates for later iterations;
  this integration requirement is also recorded in the module's Deviations.
  (PR #146: `MergeVertex`'s `Relate` is now wired to
  `Relations::relate_vertices`, and `merge_vertex` / `process_wires_and_shell_vertices`
  take the other operand mutably; it stays unreached behind `MakeShellVertex`,
  `topology.brep_p9`.)
  (PR #229: `MergeEdge` is wired to `boolean.merge_p2`'s `merge_edge` of each edge's
  primary edge use. `process_wires_and_shell_vertices` takes a `TopologyCopy` (edge
  and face notification identities around p1's `VertexCopy`) and snapshots the other
  operand's edge list before the loop, as upstream's `GetEdges`; the call stays
  unreached behind `Point3DClassify`.)
