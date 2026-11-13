#!/usr/bin/env python3
"""Inspect a native candidate and compile reviewed opaque appearance overlays.

Python 3.11 development tooling only. Never evaluates geometry, solves joints,
downloads a model, modifies a native design, or automatically approves a source.
Run --help or read ../README.md. Output files are created exclusively, not replaced.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import math
import re
import sys
import zipfile
from collections import Counter, defaultdict, deque
from pathlib import Path, PurePosixPath


PACKAGE = Path(__file__).resolve().parents[1]
MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
MAX_DOCUMENT_BYTES = 128 * 1024 * 1024
MAX_RECORDS = 100_000
JOINT_VALUES = {
    "rigid": [], "revolute": ["rad"], "slider": ["mm"],
    "cylindrical": ["rad", "mm"], "pin_slot": ["rad", "mm"],
    "planar": ["mm", "mm", "rad"], "ball": ["rad", "rad", "rad"],
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def unique_object(pairs: list[tuple]) -> dict:
    result = {}
    for key, value in pairs:
        require(key not in result, f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def reject_constant(value: str) -> None:
    raise ValueError(f"Non-finite JSON number: {value}")


def decode(data: bytes) -> dict:
    value = json.loads(data.decode("utf-8"), object_pairs_hook=unique_object,
                       parse_constant=reject_constant)
    require(isinstance(value, dict), "Expected a JSON object.")
    pending = [value]
    while pending:
        node = pending.pop()
        if isinstance(node, dict):
            pending.extend(node.values())
        elif isinstance(node, list):
            pending.extend(node)
        elif isinstance(node, float):
            require(math.isfinite(node), "Non-finite JSON number.")
    return value


def read_json(path: Path) -> dict:
    with path.open("rb") as stream:
        data = stream.read(MAX_DOCUMENT_BYTES + 1)
    require(len(data) <= MAX_DOCUMENT_BYTES, "JSON file exceeds inspection budget.")
    return decode(data)


def write_json(path: Path, value: dict) -> None:
    encoded = json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as stream:
        stream.write(encoded)


def number(value: object) -> bool:
    return type(value) is int or (type(value) is float and math.isfinite(value))


def index_records(value: object, label: str) -> dict[int, dict]:
    require(isinstance(value, list) and len(value) <= MAX_RECORDS,
            f"{label}: expected at most {MAX_RECORDS} records.")
    result = {}
    for item in value:
        require(isinstance(item, dict), f"{label}: expected object records.")
        identity = item.get("id")
        require(type(identity) is int and 0 < identity < 2**64,
                f"{label}: invalid positive integer ID.")
        require(identity not in result, f"{label}: duplicate ID {identity}.")
        result[identity] = item
    return result


def inspect_native(path: Path) -> dict:
    with path.open("rb") as stream:
        data = stream.read(MAX_ARCHIVE_BYTES + 1)
    require(len(data) <= MAX_ARCHIVE_BYTES, "Native file exceeds 64 MiB inspection budget.")
    snapshot = io.BytesIO(data)
    require(zipfile.is_zipfile(snapshot), "Expected a ZIP-based .solidcraft native container.")
    with zipfile.ZipFile(snapshot) as archive:
        entries = archive.infolist()
        require(0 < len(entries) <= 1024, "Invalid or excessive ZIP entry count.")
        names = [item.filename for item in entries]
        require(len(names) == len(set(names)), "Duplicate ZIP entry names.")
        require("document.json" in names, "Native container lacks document.json.")
        require(sum(item.file_size for item in entries) <= 512 * 1024 * 1024,
                "Native container exceeds expanded inspection budget.")
        for item in entries:
            name = PurePosixPath(item.filename)
            require(not name.is_absolute() and ".." not in name.parts and "\\" not in item.filename,
                    "Unsafe ZIP entry path.")
            require(not item.flag_bits & 1, "Encrypted ZIP entries are unsupported.")
        entry = archive.getinfo("document.json")
        require(entry.file_size <= MAX_DOCUMENT_BYTES, "Native document exceeds inspection budget.")
        document = decode(archive.read(entry))
    require(re.fullmatch(r"solidcraft/[1-9][0-9]*", document.get("format", "")) is not None,
            "Unknown native document format tag.")
    require(document.get("units") == "mm", "This demo requires native millimeter dimensions.")
    components = index_records(document.get("components", []), "components")
    occurrences = index_records(document.get("occurrences", []), "occurrences")
    features = index_records(document.get("features", []), "features")
    assembly = document.get("assembly", {})
    require(isinstance(assembly, dict), "Invalid native assembly object.")
    joints = index_records(assembly.get("joints", []), "joints")
    studies = index_records(assembly.get("studies", []), "studies")
    links = assembly.get("links", [])
    require(isinstance(links, list) and len(links) <= MAX_RECORDS, "Invalid motion link list.")
    component_ids, occurrence_ids = set(components) | {0}, set(occurrences) | {0}
    for component in components.values():
        require(component.get("parent", 0) in component_ids, "Unknown parent component.")
    for occurrence in occurrences.values():
        require(occurrence.get("component") in components, "Unknown occurrence component.")
        require(occurrence.get("parent", 0) in component_ids, "Unknown placement parent component.")
        require(type(occurrence.get("grounded", False)) is bool, "Invalid grounded flag.")
    adjacency = defaultdict(set)
    for joint in joints.values():
        require(joint.get("kind") in JOINT_VALUES, "Unrecognized joint type; extend the inspector explicitly.")
        a, b = joint.get("a", {}), joint.get("b", {})
        require(isinstance(a, dict) and isinstance(b, dict), "Invalid joint origins.")
        a, b = a.get("occurrence"), b.get("occurrence")
        require(type(a) is int and type(b) is int and a != b
                and a in occurrence_ids and b in occurrence_ids, "Invalid joint occurrence reference.")
        if not joint.get("suppressed", False):
            adjacency[a].add(b)
            adjacency[b].add(a)
        units = JOINT_VALUES[joint["kind"]]
        values, limits = joint.get("values", []), joint.get("limits", [])
        require(isinstance(values, list) and len(values) <= len(units)
                and all(number(x) for x in values), "Invalid joint values.")
        require(isinstance(limits, list) and len(limits) <= len(units), "Invalid joint limits.")
        for limit in limits:
            require(limit is None or (isinstance(limit, list) and len(limit) == 2
                    and all(number(x) for x in limit) and limit[0] <= limit[1]), "Invalid joint limit interval.")
    for link in links:
        require(isinstance(link, dict) and link.get("a") in joints and link.get("b") in joints,
                "Motion link references an unknown joint.")
    for study in studies.values():
        keys = study.get("keys", [])
        require(isinstance(keys, list) and len(keys) <= MAX_RECORDS, "Invalid motion study keys.")
        for key in keys:
            require(isinstance(key, dict) and key.get("joint") in joints, "Study references an unknown joint.")
    grounded = {0} | {key for key, item in occurrences.items() if item.get("grounded", False)}
    reachable, queue = set(grounded), deque(grounded)
    while queue:
        for following in adjacency[queue.popleft()] - reachable:
            reachable.add(following)
            queue.append(following)
    declared_names = set()
    for feature in features.values():
        if not feature.get("suppressed", False):
            names = feature.get("body_names", [])
            require(isinstance(names, list) and all(isinstance(x, str) for x in names), "Invalid declared body names.")
            declared_names.update(names)
    return {
        "schema_version": "0.1.0", "status": "inspected_container_only",
        "sha256": hashlib.sha256(data).hexdigest(), "size_bytes": len(data),
        "format": document["format"], "units": document["units"], "up_axis": "+Z",
        "name": document.get("name"),
        "counts": {"components_excluding_root": len(components), "occurrences": len(occurrences),
                   "history_features": len(features), "joints": len(joints), "motion_links": len(links),
                   "motion_studies": len(studies), "joints_by_kind": dict(Counter(j["kind"] for j in joints.values()))},
        "components": list(components.values()), "occurrences": list(occurrences.values()),
        "joints": list(joints.values()), "motion_links": links, "motion_studies": list(studies.values()),
        "declared_body_names": sorted(declared_names),
        "joint_graph": {"grounded_occurrence_ids": sorted(grounded),
                        "not_connected_to_ground_by_unsuppressed_joints": sorted(set(occurrences) - reachable),
                        "scope": "Undirected joint-edge reachability only; ignores nested placement inheritance and is not a DOF, closure, collision or solvability result."},
        "limitations": ["Native joint values, limits and study keys remain radians/mm; command angles use degrees.",
                        "Declared body names are feature metadata, not proof that a body survives evaluated history.",
                        "No geometry evaluation, world bounds, native migration, motion playback or rendering was performed.",
                        "Shape/count matches do not establish that this is the author's improved SELENE variant."],
    }


def binding_template(model: Path) -> dict:
    native = inspect_native(model)
    roles = [role for role in read_json(PACKAGE / "scenes/roles.json")["items"]
             if role.get("kind") == "appearance" and role.get("owner") == "subject"
             and role.get("opaque_proxy_allowed") is True]
    return {
        "schema_version": "0.1.0", "language": "en", "status": "unreviewed_template",
        "source_sha256": native["sha256"], "review_scope": "opaque_body_subset_only",
        "review": {"reviewer": None, "date": None, "evaluated_body_targets_checked": False,
                   "shared_definition_scope_checked": False,
                   "physical_materials_preserved": False, "opaque_approximations_accepted": False},
        "assignments": [{"role": role["id"], "status": "unresolved", "recipe_id": None,
                         "override_reason": None,
                         "bodies": [], "note": "Select exact evaluated body names on the admitted improved source; no automatic v7 mapping."}
                        for role in roles],
    }


def compile_overlay(model: Path, lock_path: Path, bindings_path: Path) -> dict:
    native, lock, bindings = inspect_native(model), read_json(lock_path), read_json(bindings_path)
    source = lock.get("active_source", {})
    require(source.get("status") == "pinned_native", "Improved native source is not admitted: source-lock status must be pinned_native.")
    for field in ("sha256", "size_bytes", "format", "units", "up_axis"):
        require(source.get(field) == native[field], f"Source-lock mismatch: {field}.")
    for old in lock.get("legacy_sources", []):
        require(old.get("sha256") != native["sha256"], "Historical Rover v7 cannot substitute for the improved showcase source.")
    require(source.get("variant") == "SELENE-showcase", "Unexpected active variant.")
    provenance_url = source.get("download_url")
    if not provenance_url and source.get("repository") and source.get("path"):
        require(re.fullmatch(r"[0-9a-f]{40}", source.get("commit") or "") is not None, "Repository source requires a pinned commit.")
        provenance_url = "https://github.com/" + source["repository"]
    require(isinstance(provenance_url, str) and provenance_url.startswith("https://"), "Pinned source requires an HTTPS provenance location.")
    admission = lock.get("admission_review", {})
    require(admission.get("variant_identity_confirmed") is True
            and admission.get("license_and_attribution_reviewed") is True
            and isinstance(admission.get("reviewer"), str) and bool(admission["reviewer"].strip()),
            "Source identity and attribution need a recorded review.")
    require(isinstance(admission.get("evidence_url"), str) and admission["evidence_url"].startswith("https://")
            and isinstance(admission.get("review_date"), str)
            and re.fullmatch(r"\d{4}-\d{2}-\d{2}", admission["review_date"]) is not None,
            "Source admission needs a dated provenance review with an HTTPS evidence URL.")
    license_review = lock.get("license_review", {})
    require(license_review.get("status") == "reviewed"
            and isinstance(license_review.get("license_expression"), str)
            and bool(license_review["license_expression"].strip())
            and isinstance(license_review.get("notices"), list),
            "Complete the file-specific license expression and notice inventory before source admission.")
    require(int(native["format"].split("/")[1]) <= 21,
            "Source format exceeds the inspected render integration contract (21); revalidate the adapter against the actual executor.")
    require(bindings.get("source_sha256") == native["sha256"], "Bindings belong to a different native revision.")
    review = bindings.get("review", {})
    require(bindings.get("status") == "reviewed" and isinstance(review.get("reviewer"), str)
            and bool(review["reviewer"].strip()) and isinstance(review.get("date"), str)
            and re.fullmatch(r"\d{4}-\d{2}-\d{2}", review["date"]) is not None,
            "Bindings need a named, dated review.")
    for key in ("evaluated_body_targets_checked", "shared_definition_scope_checked",
                "physical_materials_preserved", "opaque_approximations_accepted"):
        require(review.get(key) is True, f"Binding review missing: {key}.")
    recipes = {item["id"]: item for item in read_json(PACKAGE / "materials/appearance-pack.json")["items"]}
    roles = {item["id"]: item for item in read_json(PACKAGE / "scenes/roles.json")["items"]}
    assignments = bindings.get("assignments", [])
    require(isinstance(assignments, list) and 0 < len(assignments) <= len(roles), "Invalid assignment collection.")
    commands, used_bodies, used_roles = [], set(), set()
    declared = set(native["declared_body_names"])
    for assignment in assignments:
        require(isinstance(assignment, dict), "Invalid assignment object.")
        role = assignment.get("role")
        require(role in roles and role not in used_roles, "Unknown or repeated appearance role.")
        used_roles.add(role)
        if assignment.get("status") == "omitted":
            require(isinstance(assignment.get("note"), str) and bool(assignment["note"].strip())
                    and not assignment.get("bodies"), "Omitted role requires a reason and no targets.")
            continue
        require(assignment.get("status") == "reviewed", f"Unresolved role: {role}.")
        require(roles[role].get("kind") == "appearance" and roles[role].get("owner") == "subject"
                and roles[role].get("opaque_proxy_allowed") is True,
                "The opaque native-body adapter cannot bind framing, terrain, layers, light sources or optical/emissive roles.")
        recipe = recipes.get(assignment.get("recipe_id"))
        require(recipe is not None, f"Unknown appearance recipe for {role}.")
        if roles[role].get("candidate_material_id") not in recipe.get("source_catalog_ids", []):
            require(isinstance(assignment.get("override_reason"), str)
                    and bool(assignment["override_reason"].strip()),
                    "A different material concept requires a recorded appearance-override reason.")
        bodies = assignment.get("bodies")
        require(isinstance(bodies, list) and 0 < len(bodies) <= 10_000
                and all(isinstance(x, str) and x in declared for x in bodies),
                "Targets must be exact body names declared in this native source and reviewed in the evaluated app.")
        require(len(set(bodies)) == len(bodies) and not used_bodies.intersection(bodies),
                "Overlapping body assignments would replace a previous appearance.")
        used_bodies.update(bodies)
        parameters = dict(recipe["baseline_projection"]["runtime_parameters"])
        require(set(parameters) == {"appearance", "color", "opacity", "pbr"}
                and number(parameters["opacity"]) and parameters["opacity"] == 1,
                "Only the inspected opaque PBR parameter contract may be emitted.")
        require(isinstance(parameters["appearance"], str) and bool(parameters["appearance"].strip())
                and len(parameters["appearance"].encode("utf-8")) <= 128,
                "Appearance label must be nonempty and fit the prepared pack's 128-byte bound.")
        require(isinstance(parameters["color"], str)
                and re.fullmatch(r"#[0-9a-fA-F]{6}", parameters["color"]) is not None,
                "Prepared appearance colors must be #rrggbb sRGB8 strings.")
        pbr = parameters["pbr"]
        require(isinstance(pbr, dict) and set(pbr) == {"metallic", "roughness", "dielectric_ior"}
                and all(number(x) for x in pbr.values()) and 0 <= pbr["metallic"] <= 1
                and 0.045 <= pbr["roughness"] <= 1 and 1 <= pbr["dielectric_ior"] <= 3,
                "Appearance PBR fields exceed the inspected runtime contract.")
        parameters["bodies"] = bodies
        commands.append({"command": "appearance.assign", "params": parameters})
    require(bool(commands), "No reviewed opaque body assignments to compile.")
    # A command script is intentionally only the engine's documented envelope.
    # The native model and reviewed bindings retain provenance alongside it.
    return {"commands": commands}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    for name, help_text in (("inspect", "Read native identity and assembly metadata without evaluating geometry."),
                            ("template", "Write an unreviewed role-binding template for a candidate native file."),
                            ("compile", "Compile only explicitly reviewed opaque body assignments after source admission.")):
        command = sub.add_parser(name, help=help_text)
        command.add_argument("--model", type=Path, required=True)
        command.add_argument("--out", type=Path, required=True)
        if name == "compile":
            command.add_argument("--lock", type=Path, default=PACKAGE / "source-lock.json")
            command.add_argument("--bindings", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.operation == "inspect":
            result = inspect_native(args.model)
        elif args.operation == "template":
            result = binding_template(args.model)
        else:
            result = compile_overlay(args.model, args.lock, args.bindings)
        write_json(args.out, result)
        print(f"Wrote {args.out}. Native source unchanged; no engine commands executed.")
        return 0
    except (OSError, ValueError, TypeError, KeyError, AttributeError, OverflowError,
            RecursionError, zipfile.BadZipFile, NotImplementedError) as exc:
        print(f"Preparation blocked: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
