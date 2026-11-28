"""The CLI download: a fake release folder (file://) with a fake SHA256SUMS.txt, then a match, a
mismatch, an offline server, the cache, a PATH CLI that is too old, and `install-cli`."""

from __future__ import annotations

import hashlib
import io
import os
import socket
import subprocess
import sys
import tarfile
import textwrap
import zipfile
from pathlib import Path

import pytest

from solidcraft import _download

from test_cli import env_with, run

pytestmark = [
    pytest.mark.skipif(os.name == "nt", reason="the fake binary is a POSIX script"),
    pytest.mark.skipif(not _download.package_version(), reason="no generated bindings, so no version to download"),
]

VERSION = _download.package_version()


def fake_binary(label: str) -> bytes:
    return textwrap.dedent(f"""\
        #!{sys.executable}
        import json, sys
        print(json.dumps(["{label}", *sys.argv[1:]]))
    """).encode()


def release(tmp_path: Path, body: bytes, sums_override: str | None = None, name: str | None = None) -> Path:
    """A release folder holding this platform's CLI archive (or ``name``) and SHA256SUMS.txt."""
    folder = tmp_path / "release"
    folder.mkdir()
    name = name or _download.asset_name(VERSION)
    assert name, "no CLI archive for the platform the tests run on"
    top = name.rsplit(".", 2 if name.endswith(".tar.gz") else 1)[0]
    if name.endswith(".zip"):
        with zipfile.ZipFile(folder / name, "w") as z:
            z.writestr(f"{top}/README.md", "readme")
            z.writestr(f"{top}/{_download.EXE}", body)
    else:
        with tarfile.open(folder / name, "w:gz") as t:
            for member, data in ((f"{top}/README.md", b"readme"), (f"{top}/{_download.EXE}", body)):
                info = tarfile.TarInfo(member)
                info.size, info.mode = len(data), 0o755
                t.addfile(info, io.BytesIO(data))
    digest = hashlib.sha256((folder / name).read_bytes()).hexdigest()
    (folder / "SHA256SUMS.txt").write_text(sums_override or f"{'0' * 64}  other-file.zip\n{digest}  {name}\n")
    return folder


def env_for(tmp_path: Path, url: str, **extra: str) -> dict[str, str]:
    env = env_with(HOME=str(tmp_path / "home"), XDG_CACHE_HOME=str(tmp_path / "cache"), SOLIDCRAFT_DOWNLOAD_URL=url, **extra)
    env["PATH"] = str(tmp_path / "empty-bin") + os.pathsep + os.path.dirname(sys.executable)
    (tmp_path / "home").mkdir(exist_ok=True)
    return env


def cache_file(tmp_path: Path) -> Path:
    base = tmp_path / "home/Library/Caches" if sys.platform == "darwin" else tmp_path / "cache"
    return base / "solidcraft" / "cli" / VERSION / _download.EXE


def test_a_matching_download_runs_and_is_cached(tmp_path):
    folder = release(tmp_path, fake_binary("downloaded"))
    env = env_for(tmp_path, folder.as_uri())
    r = run(["--version"], env)
    assert r.returncode == 0, r.stderr
    assert r.stdout.splitlines()[0] == '["downloaded", "--version"]'
    assert f"downloading solidcraft-cli {VERSION}" in r.stderr
    assert cache_file(tmp_path).is_file()
    # Next time from the cache: no download line, and the release folder is not needed.
    for f in folder.iterdir():
        f.unlink()
    r = run(["mcp"], env)
    assert r.returncode == 0, r.stderr
    assert r.stdout.splitlines()[0] == '["downloaded", "mcp"]'
    assert "downloading" not in r.stderr


def test_a_release_with_only_the_whole_package_is_used(tmp_path):
    # Releases before 0.52 have no CLI-only archive: the package holding the CLI is taken instead.
    names = _download.asset_names(VERSION)
    if len(names) < 2:
        pytest.skip("this platform has only one archive name")
    folder = release(tmp_path, fake_binary("from-package"), name=names[1])
    r = run(["--version"], env_for(tmp_path, folder.as_uri()))
    assert r.returncode == 0, r.stderr
    assert r.stdout.splitlines()[0] == '["from-package", "--version"]'


def test_a_mismatched_download_is_refused_and_never_run(tmp_path):
    folder = release(tmp_path, fake_binary("tampered"), sums_override=f"{'ab' * 32}  {_download.asset_name(VERSION)}\n")
    r = run(["--version"], env_for(tmp_path, folder.as_uri()))
    assert r.returncode == 1
    assert "tampered" not in r.stdout
    assert "does not match the release's SHA256SUMS.txt" in r.stderr and "Traceback" not in r.stderr
    assert not cache_file(tmp_path).exists()
    assert not any(p.name.startswith(".") for p in cache_file(tmp_path).parent.glob("*")), "a partial download was left behind"


def test_offline_is_a_clear_error(tmp_path):
    with socket.socket() as s:  # a port nobody listens on
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    r = run(["--version"], env_for(tmp_path, f"http://127.0.0.1:{port}/v{VERSION}"))
    assert r.returncode == 1
    assert f"cannot download solidcraft-cli {VERSION}" in r.stderr
    assert "SOLIDCRAFT_CLI" in r.stderr and "Traceback" not in r.stderr


def test_no_download_turns_it_off(tmp_path):
    folder = release(tmp_path, fake_binary("downloaded"))
    r = run(["--version"], env_for(tmp_path, folder.as_uri(), SOLIDCRAFT_NO_DOWNLOAD="1"))
    assert r.returncode == 1
    assert "SOLIDCRAFT_NO_DOWNLOAD" in r.stderr and "downloading" not in r.stderr


def path_cli(tmp_path: Path, version: str) -> None:
    bin_dir = tmp_path / "empty-bin"
    bin_dir.mkdir(exist_ok=True)
    p = bin_dir / "solidcraft-cli"
    p.write_text(f'#!{sys.executable}\nimport sys\nprint("solidcraft-cli {version}" if sys.argv[1:] == ["--version"] else "path-cli")\n')
    p.chmod(0o755)


def test_a_path_cli_of_this_minor_version_or_newer_is_used(tmp_path):
    major, minor = VERSION.split(".")[:2]
    path_cli(tmp_path, f"{major}.{int(minor) + 1}.0")
    r = run(["x"], env_for(tmp_path, (tmp_path / "nowhere").as_uri()))
    assert r.returncode == 0, r.stderr
    assert r.stdout.strip() == "path-cli"


def test_an_older_path_cli_is_passed_over_for_the_download(tmp_path):
    major, minor = VERSION.split(".")[:2]
    path_cli(tmp_path, f"{major}.{int(minor) - 1}.9" if int(minor) else "0.0.1")
    folder = release(tmp_path, fake_binary("downloaded"))
    r = run(["x"], env_for(tmp_path, folder.as_uri()))
    assert r.returncode == 0, r.stderr
    assert r.stdout.splitlines()[0] == '["downloaded", "x"]'


def test_install_cli_copies_it_to_the_user_bin_folder(tmp_path):
    folder = release(tmp_path, fake_binary("downloaded"))
    env = env_for(tmp_path, folder.as_uri(), XDG_BIN_HOME=str(tmp_path / "bin"))
    r = run(["install-cli"], env)
    assert r.returncode == 0, r.stderr
    dest = tmp_path / "bin" / _download.EXE
    assert dest.read_bytes() == fake_binary("downloaded")
    assert "is not on PATH" in r.stdout and f'export PATH="{tmp_path / "bin"}:$PATH"' in r.stdout
    # Another file there is never replaced without --force.
    dest.write_text("#!/bin/sh\necho mine\n")
    r = run(["install-cli"], env)
    assert r.returncode == 1 and "--force" in r.stderr
    assert dest.read_text() == "#!/bin/sh\necho mine\n"


def test_install_cli_from_a_file_seeds_the_cache_offline(tmp_path):
    folder = release(tmp_path, fake_binary("seeded"))
    archive = folder / _download.asset_name(VERSION)
    env = env_for(tmp_path, (tmp_path / "nowhere").as_uri(), XDG_BIN_HOME=str(tmp_path / "bin"), SOLIDCRAFT_NO_DOWNLOAD="1")
    r = run(["install-cli", "--from", str(archive)], env)
    assert r.returncode == 0, r.stderr
    assert cache_file(tmp_path).read_bytes() == fake_binary("seeded")
    r = run(["--version"], env)
    assert r.returncode == 0, r.stderr
    assert r.stdout.splitlines()[0] == '["seeded", "--version"]'
    # A SHA256SUMS.txt beside the archive that disagrees refuses it.
    (folder / "SHA256SUMS.txt").write_text(f"{'cd' * 32}  {archive.name}\n")
    r = run(["install-cli", "--from", str(archive), "--force"], env)
    assert r.returncode == 1 and "does not match" in r.stderr
