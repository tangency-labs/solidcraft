# The Python API experiment

Five setups, the same tasks, model and checker (docs/python-api.md, "The experiment"):

| Setup | What the model gets |
|---|---|
| A | the plain SolidCraft MCP server (`solidcraft-cli mcp --headless`) |
| B | scx v15.1 as shipped (`docs/scx-hybrid/scx.py`, `math` and `itertools` only) |
| C | scx v15.1 with any import allowed (`scx_open.py`) |
| D | Bash and the `solidcraft` package: every command and tool, the change report (`prompts/d.md`) |
| E | D plus scx's vocabulary on top of the package (`scx_vocab.py`, `prompts/e.md`) |

Each run is `claude -p` in an empty folder with only that setup's tools, and none of the user's
settings, CLAUDE.md, skills or memory. The checker reads the final design: every expected body
within 0.1% in volume and 0.05 mm in bounding box, and nothing extra. For A, B and C,
`mcp_proxy.py` sits between the client and the server, unseen, and measures after every tool call.
For D and E, the session daemon (`solidcraft-session serve`) holds the design and the harness
measures it after the run.

```sh
cd python
uv venv --python 3.12 .venv && uv pip install --python .venv/bin/python -e '.[test]' numpy scipy pandas
cargo xtask python                                   # the bindings, from the build
export SOLIDCRAFT_CLI=$PWD/../target/ci/solidcraft-cli
.venv/bin/python bench/make_numeric.py               # tasks/numeric.json (already committed)
.venv/bin/python bench/run.py --model haiku --reps 3 --jobs 8 --out bench/results/haiku.jsonl
.venv/bin/python bench/report.py bench/results/haiku.jsonl
```

`bench/work/` (run folders, transcripts) and `bench/results/` are not committed.
