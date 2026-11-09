#!/usr/bin/env python3
"""Validate the SolidCraft v0.2 concept package using Python 3.11 stdlib only.

Usage: python3 validate_catalog.py [CATALOG_DIRECTORY] [--json]

This is a read-only data lint and analytic planning check. Passing does not bind
presets, run a renderer, certify physical accuracy, or prove runtime support.
Current-command limits are taken from SolidCraft docs/render-settings.md and
the associated Rust settings types; future authoring presets use separate rules.
Both flat v0.2 envelopes and normalized {metadata: {...}, items: [...]} envelopes
are accepted so the same validator can run before and after package assembly.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
import unicodedata
from pathlib import Path
from typing import Any


SCHEMA_VERSION = "0.2.0"
COLLECTIONS = {
    "materials.json": (600, "MAT-", 3, 1),
    "lighting.json": (280, "LUZ-", 3, 1),
    "environments.json": (280, "ENV-", 3, 1),
    "capabilities.json": (120, "CAP-", 3, 1),
    "ux.json": (56, "UX-", 3, 1),
    "simulation-requirements.json": (36, "SIM-", 3, 1),
    "roadmap.json": (19, "F", 2, 0),
    "quality-presets.json": (10, "Q-", 3, 1),
    "studio-presets.json": (24, "STU-", 3, 1),
    "environment-presets.json": (24, "ENVSET-", 3, 1),
    "scene-presets.json": (24, "SCENE-", 3, 1),
}
SUPPORT_FILES = (
    "baseline-setup-commands.json", "selene-bindings.json",
    "lunar-physics-fixtures.json", "provenance.json", "ui-integration.json",
)
REF_PATTERN = re.compile(
    r"(?:MAT|LUZ|ENV|CAP|UX|SIM|Q|STU|ENVSET|SCENE)-\d{3}|[FS]\d{2}"
)
PHYSICAL_MODES = ("lunar_physical", "space_physical")
SCENE_MODES = ("terrestrial", "lunar_physical", "space_physical", "editorial")


def is_number(value: Any) -> bool:
    return type(value) is int or (type(value) is float and math.isfinite(value))


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"Duplicate JSON object key: {key!r}")
        result[key] = value
    return result


def reject_constant(value: str) -> None:
    raise ValueError(f"Non-finite JSON constant: {value}")


def metadata(document: dict[str, Any]) -> dict[str, Any]:
    value = document.get("metadata", document)
    return value if isinstance(value, dict) else {}


def declared_ids(value: Any) -> set[str]:
    """Return safe IDs for cross-checks; field validation reports bad shapes."""
    return {x for x in value if isinstance(x, str)} if isinstance(value, list) else set()


def object_value(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


class Validator:
    def __init__(self, directory: Path):
        self.directory = directory
        self.documents: dict[str, dict[str, Any]] = {}
        self.indices: dict[str, dict[str, dict[str, Any]]] = {}
        self.errors: list[dict[str, str]] = []
        self.warnings: list[dict[str, str]] = []
        self.counts: dict[str, int] = {}

    def error(self, path: str, message: str) -> None:
        self.errors.append({"path": path, "message": message})

    def warning(self, path: str, message: str) -> None:
        self.warnings.append({"path": path, "message": message})

    def require(self, condition: bool, path: str, message: str) -> bool:
        if not condition:
            self.error(path, message)
        return condition

    def text(self, value: Any, path: str) -> bool:
        return self.require(
            isinstance(value, str) and bool(value.strip()) and value == value.strip(),
            path, "Expected a nonempty, trimmed string.",
        )

    def string_list(self, value: Any, path: str, *, nonempty: bool = True,
                    unique: bool = False) -> bool:
        if not self.require(isinstance(value, list), path, "Expected an array."):
            return False
        if nonempty:
            self.require(bool(value), path, "Expected a nonempty array.")
        valid = all([self.text(x, f"{path}[{i}]") for i, x in enumerate(value)])
        if unique and valid:
            self.require(len(value) == len(set(value)), path, "Repeated values are not allowed.")
        return valid

    def number(self, value: Any, path: str, *, minimum: float | None = None,
               maximum: float | None = None, positive: bool = False,
               integer: bool = False) -> bool:
        if not self.require(is_number(value) and (not integer or type(value) is int),
                            path, "Expected a finite number" + (" of integer type." if integer else ".")):
            return False
        if minimum is not None:
            self.require(value >= minimum, path, f"Must be at least {minimum}.")
        if maximum is not None:
            self.require(value <= maximum, path, f"Must be at most {maximum}.")
        if positive:
            self.require(value > 0, path, "Must be greater than zero.")
        return True

    def boolean(self, value: Any, path: str) -> bool:
        return self.require(type(value) is bool, path, "Expected a boolean.")

    def obj(self, value: Any, path: str, *, nonempty: bool = True) -> dict[str, Any]:
        if not self.require(isinstance(value, dict), path, "Expected an object."):
            return {}
        if nonempty:
            self.require(bool(value), path, "Expected a nonempty object.")
        return value

    def vector(self, value: Any, path: str, *, minimum: float | None = None,
               maximum: float | None = None, unit: bool = False) -> bool:
        if not self.require(isinstance(value, list) and len(value) == 3,
                            path, "Expected a three-component vector."):
            return False
        ok = all([self.number(x, f"{path}[{i}]", minimum=minimum, maximum=maximum)
                  for i, x in enumerate(value)])
        if ok and unit:
            self.require(all(abs(x) <= 1.000001 for x in value)
                         and math.isclose(sum(x*x for x in value), 1, abs_tol=1e-6),
                         path, "Expected a unit direction vector.")
        return ok

    def finite_tree(self, value: Any, path: str) -> None:
        if isinstance(value, dict):
            for key, child in value.items():
                self.finite_tree(child, f"{path}.{key}")
        elif isinstance(value, list):
            for i, child in enumerate(value):
                self.finite_tree(child, f"{path}[{i}]")
        elif isinstance(value, float):
            self.require(math.isfinite(value), path, "Non-finite numeric value.")

    def load(self) -> None:
        for filename in (*COLLECTIONS, *SUPPORT_FILES):
            path = self.directory / filename
            try:
                value = json.loads(path.read_text(encoding="utf-8"),
                                   object_pairs_hook=unique_object, parse_constant=reject_constant)
            except (OSError, UnicodeError, ValueError) as exc:
                self.error(filename, f"Cannot load strict UTF-8 JSON: {exc}")
                continue
            if self.require(isinstance(value, dict), filename, "Top level must be an object."):
                self.documents[filename] = value
                self.finite_tree(value, filename)

    def collection_checks(self) -> None:
        for filename, (count, prefix, width, start) in COLLECTIONS.items():
            doc = self.documents.get(filename)
            if doc is None:
                continue
            meta = metadata(doc)
            self.require(meta.get("schema_version") == SCHEMA_VERSION,
                         filename + ".metadata.schema_version", f"Expected {SCHEMA_VERSION!r}.")
            self.require(meta.get("language") == "en", filename + ".metadata.language", "Expected 'en'.")
            self.text(meta.get("status"), filename + ".metadata.status")
            items = doc.get("items")
            if not self.require(isinstance(items, list), filename + ".items", "Expected an items array."):
                continue
            self.counts[filename] = len(items)
            if count is None:
                self.require(16 <= len(items) <= 20, filename, "Expected 16 to 20 scene presets.")
                count = len(items)
            else:
                self.require(len(items) == count, filename, f"Expected exactly {count} items; found {len(items)}.")
            expected = {f"{prefix}{i:0{width}d}" for i in range(start, start+count)}
            if filename == "roadmap.json":
                expected = {f"F{i:02d}" for i in range(13)} | {f"S{i:02d}" for i in range(1, 7)}
            index: dict[str, dict[str, Any]] = {}
            names: set[str] = set()
            for position, item in enumerate(items):
                path = f"{filename}.items[{position}]"
                if not isinstance(item, dict):
                    self.error(path, "Each item must be an object.")
                    continue
                identity = item.get("id")
                if self.text(identity, path + ".id"):
                    self.require(identity not in index, path + ".id", "Duplicate item ID.")
                    index[identity] = item
                    path = f"{filename}:{identity}"
                name = item.get("name")
                if self.text(name, path + ".name"):
                    key = name.casefold()
                    self.require(key not in names, path + ".name", "Duplicate item name (case-insensitive).")
                    names.add(key)
                self.text(item.get("status"), path + ".status")
                if "priority" in item:
                    self.require(item["priority"] in ("P0", "P1", "P2"), path + ".priority", "Expected P0, P1 or P2.")
                if "cost" in item:
                    self.require(item["cost"] in ("Low", "Medium", "High"), path + ".cost", "Expected Low, Medium or High.")
            missing, extra = expected-index.keys(), index.keys()-expected
            if missing:
                self.error(filename, "Missing expected IDs: " + ", ".join(sorted(missing)))
            if extra:
                self.error(filename, "Unexpected IDs: " + ", ".join(sorted(extra)))
            self.indices[filename] = index

    def items(self, filename: str):
        for identity, item in self.indices.get(filename, {}).items():
            yield f"{filename}:{identity}", item

    def reference(self, value: Any, filename: str, path: str) -> dict[str, Any] | None:
        if not self.text(value, path):
            return None
        target = self.indices.get(filename, {}).get(value)
        self.require(target is not None, path, f"Unknown reference {value!r} in {filename}.")
        return target

    def capability_list(self, value: Any, path: str) -> None:
        if self.string_list(value, path, unique=True):
            for i, identity in enumerate(value):
                self.reference(identity, "capabilities.json", f"{path}[{i}]")

    def reference_tree(self, value: Any, path: str, all_ids: set[str],
                       provenance_ids: set[str], calibration_ids: set[str], source_ids: set[str]) -> None:
        if isinstance(value, dict):
            for key, child in value.items():
                childpath = f"{path}.{key}"
                if key in {"required_capabilities", "capability_requirements"}:
                    self.capability_list(child, childpath)
                elif key == "provenance_id":
                    self.require(isinstance(child, str) and child in provenance_ids, childpath,
                                 f"Unknown provenance reference {child!r}.")
                elif key == "reference_id" and isinstance(child, str) and child.startswith("CAL-"):
                    self.require(child in calibration_ids, childpath, f"Undefined calibration reference {child!r}.")
                elif key in {"source_id", "gravity_source_id"}:
                    self.require(isinstance(child, str) and child in source_ids, childpath, f"Unknown source reference {child!r}.")
                self.reference_tree(child, childpath, all_ids, provenance_ids, calibration_ids, source_ids)
        elif isinstance(value, list):
            for i, child in enumerate(value):
                self.reference_tree(child, f"{path}[{i}]", all_ids, provenance_ids, calibration_ids, source_ids)
        elif isinstance(value, str) and REF_PATTERN.fullmatch(value):
            self.require(value in all_ids, path, f"Unknown catalog reference {value!r}.")
        elif isinstance(value, str) and re.fullmatch(r"SRC-[A-Z0-9-]+", value):
            self.require(value in source_ids, path, f"Unknown source reference {value!r}.")

    def reference_checks(self) -> None:
        all_ids = {identity for index in self.indices.values() for identity in index}
        provenance_ids: set[str] = set()
        calibration_ids: set[str] = set()
        source_ids: set[str] = set()
        provenance = self.documents.get("provenance.json", {})
        for field, destination in (("items", provenance_ids), ("sources", source_ids)):
            entries = provenance.get(field, [])
            if isinstance(entries, list):
                destination.update(x["id"] for x in entries if isinstance(x, dict) and isinstance(x.get("id"), str))
        for filename in ("lighting.json", "studio-presets.json"):
            norms = metadata(self.documents.get(filename, {})).get("normalization", {})
            if isinstance(norms, dict):
                calibration_ids.update(k for k in norms if k.startswith("CAL-"))
        for filename, doc in self.documents.items():
            self.reference_tree(doc, filename, all_ids, provenance_ids, calibration_ids, source_ids)

    def required_fields(self) -> None:
        common = ("family", "description", "authoring_method", "provenance_id")
        catalog_text = {
            "materials.json": common + ("optical_model", "scale_policy"),
            "lighting.json": common + ("sources", "geometry", "contribution_policy"),
            "environments.json": common + ("representation", "geometry", "ambient_lighting", "scale_policy"),
        }
        for filename, fields in catalog_text.items():
            for path, item in self.items(filename):
                for field in fields:
                    self.text(item.get(field), path + "." + field)
                for field in ("domains", "use_cases", "controls", "capability_requirements", "validation"):
                    self.string_list(item.get(field), path + "." + field)
                self.require(item.get("status") == "concept", path + ".status", "Catalog entries must remain 'concept'.")
                self.require(item.get("priority") in ("P0", "P1", "P2"), path + ".priority", "Required priority is missing or invalid.")
                self.require(item.get("cost") in ("Low", "Medium", "High"), path + ".cost", "Required cost is missing or invalid.")
        for filename, fields in {
            "capabilities.json": ("family", "benefit"),
            "ux.json": ("area", "behavior"),
            "simulation-requirements.json": ("family",),
            "roadmap.json": ("scope",),
        }.items():
            for path, item in self.items(filename):
                for field in fields:
                    self.text(item.get(field), path + "." + field)
                self.string_list(item.get("acceptance"), path + ".acceptance")
        for path, item in self.items("capabilities.json"):
            self.string_list(item.get("controls"), path + ".controls")
        for filename in ("quality-presets.json", "studio-presets.json", "environment-presets.json", "scene-presets.json"):
            for path, item in self.items(filename):
                self.require(item.get("status") == "specified_unbound", path + ".status",
                             "Numeric recipes must remain 'specified_unbound'.")
                self.string_list(item.get("validation"), path + ".validation")
                self.capability_list(item.get("required_capabilities"), path + ".required_capabilities")

    def quality_checks(self) -> None:
        for path, item in self.items("quality-presets.json"):
            resolution = self.obj(item.get("resolution"), path + ".resolution")
            for key in ("width", "height"):
                self.number(resolution.get(key), path + ".resolution." + key, positive=True, integer=True)
            self.require(item.get("integrator_intent") == "path_tracing", path + ".integrator_intent", "Expected declared path-tracing intent.")
            sampling = self.obj(item.get("sampling"), path + ".sampling")
            low, high = sampling.get("minimum_samples"), sampling.get("maximum_samples")
            valid_low = self.number(low, path + ".sampling.minimum_samples", positive=True, integer=True)
            valid_high = self.number(high, path + ".sampling.maximum_samples", positive=True, integer=True)
            if valid_low and valid_high:
                self.require(low <= high, path + ".sampling", "Minimum samples cannot exceed maximum samples.")
            self.number(sampling.get("relative_error_target"), path + ".sampling.relative_error_target", positive=True, maximum=1)
            for key in ("error_estimator", "maximum_samples_action"):
                self.text(sampling.get(key), path + ".sampling." + key)
            for key in ("scope", "stopping_rule_binding"):
                self.text(sampling.get(key), path + ".sampling." + key)
            transport = self.obj(item.get("transport"), path + ".transport")
            for key in ("maximum_depth", "maximum_transmission_depth", "russian_roulette_after_depth"):
                self.number(transport.get(key), path + ".transport." + key, minimum=0, integer=True)
            depth = transport.get("maximum_depth")
            if is_number(depth):
                for key in ("maximum_transmission_depth", "russian_roulette_after_depth"):
                    if is_number(transport.get(key)):
                        self.require(transport[key] <= depth, path + ".transport." + key, "Cannot exceed maximum path depth.")
            self.text(transport.get("roulette_policy"), path + ".transport.roulette_policy")
            denoise = self.obj(item.get("denoising"), path + ".denoising")
            for key in ("enabled", "preserve_raw", "temporal_validation_required"):
                self.boolean(denoise.get(key), path + ".denoising." + key)
            self.require(denoise.get("preserve_raw") is True, path + ".denoising.preserve_raw", "Raw output must be preserved.")
            motion = self.obj(item.get("motion"), path + ".motion")
            self.boolean(motion.get("enabled"), path + ".motion.enabled")
            self.boolean(motion.get("requires_approved_bake"), path + ".motion.requires_approved_bake")
            self.number(motion.get("shutter_fraction"), path + ".motion.shutter_fraction", minimum=0, maximum=1)
            self.number(motion.get("temporal_samples"), path + ".motion.temporal_samples", minimum=0, integer=True)
            if motion.get("enabled") is True:
                self.require(is_number(motion.get("shutter_fraction")) and motion["shutter_fraction"] > 0,
                             path + ".motion.shutter_fraction", "Enabled motion needs a positive shutter fraction.")
                self.require(is_number(motion.get("temporal_samples")) and motion["temporal_samples"] > 0,
                             path + ".motion.temporal_samples", "Enabled motion needs temporal samples.")
                self.require(motion.get("requires_approved_bake") is True, path + ".motion.requires_approved_bake", "Motion must require an approved bake.")
                for key in ("sample_budget_policy", "shutter_alignment", "frame_rate_binding"):
                    self.text(motion.get(key), path + ".motion." + key)
                if denoise.get("enabled") is True:
                    self.require(denoise.get("temporal_validation_required") is True, path + ".denoising",
                                 "Motion denoising needs temporal validation.")
            else:
                self.require(motion.get("shutter_fraction") == 0 and motion.get("temporal_samples") == 0,
                             path + ".motion", "Disabled motion must have zero shutter fraction and temporal samples.")
            output = self.obj(item.get("output"), path + ".output")
            self.require(output.get("master") in ("linear_buffer", "linear_exr"), path + ".output.master", "Expected scene-linear master output.")
            self.require(output.get("presentation") == "png", path + ".output.presentation", "Expected PNG presentation output.")
            self.require(output.get("color_transform") == "project_declared", path + ".output.color_transform", "Expected a project-declared color transform.")
            self.require(output.get("exposure_policy") == "manual_locked", path + ".output.exposure_policy", "Expected manual locked exposure.")
            needed = {"CAP-001", "CAP-003", "CAP-006", "CAP-057", "CAP-058", "CAP-059", "CAP-073"}
            if output.get("master") == "linear_exr":
                needed.add("CAP-074")
            if denoise.get("enabled") is True:
                needed.add("CAP-065")
            if motion.get("enabled") is True:
                needed.update({"CAP-053", "CAP-078", "CAP-119"})
            self.missing_capabilities(item, needed, path)
            self.number(item.get("seed"), path + ".seed", minimum=0, integer=True)
            for key in ("goal", "budget_policy", "baseline_compatibility", "provenance_id"):
                self.text(item.get(key), path + "." + key)
            self.text(item.get("seed_policy"), path + ".seed_policy")

    def missing_capabilities(self, item: dict[str, Any], needed: set[str], path: str) -> None:
        provided = item.get("required_capabilities", [])
        if isinstance(provided, list) and all(isinstance(x, str) for x in provided):
            missing = needed - set(provided)
            if missing:
                self.error(path + ".required_capabilities", "Missing declared-feature dependencies: " + ", ".join(sorted(missing)))

    def baseline_checks(self) -> None:
        filename = "baseline-setup-commands.json"
        doc = self.documents.get(filename)
        if doc is None:
            return
        commands = doc.get("commands")
        if not self.require(isinstance(commands, list), filename + ".commands", "Expected commands array."):
            return
        self.counts[filename] = len(commands)
        self.require(len(commands) == 6, filename, "Expected exactly six baseline setup commands.")
        names = set()
        allowed = {"backend", "mode", "camera", "width", "height", "format", "exposure_ev",
                   "environment_rotation", "environment_intensity", "background"}
        for i, raw in enumerate(commands):
            path = f"{filename}.commands[{i}]"
            command = self.obj(raw, path)
            self.require(set(command) <= {"command", "params"}, path, "Unknown command-envelope field.")
            self.require(command.get("command") == "render.settings.set", path + ".command", "Only render.settings.set belongs in this baseline file.")
            params = self.obj(command.get("params"), path + ".params")
            self.require(set(params) <= {"name", "set"}, path + ".params", "Unknown parameter field.")
            name = params.get("name")
            if self.text(name, path + ".params.name"):
                self.require(len(name.encode("utf-8")) <= 128, path + ".params.name", "Name exceeds 128 UTF-8 bytes.")
                self.require(not any(unicodedata.category(c) == "Cc" for c in name), path + ".params.name", "Name contains a control character.")
                self.require(name not in names, path + ".params.name", "Duplicate setup name would overwrite another command.")
                names.add(name)
            values = self.obj(params.get("set"), path + ".params.set")
            self.require(set(values) <= allowed, path + ".params.set", "Unknown render.settings.set field: " + ", ".join(sorted(set(values)-allowed)))
            width, height = values.get("width", 1024), values.get("height", 768)
            a = self.number(width, path + ".params.set.width", positive=True, maximum=4096, integer=True)
            b = self.number(height, path + ".params.set.height", positive=True, maximum=4096, integer=True)
            if a and b:
                self.require(width * height <= 4_194_304, path + ".params.set", "Pixel product exceeds 4,194,304.")
            backend = values.get("backend", "builtin")
            self.require(isinstance(backend, str) and len(backend.encode("utf-8")) <= 64
                         and re.fullmatch(r"[a-z][a-z0-9._-]*", backend) is not None,
                         path + ".params.set.backend", "Invalid backend identifier.")
            self.require(values.get("mode", "preview") in ("preview", "final", "quick"), path + ".params.set.mode", "Invalid settings mode.")
            self.require(values.get("format", "png") in ("png", "exr"), path + ".params.set.format", "Invalid output format.")
            self.number(values.get("exposure_ev", 0), path + ".params.set.exposure_ev", minimum=-32, maximum=32)
            self.number(values.get("environment_rotation", 0), path + ".params.set.environment_rotation", minimum=-1_000_000, maximum=1_000_000)
            self.number(values.get("environment_intensity", 1), path + ".params.set.environment_intensity", minimum=0, maximum=100)
            color = values.get("background", [236, 240, 245])
            if self.require(isinstance(color, list) and len(color) == 3, path + ".params.set.background", "Expected three sRGB bytes."):
                for j, channel in enumerate(color):
                    self.number(channel, f"{path}.params.set.background[{j}]", minimum=0, maximum=255, integer=True)
            if "camera" in values:
                self.camera_checks(values["camera"], path + ".params.set.camera")

    def camera_checks(self, raw: Any, path: str) -> None:
        camera = self.obj(raw, path)
        self.require(set(camera) <= {"target", "yaw", "pitch", "distance", "fov", "roll", "up"}, path, "Unknown camera field.")
        self.vector(camera.get("target"), path + ".target", minimum=-1e9, maximum=1e9)
        self.number(camera.get("distance"), path + ".distance", positive=True, maximum=1e9)
        for key in ("yaw", "pitch", "roll"):
            self.number(camera.get(key, 0 if key == "roll" else None), path + "." + key, minimum=-1e6, maximum=1e6)
        if self.number(camera.get("fov"), path + ".fov", minimum=0):
            self.require(camera["fov"] < math.pi, path + ".fov", "FOV must be below pi radians; zero selects orthographic.")
        self.require(camera.get("up", "z") in ("z", "y"), path + ".up", "Camera up must be z or y.")

    def studio_checks(self) -> None:
        for path, item in self.items("studio-presets.json"):
            self.reference(item.get("catalog_id"), "lighting.json", path + ".catalog_id")
            self.text(item.get("description"), path + ".description")
            self.require(item.get("mode") in ("studio", "lunar_physical", "lunar_editorial"), path + ".mode", "Unknown studio mode.")
            policy = self.obj(item.get("subject_scale_policy"), path + ".subject_scale_policy")
            for key in ("mode", "definition", "origin", "axes", "position_and_size_units", "angle_units", "fit_policy", "degenerate_subject_policy", "normalization_policy"):
                self.text(policy.get(key), path + ".subject_scale_policy." + key)
            self.require(policy.get("angle_units") == "radians", path + ".subject_scale_policy.angle_units", "Angles must use radians.")
            for key in ("exposed_controls", "locks"):
                self.string_list(item.get(key), path + "." + key)
            local_ids: set[str] = set()
            for collection in ("lights", "modifiers"):
                entries = item.get(collection)
                if not self.require(isinstance(entries, list), path + "." + collection, "Expected an array; an explicitly empty array is allowed."):
                    continue
                for i, raw in enumerate(entries):
                    childpath = f"{path}.{collection}[{i}]"
                    light = self.obj(raw, childpath)
                    identity = light.get("id")
                    if self.text(identity, childpath + ".id"):
                        self.require(identity not in local_ids, childpath + ".id", "Duplicate local source/modifier ID.")
                        local_ids.add(identity)
                    if collection == "modifiers":
                        self.passive_modifier(light, childpath)
                        continue
                    source_type = light.get("type")
                    self.require(source_type in ("rectangular_area", "directional_sun"), childpath + ".type", "Unknown authored light type.")
                    color = self.obj(light.get("color"), childpath + ".color")
                    self.require(color.get("space") == "linear_srgb", childpath + ".color.space", "Expected a declared linear RGB color space.")
                    self.vector(color.get("value"), childpath + ".color.value", minimum=0)
                    intensity = self.obj(light.get("intensity"), childpath + ".intensity")
                    self.number(intensity.get("value"), childpath + ".intensity.value")
                    if source_type == "rectangular_area":
                        pos_ok = self.vector(light.get("position_b"), childpath + ".position_b")
                        target_ok = self.vector(light.get("target_b"), childpath + ".target_b")
                        if pos_ok and target_ok:
                            self.require(light["position_b"] != light["target_b"], childpath, "Emitter position and aim target must differ.")
                        self.size_checks(light.get("size_b"), childpath + ".size_b")
                        self.number(light.get("roll_rad"), childpath + ".roll_rad")
                        self.require(intensity.get("quantity") == "reference_contribution_ev", childpath + ".intensity.quantity", "Area recipes need calibrated reference EV semantics.")
                        self.text(intensity.get("reference_id"), childpath + ".intensity.reference_id")
                        self.require(light.get("energy_owner") == "analytic_light", childpath + ".energy_owner", "Area-light energy must have one analytic owner.")
                    elif source_type == "directional_sun":
                        self.vector(light.get("direction"), childpath + ".direction", unit=True)
                        self.number(light.get("angular_diameter_rad"), childpath + ".angular_diameter_rad", positive=True, maximum=math.pi)
                        self.require(intensity.get("quantity") == "relative_solar_irradiance", childpath + ".intensity.quantity", "Directional sun requires relative irradiance semantics.")
                        self.number(intensity.get("value"), childpath + ".intensity.value", minimum=0)
                        self.require(light.get("energy_owner") == "rig_sun", childpath + ".energy_owner", "Studio sun must declare rig ownership.")
                    self.text(light.get("visible_emitter_policy"), childpath + ".visible_emitter_policy")
                    if item.get("mode") == "lunar_physical":
                        self.require(source_type == "directional_sun", childpath, "Physical lunar rigs cannot contain editorial fill lights.")

    def size_checks(self, raw: Any, path: str) -> None:
        size = self.obj(raw, path)
        for key in ("width", "height"):
            self.number(size.get(key), path + "." + key, positive=True)

    def passive_modifier(self, modifier: dict[str, Any], path: str) -> None:
        self.text(modifier.get("type"), path + ".type")
        self.text(modifier.get("role"), path + ".role")
        self.vector(modifier.get("position_b"), path + ".position_b")
        self.vector(modifier.get("normal"), path + ".normal", unit=True)
        self.size_checks(modifier.get("size_b"), path + ".size_b")
        self.number(modifier.get("roll_rad"), path + ".roll_rad")
        self.require(modifier.get("emission") == 0, path + ".emission", "Passive cards/flags must not emit light.")
        material = self.obj(modifier.get("material"), path + ".material")
        self.text(material.get("model"), path + ".material.model")
        self.vector(material.get("reflectance_linear_srgb"), path + ".material.reflectance_linear_srgb", minimum=0, maximum=1)

    def environment_checks(self) -> None:
        for path, item in self.items("environment-presets.json"):
            self.reference(item.get("catalog_id"), "environments.json", path + ".catalog_id")
            self.text(item.get("description"), path + ".description")
            self.require(item.get("mode") in SCENE_MODES, path + ".mode", "Unknown environment mode.")
            self.string_list(item.get("exposed_controls"), path + ".exposed_controls")
            geometry = self.obj(item.get("geometry"), path + ".geometry")
            self.text(geometry.get("generator"), path + ".geometry.generator")
            self.require(geometry.get("units") == "mm", path + ".geometry.units", "Geometry must use explicit millimeters.")
            parameters = self.obj(geometry.get("parameters_mm"), path + ".geometry.parameters_mm")
            for key, value in parameters.items():
                dimension = re.search(r"(?:^|_)(?:width|depth|height|length|radius|thickness|separation|distance|spacing|wavelength|extent(?:_[xyz])?)$", key) is not None
                self.number(value, path + ".geometry.parameters_mm." + key,
                            positive=dimension, minimum=0 if key.endswith("amplitude") else None)
            if is_number(parameters.get("rock_min_radius")) and is_number(parameters.get("rock_max_radius")):
                self.require(parameters["rock_min_radius"] <= parameters["rock_max_radius"], path + ".geometry.parameters_mm", "Minimum rock radius cannot exceed maximum rock radius.")
            self.number(geometry.get("procedural_seed"), path + ".geometry.procedural_seed", minimum=0, integer=True)
            frame = self.obj(geometry.get("reference_frame"), path + ".geometry.reference_frame")
            self.require(frame.get("up_axis") == "+Z", path + ".geometry.reference_frame.up_axis", "Environment frame must be Z-up.")
            self.require(geometry.get("subject_scale_policy") == "preserve_physical_dimensions", path + ".geometry.subject_scale_policy", "Subject physical scale must be preserved.")
            fit = self.obj(geometry.get("stage_fit"), path + ".geometry.stage_fit")
            self.require(fit.get("mode") == "once_on_explicit_request", path + ".geometry.stage_fit.mode", "Stage fitting must be explicit and performed once.")
            self.require(fit.get("scale_subject") is False and fit.get("resize_room_dimensions") is False,
                         path + ".geometry.stage_fit", "Stage fitting cannot rescale the subject or resize authored room dimensions.")
            self.number(fit.get("minimum_clearance_mm"), path + ".geometry.stage_fit.minimum_clearance_mm", minimum=0)
            self.number(fit.get("maximum_supported_subject_extent_mm"), path + ".geometry.stage_fit.maximum_supported_subject_extent_mm", positive=True)
            self.text(geometry.get("topography_status"), path + ".geometry.topography_status")
            ground = self.obj(item.get("ground"), path + ".ground")
            self.boolean(ground.get("enabled"), path + ".ground.enabled")
            self.text(ground.get("appearance_role"), path + ".ground.appearance_role")
            gp = self.obj(ground.get("parameters"), path + ".ground.parameters")
            self.number(gp.get("elevation_mm"), path + ".ground.parameters.elevation_mm")
            self.number(gp.get("texture_scale_mm"), path + ".ground.parameters.texture_scale_mm", positive=True)
            self.number(gp.get("roughness_multiplier"), path + ".ground.parameters.roughness_multiplier", minimum=0)
            self.require(ground.get("shadow_mode") in ("physical_geometry", "shadow_catcher"), path + ".ground.shadow_mode", "Unknown ground shadow mode.")
            ambient = self.obj(item.get("ambient"), path + ".ambient")
            self.text(ambient.get("source_type"), path + ".ambient.source_type")
            self.number(ambient.get("scale"), path + ".ambient.scale", minimum=0)
            self.vector(ambient.get("color_linear_srgb"), path + ".ambient.color_linear_srgb", minimum=0)
            self.require(ambient.get("sun_disk_in_environment") is False, path + ".ambient.sun_disk_in_environment", "Ambient map must not duplicate the separately owned sun.")
            sun = self.obj(item.get("sun"), path + ".sun")
            self.boolean(sun.get("enabled"), path + ".sun.enabled")
            self.require(sun.get("owner") == "environment", path + ".sun.owner", "Environment recipes must declare solar ownership.")
            self.number(sun.get("azimuth_rad"), path + ".sun.azimuth_rad")
            self.number(sun.get("elevation_rad"), path + ".sun.elevation_rad", minimum=-math.pi/2, maximum=math.pi/2)
            self.number(sun.get("angular_diameter_rad"), path + ".sun.angular_diameter_rad", positive=True, maximum=math.pi)
            self.number(sun.get("relative_irradiance"), path + ".sun.relative_irradiance", minimum=0)
            if sun.get("enabled") is False:
                self.require(sun.get("relative_irradiance") == 0, path + ".sun.relative_irradiance", "Disabled sun must have zero relative irradiance.")
            self.require(sun.get("occlusion") == "trace_scene_geometry", path + ".sun.occlusion", "Sun occlusion must be resolved geometrically.")
            atmosphere = self.obj(item.get("atmosphere"), path + ".atmosphere")
            for key in ("enabled", "participating_volume_enabled"):
                self.boolean(atmosphere.get(key), path + ".atmosphere." + key)
            self.number(atmosphere.get("density_scale"), path + ".atmosphere.density_scale", minimum=0)
            background = self.obj(item.get("background"), path + ".background")
            self.vector(background.get("color_linear_srgb"), path + ".background.color_linear_srgb", minimum=0)
            self.number(background.get("alpha"), path + ".background.alpha", minimum=0, maximum=1)
            earth = self.obj(background.get("earth"), path + ".background.earth")
            stars = self.obj(background.get("stars"), path + ".background.stars")
            self.boolean(earth.get("enabled"), path + ".background.earth.enabled")
            self.boolean(stars.get("enabled"), path + ".background.stars.enabled")
            self.number(stars.get("radiance_scale"), path + ".background.stars.radiance_scale", minimum=0)
            if item.get("mode") in PHYSICAL_MODES:
                self.require(ambient.get("source_type") == "none" and ambient.get("scale") == 0,
                             path + ".ambient", "Physical vacuum environments require zero atmospheric ambient radiance.")
                self.require(atmosphere.get("enabled") is False and atmosphere.get("model") == "none"
                             and atmosphere.get("density_scale") == 0 and atmosphere.get("participating_volume_enabled") is False,
                             path + ".atmosphere", "Physical vacuum environments cannot have atmospheric scattering or volumes.")
                self.require(earth.get("enabled") is False, path + ".background.earth.enabled", "Earth must stay off until location/trajectory, ephemeris and exposure are bound.")
                self.require(stars.get("enabled") is False and stars.get("radiance_scale") == 0,
                             path + ".background.stars", "Unbound physical sunlit presets must not add a star overlay.")
                self.require(ground.get("shadow_mode") == "physical_geometry", path + ".ground.shadow_mode", "Physical vacuum stages require physical ground when enabled.")
                self.vacuum_extras(item, path)

    def vacuum_extras(self, value: Any, path: str) -> None:
        """Reject active terrestrial media if added to a future vacuum recipe."""
        if not isinstance(value, dict):
            return
        for key, child in value.items():
            if key in {"wind", "rain", "fog", "atmospheric_smoke"}:
                active = child.get("enabled", False) if isinstance(child, dict) else child
                self.require(active in (False, 0, None), path + "." + key, "Terrestrial weather/media are incompatible with physical vacuum.")
            if key == "dust" and isinstance(child, dict) and child.get("enabled"):
                self.require(child.get("transport") == "ballistic_particles", path + ".dust.transport", "Vacuum dust requires ballistic particle transport.")
            if isinstance(child, dict):
                self.vacuum_extras(child, path + "." + key)

    def scene_checks(self) -> None:
        for path, scene in self.items("scene-presets.json"):
            self.text(scene.get("description"), path + ".description")
            self.require(scene.get("mode") in SCENE_MODES, path + ".mode", "Unknown scene mode.")
            quality = self.reference(scene.get("quality_preset_id"), "quality-presets.json", path + ".quality_preset_id")
            studio = self.reference(scene.get("studio_preset_id"), "studio-presets.json", path + ".studio_preset_id")
            environment = self.reference(scene.get("environment_preset_id"), "environment-presets.json", path + ".environment_preset_id")
            for field in ("controls", "locks"):
                self.string_list(scene.get(field), path + "." + field, nonempty=False)
            subject = self.obj(scene.get("subject_binding"), path + ".subject_binding")
            for field in ("kind", "manifest", "selection_rule", "unresolved_action"):
                self.text(subject.get(field), path + ".subject_binding." + field)
            self.require(subject.get("preserve_physical_scale") is True, path + ".subject_binding.preserve_physical_scale", "Scene composition must preserve physical subject dimensions.")
            self.require(subject.get("unresolved_action") == "block_without_scene_mutation", path + ".subject_binding.unresolved_action", "Unresolved composition must fail atomically.")
            if isinstance(scene.get("name"), str) and scene["name"].startswith("SELENE "):
                self.require(subject.get("kind") == "improved_selene_showcase_or_reviewed_derivative"
                             and subject.get("manifest") == "../../assets/render/selene/source-lock.json"
                             and subject.get("required_source_status") == "pinned_native",
                             path + ".subject_binding", "SELENE shots require the improved showcase source; v7 references are historical only.")
            bindings = self.obj(scene.get("appearance_bindings"), path + ".appearance_bindings")
            dependencies = set()
            for preset in (quality, studio, environment):
                if preset is not None:
                    dependencies.update(declared_ids(preset.get("required_capabilities")))
            for preset, catalog in ((studio, "lighting.json"), (environment, "environments.json")):
                if preset is not None and isinstance(preset.get("catalog_id"), str):
                    concept = self.indices.get(catalog, {}).get(preset["catalog_id"], {})
                    dependencies.update(declared_ids(concept.get("capability_requirements")))
            for role, identity in bindings.items():
                self.text(role, path + ".appearance_bindings.<role>")
                material = self.reference(identity, "materials.json", path + ".appearance_bindings." + role)
                if material is not None:
                    dependencies.update(declared_ids(material.get("capability_requirements")))
            self.missing_capabilities(scene, dependencies, path)
            appearance_overrides = scene.get("appearance_overrides", {})
            if isinstance(appearance_overrides, dict):
                for role, raw in appearance_overrides.items():
                    childpath = path + ".appearance_overrides." + role
                    self.require(role in bindings, childpath, "Appearance override refers to an unbound role.")
                    appearance = self.obj(raw, childpath)
                    if "base_color_linear_srgb" in appearance:
                        self.vector(appearance["base_color_linear_srgb"], childpath + ".base_color_linear_srgb", minimum=0, maximum=1)
            else:
                self.error(path + ".appearance_overrides", "Expected an object.")
            camera = self.obj(scene.get("camera_intent"), path + ".camera_intent")
            self.require(camera.get("projection") == "perspective", path + ".camera_intent.projection", "Expected explicit perspective intent for this package.")
            if self.number(camera.get("vertical_fov_rad"), path + ".camera_intent.vertical_fov_rad", positive=True):
                self.require(camera["vertical_fov_rad"] < math.pi, path + ".camera_intent.vertical_fov_rad", "Perspective FOV must be below pi.")
            self.vector(camera.get("view_from_target_direction"), path + ".camera_intent.view_from_target_direction", unit=True)
            self.number(camera.get("frame_fill_fraction"), path + ".camera_intent.frame_fill_fraction", positive=True, maximum=1)
            self.require(camera.get("fit_policy") == "explicit_once_when_camera_not_locked", path + ".camera_intent.fit_policy", "Camera framing must be explicit and honor locks.")
            for field in ("clip_planes", "exposure"):
                self.text(camera.get(field), path + ".camera_intent." + field)
            if "source_binding_policy" in scene:
                self.text(scene["source_binding_policy"], path + ".source_binding_policy")
            if quality is not None and object_value(quality.get("motion")).get("enabled") is True:
                motion = self.obj(scene.get("motion_binding"), path + ".motion_binding")
                self.require(motion.get("approved_bake_required") is True, path + ".motion_binding.approved_bake_required", "Animation composition needs an approved bake.")
                self.number(motion.get("frame_rate_hz"), path + ".motion_binding.frame_rate_hz", positive=True)
                for field in ("frame_range", "physics_retiming"):
                    self.text(motion.get(field), path + ".motion_binding." + field)
                simulation = motion.get("required_simulation_requirements")
                if self.string_list(simulation, path + ".motion_binding.required_simulation_requirements", unique=True):
                    for identity in simulation:
                        self.reference(identity, "simulation-requirements.json", path + ".motion_binding.required_simulation_requirements")
                if "preview_quality_alternative" in motion:
                    self.reference(motion["preview_quality_alternative"], "quality-presets.json", path + ".motion_binding.preview_quality_alternative")
            if studio is None or environment is None:
                continue
            ground = object_value(environment.get("ground"))
            geometry = object_value(environment.get("geometry"))
            if ground.get("enabled") is True:
                role = ground.get("appearance_role")
                self.require(isinstance(role, str) and role in bindings, path + ".appearance_bindings", f"Enabled environment ground role {role!r} must resolve to a material.")
            if is_number(geometry.get("rock_count")) and geometry["rock_count"] > 0 and geometry.get("rock_appearance_role"):
                role = geometry["rock_appearance_role"]
                self.require(isinstance(role, str) and role in bindings, path + ".appearance_bindings", f"Environment rock role {role!r} must resolve to a material.")
            overrides = scene.get("environment_overrides", {})
            if not isinstance(overrides, dict):
                self.error(path + ".environment_overrides", "Expected an object.")
                overrides = {}
            self.require(set(overrides) <= {"ambient_scale", "sun_enabled", "sun_relative_irradiance", "background_kind"}, path + ".environment_overrides", "Unknown environment override; add explicit validation before use.")
            if "environment_override_reason" in scene:
                self.text(scene["environment_override_reason"], path + ".environment_override_reason")
            if "background_kind" in overrides:
                self.require(overrides["background_kind"] in ("scene_geometry", "black", "transparent", "linked_environment"),
                             path + ".environment_overrides.background_kind", "Unknown background representation.")
            ambient_scale = overrides.get("ambient_scale", object_value(environment.get("ambient")).get("scale"))
            self.number(ambient_scale, path + ".effective_ambient_scale", minimum=0)
            env_sun = object_value(environment.get("sun"))
            sun_enabled = overrides.get("sun_enabled", env_sun.get("enabled"))
            self.boolean(sun_enabled, path + ".effective_environment_sun_enabled")
            irradiance = overrides.get("sun_relative_irradiance", env_sun.get("relative_irradiance"))
            self.number(irradiance, path + ".effective_sun_relative_irradiance", minimum=0)
            lights = studio.get("lights", [])
            if not isinstance(lights, list):
                continue
            active_lights = [light for light in lights if isinstance(light, dict) and light.get("enabled", True) is not False]
            rig_suns = [light for light in active_lights if light.get("type") == "directional_sun"
                        and is_number(object_value(light.get("intensity")).get("value")) and light["intensity"]["value"] > 0]
            solar_count = int(sun_enabled is True and is_number(irradiance) and irradiance > 0) + len(rig_suns)
            self.require(solar_count <= 1, path, "Composition has more than one active solar source.")
            mode = scene.get("mode")
            if mode in PHYSICAL_MODES:
                self.require(environment.get("mode") == mode, path + ".mode", "Physical scene and environment modes must agree.")
                self.require(studio.get("mode") == "lunar_physical", path + ".studio_preset_id", "Physical vacuum scenes require a physical solar/pass-through rig.")
                self.require(ambient_scale == 0, path + ".environment_overrides", "Physical vacuum scenes cannot introduce ambient fill.")
                self.require(all(light.get("type") == "directional_sun" for light in active_lights),
                             path + ".studio_preset_id", "Physical vacuum scenes cannot contain studio/editorial fill lights.")
                self.require(not studio.get("modifiers"), path + ".studio_preset_id", "Unbound physical vacuum scenes cannot introduce editorial studio cards.")
                if studio.get("id") in ("STU-020", "STU-021", "STU-023"):
                    self.require(solar_count == 1, path, "This physical rig requires exactly one resolved solar emitter.")
                if studio.get("id") == "STU-022":
                    self.require(solar_count == 0, path, "Zero-source audit requires all solar emitters disabled.")
                self.vacuum_extras(scene, path)
            elif mode == "terrestrial":
                self.require(environment.get("mode") == "terrestrial", path + ".environment_preset_id", "Terrestrial scene requires a terrestrial environment.")
                self.require(studio.get("mode") == "studio", path + ".studio_preset_id", "Terrestrial scene requires an appropriate studio rig.")
            if studio.get("mode") == "studio":
                self.require(ambient_scale == 0, path + ".environment_overrides", "Calibrated studio composition must explicitly remove unaccounted environment illumination.")
                self.require(solar_count == 0, path, "Calibrated studio composition cannot silently add a solar source.")
            if studio.get("mode") == "lunar_editorial":
                self.require(mode == "editorial", path + ".mode", "Promotional lunar fill must be labeled editorial.")
                self.require(solar_count == 1, path, "Editorial solar-relative fill needs one resolved environment sun.")

    def roadmap_checks(self) -> None:
        graph = self.indices.get("roadmap.json", {})
        for identity, item in graph.items():
            path = "roadmap.json:" + identity + ".depends_on"
            deps = item.get("depends_on")
            if self.string_list(deps, path, nonempty=False, unique=True):
                for dep in deps:
                    self.reference(dep, "roadmap.json", path)
                    self.require(dep != identity, path, "A phase cannot depend on itself.")
        visiting, visited = set(), set()

        def visit(identity: str) -> None:
            if identity in visiting:
                self.error("roadmap.json:" + identity, "Dependency cycle detected.")
                return
            if identity in visited:
                return
            visiting.add(identity)
            deps = graph.get(identity, {}).get("depends_on", [])
            if isinstance(deps, list):
                for dep in deps:
                    if isinstance(dep, str) and dep in graph:
                        visit(dep)
            visiting.remove(identity)
            visited.add(identity)

        for identity in graph:
            visit(identity)

    def support_checks(self) -> None:
        """Structural and original analytic checks; no simulation is performed."""
        for filename in ("selene-bindings.json", "lunar-physics-fixtures.json", "provenance.json", "ui-integration.json"):
            doc = self.documents.get(filename)
            if doc is None:
                continue
            meta = metadata(doc)
            self.require(meta.get("schema_version") == SCHEMA_VERSION, filename + ".metadata.schema_version", "Expected v0.2 schema version.")
            self.require(meta.get("language") == "en", filename + ".metadata.language", "Expected English metadata.")
            self.text(meta.get("status"), filename + ".metadata.status")
            self.require(any(k != "metadata" for k in doc), filename, "Support document must contain substantive data.")
        self.provenance_checks()
        self.selene_checks()
        self.physics_checks()

    def support_index(self, entries: Any, path: str) -> dict[str, dict[str, Any]]:
        """Index a support collection without prescribing external ID sequences."""
        if not self.require(isinstance(entries, list) and bool(entries), path, "Expected a nonempty array of identified records."):
            return {}
        result, names = {}, set()
        for i, raw in enumerate(entries):
            itempath = f"{path}[{i}]"
            item = self.obj(raw, itempath)
            identity, name = item.get("id"), item.get("name")
            if self.text(identity, itempath + ".id"):
                self.require(identity not in result, itempath + ".id", "Duplicate support-record ID.")
                result[identity] = item
            if self.text(name, itempath + ".name"):
                self.require(name.casefold() not in names, itempath + ".name", "Duplicate support-record name.")
                names.add(name.casefold())
        return result

    def provenance_checks(self) -> None:
        filename = "provenance.json"
        doc = self.documents.get(filename)
        if doc is None:
            return
        records = self.support_index(doc.get("items"), filename + ".items")
        sources = self.support_index(doc.get("sources"), filename + ".sources")
        self.require(not set(records).intersection(sources), filename, "Authorship and reference IDs must be distinct.")
        self.require("ORIGINAL-2026-10-CATALOG" in records, filename + ".items", "Original catalog provenance record is required.")
        for identity, item in records.items():
            path = filename + ":" + identity
            for key in ("license", "author", "authorship"):
                self.text(item.get(key), path + "." + key)
            for key in ("external_assets_bundled", "commercial_product_extraction", "gpl_implementation_reference"):
                self.require(item.get(key) is False, path + "." + key, "Clean-room package must not claim bundled external assets or extracted implementations.")
            if self.string_list(item.get("sources"), path + ".sources", unique=True):
                for source in item["sources"]:
                    self.require(source in sources, path + ".sources", f"Unknown source {source!r}.")
        for identity, source in sources.items():
            path = filename + ":" + identity
            for key in ("url", "use", "accessed", "redistribution"):
                self.text(source.get(key), path + "." + key)
            self.require(isinstance(source.get("url"), str) and source["url"].startswith("https://"),
                         path + ".url", "Reference URL must use HTTPS.")
            self.require(isinstance(source.get("accessed"), str) and re.fullmatch(r"\d{4}-\d{2}-\d{2}", source["accessed"]) is not None,
                         path + ".accessed", "Expected a YYYY-MM-DD access date.")

    def selene_checks(self) -> None:
        filename = "selene-bindings.json"
        doc = self.documents.get(filename)
        if doc is None:
            return
        self.require(metadata(doc).get("status") == "historical_reference_only"
                     and metadata(doc).get("active_demo_source") is False
                     and metadata(doc).get("superseded_by") == "../../assets/render/selene/source-lock.json",
                     filename + ".metadata", "The original v7 inventory must remain explicitly ineligible as the active demo source.")
        source = self.obj(doc.get("source"), filename + ".source")
        for key in ("repository", "path", "format", "source_id"):
            self.text(source.get(key), filename + ".source." + key)
        self.require(isinstance(source.get("commit"), str) and re.fullmatch(r"[0-9a-f]{40}", source["commit"]) is not None,
                     filename + ".source.commit", "Source must be pinned to a 40-character commit SHA.")
        self.require(isinstance(source.get("sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", source["sha256"]) is not None,
                     filename + ".source.sha256", "Model identity requires a SHA-256 digest.")
        self.number(source.get("size_bytes"), filename + ".source.size_bytes", positive=True, integer=True)
        self.require(source.get("units") == "mm" and source.get("up_axis") == "+Z", filename + ".source", "Model must declare millimeters and Z-up.")
        counts = self.obj(doc.get("observed_counts"), filename + ".observed_counts")
        for key, value in counts.items():
            self.number(value, filename + ".observed_counts." + key, minimum=0, integer=True)
        components = doc.get("components")
        joints = doc.get("joints")
        if not self.require(isinstance(components, list) and isinstance(joints, list), filename, "Components and joints arrays are required."):
            return
        component_ids, occurrence_ids, component_names = set(), set(), set()
        for i, raw in enumerate(components):
            path = f"{filename}.components[{i}]"
            component = self.obj(raw, path)
            identity, name = component.get("component_id"), component.get("name")
            if self.number(identity, path + ".component_id", positive=True, integer=True):
                self.require(identity not in component_ids, path + ".component_id", "Duplicate component ID.")
                component_ids.add(identity)
            if self.text(name, path + ".name"):
                self.require(name not in component_names, path + ".name", "Duplicate component name.")
                component_names.add(name)
            occurrences = component.get("occurrence_ids")
            if self.require(isinstance(occurrences, list) and bool(occurrences), path + ".occurrence_ids", "Component needs an occurrence reference."):
                for occurrence in occurrences:
                    if self.number(occurrence, path + ".occurrence_ids", positive=True, integer=True):
                        self.require(occurrence not in occurrence_ids, path + ".occurrence_ids", "Occurrence is assigned more than once.")
                        occurrence_ids.add(occurrence)
        for i, component in enumerate(components):
            if isinstance(component, dict):
                parent = component.get("parent_component_id")
                self.require(type(parent) is int and parent in component_ids | {0}, f"{filename}.components[{i}].parent_component_id", "Unknown parent component.")
        self.require(len(component_ids) == counts.get("component_definitions_excluding_root"), filename + ".observed_counts", "Component count does not match inspected records.")
        self.require(len(occurrence_ids) == counts.get("occurrences"), filename + ".observed_counts", "Occurrence count does not match inspected records.")
        joint_ids, joint_names = set(), set()
        joint_index = {}
        for i, raw in enumerate(joints):
            path = f"{filename}.joints[{i}]"
            joint = self.obj(raw, path)
            identity, name = joint.get("id"), joint.get("name")
            if self.number(identity, path + ".id", positive=True, integer=True):
                self.require(identity not in joint_ids, path + ".id", "Duplicate joint ID.")
                joint_ids.add(identity)
                joint_index[identity] = joint
            if self.text(name, path + ".name"):
                self.require(name not in joint_names, path + ".name", "Duplicate joint name.")
                joint_names.add(name)
            self.require(joint.get("kind") == "revolute", path + ".kind", "Expected inspected revolute joint.")
            for key in ("a_occurrence_id", "b_occurrence_id"):
                value = joint.get(key)
                self.require(type(value) is int and value in occurrence_ids, path + "." + key, "Joint refers to an unknown occurrence.")
            limits = joint.get("limits_rad")
            if limits is None:
                self.require(joint.get("limit_status") == "not_declared", path + ".limit_status", "Unspecified joint limits must be explicitly identified.")
            elif self.require(isinstance(limits, list) and len(limits) == 2 and all(is_number(x) for x in limits), path + ".limits_rad", "Expected two finite radian limits or null."):
                self.require(limits[0] <= limits[1], path + ".limits_rad", "Joint lower limit exceeds upper limit.")
                self.require(joint.get("limit_status") == "inspected_source_limits", path + ".limit_status", "Declared limits must retain their inspection status.")
        self.require(len(joint_ids) == counts.get("revolute_joints"), filename + ".observed_counts", "Joint count does not match inspected records.")
        chain, arm_joints = doc.get("arm_chain_occurrence_ids"), doc.get("arm_joint_ids")
        if self.require(isinstance(chain, list) and isinstance(arm_joints, list) and len(chain) == len(arm_joints) and bool(chain), filename + ".arm_chain_occurrence_ids", "Arm chain and joint list must have matching nonzero lengths."):
            for i, (occurrence, joint_id) in enumerate(zip(chain, arm_joints)):
                self.require(type(occurrence) is int and occurrence in occurrence_ids, filename + ".arm_chain_occurrence_ids", "Unknown arm occurrence.")
                valid_id = type(joint_id) is int and joint_id in joint_index
                self.require(valid_id, filename + ".arm_joint_ids", "Unknown arm joint.")
                if valid_id:
                    joint = joint_index[joint_id]
                    self.require(joint.get("b_occurrence_id") == occurrence, filename + ".arm_joint_ids", "Arm joint does not terminate at the corresponding occurrence.")
                    if i:
                        self.require(joint.get("a_occurrence_id") == chain[i-1], filename + ".arm_joint_ids", "Arm chain connectivity is inconsistent.")
        vial = self.obj(doc.get("candidate_vial"), filename + ".candidate_vial")
        parent_component, parent_occurrence = vial.get("parent_component_id"), vial.get("parent_occurrence_id")
        self.require(type(parent_component) is int and parent_component in component_ids, filename + ".candidate_vial.parent_component_id", "Unknown vial parent component.")
        self.require(type(parent_occurrence) is int and parent_occurrence in occurrence_ids, filename + ".candidate_vial.parent_occurrence_id", "Unknown vial parent occurrence.")
        self.require(any(isinstance(component, dict)
                         and component.get("component_id") == parent_component
                         and isinstance(component.get("occurrence_ids"), list)
                         and parent_occurrence in component["occurrence_ids"] for component in components),
                     filename + ".candidate_vial.parent_occurrence_id", "Vial parent occurrence does not belong to the selected parent component.")
        self.boolean(vial.get("independent_occurrence_exists"), filename + ".candidate_vial.independent_occurrence_exists")
        self.string_list(vial.get("required_derivative_changes"), filename + ".candidate_vial.required_derivative_changes")
        refs = vial.get("body_references")
        if self.require(isinstance(refs, list) and bool(refs), filename + ".candidate_vial.body_references", "Vial body references are required."):
            body_names, features, transfers = set(), set(), set()
            for i, raw in enumerate(refs):
                path = f"{filename}.candidate_vial.body_references[{i}]"
                body = self.obj(raw, path)
                name = body.get("exact_body_name")
                if self.text(name, path + ".exact_body_name"):
                    self.require(name not in body_names, path + ".exact_body_name", "Duplicate vial body name.")
                    body_names.add(name)
                feature = body.get("source_feature_id")
                if self.number(feature, path + ".source_feature_id", positive=True, integer=True):
                    self.require(feature not in features, path + ".source_feature_id", "Duplicate source-feature reference.")
                    features.add(feature)
                ids = body.get("transfer_feature_ids")
                if self.require(isinstance(ids, list) and bool(ids), path + ".transfer_feature_ids", "Transfer-feature IDs are required."):
                    for value in ids:
                        if self.number(value, path + ".transfer_feature_ids", positive=True, integer=True):
                            self.require(value not in transfers, path + ".transfer_feature_ids", "Duplicate transfer-feature reference.")
                            transfers.add(value)
                for key in ("moves_with_vial", "contents"):
                    self.boolean(body.get(key), path + "." + key)
        policy = self.obj(doc.get("binding_policy"), filename + ".binding_policy")
        for key in ("match", "ambiguous_names", "source_geometry", "appearance_scope", "runtime_support"):
            self.text(policy.get(key), filename + ".binding_policy." + key)
        self.require(policy.get("native_model_bundled") is False, filename + ".binding_policy.native_model_bundled", "Reference-only package must not claim a bundled native model.")
        self.string_list(doc.get("attribution_requirements"), filename + ".attribution_requirements")
        hints = doc.get("appearance_role_hints")
        if self.require(isinstance(hints, list) and bool(hints), filename + ".appearance_role_hints", "Reviewed appearance role hints are required."):
            roles = set()
            for i, raw in enumerate(hints):
                path = f"{filename}.appearance_role_hints[{i}]"
                hint = self.obj(raw, path)
                role = hint.get("role")
                if self.text(role, path + ".role"):
                    self.require(role not in roles, path + ".role", "Duplicate appearance role.")
                    roles.add(role)
                self.reference(hint.get("material_concept_id"), "materials.json", path + ".material_concept_id")
                self.text(hint.get("scope_note"), path + ".scope_note")
                ids = hint.get("occurrence_ids")
                if self.require(isinstance(ids, list), path + ".occurrence_ids", "Expected occurrence-hint array."):
                    for occurrence in ids:
                        self.require(type(occurrence) is int and occurrence in occurrence_ids,
                                     path + ".occurrence_ids", "Appearance hint refers to an unknown source occurrence.")
                    expected_status = "requires_reviewed_body_or_face_selection" if ids else "requires_explicit_derivative_geometry_if_used"
                    self.require(hint.get("assignment_status") == expected_status, path + ".assignment_status", "Role hints must retain explicit target-review or derivative-geometry status.")

    def close_expected(self, observed: Any, expected: Any, tolerance: float, path: str) -> None:
        if isinstance(expected, list):
            if self.require(isinstance(observed, list) and len(observed) == len(expected), path, "Expected matching numeric vector length."):
                for i, (a, b) in enumerate(zip(observed, expected)):
                    self.close_expected(a, b, tolerance, f"{path}[{i}]")
        elif self.number(observed, path):
            self.require(abs(observed - expected) <= tolerance, path,
                         f"Analytic mismatch: expected {expected!r}, observed {observed!r}, absolute tolerance {tolerance!r}.")

    def physics_checks(self) -> None:
        """Recompute closed-form fixtures only; this does not integrate dynamics."""
        filename = "lunar-physics-fixtures.json"
        doc = self.documents.get(filename)
        if doc is None:
            return
        records = self.support_index(doc.get("items"), filename + ".items")
        self.counts[filename] = len(records)
        self.require(set(records) == {f"PHY-{i:03d}" for i in range(1, 7)}, filename + ".items", "Expected exactly PHY-001 through PHY-006.")
        for identity, item in records.items():
            path = filename + ":" + identity
            self.text(item.get("interpretation"), path + ".interpretation")
            kind = item.get("kind")
            self.text(kind, path + ".kind")
            tolerance = item.get("absolute_tolerance", 1e-12)
            if not self.number(tolerance, path + ".absolute_tolerance", positive=True, maximum=1e-6):
                continue
            if kind == "ballistic_equal_height":
                keys = ("gravity_m_s2", "initial_position_m", "initial_velocity_m_s")
                if not all([self.vector(item.get(k), path + "." + k) for k in keys]):
                    continue
                g, p, v = (item[k] for k in keys)
                if not self.require(g[0] == 0 and g[1] == 0 and g[2] < 0 and v[2] > 0, path, "Equal-height analytic fixture requires downward Z gravity and upward release."):
                    continue
                apex_time, flight_time = -v[2]/g[2], -2*v[2]/g[2]
                position = lambda t: [p[i]+v[i]*t+0.5*g[i]*t*t for i in range(3)]
                expected = {
                    "apex_time_s": apex_time, "flight_time_s": flight_time,
                    "rise_m": -v[2]*v[2]/(2*g[2]),
                    "range_m": math.hypot(v[0], v[1])*flight_time,
                    "apex_position_m": position(apex_time),
                    "landing_position_m": position(flight_time),
                    "landing_velocity_m_s": [v[i]+g[i]*flight_time for i in range(3)],
                }
                saved = self.obj(item.get("expected"), path + ".expected")
                for key, value in expected.items():
                    self.close_expected(saved.get(key), value, tolerance, path + ".expected." + key)
                self.string_list(item.get("assumptions"), path + ".assumptions")
            elif kind == "unit_conversion":
                inputs = self.obj(item.get("inputs"), path + ".inputs")
                saved = self.obj(item.get("expected"), path + ".expected")
                conversions = (
                    ("position_mm", "position_m", 1e-3),
                    ("velocity_mm_s", "velocity_m_s", 1e-3),
                    ("gravity_mm_s2", "gravity_m_s2", 1e-3),
                    ("inertia_kg_mm2", "inertia_kg_m2", 1e-6),
                    ("torque_n_mm", "torque_n_m", 1e-3),
                )
                for source, destination, factor in conversions:
                    if self.vector(inputs.get(source), path + ".inputs." + source):
                        self.close_expected(saved.get(destination), [x*factor for x in inputs[source]], tolerance, path + ".expected." + destination)
            elif kind == "release_velocity":
                keys = ("grip_velocity_m_s", "grip_angular_velocity_rad_s", "grip_to_center_of_mass_m")
                if not all([self.vector(item.get(k), path + "." + k) for k in keys]):
                    continue
                velocity, omega, r = (item[k] for k in keys)
                cross = [omega[1]*r[2]-omega[2]*r[1], omega[2]*r[0]-omega[0]*r[2], omega[0]*r[1]-omega[1]*r[0]]
                self.close_expected(item.get("expected_center_of_mass_velocity_m_s"), [velocity[i]+cross[i] for i in range(3)], tolerance, path + ".expected_center_of_mass_velocity_m_s")
            elif kind == "principal_inertia_admissibility":
                self.require(item.get("units") == "kg*m^2", path + ".units", "Inertia fixture must use kg*m^2.")
                cases = item.get("cases")
                if not self.require(isinstance(cases, list) and bool(cases), path + ".cases", "Inertia fixture requires positive and negative cases."):
                    continue
                self.require(any(isinstance(c, dict) and c.get("expected_valid") is True for c in cases)
                             and any(isinstance(c, dict) and c.get("expected_valid") is False for c in cases),
                             path + ".cases", "Inertia fixture must include both admissible and inadmissible cases.")
                for i, raw in enumerate(cases):
                    casepath = f"{path}.cases[{i}]"
                    case = self.obj(raw, casepath)
                    if self.vector(case.get("moments"), casepath + ".moments"):
                        a, b, c = case["moments"]
                        valid = min(a, b, c) > 0 and a+b >= c and a+c >= b and b+c >= a
                        self.boolean(case.get("expected_valid"), casepath + ".expected_valid")
                        self.require(case.get("expected_valid") is valid, casepath + ".expected_valid", "Admissibility disagrees with positive moments and triangle inequalities.")
            elif kind == "sample_mass":
                keys = ("container_mass_kg", "collected_sample_mass_kg", "spilled_sample_mass_kg")
                if not all([self.number(item.get(k), path + "." + k, minimum=0) for k in keys]):
                    continue
                container, sample, spilled = (item[k] for k in keys)
                self.require(container > 0, path + ".container_mass_kg", "Container mass must be positive.")
                self.require(spilled <= sample, path + ".spilled_sample_mass_kg", "Spilled mass cannot exceed collected mass.")
                self.close_expected(item.get("expected_retained_sample_mass_kg"), sample-spilled, tolerance, path + ".expected_retained_sample_mass_kg")
                self.close_expected(item.get("expected_total_sealed_mass_kg"), container+sample-spilled, tolerance, path + ".expected_total_sealed_mass_kg")
            else:
                self.error(path + ".kind", "Unknown analytic fixture kind.")

    def ui_checks(self) -> None:
        filename = "ui-integration.json"
        doc = self.documents.get(filename)
        if doc is None:
            return
        meta = metadata(doc)
        self.require(meta.get("status") == "specified_unbound" and meta.get("runtime_ready") is False,
                     filename + ".metadata", "The UI contract describes unbound intent, not implemented UI.")
        snapshots = self.obj(doc.get("source_snapshots"), filename + ".source_snapshots")
        for kind in ("integration_base", "design_reference"):
            record = self.obj(snapshots.get(kind), filename + ".source_snapshots." + kind)
            self.require(isinstance(record.get("commit"), str) and re.fullmatch(r"[0-9a-f]{40}", record["commit"]) is not None,
                         filename + ".source_snapshots." + kind, "Source snapshots require exact Git commit identities.")
        records = doc.get("source_records")
        if self.require(isinstance(records, list) and bool(records), filename + ".source_records", "Source records are required."):
            seen = set()
            self.counts["ui-integration.json source records"] = len(records)
            for index, raw in enumerate(records):
                path = f"{filename}.source_records[{index}]"
                record = self.obj(raw, path)
                identity = record.get("id")
                if self.text(identity, path + ".id"):
                    self.require(identity not in seen, path + ".id", "Duplicate source record.")
                    seen.add(identity)
                source_path = record.get("path")
                self.require(isinstance(source_path, str) and bool(source_path) and not Path(source_path).is_absolute()
                             and ".." not in Path(source_path).parts, path + ".path", "Source paths must be repository relative.")
                for field in ("baseline_git_blob", "reference_git_blob"):
                    self.require(isinstance(record.get(field), str) and re.fullmatch(r"[0-9a-f]{40}", record[field]) is not None,
                                 path + "." + field, "Expected an exact Git blob identity.")
                self.require(record.get("identical_between_revisions") is (record.get("baseline_git_blob") == record.get("reference_git_blob")),
                             path, "Source comparison must match recorded blob identities.")
        workspace = self.obj(doc.get("workspace"), filename + ".workspace")
        self.require(workspace.get("visible_tab_ids_at_baseline") == ["RENDER"] and workspace.get("group_is_tab") is False,
                     filename + ".workspace", "The baseline has one RENDER tab; SCENE and IMAGE are groups.")
        navigation = object_value(workspace.get("navigation"))
        self.require(navigation.get("handler") == "navigation::handle" and navigation.get("pick_at_baseline") is False,
                     filename + ".workspace.navigation", "Do not invent current Render picking or a separate camera controller.")
        disclosure = self.obj(doc.get("disclosure"), filename + ".disclosure")
        self.require(disclosure.get("default") == "Quick" and disclosure.get("levels") == ["Quick", "Detailed", "Expert"]
                     and disclosure.get("global_mode_switch") is False, filename + ".disclosure", "Use one inspector with progressive disclosure.")
        self.number(disclosure.get("quick_adjustment_budget"), filename + ".disclosure.quick_adjustment_budget", minimum=1, maximum=6, integer=True)
        library = self.obj(doc.get("library"), filename + ".library")
        self.require(library.get("category_labels") == ["Scenes", "Materials", "Lighting", "Environments"]
                     and library.get("category_is_workspace") is False, filename + ".library", "Library types are local category navigation.")
        recommendations = doc.get("quality_recommendations")
        if self.require(isinstance(recommendations, list), filename + ".quality_recommendations", "Expected recommendations."):
            for index, raw in enumerate(recommendations):
                path = f"{filename}.quality_recommendations[{index}]"
                record = self.obj(raw, path)
                values = record.get("preset_ids")
                if self.string_list(values, path + ".preset_ids", unique=True):
                    self.require(len(values) == 3, path, "Recommend three exact profiles, with the complete picker still available.")
                    for value in values:
                        self.reference(value, "quality-presets.json", path + ".preset_ids")
        reused, proposed = doc.get("command_reuse"), doc.get("proposed_commands")
        if self.require(isinstance(reused, list) and isinstance(proposed, list), filename, "Command reuse and proposal arrays are required."):
            ids = set()
            for raw in reused:
                record = self.obj(raw, filename + ".command_reuse")
                identity = record.get("id")
                if self.text(identity, filename + ".command_reuse.id"):
                    self.require(identity not in ids, filename + ".command_reuse", "Duplicate existing command.")
                    ids.add(identity)
                self.require(record.get("status") == "registered_at_baseline", filename + ".command_reuse", "Declare existing-command evidence scope.")
            for raw in proposed:
                record = self.obj(raw, filename + ".proposed_commands")
                identity = record.get("candidate_id")
                if self.text(identity, filename + ".proposed_commands.candidate_id"):
                    self.require(identity not in ids, filename + ".proposed_commands", "Proposed commands cannot duplicate existing routes.")
                    ids.add(identity)
                self.require(record.get("status") == "unregistered_proposal", filename + ".proposed_commands", "Do not claim new runtime registration.")
        self.require(metadata(self.documents.get("ux.json", {})).get("integration_contract") == "ui-integration.json",
                     "ux.json.metadata.integration_contract", "UX requirements must identify their UI integration contract.")

    def run(self) -> dict[str, Any]:
        self.load()
        self.collection_checks()
        self.required_fields()
        self.reference_checks()
        self.quality_checks()
        self.baseline_checks()
        self.studio_checks()
        self.environment_checks()
        self.scene_checks()
        self.roadmap_checks()
        self.support_checks()
        self.ui_checks()
        return {
            "valid": not self.errors,
            "schema_version": SCHEMA_VERSION,
            "directory": str(self.directory),
            "counts": self.counts,
            "errors": self.errors,
            "warnings": self.warnings,
            "scope": "Read-only concept data and analytic planning validation; no renderer or simulation execution.",
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("directory", nargs="?", type=Path, default=Path(__file__).resolve().parent,
                        help="Package directory (default: directory containing this script).")
    parser.add_argument("--json", action="store_true", help="Write a machine-readable JSON result to stdout.")
    args = parser.parse_args()
    validator = Validator(args.directory.resolve())
    report = validator.run()
    if args.json:
        print(json.dumps(report, indent=2, ensure_ascii=False, allow_nan=False))
    else:
        print("PASS" if report["valid"] else "FAIL", "— SolidCraft v0.2 catalog validation")
        for filename, count in report["counts"].items():
            print(f"  {filename}: {count}")
        for level in ("errors", "warnings"):
            for issue in report[level]:
                print(f"  {level[:-1].upper()} {issue['path']}: {issue['message']}")
        print(f"{len(report['errors'])} error(s), {len(report['warnings'])} warning(s).")
        print(report["scope"])
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    sys.exit(main())
