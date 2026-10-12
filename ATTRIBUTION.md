# Attribution

solidcore is a Rust port of **SMLib**, the NURBS B-rep solid modeling kernel by
NVIDIA, published as part of USD BRep:

- Repository: <https://github.com/NVIDIA-Omniverse/usd-brep>
- Pinned commit: `c9c5979`
- Licence: Apache License, Version 2.0 (see [`LICENSE`](LICENSE), copied verbatim
  from upstream, and [`NOTICE`](NOTICE))
- Copyright: (c) 2020-2026 NVIDIA CORPORATION & AFFILIATES

solidcore is an independent project. It is not affiliated with or endorsed by
NVIDIA, and "SMLib" and "NVIDIA" are used here only to identify the origin of the
code, not as branding.

## Upstream material used

Only these upstream paths are read, ported or exercised:

| Upstream path                  | Use                                                    |
|--------------------------------|--------------------------------------------------------|
| `source/SMLib/`                | Kernel being ported (geometry, topology, algorithms)   |
| `source/SM_API/`               | High-level API being ported; reference for public ops  |
| `tests/SM_API_test/`           | Reference tests; source of difftest cases              |
| `tests/prog_test/`             | Reference tests; source of difftest cases              |
| `TestFiles/pt_TestFiles/`      | Test fixtures                                          |
| `AGENTS.md`, `.agents/`        | Upstream developer documentation (OCCT/USD files excluded) |
| `LICENSE`, `THIRD_PARTY_NOTICES.md` | Licence text and notices                          |

The C++ reference build (`harness/cpp`) compiles only `source/SMLib` and
`source/SM_API`, from a local clone that is never committed (`upstream/` is
gitignored).

## Upstream material not used

The following upstream layers are **not** read, ported, built or linked:

- OCCT translation: `source/OCCT_BREP_IMPORT`, `source/OCCT_BREP_EXPORT`,
  `TestFiles/occt_breps*`
- USD / Hydra integration: `source/SM_API_USD`, `source/BREP_SM_USD`,
  `source/BREP_USD_DATA`, `source/HYDRA_USD_BREP`, `source/USD_BREP_UTILS`,
  `source/schema`, `TestFiles/usd_TestFiles`
- Python bindings and tooling: `source/SmPyLib`, `source/SmPyDevLib`, `tools/`
- Proposals: `proposals/`

No GPL/LGPL code is used anywhere in solidcore.

## Per-file provenance

Every ported Rust file begins with a header naming the upstream source file(s) it
derives from and a table mapping its Rust items to upstream names (see
[`AGENTS.md`](AGENTS.md#file-header)).
