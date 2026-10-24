#!/usr/bin/env python3
"""One native SELENE evaluation/device for reviewed, repeatable product renders.

Developer orchestration only. Does not rewrite native document JSON, mutate the
source file, synthesize images, install packages, or silently change a request.
Run with python -i to inspect the captured scene and submit explicit job JSON.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import sys
import threading
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "assets/render/selene/tools"))
from validate_native import NativeMcp, digest, require

MODEL_HASH = "3bd2bfa1c6336394c4a4208d30b52c80d06658a3bd7ea57cc93b774411d1bd68"


class RenderMcp(NativeMcp):
    """Use the existing bounded protocol, with an explicit GPU and scoped exports."""
    def __init__(self, cli: Path, model: Path, directory: Path):
        self.timeout = 300
        self.sequence = 0
        self.messages = queue.Queue(maxsize=32)
        self.stderr = (directory / "native.stderr.log").open("xb")
        self.transcript = (directory / "native.rpc.jsonl").open("x", encoding="utf-8")
        env = os.environ.copy()
        for key in list(env):
            if key.startswith(("SOLIDCRAFT_", "SOLVECRAFT_", "WGPU_")):
                env.pop(key)
        for name in ("temporary", "recovery", "crash", "control"):
            (directory / name).mkdir()
        env.update(RAYON_NUM_THREADS="2", SOLIDCRAFT_GPU="hardware", SOLIDCRAFT_REQUIRE_GPU="1",
                   WGPU_BACKEND="vulkan", RUST_LOG="solidcraft_gpu::path=info,wgpu_core=warn",
                   TEMP=str(directory / "temporary"), TMP=str(directory / "temporary"),
                   SOLIDCRAFT_RECOVERY_DIR=str(directory / "recovery"),
                   SOLIDCRAFT_CRASH_DIR=str(directory / "crash"),
                   SOLIDCRAFT_CONTROL_DIR=str(directory / "control"))
        self.process = subprocess.Popen(
            [str(cli), "mcp", "--in", str(model), "--render-gpu", "--allow-path", str(directory)],
            cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr, env=env)
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        try:
            self.server = self.request("initialize", {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "selene-render-studio", "version": "1.0"}}, timeout=1200)
            self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        except BaseException:
            self.close()
            raise


class Studio:
    def __init__(self, cli: Path, model: Path, output: Path):
        self.cli, self.model, self.output = cli.resolve(), model.resolve(), output.resolve()
        require(self.cli.is_file() and self.model.is_file(), "Missing CLI or native model")
        require(digest(self.model) == MODEL_HASH, "The selected model is not the reviewed latest SELENE master")
        require(not self.output.exists(), "Use a new output directory")
        self.output.mkdir(parents=True)
        self.closed = False
        self.report = {"status": "OPEN", "model_sha256": MODEL_HASH, "cli_sha256": digest(self.cli),
                       "model": str(self.model), "source_commit": self.git("rev-parse", "HEAD"),
                       "source_tree": self.git("rev-parse", "HEAD^{tree}"), "renders": [],
                       "scope": "Native product rendering of the pinned editable functional SELENE master"}
        self.native = RenderMcp(self.cli, self.model, self.output)
        self.scene = self.call("render.scene", {})
        require(isinstance(self.scene.get("bodies"), list) and self.scene["bodies"], "Empty captured scene")
        self.write("scene.json", self.scene)
        self.report["visible_bodies"] = len(self.scene["bodies"])
        self.report["visible_triangles"] = sum(b["triangles"] for b in self.scene["bodies"])
        self.report["bounds"] = self.scene["bounds"]
        self.write("session.json", self.report)
        print(json.dumps({k: self.report[k] for k in ("status", "visible_bodies", "visible_triangles", "bounds")}), flush=True)

    @staticmethod
    def git(*args: str) -> str:
        return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()

    def write(self, name: str, value: dict) -> None:
        (self.output / name).write_text(json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8")

    def call(self, command: str, params: dict) -> dict:
        require(not self.closed, "Studio is closed")
        result = self.native.tool("execute", {"command": command, "params": params, "verbose": False})
        # The execute MCP envelope is distinct from a command's own response.
        if result.get("ok") is False:
            raise RuntimeError(json.dumps(result, ensure_ascii=False))
        return result.get("result", result)

    def render(self, name: str, params: dict, timeout: int = 3600) -> dict:
        require(re.fullmatch(r"[a-z0-9][a-z0-9-]{0,80}", name) is not None, "Invalid render label")
        require(1 <= timeout <= 7200, "Render timeout must be 1..7200 seconds")
        path = self.output / (name + ".png")
        require(not path.exists(), "Never overwrite a completed image")
        require(digest(self.model) == MODEL_HASH, "Native authority changed")
        self.write(name + "-request.json", params)
        receipt = {"name": name, "status": "RUNNING", "source_model_sha256": MODEL_HASH,
                   "cli_sha256": self.report["cli_sha256"], "params": params,
                   "not_a_performance_benchmark": True}
        identity = None
        started = time.monotonic()
        try:
            preflight = self.call("render.preflight", params)
            receipt["preflight"] = preflight
            self.write(name + "-receipt.json", receipt)
            print("PREFLIGHT " + json.dumps(preflight.get("admission", {})), flush=True)
            job = self.call("render.start", params)
            identity = job["job"]
            last = None
            while time.monotonic() - started < timeout:
                status = self.call("render.status", {"job": identity})
                progress = status["progress"]
                update = (progress["phase"], int(100 * progress["done"] / max(1, progress["total"])))
                if update != last:
                    print("PROGRESS " + json.dumps(progress), flush=True)
                    last = update
                if progress["phase"] in ("completed", "cancelled", "failed"):
                    break
                time.sleep(3)
            else:
                raise TimeoutError("Render exceeded its explicit deadline")
            require(status["progress"]["phase"] == "completed", "Native render did not complete: " + json.dumps(status.get("error")))
            exported = self.call("render.export", {"job": identity, "path": str(path)})
            frame = exported["frame"]
            require(path.is_file() and digest(path) == frame["png_sha256"], "Exported bytes differ from the completed frame")
            require(frame["executor"] == "gpu-path", "Unexpected executor")
            require((frame["width"], frame["height"]) == (preflight["request"]["width"], preflight["request"]["height"]), "Unexpected downscale")
            receipt.update(status="PASS", export=exported, elapsed_seconds=round(time.monotonic() - started, 3),
                           source_model_unchanged=digest(self.model) == MODEL_HASH, visual_review="PENDING")
            print("COMPLETED " + json.dumps(exported), flush=True)
        except BaseException as error:
            receipt.update(status="FAIL", error=str(error))
            if identity is not None:
                try:
                    self.call("render.cancel", {"job": identity})
                except (RuntimeError, ValueError, TimeoutError, OSError):
                    pass
            raise
        finally:
            self.write(name + "-receipt.json", receipt)
            self.report["renders"].append({"name": name, "status": receipt["status"], "receipt": name + "-receipt.json"})
            self.write("session.json", self.report)
        return receipt

    def close(self) -> None:
        if not self.closed:
            self.native.close()
            self.closed = True
            self.report.update(status="CLOSED", source_model_unchanged=digest(self.model) == MODEL_HASH,
                               source_tree_after=self.git("rev-parse", "HEAD^{tree}"))
            self.write("session.json", self.report)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--model", type=Path, default=ROOT / "assets/render/selene/models/SELENE-functional.solidcraft")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    studio = Studio(args.cli, args.model, args.out)
    print("Studio is ready. Inspect studio.scene, call studio.render(label, params), then studio.close().", flush=True)
    if not sys.flags.interactive:
        studio.close()
