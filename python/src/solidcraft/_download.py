"""The ``solidcraft-cli`` of this package's own release, downloaded once and kept in the user cache.

The package carries no binary. When neither ``$SOLIDCRAFT_CLI`` nor a recent enough ``solidcraft-cli``
on ``PATH`` is there, the client fetches this platform's CLI archive from the GitHub release of its
exact version, checks it against that release's ``SHA256SUMS.txt`` (a mismatch is refused and
nothing is run), and keeps the binary under the user cache folder for the next run:

    Linux, FreeBSD   $XDG_CACHE_HOME/solidcraft/cli/<version>/  (~/.cache/...)
    macOS            ~/Library/Caches/solidcraft/cli/<version>/
    Windows          %LOCALAPPDATA%\\solidcraft\\cli\\<version>\\

``SOLIDCRAFT_NO_DOWNLOAD=1`` turns the download off. ``SOLIDCRAFT_DOWNLOAD_URL`` names another
folder holding the release's files (a mirror, or ``file://`` for tests and offline installs).
Standard library only.
"""

from __future__ import annotations

import hashlib
import os
import platform
import shutil
import sys
import sysconfig
import tarfile
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

REPO = "tangency-labs/solidcraft"
EXE = "solidcraft-cli.exe" if os.name == "nt" else "solidcraft-cli"
HINT = "install SolidCraft from solidcraft.io or set SOLIDCRAFT_CLI to the path of solidcraft-cli"


class DownloadError(RuntimeError):
    """The CLI could not be downloaded or did not match the release's checksums."""


def package_version() -> str:
    """The SolidCraft version these bindings were generated from (``0.52.0``), or ``""``."""
    try:
        from ._generated import VERSION
    except ImportError:
        return ""
    return VERSION if VERSION and VERSION != "0" else ""


def disabled() -> bool:
    return os.environ.get("SOLIDCRAFT_NO_DOWNLOAD", "").strip().lower() in ("1", "true", "yes")


def asset_names(version: str) -> list[str]:
    """This platform's archives holding ``solidcraft-cli`` in the release, best first: the CLI on its
    own, then the whole package (releases before 0.52 have only that). Empty when there is none."""
    machine = platform.machine().lower()
    if sys.platform.startswith("linux"):
        if platform.libc_ver()[0] != "glibc":
            return []  # the released binary needs glibc (musl has none)
        arch = {"x86_64": "x86_64", "amd64": "x86_64", "aarch64": "aarch64", "arm64": "aarch64"}.get(machine)
        return [f"solidcraft-cli-{version}-linux-{arch}.tar.gz", f"solidcraft-{version}-linux-{arch}.tar.gz"] if arch else []
    if sys.platform == "darwin":
        return [f"solidcraft-cli-{version}-macos-universal.zip"]
    if sys.platform == "win32":
        arch = {"win-amd64": "x64", "win32": "x86", "win-arm64": "arm64"}.get(sysconfig.get_platform())
        return [f"solidcraft-cli-{version}-windows-{arch}.zip", f"solidcraft-{version}-windows-{arch}-portable.zip"] if arch else []
    if sys.platform.startswith("freebsd") and machine in ("amd64", "x86_64"):
        return [f"solidcraft-cli-{version}-freebsd-x86_64.tar.gz", f"solidcraft-{version}-freebsd-x86_64.tar.gz"]
    return []


def asset_name(version: str) -> str | None:
    """This platform's CLI-only archive name (``asset_names``' first), or None."""
    names = asset_names(version)
    return names[0] if names else None


def cache_dir(version: str) -> Path:
    home = Path.home()
    if sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA") or home / "AppData" / "Local")
    elif sys.platform == "darwin":
        base = home / "Library" / "Caches"
    else:
        base = Path(os.environ.get("XDG_CACHE_HOME") or home / ".cache")
    return base / "solidcraft" / "cli" / version


def cached(version: str) -> str | None:
    """The CLI downloaded earlier for this version, if it is there."""
    if not version:
        return None
    path = cache_dir(version) / EXE
    return str(path) if path.is_file() else None


def release_url(version: str) -> str:
    override = os.environ.get("SOLIDCRAFT_DOWNLOAD_URL", "").strip()
    return override.rstrip("/") if override else f"https://github.com/{REPO}/releases/download/v{version}"


def _open(url: str):
    return urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "solidcraft-python"}), timeout=60)


def checksums(text: str) -> dict[str, str]:
    """``SHA256SUMS.txt`` (``<hex>  <name>`` per line, as ``sha256sum`` writes it) as {name: hex}."""
    sums = {}
    for line in text.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2 and len(parts[0]) == 64:
            sums[parts[1].lstrip("*")] = parts[0].lower()
    return sums


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def _extract(archive: Path, name: str, dest: Path) -> None:
    """Copies the archive's one ``solidcraft-cli`` (any folder) to ``dest``, byte for byte."""
    if name.endswith(".zip"):
        with zipfile.ZipFile(archive) as z:
            members = [m for m in z.infolist() if not m.is_dir() and m.filename.rsplit("/", 1)[-1] == EXE]
            if len(members) != 1:
                raise DownloadError(f"{name} holds no single {EXE}")
            with z.open(members[0]) as src, open(dest, "wb") as out:
                shutil.copyfileobj(src, out)
    else:
        with tarfile.open(archive, "r:gz") as t:
            members = [m for m in t.getmembers() if m.isfile() and m.name.rsplit("/", 1)[-1] == EXE]
            if len(members) != 1:
                raise DownloadError(f"{name} holds no single {EXE}")
            src = t.extractfile(members[0])
            if src is None:
                raise DownloadError(f"{name}: cannot read {members[0].name}")
            with src, open(dest, "wb") as out:
                shutil.copyfileobj(src, out)
    dest.chmod(0o755)


def install_archive(version: str, archive: Path, name: str, expected: str | None) -> str:
    """Checks ``archive`` against ``expected`` (when given) and puts its CLI in the cache."""
    if expected is not None:
        got = _sha256(archive)
        if got != expected:
            raise DownloadError(
                f"{name} does not match the release's SHA256SUMS.txt (expected {expected}, got {got}); "
                "it was deleted and not run"
            )
    folder = cache_dir(version)
    folder.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=f".{EXE}.", dir=folder)
    os.close(fd)
    try:
        _extract(archive, name, Path(tmp))
        os.replace(tmp, folder / EXE)  # atomic: a second process never sees half a binary
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise
    return str(folder / EXE)


def download(version: str) -> str:
    """Downloads, checks and caches this platform's CLI of ``version``; returns its path."""
    names = asset_names(version)
    if not names:
        raise DownloadError(f"the SolidCraft {version} release has no solidcraft-cli for this platform ({sys.platform} {platform.machine()})")
    base = release_url(version)
    try:
        with _open(f"{base}/SHA256SUMS.txt") as r:
            sums = checksums(r.read().decode("utf-8", "replace"))
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise DownloadError(f"cannot download solidcraft-cli {version}: {base}/SHA256SUMS.txt: {getattr(e, 'reason', e)}") from e
    name = next((n for n in names if n in sums), None)
    if name is None:
        raise DownloadError(f"the SolidCraft {version} release lists none of {', '.join(names)} in SHA256SUMS.txt")
    expected = sums[name]
    folder = cache_dir(version)
    folder.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=".download.", dir=folder)
    archive = Path(tmp)
    try:
        with os.fdopen(fd, "wb") as out:
            try:
                with _open(f"{base}/{name}") as r:
                    size = r.headers.get("Content-Length")
                    mb = f" ({int(size) / 1e6:.0f} MB)" if size and size.isdigit() else ""
                    print(f"solidcraft: downloading solidcraft-cli {version}{mb}, once; it is kept in {folder}", file=sys.stderr, flush=True)
                    shutil.copyfileobj(r, out, 1 << 20)
            except (urllib.error.URLError, OSError, ValueError) as e:
                raise DownloadError(f"cannot download solidcraft-cli {version}: {base}/{name}: {getattr(e, 'reason', e)}") from e
        return install_archive(version, archive, name, expected)
    finally:
        archive.unlink(missing_ok=True)
