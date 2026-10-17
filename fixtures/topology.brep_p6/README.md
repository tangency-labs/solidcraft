# Closure traversal fixture

`open_box.smb` is ASCII SMLib topology generated from the Apache-2.0 input
`upstream/TestFiles/pt_TestFiles/Solids/box.smb` (upstream `c9c5979`). It is an
actual five-face sheet, not an operation-result snapshot. The native file reader
constructs it afresh on both sides; Rust imports only its raw input graph.

Generation used the reference kernel and the shared fixture builder:

```cpp
const double input[] = {0, 0, 0, 0}; // file, no rebuild, no validation, no gaps
Rd rd{input, 0};
Built b;
SmStatus status;
b.brep = build(rd, "upstream/TestFiles/pt_TestFiles/Solids/box.smb", status);
// Check b.brep and status == SM_SUCCESS.
b.brep->m_bEditingEnabled = TRUE;
Graph g;
walk(b.brep, g, true);
status = b.brep->DeleteFace(g.faces.at(0)); // Check SM_SUCCESS.
status = b.brep->WriteToFile("open_box.smb"); // Check SM_SUCCESS.
```

The exported graph has one region, one shell, five faces and ten faceuses. Seeds
0 and 5 are the two sides of the face opposite the hole. Both mates share a shell
and neither seed face has a lamina edge. Traversal reaches the hole through the
neighboring faces and returns open, before allocation or missing dependencies.
For one call with no caller lock, C++ advances MARK by four and releases its lock;
two calls advance it by eight. The 56 cases compare both seeds, seven caller-lock
masks, one/two calls and both infinite-region flags, including all faceuse marks
and lock state. The native FFI regression checks that these inputs still reach
traversal rather than an earlier no-op return.
