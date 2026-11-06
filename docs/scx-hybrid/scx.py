# SPDX-License-Identifier: MIT OR Apache-2.0
"""scx: a prototype "hybrid" MCP server for SolidCraft.

The model writes a short Python script using SolidCraft's own vocabulary (box, cylinder, sketch,
extrude, hole, fillet, part...). Each call runs immediately in SolidCraft through the official
`solidcraft-cli mcp` bridge. Every `run` ends with a compact numeric summary of what changed, so the
check is automatic, and can add a small picture.

Prototype for benchmarking only: the script runs with a restricted set of builtins in this process
(only `math` and `itertools` can be imported, no file access); it is NOT a security sandbox. Run it only
with models and prompts you trust. A real integration would use a sandboxed interpreter.

usage:
  python scx.py [--port 7879] [--look-default]        connect to a running SolidCraft (solidcraft --control 7879)
  python scx.py --headless [--in FILE] [--text-only]  start its own headless SolidCraft through solidcraft-cli
    --text-only   for clients that cannot show images: no pictures, no `look` tool
  SCX_CLI=path/to/solidcraft-cli   if solidcraft-cli is not on PATH
"""
import ast, json, math, subprocess, sys, traceback

import os
CLI = os.environ.get("SCX_CLI") or "solidcraft-cli"
PORT = 7879
HEADLESS = "--headless" in sys.argv
START_FILE = next((sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == "--in" and i + 1 < len(sys.argv)), None)
for i, a in enumerate(sys.argv):
    if a == "--port":
        PORT = int(sys.argv[i + 1])
LOOK_DEFAULT = "--look-default" in sys.argv
TEXT_ONLY = "--text-only" in sys.argv

# ---------------------------------------------------------------- SolidCraft bridge (child MCP)


class Bridge:
    def __init__(self):
        args = [CLI, "mcp", "--headless", "--allow-path", os.getcwd()] + (["--in", START_FILE] if START_FILE else []) if HEADLESS else [CLI, "mcp", "--connect", f"127.0.0.1:{PORT}"]
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, encoding="utf-8")
        self.n = 0
        self.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "scx", "version": "0"}})
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _send(self, m):
        self.p.stdin.write(json.dumps(m) + "\n")
        self.p.stdin.flush()

    def rpc(self, method, params):
        self.n += 1
        self._send({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params})
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("SolidCraft bridge closed")
            m = json.loads(line)
            if m.get("id") == self.n:
                return m

    def tool(self, name, args):
        r = self.rpc("tools/call", {"name": name, "arguments": args}).get("result", {})
        return r.get("content", []), r.get("isError", False)

    def text(self, name, args):
        content, err = self.tool(name, args)
        texts = [c.get("text", "") for c in content if c.get("type") == "text"]
        t = texts[0] if texts else ""
        if err:
            raise SCError(t)
        return t

    def cmd(self, cid, params):
        global LAST_BODY
        params = {k: _plain(v) for k, v in params.items() if v is not None}
        if params.get("operation") in ("join", "cut", "intersect") and not params.get("targets") and LAST_BODY:
            if LAST_BODY in measure():
                params["targets"] = [LAST_BODY]
                LOG.append(f"({params['operation']} applied to {LAST_BODY}, the last body made or changed; into='Name' picks another)") if NOTE_TARGET else None
        t = self.text("execute", {"command": cid, "params": params})
        try:
            r = json.loads(t)
        except ValueError:
            return t
        if isinstance(r, dict):
            made = r.get("created_bodies") or []
            changed = r.get("changed_bodies") or []
            want = params.get("body_name")
            if want and made and made[0] != want:
                LOG.append(f"note: a body named {want!r} already exists, so the new one is called {made[0]!r} (remove or rename the old one first)")
            if made or changed:
                LAST_BODY = (made or changed)[0]
        return r


B = None
LAST_BODY = None
NOTE_TARGET = False


def _plain(v):
    if isinstance(v, Feature):
        return v.body
    if hasattr(v, "_finish") and hasattr(v, "id"):  # a Sketch: finish it, pass its id
        v._finish()
        return v.id
    if isinstance(v, (list, tuple)):
        return [_plain(x) for x in v]
    if isinstance(v, dict):
        return {k: _plain(x) for k, x in v.items()}
    return v


def bridge():
    global B
    if B is None:
        B = Bridge()
    return B


class SCError(Exception):
    pass


# ---------------------------------------------------------------- hints for wrong names / common slips

ALIASES = {
    "solid.boolean": "use combine(target, tools, op='cut'|'join'|'intersect'), or op='cut' on the feature itself",
    "solid.cut": "give the feature op='cut' (box/cylinder/extrude...), or combine(target, tools, op='cut')",
    "solid.union": "combine(target, tools, op='join'), or op='join' on the feature",
    "solid.subtract": "combine(target, tools, op='cut')",
    "solid.pocket": "sketch(...).extrude(-depth, op='cut')",
    "sketch.circle": "s.circle(center, d=...)",
    "sketch.rectangle": "s.rect(p0, p1) or s.rect_center(center, w, h)",
    "sketch.polyline": "s.poly(points)",
    "sketch.polygon": "s.polygon(center, sides, r_vertex) or s.poly(points)",
    "sketch.arc": "s.poly / s.slot, or cmd('sketch.arc.center_point', center=, start=, end=)",
}


def hint_for(msg):
    for k, v in ALIASES.items():
        if f"`{k}`" in msg:
            return f"{msg}\nhint: {v}"
    for k in VOCAB:
        if f"unknown command `{k}`" in msg:
            return f"{msg}\nhint: {k} is a function here: call {k}(...) directly, not cmd('{k}')"
    if "there is no sketch" in msg:
        return msg + "\nhint: draw on a sketch first: s = sketch('XY'); s.rect(...); s.extrude(...)"
    return msg


def num(v):
    if isinstance(v, float):
        r = round(v, 4)
        return int(r) if r == int(r) else r
    if isinstance(v, (list, tuple)):
        return [num(x) for x in v]
    if isinstance(v, dict):
        return {k: num(x) for k, x in v.items()}
    return v


def xyz(p, n=3):
    if p is None:
        return None
    p = list(p)
    if len(p) != n:
        raise SCError(f"expected {n} coordinates, got {p}")
    return p


# ---------------------------------------------------------------- the vocabulary

OPS = {"new", "join", "cut", "intersect"}


def _op(op):
    if op not in OPS:
        raise SCError(f"op must be one of {sorted(OPS)}, got {op!r}")
    return op


class Feature:
    def __init__(self, reply, label):
        self.reply = reply if isinstance(reply, dict) else {}
        self.name = self.reply.get("name", label)
        self.created = self.reply.get("created_bodies") or []
        self.body = self.created[0] if self.created else (self.reply.get("changed_bodies") or [None])[0]
        w = self.reply.get("warning")
        if w:
            LOG.append(f"warning ({self.name}): {w}")

    def __repr__(self):
        return f"<{self.name} body={self.body}>"


def _feat(cid, label, **p):
    return Feature(bridge().cmd(cid, p), label)


def box(x, y, z, at=(0, 0, 0), center=None, op="new", name=None, into=None):
    """Box x*y*z. at = min corner; or center=[x,y,z] (center=True: `at` is the centre)."""
    p = dict(length=x, width=y, height=z, operation=_op(op), name=name, targets=[into] if into else None)
    if center is True:
        center = at
    if center is not None and center is not False:
        p["center"] = xyz(center)
    else:
        p["corner"] = xyz(at)
    if op == "new" and name:
        p["body_name"] = name
    return _feat("solid.box", "Box", **p)


def cylinder(h, d=None, r=None, at=(0, 0, 0), axis="Z", op="new", name=None, into=None):
    """Cylinder; at = centre of the base circle; axis X|Y|Z or [x,y,z]."""
    p = dict(height=h, diameter=d, radius=r, base=xyz(at), axis=axis, operation=_op(op), name=name, targets=[into] if into else None)
    if op == "new" and name:
        p["body_name"] = name
    return _feat("solid.cylinder", "Cylinder", **p)


def cone(h, d1, d2=0, at=(0, 0, 0), axis="Z", op="new", name=None, into=None):
    """Cone/frustum: base diameter d1 at `at`, top diameter d2 at distance h along axis."""
    p = dict(height=h, diameter=d1, top_diameter=d2, base=xyz(at), axis=axis, operation=_op(op), name=name, targets=[into] if into else None)
    if op == "new" and name:
        p["body_name"] = name
    return _feat("solid.cone", "Cone", **p)


def sphere(d=None, r=None, at=(0, 0, 0), op="new", name=None, into=None):
    p = dict(diameter=d, radius=r, center=xyz(at), operation=_op(op), name=name, targets=[into] if into else None)
    if op == "new" and name:
        p["body_name"] = name
    return _feat("solid.sphere", "Sphere", **p)


def hole(at, d, depth=None, direction=None, name=None):
    """Drill a hole of diameter d at a point ON a face, into the material. depth=None: through everything in its path."""
    return _feat("solid.hole", "Hole", position=xyz(at), diameter=d, depth=depth, direction=direction, name=name)


def fillet(edges, r, name=None):
    """Round edges. edges: list of points on the edges, e.g. an edge's 'mid' from edges(body)."""
    return _feat("solid.fillet", "Fillet", edges=[xyz(e) for e in edges], radius=r, name=name)


def chamfer(edges, d, name=None):
    return _feat("solid.chamfer", "Chamfer", edges=[xyz(e) for e in edges], distance=d, name=name)


def shell(open_faces, t, body=None, name=None):
    """Hollow a body to wall thickness t, removing the faces at the given points (a point on each face)."""
    return _feat("solid.shell", "Shell", faces=[xyz(f) for f in open_faces], thickness=t, body=body, name=name)


def hollow(t, open="top", body=None, name=None):
    """Hollow a body to wall thickness t, leaving one side open: open='top'|'bottom'|'front'(-Y)|'back'(+Y)|'left'(-X)|'right'(+X)|None (closed)."""
    m = measure()
    if body is None:
        if len(m) != 1:
            raise SCError(f"hollow: say which body (body=...), there are {len(m)}: {', '.join(m)}")
        body = next(iter(m))
    lo, hi = m[body]["bbox"]
    c = [(a + b) / 2 for a, b in zip(lo, hi)]
    side = {"top": (2, hi), "bottom": (2, lo), "front": (1, lo), "back": (1, hi), "left": (0, lo), "right": (0, hi)}
    faces_ = []
    if open:
        if open not in side:
            raise SCError(f"hollow: open must be one of {sorted(side)} or None")
        ax, ext = side[open]
        pt = list(c)
        pt[ax] = ext[ax]
        faces_ = [pt]
    return _feat("solid.shell", "Shell", faces=faces_, thickness=t, body=body, name=name)


def combine(target, tools, op="cut", keep_tools=False, name=None):
    return _feat("solid.combine", "Combine", target=target, tools=list(tools), operation=_op(op), keep_tools=keep_tools, name=name)


def move(bodies, by=None, rotate=None, copy=False):
    """Move (or copy) bodies by a vector and/or rotate them about a world axis through the origin: rotate=('Z', 120)."""
    if isinstance(bodies, (str, Feature)):
        bodies = [bodies]
    p = dict(bodies=[_plain(b) for b in bodies], copy=copy)
    if by is not None:
        p["translate"] = xyz(by)
    if rotate is not None:
        ax, ang = rotate
        p["axis"] = {"X": [1, 0, 0], "Y": [0, 1, 0], "Z": [0, 0, 1]}[ax.upper()] if isinstance(ax, str) else xyz(ax)
        p["angle"] = f"{ang} deg"
    return bridge().cmd("solid.move", p)


def loft(sections, op="new", name=None, into=None):
    """Loft through sketches in order (each one closed profile)."""
    p = dict(sections=[{"sketch": _plain(x)} for x in sections], operation=_op(op), name=name, targets=[into] if into else None)
    if op == "new" and name:
        p["body_name"] = name
    return Feature(bridge().cmd("solid.loft", p), "Loft")


def remove(bodies):
    """Delete bodies (helpers you no longer need)."""
    global LAST_BODY
    if isinstance(bodies, (str, Feature)):
        bodies = [bodies]
    bodies = [_plain(b) for b in bodies]
    if LAST_BODY in bodies:
        LAST_BODY = None
    return bridge().cmd("solid.remove", dict(bodies=bodies))


def rename(body, new):
    return bridge().cmd("body.rename", dict(body=body, name=new))


PLANES = {
    # sketch (u, v) -> world;  normal = + the remaining axis
    "XY": ({"x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, 2, 1),
    "XZ": ({"x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, 1, -1),   # SC normal is -Y: flip extrude sign
    "YZ": ({"x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, 0, 1),
}


_OPEN_SKETCH = None


class Sketch:
    def __init__(self, plane="XY", offset=0, name=None):
        if plane not in PLANES:
            raise SCError("plane must be 'XY' (u,v = x,y), 'XZ' (u,v = x,z) or 'YZ' (u,v = y,z)")
        dirs, axis, self.sign = PLANES[plane]
        origin = [0, 0, 0]
        origin[axis] = offset
        global _OPEN_SKETCH
        if _OPEN_SKETCH is not None:  # one sketch is edited at a time: close the previous one first
            _OPEN_SKETCH._finish()
        r = bridge().cmd("sketch.create", dict(name=name, plane=dict(origin=origin, **dirs)))
        self._done = False
        _OPEN_SKETCH = self
        self.id = r.get("sketch") if isinstance(r, dict) else None
        self.name = name or (r.get("name") if isinstance(r, dict) else None)
        self.plane = plane

    def _c(self, cid, **p):
        return bridge().cmd(cid, p)

    def rect(self, p0, p1):
        self._c("sketch.rectangle.two_point", p0=xyz(p0, 2), p1=xyz(p1, 2)); return self

    def rect_center(self, c, w, h):
        c = xyz(c, 2)
        self._c("sketch.rectangle.center", center=c, corner=[c[0] + w / 2, c[1] + h / 2]); return self

    def circle(self, c, d=None, r=None):
        self._c("sketch.circle.center", center=xyz(c, 2), diameter=d, radius=r); return self

    def slot(self, center, length, width, angle=0):
        """Slot with fully round ends: centre point, OVERALL length (tip to tip), width; angle in degrees from the u axis."""
        c = xyz(center, 2)
        if length <= width:
            raise SCError(f"slot length {length} must be more than its width {width} (length is overall, tip to tip)")
        h = (length - width) / 2
        dx, dy = h * math.cos(math.radians(angle)), h * math.sin(math.radians(angle))
        self._c("sketch.slot.center_to_center", p0=[c[0] - dx, c[1] - dy], p1=[c[0] + dx, c[1] + dy], width=width); return self

    def polygon(self, c, sides, r_vertex, angle=0):
        self._c("sketch.polygon.inscribed", center=xyz(c, 2), radius=r_vertex, sides=sides, angle=angle); return self

    def poly(self, points):
        """Closed outline through the points."""
        self._c("sketch.line", points=[xyz(p, 2) for p in points], closed=True); return self

    def _finish(self):
        global _OPEN_SKETCH
        if self._done:
            return
        self._done = True
        if _OPEN_SKETCH is self:
            _OPEN_SKETCH = None
            try:
                bridge().cmd("sketch.finish", {})
            except SCError as e:
                if "not available" not in str(e):  # already closed by the app: fine
                    raise

    def extrude(self, dist, op="new", symmetric=False, name=None, into=None):
        """Extrude every closed region (a circle inside a rectangle makes a hole). +dist goes along +Z (XY), +Y (XZ), +X (YZ)."""
        self._finish()
        p = dict(sketch=self.id, distance=dist * self.sign, operation=_op(op), name=name, targets=[into] if into else None)
        if symmetric:
            p["direction"] = "symmetric"
        if op == "new" and name:
            p["body_name"] = name
        return Feature(bridge().cmd("solid.extrude", p), "Extrude")

    def revolve(self, axis, angle=360, op="new", name=None, into=None):
        """Revolve about a world axis 'X'|'Y'|'Z' through the origin."""
        self._finish()
        p = dict(sketch=self.id, axis=axis, angle=f"{angle} deg", operation=_op(op), name=name, targets=[into] if into else None)
        if op == "new" and name:
            p["body_name"] = name
        return Feature(bridge().cmd("solid.revolve", p), "Revolve")


def sketch(plane="XY", offset=0, name=None):
    """A sketch on XY, XZ or YZ, moved `offset` along the plane's normal axis (Z, Y, X)."""
    return Sketch(plane, offset, name)


class _Part:
    def __init__(self, name):
        r = bridge().cmd("component.create", dict(name=name))
        self.name = name
        self.id = r.get("component") if isinstance(r, dict) else None

    def __enter__(self):
        return self

    def __exit__(self, *a):
        bridge().cmd("component.activate", dict(component="root"))
        return False


def part(name):
    """A new part (component) that becomes active; use `with part('Lid'):` to return to the top level after."""
    return _Part(name)


def root():
    bridge().cmd("component.activate", dict(component="root"))


def measure(body=None):
    """{name: {volume, bbox:[min,max], faces}} for every body, or one body."""
    if isinstance(body, Feature):
        body = body.body
    m = json.loads(bridge().text("measure", {"summary": True}))
    out = {b["name"]: {"volume": num(b.get("volume_mm3")), "bbox": [num(b["bbox"]["min"]), num(b["bbox"]["max"])] if "bbox" in b else [None, None], "faces": b.get("faces")}
           for b in m.get("bodies", [])}
    return out[body] if body else out


def bodies():
    return list(measure().keys())


def edges(body):
    """Edges of a body: list of {name, mid, length}; pass e['mid'] to fillet/chamfer."""
    r = bridge().cmd("model.edges", dict(body=body))
    return [{"name": e.get("name"), "mid": num(e.get("mid")), "length": num(e.get("length"))} for e in r.get("edges", [])]


def faces(body):
    """Faces of a body: list of {name, point, area, normal?, cylinder?}; pass f['point'] to shell/hole."""
    r = bridge().cmd("model.faces", dict(body=body))
    out = []
    for f in r.get("faces", []):
        d = {"name": f.get("name"), "point": num(f.get("point")), "area": num(f.get("area"))}
        if f.get("plane_normal"):
            d["normal"] = num(f["plane_normal"])
        if f.get("cylinder"):
            c = f["cylinder"]
            d["cylinder"] = {"origin": num(c.get("origin")), "axis": num(c.get("axis")), "r": num(c.get("radius"))}
        out.append(d)
    return out


_CMDS = None


def _command_list():
    global _CMDS
    if _CMDS is None:
        t = bridge().text("list_commands", {})
        t = t[t.find("["):] if "[" in t else "[]"
        try:
            _CMDS = [c for c in json.loads(t) if isinstance(c, dict) and c.get("id")]
        except ValueError:
            _CMDS = []
    return _CMDS


def _cmd_line(c, cap=500):
    p = str(c.get('params') or 'no params')
    return f"{c['id']} ({c.get('label', '')}): " + (p if len(p) <= cap else p[:cap] + " ...")


def commands(word=""):
    """SolidCraft command ids whose id or label contains `word`, with their parameters."""
    w = word.lower()
    hits = [c for c in _command_list() if w in c["id"].lower() or w in str(c.get("label", "")).lower()]
    if not hits:
        return f"no command matches {word!r}"
    out = "\n".join(_cmd_line(c) for c in hits[:12])
    return out + (f"\n... {len(hits) - 12} more; use a longer word" if len(hits) > 12 else "")


def cmd(command_id, **params):
    """Any SolidCraft command by id, e.g. cmd('solid.pattern.circular', features=['Hole1'], axis='Z', count=6)."""
    try:
        return bridge().cmd(command_id, params)
    except SCError as e:
        doc = [c for c in _command_list() if c["id"] == command_id]
        raise SCError(f"{e}\n{_cmd_line(doc[0])}" if doc else f"{e}\n(commands('word') lists ids)") from None


def new(name="Part"):
    return bridge().text("new_design", {"name": name})


VOCAB = dict(box=box, cylinder=cylinder, cone=cone, sphere=sphere, hole=hole, fillet=fillet, chamfer=chamfer,
             shell=shell, hollow=hollow, combine=combine, move=move, remove=remove, rename=rename, sketch=sketch, part=part, root=root,
             measure=measure, bodies=bodies, edges=edges, faces=faces, cmd=cmd, new=new, loft=loft, commands=commands)
SAFE_BUILTINS = {k: __builtins__[k] if isinstance(__builtins__, dict) else getattr(__builtins__, k) for k in (
    "abs", "min", "max", "range", "len", "round", "sum", "enumerate", "zip", "list", "dict", "tuple", "float",
    "int", "str", "sorted", "reversed", "isinstance", "any", "all", "map", "filter", "bool", "set", "Exception", "ValueError",
    "dir", "repr", "hasattr", "getattr", "type")}

import itertools
_IMPORTABLE = {"math": math, "itertools": itertools}


def _import(name, globals=None, locals=None, fromlist=(), level=0):
    if name in _IMPORTABLE:
        return _IMPORTABLE[name]
    raise ImportError(f"only math and itertools can be imported here; the CAD vocabulary needs no import")


SAFE_BUILTINS["__import__"] = _import

LOG = []

# ---------------------------------------------------------------- tools


def _size(a):
    if a["bbox"][0] is None:  # a body SolidCraft could not measure
        return ()
    return num([round(q - p, 4) for p, q in zip(*a["bbox"])])


def part_map():
    """{body: part name} for bodies inside parts, and the number of parts."""
    try:
        comps = bridge().cmd("component.list", {"compact": True, "bodies": True}).get("components", [])
    except (SCError, AttributeError):
        return {}, 0
    pm = {b: c["name"] for c in comps if c.get("parent") is not None for b in c.get("bodies", [])}
    return pm, sum(1 for c in comps if c.get("parent") is not None)


def _in(pm, names):
    ps = [pm[n] for n in names if n in pm]
    if not ps:
        return ""
    if len(ps) == 1:
        return f"  part {ps[0]}"
    return f"  parts {ps[0]} .. {ps[-1]}" if len(set(ps)) == len(ps) else f"  parts {', '.join(sorted(set(ps)))}"


def summary(before, after, pm=None, nparts=(None, None)):
    pm = pm or {}
    lines = []
    new = [n for n in after if n not in before]
    groups = {}
    for n in new:
        a = after[n]
        groups.setdefault((a["volume"], a["faces"], tuple(_size(a))), []).append(n)
    for (vol, fc, size), names in groups.items():
        if len(names) >= 3:
            corners = " ".join(str(after[n]["bbox"][0]) for n in names)
            lines.append(f"+ {len(names)} alike ({names[0]} .. {names[-1]}): vol {vol} each, faces {fc}, size {list(size)}, min corners {corners}{_in(pm, names)}")
        else:
            for n in names:
                a = after[n]
                lines.append(f"+ {n}: vol {a['volume']}  bbox {a['bbox'][0]}..{a['bbox'][1]}  faces {a['faces']}{_in(pm, [n])}")
    for n, a in after.items():
        b = before.get(n)
        bb = f"{a['bbox'][0]}..{a['bbox'][1]}"
        if b is None:
            continue
        elif b != a:
            lines.append(f"~ {n}: vol {b['volume']} -> {a['volume']}  bbox {bb}  faces {a['faces']}{_in(pm, [n])}")
    gone = [n for n in before if n not in after]
    if len(gone) > 4:
        lines.append(f"- {len(gone)} bodies gone: {', '.join(gone)}")
    else:
        lines += [f"- {n} (gone)" for n in gone]
    total = round(sum(a["volume"] or 0 for a in after.values()), 3)
    if nparts[0] != nparts[1]:
        lines.append(f"parts: {nparts[0]} -> {nparts[1]}")
    lines.append(f"bodies now: {len(after)}, total volume {total}" if lines else f"no geometry changed; bodies: {len(after)}, total volume {total}")
    return "\n".join(lines)


def picture(view="iso", size=384):
    if TEXT_ONLY:
        return []
    content, err = bridge().tool("screenshot", {"source": "model", "view": view, "width": size, "height": int(size * 0.75)})
    return [c for c in content if c.get("type") == "image"]


def name_hint(e, bodies):
    """A NameError on a body's name almost always means the quotes were left off."""
    nm = getattr(e, "name", None)
    if not nm:
        return ""
    if nm in bodies:
        return f"\nhint: {nm} is a body name, so quote it: '{nm}'. Variables do not carry over between runs; body names do."
    return f"\nhint: {nm} is not defined in this script. Each run starts with fresh variables: define it again, or refer to a body by its name in quotes."


def tool_run(args):
    code = next((args[k] for k in ("code", "script", "command", "source", "python") if isinstance(args.get(k), str)), None)
    if code is None:
        return [{"type": "text", "text": "run needs the script in the `code` argument: run {\"code\": \"box(10, 10, 10)\"}"}], True
    look = args.get("look", LOOK_DEFAULT)
    LOG.clear()
    printed = []
    fresh = args.get("new")
    if fresh:
        global _OPEN_SKETCH
        _OPEN_SKETCH = None
        new(fresh if isinstance(fresh, str) else "Part")
        printed.append("Started a new, empty design.")
    env = dict(VOCAB)
    env.update(math=math, pi=math.pi, sqrt=math.sqrt, sin=math.sin, cos=math.cos, tan=math.tan, radians=math.radians,
               degrees=math.degrees, __builtins__=dict(SAFE_BUILTINS, print=lambda *a, **k: printed.append(" ".join(str(x) for x in a))))
    before = measure()
    parts0 = part_map()[1]
    err = None
    name_err = None
    try:
        tree = ast.parse(code, "<script>")
        if tree.body and isinstance(tree.body[-1], ast.Expr):
            last = tree.body[-1]
            tree.body[-1] = ast.copy_location(ast.Assign(targets=[ast.copy_location(ast.Name("__last__", ast.Store()), last)], value=last.value), last)
            ast.fix_missing_locations(tree)
        exec(compile(tree, "<script>", "exec"), env)
        lv = env.get("__last__")
        if lv is not None and not isinstance(lv, Feature):
            printed.append(json.dumps(num(lv), separators=(",", ":"), default=str)[:4000])
    except SCError as e:
        line = next((f.lineno for f in reversed(traceback.extract_tb(e.__traceback__)) if f.filename == "<script>"), "?")
        err = f"error at line {line}: {hint_for(str(e))}"
    except Exception as e:
        line = next((f.lineno for f in reversed(traceback.extract_tb(e.__traceback__)) if f.filename == "<script>"), getattr(e, "lineno", "?"))
        err = f"error at line {line}: {type(e).__name__}: {e}"
        name_err = e if isinstance(e, NameError) else None
    after = measure()
    if name_err is not None:
        err += name_hint(name_err, after)
    parts = []
    if printed:
        parts.append("\n".join(printed))
    parts += LOG
    if err:
        parts.append(err + "\n(steps before the error were kept; fix and run only the rest)")
    pm, parts1 = part_map()
    parts.append(summary(before, after, pm, (parts0, parts1)))
    content = [{"type": "text", "text": "\n".join(parts)}]
    if look and not err and not TEXT_ONLY:
        content += picture()
    return content, err is not None


def _bline(n, a):
    return f"{n} (vol {a['volume']}, bbox {a['bbox'][0]}..{a['bbox'][1]}, faces {a['faces']})"


def tool_design(args):
    """Compact outline: parts with their bodies; identical parts grouped."""
    m = measure()
    try:
        comps = bridge().cmd("component.list", {"compact": True, "bodies": True}).get("components", [])
    except (SCError, AttributeError):
        comps = []
    root_c = next((c for c in comps if c.get("parent") is None), None)
    lines = [f"{len(m)} bodies, {max(len(comps) - 1, 0)} parts, total volume {round(sum(a['volume'] or 0 for a in m.values()), 3)}"]
    if root_c and root_c.get("bodies"):
        lines.append("top level: " + "; ".join(_bline(b, m[b]) for b in root_c["bodies"] if b in m))
    groups = {}
    for c in comps:
        if c is root_c:
            continue
        bs = [b for b in c.get("bodies", []) if b in m]
        sig = tuple((m[b]["volume"], m[b]["faces"], tuple(_size(m[b]))) for b in bs)
        groups.setdefault(sig, []).append((c["name"], bs))
    for sig, members in groups.items():
        if len(members) >= 3:
            sizes = "; ".join(f"vol {v}, faces {f}, size {list(sz)}" for v, f, sz in sig) or "no bodies"
            corners = " ".join(f"{n}:{m[bs[0]]['bbox'][0]}" if bs else n for n, bs in members)
            lines.append(f"{len(members)} alike parts ({members[0][0]} .. {members[-1][0]}), each: {sizes}; min corners {corners}")
        else:
            for n, bs in members:
                lines.append(f"part {n}: " + ("; ".join(_bline(b, m[b]) for b in bs) or "no bodies"))
    if not comps:
        lines += [_bline(n, a) for n, a in m.items()]
    return [{"type": "text", "text": "\n".join(lines)}], False


def tool_look(args):
    imgs = picture(args.get("view", "iso"), int(args.get("size", 384)))
    return imgs or [{"type": "text", "text": "no picture"}], not imgs


INSTRUCTIONS = """SolidCraft CAD. Units mm, Z up. Send Python scripts to `run` as {"code": "..."}; do the whole part in one script when you can; each line acts at once. Every reply ends with what changed: + new body, ~ changed body (volume before -> after, bbox), - removed. Check them; fix with another `run` (steps before an error stay). A bare last-line expression is printed.
op='new' makes a separate body; 'join'/'cut'/'intersect' act on the last body made or changed (into='Body' picks another). Name bodies (name=). Use loops for repeats.

Vocabulary (math, pi, sqrt, sin, cos ready):
  box(x, y, z, at=min corner | center=(..), op=, name=, into=)
  cylinder(h, d=|r=, at=base centre, axis='Z'|'X'|'Y', op=, name=)   cone(h, d1, d2=0, at=, axis=, op=)   sphere(d=|r=, at=, op=)
  hole(at=point ON a face, d, depth=None (through), direction=None (into the face))
  s = sketch('XY'|'XZ'|'YZ', offset=0)   # u,v = (x,y)|(x,z)|(y,z); offset along Z|Y|X
    s.rect(p0, p1) s.rect_center(c, w, h) s.circle(c, d=) s.slot(center, length, width, angle=0) s.polygon(c, sides, r_vertex) s.poly([pts])
    s.extrude(dist, op=, symmetric=False, name=)   # +dist along +Z|+Y|+X; circle in rect = hole
    s.revolve(axis='X'|'Y'|'Z', angle=360, op=)    # about a world axis
  loft([sketches], op=, name=)
  fillet([points on edges], r)   chamfer([points on edges], d)   # edge point: edges(body)[i]['mid']
  hollow(t, open='top'|'bottom'|'front'|...|None, body=)   # or shell([point on each open face], t)
  combine(target, [tools], op='cut'|'join'|'intersect')   move(bodies, by=(dx,dy,dz), rotate=('Z', deg), copy=False)   remove(bodies)  rename(body, new)
  with part('Name'): ...   # its features go in that part
  measure() -> {body: {volume, bbox, faces}}   edges(body) -> [{name, mid, length}]   faces(body) -> [{name, point}]
  cmd('command.id', **params) for anything else; commands('word') finds ids.
run(..., look=True) adds a picture.

Example:
  box(60, 40, 8, name='Plate')
  for x, y in [(8, 8), (52, 8), (8, 32), (52, 32)]:
      hole(at=(x, y, 8), d=6)
"""

TOOLS = [
    {"name": "run", "description": "Run a Python build script in SolidCraft (vocabulary in the server instructions). Returns printed output, errors with line numbers and hints, and what changed (bodies, volumes, bboxes).",
     "inputSchema": {"type": "object", "properties": {"code": {"type": "string", "description": "Python script"},
                                                       "look": {"type": "boolean", "description": "Add a small picture after it runs"},
                                                       "new": {"type": ["boolean", "string"], "description": "true (or a name): start a new, empty design before the script runs"}},
                     "required": ["code"]}},
    {"name": "design", "description": "Outline of the design: every body with volume, bbox and faces, and the parts.",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "look", "description": "Small picture of the model.",
     "inputSchema": {"type": "object", "properties": {"view": {"type": "string", "description": "iso|front|top|right|back|left|bottom"},
                                                       "size": {"type": "integer"}}}},
]
HANDLERS = {"run": tool_run, "design": tool_design, "look": tool_look}
if TEXT_ONLY:
    TOOLS = [t for t in TOOLS if t["name"] != "look"]
    TOOLS[0]["inputSchema"]["properties"].pop("look", None)
    HANDLERS.pop("look")
    INSTRUCTIONS = INSTRUCTIONS.replace("run(..., look=True) adds a picture.\n", "")


def main():
    sys.stdin.reconfigure(encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8")
    out = sys.stdout
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        m = json.loads(line)
        mid, method = m.get("id"), m.get("method")
        if mid is None:
            continue
        if method == "initialize":
            res = {"protocolVersion": m["params"].get("protocolVersion", "2025-06-18"), "capabilities": {"tools": {}},
                   "serverInfo": {"name": "scx", "version": "0.7.1"}, "instructions": INSTRUCTIONS}
        elif method == "tools/list":
            res = {"tools": TOOLS}
        elif method == "tools/call":
            name = m["params"]["name"]
            try:
                if name not in HANDLERS:
                    r = bridge().rpc("tools/call", m["params"]).get("result", {})
                    content, is_err = r.get("content", []), r.get("isError", False)
                else:
                    content, is_err = HANDLERS[name](m["params"].get("arguments") or {})
            except Exception as e:
                content, is_err = [{"type": "text", "text": f"{type(e).__name__}: {e}"}], True
            res = {"content": content, "isError": is_err}
        elif method == "ping":
            res = {}
        else:
            out.write(json.dumps({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "unknown method"}}) + "\n")
            out.flush()
            continue
        out.write(json.dumps({"jsonrpc": "2.0", "id": mid, "result": res}) + "\n")
        out.flush()


if __name__ == "__main__":
    main()
