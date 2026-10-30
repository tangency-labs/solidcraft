#!/usr/bin/env python3
"""Check the static W23a drawing-update contract against real oracle files.

This is deliberately NOT a SolidCraft W13/W14 integration test. It must
never report drawing updates as passed: no application command is run here.
"""
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent


def read(path):
    return json.loads((ROOT / path).read_text(encoding="utf-8"))


def require(ok, message):
    if not ok:
        raise ValueError(message)


def case(name):
    return "tests/oracle/" + name


def main():
    data = json.loads((HERE / "cases.json").read_text(encoding="utf-8"))
    require(data["schema"] == "w23a-drawing-update-acceptance/1", "unexpected schema")
    require(data["no_runtime_pass_claimed"] is True, "no runtime PASS allowed")
    items = data["cases"]
    require(len(items) == 4, "exactly CamWight's four agreed E2E scenarios")
    require([c["id"] for c in items] == [f"W23A-E2E-{i:02}" for i in range(1, 5)], "case identities changed")
    require(all(c["runtime_status"] == "NOT_RUN" for c in items), "E2E cases are not runnable yet")

    source = data["source_data"]
    baseline = source["baseline"]
    require(baseline["source_oracle"] == case("84-plate-two-bores"), "baseline links wrong oracle")
    require(baseline["plate_mm"] == [100, 80, 20], "wrong plate size")
    require({b["feature"] for b in baseline["bores"]} == {"Bore1", "Bore2"}, "not two independent bores")
    features = read(case("84-plate-two-bores") + "/recipe.json")["features"]
    model_holes = {f["name"]: f for f in features if f["op"] == "hole"}
    require(set(model_holes) == {"Bore1", "Bore2"}, "baseline model feature identity")
    for bore in baseline["bores"]:
        hole = model_holes[bore["feature"]]
        require(hole["position"][:2] == bore["center_xy_mm"], "bore centre mismatch")
        require(hole["diameter"] == bore["feature"].lower() + "_d", "bore driver mismatch")
    a, b = baseline["bores"]
    require(math.hypot(a["center_xy_mm"][0] - b["center_xy_mm"][0],
                       a["center_xy_mm"][1] - b["center_xy_mm"][1]) == 50, "centre separation must be 50 mm")
    require(source["drawing_future"]["scale"] == "1:2", "wrong drawing scale")
    labels = source["drawing_future"]["labels"]
    require(len(labels) == len({x["symbolic"] for x in labels}) == 4, "labels must have unique symbolic handles")
    require(all(0 <= x["paper_xy_mm"][0] <= 420 and 0 <= x["paper_xy_mm"][1] <= 297 for x in labels), "labels outside A3")

    for state in ("84-plate-two-bores", "85-plate-width-100",
                  "86-plate-first-bore-12", "87-plate-bore1-deleted"):
        recipe = read(case(state) + "/recipe.json")
        expected = read(case(state) + "/expected.json")
        require(recipe["id"] == state and expected["source"] == "analytic", "wrong oracle metadata")
        require(expected["body_count"] == 1 and expected["volume_mm3"] > 0, "oracle reference missing")
    require(abs(read(case("84-plate-two-bores") + "/expected.json")["volume_mm3"]
                - baseline["volume_mm3"]) < 1e-9, "baseline volume mismatch")
    for item in items[:2]:
        expect = read(item["source_expected"]["oracle"] + "/expected.json")
        require(abs(item["source_expected"]["volume_mm3"] - expect["volume_mm3"]) < 1e-9,
                item["id"] + " volume mismatch")
        require(item["starts_from"] == "84-plate-two-bores", "scenarios must start independently")
    require(items[0]["source_actions"] == [{
        "command": "parameters.change",
        "params": {"name": "bore1_d", "expression": "12 mm"},
        "session": "design",
    }], "Ø12 edit must be an actual design command")
    require(items[1]["source_actions"] == [{
        "command": "timeline.delete",
        "params": {"feature": "Bore1"},
        "session": "design",
    }], "delete must name Bore1")
    deleted = read(case("87-plate-bore1-deleted") + "/recipe.json")
    kept = [f["name"] for f in deleted["features"] if f["op"] == "hole"]
    require(kept == ["Bore2"], "original Bore2 must retain identity after deletion")
    # Inspect must use W19d's real machine-readable statuses, not UI prose.
    require(all("R1" in item["dependencies"] for item in items), "R1 flat-face width dependency missing")
    require(source["drawing_future"]["labels"][2]["reference_intent"].startswith("R2:"),
            "centre-to-centre dimension must use R2 hole axes")
    require(source["drawing_future"]["labels"][3]["reference_intent"].startswith("R1:"),
            "plate-width dimension must use R1 opposing faces")
    require("view centre" in source["drawing_future"]["paper_to_view_text_at_rule"],
            "dimension text.at needs actual view-relative coordinates")
    for item in items:
        require("W19d" in item["dependencies"] and "W20a" in item["dependencies"],
                "precise dimension slices missing")
    for item in items[:2]:
        early = item["early_dimension_inspect"]
        require(early["runtime_status"] == "NOT_RUN" and early["requires"] == ["W19d", "W20a"]
                and "U2" in early["does_not_require"], "early dimension inspect subset must not claim runtime success")
        require(item["drawing_expected"]["dimension_inspect_after_source_edit"] == "immediate_model_read_without_U2",
                "dimension inspect reads the current model, not deferred view publication")
        for symbolic in ("diam_bore1", "diam_bore2", "centres_1_2", "plate_width"):
            require(item["drawing_expected"][symbolic]["status"] in ("ok", "lost"),
                    "expected dimension status must match drawing.dimension.inspect")
    require(items[0]["drawing_expected"]["view_after_deferred_update"] == "not_current_until_Update",
            "deferred update only applies to views")

    lost = items[1]["drawing_expected"]
    for name, last in (("diam_bore1", 10), ("centres_1_2", 50)):
        require(lost[name]["status"] == "lost", "lost status cannot remap to Bore2")
        require(lost[name]["historical_last_value_mm"] == last, "last value must be historical")
        require(lost[name]["attached_to_any_other_bore"] is False, "silent reassignment forbidden")
    require(len(items[2]["subcases"]) == 2, "undo/redo must cover edit and deletion")
    require(all(c.get("sequence", [{}])[-1].get("command") == "edit.redo"
                for c in items[2]["subcases"]), "each edit must be redone")
    for sub in items[2]["subcases"]:
        require(all(step["status"] in ("ok", "lost") for step in sub["sequence"]),
                "undo/redo must use W19d statuses")
    for state in items[3]["source_checkpoints"]:
        require(all(value["status"] in ("ok", "lost") for value in state["expect"].values()),
                "save/reopen checkpoints must use W19d statuses")
    require(items[3]["source_checkpoints"][2]["expect"]["diam_bore1"]["historical_last_value_mm"] == 10,
            "keep disputed E2E-04 lost-value requirement for CamWight's decision")
    require(items[3]["drawing_expected"]["lost_historical_value_does_not_become_live"] is True,
            "preserve historical value after reopen as pending design decision")
    require(items[3]["drawing_expected"]["after_every_save_reopen"] is True,
            "save/reopen checks are required")
    print("STATIC CONTRACT PASS: four W23a recipes match source oracle metadata.")
    print("DRAWING E2E AND EARLY W19d/W20a DIMENSION INSPECTION: NOT RUN (static only).")


if __name__ == "__main__":
    main()
