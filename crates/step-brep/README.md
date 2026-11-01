# step-brep

Builds solidcore B-reps from the neutral `step-import` model.

```rust
let model = step_import::import(bytes, &step_import::Limits::default())?;
let bodies = model.bodies.iter().map(step_brep::build).collect::<Result<Vec<_>, _>>()?;
```

## Why healing is optional

`build` stitches the faces but does not heal. That is enough for simple planar bodies
and avoids paying the healer's cost on every body.

Curved, trimmed bodies need UV trim curves and seam topology before tessellation or
mass properties. `build_healed` runs `SmApiHealBrep` after stitching; callers should use
it only when the fast build cannot tessellate.

## Performance check

Measured against an 87-fixture AP203/AP214/AP242 STEP corpus, using the official
`step-import` reader plus this builder with 14 parallel jobs.

| path | exact | suspect | unsupported | timeout |
|---|---:|---:|---:|---:|
| fast build | 3 | 45 | 30 | 9 |
| heal every body | 3 | 37 | 22 | 25 |
| fast build, heal only on tessellation failure | 3 | 50 | 22 | 12 |

Healing every body fixes more curved bodies but increases the timeout count. Selective
healing keeps the build fast and only pays for the healer where the fast path cannot
tessellate.

The corpus is external to this repository. No STEP fixtures are checked in.

## Topology first

`build`, `build_healed` and `build_body` first build a body from the file's own
topology: its shared edges and vertices joined as the file joins them through the
kernel's `BrepConstructor`, without geometric stitching. A face whose construction
fails is left out and reported (`ImportReport.face_failures`), the first failing face in
file order each time, up to 16 faces or 1% of the body; a face whose seam fix or UV trim
fails is kept as built and reported too. The stitched build runs as well. The topology
build is kept when it is a closed solid (manifold, display-ready, positive volume),
unless the stitched build is also closed and their volumes differ by more than 1e-4
relative: then the topology build healed before its seam fix is kept if it agrees with
the stitched one, else the first topology build. A topology build that is not closed
gives way to the stitched build, and where stitching fails too the open topology build
is kept with its left-out faces reported. `BuildMode::Strict` returns the stitched
build's error instead, and `BuildMode::StitchOnly` builds the old way only. The gate is
`tests/topology_first.rs` (`corpus`, `one_file`, both ignored).
