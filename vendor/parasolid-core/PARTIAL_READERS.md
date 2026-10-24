# Partial embedded-record readers

`parasolid_core::partial` shares the existing cadmpeg/sldkit bounded readers for
already extracted Parasolid body slices. It has no runtime dependencies and no
SolidWorks container, CadIr, Python, configuration selection, or filesystem API.

This API recognizes known compact records; it is not a complete X_B parser.
`parse_xb` and `brep::map_xb_brep` remain the schema-aware document path. Unknown
base types 3/4 in the observed deltas are still unsupported there. Calling the
partial API never upgrades that status or establishes final saved state.
The document parser's exact-key profiles and processing stages are summarized
in the [shared support matrix](https://github.com/monozukuri-ai/parasolid-kit/blob/main/docs/format-support.md#supported-profiles).
Embedded schema edits change field layouts; their implementation does not
establish model-state delta application. sldkit supplies the native container,
configuration selection and unit conversion around these shared readers.

## Contracts

- Topology retains wire IDs, byte offsets, read spans, and the inherited bounded
  partition/deltas merge: preserve existing topology records, admit missing
  referenced records, and apply recognized point updates. It does not implement
  arbitrary deletes, replacement topology, or undocumented delta opcodes.
- Native BODY/REGION/SHELL ownership and FIN normalization retain the exact
  `SCH_3701229_37102_13006` and `SCH_3601228_36001_13006` admission and link
  consistency checks. After hierarchy recovery, `native_fin::retain_face_fins`
  selects the current face loops and their opposite boundary dummies before
  normalization, excluding unreachable saved-journal FINs. Missing records,
  shared loops, cyclic ownership, or broken opposite links fail without changing
  the table. This selection does not replay journal topology updates. Incomplete
  opposite/radial/vertex evidence withholds normalized FIN topology.
- Analytic and NURBS readers return source-unit parameters and Euclidean poles.
  Analytic records also retain the unmodified `+`/`-` orientation byte.
  Length conversion belongs to the caller; knots, angles, directions, weights,
  and surface UV coordinates are not length-scaled. The neutral partial types
  describe standalone carriers, not the complete native B-Rep graph.
  The strict document B-Rep instead retains source homogeneous NURBS
  coefficients; adapters must account for this representation difference.
- Sweep/spin, blend, offset, and subset readers expose known raw payloads.
  Intersection chart polylines are derived degree-one caches, not exact
  intersection definitions. Callers retain that distinction and may filter
  unrepresentable tolerances before chart selection.
- `ReadSpan` describes only successfully interpreted bytes, relative to the
  input body. NURBS descriptor and array spans can be disjoint. Uninstrumented
  families return no typed span. Callers supply physical stream identity,
  hashes, and the uninterpreted complement; carrier envelopes are not evidence
  that every enclosed byte was decoded.
- Existing shape-preserving point/NURBS patch helpers are shared as well. They
  do not provide a general serializer or an atomic multi-record transaction.
- Readers accept fragments without a schema key except where a specific profile
  is required. Successful signature recognition is not document-wide framing,
  supported-schema certification, or complete geometry recovery. Callers bound
  input sizes before invoking these compatibility readers.

## Origin and licenses

The original parasolid-core implementation remains MIT licensed (`LICENSE`).
The adopted readers and layout constants are Apache-2.0 licensed
(`LICENSE-APACHE-2.0`); the aggregate crate declares `MIT AND Apache-2.0`.

Upstream: [cadmpeg-codec-sldprt 0.5.3](https://crates.io/crates/cadmpeg-codec-sldprt/0.5.3),
[source repository](https://github.com/cadmpeg/cadmpeg/tree/v0.5.3).
The input was sldkit's `0.5.3+sldkit.3` patched source, including its exact read
ledger and validated native hierarchy/FIN changes. The upstream archive has no
NOTICE file. Apache license text is retained verbatim; per-file headers identify
adopted code. `view.rs` and module-level neutral records are new MIT code.

Modified for sharing in 0.1.0-dev6: removed CadIr types and millimetre conversion,
used the existing BinaryReader for scalar access, exposed bounded record/patch
APIs, transferred reader tests, checked knot multiplicity overflow and truncated
UV arrays, and kept unit-dependent acceptance in the adapter. Derived surface
construction, pcurve evaluation, source-file hashing and byte classification
remain outside these readers.

SHA-256 of the exact patched input files before extraction (whole-file hashes;
only the reader/layout portions of `brep.rs`, `layout.rs`, `sweep.rs`, and
`subset.rs` were adopted):

| Input file in cadmpeg-codec-sldprt | SHA-256 |
| --- | --- |
| `src/brep.rs` | `3f9facb3dcddb23cae1b5c500c3fa4108bb80e77e1fe070d5f0db36efa13efe9` |
| `src/layout.rs` | `db8cf43a503806a075ad9f9cd5e6854ed40d876262c235530644b1f21bd1fa04` |
| `src/brep/topology.rs` | `6b55de86f4447b35e5fa06ad1e7f803bc17cde2dd915cedb822a6242339ed0fe` |
| `src/brep/native_hierarchy.rs` | `e72ff9908a49e5582d75273bd1560e8908fdca366667c1f0064981cd995fe073` |
| `src/brep/native_hierarchy/tests.rs` | `ad87c422564698f08b45805275288997c53697fb2b84e84a1b21b3c6d4e60e8c` |
| `src/brep/native_fin.rs` | `5aafa3485bd4481b4bdf209e6ba572ee3b67a524381d3d227f69edd9952f73a7` |
| `src/brep/spline.rs` | `a6e07eb98e92807124cba347318e8f51ea95ba30f392855a0579fd04ff56dac7` |
| `src/brep/intersection.rs` | `965d6a467276423e5630d1826b1db6f55ea25fdef9a716abac74194003f37561` |
| `src/brep/sweep.rs` | `40a8cdd28c96d5276e437d4c4c69aac36680e066bed19db7b09740ea8771d1de` |
| `src/brep/offset.rs` | `844a7468dc3f78d2470d9962b14b82aa298a3d01901122a6d8ed6ff4c27d0828` |
| `src/brep/blend.rs` | `28a61d97a4ddb53d9b30033dbe1e15b608d22c919be60aaea0cc5918baaf6d91` |
| `src/brep/subset.rs` | `f1ee28f75d14b781cef3039733639f047de319011b6edf8f94a92d07048058ed` |

No proprietary CAD files, externally downloaded schema catalogs, or captured
SolidWorks artifacts are included in the crate. Public tests use synthetic data.
Controlled M5 and frozen TEST1/TEST2 comparisons are private sldkit integration
checks and are not claimed as a public golden corpus.
