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

    # CamWight, 3 Nov 2026: the Drawing workspace refuses model edits.
    # This validates the recipe only; it does not execute UI acceptance.
    required_ui_sequence = [
        "close_drawing_workspace",
        "switch_to_model_workspace",
        "apply_source_edit_or_undo_redo",
        "reopen_drawing_workspace",
        "verify_views_dimensions_references_and_positions",
    ]
    policy = data["ui_workflow_policy"]
    require(policy["model_commands_during_open_drawing"] == "rejected"
            and policy["drawing_and_model_same_design_file"] is True
            and policy["required_sequence"] == required_ui_sequence
            and policy["runtime_status"] == "NOT_RUN"
            and policy["live_coediting"] == "future_only_not_current_acceptance",
            "UI test contract must close Drawing before model edits")
    for item in items:
        ui = item["ui_workflow"]
        require(ui["required_sequence"] == required_ui_sequence
                and ui["model_command_while_drawing_open"] is False
                and ui["runtime_status"] == "NOT_RUN",
                "unsupported model edit in an open drawing")
    require(items[2]["ui_workflow"]["check_after_each_undo_redo"] is True,
            "E2E-03 must reopen and verify Drawing after undo/redo")

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
    # The native extensions on top of the four recipes: each names its edits as real design
    # commands, uses W19d's status words, and its Rust test is in the dimension test file.
    tests = (ROOT / "crates/engine/src/cmd/drawing_dimension_tests.rs").read_text(encoding="utf-8")
    extensions = data["early_native_extensions"]
    require([e["id"] for e in extensions] == ["W23A-R1-WIDTH", "W23A-SUPPRESSED-BORE", "W23A-FIVE-DIMENSIONS", "W23A-R2-CENTRES",
                                              "W23A-SHEET", "W23A-FILLETED-CORNERS", "W23A-REPAIR"], "native extension identities changed")
    for e in extensions:
        require(e["runtime_status"] == "NATIVE_ENGINE_TEST", e["id"] + " is an engine test, not a drawing or UI run")
        require(all(a["session"] == "design" for a in e["source_actions"]), e["id"] + ": model edits are design commands")
        for name in e["implemented_rust_tests"]:
            require("fn " + name + "()" in tests, e["id"] + ": no test " + name)
    require(extensions[0]["source_actions"][0]["params"] == {"name": "width", "expression": "100 mm"}, "width edit must change `width`")
    require(read(extensions[0]["oracle_cross_check"] + "/expected.json")["source"] == "analytic", "width cross-check oracle")
    require([a["command"] for a in extensions[1]["source_actions"]] == ["timeline.suppress", "timeline.suppress", "timeline.delete"],
            "suppress, unsuppress, then delete Bore1")
    require(all(a["params"]["feature"] == "Bore1" for a in extensions[1]["source_actions"]), "the suppressed bore is Bore1")
    # The four plate cases: each of the plate's parameters with the value its dimension must read,
    # Bore1 for every feature edit, and only edits the plate's script can make.
    parameters = {"bore1_d": "12 mm", "bore2_d": "8 mm", "width": "100 mm", "length": "120 mm"}
    for e in extensions[2:]:
        for a in e["source_actions"]:
            if a["command"] == "parameters.change":
                require(parameters.get(a["params"]["name"]) == a["params"]["expression"], e["id"] + ": unexpected parameter edit")
            elif a["command"] in ("timeline.suppress", "timeline.delete"):
                require(a["params"]["feature"] == "Bore1", e["id"] + ": the edited feature is Bore1")
            elif a["command"] == "solid.fillet":
                require(a["params"]["body"] == "Plate", e["id"] + ": the fillet is on the plate")
            else:
                require(a["command"] in ("edit.undo", "edit.redo"), e["id"] + ": unexpected command " + a["command"])
    five = [a["params"]["name"] for a in extensions[2]["source_actions"] if a["command"] == "parameters.change"]
    require(five == list(parameters), "every parameter of the plate is changed once")
    require(any("50" in c and "40" in c and "30" in c for c in extensions[3]["checks"]), "centre to centre: 50, 40 and 30")
    # The repair: the user's own pick after a round and after a delete, never a move by itself.
    repair = extensions[6]
    require([a["command"] for a in repair["source_actions"]] == ["solid.fillet", "edit.undo", "edit.redo", "edit.undo", "timeline.delete", "edit.undo"],
            "round Bore1's rim, undo and redo it, undo it, then delete Bore1 and undo")
    require(repair["source_actions"][0]["params"]["edges"] == [[25, 20, 20]], "the round is on Bore1's rim")
    require(any("drawing.dimension.repair" in c and "one drawing undo step" in c for c in repair["checks"]), "a repair is one undo step")
    require(any("never reads Bore2 by itself" in c for c in repair["checks"]), "a lost dimension never moves to Bore2 by itself")
    print("STATIC CONTRACT PASS: four W23a recipes match source oracle metadata.")
    print("DRAWING E2E AND EARLY W19d/W20a DIMENSION INSPECTION: NOT RUN (static only).")


if __name__ == "__main__":
    main()
