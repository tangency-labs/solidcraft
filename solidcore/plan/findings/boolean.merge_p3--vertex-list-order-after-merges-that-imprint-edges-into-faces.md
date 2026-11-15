# boolean.merge_p3: vertex list order after merges that imprint edges into faces

Unit `boolean.merge_p3`, cases `upstream_merge_breps_operation_3` (`merge_breps`,
operation 3, boxes (0,0,0)+10 and (8,6,3)+(4,8,6)), `upstream_piecewise_boxes_2_keep_other`
and `upstream_piecewise_boxes_3_keep_other` (`PiecewiseMerge` with
`bCreateNewBrepsForFaces`, boxes (0,0,0)+10 and (5,0,0)+10). Deferred at
`SmBrep::MakeEdgeInFace` before; now both sides finish with the same counts
(18/36/22 and 10/20/12 faces/edges/vertices) and the same vertex positions, in a
different vertex list order. C++ lists the vertices that belong only to the part of
one operand on the far side of the other (the corners (8,6,3) and (8,6,9) inside the
first box; the x = 0 corners of the first box in the piecewise cases) after all the
others, as if that part's vertices were moved out of the B-rep and back in; the port
keeps them in their original places (`values.record[18..]` and `[8..]`). The binormal
classification of `UpdateFaces` is not involved (the merge case never evaluates one;
in the piecewise cases every evaluation succeeds). Suspected cause: a step after the
imprint in the merge editing stages that moves a region's or shell's topology between
B-reps upstream and keeps it in place in the port. Follow-up: trace which step moves the
far-side part's topology out of the B-rep and back in upstream. Listed in
`known_mismatches.toml`.
