# boolean.api_brep: tiny-box Booleans reach the UB-175 teardown once edge definitions imprint into faces

Unit `boolean.api_brep`, cases `gen0059` (`with_options`, operation 3, boxes of
size about 0.005 at (-0.0066, -0.0183, -0.0027) and (-0.0025, -0.0203, -0.0025)) and
`gen0066` (`merge_breps`, operation 2, boxes of size about 0.008 and 0.004). Until
`SmEdgeDefinition::Imprint` could run `SmBrep::MakeEdgeInFace` on the owning B-rep
these cases were deferred there. Both now run to the end of
`SmTopologyIntersector::IntersectInsertRelate`, where the port's UB-175 check
(`plan/upstream-issues.md`: `~SmProtoTopologyManager` reads a freed proto edge) finds
a proto vertex that still lists a freed proto edge: the proto vertex capture distance
is at least 0.01 (`SmTopologyIntersector.cpp:1159`), larger than the boxes, so the
intersection points collapse into shared proto vertices and some proto edges start
and end on the same one. The port returns `SM_ERR_INVALID_INPUT` there
(`gen0059` status 1007; in `gen0066` `merge_breps` reports it as `SM_ERR` 1001);
the C++ reads the freed memory and goes on (status 0, 8 and 7 result records). The
C++ build cannot be instrumented here, so no input-based rule separates these from
tiny boxes that do not reach the condition from the inputs alone, so the
`boolean.api_brep` runner rejects a case on the Rust side when the port reports exactly
this UB-175 error (the way `primitive.api_primitives` rejects UB-119): the C++ is not
run, because the input is undefined there. For that the merge's `BooleanTrees` now
passes an `InvalidInput` from `ManifoldBoolean` through instead of turning it into
`SM_ERR`.
