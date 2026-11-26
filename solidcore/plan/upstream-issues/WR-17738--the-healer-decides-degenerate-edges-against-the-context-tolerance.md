#### WR-17738. The healer decides a degenerate edge by samples against the context's tolerance, not the edge's, and squeezes it without re-ending its neighbours
- **Severity:** Wrong result (a valid edge removed, and the edges at its removed vertex left
  ending off their vertex)
- **Area:** Healing (`HealData` degenerate-edge step)
- **Location:** `source/SMLib/src/SmEdgeProps.cpp`, `SmEdgeProps::SetProps_Metadata`, which sets
  `m_bBadDegenEdge` from `SmCurve::IsDegenerate(GetApproxTol3d(), interval)`;
  `source/SMLib/src/SmHealData.cpp`, `SmHealData::Fix_DegenEdges`, which calls
  `SqueezeEdge(edge, edge->GetVertex())` (solidcore `heal/heal_data/p2.rs`
  `EdgeProps::set_props_metadata`, `heal/heal_data/p3.rs` `HealData::fix_degen_edges`).
- **What happens:** `IsDegenerate` compares the curve's end points and its first derivatives at
  three parameters with the context's approximation tolerance, half the context zone tolerance
  (5e-6 at the default 1e-5), whatever the edge's own tolerance. A body whose size class gives
  it a smaller tolerance (1e-6 from 0.005 to 0.5) loses every edge shorter than 5e-6, though
  such an edge is longer than its tolerance and valid (`SmBrep` Edge test 7). An edge shorter
  than its own, larger tolerance is not degenerate. The squeeze keeps the edge's start vertex
  where it is and deletes the other, and the curves of the edges that met the deleted vertex
  still end at its point, the edge's length away.

  ![Three panels. (a) Sampled: a ridge between two vertices, shorter than a bar for the context's approximation tolerance 5e-6, is called degenerate and squeezed into one end. (b) Certified: the same ridge with a ball of the edge's tolerance at each vertex; the balls do not meet, so the ridge is a real edge and is kept. (c) Certified: a short edge inside a ball of the edge's tolerance around its midpoint M is squeezed into M, and the two neighbouring edges are re-ended at M.](WR-17738--degenerate-edge-test.svg)

  The sampled test reads one tolerance for every body; the certified one reads the edge's own.

- **Trigger:** a block 0.1 wide on a square base whose top is a ridge 4.5e-6 long, read from a
  record with tolerances of 1e-6 (zone 5e-7) and healed: the healer changes the record's
  topology (5 faces, 9 edges, 6 vertices), so the body is not the record's.
- **How found:** #17738 (the certified short-edge detector of #17708, meant to replace this
  step's), reduced to the ridge block in triage.
- **Confirmed:** yes, in the port, which kept upstream's test and squeeze:
  `heal::the_healer_keeps_an_edge_longer_than_its_tolerance` (solidcore-ops) fails on main at
  2cc3e19431 with "the healer changed the record's topology", and
  `heal::the_healer_squeezes_an_edge_within_its_tolerance_and_re_ends_its_neighbours` lists no
  degenerate edge where the hair step's two 3e-4 riser ends lie within their 5e-4 tolerance.
  The pinned C++ was not run.
- **Fix in solidcore:** an edge is degenerate when its vertices are within its tolerance of each
  other and its whole curve is certified within its tolerance of their midpoint
  (`heal::tolerance_defects::short_edge_bound`; a closed edge, of its vertex); the step decides
  each listed edge again with its tolerance then and squeezes it with
  `heal::tolerance_defects::collapse_short_edge`, which moves the vertex kept to the midpoint and
  re-ends every neighbour there, certified.
