#!/usr/bin/env python3
"""Read-only checks for prepared SELENE resources; no runtime readiness claim.

Usage: python3 assets/render/selene/tools/validate_prepared.py [--json]
Python 3.11 standard-library development tooling, not an application dependency.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import sys
from pathlib import Path

from prepare import PACKAGE, number, read_json


CATALOG = PACKAGE.parents[2] / "docs/render-materials"
COLLECTIONS = {
    "materials/appearance-pack.json": (48, "SELENE-AP-"),
    "scenes/roles.json": (33, None),
    "scenes/stages.json": (10, "SELENE-STAGE-"),
    "scenes/shots.json": (12, "SELENE-SHOT-"),
    "motion/mechanism-acceptance.json": (12, "MCA-"),
}


class Validation:
    def __init__(self, directory: Path, catalog: Path):
        self.directory, self.catalog = directory, catalog
        self.errors: list[str] = []
        self.counts: dict[str, int] = {}
        self.documents: dict[str, dict] = {}
        self.indices: dict[str, dict] = {}

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.errors.append(message)

    def pbr(self, params: dict, label: str) -> None:
        self.check(set(params) == {"appearance", "color", "opacity", "pbr"}, label + ": unexpected appearance parameters")
        self.check(isinstance(params.get("appearance"), str) and bool(params["appearance"].strip())
                   and len(params["appearance"].encode("utf-8")) <= 128, label + ": invalid label")
        color = params.get("color")
        self.check(isinstance(color, str) and re.fullmatch(r"#[0-9a-fA-F]{6}", color) is not None, label + ": expected sRGB8 hex color")
        self.check(number(params.get("opacity")) and params["opacity"] == 1, label + ": PBR must be numerically opaque")
        pbr = params.get("pbr", {})
        self.check(isinstance(pbr, dict) and set(pbr) == {"metallic", "roughness", "dielectric_ior"}, label + ": wrong PBR fields")
        for field, lower, upper in (("metallic", 0, 1), ("roughness", .045, 1), ("dielectric_ior", 1, 3)):
            value = pbr.get(field)
            self.check(number(value) and lower <= value <= upper, label + ": invalid " + field)

    def ui_checks(self, catalog: dict) -> None:
        flow = self.documents["ui/demo-flow.json"]
        ui_path = self.directory / "ui"
        contract_path = (ui_path / flow["metadata"]["integration_contract"]).resolve()
        self.check(contract_path == (self.catalog / "ui-integration.json").resolve(), "UI flow must reference the shared integration contract")
        contract = read_json(self.catalog / "ui-integration.json")
        self.check(flow["metadata"]["status"] == "specified_unbound" and contract["metadata"]["runtime_ready"] is False,
                   "UI specification cannot imply implemented runtime behavior")
        controls = flow["controls"]
        self.check(flow["defaults"]["default_view"] == controls["disclosure"]["default"] == contract["disclosure"]["default"] == "Quick",
                   "UI disclosure defaults disagree")
        self.check(controls["disclosure"]["global_mode_switch"] is False and controls["disclosure"]["extra_ribbon_tabs"] is False,
                   "Disclosure must not create global modes or ribbon tabs")
        self.check(controls["maximum_primary_adjustments"] == contract["disclosure"]["quick_adjustment_budget"] == 6,
                   "Primary adjustment budgets disagree")
        for context, values in controls["quick_by_context"].items():
            self.check(isinstance(values, list) and 0 < len(values) <= 6 and len(set(values)) == len(values),
                       "Invalid primary control list for " + context)
        self.check(controls["quick"] == controls["quick_by_context"]["lunar_scene"], "Default lunar controls disagree")
        self.check(flow["library"]["local_categories"] == contract["library"]["category_labels"], "UI library categories disagree")
        quality = flow["quality_selection"]
        self.check((ui_path / quality["source"]).resolve() == (self.catalog / "quality-presets.json").resolve(),
                   "UI quality picker must reference the shared quality catalog")
        profiles = quality["profiles"]
        self.check(len(profiles) == len(catalog["quality-presets.json"])
                   and {p["quality_preset_id"] for p in profiles} == set(catalog["quality-presets.json"]), "UI quality profile inventory disagrees")
        for profile in profiles:
            source = catalog["quality-presets.json"].get(profile["quality_preset_id"], {})
            self.check(profile["name"] == source.get("name") and profile["status"] == source.get("status") == "specified_unbound",
                       "UI quality labels or availability disagree with the source catalog")
        shots = self.indices["scenes/shots.json"]
        recommendations = quality["recommendations"]
        self.counts["ui/demo-flow.json shot quality recommendations"] = len(recommendations)
        self.check(len(recommendations) == len(shots) and {r["shot_id"] for r in recommendations} == set(shots),
                   "Every prepared shot needs one quality recommendation group")
        for item in recommendations:
            ids = item["quality_preset_ids"]
            self.check(len(ids) == 3 and len(set(ids)) == 3 and set(ids) <= set(catalog["quality-presets.json"]),
                       item["shot_id"] + ": recommend three known quality profiles")
            self.check(item["default_quality_preset_id"] in ids
                       and item["default_quality_preset_id"] == shots.get(item["shot_id"], {}).get("quality_preset_id"),
                       item["shot_id"] + ": UI default must preserve the prepared shot quality")
        registered = {r["id"] for r in contract["command_reuse"]}
        self.check({r["command"] for r in flow["command_bindings"]["registered_authorities"]} <= registered,
                   "UI flow invents an existing command authority")
        proposals = {r["candidate_id"] for r in contract["proposed_commands"]}
        self.check(all(r["registered"] is False and r["command"] in proposals for r in flow["command_bindings"]["future_proposals"]),
                   "UI future proposals disagree with the shared contract")
        swatches = flow["swatch_opening"]
        self.check((ui_path / swatches["script"]).resolve() == (self.directory / "materials/appearance-swatches.json").resolve()
                   and swatches["script_first_command"] == "file.new" and swatches["preserve_existing_documents"] is True
                   and swatches["runtime_executed"] is False and swatches["visual_reviewed"] is False,
                   "Swatch opening must preserve document isolation and actual evidence status")
        self.check(all(s["runtime_ui_available"] is False and s["render_available_after_preflight"] is False
                       for s in flow["availability_states"]), "Illustrative UI states cannot claim current runtime enablement")

    def run(self) -> dict:
        catalog_ids = set()
        catalog = {}
        for filename in ("materials.json", "capabilities.json", "environment-presets.json", "studio-presets.json",
                         "quality-presets.json", "scene-presets.json", "lunar-physics-fixtures.json"):
            document = read_json(self.catalog / filename)
            catalog[filename] = {item["id"]: item for item in document["items"]}
            catalog_ids.update(catalog[filename])
        for path in sorted(self.directory.rglob("*.json")):
            relative = path.relative_to(self.directory).as_posix()
            if relative == "motion/interference-pairs.json":
                # This native-validation input is an explicit array, unlike catalog documents.
                with path.open("rb") as stream:
                    data = stream.read(65537)
                if len(data) > 65536:
                    raise ValueError(relative + ": selection exceeds 64 KiB")
                pairs = json.loads(data)
                valid = (isinstance(pairs, list) and 0 < len(pairs) <= 20
                         and all(isinstance(pair, list) and len(pair) == 2
                                 and all(isinstance(name, str) and 0 < len(name) <= 256 for name in pair)
                                 and pair[0] != pair[1] for pair in pairs))
                self.check(valid, relative + ": expected 1–20 pairs of distinct exact body names")
                if valid:
                    self.counts[relative + " pairs"] = len(pairs)
                    self.check(len({tuple(sorted(pair)) for pair in pairs}) == len(pairs),
                               relative + ": duplicate pair")
                continue
            document = read_json(path)
            self.documents[relative] = document
            if "metadata" in document:
                self.check(document["metadata"].get("language") == "en", relative + ": metadata must declare English")
            pending = [document]
            while pending:
                node = pending.pop()
                if isinstance(node, dict):
                    pending.extend(node.values())
                elif isinstance(node, list):
                    pending.extend(node)
                elif isinstance(node, str) and re.fullmatch(r"(?:MAT|CAP|Q|STU|ENVSET|SCENE|PHY)-\d{3}", node):
                    self.check(node in catalog_ids, relative + ": unknown catalog reference " + node)
        for filename, (count, prefix) in COLLECTIONS.items():
            items = self.documents[filename]["items"]
            identities = [item["id"] for item in items]
            self.counts[filename] = len(items)
            self.check(len(items) == count and len(set(identities)) == count, filename + ": wrong count or duplicate IDs")
            if prefix:
                self.check(identities == [f"{prefix}{i:03}" for i in range(1, count + 1)], filename + ": unexpected ID series")
            self.indices[filename] = {item["id"]: item for item in items}
        lock = self.documents["source-lock.json"]
        source = lock["active_source"]
        self.check(source["variant"] == "SELENE-showcase", "Active source must select the improved showcase")
        self.check(source["status"] in {"awaiting_native_source", "pinned_native"}, "Unknown source-admission status")
        if source["status"] == "awaiting_native_source":
            self.check(all(source.get(key) is None for key in ("repository", "commit", "path", "download_url", "sha256", "size_bytes", "format")),
                       "Unavailable native source cannot have invented identity fields")
            self.check(lock["admission_review"]["variant_identity_confirmed"] is False
                       and lock["admission_review"]["license_and_attribution_reviewed"] is False, "Unavailable native source cannot be approved")
        self.check(source["units"] == "mm" and source["up_axis"] == "+Z", "Source units must remain mm and Z-up")
        self.check(all(old["status"] == "historical_reference_only" and old["eligible_as_active_source"] is False
                       and old["sha256"] != source["sha256"] for old in lock["legacy_sources"]), "Legacy source must be excluded")
        for scene in catalog["scene-presets.json"].values():
            if scene["name"].startswith("SELENE "):
                subject = scene["subject_binding"]
                self.check(subject["manifest"] == "../../assets/render/selene/source-lock.json"
                           and subject.get("required_source_status") == "pinned_native", scene["id"] + ": stale SELENE source binding")
        recipes = self.indices["materials/appearance-pack.json"]
        for identity, item in recipes.items():
            self.pbr(item["baseline_projection"]["runtime_parameters"], identity)
            self.check(item["component_binding"] == "unbound", identity + ": recipes must not invent native targets")
            self.check(item["advanced_target"]["status"] == "authored_specification_not_runtime_bound", identity + ": advanced target availability mismatch")
        swatches = self.documents["materials/appearance-swatches.json"]["commands"]
        self.counts["materials/appearance-swatches.json commands"] = len(swatches)
        self.check(len(swatches) == 99 and swatches[0]["command"] == "file.new", "Swatches need a new design and 99 expected commands")
        bodies, seen_recipes = set(), set()
        for command in swatches:
            name, params = command["command"], command["params"]
            self.check(name in {"file.new", "solid.box", "appearance.assign"}, "Swatch script includes unexpected behavior")
            if name == "solid.box":
                body = params["body_name"]
                self.check(body not in bodies, "Duplicate swatch body name")
                bodies.add(body)
                self.check(all(number(params[k]) and params[k] > 0 for k in ("length", "width", "height")), "Invalid swatch box dimensions")
                self.check(len(params["corner"]) == 3 and all(number(x) for x in params["corner"]), "Invalid swatch corner")
            elif name == "appearance.assign":
                targets = params["bodies"]
                self.check(len(targets) == 1 and targets[0] in bodies, "Swatch assignment must target a created body")
                runtime = {k: v for k, v in params.items() if k != "bodies"}
                self.pbr(runtime, targets[0])
                identity = targets[0].split(" ", 1)[0]
                if identity in recipes:
                    self.check(identity not in seen_recipes, "Recipe assigned more than once")
                    seen_recipes.add(identity)
                    self.check(runtime == recipes[identity]["baseline_projection"]["runtime_parameters"], identity + ": swatch diverges from recipe")
        self.check(len(bodies) == 49 and seen_recipes == set(recipes), "Missing swatch bodies or appearance assignments")
        roles = self.indices["scenes/roles.json"]
        stages = self.indices["scenes/stages.json"]
        for filename in ("scenes/roles.json", "scenes/stages.json", "scenes/shots.json"):
            for record in self.documents[filename]["metadata"]["catalog_inputs"]:
                path = (self.catalog / record["path"]).resolve()
                self.check(path.is_relative_to(self.catalog.resolve()), "Catalog hash path leaves the catalog")
                self.check(hashlib.sha256(path.read_bytes()).hexdigest() == record["sha256"], filename + ": stale catalog hash for " + record["path"])
        for identity, role in roles.items():
            if role["kind"] in {"appearance_layer", "light_source", "optical_appearance", "emissive_appearance"}:
                self.check(role.get("opaque_proxy_allowed") is False, identity + ": advanced role must not enter the opaque native adapter")
        for identity, stage in stages.items():
            self.check(stage["geometry"]["subject_scale"] == 1, identity + ": do not rescale the native rover")
            self.check(stage["geometry"]["fit_subject_role_id"] in roles, identity + ": unknown fit role")
            for output in stage["geometry"]["role_outputs"]:
                for field in ("geometry_role_id", "appearance_role_id"):
                    if field in output:
                        self.check(output[field] in roles, identity + ": unknown generated role")
            if stage["mode"] == "lunar_physical":
                self.check(stage["ambient"]["enabled"] is False and stage["ambient"]["radiance_scale"] == 0, identity + ": physical lunar ambient fill")
                self.check(stage["atmosphere"]["enabled"] is False and stage["atmosphere"]["density_scale"] == 0, identity + ": physical lunar atmosphere")
                self.check(stage["editorial_fill"]["enabled"] is False, identity + ": hidden editorial fill")
                self.check(stage["background"]["earth"]["enabled"] is False and stage["background"]["stars"]["enabled"] is False, identity + ": unresolved celestial backdrop")
            sun = stage["solar_source"]
            if sun["enabled"]:
                self.check(sun["owner"] == "environment" and sun["environment_map_contains_sun"] is False, identity + ": duplicate or ambiguous Sun owner")
                self.check(sun["distance_model"] == "directional_at_infinity_for_local_stage"
                           and 0 < sun["angular_diameter_rad"] < .1, identity + ": invalid local finite-disk Sun")
        for identity, shot in self.indices["scenes/shots.json"].items():
            stage = stages[shot["stage_id"]]
            self.check(shot["mode"] == stage["mode"], identity + ": shot/stage mode mismatch")
            self.check(shot["camera"]["target_role_id"] in roles, identity + ": unknown framing role")
            needed = set(stage["required_capabilities"])
            for field, filename in (("quality_preset_id", "quality-presets.json"), ("studio_preset_id", "studio-presets.json")):
                needed.update(catalog[filename][shot[field]]["required_capabilities"])
            for role in shot["appearance_role_ids"] + shot["subject"]["required_geometry_role_ids"]:
                self.check(role in roles, identity + ": unknown role " + role)
                needed.update(roles[role]["required_capabilities"])
            self.check(needed <= set(shot["required_capabilities"]), identity + ": missing inherited capabilities " + str(sorted(needed - set(shot["required_capabilities"]))))
            camera = shot["camera"]
            self.check(0 < camera["vertical_fov_rad"] < math.pi and 0 < camera["frame_fill_fraction"] < 1, identity + ": invalid camera framing")
            self.check(shot["subject"]["preserve_native_dimensions"] is True, identity + ": preserve model scale")
        references = {entry["id"] for entry in lock["references"]}
        for item in self.indices["motion/mechanism-acceptance.json"].values():
            self.check(set(item["documentation_evidence"]["reference_ids"]) <= references, item["id"] + ": unknown documentation reference")
            if source["status"] == "awaiting_native_source":
                self.check(item["native_evidence"]["bindings"] is None and item["runtime_evidence"]["report"] is None, item["id"] + ": fabricated native/runtime evidence")
        preflight = self.documents["motion/preflight-commands.json"]["commands"]
        self.check(preflight == [{"command": name, "params": {}} for name in
                                 ("joint.list", "motion.list", "appearance.list", "material.list", "render.scene")], "Preflight must remain a read-only command inventory")
        sequence = self.documents["motion/sample-sequence.json"]
        states = {state["id"] for state in sequence["states"]}
        self.counts["motion/sample-sequence.json states"] = len(states)
        for transition in sequence["transitions"]:
            self.check(transition["from"] in states and transition["to"] in states, "Unknown sequence state")
        self.check(sequence["local_physics"]["gravity_m_s2"] == [0, 0, -1.62]
                   and sequence["local_physics"]["atmospheric_drag"] is False, "Lunar local physics assumption changed without review")
        self.check({"release", "free_flight", "capture", "miss", "store"} <= states, "Sequence lacks physical success/failure events")
        self.ui_checks(catalog)
        return {
            "valid": not self.errors, "counts": self.counts, "errors": self.errors,
            "native_source_status": source["status"], "runtime_ready": False,
            "outstanding": (["Obtain, inspect and admit the improved native file."]
                            if source["status"] == "awaiting_native_source" else []) +
                           ["Review exact native targets and execute the overlay on a saved derivative.",
                            "Bind optical, terrain, lighting and final-output resources to the actual renderer.",
                            "Run required repository gates, runtime previews and mechanism/dynamics validation."],
            "scope": "Prepared data, source-state honesty, references, hashes, opaque command projections and composition checks; no native solver or renderer execution.",
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=PACKAGE)
    parser.add_argument("--catalog", type=Path, default=CATALOG)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        report = Validation(args.directory.resolve(), args.catalog.resolve()).run()
    except (OSError, ValueError, TypeError, KeyError, AttributeError, OverflowError, RecursionError) as exc:
        report = {"valid": False, "runtime_ready": False, "errors": [str(exc)]}
    if args.json:
        print(json.dumps(report, indent=2, ensure_ascii=False, allow_nan=False))
    else:
        print("PASS" if report["valid"] else "FAIL", "— prepared SELENE resource checks")
        for filename, count in report.get("counts", {}).items():
            print(f"  {filename}: {count}")
        for error in report["errors"]:
            print("  ERROR:", error)
        print("Runtime readiness is not established by this validator.")
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    sys.exit(main())
