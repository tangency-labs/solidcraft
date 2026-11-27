"""The session daemon: one ``solidcraft-cli mcp`` process that several scripts share, so a design
lives on between them (an agent runs each script as its own process).

``solidcraft-session serve`` starts it and writes ``{"port", "token", "pid"}`` to the session file
(``$SOLIDCRAFT_SESSION``, default ``~/.solidcraft/session.json``, readable by the owner only). A
script's :func:`solidcraft.session` finds the file and connects to ``127.0.0.1:port``. Each request
is one JSON line ``{"token", "tool", "args"}``, answered with ``{"result", "notes", "images"}`` or
``{"error"}``. Only loopback connections that know the token are served."""

from __future__ import annotations

import json
import os
import secrets
import socket
import socketserver
import threading
from pathlib import Path
from typing import Any, Sequence

from ._mcp import Client, SolidCraftError


def session_file() -> Path:
    env = os.environ.get("SOLIDCRAFT_SESSION")
    return Path(env) if env else Path.home() / ".solidcraft" / "session.json"


def serve(mcp_args: Sequence[str], path: Path | None = None, cli: str | None = None, ready: threading.Event | None = None) -> None:
    """Serve until a ``stop`` request; ``mcp_args`` go to ``solidcraft-cli mcp``."""
    path = path or session_file()
    client = Client(mcp_args, cli=cli)
    token = secrets.token_hex(16)
    lock = threading.Lock()

    class Handler(socketserver.StreamRequestHandler):
        def handle(self) -> None:
            for raw in self.rfile:
                try:
                    req = json.loads(raw)
                except ValueError:
                    return
                if not isinstance(req, dict) or not secrets.compare_digest(str(req.get("token", "")), token):
                    self._reply({"error": "bad token"})
                    return
                op = req.get("op")
                if op == "info":
                    self._reply({"result": {"server_version": client.server_version, "instructions": client.instructions}})
                    continue
                if op == "stop":
                    self._reply({"result": "stopping"})
                    threading.Thread(target=server.shutdown, daemon=True).start()
                    return
                try:
                    with lock:
                        result, notes, images = client.call_tool(str(req.get("tool")), req.get("args") or {})
                    self._reply({"result": result, "notes": notes, "images": images})
                except SolidCraftError as e:
                    self._reply({"error": str(e)})

        def _reply(self, msg: dict) -> None:
            self.wfile.write((json.dumps(msg) + "\n").encode("utf-8"))
            self.wfile.flush()

    class Server(socketserver.ThreadingTCPServer):
        daemon_threads = True
        allow_reuse_address = True

    server = Server(("127.0.0.1", 0), Handler)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        json.dump({"port": server.server_address[1], "token": token, "pid": os.getpid()}, f)
    os.replace(tmp, path)
    if ready is not None:
        ready.set()
    try:
        server.serve_forever()
    finally:
        server.server_close()
        client.close()
        try:
            if json.loads(path.read_text()).get("token") == token:
                path.unlink()
        except (OSError, ValueError):
            pass


class Remote:
    """A connection to a running session daemon."""

    def __init__(self, path: Path | None = None) -> None:
        info = json.loads((path or session_file()).read_text())
        self._token = info["token"]
        self._sock = socket.create_connection(("127.0.0.1", int(info["port"])), timeout=None)
        self._file = self._sock.makefile("rwb")
        about = self._request({"op": "info"})["result"]
        self.server_version: str = about.get("server_version", "")
        self.instructions: str = about.get("instructions", "")

    def _request(self, msg: dict) -> dict:
        msg["token"] = self._token
        self._file.write((json.dumps(msg) + "\n").encode("utf-8"))
        self._file.flush()
        line = self._file.readline()
        if not line:
            raise SolidCraftError("the SolidCraft session daemon closed the connection")
        reply = json.loads(line)
        if "error" in reply:
            raise SolidCraftError(reply["error"])
        return reply

    def call_tool(self, name: str, arguments: dict) -> tuple[Any, list[str], list[dict]]:
        r = self._request({"tool": name, "args": arguments})
        return r.get("result"), list(r.get("notes") or []), list(r.get("images") or [])

    def stop(self) -> None:
        try:
            self._request({"op": "stop"})
        except (SolidCraftError, OSError):
            pass

    def close(self) -> None:
        try:
            self._file.close()
            self._sock.close()
        except OSError:
            pass


def daemon_running(path: Path | None = None) -> bool:
    p = path or session_file()
    try:
        info = json.loads(p.read_text())
        with socket.create_connection(("127.0.0.1", int(info["port"])), timeout=1):
            return True
    except (OSError, ValueError, KeyError):
        return False
