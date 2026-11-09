# Pending attribution rows

A pull request that adds an asset adds **one new file here** instead of a row in `ATTRIBUTION.md`,
so two pull requests never edit the same table.

- Name it after the change, lowercase with dashes: `render-studio-example.md`.
- Contents: the rows only, in the format of the files table in `ATTRIBUTION.md`
  (`| File | Author | Source | Licence |`); a header line is allowed and ignored.
- A file that already has a row in `ATTRIBUTION.md` or another pending file fails
  `cargo xtask lists`. `cargo xtask assets` counts pending rows as attributed.
- After a merge, the Generated files workflow runs `cargo xtask fold-pending` on `main`: it appends
  the rows to `ATTRIBUTION.md` and deletes the files. Never run it in a pull request.
