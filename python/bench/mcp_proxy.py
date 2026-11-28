"""A transparent stdio proxy in front of an MCP server, for the bench (arms A, B, C).

It passes every line through unchanged, so the model sees exactly the server's tools and replies.
After each tool call's reply reaches the client, it asks the server for
`measure {summary: true}` on its own (the client never sees that exchange) and writes the reply's
JSON to `--out`. The file therefore always holds the design as the last tool call left it, however
the client ends the server (closing its input or a signal).

    python mcp_proxy.py --out final.json -- <server command...>
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import threading


def main() -> int:
    args = sys.argv[1:]
    sep = args.index("--")
    out = args[args.index("--out") + 1]
    step = args[args.index("--step") + 1] if "--step" in args[:sep] else None
    native = args[args.index("--save") + 1] if "--save" in args[:sep] else None
    server = subprocess.Popen(args[sep + 1 :], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1)
    assert server.stdin and server.stdout
    write_lock = threading.Lock()
    tool_calls: set = set()
    n = [0]

    def to_server(line: str) -> None:
        with write_lock:
            server.stdin.write(line if line.endswith("\n") else line + "\n")
            server.stdin.flush()

    def measure() -> None:
        n[0] += 1
        req = {"jsonrpc": "2.0", "id": f"bench-{n[0]}", "method": "tools/call", "params": {"name": "measure", "arguments": {"summary": True}}}
        reqs = [req]
        if step:  # the final design as STEP too, for the real-world checker
            reqs.append({"jsonrpc": "2.0", "id": f"bench-step-{n[0]}", "method": "tools/call", "params": {"name": "export", "arguments": {"path": step}}})
        if native:  # the native design after every call: a client may end the server before a final save
            reqs.append({"jsonrpc": "2.0", "id": f"bench-step-save-{n[0]}", "method": "tools/call", "params": {"name": "save", "arguments": {"path": native, "overwrite": True}}})
        try:
            for r in reqs:
                to_server(json.dumps(r))
        except OSError:
            pass

    def save(reply: dict) -> None:
        r = reply.get("result", {})
        text = "".join(c.get("text", "") for c in r.get("content", []) if c.get("type") == "text")
        try:
            final = json.loads(text)
        except ValueError:
            final = {"error": text[:2000]}
        tmp = out + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(final, f)
        os.replace(tmp, out)

    def pump() -> None:
        for line in server.stdout:
            try:
                msg = json.loads(line)
            except ValueError:
                msg = None
            if isinstance(msg, dict) and isinstance(msg.get("id"), str) and msg["id"].startswith("bench-"):
                if msg["id"] == "bench-native":
                    saved.set()
                elif not msg["id"].startswith("bench-step-"):
                    save(msg)
                continue
            sys.stdout.write(line)
            sys.stdout.flush()
            if isinstance(msg, dict) and msg.get("id") in tool_calls:
                tool_calls.discard(msg.get("id"))
                measure()

    saved = threading.Event()
    finishing = threading.Lock()

    def finish(*_: object) -> None:
        """Save the native design (components, BOM, names) once the client is done with the server."""
        if not native or not finishing.acquire(blocking=False):
            return
        try:
            req = {"jsonrpc": "2.0", "id": "bench-native", "method": "tools/call", "params": {"name": "save", "arguments": {"path": native, "overwrite": True}}}
            to_server(json.dumps(req))
            saved.wait(timeout=60)
        except OSError:
            pass

    def on_signal(signum: int, _frame: object) -> None:
        finish()
        try:
            server.kill()
        finally:
            os._exit(0)

    for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
        signal.signal(sig, on_signal)
    threading.Thread(target=pump, daemon=True).start()
    measure()  # the starting design, in case no tool is ever called
    for line in sys.stdin:
        try:
            msg = json.loads(line)
            if isinstance(msg, dict) and msg.get("method") == "tools/call" and "id" in msg:
                tool_calls.add(msg["id"])
        except ValueError:
            pass
        to_server(line)
    finish()
    try:
        server.stdin.close()
        server.wait(timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        server.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
