"""A minimal MCP client over stdio to ``solidcraft-cli mcp --structured``.

One JSON-RPC message per line. ``--structured`` makes every tool result carry its reply as data
(``structuredContent: {"result", "notes"?}``), so nothing here parses text."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import threading
from typing import Any, Sequence

PROTOCOL = "2025-06-18"


class SolidCraftError(RuntimeError):
    """A tool or command that SolidCraft refused or that failed; the message is SolidCraft's."""


def find_cli() -> str:
    """``solidcraft-cli``: ``$SOLIDCRAFT_CLI``, the binary bundled in a wheel, then ``PATH``."""
    env = os.environ.get("SOLIDCRAFT_CLI")
    if env:
        return env
    exe = "solidcraft-cli.exe" if os.name == "nt" else "solidcraft-cli"
    bundled = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bin", exe)
    if os.path.isfile(bundled):
        return bundled
    found = shutil.which("solidcraft-cli")
    if found:
        return found
    raise SolidCraftError("solidcraft-cli not found: install SolidCraft, or set SOLIDCRAFT_CLI to its path")


class Client:
    """One ``solidcraft-cli mcp`` process and its MCP session."""

    def __init__(self, args: Sequence[str], cli: str | None = None) -> None:
        argv = [cli or find_cli(), "mcp", "--structured", *args]
        self._p = subprocess.Popen(
            argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self._next = 0
        self._lock = threading.Lock()
        init = self._rpc("initialize", {"protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": {"name": "solidcraft-python", "version": "0"}})
        self.server_version: str = init.get("serverInfo", {}).get("version", "")
        self.instructions: str = init.get("instructions", "")
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _send(self, msg: dict) -> None:
        if self._p.stdin is None:
            raise SolidCraftError("the SolidCraft MCP server has no input")
        self._p.stdin.write(json.dumps(msg) + "\n")
        self._p.stdin.flush()

    def _rpc(self, method: str, params: dict) -> Any:
        with self._lock:
            self._next += 1
            n = self._next
            self._send({"jsonrpc": "2.0", "id": n, "method": method, "params": params})
            while True:
                line = self._p.stdout.readline() if self._p.stdout else ""
                if not line:
                    raise SolidCraftError("the SolidCraft MCP server exited")
                msg = json.loads(line)
                if msg.get("id") != n:
                    continue
                if "error" in msg:
                    err = msg["error"]
                    raise SolidCraftError(err.get("message", str(err)) if isinstance(err, dict) else str(err))
                return msg.get("result", {})

    def call_tool(self, name: str, arguments: dict) -> tuple[Any, list[str], list[dict]]:
        """``(result, notes, images)`` of a tool; raises :class:`SolidCraftError` on an error result."""
        r = self._rpc("tools/call", {"name": name, "arguments": arguments})
        if "structuredContent" not in r and not r.get("isError"):
            raise SolidCraftError(
                f"solidcraft-cli {self.server_version} sends no structured results (`mcp --structured` needs a newer SolidCraft); "
                "set SOLIDCRAFT_CLI to a newer solidcraft-cli"
            )
        data = r.get("structuredContent") or {}
        result = data.get("result")
        notes = list(data.get("notes") or [])
        images = [c for c in r.get("content", []) if c.get("type") == "image"]
        if r.get("isError"):
            msg = result if isinstance(result, str) else json.dumps(result)
            raise SolidCraftError(f"{name}: {msg}")
        return result, notes, images

    def list_tools(self) -> list[dict]:
        return list(self._rpc("tools/list", {}).get("tools", []))

    def close(self) -> None:
        if self._p.poll() is None:
            try:
                if self._p.stdin:
                    self._p.stdin.close()
                self._p.wait(timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                self._p.kill()
