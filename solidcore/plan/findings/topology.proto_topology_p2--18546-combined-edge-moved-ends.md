# A proto edge combined into one running the other way had its ends moved crosswise

`ProtoTopologyManager::split_intersecting_proto_edges` (`crates/solidcore/src/topology/proto_topology/p3.rs`, upstream `SmProtoTopologyManager::SplitIntersectingProtoEdges`) splits two proto edges that overlap along a piece, combines the second piece into the first with `combine_proto_edge` (`p2.rs`, `SmProtoEdge::CombineProtoEdge`) and removes the second with `remove_proto_edge`. When the overlap runs the other way (`same_dir == false`), the two pieces end at the same two proto vertices A and B in reverse order: the first from A to B, the second from B to A. `CombineProtoEdge` compares start with start and end with end, so it takes both ends as different. For each it unlists the second edge at its own end, points that end at the first edge's (start A, end B) and lists the first edge there once more. After both, A and B no longer list the second edge, and each lists the first twice. The removal then unlists the second edge at A and B again and fails: "the proto edge is not listed".

Upstream drops that status. With `ssi_proto_topology_statuses_pass_on` (fork bcc6f01, WR-662), which SolidCraft runs, it was the boolean's error (tangency-labs/solidcraft#18546: a r 0.06 notch cut into the top rim of SolidCraft's cone, radius 3 to 0.5 over 1000, crossing the rim 9.2e-5 rad past the seam).

![Three panels. 1: proto vertices A and B, proto edge PE1 from A to B and PE2 from B to A; A and B each list PE1 and PE2. 2: upstream's combine moves PE2's start to A and its end to B, unlists it at both and lists PE1 again; A and B each list PE1 twice, and removing PE2 at A finds it not listed. 3: with this change no end moves; A and B each list PE1 and PE2, and removing PE2 unlists it at both.](topology.proto_topology_p2--18546-combined-edge-moved-ends/moved-ends.png)

*The merged edge PE2 after the combine, upstream and now* (source `moved-ends.vectorcraft`, also `moved-ends.svg`).

## Measured

On main at 6ff467866f, the notch's centre turned about the axis so that its rim crossing lies `a0` rad from the seam (positive: before it), cut with every `UpstreamFixes` flag on, with every flag but `ssi_proto_topology_statuses_pass_on`, and with none:

| a0 | all flags | all but pass-on | none |
|---|---|---|---|
| 1e-3, 0, -1e-6, -1e-5, -3e-5, -3e-4, -1e-3 | valid, 5 / 9 / 6 (0: 5 / 8 / 5) | the same | the same, except -1e-6 (face test 29) and -1e-5 (empty) |
| -9.2e-5 (the issue), -1e-4 | status 1001, not listed | valid, 5 / 10 / 7 | valid, 5 / 10 / 7 |

Faces / edges / vertices. Only the two failing positions reach the overlap branch of `split_intersecting_proto_edges`, both with `same_dir == false`. There the curve where the notch's wall meets the cone crosses the seam at a slant of about 2e-4 rad and lies within tolerance of it along a piece of 0.027 (z 999.700 to 999.714 at the issue's position): that piece is one edge shared by both, which adds an edge and a vertex to the 9 and 6 of a crossing at one point. With the change all nine positions with all flags are valid solids with the counts of the middle column.

## The change

`combine_proto_edge` takes a second edge that runs from the first's end to its start as already ending at the first's proto vertices: no end moves, nothing is unlisted or listed again, and the removal that follows unlists the second edge at B and A, where it is listed. An end that is genuinely another proto vertex still moves as upstream, and its removal still fails there with the flag. With the flag on, every combine that built before moved no end (a moved end always failed the removal), so those results are unchanged; with the flag off, a backwards combine now leaves the first edge listed once at each end instead of twice.

The goldens of `topology.proto_topology_p2`, whose random scripts call `combine_proto_edge` directly with the flag off, move in exactly the cases that make such a backwards combine (see the pull request). Tests: `topology::proto_topology::p2::tests::a_proto_edge_combined_into_one_running_either_way_is_removed`, and in `crates/solidcore-ops/tests/boolean.rs` `a_round_notch_overlapping_a_cones_seam_backwards_is_cut` (one valid solid of 5 / 10 / 7 and the frustum's volume less the notch's within 1e-9, the notch integrated over z from the two circles' lens area).
