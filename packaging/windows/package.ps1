<#
.SYNOPSIS
  Build and package SolidCraft for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    solidcraft-<version>-windows-<arch>.msi            per-machine installer (WiX v5)
    solidcraft-<version>-windows-<arch>-portable.zip   solidcraft.exe + solidcraft-cli.exe + licences
                                                       (+ the solvecraft-cli.exe compatibility alias)
    SolidCraft-<version>-windows-<arch>.exe            the app alone (x64 only: a plain download)

  The binaries link the C runtime statically (+crt-static), so nothing else needs installing.
  The icon comes from $env:SOLIDCRAFT_ICONS (solidcraft.ico, made by `solidcraft-cli icon`), or
  is drawn by the CLI just built when this machine can run it.

  Signing happens outside this script (the release workflow sends files to SignPath), so the work
  can be split in two phases with signing in between:
    -Phase stage     build, check, and copy the three .exe files to the staging folder, then stop
    -Phase package   make the MSI, portable zip and plain .exe from the staging folder as it is
                     now (holding the signed .exe files), without building again
  -Phase all (the default) does both, unsigned, as before.

  Needs: Rust (MSVC toolchain + the target) and WiX v5:
    dotnet tool install --global wix --version 5.0.2
    wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2

  Adapted from PhotoCraft's packaging (storytold/photocraft, Apache-2.0; see ATTRIBUTION.md).

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch arm64     # cross-compiled; needs the MSVC ARM64 build tools
  pwsh packaging/windows/package.ps1 -Arch x64 -Phase stage     # then sign target/windows-package/x64/*.exe
  pwsh packaging/windows/package.ps1 -Arch x64 -Phase package
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild,
  [ValidateSet('all', 'stage', 'package')] [string] $Phase = 'all'
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:SOLIDCRAFT_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI ProductVersion is numeric (major.minor.build); pre-release tags are dropped there.
$MsiVersion = ($Version -split '-')[0]

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'x86' { 'i686-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

Write-Output "SolidCraft $Version for Windows $Arch ($Target)"

if (-not $SkipBuild -and $Phase -ne 'package') {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p solidcraft -p solidcraft-cli --target $Target }
}
$Bin = Join-Path $TargetDir "$Target\release"
$Stage = Join-Path $TargetDir "windows-package\$Arch"
# -Phase package checks the staged (by then signed) copies; the other phases check the build.
$CheckDir = if ($Phase -eq 'package') { $Stage } else { $Bin }

# Check both binaries' PE headers: Machine must match -Arch; Subsystem 2 = GUI (the app opens no
# console window), 3 = console (the CLI's output reaches the terminal).
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
# solvecraft-cli.exe: the old CLI name, forwarding to solidcraft-cli.exe (compatibility shim,
# docs/compatibility.md). Built by `-p solidcraft-cli` with the CLI.
foreach ($check in @(@('solidcraft.exe', 2), @('solidcraft-cli.exe', 3), @('solvecraft-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $CheckDir $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}
& (Join-Path $PSScriptRoot 'check-icon.ps1') -Path (Join-Path $CheckDir 'solidcraft.exe')
if ($Phase -ne 'package') {
  Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force -Path $Stage | Out-Null
  Copy-Item (Join-Path $Bin 'solidcraft.exe'), (Join-Path $Bin 'solidcraft-cli.exe'), (Join-Path $Bin 'solvecraft-cli.exe') $Stage
  foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD-PARTY-LICENSES.txt') { Copy-Item (Join-Path $Root $f) $Stage }
}
if ($Phase -eq 'stage') {
  Write-Output "staged for signing: $Stage"
  exit 0
}

$Icon = if ($env:SOLIDCRAFT_ICONS) { Join-Path $env:SOLIDCRAFT_ICONS 'solidcraft.ico' } else { Join-Path $Root 'assets/icons/app/windows/exports/solidcraft.ico' }
if (-not (Test-Path $Icon)) { throw "Missing package icon: $Icon" }

# ---- MSI ---------------------------------------------------------------------------------------
$Msi = Join-Path $Dist "solidcraft-$Version-windows-$Arch.msi"
Invoke-Native 'wix build' {
  wix build (Join-Path $Root 'assets/platforms/windows/solidcraft.wxs') -arch $Arch `
    -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
    -culture en-US -loc (Join-Path $Root 'assets/platforms/windows/solidcraft.en-us.wxl') `
    -d "Version=$MsiVersion" -d "DisplayVersion=$Version" -d "BinDir=$Stage" -d "IconPath=$Icon" `
    -o $Msi
}
Invoke-Native 'MSI shortcut icon validation (ICE50)' {
  wix msi validate $Msi -ice ICE50 -intermediateFolder (Join-Path $Stage 'msi-validation')
}
# wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release assets.
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))

# ---- portable zip ------------------------------------------------------------------------------
$Portable = Join-Path $TargetDir "windows-package\solidcraft-$Version-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
foreach ($f in 'README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD-PARTY-LICENSES.txt') { Copy-Item (Join-Path $Root $f) $Portable }
$Zip = Join-Path $Dist "solidcraft-$Version-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

# ---- the app as a plain download (x64) ---------------------------------------------------------
$Outputs = @($Msi, $Zip)
if ($Arch -eq 'x64') {
  $Exe = Join-Path $Dist "SolidCraft-$Version-windows-$Arch.exe"
  Copy-Item (Join-Path $Stage 'solidcraft.exe') $Exe
  $Outputs += $Exe
}

if ($CanRun) {
  Invoke-Native 'solidcraft-cli --version' { & (Join-Path $Stage 'solidcraft-cli.exe') --version }
} else {
  Write-Output "skipping solidcraft-cli --version: an $Arch build doesn't run on this $HostArch machine"
}
Get-Item $Outputs | Format-Table Name, Length
