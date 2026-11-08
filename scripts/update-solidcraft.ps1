# C:\solidcraft\update-solidcraft.ps1
param(
    [bool]$BuildGui = $true,
    [bool]$BuildCli = $true,
    [bool]$BuildWeb = $false
)

$RepoDir = "C:\solidcraft"
Set-Location $RepoDir
$TargetWebDir  = "$RepoDir\web-dist"

# Ensure log directory exists
$LogDir = "$RepoDir\build_logs"
if (!(Test-Path $LogDir)) {
    New-Item -ItemType Directory -Path $LogDir | Out-Null
}
$Timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$LogFile = "$LogDir\build_$Timestamp.log"

# Start recording all console input/output to the log file
Start-Transcript -Path $LogFile | Out-Null

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host " Starting SolidCraft Pipeline: $(Get-Date)" -ForegroundColor Cyan
Write-Host " Log File: $LogFile" -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan

# Track status of individual targets
$GlobalSuccess = $true
$GuiSuccess    = $false
$CliSuccess    = $false
$WebSuccess    = $false

function Write-StepHeader($StepNum, $StepName) {
    Write-Host "`n----------------------------------------------------" -ForegroundColor Yellow
    Write-Host " [STEP $StepNum] $StepName" -ForegroundColor Yellow
    Write-Host "----------------------------------------------------" -ForegroundColor Yellow
}

function Write-ProminentError($Message) {
    Write-Host "`n####################################################" -ForegroundColor Red
    Write-Host " [FATAL ERROR]: $Message" -ForegroundColor Red
    Write-Host "####################################################`n" -ForegroundColor Red
}

try {
    # ----------------------------------------------------------------
    Write-StepHeader "1" "Pull Latest Changes from Git"
    # ----------------------------------------------------------------
    git pull
    if ($LASTEXITCODE -ne 0) {
        Write-ProminentError "Git pull failed. Aborting entire update chain."
        $GlobalSuccess = $false
        exit 1
    }
    Write-Host "Git pull completed successfully." -ForegroundColor Green

    # ----------------------------------------------------------------
    Write-StepHeader "2" "Run Workspace Test Suite (Debug Mode)"
    # ----------------------------------------------------------------
    cargo test --workspace --jobs 2
    if ($LASTEXITCODE -ne 0) {
        Write-ProminentError "Tests failed! Aborting deployment. Existing build remains untouched."
        $GlobalSuccess = $false
        exit 1
    }
    Write-Host "All tests passed successfully!" -ForegroundColor Green

    # ----------------------------------------------------------------
    Write-StepHeader "3" "Compile Target: GUI (solidcraft)"
    # ----------------------------------------------------------------
    if ($BuildGui) {
        Write-Host "Building solidcraft (GUI)..."
        cargo build --release -p solidcraft --jobs 2
        if ($LASTEXITCODE -ne 0) {
            Write-ProminentError "GUI compilation failed!"
        } else {
            $GuiSuccess = $true
            Write-Host "GUI compilation succeeded." -ForegroundColor Green
        }
    } else {
        Write-Host "GUI target skipped by parameter." -ForegroundColor DarkGray
    }

    # ----------------------------------------------------------------
    Write-StepHeader "4" "Compile Target: CLI (solidcraft-cli)"
    # ----------------------------------------------------------------
    if ($BuildCli) {
        Write-Host "Building solidcraft-cli (CLI)..."
        cargo build --release -p solidcraft-cli --jobs 2
        if ($LASTEXITCODE -ne 0) {
            Write-ProminentError "CLI compilation failed!"
        } else {
            $CliSuccess = $true
            Write-Host "CLI compilation succeeded." -ForegroundColor Green
        }
    } else {
        Write-Host "CLI target skipped by parameter." -ForegroundColor DarkGray
    }

    # ----------------------------------------------------------------
    Write-StepHeader "5" "Compile Target: Web (solidcraft-web)"
    # ----------------------------------------------------------------
    if ($BuildWeb) {
        Write-Host "Building solidcraft-web (WASM)..."
        cargo build --release -p solidcraft-web --target wasm32-unknown-unknown --jobs 2
        if ($LASTEXITCODE -ne 0) {
            Write-ProminentError "Web compilation failed!"
        } else {
            $WebSuccess = $true
            Write-Host "Web compilation succeeded." -ForegroundColor Green
        }
    } else {
        Write-Host "Web target skipped by parameter." -ForegroundColor DarkGray
    }

    # ----------------------------------------------------------------
    Write-StepHeader "6" "Atomic Deployment for Successful Targets"
    # ----------------------------------------------------------------
    
    # Deploy GUI if built successfully
    if ($BuildGui -and $GuiSuccess) {
        $SourceGui = "$RepoDir\target\release\solidcraft.exe"
        $TargetGui = "$RepoDir\solidcraft.exe"
        $TempGui   = "$RepoDir\solidcraft_new.exe"

        if (Test-Path $SourceGui) {
            Write-Host "Deploying GUI binary atomically..." -ForegroundColor Cyan
            Copy-Item $SourceGui $TempGui -Force
            if (Test-Path $TargetGui) {
                Move-Item $TargetGui "$RepoDir\solidcraft_old.exe" -Force -ErrorAction SilentlyContinue
                Move-Item $TempGui $TargetGui -Force
                Remove-Item "$RepoDir\solidcraft_old.exe" -ErrorAction SilentlyContinue
            } else {
                Move-Item $TempGui $TargetGui -Force
            }
            Write-Host "Successfully deployed solidcraft.exe!" -ForegroundColor Green
        } else {
            Write-ProminentError "GUI binary missing at expected path despite successful build status."
        }
    }

    # Deploy CLI if built successfully
    if ($BuildCli -and $CliSuccess) {
        $SourceCli = "$RepoDir\target\release\solidcraft-cli.exe"
        $TargetCli = "$RepoDir\solidcraft-cli.exe"
        $TempCli   = "$RepoDir\solidcraft-cli_new.exe"

        if (Test-Path $SourceCli) {
            Write-Host "Deploying CLI binary atomically..." -ForegroundColor Cyan
            Copy-Item $SourceCli $TempCli -Force
            if (Test-Path $TargetCli) {
                Move-Item $TargetCli "$RepoDir\solidcraft-cli_old.exe" -Force -ErrorAction SilentlyContinue
                Move-Item $TempCli $TargetCli -Force
                Remove-Item "$RepoDir\solidcraft-cli_old.exe" -ErrorAction SilentlyContinue
            } else {
                Move-Item $TempCli $TargetCli -Force
            }
            Write-Host "Successfully deployed solidcraft-cli.exe!" -ForegroundColor Green
        } else {
            Write-ProminentError "CLI binary missing at expected path despite successful build status."
        }
    }

    # Deploy Web assets if built successfully
    if ($BuildWeb -and $WebSuccess) {
        $SourceWebWasm = "$RepoDir\target\wasm32-unknown-unknown\release\solidcraft_web.wasm"

        if (Test-Path $SourceWebWasm) {
            Write-Host "Deploying web assets..." -ForegroundColor Cyan
            if (!(Test-Path $TargetWebDir)) {
                New-Item -ItemType Directory -Path $TargetWebDir | Out-Null
            }
            Copy-Item $SourceWebWasm "$TargetWebDir\" -Force
            Write-Host "Successfully deployed web assets!" -ForegroundColor Green
        } else {
            Write-ProminentError "Web WASM binary missing at expected path despite successful build status."
        }
    }

    # ----------------------------------------------------------------
    Write-StepHeader "7" "Pipeline Summary Report"
    # ----------------------------------------------------------------
    Write-Host "Pipeline Execution Complete." -ForegroundColor Cyan
    if ($BuildGui) { Write-Host " - GUI Target: $(if($GuiSuccess){'SUCCESS'}else{'FAILED'})" -ForegroundColor $(if($GuiSuccess){'Green'}else{'Red'}) }
    if ($BuildCli) { Write-Host " - CLI Target: $(if($CliSuccess){'SUCCESS'}else{'FAILED'})" -ForegroundColor $(if($CliSuccess){'Green'}else{'Red'}) }
    if ($BuildWeb) { Write-Host " - Web Target: $(if($WebSuccess){'SUCCESS'}else{'FAILED'})" -ForegroundColor $(if($WebSuccess){'Green'}else{'Red'}) }
    Write-Host "Full log saved to: $LogFile" -ForegroundColor Cyan

    if ((($BuildGui -and !$GuiSuccess)) -or (($BuildCli -and !$CliSuccess)) -or (($BuildWeb -and !$WebSuccess))) {
        exit 2 # Non-zero exit to flag partial failures
    }

}
catch {
    Write-ProminentError "Unexpected pipeline exception: $_"
    exit 3
}
finally {
    Stop-Transcript | Out-Null
}
