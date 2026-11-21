#!/usr/bin/env python3
"""Check OpenUSD files with OpenUSD's own validators.

Runs every validator the installed OpenUSD registers (UsdValidation) on each file and prints its
findings. Needs OpenUSD's Python bindings: `pip install usd-core`.

    python3 scripts/usd-validate.py [--ignore NAME]... FILE...

Exit status: 0 when no file has a finding (other than the ignored ones), 1 when one has, 2 when a
file does not open, 3 when OpenUSD's Python bindings are not installed (nothing was checked).
"""

import argparse
import sys


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--ignore", action="append", default=[], metavar="NAME", help="a finding name to leave out, such as MissingShaderIdInRegistry")
    parser.add_argument("files", nargs="+", metavar="FILE")
    args = parser.parse_args()
    try:
        from pxr import Usd, UsdValidation
    except ImportError:
        print("usd-validate: OpenUSD's Python bindings are not installed (pip install usd-core)", file=sys.stderr)
        return 3
    context = UsdValidation.ValidationContext(UsdValidation.ValidationRegistry().GetOrLoadAllValidators())
    found = 0
    for path in args.files:
        try:
            stage = Usd.Stage.Open(path)
        except Exception as e:  # OpenUSD raises its own error type for a file it cannot read
            print(f"{path}: does not open: {e}", file=sys.stderr)
            return 2
        if not stage:
            print(f"{path}: does not open", file=sys.stderr)
            return 2
        findings = [e for e in context.Validate(stage) if e.GetName() not in args.ignore]
        print(f"{path}: {len(findings)} findings")
        for e in findings:
            print(f"  {e.GetName()}: {e.GetMessage()}")
        found += len(findings)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
