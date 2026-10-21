# Progress site

`site/build.py` generates the GitHub Pages site from data already in the repository;
nothing on it is maintained by hand except the short intro paragraph in the generator.

```sh
python3 site/build.py && python3 -m http.server -d bin/site 8000   # build (~3 s), then open http://localhost:8000
```

Python 3.11+ standard library only (`tomllib`). Output goes to `bin/site/` (gitignored);
`--out DIR` writes elsewhere. The output is deterministic: the only date on it is the
commit date of `HEAD`.

| Page | Data |
|------|------|
| Overview (`index.html`) | `plan/units/*.toml` via `harness/plan/progress.py` (`summary`, `is_ready`), claims from the "In progress" section of `plan/PROGRESS.md`, `reports/ssi-corpus.json`, and the plan along `main`'s first-parent git history for the timeline (`plan/units/`, or `plan/modules.toml` before the per-unit split). The history is read once; overall, per-module and per-unit series come from the same pass |
| Module pages (`areas/<module>.html`) | `plan/units/` and its history: the same chart as the overview, over that module's units only |
| Units (`units/index.html`) | `plan/units/*.toml`, `harness/callgraph/units.json`, `reports/difftest/*.json` (`cases_*`, `deferred_by_unit`; copied to `reports/<unit>.json` on the site). Reports are not committed: CI fetches main's latest set with `scripts/fetch-reports.sh` before building; run it locally too |
| Unit pages (`units/<id>.html`) | the above, plus the `//! | Rust | Upstream |` table in the module's file header and the unit's status history from the timeline |
| Dependency graph (`graph.html`, `assets/graph.json`) | `plan/units/*.toml` (`depends_on`, module, status) and claims, laid out as one force-directed cluster per module; lists of ready and most-blocking units are rendered server-side, the graph by `site/assets/graph.js` |
| Upstream issues (`issues.html`) | `plan/upstream-issues.md`, split at `#### <SERIES>-<n>.` headings |
| SSI corpus (`ssi.html`) | `reports/ssi-corpus.json` |

Text from the repository is HTML-escaped before a small markdown subset (inline code,
bold, italic, http(s) links, nested `- ` lists) is applied, so raw HTML written in a PR
never reaches the page. Charts are inline SVG; `site/assets/app.js` only sorts and
filters tables, drives the chart tooltips and the overview's module picker (without JS the overview
shows the all-units chart). The only third-party code is d3-force (ISC), vendored under
`site/assets/vendor/` and used only by the graph page (see `ATTRIBUTION.md`). No external assets.

Deployment: `.github/workflows/pages.yml` rebuilds on every push to `main`, after a
"Progress checklist" run that pushed a new `plan/PROGRESS.md` (those commits carry
`[skip ci]`, so they do not trigger a push build), and on manual dispatch. GitHub Pages
must be enabled with source "GitHub Actions".
