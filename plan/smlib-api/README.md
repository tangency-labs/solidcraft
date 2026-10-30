# SolidCraft API (`smlib_*`) progress

This directory has one file per function of the SolidCraft fork's C API (tangency-labs/smlib `capi/include/smlib.h`), named `<smlib_fn>.toml`. Each file tracks where that function stands in `crates/solidcore-smlib-api`, the Rust form of the C API over solidcore. The site's "SolidCraft API" page (`site/build.py`) reads the files in sorted order.

Each function has its own file, and nobody keeps a shared list. A family PR changes the files of the functions it ports, so PRs for different families never conflict.

## Fields

| Field | Type | Meaning |
|---|---|---|
| `family` | string | The family the function belongs to. The ported families are `F1` (core: bodies, measures, check, primitives, booleans), `F1b` (flat arrays), `F2` (blends), `F3` (sweeps), `F4` (offsets) and `F5` (surface tools). `later` covers history and the functions that need fork SMLib code ported first. |
| `status` | string | One of `missing`, `partial`, `ported` or `tested`. `missing`: not in the crate. `partial`: in the crate with a branch or option missing, which `notes` names. `ported`: all of the function's glue is in the crate. `tested`: ported, and the C API's own tests for it run in the crate's tests. |
| `solidcore_fn` | string | The Rust path of the API function, such as `solidcore_smlib_api::Smlib::boolean`. Empty while `missing`. |
| `pr` | integer or `""` | The PR that ported it (or is porting it). |
| `fork_file` | string | The C API source file that defines the function, under tangency-labs/smlib. |
| `fork_commit` | string | The fork commit the port follows: that file's latest commit on `replay/1.0.1` when it was seeded or ported. |
| `notes` | string | The kernel calls the glue goes through, plus anything missing or different. |

## Updating

A family PR sets `status`, `solidcore_fn`, `pr` and `fork_commit` on its functions' files, in the same PR as the code.
