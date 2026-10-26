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
