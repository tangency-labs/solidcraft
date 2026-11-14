# Pinned upstream reader fixtures

These files are verbatim copies of the Apache-2.0 upstream SMLib test fixtures that
`arena_uv_trim_owner.rs` and `issue_1570_flat_body_ray_fire.rs` read through the Rust
B-rep reader. They are vendored so those regressions run without an `upstream/`
checkout, and they are the bytes of the pinned commit, not of whatever `upstream/`
happens to have checked out.

- Repository: <https://github.com/NVIDIA-Omniverse/usd-brep>
- Pinned release: v1.0.1, commit `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`
  (`scripts/upstream-pin.env` is the single source of truth)
- Licence: Apache License, Version 2.0 (see the repository's [`LICENSE`](../../../../../LICENSE)
  and [`NOTICE`](../../../../../NOTICE))
- Copyright: (c) 2020-2026 NVIDIA CORPORATION & AFFILIATES

| File             | Upstream path                                   | Git blob SHA-1                             | Bytes | SHA-256                                                            |
|------------------|-------------------------------------------------|--------------------------------------------|-------|--------------------------------------------------------------------|
| `box.smb`        | `TestFiles/pt_TestFiles/Solids/box.smb`         | `8ce6715a5219141a12b97bb7f0c11e6835091af9` | 14188 | `38b3eca8ae884135c58f988a4c4dc1573d48e76d612a71cc50924e20b6a5e4b8` |
| `revolution.smb` | `TestFiles/pt_TestFiles/Solids/revolution.smb`  | `20324a00ef4fa6bf16a89de2630471b1c5e64a34` | 43937 | `65d445184f8a892146b5a5d9d9aca8c0d5542590b2cb05dc6d3b3c61e6dde005` |

Each file was extracted with `git cat-file blob
3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5:<upstream path>`, and its Git blob SHA-1 and
size were checked against `git rev-parse`/`git cat-file -s` for that commit. The tests
embed them with `include_bytes!` and pass the bytes to `read_from_file`; no test reads
`upstream/` at run time.
