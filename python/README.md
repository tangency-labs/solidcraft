# solidcraft (Python)

Drive [SolidCraft](https://github.com/tangency-labs/solidcraft) from Python. Every engine command
and every MCP tool is a typed method, generated from the build, and NumPy, SciPy or pandas values
can be passed straight in.

```python
import numpy as np
import solidcraft as sc

with sc.session() as d:
    d.cmd.solid.box(length=60, width=40, height=8, body_name="Plate")
    a = np.linspace(0, 2 * np.pi, 6, endpoint=False)
    for p in np.column_stack([30 + 15 * np.cos(a), 20 + 15 * np.sin(a), np.full(6, 8.0)]):
        d.cmd.solid.hole(position=p, diameter=4)
    print(d.changes())
```

`sc.session()` uses a running session daemon (`solidcraft-session serve`) when there is one, so
several scripts work on one design, and otherwise starts its own headless SolidCraft. It talks to
`solidcraft-cli mcp --structured`, the same MCP server agents use. Design notes: `docs/python-api.md`.

```sh
pip install solidcraft
uvx solidcraft mcp     # the MCP server over stdio: command "uvx", args ["solidcraft", "mcp"]
```

The `solidcraft` command runs `solidcraft-cli` with the same arguments and exit code
(`uvx solidcraft --version`).

The package is pure Python and carries no binary. The first run downloads this version's
`solidcraft-cli` for your platform (100 to 250 MB, once) from the GitHub release, checks
it against the release's `SHA256SUMS.txt`, and keeps it in your user cache. A `solidcraft-cli` on
`PATH` of the same minor version or newer (an installed SolidCraft) is used instead, and
`SOLIDCRAFT_CLI` overrides both. Offline: `solidcraft install-cli --from <archive>` puts a
downloaded archive in the cache; `SOLIDCRAFT_NO_DOWNLOAD=1` turns the download off.
`solidcraft install-cli` also copies the CLI to `~/.local/bin` (it never edits `PATH`).

The generated bindings come from `cargo xtask python` and are not committed.
