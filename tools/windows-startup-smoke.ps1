# Start the Windows app under different graphics settings and check each one gets its window up,
# draws a frame and quits cleanly, with no crash report. The CI runner has no GPU: the default case
# shows what eframe's and wgpu's own choice picks there, and the D3D12 cases run on WARP.
#
# Usage: tools/windows-startup-smoke.ps1 -Exe target\ci\solidcraft.exe
param([Parameter(Mandatory = $true)][string]$Exe)
$ErrorActionPreference = 'Stop'

$cases = @(
    @{ Name = 'default (wgpu picks)'; Graphics = $null; Env = @{}; Args = @(); Expect = 'SolidCraft graphics: ' },
    # The default again, after a warm start.
    @{ Name = 'default, warm'; Graphics = $null; Env = @{}; Args = @(); Expect = 'SolidCraft graphics: ' },
    @{ Name = 'saved 8x anti-aliasing'; Graphics = '{"msaa":8}'; Env = @{}; Args = @(); Expect = '(requested 8x)' },
    @{ Name = 'saved 2x, backend preference dx12'; Graphics = '{"msaa":2,"backend":"dx12"}'; Env = @{}; Args = @(); Expect = '(requested 2x)' },
    @{ Name = 'damaged graphics.json'; Graphics = '{"msaa":'; Env = @{}; Args = @(); Expect = 'SolidCraft graphics: ' },
    @{ Name = 'WGPU_BACKEND=dx12 with FXC'; Graphics = $null; Env = @{ WGPU_BACKEND = 'dx12'; WGPU_DX12_COMPILER = 'fxc' }; Args = @(); Expect = 'graphics: Dx12' },
    @{ Name = 'safe mode'; Graphics = '{"msaa":8,"backend":"vulkan"}'; Env = @{}; Args = @('--safe-mode'); Expect = 'graphics: Dx12' },
    # Our own window runner (opt-in): the same app, settings and control channel without eframe.
    @{ Name = 'own window runner'; Graphics = $null; Env = @{ SOLIDCRAFT_WINDOW_RUNNER = 'own' }; Args = @(); Expect = 'runs on its own runner' }
)

$failed = @()
$port = 47811
foreach ($case in $cases) {
    $port += 1
    $root = Join-Path $env:RUNNER_TEMP ("smoke-" + $port)
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    # eframe asks Windows for the roaming folder (not %APPDATA%), so the settings go in the
    # runner's real one, emptied for each case.
    $settings = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'SolidCraft\data'
    Remove-Item -Recurse -Force $settings -ErrorAction SilentlyContinue
    $local = Join-Path $root 'local'
    $crashes = Join-Path $root 'crashes'
    New-Item -ItemType Directory -Force $settings, $local, $crashes | Out-Null
    if ($case.Graphics) { Set-Content -NoNewline -Path (Join-Path $settings 'graphics.json') -Value $case.Graphics }
    $token = -join ((1..64) | ForEach-Object { '{0:x}' -f (Get-Random -Maximum 16) })

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = (Resolve-Path $Exe).Path
    foreach ($a in (@('--sample', '--control', "$port") + $case.Args)) { [void]$psi.ArgumentList.Add($a) }
    $psi.UseShellExecute = $false
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardOutput = $true
    $psi.Environment['LOCALAPPDATA'] = $local
    $psi.Environment['SOLIDCRAFT_CRASH_DIR'] = $crashes
    $psi.Environment['SOLIDCRAFT_CONTROL_TOKEN'] = $token
    $psi.Environment.Remove('WGPU_BACKEND') | Out-Null
    $psi.Environment.Remove('WGPU_DX12_COMPILER') | Out-Null
    foreach ($k in $case.Env.Keys) { $psi.Environment[$k] = $case.Env[$k] }
    Write-Host "=== $($case.Name)"
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $p = [System.Diagnostics.Process]::Start($psi)
    $stderr = $p.StandardError.ReadToEndAsync()
    $stdout = $p.StandardOutput.ReadToEndAsync()

    $ok = $false
    $why = ''
    try {
        $client = $null
        $deadline = (Get-Date).AddSeconds(180)
        while (-not $client -and (Get-Date) -lt $deadline -and -not $p.HasExited) {
            try { $client = New-Object System.Net.Sockets.TcpClient('127.0.0.1', $port) } catch { Start-Sleep -Milliseconds 500 }
        }
        if (-not $client) { throw "no control channel (exited: $($p.HasExited))" }
        $client.ReceiveTimeout = 120000
        $stream = $client.GetStream()
        $writer = New-Object System.IO.StreamWriter($stream)
        $writer.AutoFlush = $true
        $reader = New-Object System.IO.StreamReader($stream)
        $shot = Join-Path $root 'shot.png'
        $writer.WriteLine((@{ id = 1; token = $token; method = 'ui.screenshot'; params = @{ path = $shot } } | ConvertTo-Json -Compress))
        $reply = $reader.ReadLine()
        Write-Host "screenshot: $reply"
        if (-not ($reply -like '*"ok":true*') -or -not (Test-Path $shot)) { throw "no frame drawn: $reply" }
        # Start to the first frame on screen, for comparing startups (control channel up, then a screenshot).
        Write-Host ("startup to first frame: {0} ms" -f $clock.ElapsedMilliseconds)
        $writer.WriteLine((@{ id = 2; method = 'app.quit'; params = @{ force = $true } } | ConvertTo-Json -Compress))
        Write-Host "quit: $($reader.ReadLine())"
        $client.Close()
        if (-not $p.WaitForExit(60000)) { throw 'did not exit after app.quit' }
        if ($p.ExitCode -ne 0) { throw ('exit code 0x{0:x}' -f $p.ExitCode) }
        $ok = $true
    } catch {
        $why = $_.Exception.Message
    } finally {
        if (-not $p.HasExited) { $p.Kill(); $p.WaitForExit() }
    }
    $err = $stderr.Result
    Write-Host $err
    if ($ok -and -not $err.Contains($case.Expect)) { $ok = $false; $why = "stderr does not say '$($case.Expect)'" }
    $reports = @(Get-ChildItem -Recurse -Filter report.json $crashes -ErrorAction SilentlyContinue)
    if ($reports.Count -gt 0) {
        foreach ($r in $reports) { Get-Content $r.FullName | Write-Host }
        if ($ok) { $ok = $false; $why = 'a crash report was written' }
    }
    if ($ok) { Write-Host "PASS: $($case.Name)" } else { Write-Host "FAIL: $($case.Name): $why"; $failed += $case.Name }
}
if ($failed.Count -gt 0) { Write-Error ("failed: " + ($failed -join ', ')) }
