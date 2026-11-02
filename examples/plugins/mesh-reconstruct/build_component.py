"""Package and validate a pure command-plugin component; no WASI or JavaScript adapters."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def run(args):
    print("Running " + Path(args[0]).name + " " + " ".join(args[1:]), flush=True)
    subprocess.run(args, check=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--wasm-tools", required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    run([args.wasm_tools, "validate", str(args.input)])
    package = args.output_dir / "mesh-reconstruct"
    package.mkdir()
    component = package / "plugin.wasm"
    run([args.wasm_tools, "component", "new", str(args.input), "-o", str(component)])
    run([args.wasm_tools, "validate", "--features", "component-model", str(component)])
    data = component.read_bytes()
    if len(data) > 16 * 1024 * 1024:
        raise ValueError("component exceeds the host's 16 MiB limit")
    (package / "plugin.json").write_bytes(args.manifest.read_bytes())
    receipt = {"input_sha256": hashlib.sha256(args.input.read_bytes()).hexdigest(), "component_sha256": hashlib.sha256(data).hexdigest(), "component_bytes": len(data), "runtime_execution": "pending"}
    (args.output_dir / "component-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    with (args.output_dir / "component.wit").open("wb") as stream:
        subprocess.run([args.wasm_tools, "component", "wit", str(component)], stdout=stream, check=True)
    print(json.dumps(receipt), flush=True)

if __name__ == "__main__":
    main()
