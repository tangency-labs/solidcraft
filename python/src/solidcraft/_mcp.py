"""A minimal MCP client over stdio to ``solidcraft-cli mcp --structured``.

One JSON-RPC message per line. ``--structured`` makes every tool result carry its reply as data
(``structuredContent: {"result", "notes"?}``), so nothing here parses text."""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import threading
from typing import Any, Sequence

from . import _download

PROTOCOL = "2025-06-18"


class SolidCraftError(RuntimeError):
    """A tool or command that SolidCraft refused or that failed; the message is SolidCraft's."""


def _version_tuple(text: str) -> tuple[int, int] | None:
    m = re.search(r"(\d+)\.(\d+)", text)
    return (int(m.group(1)), int(m.group(2))) if m else None


def path_cli_ok(path: str, version: str) -> tuple[bool, str]:
    """Whether a ``solidcraft-cli`` found on PATH may stand in for this package's own version:
    it must start and be the same minor version or newer. Returns (usable, why not)."""
    try:
        r = subprocess.run([path, "--version"], capture_output=True, text=True, timeout=30, check=False)
    except (OSError, subprocess.SubprocessError) as e:
        return False, f"{path} (found on PATH) cannot start: {e}"
    if r.returncode != 0:
        why = (r.stderr.strip().splitlines() or [f"exit status {r.returncode}"])[-1]
        return False, f"{path} (found on PATH) failed to start: {why}"
    want, got = _version_tuple(version), _version_tuple(r.stdout)
    if want and got and got < want:
        return False, f"{path} (found on PATH) is {r.stdout.strip()}, older than this package's {version}"
    return True, ""


def locate_cli() -> tuple[str, str]:
    """``(path, where it came from)`` of the ``solidcraft-cli`` to run: ``$SOLIDCRAFT_CLI``, one
    bundled in the package, the one downloaded earlier for this version, one on ``PATH`` of the same
    minor version or newer, then a download of this version's own (``_download``)."""
    env = os.environ.get("SOLIDCRAFT_CLI")
    if env:
        return env, "SOLIDCRAFT_CLI"
    bundled = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bin", _download.EXE)
    if os.path.isfile(bundled):
        return bundled, "bundled in this package"
    version = _download.package_version()
    cached = _download.cached(version)
    if cached:
        return cached, "downloaded for this version"
    skipped = ""
    found = shutil.which("solidcraft-cli")
    if found:
        ok, skipped = path_cli_ok(found, version)
        if ok:
            return found, "found on PATH"
    if not version:
        raise SolidCraftError(f"solidcraft-cli not found; {skipped + '; ' if skipped else ''}{_download.HINT}")
    if _download.disabled():
        raise SolidCraftError(
            f"solidcraft-cli not found and SOLIDCRAFT_NO_DOWNLOAD is set; {skipped + '; ' if skipped else ''}{_download.HINT}"
        )
    try:
        return _download.download(version), "downloaded for this version"
    except _download.DownloadError as e:
        raise SolidCraftError(f"{e}; {skipped + '; ' if skipped else ''}{_download.HINT}") from e


def find_cli() -> str:
    """The ``solidcraft-cli`` to run (``locate_cli``'s order); may download it the first time."""
    return locate_cli()[0]


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
