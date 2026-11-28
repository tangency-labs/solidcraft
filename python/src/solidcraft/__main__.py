"""``solidcraft-session``: start or stop the session daemon that keeps one design across scripts.

    solidcraft-session serve [--in design.sc] [--allow-path DIR]... [--connect HOST:PORT]
    solidcraft-session stop
    solidcraft-session status
"""

from __future__ import annotations

import os
import sys

from ._daemon import Remote, daemon_running, serve, session_file


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    op = args.pop(0) if args else "status"
    if op == "serve":
        mcp: list[str] = []
        i = 0
        connect = False
        allowed = False
        while i < len(args):
            a = args[i]
            if a in ("--in", "--allow-path", "--connect", "--token") and i + 1 < len(args):
                mcp += [a, args[i + 1]]
                connect = connect or a == "--connect"
                allowed = allowed or a == "--allow-path"
                i += 2
            else:
                print(f"solidcraft-session: unknown argument {a}", file=sys.stderr)
                return 2
        if not connect:
            mcp.insert(0, "--headless")
        if not allowed:
            mcp += ["--allow-path", os.getcwd()]
        serve(mcp)
        return 0
    if op == "stop":
        if daemon_running():
            Remote().stop()
        return 0
    if op == "status":
        up = daemon_running()
        print(f"{'running' if up else 'not running'} ({session_file()})")
        return 0 if up else 1
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
