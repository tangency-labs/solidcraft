#!/usr/bin/env python3
"""Statically verify the unsigned SolidCraft arm64 iPhoneOS IPA (stdlib only)."""

import argparse
import hashlib
from pathlib import Path, PurePosixPath
import plistlib
import struct
import sys
from xml.parsers.expat import ExpatError
import zipfile
import zlib


APP = "Payload/SolidCraft.app/"
BUNDLE_ID = "labs.tangency.solidcraft.ios"
MINIMUM_OS = (16 << 16) | (4 << 8)
LICENSES = ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.txt")
DYLIB_COMMANDS = (0xC, 0x80000018, 0x8000001F, 0x20, 0x80000023)


class VerificationError(ValueError):
    """An IPA does not meet the unsigned iOS package contract."""


def require(condition, message):
    if not condition:
        raise VerificationError(message)


def verify_macho(binary):
    """Return dependencies after checking a thin arm64 device executable."""
    require(len(binary) >= 32, "Truncated Mach-O header")
    magic, cpu, _subtype, kind, count, size, _flags, _reserved = struct.unpack_from("<8I", binary)
    require(magic == 0xFEEDFACF and cpu == 0x0100000C and kind == 2,
            "Not a thin arm64 Mach-O executable")
    end = 32 + size
    require(end <= len(binary) and size % 8 == 0 and count <= size // 8,
            "Invalid Mach-O load-command table")
    offset = 32
    deployment = None
    dependencies = []
    for _ in range(count):
        require(offset + 8 <= end, "Truncated Mach-O load command")
        command, length = struct.unpack_from("<II", binary, offset)
        require(length >= 8 and length % 8 == 0 and offset + length <= end,
                "Invalid Mach-O load-command length")
        require(command != 0x1D, "Executable has LC_CODE_SIGNATURE")
        if command == 0x32:  # LC_BUILD_VERSION
            require(length >= 24, "Truncated LC_BUILD_VERSION")
            platform, minimum, _sdk, tools = struct.unpack_from("<4I", binary, offset + 8)
            require(24 + tools * 8 == length, "Invalid LC_BUILD_VERSION tools")
            require(platform == 2, "Not an iPhoneOS device platform")
            require(deployment is None, "Duplicate LC_BUILD_VERSION")
            deployment = minimum
        # Modern iOS builds use LC_BUILD_VERSION, not conflicting legacy targets.
        require(command not in (0x24, 0x25, 0x2F, 0x30),
                "Unexpected legacy deployment-target load command")
        if command in DYLIB_COMMANDS:
            require(length >= 24, "Truncated dylib load command")
            name_offset = struct.unpack_from("<I", binary, offset + 8)[0]
            require(24 <= name_offset < length, "Invalid dylib name offset")
            raw_name = binary[offset + name_offset:offset + length]
            require(b"\0" in raw_name, "Unterminated dylib name")
            name = raw_name.split(b"\0", 1)[0].decode("utf-8")
            require(name.startswith(("/System/Library/", "/usr/lib/", "@rpath/")),
                    f"Non-iOS dependency: {name}")
            require(".." not in PurePosixPath(name).parts,
                    f"Dependency traverses its allowed prefix: {name}")
            require("/Versions/" not in name and "AppKit" not in name,
                    f"macOS framework dependency: {name}")
            dependencies.append(name)
        offset += length
    require(offset == end, "Mach-O load-command count/size mismatch")
    require(deployment == MINIMUM_OS, "Mach-O minimum OS must be 16.4")
    for framework in ("UIKit", "Metal"):
        require(any(name.endswith(f"/{framework}.framework/{framework}") for name in dependencies),
                f"Missing {framework} framework dependency")
    return dependencies


def verify_ipa(path):
    """Verify a package, returning its identity, dependencies and SHA-256.

    Raises VerificationError for invalid or unreadable packages. This is not an
    execution test, code-signing check for installation, or App Store validation.
    """
    path = Path(path)
    try:
        with zipfile.ZipFile(path) as ipa:
            entries = ipa.infolist()
            names = [entry.filename for entry in entries]
            require(len(names) == len(set(names)), "Duplicate ZIP entry")
            for name in names:
                parts = PurePosixPath(name).parts
                require(not name.startswith("/") and ".." not in parts and "\\" not in name,
                        f"Unsafe ZIP path: {name}")
                require("_CodeSignature" not in parts and not name.lower().endswith(".mobileprovision"),
                        f"IPA contains signing material: {name}")
            require(ipa.testzip() is None, "Corrupt ZIP entry")
            require(APP + "Info.plist" in names, "Missing SolidCraft Info.plist")
            info = plistlib.loads(ipa.read(APP + "Info.plist"))
            require(isinstance(info, dict), "Info.plist must be a dictionary")
            expected = {
                "CFBundleIdentifier": BUNDLE_ID,
                "CFBundlePackageType": "APPL",
                "CFBundleSupportedPlatforms": ["iPhoneOS"],
                "MinimumOSVersion": "16.4",
            }
            for key, value in expected.items():
                require(info.get(key) == value, f"Info.plist {key} must be {value!r}")
            for key in ("UIFileSharingEnabled", "LSSupportsOpeningDocumentsInPlace"):
                require(info.get(key) is True, f"Info.plist {key} must be true")
            executable = info.get("CFBundleExecutable")
            require(isinstance(executable, str) and executable not in ("", ".", "..")
                    and "/" not in executable and "\\" not in executable,
                    "Invalid CFBundleExecutable")
            require(APP + executable in names, "Missing bundle executable")
            for license_name in LICENSES:
                require(APP + license_name in names, f"Missing bundled licence: {license_name}")
                require(bool(ipa.read(APP + license_name).strip()),
                        f"Empty bundled licence: {license_name}")
            dependencies = verify_macho(ipa.read(APP + executable))
        digest = hashlib.sha256()
        with path.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(block)
        return {"bundle_id": info["CFBundleIdentifier"], "executable": executable,
                "dependencies": dependencies, "sha256": digest.hexdigest()}
    except VerificationError:
        raise
    except (OSError, ValueError, RuntimeError, EOFError, OverflowError,
            struct.error, zipfile.BadZipFile, ExpatError, zlib.error) as error:
        raise VerificationError(f"Cannot verify IPA: {error}") from error


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ipa", type=Path, help="unsigned SolidCraft device IPA")
    args = parser.parse_args(argv)
    try:
        result = verify_ipa(args.ipa)
    except VerificationError as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print(f"PASS: {args.ipa.name}: arm64 iPhoneOS, minimum 16.4, unsigned, valid ZIP")
    print(f"Bundle ID: {result['bundle_id']}")
    print(f"SHA256: {result['sha256']}")
    print("Static verification only; physical-device execution has not been tested.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
