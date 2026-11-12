#!/usr/bin/env python3
"""Check the W23 assembly sheet recipes (#11773) against the assembly they start from.

The hand counts are worked out again here from assembly.json itself, the command
script the native tests run, so the recipes can't drift from the assembly. This
is a static check of a written contract: it runs no SolidCraft command and never
reports a recipe as passed.
"""
import json
import re
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]

# Commands that change the model, refused while a drawing is open.
MODEL_COMMANDS = {"assembly.bom.renumber", "component.metadata.set", "selection.delete", "occurrence.move", "edit.undo", "edit.redo"}
IDS = ["W23-AS-03", "W23-AS-04", "W23-AS-05", "W23-AS-06"]


def require(ok, message):
    if not ok:
        raise ValueError(message)


def read(name):
    return json.loads((HERE / name).read_text(encoding="utf-8"))


class Assembly:
    """The placements assembly.json makes: which component sits how often in which parent."""

    def __init__(self, script):
        self.placed = {}  # name -> [(component, parent)] for every `as` step
        self.parents = {}  # component -> parent it was created in
        self.inside = Counter()  # (parent, component) -> placements
        self.part_numbers, self.descriptions = {}, {}
        self.shapes = {}  # component -> its bodies' rectangles seen from the top, (x0, y0, x1, y1)
        self.occurrences = {}  # parent -> [(name or None, component, (dx, dy))]
        self.active = None
        for step in script["commands"]:
            command, p = step["command"], step["params"]
            if command == "component.create":
                parent = p.get("parent", "root")
                self.parents[p["name"]] = parent
                self.inside[(parent, p["name"])] += 1
                placed = (p["name"], parent)
                self.occurrences.setdefault(parent, []).append((step.get("as"), p["name"], (0, 0)))
                self.active = p["name"] if p.get("activate", True) else None
            elif command == "occurrence.copy":
                parent = p.get("parent", "root")
                self.inside[(parent, p["component"])] += 1
                placed = (p["component"], parent)
                dx, dy = p.get("translate", [0, 0, 0])[:2]
                self.occurrences.setdefault(parent, []).append((step.get("as"), p["component"], (dx, dy)))
            elif command in ("solid.box", "solid.cylinder") and self.active is not None:
                if command == "solid.box":
                    x, y = p.get("corner", [0, 0, 0])[:2]
                    self.shapes.setdefault(self.active, []).append((x, y, x + p["length"], y + p["width"]))
                else:
                    (x, y), r = p["base"][:2], p["diameter"] / 2
                    self.shapes.setdefault(self.active, []).append((x - r, y - r, x + r, y + r))
                continue
            elif command == "component.activate":
                self.active = None
                continue
            elif command == "component.metadata.set":
                self.part_numbers[p["component"]] = p["part_number"]
                self.descriptions[p["part_number"]] = p["description"]
                continue
            else:
                continue
            if "as" in step:
                self.placed[step["as"]] = placed

    def top_view(self, source=None, hide=(), moved=None, deleted=None):
        """The size seen from the top of what a view draws: the occurrences under `source` (all if
        None) less those under `hide`, each a path of `as` names, with one named occurrence moved by
        `moved = (name, (dx, dy))` or `deleted`. None when nothing is left to draw."""
        boxes = []

        def walk(component, path, at):
            for x0, y0, x1, y1 in self.shapes.get(component, []):
                boxes.append((path, (x0 + at[0], y0 + at[1], x1 + at[0], y1 + at[1])))
            for name, child, (dx, dy) in self.occurrences.get(component, []):
                if name is not None and name == deleted:
                    continue
                if moved and name == moved[0]:
                    dx, dy = dx + moved[1][0], dy + moved[1][1]
                walk(child, path + (name,), (at[0] + dx, at[1] + dy))

        walk("root", (), (0, 0))
        under = lambda path, chosen: path[: len(chosen)] == tuple(chosen)
        drawn = [box for path, box in boxes if (source is None or any(under(path, c) for c in source)) and not any(under(path, h) for h in hide)]
        if not drawn:
            return None
        return [max(b[2] for b in drawn) - min(b[0] for b in drawn), max(b[3] for b in drawn) - min(b[1] for b in drawn)]

    def children(self, parent, without=Counter()):
        return Counter({c: n - without[(parent, c)] for (p, c), n in self.inside.items() if p == parent}) - Counter()

    def first_level(self, without=Counter()):
        return {self.part_numbers[c]: n for c, n in self.children("root", without).items()}

    def parts_only(self, without=Counter()):
        counts = Counter()

        def walk(component, times):
            inside = self.children(component, without)
            if not inside:
                counts[self.part_numbers[component]] += times
            for child, n in inside.items():
                walk(child, times * n)

        for child, n in self.children("root", without).items():
            walk(child, n)
        return dict(counts)


def main():
    data = read("cases.json")
    asm = Assembly(read(data["assembly"]))
    require(data["schema"] == "w23-assembly-sheet-acceptance/1", "unexpected schema")
    require(data["no_runtime_pass_claimed"] is True, "no recipe may claim a pass")
    cases = data["cases"]
    require([c["id"] for c in cases] == IDS, "recipes 3 to 6 of #11773, in order")
    require(all(c["runtime_status"] == "NOT_RUN" and c["waits_for"] for c in cases), "each recipe waits for what it needs")
    native = {name for tests in data["already_native"].values() for name in tests}
    for case in cases:
        require(case["native"] and set(case["native"]) <= native, f"{case['id']}: its native tests are listed in already_native")
    require(data["sheet"]["size"] == "ISO A2 landscape" and data["sheet"]["paper_mm"] == [594, 420], "an assembly's drawing starts on A2 (#12000)")

    # The hand counts, worked out again from the script.
    hand = data["hand_counts"]
    second = Counter({asm.placed["second_holder"][::-1]: 1})
    require(asm.parts_only() == hand["parts_only"], f"parts only: {asm.parts_only()}")
    require(asm.first_level() == hand["first_level"], f"first level: {asm.first_level()}")
    require(asm.parts_only(second) == hand["second_holder_deleted"]["parts_only"], "parts only without the second holder")
    require(asm.first_level(second) == hand["second_holder_deleted"]["first_level"], "first level without the second holder")
    require(any(n > 1 for n in asm.parts_only().values()), "at least one part used more than once")

    # Every name a recipe uses is one the script gives.
    names = set(asm.placed)
    for case in cases:
        for word in re.findall(r'"(first_holder|second_holder|block|plate|screw)"', json.dumps(case)):
            require(word in names, f"{case['id']}: {word} is not named in the script")

    # Model edits only with the drawing closed, then the drawing opened again before a check.
    for case in cases:
        state = "closed"
        for step in case["steps"]:
            command = step.get("do")
            if command in MODEL_COMMANDS:
                require(state == "closed", f"{case['id']}: {command} with the drawing open")
            if "from" in step or command in ("drawing.create", "drawing.open"):
                # A recipe continued from another starts where that one left the drawing: open.
                state = "open"
            elif command in ("drawing.close", "doc.open"):
                state = "closed"
            if "drawing" in step:
                require(step["drawing"] == state, f"{case['id']}: {command or step.get('pending')} says the drawing is {step['drawing']}")
            if "check" in step:
                require(state == "open" or command == "drawing.export", f"{case['id']}: a check with the drawing closed")
                require(step["check"] in case["expect"], f"{case['id']}: no expected values for {step['check']}")

    # W23-AS-03: the rows are the first level, numbered 1, 2, 3 in part-number order, with the
    # script's descriptions; the delete changes only the holder's quantity.
    rows = cases[0]["expect"]
    for key, counts in (("rows", asm.first_level()), ("rows_after_delete", asm.first_level(second))):
        listed = rows[key]
        require([r["part_number"] for r in listed] == sorted(counts), f"W23-AS-03 {key}: part numbers")
        require([r["item"] for r in listed] == [str(i) for i in range(1, len(listed) + 1)], f"W23-AS-03 {key}: items")
        for r in listed:
            require(r["quantity"] == counts[r["part_number"]], f"W23-AS-03 {key}: {r['part_number']} quantity")
            require(r["description"] == asm.descriptions[r["part_number"]], f"W23-AS-03 {key}: {r['part_number']} description")

    # W23-AS-04: a balloon picks the row its placement is counted in, the row of the longest member
    # path its occurrence path starts with (CamWight on #11881). Through a holder that's the holder's
    # first-level row, and with the holder a phantom the block's own. The item shown is that row's
    # number from the list; the deleted holder's balloon is lost, its row not found, never moved.
    holder, block = asm.part_numbers["Holder"], asm.part_numbers[asm.placed["block"][0]]
    holder_item = next(r["item"] for r in rows["rows"] if r["part_number"] == holder)
    balloons = cases[1]["expect"]
    for check, row in (("both_ok", holder), ("second_lost", holder), ("holder_phantom", block)):
        for name, through in (("first_balloon", "first_holder"), ("second_balloon", "second_holder")):
            b = balloons[check][name]
            require(b["occurrence"] == [through, "block"], f"W23-AS-04 {check}: {name} through its own holder")
            if b["status"] == "ok":
                require(b["row"] == row, f"W23-AS-04 {check}: {name} reads {row}'s row")
                require(b.get("item", holder_item if row == holder else None) == (holder_item if row == holder else None),
                        f"W23-AS-04 {check}: {name}'s item is its row's number")
    lost = balloons["second_lost"]["second_balloon"]
    require(lost["status"] == "lost" and lost["row"] == "not found", "W23-AS-04: lost, its row not found")
    require(lost["moved_to_first_holder"] is False and lost["removed"] is False, "W23-AS-04: never moved or removed")
    require(balloons["second_lost"]["first_balloon"]["status"] == "ok", "W23-AS-04: the first holder's balloon stays ok")
    phantom = [s for s in cases[1]["steps"] if s.get("do") == "component.metadata.set"]
    require(phantom == [{"do": "component.metadata.set", "params": {"component": "Holder", "bom_type": "phantom"}, "drawing": "closed"}],
            "W23-AS-04: the holder made a phantom, by a model command with the drawing closed")

    # W23-AS-05: three revisions, the second deleted, the next is still one past the highest.
    rev = cases[2]["expect"]
    made = [s for s in cases[2]["steps"] if s.get("do") == "drawing.revision.add"]
    require(rev["three_rows"]["revisions"] == [str(i) for i in range(1, len(made) + 1)], "W23-AS-05: numbered 1, 2, 3")
    left = [r for r in rev["three_rows"]["revisions"] if r != "2"]
    require(rev["after_delete"]["revisions"] == left and rev["after_delete"]["next"] == str(len(made) + 1), "W23-AS-05: 1 and 3, next 4")
    require(all(t["revision"] in left for t in rev["after_delete"]["tags"]), "W23-AS-05: a tag names a revision that is there")
    tagged = [s["params"] for s in cases[2]["steps"] if s.get("do") == "drawing.revision.tag.add"]
    require([(p["row"], p["at"]) for p in tagged] == [(f"revision {t['revision']}", t["at_mm"]) for t in rev["after_delete"]["tags"]],
            "W23-AS-05: each tag is added where the recipe expects it")
    require(cases[2]["expect"]["drawing_record_unchanged_by_the_model_edit"] is True, "W23-AS-05: unchanged by the model")

    # W23-AS-06: the same bytes before saving and after opening again.
    export = cases[3]["expect"]["same_export"]
    require(export["bytes_equal"] == ["before", "after"] and export["formats_now"] == ["svg"], "W23-AS-06: same SVG bytes")

    # Everything placed on paper fits the sheet: views, tables and tags.
    width, height = data["sheet"]["paper_mm"]
    placed = re.findall(r'"(?:at_mm|position_mm)": \[([0-9.-]+), ([0-9.-]+)\]', json.dumps(data))
    for case in cases:
        for step in case["steps"]:
            p = step.get("params", {})
            if step.get("do") == "drawing.table.add" and "place" in p.get("set", {}):
                placed.append(tuple(map(str, p["set"]["place"]["at"])))
            elif step.get("do") == "drawing.revision.tag.add":
                placed.append(tuple(map(str, p["at"])))
    require(len(placed) >= 4, "the view, the parts list and the tag are placed")
    for at in placed:
        x, y = map(float, at)
        require(0 <= x <= width and 0 <= y <= height, f"{at} is off the {data['sheet']['size']} sheet")

    # The saved views' sizes from the top at 1:1 (#11873), worked out again from the script.
    views = data["hand_views"]
    require(views["view"] == "top" and views["scale"] == "1:1", "the views are top views at 1:1")
    moved = ("second_holder", tuple(views["second_holder_moved"]))
    for view in views["views"]:
        pick = dict(source=view.get("source"), hide=view.get("hide", []))
        for key, kwargs in (("as_made", {}), ("second_holder_moved", {"moved": moved}), ("second_holder_deleted", {"deleted": "second_holder"})):
            want = view[key]
            got = asm.top_view(**pick, **kwargs)
            require(got == (None if want == "placeholder" else want), f"{view['name']}, {key}: {got}, not {want}")
    require(any(v[ "second_holder_deleted"] == "placeholder" for v in views["views"]), "one view draws only what gets deleted")
    require(set(views["native"]) <= native, "the views' native test is listed in already_native")

    # The native tests the recipes rely on are there, and the README names every recipe.
    for file, tests in data["already_native"].items():
        text = (ROOT / file).read_text(encoding="utf-8")
        for name in tests:
            require(re.search(r"#\[test\]\s*\nfn " + name + r"\(\)", text), f"{file}: no test {name}")
    readme = (HERE / "README.md").read_text(encoding="utf-8")
    require(all(i in readme for i in IDS), "the README names every recipe")

    print("STATIC CONTRACT PASS: recipes 3 to 6 match the hand counts worked out from assembly.json.")
    print("NATIVE: the engine part of each recipe is in", ", ".join(sorted(native)))
    print("RECIPES W23-AS-03 to W23-AS-06 on paper: NOT RUN (static only).")


if __name__ == "__main__":
    main()
