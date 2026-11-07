{
  description = "SolidCraft unsigned iPhoneOS cross-build tools (x86_64 Linux)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/6510408d147d8c5e6d76043e899e9251756f6a75";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      python = pkgs.python3.withPackages (p: [ p.pillow ]);
      rust = pkgs.rust-bin.stable."1.99.1".minimal.override {
        extensions = [
          "clippy"
          "rustfmt"
        ];
        targets = [
          "aarch64-apple-ios"
          "wasm32-unknown-unknown"
        ];
      };

      # Extract and patch the AppImage rather than mounting it: no FUSE, FHS
      # namespace, or access to a preinstalled host Swift runtime is required.
      xtoolImage = pkgs.appimageTools.extract {
        pname = "xtool";
        version = "1.24.0";
        src = pkgs.fetchurl {
          url = "https://github.com/xtool-org/xtool/releases/download/1.24.0/xtool-x86_64.AppImage";
          sha256 = "17xmv99qmbm9s62x2zxzfv526z63fd0qqs0v2x6chxzh2rr9vzfv";
        };
      };
      xtool = pkgs.stdenv.mkDerivation {
        pname = "xtool";
        version = "1.24.0";
        src = xtoolImage;
        nativeBuildInputs = [ pkgs.autoPatchelfHook ];
        buildInputs = [
          pkgs.stdenv.cc.cc.lib
          pkgs.gmp
          pkgs.libxcrypt-legacy
          pkgs.zlib
          pkgs.e2fsprogs
        ];
        dontBuild = true;
        dontStrip = true;
        installPhase = ''
          runHook preInstall
          mkdir -p $out
          cp -R usr/bin usr/lib $out/
          runHook postInstall
        '';
      };

      swift = pkgs.stdenv.mkDerivation {
        pname = "swift-ios-host";
        version = "6.3.3";
        src = pkgs.fetchurl {
          url = "https://download.swift.org/swift-6.3.3-release/ubuntu2404/swift-6.3.3-RELEASE/swift-6.3.3-RELEASE-ubuntu24.04.tar.gz";
          sha256 = "06fdqrrvkzhf4bb5hz93sjnjwvjj0hpfbl4yaaqnbkfwznjp50ns";
        };
        buildInputs = with pkgs; [
          stdenv.cc.cc.lib
          curl
          libedit
          libuuid
          libxml2_13
          ncurses
          sqlite
          zlib
        ];
        # Only the compiler/package manager and their resources are needed.
        # Do not unpack debuggers, SourceKit, documentation or static Linux libs.
        unpackPhase = ''
          mkdir -p $out
          tar -xzf $src --strip-components=2 -C $out --wildcards \
            '*/usr/bin/swift' '*/usr/bin/swiftc' \
            '*/usr/bin/swift-driver' '*/usr/bin/swift-frontend' \
            '*/usr/bin/swift-help' '*/usr/bin/swift-package' \
            '*/usr/bin/swift-sdk' '*/usr/bin/swift-build-tool' \
            '*/usr/bin/swift-build' '*/usr/bin/swift-autolink-extract' \
            '*/usr/bin/clang' '*/usr/bin/clang-21' \
            '*/usr/bin/clang++' '*/usr/bin/lld' '*/usr/bin/ld.lld' \
            '*/usr/lib/swift/*' '*/usr/lib/clang/*'
        '';
        dontBuild = true;
        dontInstall = true;
        dontStrip = true;
        nativeBuildInputs = [ pkgs.autoPatchelfHook ];
      };
      # SwiftPM links Package.swift on Linux even during an iOS build. Its
      # bundled clang otherwise searches /usr/lib for crt*.o and libc. Wrap
      # that same clang with Nix's host C-runtime flags, without pulling in a
      # second LLVM toolchain or injecting Linux flags into Darwin links.
      hostClang = pkgs.wrapCCWith {
        cc =
          pkgs.runCommand "swift-clang-21"
            {
              version = "21.0.0";
              passthru = {
                isClang = true;
                langCC = true;
              };
            }
            ''
              mkdir -p $out/bin
              ln -s ${swift}/bin/clang $out/bin/clang
              ln -s ${swift}/bin/clang++ $out/bin/clang++
            '';
        isClang = true;
        gccForLibs = pkgs.stdenv.cc.cc;
      };
      swiftClang = pkgs.writeShellScript "swift-clang" ''
        target_next=false
        for arg in "$@"; do
          if $target_next; then
            case "$arg" in
              *-apple-*) exec ${swift}/bin/clang "$@" ;;
            esac
            target_next=false
          fi
          case "$arg" in
            -target|--target) target_next=true ;;
            --target=*-apple-*) exec ${swift}/bin/clang "$@" ;;
          esac
        done
        exec ${hostClang}/bin/clang "$@"
      '';
      darwinTools = pkgs.stdenv.mkDerivation {
        pname = "darwin-tools";
        version = "1.1.0";
        src = pkgs.fetchurl {
          url = "https://github.com/xtool-org/darwin-tools-linux-llvm/releases/download/v1.1.0/toolset-x86_64.tar.gz";
          sha256 = "1221hxp6a26x4i13amh41kpzhjm9845c5wzjbrz70idcaqnny5m5";
        };
        nativeBuildInputs = [ pkgs.autoPatchelfHook ];
        buildInputs = [
          pkgs.stdenv.cc.cc.lib
          pkgs.zlib
          pkgs.zstd
        ];
        sourceRoot = ".";
        dontBuild = true;
        dontStrip = true;
        installPhase = ''
          mkdir -p $out
          cp -R bin $out/
        '';
      };
      sdkSource = pkgs.fetchzip {
        url = "https://github.com/theos/sdks/archive/0222fd5413cf4b9af096f37b4621afa2688572f7.tar.gz";
        sha256 = "1sqvmqgqgfn0794bb56na9qjpq96m5nrbgsmnsappgzljhsj33d5";
      };
      apiNotes =
        module: hash:
        pkgs.fetchurl {
          url = "https://raw.githubusercontent.com/swiftlang/swift/swift-5.8-RELEASE/apinotes/${module}.apinotes";
          sha256 = hash;
        };
      dispatchNotes = apiNotes "Dispatch" "06j8azv7zha8f2ar2arhn76yfqnbn205474y1s3ibid09n5w8m5k";
      osNotes = apiNotes "os" "17rg4i49px31r1j0mci3jfdqfz3drrqjhmdh55zhgw7nvg7fyz99";
      sdk = pkgs.runCommand "solidcraft-iphoneos-16.4-sdk" { } ''
        mkdir -p $out
        cp -R ${sdkSource}/iPhoneOS16.5.sdk $out/iPhoneOS.sdk
        chmod -R u+w $out
        cp ${dispatchNotes} $out/iPhoneOS.sdk/usr/include/dispatch/Dispatch.apinotes
        cp ${osNotes} $out/iPhoneOS.sdk/usr/include/os/os.apinotes
        mkdir -p $out/resources/apinotes
        cp ${dispatchNotes} $out/resources/apinotes/Dispatch.apinotes
        cp ${osNotes} $out/resources/apinotes/os.apinotes
        ln -s ${swift}/lib/swift/shims $out/resources/shims
        ln -s ${swift}/lib/clang/21 $out/resources/clang
      '';
      # Both SwiftPM and xtool find this through XDG_CONFIG_HOME. Nothing is
      # installed in ~/.swiftpm, and different checkouts cannot collide.
      swiftSDK = pkgs.runCommand "solidcraft-darwin.artifactbundle" { } ''
        mkdir -p $out
        cat > $out/info.json <<'EOF'
        ${builtins.toJSON {
          schemaVersion = "1.0";
          artifacts.darwin = {
            type = "swiftSDK";
            version = "0.0.1";
            variants = [
              {
                path = ".";
                supportedTriples = [ "x86_64-unknown-linux-gnu" ];
              }
            ];
          };
        }}
        EOF
        cat > $out/swift-sdk.json <<'EOF'
        ${builtins.toJSON {
          schemaVersion = "4.0";
          targetTriples.arm64-apple-ios = {
            sdkRootPath = "${sdk}/iPhoneOS.sdk";
            swiftResourcesPath = "${sdk}/resources";
            toolsetPaths = [ "toolset.json" ];
            librarySearchPaths = [ "${sdk}/iPhoneOS.sdk/usr/lib/swift" ];
          };
        }}
        EOF
        cat > $out/toolset.json <<'EOF'
        ${builtins.toJSON {
          schemaVersion = "1.0";
          rootPath = "${darwinTools}/bin";
          linker.path = "ld64.lld";
          librarian.path = "libtool";
          swiftCompiler.extraCLIOptions = [
            "-use-ld=lld"
            "-runtime-compatibility-version"
            "none"
          ];
        }}
        EOF
        echo 'epoch=4,darwinTools=1.1.0,oam=1.4.1' > $out/darwin-sdk-version.txt
      '';
    in
    {
      packages.${system} = {
        inherit
          darwinTools
          rust
          sdk
          swift
          xtool
          ;
        default = xtool;
      };
      # A sandboxed smoke build also catches accidental reliance on /usr/lib,
      # a host Swift installation, or a previously registered Darwin SDK.
      checks.${system}.toolchains =
        pkgs.runCommand "solidcraft-ios-toolchain-check"
          {
            nativeBuildInputs = [
              swift
              xtool
              pkgs.stdenv.cc
            ];
            SWIFT_DRIVER_CLANG_EXEC = "${swiftClang}";
          }
          ''
            export HOME="$TMPDIR/home"
            export XDG_CONFIG_HOME="$HOME/config"
            mkdir -p "$XDG_CONFIG_HOME/swiftpm/swift-sdks" Sources/Probe Sources/Bridge/include
            ln -s ${swiftSDK} "$XDG_CONFIG_HOME/swiftpm/swift-sdks/darwin.artifactbundle"
            swift --version
            xtool --version
            swift sdk list | grep -Fx darwin
            echo 'print("host link works")' > hello.swift
            swiftc hello.swift -o hello
            ./hello | grep 'host link works'
            cat > Package.swift <<'EOF'
            // swift-tools-version: 6.0
            import PackageDescription
            let package = Package(
                name: "Probe",
                platforms: [.iOS("16.4")],
                targets: [
                    .target(name: "Bridge"),
                    .executableTarget(
                        name: "Probe",
                        dependencies: ["Bridge"],
                        linkerSettings: [.unsafeFlags(["-Xlinker", "-no_adhoc_codesign"])]
                    ),
                ],
                swiftLanguageModes: [.v5]
            )
            EOF
            echo 'int bridge_value(void);' > Sources/Bridge/include/Bridge.h
            echo 'int bridge_value(void) { return 7; }' > Sources/Bridge/Bridge.c
            echo 'import UIKit; import Bridge; print(UIScreen.main.bounds, bridge_value())' > Sources/Probe/main.swift
            env -u CC -u CXX swift build --swift-sdk arm64-apple-ios --configuration release
            touch $out
          '';
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          rust
          swift
          xtool
          darwinTools
          python
          pkgs.pkg-config
          pkgs.git
          pkgs.shellcheck
        ];
        SDKROOT = "${sdk}/iPhoneOS.sdk";
        IPHONEOS_DEPLOYMENT_TARGET = "16.4";
        CC_aarch64_apple_ios = "${swift}/bin/clang";
        CARGO_TARGET_AARCH64_APPLE_IOS_LINKER = "${darwinTools}/bin/ld64.lld";
        SWIFT_DRIVER_CLANG_EXEC = "${swiftClang}";
        shellHook = ''
          root=$(git rev-parse --show-toplevel) || exit 1
          export XDG_CONFIG_HOME="$root/target/ios-swift-sdks"
          mkdir -p "$XDG_CONFIG_HOME/swiftpm/swift-sdks"
          ln -sfnT ${swiftSDK} "$XDG_CONFIG_HOME/swiftpm/swift-sdks/darwin.artifactbundle" || exit 1
          echo "SolidCraft iOS: bash \"$root/packaging/ios/build.sh\""
        '';
      };
    };
}
