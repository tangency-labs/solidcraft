# SPDX-License-Identifier: MIT OR Apache-2.0
# The scx vocabulary (docs/mcp.md, "Script mode"): what a `run` script can call. It runs in the sandboxed
# interpreter before the agent's script and reaches SolidCraft only through
# `_call(tool, arguments_json)`, which the server answers with the tool's text, or raises
# RuntimeError with the tool's error. Ported from the scx.py prototype (docs/scx-hybrid, v15.1).
import json, math

pi = math.pi
sqrt = math.sqrt
sin = math.sin
cos = math.cos
tan = math.tan
radians = math.radians
degrees = math.degrees

LOG = []
LAST_BODY = None
NOTE_TARGET = False


def _fail(msg):
    raise RuntimeError(msg)


def _text(tool, args):
    return _call(tool, json.dumps(args))


def _cmd(cid, params):
    global LAST_BODY
    params = {k: _plain(v) for k, v in params.items() if v is not None}
    if params.get("operation") in ("join", "cut", "intersect") and not params.get("targets") and LAST_BODY:
        if LAST_BODY in measure():
            params["targets"] = [LAST_BODY]
            if NOTE_TARGET:
                LOG.append(f"({params['operation']} applied to {LAST_BODY}, the last body made or changed; into='Name' picks another)")
    t = _text("execute", {"command": cid, "params": params})
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


def _plain(v):
    if isinstance(v, Feature):
        return v.body
    if isinstance(v, Sketch):  # a sketch: finish it, pass its id
        v._finish()
        return v.id
    if isinstance(v, (list, tuple)):
        return [_plain(x) for x in v]
    if isinstance(v, dict):
        return {k: _plain(x) for k, x in v.items()}
    return v


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
        _fail(f"expected {n} coordinates, got {p}")
    return p


# ---------------------------------------------------------------- the vocabulary

OPS = {"new", "join", "cut", "intersect"}


def _op(op):
    if op not in OPS:
        _fail(f"op must be one of {sorted(OPS)}, got {op!r}")
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
    return Feature(_cmd(cid, p), label)


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
            _fail(f"hollow: say which body (body=...), there are {len(m)}: {', '.join(m)}")
        body = next(iter(m))
    lo, hi = m[body]["bbox"]
    c = [(a + b) / 2 for a, b in zip(lo, hi)]
    side = {"top": (2, hi), "bottom": (2, lo), "front": (1, lo), "back": (1, hi), "left": (0, lo), "right": (0, hi)}
    faces_ = []
    if open:
        if open not in side:
            _fail(f"hollow: open must be one of {sorted(side)} or None")
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
    return _cmd("solid.move", p)


def loft(sections, op="new", name=None, into=None):
    """Loft through sketches in order (each one closed profile)."""
    p = dict(sections=[{"sketch": _plain(x)} for x in sections], operation=_op(op), name=name, targets=[into] if into else None)
    if op == "new" and name:
        p["body_name"] = name
    return Feature(_cmd("solid.loft", p), "Loft")


def remove(bodies):
    """Delete bodies (helpers you no longer need)."""
    global LAST_BODY
    if isinstance(bodies, (str, Feature)):
        bodies = [bodies]
    bodies = [_plain(b) for b in bodies]
    if LAST_BODY in bodies:
        LAST_BODY = None
    return _cmd("solid.remove", dict(bodies=bodies))


def rename(body, new):
    return _cmd("body.rename", dict(body=body, name=new))


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
            _fail("plane must be 'XY' (u,v = x,y), 'XZ' (u,v = x,z) or 'YZ' (u,v = y,z)")
        dirs, axis, self.sign = PLANES[plane]
        origin = [0, 0, 0]
        origin[axis] = offset
        global _OPEN_SKETCH
        if _OPEN_SKETCH is not None:  # one sketch is edited at a time: close the previous one first
            _OPEN_SKETCH._finish()
        r = _cmd("sketch.create", dict(name=name, plane=dict(origin=origin, **dirs)))
        self._done = False
        _OPEN_SKETCH = self
        self.id = r.get("sketch") if isinstance(r, dict) else None
        self.name = name or (r.get("name") if isinstance(r, dict) else None)
        self.plane = plane

    def _c(self, cid, **p):
        return _cmd(cid, p)

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
            _fail(f"slot length {length} must be more than its width {width} (length is overall, tip to tip)")
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
                _cmd("sketch.finish", {})
            except RuntimeError as e:
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
        return Feature(_cmd("solid.extrude", p), "Extrude")

    def revolve(self, axis, angle=360, op="new", name=None, into=None):
        """Revolve about a world axis 'X'|'Y'|'Z' through the origin."""
        self._finish()
        p = dict(sketch=self.id, axis=axis, angle=f"{angle} deg", operation=_op(op), name=name, targets=[into] if into else None)
        if op == "new" and name:
            p["body_name"] = name
        return Feature(_cmd("solid.revolve", p), "Revolve")


def sketch(plane="XY", offset=0, name=None):
    """A sketch on XY, XZ or YZ, moved `offset` along the plane's normal axis (Z, Y, X)."""
    return Sketch(plane, offset, name)


class _Part:
    def __init__(self, name):
        r = _cmd("component.create", dict(name=name))
        self.name = name
        self.id = r.get("component") if isinstance(r, dict) else None

    def __enter__(self):
        return self

    def __exit__(self, *a):
        _cmd("component.activate", dict(component="root"))
        return False


def part(name):
    """A new part (component) that becomes active; use `with part('Lid'):` to return to the top level after."""
    return _Part(name)


def root():
    _cmd("component.activate", dict(component="root"))


def measure(body=None):
    """{name: {volume, bbox:[min,max], faces}} for every body, or one body."""
    if isinstance(body, Feature):
        body = body.body
    m = json.loads(_text("measure", {"summary": True}))
    out = {b["name"]: {"volume": num(b.get("volume_mm3")), "bbox": [num(b["bbox"]["min"]), num(b["bbox"]["max"])] if "bbox" in b else [None, None], "faces": b.get("faces")}
           for b in m.get("bodies", [])}
    return out[body] if body else out


def bodies():
    return list(measure().keys())


def edges(body):
    """Edges of a body: list of {name, mid, length}; pass e['mid'] to fillet/chamfer."""
    r = _cmd("model.edges", dict(body=body))
    return [{"name": e.get("name"), "mid": num(e.get("mid")), "length": num(e.get("length"))} for e in r.get("edges", [])]


def faces(body):
    """Faces of a body: list of {name, point, area, normal?, cylinder?}; pass f['point'] to shell/hole."""
    r = _cmd("model.faces", dict(body=body))
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
        t = _text("list_commands", {})
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
        return _cmd(command_id, params)
    except RuntimeError as e:
        doc = [c for c in _command_list() if c["id"] == command_id]
        raise RuntimeError(f"{e}\n{_cmd_line(doc[0])}" if doc else f"{e}\n(commands('word') lists ids)") from None


def new(name="Part"):
    return _text("new_design", {"name": name})


VOCAB = dict(box=box, cylinder=cylinder, cone=cone, sphere=sphere, hole=hole, fillet=fillet, chamfer=chamfer,
             shell=shell, hollow=hollow, combine=combine, move=move, remove=remove, rename=rename, sketch=sketch, part=part, root=root,
             measure=measure, bodies=bodies, edges=edges, faces=faces, cmd=cmd, new=new, loft=loft, commands=commands)


def _size(a):
    if a["bbox"][0] is None:  # a body SolidCraft could not measure
        return ()
    return num([round(q - p, 4) for p, q in zip(*a["bbox"])])


def part_map():
    """{body: part name} for bodies inside parts, and the number of parts."""
    try:
        comps = _cmd("component.list", {"compact": True, "bodies": True}).get("components", [])
    except (RuntimeError, AttributeError):
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


def _bline(n, a):
    return f"{n} (vol {a['volume']}, bbox {a['bbox'][0]}..{a['bbox'][1]}, faces {a['faces']})"


def _design():
    """Compact outline: parts with their bodies; identical parts grouped."""
    m = measure()
    try:
        comps = _cmd("component.list", {"compact": True, "bodies": True}).get("components", [])
    except (RuntimeError, AttributeError):
        comps = []
    root_c = next(iter([c for c in comps if c.get("parent") is None]), None)
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
    return "\n".join(lines)


_RUN = {}


def _begin(fresh):
    """Before the agent's script: a new design if asked, and what to compare the end with."""
    global _OPEN_SKETCH
    LOG.clear()
    notes = []
    if fresh:
        _OPEN_SKETCH = None
        new(fresh if isinstance(fresh, str) else "Part")
        notes.append("Started a new, empty design.")
    _RUN["notes"] = notes
    _RUN["before"] = measure()
    _RUN["parts"] = part_map()[1]


def _show(v):
    """A bare last-line value, as the reply prints it."""
    if v is None or isinstance(v, Feature):
        return None
    try:
        return json.dumps(num(v), separators=(",", ":"))[:4000]
    except Exception:
        return repr(v)[:4000]


def _name_hint(msg, bodies):
    """A NameError on a body's name almost always means the quotes were left off."""
    start = msg.find("'")
    end = msg.find("'", start + 1)
    if start < 0 or end < 0:
        return ""
    nm = msg[start + 1:end]
    if nm in bodies:
        return f"\nhint: {nm} is a body name, so quote it: '{nm}'. Variables do not carry over between runs; body names do."
    return f"\nhint: {nm} is not defined in this script. Each run starts with fresh variables: define it again, or refer to a body by its name in quotes."


def _end(printed, etype, emsg, line, last):
    """After the agent's script: what it printed, notes, the error and what changed."""
    out = list(_RUN.get("notes", []))
    if printed:
        out.append(printed.rstrip("\n"))
    shown = _show(last) if etype is None else None
    if shown is not None:
        out.append(shown)
    out += LOG
    after = measure()
    if etype is not None:
        if etype == "RuntimeError":
            err = f"error at line {line}: {hint_for(emsg)}"
        elif etype == "TimeoutError":
            err = f"error: the script computed for longer than its limit and was stopped ({emsg})"
        elif etype == "MemoryError":
            err = f"error at line {line}: MemoryError: the script asked for more memory than scripts get"
        else:
            err = f"error at line {line}: {etype}: {emsg}"
        if etype == "NameError":
            err += _name_hint(emsg, after)
        out.append(err + "\n(steps before the error were kept; fix and run only the rest)")
    pm, parts1 = part_map()
    out.append(summary(_RUN.get("before", {}), after, pm, (_RUN.get("parts"), parts1)))
    return "\n".join(out)
