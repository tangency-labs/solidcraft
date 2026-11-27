"""The ``solidcraft`` command: it runs the found solidcraft-cli with every argument and its exit code."""

from __future__ import annotations

import os
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

import solidcraft

pytestmark = pytest.mark.skipif(os.name == "nt", reason="the fake binary is a POSIX script")

# The installed (or checked-out) package the tests import, so the child runs the same one.
SRC = str(Path(solidcraft.__file__).resolve().parents[1])


def fake_cli(tmp_path: Path) -> Path:
    """A stand-in solidcraft-cli: prints its arguments and a stdin line, exits with $FAKE_EXIT."""
    p = tmp_path / "solidcraft-cli"
    p.write_text(textwrap.dedent(f"""\
        #!{sys.executable}
        import json, os, sys
        print(json.dumps(sys.argv[1:]))
        print("stdin:" + sys.stdin.readline().strip())
        print("to stderr", file=sys.stderr)
        sys.exit(int(os.environ.get("FAKE_EXIT", "0")))
    """))
    p.chmod(0o755)
    return p


def run(args: list[str], env: dict[str, str], stdin: str = "") -> subprocess.CompletedProcess:
    code = "import sys; from solidcraft._cli import main; sys.exit(main())"
    return subprocess.run([sys.executable, "-c", code, *args], input=stdin, capture_output=True, text=True, env=env, timeout=30, check=False)


def env_with(**extra: str) -> dict[str, str]:
    env = {k: v for k, v in os.environ.items() if k != "SOLIDCRAFT_CLI"}
    env["PYTHONPATH"] = SRC + os.pathsep + env.get("PYTHONPATH", "")
    env.update(extra)
    return env


def test_arguments_stdio_and_exit_code_pass_through(tmp_path):
    cli = fake_cli(tmp_path)
    args = ["run", "replay.json", "--in", "my part.sc", "--flag=x y", ""]
    r = run(args, env_with(SOLIDCRAFT_CLI=str(cli), FAKE_EXIT="3"), stdin="hello\n")
    assert r.returncode == 3, r.stderr
    lines = r.stdout.splitlines()
    assert lines[0] == '["run", "replay.json", "--in", "my part.sc", "--flag=x y", ""]'
    assert lines[1] == "stdin:hello"
    assert r.stderr.strip() == "to stderr"


def test_no_arguments_prints_the_cli_help(tmp_path):
    cli = fake_cli(tmp_path)
    r = run([], env_with(SOLIDCRAFT_CLI=str(cli)))
    assert r.returncode == 0, r.stderr
    assert r.stdout.splitlines()[0] == '["--help"]'


def test_no_binary_is_a_clear_error_not_a_traceback(tmp_path):
    env = env_with()
    env["PATH"] = str(tmp_path)  # nothing on PATH, no SOLIDCRAFT_CLI
    code = "import sys, solidcraft._mcp as m; m.os.path.isfile = lambda p: False; from solidcraft._cli import main; sys.exit(main())"
    r = subprocess.run([sys.executable, "-c", code, "--version"], capture_output=True, text=True, env=env, timeout=30, check=False)
    assert r.returncode == 1
    assert "Traceback" not in r.stderr
    assert "SOLIDCRAFT_CLI" in r.stderr and "install SolidCraft" in r.stderr


def test_a_missing_solidcraft_cli_path_is_a_clear_error(tmp_path):
    r = run(["--version"], env_with(SOLIDCRAFT_CLI=str(tmp_path / "nope")))
    assert r.returncode == 127
    assert "Traceback" not in r.stderr and "nope" in r.stderr


def test_a_bare_solidcraft_cli_name_is_found_on_path(tmp_path):
    cli = fake_cli(tmp_path)
    env = env_with(SOLIDCRAFT_CLI=cli.name)
    env["PATH"] = str(tmp_path) + os.pathsep + env.get("PATH", "")
    r = run(["--version"], env)
    assert r.returncode == 0, r.stderr
    assert r.stdout.splitlines()[0] == '["--version"]'
