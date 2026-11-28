"""A SolidCraft session: every MCP tool as a method, every command under ``cmd``, and a report
of what changed."""

from __future__ import annotations

import atexit
import json
import os
import sys
from typing import Any, Sequence

from ._encode import plain
from ._mcp import Client, SolidCraftError

try:
    from ._generated import VERSION
    from ._generated.commands import INDEX, Commands
    from ._generated.tools import Tools
except ImportError as e:  # pragma: no cover - a source checkout before `cargo xtask python`
    raise ImportError("solidcraft's generated bindings are missing: run `cargo xtask python` in the SolidCraft checkout") from e


class Session(Tools):
    """One design session. Use :func:`solidcraft.session` (or :func:`headless`) to make one.

    - ``d.measure()``, ``d.body_topology(body=...)``, ``d.save(path=...)``: the MCP tools.
    - ``d.cmd.solid.box(length=..., ...)``: the engine commands, by id (``help(d.cmd.solid)``).
    - ``d.call("solid.box", length=10, ...)``: a command by its id string.
    - ``d.changes()``: what changed since the session started (or since ``d.mark()``).
    """

    def __init__(self, transport: Any, report: bool = False, owns: bool = True) -> None:
        self._t = transport
        self._owns = owns
        self._closed = False
        self.notes: list[str] = []
        self.images: list[dict] = []
        self.cmd = Commands(self._command)
        self.server_version: str = getattr(transport, "server_version", "")
        if self.server_version and VERSION and self.server_version != VERSION:
            print(
                f"solidcraft: the bindings are for SolidCraft {VERSION}, the server is {self.server_version}; "
                "a command that changed may refuse its arguments",
                file=sys.stderr,
            )
        self._report = report
        self._start = self._snapshot() if report else None
        self._mark = self._start
        if report:
            atexit.register(self._report_at_exit)

    # -- calling --------------------------------------------------------------------------

    def tool(self, name: str, **args: Any) -> Any:
        """Call an MCP tool by name (``d.tool("measure", summary=True)``); plugin tools too."""
        return self.tool_call(name, args, {})

    def tool_call(self, name: str, args: dict[str, Any], extra: dict[str, Any]) -> Any:
        merged = {k: plain(v, k) for k, v in {**args, **extra}.items() if v is not None}
        result, notes, images = self._t.call_tool(name, merged)
        self.notes, self.images = notes, images
        return result

    def _command(self, command: str, params: dict[str, Any], extra: dict[str, Any]) -> Any:
        merged = {k: plain(v, k) for k, v in {**params, **extra}.items() if v is not None}
        return self.tool_call("execute", {"command": command, "params": merged}, {})

    def call(self, command: str, **params: Any) -> Any:
        """Run an engine command by its id: ``d.call("solid.box", length=10, width=10, height=5)``."""
        return self._command(command, params, {})

    def describe(self, command: str) -> Any:
        """A command's documentation and input and output schemas."""
        return self.tool_call("describe_command", {"command": command}, {})

    # -- what changed ---------------------------------------------------------------------

    def _snapshot(self) -> dict[str, dict]:
        try:
            m = self.tool_call("measure", {"summary": True}, {})
        except SolidCraftError:
            return {}
        parts: dict[str, str] = {}
        try:
            comps = self._command("component.list", {"compact": True, "bodies": True}, {}).get("components", [])
            parts = {b: c["name"] for c in comps if c.get("parent") is not None for b in c.get("bodies", [])}
        except (SolidCraftError, AttributeError):
            pass
        out = {}
        for b in (m or {}).get("bodies", []):
            bb = b.get("bbox") or {}
            out[b["name"]] = {
                "volume": _num(b.get("volume_mm3")),
                "bbox": [_num(bb.get("min")), _num(bb.get("max"))] if bb else None,
                "faces": b.get("faces"),
                "part": parts.get(b["name"]),
            }
        return out

    def mark(self) -> None:
        """Start :meth:`changes` from now."""
        self._mark = self._snapshot()

    def changes(self, since: dict | None = None) -> str:
        """New (+), changed (~) and removed (-) bodies with volume, bounding box and faces."""
        before = since if since is not None else (self._mark if self._mark is not None else {})
        return _summary(before, self._snapshot())

    def _report_at_exit(self) -> None:
        if self._closed or self._start is None:
            return
        try:
            print("\n[SolidCraft] " + _summary(self._start, self._snapshot()).replace("\n", "\n[SolidCraft] "))
        except Exception as e:  # noqa: BLE001 - never hide the script's own output
            print(f"[SolidCraft] could not report changes: {e}")

    # -- lifetime -------------------------------------------------------------------------

    def close(self) -> None:
        if self._closed:
            return
        if self._report:
            self._report_at_exit()
        self._closed = True
        self._t.close()

    def __enter__(self) -> "Session":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()


def _num(v: Any) -> Any:
    if isinstance(v, float):
        r = round(v, 4)
        return int(r) if r == int(r) else r
    if isinstance(v, list):
        return [_num(x) for x in v]
    return v


def _line(n: str, a: dict) -> str:
    bb = f"{a['bbox'][0]}..{a['bbox'][1]}" if a.get("bbox") else "None..None"
    part = f"  part {a['part']}" if a.get("part") else ""
    return f"{n}: vol {a['volume']}  bbox {bb}  faces {a['faces']}{part}"


def _summary(before: dict, after: dict) -> str:
    lines = []
    new = [n for n in after if n not in before]
    groups: dict[tuple, list[str]] = {}
    for n in new:
        a = after[n]
        size = tuple(round(q - p, 4) for p, q in zip(*a["bbox"])) if a.get("bbox") else ()
        groups.setdefault((a["volume"], a["faces"], size), []).append(n)
    for (vol, faces, size), names in groups.items():
        if len(names) >= 3:
            corners = " ".join(str(after[n]["bbox"][0]) for n in names if after[n].get("bbox"))
            lines.append(f"+ {len(names)} alike ({names[0]} .. {names[-1]}): vol {vol} each, faces {faces}, size {list(size)}, min corners {corners}")
        else:
            lines += ["+ " + _line(n, after[n]) for n in names]
    for n, a in after.items():
        b = before.get(n)
        if b is not None and b != a:
            lines.append(f"~ {n}: vol {b['volume']} -> " + _line(n, a).split(": vol ", 1)[1])
    gone = [n for n in before if n not in after]
    lines += [f"- {n} (gone)" for n in gone] if len(gone) <= 4 else [f"- {len(gone)} bodies gone: {', '.join(gone)}"]
    total = round(sum(a["volume"] or 0 for a in after.values()), 3)
    lines.append(f"bodies now: {len(after)}, total volume {total}" if lines else f"no geometry changed; bodies: {len(after)}, total volume {total}")
    return "\n".join(lines)


def find(word: str = "", limit: int = 20, width: int = 300) -> str:
    """Commands whose id, label or parameter docs contain ``word``: ``id (label): parameters``,
    each cut at ``width`` characters (``help(d.cmd.<area>.<name>)`` has the rest)."""
    w = word.lower()
    hits = [(i, label, p) for i, label, p in INDEX if w in i.lower() or w in label.lower() or w in p.lower()]
    hits.sort(key=lambda h: (w not in h[0].lower(), h[0]))
    out = "\n".join(f"{i} ({label}): {p if len(p) <= width else p[:width] + ' ...'}" for i, label, p in hits[:limit])
    more = len(hits) - limit
    return (out + (f"\n... {more} more; use a longer word" if more > 0 else "")) or f"no command matches {word!r}"


def headless(design: str | None = None, allow_paths: Sequence[str] = (), report: bool = False, cli: str | None = None, extra_args: Sequence[str] = ()) -> Session:
    """A session on its own headless SolidCraft (closed with the session)."""
    args = ["--headless"]
    if design:
        args += ["--in", design]
    for p in [os.getcwd(), *allow_paths]:
        args += ["--allow-path", p]
    return Session(Client([*args, *extra_args], cli=cli), report=report)


def connect(address: str, token: str | None = None, report: bool = False, cli: str | None = None) -> Session:
    """A session on a running SolidCraft window (its control channel at ``address``)."""
    args = ["--connect", address] + (["--token", token] if token else [])
    return Session(Client(args, cli=cli), report=report)


def session(report: bool | None = None) -> Session:
    """The session daemon's design when one runs (``solidcraft-session serve``), else a headless
    one. ``report`` (default: ``$SOLIDCRAFT_REPORT``, on with a daemon) prints what changed when
    the script ends."""
    from ._daemon import Remote, daemon_running

    if report is None:
        env = os.environ.get("SOLIDCRAFT_REPORT")
        report = env.strip().lower() in ("1", "true", "yes", "on") if env is not None else None
    if daemon_running():
        s = Session(Remote(), report=True if report is None else report, owns=False)
    else:
        s = headless(report=bool(report))
    atexit.register(s.close)
    return s


__all__ = ["Session", "SolidCraftError", "headless", "connect", "session", "find"]
