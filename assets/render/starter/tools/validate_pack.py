#!/usr/bin/env python3
"""Run bounded authored-resource validation without changing the pack."""
from __future__ import annotations

import json
from collections import Counter
from pathlib import Path
import subprocess
import sys

import materials


def main():
    pack = materials.PACK
    root = materials.ROOT
    try:
        data = materials.load_json(pack / "pack.json")
        if data.get("schema") != "solidcraft-authored-render-resources/1":
            raise ValueError("unknown pack schema")
        if data.get("runtime_ready") is not False or data.get("native_library_format") is not False:
            raise ValueError("unsupported runtime/library availability claim")
        if data.get("license") != "MIT OR Apache-2.0" or data.get("version") != materials.VERSION:
            raise ValueError("invalid pack identity")
        expected = {"materials": ("materials/manifest.json", 48),
                    "environments": ("environments/manifest.json", 24),
                    "lighting": ("lighting/exports/rigs.json", 24)}
        if not isinstance(data.get("collections"), dict) or set(data["collections"]) != set(expected):
            raise ValueError("invalid collection list")
        for name, (path, count) in expected.items():
            entry = data["collections"][name]
            if entry.get("manifest") != path or type(entry.get("count")) is not int or entry["count"] != count:
                raise ValueError(f"incorrect {name} contract")
        # Child validators verify source identity, actual payloads and semantics.
        for script in ("materials.py", "environments.py", "lighting.py"):
            subprocess.run([sys.executable, "-B", str(pack / "tools" / script), "validate"],
                           cwd=root, check=True)
        actual_surfaces = len(materials.load_json(pack / "materials/manifest.json")["items"])
        actual_pngs = len(list((pack / "materials/exports").glob("SURF-*/*.png")))
        actual_hdrs = len(list((pack / "environments/exports").glob("*.hdr")))
        environment_manifest = materials.load_json(pack / "environments/manifest.json")
        lighting_manifest = materials.load_json(pack / "lighting/exports/rigs.json")
        if len(environment_manifest["items"]) != 24 or len(lighting_manifest["items"]) != 24:
            raise ValueError("child collection count mismatch")
        actual_states = dict(Counter(item["state"] for item in lighting_manifest["items"]))
        if data["collections"]["lighting"].get("status_counts") != actual_states:
            raise ValueError("lighting availability summary mismatch")
        if data["collections"]["lighting"].get("runtime_ready") is not False:
            raise ValueError("unsupported lighting runtime availability")
        if data["collections"]["environments"].get("scope") != "illumination_only":
            raise ValueError("unsupported environment geometry availability")
        if (actual_surfaces, actual_pngs, actual_hdrs) != (48, 240, 24):
            raise ValueError("export collection count mismatch")
        if data["collections"]["materials"].get("export_count") != actual_pngs or data["collections"]["environments"].get("export_count") != actual_hdrs:
            raise ValueError("export manifest count mismatch")
        attribution = (root / "ATTRIBUTION.md").read_text(encoding="utf-8")
        for path in sorted(pack.rglob("*")):
            if path.is_file() and path.suffix != ".md":
                relative = path.relative_to(root).as_posix()
                if f"| {relative} |" not in attribution:
                    raise ValueError(f"missing exact attribution: {relative}")
        print(json.dumps({"pack_status": "pass", "surfaces": actual_surfaces,
                          "png_exports": actual_pngs, "hdr_exports": actual_hdrs,
                          "runtime_ready": False}, indent=2))
        return 0
    except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.CalledProcessError) as error:
        print(f"render pack validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
