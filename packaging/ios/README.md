# Native iOS port (experimental)

This is an initial native iPhone/iPad host for SolidCraft, not a WebView or a
website packaged as an app. The Rust CAD engine and the existing desktop egui UI
are shared with the desktop app. The host uses eframe/winit and wgpu's Metal
backend, with a small Swift entry point and UIKit document picker.

The target is **arm64 iPhoneOS 16.4 or newer**, not the iOS Simulator. The product
is `SolidCraft`, with bundle identifier `labs.tangency.solidcraft.ios`. The IPA is
deliberately **unsigned**: there is no provisioning profile, `_CodeSignature`
directory or Mach-O code-signature command. It is not directly installable as a
normally signed device app; device deployment requires a separate signing and
provisioning step appropriate to the destination device.

## Current scope and limitations

- The initial port reuses the desktop layout. Landscape orientation and an iPad
  are recommended; this is not yet an iPhone-optimised UI.
- Basic touch input, pinch recognition and two-finger pan recognition are
  enabled. Gesture usability, rendering, lifecycle and performance still require
  physical-device testing.
- Open/import uses the native UIKit document picker **as a copy**. Selected
  files are copied into the sandbox's `Documents/Imports` folder before the Rust
  engine opens them. They are not edited at the provider's original location.
- Save/export writes into the app's `Documents` folder, exposed in the Files app
  by `UIFileSharingEnabled` and `LSSupportsOpeningDocumentsInPlace`. This is not a
  native destination-picker/export-sheet implementation. Opening-in-place is a
  bundle capability, not a claim that imports modify original provider files.
- Save As/export never silently overwrites a previous destination: a collision
  gets a numbered filename. Ordinary Save updates the design's established path.
- All open designs are checkpointed synchronously when iOS resigns active, with
  their recovery locks retained for resume. Preferences also have a synchronous
  atomic copy because eframe's ordinary settings flush uses a worker thread.
- Desktop external CLI agents and slicers, desktop update/system integrations,
  and image clipboard operations are unsupported.
- **No physical-device execution has been tested.** A successful cross-build and
  static IPA validation do not demonstrate that the app launches, renders or
  imports/exports correctly on an iPhone or iPad.

## Build on Linux with Nix

On an **x86_64-linux** host with Nix and flakes enabled, run from the repository
root:

```sh
nix develop ./packaging/ios --command bash packaging/ios/build.sh
```

The iOS flake supplies Swift 6.3.3, xtool 1.24.0, Darwin cross-tools 1.1.0,
Rust 1.99.1 with the `aarch64-apple-ios` and `wasm32-unknown-unknown` targets, and
Python 3 with Pillow. It also supplies and registers the Swift cross SDK in
`target/ios-swift-sdks/` within this repository; no separate Swift SDK
registration or VM-specific setup is required. Nix may need network access on
the first run to fetch the pinned inputs. The Theos SDK directory is named
`iPhoneOS16.5.sdk`, but identifies itself internally as **iPhoneOS 16.4**;
the app's deployment target and packaged minimum are also 16.4.

The SDK is subject to Apple's Xcode and Apple SDKs Agreement, as documented in
the pinned Theos SDK source's `LICENSE.md`; it is a build input, not bundled
with the app. The shell scopes `XDG_CONFIG_HOME` to `target/ios-swift-sdks/` so
neither xtool nor SwiftPM uses or modifies your normal Swift SDK registration.
xtool's AppImage is extracted and patched for Nix rather than mounted, so
building does not require FUSE.

The icon step uses Python and Pillow to size the existing attributed SolidCraft
app mark into ignored `target/ios-icons/`; it does not introduce new artwork.
`build.sh` builds the shared Cargo workspace's `solidcraft-ios` static library,
then packages the Swift host with `xtool dev build --configuration release --ipa`.
It uses the repository's `target/` directory and two Cargo jobs by default;
set `CARGO_BUILD_JOBS` to override that limit.
The cross-linker is `ld64.lld`, with ad-hoc signing disabled. lld warns that iOS
support is experimental; this warning is not evidence of device compatibility.
The package includes `LICENSE-MIT`, `LICENSE-APACHE` and
`THIRD-PARTY-LICENSES.txt`. After xtool packaging, the build deflates the ZIP
entries without changing bundle bytes or executable permissions, then runs the
static verifier. The output is `packaging/ios/xtool/SolidCraft.ipa`.

## Static verification and regression tests

From the repository root, verify the generated IPA and run the regression
tests:

```sh
nix flake check ./packaging/ios
python3 packaging/ios/verify_ipa.py packaging/ios/xtool/SolidCraft.ipa
python3 -m unittest discover -s packaging/tests -p 'test_ios.py' -v
CARGO_TARGET_DIR=target/path-tests cargo test --locked --profile ci -p solidcraft-ios --lib
```

The flake check compiles and runs a small Linux Swift program, evaluates a
Swift package manifest, and links a small UIKit executable with a C bridge for
iPhoneOS inside Nix's build sandbox. It uses a fresh home directory and SDK registration;
neither an Apple device nor the full Rust CAD build is needed for that check.

The verifier uses only Python's standard library. It checks ZIP CRC integrity,
bundle layout and identifier, deployment/platform metadata, both Files-sharing
flags, nonempty bundled licence files, a thin arm64 Mach-O executable, iPhoneOS
`LC_BUILD_VERSION` with minimum 16.4, and the absence of signing material.
Every dylib load dependency must use `/System/Library/`, `/usr/lib/` or `@rpath/`;
macOS `/Versions/` frameworks and AppKit are rejected. UIKit and Metal must be
linked. Malformed load commands and duplicate/unsafe ZIP paths are rejected.
Success prints the package SHA-256 and explicitly states that verification is
static only. The reusable `verify_ipa(path)` function returns package metadata
and raises `VerificationError` on failure.

The unittest suite creates tiny synthetic IPA/Mach-O fixtures to test acceptance
and rejection without an Apple SDK, signing identity or physical device. These
fixtures are intentionally not runnable applications.
The host-side Rust tests exercise sandbox-only save destinations, preservation
of existing designs and bounded collision handling without linking UIKit.
`recovery_ui` also contains a suspend-flush regression test for background tabs.

Repository policy requires **`cargo xtask ci` before every commit**. Run the
packaging tests in addition to that gate, and record actual cross-build,
verification and CI outcomes separately; none is implied by these instructions.

### Reproducible checks

Run the build above, then run the static verifier and regression tests using the
commands in this section. The verifier runs automatically at the end of a
successful build; rerun it separately to inspect an existing IPA. Record the
actual outcomes of the cross-build, tests and CI independently. A successful
static check does not establish physical-device compatibility.
