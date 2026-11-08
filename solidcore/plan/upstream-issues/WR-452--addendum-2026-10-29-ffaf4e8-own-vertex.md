Updates: WR-452

The end fit under `Kernel::fixes.imprint_trims_edge_to_proto_vertex` now carries the fork's correction tangency-labs/smlib ffaf4e8 (WR-498, tangency-labs/solidcore#1480): an end goes to its proto vertex's position only when that proto vertex is one vertex in each B-rep, else to its own final vertex (or is left alone). No new flag: ffaf4e8 corrects the commit this record ports. The tests named above still pass.
