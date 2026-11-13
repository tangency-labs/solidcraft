<!-- SPDX-License-Identifier: Apache-2.0 -->

# STEP reader bake-off

Measures STEP readers on one corpus: how many files each opens and refuses, how many of the
bodies come out closed, how many reference volumes they match, and how long a file takes.

```sh
bench/step/fetch.sh /tmp/step-corpus             # build the corpus (downloads; never commit it)
bench/step/run.py --corpus /tmp/step-corpus --out /tmp/bake \
    --reader base=/path/to/old/solidcraft-cli --reader new=/path/to/new/solidcraft-cli
```

- `fetch.sh` collects the files, folds duplicates by SHA-256 and writes `manifest.tsv`. The
  sources are pinned: SolidCraft's own STEP tests (when run inside SolidCraft's tree),
  dan335/step-conformance, solidcore's fixtures, cadmpeg's fixtures, and the NIST PMI and STEP
  Tools sets through `scripts/fetch-*-corpus.sh` (SHA-256 pinned). The STEP Tools files are "All
  Rights Reserved": they are downloaded for the run and never committed.
- `run.py` runs each reader as `COMMAND eval FILE --json-out OUT` (the `solidcraft-cli eval`
  shape: `bodies[]` with `volume_mm3`, `faces`, `closed`), with a timeout, and writes
  `per-file.tsv` and `summary.md`.
- `references.tsv` holds the volumes known from formulas, by the file's SHA-256: the
  step-conformance parts (their generator's profiles) and the import rig's expected volumes.

Wall time includes process start, so read it beside the counts. A machine under other load moves
the median; compare readers in the same run.
