"""``solidcraft``: run the ``solidcraft-cli`` this package finds, with every argument passed on.

    uvx solidcraft mcp                       # the MCP server, over stdio
    uvx solidcraft --version
    uvx solidcraft run replay.json --in part.sc

The binary is the one the client uses (``find_cli``): ``$SOLIDCRAFT_CLI``, the one bundled in a
platform wheel, then ``PATH``. With no arguments it prints the CLI's help.
"""

from __future__ import annotations

import os
import subprocess
import sys

from ._mcp import SolidCraftError, find_cli


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv) or ["--help"]
    try:
        cli = find_cli()
    except SolidCraftError as e:
        print(f"solidcraft: {e}", file=sys.stderr)
        return 1
    try:
        if os.name != "nt":
            sys.stdout.flush()
            sys.stderr.flush()
            os.execvp(cli, [cli, *args])  # a bare SOLIDCRAFT_CLI is looked up on PATH, as Popen does; replaces this process: stdio, signals and exit code are the CLI's
        p = subprocess.Popen([cli, *args])
    except OSError as e:
        print(f"solidcraft: cannot run {cli}: {e}", file=sys.stderr)
        return 127
    # Windows: Ctrl+C reaches the CLI too (same console); wait for it to finish and pass on its code.
    while True:
        try:
            return p.wait()
        except KeyboardInterrupt:
            continue


if __name__ == "__main__":
    sys.exit(main())
