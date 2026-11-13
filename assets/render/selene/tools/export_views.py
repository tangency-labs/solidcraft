#!/usr/bin/env python3
"""Export three actual native Design previews without saving over the master.

One bounded MCP process opens the complete model once, then changes only its
transient visibility and camera. The product renderer is a separate workflow;
these images deliberately document the Design renderer used in the response.
"""

from __future__ import annotations

import argparse
import base64
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True

import presentation
from validate_native import NativeMcp, digest, require


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--width", type=int, default=1400)
    parser.add_argument("--height", type=int, default=1000)
    args = parser.parse_args()
    require(320 <= args.width <= 2400 and 240 <= args.height <= 1800, "Preview dimensions out of bounds")
    cli, model, output = args.cli.resolve(), args.model.resolve(), args.out.resolve()
    require(cli.is_file() and model.is_file(), "Missing executable or native master")
    require(not output.exists(), "Use a fresh output directory; existing previews are never overwritten")
    output.mkdir(parents=True)
    before = digest(model)
    report = {
        "status": "in_progress", "source_model_sha256": before, "cli_sha256": digest(cli),
        "started_at": datetime.now(timezone.utc).isoformat(), "images": [],
        "scope": "Actual native Design previews. No product PBR, path tracing or physical optical claim.",
    }
    native = None
    try:
        print("Opening the complete native master once for three previews", flush=True)
        native = NativeMcp(cli, model, output, timeout=600, startup_timeout=1200)
        inventory = native.tool("inspect_design", {"measure": False, "sketches": False, "joints": False})
        issues = [f for f in inventory.get("timeline", []) if f.get("error") or f.get("warning")]
        require(not issues and inventory.get("bodies"), "Native model has no bodies or an unhealthy timeline")
        report["evaluated_bodies"] = len(inventory["bodies"])
        shots = [
            ("whole", "SELENE-overview.png", presentation.camera("Whole rover")),
            ("electronics", "SELENE-electronics.png", presentation.camera("Electronics")),
            ("bearing", "SELENE-bearing.png", {
                "target": [242, -7, 281], "yaw": -.18, "pitch": .30, "distance": 27, "fov": 0,
            }),
        ]
        for profile, filename, camera in shots:
            print(f"Rendering native Design preview: {profile}", flush=True)
            for command in presentation.visibility_commands(inventory, profile):
                native.tool("execute", {**command, "verbose": False})
            reply = native.request("tools/call", {"name": "screenshot", "arguments": {
                "source": "model", "width": args.width, "height": args.height,
                "camera": camera, "grid": False, "axes": False,
            }})
            texts = [item.get("text", "") for item in reply.get("content", []) if item.get("type") == "text"]
            require(not reply.get("isError"), "Native screenshot failed: " + " ".join(texts))
            pictures = [item for item in reply.get("content", []) if item.get("type") == "image"]
            require(len(pictures) == 1 and pictures[0].get("mimeType") == "image/png", "Expected one PNG")
            png = base64.b64decode(pictures[0]["data"], validate=True)
            require(png[:8] == b"\x89PNG\r\n\x1a\n" and len(png) > 33, "Invalid PNG header")
            dimensions = struct.unpack(">II", png[16:24])
            require(dimensions == (args.width, args.height), "Unexpected native preview dimensions")
            (output / filename).write_bytes(png)
            report["images"].append({
                "file": filename, "profile": profile, "camera": camera,
                "width": dimensions[0], "height": dimensions[1], "bytes": len(png),
                "sha256": hashlib.sha256(png).hexdigest(),
                "native_capture_metadata": [json.loads(t) for t in texts],
            })
        report["status"] = "passed"
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, TimeoutError) as error:
        report["status"] = "failed"
        report["error"] = str(error)
    finally:
        if native is not None:
            native.close()
        report["source_model_unchanged"] = digest(model) == before
        if not report["source_model_unchanged"]:
            report["status"] = "failed"
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        (output / "export-report.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    print(json.dumps({"status": report["status"], "images": len(report["images"]),
                      "model_unchanged": report["source_model_unchanged"], "report": str(output / "export-report.json")}), flush=True)
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
