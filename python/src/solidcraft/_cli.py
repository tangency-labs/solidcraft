"""``solidcraft``: run the ``solidcraft-cli`` this package finds, with every argument passed on.

    uvx solidcraft mcp                       # the MCP server, over stdio
    uvx solidcraft --version
    uvx solidcraft run replay.json --in part.sc
    uvx solidcraft install-cli [--from FILE] [--force]   # put solidcraft-cli in a user bin folder

The binary is the one the client uses (``locate_cli``): ``$SOLIDCRAFT_CLI``, the one downloaded for
this version, a ``PATH`` one of the same minor version or newer, else this version's own, downloaded
once (``_download``). With no arguments it prints the CLI's help.
"""

from __future__ import annotations

import filecmp
import os
import shutil
import subprocess
import sys

from pathlib import Path

from . import _download
from ._mcp import SolidCraftError, locate_cli


def cannot_run(cli: str, source: str, e: OSError) -> str:
    """One line naming the file that failed to start, where it came from and what to do."""
    path = (shutil.which(cli) or cli) if source == "SOLIDCRAFT_CLI" and os.sep not in cli else cli
    why = e.strerror or str(e)
    if os.path.isfile(path) and isinstance(e, FileNotFoundError):
        # The file is there, so what is missing is what it needs to start: a script's interpreter
        # or a binary's loader (a binary for another platform, or a wrapper left behind).
        why = "it exists but cannot start (its interpreter or loader is missing)"
    return f"solidcraft: cannot run {path} ({source}): {why}; {_download.HINT}"


INSTALL_USAGE = "usage: solidcraft install-cli [--from FILE] [--force]"


def user_bin_dir() -> Path:
    if os.name == "nt":
        return Path(os.environ.get("LOCALAPPDATA") or Path.home() / "AppData" / "Local") / "Programs" / "solidcraft"
    return Path(os.environ.get("XDG_BIN_HOME") or Path.home() / ".local" / "bin")


def install_cli(args: list[str]) -> int:
    """``solidcraft install-cli``: copies this version's CLI into a user bin folder. ``--from FILE``
    takes it from a file instead of the network (the release's CLI archive, checked against a
    ``SHA256SUMS.txt`` beside it when there is one, or the binary itself) and keeps it in the cache
    too, so later runs work offline. PATH is never changed; it prints how to add the folder."""
    source, force = None, False
    rest = list(args)
    while rest:
        a = rest.pop(0)
        if a == "--force":
            force = True
        elif a == "--from" and rest:
            source = Path(rest.pop(0))
        elif a.startswith("--from="):
            source = Path(a.split("=", 1)[1])
        else:
            print(INSTALL_USAGE, file=sys.stderr)
            return 2
    version = _download.package_version()
    if not version:
        print("solidcraft: this checkout has no generated bindings, so no version to install; run `cargo xtask python`", file=sys.stderr)
        return 1
    try:
        if source is not None:
            cli = seed_cache(version, source)
        else:
            cli = _download.cached(version) or (None if _download.disabled() else _download.download(version))
            if cli is None:
                print(f"solidcraft: no solidcraft-cli {version} downloaded yet and SOLIDCRAFT_NO_DOWNLOAD is set; use --from FILE", file=sys.stderr)
                return 1
    except (_download.DownloadError, OSError) as e:
        print(f"solidcraft: {e}", file=sys.stderr)
        return 1
    folder = user_bin_dir()
    dest = folder / _download.EXE
    if dest.exists() or dest.is_symlink():
        same = dest.is_file() and filecmp.cmp(cli, dest, shallow=False)
        if not same and not force:
            print(f"solidcraft: {dest} already exists and is another file; pass --force to replace it", file=sys.stderr)
            return 1
    try:
        folder.mkdir(parents=True, exist_ok=True)
        tmp = folder / f".{_download.EXE}.{os.getpid()}"
        shutil.copyfile(cli, tmp)
        tmp.chmod(0o755)
        os.replace(tmp, dest)
    except OSError as e:
        print(f"solidcraft: cannot install to {dest}: {e}", file=sys.stderr)
        return 1
    print(f"installed solidcraft-cli {version} as {dest}")
    on_path = any(Path(p).resolve() == folder.resolve() for p in os.environ.get("PATH", "").split(os.pathsep) if p)
    if not on_path:
        if os.name == "nt":
            print(f"{folder} is not on PATH; to add it for your account, run in PowerShell:")
            print(f"  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';{folder}', 'User')")
        else:
            print(f"{folder} is not on PATH; add this line to your shell profile (~/.profile, ~/.bashrc or ~/.zshrc):")
            print(f'  export PATH="{folder}:$PATH"')
    return 0


def seed_cache(version: str, source: Path) -> str:
    """Puts the CLI from ``source`` (a release archive, or the binary) in this version's cache."""
    if not source.is_file():
        raise _download.DownloadError(f"{source}: no such file")
    name = source.name
    if name.endswith((".zip", ".tar.gz")):
        sums = source.with_name("SHA256SUMS.txt")
        expected = _download.checksums(sums.read_text("utf-8", "replace")).get(name) if sums.is_file() else None
        if sums.is_file() and expected is None:
            raise _download.DownloadError(f"{sums} lists no {name}")
        return _download.install_archive(version, source, name, expected)
    folder = _download.cache_dir(version)
    folder.mkdir(parents=True, exist_ok=True)
    tmp = folder / f".{_download.EXE}.{os.getpid()}"
    shutil.copyfile(source, tmp)
    tmp.chmod(0o755)
    os.replace(tmp, folder / _download.EXE)
    return str(folder / _download.EXE)


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv) or ["--help"]
    if args[0] == "install-cli":
        return install_cli(args[1:])
    try:
        cli, source = locate_cli()
    except SolidCraftError as e:
        print(f"solidcraft: {e}", file=sys.stderr)
        return 1
    try:
        if os.name != "nt":
            sys.stdout.flush()
            sys.stderr.flush()
            os.execvp(cli, [cli, *args])  # a bare SOLIDCRAFT_CLI is looked up on PATH, as Popen does; replaces this process: stdio, signals and exit code are the CLI's
        p = subprocess.Popen([cli, *args])
    except OSError as e:
        print(cannot_run(cli, source, e), file=sys.stderr)
        return 127
    # Windows: Ctrl+C reaches the CLI too (same console); wait for it to finish and pass on its code.
    while True:
        try:
            return p.wait()
        except KeyboardInterrupt:
            continue


if __name__ == "__main__":
    sys.exit(main())
