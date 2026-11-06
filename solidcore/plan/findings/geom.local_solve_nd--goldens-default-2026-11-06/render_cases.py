"""Reproduce the evidence in an isolated checkout with export-observer.patch applied.

Apply in that isolated checkout: git apply --unidiff-zero PATH/export-observer.patch
Build first: CARGO_BUILD_JOBS=2 nice -n 10 cargo build --profile goldens -p goldens -p diagnostics
Run from that checkout: python3 PATH/render_cases.py before OUTPUT_DIRECTORY
Run again on the corrected kernel with stage after, using the same directory.
The check can return 1 when a result differs; exporting never changes its golden.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

stage, directory = sys.argv[1:]
assert stage in ("before", "after")
output = Path(directory).resolve()
output.mkdir(parents=True, exist_ok=True)
cases = [
    ("tabbed-plate", "issue_2176_tabbed_plate_tab_top_r0_5", "-40,-35", "8"),
    ("degenerate-round", "stubs4-degenerate-brepForFillet2-e9-r0.5", "-130,30", ""),
    ("open-shell", "stubs4-open_shell-e5-r0.5", "-50,-25", ""),
]
for label, case, view, highlight in cases:
    stem = f"{label}-{stage}"
    model = output / (stem + ".smb")
    environment = dict(os.environ, SOLIDCORE_2791_EXPORT=str(model))
    with (output / (stem + ".log")).open("w") as log:
        result = subprocess.run(
            ["nice", "-n", "10", "bin/goldens/goldens", "--unit", "blend.fillet_executive_p3",
             "--filter", case, "--goldens", "--no-report"],
            env=environment, stdout=log, stderr=subprocess.STDOUT,
        )
    assert result.returncode in (0, 1) and model.exists(), case
    command = ["nice", "-n", "10", "bin/goldens/solidcore-diag", str(model),
               "--out", str(output), "--view", view, "--size", "1000x800"]
    if highlight:
        command.extend(["--highlight", highlight])
    subprocess.run(command, check=True)
    if label == "tabbed-plate":
        detail = output / "detail"
        command[command.index("--out") + 1] = str(detail)
        environment = dict(os.environ, SOLIDCORE_2791_VIEWPORT="0,33.111111111111114,2,10")
        subprocess.run(command, check=True, env=environment)
        shutil.copyfile(detail / (stem + ".png"), output / (stem + "-detail.png"))
print(json.dumps({"stage": stage, "directory": str(output)}))
