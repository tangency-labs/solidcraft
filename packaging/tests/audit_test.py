"""Build tiny real binaries to check release metadata, stripping and packaging."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile


PACKAGING = Path(__file__).resolve().parents[1]
CHECK = PACKAGING / "check-audit.py"
SUFFIX = ".exe" if sys.platform == "win32" else ""


class AuditMetadata(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        for tool in ("cargo", "cargo-auditable", "rust-audit-info"):
            if not shutil.which(tool):
                raise RuntimeError(f"install {tool} before running the audit packaging tests")
        cls.directory = tempfile.TemporaryDirectory(prefix="solidcraft-audit-test-")
        cls.addClassCleanup(cls.directory.cleanup)
        cls.root = Path(cls.directory.name)
        (cls.root / "src").mkdir()
        (cls.root / "dep/src").mkdir(parents=True)
        (cls.root / "other/src").mkdir(parents=True)
        (cls.root / "Cargo.toml").write_text('''[package]
name = "audit-smoke"
version = "0.1.0"
edition = "2021"
[dependencies]
audit-dependency = { path = "dep" }
[workspace]
members = ["other"]
''')
        (cls.root / "src/main.rs").write_text('fn main() { println!("{}", audit_dependency::value()); }')
        (cls.root / "dep/Cargo.toml").write_text('''[package]
name = "audit-dependency"
version = "0.2.0"
edition = "2021"
''')
        (cls.root / "dep/src/lib.rs").write_text('pub fn value() -> u32 { 42 }')
        (cls.root / "other/Cargo.toml").write_text('''[package]
name = "audit-other"
version = "0.3.0"
edition = "2021"
[dependencies]
audit-dependency = { path = "../dep" }
''')
        (cls.root / "other/src/main.rs").write_text('fn main() { println!("{}", audit_dependency::value()); }')
        cls.env = dict(os.environ)
        # Do not let a workspace target override mix fixture binaries with the app.
        cls.env["CARGO_TARGET_DIR"] = str(cls.root / "target")
        cls.command("cargo", "generate-lockfile", "--offline")
        cls.plain = cls.build(False, "plain")
        cls.auditable = cls.build(True, "auditable")

    @classmethod
    def command(cls, *args, env=None):
        result = subprocess.run(args, cwd=cls.root, env=env or cls.env,
                                text=True, capture_output=True, check=False)
        if result.returncode:
            raise AssertionError(f"{args}:\n{result.stdout}\n{result.stderr}")
        return result

    @classmethod
    def build(cls, auditable, destination, target=None):
        env = dict(cls.env, SOLIDCRAFT_AUDITABLE="1" if auditable else "0",
                   DIST=str(cls.root / "dist"))
        args = ["--release", "--locked", "--offline", "--workspace"]
        if target:
            args += ["--target", target]
        if sys.platform == "win32":
            cls.command("cargo", *(["auditable"] if auditable else []), "build", *args, env=env)
        else:
            # Exercise the exact opt-in wrapper used by the native packaging scripts.
            cls.command("bash", "-c", '. "$1"; shift; build_release "$@"',
                        "audit-build", str(PACKAGING / "env.sh"), *args, env=env)
        binary_dir = cls.root / "target"
        if target:
            binary_dir /= target
        output = cls.root / (destination + SUFFIX)
        shutil.copyfile(binary_dir / "release" / ("audit-smoke" + SUFFIX), output)
        return output

    def check(self, path, package="audit-smoke", version="0.1.0"):
        return subprocess.run([sys.executable, str(CHECK), "--package", package,
                               "--version", version, str(path)], env=self.env,
                              text=True, capture_output=True, check=False)

    def test_plain_build_is_rejected(self):
        result = self.check(self.plain)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("audit", result.stderr.lower())

    def test_auditable_build_records_the_root_and_dependency(self):
        result = self.check(self.auditable)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("2 package records", result.stdout)

    def test_wrong_package_or_version_is_rejected(self):
        for package, version in (("wrong-package", "0.1.0"), ("audit-smoke", "9.9.9")):
            with self.subTest(package=package, version=version):
                result = self.check(self.auditable, package, version)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("expected one root package", result.stderr)

    def test_workspace_binaries_each_record_their_own_root(self):
        other = self.root / "target/release" / ("audit-other" + SUFFIX)
        result = self.check(other, "audit-other", "0.3.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("2 package records", result.stdout)
        self.assertNotEqual(self.check(other).returncode, 0)

    def test_stripped_signed_and_zipped_binary_keeps_metadata(self):
        stripped = self.root / ("stripped" + SUFFIX)
        shutil.copyfile(self.auditable, stripped)
        if sys.platform == "darwin":
            self.command("strip", "-x", str(stripped))
            self.command("codesign", "--force", "--sign", "-", str(stripped))
        elif sys.platform != "win32":
            self.command("strip", str(stripped))
        archive = self.root / "package.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
            output.write(stripped, "bin/" + stripped.name)
        with zipfile.ZipFile(archive) as source:
            unpacked = Path(source.extract("bin/" + stripped.name, self.root / "unpacked"))
        result = self.check(unpacked)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_or_corrupt_binary_is_rejected(self):
        corrupt = self.root / "corrupt"
        corrupt.write_bytes(b"not an executable")
        for path in (corrupt, self.root / "missing"):
            self.assertNotEqual(self.check(path).returncode, 0)

    @unittest.skipUnless(sys.platform == "darwin", "universal binaries are a macOS package")
    def test_every_universal_slice_must_have_metadata(self):
        arm = self.build(True, "arm", "aarch64-apple-darwin")
        intel = self.build(True, "intel", "x86_64-apple-darwin")
        plain_intel = self.build(False, "plain-intel", "x86_64-apple-darwin")
        for other, success in ((intel, True), (plain_intel, False)):
            with self.subTest(auditable_intel=success):
                fat = self.root / "universal"
                self.command("lipo", "-create", str(arm), str(other), "-output", str(fat))
                result = self.check(fat)
                self.assertEqual(result.returncode == 0, success, result.stderr)


if __name__ == "__main__":
    unittest.main()
