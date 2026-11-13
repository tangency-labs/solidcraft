<#
.SYNOPSIS
    SolidCraft Update & Build Script for Windows Environments
.DESCRIPTION
    Fetches, tests, builds, and deploys SolidCraft binaries. Native binaries
    are deployed to the repository root; timestamped backups remain under
    target\<profile>\backup\. The WebAssembly
    target is deployed to the configured web root.
#>
[CmdletBinding()]
param (
    [Alias('g')][bool]$BuildGui = $true,
    [Alias('c')][bool]$BuildCli = $true,
    [Alias('w')][switch]$BuildWeb = $false,
    [Alias('f', 'force')][switch]$ForceBuild = $false,
    [Alias('s')][switch]$SkipTests = $false,
    [string]$Branch,
    [Alias('j')][int]$Jobs = 0,
    [Alias('d')][switch]$DebugMode = $false,
    [Alias('r')][switch]$ReleaseMode = $false,
    [Alias('h', '?')][switch]$Help = $false
)

$ErrorActionPreference = 'Stop'
$BuildWorktree = $null
$PreviousCargoTargetDir = $env:CARGO_TARGET_DIR

function Show-Usage {
    @'
SolidCraft Windows Build & Update Script

Usage: .\update-solidcraft.ps1 [options]

Options:
    -f, -ForceBuild          Force a clean rebuild even if Git commits match.
    -s, -SkipTests           Skip workspace tests before building.
    -Branch <NAME>           Build a remote branch (default: current branch upstream).
    -j, -Jobs <N>            Set number of parallel compilation jobs (default: all cores).
    -d, -DebugMode           Build using debug profile.
    -r, -ReleaseMode         Build using release profile (default).
    -w, -BuildWeb            Build and deploy WebAssembly target.
    -g, -BuildGui <bool>     Enable/disable GUI build (default: true).
    -c, -BuildCli <bool>     Enable/disable CLI build (default: true).
    -h, -?, -Help            Display this help message.

Native binaries are deployed to the repository root; timestamped backups are stored under target\<profile>\backup\.
'@ | Write-Host
}

if ($Help) { Show-Usage; exit 0 }
if ($Jobs -lt 0) { throw '-Jobs must be zero (all cores) or a positive integer.' }
if ($DebugMode -and $ReleaseMode) { throw 'Choose either -DebugMode or -ReleaseMode, not both.' }

$Profile = 'release'
$CargoProfileFlag = @('--release')
if ($DebugMode) { $Profile = 'debug'; $CargoProfileFlag = @() }

# Scheduled tasks may start in an unrelated working directory.
Set-Location -LiteralPath $PSScriptRoot
$LogDir = Join-Path $PSScriptRoot 'build_logs'
New-Item -ItemType Directory -Path $LogDir -Force | Out-Null
$Timestamp = Get-Date -Format 'yyyyMMdd_HHmmss_fff'
$LogFile = Join-Path $LogDir "update_$Timestamp.log"
Start-Transcript -Path $LogFile -Append | Out-Null

function Invoke-Native {
    param([Parameter(Mandatory)][string]$Command, [string[]]$Arguments = @())
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code $LASTEXITCODE`: $Command $($Arguments -join ' ')"
    }
}

function Get-NativeOutput {
    param([Parameter(Mandatory)][string]$Command, [string[]]$Arguments = @())
    $output = & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code $LASTEXITCODE`: $Command $($Arguments -join ' ')"
    }
    return (($output | Out-String).Trim())
}

# Cargo/Git paths are relative to the repository root, not the scripts folder.
$RepoRoot = Get-NativeOutput -Command 'git' -Arguments @('-C', $PSScriptRoot, 'rev-parse', '--show-toplevel')
Set-Location -LiteralPath $RepoRoot

function Deploy-Binary {
    param(
        [Parameter(Mandatory)][string]$Source,
        [Parameter(Mandatory)][string]$Destination,
        [Parameter(Mandatory)][string]$Label,
        [Parameter(Mandatory)][string]$BackupDirectory
    )
    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) {
        throw "$Label build succeeded but expected artifact is missing: $Source"
    }
    New-Item -ItemType Directory -Path $BackupDirectory -Force | Out-Null
    $staged = "$Destination.new.$PID"
    $backup = Join-Path $BackupDirectory ((Split-Path -Leaf $Destination) + ".$((Get-Date).ToString('yyyyMMdd_HHmmss_fff')).$PID")
    $movedOld = $false
    try {
        Copy-Item -LiteralPath $Source -Destination $staged -Force
        if (Test-Path -LiteralPath $Destination -PathType Leaf) {
            Move-Item -LiteralPath $Destination -Destination $backup
            $movedOld = $true
            Write-Host "$Label`: previous binary backed up to $backup"
        }
        Move-Item -LiteralPath $staged -Destination $Destination
        Write-Host "$Label deployment successful: $Destination"
    }
    catch {
        if ($movedOld -and (Test-Path -LiteralPath $backup -PathType Leaf)) {
            if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Force }
            Move-Item -LiteralPath $backup -Destination $Destination -Force
            Write-Warning "$Label deployment failed; previous binary restored."
        }
        throw
    }
    finally {
        if (Test-Path -LiteralPath $staged) { Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue }
    }
}

function Clear-ProfileBuildArtifactsPreservingBackups {
    param([Parameter(Mandatory)][string]$TargetDirectory, [Parameter(Mandatory)][string]$BackupDirectory)

    # Do not ask Cargo to remove target/<profile> itself. Keep target/, the
    # selected profile directory, and backup/ in place; delete only generated
    # children. This avoids the directory-rename/removal failure encountered
    # when another process has target or target/<profile> open.
    New-Item -ItemType Directory -Path $TargetDirectory -Force | Out-Null
    New-Item -ItemType Directory -Path $BackupDirectory -Force | Out-Null
    Get-ChildItem -LiteralPath $TargetDirectory -Force |
        Where-Object { $_.Name -ne 'backup' } |
        Remove-Item -Recurse -Force -ErrorAction Stop
    Write-Host "Cleaned generated contents of $TargetDirectory; preserved $BackupDirectory."
}

try {
    Write-Host '=================================================='
    Write-Host "Starting SolidCraft update: $(Get-Date)"
    Write-Host "Selected build profile: $Profile"
    Write-Host "Log file: $LogFile"

    [void](Get-NativeOutput -Command 'git' -Arguments @('rev-parse', '--show-toplevel'))

    # Default to the current branch upstream. -Branch overrides this with a
    # remote-tracking branch such as main or origin/main. Never switch, reset,
    # or stash the operator's original checkout.
    if ($Branch) {
        if ($Branch.StartsWith('refs/remotes/')) {
            $SourceRef = $Branch
        }
        elseif ($Branch.Contains('/')) {
            $BranchRemote = $Branch.Split('/')[0]
            $RemoteNamesText = Get-NativeOutput -Command 'git' -Arguments @('remote')
            $RemoteNames = @($RemoteNamesText -split '\r?\n' | Where-Object { $_ })
            if ($RemoteNames -contains $BranchRemote) {
                $SourceRef = "refs/remotes/$Branch"
            }
            else {
                $SourceRef = "refs/remotes/origin/$Branch"
            }
        }
        else {
            $SourceRef = "refs/remotes/origin/$Branch"
        }
    }
    else {
        $CurrentBranch = Get-NativeOutput -Command 'git' -Arguments @('symbolic-ref', '--quiet', '--short', 'HEAD')
        $Upstream = Get-NativeOutput -Command 'git' -Arguments @('rev-parse', '--abbrev-ref', '--symbolic-full-name', '@{u}')
        $SourceRef = "refs/remotes/$Upstream"
    }

    Write-Host "[STEP 1] Fetching updates for $SourceRef..."
    Invoke-Native -Command 'git' -Arguments @('-c', 'maintenance.auto=false', '-c', 'fetch.writeCommitGraph=false', 'fetch', '--all', '--prune')
    $SourceCommit = Get-NativeOutput -Command 'git' -Arguments @('rev-parse', "$SourceRef^{commit}")
    $SourceLabel = $SourceRef -replace '^refs/remotes/', ''
    $Sha256 = [System.Security.Cryptography.SHA256]::Create()
    try { $HashBytes = $Sha256.ComputeHash([System.Text.Encoding]::UTF8.GetBytes("$SourceRef|$Profile")) } finally { $Sha256.Dispose() }
    $StateKey = [BitConverter]::ToString($HashBytes).Replace('-', '').ToLowerInvariant()
    $TargetRoot = Join-Path $RepoRoot 'target'
    $StateDir = Join-Path $TargetRoot '.solidcraft-build-state'
    New-Item -ItemType Directory -Path $StateDir -Force | Out-Null
    $StateFile = Join-Path $StateDir $StateKey
    $WebInstallState = if ($env:WEB_INSTALL_DIR) { $env:WEB_INSTALL_DIR } else { 'C:\inetpub\wwwroot\solidcraft' }
    $BuildState = "$SourceRef|$SourceCommit|$Profile|$BuildGui|$BuildCli|$BuildWeb|$SkipTests|$WebInstallState"
    $LastBuildState = ''
    if (Test-Path -LiteralPath $StateFile -PathType Leaf) {
        $LastBuildState = (Get-Content -LiteralPath $StateFile -Raw).Trim()
    }

    if (($LastBuildState -eq $BuildState) -and -not $ForceBuild) {
        Write-Host "$SourceLabel ($SourceCommit) is already built with this configuration. No build needed."
        Write-Host 'Use -ForceBuild (-f) to force a clean rebuild.'
        return
    }
    if ($ForceBuild) {
        Write-Host "Force build requested: a clean rebuild of $SourceLabel will be performed."
    }
    else {
        Write-Host "New $SourceLabel revision/configuration detected: $SourceCommit"
    }

    $BuildWorktree = Join-Path ([System.IO.Path]::GetTempPath()) "solidcraft-build-$PID"
    if (Test-Path -LiteralPath $BuildWorktree) { throw "Temporary build worktree path already exists: $BuildWorktree" }
    Write-Host "[STEP 2] Preparing detached build worktree at $SourceLabel..."
    Invoke-Native -Command 'git' -Arguments @('worktree', 'add', '--detach', $BuildWorktree, $SourceCommit)
    $env:CARGO_TARGET_DIR = Join-Path $RepoRoot 'target'
    Set-Location -LiteralPath $BuildWorktree

    $TargetDir = Join-Path $RepoRoot (Join-Path 'target' $Profile)
    $BackupDir = Join-Path $TargetDir 'backup'
    if ($ForceBuild) {
        # Protect WEB_INSTALL_DIR from being wiped out by target profile cleanup
        $WebInstallDir = if ($env:WEB_INSTALL_DIR) { $env:WEB_INSTALL_DIR } else { 'C:\inetpub\wwwroot\solidcraft' }
        $TargetFull = [System.IO.Path]::GetFullPath($TargetDir).TrimEnd([System.IO.Path]::DirectorySeparatorChar)
        $WebFull = [System.IO.Path]::GetFullPath($WebInstallDir).TrimEnd([System.IO.Path]::DirectorySeparatorChar)
        if ($WebFull -eq $TargetFull -or $WebFull.StartsWith($TargetFull + [System.IO.Path]::DirectorySeparatorChar)) {
            throw "WEB_INSTALL_DIR ($WebInstallDir) overlaps with build target directory ($TargetDir)."
        }

        Write-Host '[STEP 2A] Cleaning Cargo artifacts while preserving deployed-binary backups...'
        Clear-ProfileBuildArtifactsPreservingBackups -TargetDirectory $TargetDir -BackupDirectory $BackupDir
        if ($BuildWeb) {
            $WasmTargetDir = Join-Path $RepoRoot (Join-Path 'target' (Join-Path 'wasm32-unknown-unknown' $Profile))
            if (Test-Path -LiteralPath $WasmTargetDir) {
                Get-ChildItem -LiteralPath $WasmTargetDir -Force | Remove-Item -Recurse -Force -ErrorAction Stop
                Write-Host "Cleaned generated contents of WebAssembly target directory: $WasmTargetDir"
            }
        }
    }

    $JobsArg = @()
    if ($Jobs -gt 0) { $JobsArg = @('--jobs', "$Jobs") }

    $targetFailed = $false

    if ($SkipTests) {
        Write-Host '[STEP 3] Skipping workspace tests (-SkipTests active).'
    }
    else {
        Write-Host "[STEP 3] Running workspace tests ($Profile mode)..."
        try {
            Invoke-Native -Command 'cargo' -Arguments (@('test', '--workspace') + $CargoProfileFlag + $JobsArg)

            if ($LASTEXITCODE -ne 0) {
                Write-Host "ERROR: Workspace tests failed with exit code $LASTEXITCODE! Recording error and continuing." -ForegroundColor Red
                $targetFailed = $true
            }
        }
        catch {
            Write-Host "ERROR: Workspace tests failed: $_" -ForegroundColor Red
            $targetFailed = $true
        }
    }

    New-Item -ItemType Directory -Path $BackupDir -Force | Out-Null

    if ($BuildGui) {
        Write-Host "[STEP 4] Building native GUI binary ($Profile)..."
        try {
            Invoke-Native -Command 'cargo' -Arguments (@('build') + $CargoProfileFlag + @('--bin', 'solidcraft') + $JobsArg)
            Deploy-Binary -Source (Join-Path $TargetDir 'solidcraft.exe') -Destination (Join-Path $RepoRoot 'solidcraft.exe') -Label 'GUI' -BackupDirectory $BackupDir
        }
        catch {
            Write-Host "ERROR: GUI build or deployment failed: $_" -ForegroundColor Red
            $targetFailed = $true
        }
    }

    if ($BuildCli) {
        Write-Host "[STEP 5] Building CLI binary ($Profile)..."
        try {
            Invoke-Native -Command 'cargo' -Arguments (@('build') + $CargoProfileFlag + @('--bin', 'solidcraft-cli') + $JobsArg)
            Deploy-Binary -Source (Join-Path $TargetDir 'solidcraft-cli.exe') -Destination (Join-Path $RepoRoot 'solidcraft-cli.exe') -Label 'CLI' -BackupDirectory $BackupDir
        }
        catch {
            Write-Host "ERROR: CLI build or deployment failed: $_" -ForegroundColor Red
            $targetFailed = $true
        }
    }

    if ($BuildWeb) {
        Write-Host "[STEP 6] Building WebAssembly target ($Profile)..."
        try {
            Invoke-Native -Command 'cargo' -Arguments (@('build') + $CargoProfileFlag + @('--target', 'wasm32-unknown-unknown', '--bin', 'solidcraft-web') + $JobsArg)
            $SourceWeb = Join-Path $RepoRoot (Join-Path 'target' (Join-Path 'wasm32-unknown-unknown' (Join-Path $Profile 'solidcraft-web.wasm')))
            if (-not (Test-Path -LiteralPath $SourceWeb -PathType Leaf)) { throw "WebAssembly build succeeded but expected artifact is missing: $SourceWeb" }
            $WebInstallDir = if ($env:WEB_INSTALL_DIR) { $env:WEB_INSTALL_DIR } else { 'C:\inetpub\wwwroot\solidcraft' }
            New-Item -ItemType Directory -Path $WebInstallDir -Force | Out-Null
            $WebStaged = Join-Path $WebInstallDir "solidcraft.wasm.new.$PID"
            try {
                Copy-Item -LiteralPath $SourceWeb -Destination $WebStaged -Force
                Move-Item -LiteralPath $WebStaged -Destination (Join-Path $WebInstallDir 'solidcraft.wasm') -Force
            }
            finally {
                if (Test-Path -LiteralPath $WebStaged) { Remove-Item -LiteralPath $WebStaged -Force -ErrorAction SilentlyContinue }
            }
            Write-Host "Web deployment successful to $WebInstallDir."
        }
        catch {
            Write-Host "ERROR: Web build or deployment failed: $_" -ForegroundColor Red
            $targetFailed = $true
        }
    }

    if ($targetFailed) {
        Write-Host "ERROR: One or more targets or tests failed during execution." -ForegroundColor Red
        exit 2
    }

    $StateTemp = "$StateFile.new.$PID"
    Set-Content -LiteralPath $StateTemp -Value $BuildState -NoNewline
    Move-Item -LiteralPath $StateTemp -Destination $StateFile -Force

    Write-Host '=================================================='
    Write-Host "Update sequence completed: $(Get-Date)"
    Write-Host '=================================================='
}
catch {
    Write-Host "ERROR: Update failed: $($_.Exception.Message)" -ForegroundColor Red
    exit $(if ($_.Exception.Message -match 'targets failed') { 2 } else { 1 })
}
finally {
    Set-Location -LiteralPath $RepoRoot
    if ($BuildWorktree -and (Test-Path -LiteralPath $BuildWorktree)) {
        try {
            Invoke-Native -Command 'git' -Arguments @('worktree', 'remove', '--force', $BuildWorktree)
        }
        catch {
            Write-Warning "Could not remove temporary build worktree '$BuildWorktree': $($_.Exception.Message)"
        }
    }
    $env:CARGO_TARGET_DIR = $PreviousCargoTargetDir
    Stop-Transcript | Out-Null
}