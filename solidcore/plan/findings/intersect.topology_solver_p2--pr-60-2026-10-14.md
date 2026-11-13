# intersect.topology_solver_p2 (PR #60, 2026-10-14)

- **Scope.** All 15 owned functions have Rust bodies: the shared VertexEdge and
  EdgeEdge constructors reuse p1, followed by local traversal, six local solves,
  five public B-rep/shape entrypoints and the ordered tree-set helper. Traversal
  preserves list order, pair culling and the child-one subdivision test. The local
  solver state is restored on errors. No numerical tolerance has changed.
- **Default corpus.** 2,429 cases: 600 generated traversal cases, 2 traversal
  fixtures, 180 CurveVertex cases and 1,647 static/face/operation/request/tolerance/scale/subdivision
  cases. 2,384 compare exactly; zero fail; 45 defer (surface_p11 20, surface_p4 8,
  surface_p6 8, surface_p8 4, topology.brep_p1 2, topology.edge 3). Counts include
  rejected-operation and empty-shape cases, not only geometric solutions.
- **Concrete reference construction.** Static B-rep fixtures use real native
  `MakeShellVertex` bodies; face fixtures use `CreateFaceFromSurface`, and EdgeFace
  uses `CreateWireEdgeFromCurve`. The Rust object adapter independently supplies
  equivalent geometry and cache tree views. Native tolerances are also compared.
  These adapters do not port SmBrepCache or nonempty SmShape construction.
  Native preflight tests require construction to succeed even when a Rust case
  defers. Setup failure is a harness failure, never a deferred/pass result.
- **Point-tree storage.** The shared p1 tree fixture uses managed nodes on both
  sides. A separate native probe finds zero entries in `GetObjectList` for the
  external-node constructor and one for the managed-node constructor. The old
  inconsistent fixture could skip every C++ comparison in SolveTwoTrees.
- **Node metadata.** SmSolution has five node slots although the global traversal
  supports three trees. Static fixtures compare all five presence flags, never
  pointer addresses or dereferences of temporary native query-tree nodes.
  The added 1,080 static cases use 12/11 and 40/30 vertices, scales 1/1e3/1e6
  and tolerances 2/50 to exercise subdivision. Presence flags do not certify
  arbitrary multi-node B-rep node identities.
  Generated traversal cases use the shared p1 protocol.
- **Typed shape conversion.** Shape trees store VertexKey, EdgeKey and FaceKey.
  The topology adapter resolves these to vertices, edge CURVES and face SURFACES,
  as the native spatial lists do. Mapping the edge/face object itself is wrong.
  A regression distinguishes topology handles from geometry handles; tree mapping
  separately checks links, neighbors, node classification, ownership and errors.
  Nonempty Shape creation still requires unported topology queries.
- **Undefined EdgeVertex operand.** SmTopologySolver.cpp:1032 selects pObj2 as
  pCurve without swapping, or pObj1 when swapping: the SAME vertex selected just
  above. The cast then calls GetEdge on a vertex. The Rust port rejects that class
  with InvalidInput instead of fixing the operand. A native runtime-type probe
  verifies the selected vertex is not a curve without performing the invalid cast;
  a separate Rust test exercises both argument orders. No C++ SolveIt result is
  fabricated or counted as differential coverage of this undefined operation.
- **Remaining face paths.** Face exclusion is compared; included faces explicitly
  defer in surface/query callees. Coincidence classification still needs the p4
  curve-classification constructor overload. Range endpoints need
  `SmSurface::GlobalPointSolve` in surface_p4 on the SAME shared cache with point
  testing disabled around BOTH solves;
  an independent owned-cache copy would not suppress the solver's trimming.
  No unused independent cache is constructed before this explicit deferral.
- **Signed endpoint behavior.** Native B-rep/curve and shape/curve entrypoints
  currently test only the start (`i < 1`). Only the start parameter tuple is
  rewritten for appended point solutions; the port preserves these choices.
- **Back-edges.** p1's GlobalSolve local hook and six local-class dispatches now
  call p2. Its 87 former traversal deferrals clear: 3,248/3,532 compare exactly,
  with 284 remaining genuine foreign-callee deferrals. GeomTree compares
  611/618; GeomTools compares 721/913. Its remaining
  B-rep point-query marker names the missing topology.brep_cache adapter,
  rather than this now-ported unit.
- **Final validation.** Against current official main, all required
  local gates pass. Sensitivity detects every non-deferred case in all four
  affected units (2,384, 3,248, 611 and 721). Full validation evaluates 142 units:
  89 green, 53 partial, zero failed. Default reports match the full run byte for
  byte. Report paths and scope limits above remain authoritative.
- **Review regression.** A node whose auxiliary type is not ObjectList is empty
  in native LocalSolve, which ignores GetObjectList's status. The Rust traversal
  now preserves that behavior for both operands while retaining structural
  checks on actual object lists. Separate native and Rust tests verify success
  with zero solutions in both orders. Part-2 regressions live in p2; only their
  shared scene builder remains in p1's test module.
