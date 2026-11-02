# Manager-run local evidence collector. Does not launch/build anything or modify baseline files.
param(
    [Parameter(Mandatory = $true)][int]$Port,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$TokenPath,
    [switch]$Baseline,
    [ValidateRange(1, 1000)][int]$Frames = 30
)
$ErrorActionPreference = 'Stop'
if (-not $TokenPath) { $TokenPath = Join-Path $env:LOCALAPPDATA "SolidCraft/control/$Port.token" }
$blurToken = (Get-Content -LiteralPath $TokenPath -Raw).Trim()
$blurOutput = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $blurOutput) { throw 'Use a new output directory; existing evidence is never overwritten.' }
New-Item -ItemType Directory -Path $blurOutput | Out-Null
$blurClient = [Net.Sockets.TcpClient]::new('127.0.0.1', $Port)
$blurClient.ReceiveTimeout = 60000
$blurClient.SendTimeout = 60000
$blurStream = $blurClient.GetStream()
$blurReader = [IO.StreamReader]::new($blurStream)
$blurWriter = [IO.StreamWriter]::new($blurStream, [Text.UTF8Encoding]::new($false))
$blurWriter.AutoFlush = $true
$script:blurRequestId = 0
function Invoke-BlurControl([string]$Method, $Parameters = @{}) {
    $script:blurRequestId++
    $blurWriter.WriteLine((@{ id = $script:blurRequestId; token = $blurToken; method = $Method; params = $Parameters } | ConvertTo-Json -Compress -Depth 30))
    $blurLine = $blurReader.ReadLine()
    if (-not $blurLine) { throw 'Control connection closed.' }
    $blurReply = $blurLine | ConvertFrom-Json
    if (-not $blurReply.ok) { throw "$Method failed: $($blurReply.error)" }
    return $blurReply.result
}
function Save-BlurShot([string]$Name) {
    Invoke-BlurControl 'ui.screenshot' @{ path = (Join-Path $blurOutput "$Name.png"); allow_paths = @($blurOutput) } | Out-Null
}
try {
    $blurInitial = Invoke-BlurControl 'ui.inspect'
    $blurPrefs = Invoke-BlurControl 'ui.prefs'
    $blurInitial | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $blurOutput 'initial-inspect.json')
    if (-not $Baseline) { Invoke-BlurControl 'ui.blur' @{ radius_px = 0; surface_alpha = $null } | Out-Null }
    # Leave the current scene/theme/size untouched for the main/branch zero comparison.
    $blurTimings = @()
    for ($blurIndex = 0; $blurIndex -lt ($Frames + 5); $blurIndex++) {
        $blurClock = [Diagnostics.Stopwatch]::StartNew()
        # A screenshot forces a presented frame. Reuse only this collector's scratch file.
        Save-BlurShot 'timing-scratch'
        $blurClock.Stop()
        $blurInspect = Invoke-BlurControl 'ui.inspect'
        if ($blurIndex -ge 5) {
            $blurTimings += @{ ui_cpu_ms = $blurInspect.frame_ms; screenshot_roundtrip_ms = $blurClock.Elapsed.TotalMilliseconds }
        }
    }
    $blurReport = @{ machine = $env:COMPUTERNAME; mode = $(if ($Baseline) { 'baseline' } else { 'branch-zero' }); samples = $blurTimings; inspect = $blurInitial }
    if (-not $Baseline) { $blurReport.counters = Invoke-BlurControl 'ui.blur' }
    $blurReport | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $blurOutput 'zero-timing.json')
    Save-BlurShot 'zero-ui'
    Invoke-BlurControl 'ui.render' @{ path = (Join-Path $blurOutput 'zero-model.png'); allow_paths = @($blurOutput) } | Out-Null
    if (-not $Baseline) {
        foreach ($blurTheme in @('light', 'dark')) {
            Invoke-BlurControl 'ui.prefs' @{ theme = $blurTheme } | Out-Null
            foreach ($blurRadius in @(0, 18)) {
                Invoke-BlurControl 'ui.blur' @{ radius_px = $blurRadius; surface_alpha = 0.7 } | Out-Null
                Save-BlurShot "$blurTheme-$blurRadius"
                $blurState = Invoke-BlurControl 'ui.blur'
                $blurState | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $blurOutput "$blurTheme-$blurRadius-counters.json")
                if ($blurRadius -gt 0 -and $blurState.capability -ne 'Available') { throw "Blur unavailable: $($blurState.capability | ConvertTo-Json -Compress)" }
            }
        }
    }
} finally {
    try {
        if (-not $Baseline) { Invoke-BlurControl 'ui.blur' @{ radius_px = 0; surface_alpha = $null } | Out-Null }
        if (-not $Baseline -and $blurPrefs -and $blurPrefs.theme) { Invoke-BlurControl 'ui.prefs' @{ theme = $blurPrefs.theme } | Out-Null }
        if (-not $Baseline -and $blurInitial -and $null -ne $blurInitial.ui.dark) { Invoke-BlurControl 'ui.set' @{ dark = $blurInitial.ui.dark } | Out-Null }
    } finally {
        $blurWriter.Dispose(); $blurReader.Dispose(); $blurClient.Dispose()
    }
}
