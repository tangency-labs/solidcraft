"""Exercise the real iOS verifier using tiny synthetic IPAs, not a device SDK."""

import hashlib
import importlib.util
import os
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import warnings
import zipfile


SCRIPT = Path(__file__).resolve().parents[1] / "ios" / "verify_ipa.py"
BUILD_SCRIPT = SCRIPT.with_name("build.sh")
SPEC = importlib.util.spec_from_file_location("solidcraft_verify_ipa", SCRIPT)
VERIFIER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFIER)
APP = "Payload/SolidCraft.app/"
UIKIT = "/System/Library/Frameworks/UIKit.framework/UIKit"
METAL = "/System/Library/Frameworks/Metal.framework/Metal"


class IOSBuildScript(unittest.TestCase):
    def test_no_machine_specific_tool_paths(self):
        script = BUILD_SCRIPT.read_text()
        self.assertNotRegex(script, r"/(?:home|Users)/[^/\"'\s]+/")
        self.assertNotIn("/usr/bin/python3", script)

    def test_missing_sdk_explains_how_to_enter_shell(self):
        env = os.environ.copy()
        env.pop("SDKROOT", None)
        result = subprocess.run(["bash", str(BUILD_SCRIPT)], env=env,
                                capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("nix develop ./packaging/ios", result.stderr)

    def test_honours_caller_tools_and_sdk_environment(self):
        # Exercise the real shell script in a disposable checkout, without
        # requiring an SDK, compiling Rust, or writing into the real workspace.
        with tempfile.TemporaryDirectory(prefix="ios build ") as temp:
            root = Path(temp)
            ios = root / "packaging" / "ios"
            ios.mkdir(parents=True)
            shutil.copy(BUILD_SCRIPT, ios / "build.sh")
            (ios / "make-icons.py").write_text("print('icons')\n")
            (ios / "verify_ipa.py").write_text("print('verified')\n")
            bin_dir = root / "bin"
            bin_dir.mkdir()
            log = root / "tools.log"

            def tool(name, body):
                path = bin_dir / name
                path.write_text("#!/bin/sh\nset -eu\n" + body)
                path.chmod(0o755)

            tool("python3", f'printf "python3:%s\\n" "$*" >> "$TEST_LOG"\nexec "{sys.executable}" "$@"\n')
            tool("cargo", '''printf 'SDKROOT=%s\\nLINKER=%s\\n' "$SDKROOT" "$CARGO_TARGET_AARCH64_APPLE_IOS_LINKER" >> "$TEST_LOG"
mkdir -p "$CARGO_TARGET_DIR/aarch64-apple-ios/release"
touch "$CARGO_TARGET_DIR/aarch64-apple-ios/release/libsolidcraft_ios.a"
''')
            tool("xtool", '''printf 'xtool_SDKROOT=%s\\n' "${SDKROOT-unset}" >> "$TEST_LOG"
printf 'xtool_CC=%s\\nxtool_CXX=%s\\n' "${CC-unset}" "${CXX-unset}" >> "$TEST_LOG"
mkdir -p xtool
python3 -c 'import zipfile; zipfile.ZipFile("xtool/SolidCraft.ipa", "w").close()'
''')
            env = os.environ.copy()
            env.update({
                "PATH": f"{bin_dir}{os.pathsep}{env['PATH']}",
                "TEST_LOG": str(log),
                "SDKROOT": str(root / "custom sdk"),
                "CARGO_TARGET_AARCH64_APPLE_IOS_LINKER": str(root / "custom linker"),
                "CC": "host-only-cc",
                "CXX": "host-only-cxx",
            })
            result = subprocess.run(["bash", str(ios / "build.sh")], env=env,
                                    capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            lines = log.read_text().splitlines()
            self.assertIn(f"SDKROOT={env['SDKROOT']}", lines)
            self.assertIn(f"LINKER={env['CARGO_TARGET_AARCH64_APPLE_IOS_LINKER']}", lines)
            # SwiftPM must discover its SDK and clang, not inherit Cargo's
            # SDKROOT or mkShell's host-only C/C++ compilers.
            self.assertIn("xtool_SDKROOT=unset", lines)
            self.assertIn("xtool_CC=unset", lines)
            self.assertIn("xtool_CXX=unset", lines)
            self.assertIn(f"python3:{ios / 'make-icons.py'}", lines)
            self.assertIn(f"python3:{ios / 'verify_ipa.py'} {ios / 'xtool' / 'SolidCraft.ipa'}", lines)


def dylib(name, command=0xC):
    encoded = name.encode() + b"\0"
    size = (24 + len(encoded) + 7) // 8 * 8
    return struct.pack("<6I", command, size, 24, 0, 0, 0) + encoded.ljust(size - 24, b"\0")


def build_version(platform=2, minimum=0x100400):
    return struct.pack("<6I", 0x32, 24, platform, minimum, 0x100500, 0)


def macho(commands=None, *, magic=0xFEEDFACF, cpu=0x0100000C, kind=2):
    if commands is None:
        commands = [build_version(), dylib(UIKIT), dylib(METAL)]
    table = b"".join(commands)
    return struct.pack("<8I", magic, cpu, 0, kind, len(commands), len(table), 0, 0) + table


def plist():
    return {
        "CFBundleExecutable": "SolidCraft",
        "CFBundleIdentifier": "labs.tangency.solidcraft.ios",
        "CFBundlePackageType": "APPL",
        "CFBundleSupportedPlatforms": ["iPhoneOS"],
        "MinimumOSVersion": "16.4",
        "UIFileSharingEnabled": True,
        "LSSupportsOpeningDocumentsInPlace": True,
    }


class IOSPackage(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "SolidCraft.ipa"

    def write_ipa(self, *, info=None, binary=None, extra=None, missing=()):
        entries = {
            APP + "Info.plist": plistlib.dumps(plist() if info is None else info),
            APP + "SolidCraft": macho() if binary is None else binary,
            **{APP + name: b"Synthetic licence fixture\n" for name in VERIFIER.LICENSES},
        }
        entries.update(extra or {})
        with zipfile.ZipFile(self.path, "w", compression=zipfile.ZIP_STORED) as archive:
            for name, contents in entries.items():
                if name not in missing:
                    archive.writestr(name, contents)
        return self.path

    def assert_invalid(self, message, **fixture):
        self.write_ipa(**fixture)
        with self.assertRaisesRegex(VERIFIER.VerificationError, message):
            VERIFIER.verify_ipa(self.path)

    def test_valid_unsigned_device_package(self):
        result = VERIFIER.verify_ipa(self.write_ipa())
        self.assertEqual(result["bundle_id"], "labs.tangency.solidcraft.ios")
        self.assertEqual(result["executable"], "SolidCraft")
        self.assertEqual(result["dependencies"], [UIKIT, METAL])
        self.assertEqual(result["sha256"], hashlib.sha256(self.path.read_bytes()).hexdigest())

    def test_binary_plist_is_supported(self):
        result = VERIFIER.verify_ipa(self.write_ipa(extra={
            APP + "Info.plist": plistlib.dumps(plist(), fmt=plistlib.FMT_BINARY),
        }))
        self.assertEqual(result["bundle_id"], "labs.tangency.solidcraft.ios")

    def test_all_dylib_load_commands_and_allowed_prefixes(self):
        commands = [build_version(), dylib(UIKIT), dylib(METAL)]
        for command in VERIFIER.DYLIB_COMMANDS:
            commands += [dylib("/usr/lib/libSystem.B.dylib", command),
                         dylib("@rpath/libswiftCore.dylib", command)]
        self.assertEqual(len(VERIFIER.verify_ipa(self.write_ipa(binary=macho(commands)))["dependencies"]),
                         2 + 2 * len(VERIFIER.DYLIB_COMMANDS))

    def test_plist_identity_platform_and_file_sharing(self):
        invalid = {
            "CFBundleIdentifier": "wrong.bundle",
            "CFBundlePackageType": "FMWK",
            "CFBundleSupportedPlatforms": ["iPhoneSimulator"],
            "MinimumOSVersion": "16.5",
            "UIFileSharingEnabled": False,
            "LSSupportsOpeningDocumentsInPlace": False,
        }
        for key, value in invalid.items():
            for replacement in (value, None):
                with self.subTest(key=key, value=replacement):
                    info = plist()
                    if replacement is None:
                        del info[key]
                    else:
                        info[key] = replacement
                    self.assert_invalid(key, info=info)
        for key in ("UIFileSharingEnabled", "LSSupportsOpeningDocumentsInPlace"):
            with self.subTest(key=key, value=1):
                self.assert_invalid(key, info={**plist(), key: 1})

    def test_missing_plist_executable_or_licence(self):
        for name in ("Info.plist", "SolidCraft", *VERIFIER.LICENSES):
            with self.subTest(name=name):
                self.assert_invalid("Missing", missing=(APP + name,))
        for name in VERIFIER.LICENSES:
            with self.subTest(empty=name):
                self.assert_invalid("Empty bundled licence", extra={APP + name: b" \n"})

    def test_malformed_plist(self):
        for contents in (b"not a plist", b"<plist>", plistlib.dumps([])):
            with self.subTest(contents=contents):
                self.assert_invalid("Cannot verify|dictionary", extra={APP + "Info.plist": contents})

    def test_executable_must_be_a_basename(self):
        for name in ("", ".", "..", "../SolidCraft", "/SolidCraft", "sub/SolidCraft", "sub\\SolidCraft", 42):
            with self.subTest(name=name):
                self.assert_invalid("Invalid CFBundleExecutable", info={**plist(), "CFBundleExecutable": name})

    def test_reject_signing_material_anywhere(self):
        for name in (APP + "_CodeSignature/CodeResources", APP + "_CodeSignature",
                     APP + "embedded.mobileprovision", "other/profile.mobileprovision"):
            with self.subTest(name=name):
                self.assert_invalid("signing material", extra={name: b"signature"})
        signed = macho([build_version(), dylib(UIKIT), dylib(METAL),
                        struct.pack("<4I", 0x1D, 16, 0, 0)])
        self.assert_invalid("LC_CODE_SIGNATURE", binary=signed)

    def test_reject_non_arm64_or_non_executable_macho(self):
        for fields in ({"magic": 0xCAFEBABE}, {"magic": 0xFEEDFACE},
                       {"cpu": 0x01000007}, {"cpu": 12}, {"kind": 6}):
            with self.subTest(fields=fields):
                self.assert_invalid("arm64 Mach-O executable", binary=macho(**fields))

    def test_reject_simulator_other_platform_or_wrong_minimum(self):
        for platform, minimum in ((7, 0x100400), (1, 0x100400), (2, 0x100300), (2, 0x100500)):
            with self.subTest(platform=platform, minimum=minimum):
                self.assert_invalid("platform|minimum OS", binary=macho([
                    build_version(platform, minimum), dylib(UIKIT), dylib(METAL)]))
        self.assert_invalid("minimum OS", binary=macho([dylib(UIKIT), dylib(METAL)]))
        self.assert_invalid("Duplicate", binary=macho([
            build_version(), build_version(), dylib(UIKIT), dylib(METAL)]))
        for command in (0x24, 0x25, 0x2F, 0x30):
            with self.subTest(legacy=command):
                self.assert_invalid("legacy", binary=macho([
                    build_version(), struct.pack("<4I", command, 16, 0x100400, 0x100500),
                    dylib(UIKIT), dylib(METAL)]))

    def test_reject_mac_or_external_dependencies_in_every_load_kind(self):
        for name in ("/opt/local/lib/libbad.dylib", "/Library/Frameworks/Test.framework/Test",
                     "/System/Library/Frameworks/Metal.framework/Versions/A/Metal",
                     "/System/Library/Frameworks/AppKit.framework/AppKit",
                     "@rpath/AppKit.framework/AppKit", "/usr/lib/../../bad.dylib"):
            for command in VERIFIER.DYLIB_COMMANDS:
                with self.subTest(name=name, command=command):
                    self.assert_invalid("dependency|Dependency", binary=macho([
                        build_version(), dylib(UIKIT), dylib(METAL), dylib(name, command)]))

    def test_require_uikit_and_metal(self):
        for name in (UIKIT, METAL):
            with self.subTest(missing=name):
                self.assert_invalid("Missing .* framework", binary=macho([build_version(), dylib(name)]))
        self.assert_invalid("Missing UIKit", binary=macho([
            build_version(), dylib(UIKIT + "Extra"), dylib(METAL)]))

    def test_reject_truncated_or_inconsistent_macho(self):
        valid = macho()
        malformed = [
            b"", valid[:31], valid[:-1],
            valid[:16] + struct.pack("<I", 999) + valid[20:],
            valid[:20] + struct.pack("<I", 8) + valid[24:],
            valid[:36] + struct.pack("<I", 0) + valid[40:],
            valid[:36] + struct.pack("<I", 25) + valid[40:],
            valid[:36] + struct.pack("<I", 0xFFFFFFF8) + valid[40:],
            macho([build_version() + b"\0" * 8]),
            macho([struct.pack("<4I", 0x32, 16, 2, 0x100400)]),
            macho([build_version()[:-4] + struct.pack("<I", 1)]),
        ]
        for binary in malformed:
            with self.subTest(binary=binary):
                self.assert_invalid("Mach-O|LC_BUILD_VERSION", binary=binary)

    def test_reject_malformed_dylib_commands(self):
        valid = dylib(UIKIT)
        bad_names = [
            struct.pack("<2I", 0xC, 8),
            valid[:8] + struct.pack("<I", 8) + valid[12:],
            valid[:8] + struct.pack("<I", len(valid)) + valid[12:],
            valid[:24] + b"x" * (len(valid) - 24),
            valid[:24] + b"\xff\0" + b"\0" * (len(valid) - 26),
        ]
        for command in bad_names:
            with self.subTest(command=command):
                self.assert_invalid("dylib|Cannot verify", binary=macho([
                    build_version(), command, dylib(METAL)]))

    def test_reject_unsafe_or_duplicate_zip_paths(self):
        for name in ("../outside", "/absolute", "Payload\\outside"):
            with self.subTest(name=name):
                self.assert_invalid("Unsafe ZIP path", extra={name: b"bad"})
        self.write_ipa()
        with zipfile.ZipFile(self.path, "a") as archive:
            # Duplicate entries are intentionally malformed, not a fixture mistake.
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                archive.writestr(APP + "SolidCraft", macho())
        with self.assertRaisesRegex(VERIFIER.VerificationError, "Duplicate ZIP"):
            VERIFIER.verify_ipa(self.path)

    def test_zip_crc_and_invalid_archive(self):
        self.write_ipa()
        raw = self.path.read_bytes()
        self.path.write_bytes(raw.replace(b"Synthetic licence fixture", b"Xynthetic licence fixture", 1))
        with self.assertRaisesRegex(VERIFIER.VerificationError, "CRC|Corrupt"):
            VERIFIER.verify_ipa(self.path)
        self.path.write_bytes(b"not a ZIP")
        with self.assertRaises(VERIFIER.VerificationError):
            VERIFIER.verify_ipa(self.path)
        with self.assertRaises(VERIFIER.VerificationError):
            VERIFIER.verify_ipa(self.path.with_name("missing.ipa"))

    def test_cli_reports_static_only_and_rejects_with_python_optimization(self):
        self.write_ipa()
        result = subprocess.run([sys.executable, str(SCRIPT), str(self.path)],
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS:", result.stdout)
        self.assertIn("physical-device execution has not been tested", result.stdout)
        self.write_ipa(info={**plist(), "MinimumOSVersion": "17.0"})
        result = subprocess.run([sys.executable, "-O", str(SCRIPT), str(self.path)],
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn("FAIL:", result.stderr)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
