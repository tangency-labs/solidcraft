# Upstream SMLib issues (one file each)

One file per upstream C++ defect, so parallel PRs never conflict. Never append to a
shared file. The rules, fields, severity order and the 436 entries from before
2026-10-25 are in the frozen archive [`plan/upstream-issues.md`](../upstream-issues.md).
The site (`site/build.py`) renders the archive and every file here, in name order.

- **Name:** `<ID>--<short-slug>.md`, e.g. `WR-262--extrusion-drop-point-fast.md`. The ID
  takes the next free number in its series (`UB-`, `HANG-`, `ND-`, `WR-`, `DOC-`) across
  the archive and this directory. If two PRs pick the same number, whoever merges second
  renumbers theirs (the site flags duplicates).
- **Content:**

  ```markdown
  #### WR-262. One-line title [PSIRT if file input reaches memory corruption]
  - **Severity:** Wrong result
  - **Area:** Surfaces and volumes
  - **Location:** `source/SMLib/src/File.cpp:123`, `Function` (def. `:100`).
  - **What happens:** ...
  - **Trigger:** ...
  - **How found:** generated difftest case
  - **Confirmed:** yes, case `gen0042` of `unit.id`
  ```

  `Severity` is one of `Crash / undefined behaviour`, `Hang`, `Nondeterminism`,
  `Wrong result`, `Docs`. `Area` is free text, ideally one of the archive's section
  headings.

- **Updates** (now confirmed, fixed upstream, renumbered, withdrawn) are addenda, not
  edits: a new file `<ID>--addendum-<YYYY-MM-DD>-<slug>.md` starting with
  `Updates: <ID>` and what changed. This applies to archive entries too: CI rejects any
  line added to the frozen archives.
