#!/usr/bin/env python3
"""Select the workspace packages a pull request affects.

Usage: affected.py --paths FILE [--metadata FILE] [--github-output FILE]

FILE holds the changed paths relative to the repository root, separated by NUL (git diff -z);
"-" reads standard input. --metadata is the output of `cargo metadata --no-deps
--format-version 1` (run on demand when omitted).

Prints and optionally appends to $GITHUB_OUTPUT:
  full=true|false   test the whole workspace
  packages=a b c    the affected packages (empty when full, or when nothing needs building)

A changed file maps to the package whose directory holds it. The packages that depend on those
(directly or not, dev-dependencies included) are added. Test data under tests/ maps to the
packages that read it, a vendored crate (vendor/, a [patch] of a crates.io dependency) to the
packages that use it, and a root file or folder a crate embeds to that crate. Anything that can
change every package, or that no package owns, selects the full workspace. Documentation,
packaging, the other workflows and the GitHub bots' scripts are ignored: when only those change,
nothing is selected and full is false, which CI reads as "no code changed" (on pull requests and
on pushes to main alike).
"""
import argparse
import json
import subprocess
import sys

# Changes here can affect every package. The CI workflow itself is one; other workflows are not.
FULL_PREFIXES = (".cargo/", ".config/", "scripts/")
FULL_FILES = ("Cargo.toml", "Cargo.lock", ".github/workflows/ci.yml")
# Changes here affect no code: documentation, packaging, the other workflows and GitHub settings,
# and the GitHub bots' scripts (tested in the repository checks). Root *.md files too, unless
# embedded (below).
IGNORED_PREFIXES = ("docs/", "packaging/", ".github/", "scripts/claim/", "scripts/triage/")
IGNORED_FILES = ("vendor/README.md",)
# Root files that a crate embeds (include_str!), so they are not ignorable documentation.
EMBEDDED = {
    "ATTRIBUTION.md": "solidcraft-ui-egui",
    "LICENSE-MIT": "solidcraft-engine",
    "LICENSE-APACHE": "solidcraft-engine",
    "NOTICE": "solidcraft-engine",
    "THIRD-PARTY-LICENSES.txt": "solidcraft-engine",
}
# Folders that only some packages read: the UI embeds the catalogs and xtask checks them; the MCP
# server's tests read the agent skills.
EMBEDDED_PREFIXES = {"assets/locales/": ("solidcraft-ui-egui", "xtask"), "skills/": ("solidcraft-mcp",)}
# Test data and the package directories whose tests (or xtask) read it. Any other folder under
# tests/ selects the full workspace.
TEST_DATA = {
    "tests/hole_matrix/": ("crates/engine/",),
    "tests/projection/": ("crates/engine/",),
    "tests/taper/": ("crates/engine/",),
    "tests/oracle/": ("apps/solidcraft-cli/", "xtask/"),
    "tests/action_matrix/": ("apps/solidcraft-cli/", "crates/engine/", "xtask/"),
}
# Vendored crates (vendor/<crate>/, a [patch] of a crates.io dependency) and the dependency names
# through which workspace packages reach them: egui-wgpu directly or through eframe. Another
# vendored crate selects the full workspace.
VENDOR_USES = {
    "egui-wgpu": lambda dep: dep in ("egui-wgpu", "eframe"),
}


def load_metadata(path):
    if path:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        check=True, capture_output=True, text=True,
    ).stdout
    return json.loads(out)


def read_paths(src):
    data = sys.stdin.buffer.read() if src == "-" else open(src, "rb").read()
    return [p.decode() for p in data.split(b"\0") if p]


def select(meta, paths):
    """Return (full, sorted package names)."""
    root = meta["workspace_root"].replace("\\", "/").rstrip("/") + "/"
    pkgs = {}
    for p in meta["packages"]:
        manifest = p["manifest_path"].replace("\\", "/")
        assert manifest.startswith(root), manifest
        pkgs[p["name"]] = manifest[len(root):].rsplit("/", 1)[0] + "/"
    by_dir = {d: n for n, d in pkgs.items()}
    # Reverse dependencies inside the workspace, matched by the dependency's path.
    dependents = {n: set() for n in pkgs}
    for p in meta["packages"]:
        for d in p["dependencies"]:
            path = d.get("path")
            if not path:
                continue
            rel = path.replace("\\", "/").rstrip("/") + "/"
            target = by_dir.get(rel[len(root):]) if rel.startswith(root) else None
            if target:
                dependents[target].add(p["name"])

    def users_of(vendored):
        """The workspace packages that depend on a vendored crate (by dependency name)."""
        uses = VENDOR_USES.get(vendored)
        if uses is None:
            return set()
        return {p["name"] for p in meta["packages"] if any(uses(d["name"]) for d in p["dependencies"])}

    hit = set()
    for path in paths:
        if path in FULL_FILES or path.startswith("rust-toolchain"):
            return True, []
        if path in IGNORED_FILES or path.startswith(IGNORED_PREFIXES):
            continue
        if path.startswith(FULL_PREFIXES):
            return True, []
        owners = [d for d in by_dir if path.startswith(d)]
        if path.startswith("vendor/"):
            users = users_of(path.split("/")[1])
            if not users:
                return True, []
            hit.update(users)
        elif path.startswith("tests/"):
            data = next((v for k, v in TEST_DATA.items() if path.startswith(k)), None)
            readers = [by_dir[d] for d in data or () if d in by_dir]
            if not readers:
                return True, []
            hit.update(readers)
        elif owners:
            hit.add(by_dir[max(owners, key=len)])
        elif path in EMBEDDED:
            hit.add(EMBEDDED[path])
        elif any(path.startswith(p) for p in EMBEDDED_PREFIXES):
            hit.update(next(v for p, v in EMBEDDED_PREFIXES.items() if path.startswith(p)))
        elif "/" not in path and path.endswith(".md"):
            continue
        else:
            return True, []  # not owned by any package

    todo, seen = list(hit), set(hit)
    while todo:
        for dep in dependents[todo.pop()]:
            if dep not in seen:
                seen.add(dep)
                todo.append(dep)
    return False, sorted(seen)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--paths", required=True)
    ap.add_argument("--metadata")
    ap.add_argument("--github-output")
    a = ap.parse_args()
    full, packages = select(load_metadata(a.metadata), read_paths(a.paths))
    lines = [f"full={'true' if full else 'false'}", f"packages={' '.join(packages)}"]
    print("\n".join(lines))
    if a.github_output:
        with open(a.github_output, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
